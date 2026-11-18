//! Sections: page setup, columns, numbering, and their headers and footers

use super::*;

impl<'d> Gen<'d> {
    /// A header/footer as a Typst expression. Variants for the section's first page and
    /// even pages are picked per page, like Word. The content sits `distance` from the
    /// page edge inside a box exactly as tall as the margin.
    /// Content of each variant of a header/footer: default, then first page and even
    /// pages when the section has them
    fn hf_variants(&self, hf: &HeaderFooter) -> (String, Option<String>, Option<String>) {
        let render = |blocks: &[Block]| -> String {
            self.in_header.set(true);
            let mut inner = self.blocks_markup(blocks, false).trim().to_string();
            self.in_header.set(false);
            // Word counts the last paragraph's space after in the header's height
            // (Typst would drop weak spacing at the end of the block)
            if let Some(Block::Paragraph(p)) = blocks.last()
                && p.props.space_after > 0.0 {
                    inner.push_str(&format!("\n#v({})", pt(p.props.space_after)));
                }
            inner
        };
        (render(&hf.default), hf.first.as_ref().map(|b| render(b)), hf.even.as_ref().map(|b| render(b)))
    }

    /// A header or footer placed on the page itself, the way Word does: the header's
    /// top `distance` below the paper's top edge, the footer's bottom `distance` above
    /// the bottom edge. Variants: first page of the section, even pages, the rest.
    fn header_footer(&self, variants: &(String, Option<String>, Option<String>), width: f32, distance: f32, is_header: bool) -> String {
        let place = |inner: &str| -> String {
            if inner.is_empty() {
                return "none".into();
            }
            let left = pt(self.doc.sections[self.section].page.margin_left);
            if is_header {
                format!("place(top + left, dx: {left}, dy: {}, block(width: {})[{inner}])", pt(distance), pt(width))
            } else {
                format!("place(bottom + left, dx: {left}, dy: {}, block(width: {})[{inner}])", pt(-distance), pt(width))
            }
        };
        let (default, first, even) = variants;
        let default = place(default);
        if first.is_none() && even.is_none() {
            return default;
        }
        let first = first.as_ref().map(|b| place(b)).unwrap_or_else(|| default.clone());
        let even = even.as_ref().map(|b| place(b)).unwrap_or_else(|| default.clone());
        format!(
            "context {{ let p = here().page(); if p == locate(<ff-s{}>).page() {{ {first} }} else if calc.even(p) {{ {even} }} else {{ {default} }} }}",
            self.section
        )
    }

    /// Height of the tallest variant, measured by Typst itself during layout
    fn hf_height(variants: &(String, Option<String>, Option<String>), width: f32) -> String {
        let (default, first, even) = variants;
        let parts: Vec<String> = std::iter::once(default)
            .chain(first.iter())
            .chain(even.iter())
            .filter(|v| !v.is_empty())
            .map(|v| format!("measure(block(width: {})[{v}]).height", pt(width)))
            .collect();
        if parts.is_empty() {
            "0pt".into()
        } else {
            format!("calc.max(0pt, {})", parts.join(", "))
        }
    }

    pub(super) fn section(&self, sec: &Section, index: usize, previous: Option<&Section>) -> String {
        let page = &sec.page;
        let mut s = String::new();
        let next_starts_page = self.doc.sections.get(index + 1).is_some_and(|n| n.start != SectionStart::Continuous);
        self.trim_trailing_break.set(next_starts_page);

        // A continuous section on a page of the same size keeps flowing on that page
        let continuous = index > 0
            && sec.start == SectionStart::Continuous
            && previous.is_some_and(|p| (p.page.width - page.width).abs() < 1.0 && (p.page.height - page.height).abs() < 1.0);
        if continuous {
            let _ = writeln!(s, "#metadata(none) <ff-s{index}>");
            s.push_str(&self.blocks_markup(&sec.blocks, true));
            return s;
        }

        match sec.start {
            SectionStart::EvenPage if index > 0 => s.push_str("#pagebreak(weak: true, to: \"even\")\n"),
            SectionStart::OddPage if index > 0 => s.push_str("#pagebreak(weak: true, to: \"odd\")\n"),
            // New page settings no longer start a page by themselves inside `context`
            _ if index > 0 => s.push_str("#pagebreak(weak: true)\n"),
            _ => {}
        }

        // Word moves the body down/up so a tall header/footer always fits. The
        // header/footer heights are measured during layout, so the body starts exactly
        // where it does in Word; the page settings then apply to this section's
        // content, which `generate` closes with `]}`.
        let width = (page.width - page.margin_left - page.margin_right).max(72.0);
        let header = self.hf_variants(&sec.header);
        let footer = self.hf_variants(&sec.footer);
        // The body starts below the header shown on the page: the first page of the
        // section may show a different (taller or shorter) one than the others, so
        // that page's body is moved by the difference
        let rest = (header.0.clone(), None, header.2.clone());
        let first_page = (header.1.clone().unwrap_or_else(|| header.0.clone()), None, None);
        let _ = writeln!(
            s,
            "#context {{\nlet ff-top = calc.max({top}, {hd} + {})\nlet ff-first = calc.max({top}, {hd} + {})\nlet ff-bottom = calc.max({}, {} + {})",
            Self::hf_height(&rest, width),
            Self::hf_height(&first_page, width),
            pt(page.margin_bottom),
            pt(page.footer_distance),
            Self::hf_height(&footer, width),
            top = pt(page.margin_top),
            hd = pt(page.header_distance),
        );
        let header = self.header_footer(&header, width, page.header_distance, true);
        let footer = self.header_footer(&footer, width, page.footer_distance, false);
        let _ = writeln!(
            s,
            "set page(width: {}, height: {}, margin: (top: ff-top, bottom: ff-bottom, left: {}, right: {}), columns: {}, header: none, footer: none, background: {{ {header}; {footer} }})\n[",
            pt(page.width),
            pt(page.height),
            pt(page.margin_left),
            pt(page.margin_right),
            page.columns.max(1),
        );
        s.push_str("#v(ff-first - ff-top)\n");
        let _ = writeln!(s, "#metadata(none) <ff-s{index}>");
        if let Some(n) = sec.page_start {
            let _ = writeln!(s, "#counter(page).update({n})");
        }
        self.at_section_start.set(true);
        s.push_str(&self.blocks_markup(&sec.blocks, true));
        s
    }
}
