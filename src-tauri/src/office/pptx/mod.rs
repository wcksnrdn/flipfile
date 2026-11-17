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

// Reading a deck, in the order the reader needs it. The submodules share one namespace
// (each starts with `use super::*`), as when this was a single file.
mod colors;
mod package;
mod reader;
mod shape_tree;
mod tables;
mod templates;
mod text;
mod text_styles;
mod theme;
mod xml;

#[cfg(test)]
mod tests;

use colors::*;
use package::*;
use reader::*;
use shape_tree::*;
use tables::*;
use templates::*;
use text::*;
use text_styles::*;
use theme::*;
use xml::*;

/// The way in: a .pptx file's bytes to the slides below
pub use reader::read;

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

pub use crate::office::shapes::{Geometry, LineEnd, PathCommand, PathFill};

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
