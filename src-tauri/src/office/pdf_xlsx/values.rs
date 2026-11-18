//! Numbers, amounts and dates

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notation {
    /// 1.234.567,89
    Indonesian,
    /// 1,234,567.89
    English,
}

/// Which notation a document writes its numbers in, from the ones that can only be read
/// one way
pub fn notation(texts: &[String]) -> Notation {
    let (mut id, mut en) = (0, 0);
    for t in texts {
        for token in t.split_whitespace() {
            let token = token.trim_matches(|c: char| !c.is_ascii_digit());
            let (dots, commas) = (token.matches('.').count(), token.matches(',').count());
            if dots + commas == 0 || !token.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
                continue;
            }
            let last = token.rfind(['.', ',']).unwrap();
            let after = token.len() - last - 1;
            match (dots, commas) {
                (d, c) if d > 0 && c > 0 => {
                    if token.as_bytes()[last] == b',' { id += 2 } else { en += 2 }
                }
                (d, 0) if d > 1 => id += 1,
                (0, c) if c > 1 => en += 1,
                (1, 0) if after != 3 => en += 1,
                (0, 1) if after != 3 => id += 1,
                _ => {}
            }
        }
    }
    if id > en { Notation::Indonesian } else { Notation::English }
}

const MONTHS: [(&str, u32); 30] = [
    ("januari", 1), ("january", 1), ("februari", 2), ("february", 2), ("maret", 3), ("march", 3), ("april", 4), ("mei", 5), ("may", 5),
    ("juni", 6), ("june", 6), ("juli", 7), ("july", 7), ("agustus", 8), ("august", 8), ("september", 9), ("oktober", 10), ("october", 10),
    ("november", 11), ("desember", 12), ("december", 12), ("agu", 8), ("ags", 8), ("aug", 8), ("okt", 10), ("oct", 10), ("des", 12), ("dec", 12),
    ("sept", 9), ("sep", 9),
];

pub(super) fn month(word: &str) -> Option<(u32, bool)> {
    let w = word.trim_end_matches('.').to_lowercase();
    if let Some((_, m)) = MONTHS.iter().find(|(name, _)| *name == w) {
        // Indonesian names that differ from English ones
        let indonesian = matches!(w.as_str(), "januari" | "februari" | "maret" | "mei" | "juni" | "juli" | "agustus" | "oktober" | "desember" | "agu" | "ags" | "okt" | "des");
        return Some((*m, indonesian));
    }
    // three-letter abbreviations: jan, feb, mar, apr, jun, jul, nov
    let short = ["jan", "feb", "mar", "apr", "mei", "jun", "jul", "agu", "sep", "okt", "nov", "des"];
    let english = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    short.iter().position(|s| *s == w).map(|i| (i as u32 + 1, english[i] != w)).or_else(|| english.iter().position(|s| *s == w).map(|i| (i as u32 + 1, false)))
}

fn year(y: u32) -> u32 {
    match y {
        0..=29 => 2000 + y,
        30..=99 => 1900 + y,
        y => y,
    }
}

/// A date (and time) written out -> serial number and a format that shows it the same way
pub(super) fn date(text: &str, notation: Notation) -> Option<(f64, String)> {
    let t = text.trim();
    if t.len() < 6 || t.len() > 30 {
        return None;
    }
    let (day_part, time_part) = match t.rsplit_once(' ') {
        Some((d, tm)) if tm.contains(':') => (d.trim(), Some(tm)),
        _ => (t, None),
    };
    let serial = |y: u32, m: u32, d: u32| -> Option<f64> {
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1900..=2200).contains(&y) {
            return None;
        }
        crate::office::formula::serial(y as i64, m as i64, d as i64)
    };
    let parts: Vec<&str> = day_part.split(['/', '-', '.', ' ', ',']).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let num = |s: &str| s.parse::<u32>().ok().filter(|_| s.chars().all(|c| c.is_ascii_digit()));
    let sep = day_part.chars().find(|c| matches!(c, '/' | '-' | '.')).unwrap_or('/');
    let (value, fmt) = match (num(parts[0]), num(parts[1]), num(parts[2])) {
        // 2026-11-15
        (Some(y), Some(m), Some(d)) if parts[0].len() == 4 => (serial(y, m, d)?, format!("yyyy{sep}mm{sep}dd")),
        // 15/11/2026 or 11/15/2026
        (Some(a), Some(b), Some(y)) if parts[2].len() == 4 || parts[2].len() == 2 => {
            let day_first = if a > 12 { true } else if b > 12 { false } else { notation == Notation::Indonesian };
            let (d, m) = if day_first { (a, b) } else { (b, a) };
            let yy = if parts[2].len() == 2 { "yy" } else { "yyyy" };
            let f = if day_first { format!("dd{sep}mm{sep}{yy}") } else { format!("mm{sep}dd{sep}{yy}") };
            (serial(year(y), m, d)?, f)
        }
        // 15 November 2026, 15-Nov-26
        (Some(d), None, Some(y)) => {
            let (m, indonesian) = month(parts[1])?;
            let long = parts[1].trim_end_matches('.').len() > 3;
            let yy = if parts[2].len() == 2 { "yy" } else { "yyyy" };
            let mm = if long { "mmmm" } else { "mmm" };
            let s = if sep == ' ' || day_part.contains(' ') { ' ' } else { sep };
            let lang = if indonesian { "[$-421]" } else { "" };
            (serial(year(y), m, d)?, format!("{lang}d{s}{mm}{s}{yy}"))
        }
        // November 15, 2026
        (None, Some(d), Some(y)) if parts[2].len() == 4 => {
            let (m, _) = month(parts[0])?;
            (serial(y, m, d)?, "mmmm d, yyyy".to_string())
        }
        _ => return None,
    };
    match time_part {
        Some(tm) => {
            let (frac, tf) = time(tm)?;
            Some((value + frac, format!("{fmt} {tf}")))
        }
        None => Some((value, fmt)),
    }
}

/// "08:30" or "08:30:15" -> fraction of a day
fn time(t: &str) -> Option<(f64, &'static str)> {
    let parts: Vec<u32> = t.trim().split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts.as_slice() {
        [h, m] if *h < 24 && *m < 60 => Some(((*h as f64 * 60.0 + *m as f64) / 1440.0, "hh:mm")),
        [h, m, s] if *h < 24 && *m < 60 && *s < 60 => Some(((*h as f64 * 3600.0 + *m as f64 * 60.0 + *s as f64) / 86400.0, "hh:mm:ss")),
        _ => None,
    }
}

/// A cell's text as Excel would hold it: a number with the format that shows it as
/// written, or the text itself
pub fn typed(text: &str, notation: Notation) -> (Value, Option<String>) {
    let t = text.trim();
    let keep = || (Value::Text(t.to_string()), None);
    if t.is_empty() {
        return (Value::Empty, None);
    }
    if t.len() > 40 || t.chars().filter(|c| c.is_alphabetic()).count() > 12 {
        return keep();
    }
    if let Some(d) = date(t, notation) {
        return (Value::Number(d.0), Some(d.1));
    }
    if t.contains(':') {
        return match time(t) {
            Some((f, fmt)) => (Value::Number(f), Some(fmt.to_string())),
            None => keep(),
        };
    }
    let mut s = t.replace(['\u{a0}', ' '], "");
    // "+Rp 9.537.000" (phone numbers stay text: too many digits)
    if let Some(rest) = s.strip_prefix('+') {
        s = rest.to_string();
    }
    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].to_string();
    }
    // currencies before or after the amount
    let mut currency = None;
    for (sym, fmt) in [("IDR", "\"IDR\" "), ("Rp.", "\"Rp\""), ("Rp", "\"Rp\""), ("USD", "\"USD\" "), ("US$", "\"US$\""), ("$", "$"), ("€", "€"), ("£", "£"), ("¥", "¥")] {
        if let Some(rest) = s.strip_prefix(sym) {
            currency = Some(fmt);
            s = rest.to_string();
            break;
        }
        if let Some(rest) = s.strip_suffix(sym) {
            currency = Some(fmt);
            s = rest.to_string();
            break;
        }
    }
    if s.starts_with('-') || s.starts_with('−') {
        negative = !negative;
        s = s.trim_start_matches(['-', '−']).to_string();
    } else if s.ends_with('-') && s.len() > 1 {
        negative = !negative;
        s.pop();
    }
    let percent = s.ends_with('%');
    if percent {
        s.pop();
    }
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') || !s.starts_with(|c: char| c.is_ascii_digit()) {
        return keep();
    }
    let (dec, grp) = match notation {
        Notation::Indonesian => (',', '.'),
        Notation::English => ('.', ','),
    };
    // a lone separator of the other kind still reads as a decimal point when it can't
    // be a thousands separator ("12.5" in an Indonesian document)
    let (int_part, frac_part) = match s.rsplit_once(dec) {
        Some((i, f)) if !f.contains(grp) => (i.to_string(), Some(f.to_string())),
        Some(_) => return keep(),
        None => match s.rsplit_once(grp) {
            Some((i, f)) if !i.contains(grp) && f.len() != 3 && !i.contains(dec) => (i.to_string(), Some(f.to_string())),
            _ => (s.clone(), None),
        },
    };
    // the decimal separator can't come before the last group
    if int_part.contains(dec) {
        return keep();
    }
    let grouped = int_part.contains(grp);
    if grouped {
        let groups: Vec<&str> = int_part.split(grp).collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return keep();
        }
    }
    let digits: String = int_part.chars().filter(|c| c.is_ascii_digit()).collect();
    // codes, account and ID numbers: leading zeros, or too many digits for a number
    if (digits.len() > 1 && digits.starts_with('0') && frac_part.is_none() && !grouped) || (digits.len() >= 12 && !grouped) || digits.len() > 15 {
        return keep();
    }
    if frac_part.as_ref().is_some_and(|f| f.is_empty() || f.contains([dec, grp])) {
        return keep();
    }
    let decimals = frac_part.as_ref().map(|f| f.len()).unwrap_or(0);
    let Ok(mut n) = format!("{digits}.{}", frac_part.as_deref().unwrap_or("0")).parse::<f64>() else { return keep() };
    if negative {
        n = -n;
    }
    let zeros = if decimals > 0 { format!(".{}", "0".repeat(decimals.min(10))) } else { String::new() };
    if percent {
        return (Value::Number(n / 100.0), Some(format!("0{zeros}%")));
    }
    let body = if grouped || currency.is_some() || n.abs() >= 10_000.0 && decimals > 0 { format!("#,##0{zeros}") } else if decimals > 0 { format!("0{zeros}") } else { String::new() };
    let fmt = match (currency, body.is_empty()) {
        (Some(c), _) => {
            let b = if body.is_empty() { "#,##0".to_string() } else { body };
            Some(if t.starts_with('(') { format!("{c}{b};({c}{b})") } else { format!("{c}{b}") })
        }
        (None, false) => Some(if t.starts_with('(') { format!("{body};({body})") } else { body }),
        (None, true) => None,
    };
    (Value::Number(n), fmt)
}
