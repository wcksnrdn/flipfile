// End-to-end tests of the conversion pipeline.

use super::encode::*;
use super::input::*;
use super::strip::{strip_jpeg, strip_png};
use super::*;
use crate::pdf::writer::{jpeg_pages_to_pdf, Page};
use crate::state::settings::Resize;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, Rgb, RgbImage, RgbaImage};
use std::fs::File;
use std::path::PathBuf;

fn run(path: &str, format: &str, out_dir: Option<&Path>, options: &Options) -> Result<String, String> {
    convert(path, format, out_dir, options, "converted", &|_, _| {}).map(|c| c.path)
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flipfile-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 128, 200])))
}

// JPEG with an APP1 EXIF segment carrying Orientation = 6 (rotate 90° clockwise)
fn rotated_jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut jpeg = Vec::new();
    flatten_on_white(&sample(w, h))
        .write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, 90))
        .unwrap();
    let tiff: &[u8] = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(tiff);
    let len = (app1.len() + 2) as u16;
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn sample_docx(dir: &Path) -> PathBuf {
    use docx_rs::*;
    let docx = Docx::new()
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Quarterly Report").bold().size(36)))
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Revenue grew by twelve percent this quarter.")))
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Page one").add_break(BreakType::Page).add_text("Page two")));
    let path = dir.join("report.docx");
    let mut out = std::io::Cursor::new(Vec::new());
    docx.build().pack(&mut out).unwrap();
    fs::write(&path, out.into_inner()).unwrap();
    path
}

/// A small workbook written by hand: Rupiah accounting, a percentage, a merged title,
/// a print title row, and a hidden sheet that must not print
fn sample_xlsx(dir: &Path) -> PathBuf {
    use std::io::Write as _;
    let files: [(&str, &str); 7] = [
        ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#),
        ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
        ("xl/workbook.xml", r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Neraca" sheetId="1" r:id="rId1"/><sheet name="Draft" sheetId="2" state="hidden" r:id="rId2"/></sheets><definedNames><definedName name="_xlnm.Print_Titles" localSheetId="0">Neraca!$2:$2</definedName></definedNames></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#),
        ("xl/styles.xml", r#"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="_-[$Rp-421]* #,##0_-;\-[$Rp-421]* #,##0_-;_-[$Rp-421]* &quot;-&quot;_-;_-@_-"/></numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="14"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="2"><border/><border><bottom style="double"><color rgb="FF000000"/></bottom></border></borders><cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="164" fontId="0" fillId="0" borderId="1" applyNumberFormat="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0"><alignment horizontal="center"/></xf></cellXfs></styleSheet>"#),
        ("xl/sharedStrings.xml", r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>LAPORAN POSISI KEUANGAN</t></si><si><t>Akun</t></si><si><t>Kas dan setara kas</t></si><si><t>Pertumbuhan</t></si></sst>"#),
        ("xl/worksheets/sheet1.xml", r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cols><col min="1" max="1" width="30" customWidth="1"/><col min="2" max="2" width="22" customWidth="1"/></cols><sheetData><row r="1"><c r="A1" s="3" t="s"><v>0</v></c></row><row r="2"><c r="A2" t="s"><v>1</v></c></row><row r="3"><c r="A3" t="s"><v>2</v></c><c r="B3" s="1"><v>1500000</v></c></row><row r="4"><c r="A4" t="s"><v>3</v></c><c r="B4" s="2"><v>0.125</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#),
    ];
    let path = dir.join("neraca.xlsx");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for (name, body) in files {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.start_file("xl/worksheets/sheet2.xml", opts).unwrap();
    zip.write_all(br#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>SECRET DRAFT</t></is></c></row></sheetData></worksheet>"#).unwrap();
    zip.finish().unwrap();
    path
}

fn pdf_text(path: &str) -> (usize, String) {
    let pdfium = crate::pdf::render::pdfium().unwrap();
    let doc = pdfium.load_pdf_from_file(path, None).unwrap();
    let text = doc.pages().iter().map(|p| p.text().unwrap().all()).collect::<Vec<_>>().join("\n");
    (doc.pages().len() as usize, text)
}

/// A small deck with a title, nested bullets, a table and a picture, written by
/// flipfile itself (the tests have no PowerPoint)
fn sample_deck(dir: &Path) -> PathBuf {
    use crate::office::pptx_writer::{write, Deck, Item, NewSlide, Para, WIDE};
    let mut png = Vec::new();
    sample(64, 48).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut title = Para::new("Quarterly review", 40.0);
    title.bold = true;
    let mut first = Para::new("Revenue grew 12%", 24.0);
    first.bullet = Some(0);
    let mut nested = Para::new("Mostly in Jakarta", 20.0);
    nested.bullet = Some(1);
    let text = |paragraphs| Item::Text { x: 500_000, y: 300_000, w: 11_000_000, h: 2_000_000, paragraphs, anchor: "t", shrink: false };
    let deck = Deck {
        width: WIDE.0,
        height: WIDE.1,
        title: "Deck".into(),
        slides: vec![
            NewSlide { background: None, items: vec![text(vec![title, first, nested])] },
            NewSlide {
                background: Some([20, 30, 60]),
                items: vec![
                    Item::Picture { x: 500_000, y: 500_000, w: 3_000_000, h: 2_250_000, data: png, ext: "png", name: "chart".into() },
                    Item::Table { x: 500_000, y: 3_500_000, w: 8_000_000, rows: vec![vec!["Region".into(), "Q4".into()], vec!["Bandung".into(), "88".into()]], header: true, size: 14.0 },
                ],
            },
        ],
    };
    let path = dir.join("deck.pptx");
    fs::write(&path, write(&deck).unwrap()).unwrap();
    path
}

// The tests by kind of file; each starts with `use super::*` for the imports and
// sample files above.
#[cfg(target_os = "macos")] // audio goes through afconvert, which only macOS has
mod audio;
mod documents;
mod images;
mod pro_3d;
mod slides;
