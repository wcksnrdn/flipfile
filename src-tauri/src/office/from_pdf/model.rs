//! Geometry (top-down: `t` < `b`, in points from the page's top-left corner)

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::office) struct Rect {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl Rect {
    pub fn w(&self) -> f32 {
        self.r - self.l
    }
    pub fn h(&self) -> f32 {
        self.b - self.t
    }
    pub(super) fn cx(&self) -> f32 {
        (self.l + self.r) / 2.0
    }
    pub(super) fn cy(&self) -> f32 {
        (self.t + self.b) / 2.0
    }
    pub(super) fn union(&self, o: &Rect) -> Rect {
        Rect { l: self.l.min(o.l), t: self.t.min(o.t), r: self.r.max(o.r), b: self.b.max(o.b) }
    }
    /// Distance between the two rectangles (0 when they touch or overlap)
    pub(super) fn gap(&self, o: &Rect) -> f32 {
        let dx = (o.l - self.r).max(self.l - o.r).max(0.0);
        let dy = (o.t - self.b).max(self.t - o.b).max(0.0);
        dx.max(dy)
    }
    pub(super) fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.l && x <= self.r && y >= self.t && y <= self.b
    }
    pub(super) fn overlap_area(&self, o: &Rect) -> f32 {
        let w = (self.r.min(o.r) - self.l.max(o.l)).max(0.0);
        let h = (self.b.min(o.b) - self.t.max(o.t)).max(0.0);
        w * h
    }
    pub(super) fn area(&self) -> f32 {
        self.w().max(0.0) * self.h().max(0.0)
    }
}

// ---------------------------------------------------------------------------
// Page model, shared with the writer

#[derive(Clone, Debug, PartialEq)]
pub(in crate::office) struct Style {
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<[u8; 3]>,
    /// Extra space between characters (letter-spaced text), points
    pub spacing: f32,
    /// Raised (1, superscript) or lowered (-1, subscript) small text
    pub vert: i8,
}

#[derive(Clone, Debug)]
pub(super) struct Glyph {
    pub(super) ch: char,
    pub(super) left: f32,
    pub(super) right: f32,
    pub(super) baseline: f32,
    pub(super) style: Style,
    /// A space PDFium inferred from the layout (no character in the PDF)
    pub(super) generated: bool,
}

impl Glyph {
    pub(super) fn cx(&self) -> f32 {
        (self.left + self.right) / 2.0
    }
    pub(super) fn cy(&self) -> f32 {
        self.baseline - self.style.size * 0.3
    }
}

/// Geometry of the text after a tab: where Word's tab stop has to be
#[derive(Clone, Copy, Debug)]
pub(in crate::office) struct TabMark {
    pub left: f32,
    pub right: f32,
    /// The gap was filled with dots (table of contents)
    pub leader: bool,
    /// Where the text before the tab ends
    pub before: f32,
}

#[derive(Clone, Debug)]
pub(in crate::office) struct Line {
    /// Text runs; '\t' marks a tab, described by the matching entry of `tabs`
    pub runs: Vec<(String, Style)>,
    pub tabs: Vec<TabMark>,
    pub left: f32,
    pub right: f32,
    pub baseline: f32,
    pub size: f32,
    /// For a one-line paragraph: the distance to the next line of text when that is
    /// the document's line pitch (tighter than the 1.2 x size guess)
    pub pitch_hint: f32,
    /// Vector art sits inside the line (formulas drawn as shapes): the line keeps its
    /// layout exactly, the text around the art on tab stops
    pub art: bool,
    /// Each space between words: its place in `text()` (in characters) and where the
    /// text before it ends and after it starts (spreadsheets find narrow columns by them)
    pub gaps: Vec<(usize, f32, f32)>,
}

impl Line {
    pub fn text(&self) -> String {
        self.runs.iter().map(|(t, _)| t.as_str()).collect()
    }
    /// Text before the first tab is a list marker ("•", "1.", "a)") rather than content
    pub(super) fn marker_first(&self) -> bool {
        let text = self.text();
        let Some((head, _)) = text.split_once('\t') else { return false };
        let head = head.trim();
        head.chars().count() <= 4 && (starts_list_item(head) || head.chars().all(|c| !c.is_alphanumeric()) || is_enumerator(head))
    }
    /// Columns of text on one line (tabbed): never merged with other lines
    pub(super) fn tabular(&self) -> bool {
        self.art || self.tabs.len() > 1 || (self.tabs.len() == 1 && !self.marker_first())
    }
}

#[derive(Clone, Debug)]
pub(in crate::office) struct ImageRef {
    pub media: usize,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::office) struct Border {
    pub width: f32,
    pub color: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::office) enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Debug)]
pub(in crate::office) struct Cell {
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
    pub rect: Rect,
    /// top, right, bottom, left
    pub borders: [Option<Border>; 4],
    pub fill: Option<[u8; 3]>,
    pub valign: VAlign,
    pub blocks: Vec<Block>,
    /// Where the cell's text really ends (a layout column narrower than its cell):
    /// alignment is judged against this, the cell keeps its room against wrapping
    pub text_right: Option<f32>,
}

#[derive(Clone, Debug)]
pub(in crate::office) struct Table {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub cells: Vec<Cell>,
    /// Left and right cell padding
    pub padding: f32,
}

#[derive(Clone, Debug)]
pub(in crate::office) enum Content {
    Paragraph(Vec<Line>),
    Table(Table),
    /// Pictures side by side on one line
    Images(Vec<ImageRef>),
}

#[derive(Clone, Debug)]
pub(in crate::office) struct Block {
    pub rect: Rect,
    pub content: Content,
}

pub(in crate::office) struct Media {
    pub data: Vec<u8>,
    pub ext: &'static str,
}

#[derive(Default)]
pub(in crate::office) struct MediaStore {
    pub items: Vec<Media>,
    pub(super) index: HashMap<u64, usize>,
}

impl MediaStore {
    pub(super) fn add(&mut self, data: Vec<u8>, ext: &'static str) -> usize {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut h);
        let key = h.finish();
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        self.items.push(Media { data, ext });
        self.index.insert(key, self.items.len() - 1);
        self.items.len() - 1
    }
}

pub(in crate::office) struct PageModel {
    pub width: f32,
    pub height: f32,
    pub header: Vec<Block>,
    pub body: Vec<Block>,
    pub footer: Vec<Block>,
    /// Top-level page objects that are decoration only (coloured bands, boxes, bars,
    /// icons): drawn as a picture behind the text
    pub decor: Vec<usize>,
    /// That picture, once rendered: a page-sized transparent PNG
    pub background: Option<usize>,
    /// Rules that became the underline of text (not drawn again in the background)
    pub underline_rules: Vec<Rect>,
    /// The number printed on the page (journals often start at e.g. 2348)
    pub number: usize,
    /// Filled shapes on the page (bands and boxes behind text) and their colour
    pub fills: Vec<(Rect, [u8; 3])>,
}
