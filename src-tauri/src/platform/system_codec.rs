// HEIC/HEIF (iPhone photos) and AVIF input through the operating system's own codecs,
// so no large codec library has to be bundled:
// - macOS: ImageIO (built in).
// - Windows: WIC, which needs the free "HEIF Image Extensions" / "AV1 Video Extension"
//   from the Microsoft Store.
// - Linux: not supported yet; the user gets a clear message instead.
//
// The returned image is already upright (orientation applied).

use image::DynamicImage;
use std::path::Path;

/// True for HEIF-family files (HEIC, HEIF, AVIF), detected from the ISO-BMFF `ftyp` box.
pub fn is_heif_family(header: &[u8]) -> bool {
    if header.len() < 12 || &header[4..8] != b"ftyp" {
        return false;
    }
    matches!(
        &header[8..12],
        b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" | b"avif" | b"avis"
    )
}

#[cfg(target_os = "macos")]
pub fn decode(path: &Path) -> Result<DynamicImage, String> {
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFMutableData, CFString, CFType, CFURL};
    use objc2_image_io::{
        kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
        CGImageDestination, CGImageSource,
    };

    let failed = || "macOS could not read this image".to_string();
    let url = CFURL::from_file_path(path).ok_or_else(failed)?;
    let source = unsafe { CGImageSource::with_url(&url, None) }.ok_or_else(failed)?;

    // A full-size "thumbnail" with the transform applied is the documented way to get
    // an upright image (HEIF rotation/mirroring and EXIF orientation).
    let yes: &CFType = CFBoolean::new(true);
    let options = unsafe {
        CFDictionary::<CFString, CFType>::from_slices(
            &[kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform],
            &[yes, yes],
        )
    };
    let image = unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }.ok_or_else(failed)?;

    // Hand it to the regular pipeline as PNG; ImageIO handles color conversion (incl. HDR).
    let data = CFMutableData::new(None, 0).ok_or_else(failed)?;
    let png = CFString::from_static_str("public.png");
    let destination = unsafe { CGImageDestination::with_data(&data, &png, 1, None) }.ok_or_else(failed)?;
    unsafe {
        destination.add_image(&image, None);
        if !destination.finalize() {
            return Err(failed());
        }
    }
    image::load_from_memory(&data.to_vec()).map_err(|e| e.to_string())
}

#[cfg(windows)]
pub fn decode(path: &Path) -> Result<DynamicImage, String> {
    use image::metadata::Orientation;
    use image::RgbaImage;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GENERIC_READ, WINCODEC_ERR_COMPONENTNOTFOUND};
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, WICBitmapDitherTypeNone,
        WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

    let failed = |e: windows::core::Error| format!("Windows could not read this image ({e})");
    unsafe {
        // Each conversion runs on a worker thread; COM must be initialised there.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(failed)?;
        let decoder = factory
            .CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(|e| {
                if e.code() == WINCODEC_ERR_COMPONENTNOTFOUND {
                    "Install the free \"HEIF Image Extensions\" (and \"AV1 Video Extension\" for AVIF) \
                     from the Microsoft Store to open this photo"
                        .to_string()
                } else {
                    failed(e)
                }
            })?;
        let frame = decoder.GetFrame(0).map_err(failed)?;

        let converter = factory.CreateFormatConverter().map_err(failed)?;
        converter
            .Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .map_err(failed)?;
        let (mut w, mut h) = (0u32, 0u32);
        converter.GetSize(&mut w, &mut h).map_err(failed)?;
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut pixels).map_err(failed)?;
        let mut img = DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, pixels).ok_or("Image data is damaged")?);

        // WIC returns the stored pixels; rotation lives in the photo metadata.
        let orientation = frame.GetMetadataQueryReader().ok().and_then(|reader| {
            let mut value = PROPVARIANT::default();
            reader.GetMetadataByName(&HSTRING::from("System.Photo.Orientation"), &mut value).ok()?;
            u16::try_from(&value).ok()
        });
        if let Some(o) = orientation.and_then(|o| Orientation::from_exif(o as u8)) {
            img.apply_orientation(o);
        }
        Ok(img)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn decode(_path: &Path) -> Result<DynamicImage, String> {
    Err("HEIC and AVIF photos are not supported on Linux yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_heif_family() {
        assert!(is_heif_family(b"\0\0\0\x18ftypheic\0\0\0\0"));
        assert!(is_heif_family(b"\0\0\0\x1cftypavif\0\0\0\0"));
        assert!(!is_heif_family(b"\x89PNG\r\n\x1a\n\0\0\0\0"));
        assert!(!is_heif_family(b"\0\0\0\x18ftypisom\0\0\0\0")); // MP4 video
    }

    // Uses the system `sips` tool to produce real HEIC/AVIF files, then decodes them.
    #[cfg(target_os = "macos")]
    #[test]
    fn decodes_heic_and_avif_via_imageio() {
        let dir = std::env::temp_dir().join(format!("flipfile-heic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("wide.png");
        image::RgbImage::from_fn(64, 32, |x, _| image::Rgb([x as u8 * 4, 90, 160])).save(&png).unwrap();

        for ext in ["heic", "avif"] {
            let out = dir.join(format!("wide.{ext}"));
            let status = std::process::Command::new("sips")
                .args(["-s", "format", ext])
                .arg(&png)
                .arg("--out")
                .arg(&out)
                .output()
                .unwrap();
            assert!(status.status.success(), "sips could not write {ext}");
            let header = std::fs::read(&out).unwrap();
            assert!(is_heif_family(&header), "{ext} not detected");
            let img = decode(&out).unwrap_or_else(|e| panic!("{ext}: {e}"));
            assert_eq!((img.width(), img.height()), (64, 32), "{ext}");
        }
    }
}
