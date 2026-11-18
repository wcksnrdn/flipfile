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

// Parsing, evaluation and the workbook around them. The submodules share one namespace
// (each starts with `use super::*`), as when this was a single file.
mod book;
mod dates;
mod eval;
mod functions;
mod ops;
mod parse;
mod scalar_functions;

#[cfg(test)]
mod tests;

use dates::*;
use eval::*;
use ops::*;

// What the rest of `office` uses
pub use book::{Book, Pending};
pub use dates::{serial, today_serial, weekday0, year_month};
pub use eval::{Cells, Eval};
pub use ops::compare_values;
pub use parse::{parse, Area, Expr};

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
