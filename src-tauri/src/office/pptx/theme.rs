//! Theme: colours, fonts, fill and line styles

use super::*;

pub(super) struct Theme {
    pub(super) colors: HashMap<String, [u8; 3]>,
    pub(super) major_font: String,
    pub(super) minor_font: String,
    pub(super) fill_styles: Vec<String>,
    pub(super) background_styles: Vec<String>,
    pub(super) line_styles: Vec<String>,
}

impl Theme {
    pub(super) fn parse(xml: &str) -> Theme {
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
pub(super) fn plain_color(node: Node) -> Option<[u8; 3]> {
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
