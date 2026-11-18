//! Tables: grid columns, merged cells, per-side borders and repeating header rows

use super::*;

impl<'d> Gen<'d> {
    pub(super) fn table(&self, t: &Table) -> String {
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

    pub(super) fn cell(&self, cell: &Cell, colspan: usize, rowspan: usize, stroke: &str, inset: &str) -> String {
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
}
