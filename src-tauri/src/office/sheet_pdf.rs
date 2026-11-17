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

/// Font families to try for an Excel font: itself, then metric-compatible and common
/// look-alikes (worked out once per font name: every cell asks)
fn families(name: &str) -> std::rc::Rc<Vec<String>> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, std::rc::Rc<Vec<String>>>> = Default::default();
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(name).cloned()) {
        return hit;
    }
    let v = std::rc::Rc::new(family_list(name));
    CACHE.with(|c| c.borrow_mut().insert(name.to_string(), v.clone()));
    v
}

fn family_list(name: &str) -> Vec<String> {
    let mut v = vec![name.to_string()];
    let alt: &[&str] = match name.to_lowercase().as_str() {
        "calibri" | "calibri light" => &["Carlito"],
        "cambria" => &["Caladea"],
        "arial" => &["Liberation Sans", "Arimo"],
        "times new roman" => &["Liberation Serif", "Tinos", "Times"],
        "courier new" => &["Liberation Mono", "Cousine", "Courier"],
        "aptos narrow" => &["Aptos", "Arial Narrow", "Helvetica Neue"],
        "aptos" | "aptos display" => &["Calibri", "Carlito"],
        _ => &[],
    };
    v.extend(alt.iter().map(|s| s.to_string()));
    for common in ["Calibri", "Arial", "Helvetica", "Liberation Sans", "DejaVu Sans"] {
        if !v.iter().any(|f| f.eq_ignore_ascii_case(common)) {
            v.push(common.to_string());
        }
    }
    v
}

/// Width of `text` in the first installed family as Excel measures it (whole pixels per
/// character, see `typeset::text_width_excel`), or an estimate
fn width(fams: &[String], size: f32, bold: bool, italic: bool, text: &str) -> f32 {
    let plain: String = text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).collect();
    for f in fams {
        if let Some(w) = super::typeset::text_width_excel(f, size, bold, italic, &plain) {
            return w;
        }
    }
    plain.chars().count() as f32 * size * if bold { 0.56 } else { 0.52 }
}

/// Width of `text` as it is actually drawn (its outlines)
fn outline_width(fams: &[String], size: f32, bold: bool, italic: bool, text: &str) -> f32 {
    let plain: String = text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).collect();
    for f in fams {
        if let Some(w) = super::typeset::text_width(f, size, bold, italic, &plain) {
            return w;
        }
    }
    plain.chars().count() as f32 * size * if bold { 0.56 } else { 0.52 }
}

/// A piece of a cell's text in one font: the whole text, or a run of rich text
#[derive(Clone)]
struct Piece {
    text: String,
    font: Font,
    /// from the number format ([Red]…), over the font's own colour
    color: Option<[u8; 3]>,
    script: i8,
}

impl Piece {
    fn plain(text: String, font: &Font, color: Option<[u8; 3]>) -> Vec<Piece> {
        vec![Piece { text, font: font.clone(), color, script: 0 }]
    }
    fn size(&self) -> f32 {
        if self.script != 0 { self.font.size * 0.65 } else { self.font.size }
    }
    fn width(&self) -> f32 {
        width(&families(&self.font.name), self.size(), self.font.bold, self.font.italic, &self.text)
    }
    fn outline(&self) -> f32 {
        outline_width(&families(&self.font.name), self.size(), self.font.bold, self.font.italic, &self.text)
    }
}

fn pieces_width(pieces: &[Piece]) -> f32 {
    pieces.iter().map(Piece::width).sum()
}

/// Letter spacing (pt) that draws `pieces` exactly as wide as Excel measures them
fn tracking(pieces: &[Piece]) -> f32 {
    tracking_for(pieces, pieces_width(pieces))
}

/// `tracking` when Excel's width of the pieces is known already
fn tracking_for(pieces: &[Piece], excel: f32) -> f32 {
    let chars: usize = pieces.iter().map(|p| p.text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).count()).sum();
    if chars < 2 {
        return 0.0;
    }
    let drawn: f32 = pieces.iter().map(Piece::outline).sum();
    // only ever tighten, and never more than Windows' pixel rounding could
    ((excel - drawn) / chars as f32).clamp(-0.08 * pieces[0].font.size, 0.0)
}

/// The pieces a cell's text is drawn in: its rich text runs, or the formatted text
fn cell_pieces(shown: &Shown, style: &Style, rich: Option<&super::xlsx::Rich>, value: &Value) -> Vec<Piece> {
    if let (Some(runs), Value::Text(t)) = (rich, value)
        && shown.text == *t && shown.color.is_none() {
            return runs.iter().filter(|r| !r.text.is_empty()).map(|r| Piece { text: r.text.clone(), font: r.font.over(&style.font), color: None, script: r.font.script }).collect();
        }
    Piece::plain(shown.text.clone(), &style.font, shown.color)
}

/// Height of one line of text, the way Excel sizes rows (15 pt for Calibri 11)
fn line_height(fams: &[String], size: f32) -> f32 {
    let (a, d) = super::typeset::line_metrics(fams);
    ((a + d) * size * 1.12).max(size * 1.2)
}

fn typst_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn rgb(c: [u8; 3]) -> String {
    format!("rgb({}, {}, {})", c[0], c[1], c[2])
}

/// Typst's list of `families` for an Excel font name
fn font_list(name: &str) -> std::rc::Rc<str> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, std::rc::Rc<str>>> = Default::default();
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(name).cloned()) {
        return hit;
    }
    let list: std::rc::Rc<str> = families(name).iter().map(|s| typst_str(s)).collect::<Vec<_>>().join(", ").into();
    CACHE.with(|c| c.borrow_mut().insert(name.to_string(), list.clone()));
    list
}

/// A Typst expression for a cell's text pieces (with hidden padding runs), at `scale`;
/// `shrink` scales the font sizes further (Shrink to fit), `track` is letter spacing in pt.
///
/// `default_font` is the `font_list` the document sets for all text (see `print`), so
/// only cells in another font name theirs: a workbook has tens of thousands of cells, and
/// every byte of markup per cell costs Typst time and memory.
fn pieces_expr(pieces: &[Piece], scale: f32, shrink: f32, track: f32, default_font: &str) -> String {
    let mut all = vec![];
    for p in pieces {
        let f = &p.font;
        let fams_list = font_list(&f.name);
        let size = p.size() * shrink * scale;
        let fill = p.color.or(f.color).unwrap_or([0, 0, 0]);
        let mut base = format!("size: {size:.2}pt");
        if &*fams_list != default_font {
            base = format!("font: ({fams_list},), {base}");
        }
        if f.bold {
            base.push_str(", weight: \"bold\"");
        }
        if f.italic {
            base.push_str(", style: \"italic\"");
        }
        if track < -0.005 {
            let _ = write!(base, ", tracking: {:.3}pt", track * shrink * scale);
        }
        if p.script != 0 {
            let _ = write!(base, ", baseline: {:.2}pt", -(p.script as f32) * f.size * 0.33 * shrink * scale);
        }
        let mut parts = vec![];
        let mut cur = String::new();
        let mut hidden = false;
        let flush = |cur: &mut String, hidden: bool, parts: &mut Vec<String>| {
            if cur.is_empty() {
                return;
            }
            let paint = if hidden { "rgb(0, 0, 0, 0)".to_string() } else { rgb(fill) };
            parts.push(format!("text({base}, fill: {paint}, {})", typst_str(cur)));
            cur.clear();
        };
        for c in p.text.chars() {
            match c {
                HIDE_ON => {
                    flush(&mut cur, hidden, &mut parts);
                    hidden = true;
                }
                HIDE_OFF => {
                    flush(&mut cur, hidden, &mut parts);
                    hidden = false;
                }
                c => cur.push(c),
            }
        }
        flush(&mut cur, hidden, &mut parts);
        if parts.is_empty() {
            continue;
        }
        let mut e = parts.join(" + ");
        if f.underline {
            e = format!("underline({e})");
        }
        if f.strike {
            e = format!("strike({e})");
        }
        all.push(e);
    }
    if all.is_empty() {
        return "[]".into();
    }
    all.join(" + ")
}

/// `pieces_expr` for one text in a style
fn text_expr(text: &str, style: &Style, color: Option<[u8; 3]>, scale: f32, default_font: &str) -> String {
    pieces_expr(&Piece::plain(text.to_string(), &style.font, color), scale, 1.0, 0.0, default_font)
}

// ---------------------------------------------------------------------------
// Sheet geometry

/// The rows and columns that print, and where they are (unscaled points from the range start)
struct Grid {
    range: Range,
    /// x of each column's left edge, for columns range.1..=range.3+1
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl Grid {
    fn x(&self, c: u32) -> f32 {
        self.xs[(c - self.range.1) as usize]
    }
    fn y(&self, r: u32) -> f32 {
        self.ys[(r - self.range.0) as usize]
    }
    fn w(&self, c: u32) -> f32 {
        self.x(c + 1) - self.x(c)
    }
    fn h(&self, r: u32) -> f32 {
        self.y(r + 1) - self.y(r)
    }
}

/// What prints: the print area, or every cell with a value, fill or border (and pictures)
fn used_range(sheet: &Sheet, styles: &[Style]) -> Option<Range> {
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
fn row_heights(sheet: &Sheet, styles: &[Style], range: Range, col_w: &dyn Fn(u32) -> f32, merged: &HashMap<(u32, u32), Range>, sep: Separators, date1904: bool) -> HashMap<u32, f32> {
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
fn wrapped_lines(text: &str, fams: &[String], font: &super::xlsx::Font, w: f32) -> usize {
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

// ---------------------------------------------------------------------------
// Smart layout: sheets nobody set up for printing

/// Smallest scale a sheet is shrunk to so that all its columns fit: a PDF is mostly read
/// zoomed on a screen, where one wide table beats its columns scattered over pages.
/// Wider sheets than that continue on more pages.
const SMART_MIN_SCALE: f32 = 0.3;

/// Fitting a portrait page is worth shrinking to this scale before turning to landscape
const SMART_PORTRAIT_SCALE: f32 = 0.85;

/// Someone chose how the sheet prints: Excel's own layout is then followed exactly
fn is_set_up(sheet: &Sheet) -> bool {
    sheet.print.explicit || sheet.print_area.is_some() || sheet.title_rows.is_some() || sheet.title_cols.is_some() || !sheet.row_breaks.is_empty() || !sheet.col_breaks.is_empty()
}

/// Landscape when the columns don't fit across a portrait page
fn smart_landscape(p: &super::xlsx::Print, total_w: f32) -> bool {
    let room = (p.paper.0.min(p.paper.1) - p.margins[0] - p.margins[1]).max(36.0);
    total_w * SMART_PORTRAIT_SCALE > room
}

/// The rows to repeat on every page: the frozen rows, or else a table's heading row near
/// the top (all text, bold or shaded, as wide as the table below it)
fn heading_rows(sheet: &Sheet, styles: &[Style], range: Range) -> Option<(u32, u32)> {
    let frozen = sheet.frozen.0;
    if frozen > range.0 && frozen <= range.0 + 15 && frozen <= range.2 {
        return Some((range.0, frozen - 1));
    }
    let filled = |r: u32| sheet.cells.range((r, range.1)..=(r, range.3)).filter(|(_, c)| c.value.is_some()).count();
    let last = range.2.min(range.0 + 30);
    let widest = (range.0..=last).map(filled).max().unwrap_or(0);
    if widest < 2 {
        return None;
    }
    (range.0..=range.2.min(range.0 + 10)).filter(|&r| r < range.2).find(|&r| {
        let cells: Vec<_> = sheet.cells.range((r, range.1)..=(r, range.3)).filter(|(_, c)| c.value.is_some()).collect();
        cells.len() * 2 >= widest
            && cells.len() >= 2
            && filled(r + 1) >= 2
            && cells.iter().all(|(_, c)| {
                let s = &styles[c.style];
                matches!(c.value, Some(Value::Text(_))) && (s.font.bold || s.fill.is_some())
            })
    }).map(|r| (r, r))
}

// ---------------------------------------------------------------------------
// Pages

/// Whether anything prints on a page: a cell with a value, fill or border, or a picture,
/// chart or shape starting on it
fn has_content(sheet: &Sheet, styles: &[Style], page: &Page) -> bool {
    let (r1, r2, c1, c2) = (page.rows.0, page.rows.1, page.cols.0, page.cols.1);
    let inside = |r: u32, c: u32| r >= r1 && r <= r2 && c >= c1 && c <= c2;
    let cells = sheet.cells.range((r1, 0)..=(r2, u32::MAX)).any(|(&(_, c), cell)| {
        if c < c1 || c > c2 {
            return false;
        }
        let st = &styles[cell.style];
        let b = &st.borders;
        cell.value.is_some() || st.fill.is_some() || b.left.is_some() || b.right.is_some() || b.top.is_some() || b.bottom.is_some()
    });
    cells
        || sheet.merges.iter().any(|m| inside(m.0, m.1))
        || sheet.pictures.iter().any(|p| inside(p.from.0, p.from.1))
        || sheet.drawn.iter().any(|d| inside(d.anchor().from.0, d.anchor().from.1))
}

struct Page {
    rows: (u32, u32),
    cols: (u32, u32),
}

/// Splits `[a, b]` into bands no longer than `room`, also breaking before `breaks`
fn bands(a: u32, b: u32, size: &dyn Fn(u32) -> f32, room: f32, breaks: &[u32]) -> Vec<(u32, u32)> {
    let mut out = vec![];
    let mut start = a;
    let mut used = 0.0;
    for i in a..=b {
        let s = size(i);
        let forced = i > start && breaks.contains(&i);
        if i > start && (used + s > room + 0.01 || forced) {
            out.push((start, i - 1));
            start = i;
            used = 0.0;
        }
        used += s;
    }
    out.push((start, b));
    out
}

/// Header/footer text with Excel's codes resolved: (left, center, right)
fn header_parts(spec: &str, page: usize, pages: usize, file: &str, sheet: &str) -> [String; 3] {
    let mut parts = [String::new(), String::new(), String::new()];
    let mut cur = 1; // text before any section code goes in the centre
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '&' && i + 1 < chars.len() {
            let n = chars[i + 1];
            i += 2;
            match n {
                'L' => cur = 0,
                'C' => cur = 1,
                'R' => cur = 2,
                'P' => parts[cur].push_str(&page.to_string()),
                'N' => parts[cur].push_str(&pages.to_string()),
                'D' => parts[cur].push_str(&today()),
                'T' => {}
                'F' => parts[cur].push_str(file),
                'A' => parts[cur].push_str(sheet),
                '&' => parts[cur].push('&'),
                '"' => {
                    // &"Font,Style": skip to the closing quote
                    while i < chars.len() && chars[i] != '"' {
                        i += 1;
                    }
                    i += 1;
                }
                d if d.is_ascii_digit() => {
                    // &12: font size
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                _ => {} // &B &I &U &S &X &Y &K…: styling
            }
        } else {
            parts[cur].push(c);
            i += 1;
        }
    }
    parts
}

fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let serial = secs as f64 / 86400.0 + 25569.0;
    numfmt::format(&Value::Number(serial.floor()), "SHORTDATE", false, Separators::US).text
}

// ---------------------------------------------------------------------------

/// Typst markup for every visible sheet of `wb`
pub fn print(wb: &Workbook, sep: Separators, file_name: &str) -> Result<Printed, String> {
    struct Planned<'a> {
        sheet: &'a Sheet,
        grid: Grid,
        pages: Vec<Page>,
        scale: f32,
        page_size: (f32, f32),
        titles: Option<(u32, u32)>,
        merged: HashMap<(u32, u32), Range>,
    }
    let mut plans = vec![];
    for sheet in &wb.sheets {
        let Some(range) = used_range(sheet, &wb.styles) else { continue };
        let range = (range.0, range.1, range.2.max(range.0), range.3.max(range.1));
        // merged regions, indexed by every cell they cover
        let mut merged = HashMap::new();
        for m in &sheet.merges {
            for r in m.0..=m.2 {
                for c in m.1..=m.3 {
                    merged.insert((r, c), *m);
                }
            }
        }
        let col_w = |c: u32| sheet.col_width(c);
        let auto = row_heights(sheet, &wb.styles, range, &col_w, &merged, sep, wb.date1904);
        let row_h = |r: u32| sheet.row_heights.get(&r).copied().or_else(|| auto.get(&r).copied()).unwrap_or(sheet.default_row_height);
        let mut xs = vec![0.0];
        for c in range.1..=range.3 {
            xs.push(xs.last().unwrap() + col_w(c));
        }
        let mut ys = vec![0.0];
        for r in range.0..=range.2 {
            ys.push(ys.last().unwrap() + row_h(r));
        }
        let grid = Grid { range, xs, ys };

        let p = &sheet.print;
        let [ml, mr, mt, mb, _, _] = p.margins;
        let total_w = *grid.xs.last().unwrap();
        let total_h = *grid.ys.last().unwrap();
        let smart = !is_set_up(sheet);
        let landscape = if smart { smart_landscape(p, total_w) } else { p.landscape };
        let page_size = if landscape { (p.paper.1, p.paper.0) } else { p.paper };
        let room_w = (page_size.0 - ml - mr).max(36.0);
        let room_h = (page_size.1 - mt - mb).max(36.0);

        let scale = if smart {
            // every column on the page, unless that makes the text unreadably small
            (room_w / total_w.max(1.0)).clamp(SMART_MIN_SCALE, 1.0)
        } else {
            match p.fit {
                Some((fw, fh)) => {
                    let sx = if fw > 0 { room_w * fw as f32 / total_w.max(1.0) } else { f32::MAX };
                    let sy = if fh > 0 { room_h * fh as f32 / total_h.max(1.0) } else { f32::MAX };
                    sx.min(sy).clamp(0.1, 1.0)
                }
                None => p.scale / 100.0,
            }
        };
        let mut scale = scale;
        let mut col_bands = bands(range.1, range.3, &|c| grid.w(c) * scale, room_w, &sheet.col_breaks);
        if smart && col_bands.len() > 1 {
            // too wide even when shrunk: spread the columns evenly over the pages it takes,
            // as large as that allows, rather than a full page and a sliver
            let n = col_bands.len() as f32;
            let mut try_scale = (room_w * n / total_w).min(1.0);
            while try_scale > scale {
                let b = bands(range.1, range.3, &|c| grid.w(c) * try_scale, room_w, &sheet.col_breaks);
                if b.len() == col_bands.len() {
                    scale = try_scale;
                    col_bands = b;
                    break;
                }
                try_scale *= 0.97;
            }
        }
        let mut titles = sheet.title_rows.filter(|t| t.0 <= t.1);
        // a table running over several pages repeats its headings on each, as people
        // would set it up to print
        if smart && titles.is_none() && total_h * scale > room_h {
            titles = heading_rows(sheet, &wb.styles, range);
        }
        let titles_h = titles.map(|(a, b)| (a.max(range.0)..=b.min(range.2)).map(|r| grid.h(r)).sum::<f32>()).unwrap_or(0.0);
        let row_bands = bands(range.0, range.2, &|r| grid.h(r) * scale, room_h - titles_h * scale, &sheet.row_breaks);
        let mut pages = vec![];
        if p.over_then_down {
            for rb in &row_bands {
                for cb in &col_bands {
                    pages.push(Page { rows: *rb, cols: *cb });
                }
            }
        } else {
            for cb in &col_bands {
                for rb in &row_bands {
                    pages.push(Page { rows: *rb, cols: *cb });
                }
            }
        }
        // a page with nothing on it (rows a drawing's size only seemed to reach) is left out
        let mut pages: Vec<Page> = pages.into_iter().filter(|pg| has_content(sheet, &wb.styles, pg)).collect();
        if pages.is_empty() {
            pages.push(Page { rows: (range.0, range.2), cols: (range.1, range.3) });
        }
        plans.push(Planned { sheet, grid, pages, scale, page_size, titles, merged });
    }
    if plans.is_empty() {
        return Err("the workbook has nothing to print (all sheets are empty)".into());
    }

    let total: usize = plans.iter().map(|p| p.pages.len()).sum();
    let mut preamble = String::new();
    // What every text and cell would otherwise repeat: the workbook's own font, and a
    // line box from ascender to descender (so g/j/p are not clipped at a cell's bottom)
    let default_font = wb.styles.first().map(|s| font_list(&s.font.name)).unwrap_or_default();
    let _ = write!(preamble, "#set text(top-edge: \"ascender\", bottom-edge: \"descender\"");
    if !default_font.is_empty() {
        let _ = write!(preamble, ", font: ({default_font},)");
    }
    preamble.push_str(")\n");
    let mut sheets = vec![];
    let mut pages = vec![];
    let mut files = vec![];
    let mut page_no = 0usize;
    let mut pic_files: HashMap<(usize, usize), String> = HashMap::new();
    for (si, plan) in plans.iter().enumerate() {
        let sheet = plan.sheet;
        let p = &sheet.print;
        let s = plan.scale;
        let (pw, ph) = plan.page_size;
        let [ml, mr, mt, mb, mh, mf] = p.margins;
        let mut out = String::new();
        let _ = writeln!(out, "#set page(width: {pw:.2}pt, height: {ph:.2}pt, margin: 0pt)");
        // A cell clips its text and keeps it off the gridlines (at this sheet's scale)
        let _ = writeln!(
            out,
            "#let cell(w, h, body) = block(width: w, height: h, clip: true, inset: (left: {:.2}pt, right: {:.2}pt, top: 0.5pt, bottom: 1pt), body)",
            PAD * s,
            PAD * s
        );
        sheets.push(out);
        let first_page = p.first_page.map(|n| n as usize).unwrap_or(page_no + 1);
        for (pi, page) in plan.pages.iter().enumerate() {
            let mut out = String::new();
            page_no += 1;
            let grid = &plan.grid;
            // rows on this page: the title rows (unless they are already here), then the band
            let mut rows: Vec<u32> = vec![];
            if let Some((t1, t2)) = plan.titles
                && page.rows.0 > t2 {
                    rows.extend(t1.max(grid.range.0)..=t2.min(grid.range.2));
                }
            rows.extend(page.rows.0..=page.rows.1);
            let cols: Vec<u32> = (page.cols.0..=page.cols.1).collect();
            // y of each printed row on the page (unscaled), x of each column
            let mut ry: HashMap<u32, f32> = HashMap::new();
            let mut acc = 0.0;
            for &r in &rows {
                ry.insert(r, acc);
                acc += grid.h(r);
            }
            let content_h = acc;
            let x0 = grid.x(page.cols.0);
            let content_w = grid.x(page.cols.1 + 1) - x0;
            let room_w = pw - ml - mr;
            let room_h = ph - mt - mb;
            let ox = ml + if p.center_h { ((room_w - content_w * s) / 2.0).max(0.0) } else { 0.0 };
            let oy = mt + if p.center_v { ((room_h - content_h * s) / 2.0).max(0.0) } else { 0.0 };
            let px = |x: f32| ox + (x - x0) * s;
            let py = |y: f32| oy + y * s;
            let row_set: HashSet<u32> = rows.iter().copied().collect();
            let col_set = page.cols;

            // A merged region is drawn once per page, from its first cell visible there;
            // its value and style live in its top-left cell (which may be on another page)
            let anchor = |r: u32, c: u32| -> Option<(u32, u32)> {
                match plan.merged.get(&(r, c)) {
                    None => Some((r, c)),
                    Some(m) => {
                        let first_r = rows.iter().copied().find(|rr| *rr >= m.0 && *rr <= m.2)?;
                        let first_c = c.max(m.1).max(page.cols.0).min(m.1.max(page.cols.0));
                        ((r, c) == (first_r, first_c)).then_some((m.0, m.1))
                    }
                }
            };
            // 1. fills
            for &r in &rows {
                for &c in &cols {
                    let Some(src) = anchor(r, c) else { continue };
                    let Some(cell) = sheet.cells.get(&src).or_else(|| sheet.cells.get(&(r, c))) else { continue };
                    let Some(fill) = wb.styles[cell.style].fill else { continue };
                    let (w, h) = cell_box(grid, &plan.merged, r, c, &row_set, col_set);
                    if w <= 0.0 || h <= 0.0 {
                        continue;
                    }
                    let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, rect(width: {:.2}pt, height: {:.2}pt, fill: {}, stroke: none))", px(grid.x(c)), py(ry[&r]), w * s, h * s, rgb(fill));
                }
            }
            // data bars and icons of conditional formats
            let mut marks: Vec<_> = sheet.marks.iter().filter(|((r, c), _)| row_set.contains(r) && *c >= page.cols.0 && *c <= page.cols.1).collect();
            marks.sort_by_key(|(pos, _)| **pos);
            for (&(r, c), mark) in marks {
                let (x, y, w, h) = (px(grid.x(c)), py(ry[&r]), grid.w(c) * s, grid.h(r) * s);
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let inset = (1.5 * s).min(h / 4.0);
                match mark {
                    Mark::Bar { frac, color } => {
                        let bw = (w - 2.0 * inset) * frac;
                        let _ = writeln!(
                            out,
                            "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, rect(width: {bw:.2}pt, height: {:.2}pt, fill: gradient.linear({c}, {c}.lighten(85%)), stroke: 0.4pt + {c}))",
                            x + inset,
                            y + inset,
                            h - 2.0 * inset,
                            c = rgb(*color)
                        );
                    }
                    Mark::Icon { set, index, of, .. } => {
                        let size = (12.0 * s).min(h - 2.0 * inset).max(2.0);
                        let style = sheet.cells.get(&(r, c)).map(|cell| &wb.styles[cell.style]);
                        let dy = match style.map(|st| st.v) {
                            Some(VAlign::Top) => inset,
                            Some(VAlign::Center) => (h - size) / 2.0,
                            _ => h - size - inset,
                        };
                        let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, {})", x + inset, y + dy, icon(set, *index, *of, size));
                    }
                }
            }
            // 2. gridlines
            if p.gridlines {
                let stroke = "(paint: rgb(160, 160, 160), thickness: 0.4pt)";
                let bottom = py(content_h);
                for c in page.cols.0..=page.cols.1 + 1 {
                    let x = px(grid.x(c));
                    let _ = writeln!(out, "#place(top + left, line(start: ({x:.2}pt, {:.2}pt), end: ({x:.2}pt, {bottom:.2}pt), stroke: {stroke}))", py(0.0));
                }
                let mut y = 0.0;
                let right = px(grid.x(page.cols.1 + 1));
                for r in rows.iter().map(Some).chain([None]) {
                    let yy = py(y);
                    let _ = writeln!(out, "#place(top + left, line(start: ({:.2}pt, {yy:.2}pt), end: ({right:.2}pt, {yy:.2}pt), stroke: {stroke}))", px(x0));
                    if let Some(r) = r {
                        y += grid.h(*r);
                    }
                }
            }
            // 3. text
            for &r in &rows {
                for &c in &cols {
                    let Some(src) = anchor(r, c) else { continue };
                    let Some(cell) = sheet.cells.get(&src) else { continue };
                    let Some(value) = &cell.value else { continue };
                    if matches!(sheet.marks.get(&src), Some(Mark::Icon { show_value: false, .. })) {
                        continue;
                    }
                    let style = &wb.styles[cell.style];
                    let (cw, ch) = cell_box(grid, &plan.merged, r, c, &row_set, col_set);
                    if cw <= 0.0 || ch <= 0.0 {
                        continue;
                    }
                    let shown = numfmt::format(value, &style.num_fmt, wb.date1904, sep);
                    let is_number = matches!(value, Value::Number(_));
                    let h = match style.h {
                        HAlign::General => match value {
                            Value::Number(_) => HAlign::Right,
                            Value::Bool(_) | Value::Error(_) => HAlign::Center,
                            Value::Text(_) => HAlign::Left,
                        },
                        other => other,
                    };
                    // room for text that does not wrap: it overflows into empty neighbours
                    let in_merge = plan.merged.contains_key(&(r, c));
                    let (mut left, mut right) = (grid.x(c), grid.x(c) + cw);
                    if !style.wrap && !in_merge && !is_number && shown.fill.is_none() && style.rotation == 0 && !style.stacked {
                        let free = |cc: u32| !sheet.cells.get(&(r, cc)).map(|x| x.value.is_some()).unwrap_or(false) && !plan.merged.contains_key(&(r, cc));
                        if matches!(h, HAlign::Left | HAlign::Center | HAlign::CenterContinuous | HAlign::Justify) {
                            let mut cc = c + 1;
                            while cc <= page.cols.1 && free(cc) {
                                right = grid.x(cc + 1);
                                cc += 1;
                            }
                        }
                        if matches!(h, HAlign::Right | HAlign::Center | HAlign::CenterContinuous) {
                            let mut cc = c;
                            while cc > page.cols.0 && free(cc - 1) {
                                cc -= 1;
                                left = grid.x(cc);
                            }
                        }
                        // centred text overflows evenly
                        if matches!(h, HAlign::Center | HAlign::CenterContinuous) {
                            let mid = grid.x(c) + cw / 2.0;
                            let half = (mid - left).min(right - mid);
                            left = mid - half;
                            right = mid + half;
                        }
                    }
                    if let Some(m) = plan.merged.get(&(r, c)) {
                        // Excel aligns merged text over the whole merged width, even when
                        // the merge runs across pages: lay it out full width, show this page's slice
                        let full_l = grid.x(m.1.max(grid.range.1));
                        let full_r = grid.x(m.3.min(grid.range.3) + 1);
                        let vis_l = grid.x(c.max(page.cols.0));
                        let pieces = cell_pieces(&shown, style, sheet.rich.get(&src), value);
                        let markup = cell_text(&shown, pieces, style, h, is_number, full_r - full_l, ch, s, &default_font);
                        let _ = writeln!(
                            out,
                            "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, block(width: {:.2}pt, height: {:.2}pt, clip: true, place(top + left, dx: {:.2}pt, {markup})))",
                            px(vis_l),
                            py(ry[&r]),
                            cw * s,
                            ch * s,
                            (full_l - vis_l) * s
                        );
                        continue;
                    }
                    let pieces = cell_pieces(&shown, style, sheet.rich.get(&src), value);
                    let markup = cell_text(&shown, pieces, style, h, is_number, right - left, ch, s, &default_font);
                    let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, {markup})", px(left), py(ry[&r]));
                }
            }
            // 4. borders
            for &r in &rows {
                for &c in &cols {
                    let Some(cell) = sheet.cells.get(&(r, c)) else { continue };
                    let b = &wb.styles[cell.style].borders;
                    let (x1, y1) = (px(grid.x(c)), py(ry[&r]));
                    let (x2, y2) = (px(grid.x(c + 1)), py(ry[&r] + grid.h(r)));
                    if x2 <= x1 || y2 <= y1 {
                        continue;
                    }
                    let edges = [(b.top, (x1, y1, x2, y1)), (b.bottom, (x1, y2, x2, y2)), (b.left, (x1, y1, x1, y2)), (b.right, (x2, y1, x2, y2)), (b.up, (x1, y2, x2, y1)), (b.down, (x1, y1, x2, y2))];
                    for (edge, (a, bb, cc, d)) in edges {
                        if let Some(e) = edge {
                            draw_edge(&mut out, e, a, bb, cc, d, s);
                        }
                    }
                }
            }
            // 5. pictures
            for (k, pic) in sheet.pictures.iter().enumerate() {
                let (fr, fc, froff, fcoff) = pic.from;
                if fr < grid.range.0 || fc < grid.range.1 || fr > grid.range.2 + 1 || fc > grid.range.3 + 1 {
                    continue;
                }
                let sx = grid.xs.get((fc - grid.range.1) as usize).copied().unwrap_or(0.0) + fcoff;
                let sy = grid.ys.get((fr - grid.range.0) as usize).copied().unwrap_or(0.0) + froff;
                let (w, h) = match pic.to {
                    Some((tr, tc, troff, tcoff)) => {
                        let ex = grid.xs.get((tc.max(grid.range.1) - grid.range.1) as usize).copied().unwrap_or(*grid.xs.last().unwrap()) + tcoff;
                        let ey = grid.ys.get((tr.max(grid.range.0) - grid.range.0) as usize).copied().unwrap_or(*grid.ys.last().unwrap()) + troff;
                        ((ex - sx).max(1.0), (ey - sy).max(1.0))
                    }
                    None => pic.size,
                };
                // on this page when it starts inside the page's columns and printed rows
                let page_x = (grid.x(page.cols.0), grid.x(page.cols.1 + 1));
                let page_y = (grid.y(page.rows.0), grid.y(page.rows.1 + 1));
                if sx < page_x.0 - 0.5 || sx >= page_x.1 || sy < page_y.0 - 0.5 || sy >= page_y.1 {
                    continue;
                }
                let name = pic_files.entry((si, k)).or_insert_with(|| {
                    let (bytes, ext) = picture_bytes(&pic.data, &pic.ext);
                    let name = format!("/pic-{si}-{k}.{ext}");
                    files.push((name.clone(), bytes));
                    name
                });
                let offset_rows: f32 = rows.iter().take_while(|&&r| r < page.rows.0).map(|&r| grid.h(r)).sum();
                let y_on_page = offset_rows + (sy - page_y.0);
                let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, image({}, width: {:.2}pt, height: {:.2}pt, fit: \"stretch\"))", px(sx), py(y_on_page), typst_str(name), w * s, h * s);
            }
            // 5b. charts and shapes, placed like pictures
            for d in &sheet.drawn {
                let a = d.anchor();
                let (fr, fc, froff, fcoff) = a.from;
                if fr < grid.range.0 || fc < grid.range.1 || fr > grid.range.2 + 1 || fc > grid.range.3 + 1 {
                    continue;
                }
                let sx = grid.xs.get((fc - grid.range.1) as usize).copied().unwrap_or(0.0) + fcoff;
                let sy = grid.ys.get((fr - grid.range.0) as usize).copied().unwrap_or(0.0) + froff;
                let (w, h) = match a.to {
                    Some((tr, tc, troff, tcoff)) => {
                        let ex = grid.xs.get((tc.max(grid.range.1) - grid.range.1) as usize).copied().unwrap_or(*grid.xs.last().unwrap()) + tcoff;
                        let ey = grid.ys.get((tr.max(grid.range.0) - grid.range.0) as usize).copied().unwrap_or(*grid.ys.last().unwrap()) + troff;
                        ((ex - sx).max(1.0), (ey - sy).max(1.0))
                    }
                    None => a.size,
                };
                let page_x = (grid.x(page.cols.0), grid.x(page.cols.1 + 1));
                let page_y = (grid.y(page.rows.0), grid.y(page.rows.1 + 1));
                if sx < page_x.0 - 0.5 || sx >= page_x.1 || sy < page_y.0 - 0.5 || sy >= page_y.1 {
                    continue;
                }
                let offset_rows: f32 = rows.iter().take_while(|&&r| r < page.rows.0).map(|&r| grid.h(r)).sum();
                let (x, y) = (px(sx), py(offset_rows + (sy - page_y.0)));
                match d {
                    super::xlsx::Drawn::Chart(_, chart) => {
                        let body = super::chart::markup(chart, w, h, sep);
                        let _ = writeln!(
                            out,
                            "#place(top + left, dx: {x:.2}pt, dy: {y:.2}pt, scale(x: {pc:.3}%, y: {pc:.3}%, origin: top + left, reflow: true, box(width: {w:.2}pt, height: {h:.2}pt, clip: true)[\n{body}]))",
                            pc = s * 100.0
                        );
                    }
                    super::xlsx::Drawn::Shapes(_, shapes) => {
                        for sh in shapes {
                            let [fx, fy, fw, fh] = sh.frac;
                            let markup = super::drawing::shape_markup(sh, fw * w * s, fh * h * s, s);
                            let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, {markup})", x + fx * w * s, y + fy * h * s);
                        }
                    }
                }
            }
            // 6. header and footer
            let number = first_page + pi;
            for (spec, top) in [(&p.header, true), (&p.footer, false)] {
                let Some(spec) = spec else { continue };
                let parts = header_parts(spec, number, total, file_name, &sheet.name);
                for (i, text) in parts.iter().enumerate() {
                    if text.trim().is_empty() {
                        continue;
                    }
                    let align = ["left", "center", "right"][i];
                    let y = if top { format!("dy: {mh:.2}pt") } else { format!("dy: {:.2}pt", ph - mf - 14.0) };
                    let style = Style { font: wb.styles[0].font.clone(), ..Default::default() };
                    let t = text_expr(text, &style, None, 1.0, &default_font);
                    let _ = writeln!(out, "#place(top + left, dx: {ml:.2}pt, {y}, block(width: {:.2}pt, align({align}, {t})))", pw - ml - mr);
                }
            }
            out.push('\n');
            pages.push((sheets.len() - 1, out));
        }
    }
    Ok(Printed { preamble, sheets, pages, files })
}

/// Width and height of a cell (or of its merged region, limited to what this page shows)
fn cell_box(grid: &Grid, merged: &HashMap<(u32, u32), Range>, r: u32, c: u32, rows: &HashSet<u32>, cols: (u32, u32)) -> (f32, f32) {
    match merged.get(&(r, c)) {
        Some(m) => {
            let w: f32 = (m.1..=m.3).filter(|cc| *cc >= cols.0 && *cc <= cols.1 && *cc <= grid.range.3).map(|cc| grid.w(cc)).sum();
            let h: f32 = (m.0..=m.2).filter(|rr| rows.contains(rr) && *rr <= grid.range.2).map(|rr| grid.h(rr)).sum();
            (w, h)
        }
        None => (grid.w(c), grid.h(r)),
    }
}

/// A cell's text block: clipped to the cell (or its overflow room), aligned like Excel
#[allow(clippy::too_many_arguments)]
fn cell_text(shown: &Shown, pieces: Vec<Piece>, style: &Style, h: HAlign, is_number: bool, w: f32, ch: f32, s: f32, default_font: &str) -> String {
    let fams = families(&style.font.name);
    let indent = style.indent as f32 * 9.0;
    let inner = (w - 2.0 * PAD - indent).max(1.0);
    let v = match style.v {
        VAlign::Top => "top",
        VAlign::Center => "horizon",
        VAlign::Bottom => "bottom",
    };
    let block = |content: String, align: &str| {
        let (il, ir) = match h {
            HAlign::Left | HAlign::General | HAlign::Justify => (PAD + indent, PAD),
            HAlign::Right => (PAD, PAD + indent),
            _ => (PAD, PAD),
        };
        if (il, ir) == (PAD, PAD) {
            // the usual cell: `cell`, defined once per sheet (see `print`)
            return format!("cell({:.2}pt, {:.2}pt, align({align} + {v}, {content}))", w * s, ch * s);
        }
        format!(
            "block(width: {:.2}pt, height: {:.2}pt, clip: true, inset: (left: {:.2}pt, right: {:.2}pt, top: 0.5pt, bottom: 1pt), align({align} + {v}, {content}))",
            w * s,
            ch * s,
            il * s,
            ir * s
        )
    };
    let text_w = |t: &str| width(&fams, style.font.size, style.font.bold, style.font.italic, t);
    let hashes_expr = || pieces_expr(&Piece::plain("#".repeat(hashes(&fams, style, inner)), &style.font, shown.color), s, 1.0, 0.0, default_font);

    // accounting-style formats: the part before the fill hugs the left, the rest the right
    if let (Some(_), Some(after)) = (shown.fill, &shown.after_fill) {
        if text_w(&shown.text) + text_w(after) > inner + 0.5 && is_number {
            return block(hashes_expr(), "right");
        }
        let left = Piece::plain(shown.text.clone(), &style.font, shown.color);
        let right = Piece::plain(after.clone(), &style.font, shown.color);
        let (left_t, right_t) = (tracking(&left), tracking(&right));
        let (left, right) = (pieces_expr(&left, s, 1.0, left_t, default_font), pieces_expr(&right, s, 1.0, right_t, default_font));
        return block(format!("box(width: 100%, {left} + h(1fr) + {right})"), "left");
    }

    let align = match h {
        HAlign::Right => "right",
        HAlign::Center | HAlign::CenterContinuous => "center",
        _ => "left",
    };
    if style.stacked {
        // one letter per line, centred
        let stacked: String = shown.text.chars().filter(|c| *c != '\n').map(|c| c.to_string()).collect::<Vec<_>>().join("\n");
        let t = pieces_expr(&Piece::plain(stacked, &style.font, shown.color), s, 1.0, 0.0, default_font);
        return block(format!("par(leading: 0.14em, {t})"), "center");
    }
    if style.rotation != 0 {
        let pieces: Vec<Piece> = pieces.into_iter().map(|mut p| {
            p.text = p.text.replace('\n', " ");
            p
        }).collect();
        let t = pieces_expr(&pieces, s, 1.0, tracking(&pieces), default_font);
        let tw = pieces_width(&pieces) + 1.0;
        // anticlockwise in Excel, clockwise in Typst
        let content = format!("rotate({}deg, reflow: true, box(width: {:.2}pt, {t}))", -style.rotation, tw * s);
        // steep text stays in its cell; slanted text reaches over its neighbours like Excel's
        if style.rotation.abs() == 90 {
            return block(content, align);
        }
        return format!("box(width: {:.2}pt, height: {:.2}pt, inset: (x: {:.2}pt), align({align} + {v}, {content}))", w * s, ch * s, PAD * s);
    }

    let mut pieces = pieces;
    let mut shrink = 1.0;
    if !style.wrap {
        for p in &mut pieces {
            if p.text.contains('\n') {
                p.text = p.text.replace('\n', " ");
            }
        }
    }
    let mut tw = pieces_width(&pieces);
    if !style.wrap
        && tw > inner + 0.5 {
            if style.shrink {
                shrink = (inner / tw).max(4.0 / style.font.size);
            } else if is_number {
                // numbers never spill: Excel shows #### instead
                return block(hashes_expr(), align);
            }
        }
    if h == HAlign::Fill && pieces.len() == 1 && !pieces[0].text.is_empty() {
        let n = ((inner / tw.max(0.1)).floor() as usize).max(1);
        pieces[0].text = pieces[0].text.repeat(n);
        tw *= n as f32;
    }
    let track = tracking_for(&pieces, tw);
    let t = pieces_expr(&pieces, s, shrink, track, default_font);
    if style.wrap {
        block(format!("par(justify: {}, leading: 0.14em, {t})", h == HAlign::Justify), align)
    } else {
        // a box as wide as the text never wraps; what doesn't fit is clipped at the cell edge
        block(format!("box(width: {:.2}pt, {t})", (tw * shrink + 1.0) * s), align)
    }
}

/// An icon of a conditional format's icon set, `size` points square; `index` 0 stands for
/// the lowest values
fn icon(set: &str, index: usize, of: usize, size: f32) -> String {
    const RED: &str = "rgb(229, 57, 53)";
    const YELLOW: &str = "rgb(255, 192, 0)";
    const GREEN: &str = "rgb(0, 166, 81)";
    const GRAY: &str = "rgb(128, 128, 128)";
    const BLACK: &str = "rgb(64, 64, 64)";
    let k = size;
    let poly = |pts: &[(f32, f32)], fill: &str| {
        let pts: Vec<String> = pts.iter().map(|(x, y)| format!("({:.2}pt, {:.2}pt)", x * k, y * k)).collect();
        format!("polygon(fill: {fill}, {})", pts.join(", "))
    };
    let boxed = |inner: String| format!("box(width: {k:.2}pt, height: {k:.2}pt, {inner})");
    let circle = |fill: &str| boxed(format!("align(center + horizon, circle(radius: {:.2}pt, fill: {fill}))", k * 0.42));
    let up = [(0.5, 0.05), (0.95, 0.5), (0.68, 0.5), (0.68, 0.95), (0.32, 0.95), (0.32, 0.5), (0.05, 0.5)];
    let arrow = |deg: i32, fill: &str| boxed(format!("rotate({deg}deg, {})", poly(&up, fill)));
    // the position of `index` among `of` icons, 0.0 (lowest) to 1.0 (highest)
    let level = if of > 1 { index as f32 / (of - 1) as f32 } else { 1.0 };
    let traffic = |i: usize| match (of, i) {
        (4, 0) => BLACK,
        (_, 0) => RED,
        (n, i) if i + 1 == n => GREEN,
        _ => YELLOW,
    };
    let base = set.trim_start_matches(|c: char| c.is_ascii_digit());
    match base {
        "Arrows" | "ArrowsGray" => {
            let gray = base == "ArrowsGray";
            let deg = (180.0 - 180.0 * level).round() as i32;
            let fill = if gray { GRAY } else if index == 0 { RED } else if index + 1 == of { GREEN } else { YELLOW };
            arrow(deg, fill)
        }
        "Triangles" => match index {
            0 => boxed(poly(&[(0.1, 0.25), (0.9, 0.25), (0.5, 0.85)], RED)),
            i if i + 1 == of => boxed(poly(&[(0.5, 0.15), (0.9, 0.75), (0.1, 0.75)], GREEN)),
            _ => boxed(poly(&[(0.15, 0.4), (0.85, 0.4), (0.85, 0.6), (0.15, 0.6)], YELLOW)),
        },
        "Signs" => match index {
            0 => boxed(poly(&[(0.5, 0.05), (0.95, 0.5), (0.5, 0.95), (0.05, 0.5)], RED)),
            1 if of == 3 => boxed(poly(&[(0.5, 0.08), (0.95, 0.9), (0.05, 0.9)], YELLOW)),
            _ => circle(GREEN),
        },
        "Symbols" | "Symbols2" => {
            let (fill, mark) = match index {
                0 => (RED, "✕"),
                i if i + 1 == of => (GREEN, "✓"),
                _ => (YELLOW, "!"),
            };
            boxed(format!(
                "align(center + horizon, circle(radius: {:.2}pt, fill: {fill}, inset: 0pt, align(center + horizon, text(size: {:.2}pt, fill: white, weight: \"bold\", \"{mark}\"))))",
                k * 0.45,
                k * 0.62
            ))
        }
        "Flags" => {
            let fill = traffic(index);
            boxed(format!("{} + place(top + left, {})", poly(&[(0.2, 0.05), (0.28, 0.05), (0.28, 0.95), (0.2, 0.95)], BLACK), poly(&[(0.28, 0.08), (0.9, 0.3), (0.28, 0.52)], fill)))
        }
        "Stars" => {
            let star: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
                    let r = if i % 2 == 0 { 0.48 } else { 0.2 };
                    (0.5 + r * a.cos(), 0.52 + r * a.sin())
                })
                .collect();
            let fill = match index {
                0 => "rgb(255, 255, 255)",
                i if i + 1 == of => YELLOW,
                _ => "rgb(255, 224, 140)",
            };
            let pts: Vec<String> = star.iter().map(|(x, y)| format!("({:.2}pt, {:.2}pt)", x * k, y * k)).collect();
            boxed(format!("polygon(fill: {fill}, stroke: 0.5pt + rgb(214, 160, 0), {})", pts.join(", ")))
        }
        "Rating" | "Boxes" => {
            // bars (or boxes) lit up to the level
            let n = if base == "Boxes" { 4 } else { of - 1 };
            let lit = (level * n as f32).round() as usize;
            let mut parts = vec![];
            for i in 0..n {
                let fill = if i < lit { "rgb(31, 78, 121)" } else { "rgb(200, 200, 200)" };
                let (x, w, top) = if base == "Boxes" {
                    (0.05 + (i % 2) as f32 * 0.47, 0.42, 0.05 + (i / 2) as f32 * 0.47)
                } else {
                    let w = 0.8 / n as f32;
                    (0.1 + i as f32 * w, w * 0.8, 0.9 - 0.8 * (i + 1) as f32 / n as f32)
                };
                let bottom = if base == "Boxes" { top + 0.42 } else { 0.9 };
                parts.push(format!("place(top + left, {})", poly(&[(x, top), (x + w, top), (x + w, bottom), (x, bottom)], fill)));
            }
            boxed(parts.join(" + "))
        }
        "Quarters" => {
            let shade = (255.0 - 200.0 * level) as u8;
            boxed(format!("align(center + horizon, circle(radius: {:.2}pt, fill: rgb({shade}, {shade}, {shade}), stroke: 0.6pt + {BLACK}))", k * 0.4))
        }
        "RedToBlack" => {
            let fill = ["rgb(64, 64, 64)", "rgb(160, 160, 160)", "rgb(255, 150, 150)", "rgb(229, 57, 53)"];
            circle(fill[index.min(3)])
        }
        // TrafficLights1/2 and anything unknown: coloured circles
        _ => circle(traffic(index)),
    }
}

fn hashes(fams: &[String], style: &Style, room: f32) -> usize {
    let w = width(fams, style.font.size, style.font.bold, false, "#").max(1.0);
    ((room / w).floor() as usize).max(1)
}

fn draw_edge(out: &mut String, e: Edge, x1: f32, y1: f32, x2: f32, y2: f32, s: f32) {
    let (thick, dash) = match e.line {
        Line::Hair => (0.25, "none"),
        Line::Thin => (0.75, "none"),
        Line::Medium => (1.5, "none"),
        Line::Thick => (2.25, "none"),
        Line::Double => (0.6, "none"),
        Line::Dotted => (0.75, "\"dotted\""),
        Line::Dashed => (0.75, "\"dashed\""),
        Line::MediumDashed => (1.5, "\"dashed\""),
        Line::DashDot => (0.75, "\"dash-dotted\""),
    };
    // lines shrink with the sheet, down to the thinnest line that still prints clearly
    let t = (thick * s).max(0.3);
    let stroke = format!("(paint: {}, thickness: {t:.2}pt, dash: {dash})", rgb(e.color));
    if e.line == Line::Double {
        // two thin lines, one each side of the edge
        let d = 0.9;
        let (dx, dy) = if x1 == x2 { (d, 0.0) } else { (0.0, d) };
        for k in [-1.0, 1.0] {
            let _ = writeln!(out, "#place(top + left, line(start: ({:.2}pt, {:.2}pt), end: ({:.2}pt, {:.2}pt), stroke: {stroke}))", x1 + dx * k, y1 + dy * k, x2 + dx * k, y2 + dy * k);
        }
    } else {
        let _ = writeln!(out, "#place(top + left, line(start: ({x1:.2}pt, {y1:.2}pt), end: ({x2:.2}pt, {y2:.2}pt), stroke: {stroke}))");
    }
}

/// Typst reads PNG, JPEG and GIF; anything else is converted to PNG
fn picture_bytes(data: &[u8], ext: &str) -> (Vec<u8>, &'static str) {
    match ext {
        "png" => (data.to_vec(), "png"),
        "jpg" | "jpeg" => (data.to_vec(), "jpg"),
        "gif" => (data.to_vec(), "gif"),
        _ => {
            let mut png = vec![];
            if let Ok(img) = image::load_from_memory(data) {
                let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            }
            (png, "png")
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_split_and_break() {
        let size = |_: u32| 10.0;
        assert_eq!(bands(0, 9, &size, 35.0, &[]), vec![(0, 2), (3, 5), (6, 8), (9, 9)]);
        assert_eq!(bands(0, 5, &size, 100.0, &[2]), vec![(0, 1), (2, 5)]);
    }

    #[test]
    fn header_codes() {
        let p = header_parts("&L&\"Arial,Bold\"Laporan &A&RPage &P of &N", 2, 5, "x.xlsx", "Neraca");
        assert_eq!(p, ["Laporan Neraca".to_string(), String::new(), "Page 2 of 5".to_string()]);
    }

    /// A table `cols` wide and `rows` tall with a bold heading row
    fn table(cols: u32, rows: u32) -> Workbook {
        use super::super::xlsx::{Cell, Font};
        let mut sheet = Sheet { name: "T".into(), default_col_width: 64.0, default_row_height: 15.0, ..Default::default() };
        for c in 0..cols {
            sheet.cells.insert((0, c), Cell { value: Some(Value::Text(format!("Col {c}"))), style: 1 });
            for r in 1..rows {
                sheet.cells.insert((r, c), Cell { value: Some(Value::Number((r * c) as f64)), style: 0 });
            }
        }
        let normal = Style { num_fmt: "General".into(), ..Default::default() };
        let bold = Style { font: Font { bold: true, ..Default::default() }, ..normal.clone() };
        Workbook { sheets: vec![sheet], styles: vec![normal, bold], date1904: false }
    }

    #[test]
    fn wide_tables_fit_the_page_width() {
        // 20 columns of 64 pt: 1280 pt, Excel would print 3 pages across
        let wb = table(20, 10);
        let printed = print(&wb, Separators::US, "t.xlsx").unwrap();
        assert_eq!(printed.pages.len(), 1);
        assert!(printed.sheets[0].contains("width: 841.90pt"), "landscape A4: {}", printed.sheets[0]);
        // a sheet set up to print as it is keeps Excel's layout
        let mut wb = table(20, 10);
        wb.sheets[0].print.explicit = true;
        assert_eq!(print(&wb, Separators::US, "t.xlsx").unwrap().pages.len(), 3);
    }

    #[test]
    fn long_tables_repeat_their_heading() {
        let wb = table(5, 200);
        let printed = print(&wb, Separators::US, "t.xlsx").unwrap();
        assert!(printed.pages.len() > 2);
        assert!(printed.pages.iter().all(|(_, p)| p.contains("\"Col 3\"")), "the heading row is on every page");
        // narrow enough for portrait
        assert!(printed.sheets[0].contains("width: 595.30pt"));
    }

    #[test]
    fn typst_strings_are_escaped() {
        assert_eq!(typst_str("a \"b\" \\ #c"), "\"a \\\"b\\\" \\\\ #c\"");
    }
}
