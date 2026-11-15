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

// ---------------------------------------------------------------------------
// Numbers, amounts and dates

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notation {
    /// 1.234.567,89
    Indonesian,
    /// 1,234,567.89
    English,
}

/// Which notation a document writes its numbers in, from the ones that can only be read
/// one way
pub fn notation(texts: &[String]) -> Notation {
    let (mut id, mut en) = (0, 0);
    for t in texts {
        for token in t.split_whitespace() {
            let token = token.trim_matches(|c: char| !c.is_ascii_digit());
            let (dots, commas) = (token.matches('.').count(), token.matches(',').count());
            if dots + commas == 0 || !token.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
                continue;
            }
            let last = token.rfind(['.', ',']).unwrap();
            let after = token.len() - last - 1;
            match (dots, commas) {
                (d, c) if d > 0 && c > 0 => {
                    if token.as_bytes()[last] == b',' { id += 2 } else { en += 2 }
                }
                (d, 0) if d > 1 => id += 1,
                (0, c) if c > 1 => en += 1,
                (1, 0) if after != 3 => en += 1,
                (0, 1) if after != 3 => id += 1,
                _ => {}
            }
        }
    }
    if id > en { Notation::Indonesian } else { Notation::English }
}

const MONTHS: [(&str, u32); 30] = [
    ("januari", 1), ("january", 1), ("februari", 2), ("february", 2), ("maret", 3), ("march", 3), ("april", 4), ("mei", 5), ("may", 5),
    ("juni", 6), ("june", 6), ("juli", 7), ("july", 7), ("agustus", 8), ("august", 8), ("september", 9), ("oktober", 10), ("october", 10),
    ("november", 11), ("desember", 12), ("december", 12), ("agu", 8), ("ags", 8), ("aug", 8), ("okt", 10), ("oct", 10), ("des", 12), ("dec", 12),
    ("sept", 9), ("sep", 9),
];

fn month(word: &str) -> Option<(u32, bool)> {
    let w = word.trim_end_matches('.').to_lowercase();
    if let Some((_, m)) = MONTHS.iter().find(|(name, _)| *name == w) {
        // Indonesian names that differ from English ones
        let indonesian = matches!(w.as_str(), "januari" | "februari" | "maret" | "mei" | "juni" | "juli" | "agustus" | "oktober" | "desember" | "agu" | "ags" | "okt" | "des");
        return Some((*m, indonesian));
    }
    // three-letter abbreviations: jan, feb, mar, apr, jun, jul, nov
    let short = ["jan", "feb", "mar", "apr", "mei", "jun", "jul", "agu", "sep", "okt", "nov", "des"];
    let english = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    short.iter().position(|s| *s == w).map(|i| (i as u32 + 1, english[i] != w)).or_else(|| english.iter().position(|s| *s == w).map(|i| (i as u32 + 1, false)))
}

fn year(y: u32) -> u32 {
    match y {
        0..=29 => 2000 + y,
        30..=99 => 1900 + y,
        y => y,
    }
}

/// A date (and time) written out -> serial number and a format that shows it the same way
fn date(text: &str, notation: Notation) -> Option<(f64, String)> {
    let t = text.trim();
    if t.len() < 6 || t.len() > 30 {
        return None;
    }
    let (day_part, time_part) = match t.rsplit_once(' ') {
        Some((d, tm)) if tm.contains(':') => (d.trim(), Some(tm)),
        _ => (t, None),
    };
    let serial = |y: u32, m: u32, d: u32| -> Option<f64> {
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1900..=2200).contains(&y) {
            return None;
        }
        super::formula::serial(y as i64, m as i64, d as i64)
    };
    let parts: Vec<&str> = day_part.split(['/', '-', '.', ' ', ',']).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let num = |s: &str| s.parse::<u32>().ok().filter(|_| s.chars().all(|c| c.is_ascii_digit()));
    let sep = day_part.chars().find(|c| matches!(c, '/' | '-' | '.')).unwrap_or('/');
    let (value, fmt) = match (num(parts[0]), num(parts[1]), num(parts[2])) {
        // 2026-11-15
        (Some(y), Some(m), Some(d)) if parts[0].len() == 4 => (serial(y, m, d)?, format!("yyyy{sep}mm{sep}dd")),
        // 15/11/2026 or 11/15/2026
        (Some(a), Some(b), Some(y)) if parts[2].len() == 4 || parts[2].len() == 2 => {
            let day_first = if a > 12 { true } else if b > 12 { false } else { notation == Notation::Indonesian };
            let (d, m) = if day_first { (a, b) } else { (b, a) };
            let yy = if parts[2].len() == 2 { "yy" } else { "yyyy" };
            let f = if day_first { format!("dd{sep}mm{sep}{yy}") } else { format!("mm{sep}dd{sep}{yy}") };
            (serial(year(y), m, d)?, f)
        }
        // 15 November 2026, 15-Nov-26
        (Some(d), None, Some(y)) => {
            let (m, indonesian) = month(parts[1])?;
            let long = parts[1].trim_end_matches('.').len() > 3;
            let yy = if parts[2].len() == 2 { "yy" } else { "yyyy" };
            let mm = if long { "mmmm" } else { "mmm" };
            let s = if sep == ' ' || day_part.contains(' ') { ' ' } else { sep };
            let lang = if indonesian { "[$-421]" } else { "" };
            (serial(year(y), m, d)?, format!("{lang}d{s}{mm}{s}{yy}"))
        }
        // November 15, 2026
        (None, Some(d), Some(y)) if parts[2].len() == 4 => {
            let (m, _) = month(parts[0])?;
            (serial(y, m, d)?, "mmmm d, yyyy".to_string())
        }
        _ => return None,
    };
    match time_part {
        Some(tm) => {
            let (frac, tf) = time(tm)?;
            Some((value + frac, format!("{fmt} {tf}")))
        }
        None => Some((value, fmt)),
    }
}

/// "08:30" or "08:30:15" -> fraction of a day
fn time(t: &str) -> Option<(f64, &'static str)> {
    let parts: Vec<u32> = t.trim().split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts.as_slice() {
        [h, m] if *h < 24 && *m < 60 => Some(((*h as f64 * 60.0 + *m as f64) / 1440.0, "hh:mm")),
        [h, m, s] if *h < 24 && *m < 60 && *s < 60 => Some(((*h as f64 * 3600.0 + *m as f64 * 60.0 + *s as f64) / 86400.0, "hh:mm:ss")),
        _ => None,
    }
}

/// A cell's text as Excel would hold it: a number with the format that shows it as
/// written, or the text itself
pub fn typed(text: &str, notation: Notation) -> (Value, Option<String>) {
    let t = text.trim();
    let keep = || (Value::Text(t.to_string()), None);
    if t.is_empty() {
        return (Value::Empty, None);
    }
    if t.len() > 40 || t.chars().filter(|c| c.is_alphabetic()).count() > 12 {
        return keep();
    }
    if let Some(d) = date(t, notation) {
        return (Value::Number(d.0), Some(d.1));
    }
    if t.contains(':') {
        return match time(t) {
            Some((f, fmt)) => (Value::Number(f), Some(fmt.to_string())),
            None => keep(),
        };
    }
    let mut s = t.replace(['\u{a0}', ' '], "");
    // "+Rp 9.537.000" (phone numbers stay text: too many digits)
    if let Some(rest) = s.strip_prefix('+') {
        s = rest.to_string();
    }
    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].to_string();
    }
    // currencies before or after the amount
    let mut currency = None;
    for (sym, fmt) in [("IDR", "\"IDR\" "), ("Rp.", "\"Rp\""), ("Rp", "\"Rp\""), ("USD", "\"USD\" "), ("US$", "\"US$\""), ("$", "$"), ("€", "€"), ("£", "£"), ("¥", "¥")] {
        if let Some(rest) = s.strip_prefix(sym) {
            currency = Some(fmt);
            s = rest.to_string();
            break;
        }
        if let Some(rest) = s.strip_suffix(sym) {
            currency = Some(fmt);
            s = rest.to_string();
            break;
        }
    }
    if s.starts_with('-') || s.starts_with('−') {
        negative = !negative;
        s = s.trim_start_matches(['-', '−']).to_string();
    } else if s.ends_with('-') && s.len() > 1 {
        negative = !negative;
        s.pop();
    }
    let percent = s.ends_with('%');
    if percent {
        s.pop();
    }
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') || !s.starts_with(|c: char| c.is_ascii_digit()) {
        return keep();
    }
    let (dec, grp) = match notation {
        Notation::Indonesian => (',', '.'),
        Notation::English => ('.', ','),
    };
    // a lone separator of the other kind still reads as a decimal point when it can't
    // be a thousands separator ("12.5" in an Indonesian document)
    let (int_part, frac_part) = match s.rsplit_once(dec) {
        Some((i, f)) if !f.contains(grp) => (i.to_string(), Some(f.to_string())),
        Some(_) => return keep(),
        None => match s.rsplit_once(grp) {
            Some((i, f)) if !i.contains(grp) && f.len() != 3 && !i.contains(dec) => (i.to_string(), Some(f.to_string())),
            _ => (s.clone(), None),
        },
    };
    // the decimal separator can't come before the last group
    if int_part.contains(dec) {
        return keep();
    }
    let grouped = int_part.contains(grp);
    if grouped {
        let groups: Vec<&str> = int_part.split(grp).collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return keep();
        }
    }
    let digits: String = int_part.chars().filter(|c| c.is_ascii_digit()).collect();
    // codes, account and ID numbers: leading zeros, or too many digits for a number
    if (digits.len() > 1 && digits.starts_with('0') && frac_part.is_none() && !grouped) || (digits.len() >= 12 && !grouped) || digits.len() > 15 {
        return keep();
    }
    if frac_part.as_ref().is_some_and(|f| f.is_empty() || f.contains([dec, grp])) {
        return keep();
    }
    let decimals = frac_part.as_ref().map(|f| f.len()).unwrap_or(0);
    let Ok(mut n) = format!("{digits}.{}", frac_part.as_deref().unwrap_or("0")).parse::<f64>() else { return keep() };
    if negative {
        n = -n;
    }
    let zeros = if decimals > 0 { format!(".{}", "0".repeat(decimals.min(10))) } else { String::new() };
    if percent {
        return (Value::Number(n / 100.0), Some(format!("0{zeros}%")));
    }
    let body = if grouped || currency.is_some() || n.abs() >= 10_000.0 && decimals > 0 { format!("#,##0{zeros}") } else if decimals > 0 { format!("0{zeros}") } else { String::new() };
    let fmt = match (currency, body.is_empty()) {
        (Some(c), _) => {
            let b = if body.is_empty() { "#,##0".to_string() } else { body };
            Some(if t.starts_with('(') { format!("{c}{b};({c}{b})") } else { format!("{c}{b}") })
        }
        (None, false) => Some(if t.starts_with('(') { format!("{body};({body})") } else { body }),
        (None, true) => None,
    };
    (Value::Number(n), fmt)
}

// ---------------------------------------------------------------------------
// Laying the pages out on a sheet

/// A piece of a line between its wide gaps (a column's text)
#[derive(Clone, Debug)]
struct Segment {
    text: String,
    style: Style,
    left: f32,
    right: f32,
}

/// Gaps of at least this many font sizes always part columns; narrower ones (down to the
/// gap the page is read with) only inside a table whose rows all have them
const STRONG_GAP: f32 = 1.0;

/// A line's parts between wide gaps
fn segments(line: &Line) -> Vec<Segment> {
    let mut out: Vec<Segment> = vec![];
    for s in fine_segments(line) {
        match out.last_mut() {
            Some(prev) if s.left - prev.right < s.style.size.max(prev.style.size) * STRONG_GAP => {
                prev.text.push(' ');
                prev.text.push_str(&s.text);
                prev.right = s.right;
            }
            _ => out.push(s),
        }
    }
    out
}

/// A line's parts between every gap the page was read with
fn fine_segments(line: &Line) -> Vec<Segment> {
    split_segments(line, &[])
}

/// `fine_segments`, also parted at the spaces where a column starts (`bounds`, found
/// from the other rows of a table)
fn split_segments(line: &Line, bounds: &[f32]) -> Vec<Segment> {
    let gaps: std::collections::HashMap<usize, (f32, f32)> = line.gaps.iter().map(|&(i, a, b)| (i, (a, b))).collect();
    let mut out: Vec<Segment> = vec![];
    let mut cur = String::new();
    let mut style: Option<Style> = None;
    let mut tab = 0;
    // where the current part starts when a split space began it
    let mut left_here: Option<f32> = None;
    let push = |out: &mut Vec<Segment>, cur: &mut String, style: &Option<Style>, left: f32, right: f32| {
        let text = std::mem::take(cur);
        if text.trim().is_empty() {
            return;
        }
        let st = style.clone().unwrap_or_else(|| line.runs[0].1.clone());
        out.push(Segment { text: text.trim().to_string(), style: st, left, right: right.max(left + 1.0) });
    };
    let tab_left = |tab: usize| if tab == 0 { line.left } else { line.tabs[tab - 1].left };
    let mut idx = 0;
    for (text, st) in &line.runs {
        for ch in text.chars() {
            if ch == '\t' {
                let left = left_here.take().unwrap_or_else(|| tab_left(tab));
                // up to where the next part's gap starts
                let right = line.tabs.get(tab).map(|t| t.before).unwrap_or(line.right);
                push(&mut out, &mut cur, &style, left, right);
                style = None;
                tab += 1;
            } else if ch == ' '
                && !cur.trim().is_empty()
                // not inside an expression ("Rp 100.000 – Rp 71.500")
                && !cur.trim_end().ends_with(['–', '-', '×', '+', '=', '/', ':', '(']) 
                && !matches!(cur.split_whitespace().last(), Some("Rp" | "Rp." | "IDR" | "$" | "x" | "X"))
                && gaps.get(&idx).is_some_and(|(_, after)| bounds.iter().any(|b| (after - b).abs() < 2.5))
            {
                let (before, after) = gaps[&idx];
                let left = left_here.take().unwrap_or_else(|| tab_left(tab));
                push(&mut out, &mut cur, &style, left, before);
                style = None;
                left_here = Some(after);
            } else {
                if style.is_none() && !ch.is_whitespace() {
                    style = Some(st.clone());
                }
                cur.push(ch);
            }
            idx += 1;
        }
    }
    let left = left_here.take().unwrap_or_else(|| tab_left(tab));
    push(&mut out, &mut cur, &style, left, line.right);
    out
}

/// Where columns of a table start: the starts of its heading's parts, and places where
/// several rows start a part
fn column_starts(lines: &[Line]) -> Vec<f32> {
    let parts: Vec<Vec<Segment>> = lines.iter().map(fine_segments).collect();
    let mut starts: Vec<f32> = parts.iter().take(2).flatten().map(|s| s.left).collect();
    let mut all: Vec<f32> = parts.iter().flatten().map(|s| s.left).collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut i = 0;
    while i < all.len() {
        let mut j = i;
        while j + 1 < all.len() && all[j + 1] - all[i] < 2.0 {
            j += 1;
        }
        if j - i + 1 >= 3 {
            starts.push(all[i]);
        }
        i = j + 1;
    }
    starts
}

/// What the page body reads as, in order
enum Item<'a> {
    Line(Line),
    Table(&'a from_pdf::Table),
}

/// A table only by layout: text set side by side without lines or shading, or a box
/// drawn around a part of the page (an address, a table of its own) rather than cells
fn layout_only(t: &from_pdf::Table) -> bool {
    if t.cells.iter().all(|c| c.fill.is_none() && c.borders.iter().all(Option::is_none)) {
        return true;
    }
    let width = t.xs.last().copied().unwrap_or(0.0) - t.xs.first().copied().unwrap_or(0.0);
    t.cells.iter().any(|c| {
        let mut lines = vec![];
        collect_lines(&c.blocks, &mut lines);
        let table_rows = lines.iter().filter(|l| segments(l).len() >= 3).count();
        // a box: one cell, a cell holding a table of its own, or a wide one holding a
        // page's worth of text
        (t.cells.len() == 1 && lines.len() >= 2) || table_rows >= 2 || (c.rect.w() >= width * 0.7 && lines.len() > 8)
    })
}

/// Lines side by side (the same baseline) joined into one, the gaps between them tabs:
/// the row they were before a layout split them into columns
fn rejoin(mut lines: Vec<Line>) -> Vec<Line> {
    lines.sort_by(|a, b| a.baseline.partial_cmp(&b.baseline).unwrap_or(std::cmp::Ordering::Equal).then(a.left.partial_cmp(&b.left).unwrap_or(std::cmp::Ordering::Equal)));
    let mut out: Vec<Line> = vec![];
    for l in lines {
        match out.last_mut() {
            Some(prev) if (prev.baseline - l.baseline).abs() < prev.size.min(l.size) * 0.4 && l.left > prev.right => {
                let style = l.runs.first().map(|r| r.1.clone()).unwrap_or_else(|| prev.runs[0].1.clone());
                prev.runs.push(("\t".into(), style));
                prev.tabs.push(from_pdf::TabMark { left: l.left, right: l.right, leader: false, before: prev.right });
                // the joined line's own tabs keep their places
                let first_text_right = l.tabs.first().map(|t| t.left).unwrap_or(l.right);
                if let Some(t) = prev.tabs.last_mut() {
                    t.right = if l.tabs.is_empty() { l.right } else { first_text_right };
                }
                prev.tabs.extend(l.tabs.iter().copied());
                let offset = prev.text().chars().count();
                prev.gaps.extend(l.gaps.iter().map(|&(i, a, b)| (i + offset, a, b)));
                prev.runs.extend(l.runs);
                prev.right = prev.right.max(l.right);
                prev.size = prev.size.max(l.size);
            }
            _ => out.push(l),
        }
    }
    out
}

fn items<'a>(blocks: &'a [Block], out: &mut Vec<Item<'a>>) {
    for b in blocks {
        match &b.content {
            Content::Paragraph(lines) => out.extend(lines.iter().cloned().map(Item::Line)),
            Content::Table(t) if layout_only(t) => {
                let mut lines = vec![];
                for c in &t.cells {
                    collect_lines(&c.blocks, &mut lines);
                }
                out.extend(rejoin(lines.into_iter().cloned().collect()).into_iter().map(Item::Line));
            }
            Content::Table(t) => out.push(Item::Table(t)),
            Content::Images(_) => {}
        }
    }
}

/// A table on the sheet, to continue on the next page
#[derive(Clone)]
struct Placed {
    /// its heading row's texts
    heading: Vec<String>,
    /// where its columns are on the page (left, right) and on the sheet
    spans: Vec<(f32, f32, u32)>,
}

struct Builder {
    sheet: w::Sheet,
    row: u32,
    notation: Notation,
    /// the table the sheet ended with
    last: Option<Placed>,
    /// widest content of each column, in points
    widths: BTreeMap<u32, f32>,
    first_table: bool,
    /// bottom of what was placed last on this page, in points (None at a page's top)
    bottom: Option<f32>,
    /// the page's filled bands and boxes (behind text: a cell's shading)
    fills: Vec<(from_pdf::Rect, [u8; 3])>,
}

fn look_of(style: &Style) -> Look {
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
    fn put(&mut self, r: u32, c: u32, text: &str, mut look: Look) {
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
    fn fill_at(&self, s: &Segment, baseline: f32) -> Option<[u8; 3]> {
        let (x, y) = ((s.left + s.right) / 2.0, baseline - s.style.size * 0.35);
        self.fills
            .iter()
            .filter(|(r, _)| x >= r.l && x <= r.r && y >= r.t && y <= r.b)
            .min_by(|a, b| (a.0.w() * a.0.h()).partial_cmp(&(b.0.w() * b.0.h())).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, c)| *c)
    }

    fn width(&mut self, c: u32, pt: f32) {
        let e = self.widths.entry(c).or_insert(0.0);
        *e = e.max(pt);
    }

    /// Lines of text that aren't a table: one row each, gaps splitting them over columns
    fn text_line(&mut self, line: &Line) {
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

    /// Lines whose columns line up: a table drawn without lines
    fn stream_table(&mut self, lines: &[Line], page_top: bool) {
        // every gap counts inside a table: columns closer than a font size apart (a date
        // and its description) are still columns when the rows agree on them
        let bounds = column_starts(lines);
        let rows: Vec<Vec<Segment>> = lines.iter().map(|l| split_segments(l, &bounds)).collect();
        let strong: Vec<usize> = lines.iter().map(|l| segments(l).len()).collect();
        // columns from the lines with the usual number of parts (data rows, not a heading
        // split over two lines), the others fitted in
        let most = {
            let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
            for r in rows.iter().filter(|r| r.len() >= 2) {
                *counts.entry(r.len()).or_default() += 1;
            }
            counts.iter().max_by_key(|(len, n)| (**n, **len)).map(|(len, _)| *len).unwrap_or(0)
        };
        // each column where most of those rows have it (medians: a stray long text doesn't
        // widen it over its neighbours)
        let mut edges: Vec<(Vec<f32>, Vec<f32>)> = vec![];
        for r in rows.iter().filter(|r| r.len() == most) {
            for (i, s) in r.iter().enumerate() {
                if edges.len() <= i {
                    edges.push((vec![], vec![]));
                }
                edges[i].0.push(s.left);
                edges[i].1.push(s.right);
            }
        }
        let median = |v: &mut Vec<f32>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v[v.len() / 2]
        };
        let mut cols: Vec<(f32, f32)> = edges.iter_mut().map(|(l, r)| (median(l), median(r))).collect();
        let column_of = |cols: &mut Vec<(f32, f32)>, s: &Segment| -> usize {
            let overlap = |c: &(f32, f32)| (s.right.min(c.1) - s.left.max(c.0)).max(0.0);
            if let Some((i, _)) = cols.iter().enumerate().filter(|(_, c)| overlap(c) > 0.0).max_by(|a, b| overlap(a.1).partial_cmp(&overlap(b.1)).unwrap_or(std::cmp::Ordering::Equal)) {
                return i;
            }
            // between two columns' text: it starts after the left one's text ends, so it is
            // the right one's (a heading set at the start of a column of right-aligned
            // numbers), unless that is too far off
            if let Some((i, _)) = cols.iter().enumerate().filter(|(_, c)| c.0 >= s.left && c.0 - s.right < s.style.size * 4.0).min_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap_or(std::cmp::Ordering::Equal)) {
                return i;
            }
            // between columns: a column of its own (at the end: rows placed already keep
            // their columns' numbers; the order left to right is worked out after)
            cols.push((s.left, s.right));
            cols.len() - 1
        };
        let mut placed_rows: Vec<Vec<(usize, Segment)>> = vec![];
        for r in &rows {
            let mut cells = vec![];
            for s in r {
                let i = column_of(&mut cols, s);
                cells.push((i, s.clone()));
            }
            placed_rows.push(cells);
        }
        // columns as found, left to right; text moves when a column was added later
        let order: Vec<usize> = {
            let mut idx: Vec<usize> = (0..cols.len()).collect();
            idx.sort_by(|a, b| cols[*a].0.partial_cmp(&cols[*b].0).unwrap_or(std::cmp::Ordering::Equal));
            let mut rank = vec![0; cols.len()];
            for (k, i) in idx.into_iter().enumerate() {
                rank[i] = k;
            }
            rank
        };
        let mut grid: Vec<Vec<(String, Option<Style>)>> = vec![];
        let mut shading: Vec<Vec<Option<[u8; 3]>>> = vec![];
        let mut wraps: Vec<bool> = vec![];
        // a wrapped cell's lines above its row (text set to the bottom of a tall row)
        let mut ahead: Vec<(usize, String)> = vec![];
        let full_rows: Vec<usize> = (0..placed_rows.len()).filter(|k| strong[*k] >= 2).collect();
        // baseline of the last line put in each column of the row before
        let mut col_base: Vec<Option<f32>> = vec![None; cols.len()];
        for (k, cells) in placed_rows.iter().enumerate() {
            let full = strong[k] >= 2;
            // a line with one part: more text of a cell in the nearer row, above or below
            if !full && cells.len() == 1 {
                let (i, s) = &cells[0];
                let col = order[*i];
                let base = lines[k].baseline;
                let prev = col_base.get(col).copied().flatten().map(|b| base - b);
                let next = full_rows.iter().find(|r| **r > k).map(|r| lines[*r].baseline - base);
                // only a line that fits its column can be more of a cell's text (not a
                // paragraph under the table)
                let fits = cols.get(*i).is_some_and(|c| s.right - s.left <= (c.1 - c.0) * 1.4 + s.style.size * 2.0);
                let texty = fits && typed(&s.text, self.notation).0.is_text();
                let joins_prev = grid.last().is_some_and(|p: &Vec<(String, Option<Style>)>| col < p.len() && !p[col].0.is_empty() && typed(&p[col].0, self.notation).0.is_text());
                match (prev, next) {
                    (_, Some(n)) if texty && (prev.is_none_or(|p| n < p) || !joins_prev) => {
                        ahead.push((col, s.text.clone()));
                        continue;
                    }
                    _ if texty && joins_prev => {
                        let p = grid.last_mut().unwrap();
                        p[col].0.push('\n');
                        p[col].0.push_str(&s.text);
                        *wraps.last_mut().unwrap() = true;
                        col_base[col] = Some(base);
                        continue;
                    }
                    _ => {}
                }
            }
            let mut row = vec![(String::new(), None); cols.len()];
            let had_ahead = !ahead.is_empty();
            for (col, text) in ahead.drain(..) {
                if col < row.len() {
                    if !row[col].0.is_empty() {
                        row[col].0.push('\n');
                    }
                    row[col].0.push_str(&text);
                }
            }
            let mut row_fill = vec![None; cols.len()];
            for (i, s) in cells {
                let col = order[*i];
                if !row[col].0.is_empty() {
                    row[col].0.push(if had_ahead { '\n' } else { ' ' });
                }
                row[col].0.push_str(&s.text);
                row[col].1.get_or_insert(s.style.clone());
                if row_fill[col].is_none() {
                    row_fill[col] = self.fill_at(s, lines[k].baseline);
                }
            }
            // a band behind the whole row (a heading): every cell of it
            let filled: Vec<[u8; 3]> = row_fill.iter().flatten().copied().collect();
            if filled.len() >= 2 && filled.iter().all(|c| *c == filled[0]) {
                row_fill = vec![Some(filled[0]); cols.len()];
            }
            shading.push(row_fill);
            grid.push(row);
            wraps.push(had_ahead);
            col_base = vec![None; cols.len()];
            for (i, _) in cells {
                col_base[order[*i]] = Some(lines[k].baseline);
            }
        }
        // under the columns of the table before (totals under their amounts)
        let sorted_cols: Vec<(f32, f32)> = {
            let mut v = cols.clone();
            v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        let shift = self.align(&sorted_cols, page_top);
        // a currency sign in a column of its own ("Rp" set apart from the amount): part of
        // the amount
        for row in &mut grid {
            for c in 0..row.len().saturating_sub(1) {
                let sign = matches!(row[c].0.trim(), "Rp" | "Rp." | "IDR" | "$" | "USD" | "€" | "£");
                if sign && matches!(typed(&row[c + 1].0, self.notation).0, Value::Number(_)) {
                    let amount = format!("{} {}", row[c].0.trim(), row[c + 1].0);
                    row[c + 1].0 = amount;
                    row[c].0.clear();
                }
            }
        }
        let heading: Vec<String> = grid.first().map(|r| r.iter().map(|c| c.0.clone()).collect()).unwrap_or_default();
        let mut skip = 0;
        let continued = self.continues(page_top, &sorted_cols);
        if continued {
            // the same table, continued: its heading again is dropped
            if self.last.as_ref().is_some_and(|l| l.heading == heading) {
                skip = 1;
            }
        } else if page_top {
            self.gap();
        } else if let Some(first) = lines.first() {
            self.space_before(first.baseline - first.size, first.size);
        }
        let aligned = shift.iter().enumerate().any(|(i, c)| *c != i as u32);
        let header_row = !aligned && !continued && heading_like(&grid);
        for (k, row) in grid.iter().enumerate().skip(skip) {
            for (c, (text, style)) in row.iter().enumerate() {
                let fill = shading.get(k).and_then(|r| r.get(c)).copied().flatten();
                let c = shift.get(c).copied().unwrap_or(c as u32) as usize;
                let mut look = style.as_ref().map(look_of).unwrap_or_default();
                look.fill = look.fill.or(fill);
                if wraps[k] {
                    // the row's cells line up at the top; only cells of several lines wrap
                    look.wrap = text.contains('\n');
                    look.v = Some("top");
                }
                if k == 0 && header_row {
                    look.bold = true;
                    look.borders[3] = Some((Edge::Thin, [0, 0, 0]));
                }
                self.put(self.row, c as u32, text, look);
            }
            if wraps[k] {
                let lines = row.iter().map(|c| c.0.lines().count()).max().unwrap_or(1);
                self.sheet.row_heights.insert(self.row, ROW * lines as f32);
            }
            if k == 0 && header_row && self.first_table && self.row < 30 && grid.len() >= 8 {
                // (heading rows stay on screen in long tables)
                self.sheet.freeze_rows = self.row + 1;
            }
            self.row += 1;
        }
        for (c, (l, r)) in cols.iter().enumerate() {
            let at = shift.get(order[c]).copied().unwrap_or(order[c] as u32);
            self.width(at, r - l);
        }
        self.first_table = false;
        if let Some(l) = lines.last() {
            self.bottom = Some(l.baseline + l.size * 0.3);
        }
        if !continued && !aligned {
            let spans = sorted_cols.iter().zip(&shift).map(|((l, r), at)| (*l, *r, *at)).collect();
            self.last = Some(Placed { heading, spans });
        }
    }

    /// At the top of a page, the table the last page ended with carries on: the same
    /// columns, by their left or right edges (numbers line up on the right)
    fn continues(&self, page_top: bool, cols: &[(f32, f32)]) -> bool {
        page_top
            && self.last.as_ref().is_some_and(|l| {
                l.spans.len() == cols.len()
                    && l.spans.iter().zip(cols).all(|(a, b)| (a.0 - b.0).abs() < 12.0 || (a.1 - b.1).abs() < 12.0 || a.1.min(b.1) - a.0.max(b.0) > 0.5 * (a.1 - a.0).min(b.1 - b.0))
            })
    }

    /// A table drawn with lines
    fn ruled_table(&mut self, t: &from_pdf::Table, page_top: bool) {
        let ncols = t.xs.len().saturating_sub(1);
        let nrows = t.ys.len().saturating_sub(1);
        if ncols == 0 || nrows == 0 {
            return;
        }
        let text_of = |cell: &from_pdf::Cell| -> (String, Option<Style>, Option<&'static str>) {
            let mut lines = vec![];
            collect_lines(&cell.blocks, &mut lines);
            let style = lines.iter().find_map(|l| l.runs.iter().find(|(t, _)| !t.trim().is_empty()).map(|(_, s)| s.clone()));
            let text = lines.iter().map(|l| l.text().replace('\t', " ").trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
            // alignment from where the text sits in the cell
            let align = lines.first().and_then(|l| {
                let (room_l, room_r) = (cell.rect.l + t.padding, cell.rect.r - t.padding);
                let (gap_l, gap_r) = (l.left - room_l, room_r - l.right);
                if (gap_l - gap_r).abs() < 3.0 && gap_l > 4.0 {
                    Some("center")
                } else if gap_r < 3.0 && gap_l > 6.0 {
                    Some("right")
                } else {
                    None
                }
            });
            (text, style, align)
        };
        let heading: Vec<String> = (0..ncols).map(|c| t.cells.iter().find(|cell| cell.row == 0 && cell.col == c).map(|cell| text_of(cell).0).unwrap_or_default()).collect();
        let mut skip_rows = 0;
        let spans_now: Vec<(f32, f32)> = (0..ncols).map(|c| (t.xs[c], t.xs[c + 1])).collect();
        let continued = self.continues(page_top, &spans_now);
        if continued {
            if self.last.as_ref().is_some_and(|l| l.heading == heading) {
                skip_rows = 1;
            }
        } else if page_top {
            self.gap();
        } else {
            self.space_before(t.ys[0], 11.0);
        }
        let shift = self.align(&(0..ncols).map(|c| (t.xs[c], t.xs[c + 1])).collect::<Vec<_>>(), page_top);
        let aligned = shift.iter().enumerate().any(|(i, c)| *c != i as u32);
        let col_at = |c: usize| shift.get(c).copied().unwrap_or(c as u32);
        let base = self.row;
        let edge = |b: Option<from_pdf::Border>| b.map(|b| (if b.width >= 2.0 { Edge::Thick } else if b.width >= 1.2 { Edge::Medium } else { Edge::Thin }, b.color));
        for cell in &t.cells {
            if cell.row < skip_rows {
                continue;
            }
            let r = base + (cell.row - skip_rows) as u32;
            let c = col_at(cell.col);
            let (text, style, align) = text_of(cell);
            let mut look = style.as_ref().map(look_of).unwrap_or_default();
            look.fill = cell.fill;
            look.h = align;
            look.v = Some(match cell.valign {
                from_pdf::VAlign::Top => "top",
                from_pdf::VAlign::Center => "center",
                from_pdf::VAlign::Bottom => "bottom",
            })
            .filter(|v| *v != "bottom");
            look.wrap = text.contains('\n');
            let [top, right, bottom, left] = cell.borders;
            look.borders = [edge(left), edge(right), edge(top), edge(bottom)];
            self.put(r, c, &text, look.clone());
            if cell.rows > 1 || cell.cols > 1 {
                let (r2, c2) = (r + cell.rows as u32 - 1, col_at(cell.col + cell.cols - 1));
                self.sheet.merges.push((r, c, r2, c2));
                // the merged cells carry the borders too
                for rr in r..=r2 {
                    for cc in c..=c2 {
                        if (rr, cc) != (r, c) {
                            let mut l = Look { borders: look.borders, fill: look.fill, ..Default::default() };
                            if rr != r2 {
                                l.borders[3] = None;
                            }
                            self.sheet.cells.entry((rr, cc)).or_insert(w::Cell { value: Value::Empty, look: l });
                        }
                    }
                }
            } else if !aligned {
                self.width(c, cell.rect.w());
            }
        }
        for k in skip_rows..nrows {
            let h = t.ys[k + 1] - t.ys[k];
            if h > ROW * 1.3 {
                self.sheet.row_heights.insert(base + (k - skip_rows) as u32, h.min(409.0));
            }
        }
        self.row = base + (nrows - skip_rows) as u32;
        self.bottom = t.ys.last().copied();
        self.first_table = false;
        if !continued && !aligned {
            let spans = (0..ncols).map(|c| (t.xs[c], t.xs[c + 1], c as u32)).collect();
            self.last = Some(Placed { heading, spans });
        }
    }

    /// Sheet columns for columns at `cols` (left, right; left to right): under the table
    /// before when they sit under some of its columns (totals under their amounts),
    /// otherwise from column A
    fn align(&self, cols: &[(f32, f32)], page_top: bool) -> Vec<u32> {
        let plain = || (0..cols.len() as u32).collect();
        let Some(last) = self.last.as_ref().filter(|last| cols.len() <= last.spans.len() && !page_top) else { return plain() };
        let mut used = 0usize;
        let mut out = vec![];
        let mut anchored = false;
        for (k, (l, r)) in cols.iter().enumerate() {
            let left_after = cols.len() - k - 1;
            let candidates = &last.spans[used..last.spans.len() - left_after];
            let centre = (l + r) / 2.0;
            let d = |s: &(f32, f32, u32)| ((s.0 + s.1) / 2.0 - centre).abs().min((s.1 - r).abs());
            let Some((j, s)) = candidates.iter().enumerate().min_by(|a, b| d(a.1).partial_cmp(&d(b.1)).unwrap_or(std::cmp::Ordering::Equal)) else { return plain() };
            if r.min(s.1) - l.max(s.0) > 0.0 || (r - s.1).abs() < 6.0 {
                anchored = true;
            }
            used += j + 1;
            out.push(s.2);
        }
        if anchored { out } else { plain() }
    }

    /// A blank row between things, except at the very top
    fn gap(&mut self) {
        if self.row > 0 && self.sheet.cells.range((self.row - 1, 0)..).next().is_some() {
            self.row += 1;
        }
    }

    /// Before something starting at `top` (points down the page): a blank row where the
    /// PDF leaves a blank line or more
    fn space_before(&mut self, top: f32, size: f32) {
        if let Some(b) = self.bottom {
            if top - b > size.max(9.0) * 1.1 {
                self.gap();
            }
        }
    }
}

impl Value {
    fn is_text(&self) -> bool {
        matches!(self, Value::Text(_))
    }
}

fn collect_lines<'a>(blocks: &'a [Block], out: &mut Vec<&'a Line>) {
    for b in blocks {
        match &b.content {
            Content::Paragraph(lines) => out.extend(lines.iter()),
            Content::Table(t) => {
                for c in &t.cells {
                    collect_lines(&c.blocks, out);
                }
            }
            Content::Images(_) => {}
        }
    }
}


/// A first row of only text over rows that have numbers: the heading
fn heading_like(grid: &[Vec<(String, Option<Style>)>]) -> bool {
    let Some(first) = grid.first() else { return false };
    let filled: Vec<&String> = first.iter().map(|c| &c.0).filter(|t| !t.is_empty()).collect();
    if filled.len() < 2 || grid.len() < 3 {
        return false;
    }
    let texty = filled.iter().all(|t| t.chars().any(|c| c.is_alphabetic()) && !t.chars().any(|c| c.is_ascii_digit()));
    let numbers_below = grid[1..].iter().flat_map(|r| r.iter()).filter(|c| c.0.chars().any(|ch| ch.is_ascii_digit())).count() > 0;
    texty && numbers_below
}

/// Lines that are part of a table drawn without lines: wide gaps in them
fn tabular(l: &Line) -> bool {
    l.tabs.iter().any(|t| !t.leader) && segments(l).len() >= 2
}

fn page_into(b: &mut Builder, page: &PageModel) {
    // shading behind text: coloured, and smaller than the page (not its background)
    let page_area = page.width * page.height;
    b.fills = page.fills.iter().filter(|(r, c)| r.w() * r.h() < page_area * 0.5 && r.h() > 4.0 && c.iter().any(|v| *v < 245)).cloned().collect();
    let mut list = vec![];
    items(&page.body, &mut list);
    // whether a line starts a table: tabular, or a wrapped cell's first line right above
    // a tabular one
    let starts_table = |i: usize| -> bool {
        match &list[i] {
            Item::Line(l) if tabular(l) => true,
            Item::Line(l) => matches!(list.get(i + 1), Some(Item::Line(n)) if tabular(n) && n.baseline - l.baseline < l.size.max(n.size) * 2.0 && segments(l).len() == 1),
            _ => false,
        }
    };
    // a new page: a blank row, unless a table carries on (decided by the table itself)
    if !matches!(list.first(), Some(Item::Table(_))) && !(!list.is_empty() && starts_table(0)) {
        b.gap();
    }
    let mut i = 0;
    let mut first = true;
    while i < list.len() {
        let page_top = first;
        first = false;
        match &list[i] {
            Item::Table(t) => {
                b.ruled_table(t, page_top);
                i += 1;
            }
            Item::Line(l) if starts_table(i) => {
                // the run of lines belonging to one table: tabular lines, and single
                // lines between them (wrapped descriptions)
                let mut run = vec![l.clone()];
                let mut j = i + 1;
                let mut last_tab = 0;
                // the edges of the parts seen so far: a row whose parts line up with none of
                // them starts another table
                // (the first row may be a heading, set differently from the rows under it:
                // the rows from the second on are compared)
                let (mut seen_l, mut seen_r): (Vec<f32>, Vec<f32>) = (vec![], vec![]);
                let mut tabular_seen = usize::from(tabular(l));
                while j < list.len() {
                    let Item::Line(next) = &list[j] else { break };
                    let prev = run.last().unwrap();
                    let close = next.baseline - prev.baseline < prev.size.max(next.size) * 3.2 && next.baseline > prev.baseline - 1.0;
                    if !close {
                        break;
                    }
                    if tabular(next) {
                        let parts = segments(next);
                        tabular_seen += 1;
                        if tabular_seen > 2 && !seen_l.is_empty() {
                            let fits = parts.iter().filter(|s| seen_l.iter().any(|x| (x - s.left).abs() < 4.0) || seen_r.iter().any(|x| (x - s.right).abs() < 4.0)).count();
                            if fits * 2 < parts.len() + 1 {
                                break;
                            }
                        }
                        for s in parts {
                            seen_l.push(s.left);
                            seen_r.push(s.right);
                        }
                    }
                    run.push(next.clone());
                    if tabular(next) {
                        last_tab = run.len() - 1;
                    }
                    j += 1;
                }
                // single lines after the last tabular one are not part of it
                run.truncate(last_tab.max(usize::from(!tabular(l))) + 1);
                let tab_lines = run.iter().filter(|l| tabular(l)).count();
                if tab_lines >= 2 {
                    b.stream_table(&run, page_top);
                    i += run.len();
                } else {
                    b.text_line(l);
                    i += 1;
                }
            }
            Item::Line(l) => {
                b.text_line(l);
                i += 1;
            }
        }
    }
}

/// PDF pages laid out on a sheet named `name`
fn build(pages: &[PageModel], name: &str) -> w::Sheet {
    let mut texts = vec![];
    for p in pages {
        let mut list = vec![];
        items(&p.body, &mut list);
        for it in list {
            match it {
                Item::Line(l) => texts.push(l.text()),
                Item::Table(t) => {
                    let mut lines = vec![];
                    for c in &t.cells {
                        collect_lines(&c.blocks, &mut lines);
                    }
                    texts.extend(lines.iter().map(|l| l.text()));
                }
            }
        }
    }
    let mut b = Builder { sheet: w::Sheet { name: name.to_string(), ..Default::default() }, row: 0, notation: notation(&texts), last: None, widths: BTreeMap::new(), first_table: true, bottom: None, fills: vec![] };
    for page in pages {
        b.bottom = None;
        page_into(&mut b, page);
    }
    // column widths from the PDF's own (points -> Excel's characters of Calibri 11)
    for (c, pt) in &b.widths {
        let chars = ((pt + 6.0) * 96.0 / 72.0 - 5.0) / 7.0;
        b.sheet.col_widths.insert(*c, chars.clamp(4.0, 80.0));
    }
    // a column of long text without a measured width
    let used: Vec<u32> = b.sheet.cells.keys().map(|k| k.1).collect();
    for c in used {
        b.sheet.col_widths.entry(c).or_insert(10.0);
    }
    b.sheet
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notations() {
        let id = vec!["Saldo 1.234.567,89".to_string(), "Total 2.500.000".into()];
        assert_eq!(notation(&id), Notation::Indonesian);
        let en = vec!["Balance 1,234,567.89".to_string(), "12.50".into()];
        assert_eq!(notation(&en), Notation::English);
    }

    #[test]
    fn amounts_become_numbers() {
        let id = Notation::Indonesian;
        assert_eq!(typed("1.234.567,89", id), (Value::Number(1234567.89), Some("#,##0.00".into())));
        assert_eq!(typed("Rp 25.000", id), (Value::Number(25000.0), Some("\"Rp\"#,##0".into())));
        assert_eq!(typed("Rp. 1.500.000,50", id), (Value::Number(1500000.5), Some("\"Rp\"#,##0.00".into())));
        assert_eq!(typed("(2.500)", id), (Value::Number(-2500.0), Some("#,##0;(#,##0)".into())));
        assert_eq!(typed("12,5%", id), (Value::Number(0.125), Some("0.0%".into())));
        assert_eq!(typed("42", id), (Value::Number(42.0), None));
        assert_eq!(typed("+Rp 9.537.000", id), (Value::Number(9537000.0), Some("\"Rp\"#,##0".into())));
        assert!(matches!(typed("+62 21 4586 1234", id).0, Value::Text(_)));
        let en = Notation::English;
        assert_eq!(typed("$1,234.50", en), (Value::Number(1234.5), Some("$#,##0.00".into())));
        assert_eq!(typed("-12.75", en), (Value::Number(-12.75), Some("0.00".into())));
    }

    #[test]
    fn codes_stay_text() {
        let id = Notation::Indonesian;
        for code in ["0012", "3201234567890001", "081234567890", "1.2.3", "QO-1007", "12,345,678"] {
            assert!(matches!(typed(code, id).0, Value::Text(_)), "{code}");
        }
    }

    #[test]
    fn dates_become_dates() {
        let id = Notation::Indonesian;
        let d = |t: &str| typed(t, id);
        assert_eq!(d("15/11/2026"), (Value::Number(46341.0), Some("dd/mm/yyyy".into())));
        assert_eq!(d("2026-11-15"), (Value::Number(46341.0), Some("yyyy-mm-dd".into())));
        assert_eq!(d("15 November 2026"), (Value::Number(46341.0), Some("d mmmm yyyy".into())));
        assert_eq!(d("15 Desember 2026").1, Some("[$-421]d mmmm yyyy".into()));
        assert_eq!(d("15-Nov-26"), (Value::Number(46341.0), Some("d-mmm-yy".into())));
        let (v, f) = d("15/11/2026 08:30");
        assert_eq!(f, Some("dd/mm/yyyy hh:mm".into()));
        assert!(matches!(v, Value::Number(n) if (n - 46341.354166).abs() < 1e-4));
        // the month can only be the second number
        assert_eq!(typed("11/15/2026", Notation::English).1, Some("mm/dd/yyyy".into()));
    }
}
