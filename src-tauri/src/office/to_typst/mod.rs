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

// The generator's parts. The submodules share one namespace (each starts with
// `use super::*`), as when this was a single file.
mod blocks;
mod inline;
mod paragraph;
mod section;
mod table;
mod values;

#[cfg(test)]
mod tests;

use blocks::*;
use values::*;

// What slides_pdf shares: Typst strings, points and font lists
pub(crate) use values::{pt, q};
pub use values::font_list;

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
