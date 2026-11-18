//! The workbook model `read` returns: sheets with their cells, resolved styles, sizes in
//! points, merges, drawings and print setup

use super::*;

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
    /// diagonal lines: bottom-left to top-right, top-left to bottom-right
    pub up: Option<Edge>,
    pub down: Option<Edge>,
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
    Chart(Anchor, Box<crate::office::chart::Chart>),
    Shapes(Anchor, Vec<crate::office::drawing::Shape>),
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
    pub chart_parts: Vec<ChartPart>,
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
    pub formulas: HashMap<(u32, u32), crate::office::formula::Pending>,
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
