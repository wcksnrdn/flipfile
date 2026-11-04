// PDF in the image pipeline: rendering PDF pages to images, and merging several
// images into one multi-page PDF.

use image::DynamicImage;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{mpsc, Mutex};
use tauri::ipc::Channel;

use super::encode::{encode, pdf_page, resize};
use super::input::{file_name, header, open_oriented};
use super::memory;
use super::output::{save_err, unique_output_dir, unique_output_path};
use super::{isolate, par_map, BatchResult, Converted, OnPage, Progress};
use crate::pdf::render as pdf_render;
use crate::pdf::writer::write_jpeg_pdf;
use crate::state::settings::Options;

/// Rendered pages waiting for, or being, encoded at the same time stay within this:
/// all cores for ordinary pages (A4 at 200 DPI is 15 MB), one at a time for posters.
const PAGE_MEMORY: u64 = 256 << 20;

/// A one-page PDF becomes `name_converted.ext`; longer ones get a `name_pages/` folder
/// with `page-01.ext`, `page-02.ext`, … so they stay together and sort correctly.
/// `pdf` is read; results are named after `name_from` (the PDF itself, or the Word
/// document it was made from).
pub(super) fn pdf_to_images(
    pdf: &Path,
    name_from: &Path,
    ext: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let input = name_from;
    let info = pdf_render::page_info(pdf)?;
    let total = info.pages;
    if total == 0 {
        return Err(format!("\"{}\" has no pages", file_name(input)));
    }
    on_page(0, total);
    let save = |img: DynamicImage, path: &Path| encode(&resize(img, options.resize), ext, path, options.quality);

    if total == 1 {
        let output = unique_output_path(input, out_dir, "converted", ext);
        pdf_render::render_pages(pdf, |_, img| save(img, &output))?;
        on_page(1, 1);
        return Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 1, sizes: None, note: None });
    }

    let folder = unique_output_dir(input, out_dir, "pages");
    fs::create_dir_all(&folder).map_err(save_err)?;
    let digits = total.to_string().len().max(2);
    let rendered = render_and_save(pdf, total, info.largest_bitmap, on_page, |i, img| {
        save(img, &folder.join(format!("page-{:0digits$}.{ext}", i + 1)))
    });
    match rendered {
        Ok(pages) => Ok(Converted { path: folder.to_string_lossy().into_owned(), pages, sizes: None, note: None }),
        Err(e) => {
            let _ = fs::remove_dir_all(&folder); // no half-finished folders
            Err(e)
        }
    }
}

/// Renders the pages in order on this thread (PDFium runs one call at a time anyway)
/// and saves them on worker threads, so encoding, the slow part for most formats, uses
/// every core. At most `PAGE_MEMORY` of rendered pages are held at once.
fn render_and_save(
    pdf: &Path,
    total: usize,
    page_bytes: u64,
    on_page: OnPage,
    save: impl Fn(usize, DynamicImage) -> Result<(), String> + Sync,
) -> Result<usize, String> {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    // Each worker holds a page, one waits in the queue and one is being rendered
    let workers = ((PAGE_MEMORY / page_bytes.max(1)).saturating_sub(2) as usize).clamp(1, cores).min(total);
    // Reported in order, whichever worker finishes
    let done = Mutex::new(0);
    let saved = |i: usize, img: DynamicImage| -> Result<(), String> {
        save(i, img)?;
        let mut done = done.lock().unwrap();
        *done += 1;
        on_page(*done, total);
        Ok(())
    };
    if workers == 1 {
        return pdf_render::render_pages(pdf, saved);
    }

    let (queue, pages) = mpsc::sync_channel::<(usize, DynamicImage)>(1);
    let pages = Mutex::new(pages);
    let failed: Mutex<Option<String>> = Mutex::new(None);
    let rendered = std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                // (the queue is only locked while taking a page, not while saving it)
                let next = pages.lock().unwrap().recv();
                let Ok((i, img)) = next else { break };
                if failed.lock().unwrap().is_some() {
                    continue; // drain what is left after a failure
                }
                // A worker that panicked would stop taking pages and leave the renderer
                // waiting on a full queue
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| saved(i, img)))
                    .unwrap_or_else(|_| Err(format!("Page {} could not be saved (internal error)", i + 1)));
                if let Err(e) = result {
                    failed.lock().unwrap().get_or_insert(e);
                }
            });
        }
        let rendered = pdf_render::render_pages(pdf, |i, img| {
            if let Some(e) = failed.lock().unwrap().clone() {
                return Err(e);
            }
            queue.send((i, img)).map_err(|e| e.to_string())
        });
        drop(queue); // the workers finish the queue, then stop
        rendered
    });
    match failed.into_inner().unwrap() {
        Some(e) => Err(e),
        None => rendered,
    }
}

pub(super) fn merge_to_pdf(
    paths: &[String],
    progress: &Channel<Progress>,
    out_dir: Option<&Path>,
    options: &Options,
) -> Result<BatchResult, String> {
    let pages = par_map(
        paths,
        progress,
        |p| memory::cost(p, "jpg"),
        |p| {
            isolate(p, || {
                if pdf_render::is_pdf(&header(Path::new(p))) {
                    return Err(format!("\"{}\": PDFs cannot be merged yet", file_name(Path::new(p))));
                }
                open_oriented(Path::new(p)).and_then(|(img, _)| pdf_page(&resize(img, options.resize), options.quality))
            })
        },
    );

    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for page in pages {
        match page {
            Ok(p) => ok.push(p),
            Err(e) => errors.push(e),
        }
    }
    if ok.is_empty() {
        return Ok(BatchResult { outputs: vec![], errors, pages: 0, bytes_before: 0, bytes_after: 0, note: None });
    }

    // Named after the first file, saved next to it
    let output = unique_output_path(Path::new(&paths[0]), out_dir, "merged", "pdf");
    let mut file = File::create(&output).map(BufWriter::new).map_err(save_err)?;
    write_jpeg_pdf(&ok, &mut file).and_then(|_| file.flush()).map_err(save_err)?;
    Ok(BatchResult {
        outputs: vec![output.to_string_lossy().into_owned()],
        errors,
        pages: 0,
        bytes_before: 0,
        bytes_after: 0,
        note: None,
    })
}
