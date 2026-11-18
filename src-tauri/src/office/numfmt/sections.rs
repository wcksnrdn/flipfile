//! Sections: a format's `;`-separated parts, their colours and conditions

/// Splits a format into its `;`-separated sections (ignoring `;` in quotes, escapes, brackets)
pub(super) fn sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut bracket = false;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        let cur = out.last_mut().unwrap();
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '\\' | '_' | '*' if !quoted => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '[' if !quoted => {
                bracket = true;
                cur.push(c);
            }
            ']' if !quoted => {
                bracket = false;
                cur.push(c);
            }
            ';' if !quoted && !bracket => out.push(String::new()),
            _ => cur.push(c),
        }
    }
    out
}

/// `[Red]`, `[Color10]`, `[$Rp-421]`, `[>100]` taken off the front (and anywhere) of a section
pub(super) struct Modifiers {
    pub(super) color: Option<[u8; 3]>,
    pub(super) condition: Option<(String, f64)>,
    pub(super) body: String,
}

pub(super) fn modifiers(section: &str) -> Modifiers {
    let mut color = None;
    let mut condition = None;
    let mut body = String::new();
    let mut quoted = false;
    let mut chars = section.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                body.push(c);
            }
            '\\' | '_' | '*' if !quoted => {
                body.push(c);
                if let Some(n) = chars.next() {
                    body.push(n);
                }
            }
            '[' if !quoted => {
                let inner: String = chars.by_ref().take_while(|&c| c != ']').collect();
                let lower = inner.to_lowercase();
                if let Some(rgb) = named_color(&lower) {
                    color = Some(rgb);
                } else if let Some(rest) = inner.strip_prefix('$') {
                    // [$Rp-421]: a currency symbol (before the '-'), and a locale
                    let symbol = rest.split('-').next().unwrap_or("");
                    if !symbol.is_empty() {
                        body.push('"');
                        body.push_str(&symbol.replace('"', ""));
                        body.push('"');
                    }
                    // the locale stays (ignored when printing): dates take their language from it
                    if let Some((_, lcid)) = rest.split_once('-') {
                        body.push_str("[$-");
                        body.push_str(lcid);
                        body.push(']');
                    }
                } else if let Some(op_end) = inner.find(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
                    let (op, num) = inner.split_at(op_end);
                    if let (true, Ok(v)) = (matches!(op, "<" | ">" | "=" | "<=" | ">=" | "<>"), num.parse::<f64>()) {
                        condition = Some((op.to_string(), v));
                    } else {
                        // elapsed time ([h], [mm], …) stays in the body
                        body.push('[');
                        body.push_str(&inner);
                        body.push(']');
                    }
                } else {
                    body.push('[');
                    body.push_str(&inner);
                    body.push(']');
                }
            }
            _ => body.push(c),
        }
    }
    Modifiers { color, condition, body }
}

fn named_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "black" => [0, 0, 0],
        "blue" => [0, 0, 255],
        "cyan" => [0, 255, 255],
        "green" => [0, 128, 0],
        "magenta" => [255, 0, 255],
        "red" => [255, 0, 0],
        "white" => [255, 255, 255],
        "yellow" => [255, 255, 0],
        _ => {
            let n: usize = name.strip_prefix("color")?.parse().ok()?;
            return crate::office::xlsx::indexed_color(n.checked_sub(1)? + 8);
        }
    })
}

pub(super) fn holds(cond: &(String, f64), v: f64) -> bool {
    let (op, x) = cond;
    match op.as_str() {
        "<" => v < *x,
        ">" => v > *x,
        "=" => v == *x,
        "<=" => v <= *x,
        ">=" => v >= *x,
        "<>" => v != *x,
        _ => false,
    }
}
