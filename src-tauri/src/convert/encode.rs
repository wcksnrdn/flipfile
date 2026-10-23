// Image encoding: resizing, per-format encoders and the small pixel helpers they share
// (alpha handling, white background, square icons, JPEG pages for PDF).

use image::codecs::avif::AvifEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage, RgbaImage};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use super::output::save_err;
use crate::pdf::writer::{jpeg_pages_to_pdf, Page};
use crate::state::settings::{Resize, COMPRESS_QUALITY};

// Measured on a 12 MP photo (release build, 8 cores): speed 6 took 6.3 s, speed 9 took
// 2.7 s for the same file size; speed 10 is faster again but ~20% larger.
pub(super) const AVIF_SPEED: u8 = 9; // 1 = smallest file, 10 = fastest
// GIF palette quantisation (NeuQuant), 1 = best and slowest. Speed 10 measured 6x faster
// than the default 1 on a 12 MP photo for a ~4% larger file.
pub(super) const GIF_SPEED: i32 = 10;
pub(super) const ICO_MAX: u32 = 256; // largest size the ICO format allows

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
pub(super) fn square_icon(img: &DynamicImage) -> RgbaImage {
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
