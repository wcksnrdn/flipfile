// Writes Excel workbooks (.xlsx) with real formatting: fonts, fills, borders,
// alignment and wrapping, number formats, merged cells, column widths, row heights and
// frozen heading rows. Used for PDF -> Excel, where the cells come with the look they
// had in the PDF.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub type Rgb = [u8; 3];

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    Thin,
    Medium,
    Thick,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Look {
    /// empty: the workbook's font (Calibri)
    pub font: String,
    /// tenths of a point; 0: 11 pt
    pub size10: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<Rgb>,
    pub fill: Option<Rgb>,
    /// left, right, top, bottom
    pub borders: [Option<(Edge, Rgb)>; 4],
    /// "left", "center", "right"
    pub h: Option<&'static str>,
    /// "top", "center" (bottom when unset)
    pub v: Option<&'static str>,
    pub wrap: bool,
    pub num_fmt: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub value: Value,
    pub look: Look,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<(u32, u32), Cell>,
    /// (first row, first col, last row, last col)
    pub merges: Vec<(u32, u32, u32, u32)>,
    /// column widths in characters (Excel's unit)
    pub col_widths: BTreeMap<u32, f32>,
    /// row heights in points
    pub row_heights: BTreeMap<u32, f32>,
    /// rows kept on screen while scrolling
    pub freeze_rows: u32,
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // characters XML can't carry
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\t' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// "AB" for column 27 (zero-based)
pub fn col_name(mut c: u32) -> String {
    let mut s = vec![];
    loop {
        s.push((b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.iter().rev().collect()
}

fn hex(c: Rgb) -> String {
    format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The styles part, with each distinct look as a cell format
struct Styles {
    fonts: Vec<String>,
    fills: Vec<String>,
    borders: Vec<String>,
    num_fmts: Vec<String>,
    xfs: Vec<String>,
    index: HashMap<Look, usize>,
}

impl Styles {
    fn new() -> Self {
        Styles {
            fonts: vec![r#"<font><sz val="11"/><name val="Calibri"/><family val="2"/></font>"#.into()],
            fills: vec![r#"<fill><patternFill patternType="none"/></fill>"#.into(), r#"<fill><patternFill patternType="gray125"/></fill>"#.into()],
            borders: vec!["<border><left/><right/><top/><bottom/><diagonal/></border>".into()],
            num_fmts: vec![],
            xfs: vec![r#"<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#.into()],
            index: HashMap::from([(Look::default(), 0)]),
        }
    }

    fn intern(list: &mut Vec<String>, item: String) -> usize {
        match list.iter().position(|x| *x == item) {
            Some(i) => i,
            None => {
                list.push(item);
                list.len() - 1
            }
        }
    }

    fn xf(&mut self, look: &Look) -> usize {
        if let Some(&i) = self.index.get(look) {
            return i;
        }
        let mut font = String::from("<font>");
        if look.bold {
            font.push_str("<b/>");
        }
        if look.italic {
            font.push_str("<i/>");
        }
        if look.underline {
            font.push_str("<u/>");
        }
        let size = if look.size10 == 0 { 11.0 } else { look.size10 as f32 / 10.0 };
        font.push_str(&format!("<sz val=\"{size}\"/>"));
        if let Some(c) = look.color {
            font.push_str(&format!("<color rgb=\"{}\"/>", hex(c)));
        }
        let name = if look.font.is_empty() { "Calibri" } else { &look.font };
        font.push_str(&format!("<name val=\"{}\"/></font>", esc(name)));
        let font_id = Self::intern(&mut self.fonts, font);
        let fill_id = match look.fill {
            Some(c) => Self::intern(&mut self.fills, format!("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"{}\"/><bgColor indexed=\"64\"/></patternFill></fill>", hex(c))),
            None => 0,
        };
        let border_id = if look.borders.iter().all(Option::is_none) {
            0
        } else {
            let side = |name: &str, e: Option<(Edge, Rgb)>| match e {
                Some((kind, c)) => {
                    let style = match kind {
                        Edge::Thin => "thin",
                        Edge::Medium => "medium",
                        Edge::Thick => "thick",
                    };
                    format!("<{name} style=\"{style}\"><color rgb=\"{}\"/></{name}>", hex(c))
                }
                None => format!("<{name}/>"),
            };
            let [l, r, t, b] = look.borders;
            Self::intern(&mut self.borders, format!("<border>{}{}{}{}<diagonal/></border>", side("left", l), side("right", r), side("top", t), side("bottom", b)))
        };
        let fmt_id = match &look.num_fmt {
            None => 0,
            Some(code) => match builtin_format(code) {
                Some(id) => id,
                None => 164 + Self::intern(&mut self.num_fmts, code.clone()),
            },
        };
        let mut xf = format!("<xf numFmtId=\"{fmt_id}\" fontId=\"{font_id}\" fillId=\"{fill_id}\" borderId=\"{border_id}\" xfId=\"0\"");
        if fmt_id != 0 {
            xf.push_str(" applyNumberFormat=\"1\"");
        }
        if font_id != 0 {
            xf.push_str(" applyFont=\"1\"");
        }
        if fill_id != 0 {
            xf.push_str(" applyFill=\"1\"");
        }
        if border_id != 0 {
            xf.push_str(" applyBorder=\"1\"");
        }
        if look.h.is_some() || look.v.is_some() || look.wrap {
            xf.push_str(" applyAlignment=\"1\"><alignment");
            if let Some(h) = look.h {
                xf.push_str(&format!(" horizontal=\"{h}\""));
            }
            if let Some(v) = look.v {
                xf.push_str(&format!(" vertical=\"{v}\""));
            }
            if look.wrap {
                xf.push_str(" wrapText=\"1\"");
            }
            xf.push_str("/></xf>");
        } else {
            xf.push_str("/>");
        }
        self.xfs.push(xf);
        let i = self.xfs.len() - 1;
        self.index.insert(look.clone(), i);
        i
    }

    fn xml(&self) -> String {
        let mut s = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#);
        if !self.num_fmts.is_empty() {
            s.push_str(&format!("<numFmts count=\"{}\">", self.num_fmts.len()));
            for (i, code) in self.num_fmts.iter().enumerate() {
                s.push_str(&format!("<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>", 164 + i, esc(code)));
            }
            s.push_str("</numFmts>");
        }
        s.push_str(&format!("<fonts count=\"{}\">{}</fonts>", self.fonts.len(), self.fonts.concat()));
        s.push_str(&format!("<fills count=\"{}\">{}</fills>", self.fills.len(), self.fills.concat()));
        s.push_str(&format!("<borders count=\"{}\">{}</borders>", self.borders.len(), self.borders.concat()));
        s.push_str(r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#);
        s.push_str(&format!("<cellXfs count=\"{}\">{}</cellXfs>", self.xfs.len(), self.xfs.concat()));
        s.push_str(r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#);
        s
    }
}

/// Excel's own number formats, which need no definition
fn builtin_format(code: &str) -> Option<usize> {
    Some(match code {
        "General" => 0,
        "0" => 1,
        "0.00" => 2,
        "#,##0" => 3,
        "#,##0.00" => 4,
        "0%" => 9,
        "0.00%" => 10,
        "h:mm" => 20,
        _ => return None,
    })
}

/// A sheet name Excel accepts: no \ / ? * [ ] :, at most 31 characters, unique
fn sheet_name(name: &str, used: &mut Vec<String>) -> String {
    let clean: String = name.chars().filter(|c| !matches!(c, '\\' | '/' | '?' | '*' | '[' | ']' | ':')).collect();
    let clean = clean.trim().trim_matches('\'').to_string();
    let base: String = if clean.is_empty() { "Sheet".into() } else { clean.chars().take(31).collect() };
    let mut candidate = base.clone();
    let mut n = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
        let suffix = format!(" ({n})");
        candidate = format!("{}{suffix}", base.chars().take(31 - suffix.len()).collect::<String>());
        n += 1;
    }
    used.push(candidate.clone());
    candidate
}

fn number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

pub fn write(sheets: &[Sheet]) -> Result<Vec<u8>, String> {
    let err = |e: zip::result::ZipError| format!("the workbook could not be written ({e})");
    let io = |e: std::io::Error| format!("the workbook could not be written ({e})");
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let sheets: Vec<Sheet> = if sheets.is_empty() { vec![Sheet { name: "Sheet1".into(), ..Default::default() }] } else { sheets.to_vec() };
    let mut styles = Styles::new();
    let mut used = vec![];
    let names: Vec<String> = sheets.iter().map(|s| sheet_name(&s.name, &mut used)).collect();

    let mut parts = vec![];
    for sheet in &sheets {
        let mut ws = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#);
        if let Some(&(r2, _)) = sheet.cells.keys().next_back() {
            let last_col = sheet.cells.keys().map(|k| k.1).max().unwrap_or(0);
            ws.push_str(&format!("<dimension ref=\"A1:{}{}\"/>", col_name(last_col), r2 + 1));
        }
        if sheet.freeze_rows > 0 {
            ws.push_str(&format!(
                "<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"{n}\" topLeftCell=\"A{}\" activePane=\"bottomLeft\" state=\"frozen\"/><selection pane=\"bottomLeft\" activeCell=\"A{}\" sqref=\"A{}\"/></sheetView></sheetViews>",
                sheet.freeze_rows + 1,
                sheet.freeze_rows + 1,
                sheet.freeze_rows + 1,
                n = sheet.freeze_rows
            ));
        }
        ws.push_str("<sheetFormatPr defaultRowHeight=\"15\"/>");
        if !sheet.col_widths.is_empty() {
            ws.push_str("<cols>");
            for (c, w) in &sheet.col_widths {
                ws.push_str(&format!("<col min=\"{0}\" max=\"{0}\" width=\"{1:.2}\" customWidth=\"1\"/>", c + 1, w.clamp(1.0, 255.0)));
            }
            ws.push_str("</cols>");
        }
        ws.push_str("<sheetData>");
        let mut row: Option<u32> = None;
        let rows_with_height: Vec<u32> = sheet.row_heights.keys().copied().collect();
        let mut pending_heights = rows_with_height.iter().peekable();
        let open_row = |ws: &mut String, r: u32| {
            match sheet.row_heights.get(&r) {
                Some(h) => ws.push_str(&format!("<row r=\"{}\" ht=\"{:.2}\" customHeight=\"1\">", r + 1, h.clamp(1.0, 409.0))),
                None => ws.push_str(&format!("<row r=\"{}\">", r + 1)),
            }
        };
        for (&(r, c), cell) in &sheet.cells {
            if row != Some(r) {
                if row.is_some() {
                    ws.push_str("</row>");
                }
                // rows that only have a height
                while let Some(&&hr) = pending_heights.peek() {
                    if hr >= r {
                        break;
                    }
                    if row.is_none_or(|p| hr > p) {
                        ws.push_str(&format!("<row r=\"{}\" ht=\"{:.2}\" customHeight=\"1\"/>", hr + 1, sheet.row_heights[&hr].clamp(1.0, 409.0)));
                    }
                    pending_heights.next();
                }
                if pending_heights.peek() == Some(&&r) {
                    pending_heights.next();
                }
                open_row(&mut ws, r);
                row = Some(r);
            }
            let xf = styles.xf(&cell.look);
            let s = if xf == 0 { String::new() } else { format!(" s=\"{xf}\"") };
            let at = format!("{}{}", col_name(c), r + 1);
            match &cell.value {
                Value::Number(n) if n.is_finite() => ws.push_str(&format!("<c r=\"{at}\"{s}><v>{}</v></c>", number(*n))),
                Value::Text(t) if !t.is_empty() => ws.push_str(&format!("<c r=\"{at}\"{s} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", esc(t))),
                _ => ws.push_str(&format!("<c r=\"{at}\"{s}/>")),
            }
        }
        if row.is_some() {
            ws.push_str("</row>");
        }
        ws.push_str("</sheetData>");
        if !sheet.merges.is_empty() {
            ws.push_str(&format!("<mergeCells count=\"{}\">", sheet.merges.len()));
            for (r1, c1, r2, c2) in &sheet.merges {
                ws.push_str(&format!("<mergeCell ref=\"{}{}:{}{}\"/>", col_name(*c1), r1 + 1, col_name(*c2), r2 + 1));
            }
            ws.push_str("</mergeCells>");
        }
        ws.push_str(r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#);
        parts.push(ws);
    }

    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#);
    for i in 0..parts.len() {
        ct.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>", i + 1));
    }
    ct.push_str("</Types>");
    zip.start_file("[Content_Types].xml", opts).map_err(err)?;
    zip.write_all(ct.as_bytes()).map_err(io)?;
    zip.start_file("_rels/.rels", opts).map_err(err)?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#).map_err(io)?;
    let mut wb = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#);
    for (i, name) in names.iter().enumerate() {
        wb.push_str(&format!("<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>", esc(name), i + 1, i + 1));
        rels.push_str(&format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>", i + 1, i + 1));
    }
    wb.push_str("</sheets></workbook>");
    rels.push_str("</Relationships>");
    zip.start_file("xl/workbook.xml", opts).map_err(err)?;
    zip.write_all(wb.as_bytes()).map_err(io)?;
    zip.start_file("xl/_rels/workbook.xml.rels", opts).map_err(err)?;
    zip.write_all(rels.as_bytes()).map_err(io)?;
    for (i, ws) in parts.iter().enumerate() {
        zip.start_file(format!("xl/worksheets/sheet{}.xml", i + 1), opts).map_err(err)?;
        zip.write_all(ws.as_bytes()).map_err(io)?;
    }
    // the styles last: every sheet has added its looks
    zip.start_file("xl/styles.xml", opts).map_err(err)?;
    zip.write_all(styles.xml().as_bytes()).map_err(io)?;
    zip.finish().map_err(err)?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_workbooks_read_back() {
        let mut s = Sheet { name: "Data: Q1/Q2".into(), freeze_rows: 1, ..Default::default() };
        let bold = Look { bold: true, fill: Some([0xD9, 0xE1, 0xF2]), borders: [Some((Edge::Thin, [0, 0, 0])); 4], ..Default::default() };
        s.cells.insert((0, 0), Cell { value: Value::Text("Nama".into()), look: bold.clone() });
        s.cells.insert((0, 1), Cell { value: Value::Text("Jumlah".into()), look: bold });
        s.cells.insert((1, 0), Cell { value: Value::Text("Kopi <& teh>".into()), look: Look::default() });
        s.cells.insert((1, 1), Cell { value: Value::Number(1234567.5), look: Look { num_fmt: Some("\"Rp\"#,##0.00".into()), ..Default::default() } });
        s.merges.push((3, 0, 3, 1));
        s.cells.insert((3, 0), Cell { value: Value::Text("Total".into()), look: Look::default() });
        s.col_widths.insert(0, 20.0);
        s.row_heights.insert(2, 30.0);
        let bytes = write(&[s]).unwrap();
        let wb = super::super::xlsx::read(&bytes).unwrap();
        let sheet = &wb.sheets[0];
        assert_eq!(sheet.name, "Data Q1Q2");
        assert_eq!(sheet.cells[&(1, 1)].value, Some(super::super::numfmt::Value::Number(1234567.5)));
        assert_eq!(wb.styles[sheet.cells[&(1, 1)].style].num_fmt, "\"Rp\"#,##0.00");
        assert!(wb.styles[sheet.cells[&(0, 0)].style].font.bold);
        assert_eq!(sheet.merges, vec![(3, 0, 3, 1)]);
        assert_eq!(sheet.row_heights.get(&2), Some(&30.0));
        assert_eq!(sheet.frozen, (1, 0));
    }
}
