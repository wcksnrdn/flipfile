//! Functions of single values: logic, text, maths, rounding and dates

use super::*;

impl Eval<'_> {
    pub(super) fn scalar_call(&self, name: &str, args: &[Expr]) -> Result<V, &'static str> {
        let n = |i: usize| self.num_arg(args, i, None);
        let t = |i: usize| self.text_arg(args, i);
        Ok(match name {
            "SUM" => V::Num(self.numbers(args)?.iter().sum()),
            "PRODUCT" => V::Num(self.numbers(args)?.iter().product()),
            "AVERAGE" => {
                let v = self.numbers(args)?;
                if v.is_empty() {
                    return Err("#DIV/0!");
                }
                V::Num(v.iter().sum::<f64>() / v.len() as f64)
            }
            "MIN" => V::Num(self.numbers(args)?.into_iter().reduce(f64::min).unwrap_or(0.0)),
            "MAX" => V::Num(self.numbers(args)?.into_iter().reduce(f64::max).unwrap_or(0.0)),
            "MEDIAN" => {
                let mut v = self.numbers(args)?;
                if v.is_empty() {
                    return Err("#NUM!");
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let m = v.len() / 2;
                V::Num(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 })
            }
            "LARGE" | "SMALL" => {
                let mut v = self.numbers(&args[..1.min(args.len())])?;
                let k = n(1)? as usize;
                v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                if name == "LARGE" {
                    v.reverse();
                }
                V::Num(*v.get(k.checked_sub(1).ok_or("#NUM!")?).ok_or("#NUM!")?)
            }
            "RANK" | "RANK.EQ" => {
                let x = n(0)?;
                let v = self.numbers(&args[1..2.min(args.len())])?;
                let asc = self.num_arg(args, 2, Some(0.0))? != 0.0;
                if !v.contains(&x) {
                    return Err("#N/A");
                }
                V::Num(1.0 + v.iter().filter(|y| if asc { **y < x } else { **y > x }).count() as f64)
            }
            "COUNT" => V::Num(self.flat(args).iter().filter(|(v, refs)| matches!(v, V::Num(_)) || (!refs && v.num().is_ok() && !matches!(v, V::Blank))).count() as f64),
            "COUNTA" => V::Num(self.flat(args).iter().filter(|(v, _)| !matches!(v, V::Blank)).count() as f64),
            "COUNTBLANK" => V::Num(self.flat(args).iter().filter(|(v, _)| matches!(v, V::Blank) || *v == V::Text(String::new())).count() as f64),
            "AND" | "OR" | "XOR" => {
                let mut vals = vec![];
                for (v, refs) in self.flat(args) {
                    match v {
                        V::Err(e) => return Err(e),
                        V::Blank => {}
                        V::Text(_) if refs => {}
                        v => vals.push(v.truth()?),
                    }
                }
                if vals.is_empty() {
                    return Err("#VALUE!");
                }
                V::Bool(match name {
                    "AND" => vals.iter().all(|b| *b),
                    "OR" => vals.iter().any(|b| *b),
                    _ => vals.iter().filter(|b| **b).count() % 2 == 1,
                })
            }
            "NOT" => V::Bool(!self.scalar(args.first().ok_or("#VALUE!")?).truth()?),
            "TRUE" => V::Bool(true),
            "FALSE" => V::Bool(false),
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
                let (x, d) = (n(0)?, self.num_arg(args, 1, Some(0.0))?.trunc());
                let m = 10f64.powf(d);
                // nudge away the binary representation error (2.675 -> 2.68 like Excel)
                let scaled = x * m;
                let scaled = if (scaled - scaled.round()).abs() < 1e-9 { scaled.round() } else { scaled + scaled.signum() * 1e-9 };
                V::Num(match name {
                    "ROUND" => scaled.abs().round() * scaled.signum(),
                    "ROUNDUP" => scaled.abs().ceil() * scaled.signum(),
                    _ => scaled.trunc(),
                } / m)
            }
            "INT" => V::Num(n(0)?.floor()),
            "TRUNC" => {
                let m = 10f64.powf(self.num_arg(args, 1, Some(0.0))?);
                V::Num((n(0)? * m).trunc() / m)
            }
            "ABS" => V::Num(n(0)?.abs()),
            "SIGN" => V::Num(n(0)?.signum() * (n(0)? != 0.0) as u8 as f64),
            "SQRT" => {
                let x = n(0)?;
                if x < 0.0 {
                    return Err("#NUM!");
                }
                V::Num(x.sqrt())
            }
            "POWER" => V::Num(n(0)?.powf(n(1)?)),
            "EXP" => V::Num(n(0)?.exp()),
            "LN" => V::Num(n(0)?.ln()),
            "LOG10" => V::Num(n(0)?.log10()),
            "LOG" => V::Num(n(0)?.log(self.num_arg(args, 1, Some(10.0))?)),
            "PI" => V::Num(std::f64::consts::PI),
            "MOD" => {
                let (a, b) = (n(0)?, n(1)?);
                if b == 0.0 {
                    return Err("#DIV/0!");
                }
                V::Num(a - b * (a / b).floor())
            }
            "QUOTIENT" => {
                let b = n(1)?;
                if b == 0.0 {
                    return Err("#DIV/0!");
                }
                V::Num((n(0)? / b).trunc())
            }
            "CEILING" | "CEILING.MATH" | "FLOOR" | "FLOOR.MATH" | "MROUND" => {
                let x = n(0)?;
                let m = self.num_arg(args, 1, Some(1.0))?.abs();
                if m == 0.0 {
                    return Ok(V::Num(0.0));
                }
                V::Num(match name {
                    "MROUND" => (x / m).round() * m,
                    n if n.starts_with("CEILING") => (x / m).ceil() * m,
                    _ => (x / m).floor() * m,
                })
            }
            "ISBLANK" => V::Bool(matches!(self.scalar(args.first().ok_or("#VALUE!")?), V::Blank)),
            "ISNUMBER" => V::Bool(matches!(self.scalar(args.first().ok_or("#VALUE!")?), V::Num(_))),
            "ISTEXT" => V::Bool(matches!(self.scalar(args.first().ok_or("#VALUE!")?), V::Text(_))),
            "ISNONTEXT" => V::Bool(!matches!(self.scalar(args.first().ok_or("#VALUE!")?), V::Text(_))),
            "ISLOGICAL" => V::Bool(matches!(self.scalar(args.first().ok_or("#VALUE!")?), V::Bool(_))),
            "ISERROR" | "ISERR" | "ISNA" => {
                let v = self.scalar(args.first().ok_or("#VALUE!")?);
                match v {
                    V::Err(UNSUPPORTED) => return Err(UNSUPPORTED),
                    V::Err(e) => V::Bool(match name {
                        "ISNA" => e == "#N/A",
                        "ISERR" => e != "#N/A",
                        _ => true,
                    }),
                    _ => V::Bool(false),
                }
            }
            "ISEVEN" => V::Bool(n(0)?.trunc() as i64 % 2 == 0),
            "ISODD" => V::Bool(n(0)?.trunc() as i64 % 2 != 0),
            "NA" => return Err("#N/A"),
            "N" => match self.scalar(args.first().ok_or("#VALUE!")?) {
                V::Num(x) => V::Num(x),
                V::Bool(b) => V::Num(b as u8 as f64),
                V::Err(e) => return Err(e),
                _ => V::Num(0.0),
            },
            "LEN" => V::Num(t(0)?.chars().count() as f64),
            "LEFT" | "RIGHT" => {
                let s: Vec<char> = t(0)?.chars().collect();
                let k = (self.num_arg(args, 1, Some(1.0))?.max(0.0) as usize).min(s.len());
                V::Text(if name == "LEFT" { s[..k].iter().collect() } else { s[s.len() - k..].iter().collect() })
            }
            "MID" => {
                let s: Vec<char> = t(0)?.chars().collect();
                let start = (n(1)?.max(1.0) as usize - 1).min(s.len());
                let k = (n(2)?.max(0.0) as usize).min(s.len() - start);
                V::Text(s[start..start + k].iter().collect())
            }
            "UPPER" => V::Text(t(0)?.to_uppercase()),
            "LOWER" => V::Text(t(0)?.to_lowercase()),
            "PROPER" => {
                let mut out = String::new();
                let mut start = true;
                for c in t(0)?.chars() {
                    out.extend(if start { c.to_uppercase().collect::<Vec<_>>() } else { c.to_lowercase().collect() });
                    start = !c.is_alphabetic();
                }
                V::Text(out)
            }
            "TRIM" => V::Text(t(0)?.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")),
            "CLEAN" => V::Text(t(0)?.chars().filter(|c| (*c as u32) >= 32).collect()),
            "CONCATENATE" | "CONCAT" => {
                let mut s = String::new();
                for (v, _) in self.flat(args) {
                    s.push_str(&v.text()?);
                }
                V::Text(s)
            }
            "TEXTJOIN" => {
                let delim = t(0)?;
                let skip = self.scalar(args.get(1).ok_or("#VALUE!")?).truth()?;
                let mut parts = vec![];
                for (v, _) in self.flat(args.get(2..).unwrap_or(&[])) {
                    let s = v.text()?;
                    if !(skip && s.is_empty()) {
                        parts.push(s);
                    }
                }
                V::Text(parts.join(&delim))
            }
            "REPT" => V::Text(t(0)?.repeat(n(1)?.clamp(0.0, 32767.0) as usize)),
            "EXACT" => V::Bool(t(0)? == t(1)?),
            "SUBSTITUTE" => {
                let (s, old, new) = (t(0)?, t(1)?, t(2)?);
                if old.is_empty() {
                    return Ok(V::Text(s));
                }
                match args.get(3) {
                    Some(_) => {
                        let k = n(3)? as usize;
                        match s.match_indices(&old).nth(k.saturating_sub(1)) {
                            Some((i, _)) if k >= 1 => V::Text(format!("{}{}{}", &s[..i], new, &s[i + old.len()..])),
                            _ => V::Text(s),
                        }
                    }
                    None => V::Text(s.replace(&old, &new)),
                }
            }
            "FIND" | "SEARCH" => {
                let (needle, hay) = (t(0)?, t(1)?);
                let start = self.num_arg(args, 2, Some(1.0))?.max(1.0) as usize - 1;
                let hay_chars: Vec<char> = hay.chars().collect();
                let tail: String = hay_chars.get(start..).ok_or("#VALUE!")?.iter().collect();
                let at = if name == "FIND" { tail.find(&needle) } else { tail.to_lowercase().find(&needle.to_lowercase()) };
                let at = at.ok_or("#VALUE!")?;
                V::Num((start + tail[..at].chars().count() + 1) as f64)
            }
            "VALUE" | "NUMBERVALUE" => match self.scalar(args.first().ok_or("#VALUE!")?) {
                V::Num(x) => V::Num(x),
                V::Text(s) => V::Num(parse_number(&s.replace(',', "")).ok_or("#VALUE!")?),
                V::Err(e) => return Err(e),
                _ => return Err("#VALUE!"),
            },
            "TEXT" => {
                let v = self.scalar(args.first().ok_or("#VALUE!")?);
                let code = t(1)?;
                match v {
                    V::Num(x) => V::Text(visible(&numfmt::format(&Value::Number(x), &code, self.date1904, self.sep).text)),
                    V::Err(e) => return Err(e),
                    other => V::Text(other.text()?),
                }
            }
            "FIXED" => {
                let d = self.num_arg(args, 1, Some(2.0))?.max(0.0) as usize;
                let code = if d == 0 { "#,##0".to_string() } else { format!("#,##0.{}", "0".repeat(d)) };
                V::Text(visible(&numfmt::format(&Value::Number(n(0)?), &code, false, self.sep).text))
            }
            "CHAR" => V::Text(char::from_u32(n(0)? as u32).map(String::from).unwrap_or_default()),
            "TODAY" => V::Num(today(self.date1904).floor()),
            "NOW" => V::Num(today(self.date1904)),
            "DATE" => V::Num(date_serial(n(0)? as i64, n(1)? as i64, n(2)? as i64, self.date1904).ok_or("#NUM!")?),
            "YEAR" | "MONTH" | "DAY" => {
                let (y, m, d) = serial_date(n(0)?, self.date1904).ok_or("#NUM!")?;
                V::Num(match name {
                    "YEAR" => y,
                    "MONTH" => m,
                    _ => d,
                } as f64)
            }
            "WEEKDAY" => {
                let days = n(0)?.floor() as i64 + if self.date1904 { 1462 } else { 0 };
                // serial 1 (1900-01-01) was a Sunday in Excel's calendar
                let sunday_first = (days - 1).rem_euclid(7) + 1;
                V::Num(match self.num_arg(args, 1, Some(1.0))? as i64 {
                    2 => (sunday_first + 5) % 7 + 1,
                    3 => (sunday_first + 5) % 7,
                    _ => sunday_first,
                } as f64)
            }
            "EDATE" | "EOMONTH" => {
                let (y, m, d) = serial_date(n(0)?, self.date1904).ok_or("#NUM!")?;
                let k = n(1)?.trunc() as i64;
                let total = y * 12 + (m - 1) + k;
                let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
                let last = days_in_month(ny, nm);
                let day = if name == "EOMONTH" { last } else { d.min(last) };
                V::Num(date_serial(ny, nm, day, self.date1904).ok_or("#NUM!")?)
            }
            "DAYS" => V::Num((n(0)?.floor() - n(1)?.floor()).trunc()),
            "DATEDIF" => {
                let (a, b) = (n(0)?.floor(), n(1)?.floor());
                if a > b {
                    return Err("#NUM!");
                }
                let (ya, ma, da) = serial_date(a, self.date1904).ok_or("#NUM!")?;
                let (yb, mb, db) = serial_date(b, self.date1904).ok_or("#NUM!")?;
                let months = (yb - ya) * 12 + (mb - ma) - (db < da) as i64;
                V::Num(match t(2)?.to_uppercase().as_str() {
                    "D" => b - a,
                    "M" => months as f64,
                    "Y" => (months / 12) as f64,
                    "YM" => (months % 12) as f64,
                    _ => return Err(UNSUPPORTED),
                })
            }
            _ => return Err(UNSUPPORTED),
        })
    }
}
