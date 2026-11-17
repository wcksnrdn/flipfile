//! Text

use super::*;

pub(super) fn text_body(body: Node, body_prs: &[String], levels: &Levels, cx: &Context, slide_number: bool) -> Option<TextBody> {
    // bodyPr: the shape's own attributes win over its placeholder's
    let mut insets = [7.2, 3.6, 7.2, 3.6];
    let mut anchor = None;
    let mut wrap_text = None;
    let mut grows = false;
    let mut font_scale = None;
    let mut spacing_reduction = None;
    for xml in body_prs.iter().rev() {
        let wrapped = wrap(xml);
        let Ok(doc) = Document::parse(&wrapped) else { continue };
        let Some(pr) = doc.root_element().first_element_child() else { continue };
        for (i, name) in ["lIns", "tIns", "rIns", "bIns"].iter().enumerate() {
            if let Some(v) = emu(pr, name) {
                insets[i] = v;
            }
        }
        if let Some(a) = pr.attribute("anchor") {
            anchor = Some(match a {
                "ctr" => Anchor::Middle,
                "b" => Anchor::Bottom,
                _ => Anchor::Top,
            });
        }
        if let Some(w) = pr.attribute("wrap") {
            wrap_text = Some(w != "none");
        }
        if child(pr, "spAutoFit").is_some() {
            grows = true;
        } else if child(pr, "normAutofit").is_some() || child(pr, "noAutofit").is_some() {
            grows = false;
        }
        if let Some(fit) = child(pr, "normAutofit") {
            font_scale = num(fit, "fontScale").map(|v| v / 100_000.0).or(font_scale);
            spacing_reduction = num(fit, "lnSpcReduction").map(|v| v / 100_000.0).or(spacing_reduction);
        }
    }
    let scale = font_scale.unwrap_or(1.0);
    let mut paragraphs = vec![];
    let mut numbering: HashMap<usize, usize> = HashMap::new();
    for p in children(body, "p") {
        let ppr = child(p, "pPr");
        let level = ppr.and_then(|n| num(n, "lvl")).unwrap_or(0.0).clamp(0.0, 8.0) as usize;
        let mut style = levels.0[level].clone();
        if let Some(ppr) = ppr {
            style.over(&LevelStyle::parse(ppr, &cx.colors, cx.theme));
        }
        let mut runs = vec![];
        for r in p.children().filter(Node::is_element) {
            match r.tag_name().name() {
                "r" | "fld" => {
                    let mut rs = style.run.clone();
                    if let Some(rpr) = child(r, "rPr") {
                        rs.over(&RunStyle::parse(rpr, &cx.colors, cx.theme));
                    }
                    let mut text: String = child(r, "t").and_then(|t| t.text()).unwrap_or("").to_string();
                    if r.tag_name().name() == "fld" && r.attribute("type").is_some_and(|t| t.starts_with("slidenum")) {
                        text = cx.number.to_string();
                    }
                    runs.push(make_run(text, &rs, scale, false));
                }
                "br" => {
                    let mut rs = style.run.clone();
                    if let Some(rpr) = child(r, "rPr") {
                        rs.over(&RunStyle::parse(rpr, &cx.colors, cx.theme));
                    }
                    runs.push(make_run(String::new(), &rs, scale, true));
                }
                _ => {}
            }
        }
        let mut end = style.run.clone();
        if let Some(e) = child(p, "endParaRPr") {
            end.over(&RunStyle::parse(e, &cx.colors, cx.theme));
        }
        let size = end.size.unwrap_or(18.0) * scale;
        let has_text = runs.iter().any(|r| !r.text.is_empty());
        let bullet = match (&style.bullet, has_text && !slide_number) {
            (Some(BulletStyle::Char(c)), true) => Some(c.clone()),
            (Some(BulletStyle::Number(kind, start)), true) => {
                let n = numbering.entry(level).or_insert(*start);
                let text = number_text(kind, *n);
                *n += 1;
                Some(text)
            }
            _ => None,
        };
        // numbering restarts after a paragraph of a shallower level
        numbering.retain(|&l, _| l <= level);
        let first_size = runs.iter().find(|r| !r.line_break).map(|r| r.size).unwrap_or(size);
        let line_spacing = match style.line_spacing.unwrap_or(LineSpacing::Multiple(1.0)) {
            LineSpacing::Multiple(m) => LineSpacing::Multiple((m - spacing_reduction.unwrap_or(0.0)).max(0.5)),
            other => other,
        };
        let spacing_points = |s: Option<Spacing>| match s {
            Some(Spacing::Points(p)) => p,
            Some(Spacing::Percent(f)) => f * first_size * 1.2,
            None => 0.0,
        };
        paragraphs.push(Paragraph {
            align: style.align.unwrap_or(Align::Left),
            margin_left: style.margin_left.unwrap_or(0.0),
            indent: style.indent.unwrap_or(0.0),
            space_before: spacing_points(style.space_before),
            space_after: spacing_points(style.space_after),
            line_spacing,
            bullet: bullet.map(|text| Bullet {
                text,
                font: style.bullet_font.clone(),
                color: style.bullet_color,
                scale: style.bullet_scale.unwrap_or(1.0),
                points: style.bullet_points.map(|p| p * scale),
            }),
            runs,
            size,
        });
    }
    if paragraphs.is_empty() {
        return None;
    }
    Some(TextBody { insets, anchor: anchor.unwrap_or(Anchor::Top), wrap: wrap_text.unwrap_or(true), grows, paragraphs })
}

fn make_run(text: String, style: &RunStyle, scale: f32, line_break: bool) -> Run {
    let size = style.size.unwrap_or(18.0);
    Run {
        kern: style.kern.is_some_and(|k| k > 0.0 && size >= k),
        spacing: style.spacing.unwrap_or(0.0) * scale,
        highlight: style.highlight,
        caps: style.caps.unwrap_or_default(),
        text,
        font: style.font.clone().unwrap_or_else(|| "Calibri".into()),
        size: style.size.unwrap_or(18.0) * scale,
        bold: style.bold.unwrap_or(false),
        italic: style.italic.unwrap_or(false),
        underline: style.underline.unwrap_or(false),
        strike: style.strike.unwrap_or(false),
        color: style.color.unwrap_or(Rgba { rgb: [0, 0, 0], alpha: 1.0 }),
        baseline: style.baseline.unwrap_or(0.0),
        line_break,
    }
}

pub(super) fn number_text(kind: &str, n: usize) -> String {
    let roman = |mut n: usize| {
        let table = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
        let mut s = String::new();
        for (v, r) in table {
            while n >= v {
                s.push_str(r);
                n -= v;
            }
        }
        s
    };
    let alpha = |n: usize| ((b'a' + ((n.max(1) - 1) % 26) as u8) as char).to_string();
    let body = if kind.starts_with("alphaLc") {
        alpha(n)
    } else if kind.starts_with("alphaUc") {
        alpha(n).to_uppercase()
    } else if kind.starts_with("romanLc") {
        roman(n)
    } else if kind.starts_with("romanUc") {
        roman(n).to_uppercase()
    } else {
        n.to_string()
    };
    if kind.ends_with("ParenBoth") {
        format!("({body})")
    } else if kind.ends_with("ParenR") {
        format!("{body})")
    } else if kind.ends_with("Plain") {
        body
    } else {
        format!("{body}.")
    }
}

pub(super) fn slide_title(tree: Node) -> String {
    tree.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "sp")
        .find(|sp| placeholder_of(*sp).is_some_and(|(k, _)| text_kind(&k) == TextKind::Title))
        .map(|sp| paragraphs_text(sp))
        .unwrap_or_default()
}

fn paragraphs_text(node: Node) -> String {
    node.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(|p| p.descendants().filter(|t| t.is_element() && t.tag_name().name() == "t").filter_map(|t| t.text()).collect::<String>())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The speaker notes: the text of the notes page's body placeholder
pub(super) fn notes_text(xml: &str) -> String {
    let Ok(doc) = Document::parse(xml) else { return String::new() };
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "sp")
        .filter(|sp| placeholder_of(*sp).is_some_and(|(k, _)| k == "body"))
        .map(paragraphs_text)
        .collect::<Vec<_>>()
        .join("\n")
}
