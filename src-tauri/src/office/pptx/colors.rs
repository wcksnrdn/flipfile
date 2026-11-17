//! Colour resolution with a theme, a colour map and modifiers

use super::*;

#[derive(Clone)]
pub(super) struct Colors<'t> {
    pub(super) theme: &'t Theme,
    /// bg1/tx1/bg2/tx2/… -> theme slot (from the master, or a slide's override)
    pub(super) map: HashMap<String, String>,
    /// phClr: the colour a style reference brings
    pub(super) placeholder: Option<Rgba>,
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
    pub(super) fn color(&self, node: Node) -> Option<Rgba> {
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
    pub(super) fn first(&self, node: Node) -> Option<Rgba> {
        node.children().filter(Node::is_element).find_map(|c| self.color(c))
    }

    pub(super) fn with_placeholder(&self, placeholder: Option<Rgba>) -> Colors<'_> {
        Colors { theme: self.theme, map: self.map.clone(), placeholder }
    }
}

pub(super) fn to_hsl([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
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

pub(super) fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
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
