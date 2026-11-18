//! Runs and inline content, and a paragraph's content laid out at its tab stops

use super::*;

impl<'d> Gen<'d> {
    fn run_markup(&self, text: &str, r: &RunProps) -> String {
        let text = if r.caps { text.to_uppercase() } else { text.to_string() };
        // Word may break after a hyphen before a digit ("REG-|001"); Unicode line
        // breaking forbids it, so a zero-width break opportunity is added there
        let text = if text.contains('-') { allow_break_after_hyphen(&text) } else { text };
        format!("#{}", self.run_expr(&q(&text), r))
    }

    /// `text(..., body)` wrapped in decorations; `body` is a Typst expression.
    fn run_expr(&self, body: &str, r: &RunProps) -> String {
        let mut args = String::new();
        if let Some(font) = &r.font
            && !font.eq_ignore_ascii_case(&self.doc.default_font) && !is_symbol_font(font) {
                let _ = write!(args, "font: {}, ", font_list(font));
            }
        if (r.size - self.doc.default_size).abs() > 0.01 {
            let _ = write!(args, "size: {}, ", pt(r.size));
        }
        if r.bold {
            args.push_str("weight: \"bold\", ");
        }
        if r.italic {
            args.push_str("style: \"italic\", ");
        }
        if let Some(c) = r.color {
            let _ = write!(args, "fill: {}, ", rgb(c));
        }
        if r.spacing.abs() > 0.01 {
            let _ = write!(args, "tracking: {}, ", pt(r.spacing));
        }
        let mut m = format!("text({args}{body})");
        if r.small_caps {
            m = format!("smallcaps({m})");
        }
        if r.underline {
            m = format!("underline({m})");
        }
        if r.strike {
            m = format!("strike({m})");
        }
        if let Some(h) = r.highlight {
            m = format!("highlight(fill: {}, {m})", rgb(h));
        }
        match r.vert {
            VertAlign::Super => format!("super({m})"),
            VertAlign::Sub => format!("sub({m})"),
            VertAlign::Baseline => m,
        }
    }

    /// Markup for a run of inlines without tabs.
    fn inline_markup(&self, inlines: &[Inline]) -> String {
        let mut s = String::new();
        let mut i = 0;
        while i < inlines.len() {
            match &inlines[i] {
                Inline::Text(text, props) => {
                    // Merge consecutive runs with identical formatting
                    let mut merged = text.clone();
                    while let Some(Inline::Text(next, p2)) = inlines.get(i + 1) {
                        if p2 != props {
                            break;
                        }
                        merged.push_str(next);
                        i += 1;
                    }
                    s.push_str(&self.run_markup(&merged, props));
                }
                Inline::Tab => s.push_str("#h(2em)"), // only reached outside tabbed paragraphs
                Inline::AlignTab => s.push_str("#h(1fr)"),
                Inline::TextBox(blocks) => {
                    let inner = self.blocks_markup(blocks, false);
                    s.push_str(&format!("#block(width: 100%)[{inner}]"));
                }
                Inline::LineBreak => s.push_str(if self.justify_breaks.get() { "#linebreak(justify: true)" } else { "#linebreak()" }),
                Inline::Bookmark(name) => {
                    if !self.in_header.get() {
                        s.push_str(&format!("#metadata(none)<ff-bm-{name}>"));
                    }
                }
                Inline::PageRef { bookmark, cached, props } => {
                    if self.doc.bookmarks.contains(bookmark) {
                        let expr = format!("str(counter(page).at(<ff-bm-{bookmark}>).first())");
                        s.push_str(&format!("#context {}", self.run_expr(&expr, props)));
                    } else if !cached.is_empty() {
                        s.push_str(&self.run_markup(cached, props));
                    }
                }
                // Page/column breaks are split out at block level (see `split_breaks`)
                Inline::PageBreak | Inline::ColumnBreak => {}
                Inline::PageNumber(props) => {
                    s.push_str(&format!("#context {}", self.run_expr("counter(page).display()", props)));
                }
                Inline::PageCount(props) => {
                    // Physical page count, like Word's NUMPAGES
                    s.push_str(&format!("#context {}", self.run_expr("str(locate(<ff-end>).page())", props)));
                }
                Inline::SectionPageCount(props) => {
                    let start = format!("locate(<ff-s{}>).page()", self.section);
                    let end = if self.section + 1 < self.doc.sections.len() {
                        format!("(locate(<ff-s{}>).page() - 1)", self.section + 1)
                    } else {
                        "locate(<ff-end>).page()".to_string()
                    };
                    s.push_str(&format!("#context {}", self.run_expr(&format!("str({end} - {start} + 1)"), props)));
                }
                // Fixed to the page: placed by `blocks_markup` in front of the paragraph
                Inline::Image { on_page: true, float: Some(_), .. } if !self.in_header.get() => {}
                Inline::Image { index, width, height, float: Some((dx, dy)), .. } if *width > 0.0 && *height > 0.0 => {
                    if let Some(Some(path)) = self.image_paths.get(*index) {
                        // A zero-size box at the anchor; the offset is from the line's top
                        s.push_str(&format!(
                            "#box(width: 0pt, height: 0pt, place(top + left, dx: {}, dy: {} - 0.8em, image({}, width: {}, height: {}, fit: \"stretch\")))",
                            pt(*dx), pt(*dy), q(path), pt(*width), pt(*height)
                        ));
                    }
                }
                Inline::Image { index, width, height, .. } => {
                    if let Some(Some(path)) = self.image_paths.get(*index) {
                        let (mut w, mut h) = (*width, *height);
                        if w <= 0.0 || h <= 0.0 {
                            s.push_str(&format!("#box(image({}, width: {}))", q(path), pt(self.text_width.min(300.0))));
                        } else {
                            if w > self.text_width {
                                h *= self.text_width / w;
                                w = self.text_width;
                            }
                            s.push_str(&format!("#box(image({}, width: {}, height: {}, fit: \"stretch\"))", q(path), pt(w), pt(h)));
                        }
                    }
                }
            }
            i += 1;
        }
        s
    }

    /// Content of a paragraph: plain, or split at tabs and laid out by `ff-tabbed`.
    /// `extra_stop` is the hanging-indent position, which Word treats as a tab stop.
    pub(super) fn paragraph_content(&self, inlines: &[Inline], tabs: &[TabStop], extra_stop: Option<f32>, first_line_x: f32) -> String {
        if !inlines.iter().any(|i| matches!(i, Inline::Tab)) {
            return self.inline_markup(inlines);
        }
        let segments: Vec<String> = inlines
            .split(|i| matches!(i, Inline::Tab))
            .map(|seg| format!("[{}]", self.inline_markup(seg)))
            .collect();
        let mut stops: Vec<TabStop> = tabs.to_vec();
        if let Some(pos) = extra_stop
            && !stops.iter().any(|t| (t.pos - pos).abs() < 0.5) {
                stops.push(TabStop { pos, align: TabAlign::Left, leader: None });
            }
        stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
        let stops: Vec<String> = stops
            .iter()
            .map(|t| {
                let align = match t.align {
                    TabAlign::Left => "left",
                    TabAlign::Center => "center",
                    TabAlign::Right | TabAlign::Decimal => "right",
                };
                let leader = t.leader.map(|c| q(&c.to_string())).unwrap_or_else(|| "none".into());
                format!("(pos: {}, align: \"{align}\", leader: {leader})", pt(t.pos))
            })
            .collect();
        let stops = if stops.is_empty() { "()".to_string() } else { format!("({},)", stops.join(", ")) };
        format!("#ff-tabbed({stops}, {}, {}, {})", pt(first_line_x), pt(self.doc.default_tab), segments.join(", "))
    }
}
