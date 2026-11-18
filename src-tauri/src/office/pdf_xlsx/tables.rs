//! Tables on the sheet: drawn without lines (columns that line up) or with them,
//! continued from the page before, and aligned under the table above

use super::*;

impl Builder {
    /// Lines whose columns line up: a table drawn without lines
    pub(super) fn stream_table(&mut self, lines: &[Line], page_top: bool) {
        // every gap counts inside a table: columns closer than a font size apart (a date
        // and its description) are still columns when the rows agree on them
        let bounds = column_starts(lines);
        let rows: Vec<Vec<Segment>> = lines.iter().map(|l| split_segments(l, &bounds)).collect();
        let strong: Vec<usize> = lines.iter().map(|l| segments(l).len()).collect();
        // columns from the lines with the usual number of parts (data rows, not a heading
        // split over two lines), the others fitted in
        let most = {
            let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
            for r in rows.iter().filter(|r| r.len() >= 2) {
                *counts.entry(r.len()).or_default() += 1;
            }
            counts.iter().max_by_key(|(len, n)| (**n, **len)).map(|(len, _)| *len).unwrap_or(0)
        };
        // each column where most of those rows have it (medians: a stray long text doesn't
        // widen it over its neighbours)
        let mut edges: Vec<(Vec<f32>, Vec<f32>)> = vec![];
        for r in rows.iter().filter(|r| r.len() == most) {
            for (i, s) in r.iter().enumerate() {
                if edges.len() <= i {
                    edges.push((vec![], vec![]));
                }
                edges[i].0.push(s.left);
                edges[i].1.push(s.right);
            }
        }
        let median = |v: &mut Vec<f32>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v[v.len() / 2]
        };
        let mut cols: Vec<(f32, f32)> = edges.iter_mut().map(|(l, r)| (median(l), median(r))).collect();
        let column_of = |cols: &mut Vec<(f32, f32)>, s: &Segment| -> usize {
            let overlap = |c: &(f32, f32)| (s.right.min(c.1) - s.left.max(c.0)).max(0.0);
            if let Some((i, _)) = cols.iter().enumerate().filter(|(_, c)| overlap(c) > 0.0).max_by(|a, b| overlap(a.1).partial_cmp(&overlap(b.1)).unwrap_or(std::cmp::Ordering::Equal)) {
                return i;
            }
            // between two columns' text: it starts after the left one's text ends, so it is
            // the right one's (a heading set at the start of a column of right-aligned
            // numbers), unless that is too far off
            if let Some((i, _)) = cols.iter().enumerate().filter(|(_, c)| c.0 >= s.left && c.0 - s.right < s.style.size * 4.0).min_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap_or(std::cmp::Ordering::Equal)) {
                return i;
            }
            // between columns: a column of its own (at the end: rows placed already keep
            // their columns' numbers; the order left to right is worked out after)
            cols.push((s.left, s.right));
            cols.len() - 1
        };
        let mut placed_rows: Vec<Vec<(usize, Segment)>> = vec![];
        for r in &rows {
            let mut cells = vec![];
            for s in r {
                let i = column_of(&mut cols, s);
                cells.push((i, s.clone()));
            }
            placed_rows.push(cells);
        }
        // columns as found, left to right; text moves when a column was added later
        let order: Vec<usize> = {
            let mut idx: Vec<usize> = (0..cols.len()).collect();
            idx.sort_by(|a, b| cols[*a].0.partial_cmp(&cols[*b].0).unwrap_or(std::cmp::Ordering::Equal));
            let mut rank = vec![0; cols.len()];
            for (k, i) in idx.into_iter().enumerate() {
                rank[i] = k;
            }
            rank
        };
        let mut grid: Vec<Vec<(String, Option<Style>)>> = vec![];
        let mut shading: Vec<Vec<Option<[u8; 3]>>> = vec![];
        let mut wraps: Vec<bool> = vec![];
        // a wrapped cell's lines above its row (text set to the bottom of a tall row)
        let mut ahead: Vec<(usize, String)> = vec![];
        let full_rows: Vec<usize> = (0..placed_rows.len()).filter(|k| strong[*k] >= 2).collect();
        // baseline of the last line put in each column of the row before
        let mut col_base: Vec<Option<f32>> = vec![None; cols.len()];
        for (k, cells) in placed_rows.iter().enumerate() {
            let full = strong[k] >= 2;
            // a line with one part: more text of a cell in the nearer row, above or below
            if !full && cells.len() == 1 {
                let (i, s) = &cells[0];
                let col = order[*i];
                let base = lines[k].baseline;
                let prev = col_base.get(col).copied().flatten().map(|b| base - b);
                let next = full_rows.iter().find(|r| **r > k).map(|r| lines[*r].baseline - base);
                // only a line that fits its column can be more of a cell's text (not a
                // paragraph under the table)
                let fits = cols.get(*i).is_some_and(|c| s.right - s.left <= (c.1 - c.0) * 1.4 + s.style.size * 2.0);
                let texty = fits && typed(&s.text, self.notation).0.is_text();
                let joins_prev = grid.last().is_some_and(|p: &Vec<(String, Option<Style>)>| col < p.len() && !p[col].0.is_empty() && typed(&p[col].0, self.notation).0.is_text());
                match (prev, next) {
                    (_, Some(n)) if texty && (prev.is_none_or(|p| n < p) || !joins_prev) => {
                        ahead.push((col, s.text.clone()));
                        continue;
                    }
                    _ if texty && joins_prev => {
                        let p = grid.last_mut().unwrap();
                        p[col].0.push('\n');
                        p[col].0.push_str(&s.text);
                        *wraps.last_mut().unwrap() = true;
                        col_base[col] = Some(base);
                        continue;
                    }
                    _ => {}
                }
            }
            let mut row = vec![(String::new(), None); cols.len()];
            let had_ahead = !ahead.is_empty();
            for (col, text) in ahead.drain(..) {
                if col < row.len() {
                    if !row[col].0.is_empty() {
                        row[col].0.push('\n');
                    }
                    row[col].0.push_str(&text);
                }
            }
            let mut row_fill = vec![None; cols.len()];
            for (i, s) in cells {
                let col = order[*i];
                if !row[col].0.is_empty() {
                    row[col].0.push(if had_ahead { '\n' } else { ' ' });
                }
                row[col].0.push_str(&s.text);
                row[col].1.get_or_insert(s.style.clone());
                if row_fill[col].is_none() {
                    row_fill[col] = self.fill_at(s, lines[k].baseline);
                }
            }
            // a band behind the whole row (a heading): every cell of it
            let filled: Vec<[u8; 3]> = row_fill.iter().flatten().copied().collect();
            if filled.len() >= 2 && filled.iter().all(|c| *c == filled[0]) {
                row_fill = vec![Some(filled[0]); cols.len()];
            }
            shading.push(row_fill);
            grid.push(row);
            wraps.push(had_ahead);
            col_base = vec![None; cols.len()];
            for (i, _) in cells {
                col_base[order[*i]] = Some(lines[k].baseline);
            }
        }
        // under the columns of the table before (totals under their amounts)
        let sorted_cols: Vec<(f32, f32)> = {
            let mut v = cols.clone();
            v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        let shift = self.align(&sorted_cols, page_top);
        // a currency sign in a column of its own ("Rp" set apart from the amount): part of
        // the amount
        for row in &mut grid {
            for c in 0..row.len().saturating_sub(1) {
                let sign = matches!(row[c].0.trim(), "Rp" | "Rp." | "IDR" | "$" | "USD" | "€" | "£");
                if sign && matches!(typed(&row[c + 1].0, self.notation).0, Value::Number(_)) {
                    let amount = format!("{} {}", row[c].0.trim(), row[c + 1].0);
                    row[c + 1].0 = amount;
                    row[c].0.clear();
                }
            }
        }
        let heading: Vec<String> = grid.first().map(|r| r.iter().map(|c| c.0.clone()).collect()).unwrap_or_default();
        let mut skip = 0;
        let continued = self.continues(page_top, &sorted_cols);
        if continued {
            // the same table, continued: its heading again is dropped
            if self.last.as_ref().is_some_and(|l| l.heading == heading) {
                skip = 1;
            }
        } else if page_top {
            self.gap();
        } else if let Some(first) = lines.first() {
            self.space_before(first.baseline - first.size, first.size);
        }
        let aligned = shift.iter().enumerate().any(|(i, c)| *c != i as u32);
        let header_row = !aligned && !continued && heading_like(&grid);
        for (k, row) in grid.iter().enumerate().skip(skip) {
            for (c, (text, style)) in row.iter().enumerate() {
                let fill = shading.get(k).and_then(|r| r.get(c)).copied().flatten();
                let c = shift.get(c).copied().unwrap_or(c as u32) as usize;
                let mut look = style.as_ref().map(look_of).unwrap_or_default();
                look.fill = look.fill.or(fill);
                if wraps[k] {
                    // the row's cells line up at the top; only cells of several lines wrap
                    look.wrap = text.contains('\n');
                    look.v = Some("top");
                }
                if k == 0 && header_row {
                    look.bold = true;
                    look.borders[3] = Some((Edge::Thin, [0, 0, 0]));
                }
                self.put(self.row, c as u32, text, look);
            }
            if wraps[k] {
                let lines = row.iter().map(|c| c.0.lines().count()).max().unwrap_or(1);
                self.sheet.row_heights.insert(self.row, ROW * lines as f32);
            }
            if k == 0 && header_row && self.first_table && self.row < 30 && grid.len() >= 8 {
                // (heading rows stay on screen in long tables)
                self.sheet.freeze_rows = self.row + 1;
            }
            self.row += 1;
        }
        for (c, (l, r)) in cols.iter().enumerate() {
            let at = shift.get(order[c]).copied().unwrap_or(order[c] as u32);
            self.width(at, r - l);
        }
        self.first_table = false;
        if let Some(l) = lines.last() {
            self.bottom = Some(l.baseline + l.size * 0.3);
        }
        if !continued && !aligned {
            let spans = sorted_cols.iter().zip(&shift).map(|((l, r), at)| (*l, *r, *at)).collect();
            self.last = Some(Placed { heading, spans });
        }
    }

    /// At the top of a page, the table the last page ended with carries on: the same
    /// columns, by their left or right edges (numbers line up on the right)
    fn continues(&self, page_top: bool, cols: &[(f32, f32)]) -> bool {
        page_top
            && self.last.as_ref().is_some_and(|l| {
                l.spans.len() == cols.len()
                    && l.spans.iter().zip(cols).all(|(a, b)| (a.0 - b.0).abs() < 12.0 || (a.1 - b.1).abs() < 12.0 || a.1.min(b.1) - a.0.max(b.0) > 0.5 * (a.1 - a.0).min(b.1 - b.0))
            })
    }

    /// A table drawn with lines
    pub(super) fn ruled_table(&mut self, t: &from_pdf::Table, page_top: bool) {
        let ncols = t.xs.len().saturating_sub(1);
        let nrows = t.ys.len().saturating_sub(1);
        if ncols == 0 || nrows == 0 {
            return;
        }
        let text_of = |cell: &from_pdf::Cell| -> (String, Option<Style>, Option<&'static str>) {
            let mut lines = vec![];
            collect_lines(&cell.blocks, &mut lines);
            let style = lines.iter().find_map(|l| l.runs.iter().find(|(t, _)| !t.trim().is_empty()).map(|(_, s)| s.clone()));
            let text = lines.iter().map(|l| l.text().replace('\t', " ").trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
            // alignment from where the text sits in the cell
            let align = lines.first().and_then(|l| {
                let (room_l, room_r) = (cell.rect.l + t.padding, cell.rect.r - t.padding);
                let (gap_l, gap_r) = (l.left - room_l, room_r - l.right);
                if (gap_l - gap_r).abs() < 3.0 && gap_l > 4.0 {
                    Some("center")
                } else if gap_r < 3.0 && gap_l > 6.0 {
                    Some("right")
                } else {
                    None
                }
            });
            (text, style, align)
        };
        let heading: Vec<String> = (0..ncols).map(|c| t.cells.iter().find(|cell| cell.row == 0 && cell.col == c).map(|cell| text_of(cell).0).unwrap_or_default()).collect();
        let mut skip_rows = 0;
        let spans_now: Vec<(f32, f32)> = (0..ncols).map(|c| (t.xs[c], t.xs[c + 1])).collect();
        let continued = self.continues(page_top, &spans_now);
        if continued {
            if self.last.as_ref().is_some_and(|l| l.heading == heading) {
                skip_rows = 1;
            }
        } else if page_top {
            self.gap();
        } else {
            self.space_before(t.ys[0], 11.0);
        }
        let shift = self.align(&(0..ncols).map(|c| (t.xs[c], t.xs[c + 1])).collect::<Vec<_>>(), page_top);
        let aligned = shift.iter().enumerate().any(|(i, c)| *c != i as u32);
        let col_at = |c: usize| shift.get(c).copied().unwrap_or(c as u32);
        let base = self.row;
        let edge = |b: Option<from_pdf::Border>| b.map(|b| (if b.width >= 2.0 { Edge::Thick } else if b.width >= 1.2 { Edge::Medium } else { Edge::Thin }, b.color));
        for cell in &t.cells {
            if cell.row < skip_rows {
                continue;
            }
            let r = base + (cell.row - skip_rows) as u32;
            let c = col_at(cell.col);
            let (text, style, align) = text_of(cell);
            let mut look = style.as_ref().map(look_of).unwrap_or_default();
            look.fill = cell.fill;
            look.h = align;
            look.v = Some(match cell.valign {
                from_pdf::VAlign::Top => "top",
                from_pdf::VAlign::Center => "center",
                from_pdf::VAlign::Bottom => "bottom",
            })
            .filter(|v| *v != "bottom");
            look.wrap = text.contains('\n');
            let [top, right, bottom, left] = cell.borders;
            look.borders = [edge(left), edge(right), edge(top), edge(bottom)];
            self.put(r, c, &text, look.clone());
            if cell.rows > 1 || cell.cols > 1 {
                let (r2, c2) = (r + cell.rows as u32 - 1, col_at(cell.col + cell.cols - 1));
                self.sheet.merges.push((r, c, r2, c2));
                // the merged cells carry the borders too
                for rr in r..=r2 {
                    for cc in c..=c2 {
                        if (rr, cc) != (r, c) {
                            let mut l = Look { borders: look.borders, fill: look.fill, ..Default::default() };
                            if rr != r2 {
                                l.borders[3] = None;
                            }
                            self.sheet.cells.entry((rr, cc)).or_insert(w::Cell { value: Value::Empty, look: l });
                        }
                    }
                }
            } else if !aligned {
                self.width(c, cell.rect.w());
            }
        }
        for k in skip_rows..nrows {
            let h = t.ys[k + 1] - t.ys[k];
            if h > ROW * 1.3 {
                self.sheet.row_heights.insert(base + (k - skip_rows) as u32, h.min(409.0));
            }
        }
        self.row = base + (nrows - skip_rows) as u32;
        self.bottom = t.ys.last().copied();
        self.first_table = false;
        if !continued && !aligned {
            let spans = (0..ncols).map(|c| (t.xs[c], t.xs[c + 1], c as u32)).collect();
            self.last = Some(Placed { heading, spans });
        }
    }

    /// Sheet columns for columns at `cols` (left, right; left to right): under the table
    /// before when they sit under some of its columns (totals under their amounts),
    /// otherwise from column A
    pub(super) fn align(&self, cols: &[(f32, f32)], page_top: bool) -> Vec<u32> {
        let plain = || (0..cols.len() as u32).collect();
        let Some(last) = self.last.as_ref().filter(|last| cols.len() <= last.spans.len() && !page_top) else { return plain() };
        let mut used = 0usize;
        let mut out = vec![];
        let mut anchored = false;
        for (k, (l, r)) in cols.iter().enumerate() {
            let left_after = cols.len() - k - 1;
            let candidates = &last.spans[used..last.spans.len() - left_after];
            let centre = (l + r) / 2.0;
            let d = |s: &(f32, f32, u32)| ((s.0 + s.1) / 2.0 - centre).abs().min((s.1 - r).abs());
            let Some((j, s)) = candidates.iter().enumerate().min_by(|a, b| d(a.1).partial_cmp(&d(b.1)).unwrap_or(std::cmp::Ordering::Equal)) else { return plain() };
            if r.min(s.1) - l.max(s.0) > 0.0 || (r - s.1).abs() < 6.0 {
                anchored = true;
            }
            used += j + 1;
            out.push(s.2);
        }
        if anchored { out } else { plain() }
    }
}
