use image::codecs::avif::AvifEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgb, RgbImage, RgbaImage};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::grants::Grants;
use crate::history::History;
use crate::settings::{Options, Resize, Settings, COMPRESS_QUALITY};
use crate::pdf::{jpeg_pages_to_pdf, Page};
use crate::office;
use crate::pdf_render;
use crate::strip::{strip_jpeg, strip_png};
use crate::system_codec;

// Measured on a 12 MP photo (release build, 8 cores): speed 6 took 6.3 s, speed 9 took
// 2.7 s for the same file size; speed 10 is faster again but ~20% larger.
const AVIF_SPEED: u8 = 9; // 1 = smallest file, 10 = fastest
// GIF palette quantisation (NeuQuant), 1 = best and slowest. Speed 10 measured 6x faster
// than the default 1 on a 12 MP photo for a ~4% larger file.
const GIF_SPEED: i32 = 10;
const ICO_MAX: u32 = 256; // largest size the ICO format allows

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
    crate::tray::refresh(&app);
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

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("File")
}

/// Decodes an image and applies its EXIF orientation, so phone photos are not
/// saved sideways once the orientation tag is gone.
fn open_oriented(path: &Path) -> Result<(DynamicImage, Orientation), String> {
    let not_image = || format!("\"{}\" is not a supported image", file_name(path));

    // HEIC (iPhone photos) and AVIF are decoded by the OS, already upright.
    if is_heif_family_file(path) {
        let img = system_codec::decode(path).map_err(|e| format!("\"{}\": {e}", file_name(path)))?;
        return Ok((img, Orientation::NoTransforms));
    }

    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|_| not_image())?
        .into_decoder()
        .map_err(|_| not_image())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| not_image())?;
    img.apply_orientation(orientation);
    Ok((img, orientation))
}

fn header(path: &Path) -> [u8; 12] {
    let mut header = [0u8; 12];
    let _ = File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut header));
    header
}

fn is_heif_family_file(path: &Path) -> bool {
    system_codec::is_heif_family(&header(path))
}

/// `Some(false)` for .docx-family files, `Some(true)` for legacy binary .doc, from the
/// file's content *and* extension (other ZIP-based files are not Word documents).
fn word_kind(path: &Path) -> Option<bool> {
    let head = header(path);
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if head.starts_with(b"PK\x03\x04") && matches!(ext.as_str(), "docx" | "docm" | "dotx" | "dotm") {
        Some(false)
    } else if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) && matches!(ext.as_str(), "doc" | "dot") {
        Some(true)
    } else {
        None
    }
}

/// Word document -> PDF (best engine on this computer), or -> images through a PDF.
fn from_word(
    input: &Path,
    legacy: bool,
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let name = file_name(input);
    match format {
        "docx" | "docx-ocr" => Err(format!("\"{name}\" is already a Word document")),
        "pdf" => {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            let engine = office::apps::word_to_pdf(input, &output, legacy).map_err(|e| format!("\"{name}\": {e}"))?;
            // Shown in the status, so it is clear which engine produced the PDF
            let note = match engine {
                office::apps::Engine::Word => "via Word",
                office::apps::Engine::LibreOffice => "via LibreOffice",
                office::apps::Engine::BuiltIn => "built-in engine",
            };
            Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some(note.into()) })
        }
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "tiff" | "gif" | "ico" | "avif" => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let temp = std::env::temp_dir().join(format!("flipfile-word-{}-{nanos}.pdf", std::process::id()));
            let result = office::apps::word_to_pdf(input, &temp, legacy)
                .map_err(|e| format!("\"{name}\": {e}"))
                .and_then(|_| pdf_to_images(&temp, input, format, out_dir, options, on_page));
            let _ = fs::remove_file(&temp);
            result
        }
        _ => Err(format!("\"{name}\": pick PDF or an image format for Word documents")),
    }
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

/// Scanned PDF or photo of a document -> editable Word document, read by the system's
/// text recognition (macOS Vision / Windows OCR)
fn ocr_to_word(input: &Path, is_pdf: bool, out_dir: Option<&Path>, on_page: OnPage) -> Result<Converted, String> {
    let name = file_name(input);
    let mut pages = vec![];
    if is_pdf {
        let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        let pdfium = pdf_render::pdfium()?;
        let document = pdfium.load_pdf_from_byte_slice(&data, None).map_err(|_| format!("\"{name}\": the PDF could not be opened"))?;
        let total = document.pages().len() as usize;
        for (index, page) in document.pages().iter().enumerate() {
            on_page(index, total);
            let (w, h) = (page.width().value, page.height().value);
            // 300 dpi reads small print well; very large pages are capped
            let scale = (300.0 / 72.0f32).min(3500.0 / w.max(h).max(1.0));
            let config = pdfium_render::prelude::PdfRenderConfig::new().scale_page_by_factor(scale);
            let image = page.render_with_config(&config).map_err(|e| format!("\"{name}\": {e}"))?.as_image().map_err(|e| format!("\"{name}\": {e}"))?;
            let scan = image.to_rgba8();
            let lines = office::ocr::recognize(&scan).map_err(|e| format!("\"{name}\": {e}"))?;
            pages.push(office::from_pdf::OcrPage { width: w, height: h, lines, scan: Some(scan) });
        }
        on_page(total, total);
    } else {
        let (img, _) = open_oriented(input)?;
        on_page(0, 1);
        let scan = img.to_rgba8();
        let lines = office::ocr::recognize(&scan).map_err(|e| format!("\"{name}\": {e}"))?;
        // A photo becomes a page as wide as A4, in the photo's proportions
        let width = 595.3;
        let height = width * img.height() as f32 / img.width().max(1) as f32;
        pages.push(office::from_pdf::OcrPage { width, height, lines, scan: Some(scan) });
        on_page(1, 1);
    }
    let docx = office::from_pdf::ocr_to_docx(pages).map_err(|e| format!("\"{name}\": {e}"))?;
    let output = unique_output_path(input, out_dir, "converted", "docx");
    fs::write(&output, docx).map_err(save_err)?;
    Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some("text recognised".into()) })
}

/// The extension to write when keeping a file's own format. HEIC cannot be written,
/// so iPhone photos become JPG, the format every app can open.
fn same_format_ext(input: &Path) -> Result<String, String> {
    let head = header(input);
    if system_codec::is_heif_family(&head) {
        return Ok(if matches!(&head[8..12], b"avif" | b"avis") { "avif" } else { "jpg" }.into());
    }
    let own = input.extension().and_then(|e| e.to_str()).map(str::to_lowercase);
    let format = ImageReader::open(input)
        .and_then(|r| r.with_guessed_format())
        .ok()
        .and_then(|r| r.format())
        .ok_or_else(|| format!("\"{}\" is not a supported image", file_name(input)))?;
    Ok(match format {
        // Keep the user's own spelling (.jpeg vs .jpg)
        ImageFormat::Jpeg => own.filter(|e| e == "jpeg").unwrap_or_else(|| "jpg".into()),
        ImageFormat::Png => "png".into(),
        ImageFormat::WebP => "webp".into(),
        ImageFormat::Bmp => "bmp".into(),
        ImageFormat::Tiff => "tiff".into(),
        ImageFormat::Gif => "gif".into(),
        ImageFormat::Ico => "ico".into(),
        ImageFormat::Avif => "avif".into(),
        // Formats that can be read but not written (e.g. TGA, PNM) become PNG
        _ => "png".into(),
    })
}

/// A one-page PDF becomes `name_converted.ext`; longer ones get a `name_pages/` folder
/// with `page-01.ext`, `page-02.ext`, … so they stay together and sort correctly.
/// `pdf` is read; results are named after `name_from` (the PDF itself, or the Word
/// document it was made from).
fn pdf_to_images(
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

fn save_err(e: impl ToString) -> String {
    format!("Could not save: {}", e.to_string())
}

/// BMP and TIFF store raw pixels, so an alpha channel that is fully opaque only wastes
/// space: 3 bytes per pixel instead of 4 is 25% smaller for an identical image.
fn drop_opaque_alpha(img: &DynamicImage) -> DynamicImage {
    if !img.color().has_alpha() {
        return DynamicImage::ImageRgb8(img.to_rgb8());
    }
    let rgba = img.to_rgba8();
    if rgba.pixels().all(|p| p[3] == u8::MAX) {
        DynamicImage::ImageRgb8(img.to_rgb8())
    } else {
        DynamicImage::ImageRgba8(rgba)
    }
}

/// "Compress" for TIFF: lossless Deflate with the horizontal predictor (both part of
/// the TIFF standard and read by every major app). The `image` crate only writes
/// uncompressed TIFF, so the `tiff` crate is used directly.
fn write_tiff_deflate(img: &DynamicImage, output: &Path) -> Result<(), String> {
    use tiff::encoder::compression::DeflateLevel;
    use tiff::encoder::{colortype, Compression, TiffEncoder};
    use tiff::tags::Predictor;

    let file = File::create(output).map(BufWriter::new).map_err(save_err)?;
    let mut encoder = TiffEncoder::new(file)
        .map_err(save_err)?
        .with_compression(Compression::Deflate(DeflateLevel::Best))
        .with_predictor(Predictor::Horizontal);
    match drop_opaque_alpha(img) {
        DynamicImage::ImageRgb8(rgb) => encoder.write_image::<colortype::RGB8>(rgb.width(), rgb.height(), rgb.as_raw()),
        other => {
            let rgba = other.to_rgba8();
            encoder.write_image::<colortype::RGBA8>(rgba.width(), rgba.height(), rgba.as_raw())
        }
    }
    .map_err(save_err)
}

/// Applies the resize setting. Never upscales.
fn resize(img: DynamicImage, resize: Resize) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let (tw, th) = match resize {
        Resize::Original => return img,
        Resize::Scale(f) => (((w as f32 * f).round() as u32).max(1), ((h as f32 * f).round() as u32).max(1)),
        Resize::MaxEdge(max) if w.max(h) > max => {
            let f = max as f32 / w.max(h) as f32;
            (((w as f32 * f).round() as u32).max(1), ((h as f32 * f).round() as u32).max(1))
        }
        Resize::MaxEdge(_) => return img,
    };
    img.resize_exact(tw, th, FilterType::Lanczos3)
}

// AVIF at the same number looks noticeably better than JPEG, so it is shifted down
// to keep files small while matching the perceived quality.
fn avif_quality(quality: u8) -> u8 {
    quality.saturating_sub(10).max(30)
}

fn encode(img: &DynamicImage, ext: &str, output: &Path, quality: u8) -> Result<(), String> {
    let rgba = || DynamicImage::ImageRgba8(img.to_rgba8());
    let create = || File::create(output).map(BufWriter::new).map_err(save_err);

    match ext {
        "jpg" | "jpeg" => flatten_on_white(img)
            .write_with_encoder(JpegEncoder::new_with_quality(&mut create()?, quality))
            .map_err(save_err),
        // PNG is lossless; "Compress" / "Smaller File" still squeezes it with the slowest,
        // strongest deflate setting.
        "png" if quality <= COMPRESS_QUALITY => img
            .write_with_encoder(PngEncoder::new_with_quality(&mut create()?, CompressionType::Best, PngFilter::Adaptive))
            .map_err(save_err),
        "png" => img.save_with_format(output, ImageFormat::Png).map_err(save_err),
        "webp" => rgba().save_with_format(output, ImageFormat::WebP).map_err(save_err),
        "bmp" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Bmp).map_err(save_err),
        "tiff" if quality <= COMPRESS_QUALITY => write_tiff_deflate(img, output),
        "tiff" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Tiff).map_err(save_err),
        "gif" => {
            let rgba = img.to_rgba8();
            GifEncoder::new_with_speed(&mut create()?, GIF_SPEED)
                .encode(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
                .map_err(save_err)
        }
        "ico" => DynamicImage::ImageRgba8(square_icon(img))
            .save_with_format(output, ImageFormat::Ico)
            .map_err(save_err),
        "avif" => rgba()
            .write_with_encoder(AvifEncoder::new_with_speed_quality(&mut create()?, AVIF_SPEED, avif_quality(quality)))
            .map_err(save_err),
        "pdf" => {
            let page = pdf_page(img, quality)?;
            create()?
                .write_all(&jpeg_pages_to_pdf(&[page]))
                .map_err(save_err)
        }
        _ => unreachable!(),
    }
}

// Icons must be square and at most 256px: fit inside, then center on a transparent canvas.
fn square_icon(img: &DynamicImage) -> RgbaImage {
    let fitted = if img.width() > ICO_MAX || img.height() > ICO_MAX {
        img.resize(ICO_MAX, ICO_MAX, FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let side = fitted.width().max(fitted.height());
    let mut canvas = RgbaImage::new(side, side);
    let x = (side - fitted.width()) / 2;
    let y = (side - fitted.height()) / 2;
    image::imageops::overlay(&mut canvas, &fitted.to_rgba8(), x as i64, y as i64);
    canvas
}

fn pdf_page(img: &DynamicImage, quality: u8) -> Result<Page, String> {
    let rgb = flatten_on_white(img);
    let mut jpeg = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, quality))
        .map_err(save_err)?;
    Ok(Page { jpeg, width: rgb.width(), height: rgb.height() })
}

fn merge_to_pdf(
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

/// Removes metadata (EXIF, GPS, camera info, comments) while keeping the format.
/// JPEG and PNG are cleaned losslessly; other formats are re-encoded.
fn clean(input: &Path, out_dir: Option<&Path>) -> Result<String, String> {
    let name = file_name(input);
    let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
    if system_codec::is_heif_family(&data) {
        return Err(format!("\"{name}\": HEIC/AVIF cannot be cleaned yet; convert it to JPG instead"));
    }
    let format = image::guess_format(&data).map_err(|_| format!("\"{name}\" is not a supported image"))?;
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_else(|| format.extensions_str()[0].to_string());
    let output = unique_output_path(input, out_dir, "clean", &ext);

    // A rotated photo cannot simply lose its orientation tag, so it is re-encoded upright.
    let (img, orientation) = open_oriented(input)?;
    let lossless = match (format, orientation) {
        (ImageFormat::Jpeg, Orientation::NoTransforms) => strip_jpeg(&data),
        (ImageFormat::Png, Orientation::NoTransforms) => strip_png(&data),
        _ => None,
    };

    if let Some(bytes) = lossless {
        fs::write(&output, bytes).map_err(save_err)?;
    } else {
        match format {
            ImageFormat::Jpeg => flatten_on_white(&img)
                .write_with_encoder(JpegEncoder::new_with_quality(
                    &mut File::create(&output).map(BufWriter::new).map_err(save_err)?,
                    95,
                ))
                .map_err(save_err)?,
            f if f.writing_enabled() => img.save_with_format(&output, f).map_err(save_err)?,
            _ => return Err(format!("\"{name}\": this format cannot be cleaned yet")),
        }
    }
    Ok(output.to_string_lossy().into_owned())
}

// Transparent areas get a white background (JPG & PDF have no transparency)
fn flatten_on_white(img: &DynamicImage) -> RgbImage {
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    out
}

/// Like `unique_output_path`, for the folder that holds the pages of a PDF.
fn unique_output_dir(input: &Path, out_dir: Option<&Path>, suffix: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let dir = out_dir.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")));
    let mut candidate = dir.join(format!("{stem}_{suffix}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}_{suffix}_{n}"));
        n += 1;
    }
    candidate
}

/// Results never overwrite an existing file. By default they go next to the input
/// as `name_suffix.ext`; with `out_dir` (pasted clipboard images, whose source is a
/// temporary file) they go there under the plain name `name.ext`.
fn unique_output_path(input: &Path, out_dir: Option<&Path>, suffix: &str, ext: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let (dir, base) = match out_dir {
        Some(dir) => (dir, stem.to_string()),
        None => (input.parent().unwrap_or(Path::new(".")), format!("{stem}_{suffix}")),
    };
    let mut candidate = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{base}_{n}.{ext}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn run(path: &str, format: &str, out_dir: Option<&Path>, options: &Options) -> Result<String, String> {
        convert(path, format, out_dir, options, "converted", &|_, _| {}).map(|c| c.path)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flipfile-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 128, 200])))
    }

    // JPEG with an APP1 EXIF segment carrying Orientation = 6 (rotate 90° clockwise)
    fn rotated_jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut jpeg = Vec::new();
        flatten_on_white(&sample(w, h))
            .write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, 90))
            .unwrap();
        let tiff: &[u8] = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn converts_to_every_format() {
        let dir = temp_dir("formats");
        let src = dir.join("photo.png");
        sample(300, 200).save(&src).unwrap();
        for ext in ["jpg", "jpeg", "png", "webp", "bmp", "tiff", "gif", "ico", "avif", "pdf"] {
            let out = run(src.to_str().unwrap(), ext, None, &Options::default()).unwrap_or_else(|e| panic!("{ext}: {e}"));
            assert!(out.ends_with(&format!("photo_converted.{ext}")), "{out}");
            if !matches!(ext, "pdf" | "avif") {
                image::open(&out).unwrap_or_else(|e| panic!("{ext} unreadable: {e}"));
            }
        }
        // Never overwrites
        assert!(run(src.to_str().unwrap(), "png", None, &Options::default()).unwrap().ends_with("photo_converted_2.png"));
    }

    #[test]
    fn ico_is_square_and_at_most_256() {
        let dir = temp_dir("ico");
        let src = dir.join("wide.png");
        sample(1000, 400).save(&src).unwrap();
        let ico = image::open(run(src.to_str().unwrap(), "ico", None, &Options::default()).unwrap()).unwrap();
        assert_eq!((ico.width(), ico.height()), (256, 256));
    }

    #[test]
    fn applies_exif_orientation() {
        let dir = temp_dir("orient");
        let src = dir.join("phone.jpg");
        fs::write(&src, rotated_jpeg(40, 20)).unwrap();
        let out = image::open(run(src.to_str().unwrap(), "png", None, &Options::default()).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (20, 40));
    }

    #[test]
    fn clean_strips_exif_losslessly() {
        let dir = temp_dir("clean");
        let src = dir.join("plain.jpg");
        let data = rotated_jpeg(40, 20);
        // Same file but without rotation, so the lossless path is taken
        let stripped_once = strip_jpeg(&data).unwrap();
        assert!(stripped_once.len() < data.len());
        assert!(!stripped_once.windows(6).any(|w| w == b"Exif\0\0"));
        fs::write(&src, &stripped_once).unwrap();
        let out = clean(&src, None).unwrap();
        assert!(out.ends_with("plain_clean.jpg"));
        assert_eq!(fs::read(&out).unwrap(), stripped_once);

        // Rotated photo: re-encoded upright, EXIF gone
        let rotated = dir.join("rotated.jpg");
        fs::write(&rotated, &data).unwrap();
        let out = clean(&rotated, None).unwrap();
        let bytes = fs::read(&out).unwrap();
        assert!(!bytes.windows(6).any(|w| w == b"Exif\0\0"));
        let img = image::open(&out).unwrap();
        assert_eq!((img.width(), img.height()), (20, 40));
    }

    #[test]
    fn png_strip_drops_text_chunks() {
        let dir = temp_dir("png");
        let src = dir.join("a.png");
        sample(10, 10).save(&src).unwrap();
        let mut data = fs::read(&src).unwrap();
        // Insert a tEXt chunk right after IHDR (8 sig + 25 IHDR)
        let text = b"Comment\0secret";
        let mut chunk = (text.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(text);
        chunk.extend_from_slice(&[0, 0, 0, 0]); // CRC is not validated by strip_png
        data.splice(33..33, chunk);
        let out = strip_png(&data).unwrap();
        assert!(!out.windows(6).any(|w| w == b"secret"));
        image::load_from_memory(&out).unwrap();
    }

    #[test]
    fn merges_images_into_one_pdf() {
        let dir = temp_dir("merge");
        let mut paths = vec![];
        for (i, (w, h)) in [(100, 50), (60, 90), (80, 80)].into_iter().enumerate() {
            let p = dir.join(format!("img{i}.png"));
            sample(w, h).save(&p).unwrap();
            paths.push(p.to_string_lossy().into_owned());
        }
        paths.push(dir.join("missing.png").to_string_lossy().into_owned());
        let channel = Channel::new(|_| Ok(()));
        let result = merge_to_pdf(&paths, &channel, None, &Options::default()).unwrap();
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.outputs[0].ends_with("img0_merged.pdf"));
        let pdf = fs::read(&result.outputs[0]).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 3"));
        assert_eq!(text.matches("/Type /Page ").count(), 3);
    }

    #[test]
    fn out_dir_uses_plain_names() {
        let dir = temp_dir("outdir");
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let src = dir.join("Clipboard 2026-10-20 at 14.03.12.png");
        sample(20, 20).save(&src).unwrap();
        let a = run(src.to_str().unwrap(), "jpg", Some(&out), &Options::default()).unwrap();
        let b = run(src.to_str().unwrap(), "jpg", Some(&out), &Options::default()).unwrap();
        assert_eq!(Path::new(&a), out.join("Clipboard 2026-10-20 at 14.03.12.jpg"));
        assert_eq!(Path::new(&b), out.join("Clipboard 2026-10-20 at 14.03.12_2.jpg"));
    }

    #[test]
    fn resizes_without_upscaling() {
        let img = sample(400, 200);
        assert_eq!(resize(img.clone(), Resize::Original).dimensions(), (400, 200));
        assert_eq!(resize(img.clone(), Resize::Scale(0.5)).dimensions(), (200, 100));
        assert_eq!(resize(img.clone(), Resize::MaxEdge(100)).dimensions(), (100, 50));
        assert_eq!(resize(img, Resize::MaxEdge(1920)).dimensions(), (400, 200));
    }

    #[test]
    fn lower_quality_gives_smaller_jpeg() {
        let dir = temp_dir("quality");
        let src = dir.join("noise.png");
        DynamicImage::ImageRgb8(RgbImage::from_fn(300, 300, |x, y| Rgb([(x * 7 ^ y * 13) as u8, (x * y) as u8, (x + y) as u8])))
            .save(&src)
            .unwrap();
        let big = run(src.to_str().unwrap(), "jpg", None, &Options { resize: Resize::Original, quality: 95 }).unwrap();
        let small = run(src.to_str().unwrap(), "jpg", None, &Options { resize: Resize::Original, quality: 60 }).unwrap();
        assert!(fs::metadata(&small).unwrap().len() < fs::metadata(&big).unwrap().len());
    }

    // Uses the PDFium library downloaded by `npm run fetch-pdfium`.
    #[test]
    fn renders_pdf_pages_to_images() {
        let dir = temp_dir("pdfrender");
        let pages: Vec<Page> = [(200, 100), (100, 200), (150, 150)]
            .into_iter()
            .map(|(w, h)| pdf_page(&sample(w, h), 90).unwrap())
            .collect();
        let doc = dir.join("doc.pdf");
        fs::write(&doc, jpeg_pages_to_pdf(&pages)).unwrap();
        let one = dir.join("single.pdf");
        fs::write(&one, jpeg_pages_to_pdf(&pages[..1])).unwrap();

        // Multi-page: a folder with one image per page, at 200 DPI (page size is 0.75 pt/px)
        let reported = Mutex::new(vec![]);
        let on_page = |done: usize, total: usize| reported.lock().unwrap().push((done, total));
        let c = convert(doc.to_str().unwrap(), "png", None, &Options::default(), "converted", &on_page).unwrap();
        assert_eq!(*reported.lock().unwrap(), vec![(0, 3), (1, 3), (2, 3), (3, 3)]);
        assert_eq!(c.pages, 3);
        assert!(c.path.ends_with("doc_pages"));
        let first = image::open(Path::new(&c.path).join("page-01.png")).unwrap();
        let expected = (200.0 * 0.75 * 200.0 / 72.0_f32).round() as u32;
        assert!((first.width() as i64 - expected as i64).abs() <= 2, "{} vs {expected}", first.width());
        assert!(first.width() > first.height());
        assert!(Path::new(&c.path).join("page-03.png").exists());

        // Single page: a plain file; resize settings apply to rendered pages too
        let small = Options { resize: Resize::MaxEdge(100), quality: 90 };
        let c = convert(one.to_str().unwrap(), "jpg", None, &small, "converted", &|_, _| {}).unwrap();
        assert!(c.path.ends_with("single_converted.jpg"));
        assert_eq!(image::open(&c.path).unwrap().width(), 100);

        // PDF -> PDF is refused with a clear message
        assert!(convert(doc.to_str().unwrap(), "pdf", None, &Options::default(), "converted", &|_, _| {}).is_err());
    }

    #[test]
    fn preset_drop_keeps_the_format() {
        let dir = temp_dir("same");
        let noisy = |w: u32, h: u32| {
            DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| Rgb([(x * 7 ^ y * 13) as u8, (x * y) as u8, (x + y) as u8])))
        };
        let photo = dir.join("photo.jpeg");
        noisy(400, 300).write_with_encoder(JpegEncoder::new_with_quality(&mut File::create(&photo).unwrap(), 98)).unwrap();
        let shot = dir.join("shot.png");
        noisy(400, 300).save(&shot).unwrap();
        let compress = Options { resize: Resize::Original, quality: COMPRESS_QUALITY };
        let none = &|_: usize, _: usize| {};

        // Compress: same format and spelling, smaller file, sizes reported
        let c = convert(photo.to_str().unwrap(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
        assert!(c.path.ends_with("photo_compressed.jpeg"), "{}", c.path);
        let (before, after) = c.sizes.unwrap();
        assert!(after < before, "{after} >= {before}");

        // PNG stays PNG (lossless, max compression)
        let c = convert(shot.to_str().unwrap(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
        assert!(c.path.ends_with("shot_compressed.png"));
        assert_eq!(image::open(&c.path).unwrap().to_rgb8(), image::open(&shot).unwrap().to_rgb8());

        // Resize preset: same format, smaller dimensions
        let half = Options { resize: Resize::Scale(0.5), quality: 90 };
        let c = convert(shot.to_str().unwrap(), SAME_FORMAT, None, &half, "resized", none).unwrap();
        assert!(c.path.ends_with("shot_resized.png"));
        assert_eq!(image::open(&c.path).unwrap().dimensions(), (200, 150));
    }

    #[test]
    fn bmp_and_tiff_get_as_small_as_their_format_allows() {
        let dir = temp_dir("rawformats");
        let photo = dir.join("photo.png");
        // Opaque, but stored as RGBA (like most screenshots)
        DynamicImage::ImageRgba8(RgbaImage::from_fn(300, 200, |x, y| image::Rgba([(x * 3) as u8, (y * 5) as u8, 90, 255])))
            .save(&photo)
            .unwrap();
        let none = &|_: usize, _: usize| {};
        let plain = Options::default();
        let compress = Options { resize: Resize::Original, quality: COMPRESS_QUALITY };

        // BMP: 24-bit for opaque images (3 bytes per pixel + header), with an honest note on Compress
        let bmp = run(photo.to_str().unwrap(), "bmp", None, &plain).unwrap();
        assert!(fs::metadata(&bmp).unwrap().len() < 300 * 200 * 3 + 1024);
        let c = convert(bmp.as_str(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
        assert!(c.path.ends_with("_compressed.bmp"));
        assert_eq!(c.note.as_deref(), Some("BMP can't be compressed further"));
        assert_eq!(image::open(&c.path).unwrap().to_rgb8(), image::open(&photo).unwrap().to_rgb8());

        // Transparent images keep their alpha channel in BMP
        let clear = dir.join("clear.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 40, image::Rgba([10, 20, 30, 0]))).save(&clear).unwrap();
        let bmp = run(clear.to_str().unwrap(), "bmp", None, &plain).unwrap();
        assert!(image::open(&bmp).unwrap().color().has_alpha());

        // TIFF: Compress uses lossless Deflate, so it is smaller and pixel-identical
        let tif = run(photo.to_str().unwrap(), "tiff", None, &plain).unwrap();
        let small = convert(tif.as_str(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
        assert!(small.path.ends_with("_compressed.tiff"));
        let (before, after) = small.sizes.unwrap();
        assert!(after < before / 2, "{after} vs {before}");
        assert_eq!(image::open(&small.path).unwrap().to_rgb8(), image::open(&photo).unwrap().to_rgb8());
    }

    fn sample_docx(dir: &Path) -> PathBuf {
        use docx_rs::*;
        let docx = Docx::new()
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Quarterly Report").bold().size(36)))
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Revenue grew by twelve percent this quarter.")))
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Page one").add_break(BreakType::Page).add_text("Page two")));
        let path = dir.join("report.docx");
        let mut out = std::io::Cursor::new(Vec::new());
        docx.build().pack(&mut out).unwrap();
        fs::write(&path, out.into_inner()).unwrap();
        path
    }

    #[test]
    fn word_documents_convert_through_the_pipeline() {
        let dir = temp_dir("word");
        let docx = sample_docx(&dir);
        let none = &|_: usize, _: usize| {};
        let plain = Options::default();

        // Word -> PDF (built-in engine in tests)
        let pdf = convert(docx.to_str().unwrap(), "pdf", None, &plain, "converted", none).unwrap();
        assert!(pdf.path.ends_with("report_converted.pdf"));
        assert!(fs::read(&pdf.path).unwrap().starts_with(b"%PDF-"));

        // Word -> images: one per page, named after the Word file
        let png = convert(docx.to_str().unwrap(), "png", None, &plain, "converted", none).unwrap();
        assert_eq!(png.pages, 2);
        assert!(png.path.ends_with("report_pages"));

        // PDF -> Word, and that Word file reads back with the same text
        let back = convert(&pdf.path, "docx", None, &plain, "converted", none).unwrap();
        assert!(back.path.ends_with("report_converted_converted.docx"));
        let model = crate::office::docx::read(&fs::read(&back.path).unwrap()).unwrap();
        let text: String = model
            .sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .filter_map(|b| match b {
                crate::office::docx::Block::Paragraph(p) => Some(
                    p.inlines
                        .iter()
                        .filter_map(|i| match i {
                            crate::office::docx::Inline::Text(t, _) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Quarterly Report"), "{text}");
        assert!(text.contains("Revenue grew by twelve percent this quarter."), "{text}");
        assert!(text.contains("Page two"), "{text}");

        // Clear refusals for conversions that make no sense
        assert!(convert(docx.to_str().unwrap(), "docx", None, &plain, "converted", none).is_err());
        let photo = dir.join("photo.png");
        sample(20, 20).save(&photo).unwrap();
        let err = convert(photo.to_str().unwrap(), "docx", None, &plain, "converted", none).err().unwrap();
        assert!(err.contains("only PDF files can become Word documents"), "{err}");

        // Word OCR: a picture of text becomes an editable Word document (macOS/Windows)
        if cfg!(any(target_os = "macos", windows)) {
            let page = dir.join("scan.png");
            let mut img = RgbImage::from_pixel(900, 300, Rgb([255, 255, 255]));
            // "HELLO" in big block letters: 5x7 dot patterns, 12 px per dot
            let font: [[&str; 7]; 5] = [
                ["X...X", "X...X", "X...X", "XXXXX", "X...X", "X...X", "X...X"],
                ["XXXXX", "X....", "X....", "XXXX.", "X....", "X....", "XXXXX"],
                ["X....", "X....", "X....", "X....", "X....", "X....", "XXXXX"],
                ["X....", "X....", "X....", "X....", "X....", "X....", "XXXXX"],
                [".XXX.", "X...X", "X...X", "X...X", "X...X", "X...X", ".XXX."],
            ];
            for (k, letter) in font.iter().enumerate() {
                for (row, bits) in letter.iter().enumerate() {
                    for (col, bit) in bits.chars().enumerate() {
                        if bit == 'X' {
                            for dy in 0..12 {
                                for dx in 0..12 {
                                    img.put_pixel(60 + k as u32 * 160 + col as u32 * 12 + dx, 60 + row as u32 * 12 + dy, Rgb([0, 0, 0]));
                                }
                            }
                        }
                    }
                }
            }
            img.save(&page).unwrap();
            let done = convert(page.to_str().unwrap(), "docx-ocr", None, &plain, "converted", none).unwrap();
            let model = crate::office::docx::read(&fs::read(&done.path).unwrap()).unwrap();
            let text = format!("{:?}", model.sections);
            // (the blocky hand-drawn O may read as C or 0)
            assert!(text.to_uppercase().contains("HELL"), "{text}");
        }
        // A ZIP that is not a Word document is not treated as one
        let fake = dir.join("archive.docx.zip");
        fs::write(&fake, b"PK\x03\x04 not really").unwrap();
        assert!(word_kind(&fake).is_none());
    }

    /// Word OCR during development:
    /// `FLIPFILE_SCAN=scan.pdf|photo.jpg FLIPFILE_OUT=dir cargo test review_ocr -- --ignored --nocapture`
    /// writes the Word file into `dir` and renders it back as ocr1.png, ocr2.png, …
    #[test]
    #[ignore]
    fn review_ocr() {
        use pdfium_render::prelude::PdfRenderConfig;
        let input = std::env::var("FLIPFILE_SCAN").expect("set FLIPFILE_SCAN");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        let none = &|_: usize, _: usize| {};
        let start = std::time::Instant::now();
        let done = convert(&input, "docx-ocr", Some(&out), &Options::default(), "converted", none).unwrap();
        println!("ocr -> docx: {:.2}s {}", start.elapsed().as_secs_f64(), done.path);
        let pdf = crate::office::docx_to_pdf_builtin(&fs::read(&done.path).unwrap()).unwrap();
        let pdfium = pdf_render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
        for (i, page) in doc.pages().iter().enumerate().take(4) {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(900)).unwrap().as_image().unwrap();
            img.save(out.join(format!("ocr{}.png", i + 1))).unwrap();
        }
        println!("pages: {}", doc.pages().len());
    }

}
