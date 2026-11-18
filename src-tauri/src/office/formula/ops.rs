//! Operators, comparison, matching and criteria (as COUNTIF and MATCH apply them)

use super::*;

pub(super) fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut hidden = false;
    for c in s.chars() {
        match c {
            numfmt::HIDE_ON => hidden = true,
            numfmt::HIDE_OFF => hidden = false,
            c if !hidden => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

pub(super) fn map1(a: A, f: impl Fn(&V) -> V) -> A {
    match a {
        A::S(v) => A::S(f(&v)),
        A::M(r, c, vs) => A::M(r, c, vs.iter().map(f).collect()),
    }
}

pub(super) fn map2(a: A, b: A, f: impl Fn(&V, &V) -> V) -> A {
    match (&a, &b) {
        (A::S(x), A::S(y)) => A::S(f(x, y)),
        _ => {
            let (ra, ca) = a.dims();
            let (rb, cb) = b.dims();
            let (rows, cols) = (ra.max(rb), ca.max(cb));
            let vs = (0..rows * cols).map(|i| f(&a.at(i / cols, i % cols), &b.at(i / cols, i % cols))).collect();
            A::M(rows, cols, vs)
        }
    }
}

pub(super) fn binary(op: &str, a: &V, b: &V) -> V {
    if let V::Err(e) = a {
        return V::Err(e);
    }
    if let V::Err(e) = b {
        return V::Err(e);
    }
    let num = |f: fn(f64, f64) -> Option<f64>| match (a.num(), b.num()) {
        (Ok(x), Ok(y)) => f(x, y).map(V::Num).unwrap_or(V::Err("#DIV/0!")),
        (Err(e), _) | (_, Err(e)) => V::Err(e),
    };
    match op {
        "+" => num(|x, y| Some(x + y)),
        "-" => num(|x, y| Some(x - y)),
        "*" => num(|x, y| Some(x * y)),
        "/" => num(|x, y| (y != 0.0).then(|| x / y)),
        "^" => num(|x, y| Some(x.powf(y))),
        "&" => match (a.text(), b.text()) {
            (Ok(x), Ok(y)) => V::Text(x + &y),
            (Err(e), _) | (_, Err(e)) => V::Err(e),
        },
        _ => {
            let o = compare(a, b);
            V::Bool(match op {
                "=" => o == std::cmp::Ordering::Equal,
                "<>" => o != std::cmp::Ordering::Equal,
                "<" => o == std::cmp::Ordering::Less,
                ">" => o == std::cmp::Ordering::Greater,
                "<=" => o != std::cmp::Ordering::Greater,
                _ => o != std::cmp::Ordering::Less,
            })
        }
    }
}

/// Excel's ordering: numbers < text < logical values; text ignores case; a blank is
/// 0, "" or FALSE, whichever it is compared with
pub(super) fn compare(a: &V, b: &V) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let rank = |v: &V| match v {
        V::Num(_) => 0,
        V::Text(_) => 1,
        V::Bool(_) => 2,
        _ => 3,
    };
    let (a, b) = match (a, b) {
        (V::Blank, V::Blank) => return Equal,
        (V::Blank, V::Num(_)) => (&V::Num(0.0), b),
        (V::Num(_), V::Blank) => (a, &V::Num(0.0)),
        (V::Blank, V::Text(_)) => (&V::Text(String::new()) as &V, b),
        (V::Text(_), V::Blank) => (a, &V::Text(String::new()) as &V),
        (V::Blank, V::Bool(_)) => (&V::Bool(false), b),
        (V::Bool(_), V::Blank) => (a, &V::Bool(false)),
        _ => (a, b),
    };
    match (a, b) {
        (V::Num(x), V::Num(y)) => x.partial_cmp(y).unwrap_or(Equal),
        (V::Text(x), V::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (V::Bool(x), V::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// Excel's ordering of two values (conditional formats compare with it)
pub fn compare_values(a: &V, b: &V) -> std::cmp::Ordering {
    compare(a, b)
}

pub(super) fn equal(a: &V, b: &V) -> bool {
    compare(a, b) == std::cmp::Ordering::Equal
}

/// MATCH: 0 exact (with * ? wildcards in text), 1 largest ≤ (sorted ascending), -1
/// smallest ≥ (sorted descending)
pub(super) fn match_in(v: &V, list: &[&V], kind: f64) -> Option<usize> {
    if kind == 0.0 {
        if let V::Text(p) = v
            && p.contains(['*', '?']) {
                return list.iter().position(|x| matches!(x, V::Text(t) if wildcard(p, t)));
            }
        return list.iter().position(|x| !matches!(x, V::Blank) && rank_eq(v, x));
    }
    let mut best = None;
    for (i, x) in list.iter().enumerate() {
        if matches!(x, V::Blank) || std::mem::discriminant(*x) != std::mem::discriminant(v) {
            continue;
        }
        let o = compare(x, v);
        if (kind > 0.0 && o != std::cmp::Ordering::Greater) || (kind < 0.0 && o != std::cmp::Ordering::Less) {
            best = Some(i);
        } else {
            break;
        }
    }
    best
}

fn rank_eq(a: &V, b: &V) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b) && equal(a, b)
}

/// `*` any run, `?` any one character, `~` escapes; case-insensitive
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some('?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some('~') if p.len() > 1 => t.first() == Some(&p[1]) && go(&p[2..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(&p, &t)
}

/// Whether a cell meets a COUNTIF criterion: 5, "apple", ">=10", "<>x", "a*"
pub(super) fn criterion(cell: &V, crit: &V) -> bool {
    let (op, rhs) = match crit {
        V::Text(s) => {
            let ops = ["<=", ">=", "<>", "<", ">", "="];
            match ops.iter().find(|o| s.starts_with(**o)) {
                Some(o) => (*o, s[o.len()..].to_string()),
                None => ("=", s.clone()),
            }
        }
        V::Num(n) => return matches!(cell, V::Num(x) if x == n) || matches!(cell, V::Text(t) if parse_number(t) == Some(*n)),
        V::Bool(b) => return matches!(cell, V::Bool(x) if x == b),
        V::Blank => return matches!(cell, V::Blank),
        V::Err(_) => return cell == crit,
    };
    let target = match parse_number(&rhs) {
        Some(n) => V::Num(n),
        None if rhs.eq_ignore_ascii_case("true") => V::Bool(true),
        None if rhs.eq_ignore_ascii_case("false") => V::Bool(false),
        None => V::Text(rhs.clone()),
    };
    match op {
        "=" | "<>" => {
            let hit = match (&target, cell) {
                (V::Text(p), _) if p.is_empty() => matches!(cell, V::Blank) || *cell == V::Text(String::new()),
                (V::Text(p), V::Text(t)) => wildcard(p, t),
                (V::Text(_), _) => false,
                (V::Num(n), V::Text(t)) => parse_number(t) == Some(*n),
                (_, V::Blank) => false,
                (t, c) => rank_eq(t, c),
            };
            hit == (op == "=")
        }
        _ => {
            // < > compare only like with like
            if std::mem::discriminant(&target) != std::mem::discriminant(cell) {
                return false;
            }
            let o = compare(cell, &target);
            match op {
                "<" => o == std::cmp::Ordering::Less,
                ">" => o == std::cmp::Ordering::Greater,
                "<=" => o != std::cmp::Ordering::Greater,
                _ => o != std::cmp::Ordering::Less,
            }
        }
    }
}
