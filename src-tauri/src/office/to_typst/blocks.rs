//! The generator: a document body's blocks with Word's spacing between them, and where
//! page and column breaks may fall. Runs, paragraphs, tables and sections are written by
//! further `impl Gen` blocks (inline.rs, paragraph.rs, table.rs, section.rs)

use super::*;

pub(super) struct Gen<'d> {
    pub(super) doc: &'d Document,
    pub(super) text_width: f32,
    pub(super) image_paths: &'d [Option<String>],
    /// Index of the section being written (for SECTIONPAGES and first-page headers)
    pub(super) section: usize,
    /// Inside a header/footer: repeated on every page, so no bookmark labels there
    pub(super) in_header: std::cell::Cell<bool>,
    /// At the very start of a section (already on a new page): page breaks before the
    /// first content would only produce a blank page
    pub(super) at_section_start: std::cell::Cell<bool>,
    /// The next section starts on a new page anyway: a page break at the very end of
    /// this one would leave a blank page (Word merges the two)
    pub(super) trim_trailing_break: std::cell::Cell<bool>,
    /// How many tables deep the current content is (0 = body text)
    pub(super) table_depth: std::cell::Cell<u32>,
    /// The paragraph being written is justified, and so are its lines before manual
    /// line breaks (Word's default)
    pub(super) justify_breaks: std::cell::Cell<bool>,
}

impl<'d> Gen<'d> {
    /// Emits blocks with Word-like vertical spacing between them. Page breaks are only
    /// allowed at the top level (`top`), so they are dropped inside table cells.
    pub(super) fn blocks_markup(&self, blocks: &[Block], top: bool) -> String {
        let mut blocks = split_breaks(blocks, self.doc.legacy_layout);
        if top && self.trim_trailing_break.get() {
            // Drop invisible trailing empty paragraphs and a page break before them: they
            // can only spill onto an otherwise blank page
            let is_empty = |p: &Piece| matches!(p, Piece::Block(Block::Paragraph(par)) if par.inlines.is_empty() && par.marker.is_none());
            while blocks.last().is_some_and(|p| is_empty(p) || matches!(p, Piece::PageBreak)) {
                blocks.pop();
            }
        }
        let mut s = String::new();
        let mut prev_after: Option<(f32, Option<String>, bool)> = None;
        for item in blocks {
            match item {
                Piece::PageBreak => {
                    if top && !self.at_section_start.get() {
                        s.push_str("#pagebreak(weak: true)\n");
                    }
                    prev_after = None;
                }
                Piece::ColumnBreak => {
                    if top && self.doc.sections.get(self.section).is_some_and(|sec| sec.page.columns > 1) {
                        s.push_str("#colbreak(weak: true)\n");
                    }
                }
                Piece::Block(Block::Paragraph(p)) => {
                    if top && p.props.page_break_before && !self.at_section_start.get() {
                        s.push_str("#pagebreak(weak: true)\n");
                        prev_after = None;
                    }
                    if let Some((after, style, contextual)) = &prev_after {
                        let same_style = style.is_some() && *style == p.props.style_id;
                        let gap = if same_style && (*contextual || p.props.contextual_spacing) {
                            0.0
                        } else {
                            after + p.props.space_before
                        };
                        if gap > 0.0 {
                            let _ = writeln!(s, "#v({}, weak: true)", pt(gap));
                        }
                    } else if p.props.space_before > 0.0 && (!s.is_empty() || !top) {
                        // Word keeps the space before the first paragraph of a table cell
                        // or header (only the top of the body's page drops it here)
                        let _ = writeln!(s, "#v({})", pt(p.props.space_before));
                    }
                    // Pictures fixed to the page (e.g. a page's background) are placed on the
                    // page this paragraph starts on, measured from the paper's corner
                    if top && !self.in_header.get() {
                        let margin_left = self.doc.sections[self.section].page.margin_left;
                        for inline in &p.inlines {
                            if let Inline::Image { index, width, height, float: Some((x, y)), on_page: true } = inline
                                && let Some(Some(path)) = self.image_paths.get(*index) {
                                    let _ = writeln!(
                                        s,
                                        "#place(top + left, dx: {}, dy: {} - ff-top, image({}, width: {}, height: {}, fit: \"stretch\"))",
                                        pt(x - margin_left),
                                        pt(*y),
                                        q(path),
                                        pt(*width),
                                        pt(*height)
                                    );
                                }
                        }
                    }
                    s.push_str(&self.paragraph(&p));
                    s.push('\n');
                    if top {
                        self.at_section_start.set(false);
                    }
                    prev_after = Some((p.props.space_after, p.props.style_id.clone(), p.props.contextual_spacing));
                }
                Piece::Block(Block::Table(t)) => {
                    if let Some((after, _, _)) = prev_after
                        && after > 0.0 {
                            let _ = writeln!(s, "#v({}, weak: true)", pt(after));
                        }
                    s.push_str(&self.table(&t));
                    s.push('\n');
                    if top {
                        self.at_section_start.set(false);
                    }
                    prev_after = Some((0.0, None, false));
                }
            }
        }
        s
    }

    /// Rough height of blocks laid out at `width`, used to grow page margins so tall
    /// headers and footers fit, as Word does.
    pub(super) fn estimate_height(&self, blocks: &[Block], width: f32) -> f32 {
        let mut h = 0.0;
        for b in blocks {
            match b {
                Block::Paragraph(p) => {
                    let size = self.main_size(p);
                    let chars: usize = p
                        .inlines
                        .iter()
                        .map(|i| match i {
                            Inline::Text(t, _) => t.chars().count(),
                            _ => 1,
                        })
                        .sum();
                    let usable = (width - p.props.indent_left - p.props.indent_right).max(36.0);
                    let wraps = ((chars as f32 * size * 0.5) / usable).ceil().max(1.0);
                    let breaks = p.inlines.iter().filter(|i| matches!(i, Inline::LineBreak)).count() as f32;
                    let line = self.line_box(p).0;
                    let images = p
                        .inlines
                        .iter()
                        .filter_map(|i| match i {
                            Inline::Image { height, float: None, .. } => Some(*height),
                            Inline::TextBox(inner) => Some(self.estimate_height(inner, width)),
                            _ => None,
                        })
                        .fold(0.0, f32::max);
                    h += ((wraps + breaks) * line).max(images) + p.props.space_before + p.props.space_after;
                }
                Block::Table(t) => {
                    let total: f32 = t.columns.iter().sum();
                    let scale = if total > width && total > 0.0 { width / total } else { 1.0 };
                    for row in &t.rows {
                        let mut col = 0;
                        let mut row_h: f32 = 0.0;
                        for cell in &row.cells {
                            let cw: f32 = t.columns.iter().skip(col).take(cell.span).sum::<f32>() * scale;
                            col += cell.span;
                            if cell.vmerge != VMerge::Continue {
                                row_h = row_h.max(self.estimate_height(&cell.blocks, cw.max(24.0) - 10.0));
                            }
                        }
                        h += row_h + 3.0;
                    }
                }
            }
        }
        h
    }
}

// Short-lived (split_breaks hands them straight on), so the Block stays inline rather
// than boxed: boxing would only add an allocation per block
#[allow(clippy::large_enum_variant)]
enum Piece {
    Block(Block),
    PageBreak,
    ColumnBreak,
}

/// Typst only allows page breaks between blocks, so a paragraph containing a manual
/// page break is split into two paragraphs around it.
/// `legacy`: in documents from Word 2010 and older the paragraph mark after a page
/// break moves to the next page, where it takes up an empty line.
fn split_breaks(blocks: &[Block], legacy: bool) -> Vec<Piece> {
    let mut out = vec![];
    for b in blocks {
        match b {
            Block::Paragraph(p) if p.inlines.iter().any(|i| matches!(i, Inline::PageBreak | Inline::ColumnBreak)) => {
                let mut current = Paragraph { props: p.props.clone(), inlines: vec![], marker: p.marker.clone(), mark: p.mark.clone() };
                let mut first = true;
                for inline in &p.inlines {
                    match inline {
                        Inline::PageBreak | Inline::ColumnBreak => {
                            let has_content = !current.inlines.is_empty() || (first && current.marker.is_some());
                            let mut part = std::mem::replace(
                                &mut current,
                                Paragraph { props: p.props.clone(), inlines: vec![], marker: None, mark: p.mark.clone() },
                            );
                            if has_content {
                                if !first {
                                    part.props.page_break_before = false;
                                }
                                out.push(Piece::Block(Block::Paragraph(part)));
                            }
                            current.props.page_break_before = false;
                            current.props.space_before = 0.0;
                            out.push(if matches!(inline, Inline::PageBreak) { Piece::PageBreak } else { Piece::ColumnBreak });
                            first = false;
                        }
                        other => current.inlines.push(other.clone()),
                    }
                }
                if !current.inlines.is_empty() || (legacy && matches!(p.inlines.last(), Some(Inline::PageBreak))) {
                    out.push(Piece::Block(Block::Paragraph(current)));
                }
            }
            other => out.push(Piece::Block(other.clone())),
        }
    }
    out
}
