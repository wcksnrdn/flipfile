// The conversion pipeline behind the `convert_batch` command: checks the grant, picks
// the output folder and settings, converts every file in parallel and reports progress.
//
// - input:  inspecting and decoding input files
// - encode: resizing and writing every image format
// - pdf:    PDF -> images, images -> one merged PDF
// - word:   Word -> PDF/images, OCR -> Word
// - clean:  lossless metadata removal (with strip)
// - output: unique output paths

mod clean;
mod encode;
mod input;
mod output;
mod pdf;
mod strip;
mod word;
#[cfg(test)]
mod tests;

use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::office;
use crate::pdf::render as pdf_render;
use crate::state::grants::Grants;
use crate::state::history::History;
use crate::state::settings::{Options, Settings, COMPRESS_QUALITY};
use clean::clean;
use encode::{encode, resize};
use input::{file_name, header, open_oriented, same_format_ext, word_kind};
use output::{save_err, unique_output_path};
use pdf::{merge_to_pdf, pdf_to_images};
use word::{from_word, ocr_to_word};

#[derive(Clone, Serialize)]
pub struct Progress {
    done: usize,
    total: usize,
    unit: &'static str, // "file", or "page" while a single PDF is being rendered
}

/// Reports `(pages done, total pages)` while rendering a PDF.
type OnPage<'a> = &'a (dyn Fn(usize, usize) + Sync);

#[derive(Serialize)]
pub struct BatchResult {
    outputs: Vec<String>,
    errors: Vec<String>,
    pages: usize, // pages rendered from PDFs, for the "12 pages → PNG" message
    // Total input/output size of single-file results, so compressing shows its effect
    bytes_before: u64,
    bytes_after: u64,
    note: Option<String>, // e.g. why a format could not get smaller
}

/// One converted input: a file, or a folder of page images for a multi-page PDF.
struct Converted {
    path: String,
    pages: usize,
    sizes: Option<(u64, u64)>, // input and output bytes, for plain file -> file results
    note: Option<String>,
}

impl Converted {
    fn file(input: &Path, output: &Path) -> Self {
        let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        Converted {
            path: output.to_string_lossy().into_owned(),
            pages: 0,
            sizes: Some((size(input), size(output))),
            note: None,
        }
    }
}

/// Dropping on a size preset (Option/Alt ring) keeps each file's own format.
pub const SAME_FORMAT: &str = "same";

/// Converts every file (in parallel) and reports progress after each one.
/// Several images dropped on PDF are merged into a single multi-page PDF.
#[tauri::command]
pub async fn convert_batch(
    app: AppHandle,
    paths: Vec<String>,
    format: String,
    on_progress: Channel<Progress>,
    to_downloads: bool,
    preset: Option<String>,
) -> Result<BatchResult, String> {
    // Only files the user dropped or pasted (see grants.rs)
    if let Err(denied) = app.state::<Grants>().check(&paths) {
        return Err(format!("\"{}\" was not given to flipfile", file_name(Path::new(denied))));
    }
    // The output folder is decided here, never taken from the UI: pasted clipboard
    // images (whose source is a temporary file) go to Downloads, everything else
    // next to its original.
    let out_dir = if to_downloads {
        Some(app.path().download_dir().map_err(|_| "The Downloads folder could not be found".to_string())?)
    } else {
        None
    };
    let options = app.state::<Mutex<Settings>>().lock().unwrap().options(preset.as_deref());
    // Name for same-format results: photo_compressed.jpg / photo_resized.png
    let suffix = match preset.as_deref() {
        Some("compress") => "compressed",
        Some(_) => "resized",
        None => "converted",
    };
    // Runs on a separate thread so the UI never freezes
    let result = tauri::async_runtime::spawn_blocking(move || {
        let out_dir = out_dir.as_deref();
        let all_images = paths.iter().all(|p| word_kind(Path::new(p)).is_none());
        if format == "pdf" && paths.len() > 1 && all_images {
            merge_to_pdf(&paths, &on_progress, out_dir, &options)
        } else {
            // A single PDF can take a while: report its pages instead of the one file.
            let single = paths.len() == 1;
            let on_page = |done: usize, total: usize| {
                if single {
                    let _ = on_progress.send(Progress { done, total, unit: "page" });
                }
            };
            let results = par_map(&paths, &on_progress, |p| convert(p, &format, out_dir, &options, suffix, &on_page));
            let mut batch = BatchResult {
                outputs: vec![],
                errors: vec![],
                pages: 0,
                bytes_before: 0,
                bytes_after: 0,
                note: None,
            };
            for r in results {
                match r {
                    Ok(c) => {
                        batch.outputs.push(c.path);
                        batch.pages += c.pages;
                        if let Some((before, after)) = c.sizes {
                            batch.bytes_before += before;
                            batch.bytes_after += after;
                        }
                        if batch.note.is_none() {
                            batch.note = c.note;
                        }
                    }
                    Err(e) => batch.errors.push(e),
                }
            }
            Ok(batch)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    app.state::<Mutex<History>>().lock().unwrap().record(&result.outputs);
    crate::platform::tray::refresh(&app);
    Ok(result)
}

/// Runs `f` over all paths on a small worker pool, keeping the input order.
fn par_map<T: Send>(
    paths: &[String],
    progress: &Channel<Progress>,
    f: impl Fn(&str) -> T + Sync,
) -> Vec<T> {
    let total = paths.len();
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).min(total).max(1);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..total).map(|_| None).collect());

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let r = f(&paths[i]);
                results.lock().unwrap()[i] = Some(r);
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                let _ = progress.send(Progress { done: d, total, unit: "file" });
            });
        }
    });

    results.into_inner().unwrap().into_iter().map(|r| r.unwrap()).collect()
}

fn convert(
    path: &str,
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    suffix: &str,
    on_page: OnPage,
) -> Result<Converted, String> {
    let input = Path::new(path);
    let name = file_name(input);
    let is_pdf = pdf_render::is_pdf(&header(input));
    if let Some(legacy) = word_kind(input) {
        return from_word(input, legacy, format, out_dir, options, on_page);
    }
    if format == "docx-ocr" {
        return ocr_to_word(input, is_pdf, out_dir, on_page);
    }
    if format == "docx" {
        if !is_pdf {
            return Err(format!("\"{name}\": only PDF files can become Word documents"));
        }
        let output = unique_output_path(input, out_dir, "converted", "docx");
        let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        let docx = office::from_pdf::pdf_to_docx(pdf_render::pdfium()?, &data, on_page).map_err(|e| format!("\"{name}\": {e}"))?;
        fs::write(&output, docx).map_err(save_err)?;
        return Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: None });
    }
    if is_pdf && matches!(format, "pdf" | "clean" | SAME_FORMAT) {
        return Err(match format {
            "pdf" => format!("\"{name}\" is already a PDF"),
            "clean" => format!("\"{name}\": cleaning PDFs is not supported yet"),
            _ => format!("\"{name}\": release Option/Alt and pick an image format for PDFs"),
        });
    }
    if format == "clean" {
        return clean(input, out_dir).map(|path| Converted { path, pages: 0, sizes: None, note: None });
    }

    let (ext, suffix) = match format {
        SAME_FORMAT => (same_format_ext(input)?, suffix),
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "pdf" | "tiff" | "gif" | "ico" | "avif" => {
            (format.to_string(), "converted")
        }
        _ => return Err("This format is not supported yet".into()),
    };
    let ext = ext.as_str();

    if is_pdf {
        return pdf_to_images(input, input, ext, out_dir, options, on_page);
    }

    let (img, _) = open_oriented(input)?;
    let img = resize(img, options.resize);
    let output = unique_output_path(input, out_dir, suffix, ext);
    encode(&img, ext, &output, options.quality)?;
    let mut converted = Converted::file(input, &output);
    if ext == "bmp" && options.quality <= COMPRESS_QUALITY {
        // BMP stores raw pixels: 24-bit (done for every opaque image) is as small as it gets.
        converted.note = Some("BMP can't be compressed further".into());
    }
    Ok(converted)
}
