// Reads a Word document (.docx, Office Open XML) into a small layout model:
// page setup, paragraphs with resolved formatting, lists, tables, images, header and
// footer. Styles are resolved the way Word does it: document defaults, then the
// paragraph style chain, then character styles, then direct formatting.
//
// Anything not understood is skipped rather than failing the whole document.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use roxmltree::{Document as Xml, Node};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const V: &str = "urn:schemas-microsoft-com:vml";
const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Refuses absurd inputs (zip bombs, gigantic embedded media).
const MAX_PART_BYTES: u64 = 200 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Model

#[derive(Clone, Debug)]
pub struct Document {
    pub sections: Vec<Section>,
    pub images: Vec<Image>,
    pub default_font: String,
    pub default_size: f32,
    pub lang: Option<String>,
    /// Distance between Word's default tab stops (points)
    pub default_tab: f32,
    /// Names of all bookmarks in the body (sanitised)
    pub bookmarks: std::collections::HashSet<String>,
    /// Laid out like Word 2010 and older (compatibility mode < 15)
    pub legacy_layout: bool,
    /// Justified lines that end in a manual line break are stretched too (Word's
    /// default; off with the `doNotExpandShiftReturn` setting)
    pub justify_breaks: bool,
}

/// How a section starts relative to the previous one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SectionStart {
    NextPage,
    Continuous,
    EvenPage,
    OddPage,
}

/// A Word section: its own page size/orientation, margins, header, footer and numbering.
#[derive(Clone, Debug)]
pub struct Section {
    pub page: PageSetup,
    pub header: HeaderFooter,
    pub footer: HeaderFooter,
    pub blocks: Vec<Block>,
    pub start: SectionStart,
    /// Page numbering restarts at this number (`w:pgNumType w:start`)
    pub page_start: Option<u32>,
}

/// Word can show a different header/footer on the first page (`titlePg`) and on even
/// pages (`evenAndOddHeaders`). `None` = same as the default one.
#[derive(Clone, Debug, Default)]
pub struct HeaderFooter {
    pub default: Vec<Block>,
    pub first: Option<Vec<Block>>,
    pub even: Option<Vec<Block>>,
}

#[derive(Clone, Debug)]
pub struct PageSetup {
    pub width: f32, // points
    pub height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub header_distance: f32,
    pub footer_distance: f32,
    pub columns: u32,
}

impl Default for PageSetup {
    fn default() -> Self {
        // A4 with Word's default 1" margins
        PageSetup {
            width: 595.3,
            height: 841.9,
            margin_top: 72.0,
            margin_right: 72.0,
            margin_bottom: 72.0,
            margin_left: 72.0,
            header_distance: 36.0,
            footer_distance: 36.0,
            columns: 1,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

#[derive(Clone, Debug, Default)]
pub struct Paragraph {
    pub props: ParaProps,
    pub inlines: Vec<Inline>,
    pub marker: Option<ListMarker>,
    /// Formatting of the paragraph mark: sets the line height of an empty paragraph
    pub mark: RunProps,
}

#[derive(Clone, Debug)]
pub struct ListMarker {
    pub text: String,
    pub run: RunProps,
    /// What follows the number/bullet (`w:suff`): a tab (default), a space or nothing
    pub suffix: MarkerSuffix,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkerSuffix {
    Tab,
    Space,
    Nothing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineSpacing {
    Multiple(f32), // 1.0 = single
    Exact(f32),    // points
    AtLeast(f32),  // points
}

#[derive(Clone, Debug)]
pub struct ParaProps {
    pub align: Align,
    pub space_before: f32,
    pub space_after: f32,
    pub line: LineSpacing,
    pub indent_left: f32,
    pub indent_right: f32,
    pub first_line: f32, // negative = hanging
    pub page_break_before: bool,
    pub fill: Option<[u8; 3]>,
    pub contextual_spacing: bool,
    pub style_id: Option<String>,
    pub keep_with_next: bool,
    /// Has centre/right/decimal tab stops (headers, footers, "Title ...... page"):
    /// tabs then push text apart instead of jumping half an inch.
    /// Custom tab stops, measured from the left margin (sorted by position)
    pub tabs: Vec<TabStop>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabAlign {
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    pub pos: f32, // points from the left margin
    pub align: TabAlign,
    pub leader: Option<char>,
}

impl Default for ParaProps {
    fn default() -> Self {
        ParaProps {
            align: Align::Left,
            space_before: 0.0,
            space_after: 0.0,
            line: LineSpacing::Multiple(1.0),
            indent_left: 0.0,
            indent_right: 0.0,
            first_line: 0.0,
            page_break_before: false,
            fill: None,
            contextual_spacing: false,
            style_id: None,
            keep_with_next: false,
            tabs: vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VertAlign {
    #[default]
    Baseline,
    Super,
    Sub,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunProps {
    pub font: Option<String>,
    pub size: f32, // points
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<[u8; 3]>,
    pub highlight: Option<[u8; 3]>,
    pub vert: VertAlign,
    pub caps: bool,
    pub small_caps: bool,
    pub hidden: bool,
    /// Extra space between characters (`w:spacing`), points
    pub spacing: f32,
}

impl Default for RunProps {
    fn default() -> Self {
        RunProps {
            font: None,
            size: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
            highlight: None,
            vert: VertAlign::Baseline,
            caps: false,
            small_caps: false,
            hidden: false,
            spacing: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Inline {
    Text(String, RunProps),
    /// `float`: offset (pt) from the anchoring paragraph for a picture that text does
    /// not wrap around (in front of or behind the text); it then takes no space
    /// `on_page`: `float` is measured from the page's top-left corner instead
    Image { index: usize, width: f32, height: f32, float: Option<(f32, f32)>, on_page: bool },
    LineBreak,
    PageBreak,
    ColumnBreak,
    Tab,
    /// Positional tab (`w:ptab`): always pushes what follows to the given alignment
    AlignTab,
    /// Contents of a text box, shown where the box is anchored
    TextBox(Vec<Block>),
    PageNumber(RunProps),
    PageCount(RunProps),
    /// SECTIONPAGES: number of pages in the current section
    SectionPageCount(RunProps),
    /// A bookmark (target of PAGEREF / table-of-contents entries)
    Bookmark(String),
    /// PAGEREF: page of a bookmark, computed from the new layout; `cached` is Word's
    /// last value, used when the bookmark does not exist
    PageRef { bookmark: String, cached: String, props: RunProps },
}

/// One border line; `None` in a slot means "no line".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32, // points
    pub color: [u8; 3],
}

/// Table-level borders (outer sides plus the lines between cells).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TableBorders {
    pub top: Option<Border>,
    pub left: Option<Border>,
    pub bottom: Option<Border>,
    pub right: Option<Border>,
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
}

/// Per-cell overrides: `Some(None)` = explicitly no line, `None` = use the table's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellBorders {
    pub top: Option<Option<Border>>,
    pub left: Option<Option<Border>>,
    pub bottom: Option<Option<Border>>,
    pub right: Option<Option<Border>>,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub columns: Vec<f32>, // points
    pub rows: Vec<Row>,
    pub borders: TableBorders,
    pub align: Align,
    pub cell_margin_left: f32,
    pub cell_margin_right: f32,
    pub cell_margin_top: f32,
    pub cell_margin_bottom: f32,
    /// tblInd: from the left margin to the table's left edge (may be negative)
    pub indent: f32,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub header: bool,
    /// `w:trHeight`: (height in points, exact). Not exact = at least this tall.
    pub height: Option<(f32, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VMerge {
    None,
    Restart,
    Continue,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub span: usize,
    pub vmerge: VMerge,
    pub fill: Option<[u8; 3]>,
    pub valign: Align, // Left = top, Center = middle, Right = bottom
    pub borders: CellBorders,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub data: Vec<u8>,
    pub ext: String,
}

// ---------------------------------------------------------------------------
// Partial properties (as written in XML) and their resolution

#[derive(Clone, Debug, Default)]
struct RunPatch {
    font: Option<String>,
    font_theme: Option<String>,
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<Option<[u8; 3]>>,
    highlight: Option<Option<[u8; 3]>>,
    vert: Option<VertAlign>,
    caps: Option<bool>,
    small_caps: Option<bool>,
    hidden: Option<bool>,
    spacing: Option<f32>,
}

impl RunPatch {
    fn merge(&mut self, other: &RunPatch) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f.clone(); } )* };
        }
        take!(size, bold, italic, underline, strike, color, highlight, vert, caps, small_caps, hidden, spacing);
        if other.font.is_some() || other.font_theme.is_some() {
            self.font = other.font.clone();
            self.font_theme = other.font_theme.clone();
        }
    }

    fn apply(&self, base: &RunProps, theme: &Theme) -> RunProps {
        let mut r = base.clone();
        if let Some(theme_font) = &self.font_theme {
            if let Some(f) = theme.resolve(theme_font) {
                r.font = Some(f);
            }
        } else if let Some(f) = &self.font {
            r.font = Some(f.clone());
        }
        if let Some(v) = self.size {
            r.size = v;
        }
        if let Some(v) = self.spacing {
            r.spacing = v;
        }
        if let Some(v) = self.bold {
            r.bold = v;
        }
        if let Some(v) = self.italic {
            r.italic = v;
        }
        if let Some(v) = self.underline {
            r.underline = v;
        }
        if let Some(v) = self.strike {
            r.strike = v;
        }
        if let Some(v) = self.color {
            r.color = v;
        }
        if let Some(v) = self.highlight {
            r.highlight = v;
        }
        if let Some(v) = self.vert {
            r.vert = v;
        }
        if let Some(v) = self.caps {
            r.caps = v;
        }
        if let Some(v) = self.small_caps {
            r.small_caps = v;
        }
        if let Some(v) = self.hidden {
            r.hidden = v;
        }
        r
    }
}

#[derive(Clone, Debug, Default)]
struct ParaPatch {
    align: Option<Align>,
    space_before: Option<f32>,
    space_after: Option<f32>,
    line: Option<LineSpacing>,
    indent_left: Option<f32>,
    indent_right: Option<f32>,
    first_line: Option<f32>,
    page_break_before: Option<bool>,
    fill: Option<Option<[u8; 3]>>,
    contextual_spacing: Option<bool>,
    keep_with_next: Option<bool>,
    /// (stop, cleared): stops add up through the style chain; `clear` removes one
    tabs: Vec<(TabStop, bool)>,
    num: Option<(String, usize)>, // numId, level
}

impl ParaPatch {
    fn merge(&mut self, o: &ParaPatch) {
        macro_rules! take {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        take!(
            align,
            space_before,
            space_after,
            line,
            indent_left,
            indent_right,
            first_line,
            page_break_before,
            fill,
            contextual_spacing,
            keep_with_next,
            num
        );
        self.tabs.extend(o.tabs.iter().cloned());
    }

    fn apply(&self, base: &ParaProps) -> ParaProps {
        let mut p = base.clone();
        if let Some(v) = self.align {
            p.align = v;
        }
        if let Some(v) = self.space_before {
            p.space_before = v;
        }
        if let Some(v) = self.space_after {
            p.space_after = v;
        }
        if let Some(v) = self.line {
            p.line = v;
        }
        if let Some(v) = self.indent_left {
            p.indent_left = v;
        }
        if let Some(v) = self.indent_right {
            p.indent_right = v;
        }
        if let Some(v) = self.first_line {
            p.first_line = v;
        }
        if let Some(v) = self.page_break_before {
            p.page_break_before = v;
        }
        if let Some(v) = self.fill {
            p.fill = v;
        }
        if let Some(v) = self.contextual_spacing {
            p.contextual_spacing = v;
        }
        if let Some(v) = self.keep_with_next {
            p.keep_with_next = v;
        }
        let mut stops: Vec<TabStop> = vec![];
        for (stop, cleared) in &self.tabs {
            stops.retain(|t| (t.pos - stop.pos).abs() > 0.5);
            if !cleared {
                stops.push(*stop);
            }
        }
        stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
        p.tabs = stops;
        p
    }
}

#[derive(Clone, Debug, Default)]
struct Style {
    kind: String,
    name: String,
    based_on: Option<String>,
    run: RunPatch,
    para: ParaPatch,
    table_borders: Option<TableBorders>,
    /// Table styles: default cell padding by side (top, left, bottom, right)
    cell_margins: HashMap<&'static str, f32>,
}

#[derive(Default)]
struct Theme {
    major: Option<String>,
    minor: Option<String>,
}

impl Theme {
    fn resolve(&self, name: &str) -> Option<String> {
        if name.starts_with("major") {
            self.major.clone()
        } else if name.starts_with("minor") {
            self.minor.clone()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
struct Level {
    start: u32,
    format: String,
    text: String,
    para: ParaPatch,
    run: RunPatch,
    suffix: MarkerSuffix,
}

fn parse_level(l: Node) -> Level {
    Level {
        start: w(l, "start").and_then(val).and_then(|v| v.parse().ok()).unwrap_or(1),
        format: w(l, "numFmt").and_then(val).unwrap_or("decimal").to_string(),
        text: w(l, "lvlText").and_then(val).unwrap_or("").to_string(),
        para: w(l, "pPr").map(parse_ppr).unwrap_or_default(),
        run: w(l, "rPr").map(parse_rpr).unwrap_or_default(),
        suffix: match w(l, "suff").and_then(val) {
            Some("space") => MarkerSuffix::Space,
            Some("nothing") => MarkerSuffix::Nothing,
            _ => MarkerSuffix::Tab,
        },
    }
}

#[derive(Default)]
struct Numbering {
    // abstractNumId -> levels
    abstracts: HashMap<String, Vec<Level>>,
    // numId -> (abstractNumId, start overrides per level, whole-level overrides)
    nums: HashMap<String, (String, HashMap<usize, u32>, HashMap<usize, Level>)>,
}

// ---------------------------------------------------------------------------
// Parsing helpers

fn w<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name && c.tag_name().namespace() == Some(W))
}

fn wattr<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attribute((W, name))
}

fn val<'a>(node: Node<'a, 'a>) -> Option<&'a str> {
    wattr(node, "val")
}

/// `<w:b/>`, `<w:b w:val="1"/>`, `<w:b w:val="false"/>`…
fn toggle(node: Node) -> bool {
    !matches!(val(node), Some("0" | "false" | "off" | "none"))
}

fn twips(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().map(|v| v / 20.0)
}

fn hex_color(s: &str) -> Option<[u8; 3]> {
    if s.len() != 6 || s.eq_ignore_ascii_case("auto") {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn highlight_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "yellow" => [255, 255, 0],
        "green" => [0, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "blue" => [0, 0, 255],
        "red" => [255, 0, 0],
        "darkBlue" => [0, 0, 139],
        "darkCyan" => [0, 139, 139],
        "darkGreen" => [0, 100, 0],
        "darkMagenta" => [139, 0, 139],
        "darkRed" => [139, 0, 0],
        "darkYellow" => [128, 128, 0],
        "darkGray" => [169, 169, 169],
        "lightGray" => [211, 211, 211],
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        _ => return None,
    })
}

fn parse_rpr(rpr: Node) -> RunPatch {
    let mut p = RunPatch::default();
    for c in rpr.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match c.tag_name().name() {
            "rFonts" => {
                if let Some(t) = wattr(c, "asciiTheme").or(wattr(c, "hAnsiTheme")) {
                    p.font_theme = Some(t.to_string());
                    p.font = None;
                // Only the Latin font slots: `cs` (complex scripts) and `eastAsia` must not
                // replace the font used for Latin text
                } else if let Some(f) = wattr(c, "ascii").or(wattr(c, "hAnsi")) {
                    p.font = Some(f.to_string());
                    p.font_theme = None;
                }
            }
            "sz" => p.size = val(c).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 2.0),
            "b" => p.bold = Some(toggle(c)),
            "i" => p.italic = Some(toggle(c)),
            "u" => p.underline = Some(!matches!(val(c), Some("none" | "0" | "false"))),
            "strike" | "dstrike" => p.strike = Some(toggle(c)),
            "color" => p.color = Some(val(c).and_then(hex_color)),
            "highlight" => p.highlight = Some(val(c).and_then(highlight_color)),
            "shd" => {
                if let Some(fill) = wattr(c, "fill").and_then(hex_color) {
                    p.highlight = Some(Some(fill));
                }
            }
            "vertAlign" => {
                p.vert = Some(match val(c) {
                    Some("superscript") => VertAlign::Super,
                    Some("subscript") => VertAlign::Sub,
                    _ => VertAlign::Baseline,
                })
            }
            "caps" => p.caps = Some(toggle(c)),
            "smallCaps" => p.small_caps = Some(toggle(c)),
            "vanish" => p.hidden = Some(toggle(c)),
            "spacing" => p.spacing = val(c).and_then(twips),
            _ => {}
        }
    }
    p
}

fn parse_ppr(ppr: Node) -> ParaPatch {
    let mut p = ParaPatch::default();
    for c in ppr.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match c.tag_name().name() {
            "jc" => {
                p.align = Some(match val(c) {
                    Some("center") => Align::Center,
                    Some("right" | "end") => Align::Right,
                    Some("both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida") => Align::Justify,
                    _ => Align::Left,
                })
            }
            "spacing" => {
                if let Some(v) = wattr(c, "before").and_then(twips) {
                    p.space_before = Some(v);
                }
                if let Some(v) = wattr(c, "after").and_then(twips) {
                    p.space_after = Some(v);
                }
                if let Some(line) = wattr(c, "line").and_then(|v| v.parse::<f32>().ok()) {
                    p.line = Some(match wattr(c, "lineRule") {
                        Some("exact") => LineSpacing::Exact(line / 20.0),
                        Some("atLeast") => LineSpacing::AtLeast(line / 20.0),
                        _ => LineSpacing::Multiple(line / 240.0),
                    });
                }
            }
            "ind" => {
                if let Some(v) = wattr(c, "left").or(wattr(c, "start")).and_then(twips) {
                    p.indent_left = Some(v);
                }
                if let Some(v) = wattr(c, "right").or(wattr(c, "end")).and_then(twips) {
                    p.indent_right = Some(v);
                }
                if let Some(v) = wattr(c, "hanging").and_then(twips) {
                    p.first_line = Some(-v);
                } else if let Some(v) = wattr(c, "firstLine").and_then(twips) {
                    p.first_line = Some(v);
                }
            }
            "tabs" => {
                for t in c.children().filter(|t| t.tag_name().name() == "tab") {
                    let Some(pos) = wattr(t, "pos").and_then(twips) else { continue };
                    let align = match val(t) {
                        Some("center") => TabAlign::Center,
                        Some("right" | "end") => TabAlign::Right,
                        Some("decimal") => TabAlign::Decimal,
                        _ => TabAlign::Left,
                    };
                    let leader = match wattr(t, "leader") {
                        Some("dot") => Some('.'),
                        Some("hyphen") => Some('-'),
                        Some("underscore" | "heavy") => Some('_'),
                        Some("middleDot") => Some('·'),
                        _ => None,
                    };
                    let cleared = val(t) == Some("clear");
                    p.tabs.push((TabStop { pos, align, leader }, cleared));
                }
            }
            "pageBreakBefore" => p.page_break_before = Some(toggle(c)),
            "contextualSpacing" => p.contextual_spacing = Some(toggle(c)),
            "keepNext" => p.keep_with_next = Some(toggle(c)),
            "shd" => p.fill = Some(wattr(c, "fill").and_then(hex_color)),
            "numPr" => {
                let id = w(c, "numId").and_then(val).map(str::to_string);
                let level = w(c, "ilvl").and_then(val).and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(id) = id {
                    p.num = Some((id, level));
                }
            }
            _ => {}
        }
    }
    p
}

/// One `<w:top w:val="single" w:sz="4" w:color="000000"/>`-style border.
fn border(node: Node) -> Option<Border> {
    match val(node) {
        None | Some("nil" | "none") => None,
        Some(_) => {
            // `w:sz` is in eighths of a point; Word draws at least a hairline
            let width = wattr(node, "sz").and_then(|v| v.parse::<f32>().ok()).map(|v| (v / 8.0).max(0.25)).unwrap_or(0.5);
            let color = wattr(node, "color").and_then(hex_color).unwrap_or([0, 0, 0]);
            Some(Border { width, color })
        }
    }
}

fn table_borders(node: Node) -> TableBorders {
    let side = |names: &[&str]| names.iter().find_map(|n| w(node, n)).and_then(border);
    TableBorders {
        top: side(&["top"]),
        left: side(&["left", "start"]),
        bottom: side(&["bottom"]),
        right: side(&["right", "end"]),
        inside_h: side(&["insideH"]),
        inside_v: side(&["insideV"]),
    }
}

fn cell_borders(node: Node) -> CellBorders {
    let side = |names: &[&str]| names.iter().find_map(|n| w(node, n)).map(border);
    CellBorders { top: side(&["top"]), left: side(&["left", "start"]), bottom: side(&["bottom"]), right: side(&["right", "end"]) }
}

// ---------------------------------------------------------------------------
// Package (zip) access

struct Package {
    files: HashMap<String, Vec<u8>>,
}

impl Package {
    fn open(bytes: &[u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "not a valid Word document".to_string())?;
        let mut files = HashMap::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let Ok(name) = entry.name().map(|n| n.to_string()) else { continue };
            // Only what the converter reads: XML parts and media
            if !(name.ends_with(".xml") || name.ends_with(".rels") || name.starts_with("word/media/")) {
                continue;
            }
            let mut data = Vec::new();
            (&mut entry).take(MAX_PART_BYTES + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
            total += data.len() as u64;
            if data.len() as u64 > MAX_PART_BYTES || total > 4 * MAX_PART_BYTES {
                return Err("the document is too large".into());
            }
            files.insert(name, data);
        }
        Ok(Package { files })
    }

    fn text(&self, name: &str) -> Option<String> {
        self.files.get(name).map(|b| String::from_utf8_lossy(b).into_owned())
    }
}

/// Relationship id -> target path inside the package.
fn relationships(pkg: &Package, part: &str) -> HashMap<String, String> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_name = format!("{dir}/_rels/{file}.rels");
    let mut map = HashMap::new();
    let Some(text) = pkg.text(&rels_name) else { return map };
    let Ok(xml) = Xml::parse(&text) else { return map };
    for rel in xml.descendants().filter(|n| n.tag_name().name() == "Relationship") {
        if let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) {
            if rel.attribute("TargetMode") == Some("External") {
                continue; // never follow external links
            }
            map.insert(id.to_string(), resolve_path(dir, target));
        }
    }
    map
}

fn resolve_path(dir: &str, target: &str) -> String {
    let joined = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("{dir}/{target}") };
    let mut parts: Vec<&str> = vec![];
    for p in joined.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
// Reader

struct Reader<'p> {
    pkg: &'p Package,
    styles: HashMap<String, Style>,
    default_para_style: Option<String>,
    /// The table style used by tables that name none ("Normal Table")
    default_table_style: Option<String>,
    /// Documents from Word 2010 and older (compatibility mode < 15) place tables so that
    /// the text of the first cell, not its border, lines up with the margin
    legacy_tables: bool,
    doc_run: RunPatch,
    doc_para: ParaPatch,
    theme: Theme,
    numbering: Numbering,
    counters: HashMap<String, Vec<u32>>, // numId -> counter per level
    images: Vec<Image>,
    image_index: HashMap<String, usize>,
    bookmarks: std::collections::HashSet<String>,
    field: FieldState,
}

impl<'p> Reader<'p> {
    fn new(pkg: &'p Package) -> Self {
        let mut r = Reader {
            pkg,
            styles: HashMap::new(),
            default_para_style: None,
            default_table_style: None,
            legacy_tables: false,
            doc_run: RunPatch::default(),
            doc_para: ParaPatch::default(),
            theme: Theme::default(),
            numbering: Numbering::default(),
            counters: HashMap::new(),
            images: vec![],
            image_index: HashMap::new(),
            bookmarks: Default::default(),
            field: FieldState::default(),
        };
        r.load_theme();
        r.load_styles();
        r.load_numbering();
        r
    }

    fn load_theme(&mut self) {
        let Some(text) = self.pkg.text("word/theme/theme1.xml") else { return };
        let Ok(xml) = Xml::parse(&text) else { return };
        let font = |kind: &str| {
            xml.descendants()
                .find(|n| n.tag_name().name() == kind && n.tag_name().namespace() == Some(A))
                .and_then(|f| f.children().find(|c| c.tag_name().name() == "latin"))
                .and_then(|l| l.attribute("typeface"))
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        };
        self.theme.major = font("majorFont");
        self.theme.minor = font("minorFont");
    }

    fn load_styles(&mut self) {
        let Some(text) = self.pkg.text("word/styles.xml") else { return };
        let Ok(xml) = Xml::parse(&text) else { return };
        let root = xml.root_element();
        if let Some(defaults) = w(root, "docDefaults") {
            if let Some(rpr) = w(defaults, "rPrDefault").and_then(|n| w(n, "rPr")) {
                self.doc_run = parse_rpr(rpr);
            }
            if let Some(ppr) = w(defaults, "pPrDefault").and_then(|n| w(n, "pPr")) {
                self.doc_para = parse_ppr(ppr);
            }
        }
        for s in root.children().filter(|c| c.tag_name().name() == "style") {
            let Some(id) = wattr(s, "styleId") else { continue };
            let kind = wattr(s, "type").unwrap_or("paragraph").to_string();
            if kind == "paragraph" && wattr(s, "default").is_some_and(|v| v == "1" || v == "true") {
                self.default_para_style = Some(id.to_string());
            }
            if kind == "table" && wattr(s, "default").is_some_and(|v| v == "1" || v == "true") {
                self.default_table_style = Some(id.to_string());
            }
            let table_borders = w(s, "tblPr").and_then(|t| w(t, "tblBorders")).map(table_borders);
            let mut cell_margins = HashMap::new();
            if let Some(mar) = w(s, "tblPr").and_then(|t| w(t, "tblCellMar")) {
                for (side, alias) in [("top", "top"), ("bottom", "bottom"), ("left", "start"), ("right", "end")] {
                    if let Some(v) = w(mar, side).or_else(|| w(mar, alias)).and_then(|m| wattr(m, "w")).and_then(twips) {
                        cell_margins.insert(side, v);
                    }
                }
            }
            self.styles.insert(
                id.to_string(),
                Style {
                    kind,
                    name: w(s, "name").and_then(val).unwrap_or("").to_string(),
                    based_on: w(s, "basedOn").and_then(val).map(str::to_string),
                    run: w(s, "rPr").map(parse_rpr).unwrap_or_default(),
                    para: w(s, "pPr").map(parse_ppr).unwrap_or_default(),
                    table_borders,
                    cell_margins,
                },
            );
        }
    }

    fn load_numbering(&mut self) {
        let Some(text) = self.pkg.text("word/numbering.xml") else { return };
        let Ok(xml) = Xml::parse(&text) else { return };
        let root = xml.root_element();
        for a in root.children().filter(|c| c.tag_name().name() == "abstractNum") {
            let Some(id) = wattr(a, "abstractNumId") else { continue };
            let mut levels = vec![];
            for l in a.children().filter(|c| c.tag_name().name() == "lvl") {
                let idx: usize = wattr(l, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(levels.len());
                while levels.len() <= idx {
                    levels.push(Level {
                        start: 1,
                        format: "decimal".into(),
                        text: "%1.".into(),
                        para: ParaPatch::default(),
                        run: RunPatch::default(),
                        suffix: MarkerSuffix::Tab,
                    });
                }
                levels[idx] = parse_level(l);
            }
            self.numbering.abstracts.insert(id.to_string(), levels);
        }
        for n in root.children().filter(|c| c.tag_name().name() == "num") {
            let Some(id) = wattr(n, "numId") else { continue };
            let Some(abs) = w(n, "abstractNumId").and_then(val) else { continue };
            let mut overrides = HashMap::new();
            let mut level_overrides = HashMap::new();
            for o in n.children().filter(|c| c.tag_name().name() == "lvlOverride") {
                let Some(lvl) = wattr(o, "ilvl").and_then(|v| v.parse::<usize>().ok()) else { continue };
                if let Some(start) = w(o, "startOverride").and_then(val).and_then(|v| v.parse().ok()) {
                    overrides.insert(lvl, start);
                }
                // A full level definition replaces the abstract one for this list
                if let Some(l) = w(o, "lvl") {
                    level_overrides.insert(lvl, parse_level(l));
                }
            }
            self.numbering.nums.insert(id.to_string(), (abs.to_string(), overrides, level_overrides));
        }
    }

    /// Style chain from the most basic ancestor to `id` itself.
    fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out = vec![];
        let mut current = Some(id.to_string());
        let mut guard = 0;
        while let Some(cid) = current {
            let Some(style) = self.styles.get(&cid) else { break };
            out.push(style);
            current = style.based_on.clone();
            guard += 1;
            if guard > 32 {
                break; // cyclic basedOn
            }
        }
        out.reverse();
        out
    }

    fn para_style_patches(&self, style_id: Option<&str>) -> (ParaPatch, RunPatch) {
        let mut para = self.doc_para.clone();
        let mut run = self.doc_run.clone();
        let id = style_id.map(str::to_string).or_else(|| self.default_para_style.clone());
        if let Some(id) = id {
            for s in self.chain(&id) {
                para.merge(&s.para);
                run.merge(&s.run);
            }
        }
        (para, run)
    }

    fn base_run(&self) -> RunProps {
        let mut base = RunProps::default();
        if let Some(minor) = &self.theme.minor {
            base.font = Some(minor.clone());
        }
        base.size = 10.0; // Word's built-in default when nothing is specified
        base
    }

    // -- body ---------------------------------------------------------------

    fn blocks(&mut self, container: Node, rels: &HashMap<String, String>) -> Vec<Block> {
        let mut out = vec![];
        for child in container.children().filter(|c| c.is_element()) {
            match (child.tag_name().namespace(), child.tag_name().name()) {
                (Some(W), "p") => self.paragraph(child, rels, &mut out),
                (Some(W), "tbl") => out.push(Block::Table(self.table(child, rels))),
                (Some(W), "sdt") => {
                    if let Some(content) = w(child, "sdtContent") {
                        out.extend(self.blocks(content, rels));
                    }
                }
                (Some(W), "customXml" | "ins" | "smartTag") => out.extend(self.blocks(child, rels)),
                _ => {}
            }
        }
        out
    }

    fn paragraph(&mut self, p: Node, rels: &HashMap<String, String>, out: &mut Vec<Block>) {
        let ppr = w(p, "pPr");
        let style_id = ppr.and_then(|n| w(n, "pStyle")).and_then(val).map(str::to_string);
        let (mut para_patch, style_run) = self.para_style_patches(style_id.as_deref());
        let direct_para = ppr.map(parse_ppr).unwrap_or_default();

        // Numbering may come from the style or from the paragraph itself
        let num = direct_para.num.clone().or(para_patch.num.clone());
        let mut marker = None;
        let mut level_patch = ParaPatch::default();
        let base = self.base_run();
        let para_run = style_run.apply(&base, &self.theme);
        if let Some((num_id, level)) = num.filter(|(id, _)| id != "0") {
            if let Some((text, lvl_para, lvl_run, suffix)) = self.next_marker(&num_id, level) {
                level_patch = lvl_para;
                let mut run = para_run.clone();
                run = lvl_run.apply(&run, &self.theme);
                if let Some(t) = text {
                    marker = Some(ListMarker { text: t, run, suffix });
                }
            }
        }
        // Numbering indents sit between the style and the paragraph's own formatting
        para_patch.merge(&level_patch);
        para_patch.merge(&direct_para);
        let mut props = para_patch.apply(&ParaProps::default());
        props.style_id = style_id.clone();

        let mut inlines = vec![];
        // Fields can span paragraphs (a table of contents does), so the state persists
        let mut field = std::mem::take(&mut self.field);
        self.inlines(p, rels, &para_run, &mut inlines, &mut field);
        self.field = field;
        let display_math = p.children().any(|c| c.tag_name().name() == "oMathPara" && c.tag_name().namespace() == Some(M))
            && !p.children().any(|c| c.tag_name().name() == "r" && c.tag_name().namespace() == Some(W));
        if display_math && direct_para.align.is_none() {
            props.align = Align::Center;
        }

        let mark = match ppr.and_then(|n| w(n, "rPr")) {
            Some(rpr) => parse_rpr(rpr).apply(&para_run, &self.theme),
            None => para_run.clone(),
        };
        out.push(Block::Paragraph(Paragraph { props, inlines, marker, mark }));
    }

    fn inlines(&mut self, node: Node, rels: &HashMap<String, String>, para_run: &RunProps, out: &mut Vec<Inline>, field: &mut FieldState) {
        for c in node.children().filter(|c| c.is_element()) {
            match (c.tag_name().namespace(), c.tag_name().name()) {
                (Some(W), "r") => self.run(c, rels, para_run, out, field),
                (Some(W), "hyperlink" | "smartTag" | "customXml" | "ins" | "fldSimple" | "bdo" | "dir") => {
                    if c.tag_name().name() == "fldSimple" {
                        match field_name(wattr(c, "instr").unwrap_or("")).as_str() {
                            "PAGE" => {
                                out.push(Inline::PageNumber(para_run.clone()));
                                continue;
                            }
                            "NUMPAGES" => {
                                out.push(Inline::PageCount(para_run.clone()));
                                continue;
                            }
                            "SECTIONPAGES" => {
                                out.push(Inline::SectionPageCount(para_run.clone()));
                                continue;
                            }
                            _ => {}
                        }
                    }
                    self.inlines(c, rels, para_run, out, field)
                }
                (Some(W), "sdt") => {
                    if let Some(content) = w(c, "sdtContent") {
                        self.inlines(content, rels, para_run, out, field);
                    }
                }
                // Deleted text (tracked changes) is not part of the final document
                (Some(W), "del" | "moveFrom") => {}
                (Some(W), "bookmarkStart") => {
                    if let Some(name) = wattr(c, "name") {
                        let label = bookmark_label(name);
                        self.bookmarks.insert(label.clone());
                        out.push(Inline::Bookmark(label));
                    }
                }
                // Equations: their text, in reading order (variables in italic, as Word shows them)
                (Some(M), "oMathPara" | "oMath") => self.math(c, para_run, out),
                _ => {}
            }
        }
    }

    fn run(&mut self, r: Node, rels: &HashMap<String, String>, para_run: &RunProps, out: &mut Vec<Inline>, field: &mut FieldState) {
        let rpr = w(r, "rPr");
        let mut patch = RunPatch::default();
        if let Some(style) = rpr.and_then(|n| w(n, "rStyle")).and_then(val) {
            // Word drops the blue "Hyperlink" look from table-of-contents entries when it
            // updates them, so it is ignored there
            let skip = field.in_toc() && self.styles.get(style).is_some_and(|s| s.name.eq_ignore_ascii_case("hyperlink"));
            if !skip {
                for s in self.chain(style) {
                    if s.kind == "character" {
                        patch.merge(&s.run);
                    }
                }
            }
        }
        if let Some(rpr) = rpr {
            patch.merge(&parse_rpr(rpr));
        }
        let props = patch.apply(para_run, &self.theme);
        if props.hidden {
            return;
        }

        for c in r.children().filter(|c| c.is_element()) {
            let name = c.tag_name().name();
            // Complex fields: keep PAGE/NUMPAGES live, otherwise show the cached result
            match name {
                "fldChar" => {
                    match wattr(c, "fldCharType") {
                        Some("begin") => field.stack.push(FieldFrame::default()),
                        Some("separate") => {
                            let visible_outside = field.stack.len() <= 1 || field.stack[..field.stack.len() - 1].iter().all(|f| f.in_result && !f.replaced);
                            if let Some(top) = field.stack.last_mut() {
                                top.in_result = true;
                                // Page numbers stay live; everything else shows Word's cached result
                                let live = match field_name(&top.instr).as_str() {
                                    "PAGEREF" => top.instr.split_whitespace().nth(1).map(|b| Inline::PageRef {
                                        bookmark: bookmark_label(b),
                                        cached: String::new(),
                                        props: props.clone(),
                                    }),
                                    "PAGE" => Some(Inline::PageNumber(props.clone())),
                                    "NUMPAGES" => Some(Inline::PageCount(props.clone())),
                                    "SECTIONPAGES" => Some(Inline::SectionPageCount(props.clone())),
                                    _ => None,
                                };
                                if let Some(live) = live {
                                    top.replaced = true;
                                    if visible_outside {
                                        out.push(live);
                                    }
                                }
                            }
                        }
                        Some("end") => {
                            field.stack.pop();
                        }
                        _ => {}
                    }
                    continue;
                }
                "instrText" => {
                    if let Some(top) = field.stack.last_mut() {
                        if !top.in_result {
                            top.instr.push_str(c.text().unwrap_or(""));
                        }
                    }
                    continue;
                }
                _ => {}
            }
            if !field.visible() {
                // Keep a PAGEREF's cached value as its fallback
                if name == "t" && field.stack.last().is_some_and(|f| f.replaced && f.in_result && field_name(&f.instr) == "PAGEREF") {
                    if let Some(Inline::PageRef { cached, .. }) = out.iter_mut().rev().find(|i| matches!(i, Inline::PageRef { .. })) {
                        cached.push_str(c.text().unwrap_or(""));
                    }
                }
                continue; // field code, or a cached value replaced by a live field
            }
            match name {
                "t" => {
                    let text = c.text().unwrap_or("");
                    if !text.is_empty() {
                        out.push(Inline::Text(text.to_string(), props.clone()));
                    }
                }
                "tab" => out.push(Inline::Tab),
                "ptab" => out.push(Inline::AlignTab),
                // Newer drawing markup with a fallback for old readers: take the first choice
                "AlternateContent" => {
                    if let Some(choice) = c.children().find(|n| n.tag_name().name() == "Choice").or_else(|| c.children().find(|n| n.tag_name().name() == "Fallback")) {
                        for item in choice.children().filter(|n| n.is_element()) {
                            match item.tag_name().name() {
                                "drawing" => self.drawing(item, rels, out),
                                "pict" => self.vml_image(item, rels, out),
                                _ => {}
                            }
                        }
                    }
                }
                "br" | "cr" => out.push(match wattr(c, "type") {
                    Some("page") => Inline::PageBreak,
                    Some("column") => Inline::ColumnBreak,
                    _ => Inline::LineBreak,
                }),
                "noBreakHyphen" => out.push(Inline::Text("\u{2011}".into(), props.clone())),
                "softHyphen" => out.push(Inline::Text("\u{00AD}".into(), props.clone())),
                "sym" => {
                    let ch = wattr(c, "char").and_then(|h| u32::from_str_radix(h, 16).ok()).map(symbol_char);
                    if let Some(ch) = ch {
                        out.push(Inline::Text(ch.to_string(), props.clone()));
                    }
                }
                "drawing" => self.drawing(c, rels, out),
                "pict" | "object" => self.vml_image(c, rels, out),
                "lastRenderedPageBreak" => {}
                _ => {}
            }
        }
    }

    fn math(&self, node: Node, para_run: &RunProps, out: &mut Vec<Inline>) {
        for r in node.descendants().filter(|n| n.tag_name().name() == "r" && n.tag_name().namespace() == Some(M)) {
            let text: String = r
                .children()
                .filter(|c| c.tag_name().name() == "t" && c.tag_name().namespace() == Some(M))
                .filter_map(|t| t.text())
                .collect();
            if text.is_empty() {
                continue;
            }
            let mut props = match w(r, "rPr") {
                Some(rpr) => parse_rpr(rpr).apply(para_run, &self.theme),
                None => para_run.clone(),
            };
            // `m:sty val="p"` = plain (upright); otherwise letters are italic in Word's math
            let plain = r
                .children()
                .find(|c| c.tag_name().name() == "rPr" && c.tag_name().namespace() == Some(M))
                .and_then(|p| p.children().find(|c| c.tag_name().name() == "sty"))
                .and_then(|s| s.attribute((M, "val")))
                == Some("p");
            props.italic = !plain && text.chars().any(char::is_alphabetic);
            out.push(Inline::Text(text, props));
        }
    }

    fn drawing(&mut self, d: Node, rels: &HashMap<String, String>, out: &mut Vec<Inline>) {
        let Some(holder) = d.children().find(|c| c.is_element() && (c.tag_name().name() == "inline" || c.tag_name().name() == "anchor")) else { return };
        let extent = holder.children().find(|c| c.tag_name().name() == "extent" && c.tag_name().namespace() == Some(WP));
        let (cx, cy) = extent
            .map(|e| {
                (
                    e.attribute("cx").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
                    e.attribute("cy").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
                )
            })
            .unwrap_or((0.0, 0.0));
        if let Some(text_box) = holder.descendants().find(|n| n.tag_name().name() == "txbxContent" && n.tag_name().namespace() == Some(W)) {
            let blocks = self.blocks(text_box, rels);
            if !blocks.is_empty() {
                out.push(Inline::TextBox(blocks));
            }
            return;
        }
        let blip = holder.descendants().find(|n| n.tag_name().name() == "blip" && n.tag_name().namespace() == Some(A));
        let Some(rid) = blip.and_then(|b| b.attribute((R, "embed"))) else { return };
        // Anchored without text wrapping: positioned over the page, taking no space.
        // Only offsets relative to the anchoring column/paragraph can be placed reliably.
        let floating = holder.tag_name().name() == "anchor"
            && holder.children().any(|c| c.tag_name().namespace() == Some(WP) && c.tag_name().name() == "wrapNone");
        let position = |axis: &str| holder.children().find(|c| c.tag_name().namespace() == Some(WP) && c.tag_name().name() == axis);
        let relative = |axis: &str| position(axis).and_then(|p| p.attribute("relativeFrom")).unwrap_or("column");
        // Fixed to the page (pictures behind/in front of the text placed on the paper)
        let on_page = floating && relative("positionH") == "page" && relative("positionV") == "page";
        let float = floating.then(|| {
            let offset = |axis: &str| -> f32 {
                position(axis)
                    .filter(|p| on_page || matches!(p.attribute("relativeFrom"), Some("column" | "character" | "paragraph" | "line")))
                    .and_then(|p| p.children().find(|c| c.tag_name().name() == "posOffset"))
                    .and_then(|o| o.text())
                    .and_then(|t| t.trim().parse::<f64>().ok())
                    .map(|v| (v / 12700.0) as f32)
                    .unwrap_or(0.0)
            };
            (offset("positionH"), offset("positionV"))
        });
        if let Some(index) = self.image(rid, rels) {
            out.push(Inline::Image { index, width: (cx / 12700.0) as f32, height: (cy / 12700.0) as f32, float, on_page });
        }
    }

    fn vml_image(&mut self, pict: Node, rels: &HashMap<String, String>, out: &mut Vec<Inline>) {
        if let Some(text_box) = pict.descendants().find(|n| n.tag_name().name() == "txbxContent" && n.tag_name().namespace() == Some(W)) {
            let blocks = self.blocks(text_box, rels);
            if !blocks.is_empty() {
                out.push(Inline::TextBox(blocks));
            }
            return;
        }
        let Some(data) = pict.descendants().find(|n| n.tag_name().name() == "imagedata" && n.tag_name().namespace() == Some(V)) else { return };
        let Some(rid) = data.attribute((R, "id")) else { return };
        let style = pict.descendants().find(|n| n.tag_name().name() == "shape").and_then(|s| s.attribute("style")).unwrap_or("");
        let dim = |key: &str| -> f32 {
            style
                .split(';')
                .find_map(|kv| kv.trim().strip_prefix(key).and_then(|v| v.strip_prefix(':')))
                .and_then(|v| v.trim().strip_suffix("pt").and_then(|n| n.parse().ok()))
                .unwrap_or(0.0)
        };
        if let Some(index) = self.image(rid, rels) {
            out.push(Inline::Image { index, width: dim("width"), height: dim("height"), float: None, on_page: false });
        }
    }

    fn image(&mut self, rid: &str, rels: &HashMap<String, String>) -> Option<usize> {
        let target = rels.get(rid)?;
        if let Some(&i) = self.image_index.get(target) {
            return Some(i);
        }
        let data = self.pkg.files.get(target)?.clone();
        let ext = target.rsplit('.').next().unwrap_or("").to_lowercase();
        self.images.push(Image { data, ext });
        let index = self.images.len() - 1;
        self.image_index.insert(target.clone(), index);
        Some(index)
    }

    /// Advances the list counter and returns the marker text plus the level's formatting.
    fn next_marker(&mut self, num_id: &str, level: usize) -> Option<(Option<String>, ParaPatch, RunPatch, MarkerSuffix)> {
        let (abs_id, overrides, level_overrides) = self.numbering.nums.get(num_id)?.clone();
        let mut levels = self.numbering.abstracts.get(&abs_id).cloned().unwrap_or_default();
        for (i, l) in level_overrides {
            while levels.len() <= i {
                levels.push(l.clone());
            }
            levels[i] = l;
        }
        let lvl = levels.get(level)?.clone();
        let starts: Vec<u32> = levels.iter().enumerate().map(|(i, l)| *overrides.get(&i).unwrap_or(&l.start)).collect();

        // Lists built on the same definition continue each other's numbering, as in
        // Word, unless this list explicitly restarts (startOverride)
        let key = if overrides.is_empty() { format!("abstract:{abs_id}") } else { format!("num:{num_id}") };
        let counters = self.counters.entry(key).or_insert_with(|| vec![0; levels.len().max(9)]);
        if counters.len() <= level {
            counters.resize(level + 1, 0);
        }
        counters[level] = if counters[level] == 0 { starts[level] } else { counters[level] + 1 };
        for deeper in counters.iter_mut().skip(level + 1) {
            *deeper = 0;
        }
        let snapshot = counters.clone();

        if lvl.format == "none" {
            return Some((None, lvl.para, lvl.run, lvl.suffix));
        }
        let text = if lvl.format == "bullet" {
            bullet_char(&lvl.text, lvl.run.font.as_deref())
        } else {
            let mut t = lvl.text.clone();
            for (i, l) in levels.iter().enumerate().take(level + 1) {
                let n = if snapshot[i] == 0 { starts[i] } else { snapshot[i] };
                let fmt = if lvl.format == "decimal" && i != level { "decimal" } else { l.format.as_str() };
                t = t.replace(&format!("%{}", i + 1), &format_number(n, fmt));
            }
            t
        };
        Some((Some(text), lvl.para, lvl.run, lvl.suffix))
    }

    // -- tables -------------------------------------------------------------

    fn table(&mut self, tbl: Node, rels: &HashMap<String, String>) -> Table {
        let tblpr = w(tbl, "tblPr");
        let columns: Vec<f32> = w(tbl, "tblGrid")
            .map(|g| g.children().filter(|c| c.tag_name().name() == "gridCol").filter_map(|c| wattr(c, "w").and_then(twips)).collect())
            .unwrap_or_default();

        // Borders: direct table borders, else the table style's, else none
        let mut borders = tblpr.and_then(|p| w(p, "tblBorders")).map(table_borders);
        if borders.is_none() {
            if let Some(style) = tblpr.and_then(|p| w(p, "tblStyle")).and_then(val) {
                borders = self.chain(style).iter().rev().find_map(|s| s.table_borders);
            }
        }
        let align = match tblpr.and_then(|p| w(p, "jc")).and_then(val) {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            _ => Align::Left,
        };
        // Cell padding: the table's own, else its style's (nearest in the chain)
        let style_id = tblpr.and_then(|p| w(p, "tblStyle")).and_then(val).map(str::to_string).or_else(|| self.default_table_style.clone());
        let style_margins: HashMap<&str, f32> = {
            let chain = style_id.map(|s| self.chain(&s)).unwrap_or_default();
            ["top", "bottom", "left", "right"].into_iter().filter_map(|side| chain.iter().find_map(|st| st.cell_margins.get(side).copied()).map(|v| (side, v))).collect()
        };
        let margin = |side: &str| -> Option<f32> {
            let alias = match side {
                "left" => "start",
                "right" => "end",
                other => other,
            };
            tblpr
                .and_then(|p| w(p, "tblCellMar"))
                .and_then(|m| w(m, side).or_else(|| w(m, alias)))
                .and_then(|s| wattr(s, "w"))
                .and_then(twips)
                .or_else(|| style_margins.get(side).copied())
        };

        let mut rows = vec![];
        for tr in tbl.children().filter(|c| c.tag_name().name() == "tr" && c.tag_name().namespace() == Some(W)) {
            let header = w(tr, "trPr").and_then(|p| w(p, "tblHeader")).is_some_and(toggle);
            let height = w(tr, "trPr").and_then(|p| w(p, "trHeight")).and_then(|h| {
                let v = wattr(h, "val").and_then(twips)?;
                (v > 0.0).then_some((v, wattr(h, "hRule") == Some("exact")))
            });
            let mut cells = vec![];
            for tc in tr.children().filter(|c| c.is_element()) {
                let tc = match tc.tag_name().name() {
                    "tc" => tc,
                    // Content controls wrapping a cell
                    "sdt" => match w(tc, "sdtContent").and_then(|c| w(c, "tc")) {
                        Some(cell) => cell,
                        None => continue,
                    },
                    _ => continue,
                };
                let tcpr = w(tc, "tcPr");
                let span = tcpr.and_then(|p| w(p, "gridSpan")).and_then(val).and_then(|v| v.parse().ok()).unwrap_or(1usize).max(1);
                let vmerge = match tcpr.and_then(|p| w(p, "vMerge")) {
                    Some(m) if val(m) == Some("restart") => VMerge::Restart,
                    Some(_) => VMerge::Continue,
                    None => VMerge::None,
                };
                let fill = tcpr.and_then(|p| w(p, "shd")).and_then(|s| wattr(s, "fill")).and_then(hex_color);
                let valign = match tcpr.and_then(|p| w(p, "vAlign")).and_then(val) {
                    Some("center") => Align::Center,
                    Some("bottom") => Align::Right,
                    _ => Align::Left,
                };
                let borders = tcpr.and_then(|p| w(p, "tcBorders")).map(cell_borders).unwrap_or_default();
                let blocks = self.blocks(tc, rels);
                cells.push(Cell { blocks, span, vmerge, fill, valign, borders });
            }
            rows.push(Row { cells, header, height });
        }

        Table {
            columns,
            rows,
            borders: borders.unwrap_or_default(),
            align,
            cell_margin_left: margin("left").unwrap_or(5.4),
            cell_margin_right: margin("right").unwrap_or(5.4),
            // Word's default: no padding above and below the text of a cell
            cell_margin_top: margin("top").unwrap_or(0.0),
            cell_margin_bottom: margin("bottom").unwrap_or(0.0),
            indent: {
                let ind = tblpr.and_then(|p| w(p, "tblInd")).filter(|i| wattr(*i, "type").is_none_or(|t| t == "dxa")).and_then(|i| wattr(i, "w")).and_then(twips).unwrap_or(0.0);
                if self.legacy_tables && align == Align::Left { ind - margin("left").unwrap_or(5.4) } else { ind }
            },
        }
    }

    // -- sections, header, footer -----------------------------------------

    fn page_setup(&self, sect: Option<Node>) -> PageSetup {
        let mut page = PageSetup::default();
        let Some(sect) = sect else { return page };
        if let Some(sz) = w(sect, "pgSz") {
            if let Some(v) = wattr(sz, "w").and_then(twips) {
                page.width = v;
            }
            if let Some(v) = wattr(sz, "h").and_then(twips) {
                page.height = v;
            }
        }
        if let Some(m) = w(sect, "pgMar") {
            let get = |k: &str| wattr(m, k).and_then(twips).map(f32::abs);
            page.margin_top = get("top").unwrap_or(page.margin_top);
            page.margin_bottom = get("bottom").unwrap_or(page.margin_bottom);
            page.margin_left = get("left").or(get("start")).unwrap_or(page.margin_left);
            page.margin_right = get("right").or(get("end")).unwrap_or(page.margin_right);
            page.header_distance = get("header").unwrap_or(page.header_distance);
            page.footer_distance = get("footer").unwrap_or(page.footer_distance);
        }
        if let Some(cols) = w(sect, "cols") {
            page.columns = wattr(cols, "num").and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 6);
        }
        page
    }

    fn part_blocks(&mut self, target: &str) -> Vec<Block> {
        let Some(text) = self.pkg.text(target) else { return vec![] };
        let Ok(xml) = Xml::parse(&text) else { return vec![] };
        let part_rels = relationships(self.pkg, target);
        let body_fields = std::mem::take(&mut self.field);
        let blocks = self.blocks(xml.root_element(), &part_rels);
        self.field = body_fields;
        blocks
    }

    fn section_start(sect: Node) -> SectionStart {
        match w(sect, "type").and_then(val) {
            Some("continuous") => SectionStart::Continuous,
            Some("evenPage") => SectionStart::EvenPage,
            Some("oddPage") => SectionStart::OddPage,
            _ => SectionStart::NextPage,
        }
    }
}

/// Word fields can nest (a table of contents is a TOC field full of PAGEREF fields).
#[derive(Default)]
struct FieldState {
    stack: Vec<FieldFrame>,
}

#[derive(Default)]
struct FieldFrame {
    instr: String,
    in_result: bool,
    replaced: bool,
}

/// Bookmark names become Typst labels: keep a safe character set.
pub fn bookmark_label(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

impl FieldState {
    fn in_toc(&self) -> bool {
        self.stack.iter().any(|f| f.in_result && field_name(&f.instr) == "TOC")
    }

    /// Text is shown only when it is part of every open field's displayed result and
    /// no open field is being replaced by a live value.
    fn visible(&self) -> bool {
        self.stack.iter().all(|f| f.in_result && !f.replaced)
    }
}

/// First word of a field instruction: `PAGEREF _Toc123 \h` -> "PAGEREF"
fn field_name(instr: &str) -> String {
    instr.split_whitespace().next().unwrap_or("").to_uppercase()
}

/// Bullets are often stored as private-use characters of the Symbol/Wingdings fonts.
fn bullet_char(text: &str, font: Option<&str>) -> String {
    let font = font.unwrap_or("").to_lowercase();
    let ch = text.chars().next().unwrap_or('•');
    let mapped = match (ch as u32, font.as_str()) {
        (0xF0B7, _) | (0xB7, "symbol") => '•',
        (0xF0A7, _) | (0xA7, "wingdings") => '▪',
        (0xF0D8, _) | (0xD8, "wingdings") => '➢',
        (0xF0FC, _) | (0xFC, "wingdings") => '✓',
        (0xF076, _) | (0x76, "wingdings") => '❖',
        (0xF06E, _) | (0x6E, "wingdings") => '■',
        (0xF0A8, _) => '◆',
        (_, "courier new") if ch == 'o' => '◦',
        (c, _) if (0xF000..=0xF0FF).contains(&c) => '•',
        _ => ch,
    };
    mapped.to_string()
}

fn symbol_char(code: u32) -> char {
    let code = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
    match code {
        0xB7 => '•',
        0xA7 => '▪',
        0xFC => '✓',
        0xD8 => '➢',
        _ => char::from_u32(code).unwrap_or('•'),
    }
}

pub fn format_number(n: u32, format: &str) -> String {
    match format {
        "lowerLetter" => letters(n, false),
        "upperLetter" => letters(n, true),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        "decimalZero" => format!("{n:02}"),
        "ordinal" => format!("{n}."),
        _ => n.to_string(),
    }
}

fn letters(n: u32, upper: bool) -> String {
    // Word repeats letters after z: a..z, aa..zz, aaa…
    let n = n.max(1) - 1;
    let ch = (b'a' + (n % 26) as u8) as char;
    let s: String = std::iter::repeat_n(ch, (n / 26 + 1) as usize).collect();
    if upper { s.to_uppercase() } else { s }
}

fn roman(mut n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

// ---------------------------------------------------------------------------

pub fn read(bytes: &[u8]) -> Result<Document, String> {
    let pkg = Package::open(bytes)?;
    // The main document part is usually word/document.xml; follow the package rels to be sure
    let main = main_part(&pkg).ok_or("this file has no Word document inside")?;
    let text = pkg.text(&main).ok_or("this file has no Word document inside")?;
    let xml = Xml::parse(&text).map_err(|_| "the document text is damaged".to_string())?;
    let body = w(xml.root_element(), "body").ok_or("the document has no body")?;
    let rels = relationships(&pkg, &main);

    let mut reader = Reader::new(&pkg);
    let settings = pkg.text("word/settings.xml");
    let settings_xml = settings.as_deref().and_then(|t| Xml::parse(t).ok());
    let even_odd = settings_xml
        .as_ref()
        .is_some_and(|x| x.descendants().any(|n| n.tag_name().name() == "evenAndOddHeaders" && toggle(n)));
    let default_tab = settings_xml
        .as_ref()
        .and_then(|x| x.descendants().find(|n| n.tag_name().name() == "defaultTabStop").and_then(val).and_then(twips))
        .filter(|v| *v > 1.0)
        .unwrap_or(36.0);
    // No compatibility mode at all: an old document (Word treats it like Word 2007)
    let compat_mode = settings_xml
        .as_ref()
        .and_then(|x| {
            x.descendants()
                .find(|n| n.tag_name().name() == "compatSetting" && wattr(*n, "name") == Some("compatibilityMode"))
                .and_then(|n| wattr(n, "val"))
                .and_then(|v| v.parse::<u32>().ok())
        })
        .unwrap_or(12);
    reader.legacy_tables = compat_mode < 15;

    // A paragraph carrying a <w:sectPr> ends a section; the body's own sectPr is the last one
    let mut parts: Vec<(Vec<Block>, Option<Node>)> = vec![];
    let mut current: Vec<Block> = vec![];
    for child in body.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match child.tag_name().name() {
            "p" => {
                reader.paragraph(child, &rels, &mut current);
                if let Some(sect) = w(child, "pPr").and_then(|p| w(p, "sectPr")) {
                    parts.push((std::mem::take(&mut current), Some(sect)));
                }
            }
            "tbl" => current.push(Block::Table(reader.table(child, &rels))),
            "sectPr" => parts.push((std::mem::take(&mut current), Some(child))),
            "sdt" | "customXml" | "ins" | "smartTag" => {
                let container = if child.tag_name().name() == "sdt" { w(child, "sdtContent") } else { Some(child) };
                if let Some(c) = container {
                    current.extend(reader.blocks(c, &rels));
                }
            }
            _ => {}
        }
    }
    if !current.is_empty() || parts.is_empty() {
        parts.push((current, None));
    }

    // Headers/footers are inherited per type from the previous section, as in Word
    let mut inherited: HashMap<(&str, &str), String> = HashMap::new();
    let mut cache: HashMap<String, Vec<Block>> = HashMap::new();
    let mut sections = vec![];
    for (blocks, sect) in parts {
        if let Some(sect) = sect {
            for kind in ["header", "footer"] {
                let tag = format!("{kind}Reference");
                for r in sect.children().filter(|c| c.tag_name().name() == tag) {
                    let which = match wattr(r, "type").unwrap_or("default") {
                        "first" => "first",
                        "even" => "even",
                        _ => "default",
                    };
                    if let Some(target) = r.attribute((R, "id")).and_then(|id| rels.get(id)) {
                        inherited.insert((kind, which), target.clone());
                    }
                }
            }
        }
        let title_page = sect.and_then(|s| w(s, "titlePg")).is_some_and(toggle);
        let mut load = |kind: &'static str, which: &'static str| -> Vec<Block> {
            let Some(target) = inherited.get(&(kind, which)).cloned() else { return vec![] };
            if let Some(b) = cache.get(&target) {
                return b.clone();
            }
            let b = reader.part_blocks(&target);
            cache.insert(target, b.clone());
            b
        };
        let mut hf = |kind: &'static str| HeaderFooter {
            default: load(kind, "default"),
            first: title_page.then(|| load(kind, "first")),
            even: even_odd.then(|| load(kind, "even")),
        };
        let header = hf("header");
        let footer = hf("footer");
        sections.push(Section {
            page: reader.page_setup(sect),
            header,
            footer,
            blocks,
            start: sect.map(Reader::section_start).unwrap_or(SectionStart::NextPage),
            page_start: sect.and_then(|s| w(s, "pgNumType")).and_then(|n| wattr(n, "start")).and_then(|v| v.parse().ok()),
        });
    }

    let (_, style_run) = reader.para_style_patches(None);
    let base = reader.base_run();
    let default_run = style_run.apply(&base, &reader.theme);
    let lang = document_language(&pkg);

    Ok(Document {
        sections,
        images: reader.images,
        default_font: default_run.font.unwrap_or_else(|| "Calibri".into()),
        default_size: default_run.size,
        lang,
        default_tab,
        legacy_layout: compat_mode < 15,
        justify_breaks: !settings_xml.as_ref().is_some_and(|x| x.descendants().any(|n| n.tag_name().name() == "doNotExpandShiftReturn" && toggle(n))),
        bookmarks: std::mem::take(&mut reader.bookmarks),
    })
}

fn main_part(pkg: &Package) -> Option<String> {
    let text = pkg.text("_rels/.rels")?;
    let xml = Xml::parse(&text).ok()?;
    xml.descendants()
        .filter(|n| n.tag_name().name() == "Relationship")
        .find(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/officeDocument")))
        .and_then(|n| n.attribute("Target"))
        .map(|t| resolve_path("", t))
        .filter(|p| pkg.files.contains_key(p))
        .or_else(|| pkg.files.contains_key("word/document.xml").then(|| "word/document.xml".to_string()))
}

fn document_language(pkg: &Package) -> Option<String> {
    let text = pkg.text("word/styles.xml")?;
    let xml = Xml::parse(&text).ok()?;
    xml.descendants()
        .find(|n| n.tag_name().name() == "lang" && n.tag_name().namespace() == Some(W))
        .and_then(val)
        .and_then(|l| l.split('-').next())
        .map(|l| l.to_lowercase())
        .filter(|l| l.len() == 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_like_word() {
        assert_eq!(format_number(3, "lowerLetter"), "c");
        assert_eq!(format_number(27, "lowerLetter"), "aa");
        assert_eq!(format_number(4, "upperRoman"), "IV");
        assert_eq!(format_number(14, "lowerRoman"), "xiv");
        assert_eq!(format_number(7, "decimal"), "7");
    }

    #[test]
    fn relationship_paths_resolve() {
        assert_eq!(resolve_path("word", "media/image1.png"), "word/media/image1.png");
        assert_eq!(resolve_path("word", "../customXml/item1.xml"), "customXml/item1.xml");
        assert_eq!(resolve_path("word", "/word/media/a.png"), "word/media/a.png");
        // Cannot escape the package root
        assert_eq!(resolve_path("word", "../../../etc/passwd"), "etc/passwd");
    }

    #[test]
    fn rejects_non_documents() {
        assert!(read(b"not a zip").is_err());
    }
}
