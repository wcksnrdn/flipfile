// What a sheet's drawing layer holds besides pictures: shapes and text boxes (with the
// same preset outlines as slides, see `shapes`), and the colours DrawingML describes
// (theme colours with their lighter/darker modifiers), which charts use too.

use roxmltree::Node;

use super::shapes::{self, Geometry, PathCommand, PathFill};
use super::xlsx::Rgb;

/// Points per EMU
pub const EMU: f32 = 1.0 / 12700.0;

pub fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}

pub fn children<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| c.tag_name().name() == name)
}

fn val_f32(n: Node, name: &str) -> Option<f32> {
    n.attribute(name).and_then(|v| v.parse().ok())
}

// ---------------------------------------------------------------------------
// Colours

fn to_hsl([r, g, b]: Rgb) -> (f32, f32, f32) {
    let [r, g, b] = [r, g, b].map(|v| v as f32 / 255.0);
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
    } / 6.0;
    (h, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> Rgb {
    if s == 0.0 {
        let v = (l.clamp(0.0, 1.0) * 255.0).round() as u8;
        return [v, v, v];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        t = t.rem_euclid(1.0);
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
    [hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0)].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A colour element (`srgbClr`, `schemeClr`, `sysClr`, `prstClr`) with its modifiers;
/// `theme` is dk1, lt1, dk2, lt2, accent1..6, hlink, folHlink
pub fn color(n: Node, theme: &[Rgb]) -> Option<Rgb> {
    let base = match n.tag_name().name() {
        "srgbClr" => hex(n.attribute("val")?),
        "sysClr" => n.attribute("lastClr").and_then(hex).or_else(|| match n.attribute("val") {
            Some("window") => Some([255, 255, 255]),
            _ => Some([0, 0, 0]),
        }),
        "schemeClr" => {
            let i = match n.attribute("val")? {
                "dk1" | "tx1" => 0,
                "lt1" | "bg1" => 1,
                "dk2" | "tx2" => 2,
                "lt2" | "bg2" => 3,
                "accent1" => 4,
                "accent2" => 5,
                "accent3" => 6,
                "accent4" => 7,
                "accent5" => 8,
                "accent6" => 9,
                "hlink" => 10,
                "folHlink" => 11,
                // the colour a style reference passes in: unknown here
                _ => 4,
            };
            theme.get(i).copied().or(Some(if i == 1 { [255, 255, 255] } else { [0, 0, 0] }))
        }
        "prstClr" => Some(match n.attribute("val")? {
            "white" => [255, 255, 255],
            "red" => [255, 0, 0],
            "green" => [0, 128, 0],
            "blue" => [0, 0, 255],
            "yellow" => [255, 255, 0],
            "gray" | "grey" => [128, 128, 128],
            _ => [0, 0, 0],
        }),
        _ => return None,
    }?;
    Some(modify(base, n))
}

fn hex(v: &str) -> Option<Rgb> {
    let x = u32::from_str_radix(v.get(v.len().checked_sub(6)?..)?, 16).ok()?;
    Some([(x >> 16) as u8, (x >> 8) as u8, x as u8])
}

/// lumMod / lumOff / tint / shade, in that order of appearance
fn modify(mut c: Rgb, n: Node) -> Rgb {
    for m in n.children().filter(|m| m.is_element()) {
        let v = val_f32(m, "val").unwrap_or(100_000.0) / 100_000.0;
        match m.tag_name().name() {
            "lumMod" => {
                let (h, s, l) = to_hsl(c);
                c = from_hsl(h, s, l * v);
            }
            "lumOff" => {
                let (h, s, l) = to_hsl(c);
                c = from_hsl(h, s, (l + v).min(1.0));
            }
            "tint" => c = c.map(|x| (x as f32 + (255.0 - x as f32) * (1.0 - v)).round() as u8),
            "shade" => c = c.map(|x| (x as f32 * v).round() as u8),
            _ => {}
        }
    }
    c
}

/// The colour of a fill element (`solidFill`, `gradFill` -> its first stop), or
/// `Some(None)` for `noFill`
pub fn fill(parent: Node, theme: &[Rgb]) -> Option<Option<Rgb>> {
    for f in parent.children().filter(|c| c.is_element()) {
        match f.tag_name().name() {
            "noFill" => return Some(None),
            "solidFill" => return Some(f.children().find(|c| c.is_element()).and_then(|c| color(c, theme))),
            "gradFill" => {
                let stop = f.descendants().find(|d| d.tag_name().name() == "gs");
                return Some(stop.and_then(|s| s.children().find(|c| c.is_element())).and_then(|c| color(c, theme)));
            }
            "pattFill" => return Some(child(f, "fgClr").and_then(|c| c.children().find(|c| c.is_element())).and_then(|c| color(c, theme))),
            _ => {}
        }
    }
    None
}

/// An outline (`a:ln`): colour and width in points, or `Some(None)` when there is none
pub fn line(sp_pr: Node, theme: &[Rgb]) -> Option<Option<(Rgb, f32)>> {
    let ln = child(sp_pr, "ln")?;
    let width = val_f32(ln, "w").map(|w| w * EMU).unwrap_or(0.75);
    match fill(ln, theme) {
        Some(None) => Some(None),
        Some(Some(c)) => Some(Some((c, width))),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// Shapes

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Rgb,
}

#[derive(Clone, Debug)]
pub struct Para {
    pub runs: Vec<Run>,
    /// "left", "center", "right"
    pub align: &'static str,
}

/// A shape or text box, positioned by its anchor (sizes in points)
#[derive(Clone, Debug)]
pub struct Shape {
    pub geometry: Geometry,
    /// where it sits inside its anchor's box, for shapes in a group: x, y, w, h as
    /// fractions of the box
    pub frac: [f32; 4],
    pub fill: Option<Rgb>,
    pub line: Option<(Rgb, f32)>,
    pub paras: Vec<Para>,
    /// "top", "horizon", "bottom"
    pub anchor: &'static str,
    /// left, top, right, bottom
    pub insets: [f32; 4],
    pub rotation: f32,
}

/// The shapes of one drawing anchor: a shape, a connector, or a group of them
pub fn shapes(anchor: Node, theme: &[Rgb], box_size: (f32, f32)) -> Vec<Shape> {
    let mut out = vec![];
    for n in anchor.children().filter(|c| c.is_element()) {
        collect(n, theme, box_size, None, &mut out);
    }
    out
}

/// A group's child coordinate space -> fractions of the anchor's box
#[derive(Clone, Copy)]
struct Space {
    off: (f32, f32),
    ext: (f32, f32),
    /// the group's box, as fractions of the anchor's box
    frac: [f32; 4],
}

fn xfrm(sp_pr: Option<Node>) -> Option<(f32, f32, f32, f32)> {
    let x = child(sp_pr?, "xfrm")?;
    let off = child(x, "off")?;
    let ext = child(x, "ext")?;
    Some((val_f32(off, "x")?, val_f32(off, "y")?, val_f32(ext, "cx")?, val_f32(ext, "cy")?))
}

fn place_in(space: Option<Space>, sp_pr: Option<Node>) -> [f32; 4] {
    match (space, xfrm(sp_pr)) {
        (Some(s), Some((x, y, w, h))) if s.ext.0 > 0.0 && s.ext.1 > 0.0 => {
            let fx = (x - s.off.0) / s.ext.0;
            let fy = (y - s.off.1) / s.ext.1;
            [s.frac[0] + fx * s.frac[2], s.frac[1] + fy * s.frac[3], w / s.ext.0 * s.frac[2], h / s.ext.1 * s.frac[3]]
        }
        (Some(s), _) => s.frac,
        _ => [0.0, 0.0, 1.0, 1.0],
    }
}

fn collect(n: Node, theme: &[Rgb], box_size: (f32, f32), space: Option<Space>, out: &mut Vec<Shape>) {
    match n.tag_name().name() {
        "grpSp" => {
            let pr = child(n, "grpSpPr");
            let frac = place_in(space, pr);
            let ch = pr.and_then(|p| child(p, "xfrm"));
            let inner = ch.and_then(|x| {
                let off = child(x, "chOff")?;
                let ext = child(x, "chExt")?;
                Some(Space { off: (val_f32(off, "x")?, val_f32(off, "y")?), ext: (val_f32(ext, "cx")?, val_f32(ext, "cy")?), frac })
            });
            for c in n.children().filter(|c| c.is_element()) {
                collect(c, theme, box_size, inner.or(Some(Space { off: (0.0, 0.0), ext: (1.0, 1.0), frac })), out);
            }
        }
        "sp" | "cxnSp" => {
            if let Some(s) = shape(n, theme, box_size, space) {
                out.push(s);
            }
        }
        _ => {}
    }
}

fn shape(n: Node, theme: &[Rgb], box_size: (f32, f32), space: Option<Space>) -> Option<Shape> {
    let sp_pr = child(n, "spPr");
    let frac = place_in(space, sp_pr);
    let (w, h) = (box_size.0 * frac[2], box_size.1 * frac[3]);
    let style = child(n, "style");
    // a style reference gives the colours the shape doesn't set itself
    let style_color = |name: &str| -> Option<(u32, Option<Rgb>)> {
        let r = child(style?, name)?;
        let idx = r.attribute("idx").and_then(|v| v.parse().ok()).unwrap_or(0);
        Some((idx, r.children().find(|c| c.is_element()).and_then(|c| color(c, theme))))
    };
    let preset = sp_pr.and_then(|p| child(p, "prstGeom"));
    let name = preset.and_then(|p| p.attribute("prst")).unwrap_or("rect");
    let adj: Vec<(String, f32)> = preset
        .and_then(|p| child(p, "avLst"))
        .map(|av| children(av, "gd").filter_map(|g| Some((g.attribute("name")?.to_string(), g.attribute("fmla")?.strip_prefix("val ")?.trim().parse().ok()?))).collect())
        .unwrap_or_default();
    let adjust = |key: &str, default: f32| adj.iter().find(|(k, _)| k == key).map(|(_, v)| *v).unwrap_or(default);
    let geometry = shapes::preset(name, &adjust, w.max(0.1), h.max(0.1)).unwrap_or(Geometry::Rect);
    let open = n.tag_name().name() == "cxnSp" || geometry.is_open();
    let fill = match sp_pr.and_then(|p| fill(p, theme)) {
        Some(f) => f,
        None => style_color("fillRef").filter(|(i, _)| *i > 0).and_then(|(_, c)| c),
    };
    let line = match sp_pr.and_then(|p| line(p, theme)) {
        Some(l) => l,
        None => {
            let width = sp_pr.and_then(|p| child(p, "ln")).and_then(|ln| val_f32(ln, "w")).map(|w| w * EMU).unwrap_or(0.75);
            style_color("lnRef").filter(|(i, _)| *i > 0).and_then(|(_, c)| c).map(|c| (c, width))
        }
    };
    let font_color = style_color("fontRef").and_then(|(_, c)| c).unwrap_or(theme.first().copied().unwrap_or([0, 0, 0]));
    let rotation = sp_pr.and_then(|p| child(p, "xfrm")).and_then(|x| val_f32(x, "rot")).unwrap_or(0.0) / 60000.0;
    let body = child(n, "txBody");
    let pr = body.and_then(|b| child(b, "bodyPr"));
    let inset = |name: &str, d: f32| pr.and_then(|p| val_f32(p, name)).map(|v| v * EMU).unwrap_or(d);
    let anchor = match pr.and_then(|p| p.attribute("anchor")) {
        Some("ctr") => "horizon",
        Some("b") => "bottom",
        _ => "top",
    };
    let paras = body.map(|b| paragraphs(b, theme, font_color)).unwrap_or_default();
    if fill.is_none() && line.is_none() && paras.iter().all(|p| p.runs.is_empty()) {
        return None;
    }
    Some(Shape {
        geometry,
        frac,
        fill: if open { None } else { fill },
        line,
        paras,
        anchor,
        insets: [inset("lIns", 7.2), inset("tIns", 3.6), inset("rIns", 7.2), inset("bIns", 3.6)],
        rotation,
    })
}

/// The paragraphs of a text body (`txBody`, also chart titles' `rich`)
pub fn paragraphs(body: Node, theme: &[Rgb], default_color: Rgb) -> Vec<Para> {
    children(body, "p")
        .map(|p| {
            let ppr = child(p, "pPr");
            let def = ppr.and_then(|pp| child(pp, "defRPr"));
            let align = match ppr.and_then(|pp| pp.attribute("algn")) {
                Some("ctr") => "center",
                Some("r") => "right",
                _ => "left",
            };
            let mut runs = vec![];
            for r in p.children().filter(|c| matches!(c.tag_name().name(), "r" | "fld" | "br")) {
                if r.tag_name().name() == "br" {
                    if let Some(last) = runs.last_mut() {
                        let last: &mut Run = last;
                        last.text.push('\n');
                    }
                    continue;
                }
                let text: String = child(r, "t").and_then(|t| t.text()).unwrap_or("").to_string();
                let rpr = child(r, "rPr");
                let get = |name: &str| rpr.and_then(|x| x.attribute(name)).or_else(|| def.and_then(|d| d.attribute(name)));
                let size = get("sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0).unwrap_or(11.0);
                let on = |name: &str| get(name).map(|v| v == "1" || v == "true").unwrap_or(false);
                let color = rpr
                    .and_then(|x| fill(x, theme))
                    .or_else(|| def.and_then(|d| fill(d, theme)))
                    .flatten()
                    .unwrap_or(default_color);
                runs.push(Run { text, size, bold: on("b"), italic: on("i"), underline: get("u").is_some_and(|u| u != "none"), color });
            }
            Para { runs, align }
        })
        .collect()
}

fn rgb(c: Rgb) -> String {
    format!("rgb({}, {}, {})", c[0], c[1], c[2])
}

fn esc(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Typst content for paragraphs, sizes scaled by `s`
pub fn paras_markup(paras: &[Para], s: f32) -> String {
    let mut out = vec![];
    for p in paras {
        let runs: Vec<String> = p
            .runs
            .iter()
            .filter(|r| !r.text.is_empty())
            .map(|r| {
                let mut t = format!("text(size: {:.2}pt, fill: {}", r.size * s, rgb(r.color));
                if r.bold {
                    t.push_str(", weight: \"bold\"");
                }
                if r.italic {
                    t.push_str(", style: \"italic\"");
                }
                t.push_str(&format!(", {})", esc(&r.text)));
                if r.underline { format!("underline({t})") } else { t }
            })
            .collect();
        let body = if runs.is_empty() { "[]".to_string() } else { runs.join(" + ") };
        out.push(format!("align({}, par(leading: 0.3em, {body}))", p.align));
    }
    if out.is_empty() {
        return "[]".into();
    }
    format!("stack(spacing: 0.3em, {})", out.join(", "))
}

/// Typst content for a shape `w` × `h` points (already scaled by `s`)
pub fn shape_markup(sh: &Shape, w: f32, h: f32, s: f32) -> String {
    let fill = sh.fill.map(rgb).unwrap_or_else(|| "none".into());
    let stroke = sh.line.map(|(c, width)| format!("{:.2}pt + {}", (width * s).max(0.25), rgb(c))).unwrap_or_else(|| "none".into());
    let outline = match &sh.geometry {
        Geometry::Rect => format!("rect(width: {w:.2}pt, height: {h:.2}pt, fill: {fill}, stroke: {stroke})"),
        Geometry::RoundRect(r) => format!("rect(width: {w:.2}pt, height: {h:.2}pt, fill: {fill}, stroke: {stroke}, radius: {:.2}pt)", r[0].min(w.min(h) / 2.0)),
        Geometry::Ellipse => format!("ellipse(width: {w:.2}pt, height: {h:.2}pt, fill: {fill}, stroke: {stroke})"),
        Geometry::Paths(subs) => {
            // the preset was built for the unscaled box
            let k = s;
            let pt = |x: f32, y: f32| format!("({:.2}pt, {:.2}pt)", x * k, y * k);
            let line_paint = sh.line.map(|(c, _)| rgb(c)).unwrap_or_else(|| "none".into());
            let parts: Vec<String> = subs
                .iter()
                .filter_map(|sub| {
                    let steps: Vec<String> = sub
                        .commands
                        .iter()
                        .map(|c| match c {
                            PathCommand::Move(x, y) => format!("curve.move({})", pt(*x, *y)),
                            PathCommand::Line(x, y) => format!("curve.line({})", pt(*x, *y)),
                            PathCommand::Cubic([a, b, c, d, e, f]) => format!("curve.cubic({}, {}, {})", pt(*a, *b), pt(*c, *d), pt(*e, *f)),
                            PathCommand::Close => "curve.close(mode: \"straight\")".into(),
                        })
                        .collect();
                    let f = match sub.fill {
                        PathFill::Shape => fill.as_str(),
                        PathFill::Line => line_paint.as_str(),
                        PathFill::None => "none",
                    };
                    let st = if sub.stroke { stroke.as_str() } else { "none" };
                    (!steps.is_empty() && !(f == "none" && st == "none")).then(|| format!("place(top + left, curve(fill: {f}, stroke: {st}, {}))", steps.join(", ")))
                })
                .collect();
            format!("box(width: {w:.2}pt, height: {h:.2}pt, {{ {} }})", parts.join("\n"))
        }
    };
    let has_text = sh.paras.iter().any(|p| p.runs.iter().any(|r| !r.text.trim().is_empty()));
    let mut body = format!("place(top + left, {outline})");
    if has_text {
        let [l, t, r, b] = sh.insets.map(|v| v * s);
        let _ = std::fmt::Write::write_fmt(
            &mut body,
            format_args!(
                " + place(top + left, box(width: {w:.2}pt, height: {h:.2}pt, inset: (left: {l:.2}pt, top: {t:.2}pt, right: {r:.2}pt, bottom: {b:.2}pt), align({}, {})))",
                sh.anchor,
                paras_markup(&sh.paras, s)
            ),
        );
    }
    let content = format!("box(width: {w:.2}pt, height: {h:.2}pt, {body})");
    if sh.rotation.abs() > 0.1 { format!("rotate({:.2}deg, {content})", sh.rotation) } else { content }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAW: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

    #[test]
    fn theme_colours_and_modifiers() {
        let theme = [[0, 0, 0], [255, 255, 255], [0x44, 0x54, 0x6A], [0xE7, 0xE6, 0xE6], [0x44, 0x72, 0xC4]];
        let xml = format!(r#"<a:solidFill xmlns:a="{DRAW}"><a:schemeClr val="accent1"><a:lumMod val="60000"/><a:lumOff val="40000"/></a:schemeClr></a:solidFill>"#);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let c = color(doc.root_element().first_element_child().unwrap(), &theme).unwrap();
        // accent1 "lighter 40%": 8FAADC
        assert!(c.iter().zip([0x8F, 0xAA, 0xDC]).all(|(a, b)| (*a as i32 - b).abs() <= 2), "{c:x?}");
    }
}
