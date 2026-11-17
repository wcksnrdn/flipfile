//! Unit tests of the reader

use super::*;

#[test]
fn colours_follow_theme_modifiers() {
    assert_eq!(from_hsl(0.0, 1.0, 0.5), [255, 0, 0]);
    let (h, s, l) = to_hsl([68, 114, 196]);
    assert_eq!(from_hsl(h, s, l), [68, 114, 196]);
    assert_eq!(number_text("arabicPeriod", 3), "3.");
    assert_eq!(number_text("romanUcPeriod", 4), "IV.");
    assert_eq!(number_text("alphaLcParenR", 2), "b)");
    assert_eq!(resolve("ppt/slides", "../media/image1.png"), "ppt/media/image1.png");
}

/// An Embedded OpenType wrapper as PowerPoint saves one: a 228-byte header, then the font
fn eot(font: &[u8], flags: u32) -> Vec<u8> {
    let mut out = vec![0u8; 228];
    let size = (out.len() + font.len()) as u32;
    out[0..4].copy_from_slice(&size.to_le_bytes());
    out[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
    out[8..12].copy_from_slice(&0x0002_0002u32.to_le_bytes());
    out[12..16].copy_from_slice(&flags.to_le_bytes());
    out[34..36].copy_from_slice(&[0x4C, 0x50]);
    out.extend_from_slice(font);
    out
}

#[test]
fn embedded_fonts_are_unwrapped() {
    let font: Vec<u8> = [0, 1, 0, 0].into_iter().chain((0..60).map(|i| i as u8)).collect();
    // A plain font file, and one in an EOT wrapper, plain or XOR-obscured
    assert_eq!(font_data(&font).as_deref(), Some(font.as_slice()));
    assert_eq!(font_data(&eot(&font, 0)).as_deref(), Some(font.as_slice()));
    let obscured: Vec<u8> = font.iter().map(|b| b ^ 0x50).collect();
    assert_eq!(font_data(&eot(&obscured, 0x1000_0000)).as_deref(), Some(font.as_slice()));
    // MicroType Express compression is not unpacked; damaged headers are refused
    assert_eq!(font_data(&eot(&font, 0x4)), None);
    let mut bad = eot(&font, 0);
    bad[4..8].copy_from_slice(&10_000u32.to_le_bytes());
    assert_eq!(font_data(&bad), None);
    assert_eq!(font_data(b"not a font at all, not even close"), None);
}
