//! One sheet: its cells, column widths, conditional formats, tables and paper

use super::*;

/// Pixel width of a column `width` (in characters), as Excel computes it
pub(super) fn col_px(width: f32, mdw: f32) -> f32 {
    (((256.0 * width + (128.0 / mdw).trunc()) / 256.0) * mdw).trunc()
}

/// The widest digit of the workbook's default font, in pixels (7 for Calibri 11)
pub(super) fn max_digit_width(font: &Font) -> f32 {
    let w = crate::office::typeset::text_width(&font.name, font.size, false, false, "0")
        .or_else(|| crate::office::typeset::text_width("Calibri", font.size, false, false, "0"))
        .map(|pt| (pt * 96.0 / 72.0).trunc())
        .unwrap_or((font.size * 7.0 / 11.0).round());
    w.clamp(4.0, 30.0)
}

pub(super) fn read_sheet(xml: &str, strings: &[(String, Option<Rich>)], styles: &[Style], mdw: f32, colors: &Colors) -> Result<Sheet, String> {
    let doc = Document::parse(xml).map_err(|_| "damaged sheet".to_string())?;
    let root = doc.root_element();
    let mut sheet = Sheet::default();

    if let Some(pane) = child(root, "sheetViews").and_then(|v| child(v, "sheetView")).and_then(|v| child(v, "pane"))
        && matches!(pane.attribute("state"), Some("frozen") | Some("frozenSplit")) {
            sheet.frozen = (attr_f32(pane, "ySplit").unwrap_or(0.0) as u32, attr_f32(pane, "xSplit").unwrap_or(0.0) as u32);
        }

    // default sizes
    let fmt = child(root, "sheetFormatPr");
    let base_font = &styles[0].font;
    sheet.default_row_height = fmt
        .and_then(|f| attr_f32(f, "defaultRowHeight"))
        .unwrap_or_else(|| (base_font.size * 15.0 / 11.0).max(12.0));
    let default_px = if let Some(w) = fmt.and_then(|f| attr_f32(f, "defaultColWidth")) {
        col_px(w, mdw)
    } else {
        let base = fmt.and_then(|f| attr_f32(f, "baseColWidth")).unwrap_or(8.0);
        // Excel rounds the default column up to a multiple of 8 pixels
        ((base * mdw + 5.0) / 8.0).ceil() * 8.0
    };
    sheet.default_col_width = default_px * 0.75;
    sheet.auto_rows = true;

    if let Some(cols) = child(root, "cols") {
        for c in children(cols, "col") {
            let (Some(min), Some(max)) = (attr_u32(c, "min"), attr_u32(c, "max")) else { continue };
            let hidden = attr_bool(c, "hidden").unwrap_or(false);
            let w = if hidden { 0.0 } else { attr_f32(c, "width").map(|w| col_px(w, mdw) * 0.75).unwrap_or(sheet.default_col_width) };
            if let Some(st) = attr_u32(c, "style").filter(|&st| st > 0 && (st as usize) < styles.len()) {
                for i in min.max(1)..=max.min(16384) {
                    sheet.col_styles.insert(i - 1, st as usize);
                }
            }
            // a <col> spanning the rest of the sheet only sets the default
            if max - min > 16000 {
                sheet.default_col_width = w;
                continue;
            }
            for i in min.max(1)..=max.min(16384) {
                sheet.col_widths.insert(i - 1, w);
            }
        }
    }

    // shared formulas: index -> (text, the cell it was written for)
    let mut shared: HashMap<String, (Arc<str>, u32, u32)> = HashMap::new();
    if let Some(data) = child(root, "sheetData") {
        for (ri, row) in children(data, "row").enumerate() {
            let r = attr_u32(row, "r").map(|r| r - 1).unwrap_or(ri as u32);
            if attr_bool(row, "hidden").unwrap_or(false) {
                sheet.row_heights.insert(r, 0.0);
            } else if let Some(ht) = attr_f32(row, "ht") {
                sheet.row_heights.insert(r, ht);
            }
            let row_style = attr_u32(row, "s").filter(|_| attr_bool(row, "customFormat").unwrap_or(false));
            let mut next_col = 0u32;
            for c in children(row, "c") {
                let (cr, cc) = c.attribute("r").and_then(cell_ref).unwrap_or((r, next_col));
                next_col = cc + 1;
                let style = attr_u32(c, "s").or(row_style).unwrap_or(0) as usize;
                let style = if style < styles.len() { style } else { 0 };
                let v = child(c, "v").and_then(|v| v.text());
                let value = match c.attribute("t").unwrap_or("n") {
                    "s" => v.and_then(|i| i.trim().parse::<usize>().ok()).and_then(|i| strings.get(i)).map(|(s, rich)| {
                        if let Some(rich) = rich {
                            sheet.rich.insert((cr, cc), rich.clone());
                        }
                        Value::Text(s.clone())
                    }),
                    "str" => v.map(|s| Value::Text(s.to_string())),
                    "inlineStr" => child(c, "is").map(|is| {
                        if let Some(rich) = runs(is, colors) {
                            sheet.rich.insert((cr, cc), rich);
                        }
                        Value::Text(rich_text(is))
                    }),
                    "b" => v.map(|s| Value::Bool(s.trim() == "1")),
                    "e" => v.map(|s| Value::Error(s.to_string())),
                    "d" => v.map(|s| Value::Text(s.to_string())),
                    _ => v.and_then(|s| s.trim().parse::<f64>().ok()).map(Value::Number),
                };
                let value = value.filter(|v| !matches!(v, Value::Text(t) if t.is_empty()));
                // a formula: kept to work out when the file has no result for it
                let mut formula = false;
                if let Some(f) = child(c, "f") {
                    let text = f.text().map(str::trim).filter(|t| !t.is_empty());
                    let si = f.attribute("si").filter(|_| f.attribute("t") == Some("shared"));
                    let pending = match (text, si) {
                        (Some(t), si) => {
                            let text: Arc<str> = Arc::from(t);
                            if let Some(si) = si {
                                shared.insert(si.to_string(), (text.clone(), cr, cc));
                            }
                            Some(crate::office::formula::Pending { text, shift: (0, 0) })
                        }
                        (None, Some(si)) => shared.get(si).map(|(text, r0, c0)| crate::office::formula::Pending { text: text.clone(), shift: (cr as i64 - *r0 as i64, cc as i64 - *c0 as i64) }),
                        _ => None,
                    };
                    if let (Some(p), None) = (pending, &value) {
                        sheet.formulas.insert((cr, cc), p);
                        formula = true;
                    }
                }
                if value.is_none() && style == 0 && !formula {
                    continue;
                }
                sheet.cells.insert((cr, cc), Cell { value, style });
            }
        }
    }

    if let Some(m) = child(root, "mergeCells") {
        sheet.merges = children(m, "mergeCell").filter_map(|n| n.attribute("ref").and_then(range_ref)).collect();
    }

    let p = &mut sheet.print;
    if let Some(o) = child(root, "printOptions") {
        p.gridlines = attr_bool(o, "gridLines").unwrap_or(false);
        p.center_h = attr_bool(o, "horizontalCentered").unwrap_or(false);
        p.center_v = attr_bool(o, "verticalCentered").unwrap_or(false);
    }
    if let Some(m) = child(root, "pageMargins") {
        let inch = |n: &str, d: f32| attr_f32(m, n).unwrap_or(d) * 72.0;
        p.margins = [inch("left", 0.7), inch("right", 0.7), inch("top", 0.75), inch("bottom", 0.75), inch("header", 0.3), inch("footer", 0.3)];
    }
    if let Some(s) = child(root, "pageSetup") {
        if let Some(size) = attr_u32(s, "paperSize").and_then(paper) {
            p.paper = size;
        }
        p.landscape = s.attribute("orientation") == Some("landscape");
        // Excel's range is 10-400%; other programs write 0 (or nonsense) for "not set"
        p.scale = attr_f32(s, "scale").filter(|v| (10.0..=400.0).contains(v)).unwrap_or(100.0);
        p.explicit |= p.landscape || p.scale != 100.0;
        p.over_then_down = s.attribute("pageOrder") == Some("overThenDown");
        if attr_bool(s, "useFirstPageNumber").unwrap_or(false) {
            p.first_page = attr_u32(s, "firstPageNumber");
        }
        let fit_on = child(root, "sheetPr").and_then(|sp| child(sp, "pageSetUpPr")).and_then(|u| attr_bool(u, "fitToPage")).unwrap_or(false);
        if fit_on {
            p.fit = Some((attr_u32(s, "fitToWidth").unwrap_or(1), attr_u32(s, "fitToHeight").unwrap_or(1)));
            p.explicit = true;
        }
    }
    if let Some(hf) = child(root, "headerFooter") {
        p.header = child(hf, "oddHeader").and_then(|n| n.text()).map(str::to_string);
        p.footer = child(hf, "oddFooter").and_then(|n| n.text()).map(str::to_string);
    }
    let breaks = |name: &str| -> Vec<u32> {
        child(root, name).map(|b| children(b, "brk").filter_map(|n| attr_u32(n, "id")).filter(|&i| i > 0).collect()).unwrap_or_default()
    };
    // a break "at id" puts row/column `id` (zero-based) first on the next page
    sheet.row_breaks = breaks("rowBreaks");
    sheet.col_breaks = breaks("colBreaks");
    sheet.conds = children(root, "conditionalFormatting").filter_map(|cf| cond_format(cf, colors)).collect();
    Ok(sheet)
}

/// A `<conditionalFormatting>`: its ranges and rules
fn cond_format(cf: Node, colors: &Colors) -> Option<CondFormat> {
    let ranges: Vec<Range> = cf.attribute("sqref")?.split_whitespace().filter_map(range_ref).collect();
    if ranges.is_empty() {
        return None;
    }
    let cfvos = |n: Node| -> Vec<Cfvo> {
        children(n, "cfvo").map(|v| Cfvo { kind: v.attribute("type").unwrap_or("min").to_string(), val: v.attribute("val").map(str::to_string), gte: attr_bool(v, "gte").unwrap_or(true) }).collect()
    };
    let rules = children(cf, "cfRule")
        .map(|r| {
            let rule = r.attribute("type").unwrap_or("").to_string();
            let kind = if let Some(n) = child(r, "colorScale") {
                CfKind::Scale(cfvos(n), children(n, "color").filter_map(|c| colors.get(c)).collect())
            } else if let Some(n) = child(r, "dataBar") {
                let v = cfvos(n);
                let lo = v.first().cloned().unwrap_or(Cfvo { kind: "min".into(), val: None, gte: true });
                let hi = v.get(1).cloned().unwrap_or(Cfvo { kind: "max".into(), val: None, gte: true });
                CfKind::Bar(lo, hi, child(n, "color").and_then(|c| colors.get(c)).unwrap_or([0x63, 0x8E, 0xC6]))
            } else if let Some(n) = child(r, "iconSet") {
                CfKind::Icons {
                    set: n.attribute("iconSet").unwrap_or("3TrafficLights1").to_string(),
                    cfvos: cfvos(n),
                    reverse: attr_bool(n, "reverse").unwrap_or(false),
                    show_value: attr_bool(n, "showValue").unwrap_or(true),
                }
            } else {
                CfKind::Rule
            };
            CfRule {
                kind,
                rule,
                priority: r.attribute("priority").and_then(|p| p.parse().ok()).unwrap_or(i32::MAX),
                dxf: attr_u32(r, "dxfId").map(|i| i as usize),
                operator: r.attribute("operator").map(str::to_string),
                formulas: children(r, "formula").filter_map(|f| f.text()).map(str::to_string).collect(),
                text: r.attribute("text").map(str::to_string),
                stop: attr_bool(r, "stopIfTrue").unwrap_or(false),
                rank: attr_u32(r, "rank").unwrap_or(10),
                percent: attr_bool(r, "percent").unwrap_or(false),
                bottom: attr_bool(r, "bottom").unwrap_or(false),
                above: attr_bool(r, "aboveAverage").unwrap_or(true),
                equal_average: attr_bool(r, "equalAverage").unwrap_or(false),
                time_period: r.attribute("timePeriod").map(str::to_string),
            }
        })
        .collect();
    Some(CondFormat { ranges, rules })
}

/// One side of a `<border>` (also in differential formats)
pub(super) fn border_edge(b: Node, name: &str, colors: &Colors) -> Option<Edge> {
    let e = child(b, name)?;
    let line = match e.attribute("style")? {
        "hair" => Line::Hair,
        "thin" => Line::Thin,
        "medium" => Line::Medium,
        "thick" => Line::Thick,
        "double" => Line::Double,
        "dotted" => Line::Dotted,
        "dashed" => Line::Dashed,
        "mediumDashed" => Line::MediumDashed,
        "dashDot" | "dashDotDot" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => Line::DashDot,
        _ => return None,
    };
    let color = child(e, "color").and_then(|c| colors.get(c)).unwrap_or([0, 0, 0]);
    Some(Edge { line, color })
}

/// The tables (Format as Table) of a sheet
pub(super) fn read_tables(pkg: &mut Package, sheet_path: &str, sheet_xml: &str) -> Vec<TableDef> {
    let Ok(doc) = Document::parse(sheet_xml) else { return vec![] };
    let Some(parts) = child(doc.root_element(), "tableParts") else { return vec![] };
    let rels = pkg.rels(sheet_path);
    let mut out = vec![];
    for part in children(parts, "tablePart") {
        let Some(path) = part.attribute((REL, "id")).and_then(|id| rels.get(id)).cloned() else { continue };
        let Some(xml) = pkg.text(&path) else { continue };
        let Ok(t) = Document::parse(&xml) else { continue };
        let t = t.root_element();
        let Some(range) = t.attribute("ref").and_then(range_ref) else { continue };
        let info = child(t, "tableStyleInfo");
        let flag = |name: &str| info.and_then(|i| attr_bool(i, name)).unwrap_or(false);
        out.push(TableDef {
            range,
            header_rows: attr_u32(t, "headerRowCount").unwrap_or(1),
            totals_rows: attr_u32(t, "totalsRowCount").unwrap_or(0),
            style: info.and_then(|i| i.attribute("name")).map(str::to_string),
            row_stripes: flag("showRowStripes"),
            col_stripes: flag("showColumnStripes"),
            first_col: flag("showFirstColumn"),
            last_col: flag("showLastColumn"),
        });
    }
    out
}

/// Paper sizes Excel writes in `paperSize`, in points (portrait)
pub(super) fn paper(code: u32) -> Option<(f32, f32)> {
    Some(match code {
        1 => (612.0, 792.0),   // Letter
        3 => (792.0, 1224.0),  // Tabloid
        5 => (612.0, 1008.0),  // Legal
        7 => (522.0, 756.0),   // Executive
        8 => (841.9, 1190.6),  // A3
        9 => (595.3, 841.9),   // A4
        11 => (419.5, 595.3),  // A5
        12 => (708.7, 1000.6), // B4 (JIS)
        13 => (515.9, 728.5),  // B5 (JIS)
        14 => (612.0, 936.0),  // Folio / F4 (8.5 x 13 in), common in Indonesia
        _ => return None,
    })
}
