//! Unit tests of the formats

use super::*;

/// What is visible: hidden padding removed, a fill shown as one character
fn visible(s: &Shown) -> String {
    let mut t = s.text.clone();
    if let Some(fill) = s.fill {
        t.push(fill);
        t.push_str(s.after_fill.as_deref().unwrap_or(""));
    }
    let mut out = String::new();
    let mut hidden = false;
    for c in t.chars() {
        match c {
            HIDE_ON => hidden = true,
            HIDE_OFF => hidden = false,
            c if !hidden => out.push(c),
            _ => {}
        }
    }
    out
}
fn f(v: f64, code: &str) -> String {
    visible(&format(&Value::Number(v), code, false, Separators::US))
}
fn id(v: f64, code: &str) -> String {
    visible(&format(&Value::Number(v), code, false, Separators::COMMA))
}

#[test]
fn general_numbers() {
    assert_eq!(f(0.0, "General"), "0");
    assert_eq!(f(1234.5, "General"), "1234.5");
    assert_eq!(f(0.1 + 0.2, "General"), "0.3");
    assert_eq!(f(-12.0, "General"), "-12");
    assert_eq!(f(123456789012.0, "General"), "1.23457E+11");
    assert_eq!(id(1234.5, "General"), "1234,5");
}

#[test]
fn plain_patterns() {
    assert_eq!(f(1234567.891, "#,##0.00"), "1,234,567.89");
    assert_eq!(id(1234567.891, "#,##0.00"), "1.234.567,89");
    assert_eq!(f(-1234.5, "#,##0"), "-1,235");
    assert_eq!(f(0.5, "0%"), "50%");
    assert_eq!(f(0.12345, "0.00%"), "12.35%");
    assert_eq!(f(7.0, "000"), "007");
    assert_eq!(f(0.5, "#.##"), ".5");
    assert_eq!(f(1.005, "0.00"), "1.01");
    assert_eq!(f(2.675, "0.00"), "2.68");
    assert_eq!(f(1234567.0, "#,##0,"), "1,235");
    assert_eq!(f(12345.0, "0.00E+00"), "1.23E+04");
    assert_eq!(f(12.5, ".00"), "12.50");
    assert_eq!(f(5.0, "[$-409]#,##0"), "5");
}

#[test]
fn sections_and_colours() {
    assert_eq!(f(-1234.0, "#,##0;(#,##0)"), "(1,234)");
    assert_eq!(f(0.0, "#,##0;(#,##0);\"-\""), "-");
    let s = format(&Value::Number(-5.0), "#,##0;[Red]-#,##0", false, Separators::US);
    assert_eq!(s.color, Some([255, 0, 0]));
    assert_eq!(s.text, "-5");
    assert_eq!(f(150.0, "[>100]\"big\";\"small\""), "big");
}

#[test]
fn accounting() {
    let code = builtin(44).unwrap();
    let s = format(&Value::Number(-1250.5), code, false, Separators::US);
    assert_eq!(s.fill, Some(' '));
    assert_eq!(f(-1250.5, code), "$ (1,250.50)");
    assert_eq!(f(1250.5, code), "$ 1,250.50");
    // the padding is there, just invisible: positives line up with the ")" of negatives
    assert!(s.text.starts_with(HIDE_ON) && f(1250.5, code) != visible(&Shown::plain(format(&Value::Number(1250.5), code, false, Separators::US).text)));
    // Rupiah, as Indonesian Excel writes it
    let rp = r#"_-[$Rp-421]* #,##0_-;\-[$Rp-421]* #,##0_-;_-[$Rp-421]* "-"_-;_-@_-"#;
    assert_eq!(id(1500000.0, rp), "Rp 1.500.000");
    assert_eq!(id(-25000.0, rp), "-Rp 25.000");
    assert_eq!(id(0.0, rp), "Rp -");
}

#[test]
fn text_sections() {
    let s = format(&Value::Text("Total".into()), r#"_(@_)"#, false, Separators::US);
    assert_eq!(visible(&s), "Total");
    assert_eq!(format(&Value::Text("abc".into()), "0.00", false, Separators::US).text, "abc");
    assert_eq!(format(&Value::Text("x".into()), "\"Name: \"@", false, Separators::US).text, "Name: x");
}

#[test]
fn dates() {
    // 45000 = 2023-03-15, 1900-system
    assert_eq!(f(45000.0, "yyyy-mm-dd"), "2023-03-15");
    assert_eq!(f(45000.0, "d mmmm yyyy"), "15 March 2023");
    assert_eq!(f(45000.0, "ddd, d mmm yy"), "Wed, 15 Mar 23");
    assert_eq!(f(45000.75, "h:mm AM/PM"), "6:00 PM");
    assert_eq!(f(45000.5, "hh:mm:ss"), "12:00:00");
    assert_eq!(f(1.5, "[h]:mm"), "36:00");
    assert_eq!(f(45000.0, builtin(14).unwrap()), "3/15/2023");
    assert_eq!(id(45000.0, builtin(14).unwrap()), "15/03/2023");
    assert_eq!(f(45000.0, "[$-421]dddd, d mmmm yyyy"), "Rabu, 15 Maret 2023");
    assert_eq!(f(60.0, "yyyy-mm-dd"), "1900-02-29");
    assert_eq!(f(61.0, "yyyy-mm-dd"), "1900-03-01");
    assert_eq!(f(1.0, "yyyy-mm-dd"), "1900-01-01");
    let id_sys = Separators { indonesian: true, ..Separators::COMMA };
    assert_eq!(visible(&format(&Value::Number(45000.0), "dddd, d mmmm yyyy", false, id_sys)), "Rabu, 15 Maret 2023");
    assert_eq!(visible(&format(&Value::Number(45000.0), "[$-409]d mmm yyyy", false, id_sys)), "15 Mar 2023");
    assert!(is_date_format("dd/mm/yyyy") && is_date_format("[h]:mm") && !is_date_format("#,##0.00") && !is_date_format("[Red]0.00"));
}

#[test]
fn fractions() {
    assert_eq!(f(1.25, "# ?/?"), "1 1/4");
    assert_eq!(f(0.333, "?/?"), "1/3");
}
