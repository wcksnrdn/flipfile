//! The document body: paragraphs, runs, fields, math, drawings and list markers

use super::*;

impl<'p> Reader<'p> {
    pub(super) fn blocks(&mut self, container: Node, rels: &HashMap<String, String>) -> Vec<Block> {
        let mut out = vec![];
        for child in container.children().filter(|c| c.is_element()) {
            match (child.tag_name().namespace(), child.tag_name().name()) {
                (Some(W), "p") => self.paragraph(child, rels, &mut out),
                (Some(W), "tbl") => out.push(Block::Table(self.table(child, rels))),
                (Some(W), "sdt") => {
                    if let Some(content) = w(child, "sdtContent") {
                        out.extend(self.blocks(content, rels));
                    }
                }
                (Some(W), "customXml" | "ins" | "smartTag") => out.extend(self.blocks(child, rels)),
                _ => {}
            }
        }
        out
    }

    pub(super) fn paragraph(&mut self, p: Node, rels: &HashMap<String, String>, out: &mut Vec<Block>) {
        let ppr = w(p, "pPr");
        let style_id = ppr.and_then(|n| w(n, "pStyle")).and_then(val).map(str::to_string);
        let (mut para_patch, style_run) = self.para_style_patches(style_id.as_deref());
        let direct_para = ppr.map(parse_ppr).unwrap_or_default();

        // Numbering may come from the style or from the paragraph itself
        let num = direct_para.num.clone().or(para_patch.num.clone());
        let mut marker = None;
        let mut level_patch = ParaPatch::default();
        let base = self.base_run();
        let para_run = style_run.apply(&base, &self.theme);
        if let Some((num_id, level)) = num.filter(|(id, _)| id != "0")
            && let Some((text, lvl_para, lvl_run, suffix)) = self.next_marker(&num_id, level) {
                level_patch = lvl_para;
                let mut run = para_run.clone();
                run = lvl_run.apply(&run, &self.theme);
                if let Some(t) = text {
                    marker = Some(ListMarker { text: t, run, suffix });
                }
            }
        // Numbering indents sit between the style and the paragraph's own formatting
        para_patch.merge(&level_patch);
        para_patch.merge(&direct_para);
        let mut props = para_patch.apply(&ParaProps::default());
        props.style_id = style_id.clone();

        let mut inlines = vec![];
        // Fields can span paragraphs (a table of contents does), so the state persists
        let mut field = std::mem::take(&mut self.field);
        self.inlines(p, rels, &para_run, &mut inlines, &mut field);
        self.field = field;
        let display_math = p.children().any(|c| c.tag_name().name() == "oMathPara" && c.tag_name().namespace() == Some(M))
            && !p.children().any(|c| c.tag_name().name() == "r" && c.tag_name().namespace() == Some(W));
        if display_math && direct_para.align.is_none() {
            props.align = Align::Center;
        }

        let mark = match ppr.and_then(|n| w(n, "rPr")) {
            Some(rpr) => parse_rpr(rpr).apply(&para_run, &self.theme),
            None => para_run.clone(),
        };
        out.push(Block::Paragraph(Paragraph { props, inlines, marker, mark }));
    }

    fn inlines(&mut self, node: Node, rels: &HashMap<String, String>, para_run: &RunProps, out: &mut Vec<Inline>, field: &mut FieldState) {
        for c in node.children().filter(|c| c.is_element()) {
            match (c.tag_name().namespace(), c.tag_name().name()) {
                (Some(W), "r") => self.run(c, rels, para_run, out, field),
                (Some(W), "hyperlink" | "smartTag" | "customXml" | "ins" | "fldSimple" | "bdo" | "dir") => {
                    if c.tag_name().name() == "fldSimple" {
                        match field_name(wattr(c, "instr").unwrap_or("")).as_str() {
                            "PAGE" => {
                                out.push(Inline::PageNumber(para_run.clone()));
                                continue;
                            }
                            "NUMPAGES" => {
                                out.push(Inline::PageCount(para_run.clone()));
                                continue;
                            }
                            "SECTIONPAGES" => {
                                out.push(Inline::SectionPageCount(para_run.clone()));
                                continue;
                            }
                            _ => {}
                        }
                    }
                    self.inlines(c, rels, para_run, out, field)
                }
                (Some(W), "sdt") => {
                    if let Some(content) = w(c, "sdtContent") {
                        self.inlines(content, rels, para_run, out, field);
                    }
                }
                // Deleted text (tracked changes) is not part of the final document
                (Some(W), "del" | "moveFrom") => {}
                (Some(W), "bookmarkStart") => {
                    if let Some(name) = wattr(c, "name") {
                        let label = bookmark_label(name);
                        self.bookmarks.insert(label.clone());
                        out.push(Inline::Bookmark(label));
                    }
                }
                // Equations: their text, in reading order (variables in italic, as Word shows them)
                (Some(M), "oMathPara" | "oMath") => self.math(c, para_run, out),
                _ => {}
            }
        }
    }

    pub(super) fn run(&mut self, r: Node, rels: &HashMap<String, String>, para_run: &RunProps, out: &mut Vec<Inline>, field: &mut FieldState) {
        let rpr = w(r, "rPr");
        let mut patch = RunPatch::default();
        if let Some(style) = rpr.and_then(|n| w(n, "rStyle")).and_then(val) {
            // Word drops the blue "Hyperlink" look from table-of-contents entries when it
            // updates them, so it is ignored there
            let skip = field.in_toc() && self.styles.get(style).is_some_and(|s| s.name.eq_ignore_ascii_case("hyperlink"));
            if !skip {
                for s in self.chain(style) {
                    if s.kind == "character" {
                        patch.merge(&s.run);
                    }
                }
            }
        }
        if let Some(rpr) = rpr {
            patch.merge(&parse_rpr(rpr));
        }
        let props = patch.apply(para_run, &self.theme);
        if props.hidden {
            return;
        }

        for c in r.children().filter(|c| c.is_element()) {
            let name = c.tag_name().name();
            // Complex fields: keep PAGE/NUMPAGES live, otherwise show the cached result
            match name {
                "fldChar" => {
                    match wattr(c, "fldCharType") {
                        Some("begin") => field.stack.push(FieldFrame::default()),
                        Some("separate") => {
                            let visible_outside = field.stack.len() <= 1 || field.stack[..field.stack.len() - 1].iter().all(|f| f.in_result && !f.replaced);
                            if let Some(top) = field.stack.last_mut() {
                                top.in_result = true;
                                // Page numbers stay live; everything else shows Word's cached result
                                let live = match field_name(&top.instr).as_str() {
                                    "PAGEREF" => top.instr.split_whitespace().nth(1).map(|b| Inline::PageRef {
                                        bookmark: bookmark_label(b),
                                        cached: String::new(),
                                        props: props.clone(),
                                    }),
                                    "PAGE" => Some(Inline::PageNumber(props.clone())),
                                    "NUMPAGES" => Some(Inline::PageCount(props.clone())),
                                    "SECTIONPAGES" => Some(Inline::SectionPageCount(props.clone())),
                                    _ => None,
                                };
                                if let Some(live) = live {
                                    top.replaced = true;
                                    if visible_outside {
                                        out.push(live);
                                    }
                                }
                            }
                        }
                        Some("end") => {
                            field.stack.pop();
                        }
                        _ => {}
                    }
                    continue;
                }
                "instrText" => {
                    if let Some(top) = field.stack.last_mut()
                        && !top.in_result {
                            top.instr.push_str(c.text().unwrap_or(""));
                        }
                    continue;
                }
                _ => {}
            }
            if !field.visible() {
                // Keep a PAGEREF's cached value as its fallback
                if name == "t" && field.stack.last().is_some_and(|f| f.replaced && f.in_result && field_name(&f.instr) == "PAGEREF")
                    && let Some(Inline::PageRef { cached, .. }) = out.iter_mut().rev().find(|i| matches!(i, Inline::PageRef { .. })) {
                        cached.push_str(c.text().unwrap_or(""));
                    }
                continue; // field code, or a cached value replaced by a live field
            }
            match name {
                "t" => {
                    let text = c.text().unwrap_or("");
                    if !text.is_empty() {
                        out.push(Inline::Text(text.to_string(), props.clone()));
                    }
                }
                "tab" => out.push(Inline::Tab),
                "ptab" => out.push(Inline::AlignTab),
                // Newer drawing markup with a fallback for old readers: take the first choice
                "AlternateContent" => {
                    if let Some(choice) = c.children().find(|n| n.tag_name().name() == "Choice").or_else(|| c.children().find(|n| n.tag_name().name() == "Fallback")) {
                        for item in choice.children().filter(|n| n.is_element()) {
                            match item.tag_name().name() {
                                "drawing" => self.drawing(item, rels, out),
                                "pict" => self.vml_image(item, rels, out),
                                _ => {}
                            }
                        }
                    }
                }
                "br" | "cr" => out.push(match wattr(c, "type") {
                    Some("page") => Inline::PageBreak,
                    Some("column") => Inline::ColumnBreak,
                    _ => Inline::LineBreak,
                }),
                "noBreakHyphen" => out.push(Inline::Text("\u{2011}".into(), props.clone())),
                "softHyphen" => out.push(Inline::Text("\u{00AD}".into(), props.clone())),
                "sym" => {
                    let ch = wattr(c, "char").and_then(|h| u32::from_str_radix(h, 16).ok()).map(symbol_char);
                    if let Some(ch) = ch {
                        out.push(Inline::Text(ch.to_string(), props.clone()));
                    }
                }
                "drawing" => self.drawing(c, rels, out),
                "pict" | "object" => self.vml_image(c, rels, out),
                "lastRenderedPageBreak" => {}
                _ => {}
            }
        }
    }

    fn math(&self, node: Node, para_run: &RunProps, out: &mut Vec<Inline>) {
        for r in node.descendants().filter(|n| n.tag_name().name() == "r" && n.tag_name().namespace() == Some(M)) {
            let text: String = r
                .children()
                .filter(|c| c.tag_name().name() == "t" && c.tag_name().namespace() == Some(M))
                .filter_map(|t| t.text())
                .collect();
            if text.is_empty() {
                continue;
            }
            let mut props = match w(r, "rPr") {
                Some(rpr) => parse_rpr(rpr).apply(para_run, &self.theme),
                None => para_run.clone(),
            };
            // `m:sty val="p"` = plain (upright); otherwise letters are italic in Word's math
            let plain = r
                .children()
                .find(|c| c.tag_name().name() == "rPr" && c.tag_name().namespace() == Some(M))
                .and_then(|p| p.children().find(|c| c.tag_name().name() == "sty"))
                .and_then(|s| s.attribute((M, "val")))
                == Some("p");
            props.italic = !plain && text.chars().any(char::is_alphabetic);
            out.push(Inline::Text(text, props));
        }
    }

    fn drawing(&mut self, d: Node, rels: &HashMap<String, String>, out: &mut Vec<Inline>) {
        let Some(holder) = d.children().find(|c| c.is_element() && (c.tag_name().name() == "inline" || c.tag_name().name() == "anchor")) else { return };
        let extent = holder.children().find(|c| c.tag_name().name() == "extent" && c.tag_name().namespace() == Some(WP));
        let (cx, cy) = extent
            .map(|e| {
                (
                    e.attribute("cx").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
                    e.attribute("cy").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
                )
            })
            .unwrap_or((0.0, 0.0));
        if let Some(text_box) = holder.descendants().find(|n| n.tag_name().name() == "txbxContent" && n.tag_name().namespace() == Some(W)) {
            let blocks = self.blocks(text_box, rels);
            if !blocks.is_empty() {
                out.push(Inline::TextBox(blocks));
            }
            return;
        }
        let blip = holder.descendants().find(|n| n.tag_name().name() == "blip" && n.tag_name().namespace() == Some(A));
        let Some(rid) = blip.and_then(|b| b.attribute((R, "embed"))) else { return };
        // Anchored without text wrapping: positioned over the page, taking no space.
        // Only offsets relative to the anchoring column/paragraph can be placed reliably.
        let floating = holder.tag_name().name() == "anchor"
            && holder.children().any(|c| c.tag_name().namespace() == Some(WP) && c.tag_name().name() == "wrapNone");
        let position = |axis: &str| holder.children().find(|c| c.tag_name().namespace() == Some(WP) && c.tag_name().name() == axis);
        let relative = |axis: &str| position(axis).and_then(|p| p.attribute("relativeFrom")).unwrap_or("column");
        // Fixed to the page (pictures behind/in front of the text placed on the paper)
        let on_page = floating && relative("positionH") == "page" && relative("positionV") == "page";
        let float = floating.then(|| {
            let offset = |axis: &str| -> f32 {
                position(axis)
                    .filter(|p| on_page || matches!(p.attribute("relativeFrom"), Some("column" | "character" | "paragraph" | "line")))
                    .and_then(|p| p.children().find(|c| c.tag_name().name() == "posOffset"))
                    .and_then(|o| o.text())
                    .and_then(|t| t.trim().parse::<f64>().ok())
                    .map(|v| (v / 12700.0) as f32)
                    .unwrap_or(0.0)
            };
            (offset("positionH"), offset("positionV"))
        });
        if let Some(index) = self.image(rid, rels) {
            out.push(Inline::Image { index, width: (cx / 12700.0) as f32, height: (cy / 12700.0) as f32, float, on_page });
        }
    }

    fn vml_image(&mut self, pict: Node, rels: &HashMap<String, String>, out: &mut Vec<Inline>) {
        if let Some(text_box) = pict.descendants().find(|n| n.tag_name().name() == "txbxContent" && n.tag_name().namespace() == Some(W)) {
            let blocks = self.blocks(text_box, rels);
            if !blocks.is_empty() {
                out.push(Inline::TextBox(blocks));
            }
            return;
        }
        let Some(data) = pict.descendants().find(|n| n.tag_name().name() == "imagedata" && n.tag_name().namespace() == Some(V)) else { return };
        let Some(rid) = data.attribute((R, "id")) else { return };
        let style = pict.descendants().find(|n| n.tag_name().name() == "shape").and_then(|s| s.attribute("style")).unwrap_or("");
        let dim = |key: &str| -> f32 {
            style
                .split(';')
                .find_map(|kv| kv.trim().strip_prefix(key).and_then(|v| v.strip_prefix(':')))
                .and_then(|v| v.trim().strip_suffix("pt").and_then(|n| n.parse().ok()))
                .unwrap_or(0.0)
        };
        if let Some(index) = self.image(rid, rels) {
            out.push(Inline::Image { index, width: dim("width"), height: dim("height"), float: None, on_page: false });
        }
    }

    fn image(&mut self, rid: &str, rels: &HashMap<String, String>) -> Option<usize> {
        let target = rels.get(rid)?;
        if let Some(&i) = self.image_index.get(target) {
            return Some(i);
        }
        let data = self.pkg.files.get(target)?.clone();
        let ext = target.rsplit('.').next().unwrap_or("").to_lowercase();
        self.images.push(Image { data, ext });
        let index = self.images.len() - 1;
        self.image_index.insert(target.clone(), index);
        Some(index)
    }

    /// Advances the list counter and returns the marker text plus the level's formatting.
    fn next_marker(&mut self, num_id: &str, level: usize) -> Option<(Option<String>, ParaPatch, RunPatch, MarkerSuffix)> {
        let (abs_id, overrides, level_overrides) = self.numbering.nums.get(num_id)?.clone();
        let mut levels = self.numbering.abstracts.get(&abs_id).cloned().unwrap_or_default();
        for (i, l) in level_overrides {
            while levels.len() <= i {
                levels.push(l.clone());
            }
            levels[i] = l;
        }
        let lvl = levels.get(level)?.clone();
        let starts: Vec<u32> = levels.iter().enumerate().map(|(i, l)| *overrides.get(&i).unwrap_or(&l.start)).collect();

        // Lists built on the same definition continue each other's numbering, as in
        // Word, unless this list explicitly restarts (startOverride)
        let key = if overrides.is_empty() { format!("abstract:{abs_id}") } else { format!("num:{num_id}") };
        let counters = self.counters.entry(key).or_insert_with(|| vec![0; levels.len().max(9)]);
        if counters.len() <= level {
            counters.resize(level + 1, 0);
        }
        counters[level] = if counters[level] == 0 { starts[level] } else { counters[level] + 1 };
        for deeper in counters.iter_mut().skip(level + 1) {
            *deeper = 0;
        }
        let snapshot = counters.clone();

        if lvl.format == "none" {
            return Some((None, lvl.para, lvl.run, lvl.suffix));
        }
        let text = if lvl.format == "bullet" {
            bullet_char(&lvl.text, lvl.run.font.as_deref())
        } else {
            let mut t = lvl.text.clone();
            for (i, l) in levels.iter().enumerate().take(level + 1) {
                let n = if snapshot[i] == 0 { starts[i] } else { snapshot[i] };
                let fmt = if lvl.format == "decimal" && i != level { "decimal" } else { l.format.as_str() };
                t = t.replace(&format!("%{}", i + 1), &format_number(n, fmt));
            }
            t
        };
        Some((Some(text), lvl.para, lvl.run, lvl.suffix))
    }
}
