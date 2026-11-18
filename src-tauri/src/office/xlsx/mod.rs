// Reads an Excel workbook (.xlsx) into a model ready for printing: every visible
// sheet's cells with their resolved styles, column widths and row heights in points,
// merged cells, pictures, and the print setup (paper, margins, scale, print area,
// repeated title rows, page breaks, header and footer).
//
// Formulas are not recalculated: Excel stores each formula's last result next to it,
// and that is what gets printed.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::sync::Arc;

use roxmltree::{Document, Node};

use super::numfmt::{self, Value};
use super::sheet_style::{Cfvo, CfKind, CfRule, CondFormat, Dxf, Mark, TableDef};

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DRAW: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";

/// Points per EMU (drawing offsets)
const EMU: f32 = 1.0 / 12700.0;

// The reader's parts. The submodules share one namespace (each starts with
// `use super::*`), as when this was a single file.
mod colors;
mod computed;
mod csv;
mod model;
mod package;
mod pictures;
mod sheet;
mod strings;
mod styles;

#[cfg(test)]
mod tests;

use colors::*;
use computed::*;
use package::*;
use pictures::*;
use sheet::*;
use strings::*;
use styles::*;

// The workbook model, and the helpers the rest of `office` shares
pub use colors::{indexed_color, tint};
pub use csv::read_csv;
pub use model::*;
pub use package::{cell_ref, range_ref};
pub use pictures::ChartPart;

pub fn read(bytes: &[u8]) -> Result<Workbook, String> {
    let zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|_| "not an Excel workbook (.xlsx)".to_string())?;
    let mut pkg = Package { zip };
    // the officeDocument relationship, usually xl/workbook.xml
    let wb_path = pkg
        .rels_list("")
        .into_iter()
        .find(|(_, kind, _)| kind.ends_with("/officeDocument"))
        .map(|(_, _, t)| t)
        .unwrap_or_else(|| "xl/workbook.xml".into());
    let wb_xml = pkg.text(&wb_path).ok_or("the workbook is damaged (no workbook.xml)")?;
    let wb_list = pkg.rels_list(&wb_path);
    let wb_rels: HashMap<String, String> = wb_list.iter().map(|(id, _, t)| (id.clone(), t.clone())).collect();
    let wb_doc = Document::parse(&wb_xml).map_err(|_| "the workbook is damaged".to_string())?;
    let wb = wb_doc.root_element();

    let date1904 = child(wb, "workbookPr").and_then(|p| attr_bool(p, "date1904")).unwrap_or(false);

    let rel_target = |kind: &str| wb_list.iter().find(|(_, k, _)| k.ends_with(kind)).map(|(_, _, t)| t.clone());
    let theme = rel_target("/theme").and_then(|p| pkg.text(&p)).map(|x| theme_colors(&x)).unwrap_or_else(default_theme);
    let indexed = rel_target("/styles").and_then(|p| pkg.text(&p)).and_then(|x| custom_palette(&x));
    let colors = Colors { theme: if theme.is_empty() { default_theme() } else { theme }, indexed };
    let styles_xml = rel_target("/styles").and_then(|p| pkg.text(&p));
    let mut styles = styles_xml.as_deref().map(|x| read_styles(x, &colors)).unwrap_or_else(|| vec![Style { num_fmt: "General".into(), ..Default::default() }]);
    let (dxfs, table_styles) = styles_xml.as_deref().map(|x| read_dxfs(x, &colors)).unwrap_or_default();
    let strings = rel_target("/sharedStrings").and_then(|p| pkg.text(&p)).map(|x| shared_strings(&x, &colors)).unwrap_or_default();
    let mdw = max_digit_width(&styles[0].font);

    // print areas and titles, per sheet index
    let mut names: Vec<(String, Option<usize>, String)> = vec![];
    if let Some(dn) = child(wb, "definedNames") {
        for n in children(dn, "definedName") {
            if let Some(name) = n.attribute("name") {
                names.push((name.to_string(), n.attribute("localSheetId").and_then(|v| v.parse().ok()), n.text().unwrap_or("").to_string()));
            }
        }
    }

    let mut sheets = vec![];
    // hidden sheets don't print, but formulas may read them
    let mut hidden = vec![];
    if let Some(list) = child(wb, "sheets") {
        for (index, s) in children(list, "sheet").enumerate() {
            let is_hidden = matches!(s.attribute("state"), Some("hidden") | Some("veryHidden"));
            let name = s.attribute("name").unwrap_or("Sheet").to_string();
            let Some(rid) = s.attribute((REL, "id")) else { continue };
            let Some(path) = wb_rels.get(rid).cloned() else { continue };
            // chart sheets and dialog sheets have no cells to print
            if !path.contains("worksheets") {
                continue;
            }
            let Some(xml) = pkg.text(&path) else { continue };
            let mut sheet = read_sheet(&xml, &strings, &styles, mdw, &colors).map_err(|e| format!("sheet \"{name}\": {e}"))?;
            sheet.name = name.clone();
            (sheet.pictures, sheet.chart_parts, sheet.drawn) = read_drawing(&mut pkg, &path, &xml, &colors.theme);
            sheet.tables = read_tables(&mut pkg, &path, &xml);
            for (n, local, text) in &names {
                let mine = local.map(|l| l == index).unwrap_or_else(|| refers_to_sheet(text, &name));
                if !mine {
                    continue;
                }
                match n.as_str() {
                    "_xlnm.Print_Area" => sheet.print_area = text.split(',').next().and_then(|r| range_ref(strip_sheet(r))),
                    "_xlnm.Print_Titles" => {
                        for part in text.split(',') {
                            let r = strip_sheet(part);
                            let (a, b) = r.split_once(':').unwrap_or((r, r));
                            let (a, b) = (a.trim_start_matches('$'), b.trim_start_matches('$'));
                            if let (Ok(r1), Ok(r2)) = (a.parse::<u32>(), b.parse::<u32>()) {
                                sheet.title_rows = Some((r1.saturating_sub(1), r2.saturating_sub(1)));
                            } else if let (Some(c1), Some(c2)) = (col_letters(a), col_letters(b)) {
                                sheet.title_cols = Some((c1, c2));
                            }
                        }
                    }
                    _ => {}
                }
            }
            sheets.push(sheet);
            hidden.push(is_hidden);
        }
    }
    work_out_formulas(&mut sheets, date1904);
    let tables: Vec<Vec<TableDef>> = sheets.iter_mut().map(|s| std::mem::take(&mut s.tables)).collect();
    let conds: Vec<Vec<CondFormat>> = sheets.iter_mut().map(|s| std::mem::take(&mut s.conds)).collect();
    let defs = super::sheet_style::Defs { dxfs: &dxfs, table_styles: &table_styles, theme: &colors.theme };
    super::sheet_style::apply(&mut sheets, &mut styles, &defs, &tables, &conds, date1904);
    read_charts(&mut sheets, &styles, &colors.theme, date1904);
    let mut hidden = hidden.into_iter();
    sheets.retain(|_| !hidden.next().unwrap_or(false));
    if sheets.is_empty() {
        return Err("the workbook has no sheets to print".into());
    }
    Ok(Workbook { sheets, styles, date1904 })
}
