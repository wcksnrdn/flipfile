// Word documents in the pipeline: Word -> PDF / images, and OCR of scans and photos
// into an editable Word document.

use std::fs;
use std::path::Path;

use super::input::{file_name, open_oriented};
use super::output::{save_err, unique_output_path};
use super::pdf::pdf_to_images;
use super::{Converted, OnPage};
use crate::office;
use crate::pdf::render as pdf_render;
use crate::state::settings::Options;

/// Word document -> PDF (best engine on this computer), or -> images through a PDF.
pub(super) fn from_word(
    input: &Path,
    legacy: bool,
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let name = file_name(input);
    match format {
        "docx" | "docx-ocr" => Err(format!("\"{name}\" is already a Word document")),
        "pdf" => {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            let engine = office::apps::word_to_pdf(input, &output, legacy).map_err(|e| format!("\"{name}\": {e}"))?;
            // Shown in the status, so it is clear which engine produced the PDF
            let note = match engine {
                office::apps::Engine::Word => "via Word",
                office::apps::Engine::LibreOffice => "via LibreOffice",
                office::apps::Engine::BuiltIn => "built-in engine",
            };
            Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some(note.into()) })
        }
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "tiff" | "gif" | "ico" | "avif" => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let temp = std::env::temp_dir().join(format!("flipfile-word-{}-{nanos}.pdf", std::process::id()));
            let result = office::apps::word_to_pdf(input, &temp, legacy)
                .map_err(|e| format!("\"{name}\": {e}"))
                .and_then(|_| pdf_to_images(&temp, input, format, out_dir, options, on_page));
            let _ = fs::remove_file(&temp);
            result
        }
        _ => Err(format!("\"{name}\": pick PDF or an image format for Word documents")),
    }
}

/// Scanned PDF or photo of a document -> editable Word document, read by the system's
/// text recognition (macOS Vision / Windows OCR)
pub(super) fn ocr_to_word(input: &Path, is_pdf: bool, out_dir: Option<&Path>, on_page: OnPage) -> Result<Converted, String> {
    let name = file_name(input);
    let mut pages = vec![];
    if is_pdf {
        let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        let pdfium = pdf_render::pdfium()?;
        let document = pdfium.load_pdf_from_byte_slice(&data, None).map_err(|_| format!("\"{name}\": the PDF could not be opened"))?;
        let total = document.pages().len() as usize;
        for (index, page) in document.pages().iter().enumerate() {
            on_page(index, total);
            let (w, h) = (page.width().value, page.height().value);
            // 300 dpi reads small print well; very large pages are capped
            let scale = (300.0 / 72.0f32).min(3500.0 / w.max(h).max(1.0));
            let config = pdfium_render::prelude::PdfRenderConfig::new().scale_page_by_factor(scale);
            let image = page.render_with_config(&config).map_err(|e| format!("\"{name}\": {e}"))?.as_image().map_err(|e| format!("\"{name}\": {e}"))?;
            let scan = image.to_rgba8();
            let lines = office::ocr::recognize(&scan).map_err(|e| format!("\"{name}\": {e}"))?;
            pages.push(office::from_pdf::OcrPage { width: w, height: h, lines, scan: Some(scan) });
        }
        on_page(total, total);
    } else {
        let (img, _) = open_oriented(input)?;
        on_page(0, 1);
        let scan = img.to_rgba8();
        let lines = office::ocr::recognize(&scan).map_err(|e| format!("\"{name}\": {e}"))?;
        // A photo becomes a page as wide as A4, in the photo's proportions
        let width = 595.3;
        let height = width * img.height() as f32 / img.width().max(1) as f32;
        pages.push(office::from_pdf::OcrPage { width, height, lines, scan: Some(scan) });
        on_page(1, 1);
    }
    let docx = office::from_pdf::ocr_to_docx(pages).map_err(|e| format!("\"{name}\": {e}"))?;
    let output = unique_output_path(input, out_dir, "converted", "docx");
    fs::write(&output, docx).map_err(save_err)?;
    Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some("text recognised".into()) })
}
