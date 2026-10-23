// "Clean": removes metadata while keeping the format (lossless for JPEG and PNG).

use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::ImageFormat;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use super::encode::flatten_on_white;
use super::input::{file_name, open_oriented};
use super::output::{save_err, unique_output_path};
use super::strip::{strip_jpeg, strip_png};
use crate::platform::system_codec;

/// Removes metadata (EXIF, GPS, camera info, comments) while keeping the format.
/// JPEG and PNG are cleaned losslessly; other formats are re-encoded.
pub(super) fn clean(input: &Path, out_dir: Option<&Path>) -> Result<String, String> {
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
