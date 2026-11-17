//! Fonts and text

use super::*;

/// "ABCDEF+TimesNewRomanPS-BoldMT" -> "Times New Roman"
pub(super) fn clean_font_name(raw: &str) -> String {
    let name = raw.split_once('+').map(|(_, n)| n).unwrap_or(raw);
    let base = name.split(['-', ',']).next().unwrap_or(name);
    let base = base.trim_end_matches("PSMT").trim_end_matches("MT").trim_end_matches("PS");
    // Open fonts made to the exact metrics of the Office ones: the Office font keeps
    // every line as wide as in the PDF, on any computer with Word
    let mapped = match base {
        "Helvetica" | "ArialMT" | "LiberationSans" | "Arimo" | "NimbusSans" | "NimbusSanL" => "Arial",
        "Times" | "TimesRoman" | "Times-Roman" | "LiberationSerif" | "Tinos" | "NimbusRoman" | "NimbusRomNo9L" => "Times New Roman",
        "Courier" | "LiberationMono" | "Cousine" | "NimbusMono" | "NimbusMonoPS" => "Courier New",
        "Carlito" => "Calibri",
        "Caladea" => "Cambria",
        other => other,
    };
    // CamelCase -> spaced words ("TimesNewRoman" -> "Times New Roman")
    let mut out = String::new();
    let chars: Vec<char> = mapped.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
            out.push(' ');
        }
        out.push(*c);
    }
    if out.is_empty() { "Calibri".into() } else { out }
}

pub(super) fn starts_list_item(text: &str) -> bool {
    let t = text.trim_start();
    // The \u{F0..} ones are bullets of the Symbol and Wingdings fonts
    if t.starts_with(['•', '●', '◦', '○', '▪', '■', '□', '–', '-', '*', '➢', '✓', '\u{F0B7}', '\u{F0A7}', '\u{F0D8}', '\u{F076}']) {
        return true;
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && digits.len() <= 3 && t[digits.len()..].starts_with(['.', ')'])
}

/// "a.", "iv)", "B." and similar
pub(super) fn is_enumerator(text: &str) -> bool {
    let Some(body) = text.strip_suffix(['.', ')']) else { return false };
    let body = body.trim_start_matches('(');
    !body.is_empty() && body.len() <= 4 && body.chars().all(|c| c.is_ascii_alphanumeric())
}

pub(super) fn color_of(c: Result<PdfColor, PdfiumError>) -> Option<[u8; 3]> {
    let c = c.ok()?;
    (c.alpha() > 30).then(|| [c.red(), c.green(), c.blue()])
}

pub(super) fn is_white(c: [u8; 3]) -> bool {
    c.iter().all(|v| *v >= 245)
}
