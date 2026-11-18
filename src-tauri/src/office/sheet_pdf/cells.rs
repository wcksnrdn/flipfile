//! Drawing a cell: its box, text (overflow, ####, wrapping), icons, borders and pictures

use super::*;

/// Width and height of a cell (or of its merged region, limited to what this page shows)
pub(super) fn cell_box(grid: &Grid, merged: &HashMap<(u32, u32), Range>, r: u32, c: u32, rows: &HashSet<u32>, cols: (u32, u32)) -> (f32, f32) {
    match merged.get(&(r, c)) {
        Some(m) => {
            let w: f32 = (m.1..=m.3).filter(|cc| *cc >= cols.0 && *cc <= cols.1 && *cc <= grid.range.3).map(|cc| grid.w(cc)).sum();
            let h: f32 = (m.0..=m.2).filter(|rr| rows.contains(rr) && *rr <= grid.range.2).map(|rr| grid.h(rr)).sum();
            (w, h)
        }
        None => (grid.w(c), grid.h(r)),
    }
}

/// A cell's text block: clipped to the cell (or its overflow room), aligned like Excel
#[allow(clippy::too_many_arguments)]
pub(super) fn cell_text(shown: &Shown, pieces: Vec<Piece>, style: &Style, h: HAlign, is_number: bool, w: f32, ch: f32, s: f32, default_font: &str) -> String {
    let fams = families(&style.font.name);
    let indent = style.indent as f32 * 9.0;
    let inner = (w - 2.0 * PAD - indent).max(1.0);
    let v = match style.v {
        VAlign::Top => "top",
        VAlign::Center => "horizon",
        VAlign::Bottom => "bottom",
    };
    let block = |content: String, align: &str| {
        let (il, ir) = match h {
            HAlign::Left | HAlign::General | HAlign::Justify => (PAD + indent, PAD),
            HAlign::Right => (PAD, PAD + indent),
            _ => (PAD, PAD),
        };
        if (il, ir) == (PAD, PAD) {
            // the usual cell: `cell`, defined once per sheet (see `print`)
            return format!("cell({:.2}pt, {:.2}pt, align({align} + {v}, {content}))", w * s, ch * s);
        }
        format!(
            "block(width: {:.2}pt, height: {:.2}pt, clip: true, inset: (left: {:.2}pt, right: {:.2}pt, top: 0.5pt, bottom: 1pt), align({align} + {v}, {content}))",
            w * s,
            ch * s,
            il * s,
            ir * s
        )
    };
    let text_w = |t: &str| width(&fams, style.font.size, style.font.bold, style.font.italic, t);
    let hashes_expr = || pieces_expr(&Piece::plain("#".repeat(hashes(&fams, style, inner)), &style.font, shown.color), s, 1.0, 0.0, default_font);

    // accounting-style formats: the part before the fill hugs the left, the rest the right
    if let (Some(_), Some(after)) = (shown.fill, &shown.after_fill) {
        if text_w(&shown.text) + text_w(after) > inner + 0.5 && is_number {
            return block(hashes_expr(), "right");
        }
        let left = Piece::plain(shown.text.clone(), &style.font, shown.color);
        let right = Piece::plain(after.clone(), &style.font, shown.color);
        let (left_t, right_t) = (tracking(&left), tracking(&right));
        let (left, right) = (pieces_expr(&left, s, 1.0, left_t, default_font), pieces_expr(&right, s, 1.0, right_t, default_font));
        return block(format!("box(width: 100%, {left} + h(1fr) + {right})"), "left");
    }

    let align = match h {
        HAlign::Right => "right",
        HAlign::Center | HAlign::CenterContinuous => "center",
        _ => "left",
    };
    if style.stacked {
        // one letter per line, centred
        let stacked: String = shown.text.chars().filter(|c| *c != '\n').map(|c| c.to_string()).collect::<Vec<_>>().join("\n");
        let t = pieces_expr(&Piece::plain(stacked, &style.font, shown.color), s, 1.0, 0.0, default_font);
        return block(format!("par(leading: 0.14em, {t})"), "center");
    }
    if style.rotation != 0 {
        let pieces: Vec<Piece> = pieces.into_iter().map(|mut p| {
            p.text = p.text.replace('\n', " ");
            p
        }).collect();
        let t = pieces_expr(&pieces, s, 1.0, tracking(&pieces), default_font);
        let tw = pieces_width(&pieces) + 1.0;
        // anticlockwise in Excel, clockwise in Typst
        let content = format!("rotate({}deg, reflow: true, box(width: {:.2}pt, {t}))", -style.rotation, tw * s);
        // steep text stays in its cell; slanted text reaches over its neighbours like Excel's
        if style.rotation.abs() == 90 {
            return block(content, align);
        }
        return format!("box(width: {:.2}pt, height: {:.2}pt, inset: (x: {:.2}pt), align({align} + {v}, {content}))", w * s, ch * s, PAD * s);
    }

    let mut pieces = pieces;
    let mut shrink = 1.0;
    if !style.wrap {
        for p in &mut pieces {
            if p.text.contains('\n') {
                p.text = p.text.replace('\n', " ");
            }
        }
    }
    let mut tw = pieces_width(&pieces);
    if !style.wrap
        && tw > inner + 0.5 {
            if style.shrink {
                shrink = (inner / tw).max(4.0 / style.font.size);
            } else if is_number {
                // numbers never spill: Excel shows #### instead
                return block(hashes_expr(), align);
            }
        }
    if h == HAlign::Fill && pieces.len() == 1 && !pieces[0].text.is_empty() {
        let n = ((inner / tw.max(0.1)).floor() as usize).max(1);
        pieces[0].text = pieces[0].text.repeat(n);
        tw *= n as f32;
    }
    let track = tracking_for(&pieces, tw);
    let t = pieces_expr(&pieces, s, shrink, track, default_font);
    if style.wrap {
        block(format!("par(justify: {}, leading: 0.14em, {t})", h == HAlign::Justify), align)
    } else {
        // a box as wide as the text never wraps; what doesn't fit is clipped at the cell edge
        block(format!("box(width: {:.2}pt, {t})", (tw * shrink + 1.0) * s), align)
    }
}

/// An icon of a conditional format's icon set, `size` points square; `index` 0 stands for
/// the lowest values
pub(super) fn icon(set: &str, index: usize, of: usize, size: f32) -> String {
    const RED: &str = "rgb(229, 57, 53)";
    const YELLOW: &str = "rgb(255, 192, 0)";
    const GREEN: &str = "rgb(0, 166, 81)";
    const GRAY: &str = "rgb(128, 128, 128)";
    const BLACK: &str = "rgb(64, 64, 64)";
    let k = size;
    let poly = |pts: &[(f32, f32)], fill: &str| {
        let pts: Vec<String> = pts.iter().map(|(x, y)| format!("({:.2}pt, {:.2}pt)", x * k, y * k)).collect();
        format!("polygon(fill: {fill}, {})", pts.join(", "))
    };
    let boxed = |inner: String| format!("box(width: {k:.2}pt, height: {k:.2}pt, {inner})");
    let circle = |fill: &str| boxed(format!("align(center + horizon, circle(radius: {:.2}pt, fill: {fill}))", k * 0.42));
    let up = [(0.5, 0.05), (0.95, 0.5), (0.68, 0.5), (0.68, 0.95), (0.32, 0.95), (0.32, 0.5), (0.05, 0.5)];
    let arrow = |deg: i32, fill: &str| boxed(format!("rotate({deg}deg, {})", poly(&up, fill)));
    // the position of `index` among `of` icons, 0.0 (lowest) to 1.0 (highest)
    let level = if of > 1 { index as f32 / (of - 1) as f32 } else { 1.0 };
    let traffic = |i: usize| match (of, i) {
        (4, 0) => BLACK,
        (_, 0) => RED,
        (n, i) if i + 1 == n => GREEN,
        _ => YELLOW,
    };
    let base = set.trim_start_matches(|c: char| c.is_ascii_digit());
    match base {
        "Arrows" | "ArrowsGray" => {
            let gray = base == "ArrowsGray";
            let deg = (180.0 - 180.0 * level).round() as i32;
            let fill = if gray { GRAY } else if index == 0 { RED } else if index + 1 == of { GREEN } else { YELLOW };
            arrow(deg, fill)
        }
        "Triangles" => match index {
            0 => boxed(poly(&[(0.1, 0.25), (0.9, 0.25), (0.5, 0.85)], RED)),
            i if i + 1 == of => boxed(poly(&[(0.5, 0.15), (0.9, 0.75), (0.1, 0.75)], GREEN)),
            _ => boxed(poly(&[(0.15, 0.4), (0.85, 0.4), (0.85, 0.6), (0.15, 0.6)], YELLOW)),
        },
        "Signs" => match index {
            0 => boxed(poly(&[(0.5, 0.05), (0.95, 0.5), (0.5, 0.95), (0.05, 0.5)], RED)),
            1 if of == 3 => boxed(poly(&[(0.5, 0.08), (0.95, 0.9), (0.05, 0.9)], YELLOW)),
            _ => circle(GREEN),
        },
        "Symbols" | "Symbols2" => {
            let (fill, mark) = match index {
                0 => (RED, "✕"),
                i if i + 1 == of => (GREEN, "✓"),
                _ => (YELLOW, "!"),
            };
            boxed(format!(
                "align(center + horizon, circle(radius: {:.2}pt, fill: {fill}, inset: 0pt, align(center + horizon, text(size: {:.2}pt, fill: white, weight: \"bold\", \"{mark}\"))))",
                k * 0.45,
                k * 0.62
            ))
        }
        "Flags" => {
            let fill = traffic(index);
            boxed(format!("{} + place(top + left, {})", poly(&[(0.2, 0.05), (0.28, 0.05), (0.28, 0.95), (0.2, 0.95)], BLACK), poly(&[(0.28, 0.08), (0.9, 0.3), (0.28, 0.52)], fill)))
        }
        "Stars" => {
            let star: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
                    let r = if i % 2 == 0 { 0.48 } else { 0.2 };
                    (0.5 + r * a.cos(), 0.52 + r * a.sin())
                })
                .collect();
            let fill = match index {
                0 => "rgb(255, 255, 255)",
                i if i + 1 == of => YELLOW,
                _ => "rgb(255, 224, 140)",
            };
            let pts: Vec<String> = star.iter().map(|(x, y)| format!("({:.2}pt, {:.2}pt)", x * k, y * k)).collect();
            boxed(format!("polygon(fill: {fill}, stroke: 0.5pt + rgb(214, 160, 0), {})", pts.join(", ")))
        }
        "Rating" | "Boxes" => {
            // bars (or boxes) lit up to the level
            let n = if base == "Boxes" { 4 } else { of - 1 };
            let lit = (level * n as f32).round() as usize;
            let mut parts = vec![];
            for i in 0..n {
                let fill = if i < lit { "rgb(31, 78, 121)" } else { "rgb(200, 200, 200)" };
                let (x, w, top) = if base == "Boxes" {
                    (0.05 + (i % 2) as f32 * 0.47, 0.42, 0.05 + (i / 2) as f32 * 0.47)
                } else {
                    let w = 0.8 / n as f32;
                    (0.1 + i as f32 * w, w * 0.8, 0.9 - 0.8 * (i + 1) as f32 / n as f32)
                };
                let bottom = if base == "Boxes" { top + 0.42 } else { 0.9 };
                parts.push(format!("place(top + left, {})", poly(&[(x, top), (x + w, top), (x + w, bottom), (x, bottom)], fill)));
            }
            boxed(parts.join(" + "))
        }
        "Quarters" => {
            let shade = (255.0 - 200.0 * level) as u8;
            boxed(format!("align(center + horizon, circle(radius: {:.2}pt, fill: rgb({shade}, {shade}, {shade}), stroke: 0.6pt + {BLACK}))", k * 0.4))
        }
        "RedToBlack" => {
            let fill = ["rgb(64, 64, 64)", "rgb(160, 160, 160)", "rgb(255, 150, 150)", "rgb(229, 57, 53)"];
            circle(fill[index.min(3)])
        }
        // TrafficLights1/2 and anything unknown: coloured circles
        _ => circle(traffic(index)),
    }
}

fn hashes(fams: &[String], style: &Style, room: f32) -> usize {
    let w = width(fams, style.font.size, style.font.bold, false, "#").max(1.0);
    ((room / w).floor() as usize).max(1)
}

pub(super) fn draw_edge(out: &mut String, e: Edge, x1: f32, y1: f32, x2: f32, y2: f32, s: f32) {
    let (thick, dash) = match e.line {
        Line::Hair => (0.25, "none"),
        Line::Thin => (0.75, "none"),
        Line::Medium => (1.5, "none"),
        Line::Thick => (2.25, "none"),
        Line::Double => (0.6, "none"),
        Line::Dotted => (0.75, "\"dotted\""),
        Line::Dashed => (0.75, "\"dashed\""),
        Line::MediumDashed => (1.5, "\"dashed\""),
        Line::DashDot => (0.75, "\"dash-dotted\""),
    };
    // lines shrink with the sheet, down to the thinnest line that still prints clearly
    let t = (thick * s).max(0.3);
    let stroke = format!("(paint: {}, thickness: {t:.2}pt, dash: {dash})", rgb(e.color));
    if e.line == Line::Double {
        // two thin lines, one each side of the edge
        let d = 0.9;
        let (dx, dy) = if x1 == x2 { (d, 0.0) } else { (0.0, d) };
        for k in [-1.0, 1.0] {
            let _ = writeln!(out, "#place(top + left, line(start: ({:.2}pt, {:.2}pt), end: ({:.2}pt, {:.2}pt), stroke: {stroke}))", x1 + dx * k, y1 + dy * k, x2 + dx * k, y2 + dy * k);
        }
    } else {
        let _ = writeln!(out, "#place(top + left, line(start: ({x1:.2}pt, {y1:.2}pt), end: ({x2:.2}pt, {y2:.2}pt), stroke: {stroke}))");
    }
}

/// Typst reads PNG, JPEG and GIF; anything else is converted to PNG
pub(super) fn picture_bytes(data: &[u8], ext: &str) -> (Vec<u8>, &'static str) {
    match ext {
        "png" => (data.to_vec(), "png"),
        "jpg" | "jpeg" => (data.to_vec(), "jpg"),
        "gif" => (data.to_vec(), "gif"),
        _ => {
            let mut png = vec![];
            if let Ok(img) = image::load_from_memory(data) {
                let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            }
            (png, "png")
        }
    }
}
