//! Numbers

use super::*;

pub(super) fn format_number(v: f64, code: &str, date1904: bool, sep: Separators) -> Shown {
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

pub(super) fn group(digits: &str, sep: char) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) && c.is_ascii_digit() && chars[i - 1].is_ascii_digit() {
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
