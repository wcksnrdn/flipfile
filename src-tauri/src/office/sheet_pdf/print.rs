//! Printing a workbook: each visible sheet split into pages, and every page drawn

use super::*;

/// Typst markup for every visible sheet of `wb`
pub fn print(wb: &Workbook, sep: Separators, file_name: &str) -> Result<Printed, String> {
    struct Planned<'a> {
        sheet: &'a Sheet,
        grid: Grid,
        pages: Vec<Page>,
        scale: f32,
        page_size: (f32, f32),
        titles: Option<(u32, u32)>,
        merged: HashMap<(u32, u32), Range>,
    }
    let mut plans = vec![];
    for sheet in &wb.sheets {
        let Some(range) = used_range(sheet, &wb.styles) else { continue };
        let range = (range.0, range.1, range.2.max(range.0), range.3.max(range.1));
        // merged regions, indexed by every cell they cover
        let mut merged = HashMap::new();
        for m in &sheet.merges {
            for r in m.0..=m.2 {
                for c in m.1..=m.3 {
                    merged.insert((r, c), *m);
                }
            }
        }
        let col_w = |c: u32| sheet.col_width(c);
        let auto = row_heights(sheet, &wb.styles, range, &col_w, &merged, sep, wb.date1904);
        let row_h = |r: u32| sheet.row_heights.get(&r).copied().or_else(|| auto.get(&r).copied()).unwrap_or(sheet.default_row_height);
        let mut xs = vec![0.0];
        for c in range.1..=range.3 {
            xs.push(xs.last().unwrap() + col_w(c));
        }
        let mut ys = vec![0.0];
        for r in range.0..=range.2 {
            ys.push(ys.last().unwrap() + row_h(r));
        }
        let grid = Grid { range, xs, ys };

        let p = &sheet.print;
        let [ml, mr, mt, mb, _, _] = p.margins;
        let total_w = *grid.xs.last().unwrap();
        let total_h = *grid.ys.last().unwrap();
        let smart = !is_set_up(sheet);
        let landscape = if smart { smart_landscape(p, total_w) } else { p.landscape };
        let page_size = if landscape { (p.paper.1, p.paper.0) } else { p.paper };
        let room_w = (page_size.0 - ml - mr).max(36.0);
        let room_h = (page_size.1 - mt - mb).max(36.0);

        let scale = if smart {
            // every column on the page, unless that makes the text unreadably small
            (room_w / total_w.max(1.0)).clamp(SMART_MIN_SCALE, 1.0)
        } else {
            match p.fit {
                Some((fw, fh)) => {
                    let sx = if fw > 0 { room_w * fw as f32 / total_w.max(1.0) } else { f32::MAX };
                    let sy = if fh > 0 { room_h * fh as f32 / total_h.max(1.0) } else { f32::MAX };
                    sx.min(sy).clamp(0.1, 1.0)
                }
                None => p.scale / 100.0,
            }
        };
        let mut scale = scale;
        let mut col_bands = bands(range.1, range.3, &|c| grid.w(c) * scale, room_w, &sheet.col_breaks);
        if smart && col_bands.len() > 1 {
            // too wide even when shrunk: spread the columns evenly over the pages it takes,
            // as large as that allows, rather than a full page and a sliver
            let n = col_bands.len() as f32;
            let mut try_scale = (room_w * n / total_w).min(1.0);
            while try_scale > scale {
                let b = bands(range.1, range.3, &|c| grid.w(c) * try_scale, room_w, &sheet.col_breaks);
                if b.len() == col_bands.len() {
                    scale = try_scale;
                    col_bands = b;
                    break;
                }
                try_scale *= 0.97;
            }
        }
        let mut titles = sheet.title_rows.filter(|t| t.0 <= t.1);
        // a table running over several pages repeats its headings on each, as people
        // would set it up to print
        if smart && titles.is_none() && total_h * scale > room_h {
            titles = heading_rows(sheet, &wb.styles, range);
        }
        let titles_h = titles.map(|(a, b)| (a.max(range.0)..=b.min(range.2)).map(|r| grid.h(r)).sum::<f32>()).unwrap_or(0.0);
        let row_bands = bands(range.0, range.2, &|r| grid.h(r) * scale, room_h - titles_h * scale, &sheet.row_breaks);
        let mut pages = vec![];
        if p.over_then_down {
            for rb in &row_bands {
                for cb in &col_bands {
                    pages.push(Page { rows: *rb, cols: *cb });
                }
            }
        } else {
            for cb in &col_bands {
                for rb in &row_bands {
                    pages.push(Page { rows: *rb, cols: *cb });
                }
            }
        }
        // a page with nothing on it (rows a drawing's size only seemed to reach) is left out
        let mut pages: Vec<Page> = pages.into_iter().filter(|pg| has_content(sheet, &wb.styles, pg)).collect();
        if pages.is_empty() {
            pages.push(Page { rows: (range.0, range.2), cols: (range.1, range.3) });
        }
        plans.push(Planned { sheet, grid, pages, scale, page_size, titles, merged });
    }
    if plans.is_empty() {
        return Err("the workbook has nothing to print (all sheets are empty)".into());
    }

    let total: usize = plans.iter().map(|p| p.pages.len()).sum();
    let mut preamble = String::new();
    // What every text and cell would otherwise repeat: the workbook's own font, and a
    // line box from ascender to descender (so g/j/p are not clipped at a cell's bottom)
    let default_font = wb.styles.first().map(|s| font_list(&s.font.name)).unwrap_or_default();
    let _ = write!(preamble, "#set text(top-edge: \"ascender\", bottom-edge: \"descender\"");
    if !default_font.is_empty() {
        let _ = write!(preamble, ", font: ({default_font},)");
    }
    preamble.push_str(")\n");
    let mut sheets = vec![];
    let mut pages = vec![];
    let mut files = vec![];
    let mut page_no = 0usize;
    let mut pic_files: HashMap<(usize, usize), String> = HashMap::new();
    for (si, plan) in plans.iter().enumerate() {
        let sheet = plan.sheet;
        let p = &sheet.print;
        let s = plan.scale;
        let (pw, ph) = plan.page_size;
        let [ml, mr, mt, mb, mh, mf] = p.margins;
        let mut out = String::new();
        let _ = writeln!(out, "#set page(width: {pw:.2}pt, height: {ph:.2}pt, margin: 0pt)");
        // A cell clips its text and keeps it off the gridlines (at this sheet's scale)
        let _ = writeln!(
            out,
            "#let cell(w, h, body) = block(width: w, height: h, clip: true, inset: (left: {:.2}pt, right: {:.2}pt, top: 0.5pt, bottom: 1pt), body)",
            PAD * s,
            PAD * s
        );
        sheets.push(out);
        let first_page = p.first_page.map(|n| n as usize).unwrap_or(page_no + 1);
        for (pi, page) in plan.pages.iter().enumerate() {
            let mut out = String::new();
            page_no += 1;
            let grid = &plan.grid;
            // rows on this page: the title rows (unless they are already here), then the band
            let mut rows: Vec<u32> = vec![];
            if let Some((t1, t2)) = plan.titles
                && page.rows.0 > t2 {
                    rows.extend(t1.max(grid.range.0)..=t2.min(grid.range.2));
                }
            rows.extend(page.rows.0..=page.rows.1);
            let cols: Vec<u32> = (page.cols.0..=page.cols.1).collect();
            // y of each printed row on the page (unscaled), x of each column
            let mut ry: HashMap<u32, f32> = HashMap::new();
            let mut acc = 0.0;
            for &r in &rows {
                ry.insert(r, acc);
                acc += grid.h(r);
            }
            let content_h = acc;
            let x0 = grid.x(page.cols.0);
            let content_w = grid.x(page.cols.1 + 1) - x0;
            let room_w = pw - ml - mr;
            let room_h = ph - mt - mb;
            let ox = ml + if p.center_h { ((room_w - content_w * s) / 2.0).max(0.0) } else { 0.0 };
            let oy = mt + if p.center_v { ((room_h - content_h * s) / 2.0).max(0.0) } else { 0.0 };
            let px = |x: f32| ox + (x - x0) * s;
            let py = |y: f32| oy + y * s;
            let row_set: HashSet<u32> = rows.iter().copied().collect();
            let col_set = page.cols;

            // A merged region is drawn once per page, from its first cell visible there;
            // its value and style live in its top-left cell (which may be on another page)
            let anchor = |r: u32, c: u32| -> Option<(u32, u32)> {
                match plan.merged.get(&(r, c)) {
                    None => Some((r, c)),
                    Some(m) => {
                        let first_r = rows.iter().copied().find(|rr| *rr >= m.0 && *rr <= m.2)?;
                        let first_c = c.max(m.1).max(page.cols.0).min(m.1.max(page.cols.0));
                        ((r, c) == (first_r, first_c)).then_some((m.0, m.1))
                    }
                }
            };
            // 1. fills
            for &r in &rows {
                for &c in &cols {
                    let Some(src) = anchor(r, c) else { continue };
                    let Some(cell) = sheet.cells.get(&src).or_else(|| sheet.cells.get(&(r, c))) else { continue };
                    let Some(fill) = wb.styles[cell.style].fill else { continue };
                    let (w, h) = cell_box(grid, &plan.merged, r, c, &row_set, col_set);
                    if w <= 0.0 || h <= 0.0 {
                        continue;
                    }
                    let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, rect(width: {:.2}pt, height: {:.2}pt, fill: {}, stroke: none))", px(grid.x(c)), py(ry[&r]), w * s, h * s, rgb(fill));
                }
            }
            // data bars and icons of conditional formats
            let mut marks: Vec<_> = sheet.marks.iter().filter(|((r, c), _)| row_set.contains(r) && *c >= page.cols.0 && *c <= page.cols.1).collect();
            marks.sort_by_key(|(pos, _)| **pos);
            for (&(r, c), mark) in marks {
                let (x, y, w, h) = (px(grid.x(c)), py(ry[&r]), grid.w(c) * s, grid.h(r) * s);
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let inset = (1.5 * s).min(h / 4.0);
                match mark {
                    Mark::Bar { frac, color } => {
                        let bw = (w - 2.0 * inset) * frac;
                        let _ = writeln!(
                            out,
                            "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, rect(width: {bw:.2}pt, height: {:.2}pt, fill: gradient.linear({c}, {c}.lighten(85%)), stroke: 0.4pt + {c}))",
                            x + inset,
                            y + inset,
                            h - 2.0 * inset,
                            c = rgb(*color)
                        );
                    }
                    Mark::Icon { set, index, of, .. } => {
                        let size = (12.0 * s).min(h - 2.0 * inset).max(2.0);
                        let style = sheet.cells.get(&(r, c)).map(|cell| &wb.styles[cell.style]);
                        let dy = match style.map(|st| st.v) {
                            Some(VAlign::Top) => inset,
                            Some(VAlign::Center) => (h - size) / 2.0,
                            _ => h - size - inset,
                        };
                        let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, {})", x + inset, y + dy, icon(set, *index, *of, size));
                    }
                }
            }
            // 2. gridlines
            if p.gridlines {
                let stroke = "(paint: rgb(160, 160, 160), thickness: 0.4pt)";
                let bottom = py(content_h);
                for c in page.cols.0..=page.cols.1 + 1 {
                    let x = px(grid.x(c));
                    let _ = writeln!(out, "#place(top + left, line(start: ({x:.2}pt, {:.2}pt), end: ({x:.2}pt, {bottom:.2}pt), stroke: {stroke}))", py(0.0));
                }
                let mut y = 0.0;
                let right = px(grid.x(page.cols.1 + 1));
                for r in rows.iter().map(Some).chain([None]) {
                    let yy = py(y);
                    let _ = writeln!(out, "#place(top + left, line(start: ({:.2}pt, {yy:.2}pt), end: ({right:.2}pt, {yy:.2}pt), stroke: {stroke}))", px(x0));
                    if let Some(r) = r {
                        y += grid.h(*r);
                    }
                }
            }
            // 3. text
            for &r in &rows {
                for &c in &cols {
                    let Some(src) = anchor(r, c) else { continue };
                    let Some(cell) = sheet.cells.get(&src) else { continue };
                    let Some(value) = &cell.value else { continue };
                    if matches!(sheet.marks.get(&src), Some(Mark::Icon { show_value: false, .. })) {
                        continue;
                    }
                    let style = &wb.styles[cell.style];
                    let (cw, ch) = cell_box(grid, &plan.merged, r, c, &row_set, col_set);
                    if cw <= 0.0 || ch <= 0.0 {
                        continue;
                    }
                    let shown = numfmt::format(value, &style.num_fmt, wb.date1904, sep);
                    let is_number = matches!(value, Value::Number(_));
                    let h = match style.h {
                        HAlign::General => match value {
                            Value::Number(_) => HAlign::Right,
                            Value::Bool(_) | Value::Error(_) => HAlign::Center,
                            Value::Text(_) => HAlign::Left,
                        },
                        other => other,
                    };
                    // room for text that does not wrap: it overflows into empty neighbours
                    let in_merge = plan.merged.contains_key(&(r, c));
                    let (mut left, mut right) = (grid.x(c), grid.x(c) + cw);
                    if !style.wrap && !in_merge && !is_number && shown.fill.is_none() && style.rotation == 0 && !style.stacked {
                        let free = |cc: u32| !sheet.cells.get(&(r, cc)).map(|x| x.value.is_some()).unwrap_or(false) && !plan.merged.contains_key(&(r, cc));
                        if matches!(h, HAlign::Left | HAlign::Center | HAlign::CenterContinuous | HAlign::Justify) {
                            let mut cc = c + 1;
                            while cc <= page.cols.1 && free(cc) {
                                right = grid.x(cc + 1);
                                cc += 1;
                            }
                        }
                        if matches!(h, HAlign::Right | HAlign::Center | HAlign::CenterContinuous) {
                            let mut cc = c;
                            while cc > page.cols.0 && free(cc - 1) {
                                cc -= 1;
                                left = grid.x(cc);
                            }
                        }
                        // centred text overflows evenly
                        if matches!(h, HAlign::Center | HAlign::CenterContinuous) {
                            let mid = grid.x(c) + cw / 2.0;
                            let half = (mid - left).min(right - mid);
                            left = mid - half;
                            right = mid + half;
                        }
                    }
                    if let Some(m) = plan.merged.get(&(r, c)) {
                        // Excel aligns merged text over the whole merged width, even when
                        // the merge runs across pages: lay it out full width, show this page's slice
                        let full_l = grid.x(m.1.max(grid.range.1));
                        let full_r = grid.x(m.3.min(grid.range.3) + 1);
                        let vis_l = grid.x(c.max(page.cols.0));
                        let pieces = cell_pieces(&shown, style, sheet.rich.get(&src), value);
                        let markup = cell_text(&shown, pieces, style, h, is_number, full_r - full_l, ch, s, &default_font);
                        let _ = writeln!(
                            out,
                            "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, block(width: {:.2}pt, height: {:.2}pt, clip: true, place(top + left, dx: {:.2}pt, {markup})))",
                            px(vis_l),
                            py(ry[&r]),
                            cw * s,
                            ch * s,
                            (full_l - vis_l) * s
                        );
                        continue;
                    }
                    let pieces = cell_pieces(&shown, style, sheet.rich.get(&src), value);
                    let markup = cell_text(&shown, pieces, style, h, is_number, right - left, ch, s, &default_font);
                    let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, {markup})", px(left), py(ry[&r]));
                }
            }
            // 4. borders
            for &r in &rows {
                for &c in &cols {
                    let Some(cell) = sheet.cells.get(&(r, c)) else { continue };
                    let b = &wb.styles[cell.style].borders;
                    let (x1, y1) = (px(grid.x(c)), py(ry[&r]));
                    let (x2, y2) = (px(grid.x(c + 1)), py(ry[&r] + grid.h(r)));
                    if x2 <= x1 || y2 <= y1 {
                        continue;
                    }
                    let edges = [(b.top, (x1, y1, x2, y1)), (b.bottom, (x1, y2, x2, y2)), (b.left, (x1, y1, x1, y2)), (b.right, (x2, y1, x2, y2)), (b.up, (x1, y2, x2, y1)), (b.down, (x1, y1, x2, y2))];
                    for (edge, (a, bb, cc, d)) in edges {
                        if let Some(e) = edge {
                            draw_edge(&mut out, e, a, bb, cc, d, s);
                        }
                    }
                }
            }
            // 5. pictures
            for (k, pic) in sheet.pictures.iter().enumerate() {
                let (fr, fc, froff, fcoff) = pic.from;
                if fr < grid.range.0 || fc < grid.range.1 || fr > grid.range.2 + 1 || fc > grid.range.3 + 1 {
                    continue;
                }
                let sx = grid.xs.get((fc - grid.range.1) as usize).copied().unwrap_or(0.0) + fcoff;
                let sy = grid.ys.get((fr - grid.range.0) as usize).copied().unwrap_or(0.0) + froff;
                let (w, h) = match pic.to {
                    Some((tr, tc, troff, tcoff)) => {
                        let ex = grid.xs.get((tc.max(grid.range.1) - grid.range.1) as usize).copied().unwrap_or(*grid.xs.last().unwrap()) + tcoff;
                        let ey = grid.ys.get((tr.max(grid.range.0) - grid.range.0) as usize).copied().unwrap_or(*grid.ys.last().unwrap()) + troff;
                        ((ex - sx).max(1.0), (ey - sy).max(1.0))
                    }
                    None => pic.size,
                };
                // on this page when it starts inside the page's columns and printed rows
                let page_x = (grid.x(page.cols.0), grid.x(page.cols.1 + 1));
                let page_y = (grid.y(page.rows.0), grid.y(page.rows.1 + 1));
                if sx < page_x.0 - 0.5 || sx >= page_x.1 || sy < page_y.0 - 0.5 || sy >= page_y.1 {
                    continue;
                }
                let name = pic_files.entry((si, k)).or_insert_with(|| {
                    let (bytes, ext) = picture_bytes(&pic.data, &pic.ext);
                    let name = format!("/pic-{si}-{k}.{ext}");
                    files.push((name.clone(), bytes));
                    name
                });
                let offset_rows: f32 = rows.iter().take_while(|&&r| r < page.rows.0).map(|&r| grid.h(r)).sum();
                let y_on_page = offset_rows + (sy - page_y.0);
                let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, image({}, width: {:.2}pt, height: {:.2}pt, fit: \"stretch\"))", px(sx), py(y_on_page), typst_str(name), w * s, h * s);
            }
            // 5b. charts and shapes, placed like pictures
            for d in &sheet.drawn {
                let a = d.anchor();
                let (fr, fc, froff, fcoff) = a.from;
                if fr < grid.range.0 || fc < grid.range.1 || fr > grid.range.2 + 1 || fc > grid.range.3 + 1 {
                    continue;
                }
                let sx = grid.xs.get((fc - grid.range.1) as usize).copied().unwrap_or(0.0) + fcoff;
                let sy = grid.ys.get((fr - grid.range.0) as usize).copied().unwrap_or(0.0) + froff;
                let (w, h) = match a.to {
                    Some((tr, tc, troff, tcoff)) => {
                        let ex = grid.xs.get((tc.max(grid.range.1) - grid.range.1) as usize).copied().unwrap_or(*grid.xs.last().unwrap()) + tcoff;
                        let ey = grid.ys.get((tr.max(grid.range.0) - grid.range.0) as usize).copied().unwrap_or(*grid.ys.last().unwrap()) + troff;
                        ((ex - sx).max(1.0), (ey - sy).max(1.0))
                    }
                    None => a.size,
                };
                let page_x = (grid.x(page.cols.0), grid.x(page.cols.1 + 1));
                let page_y = (grid.y(page.rows.0), grid.y(page.rows.1 + 1));
                if sx < page_x.0 - 0.5 || sx >= page_x.1 || sy < page_y.0 - 0.5 || sy >= page_y.1 {
                    continue;
                }
                let offset_rows: f32 = rows.iter().take_while(|&&r| r < page.rows.0).map(|&r| grid.h(r)).sum();
                let (x, y) = (px(sx), py(offset_rows + (sy - page_y.0)));
                match d {
                    crate::office::xlsx::Drawn::Chart(_, chart) => {
                        let body = crate::office::chart::markup(chart, w, h, sep);
                        let _ = writeln!(
                            out,
                            "#place(top + left, dx: {x:.2}pt, dy: {y:.2}pt, scale(x: {pc:.3}%, y: {pc:.3}%, origin: top + left, reflow: true, box(width: {w:.2}pt, height: {h:.2}pt, clip: true)[\n{body}]))",
                            pc = s * 100.0
                        );
                    }
                    crate::office::xlsx::Drawn::Shapes(_, shapes) => {
                        for sh in shapes {
                            let [fx, fy, fw, fh] = sh.frac;
                            let markup = crate::office::drawing::shape_markup(sh, fw * w * s, fh * h * s, s);
                            let _ = writeln!(out, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, {markup})", x + fx * w * s, y + fy * h * s);
                        }
                    }
                }
            }
            // 6. header and footer
            let number = first_page + pi;
            for (spec, top) in [(&p.header, true), (&p.footer, false)] {
                let Some(spec) = spec else { continue };
                let parts = header_parts(spec, number, total, file_name, &sheet.name);
                for (i, text) in parts.iter().enumerate() {
                    if text.trim().is_empty() {
                        continue;
                    }
                    let align = ["left", "center", "right"][i];
                    let y = if top { format!("dy: {mh:.2}pt") } else { format!("dy: {:.2}pt", ph - mf - 14.0) };
                    let style = Style { font: wb.styles[0].font.clone(), ..Default::default() };
                    let t = text_expr(text, &style, None, 1.0, &default_font);
                    let _ = writeln!(out, "#place(top + left, dx: {ml:.2}pt, {y}, block(width: {:.2}pt, align({align}, {t})))", pw - ml - mr);
                }
            }
            out.push('\n');
            pages.push((sheets.len() - 1, out));
        }
    }
    Ok(Printed { preamble, sheets, pages, files })
}
