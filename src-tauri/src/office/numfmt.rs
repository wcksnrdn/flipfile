// Excel number formats: turns a cell's value and its format code ("#,##0.00",
// accounting, dates, percentages, [Red], …) into the text Excel would show.
//
// Separators follow the computer's region, as Excel does: 1,234.56 in the US,
// 1.234,56 in Indonesia. Excel's padding (`_)`) becomes invisible text of the right
// width, and its fill (`* `) is reported so the caller can stretch it across the cell
// (that is what puts the currency sign at the left edge in accounting formats).

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

// ---------------------------------------------------------------------------
// Sections

/// Splits a format into its `;`-separated sections (ignoring `;` in quotes, escapes, brackets)
fn sections(code: &str) -> Vec<String> {
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
struct Modifiers {
    color: Option<[u8; 3]>,
    condition: Option<(String, f64)>,
    body: String,
}

fn modifiers(section: &str) -> Modifiers {
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
            return super::xlsx::indexed_color(n.checked_sub(1)? + 8);
        }
    })
}

fn holds(cond: &(String, f64), v: f64) -> bool {
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

// ---------------------------------------------------------------------------
// Text

fn format_text(text: &str, code: &str) -> Shown {
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
fn render_literals(body: &str, at: &dyn Fn(char) -> Option<String>) -> Shown {
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
                if let Some(n) = chars.next() {
                    if out.fill.is_none() {
                        out.fill = Some(n);
                        out.text = std::mem::take(&mut cur);
                    }
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

// ---------------------------------------------------------------------------
// Numbers

fn format_number(v: f64, code: &str, date1904: bool, sep: Separators) -> Shown {
    let code = if code.trim().is_empty() { "General" } else { code };
    let secs: Vec<Modifiers> = sections(code).iter().map(|s| modifiers(s)).collect();
    // Pick the section, and whether it already shows the minus sign itself
    let (m, abs) = if secs.iter().any(|s| s.condition.is_some()) {
        let i = secs.iter().position(|s| s.condition.as_ref().map(|c| holds(c, v)).unwrap_or(false));
        match i {
            Some(i) => (&secs[i], v < 0.0 && i > 0),
            None => {
                // the first section without a condition
                let i = secs.iter().position(|s| s.condition.is_none()).unwrap_or(secs.len() - 1);
                (&secs[i], v < 0.0)
            }
        }
    } else {
        match secs.len() {
            1 => (&secs[0], false),
            2 => {
                if v < 0.0 {
                    (&secs[1], true)
                } else {
                    (&secs[0], false)
                }
            }
            _ => {
                if v < 0.0 {
                    (&secs[1], true)
                } else if v == 0.0 {
                    (&secs[2], false)
                } else {
                    (&secs[0], false)
                }
            }
        }
    };
    let value = if abs { v.abs() } else { v };
    let body = m.body.trim();
    let mut shown = if body.eq_ignore_ascii_case("general") || body.is_empty() && secs.len() == 1 {
        Shown::plain(general(value, sep))
    } else if is_date_format(&m.body) {
        format_date(value, &m.body, date1904, sep)
    } else {
        number_pattern(value, &m.body, sep)
    };
    shown.color = m.color;
    shown
}

/// Excel's "General": up to 11 characters, switching to scientific for huge/tiny numbers
pub fn general(v: f64, sep: Separators) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    let s = if !(1e-9..1e11).contains(&a) {
        let s = format!("{:.5E}", v);
        // 1.23457E11 -> 1.23457E+11, trimming zeros in the mantissa
        let (mant, exp) = s.split_once('E').unwrap();
        let mant = trim_zeros(mant);
        let e: i32 = exp.parse().unwrap_or(0);
        format!("{mant}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
    } else {
        // 10 significant digits fit in 11 characters with the point
        let int_digits = (a.log10().floor() as i32 + 1).max(1);
        let decimals = (10 - int_digits).clamp(0, 9) as usize;
        trim_zeros(&format!("{:.*}", decimals, v))
    };
    s.replace('.', &sep.decimal.to_string())
}

fn trim_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

#[derive(Debug)]
enum Tok {
    Lit(String),
    Hidden(char),
    Fill(char),
    Digit(char), // '0', '#', '?'
    Point,
    Group,
    Percent,
    Exp(bool), // E+ (true) / E- (false)
    Slash,
    At,
}

fn tokenize(body: &str) -> Vec<Tok> {
    let mut t = vec![];
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let s: String = chars.by_ref().take_while(|&q| q != '"').collect();
                t.push(Tok::Lit(s));
            }
            '\\' => {
                if let Some(n) = chars.next() {
                    t.push(Tok::Lit(n.to_string()));
                }
            }
            '_' => {
                if let Some(n) = chars.next() {
                    t.push(Tok::Hidden(n));
                }
            }
            '*' => {
                if let Some(n) = chars.next() {
                    t.push(Tok::Fill(n));
                }
            }
            '[' => {
                // locale tags ([$-421]) and the like print nothing
                for q in chars.by_ref() {
                    if q == ']' {
                        break;
                    }
                }
            }
            '0' | '#' | '?' => t.push(Tok::Digit(c)),
            '.' => t.push(Tok::Point),
            ',' => t.push(Tok::Group),
            '%' => t.push(Tok::Percent),
            'E' | 'e' if matches!(chars.peek(), Some('+') | Some('-')) => {
                let plus = chars.next() == Some('+');
                t.push(Tok::Exp(plus));
            }
            '/' => t.push(Tok::Slash),
            '@' => t.push(Tok::At),
            _ => t.push(Tok::Lit(c.to_string())),
        }
    }
    t
}

fn number_pattern(v: f64, body: &str, sep: Separators) -> Shown {
    let toks = tokenize(body);
    let has_digits = toks.iter().any(|t| matches!(t, Tok::Digit(_)));
    if !has_digits {
        // only literals, e.g. "-" for zero in accounting formats
        let at = |_| Some(general(v, sep));
        return render_literals(body, &at);
    }
    if toks.iter().any(|t| matches!(t, Tok::Slash)) {
        return fraction(v, &toks, sep);
    }
    let mut v = v;
    for t in &toks {
        if matches!(t, Tok::Percent) {
            v *= 100.0;
        }
    }
    // Commas right after the last digit placeholder divide by 1000 each
    let last_digit = toks.iter().rposition(|t| matches!(t, Tok::Digit(_))).unwrap();
    let mut scale_commas = 0;
    for t in &toks[last_digit + 1..] {
        match t {
            Tok::Group => scale_commas += 1,
            Tok::Point | Tok::Digit(_) | Tok::Exp(_) => break,
            _ => break,
        }
    }
    v /= 1000f64.powi(scale_commas);

    let exp_at = toks.iter().position(|t| matches!(t, Tok::Exp(_)));
    let point_at = toks.iter().position(|t| matches!(t, Tok::Point));
    let mantissa_end = exp_at.unwrap_or(toks.len());
    let int_end = point_at.filter(|&p| p < mantissa_end).unwrap_or(mantissa_end);
    let int_places = toks[..int_end].iter().filter(|t| matches!(t, Tok::Digit(_))).count();
    let dec_places: Vec<char> = toks[int_end..mantissa_end].iter().filter_map(|t| if let Tok::Digit(d) = t { Some(*d) } else { None }).collect();
    // a group comma between integer digit placeholders turns on thousands separators
    let first_int_digit = toks[..int_end].iter().position(|t| matches!(t, Tok::Digit(_)));
    let grouping = first_int_digit
        .map(|f| toks[f..int_end].iter().enumerate().any(|(i, t)| matches!(t, Tok::Group) && toks[f + i + 1..int_end].iter().any(|t| matches!(t, Tok::Digit(_)))))
        .unwrap_or(false);

    // scientific: normalise the mantissa
    let mut exponent = 0i32;
    if exp_at.is_some() && v != 0.0 {
        exponent = v.abs().log10().floor() as i32 - (int_places.max(1) as i32 - 1);
        v /= 10f64.powi(exponent);
    }

    let negative = v < 0.0;
    let rounded = round_half_away(v.abs(), dec_places.len());
    let s = format!("{:.*}", dec_places.len(), rounded);
    let (int_str, dec_str) = s.split_once('.').unwrap_or((&s, ""));
    let mut int_digits: Vec<char> = int_str.chars().collect();
    if int_digits == ['0'] && !toks[..int_end].iter().any(|t| matches!(t, Tok::Digit('0'))) {
        int_digits.clear(); // "#.##" shows .5, not 0.5
    }

    // Integer part: fill placeholders right to left, extra digits go into the leftmost
    let int_toks: Vec<&Tok> = toks[..int_end].iter().collect();
    let mut int_out: Vec<String> = vec![String::new(); int_toks.len()];
    let mut di = int_digits.len() as isize - 1;
    let mut placeholders_seen = 0;
    for (i, t) in int_toks.iter().enumerate().rev() {
        if let Tok::Digit(d) = t {
            placeholders_seen += 1;
            let mut piece = String::new();
            if placeholders_seen == int_places {
                // leftmost placeholder takes all remaining digits
                while di >= 0 {
                    piece.insert(0, int_digits[di as usize]);
                    di -= 1;
                }
                if piece.is_empty() {
                    piece = pad_digit(*d);
                }
            } else if di >= 0 {
                piece.push(int_digits[di as usize]);
                di -= 1;
            } else {
                piece = pad_digit(*d);
            }
            int_out[i] = piece;
        }
    }
    // grouping applies to the digits, so rebuild the digit run with separators
    let digits_joined: String = int_toks.iter().enumerate().filter(|(_, t)| matches!(t, Tok::Digit(_))).map(|(i, _)| int_out[i].clone()).collect();
    let digits_joined = if int_places == 0 { int_digits.iter().collect() } else { digits_joined };
    let grouped = if grouping { group(&digits_joined, sep.group) } else { digits_joined.clone() };

    // Decimals: '0' keeps zeros, '#' drops trailing ones, '?' turns them into spaces
    let mut dec: Vec<char> = dec_str.chars().collect();
    for i in (0..dec.len()).rev() {
        if dec[i] != '0' {
            break;
        }
        match dec_places[i] {
            '#' => {
                dec.pop();
            }
            '?' => dec[i] = '\u{2007}',
            _ => break,
        }
    }

    // Assemble, emitting the digit run once at the first integer placeholder
    let mut out = Shown::default();
    let mut cur = String::new();
    if negative {
        cur.push('-');
    }
    let mut digits_written = false;
    let mut dec_written = false;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(s) => cur.push_str(s),
            Tok::Hidden(c) => {
                cur.push(HIDE_ON);
                cur.push(*c);
                cur.push(HIDE_OFF);
            }
            Tok::Fill(c) => {
                if out.fill.is_none() {
                    out.fill = Some(*c);
                    out.text = std::mem::take(&mut cur);
                }
            }
            Tok::Digit(_) if i < int_end => {
                if !digits_written {
                    cur.push_str(&grouped);
                    digits_written = true;
                }
            }
            Tok::Point if i == int_end && point_at.is_some() => {
                if !digits_written {
                    cur.push_str(&grouped);
                    digits_written = true;
                }
                cur.push(sep.decimal);
            }
            Tok::Digit(_) if i < mantissa_end => {
                if !dec_written {
                    cur.extend(dec.iter());
                    dec_written = true;
                }
            }
            Tok::Exp(plus) => {
                cur.push('E');
                if exponent < 0 {
                    cur.push('-');
                } else if *plus {
                    cur.push('+');
                }
                let exp_places = toks[i + 1..].iter().filter(|t| matches!(t, Tok::Digit(_))).count().max(1);
                cur.push_str(&format!("{:0width$}", exponent.abs(), width = exp_places));
                // the exponent's digit placeholders are consumed here
                return finish(out, cur, &toks[i + 1..]);
            }
            Tok::Digit(_) => {}
            Tok::Percent => cur.push('%'),
            Tok::Group | Tok::Point | Tok::Slash | Tok::At => {}
        }
    }
    if out.fill.is_some() {
        out.after_fill = Some(cur);
    } else {
        out.text = cur;
    }
    out
}

/// Literals after a scientific exponent
fn finish(mut out: Shown, mut cur: String, rest: &[Tok]) -> Shown {
    for t in rest {
        match t {
            Tok::Lit(s) => cur.push_str(s),
            Tok::Hidden(c) => {
                cur.push(HIDE_ON);
                cur.push(*c);
                cur.push(HIDE_OFF);
            }
            _ => {}
        }
    }
    if out.fill.is_some() {
        out.after_fill = Some(cur);
    } else {
        out.text = cur;
    }
    out
}

fn pad_digit(d: char) -> String {
    match d {
        '0' => "0".into(),
        '?' => "\u{2007}".into(), // a figure space keeps columns aligned
        _ => String::new(),
    }
}

fn group(digits: &str, sep: char) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 && c.is_ascii_digit() && chars[i - 1].is_ascii_digit() {
            out.push(sep);
        }
        out.push(*c);
    }
    out
}

/// Rounds half away from zero at `places` decimals (as Excel does), robust to 2.675-style binary noise
fn round_half_away(v: f64, places: usize) -> f64 {
    let f = 10f64.powi(places as i32);
    let x = v * f;
    // nudge by a relative epsilon so values like 1.005 round up like in Excel
    let nudged = x + x.abs() * 1e-12;
    nudged.round() / f
}

/// "# ?/?" style fractions: the closest fraction with a 1- or 2-digit denominator
fn fraction(v: f64, toks: &[Tok], sep: Separators) -> Shown {
    let slash = toks.iter().position(|t| matches!(t, Tok::Slash)).unwrap();
    let den_places = toks[slash + 1..].iter().filter(|t| matches!(t, Tok::Digit(_))).count().max(1);
    let max_den = 10i64.pow(den_places as u32) - 1;
    let whole_part = toks[..slash].iter().any(|t| matches!(t, Tok::Lit(s) if s == " "));
    let sign = if v < 0.0 { "-" } else { "" };
    let a = v.abs();
    let (whole, frac) = if whole_part { (a.trunc(), a.fract()) } else { (0.0, a) };
    let (mut best_n, mut best_d, mut best_err) = (0i64, 1i64, f64::MAX);
    for d in 1..=max_den {
        let n = (frac * d as f64).round() as i64;
        let err = (frac - n as f64 / d as f64).abs();
        if err < best_err - 1e-12 {
            (best_n, best_d, best_err) = (n, d, err);
        }
    }
    let text = if best_n == 0 {
        format!("{sign}{}", whole as i64)
    } else if whole_part && whole > 0.0 {
        format!("{sign}{} {best_n}/{best_d}", whole as i64)
    } else {
        let _ = sep;
        format!("{sign}{best_n}/{best_d}")
    };
    Shown::plain(text)
}

// ---------------------------------------------------------------------------
// Dates and times

/// (year, month, day, seconds of the day) for an Excel serial date
fn serial_to_date(v: f64, date1904: bool) -> (i64, u32, u32, f64) {
    let mut days = v.floor() as i64;
    let secs = ((v - v.floor()) * 86400.0 * 1000.0).round() / 1000.0;
    if date1904 {
        days += 1462; // 1904-01-01 in the 1900 system
    }
    // Excel's 1900 system pretends 1900-02-29 exists (serial 60)
    if !date1904 && days == 60 {
        return (1900, 2, 29, secs);
    }
    if !date1904 && days < 60 {
        days += 1;
    }
    // days since 1899-12-30 -> civil date (Howard Hinnant's algorithm)
    let z = days - 25569 + 719468; // shift to days since 0000-03-01 via the Unix epoch
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d, secs)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS_ID: [&str; 12] = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
const DAYS_ID: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];

fn weekday(y: i64, m: u32, d: u32) -> usize {
    // Zeller-like: days since a known Sunday
    let (y, m) = if m < 3 { (y - 1, m + 12) } else { (y, m) };
    let k = y.rem_euclid(100);
    let j = y.div_euclid(100);
    let h = (d as i64 + (13 * (m as i64 + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    ((h + 6) % 7) as usize // 0 = Sunday
}

fn format_date(v: f64, body: &str, date1904: bool, sep: Separators) -> Shown {
    // the system's short date: day first outside the US-style regions
    let short = if sep == Separators::US { "m/d/yyyy" } else { "dd/mm/yyyy" };
    let body = body.replace("SHORTDATE", short);
    // the format's own language wins ([$-421] Indonesian, [$-409] English), else the system's
    let indonesian = body.contains("-421") || body.contains("-0421") || (sep.indonesian && !body.contains("-409"));
    let (months, days) = if indonesian { (&MONTHS_ID, &DAYS_ID) } else { (&MONTHS, &DAYS) };
    let (y, mo, d, secs) = serial_to_date(v, date1904);
    let ampm = body.to_lowercase().contains("am/pm") || body.to_lowercase().contains("a/p");

    // seconds decimals ("ss.0") round the time first
    let frac_places = body.split(['s', 'S']).nth(1).map(|r| r.chars().skip(1).take_while(|c| *c == '0').count()).filter(|_| body.contains("s.0") || body.contains("S.0")).unwrap_or(0);
    let total = if frac_places == 0 { secs.round() } else { secs };
    let hh = (total / 3600.0).floor() as i64;
    let mm = ((total % 3600.0) / 60.0).floor() as i64;
    let ss = total % 60.0;

    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut last_was_hour = false;
    let run = |i: usize, c: char| chars[i..].iter().take_while(|&&x| x.to_ascii_lowercase() == c).count();
    while i < chars.len() {
        let c = chars[i];
        let lc = c.to_ascii_lowercase();
        match lc {
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                if i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                }
                i += 2;
            }
            '_' => {
                if i + 1 < chars.len() {
                    out.push(HIDE_ON);
                    out.push(chars[i + 1]);
                    out.push(HIDE_OFF);
                }
                i += 2;
            }
            '*' => i += 2,
            '[' => {
                let end = chars[i..].iter().position(|&x| x == ']').map(|p| i + p).unwrap_or(chars.len() - 1);
                let inner: String = chars[i + 1..end].iter().collect::<String>().to_lowercase();
                let total_h = (v * 24.0).floor() as i64;
                match inner.chars().next() {
                    Some('h') => out.push_str(&format!("{:0w$}", total_h, w = inner.len())),
                    Some('m') => out.push_str(&format!("{:0w$}", (v * 1440.0).floor() as i64, w = inner.len())),
                    Some('s') => out.push_str(&format!("{:0w$}", (v * 86400.0).round() as i64, w = inner.len())),
                    _ => {}
                }
                last_was_hour = inner.starts_with('h');
                i = end + 1;
            }
            'y' => {
                let n = run(i, 'y');
                out.push_str(&if n <= 2 { format!("{:02}", y % 100) } else { format!("{y}") });
                i += n;
            }
            'd' => {
                let n = run(i, 'd');
                let wd = weekday(y, mo, d);
                out.push_str(&match n {
                    1 => d.to_string(),
                    2 => format!("{d:02}"),
                    3 => days[wd][..3].to_string(),
                    _ => days[wd].to_string(),
                });
                i += n;
            }
            'h' => {
                let n = run(i, 'h');
                let h = if ampm { ((hh % 12) + 11) % 12 + 1 } else { hh % 24 };
                out.push_str(&if n >= 2 { format!("{h:02}") } else { h.to_string() });
                last_was_hour = true;
                i += n;
            }
            'm' => {
                let n = run(i, 'm');
                // minutes when right after hours or right before seconds
                let next_is_sec = chars[i + n..].iter().find(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_lowercase() == 's').unwrap_or(false);
                if (last_was_hour || next_is_sec) && n <= 2 {
                    out.push_str(&if n == 2 { format!("{mm:02}") } else { mm.to_string() });
                } else {
                    let name = months[(mo - 1) as usize];
                    out.push_str(&match n {
                        1 => mo.to_string(),
                        2 => format!("{mo:02}"),
                        3 => name[..3].to_string(),
                        5 => name[..1].to_string(),
                        _ => name.to_string(),
                    });
                }
                last_was_hour = false;
                i += n;
            }
            's' => {
                let n = run(i, 's');
                let whole = ss.floor() as i64;
                out.push_str(&if n >= 2 { format!("{whole:02}") } else { whole.to_string() });
                i += n;
                if i < chars.len() && chars[i] == '.' && chars.get(i + 1) == Some(&'0') {
                    let places = chars[i + 1..].iter().take_while(|&&c| c == '0').count();
                    let frac = ((ss - ss.floor()) * 10f64.powi(places as i32)).round() as i64;
                    out.push(sep.decimal);
                    out.push_str(&format!("{frac:0places$}"));
                    i += 1 + places;
                }
                last_was_hour = false;
            }
            'a' if body[byte_index(&chars, i)..].to_lowercase().starts_with("am/pm") => {
                out.push_str(if hh % 24 < 12 { "AM" } else { "PM" });
                i += 5;
            }
            'a' if body[byte_index(&chars, i)..].to_lowercase().starts_with("a/p") => {
                out.push_str(if hh % 24 < 12 { "A" } else { "P" });
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    Shown::plain(out)
}

fn byte_index(chars: &[char], i: usize) -> usize {
    chars[..i].iter().map(|c| c.len_utf8()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is visible: hidden padding removed, a fill shown as one character
    fn visible(s: &Shown) -> String {
        let mut t = s.text.clone();
        if let Some(fill) = s.fill {
            t.push(fill);
            t.push_str(s.after_fill.as_deref().unwrap_or(""));
        }
        let mut out = String::new();
        let mut hidden = false;
        for c in t.chars() {
            match c {
                HIDE_ON => hidden = true,
                HIDE_OFF => hidden = false,
                c if !hidden => out.push(c),
                _ => {}
            }
        }
        out
    }
    fn f(v: f64, code: &str) -> String {
        visible(&format(&Value::Number(v), code, false, Separators::US))
    }
    fn id(v: f64, code: &str) -> String {
        visible(&format(&Value::Number(v), code, false, Separators::COMMA))
    }

    #[test]
    fn general_numbers() {
        assert_eq!(f(0.0, "General"), "0");
        assert_eq!(f(1234.5, "General"), "1234.5");
        assert_eq!(f(0.1 + 0.2, "General"), "0.3");
        assert_eq!(f(-12.0, "General"), "-12");
        assert_eq!(f(123456789012.0, "General"), "1.23457E+11");
        assert_eq!(id(1234.5, "General"), "1234,5");
    }

    #[test]
    fn plain_patterns() {
        assert_eq!(f(1234567.891, "#,##0.00"), "1,234,567.89");
        assert_eq!(id(1234567.891, "#,##0.00"), "1.234.567,89");
        assert_eq!(f(-1234.5, "#,##0"), "-1,235");
        assert_eq!(f(0.5, "0%"), "50%");
        assert_eq!(f(0.12345, "0.00%"), "12.35%");
        assert_eq!(f(7.0, "000"), "007");
        assert_eq!(f(0.5, "#.##"), ".5");
        assert_eq!(f(1.005, "0.00"), "1.01");
        assert_eq!(f(2.675, "0.00"), "2.68");
        assert_eq!(f(1234567.0, "#,##0,"), "1,235");
        assert_eq!(f(12345.0, "0.00E+00"), "1.23E+04");
        assert_eq!(f(12.5, ".00"), "12.50");
        assert_eq!(f(5.0, "[$-409]#,##0"), "5");
    }

    #[test]
    fn sections_and_colours() {
        assert_eq!(f(-1234.0, "#,##0;(#,##0)"), "(1,234)");
        assert_eq!(f(0.0, "#,##0;(#,##0);\"-\""), "-");
        let s = format(&Value::Number(-5.0), "#,##0;[Red]-#,##0", false, Separators::US);
        assert_eq!(s.color, Some([255, 0, 0]));
        assert_eq!(s.text, "-5");
        assert_eq!(f(150.0, "[>100]\"big\";\"small\""), "big");
    }

    #[test]
    fn accounting() {
        let code = builtin(44).unwrap();
        let s = format(&Value::Number(-1250.5), code, false, Separators::US);
        assert_eq!(s.fill, Some(' '));
        assert_eq!(f(-1250.5, code), "$ (1,250.50)");
        assert_eq!(f(1250.5, code), "$ 1,250.50");
        // the padding is there, just invisible: positives line up with the ")" of negatives
        assert!(s.text.starts_with(HIDE_ON) && f(1250.5, code) != visible(&Shown::plain(format(&Value::Number(1250.5), code, false, Separators::US).text)));
        // Rupiah, as Indonesian Excel writes it
        let rp = r#"_-[$Rp-421]* #,##0_-;\-[$Rp-421]* #,##0_-;_-[$Rp-421]* "-"_-;_-@_-"#;
        assert_eq!(id(1500000.0, rp), "Rp 1.500.000");
        assert_eq!(id(-25000.0, rp), "-Rp 25.000");
        assert_eq!(id(0.0, rp), "Rp -");
    }

    #[test]
    fn text_sections() {
        let s = format(&Value::Text("Total".into()), r#"_(@_)"#, false, Separators::US);
        assert_eq!(visible(&s), "Total");
        assert_eq!(format(&Value::Text("abc".into()), "0.00", false, Separators::US).text, "abc");
        assert_eq!(format(&Value::Text("x".into()), "\"Name: \"@", false, Separators::US).text, "Name: x");
    }

    #[test]
    fn dates() {
        // 45000 = 2023-03-15, 1900-system
        assert_eq!(f(45000.0, "yyyy-mm-dd"), "2023-03-15");
        assert_eq!(f(45000.0, "d mmmm yyyy"), "15 March 2023");
        assert_eq!(f(45000.0, "ddd, d mmm yy"), "Wed, 15 Mar 23");
        assert_eq!(f(45000.75, "h:mm AM/PM"), "6:00 PM");
        assert_eq!(f(45000.5, "hh:mm:ss"), "12:00:00");
        assert_eq!(f(1.5, "[h]:mm"), "36:00");
        assert_eq!(f(45000.0, builtin(14).unwrap()), "3/15/2023");
        assert_eq!(id(45000.0, builtin(14).unwrap()), "15/03/2023");
        assert_eq!(f(45000.0, "[$-421]dddd, d mmmm yyyy"), "Rabu, 15 Maret 2023");
        assert_eq!(f(60.0, "yyyy-mm-dd"), "1900-02-29");
        assert_eq!(f(61.0, "yyyy-mm-dd"), "1900-03-01");
        assert_eq!(f(1.0, "yyyy-mm-dd"), "1900-01-01");
        let id_sys = Separators { indonesian: true, ..Separators::COMMA };
        assert_eq!(visible(&format(&Value::Number(45000.0), "dddd, d mmmm yyyy", false, id_sys)), "Rabu, 15 Maret 2023");
        assert_eq!(visible(&format(&Value::Number(45000.0), "[$-409]d mmm yyyy", false, id_sys)), "15 Mar 2023");
        assert!(is_date_format("dd/mm/yyyy") && is_date_format("[h]:mm") && !is_date_format("#,##0.00") && !is_date_format("[Red]0.00"));
    }

    #[test]
    fn fractions() {
        assert_eq!(f(1.25, "# ?/?"), "1 1/4");
        assert_eq!(f(0.333, "?/?"), "1/3");
    }
}
