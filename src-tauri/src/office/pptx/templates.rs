//! Masters and layouts: their placeholders, styles and backgrounds

use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum TextKind {
    Title,
    Body,
    Other,
}

/// A placeholder on a layout or master, for slides to inherit from
#[derive(Clone)]
pub(super) struct PlaceholderDef {
    pub(super) kind: String,
    pub(super) idx: Option<String>,
    pub(super) frame: Option<Frame>,
    /// spPr (fill, line) and bodyPr, as XML
    pub(super) sp_pr: Option<String>,
    pub(super) body_pr: Option<String>,
    pub(super) levels: Levels,
}

pub(super) struct Template {
    pub(super) xml: String,
    pub(super) color_map: HashMap<String, String>,
    pub(super) placeholders: Vec<PlaceholderDef>,
    pub(super) show_master_shapes: bool,
}

pub(super) fn placeholder_of(sp: Node) -> Option<(String, Option<String>)> {
    let ph = sp.children().filter(Node::is_element).find_map(|nv| {
        if nv.tag_name().name().starts_with("nv") {
            path(nv, &["nvPr", "ph"])
        } else {
            None
        }
    })?;
    Some((ph.attribute("type").unwrap_or("obj").to_string(), ph.attribute("idx").map(str::to_string)))
}

pub(super) fn text_kind(kind: &str) -> TextKind {
    match kind {
        "title" | "ctrTitle" => TextKind::Title,
        "dt" | "ftr" | "sldNum" | "hdr" => TextKind::Other,
        _ => TextKind::Body,
    }
}

pub(super) fn frame_of(sp_pr: Option<Node>) -> Option<Frame> {
    let xfrm = sp_pr.and_then(|p| child(p, "xfrm"))?;
    xfrm_frame(xfrm)
}

pub(super) fn xfrm_frame(xfrm: Node) -> Option<Frame> {
    let off = child(xfrm, "off")?;
    let ext = child(xfrm, "ext")?;
    Some(Frame {
        x: emu(off, "x").unwrap_or(0.0),
        y: emu(off, "y").unwrap_or(0.0),
        w: emu(ext, "cx").unwrap_or(0.0),
        h: emu(ext, "cy").unwrap_or(0.0),
        rotation: num(xfrm, "rot").unwrap_or(0.0) / 60_000.0,
        flip_h: flag(xfrm, "flipH").unwrap_or(false),
        flip_v: flag(xfrm, "flipV").unwrap_or(false),
    })
}

pub(super) fn color_map(node: Option<Node>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(node) = node {
        for a in node.attributes() {
            map.insert(a.name().to_string(), a.value().to_string());
        }
    }
    map
}

impl Template {
    pub(super) fn parse(xml: String, colors: &Colors, theme: &Theme, parent_map: Option<&HashMap<String, String>>) -> Template {
        let mut t = Template { xml: String::new(), color_map: HashMap::new(), placeholders: vec![], show_master_shapes: true };
        if let Ok(doc) = Document::parse(&xml) {
            let root = doc.root_element();
            t.show_master_shapes = flag(root, "showMasterSp").unwrap_or(true);
            t.color_map = match parent_map {
                // a layout keeps the master's map unless it overrides it
                Some(parent) => match path(root, &["clrMapOvr", "overrideClrMapping"]) {
                    Some(o) => color_map(Some(o)),
                    None => parent.clone(),
                },
                None => color_map(child(root, "clrMap")),
            };
            let colors = Colors { theme, map: t.color_map.clone(), placeholder: colors.placeholder };
            if let Some(tree) = path(root, &["cSld", "spTree"]) {
                for sp in tree.descendants().filter(|n| n.is_element() && n.tag_name().name() == "sp") {
                    let Some((kind, idx)) = placeholder_of(sp) else { continue };
                    let sp_pr = child(sp, "spPr");
                    let body = child(sp, "txBody");
                    t.placeholders.push(PlaceholderDef {
                        kind,
                        idx,
                        frame: frame_of(sp_pr),
                        sp_pr: sp_pr.map(|n| xml[n.range()].to_string()),
                        body_pr: body.and_then(|b| child(b, "bodyPr")).map(|n| xml[n.range()].to_string()),
                        levels: Levels::parse(body.and_then(|b| child(b, "lstStyle")), &colors, theme),
                    });
                }
            }
        }
        t.xml = xml;
        t
    }

    /// The placeholder a slide's (kind, idx) inherits from here
    pub(super) fn find(&self, kind: &str, idx: Option<&str>) -> Option<&PlaceholderDef> {
        let same_kind = |a: &str, b: &str| a == b || (text_kind(a) == TextKind::Title && text_kind(b) == TextKind::Title);
        idx.and_then(|i| self.placeholders.iter().find(|p| p.idx.as_deref() == Some(i)))
            .or_else(|| self.placeholders.iter().find(|p| same_kind(&p.kind, kind)))
            .or_else(|| match text_kind(kind) {
                TextKind::Body if kind != "body" => self.placeholders.iter().find(|p| p.kind == "body"),
                _ => None,
            })
    }
}
