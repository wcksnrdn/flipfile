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

#[test]
fn converts_to_every_format() {
    let dir = temp_dir("formats");
    let src = dir.join("photo.png");
    sample(300, 200).save(&src).unwrap();
    for ext in ["jpg", "jpeg", "png", "webp", "bmp", "tiff", "gif", "ico", "avif", "pdf"] {
        let out = run(src.to_str().unwrap(), ext, None, &Options::default()).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert!(out.ends_with(&format!("photo_converted.{ext}")), "{out}");
        if !matches!(ext, "pdf" | "avif") {
            image::open(&out).unwrap_or_else(|e| panic!("{ext} unreadable: {e}"));
        }
    }
    // Never overwrites
    assert!(run(src.to_str().unwrap(), "png", None, &Options::default()).unwrap().ends_with("photo_converted_2.png"));
}

#[test]
fn ico_is_square_and_at_most_256() {
    let dir = temp_dir("ico");
    let src = dir.join("wide.png");
    sample(1000, 400).save(&src).unwrap();
    let ico = image::open(run(src.to_str().unwrap(), "ico", None, &Options::default()).unwrap()).unwrap();
    assert_eq!((ico.width(), ico.height()), (256, 256));
}

#[test]
fn applies_exif_orientation() {
    let dir = temp_dir("orient");
    let src = dir.join("phone.jpg");
    fs::write(&src, rotated_jpeg(40, 20)).unwrap();
    let out = image::open(run(src.to_str().unwrap(), "png", None, &Options::default()).unwrap()).unwrap();
    assert_eq!((out.width(), out.height()), (20, 40));
}

#[test]
fn clean_strips_exif_losslessly() {
    let dir = temp_dir("clean");
    let src = dir.join("plain.jpg");
    let data = rotated_jpeg(40, 20);
    // Same file but without rotation, so the lossless path is taken
    let stripped_once = strip_jpeg(&data).unwrap();
    assert!(stripped_once.len() < data.len());
    assert!(!stripped_once.windows(6).any(|w| w == b"Exif\0\0"));
    fs::write(&src, &stripped_once).unwrap();
    let out = clean(&src, None).unwrap();
    assert!(out.ends_with("plain_clean.jpg"));
    assert_eq!(fs::read(&out).unwrap(), stripped_once);

    // Rotated photo: re-encoded upright, EXIF gone
    let rotated = dir.join("rotated.jpg");
    fs::write(&rotated, &data).unwrap();
    let out = clean(&rotated, None).unwrap();
    let bytes = fs::read(&out).unwrap();
    assert!(!bytes.windows(6).any(|w| w == b"Exif\0\0"));
    let img = image::open(&out).unwrap();
    assert_eq!((img.width(), img.height()), (20, 40));
}

#[test]
fn png_strip_drops_text_chunks() {
    let dir = temp_dir("png");
    let src = dir.join("a.png");
    sample(10, 10).save(&src).unwrap();
    let mut data = fs::read(&src).unwrap();
    // Insert a tEXt chunk right after IHDR (8 sig + 25 IHDR)
    let text = b"Comment\0secret";
    let mut chunk = (text.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(text);
    chunk.extend_from_slice(&[0, 0, 0, 0]); // CRC is not validated by strip_png
    data.splice(33..33, chunk);
    let out = strip_png(&data).unwrap();
    assert!(!out.windows(6).any(|w| w == b"secret"));
    image::load_from_memory(&out).unwrap();
}

#[test]
fn merges_images_into_one_pdf() {
    let dir = temp_dir("merge");
    let mut paths = vec![];
    for (i, (w, h)) in [(100, 50), (60, 90), (80, 80)].into_iter().enumerate() {
        let p = dir.join(format!("img{i}.png"));
        sample(w, h).save(&p).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }
    paths.push(dir.join("missing.png").to_string_lossy().into_owned());
    let channel = Channel::new(|_| Ok(()));
    let result = merge_to_pdf(&paths, &channel, None, &Options::default()).unwrap();
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.errors.len(), 1);
    assert!(result.outputs[0].ends_with("img0_merged.pdf"));
    let pdf = fs::read(&result.outputs[0]).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 3"));
    assert_eq!(text.matches("/Type /Page ").count(), 3);
}

#[test]
fn out_dir_uses_plain_names() {
    let dir = temp_dir("outdir");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let src = dir.join("Clipboard 2026-10-20 at 14.03.12.png");
    sample(20, 20).save(&src).unwrap();
    let a = run(src.to_str().unwrap(), "jpg", Some(&out), &Options::default()).unwrap();
    let b = run(src.to_str().unwrap(), "jpg", Some(&out), &Options::default()).unwrap();
    assert_eq!(Path::new(&a), out.join("Clipboard 2026-10-20 at 14.03.12.jpg"));
    assert_eq!(Path::new(&b), out.join("Clipboard 2026-10-20 at 14.03.12_2.jpg"));
}

#[test]
fn resizes_without_upscaling() {
    let img = sample(400, 200);
    assert_eq!(resize(img.clone(), Resize::Original).dimensions(), (400, 200));
    assert_eq!(resize(img.clone(), Resize::Scale(0.5)).dimensions(), (200, 100));
    assert_eq!(resize(img.clone(), Resize::MaxEdge(100)).dimensions(), (100, 50));
    assert_eq!(resize(img, Resize::MaxEdge(1920)).dimensions(), (400, 200));
}

#[test]
fn lower_quality_gives_smaller_jpeg() {
    let dir = temp_dir("quality");
    let src = dir.join("noise.png");
    DynamicImage::ImageRgb8(RgbImage::from_fn(300, 300, |x, y| Rgb([(x * 7 ^ y * 13) as u8, (x * y) as u8, (x + y) as u8])))
        .save(&src)
        .unwrap();
    let big = run(src.to_str().unwrap(), "jpg", None, &Options { resize: Resize::Original, quality: 95 }).unwrap();
    let small = run(src.to_str().unwrap(), "jpg", None, &Options { resize: Resize::Original, quality: 60 }).unwrap();
    assert!(fs::metadata(&small).unwrap().len() < fs::metadata(&big).unwrap().len());
}

// Uses the PDFium library downloaded by `npm run fetch-pdfium`.
#[test]
fn renders_pdf_pages_to_images() {
    let dir = temp_dir("pdfrender");
    let pages: Vec<Page> = [(200, 100), (100, 200), (150, 150)]
        .into_iter()
        .map(|(w, h)| pdf_page(&sample(w, h), 90).unwrap())
        .collect();
    let doc = dir.join("doc.pdf");
    fs::write(&doc, jpeg_pages_to_pdf(&pages)).unwrap();
    let one = dir.join("single.pdf");
    fs::write(&one, jpeg_pages_to_pdf(&pages[..1])).unwrap();

    // Multi-page: a folder with one image per page, at 200 DPI (page size is 0.75 pt/px)
    let reported = Mutex::new(vec![]);
    let on_page = |done: usize, total: usize| reported.lock().unwrap().push((done, total));
    let c = convert(doc.to_str().unwrap(), "png", None, &Options::default(), "converted", &on_page).unwrap();
    assert_eq!(*reported.lock().unwrap(), vec![(0, 3), (1, 3), (2, 3), (3, 3)]);
    assert_eq!(c.pages, 3);
    assert!(c.path.ends_with("doc_pages"));
    let first = image::open(Path::new(&c.path).join("page-01.png")).unwrap();
    let expected = (200.0 * 0.75 * 200.0 / 72.0_f32).round() as u32;
    assert!((first.width() as i64 - expected as i64).abs() <= 2, "{} vs {expected}", first.width());
    assert!(first.width() > first.height());
    assert!(Path::new(&c.path).join("page-03.png").exists());

    // Single page: a plain file; resize settings apply to rendered pages too
    let small = Options { resize: Resize::MaxEdge(100), quality: 90 };
    let c = convert(one.to_str().unwrap(), "jpg", None, &small, "converted", &|_, _| {}).unwrap();
    assert!(c.path.ends_with("single_converted.jpg"));
    assert_eq!(image::open(&c.path).unwrap().width(), 100);

    // PDF -> PDF is refused with a clear message
    assert!(convert(doc.to_str().unwrap(), "pdf", None, &Options::default(), "converted", &|_, _| {}).is_err());
}

#[test]
fn preset_drop_keeps_the_format() {
    let dir = temp_dir("same");
    let noisy = |w: u32, h: u32| {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| Rgb([(x * 7 ^ y * 13) as u8, (x * y) as u8, (x + y) as u8])))
    };
    let photo = dir.join("photo.jpeg");
    noisy(400, 300).write_with_encoder(JpegEncoder::new_with_quality(&mut File::create(&photo).unwrap(), 98)).unwrap();
    let shot = dir.join("shot.png");
    noisy(400, 300).save(&shot).unwrap();
    let compress = Options { resize: Resize::Original, quality: COMPRESS_QUALITY };
    let none = &|_: usize, _: usize| {};

    // Compress: same format and spelling, smaller file, sizes reported
    let c = convert(photo.to_str().unwrap(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(c.path.ends_with("photo_compressed.jpeg"), "{}", c.path);
    let (before, after) = c.sizes.unwrap();
    assert!(after < before, "{after} >= {before}");

    // PNG stays PNG (lossless, max compression)
    let c = convert(shot.to_str().unwrap(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(c.path.ends_with("shot_compressed.png"));
    assert_eq!(image::open(&c.path).unwrap().to_rgb8(), image::open(&shot).unwrap().to_rgb8());

    // Resize preset: same format, smaller dimensions
    let half = Options { resize: Resize::Scale(0.5), quality: 90 };
    let c = convert(shot.to_str().unwrap(), SAME_FORMAT, None, &half, "resized", none).unwrap();
    assert!(c.path.ends_with("shot_resized.png"));
    assert_eq!(image::open(&c.path).unwrap().dimensions(), (200, 150));
}

#[test]
fn bmp_and_tiff_get_as_small_as_their_format_allows() {
    let dir = temp_dir("rawformats");
    let photo = dir.join("photo.png");
    // Opaque, but stored as RGBA (like most screenshots)
    DynamicImage::ImageRgba8(RgbaImage::from_fn(300, 200, |x, y| image::Rgba([(x * 3) as u8, (y * 5) as u8, 90, 255])))
        .save(&photo)
        .unwrap();
    let none = &|_: usize, _: usize| {};
    let plain = Options::default();
    let compress = Options { resize: Resize::Original, quality: COMPRESS_QUALITY };

    // BMP: 24-bit for opaque images (3 bytes per pixel + header), with an honest note on Compress
    let bmp = run(photo.to_str().unwrap(), "bmp", None, &plain).unwrap();
    assert!(fs::metadata(&bmp).unwrap().len() < 300 * 200 * 3 + 1024);
    let c = convert(bmp.as_str(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(c.path.ends_with("_compressed.bmp"));
    assert_eq!(c.note.as_deref(), Some("BMP can't be compressed further"));
    assert_eq!(image::open(&c.path).unwrap().to_rgb8(), image::open(&photo).unwrap().to_rgb8());

    // Transparent images keep their alpha channel in BMP
    let clear = dir.join("clear.png");
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 40, image::Rgba([10, 20, 30, 0]))).save(&clear).unwrap();
    let bmp = run(clear.to_str().unwrap(), "bmp", None, &plain).unwrap();
    assert!(image::open(&bmp).unwrap().color().has_alpha());

    // TIFF: Compress uses lossless Deflate, so it is smaller and pixel-identical
    let tif = run(photo.to_str().unwrap(), "tiff", None, &plain).unwrap();
    let small = convert(tif.as_str(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(small.path.ends_with("_compressed.tiff"));
    let (before, after) = small.sizes.unwrap();
    assert!(after < before / 2, "{after} vs {before}");
    assert_eq!(image::open(&small.path).unwrap().to_rgb8(), image::open(&photo).unwrap().to_rgb8());
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

#[test]
fn spreadsheets_convert_through_the_pipeline() {
    let dir = temp_dir("sheet");
    let xlsx = sample_xlsx(&dir);
    let none = &|_: usize, _: usize| {};
    let plain = Options::default();

    // Excel -> PDF (built-in engine in tests)
    let pdf = convert(xlsx.to_str().unwrap(), "pdf", None, &plain, "converted", none).unwrap();
    assert!(pdf.path.ends_with("neraca_converted.pdf"));
    assert_eq!(pdf.note.as_deref(), Some("built-in engine"));
    let (pages, text) = pdf_text(&pdf.path);
    assert_eq!(pages, 1, "{text}");
    for expected in ["LAPORAN POSISI KEUANGAN", "Kas dan setara kas", "Rp", "Pertumbuhan"] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
    // the amount, grouped in the computer's own region style
    let sep = crate::office::numfmt::Separators::system();
    assert!(text.contains(&format!("1{g}500{g}000", g = sep.group)), "{text}");
    assert!(text.contains(&format!("12{d}50%", d = sep.decimal)), "{text}");
    assert!(!text.contains("SECRET DRAFT"), "hidden sheets must not print");

    // Excel -> images, through the PDF
    let png = convert(xlsx.to_str().unwrap(), "png", None, &plain, "converted", none).unwrap();
    assert!(png.path.ends_with(".png"), "{}", png.path);

    // CSV -> PDF, with ; separators detected
    let csv = dir.join("kas.csv");
    fs::write(&csv, "Tanggal;Keterangan;Jumlah\n2026-01-02;Setoran awal;2500000\n").unwrap();
    let pdf = convert(csv.to_str().unwrap(), "pdf", None, &plain, "converted", none).unwrap();
    let (_, text) = pdf_text(&pdf.path);
    assert!(text.contains("Setoran awal") && text.contains("2500000"), "{text}");

    // Spreadsheets also convert into Word documents as tables
    let docx_out = convert(xlsx.to_str().unwrap(), "docx", None, &plain, "converted", none).unwrap();
    assert!(docx_out.path.ends_with(".docx"));
}

#[test]
fn word_documents_convert_through_the_pipeline() {
    let dir = temp_dir("word");
    let docx = sample_docx(&dir);
    let none = &|_: usize, _: usize| {};
    let plain = Options::default();

    // Word -> PDF (built-in engine in tests)
    let pdf = convert(docx.to_str().unwrap(), "pdf", None, &plain, "converted", none).unwrap();
    assert!(pdf.path.ends_with("report_converted.pdf"));
    assert!(fs::read(&pdf.path).unwrap().starts_with(b"%PDF-"));

    // Word -> images: one per page, named after the Word file
    let png = convert(docx.to_str().unwrap(), "png", None, &plain, "converted", none).unwrap();
    assert_eq!(png.pages, 2);
    assert!(png.path.ends_with("report_pages"));

    // PDF -> Word, and that Word file reads back with the same text
    let back = convert(&pdf.path, "docx", None, &plain, "converted", none).unwrap();
    assert!(back.path.ends_with("report_converted_converted.docx"));
    let model = crate::office::docx::read(&fs::read(&back.path).unwrap()).unwrap();
    let text: String = model
        .sections
        .iter()
        .flat_map(|s| s.blocks.iter())
        .filter_map(|b| match b {
            crate::office::docx::Block::Paragraph(p) => Some(
                p.inlines
                    .iter()
                    .filter_map(|i| match i {
                        crate::office::docx::Inline::Text(t, _) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Quarterly Report"), "{text}");
    assert!(text.contains("Revenue grew by twelve percent this quarter."), "{text}");
    assert!(text.contains("Page two"), "{text}");

    // Clear refusals for conversions that make no sense
    assert!(convert(docx.to_str().unwrap(), "docx", None, &plain, "converted", none).is_err());
    let photo = dir.join("photo.png");
    sample(20, 20).save(&photo).unwrap();
    let err = convert(photo.to_str().unwrap(), "docx", None, &plain, "converted", none).err().unwrap();
    assert!(err.contains("only PDF files can become Word documents"), "{err}");

    // Word OCR: a picture of text becomes an editable Word document (macOS/Windows)
    if cfg!(any(target_os = "macos", windows)) {
        let page = dir.join("scan.png");
        let mut img = RgbImage::from_pixel(900, 300, Rgb([255, 255, 255]));
        // "HELLO" in big block letters: 5x7 dot patterns, 12 px per dot
        let font: [[&str; 7]; 5] = [
            ["X...X", "X...X", "X...X", "XXXXX", "X...X", "X...X", "X...X"],
            ["XXXXX", "X....", "X....", "XXXX.", "X....", "X....", "XXXXX"],
            ["X....", "X....", "X....", "X....", "X....", "X....", "XXXXX"],
            ["X....", "X....", "X....", "X....", "X....", "X....", "XXXXX"],
            [".XXX.", "X...X", "X...X", "X...X", "X...X", "X...X", ".XXX."],
        ];
        for (k, letter) in font.iter().enumerate() {
            for (row, bits) in letter.iter().enumerate() {
                for (col, bit) in bits.chars().enumerate() {
                    if bit == 'X' {
                        for dy in 0..12 {
                            for dx in 0..12 {
                                img.put_pixel(60 + k as u32 * 160 + col as u32 * 12 + dx, 60 + row as u32 * 12 + dy, Rgb([0, 0, 0]));
                            }
                        }
                    }
                }
            }
        }
        img.save(&page).unwrap();
        let done = convert(page.to_str().unwrap(), "docx-ocr", None, &plain, "converted", none).unwrap();
        let model = crate::office::docx::read(&fs::read(&done.path).unwrap()).unwrap();
        let text = format!("{:?}", model.sections);
        // (the blocky hand-drawn O may read as C or 0)
        assert!(text.to_uppercase().contains("HELL"), "{text}");
    }
    // A ZIP that is not a Word document is not treated as one
    let fake = dir.join("archive.docx.zip");
    fs::write(&fake, b"PK\x03\x04 not really").unwrap();
    assert!(word_kind(&fake).is_none());
}

/// Word OCR during development:
/// `FLIPFILE_SCAN=scan.pdf|photo.jpg FLIPFILE_OUT=dir cargo test review_ocr -- --ignored --nocapture`
/// writes the Word file into `dir` and renders it back as ocr1.png, ocr2.png, …
#[test]
#[ignore]
fn review_ocr() {
    use pdfium_render::prelude::PdfRenderConfig;
    let input = std::env::var("FLIPFILE_SCAN").expect("set FLIPFILE_SCAN");
    let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
    let none = &|_: usize, _: usize| {};
    let start = std::time::Instant::now();
    let done = convert(&input, "docx-ocr", Some(&out), &Options::default(), "converted", none).unwrap();
    println!("ocr -> docx: {:.2}s {}", start.elapsed().as_secs_f64(), done.path);
    let pdf = crate::office::docx_to_pdf_builtin(&fs::read(&done.path).unwrap()).unwrap();
    let pdfium = pdf_render::pdfium().unwrap();
    let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
    for (i, page) in doc.pages().iter().enumerate().take(4) {
        let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(900)).unwrap().as_image().unwrap();
        img.save(out.join(format!("ocr{}.png", i + 1))).unwrap();
    }
    println!("pages: {}", doc.pages().len());
}

#[test]
fn webp_is_lossy_and_respects_quality() {
    let dir = temp_dir("webp-lossy");
    let src = dir.join("noisy.png");
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(400, 300, |x, y| {
        Rgb([(x * 17 ^ y * 31) as u8, (x * y) as u8, (x * 3 + y * 5) as u8])
    }));
    img.save(&src).unwrap();

    let high = run(src.to_str().unwrap(), "webp", None, &Options { resize: Resize::Original, quality: 92 }).unwrap();
    let low = run(src.to_str().unwrap(), "webp", None, &Options { resize: Resize::Original, quality: 55 }).unwrap();
    let high_len = fs::metadata(&high).unwrap().len();
    let low_len = fs::metadata(&low).unwrap().len();
    let png_len = fs::metadata(&src).unwrap().len();
    assert!(low_len < high_len, "low {low_len} >= high {high_len}");
    assert!(high_len < png_len, "webp {high_len} >= png {png_len}");
}

#[test]
fn ico_multisize_and_icns_are_valid() {
    let dir = temp_dir("icons");
    let src = dir.join("app.png");
    sample(512, 320).save(&src).unwrap();

    // Multi-size ICO has 6 entries (16, 32, 48, 64, 128, 256) in the ICONDIR header
    let ico_path = run(src.to_str().unwrap(), "ico", None, &Options::default()).unwrap();
    let ico_bytes = fs::read(&ico_path).unwrap();
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]);
    assert_eq!(count, 6);
    let decoded = image::open(&ico_path).unwrap();
    assert_eq!(decoded.dimensions(), (256, 256));

    // ICNS starts with "icns" and matches its big-endian length header
    let icns_path = run(src.to_str().unwrap(), "icns", None, &Options::default()).unwrap();
    let icns_bytes = fs::read(&icns_path).unwrap();
    assert_eq!(&icns_bytes[0..4], b"icns");
    let total_len = u32::from_be_bytes([icns_bytes[4], icns_bytes[5], icns_bytes[6], icns_bytes[7]]) as usize;
    assert_eq!(total_len, icns_bytes.len());
}

#[cfg(target_os = "macos")]
#[test]
fn encodes_and_decodes_heic() {
    let dir = temp_dir("heic-out");
    let src = dir.join("photo.png");
    sample(120, 80).save(&src).unwrap();
    let out = run(src.to_str().unwrap(), "heic", None, &Options::default()).unwrap();
    assert!(out.ends_with(".heic"));
    let head = fs::read(&out).unwrap();
    assert!(crate::platform::system_codec::is_heif_family(&head));
    let decoded = crate::platform::system_codec::decode(Path::new(&out)).unwrap();
    assert_eq!(decoded.dimensions(), (120, 80));
}

#[test]
fn photo_tools_transform_square_and_compress_to_target() {
    let dir = temp_dir("photo-tools");
    let src = dir.join("rect.jpg");
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(600, 400, |x, y| {
        Rgb([(x * 13 ^ y * 29) as u8, (x + y * 3) as u8, (x ^ y) as u8])
    }));
    img.write_with_encoder(JpegEncoder::new_with_quality(&mut File::create(&src).unwrap(), 96)).unwrap();

    // Rotate 90 CW swaps width and height
    let rot = run(src.to_str().unwrap(), "rot-r", None, &Options::default()).unwrap();
    assert_eq!(image::open(&rot).unwrap().dimensions(), (400, 600));

    // Flip and grayscale preserve dimensions
    let gray = run(src.to_str().unwrap(), "grayscale", None, &Options::default()).unwrap();
    assert_eq!(image::open(&gray).unwrap().dimensions(), (600, 400));

    // Square pads 600x400 to 600x600
    let sq = run(src.to_str().unwrap(), "square", None, &Options::default()).unwrap();
    assert_eq!(image::open(&sq).unwrap().dimensions(), (600, 600));

    // Target-size compression fits strictly under 100 KB
    let c100 = run(src.to_str().unwrap(), "cmp-100k", None, &Options::default()).unwrap();
    let size = fs::metadata(&c100).unwrap().len();
    assert!(size <= 100_000, "expected <= 100 KB, got {size}");

    // Lossless compression works on PNG
    let png = dir.join("graphic.png");
    sample(200, 150).save(&png).unwrap();
    let lossless = run(png.to_str().unwrap(), "cmp-lossless", None, &Options::default()).unwrap();
    assert_eq!(image::open(&lossless).unwrap().to_rgb8(), image::open(&png).unwrap().to_rgb8());
}

#[test]
fn encodes_and_decodes_pro_3d_photo_formats() {
    let dir = temp_dir("photo-pro");
    let src = dir.join("texture.png");
    sample(64, 48).save(&src).unwrap();

    for fmt in ["tga", "qoi", "exr", "hdr", "ppm"] {
        let out = run(src.to_str().unwrap(), fmt, None, &Options::default()).unwrap();
        assert!(out.ends_with(&format!(".{fmt}")), "expected .{fmt}, got {out}");
        let back = run(&out, "png", None, &Options::default()).unwrap();
        assert_eq!(image::open(&back).unwrap().dimensions(), (64, 48));
    }

    let svg = run(src.to_str().unwrap(), "svg", None, &Options::default()).unwrap();
    assert!(svg.ends_with(".svg"));
    let svg_text = fs::read_to_string(&svg).unwrap();
    assert!(svg_text.contains("<svg") && svg_text.contains("data:image/png;base64,"));
}

#[test]
fn cross_converts_office_data_and_spreadsheets() {
    let dir = temp_dir("office-data");
    let csv = dir.join("sales.csv");
    fs::write(&csv, "item,qty,price\nWidget,12,9.50\nGadget,5,19.99\n").unwrap();

    // CSV -> XLSX -> JSON -> YAML -> XML -> SQL -> MD Table -> TSV
    let xlsx = run(csv.to_str().unwrap(), "xlsx", None, &Options::default()).unwrap();
    assert!(xlsx.ends_with(".xlsx"));

    let json = run(&xlsx, "json", None, &Options::default()).unwrap();
    let json_text = fs::read_to_string(&json).unwrap();
    assert!(json_text.contains("\"Widget\"") && json_text.contains("12"));

    let yaml = run(&json, "yaml", None, &Options::default()).unwrap();
    let yaml_text = fs::read_to_string(&yaml).unwrap();
    assert!(yaml_text.contains("item: Widget"));

    let xml = run(&yaml, "xml", None, &Options::default()).unwrap();
    let xml_text = fs::read_to_string(&xml).unwrap();
    assert!(xml_text.contains("<item>Widget</item>"));

    let sql = run(&xml, "sql", None, &Options::default()).unwrap();
    let sql_text = fs::read_to_string(&sql).unwrap();
    assert!(sql_text.contains("CREATE TABLE") && sql_text.contains("INSERT INTO") && sql_text.contains("'Widget'"));

    let md_tbl = run(csv.to_str().unwrap(), "md-table", None, &Options::default()).unwrap();
    let md_text = fs::read_to_string(&md_tbl).unwrap();
    assert!(md_text.contains("| item | qty | price |") && md_text.contains("| --- | --- | --- |"));

    let tsv = run(csv.to_str().unwrap(), "tsv", None, &Options::default()).unwrap();
    let tsv_text = fs::read_to_string(&tsv).unwrap();
    assert!(tsv_text.contains("item\tqty\tprice"));
}

#[test]
fn cross_converts_office_documents_and_ebooks() {
    let dir = temp_dir("office-docs");
    let md = dir.join("guide.md");
    fs::write(&md, "# Flipfile Guide\n\nConvert documents locally.\n\n- Fast\n- Offline\n").unwrap();

    let html = run(md.to_str().unwrap(), "html", None, &Options::default()).unwrap();
    assert!(fs::read_to_string(&html).unwrap().contains("<h1>Flipfile Guide</h1>"));

    let epub = run(md.to_str().unwrap(), "epub", None, &Options::default()).unwrap();
    assert!(epub.ends_with(".epub"));
    let txt_from_epub = run(&epub, "txt", None, &Options::default()).unwrap();
    assert!(fs::read_to_string(&txt_from_epub).unwrap().contains("Flipfile Guide"));

    let odt = run(md.to_str().unwrap(), "odt", None, &Options::default()).unwrap();
    assert!(odt.ends_with(".odt"));
    let md_from_odt = run(&odt, "md", None, &Options::default()).unwrap();
    assert!(fs::read_to_string(&md_from_odt).unwrap().contains("# Flipfile Guide"));

    let rtf = run(md.to_str().unwrap(), "rtf", None, &Options::default()).unwrap();
    assert!(fs::read_to_string(&rtf).unwrap().starts_with("{\\rtf1"));

    let docx = run(md.to_str().unwrap(), "docx", None, &Options::default()).unwrap();
    assert!(docx.ends_with(".docx"));

    let pdf = run(md.to_str().unwrap(), "pdf", None, &Options::default()).unwrap();
    assert!(pdf.ends_with(".pdf"));
    assert!(fs::metadata(&pdf).unwrap().len() > 200);
}

#[cfg(target_os = "macos")]
#[test]
fn converts_audio_formats_via_system_afconvert() {
    let dir = temp_dir("audio-conv");
    let wav = dir.join("tone.wav");

    // Write a valid 0.1s 8kHz mono 16-bit PCM WAV file (800 samples = 1600 bytes)
    let samples: Vec<i16> = (0..800).map(|i| ((i * 40) % 4000 - 2000) as i16).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // 1 channel
    bytes.extend_from_slice(&8000u32.to_le_bytes()); // 8000 Hz
    bytes.extend_from_slice(&16000u32.to_le_bytes()); // byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(&wav, &bytes).unwrap();

    for (fmt, ext) in [("m4a", ".m4a"), ("flac", ".flac"), ("aiff", ".aiff"), ("caf", ".caf")] {
        let out = run(wav.to_str().unwrap(), fmt, None, &Options::default()).unwrap();
        assert!(out.ends_with(ext), "expected {ext}, got {out}");
        assert!(fs::metadata(&out).unwrap().len() > 32);
    }
}


