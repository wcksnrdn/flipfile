// Reads a PowerPoint presentation (.pptx) into slides that are ready to draw: every
// value PowerPoint inherits (slide -> layout -> master -> theme, and the presentation's
// default text style) is resolved here, so the slides hold plain positions in points,
// colours, fonts and sizes.
//
// Covered: slide size, backgrounds (colour, gradient, picture), shapes with preset or
// custom geometry, fills, outlines, rotation and flips, text with paragraph and run
// formatting, bullets and numbering, autofit, placeholders inherited from layouts and
// masters, theme colours and fonts, shape styles, pictures (cropped), groups, tables
// (with the default table style), SmartArt (its saved drawing), slide numbers, hidden
// slides and speaker notes. Not drawn: charts, video, shadows and 3-D effects.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use roxmltree::{Document, Node};

/// EMU (English Metric Units) per point
const EMU: f32 = 12_700.0;

pub struct Presentation {
    /// Slide size in points
    pub width: f32,
    pub height: f32,
    pub slides: Vec<Slide>,
    /// Pictures the slides use, by their path in the package
    pub media: HashMap<String, Vec<u8>>,
    /// Fonts saved in the file (PowerPoint's "Embed fonts in the file"): the typeface
    /// name the slides use, and its TrueType/OpenType data
    pub fonts: Vec<(String, Vec<u8>)>,
}

pub struct Slide {
    pub hidden: bool,
    pub background: Option<Fill>,
    pub elements: Vec<Element>,
    pub title: String,
    pub notes: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub rgb: [u8; 3],
    /// 0 transparent .. 1 opaque
    pub alpha: f32,
}

#[derive(Clone, Debug)]
pub enum Fill {
    Solid(Rgba),
    /// Stops (0..1, colour) and the angle the colours change along, in degrees
    Gradient(Vec<(f32, Rgba)>, f32),
    /// A picture from `Presentation::media`, stretched over the area
    Picture(String),
}

#[derive(Clone, Debug)]
pub struct Line {
    pub color: Rgba,
    /// Points
    pub width: f32,
    pub dash: Option<&'static str>,
    /// Arrowheads at the start and end: kind, width and length as multiples of the width
    pub head: Option<(LineEnd, f32, f32)>,
    pub tail: Option<(LineEnd, f32, f32)>,
}

/// Where an element sits, in points from the slide's top-left corner
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Clockwise, in degrees
    pub rotation: f32,
    pub flip_h: bool,
    pub flip_v: bool,
}

pub use super::shapes::{Geometry, LineEnd, PathCommand, PathFill};

#[derive(Clone, Debug)]
pub enum Element {
    Shape {
        frame: Frame,
        geometry: Geometry,
        fill: Option<Fill>,
        line: Option<Line>,
        text: Option<TextBody>,
        /// The placeholder it fills on its slide (title, body, sldNum, …)
        placeholder: Option<String>,
    },
    Picture {
        frame: Frame,
        media: String,
        /// left, top, right, bottom cut off, as fractions
        crop: [f32; 4],
        /// The outline the picture is cut to
        geometry: Geometry,
        line: Option<Line>,
    },
    Table { frame: Frame, columns: Vec<f32>, rows: Vec<TableRow> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug)]
pub struct TextBody {
    /// left, top, right, bottom, in points
    pub insets: [f32; 4],
    pub anchor: Anchor,
    pub wrap: bool,
    /// The shape resizes to fit its text (spAutoFit): its saved height is that of the
    /// text as the author's app laid it out
    pub grows: bool,
    pub paragraphs: Vec<Paragraph>,
}

#[derive(Clone, Debug)]
pub struct Paragraph {
    pub align: Align,
    /// Left margin of the text and the first line's offset from it (negative: hanging)
    pub margin_left: f32,
    pub indent: f32,
    pub space_before: f32,
    pub space_after: f32,
    /// A multiple of single spacing, or exact points
    pub line_spacing: LineSpacing,
    pub bullet: Option<Bullet>,
    pub runs: Vec<Run>,
    /// Size of an empty paragraph's line
    pub size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineSpacing {
    Multiple(f32),
    Points(f32),
}

#[derive(Clone, Debug)]
pub struct Bullet {
    pub text: String,
    pub font: Option<String>,
    pub color: Option<Rgba>,
    /// Size relative to the first run
    pub scale: f32,
    /// Or a size of its own, in points
    pub points: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Rgba,
    /// Raised (superscript, positive) or lowered, as a fraction of the size
    pub baseline: f32,
    /// Kerned (PowerPoint kerns only from a size the text asks for)
    pub kern: bool,
    /// Extra space between letters, in points
    pub spacing: f32,
    pub highlight: Option<Rgba>,
    pub caps: Caps,
    /// A line break instead of text
    pub line_break: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Caps {
    #[default]
    None,
    All,
    Small,
}

#[derive(Clone, Debug)]
pub struct TableRow {
    pub height: f32,
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug)]
pub struct TableCell {
    pub col_span: usize,
    pub row_span: usize,
    /// Covered by a neighbour's span: not drawn
    pub merged: bool,
    pub fill: Option<Fill>,
    /// left, top, right, bottom
    pub borders: [Option<Line>; 4],
    pub text: TextBody,
}

// ---------------------------------------------------------------------------
// XML helpers (matched by local name: the same shapes appear as p:sp, dsp:sp, …)

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn children<'a, 'i: 'a>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn path<'a, 'i>(node: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    names.iter().try_fold(node, |n, name| child(n, name))
}

fn num(node: Node, name: &str) -> Option<f32> {
    node.attribute(name)?.trim().parse().ok().filter(|v: &f32| v.is_finite())
}

fn flag(node: Node, name: &str) -> Option<bool> {
    node.attribute(name).map(|v| v == "1" || v == "true" || v == "on")
}

/// A length in points, within what a slide can hold (PowerPoint's limit is 56 in)
fn emu(node: Node, name: &str) -> Option<f32> {
    num(node, name).map(|v| (v / EMU).clamp(-100_000.0, 100_000.0))
}

// ---------------------------------------------------------------------------
// Package: parts and their relationships

/// The parts of the package, unpacked when asked for, within a budget: a small
/// hostile file cannot expand into gigabytes
struct Package {
    zip: std::cell::RefCell<zip::ZipArchive<Cursor<Vec<u8>>>>,
    /// Part name (without a leading slash) -> its index in the archive
    names: HashMap<String, usize>,
    budget: std::cell::Cell<u64>,
}

/// Unpacked bytes one presentation may use, all parts together
const UNPACK_BUDGET: u64 = 1 << 30;

impl Package {
    fn open(bytes: &[u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|_| "this is not a PowerPoint (.pptx) file".to_string())?;
        let mut names = HashMap::new();
        for i in 0..zip.len() {
            let Ok(file) = zip.by_index_raw(i) else { continue };
            if file.is_dir() {
                continue;
            }
            if let Ok(name) = file.name() {
                names.insert(name.trim_start_matches('/').to_string(), i);
            }
        }
        Ok(Package { zip: std::cell::RefCell::new(zip), names, budget: std::cell::Cell::new(UNPACK_BUDGET) })
    }

    fn get(&self, name: &str) -> Option<Vec<u8>> {
        let index = *self.names.get(name)?;
        let mut zip = self.zip.borrow_mut();
        let file = zip.by_index(index).ok()?;
        let left = self.budget.get();
        // Honest sizes beyond the budget are refused before anything is unpacked
        if file.size() > left {
            return None;
        }
        let mut data = Vec::new();
        file.take(left + 1).read_to_end(&mut data).ok()?;
        if data.len() as u64 > left {
            self.budget.set(0);
            return None;
        }
        self.budget.set(left - data.len() as u64);
        Some(data)
    }

    fn text(&self, name: &str) -> Option<String> {
        let text = self.get(name).map(|b| String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))?;
        // Nested too deep to parse safely: as good as missing
        (!super::xml_too_deep(&text)).then_some(text)
    }

    /// The part's relationships: id -> (type, target path)
    fn rels(&self, part: &str) -> HashMap<String, (String, String)> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels_path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        let mut out = HashMap::new();
        let Some(xml) = self.text(&rels_path) else { return out };
        let Ok(doc) = Document::parse(&xml) else { return out };
        for rel in doc.descendants().filter(|n| n.tag_name().name() == "Relationship") {
            let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) else { continue };
            if rel.attribute("TargetMode") == Some("External") {
                continue;
            }
            let kind = rel.attribute("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
            out.insert(id.to_string(), (kind, resolve(dir, target)));
        }
        out
    }
}

/// `../media/image1.png` relative to `ppt/slides` -> `ppt/media/image1.png`
fn resolve(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { vec![] } else { dir.split('/').filter(|p| !p.is_empty()).collect() };
    for piece in target.split('/') {
        match piece {
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
// Theme: colours, fonts, fill and line styles

struct Theme {
    colors: HashMap<String, [u8; 3]>,
    major_font: String,
    minor_font: String,
    fill_styles: Vec<String>,
    background_styles: Vec<String>,
    line_styles: Vec<String>,
}

impl Theme {
    fn parse(xml: &str) -> Theme {
        let mut theme = Theme {
            colors: HashMap::new(),
            major_font: "Calibri Light".into(),
            minor_font: "Calibri".into(),
            fill_styles: vec![],
            background_styles: vec![],
            line_styles: vec![],
        };
        let Ok(doc) = Document::parse(xml) else { return theme };
        if let Some(scheme) = doc.descendants().find(|n| n.tag_name().name() == "clrScheme") {
            for slot in scheme.children().filter(Node::is_element) {
                let color = slot.children().find(Node::is_element).and_then(|c| plain_color(c));
                if let Some(color) = color {
                    theme.colors.insert(slot.tag_name().name().to_string(), color);
                }
            }
        }
        let font = |kind: &str| {
            doc.descendants()
                .find(|n| n.tag_name().name() == kind)
                .and_then(|f| child(f, "latin"))
                .and_then(|l| l.attribute("typeface"))
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        };
        if let Some(f) = font("majorFont") {
            theme.major_font = f;
        }
        if let Some(f) = font("minorFont") {
            theme.minor_font = f;
        }
        // Style lists are kept as XML, resolved with the colour of whoever refers to them
        let list = |name: &str| -> Vec<String> {
            doc.descendants()
                .find(|n| n.tag_name().name() == name)
                .map(|l| l.children().filter(Node::is_element).map(|c| xml[c.range()].to_string()).collect())
                .unwrap_or_default()
        };
        theme.fill_styles = list("fillStyleLst");
        theme.background_styles = list("bgFillStyleLst");
        theme.line_styles = list("lnStyleLst");
        theme
    }
}

/// A colour that needs no theme (srgbClr, sysClr, prstClr), without modifiers
fn plain_color(node: Node) -> Option<[u8; 3]> {
    match node.tag_name().name() {
        "srgbClr" => hex(node.attribute("val")?),
        "sysClr" => node.attribute("lastClr").and_then(hex).or_else(|| match node.attribute("val")? {
            "window" => Some([255, 255, 255]),
            _ => Some([0, 0, 0]),
        }),
        "prstClr" => preset_color(node.attribute("val")?),
        _ => None,
    }
}

fn hex(v: &str) -> Option<[u8; 3]> {
    let v = u32::from_str_radix(v.get(..6)?, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn preset_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "gray" | "grey" => [128, 128, 128],
        "lightGray" | "ltGray" => [211, 211, 211],
        "darkGray" | "dkGray" => [169, 169, 169],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "navy" => [0, 0, 128],
        "maroon" => [128, 0, 0],
        "olive" => [128, 128, 0],
        "teal" => [0, 128, 128],
        "silver" => [192, 192, 192],
        "lime" => [0, 255, 0],
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Colour resolution with a theme, a colour map and modifiers

#[derive(Clone)]
struct Colors<'t> {
    theme: &'t Theme,
    /// bg1/tx1/bg2/tx2/… -> theme slot (from the master, or a slide's override)
    map: HashMap<String, String>,
    /// phClr: the colour a style reference brings
    placeholder: Option<Rgba>,
}

impl Colors<'_> {
    fn scheme(&self, name: &str) -> Option<Rgba> {
        if name == "phClr" {
            return self.placeholder;
        }
        let slot = self.map.get(name).map(String::as_str).unwrap_or(name);
        let slot = match slot {
            "bg1" => "lt1",
            "tx1" => "dk1",
            "bg2" => "lt2",
            "tx2" => "dk2",
            s => s,
        };
        self.theme.colors.get(slot).map(|&rgb| Rgba { rgb, alpha: 1.0 })
    }

    /// A colour element (srgbClr, schemeClr, …) with its modifiers
    fn color(&self, node: Node) -> Option<Rgba> {
        let base = match node.tag_name().name() {
            "schemeClr" => self.scheme(node.attribute("val")?)?,
            "scrgbClr" => {
                let c = |n: &str| (num(node, n).unwrap_or(0.0) / 100_000.0).clamp(0.0, 1.0);
                let gamma = |v: f32| (if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 } * 255.0).round() as u8;
                Rgba { rgb: [gamma(c("r")), gamma(c("g")), gamma(c("b"))], alpha: 1.0 }
            }
            "hslClr" => {
                let h = num(node, "hue").unwrap_or(0.0) / 60_000.0;
                let s = num(node, "sat").unwrap_or(0.0) / 100_000.0;
                let l = num(node, "lum").unwrap_or(0.0) / 100_000.0;
                Rgba { rgb: from_hsl(h, s, l), alpha: 1.0 }
            }
            _ => Rgba { rgb: plain_color(node)?, alpha: 1.0 },
        };
        Some(modify(base, node))
    }

    /// The colour inside a fill-like element (solidFill, a style reference, …)
    fn first(&self, node: Node) -> Option<Rgba> {
        node.children().filter(Node::is_element).find_map(|c| self.color(c))
    }

    fn with_placeholder(&self, placeholder: Option<Rgba>) -> Colors<'_> {
        Colors { theme: self.theme, map: self.map.clone(), placeholder }
    }
}

fn to_hsl([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let f = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    [f(r), f(g), f(b)]
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let c = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

/// Applies lumMod, lumOff, tint, shade, satMod and alpha, in document order
fn modify(mut color: Rgba, node: Node) -> Rgba {
    for m in node.children().filter(Node::is_element) {
        let v = num(m, "val").unwrap_or(100_000.0) / 100_000.0;
        match m.tag_name().name() {
            "lumMod" | "lumOff" | "satMod" | "satOff" => {
                let (h, mut s, mut l) = to_hsl(color.rgb);
                match m.tag_name().name() {
                    "lumMod" => l *= v,
                    "lumOff" => l += v,
                    "satMod" => s *= v,
                    _ => s += v,
                }
                color.rgb = from_hsl(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
            }
            // tint and shade work on linear light: tx1 tinted 75% is #898989, not #404040
            "tint" => color.rgb = color.rgb.map(|c| from_linear(1.0 - v * (1.0 - to_linear(c)))),
            "shade" => color.rgb = color.rgb.map(|c| from_linear(to_linear(c) * v)),
            "alpha" => color.alpha = v.clamp(0.0, 1.0),
            "gray" => {
                let g = (0.299 * color.rgb[0] as f32 + 0.587 * color.rgb[1] as f32 + 0.114 * color.rgb[2] as f32) as u8;
                color.rgb = [g; 3];
            }
            _ => {}
        }
    }
    color
}

// ---------------------------------------------------------------------------
// Text styles: nine levels of paragraph properties with a default run

#[derive(Clone, Debug, Default)]
struct RunStyle {
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<Rgba>,
    font: Option<String>,
    baseline: Option<f32>,
    /// Smallest size that is kerned
    kern: Option<f32>,
    spacing: Option<f32>,
    highlight: Option<Rgba>,
    caps: Option<Caps>,
}

impl RunStyle {
    fn parse(node: Node, colors: &Colors, theme: &Theme) -> RunStyle {
        RunStyle {
            size: num(node, "sz").map(|v| (v / 100.0).clamp(1.0, 4000.0)),
            bold: flag(node, "b"),
            italic: flag(node, "i"),
            underline: node.attribute("u").map(|u| u != "none"),
            strike: node.attribute("strike").map(|s| s != "noStrike"),
            color: child(node, "solidFill").and_then(|f| colors.first(f)).or_else(|| {
                // gradient text: its first colour
                child(node, "gradFill").and_then(|g| path(g, &["gsLst", "gs"])).and_then(|gs| colors.first(gs))
            }),
            font: child(node, "latin").and_then(|l| l.attribute("typeface")).map(|t| theme_font(t, theme)),
            baseline: num(node, "baseline").map(|v| v / 100_000.0),
            kern: num(node, "kern").map(|v| v / 100.0),
            spacing: num(node, "spc").map(|v| v / 100.0),
            highlight: child(node, "highlight").and_then(|h| colors.first(h)),
            caps: node.attribute("cap").map(|c| match c {
                "all" => Caps::All,
                "small" => Caps::Small,
                _ => Caps::None,
            }),
        }
    }

    fn over(&mut self, top: &RunStyle) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(size, bold, italic, underline, strike, color, font, baseline, kern, spacing, highlight, caps);
    }
}

fn theme_font(face: &str, theme: &Theme) -> String {
    match face {
        f if f.starts_with("+mj") => theme.major_font.clone(),
        f if f.starts_with("+mn") => theme.minor_font.clone(),
        f => f.to_string(),
    }
}

#[derive(Clone, Debug)]
enum BulletStyle {
    None,
    Char(String),
    Number(String, usize),
}

#[derive(Clone, Debug, Default)]
struct LevelStyle {
    margin_left: Option<f32>,
    indent: Option<f32>,
    align: Option<Align>,
    space_before: Option<Spacing>,
    space_after: Option<Spacing>,
    line_spacing: Option<LineSpacing>,
    bullet: Option<BulletStyle>,
    bullet_font: Option<String>,
    bullet_color: Option<Rgba>,
    bullet_scale: Option<f32>,
    bullet_points: Option<f32>,
    run: RunStyle,
}

#[derive(Clone, Copy, Debug)]
enum Spacing {
    Points(f32),
    /// Of a line (the font size × 1.2)
    Percent(f32),
}

fn spacing(node: Node) -> Option<Spacing> {
    if let Some(p) = child(node, "spcPts") {
        return num(p, "val").map(|v| Spacing::Points(v / 100.0));
    }
    child(node, "spcPct").and_then(|p| num(p, "val")).map(|v| Spacing::Percent(v / 100_000.0))
}

impl LevelStyle {
    fn parse(node: Node, colors: &Colors, theme: &Theme) -> LevelStyle {
        let mut s = LevelStyle {
            margin_left: emu(node, "marL"),
            indent: emu(node, "indent"),
            align: node.attribute("algn").map(|a| match a {
                "ctr" => Align::Center,
                "r" => Align::Right,
                "just" | "dist" | "justLow" | "thaiDist" => Align::Justify,
                _ => Align::Left,
            }),
            space_before: child(node, "spcBef").and_then(spacing),
            space_after: child(node, "spcAft").and_then(spacing),
            line_spacing: child(node, "lnSpc").and_then(|l| {
                if let Some(p) = child(l, "spcPct") {
                    num(p, "val").map(|v| LineSpacing::Multiple(v / 100_000.0))
                } else {
                    child(l, "spcPts").and_then(|p| num(p, "val")).map(|v| LineSpacing::Points(v / 100.0))
                }
            }),
            run: child(node, "defRPr").map(|r| RunStyle::parse(r, colors, theme)).unwrap_or_default(),
            ..Default::default()
        };
        if child(node, "buNone").is_some() {
            s.bullet = Some(BulletStyle::None);
        } else if let Some(c) = child(node, "buChar") {
            s.bullet = Some(BulletStyle::Char(c.attribute("char").unwrap_or("•").to_string()));
        } else if let Some(n) = child(node, "buAutoNum") {
            let kind = n.attribute("type").unwrap_or("arabicPeriod").to_string();
            s.bullet = Some(BulletStyle::Number(kind, num(n, "startAt").unwrap_or(1.0) as usize));
        }
        s.bullet_font = child(node, "buFont").and_then(|f| f.attribute("typeface")).map(|t| theme_font(t, theme));
        s.bullet_color = child(node, "buClr").and_then(|c| colors.first(c));
        s.bullet_scale = child(node, "buSzPct").and_then(|p| num(p, "val")).map(|v| v / 100_000.0);
        s.bullet_points = child(node, "buSzPts").and_then(|p| num(p, "val")).map(|v| v / 100.0);
        if child(node, "buSzTx").is_some() {
            s.bullet_scale = Some(1.0);
        }
        s
    }

    fn over(&mut self, top: &LevelStyle) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(margin_left, indent, align, space_before, space_after, line_spacing, bullet, bullet_font, bullet_color, bullet_scale, bullet_points);
        self.run.over(&top.run);
    }
}

#[derive(Clone, Debug, Default)]
struct Levels([LevelStyle; 9]);

impl Levels {
    /// A list style element (lstStyle, titleStyle, …): lvl1pPr .. lvl9pPr
    fn parse(node: Option<Node>, colors: &Colors, theme: &Theme) -> Levels {
        let mut levels = Levels::default();
        let Some(node) = node else { return levels };
        for (i, level) in levels.0.iter_mut().enumerate() {
            if let Some(p) = child(node, &format!("lvl{}pPr", i + 1)) {
                *level = LevelStyle::parse(p, colors, theme);
            }
        }
        // defPPr: the defaults of every level
        if let Some(p) = child(node, "defPPr") {
            let base = LevelStyle::parse(p, colors, theme);
            for level in &mut levels.0 {
                let mut merged = base.clone();
                merged.over(level);
                *level = merged;
            }
        }
        levels
    }

    fn over(&mut self, top: &Levels) {
        for (a, b) in self.0.iter_mut().zip(&top.0) {
            a.over(b);
        }
    }
}

// ---------------------------------------------------------------------------
// Masters and layouts: their placeholders, styles and backgrounds

#[derive(Clone, Copy, PartialEq, Debug)]
enum TextKind {
    Title,
    Body,
    Other,
}

/// A placeholder on a layout or master, for slides to inherit from
#[derive(Clone)]
struct PlaceholderDef {
    kind: String,
    idx: Option<String>,
    frame: Option<Frame>,
    /// spPr (fill, line) and bodyPr, as XML
    sp_pr: Option<String>,
    body_pr: Option<String>,
    levels: Levels,
}

struct Template {
    xml: String,
    color_map: HashMap<String, String>,
    placeholders: Vec<PlaceholderDef>,
    show_master_shapes: bool,
}

fn placeholder_of(sp: Node) -> Option<(String, Option<String>)> {
    let ph = sp.children().filter(Node::is_element).find_map(|nv| {
        if nv.tag_name().name().starts_with("nv") {
            path(nv, &["nvPr", "ph"])
        } else {
            None
        }
    })?;
    Some((ph.attribute("type").unwrap_or("obj").to_string(), ph.attribute("idx").map(str::to_string)))
}

fn text_kind(kind: &str) -> TextKind {
    match kind {
        "title" | "ctrTitle" => TextKind::Title,
        "dt" | "ftr" | "sldNum" | "hdr" => TextKind::Other,
        _ => TextKind::Body,
    }
}

fn frame_of(sp_pr: Option<Node>) -> Option<Frame> {
    let xfrm = sp_pr.and_then(|p| child(p, "xfrm"))?;
    xfrm_frame(xfrm)
}

fn xfrm_frame(xfrm: Node) -> Option<Frame> {
    let off = child(xfrm, "off")?;
    let ext = child(xfrm, "ext")?;
    Some(Frame {
        x: emu(off, "x").unwrap_or(0.0),
        y: emu(off, "y").unwrap_or(0.0),
        w: emu(ext, "cx").unwrap_or(0.0),
        h: emu(ext, "cy").unwrap_or(0.0),
        rotation: num(xfrm, "rot").unwrap_or(0.0) / 60_000.0,
        flip_h: flag(xfrm, "flipH").unwrap_or(false),
        flip_v: flag(xfrm, "flipV").unwrap_or(false),
    })
}

fn color_map(node: Option<Node>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(node) = node {
        for a in node.attributes() {
            map.insert(a.name().to_string(), a.value().to_string());
        }
    }
    map
}

impl Template {
    fn parse(xml: String, colors: &Colors, theme: &Theme, parent_map: Option<&HashMap<String, String>>) -> Template {
        let mut t = Template { xml: String::new(), color_map: HashMap::new(), placeholders: vec![], show_master_shapes: true };
        if let Ok(doc) = Document::parse(&xml) {
            let root = doc.root_element();
            t.show_master_shapes = flag(root, "showMasterSp").unwrap_or(true);
            t.color_map = match parent_map {
                // a layout keeps the master's map unless it overrides it
                Some(parent) => match path(root, &["clrMapOvr", "overrideClrMapping"]) {
                    Some(o) => color_map(Some(o)),
                    None => parent.clone(),
                },
                None => color_map(child(root, "clrMap")),
            };
            let colors = Colors { theme, map: t.color_map.clone(), placeholder: colors.placeholder };
            if let Some(tree) = path(root, &["cSld", "spTree"]) {
                for sp in tree.descendants().filter(|n| n.is_element() && n.tag_name().name() == "sp") {
                    let Some((kind, idx)) = placeholder_of(sp) else { continue };
                    let sp_pr = child(sp, "spPr");
                    let body = child(sp, "txBody");
                    t.placeholders.push(PlaceholderDef {
                        kind,
                        idx,
                        frame: frame_of(sp_pr),
                        sp_pr: sp_pr.map(|n| xml[n.range()].to_string()),
                        body_pr: body.and_then(|b| child(b, "bodyPr")).map(|n| xml[n.range()].to_string()),
                        levels: Levels::parse(body.and_then(|b| child(b, "lstStyle")), &colors, theme),
                    });
                }
            }
        }
        t.xml = xml;
        t
    }

    /// The placeholder a slide's (kind, idx) inherits from here
    fn find(&self, kind: &str, idx: Option<&str>) -> Option<&PlaceholderDef> {
        let same_kind = |a: &str, b: &str| a == b || (text_kind(a) == TextKind::Title && text_kind(b) == TextKind::Title);
        idx.and_then(|i| self.placeholders.iter().find(|p| p.idx.as_deref() == Some(i)))
            .or_else(|| self.placeholders.iter().find(|p| same_kind(&p.kind, kind)))
            .or_else(|| match text_kind(kind) {
                TextKind::Body if kind != "body" => self.placeholders.iter().find(|p| p.kind == "body"),
                _ => None,
            })
    }
}

// ---------------------------------------------------------------------------
// Reading

pub fn read(bytes: &[u8]) -> Result<Presentation, String> {
    let package = Package::open(bytes)?;
    let presentation_part = package
        .rels("")
        .into_values()
        .find(|(kind, _)| kind == "officeDocument")
        .map(|(_, target)| target)
        .unwrap_or_else(|| "ppt/presentation.xml".into());
    let presentation_xml = package.text(&presentation_part).ok_or("the presentation part is missing")?;
    let doc = Document::parse(&presentation_xml).map_err(|_| "the presentation could not be read")?;
    let root = doc.root_element();
    let size = child(root, "sldSz");
    let width = size.and_then(|s| emu(s, "cx")).filter(|w| *w > 0.0).unwrap_or(720.0);
    let height = size.and_then(|s| emu(s, "cy")).filter(|h| *h > 0.0).unwrap_or(540.0);
    let first_number = num(root, "firstSlideNum").unwrap_or(1.0) as usize;
    let rels = package.rels(&presentation_part);
    let slide_parts: Vec<String> = child(root, "sldIdLst")
        .map(|list| {
            children(list, "sldId")
                .filter_map(|s| s.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id")))
                .filter_map(|id| rels.get(id).map(|(_, target)| target.clone()))
                .collect()
        })
        .unwrap_or_default();
    if slide_parts.is_empty() {
        return Err("the presentation has no slides".into());
    }
    let default_style = child(root, "defaultTextStyle").map(|n| presentation_xml[n.range()].to_string());
    let table_styles = package.text("ppt/tableStyles.xml").unwrap_or_default();

    let mut reader = Reader {
        package: &package,
        themes: HashMap::new(),
        templates: HashMap::new(),
        default_style,
        table_styles,
        media: HashMap::new(),
    };
    let mut slides = vec![];
    for (i, part) in slide_parts.iter().enumerate() {
        if let Some(slide) = reader.slide(part, first_number + i) {
            slides.push(slide);
        }
    }
    // Every slide damaged (or nested too deep to read): an empty deck would only hide that
    if slides.is_empty() {
        return Err("none of the slides could be read".into());
    }
    let media = reader.media;
    let fonts = embedded_fonts(&package, root, &rels);
    Ok(Presentation { width, height, slides, media, fonts })
}

/// The fonts listed in the presentation's `embeddedFontLst`, each style that could be read
fn embedded_fonts(package: &Package, root: Node, rels: &HashMap<String, (String, String)>) -> Vec<(String, Vec<u8>)> {
    let Some(list) = child(root, "embeddedFontLst") else { return vec![] };
    let mut out = vec![];
    for font in children(list, "embeddedFont") {
        let Some(name) = child(font, "font").and_then(|f| f.attribute("typeface")).filter(|n| !n.trim().is_empty()) else { continue };
        for style in ["regular", "bold", "italic", "boldItalic"] {
            let Some(id) = child(font, style).and_then(|s| s.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id"))) else { continue };
            let Some((_, target)) = rels.get(id) else { continue };
            if let Some(data) = package.get(target).and_then(|part| font_data(&part)) {
                out.push((name.trim().to_string(), data));
            }
        }
    }
    out
}

/// The TrueType/OpenType font in an embedded font part: an Embedded OpenType wrapper
/// (.fntdata), XOR-obscured or not, or a plain font file. Fonts compressed with
/// MicroType Express are left out, and the slides use look-alikes for them.
fn font_data(part: &[u8]) -> Option<Vec<u8>> {
    let is_font = |d: &[u8]| d.len() > 12 && matches!(&d[..4], [0, 1, 0, 0] | b"OTTO" | b"true");
    if is_font(part) {
        return Some(part.to_vec());
    }
    let u32_at = |i: usize| part.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (size, data_size, version, flags) = (u32_at(0)? as usize, u32_at(4)? as usize, u32_at(8)?, u32_at(12)?);
    const COMPRESSED: u32 = 0x4;
    const XOR: u32 = 0x1000_0000;
    let magic = part.get(34..36) == Some(&[0x4C, 0x50]);
    if !magic || !matches!(version, 0x0001_0000 | 0x0002_0001 | 0x0002_0002) || flags & COMPRESSED != 0 || data_size == 0 || data_size > size || size > part.len() {
        return None;
    }
    // The font data is the last field of the header
    let mut data = part[size - data_size..size].to_vec();
    if flags & XOR != 0 {
        data.iter_mut().for_each(|b| *b ^= 0x50);
    }
    is_font(&data).then_some(data)
}

struct Reader<'p> {
    package: &'p Package,
    themes: HashMap<String, Theme>,
    templates: HashMap<String, Template>,
    default_style: Option<String>,
    table_styles: String,
    media: HashMap<String, Vec<u8>>,
}

/// What a slide's shapes need to be resolved
struct Context<'a> {
    theme: &'a Theme,
    colors: Colors<'a>,
    master: &'a Template,
    layout: &'a Template,
    /// The master's title, body and other placeholder styles, and the presentation's
    /// default text style (text boxes, shapes and tables)
    base: [Levels; 4],
    part: String,
    rels: HashMap<String, (String, String)>,
    number: usize,
}

impl<'p> Reader<'p> {
    fn theme(&mut self, part: &str) -> &Theme {
        if !self.themes.contains_key(part) {
            let theme = Theme::parse(&self.package.text(part).unwrap_or_default());
            self.themes.insert(part.to_string(), theme);
        }
        &self.themes[part]
    }

    fn slide(&mut self, part: &str, number: usize) -> Option<Slide> {
        let xml = self.package.text(part)?;
        let doc = Document::parse(&xml).ok()?;
        let root = doc.root_element();
        let rels = self.package.rels(part);
        let target = |kind: &str| rels.values().find(|(k, _)| k == kind).map(|(_, t)| t.clone());
        let layout_part = target("slideLayout").unwrap_or_default();
        let layout_rels = self.package.rels(&layout_part);
        let master_part = layout_rels.values().find(|(k, _)| k == "slideMaster").map(|(_, t)| t.clone()).unwrap_or_default();
        let master_rels = self.package.rels(&master_part);
        let theme_part = master_rels.values().find(|(k, _)| k == "theme").map(|(_, t)| t.clone()).unwrap_or_default();
        self.theme(&theme_part);
        let theme = &self.themes[&theme_part];

        let blank = Colors { theme, map: HashMap::new(), placeholder: None };
        if !self.templates.contains_key(&master_part) {
            let master = Template::parse(self.package.text(&master_part).unwrap_or_default(), &blank, theme, None);
            self.templates.insert(master_part.clone(), master);
        }
        if !self.templates.contains_key(&layout_part) {
            let master_map = self.templates[&master_part].color_map.clone();
            let layout = Template::parse(self.package.text(&layout_part).unwrap_or_default(), &blank, theme, Some(&master_map));
            self.templates.insert(layout_part.clone(), layout);
        }
        let master = &self.templates[&master_part];
        let layout = &self.templates[&layout_part];

        let map = match path(root, &["clrMapOvr", "overrideClrMapping"]) {
            Some(o) => color_map(Some(o)),
            None => layout.color_map.clone(),
        };
        let colors = Colors { theme, map, placeholder: None };
        let base = {
            let master_doc = Document::parse(&master.xml).ok();
            let styles = master_doc.as_ref().and_then(|d| path(d.root_element(), &["txStyles"]));
            let default = self.default_style.as_deref().and_then(|x| Document::parse(x).ok());
            let plain = Levels::parse(default.as_ref().map(|d| d.root_element()), &colors, theme);
            let parse = |name: &str| Levels::parse(styles.and_then(|s| child(s, name)), &colors, theme);
            let (mut title, mut body, mut other) = (plain.clone(), plain.clone(), plain.clone());
            title.over(&parse("titleStyle"));
            body.over(&parse("bodyStyle"));
            other.over(&parse("otherStyle"));
            [title, body, other, plain]
        };
        let cx = Context { theme, colors: colors.clone(), master, layout, base, part: part.to_string(), rels: rels.clone(), number };

        // Background: the slide's, else the layout's, else the master's
        let background = [(&xml, &rels, &colors)]
            .into_iter()
            .find_map(|(x, r, c)| background(x, r, c, theme, &mut self.media, self.package))
            .or_else(|| {
                let c = Colors { theme, map: layout.color_map.clone(), placeholder: None };
                background(&layout.xml, &layout_rels, &c, theme, &mut self.media, self.package)
            })
            .or_else(|| {
                let c = Colors { theme, map: master.color_map.clone(), placeholder: None };
                background(&master.xml, &master_rels, &c, theme, &mut self.media, self.package)
            });

        let mut elements = vec![];
        let mut sink = Sink { elements: &mut elements, media: &mut self.media, package: self.package, table_styles: &self.table_styles, depth: 0 };
        let show_master = flag(root, "showMasterSp").unwrap_or(true) && layout.show_master_shapes;
        if show_master {
            let master_cx = Context { rels: master_rels.clone(), part: master_part.clone(), colors: Colors { theme, map: master.color_map.clone(), placeholder: None }, ..cx.clone_shallow() };
            if let Ok(d) = Document::parse(&master.xml)
                && let Some(tree) = path(d.root_element(), &["cSld", "spTree"])
            {
                shapes(tree, &master_cx, &mut sink, &Transform::default(), false);
            }
        }
        if flag(root, "showMasterSp").unwrap_or(true) {
            let layout_cx = Context { rels: layout_rels.clone(), part: layout_part.clone(), colors: Colors { theme, map: layout.color_map.clone(), placeholder: None }, ..cx.clone_shallow() };
            if let Ok(d) = Document::parse(&layout.xml)
                && let Some(tree) = path(d.root_element(), &["cSld", "spTree"])
            {
                shapes(tree, &layout_cx, &mut sink, &Transform::default(), false);
            }
        }
        let mut title = String::new();
        if let Some(tree) = path(root, &["cSld", "spTree"]) {
            shapes(tree, &cx, &mut sink, &Transform::default(), true);
            title = slide_title(tree);
        }
        let notes = target("notesSlide").and_then(|n| self.package.text(&n)).map(|x| notes_text(&x)).unwrap_or_default();
        Some(Slide { hidden: root.attribute("show") == Some("0"), background, elements, title, notes })
    }
}

impl<'a> Context<'a> {
    fn clone_shallow(&self) -> Context<'a> {
        Context {
            theme: self.theme,
            colors: self.colors.clone(),
            master: self.master,
            layout: self.layout,
            base: self.base.clone(),
            part: self.part.clone(),
            rels: self.rels.clone(),
            number: self.number,
        }
    }
}

fn background(
    xml: &str,
    rels: &HashMap<String, (String, String)>,
    colors: &Colors,
    theme: &Theme,
    media: &mut HashMap<String, Vec<u8>>,
    package: &Package,
) -> Option<Fill> {
    let doc = Document::parse(xml).ok()?;
    let bg = path(doc.root_element(), &["cSld", "bg"])?;
    if let Some(pr) = child(bg, "bgPr") {
        return fill_of(pr, colors, rels, media, package);
    }
    let reference = child(bg, "bgRef")?;
    let idx = num(reference, "idx").unwrap_or(0.0) as usize;
    let color = colors.first(reference);
    let style = if idx >= 1001 { theme.background_styles.get(idx - 1001) } else { theme.fill_styles.get(idx.checked_sub(1)?) }?;
    let wrapped = format!("<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{style}</w>");
    let style_doc = Document::parse(&wrapped).ok()?;
    fill_of(style_doc.root_element(), &colors.with_placeholder(color), rels, media, package)
}

/// The fill inside an element holding one (spPr, bgPr, tcPr, …)
fn fill_of(
    node: Node,
    colors: &Colors,
    rels: &HashMap<String, (String, String)>,
    media: &mut HashMap<String, Vec<u8>>,
    package: &Package,
) -> Option<Fill> {
    for f in node.children().filter(Node::is_element) {
        match f.tag_name().name() {
            "noFill" => return None,
            "solidFill" => return colors.first(f).map(Fill::Solid),
            "gradFill" => {
                let mut stops: Vec<(f32, Rgba)> = path(f, &["gsLst"])
                    .map(|l| children(l, "gs").filter_map(|gs| Some((num(gs, "pos")? / 100_000.0, colors.first(gs)?))).collect())
                    .unwrap_or_default();
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                let angle = child(f, "lin").and_then(|l| num(l, "ang")).map(|a| a / 60_000.0).unwrap_or(90.0);
                return match stops.len() {
                    0 => None,
                    1 => Some(Fill::Solid(stops[0].1)),
                    _ => Some(Fill::Gradient(stops, angle)),
                };
            }
            "blipFill" => {
                let id = path(f, &["blip"]).and_then(|b| b.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "embed")))?;
                let target = rels.get(id)?.1.clone();
                if !media.contains_key(&target) {
                    let data = package.get(&target)?;
                    media.insert(target.clone(), data);
                }
                return Some(Fill::Picture(target));
            }
            "pattFill" => return child(f, "fgClr").and_then(|c| colors.first(c)).map(Fill::Solid),
            "grpFill" => return None,
            _ => {}
        }
    }
    None
}

fn line_of(node: Node, colors: &Colors) -> Option<Option<Line>> {
    let ln = child(node, "ln")?;
    if child(ln, "noFill").is_some() {
        return Some(None);
    }
    let color = child(ln, "solidFill").and_then(|f| colors.first(f)).or_else(|| child(ln, "gradFill").and_then(|g| path(g, &["gsLst", "gs"])).and_then(|gs| colors.first(gs)));
    let width = emu(ln, "w");
    let dash = child(ln, "prstDash").and_then(|d| d.attribute("val")).and_then(|v| match v {
        "solid" => None,
        "dot" | "sysDot" => Some("dotted"),
        "dash" | "sysDash" | "lgDash" => Some("dashed"),
        "dashDot" | "sysDashDot" | "lgDashDot" | "lgDashDotDot" | "sysDashDotDot" => Some("dash-dotted"),
        _ => None,
    });
    let end = |name: &str| {
        let e = child(ln, name)?;
        let kind = LineEnd::parse(e.attribute("type")?)?;
        Some((kind, super::shapes::end_size(e.attribute("w")), super::shapes::end_size(e.attribute("len"))))
    };
    let (head, tail) = (end("headEnd"), end("tailEnd"));
    match (color, width) {
        (None, None) if head.is_none() && tail.is_none() => None,
        (color, width) => Some(Some(Line { color: color.unwrap_or(Rgba { rgb: [0, 0, 0], alpha: 1.0 }), width: width.unwrap_or(0.75), dash, head, tail })),
    }
}

// ---------------------------------------------------------------------------
// Shapes

/// Maps a group's child coordinates onto the slide: one level per enclosing group,
/// innermost first
#[derive(Clone, Default)]
struct Transform(Vec<GroupLevel>);

/// A group: its children's coordinates scaled and moved into its frame, which is
/// then flipped and rotated about its centre
#[derive(Clone, Copy)]
struct GroupLevel {
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
    center: (f32, f32),
    rotation: f32,
    flip_h: bool,
    flip_v: bool,
}

impl Transform {
    fn apply(&self, mut f: Frame) -> Frame {
        for g in &self.0 {
            f = Frame { x: f.x * g.sx + g.tx, y: f.y * g.sy + g.ty, w: f.w * g.sx, h: f.h * g.sy, ..f };
            let (mut mx, mut my) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
            if g.flip_h {
                mx = 2.0 * g.center.0 - mx;
                f.rotation = -f.rotation;
                f.flip_h = !f.flip_h;
            }
            if g.flip_v {
                my = 2.0 * g.center.1 - my;
                f.rotation = -f.rotation;
                f.flip_v = !f.flip_v;
            }
            if g.rotation != 0.0 {
                let (s, c) = g.rotation.to_radians().sin_cos();
                let (dx, dy) = (mx - g.center.0, my - g.center.1);
                mx = g.center.0 + dx * c - dy * s;
                my = g.center.1 + dx * s + dy * c;
                f.rotation += g.rotation;
            }
            f.x = mx - f.w / 2.0;
            f.y = my - f.h / 2.0;
        }
        f
    }

    /// Inside a group (or other container) placed at `level` within this transform
    fn inside(&self, level: GroupLevel) -> Transform {
        let mut levels = vec![level];
        levels.extend(self.0.iter().copied());
        Transform(levels)
    }
}

impl GroupLevel {
    fn offset(x: f32, y: f32) -> GroupLevel {
        GroupLevel { sx: 1.0, sy: 1.0, tx: x, ty: y, center: (0.0, 0.0), rotation: 0.0, flip_h: false, flip_v: false }
    }
}

struct Sink<'s> {
    elements: &'s mut Vec<Element>,
    media: &'s mut HashMap<String, Vec<u8>>,
    package: &'s Package,
    table_styles: &'s str,
    /// Groups (and alternate contents) entered: nesting is limited, as a file nested
    /// thousands deep would otherwise overflow the stack
    depth: usize,
}

/// Deeper than PowerPoint lets anyone nest groups by hand
const MAX_DEPTH: usize = 48;

/// Collects the drawable elements of a shape tree, in drawing order. `slide`: whether
/// placeholders are drawn (on masters and layouts they are only templates).
fn shapes(tree: Node, cx: &Context, sink: &mut Sink, t: &Transform, slide: bool) {
    if sink.depth >= MAX_DEPTH {
        return;
    }
    sink.depth += 1;
    shapes_in(tree, cx, sink, t, slide);
    sink.depth -= 1;
}

fn shapes_in(tree: Node, cx: &Context, sink: &mut Sink, t: &Transform, slide: bool) {
    for node in tree.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "sp" | "cxnSp" => {
                let ph = placeholder_of(node);
                if ph.is_some() && !slide {
                    continue;
                }
                if let Some(e) = shape(node, cx, sink, t, ph) {
                    sink.elements.push(e);
                }
            }
            "pic" => {
                if placeholder_of(node).is_some() && !slide {
                    continue;
                }
                if let Some(e) = picture(node, cx, sink, t) {
                    sink.elements.push(e);
                }
            }
            "grpSp" => {
                let xfrm = path(node, &["grpSpPr", "xfrm"]);
                let inner = xfrm
                    .and_then(|x| {
                        let f = xfrm_frame(x)?;
                        let ch_off = child(x, "chOff");
                        let ch_ext = child(x, "chExt");
                        let (cx0, cy0) = (ch_off.and_then(|o| emu(o, "x")).unwrap_or(f.x), ch_off.and_then(|o| emu(o, "y")).unwrap_or(f.y));
                        let (cw, chh) = (ch_ext.and_then(|e| emu(e, "cx")).unwrap_or(f.w), ch_ext.and_then(|e| emu(e, "cy")).unwrap_or(f.h));
                        let sx = if cw > 0.0 { f.w / cw } else { 1.0 };
                        let sy = if chh > 0.0 { f.h / chh } else { 1.0 };
                        Some(t.inside(GroupLevel {
                            sx,
                            sy,
                            tx: f.x - cx0 * sx,
                            ty: f.y - cy0 * sy,
                            center: (f.x + f.w / 2.0, f.y + f.h / 2.0),
                            rotation: f.rotation,
                            flip_h: f.flip_h,
                            flip_v: f.flip_v,
                        }))
                    })
                    .unwrap_or_else(|| t.clone());
                shapes(node, cx, sink, &inner, slide);
            }
            "graphicFrame" => graphic_frame(node, cx, sink, t),
            // Newer content with an older equivalent for other readers
            "AlternateContent" => {
                if let Some(fallback) = child(node, "Fallback") {
                    shapes(fallback, cx, sink, t, slide);
                } else if let Some(choice) = child(node, "Choice") {
                    shapes(choice, cx, sink, t, slide);
                }
            }
            _ => {}
        }
    }
}

fn shape(node: Node, cx: &Context, sink: &mut Sink, t: &Transform, ph: Option<(String, Option<String>)>) -> Option<Element> {
    let sp_pr = child(node, "spPr");
    // Inherited placeholder definitions: the layout's, then the master's
    let (layout_def, master_def) = match &ph {
        Some((kind, idx)) => {
            let l = cx.layout.find(kind, idx.as_deref());
            let m = cx.master.find(l.map(|d| d.kind.as_str()).unwrap_or(kind), None);
            (l, m)
        }
        None => (None, None),
    };
    let frame = frame_of(sp_pr).or_else(|| layout_def.and_then(|d| d.frame)).or_else(|| master_def.and_then(|d| d.frame))?;
    let frame = t.apply(frame);

    // Style references (p:style) give default fill, line and text colour
    let style = child(node, "style");
    let style_ref = |name: &str| style.and_then(|s| child(s, name));
    let style_fill = style_ref("fillRef").and_then(|r| {
        let idx = num(r, "idx").unwrap_or(0.0) as usize;
        let list = if idx >= 1001 { &cx.theme.background_styles } else { &cx.theme.fill_styles };
        let xml = list.get(if idx >= 1001 { idx - 1001 } else { idx.checked_sub(1)? })?;
        let wrapped = format!("<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{xml}</w>");
        let doc = Document::parse(&wrapped).ok()?;
        fill_of(doc.root_element(), &cx.colors.with_placeholder(cx.colors.first(r)), &cx.rels, sink.media, sink.package)
    });
    let style_line = style_ref("lnRef").and_then(|r| {
        let idx = num(r, "idx").unwrap_or(0.0) as usize;
        let xml = cx.theme.line_styles.get(idx.checked_sub(1)?)?;
        let wrapped = format!("<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{xml}</w>");
        let doc = Document::parse(&wrapped).ok()?;
        line_of(doc.root_element(), &cx.colors.with_placeholder(cx.colors.first(r))).flatten()
    });
    let font_ref = style_ref("fontRef");

    // Fill and line: the shape's own, else its placeholder's, else the style's
    let inherited_sp_pr = |def: Option<&PlaceholderDef>| def.and_then(|d| d.sp_pr.clone());
    let has_fill = |n: Node| n.children().any(|c| matches!(c.tag_name().name(), "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill"));
    let mut fill = None;
    let mut fill_found = false;
    for xml in [sp_pr.map(|n| n.document().input_text()[n.range()].to_string()), inherited_sp_pr(layout_def), inherited_sp_pr(master_def)].into_iter().flatten() {
        let wrapped = wrap(&xml);
        let Ok(doc) = Document::parse(&wrapped) else { continue };
        let pr = doc.root_element().first_element_child().unwrap_or(doc.root_element());
        if has_fill(pr) {
            fill = fill_of(pr, &cx.colors, &cx.rels, sink.media, sink.package);
            fill_found = true;
            break;
        }
    }
    if !fill_found {
        fill = style_fill;
    }
    let mut line = None;
    let mut line_found = false;
    for xml in [sp_pr.map(|n| n.document().input_text()[n.range()].to_string()), inherited_sp_pr(layout_def), inherited_sp_pr(master_def)].into_iter().flatten() {
        let wrapped = wrap(&xml);
        let Ok(doc) = Document::parse(&wrapped) else { continue };
        let pr = doc.root_element().first_element_child().unwrap_or(doc.root_element());
        if let Some(l) = line_of(pr, &cx.colors) {
            line = l;
            line_found = true;
            break;
        }
    }
    if !line_found {
        line = style_line;
    }
    let mut geometry = sp_pr.map(|p| geometry_of(p, frame.w, frame.h)).unwrap_or(Geometry::Rect);
    // A connector or line has no inside
    let fill = if geometry.is_open() { None } else { fill };
    if let (Some(l), Geometry::Paths(subs)) = (&line, &mut geometry)
        && (l.head.is_some() || l.tail.is_some())
    {
        let heads = super::shapes::arrowheads(&Geometry::Paths(subs.clone()), l.width, l.head, l.tail);
        subs.extend(heads);
    }

    let text = child(node, "txBody").and_then(|body| {
        let mut levels = cx.base[match ph.as_ref().map(|(k, _)| text_kind(k)) {
            Some(TextKind::Title) => 0,
            Some(TextKind::Body) => 1,
            Some(TextKind::Other) => 2,
            None => 3,
        }]
        .clone();
        if let Some(d) = master_def {
            levels.over(&d.levels);
        }
        if let Some(d) = layout_def {
            levels.over(&d.levels);
        }
        // The style's font reference: the shape's default text colour and font, under
        // what the shape's own list style says
        if let Some(r) = font_ref {
            let color = cx.colors.first(r);
            let font = match r.attribute("idx") {
                Some("major") => Some(cx.theme.major_font.clone()),
                Some("minor") => Some(cx.theme.minor_font.clone()),
                _ => None,
            };
            for level in &mut levels.0 {
                level.run.color = color.or(level.run.color);
                level.run.font = font.clone().or(level.run.font.take());
            }
        }
        levels.over(&Levels::parse(child(body, "lstStyle"), &cx.colors, cx.theme));
        let body_prs: Vec<String> = [
            child(body, "bodyPr").map(|n| n.document().input_text()[n.range()].to_string()),
            layout_def.and_then(|d| d.body_pr.clone()),
            master_def.and_then(|d| d.body_pr.clone()),
        ]
        .into_iter()
        .flatten()
        .collect();
        text_body(body, &body_prs, &levels, cx, matches!(ph.as_ref().map(|p| p.0.as_str()), Some("sldNum")))
    });
    let text = text.map(|mut text| {
        let area = sp_pr.and_then(|p| child(p, "prstGeom")).and_then(|g| super::shapes::text_rect(g.attribute("prst")?, &adjust(g), frame.w, frame.h));
        if let Some(area) = area {
            for (inset, extra) in text.insets.iter_mut().zip(area) {
                *inset += extra;
            }
        }
        text
    });
    if fill.is_none() && line.is_none() && text.as_ref().is_none_or(|t| t.paragraphs.iter().all(|p| p.runs.iter().all(|r| r.text.trim().is_empty()))) {
        return None;
    }
    Some(Element::Shape { frame, geometry, fill, line, text, placeholder: ph.map(|(kind, _)| kind) })
}

fn wrap(xml: &str) -> String {
    format!(
        "<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">{xml}</w>"
    )
}

/// A preset's adjust values (avLst), as stored
fn adjust<'a>(preset: Node<'a, 'a>) -> impl Fn(&str, f32) -> f32 + 'a {
    move |name: &str, default: f32| {
        path(preset, &["avLst"])
            .and_then(|l| children(l, "gd").find(|g| g.attribute("name") == Some(name)))
            .and_then(|g| g.attribute("fmla"))
            .and_then(|f| f.strip_prefix("val ")?.trim().parse::<f32>().ok())
            .unwrap_or(default)
    }
}

fn geometry_of(sp_pr: Node, w: f32, h: f32) -> Geometry {
    if let Some(g) = child(sp_pr, "custGeom").and_then(|c| custom_geometry(c, w, h)) {
        return g;
    }
    let Some(preset) = child(sp_pr, "prstGeom") else { return Geometry::Rect };
    super::shapes::preset(preset.attribute("prst").unwrap_or("rect"), &adjust(preset), w, h).unwrap_or(Geometry::Rect)
}

/// Custom geometry (freeforms, icons). Coordinates may name guides: the shape's
/// built-in sizes (w, h, wd2, …) and the formulas in its gdLst/avLst.
fn custom_geometry(custom: Node, w: f32, h: f32) -> Option<Geometry> {
    use super::shapes::CustomStep;
    let mut guides = Guides::new(w * EMU, h * EMU);
    for list in ["avLst", "gdLst"] {
        if let Some(l) = child(custom, list) {
            for gd in children(l, "gd") {
                if let (Some(name), Some(fmla)) = (gd.attribute("name"), gd.attribute("fmla")) {
                    let v = guides.formula(fmla);
                    guides.values.insert(name.to_string(), v);
                }
            }
        }
    }
    let mut list = vec![];
    for p in children(path(custom, &["pathLst"])?, "path") {
        // Without its own size a path is in the shape's units (EMU)
        let pw = num(p, "w").filter(|v| *v > 0.0).unwrap_or(w * EMU);
        let ph = num(p, "h").filter(|v| *v > 0.0).unwrap_or(h * EMU);
        let fill = p.attribute("fill") != Some("none");
        let stroke = !matches!(p.attribute("stroke"), Some("0" | "false"));
        let pt = |n: Node| (guides.get(n.attribute("x").unwrap_or("0")), guides.get(n.attribute("y").unwrap_or("0")));
        let mut steps = vec![];
        for c in p.children().filter(Node::is_element) {
            let pts: Vec<(f32, f32)> = children(c, "pt").map(pt).collect();
            match (c.tag_name().name(), pts.as_slice()) {
                ("moveTo", [a, ..]) => steps.push(CustomStep::Move(a.0, a.1)),
                ("lnTo", [a, ..]) => steps.push(CustomStep::Line(a.0, a.1)),
                ("cubicBezTo", [a, b, e, ..]) => steps.push(CustomStep::Cubic([a.0, a.1, b.0, b.1, e.0, e.1])),
                ("quadBezTo", [q, e, ..]) => steps.push(CustomStep::Quad(q.0, q.1, e.0, e.1)),
                ("arcTo", _) => {
                    let g = |n: &str| guides.get(c.attribute(n).unwrap_or("0"));
                    steps.push(CustomStep::Arc(g("wR"), g("hR"), g("stAng") / 60_000.0, g("swAng") / 60_000.0));
                }
                ("close", _) => steps.push(CustomStep::Close),
                _ => {}
            }
        }
        list.push((pw, ph, fill, stroke, steps));
    }
    super::shapes::custom(&list, w, h)
}

/// Shape guide values for custom geometry formulas
struct Guides {
    values: HashMap<String, f32>,
}

impl Guides {
    fn new(w: f32, h: f32) -> Guides {
        let ss = w.min(h);
        let mut values = HashMap::new();
        for (k, v) in [
            ("w", w), ("h", h), ("l", 0.0), ("t", 0.0), ("r", w), ("b", h), ("hc", w / 2.0), ("vc", h / 2.0),
            ("ss", ss), ("ls", w.max(h)), ("cd2", 10_800_000.0), ("cd4", 5_400_000.0), ("cd8", 2_700_000.0),
            ("3cd4", 16_200_000.0), ("3cd8", 8_100_000.0), ("5cd8", 13_500_000.0), ("7cd8", 18_900_000.0),
        ] {
            values.insert(k.to_string(), v);
        }
        for d in [2.0f32, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 32.0] {
            values.insert(format!("wd{d}"), w / d);
            values.insert(format!("hd{d}"), h / d);
            values.insert(format!("ssd{d}"), ss / d);
        }
        Guides { values }
    }

    fn get(&self, token: &str) -> f32 {
        token.parse().unwrap_or_else(|_| self.values.get(token).copied().unwrap_or(0.0))
    }

    fn formula(&self, fmla: &str) -> f32 {
        let mut parts = fmla.split_whitespace();
        let op = parts.next().unwrap_or("");
        let args: Vec<f32> = parts.map(|t| self.get(t)).collect();
        let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
        let rad = |v: f32| (v / 60_000.0).to_radians();
        match op {
            "val" => a(0),
            "*/" => if a(2) == 0.0 { 0.0 } else { a(0) * a(1) / a(2) },
            "+-" => a(0) + a(1) - a(2),
            "+/" => if a(2) == 0.0 { 0.0 } else { (a(0) + a(1)) / a(2) },
            "?:" => if a(0) > 0.0 { a(1) } else { a(2) },
            "abs" => a(0).abs(),
            "sqrt" => a(0).max(0.0).sqrt(),
            "max" => a(0).max(a(1)),
            "min" => a(0).min(a(1)),
            "pin" => a(1).clamp(a(0).min(a(2)), a(2).max(a(0))),
            "mod" => (a(0) * a(0) + a(1) * a(1) + a(2) * a(2)).sqrt(),
            "sin" => a(0) * rad(a(1)).sin(),
            "cos" => a(0) * rad(a(1)).cos(),
            "tan" => a(0) * rad(a(1)).tan(),
            "at2" => a(1).atan2(a(0)).to_degrees() * 60_000.0,
            "cat2" => a(0) * a(2).atan2(a(1)).cos(),
            "sat2" => a(0) * a(2).atan2(a(1)).sin(),
            _ => 0.0,
        }
    }
}

fn picture(node: Node, cx: &Context, sink: &mut Sink, t: &Transform) -> Option<Element> {
    let blip_fill = child(node, "blipFill")?;
    let id = child(blip_fill, "blip")?.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "embed"))?;
    let target = cx.rels.get(id)?.1.clone();
    if !sink.media.contains_key(&target) {
        let data = sink.package.get(&target)?;
        sink.media.insert(target.clone(), data);
    }
    let sp_pr = child(node, "spPr");
    let ph = placeholder_of(node);
    let frame = frame_of(sp_pr).or_else(|| {
        let (kind, idx) = ph.as_ref()?;
        cx.layout.find(kind, idx.as_deref()).and_then(|d| d.frame).or_else(|| cx.master.find(kind, None).and_then(|d| d.frame))
    })?;
    let crop = child(blip_fill, "srcRect")
        .map(|r| ["l", "t", "r", "b"].map(|k| num(r, k).unwrap_or(0.0) / 100_000.0))
        .unwrap_or([0.0; 4]);
    let frame = t.apply(frame);
    let geometry = sp_pr.map(|p| geometry_of(p, frame.w, frame.h)).unwrap_or(Geometry::Rect);
    let line = sp_pr.and_then(|p| line_of(p, &cx.colors)).flatten();
    Some(Element::Picture { frame, media: target, crop, geometry, line })
}

fn graphic_frame(node: Node, cx: &Context, sink: &mut Sink, t: &Transform) {
    let Some(local) = child(node, "xfrm").and_then(xfrm_frame) else { return };
    let frame = t.apply(local);
    let Some(data) = path(node, &["graphic", "graphicData"]) else { return };
    if let Some(table) = child(data, "tbl") {
        if let Some(e) = table_element(table, frame, cx, sink) {
            sink.elements.push(e);
        }
        return;
    }
    // SmartArt: PowerPoint saves a drawing of it next to the diagram data
    if data.attribute("uri").is_some_and(|u| u.ends_with("/diagram")) {
        let drawing = cx.rels.values().find(|(k, _)| k == "diagramDrawing").map(|(_, p)| p.clone());
        let Some(part) = drawing else { return };
        let Some(xml) = sink.package.text(&part) else { return };
        let Ok(doc) = Document::parse(&xml) else { return };
        if let Some(tree) = doc.descendants().find(|n| n.tag_name().name() == "spTree") {
            let rels = sink.package.rels(&part);
            let dcx = Context { rels, part: part.clone(), ..cx.clone_shallow() };
            shapes(tree, &dcx, sink, &t.inside(GroupLevel::offset(local.x, local.y)), true);
        }
    }
}

// ---------------------------------------------------------------------------
// Text

fn text_body(body: Node, body_prs: &[String], levels: &Levels, cx: &Context, slide_number: bool) -> Option<TextBody> {
    // bodyPr: the shape's own attributes win over its placeholder's
    let mut insets = [7.2, 3.6, 7.2, 3.6];
    let mut anchor = None;
    let mut wrap_text = None;
    let mut grows = false;
    let mut font_scale = None;
    let mut spacing_reduction = None;
    for xml in body_prs.iter().rev() {
        let wrapped = wrap(xml);
        let Ok(doc) = Document::parse(&wrapped) else { continue };
        let Some(pr) = doc.root_element().first_element_child() else { continue };
        for (i, name) in ["lIns", "tIns", "rIns", "bIns"].iter().enumerate() {
            if let Some(v) = emu(pr, name) {
                insets[i] = v;
            }
        }
        if let Some(a) = pr.attribute("anchor") {
            anchor = Some(match a {
                "ctr" => Anchor::Middle,
                "b" => Anchor::Bottom,
                _ => Anchor::Top,
            });
        }
        if let Some(w) = pr.attribute("wrap") {
            wrap_text = Some(w != "none");
        }
        if child(pr, "spAutoFit").is_some() {
            grows = true;
        } else if child(pr, "normAutofit").is_some() || child(pr, "noAutofit").is_some() {
            grows = false;
        }
        if let Some(fit) = child(pr, "normAutofit") {
            font_scale = num(fit, "fontScale").map(|v| v / 100_000.0).or(font_scale);
            spacing_reduction = num(fit, "lnSpcReduction").map(|v| v / 100_000.0).or(spacing_reduction);
        }
    }
    let scale = font_scale.unwrap_or(1.0);
    let mut paragraphs = vec![];
    let mut numbering: HashMap<usize, usize> = HashMap::new();
    for p in children(body, "p") {
        let ppr = child(p, "pPr");
        let level = ppr.and_then(|n| num(n, "lvl")).unwrap_or(0.0).clamp(0.0, 8.0) as usize;
        let mut style = levels.0[level].clone();
        if let Some(ppr) = ppr {
            style.over(&LevelStyle::parse(ppr, &cx.colors, cx.theme));
        }
        let mut runs = vec![];
        for r in p.children().filter(Node::is_element) {
            match r.tag_name().name() {
                "r" | "fld" => {
                    let mut rs = style.run.clone();
                    if let Some(rpr) = child(r, "rPr") {
                        rs.over(&RunStyle::parse(rpr, &cx.colors, cx.theme));
                    }
                    let mut text: String = child(r, "t").and_then(|t| t.text()).unwrap_or("").to_string();
                    if r.tag_name().name() == "fld" && r.attribute("type").is_some_and(|t| t.starts_with("slidenum")) {
                        text = cx.number.to_string();
                    }
                    runs.push(make_run(text, &rs, scale, false));
                }
                "br" => {
                    let mut rs = style.run.clone();
                    if let Some(rpr) = child(r, "rPr") {
                        rs.over(&RunStyle::parse(rpr, &cx.colors, cx.theme));
                    }
                    runs.push(make_run(String::new(), &rs, scale, true));
                }
                _ => {}
            }
        }
        let mut end = style.run.clone();
        if let Some(e) = child(p, "endParaRPr") {
            end.over(&RunStyle::parse(e, &cx.colors, cx.theme));
        }
        let size = end.size.unwrap_or(18.0) * scale;
        let has_text = runs.iter().any(|r| !r.text.is_empty());
        let bullet = match (&style.bullet, has_text && !slide_number) {
            (Some(BulletStyle::Char(c)), true) => Some(c.clone()),
            (Some(BulletStyle::Number(kind, start)), true) => {
                let n = numbering.entry(level).or_insert(*start);
                let text = number_text(kind, *n);
                *n += 1;
                Some(text)
            }
            _ => None,
        };
        // numbering restarts after a paragraph of a shallower level
        numbering.retain(|&l, _| l <= level);
        let first_size = runs.iter().find(|r| !r.line_break).map(|r| r.size).unwrap_or(size);
        let line_spacing = match style.line_spacing.unwrap_or(LineSpacing::Multiple(1.0)) {
            LineSpacing::Multiple(m) => LineSpacing::Multiple((m - spacing_reduction.unwrap_or(0.0)).max(0.5)),
            other => other,
        };
        let spacing_points = |s: Option<Spacing>| match s {
            Some(Spacing::Points(p)) => p,
            Some(Spacing::Percent(f)) => f * first_size * 1.2,
            None => 0.0,
        };
        paragraphs.push(Paragraph {
            align: style.align.unwrap_or(Align::Left),
            margin_left: style.margin_left.unwrap_or(0.0),
            indent: style.indent.unwrap_or(0.0),
            space_before: spacing_points(style.space_before),
            space_after: spacing_points(style.space_after),
            line_spacing,
            bullet: bullet.map(|text| Bullet {
                text,
                font: style.bullet_font.clone(),
                color: style.bullet_color,
                scale: style.bullet_scale.unwrap_or(1.0),
                points: style.bullet_points.map(|p| p * scale),
            }),
            runs,
            size,
        });
    }
    if paragraphs.is_empty() {
        return None;
    }
    Some(TextBody { insets, anchor: anchor.unwrap_or(Anchor::Top), wrap: wrap_text.unwrap_or(true), grows, paragraphs })
}

fn make_run(text: String, style: &RunStyle, scale: f32, line_break: bool) -> Run {
    let size = style.size.unwrap_or(18.0);
    Run {
        kern: style.kern.is_some_and(|k| k > 0.0 && size >= k),
        spacing: style.spacing.unwrap_or(0.0) * scale,
        highlight: style.highlight,
        caps: style.caps.unwrap_or_default(),
        text,
        font: style.font.clone().unwrap_or_else(|| "Calibri".into()),
        size: style.size.unwrap_or(18.0) * scale,
        bold: style.bold.unwrap_or(false),
        italic: style.italic.unwrap_or(false),
        underline: style.underline.unwrap_or(false),
        strike: style.strike.unwrap_or(false),
        color: style.color.unwrap_or(Rgba { rgb: [0, 0, 0], alpha: 1.0 }),
        baseline: style.baseline.unwrap_or(0.0),
        line_break,
    }
}

fn number_text(kind: &str, n: usize) -> String {
    let roman = |mut n: usize| {
        let table = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
        let mut s = String::new();
        for (v, r) in table {
            while n >= v {
                s.push_str(r);
                n -= v;
            }
        }
        s
    };
    let alpha = |n: usize| ((b'a' + ((n.max(1) - 1) % 26) as u8) as char).to_string();
    let body = if kind.starts_with("alphaLc") {
        alpha(n)
    } else if kind.starts_with("alphaUc") {
        alpha(n).to_uppercase()
    } else if kind.starts_with("romanLc") {
        roman(n)
    } else if kind.starts_with("romanUc") {
        roman(n).to_uppercase()
    } else {
        n.to_string()
    };
    if kind.ends_with("ParenBoth") {
        format!("({body})")
    } else if kind.ends_with("ParenR") {
        format!("{body})")
    } else if kind.ends_with("Plain") {
        body
    } else {
        format!("{body}.")
    }
}

fn slide_title(tree: Node) -> String {
    tree.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "sp")
        .find(|sp| placeholder_of(*sp).is_some_and(|(k, _)| text_kind(&k) == TextKind::Title))
        .map(|sp| paragraphs_text(sp))
        .unwrap_or_default()
}

fn paragraphs_text(node: Node) -> String {
    node.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(|p| p.descendants().filter(|t| t.is_element() && t.tag_name().name() == "t").filter_map(|t| t.text()).collect::<String>())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The speaker notes: the text of the notes page's body placeholder
fn notes_text(xml: &str) -> String {
    let Ok(doc) = Document::parse(xml) else { return String::new() };
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "sp")
        .filter(|sp| placeholder_of(*sp).is_some_and(|(k, _)| k == "body"))
        .map(paragraphs_text)
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tables

/// Parts of a table style that apply to a cell: fill, text colour and boldness, borders
#[derive(Default, Clone)]
struct CellLook {
    fill: Option<Fill>,
    color: Option<Rgba>,
    bold: Option<bool>,
    borders: [Option<Line>; 4],
    inside_h: Option<Line>,
    inside_v: Option<Line>,
}

impl CellLook {
    fn over(&mut self, top: &CellLook) {
        if top.fill.is_some() {
            self.fill = top.fill.clone();
        }
        if top.color.is_some() {
            self.color = top.color;
        }
        if top.bold.is_some() {
            self.bold = top.bold;
        }
        for i in 0..4 {
            if top.borders[i].is_some() {
                self.borders[i] = top.borders[i].clone();
            }
        }
        if top.inside_h.is_some() {
            self.inside_h = top.inside_h.clone();
        }
        if top.inside_v.is_some() {
            self.inside_v = top.inside_v.clone();
        }
    }
}

/// "Medium Style 2 - Accent 1", PowerPoint's default table style, which files name
/// but don't always define
const DEFAULT_TABLE_STYLE: &str = r#"<a:tblStyle xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" styleId="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"><a:wholeTbl><a:tcTxStyle><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="dk1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:left><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:left><a:right><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:right><a:top><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top><a:bottom><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom><a:insideH><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideH><a:insideV><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideV></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="20000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:wholeTbl><a:band1H><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1H><a:band1V><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1V><a:lastCol><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastCol><a:firstCol><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstCol><a:lastRow><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:top><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastRow><a:firstRow><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:bottom><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstRow></a:tblStyle>"#;

fn table_style_part(style: Node, name: &str, colors: &Colors) -> CellLook {
    let mut look = CellLook::default();
    let Some(part) = child(style, name) else { return look };
    if let Some(tx) = child(part, "tcTxStyle") {
        look.bold = flag(tx, "b");
        look.color = tx.children().filter(Node::is_element).filter(|c| c.tag_name().name() != "fontRef").find_map(|c| colors.color(c));
    }
    if let Some(tc) = child(part, "tcStyle") {
        if let Some(f) = child(tc, "fill") {
            look.fill = f.children().filter(Node::is_element).find_map(|c| match c.tag_name().name() {
                "solidFill" => colors.first(c).map(Fill::Solid),
                _ => None,
            });
        }
        if let Some(b) = child(tc, "tcBdr") {
            let side = |n: &str| child(b, n).and_then(|s| line_of(s, colors)).flatten();
            look.borders = [side("left"), side("top"), side("right"), side("bottom")];
            look.inside_h = side("insideH");
            look.inside_v = side("insideV");
        }
    }
    look
}

fn table_element(table: Node, frame: Frame, cx: &Context, sink: &mut Sink) -> Option<Element> {
    let columns: Vec<f32> = children(child(table, "tblGrid")?, "gridCol").map(|c| emu(c, "w").unwrap_or(0.0)).collect();
    let pr = child(table, "tblPr");
    let opt = |name: &str| pr.and_then(|p| flag(p, name)).unwrap_or(false);
    let style_id = pr.and_then(|p| child(p, "tableStyleId")).and_then(|s| s.text()).unwrap_or("").trim().to_string();
    let styles_doc = Document::parse(sink.table_styles).ok();
    let default_doc = Document::parse(DEFAULT_TABLE_STYLE).ok();
    let style = styles_doc
        .as_ref()
        .and_then(|d| d.descendants().find(|n| n.tag_name().name() == "tblStyle" && n.attribute("styleId") == Some(style_id.as_str())))
        .or_else(|| (style_id == "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}").then(|| default_doc.as_ref().map(|d| d.root_element())).flatten());

    let rows_xml: Vec<Node> = children(table, "tr").collect();
    let (n_rows, n_cols) = (rows_xml.len(), columns.len());
    let mut rows = vec![];
    for (ri, tr) in rows_xml.iter().enumerate() {
        let mut cells = vec![];
        for (ci, tc) in children(*tr, "tc").enumerate() {
            // The table style's parts, from the whole table to the most specific
            let mut look = CellLook::default();
            if let Some(style) = style {
                look = table_style_part(style, "wholeTbl", &cx.colors);
                let band_row = if opt("firstRow") { ri.wrapping_sub(1) } else { ri };
                if opt("bandRow") && !(opt("firstRow") && ri == 0) && !(opt("lastRow") && ri + 1 == n_rows) {
                    look.over(&table_style_part(style, if band_row % 2 == 0 { "band1H" } else { "band2H" }, &cx.colors));
                }
                if opt("bandCol") {
                    look.over(&table_style_part(style, if ci % 2 == 0 { "band1V" } else { "band2V" }, &cx.colors));
                }
                if opt("firstCol") && ci == 0 {
                    look.over(&table_style_part(style, "firstCol", &cx.colors));
                }
                if opt("lastCol") && ci + 1 == n_cols {
                    look.over(&table_style_part(style, "lastCol", &cx.colors));
                }
                if opt("lastRow") && ri + 1 == n_rows {
                    look.over(&table_style_part(style, "lastRow", &cx.colors));
                }
                if opt("firstRow") && ri == 0 {
                    look.over(&table_style_part(style, "firstRow", &cx.colors));
                }
                // inside borders become the borders between cells
                if ri > 0 && look.borders[1].is_none() {
                    look.borders[1] = look.inside_h.clone();
                }
                if ci > 0 && look.borders[0].is_none() {
                    look.borders[0] = look.inside_v.clone();
                }
            }
            let tc_pr = child(tc, "tcPr");
            let mut fill = look.fill.clone();
            if let Some(p) = tc_pr
                && p.children().any(|c| matches!(c.tag_name().name(), "noFill" | "solidFill" | "gradFill" | "blipFill"))
            {
                fill = fill_of(p, &cx.colors, &cx.rels, sink.media, sink.package);
            }
            let mut borders = look.borders.clone();
            if let Some(p) = tc_pr {
                for (i, name) in ["lnL", "lnT", "lnR", "lnB"].iter().enumerate() {
                    if let Some(ln) = child(p, name) {
                        borders[i] = if child(ln, "noFill").is_some() {
                            None
                        } else {
                            let color = child(ln, "solidFill").and_then(|f| cx.colors.first(f));
                            color.map(|color| Line { color, width: emu(ln, "w").unwrap_or(0.75), dash: None, head: None, tail: None })
                        };
                    }
                }
            }
            let mut levels = cx.base[3].clone();
            for l in &mut levels.0 {
                if look.color.is_some() {
                    l.run.color = look.color;
                }
                if look.bold.is_some() {
                    l.run.bold = look.bold;
                }
            }
            let margins = [
                tc_pr.and_then(|p| emu(p, "marL")).unwrap_or(7.2),
                tc_pr.and_then(|p| emu(p, "marT")).unwrap_or(3.6),
                tc_pr.and_then(|p| emu(p, "marR")).unwrap_or(7.2),
                tc_pr.and_then(|p| emu(p, "marB")).unwrap_or(3.6),
            ];
            let anchor = match tc_pr.and_then(|p| p.attribute("anchor")) {
                Some("ctr") => Anchor::Middle,
                Some("b") => Anchor::Bottom,
                _ => Anchor::Top,
            };
            let mut text = child(tc, "txBody")
                .and_then(|b| text_body(b, &[], &levels, cx, false))
                .unwrap_or(TextBody { insets: margins, anchor, wrap: true, grows: false, paragraphs: vec![] });
            text.insets = margins;
            text.anchor = anchor;
            cells.push(TableCell {
                col_span: num(tc, "gridSpan").unwrap_or(1.0).max(1.0) as usize,
                row_span: num(tc, "rowSpan").unwrap_or(1.0).max(1.0) as usize,
                merged: flag(tc, "hMerge").unwrap_or(false) || flag(tc, "vMerge").unwrap_or(false),
                fill,
                borders,
                text,
            });
        }
        rows.push(TableRow { height: emu(*tr, "h").unwrap_or(0.0), cells });
    }
    (!rows.is_empty()).then_some(Element::Table { frame, columns, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_theme_modifiers() {
        assert_eq!(from_hsl(0.0, 1.0, 0.5), [255, 0, 0]);
        let (h, s, l) = to_hsl([68, 114, 196]);
        assert_eq!(from_hsl(h, s, l), [68, 114, 196]);
        assert_eq!(number_text("arabicPeriod", 3), "3.");
        assert_eq!(number_text("romanUcPeriod", 4), "IV.");
        assert_eq!(number_text("alphaLcParenR", 2), "b)");
        assert_eq!(resolve("ppt/slides", "../media/image1.png"), "ppt/media/image1.png");
    }

    /// An Embedded OpenType wrapper as PowerPoint saves one: a 228-byte header, then the font
    fn eot(font: &[u8], flags: u32) -> Vec<u8> {
        let mut out = vec![0u8; 228];
        let size = (out.len() + font.len()) as u32;
        out[0..4].copy_from_slice(&size.to_le_bytes());
        out[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
        out[8..12].copy_from_slice(&0x0002_0002u32.to_le_bytes());
        out[12..16].copy_from_slice(&flags.to_le_bytes());
        out[34..36].copy_from_slice(&[0x4C, 0x50]);
        out.extend_from_slice(font);
        out
    }

    #[test]
    fn embedded_fonts_are_unwrapped() {
        let font: Vec<u8> = [0, 1, 0, 0].into_iter().chain((0..60).map(|i| i as u8)).collect();
        // A plain font file, and one in an EOT wrapper, plain or XOR-obscured
        assert_eq!(font_data(&font).as_deref(), Some(font.as_slice()));
        assert_eq!(font_data(&eot(&font, 0)).as_deref(), Some(font.as_slice()));
        let obscured: Vec<u8> = font.iter().map(|b| b ^ 0x50).collect();
        assert_eq!(font_data(&eot(&obscured, 0x1000_0000)).as_deref(), Some(font.as_slice()));
        // MicroType Express compression is not unpacked; damaged headers are refused
        assert_eq!(font_data(&eot(&font, 0x4)), None);
        let mut bad = eot(&font, 0);
        bad[4..8].copy_from_slice(&10_000u32.to_le_bytes());
        assert_eq!(font_data(&bad), None);
        assert_eq!(font_data(b"not a font at all, not even close"), None);
    }
}
