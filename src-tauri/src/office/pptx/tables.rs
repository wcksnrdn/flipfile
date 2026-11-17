//! Tables

use super::*;

/// Parts of a table style that apply to a cell: fill, text colour and boldness, borders
#[derive(Default, Clone)]
struct CellLook {
    pub(super) fill: Option<Fill>,
    pub(super) color: Option<Rgba>,
    pub(super) bold: Option<bool>,
    pub(super) borders: [Option<Line>; 4],
    pub(super) inside_h: Option<Line>,
    pub(super) inside_v: Option<Line>,
}

impl CellLook {
    pub(super) fn over(&mut self, top: &CellLook) {
        if top.fill.is_some() {
            self.fill = top.fill.clone();
        }
        if top.color.is_some() {
            self.color = top.color;
        }
        if top.bold.is_some() {
            self.bold = top.bold;
        }
        for i in 0..4 {
            if top.borders[i].is_some() {
                self.borders[i] = top.borders[i].clone();
            }
        }
        if top.inside_h.is_some() {
            self.inside_h = top.inside_h.clone();
        }
        if top.inside_v.is_some() {
            self.inside_v = top.inside_v.clone();
        }
    }
}

/// "Medium Style 2 - Accent 1", PowerPoint's default table style, which files name
/// but don't always define
const DEFAULT_TABLE_STYLE: &str = r#"<a:tblStyle xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" styleId="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}"><a:wholeTbl><a:tcTxStyle><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="dk1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:left><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:left><a:right><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:right><a:top><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top><a:bottom><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom><a:insideH><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideH><a:insideV><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideV></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="20000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:wholeTbl><a:band1H><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1H><a:band1V><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1V><a:lastCol><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastCol><a:firstCol><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstCol><a:lastRow><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:top><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastRow><a:firstRow><a:tcTxStyle b="on"><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:bottom><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstRow></a:tblStyle>"#;

fn table_style_part(style: Node, name: &str, colors: &Colors) -> CellLook {
    let mut look = CellLook::default();
    let Some(part) = child(style, name) else { return look };
    if let Some(tx) = child(part, "tcTxStyle") {
        look.bold = flag(tx, "b");
        look.color = tx.children().filter(Node::is_element).filter(|c| c.tag_name().name() != "fontRef").find_map(|c| colors.color(c));
    }
    if let Some(tc) = child(part, "tcStyle") {
        if let Some(f) = child(tc, "fill") {
            look.fill = f.children().filter(Node::is_element).find_map(|c| match c.tag_name().name() {
                "solidFill" => colors.first(c).map(Fill::Solid),
                _ => None,
            });
        }
        if let Some(b) = child(tc, "tcBdr") {
            let side = |n: &str| child(b, n).and_then(|s| line_of(s, colors)).flatten();
            look.borders = [side("left"), side("top"), side("right"), side("bottom")];
            look.inside_h = side("insideH");
            look.inside_v = side("insideV");
        }
    }
    look
}

pub(super) fn table_element(table: Node, frame: Frame, cx: &Context, sink: &mut Sink) -> Option<Element> {
    let columns: Vec<f32> = children(child(table, "tblGrid")?, "gridCol").map(|c| emu(c, "w").unwrap_or(0.0)).collect();
    let pr = child(table, "tblPr");
    let opt = |name: &str| pr.and_then(|p| flag(p, name)).unwrap_or(false);
    let style_id = pr.and_then(|p| child(p, "tableStyleId")).and_then(|s| s.text()).unwrap_or("").trim().to_string();
    let styles_doc = Document::parse(sink.table_styles).ok();
    let default_doc = Document::parse(DEFAULT_TABLE_STYLE).ok();
    let style = styles_doc
        .as_ref()
        .and_then(|d| d.descendants().find(|n| n.tag_name().name() == "tblStyle" && n.attribute("styleId") == Some(style_id.as_str())))
        .or_else(|| (style_id == "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}").then(|| default_doc.as_ref().map(|d| d.root_element())).flatten());

    let rows_xml: Vec<Node> = children(table, "tr").collect();
    let (n_rows, n_cols) = (rows_xml.len(), columns.len());
    let mut rows = vec![];
    for (ri, tr) in rows_xml.iter().enumerate() {
        let mut cells = vec![];
        for (ci, tc) in children(*tr, "tc").enumerate() {
            // The table style's parts, from the whole table to the most specific
            let mut look = CellLook::default();
            if let Some(style) = style {
                look = table_style_part(style, "wholeTbl", &cx.colors);
                let band_row = if opt("firstRow") { ri.wrapping_sub(1) } else { ri };
                if opt("bandRow") && !(opt("firstRow") && ri == 0) && !(opt("lastRow") && ri + 1 == n_rows) {
                    look.over(&table_style_part(style, if band_row % 2 == 0 { "band1H" } else { "band2H" }, &cx.colors));
                }
                if opt("bandCol") {
                    look.over(&table_style_part(style, if ci % 2 == 0 { "band1V" } else { "band2V" }, &cx.colors));
                }
                if opt("firstCol") && ci == 0 {
                    look.over(&table_style_part(style, "firstCol", &cx.colors));
                }
                if opt("lastCol") && ci + 1 == n_cols {
                    look.over(&table_style_part(style, "lastCol", &cx.colors));
                }
                if opt("lastRow") && ri + 1 == n_rows {
                    look.over(&table_style_part(style, "lastRow", &cx.colors));
                }
                if opt("firstRow") && ri == 0 {
                    look.over(&table_style_part(style, "firstRow", &cx.colors));
                }
                // inside borders become the borders between cells
                if ri > 0 && look.borders[1].is_none() {
                    look.borders[1] = look.inside_h.clone();
                }
                if ci > 0 && look.borders[0].is_none() {
                    look.borders[0] = look.inside_v.clone();
                }
            }
            let tc_pr = child(tc, "tcPr");
            let mut fill = look.fill.clone();
            if let Some(p) = tc_pr
                && p.children().any(|c| matches!(c.tag_name().name(), "noFill" | "solidFill" | "gradFill" | "blipFill"))
            {
                fill = fill_of(p, &cx.colors, &cx.rels, sink.media, sink.package);
            }
            let mut borders = look.borders.clone();
            if let Some(p) = tc_pr {
                for (i, name) in ["lnL", "lnT", "lnR", "lnB"].iter().enumerate() {
                    if let Some(ln) = child(p, name) {
                        borders[i] = if child(ln, "noFill").is_some() {
                            None
                        } else {
                            let color = child(ln, "solidFill").and_then(|f| cx.colors.first(f));
                            color.map(|color| Line { color, width: emu(ln, "w").unwrap_or(0.75), dash: None, head: None, tail: None })
                        };
                    }
                }
            }
            let mut levels = cx.base[3].clone();
            for l in &mut levels.0 {
                if look.color.is_some() {
                    l.run.color = look.color;
                }
                if look.bold.is_some() {
                    l.run.bold = look.bold;
                }
            }
            let margins = [
                tc_pr.and_then(|p| emu(p, "marL")).unwrap_or(7.2),
                tc_pr.and_then(|p| emu(p, "marT")).unwrap_or(3.6),
                tc_pr.and_then(|p| emu(p, "marR")).unwrap_or(7.2),
                tc_pr.and_then(|p| emu(p, "marB")).unwrap_or(3.6),
            ];
            let anchor = match tc_pr.and_then(|p| p.attribute("anchor")) {
                Some("ctr") => Anchor::Middle,
                Some("b") => Anchor::Bottom,
                _ => Anchor::Top,
            };
            let mut text = child(tc, "txBody")
                .and_then(|b| text_body(b, &[], &levels, cx, false))
                .unwrap_or(TextBody { insets: margins, anchor, wrap: true, grows: false, paragraphs: vec![] });
            text.insets = margins;
            text.anchor = anchor;
            cells.push(TableCell {
                col_span: num(tc, "gridSpan").unwrap_or(1.0).max(1.0) as usize,
                row_span: num(tc, "rowSpan").unwrap_or(1.0).max(1.0) as usize,
                merged: flag(tc, "hMerge").unwrap_or(false) || flag(tc, "vMerge").unwrap_or(false),
                fill,
                borders,
                text,
            });
        }
        rows.push(TableRow { height: emu(*tr, "h").unwrap_or(0.0), cells });
    }
    (!rows.is_empty()).then_some(Element::Table { frame, columns, rows })
}
