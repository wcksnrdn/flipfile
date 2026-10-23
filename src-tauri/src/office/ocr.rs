// Text recognition (OCR) with the operating system's own engine, so nothing has to be
// bundled or installed:
// - macOS: the Vision framework (the engine behind Live Text).
// - Windows: Windows.Media.Ocr (Windows 10/11), in the languages installed for the user.
// - Linux: there is no built-in engine; the user gets a clear message.
//
// Each recognised line comes back with its text and its box on the image, in
// fractions of the image size (0..1, origin at the top-left).

/// One line of recognised text
#[derive(Clone, Debug)]
pub struct TextLine {
    pub text: String,
    /// left, top, right, bottom as fractions of the image (top-left origin)
    pub rect: [f32; 4],
    /// Each word with its own box (same units), when the engine gives them: words far
    /// apart on one "line" (two columns, a table row) stay apart
    pub words: Vec<(String, [f32; 4])>,
}

#[cfg(target_os = "macos")]
pub fn recognize(image: &image::RgbaImage) -> Result<Vec<TextLine>, String> {
    // Conversions run on worker threads, which have no autorelease pool of their own
    objc2::rc::autoreleasepool(|_| recognize_in_pool(image))
}

#[cfg(target_os = "macos")]
fn recognize_in_pool(image: &image::RgbaImage) -> Result<Vec<TextLine>, String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
    use objc2_vision::{VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

    let failed = |e: &dyn std::fmt::Debug| format!("text recognition failed ({e:?})");
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone()).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| failed(&e))?;
    let data = NSData::with_bytes(&png);
    let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
    let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &options);

    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    // Indonesian and English first when this macOS knows them, else its own choice
    let wanted = ["id-ID", "en-US"];
    if let Ok(supported) = unsafe { request.supportedRecognitionLanguagesAndReturnError() } {
        let known: Vec<String> = supported.iter().map(|s| s.to_string()).collect();
        let languages: Vec<Retained<NSString>> = wanted.iter().filter(|w| known.iter().any(|k| k == *w)).map(|w| NSString::from_str(w)).collect();
        if !languages.is_empty() {
            request.setRecognitionLanguages(&NSArray::from_retained_slice(&languages));
        }
    }
    let requests: Retained<NSArray<VNRequest>> = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(request.clone()))]);
    handler.performRequests_error(&requests).map_err(|e| failed(&e))?;

    let mut out = vec![];
    for observation in request.results().map(|r| r.to_vec()).unwrap_or_default() {
        let Some(best) = observation.topCandidates(1).to_vec().into_iter().next() else { continue };
        // Dotless ı (Turkish) only shows up as a misreading of i in these languages
        let text = best.string().to_string().replace('ı', "i");
        if text.trim().is_empty() {
            continue;
        }
        // Vision measures from the bottom-left corner
        let flip = |b: objc2_core_foundation::CGRect| -> [f32; 4] {
            let (x, y, w, h) = (b.origin.x as f32, b.origin.y as f32, b.size.width as f32, b.size.height as f32);
            [x, 1.0 - (y + h), x + w, 1.0 - y]
        };
        let rect = flip(unsafe { observation.boundingBox() });
        // Every word's own box (ranges count UTF-16 units, as NSString does)
        let mut words = vec![];
        let mut offset = 0usize;
        for piece in text.split(' ') {
            let len = piece.encode_utf16().count();
            if len > 0 {
                let range = objc2_foundation::NSRange::new(offset, len);
                let mut error: *mut objc2_foundation::NSError = std::ptr::null_mut();
                let found: Option<Retained<objc2_vision::VNRectangleObservation>> =
                    unsafe { objc2::msg_send![&*best, boundingBoxForRange: range, error: &mut error] };
                if let Some(word) = found {
                    words.push((piece.to_string(), flip(unsafe { word.boundingBox() })));
                }
            }
            offset += len + 1;
        }
        if words.iter().map(|w| w.0.chars().count()).sum::<usize>() < text.split_whitespace().map(|w| w.chars().count()).sum::<usize>() {
            words.clear(); // incomplete: the line's box is used instead
        }
        out.push(TextLine { text, rect, words });
    }
    Ok(out)
}

#[cfg(windows)]
pub fn recognize(image: &image::RgbaImage) -> Result<Vec<TextLine>, String> {
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Security::Cryptography::CryptographicBuffer;

    let failed = |e: windows::core::Error| format!("text recognition failed ({e})");
    let run = || -> windows::core::Result<Vec<TextLine>> {
        // The engine's own size limit
        let max = OcrEngine::MaxImageDimension().unwrap_or(2600).max(1000);
        let (w, h) = image.dimensions();
        let scaled;
        let image = if w.max(h) > max {
            let k = max as f32 / w.max(h) as f32;
            scaled = image::imageops::resize(image, (w as f32 * k) as u32, (h as f32 * k) as u32, image::imageops::FilterType::Triangle);
            &scaled
        } else {
            image
        };
        let (w, h) = image.dimensions();
        // BGRA, as the engine wants it
        let mut bgra = image.as_raw().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let buffer = CryptographicBuffer::CreateFromByteArray(&bgra)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32)?;
        // The languages the user has installed in Windows
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut out = vec![];
        for line in result.Lines()? {
            let mut text = String::new();
            let mut words = vec![];
            let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for word in line.Words()? {
                let rect = word.BoundingRect()?;
                if !text.is_empty() {
                    text.push(' ');
                }
                let word_text = word.Text()?.to_string();
                text.push_str(&word_text);
                let (fw, fh) = (w as f32, h as f32);
                words.push((word_text, [rect.X / fw, rect.Y / fh, (rect.X + rect.Width) / fw, (rect.Y + rect.Height) / fh]));
                l = l.min(rect.X);
                t = t.min(rect.Y);
                r = r.max(rect.X + rect.Width);
                b = b.max(rect.Y + rect.Height);
            }
            if !text.trim().is_empty() {
                out.push(TextLine { text, rect: [l / w as f32, t / h as f32, r / w as f32, b / h as f32], words });
            }
        }
        Ok(out)
    };
    run().map_err(failed)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn recognize(_image: &image::RgbaImage) -> Result<Vec<TextLine>, String> {
    Err("text recognition (OCR) is not available on Linux yet".into())
}

#[cfg(test)]
mod tests {
    /// `FLIPFILE_SCAN=page.png cargo test dump_ocr -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_ocr() {
        let img = image::open(std::env::var("FLIPFILE_SCAN").unwrap()).unwrap().to_rgba8();
        for line in super::recognize(&img).unwrap().iter().take(12) {
            println!("{:?} words={} {:?}", line.rect.map(|v| (v * 1000.0).round() / 1000.0), line.words.len(), line.text);
        }
    }
}
