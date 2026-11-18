//! Text sections, and the literal text every kind of section may hold

use super::*;

pub(super) fn format_text(text: &str, code: &str) -> Shown {
    let secs = sections(code);
    // the text section is the 4th, or a lone section that has an '@'
    let sec = if secs.len() >= 4 {
        Some(&secs[3])
    } else {
        secs.iter().find(|s| s.contains('@'))
    };
    let Some(sec) = sec else { return Shown::plain(text.to_string()) };
    let m = modifiers(sec);
    let mut out = render_literals(&m.body, &|_| Some(text.to_string()));
    out.color = m.color;
    out
}

/// Renders a section that only has literals, padding, fill and (via `at`) the '@' text
pub(super) fn render_literals(body: &str, at: &dyn Fn(char) -> Option<String>) -> Shown {
    let mut out = Shown::default();
    let mut cur = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for q in chars.by_ref() {
                    if q == '"' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '_' => {
                if let Some(n) = chars.next() {
                    cur.push(HIDE_ON);
                    cur.push(n);
                    cur.push(HIDE_OFF);
                }
            }
            '*' => {
                if let Some(n) = chars.next()
                    && out.fill.is_none() {
                        out.fill = Some(n);
                        out.text = std::mem::take(&mut cur);
                    }
            }
            '@' => {
                if let Some(t) = at(c) {
                    cur.push_str(&t);
                }
            }
            '[' => {
                for q in chars.by_ref() {
                    if q == ']' {
                        break;
                    }
                }
            }
            _ => cur.push(c),
        }
    }
    if out.fill.is_some() {
        out.after_fill = Some(cur);
    } else {
        out.text = cur;
    }
    out
}
