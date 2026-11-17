//! Text styles: nine levels of paragraph properties with a default run

use super::*;

#[derive(Clone, Debug, Default)]
pub(super) struct RunStyle {
    pub(super) size: Option<f32>,
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    pub(super) underline: Option<bool>,
    pub(super) strike: Option<bool>,
    pub(super) color: Option<Rgba>,
    pub(super) font: Option<String>,
    pub(super) baseline: Option<f32>,
    /// Smallest size that is kerned
    pub(super) kern: Option<f32>,
    pub(super) spacing: Option<f32>,
    pub(super) highlight: Option<Rgba>,
    pub(super) caps: Option<Caps>,
}

impl RunStyle {
    pub(super) fn parse(node: Node, colors: &Colors, theme: &Theme) -> RunStyle {
        RunStyle {
            size: num(node, "sz").map(|v| (v / 100.0).clamp(1.0, 4000.0)),
            bold: flag(node, "b"),
            italic: flag(node, "i"),
            underline: node.attribute("u").map(|u| u != "none"),
            strike: node.attribute("strike").map(|s| s != "noStrike"),
            color: child(node, "solidFill").and_then(|f| colors.first(f)).or_else(|| {
                // gradient text: its first colour
                child(node, "gradFill").and_then(|g| path(g, &["gsLst", "gs"])).and_then(|gs| colors.first(gs))
            }),
            font: child(node, "latin").and_then(|l| l.attribute("typeface")).map(|t| theme_font(t, theme)),
            baseline: num(node, "baseline").map(|v| v / 100_000.0),
            kern: num(node, "kern").map(|v| v / 100.0),
            spacing: num(node, "spc").map(|v| v / 100.0),
            highlight: child(node, "highlight").and_then(|h| colors.first(h)),
            caps: node.attribute("cap").map(|c| match c {
                "all" => Caps::All,
                "small" => Caps::Small,
                _ => Caps::None,
            }),
        }
    }

    pub(super) fn over(&mut self, top: &RunStyle) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(size, bold, italic, underline, strike, color, font, baseline, kern, spacing, highlight, caps);
    }
}

fn theme_font(face: &str, theme: &Theme) -> String {
    match face {
        f if f.starts_with("+mj") => theme.major_font.clone(),
        f if f.starts_with("+mn") => theme.minor_font.clone(),
        f => f.to_string(),
    }
}

#[derive(Clone, Debug)]
pub(super) enum BulletStyle {
    None,
    Char(String),
    Number(String, usize),
}

#[derive(Clone, Debug, Default)]
pub(super) struct LevelStyle {
    pub(super) margin_left: Option<f32>,
    pub(super) indent: Option<f32>,
    pub(super) align: Option<Align>,
    pub(super) space_before: Option<Spacing>,
    pub(super) space_after: Option<Spacing>,
    pub(super) line_spacing: Option<LineSpacing>,
    pub(super) bullet: Option<BulletStyle>,
    pub(super) bullet_font: Option<String>,
    pub(super) bullet_color: Option<Rgba>,
    pub(super) bullet_scale: Option<f32>,
    pub(super) bullet_points: Option<f32>,
    pub(super) run: RunStyle,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Spacing {
    Points(f32),
    /// Of a line (the font size × 1.2)
    Percent(f32),
}

pub(super) fn spacing(node: Node) -> Option<Spacing> {
    if let Some(p) = child(node, "spcPts") {
        return num(p, "val").map(|v| Spacing::Points(v / 100.0));
    }
    child(node, "spcPct").and_then(|p| num(p, "val")).map(|v| Spacing::Percent(v / 100_000.0))
}

impl LevelStyle {
    pub(super) fn parse(node: Node, colors: &Colors, theme: &Theme) -> LevelStyle {
        let mut s = LevelStyle {
            margin_left: emu(node, "marL"),
            indent: emu(node, "indent"),
            align: node.attribute("algn").map(|a| match a {
                "ctr" => Align::Center,
                "r" => Align::Right,
                "just" | "dist" | "justLow" | "thaiDist" => Align::Justify,
                _ => Align::Left,
            }),
            space_before: child(node, "spcBef").and_then(spacing),
            space_after: child(node, "spcAft").and_then(spacing),
            line_spacing: child(node, "lnSpc").and_then(|l| {
                if let Some(p) = child(l, "spcPct") {
                    num(p, "val").map(|v| LineSpacing::Multiple(v / 100_000.0))
                } else {
                    child(l, "spcPts").and_then(|p| num(p, "val")).map(|v| LineSpacing::Points(v / 100.0))
                }
            }),
            run: child(node, "defRPr").map(|r| RunStyle::parse(r, colors, theme)).unwrap_or_default(),
            ..Default::default()
        };
        if child(node, "buNone").is_some() {
            s.bullet = Some(BulletStyle::None);
        } else if let Some(c) = child(node, "buChar") {
            s.bullet = Some(BulletStyle::Char(c.attribute("char").unwrap_or("•").to_string()));
        } else if let Some(n) = child(node, "buAutoNum") {
            let kind = n.attribute("type").unwrap_or("arabicPeriod").to_string();
            s.bullet = Some(BulletStyle::Number(kind, num(n, "startAt").unwrap_or(1.0) as usize));
        }
        s.bullet_font = child(node, "buFont").and_then(|f| f.attribute("typeface")).map(|t| theme_font(t, theme));
        s.bullet_color = child(node, "buClr").and_then(|c| colors.first(c));
        s.bullet_scale = child(node, "buSzPct").and_then(|p| num(p, "val")).map(|v| v / 100_000.0);
        s.bullet_points = child(node, "buSzPts").and_then(|p| num(p, "val")).map(|v| v / 100.0);
        if child(node, "buSzTx").is_some() {
            s.bullet_scale = Some(1.0);
        }
        s
    }

    pub(super) fn over(&mut self, top: &LevelStyle) {
        macro_rules! take {
            ($($f:ident),*) => { $( if top.$f.is_some() { self.$f = top.$f.clone(); } )* };
        }
        take!(margin_left, indent, align, space_before, space_after, line_spacing, bullet, bullet_font, bullet_color, bullet_scale, bullet_points);
        self.run.over(&top.run);
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Levels(pub(super) [LevelStyle; 9]);

impl Levels {
    /// A list style element (lstStyle, titleStyle, …): lvl1pPr .. lvl9pPr
    pub(super) fn parse(node: Option<Node>, colors: &Colors, theme: &Theme) -> Levels {
        let mut levels = Levels::default();
        let Some(node) = node else { return levels };
        for (i, level) in levels.0.iter_mut().enumerate() {
            if let Some(p) = child(node, &format!("lvl{}pPr", i + 1)) {
                *level = LevelStyle::parse(p, colors, theme);
            }
        }
        // defPPr: the defaults of every level
        if let Some(p) = child(node, "defPPr") {
            let base = LevelStyle::parse(p, colors, theme);
            for level in &mut levels.0 {
                let mut merged = base.clone();
                merged.over(level);
                *level = merged;
            }
        }
        levels
    }

    pub(super) fn over(&mut self, top: &Levels) {
        for (a, b) in self.0.iter_mut().zip(&top.0) {
            a.over(b);
        }
    }
}
