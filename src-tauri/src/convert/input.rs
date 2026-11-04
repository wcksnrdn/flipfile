// Inspecting input files: names, magic bytes, Word/Excel detection, decoding with EXIF
// orientation, and the extension to keep when a file stays in its own format.

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::fs::File;
use std::path::Path;

use crate::platform::system_codec;

pub(super) fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("File")
}

/// Decodes an image and applies its EXIF orientation, so phone photos are not
/// saved sideways once the orientation tag is gone.
pub(super) fn open_oriented(path: &Path) -> Result<(DynamicImage, Orientation), String> {
    let not_image = || format!("\"{}\" is not a supported image", file_name(path));

    // HEIC (iPhone photos) and AVIF are decoded by the OS, already upright.
    if is_heif_family_file(path) {
        let img = system_codec::decode(path).map_err(|e| format!("\"{}\": {e}", file_name(path)))?;
        return Ok((img, Orientation::NoTransforms));
    }

    let decoded = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|_| ())
        .and_then(|r| r.into_decoder().map_err(|_| ()))
        .and_then(|mut decoder| {
            let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
            let profile = decoder.icc_profile().ok().flatten().or_else(|| tiff_icc_profile(path));
            let mut img = DynamicImage::from_decoder(decoder).map_err(|_| ())?;
            img.apply_orientation(orientation);
            if let Some(profile) = profile {
                to_srgb(&mut img, &profile);
            }
            Ok((img, orientation))
        });

    if let Ok(pair) = decoded {
        return Ok(pair);
    }

    // Fallback to OS codec (ImageIO on macOS / WIC on Windows) for PSD, JPEG 2000,
    // and 25+ camera RAW formats (DNG, CR2, CR3, NEF, ARW, RAF, ORF, RW2, PEF, …).
    if let Ok(img) = system_codec::decode(path) {
        return Ok((img, Orientation::NoTransforms));
    }

    Err(not_image())
}

/// Converts pixels described by an embedded ICC profile into sRGB, the colour space
/// every file flipfile writes is read in (they carry no profile). Mac screenshots and
/// iPhone photos are Display P3: without this their colours come out dull (pure red
/// as 232, 52, 35). Pixels already in sRGB are left untouched; 18 ms for 12 MP.
pub(super) fn to_srgb(img: &mut DynamicImage, profile: &[u8]) {
    use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
    let Ok(source) = ColorProfile::new_from_slice(profile) else { return };
    if source.color_space != DataColorSpace::Rgb {
        return; // gray and CMYK profiles describe other pixels than these
    }
    let srgb = ColorProfile::new_srgb();
    let options = TransformOptions::default();
    let layout = |alpha: bool| if alpha { Layout::Rgba } else { Layout::Rgb };
    match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => {
            let alpha = img.color().has_alpha();
            let Ok(t) = source.create_transform_8bit(layout(alpha), &srgb, layout(alpha), options) else { return };
            let channels = if alpha { 4 } else { 3 };
            // 125 colours across the cube tell an sRGB profile (any version) from others
            let probe: Vec<u8> = (0..125u32)
                .flat_map(|i| [(i % 5 * 63) as u8, (i / 5 % 5 * 63) as u8, (i / 25 * 63) as u8, 255].into_iter().take(channels))
                .collect();
            let mut probed = vec![0; probe.len()];
            if t.transform(&probe, &mut probed).is_err() || probe.iter().zip(&probed).all(|(a, b)| a.abs_diff(*b) <= 1) {
                return;
            }
            let data: &mut [u8] = match img {
                DynamicImage::ImageRgb8(p) => p,
                DynamicImage::ImageRgba8(p) => p,
                _ => unreachable!(),
            };
            transform_in_place(|src, dst| t.transform(src, dst).is_ok(), data, channels);
        }
        DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) => {
            let alpha = img.color().has_alpha();
            let Ok(t) = source.create_transform_16bit(layout(alpha), &srgb, layout(alpha), options) else { return };
            let data: &mut [u16] = match img {
                DynamicImage::ImageRgb16(p) => p,
                DynamicImage::ImageRgba16(p) => p,
                _ => unreachable!(),
            };
            transform_in_place(|src, dst| t.transform(src, dst).is_ok(), data, if alpha { 4 } else { 3 });
        }
        _ => {}
    }
}

/// The ICC profile of a TIFF file: the `image` crate's TIFF decoder never hands it over.
fn tiff_icc_profile(path: &Path) -> Option<Vec<u8>> {
    let head = header(path);
    if !head.starts_with(b"II*\0") && !head.starts_with(b"MM\0*") {
        return None;
    }
    let file = std::io::BufReader::new(File::open(path).ok()?);
    tiff::decoder::Decoder::new(file).ok()?.get_tag_u8_vec(tiff::tags::Tag::IccProfile).ok()
}

/// Runs `transform` over `data` a slice at a time, so no second copy of the image is needed.
fn transform_in_place<V: Copy + Default>(transform: impl Fn(&[V], &mut [V]) -> bool, data: &mut [V], channels: usize) {
    let mut scratch = vec![V::default(); (channels << 16).min(data.len())];
    for part in data.chunks_mut(channels << 16) {
        let out = &mut scratch[..part.len()];
        if transform(part, out) {
            part.copy_from_slice(out);
        }
    }
}

pub(super) fn header(path: &Path) -> [u8; 12] {
    let mut header = [0u8; 12];
    let _ = File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut header));
    header
}

pub(super) fn is_heif_family_file(path: &Path) -> bool {
    system_codec::is_heif_family(&header(path))
}

/// `Some(false)` for .docx-family files, `Some(true)` for legacy binary .doc, from the
/// file's content *and* extension (other ZIP-based files are not Word documents).
pub(super) fn word_kind(path: &Path) -> Option<bool> {
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

/// The kind of spreadsheet, from the file's content *and* extension
pub(super) fn sheet_kind(path: &Path) -> Option<crate::office::apps::SheetKind> {
    use crate::office::apps::SheetKind;
    let head = header(path);
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    match ext.as_str() {
        "xlsx" | "xlsm" | "xltx" | "xltm" if head.starts_with(b"PK\x03\x04") => Some(SheetKind::Xlsx),
        "xls" | "xlt" if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) => Some(SheetKind::Other),
        "ods" if head.starts_with(b"PK\x03\x04") => Some(SheetKind::Other),
        "csv" => Some(SheetKind::Csv),
        _ => None,
    }
}

/// The extension to write when keeping a file's own format.
pub(super) fn same_format_ext(input: &Path) -> Result<String, String> {
    let head = header(input);
    if system_codec::is_heif_family(&head) {
        if matches!(&head[8..12], b"avif" | b"avis") {
            return Ok("avif".into());
        }
        return Ok(if cfg!(target_os = "macos") { "heic" } else { "jpg" }.into());
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
        ImageFormat::Tga => "tga".into(),
        ImageFormat::Qoi => "qoi".into(),
        ImageFormat::OpenExr => "exr".into(),
        ImageFormat::Hdr => "hdr".into(),
        ImageFormat::Pnm => "ppm".into(),
        // Formats that can be read but not written (e.g. DDS, Farbfeld) become PNG
        _ => "png".into(),
    })
}
