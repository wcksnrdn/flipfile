//! Reading

use super::*;

pub fn read(bytes: &[u8]) -> Result<Presentation, String> {
    let package = Package::open(bytes)?;
    let presentation_part = package
        .rels("")
        .into_values()
        .find(|(kind, _)| kind == "officeDocument")
        .map(|(_, target)| target)
        .unwrap_or_else(|| "ppt/presentation.xml".into());
    let presentation_xml = package.text(&presentation_part).ok_or("the presentation part is missing")?;
    let doc = Document::parse(&presentation_xml).map_err(|_| "the presentation could not be read")?;
    let root = doc.root_element();
    let size = child(root, "sldSz");
    let width = size.and_then(|s| emu(s, "cx")).filter(|w| *w > 0.0).unwrap_or(720.0);
    let height = size.and_then(|s| emu(s, "cy")).filter(|h| *h > 0.0).unwrap_or(540.0);
    let first_number = num(root, "firstSlideNum").unwrap_or(1.0) as usize;
    let rels = package.rels(&presentation_part);
    let slide_parts: Vec<String> = child(root, "sldIdLst")
        .map(|list| {
            children(list, "sldId")
                .filter_map(|s| s.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id")))
                .filter_map(|id| rels.get(id).map(|(_, target)| target.clone()))
                .collect()
        })
        .unwrap_or_default();
    if slide_parts.is_empty() {
        return Err("the presentation has no slides".into());
    }
    let default_style = child(root, "defaultTextStyle").map(|n| presentation_xml[n.range()].to_string());
    let table_styles = package.text("ppt/tableStyles.xml").unwrap_or_default();

    let mut reader = Reader {
        package: &package,
        themes: HashMap::new(),
        templates: HashMap::new(),
        default_style,
        table_styles,
        media: HashMap::new(),
    };
    let mut slides = vec![];
    for (i, part) in slide_parts.iter().enumerate() {
        if let Some(slide) = reader.slide(part, first_number + i) {
            slides.push(slide);
        }
    }
    // Every slide damaged (or nested too deep to read): an empty deck would only hide that
    if slides.is_empty() {
        return Err("none of the slides could be read".into());
    }
    let media = reader.media;
    let fonts = embedded_fonts(&package, root, &rels);
    Ok(Presentation { width, height, slides, media, fonts })
}

/// The fonts listed in the presentation's `embeddedFontLst`, each style that could be read
fn embedded_fonts(package: &Package, root: Node, rels: &HashMap<String, (String, String)>) -> Vec<(String, Vec<u8>)> {
    let Some(list) = child(root, "embeddedFontLst") else { return vec![] };
    let mut out = vec![];
    for font in children(list, "embeddedFont") {
        let Some(name) = child(font, "font").and_then(|f| f.attribute("typeface")).filter(|n| !n.trim().is_empty()) else { continue };
        for style in ["regular", "bold", "italic", "boldItalic"] {
            let Some(id) = child(font, style).and_then(|s| s.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id"))) else { continue };
            let Some((_, target)) = rels.get(id) else { continue };
            if let Some(data) = package.get(target).and_then(|part| font_data(&part)) {
                out.push((name.trim().to_string(), data));
            }
        }
    }
    out
}

/// The TrueType/OpenType font in an embedded font part: an Embedded OpenType wrapper
/// (.fntdata), XOR-obscured or not, or a plain font file. Fonts compressed with
/// MicroType Express are left out, and the slides use look-alikes for them.
pub(super) fn font_data(part: &[u8]) -> Option<Vec<u8>> {
    let is_font = |d: &[u8]| d.len() > 12 && matches!(&d[..4], [0, 1, 0, 0] | b"OTTO" | b"true");
    if is_font(part) {
        return Some(part.to_vec());
    }
    let u32_at = |i: usize| part.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (size, data_size, version, flags) = (u32_at(0)? as usize, u32_at(4)? as usize, u32_at(8)?, u32_at(12)?);
    const COMPRESSED: u32 = 0x4;
    const XOR: u32 = 0x1000_0000;
    let magic = part.get(34..36) == Some(&[0x4C, 0x50]);
    if !magic || !matches!(version, 0x0001_0000 | 0x0002_0001 | 0x0002_0002) || flags & COMPRESSED != 0 || data_size == 0 || data_size > size || size > part.len() {
        return None;
    }
    // The font data is the last field of the header
    let mut data = part[size - data_size..size].to_vec();
    if flags & XOR != 0 {
        data.iter_mut().for_each(|b| *b ^= 0x50);
    }
    is_font(&data).then_some(data)
}

struct Reader<'p> {
    pub(super) package: &'p Package,
    pub(super) themes: HashMap<String, Theme>,
    pub(super) templates: HashMap<String, Template>,
    pub(super) default_style: Option<String>,
    pub(super) table_styles: String,
    pub(super) media: HashMap<String, Vec<u8>>,
}

/// What a slide's shapes need to be resolved
pub(super) struct Context<'a> {
    pub(super) theme: &'a Theme,
    pub(super) colors: Colors<'a>,
    pub(super) master: &'a Template,
    pub(super) layout: &'a Template,
    /// The master's title, body and other placeholder styles, and the presentation's
    /// default text style (text boxes, shapes and tables)
    pub(super) base: [Levels; 4],
    pub(super) part: String,
    pub(super) rels: HashMap<String, (String, String)>,
    pub(super) number: usize,
}

impl<'p> Reader<'p> {
    pub(super) fn theme(&mut self, part: &str) -> &Theme {
        if !self.themes.contains_key(part) {
            let theme = Theme::parse(&self.package.text(part).unwrap_or_default());
            self.themes.insert(part.to_string(), theme);
        }
        &self.themes[part]
    }

    fn slide(&mut self, part: &str, number: usize) -> Option<Slide> {
        let xml = self.package.text(part)?;
        let doc = Document::parse(&xml).ok()?;
        let root = doc.root_element();
        let rels = self.package.rels(part);
        let target = |kind: &str| rels.values().find(|(k, _)| k == kind).map(|(_, t)| t.clone());
        let layout_part = target("slideLayout").unwrap_or_default();
        let layout_rels = self.package.rels(&layout_part);
        let master_part = layout_rels.values().find(|(k, _)| k == "slideMaster").map(|(_, t)| t.clone()).unwrap_or_default();
        let master_rels = self.package.rels(&master_part);
        let theme_part = master_rels.values().find(|(k, _)| k == "theme").map(|(_, t)| t.clone()).unwrap_or_default();
        self.theme(&theme_part);
        let theme = &self.themes[&theme_part];

        let blank = Colors { theme, map: HashMap::new(), placeholder: None };
        if !self.templates.contains_key(&master_part) {
            let master = Template::parse(self.package.text(&master_part).unwrap_or_default(), &blank, theme, None);
            self.templates.insert(master_part.clone(), master);
        }
        if !self.templates.contains_key(&layout_part) {
            let master_map = self.templates[&master_part].color_map.clone();
            let layout = Template::parse(self.package.text(&layout_part).unwrap_or_default(), &blank, theme, Some(&master_map));
            self.templates.insert(layout_part.clone(), layout);
        }
        let master = &self.templates[&master_part];
        let layout = &self.templates[&layout_part];

        let map = match path(root, &["clrMapOvr", "overrideClrMapping"]) {
            Some(o) => color_map(Some(o)),
            None => layout.color_map.clone(),
        };
        let colors = Colors { theme, map, placeholder: None };
        let base = {
            let master_doc = Document::parse(&master.xml).ok();
            let styles = master_doc.as_ref().and_then(|d| path(d.root_element(), &["txStyles"]));
            let default = self.default_style.as_deref().and_then(|x| Document::parse(x).ok());
            let plain = Levels::parse(default.as_ref().map(|d| d.root_element()), &colors, theme);
            let parse = |name: &str| Levels::parse(styles.and_then(|s| child(s, name)), &colors, theme);
            let (mut title, mut body, mut other) = (plain.clone(), plain.clone(), plain.clone());
            title.over(&parse("titleStyle"));
            body.over(&parse("bodyStyle"));
            other.over(&parse("otherStyle"));
            [title, body, other, plain]
        };
        let cx = Context { theme, colors: colors.clone(), master, layout, base, part: part.to_string(), rels: rels.clone(), number };

        // Background: the slide's, else the layout's, else the master's
        let background = [(&xml, &rels, &colors)]
            .into_iter()
            .find_map(|(x, r, c)| background(x, r, c, theme, &mut self.media, self.package))
            .or_else(|| {
                let c = Colors { theme, map: layout.color_map.clone(), placeholder: None };
                background(&layout.xml, &layout_rels, &c, theme, &mut self.media, self.package)
            })
            .or_else(|| {
                let c = Colors { theme, map: master.color_map.clone(), placeholder: None };
                background(&master.xml, &master_rels, &c, theme, &mut self.media, self.package)
            });

        let mut elements = vec![];
        let mut sink = Sink { elements: &mut elements, media: &mut self.media, package: self.package, table_styles: &self.table_styles, depth: 0 };
        let show_master = flag(root, "showMasterSp").unwrap_or(true) && layout.show_master_shapes;
        if show_master {
            let master_cx = Context { rels: master_rels.clone(), part: master_part.clone(), colors: Colors { theme, map: master.color_map.clone(), placeholder: None }, ..cx.clone_shallow() };
            if let Ok(d) = Document::parse(&master.xml)
                && let Some(tree) = path(d.root_element(), &["cSld", "spTree"])
            {
                shapes(tree, &master_cx, &mut sink, &Transform::default(), false);
            }
        }
        if flag(root, "showMasterSp").unwrap_or(true) {
            let layout_cx = Context { rels: layout_rels.clone(), part: layout_part.clone(), colors: Colors { theme, map: layout.color_map.clone(), placeholder: None }, ..cx.clone_shallow() };
            if let Ok(d) = Document::parse(&layout.xml)
                && let Some(tree) = path(d.root_element(), &["cSld", "spTree"])
            {
                shapes(tree, &layout_cx, &mut sink, &Transform::default(), false);
            }
        }
        let mut title = String::new();
        if let Some(tree) = path(root, &["cSld", "spTree"]) {
            shapes(tree, &cx, &mut sink, &Transform::default(), true);
            title = slide_title(tree);
        }
        let notes = target("notesSlide").and_then(|n| self.package.text(&n)).map(|x| notes_text(&x)).unwrap_or_default();
        Some(Slide { hidden: root.attribute("show") == Some("0"), background, elements, title, notes })
    }
}

impl<'a> Context<'a> {
    pub(super) fn clone_shallow(&self) -> Context<'a> {
        Context {
            theme: self.theme,
            colors: self.colors.clone(),
            master: self.master,
            layout: self.layout,
            base: self.base.clone(),
            part: self.part.clone(),
            rels: self.rels.clone(),
            number: self.number,
        }
    }
}

pub(super) fn background(
    xml: &str,
    rels: &HashMap<String, (String, String)>,
    colors: &Colors,
    theme: &Theme,
    media: &mut HashMap<String, Vec<u8>>,
    package: &Package,
) -> Option<Fill> {
    let doc = Document::parse(xml).ok()?;
    let bg = path(doc.root_element(), &["cSld", "bg"])?;
    if let Some(pr) = child(bg, "bgPr") {
        return fill_of(pr, colors, rels, media, package);
    }
    let reference = child(bg, "bgRef")?;
    let idx = num(reference, "idx").unwrap_or(0.0) as usize;
    let color = colors.first(reference);
    let style = if idx >= 1001 { theme.background_styles.get(idx - 1001) } else { theme.fill_styles.get(idx.checked_sub(1)?) }?;
    let wrapped = format!("<w xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">{style}</w>");
    let style_doc = Document::parse(&wrapped).ok()?;
    fill_of(style_doc.root_element(), &colors.with_placeholder(color), rels, media, package)
}

/// The fill inside an element holding one (spPr, bgPr, tcPr, …)
pub(super) fn fill_of(
    node: Node,
    colors: &Colors,
    rels: &HashMap<String, (String, String)>,
    media: &mut HashMap<String, Vec<u8>>,
    package: &Package,
) -> Option<Fill> {
    for f in node.children().filter(Node::is_element) {
        match f.tag_name().name() {
            "noFill" => return None,
            "solidFill" => return colors.first(f).map(Fill::Solid),
            "gradFill" => {
                let mut stops: Vec<(f32, Rgba)> = path(f, &["gsLst"])
                    .map(|l| children(l, "gs").filter_map(|gs| Some((num(gs, "pos")? / 100_000.0, colors.first(gs)?))).collect())
                    .unwrap_or_default();
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                let angle = child(f, "lin").and_then(|l| num(l, "ang")).map(|a| a / 60_000.0).unwrap_or(90.0);
                return match stops.len() {
                    0 => None,
                    1 => Some(Fill::Solid(stops[0].1)),
                    _ => Some(Fill::Gradient(stops, angle)),
                };
            }
            "blipFill" => {
                let id = path(f, &["blip"]).and_then(|b| b.attribute(("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "embed")))?;
                let target = rels.get(id)?.1.clone();
                if !media.contains_key(&target) {
                    let data = package.get(&target)?;
                    media.insert(target.clone(), data);
                }
                return Some(Fill::Picture(target));
            }
            "pattFill" => return child(f, "fgClr").and_then(|c| colors.first(c)).map(Fill::Solid),
            "grpFill" => return None,
            _ => {}
        }
    }
    None
}

pub(super) fn line_of(node: Node, colors: &Colors) -> Option<Option<Line>> {
    let ln = child(node, "ln")?;
    if child(ln, "noFill").is_some() {
        return Some(None);
    }
    let color = child(ln, "solidFill").and_then(|f| colors.first(f)).or_else(|| child(ln, "gradFill").and_then(|g| path(g, &["gsLst", "gs"])).and_then(|gs| colors.first(gs)));
    let width = emu(ln, "w");
    let dash = child(ln, "prstDash").and_then(|d| d.attribute("val")).and_then(|v| match v {
        "solid" => None,
        "dot" | "sysDot" => Some("dotted"),
        "dash" | "sysDash" | "lgDash" => Some("dashed"),
        "dashDot" | "sysDashDot" | "lgDashDot" | "lgDashDotDot" | "sysDashDotDot" => Some("dash-dotted"),
        _ => None,
    });
    let end = |name: &str| {
        let e = child(ln, name)?;
        let kind = LineEnd::parse(e.attribute("type")?)?;
        Some((kind, crate::office::shapes::end_size(e.attribute("w")), crate::office::shapes::end_size(e.attribute("len"))))
    };
    let (head, tail) = (end("headEnd"), end("tailEnd"));
    match (color, width) {
        (None, None) if head.is_none() && tail.is_none() => None,
        (color, width) => Some(Some(Line { color: color.unwrap_or(Rgba { rgb: [0, 0, 0], alpha: 1.0 }), width: width.unwrap_or(0.75), dash, head, tail })),
    }
}
