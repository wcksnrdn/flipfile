// Image encoding: resizing, per-format encoders and the small pixel helpers they share
// (alpha handling, white background, square icons, multi-size ICO/ICNS, lossy WEBP,
// oxipng lossless PNG, HEIC, and JPEG pages for PDF).

use image::codecs::avif::AvifEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageFormat, Rgb, RgbImage, RgbaImage};
use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Write};
use std::path::Path;

use super::output::save_err;
use crate::pdf::writer::{jpeg_pages_to_pdf, Page};
use crate::platform::system_codec;
use crate::state::settings::{Resize, COMPRESS_QUALITY};

// Measured on a 12 MP photo (release build, 8 cores): speed 6 took 6.3 s, speed 9 took
// 2.7 s for the same file size; speed 10 is faster again but ~20% larger.
pub(super) const AVIF_SPEED: u8 = 9; // 1 = smallest file, 10 = fastest
// GIF palette quantisation (NeuQuant), 1 = best and slowest. Speed 10 measured 6x faster
// than the default 1 on a 12 MP photo for a ~4% larger file.
pub(super) const GIF_SPEED: i32 = 10;
pub(super) const ICO_MAX: u32 = 256; // largest size the ICO format allows
pub(super) const ICO_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];
pub(super) const ICNS_ENTRIES: &[(&[u8; 4], u32)] = &[
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
];

/// BMP and TIFF store raw pixels, so an alpha channel that is fully opaque only wastes
/// space: 3 bytes per pixel instead of 4 is 25% smaller for an identical image.
pub(super) fn drop_opaque_alpha(img: &DynamicImage) -> DynamicImage {
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
pub(super) fn write_tiff_deflate(img: &DynamicImage, output: &Path) -> Result<(), String> {
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

/// Lossy (or lossless at quality >= 100) WebP encoding via libwebp.
pub(super) fn encode_webp_bytes(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    match drop_opaque_alpha(img) {
        DynamicImage::ImageRgb8(rgb) => {
            let enc = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
            let mem = if quality >= 100 {
                enc.encode_lossless()
            } else {
                enc.encode(f32::from(quality.clamp(1, 100)))
            };
            Ok(mem.to_vec())
        }
        other => {
            let rgba = other.to_rgba8();
            let enc = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
            let mem = if quality >= 100 {
                enc.encode_lossless()
            } else {
                enc.encode(f32::from(quality.clamp(1, 100)))
            };
            Ok(mem.to_vec())
        }
    }
}

/// Lossless PNG optimisation via `oxipng`.
pub(super) fn encode_png_oxipng(img: &DynamicImage, output: &Path) -> Result<(), String> {
    let mut raw = Vec::new();
    img.write_with_encoder(PngEncoder::new_with_quality(
        &mut raw,
        CompressionType::Best,
        PngFilter::Adaptive,
    ))
    .map_err(save_err)?;
    let mut opts = oxipng::Options::from_preset(2);
    opts.strip = oxipng::StripChunks::Safe;
    let optimized = oxipng::optimize_from_memory(&raw, &opts).unwrap_or(raw);
    fs::write(output, optimized).map_err(save_err)
}

/// Multi-resolution Windows `.ico` file containing standard icon sizes up to 256×256.
pub(super) fn write_ico_multisize(img: &DynamicImage, output: &Path) -> Result<(), String> {
    let base = square_icon(img);
    let max_side = base.width().min(ICO_MAX);
    let mut sizes: Vec<u32> = ICO_SIZES.iter().copied().filter(|&s| s < max_side).collect();
    sizes.push(max_side);

    let mut rendered: Vec<(u32, RgbaImage)> = Vec::with_capacity(sizes.len());
    for s in sizes {
        let frame_img = if s == base.width() {
            base.clone()
        } else {
            image::imageops::resize(&base, s, s, FilterType::Lanczos3)
        };
        rendered.push((s, frame_img));
    }

    let mut frames: Vec<IcoFrame<'_>> = Vec::with_capacity(rendered.len());
    for (s, rgba) in &rendered {
        frames.push(IcoFrame::as_png(rgba.as_raw(), *s, *s, ExtendedColorType::Rgba8).map_err(save_err)?);
    }
    let file = File::create(output).map(BufWriter::new).map_err(save_err)?;
    IcoEncoder::new(file).encode_images(&frames).map_err(save_err)
}

/// Apple `.icns` icon bundle containing PNG slices from 16×16 up to 1024×1024.
pub(super) fn write_icns(img: &DynamicImage, output: &Path) -> Result<(), String> {
    let base = square_canvas(img, 1024);
    let max_side = base.width();
    let mut chunks: Vec<([u8; 4], Vec<u8>)> = Vec::new();

    for &(ostype, size) in ICNS_ENTRIES {
        if size > max_side && !chunks.is_empty() {
            continue;
        }
        let target = size.min(max_side);
        let resized = if target == base.width() {
            base.clone()
        } else {
            image::imageops::resize(&base, target, target, FilterType::Lanczos3)
        };
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(resized)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .map_err(save_err)?;
        chunks.push((*ostype, png));
    }

    let total_len: u32 = 8 + chunks.iter().map(|(_, d)| 8 + d.len() as u32).sum::<u32>();
    let mut out = Vec::with_capacity(total_len as usize);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&total_len.to_be_bytes());
    for (ostype, png) in chunks {
        let chunk_len = 8 + png.len() as u32;
        out.extend_from_slice(&ostype);
        out.extend_from_slice(&chunk_len.to_be_bytes());
        out.extend_from_slice(&png);
    }
    fs::write(output, out).map_err(save_err)
}

/// Writes an `.svg` wrapper around a lossless PNG data URI so any image or icon can be
/// embedded in SVG tools and web pages at its exact viewBox dimensions.
pub(super) fn write_svg(img: &DynamicImage, output: &Path) -> Result<(), String> {
    let mut png = Vec::new();
    drop_opaque_alpha(img)
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(save_err)?;
    let b64 = base64_encode(&png);
    let (w, h) = (img.width(), img.height());
    let svg = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">
  <image width="{w}" height="{h}" href="data:image/png;base64,{b64}"/>
</svg>
"#
    );
    fs::write(output, svg).map_err(save_err)
}

pub(super) fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Applies the resize setting. Never upscales.
pub(super) fn resize(img: DynamicImage, resize: Resize) -> DynamicImage {
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
pub(super) fn avif_quality(quality: u8) -> u8 {
    quality.saturating_sub(10).max(30)
}

pub(super) fn encode(img: &DynamicImage, ext: &str, output: &Path, quality: u8) -> Result<(), String> {
    let rgba = || DynamicImage::ImageRgba8(img.to_rgba8());
    let create = || File::create(output).map(BufWriter::new).map_err(save_err);

    match ext {
        "jpg" | "jpeg" => flatten_on_white(img)
            .write_with_encoder(JpegEncoder::new_with_quality(&mut create()?, quality))
            .map_err(save_err),
        // PNG is lossless; "Compress" / "Smaller File" squeezes it further with oxipng.
        "png" if quality <= COMPRESS_QUALITY => encode_png_oxipng(img, output),
        "png" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Png).map_err(save_err),
        "webp" => {
            let bytes = encode_webp_bytes(img, quality)?;
            fs::write(output, bytes).map_err(save_err)
        }
        "heic" => system_codec::encode_heic(img, quality, output),
        "bmp" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Bmp).map_err(save_err),
        "tiff" if quality <= COMPRESS_QUALITY => write_tiff_deflate(img, output),
        "tiff" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Tiff).map_err(save_err),
        "gif" => {
            let rgba = img.to_rgba8();
            GifEncoder::new_with_speed(&mut create()?, GIF_SPEED)
                .encode(rgba.as_raw(), rgba.width(), rgba.height(), ExtendedColorType::Rgba8)
                .map_err(save_err)
        }
        "ico" => write_ico_multisize(img, output),
        "icns" => write_icns(img, output),
        "tga" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Tga).map_err(save_err),
        "qoi" => drop_opaque_alpha(img).save_with_format(output, ImageFormat::Qoi).map_err(save_err),
        "exr" => DynamicImage::ImageRgba32F(img.to_rgba32f())
            .save_with_format(output, ImageFormat::OpenExr)
            .map_err(save_err),
        "hdr" => DynamicImage::ImageRgb32F(img.to_rgb32f())
            .save_with_format(output, ImageFormat::Hdr)
            .map_err(save_err),
        "ppm" => DynamicImage::ImageRgb8(img.to_rgb8())
            .save_with_format(output, ImageFormat::Pnm)
            .map_err(save_err),
        "svg" => write_svg(img, output),
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

/// Centers `img` on a square transparent canvas of at most `max_side` px.
pub(super) fn square_canvas(img: &DynamicImage, max_side: u32) -> RgbaImage {
    let fitted = if img.width() > max_side || img.height() > max_side {
        img.resize(max_side, max_side, FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let side = fitted.width().max(fitted.height()).max(1);
    let mut canvas = RgbaImage::new(side, side);
    let x = (side - fitted.width()) / 2;
    let y = (side - fitted.height()) / 2;
    image::imageops::overlay(&mut canvas, &fitted.to_rgba8(), x as i64, y as i64);
    canvas
}

// Icons must be square and at most 256px: fit inside, then center on a transparent canvas.
pub(super) fn square_icon(img: &DynamicImage) -> RgbaImage {
    square_canvas(img, ICO_MAX)
}

pub(super) fn pdf_page(img: &DynamicImage, quality: u8) -> Result<Page, String> {
    let rgb = flatten_on_white(img);
    let mut jpeg = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, quality))
        .map_err(save_err)?;
    Ok(Page { jpeg, width: rgb.width(), height: rgb.height() })
}

// Transparent areas get a white background (JPG & PDF have no transparency)
pub(super) fn flatten_on_white(img: &DynamicImage) -> RgbImage {
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    out
}
