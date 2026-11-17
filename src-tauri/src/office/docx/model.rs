//! The document model `read` returns: sections with their page setup, header and footer,
//! and their blocks (paragraphs, tables, images) with every style resolved

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
