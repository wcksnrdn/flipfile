// Inspecting input files: names, magic bytes, Word detection, decoding with EXIF
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

/// The extension to write when keeping a file's own format. HEIC cannot be written,
/// so iPhone photos become JPG, the format every app can open.
pub(super) fn same_format_ext(input: &Path) -> Result<String, String> {
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
