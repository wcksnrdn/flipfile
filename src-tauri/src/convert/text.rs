// The readable text of a dropped file, for AI summaries (src/summary): what a person
// would read, as Markdown (headings, list items, table rows) that the model follows well.
// Documents are read by the same engines as the conversions; scanned pages and photos go
// through the system's text recognition (office/ocr.rs), recordings and videos through
// speech recognition (convert/transcribe).

use std::fs;
use std::path::Path;

use super::input::{file_name, header, open_oriented, sheet_kind, word_kind};
use super::office_extra::{blocks_to_markdown, ext_lower, is_extra_office_input, read_any_blocks, DocBlock};
use super::slides::slides_kind;
use super::OnPage;
use crate::office;
use crate::office::apps::{Engine, SheetKind};
use crate::pdf::render as pdf_render;

/// What a file says, ready for the model
#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    /// Pages of a PDF or scan; 0 when the format has no fixed pages (Word, web pages)
    pub pages: usize,
    pub words: usize,
    /// "PDF", "Word document", ...: shown in the panel and given to the model as context
    pub kind: &'static str,
    /// Read by text recognition (a scan or a photo)
    pub ocr: bool,
}

/// Longer texts are cut here: the summary condenses a book to a few thousand words anyway
const MAX_CHARS: usize = 1_500_000;
/// A PDF page with less text than this is read by text recognition (a scan)
const SCANNED_PAGE_CHARS: usize = 40;
/// Scanned pages read at most (text recognition takes about half a second a page)
const MAX_OCR_PAGES: usize = 60;
/// Table rows kept per table: the model needs the shape of the data, not every row
const MAX_ROWS: usize = 120;

pub fn extract(path: &Path, on_page: OnPage) -> Result<Text, String> {
    let name = file_name(path);
    let (text, pages, kind, ocr) = if pdf_render::is_pdf(&header(path)) {
        let data = fs::read(path).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        let (text, pages, ocr) = pdf_text(&data, name, on_page)?;
        (text, pages, "PDF", ocr)
    } else if let Some(legacy) = word_kind(path) {
        if legacy {
            via_pdf(path, "Word document", on_page, |out| office::apps::word_to_pdf(path, out, true))?
        } else {
            (document_markdown(path)?, 0, "Word document", false)
        }
    } else if let Some(legacy) = slides_kind(path) {
        // .odp reads like .pptx; only the binary .ppt family needs PowerPoint or LibreOffice
        match ext_lower(path).as_str() {
            "ppt" | "pps" | "pot" => via_pdf(path, "Presentation", on_page, |out| office::apps::presentation_to_pdf(path, out, legacy))?,
            _ => (document_markdown(path)?, 0, "Presentation", false),
        }
    } else if let Some(kind) = sheet_kind(path) {
        if kind == SheetKind::Other {
            via_pdf(path, "Spreadsheet", on_page, |out| office::apps::sheet_to_pdf(path, out, kind))?
        } else {
            (document_markdown(path)?, 0, "Spreadsheet", false)
        }
    } else if is_extra_office_input(path) {
        let kind = match ext_lower(path).as_str() {
            "md" | "markdown" => "Markdown document",
            "html" | "htm" => "Web page",
            "epub" => "E-book",
            "json" | "yaml" | "yml" | "xml" | "tsv" | "tab" => "Data file",
            "txt" | "typ" | "tex" => "Text document",
            _ => "Document",
        };
        (document_markdown(path)?, 0, kind, false)
    } else if matches!(ext_lower(path).as_str(), "text" | "log") {
        let bytes = fs::read(path).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        (String::from_utf8_lossy(&bytes).into_owned(), 0, "Text document", false)
    } else if super::transcribe::is_recording(path) {
        // Spoken, not written: Whisper writes it down first (progress in percent)
        let (text, _language) = super::transcribe::transcript_text(path, on_page)?;
        (text, 0, "Recording", false)
    } else if let Ok((image, _)) = open_oriented(path) {
        on_page(0, 1);
        let text = recognize(&image.to_rgba8()).map_err(|e| format!("\"{name}\": {e}"))?;
        on_page(1, 1);
        (text, 1, "Image", true)
    } else {
        return Err(format!("\"{name}\": summaries read documents, PDFs, images and recordings"));
    };

    let mut text = text.trim().to_string();
    if text.is_empty() {
        return Err(format!("\"{name}\" has no text to read"));
    }
    if text.len() > MAX_CHARS {
        let mut end = MAX_CHARS;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let words = text.split_whitespace().count();
    Ok(Text { text, pages, words, kind, ocr })
}

/// Any document the conversions can read into blocks, as Markdown
fn document_markdown(path: &Path) -> Result<String, String> {
    let blocks: Vec<DocBlock> = read_any_blocks(path)?
        .into_iter()
        .map(|block| match block {
            DocBlock::Table(mut rows) if rows.len() > MAX_ROWS => {
                let more = rows.len() - MAX_ROWS;
                rows.truncate(MAX_ROWS);
                rows.push(vec![format!("… {more} more rows")]);
                DocBlock::Table(rows)
            }
            block => block,
        })
        .collect();
    Ok(blocks_to_markdown(&blocks))
}

/// A format only Word, PowerPoint, Excel or LibreOffice can open: printed to a
/// temporary PDF by that app, then read like any PDF
fn via_pdf(
    path: &Path,
    kind: &'static str,
    on_page: OnPage,
    print: impl FnOnce(&Path) -> Result<Engine, String>,
) -> Result<(String, usize, &'static str, bool), String> {
    let name = file_name(path);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let temp = std::env::temp_dir().join(format!("flipfile-summary-{}-{nanos}.pdf", std::process::id()));
    let result = print(&temp)
        .map_err(|e| format!("\"{name}\": {e}"))
        .and_then(|_| fs::read(&temp).map_err(|e| format!("\"{name}\" could not be read: {e}")))
        .and_then(|data| pdf_text(&data, name, on_page));
    let _ = fs::remove_file(&temp);
    let (text, pages, ocr) = result?;
    Ok((text, pages, kind, ocr))
}

/// The text of every page. A scan (hardly any text on its pages) is read by text
/// recognition instead; in a PDF with a text layer, only pages without any text are
/// (a scanned appendix), so short pages such as titles never slow the reading down.
/// Returns the text, the page count and whether text recognition was used.
fn pdf_text(data: &[u8], name: &str, on_page: OnPage) -> Result<(String, usize, bool), String> {
    let pdfium = pdf_render::pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(data, None)
        .map_err(|_| format!("\"{name}\": the PDF could not be opened"))?;
    let total = document.pages().len() as usize;
    let mut pages: Vec<String> = Vec::with_capacity(total);
    let mut length = 0;
    for page in document.pages().iter() {
        let text = paragraphs(&page.text().map(|t| t.all()).unwrap_or_default());
        length += text.len();
        pages.push(text);
        if length > MAX_CHARS {
            break;
        }
    }

    let visible = |text: &str| text.chars().filter(|c| !c.is_whitespace()).count();
    let scan = pages.iter().map(|p| visible(p)).sum::<usize>() < SCANNED_PAGE_CHARS * pages.len().max(1);
    let unread: Vec<usize> = (0..pages.len())
        .filter(|&i| if scan { visible(&pages[i]) < SCANNED_PAGE_CHARS } else { visible(&pages[i]) == 0 })
        .take(if scan { MAX_OCR_PAGES } else { MAX_OCR_PAGES / 6 })
        .collect();
    let mut recognized = false;
    for (done, &index) in unread.iter().enumerate() {
        on_page(done, unread.len());
        let Ok(page) = document.pages().get(index as i32) else { continue };
        let (w, h) = (page.width().value, page.height().value);
        // 200 dpi reads body text well and keeps text recognition quick
        let scale = (200.0 / 72.0f32).min(2500.0 / w.max(h).max(1.0));
        let config = pdfium_render::prelude::PdfRenderConfig::new().scale_page_by_factor(scale);
        let image = page.render_with_config(&config).map_err(|e| format!("\"{name}\": {e}"))?.as_image().map_err(|e| format!("\"{name}\": {e}"))?;
        match recognize(&image.to_rgba8()) {
            Ok(read) if visible(&read) > visible(&pages[index]) => {
                pages[index] = read;
                recognized = true;
            }
            Ok(_) => {}
            // No text recognition on this system (Linux): the text layer is all there is
            Err(e) if visible(&pages.concat()) == 0 => return Err(format!("\"{name}\" is a scan, and {e}")),
            Err(_) => break,
        }
    }
    if !unread.is_empty() {
        on_page(unread.len(), unread.len());
    }

    let text = pages.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n");
    Ok((text, total, recognized))
}

/// PDF text comes line by line: lines are joined into paragraphs (a blank line between
/// paragraphs). A line ending in a dash continues on the next one: a word broken in two
/// loses the dash ("inter-" "national"), a compound or range keeps it ("Jakarta-Bandung").
fn paragraphs(text: &str) -> String {
    let mut out = String::new();
    let mut line_open = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            if line_open {
                out.push_str("\n\n");
                line_open = false;
            }
            continue;
        }
        if line_open {
            if out.ends_with('-') {
                if line.starts_with(|c: char| c.is_lowercase()) {
                    out.pop();
                }
            } else {
                out.push(' ');
            }
        }
        out.push_str(line);
        line_open = true;
    }
    out
}

/// Text recognition of a scan or photo, line by line in reading order
fn recognize(image: &image::RgbaImage) -> Result<String, String> {
    let mut lines = office::ocr::recognize(image)?;
    lines.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]).then(a.rect[0].total_cmp(&b.rect[0])));
    let mut out = String::new();
    let mut last_bottom: Option<f32> = None;
    for line in &lines {
        if let Some(bottom) = last_bottom {
            // A gap taller than a line starts a new paragraph
            let height = line.rect[3] - line.rect[1];
            out.push_str(if line.rect[1] - bottom > height { "\n\n" } else { "\n" });
        }
        out.push_str(line.text.trim());
        last_bottom = Some(line.rect[3]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_lines_become_paragraphs() {
        let text = "The quick brown\nfox jumps over the inter-\nnational dog.\n\nSecond para-\ngraph here.\n";
        assert_eq!(paragraphs(text), "The quick brown fox jumps over the international dog.\n\nSecond paragraph here.");
        // A dash before a capital or a number is kept (names, ranges)
        assert_eq!(paragraphs("Jakarta-\nBandung 2025-\n2026"), "Jakarta-Bandung 2025-2026");
    }

    #[test]
    fn documents_are_read_as_markdown() {
        let dir = std::env::temp_dir().join(format!("flipfile-text-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let md = dir.join("notes.md");
        fs::write(&md, "# Launch plan\n\nShip on **15 December**.\n\n- Budget: Rp 250 juta\n- Owner: Sari\n").unwrap();
        let read = extract(&md, &|_, _| {}).unwrap();
        assert_eq!(read.kind, "Markdown document");
        assert!(read.text.contains("# Launch plan"), "{}", read.text);
        assert!(read.text.contains("- Budget: Rp 250 juta"), "{}", read.text);
        assert_eq!(read.words, read.text.split_whitespace().count());

        let empty = dir.join("empty.txt");
        fs::write(&empty, "  \n\n ").unwrap();
        assert!(extract(&empty, &|_, _| {}).unwrap_err().contains("no text"));

        // A recording is transcribed first: a broken one fails as a recording
        let audio = dir.join("song.mp3");
        fs::write(&audio, b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap();
        let err = extract(&audio, &|_, _| {}).unwrap_err();
        assert!(err.contains("song.mp3") && !err.contains("summaries read"), "{err}");
        // Neither a document, an image nor a recording
        let other = dir.join("data.bin");
        fs::write(&other, [0u8, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        assert!(extract(&other, &|_, _| {}).unwrap_err().contains("summaries read"));
        let _ = fs::remove_dir_all(&dir);
    }
}
