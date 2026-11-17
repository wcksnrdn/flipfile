//! Scanned pages: text recognised by OCR, laid over the scan with the text taken out

use super::*;

/// A page read by text recognition: its size (points) and the lines found on it
pub struct OcrPage {
    pub width: f32,
    pub height: f32,
    pub lines: Vec<crate::office::ocr::TextLine>,
    /// The page as scanned: with the recognised text taken out, it becomes the page's
    /// background (logos, lines, tables, photos, signatures stay where they were)
    pub scan: Option<image::RgbaImage>,
}

/// Font sizes from text boxes vary with the letters in them (descenders, capitals):
/// sizes close to the page's usual body size snap to it, headings and small print
/// keep their own
fn snap_sizes(sizes: &[f32]) -> Vec<f32> {
    let mut sorted: Vec<f32> = sizes.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The size most lines are near
    let body = sorted
        .iter()
        .map(|&s| (s, sorted.iter().filter(|&&o| (o - s).abs() <= s * 0.2).count()))
        .max_by_key(|&(_, n)| n)
        .map(|(s, _)| s)
        .unwrap_or(11.0);
    let near: Vec<f32> = sorted.iter().copied().filter(|s| (s - body).abs() <= body * 0.25).collect();
    let body = near.iter().sum::<f32>() / near.len().max(1) as f32;
    // Recognised boxes of short pieces without tall letters ("semakin") come out much
    // lower than the text really is: anything from 2/3 to 1.3 x is body text
    sizes
        .iter()
        .map(|&s| if s >= body * 0.66 && s <= body * 1.3 { body } else { s })
        .map(|s| (s * 2.0).round() / 2.0)
        .collect()
}

/// Erases one recognised box from the scan, row by row, blending from the colour just
/// left of it to the colour just right of it (smooth on gradients and photos), and
/// returns the colour of the ink that was there
fn erase_box(img: &mut image::RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32) -> [u8; 3] {
    let (w, _) = img.dimensions();
    let sample = |img: &image::RgbaImage, x: u32, y: u32| -> [f32; 3] {
        let p = img.get_pixel(x.min(w - 1), y);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    // Ink: the pixels that differ most from the paper around them
    let mut ink_sum = [0f32; 3];
    let mut ink_n = 0f32;
    let mut rows: Vec<([f32; 3], [f32; 3])> = vec![];
    for y in y0..=y1 {
        let left = sample(img, x0.saturating_sub(2), y);
        let right = sample(img, x1 + 2, y);
        rows.push((left, right));
        let paper = [(left[0] + right[0]) / 2.0, (left[1] + right[1]) / 2.0, (left[2] + right[2]) / 2.0];
        for x in (x0..=x1).step_by(2) {
            let p = sample(img, x, y);
            let d = (0..3).map(|i| (p[i] - paper[i]).abs()).sum::<f32>();
            if d > 150.0 {
                (0..3).for_each(|i| ink_sum[i] += p[i]);
                ink_n += 1.0;
            }
        }
    }
    for (k, y) in (y0..=y1).enumerate() {
        let (left, right) = rows[k];
        let span = (x1 - x0).max(1) as f32;
        for x in x0..=x1 {
            let t = (x - x0) as f32 / span;
            let c = [0, 1, 2].map(|i| (left[i] + (right[i] - left[i]) * t).round().clamp(0.0, 255.0) as u8);
            img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
        }
    }
    if ink_n > 0.0 {
        [0, 1, 2].map(|i| (ink_sum[i] / ink_n).round() as u8)
    } else {
        [0, 0, 0]
    }
}

/// The scan with every recognised line erased, and each line's text colour
fn erase_text(scan: &image::RgbaImage, lines: &[crate::office::ocr::TextLine]) -> (image::RgbaImage, Vec<[u8; 3]>) {
    let mut img = scan.clone();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let mut colors = vec![];
    for line in lines {
        let [l, t, r, b] = line.rect;
        let pad_y = (b - t) * h * 0.18;
        let pad_x = (b - t) * h * 0.3;
        let x0 = ((l * w - pad_x).max(0.0)) as u32;
        let y0 = ((t * h - pad_y).max(0.0)) as u32;
        let x1 = ((r * w + pad_x).min(w - 1.0)) as u32;
        let y1 = ((b * h + pad_y).min(h - 1.0)) as u32;
        if x1 <= x0 || y1 <= y0 {
            colors.push([0, 0, 0]);
            continue;
        }
        colors.push(erase_box(&mut img, x0, y0, x1, y1));
    }
    (img, colors)
}

/// Recognised text -> .docx, through the same layout reconstruction as PDF text:
/// every recognised line becomes characters spread over its box, so paragraphs,
/// alignment, indents, columns, running headers/footers and page numbers come out
/// the same way. (The scan itself is not kept: the result is the editable text.)
pub fn ocr_to_docx(pages: Vec<OcrPage>) -> Result<Vec<u8>, String> {
    if pages.iter().all(|p| p.lines.is_empty()) {
        return Err("no text was found to recognise".into());
    }
    let mut media = MediaStore::default();
    let mut models = vec![];
    for page in &pages {
        let mut glyphs = vec![];
        let (erased, colors) = match &page.scan {
            Some(scan) => {
                let (img, colors) = erase_text(scan, &page.lines);
                (Some(img), colors)
            }
            None => (None, vec![[0, 0, 0]; page.lines.len()]),
        };
        // A text box spans ascenders to descenders: about 1.2 x the font size. The
        // median word of a line decides, so one tall mark (a superscript) does not
        let box_height = |l: &crate::office::ocr::TextLine| -> f32 {
            let mut hs: Vec<f32> = l.words.iter().filter(|w| w.0.chars().count() >= 2).map(|w| w.1[3] - w.1[1]).collect();
            if hs.is_empty() {
                return l.rect[3] - l.rect[1];
            }
            hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            hs[hs.len() / 2]
        };
        let raw: Vec<f32> = page.lines.iter().map(|l| (box_height(l) * page.height / 1.2).clamp(4.0, 72.0)).collect();
        let sizes = snap_sizes(&raw);
        for ((line, &size), &ink) in page.lines.iter().zip(&sizes).zip(&colors) {
            let [l, t, r, b] = line.rect;
            let (l, t, r, b) = (l * page.width, t * page.height, r * page.width, b * page.height);
            let _ = t;
            let h = (box_height(line) * page.height).max(1.0);
            let baseline = b - h * 0.2;
            // Dark ink is plain text colour; anything else keeps its colour
            let color = (ink.iter().any(|v| *v > 90)).then_some(ink);
            let style = Style { font: "Arial".into(), size, bold: false, italic: false, underline: false, color, spacing: 0.0, vert: 0 };
            // Characters spread evenly over each word's box (or the whole line's)
            let pieces: Vec<(String, f32, f32)> = if line.words.is_empty() {
                vec![(line.text.clone(), l, r)]
            } else {
                line.words.iter().map(|(w, rect)| (w.clone(), rect[0] * page.width, rect[2] * page.width)).collect()
            };
            glyphs.push(Glyph { ch: '\n', left: l, right: l, baseline, style: style.clone(), generated: false });
            for (k, (word, wl, wr)) in pieces.iter().enumerate() {
                let chars: Vec<char> = word.chars().collect();
                let step = (wr - wl) / chars.len().max(1) as f32;
                if k > 0 {
                    glyphs.push(Glyph { ch: ' ', left: *wl, right: *wl, baseline, style: style.clone(), generated: false });
                }
                for (i, ch) in chars.iter().enumerate() {
                    let left = wl + step * i as f32;
                    glyphs.push(Glyph { ch: *ch, left, right: left + step * 0.95, baseline, style: style.clone(), generated: false });
                }
            }
        }
        let refs: Vec<&Glyph> = glyphs.iter().collect();
        let text_right = glyphs.iter().map(|g| g.right).fold(0.0, f32::max).max(page.width * 0.6);
        let body = flow(&refs, vec![], text_right, &[], vec![]);
        // What is left of the scan without its text, unless that is just blank paper
        let background = erased.and_then(|erased| {
            let max_w = (page.width * 150.0 / 72.0) as u32;
            let small = if erased.width() > max_w {
                image::imageops::resize(&erased, max_w, (erased.height() as f32 * max_w as f32 / erased.width() as f32) as u32, image::imageops::FilterType::Triangle)
            } else {
                erased
            };
            let ink = small.pixels().filter(|p| p[0].min(p[1]).min(p[2]) < 200).count();
            if ink * 1000 < small.pixels().len() {
                return None;
            }
            let mut jpg = Vec::new();
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 80);
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(small).to_rgb8()).write_with_encoder(encoder).ok()?;
            Some(media.add(jpg, "jpeg"))
        });
        models.push(PageModel {
            width: page.width,
            height: page.height,
            header: vec![],
            body,
            footer: vec![],
            decor: vec![],
            background,
            number: 0,
            underline_rules: vec![],
            fills: vec![],
        });
    }
    number_pages(&mut models);
    split_running_content(&mut models);
    writer::write(&models, &media)
}
