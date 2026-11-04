// Prints a workbook to PDF the way Excel does: each visible sheet's print area (or
// used range) is split into pages down-then-over, scaled or fitted to the page, with
// repeated title rows, page breaks, centring, header and footer. Cells keep their
// fills, borders, fonts and number formats; text overflows into empty neighbours,
// numbers that don't fit show ####, and rows without a stored height grow to fit
// their wrapped text.
//
// Everything is positioned absolutely (in points) and typeset with Typst.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::numfmt::{self, Separators, Shown, Value, HIDE_OFF, HIDE_ON};
use super::xlsx::{Edge, HAlign, Line, Range, Sheet, Style, VAlign, Workbook};

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

/// Font families to try for an Excel font: itself, then metric-compatible and common look-alikes
fn families(name: &str) -> Vec<String> {
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

/// Width of `text` in the first installed family, or an estimate
fn width(fams: &[String], size: f32, bold: bool, italic: bool, text: &str) -> f32 {
    let plain: String = text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).collect();
    for f in fams {
        if let Some(w) = super::typeset::text_width(f, size, bold, italic, &plain) {
            return w;
        }
    }
    plain.chars().count() as f32 * size * if bold { 0.56 } else { 0.52 }
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

/// A Typst expression for `text` (with hidden padding runs) in a style
/// Typst's list of `families` for an Excel font name
fn font_list(name: &str) -> String {
    families(name).iter().map(|s| typst_str(s)).collect::<Vec<_>>().join(", ")
}

/// `default_font` is the `font_list` the document sets for all text (see `print`), so
/// only cells in another font name theirs: a workbook has tens of thousands of cells, and
/// every byte of markup per cell costs Typst time and memory.
fn text_expr(text: &str, style: &Style, color: Option<[u8; 3]>, scale: f32, size_override: Option<f32>, default_font: &str) -> String {
    let f = &style.font;
    let fams_list = font_list(&f.name);
    let size = size_override.unwrap_or(f.size) * scale;
    let fill = color.or(f.color).unwrap_or([0, 0, 0]);
    let mut base = format!("size: {size:.2}pt");
    if fams_list != default_font {
        base = format!("font: ({fams_list},), {base}");
    }
    if f.bold {
        base.push_str(", weight: \"bold\"");
    }
    if f.italic {
        base.push_str(", style: \"italic\"");
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
    for c in text.chars() {
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
        return "[]".into();
    }
    let mut e = parts.join(" + ");
    if f.underline {
        e = format!("underline({e})");
    }
    if f.strike {
        e = format!("strike({e})");
    }
    e
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
    for p in &sheet.pictures {
        grow(p.from.0, p.from.1);
        if let Some(to) = p.to {
            grow(to.0, to.1.saturating_sub(if to.3 == 0.0 { 1 } else { 0 }));
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
        let lh = line_height(&fams, style.font.size);
        let mut need = lh;
        if style.wrap {
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
// Pages

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
        let page_size = if p.landscape { (p.paper.1, p.paper.0) } else { p.paper };
        let [ml, mr, mt, mb, _, _] = p.margins;
        let room_w = (page_size.0 - ml - mr).max(36.0);
        let room_h = (page_size.1 - mt - mb).max(36.0);
        let titles = sheet.title_rows.filter(|t| t.0 <= t.1);
        let titles_h = titles.map(|(a, b)| (a..=b).map(|r| sheet.row_heights.get(&r).copied().unwrap_or(sheet.default_row_height)).sum::<f32>()).unwrap_or(0.0);

        let total_w = *grid.xs.last().unwrap();
        let total_h = *grid.ys.last().unwrap();
        let scale = match p.fit {
            Some((fw, fh)) => {
                let sx = if fw > 0 { room_w * fw as f32 / total_w.max(1.0) } else { f32::MAX };
                let sy = if fh > 0 { room_h * fh as f32 / total_h.max(1.0) } else { f32::MAX };
                sx.min(sy).min(1.0).max(0.1)
            }
            None => p.scale / 100.0,
        };
        let col_bands = bands(range.1, range.3, &|c| grid.w(c) * scale, room_w, &sheet.col_breaks);
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
            if let Some((t1, t2)) = plan.titles {
                if page.rows.0 > t2 {
                    rows.extend(t1.max(grid.range.0)..=t2.min(grid.range.2));
                }
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
                    if !style.wrap && !in_merge && !is_number && shown.fill.is_none() {
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
                        let markup = cell_text(&shown, style, h, is_number, full_r - full_l, ch, s, &default_font);
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
                    let markup = cell_text(&shown, style, h, is_number, right - left, ch, s, &default_font);
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
                    let edges = [(b.top, (x1, y1, x2, y1)), (b.bottom, (x1, y2, x2, y2)), (b.left, (x1, y1, x1, y2)), (b.right, (x2, y1, x2, y2))];
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
                    let t = text_expr(text, &style, None, 1.0, None, &default_font);
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
fn cell_text(shown: &Shown, style: &Style, h: HAlign, is_number: bool, w: f32, ch: f32, s: f32, default_font: &str) -> String {
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

    // accounting-style formats: the part before the fill hugs the left, the rest the right
    if let (Some(_), Some(after)) = (shown.fill, &shown.after_fill) {
        if text_w(&shown.text) + text_w(after) > inner + 0.5 && is_number {
            return block(text_expr(&"#".repeat(hashes(&fams, style, inner)), style, shown.color, s, None, default_font), "right");
        }
        let left = text_expr(&shown.text, style, shown.color, s, None, default_font);
        let right = text_expr(after, style, shown.color, s, None, default_font);
        return block(format!("box(width: 100%, {left} + h(1fr) + {right})"), "left");
    }

    let mut text = shown.text.clone();
    let mut size = None;
    if !style.wrap {
        text = text.replace('\n', " ");
        let tw = text_w(&text);
        if tw > inner + 0.5 {
            if style.shrink {
                size = Some((style.font.size * inner / tw).max(4.0));
            } else if is_number {
                // numbers never spill: Excel shows #### instead
                text = "#".repeat(hashes(&fams, style, inner));
            }
        }
    }
    if h == HAlign::Fill && !text.is_empty() {
        let tw = text_w(&text).max(0.1);
        let n = ((inner / tw).floor() as usize).max(1);
        text = text.repeat(n);
    }
    let align = match h {
        HAlign::Right => "right",
        HAlign::Center | HAlign::CenterContinuous => "center",
        _ => "left",
    };
    let t = text_expr(&text, style, shown.color, s, size, default_font);
    if style.wrap {
        block(format!("par(justify: {}, leading: 0.14em, {t})", h == HAlign::Justify), align)
    } else {
        // a box as wide as the text never wraps; what doesn't fit is clipped at the cell edge
        let tw = text_w(&text) * size.map(|z| z / style.font.size).unwrap_or(1.0);
        block(format!("box(width: {:.2}pt, {t})", (tw + 1.0) * s), align)
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
    // lines keep their weight when the sheet is scaled down, like a printer driver does
    let t = (thick * s.max(0.6)).max(0.25);
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
    let printed = print(wb, sep, file_name)?;
    // Typeset in runs of 4 pages, side by side, into one PDF: measured best (8 pages per
    // run is slower, and a whole workbook at once needs twice the memory and time)
    let per_run = 4;
    let runs: Vec<String> =
        (0..printed.pages.len()).step_by(per_run).map(|i| printed.chunk(i..(i + per_run).min(printed.pages.len()))).collect();
    super::typeset::to_pdf_in_runs(runs, printed.files).map_err(|e| format!("the workbook could not be laid out ({e})"))
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

    #[test]
    fn typst_strings_are_escaped() {
        assert_eq!(typst_str("a \"b\" \\ #c"), "\"a \\\"b\\\" \\\\ #c\"");
    }
}
