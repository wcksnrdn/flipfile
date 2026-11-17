//! Writing Tabular Formats (XLSX, CSV, TSV, JSON, YAML, XML, SQL, MD-Table)

use super::*;

pub(super) fn write_tabular(sheets: &[TableSheet], format: &str, input: &Path) -> Result<Vec<u8>, String> {
    match format {
        "xlsx" => write_xlsx(sheets),
        "csv" => Ok(write_delimited(sheets, b',')),
        "tsv" => Ok(write_delimited(sheets, b'\t')),
        "json" => Ok(write_json(sheets).into_bytes()),
        "yaml" => Ok(write_yaml(sheets).into_bytes()),
        "xml" => Ok(write_xml(sheets).into_bytes()),
        "sql" => {
            let table_name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("data");
            Ok(write_sql(sheets, table_name).into_bytes())
        }
        "md-table" => Ok(write_md_tables(sheets).into_bytes()),
        _ => Err("Unsupported tabular format".into()),
    }
}

fn write_delimited(sheets: &[TableSheet], delim: u8) -> Vec<u8> {
    let d = delim as char;
    let mut out = String::new();
    if let Some(sheet) = sheets.first() {
        for row in &sheet.rows {
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    out.push(d);
                }
                if cell.contains(d) || cell.contains('"') || cell.contains('\n') || cell.contains('\r') {
                    out.push('"');
                    out.push_str(&cell.replace('"', "\"\""));
                    out.push('"');
                } else {
                    out.push_str(cell);
                }
            }
            out.push('\n');
        }
    }
    out.into_bytes()
}

fn parse_typed_json(s: &str) -> serde_json::Value {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Null;
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return serde_json::Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return serde_json::Value::Bool(false);
    }
    // Preserve leading-zero codes (e.g. "0012") as strings
    if !(trimmed.starts_with('0') && trimmed.len() > 1 && !trimmed.starts_with("0.")) {
        if let Ok(i) = trimmed.parse::<i64>() {
            return serde_json::Value::Number(i.into());
        }
        if let Ok(f) = trimmed.parse::<f64>()
            && let Some(n) = serde_json::Number::from_f64(f) {
                return serde_json::Value::Number(n);
            }
    }
    serde_json::Value::String(s.to_string())
}

fn sheet_to_json_records(sheet: &TableSheet) -> serde_json::Value {
    if sheet.rows.len() < 2 {
        return serde_json::Value::Array(
            sheet
                .rows
                .iter()
                .map(|r| serde_json::Value::Array(r.iter().map(|c| parse_typed_json(c)).collect()))
                .collect(),
        );
    }
    let headers: Vec<String> = sheet.rows[0]
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let t = h.trim();
            if t.is_empty() { format!("col_{}", i + 1) } else { t.to_string() }
        })
        .collect();
    let mut arr = Vec::with_capacity(sheet.rows.len() - 1);
    for row in &sheet.rows[1..] {
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let mut obj = serde_json::Map::new();
        for (i, h) in headers.iter().enumerate() {
            let val = row.get(i).map(|s| s.as_str()).unwrap_or("");
            obj.insert(h.clone(), parse_typed_json(val));
        }
        arr.push(serde_json::Value::Object(obj));
    }
    serde_json::Value::Array(arr)
}

fn write_json(sheets: &[TableSheet]) -> String {
    let val = if sheets.len() == 1 {
        sheet_to_json_records(&sheets[0])
    } else {
        let mut map = serde_json::Map::new();
        for s in sheets {
            map.insert(s.name.clone(), sheet_to_json_records(s));
        }
        serde_json::Value::Object(map)
    };
    serde_json::to_string_pretty(&val).unwrap_or_else(|_| "[]".into())
}

fn write_yaml(sheets: &[TableSheet]) -> String {
    let mut out = String::from("---\n");
    for sheet in sheets {
        if sheets.len() > 1 {
            out.push_str(&format!("{}:\n", yaml_ident(&sheet.name)));
        }
        if sheet.rows.len() >= 2 {
            let headers = &sheet.rows[0];
            for row in &sheet.rows[1..] {
                if row.iter().all(|c| c.trim().is_empty()) {
                    continue;
                }
                for (i, h) in headers.iter().enumerate() {
                    let key = if h.trim().is_empty() { format!("col_{}", i + 1) } else { h.trim().to_string() };
                    let val = row.get(i).map(String::as_str).unwrap_or("");
                    let prefix = if i == 0 { "- " } else { "  " };
                    let indent = if sheets.len() > 1 { "  " } else { "" };
                    out.push_str(&format!("{indent}{prefix}{key}: {}\n", yaml_scalar(val)));
                }
            }
        } else {
            for row in &sheet.rows {
                out.push_str(&format!("- {:?}\n", row));
            }
        }
    }
    out
}

fn yaml_ident(s: &str) -> String {
    let clean: String = s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if clean.is_empty() { "sheet".into() } else { clean }
}

fn yaml_scalar(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "\"\"".into();
    }
    if t.parse::<f64>().is_ok() || t == "true" || t == "false" {
        return t.to_string();
    }
    if t.contains(':') || t.contains('#') || t.contains('\n') || t.contains('"') {
        format!("{t:?}")
    } else {
        t.to_string()
    }
}

fn write_xml(sheets: &[TableSheet]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<workbook>\n");
    for sheet in sheets {
        out.push_str(&format!("  <sheet name=\"{}\">\n", xml_escape(&sheet.name)));
        if sheet.rows.len() >= 2 {
            let tags: Vec<String> = sheet.rows[0]
                .iter()
                .enumerate()
                .map(|(i, h)| sanitize_xml_tag(h, i))
                .collect();
            for row in &sheet.rows[1..] {
                if row.iter().all(|c| c.trim().is_empty()) {
                    continue;
                }
                out.push_str("    <record>\n");
                for (i, tag) in tags.iter().enumerate() {
                    let val = row.get(i).map(String::as_str).unwrap_or("");
                    out.push_str(&format!("      <{tag}>{}</{tag}>\n", xml_escape(val)));
                }
                out.push_str("    </record>\n");
            }
        } else {
            for row in &sheet.rows {
                out.push_str("    <row>\n");
                for cell in row {
                    out.push_str(&format!("      <cell>{}</cell>\n", xml_escape(cell)));
                }
                out.push_str("    </row>\n");
            }
        }
        out.push_str("  </sheet>\n");
    }
    out.push_str("</workbook>\n");
    out
}

fn sanitize_xml_tag(s: &str, idx: usize) -> String {
    let mut out = String::new();
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || (ch == '-' && !out.is_empty()) {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() || out.chars().next().unwrap().is_ascii_digit() {
        format!("col_{}", idx + 1)
    } else {
        out
    }
}

fn write_sql(sheets: &[TableSheet], default_table: &str) -> String {
    let mut out = String::new();
    for sheet in sheets {
        if sheet.rows.is_empty() {
            continue;
        }
        let table = sanitize_xml_tag(if sheets.len() == 1 { default_table } else { &sheet.name }, 0);
        let cols: Vec<String> = sheet.rows[0]
            .iter()
            .enumerate()
            .map(|(i, h)| sanitize_xml_tag(h, i))
            .collect();
        let col_defs: Vec<String> = cols.iter().map(|c| format!("  \"{c}\" TEXT")).collect();
        out.push_str(&format!("CREATE TABLE IF NOT EXISTS \"{table}\" (\n{}\n);\n\n", col_defs.join(",\n")));

        let data_rows = if sheet.rows.len() > 1 { &sheet.rows[1..] } else { &sheet.rows[..] };
        let col_list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        for row in data_rows {
            if row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            let vals: Vec<String> = (0..cols.len())
                .map(|i| {
                    let v = row.get(i).map(String::as_str).unwrap_or("").trim();
                    if v.is_empty() {
                        "NULL".into()
                    } else if v.parse::<f64>().is_ok() && !v.starts_with('0') {
                        v.to_string()
                    } else {
                        format!("'{}'", v.replace('\'', "''"))
                    }
                })
                .collect();
            out.push_str(&format!("INSERT INTO \"{table}\" ({col_list}) VALUES ({});\n", vals.join(", ")));
        }
        out.push('\n');
    }
    out
}

fn write_md_tables(sheets: &[TableSheet]) -> String {
    let mut out = String::new();
    for (idx, sheet) in sheets.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if sheets.len() > 1 {
            out.push_str(&format!("## {}\n\n", sheet.name));
        }
        out.push_str(&rows_to_md_table(&sheet.rows));
    }
    out
}

pub(super) fn rows_to_md_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let mut out = String::new();
    let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    let header = &rows[0];
    out.push('|');
    for c in 0..cols {
        out.push_str(&format!(" {} |", esc(header.get(c).map(String::as_str).unwrap_or(""))));
    }
    out.push_str("\n|");
    for _ in 0..cols {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in rows.iter().skip(1) {
        out.push('|');
        for c in 0..cols {
            out.push_str(&format!(" {} |", esc(row.get(c).map(String::as_str).unwrap_or(""))));
        }
        out.push('\n');
    }
    out
}

/// Writes a valid `.xlsx` Office Open XML workbook from `sheets`.
fn write_xlsx(sheets: &[TableSheet]) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let active_sheets = if sheets.is_empty() {
        vec![TableSheet { name: "Sheet1".into(), rows: vec![] }]
    } else {
        sheets.to_vec()
    };

    // [Content_Types].xml
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
"#,
    );
    for i in 0..active_sheets.len() {
        ct.push_str(&format!(
            "  <Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\n",
            i + 1
        ));
    }
    ct.push_str("</Types>");
    zip.start_file("[Content_Types].xml", opts).map_err(save_err)?;
    zip.write_all(ct.as_bytes()).map_err(save_err)?;

    // _rels/.rels
    zip.start_file("_rels/.rels", opts).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
    )
    .map_err(save_err)?;

    // xl/workbook.xml & xl/_rels/workbook.xml.rels
    let mut wb_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
"#,
    );
    let mut wb_rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
"#,
    );
    for (i, s) in active_sheets.iter().enumerate() {
        let idx = i + 1;
        let safe_name: String = s.name.chars().filter(|&c| !matches!(c, '\\' | '/' | '?' | '*' | '[' | ']' | ':')).take(31).collect();
        let name = if safe_name.is_empty() { format!("Sheet{idx}") } else { safe_name };
        wb_xml.push_str(&format!(
            "    <sheet name=\"{}\" sheetId=\"{idx}\" r:id=\"rId{idx}\"/>\n",
            xml_escape(&name)
        ));
        wb_rels.push_str(&format!(
            "  <Relationship Id=\"rId{idx}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{idx}.xml\"/>\n"
        ));
    }
    wb_xml.push_str("  </sheets>\n</workbook>");
    wb_rels.push_str("</Relationships>");

    zip.start_file("xl/workbook.xml", opts).map_err(save_err)?;
    zip.write_all(wb_xml.as_bytes()).map_err(save_err)?;
    zip.start_file("xl/_rels/workbook.xml.rels", opts).map_err(save_err)?;
    zip.write_all(wb_rels.as_bytes()).map_err(save_err)?;

    // xl/styles.xml (style 0 = normal, style 1 = bold header)
    zip.start_file("xl/styles.xml", opts).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <fonts count="2">
    <font><sz val="11"/><name val="Calibri"/></font>
    <font><b/><sz val="11"/><name val="Calibri"/></font>
  </fonts>
  <fills count="2">
    <fill><patternFill patternType="none"/></fill>
    <fill><patternFill patternType="gray125"/></fill>
  </fills>
  <borders count="1">
    <border><left/><right/><top/><bottom/><diagonal/></border>
  </borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="2">
    <xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>
    <xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>
  </cellXfs>
</styleSheet>"#,
    )
    .map_err(save_err)?;

    // xl/worksheets/sheetN.xml
    for (i, sheet) in active_sheets.iter().enumerate() {
        let mut ws = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
"#,
        );
        for (r_idx, row) in sheet.rows.iter().enumerate() {
            let row_num = r_idx + 1;
            ws.push_str(&format!("    <row r=\"{row_num}\">\n"));
            for (c_idx, val) in row.iter().enumerate() {
                let col_ref = xlsx_col_name(c_idx as u32);
                let cell_ref = format!("{col_ref}{row_num}");
                let style_attr = if r_idx == 0 && sheet.rows.len() > 1 { " s=\"1\"" } else { "" };
                let trimmed = val.trim();
                let is_num = !trimmed.is_empty()
                    && !(trimmed.starts_with('0') && trimmed.len() > 1 && !trimmed.starts_with("0."))
                    && trimmed.parse::<f64>().is_ok();
                if is_num && r_idx > 0 {
                    ws.push_str(&format!("      <c r=\"{cell_ref}\"{style_attr}><v>{trimmed}</v></c>\n"));
                } else {
                    ws.push_str(&format!(
                        "      <c r=\"{cell_ref}\" t=\"inlineStr\"{style_attr}><is><t xml:space=\"preserve\">{}</t></is></c>\n",
                        xml_escape(val)
                    ));
                }
            }
            ws.push_str("    </row>\n");
        }
        ws.push_str("  </sheetData>\n</worksheet>");
        zip.start_file(format!("xl/worksheets/sheet{}.xml", i + 1), opts)
            .map_err(save_err)?;
        zip.write_all(ws.as_bytes()).map_err(save_err)?;
    }

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

fn xlsx_col_name(mut col: u32) -> String {
    let mut name = Vec::new();
    loop {
        name.push((b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    name.into_iter().rev().collect()
}

pub(super) fn sheets_to_workbook(sheets: &[TableSheet]) -> Workbook {
    let normal = Style { wrap: true, ..Default::default() };
    let mut bold = normal.clone();
    bold.font.bold = true;

    let mut wb_sheets = Vec::new();
    for ts in sheets {
        let mut sheet = Sheet {
            name: ts.name.clone(),
            default_col_width: 72.0,
            default_row_height: 16.0,
            auto_rows: true,
            ..Default::default()
        };
        sheet.print.gridlines = true;
        for (r, row) in ts.rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                let value = if text.trim().is_empty() {
                    None
                } else if r > 0 && !(text.starts_with('0') && text.len() > 1 && !text.starts_with("0.")) {
                    text.trim()
                        .parse::<f64>()
                        .ok()
                        .map(Value::Number)
                        .or_else(|| Some(Value::Text(text.clone())))
                } else {
                    Some(Value::Text(text.clone()))
                };
                sheet.cells.insert(
                    (r as u32, c as u32),
                    Cell {
                        value,
                        style: usize::from(r == 0 && ts.rows.len() > 1),
                    },
                );
            }
        }
        wb_sheets.push(sheet);
    }

    Workbook {
        sheets: wb_sheets,
        styles: vec![normal, bold],
        date1904: false,
    }
}
