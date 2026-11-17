//! Reading Tables (XLSX, CSV, TSV, JSON, YAML, XML, DOCX tables, PDF tables)

use super::*;

pub(in crate::convert) fn read_any_tables(input: &Path) -> Result<Vec<TableSheet>, String> {
    let name = file_name(input);
    let ext = ext_lower(input);
    let bytes = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
    let default_sheet = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Sheet1").to_string();

    match ext.as_str() {
        "xlsx" | "xlsm" | "xltx" | "xltm" => {
            let wb = xlsx::read(&bytes)?;
            Ok(workbook_to_sheets(&wb))
        }
        "csv" | "tsv" | "tab" => {
            let wb = xlsx::read_csv(&bytes, name)?;
            Ok(workbook_to_sheets(&wb))
        }
        "json" => parse_json_tables(&bytes, &default_sheet),
        "yaml" | "yml" => parse_yaml_tables(&bytes, &default_sheet),
        "xml" => parse_xml_tables(&bytes, &default_sheet),
        _ => {
            // Extract tables (or lines as rows) from any document (PDF, DOCX, MD, HTML, etc.)
            let blocks = read_any_blocks(input)?;
            let mut sheets = Vec::new();
            for block in &blocks {
                if let DocBlock::Table(rows) = block
                    && !rows.is_empty() {
                        let sheet_name = if sheets.is_empty() {
                            default_sheet.clone()
                        } else {
                            format!("Table{}", sheets.len() + 1)
                        };
                        sheets.push(TableSheet { name: sheet_name, rows: rows.clone() });
                    }
            }
            if sheets.is_empty() {
                let rows: Vec<Vec<String>> = blocks
                    .into_iter()
                    .filter_map(|b| match b {
                        DocBlock::Heading(_, t) | DocBlock::Paragraph(t) | DocBlock::ListItem(t) => Some(vec![t]),
                        _ => None,
                    })
                    .collect();
                if rows.is_empty() {
                    return Err(format!("\"{name}\" has no tabular data to convert"));
                }
                sheets.push(TableSheet { name: default_sheet, rows });
            }
            Ok(sheets)
        }
    }
}

fn workbook_to_sheets(wb: &Workbook) -> Vec<TableSheet> {
    let sep = Separators::system();
    let mut out = Vec::new();
    for sheet in &wb.sheets {
        let Some(&(max_r, _)) = sheet.cells.keys().next_back() else {
            continue;
        };
        let max_c = sheet.cells.keys().map(|&(_, c)| c).max().unwrap_or(0);
        let mut rows = Vec::with_capacity((max_r + 1) as usize);
        for r in 0..=max_r {
            let mut row = Vec::with_capacity((max_c + 1) as usize);
            for c in 0..=max_c {
                let text = match sheet.cells.get(&(r, c)) {
                    Some(cell) => match &cell.value {
                        Some(val) => {
                            let code = wb.styles.get(cell.style).map(|s| s.num_fmt.as_str()).unwrap_or("General");
                            let shown = numfmt::format(val, code, wb.date1904, sep);
                            clean_shown(&shown.text)
                        }
                        None => String::new(),
                    },
                    None => String::new(),
                };
                row.push(text);
            }
            // Trim trailing empty cells only if the whole row isn't empty
            rows.push(row);
        }
        out.push(TableSheet { name: sheet.name.clone(), rows });
    }
    out
}

fn clean_shown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut hidden = false;
    for ch in s.chars() {
        if ch == numfmt::HIDE_ON {
            hidden = true;
        } else if ch == numfmt::HIDE_OFF {
            hidden = false;
        } else if !hidden {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn parse_json_tables(bytes: &[u8], default_name: &str) -> Result<Vec<TableSheet>, String> {
    let val: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("Invalid JSON: {e}"))?;
    let mut sheets = Vec::new();
    match val {
        serde_json::Value::Array(items) => {
            sheets.push(json_array_to_sheet(default_name, &items));
        }
        serde_json::Value::Object(map) => {
            // Either an object of sheet arrays, or a single flat object
            let all_arrays = !map.is_empty() && map.values().all(|v| v.is_array());
            if all_arrays {
                for (k, v) in map {
                    if let serde_json::Value::Array(items) = v {
                        sheets.push(json_array_to_sheet(&k, &items));
                    }
                }
            } else {
                let mut rows = vec![vec!["Key".to_string(), "Value".to_string()]];
                for (k, v) in map {
                    rows.push(vec![k, json_scalar(&v)]);
                }
                sheets.push(TableSheet { name: default_name.to_string(), rows });
            }
        }
        other => {
            sheets.push(TableSheet {
                name: default_name.to_string(),
                rows: vec![vec![json_scalar(&other)]],
            });
        }
    }
    Ok(sheets)
}

fn json_array_to_sheet(name: &str, items: &[serde_json::Value]) -> TableSheet {
    if items.is_empty() {
        return TableSheet { name: name.to_string(), rows: vec![] };
    }
    if items.iter().all(|v| v.is_object()) {
        let mut keys = Vec::new();
        for item in items {
            if let Some(obj) = item.as_object() {
                for k in obj.keys() {
                    if !keys.contains(k) {
                        keys.push(k.clone());
                    }
                }
            }
        }
        let mut rows = Vec::with_capacity(items.len() + 1);
        rows.push(keys.clone());
        for item in items {
            let obj = item.as_object().unwrap();
            rows.push(keys.iter().map(|k| obj.get(k).map(json_scalar).unwrap_or_default()).collect());
        }
        TableSheet { name: name.to_string(), rows }
    } else if items.iter().all(|v| v.is_array()) {
        let rows = items
            .iter()
            .map(|v| v.as_array().unwrap().iter().map(json_scalar).collect())
            .collect();
        TableSheet { name: name.to_string(), rows }
    } else {
        let rows = items.iter().map(|v| vec![json_scalar(v)]).collect();
        TableSheet { name: name.to_string(), rows }
    }
}

fn json_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn parse_yaml_tables(bytes: &[u8], default_name: &str) -> Result<Vec<TableSheet>, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut records: Vec<BTreeMap<String, String>> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut current: BTreeMap<String, String> = BTreeMap::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line == "---" || line == "..." {
            continue;
        }
        let (is_new, rest) = if let Some(r) = line.strip_prefix("- ") {
            (true, r.trim())
        } else if line == "-" {
            (true, "")
        } else {
            (false, line)
        };
        if is_new && !current.is_empty() {
            records.push(std::mem::take(&mut current));
        }
        if let Some((k, v)) = rest.split_once(':') {
            let key = k.trim().trim_matches('"').trim_matches('\'').to_string();
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !key.is_empty() {
                if !keys.contains(&key) {
                    keys.push(key.clone());
                }
                current.insert(key, val);
            }
        }
    }
    if !current.is_empty() {
        records.push(current);
    }
    if records.is_empty() {
        return Err("No structured records found in YAML file".into());
    }
    let mut rows = Vec::with_capacity(records.len() + 1);
    rows.push(keys.clone());
    for rec in records {
        rows.push(keys.iter().map(|k| rec.get(k).cloned().unwrap_or_default()).collect());
    }
    Ok(vec![TableSheet { name: default_name.to_string(), rows }])
}

fn parse_xml_tables(bytes: &[u8], default_name: &str) -> Result<Vec<TableSheet>, String> {
    let text = String::from_utf8_lossy(bytes);
    if crate::office::xml_too_deep(&text) {
        return Err("Invalid XML: nested too deeply".into());
    }
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("Invalid XML: {e}"))?;
    let root = doc.root_element();

    // Check if root contains <sheet> / container elements whose children are the actual records
    let mut sheets = Vec::new();
    let root_children: Vec<_> = root.children().filter(|n| n.is_element()).collect();
    let has_nested_records = root_children.iter().any(|c| {
        c.children()
            .filter(|n| n.is_element())
            .any(|grand| grand.children().any(|gg| gg.is_element()))
    });

    if has_nested_records {
        for (idx, sheet_node) in root_children.iter().enumerate() {
            let name = sheet_node
                .attribute("name")
                .map(String::from)
                .unwrap_or_else(|| format!("Sheet{}", idx + 1));
            if let Some(sheet) = parse_xml_record_parent(*sheet_node, &name) {
                sheets.push(sheet);
            }
        }
    } else if let Some(sheet) = parse_xml_record_parent(root, default_name) {
        sheets.push(sheet);
    }

    if sheets.is_empty() {
        return Err("No table rows found in XML file".into());
    }
    Ok(sheets)
}

fn parse_xml_record_parent(parent: roxmltree::Node<'_, '_>, sheet_name: &str) -> Option<TableSheet> {
    let mut records: Vec<BTreeMap<String, String>> = Vec::new();
    let mut keys: Vec<String> = Vec::new();

    for child in parent.children().filter(|n| n.is_element()) {
        let mut rec = BTreeMap::new();
        for attr in child.attributes() {
            let k = attr.name().to_string();
            if !keys.contains(&k) {
                keys.push(k.clone());
            }
            rec.insert(k, attr.value().to_string());
        }
        for field in child.children().filter(|n| n.is_element()) {
            let k = field.tag_name().name().to_string();
            let v = field.text().unwrap_or("").trim().to_string();
            if !keys.contains(&k) {
                keys.push(k.clone());
            }
            rec.insert(k, v);
        }
        if !rec.is_empty() {
            records.push(rec);
        }
    }
    if records.is_empty() {
        return None;
    }
    let mut rows = vec![keys.clone()];
    for rec in records {
        rows.push(keys.iter().map(|k| rec.get(k).cloned().unwrap_or_default()).collect());
    }
    Some(TableSheet { name: sheet_name.to_string(), rows })
}
