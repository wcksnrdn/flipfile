// PDF -> Word (.docx), in the spirit of pdf2docx: PDFium gives every character with
// its position, size and font, and every vector shape. From that geometry the page
// layout is rebuilt:
//
// - ruling lines that form a grid become real Word tables (merged cells, borders per
//   cell side, cell shading, row heights);
// - characters become lines, lines become paragraphs (alignment, indents, exact line
//   pitch, spacing), and wide gaps become tabs with matching tab stops (dot leaders too);
// - content repeated at the top/bottom of the pages becomes the Word header/footer,
//   with the page number as a live PAGE field;
// - vector drawings (charts, flowcharts) become pictures, images stay where they were;
// - every page keeps its own size and orientation.
//
// The result is an editable document that keeps the original page layout; it is
// written by `pdf_docx_writer`.

use super::pdf_docx_writer as writer;
use pdfium_render::prelude::*;
use std::collections::HashMap;

/// Gaps wider than this many font sizes inside a line become a tab (column layouts).
const TAB_GAP: f32 = 2.0;

thread_local! {
    /// `TAB_GAP` while a page is read for a spreadsheet (see `with_tab_gap`)
    static TAB_GAP_NOW: std::cell::Cell<f32> = const { std::cell::Cell::new(TAB_GAP) };
}

/// Reads pages with gaps wider than `gap` font sizes splitting lines: spreadsheets want
/// columns that Word would keep on one line (amounts a few spaces apart)
pub(super) fn with_tab_gap<T>(gap: f32, f: impl FnOnce() -> T) -> T {
    let before = TAB_GAP_NOW.with(|g| g.replace(gap));
    let out = f();
    TAB_GAP_NOW.with(|g| g.set(before));
    out
}
/// Gaps wider than this many font sizes get a space if the PDF has none.
const SPACE_GAP: f32 = 0.22;
/// Resolution of drawings that are turned into pictures.
const DRAWING_DPI: f32 = 200.0;
/// Resolution of the decoration picture behind a page's text.
const BACKGROUND_DPI: f32 = 150.0;

// The page analysis, in the order a page goes through it. The submodules share one
// namespace (each starts with `use super::*`), as when this was a single file.
mod fonts;
mod graphics;
mod model;
mod page;
mod running;
mod scanned;
mod text;

#[cfg(test)]
mod review;
#[cfg(test)]
mod tests;

use fonts::*;
use graphics::*;
use model::*;
use page::*;
use text::*;

// What the rest of `office` uses: the page model the writers build from, and the steps
// pdf_xlsx reuses to read pages for a spreadsheet
pub(super) use model::{Block, Border, Cell, Content, ImageRef, Line, MediaStore, PageModel, Rect, Style, TabMark, Table, VAlign};
pub(super) use page::page_model;
pub(super) use running::{number_pages, signature, split_numbers, split_running_content, Piece};
pub(super) use text::line_pitch;
pub use scanned::{ocr_to_docx, OcrPage};

/// Converts PDF bytes to a .docx file, one Word page per PDF page.
pub fn pdf_to_docx(pdfium: &Pdfium, pdf: &[u8], on_page: &dyn Fn(usize, usize)) -> Result<Vec<u8>, String> {
    let document = pdfium.load_pdf_from_byte_slice(pdf, None).map_err(|e| {
        if format!("{e:?}").contains("Password") {
            "the PDF is password-protected".to_string()
        } else {
            "the PDF could not be opened".to_string()
        }
    })?;
    let pages = document.pages();
    let total = pages.len() as usize;
    if total == 0 {
        return Err("the PDF has no pages".into());
    }
    let mut media = MediaStore::default();
    let mut models = vec![];
    // A second copy of the document: its pages are stripped down to their decoration
    // before rendering (the first copy stays intact for reading)
    let mut stripped: Option<PdfDocument> = None;
    for (index, page) in pages.iter().enumerate() {
        on_page(index, total);
        let mut model = page_model(&page, &document, &mut media);
        let annotated = !page.annotations().is_empty();
        if !model.decor.is_empty() || annotated {
            if stripped.is_none() {
                stripped = pdfium.load_pdf_from_byte_slice(pdf, None).ok();
            }
            if let Some(doc) = &stripped {
                model.background = background(doc, index, &model.decor, &model.underline_rules, &mut media);
            }
        }
        models.push(model);
    }
    number_pages(&mut models);
    split_running_content(&mut models);
    on_page(total, total);
    writer::write(&models, &media)
}
