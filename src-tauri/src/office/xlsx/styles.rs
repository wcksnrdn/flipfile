//! Styles: cell formats, and the differential formats and table styles they build on

use super::*;

pub(super) fn read_styles(xml: &str, colors: &Colors) -> Vec<Style> {
    let Ok(doc) = Document::parse(xml) else { return vec![Style::default()] };
    let root = doc.root_element();
    let mut formats: HashMap<u32, String> = HashMap::new();
    if let Some(nf) = child(root, "numFmts") {
        for f in children(nf, "numFmt") {
            if let (Some(id), Some(code)) = (attr_u32(f, "numFmtId"), f.attribute("formatCode")) {
                formats.insert(id, code.to_string());
            }
        }
    }
    let fonts: Vec<Font> = child(root, "fonts")
        .map(|fs| {
            children(fs, "font")
                .map(|f| {
                    let val = |name: &str| child(f, name);
                    let on = |name: &str| val(name).map(|n| n.attribute("val").map(|v| v != "0" && v != "false").unwrap_or(true)).unwrap_or(false);
                    Font {
                        name: val("name").and_then(|n| n.attribute("val")).unwrap_or("Calibri").to_string(),
                        size: val("sz").and_then(|n| attr_f32(n, "val")).unwrap_or(11.0),
                        bold: on("b"),
                        italic: on("i"),
                        underline: val("u").map(|n| n.attribute("val") != Some("none")).unwrap_or(false),
                        strike: on("strike"),
                        color: val("color").and_then(|c| colors.get(c)),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<Option<Rgb>> = child(root, "fills")
        .map(|fs| {
            children(fs, "fill")
                .map(|f| {
                    // a gradient prints as the blend of its colours
                    if let Some(g) = child(f, "gradientFill") {
                        let stops: Vec<Rgb> = children(g, "stop").filter_map(|s| child(s, "color").and_then(|c| colors.get(c))).collect();
                        if stops.is_empty() {
                            return None;
                        }
                        let n = stops.len() as u32;
                        return Some([0, 1, 2].map(|i| (stops.iter().map(|c| c[i] as u32).sum::<u32>() / n) as u8));
                    }
                    let p = child(f, "patternFill")?;
                    let kind = p.attribute("patternType").unwrap_or("none");
                    if kind == "none" {
                        return None;
                    }
                    let fg = child(p, "fgColor").and_then(|c| colors.get(c));
                    let bg = child(p, "bgColor").and_then(|c| colors.get(c));
                    match kind {
                        "solid" => fg.or(bg),
                        // patterns print as a blend of the two colours
                        _ => {
                            let a = fg.unwrap_or([0, 0, 0]);
                            let b = bg.unwrap_or([255, 255, 255]);
                            let k = match kind {
                                "gray125" => 0.125,
                                "gray0625" => 0.0625,
                                "lightGray" => 0.25,
                                "mediumGray" => 0.5,
                                "darkGray" => 0.75,
                                _ => 0.5,
                            };
                            Some([0, 1, 2].map(|i| (a[i] as f32 * k + b[i] as f32 * (1.0 - k)).round() as u8))
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let borders: Vec<Borders> = child(root, "borders")
        .map(|bs| {
            children(bs, "border")
                .map(|b| {
                    let edge = |name: &str| -> Option<Edge> {
                        let e = child(b, name).or_else(|| child(b, if name == "left" { "start" } else if name == "right" { "end" } else { name }))?;
                        let line = match e.attribute("style")? {
                            "hair" => Line::Hair,
                            "thin" => Line::Thin,
                            "medium" => Line::Medium,
                            "thick" => Line::Thick,
                            "double" => Line::Double,
                            "dotted" => Line::Dotted,
                            "dashed" => Line::Dashed,
                            "mediumDashed" => Line::MediumDashed,
                            "dashDot" | "dashDotDot" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => Line::DashDot,
                            _ => return None,
                        };
                        let color = child(e, "color").and_then(|c| colors.get(c)).unwrap_or([0, 0, 0]);
                        Some(Edge { line, color })
                    };
                    let diagonal = edge("diagonal");
                    Borders {
                        left: edge("left"),
                        right: edge("right"),
                        top: edge("top"),
                        bottom: edge("bottom"),
                        up: diagonal.filter(|_| attr_bool(b, "diagonalUp").unwrap_or(false)),
                        down: diagonal.filter(|_| attr_bool(b, "diagonalDown").unwrap_or(false)),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut styles = vec![];
    if let Some(xfs) = child(root, "cellXfs") {
        for xf in children(xfs, "xf") {
            let fmt_id = attr_u32(xf, "numFmtId").unwrap_or(0);
            let num_fmt = formats.get(&fmt_id).cloned().or_else(|| numfmt::builtin(fmt_id).map(str::to_string)).unwrap_or_else(|| "General".into());
            let mut s = Style {
                num_fmt,
                font: attr_u32(xf, "fontId").and_then(|i| fonts.get(i as usize)).cloned().unwrap_or_default(),
                fill: attr_u32(xf, "fillId").and_then(|i| fills.get(i as usize)).copied().flatten(),
                borders: attr_u32(xf, "borderId").and_then(|i| borders.get(i as usize)).cloned().unwrap_or_default(),
                ..Default::default()
            };
            if let Some(a) = child(xf, "alignment") {
                s.h = match a.attribute("horizontal") {
                    Some("left") => HAlign::Left,
                    Some("center") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    Some("fill") => HAlign::Fill,
                    Some("justify") | Some("distributed") => HAlign::Justify,
                    Some("centerContinuous") => HAlign::CenterContinuous,
                    _ => HAlign::General,
                };
                s.v = match a.attribute("vertical") {
                    Some("top") => VAlign::Top,
                    Some("center") | Some("justify") | Some("distributed") => VAlign::Center,
                    _ => VAlign::Bottom,
                };
                s.wrap = attr_bool(a, "wrapText").unwrap_or(false);
                s.indent = attr_u32(a, "indent").unwrap_or(0);
                s.shrink = attr_bool(a, "shrinkToFit").unwrap_or(false);
                match attr_u32(a, "textRotation").unwrap_or(0) {
                    255 => s.stacked = true,
                    r @ 0..=90 => s.rotation = r as i16,
                    r @ 91..=180 => s.rotation = 90 - r as i16,
                    _ => {}
                }
            }
            styles.push(s);
        }
    }
    if styles.is_empty() {
        styles.push(Style { num_fmt: "General".into(), font: fonts.first().cloned().unwrap_or_default(), ..Default::default() });
    }
    styles
}

/// The differential formats (`<dxfs>`) and custom table styles of styles.xml
pub(super) fn read_dxfs(xml: &str, colors: &Colors) -> (Vec<Dxf>, crate::office::sheet_style::TableStyles) {
    let Ok(doc) = Document::parse(xml) else { return Default::default() };
    let root = doc.root_element();
    let dxfs = child(root, "dxfs")
        .map(|ds| {
            children(ds, "dxf")
                .map(|d| {
                    let mut x = Dxf::default();
                    if let Some(f) = child(d, "font") {
                        let on = |name: &str| child(f, name).map(|n| n.attribute("val").map(|v| v != "0" && v != "false").unwrap_or(true));
                        x.bold = on("b");
                        x.italic = on("i");
                        x.strike = on("strike");
                        x.underline = child(f, "u").map(|n| n.attribute("val") != Some("none"));
                        x.color = child(f, "color").and_then(|c| colors.get(c));
                    }
                    if let Some(p) = child(d, "fill").and_then(|f| child(f, "patternFill"))
                        && p.attribute("patternType") != Some("none") {
                            // a differential fill keeps its solid colour in bgColor
                            x.fill = child(p, "bgColor").and_then(|c| colors.get(c)).or_else(|| child(p, "fgColor").and_then(|c| colors.get(c)));
                        }
                    if let Some(n) = child(d, "numFmt") {
                        x.num_fmt = n.attribute("formatCode").map(str::to_string);
                    }
                    if let Some(b) = child(d, "border") {
                        x.borders = Borders { left: border_edge(b, "left", colors), right: border_edge(b, "right", colors), top: border_edge(b, "top", colors), bottom: border_edge(b, "bottom", colors), ..Default::default() };
                        x.inside_h = border_edge(b, "horizontal", colors);
                        x.inside_v = border_edge(b, "vertical", colors);
                    }
                    x
                })
                .collect()
        })
        .unwrap_or_default();
    let mut styles = HashMap::new();
    if let Some(ts) = child(root, "tableStyles") {
        for t in children(ts, "tableStyle") {
            let Some(name) = t.attribute("name") else { continue };
            let parts = children(t, "tableStyleElement").filter_map(|e| Some((e.attribute("type")?.to_string(), attr_u32(e, "dxfId")? as usize))).collect();
            styles.insert(name.to_string(), parts);
        }
    }
    (dxfs, styles)
}
