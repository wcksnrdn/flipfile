//! Text: font families and widths, and cell text as Typst markup

use super::*;

/// Font families to try for an Excel font: itself, then metric-compatible and common
/// look-alikes (worked out once per font name: every cell asks)
pub(super) fn families(name: &str) -> std::rc::Rc<Vec<String>> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, std::rc::Rc<Vec<String>>>> = Default::default();
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(name).cloned()) {
        return hit;
    }
    let v = std::rc::Rc::new(family_list(name));
    CACHE.with(|c| c.borrow_mut().insert(name.to_string(), v.clone()));
    v
}

fn family_list(name: &str) -> Vec<String> {
    let mut v = vec![name.to_string()];
    let alt: &[&str] = match name.to_lowercase().as_str() {
        "calibri" | "calibri light" => &["Carlito"],
        "cambria" => &["Caladea"],
        "arial" => &["Liberation Sans", "Arimo"],
        "times new roman" => &["Liberation Serif", "Tinos", "Times"],
        "courier new" => &["Liberation Mono", "Cousine", "Courier"],
        "aptos narrow" => &["Aptos", "Arial Narrow", "Helvetica Neue"],
        "aptos" | "aptos display" => &["Calibri", "Carlito"],
        _ => &[],
    };
    v.extend(alt.iter().map(|s| s.to_string()));
    for common in ["Calibri", "Arial", "Helvetica", "Liberation Sans", "DejaVu Sans"] {
        if !v.iter().any(|f| f.eq_ignore_ascii_case(common)) {
            v.push(common.to_string());
        }
    }
    v
}

/// Width of `text` in the first installed family as Excel measures it (whole pixels per
/// character, see `typeset::text_width_excel`), or an estimate
pub(super) fn width(fams: &[String], size: f32, bold: bool, italic: bool, text: &str) -> f32 {
    let plain: String = text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).collect();
    for f in fams {
        if let Some(w) = crate::office::typeset::text_width_excel(f, size, bold, italic, &plain) {
            return w;
        }
    }
    plain.chars().count() as f32 * size * if bold { 0.56 } else { 0.52 }
}

/// Width of `text` as it is actually drawn (its outlines)
fn outline_width(fams: &[String], size: f32, bold: bool, italic: bool, text: &str) -> f32 {
    let plain: String = text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).collect();
    for f in fams {
        if let Some(w) = crate::office::typeset::text_width(f, size, bold, italic, &plain) {
            return w;
        }
    }
    plain.chars().count() as f32 * size * if bold { 0.56 } else { 0.52 }
}

/// A piece of a cell's text in one font: the whole text, or a run of rich text
#[derive(Clone)]
pub(super) struct Piece {
    pub(super) text: String,
    pub(super) font: Font,
    /// from the number format ([Red]…), over the font's own colour
    pub(super) color: Option<[u8; 3]>,
    pub(super) script: i8,
}

impl Piece {
    pub(super) fn plain(text: String, font: &Font, color: Option<[u8; 3]>) -> Vec<Piece> {
        vec![Piece { text, font: font.clone(), color, script: 0 }]
    }
    pub(super) fn size(&self) -> f32 {
        if self.script != 0 { self.font.size * 0.65 } else { self.font.size }
    }
    fn width(&self) -> f32 {
        width(&families(&self.font.name), self.size(), self.font.bold, self.font.italic, &self.text)
    }
    fn outline(&self) -> f32 {
        outline_width(&families(&self.font.name), self.size(), self.font.bold, self.font.italic, &self.text)
    }
}

pub(super) fn pieces_width(pieces: &[Piece]) -> f32 {
    pieces.iter().map(Piece::width).sum()
}

/// Letter spacing (pt) that draws `pieces` exactly as wide as Excel measures them
pub(super) fn tracking(pieces: &[Piece]) -> f32 {
    tracking_for(pieces, pieces_width(pieces))
}

/// `tracking` when Excel's width of the pieces is known already
pub(super) fn tracking_for(pieces: &[Piece], excel: f32) -> f32 {
    let chars: usize = pieces.iter().map(|p| p.text.chars().filter(|c| *c != HIDE_ON && *c != HIDE_OFF).count()).sum();
    if chars < 2 {
        return 0.0;
    }
    let drawn: f32 = pieces.iter().map(Piece::outline).sum();
    // only ever tighten, and never more than Windows' pixel rounding could
    ((excel - drawn) / chars as f32).clamp(-0.08 * pieces[0].font.size, 0.0)
}

/// The pieces a cell's text is drawn in: its rich text runs, or the formatted text
pub(super) fn cell_pieces(shown: &Shown, style: &Style, rich: Option<&crate::office::xlsx::Rich>, value: &Value) -> Vec<Piece> {
    if let (Some(runs), Value::Text(t)) = (rich, value)
        && shown.text == *t && shown.color.is_none() {
            return runs.iter().filter(|r| !r.text.is_empty()).map(|r| Piece { text: r.text.clone(), font: r.font.over(&style.font), color: None, script: r.font.script }).collect();
        }
    Piece::plain(shown.text.clone(), &style.font, shown.color)
}

/// Height of one line of text, the way Excel sizes rows (15 pt for Calibri 11)
pub(super) fn line_height(fams: &[String], size: f32) -> f32 {
    let (a, d) = crate::office::typeset::line_metrics(fams);
    ((a + d) * size * 1.12).max(size * 1.2)
}

pub(super) fn typst_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(super) fn rgb(c: [u8; 3]) -> String {
    format!("rgb({}, {}, {})", c[0], c[1], c[2])
}

/// Typst's list of `families` for an Excel font name
pub(super) fn font_list(name: &str) -> std::rc::Rc<str> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<String, std::rc::Rc<str>>> = Default::default();
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(name).cloned()) {
        return hit;
    }
    let list: std::rc::Rc<str> = families(name).iter().map(|s| typst_str(s)).collect::<Vec<_>>().join(", ").into();
    CACHE.with(|c| c.borrow_mut().insert(name.to_string(), list.clone()));
    list
}

/// A Typst expression for a cell's text pieces (with hidden padding runs), at `scale`;
/// `shrink` scales the font sizes further (Shrink to fit), `track` is letter spacing in pt.
///
/// `default_font` is the `font_list` the document sets for all text (see `print`), so
/// only cells in another font name theirs: a workbook has tens of thousands of cells, and
/// every byte of markup per cell costs Typst time and memory.
pub(super) fn pieces_expr(pieces: &[Piece], scale: f32, shrink: f32, track: f32, default_font: &str) -> String {
    let mut all = vec![];
    for p in pieces {
        let f = &p.font;
        let fams_list = font_list(&f.name);
        let size = p.size() * shrink * scale;
        let fill = p.color.or(f.color).unwrap_or([0, 0, 0]);
        let mut base = format!("size: {size:.2}pt");
        if &*fams_list != default_font {
            base = format!("font: ({fams_list},), {base}");
        }
        if f.bold {
            base.push_str(", weight: \"bold\"");
        }
        if f.italic {
            base.push_str(", style: \"italic\"");
        }
        if track < -0.005 {
            let _ = write!(base, ", tracking: {:.3}pt", track * shrink * scale);
        }
        if p.script != 0 {
            let _ = write!(base, ", baseline: {:.2}pt", -(p.script as f32) * f.size * 0.33 * shrink * scale);
        }
        let mut parts = vec![];
        let mut cur = String::new();
        let mut hidden = false;
        let flush = |cur: &mut String, hidden: bool, parts: &mut Vec<String>| {
            if cur.is_empty() {
                return;
            }
            let paint = if hidden { "rgb(0, 0, 0, 0)".to_string() } else { rgb(fill) };
            parts.push(format!("text({base}, fill: {paint}, {})", typst_str(cur)));
            cur.clear();
        };
        for c in p.text.chars() {
            match c {
                HIDE_ON => {
                    flush(&mut cur, hidden, &mut parts);
                    hidden = true;
                }
                HIDE_OFF => {
                    flush(&mut cur, hidden, &mut parts);
                    hidden = false;
                }
                c => cur.push(c),
            }
        }
        flush(&mut cur, hidden, &mut parts);
        if parts.is_empty() {
            continue;
        }
        let mut e = parts.join(" + ");
        if f.underline {
            e = format!("underline({e})");
        }
        if f.strike {
            e = format!("strike({e})");
        }
        all.push(e);
    }
    if all.is_empty() {
        return "[]".into();
    }
    all.join(" + ")
}

/// `pieces_expr` for one text in a style
pub(super) fn text_expr(text: &str, style: &Style, color: Option<[u8; 3]>, scale: f32, default_font: &str) -> String {
    pieces_expr(&Piece::plain(text.to_string(), &style.font, color), scale, 1.0, 0.0, default_font)
}
