//! Unit tests of parsing and evaluation

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
