//! Dates and times

use super::*;

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

pub(super) fn format_date(v: f64, body: &str, date1904: bool, sep: Separators) -> Shown {
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
                let next_is_sec = chars[i + n..].iter().find(|c| c.is_ascii_alphabetic()).map(|c| c.eq_ignore_ascii_case(&'s')).unwrap_or(false);
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
