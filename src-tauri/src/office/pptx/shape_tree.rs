//! Shapes

use super::*;

/// Maps a group's child coordinates onto the slide: one level per enclosing group,
/// innermost first
#[derive(Clone, Default)]
pub(super) struct Transform(Vec<GroupLevel>);

/// A group: its children's coordinates scaled and moved into its frame, which is
/// then flipped and rotated about its centre
#[derive(Clone, Copy)]
struct GroupLevel {
    pub(super) sx: f32,
    pub(super) sy: f32,
    pub(super) tx: f32,
    pub(super) ty: f32,
    pub(super) center: (f32, f32),
    pub(super) rotation: f32,
    pub(super) flip_h: bool,
    pub(super) flip_v: bool,
}

impl Transform {
    fn apply(&self, mut f: Frame) -> Frame {
        for g in &self.0 {
            f = Frame { x: f.x * g.sx + g.tx, y: f.y * g.sy + g.ty, w: f.w * g.sx, h: f.h * g.sy, ..f };
            let (mut mx, mut my) = (f.x + f.w / 2.0, f.y + f.h / 2.0);
            if g.flip_h {
                mx = 2.0 * g.center.0 - mx;
                f.rotation = -f.rotation;
                f.flip_h = !f.flip_h;
            }
            if g.flip_v {
                my = 2.0 * g.center.1 - my;
                f.rotation = -f.rotation;
                f.flip_v = !f.flip_v;
            }
            if g.rotation != 0.0 {
                let (s, c) = g.rotation.to_radians().sin_cos();
                let (dx, dy) = (mx - g.center.0, my - g.center.1);
                mx = g.center.0 + dx * c - dy * s;
                my = g.center.1 + dx * s + dy * c;
                f.rotation += g.rotation;
            }
            f.x = mx - f.w / 2.0;
            f.y = my - f.h / 2.0;
        }
        f
    }

    /// Inside a group (or other container) placed at `level` within this transform
    fn inside(&self, level: GroupLevel) -> Transform {
        let mut levels = vec![level];
        levels.extend(self.0.iter().copied());
        Transform(levels)
    }
}

impl GroupLevel {
    fn offset(x: f32, y: f32) -> GroupLevel {
        GroupLevel { sx: 1.0, sy: 1.0, tx: x, ty: y, center: (0.0, 0.0), rotation: 0.0, flip_h: false, flip_v: false }
    }
}

pub(super) struct Sink<'s> {
    pub(super) elements: &'s mut Vec<Element>,
    pub(super) media: &'s mut HashMap<String, Vec<u8>>,
    pub(super) package: &'s Package,
    pub(super) table_styles: &'s str,
    /// Groups (and alternate contents) entered: nesting is limited, as a file nested
    /// thousands deep would otherwise overflow the stack
    pub(super) depth: usize,
}

/// Deeper than PowerPoint lets anyone nest groups by hand
const MAX_DEPTH: usize = 48;

/// Collects the drawable elements of a shape tree, in drawing order. `slide`: whether
/// placeholders are drawn (on masters and layouts they are only templates).
pub(super) fn shapes(tree: Node, cx: &Context, sink: &mut Sink, t: &Transform, slide: bool) {
    if sink.depth >= MAX_DEPTH {
        return;
    }
    sink.depth += 1;
    shapes_in(tree, cx, sink, t, slide);
    sink.depth -= 1;
}

fn shapes_in(tree: Node, cx: &Context, sink: &mut Sink, t: &Transform, slide: bool) {
    for node in tree.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "sp" | "cxnSp" => {
                let ph = placeholder_of(node);
                if ph.is_some() && !slide {
                    continue;
                }
                if let Some(e) = shape(node, cx, sink, t, ph) {
                    sink.elements.push(e);
                }
            }
            "pic" => {
                if placeholder_of(node).is_some() && !slide {
                    continue;
                }
                if let Some(e) = picture(node, cx, sink, t) {
                    sink.elements.push(e);
                }
            }
            "grpSp" => {
                let xfrm = path(node, &["grpSpPr", "xfrm"]);
                let inner = xfrm
                    .and_then(|x| {
                        let f = xfrm_frame(x)?;
                        let ch_off = child(x, "chOff");
                        let ch_ext = child(x, "chExt");
                        let (cx0, cy0) = (ch_off.and_then(|o| emu(o, "x")).unwrap_or(f.x), ch_off.and_then(|o| emu(o, "y")).unwrap_or(f.y));
                        let (cw, chh) = (ch_ext.and_then(|e| emu(e, "cx")).unwrap_or(f.w), ch_ext.and_then(|e| emu(e, "cy")).unwrap_or(f.h));
                        let sx = if cw > 0.0 { f.w / cw } else { 1.0 };
                        let sy = if chh > 0.0 { f.h / chh } else { 1.0 };
                        Some(t.inside(GroupLevel {
                            sx,
                            sy,
                            tx: f.x - cx0 * sx,
                            ty: f.y - cy0 * sy,
                            center: (f.x + f.w / 2.0, f.y + f.h / 2.0),
                            rotation: f.rotation,
                            flip_h: f.flip_h,
                            flip_v: f.flip_v,
                        }))
                    })
                    .unwrap_or_else(|| t.clone());
                shapes(node, cx, sink, &inner, slide);
            }
            "graphicFrame" => graphic_frame(node, cx, sink, t),
            // Newer content with an older equivalent for other readers
            "AlternateContent" => {
                if let Some(fallback) = child(node, "Fallback") {
                    shapes(fallback, cx, sink, t, slide);
                } else if let Some(choice) = child(node, "Choice") {
                    shapes(choice, cx, sink, t, slide);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn shape(node: Node, cx: &Context, sink: &mut Sink, t: &Transform, ph: Option<(String, Option<String>)>) -> Option<Element> {
    let sp_pr = child(node, "spPr");
    // Inherited placeholder definitions: the layout's, then the master's
    let (layout_def, master_def) = match &ph {
        Some((kind, idx)) => {
            let l = cx.layout.find(kind, idx.as_deref());
            let m = cx.master.find(l.map(|d| d.kind.as_str()).unwrap_or(kind), None);
            (l, m)
        }
        None => (None, None),
    };
    let frame = frame_of(sp_pr).or_else(|| layout_def.and_then(|d| d.frame)).or_else(|| master_def.and_then(|d| d.frame))?;
    let frame = t.apply(frame);

    // Style references (p:style) give default fill, line and text colour
    let style = child(node, "style");
    let style_ref = |name: &str| style.and_then(|s| child(s, name));
    let style_fill = style_ref("fillRef").and_then(|r| {
        let idx = num(r, "idx").unwrap_or(0.0) as usize;
        let list = if idx >= 1001 { &cx.theme.background_styles } else { &cx.theme.fill_styles };
        let xml = list.get(if idx >= 1001 { idx - 1001 } else { idx.checked_sub(1)? })?;
        let wrapped = format!("<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{xml}</w>");
        let doc = Document::parse(&wrapped).ok()?;
        fill_of(doc.root_element(), &cx.colors.with_placeholder(cx.colors.first(r)), &cx.rels, sink.media, sink.package)
    });
    let style_line = style_ref("lnRef").and_then(|r| {
        let idx = num(r, "idx").unwrap_or(0.0) as usize;
        let xml = cx.theme.line_styles.get(idx.checked_sub(1)?)?;
        let wrapped = format!("<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{xml}</w>");
        let doc = Document::parse(&wrapped).ok()?;
        line_of(doc.root_element(), &cx.colors.with_placeholder(cx.colors.first(r))).flatten()
    });
    let font_ref = style_ref("fontRef");

    // Fill and line: the shape's own, else its placeholder's, else the style's
    let inherited_sp_pr = |def: Option<&PlaceholderDef>| def.and_then(|d| d.sp_pr.clone());
    let has_fill = |n: Node| n.children().any(|c| matches!(c.tag_name().name(), "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill"));
    let mut fill = None;
    let mut fill_found = false;
    for xml in [sp_pr.map(|n| n.document().input_text()[n.range()].to_string()), inherited_sp_pr(layout_def), inherited_sp_pr(master_def)].into_iter().flatten() {
        let wrapped = wrap(&xml);
        let Ok(doc) = Document::parse(&wrapped) else { continue };
        let pr = doc.root_element().first_element_child().unwrap_or(doc.root_element());
        if has_fill(pr) {
            fill = fill_of(pr, &cx.colors, &cx.rels, sink.media, sink.package);
            fill_found = true;
            break;
        }
    }
    if !fill_found {
        fill = style_fill;
    }
    let mut line = None;
    let mut line_found = false;
    for xml in [sp_pr.map(|n| n.document().input_text()[n.range()].to_string()), inherited_sp_pr(layout_def), inherited_sp_pr(master_def)].into_iter().flatten() {
        let wrapped = wrap(&xml);
        let Ok(doc) = Document::parse(&wrapped) else { continue };
        let pr = doc.root_element().first_element_child().unwrap_or(doc.root_element());
        if let Some(l) = line_of(pr, &cx.colors) {
            line = l;
            line_found = true;
            break;
        }
    }
    if !line_found {
        line = style_line;
    }
    let mut geometry = sp_pr.map(|p| geometry_of(p, frame.w, frame.h)).unwrap_or(Geometry::Rect);
    // A connector or line has no inside
    let fill = if geometry.is_open() { None } else { fill };
    if let (Some(l), Geometry::Paths(subs)) = (&line, &mut geometry)
        && (l.head.is_some() || l.tail.is_some())
    {
        let heads = crate::office::shapes::arrowheads(&Geometry::Paths(subs.clone()), l.width, l.head, l.tail);
        subs.extend(heads);
    }

    let text = child(node, "txBody").and_then(|body| {
        let mut levels = cx.base[match ph.as_ref().map(|(k, _)| text_kind(k)) {
            Some(TextKind::Title) => 0,
            Some(TextKind::Body) => 1,
            Some(TextKind::Other) => 2,
            None => 3,
        }]
        .clone();
        if let Some(d) = master_def {
            levels.over(&d.levels);
        }
        if let Some(d) = layout_def {
            levels.over(&d.levels);
        }
        // The style's font reference: the shape's default text colour and font, under
        // what the shape's own list style says
        if let Some(r) = font_ref {
            let color = cx.colors.first(r);
            let font = match r.attribute("idx") {
                Some("major") => Some(cx.theme.major_font.clone()),
                Some("minor") => Some(cx.theme.minor_font.clone()),
                _ => None,
            };
            for level in &mut levels.0 {
                level.run.color = color.or(level.run.color);
                level.run.font = font.clone().or(level.run.font.take());
            }
        }
        levels.over(&Levels::parse(child(body, "lstStyle"), &cx.colors, cx.theme));
        let body_prs: Vec<String> = [
            child(body, "bodyPr").map(|n| n.document().input_text()[n.range()].to_string()),
            layout_def.and_then(|d| d.body_pr.clone()),
            master_def.and_then(|d| d.body_pr.clone()),
        ]
        .into_iter()
        .flatten()
        .collect();
        text_body(body, &body_prs, &levels, cx, matches!(ph.as_ref().map(|p| p.0.as_str()), Some("sldNum")))
    });
    let text = text.map(|mut text| {
        let area = sp_pr.and_then(|p| child(p, "prstGeom")).and_then(|g| crate::office::shapes::text_rect(g.attribute("prst")?, &adjust(g), frame.w, frame.h));
        if let Some(area) = area {
            for (inset, extra) in text.insets.iter_mut().zip(area) {
                *inset += extra;
            }
        }
        text
    });
    if fill.is_none() && line.is_none() && text.as_ref().is_none_or(|t| t.paragraphs.iter().all(|p| p.runs.iter().all(|r| r.text.trim().is_empty()))) {
        return None;
    }
    Some(Element::Shape { frame, geometry, fill, line, text, placeholder: ph.map(|(kind, _)| kind) })
}

pub(super) fn wrap(xml: &str) -> String {
    format!(
        "<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">{xml}</w>"
    )
}

/// A preset's adjust values (avLst), as stored
fn adjust<'a>(preset: Node<'a, 'a>) -> impl Fn(&str, f32) -> f32 + 'a {
    move |name: &str, default: f32| {
        path(preset, &["avLst"])
            .and_then(|l| children(l, "gd").find(|g| g.attribute("name") == Some(name)))
            .and_then(|g| g.attribute("fmla"))
            .and_then(|f| f.strip_prefix("val ")?.trim().parse::<f32>().ok())
            .unwrap_or(default)
    }
}

fn geometry_of(sp_pr: Node, w: f32, h: f32) -> Geometry {
    if let Some(g) = child(sp_pr, "custGeom").and_then(|c| custom_geometry(c, w, h)) {
        return g;
    }
    let Some(preset) = child(sp_pr, "prstGeom") else { return Geometry::Rect };
    crate::office::shapes::preset(preset.attribute("prst").unwrap_or("rect"), &adjust(preset), w, h).unwrap_or(Geometry::Rect)
}

/// Custom geometry (freeforms, icons). Coordinates may name guides: the shape's
/// built-in sizes (w, h, wd2, …) and the formulas in its gdLst/avLst.
fn custom_geometry(custom: Node, w: f32, h: f32) -> Option<Geometry> {
    use crate::office::shapes::CustomStep;
    let mut guides = Guides::new(w * EMU, h * EMU);
    for list in ["avLst", "gdLst"] {
        if let Some(l) = child(custom, list) {
            for gd in children(l, "gd") {
                if let (Some(name), Some(fmla)) = (gd.attribute("name"), gd.attribute("fmla")) {
                    let v = guides.formula(fmla);
                    guides.values.insert(name.to_string(), v);
                }
            }
        }
    }
    let mut list = vec![];
    for p in children(path(custom, &["pathLst"])?, "path") {
        // Without its own size a path is in the shape's units (EMU)
        let pw = num(p, "w").filter(|v| *v > 0.0).unwrap_or(w * EMU);
        let ph = num(p, "h").filter(|v| *v > 0.0).unwrap_or(h * EMU);
        let fill = p.attribute("fill") != Some("none");
        let stroke = !matches!(p.attribute("stroke"), Some("0" | "false"));
        let pt = |n: Node| (guides.get(n.attribute("x").unwrap_or("0")), guides.get(n.attribute("y").unwrap_or("0")));
        let mut steps = vec![];
        for c in p.children().filter(Node::is_element) {
            let pts: Vec<(f32, f32)> = children(c, "pt").map(pt).collect();
            match (c.tag_name().name(), pts.as_slice()) {
                ("moveTo", [a, ..]) => steps.push(CustomStep::Move(a.0, a.1)),
                ("lnTo", [a, ..]) => steps.push(CustomStep::Line(a.0, a.1)),
                ("cubicBezTo", [a, b, e, ..]) => steps.push(CustomStep::Cubic([a.0, a.1, b.0, b.1, e.0, e.1])),
                ("quadBezTo", [q, e, ..]) => steps.push(CustomStep::Quad(q.0, q.1, e.0, e.1)),
                ("arcTo", _) => {
                    let g = |n: &str| guides.get(c.attribute(n).unwrap_or("0"));
                    steps.push(CustomStep::Arc(g("wR"), g("hR"), g("stAng") / 60_000.0, g("swAng") / 60_000.0));
                }
                ("close", _) => steps.push(CustomStep::Close),
                _ => {}
            }
        }
        list.push((pw, ph, fill, stroke, steps));
    }
    crate::office::shapes::custom(&list, w, h)
}

/// Shape guide values for custom geometry formulas
struct Guides {
    pub(super) values: HashMap<String, f32>,
}

impl Guides {
    pub(super) fn new(w: f32, h: f32) -> Guides {
        let ss = w.min(h);
        let mut values = HashMap::new();
        for (k, v) in [
            ("w", w), ("h", h), ("l", 0.0), ("t", 0.0), ("r", w), ("b", h), ("hc", w / 2.0), ("vc", h / 2.0),
            ("ss", ss), ("ls", w.max(h)), ("cd2", 10_800_000.0), ("cd4", 5_400_000.0), ("cd8", 2_700_000.0),
            ("3cd4", 16_200_000.0), ("3cd8", 8_100_000.0), ("5cd8", 13_500_000.0), ("7cd8", 18_900_000.0),
        ] {
            values.insert(k.to_string(), v);
        }
        for d in [2.0f32, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 32.0] {
            values.insert(format!("wd{d}"), w / d);
            values.insert(format!("hd{d}"), h / d);
            values.insert(format!("ssd{d}"), ss / d);
        }
        Guides { values }
    }

    pub(super) fn get(&self, token: &str) -> f32 {
        token.parse().unwrap_or_else(|_| self.values.get(token).copied().unwrap_or(0.0))
    }

    fn formula(&self, fmla: &str) -> f32 {
        let mut parts = fmla.split_whitespace();
        let op = parts.next().unwrap_or("");
        let args: Vec<f32> = parts.map(|t| self.get(t)).collect();
        let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
        let rad = |v: f32| (v / 60_000.0).to_radians();
        match op {
            "val" => a(0),
            "*/" => if a(2) == 0.0 { 0.0 } else { a(0) * a(1) / a(2) },
            "+-" => a(0) + a(1) - a(2),
            "+/" => if a(2) == 0.0 { 0.0 } else { (a(0) + a(1)) / a(2) },
            "?:" => if a(0) > 0.0 { a(1) } else { a(2) },
            "abs" => a(0).abs(),
            "sqrt" => a(0).max(0.0).sqrt(),
            "max" => a(0).max(a(1)),
            "min" => a(0).min(a(1)),
            "pin" => a(1).clamp(a(0).min(a(2)), a(2).max(a(0))),
            "mod" => (a(0) * a(0) + a(1) * a(1) + a(2) * a(2)).sqrt(),
            "sin" => a(0) * rad(a(1)).sin(),
            "cos" => a(0) * rad(a(1)).cos(),
            "tan" => a(0) * rad(a(1)).tan(),
            "at2" => a(1).atan2(a(0)).to_degrees() * 60_000.0,
            "cat2" => a(0) * a(2).atan2(a(1)).cos(),
            "sat2" => a(0) * a(2).atan2(a(1)).sin(),
            _ => 0.0,
        }
    }
}

pub(super) fn picture(node: Node, cx: &Context, sink: &mut Sink, t: &Transform) -> Option<Element> {
    let blip_fill = child(node, "blipFill")?;
    let id = child(blip_fill, "blip")?.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "embed"))?;
    let target = cx.rels.get(id)?.1.clone();
    if !sink.media.contains_key(&target) {
        let data = sink.package.get(&target)?;
        sink.media.insert(target.clone(), data);
    }
    let sp_pr = child(node, "spPr");
    let ph = placeholder_of(node);
    let frame = frame_of(sp_pr).or_else(|| {
        let (kind, idx) = ph.as_ref()?;
        cx.layout.find(kind, idx.as_deref()).and_then(|d| d.frame).or_else(|| cx.master.find(kind, None).and_then(|d| d.frame))
    })?;
    let crop = child(blip_fill, "srcRect")
        .map(|r| ["l", "t", "r", "b"].map(|k| num(r, k).unwrap_or(0.0) / 100_000.0))
        .unwrap_or([0.0; 4]);
    let frame = t.apply(frame);
    let geometry = sp_pr.map(|p| geometry_of(p, frame.w, frame.h)).unwrap_or(Geometry::Rect);
    let line = sp_pr.and_then(|p| line_of(p, &cx.colors)).flatten();
    Some(Element::Picture { frame, media: target, crop, geometry, line })
}

fn graphic_frame(node: Node, cx: &Context, sink: &mut Sink, t: &Transform) {
    let Some(local) = child(node, "xfrm").and_then(xfrm_frame) else { return };
    let frame = t.apply(local);
    let Some(data) = path(node, &["graphic", "graphicData"]) else { return };
    if let Some(table) = child(data, "tbl") {
        if let Some(e) = table_element(table, frame, cx, sink) {
            sink.elements.push(e);
        }
        return;
    }
    // SmartArt: PowerPoint saves a drawing of it next to the diagram data
    if data.attribute("uri").is_some_and(|u| u.ends_with("/diagram")) {
        let drawing = cx.rels.values().find(|(k, _)| k == "diagramDrawing").map(|(_, p)| p.clone());
        let Some(part) = drawing else { return };
        let Some(xml) = sink.package.text(&part) else { return };
        let Ok(doc) = Document::parse(&xml) else { return };
        if let Some(tree) = doc.descendants().find(|n| n.tag_name().name() == "spTree") {
            let rels = sink.package.rels(&part);
            let dcx = Context { rels, part: part.clone(), ..cx.clone_shallow() };
            shapes(tree, &dcx, sink, &t.inside(GroupLevel::offset(local.x, local.y)), true);
        }
    }
}
