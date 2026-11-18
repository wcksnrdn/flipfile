// PDF -> Excel (.xlsx). The page layout is read like PDF -> Word does (`from_pdf`):
// characters with their fonts, ruling lines that form tables, and lines of text whose
// wide gaps mark columns. From that:
//
// - tables drawn with lines become cells with their merges, borders, shading and row
//   heights;
// - tables drawn without lines (bank statements, invoices, reports) are found from
//   lines of text whose columns line up, with wrapped descriptions kept in their cell;
// - a table running over several pages becomes one table, its repeated heading row
//   dropped;
// - text becomes real numbers, percentages, amounts and dates (Indonesian and English
//   notation, worked out per document), while codes, account and ID numbers stay text;
// - the rest of the text (titles, notes) keeps its row, size and weight.
//
// Everything goes on one sheet, top to bottom, the way the pages read.

use std::collections::BTreeMap;

use pdfium_render::prelude::*;

use super::from_pdf::{self, Block, Content, Line, PageModel, Style};
use super::xlsx_writer::{self as w, Edge, Look, Value};

/// Default row height in points (Calibri 11)
const ROW: f32 = 15.0;

// Values first, then the layout. The submodules share one namespace (each starts with
// `use super::*`), as when this was a single file.
mod builder;
mod page;
mod segments;
mod tables;
mod values;

#[cfg(test)]
mod tests;

use builder::*;
use page::*;
use segments::*;

// Public in the single file too: how text reads as a number, amount or date
pub use values::{notation, typed, Notation};

/// Reads the pages of a PDF the way PDF -> Word does
fn pages(pdfium: &Pdfium, pdf: &[u8], on_page: &dyn Fn(usize, usize)) -> Result<Vec<PageModel>, String> {
    let document = pdfium.load_pdf_from_byte_slice(pdf, None).map_err(|e| {
        if format!("{e:?}").contains("Password") { "the PDF is password-protected".to_string() } else { "the PDF could not be opened".to_string() }
    })?;
    let total = document.pages().len() as usize;
    if total == 0 {
        return Err("the PDF has no pages".into());
    }
    let mut media = from_pdf::MediaStore::default();
    let mut models = vec![];
    for (index, page) in document.pages().iter().enumerate() {
        on_page(index, total);
        // a gap of one font size already separates two columns' values
        models.push(from_pdf::with_tab_gap(0.5, || from_pdf::page_model(&page, &document, &mut media)));
    }
    if models.iter().all(|m| m.body.is_empty()) {
        // a scan: the text recognised on each page, by the system's own engine
        models.clear();
        for (index, page) in document.pages().iter().enumerate() {
            on_page(index, total);
            let (w, h) = (page.width().value, page.height().value);
            let scale = (300.0 / 72.0f32).min(3500.0 / w.max(h).max(1.0));
            let image = page.render_with_config(&PdfRenderConfig::new().scale_page_by_factor(scale)).map_err(|e| format!("{e}"))?.as_image().map_err(|e| format!("{e}"))?;
            let lines = super::ocr::recognize(&image.to_rgba8())?;
            models.push(ocr_page(&lines, w, h));
        }
        if models.iter().all(|m| m.body.is_empty()) {
            return Err("no text was found in the PDF".into());
        }
    }
    from_pdf::number_pages(&mut models);
    from_pdf::split_running_content(&mut models);
    on_page(total, total);
    Ok(models)
}

/// A scanned page from its recognised lines: words far apart split a line into columns
fn ocr_page(lines: &[super::ocr::TextLine], w: f32, h: f32) -> PageModel {
    let mut out = vec![];
    for l in lines {
        let [_, top, _, bottom] = l.rect;
        let height = (bottom - top) * h;
        if height <= 0.0 || l.text.trim().is_empty() {
            continue;
        }
        // a recognised line box is about 1.25 font sizes tall
        let size = (height / 1.25).clamp(4.0, 72.0);
        let style = Style { font: "Calibri".into(), size, bold: false, italic: false, underline: false, color: None, spacing: 0.0, vert: 0 };
        let words: Vec<(String, [f32; 4])> = if l.words.is_empty() { vec![(l.text.clone(), l.rect)] } else { l.words.clone() };
        let mut line = Line { runs: vec![], tabs: vec![], left: words[0].1[0] * w, right: words[0].1[2] * w, baseline: bottom * h - size * 0.25, size, pitch_hint: 0.0, art: false, gaps: vec![] };
        let mut text = String::new();
        let mut last_right = line.left;
        for (k, (word, r)) in words.iter().enumerate() {
            let (left, right) = (r[0] * w, r[2] * w);
            if k > 0 {
                if left - last_right > size * 1.0 {
                    text.push('\t');
                    line.tabs.push(from_pdf::TabMark { left, right, leader: false, before: last_right });
                } else {
                    text.push(' ');
                }
            }
            text.push_str(word);
            if let Some(t) = line.tabs.last_mut() {
                t.right = t.right.max(right);
            }
            last_right = right;
            line.right = line.right.max(right);
        }
        line.runs.push((text, style));
        out.push(line);
    }
    // lines recognised side by side rarely share a baseline exactly: one row each
    out.sort_by(|a, b| a.baseline.partial_cmp(&b.baseline).unwrap_or(std::cmp::Ordering::Equal));
    let mut row_base: Option<(f32, f32)> = None;
    for l in &mut out {
        match row_base {
            Some((base, size)) if (l.baseline - base).abs() < size.min(l.size) * 0.6 => l.baseline = base,
            _ => row_base = Some((l.baseline, l.size)),
        }
    }
    let lines = rejoin(out);
    let body = lines
        .into_iter()
        .map(|l| Block { rect: from_pdf::Rect { l: l.left, t: l.baseline - l.size, r: l.right, b: l.baseline + l.size * 0.3 }, content: Content::Paragraph(vec![l]) })
        .collect();
    PageModel { width: w, height: h, header: vec![], body, footer: vec![], decor: vec![], background: None, underline_rules: vec![], number: 0, fills: vec![] }
}

/// PDF -> .xlsx
pub fn pdf_to_xlsx(pdfium: &Pdfium, pdf: &[u8], name: &str, on_page: &dyn Fn(usize, usize)) -> Result<Vec<u8>, String> {
    let models = pages(pdfium, pdf, on_page)?;
    w::write(&[build(&models, name)])
}

/// PDF -> rows of text (for CSV, JSON and the other data formats)
pub fn pdf_to_rows(pdfium: &Pdfium, pdf: &[u8], on_page: &dyn Fn(usize, usize)) -> Result<Vec<Vec<String>>, String> {
    let models = pages(pdfium, pdf, on_page)?;
    let sheet = build(&models, "Sheet1");
    let last_row = sheet.cells.keys().map(|k| k.0).max().unwrap_or(0);
    let last_col = sheet.cells.keys().map(|k| k.1).max().unwrap_or(0);
    let mut rows = vec![vec![String::new(); last_col as usize + 1]; last_row as usize + 1];
    for (&(r, c), cell) in &sheet.cells {
        if let Some(t) = original_text(cell) {
            rows[r as usize][c as usize] = t;
        }
    }
    Ok(rows)
}

/// A cell's value written out plainly (numbers without their grouping)
fn original_text(cell: &w::Cell) -> Option<String> {
    Some(match &cell.value {
        Value::Text(t) => t.clone(),
        Value::Number(n) => {
            let fmt = cell.look.num_fmt.as_deref().unwrap_or("General");
            if fmt.contains('y') || fmt.contains('h') {
                super::numfmt::format(&super::numfmt::Value::Number(*n), fmt.trim_start_matches("[$-421]"), false, super::numfmt::Separators::US).text
            } else if fmt.contains('%') {
                format!("{}", n * 100.0) + "%"
            } else {
                let s = format!("{n}");
                if s.ends_with(".0") { s[..s.len() - 2].to_string() } else { s }
            }
        }
        Value::Empty => return None,
    })
}
