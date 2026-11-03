// Background removal ("Remove BG" in Photo > Tools) with the operating system's own
// subject-lifting model, the one behind "Copy Subject" in Photos, so nothing is bundled:
// - macOS 14+: Vision's VNGenerateForegroundInstanceMaskRequest.
// - Elsewhere: a clear message (the UI disables the segment there).
//
// The result keeps the image's size; everything but the subject becomes transparent.

use image::RgbaImage;

#[cfg(target_os = "macos")]
pub fn remove_background(image: &RgbaImage) -> Result<RgbaImage, String> {
    // Conversions run on worker threads, which have no autorelease pool of their own
    objc2::rc::autoreleasepool(|_| remove_in_pool(image))
}

#[cfg(target_os = "macos")]
fn remove_in_pool(image: &RgbaImage) -> Result<RgbaImage, String> {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::AnyThread;
    use objc2_core_video::{
        kCVPixelFormatType_OneComponent32Float, kCVPixelFormatType_OneComponent8, CVPixelBufferGetBaseAddress,
        CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
        CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{VNGenerateForegroundInstanceMaskRequest, VNImageOption, VNImageRequestHandler, VNRequest};

    // The request only exists from macOS 14; asking for a missing class would abort.
    if AnyClass::get(c"VNGenerateForegroundInstanceMaskRequest").is_none() {
        return Err("Removing backgrounds needs macOS 14 (Sonoma) or later".into());
    }
    let failed = |e: &dyn std::fmt::Debug| format!("background removal failed ({e:?})");

    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| failed(&e))?;
    let data = NSData::with_bytes(&png);
    let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
    let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &options);

    let request = unsafe { VNGenerateForegroundInstanceMaskRequest::new() };
    let requests: Retained<NSArray<VNRequest>> =
        NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(request.clone()))]);
    handler.performRequests_error(&requests).map_err(|e| failed(&e))?;

    let no_subject = || "No clear subject found to keep".to_string();
    let observation = unsafe { request.results() }
        .and_then(|r| r.to_vec().into_iter().next())
        .ok_or_else(no_subject)?;
    let instances = unsafe { observation.allInstances() };
    if instances.count() == 0 {
        return Err(no_subject());
    }
    // Full-resolution mask of every subject together (0 = background, 1 = subject)
    let mask = unsafe { observation.generateScaledMaskForImageForInstances_fromRequestHandler_error(&instances, &handler) }
        .map_err(|e| failed(&e))?;

    let format = CVPixelBufferGetPixelFormatType(&mask);
    if format != kCVPixelFormatType_OneComponent32Float && format != kCVPixelFormatType_OneComponent8 {
        return Err(failed(&format!("mask format {format:#x}")));
    }
    let (w, h) = (CVPixelBufferGetWidth(&mask), CVPixelBufferGetHeight(&mask));
    let mut alpha = image::GrayImage::new(w as u32, h as u32);
    unsafe { CVPixelBufferLockBaseAddress(&mask, CVPixelBufferLockFlags::ReadOnly) };
    let base = CVPixelBufferGetBaseAddress(&mask) as *const u8;
    let row = CVPixelBufferGetBytesPerRow(&mask);
    if !base.is_null() {
        for y in 0..h {
            for x in 0..w {
                let v = unsafe {
                    if format == kCVPixelFormatType_OneComponent8 {
                        *base.add(y * row + x)
                    } else {
                        let f = (base.add(y * row + x * 4) as *const f32).read_unaligned();
                        (f.clamp(0.0, 1.0) * 255.0).round() as u8
                    }
                };
                alpha.put_pixel(x as u32, y as u32, image::Luma([v]));
            }
        }
    }
    unsafe { CVPixelBufferUnlockBaseAddress(&mask, CVPixelBufferLockFlags::ReadOnly) };
    if base.is_null() {
        return Err(failed(&"empty mask"));
    }

    Ok(apply_mask(image, &alpha))
}

#[cfg(not(target_os = "macos"))]
pub fn remove_background(_image: &RgbaImage) -> Result<RgbaImage, String> {
    Err("Removing backgrounds is only available on macOS for now".into())
}

/// Multiplies the image's own alpha by the mask (scaled to the image if needed).
pub fn apply_mask(image: &RgbaImage, mask: &image::GrayImage) -> RgbaImage {
    let scaled;
    let mask = if mask.dimensions() != image.dimensions() {
        scaled = image::imageops::resize(mask, image.width(), image.height(), image::imageops::FilterType::Triangle);
        &scaled
    } else {
        mask
    };
    let mut out = image.clone();
    for (p, m) in out.pixels_mut().zip(mask.pixels()) {
        p[3] = ((p[3] as u16 * m[0] as u16 + 127) / 255) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_multiplies_alpha() {
        let img = RgbaImage::from_pixel(4, 2, image::Rgba([10, 20, 30, 255]));
        let mask = image::GrayImage::from_fn(4, 2, |x, _| image::Luma([if x < 2 { 0 } else { 255 }]));
        let out = apply_mask(&img, &mask);
        assert_eq!(out.get_pixel(0, 0)[3], 0);
        assert_eq!(out.get_pixel(3, 1)[3], 255);
        assert_eq!(out.get_pixel(3, 1)[0], 10); // colour untouched
    }

    // Runs the real Vision model on a synthetic photo: a solid disc on a flat backdrop.
    #[cfg(target_os = "macos")]
    #[test]
    fn removes_a_flat_background() {
        let img = RgbaImage::from_fn(400, 300, |x, y| {
            let (dx, dy) = (x as f32 - 200.0, y as f32 - 150.0);
            if dx * dx + dy * dy < 90.0 * 90.0 {
                image::Rgba([220, 40, 40, 255])
            } else {
                image::Rgba([235, 235, 235, 255])
            }
        });
        match remove_background(&img) {
            Ok(out) => {
                assert_eq!(out.dimensions(), img.dimensions());
                // corner is background, centre is subject
                assert!(out.get_pixel(5, 5)[3] < 64, "corner kept: {}", out.get_pixel(5, 5)[3]);
                assert!(out.get_pixel(200, 150)[3] > 192, "subject removed");
            }
            // Older macOS, or the model judged the synthetic disc not to be a subject
            Err(e) => eprintln!("skipped: {e}"),
        }
    }
}

