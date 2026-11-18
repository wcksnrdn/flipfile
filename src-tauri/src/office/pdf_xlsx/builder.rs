//! The sheet being built: cells, column widths, lines of text and the space between
//! things; the tables it places are in tables.rs

use super::*;

/// A table on the sheet, to continue on the next page
#[derive(Clone)]
pub(super) struct Placed {
    /// its heading row's texts
    pub(super) heading: Vec<String>,
    /// where its columns are on the page (left, right) and on the sheet
    pub(super) spans: Vec<(f32, f32, u32)>,
}

pub(super) struct Builder {
    pub(super) sheet: w::Sheet,
    pub(super) row: u32,
    pub(super) notation: Notation,
    /// the table the sheet ended with
    pub(super) last: Option<Placed>,
    /// widest content of each column, in points
    pub(super) widths: BTreeMap<u32, f32>,
    pub(super) first_table: bool,
    /// bottom of what was placed last on this page, in points (None at a page's top)
    pub(super) bottom: Option<f32>,
    /// the page's filled bands and boxes (behind text: a cell's shading)
    pub(super) fills: Vec<(from_pdf::Rect, [u8; 3])>,
}

pub(super) fn look_of(style: &Style) -> Look {
    let size10 = (style.size * 10.0).round() as u16;
    Look {
        size10: if size10 == 110 { 0 } else { size10.clamp(40, 720) },
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        color: style.color.filter(|c| *c != [0, 0, 0]),
        ..Default::default()
    }
}

impl Builder {
    pub(super) fn put(&mut self, r: u32, c: u32, text: &str, mut look: Look) {
        // white text whose dark background didn't come along would be invisible
        if look.fill.is_none() && look.color.is_some_and(|c| c.iter().all(|v| *v > 225)) {
            look.color = None;
        }
        let (value, fmt) = typed(text, self.notation);
        if matches!(value, Value::Empty) && look == Look::default() {
            return;
        }
        if fmt.is_some() {
            look.num_fmt = fmt;
        }
        // text that reads as a number but must stay text keeps Excel from guessing
        if matches!(value, Value::Text(_)) && look.h.is_none() && text.trim().chars().all(|c| c.is_ascii_digit()) {
            look.h = Some("left");
        }
        self.sheet.cells.insert((r, c), w::Cell { value, look });
    }

    /// The shading behind a part of a line: the smallest filled box around its middle
    pub(super) fn fill_at(&self, s: &Segment, baseline: f32) -> Option<[u8; 3]> {
        let (x, y) = ((s.left + s.right) / 2.0, baseline - s.style.size * 0.35);
        self.fills
            .iter()
            .filter(|(r, _)| x >= r.l && x <= r.r && y >= r.t && y <= r.b)
            .min_by(|a, b| (a.0.w() * a.0.h()).partial_cmp(&(b.0.w() * b.0.h())).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, c)| *c)
    }

    pub(super) fn width(&mut self, c: u32, pt: f32) {
        let e = self.widths.entry(c).or_insert(0.0);
        *e = e.max(pt);
    }

    /// Lines of text that aren't a table: one row each, gaps splitting them over columns
    pub(super) fn text_line(&mut self, line: &Line) {
        let segs = segments(line);
        let at = if segs.len() > 1 { self.align(&segs.iter().map(|s| (s.left, s.right)).collect::<Vec<_>>(), false) } else { vec![0] };
        let aligned = at.iter().enumerate().any(|(i, c)| *c != i as u32);
        // a row lined up with the table above belongs to it: no blank row between
        if !aligned {
            self.space_before(line.baseline - line.size, line.size);
        }
        self.bottom = Some(line.baseline + line.size * 0.3);
        for (i, s) in segs.iter().enumerate() {
            let mut look = look_of(&s.style);
            look.fill = self.fill_at(s, line.baseline);
            let c = at.get(i).copied().unwrap_or(i as u32);
            self.put(self.row, c, &s.text, look);
            // a line on its own spills over empty cells; only parts set the width
            if segs.len() > 1 && !aligned {
                self.width(c, s.right - s.left);
            }
        }
        if line.size > 13.0 {
            self.sheet.row_heights.insert(self.row, line.size * 1.35);
        }
        self.row += 1;
    }

    /// A blank row between things, except at the very top
    pub(super) fn gap(&mut self) {
        if self.row > 0 && self.sheet.cells.range((self.row - 1, 0)..).next().is_some() {
            self.row += 1;
        }
    }

    /// Before something starting at `top` (points down the page): a blank row where the
    /// PDF leaves a blank line or more
    pub(super) fn space_before(&mut self, top: f32, size: f32) {
        if let Some(b) = self.bottom
            && top - b > size.max(9.0) * 1.1 {
                self.gap();
            }
    }
}
