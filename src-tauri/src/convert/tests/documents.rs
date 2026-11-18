//! Office documents, spreadsheets and e-books through the pipeline, and the text
//! summaries read from each kind

use super::*;

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
        // (the blocky hand-drawn O may read as C or 0; the fast recogniser, used when the
        // accurate one fails, keeps the gaps between the letters: "H E L L O")
        assert!(text.to_uppercase().replace(' ', "").contains("HELL"), "{text}");
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

#[test]
fn summaries_read_the_text_of_every_document_kind() {
    let dir = temp_dir("summary-text");
    let none = &|_: usize, _: usize| {};

    let docx = sample_docx(&dir);
    let word = text::extract(&docx, none).unwrap();
    assert_eq!(word.kind, "Word document");
    assert!(word.text.contains("Revenue grew by twelve percent this quarter."), "{}", word.text);

    // A PDF: its pages are counted, its lines joined into paragraphs
    let pdf = run(docx.to_str().unwrap(), "pdf", None, &Options::default()).unwrap();
    let read = text::extract(Path::new(&pdf), none).unwrap();
    assert_eq!((read.kind, read.pages, read.ocr), ("PDF", 2, false));
    assert!(read.text.contains("Revenue grew by twelve percent this quarter."), "{}", read.text);

    let deck = text::extract(&sample_deck(&dir), none).unwrap();
    assert_eq!(deck.kind, "Presentation");
    assert!(deck.text.contains("Revenue grew 12%") && deck.text.contains("Bandung"), "{}", deck.text);

    // Visible cells as a Markdown table, hidden sheets left out
    let sheet = text::extract(&sample_xlsx(&dir), none).unwrap();
    assert_eq!(sheet.kind, "Spreadsheet");
    assert!(sheet.text.contains("Kas dan setara kas"), "{}", sheet.text);
}

/// A scanned page has no text layer: it is read by the system's text recognition
#[cfg(target_os = "macos")]
#[test]
fn summaries_read_scanned_pdfs() {
    let dir = temp_dir("summary-scan");
    // The text of a real PDF, as pixels only
    let pdf = run(sample_docx(&dir).to_str().unwrap(), "pdf", None, &Options::default()).unwrap();
    let png = run(&pdf, "png", None, &Options::default()).unwrap();
    let page = fs::read_dir(&png).unwrap().map(|e| e.unwrap().path()).min().unwrap();
    let scan = dir.join("scan.pdf");
    let image = image::open(&page).unwrap().to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90).encode_image(&image).unwrap();
    let (w, h) = image.dimensions();
    fs::write(&scan, jpeg_pages_to_pdf(&[Page { jpeg, width: w, height: h, gray: false }])).unwrap();

    let read = text::extract(&scan, &|_, _| {}).unwrap();
    assert!(read.ocr, "a page without a text layer is recognised");
    assert!(read.text.contains("Revenue grew"), "{}", read.text);
}
