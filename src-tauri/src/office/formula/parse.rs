//! Parsing

use super::*;

/// A cell or a block of cells; `abs` marks the `$` parts (row1, col1, row2, col2) that
/// stay put when the formula is copied elsewhere
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub sheet: Option<String>,
    pub r1: u32,
    pub c1: u32,
    pub r2: u32,
    pub c2: u32,
    pub(super) abs: [bool; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Lit(V),
    Area(Area),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    /// whatever this engine doesn't know (names, structured references, …)
    Unsupported,
}

impl Expr {
    /// The formula as it would read `dr` rows and `dc` columns further on (shared
    /// formulas, conditional formats applied over a range)
    pub fn shifted(&self, dr: i64, dc: i64) -> Expr {
        let mv = |v: u32, d: i64, abs: bool, max: u32| if abs { v } else { (v as i64 + d).clamp(0, max as i64) as u32 };
        match self {
            Expr::Area(a) => {
                let (r1, r2) = (mv(a.r1, dr, a.abs[0], MAX_ROW), mv(a.r2, dr, a.abs[2], MAX_ROW));
                let (c1, c2) = (mv(a.c1, dc, a.abs[1], MAX_COL), mv(a.c2, dc, a.abs[3], MAX_COL));
                // $A1:B$2 moved down: the corners may swap
                let (r1, r2, ar1, ar2) = if r1 <= r2 { (r1, r2, a.abs[0], a.abs[2]) } else { (r2, r1, a.abs[2], a.abs[0]) };
                let (c1, c2, ac1, ac2) = if c1 <= c2 { (c1, c2, a.abs[1], a.abs[3]) } else { (c2, c1, a.abs[3], a.abs[1]) };
                Expr::Area(Area { sheet: a.sheet.clone(), r1, c1, r2, c2, abs: [ar1, ac1, ar2, ac2] })
            }
            Expr::Neg(e) => Expr::Neg(Box::new(e.shifted(dr, dc))),
            Expr::Pct(e) => Expr::Pct(Box::new(e.shifted(dr, dc))),
            Expr::Bin(op, a, b) => Expr::Bin(op, Box::new(a.shifted(dr, dc)), Box::new(b.shifted(dr, dc))),
            Expr::Call(f, args) => Expr::Call(f.clone(), args.iter().map(|a| a.shifted(dr, dc)).collect()),
            other => other.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Lit(V),
    Area(Area),
    Func(String),
    Op(&'static str),
    Open,
    Close,
    Comma,
    Unsupported,
}

/// "AB" -> 27 (zero-based column)
fn col_number(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 3 || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let c = s.chars().fold(0u32, |acc, c| acc * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1)) - 1;
    (c <= MAX_COL).then_some(c)
}

/// "$B$12" -> (row, col, row absolute, col absolute)
fn cell_part(s: &str) -> Option<(u32, u32, bool, bool)> {
    let (abs_c, s) = s.strip_prefix('$').map(|r| (true, r)).unwrap_or((false, s));
    let letters = s.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let col = col_number(&s[..letters])?;
    let rest = &s[letters..];
    let (abs_r, digits) = rest.strip_prefix('$').map(|r| (true, r)).unwrap_or((false, rest));
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let row: u32 = digits.parse().ok()?;
    (1..=MAX_ROW + 1).contains(&row).then_some((row - 1, col, abs_r, abs_c))
}

fn tokens(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    let word_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '$');
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\n' | '\r' | '\t' => i += 1,
            '"' => {
                let mut s = String::new();
                i += 1;
                while i < chars.len() {
                    if chars[i] == '"' {
                        if chars.get(i + 1) == Some(&'"') {
                            s.push('"');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                i += 1;
                out.push(Tok::Str(s));
            }
            '#' => {
                let rest: String = chars[i..(i + 14).min(chars.len())].iter().collect::<String>().to_uppercase();
                match ["#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#REF!", "#VALUE!"].iter().find(|k| rest.starts_with(**k)) {
                    Some(k) => {
                        i += k.len();
                        out.push(Tok::Lit(V::Err(error_name(k))));
                    }
                    None => {
                        i += 1;
                        out.push(Tok::Unsupported);
                    }
                }
            }
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            ',' | ';' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '{' | '[' | '@' => {
                out.push(Tok::Unsupported);
                i += 1;
            }
            '<' | '>' => {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let op = match two.as_str() {
                    "<=" => "<=",
                    ">=" => ">=",
                    "<>" => "<>",
                    _ => {
                        if c == '<' { "<" } else { ">" }
                    }
                };
                i += op.len();
                out.push(Tok::Op(op));
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '=' | '%' | ':' => {
                out.push(Tok::Op(match c {
                    '+' => "+",
                    '-' => "-",
                    '*' => "*",
                    '/' => "/",
                    '^' => "^",
                    '&' => "&",
                    '=' => "=",
                    '%' => "%",
                    _ => ":",
                }));
                i += 1;
            }
            '0'..='9' | '.' if !matches!(out.last(), Some(Tok::Op(":"))) => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                if i < chars.len() && matches!(chars[i], 'e' | 'E') && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit() || matches!(c, '+' | '-')) {
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let s: String = chars[start..i].iter().collect();
                out.push(s.parse().map(Tok::Num).unwrap_or(Tok::Unsupported));
            }
            '\'' => {
                // 'Sheet name'!A1
                let mut name = String::new();
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            name.push('\'');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    name.push(chars[i]);
                    i += 1;
                }
                i += 1;
                if chars.get(i) == Some(&'!') {
                    i += 1;
                    let start = i;
                    while i < chars.len() && (word_char(chars[i]) || chars[i] == ':') {
                        i += 1;
                    }
                    let r: String = chars[start..i].iter().collect();
                    out.push(area(Some(name), &r).map(Tok::Area).unwrap_or(Tok::Unsupported));
                } else {
                    out.push(Tok::Unsupported);
                }
            }
            c if word_char(c) => {
                let start = i;
                while i < chars.len() && word_char(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if chars.get(i) == Some(&'(') {
                    let name = word.to_uppercase();
                    let name = name.trim_start_matches("_XLFN.").trim_start_matches("_XLWS.").to_string();
                    out.push(Tok::Func(name));
                } else if chars.get(i) == Some(&'!') {
                    i += 1;
                    let start = i;
                    while i < chars.len() && (word_char(chars[i]) || chars[i] == ':') {
                        i += 1;
                    }
                    let r: String = chars[start..i].iter().collect();
                    out.push(area(Some(word), &r).map(Tok::Area).unwrap_or(Tok::Unsupported));
                } else if word.eq_ignore_ascii_case("TRUE") {
                    out.push(Tok::Lit(V::Bool(true)));
                } else if word.eq_ignore_ascii_case("FALSE") {
                    out.push(Tok::Lit(V::Bool(false)));
                } else {
                    // A1, or A1:B2 / A:C (the second half follows a ':')
                    let mut text = word.clone();
                    if chars.get(i) == Some(&':') {
                        let mut j = i + 1;
                        while j < chars.len() && word_char(chars[j]) {
                            j += 1;
                        }
                        let second: String = chars[i + 1..j].iter().collect();
                        let joined = format!("{word}:{second}");
                        if area(None, &joined).is_some() {
                            text = joined;
                            i = j;
                        }
                    }
                    out.push(area(None, &text).map(Tok::Area).unwrap_or(Tok::Unsupported));
                }
            }
            _ => {
                out.push(Tok::Unsupported);
                i += 1;
            }
        }
    }
    out
}

/// "A1", "$A$1:B9", "A:C" or "3:5" on a sheet
pub(super) fn area(sheet: Option<String>, s: &str) -> Option<Area> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    if let (Some(x), Some(y)) = (cell_part(a), cell_part(b)) {
        let (r1, r2, ar1, ar2) = if x.0 <= y.0 { (x.0, y.0, x.2, y.2) } else { (y.0, x.0, y.2, x.2) };
        let (c1, c2, ac1, ac2) = if x.1 <= y.1 { (x.1, y.1, x.3, y.3) } else { (y.1, x.1, y.3, x.3) };
        return Some(Area { sheet, r1, c1, r2, c2, abs: [ar1, ac1, ar2, ac2] });
    }
    if !s.contains(':') {
        return None;
    }
    let strip = |p: &str| (p.starts_with('$'), p.trim_start_matches('$').to_string());
    let ((aa, a), (ab, b)) = (strip(a), strip(b));
    if let (Some(c1), Some(c2)) = (col_number(&a), col_number(&b)) {
        return Some(Area { sheet, r1: 0, c1: c1.min(c2), r2: MAX_ROW, c2: c1.max(c2), abs: [true, aa, true, ab] });
    }
    if let (Ok(r1), Ok(r2)) = (a.parse::<u32>(), b.parse::<u32>())
        && r1 >= 1 && r2 >= 1 {
            return Some(Area { sheet, r1: r1.min(r2) - 1, c1: 0, r2: r1.max(r2) - 1, c2: MAX_COL, abs: [aa, true, ab, true] });
        }
    None
}

pub fn parse(formula: &str) -> Expr {
    let toks = tokens(formula.trim_start_matches('='));
    let mut p = Parser { toks, at: 0 };
    let e = p.compare();
    if p.at < p.toks.len() { Expr::Unsupported } else { e }
}

struct Parser {
    pub(super) toks: Vec<Tok>,
    pub(super) at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        if let Some(Tok::Op(o)) = self.peek()
            && let Some(found) = ops.iter().find(|x| *x == o) {
                self.at += 1;
                return Some(found);
            }
        None
    }
    fn binary(&mut self, ops: &[&'static str], next: fn(&mut Parser) -> Expr) -> Expr {
        let mut e = next(self);
        while let Some(op) = self.op(ops) {
            let rhs = next(self);
            e = Expr::Bin(op, Box::new(e), Box::new(rhs));
        }
        e
    }
    fn compare(&mut self) -> Expr {
        self.binary(&["=", "<>", "<", ">", "<=", ">="], Parser::concat)
    }
    fn concat(&mut self) -> Expr {
        self.binary(&["&"], Parser::add)
    }
    fn add(&mut self) -> Expr {
        self.binary(&["+", "-"], Parser::mul)
    }
    fn mul(&mut self) -> Expr {
        self.binary(&["*", "/"], Parser::pow)
    }
    fn pow(&mut self) -> Expr {
        self.binary(&["^"], Parser::unary)
    }
    fn unary(&mut self) -> Expr {
        match self.op(&["-", "+"]) {
            Some("-") => Expr::Neg(Box::new(self.unary())),
            Some(_) => self.unary(),
            None => self.postfix(),
        }
    }
    fn postfix(&mut self) -> Expr {
        let mut e = self.primary();
        while self.op(&["%"]).is_some() {
            e = Expr::Pct(Box::new(e));
        }
        e
    }
    fn primary(&mut self) -> Expr {
        let Some(t) = self.toks.get(self.at).cloned() else { return Expr::Unsupported };
        self.at += 1;
        match t {
            Tok::Num(n) => Expr::Lit(V::Num(n)),
            Tok::Str(s) => Expr::Lit(V::Text(s)),
            Tok::Lit(v) => Expr::Lit(v),
            Tok::Area(a) => Expr::Area(a),
            Tok::Open => {
                let e = self.compare();
                if self.peek() == Some(&Tok::Close) {
                    self.at += 1;
                    e
                } else {
                    Expr::Unsupported
                }
            }
            Tok::Func(name) => {
                if self.peek() != Some(&Tok::Open) {
                    return Expr::Unsupported;
                }
                self.at += 1;
                let mut args = vec![];
                if self.peek() == Some(&Tok::Close) {
                    self.at += 1;
                    return Expr::Call(name, args);
                }
                loop {
                    // an empty argument, as in IF(A1,,1)
                    if matches!(self.peek(), Some(Tok::Comma) | Some(Tok::Close)) {
                        args.push(Expr::Lit(V::Blank));
                    } else {
                        args.push(self.compare());
                    }
                    match self.peek() {
                        Some(Tok::Comma) => self.at += 1,
                        Some(Tok::Close) => {
                            self.at += 1;
                            return Expr::Call(name, args);
                        }
                        _ => return Expr::Unsupported,
                    }
                }
            }
            _ => Expr::Unsupported,
        }
    }
}
