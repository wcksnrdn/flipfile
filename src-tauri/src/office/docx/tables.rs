//! Tables: grid, merged cells, borders and shading

use super::*;

impl<'p> Reader<'p> {
    pub(super) fn table(&mut self, tbl: Node, rels: &HashMap<String, String>) -> Table {
        let tblpr = w(tbl, "tblPr");
        let columns: Vec<f32> = w(tbl, "tblGrid")
            .map(|g| g.children().filter(|c| c.tag_name().name() == "gridCol").filter_map(|c| wattr(c, "w").and_then(twips)).collect())
            .unwrap_or_default();

        // Borders: direct table borders, else the table style's, else none
        let mut borders = tblpr.and_then(|p| w(p, "tblBorders")).map(table_borders);
        if borders.is_none()
            && let Some(style) = tblpr.and_then(|p| w(p, "tblStyle")).and_then(val) {
                borders = self.chain(style).iter().rev().find_map(|s| s.table_borders);
            }
        let align = match tblpr.and_then(|p| w(p, "jc")).and_then(val) {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            _ => Align::Left,
        };
        // Cell padding: the table's own, else its style's (nearest in the chain)
        let style_id = tblpr.and_then(|p| w(p, "tblStyle")).and_then(val).map(str::to_string).or_else(|| self.default_table_style.clone());
        let style_margins: HashMap<&str, f32> = {
            let chain = style_id.map(|s| self.chain(&s)).unwrap_or_default();
            ["top", "bottom", "left", "right"].into_iter().filter_map(|side| chain.iter().find_map(|st| st.cell_margins.get(side).copied()).map(|v| (side, v))).collect()
        };
        let margin = |side: &str| -> Option<f32> {
            let alias = match side {
                "left" => "start",
                "right" => "end",
                other => other,
            };
            tblpr
                .and_then(|p| w(p, "tblCellMar"))
                .and_then(|m| w(m, side).or_else(|| w(m, alias)))
                .and_then(|s| wattr(s, "w"))
                .and_then(twips)
                .or_else(|| style_margins.get(side).copied())
        };

        let mut rows = vec![];
        for tr in tbl.children().filter(|c| c.tag_name().name() == "tr" && c.tag_name().namespace() == Some(W)) {
            let header = w(tr, "trPr").and_then(|p| w(p, "tblHeader")).is_some_and(toggle);
            let height = w(tr, "trPr").and_then(|p| w(p, "trHeight")).and_then(|h| {
                let v = wattr(h, "val").and_then(twips)?;
                (v > 0.0).then_some((v, wattr(h, "hRule") == Some("exact")))
            });
            let mut cells = vec![];
            for tc in tr.children().filter(|c| c.is_element()) {
                let tc = match tc.tag_name().name() {
                    "tc" => tc,
                    // Content controls wrapping a cell
                    "sdt" => match w(tc, "sdtContent").and_then(|c| w(c, "tc")) {
                        Some(cell) => cell,
                        None => continue,
                    },
                    _ => continue,
                };
                let tcpr = w(tc, "tcPr");
                let span = tcpr.and_then(|p| w(p, "gridSpan")).and_then(val).and_then(|v| v.parse().ok()).unwrap_or(1usize).max(1);
                let vmerge = match tcpr.and_then(|p| w(p, "vMerge")) {
                    Some(m) if val(m) == Some("restart") => VMerge::Restart,
                    Some(_) => VMerge::Continue,
                    None => VMerge::None,
                };
                let fill = tcpr.and_then(|p| w(p, "shd")).and_then(|s| wattr(s, "fill")).and_then(hex_color);
                let valign = match tcpr.and_then(|p| w(p, "vAlign")).and_then(val) {
                    Some("center") => Align::Center,
                    Some("bottom") => Align::Right,
                    _ => Align::Left,
                };
                let borders = tcpr.and_then(|p| w(p, "tcBorders")).map(cell_borders).unwrap_or_default();
                let blocks = self.blocks(tc, rels);
                cells.push(Cell { blocks, span, vmerge, fill, valign, borders });
            }
            rows.push(Row { cells, header, height });
        }

        Table {
            columns,
            rows,
            borders: borders.unwrap_or_default(),
            align,
            cell_margin_left: margin("left").unwrap_or(5.4),
            cell_margin_right: margin("right").unwrap_or(5.4),
            // Word's default: no padding above and below the text of a cell
            cell_margin_top: margin("top").unwrap_or(0.0),
            cell_margin_bottom: margin("bottom").unwrap_or(0.0),
            indent: {
                let ind = tblpr.and_then(|p| w(p, "tblInd")).filter(|i| wattr(*i, "type").is_none_or(|t| t == "dxa")).and_then(|i| wattr(i, "w")).and_then(twips).unwrap_or(0.0);
                if self.legacy_tables && align == Align::Left { ind - margin("left").unwrap_or(5.4) } else { ind }
            },
        }
    }
}
