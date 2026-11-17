
use super::*;

// Parsing helpers

pub(super) fn w<'a>(node: Node<'a, 'a>, name: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|c| c.is_element() && c.tag_name().name() == name && c.tag_name().namespace() == Some(W))
}

pub(super) fn wattr<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attribute((W, name))
}

pub(super) fn val<'a>(node: Node<'a, 'a>) -> Option<&'a str> {
    wattr(node, "val")
}

/// `<w:b/>`, `<w:b w:val="1"/>`, `<w:b w:val="false"/>`…
pub(super) fn toggle(node: Node) -> bool {
    !matches!(val(node), Some("0" | "false" | "off" | "none"))
}

pub(super) fn twips(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().map(|v| v / 20.0)
}

pub(super) fn hex_color(s: &str) -> Option<[u8; 3]> {
    if s.len() != 6 || s.eq_ignore_ascii_case("auto") {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn highlight_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "yellow" => [255, 255, 0],
        "green" => [0, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "blue" => [0, 0, 255],
        "red" => [255, 0, 0],
        "darkBlue" => [0, 0, 139],
        "darkCyan" => [0, 139, 139],
        "darkGreen" => [0, 100, 0],
        "darkMagenta" => [139, 0, 139],
        "darkRed" => [139, 0, 0],
        "darkYellow" => [128, 128, 0],
        "darkGray" => [169, 169, 169],
        "lightGray" => [211, 211, 211],
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        _ => return None,
    })
}

pub(super) fn parse_rpr(rpr: Node) -> RunPatch {
    let mut p = RunPatch::default();
    for c in rpr.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match c.tag_name().name() {
            "rFonts" => {
                if let Some(t) = wattr(c, "asciiTheme").or(wattr(c, "hAnsiTheme")) {
                    p.font_theme = Some(t.to_string());
                    p.font = None;
                // Only the Latin font slots: `cs` (complex scripts) and `eastAsia` must not
                // replace the font used for Latin text
                } else if let Some(f) = wattr(c, "ascii").or(wattr(c, "hAnsi")) {
                    p.font = Some(f.to_string());
                    p.font_theme = None;
                }
            }
            "sz" => p.size = val(c).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 2.0),
            "b" => p.bold = Some(toggle(c)),
            "i" => p.italic = Some(toggle(c)),
            "u" => p.underline = Some(!matches!(val(c), Some("none" | "0" | "false"))),
            "strike" | "dstrike" => p.strike = Some(toggle(c)),
            "color" => p.color = Some(val(c).and_then(hex_color)),
            "highlight" => p.highlight = Some(val(c).and_then(highlight_color)),
            "shd" => {
                if let Some(fill) = wattr(c, "fill").and_then(hex_color) {
                    p.highlight = Some(Some(fill));
                }
            }
            "vertAlign" => {
                p.vert = Some(match val(c) {
                    Some("superscript") => VertAlign::Super,
                    Some("subscript") => VertAlign::Sub,
                    _ => VertAlign::Baseline,
                })
            }
            "caps" => p.caps = Some(toggle(c)),
            "smallCaps" => p.small_caps = Some(toggle(c)),
            "vanish" => p.hidden = Some(toggle(c)),
            "spacing" => p.spacing = val(c).and_then(twips),
            _ => {}
        }
    }
    p
}

pub(super) fn parse_ppr(ppr: Node) -> ParaPatch {
    let mut p = ParaPatch::default();
    for c in ppr.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match c.tag_name().name() {
            "jc" => {
                p.align = Some(match val(c) {
                    Some("center") => Align::Center,
                    Some("right" | "end") => Align::Right,
                    Some("both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida") => Align::Justify,
                    _ => Align::Left,
                })
            }
            "spacing" => {
                if let Some(v) = wattr(c, "before").and_then(twips) {
                    p.space_before = Some(v);
                }
                if let Some(v) = wattr(c, "after").and_then(twips) {
                    p.space_after = Some(v);
                }
                if let Some(line) = wattr(c, "line").and_then(|v| v.parse::<f32>().ok()) {
                    p.line = Some(match wattr(c, "lineRule") {
                        Some("exact") => LineSpacing::Exact(line / 20.0),
                        Some("atLeast") => LineSpacing::AtLeast(line / 20.0),
                        _ => LineSpacing::Multiple(line / 240.0),
                    });
                }
            }
            "ind" => {
                if let Some(v) = wattr(c, "left").or(wattr(c, "start")).and_then(twips) {
                    p.indent_left = Some(v);
                }
                if let Some(v) = wattr(c, "right").or(wattr(c, "end")).and_then(twips) {
                    p.indent_right = Some(v);
                }
                if let Some(v) = wattr(c, "hanging").and_then(twips) {
                    p.first_line = Some(-v);
                } else if let Some(v) = wattr(c, "firstLine").and_then(twips) {
                    p.first_line = Some(v);
                }
            }
            "tabs" => {
                for t in c.children().filter(|t| t.tag_name().name() == "tab") {
                    let Some(pos) = wattr(t, "pos").and_then(twips) else { continue };
                    let align = match val(t) {
                        Some("center") => TabAlign::Center,
                        Some("right" | "end") => TabAlign::Right,
                        Some("decimal") => TabAlign::Decimal,
                        _ => TabAlign::Left,
                    };
                    let leader = match wattr(t, "leader") {
                        Some("dot") => Some('.'),
                        Some("hyphen") => Some('-'),
                        Some("underscore" | "heavy") => Some('_'),
                        Some("middleDot") => Some('·'),
                        _ => None,
                    };
                    let cleared = val(t) == Some("clear");
                    p.tabs.push((TabStop { pos, align, leader }, cleared));
                }
            }
            "pageBreakBefore" => p.page_break_before = Some(toggle(c)),
            "contextualSpacing" => p.contextual_spacing = Some(toggle(c)),
            "keepNext" => p.keep_with_next = Some(toggle(c)),
            "shd" => p.fill = Some(wattr(c, "fill").and_then(hex_color)),
            "numPr" => {
                let id = w(c, "numId").and_then(val).map(str::to_string);
                let level = w(c, "ilvl").and_then(val).and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(id) = id {
                    p.num = Some((id, level));
                }
            }
            _ => {}
        }
    }
    p
}

/// One `<w:top w:val="single" w:sz="4" w:color="000000"/>`-style border.
pub(super) fn border(node: Node) -> Option<Border> {
    match val(node) {
        None | Some("nil" | "none") => None,
        Some(_) => {
            // `w:sz` is in eighths of a point; Word draws at least a hairline
            let width = wattr(node, "sz").and_then(|v| v.parse::<f32>().ok()).map(|v| (v / 8.0).max(0.25)).unwrap_or(0.5);
            let color = wattr(node, "color").and_then(hex_color).unwrap_or([0, 0, 0]);
            Some(Border { width, color })
        }
    }
}

pub(super) fn table_borders(node: Node) -> TableBorders {
    let side = |names: &[&str]| names.iter().find_map(|n| w(node, n)).and_then(border);
    TableBorders {
        top: side(&["top"]),
        left: side(&["left", "start"]),
        bottom: side(&["bottom"]),
        right: side(&["right", "end"]),
        inside_h: side(&["insideH"]),
        inside_v: side(&["insideV"]),
    }
}

pub(super) fn cell_borders(node: Node) -> CellBorders {
    let side = |names: &[&str]| names.iter().find_map(|n| w(node, n)).map(border);
    CellBorders { top: side(&["top"]), left: side(&["left", "start"]), bottom: side(&["bottom"]), right: side(&["right", "end"]) }
}

// ---------------------------------------------------------------------------
