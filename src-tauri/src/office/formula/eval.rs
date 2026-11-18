//! Evaluation

use super::*;

/// What a formula reads: the workbook's cells
pub trait Cells {
    fn get(&self, sheet: usize, r: u32, c: u32) -> V;
    fn sheet_index(&self, name: &str) -> Option<usize>;
    /// last used (row, column) of a sheet, to bound whole-column references
    fn extent(&self, sheet: usize) -> (u32, u32);
}

/// A value, or a block of them (rows × columns, row by row)
#[derive(Clone, Debug)]
pub(super) enum A {
    S(V),
    M(usize, usize, Vec<V>),
}

impl A {
    pub(super) fn values(&self) -> Vec<&V> {
        match self {
            A::S(v) => vec![v],
            A::M(_, _, vs) => vs.iter().collect(),
        }
    }
    pub(super) fn dims(&self) -> (usize, usize) {
        match self {
            A::S(_) => (1, 1),
            A::M(r, c, _) => (*r, *c),
        }
    }
    pub(super) fn at(&self, r: usize, c: usize) -> V {
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

    pub(super) fn scalar(&self, e: &Expr) -> V {
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

    pub(super) fn eval(&self, e: &Expr) -> A {
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
    pub(super) fn flat(&self, args: &[Expr]) -> Vec<(V, bool)> {
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
    pub(super) fn numbers(&self, args: &[Expr]) -> Result<Vec<f64>, &'static str> {
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

    pub(super) fn num_arg(&self, args: &[Expr], i: usize, default: Option<f64>) -> Result<f64, &'static str> {
        match args.get(i) {
            Some(Expr::Lit(V::Blank)) | None => default.ok_or("#VALUE!"),
            Some(e) => self.scalar(e).num(),
        }
    }

    pub(super) fn text_arg(&self, args: &[Expr], i: usize) -> Result<String, &'static str> {
        args.get(i).map(|e| self.scalar(e).text()).unwrap_or(Ok(String::new()))
    }
}
