//! Unit tests of the page analysis

use super::*;

#[test]
fn font_names_are_cleaned() {
    assert_eq!(clean_font_name("ABCDEF+TimesNewRomanPS-BoldMT"), "Times New Roman");
    assert_eq!(clean_font_name("Calibri-Bold"), "Calibri");
    assert_eq!(clean_font_name("Helvetica"), "Arial");
    assert_eq!(clean_font_name("ArialMT"), "Arial");
}

#[test]
fn detects_list_items() {
    assert!(starts_list_item("• Fast"));
    assert!(starts_list_item("12. Twelfth"));
    assert!(!starts_list_item("2026 was a year"));
    assert!(!starts_list_item("Plain text"));
    assert!(is_enumerator("a."));
    assert!(is_enumerator("(iv)"));
    assert!(!is_enumerator("word"));
}

#[test]
fn page_numbers_become_fields() {
    assert_eq!(split_numbers("Page 4 of 12", 4, 12), vec![Piece::Text("Page "), Piece::Page, Piece::Text(" of "), Piece::Pages]);
    assert_eq!(split_numbers("Halaman 4/81", 4, 112), vec![Piece::Text("Halaman "), Piece::Page, Piece::Text("/81")]);
    // Codes and dates are left alone
    assert_eq!(split_numbers("WI-QO-1007.00 v4.2", 4, 12), vec![Piece::Text("WI-QO-1007.00 v4.2")]);
}

fn style() -> Style {
    Style { font: "Arial".into(), size: 10.0, bold: false, italic: false, underline: false, color: None, spacing: 0.0, vert: 0 }
}

fn glyph(ch: char, left: f32, baseline: f32) -> Glyph {
    Glyph { ch, left, right: left + 5.0, baseline, style: style(), generated: false }
}

#[test]
fn dot_leaders_become_one_tab() {
    let mut gs = vec![];
    let mut x = 10.0;
    for c in "Intro".chars() {
        gs.push(glyph(c, x, 100.0));
        x += 5.0;
    }
    for _ in 0..20 {
        gs.push(glyph('.', x, 100.0));
        x += 5.0;
    }
    gs.push(glyph('7', x + 2.0, 100.0));
    let refs: Vec<&Glyph> = gs.iter().collect();
    let ls = lines(&refs, &[]);
    assert_eq!(ls.len(), 1);
    assert_eq!(ls[0].text(), "Intro\t7");
    assert_eq!(ls[0].tabs.len(), 1);
    assert!(ls[0].tabs[0].leader);
    assert!((ls[0].tabs[0].left - (x + 2.0)).abs() < 0.01);
}

#[test]
fn row_rules_without_column_lines_become_tables() {
    let black = [0, 0, 0];
    let h = |y: f32| Seg { horizontal: true, pos: y, a: 50.0, b: 300.0, width: 0.5, color: black };
    let segs = vec![h(100.0), h(120.0), h(140.0)];
    let groups = segment_groups(&segs);
    let stacks = rule_stacks(&segs, &groups);
    assert_eq!(stacks.len(), 1);
    // Two columns of text between the rules
    let mut glyphs = vec![];
    for (row, y) in [(0, 114.0), (1, 134.0)] {
        for (k, c) in "Name".chars().enumerate() {
            glyphs.push(glyph(c, 60.0 + k as f32 * 5.0, y));
        }
        for (k, c) in format!("Value{row}").chars().enumerate() {
            glyphs.push(glyph(c, 200.0 + k as f32 * 5.0, y));
        }
    }
    let (_, lat) = rule_table(&segs, &stacks[0], &glyphs).unwrap();
    assert_eq!((lat.xs.len() - 1, lat.ys.len() - 1), (2, 2));
    assert!(lat.cells.iter().all(|c| c.borders[0].is_some() && c.borders[1].is_none()));
}

#[test]
fn list_numbers_drawn_after_their_text_join_the_line() {
    let mut gs = vec![];
    for (k, c) in "Payment terms".chars().enumerate() {
        gs.push(glyph(c, 40.0 + k as f32 * 5.0, 100.0));
    }
    // "2." comes later in the PDF, a hair lower, left of the text
    gs.push(glyph('2', 26.0, 100.4));
    gs.push(glyph('.', 31.0, 100.4));
    let refs: Vec<&Glyph> = gs.iter().collect();
    let ls = lines(&refs, &[]);
    assert_eq!(ls.len(), 1);
    assert!(ls[0].text().starts_with("2."), "{}", ls[0].text());
}

#[test]
fn grids_of_lines_become_tables() {
    let black = [0, 0, 0];
    let h = |y: f32, a: f32, b: f32| Seg { horizontal: true, pos: y, a, b, width: 0.5, color: black };
    let v = |x: f32, a: f32, b: f32| Seg { horizontal: false, pos: x, a, b, width: 0.5, color: black };
    // 2 x 2 grid whose top row is one merged cell
    let segs = vec![h(0.0, 0.0, 200.0), h(20.0, 0.0, 200.0), h(40.0, 0.0, 200.0), v(0.0, 0.0, 40.0), v(200.0, 0.0, 40.0), v(100.0, 20.0, 40.0)];
    let groups = segment_groups(&segs);
    assert_eq!(groups.len(), 1);
    let lat = lattice(&segs, &groups[0], &[]).unwrap();
    assert_eq!(lat.xs.len(), 3);
    assert_eq!(lat.ys.len(), 3);
    assert_eq!(lat.cells.len(), 3);
    assert_eq!((lat.cells[0].cols, lat.cells[0].rows), (2, 1));
    assert!(lat.cells.iter().all(|c| c.borders.iter().all(|b| b.is_some())));
}
