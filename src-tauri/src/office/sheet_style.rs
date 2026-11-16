// The formatting Excel draws on top of a cell's own style: tables formatted with a
// table style (Format as Table: header colours, banded rows, totals row) and
// conditional formats (highlighted values, colour scales, data bars, icon sets).
//
// Neither is stored in the cells. Both are worked out here once the workbook is read
// and folded into the cells' styles, so printing only sees ordinary cells; data bars
// and icons, which are drawn rather than styled, become marks on their cells.
//
// The order is Excel's: a table style only fills in what a cell doesn't set itself, and
// a conditional format overrides both.

use std::collections::HashMap;

use super::formula::{self, Book, Cells, Eval, V};
use super::xlsx::{Borders, Edge, Range, Rgb, Sheet, Style};

/// A differential format: only the parts it sets (conditional formats, table styles)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dxf {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<Rgb>,
    pub fill: Option<Rgb>,
    pub num_fmt: Option<String>,
    /// the outer edges of the area it formats
    pub borders: Borders,
    /// lines between its rows and between its columns
    pub inside_h: Option<Edge>,
    pub inside_v: Option<Edge>,
}

impl Dxf {
    /// Takes the parts `other` sets that this one doesn't yet
    fn fill_in(&mut self, other: &Dxf) {
        self.bold = self.bold.or(other.bold);
        self.italic = self.italic.or(other.italic);
        self.underline = self.underline.or(other.underline);
        self.strike = self.strike.or(other.strike);
        self.color = self.color.or(other.color);
        self.fill = self.fill.or(other.fill);
        if self.num_fmt.is_none() {
            self.num_fmt = other.num_fmt.clone();
        }
        let b = &mut self.borders;
        b.left = b.left.or(other.borders.left);
        b.right = b.right.or(other.borders.right);
        b.top = b.top.or(other.borders.top);
        b.bottom = b.bottom.or(other.borders.bottom);
    }

    /// Lays `other` over this one (what `other` sets wins)
    fn cover(&mut self, other: &Dxf) {
        let mut o = other.clone();
        o.fill_in(self);
        *self = o;
    }
}

/// A table made with Format as Table
#[derive(Clone, Debug)]
pub struct TableDef {
    pub range: Range,
    pub header_rows: u32,
    pub totals_rows: u32,
    pub style: Option<String>,
    pub row_stripes: bool,
    pub col_stripes: bool,
    pub first_col: bool,
    pub last_col: bool,
}

/// A threshold of a colour scale, data bar or icon set
#[derive(Clone, Debug, PartialEq)]
pub struct Cfvo {
    /// min, max, num, percent, percentile, formula, autoMin, autoMax
    pub kind: String,
    pub val: Option<String>,
    pub gte: bool,
}

#[derive(Clone, Debug)]
pub enum CfKind {
    /// cellIs, expression, containsText, top10, aboveAverage, … with their dxf
    Rule,
    Scale(Vec<Cfvo>, Vec<Rgb>),
    Bar(Cfvo, Cfvo, Rgb),
    Icons { set: String, cfvos: Vec<Cfvo>, reverse: bool, show_value: bool },
}

#[derive(Clone, Debug)]
pub struct CfRule {
    pub kind: CfKind,
    /// the rule's type attribute
    pub rule: String,
    pub priority: i32,
    pub dxf: Option<usize>,
    pub operator: Option<String>,
    pub formulas: Vec<String>,
    pub text: Option<String>,
    pub stop: bool,
    pub rank: u32,
    pub percent: bool,
    pub bottom: bool,
    pub above: bool,
    pub equal_average: bool,
    pub time_period: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CondFormat {
    pub ranges: Vec<Range>,
    pub rules: Vec<CfRule>,
}

/// Something drawn in a cell by a conditional format
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    /// a data bar `frac` of the cell wide
    Bar { frac: f32, color: Rgb },
    /// icon `index` (0 = the lowest values) of an icon set
    Icon { set: String, index: usize, of: usize, show_value: bool },
}

/// What a workbook defines for these formats
pub struct Defs<'a> {
    pub dxfs: &'a [Dxf],
    /// custom table styles: name -> (element type, dxf index)
    pub table_styles: &'a HashMap<String, Vec<(String, usize)>>,
    /// dk1, lt1, dk2, lt2, accent1..6
    pub theme: &'a [Rgb],
}

/// Folds table styles and conditional formats into the cells of `sheets`
pub fn apply(sheets: &mut [Sheet], styles: &mut Vec<Style>, defs: &Defs, tables: &[Vec<TableDef>], conds: &[Vec<CondFormat>], date1904: bool) {
    let mut cache: HashMap<(usize, String), usize> = HashMap::new();
    // the Normal style's font: a cell in it counts as not formatted by hand
    let normal = styles.first().map(|s| s.font.clone()).unwrap_or_default();
    for si in 0..sheets.len() {
        let no_tables = tables.get(si).is_none_or(|t| t.is_empty());
        let no_conds = conds.get(si).is_none_or(|c| c.is_empty());
        if no_tables && no_conds {
            continue;
        }
        // table formatting first: conditional formats test the cells' values, not looks
        let mut under: HashMap<(u32, u32), Dxf> = HashMap::new();
        for t in tables.get(si).into_iter().flatten() {
            if let Some(elements) = table_style(t.style.as_deref(), defs) {
                table_overlay(t, &elements, &mut under);
            }
        }
        let (over, marks) = match conds.get(si) {
            Some(cs) if !cs.is_empty() => conditional(sheets, si, cs, defs, date1904),
            _ => (HashMap::new(), HashMap::new()),
        };
        let sheet = &mut sheets[si];
        sheet.marks = marks;
        let mut positions: Vec<(u32, u32)> = under.keys().chain(over.keys()).copied().collect();
        positions.sort_unstable();
        positions.dedup();
        for pos in positions {
            let base = sheet.cells.get(&pos).map(|c| c.style).unwrap_or_else(|| sheet.col_styles.get(&pos.1).copied().unwrap_or(0));
            let (u, o) = (under.get(&pos), over.get(&pos));
            let key = format!("{u:?}{o:?}");
            let idx = *cache.entry((base, key)).or_insert_with(|| {
                let mut s = styles[base].clone();
                if let Some(u) = u {
                    lay_under(&mut s, u, &normal);
                }
                if let Some(o) = o {
                    lay_over(&mut s, o);
                }
                styles.push(s);
                styles.len() - 1
            });
            sheet.cells.entry(pos).or_insert(super::xlsx::Cell { value: None, style: 0 }).style = idx;
        }
    }
}

/// A table style under a cell's own formatting: only what the cell leaves unset (a font
/// colour the same as the Normal style's counts as unset)
fn lay_under(s: &mut Style, d: &Dxf, normal: &super::xlsx::Font) {
    if s.fill.is_none() {
        s.fill = d.fill;
    }
    if s.font.color.is_none() || s.font.color == normal.color {
        s.font.color = d.color.or(s.font.color);
    }
    if d.bold == Some(true) {
        s.font.bold = true;
    }
    if d.italic == Some(true) {
        s.font.italic = true;
    }
    let b = &mut s.borders;
    b.left = b.left.or(d.borders.left);
    b.right = b.right.or(d.borders.right);
    b.top = b.top.or(d.borders.top);
    b.bottom = b.bottom.or(d.borders.bottom);
}

/// A conditional format over everything
fn lay_over(s: &mut Style, d: &Dxf) {
    if let Some(f) = d.fill {
        s.fill = Some(f);
    }
    if let Some(c) = d.color {
        s.font.color = Some(c);
    }
    if let Some(b) = d.bold {
        s.font.bold = b;
    }
    if let Some(i) = d.italic {
        s.font.italic = i;
    }
    if let Some(u) = d.underline {
        s.font.underline = u;
    }
    if let Some(x) = d.strike {
        s.font.strike = x;
    }
    if let Some(n) = &d.num_fmt {
        s.num_fmt = n.clone();
    }
    let b = &mut s.borders;
    b.left = d.borders.left.or(b.left);
    b.right = d.borders.right.or(b.right);
    b.top = d.borders.top.or(b.top);
    b.bottom = d.borders.bottom.or(b.bottom);
}

// ---------------------------------------------------------------------------
// Table styles

/// The parts of a table style, from the least to the most specific
#[derive(Default)]
struct Elements {
    whole: Option<Dxf>,
    row1: Option<Dxf>,
    row2: Option<Dxf>,
    col1: Option<Dxf>,
    col2: Option<Dxf>,
    first_col: Option<Dxf>,
    last_col: Option<Dxf>,
    header: Option<Dxf>,
    totals: Option<Dxf>,
}

fn table_style(name: Option<&str>, defs: &Defs) -> Option<Elements> {
    let name = name?;
    if let Some(parts) = defs.table_styles.get(name) {
        let mut e = Elements::default();
        for (kind, dxf) in parts {
            let d = defs.dxfs.get(*dxf).cloned();
            match kind.as_str() {
                "wholeTable" => e.whole = d,
                "firstRowStripe" => e.row1 = d,
                "secondRowStripe" => e.row2 = d,
                "firstColumnStripe" => e.col1 = d,
                "secondColumnStripe" => e.col2 = d,
                "firstColumn" => e.first_col = d,
                "lastColumn" => e.last_col = d,
                "headerRow" => e.header = d,
                "totalRow" => e.totals = d,
                _ => {}
            }
        }
        return Some(e);
    }
    builtin(name, defs.theme)
}

fn tint(c: Rgb, t: f32) -> Rgb {
    super::xlsx::tint(c, t)
}

fn line(kind: super::xlsx::Line, color: Rgb) -> Option<Edge> {
    Some(Edge { line: kind, color })
}

/// Excel's built-in table styles (Light 1-21, Medium 1-28, Dark 1-11), made from the
/// workbook's theme colours like Excel makes them
fn builtin(name: &str, theme: &[Rgb]) -> Option<Elements> {
    use super::xlsx::Line::{Double, Medium, Thin};
    let rest = name.strip_prefix("TableStyle")?;
    let (family, n) = if let Some(n) = rest.strip_prefix("Light") {
        ("light", n)
    } else if let Some(n) = rest.strip_prefix("Medium") {
        ("medium", n)
    } else if let Some(n) = rest.strip_prefix("Dark") {
        ("dark", n)
    } else {
        return None;
    };
    let n: usize = n.parse().ok()?;
    let theme_color = |i: usize| theme.get(i).copied().unwrap_or([0, 0, 0]);
    let dark = theme_color(0);
    let white = [255, 255, 255];
    // within each group of seven: dark 1, then accents 1-6
    let color = |k: usize| if k == 0 { dark } else { theme_color(3 + k) };
    let bold = || Dxf { bold: Some(true), ..Default::default() };
    let mut e = Elements::default();
    match family {
        "light" if (1..=7).contains(&n) => {
            let c = color(n - 1);
            let band = if n == 1 { tint(c, 0.85) } else { tint(c, 0.8) };
            e.whole = Some(Dxf { color: Some(if n == 1 { dark } else { tint(c, -0.25) }), borders: Borders { top: line(Thin, c), bottom: line(Thin, c), ..Default::default() }, ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), borders: Borders { bottom: line(Thin, c), ..Default::default() }, ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Thin, c), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(band), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(bold());
            e.last_col = Some(bold());
        }
        "light" if (8..=14).contains(&n) => {
            let c = color(n - 8);
            let all = Borders { left: line(Thin, c), right: line(Thin, c), top: line(Thin, c), bottom: line(Thin, c), ..Default::default() };
            e.whole = Some(Dxf { borders: all.clone(), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Double, c), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { borders: Borders { top: line(Thin, c), bottom: line(Thin, c), ..Default::default() }, ..Default::default() });
            e.col1 = Some(Dxf { borders: Borders { left: line(Thin, c), right: line(Thin, c), ..Default::default() }, ..Default::default() });
            e.first_col = Some(bold());
            e.last_col = Some(bold());
        }
        "light" if (15..=21).contains(&n) => {
            let c = color(n - 15);
            let all = Borders { left: line(Thin, c), right: line(Thin, c), top: line(Thin, c), bottom: line(Thin, c), ..Default::default() };
            e.whole = Some(Dxf { borders: all, inside_h: line(Thin, c), inside_v: line(Thin, c), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), borders: Borders { bottom: line(Medium, c), ..Default::default() }, ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Double, c), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(if n == 15 { tint(c, 0.85) } else { tint(c, 0.8) }), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(bold());
            e.last_col = Some(bold());
        }
        "medium" if (1..=7).contains(&n) => {
            let c = color(n - 1);
            let rule = if n == 1 { tint(c, 0.35) } else { tint(c, 0.4) };
            let outer = Borders { left: line(Thin, rule), right: line(Thin, rule), top: line(Thin, rule), bottom: line(Thin, rule), ..Default::default() };
            e.whole = Some(Dxf { borders: outer, inside_h: line(Thin, rule), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Double, c), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(if n == 1 { tint(c, 0.85) } else { tint(c, 0.8) }), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(bold());
            e.last_col = Some(bold());
        }
        "medium" if (8..=14).contains(&n) => {
            let c = color(n - 8);
            let white_line = line(Thin, white);
            e.whole = Some(Dxf { fill: Some(if n == 8 { tint(c, 0.85) } else { tint(c, 0.8) }), inside_h: white_line, inside_v: white_line, ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), borders: Borders { bottom: line(Medium, white), ..Default::default() }, ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), borders: Borders { top: line(Medium, white), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(if n == 8 { tint(c, 0.65) } else { tint(c, 0.6) }), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), ..Default::default() });
            e.last_col = e.first_col.clone();
        }
        "medium" if (15..=21).contains(&n) => {
            let c = color(n - 15);
            let outer = Borders { left: None, right: None, top: line(Medium, dark), bottom: line(Medium, dark), ..Default::default() };
            e.whole = Some(Dxf { borders: outer, inside_h: line(Thin, dark), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), borders: Borders { bottom: line(Medium, dark), ..Default::default() }, ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Double, dark), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(tint(dark, 0.85)), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(c), ..Default::default() });
            e.last_col = e.first_col.clone();
        }
        "medium" if (22..=28).contains(&n) => {
            let c = color(n - 22);
            let rule = if n == 22 { tint(c, 0.35) } else { tint(c, 0.4) };
            let all = Borders { left: line(Thin, rule), right: line(Thin, rule), top: line(Thin, rule), bottom: line(Thin, rule), ..Default::default() };
            e.whole = Some(Dxf { fill: Some(if n == 22 { tint(c, 0.85) } else { tint(c, 0.8) }), borders: all, inside_h: line(Thin, rule), inside_v: line(Thin, rule), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Double, c), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(if n == 22 { tint(c, 0.65) } else { tint(c, 0.6) }), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(bold());
            e.last_col = Some(bold());
        }
        "dark" if (1..=7).contains(&n) => {
            let c = color(n - 1);
            let (body, band) = if n == 1 { (tint(dark, 0.35), tint(dark, 0.15)) } else { (tint(c, -0.25), tint(c, -0.5)) };
            e.whole = Some(Dxf { color: Some(white), fill: Some(body), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(dark), borders: Borders { bottom: line(Medium, white), ..Default::default() }, ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(band), borders: Borders { top: line(Double, white), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(band), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(band), ..Default::default() });
            e.last_col = e.first_col.clone();
        }
        "dark" if (8..=11).contains(&n) => {
            // pairs of colours: dark 1 / dark 1, accent 1 / 2, 3 / 4, 5 / 6
            let (c, head) = if n == 8 { (dark, dark) } else { (color(2 * (n - 8) - 1), color(2 * (n - 8))) };
            e.whole = Some(Dxf { fill: Some(tint(c, 0.8)), ..Default::default() });
            e.header = Some(Dxf { bold: Some(true), color: Some(white), fill: Some(head), ..Default::default() });
            e.totals = Some(Dxf { bold: Some(true), borders: Borders { top: line(Double, dark), ..Default::default() }, ..Default::default() });
            e.row1 = Some(Dxf { fill: Some(tint(c, 0.6)), ..Default::default() });
            e.col1 = e.row1.clone();
            e.first_col = Some(bold());
            e.last_col = Some(bold());
        }
        _ => return None,
    }
    Some(e)
}

/// Each cell's share of a table's style
fn table_overlay(t: &TableDef, el: &Elements, out: &mut HashMap<(u32, u32), Dxf>) {
    let (r1, c1, r2, c2) = t.range;
    let header_end = r1 + t.header_rows; // first row after the header
    let totals_start = (r2 + 1).saturating_sub(t.totals_rows).max(header_end);
    // an element applied to the block (top, left, bottom, right) of cells
    let region = |d: &Dxf, (rt, rl, rb, rr): Range, r: u32, c: u32| -> Dxf {
        let mut x = d.clone();
        x.borders = Borders {
            left: if c == rl { d.borders.left } else { d.inside_v },
            right: if c == rr { d.borders.right } else { d.inside_v },
            top: if r == rt { d.borders.top } else { d.inside_h },
            bottom: if r == rb { d.borders.bottom } else { d.inside_h },
            ..Default::default()
        };
        x
    };
    for r in r1..=r2 {
        for c in c1..=c2 {
            let mut d = Dxf::default();
            let header = r < header_end;
            let totals = r >= totals_start && t.totals_rows > 0;
            let body = !header && !totals;
            let mut layer = |x: Dxf| d.cover(&x);
            if let Some(w) = &el.whole {
                layer(region(w, t.range, r, c));
            }
            if t.col_stripes && body {
                let k = c - c1;
                let stripe = if k % 2 == 0 { &el.col1 } else { &el.col2 };
                if let Some(s) = stripe {
                    layer(region(s, (header_end, c, totals_start.saturating_sub(1), c), r, c));
                }
            }
            if t.row_stripes && body {
                let k = r - header_end;
                let stripe = if k % 2 == 0 { &el.row1 } else { &el.row2 };
                if let Some(s) = stripe {
                    layer(region(s, (r, c1, r, c2), r, c));
                }
            }
            if t.first_col && c == c1 {
                if let Some(s) = &el.first_col {
                    layer(region(s, (r1, c1, r2, c1), r, c));
                }
            }
            if t.last_col && c == c2 {
                if let Some(s) = &el.last_col {
                    layer(region(s, (r1, c2, r2, c2), r, c));
                }
            }
            if header {
                if let Some(s) = &el.header {
                    layer(region(s, (r1, c1, header_end - 1, c2), r, c));
                }
            }
            if totals {
                if let Some(s) = &el.totals {
                    layer(region(s, (totals_start, c1, r2, c2), r, c));
                }
            }
            if d != Dxf::default() {
                out.insert((r, c), d);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Conditional formats

struct SheetCells<'a> {
    book: Book<'a>,
}

impl Cells for SheetCells<'_> {
    fn get(&self, sheet: usize, r: u32, c: u32) -> V {
        self.book.get(sheet, r, c)
    }
    fn sheet_index(&self, name: &str) -> Option<usize> {
        self.book.sheet_index(name)
    }
    fn extent(&self, sheet: usize) -> (u32, u32) {
        self.book.extent(sheet)
    }
}

type Overlays = HashMap<(u32, u32), Dxf>;

fn conditional(sheets: &[Sheet], si: usize, conds: &[CondFormat], defs: &Defs, date1904: bool) -> (Overlays, HashMap<(u32, u32), Mark>) {
    let empty: Vec<HashMap<(u32, u32), formula::Pending>> = vec![HashMap::new(); sheets.len()];
    let cells = SheetCells {
        book: Book::new(sheets.iter().map(|s| s.name.clone()).collect(), sheets.iter().map(|s| &s.cells).collect(), empty.iter().collect(), date1904),
    };
    let (max_r, max_c) = cells.extent(si);
    let value = |r: u32, c: u32| cells.get(si, r, c);
    let mut over: Overlays = HashMap::new();
    let mut marks = HashMap::new();
    // rules of every range, highest priority (lowest number) first
    let mut rules: Vec<(&CondFormat, &CfRule)> = conds.iter().flat_map(|cf| cf.rules.iter().map(move |r| (cf, r))).collect();
    rules.sort_by_key(|(_, r)| r.priority);
    let mut stopped: std::collections::HashSet<(u32, u32)> = Default::default();
    let mut scaled: std::collections::HashSet<(u32, u32)> = Default::default();
    for (cf, rule) in rules {
        // the cells of the format, within the used part of the sheet
        let positions: Vec<(u32, u32)> = cf
            .ranges
            .iter()
            .flat_map(|&(r1, c1, r2, c2)| {
                let (r2, c2) = (r2.min(max_r), c2.min(max_c));
                (r1..=r2).flat_map(move |r| (c1..=c2).map(move |c| (r, c)))
            })
            .collect();
        if positions.is_empty() {
            continue;
        }
        let origin = cf.ranges.first().map(|r| (r.0, r.1)).unwrap_or((0, 0));
        let values: Vec<V> = positions.iter().map(|&(r, c)| value(r, c)).collect();
        let numbers: Vec<f64> = values.iter().filter_map(|v| if let V::Num(n) = v { Some(*n) } else { None }).collect();
        match &rule.kind {
            CfKind::Scale(cfvos, colors) => {
                if numbers.is_empty() || cfvos.len() != colors.len() || cfvos.len() < 2 {
                    continue;
                }
                let stops: Vec<f64> = cfvos.iter().map(|v| threshold(v, &numbers, &cells, si, origin, date1904)).collect();
                for (pos, v) in positions.iter().zip(&values) {
                    let V::Num(x) = v else { continue };
                    if stopped.contains(pos) || !scaled.insert(*pos) {
                        continue;
                    }
                    over.entry(*pos).or_default().fill_in(&Dxf { fill: Some(scale_color(*x, &stops, colors)), ..Default::default() });
                }
            }
            CfKind::Bar(lo, hi, color) => {
                if numbers.is_empty() {
                    continue;
                }
                let (a, b) = (threshold(lo, &numbers, &cells, si, origin, date1904), threshold(hi, &numbers, &cells, si, origin, date1904));
                for (pos, v) in positions.iter().zip(&values) {
                    let V::Num(x) = v else { continue };
                    if stopped.contains(pos) || marks.contains_key(pos) {
                        continue;
                    }
                    // Excel's bars never shrink to nothing: 10% at the minimum
                    let t = if b > a { ((x - a) / (b - a)).clamp(0.0, 1.0) } else { 1.0 };
                    marks.insert(*pos, Mark::Bar { frac: (0.1 + 0.9 * t) as f32, color: *color });
                }
            }
            CfKind::Icons { set, cfvos, reverse, show_value } => {
                if numbers.is_empty() || cfvos.is_empty() {
                    continue;
                }
                let stops: Vec<(f64, bool)> = cfvos.iter().map(|v| (threshold(v, &numbers, &cells, si, origin, date1904), v.gte)).collect();
                for (pos, v) in positions.iter().zip(&values) {
                    let V::Num(x) = v else { continue };
                    if stopped.contains(pos) || marks.contains_key(pos) {
                        continue;
                    }
                    let mut index = 0;
                    for (i, (s, gte)) in stops.iter().enumerate().skip(1) {
                        if if *gte { *x >= *s } else { *x > *s } {
                            index = i;
                        }
                    }
                    let of = cfvos.len();
                    let index = if *reverse { of - 1 - index } else { index };
                    marks.insert(*pos, Mark::Icon { set: set.clone(), index, of, show_value: *show_value });
                }
            }
            CfKind::Rule => {
                let Some(dxf) = rule.dxf.and_then(|i| defs.dxfs.get(i)) else { continue };
                let exprs: Vec<formula::Expr> = rule.formulas.iter().map(|f| formula::parse(f)).collect();
                let stats = Stats::new(&values, &numbers, rule);
                for (pos, v) in positions.iter().zip(&values) {
                    if stopped.contains(pos) {
                        continue;
                    }
                    let shift = (pos.0 as i64 - origin.0 as i64, pos.1 as i64 - origin.1 as i64);
                    let eval = |e: &formula::Expr| Eval { cells: &cells, sheet: si, here: *pos, date1904, sep: cells.book.sep }.value(&e.shifted(shift.0, shift.1));
                    if matches(rule, v, &exprs, &eval, &stats, date1904) {
                        over.entry(*pos).or_default().fill_in(dxf);
                        if rule.stop {
                            stopped.insert(*pos);
                        }
                    }
                }
            }
        }
    }
    (over, marks)
}

/// What rules over the whole range need: averages, ranks, counts
struct Stats {
    average: f64,
    cutoff: Option<f64>,
    counts: HashMap<String, usize>,
}

fn key(v: &V) -> Option<String> {
    match v {
        V::Num(n) => Some(format!("n{n}")),
        V::Text(t) => Some(format!("t{}", t.to_lowercase())),
        V::Bool(b) => Some(format!("b{b}")),
        _ => None,
    }
}

impl Stats {
    fn new(values: &[V], numbers: &[f64], rule: &CfRule) -> Stats {
        let average = if numbers.is_empty() { 0.0 } else { numbers.iter().sum::<f64>() / numbers.len() as f64 };
        let cutoff = (rule.rule == "top10" && !numbers.is_empty()).then(|| {
            let mut sorted = numbers.to_vec();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            if !rule.bottom {
                sorted.reverse();
            }
            let n = if rule.percent { ((numbers.len() as f64 * rule.rank as f64 / 100.0).floor() as usize).max(1) } else { rule.rank.max(1) as usize };
            sorted[n.min(sorted.len()) - 1]
        });
        let mut counts = HashMap::new();
        if matches!(rule.rule.as_str(), "duplicateValues" | "uniqueValues") {
            for v in values {
                if let Some(k) = key(v) {
                    *counts.entry(k).or_insert(0) += 1;
                }
            }
        }
        Stats { average, cutoff, counts }
    }
}

fn matches(rule: &CfRule, v: &V, exprs: &[formula::Expr], eval: &dyn Fn(&formula::Expr) -> V, stats: &Stats, date1904: bool) -> bool {
    let text = || match v {
        V::Text(t) => t.to_lowercase(),
        V::Num(n) => format!("{n}"),
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        _ => String::new(),
    };
    let needle = || rule.text.clone().unwrap_or_default().to_lowercase();
    match rule.rule.as_str() {
        "cellIs" => {
            if matches!(v, V::Blank) && !matches!(rule.operator.as_deref(), Some("equal") | Some("notEqual") | Some("lessThan") | Some("lessThanOrEqual")) {
                return false;
            }
            let operand = |i: usize| exprs.get(i).map(eval).unwrap_or(V::Blank);
            let cmp = |i: usize| formula::compare_values(v, &operand(i));
            use std::cmp::Ordering::*;
            match rule.operator.as_deref().unwrap_or("equal") {
                "equal" => cmp(0) == Equal,
                "notEqual" => cmp(0) != Equal,
                "greaterThan" => cmp(0) == Greater,
                "lessThan" => cmp(0) == Less,
                "greaterThanOrEqual" => cmp(0) != Less,
                "lessThanOrEqual" => cmp(0) != Greater,
                "between" | "notBetween" => {
                    let (a, b) = (operand(0), operand(1));
                    let (lo, hi) = if formula::compare_values(&a, &b) == Greater { (b, a) } else { (a, b) };
                    let inside = formula::compare_values(v, &lo) != Less && formula::compare_values(v, &hi) != Greater;
                    inside == (rule.operator.as_deref() == Some("between"))
                }
                _ => false,
            }
        }
        "expression" => exprs.first().map(|e| matches!(eval(e), V::Bool(true)) || matches!(eval(e), V::Num(n) if n != 0.0)).unwrap_or(false),
        "containsText" => text().contains(&needle()),
        "notContainsText" => !text().contains(&needle()),
        "beginsWith" => text().starts_with(&needle()),
        "endsWith" => text().ends_with(&needle()),
        "containsBlanks" => matches!(v, V::Blank) || matches!(v, V::Text(t) if t.trim().is_empty()),
        "notContainsBlanks" => !(matches!(v, V::Blank) || matches!(v, V::Text(t) if t.trim().is_empty())),
        "containsErrors" => matches!(v, V::Err(_)),
        "notContainsErrors" => !matches!(v, V::Err(_)),
        "top10" => match (v, stats.cutoff) {
            (V::Num(x), Some(cut)) => if rule.bottom { *x <= cut } else { *x >= cut },
            _ => false,
        },
        "aboveAverage" => match v {
            V::Num(x) => match (rule.above, rule.equal_average) {
                (true, false) => *x > stats.average,
                (true, true) => *x >= stats.average,
                (false, false) => *x < stats.average,
                (false, true) => *x <= stats.average,
            },
            _ => false,
        },
        "duplicateValues" => key(v).and_then(|k| stats.counts.get(&k)).is_some_and(|n| *n > 1),
        "uniqueValues" => key(v).and_then(|k| stats.counts.get(&k)).is_some_and(|n| *n == 1),
        "timePeriod" => match v {
            V::Num(x) => in_period(x.floor(), rule.time_period.as_deref().unwrap_or(""), date1904),
            _ => false,
        },
        _ => false,
    }
}

fn in_period(day: f64, period: &str, date1904: bool) -> bool {
    let today = match formula::today_serial(date1904) {
        Some(t) => t.floor(),
        None => return false,
    };
    let month = |d: f64| formula::year_month(d, date1904);
    // weeks start on Sunday, as in Excel
    let week_start = today - formula::weekday0(today, date1904) as f64;
    match period {
        "today" => day == today,
        "yesterday" => day == today - 1.0,
        "tomorrow" => day == today + 1.0,
        "last7Days" => day <= today && day > today - 7.0,
        "thisWeek" => day >= week_start && day < week_start + 7.0,
        "lastWeek" => day >= week_start - 7.0 && day < week_start,
        "nextWeek" => day >= week_start + 7.0 && day < week_start + 14.0,
        "thisMonth" | "lastMonth" | "nextMonth" => {
            let (Some((y, m)), Some((ty, tm))) = (month(day), month(today)) else { return false };
            let delta = (y * 12 + m) - (ty * 12 + tm);
            delta == match period {
                "thisMonth" => 0,
                "lastMonth" => -1,
                _ => 1,
            }
        }
        _ => false,
    }
}

/// The value a colour scale / data bar / icon set threshold stands for
fn threshold(v: &Cfvo, numbers: &[f64], cells: &SheetCells, si: usize, origin: (u32, u32), date1904: bool) -> f64 {
    let min = numbers.iter().copied().fold(f64::MAX, f64::min);
    let max = numbers.iter().copied().fold(f64::MIN, f64::max);
    let val = || -> Option<f64> {
        let raw = v.val.as_deref()?;
        raw.trim().parse::<f64>().ok().or_else(|| {
            let e = formula::parse(raw);
            match (Eval { cells, sheet: si, here: origin, date1904, sep: cells.book.sep }).value(&e) {
                V::Num(n) => Some(n),
                _ => None,
            }
        })
    };
    match v.kind.as_str() {
        "min" | "autoMin" => min,
        "max" | "autoMax" => max,
        "num" | "formula" => val().unwrap_or(min),
        "percent" => min + (max - min) * val().unwrap_or(0.0) / 100.0,
        "percentile" => {
            let mut s = numbers.to_vec();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let p = (val().unwrap_or(50.0) / 100.0).clamp(0.0, 1.0) * (s.len() - 1) as f64;
            let (lo, hi) = (p.floor() as usize, p.ceil() as usize);
            s[lo] + (s[hi] - s[lo]) * (p - lo as f64)
        }
        _ => min,
    }
}

fn scale_color(x: f64, stops: &[f64], colors: &[Rgb]) -> Rgb {
    if x <= stops[0] {
        return colors[0];
    }
    for i in 1..stops.len() {
        if x <= stops[i] {
            let t = if stops[i] > stops[i - 1] { ((x - stops[i - 1]) / (stops[i] - stops[i - 1])) as f32 } else { 1.0 };
            let (a, b) = (colors[i - 1], colors[i]);
            return [0, 1, 2].map(|k| (a[k] as f32 + (b[k] as f32 - a[k] as f32) * t).round() as u8);
        }
    }
    colors[colors.len() - 1]
}

/// Whether a cell value is shown (`Value`) — used by tests
#[cfg(test)]
fn num(n: f64) -> Option<super::numfmt::Value> {
    Some(super::numfmt::Value::Number(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::xlsx::{Cell, Line};

    fn sheet(values: &[(u32, u32, f64)]) -> Sheet {
        let mut s = Sheet { name: "S".into(), ..Default::default() };
        for &(r, c, v) in values {
            s.cells.insert((r, c), Cell { value: num(v), style: 0 });
        }
        s
    }

    fn rule(kind: CfKind, name: &str) -> CfRule {
        CfRule { kind, rule: name.into(), priority: 1, dxf: None, operator: None, formulas: vec![], text: None, stop: false, rank: 10, percent: false, bottom: false, above: true, equal_average: false, time_period: None }
    }

    #[test]
    fn medium2_tables_band_their_rows() {
        let theme = [[0, 0, 0], [255, 255, 255], [0x44, 0x54, 0x6A], [0xE7, 0xE6, 0xE6], [0x44, 0x72, 0xC4], [0xED, 0x7D, 0x31]];
        let e = builtin("TableStyleMedium2", &theme).unwrap();
        assert_eq!(e.header.as_ref().unwrap().fill, Some([0x44, 0x72, 0xC4]));
        let band = e.row1.as_ref().unwrap().fill.unwrap();
        assert!(band.iter().zip([0xD9, 0xE1, 0xF2]).all(|(a, b)| (*a as i32 - b).abs() <= 2), "{band:x?}");

        let t = TableDef { range: (0, 0, 4, 1), header_rows: 1, totals_rows: 1, style: Some("TableStyleMedium2".into()), row_stripes: true, col_stripes: false, first_col: false, last_col: false };
        let mut out = HashMap::new();
        table_overlay(&t, &e, &mut out);
        assert_eq!(out[&(0, 0)].color, Some([255, 255, 255]));
        assert_eq!(out[&(1, 0)].fill, Some(band));
        assert_eq!(out[&(2, 0)].fill, None);
        assert_eq!(out[&(4, 1)].borders.top.map(|e| e.line), Some(Line::Double));
    }

    #[test]
    fn conditional_formats() {
        let mut sheets = vec![sheet(&[(0, 0, 10.0), (1, 0, 50.0), (2, 0, 90.0)])];
        let red = Dxf { fill: Some([255, 0, 0]), ..Default::default() };
        let mut big = rule(CfKind::Rule, "cellIs");
        big.operator = Some("greaterThan".into());
        big.formulas = vec!["40".into()];
        big.dxf = Some(0);
        let bars = rule(CfKind::Bar(Cfvo { kind: "min".into(), val: None, gte: true }, Cfvo { kind: "max".into(), val: None, gte: true }, [0, 128, 0]), "dataBar");
        let conds = vec![vec![CondFormat { ranges: vec![(0, 0, 2, 0)], rules: vec![big, bars] }]];
        let mut styles = vec![Style::default()];
        let defs = Defs { dxfs: &[red], table_styles: &HashMap::new(), theme: &[] };
        apply(&mut sheets, &mut styles, &defs, &[vec![]], &conds, false);
        let s = &sheets[0];
        assert_eq!(styles[s.cells[&(0, 0)].style].fill, None);
        assert_eq!(styles[s.cells[&(1, 0)].style].fill, Some([255, 0, 0]));
        assert_eq!(s.marks[&(0, 0)], Mark::Bar { frac: 0.1, color: [0, 128, 0] });
        assert_eq!(s.marks[&(2, 0)], Mark::Bar { frac: 1.0, color: [0, 128, 0] });
    }

    #[test]
    fn colour_scales_blend() {
        let c = scale_color(50.0, &[0.0, 100.0], &[[0, 0, 0], [200, 100, 0]]);
        assert_eq!(c, [100, 50, 0]);
    }
}
