// Office documents (Word, Excel) <-> PDF, implemented in-process.

pub mod apps;
pub mod docx;
pub mod from_pdf;
pub mod handout;
pub mod numfmt;
pub mod pptx;
pub mod pptx_writer;
pub mod ocr;
pub mod shapes;
pub mod sheet_pdf;
pub mod slides_pdf;
mod pdf_docx_writer;
pub mod to_typst;
pub mod typeset;
pub mod xlsx;

/// Excel (.xlsx) or CSV -> PDF with flipfile's built-in engine (reader + Typst layout).
pub fn sheet_to_pdf_builtin(bytes: &[u8], file_name: &str, csv: bool) -> Result<Vec<u8>, String> {
    let workbook = if csv { xlsx::read_csv(bytes, file_name)? } else { xlsx::read(bytes)? };
    sheet_pdf::to_pdf(&workbook, numfmt::Separators::system(), file_name)
}

/// Word -> PDF with flipfile's built-in engine (DOCX reader + Typst layout).
pub fn docx_to_pdf_builtin(docx_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let document = docx::read(docx_bytes)?;
    let generated = to_typst::generate(&document);
    typeset::to_pdf(generated.markup, generated.files).map_err(|e| format!("the document could not be laid out ({e})"))
}

/// PowerPoint -> PDF with flipfile's built-in engine (PPTX reader + Typst layout).
pub fn pptx_to_pdf_builtin(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let deck = pptx::read(bytes)?;
    slides_pdf::to_pdf(&deck)
}

/// Deeper than any real document nests its elements (Office parts stay under ~30)
const MAX_XML_DEPTH: usize = 256;

/// Whether `xml` nests elements deeper than [`MAX_XML_DEPTH`]. roxmltree's tokenizer
/// recurses once per open element, so a part nested thousands deep overflows the stack,
/// an abort no `catch_unwind` can stop, before any limit of our own is reached. A quick
/// scan of the bytes refuses such a part before it is parsed.
pub(crate) fn xml_too_deep(xml: &str) -> bool {
    let bytes = xml.as_bytes();
    let find = |from: usize, needle: &[u8]| {
        bytes.get(from..).and_then(|rest| rest.windows(needle.len()).position(|w| w == needle)).map(|p| from + p + needle.len())
    };
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(p) = bytes.get(i..).and_then(|rest| rest.iter().position(|&b| b == b'<')) {
        i += p + 1;
        // Comments, CDATA and declarations may hold '<' that opens nothing
        let skip_to = match bytes.get(i) {
            Some(b'?') => find(i, b"?>"),
            Some(b'!') if bytes[i..].starts_with(b"!--") => find(i, b"-->"),
            Some(b'!') if bytes[i..].starts_with(b"![CDATA[") => find(i, b"]]>"),
            Some(b'!') => find(i, b">"),
            Some(b'/') => {
                depth = depth.saturating_sub(1);
                find(i, b">")
            }
            _ => {
                // An opening tag: to its '>', past quoted values (which may hold '>' or "/>")
                let mut quote = None;
                let mut end = None;
                for (k, &b) in bytes[i..].iter().enumerate() {
                    match (quote, b) {
                        (Some(q), _) if b == q => quote = None,
                        (Some(_), _) => {}
                        (None, b'"' | b'\'') => quote = Some(b),
                        (None, b'>') => {
                            end = Some(i + k);
                            break;
                        }
                        _ => {}
                    }
                }
                if end.is_some_and(|e| bytes[e - 1] != b'/') {
                    depth += 1;
                    if depth > MAX_XML_DEPTH {
                        return true;
                    }
                }
                end.map(|e| e + 1)
            }
        };
        match skip_to {
            Some(next) => i = next,
            None => break,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use docx_rs::*;
    use pdfium_render::prelude::*;

    fn pdf_text(pdf: &[u8]) -> (usize, String) {
        // The app's shared PDFium binding: PDFium can only be bound once per process
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(pdf, None).unwrap();
        let mut text = String::new();
        for page in doc.pages().iter() {
            text.push_str(&page.text().unwrap().all());
            text.push('\n');
        }
        (doc.pages().len() as usize, text)
    }

    fn sample_docx() -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 30]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let numbered = |t: &str| {
            Paragraph::new()
                .add_run(Run::new().add_text(t))
                .numbering(NumberingId::new(2), IndentLevel::new(0))
        };
        let cell = |t: &str| TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(t)));

        let docx = Docx::new()
            .header(Header::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text("Quarterly Report Header"))))
            .footer(Footer::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text("Footer text"))))
            .add_abstract_numbering(
                AbstractNumbering::new(1).add_level(
                    Level::new(0, Start::new(1), NumberFormat::new("decimal"), LevelText::new("%1."), LevelJc::new("left"))
                        .indent(Some(720), Some(SpecialIndentType::Hanging(360)), None, None),
                ),
            )
            .add_numbering(Numbering::new(2, 1))
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Flipfile Test Document").bold().size(40)).align(AlignmentType::Center))
            .add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text("Normal text, "))
                    .add_run(Run::new().add_text("bold text, ").bold())
                    .add_run(Run::new().add_text("italic text").italic()),
            )
            .add_paragraph(numbered("First item"))
            .add_paragraph(numbered("Second item"))
            .add_paragraph(numbered("Third item"))
            .add_table(Table::new(vec![
                TableRow::new(vec![cell("Name"), cell("Price")]),
                TableRow::new(vec![cell("Coffee"), cell("25000")]),
            ]))
            .add_paragraph(Paragraph::new().add_run(Run::new().add_image(Pic::new(&png))))
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Before break").add_break(BreakType::Page).add_text("After break")))
            .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Quotes \"and\" #hash [brackets] \\backslash")));

        let mut out = std::io::Cursor::new(Vec::new());
        docx.build().pack(&mut out).unwrap();
        out.into_inner()
    }

    #[test]
    fn word_document_becomes_pdf() {
        let docx = sample_docx();
        let model = docx::read(&docx).unwrap();
        let pdf = docx_to_pdf_builtin(&docx).unwrap_or_else(|e| {
            let g = to_typst::generate(&model);
            panic!("{e}\n---\n{}", g.markup)
        });
        let (pages, text) = pdf_text(&pdf);
        assert_eq!(pages, 2, "{text}");
        for expected in [
            "Flipfile Test Document",
            "bold text",
            "italic text",
            "1.",
            "Third item",
            "3.",
            "Coffee",
            "25000",
            "Before break",
            "After break",
            "Quarterly Report Header",
            "Footer text",
            "Quotes \"and\" #hash [brackets] \\backslash",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
        assert_eq!(model.images.len(), 1);
    }

    /// Two sheets on different paper, many pages each, with a header numbering them all
    fn long_workbook() -> Vec<u8> {
        use std::io::Write;
        let rows = |n: usize, label: &str| -> String {
            (1..=n)
                .map(|r| format!(r#"<row r="{r}"><c r="A{r}" t="inlineStr"><is><t>{label} row {r}</t></is></c><c r="B{r}"><v>{}</v></c><c r="C{r}" t="inlineStr"><is><t>Some longer text that spills over</t></is></c></row>"#, r * 1250))
                .collect()
        };
        let sheet = |n: usize, label: &str, landscape: bool| {
            format!(
                r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{}</sheetData><pageSetup orientation="{}"/><headerFooter><oddHeader>&amp;CPage &amp;P of &amp;N</oddHeader></headerFooter></worksheet>"#,
                rows(n, label),
                if landscape { "landscape" } else { "portrait" }
            )
        };
        let files = [
            ("[Content_Types].xml".to_string(), r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#.to_string()),
            ("_rels/.rels".to_string(), r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),
            ("xl/workbook.xml".to_string(), r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="One" sheetId="1" r:id="rId1"/><sheet name="Two" sheetId="2" r:id="rId2"/></sheets></workbook>"#.to_string()),
            ("xl/_rels/workbook.xml.rels".to_string(), r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#.to_string()),
            ("xl/worksheets/sheet1.xml".to_string(), sheet(260, "First", false)),
            ("xl/worksheets/sheet2.xml".to_string(), sheet(130, "Second", true)),
        ];
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in files {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    // Workbooks are typeset in runs of pages on every core, then put together: the pages
    // must come out exactly as from one document.
    #[test]
    fn workbook_typeset_in_runs_matches_one_document() {
        let wb = xlsx::read(&long_workbook()).unwrap();
        let printed = sheet_pdf::print(&wb, numfmt::Separators::US, "long.xlsx").unwrap();
        let whole = typeset::to_pdf(printed.markup(), printed.files.clone()).unwrap();
        let runs = sheet_pdf::to_pdf(&wb, numfmt::Separators::US, "long.xlsx").unwrap();

        let pdfium = crate::pdf::render::pdfium().unwrap();
        let (a, b) = (pdfium.load_pdf_from_byte_slice(&whole, None).unwrap(), pdfium.load_pdf_from_byte_slice(&runs, None).unwrap());
        assert!(a.pages().len() > 8, "{} pages: too few to be split", a.pages().len());
        assert_eq!(a.pages().len(), b.pages().len());
        let config = PdfRenderConfig::new().set_target_width(500);
        for (i, (pa, pb)) in a.pages().iter().zip(b.pages().iter()).enumerate() {
            assert_eq!((pa.width(), pa.height()), (pb.width(), pb.height()), "page {} size", i + 1);
            let (ia, ib) = (pa.render_with_config(&config).unwrap().as_rgba_bytes(), pb.render_with_config(&config).unwrap().as_rgba_bytes());
            assert!(ia == ib, "page {} differs", i + 1);
        }
        // and each font once, not once per run
        assert!(runs.len() < whole.len() * 11 / 10, "{} vs {} bytes", runs.len(), whole.len());
    }

    /// Visual check during development:
    /// `FLIPFILE_DOCX=in.docx FLIPFILE_OUT=dir cargo test review_docx -- --ignored`
    /// writes the PDF and a PNG per page into `dir`.
    #[test]
    #[ignore]
    fn review_docx() {
        let input = std::env::var("FLIPFILE_DOCX").expect("set FLIPFILE_DOCX");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        let docx = std::fs::read(&input).unwrap();
        if std::env::var("FLIPFILE_MARKUP").is_ok() {
            println!("{}", to_typst::generate(&docx::read(&docx).unwrap()).markup);
        }
        let pdf = docx_to_pdf_builtin(&docx).unwrap();
        std::fs::write(out.join("review.pdf"), &pdf).unwrap();
        // The app's shared PDFium binding: PDFium can only be bound once per process
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
        for (i, page) in doc.pages().iter().enumerate() {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(900)).unwrap().as_image().unwrap();
            img.save(out.join(format!("page{}.png", i + 1))).unwrap();
        }
        println!("pages: {}", doc.pages().len());
    }

    /// Visual check of PowerPoint -> PDF during development:
    /// `FLIPFILE_PPTX=in.pptx FLIPFILE_OUT=dir cargo test review_pptx -- --ignored`
    /// writes the PDF and a PNG per slide into `dir` (FLIPFILE_MARKUP=1 prints the markup).
    #[test]
    #[ignore]
    fn review_pptx() {
        let input = std::env::var("FLIPFILE_PPTX").expect("set FLIPFILE_PPTX");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        let bytes = std::fs::read(&input).unwrap();
        if std::env::var("FLIPFILE_MARKUP").is_ok() {
            println!("{}", slides_pdf::markup(&pptx::read(&bytes).unwrap()).0.join("\n"));
        }
        let pdf = pptx_to_pdf_builtin(&bytes).unwrap();
        std::fs::write(out.join("review.pdf"), &pdf).unwrap();
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
        for (i, page) in doc.pages().iter().enumerate().take(12) {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(1100)).unwrap().as_image().unwrap();
            img.save(out.join(format!("slide{}.png", i + 1))).unwrap();
        }
        println!("slides: {}", doc.pages().len());
    }

    /// Visual check of Excel -> PDF during development:
    /// `FLIPFILE_XLSX=in.xlsx FLIPFILE_OUT=dir cargo test review_xlsx -- --ignored --nocapture`
    /// writes the PDF and a PNG per page into `dir` (FLIPFILE_MARKUP=1 prints the markup,
    /// FLIPFILE_SEP=, uses 1.234,56 separators).
    #[test]
    #[ignore]
    fn review_xlsx() {
        let input = std::env::var("FLIPFILE_XLSX").expect("set FLIPFILE_XLSX");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        let bytes = std::fs::read(&input).unwrap();
        let csv = input.to_lowercase().ends_with(".csv");
        let wb = if csv { xlsx::read_csv(&bytes, "review.csv").unwrap() } else { xlsx::read(&bytes).unwrap() };
        let sep = if std::env::var("FLIPFILE_SEP").as_deref() == Ok(",") { numfmt::Separators::COMMA } else { numfmt::Separators::US };
        let printed = sheet_pdf::print(&wb, sep, "review.xlsx").unwrap();
        if std::env::var("FLIPFILE_MARKUP").is_ok() {
            println!("{}", printed.markup());
        }
        let start = std::time::Instant::now();
        let pdf = typeset::to_pdf(printed.markup(), printed.files).unwrap();
        println!("typeset in {:?}", start.elapsed());
        std::fs::write(out.join("review.pdf"), &pdf).unwrap();
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
        for (i, page) in doc.pages().iter().enumerate().take(12) {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(1100)).unwrap().as_image().unwrap();
            img.save(out.join(format!("page{}.png", i + 1))).unwrap();
        }
        println!("sheets: {}, pages: {}", wb.sheets.len(), doc.pages().len());
    }

    /// Renders hand-written Typst markup (with the engine's preamble) to PNGs:
    /// `FLIPFILE_TYP=test.typ FLIPFILE_OUT=dir cargo test review_markup -- --ignored`
    #[test]
    #[ignore]
    fn review_markup() {
        let input = std::env::var("FLIPFILE_TYP").expect("set FLIPFILE_TYP");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        let markup = format!("{}{}", to_typst::PREAMBLE, std::fs::read_to_string(&input).unwrap());
        let pdf = typeset::to_pdf(markup, vec![]).unwrap();
        std::fs::write(out.join("typ.pdf"), &pdf).unwrap();
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
        for (i, page) in doc.pages().iter().enumerate().take(4) {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(900)).unwrap().as_image().unwrap();
            img.save(out.join(format!("typ{}.png", i + 1))).unwrap();
        }
    }

    /// Visual check of PDF -> Word during development:
    /// `FLIPFILE_PDF=in.pdf FLIPFILE_OUT=dir cargo test review_pdf -- --ignored`
    /// writes review.docx, and renders it back through the Word -> PDF engine as PNGs.
    #[test]
    #[ignore]
    fn review_pdf() {
        let input = std::env::var("FLIPFILE_PDF").expect("set FLIPFILE_PDF");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        // The app's shared PDFium binding: PDFium can only be bound once per process
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let start = std::time::Instant::now();
        let docx = from_pdf::pdf_to_docx(&pdfium, &std::fs::read(&input).unwrap(), &|_, _| {}).unwrap();
        println!("pdf -> docx: {:.2}s, {} bytes", start.elapsed().as_secs_f64(), docx.len());
        std::fs::write(out.join("review.docx"), &docx).unwrap();
        let pdf = docx_to_pdf_builtin(&docx).unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&pdf, None).unwrap();
        for (i, page) in doc.pages().iter().enumerate().take(4) {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(900)).unwrap().as_image().unwrap();
            img.save(out.join(format!("back{}.png", i + 1))).unwrap();
        }
        let original = pdfium.load_pdf_from_file(&input, None).unwrap();
        for (i, page) in original.pages().iter().enumerate().take(4) {
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(900)).unwrap().as_image().unwrap();
            img.save(out.join(format!("orig{}.png", i + 1))).unwrap();
        }
        println!("pages: original {} -> round trip {}", original.pages().len(), doc.pages().len());
    }

    /// Where two PDFs drift apart: prints the start of every page's text side by side.
    /// `FLIPFILE_PDF=ours.pdf FLIPFILE_REF=word.pdf cargo test compare_pages -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn compare_pages() {
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let heads = |var: &str| -> Vec<String> {
            let doc = pdfium.load_pdf_from_file(&std::env::var(var).unwrap(), None).unwrap();
            doc.pages()
                .iter()
                .map(|p| {
                    let text = p.text().map(|t| t.all()).unwrap_or_default();
                    let words: Vec<&str> = text.split_whitespace().collect();
                    // Skip the running header (same on every page): take words 10..
                    let skip: usize = std::env::var("FLIPFILE_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
                    let pick: Vec<&str> = if std::env::var("FLIPFILE_TAIL").is_ok() { words.iter().rev().take(6).rev().cloned().collect() } else { words.iter().skip(skip).take(7).cloned().collect() };
                    pick.join(" ")
                })
                .collect()
        };
        // FLIPFILE_MAP=1: for every page of the reference, where its most distinctive
        // line ended up in ours (shows exactly where pages start to drift)
        if std::env::var("FLIPFILE_MAP").is_ok() {
            let raw = |var: &str| -> Vec<String> {
                let doc = pdfium.load_pdf_from_file(&std::env::var(var).unwrap(), None).unwrap();
                doc.pages().iter().map(|p| p.text().map(|t| t.all()).unwrap_or_default()).collect()
            };
            let squash = |t: &str| t.chars().filter(|c| c.is_alphanumeric()).collect::<String>();
            let (ours, reference) = (raw("FLIPFILE_PDF"), raw("FLIPFILE_REF"));
            let ours_flat: Vec<String> = ours.iter().map(|t| squash(t)).collect();
            let mut last = 0usize;
            for (i, page) in reference.iter().enumerate() {
                let mut lines: Vec<&str> = page.lines().filter(|l| l.trim().len() > 20 && !l.contains("Halaman") && !l.contains("Lampiran")).collect();
                lines.sort_by_key(|l| std::cmp::Reverse(l.len()));
                let found = lines.iter().take(3).find_map(|l| {
                    let needle = squash(l);
                    let hits: Vec<usize> = ours_flat.iter().enumerate().filter(|(_, t)| t.contains(&needle)).map(|(k, _)| k + 1).collect();
                    hits.iter().copied().find(|&k| k >= last).or(hits.first().copied())
                });
                if let Some(k) = found {
                    last = k;
                }
                println!("map {:>3} -> {:>5?} {}", i + 1, found, if found.is_some_and(|k| k != i + 1) { "<<" } else { "" });
            }
            return;
        }
        // FLIPFILE_FIND="a|b": the page of each needle in both PDFs instead
        if let Ok(needles) = std::env::var("FLIPFILE_FIND") {
            let texts = |var: &str| -> Vec<String> {
                let doc = pdfium.load_pdf_from_file(&std::env::var(var).unwrap(), None).unwrap();
                doc.pages().iter().map(|p| p.text().map(|t| t.all().split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default()).collect()
            };
            let (ours, reference) = (texts("FLIPFILE_PDF"), texts("FLIPFILE_REF"));
            let find = |pages: &[String], needle: &str| pages.iter().skip(3).position(|t| t.contains(needle)).map(|i| i + 4);
            // FLIPFILE_Y=1: also where each needle's first character sits on its page
            let y_of = |var: &str, page: usize, needle: &str| -> Option<f32> {
                let doc = pdfium.load_pdf_from_file(&std::env::var(var).unwrap(), None).ok()?;
                let p = doc.pages().get((page - 1) as i32).ok()?;
                let text = p.text().ok()?;
                let search = text.search(needle, &PdfSearchOptions::new()).ok()?;
                let found = search.find_next()?;
                Some(p.height().value - found.first().ok()?.bounds().top().value)
            };
            for needle in needles.split('|') {
                if std::env::var("FLIPFILE_Y").is_ok() {
                    let (a, b) = (find(&ours, needle), find(&reference, needle));
                    let ya = a.and_then(|p| y_of("FLIPFILE_PDF", p, needle));
                    let yb = b.and_then(|p| y_of("FLIPFILE_REF", p, needle));
                    println!("{a:>5?} {ya:>8.1?}   {b:>5?} {yb:>8.1?}  {needle}");
                    continue;
                }
                println!("{:>5?} {:>5?}  {needle}", find(&ours, needle), find(&reference, needle));
            }
            return;
        }
        let (ours, reference) = (heads("FLIPFILE_PDF"), heads("FLIPFILE_REF"));
        for i in 0..ours.len().max(reference.len()) {
            println!("{:>3} | {:<60} | {}", i + 1, ours.get(i).map(String::as_str).unwrap_or(""), reference.get(i).map(String::as_str).unwrap_or(""));
        }
    }

    /// Renders the first pages of any PDF: `FLIPFILE_PDF=in.pdf FLIPFILE_OUT=dir FLIPFILE_PREFIX=ref cargo test render_pdf_pages -- --ignored`
    #[test]
    #[ignore]
    fn render_pdf_pages() {
        let input = std::env::var("FLIPFILE_PDF").expect("set FLIPFILE_PDF");
        let out = std::path::PathBuf::from(std::env::var("FLIPFILE_OUT").expect("set FLIPFILE_OUT"));
        let prefix = std::env::var("FLIPFILE_PREFIX").unwrap_or_else(|_| "page".into());
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let doc = pdfium.load_pdf_from_file(&input, None).unwrap();
        let from: usize = std::env::var("FLIPFILE_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        for (i, page) in doc.pages().iter().enumerate().skip(from - 1).take(8) {
            let width: i32 = std::env::var("FLIPFILE_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(800);
            let img = page.render_with_config(&PdfRenderConfig::new().set_target_width(width)).unwrap().as_image().unwrap();
            img.save(out.join(format!("{prefix}{}.png", i + 1))).unwrap();
        }
        println!("pages: {}", doc.pages().len());
    }

    #[test]
    fn deeply_nested_xml_is_refused_before_parsing() {
        let nest = |n: usize, open: &str, close: &str| format!("<?xml version=\"1.0\"?><r>{}{}</r>", open.repeat(n), close.repeat(n));
        assert!(!xml_too_deep(&nest(200, "<a x=\"1\">", "</a>")));
        assert!(xml_too_deep(&nest(3000, "<p:grpSp>", "</p:grpSp>")));
        // Empty elements, comments and CDATA open nothing
        assert!(!xml_too_deep(&nest(3000, "<a/><!-- <b> --><![CDATA[<c>]]>", "")));
        // A quoted "/>" or ">" does not end the tag early
        assert!(xml_too_deep(&nest(3000, "<a v=\"/>\" w='>'>", "</a>")));
    }
}
