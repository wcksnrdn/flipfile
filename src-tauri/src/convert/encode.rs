// Image encoding: resizing, per-format encoders and the small pixel helpers they share
// (alpha handling, white background, square icons, multi-size ICO/ICNS, lossy WEBP,
// oxipng lossless PNG, HEIC, and JPEG pages for PDF).
//
// Pixels are borrowed whenever an encoder can take them as they are (most photos are
// 8-bit RGB, most screenshots opaque 8-bit RGBA), so converting a big image holds one
// copy of it instead of two or three.

use fast_image_resize as fr;
use image::codecs::avif::AvifEncoder;
use image::codecs::gif::GifEncoder;
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageFormat, RgbImage, RgbaImage};
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Write};
use std::path::Path;
use std::time::Duration;

use super::output::save_err;
use crate::pdf::writer::{write_jpeg_pdf, Page};
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
/// From this JPEG quality up ("High", "Best") colour keeps full resolution (4:4:4, as
/// macOS writes JPEG). Below it the user asked for a small file, and colour is stored at
/// half resolution (4:2:0, as cameras and libjpeg do): ~17% smaller, 1.8x faster.
pub(super) const JPEG_FULL_CHROMA: u8 = 90;
/// The largest width or height a WebP image can have
const WEBP_MAX: u32 = 16383;
/// From this many pixels oxipng compresses one level lower (10 instead of 11)
const OXIPNG_BIG: u64 = 8_000_000;
/// oxipng tries filters and compression levels until this much time has passed, then
/// keeps the best result so far: a 100 MP PNG must not take minutes.
const OXIPNG_TIMEOUT: Duration = Duration::from_secs(15);

/// Images that are mostly flat areas: screenshots, documents, drawings, logos. On these
/// pixels very often repeat their left neighbour exactly (89-95% measured), on photos
/// and gradients almost never (13-15%, less with real sensor noise). Encoders have
/// settings that pay off for one kind and cost on the other.
pub(super) fn looks_like_graphic(img: &DynamicImage) -> bool {
    let (bytes, step) = match img {
        DynamicImage::ImageRgb8(p) => (p.as_raw().as_slice(), 3),
        DynamicImage::ImageRgba8(p) => (p.as_raw().as_slice(), 4),
        DynamicImage::ImageLuma8(p) => (p.as_raw().as_slice(), 1),
        _ => return false,
    };
    let row = img.width() as usize * step;
    if row < 2 * step {
        return false;
    }
    // Every other row is plenty: 3 ms for 12 MP
    fn repeats<const N: usize>(bytes: &[u8], row: usize) -> (usize, usize) {
        let (mut same, mut total) = (0, 0);
        for line in bytes.chunks_exact(row).step_by(2) {
            let pixels = line.as_chunks::<N>().0;
            same += pixels.windows(2).filter(|pair| pair[0] == pair[1]).count();
            total += pixels.len() - 1;
        }
        (same, total)
    }
    let (same, total) = match step {
        1 => repeats::<1>(bytes, row),
        3 => repeats::<3>(bytes, row),
        _ => repeats::<4>(bytes, row),
    };
    same * 2 > total
}

/// RGB(A) pixels that are all exactly gray (rendered text pages, black and white
/// scans): one channel says the same, a third of the data.
fn gray_channel(bytes: &[u8], step: usize) -> Option<Vec<u8>> {
    if !bytes.chunks_exact(step).all(|p| p[0] == p[1] && p[1] == p[2]) {
        return None;
    }
    Some(bytes.chunks_exact(step).map(|p| p[0]).collect())
}

/// `img` as one-channel grayscale when it is opaque and exactly gray.
fn as_gray(img: &DynamicImage) -> Option<image::GrayImage> {
    let gray = match img {
        DynamicImage::ImageRgb8(p) => gray_channel(p.as_raw(), 3)?,
        DynamicImage::ImageRgba8(p) if is_opaque(p) => gray_channel(p.as_raw(), 4)?,
        _ => return None,
    };
    image::GrayImage::from_raw(img.width(), img.height(), gray)
}

fn is_opaque(rgba: &RgbaImage) -> bool {
    rgba.as_raw().as_chunks::<4>().0.iter().all(|p| p[3] == u8::MAX)
}

fn rgb_from_rgba(rgba: &RgbaImage) -> RgbImage {
    let mut rgb = vec![0u8; rgba.width() as usize * rgba.height() as usize * 3];
    for (o, p) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(rgba.as_raw().as_chunks::<4>().0) {
        o.copy_from_slice(&p[..3]);
    }
    RgbImage::from_raw(rgba.width(), rgba.height(), rgb).expect("buffer sized for the image")
}

/// True if the image has an alpha channel that is not fully opaque.
pub(super) fn has_transparency(img: &DynamicImage) -> bool {
    match img {
        DynamicImage::ImageRgba8(rgba) => !is_opaque(rgba),
        _ if img.color().has_alpha() => !is_opaque(&img.to_rgba8()),
        _ => false,
    }
}

/// 8-bit RGB, or RGBA when some of it is transparent: what the raw-pixel formats (BMP,
/// TIFF, QOI, WEBP, AVIF, GIF) should store. An alpha channel that is fully opaque
/// only wastes space: 3 bytes per pixel instead of 4 is 25% smaller for an identical
/// image. Borrowed when the image already is what the encoder needs.
pub(super) fn drop_opaque_alpha(img: &DynamicImage) -> Cow<'_, DynamicImage> {
    match img {
        DynamicImage::ImageRgb8(_) => Cow::Borrowed(img),
        DynamicImage::ImageRgba8(rgba) if is_opaque(rgba) => Cow::Owned(DynamicImage::ImageRgb8(rgb_from_rgba(rgba))),
        DynamicImage::ImageRgba8(_) => Cow::Borrowed(img),
        _ if img.color().has_alpha() => {
            let rgba = img.to_rgba8();
            if is_opaque(&rgba) {
                Cow::Owned(DynamicImage::ImageRgb8(rgb_from_rgba(&rgba)))
            } else {
                Cow::Owned(DynamicImage::ImageRgba8(rgba))
            }
        }
        _ => Cow::Owned(DynamicImage::ImageRgb8(img.to_rgb8())),
    }
}

/// Like `drop_opaque_alpha`, but grayscale (also RGB that is exactly gray) stays one
/// channel, a third of the data, for the formats that store it: PNG, TIFF, BMP and TGA.
fn keep_gray(img: &DynamicImage) -> Cow<'_, DynamicImage> {
    match img {
        DynamicImage::ImageLuma8(_) => Cow::Borrowed(img),
        DynamicImage::ImageLuma16(_) => Cow::Owned(DynamicImage::ImageLuma8(img.to_luma8())),
        DynamicImage::ImageLumaA8(_) | DynamicImage::ImageLumaA16(_) => {
            let gray = img.to_luma_alpha8();
            if gray.pixels().all(|p| p[1] == u8::MAX) {
                Cow::Owned(DynamicImage::ImageLuma8(img.to_luma8()))
            } else {
                Cow::Owned(DynamicImage::ImageLumaA8(gray))
            }
        }
        _ => match as_gray(img) {
            Some(gray) => Cow::Owned(DynamicImage::ImageLuma8(gray)),
            None => drop_opaque_alpha(img),
        },
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
    match &*keep_gray(img) {
        DynamicImage::ImageLuma8(p) => encoder.write_image::<colortype::Gray8>(p.width(), p.height(), p.as_raw()),
        DynamicImage::ImageRgb8(p) => encoder.write_image::<colortype::RGB8>(p.width(), p.height(), p.as_raw()),
        DynamicImage::ImageRgba8(p) => encoder.write_image::<colortype::RGBA8>(p.width(), p.height(), p.as_raw()),
        other => {
            let rgba = other.to_rgba8();
            encoder.write_image::<colortype::RGBA8>(rgba.width(), rgba.height(), rgba.as_raw())
        }
    }
    .map_err(save_err)
}

/// Lossy (or lossless at quality >= 100) WebP encoding via libwebp. `fast` (method 2
/// instead of 4) is twice as fast for files a few percent larger: for trial encodes.
pub(super) fn encode_webp_bytes(img: &DynamicImage, quality: u8, fast: bool) -> Result<Vec<u8>, String> {
    let (w, h) = (img.width(), img.height());
    if w > WEBP_MAX || h > WEBP_MAX {
        return Err(format!("WEBP cannot be larger than {WEBP_MAX} px per side (this image is {w} × {h})"));
    }
    let pixels = drop_opaque_alpha(img);
    let encoder = match &*pixels {
        DynamicImage::ImageRgb8(rgb) => webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()),
        DynamicImage::ImageRgba8(rgba) => webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()),
        _ => unreachable!("drop_opaque_alpha returns 8-bit RGB or RGBA"),
    };
    // Not `encode()`/`encode_lossless()`: they panic when libwebp fails, closing the app
    if quality >= 100 {
        return encoder.encode_simple(true, 75.0).map(|m| m.to_vec()).map_err(|e| save_err(format!("{e:?}")));
    }
    let mut config = webp::WebPConfig::new().map_err(|_| save_err("WEBP settings"))?;
    config.quality = f32::from(quality.clamp(1, 100));
    config.method = if fast { 2 } else { 4 };
    if looks_like_graphic(&pixels) {
        // libwebp's "picture" preset: 4% smaller than its defaults at the same
        // SSIMULACRA2 score on screenshots, documents and logos (and worse on photos)
        config.sns_strength = 80;
        config.filter_sharpness = 4;
        config.filter_strength = 35;
        config.preprocessing &= !2;
    }
    encoder.encode_advanced(&config).map(|m| m.to_vec()).map_err(|e| save_err(format!("{e:?}")))
}

/// Lossless PNG optimisation via `oxipng`, straight from the pixels: no first PNG is
/// compressed only to be decoded and recompressed again (2.6 s instead of 3.8 s for a
/// 5K screenshot, identical file).
pub(super) fn encode_png_oxipng(img: &DynamicImage, output: &Path) -> Result<(), String> {
    use oxipng::ColorType as C;
    let (w, h) = (img.width(), img.height());
    let (color, data) = match keep_gray(img).into_owned() {
        DynamicImage::ImageLuma8(p) => (C::Grayscale { transparent_shade: None }, p.into_raw()),
        DynamicImage::ImageLumaA8(p) => (C::GrayscaleAlpha, p.into_raw()),
        DynamicImage::ImageRgb8(p) => (C::RGB { transparent_color: None }, p.into_raw()),
        DynamicImage::ImageRgba8(p) => (C::RGBA, p.into_raw()),
        _ => unreachable!("keep_gray returns 8-bit gray or RGB(A)"),
    };
    let raw = oxipng::RawImage::new(w, h, color, oxipng::BitDepth::Eight, data).map_err(save_err)?;
    let mut opts = oxipng::Options::from_preset(2);
    opts.strip = oxipng::StripChunks::Safe;
    opts.timeout = Some(OXIPNG_TIMEOUT);
    if u64::from(w) * u64::from(h) >= OXIPNG_BIG {
        // Measured on a 5K screenshot: 40% faster (1.6 s instead of 2.7 s), 0.6% larger
        opts.deflater = oxipng::Deflater::Libdeflater { compression: 10 };
    }
    let png = raw.create_optimized_png(&opts).map_err(save_err)?;
    fs::write(output, png).map_err(save_err)
}

/// Multi-resolution Windows `.ico` file containing standard icon sizes up to 256×256.
pub(super) fn write_ico_multisize(img: &DynamicImage, output: &Path) -> Result<(), String> {
    let base = square_icon(img);
    let max_side = base.width().min(ICO_MAX);
    let mut sizes: Vec<u32> = ICO_SIZES.iter().copied().filter(|&s| s < max_side).collect();
    sizes.push(max_side);

    let mut rendered: Vec<(u32, RgbaImage)> = Vec::with_capacity(sizes.len());
    for s in sizes {
        let frame_img = if s == base.width() { base.clone() } else { resize_rgba(&base, s, s) };
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
        let resized = if target == base.width() { base.clone() } else { resize_rgba(&base, target, target) };
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
    keep_gray(img)
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
    resize_exact(&img, tw, th)
}

fn lanczos3() -> fr::ResizeOptions {
    fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3))
}

/// Lanczos3 with SIMD (NEON, SSE4.1, AVX2): 3-4x faster than `image`'s own resize.
/// Transparent images are resized with premultiplied alpha, so edges get no dark fringe.
pub(super) fn resize_exact(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    if (w, h) == (img.width(), img.height()) {
        return img.clone();
    }
    let mut out = DynamicImage::new(w.max(1), h.max(1), img.color());
    match fr::Resizer::new().resize(img, &mut out, &lanczos3()) {
        Ok(()) => out,
        // A pixel type it does not handle
        Err(_) => img.resize_exact(w, h, FilterType::Lanczos3),
    }
}

/// Scales to the largest size that fits in `max_w` × `max_h`, keeping the proportions
/// (like `DynamicImage::resize`).
pub(super) fn resize_to_fit(img: &DynamicImage, max_w: u32, max_h: u32) -> DynamicImage {
    let (w, h) = (f64::from(img.width().max(1)), f64::from(img.height().max(1)));
    let ratio = (f64::from(max_w) / w).min(f64::from(max_h) / h);
    let fit = |side: f64| ((side * ratio).round() as u32).max(1);
    resize_exact(img, fit(w), fit(h))
}

pub(super) fn resize_rgba(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w.max(1), h.max(1));
    match fr::Resizer::new().resize(img, &mut out, &lanczos3()) {
        Ok(()) => out,
        Err(_) => image::imageops::resize(img, w, h, FilterType::Lanczos3),
    }
}

// AVIF at the same number looks noticeably better than JPEG, so it is shifted down
// to keep files small while matching the perceived quality.
pub(super) fn avif_quality(quality: u8) -> u8 {
    quality.saturating_sub(10).max(30)
}

/// Encodes `img` as AVIF into `out`: with the system's encoder where there is one (see
/// `system_codec::encode_avif`), else with rav1e, without an alpha plane when opaque.
pub(super) fn encode_avif(img: &DynamicImage, quality: u8, mut out: impl Write) -> Result<(), String> {
    if let Some(avif) = system_codec::encode_avif(img, quality) {
        return out.write_all(&avif).map_err(save_err);
    }
    let pixels = drop_opaque_alpha(img);
    // rav1e's speed 9 allows no block smaller than 16 px: on screenshots and documents
    // that costs 40% more bytes than speed 8 at the same SSIMULACRA2 score
    let speed = if looks_like_graphic(&pixels) { AVIF_SPEED - 1 } else { AVIF_SPEED };
    pixels
        .write_with_encoder(AvifEncoder::new_with_speed_quality(out, speed, avif_quality(quality)))
        .map_err(save_err)
}

/// Writes `img` to `output` as `ext`. A failed encoder leaves no empty or half-written
/// file behind (the name was free, so nothing else is removed).
pub(super) fn encode(img: &DynamicImage, ext: &str, output: &Path, quality: u8) -> Result<(), String> {
    let result = encode_file(img, ext, output, quality);
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}

fn encode_file(img: &DynamicImage, ext: &str, output: &Path, quality: u8) -> Result<(), String> {
    let create = || File::create(output).map(BufWriter::new).map_err(save_err);

    match ext {
        "jpg" | "jpeg" => {
            let mut file = create()?;
            encode_jpeg(img, quality, &mut file)?;
            file.flush().map_err(save_err)
        }
        // PNG is lossless; "Compress" / "Smaller File" squeezes it further with oxipng.
        "png" if quality <= COMPRESS_QUALITY => encode_png_oxipng(img, output),
        "png" => keep_gray(img).save_with_format(output, ImageFormat::Png).map_err(save_err),
        "webp" => {
            let bytes = encode_webp_bytes(img, quality, false)?;
            fs::write(output, bytes).map_err(save_err)
        }
        "heic" => system_codec::encode_heic(img, quality, output),
        "bmp" => keep_gray(img).save_with_format(output, ImageFormat::Bmp).map_err(save_err),
        "tiff" if quality <= COMPRESS_QUALITY => write_tiff_deflate(img, output),
        "tiff" => keep_gray(img).save_with_format(output, ImageFormat::Tiff).map_err(save_err),
        "gif" => drop_opaque_alpha(img)
            .write_with_encoder(GifEncoder::new_with_speed(create()?, GIF_SPEED))
            .map_err(save_err),
        "ico" => write_ico_multisize(img, output),
        "icns" => write_icns(img, output),
        "tga" => keep_gray(img).save_with_format(output, ImageFormat::Tga).map_err(save_err),
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
        "avif" => {
            let mut file = create()?;
            encode_avif(img, quality, &mut file)?;
            file.flush().map_err(save_err)
        }
        "pdf" => {
            let page = pdf_page(img, quality)?;
            let mut file = create()?;
            write_jpeg_pdf(&[page], &mut file).and_then(|_| file.flush()).map_err(save_err)
        }
        _ => unreachable!(),
    }
}

/// Centers `img` on a square transparent canvas of at most `max_side` px.
pub(super) fn square_canvas(img: &DynamicImage, max_side: u32) -> RgbaImage {
    let fitted = if img.width() > max_side || img.height() > max_side {
        Cow::Owned(resize_to_fit(img, max_side, max_side))
    } else {
        Cow::Borrowed(img)
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

/// The pixels `jpeg-encoder` reads for `img`, borrowed when they can be used as they are
/// (it skips the alpha byte of opaque RGBA itself). Grayscale stays one channel: a
/// third of the work and a smaller file.
fn jpeg_pixels(img: &DynamicImage) -> (Cow<'_, [u8]>, jpeg_encoder::ColorType) {
    use jpeg_encoder::ColorType as C;
    if let Some(gray) = as_gray(img) {
        return (Cow::Owned(gray.into_raw()), C::Luma);
    }
    match img {
        DynamicImage::ImageLuma8(p) => (Cow::Borrowed(p.as_raw().as_slice()), C::Luma),
        DynamicImage::ImageRgb8(p) => (Cow::Borrowed(p.as_raw().as_slice()), C::Rgb),
        DynamicImage::ImageRgba8(p) if is_opaque(p) => (Cow::Borrowed(p.as_raw().as_slice()), C::Rgba),
        DynamicImage::ImageLuma16(_) => (Cow::Owned(img.to_luma8().into_raw()), C::Luma),
        _ => (Cow::Owned(flatten_on_white(img).into_owned().into_raw()), C::Rgb),
    }
}

fn write_jpeg(pixels: &[u8], w: u32, h: u32, color: jpeg_encoder::ColorType, quality: u8, out: impl Write) -> Result<(), String> {
    let (Ok(w16), Ok(h16)) = (u16::try_from(w), u16::try_from(h)) else {
        return Err(format!("JPEG cannot be larger than 65535 px per side (this image is {w} × {h})"));
    };
    let mut encoder = jpeg_encoder::Encoder::new(out, quality.clamp(1, 100));
    // Never with optimised Huffman tables: they buffer every block of the image (+72 MB
    // for 12 MP) for ~1% smaller files, and the `image` crate's decoder misreads 4:2:0
    // files written with them.
    if quality >= JPEG_FULL_CHROMA {
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
    } else {
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
        // Quantisation tables tuned for MS-SSIM instead of the standard's: at the same
        // SSIMULACRA2 score 3% smaller on photos, 12-28% on screenshots, documents and
        // gradients. (At 4:4:4, for the top qualities, the standard tables do best.)
        use jpeg_encoder::QuantizationTableType::CustomMsSsim;
        encoder.set_quantization_tables(CustomMsSsim, CustomMsSsim);
    }
    encoder.encode(pixels, w16, h16, color).map_err(save_err)
}

/// JPEG through `jpeg-encoder` (transparent areas become white).
pub(super) fn encode_jpeg(img: &DynamicImage, quality: u8, out: impl Write) -> Result<(), String> {
    let (pixels, color) = jpeg_pixels(img);
    write_jpeg(&pixels, img.width(), img.height(), color, quality, out)
}

pub(super) fn pdf_page(img: &DynamicImage, quality: u8) -> Result<Page, String> {
    let (pixels, color) = jpeg_pixels(img);
    let mut jpeg = Vec::new();
    write_jpeg(&pixels, img.width(), img.height(), color, quality, &mut jpeg)?;
    Ok(Page { jpeg, width: img.width(), height: img.height(), gray: color == jpeg_encoder::ColorType::Luma })
}

// Transparent areas get a white background (JPG & PDF have no transparency)
pub(super) fn flatten_on_white(img: &DynamicImage) -> Cow<'_, RgbImage> {
    let blend = |rgba: &RgbaImage| {
        let mut rgb = vec![0u8; rgba.width() as usize * rgba.height() as usize * 3];
        for (o, p) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(rgba.as_raw().as_chunks::<4>().0) {
            let a = p[3] as u32;
            for c in 0..3 {
                o[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
            }
        }
        RgbImage::from_raw(rgba.width(), rgba.height(), rgb).expect("buffer sized for the image")
    };
    match img {
        DynamicImage::ImageRgb8(rgb) => Cow::Borrowed(rgb),
        DynamicImage::ImageRgba8(rgba) => Cow::Owned(blend(rgba)),
        _ if img.color().has_alpha() => Cow::Owned(blend(&img.to_rgba8())),
        _ => Cow::Owned(img.to_rgb8()),
    }
}
