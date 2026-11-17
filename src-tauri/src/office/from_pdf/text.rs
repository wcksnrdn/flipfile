//! Characters and lines

use super::*;

pub(super) fn glyphs(page: &PdfPage, page_h: f32, underlines: &[Seg]) -> Vec<Glyph> {
    let Ok(text) = page.text() else { return vec![] };
    let mut out = vec![];
    let mut families: HashMap<String, String> = HashMap::new();
    for c in text.chars().iter() {
        let Some(ch) = c.unicode_char() else { continue };
        if ch == '\r' || ch == '\n' || ch == '\u{2}' || ch == '\u{FFFE}' {
            continue;
        }
        let generated = c.is_generated().unwrap_or(false);
        if generated && ch != ' ' {
            continue;
        }
        let Ok(bounds) = c.loose_bounds() else { continue };
        let baseline = page_h - c.origin_y().map(|y| y.value).unwrap_or(bounds.bottom().value);
        let size = c.scaled_font_size().value;
        if generated {
            // Only a hint for the line builder: where PDFium saw a word boundary
            let style = Style { font: String::new(), size: 0.0, bold: false, italic: false, underline: false, color: None, spacing: 0.0, vert: 0 };
            out.push(Glyph { ch, left: bounds.left().value, right: bounds.left().value, baseline, style, generated });
            continue;
        }
        if !(size.is_finite() && size > 0.5) {
            continue;
        }
        let font = c.font_name();
        let lower = font.to_lowercase();
        // A font this computer does not have: the Office font of the same kind, chosen
        // from the PDF's own description of the font (looked up once per font)
        let family = families
            .entry(font.clone())
            .or_insert_with(|| {
                let clean = clean_font_name(&font);
                if crate::office::typeset::has_font(&clean) {
                    clean
                } else if c.font_is_fixed_pitch() || lower.contains("mono") || lower.contains("courier") {
                    "Courier New".into()
                } else if c.font_is_serif() || lower.contains("serif") && !lower.contains("sans") {
                    "Times New Roman".into()
                } else {
                    "Arial".into()
                }
            })
            .clone();
        let bold = c.font_weight().map(|w| matches!(w, PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900)).unwrap_or(false)
            || c.font_is_bold_reenforced()
            || lower.contains("bold")
            || lower.contains("black")
            || lower.contains("heavy");
        let italic = c.font_is_italic() || lower.contains("italic") || lower.contains("oblique");
        let color = c.fill_color().ok().map(|col| [col.red(), col.green(), col.blue()]).filter(|c| c.iter().any(|v| *v > 40));
        let (left, right) = (bounds.left().value, bounds.right().value);
        // A thin rule just under the baseline that spans this character: underlined
        let underline = ch != ' '
            && underlines.iter().any(|s| {
                s.pos > baseline - 0.5 && s.pos < baseline + size * 0.35 && s.a <= left + (right - left) * 0.5 && s.b >= right - (right - left) * 0.5
            });
        out.push(Glyph {
            ch,
            left,
            right,
            baseline,
            style: Style { font: family, size: (size * 2.0).round() / 2.0, bold, italic, underline, color, spacing: 0.0, vert: 0 },
            generated,
        });
    }
    out
}

fn is_dot(c: char) -> bool {
    matches!(c, '.' | '…' | '·' | '‧')
}

/// Characters in PDF order -> lines. A new line starts when the baseline moves by
/// more than half a font size or the text jumps back to the left.
///
/// Within a line, the spacing between letters is measured first: letter-spaced text
/// ("H E A D I N G" style tracking) keeps its spacing as Word character spacing, and
/// only gaps clearly wider than that become spaces; much wider gaps become tabs, and
/// runs of dots become a dot-leader tab.
/// `art`: rectangles of vector art on the page; a gap in a line where art sits is kept
/// as a tab, so the text stays beside the art.
pub(super) fn lines(glyphs: &[&Glyph], art: &[Rect]) -> Vec<Line> {
    // 1. Characters -> raw lines
    let mut raw: Vec<Vec<&Glyph>> = vec![];
    let mut baseline = 0.0f32;
    let mut size = 0.0f32;
    let mut last_right = 0.0f32;
    let mut forced = false;
    for &g in glyphs {
        // A line break marker (text recognition: one recognised line ends here)
        if g.ch == '\n' {
            forced = true;
            continue;
        }
        if g.ch == ' ' {
            if let Some(line) = raw.last_mut()
                && (g.baseline - baseline).abs() <= size.max(1.0) * 0.5 {
                    line.push(g);
                }
            continue;
        }
        let new_line = std::mem::take(&mut forced) || raw.is_empty() || (g.baseline - baseline).abs() > size.max(g.style.size) * 0.5 || g.left < last_right - size * 2.0;
        if new_line {
            raw.push(vec![g]);
            baseline = g.baseline;
            size = g.style.size;
            last_right = g.right;
        } else {
            raw.last_mut().unwrap().push(g);
            size = size.max(g.style.size);
            last_right = last_right.max(g.right);
        }
    }

    // Pieces of one line that the PDF draws out of order (a list number written after
    // its text, a word added later) come back together: same baseline, right next to
    // each other. Columns further apart stay separate lines.
    loop {
        let mut merged = false;
        'search: for i in 0..raw.len() {
            for j in 0..raw.len() {
                if i == j {
                    continue;
                }
                let letters = |line: &Vec<&Glyph>| -> Option<(f32, f32, f32, f32, usize, bool)> {
                    let l: Vec<&&Glyph> = line.iter().filter(|g| g.ch != ' ').collect();
                    let first = l.first()?;
                    let text: String = l.iter().map(|g| g.ch).collect();
                    // A list marker: "3." "a)" "•" (a bare number may be a badge)
                    let marker = starts_list_item(&text) || is_enumerator(&text);
                    Some((
                        l.iter().map(|g| g.left).fold(f32::MAX, f32::min),
                        l.iter().map(|g| g.right).fold(f32::MIN, f32::max),
                        first.baseline,
                        l.iter().map(|g| g.style.size).fold(0.0, f32::max),
                        l.len(),
                        marker,
                    ))
                };
                let (Some(a), Some(b)) = (letters(&raw[i]), letters(&raw[j])) else { continue };
                let size = a.3.max(b.3);
                // j right of i: i's end to j's start. Close as a word gap, or a short
                // piece (list number, bullet, footnote mark) a little further away;
                // anything else may be the next column
                let gap = b.0 - a.1;
                let similar = (a.3 - b.3).abs() <= a.3.min(b.3) * 0.2;
                let marker = a.5 && a.4 <= 3 && gap < a.3.min(b.3) * 1.5;
                let close = similar && (gap < size * 0.6 || marker);
                if (a.2 - b.2).abs() < size * 0.3 && gap > -size * 0.2 && close {
                    let tail = raw.remove(j);
                    let i = if j < i { i - 1 } else { i };
                    raw[i].extend(tail);
                    merged = true;
                    break 'search;
                }
            }
        }
        if !merged {
            break;
        }
    }

    // 2. Each raw line -> text with spaces, tabs and character spacing
    let mut out: Vec<Line> = vec![];
    for glyphs in raw {
        let letters: Vec<&Glyph> = glyphs.iter().copied().filter(|g| g.ch != ' ').collect();
        let Some(first) = letters.first() else { continue };
        // The line's size is that of most of its text: a bigger list number or bullet
        // must not make the line look like a different paragraph (or a taller one)
        let line_size = {
            let mut counts: Vec<(f32, usize)> = vec![];
            for g in &letters {
                match counts.iter_mut().find(|(s, _)| (*s - g.style.size).abs() < 0.01) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((g.style.size, 1)),
                }
            }
            counts.iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))).map(|c| c.0).unwrap_or(first.style.size)
        };
        // Tracking: the typical gap between neighbouring letters (lower quartile, so
        // word gaps do not count)
        let mut gaps: Vec<f32> = letters.windows(2).map(|w| w[1].left - w[0].right).filter(|g| *g > -line_size * 0.1 && *g < line_size * 0.6).collect();
        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let tracking = if gaps.len() >= 4 { gaps[gaps.len() / 4].max(0.0) } else { 0.0 };
        let tracking = if tracking > line_size * 0.06 { tracking } else { 0.0 };

        // The line's baseline: where most of its characters sit (not a raised marker)
        let baseline = {
            let mut b: Vec<f32> = letters.iter().map(|g| g.baseline).collect();
            b.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
            b[b.len() / 2]
        };
        let mut line = Line { runs: vec![], tabs: vec![], left: first.left, right: first.right, baseline, size: line_size, pitch_hint: 0.0, art: false, gaps: vec![] };
        let band = (baseline - line_size * 0.9, baseline + line_size * 0.3);
        let art_in_gap = |from: f32, to: f32| art.iter().any(|r| r.l < to && r.r > from && r.t < band.1 && r.b > band.0 && r.h() < line_size * 4.0);
        let mut last_right = first.left;
        let mut real_space = false;
        let mut hinted_space = false;
        let mut in_leader = false;
        for g in glyphs {
            if g.ch == ' ' {
                if g.generated {
                    hinted_space = true;
                } else {
                    real_space = true;
                }
                continue;
            }
            if in_leader {
                if is_dot(g.ch) {
                    last_right = g.right.max(last_right);
                    continue;
                }
                in_leader = false;
                if let Some(t) = line.tabs.last_mut() {
                    t.left = g.left;
                    t.right = g.right;
                }
                real_space = false;
                hinted_space = false;
            }
            let mut style = g.style.clone();
            style.spacing = tracking;
            // Small text above/below the line: superscript (footnote and citation
            // numbers, powers) or subscript
            let rise = baseline - g.baseline;
            // Only attached to the text before it (x², a citation¹²): not the first thing
            // on the line, nor after a tab (a badge number beside a heading)
            let tab_gap = TAB_GAP_NOW.with(|t| t.get());
            let attached = !line.runs.is_empty() && g.left - last_right - tracking < g.style.size * tab_gap;
            if attached && g.style.size < line_size * 0.85 {
                if rise > line_size * 0.2 {
                    style.vert = 1;
                } else if rise < -line_size * 0.1 {
                    style.vert = -1;
                }
            }
            if !line.runs.is_empty() {
                let gap = g.left - last_right - tracking;
                let ends_with_tab = line.runs.last().is_some_and(|(t, _)| t.ends_with('\t'));
                let around_art = gap > g.style.size * 0.15 && art_in_gap(last_right, g.left);
                if around_art {
                    line.art = true;
                }
                if !ends_with_tab {
                    if gap > g.style.size * tab_gap || around_art {
                        push_char(&mut line, '\t', &style);
                        line.tabs.push(TabMark { left: g.left, right: g.right, leader: false, before: last_right });
                    } else if real_space || gap > g.style.size * SPACE_GAP || (hinted_space && gap > g.style.size * 0.12) {
                        let at: usize = line.runs.iter().map(|(t, _)| t.chars().count()).sum();
                        line.gaps.push((at, last_right, g.left));
                        push_char(&mut line, ' ', &style);
                    }
                }
            }
            real_space = false;
            hinted_space = false;
            push_char(&mut line, g.ch, &style);
            line.right = line.right.max(g.right);
            if let Some(t) = line.tabs.last_mut()
                && (!t.leader || t.left > 0.0) {
                    t.right = t.right.max(g.right);
                }
            last_right = g.right.max(last_right);
            // Four dots in a row: a leader. The dots become one tab.
            if is_dot(g.ch) && trailing_dots(&line) >= 4 {
                remove_trailing_dots(&mut line);
                push_char(&mut line, '\t', &style);
                line.tabs.push(TabMark { left: g.right, right: g.right, leader: true, before: g.right });
                in_leader = true;
            }
        }
        // A line that ends in a tab (leader dots with nothing after): drop the tab
        while line.runs.last().is_some_and(|(t, _)| t.trim_end_matches(' ').ends_with('\t')) {
            let (t, _) = line.runs.last_mut().unwrap();
            let trimmed = t.trim_end_matches(' ').trim_end_matches('\t').to_string();
            *t = trimmed;
            line.tabs.pop();
            if line.runs.last().is_some_and(|(t, _)| t.is_empty()) {
                line.runs.pop();
            }
        }
        if let Some((t, _)) = line.runs.last_mut() {
            let trimmed = t.trim_end().to_string();
            *t = trimmed;
        }
        // Keep `tabs` in step with the tab characters
        let n = line.text().matches('\t').count();
        line.tabs.truncate(n);
        if !line.text().trim().is_empty() {
            out.push(line);
        }
    }
    out
}

fn trailing_dots(line: &Line) -> usize {
    let mut n = 0;
    for (t, _) in line.runs.iter().rev() {
        for c in t.chars().rev() {
            if is_dot(c) {
                n += if c == '…' { 3 } else { 1 };
            } else if c != ' ' {
                return n;
            }
        }
    }
    n
}

fn remove_trailing_dots(line: &mut Line) {
    while let Some((t, _)) = line.runs.last_mut() {
        while t.ends_with(|c: char| is_dot(c) || c == ' ') {
            t.pop();
        }
        if t.is_empty() {
            line.runs.pop();
        } else {
            break;
        }
    }
}

fn push_char(line: &mut Line, ch: char, style: &Style) {
    match line.runs.last_mut() {
        Some((text, s)) if s == style || ch == ' ' || ch == '\t' => text.push(ch),
        _ => line.runs.push((ch.to_string(), style.clone())),
    }
}

/// Lines -> paragraphs, using spacing, indents, short last lines, size changes and bullets.
pub(super) fn paragraphs(lines: Vec<Line>, column_right: f32) -> Vec<Vec<Line>> {
    // Each line's own column: in a two-column page the left column ends long before
    // the page's text does, and its full lines must not look "short"
    let rights: Vec<f32> = lines
        .iter()
        .map(|l| {
            // Only where another column runs beside it
            let beside = lines.iter().any(|o| o.left > l.right + 8.0 && (o.baseline - l.baseline).abs() < 150.0);
            if !beside {
                return column_right;
            }
            let near = lines.iter().filter(|o| (o.left - l.left).abs() < 40.0 && (o.baseline - l.baseline).abs() < 150.0 && o.tabs.is_empty());
            near.map(|o| o.right).fold(l.right, f32::max).min(column_right)
        })
        .collect();
    // A "tab" in a line of justified text is only a stretched space: the line runs to
    // the column's edge like the lines above/below it, which have no tabs
    let mut lines = lines;
    for i in 0..lines.len() {
        let l = &lines[i];
        if l.tabs.is_empty() || l.tabs.iter().any(|t| t.leader) || (rights[i] - l.right).abs() > l.size {
            continue;
        }
        let like = |o: &Line| o.tabs.is_empty() && (o.left - l.left).abs() < l.size * 1.5 && (o.right - l.right).abs() < l.size && (o.baseline - l.baseline).abs() < l.size * 2.5;
        let justified = (i > 0 && like(&lines[i - 1])) || lines.get(i + 1).is_some_and(like);
        if justified {
            if cfg!(test) && std::env::var("FLIPFILE_BLOCKS").is_ok() {
                eprintln!("justify-tab: {}", lines[i].text());
            }
            let line = &mut lines[i];
            for (text, _) in line.runs.iter_mut() {
                *text = text.replace('\t', " ");
            }
            line.tabs.clear();
        }
    }
    let mut out: Vec<Vec<Line>> = vec![];
    let mut prev_right_edge = column_right;
    for (index, line) in lines.into_iter().enumerate() {
        let column_right = std::mem::replace(&mut prev_right_edge, rights[index]);
        let Some(para) = out.last_mut() else {
            out.push(vec![line]);
            continue;
        };
        let prev = para.last().unwrap();
        let line_height = prev.size.max(line.size) * 1.2;
        let gap = line.baseline - prev.baseline;
        // Typical spacing inside this paragraph so far
        let typical = if para.len() >= 2 {
            para.windows(2).map(|w| w[1].baseline - w[0].baseline).fold(0.0, f32::max)
        } else {
            line_height * 1.25
        };
        let size_change = (prev.size - line.size).abs() > prev.size.min(line.size) * 0.15;
        let style_change = prev.runs.last().map(|(_, s)| s.bold) != line.runs.first().map(|(_, s)| s.bold) && para.len() == 1 && prev.size >= line.size;
        // A short line ends a paragraph only if the next line's first word would have
        // fitted after it; otherwise the line simply wrapped early before a long word.
        let room = column_right - prev.right;
        let next_text = line.text();
        let first_word = next_text.split_whitespace().next().unwrap_or("");
        let char_width = (line.right - line.left) / next_text.chars().count().max(1) as f32;
        let word_width = (first_word.chars().count() as f32 + 1.0) * char_width;
        let prev_short = room > prev.size * 1.5 && word_width < room;
        // Wrapped lines of a list item line up with the text after the marker
        let text_left = if para.len() == 1 && prev.marker_first() { prev.tabs.first().map(|t| t.left).unwrap_or(prev.left) } else { prev.left };
        let indented = para.len() > 1 && line.left > prev.left + prev.size * 1.2 && !prev_short;
        let misaligned = para.len() == 1 && prev.marker_first() && (line.left - text_left).abs() > prev.size * 0.8;
        let starts = (gap > typical * 1.3 + 0.5)
            || gap < 0.0
            || size_change
            || style_change
            || prev_short
            || prev.tabular()
            || line.tabular()
            || line.marker_first()
            || starts_list_item(&line.text())
            || indented
            || misaligned;
        if starts {
            out.push(vec![line]);
        } else {
            para.push(line);
        }
    }
    out
}

/// Line box of a paragraph: the pitch Word will use and the rectangle it fills
pub(in crate::office) fn line_pitch(lines: &[Line]) -> f32 {
    if lines.len() >= 2 {
        let pitch = lines[1].baseline - lines[0].baseline;
        if pitch > lines[0].size * 0.8 && pitch < lines[0].size * 3.0 {
            return pitch;
        }
    }
    if lines[0].pitch_hint > 0.0 {
        return lines[0].pitch_hint;
    }
    lines[0].size * 1.2
}

/// How much tighter (points per character) the line's characters must sit for its text,
/// at its fonts' natural widths, to fit the width it has on the PDF page; `None` when
/// a font is not installed (nothing reliable to measure with)
fn squeeze_needed(line: &Line) -> Option<f32> {
    let mut natural = 0.0;
    let mut chars = 0usize;
    for (text, style) in &line.runs {
        if style.spacing > 0.0 {
            return None; // letter-spaced on purpose
        }
        natural += crate::office::typeset::text_width(&style.font, style.size, style.bold, style.italic, text)?;
        chars += text.chars().count();
    }
    let available = line.right - line.left;
    (chars > 0 && natural > available).then(|| (natural - available) / chars as f32 + 0.02)
}

fn paragraph_block(lines: Vec<Line>) -> Block {
    let pitch = line_pitch(&lines);
    let first = &lines[0];
    let last = lines.last().unwrap();
    let rect = Rect {
        l: lines.iter().map(|l| l.left).fold(f32::MAX, f32::min),
        r: lines.iter().map(|l| l.right).fold(f32::MIN, f32::max),
        t: first.baseline - pitch * 0.8,
        b: last.baseline + pitch * 0.2,
    };
    Block { rect, content: Content::Paragraph(lines) }
}

/// Text and pictures inside one area (the page or a table cell) -> blocks in reading order
/// `extra`: blocks built elsewhere (tables) that share the area's layout
pub(super) fn flow(glyphs: &[&Glyph], images: Vec<ImageRef>, column_right: f32, art: &[Rect], extra: Vec<Block>) -> Vec<Block> {
    let lines = lines(glyphs, art);
    let mut paras = paragraphs(lines, column_right);
    // One-line paragraphs: lines in a row (list items, short lines) sit at the text's
    // real pitch, often tighter than 1.2 x the size; use the distance to the next line
    for i in 0..paras.len() {
        if paras[i].len() != 1 {
            continue;
        }
        let (baseline, size) = (paras[i][0].baseline, paras[i][0].size);
        let next = paras.get(i + 1).map(|p| p[0].baseline - baseline);
        let prev = i.checked_sub(1).and_then(|j| paras.get(j)).map(|p| baseline - p.last().unwrap().baseline);
        let hint = [next, prev].into_iter().flatten().filter(|d| *d > size * 0.85 && *d < size * 1.2).fold(f32::MAX, f32::min);
        if hint < f32::MAX {
            paras[i][0].pitch_hint = hint;
        }
    }
    // Lines the PDF squeezed (justified text with spaces narrower than normal): Word
    // cannot squeeze spaces, so the paragraph gets slightly condensed characters,
    // just enough for every line to fit where it did
    for para in paras.iter_mut() {
        let squeeze = para.iter().filter(|l| l.tabs.is_empty()).filter_map(squeeze_needed).fold(0.0f32, f32::max);
        // Up to 8% of an em per character: what a substitute font of the same kind needs
        let size = para[0].size;
        if squeeze > 0.01 && squeeze <= (size * 0.08).max(0.6) {
            for line in para.iter_mut() {
                for (_, style) in line.runs.iter_mut() {
                    style.spacing = style.spacing.min(0.0) - squeeze;
                }
            }
        }
    }
    let mut blocks: Vec<Block> = paras.into_iter().map(paragraph_block).collect();
    // Pictures whose rows overlap stand side by side
    let mut images = images;
    images.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
    let mut rows: Vec<Vec<ImageRef>> = vec![];
    for img in images {
        match rows.last_mut() {
            // Side by side: overlapping rows of the page, and apart from each other
            Some(row)
                if row.iter().any(|o| o.rect.b.min(img.rect.b) - o.rect.t.max(img.rect.t) > img.rect.h().min(o.rect.h()) * 0.3)
                    && row.iter().all(|o| o.rect.r.min(img.rect.r) - o.rect.l.max(img.rect.l) <= 1.0) =>
            {
                row.push(img)
            }
            _ => rows.push(vec![img]),
        }
    }
    for mut row in rows {
        row.sort_by(|a, b| a.rect.l.partial_cmp(&b.rect.l).unwrap_or(std::cmp::Ordering::Equal));
        let rect = row.iter().skip(1).fold(row[0].rect, |r, i| r.union(&i.rect));
        blocks.push(Block { rect, content: Content::Images(row) });
    }
    blocks.extend(extra);
    blocks.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
    side_by_side(blocks, column_right)
}

/// Blocks that share the same height on the page but sit apart horizontally (two
/// columns, a picture beside text, a page number beside a note) are put into the cells
/// of a borderless table, so they stay side by side instead of being stacked.
/// `column_right`: the right edge of the text area; the last column may use all the
/// room up to it, so a line that just fitted in the PDF never wraps in Word.
fn side_by_side(blocks: Vec<Block>, column_right: f32) -> Vec<Block> {
    if cfg!(test) && std::env::var("FLIPFILE_BLOCKS").is_ok() {
        for b in &blocks {
            let text = match &b.content {
                Content::Paragraph(l) => l.first().map(|l| l.text()).unwrap_or_default(),
                Content::Table(_) => "<table>".into(),
                Content::Images(_) => "<images>".into(),
            };
            eprintln!("blk {:6.1} {:6.1} {:6.1} {:6.1} {}", b.rect.l, b.rect.t, b.rect.r, b.rect.b, text.chars().take(50).collect::<String>());
        }
    }
    let mut out = vec![];
    let mut i = 0;
    while i < blocks.len() {
        let mut bottom = blocks[i].rect.b;
        let mut j = i + 1;
        while j < blocks.len() {
            let r = &blocks[j].rect;
            let overlap = bottom.min(r.b) - r.t;
            if overlap > 2.0 && overlap > r.h().min(bottom - blocks[i].rect.t) * 0.3 {
                bottom = bottom.max(r.b);
                j += 1;
            } else {
                break;
            }
        }
        let mut group: Vec<Block> = blocks[i..j].to_vec();
        i = j;
        if group.len() < 2 {
            out.extend(group);
            continue;
        }
        // Columns: blocks whose horizontal extents overlap belong together
        group.sort_by(|a, b| a.rect.l.partial_cmp(&b.rect.l).unwrap_or(std::cmp::Ordering::Equal));
        let mut columns: Vec<(f32, f32, Vec<Block>)> = vec![];
        for b in group {
            match columns.last_mut() {
                Some((_, r, col)) if b.rect.l < *r - 1.0 => {
                    *r = r.max(b.rect.r);
                    col.push(b);
                }
                _ => columns.push((b.rect.l, b.rect.r, vec![b])),
            }
        }
        if columns.len() < 2 {
            let mut col = columns.pop().map(|c| c.2).unwrap_or_default();
            col.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
            out.extend(col);
            continue;
        }
        // The columns go on as long as what follows stays on one side of every gutter
        // (the gaps between the columns): a two-column page becomes one table with a
        // whole column in each cell, instead of many small ones
        let gutters: Vec<(f32, f32)> = columns.windows(2).map(|w| (w[0].1, w[1].0)).filter(|(a, b)| b - a >= 6.0).collect();
        // Only text columns (2–3 of similar width), not a row of cards or a label
        // beside a paragraph
        let widths: Vec<f32> = columns.iter().map(|c| c.1 - c.0).collect();
        let span = columns.last().unwrap().1 - columns[0].0;
        let narrowest = widths.iter().copied().fold(f32::MAX, f32::min);
        let widest = widths.iter().copied().fold(0.0, f32::max);
        let text_columns = (2..=3).contains(&columns.len()) && narrowest >= span * 0.28 && widest <= narrowest * 1.6;
        if text_columns && gutters.len() == columns.len() - 1 {
            let mut band_bottom = columns.iter().flat_map(|c| c.2.iter()).map(|b| b.rect.b).fold(f32::MIN, f32::max);
            while i < blocks.len() {
                let b = &blocks[i].rect;
                // Into the gutter by more than a third of it (lines of a column end a
                // little unevenly, more so in recognised scans)
                let crosses = gutters.iter().any(|&(lo, hi)| {
                    let slack = ((hi - lo) / 3.0).max(1.0);
                    b.r > lo + slack && b.l < hi - slack
                });
                // Far below the band (a new part of the page): stop
                if crosses || b.t > band_bottom + 40.0 {
                    break;
                }
                let k = gutters.iter().filter(|&&(lo, hi)| b.l >= (lo + hi) / 2.0).count();
                // Well below what its column holds so far (a new section starts), or
                // side by side with the next block inside one column: not running text
                let column_bottom = columns[k].2.iter().map(|c| c.rect.b).fold(f32::MIN, f32::max);
                let beside_next = blocks.get(i + 1).is_some_and(|n| {
                    let n = &n.rect;
                    let same_column = gutters.iter().filter(|&&(lo, hi)| n.l >= (lo + hi) / 2.0).count() == k;
                    same_column && b.b.min(n.b) - b.t.max(n.t) > 2.0 && (n.l >= b.r - 1.0 || n.r <= b.l + 1.0)
                });
                if b.t - column_bottom > 25.0 || beside_next {
                    break;
                }
                let col = &mut columns[k];
                col.0 = col.0.min(b.l);
                col.1 = col.1.max(b.r);
                band_bottom = band_bottom.max(b.b);
                col.2.push(blocks[i].clone());
                i += 1;
            }
        }
        let top = columns.iter().flat_map(|c| c.2.iter()).map(|b| b.rect.t).fold(f32::MAX, f32::min);
        let bottom = columns.iter().flat_map(|c| c.2.iter()).map(|b| b.rect.b).fold(f32::MIN, f32::max);
        // Each cell starts where its column's text starts and runs up to the next
        // column (all the gap as room against wrapping); it also remembers where its
        // text ends, for judging alignment
        let mut xs = vec![columns[0].0];
        for w in columns.windows(2) {
            xs.push(w[1].0);
        }
        let text_rights: Vec<f32> = columns.iter().map(|c| c.1).collect();
        xs.push(columns.last().unwrap().1.max(column_right));
        let cells = columns
            .into_iter()
            .enumerate()
            .map(|(k, (_, _, mut col))| {
                col.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
                Cell {
                    row: 0,
                    col: k,
                    rows: 1,
                    cols: 1,
                    rect: Rect { l: xs[k], r: xs[k + 1], t: top, b: bottom },
                    borders: [None; 4],
                    fill: None,
                    valign: VAlign::Top,
                    blocks: col,
                    text_right: Some(text_rights[k]),
                }
            })
            .collect();
        let rect = Rect { l: xs[0], r: *xs.last().unwrap(), t: top, b: bottom };
        out.push(Block { rect, content: Content::Table(Table { xs, ys: vec![top, bottom], cells, padding: 0.0 }) });
    }
    out
}
