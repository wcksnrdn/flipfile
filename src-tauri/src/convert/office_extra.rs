// Extended Office, Document, E-book, Presentation & Developer Data conversions:
// - Document formats: PDF, DOCX, ODT, RTF, MD, TXT, HTML, EPUB, PPTX/ODP
// - Spreadsheet & Data formats: XLSX, CSV, TSV, JSON, YAML, XML, SQL, MD-Table

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::encode::{encode, resize};
use super::input::file_name;
use super::output::{save_err, unique_output_path};
use super::pdf::pdf_to_images;
use super::{Converted, OnPage};
use crate::office::numfmt::{self, Separators, Value};
use crate::office::xlsx::{self, Cell, Sheet, Style, Workbook};
use crate::office::{docx, typeset};
use crate::pdf::render as pdf_render;
use crate::state::settings::Options;

/// Structured document block for cross-converting documents (MD, HTML, TXT, RTF, ODT, EPUB, DOCX, PDF, PPTX).
#[derive(Clone, Debug)]
pub enum DocBlock {
    Heading(u8, String),
    Paragraph(String),
    ListItem(String),
    CodeBlock(String),
    Table(Vec<Vec<String>>),
    PageBreak,
}

/// One tabular sheet of rows & columns for cross-converting spreadsheets and data formats.
#[derive(Clone, Debug)]
pub struct TableSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// True if `format` is an Office/Document/Data output target handled here.
pub(super) fn is_office_target(format: &str) -> bool {
    matches!(
        format,
        "xlsx" | "csv" | "tsv" | "json" | "yaml" | "xml" | "sql" | "md-table" | "md" | "txt" | "html" | "rtf" | "epub" | "odt"
    )
}

/// True if `path` is an extended office/document/data input file (MD, TXT, HTML, RTF, ODT, EPUB, PPTX, JSON, TSV, YAML, XML).
pub(super) fn is_extra_office_input(path: &Path) -> bool {
    let ext = ext_lower(path);
    matches!(
        ext.as_str(),
        "md" | "markdown"
            | "txt"
            | "html"
            | "htm"
            | "rtf"
            | "odt"
            | "epub"
            | "pptx"
            | "ppt"
            | "odp"
            | "tsv"
            | "tab"
            | "json"
            | "yaml"
            | "yml"
            | "xml"
            | "typ"
            | "tex"
    )
}

fn ext_lower(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default()
}

fn is_tabular_ext(ext: &str) -> bool {
    matches!(
        ext,
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xls" | "ods" | "csv" | "tsv" | "tab" | "json" | "yaml" | "yml" | "xml"
    )
}

fn is_tabular_target(format: &str) -> bool {
    matches!(format, "xlsx" | "csv" | "tsv" | "json" | "yaml" | "xml" | "sql" | "md-table")
}

/// Main entry point for converting any Office/Document/Data input or target.
pub(super) fn convert_office(
    input: &Path,
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let in_ext = ext_lower(input);
    let name = file_name(input);

    if in_ext == format || (in_ext == "markdown" && format == "md") || (in_ext == "htm" && format == "html") || (in_ext == "yml" && format == "yaml") {
        return Err(format!("\"{name}\" is already in {} format", format.to_uppercase()));
    }

    // 1. Any tabular/data target (xlsx, csv, tsv, json, yaml, xml, sql, md-table)
    if is_tabular_target(format) {
        let sheets = read_any_tables(input)?;
        let out_ext = if format == "md-table" { "md" } else { format };
        let output = unique_output_path(input, out_dir, "converted", out_ext);
        let bytes = write_tabular(&sheets, format, input)?;
        fs::write(&output, bytes).map_err(save_err)?;
        return Ok(Converted::file(input, &output));
    }

    // 2. Tabular input (JSON, TSV, YAML, XML) -> PDF or Image
    if is_tabular_ext(&in_ext) && (format == "pdf" || is_image_target(format)) {
        let sheets = read_any_tables(input)?;
        let wb = sheets_to_workbook(&sheets);
        let pdf_bytes = crate::office::sheet_pdf::to_pdf(&wb, Separators::system(), name)?;
        if format == "pdf" {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            fs::write(&output, pdf_bytes).map_err(save_err)?;
            return Ok(Converted {
                path: output.to_string_lossy().into_owned(),
                pages: 0,
                sizes: None,
                note: Some("built-in engine".into()),
            });
        }
        return render_pdf_bytes_to_images(&pdf_bytes, input, format, out_dir, options, on_page);
    }

    // 3. Document / Presentation / Tabular -> Document target (md, txt, html, rtf, epub, odt, docx, pdf, or image)
    let blocks = read_any_blocks(input)?;
    let title = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Document");

    match format {
        "md" => {
            let output = unique_output_path(input, out_dir, "converted", "md");
            fs::write(&output, blocks_to_markdown(&blocks)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "txt" => {
            let output = unique_output_path(input, out_dir, "converted", "txt");
            fs::write(&output, blocks_to_text(&blocks)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "html" => {
            let output = unique_output_path(input, out_dir, "converted", "html");
            fs::write(&output, blocks_to_html(&blocks, title)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "rtf" => {
            let output = unique_output_path(input, out_dir, "converted", "rtf");
            fs::write(&output, blocks_to_rtf(&blocks)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "epub" => {
            let output = unique_output_path(input, out_dir, "converted", "epub");
            let bytes = blocks_to_epub(&blocks, title)?;
            fs::write(&output, bytes).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "odt" => {
            let output = unique_output_path(input, out_dir, "converted", "odt");
            let bytes = blocks_to_odt(&blocks, title)?;
            fs::write(&output, bytes).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "docx" => {
            let output = unique_output_path(input, out_dir, "converted", "docx");
            let bytes = blocks_to_docx(&blocks)?;
            fs::write(&output, bytes).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "pdf" => {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            let pdf_bytes = blocks_to_pdf(&blocks)?;
            fs::write(&output, pdf_bytes).map_err(save_err)?;
            Ok(Converted {
                path: output.to_string_lossy().into_owned(),
                pages: 0,
                sizes: None,
                note: Some("built-in engine".into()),
            })
        }
        ext if is_image_target(ext) => {
            let pdf_bytes = blocks_to_pdf(&blocks)?;
            render_pdf_bytes_to_images(&pdf_bytes, input, ext, out_dir, options, on_page)
        }
        _ => Err(format!("\"{name}\": cannot convert to {format}")),
    }
}

fn is_image_target(format: &str) -> bool {
    matches!(
        format,
        "jpg" | "jpeg" | "png" | "webp" | "heic" | "bmp" | "tiff" | "gif" | "ico" | "icns" | "avif" | "tga" | "qoi" | "exr" | "hdr" | "ppm" | "svg"
    )
}

fn render_pdf_bytes_to_images(
    pdf_bytes: &[u8],
    input: &Path,
    ext: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let temp = std::env::temp_dir().join(format!("flipfile-doc-{}-{nanos}.pdf", std::process::id()));
    fs::write(&temp, pdf_bytes).map_err(save_err)?;
    let res = pdf_to_images(&temp, input, ext, out_dir, options, on_page);
    let _ = fs::remove_file(&temp);
    res
}

// ---------------------------------------------------------------------------
// Reading Tables (XLSX, CSV, TSV, JSON, YAML, XML, DOCX tables, PDF tables)

fn read_any_tables(input: &Path) -> Result<Vec<TableSheet>, String> {
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
                if let DocBlock::Table(rows) = block {
                    if !rows.is_empty() {
                        let sheet_name = if sheets.is_empty() {
                            default_sheet.clone()
                        } else {
                            format!("Table{}", sheets.len() + 1)
                        };
                        sheets.push(TableSheet { name: sheet_name, rows: rows.clone() });
                    }
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

// ---------------------------------------------------------------------------
// Writing Tabular Formats (XLSX, CSV, TSV, JSON, YAML, XML, SQL, MD-Table)

fn write_tabular(sheets: &[TableSheet], format: &str, input: &Path) -> Result<Vec<u8>, String> {
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
        if let Ok(f) = trimmed.parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return serde_json::Value::Number(n);
            }
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

fn rows_to_md_table(rows: &[Vec<String>]) -> String {
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

fn sheets_to_workbook(sheets: &[TableSheet]) -> Workbook {
    let mut normal = Style::default();
    normal.wrap = true;
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

// ---------------------------------------------------------------------------
// Reading Documents (PDF, DOCX, MD, TXT, HTML, RTF, ODT, EPUB, PPTX/ODP, Tables)

fn read_any_blocks(input: &Path) -> Result<Vec<DocBlock>, String> {
    let name = file_name(input);
    let ext = ext_lower(input);
    let bytes = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;

    if pdf_render::is_pdf(&bytes) || ext == "pdf" {
        return read_pdf_blocks(&bytes, name);
    }

    match ext.as_str() {
        "docx" | "docm" | "dotx" | "dotm" => read_docx_blocks(&bytes),
        "md" | "markdown" => Ok(parse_markdown_blocks(&String::from_utf8_lossy(&bytes))),
        "txt" | "typ" | "tex" => Ok(parse_text_blocks(&String::from_utf8_lossy(&bytes))),
        "html" | "htm" => Ok(parse_html_blocks(&String::from_utf8_lossy(&bytes))),
        "rtf" => Ok(parse_rtf_blocks(&bytes)),
        "odt" => read_odt_blocks(&bytes),
        "epub" => read_epub_blocks(&bytes),
        "pptx" | "odp" => read_presentation_blocks(&bytes, &ext),
        "xlsx" | "xlsm" | "xltx" | "xltm" | "csv" | "tsv" | "tab" | "json" | "yaml" | "yml" | "xml" => {
            let sheets = read_any_tables(input)?;
            let mut blocks = Vec::new();
            for s in sheets {
                if !s.name.is_empty() {
                    blocks.push(DocBlock::Heading(2, s.name));
                }
                if !s.rows.is_empty() {
                    blocks.push(DocBlock::Table(s.rows));
                }
            }
            Ok(blocks)
        }
        _ => Err(format!("\"{name}\": unsupported document format")),
    }
}

fn read_pdf_blocks(bytes: &[u8], name: &str) -> Result<Vec<DocBlock>, String> {
    let pdfium = pdf_render::pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|_| format!("\"{name}\": the PDF could not be opened"))?;
    let mut blocks = Vec::new();
    for (idx, page) in doc.pages().iter().enumerate() {
        if idx > 0 {
            blocks.push(DocBlock::PageBreak);
        }
        let text = page.text().map(|t| t.all()).unwrap_or_default();
        for para in text.split("\n\n") {
            let clean: String = para
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !clean.is_empty() {
                blocks.push(DocBlock::Paragraph(clean));
            }
        }
    }
    if blocks.is_empty() {
        return Err(format!("\"{name}\" has no selectable text (use WORD OCR for scanned PDFs)"));
    }
    Ok(blocks)
}

fn read_docx_blocks(bytes: &[u8]) -> Result<Vec<DocBlock>, String> {
    let doc = docx::read(bytes)?;
    let mut out = Vec::new();
    for section in &doc.sections {
        for block in &section.blocks {
            collect_docx_block(block, &mut out);
        }
    }
    Ok(out)
}

fn collect_docx_block(block: &docx::Block, out: &mut Vec<DocBlock>) {
    match block {
        docx::Block::Paragraph(p) => {
            let mut text = String::new();
            for inline in &p.inlines {
                match inline {
                    docx::Inline::Text(t, _) => text.push_str(t),
                    docx::Inline::Tab | docx::Inline::AlignTab => text.push('\t'),
                    docx::Inline::LineBreak => text.push('\n'),
                    docx::Inline::PageBreak => out.push(DocBlock::PageBreak),
                    docx::Inline::TextBox(inner) => {
                        for b in inner {
                            collect_docx_block(b, out);
                        }
                    }
                    _ => {}
                }
            }
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                return;
            }
            if p.marker.is_some() {
                out.push(DocBlock::ListItem(trimmed));
                return;
            }
            if let Some(ref style) = p.props.style_id {
                let s = style.to_lowercase();
                if s.starts_with("heading") || s == "title" {
                    let lvl = s.chars().last().and_then(|c| c.to_digit(10)).unwrap_or(1).clamp(1, 6) as u8;
                    out.push(DocBlock::Heading(lvl, trimmed));
                    return;
                }
            }
            let max_size = p.inlines.iter().fold(0.0f32, |acc, i| match i {
                docx::Inline::Text(_, r) => acc.max(r.size),
                _ => acc,
            });
            if max_size >= 18.0 {
                out.push(DocBlock::Heading(1, trimmed));
            } else if max_size >= 14.0 {
                out.push(DocBlock::Heading(2, trimmed));
            } else {
                out.push(DocBlock::Paragraph(trimmed));
            }
        }
        docx::Block::Table(t) => {
            let mut rows = Vec::new();
            for row in &t.rows {
                let mut r_cells = Vec::new();
                for cell in &row.cells {
                    let mut sub = Vec::new();
                    for b in &cell.blocks {
                        collect_docx_block(b, &mut sub);
                    }
                    let joined = sub
                        .into_iter()
                        .filter_map(|b| match b {
                            DocBlock::Heading(_, s) | DocBlock::Paragraph(s) | DocBlock::ListItem(s) => Some(s),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    r_cells.push(joined);
                }
                if !r_cells.is_empty() {
                    rows.push(r_cells);
                }
            }
            if !rows.is_empty() {
                out.push(DocBlock::Table(rows));
            }
        }
    }
}

fn parse_markdown_blocks(md: &str) -> Vec<DocBlock> {
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut code_buf = String::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut para_buf = String::new();

    let flush_para = |para_buf: &mut String, blocks: &mut Vec<DocBlock>| {
        let t = para_buf.trim();
        if !t.is_empty() {
            blocks.push(DocBlock::Paragraph(clean_md_inline(t)));
        }
        para_buf.clear();
    };
    let flush_table = |table_rows: &mut Vec<Vec<String>>, blocks: &mut Vec<DocBlock>| {
        if !table_rows.is_empty() {
            blocks.push(DocBlock::Table(std::mem::take(table_rows)));
        }
    };

    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            flush_para(&mut para_buf, &mut blocks);
            flush_table(&mut table_rows, &mut blocks);
            if in_code {
                blocks.push(DocBlock::CodeBlock(std::mem::take(&mut code_buf)));
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(raw);
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2 {
            flush_para(&mut para_buf, &mut blocks);
            let cells: Vec<String> = trimmed[1..trimmed.len() - 1]
                .split('|')
                .map(|c| clean_md_inline(c.trim()))
                .collect();
            let is_sep = cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
            if !is_sep {
                table_rows.push(cells);
            }
            continue;
        } else {
            flush_table(&mut table_rows, &mut blocks);
        }

        if trimmed.is_empty() {
            flush_para(&mut para_buf, &mut blocks);
            continue;
        }

        if let Some(hashes) = trimmed.split_whitespace().next() {
            if hashes.chars().all(|c| c == '#') && hashes.len() <= 6 {
                flush_para(&mut para_buf, &mut blocks);
                let rest = trimmed[hashes.len()..].trim();
                blocks.push(DocBlock::Heading(hashes.len() as u8, clean_md_inline(rest)));
                continue;
            }
        }

        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
            flush_para(&mut para_buf, &mut blocks);
            blocks.push(DocBlock::ListItem(clean_md_inline(item.trim())));
            continue;
        }

        if !para_buf.is_empty() {
            para_buf.push(' ');
        }
        para_buf.push_str(trimmed);
    }
    flush_para(&mut para_buf, &mut blocks);
    flush_table(&mut table_rows, &mut blocks);
    if !code_buf.is_empty() {
        blocks.push(DocBlock::CodeBlock(code_buf));
    }
    blocks
}

fn clean_md_inline(s: &str) -> String {
    s.replace("**", "").replace("__", "").replace('`', "")
}

fn parse_text_blocks(txt: &str) -> Vec<DocBlock> {
    let mut blocks = Vec::new();
    let mut buf = String::new();
    for line in txt.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !buf.is_empty() {
                blocks.push(DocBlock::Paragraph(std::mem::take(&mut buf)));
            }
        } else {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(t);
        }
    }
    if !buf.is_empty() {
        blocks.push(DocBlock::Paragraph(buf));
    }
    blocks
}

fn parse_html_blocks(html: &str) -> Vec<DocBlock> {
    // Lightweight tag-aware parser that works on both strict XHTML and loose HTML5
    let mut blocks = Vec::new();
    if let Ok(doc) = roxmltree::Document::parse(html) {
        collect_xml_html_nodes(doc.root(), &mut blocks);
        if !blocks.is_empty() {
            return blocks;
        }
    }
    // Loose HTML fallback: split by block tags and strip inline tags
    let normalized = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("</p>", "\n\n")
        .replace("</div>", "\n\n")
        .replace("</h1>", "\n\n")
        .replace("</h2>", "\n\n")
        .replace("</h3>", "\n\n")
        .replace("</li>", "\n");
    let mut plain = String::with_capacity(normalized.len());
    let mut in_tag = false;
    for ch in normalized.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    parse_text_blocks(&html_unescape(&plain))
}

fn collect_xml_html_nodes(node: roxmltree::Node, out: &mut Vec<DocBlock>) {
    for child in node.children().filter(|n| n.is_element()) {
        let tag = child.tag_name().name().to_ascii_lowercase();
        match tag.as_str() {
            "script" | "style" | "head" => {}
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let lvl = tag[1..].parse::<u8>().unwrap_or(1);
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::Heading(lvl, text));
                }
            }
            "p" => {
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::Paragraph(text));
                }
            }
            "li" => {
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::ListItem(text));
                }
            }
            "pre" => {
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::CodeBlock(text));
                }
            }
            "table" => {
                let mut rows = Vec::new();
                for tr in child.descendants().filter(|n| n.tag_name().name().eq_ignore_ascii_case("tr")) {
                    let cells: Vec<String> = tr
                        .children()
                        .filter(|n| matches!(n.tag_name().name().to_ascii_lowercase().as_str(), "td" | "th"))
                        .map(collect_node_text)
                        .collect();
                    if !cells.is_empty() {
                        rows.push(cells);
                    }
                }
                if !rows.is_empty() {
                    out.push(DocBlock::Table(rows));
                }
            }
            _ => collect_xml_html_nodes(child, out),
        }
    }
}

fn collect_node_text(node: roxmltree::Node) -> String {
    let mut s = String::new();
    for desc in node.descendants() {
        if desc.is_text() {
            if let Some(t) = desc.text() {
                s.push_str(t);
            }
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn parse_rtf_blocks(bytes: &[u8]) -> Vec<DocBlock> {
    let raw = String::from_utf8_lossy(bytes);
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    let mut depth = 0i32;
    let mut skip_depth: Option<i32> = None;

    while let Some(c) = chars.next() {
        match c {
            '{' => depth += 1,
            '}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth -= 1;
            }
            '\\' => {
                let mut word = String::new();
                if let Some(&next) = chars.peek() {
                    if matches!(next, '\\' | '{' | '}') {
                        if skip_depth.is_none() {
                            text.push(chars.next().unwrap());
                        }
                        continue;
                    }
                    if next == '*' {
                        skip_depth = Some(depth);
                        chars.next();
                        continue;
                    }
                }
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_alphabetic() {
                        word.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '-' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                if matches!(word.as_str(), "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict") && skip_depth.is_none() {
                    skip_depth = Some(depth);
                } else if skip_depth.is_none() {
                    match word.as_str() {
                        "par" | "line" => text.push_str("\n\n"),
                        "tab" => text.push('\t'),
                        _ => {}
                    }
                }
            }
            '\r' | '\n' => {}
            other if skip_depth.is_none() => text.push(other),
            _ => {}
        }
    }
    parse_text_blocks(&text)
}

fn read_odt_blocks(bytes: &[u8]) -> Result<Vec<DocBlock>, String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Could not open .odt archive")?;
    let mut xml = String::new();
    zip.by_name("content.xml")
        .map_err(|_| "Missing content.xml in .odt")?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
    let mut blocks = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "h" => {
                let text = collect_node_text(node);
                if !text.is_empty() {
                    blocks.push(DocBlock::Heading(2, text));
                }
            }
            "p" => {
                let parent_tag = node.parent().map(|p| p.tag_name().name()).unwrap_or("");
                if parent_tag != "table-cell" {
                    let text = collect_node_text(node);
                    if !text.is_empty() {
                        blocks.push(DocBlock::Paragraph(text));
                    }
                }
            }
            "table" => {
                let mut rows = Vec::new();
                for tr in node.children().filter(|n| n.tag_name().name() == "table-row") {
                    let cells: Vec<String> = tr
                        .children()
                        .filter(|n| n.tag_name().name() == "table-cell")
                        .map(collect_node_text)
                        .collect();
                    if !cells.is_empty() {
                        rows.push(cells);
                    }
                }
                if !rows.is_empty() {
                    blocks.push(DocBlock::Table(rows));
                }
            }
            _ => {}
        }
    }
    Ok(blocks)
}

fn read_epub_blocks(bytes: &[u8]) -> Result<Vec<DocBlock>, String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Could not open .epub archive")?;
    let mut html_files: Vec<String> = zip
        .file_names()
        .filter_map(|r| r.ok().map(String::from))
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            (l.ends_with(".xhtml") || l.ends_with(".html") || l.ends_with(".htm")) && !l.contains("nav") && !l.contains("toc")
        })
        .collect();
    html_files.sort();
    let mut blocks = Vec::new();
    for name in html_files {
        if let Ok(mut f) = zip.by_name(&name) {
            let mut content = String::new();
            if f.read_to_string(&mut content).is_ok() {
                blocks.extend(parse_html_blocks(&content));
            }
        }
    }
    if blocks.is_empty() {
        return Err("No readable chapters found in EPUB".into());
    }
    Ok(blocks)
}

fn read_presentation_blocks(bytes: &[u8], ext: &str) -> Result<Vec<DocBlock>, String> {
    if ext == "odp" {
        return read_odt_blocks(bytes);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Could not open .pptx archive")?;
    let mut slides: Vec<(usize, String)> = zip
        .file_names()
        .filter_map(|r| r.ok().map(String::from))
        .filter_map(|name| {
            let num = name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse::<usize>().ok()?;
            Some((num, name))
        })
        .collect();
    slides.sort_by_key(|(n, _)| *n);

    let mut blocks = Vec::new();
    for (num, path) in slides {
        if num > 1 {
            blocks.push(DocBlock::PageBreak);
        }
        let mut xml = String::new();
        if let Ok(mut f) = zip.by_name(&path) {
            let _ = f.read_to_string(&mut xml);
        }
        if let Ok(doc) = roxmltree::Document::parse(&xml) {
            let mut first = true;
            for p in doc.descendants().filter(|n| n.tag_name().name() == "p") {
                let text: String = p
                    .descendants()
                    .filter(|n| n.tag_name().name() == "t")
                    .filter_map(|n| n.text())
                    .collect::<Vec<_>>()
                    .join("");
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    if first {
                        blocks.push(DocBlock::Heading(2, trimmed.to_string()));
                        first = false;
                    } else {
                        blocks.push(DocBlock::ListItem(trimmed.to_string()));
                    }
                }
            }
        }
    }
    if blocks.is_empty() {
        return Err("No slide text found in presentation".into());
    }
    Ok(blocks)
}

// ---------------------------------------------------------------------------
// Writing Documents (MD, TXT, HTML, RTF, EPUB, ODT, DOCX, PDF)

fn blocks_to_markdown(blocks: &[DocBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let hashes = "#".repeat((*lvl as usize).clamp(1, 6));
                out.push_str(&format!("{hashes} {t}\n\n"));
            }
            DocBlock::Paragraph(t) => out.push_str(&format!("{t}\n\n")),
            DocBlock::ListItem(t) => out.push_str(&format!("- {t}\n")),
            DocBlock::CodeBlock(c) => out.push_str(&format!("```\n{c}\n```\n\n")),
            DocBlock::Table(rows) => {
                out.push_str(&rows_to_md_table(rows));
                out.push('\n');
            }
            DocBlock::PageBreak => out.push_str("\n---\n\n"),
        }
    }
    out
}

fn blocks_to_text(blocks: &[DocBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(_, t) => out.push_str(&format!("{}\n{}\n\n", t, "=".repeat(t.chars().count().min(60)))),
            DocBlock::Paragraph(t) => out.push_str(&format!("{t}\n\n")),
            DocBlock::ListItem(t) => out.push_str(&format!("  * {t}\n")),
            DocBlock::CodeBlock(c) => out.push_str(&format!("{c}\n\n")),
            DocBlock::Table(rows) => {
                for r in rows {
                    out.push_str(&r.join("\t"));
                    out.push('\n');
                }
                out.push('\n');
            }
            DocBlock::PageBreak => out.push_str("\n----------------------------------------\n\n"),
        }
    }
    out
}

fn blocks_to_html(blocks: &[DocBlock], title: &str) -> String {
    let mut body = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let l = (*lvl).clamp(1, 6);
                body.push_str(&format!("  <h{l}>{}</h{l}>\n", xml_escape(t)));
            }
            DocBlock::Paragraph(t) => body.push_str(&format!("  <p>{}</p>\n", xml_escape(t))),
            DocBlock::ListItem(t) => body.push_str(&format!("  <ul><li>{}</li></ul>\n", xml_escape(t))),
            DocBlock::CodeBlock(c) => body.push_str(&format!("  <pre><code>{}</code></pre>\n", xml_escape(c))),
            DocBlock::Table(rows) => {
                body.push_str("  <table>\n");
                for (r_idx, r) in rows.iter().enumerate() {
                    body.push_str("    <tr>");
                    let tag = if r_idx == 0 && rows.len() > 1 { "th" } else { "td" };
                    for cell in r {
                        body.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(cell)));
                    }
                    body.push_str("</tr>\n");
                }
                body.push_str("  </table>\n");
            }
            DocBlock::PageBreak => body.push_str("  <hr/>\n"),
        }
    }
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
  <title>{}</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 780px; margin: 2.5rem auto; padding: 0 1.25rem; line-height: 1.6; color: #1d1d1f; }}
    h1, h2, h3 {{ margin-top: 1.6em; }}
    table {{ border-collapse: collapse; width: 100%; margin: 1.2rem 0; }}
    th, td {{ border: 1px solid #d2d2d7; padding: 0.5rem 0.75rem; text-align: left; }}
    th {{ background: #f5f5f7; font-weight: 600; }}
    pre {{ background: #f5f5f7; padding: 1rem; border-radius: 6px; overflow-x: auto; }}
  </style>
</head>
<body>
{body}</body>
</html>
"#,
        xml_escape(title)
    )
}

fn blocks_to_rtf(blocks: &[DocBlock]) -> String {
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Calibri;}}\\fs22\n");
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let sz = match lvl {
                    1 => 36,
                    2 => 30,
                    _ => 26,
                };
                out.push_str(&format!("\\pard\\sa160\\b\\fs{sz} {}\\b0\\fs22\\par\n", rtf_escape(t)));
            }
            DocBlock::Paragraph(t) => out.push_str(&format!("\\pard\\sa140 {}\\par\n", rtf_escape(t))),
            DocBlock::ListItem(t) => out.push_str(&format!("\\pard\\li360\\sa80 \\u8226? {}\\par\n", rtf_escape(t))),
            DocBlock::CodeBlock(c) => out.push_str(&format!("\\pard\\sa140 {}\\par\n", rtf_escape(c))),
            DocBlock::Table(rows) => {
                for r in rows {
                    let line = r.iter().map(|c| rtf_escape(c)).collect::<Vec<_>>().join("\\tab ");
                    out.push_str(&format!("\\pard\\sa80 {line}\\par\n"));
                }
            }
            DocBlock::PageBreak => out.push_str("\\page\n"),
        }
    }
    out.push('}');
    out
}

fn rtf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\line "),
            c if c.is_ascii() => out.push(c),
            c => out.push_str(&format!("\\u{}?", c as i32)),
        }
    }
    out
}

fn blocks_to_epub(blocks: &[DocBlock], title: &str) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // EPUB requires `mimetype` as the first, uncompressed entry
    zip.start_file("mimetype", stored).map_err(save_err)?;
    zip.write_all(b"application/epub+zip").map_err(save_err)?;

    zip.start_file("META-INF/container.xml", deflated).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )
    .map_err(save_err)?;

    let opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:flipfile-epub-001</dc:identifier>
    <dc:title>{}</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-11-03T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>"#,
        xml_escape(title)
    );
    zip.start_file("OEBPS/content.opf", deflated).map_err(save_err)?;
    zip.write_all(opf.as_bytes()).map_err(save_err)?;

    let nav = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>{t}</title></head>
<body>
  <nav epub:type="toc"><ol><li><a href="chapter1.xhtml">{t}</a></li></ol></nav>
</body>
</html>"#,
        t = xml_escape(title)
    );
    zip.start_file("OEBPS/nav.xhtml", deflated).map_err(save_err)?;
    zip.write_all(nav.as_bytes()).map_err(save_err)?;

    let mut chapter = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>{}</title></head>
<body>
"#,
        xml_escape(title)
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let l = (*lvl).clamp(1, 6);
                chapter.push_str(&format!("  <h{l}>{}</h{l}>\n", xml_escape(t)));
            }
            DocBlock::Paragraph(t) => chapter.push_str(&format!("  <p>{}</p>\n", xml_escape(t))),
            DocBlock::ListItem(t) => chapter.push_str(&format!("  <ul><li>{}</li></ul>\n", xml_escape(t))),
            DocBlock::CodeBlock(c) => chapter.push_str(&format!("  <pre>{}</pre>\n", xml_escape(c))),
            DocBlock::Table(rows) => {
                chapter.push_str("  <table>\n");
                for r in rows {
                    chapter.push_str("    <tr>");
                    for cell in r {
                        chapter.push_str(&format!("<td>{}</td>", xml_escape(cell)));
                    }
                    chapter.push_str("</tr>\n");
                }
                chapter.push_str("  </table>\n");
            }
            DocBlock::PageBreak => chapter.push_str("  <hr/>\n"),
        }
    }
    chapter.push_str("</body>\n</html>");
    zip.start_file("OEBPS/chapter1.xhtml", deflated).map_err(save_err)?;
    zip.write_all(chapter.as_bytes()).map_err(save_err)?;

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

fn blocks_to_odt(blocks: &[DocBlock], _title: &str) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).map_err(save_err)?;
    zip.write_all(b"application/vnd.oasis.opendocument.text").map_err(save_err)?;

    zip.start_file("META-INF/manifest.xml", deflated).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
  <manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.text"/>
  <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"#,
    )
    .map_err(save_err)?;

    let mut content = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" office:version="1.2">
  <office:body>
    <office:text>
"#,
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                content.push_str(&format!(
                    "      <text:h text:outline-level=\"{}\">{}</text:h>\n",
                    (*lvl).clamp(1, 6),
                    xml_escape(t)
                ));
            }
            DocBlock::Paragraph(t) | DocBlock::CodeBlock(t) => {
                content.push_str(&format!("      <text:p>{}</text:p>\n", xml_escape(t)));
            }
            DocBlock::ListItem(t) => {
                content.push_str(&format!("      <text:p>• {}</text:p>\n", xml_escape(t)));
            }
            DocBlock::Table(rows) => {
                content.push_str("      <table:table>\n");
                for r in rows {
                    content.push_str("        <table:table-row>\n");
                    for cell in r {
                        content.push_str(&format!(
                            "          <table:table-cell><text:p>{}</text:p></table:table-cell>\n",
                            xml_escape(cell)
                        ));
                    }
                    content.push_str("        </table:table-row>\n");
                }
                content.push_str("      </table:table>\n");
            }
            DocBlock::PageBreak => {}
        }
    }
    content.push_str("    </office:text>\n  </office:body>\n</office:document-content>");
    zip.start_file("content.xml", deflated).map_err(save_err)?;
    zip.write_all(content.as_bytes()).map_err(save_err)?;

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

fn blocks_to_docx(blocks: &[DocBlock]) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", opts).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    )
    .map_err(save_err)?;

    zip.start_file("_rels/.rels", opts).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    )
    .map_err(save_err)?;

    let mut doc_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
"#,
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let sz = match lvl {
                    1 => 36,
                    2 => 28,
                    _ => 24,
                };
                doc_xml.push_str(&format!(
                    "    <w:p><w:r><w:rPr><w:b/><w:sz w:val=\"{sz}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>\n",
                    xml_escape(t)
                ));
            }
            DocBlock::Paragraph(t) | DocBlock::CodeBlock(t) => {
                doc_xml.push_str(&format!(
                    "    <w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>\n",
                    xml_escape(t)
                ));
            }
            DocBlock::ListItem(t) => {
                doc_xml.push_str(&format!(
                    "    <w:p><w:r><w:t xml:space=\"preserve\">• {}</w:t></w:r></w:p>\n",
                    xml_escape(t)
                ));
            }
            DocBlock::Table(rows) => {
                doc_xml.push_str("    <w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/></w:tblBorders></w:tblPr>\n");
                for (r_idx, r) in rows.iter().enumerate() {
                    doc_xml.push_str("      <w:tr>");
                    for cell in r {
                        let bold = if r_idx == 0 && rows.len() > 1 { "<w:rPr><w:b/></w:rPr>" } else { "" };
                        doc_xml.push_str(&format!(
                            "<w:tc><w:p><w:r>{bold}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>",
                            xml_escape(cell)
                        ));
                    }
                    doc_xml.push_str("</w:tr>\n");
                }
                doc_xml.push_str("    </w:tbl>\n");
            }
            DocBlock::PageBreak => {
                doc_xml.push_str("    <w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n");
            }
        }
    }
    doc_xml.push_str("  </w:body>\n</w:document>");
    zip.start_file("word/document.xml", opts).map_err(save_err)?;
    zip.write_all(doc_xml.as_bytes()).map_err(save_err)?;

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

fn blocks_to_pdf(blocks: &[DocBlock]) -> Result<Vec<u8>, String> {
    let mut markup = String::from(
        r#"#set page(paper: "a4", margin: 2cm)
#set text(size: 11pt)
#set par(justify: false, leading: 0.65em)
"#,
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let eqs = "=".repeat((*lvl as usize).clamp(1, 5));
                markup.push_str(&format!("{eqs} #{:?}\n\n", t));
            }
            DocBlock::Paragraph(t) => {
                markup.push_str(&format!("#{:?}\n\n", t));
            }
            DocBlock::ListItem(t) => {
                markup.push_str(&format!("- #{:?}\n", t));
            }
            DocBlock::CodeBlock(c) => {
                markup.push_str(&format!("#raw(block: true, {:?})\n\n", c));
            }
            DocBlock::Table(rows) => {
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                markup.push_str(&format!("#table(columns: {cols}, stroke: 0.5pt + rgb(\"#cccccc\"), inset: 6pt,\n"));
                for (r_idx, r) in rows.iter().enumerate() {
                    for c in 0..cols {
                        let cell = r.get(c).map(String::as_str).unwrap_or("");
                        if r_idx == 0 && rows.len() > 1 {
                            markup.push_str(&format!("  strong({:?}),\n", cell));
                        } else {
                            markup.push_str(&format!("  {:?},\n", cell));
                        }
                    }
                }
                markup.push_str(")\n\n");
            }
            DocBlock::PageBreak => markup.push_str("#pagebreak(weak: true)\n"),
        }
    }
    typeset::to_pdf(markup, vec![]).map_err(|e| format!("the document could not be laid out ({e})"))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\r' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

#[allow(dead_code)]
fn _keep_imports() {
    let _ = encode;
    let _ = resize;
}
