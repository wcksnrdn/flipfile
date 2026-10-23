// PDF -> Word (.docx), in the spirit of pdf2docx: PDFium gives every character with
// its position, size and font, and every vector shape. From that geometry the page
// layout is rebuilt:
//
// - ruling lines that form a grid become real Word tables (merged cells, borders per
//   cell side, cell shading, row heights);
// - characters become lines, lines become paragraphs (alignment, indents, exact line
//   pitch, spacing), and wide gaps become tabs with matching tab stops (dot leaders too);
// - content repeated at the top/bottom of the pages becomes the Word header/footer,
//   with the page number as a live PAGE field;
// - vector drawings (charts, flowcharts) become pictures, images stay where they were;
// - every page keeps its own size and orientation.
//
// The result is an editable document that keeps the original page layout; it is
// written by `pdf_docx_writer`.

use super::pdf_docx_writer as writer;
use pdfium_render::prelude::*;
use std::collections::HashMap;

/// Gaps wider than this many font sizes inside a line become a tab (column layouts).
const TAB_GAP: f32 = 2.0;
/// Gaps wider than this many font sizes get a space if the PDF has none.
const SPACE_GAP: f32 = 0.22;
/// Resolution of drawings that are turned into pictures.
const DRAWING_DPI: f32 = 200.0;
/// Resolution of the decoration picture behind a page's text.
const BACKGROUND_DPI: f32 = 150.0;

// ---------------------------------------------------------------------------
// Geometry (top-down: `t` < `b`, in points from the page's top-left corner)

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rect {
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
    fn cx(&self) -> f32 {
        (self.l + self.r) / 2.0
    }
    fn cy(&self) -> f32 {
        (self.t + self.b) / 2.0
    }
    fn union(&self, o: &Rect) -> Rect {
        Rect { l: self.l.min(o.l), t: self.t.min(o.t), r: self.r.max(o.r), b: self.b.max(o.b) }
    }
    /// Distance between the two rectangles (0 when they touch or overlap)
    fn gap(&self, o: &Rect) -> f32 {
        let dx = (o.l - self.r).max(self.l - o.r).max(0.0);
        let dy = (o.t - self.b).max(self.t - o.b).max(0.0);
        dx.max(dy)
    }
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.l && x <= self.r && y >= self.t && y <= self.b
    }
    fn overlap_area(&self, o: &Rect) -> f32 {
        let w = (self.r.min(o.r) - self.l.max(o.l)).max(0.0);
        let h = (self.b.min(o.b) - self.t.max(o.t)).max(0.0);
        w * h
    }
    fn area(&self) -> f32 {
        self.w().max(0.0) * self.h().max(0.0)
    }
}

// ---------------------------------------------------------------------------
// Page model, shared with the writer

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Style {
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
struct Glyph {
    ch: char,
    left: f32,
    right: f32,
    baseline: f32,
    style: Style,
    /// A space PDFium inferred from the layout (no character in the PDF)
    generated: bool,
}

impl Glyph {
    fn cx(&self) -> f32 {
        (self.left + self.right) / 2.0
    }
    fn cy(&self) -> f32 {
        self.baseline - self.style.size * 0.3
    }
}

/// Geometry of the text after a tab: where Word's tab stop has to be
#[derive(Clone, Copy, Debug)]
pub(super) struct TabMark {
    pub left: f32,
    pub right: f32,
    /// The gap was filled with dots (table of contents)
    pub leader: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Line {
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
}

impl Line {
    pub fn text(&self) -> String {
        self.runs.iter().map(|(t, _)| t.as_str()).collect()
    }
    /// Text before the first tab is a list marker ("•", "1.", "a)") rather than content
    fn marker_first(&self) -> bool {
        let text = self.text();
        let Some((head, _)) = text.split_once('\t') else { return false };
        let head = head.trim();
        head.chars().count() <= 4 && (starts_list_item(head) || head.chars().all(|c| !c.is_alphanumeric()) || is_enumerator(head))
    }
    /// Columns of text on one line (tabbed): never merged with other lines
    fn tabular(&self) -> bool {
        self.art || self.tabs.len() > 1 || (self.tabs.len() == 1 && !self.marker_first())
    }
}

#[derive(Clone, Debug)]
pub(super) struct ImageRef {
    pub media: usize,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Border {
    pub width: f32,
    pub color: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Debug)]
pub(super) struct Cell {
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
pub(super) struct Table {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub cells: Vec<Cell>,
    /// Left and right cell padding
    pub padding: f32,
}

#[derive(Clone, Debug)]
pub(super) enum Content {
    Paragraph(Vec<Line>),
    Table(Table),
    /// Pictures side by side on one line
    Images(Vec<ImageRef>),
}

#[derive(Clone, Debug)]
pub(super) struct Block {
    pub rect: Rect,
    pub content: Content,
}

pub(super) struct Media {
    pub data: Vec<u8>,
    pub ext: &'static str,
}

#[derive(Default)]
pub(super) struct MediaStore {
    pub items: Vec<Media>,
    index: HashMap<u64, usize>,
}

impl MediaStore {
    fn add(&mut self, data: Vec<u8>, ext: &'static str) -> usize {
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

pub(super) struct PageModel {
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
}

// ---------------------------------------------------------------------------
// Fonts and text

/// "ABCDEF+TimesNewRomanPS-BoldMT" -> "Times New Roman"
fn clean_font_name(raw: &str) -> String {
    let name = raw.split_once('+').map(|(_, n)| n).unwrap_or(raw);
    let base = name.split(['-', ',']).next().unwrap_or(name);
    let base = base.trim_end_matches("PSMT").trim_end_matches("MT").trim_end_matches("PS");
    // Open fonts made to the exact metrics of the Office ones: the Office font keeps
    // every line as wide as in the PDF, on any computer with Word
    let mapped = match base {
        "Helvetica" | "ArialMT" | "LiberationSans" | "Arimo" | "NimbusSans" | "NimbusSanL" => "Arial",
        "Times" | "TimesRoman" | "Times-Roman" | "LiberationSerif" | "Tinos" | "NimbusRoman" | "NimbusRomNo9L" => "Times New Roman",
        "Courier" | "LiberationMono" | "Cousine" | "NimbusMono" | "NimbusMonoPS" => "Courier New",
        "Carlito" => "Calibri",
        "Caladea" => "Cambria",
        other => other,
    };
    // CamelCase -> spaced words ("TimesNewRoman" -> "Times New Roman")
    let mut out = String::new();
    let chars: Vec<char> = mapped.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
            out.push(' ');
        }
        out.push(*c);
    }
    if out.is_empty() { "Calibri".into() } else { out }
}

fn starts_list_item(text: &str) -> bool {
    let t = text.trim_start();
    // The \u{F0..} ones are bullets of the Symbol and Wingdings fonts
    if t.starts_with(['•', '●', '◦', '○', '▪', '■', '□', '–', '-', '*', '➢', '✓', '\u{F0B7}', '\u{F0A7}', '\u{F0D8}', '\u{F076}']) {
        return true;
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && digits.len() <= 3 && t[digits.len()..].starts_with(['.', ')'])
}

/// "a.", "iv)", "B." and similar
fn is_enumerator(text: &str) -> bool {
    let Some(body) = text.strip_suffix(['.', ')']) else { return false };
    let body = body.trim_start_matches('(');
    !body.is_empty() && body.len() <= 4 && body.chars().all(|c| c.is_ascii_alphanumeric())
}

fn color_of(c: Result<PdfColor, PdfiumError>) -> Option<[u8; 3]> {
    let c = c.ok()?;
    (c.alpha() > 30).then(|| [c.red(), c.green(), c.blue()])
}

fn is_white(c: [u8; 3]) -> bool {
    c.iter().all(|v| *v >= 245)
}

// ---------------------------------------------------------------------------
// Vector graphics: ruling lines, shaded rectangles and drawings

#[derive(Clone, Copy, Debug)]
struct Seg {
    horizontal: bool,
    /// y of a horizontal segment, x of a vertical one
    pos: f32,
    a: f32,
    b: f32,
    width: f32,
    color: [u8; 3],
}

impl Seg {
    fn rect(&self) -> Rect {
        let half = self.width.max(0.5) / 2.0;
        if self.horizontal {
            Rect { l: self.a, r: self.b, t: self.pos - half, b: self.pos + half }
        } else {
            Rect { l: self.pos - half, r: self.pos + half, t: self.a, b: self.b }
        }
    }
}

#[derive(Default)]
struct Graphics {
    segs: Vec<Seg>,
    fills: Vec<(Rect, [u8; 3])>,
    drawings: Vec<Rect>,
}

fn graphics(page: &PdfPage, page_h: f32) -> Graphics {
    let mut g = Graphics::default();
    for object in page.objects().iter() {
        collect_graphics(&object, None, page_h, &mut g, 0);
    }
    g
}

fn collect_graphics(object: &PdfPageObject, parent: Option<PdfMatrix>, page_h: f32, g: &mut Graphics, depth: u32) {
    if let Some(form) = object.as_x_object_form_object() {
        if depth < 4 {
            let matrix = match (form.matrix().ok(), parent) {
                (Some(m), Some(p)) => Some(m.multiply(p)),
                (m, p) => m.or(p),
            };
            for child in form.iter() {
                collect_graphics(&child, matrix, page_h, g, depth + 1);
            }
        }
        return;
    }
    let Some(path) = object.as_path_object() else { return };
    let filled = path.fill_mode().map(|m| m != PdfPathFillMode::None).unwrap_or(false);
    let stroked = path.is_stroked().unwrap_or(false);
    let fill = color_of(object.fill_color()).filter(|c| filled && !is_white(*c));
    let stroke = color_of(object.stroke_color()).filter(|c| stroked && !is_white(*c));
    if fill.is_none() && stroke.is_none() {
        return; // invisible or white (backgrounds, masks)
    }
    let matrix = match (path.matrix().ok(), parent) {
        (Some(m), Some(p)) => Some(m.multiply(p)),
        (m, p) => m.or(p),
    };
    let stroke_width = object.stroke_width().map(|w| w.value).unwrap_or(1.0).max(0.25);
    let scale = matrix.map(|m| ((m.a() * m.a() + m.b() * m.b()).sqrt()).max(0.01)).unwrap_or(1.0);
    let stroke_width = stroke_width * scale;

    // Split into subpaths of points (top-down coordinates)
    let mut subpaths: Vec<(Vec<(f32, f32)>, bool)> = vec![];
    let segments = path.segments();
    for seg in segments.iter() {
        let (x, y) = seg.point();
        let (x, y) = match matrix {
            Some(m) => m.apply_to_points(x, y),
            None => (x, y),
        };
        let p = (x.value, page_h - y.value);
        match seg.segment_type() {
            PdfPathSegmentType::MoveTo => subpaths.push((vec![p], false)),
            PdfPathSegmentType::BezierTo => {
                if subpaths.is_empty() {
                    subpaths.push((vec![], false));
                }
                let last = subpaths.last_mut().unwrap();
                last.0.push(p);
                last.1 = true;
            }
            _ => {
                if subpaths.is_empty() {
                    subpaths.push((vec![], false));
                }
                subpaths.last_mut().unwrap().0.push(p);
            }
        }
        if seg.is_close() {
            if let Some(first) = subpaths.last().and_then(|s| s.0.first().copied()) {
                subpaths.last_mut().unwrap().0.push(first);
            }
        }
    }

    for (points, curved) in subpaths {
        if points.len() < 2 {
            continue;
        }
        let bbox = points.iter().fold(Rect { l: f32::MAX, t: f32::MAX, r: f32::MIN, b: f32::MIN }, |r, &(x, y)| Rect {
            l: r.l.min(x),
            t: r.t.min(y),
            r: r.r.max(x),
            b: r.b.max(y),
        });
        let axis = points.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.6 || (w[0].1 - w[1].1).abs() < 0.6);
        if curved || !axis {
            if bbox.w() > 4.0 || bbox.h() > 4.0 {
                g.drawings.push(bbox);
            }
            continue;
        }
        if let Some(color) = fill {
            if bbox.h() <= 2.5 && bbox.w() > 2.0 {
                g.segs.push(Seg { horizontal: true, pos: bbox.cy(), a: bbox.l, b: bbox.r, width: bbox.h().max(0.25), color });
            } else if bbox.w() <= 2.5 && bbox.h() > 2.0 {
                g.segs.push(Seg { horizontal: false, pos: bbox.cx(), a: bbox.t, b: bbox.b, width: bbox.w().max(0.25), color });
            } else if bbox.w() > 2.0 && bbox.h() > 2.0 {
                g.fills.push((bbox, color));
            }
        }
        if let Some(color) = stroke {
            for w in points.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if (y0 - y1).abs() < 0.6 && (x0 - x1).abs() > 1.0 {
                    g.segs.push(Seg { horizontal: true, pos: (y0 + y1) / 2.0, a: x0.min(x1), b: x0.max(x1), width: stroke_width, color });
                } else if (x0 - x1).abs() < 0.6 && (y0 - y1).abs() > 1.0 {
                    g.segs.push(Seg { horizontal: false, pos: (x0 + x1) / 2.0, a: y0.min(y1), b: y0.max(y1), width: stroke_width, color });
                }
            }
        }
    }
}

/// Joins collinear pieces of the same line (tables are often drawn cell by cell)
fn merge_segments(mut segs: Vec<Seg>) -> Vec<Seg> {
    segs.sort_by(|a, b| (a.horizontal, a.pos, a.a).partial_cmp(&(b.horizontal, b.pos, b.a)).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Seg> = vec![];
    for s in segs {
        // Compare with recent segments on (almost) the same line
        let found = out.iter_mut().rev().take(12).find(|o| o.horizontal == s.horizontal && (o.pos - s.pos).abs() <= 1.0 && s.a <= o.b + 2.0 && s.b >= o.a - 2.0);
        match found {
            Some(o) => {
                o.a = o.a.min(s.a);
                o.b = o.b.max(s.b);
                o.width = o.width.max(s.width);
            }
            None => out.push(s),
        }
    }
    out
}

fn find(parent: &mut [usize], i: usize) -> usize {
    let mut i = i;
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Groups of touching segments: each group is a candidate table
fn segment_groups(segs: &[Seg]) -> Vec<Vec<usize>> {
    let n = segs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let tol = 2.5;
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&segs[i], &segs[j]);
            let touch = if a.horizontal != b.horizontal {
                let (h, v) = if a.horizontal { (a, b) } else { (b, a) };
                v.pos >= h.a - tol && v.pos <= h.b + tol && h.pos >= v.a - tol && h.pos <= v.b + tol
            } else {
                false
            };
            if touch {
                union(&mut parent, i, j);
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]);
    out
}

fn cluster(mut values: Vec<f32>, tol: f32) -> Vec<f32> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f32, usize)> = vec![];
    for v in values {
        match out.last_mut() {
            Some((c, n)) if v - *c / *n as f32 <= tol => {
                *c += v;
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out.into_iter().map(|(c, n)| c / n as f32).collect()
}

/// Stacks of horizontal rules of the same length, one under the other, with no
/// vertical lines: the row rules of a table drawn without column lines (common in
/// journals and reports). Returns indexes into `segs`, top to bottom.
fn rule_stacks(segs: &[Seg], groups: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut lone: Vec<usize> = groups
        .iter()
        .filter(|g| g.len() == 1 && segs[g[0]].horizontal && segs[g[0]].b - segs[g[0]].a >= 40.0)
        .map(|g| g[0])
        .collect();
    lone.sort_by(|&a, &b| segs[a].pos.partial_cmp(&segs[b].pos).unwrap_or(std::cmp::Ordering::Equal));
    let mut stacks: Vec<Vec<usize>> = vec![];
    for i in lone {
        let s = &segs[i];
        let fits = |stack: &Vec<usize>| {
            let last = &segs[*stack.last().unwrap()];
            (last.a - s.a).abs() <= 3.0 && (last.b - s.b).abs() <= 3.0 && s.pos - last.pos > 4.0 && s.pos - last.pos < 220.0
        };
        match stacks.iter_mut().find(|st| fits(st)) {
            Some(st) => st.push(i),
            None => stacks.push(vec![i]),
        }
    }
    stacks.retain(|st| st.len() >= 3);
    stacks
}

/// A stack of row rules -> table: rows between the rules, columns where the text in
/// between leaves a vertical gap all the way down
fn rule_table(segs: &[Seg], stack: &[usize], glyphs: &[Glyph]) -> Option<(Rect, Lattice)> {
    let first = &segs[stack[0]];
    let ys: Vec<f32> = stack.iter().map(|&i| segs[i].pos).collect();
    let (l, r) = (stack.iter().map(|&i| segs[i].a).fold(f32::MAX, f32::min), stack.iter().map(|&i| segs[i].b).fold(f32::MIN, f32::max));
    let rect = Rect { l, r, t: ys[0], b: *ys.last()? };
    let inside: Vec<&Glyph> = glyphs.iter().filter(|g| g.ch != ' ' && rect.contains(g.cx(), g.cy())).collect();
    if inside.len() < 4 {
        return None;
    }
    // Columns: horizontal spans covered by text, merged; their gaps split the columns
    let mut spans: Vec<(f32, f32)> = inside.iter().map(|g| (g.left, g.right)).collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let size = inside.iter().map(|g| g.style.size).fold(0.0, f32::max);
    let mut merged: Vec<(f32, f32)> = vec![];
    for (a, b) in spans {
        match merged.last_mut() {
            Some(m) if a <= m.1 + size * 0.6 => m.1 = m.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut xs = vec![l];
    for w in merged.windows(2) {
        xs.push((w[0].1 + w[1].0) / 2.0);
    }
    xs.push(r);
    // One column is a ruled box of text, not a table
    if xs.len() < 3 {
        return None;
    }
    let border = Some(Border { width: first.width, color: first.color });
    let mut cells = vec![];
    for row in 0..ys.len() - 1 {
        for col in 0..xs.len() - 1 {
            let cell = Rect { l: xs[col], r: xs[col + 1], t: ys[row], b: ys[row + 1] };
            cells.push(Cell { row, col, rows: 1, cols: 1, rect: cell, borders: [border, None, border, None], fill: None, valign: VAlign::Top, blocks: vec![], text_right: None });
        }
    }
    Some((rect, Lattice { xs, ys, cells }))
}

/// A grid of ruling lines -> table structure (cells with spans and borders)
struct Lattice {
    xs: Vec<f32>,
    ys: Vec<f32>,
    cells: Vec<Cell>,
}

fn lattice(segs: &[Seg], group: &[usize], fills: &[(Rect, [u8; 3])]) -> Option<Lattice> {
    let hs: Vec<&Seg> = group.iter().map(|&i| &segs[i]).filter(|s| s.horizontal).collect();
    let vs: Vec<&Seg> = group.iter().map(|&i| &segs[i]).filter(|s| !s.horizontal).collect();
    if hs.len() < 2 || vs.len() < 2 {
        return None;
    }
    let xs = cluster(vs.iter().map(|s| s.pos).collect(), 2.0);
    let ys = cluster(hs.iter().map(|s| s.pos).collect(), 2.0);
    if xs.len() < 2 || ys.len() < 2 {
        return None;
    }
    let (nr, nc) = (ys.len() - 1, xs.len() - 1);
    // Is the boundary at grid line `y` drawn between x0 and x1 (or at `x` between y0 and y1)?
    let h_at = |k: usize, c: usize| -> Option<Border> {
        let (y, x0, x1) = (ys[k], xs[c], xs[c + 1]);
        let need = (x1 - x0) * 0.6;
        hs.iter()
            .filter(|s| (s.pos - y).abs() <= 2.0)
            .find(|s| s.b.min(x1) - s.a.max(x0) >= need)
            .map(|s| Border { width: s.width, color: s.color })
    };
    let v_at = |k: usize, r: usize| -> Option<Border> {
        let (x, y0, y1) = (xs[k], ys[r], ys[r + 1]);
        let need = (y1 - y0) * 0.6;
        vs.iter()
            .filter(|s| (s.pos - x).abs() <= 2.0)
            .find(|s| s.b.min(y1) - s.a.max(y0) >= need)
            .map(|s| Border { width: s.width, color: s.color })
    };

    let mut taken = vec![vec![false; nc]; nr];
    let mut cells = vec![];
    for r in 0..nr {
        for c in 0..nc {
            if taken[r][c] {
                continue;
            }
            // Grow right while no vertical line separates, then down while no horizontal one does
            let mut w = 1;
            while c + w < nc && !taken[r][c + w] && v_at(c + w, r).is_none() {
                w += 1;
            }
            let mut h = 1;
            while r + h < nr
                && (c..c + w).all(|cc| !taken[r + h][cc] && h_at(r + h, cc).is_none())
                && (c + 1..c + w).all(|k| v_at(k, r + h).is_none())
            {
                h += 1;
            }
            for row in taken.iter_mut().skip(r).take(h) {
                for t in row.iter_mut().skip(c).take(w) {
                    *t = true;
                }
            }
            let rect = Rect { l: xs[c], r: xs[c + w], t: ys[r], b: ys[r + h] };
            let side = |found: Vec<Option<Border>>| -> Option<Border> {
                let n = found.len();
                let present: Vec<Border> = found.into_iter().flatten().collect();
                (present.len() * 2 >= n && !present.is_empty()).then(|| present[0])
            };
            let borders = [
                side((c..c + w).map(|cc| h_at(r, cc)).collect()),
                side((r..r + h).map(|rr| v_at(c + w, rr)).collect()),
                side((c..c + w).map(|cc| h_at(r + h, cc)).collect()),
                side((r..r + h).map(|rr| v_at(c, rr)).collect()),
            ];
            // Shading: a coloured rectangle covering most of the cell
            let fill = fills
                .iter()
                .rfind(|(f, _)| f.overlap_area(&rect) > rect.area() * 0.6)
                .map(|(_, c)| *c);
            cells.push(Cell { row: r, col: c, rows: h, cols: w, rect, borders, fill, valign: VAlign::Top, blocks: vec![], text_right: None });
        }
    }
    Some(Lattice { xs, ys, cells })
}

// ---------------------------------------------------------------------------
// Characters and lines

fn glyphs(page: &PdfPage, page_h: f32, underlines: &[Seg]) -> Vec<Glyph> {
    let Ok(text) = page.text() else { return vec![] };
    let mut out = vec![];
    let mut families: HashMap<String, String> = HashMap::new();
    for c in text.chars().iter() {
        let Some(ch) = c.unicode_char() else { continue };
        if ch == '\r' || ch == '\n' || ch == '\u{2}' || ch == '\u{FFFE}' {
            continue;
        }
        let generated = c.is_generated().unwrap_or(false);
        if generated && ch != ' ' {
            continue;
        }
        let Ok(bounds) = c.loose_bounds() else { continue };
        let baseline = page_h - c.origin_y().map(|y| y.value).unwrap_or(bounds.bottom().value);
        let size = c.scaled_font_size().value;
        if generated {
            // Only a hint for the line builder: where PDFium saw a word boundary
            let style = Style { font: String::new(), size: 0.0, bold: false, italic: false, underline: false, color: None, spacing: 0.0, vert: 0 };
            out.push(Glyph { ch, left: bounds.left().value, right: bounds.left().value, baseline, style, generated });
            continue;
        }
        if !(size.is_finite() && size > 0.5) {
            continue;
        }
        let font = c.font_name();
        let lower = font.to_lowercase();
        // A font this computer does not have: the Office font of the same kind, chosen
        // from the PDF's own description of the font (looked up once per font)
        let family = families
            .entry(font.clone())
            .or_insert_with(|| {
                let clean = clean_font_name(&font);
                if super::typeset::has_font(&clean) {
                    clean
                } else if c.font_is_fixed_pitch() || lower.contains("mono") || lower.contains("courier") {
                    "Courier New".into()
                } else if c.font_is_serif() || lower.contains("serif") && !lower.contains("sans") {
                    "Times New Roman".into()
                } else {
                    "Arial".into()
                }
            })
            .clone();
        let bold = c.font_weight().map(|w| matches!(w, PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900)).unwrap_or(false)
            || c.font_is_bold_reenforced()
            || lower.contains("bold")
            || lower.contains("black")
            || lower.contains("heavy");
        let italic = c.font_is_italic() || lower.contains("italic") || lower.contains("oblique");
        let color = c.fill_color().ok().map(|col| [col.red(), col.green(), col.blue()]).filter(|c| c.iter().any(|v| *v > 40));
        let (left, right) = (bounds.left().value, bounds.right().value);
        // A thin rule just under the baseline that spans this character: underlined
        let underline = ch != ' '
            && underlines.iter().any(|s| {
                s.pos > baseline - 0.5 && s.pos < baseline + size * 0.35 && s.a <= left + (right - left) * 0.5 && s.b >= right - (right - left) * 0.5
            });
        out.push(Glyph {
            ch,
            left,
            right,
            baseline,
            style: Style { font: family, size: (size * 2.0).round() / 2.0, bold, italic, underline, color, spacing: 0.0, vert: 0 },
            generated,
        });
    }
    out
}

fn is_dot(c: char) -> bool {
    matches!(c, '.' | '…' | '·' | '‧')
}

/// Characters in PDF order -> lines. A new line starts when the baseline moves by
/// more than half a font size or the text jumps back to the left.
///
/// Within a line, the spacing between letters is measured first: letter-spaced text
/// ("H E A D I N G" style tracking) keeps its spacing as Word character spacing, and
/// only gaps clearly wider than that become spaces; much wider gaps become tabs, and
/// runs of dots become a dot-leader tab.
/// `art`: rectangles of vector art on the page; a gap in a line where art sits is kept
/// as a tab, so the text stays beside the art.
fn lines(glyphs: &[&Glyph], art: &[Rect]) -> Vec<Line> {
    // 1. Characters -> raw lines
    let mut raw: Vec<Vec<&Glyph>> = vec![];
    let mut baseline = 0.0f32;
    let mut size = 0.0f32;
    let mut last_right = 0.0f32;
    let mut forced = false;
    for &g in glyphs {
        // A line break marker (text recognition: one recognised line ends here)
        if g.ch == '\n' {
            forced = true;
            continue;
        }
        if g.ch == ' ' {
            if let Some(line) = raw.last_mut() {
                if (g.baseline - baseline).abs() <= size.max(1.0) * 0.5 {
                    line.push(g);
                }
            }
            continue;
        }
        let new_line = std::mem::take(&mut forced) || raw.is_empty() || (g.baseline - baseline).abs() > size.max(g.style.size) * 0.5 || g.left < last_right - size * 2.0;
        if new_line {
            raw.push(vec![g]);
            baseline = g.baseline;
            size = g.style.size;
            last_right = g.right;
        } else {
            raw.last_mut().unwrap().push(g);
            size = size.max(g.style.size);
            last_right = last_right.max(g.right);
        }
    }

    // Pieces of one line that the PDF draws out of order (a list number written after
    // its text, a word added later) come back together: same baseline, right next to
    // each other. Columns further apart stay separate lines.
    loop {
        let mut merged = false;
        'search: for i in 0..raw.len() {
            for j in 0..raw.len() {
                if i == j {
                    continue;
                }
                let letters = |line: &Vec<&Glyph>| -> Option<(f32, f32, f32, f32, usize, bool)> {
                    let l: Vec<&&Glyph> = line.iter().filter(|g| g.ch != ' ').collect();
                    let first = l.first()?;
                    let text: String = l.iter().map(|g| g.ch).collect();
                    // A list marker: "3." "a)" "•" (a bare number may be a badge)
                    let marker = starts_list_item(&text) || is_enumerator(&text);
                    Some((
                        l.iter().map(|g| g.left).fold(f32::MAX, f32::min),
                        l.iter().map(|g| g.right).fold(f32::MIN, f32::max),
                        first.baseline,
                        l.iter().map(|g| g.style.size).fold(0.0, f32::max),
                        l.len(),
                        marker,
                    ))
                };
                let (Some(a), Some(b)) = (letters(&raw[i]), letters(&raw[j])) else { continue };
                let size = a.3.max(b.3);
                // j right of i: i's end to j's start. Close as a word gap, or a short
                // piece (list number, bullet, footnote mark) a little further away;
                // anything else may be the next column
                let gap = b.0 - a.1;
                let similar = (a.3 - b.3).abs() <= a.3.min(b.3) * 0.2;
                let marker = a.5 && a.4 <= 3 && gap < a.3.min(b.3) * 1.5;
                let close = similar && (gap < size * 0.6 || marker);
                if (a.2 - b.2).abs() < size * 0.3 && gap > -size * 0.2 && close {
                    let tail = raw.remove(j);
                    let i = if j < i { i - 1 } else { i };
                    raw[i].extend(tail);
                    merged = true;
                    break 'search;
                }
            }
        }
        if !merged {
            break;
        }
    }

    // 2. Each raw line -> text with spaces, tabs and character spacing
    let mut out: Vec<Line> = vec![];
    for glyphs in raw {
        let letters: Vec<&Glyph> = glyphs.iter().copied().filter(|g| g.ch != ' ').collect();
        let Some(first) = letters.first() else { continue };
        // The line's size is that of most of its text: a bigger list number or bullet
        // must not make the line look like a different paragraph (or a taller one)
        let line_size = {
            let mut counts: Vec<(f32, usize)> = vec![];
            for g in &letters {
                match counts.iter_mut().find(|(s, _)| (*s - g.style.size).abs() < 0.01) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((g.style.size, 1)),
                }
            }
            counts.iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))).map(|c| c.0).unwrap_or(first.style.size)
        };
        // Tracking: the typical gap between neighbouring letters (lower quartile, so
        // word gaps do not count)
        let mut gaps: Vec<f32> = letters.windows(2).map(|w| w[1].left - w[0].right).filter(|g| *g > -line_size * 0.1 && *g < line_size * 0.6).collect();
        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let tracking = if gaps.len() >= 4 { gaps[gaps.len() / 4].max(0.0) } else { 0.0 };
        let tracking = if tracking > line_size * 0.06 { tracking } else { 0.0 };

        // The line's baseline: where most of its characters sit (not a raised marker)
        let baseline = {
            let mut b: Vec<f32> = letters.iter().map(|g| g.baseline).collect();
            b.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
            b[b.len() / 2]
        };
        let mut line = Line { runs: vec![], tabs: vec![], left: first.left, right: first.right, baseline, size: line_size, pitch_hint: 0.0, art: false };
        let band = (baseline - line_size * 0.9, baseline + line_size * 0.3);
        let art_in_gap = |from: f32, to: f32| art.iter().any(|r| r.l < to && r.r > from && r.t < band.1 && r.b > band.0 && r.h() < line_size * 4.0);
        let mut last_right = first.left;
        let mut real_space = false;
        let mut hinted_space = false;
        let mut in_leader = false;
        for g in glyphs {
            if g.ch == ' ' {
                if g.generated {
                    hinted_space = true;
                } else {
                    real_space = true;
                }
                continue;
            }
            if in_leader {
                if is_dot(g.ch) {
                    last_right = g.right.max(last_right);
                    continue;
                }
                in_leader = false;
                if let Some(t) = line.tabs.last_mut() {
                    t.left = g.left;
                    t.right = g.right;
                }
                real_space = false;
                hinted_space = false;
            }
            let mut style = g.style.clone();
            style.spacing = tracking;
            // Small text above/below the line: superscript (footnote and citation
            // numbers, powers) or subscript
            let rise = baseline - g.baseline;
            // Only attached to the text before it (x², a citation¹²): not the first thing
            // on the line, nor after a tab (a badge number beside a heading)
            let attached = !line.runs.is_empty() && g.left - last_right - tracking < g.style.size * TAB_GAP;
            if attached && g.style.size < line_size * 0.85 {
                if rise > line_size * 0.2 {
                    style.vert = 1;
                } else if rise < -line_size * 0.1 {
                    style.vert = -1;
                }
            }
            if !line.runs.is_empty() {
                let gap = g.left - last_right - tracking;
                let ends_with_tab = line.runs.last().is_some_and(|(t, _)| t.ends_with('\t'));
                let around_art = gap > g.style.size * 0.15 && art_in_gap(last_right, g.left);
                if around_art {
                    line.art = true;
                }
                if !ends_with_tab {
                    if gap > g.style.size * TAB_GAP || around_art {
                        push_char(&mut line, '\t', &style);
                        line.tabs.push(TabMark { left: g.left, right: g.right, leader: false });
                    } else if real_space || gap > g.style.size * SPACE_GAP || (hinted_space && gap > g.style.size * 0.12) {
                        push_char(&mut line, ' ', &style);
                    }
                }
            }
            real_space = false;
            hinted_space = false;
            push_char(&mut line, g.ch, &style);
            line.right = line.right.max(g.right);
            if let Some(t) = line.tabs.last_mut() {
                if !t.leader || t.left > 0.0 {
                    t.right = t.right.max(g.right);
                }
            }
            last_right = g.right.max(last_right);
            // Four dots in a row: a leader. The dots become one tab.
            if is_dot(g.ch) && trailing_dots(&line) >= 4 {
                remove_trailing_dots(&mut line);
                push_char(&mut line, '\t', &style);
                line.tabs.push(TabMark { left: g.right, right: g.right, leader: true });
                in_leader = true;
            }
        }
        // A line that ends in a tab (leader dots with nothing after): drop the tab
        while line.runs.last().is_some_and(|(t, _)| t.trim_end_matches(' ').ends_with('\t')) {
            let (t, _) = line.runs.last_mut().unwrap();
            let trimmed = t.trim_end_matches(' ').trim_end_matches('\t').to_string();
            *t = trimmed;
            line.tabs.pop();
            if line.runs.last().is_some_and(|(t, _)| t.is_empty()) {
                line.runs.pop();
            }
        }
        if let Some((t, _)) = line.runs.last_mut() {
            let trimmed = t.trim_end().to_string();
            *t = trimmed;
        }
        // Keep `tabs` in step with the tab characters
        let n = line.text().matches('\t').count();
        line.tabs.truncate(n);
        if !line.text().trim().is_empty() {
            out.push(line);
        }
    }
    out
}

fn trailing_dots(line: &Line) -> usize {
    let mut n = 0;
    for (t, _) in line.runs.iter().rev() {
        for c in t.chars().rev() {
            if is_dot(c) {
                n += if c == '…' { 3 } else { 1 };
            } else if c != ' ' {
                return n;
            }
        }
    }
    n
}

fn remove_trailing_dots(line: &mut Line) {
    while let Some((t, _)) = line.runs.last_mut() {
        while t.ends_with(|c: char| is_dot(c) || c == ' ') {
            t.pop();
        }
        if t.is_empty() {
            line.runs.pop();
        } else {
            break;
        }
    }
}

fn push_char(line: &mut Line, ch: char, style: &Style) {
    match line.runs.last_mut() {
        Some((text, s)) if s == style || ch == ' ' || ch == '\t' => text.push(ch),
        _ => line.runs.push((ch.to_string(), style.clone())),
    }
}

/// Lines -> paragraphs, using spacing, indents, short last lines, size changes and bullets.
fn paragraphs(lines: Vec<Line>, column_right: f32) -> Vec<Vec<Line>> {
    // Each line's own column: in a two-column page the left column ends long before
    // the page's text does, and its full lines must not look "short"
    let rights: Vec<f32> = lines
        .iter()
        .map(|l| {
            // Only where another column runs beside it
            let beside = lines.iter().any(|o| o.left > l.right + 8.0 && (o.baseline - l.baseline).abs() < 150.0);
            if !beside {
                return column_right;
            }
            let near = lines.iter().filter(|o| (o.left - l.left).abs() < 40.0 && (o.baseline - l.baseline).abs() < 150.0 && o.tabs.is_empty());
            near.map(|o| o.right).fold(l.right, f32::max).min(column_right)
        })
        .collect();
    // A "tab" in a line of justified text is only a stretched space: the line runs to
    // the column's edge like the lines above/below it, which have no tabs
    let mut lines = lines;
    for i in 0..lines.len() {
        let l = &lines[i];
        if l.tabs.is_empty() || l.tabs.iter().any(|t| t.leader) || (rights[i] - l.right).abs() > l.size {
            continue;
        }
        let like = |o: &Line| o.tabs.is_empty() && (o.left - l.left).abs() < l.size * 1.5 && (o.right - l.right).abs() < l.size && (o.baseline - l.baseline).abs() < l.size * 2.5;
        let justified = (i > 0 && like(&lines[i - 1])) || lines.get(i + 1).is_some_and(like);
        if justified {
            if cfg!(test) && std::env::var("FLIPFILE_BLOCKS").is_ok() {
                eprintln!("justify-tab: {}", lines[i].text());
            }
            let line = &mut lines[i];
            for (text, _) in line.runs.iter_mut() {
                *text = text.replace('\t', " ");
            }
            line.tabs.clear();
        }
    }
    let mut out: Vec<Vec<Line>> = vec![];
    let mut prev_right_edge = column_right;
    for (index, line) in lines.into_iter().enumerate() {
        let column_right = std::mem::replace(&mut prev_right_edge, rights[index]);
        let Some(para) = out.last_mut() else {
            out.push(vec![line]);
            continue;
        };
        let prev = para.last().unwrap();
        let line_height = prev.size.max(line.size) * 1.2;
        let gap = line.baseline - prev.baseline;
        // Typical spacing inside this paragraph so far
        let typical = if para.len() >= 2 {
            para.windows(2).map(|w| w[1].baseline - w[0].baseline).fold(0.0, f32::max)
        } else {
            line_height * 1.25
        };
        let size_change = (prev.size - line.size).abs() > prev.size.min(line.size) * 0.15;
        let style_change = prev.runs.last().map(|(_, s)| s.bold) != line.runs.first().map(|(_, s)| s.bold) && para.len() == 1 && prev.size >= line.size;
        // A short line ends a paragraph only if the next line's first word would have
        // fitted after it; otherwise the line simply wrapped early before a long word.
        let room = column_right - prev.right;
        let next_text = line.text();
        let first_word = next_text.split_whitespace().next().unwrap_or("");
        let char_width = (line.right - line.left) / next_text.chars().count().max(1) as f32;
        let word_width = (first_word.chars().count() as f32 + 1.0) * char_width;
        let prev_short = room > prev.size * 1.5 && word_width < room;
        // Wrapped lines of a list item line up with the text after the marker
        let text_left = if para.len() == 1 && prev.marker_first() { prev.tabs.first().map(|t| t.left).unwrap_or(prev.left) } else { prev.left };
        let indented = para.len() > 1 && line.left > prev.left + prev.size * 1.2 && !prev_short;
        let misaligned = para.len() == 1 && prev.marker_first() && (line.left - text_left).abs() > prev.size * 0.8;
        let starts = (gap > typical * 1.3 + 0.5)
            || gap < 0.0
            || size_change
            || style_change
            || prev_short
            || prev.tabular()
            || line.tabular()
            || line.marker_first()
            || starts_list_item(&line.text())
            || indented
            || misaligned;
        if starts {
            out.push(vec![line]);
        } else {
            para.push(line);
        }
    }
    out
}

/// Line box of a paragraph: the pitch Word will use and the rectangle it fills
pub(super) fn line_pitch(lines: &[Line]) -> f32 {
    if lines.len() >= 2 {
        let pitch = lines[1].baseline - lines[0].baseline;
        if pitch > lines[0].size * 0.8 && pitch < lines[0].size * 3.0 {
            return pitch;
        }
    }
    if lines[0].pitch_hint > 0.0 {
        return lines[0].pitch_hint;
    }
    lines[0].size * 1.2
}

/// How much tighter (points per character) the line's characters must sit for its text,
/// at its fonts' natural widths, to fit the width it has on the PDF page; `None` when
/// a font is not installed (nothing reliable to measure with)
fn squeeze_needed(line: &Line) -> Option<f32> {
    let mut natural = 0.0;
    let mut chars = 0usize;
    for (text, style) in &line.runs {
        if style.spacing > 0.0 {
            return None; // letter-spaced on purpose
        }
        natural += super::typeset::text_width(&style.font, style.size, style.bold, style.italic, text)?;
        chars += text.chars().count();
    }
    let available = line.right - line.left;
    (chars > 0 && natural > available).then(|| (natural - available) / chars as f32 + 0.02)
}

fn paragraph_block(lines: Vec<Line>) -> Block {
    let pitch = line_pitch(&lines);
    let first = &lines[0];
    let last = lines.last().unwrap();
    let rect = Rect {
        l: lines.iter().map(|l| l.left).fold(f32::MAX, f32::min),
        r: lines.iter().map(|l| l.right).fold(f32::MIN, f32::max),
        t: first.baseline - pitch * 0.8,
        b: last.baseline + pitch * 0.2,
    };
    Block { rect, content: Content::Paragraph(lines) }
}

/// Text and pictures inside one area (the page or a table cell) -> blocks in reading order
/// `extra`: blocks built elsewhere (tables) that share the area's layout
fn flow(glyphs: &[&Glyph], images: Vec<ImageRef>, column_right: f32, art: &[Rect], extra: Vec<Block>) -> Vec<Block> {
    let lines = lines(glyphs, art);
    let mut paras = paragraphs(lines, column_right);
    // One-line paragraphs: lines in a row (list items, short lines) sit at the text's
    // real pitch, often tighter than 1.2 x the size; use the distance to the next line
    for i in 0..paras.len() {
        if paras[i].len() != 1 {
            continue;
        }
        let (baseline, size) = (paras[i][0].baseline, paras[i][0].size);
        let next = paras.get(i + 1).map(|p| p[0].baseline - baseline);
        let prev = i.checked_sub(1).and_then(|j| paras.get(j)).map(|p| baseline - p.last().unwrap().baseline);
        let hint = [next, prev].into_iter().flatten().filter(|d| *d > size * 0.85 && *d < size * 1.2).fold(f32::MAX, f32::min);
        if hint < f32::MAX {
            paras[i][0].pitch_hint = hint;
        }
    }
    // Lines the PDF squeezed (justified text with spaces narrower than normal): Word
    // cannot squeeze spaces, so the paragraph gets slightly condensed characters,
    // just enough for every line to fit where it did
    for para in paras.iter_mut() {
        let squeeze = para.iter().filter(|l| l.tabs.is_empty()).filter_map(squeeze_needed).fold(0.0f32, f32::max);
        // Up to 8% of an em per character: what a substitute font of the same kind needs
        let size = para[0].size;
        if squeeze > 0.01 && squeeze <= (size * 0.08).max(0.6) {
            for line in para.iter_mut() {
                for (_, style) in line.runs.iter_mut() {
                    style.spacing = style.spacing.min(0.0) - squeeze;
                }
            }
        }
    }
    let mut blocks: Vec<Block> = paras.into_iter().map(paragraph_block).collect();
    // Pictures whose rows overlap stand side by side
    let mut images = images;
    images.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
    let mut rows: Vec<Vec<ImageRef>> = vec![];
    for img in images {
        match rows.last_mut() {
            // Side by side: overlapping rows of the page, and apart from each other
            Some(row)
                if row.iter().any(|o| o.rect.b.min(img.rect.b) - o.rect.t.max(img.rect.t) > img.rect.h().min(o.rect.h()) * 0.3)
                    && row.iter().all(|o| o.rect.r.min(img.rect.r) - o.rect.l.max(img.rect.l) <= 1.0) =>
            {
                row.push(img)
            }
            _ => rows.push(vec![img]),
        }
    }
    for mut row in rows {
        row.sort_by(|a, b| a.rect.l.partial_cmp(&b.rect.l).unwrap_or(std::cmp::Ordering::Equal));
        let rect = row.iter().skip(1).fold(row[0].rect, |r, i| r.union(&i.rect));
        blocks.push(Block { rect, content: Content::Images(row) });
    }
    blocks.extend(extra);
    blocks.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
    side_by_side(blocks, column_right)
}

/// Blocks that share the same height on the page but sit apart horizontally (two
/// columns, a picture beside text, a page number beside a note) are put into the cells
/// of a borderless table, so they stay side by side instead of being stacked.
/// `column_right`: the right edge of the text area; the last column may use all the
/// room up to it, so a line that just fitted in the PDF never wraps in Word.
fn side_by_side(blocks: Vec<Block>, column_right: f32) -> Vec<Block> {
    if cfg!(test) && std::env::var("FLIPFILE_BLOCKS").is_ok() {
        for b in &blocks {
            let text = match &b.content {
                Content::Paragraph(l) => l.first().map(|l| l.text()).unwrap_or_default(),
                Content::Table(_) => "<table>".into(),
                Content::Images(_) => "<images>".into(),
            };
            eprintln!("blk {:6.1} {:6.1} {:6.1} {:6.1} {}", b.rect.l, b.rect.t, b.rect.r, b.rect.b, text.chars().take(50).collect::<String>());
        }
    }
    let mut out = vec![];
    let mut i = 0;
    while i < blocks.len() {
        let mut bottom = blocks[i].rect.b;
        let mut j = i + 1;
        while j < blocks.len() {
            let r = &blocks[j].rect;
            let overlap = bottom.min(r.b) - r.t;
            if overlap > 2.0 && overlap > r.h().min(bottom - blocks[i].rect.t) * 0.3 {
                bottom = bottom.max(r.b);
                j += 1;
            } else {
                break;
            }
        }
        let mut group: Vec<Block> = blocks[i..j].to_vec();
        i = j;
        if group.len() < 2 {
            out.extend(group);
            continue;
        }
        // Columns: blocks whose horizontal extents overlap belong together
        group.sort_by(|a, b| a.rect.l.partial_cmp(&b.rect.l).unwrap_or(std::cmp::Ordering::Equal));
        let mut columns: Vec<(f32, f32, Vec<Block>)> = vec![];
        for b in group {
            match columns.last_mut() {
                Some((_, r, col)) if b.rect.l < *r - 1.0 => {
                    *r = r.max(b.rect.r);
                    col.push(b);
                }
                _ => columns.push((b.rect.l, b.rect.r, vec![b])),
            }
        }
        if columns.len() < 2 {
            let mut col = columns.pop().map(|c| c.2).unwrap_or_default();
            col.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
            out.extend(col);
            continue;
        }
        // The columns go on as long as what follows stays on one side of every gutter
        // (the gaps between the columns): a two-column page becomes one table with a
        // whole column in each cell, instead of many small ones
        let gutters: Vec<(f32, f32)> = columns.windows(2).map(|w| (w[0].1, w[1].0)).filter(|(a, b)| b - a >= 6.0).collect();
        // Only text columns (2–3 of similar width), not a row of cards or a label
        // beside a paragraph
        let widths: Vec<f32> = columns.iter().map(|c| c.1 - c.0).collect();
        let span = columns.last().unwrap().1 - columns[0].0;
        let narrowest = widths.iter().copied().fold(f32::MAX, f32::min);
        let widest = widths.iter().copied().fold(0.0, f32::max);
        let text_columns = (2..=3).contains(&columns.len()) && narrowest >= span * 0.28 && widest <= narrowest * 1.6;
        if text_columns && gutters.len() == columns.len() - 1 {
            let mut band_bottom = columns.iter().flat_map(|c| c.2.iter()).map(|b| b.rect.b).fold(f32::MIN, f32::max);
            while i < blocks.len() {
                let b = &blocks[i].rect;
                // Into the gutter by more than a third of it (lines of a column end a
                // little unevenly, more so in recognised scans)
                let crosses = gutters.iter().any(|&(lo, hi)| {
                    let slack = ((hi - lo) / 3.0).max(1.0);
                    b.r > lo + slack && b.l < hi - slack
                });
                // Far below the band (a new part of the page): stop
                if crosses || b.t > band_bottom + 40.0 {
                    break;
                }
                let k = gutters.iter().filter(|&&(lo, hi)| b.l >= (lo + hi) / 2.0).count();
                // Well below what its column holds so far (a new section starts), or
                // side by side with the next block inside one column: not running text
                let column_bottom = columns[k].2.iter().map(|c| c.rect.b).fold(f32::MIN, f32::max);
                let beside_next = blocks.get(i + 1).is_some_and(|n| {
                    let n = &n.rect;
                    let same_column = gutters.iter().filter(|&&(lo, hi)| n.l >= (lo + hi) / 2.0).count() == k;
                    same_column && b.b.min(n.b) - b.t.max(n.t) > 2.0 && (n.l >= b.r - 1.0 || n.r <= b.l + 1.0)
                });
                if b.t - column_bottom > 25.0 || beside_next {
                    break;
                }
                let col = &mut columns[k];
                col.0 = col.0.min(b.l);
                col.1 = col.1.max(b.r);
                band_bottom = band_bottom.max(b.b);
                col.2.push(blocks[i].clone());
                i += 1;
            }
        }
        let top = columns.iter().flat_map(|c| c.2.iter()).map(|b| b.rect.t).fold(f32::MAX, f32::min);
        let bottom = columns.iter().flat_map(|c| c.2.iter()).map(|b| b.rect.b).fold(f32::MIN, f32::max);
        // Each cell starts where its column's text starts and runs up to the next
        // column (all the gap as room against wrapping); it also remembers where its
        // text ends, for judging alignment
        let mut xs = vec![columns[0].0];
        for w in columns.windows(2) {
            xs.push(w[1].0);
        }
        let text_rights: Vec<f32> = columns.iter().map(|c| c.1).collect();
        xs.push(columns.last().unwrap().1.max(column_right));
        let cells = columns
            .into_iter()
            .enumerate()
            .map(|(k, (_, _, mut col))| {
                col.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
                Cell {
                    row: 0,
                    col: k,
                    rows: 1,
                    cols: 1,
                    rect: Rect { l: xs[k], r: xs[k + 1], t: top, b: bottom },
                    borders: [None; 4],
                    fill: None,
                    valign: VAlign::Top,
                    blocks: col,
                    text_right: Some(text_rights[k]),
                }
            })
            .collect();
        let rect = Rect { l: xs[0], r: *xs.last().unwrap(), t: top, b: bottom };
        out.push(Block { rect, content: Content::Table(Table { xs, ys: vec![top, bottom], cells, padding: 0.0 }) });
    }
    out
}

// ---------------------------------------------------------------------------
// One page

/// The page with only its decoration (and annotations: stamps, highlights, signatures)
/// left, rendered to a transparent PNG the size of the page
fn background(doc: &PdfDocument, index: usize, keep: &[usize], underlines: &[Rect], media: &mut MediaStore) -> Option<usize> {
    let mut page = doc.pages().get(index as i32).ok()?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    let count = page.objects().len();
    for i in (0..count).rev() {
        if keep.binary_search(&i).is_err() {
            let _ = page.objects_mut().remove_object_at_index(i);
        }
    }
    // Text inside the forms kept for their graphics is real text in the document: it
    // must not show a second time in the background
    let page_h = page.height().value;
    for object in page.objects().iter() {
        if let Some(form) = object.as_x_object_form_object() {
            hide_text(form, form.matrix().ok(), page_h, underlines, 0);
        }
    }
    // 150 dpi, but at most 2500 px on the long side (slides can be huge pages)
    let longest = page.width().value.max(page.height().value).max(1.0);
    let scale = (BACKGROUND_DPI / 72.0).min(2500.0 / longest);
    let config = PdfRenderConfig::new().scale_page_by_factor(scale).set_clear_color(PdfColor::new(255, 255, 255, 0));
    let image = page.render_with_config(&config).ok()?.as_image().ok()?;
    // Nothing visible left (e.g. invisible annotations): no picture
    let rgba = image.to_rgba8();
    if !rgba.pixels().any(|p| p[3] > 8) {
        return None;
    }
    // Covering (nearly) the whole page, like a designed or photo backdrop: nothing to
    // see through, and JPEG is a fraction of the size
    let opaque = rgba.pixels().filter(|p| p[3] >= 250).count();
    if opaque as f64 > rgba.pixels().len() as f64 * 0.97 {
        let mut jpg = Vec::new();
        let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85);
        image::DynamicImage::ImageRgb8(rgb).write_with_encoder(encoder).ok()?;
        return Some(media.add(jpg, "jpeg"));
    }
    // Mostly sparse line art: grey when it has no colour, and the strongest compression
    let grey = rgba.pixels().all(|p| p[3] == 0 || (p[0] == p[1] && p[1] == p[2]));
    let picture = if grey { image::DynamicImage::ImageLumaA8(image::DynamicImage::ImageRgba8(rgba).to_luma_alpha8()) } else { image::DynamicImage::ImageRgba8(rgba) };
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(&mut png, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive);
    picture.write_with_encoder(encoder).ok()?;
    Some(media.add(png, "png"))
}

/// `shown`: the size the picture has on the page (points); more pixels than 300 dpi
/// at that size are not kept
/// Inside a form kept for the background: its text and the rules that became Word
/// underlines are made invisible (they are in the document itself)
fn hide_text(form: &PdfPageXObjectFormObject, matrix: Option<PdfMatrix>, page_h: f32, underlines: &[Rect], depth: u32) {
    for i in 0..form.len() {
        let Ok(mut child) = form.get(i) else { continue };
        if let Some(text) = child.as_text_object_mut() {
            let _ = text.set_render_mode(PdfPageTextRenderMode::Invisible);
            continue;
        }
        if child.object_type() == PdfPageObjectType::Path && !underlines.is_empty() {
            if let Ok(b) = child.bounds() {
                // The form's own placement on the page
                let corners = [(b.left(), b.bottom()), (b.right(), b.top())].map(|(x, y)| match matrix {
                    Some(m) => m.apply_to_points(x, y),
                    None => (x, y),
                });
                let (l, r) = (corners[0].0.value.min(corners[1].0.value), corners[0].0.value.max(corners[1].0.value));
                let (t, bt) = (page_h - corners[0].1.value.max(corners[1].1.value), page_h - corners[0].1.value.min(corners[1].1.value));
                let rect = Rect { l, r, t, b: bt };
                let is_underline = rect.h() <= 3.5 && underlines.iter().any(|u| (u.cy() - rect.cy()).abs() < 2.0 && u.r.min(rect.r) - u.l.max(rect.l) > rect.w() * 0.5);
                if is_underline {
                    let _ = child.set_fill_color(PdfColor::new(0, 0, 0, 0));
                    let _ = child.set_stroke_color(PdfColor::new(0, 0, 0, 0));
                }
            }
        }
        if depth < 4 {
            if let Some(inner) = child.as_x_object_form_object() {
                let m = match (inner.matrix().ok(), matrix) {
                    (Some(a), Some(b)) => Some(a.multiply(b)),
                    (a, b) => a.or(b),
                };
                hide_text(inner, m, page_h, underlines, depth + 1);
            }
        }
    }
}

fn encode_image(img: &image::DynamicImage, shown: Option<Rect>, media: &mut MediaStore) -> Option<usize> {
    let mut owned;
    let mut img = img;
    if let Some(r) = shown {
        let max_w = (r.w() * 300.0 / 72.0).max(16.0) as u32;
        if img.width() > max_w + max_w / 7 {
            let h = ((img.height() as f32) * max_w as f32 / img.width() as f32).round().max(1.0) as u32;
            owned = img.resize_exact(max_w, h, image::imageops::FilterType::Lanczos3);
            img = &owned;
        }
    }
    // An alpha channel that is opaque everywhere is no transparency at all
    if img.color().has_alpha() && img.to_rgba8().pixels().all(|p| p[3] == 255) {
        owned = image::DynamicImage::ImageRgb8(img.to_rgb8());
        img = &owned;
    }
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    // Photos are far smaller as JPEG; keep PNG for screenshots, line art and transparency
    if png.len() > 150_000 && !img.color().has_alpha() {
        let mut jpg = Vec::new();
        let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90);
        if rgb.write_with_encoder(encoder).is_ok() && jpg.len() < png.len() * 2 / 3 {
            return Some(media.add(jpg, "jpeg"));
        }
    }
    Some(media.add(png, "png"))
}

fn page_model(page: &PdfPage, document: &PdfDocument, media: &mut MediaStore) -> PageModel {
    let (page_w, page_h) = (page.width().value, page.height().value);
    let g = graphics(page, page_h);
    let segs = merge_segments(g.segs);
    let groups = segment_groups(&segs);
    let stacks = rule_stacks(&segs, &groups);
    // Underlines: thin horizontal rules that are not part of a grid (table borders)
    // nor of a stack of row rules
    let underlines: Vec<Seg> = groups
        .iter()
        .filter(|grp| grp.iter().all(|&i| segs[i].horizontal))
        .flat_map(|grp| grp.iter().copied())
        .filter(|i| !stacks.iter().any(|s| s.contains(i)))
        .map(|i| segs[i])
        .filter(|s| s.width <= 3.0)
        .collect();
    let all_glyphs = glyphs(page, page_h, &underlines);

    // Drawings (curves, diagonals) and the lines that touch them form pictures
    // Small curves (rounded corners of a card, icons, bullets) are not diagrams: they are
    // drawn in the page's background and the text around them stays text
    let mut regions: Vec<Rect> = vec![];
    for d in &g.drawings {
        if d.w().max(d.h()) >= 15.0 {
            regions.push(*d);
        }
    }
    let group_rects: Vec<Rect> = groups.iter().map(|grp| grp.iter().skip(1).fold(segs[grp[0]].rect(), |r, &i| r.union(&segs[i].rect()))).collect();
    let mut table_groups: Vec<usize> = vec![];
    loop {
        // Merge touching regions
        let mut merged = false;
        'outer: for i in 0..regions.len() {
            for j in i + 1..regions.len() {
                if regions[i].gap(&regions[j]) <= 3.0 {
                    regions[i] = regions[i].union(&regions[j]);
                    regions.remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    for (gi, grp) in groups.iter().enumerate() {
        let rect = group_rects[gi];
        let horizontal = grp.iter().filter(|&&i| segs[i].horizontal).count();
        let vertical = grp.len() - horizontal;
        let grid_like = horizontal >= 3 && vertical >= 3;
        let touching = regions.iter().position(|r| r.gap(&rect) <= 2.0);
        match touching {
            // Boxes joined to arrows: part of a diagram
            Some(ri) if !grid_like => regions[ri] = regions[ri].union(&rect),
            _ => table_groups.push(gi),
        }
    }
    // A "drawing" that is really a frame around lots of text stays text
    let page_area = page_w * page_h;
    regions.retain(|r| {
        if r.w() < 24.0 || r.h() < 12.0 || r.area() > page_area * 0.85 {
            return false;
        }
        let text_area: f32 = all_glyphs.iter().filter(|gl| r.contains(gl.cx(), gl.cy())).map(|gl| (gl.right - gl.left) * gl.style.size).sum();
        text_area < r.area() * 0.2
    });

    // Tables
    let mut tables: Vec<(Rect, Lattice)> = vec![];
    for gi in table_groups {
        let rect = group_rects[gi];
        if regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.5) {
            continue;
        }
        if let Some(lat) = lattice(&segs, &groups[gi], &g.fills) {
            let has_text = all_glyphs.iter().any(|gl| rect.contains(gl.cx(), gl.cy()));
            if lat.cells.len() >= 2 || has_text {
                tables.push((rect, lat));
            }
        }
    }
    for stack in &stacks {
        if let Some((rect, lat)) = rule_table(&segs, stack, &all_glyphs) {
            let taken = tables.iter().any(|(t, _)| t.overlap_area(&rect) > 0.0) || regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.3);
            if !taken {
                tables.push((rect, lat));
            }
        }
    }

    // Raster images. One covering most of the page (a cover, a photo backdrop, a
    // certificate's artwork) is part of the page's background, at its exact place;
    // a scan under an invisible OCR text layer is dropped (the text is what matters).
    let ocr_layer = page
        .objects()
        .iter()
        .filter_map(|o| o.as_text_object().map(|t| t.render_mode()))
        .take(50)
        .filter(|m| matches!(m, PdfPageTextRenderMode::Invisible))
        .count()
        > 10;
    let mut backdrops: Vec<usize> = vec![];
    let mut pictures: Vec<(Rect, image::DynamicImage)> = vec![];
    let mut images: Vec<ImageRef> = vec![];
    for (index, object) in page.objects().iter().enumerate() {
        let Some(image) = object.as_image_object() else { continue };
        let Ok(bounds) = object.bounds() else { continue };
        let rect = Rect { l: bounds.left().value, r: bounds.right().value, t: page_h - bounds.top().value, b: page_h - bounds.bottom().value };
        if rect.w() < 8.0 || rect.h() < 8.0 {
            continue; // decorative dots and lines
        }
        if regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.5) {
            continue; // drawn into the picture of that region
        }
        // Partly over a diagram picture: it stays whole, at its place, behind the text
        if regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.05) {
            backdrops.push(index);
            continue;
        }
        if rect.area() > page_area * 0.5 {
            if !ocr_layer {
                backdrops.push(index);
            }
            continue;
        }
        // Text written over the picture (a labelled box, a banner, a captioned photo):
        // the picture stays at its place behind the text, as part of the background
        let under_text = all_glyphs.iter().any(|gl| gl.ch != ' ' && rect.contains(gl.cx(), gl.cy()));
        if under_text && !ocr_layer {
            backdrops.push(index);
            continue;
        }
        let Ok(img) = image.get_processed_image(document).or_else(|_| image.get_raw_image()) else { continue };
        pictures.push((rect, img));
    }
    // Pictures that touch or overlap form one picture: a logo exported as strips, a
    // photo in tiles, a screenshot with a pop-up over it. Separate inline pictures
    // would show seams (or stack) in Word, so they are put together at full resolution.
    let mut group: Vec<usize> = (0..pictures.len()).collect();
    for i in 0..pictures.len() {
        for j in i + 1..pictures.len() {
            let (a, b) = (&pictures[i].0, &pictures[j].0);
            let touching = a.gap(b) <= 1.5 && (a.overlap_area(b) > 0.0 || (a.b.min(b.b) - a.t.max(b.t)) > 1.0 || (a.r.min(b.r) - a.l.max(b.l)) > 1.0);
            if touching {
                union(&mut group, i, j);
            }
        }
    }
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..pictures.len() {
        let root = find(&mut group, i);
        clusters.entry(root).or_default().push(i);
    }
    let mut clusters: Vec<Vec<usize>> = clusters.into_values().collect();
    clusters.sort_by_key(|c| c[0]);
    for members in clusters {
        if members.len() == 1 {
            let (rect, img) = &pictures[members[0]];
            if let Some(m) = encode_image(img, Some(*rect), media) {
                images.push(ImageRef { media: m, rect: *rect });
            }
            continue;
        }
        let bounds = members.iter().skip(1).fold(pictures[members[0]].0, |r, &i| r.union(&pictures[i].0));
        // The sharpest member sets the resolution (capped at 4000 px on the long side)
        let scale = members
            .iter()
            .map(|&i| pictures[i].1.width() as f32 / pictures[i].0.w().max(1.0))
            .fold(1.0f32, f32::max)
            .min(4000.0 / bounds.w().max(bounds.h()).max(1.0));
        let (cw, ch) = ((bounds.w() * scale).round().max(1.0) as u32, (bounds.h() * scale).round().max(1.0) as u32);
        let mut canvas = image::RgbaImage::new(cw, ch);
        for &i in &members {
            let (rect, img) = &pictures[i];
            let (w, h) = ((rect.w() * scale).round().max(1.0) as u32, (rect.h() * scale).round().max(1.0) as u32);
            let tile = image::imageops::resize(&img.to_rgba8(), w, h, image::imageops::FilterType::Triangle);
            let (x, y) = (((rect.l - bounds.l) * scale).round() as i64, ((rect.t - bounds.t) * scale).round() as i64);
            image::imageops::overlay(&mut canvas, &tile, x, y);
        }
        if let Some(m) = encode_image(&image::DynamicImage::ImageRgba8(canvas), Some(bounds), media) {
            images.push(ImageRef { media: m, rect: bounds });
        }
    }
    // Diagram regions become pictures, rendered from the page
    if !regions.is_empty() {
        let scale = DRAWING_DPI / 72.0;
        if let Ok(bitmap) = page.render_with_config(&PdfRenderConfig::new().scale_page_by_factor(scale)) {
            if let Ok(full) = bitmap.as_image() {
                for r in &regions {
                    let pad = 1.5;
                    let x = ((r.l - pad).max(0.0) * scale) as u32;
                    let y = ((r.t - pad).max(0.0) * scale) as u32;
                    let w = (((r.w() + pad * 2.0) * scale) as u32).min(full.width().saturating_sub(x));
                    let h = (((r.h() + pad * 2.0) * scale) as u32).min(full.height().saturating_sub(y));
                    if w < 4 || h < 4 {
                        continue;
                    }
                    let crop = full.crop_imm(x, y, w, h);
                    if let Some(m) = encode_image(&crop, None, media) {
                        let rect = Rect { l: x as f32 / scale, t: y as f32 / scale, r: (x + w) as f32 / scale, b: (y + h) as f32 / scale };
                        images.push(ImageRef { media: m, rect });
                    }
                }
            }
        }
    }

    // Distribute text and pictures: pictures' own text, table cells, free flow
    // Tables inside a cell of another table (nested): smallest first, so text goes to
    // the innermost table, and each one knows the cell it sits in
    tables.sort_by(|a, b| a.0.area().partial_cmp(&b.0.area()).unwrap_or(std::cmp::Ordering::Equal));
    let parents: Vec<Option<(usize, usize)>> = (0..tables.len())
        .map(|i| {
            let r = tables[i].0;
            let j = (i + 1..tables.len()).find(|&j| {
                let o = tables[j].0;
                r.l >= o.l - 2.0 && r.r <= o.r + 2.0 && r.t >= o.t - 2.0 && r.b <= o.b + 2.0
            })?;
            let cell = tables[j].1.cells.iter().position(|c| c.rect.contains(r.cx(), r.cy()))?;
            Some((j, cell))
        })
        .collect();
    let mut free_glyphs: Vec<&Glyph> = vec![];
    let mut cell_glyphs: Vec<Vec<Vec<&Glyph>>> = tables.iter().map(|(_, l)| vec![vec![]; l.cells.len()]).collect();
    'glyph: for gl in &all_glyphs {
        let (x, y) = (gl.cx(), gl.cy());
        if regions.iter().any(|r| r.contains(x, y)) {
            continue;
        }
        for (ti, (rect, lat)) in tables.iter().enumerate() {
            if rect.contains(x, y) {
                if let Some(ci) = lat.cells.iter().position(|c| c.rect.contains(x, y)) {
                    cell_glyphs[ti][ci].push(gl);
                    continue 'glyph;
                }
            }
        }
        free_glyphs.push(gl);
    }
    let mut free_images: Vec<ImageRef> = vec![];
    let mut cell_images: Vec<Vec<Vec<ImageRef>>> = tables.iter().map(|(_, l)| vec![vec![]; l.cells.len()]).collect();
    'image: for img in images {
        let (x, y) = (img.rect.cx(), img.rect.cy());
        for (ti, (rect, lat)) in tables.iter().enumerate() {
            if rect.contains(x, y) {
                if let Some(ci) = lat.cells.iter().position(|c| c.rect.contains(x, y)) {
                    cell_images[ti][ci].push(img);
                    continue 'image;
                }
            }
        }
        free_images.push(img);
    }

    // Decoration: every vector object (and annotation) not turned into something else
    // above stays visible through a picture behind the text
    let underlined: Vec<&Glyph> = all_glyphs.iter().filter(|g| g.style.underline).collect();
    let mut decor = vec![];
    for (index, object) in page.objects().iter().enumerate() {
        if matches!(object.object_type(), PdfPageObjectType::Text | PdfPageObjectType::Image) {
            continue;
        }
        let Ok(b) = object.bounds() else { continue };
        let rect = Rect { l: b.left().value, r: b.right().value, t: page_h - b.top().value, b: page_h - b.bottom().value };
        if rect.w() < 0.3 && rect.h() < 0.3 {
            continue;
        }
        if let Some(path) = object.as_path_object() {
            let filled = path.fill_mode().map(|m| m != PdfPathFillMode::None).unwrap_or(false) && color_of(object.fill_color()).is_some_and(|c| !is_white(c));
            let stroked = path.is_stroked().unwrap_or(false) && color_of(object.stroke_color()).is_some_and(|c| !is_white(c));
            if !filled && !stroked {
                continue;
            }
        }
        let thin = rect.w() <= 3.5 || rect.h() <= 3.5;
        let (cx, cy) = (rect.cx(), rect.cy());
        let in_region = regions.iter().any(|r| r.contains(cx, cy));
        let table_part = tables.iter().any(|(t, lat)| {
            let inside = rect.l >= t.l - 2.0 && rect.r <= t.r + 2.0 && rect.t >= t.t - 2.0 && rect.b <= t.b + 2.0;
            inside && (thin || lat.cells.iter().any(|c| c.fill.is_some() && c.rect.overlap_area(&rect) > c.rect.area() * 0.6))
        });
        let underline = rect.h() <= 3.5
            && underlined.iter().any(|g| cy > g.baseline - 0.5 && cy < g.baseline + g.style.size * 0.35 && rect.l <= g.right && rect.r >= g.left);
        if !in_region && !table_part && !underline {
            decor.push(index);
        }
    }
    decor.extend(backdrops);
    decor.sort_unstable();

    let text_right = free_glyphs.iter().map(|g| g.right).fold(0.0, f32::max).max(page_w * 0.6);
    // Small vector shapes among the text (formulas, symbols drawn as outlines)
    let art: Vec<Rect> = decor
        .iter()
        .filter_map(|&i| page.objects().get(i).ok())
        .filter(|o| matches!(o.object_type(), PdfPageObjectType::Path | PdfPageObjectType::XObjectForm))
        .filter_map(|o| o.bounds().ok())
        .map(|b| Rect { l: b.left().value, r: b.right().value, t: page_h - b.top().value, b: page_h - b.bottom().value })
        .filter(|r| r.h() < 40.0 && r.w() < page_w * 0.5)
        .collect();
    // Tables first: they take part in the page's columns like any other block
    let mut table_blocks = vec![];
    let mut nested: HashMap<(usize, usize), Vec<Block>> = HashMap::new();
    for (ti, (rect, lat)) in tables.into_iter().enumerate() {
        let mut insets = vec![];
        let mut cells = lat.cells;
        for (ci, cell) in cells.iter_mut().enumerate() {
            let glyphs = std::mem::take(&mut cell_glyphs[ti][ci]);
            let imgs = std::mem::take(&mut cell_images[ti][ci]);
            let inner = nested.remove(&(ti, ci)).unwrap_or_default();
            cell.blocks = flow(&glyphs, imgs, cell.rect.r - 1.0, &art, inner);
            if let Some(first) = cell.blocks.first() {
                insets.push(first.rect.l - cell.rect.l);
                let top = first.rect.t - cell.rect.t;
                let bottom = cell.rect.b - cell.blocks.last().unwrap().rect.b;
                cell.valign = if top > 3.0 && (top - bottom).abs() < 3.0 + top * 0.15 {
                    VAlign::Center
                } else if bottom < top - 3.0 && bottom < 4.0 {
                    VAlign::Bottom
                } else {
                    VAlign::Top
                };
            }
        }
        // Small padding: Word then has at least the room the PDF had, so cells do not
        // wrap into more lines (indents keep the text where it was)
        let padding = insets.iter().copied().fold(f32::MAX, f32::min).clamp(0.5, 2.0);
        let block = Block { rect, content: Content::Table(Table { xs: lat.xs, ys: lat.ys, cells, padding }) };
        match parents[ti] {
            Some(cell) => nested.entry(cell).or_default().push(block),
            None => table_blocks.push(block),
        }
    }
    let blocks = flow(&free_glyphs, free_images, text_right, &art, table_blocks);

    // The rules drawn as underlines of text (Word underlines that text instead)
    let underline_rules: Vec<Rect> = underlines
        .iter()
        .filter(|s| underlined.iter().any(|g| s.pos > g.baseline - 0.5 && s.pos < g.baseline + g.style.size * 0.35 && s.a <= g.right && s.b >= g.left))
        .map(|s| s.rect())
        .collect();
    PageModel { width: page_w, height: page_h, header: vec![], body: blocks, footer: vec![], decor, background: None, number: 0, underline_rules }
}

// ---------------------------------------------------------------------------
// Running headers and footers

/// What a block shows, with the page number (and page count) as placeholders, so the
/// same footer on page 3 and page 4 compares equal. `with_position`: also where it is
/// (roughly), to tell apart portrait and landscape versions of a footer.
pub(super) fn signature(block: &Block, page: usize, total: usize, with_position: bool) -> String {
    let pos = if with_position { format!("@{:.0},{:.0}", block.rect.l / 12.0, block.rect.t / 12.0) } else { String::new() };
    match &block.content {
        Content::Paragraph(lines) => {
            let text: String = lines.iter().map(|l| l.text()).collect::<Vec<_>>().join("\n");
            format!("p{}{pos}", number_placeholders(&text, page, total))
        }
        Content::Images(imgs) => format!("i{}{pos}", imgs.iter().map(|i| format!("{}:{:.0}x{:.0}", i.media, i.rect.w(), i.rect.h())).collect::<Vec<_>>().join(",")),
        Content::Table(t) => {
            let cells: Vec<String> = t.cells.iter().map(|c| c.blocks.iter().map(|b| signature(b, page, total, false)).collect::<Vec<_>>().join("|")).collect();
            format!("t{}x{}:{}{pos}", t.xs.len(), t.ys.len(), cells.join(";"))
        }
    }
}

/// Splits text into pieces: plain text, or a number that equals the page number / count
#[derive(Debug, PartialEq)]
pub(super) enum Piece<'a> {
    Text(&'a str),
    Page,
    Pages,
}

pub(super) fn split_numbers(text: &str, page: usize, total: usize) -> Vec<Piece<'_>> {
    let mut out = vec![];
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let s = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // Only standalone numbers ("3", "3/12", "Page 3"), not parts of codes ("QO-1007")
            let before = text[..s].chars().next_back();
            let after = text[i..].chars().next();
            let standalone = !before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_') && !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_');
            let value: usize = text[s..i].parse().unwrap_or(usize::MAX);
            let piece = if standalone && value == page { Some(Piece::Page) } else if standalone && value == total && total > 1 { Some(Piece::Pages) } else { None };
            if let Some(p) = piece {
                if start < s {
                    out.push(Piece::Text(&text[start..s]));
                }
                out.push(p);
                start = i;
            }
        } else {
            i += 1;
        }
    }
    if start < text.len() {
        out.push(Piece::Text(&text[start..]));
    }
    out
}

fn number_placeholders(text: &str, page: usize, total: usize) -> String {
    split_numbers(text, page, total)
        .into_iter()
        .map(|p| match p {
            Piece::Text(t) => t.to_string(),
            Piece::Page => "{PAGE}".into(),
            Piece::Pages => "{PAGES}".into(),
        })
        .collect()
}

/// The printed page numbers: the PDF's page index plus the offset that most pages'
/// top/bottom numbers agree on (0 when there is none)
fn number_pages(pages: &mut [PageModel]) {
    let total = pages.len();
    let mut votes: HashMap<i64, usize> = HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        let mut seen = std::collections::HashSet::new();
        for b in p.body.iter().filter(|b| b.rect.b < p.height * 0.2 || b.rect.t > p.height * 0.82) {
            let Content::Paragraph(lines) = &b.content else { continue };
            for line in lines {
                let text = line.text();
                for token in text.split(|c: char| !c.is_ascii_digit()).filter(|t| !t.is_empty() && t.len() <= 5) {
                    if let Ok(n) = token.parse::<i64>() {
                        let offset = n - (i as i64 + 1);
                        if seen.insert(offset) {
                            *votes.entry(offset).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    let needed = (total / 2).max(2);
    let offset = votes
        .into_iter()
        .filter(|(o, n)| *n >= needed && *o > -(total as i64))
        .max_by_key(|(o, n)| (*n, *o == 0))
        .map(|(o, _)| o)
        .unwrap_or(0);
    for (i, p) in pages.iter_mut().enumerate() {
        p.number = (i as i64 + 1 + offset).max(0) as usize;
    }
}

/// Moves blocks that repeat near the top/bottom of several pages into the header/footer
fn split_running_content(pages: &mut [PageModel]) {
    let total = pages.len();
    if total < 2 {
        return;
    }
    let band = |p: &PageModel, b: &Block| -> Option<bool> {
        if b.rect.b < p.height * 0.2 {
            Some(true)
        } else if b.rect.t > p.height * 0.82 {
            Some(false)
        } else {
            None
        }
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in pages.iter() {
        let mut seen = std::collections::HashSet::new();
        for b in &p.body {
            if band(p, b).is_some() {
                let sig = signature(b, p.number, total, false);
                if seen.insert(sig.clone()) {
                    *counts.entry(sig).or_default() += 1;
                }
            }
        }
    }
    // Twice is enough: it must also sit in the top/bottom band, and run-of-the-mill text
    // does not repeat word for word there
    let needed = 2;
    for p in pages.iter_mut() {
        let body = std::mem::take(&mut p.body);
        let height = p.height;
        for b in body {
            let running = counts.get(&signature(&b, p.number, total, false)).is_some_and(|n| *n >= needed);
            let is_header = if b.rect.b < height * 0.2 { Some(true) } else if b.rect.t > height * 0.82 { Some(false) } else { None };
            match (running, is_header) {
                (true, Some(true)) => p.header.push(b),
                (true, Some(false)) => p.footer.push(b),
                _ => p.body.push(b),
            }
        }
        // Running content must not sit between body blocks: keep only the header
        // blocks above all body text, and footer blocks below it
        let body_top = p.body.iter().map(|b| b.rect.t).fold(f32::MAX, f32::min);
        let body_bottom = p.body.iter().map(|b| b.rect.b).fold(f32::MIN, f32::max);
        let (header, back): (Vec<Block>, Vec<Block>) = std::mem::take(&mut p.header).into_iter().partition(|b| b.rect.b <= body_top + 2.0);
        p.header = header;
        p.body.extend(back);
        let (footer, back): (Vec<Block>, Vec<Block>) = std::mem::take(&mut p.footer).into_iter().partition(|b| b.rect.t >= body_bottom - 2.0);
        p.footer = footer;
        p.body.extend(back);
        p.body.sort_by(|a, b| a.rect.t.partial_cmp(&b.rect.t).unwrap_or(std::cmp::Ordering::Equal));
    }
}

/// Converts PDF bytes to a .docx file, one Word page per PDF page.
pub fn pdf_to_docx(pdfium: &Pdfium, pdf: &[u8], on_page: &dyn Fn(usize, usize)) -> Result<Vec<u8>, String> {
    let document = pdfium.load_pdf_from_byte_slice(pdf, None).map_err(|e| {
        if format!("{e:?}").contains("Password") {
            "the PDF is password-protected".to_string()
        } else {
            "the PDF could not be opened".to_string()
        }
    })?;
    let pages = document.pages();
    let total = pages.len() as usize;
    if total == 0 {
        return Err("the PDF has no pages".into());
    }
    let mut media = MediaStore::default();
    let mut models = vec![];
    // A second copy of the document: its pages are stripped down to their decoration
    // before rendering (the first copy stays intact for reading)
    let mut stripped: Option<PdfDocument> = None;
    for (index, page) in pages.iter().enumerate() {
        on_page(index, total);
        let mut model = page_model(&page, &document, &mut media);
        let annotated = !page.annotations().is_empty();
        if !model.decor.is_empty() || annotated {
            if stripped.is_none() {
                stripped = pdfium.load_pdf_from_byte_slice(pdf, None).ok();
            }
            if let Some(doc) = &stripped {
                model.background = background(doc, index, &model.decor, &model.underline_rules, &mut media);
            }
        }
        models.push(model);
    }
    number_pages(&mut models);
    split_running_content(&mut models);
    on_page(total, total);
    writer::write(&models, &media)
}

/// A page read by text recognition: its size (points) and the lines found on it
pub struct OcrPage {
    pub width: f32,
    pub height: f32,
    pub lines: Vec<super::ocr::TextLine>,
    /// The page as scanned: with the recognised text taken out, it becomes the page's
    /// background (logos, lines, tables, photos, signatures stay where they were)
    pub scan: Option<image::RgbaImage>,
}

/// Font sizes from text boxes vary with the letters in them (descenders, capitals):
/// sizes close to the page's usual body size snap to it, headings and small print
/// keep their own
fn snap_sizes(sizes: &[f32]) -> Vec<f32> {
    let mut sorted: Vec<f32> = sizes.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The size most lines are near
    let body = sorted
        .iter()
        .map(|&s| (s, sorted.iter().filter(|&&o| (o - s).abs() <= s * 0.2).count()))
        .max_by_key(|&(_, n)| n)
        .map(|(s, _)| s)
        .unwrap_or(11.0);
    let near: Vec<f32> = sorted.iter().copied().filter(|s| (s - body).abs() <= body * 0.25).collect();
    let body = near.iter().sum::<f32>() / near.len().max(1) as f32;
    // Recognised boxes of short pieces without tall letters ("semakin") come out much
    // lower than the text really is: anything from 2/3 to 1.3 x is body text
    sizes
        .iter()
        .map(|&s| if s >= body * 0.66 && s <= body * 1.3 { body } else { s })
        .map(|s| (s * 2.0).round() / 2.0)
        .collect()
}

/// Erases one recognised box from the scan, row by row, blending from the colour just
/// left of it to the colour just right of it (smooth on gradients and photos), and
/// returns the colour of the ink that was there
fn erase_box(img: &mut image::RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32) -> [u8; 3] {
    let (w, _) = img.dimensions();
    let sample = |img: &image::RgbaImage, x: u32, y: u32| -> [f32; 3] {
        let p = img.get_pixel(x.min(w - 1), y);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    // Ink: the pixels that differ most from the paper around them
    let mut ink_sum = [0f32; 3];
    let mut ink_n = 0f32;
    let mut rows: Vec<([f32; 3], [f32; 3])> = vec![];
    for y in y0..=y1 {
        let left = sample(img, x0.saturating_sub(2), y);
        let right = sample(img, x1 + 2, y);
        rows.push((left, right));
        let paper = [(left[0] + right[0]) / 2.0, (left[1] + right[1]) / 2.0, (left[2] + right[2]) / 2.0];
        for x in (x0..=x1).step_by(2) {
            let p = sample(img, x, y);
            let d = (0..3).map(|i| (p[i] - paper[i]).abs()).sum::<f32>();
            if d > 150.0 {
                (0..3).for_each(|i| ink_sum[i] += p[i]);
                ink_n += 1.0;
            }
        }
    }
    for (k, y) in (y0..=y1).enumerate() {
        let (left, right) = rows[k];
        let span = (x1 - x0).max(1) as f32;
        for x in x0..=x1 {
            let t = (x - x0) as f32 / span;
            let c = [0, 1, 2].map(|i| (left[i] + (right[i] - left[i]) * t).round().clamp(0.0, 255.0) as u8);
            img.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
        }
    }
    if ink_n > 0.0 {
        [0, 1, 2].map(|i| (ink_sum[i] / ink_n).round() as u8)
    } else {
        [0, 0, 0]
    }
}

/// The scan with every recognised line erased, and each line's text colour
fn erase_text(scan: &image::RgbaImage, lines: &[super::ocr::TextLine]) -> (image::RgbaImage, Vec<[u8; 3]>) {
    let mut img = scan.clone();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let mut colors = vec![];
    for line in lines {
        let [l, t, r, b] = line.rect;
        let pad_y = (b - t) * h * 0.18;
        let pad_x = (b - t) * h * 0.3;
        let x0 = ((l * w - pad_x).max(0.0)) as u32;
        let y0 = ((t * h - pad_y).max(0.0)) as u32;
        let x1 = ((r * w + pad_x).min(w - 1.0)) as u32;
        let y1 = ((b * h + pad_y).min(h - 1.0)) as u32;
        if x1 <= x0 || y1 <= y0 {
            colors.push([0, 0, 0]);
            continue;
        }
        colors.push(erase_box(&mut img, x0, y0, x1, y1));
    }
    (img, colors)
}

/// Recognised text -> .docx, through the same layout reconstruction as PDF text:
/// every recognised line becomes characters spread over its box, so paragraphs,
/// alignment, indents, columns, running headers/footers and page numbers come out
/// the same way. (The scan itself is not kept: the result is the editable text.)
pub fn ocr_to_docx(pages: Vec<OcrPage>) -> Result<Vec<u8>, String> {
    if pages.iter().all(|p| p.lines.is_empty()) {
        return Err("no text was found to recognise".into());
    }
    let mut media = MediaStore::default();
    let mut models = vec![];
    for page in &pages {
        let mut glyphs = vec![];
        let (erased, colors) = match &page.scan {
            Some(scan) => {
                let (img, colors) = erase_text(scan, &page.lines);
                (Some(img), colors)
            }
            None => (None, vec![[0, 0, 0]; page.lines.len()]),
        };
        // A text box spans ascenders to descenders: about 1.2 x the font size. The
        // median word of a line decides, so one tall mark (a superscript) does not
        let box_height = |l: &super::ocr::TextLine| -> f32 {
            let mut hs: Vec<f32> = l.words.iter().filter(|w| w.0.chars().count() >= 2).map(|w| w.1[3] - w.1[1]).collect();
            if hs.is_empty() {
                return l.rect[3] - l.rect[1];
            }
            hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            hs[hs.len() / 2]
        };
        let raw: Vec<f32> = page.lines.iter().map(|l| (box_height(l) * page.height / 1.2).clamp(4.0, 72.0)).collect();
        let sizes = snap_sizes(&raw);
        for ((line, &size), &ink) in page.lines.iter().zip(&sizes).zip(&colors) {
            let [l, t, r, b] = line.rect;
            let (l, t, r, b) = (l * page.width, t * page.height, r * page.width, b * page.height);
            let _ = t;
            let h = (box_height(line) * page.height).max(1.0);
            let baseline = b - h * 0.2;
            // Dark ink is plain text colour; anything else keeps its colour
            let color = (ink.iter().any(|v| *v > 90)).then_some(ink);
            let style = Style { font: "Arial".into(), size, bold: false, italic: false, underline: false, color, spacing: 0.0, vert: 0 };
            // Characters spread evenly over each word's box (or the whole line's)
            let pieces: Vec<(String, f32, f32)> = if line.words.is_empty() {
                vec![(line.text.clone(), l, r)]
            } else {
                line.words.iter().map(|(w, rect)| (w.clone(), rect[0] * page.width, rect[2] * page.width)).collect()
            };
            glyphs.push(Glyph { ch: '\n', left: l, right: l, baseline, style: style.clone(), generated: false });
            for (k, (word, wl, wr)) in pieces.iter().enumerate() {
                let chars: Vec<char> = word.chars().collect();
                let step = (wr - wl) / chars.len().max(1) as f32;
                if k > 0 {
                    glyphs.push(Glyph { ch: ' ', left: *wl, right: *wl, baseline, style: style.clone(), generated: false });
                }
                for (i, ch) in chars.iter().enumerate() {
                    let left = wl + step * i as f32;
                    glyphs.push(Glyph { ch: *ch, left, right: left + step * 0.95, baseline, style: style.clone(), generated: false });
                }
            }
        }
        let refs: Vec<&Glyph> = glyphs.iter().collect();
        let text_right = glyphs.iter().map(|g| g.right).fold(0.0, f32::max).max(page.width * 0.6);
        let body = flow(&refs, vec![], text_right, &[], vec![]);
        // What is left of the scan without its text, unless that is just blank paper
        let background = erased.and_then(|erased| {
            let max_w = (page.width * 150.0 / 72.0) as u32;
            let small = if erased.width() > max_w {
                image::imageops::resize(&erased, max_w, (erased.height() as f32 * max_w as f32 / erased.width() as f32) as u32, image::imageops::FilterType::Triangle)
            } else {
                erased
            };
            let ink = small.pixels().filter(|p| p[0].min(p[1]).min(p[2]) < 200).count();
            if ink * 1000 < small.pixels().len() {
                return None;
            }
            let mut jpg = Vec::new();
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 80);
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(small).to_rgb8()).write_with_encoder(encoder).ok()?;
            Some(media.add(jpg, "jpeg"))
        });
        models.push(PageModel {
            width: page.width,
            height: page.height,
            header: vec![],
            body,
            footer: vec![],
            decor: vec![],
            background,
            number: 0,
            underline_rules: vec![],
        });
    }
    number_pages(&mut models);
    split_running_content(&mut models);
    writer::write(&models, &media)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_names_are_cleaned() {
        assert_eq!(clean_font_name("ABCDEF+TimesNewRomanPS-BoldMT"), "Times New Roman");
        assert_eq!(clean_font_name("Calibri-Bold"), "Calibri");
        assert_eq!(clean_font_name("Helvetica"), "Arial");
        assert_eq!(clean_font_name("ArialMT"), "Arial");
    }

    #[test]
    fn detects_list_items() {
        assert!(starts_list_item("• Fast"));
        assert!(starts_list_item("12. Twelfth"));
        assert!(!starts_list_item("2026 was a year"));
        assert!(!starts_list_item("Plain text"));
        assert!(is_enumerator("a."));
        assert!(is_enumerator("(iv)"));
        assert!(!is_enumerator("word"));
    }

    #[test]
    fn page_numbers_become_fields() {
        assert_eq!(split_numbers("Page 4 of 12", 4, 12), vec![Piece::Text("Page "), Piece::Page, Piece::Text(" of "), Piece::Pages]);
        assert_eq!(split_numbers("Halaman 4/81", 4, 112), vec![Piece::Text("Halaman "), Piece::Page, Piece::Text("/81")]);
        // Codes and dates are left alone
        assert_eq!(split_numbers("WI-QO-1007.00 v4.2", 4, 12), vec![Piece::Text("WI-QO-1007.00 v4.2")]);
    }

    fn style() -> Style {
        Style { font: "Arial".into(), size: 10.0, bold: false, italic: false, underline: false, color: None, spacing: 0.0, vert: 0 }
    }

    fn glyph(ch: char, left: f32, baseline: f32) -> Glyph {
        Glyph { ch, left, right: left + 5.0, baseline, style: style(), generated: false }
    }

    #[test]
    fn dot_leaders_become_one_tab() {
        let mut gs = vec![];
        let mut x = 10.0;
        for c in "Intro".chars() {
            gs.push(glyph(c, x, 100.0));
            x += 5.0;
        }
        for _ in 0..20 {
            gs.push(glyph('.', x, 100.0));
            x += 5.0;
        }
        gs.push(glyph('7', x + 2.0, 100.0));
        let refs: Vec<&Glyph> = gs.iter().collect();
        let ls = lines(&refs, &[]);
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].text(), "Intro\t7");
        assert_eq!(ls[0].tabs.len(), 1);
        assert!(ls[0].tabs[0].leader);
        assert!((ls[0].tabs[0].left - (x + 2.0)).abs() < 0.01);
    }

    #[test]
    fn row_rules_without_column_lines_become_tables() {
        let black = [0, 0, 0];
        let h = |y: f32| Seg { horizontal: true, pos: y, a: 50.0, b: 300.0, width: 0.5, color: black };
        let segs = vec![h(100.0), h(120.0), h(140.0)];
        let groups = segment_groups(&segs);
        let stacks = rule_stacks(&segs, &groups);
        assert_eq!(stacks.len(), 1);
        // Two columns of text between the rules
        let mut glyphs = vec![];
        for (row, y) in [(0, 114.0), (1, 134.0)] {
            for (k, c) in "Name".chars().enumerate() {
                glyphs.push(glyph(c, 60.0 + k as f32 * 5.0, y));
            }
            for (k, c) in format!("Value{row}").chars().enumerate() {
                glyphs.push(glyph(c, 200.0 + k as f32 * 5.0, y));
            }
        }
        let (_, lat) = rule_table(&segs, &stacks[0], &glyphs).unwrap();
        assert_eq!((lat.xs.len() - 1, lat.ys.len() - 1), (2, 2));
        assert!(lat.cells.iter().all(|c| c.borders[0].is_some() && c.borders[1].is_none()));
    }

    #[test]
    fn list_numbers_drawn_after_their_text_join_the_line() {
        let mut gs = vec![];
        for (k, c) in "Payment terms".chars().enumerate() {
            gs.push(glyph(c, 40.0 + k as f32 * 5.0, 100.0));
        }
        // "2." comes later in the PDF, a hair lower, left of the text
        gs.push(glyph('2', 26.0, 100.4));
        gs.push(glyph('.', 31.0, 100.4));
        let refs: Vec<&Glyph> = gs.iter().collect();
        let ls = lines(&refs, &[]);
        assert_eq!(ls.len(), 1);
        assert!(ls[0].text().starts_with("2."), "{}", ls[0].text());
    }

    #[test]
    fn grids_of_lines_become_tables() {
        let black = [0, 0, 0];
        let h = |y: f32, a: f32, b: f32| Seg { horizontal: true, pos: y, a, b, width: 0.5, color: black };
        let v = |x: f32, a: f32, b: f32| Seg { horizontal: false, pos: x, a, b, width: 0.5, color: black };
        // 2 x 2 grid whose top row is one merged cell
        let segs = vec![h(0.0, 0.0, 200.0), h(20.0, 0.0, 200.0), h(40.0, 0.0, 200.0), v(0.0, 0.0, 40.0), v(200.0, 0.0, 40.0), v(100.0, 20.0, 40.0)];
        let groups = segment_groups(&segs);
        assert_eq!(groups.len(), 1);
        let lat = lattice(&segs, &groups[0], &[]).unwrap();
        assert_eq!(lat.xs.len(), 3);
        assert_eq!(lat.ys.len(), 3);
        assert_eq!(lat.cells.len(), 3);
        assert_eq!((lat.cells[0].cols, lat.cells[0].rows), (2, 1));
        assert!(lat.cells.iter().all(|c| c.borders.iter().all(|b| b.is_some())));
    }
}

#[cfg(test)]
mod review {
    use super::*;

    /// What the analysis sees on one page:
    /// `FLIPFILE_PDF=in.pdf FLIPFILE_PAGE=2 cargo test dump_pdf_page -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_pdf_page() {
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_file(&std::env::var("FLIPFILE_PDF").unwrap(), None).unwrap();
        let index: i32 = std::env::var("FLIPFILE_PAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        let page = doc.pages().get(index - 1).unwrap();
        let text = page.text().unwrap();
        let mut modes: HashMap<String, usize> = HashMap::new();
        for c in text.chars().iter() {
            *modes.entry(format!("{:?}", c.render_mode().ok())).or_default() += 1;
        }
        println!("render modes: {modes:?}");
        let mut kinds: HashMap<String, usize> = HashMap::new();
        for o in page.objects().iter() {
            *kinds.entry(format!("{:?}", o.object_type())).or_default() += 1;
        }
        println!("objects: {kinds:?}");
        if let Ok(probe) = std::env::var("FLIPFILE_CHARS") {
            // Raw characters of the first line containing `probe`'s first letter run
            for c in text.chars().iter().take(400) {
                let ch = c.unicode_char().unwrap_or('?');
                if probe.contains(ch) || ch == ' ' {
                    let b = c.loose_bounds().ok();
                    println!("{ch:?} gen={:?} l={:?} r={:?}", c.is_generated().ok(), b.map(|b| b.left().value), b.map(|b| b.right().value));
                }
            }
        }
        let mut media = MediaStore::default();
        let model = page_model(&page, &doc, &mut media);
        if let Ok(needle) = std::env::var("FLIPFILE_LINES") {
            // Baselines of every line containing the needle's words, tables included
            fn walk(blocks: &[Block], needle: &str) {
                for b in blocks {
                    match &b.content {
                        Content::Paragraph(lines) => {
                            for l in lines.iter().filter(|l| needle.split('|').any(|n| l.text().contains(n))) {
                                println!("{:7.2} {:6.2} size {:4.1}  {}  [{}]", l.baseline, l.left, l.size, l.text(), l.runs.first().map(|r| r.1.font.as_str()).unwrap_or(""));
                            }
                        }
                        Content::Table(t) => t.cells.iter().for_each(|c| walk(&c.blocks, needle)),
                        Content::Images(_) => {}
                    }
                }
            }
            walk(&model.body, &needle);
            walk(&model.header, &needle);
            return;
        }
        for b in &model.body {
            let kind = match &b.content {
                Content::Paragraph(lines) => format!("P {:?}", lines.iter().map(|l| l.text()).collect::<Vec<_>>()),
                Content::Table(t) => format!("T {}x{} cells {} rows at {:?}", t.xs.len() - 1, t.ys.len() - 1, t.cells.len(), t.ys.iter().map(|y| (y * 10.0).round() / 10.0).collect::<Vec<_>>()),
                Content::Images(i) => format!("I {}", i.len()),
            };
            println!("{:7.1} {:7.1} {:7.1} {:7.1}  {}", b.rect.l, b.rect.t, b.rect.r, b.rect.b, kind.chars().take(160).collect::<String>());
        }
    }
}
