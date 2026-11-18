//! Functions that work on whole blocks or on references themselves (sums, counts,
//! lookups, SUMPRODUCT, …); the rest fall through to scalar_functions.rs

use super::*;

impl Eval<'_> {
    pub(super) fn call(&self, name: &str, args: &[Expr]) -> A {
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
}
