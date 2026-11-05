// Draws the slides `pptx::read` returns as Typst pages: one page per slide, every
// element placed where PowerPoint puts it, laid out in runs of a few slides at a time.

use std::collections::HashMap;
use std::fmt::Write;

use super::pptx::{Align, Anchor, Caps, Element, Fill, Frame, Geometry, Line, LineSpacing, PathCommand, PathFill, Presentation, Rgba, Slide, TableCell, TextBody};
use super::to_typst::{font_list, pt, q};

/// Slides laid out together: enough to share fonts and pictures in one layout, few
/// enough that a long deck uses several cores and little memory
const SLIDES_PER_RUN: usize = 4;

pub fn to_pdf(deck: &Presentation) -> Result<Vec<u8>, String> {
    let (runs, files) = markup(deck);
    super::typeset::to_pdf_in_runs(runs, files).map_err(|e| format!("the slides could not be laid out ({e})"))
}

/// The slides PowerPoint would export: hidden ones are left out (unless all are hidden)
pub fn shown(deck: &Presentation) -> Vec<&Slide> {
    let visible: Vec<&Slide> = deck.slides.iter().filter(|s| !s.hidden).collect();
    if visible.is_empty() { deck.slides.iter().collect() } else { visible }
}

/// Typst markup for every run of slides, and the pictures they use
pub fn markup(deck: &Presentation) -> (Vec<String>, Vec<(String, Vec<u8>)>) {
    let mut pictures = Pictures::default();
    let preamble = format!(
        "#set page(width: {}, height: {}, margin: 0pt)\n#set par(spacing: 0pt, leading: 0pt, linebreaks: \"simple\", justification-limits: (spacing: (min: 100%, max: 1000%), tracking: (min: 0em, max: 0em)))\n#set block(spacing: 0pt)\n#set text(hyphenate: false, overhang: false, ligatures: false)\n",
        pt(deck.width),
        pt(deck.height)
    );
    let slides = shown(deck);
    let mut runs = vec![];
    for chunk in slides.chunks(SLIDES_PER_RUN) {
        let mut out = preamble.clone();
        for slide in chunk {
            slide_markup(&mut out, slide, deck, &mut pictures);
        }
        runs.push(out);
    }
    if runs.is_empty() {
        runs.push(preamble + "#page[]\n");
    }
    (runs, pictures.files)
}

/// Pictures handed to Typst, once each, in a format it reads
#[derive(Default)]
struct Pictures {
    paths: HashMap<String, Option<String>>,
    files: Vec<(String, Vec<u8>)>,
}

impl Pictures {
    fn path(&mut self, media: &str, deck: &Presentation) -> Option<String> {
        if let Some(p) = self.paths.get(media) {
            return p.clone();
        }
        let path = deck.media.get(media).and_then(|data| {
            let ext = media.rsplit('.').next().unwrap_or("").to_lowercase();
            let index = self.files.len();
            match ext.as_str() {
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => {
                    let path = format!("/media/{index}.{ext}");
                    self.files.push((path.clone(), data.clone()));
                    Some(path)
                }
                // EMF/WMF and other vector formats Typst cannot draw are left out
                "emf" | "wmf" | "emz" | "wmz" => None,
                _ => {
                    let img = image::load_from_memory(data).ok()?;
                    let mut png = Vec::new();
                    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
                    let path = format!("/media/{index}.png");
                    self.files.push((path.clone(), png));
                    Some(path)
                }
            }
        });
        self.paths.insert(media.to_string(), path.clone());
        path
    }
}

fn color(c: Rgba) -> String {
    let [r, g, b] = c.rgb;
    if c.alpha >= 0.999 {
        format!("rgb({r}, {g}, {b})")
    } else {
        format!("rgb({r}, {g}, {b}, {})", (c.alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

/// A Typst paint for a solid or gradient fill (pictures are drawn separately)
fn paint(fill: &Fill) -> Option<String> {
    match fill {
        Fill::Solid(c) => Some(color(*c)),
        Fill::Gradient(stops, angle) => {
            let stops: Vec<String> = stops.iter().map(|(pos, c)| format!("({}, {:.1}%)", color(*c), pos.clamp(0.0, 1.0) * 100.0)).collect();
            Some(format!("gradient.linear({}, angle: {:.1}deg)", stops.join(", "), angle))
        }
        Fill::Picture(_) => None,
    }
}

fn stroke(line: Option<&Line>) -> String {
    match line {
        Some(l) => {
            let dash = l.dash.map(|d| format!(", dash: {}", q(d))).unwrap_or_default();
            format!("(paint: {}, thickness: {}{dash})", color(l.color), pt(l.width.max(0.25)))
        }
        None => "none".into(),
    }
}

fn slide_markup(out: &mut String, slide: &Slide, deck: &Presentation, pictures: &mut Pictures) {
    let mut fill = "none".to_string();
    let mut background = "none".to_string();
    match &slide.background {
        Some(Fill::Picture(media)) => {
            if let Some(path) = pictures.path(media, deck) {
                background = format!("image({}, width: 100%, height: 100%, fit: \"stretch\")", q(&path));
            }
        }
        Some(f) => fill = paint(f).unwrap_or(fill),
        None => fill = "white".into(),
    }
    let _ = writeln!(out, "#page(fill: {fill}, background: {background})[");
    for element in &slide.elements {
        element_markup(out, element, deck, pictures);
    }
    out.push_str("]\n");
}

/// Places `body` (drawn in a w × h box) at the frame, rotated about its centre
fn place(out: &mut String, frame: &Frame, body: &str) {
    let boxed = format!("box(width: {}, height: {}, {body})", pt(frame.w.max(0.0)), pt(frame.h.max(0.0)));
    let rotated = if frame.rotation.is_finite() && frame.rotation.rem_euclid(360.0).abs() > 0.01 { format!("rotate({:.2}deg, origin: center + horizon, reflow: false, {boxed})", frame.rotation) } else { boxed };
    let _ = writeln!(out, "#place(top + left, dx: {}, dy: {}, {rotated})", pt(frame.x), pt(frame.y));
}

fn element_markup(out: &mut String, element: &Element, deck: &Presentation, pictures: &mut Pictures) {
    match element {
        Element::Shape { frame, geometry, fill, line, text, .. } => {
            let mut body = vec![];
            let paint = fill_paint(frame, fill.as_ref(), deck, pictures);
            if let Some(g) = geometry_markup(frame, geometry, paint.as_ref(), line.as_ref()) {
                body.push(format!("place(top + left, {g})"));
            }
            if let Some(text) = text {
                body.push(format!("place(top + left, {})", text_markup(text, frame.w, frame.h)));
            }
            if !body.is_empty() {
                place(out, frame, &format!("{{ {} }}", body.join("\n")));
            }
        }
        Element::Picture { frame, media, crop, geometry, line } => {
            let Some(content) = picture_content(frame, media, *crop, deck, pictures) else { return };
            let body = match geometry {
                Geometry::Rect => format!("box(width: 100%, height: 100%, stroke: {}, {content})", stroke(line.as_ref())),
                // Other outlines are filled with the picture
                g => geometry_markup(frame, g, Some(&format!("tiling(size: ({}, {}), relative: \"parent\", {content})", pt(frame.w), pt(frame.h))), line.as_ref()).unwrap_or_default(),
            };
            place(out, frame, &body);
        }
        Element::Table { frame, columns, rows } => table_markup(out, frame, columns, rows),
    }
}

/// A picture cut by `crop` and stretched over the frame, clipped to it
fn picture_content(frame: &Frame, media: &str, crop: [f32; 4], deck: &Presentation, pictures: &mut Pictures) -> Option<String> {
    let path = pictures.path(media, deck)?;
    let [l, t, r, b] = crop;
    let (sw, sh) = ((1.0 - l - r).max(0.01), (1.0 - t - b).max(0.01));
    let (w, h) = (frame.w / sw, frame.h / sh);
    let img = format!("image({}, width: {}, height: {}, fit: \"stretch\")", q(&path), pt(w), pt(h));
    let mut content = format!("box(clip: true, width: {}, height: {}, place(top + left, dx: {}, dy: {}, {img}))", pt(frame.w), pt(frame.h), pt(-l * w), pt(-t * h));
    // A flipped picture shows its cropped part mirrored
    if frame.flip_h || frame.flip_v {
        content = format!("scale(x: {}%, y: {}%, {content})", if frame.flip_h { -100 } else { 100 }, if frame.flip_v { -100 } else { 100 });
    }
    Some(content)
}

/// A point in the box, honouring flips
fn point(frame: &Frame, x: f32, y: f32) -> String {
    let x = if frame.flip_h { frame.w - x } else { x };
    let y = if frame.flip_v { frame.h - y } else { y };
    format!("({}, {})", pt(x), pt(y))
}

/// The paint of a fill: a colour, gradient or picture (as a tiling)
fn fill_paint(frame: &Frame, fill: Option<&Fill>, deck: &Presentation, pictures: &mut Pictures) -> Option<String> {
    match fill? {
        Fill::Picture(media) => {
            let content = picture_content(frame, media, [0.0; 4], deck, pictures)?;
            Some(format!("tiling(size: ({}, {}), relative: \"parent\", {content})", pt(frame.w), pt(frame.h)))
        }
        f => paint(f),
    }
}

fn geometry_markup(frame: &Frame, geometry: &Geometry, fill: Option<&String>, line: Option<&Line>) -> Option<String> {
    let s = stroke(line);
    let f = fill.cloned().unwrap_or_else(|| "none".into());
    if f == "none" && s == "none" {
        return None;
    }
    let (w, h) = (pt(frame.w), pt(frame.h));
    Some(match geometry {
        Geometry::Rect => format!("rect(width: {w}, height: {h}, fill: {f}, stroke: {s})"),
        Geometry::RoundRect(r) => {
            let [mut tl, mut tr, mut br, mut bl] = r.map(|v| v.clamp(0.0, frame.w.min(frame.h) / 2.0));
            if frame.flip_h {
                (tl, tr, br, bl) = (tr, tl, bl, br);
            }
            if frame.flip_v {
                (tl, tr, br, bl) = (bl, br, tr, tl);
            }
            format!(
                "rect(width: {w}, height: {h}, fill: {f}, stroke: {s}, radius: (top-left: {}, top-right: {}, bottom-right: {}, bottom-left: {}))",
                pt(tl),
                pt(tr),
                pt(br),
                pt(bl)
            )
        }
        Geometry::Ellipse => format!("ellipse(width: {w}, height: {h}, fill: {f}, stroke: {s})"),
        Geometry::Paths(subs) => {
            let line_paint = line.map(|l| color(l.color)).unwrap_or_else(|| "none".into());
            let mut parts = vec![];
            for sub in subs {
                let mut steps = vec![];
                for c in &sub.commands {
                    steps.push(match c {
                        PathCommand::Move(x, y) => format!("curve.move({})", point(frame, *x, *y)),
                        PathCommand::Line(x, y) => format!("curve.line({})", point(frame, *x, *y)),
                        PathCommand::Cubic([a, b, c, d, e, g]) => format!("curve.cubic({}, {}, {})", point(frame, *a, *b), point(frame, *c, *d), point(frame, *e, *g)),
                        PathCommand::Close => "curve.close(mode: \"straight\")".into(),
                    });
                }
                let fill = match sub.fill {
                    PathFill::Shape => f.as_str(),
                    PathFill::Line => line_paint.as_str(),
                    PathFill::None => "none",
                };
                let stroke = if sub.stroke { s.as_str() } else { "none" };
                if (fill == "none" && stroke == "none") || steps.is_empty() {
                    continue;
                }
                let rule = if sub.even_odd { "even-odd" } else { "non-zero" };
                parts.push(format!("place(top + left, curve(fill: {fill}, fill-rule: \"{rule}\", stroke: {stroke}, {}))", steps.join(", ")));
            }
            if parts.is_empty() {
                return None;
            }
            format!("{{ {} }}", parts.join("\n"))
        }
    })
}

fn anchor_name(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::Top => "top",
        Anchor::Middle => "horizon",
        Anchor::Bottom => "bottom",
    }
}

/// A text body in a w × h box: insets, vertical anchor, paragraphs
fn text_markup(text: &TextBody, w: f32, h: f32) -> String {
    let [l, t, r, b] = text.insets;
    let (iw, ih) = ((w - l - r).max(1.0), (h - t - b).max(0.0));
    let width = if text.wrap { pt(iw) } else { "auto".into() };
    format!(
        "box(inset: (left: {}, top: {}), width: {}, height: {}, align({}, block(width: {width}, {{ {} }})))",
        pt(l),
        pt(t),
        pt(w - r),
        pt(t + ih),
        anchor_name(text.anchor),
        paragraphs_markup(text)
    )
}

/// The paragraphs of a text body, one block each
fn paragraphs_markup(text: &TextBody) -> String {
    let mut paragraphs = vec![];
    for (i, p) in text.paragraphs.iter().enumerate() {
        let first_size = p.runs.iter().find(|r| !r.line_break && !r.text.is_empty()).map(|r| r.size).unwrap_or(p.size);
        // PowerPoint's single line is 1.2 × the font size; the baseline sits a
        // descent above the line's bottom
        let families: Vec<String> = p.runs.first().map(|r| vec![r.font.clone()]).unwrap_or_default();
        let (_, descent) = super::typeset::line_metrics(&families);
        let (top_edge, bottom_edge) = match p.line_spacing {
            LineSpacing::Multiple(m) => (format!("{:.3}em", (1.2 * m - descent).max(0.3)), format!("{:.3}em", -descent)),
            LineSpacing::Points(points) => (pt((points - descent * first_size).max(1.0)), pt(-descent * first_size)),
        };
        let align = match p.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "left",
        };
        // The first line starts at margin + indent; the others at the margin
        let start = (p.margin_left + p.indent).max(0.0);
        let hang = p.margin_left - start;
        let mut content = String::new();
        if let Some(bullet) = &p.bullet {
            let first = p.runs.iter().find(|r| !r.line_break);
            let font = bullet.font.as_deref().filter(|f| !is_symbol_font(f)).map(str::to_string).or_else(|| first.map(|r| r.font.clone())).unwrap_or_else(|| "Calibri".into());
            let mut symbol = super::docx::bullet_char(&bullet.text, bullet.font.as_deref());
            if !symbol.chars().all(super::typeset::covers) {
                symbol = "•".into();
            }
            let fill = bullet.color.or(first.map(|r| r.color)).map(color).unwrap_or_else(|| "black".into());
            let size = bullet.points.unwrap_or(first_size * bullet.scale);
            let mark = format!("text(font: {}, size: {}, fill: {fill}, {})", font_list(&font), pt(size), q(&symbol));
            if hang > size * 0.6 {
                let _ = write!(content, "box(width: {}, {mark})", pt(hang));
            } else {
                let _ = write!(content, "{mark} + h(0.4em)");
            }
        }
        let mut pieces = vec![];
        for run in &p.runs {
            if run.line_break {
                pieces.push("linebreak()".to_string());
                continue;
            }
            if run.text.is_empty() {
                continue;
            }
            let mut size = run.size;
            let mut extra = String::new();
            if run.baseline.abs() > 0.001 {
                extra = format!(", baseline: {}", pt(-run.baseline * run.size));
                size *= 2.0 / 3.0;
            }
            let weight = if run.bold { ", weight: \"bold\"" } else { "" };
            let style = if run.italic { ", style: \"italic\"" } else { "" };
            let kerning = if run.kern { "" } else { ", kerning: false" };
            if run.spacing.abs() > 0.01 {
                extra.push_str(&format!(", tracking: {}", pt(run.spacing)));
            }
            let text = run.text.replace('\u{b}', " ");
            let text = if run.caps == Caps::All { text.to_uppercase() } else { text };
            let mut piece = format!("text(font: {}, size: {}, fill: {}{weight}{style}{kerning}{extra}, {})", font_list(&run.font), pt(size), color(run.color), q(&text));
            if run.caps == Caps::Small {
                piece = format!("smallcaps(all: true, {piece})");
            }
            if let Some(h) = run.highlight {
                piece = format!("highlight(fill: {}, {piece})", color(h));
            }
            if run.underline {
                piece = format!("underline({piece})");
            }
            if run.strike {
                piece = format!("strike({piece})");
            }
            pieces.push(piece);
        }
        if pieces.is_empty() {
            // An empty paragraph keeps the height of its line
            pieces.push(format!("hide(text(size: {}, \"X\"))", pt(p.size)));
        }
        let joined = pieces.join(" + ");
        let line = if text.wrap { joined } else { format!("box({joined})") };
        if content.is_empty() {
            content = line;
        } else {
            content = format!("{content} + {line}");
        }
        // PowerPoint spaces paragraphs apart, not the text from its box
        let before = if i == 0 { 0.0 } else { p.space_before };
        let after = if i + 1 == text.paragraphs.len() { 0.0 } else { p.space_after };
        paragraphs.push(format!(
            "block(width: 100%, inset: (left: {}, top: {}, bottom: {}), {{ set text(size: {}, top-edge: {top_edge}, bottom-edge: {bottom_edge}); align({align}, par(justify: {}, hanging-indent: {}, {content})) }})",
            pt(start),
            pt(before),
            pt(after),
            pt(first_size),
            p.align == Align::Justify,
            pt(hang),
        ));
    }
    paragraphs.join("\n")
}

fn is_symbol_font(f: &str) -> bool {
    let n = f.to_lowercase();
    n.contains("symbol") || n.contains("wingdings") || n.contains("webdings")
}

fn table_markup(out: &mut String, frame: &Frame, columns: &[f32], rows: &[super::pptx::TableRow]) {
    // Rows are at least as tall as the file says and grow with their text: an extra
    // empty column holds each row's minimum height
    let mut cols: Vec<String> = columns.iter().map(|w| pt(*w)).collect();
    cols.push("0pt".into());
    // Every cell is placed explicitly on a grid of what is taken, its spans cut to the
    // free room: a span past the edge or into a neighbour would fail the whole slide
    let n = columns.len();
    let mut taken = vec![vec![false; n]; rows.len()];
    let mut cells = vec![];
    for (y, row) in rows.iter().enumerate() {
        let mut x = 0;
        for cell in &row.cells {
            if cell.merged {
                continue;
            }
            while x < n && taken[y][x] {
                x += 1;
            }
            if x == n {
                break;
            }
            let col_span = (x..n).take(cell.col_span.max(1)).take_while(|&c| !taken[y][c]).count();
            let row_span = (y..rows.len()).take(cell.row_span.max(1)).take_while(|&r| taken[r][x..x + col_span].iter().all(|t| !t)).count();
            for r in &mut taken[y..y + row_span] {
                r[x..x + col_span].fill(true);
            }
            cells.push(cell_markup(cell, x, y, col_span, row_span));
            x += col_span;
        }
        cells.push(format!("table.cell(x: {n}, y: {y}, inset: 0pt, stroke: none, box(width: 0pt, height: {}))", pt(row.height)));
    }
    let body = format!("table(columns: ({}), inset: 0pt, stroke: none, {})", cols.join(", "), cells.join(",\n"));
    let _ = writeln!(out, "#place(top + left, dx: {}, dy: {}, {body})", pt(frame.x), pt(frame.y));
}

fn cell_markup(cell: &TableCell, x: usize, y: usize, col_span: usize, row_span: usize) -> String {
    let side = |l: &Option<Line>| stroke(l.as_ref());
    let [l, t, r, b] = &cell.borders;
    let fill = cell.fill.as_ref().and_then(paint).unwrap_or_else(|| "none".into());
    let [il, it, ir, ib] = cell.text.insets;
    format!(
        "table.cell(x: {x}, y: {y}, colspan: {col_span}, rowspan: {row_span}, fill: {fill}, stroke: (left: {}, top: {}, right: {}, bottom: {}), inset: (left: {}, top: {}, right: {}, bottom: {}), align: {}, block(width: 100%, {{ {} }}))",
        side(l),
        side(t),
        side(r),
        side(b),
        pt(il),
        pt(it),
        pt(ir),
        pt(ib),
        anchor_name(cell.text.anchor),
        paragraphs_markup(&cell.text)
    )
}
