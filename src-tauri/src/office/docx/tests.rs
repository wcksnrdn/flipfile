//! Unit tests of the reader

use super::*;

#[test]
fn numbers_like_word() {
    assert_eq!(format_number(3, "lowerLetter"), "c");
    assert_eq!(format_number(27, "lowerLetter"), "aa");
    assert_eq!(format_number(4, "upperRoman"), "IV");
    assert_eq!(format_number(14, "lowerRoman"), "xiv");
    assert_eq!(format_number(7, "decimal"), "7");
}

#[test]
fn relationship_paths_resolve() {
    assert_eq!(resolve_path("word", "media/image1.png"), "word/media/image1.png");
    assert_eq!(resolve_path("word", "../customXml/item1.xml"), "customXml/item1.xml");
    assert_eq!(resolve_path("word", "/word/media/a.png"), "word/media/a.png");
    // Cannot escape the package root
    assert_eq!(resolve_path("word", "../../../etc/passwd"), "etc/passwd");
}

#[test]
fn rejects_non_documents() {
    assert!(read(b"not a zip").is_err());
}
