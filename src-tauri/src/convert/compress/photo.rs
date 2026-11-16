// Photos at a compress level: lossy formats (JPG, WebP, AVIF, HEIC) stay in their format
// at the level's quality; lossless ones (PNG, TIFF, BMP, ...) are only optimised
// losslessly at Light, and become JPG (WebP when transparent) from Balanced on, where
// the bytes are. Camera RAW, Photoshop and JPEG 2000 files, which only the system can
// read, become JPG (WebP when transparent) at every level. Large photos are scaled down
// from Strong on.

use std::borrow::Cow;
use std::fs;
use std::path::Path;

use image::DynamicImage;

use super::{changed, Level, Made};
use crate::convert::encode::{encode, encode_avif, encode_jpeg, encode_png_oxipng, encode_webp_bytes, has_transparency, resize, write_tiff_deflate};
use crate::convert::input::{open_oriented, same_format_ext};
use crate::convert::output::{save_err, unique_output_path};
use crate::state::settings::Resize;

/// Encoder quality and longest side of each level
fn params(level: Level) -> (u8, Option<u32>) {
    match level {
        Level::Light => (82, None),
        Level::Balanced => (72, Some(3200)),
        Level::Strong => (58, Some(1920)),
        Level::Extreme => (40, Some(1280)),
    }
}

/// Photos the system reads but flipfile can't write back: camera RAW, Photoshop, JPEG 2000
const SOURCE_ONLY: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "srw", "x3f", "3fr", "iiq", "rwl", "erf",
    "mef", "mos", "kdc", "dcr", "psd", "psb", "jp2", "j2k", "jpx",
];

/// True for a camera RAW, Photoshop or JPEG 2000 file, on a system that reads them
pub(super) fn is_source_only(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    cfg!(any(target_os = "macos", windows)) && SOURCE_ONLY.contains(&ext.as_str())
}

/// The format a photo keeps, or "source" for one that can only be read
pub(super) fn own_format(input: &Path) -> Result<String, String> {
    match same_format_ext(input) {
        Err(_) if is_source_only(input) => Ok("source".into()),
        other => other,
    }
}

pub(super) fn compress(input: &Path, level: Level, out_dir: Option<&Path>) -> Result<Made, String> {
    let own = own_format(input)?;
    let (img, _) = open_oriented(input)?;
    let (bytes, ext) = encode_level(&img, &own, level)?;
    let path = unique_output_path(input, out_dir, "compressed", &ext);
    fs::write(&path, bytes).map_err(save_err)?;
    Ok(Made { changed: changed(input, &ext), path })
}

/// The file `level` makes of `img` (whose own format is `own`), and its extension
pub(super) fn encode_level(img: &DynamicImage, own: &str, level: Level) -> Result<(Vec<u8>, String), String> {
    let (quality, max_edge) = params(level);
    let img: Cow<'_, DynamicImage> = match max_edge {
        Some(edge) if img.width().max(img.height()) > edge => Cow::Owned(resize(img.clone(), Resize::MaxEdge(edge))),
        _ => Cow::Borrowed(img),
    };
    let lossy = matches!(own, "jpg" | "jpeg" | "webp" | "avif" | "heic");
    let ext = if lossy {
        own.to_string()
    } else if level == Level::Light && own != "source" {
        if own == "tiff" { "tiff" } else { "png" }.to_string()
    } else if has_transparency(&img) {
        "webp".into()
    } else {
        "jpg".into()
    };
    let mut bytes = Vec::new();
    match ext.as_str() {
        "jpg" | "jpeg" => encode_jpeg(&img, quality, &mut bytes)?,
        "webp" => bytes = encode_webp_bytes(&img, quality, false)?,
        "avif" => encode_avif(&img, quality, &mut bytes)?,
        "png" => bytes = via_file("png", |path| encode_png_oxipng(&img, path))?,
        "tiff" => bytes = via_file("tiff", |path| write_tiff_deflate(&img, path))?,
        // HEIC is written by the system's encoder (macOS), from a file
        other => bytes = via_file(other, |path| encode(&img, other, path, quality))?,
    }
    Ok((bytes, ext))
}

/// Bytes of an encoder that only writes files
fn via_file(ext: &str, write: impl FnOnce(&Path) -> Result<(), String>) -> Result<Vec<u8>, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tid = format!("{:?}", std::thread::current().id()).replace(|c: char| !c.is_ascii_alphanumeric(), "");
    let path = std::env::temp_dir().join(format!("flipfile-cmp-{}-{tid}-{nanos}.{ext}", std::process::id()));
    let result = write(&path).and_then(|_| fs::read(&path).map_err(|e| e.to_string()));
    let _ = fs::remove_file(&path);
    result
}
