//! Unit tests of the values and the layout

use super::*;

#[test]
fn notations() {
    let id = vec!["Saldo 1.234.567,89".to_string(), "Total 2.500.000".into()];
    assert_eq!(notation(&id), Notation::Indonesian);
    let en = vec!["Balance 1,234,567.89".to_string(), "12.50".into()];
    assert_eq!(notation(&en), Notation::English);
}

#[test]
fn amounts_become_numbers() {
    let id = Notation::Indonesian;
    assert_eq!(typed("1.234.567,89", id), (Value::Number(1234567.89), Some("#,##0.00".into())));
    assert_eq!(typed("Rp 25.000", id), (Value::Number(25000.0), Some("\"Rp\"#,##0".into())));
    assert_eq!(typed("Rp. 1.500.000,50", id), (Value::Number(1500000.5), Some("\"Rp\"#,##0.00".into())));
    assert_eq!(typed("(2.500)", id), (Value::Number(-2500.0), Some("#,##0;(#,##0)".into())));
    assert_eq!(typed("12,5%", id), (Value::Number(0.125), Some("0.0%".into())));
    assert_eq!(typed("42", id), (Value::Number(42.0), None));
    assert_eq!(typed("+Rp 9.537.000", id), (Value::Number(9537000.0), Some("\"Rp\"#,##0".into())));
    assert!(matches!(typed("+62 21 4586 1234", id).0, Value::Text(_)));
    let en = Notation::English;
    assert_eq!(typed("$1,234.50", en), (Value::Number(1234.5), Some("$#,##0.00".into())));
    assert_eq!(typed("-12.75", en), (Value::Number(-12.75), Some("0.00".into())));
}

#[test]
fn codes_stay_text() {
    let id = Notation::Indonesian;
    for code in ["0012", "3201234567890001", "081234567890", "1.2.3", "QO-1007", "12,345,678"] {
        assert!(matches!(typed(code, id).0, Value::Text(_)), "{code}");
    }
}

/// A workbook printed to PDF by flipfile's own engine and read back: the table, its
/// dates and amounts come back as values
#[test]
fn tables_come_back_from_a_pdf() {
    use crate::office::numfmt::{Separators, Value as N};
    use crate::office::xlsx::{self, Cell, Font, Sheet, Style, Workbook};
    let mut sheet = Sheet { name: "T".into(), default_col_width: 90.0, default_row_height: 15.0, ..Default::default() };
    sheet.col_widths.insert(1, 150.0);
    for (c, h) in ["Tanggal", "Keterangan", "Jumlah"].iter().enumerate() {
        sheet.cells.insert((0, c as u32), Cell { value: Some(N::Text(h.to_string())), style: 1 });
    }
    for i in 1..=12u32 {
        sheet.cells.insert((i, 0), Cell { value: Some(N::Number(46340.0 + i as f64)), style: 2 });
        sheet.cells.insert((i, 1), Cell { value: Some(N::Text(format!("Pembayaran tagihan {i}"))), style: 0 });
        sheet.cells.insert((i, 2), Cell { value: Some(N::Number(1250000.5 * i as f64)), style: 3 });
    }
    let normal = Style { num_fmt: "General".into(), ..Default::default() };
    let styles = vec![
        normal.clone(),
        Style { font: Font { bold: true, ..Default::default() }, ..normal.clone() },
        Style { num_fmt: "dd/mm/yyyy".into(), ..normal.clone() },
        Style { num_fmt: "#,##0.00".into(), ..normal },
    ];
    let wb = Workbook { sheets: vec![sheet], styles, date1904: false };
    let pdf = crate::office::sheet_pdf::to_pdf(&wb, Separators::COMMA, "t.xlsx").unwrap();
    let bytes = pdf_to_xlsx(crate::pdf::render::pdfium().unwrap(), &pdf, "T", &|_, _| {}).unwrap();
    let back = xlsx::read(&bytes).unwrap();
    let s = &back.sheets[0];
    let heading = s.cells.iter().find(|(_, c)| c.value == Some(N::Text("Tanggal".into()))).map(|(k, _)| *k).expect("the heading");
    let (r, c) = heading;
    assert_eq!(s.cells[&(r, c + 1)].value, Some(N::Text("Keterangan".into())));
    assert_eq!(s.cells[&(r + 1, c)].value, Some(N::Number(46341.0)));
    assert_eq!(back.styles[s.cells[&(r + 1, c)].style].num_fmt, "dd/mm/yyyy");
    assert_eq!(s.cells[&(r + 1, c + 1)].value, Some(N::Text("Pembayaran tagihan 1".into())));
    assert_eq!(s.cells[&(r + 12, c + 2)].value, Some(N::Number(15000006.0)));
    assert_eq!(back.styles[s.cells[&(r + 12, c + 2)].style].num_fmt, "#,##0.00");
}

#[test]
fn dates_become_dates() {
    let id = Notation::Indonesian;
    let d = |t: &str| typed(t, id);
    assert_eq!(d("15/11/2026"), (Value::Number(46341.0), Some("dd/mm/yyyy".into())));
    assert_eq!(d("2026-11-15"), (Value::Number(46341.0), Some("yyyy-mm-dd".into())));
    assert_eq!(d("15 November 2026"), (Value::Number(46341.0), Some("d mmmm yyyy".into())));
    assert_eq!(d("15 Desember 2026").1, Some("[$-421]d mmmm yyyy".into()));
    assert_eq!(d("15-Nov-26"), (Value::Number(46341.0), Some("d-mmm-yy".into())));
    let (v, f) = d("15/11/2026 08:30");
    assert_eq!(f, Some("dd/mm/yyyy hh:mm".into()));
    assert!(matches!(v, Value::Number(n) if (n - 46341.354166).abs() < 1e-4));
    // the month can only be the second number
    assert_eq!(typed("11/15/2026", Notation::English).1, Some("mm/dd/yyyy".into()));
}
