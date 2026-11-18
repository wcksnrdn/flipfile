//! Unit tests of the markup

use super::*;

#[test]
fn codes_can_break_after_inner_hyphens() {
    assert_eq!(allow_break_after_hyphen("CPX-REG-001"), "CPX-REG-\u{200B}001");
    assert_eq!(allow_break_after_hyphen("-5 and A-B"), "-5 and A-B");
}

#[test]
fn strings_cannot_escape_into_code() {
    assert_eq!(q(r#"a"b\c"#), r#""a\"b\\c""#);
    assert_eq!(q("x\u{0}y\nz"), "\"xy z\"");
    // Markup-looking text stays text
    assert_eq!(q("#set page(width: 1pt)"), "\"#set page(width: 1pt)\"");
}

#[test]
fn fonts_get_similar_fallbacks() {
    assert!(font_list("Times New Roman").contains("Liberation Serif"));
    assert!(font_list("Calibri").contains("Carlito"));
    assert!(font_list("Courier New").contains("Menlo"));
}
