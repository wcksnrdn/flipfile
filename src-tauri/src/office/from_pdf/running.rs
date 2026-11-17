//! Running headers and footers

use super::*;

/// What a block shows, with the page number (and page count) as placeholders, so the
/// same footer on page 3 and page 4 compares equal. `with_position`: also where it is
/// (roughly), to tell apart portrait and landscape versions of a footer.
pub(in crate::office) fn signature(block: &Block, page: usize, total: usize, with_position: bool) -> String {
    let pos = if with_position { format!("@{:.0},{:.0}", block.rect.l / 12.0, block.rect.t / 12.0) } else { String::new() };
    match &block.content {
        Content::Paragraph(lines) => {
            let text: String = lines.iter().map(|l| l.text()).collect::<Vec<_>>().join("\n");
            format!("p{}{pos}", number_placeholders(&text, page, total))
        }
        Content::Images(imgs) => format!("i{}{pos}", imgs.iter().map(|i| format!("{}:{:.0}x{:.0}", i.media, i.rect.w(), i.rect.h())).collect::<Vec<_>>().join(",")),
        Content::Table(t) => {
            let cells: Vec<String> = t.cells.iter().map(|c| c.blocks.iter().map(|b| signature(b, page, total, false)).collect::<Vec<_>>().join("|")).collect();
            format!("t{}x{}:{}{pos}", t.xs.len(), t.ys.len(), cells.join(";"))
        }
    }
}

/// Splits text into pieces: plain text, or a number that equals the page number / count
#[derive(Debug, PartialEq)]
pub(in crate::office) enum Piece<'a> {
    Text(&'a str),
    Page,
    Pages,
}

pub(in crate::office) fn split_numbers(text: &str, page: usize, total: usize) -> Vec<Piece<'_>> {
    let mut out = vec![];
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let s = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Only standalone numbers ("3", "3/12", "Page 3"), not parts of codes ("QO-1007")
            let before = text[..s].chars().next_back();
            let after = text[i..].chars().next();
            let standalone = !before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_') && !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_');
            let value: usize = text[s..i].parse().unwrap_or(usize::MAX);
            let piece = if standalone && value == page { Some(Piece::Page) } else if standalone && value == total && total > 1 { Some(Piece::Pages) } else { None };
            if let Some(p) = piece {
                if start < s {
                    out.push(Piece::Text(&text[start..s]));
                }
                out.push(p);
                start = i;
            }
        } else {
            i += 1;
        }
    }
    if start < text.len() {
        out.push(Piece::Text(&text[start..]));
    }
    out
}

fn number_placeholders(text: &str, page: usize, total: usize) -> String {
    split_numbers(text, page, total)
        .into_iter()
        .map(|p| match p {
            Piece::Text(t) => t.to_string(),
            Piece::Page => "{PAGE}".into(),
            Piece::Pages => "{PAGES}".into(),
        })
        .collect()
}

/// The printed page numbers: the PDF's page index plus the offset that most pages'
/// top/bottom numbers agree on (0 when there is none)
pub(in crate::office) fn number_pages(pages: &mut [PageModel]) {
    let total = pages.len();
    let mut votes: HashMap<i64, usize> = HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        let mut seen = std::collections::HashSet::new();
        for b in p.body.iter().filter(|b| b.rect.b < p.height * 0.2 || b.rect.t > p.height * 0.82) {
            let Content::Paragraph(lines) = &b.content else { continue };
            for line in lines {
                let text = line.text();
                for token in text.split(|c: char| !c.is_ascii_digit()).filter(|t| !t.is_empty() && t.len() <= 5) {
                    if let Ok(n) = token.parse::<i64>() {
                        let offset = n - (i as i64 + 1);
                        if seen.insert(offset) {
                            *votes.entry(offset).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    let needed = (total / 2).max(2);
    let offset = votes
        .into_iter()
        .filter(|(o, n)| *n >= needed && *o > -(total as i64))
        .max_by_key(|(o, n)| (*n, *o == 0))
        .map(|(o, _)| o)
        .unwrap_or(0);
    for (i, p) in pages.iter_mut().enumerate() {
        p.number = (i as i64 + 1 + offset).max(0) as usize;
    }
}

/// Moves blocks that repeat near the top/bottom of several pages into the header/footer
pub(in crate::office) fn split_running_content(pages: &mut [PageModel]) {
    let total = pages.len();
    if total < 2 {
        return;
    }
    let band = |p: &PageModel, b: &Block| -> Option<bool> {
        if b.rect.b < p.height * 0.2 {
            Some(true)
        } else if b.rect.t > p.height * 0.82 {
            Some(false)
        } else {
            None
        }
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in pages.iter() {
        let mut seen = std::collections::HashSet::new();
        for b in &p.body {
            if band(p, b).is_some() {
                let sig = signature(b, p.number, total, false);
                if seen.insert(sig.clone()) {
                    *counts.entry(sig).or_default() += 1;
                }
            }
        }
    }
    // Twice is enough: it must also sit in the top/bottom band, and run-of-the-mill text
    // does not repeat word for word there
    let needed = 2;
    for p in pages.iter_mut() {
        let body = std::mem::take(&mut p.body);
        let height = p.height;
        for b in body {
            let running = counts.get(&signature(&b, p.number, total, false)).is_some_and(|n| *n >= needed);
            let is_header = if b.rect.b < height * 0.2 { Some(true) } else if b.rect.t > height * 0.82 { Some(false) } else { None };
            match (running, is_header) {
                (true, Some(true)) => p.header.push(b),
                (true, Some(false)) => p.footer.push(b),
                _ => p.body.push(b),
            }
        }
        // Running content must not sit between body blocks: keep only the header
        // blocks above all body text, and footer blocks below it
        let body_top = p.body.iter().map(|b| b.rect.t).fold(f32::MAX, f32::min);
        let body_bottom = p.body.iter().map(|b| b.rect.b).fold(f32::MIN, f32::max);
        let (header, back): (Vec<Block>, Vec<Block>) = std::mem::take(&mut p.header).into_iter().partition(|b| b.rect.b <= body_top + 2.0);
        p.header = header;
        p.body.extend(back);
        let (footer, back): (Vec<Block>, Vec<Block>) = std::mem::take(&mut p.footer).into_iter().partition(|b| b.rect.t >= body_bottom - 2.0);
        p.footer = footer;
        p.body.extend(back);
        p.body.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
    }
}
