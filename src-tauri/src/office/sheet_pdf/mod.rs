// Prints a workbook to PDF the way Excel does: each visible sheet's print area (or
// used range) is split into pages down-then-over, scaled or fitted to the page, with
// repeated title rows, page breaks, centring, header and footer. Cells keep their
// fills, borders, fonts and number formats; text overflows into empty neighbours,
// numbers that don't fit show ####, and rows without a stored height grow to fit
// their wrapped text.
//
// Sheets nobody set up for printing are laid out to be read instead: turned to
// landscape and shrunk so that all their columns fit across the page, with a table's
// heading row repeated on every page (see "Smart layout").
//
// Everything is positioned absolutely (in points) and typeset with Typst.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::numfmt::{self, Separators, Shown, Value, HIDE_OFF, HIDE_ON};
use super::sheet_style::Mark;
use super::xlsx::{Edge, Font, HAlign, Line, Range, Sheet, Style, VAlign, Workbook};

/// Space between a cell's edge and its text (Excel leaves ~2 px)
const PAD: f32 = 1.5;

/// Everything needed to typeset the PDF. Pages are positioned absolutely and numbered
/// by this engine, so any run of them is a document of its own (see `chunk`).
pub struct Printed {
    /// Document settings, first in every document
    preamble: String,
    /// Each sheet's page settings
    sheets: Vec<String>,
    /// Each page: its sheet, then its content
    pages: Vec<(usize, String)>,
    pub files: Vec<(String, Vec<u8>)>,
}

impl Printed {
    /// The pages in `range` as a document of their own
    pub fn chunk(&self, range: std::ops::Range<usize>) -> String {
        let mut markup = self.preamble.clone();
        let mut sheet = None;
        for (i, (si, body)) in self.pages[range].iter().enumerate() {
            if sheet != Some(*si) {
                markup.push_str(&self.sheets[*si]);
                sheet = Some(*si);
            }
            if i > 0 {
                markup.push_str("#pagebreak()\n");
            }
            markup.push_str(body);
        }
        markup
    }

    /// The whole workbook as one document
    #[cfg(test)]
    pub fn markup(&self) -> String {
        self.chunk(0..self.pages.len())
    }
}

// The printer's parts. The submodules share one namespace (each starts with
// `use super::*`), as when this was a single file.
mod cells;
mod geometry;
mod pages;
mod print;
mod smart;
mod text;

#[cfg(test)]
mod tests;

use cells::*;
use geometry::*;
use pages::*;
use smart::*;
use text::*;

// What the rest of `office` uses besides to_pdf
pub use print::print;

/// Workbook -> PDF bytes with the built-in engine
pub fn to_pdf(wb: &Workbook, sep: Separators, file_name: &str) -> Result<Vec<u8>, String> {
    match typeset_workbook(wb, sep, file_name) {
        Ok(pdf) => Ok(pdf),
        // a chart or shape this engine draws wrongly must not cost the whole workbook
        Err(e) if wb.sheets.iter().any(|s| !s.drawn.is_empty()) => {
            eprintln!("[flipfile] Excel -> PDF: retrying without charts and shapes ({e})");
            let mut plain = Workbook { sheets: wb.sheets.clone(), styles: wb.styles.clone(), date1904: wb.date1904 };
            for s in &mut plain.sheets {
                s.drawn.clear();
            }
            typeset_workbook(&plain, sep, file_name)
        }
        Err(e) => Err(e),
    }
}

fn typeset_workbook(wb: &Workbook, sep: Separators, file_name: &str) -> Result<Vec<u8>, String> {
    let printed = print(wb, sep, file_name)?;
    // Typeset in runs of 4 pages, side by side, into one PDF: measured best (8 pages per
    // run is slower, and a whole workbook at once needs twice the memory and time)
    let per_run = 4;
    let runs: Vec<String> =
        (0..printed.pages.len()).step_by(per_run).map(|i| printed.chunk(i..(i + per_run).min(printed.pages.len()))).collect();
    super::typeset::to_pdf_in_runs(runs, printed.files, &[]).map_err(|e| format!("the workbook could not be laid out ({e})"))
}
