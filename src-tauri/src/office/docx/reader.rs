//! 
//! The body (body.rs) and tables (tables.rs) are read by further `impl Reader` blocks.

use super::*;

// Reader

pub(super) struct Reader<'p> {
    pub(super) pkg: &'p Package,
    pub(super) styles: HashMap<String, Style>,
    pub(super) default_para_style: Option<String>,
    /// The table style used by tables that name none ("Normal Table")
    pub(super) default_table_style: Option<String>,
    /// Documents from Word 2010 and older (compatibility mode < 15) place tables so that
    /// the text of the first cell, not its border, lines up with the margin
    pub(super) legacy_tables: bool,
    pub(super) doc_run: RunPatch,
    pub(super) doc_para: ParaPatch,
    pub(super) theme: Theme,
    pub(super) numbering: Numbering,
    pub(super) counters: HashMap<String, Vec<u32>>, // numId -> counter per level
    pub(super) images: Vec<Image>,
    pub(super) image_index: HashMap<String, usize>,
    pub(super) bookmarks: std::collections::HashSet<String>,
    pub(super) field: FieldState,
}

impl<'p> Reader<'p> {
    pub(super) fn new(pkg: &'p Package) -> Self {
        let mut r = Reader {
            pkg,
            styles: HashMap::new(),
            default_para_style: None,
            default_table_style: None,
            legacy_tables: false,
            doc_run: RunPatch::default(),
            doc_para: ParaPatch::default(),
            theme: Theme::default(),
            numbering: Numbering::default(),
            counters: HashMap::new(),
            images: vec![],
            image_index: HashMap::new(),
            bookmarks: Default::default(),
            field: FieldState::default(),
        };
        r.load_theme();
        r.load_styles();
        r.load_numbering();
        r
    }

    fn load_theme(&mut self) {
        let Some(text) = self.pkg.text("word/theme/theme1.xml") else { return };
        let Ok(xml) = Xml::parse(&text) else { return };
        let font = |kind: &str| {
            xml.descendants()
                .find(|n| n.tag_name().name() == kind && n.tag_name().namespace() == Some(A))
                .and_then(|f| f.children().find(|c| c.tag_name().name() == "latin"))
                .and_then(|l| l.attribute("typeface"))
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        };
        self.theme.major = font("majorFont");
        self.theme.minor = font("minorFont");
    }

    fn load_styles(&mut self) {
        let Some(text) = self.pkg.text("word/styles.xml") else { return };
        let Ok(xml) = Xml::parse(&text) else { return };
        let root = xml.root_element();
        if let Some(defaults) = w(root, "docDefaults") {
            if let Some(rpr) = w(defaults, "rPrDefault").and_then(|n| w(n, "rPr")) {
                self.doc_run = parse_rpr(rpr);
            }
            if let Some(ppr) = w(defaults, "pPrDefault").and_then(|n| w(n, "pPr")) {
                self.doc_para = parse_ppr(ppr);
            }
        }
        for s in root.children().filter(|c| c.tag_name().name() == "style") {
            let Some(id) = wattr(s, "styleId") else { continue };
            let kind = wattr(s, "type").unwrap_or("paragraph").to_string();
            if kind == "paragraph" && wattr(s, "default").is_some_and(|v| v == "1" || v == "true") {
                self.default_para_style = Some(id.to_string());
            }
            if kind == "table" && wattr(s, "default").is_some_and(|v| v == "1" || v == "true") {
                self.default_table_style = Some(id.to_string());
            }
            let table_borders = w(s, "tblPr").and_then(|t| w(t, "tblBorders")).map(table_borders);
            let mut cell_margins = HashMap::new();
            if let Some(mar) = w(s, "tblPr").and_then(|t| w(t, "tblCellMar")) {
                for (side, alias) in [("top", "top"), ("bottom", "bottom"), ("left", "start"), ("right", "end")] {
                    if let Some(v) = w(mar, side).or_else(|| w(mar, alias)).and_then(|m| wattr(m, "w")).and_then(twips) {
                        cell_margins.insert(side, v);
                    }
                }
            }
            self.styles.insert(
                id.to_string(),
                Style {
                    kind,
                    name: w(s, "name").and_then(val).unwrap_or("").to_string(),
                    based_on: w(s, "basedOn").and_then(val).map(str::to_string),
                    run: w(s, "rPr").map(parse_rpr).unwrap_or_default(),
                    para: w(s, "pPr").map(parse_ppr).unwrap_or_default(),
                    table_borders,
                    cell_margins,
                },
            );
        }
    }

    fn load_numbering(&mut self) {
        let Some(text) = self.pkg.text("word/numbering.xml") else { return };
        let Ok(xml) = Xml::parse(&text) else { return };
        let root = xml.root_element();
        for a in root.children().filter(|c| c.tag_name().name() == "abstractNum") {
            let Some(id) = wattr(a, "abstractNumId") else { continue };
            let mut levels = vec![];
            for l in a.children().filter(|c| c.tag_name().name() == "lvl") {
                let idx: usize = wattr(l, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(levels.len());
                while levels.len() <= idx {
                    levels.push(Level {
                        start: 1,
                        format: "decimal".into(),
                        text: "%1.".into(),
                        para: ParaPatch::default(),
                        run: RunPatch::default(),
                        suffix: MarkerSuffix::Tab,
                    });
                }
                levels[idx] = parse_level(l);
            }
            self.numbering.abstracts.insert(id.to_string(), levels);
        }
        for n in root.children().filter(|c| c.tag_name().name() == "num") {
            let Some(id) = wattr(n, "numId") else { continue };
            let Some(abs) = w(n, "abstractNumId").and_then(val) else { continue };
            let mut overrides = HashMap::new();
            let mut level_overrides = HashMap::new();
            for o in n.children().filter(|c| c.tag_name().name() == "lvlOverride") {
                let Some(lvl) = wattr(o, "ilvl").and_then(|v| v.parse::<usize>().ok()) else { continue };
                if let Some(start) = w(o, "startOverride").and_then(val).and_then(|v| v.parse().ok()) {
                    overrides.insert(lvl, start);
                }
                // A full level definition replaces the abstract one for this list
                if let Some(l) = w(o, "lvl") {
                    level_overrides.insert(lvl, parse_level(l));
                }
            }
            self.numbering.nums.insert(id.to_string(), (abs.to_string(), overrides, level_overrides));
        }
    }

    /// Style chain from the most basic ancestor to `id` itself.
    pub(super) fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out = vec![];
        let mut current = Some(id.to_string());
        let mut guard = 0;
        while let Some(cid) = current {
            let Some(style) = self.styles.get(&cid) else { break };
            out.push(style);
            current = style.based_on.clone();
            guard += 1;
            if guard > 32 {
                break; // cyclic basedOn
            }
        }
        out.reverse();
        out
    }

    pub(super) fn para_style_patches(&self, style_id: Option<&str>) -> (ParaPatch, RunPatch) {
        let mut para = self.doc_para.clone();
        let mut run = self.doc_run.clone();
        let id = style_id.map(str::to_string).or_else(|| self.default_para_style.clone());
        if let Some(id) = id {
            for s in self.chain(&id) {
                para.merge(&s.para);
                run.merge(&s.run);
            }
        }
        (para, run)
    }

    pub(super) fn base_run(&self) -> RunProps {
        let mut base = RunProps::default();
        if let Some(minor) = &self.theme.minor {
            base.font = Some(minor.clone());
        }
        base.size = 10.0; // Word's built-in default when nothing is specified
        base
    }
}

// Sections, header, footer
impl<'p> Reader<'p> {
    pub(super) fn page_setup(&self, sect: Option<Node>) -> PageSetup {
        let mut page = PageSetup::default();
        let Some(sect) = sect else { return page };
        if let Some(sz) = w(sect, "pgSz") {
            if let Some(v) = wattr(sz, "w").and_then(twips) {
                page.width = v;
            }
            if let Some(v) = wattr(sz, "h").and_then(twips) {
                page.height = v;
            }
        }
        if let Some(m) = w(sect, "pgMar") {
            let get = |k: &str| wattr(m, k).and_then(twips).map(f32::abs);
            page.margin_top = get("top").unwrap_or(page.margin_top);
            page.margin_bottom = get("bottom").unwrap_or(page.margin_bottom);
            page.margin_left = get("left").or(get("start")).unwrap_or(page.margin_left);
            page.margin_right = get("right").or(get("end")).unwrap_or(page.margin_right);
            page.header_distance = get("header").unwrap_or(page.header_distance);
            page.footer_distance = get("footer").unwrap_or(page.footer_distance);
        }
        if let Some(cols) = w(sect, "cols") {
            page.columns = wattr(cols, "num").and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 6);
        }
        page
    }

    pub(super) fn part_blocks(&mut self, target: &str) -> Vec<Block> {
        let Some(text) = self.pkg.text(target) else { return vec![] };
        let Ok(xml) = Xml::parse(&text) else { return vec![] };
        let part_rels = relationships(self.pkg, target);
        let body_fields = std::mem::take(&mut self.field);
        let blocks = self.blocks(xml.root_element(), &part_rels);
        self.field = body_fields;
        blocks
    }

    pub(super) fn section_start(sect: Node) -> SectionStart {
        match w(sect, "type").and_then(val) {
            Some("continuous") => SectionStart::Continuous,
            Some("evenPage") => SectionStart::EvenPage,
            Some("oddPage") => SectionStart::OddPage,
            _ => SectionStart::NextPage,
        }
    }
}
