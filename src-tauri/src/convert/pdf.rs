// PDF in the image pipeline: rendering PDF pages to images, and merging several
// images into one multi-page PDF.

use image::DynamicImage;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tauri::ipc::Channel;

use super::encode::{encode, pdf_page, resize};
use super::input::{file_name, header, open_oriented};
use super::output::{save_err, unique_output_dir, unique_output_path};
use super::{par_map, BatchResult, Converted, OnPage, Progress};
use crate::pdf::render as pdf_render;
use crate::pdf::writer::jpeg_pages_to_pdf;
use crate::state::settings::Options;

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
    let total = pdf_render::page_count(pdf)?;
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
    let rendered = pdf_render::render_pages(pdf, |i, img| {
        save(img, &folder.join(format!("page-{:0digits$}.{ext}", i + 1)))?;
        on_page(i + 1, total);
        Ok(())
    });
    match rendered {
        Ok(pages) => Ok(Converted { path: folder.to_string_lossy().into_owned(), pages, sizes: None, note: None }),
        Err(e) => {
            let _ = fs::remove_dir_all(&folder); // no half-finished folders
            Err(e)
        }
    }
}

pub(super) fn merge_to_pdf(
    paths: &[String],
    progress: &Channel<Progress>,
    out_dir: Option<&Path>,
    options: &Options,
) -> Result<BatchResult, String> {
    let pages = par_map(paths, progress, |p| {
        if pdf_render::is_pdf(&header(Path::new(p))) {
            return Err(format!("\"{}\": PDFs cannot be merged yet", file_name(Path::new(p))));
        }
        open_oriented(Path::new(p)).and_then(|(img, _)| pdf_page(&resize(img, options.resize), options.quality))
    });

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
    File::create(&output)
        .and_then(|mut f| f.write_all(&jpeg_pages_to_pdf(&ok)))
        .map_err(save_err)?;
    Ok(BatchResult {
        outputs: vec![output.to_string_lossy().into_owned()],
        errors,
        pages: 0,
        bytes_before: 0,
        bytes_after: 0,
        note: None,
    })
}
