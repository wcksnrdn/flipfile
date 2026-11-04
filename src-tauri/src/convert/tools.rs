// Photo room tools: background removal, geometric transforms (rotate, flip, grayscale),
// 1:1 square padding, lossless and target-size compression, and animated GIF creation.

use image::codecs::gif::{GifEncoder, Repeat};
use image::metadata::Orientation;
use image::{Delay, DynamicImage, Frame, ImageFormat, Rgb, RgbImage, RgbaImage};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;
use tauri::ipc::Channel;

use super::encode::{
    encode, encode_avif, encode_jpeg, encode_png_oxipng, encode_webp_bytes, flatten_on_white, has_transparency, resize,
    resize_exact, resize_to_fit, write_tiff_deflate, GIF_SPEED, JPEG_FULL_CHROMA,
};
use super::input::{file_name, header, open_oriented, same_format_ext};
use super::memory;
use super::output::{save_err, unique_output_path};
use super::strip::strip_jpeg;
use super::{isolate, par_map, BatchResult, Converted, Progress};
use crate::pdf::render as pdf_render;
use crate::platform::background;
use crate::state::settings::{Options, Resize, COMPRESS_QUALITY};

/// The largest square the Square tool makes (a 1 GB canvas); bigger images are scaled
/// down to fit.
pub(super) const SQUARE_MAX: u32 = 16_384;

/// True if `format` is one of the Photo tools handled by this module.
pub(super) fn is_tool(format: &str) -> bool {
    matches!(
        format,
        "nobg"
            | "rot-r"
            | "rot-l"
            | "rot-180"
            | "flip-h"
            | "flip-v"
            | "grayscale"
            | "square"
            | "gif-anim"
            | "cmp-auto"
            | "cmp-lossless"
            | "cmp-1m"
            | "cmp-500k"
            | "cmp-200k"
            | "cmp-100k"
    )
}

/// Runs a single-file photo tool on `input`.
pub(super) fn run_tool(
    input: &Path,
    tool: &str,
    out_dir: Option<&Path>,
    options: &Options,
) -> Result<Converted, String> {
    let name = file_name(input);
    if pdf_render::is_pdf(&header(input)) {
        return Err(format!("\"{name}\": photo tools only work on image files"));
    }

    match tool {
        "nobg" => remove_bg_file(input, out_dir, options),
        "rot-r" | "rot-l" | "rot-180" | "flip-h" | "flip-v" | "grayscale" => {
            transform_file(input, tool, out_dir, options)
        }
        "square" => square_file(input, out_dir, options),
        "cmp-auto" => compress_auto_file(input, out_dir, options),
        "cmp-lossless" => compress_lossless_file(input, out_dir, options),
        "cmp-1m" => compress_target_file(input, 1_000_000, "1mb", out_dir, options),
        "cmp-500k" => compress_target_file(input, 500_000, "500kb", out_dir, options),
        "cmp-200k" => compress_target_file(input, 200_000, "200kb", out_dir, options),
        "cmp-100k" => compress_target_file(input, 100_000, "100kb", out_dir, options),
        _ => Err("Unknown photo tool".into()),
    }
}

/// Removes the background via the OS Vision engine and saves a transparent PNG.
fn remove_bg_file(input: &Path, out_dir: Option<&Path>, options: &Options) -> Result<Converted, String> {
    let (img, _) = open_oriented(input)?;
    let img = resize(img, options.resize);
    let cutout = background::remove_background(&img.to_rgba8())?;
    let output = unique_output_path(input, out_dir, "nobg", "png");
    DynamicImage::ImageRgba8(cutout)
        .save_with_format(&output, ImageFormat::Png)
        .map_err(save_err)?;
    Ok(Converted::file(input, &output))
}

/// Rotates, flips, or converts to grayscale while keeping the file's own format.
fn transform_file(input: &Path, tool: &str, out_dir: Option<&Path>, options: &Options) -> Result<Converted, String> {
    let ext = same_format_ext(input)?;
    let (img, _) = open_oriented(input)?;
    let img = resize(img, options.resize);
    let (transformed, suffix) = match tool {
        "rot-r" => (img.rotate90(), "rotated"),
        "rot-l" => (img.rotate270(), "rotated"),
        "rot-180" => (img.rotate180(), "rotated"),
        "flip-h" => (img.fliph(), "flipped"),
        "flip-v" => (img.flipv(), "flipped"),
        "grayscale" => (img.grayscale(), "grayscale"),
        _ => unreachable!(),
    };
    let output = unique_output_path(input, out_dir, suffix, &ext);
    encode(&transformed, &ext, &output, options.quality)?;
    Ok(Converted::file(input, &output))
}

/// Pads an image to a 1:1 square aspect ratio without cropping any pixels.
/// Formats without alpha (JPG, BMP) get a clean white backdrop; others stay transparent.
fn square_file(input: &Path, out_dir: Option<&Path>, options: &Options) -> Result<Converted, String> {
    let ext = same_format_ext(input)?;
    let (img, _) = open_oriented(input)?;
    let mut img = resize(img, options.resize);
    // A 70 000 px panorama would otherwise need a 20 GB canvas
    let mut note = None;
    if img.width().max(img.height()) > SQUARE_MAX {
        img = resize_to_fit(&img, SQUARE_MAX, SQUARE_MAX);
        note = Some(format!("Scaled to {SQUARE_MAX} px"));
    }
    let (w, h) = (img.width(), img.height());
    let side = w.max(h).max(1);
    let (x, y) = ((side - w) / 2, (side - h) / 2);
    let canvas = if matches!(ext.as_str(), "jpg" | "jpeg" | "bmp" | "pdf") {
        // Opaque: 3 bytes per pixel, the photo's rows copied onto white
        let rgb = flatten_on_white(&img);
        let mut canvas = RgbImage::from_pixel(side, side, Rgb([255, 255, 255]));
        let row = w as usize * 3;
        for (i, line) in rgb.as_raw().chunks_exact(row).enumerate() {
            let at = ((y as usize + i) * side as usize + x as usize) * 3;
            canvas.as_mut()[at..at + row].copy_from_slice(line);
        }
        DynamicImage::ImageRgb8(canvas)
    } else {
        let mut canvas = RgbaImage::new(side, side);
        image::imageops::overlay(&mut canvas, &img.to_rgba8(), i64::from(x), i64::from(y));
        DynamicImage::ImageRgba8(canvas)
    };

    let output = unique_output_path(input, out_dir, "square", &ext);
    encode(&canvas, &ext, &output, options.quality)?;
    let mut converted = Converted::file(input, &output);
    converted.note = note;
    Ok(converted)
}

/// Smart auto-compression in the image's own format at `COMPRESS_QUALITY` (65%).
fn compress_auto_file(input: &Path, out_dir: Option<&Path>, options: &Options) -> Result<Converted, String> {
    let ext = same_format_ext(input)?;
    let (img, _) = open_oriented(input)?;
    let img = resize(img, options.resize);
    let output = unique_output_path(input, out_dir, "compressed", &ext);
    let quality = COMPRESS_QUALITY.min(options.quality);
    encode(&img, &ext, &output, quality)?;
    let mut converted = Converted::file(input, &output);
    if ext == "bmp" {
        converted.note = Some("BMP can't be compressed further".into());
    }
    Ok(converted)
}

/// Lossless optimisation: strips JPEG metadata without re-encoding, runs `oxipng` on PNG,
/// writes lossless WebP, or Deflate-compresses TIFF.
fn compress_lossless_file(input: &Path, out_dir: Option<&Path>, options: &Options) -> Result<Converted, String> {
    let ext = same_format_ext(input)?;
    let output = unique_output_path(input, out_dir, "lossless", &ext);
    let raw_input = fs::read(input).ok();
    let (img, orientation) = open_oriented(input)?;
    let unresized = options.resize == Resize::Original && orientation == Orientation::NoTransforms;

    if unresized {
        if let Some(ref data) = raw_input {
            match image::guess_format(data).ok() {
                Some(ImageFormat::Jpeg) => {
                    if let Some(stripped) = strip_jpeg(data) {
                        let best = if stripped.len() < data.len() { &stripped } else { data };
                        fs::write(&output, best).map_err(save_err)?;
                        return Ok(Converted::file(input, &output));
                    }
                }
                Some(ImageFormat::Png) => {
                    let mut opts = oxipng::Options::from_preset(3);
                    opts.strip = oxipng::StripChunks::Safe;
                    let optimized = oxipng::optimize_from_memory(data, &opts).unwrap_or_else(|_| data.clone());
                    let best = if optimized.len() < data.len() { &optimized } else { data };
                    fs::write(&output, best).map_err(save_err)?;
                    return Ok(Converted::file(input, &output));
                }
                _ => {}
            }
        }
    }

    let img = resize(img, options.resize);
    match ext.as_str() {
        "png" => encode_png_oxipng(&img, &output)?,
        "webp" => {
            let bytes = encode_webp_bytes(&img, 100, false)?;
            fs::write(&output, bytes).map_err(save_err)?;
        }
        "tiff" => write_tiff_deflate(&img, &output)?,
        _ => encode(&img, &ext, &output, 95)?,
    }
    Ok(Converted::file(input, &output))
}

/// Encodes `img` into memory in a lossy format (`jpg`, `webp`, or `avif`) at `quality`.
/// `trial`: only to find the quality that fits, where WEBP can use its faster method.
fn encode_lossy_mem(img: &DynamicImage, ext: &str, quality: u8, trial: bool) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    match ext {
        "webp" => return encode_webp_bytes(img, quality, trial),
        "avif" => encode_avif(img, quality, &mut buf)?,
        _ => encode_jpeg(img, quality, &mut buf)?,
    }
    Ok(buf)
}

/// Compresses an image so the output file is at most `target_bytes` (e.g. 1 MB, 500 KB, 200 KB, 100 KB).
/// Uses binary search over quality first, then progressively scales dimensions if needed.
fn compress_target_file(
    input: &Path,
    target_bytes: usize,
    suffix: &str,
    out_dir: Option<&Path>,
    options: &Options,
) -> Result<Converted, String> {
    let own_ext = same_format_ext(input)?;
    let (img, _) = open_oriented(input)?;
    let img = resize(img, options.resize);

    // If the input is a PNG that already fits after oxipng, keep it as PNG.
    if own_ext == "png" {
        let tmp = unique_output_path(input, out_dir, suffix, "png");
        encode_png_oxipng(&img, &tmp)?;
        let len = fs::metadata(&tmp).map(|m| m.len() as usize).unwrap_or(usize::MAX);
        if len <= target_bytes {
            return Ok(Converted::file(input, &tmp));
        }
        let _ = fs::remove_file(&tmp);
    }

    // Lossless formats (PNG/BMP/TIFF/GIF) switch to WEBP (if transparent) or JPG to hit tight byte budgets.
    let (ext, switched) = match own_ext.as_str() {
        "jpg" | "jpeg" | "webp" | "avif" => (own_ext.as_str(), false),
        _ if has_transparency(&img) => ("webp", true),
        _ => ("jpg", true),
    };
    // JPEG keeps colour at half resolution below `JPEG_FULL_CHROMA`, which is where a
    // byte budget is best spent
    let max_q = if matches!(ext, "jpg" | "jpeg") { JPEG_FULL_CHROMA - 1 } else { 92 };

    let mut current = img;
    let mut best_bytes: Option<Vec<u8>> = None;

    for round in 0..7 {
        if round > 0 {
            let (w, h) = (current.width(), current.height());
            let nw = ((w as f32 * 0.75).round() as u32).max(32);
            let nh = ((h as f32 * 0.75).round() as u32).max(32);
            if nw == w && nh == h {
                break;
            }
            current = resize_exact(&current, nw, nh);
        }

        // Fast check: does minimum reasonable quality at this resolution fit?
        let min_q = 15u8;
        let min_attempt = encode_lossy_mem(&current, ext, min_q, true)?;
        if min_attempt.len() > target_bytes {
            best_bytes = Some(min_attempt);
            continue;
        }

        // Binary search for the highest quality in min_q..=max_q that fits in `target_bytes`.
        let (mut lo, mut hi) = (min_q, max_q);
        let (mut fit, mut fit_q) = (min_attempt, min_q);
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let candidate = encode_lossy_mem(&current, ext, mid, true)?;
            if candidate.len() <= target_bytes {
                (fit, fit_q) = (candidate, mid);
                lo = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                hi = mid - 1;
            }
        }
        // The quality was found with the fast trial encoder; the real one usually makes
        // the file smaller still (and better looking), but must fit too
        if ext == "webp" {
            let final_bytes = encode_lossy_mem(&current, ext, fit_q, false)?;
            if final_bytes.len() <= target_bytes {
                fit = final_bytes;
            }
        }
        best_bytes = Some(fit);
        break;
    }

    let bytes = best_bytes.ok_or_else(|| "Could not compress image".to_string())?;
    let output = unique_output_path(input, out_dir, suffix, ext);
    fs::write(&output, &bytes).map_err(save_err)?;
    let mut converted = Converted::file(input, &output);
    if switched {
        converted.note = Some(format!("Saved as {}", ext.to_uppercase()));
    }
    Ok(converted)
}

/// Combines one or more images into an animated GIF (`<first>_animated.gif`).
pub(super) fn merge_to_gif(
    paths: &[String],
    progress: &Channel<Progress>,
    out_dir: Option<&Path>,
    options: &Options,
) -> Result<BatchResult, String> {
    let decoded = par_map(paths, progress, |p| memory::cost(p, "gif-anim"), |p| {
        isolate(p, || {
            let path = Path::new(p);
            if pdf_render::is_pdf(&header(path)) {
                return Err(format!("\"{}\": PDFs cannot be turned into animated GIFs", file_name(path)));
            }
            let (img, _) = open_oriented(path)?;
            let effective_resize = match options.resize {
                Resize::Original => Resize::MaxEdge(720),
                other => other,
            };
            Ok(resize(img, effective_resize))
        })
    });

    let mut images = Vec::new();
    let mut errors = Vec::new();
    for item in decoded {
        match item {
            Ok(img) => images.push(img),
            Err(e) => errors.push(e),
        }
    }
    if images.is_empty() {
        return Ok(BatchResult {
            outputs: vec![],
            errors,
            pages: 0,
            bytes_before: 0,
            bytes_after: 0,
            note: None,
        });
    }

    let (canvas_w, canvas_h) = (images[0].width().max(1), images[0].height().max(1));
    let delay = Delay::from_numer_denom_ms(400, 1);
    let output = unique_output_path(Path::new(&paths[0]), out_dir, "animated", "gif");

    let file = File::create(&output).map(BufWriter::new).map_err(save_err)?;
    let mut encoder = GifEncoder::new_with_speed(file, GIF_SPEED);
    encoder.set_repeat(Repeat::Infinite).map_err(save_err)?;

    for img in &images {
        let fitted = if img.width() != canvas_w || img.height() != canvas_h {
            let scaled = resize_to_fit(img, canvas_w, canvas_h);
            let mut canvas = RgbaImage::new(canvas_w, canvas_h);
            let x = (canvas_w - scaled.width()) / 2;
            let y = (canvas_h - scaled.height()) / 2;
            image::imageops::overlay(&mut canvas, &scaled.to_rgba8(), i64::from(x), i64::from(y));
            canvas
        } else {
            img.to_rgba8()
        };
        let frame = Frame::from_parts(fitted, 0, 0, delay);
        encoder.encode_frame(frame).map_err(save_err)?;
    }
    drop(encoder);

    Ok(BatchResult {
        outputs: vec![output.to_string_lossy().into_owned()],
        errors,
        pages: 0,
        bytes_before: 0,
        bytes_after: 0,
        note: Some(format!("{} frames", images.len())),
    })
}
