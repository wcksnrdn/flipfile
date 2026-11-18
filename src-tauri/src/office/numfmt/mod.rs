// Excel number formats: turns a cell's value and its format code ("#,##0.00",
// accounting, dates, percentages, [Red], …) into the text Excel would show.
//
// Separators follow the computer's region, as Excel does: 1,234.56 in the US,
// 1.234,56 in Indonesia. Excel's padding (`_)`) becomes invisible text of the right
// width, and its fill (`* `) is reported so the caller can stretch it across the cell
// (that is what puts the currency sign at the left edge in accounting formats).

// A format code, read section by section. The submodules share one namespace (each
// starts with `use super::*`), as when this was a single file.
mod dates;
mod numbers;
mod sections;
mod text;

#[cfg(test)]
mod tests;

use dates::*;
use numbers::*;
use sections::*;
use text::*;


/// Decimal and thousands separators of the computer's region
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Separators {
    pub decimal: char,
    pub group: char,
    /// day and month names in Indonesian (the system's language), like Indonesian Excel
    pub indonesian: bool,
}

impl Separators {
    pub const US: Separators = Separators { decimal: '.', group: ',', indonesian: false };
    #[cfg(test)]
    pub const COMMA: Separators = Separators { decimal: ',', group: '.', indonesian: false };

    /// The separators of the computer's region, as Excel uses them
    pub fn system() -> Separators {
        let (decimal, group, lang) = system_separators().unwrap_or(('.', ',', String::new()));
        // a region without grouping (or with a space) still needs something sensible
        let group = if group == decimal { if decimal == ',' { '.' } else { ',' } } else { group };
        Separators { decimal, group, indonesian: lang.to_lowercase().starts_with("id") }
    }
}

// macOS: the region set in System Settings
#[cfg(target_os = "macos")]
fn system_separators() -> Option<(char, char, String)> {
    use objc2_foundation::NSLocale;
    let locale = NSLocale::currentLocale();
    let first = |s: objc2::rc::Retained<objc2_foundation::NSString>| s.to_string().chars().next();
    Some((first(locale.decimalSeparator())?, first(locale.groupingSeparator()).unwrap_or(','), locale.languageCode().to_string()))
}

// Windows: the user's region (Settings > Time & language > Region)
#[cfg(windows)]
fn system_separators() -> Option<(char, char, String)> {
    use windows::core::PCWSTR;
    use windows::Win32::Globalization::{GetLocaleInfoEx, GetUserDefaultLocaleName, LOCALE_SDECIMAL, LOCALE_STHOUSAND};
    let read = |kind: u32| -> Option<char> {
        let mut buf = [0u16; 8];
        // SAFETY: a null locale name means the user's default locale; the buffer is ours
        let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), kind, Some(&mut buf)) };
        (n > 1).then(|| char::from_u32(buf[0] as u32)).flatten()
    };
    let mut name = [0u16; 85];
    // SAFETY: the buffer is ours and LOCALE_NAME_MAX_LENGTH (85) long
    let n = unsafe { GetUserDefaultLocaleName(&mut name) };
    let lang = String::from_utf16_lossy(&name[..(n.max(1) - 1) as usize]);
    Some((read(LOCALE_SDECIMAL)?, read(LOCALE_STHOUSAND).unwrap_or(','), lang))
}

// Linux: the locale environment (LC_ALL, LC_NUMERIC, LANG)
#[cfg(not(any(target_os = "macos", windows)))]
fn system_separators() -> Option<(char, char, String)> {
    let lang = ["LC_ALL", "LC_NUMERIC", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))?;
    let code = lang.split(['_', '.', '@']).next()?.to_lowercase();
    // regions that write 1.234,56
    let comma = ["id", "de", "nl", "it", "es", "pt", "da", "tr", "el", "ro", "hr", "sl", "sr", "vi", "fr", "ru", "pl", "cs", "sv", "fi", "nb", "uk"];
    Some(if comma.contains(&code.as_str()) { (',', '.', code) } else { ('.', ',', code) })
}

/// Marks text that takes up room but is not drawn (Excel's `_x` padding): the
/// characters between `HIDE_ON` and `HIDE_OFF` are rendered transparent.
pub const HIDE_ON: char = '\u{E000}';
pub const HIDE_OFF: char = '\u{E001}';

/// What a formatted cell shows
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    /// Text before the fill point (or all of it), may contain hidden runs
    pub text: String,
    /// Text after the fill point, when the format has a `*x` fill
    pub after_fill: Option<String>,
    /// The repeated fill character (usually a space)
    pub fill: Option<char>,
    /// A colour from the format (`[Red]`), as RGB
    pub color: Option<[u8; 3]>,
}

impl Shown {
    fn plain(text: String) -> Self {
        Shown { text, ..Default::default() }
    }
}

/// The codes of Excel's built-in formats (those with no `<numFmt>` entry)
pub fn builtin(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "SHORTDATE", // the system's short date
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "SHORTDATE h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        41 => r#"_(* #,##0_);_(* \(#,##0\);_(* "-"_);_(@_)"#,
        42 => r#"_("$"* #,##0_);_("$"* \(#,##0\);_("$"* "-"_);_(@_)"#,
        43 => r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#,
        44 => r#"_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)"#,
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// Does this format show a date or time (so numbers in it are serial dates)?
pub fn is_date_format(code: &str) -> bool {
    if code.contains("SHORTDATE") {
        return true;
    }
    let mut quoted = false;
    let mut chars = code.chars().peekable();
    let mut prev = ' ';
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' | '_' | '*' if !quoted => {
                chars.next();
            }
            '[' if !quoted => {
                // [h] [m] [s] are elapsed time; [Red], [$-409], [>0] are not dates
                let inner: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if matches!(inner.to_lowercase().as_str(), "h" | "hh" | "m" | "mm" | "s" | "ss") {
                    return true;
                }
            }
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' if !quoted => return true,
            // "m" alone would be ambiguous only next to digit placeholders; Excel treats it as month/minute
            'm' | 'M' if !quoted && prev != 'E' && prev != 'e' => return true,
            _ => {}
        }
        prev = c;
    }
    false
}

/// A cell's value
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(String),
}

/// The text Excel shows for `value` in format `code`
pub fn format(value: &Value, code: &str, date1904: bool, sep: Separators) -> Shown {
    match value {
        Value::Bool(b) => Shown::plain(if *b { "TRUE" } else { "FALSE" }.into()),
        Value::Error(e) => Shown::plain(e.clone()),
        Value::Text(t) => format_text(t, code),
        Value::Number(n) => format_number(*n, code, date1904, sep),
    }
}
