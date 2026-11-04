// HEIC/HEIF (iPhone photos) and AVIF input through the operating system's own codecs,
// so no large codec library has to be bundled:
// - macOS: ImageIO (built in).
// - Windows: WIC, which needs the free "HEIF Image Extensions" / "AV1 Video Extension"
//   from the Microsoft Store.
// - Linux: not supported yet; the user gets a clear message instead.
//
// The returned image is already upright (orientation applied). On macOS the pixels are
// handed over directly (no PNG in between, 7x faster) and converted to sRGB, so the
// wide-gamut colours of iPhone photos (Display P3) stay right once written to a file
// without that profile.

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
    // Conversions run on worker threads, which have no autorelease pool of their own
    objc2::rc::autoreleasepool(|_| decode_in_pool(path))
}

#[cfg(target_os = "macos")]
fn decode_in_pool(path: &Path) -> Result<DynamicImage, String> {
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFString, CFType, CFURL};
    use objc2_image_io::{
        kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, CGImageSource,
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
    srgb_pixels(&image).ok_or_else(failed)
}

/// Draws `image` into a plain 8-bit sRGB buffer; CoreGraphics converts from the image's
/// own colour space. Opaque images come back as RGB, the others as (straight) RGBA.
#[cfg(target_os = "macos")]
fn srgb_pixels(image: &objc2_core_graphics::CGImage) -> Option<DynamicImage> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
        CGImageByteOrderInfo,
    };

    let (w, h) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let (width, height) = (u32::try_from(w).ok()?, u32::try_from(h).ok()?);
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let mut pixels = vec![0u8; w.checked_mul(h)?.checked_mul(4)?];
    // R, G, B, A, with premultiplied alpha: the only kind a bitmap context draws into
    let format = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    let context = unsafe { CGBitmapContextCreate(pixels.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&srgb), format) }?;
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    drop(context);

    // (ImageIO hands out full-size thumbnails with an alpha channel even for opaque photos)
    if pixels.as_chunks::<4>().0.iter().all(|p| p[3] == u8::MAX) {
        // RGBX -> RGB in place: a quarter less memory for the rest of the conversion
        for i in 0..w * h {
            let (from, to) = (i * 4, i * 3);
            pixels[to] = pixels[from];
            pixels[to + 1] = pixels[from + 1];
            pixels[to + 2] = pixels[from + 2];
        }
        pixels.truncate(w * h * 3);
        pixels.shrink_to_fit();
        return image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8);
    }
    for p in pixels.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
}

/// A CGImage of 8-bit sRGB pixels, 3 (RGB) or 4 (RGBA, straight alpha) per pixel; the
/// fourth byte is skipped when `opaque`. The pixels are copied into it.
#[cfg(target_os = "macos")]
pub fn cg_image(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    opaque: bool,
) -> Option<objc2_core_foundation::CFRetained<objc2_core_graphics::CGImage>> {
    use objc2_core_foundation::CFData;
    use objc2_core_graphics::{
        kCGColorSpaceSRGB, CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
        CGImageByteOrderInfo,
    };

    let (w, h) = (width as usize, height as usize);
    if pixels.len() != w * h * channels || !matches!(channels, 3 | 4) {
        return None;
    }
    let alpha = match (channels, opaque) {
        (3, _) => CGImageAlphaInfo::None,
        (_, true) => CGImageAlphaInfo::NoneSkipLast,
        _ => CGImageAlphaInfo::Last,
    };
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let data = CFData::from_bytes(pixels);
    let provider = CGDataProvider::with_cf_data(Some(&data))?;
    unsafe {
        CGImage::new(
            w,
            h,
            8,
            8 * channels,
            w * channels,
            Some(&srgb),
            CGBitmapInfo(alpha.0 | CGImageByteOrderInfo::OrderDefault.0),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
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

/// Writes HEIC (the iPhone's own format: about half the size of JPG at the same
/// quality) with ImageIO. `quality` is 1..=100, as for JPG.
#[cfg(target_os = "macos")]
pub fn encode_heic(img: &DynamicImage, quality: u8, output: &Path) -> Result<(), String> {
    let heic = encode_with_imageio(img, "public.heic", f64::from(quality.clamp(1, 100)) / 100.0)
        .ok_or_else(|| "macOS could not write HEIC".to_string())?;
    std::fs::write(output, heic).map_err(|e| format!("Could not save: {e}"))
}

#[cfg(not(target_os = "macos"))]
pub fn encode_heic(_img: &DynamicImage, _quality: u8, _output: &Path) -> Result<(), String> {
    Err("Saving as HEIC is only available on macOS for now".into())
}

/// AVIF with ImageIO (macOS 14+), or `None` where it cannot write AVIF. Measured on
/// photos, screenshots, documents, gradients and logos against rav1e (the built-in
/// encoder) at the same SSIMULACRA2 score: 3-4x faster on photos for files ~5% larger,
/// and 24-58% smaller files on everything else, where its screen-content tools shine.
/// `quality` is 1..=100, as for JPG, and matches what rav1e gave at the same setting.
pub fn encode_avif(img: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let q = (0.0575 + 0.00625 * f64::from(quality.clamp(1, 100))).clamp(0.05, 1.0);
        encode_with_imageio(img, "public.avif", q)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (img, quality);
        None
    }
}

/// Encodes `img` as `uti` (a format ImageIO writes) at `quality` 0..=1, in memory.
/// ImageIO takes the pixels as they are (no PNG in between), keeping transparency.
#[cfg(target_os = "macos")]
fn encode_with_imageio(img: &DynamicImage, uti: &'static str, quality: f64) -> Option<Vec<u8>> {
    objc2::rc::autoreleasepool(|_| {
        use objc2_core_foundation::{CFDictionary, CFMutableData, CFNumber, CFString, CFType};
        use objc2_image_io::{kCGImageDestinationLossyCompressionQuality, CGImageDestination};

        let rgb;
        let rgba;
        let (pixels, channels) = match img {
            DynamicImage::ImageRgb8(p) => (p.as_raw(), 3),
            DynamicImage::ImageRgba8(p) => (p.as_raw(), 4),
            _ if img.color().has_alpha() => {
                rgba = img.to_rgba8();
                (rgba.as_raw(), 4)
            }
            _ => {
                rgb = img.to_rgb8();
                (rgb.as_raw(), 3)
            }
        };
        // An opaque RGBA image (a screenshot) needs no alpha plane in the file
        let opaque = channels == 3 || pixels.as_chunks::<4>().0.iter().all(|p| p[3] == u8::MAX);
        let image = cg_image(pixels, img.width(), img.height(), channels, opaque)?;

        let data = CFMutableData::new(None, 0)?;
        let kind = CFString::from_static_str(uti);
        // (nil where this macOS cannot write the format)
        let destination = unsafe { CGImageDestination::with_data(&data, &kind, 1, None) }?;
        let q = CFNumber::new_f64(quality);
        let q: &CFType = &q;
        let properties =
            unsafe { CFDictionary::<CFString, CFType>::from_slices(&[kCGImageDestinationLossyCompressionQuality], &[q]) };
        unsafe {
            destination.add_image(&image, Some(properties.as_opaque()));
            destination.finalize().then(|| data.to_vec())
        }
    })
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
            assert_eq!(img.color(), image::ColorType::Rgb8, "{ext}: opaque photos decode to RGB");
        }
    }

    // iPhone photos are Display P3: their pixel values only mean the right colour together
    // with that profile, which the files flipfile writes do not carry. Decoding converts
    // them to sRGB, so pure sRGB red stored as P3 comes back as pure red.
    #[cfg(target_os = "macos")]
    #[test]
    fn wide_gamut_photos_decode_to_srgb() {
        let dir = std::env::temp_dir().join(format!("flipfile-p3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let red = dir.join("red.png");
        image::RgbImage::from_pixel(32, 32, image::Rgb([255, 0, 0])).save(&red).unwrap();
        let p3 = dir.join("red-p3.heic");
        let sips = std::process::Command::new("sips")
            .args(["-m", "/System/Library/ColorSync/Profiles/Display P3.icc", "-s", "format", "heic"])
            .arg(&red)
            .arg("--out")
            .arg(&p3)
            .output()
            .unwrap();
        assert!(sips.status.success(), "sips could not write a P3 HEIC");

        let px = decode(&p3).unwrap().to_rgb8().get_pixel(16, 16).0;
        assert!(px[0] >= 250 && px[1] <= 6 && px[2] <= 6, "P3 red decoded as {px:?}, not sRGB red");
    }
}
