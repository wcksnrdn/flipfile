//! Unit tests of the reader

use super::*;
use super::csv::parse_csv;

#[test]
fn references() {
    assert_eq!(cell_ref("A1"), Some((0, 0)));
    assert_eq!(cell_ref("$AB$12"), Some((11, 27)));
    assert_eq!(range_ref("$A$1:$C$9"), Some((0, 0, 8, 2)));
    assert_eq!(resolve("xl/worksheets", "../media/image1.png"), "xl/media/image1.png");
    assert_eq!(resolve("xl", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
}

#[test]
fn column_widths_match_excel() {
    // Calibri 11: max digit width 7 px; Excel's default 8.43 is stored as 9.140625 = 64 px
    assert_eq!(col_px(9.140625, 7.0), 64.0);
    assert_eq!(col_px(13.7109375, 7.0), 96.0);
}

#[test]
fn tints() {
    assert_eq!(tint([0x44, 0x72, 0xC4], 0.0), [0x44, 0x72, 0xC4]);
    // accent1 at 80% lighter: Excel shows D9E1F2
    let c = tint([0x44, 0x72, 0xC4], 0.799_981_7);
    assert!(c.iter().zip([0xD9, 0xE1, 0xF2]).all(|(a, b)| (*a as i32 - b).abs() <= 2), "{c:x?}");
}

#[test]
fn print_scales_outside_excels_range_mean_not_set() {
    // other programs write scale="0" for "no scale chosen": printing at 10% made the
    // sheet a speck on the page
    let xml = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><pageSetup paperSize="0" scale="0" orientation="portrait"/></worksheet>"#;
    let colors = Colors { theme: default_theme(), indexed: None };
    let sheet = read_sheet(xml, &[], &[Style::default()], 7.0, &colors).unwrap();
    assert_eq!(sheet.print.scale, 100.0);
    assert!(!sheet.print.explicit);
}

#[test]
fn csv_fields() {
    let rows = parse_csv("a,\"b,c\",\"say \"\"hi\"\"\"\n1;2\n", ',');
    assert_eq!(rows[0], vec!["a", "b,c", "say \"hi\""]);
    let wb = read_csv(b"Name;Amount\nKopi;25000\nTeh;0012\n", "x.csv").unwrap();
    let s = &wb.sheets[0];
    assert_eq!(s.cells[&(1, 1)].value, Some(Value::Number(25000.0)));
    // leading zeros are codes, not numbers
    assert_eq!(s.cells[&(2, 1)].value, Some(Value::Text("0012".into())));
}
