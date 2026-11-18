//! A page onto the sheet, and the sheet built from every page

use super::*;

impl Value {
    pub(super) fn is_text(&self) -> bool {
        matches!(self, Value::Text(_))
    }
}

pub(super) fn collect_lines<'a>(blocks: &'a [Block], out: &mut Vec<&'a Line>) {
    for b in blocks {
        match &b.content {
            Content::Paragraph(lines) => out.extend(lines.iter()),
            Content::Table(t) => {
                for c in &t.cells {
                    collect_lines(&c.blocks, out);
                }
            }
            Content::Images(_) => {}
        }
    }
}


/// A first row of only text over rows that have numbers: the heading
pub(super) fn heading_like(grid: &[Vec<(String, Option<Style>)>]) -> bool {
    let Some(first) = grid.first() else { return false };
    let filled: Vec<&String> = first.iter().map(|c| &c.0).filter(|t| !t.is_empty()).collect();
    if filled.len() < 2 || grid.len() < 3 {
        return false;
    }
    let texty = filled.iter().all(|t| t.chars().any(|c| c.is_alphabetic()) && !t.chars().any(|c| c.is_ascii_digit()));
    let numbers_below = grid[1..].iter().flat_map(|r| r.iter()).filter(|c| c.0.chars().any(|ch| ch.is_ascii_digit())).count() > 0;
    texty && numbers_below
}

/// Lines that are part of a table drawn without lines: wide gaps in them
fn tabular(l: &Line) -> bool {
    l.tabs.iter().any(|t| !t.leader) && segments(l).len() >= 2
}

fn page_into(b: &mut Builder, page: &PageModel) {
    // shading behind text: coloured, and smaller than the page (not its background)
    let page_area = page.width * page.height;
    b.fills = page.fills.iter().filter(|(r, c)| r.w() * r.h() < page_area * 0.5 && r.h() > 4.0 && c.iter().any(|v| *v < 245)).cloned().collect();
    let mut list = vec![];
    items(&page.body, &mut list);
    // whether a line starts a table: tabular, or a wrapped cell's first line right above
    // a tabular one
    let starts_table = |i: usize| -> bool {
        match &list[i] {
            Item::Line(l) if tabular(l) => true,
            Item::Line(l) => matches!(list.get(i + 1), Some(Item::Line(n)) if tabular(n) && n.baseline - l.baseline < l.size.max(n.size) * 2.0 && segments(l).len() == 1),
            _ => false,
        }
    };
    // a new page: a blank row, unless a table carries on (decided by the table itself)
    if !matches!(list.first(), Some(Item::Table(_))) && !(!list.is_empty() && starts_table(0)) {
        b.gap();
    }
    let mut i = 0;
    let mut first = true;
    while i < list.len() {
        let page_top = first;
        first = false;
        match &list[i] {
            Item::Table(t) => {
                b.ruled_table(t, page_top);
                i += 1;
            }
            Item::Line(l) if starts_table(i) => {
                // the run of lines belonging to one table: tabular lines, and single
                // lines between them (wrapped descriptions)
                let mut run = vec![l.clone()];
                let mut j = i + 1;
                let mut last_tab = 0;
                // the edges of the parts seen so far: a row whose parts line up with none of
                // them starts another table
                // (the first row may be a heading, set differently from the rows under it:
                // the rows from the second on are compared)
                let (mut seen_l, mut seen_r): (Vec<f32>, Vec<f32>) = (vec![], vec![]);
                let mut tabular_seen = usize::from(tabular(l));
                while j < list.len() {
                    let Item::Line(next) = &list[j] else { break };
                    let prev = run.last().unwrap();
                    let close = next.baseline - prev.baseline < prev.size.max(next.size) * 3.2 && next.baseline > prev.baseline - 1.0;
                    if !close {
                        break;
                    }
                    if tabular(next) {
                        let parts = segments(next);
                        tabular_seen += 1;
                        if tabular_seen > 2 && !seen_l.is_empty() {
                            let fits = parts.iter().filter(|s| seen_l.iter().any(|x| (x - s.left).abs() < 4.0) || seen_r.iter().any(|x| (x - s.right).abs() < 4.0)).count();
                            if fits * 2 < parts.len() + 1 {
                                break;
                            }
                        }
                        for s in parts {
                            seen_l.push(s.left);
                            seen_r.push(s.right);
                        }
                    }
                    run.push(next.clone());
                    if tabular(next) {
                        last_tab = run.len() - 1;
                    }
                    j += 1;
                }
                // single lines after the last tabular one are not part of it
                run.truncate(last_tab.max(usize::from(!tabular(l))) + 1);
                let tab_lines = run.iter().filter(|l| tabular(l)).count();
                if tab_lines >= 2 {
                    b.stream_table(&run, page_top);
                    i += run.len();
                } else {
                    b.text_line(l);
                    i += 1;
                }
            }
            Item::Line(l) => {
                b.text_line(l);
                i += 1;
            }
        }
    }
}

/// PDF pages laid out on a sheet named `name`
pub(super) fn build(pages: &[PageModel], name: &str) -> w::Sheet {
    let mut texts = vec![];
    for p in pages {
        let mut list = vec![];
        items(&p.body, &mut list);
        for it in list {
            match it {
                Item::Line(l) => texts.push(l.text()),
                Item::Table(t) => {
                    let mut lines = vec![];
                    for c in &t.cells {
                        collect_lines(&c.blocks, &mut lines);
                    }
                    texts.extend(lines.iter().map(|l| l.text()));
                }
            }
        }
    }
    let mut b = Builder { sheet: w::Sheet { name: name.to_string(), ..Default::default() }, row: 0, notation: notation(&texts), last: None, widths: BTreeMap::new(), first_table: true, bottom: None, fills: vec![] };
    for page in pages {
        b.bottom = None;
        page_into(&mut b, page);
    }
    // column widths from the PDF's own (points -> Excel's characters of Calibri 11)
    for (c, pt) in &b.widths {
        let chars = ((pt + 6.0) * 96.0 / 72.0 - 5.0) / 7.0;
        b.sheet.col_widths.insert(*c, chars.clamp(4.0, 80.0));
    }
    // a column of long text without a measured width
    let used: Vec<u32> = b.sheet.cells.keys().map(|k| k.1).collect();
    for c in used {
        b.sheet.col_widths.entry(c).or_insert(10.0);
    }
    b.sheet
}
