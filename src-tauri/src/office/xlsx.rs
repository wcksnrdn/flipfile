// Reads an Excel workbook (.xlsx) into a model ready for printing: every visible
// sheet's cells with their resolved styles, column widths and row heights in points,
// merged cells, pictures, and the print setup (paper, margins, scale, print area,
// repeated title rows, page breaks, header and footer).
//
// Formulas are not recalculated: Excel stores each formula's last result next to it,
// and that is what gets printed.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::sync::Arc;

use roxmltree::{Document, Node};

use super::numfmt::{self, Value};
use super::sheet_style::{Cfvo, CfKind, CfRule, CondFormat, Dxf, Mark, TableDef};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DRAW: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";

/// Points per EMU (drawing offsets)
const EMU: f32 = 1.0 / 12700.0;

pub type Rgb = [u8; 3];

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub name: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Rgb>,
}

impl Default for Font {
    fn default() -> Self {
        Font { name: "Calibri".into(), size: 11.0, bold: false, italic: false, underline: false, strike: false, color: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Line {
    Hair,
    Thin,
    Medium,
    Thick,
    Double,
    Dotted,
    Dashed,
    MediumDashed,
    DashDot,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub line: Line,
    pub color: Rgb,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Borders {
    pub left: Option<Edge>,
    pub right: Option<Edge>,
    pub top: Option<Edge>,
    pub bottom: Option<Edge>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum HAlign {
    #[default]
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VAlign {
    Top,
    Center,
    #[default]
    Bottom,
}

/// A cell format (`<xf>`), fully resolved
#[derive(Clone, Debug, Default)]
pub struct Style {
    pub num_fmt: String,
    pub font: Font,
    pub fill: Option<Rgb>,
    pub borders: Borders,
    pub h: HAlign,
    pub v: VAlign,
    pub wrap: bool,
    pub indent: u32,
    pub shrink: bool,
    /// text turned this many degrees anticlockwise (-90..=90)
    pub rotation: i16,
    /// letters stacked one under the other ("Vertical Text")
    pub stacked: bool,
}

/// What a run of rich text changes from its cell's font
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunFont {
    pub name: Option<String>,
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<Rgb>,
    /// 1 superscript, -1 subscript
    pub script: i8,
}

impl RunFont {
    /// The cell's font with this run's changes
    pub fn over(&self, base: &Font) -> Font {
        Font {
            name: self.name.clone().unwrap_or_else(|| base.name.clone()),
            size: self.size.unwrap_or(base.size),
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            underline: self.underline.unwrap_or(base.underline),
            strike: self.strike.unwrap_or(base.strike),
            color: self.color.or(base.color),
        }
    }
}

/// A piece of a cell's text in its own font (bold words, a red figure…)
#[derive(Clone, Debug, PartialEq)]
pub struct RichRun {
    pub text: String,
    pub font: RunFont,
}

pub type Rich = Arc<Vec<RichRun>>;

#[derive(Clone, Debug)]
pub struct Cell {
    pub value: Option<Value>,
    pub style: usize,
}

/// `(first row, first col, last row, last col)`, zero-based and inclusive
pub type Range = (u32, u32, u32, u32);

#[derive(Clone, Debug)]
pub struct Picture {
    /// where the picture goes: top-left (row, col, row offset pt, col offset pt) and size in pt
    pub from: (u32, u32, f32, f32),
    pub to: Option<(u32, u32, f32, f32)>,
    pub size: (f32, f32),
    pub data: Vec<u8>,
    pub ext: String,
}

/// Where a drawing object sits: top-left (row, col, row offset pt, col offset pt), the
/// same for its bottom-right corner (two-cell anchors), and its size in points
#[derive(Clone, Debug)]
pub struct Anchor {
    pub from: (u32, u32, f32, f32),
    pub to: Option<(u32, u32, f32, f32)>,
    pub size: (f32, f32),
}

/// A chart or shapes of the drawing layer
#[derive(Clone, Debug)]
pub enum Drawn {
    Chart(Anchor, Box<super::chart::Chart>),
    Shapes(Anchor, Vec<super::drawing::Shape>),
}

impl Drawn {
    pub fn anchor(&self) -> &Anchor {
        match self {
            Drawn::Chart(a, _) | Drawn::Shapes(a, _) => a,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Print {
    /// paper size in points, before orientation
    pub paper: (f32, f32),
    pub landscape: bool,
    /// left, right, top, bottom, header, footer — in points
    pub margins: [f32; 6],
    /// percent, used unless `fit` is set
    pub scale: f32,
    /// pages wide, pages tall (0 = as many as needed)
    pub fit: Option<(u32, u32)>,
    pub gridlines: bool,
    pub center_h: bool,
    pub center_v: bool,
    pub over_then_down: bool,
    pub header: Option<String>,
    pub footer: Option<String>,
    pub first_page: Option<u32>,
    /// someone chose how this sheet prints (landscape, a scale, fit to pages); otherwise
    /// the PDF lays it out to be readable instead of copying Excel's defaults
    pub explicit: bool,
}

impl Default for Print {
    fn default() -> Self {
        Print {
            paper: (595.3, 841.9),
            landscape: false,
            // Excel's "Normal" margins: 0.7" sides, 0.75" top/bottom, 0.3" header/footer
            margins: [50.4, 50.4, 54.0, 54.0, 21.6, 21.6],
            scale: 100.0,
            fit: None,
            gridlines: false,
            center_h: false,
            center_v: false,
            over_then_down: false,
            header: None,
            footer: None,
            first_page: None,
            explicit: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<(u32, u32), Cell>,
    /// width in points of columns that are not the default (`0.0` = hidden)
    pub col_widths: BTreeMap<u32, f32>,
    pub default_col_width: f32,
    /// height in points of rows that are not the default (`0.0` = hidden)
    pub row_heights: BTreeMap<u32, f32>,
    pub default_row_height: f32,
    /// rows Excel would size to their (wrapped) text: no height stored
    pub auto_rows: bool,
    pub merges: Vec<Range>,
    pub pictures: Vec<Picture>,
    /// charts and shapes
    pub drawn: Vec<Drawn>,
    /// charts waiting for the cells they plot to be worked out, with their place among
    /// `drawn` (what is drawn later covers what came before)
    pub chart_parts: Vec<(usize, Anchor, String)>,
    pub print: Print,
    pub print_area: Option<Range>,
    pub title_rows: Option<(u32, u32)>,
    pub title_cols: Option<(u32, u32)>,
    /// page breaks: a new page starts *before* these rows / columns
    pub row_breaks: Vec<u32>,
    pub col_breaks: Vec<u32>,
    /// cells whose text mixes fonts, by position
    pub rich: HashMap<(u32, u32), Rich>,
    /// formulas the file stores no result for, worked out once the workbook is read
    pub formulas: HashMap<(u32, u32), super::formula::Pending>,
    /// the style of empty cells in a column (cells of a table that hold nothing)
    pub col_styles: HashMap<u32, usize>,
    /// Format as Table and conditional formats, folded into the cells once read
    pub tables: Vec<TableDef>,
    pub conds: Vec<CondFormat>,
    /// data bars and icons drawn by conditional formats
    pub marks: HashMap<(u32, u32), Mark>,
    /// rows and columns frozen on screen (Freeze Panes): usually a table's headings
    pub frozen: (u32, u32),
}

impl Sheet {
    pub fn col_width(&self, c: u32) -> f32 {
        self.col_widths.get(&c).copied().unwrap_or(self.default_col_width)
    }
}

pub struct Workbook {
    pub sheets: Vec<Sheet>,
    pub styles: Vec<Style>,
    pub date1904: bool,
}

// ---------------------------------------------------------------------------
// Package helpers

struct Package {
    zip: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Package {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let path = path.trim_start_matches('/');
        let mut f = self.zip.by_name(path).ok()?;
        // a corrupt or hostile file must not exhaust memory
        if f.size() > 512 * 1024 * 1024 {
            return None;
        }
        let mut out = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut out).ok()?;
        Some(out)
    }
    fn text(&mut self, path: &str) -> Option<String> {
        let text = String::from_utf8(self.read(path)?).ok()?;
        // Nested too deep to parse safely: as good as missing
        (!super::xml_too_deep(&text)).then_some(text)
    }
    /// Relationships of a part: (id, type, resolved target path), internal ones only
    fn rels_list(&mut self, part: &str) -> Vec<(String, String, String)> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels_path = format!("{dir}/_rels/{file}.rels");
        let mut out = vec![];
        let Some(xml) = self.text(&rels_path) else { return out };
        let Ok(doc) = Document::parse(&xml) else { return out };
        for r in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
            if let (Some(id), Some(target)) = (r.attribute("Id"), r.attribute("Target")) {
                if r.attribute("TargetMode") == Some("External") {
                    continue;
                }
                out.push((id.to_string(), r.attribute("Type").unwrap_or("").to_string(), resolve(dir, target)));
            }
        }
        out
    }
    /// Relationships of a part: id -> resolved target path
    fn rels(&mut self, part: &str) -> HashMap<String, String> {
        self.rels_list(part).into_iter().map(|(id, _, t)| (id, t)).collect()
    }
}

/// `xl/worksheets` + `../media/a.png` -> `xl/media/a.png`
fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}
fn children<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| c.tag_name().name() == name)
}
fn attr_f32(n: Node, name: &str) -> Option<f32> {
    n.attribute(name).and_then(|v| v.parse().ok())
}
fn attr_u32(n: Node, name: &str) -> Option<u32> {
    n.attribute(name).and_then(|v| v.parse().ok())
}
fn attr_bool(n: Node, name: &str) -> Option<bool> {
    n.attribute(name).map(|v| v == "1" || v == "true")
}

/// "AB12" -> (row 11, col 27)
pub fn cell_ref(r: &str) -> Option<(u32, u32)> {
    let r = r.trim_start_matches('$');
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = r[letters.len()..].trim_start_matches('$');
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut col = 0u32;
    for c in letters.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = digits.parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

fn col_letters(s: &str) -> Option<u32> {
    let s = s.trim_start_matches('$');
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut col = 0u32;
    for c in s.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    Some(col - 1)
}

/// "A1:C9" (also "$A$1:$C$9", or a single "B4")
pub fn range_ref(r: &str) -> Option<Range> {
    let (a, b) = r.split_once(':').unwrap_or((r, r));
    let (r1, c1) = cell_ref(a)?;
    let (r2, c2) = cell_ref(b)?;
    Some((r1.min(r2), c1.min(c2), r1.max(r2), c1.max(c2)))
}

// ---------------------------------------------------------------------------
// Colours

/// Excel's legacy 64-colour palette
pub fn indexed_color(i: usize) -> Option<Rgb> {
    const P: [u32; 64] = [
        0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00,
        0xFF00FF, 0x00FFFF, 0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF,
        0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF,
        0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699, 0x969696,
        0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333,
    ];
    P.get(i).map(|v| [(v >> 16) as u8, (v >> 8) as u8, *v as u8])
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    let s = if s.len() == 8 { &s[2..] } else { s };
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Lightens (tint > 0) or darkens (tint < 0) like Excel, in HSL
pub fn tint(c: Rgb, t: f32) -> Rgb {
    if t == 0.0 {
        return c;
    }
    let [r, g, b] = c.map(|v| v as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let mut l = (max + min) / 2.0;
    let (h, s) = if max == min {
        (0.0, 0.0)
    } else {
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
        (h, s)
    };
    l = if t < 0.0 { l * (1.0 + t) } else { l * (1.0 - t) + t };
    let hue = |p: f32, q: f32, mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue(p, q, h + 1.0 / 3.0), hue(p, q, h), hue(p, q, h - 1.0 / 3.0))
    };
    [r, g, b].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

struct Colors {
    theme: Vec<Rgb>,
    indexed: Option<Vec<Rgb>>,
}

impl Colors {
    /// `<color rgb|theme|indexed tint>`; None for "automatic"
    fn get(&self, n: Node) -> Option<Rgb> {
        let t = attr_f32(n, "tint").unwrap_or(0.0);
        let base = if let Some(rgb) = n.attribute("rgb") {
            hex(rgb)
        } else if let Some(th) = attr_u32(n, "theme") {
            // Excel swaps the first two pairs: 0 = lt1, 1 = dk1, 2 = lt2, 3 = dk2
            let i = match th {
                0 => 1,
                1 => 0,
                2 => 3,
                3 => 2,
                i => i as usize as u32,
            } as usize;
            self.theme.get(i).copied()
        } else if let Some(ix) = attr_u32(n, "indexed") {
            if ix == 64 {
                return None; // system foreground: automatic
            }
            self.indexed.as_ref().and_then(|p| p.get(ix as usize).copied()).or_else(|| indexed_color(ix as usize))
        } else {
            None
        };
        base.map(|c| tint(c, t))
    }
}

fn theme_colors(xml: &str) -> Vec<Rgb> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    let Some(scheme) = doc.descendants().find(|n| n.has_tag_name((DRAW, "clrScheme"))) else { return vec![] };
    let mut out = vec![];
    for name in ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"] {
        let c = scheme
            .children()
            .find(|c| c.tag_name().name() == name)
            .and_then(|e| e.children().find(|c| c.is_element()))
            .and_then(|c| c.attribute("lastClr").or(c.attribute("val")))
            .and_then(hex)
            .unwrap_or([0, 0, 0]);
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// Styles

fn read_styles(xml: &str, colors: &Colors) -> Vec<Style> {
    let Ok(doc) = Document::parse(xml) else { return vec![Style::default()] };
    let root = doc.root_element();
    let mut formats: HashMap<u32, String> = HashMap::new();
    if let Some(nf) = child(root, "numFmts") {
        for f in children(nf, "numFmt") {
            if let (Some(id), Some(code)) = (attr_u32(f, "numFmtId"), f.attribute("formatCode")) {
                formats.insert(id, code.to_string());
            }
        }
    }
    let fonts: Vec<Font> = child(root, "fonts")
        .map(|fs| {
            children(fs, "font")
                .map(|f| {
                    let val = |name: &str| child(f, name);
                    let on = |name: &str| val(name).map(|n| n.attribute("val").map(|v| v != "0" && v != "false").unwrap_or(true)).unwrap_or(false);
                    Font {
                        name: val("name").and_then(|n| n.attribute("val")).unwrap_or("Calibri").to_string(),
                        size: val("sz").and_then(|n| attr_f32(n, "val")).unwrap_or(11.0),
                        bold: on("b"),
                        italic: on("i"),
                        underline: val("u").map(|n| n.attribute("val") != Some("none")).unwrap_or(false),
                        strike: on("strike"),
                        color: val("color").and_then(|c| colors.get(c)),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<Option<Rgb>> = child(root, "fills")
        .map(|fs| {
            children(fs, "fill")
                .map(|f| {
                    let p = child(f, "patternFill")?;
                    let kind = p.attribute("patternType").unwrap_or("none");
                    if kind == "none" {
                        return None;
                    }
                    let fg = child(p, "fgColor").and_then(|c| colors.get(c));
                    let bg = child(p, "bgColor").and_then(|c| colors.get(c));
                    match kind {
                        "solid" => fg.or(bg),
                        // patterns print as a blend of the two colours
                        _ => {
                            let a = fg.unwrap_or([0, 0, 0]);
                            let b = bg.unwrap_or([255, 255, 255]);
                            let k = match kind {
                                "gray125" => 0.125,
                                "gray0625" => 0.0625,
                                "lightGray" => 0.25,
                                "mediumGray" => 0.5,
                                "darkGray" => 0.75,
                                _ => 0.5,
                            };
                            Some([0, 1, 2].map(|i| (a[i] as f32 * k + b[i] as f32 * (1.0 - k)).round() as u8))
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let borders: Vec<Borders> = child(root, "borders")
        .map(|bs| {
            children(bs, "border")
                .map(|b| {
                    let edge = |name: &str| -> Option<Edge> {
                        let e = child(b, name).or_else(|| child(b, if name == "left" { "start" } else if name == "right" { "end" } else { name }))?;
                        let line = match e.attribute("style")? {
                            "hair" => Line::Hair,
                            "thin" => Line::Thin,
                            "medium" => Line::Medium,
                            "thick" => Line::Thick,
                            "double" => Line::Double,
                            "dotted" => Line::Dotted,
                            "dashed" => Line::Dashed,
                            "mediumDashed" => Line::MediumDashed,
                            "dashDot" | "dashDotDot" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => Line::DashDot,
                            _ => return None,
                        };
                        let color = child(e, "color").and_then(|c| colors.get(c)).unwrap_or([0, 0, 0]);
                        Some(Edge { line, color })
                    };
                    Borders { left: edge("left"), right: edge("right"), top: edge("top"), bottom: edge("bottom") }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut styles = vec![];
    if let Some(xfs) = child(root, "cellXfs") {
        for xf in children(xfs, "xf") {
            let fmt_id = attr_u32(xf, "numFmtId").unwrap_or(0);
            let num_fmt = formats.get(&fmt_id).cloned().or_else(|| numfmt::builtin(fmt_id).map(str::to_string)).unwrap_or_else(|| "General".into());
            let mut s = Style {
                num_fmt,
                font: attr_u32(xf, "fontId").and_then(|i| fonts.get(i as usize)).cloned().unwrap_or_default(),
                fill: attr_u32(xf, "fillId").and_then(|i| fills.get(i as usize)).copied().flatten(),
                borders: attr_u32(xf, "borderId").and_then(|i| borders.get(i as usize)).cloned().unwrap_or_default(),
                ..Default::default()
            };
            if let Some(a) = child(xf, "alignment") {
                s.h = match a.attribute("horizontal") {
                    Some("left") => HAlign::Left,
                    Some("center") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    Some("fill") => HAlign::Fill,
                    Some("justify") | Some("distributed") => HAlign::Justify,
                    Some("centerContinuous") => HAlign::CenterContinuous,
                    _ => HAlign::General,
                };
                s.v = match a.attribute("vertical") {
                    Some("top") => VAlign::Top,
                    Some("center") | Some("justify") | Some("distributed") => VAlign::Center,
                    _ => VAlign::Bottom,
                };
                s.wrap = attr_bool(a, "wrapText").unwrap_or(false);
                s.indent = attr_u32(a, "indent").unwrap_or(0);
                s.shrink = attr_bool(a, "shrinkToFit").unwrap_or(false);
                match attr_u32(a, "textRotation").unwrap_or(0) {
                    255 => s.stacked = true,
                    r @ 0..=90 => s.rotation = r as i16,
                    r @ 91..=180 => s.rotation = 90 - r as i16,
                    _ => {}
                }
            }
            styles.push(s);
        }
    }
    if styles.is_empty() {
        styles.push(Style { num_fmt: "General".into(), font: fonts.first().cloned().unwrap_or_default(), ..Default::default() });
    }
    styles
}

// ---------------------------------------------------------------------------
// Workbook

/// Shared strings: the plain text, and its runs when it mixes fonts
fn shared_strings(xml: &str, colors: &Colors) -> Vec<(String, Option<Rich>)> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    doc.root_element().children().filter(|n| n.has_tag_name((MAIN, "si"))).map(|si| (rich_text(si), runs(si, colors))).collect()
}

/// The runs of an `<si>` / `<is>` that sets fonts of its own (None for plain text)
fn runs(n: Node, colors: &Colors) -> Option<Rich> {
    let rs: Vec<Node> = children(n, "r").collect();
    if rs.is_empty() || !rs.iter().any(|r| child(*r, "rPr").is_some()) {
        return None;
    }
    let out: Vec<RichRun> = rs
        .iter()
        .map(|r| {
            let text = child(*r, "t").and_then(|t| t.text()).unwrap_or("").to_string();
            let font = child(*r, "rPr").map(|p| {
                let val = |name: &str| child(p, name);
                let on = |name: &str| val(name).map(|n| n.attribute("val").map(|v| v != "0" && v != "false").unwrap_or(true));
                RunFont {
                    name: val("rFont").and_then(|n| n.attribute("val")).map(str::to_string),
                    size: val("sz").and_then(|n| attr_f32(n, "val")),
                    bold: on("b"),
                    italic: on("i"),
                    underline: val("u").map(|n| n.attribute("val") != Some("none")),
                    strike: on("strike"),
                    color: val("color").and_then(|c| colors.get(c)),
                    script: match val("vertAlign").and_then(|n| n.attribute("val")) {
                        Some("superscript") => 1,
                        Some("subscript") => -1,
                        _ => 0,
                    },
                }
            });
            RichRun { text, font: font.unwrap_or_default() }
        })
        .collect();
    Some(Arc::new(out))
}

/// The text of an `<si>` / `<is>`: its `<t>` runs, without phonetic hints
fn rich_text(n: Node) -> String {
    let mut s = String::new();
    for t in n.descendants().filter(|d| d.has_tag_name((MAIN, "t"))) {
        if t.ancestors().any(|a| a.has_tag_name((MAIN, "rPh"))) {
            continue;
        }
        s.push_str(t.text().unwrap_or(""));
    }
    s
}

/// Pixel width of a column `width` (in characters), as Excel computes it
fn col_px(width: f32, mdw: f32) -> f32 {
    (((256.0 * width + (128.0 / mdw).trunc()) / 256.0) * mdw).trunc()
}

/// The widest digit of the workbook's default font, in pixels (7 for Calibri 11)
fn max_digit_width(font: &Font) -> f32 {
    let w = super::typeset::text_width(&font.name, font.size, false, false, "0")
        .or_else(|| super::typeset::text_width("Calibri", font.size, false, false, "0"))
        .map(|pt| (pt * 96.0 / 72.0).trunc())
        .unwrap_or((font.size * 7.0 / 11.0).round());
    w.clamp(4.0, 30.0)
}

pub fn read(bytes: &[u8]) -> Result<Workbook, String> {
    let zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|_| "not an Excel workbook (.xlsx)".to_string())?;
    let mut pkg = Package { zip };
    // the officeDocument relationship, usually xl/workbook.xml
    let wb_path = pkg
        .rels_list("")
        .into_iter()
        .find(|(_, kind, _)| kind.ends_with("/officeDocument"))
        .map(|(_, _, t)| t)
        .unwrap_or_else(|| "xl/workbook.xml".into());
    let wb_xml = pkg.text(&wb_path).ok_or("the workbook is damaged (no workbook.xml)")?;
    let wb_list = pkg.rels_list(&wb_path);
    let wb_rels: HashMap<String, String> = wb_list.iter().map(|(id, _, t)| (id.clone(), t.clone())).collect();
    let wb_doc = Document::parse(&wb_xml).map_err(|_| "the workbook is damaged".to_string())?;
    let wb = wb_doc.root_element();

    let date1904 = child(wb, "workbookPr").and_then(|p| attr_bool(p, "date1904")).unwrap_or(false);

    let rel_target = |kind: &str| wb_list.iter().find(|(_, k, _)| k.ends_with(kind)).map(|(_, _, t)| t.clone());
    let theme = rel_target("/theme").and_then(|p| pkg.text(&p)).map(|x| theme_colors(&x)).unwrap_or_else(default_theme);
    let indexed = rel_target("/styles").and_then(|p| pkg.text(&p)).and_then(|x| custom_palette(&x));
    let colors = Colors { theme: if theme.is_empty() { default_theme() } else { theme }, indexed };
    let styles_xml = rel_target("/styles").and_then(|p| pkg.text(&p));
    let mut styles = styles_xml.as_deref().map(|x| read_styles(x, &colors)).unwrap_or_else(|| vec![Style { num_fmt: "General".into(), ..Default::default() }]);
    let (dxfs, table_styles) = styles_xml.as_deref().map(|x| read_dxfs(x, &colors)).unwrap_or_default();
    let strings = rel_target("/sharedStrings").and_then(|p| pkg.text(&p)).map(|x| shared_strings(&x, &colors)).unwrap_or_default();
    let mdw = max_digit_width(&styles[0].font);

    // print areas and titles, per sheet index
    let mut names: Vec<(String, Option<usize>, String)> = vec![];
    if let Some(dn) = child(wb, "definedNames") {
        for n in children(dn, "definedName") {
            if let Some(name) = n.attribute("name") {
                names.push((name.to_string(), n.attribute("localSheetId").and_then(|v| v.parse().ok()), n.text().unwrap_or("").to_string()));
            }
        }
    }

    let mut sheets = vec![];
    // hidden sheets don't print, but formulas may read them
    let mut hidden = vec![];
    if let Some(list) = child(wb, "sheets") {
        for (index, s) in children(list, "sheet").enumerate() {
            let is_hidden = matches!(s.attribute("state"), Some("hidden") | Some("veryHidden"));
            let name = s.attribute("name").unwrap_or("Sheet").to_string();
            let Some(rid) = s.attribute((REL, "id")) else { continue };
            let Some(path) = wb_rels.get(rid).cloned() else { continue };
            // chart sheets and dialog sheets have no cells to print
            if !path.contains("worksheets") {
                continue;
            }
            let Some(xml) = pkg.text(&path) else { continue };
            let mut sheet = read_sheet(&xml, &strings, &styles, mdw, &colors).map_err(|e| format!("sheet \"{name}\": {e}"))?;
            sheet.name = name.clone();
            (sheet.pictures, sheet.chart_parts, sheet.drawn) = read_drawing(&mut pkg, &path, &xml, &colors.theme);
            sheet.tables = read_tables(&mut pkg, &path, &xml);
            for (n, local, text) in &names {
                let mine = local.map(|l| l == index).unwrap_or_else(|| refers_to_sheet(text, &name));
                if !mine {
                    continue;
                }
                match n.as_str() {
                    "_xlnm.Print_Area" => sheet.print_area = text.split(',').next().and_then(|r| range_ref(strip_sheet(r))),
                    "_xlnm.Print_Titles" => {
                        for part in text.split(',') {
                            let r = strip_sheet(part);
                            let (a, b) = r.split_once(':').unwrap_or((r, r));
                            let (a, b) = (a.trim_start_matches('$'), b.trim_start_matches('$'));
                            if let (Ok(r1), Ok(r2)) = (a.parse::<u32>(), b.parse::<u32>()) {
                                sheet.title_rows = Some((r1.saturating_sub(1), r2.saturating_sub(1)));
                            } else if let (Some(c1), Some(c2)) = (col_letters(a), col_letters(b)) {
                                sheet.title_cols = Some((c1, c2));
                            }
                        }
                    }
                    _ => {}
                }
            }
            sheets.push(sheet);
            hidden.push(is_hidden);
        }
    }
    work_out_formulas(&mut sheets, date1904);
    let tables: Vec<Vec<TableDef>> = sheets.iter_mut().map(|s| std::mem::take(&mut s.tables)).collect();
    let conds: Vec<Vec<CondFormat>> = sheets.iter_mut().map(|s| std::mem::take(&mut s.conds)).collect();
    let defs = super::sheet_style::Defs { dxfs: &dxfs, table_styles: &table_styles, theme: &colors.theme };
    super::sheet_style::apply(&mut sheets, &mut styles, &defs, &tables, &conds, date1904);
    read_charts(&mut sheets, &styles, &colors.theme, date1904);
    let mut hidden = hidden.into_iter();
    sheets.retain(|_| !hidden.next().unwrap_or(false));
    if sheets.is_empty() {
        return Err("the workbook has no sheets to print".into());
    }
    Ok(Workbook { sheets, styles, date1904 })
}

/// Charts, once the cells they plot hold their values
fn read_charts(sheets: &mut [Sheet], styles: &[Style], theme: &[Rgb], date1904: bool) {
    let parts: Vec<Vec<(usize, Anchor, String)>> = sheets.iter_mut().map(|s| std::mem::take(&mut s.chart_parts)).collect();
    if parts.iter().all(|p| p.is_empty()) {
        return;
    }
    let sep = numfmt::Separators::system();
    for (si, list) in parts.into_iter().enumerate() {
        let mut added = 0;
        for (at, anchor, xml) in list {
            let data = |f: &str| -> Vec<(Option<f64>, String)> {
                let super::formula::Expr::Area(a) = super::formula::parse(f) else { return vec![] };
                let target = match &a.sheet {
                    Some(name) => sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name)),
                    None => Some(si),
                };
                let Some(sheet) = target.map(|t| &sheets[t]) else { return vec![] };
                let last = sheet.cells.keys().next_back().map(|k| k.0).unwrap_or(0);
                let (r2, c2) = (a.r2.min(last), a.c2.min(a.c1 + 16_383));
                let mut out = vec![];
                for r in a.r1..=r2 {
                    for c in a.c1..=c2 {
                        let cell = sheet.cells.get(&(r, c));
                        let value = cell.and_then(|c| c.value.as_ref());
                        let n = match value {
                            Some(Value::Number(n)) => Some(*n),
                            _ => None,
                        };
                        let code = cell.map(|c| styles[c.style].num_fmt.as_str()).unwrap_or("General");
                        let shown = value.map(|v| numfmt::format(v, code, date1904, sep).text).unwrap_or_default();
                        let shown: String = shown.chars().filter(|c| *c != numfmt::HIDE_ON && *c != numfmt::HIDE_OFF).collect();
                        out.push((n, shown.trim().to_string()));
                        if out.len() > 100_000 {
                            return out;
                        }
                    }
                }
                out
            };
            if let Some(chart) = super::chart::parse(&xml, theme, &data) {
                let at = (at + added).min(sheets[si].drawn.len());
                sheets[si].drawn.insert(at, Drawn::Chart(anchor, Box::new(chart)));
                added += 1;
            }
        }
    }
}

/// Fills in the results of formulas stored without one, top to bottom
fn work_out_formulas(sheets: &mut [Sheet], date1904: bool) {
    if sheets.iter().all(|s| s.formulas.is_empty()) {
        return;
    }
    let results: Vec<(usize, (u32, u32), Option<Value>)> = {
        let book = super::formula::Book::new(
            sheets.iter().map(|s| s.name.clone()).collect(),
            sheets.iter().map(|s| &s.cells).collect(),
            sheets.iter().map(|s| &s.formulas).collect(),
            date1904,
        );
        let mut out = vec![];
        for (si, s) in sheets.iter().enumerate() {
            let mut keys: Vec<(u32, u32)> = s.formulas.keys().copied().collect();
            keys.sort_unstable();
            out.extend(keys.into_iter().map(|(r, c)| (si, (r, c), book.result(si, r, c).into_value())));
        }
        out
    };
    for (si, pos, value) in results {
        if let Some(value) = value.filter(|v| !matches!(v, Value::Text(t) if t.is_empty())) {
            sheets[si].cells.entry(pos).or_insert(Cell { value: None, style: 0 }).value = Some(value);
        }
    }
    for s in sheets {
        s.formulas = HashMap::new();
    }
}

fn strip_sheet(r: &str) -> &str {
    r.rsplit_once('!').map(|(_, r)| r).unwrap_or(r).trim()
}

fn refers_to_sheet(text: &str, sheet: &str) -> bool {
    let quoted = format!("'{}'!", sheet.replace('\'', "''"));
    text.starts_with(&quoted) || text.starts_with(&format!("{sheet}!"))
}

fn default_theme() -> Vec<Rgb> {
    // Office 2013+ theme
    ["000000", "FFFFFF", "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47", "0563C1", "954F72"]
        .iter()
        .filter_map(|h| hex(h))
        .collect()
}

fn custom_palette(styles_xml: &str) -> Option<Vec<Rgb>> {
    let doc = Document::parse(styles_xml).ok()?;
    let ic = doc.descendants().find(|n| n.has_tag_name((MAIN, "indexedColors")))?;
    Some(children(ic, "rgbColor").filter_map(|c| c.attribute("rgb").and_then(hex)).collect())
}

fn read_sheet(xml: &str, strings: &[(String, Option<Rich>)], styles: &[Style], mdw: f32, colors: &Colors) -> Result<Sheet, String> {
    let doc = Document::parse(xml).map_err(|_| "damaged sheet".to_string())?;
    let root = doc.root_element();
    let mut sheet = Sheet::default();

    if let Some(pane) = child(root, "sheetViews").and_then(|v| child(v, "sheetView")).and_then(|v| child(v, "pane")) {
        if matches!(pane.attribute("state"), Some("frozen") | Some("frozenSplit")) {
            sheet.frozen = (attr_f32(pane, "ySplit").unwrap_or(0.0) as u32, attr_f32(pane, "xSplit").unwrap_or(0.0) as u32);
        }
    }

    // default sizes
    let fmt = child(root, "sheetFormatPr");
    let base_font = &styles[0].font;
    sheet.default_row_height = fmt
        .and_then(|f| attr_f32(f, "defaultRowHeight"))
        .unwrap_or_else(|| (base_font.size * 15.0 / 11.0).max(12.0));
    let default_px = if let Some(w) = fmt.and_then(|f| attr_f32(f, "defaultColWidth")) {
        col_px(w, mdw)
    } else {
        let base = fmt.and_then(|f| attr_f32(f, "baseColWidth")).unwrap_or(8.0);
        // Excel rounds the default column up to a multiple of 8 pixels
        ((base * mdw + 5.0) / 8.0).ceil() * 8.0
    };
    sheet.default_col_width = default_px * 0.75;
    sheet.auto_rows = true;

    if let Some(cols) = child(root, "cols") {
        for c in children(cols, "col") {
            let (Some(min), Some(max)) = (attr_u32(c, "min"), attr_u32(c, "max")) else { continue };
            let hidden = attr_bool(c, "hidden").unwrap_or(false);
            let w = if hidden { 0.0 } else { attr_f32(c, "width").map(|w| col_px(w, mdw) * 0.75).unwrap_or(sheet.default_col_width) };
            if let Some(st) = attr_u32(c, "style").filter(|&st| st > 0 && (st as usize) < styles.len()) {
                for i in min.max(1)..=max.min(16384) {
                    sheet.col_styles.insert(i - 1, st as usize);
                }
            }
            // a <col> spanning the rest of the sheet only sets the default
            if max - min > 16000 {
                sheet.default_col_width = w;
                continue;
            }
            for i in min.max(1)..=max.min(16384) {
                sheet.col_widths.insert(i - 1, w);
            }
        }
    }

    // shared formulas: index -> (text, the cell it was written for)
    let mut shared: HashMap<String, (Arc<str>, u32, u32)> = HashMap::new();
    if let Some(data) = child(root, "sheetData") {
        for (ri, row) in children(data, "row").enumerate() {
            let r = attr_u32(row, "r").map(|r| r - 1).unwrap_or(ri as u32);
            if attr_bool(row, "hidden").unwrap_or(false) {
                sheet.row_heights.insert(r, 0.0);
            } else if let Some(ht) = attr_f32(row, "ht") {
                sheet.row_heights.insert(r, ht);
            }
            let row_style = attr_u32(row, "s").filter(|_| attr_bool(row, "customFormat").unwrap_or(false));
            let mut next_col = 0u32;
            for c in children(row, "c") {
                let (cr, cc) = c.attribute("r").and_then(cell_ref).unwrap_or((r, next_col));
                next_col = cc + 1;
                let style = attr_u32(c, "s").or(row_style).unwrap_or(0) as usize;
                let style = if style < styles.len() { style } else { 0 };
                let v = child(c, "v").and_then(|v| v.text());
                let value = match c.attribute("t").unwrap_or("n") {
                    "s" => v.and_then(|i| i.trim().parse::<usize>().ok()).and_then(|i| strings.get(i)).map(|(s, rich)| {
                        if let Some(rich) = rich {
                            sheet.rich.insert((cr, cc), rich.clone());
                        }
                        Value::Text(s.clone())
                    }),
                    "str" => v.map(|s| Value::Text(s.to_string())),
                    "inlineStr" => child(c, "is").map(|is| {
                        if let Some(rich) = runs(is, colors) {
                            sheet.rich.insert((cr, cc), rich);
                        }
                        Value::Text(rich_text(is))
                    }),
                    "b" => v.map(|s| Value::Bool(s.trim() == "1")),
                    "e" => v.map(|s| Value::Error(s.to_string())),
                    "d" => v.map(|s| Value::Text(s.to_string())),
                    _ => v.and_then(|s| s.trim().parse::<f64>().ok()).map(Value::Number),
                };
                let value = value.filter(|v| !matches!(v, Value::Text(t) if t.is_empty()));
                // a formula: kept to work out when the file has no result for it
                let mut formula = false;
                if let Some(f) = child(c, "f") {
                    let text = f.text().map(str::trim).filter(|t| !t.is_empty());
                    let si = f.attribute("si").filter(|_| f.attribute("t") == Some("shared"));
                    let pending = match (text, si) {
                        (Some(t), si) => {
                            let text: Arc<str> = Arc::from(t);
                            if let Some(si) = si {
                                shared.insert(si.to_string(), (text.clone(), cr, cc));
                            }
                            Some(super::formula::Pending { text, shift: (0, 0) })
                        }
                        (None, Some(si)) => shared.get(si).map(|(text, r0, c0)| super::formula::Pending { text: text.clone(), shift: (cr as i64 - *r0 as i64, cc as i64 - *c0 as i64) }),
                        _ => None,
                    };
                    if let (Some(p), None) = (pending, &value) {
                        sheet.formulas.insert((cr, cc), p);
                        formula = true;
                    }
                }
                if value.is_none() && style == 0 && !formula {
                    continue;
                }
                sheet.cells.insert((cr, cc), Cell { value, style });
            }
        }
    }

    if let Some(m) = child(root, "mergeCells") {
        sheet.merges = children(m, "mergeCell").filter_map(|n| n.attribute("ref").and_then(range_ref)).collect();
    }

    let p = &mut sheet.print;
    if let Some(o) = child(root, "printOptions") {
        p.gridlines = attr_bool(o, "gridLines").unwrap_or(false);
        p.center_h = attr_bool(o, "horizontalCentered").unwrap_or(false);
        p.center_v = attr_bool(o, "verticalCentered").unwrap_or(false);
    }
    if let Some(m) = child(root, "pageMargins") {
        let inch = |n: &str, d: f32| attr_f32(m, n).unwrap_or(d) * 72.0;
        p.margins = [inch("left", 0.7), inch("right", 0.7), inch("top", 0.75), inch("bottom", 0.75), inch("header", 0.3), inch("footer", 0.3)];
    }
    if let Some(s) = child(root, "pageSetup") {
        if let Some(size) = attr_u32(s, "paperSize").and_then(paper) {
            p.paper = size;
        }
        p.landscape = s.attribute("orientation") == Some("landscape");
        p.scale = attr_f32(s, "scale").unwrap_or(100.0).clamp(10.0, 400.0);
        p.explicit |= p.landscape || p.scale != 100.0;
        p.over_then_down = s.attribute("pageOrder") == Some("overThenDown");
        if attr_bool(s, "useFirstPageNumber").unwrap_or(false) {
            p.first_page = attr_u32(s, "firstPageNumber");
        }
        let fit_on = child(root, "sheetPr").and_then(|sp| child(sp, "pageSetUpPr")).and_then(|u| attr_bool(u, "fitToPage")).unwrap_or(false);
        if fit_on {
            p.fit = Some((attr_u32(s, "fitToWidth").unwrap_or(1), attr_u32(s, "fitToHeight").unwrap_or(1)));
            p.explicit = true;
        }
    }
    if let Some(hf) = child(root, "headerFooter") {
        p.header = child(hf, "oddHeader").and_then(|n| n.text()).map(str::to_string);
        p.footer = child(hf, "oddFooter").and_then(|n| n.text()).map(str::to_string);
    }
    let breaks = |name: &str| -> Vec<u32> {
        child(root, name).map(|b| children(b, "brk").filter_map(|n| attr_u32(n, "id")).filter(|&i| i > 0).collect()).unwrap_or_default()
    };
    // a break "at id" puts row/column `id` (zero-based) first on the next page
    sheet.row_breaks = breaks("rowBreaks");
    sheet.col_breaks = breaks("colBreaks");
    sheet.conds = children(root, "conditionalFormatting").filter_map(|cf| cond_format(cf, colors)).collect();
    Ok(sheet)
}

/// A `<conditionalFormatting>`: its ranges and rules
fn cond_format(cf: Node, colors: &Colors) -> Option<CondFormat> {
    let ranges: Vec<Range> = cf.attribute("sqref")?.split_whitespace().filter_map(range_ref).collect();
    if ranges.is_empty() {
        return None;
    }
    let cfvos = |n: Node| -> Vec<Cfvo> {
        children(n, "cfvo").map(|v| Cfvo { kind: v.attribute("type").unwrap_or("min").to_string(), val: v.attribute("val").map(str::to_string), gte: attr_bool(v, "gte").unwrap_or(true) }).collect()
    };
    let rules = children(cf, "cfRule")
        .map(|r| {
            let rule = r.attribute("type").unwrap_or("").to_string();
            let kind = if let Some(n) = child(r, "colorScale") {
                CfKind::Scale(cfvos(n), children(n, "color").filter_map(|c| colors.get(c)).collect())
            } else if let Some(n) = child(r, "dataBar") {
                let v = cfvos(n);
                let lo = v.first().cloned().unwrap_or(Cfvo { kind: "min".into(), val: None, gte: true });
                let hi = v.get(1).cloned().unwrap_or(Cfvo { kind: "max".into(), val: None, gte: true });
                CfKind::Bar(lo, hi, child(n, "color").and_then(|c| colors.get(c)).unwrap_or([0x63, 0x8E, 0xC6]))
            } else if let Some(n) = child(r, "iconSet") {
                CfKind::Icons {
                    set: n.attribute("iconSet").unwrap_or("3TrafficLights1").to_string(),
                    cfvos: cfvos(n),
                    reverse: attr_bool(n, "reverse").unwrap_or(false),
                    show_value: attr_bool(n, "showValue").unwrap_or(true),
                }
            } else {
                CfKind::Rule
            };
            CfRule {
                kind,
                rule,
                priority: r.attribute("priority").and_then(|p| p.parse().ok()).unwrap_or(i32::MAX),
                dxf: attr_u32(r, "dxfId").map(|i| i as usize),
                operator: r.attribute("operator").map(str::to_string),
                formulas: children(r, "formula").filter_map(|f| f.text()).map(str::to_string).collect(),
                text: r.attribute("text").map(str::to_string),
                stop: attr_bool(r, "stopIfTrue").unwrap_or(false),
                rank: attr_u32(r, "rank").unwrap_or(10),
                percent: attr_bool(r, "percent").unwrap_or(false),
                bottom: attr_bool(r, "bottom").unwrap_or(false),
                above: attr_bool(r, "aboveAverage").unwrap_or(true),
                equal_average: attr_bool(r, "equalAverage").unwrap_or(false),
                time_period: r.attribute("timePeriod").map(str::to_string),
            }
        })
        .collect();
    Some(CondFormat { ranges, rules })
}

/// One side of a `<border>` (also in differential formats)
fn border_edge(b: Node, name: &str, colors: &Colors) -> Option<Edge> {
    let e = child(b, name)?;
    let line = match e.attribute("style")? {
        "hair" => Line::Hair,
        "thin" => Line::Thin,
        "medium" => Line::Medium,
        "thick" => Line::Thick,
        "double" => Line::Double,
        "dotted" => Line::Dotted,
        "dashed" => Line::Dashed,
        "mediumDashed" => Line::MediumDashed,
        "dashDot" | "dashDotDot" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => Line::DashDot,
        _ => return None,
    };
    let color = child(e, "color").and_then(|c| colors.get(c)).unwrap_or([0, 0, 0]);
    Some(Edge { line, color })
}

/// The differential formats (`<dxfs>`) and custom table styles of styles.xml
fn read_dxfs(xml: &str, colors: &Colors) -> (Vec<Dxf>, HashMap<String, Vec<(String, usize)>>) {
    let Ok(doc) = Document::parse(xml) else { return Default::default() };
    let root = doc.root_element();
    let dxfs = child(root, "dxfs")
        .map(|ds| {
            children(ds, "dxf")
                .map(|d| {
                    let mut x = Dxf::default();
                    if let Some(f) = child(d, "font") {
                        let on = |name: &str| child(f, name).map(|n| n.attribute("val").map(|v| v != "0" && v != "false").unwrap_or(true));
                        x.bold = on("b");
                        x.italic = on("i");
                        x.strike = on("strike");
                        x.underline = child(f, "u").map(|n| n.attribute("val") != Some("none"));
                        x.color = child(f, "color").and_then(|c| colors.get(c));
                    }
                    if let Some(p) = child(d, "fill").and_then(|f| child(f, "patternFill")) {
                        if p.attribute("patternType") != Some("none") {
                            // a differential fill keeps its solid colour in bgColor
                            x.fill = child(p, "bgColor").and_then(|c| colors.get(c)).or_else(|| child(p, "fgColor").and_then(|c| colors.get(c)));
                        }
                    }
                    if let Some(n) = child(d, "numFmt") {
                        x.num_fmt = n.attribute("formatCode").map(str::to_string);
                    }
                    if let Some(b) = child(d, "border") {
                        x.borders = Borders { left: border_edge(b, "left", colors), right: border_edge(b, "right", colors), top: border_edge(b, "top", colors), bottom: border_edge(b, "bottom", colors) };
                        x.inside_h = border_edge(b, "horizontal", colors);
                        x.inside_v = border_edge(b, "vertical", colors);
                    }
                    x
                })
                .collect()
        })
        .unwrap_or_default();
    let mut styles = HashMap::new();
    if let Some(ts) = child(root, "tableStyles") {
        for t in children(ts, "tableStyle") {
            let Some(name) = t.attribute("name") else { continue };
            let parts = children(t, "tableStyleElement").filter_map(|e| Some((e.attribute("type")?.to_string(), attr_u32(e, "dxfId")? as usize))).collect();
            styles.insert(name.to_string(), parts);
        }
    }
    (dxfs, styles)
}

/// The tables (Format as Table) of a sheet
fn read_tables(pkg: &mut Package, sheet_path: &str, sheet_xml: &str) -> Vec<TableDef> {
    let Ok(doc) = Document::parse(sheet_xml) else { return vec![] };
    let Some(parts) = child(doc.root_element(), "tableParts") else { return vec![] };
    let rels = pkg.rels(sheet_path);
    let mut out = vec![];
    for part in children(parts, "tablePart") {
        let Some(path) = part.attribute((REL, "id")).and_then(|id| rels.get(id)).cloned() else { continue };
        let Some(xml) = pkg.text(&path) else { continue };
        let Ok(t) = Document::parse(&xml) else { continue };
        let t = t.root_element();
        let Some(range) = t.attribute("ref").and_then(range_ref) else { continue };
        let info = child(t, "tableStyleInfo");
        let flag = |name: &str| info.and_then(|i| attr_bool(i, name)).unwrap_or(false);
        out.push(TableDef {
            range,
            header_rows: attr_u32(t, "headerRowCount").unwrap_or(1),
            totals_rows: attr_u32(t, "totalsRowCount").unwrap_or(0),
            style: info.and_then(|i| i.attribute("name")).map(str::to_string),
            row_stripes: flag("showRowStripes"),
            col_stripes: flag("showColumnStripes"),
            first_col: flag("showFirstColumn"),
            last_col: flag("showLastColumn"),
        });
    }
    out
}

/// Paper sizes Excel writes in `paperSize`, in points (portrait)
fn paper(code: u32) -> Option<(f32, f32)> {
    Some(match code {
        1 => (612.0, 792.0),   // Letter
        3 => (792.0, 1224.0),  // Tabloid
        5 => (612.0, 1008.0),  // Legal
        7 => (522.0, 756.0),   // Executive
        8 => (841.9, 1190.6),  // A3
        9 => (595.3, 841.9),   // A4
        11 => (419.5, 595.3),  // A5
        12 => (708.7, 1000.6), // B4 (JIS)
        13 => (515.9, 728.5),  // B5 (JIS)
        14 => (612.0, 936.0),  // Folio / F4 (8.5 x 13 in), common in Indonesia
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Pictures

/// A sheet's drawing layer: pictures, charts (their XML, read once the cells are worked
/// out) and shapes
fn read_drawing(pkg: &mut Package, sheet_path: &str, sheet_xml: &str, theme: &[Rgb]) -> (Vec<Picture>, Vec<(usize, Anchor, String)>, Vec<Drawn>) {
    let none = || (vec![], vec![], vec![]);
    let Ok(doc) = Document::parse(sheet_xml) else { return none() };
    let Some(drawing) = doc.root_element().children().find(|n| n.tag_name().name() == "drawing") else { return none() };
    let Some(rid) = drawing.attribute((REL, "id")) else { return none() };
    let rels = pkg.rels(sheet_path);
    let Some(dpath) = rels.get(rid).cloned() else { return none() };
    let Some(dxml) = pkg.text(&dpath) else { return none() };
    let drels = pkg.rels(&dpath);
    let Ok(ddoc) = Document::parse(&dxml) else { return none() };
    let (mut pictures, mut charts, mut drawn) = (vec![], vec![], vec![]);
    for anchor in ddoc.root_element().children().filter(|n| n.is_element()) {
        let kind = anchor.tag_name().name();
        if !matches!(kind, "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor") {
            continue;
        }
        let marker = |name: &str| -> Option<(u32, u32, f32, f32)> {
            let m = child(anchor, name)?;
            let num = |n: &str| child(m, n).and_then(|x| x.text()).and_then(|t| t.trim().parse::<f64>().ok());
            Some((num("row")? as u32, num("col")? as u32, num("rowOff").unwrap_or(0.0) as f32 * EMU, num("colOff").unwrap_or(0.0) as f32 * EMU))
        };
        let ext_size = anchor
            .descendants()
            .find(|n| n.has_tag_name((DRAW, "ext")) || n.has_tag_name((XDR, "ext")))
            .and_then(|e| Some((attr_f32(e, "cx")? * EMU, attr_f32(e, "cy")? * EMU)))
            .unwrap_or((0.0, 0.0));
        let place = match kind {
            "twoCellAnchor" => match (marker("from"), marker("to")) {
                (Some(from), Some(to)) => Anchor { from, to: Some(to), size: ext_size },
                _ => continue,
            },
            "oneCellAnchor" => match marker("from") {
                Some(from) => Anchor { from, to: None, size: ext_size },
                None => continue,
            },
            _ => continue,
        };
        // a picture
        if let Some(pic) = anchor.children().find(|n| n.has_tag_name((XDR, "pic"))) {
            let Some(embed) = pic.descendants().find(|n| n.has_tag_name((DRAW, "blip"))).and_then(|b| b.attribute((REL, "embed"))) else { continue };
            let Some(media) = drels.get(embed).cloned() else { continue };
            let ext = media.rsplit('.').next().unwrap_or("png").to_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp") {
                continue;
            }
            let Some(data) = pkg.read(&media) else { continue };
            pictures.push(Picture { from: place.from, to: place.to, size: place.size, data, ext });
            continue;
        }
        // a chart
        if let Some(frame) = anchor.children().find(|n| n.has_tag_name((XDR, "graphicFrame"))) {
            let chart_rel = frame.descendants().find(|n| n.tag_name().name() == "chart").and_then(|c| c.attribute((REL, "id")));
            if let Some(xml) = chart_rel.and_then(|id| drels.get(id).cloned()).and_then(|p| pkg.text(&p)) {
                charts.push((drawn.len(), place, xml));
            }
            continue;
        }
        // shapes, text boxes, lines and groups of them
        let shapes = super::drawing::shapes(anchor, theme, place.size);
        if !shapes.is_empty() {
            drawn.push(Drawn::Shapes(place, shapes));
        }
    }
    (pictures, charts, drawn)
}

// ---------------------------------------------------------------------------
// CSV, shown as a plain table

/// A CSV file as a one-sheet workbook: separators detected (`,` `;` tab), numbers kept as numbers
pub fn read_csv(bytes: &[u8], name: &str) -> Result<Workbook, String> {
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).into_owned();
    let first = text.lines().next().unwrap_or("");
    let delim = [',', ';', '\t', '|'].into_iter().max_by_key(|d| first.matches(*d).count()).unwrap_or(',');
    let mut sheet = Sheet { name: name.to_string(), default_col_width: 48.0, default_row_height: 15.0, auto_rows: true, ..Default::default() };
    sheet.print.gridlines = true;
    let mut widths: HashMap<u32, usize> = HashMap::new();
    for (r, record) in parse_csv(&text, delim).into_iter().enumerate() {
        for (c, field) in record.into_iter().enumerate() {
            if field.is_empty() {
                continue;
            }
            let w = widths.entry(c as u32).or_default();
            *w = (*w).max(field.chars().count());
            let style = if r == 0 { 1 } else { 0 };
            let value = match field.trim().parse::<f64>() {
                Ok(n) if !field.trim().starts_with('0') || field.trim() == "0" || field.trim().starts_with("0.") => Value::Number(n),
                _ => Value::Text(field),
            };
            sheet.cells.insert((r as u32, c as u32), Cell { value: Some(value), style });
        }
    }
    // columns as wide as their content (within reason), like Excel's autofit
    for (c, chars) in widths {
        sheet.col_widths.insert(c, (chars.clamp(6, 50) as f32 * 5.6 + 8.0).min(300.0));
    }
    if sheet.cells.is_empty() {
        return Err("the CSV file is empty".into());
    }
    let normal = Style { num_fmt: "General".into(), ..Default::default() };
    let header = Style { font: Font { bold: true, ..Default::default() }, fill: Some([242, 242, 242]), ..normal.clone() };
    Ok(Workbook { sheets: vec![sheet], styles: vec![normal, header], date1904: false })
}

fn parse_csv(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            }
            '"' if field.is_empty() => quoted = true,
            c if c == delim && !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references() {
        assert_eq!(cell_ref("A1"), Some((0, 0)));
        assert_eq!(cell_ref("$AB$12"), Some((11, 27)));
        assert_eq!(range_ref("$A$1:$C$9"), Some((0, 0, 8, 2)));
        assert_eq!(resolve("xl/worksheets", "../media/image1.png"), "xl/media/image1.png");
        assert_eq!(resolve("xl", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
    }

    #[test]
    fn column_widths_match_excel() {
        // Calibri 11: max digit width 7 px; Excel's default 8.43 is stored as 9.140625 = 64 px
        assert_eq!(col_px(9.140625, 7.0), 64.0);
        assert_eq!(col_px(13.7109375, 7.0), 96.0);
    }

    #[test]
    fn tints() {
        assert_eq!(tint([0x44, 0x72, 0xC4], 0.0), [0x44, 0x72, 0xC4]);
        // accent1 at 80% lighter: Excel shows D9E1F2
        let c = tint([0x44, 0x72, 0xC4], 0.7999816888943144);
        assert!(c.iter().zip([0xD9, 0xE1, 0xF2]).all(|(a, b)| (*a as i32 - b).abs() <= 2), "{c:x?}");
    }

    #[test]
    fn csv_fields() {
        let rows = parse_csv("a,\"b,c\",\"say \"\"hi\"\"\"\n1;2\n", ',');
        assert_eq!(rows[0], vec!["a", "b,c", "say \"hi\""]);
        let wb = read_csv(b"Name;Amount\nKopi;25000\nTeh;0012\n", "x.csv").unwrap();
        let s = &wb.sheets[0];
        assert_eq!(s.cells[&(1, 1)].value, Some(Value::Number(25000.0)));
        // leading zeros are codes, not numbers
        assert_eq!(s.cells[&(2, 1)].value, Some(Value::Text("0012".into())));
    }
}
