//! Unit tests of the layout

use super::*;

#[test]
fn bands_split_and_break() {
    let size = |_: u32| 10.0;
    assert_eq!(bands(0, 9, &size, 35.0, &[]), vec![(0, 2), (3, 5), (6, 8), (9, 9)]);
    assert_eq!(bands(0, 5, &size, 100.0, &[2]), vec![(0, 1), (2, 5)]);
}

#[test]
fn header_codes() {
    let p = header_parts("&L&\"Arial,Bold\"Laporan &A&RPage &P of &N", 2, 5, "x.xlsx", "Neraca");
    assert_eq!(p, ["Laporan Neraca".to_string(), String::new(), "Page 2 of 5".to_string()]);
}

/// A table `cols` wide and `rows` tall with a bold heading row
fn table(cols: u32, rows: u32) -> Workbook {
    use super::super::xlsx::{Cell, Font};
    let mut sheet = Sheet { name: "T".into(), default_col_width: 64.0, default_row_height: 15.0, ..Default::default() };
    for c in 0..cols {
        sheet.cells.insert((0, c), Cell { value: Some(Value::Text(format!("Col {c}"))), style: 1 });
        for r in 1..rows {
            sheet.cells.insert((r, c), Cell { value: Some(Value::Number((r * c) as f64)), style: 0 });
        }
    }
    let normal = Style { num_fmt: "General".into(), ..Default::default() };
    let bold = Style { font: Font { bold: true, ..Default::default() }, ..normal.clone() };
    Workbook { sheets: vec![sheet], styles: vec![normal, bold], date1904: false }
}

#[test]
fn wide_tables_fit_the_page_width() {
    // 20 columns of 64 pt: 1280 pt, Excel would print 3 pages across
    let wb = table(20, 10);
    let printed = print(&wb, Separators::US, "t.xlsx").unwrap();
    assert_eq!(printed.pages.len(), 1);
    assert!(printed.sheets[0].contains("width: 841.90pt"), "landscape A4: {}", printed.sheets[0]);
    // a sheet set up to print as it is keeps Excel's layout
    let mut wb = table(20, 10);
    wb.sheets[0].print.explicit = true;
    assert_eq!(print(&wb, Separators::US, "t.xlsx").unwrap().pages.len(), 3);
}

#[test]
fn long_tables_repeat_their_heading() {
    let wb = table(5, 200);
    let printed = print(&wb, Separators::US, "t.xlsx").unwrap();
    assert!(printed.pages.len() > 2);
    assert!(printed.pages.iter().all(|(_, p)| p.contains("\"Col 3\"")), "the heading row is on every page");
    // narrow enough for portrait
    assert!(printed.sheets[0].contains("width: 595.30pt"));
}

#[test]
fn typst_strings_are_escaped() {
    assert_eq!(typst_str("a \"b\" \\ #c"), "\"a \\\"b\\\" \\\\ #c\"");
}
