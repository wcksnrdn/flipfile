// The conversion pipeline behind the `convert_batch` command: checks the grant, picks
// the output folder and settings, converts every file in parallel and reports progress.
//
// - input:  inspecting and decoding input files
// - encode: resizing and writing every image format
// - pdf:    PDF -> images, images -> one merged PDF
// - word:   Word and Excel -> PDF/images, OCR -> Word
// - slides: PowerPoint -> PDF/images/Word/outline, anything -> PowerPoint
// - clean:  lossless metadata removal (with strip)
// - text:   the readable text of any document, for AI summaries (src/summary)
// - tools:  background removal, transforms, square pad, target-size compress, animated GIF
// - transcribe: the speech in a recording or video as subtitles or a transcript
// - compress: the Option/Alt ring's levels (Light to Extreme) for photos, PDFs, video, audio
// - output: unique output paths
// - memory: the memory budget that decides how many files convert at once

#[cfg(test)]
mod bench;
mod clean;
pub mod compress;
mod encode;
mod faces;
mod input;
mod media;
mod memory;
mod office_extra;
mod output;
mod pdf;
mod slides;
#[cfg(test)]
mod stress;
mod strip;
pub mod text;
mod tools;
pub mod transcribe;
mod upscale;
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
use input::{file_name, header, open_oriented, same_format_ext, sheet_kind, word_kind};
use media::{convert_media, is_media_target};
use office_extra::{convert_office, is_extra_office_input, is_office_target};
use output::{save_err, Reservations};
pub(crate) use output::unique_output_path;
use pdf::{merge_to_pdf, pdf_to_images};
use tools::{is_tool, is_upscale, merge_to_gif, run_tool};
use transcribe::{is_transcribe_target, transcribe_file};
use word::{from_sheet, from_word, ocr_to_word};

#[derive(Clone, Serialize)]
pub struct Progress {
    done: usize,
    total: usize,
    unit: &'static str, // "file", "page" while a single PDF is being rendered, "step" while one image is upscaled
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
    let suffix = preset_suffix(preset.as_deref());
    // Runs on a separate thread so the UI never freezes
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = run_batch(&paths, &format, out_dir.as_deref(), &options, suffix, &on_progress);
        // An idle flipfile should not hold on to what the last document needed
        office::typeset::release_caches();
        crate::platform::upscale::release();
        crate::platform::whisper::release();
        result
    })
    .await
    .map_err(|e| e.to_string())??;

    app.state::<Mutex<History>>().lock().unwrap().record(&result.outputs);
    crate::platform::tray::refresh(&app);
    Ok(result)
}

/// Starts what converting `paths` will need while the user is still picking a format:
/// the first Office document or PDF -> Word would otherwise wait 0.1-0.8 s for the
/// computer's fonts to be scanned (once per run; nothing happens for other files).
/// (On a thread of its own: it reads the files' first bytes, which may be on a slow disk.)
pub fn warm_up(paths: &[std::path::PathBuf]) {
    let paths = paths.to_vec();
    let _ = std::thread::Builder::new().name("warm-up".into()).spawn(move || {
        let documents = paths.iter().any(|p| {
            word_kind(p).is_some() || sheet_kind(p).is_some() || slides::slides_kind(p).is_some() || is_extra_office_input(p) || pdf_render::is_pdf(&header(p))
        });
        if documents {
            office::typeset::has_font("");
        }
    });
}

/// Name for same-format results: photo_compressed.jpg / photo_resized.png
fn preset_suffix(preset: Option<&str>) -> &'static str {
    match preset {
        Some("compress") => "compressed",
        Some(_) => "resized",
        None => "converted",
    }
}

/// One drop, without the app around it (grants, output folder, history): what
/// `convert_batch` runs, and what the tests and benchmarks run.
fn run_batch(
    paths: &[String],
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    suffix: &str,
    on_progress: &Channel<Progress>,
) -> Result<BatchResult, String> {
    let _names = Reservations::start();
    let all_images = paths.iter().all(|p| {
        let path = Path::new(p);
        word_kind(path).is_none() && sheet_kind(path).is_none() && !is_extra_office_input(path)
    });
    if format == "pdf" && paths.len() > 1 && all_images {
        return merge_to_pdf(paths, on_progress, out_dir, options);
    }
    if format == "gif-anim" && all_images {
        return merge_to_gif(paths, on_progress, out_dir, options);
    }
    if format == "pptx" && paths.len() > 1 && all_images && !paths.iter().any(|p| pdf_render::is_pdf(&header(Path::new(p)))) {
        return slides::merge_to_pptx(paths, on_progress, out_dir, options);
    }
    // A single PDF can take a while: report its pages instead of the one file
    // (or for an upscale or a transcription, its steps).
    let single = paths.len() == 1;
    let unit = if is_upscale(format) || is_transcribe_target(format) || compress::Level::from_format(format).is_some() { "step" } else { "page" };
    let on_page = |done: usize, total: usize| {
        if single {
            let _ = on_progress.send(Progress { done, total, unit });
        }
    };
    let results = par_map(paths, on_progress, |p| memory::cost(p, format), |p| {
        isolate(p, || convert(p, format, out_dir, options, suffix, &on_page))
    });
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

/// Converting a file runs it through many third-party decoders and encoders: a bug that
/// panics on some unusual file fails that file only, instead of the batch (and the app;
/// hence `panic = "unwind"` for release builds in Cargo.toml).
fn isolate<T>(path: &str, convert: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(convert))
        .unwrap_or_else(|_| Err(format!("\"{}\" could not be converted (internal error)", file_name(Path::new(path)))))
}

/// Runs `f` over all paths on a worker pool (one per core), keeping the input order.
/// Before starting on a file, a worker takes `cost` of it (the memory converting it
/// needs) from the shared budget, so big files run only as many at once as fit.
fn par_map<T: Send>(
    paths: &[String],
    progress: &Channel<Progress>,
    cost: impl Fn(&str) -> u64 + Sync,
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
                let r = {
                    // (reads the file's header: a decoder bug there must not end the worker)
                    let bytes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cost(&paths[i]))).unwrap_or(0);
                    let _memory = memory::Budget::global().take(bytes);
                    let _names = Reservations::start();
                    f(&paths[i])
                };
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
    if is_transcribe_target(format) {
        return transcribe_file(input, format, out_dir, on_page);
    }
    if compress::Level::from_format(format).is_some() {
        return compress::compress_file(input, format, out_dir, on_page);
    }
    if is_media_target(format) {
        return convert_media(input, format, out_dir);
    }
    if let Some(legacy) = slides::slides_kind(input) {
        return slides::from_slides(input, legacy, format, out_dir, options, on_page);
    }
    if format == "pptx" {
        return slides::to_pptx(input, out_dir, options, on_page);
    }
    if is_office_target(format) {
        return convert_office(input, format, out_dir, options, on_page);
    }
    let is_pdf = pdf_render::is_pdf(&header(input));
    if let Some(legacy) = word_kind(input) {
        return from_word(input, legacy, format, out_dir, options, on_page);
    }
    if let Some(kind) = sheet_kind(input) {
        return from_sheet(input, kind, format, out_dir, options, on_page);
    }
    if is_extra_office_input(input) {
        return convert_office(input, format, out_dir, options, on_page);
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
    if is_tool(format) {
        return run_tool(input, format, out_dir, options, on_page);
    }

    let (ext, suffix) = match format {
        SAME_FORMAT => (same_format_ext(input)?, suffix),
        "jpg" | "jpeg" | "png" | "webp" | "heic" | "bmp" | "pdf" | "tiff" | "gif" | "ico" | "icns" | "avif"
        | "tga" | "qoi" | "exr" | "hdr" | "ppm" | "svg" => (format.to_string(), "converted"),
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
