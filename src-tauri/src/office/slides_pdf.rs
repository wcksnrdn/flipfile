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
    super::typeset::to_pdf_in_runs(runs, files, &deck.fonts).map_err(|e| format!("the slides could not be laid out ({e})"))
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
    // Unwrapped text is aligned in the box too: its lines only stay whole
    format!(
        "box(inset: (left: {}, top: {}), width: {}, height: {}, align({}, block(width: {}, {{ {} }})))",
        pt(l),
        pt(t),
        pt(w - r),
        pt(t + ih),
        anchor_name(text.anchor),
        pt(iw),
        paragraphs_markup(text, Some(iw), saved_unwrapped(text, ih))
    )
}

/// Whether a body that grows with its text (spAutoFit) was saved as tall as one line
/// per paragraph: the author's app broke none of them over several lines
fn saved_unwrapped(text: &TextBody, height: f32) -> bool {
    if !text.grows || !text.wrap {
        return false;
    }
    let (mut total, mut smallest) = (0.0, f32::MAX);
    for (i, p) in text.paragraphs.iter().enumerate() {
        let size = p.runs.iter().filter(|r| !r.line_break && !r.text.is_empty()).map(|r| r.size).fold(0.0, f32::max);
        let size = if size > 0.0 { size } else { p.size };
        let line = match p.line_spacing {
            LineSpacing::Multiple(m) => 1.2 * m * size,
            LineSpacing::Points(points) => points,
        };
        total += line * (1 + p.runs.iter().filter(|r| r.line_break).count()) as f32;
        if i > 0 {
            total += p.space_before;
        }
        if i + 1 < text.paragraphs.len() {
            total += p.space_after;
        }
        smallest = smallest.min(line);
    }
    (height - total).abs() < smallest * 0.5
}

/// Keeps a paragraph's `content` whole, on one line per line break however wide, when
/// it is at most `limit` wide (`None`: always). A box of its own width cannot break,
/// and the paragraph still aligns it: wider than the text area, it spills past the far
/// side of its alignment (both sides when centred), as in PowerPoint.
fn whole_line(content: &str, limit: Option<f32>) -> String {
    // Without a width, a box takes the paragraph's and wraps inside it
    let kept = "box(width: natural, line)".to_string();
    let kept = match limit {
        Some(limit) => format!("if natural <= {} {{ {kept} }} else {{ line }}", pt(limit)),
        None => kept,
    };
    format!("context {{ let line = {{ {content} }}; let natural = measure(line).width; {kept} }}")
}

/// The paragraphs of a text body `width` wide (`None`: a table cell, whose text always
/// wraps), one block each. `unwrapped`: the author's app set every paragraph on one
/// line (see `saved_unwrapped`).
fn paragraphs_markup(text: &TextBody, width: Option<f32>, unwrapped: bool) -> String {
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
        let line = pieces.join(" + ");
        if content.is_empty() {
            content = line;
        } else {
            content = format!("{content} + {line}");
        }
        if width.is_some() && !text.wrap {
            content = whole_line(&content, None);
        } else if let Some(room) = width.map(|w| w - start).filter(|_| unwrapped) {
            // A line the author's app fit is kept whole when it comes out a little wider
            // here (it kerned, or measured otherwise): breaking it would push its last
            // word over whatever lies below the box, which was placed for one line. Much
            // wider means the saved height is out of date, and the text wraps as usual.
            content = whole_line(&content, Some(room * 1.05));
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
        paragraphs_markup(&cell.text, None, false)
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    /// A one-slide deck holding `boxes` (spTree children)
    fn deck(boxes: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let parts = [
            ("_rels/.rels", format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{rel}/officeDocument\" Target=\"ppt/presentation.xml\"/></Relationships>")),
            ("ppt/presentation.xml", format!("<p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"{rel}\"><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/></p:sldIdLst><p:sldSz cx=\"18288000\" cy=\"10287000\"/></p:presentation>")),
            ("ppt/_rels/presentation.xml.rels", format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{rel}/slide\" Target=\"slides/slide1.xml\"/></Relationships>")),
            ("ppt/slides/slide1.xml", format!("<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><p:cSld><p:spTree>{boxes}</p:spTree></p:cSld></p:sld>")),
        ];
        for (name, xml) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    /// A text box `width` points wide and `height` tall, one paragraph of 40 pt text
    /// with lines 56 pt apart
    fn text_box(font: &str, text: &str, width: f32, height: f32, fit: &str) -> String {
        text_box_at(font, text, 0.0, width, height, "", fit, "ctr")
    }

    /// `text_box`, `x` points from the left, with more `bodyPr` attributes and an alignment
    #[allow(clippy::too_many_arguments)]
    fn text_box_at(font: &str, text: &str, x: f32, width: f32, height: f32, body: &str, fit: &str, align: &str) -> String {
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"t\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"0\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" {body}>{fit}</a:bodyPr><a:p><a:pPr algn=\"{align}\"><a:lnSpc><a:spcPts val=\"5600\"/></a:lnSpc></a:pPr><a:r><a:rPr sz=\"4000\"><a:latin typeface=\"{font}\"/></a:rPr><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>",
            (x * 12_700.0) as i64,
            (width * 12_700.0) as i64,
            (height * 12_700.0) as i64
        )
    }

    /// Lines of text on the PDF's first page
    fn lines(pdf: &[u8]) -> usize {
        text_extent(pdf).0
    }

    /// Lines of text on the PDF's first page, and where the text starts and ends across
    /// it (points from the left edge)
    fn text_extent(pdf: &[u8]) -> (usize, f32, f32) {
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(pdf, None).unwrap();
        let page = doc.pages().first().unwrap();
        let text = page.text().unwrap();
        let count = text.all().lines().filter(|l| !l.trim().is_empty()).count();
        let (mut left, mut right) = (f32::MAX, f32::MIN);
        for segment in text.segments().iter() {
            let bounds = segment.bounds();
            left = left.min(bounds.left().value);
            right = right.max(bounds.right().value);
        }
        (count, left, right)
    }

    /// The width of `text` at 40 pt in a font every computer has one of
    fn common_font(text: &str) -> Option<(&'static str, f32)> {
        ["Arial", "Helvetica", "Liberation Sans", "DejaVu Sans"].into_iter().find_map(|f| super::super::typeset::text_width(f, 40.0, false, false, text).map(|w| (f, w)))
    }

    #[test]
    fn titles_the_author_fit_on_one_line_stay_on_one() {
        let text = "Catatan Perubahan Belum Terlaksana";
        let Some((font, natural)) = common_font(text) else { return };
        // 2% narrower than the text here, as when the author's app kerned it: the box
        // was saved one line tall, so the line stays whole
        let snug = text_box(font, text, natural * 0.98, 53.5, "<a:spAutoFit/>");
        assert_eq!(lines(&super::super::pptx_to_pdf_builtin(&deck(&snug)).unwrap()), 1);
        // Without that evidence, or far too narrow (an out-of-date height), it wraps
        let fixed = text_box(font, text, natural * 0.98, 53.5, "");
        assert_eq!(lines(&super::super::pptx_to_pdf_builtin(&deck(&fixed)).unwrap()), 2);
        let stale = text_box(font, text, natural * 0.7, 53.5, "<a:spAutoFit/>");
        assert_eq!(lines(&super::super::pptx_to_pdf_builtin(&deck(&stale)).unwrap()), 2);
        // Saved two lines tall: the author's app wrapped it too
        let wrapped = text_box(font, text, natural * 0.98, 109.5, "<a:spAutoFit/>");
        assert_eq!(lines(&super::super::pptx_to_pdf_builtin(&deck(&wrapped)).unwrap()), 2);
    }

    #[test]
    fn unwrapped_text_overflows_its_box_on_one_line() {
        let text = "Catatan Perubahan Belum Terlaksana";
        let Some((font, natural)) = common_font(text) else { return };
        // A box half as wide as its text, centred on x = 600 pt: PowerPoint keeps the
        // line whole, spilling out of both sides of the box alike, or past its far side
        // when the text is aligned to one
        let (x, width) = (600.0 - natural / 4.0, natural / 2.0);
        for (align, start, end) in [("ctr", 600.0 - natural / 2.0, 600.0 + natural / 2.0), ("l", x, x + natural), ("r", x + width - natural, x + width)] {
            let shape = text_box_at(font, text, x, width, 60.0, "wrap=\"none\"", "", align);
            let (count, left, right) = text_extent(&super::super::pptx_to_pdf_builtin(&deck(&shape)).unwrap());
            assert_eq!(count, 1, "{align}");
            assert!((left - start).abs() < 6.0 && (right - end).abs() < 6.0, "{align}: {left}..{right}, expected {start}..{end}");
        }
        // Twice as wide as its text: aligned in the box, as wrapped text would be
        let (x, width) = (600.0 - natural, natural * 2.0);
        for (align, start) in [("ctr", 600.0 - natural / 2.0), ("l", x), ("r", x + width - natural)] {
            let shape = text_box_at(font, text, x, width, 60.0, "wrap=\"none\"", "", align);
            let (count, left, right) = text_extent(&super::super::pptx_to_pdf_builtin(&deck(&shape)).unwrap());
            assert_eq!(count, 1, "{align}");
            assert!((left - start).abs() < 6.0 && (right - start - natural).abs() < 6.0, "{align}: {left}..{right}, expected {start}..");
        }
    }
}
