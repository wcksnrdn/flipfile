//! Colours

use super::*;

/// Excel's legacy 64-colour palette
pub fn indexed_color(i: usize) -> Option<Rgb> {
    const P: [u32; 64] = [
        0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00,
        0xFF00FF, 0x00FFFF, 0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF,
        0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF,
        0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699, 0x969696,
        0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333,
    ];
    P.get(i).map(|v| [(v >> 16) as u8, (v >> 8) as u8, *v as u8])
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    let s = if s.len() == 8 { &s[2..] } else { s };
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Lightens (tint > 0) or darkens (tint < 0) like Excel, in HSL
pub fn tint(c: Rgb, t: f32) -> Rgb {
    if t == 0.0 {
        return c;
    }
    let [r, g, b] = c.map(|v| v as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let mut l = (max + min) / 2.0;
    let (h, s) = if max == min {
        (0.0, 0.0)
    } else {
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
        (h, s)
    };
    l = if t < 0.0 { l * (1.0 + t) } else { l * (1.0 - t) + t };
    let hue = |p: f32, q: f32, mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
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
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue(p, q, h + 1.0 / 3.0), hue(p, q, h), hue(p, q, h - 1.0 / 3.0))
    };
    [r, g, b].map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
}

pub(super) struct Colors {
    pub(super) theme: Vec<Rgb>,
    pub(super) indexed: Option<Vec<Rgb>>,
}

impl Colors {
    /// `<color rgb|theme|indexed tint>`; None for "automatic"
    pub(super) fn get(&self, n: Node) -> Option<Rgb> {
        let t = attr_f32(n, "tint").unwrap_or(0.0);
        let base = if let Some(rgb) = n.attribute("rgb") {
            hex(rgb)
        } else if let Some(th) = attr_u32(n, "theme") {
            // Excel swaps the first two pairs: 0 = lt1, 1 = dk1, 2 = lt2, 3 = dk2
            let i = match th {
                0 => 1,
                1 => 0,
                2 => 3,
                3 => 2,
                i => i as usize as u32,
            } as usize;
            self.theme.get(i).copied()
        } else if let Some(ix) = attr_u32(n, "indexed") {
            if ix == 64 {
                return None; // system foreground: automatic
            }
            self.indexed.as_ref().and_then(|p| p.get(ix as usize).copied()).or_else(|| indexed_color(ix as usize))
        } else {
            None
        };
        base.map(|c| tint(c, t))
    }
}

pub(super) fn theme_colors(xml: &str) -> Vec<Rgb> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    let Some(scheme) = doc.descendants().find(|n| n.has_tag_name((DRAW, "clrScheme"))) else { return vec![] };
    let mut out = vec![];
    for name in ["dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"] {
        let c = scheme
            .children()
            .find(|c| c.tag_name().name() == name)
            .and_then(|e| e.children().find(|c| c.is_element()))
            .and_then(|c| c.attribute("lastClr").or(c.attribute("val")))
            .and_then(hex)
            .unwrap_or([0, 0, 0]);
        out.push(c);
    }
    out
}

pub(super) fn default_theme() -> Vec<Rgb> {
    // Office 2013+ theme
    ["000000", "FFFFFF", "44546A", "E7E6E6", "4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47", "0563C1", "954F72"]
        .iter()
        .filter_map(|h| hex(h))
        .collect()
}

pub(super) fn custom_palette(styles_xml: &str) -> Option<Vec<Rgb>> {
    let doc = Document::parse(styles_xml).ok()?;
    let ic = doc.descendants().find(|n| n.has_tag_name((MAIN, "indexedColors")))?;
    Some(children(ic, "rgbColor").filter_map(|c| c.attribute("rgb").and_then(hex)).collect())
}
