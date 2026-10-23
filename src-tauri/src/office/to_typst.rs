// Turns the Word document model into Typst markup that reproduces Word's layout:
// sections (page size, orientation, margins, columns, numbering), headers and footers
// (first/even variants, inherited between sections, margins that grow to fit them),
// paragraph spacing and line height, indents, real tab stops (measured during layout),
// lists, tables (per-side borders, merged cells, repeating header rows) and images.
//
// Every piece of document text is emitted as an escaped Typst *string* (never as
// markup), so no content can change the meaning of the generated code.

use std::fmt::Write as _;

use super::docx::{
    Align, Block, Border, Cell, Document, HeaderFooter, Inline, LineSpacing, MarkerSuffix, Paragraph, RunProps, Section,
    SectionStart, TabAlign, TabStop, Table, VMerge, VertAlign,
};


/// Layout helper used by paragraphs with tabs: places each segment at the next tab
/// stop (left/centre/right/decimal, with optional leader), measuring the text as it
/// is laid out. Positions are relative to the left margin, like Word's.
pub(super) const PREAMBLE: &str = r#"
#let ff-tabbed(stops, start, step, ..segs) = context {
  let segs = segs.pos()
  let x = start
  let out = []
  for (i, s) in segs.enumerate() {
    let w = measure(s).width
    if i > 0 {
      let found = none
      for st in stops {
        if found == none and st.pos > x + 0.5pt { found = st }
      }
      if found == none {
        found = (pos: (calc.floor(x / step) + 1) * step, align: "left", leader: none)
      }
      let target = if found.align == "right" { found.pos - w } else if found.align == "center" { found.pos - w / 2 } else { found.pos }
      let gap = if target > x { target - x } else { 0pt }
      out += if found.leader != none and gap > 0pt { box(width: gap, repeat(gap: 0.12em, found.leader)) } else { h(gap) }
      x = x + gap
    }
    out += s
    x = x + w
  }
  out
}
"#;

pub struct Output {
    pub markup: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Escaped Typst string literal.
fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn pt(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    format!("{:.2}pt", v)
}

fn rgb(c: [u8; 3]) -> String {
    format!("rgb(\"#{:02x}{:02x}{:02x}\")", c[0], c[1], c[2])
}

fn stroke(b: Option<Border>) -> String {
    match b {
        Some(b) => format!("{} + {}", pt(b.width), rgb(b.color)),
        None => "none".into(),
    }
}

/// The document font first, then look-alikes that are commonly installed, so text
/// keeps a similar look and width when the original font is missing.
pub fn font_list(name: &str) -> String {
    let quoted: Vec<String> = font_names(name).iter().map(|n| q(n)).collect();
    format!("({})", quoted.join(", "))
}

fn font_names(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let fallbacks: &[&str] = if ["courier", "consolas", "mono", "menlo", "code"].iter().any(|k| lower.contains(k)) {
        &["Consolas", "Menlo", "Courier New", "Liberation Mono", "DejaVu Sans Mono"]
    } else if ["times", "cambria", "georgia", "garamond", "book", "serif", "palatino", "minion", "constantia", "baskerville"]
        .iter()
        .any(|k| lower.contains(k))
        && !lower.contains("sans")
    {
        &["Cambria", "Times New Roman", "Times", "Liberation Serif", "DejaVu Serif", "Libertinus Serif"]
    } else {
        &["Aptos", "Calibri", "Carlito", "Helvetica Neue", "Arial", "Segoe UI", "Liberation Sans", "DejaVu Sans"]
    };
    let mut names = vec![name.to_string()];
    for f in fallbacks {
        if !f.eq_ignore_ascii_case(name) {
            names.push(f.to_string());
        }
    }
    names
}

fn allow_break_after_hyphen(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    let mut previous = ' ';
    while let Some(c) = chars.next() {
        out.push(c);
        // Only inside a word: a leading minus ("-5") stays attached to its number
        if c == '-' && previous.is_alphanumeric() && chars.peek().is_some_and(|n| n.is_ascii_digit()) {
            out.push('\u{200B}');
        }
        previous = c;
    }
    out
}

struct Gen<'d> {
    doc: &'d Document,
    text_width: f32,
    image_paths: &'d [Option<String>],
    /// Index of the section being written (for SECTIONPAGES and first-page headers)
    section: usize,
    /// Inside a header/footer: repeated on every page, so no bookmark labels there
    in_header: std::cell::Cell<bool>,
    /// At the very start of a section (already on a new page): page breaks before the
    /// first content would only produce a blank page
    at_section_start: std::cell::Cell<bool>,
    /// The next section starts on a new page anyway: a page break at the very end of
    /// this one would leave a blank page (Word merges the two)
    trim_trailing_break: std::cell::Cell<bool>,
    /// How many tables deep the current content is (0 = body text)
    table_depth: std::cell::Cell<u32>,
    /// The paragraph being written is justified, and so are its lines before manual
    /// line breaks (Word's default)
    justify_breaks: std::cell::Cell<bool>,
}

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
        if let Some(font) = &r.font {
            if !font.eq_ignore_ascii_case(&self.doc.default_font) && !is_symbol_font(font) {
                let _ = write!(args, "font: {}, ", font_list(font));
            }
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
    fn paragraph_content(&self, inlines: &[Inline], tabs: &[TabStop], extra_stop: Option<f32>, first_line_x: f32) -> String {
        if !inlines.iter().any(|i| matches!(i, Inline::Tab)) {
            return self.inline_markup(inlines);
        }
        let segments: Vec<String> = inlines
            .split(|i| matches!(i, Inline::Tab))
            .map(|seg| format!("[{}]", self.inline_markup(seg)))
            .collect();
        let mut stops: Vec<TabStop> = tabs.to_vec();
        if let Some(pos) = extra_stop {
            if !stops.iter().any(|t| (t.pos - pos).abs() < 0.5) {
                stops.push(TabStop { pos, align: TabAlign::Left, leader: None });
            }
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

    fn main_size(&self, p: &Paragraph) -> f32 {
        p.inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Text(_, r) => Some(r.size),
                _ => None,
            })
            .fold(None, |acc: Option<f32>, s| Some(acc.map_or(s, |a| a.max(s))))
            .or(p.marker.as_ref().map(|m| m.run.size))
            .unwrap_or(p.mark.size)
    }

    /// Word's line box for this paragraph: (height of every line, distance from the
    /// line's top to the baseline), in points. Word makes each line exactly this tall
    /// and puts the extra height of 1.5/double/exact spacing above the text; the text
    /// edges below reproduce that, so Typst lines need no leading at all.
    fn line_box(&self, p: &Paragraph) -> (f32, f32) {
        let size = self.main_size(p);
        let font = p
            .inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Text(_, r) if (r.size - size).abs() < 0.01 => r.font.clone(),
                _ => None,
            })
            .next()
            .or_else(|| p.mark.font.clone())
            .unwrap_or_else(|| self.doc.default_font.clone());
        let (ascent, descent) = super::typeset::line_metrics(&font_names(&font));
        let single = (ascent + descent) * size;
        let height = match p.props.line {
            LineSpacing::Multiple(m) => m * single,
            LineSpacing::Exact(h) => h,
            LineSpacing::AtLeast(h) => h.max(single),
        };
        (height, (height - descent * size).max(0.0))
    }

    fn paragraph(&self, p: &Paragraph) -> String {
        let props = &p.props;
        let (line_height, baseline) = self.line_box(p);
        self.justify_breaks.set(p.props.align == Align::Justify && self.doc.justify_breaks);

        if p.inlines.is_empty() && p.marker.is_none() {
            // Empty paragraph: keeps its line height, like pressing Enter in Word
            return format!("#block(height: {}, width: 100%)[]", pt(line_height));
        }

        let align = match props.align {
            Align::Center => "center",
            Align::Right => "right",
            _ => "left",
        };
        let mut set = format!(
            "#set text(top-edge: {}, bottom-edge: {})\n#set par(leading: 0pt, justify: {}",
            pt(baseline),
            pt(baseline - line_height),
            props.align == Align::Justify
        );
        let left = props.indent_left.max(0.0);
        let right = props.indent_right.max(0.0);
        let (pad_left, first) = if props.first_line < 0.0 {
            let hang = -props.first_line;
            let _ = write!(set, ", hanging-indent: {}", pt(hang.min(left.max(hang))));
            ((left - hang).max(0.0), 0.0)
        } else {
            (left, props.first_line)
        };
        set.push(')');

        // A list number/bullet is just the start of the first line, followed by a tab
        // (or a space), exactly as Word lays it out.
        let mut inlines = p.inlines.clone();
        if let Some(marker) = &p.marker {
            let mut run = marker.run.clone();
            if run.font.as_deref().is_some_and(is_symbol_font) {
                run.font = None;
            }
            let mut lead = vec![Inline::Text(marker.text.clone(), run.clone())];
            match marker.suffix {
                MarkerSuffix::Tab => lead.push(Inline::Tab),
                MarkerSuffix::Space => lead.push(Inline::Text(" ".into(), run)),
                MarkerSuffix::Nothing => {}
            }
            lead.extend(inlines);
            inlines = lead;
        }
        let extra_stop = (props.first_line < 0.0).then_some(left);
        let content = self.paragraph_content(&inlines, &props.tabs, extra_stop, pad_left + first);
        // First-line indent as fixed space: Typst's own first-line indent skips a
        // block's only paragraph, and every Word paragraph is its own block here
        let indent = if first > 0.0 { format!("#box(width: {})", pt(first)) } else { String::new() };
        // An explicit `par`: Typst ignores paragraph indents for a lone paragraph in a block
        // A strut: every line is at least Word's line box, even one holding only a
        // floating picture or a tab
        let strut = format!("#box(width: 0pt, height: {}, baseline: {})", pt(line_height), pt(line_height - baseline));
        let body = format!("#pad(left: {}, right: {})[{set}\n#align({align})[#par[{strut}{indent}{content}]]]", pt(pad_left), pt(right));

        match props.fill {
            Some(fill) => format!("#block(width: 100%, fill: {}, inset: (x: 2pt, y: 1pt))[{body}]", rgb(fill)),
            None => format!("#block(width: 100%)[{body}]"),
        }
    }

    fn table(&self, t: &Table) -> String {
        // Grid columns: from the document, widened if some row needs more
        let needed = t.rows.iter().map(|r| r.cells.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(1).max(1);
        let mut widths = t.columns.clone();
        let avg = if widths.is_empty() { 0.0 } else { widths.iter().sum::<f32>() / widths.len() as f32 };
        while widths.len() < needed {
            widths.push(if avg > 0.0 { avg } else { self.text_width / needed as f32 });
        }
        // Like Word, a table wider than the text keeps its columns and extends into the
        // page margins (centred tables into both); only beyond that is it scaled down
        let page = &self.doc.sections[self.section].page;
        let edge = 14.4; // keep at least 0.2 in of paper around the table
        let limit = if self.table_depth.get() == 0 && page.columns <= 1 {
            match t.align {
                Align::Center => self.text_width + 2.0 * (page.margin_left.min(page.margin_right) - edge).max(0.0),
                Align::Right => self.text_width + (page.margin_left - edge).max(0.0),
                _ => self.text_width + (page.margin_right - edge).max(0.0),
            }
        } else {
            self.text_width
        };
        let total: f32 = widths.iter().sum();
        if total > limit && total > 0.0 {
            let k = limit / total;
            widths.iter_mut().for_each(|w| *w *= k);
        }
        let overflow = (widths.iter().sum::<f32>() - self.text_width).max(0.0);
        self.table_depth.set(self.table_depth.get() + 1);
        let columns: Vec<String> = widths.iter().map(|w| if *w > 0.0 { pt(*w) } else { "1fr".into() }).collect();
        let ncols = columns.len();
        let nrows = t.rows.len();

        // Grid position of every cell, to turn Word's vMerge into rowspans
        let positions: Vec<Vec<usize>> = t
            .rows
            .iter()
            .map(|r| {
                let mut col = 0;
                r.cells
                    .iter()
                    .map(|c| {
                        let start = col;
                        col += c.span;
                        start
                    })
                    .collect()
            })
            .collect();

        let mut rows: Vec<Vec<String>> = vec![];
        let mut row_borders = vec![0.0f32; nrows];
        for (ri, row) in t.rows.iter().enumerate() {
            let mut cells = vec![];
            let mut used = 0;
            for (ci, cell) in row.cells.iter().enumerate() {
                used += cell.span;
                if cell.vmerge == VMerge::Continue {
                    continue; // covered by the rowspan of the cell above
                }
                let col = positions[ri][ci];
                let mut rowspan = 1;
                if cell.vmerge == VMerge::Restart {
                    for (brow, bpos) in t.rows.iter().zip(&positions).skip(ri + 1) {
                        let continues = brow.cells.iter().zip(bpos).any(|(c, &p)| p == col && c.vmerge == VMerge::Continue);
                        if !continues {
                            break;
                        }
                        rowspan += 1;
                    }
                }
                let span = cell.span.min(ncols);
                // Each side: the cell's own border, else the table's outer or inner line
                let b = &t.borders;
                let top = cell.borders.top.unwrap_or(if ri == 0 { b.top } else { b.inside_h });
                let bottom = cell.borders.bottom.unwrap_or(if ri + rowspan >= nrows { b.bottom } else { b.inside_h });
                let left = cell.borders.left.unwrap_or(if col == 0 { b.left } else { b.inside_v });
                let right = cell.borders.right.unwrap_or(if col + span >= ncols { b.right } else { b.inside_v });
                let stroke = format!("(top: {}, bottom: {}, left: {}, right: {})", stroke(top), stroke(bottom), stroke(left), stroke(right));
                // Word makes a row taller by the width of its top and bottom borders
                let border_extra = top.map(|b| b.width).unwrap_or(0.0) + bottom.map(|b| b.width).unwrap_or(0.0);
                if rowspan == 1 {
                    row_borders[ri] = row_borders[ri].max(border_extra);
                }
                let inset = format!(
                    "(left: {}, right: {}, top: {}, bottom: {})",
                    pt(t.cell_margin_left),
                    pt(t.cell_margin_right),
                    pt(t.cell_margin_top + top.map(|b| b.width).unwrap_or(0.0)),
                    pt(t.cell_margin_bottom + bottom.map(|b| b.width).unwrap_or(0.0))
                );
                cells.push(self.cell(cell, span, rowspan, &stroke, &inset));
            }
            // Short rows: fill the rest of the grid so later rows stay aligned
            for _ in used..ncols {
                cells.push("[]".into());
            }
            rows.push(cells);
        }

        // Rows marked "repeat as header row" in Word repeat on every page, as in Word
        let header_rows = t.rows.iter().take_while(|r| r.header).count().min(rows.len());
        let mut cells: Vec<String> = vec![];
        if header_rows > 0 {
            let header: Vec<String> = rows[..header_rows].concat();
            cells.push(format!("table.header({})", header.join(", ")));
        }
        cells.extend(rows[header_rows..].concat());
        self.table_depth.set(self.table_depth.get() - 1);

        let align = match t.align {
            Align::Center => "center",
            Align::Right => "right",
            _ => "left",
        };
        // Row heights: exact rows are fixed; "at least" rows grow when their content
        // (estimated) is taller than the minimum
        let row_heights: Vec<String> = t
            .rows
            .iter()
            .enumerate()
            .map(|(ri, r)| match r.height {
                Some((h, true)) => pt(h),
                Some((h, false)) => {
                    let mut col = 0;
                    let content = r
                        .cells
                        .iter()
                        .map(|c| {
                            let w: f32 = widths.iter().skip(col).take(c.span).sum();
                            col += c.span;
                            self.estimate_height(&c.blocks, (w - t.cell_margin_left - t.cell_margin_right).max(12.0)) + t.cell_margin_top + t.cell_margin_bottom
                        })
                        .fold(0.0, f32::max);
                    // A minimum height grows by half the borders (measured from Word)
                    if content < h { pt(h + row_borders[ri] / 2.0) } else { "auto".into() }
                }
                None => "auto".into(),
            })
            .collect();
        // Negative padding widens the area into the margins by the overflow
        // Left-aligned tables also move by their indent (tblInd)
        let table_width: f32 = widths.iter().sum();
        let bleed = match t.align {
            Align::Center if overflow > 0.0 => format!("x: {}", pt(-overflow / 2.0)),
            Align::Right if overflow > 0.0 => format!("left: {}", pt(-overflow)),
            Align::Center | Align::Right => String::new(),
            _ if self.table_depth.get() == 0 || t.indent != 0.0 => {
                let right = (table_width + t.indent - self.text_width).max(0.0);
                format!("left: {}, right: {}", pt(t.indent), pt(-right))
            }
            _ => String::new(),
        };
        format!(
            "#pad({bleed})[#block(width: 100%)[#align({align})[#table(columns: ({},), rows: ({},), inset: (left: {}, right: {}, top: {}, bottom: {}), stroke: none, {})]]]",
            columns.join(", "),
            row_heights.join(", "),
            pt(t.cell_margin_left),
            pt(t.cell_margin_right),
            pt(t.cell_margin_top),
            pt(t.cell_margin_bottom),
            cells.join(", ")
        )
    }

    fn cell(&self, cell: &Cell, colspan: usize, rowspan: usize, stroke: &str, inset: &str) -> String {
        let content = self.blocks_markup(&cell.blocks, false);
        let mut args = vec![];
        if colspan > 1 {
            args.push(format!("colspan: {colspan}"));
        }
        if rowspan > 1 {
            args.push(format!("rowspan: {rowspan}"));
        }
        if let Some(fill) = cell.fill {
            args.push(format!("fill: {}", rgb(fill)));
        }
        args.push(format!("stroke: {stroke}"));
        args.push(format!("inset: {inset}"));
        args.push(
            match cell.valign {
                Align::Center => "align: horizon",
                Align::Right => "align: bottom",
                _ => "align: top",
            }
            .to_string(),
        );
        format!("table.cell({})[{content}]", args.join(", "))
    }

    /// Emits blocks with Word-like vertical spacing between them. Page breaks are only
    /// allowed at the top level (`top`), so they are dropped inside table cells.
    fn blocks_markup(&self, blocks: &[Block], top: bool) -> String {
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
                            if let Inline::Image { index, width, height, float: Some((x, y)), on_page: true } = inline {
                                if let Some(Some(path)) = self.image_paths.get(*index) {
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
                    }
                    s.push_str(&self.paragraph(&p));
                    s.push('\n');
                    if top {
                        self.at_section_start.set(false);
                    }
                    prev_after = Some((p.props.space_after, p.props.style_id.clone(), p.props.contextual_spacing));
                }
                Piece::Block(Block::Table(t)) => {
                    if let Some((after, _, _)) = prev_after {
                        if after > 0.0 {
                            let _ = writeln!(s, "#v({}, weak: true)", pt(after));
                        }
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
    fn estimate_height(&self, blocks: &[Block], width: f32) -> f32 {
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
            if let Some(Block::Paragraph(p)) = blocks.last() {
                if p.props.space_after > 0.0 {
                    inner.push_str(&format!("\n#v({})", pt(p.props.space_after)));
                }
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

    fn section(&self, sec: &Section, index: usize, previous: Option<&Section>) -> String {
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

fn is_symbol_font(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("symbol") || n.contains("wingdings") || n.contains("webdings")
}

/// Typst reads PNG, JPEG, GIF, SVG and WebP; BMP/TIFF are converted to PNG here.
/// Vector formats Word sometimes uses (EMF/WMF) cannot be drawn and are left out.
fn prepare_image(index: usize, image: &super::docx::Image) -> Option<(String, Vec<u8>)> {
    match image.ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => Some((format!("/img/{index}.{}", image.ext), image.data.clone())),
        _ => {
            let img = image::load_from_memory(&image.data).ok()?;
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some((format!("/img/{index}.png"), png))
        }
    }
}

pub fn generate(doc: &Document) -> Output {
    let mut files = vec![];
    let mut image_paths = vec![];
    for (i, image) in doc.images.iter().enumerate() {
        match prepare_image(i, image) {
            Some((path, data)) => {
                image_paths.push(Some(path.clone()));
                files.push((path, data));
            }
            None => image_paths.push(None),
        }
    }

    let mut m = String::new();
    let lang = doc.lang.as_deref().filter(|l| l.chars().all(|c| c.is_ascii_lowercase())).unwrap_or("en");
    let _ = writeln!(
        m,
        // A fixed 1 em line box for every font, so line pitch = size + leading exactly,
        // whatever the font's own ascender/descender metrics are.
        "#set text(font: {}, size: {}, lang: {}, top-edge: 0.8em, bottom-edge: -0.2em, hyphenate: false)",
        font_list(&doc.default_font),
        pt(doc.default_size),
        q(lang)
    );
    m.push_str("#set par(spacing: 0pt)\n#set block(spacing: 0pt)\n");
    m.push_str(PREAMBLE);

    let mut previous: Option<&Section> = None;
    let mut open_block = false;
    for (index, sec) in doc.sections.iter().enumerate() {
        let page = &sec.page;
        let text_width = (page.width - page.margin_left - page.margin_right).max(72.0) / page.columns.max(1) as f32;
        let g = Gen {
            doc,
            text_width,
            image_paths: &image_paths,
            section: index,
            in_header: Default::default(),
            at_section_start: Default::default(),
            trim_trailing_break: Default::default(),
            table_depth: Default::default(),
            justify_breaks: Default::default(),
        };
        let markup = g.section(sec, index, previous);
        // A section with its own page settings closes the previous one's block (and
        // any page break that starts it comes after that)
        if markup.contains("#context {\nlet ff-top") {
            if open_block {
                m.push_str("]\n}\n");
            }
            open_block = true;
        }
        m.push_str(&markup);
        previous = Some(sec);
    }
    // Marks the last page, for NUMPAGES and the last section's SECTIONPAGES
    m.push_str("#metadata(none) <ff-end>\n");
    if open_block {
        m.push_str("]\n}\n");
    }
    Output { markup: m, files }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_can_break_after_inner_hyphens() {
        assert_eq!(allow_break_after_hyphen("CPX-REG-001"), "CPX-REG-\u{200B}001");
        assert_eq!(allow_break_after_hyphen("-5 and A-B"), "-5 and A-B");
    }

    #[test]
    fn strings_cannot_escape_into_code() {
        assert_eq!(q(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(q("x\u{0}y\nz"), "\"xy z\"");
        // Markup-looking text stays text
        assert_eq!(q("#set page(width: 1pt)"), "\"#set page(width: 1pt)\"");
    }

    #[test]
    fn fonts_get_similar_fallbacks() {
        assert!(font_list("Times New Roman").contains("Liberation Serif"));
        assert!(font_list("Calibri").contains("Carlito"));
        assert!(font_list("Courier New").contains("Menlo"));
    }
}
