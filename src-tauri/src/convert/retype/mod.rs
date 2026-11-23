// Upscale Text (Photo > Tools > Upscale > Text): documents, screenshots, chats and
// posters enlarged so their writing comes out crisp and right. An upscaler that draws
// detail draws letters too, and invents some: small text comes back as other words. Here
// no letter is invented:
// - JPEG artefacts are cleaned first (FBCNN), so neither letters nor paper keep speckle;
// - shapes, rules and panels are enlarged by a faithful upscaler (RealESRNet); where
//   text is, a faithful enlargement (Lanczos, sharpened) is kept instead;
// - the operating system's OCR reads the text, and each word it reads with certainty is
//   typed again over it: crisp, in the closest font, size, weight and ink colour, on the
//   picture's own ground (the old ink painted out).
// A word is typed again only when the evidence is clear: two readings agree on it (with
// and without the engine's language correction), it is a real word (or a number, date,
// address), and drawn it lies where the ink is. A line with many doubtful words, styled
// lettering (outlined, shadowed, a game's badge) and the dots and commas of punctuation
// keep the faithful pixels.

mod font;

use image::{DynamicImage, RgbImage, RgbaImage};

use super::encode::resize_exact;
use super::upscale::{self, tile_count, Models, Work};
use crate::office::ocr::{self, TextLine};
use crate::platform::upscale::Upscaler;
use font::{faces, Face};

/// What Upscale Text did
#[derive(Debug, Default, PartialEq)]
pub(super) struct Typed {
    /// Words typed again
    pub words: usize,
    /// Lines of text found, and how many of them were typed again
    pub lines: usize,
    pub lines_typed: usize,
}

/// The side, at most, of the picture the OCR reads: more is slow and adds nothing
const OCR_SIDE: f32 = 4000.0;

/// `img` made `factor` (2 or 4) times larger as a document. `report` hears how far it
/// has got: (done, total).
pub(super) fn upscale_text(
    img: &DynamicImage,
    factor: u32,
    models: &mut dyn Models,
    report: &dyn Fn(usize, usize),
) -> Result<(DynamicImage, Typed), String> {
    assert!(factor == 2 || factor == 4, "upscaling by {factor} is not supported");
    let s = factor as usize;
    let src = img.to_rgb8();
    let (w, h) = (src.width() as usize, src.height() as usize);

    let tiles = tile_count(w, h, Upscaler::Text);
    let mut work = Work::new(models.timings(), report);
    work.plan_with(&[(Upscaler::Text, tiles)], tiles, 0, 0);
    let cleaned = upscale::clean(&src, models, &mut work)?;
    let shapes = upscale::enlarge_with(&cleaned, s, Upscaler::Text, models, &mut work)?;
    models.learned(work.timings);
    // Faithful, a little crisper: an unsharp mask only steepens edges that are there
    let mut faithful = resize_exact(&DynamicImage::ImageRgb8(cleaned), (w * s) as u32, (h * s) as u32).to_rgb8();
    unsharp(&mut faithful, 0.6, s as f32 / 2.0);
    // The original enlarged as it is, where the ink is measured
    let plain = resize_exact(&DynamicImage::ImageRgb8(src.clone()), (w * s) as u32, (h * s) as u32).to_rgb8();

    // The text, read twice
    let lines = read(&src, s, true);
    let second = read(&src, s, false);

    // Shapes from the upscaler; around every line the faithful enlargement
    let mut out = shapes;
    let mut around = vec![0f32; w * s * h * s];
    for line in &lines {
        let [x, y, bw, bh] = line.rect;
        let pad = bh * 0.3;
        fill(&mut around, w * s, h * s, x - pad, y - pad, x + bw + pad, y + bh + pad, 1.0);
    }
    blur(&mut around, w * s, h * s, 3.0 * s as f32 / 4.0);
    blend(&mut out, &faithful, &around);

    let mut typed = Typed { lines: lines.len(), ..Typed::default() };
    let all: Vec<Face> = vec![];
    let faces = if faces().is_empty() { &all[..] } else { faces() };
    let fits: Vec<Fit> = lines.iter().filter_map(|line| fit(line, &lines, &src, &plain, &faithful, s, faces)).collect();
    let chosen = choose(&fits);
    for (fit, (face, height)) in fits.iter().zip(chosen) {
        // Styled lettering no plain font draws well is left as it is
        if fit.scores[face].0 > STYLED {
            continue;
        }
        if let Some(words) = draw(fit, &faces[face], fit.scores[face], height, &second, &faithful, &mut out, s) {
            typed.words += words;
            typed.lines_typed += 1;
        }
    }
    Ok((upscale::with_alpha(img, out), typed))
}

/// Above this, the closest font draws a line badly: lettering with an outline, shadow,
/// gradient or a face like no other
const STYLED: f32 = 0.2;

/// One reading of the text, in the result's pixels: lines with their words, each a box
/// [left, top, width, height]
struct Read {
    rect: [f32; 4],
    text: String,
    words: Vec<(String, [f32; 4])>,
}

/// The OCR's reading of `src`, enlarged so small text reads (to at most OCR_SIDE), in
/// pixels of the result (`s` times `src`). `corrected`: with the engine's language
/// correction. Windows' engine reads one way only: there the second reading is of the
/// picture at another size. Where there is no OCR (Linux), nothing is read.
fn read(src: &RgbImage, s: usize, corrected: bool) -> Vec<Read> {
    let (w, h) = (src.width() as f32, src.height() as f32);
    let mut zoom = (OCR_SIDE / w.max(h)).clamp(1.0, 4.0);
    if !corrected && !cfg!(target_os = "macos") {
        zoom = (zoom * 0.75).max(1.0);
    }
    let (zw, zh) = ((w * zoom).round() as u32, (h * zoom).round() as u32);
    let big: RgbaImage = resize_exact(&DynamicImage::ImageRgb8(src.clone()), zw, zh).to_rgba8();
    let Ok(lines) = ocr::recognize_with(&big, corrected) else { return vec![] };
    let (rw, rh) = (w * s as f32, h * s as f32);
    let px = |r: [f32; 4]| [r[0] * rw, r[1] * rh, (r[2] - r[0]) * rw, (r[3] - r[1]) * rh];
    lines
        .into_iter()
        .filter(|line: &TextLine| !line.words.is_empty())
        .map(|line| Read { rect: px(line.rect), text: line.text, words: line.words.into_iter().map(|(t, r)| (t, px(r))).collect() })
        .collect()
}

/// A line measured: where its ink is, and how well each face draws it
struct Fit<'a> {
    line: &'a Read,
    /// The region worked in, in the result's pixels (multiples of `s`), and in the
    /// original's
    region: [usize; 4],
    small: [usize; 4],
    /// The original's ink in the region (darkness above the paper, or lightness above
    /// the ground for light text), other lines' ink taken out
    ink_raw: Vec<f32>,
    /// Ink at the result's size, 0 to 1: of the plain enlargement, and of the faithful one
    ink_up: Vec<f32>,
    ink_base: Vec<f32>,
    ink_colour: [f32; 3],
    /// The line's ink box in the region: top, bottom, left, right
    ink_box: [usize; 4],
    /// Rows of `ink_raw` that are the line's own
    rows: [usize; 2],
    /// Per face: (mismatch 0 to 2, drawn top and bottom in the region, ink height at 100 px)
    scores: Vec<(f32, [usize; 2], usize)>,
}

fn gray(p: &[u8]) -> f32 {
    (f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2])) / 765.0
}

fn percentile(values: &[f32], share: f32) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * share) as usize]
}

/// Measures `line`, and draws it in every face to see which fits
fn fit<'a>(line: &'a Read, lines: &[Read], src: &RgbImage, plain: &RgbImage, base: &RgbImage, s: usize, faces: &[Face]) -> Option<Fit<'a>> {
    let (rw, rh) = (plain.width() as usize, plain.height() as usize);
    let [x, y, w, h] = line.rect;
    let pad = h * 0.25;
    let snap_down = |v: f32| ((v.max(0.0) as usize) / s) * s;
    let snap_up = |v: f32, most: usize| ((v.max(0.0).ceil() as usize).div_ceil(s) * s).min(most);
    let (x0, y0) = (snap_down(x - pad), snap_down(y - pad));
    let (x1, y1) = (snap_up(x + w + pad, rw), snap_up(y + h + pad, rh));
    if x1 <= x0 + s || y1 <= y0 + s {
        return None;
    }
    let (sx0, sy0, sx1, sy1) = (x0 / s, y0 / s, x1 / s, y1 / s);
    let (sw, sh) = (sx1 - sx0, sy1 - sy0);
    let g: Vec<f32> = (sy0..sy1).flat_map(|yy| (sx0..sx1).map(move |xx| (xx, yy))).map(|(xx, yy)| gray(&src.get_pixel(xx as u32, yy as u32).0)).collect();
    let (lo, hi) = (percentile(&g, 0.03), percentile(&g, 0.97));
    if hi - lo < 0.2 {
        return None;
    }
    let dark = g.iter().filter(|&&v| v < (lo + hi) / 2.0).count() * 2 < g.len();
    let mut ink_raw: Vec<f32> = g.iter().map(|&v| if dark { (hi - v).max(0.0) } else { (v - lo).max(0.0) }).collect();
    // Ink of the lines above and below (a paragraph's descenders and ascenders reach in)
    // is not this line's
    for other in lines {
        if std::ptr::eq(other, line) {
            continue;
        }
        let [ox, oy, ow, oh] = other.rect;
        if oy + oh < y0 as f32 || oy > y1 as f32 || ox + ow < x0 as f32 || ox > x1 as f32 {
            continue;
        }
        let (mine, theirs) = (y + h / 2.0, oy + oh / 2.0);
        let cut = (mine + theirs) / 2.0;
        let (a, b) = if theirs > mine { (cut, y1 as f32) } else { (y0 as f32, cut) };
        let (r0, r1) = (((a - y0 as f32).max(0.0) as usize) / s, ((b - y0 as f32).max(0.0) as usize).div_ceil(s).min(sh));
        let (c0, c1) = (((ox - x0 as f32).max(0.0) as usize) / s, ((ox + ow - x0 as f32).max(0.0) as usize).div_ceil(s).min(sw));
        for r in r0..r1 {
            for c in c0..c1 {
                ink_raw[r * sw + c] = 0.0;
            }
        }
    }
    // The ink and paper colours, from the faithful enlargement
    let (bw, bh) = (x1 - x0, y1 - y0);
    let reg: Vec<[f32; 3]> = (y0..y1)
        .flat_map(|yy| (x0..x1).map(move |xx| (xx, yy)))
        .map(|(xx, yy)| base.get_pixel(xx as u32, yy as u32).0.map(|v| f32::from(v) / 255.0))
        .collect();
    let gr: Vec<f32> = reg.iter().map(|p| (p[0] + p[1] + p[2]) / 3.0).collect();
    let mean_where = |keep: &dyn Fn(f32) -> bool| {
        let chosen: Vec<&[f32; 3]> = reg.iter().zip(&gr).filter(|(_, g)| keep(**g)).map(|(p, _)| p).collect();
        let n = chosen.len().max(1) as f32;
        [0, 1, 2].map(|c| chosen.iter().map(|p| p[c]).sum::<f32>() / n)
    };
    let (ink_at, paper_at) = if dark { (percentile(&gr, 0.015), percentile(&gr, 0.6)) } else { (percentile(&gr, 0.985), percentile(&gr, 0.4)) };
    let ink_colour = if dark { mean_where(&|g| g <= ink_at) } else { mean_where(&|g| g >= ink_at) };
    let paper_colour = if dark { mean_where(&|g| g >= paper_at) } else { mean_where(&|g| g <= paper_at) };
    let (pg, ig) = ((paper_colour[0] + paper_colour[1] + paper_colour[2]) / 3.0, (ink_colour[0] + ink_colour[1] + ink_colour[2]) / 3.0);
    let ink_base: Vec<f32> = gr.iter().map(|&v| if dark { ((pg - v) / (pg - ig).max(0.05)).clamp(0.0, 1.0) } else { ((v - pg) / (ig - pg).max(0.05)).clamp(0.0, 1.0) }).collect();
    let ink_up: Vec<f32> = (y0..y1)
        .flat_map(|yy| (x0..x1).map(move |xx| (xx, yy)))
        .map(|(xx, yy)| gray(&plain.get_pixel(xx as u32, yy as u32).0))
        .map(|v| if dark { ((hi - v) / (hi - lo)).clamp(0.0, 1.0) } else { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) })
        .collect();
    let [ty0, ty1, tx0, tx1] = ink_box(&ink_up, bw, bh)?;
    let rows = [(ty0 / s).saturating_sub(1), (ty1.div_ceil(s) + 1).min(sh)];

    // Every face, at a few heights, each word at its own place (the spaces between words,
    // a table's columns, are the picture's): the ink measured on a blurred enlargement is
    // taller than the letters by the blur, so the real height is searched for
    let runs = word_runs(&ink_up, bw, ty0, ty1);
    let placed: Vec<(&str, (usize, usize))> = line
        .words
        .iter()
        .filter_map(|(t, [wx, _, ww, _])| {
            let mine: Vec<&(usize, usize)> = runs.iter().filter(|r| { let c = x0 as f32 + (r.0 + r.1) as f32 / 2.0; c >= *wx && c <= wx + ww }).collect();
            Some((t.as_str(), (mine.first()?.0, mine.last()?.1)))
        })
        .collect();
    if placed.is_empty() {
        return None;
    }
    let inked: usize = placed.iter().map(|(_, (a, b))| b - a).sum();
    let text = line.text.as_str();
    let scores = faces
        .iter()
        .map(|face| {
            let Some(whole) = face.draw(text, 100.0).and_then(|d| d.ink(0.3)) else { return (f32::MAX, [0, 0], 1) };
            let drawn: Vec<Option<font::Drawn>> = placed.iter().map(|(t, _)| face.draw(t, 100.0).and_then(|d| d.ink(0.3))).collect();
            let natural: usize = drawn.iter().flatten().map(|d| d.w).sum();
            let mut best = (f32::MAX, [0, 0], whole.h);
            for k in [0.78, 0.84, 0.9, 0.96, 1.0] {
                let th = (((ty1 - ty0) as f32 * k).round() as usize).max(4);
                let scale = th as f32 / whole.h as f32;
                let spread = (ty1 - ty0) - th;
                // The words' own widths against their ink's (less the blur), within limits
                let ratio = (inked as f32 - (spread * placed.len()) as f32) / (natural as f32 * scale);
                if !(0.85..1.18).contains(&ratio) {
                    continue;
                }
                let oy = ty0 + spread / 2;
                let baseline = oy as f32 - whole.top as f32 * scale;
                let mut cov = vec![0f32; bw * bh];
                for (d, (_, (a, b))) in drawn.iter().zip(&placed) {
                    let Some(d) = d else { continue };
                    // Each word as wide as its own ink (less the blur), as tall as the face makes it
                    let natural_w = d.w as f32 * scale;
                    let own = ((b - a) as isize - spread as isize).max(1) as f32;
                    let dw = (if (0.8..1.25).contains(&(own / natural_w)) { own } else { natural_w }).round().max(1.0) as usize;
                    let dh = ((d.h as f32 * scale).round() as usize).max(1);
                    let x = (a + b) as isize / 2 - dw as isize / 2;
                    let y = (baseline + d.top as f32 * scale).round() as isize;
                    paste(&mut cov, bw, bh, &stretch(&d.coverage, d.w, d.h, dw, dh), dw, dh, x, y);
                }
                let small = shrink(&cov, bw, bh, s);
                let score = 1.0 - correlation(&small[rows[0] * sw..rows[1] * sw], &ink_raw[rows[0] * sw..rows[1] * sw]);
                if score < best.0 {
                    best = (score, [oy, oy + th], whole.h);
                }
            }
            best
        })
        .collect();
    Some(Fit { line, region: [x0, y0, x1, y1], small: [sx0, sy0, sx1, sy1], ink_raw, ink_up, ink_base, ink_colour, ink_box: [ty0, ty1, tx0, tx1], rows, scores })
}

/// A face and a height for every line. The face is decided by the whole row a line sits
/// in (a table's cells, a line's pieces), as a short cell alone can't tell bold from
/// regular nor one face's digits from another's; and lines in the same face and about the
/// same size (a paragraph, a column) share one height, so none comes out smaller.
fn choose(fits: &[Fit]) -> Vec<(usize, f32)> {
    let faces = faces();
    // Rows: lines whose middles are level
    let mut rows: Vec<Vec<usize>> = vec![];
    for (i, f) in fits.iter().enumerate() {
        let [_, y, _, h] = f.line.rect;
        let row = rows.iter_mut().find(|row| {
            let [_, ry, _, rh] = fits[row[0]].line.rect;
            ((y + h / 2.0) - (ry + rh / 2.0)).abs() < h.min(rh) * 0.6
        });
        match row {
            Some(row) => row.push(i),
            None => rows.push(vec![i]),
        }
    }
    // Per row, the one face that fits all its lines best together (a table's cells and a
    // line's pieces share their face; a date alone fits many faces almost equally)
    let mut chosen = vec![0usize; fits.len()];
    for row in &rows {
        let together = |face: usize| row.iter().map(|&i| fits[i].scores[face].0.min(1.0)).sum::<f32>();
        // Bold only when it clearly fits the row better than regular
        let best_with = |bold: bool| (0..faces.len()).filter(|&f| faces[f].bold == bold).min_by(|&a, &b| together(a).total_cmp(&together(b)));
        let face = match (best_with(false), best_with(true)) {
            (Some(r), Some(b)) if together(b) < together(r) - 0.005 * row.len() as f32 => b,
            (Some(r), _) => r,
            (None, Some(b)) => b,
            (None, None) => 0,
        };
        // A line the row's face draws badly is left as it is (STYLED), not typed in
        // another face
        for &i in row {
            chosen[i] = face;
        }
    }
    let height = |i: usize| {
        let [a, b] = fits[i].scores[chosen[i]].1;
        (b - a) as f32
    };
    (0..fits.len())
        .map(|i| {
            let mine = height(i);
            let mut same: Vec<f32> = (0..fits.len()).filter(|&j| chosen[j] == chosen[i] && (height(j) - mine).abs() <= 0.15 * mine).map(height).collect();
            same.sort_by(f32::total_cmp);
            (chosen[i], same[same.len() / 2])
        })
        .collect()
}

/// Punctuation before and after a word, and the word itself
fn split_affix(word: &str) -> (&str, &str, &str) {
    let core = |c: char| c.is_alphanumeric() || c == '_';
    let start = word.find(core).unwrap_or(word.len());
    let end = word.rfind(core).map_or(start, |i| i + word[i..].chars().next().map_or(1, char::len_utf8));
    (&word[..start], &word[start..end], &word[end..])
}

/// Whether a word may be typed again: a real word, or a number, date, time, amount,
/// e-mail or web address (judged by the readings agreeing and its pixels)
fn trusted(word: &str) -> bool {
    let (_, core, _) = split_affix(word);
    if core.is_empty() {
        return true;
    }
    // Numbers, dates, times, amounts: digits and their separators
    if core.chars().all(|c| c.is_ascii_digit() || ".,:/-+%()".contains(c)) {
        return true;
    }
    // A room or a code: a capital and digits (R.301, A4), Roman numerals (II, IV)
    let mut chars = core.chars();
    if chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.as_str().trim_start_matches('.').chars().all(|c| c.is_ascii_digit() || c == '.') && core.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if core.chars().all(|c| "IVX".contains(c)) {
        return true;
    }
    // Addresses: an @ or a dot between plain parts
    let plain = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "-_/".contains(c));
    if core.contains('@') || (core.contains('.') && core.split('.').all(plain)) {
        return core.split(['@', '.']).all(plain);
    }
    if core.contains('-') {
        return core.split('-').all(crate::platform::spell::known);
    }
    crate::platform::spell::known(core)
}

/// Types `fit`'s line again in `face`, at `height`, onto `out`. Returns the words typed,
/// or None when the line is left as it is (too many doubtful words).
#[allow(clippy::too_many_arguments)]
fn draw(fit: &Fit, face: &Face, score: (f32, [usize; 2], usize), height: f32, second: &[Read], base: &RgbImage, out: &mut RgbImage, s: usize) -> Option<usize> {
    let line = fit.line;
    let [x0, y0, x1, y1] = fit.region;
    let [sx0, _, sx1, _] = fit.small;
    let (bw, bh, sw) = (x1 - x0, y1 - y0, sx1 - sx0);
    let [ty0, ty1, tx0, tx1] = fit.ink_box;
    let [r0, r1] = fit.rows;
    let (_, [fa, fb], a_h) = score;
    // The line's size and baseline in that face (the height its block shares, centred on
    // this line's own)
    let mid = (fa + fb) as f32 / 2.0;
    let (fy0, fy1) = ((mid - height / 2.0).round() as isize, (mid + height / 2.0).round() as isize);
    let px = 100.0 * (fy1 - fy0) as f32 / a_h as f32;
    let top = face.draw(&line.text, px)?.ink(0.3)?.top;
    let baseline = fy0 - top as isize;
    let spread = (ty1 - ty0) as isize - (fy1 - fy0); // the blur's, both sides

    let [_, ly, _, lh] = line.rect;
    // The second reading's words on the same line, by their middles
    let second_words: Vec<(f32, &str)> = second
        .iter()
        .filter(|r| (r.rect[1] - ly).abs() < lh / 2.0)
        .flat_map(|r| r.words.iter().map(|(t, b)| (b[0] + b[2] / 2.0, t.as_str())))
        .collect();
    let runs = word_runs(&fit.ink_up, bw, ty0, ty1);
    let mut cov = vec![0f32; bw * bh];
    let mut keep = vec![0f32; bw * bh];
    let (mut doubtful, mut typed_spans, mut dropped, mut words) = (vec![], vec![], 0usize, 0usize);
    let pad_y = (6 * s / 4).max(2);
    for (text, [wx, _, ww, _]) in &line.words {
        // The runs of ink whose middle lies within the OCR's box of the word
        let mine: Vec<&(usize, usize)> = runs.iter().filter(|r| { let c = x0 as f32 + (r.0 + r.1) as f32 / 2.0; c >= *wx && c <= wx + ww }).collect();
        let (Some(first), Some(last)) = (mine.first(), mine.last()) else { continue };
        let (ix0, ix1) = (first.0, last.1);
        let (c0, c1) = (ix0 / s, ix1.div_ceil(s).min(sw));
        // What the word is, the OCR decides; typed again only when both readings agree
        // and it is a real word
        let (pre, core, post) = split_affix(text);
        let agreed = second_words.iter().any(|&(c, t)| c >= *wx && c <= wx + ww && split_affix(t).1 == core);
        if !core.is_empty() && (!agreed || !trusted(text)) {
            dropped += 1;
            doubtful.push((ix0, ix1));
            continue;
        }
        // Drawn at the word's place: its own ink's width (less the blur), on the line's
        // baseline, nudged a few pixels to where it fits best. A dot, comma, colon or
        // semicolon is a pixel or two in a small picture: never drawn, its pixels stay
        let Some(drawn) = face.draw(text, px) else { continue };
        let Some(ink) = drawn.ink(0.3) else { continue };
        let width = ((ix1 - ix0) as isize - spread).max(2) as usize;
        if !(0.8..1.25).contains(&(width as f32 / ink.w as f32)) {
            dropped += 1;
            doubtful.push((ix0, ix1));
            continue;
        }
        let rows_full: Vec<f32> = (0..drawn.h)
            .flat_map(|yy| (0..ink.w).map(move |xx| (xx, yy)))
            .map(|(xx, yy)| drawn.coverage[yy * drawn.w + (xx as i32 + ink.left - drawn.left) as usize])
            .collect();
        let mut glyphs = stretch(&rows_full, ink.w, drawn.h, width, drawn.h);
        let punctuation = |a: &str| a.chars().any(|c| ".,:;".contains(c));
        let full = face.advance(text, px).max(1.0);
        let core_start = if pre.is_empty() { 0 } else { (width as f32 * face.advance(pre, px) / full).round() as usize };
        let core_end = if post.is_empty() { width } else { (width as f32 * face.advance(&format!("{pre}{core}"), px) / full).round() as usize };
        for yy in 0..drawn.h {
            for xx in 0..width {
                if (xx < core_start && punctuation(pre)) || (xx >= core_end && punctuation(post)) {
                    glyphs[yy * width + xx] = 0.0;
                }
            }
        }
        let (gy, gx) = (baseline + drawn.top as isize, ix0 as isize + ((ix1 - ix0) as isize - width as isize) / 2);
        let step = (s / 2).max(1) as isize;
        let mut best: Option<(f32, Vec<f32>)> = None;
        for dy in [-2, -1, 0, 1, 2] {
            for dx in [-2, -1, 0, 1, 2] {
                let mut c = vec![0f32; bw * bh];
                paste(&mut c, bw, bh, &glyphs, width, drawn.h, gx + dx * step, gy + dy * step);
                let small = shrink(&c, bw, bh, s);
                let (mut fit_ab, mut fit_aa, mut real) = (0f32, 0f32, 0f32);
                for r in r0..r1 {
                    for col in c0..c1 {
                        let (a, b) = (small[r * sw + col], fit.ink_raw[r * sw + col]);
                        fit_ab += a * b;
                        fit_aa += a * a;
                        real += b;
                    }
                }
                let alpha = fit_ab / fit_aa.max(1e-6);
                let mut miss = 0f32;
                for r in r0..r1 {
                    for col in c0..c1 {
                        miss += (small[r * sw + col] * alpha - fit.ink_raw[r * sw + col]).abs();
                    }
                }
                let rel = miss / real.max(1e-3);
                if best.as_ref().is_none_or(|b| rel < b.0) {
                    best = Some((rel, c));
                }
            }
        }
        let Some((rel, c)) = best else { continue };
        // The drawing must be where the ink is (a gross check of placement and size)
        if rel >= 0.8 {
            dropped += 1;
            doubtful.push((ix0, ix1));
            continue;
        }
        let cols: Vec<usize> = (0..bw).filter(|&xx| (0..bh).any(|yy| c[yy * bw + xx] > 0.0)).collect();
        let (Some(&cmin), Some(&cmax)) = (cols.first(), cols.last()) else { continue };
        for yy in ty0.saturating_sub(pad_y)..(ty1 + pad_y).min(bh) {
            for xx in cmin.saturating_sub(3)..(cmax + 4).min(bw) {
                keep[yy * bw + xx] = 1.0;
            }
        }
        for (v, n) in cov.iter_mut().zip(&c) {
            *v = v.max(*n);
        }
        typed_spans.push((cmin.min(ix0 + core_start), (cmax + 1).max(ix0 + core_end)));
        words += 1;
    }
    if dropped * 4 > line.words.len().max(1) || words == 0 {
        return None;
    }
    // The rest of the line's ink (punctuation, rules, doubtful words) keeps its own
    // shape, given the same crisp edge and ink colour; the old ink of a typed word is
    // gone entirely
    let mut band = vec![0f32; bw * bh];
    for yy in ty0.saturating_sub(pad_y)..(ty1 + pad_y).min(bh) {
        for xx in tx0.saturating_sub(4)..(tx1 + 4).min(bw) {
            band[yy * bw + xx] = 1.0;
        }
    }
    let clear = |v: &mut Vec<f32>, spans: &[(usize, usize)]| {
        for &(a, b) in spans {
            for yy in 0..bh {
                for xx in a.saturating_sub(3)..(b + 3).min(bw) {
                    v[yy * bw + xx] = 0.0;
                }
            }
        }
    };
    // A doubtful word isn't typed: its own ink keeps its shape, with the same crisp edge
    // and ink colour as the rest, so it doesn't stand out blurred among typed words
    let mut allowed = band.clone();
    clear(&mut allowed, &typed_spans);
    for i in 0..bw * bh {
        let t = ((fit.ink_base[i] - 0.5) / 0.2 * 0.5 + 0.5).clamp(0.0, 1.0);
        cov[i] = cov[i].max(t * t * (3.0 - 2.0 * t) * allowed[i]);
        keep[i] = keep[i].max(band[i]);
    }
    blur(&mut keep, bw, bh, 1.5 * s as f32 / 4.0);
    // Behind it, the ground as it is (a coloured table row, a panel): the faithful
    // enlargement with its old ink painted out
    let mut old_ink: Vec<bool> = fit.ink_base.iter().map(|&v| v > 0.15).collect();
    dilate(&mut old_ink, bw, bh, 3);
    let ground = inpaint(base, [x0, y0, x1, y1], &old_ink);
    let ink = fit.ink_colour;
    for yy in 0..bh {
        for xx in 0..bw {
            let i = yy * bw + xx;
            if keep[i] <= 0.0 {
                continue;
            }
            let p = out.get_pixel_mut((x0 + xx) as u32, (y0 + yy) as u32);
            for c in 0..3 {
                let drawn = ground[i][c] * (1.0 - cov[i]) + ink[c] * cov[i];
                let v = f32::from(p.0[c]) / 255.0 * (1.0 - keep[i]) + drawn * keep[i];
                p.0[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    Some(words)
}

/// The line's own ink in a region's inkness map: the run of inked rows through its
/// middle (a paragraph's neighbouring lines reach in), then the columns inked within it.
/// [top, bottom, left, right]
fn ink_box(ink: &[f32], w: usize, h: usize) -> Option<[usize; 4]> {
    let on: Vec<bool> = (0..h).map(|y| ink[y * w..(y + 1) * w].iter().filter(|&&v| v > 0.5).count() as f32 / w as f32 > 0.004).collect();
    let (mid, reach) = (h / 2, h / 6);
    let window = mid.saturating_sub(reach)..(mid + reach + 1).min(h);
    let centre = if on[mid] { mid } else { window.clone().find(|&y| on[y])? };
    let (mut top, mut bottom) = (centre, centre);
    while top > 0 && on[top - 1] {
        top -= 1;
    }
    while bottom + 1 < h && on[bottom + 1] {
        bottom += 1;
    }
    let cols: Vec<usize> = (0..w).filter(|&x| (top..=bottom).any(|y| ink[y * w + x] > 0.5)).collect();
    Some([top, bottom + 1, *cols.first()?, *cols.last()? + 1])
}

/// The words as the picture shows them: runs of inked columns between spaces
fn word_runs(ink: &[f32], w: usize, top: usize, bottom: usize) -> Vec<(usize, usize)> {
    let gap = ((0.16 * (bottom - top) as f32) as usize).max(3);
    let mut runs = vec![];
    let (mut start, mut blank) = (None, 0);
    for x in 0..w {
        if (top..bottom).any(|y| ink[y * w + x] > 0.5) {
            start.get_or_insert(x);
            blank = 0;
        } else if let Some(s) = start {
            blank += 1;
            if blank >= gap {
                runs.push((s, x + 1 - blank));
                start = None;
            }
        }
    }
    if let Some(s) = start {
        runs.push((s, w - blank));
    }
    runs
}

/// Pearson correlation: how alike two shapes are, whatever their darkness
fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().max(1) as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut ab, mut aa, mut bb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        ab += (x - ma) * (y - mb);
        aa += (x - ma) * (x - ma);
        bb += (y - mb) * (y - mb);
    }
    ab / ((aa * bb).sqrt() + 1e-6)
}

/// `values` (w x h) resized to tw x th (bilinear)
fn stretch(values: &[f32], w: usize, h: usize, tw: usize, th: usize) -> Vec<f32> {
    let img = image::ImageBuffer::<image::Luma<f32>, Vec<f32>>::from_raw(w as u32, h as u32, values.to_vec()).expect("sized");
    image::imageops::resize(&img, tw as u32, th as u32, image::imageops::FilterType::Triangle).into_raw()
}

/// `piece` (pw x ph) laid onto `onto` (w x h) with its top left at (x, y), clipped
#[allow(clippy::too_many_arguments)]
fn paste(onto: &mut [f32], w: usize, h: usize, piece: &[f32], pw: usize, ph: usize, x: isize, y: isize) {
    for py in 0..ph {
        let yy = y + py as isize;
        if yy < 0 || yy >= h as isize {
            continue;
        }
        for px in 0..pw {
            let xx = x + px as isize;
            if xx >= 0 && xx < w as isize {
                onto[yy as usize * w + xx as usize] = piece[py * pw + px];
            }
        }
    }
}

/// A coverage map at the result's size brought to the original's: the average of each
/// s x s block, then the original's own softness (a slight blur)
fn shrink(values: &[f32], w: usize, h: usize, s: usize) -> Vec<f32> {
    let (sw, sh) = (w / s, h / s);
    let mut small = vec![0f32; sw * sh];
    for y in 0..sh {
        for x in 0..sw {
            let mut sum = 0.0;
            for dy in 0..s {
                for dx in 0..s {
                    sum += values[(y * s + dy) * w + x * s + dx];
                }
            }
            small[y * sw + x] = sum / (s * s) as f32;
        }
    }
    blur(&mut small, sw, sh, 0.6);
    small
}

/// A gaussian blur of `sigma` pixels (separable, edges repeated)
fn blur(values: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return;
    }
    let radius = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = kernel.iter().sum();
    let mut across = vec![0f32; values.len()];
    for y in 0..h {
        for x in 0..w {
            across[y * w + x] = kernel.iter().enumerate().map(|(k, wt)| values[y * w + (x as isize + k as isize - radius).clamp(0, w as isize - 1) as usize] * wt).sum::<f32>() / total;
        }
    }
    for y in 0..h {
        for x in 0..w {
            values[y * w + x] = kernel.iter().enumerate().map(|(k, wt)| across[(y as isize + k as isize - radius).clamp(0, h as isize - 1) as usize * w + x] * wt).sum::<f32>() / total;
        }
    }
}

/// Grows the true cells of a mask by `r` in every direction
fn dilate(mask: &mut [bool], w: usize, h: usize, r: usize) {
    let src = mask.to_vec();
    for y in 0..h {
        for x in 0..w {
            mask[y * w + x] = (y.saturating_sub(r)..(y + r + 1).min(h)).any(|yy| (x.saturating_sub(r)..(x + r + 1).min(w)).any(|xx| src[yy * w + xx]));
        }
    }
}

/// The region of `img` with the `hole` filled from what surrounds it (push-pull: the
/// known pixels averaged down a pyramid, then the holes filled from coarser levels on
/// the way back up), for the ground behind text
fn inpaint(img: &RgbImage, [x0, y0, x1, y1]: [usize; 4], hole: &[bool]) -> Vec<[f32; 3]> {
    let (w, h) = (x1 - x0, y1 - y0);
    let mut level: Vec<([f32; 3], f32)> = (0..w * h)
        .map(|i| {
            let p = img.get_pixel((x0 + i % w) as u32, (y0 + i / w) as u32).0.map(|v| f32::from(v) / 255.0);
            if hole[i] { ([0.0; 3], 0.0) } else { (p, 1.0) }
        })
        .collect();
    let mut levels = vec![];
    let (mut lw, mut lh) = (w, h);
    while (lw > 1 || lh > 1) && level.iter().any(|(_, k)| *k < 1.0) {
        let (nw, nh) = (lw.div_ceil(2), lh.div_ceil(2));
        let mut next = vec![([0f32; 3], 0f32); nw * nh];
        for y in 0..lh {
            for x in 0..lw {
                let (p, k) = level[y * lw + x];
                let n = &mut next[(y / 2) * nw + x / 2];
                for (sum, v) in n.0.iter_mut().zip(p) {
                    *sum += v * k;
                }
                n.1 += k;
            }
        }
        for n in next.iter_mut() {
            if n.1 > 0.0 {
                n.0 = n.0.map(|v| v / n.1);
                n.1 = 1.0;
            }
        }
        levels.push((level, lw, lh));
        level = next;
        (lw, lh) = (nw, nh);
    }
    while let Some((mut finer, fw, fh)) = levels.pop() {
        for y in 0..fh {
            for x in 0..fw {
                let i = y * fw + x;
                if finer[i].1 < 1.0 {
                    let coarse = level[(y / 2).min(lh - 1) * lw + (x / 2).min(lw - 1)].0;
                    let k = finer[i].1;
                    finer[i] = ([0, 1, 2].map(|c| finer[i].0[c] * k + coarse[c] * (1.0 - k)), 1.0);
                }
            }
        }
        level = finer;
        (lw, lh) = (fw, fh);
    }
    level.into_iter().map(|(p, _)| p).collect()
}

/// Sets the cells of a w x h map inside a box to `value`
#[allow(clippy::too_many_arguments)]
fn fill(map: &mut [f32], w: usize, h: usize, x0: f32, y0: f32, x1: f32, y1: f32, value: f32) {
    for y in (y0.max(0.0) as usize)..(y1.max(0.0) as usize).min(h) {
        for x in (x0.max(0.0) as usize)..(x1.max(0.0) as usize).min(w) {
            map[y * w + x] = value;
        }
    }
}

/// `onto` with `over` laid over it where `mask` (0 to 1) says
fn blend(onto: &mut RgbImage, over: &RgbImage, mask: &[f32]) {
    for ((p, q), &m) in onto.pixels_mut().zip(over.pixels()).zip(mask) {
        if m > 0.0 {
            for (v, w) in p.0.iter_mut().zip(q.0) {
                *v = (f32::from(*v) * (1.0 - m) + f32::from(w) * m).round() as u8;
            }
        }
    }
}

/// An unsharp mask: `amount` of the detail finer than `sigma` added again
fn unsharp(img: &mut RgbImage, amount: f32, sigma: f32) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    for c in 0..3 {
        let plane: Vec<f32> = img.pixels().map(|p| f32::from(p.0[c])).collect();
        let mut soft = plane.clone();
        blur(&mut soft, w, h, sigma);
        for (i, p) in img.pixels_mut().enumerate() {
            p.0[c] = (plane[i] + amount * (plane[i] - soft[i])).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests;
