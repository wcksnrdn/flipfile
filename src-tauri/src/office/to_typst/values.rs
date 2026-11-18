//! Typst values: escaped strings, points, colours, strokes, font lists and images

use super::*;

/// Escaped Typst string literal.
pub(crate) fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn pt(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{:.2}pt", v)
}

pub(super) fn rgb(c: [u8; 3]) -> String {
    format!("rgb(\"#{:02x}{:02x}{:02x}\")", c[0], c[1], c[2])
}

pub(super) fn stroke(b: Option<Border>) -> String {
    match b {
        Some(b) => format!("{} + {}", pt(b.width), rgb(b.color)),
        None => "none".into(),
    }
}

/// The document font first, then look-alikes that are commonly installed, so text
/// keeps a similar look and width when the original font is missing.
pub fn font_list(name: &str) -> String {
    let quoted: Vec<String> = font_names(name).iter().map(|n| q(n)).collect();
    format!("({})", quoted.join(", "))
}

pub(super) fn font_names(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let fallbacks: &[&str] = if ["courier", "consolas", "mono", "menlo", "code"].iter().any(|k| lower.contains(k)) {
        &["Consolas", "Menlo", "Courier New", "Liberation Mono", "DejaVu Sans Mono"]
    } else if ["times", "cambria", "georgia", "garamond", "book", "serif", "palatino", "minion", "constantia", "baskerville"]
        .iter()
        .any(|k| lower.contains(k))
        && !lower.contains("sans")
    {
        &["Cambria", "Times New Roman", "Times", "Liberation Serif", "DejaVu Serif", "Libertinus Serif"]
    } else {
        &["Aptos", "Calibri", "Carlito", "Helvetica Neue", "Arial", "Segoe UI", "Liberation Sans", "DejaVu Sans"]
    };
    let mut names = vec![name.to_string()];
    for f in fallbacks {
        if !f.eq_ignore_ascii_case(name) {
            names.push(f.to_string());
        }
    }
    names
}

pub(super) fn allow_break_after_hyphen(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    let mut previous = ' ';
    while let Some(c) = chars.next() {
        out.push(c);
        // Only inside a word: a leading minus ("-5") stays attached to its number
        if c == '-' && previous.is_alphanumeric() && chars.peek().is_some_and(|n| n.is_ascii_digit()) {
            out.push('\u{200B}');
        }
        previous = c;
    }
    out
}

pub(super) fn is_symbol_font(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("symbol") || n.contains("wingdings") || n.contains("webdings")
}

/// Typst reads PNG, JPEG, GIF, SVG and WebP; BMP/TIFF are converted to PNG here.
/// Vector formats Word sometimes uses (EMF/WMF) cannot be drawn and are left out.
pub(super) fn prepare_image(index: usize, image: &crate::office::docx::Image) -> Option<(String, Vec<u8>)> {
    match image.ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => Some((format!("/img/{index}.{}", image.ext), image.data.clone())),
        _ => {
            let img = image::load_from_memory(&image.data).ok()?;
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some((format!("/img/{index}.png"), png))
        }
    }
}
