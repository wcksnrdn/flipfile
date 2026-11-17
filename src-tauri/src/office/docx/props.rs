
use super::*;

// Partial properties (as written in XML) and their resolution

#[derive(Clone, Debug, Default)]
pub(super) struct RunPatch {
    pub(super) font: Option<String>,
    pub(super) font_theme: Option<String>,
    pub(super) size: Option<f32>,
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) underline: Option<bool>,
    pub(super) strike: Option<bool>,
    pub(super) color: Option<Option<[u8; 3]>>,
    pub(super) highlight: Option<Option<[u8; 3]>>,
    pub(super) vert: Option<VertAlign>,
    pub(super) caps: Option<bool>,
    pub(super) small_caps: Option<bool>,
    pub(super) hidden: Option<bool>,
    pub(super) spacing: Option<f32>,
}

impl RunPatch {
    pub(super) fn merge(&mut self, other: &RunPatch) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f.clone(); } )* };
        }
        take!(size, bold, italic, underline, strike, color, highlight, vert, caps, small_caps, hidden, spacing);
        if other.font.is_some() || other.font_theme.is_some() {
            self.font = other.font.clone();
            self.font_theme = other.font_theme.clone();
        }
    }

    pub(super) fn apply(&self, base: &RunProps, theme: &Theme) -> RunProps {
        let mut r = base.clone();
        if let Some(theme_font) = &self.font_theme {
            if let Some(f) = theme.resolve(theme_font) {
                r.font = Some(f);
            }
        } else if let Some(f) = &self.font {
            r.font = Some(f.clone());
        }
        if let Some(v) = self.size {
            r.size = v;
        }
        if let Some(v) = self.spacing {
            r.spacing = v;
        }
        if let Some(v) = self.bold {
            r.bold = v;
        }
        if let Some(v) = self.italic {
            r.italic = v;
        }
        if let Some(v) = self.underline {
            r.underline = v;
        }
        if let Some(v) = self.strike {
            r.strike = v;
        }
        if let Some(v) = self.color {
            r.color = v;
        }
        if let Some(v) = self.highlight {
            r.highlight = v;
        }
        if let Some(v) = self.vert {
            r.vert = v;
        }
        if let Some(v) = self.caps {
            r.caps = v;
        }
        if let Some(v) = self.small_caps {
            r.small_caps = v;
        }
        if let Some(v) = self.hidden {
            r.hidden = v;
        }
        r
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct ParaPatch {
    pub(super) align: Option<Align>,
    pub(super) space_before: Option<f32>,
    pub(super) space_after: Option<f32>,
    pub(super) line: Option<LineSpacing>,
    pub(super) indent_left: Option<f32>,
    pub(super) indent_right: Option<f32>,
    pub(super) first_line: Option<f32>,
    pub(super) page_break_before: Option<bool>,
    pub(super) fill: Option<Option<[u8; 3]>>,
    pub(super) contextual_spacing: Option<bool>,
    pub(super) keep_with_next: Option<bool>,
    /// (stop, cleared): stops add up through the style chain; `clear` removes one
    pub(super) tabs: Vec<(TabStop, bool)>,
    pub(super) num: Option<(String, usize)>, // numId, level
}

impl ParaPatch {
    pub(super) fn merge(&mut self, o: &ParaPatch) {
        macro_rules! take {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        take!(
            align,
            space_before,
            space_after,
            line,
            indent_left,
            indent_right,
            first_line,
            page_break_before,
            fill,
            contextual_spacing,
            keep_with_next,
            num
        );
        self.tabs.extend(o.tabs.iter().cloned());
    }

    pub(super) fn apply(&self, base: &ParaProps) -> ParaProps {
        let mut p = base.clone();
        if let Some(v) = self.align {
            p.align = v;
        }
        if let Some(v) = self.space_before {
            p.space_before = v;
        }
        if let Some(v) = self.space_after {
            p.space_after = v;
        }
        if let Some(v) = self.line {
            p.line = v;
        }
        if let Some(v) = self.indent_left {
            p.indent_left = v;
        }
        if let Some(v) = self.indent_right {
            p.indent_right = v;
        }
        if let Some(v) = self.first_line {
            p.first_line = v;
        }
        if let Some(v) = self.page_break_before {
            p.page_break_before = v;
        }
        if let Some(v) = self.fill {
            p.fill = v;
        }
        if let Some(v) = self.contextual_spacing {
            p.contextual_spacing = v;
        }
        if let Some(v) = self.keep_with_next {
            p.keep_with_next = v;
        }
        let mut stops: Vec<TabStop> = vec![];
        for (stop, cleared) in &self.tabs {
            stops.retain(|t| (t.pos - stop.pos).abs() > 0.5);
            if !cleared {
                stops.push(*stop);
            }
        }
        stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
        p.tabs = stops;
        p
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Style {
    pub(super) kind: String,
    pub(super) name: String,
    pub(super) based_on: Option<String>,
    pub(super) run: RunPatch,
    pub(super) para: ParaPatch,
    pub(super) table_borders: Option<TableBorders>,
    /// Table styles: default cell padding by side (top, left, bottom, right)
    pub(super) cell_margins: HashMap<&'static str, f32>,
}

#[derive(Default)]
pub(super) struct Theme {
    pub(super) major: Option<String>,
    pub(super) minor: Option<String>,
}

impl Theme {
    fn resolve(&self, name: &str) -> Option<String> {
        if name.starts_with("major") {
            self.major.clone()
        } else if name.starts_with("minor") {
            self.minor.clone()
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Level {
    pub(super) start: u32,
    pub(super) format: String,
    pub(super) text: String,
    pub(super) para: ParaPatch,
    pub(super) run: RunPatch,
    pub(super) suffix: MarkerSuffix,
}

pub(super) fn parse_level(l: Node) -> Level {
    Level {
        start: w(l, "start").and_then(val).and_then(|v| v.parse().ok()).unwrap_or(1),
        format: w(l, "numFmt").and_then(val).unwrap_or("decimal").to_string(),
        text: w(l, "lvlText").and_then(val).unwrap_or("").to_string(),
        para: w(l, "pPr").map(parse_ppr).unwrap_or_default(),
        run: w(l, "rPr").map(parse_rpr).unwrap_or_default(),
        suffix: match w(l, "suff").and_then(val) {
            Some("space") => MarkerSuffix::Space,
            Some("nothing") => MarkerSuffix::Nothing,
            _ => MarkerSuffix::Tab,
        },
    }
}

#[derive(Default)]
pub(super) struct Numbering {
    // abstractNumId -> levels
    pub(super) abstracts: HashMap<String, Vec<Level>>,
    // numId -> its definition
    pub(super) nums: HashMap<String, NumDef>,
}

/// A `<w:num>`: (abstractNumId, start overrides per level, whole-level overrides)
type NumDef = (String, HashMap<usize, u32>, HashMap<usize, Level>);

// ---------------------------------------------------------------------------
