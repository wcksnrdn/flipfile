//! Laying the pages out: lines split at their wide gaps into segments and columns,
//! and blocks into the items placed on the sheet

use super::*;

/// A piece of a line between its wide gaps (a column's text)
#[derive(Clone, Debug)]
pub(super) struct Segment {
    pub(super) text: String,
    pub(super) style: Style,
    pub(super) left: f32,
    pub(super) right: f32,
}

/// Gaps of at least this many font sizes always part columns; narrower ones (down to the
/// gap the page is read with) only inside a table whose rows all have them
const STRONG_GAP: f32 = 1.0;

/// A line's parts between wide gaps
pub(super) fn segments(line: &Line) -> Vec<Segment> {
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
pub(super) fn split_segments(line: &Line, bounds: &[f32]) -> Vec<Segment> {
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
pub(super) fn column_starts(lines: &[Line]) -> Vec<f32> {
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
pub(super) enum Item<'a> {
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
pub(super) fn rejoin(mut lines: Vec<Line>) -> Vec<Line> {
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

pub(super) fn items<'a>(blocks: &'a [Block], out: &mut Vec<Item<'a>>) {
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
