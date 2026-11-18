//! Pages

use super::*;

/// Whether anything prints on a page: a cell with a value, fill or border, or a picture,
/// chart or shape starting on it
pub(super) fn has_content(sheet: &Sheet, styles: &[Style], page: &Page) -> bool {
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

pub(super) struct Page {
    pub(super) rows: (u32, u32),
    pub(super) cols: (u32, u32),
}

/// Splits `[a, b]` into bands no longer than `room`, also breaking before `breaks`
pub(super) fn bands(a: u32, b: u32, size: &dyn Fn(u32) -> f32, room: f32, breaks: &[u32]) -> Vec<(u32, u32)> {
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
pub(super) fn header_parts(spec: &str, page: usize, pages: usize, file: &str, sheet: &str) -> [String; 3] {
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
