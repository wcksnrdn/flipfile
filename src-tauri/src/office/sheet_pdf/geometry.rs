//! Sheet geometry

use super::*;

/// The rows and columns that print, and where they are (unscaled points from the range start)
pub(super) struct Grid {
    pub(super) range: Range,
    /// x of each column's left edge, for columns range.1..=range.3+1
    pub(super) xs: Vec<f32>,
    pub(super) ys: Vec<f32>,
}

impl Grid {
    pub(super) fn x(&self, c: u32) -> f32 {
        self.xs[(c - self.range.1) as usize]
    }
    pub(super) fn y(&self, r: u32) -> f32 {
        self.ys[(r - self.range.0) as usize]
    }
    pub(super) fn w(&self, c: u32) -> f32 {
        self.x(c + 1) - self.x(c)
    }
    pub(super) fn h(&self, r: u32) -> f32 {
        self.y(r + 1) - self.y(r)
    }
}

/// What prints: the print area, or every cell with a value, fill or border (and pictures)
pub(super) fn used_range(sheet: &Sheet, styles: &[Style]) -> Option<Range> {
    if let Some(a) = sheet.print_area {
        return Some(a);
    }
    let mut r: Option<Range> = None;
    let mut grow = |row: u32, col: u32| {
        r = Some(match r {
            None => (row, col, row, col),
            Some((r1, c1, r2, c2)) => (r1.min(row), c1.min(col), r2.max(row), c2.max(col)),
        });
    };
    for (&(row, col), cell) in &sheet.cells {
        let s = &styles[cell.style];
        let b = &s.borders;
        let visible = cell.value.is_some() || s.fill.is_some() || b.left.is_some() || b.right.is_some() || b.top.is_some() || b.bottom.is_some();
        if visible {
            grow(row, col);
        }
    }
    let has_cells = !sheet.cells.is_empty();
    for m in &sheet.merges {
        if has_cells {
            grow(m.0, m.1);
            grow(m.2, m.3);
        }
    }
    let anchors = sheet.pictures.iter().map(|p| (p.from, p.to, p.size)).chain(sheet.drawn.iter().map(|d| (d.anchor().from, d.anchor().to, d.anchor().size)));
    for (from, to, size) in anchors {
        grow(from.0, from.1);
        match to {
            Some(to) => grow(to.0.saturating_sub(if to.2 == 0.0 { 1 } else { 0 }), to.1.saturating_sub(if to.3 == 0.0 { 1 } else { 0 })),
            None => {
                // anchored by its size: the cell its bottom-right corner reaches
                let (mut c, mut x) = (from.1, size.0 + from.3);
                while x > sheet.col_width(c) && c < from.1 + 500 {
                    x -= sheet.col_width(c).max(1.0);
                    c += 1;
                }
                let (mut r, mut y) = (from.0, size.1 + from.2);
                let row_h = |r: u32| sheet.row_heights.get(&r).copied().unwrap_or(sheet.default_row_height).max(1.0);
                while y > row_h(r) && r < from.0 + 2000 {
                    y -= row_h(r);
                    r += 1;
                }
                grow(r, c);
            }
        }
    }
    drop(grow);
    // Excel prints from A1 when the content starts near it
    r.map(|(r1, c1, r2, c2)| (if r1 < 3 { 0 } else { r1 }, if c1 < 3 { 0 } else { c1 }, r2, c2))
}

/// Row heights, growing rows without a stored height to fit wrapped or large text
pub(super) fn row_heights(sheet: &Sheet, styles: &[Style], range: Range, col_w: &dyn Fn(u32) -> f32, merged: &HashMap<(u32, u32), Range>, sep: Separators, date1904: bool) -> HashMap<u32, f32> {
    let mut out = HashMap::new();
    for (&(r, c), cell) in sheet.cells.range((range.0, 0)..=(range.2, u32::MAX)) {
        if c < range.1 || c > range.3 || sheet.row_heights.contains_key(&r) {
            continue;
        }
        let Some(value) = &cell.value else { continue };
        // merged cells don't grow their row (Excel doesn't either)
        if merged.get(&(r, c)).map(|m| m.0 != m.2).unwrap_or(false) {
            continue;
        }
        let style = &styles[cell.style];
        let fams = families(&style.font.name);
        // the largest font in the cell sets the line height
        let size = sheet.rich.get(&(r, c)).map(|runs| runs.iter().map(|run| run.font.size.unwrap_or(style.font.size)).fold(style.font.size, f32::max)).unwrap_or(style.font.size);
        let lh = line_height(&fams, size);
        let mut need = lh;
        if style.rotation != 0 || style.stacked {
            let shown = numfmt::format(value, &style.num_fmt, date1904, sep);
            let text = shown.text.replace('\n', " ");
            need = if style.stacked {
                lh * text.chars().count() as f32
            } else {
                let a = (style.rotation as f32).abs().to_radians();
                width(&fams, size, style.font.bold, style.font.italic, &text) * a.sin() + lh * a.cos() + 2.0 * PAD
            };
        } else if style.wrap {
            let shown = numfmt::format(value, &style.num_fmt, date1904, sep);
            let w = merged.get(&(r, c)).map(|m| (m.1..=m.3).map(col_w).sum()).unwrap_or_else(|| col_w(c)) - 2.0 * PAD;
            let lines = wrapped_lines(&shown.text, &fams, &style.font, w.max(4.0));
            need = lh * lines as f32;
        }
        let e = out.entry(r).or_insert(sheet.default_row_height);
        if need > *e {
            *e = need;
        }
    }
    out
}

/// How many lines `text` takes when wrapped at `w` points
fn wrapped_lines(text: &str, fams: &[String], font: &crate::office::xlsx::Font, w: f32) -> usize {
    let mut lines = 0;
    for para in text.split('\n') {
        let mut line = String::new();
        let mut n = 1;
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && width(fams, font.size, font.bold, font.italic, &candidate) > w {
                n += 1;
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        lines += n;
    }
    lines.max(1)
}
