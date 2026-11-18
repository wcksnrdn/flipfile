//! CSV, shown as a plain table

use super::*;

/// A CSV file as a one-sheet workbook: separators detected (`,` `;` tab), numbers kept as numbers
pub fn read_csv(bytes: &[u8], name: &str) -> Result<Workbook, String> {
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).into_owned();
    let first = text.lines().next().unwrap_or("");
    let delim = [',', ';', '\t', '|'].into_iter().max_by_key(|d| first.matches(*d).count()).unwrap_or(',');
    let mut sheet = Sheet { name: name.to_string(), default_col_width: 48.0, default_row_height: 15.0, auto_rows: true, ..Default::default() };
    sheet.print.gridlines = true;
    let mut widths: HashMap<u32, usize> = HashMap::new();
    for (r, record) in parse_csv(&text, delim).into_iter().enumerate() {
        for (c, field) in record.into_iter().enumerate() {
            if field.is_empty() {
                continue;
            }
            let w = widths.entry(c as u32).or_default();
            *w = (*w).max(field.chars().count());
            let style = if r == 0 { 1 } else { 0 };
            let value = match field.trim().parse::<f64>() {
                Ok(n) if !field.trim().starts_with('0') || field.trim() == "0" || field.trim().starts_with("0.") => Value::Number(n),
                _ => Value::Text(field),
            };
            sheet.cells.insert((r as u32, c as u32), Cell { value: Some(value), style });
        }
    }
    // columns as wide as their content (within reason), like Excel's autofit
    for (c, chars) in widths {
        sheet.col_widths.insert(c, (chars.clamp(6, 50) as f32 * 5.6 + 8.0).min(300.0));
    }
    if sheet.cells.is_empty() {
        return Err("the CSV file is empty".into());
    }
    let normal = Style { num_fmt: "General".into(), ..Default::default() };
    let header = Style { font: Font { bold: true, ..Default::default() }, fill: Some([242, 242, 242]), ..normal.clone() };
    Ok(Workbook { sheets: vec![sheet], styles: vec![normal, header], date1904: false })
}

pub(super) fn parse_csv(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            }
            '"' if field.is_empty() => quoted = true,
            c if c == delim && !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}
