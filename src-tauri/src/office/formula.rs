// Excel formulas, worked out for the cells whose file stores no result. Excel keeps the
// last result next to every formula and that is what prints, but workbooks written by
// other programs (exports from web apps, ExcelJS, openpyxl, …) often store the formula
// alone, and conditional formats are formulas too.
//
// The common functions are covered (sums, counts and their IF/IFS variants, lookups,
// logic, text, dates, rounding) with Excel's own rules for blanks, text and errors, and
// array arithmetic as SUMPRODUCT needs it. A formula using anything else (INDIRECT,
// structured table references, names, …) is reported as unsupported, and its cell is
// left empty rather than showing a result Excel wouldn't.

use std::collections::HashMap;

use super::numfmt::{self, Separators, Value};

/// The last row and column Excel has (zero-based)
const MAX_ROW: u32 = 1_048_575;
const MAX_COL: u32 = 16_383;

pub const UNSUPPORTED: &str = "unsupported";

/// A value while a formula is worked out
#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Num(f64),
    Text(String),
    Bool(bool),
    Err(&'static str),
    Blank,
}

impl V {
    pub fn from_value(v: Option<&Value>) -> V {
        match v {
            None => V::Blank,
            Some(Value::Number(n)) => V::Num(*n),
            Some(Value::Text(t)) => V::Text(t.clone()),
            Some(Value::Bool(b)) => V::Bool(*b),
            Some(Value::Error(e)) => V::Err(error_name(e)),
        }
    }

    /// A formula's result as a cell value (`None`: unsupported, leave the cell empty)
    pub fn into_value(self) -> Option<Value> {
        Some(match self {
            V::Num(n) if n.is_finite() => Value::Number(n),
            V::Num(_) => Value::Error("#NUM!".into()),
            V::Text(t) => Value::Text(t),
            V::Bool(b) => Value::Bool(b),
            V::Err(UNSUPPORTED) => return None,
            V::Err(e) => Value::Error(e.into()),
            // =A1 of an empty cell shows 0
            V::Blank => Value::Number(0.0),
        })
    }

    fn num(&self) -> Result<f64, &'static str> {
        match self {
            V::Num(n) => Ok(*n),
            V::Bool(b) => Ok(*b as u8 as f64),
            V::Blank => Ok(0.0),
            V::Text(t) => parse_number(t).ok_or("#VALUE!"),
            V::Err(e) => Err(e),
        }
    }

    fn text(&self) -> Result<String, &'static str> {
        match self {
            V::Num(n) => Ok(general(*n)),
            V::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.into()),
            V::Blank => Ok(String::new()),
            V::Text(t) => Ok(t.clone()),
            V::Err(e) => Err(e),
        }
    }

    fn truth(&self) -> Result<bool, &'static str> {
        match self {
            V::Bool(b) => Ok(*b),
            V::Num(n) => Ok(*n != 0.0),
            V::Blank => Ok(false),
            V::Text(t) if t.eq_ignore_ascii_case("true") => Ok(true),
            V::Text(t) if t.eq_ignore_ascii_case("false") => Ok(false),
            V::Text(_) => Err("#VALUE!"),
            V::Err(e) => Err(e),
        }
    }
}

fn error_name(e: &str) -> &'static str {
    match e.trim() {
        "#DIV/0!" => "#DIV/0!",
        "#N/A" => "#N/A",
        "#NAME?" => "#NAME?",
        "#NULL!" => "#NULL!",
        "#NUM!" => "#NUM!",
        "#REF!" => "#REF!",
        _ => "#VALUE!",
    }
}

fn parse_number(t: &str) -> Option<f64> {
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(p) = t.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().map(|n| n / 100.0);
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A number as Excel's General format turns it into text
fn general(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let s = format!("{:.*}", 15usize.saturating_sub(n.abs().log10().max(0.0) as usize + 1), n);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" { "0".into() } else { s }
}

// ---------------------------------------------------------------------------
// Parsing

/// A cell or a block of cells; `abs` marks the `$` parts (row1, col1, row2, col2) that
/// stay put when the formula is copied elsewhere
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub sheet: Option<String>,
    pub r1: u32,
    pub c1: u32,
    pub r2: u32,
    pub c2: u32,
    abs: [bool; 4],
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
fn area(sheet: Option<String>, s: &str) -> Option<Area> {
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
    if let (Ok(r1), Ok(r2)) = (a.parse::<u32>(), b.parse::<u32>()) {
        if r1 >= 1 && r2 >= 1 {
            return Some(Area { sheet, r1: r1.min(r2) - 1, c1: 0, r2: r1.max(r2) - 1, c2: MAX_COL, abs: [aa, true, ab, true] });
        }
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
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        if let Some(Tok::Op(o)) = self.peek() {
            if let Some(found) = ops.iter().find(|x| *x == o) {
                self.at += 1;
                return Some(found);
            }
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

// ---------------------------------------------------------------------------
// Evaluation

/// What a formula reads: the workbook's cells
pub trait Cells {
    fn get(&self, sheet: usize, r: u32, c: u32) -> V;
    fn sheet_index(&self, name: &str) -> Option<usize>;
    /// last used (row, column) of a sheet, to bound whole-column references
    fn extent(&self, sheet: usize) -> (u32, u32);
}

/// A value, or a block of them (rows × columns, row by row)
#[derive(Clone, Debug)]
enum A {
    S(V),
    M(usize, usize, Vec<V>),
}

impl A {
    fn values(&self) -> Vec<&V> {
        match self {
            A::S(v) => vec![v],
            A::M(_, _, vs) => vs.iter().collect(),
        }
    }
    fn dims(&self) -> (usize, usize) {
        match self {
            A::S(_) => (1, 1),
            A::M(r, c, _) => (*r, *c),
        }
    }
    fn at(&self, r: usize, c: usize) -> V {
        match self {
            A::S(v) => v.clone(),
            A::M(rows, cols, vs) => {
                // a single row or column repeats across the other direction
                let r = if *rows == 1 { 0 } else { r };
                let c = if *cols == 1 { 0 } else { c };
                vs.get(r * cols + c).cloned().unwrap_or(V::Err("#N/A"))
            }
        }
    }
}

pub struct Eval<'a> {
    pub cells: &'a dyn Cells,
    pub sheet: usize,
    /// the formula's own cell
    pub here: (u32, u32),
    pub date1904: bool,
    pub sep: Separators,
}

impl Eval<'_> {
    pub fn value(&self, e: &Expr) -> V {
        self.scalar(e)
    }

    fn scalar(&self, e: &Expr) -> V {
        match self.eval(e) {
            A::S(v) => v,
            A::M(rows, cols, vs) => {
                // implicit intersection with the formula's row or column
                if let Expr::Area(a) = e {
                    if cols == 1 && (a.r1..=a.r2).contains(&self.here.0) {
                        return vs.get((self.here.0 - a.r1) as usize).cloned().unwrap_or(V::Err("#VALUE!"));
                    }
                    if rows == 1 && (a.c1..=a.c2).contains(&self.here.1) {
                        return vs.get((self.here.1 - a.c1) as usize).cloned().unwrap_or(V::Err("#VALUE!"));
                    }
                }
                if rows * cols == 1 { vs[0].clone() } else { V::Err("#VALUE!") }
            }
        }
    }

    fn sheet_of(&self, a: &Area) -> Option<usize> {
        match &a.sheet {
            None => Some(self.sheet),
            Some(name) => self.cells.sheet_index(name),
        }
    }

    fn area(&self, a: &Area) -> A {
        let Some(sheet) = self.sheet_of(a) else { return A::S(V::Err("#REF!")) };
        if a.r1 == a.r2 && a.c1 == a.c2 {
            return A::S(self.cells.get(sheet, a.r1, a.c1));
        }
        let (er, ec) = self.cells.extent(sheet);
        let r2 = a.r2.min(er.max(a.r1));
        let c2 = a.c2.min(ec.max(a.c1));
        let (rows, cols) = ((r2 - a.r1 + 1) as usize, (c2 - a.c1 + 1) as usize);
        if rows * cols > 4_000_000 {
            return A::S(V::Err(UNSUPPORTED));
        }
        let mut vs = Vec::with_capacity(rows * cols);
        for r in a.r1..=r2 {
            for c in a.c1..=c2 {
                vs.push(self.cells.get(sheet, r, c));
            }
        }
        A::M(rows, cols, vs)
    }

    fn eval(&self, e: &Expr) -> A {
        match e {
            Expr::Lit(v) => A::S(v.clone()),
            Expr::Area(a) => self.area(a),
            Expr::Unsupported => A::S(V::Err(UNSUPPORTED)),
            Expr::Neg(x) => map1(self.eval(x), |v| v.num().map(|n| V::Num(-n)).unwrap_or_else(V::Err)),
            Expr::Pct(x) => map1(self.eval(x), |v| v.num().map(|n| V::Num(n / 100.0)).unwrap_or_else(V::Err)),
            Expr::Bin(op, a, b) => map2(self.eval(a), self.eval(b), |x, y| binary(op, x, y)),
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    /// Every value of the arguments: scalars as they are, blocks cell by cell. `refs`
    /// tells for each whether it came from cells (where text and blanks are skipped by
    /// SUM and its kind) or was typed in.
    fn flat(&self, args: &[Expr]) -> Vec<(V, bool)> {
        let mut out = vec![];
        for a in args {
            match self.eval(a) {
                A::S(v) => out.push((v, matches!(a, Expr::Area(_)))),
                A::M(_, _, vs) => out.extend(vs.into_iter().map(|v| (v, true))),
            }
        }
        out
    }

    /// The numbers SUM, AVERAGE, MIN, … count, or the first error
    fn numbers(&self, args: &[Expr]) -> Result<Vec<f64>, &'static str> {
        let mut out = vec![];
        for (v, from_cells) in self.flat(args) {
            match v {
                V::Num(n) => out.push(n),
                V::Err(e) => return Err(e),
                V::Bool(b) if !from_cells => out.push(b as u8 as f64),
                V::Text(t) if !from_cells => out.push(parse_number(&t).ok_or("#VALUE!")?),
                _ => {}
            }
        }
        Ok(out)
    }

    fn num_arg(&self, args: &[Expr], i: usize, default: Option<f64>) -> Result<f64, &'static str> {
        match args.get(i) {
            Some(Expr::Lit(V::Blank)) | None => default.ok_or("#VALUE!"),
            Some(e) => self.scalar(e).num(),
        }
    }

    fn text_arg(&self, args: &[Expr], i: usize) -> Result<String, &'static str> {
        args.get(i).map(|e| self.scalar(e).text()).unwrap_or(Ok(String::new()))
    }

    fn call(&self, name: &str, args: &[Expr]) -> A {
        // functions that work on whole blocks or on references themselves
        match name {
            "ROW" | "COLUMN" => {
                let v = match args.first() {
                    None => if name == "ROW" { self.here.0 } else { self.here.1 },
                    Some(Expr::Area(a)) => if name == "ROW" { a.r1 } else { a.c1 },
                    Some(_) => return A::S(V::Err("#VALUE!")),
                };
                return A::S(V::Num(v as f64 + 1.0));
            }
            "ROWS" | "COLUMNS" => {
                let Some(a) = args.first() else { return A::S(V::Err("#VALUE!")) };
                let (r, c) = match a {
                    Expr::Area(a) => ((a.r2 - a.r1 + 1) as usize, (a.c2 - a.c1 + 1) as usize),
                    e => self.eval(e).dims(),
                };
                return A::S(V::Num(if name == "ROWS" { r } else { c } as f64));
            }
            "IF" => {
                let cond = match args.first() {
                    Some(e) => self.eval(e),
                    None => return A::S(V::Err("#VALUE!")),
                };
                // only the branch taken is worked out (the other may be an error)
                return match cond {
                    A::S(v) => match v.truth() {
                        Ok(true) => args.get(1).map(|e| self.eval(e)).unwrap_or(A::S(V::Bool(true))),
                        Ok(false) => args.get(2).map(|e| self.eval(e)).unwrap_or(A::S(V::Bool(false))),
                        Err(e) => A::S(V::Err(e)),
                    },
                    m => {
                        let yes = args.get(1).map(|e| self.eval(e)).unwrap_or(A::S(V::Bool(true)));
                        let no = args.get(2).map(|e| self.eval(e)).unwrap_or(A::S(V::Bool(false)));
                        let (rows, cols) = m.dims();
                        let vs = (0..rows * cols).map(|i| {
                            let (r, c) = (i / cols, i % cols);
                            match m.at(r, c).truth() {
                                Ok(true) => yes.at(r, c),
                                Ok(false) => no.at(r, c),
                                Err(e) => V::Err(e),
                            }
                        }).collect();
                        A::M(rows, cols, vs)
                    }
                };
            }
            "IFERROR" | "IFNA" => {
                let v = args.first().map(|e| self.scalar(e)).unwrap_or(V::Blank);
                let caught = match &v {
                    V::Err(UNSUPPORTED) => false,
                    V::Err(e) => name == "IFERROR" || *e == "#N/A",
                    _ => false,
                };
                return A::S(if caught { args.get(1).map(|e| self.scalar(e)).unwrap_or(V::Blank) } else { v });
            }
            "IFS" => {
                for pair in args.chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    match self.scalar(&pair[0]).truth() {
                        Ok(true) => return A::S(self.scalar(&pair[1])),
                        Ok(false) => {}
                        Err(e) => return A::S(V::Err(e)),
                    }
                }
                return A::S(V::Err("#N/A"));
            }
            "SWITCH" => {
                let Some(first) = args.first() else { return A::S(V::Err("#VALUE!")) };
                let v = self.scalar(first);
                let rest = &args[1..];
                for pair in rest.chunks(2) {
                    if pair.len() == 1 {
                        return A::S(self.scalar(&pair[0]));
                    }
                    if equal(&v, &self.scalar(&pair[0])) {
                        return A::S(self.scalar(&pair[1]));
                    }
                }
                return A::S(V::Err("#N/A"));
            }
            "CHOOSE" => {
                let n = match self.num_arg(args, 0, None) {
                    Ok(n) => n as usize,
                    Err(e) => return A::S(V::Err(e)),
                };
                return args.get(n).filter(|_| n >= 1).map(|e| self.eval(e)).unwrap_or(A::S(V::Err("#VALUE!")));
            }
            "SUMPRODUCT" => {
                let blocks: Vec<A> = args.iter().map(|e| self.eval(e)).collect();
                let Some(first) = blocks.first() else { return A::S(V::Err("#VALUE!")) };
                let (rows, cols) = first.dims();
                if blocks.iter().any(|b| b.dims() != (rows, cols)) {
                    return A::S(V::Err("#VALUE!"));
                }
                let mut total = 0.0;
                for r in 0..rows {
                    for c in 0..cols {
                        let mut p = 1.0;
                        for b in &blocks {
                            match b.at(r, c) {
                                V::Num(n) => p *= n,
                                V::Bool(x) if blocks.len() == 1 => p *= x as u8 as f64,
                                V::Err(e) => return A::S(V::Err(e)),
                                _ => p = 0.0,
                            }
                        }
                        total += p;
                    }
                }
                return A::S(V::Num(total));
            }
            "INDEX" => {
                let Some(e) = args.first() else { return A::S(V::Err("#VALUE!")) };
                let block = self.eval(e);
                let (rows, cols) = block.dims();
                let (r, c) = match (self.num_arg(args, 1, Some(0.0)), self.num_arg(args, 2, Some(0.0))) {
                    (Ok(r), Ok(c)) => (r as usize, c as usize),
                    (Err(e), _) | (_, Err(e)) => return A::S(V::Err(e)),
                };
                // INDEX(column, n) and INDEX(row, n) both pick the n-th
                let (r, c) = if rows == 1 && args.len() == 2 { (1, r) } else { (r, c.max(1)) };
                let r = if rows == 1 && r == 0 { 1 } else { r };
                if r == 0 || c == 0 || r > rows || c > cols {
                    return A::S(V::Err("#REF!"));
                }
                return A::S(block.at(r - 1, c - 1));
            }
            "MATCH" => {
                let v = args.first().map(|e| self.scalar(e)).unwrap_or(V::Blank);
                let list = args.get(1).map(|e| self.eval(e)).unwrap_or(A::S(V::Blank));
                let kind = self.num_arg(args, 2, Some(1.0)).unwrap_or(1.0);
                let vals = list.values();
                return A::S(match_in(&v, &vals, kind).map(|i| V::Num(i as f64 + 1.0)).unwrap_or(V::Err("#N/A")));
            }
            "VLOOKUP" | "HLOOKUP" => {
                let v = args.first().map(|e| self.scalar(e)).unwrap_or(V::Blank);
                let table = args.get(1).map(|e| self.eval(e)).unwrap_or(A::S(V::Blank));
                let n = match self.num_arg(args, 2, None) {
                    Ok(n) if n >= 1.0 => n as usize,
                    Ok(_) => return A::S(V::Err("#VALUE!")),
                    Err(e) => return A::S(V::Err(e)),
                };
                let approx = args.get(3).filter(|e| **e != Expr::Lit(V::Blank)).map(|e| self.scalar(e).truth().unwrap_or(true)).unwrap_or(true);
                let (rows, cols) = table.dims();
                let vertical = name == "VLOOKUP";
                let keys: Vec<V> = if vertical { (0..rows).map(|r| table.at(r, 0)).collect() } else { (0..cols).map(|c| table.at(0, c)).collect() };
                let refs: Vec<&V> = keys.iter().collect();
                let Some(i) = match_in(&v, &refs, if approx { 1.0 } else { 0.0 }) else { return A::S(V::Err("#N/A")) };
                if (vertical && n > cols) || (!vertical && n > rows) {
                    return A::S(V::Err("#REF!"));
                }
                return A::S(if vertical { table.at(i, n - 1) } else { table.at(n - 1, i) });
            }
            "XLOOKUP" => {
                let v = args.first().map(|e| self.scalar(e)).unwrap_or(V::Blank);
                let keys = args.get(1).map(|e| self.eval(e)).unwrap_or(A::S(V::Blank));
                let results = args.get(2).map(|e| self.eval(e)).unwrap_or(A::S(V::Blank));
                let vals = keys.values();
                return A::S(match match_in(&v, &vals, 0.0) {
                    Some(i) => {
                        let (rows, cols) = results.dims();
                        if rows == 1 { results.at(0, i.min(cols.saturating_sub(1))) } else { results.at(i, 0) }
                    }
                    None => args.get(3).map(|e| self.scalar(e)).unwrap_or(V::Err("#N/A")),
                });
            }
            "COUNTIF" | "SUMIF" | "AVERAGEIF" => {
                let range = args.first().map(|e| self.eval(e)).unwrap_or(A::S(V::Blank));
                let crit = args.get(1).map(|e| self.scalar(e)).unwrap_or(V::Blank);
                let sum = args.get(2).map(|e| self.eval(e)).unwrap_or_else(|| range.clone());
                let (rows, cols) = range.dims();
                let (mut n, mut total) = (0.0, 0.0);
                for r in 0..rows {
                    for c in 0..cols {
                        if criterion(&range.at(r, c), &crit) {
                            n += 1.0;
                            if let V::Num(x) = sum.at(r, c) {
                                total += x;
                            }
                        }
                    }
                }
                return A::S(match name {
                    "COUNTIF" => V::Num(n),
                    "SUMIF" => V::Num(total),
                    _ if n == 0.0 => V::Err("#DIV/0!"),
                    _ => V::Num(total / n),
                });
            }
            "COUNTIFS" | "SUMIFS" | "AVERAGEIFS" | "MAXIFS" | "MINIFS" => {
                let (target, pairs) = if name == "COUNTIFS" { (None, args) } else { (args.first().map(|e| self.eval(e)), args.get(1..).unwrap_or(&[])) };
                let conds: Vec<(A, V)> = pairs.chunks(2).filter(|p| p.len() == 2).map(|p| (self.eval(&p[0]), self.scalar(&p[1]))).collect();
                let Some((first, _)) = conds.first() else { return A::S(V::Err("#VALUE!")) };
                let (rows, cols) = first.dims();
                let mut hits = vec![];
                for r in 0..rows {
                    for c in 0..cols {
                        if conds.iter().all(|(range, crit)| criterion(&range.at(r, c), crit)) {
                            hits.push(target.as_ref().map(|t| t.at(r, c)).unwrap_or(V::Blank));
                        }
                    }
                }
                let nums: Vec<f64> = hits.iter().filter_map(|v| if let V::Num(n) = v { Some(*n) } else { None }).collect();
                return A::S(match name {
                    "COUNTIFS" => V::Num(hits.len() as f64),
                    "SUMIFS" => V::Num(nums.iter().sum()),
                    "MAXIFS" => V::Num(nums.iter().copied().fold(f64::MIN, f64::max).max(if nums.is_empty() { 0.0 } else { f64::MIN })),
                    "MINIFS" => V::Num(if nums.is_empty() { 0.0 } else { nums.iter().copied().fold(f64::MAX, f64::min) }),
                    _ if nums.is_empty() => V::Err("#DIV/0!"),
                    _ => V::Num(nums.iter().sum::<f64>() / nums.len() as f64),
                });
            }
            "SUBTOTAL" | "AGGREGATE" => {
                let f = match self.num_arg(args, 0, None) {
                    Ok(f) => f as u32 % 100,
                    Err(e) => return A::S(V::Err(e)),
                };
                let rest = if name == "AGGREGATE" { args.get(2..).unwrap_or(&[]) } else { args.get(1..).unwrap_or(&[]) };
                let inner = match f {
                    1 => "AVERAGE",
                    2 => "COUNT",
                    3 => "COUNTA",
                    4 => "MAX",
                    5 => "MIN",
                    6 => "PRODUCT",
                    9 => "SUM",
                    _ => return A::S(V::Err(UNSUPPORTED)),
                };
                return self.call(inner, rest);
            }
            _ => {}
        }
        A::S(self.scalar_call(name, args).unwrap_or_else(V::Err))
    }

    fn scalar_call(&self, name: &str, args: &[Expr]) -> Result<V, &'static str> {
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
            "REPT" => V::Text(t(0)?.repeat(n(1)?.max(0.0).min(32767.0) as usize)),
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

fn visible(s: &str) -> String {
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

fn map1(a: A, f: impl Fn(&V) -> V) -> A {
    match a {
        A::S(v) => A::S(f(&v)),
        A::M(r, c, vs) => A::M(r, c, vs.iter().map(f).collect()),
    }
}

fn map2(a: A, b: A, f: impl Fn(&V, &V) -> V) -> A {
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

fn binary(op: &str, a: &V, b: &V) -> V {
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
fn compare(a: &V, b: &V) -> std::cmp::Ordering {
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

fn equal(a: &V, b: &V) -> bool {
    compare(a, b) == std::cmp::Ordering::Equal
}

/// MATCH: 0 exact (with * ? wildcards in text), 1 largest ≤ (sorted ascending), -1
/// smallest ≥ (sorted descending)
fn match_in(v: &V, list: &[&V], kind: f64) -> Option<usize> {
    if kind == 0.0 {
        if let V::Text(p) = v {
            if p.contains(['*', '?']) {
                return list.iter().position(|x| matches!(x, V::Text(t) if wildcard(p, t)));
            }
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
fn criterion(cell: &V, crit: &V) -> bool {
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

// ---------------------------------------------------------------------------
// Dates (serial numbers: days since 1899-12-30, or since 1904-01-01)

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// Unix day number of serial 0
fn epoch(date1904: bool) -> i64 {
    if date1904 { days_from_civil(1904, 1, 1) } else { days_from_civil(1899, 12, 30) }
}

fn date_serial(y: i64, m: i64, d: i64, date1904: bool) -> Option<f64> {
    // years 0-1899 count from 1900, as in Excel
    let y = if (0..1900).contains(&y) { y + 1900 } else { y };
    let total = y * 12 + (m - 1);
    let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    let serial = days_from_civil(y, m, 1) + d - 1 - epoch(date1904);
    (serial >= 0).then_some(serial as f64)
}

/// The serial number of a date (1900 date system)
pub fn serial(y: i64, m: i64, d: i64) -> Option<f64> {
    date_serial(y, m, d, false)
}

fn serial_date(serial: f64, date1904: bool) -> Option<(i64, i64, i64)> {
    if !(0.0..3e6).contains(&serial) {
        return None;
    }
    Some(civil_from_days(serial.floor() as i64 + epoch(date1904)))
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)
}

pub fn today_serial(date1904: bool) -> Option<f64> {
    Some(today(date1904))
}

/// (year, month) of a serial date
pub fn year_month(serial: f64, date1904: bool) -> Option<(i64, i64)> {
    serial_date(serial, date1904).map(|(y, m, _)| (y, m))
}

/// Day of the week of a serial date, 0 = Sunday
pub fn weekday0(serial: f64, date1904: bool) -> i64 {
    (serial.floor() as i64 + if date1904 { 1462 } else { 0 } - 1).rem_euclid(7)
}

fn today(date1904: bool) -> f64 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    secs / 86400.0 - epoch(date1904) as f64
}

// ---------------------------------------------------------------------------
// Whole workbooks

/// A formula waiting for its result: its text, and how far it is copied from where it was
/// written (shared formulas)
#[derive(Clone, Debug)]
pub struct Pending {
    pub text: std::sync::Arc<str>,
    pub shift: (i64, i64),
}

/// The values of one workbook's sheets, with formulas worked out on demand (each once,
/// in whatever order they refer to each other)
pub struct Book<'a> {
    pub names: Vec<String>,
    pub values: Vec<&'a std::collections::BTreeMap<(u32, u32), super::xlsx::Cell>>,
    pub pending: Vec<&'a HashMap<(u32, u32), Pending>>,
    pub date1904: bool,
    pub sep: Separators,
    results: std::cell::RefCell<HashMap<(usize, u32, u32), V>>,
    parsed: std::cell::RefCell<HashMap<*const str, std::rc::Rc<Expr>>>,
    busy: std::cell::RefCell<std::collections::HashSet<(usize, u32, u32)>>,
    extents: Vec<(u32, u32)>,
}

impl<'a> Book<'a> {
    pub fn new(names: Vec<String>, values: Vec<&'a std::collections::BTreeMap<(u32, u32), super::xlsx::Cell>>, pending: Vec<&'a HashMap<(u32, u32), Pending>>, date1904: bool) -> Self {
        let extents = values
            .iter()
            .zip(&pending)
            .map(|(cells, p)| {
                let keys = cells.keys().chain(p.keys());
                keys.fold((0, 0), |(r, c), &(kr, kc)| (r.max(kr), c.max(kc)))
            })
            .collect();
        Book {
            names,
            values,
            pending,
            date1904,
            sep: Separators::system(),
            results: Default::default(),
            parsed: Default::default(),
            busy: Default::default(),
            extents,
        }
    }

    /// The result of the formula in a cell
    pub fn result(&self, sheet: usize, r: u32, c: u32) -> V {
        if let Some(v) = self.results.borrow().get(&(sheet, r, c)) {
            return v.clone();
        }
        let Some(p) = self.pending[sheet].get(&(r, c)) else { return V::Blank };
        {
            // a formula that (indirectly) refers to itself, or a chain too deep to follow
            // here (cells are worked out top to bottom, so chains stay short)
            let mut busy = self.busy.borrow_mut();
            if busy.len() > 200 || !busy.insert((sheet, r, c)) {
                return V::Err(UNSUPPORTED);
            }
        }
        let expr = self.expr(p);
        let v = Eval { cells: self, sheet, here: (r, c), date1904: self.date1904, sep: self.sep }.value(&expr);
        self.busy.borrow_mut().remove(&(sheet, r, c));
        self.results.borrow_mut().insert((sheet, r, c), v.clone());
        v
    }

    fn expr(&self, p: &Pending) -> std::rc::Rc<Expr> {
        let key = std::sync::Arc::as_ptr(&p.text);
        let base = self.parsed.borrow().get(&key).cloned();
        let base = base.unwrap_or_else(|| {
            let e = std::rc::Rc::new(parse(&p.text));
            self.parsed.borrow_mut().insert(key, e.clone());
            e
        });
        if p.shift == (0, 0) { base } else { std::rc::Rc::new(base.shifted(p.shift.0, p.shift.1)) }
    }
}

impl Cells for Book<'_> {
    fn get(&self, sheet: usize, r: u32, c: u32) -> V {
        if self.pending.get(sheet).is_some_and(|p| p.contains_key(&(r, c))) {
            return self.result(sheet, r, c);
        }
        V::from_value(self.values.get(sheet).and_then(|s| s.get(&(r, c))).and_then(|c| c.value.as_ref()))
    }
    fn sheet_index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n.eq_ignore_ascii_case(name))
    }
    fn extent(&self, sheet: usize) -> (u32, u32) {
        self.extents.get(sheet).copied().unwrap_or((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Grid(BTreeMap<(u32, u32), V>);
    impl Cells for Grid {
        fn get(&self, _: usize, r: u32, c: u32) -> V {
            self.0.get(&(r, c)).cloned().unwrap_or(V::Blank)
        }
        fn sheet_index(&self, _: &str) -> Option<usize> {
            Some(0)
        }
        fn extent(&self, _: usize) -> (u32, u32) {
            self.0.keys().fold((0, 0), |(r, c), &(kr, kc)| (r.max(kr), c.max(kc)))
        }
    }

    fn grid() -> Grid {
        let mut g = BTreeMap::new();
        // A1:A4 = 10, 20, 30, "x"; B1:B4 = Kopi, Teh, Kopi, Coklat
        for (i, v) in [V::Num(10.0), V::Num(20.0), V::Num(30.0), V::Text("x".into())].into_iter().enumerate() {
            g.insert((i as u32, 0), v);
        }
        for (i, t) in ["Kopi", "Teh", "Kopi", "Coklat"].into_iter().enumerate() {
            g.insert((i as u32, 1), V::Text(t.into()));
        }
        Grid(g)
    }

    fn eval(f: &str) -> V {
        let g = grid();
        Eval { cells: &g, sheet: 0, here: (5, 3), date1904: false, sep: Separators::US }.value(&parse(f))
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("=1+2*3"), V::Num(7.0));
        assert_eq!(eval("=-2^2"), V::Num(4.0));
        assert_eq!(eval("=(1+2)*3&\"!\""), V::Text("9!".into()));
        assert_eq!(eval("=50%"), V::Num(0.5));
        assert_eq!(eval("=1/0"), V::Err("#DIV/0!"));
        assert_eq!(eval("=A1+A2"), V::Num(30.0));
        assert_eq!(eval("=A4+1"), V::Err("#VALUE!"));
        assert_eq!(eval("=Z9+1"), V::Num(1.0));
    }

    #[test]
    fn functions() {
        assert_eq!(eval("=SUM(A1:A4)"), V::Num(60.0));
        assert_eq!(eval("=SUM(A:A)"), V::Num(60.0));
        assert_eq!(eval("=AVERAGE(A1:A3)"), V::Num(20.0));
        assert_eq!(eval("=COUNT(A1:A4)"), V::Num(3.0));
        assert_eq!(eval("=COUNTA(A1:B4)"), V::Num(8.0));
        assert_eq!(eval("=COUNTIF(B1:B4,\"Kopi\")"), V::Num(2.0));
        assert_eq!(eval("=SUMIF(B1:B4,\"Kopi\",A1:A4)"), V::Num(40.0));
        assert_eq!(eval("=COUNTIF(A1:A4,\">15\")"), V::Num(2.0));
        assert_eq!(eval("=SUMIFS(A1:A4,B1:B4,\"K*\",A1:A4,\"<25\")"), V::Num(10.0));
        assert_eq!(eval("=SUMPRODUCT((B1:B3=\"Kopi\")*A1:A3)"), V::Num(40.0));
        assert_eq!(eval("=IF(A1>5,\"big\",1/0)"), V::Text("big".into()));
        assert_eq!(eval("=IFERROR(1/0,\"-\")"), V::Text("-".into()));
        assert_eq!(eval("=VLOOKUP(20,A1:B3,2,FALSE)"), V::Text("Teh".into()));
        assert_eq!(eval("=INDEX(B1:B4,MATCH(30,A1:A4,0))"), V::Text("Kopi".into()));
        assert_eq!(eval("=XLOOKUP(\"Teh\",B1:B4,A1:A4)"), V::Num(20.0));
        assert_eq!(eval("=ROUND(2.675,2)"), V::Num(2.68));
        assert_eq!(eval("=ROW()-1"), V::Num(5.0));
        assert_eq!(eval("=LEFT(\"flipfile\",4)&UPPER(MID(\"abc\",2,1))"), V::Text("flipB".into()));
        assert_eq!(eval("=TEXT(1234.5,\"#,##0.00\")"), V::Text("1,234.50".into()));
        assert_eq!(eval("=SUBTOTAL(109,A1:A3)"), V::Num(60.0));
        assert_eq!(eval("=AND(A1>5,OR(FALSE,B1=\"kopi\"))"), V::Bool(true));
    }

    #[test]
    fn dates() {
        // 2026-11-15 is serial 46341
        assert_eq!(eval("=DATE(2026,11,15)"), V::Num(46341.0));
        assert_eq!(eval("=YEAR(46341)&\"-\"&MONTH(46341)&\"-\"&DAY(46341)"), V::Text("2026-11-15".into()));
        assert_eq!(eval("=EOMONTH(46341,1)"), V::Num(46387.0));
        assert_eq!(eval("=WEEKDAY(46341)"), V::Num(1.0)); // a Sunday
    }

    #[test]
    fn copies_of_a_formula_move_their_references() {
        let e = parse("=SUM($A1:B$2)+C3").shifted(2, 1);
        assert_eq!(e, parse("=SUM($A3:C$2)+D5"));
    }

    #[test]
    fn unknown_things_are_unsupported_not_wrong() {
        assert_eq!(eval("=INDIRECT(\"A1\")"), V::Err(UNSUPPORTED));
        assert_eq!(eval("=Sales[Total]"), V::Err(UNSUPPORTED));
        assert_eq!(eval("=IFERROR(INDIRECT(\"A1\"),0)"), V::Err(UNSUPPORTED));
        assert_eq!(eval("=MyName*2"), V::Err(UNSUPPORTED));
    }
}
