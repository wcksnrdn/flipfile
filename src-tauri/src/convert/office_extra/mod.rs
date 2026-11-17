// Extended Office, Document, E-book, Presentation & Developer Data conversions:
// - Document formats: PDF, DOCX, ODT, RTF, MD, TXT, HTML, EPUB, PPTX/ODP
// - Spreadsheet & Data formats: XLSX, CSV, TSV, JSON, YAML, XML, SQL, MD-Table

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::input::file_name;
use super::output::{save_err, unique_output_path};
use super::pdf::pdf_to_images;
use super::{Converted, OnPage};
use crate::office::numfmt::{self, Separators, Value};
use crate::office::xlsx::{self, Cell, Sheet, Style, Workbook};
use crate::office::{docx, typeset};
use crate::pdf::render as pdf_render;
use crate::state::settings::Options;

// Each direction has its own module; they share one namespace (each starts with
// `use super::*`), as when this was a single file.
mod read_docs;
mod read_tables;
mod write_docs;
mod write_tables;

use write_docs::*;
use write_tables::*;

// What the rest of `convert` uses: documents and tables read from any input, and
// documents written as Markdown, Word or PDF
pub(super) use read_docs::read_any_blocks;
pub(super) use read_tables::read_any_tables;
pub(super) use write_docs::{blocks_to_docx, blocks_to_markdown, blocks_to_pdf};

/// Structured document block for cross-converting documents (MD, HTML, TXT, RTF, ODT, EPUB, DOCX, PDF, PPTX).
#[derive(Clone, Debug)]
pub enum DocBlock {
    Heading(u8, String),
    Paragraph(String),
    ListItem(String),
    CodeBlock(String),
    Table(Vec<Vec<String>>),
    PageBreak,
}

/// One tabular sheet of rows & columns for cross-converting spreadsheets and data formats.
#[derive(Clone, Debug)]
pub struct TableSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// True if `format` is an Office/Document/Data output target handled here.
pub(super) fn is_office_target(format: &str) -> bool {
    matches!(
        format,
        "xlsx" | "csv" | "tsv" | "json" | "yaml" | "xml" | "sql" | "md-table" | "md" | "txt" | "html" | "rtf" | "epub" | "odt"
    )
}

/// True if `path` is an extended office/document/data input file (MD, TXT, HTML, RTF, ODT, EPUB, PPTX, JSON, TSV, YAML, XML).
pub(super) fn is_extra_office_input(path: &Path) -> bool {
    let ext = ext_lower(path);
    matches!(
        ext.as_str(),
        "md" | "markdown"
            | "txt"
            | "html"
            | "htm"
            | "rtf"
            | "odt"
            | "epub"
            | "pptx"
            | "ppt"
            | "odp"
            | "tsv"
            | "tab"
            | "json"
            | "yaml"
            | "yml"
            | "xml"
            | "typ"
            | "tex"
    )
}

pub(super) fn ext_lower(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default()
}

pub(super) fn is_tabular_ext(ext: &str) -> bool {
    matches!(
        ext,
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xls" | "ods" | "csv" | "tsv" | "tab" | "json" | "yaml" | "yml" | "xml"
    )
}

fn is_tabular_target(format: &str) -> bool {
    matches!(format, "xlsx" | "csv" | "tsv" | "json" | "yaml" | "xml" | "sql" | "md-table")
}

/// Main entry point for converting any Office/Document/Data input or target.
pub(super) fn convert_office(
    input: &Path,
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let in_ext = ext_lower(input);
    let name = file_name(input);

    if in_ext == format || (in_ext == "markdown" && format == "md") || (in_ext == "htm" && format == "html") || (in_ext == "yml" && format == "yaml") {
        return Err(format!("\"{name}\" is already in {} format", format.to_uppercase()));
    }

    // 1a. PDF -> Excel: tables rebuilt from the page layout, with their formatting
    if is_tabular_target(format) && (in_ext == "pdf" || pdf_render::is_pdf(&super::input::header(input))) {
        let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        let pdfium = pdf_render::pdfium()?;
        let out_ext = if format == "md-table" { "md" } else { format };
        let output = unique_output_path(input, out_dir, "converted", out_ext);
        let bytes = if format == "xlsx" {
            let sheet = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Sheet1");
            crate::office::pdf_xlsx::pdf_to_xlsx(pdfium, &data, sheet, on_page).map_err(|e| format!("\"{name}\": {e}"))?
        } else {
            let rows = crate::office::pdf_xlsx::pdf_to_rows(pdfium, &data, on_page).map_err(|e| format!("\"{name}\": {e}"))?;
            let sheet = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Sheet1").to_string();
            write_tabular(&[TableSheet { name: sheet, rows }], format, input)?
        };
        fs::write(&output, bytes).map_err(save_err)?;
        return Ok(Converted::file(input, &output));
    }

    // 1b. Any tabular/data target (xlsx, csv, tsv, json, yaml, xml, sql, md-table)
    if is_tabular_target(format) {
        let sheets = read_any_tables(input)?;
        let out_ext = if format == "md-table" { "md" } else { format };
        let output = unique_output_path(input, out_dir, "converted", out_ext);
        let bytes = write_tabular(&sheets, format, input)?;
        fs::write(&output, bytes).map_err(save_err)?;
        return Ok(Converted::file(input, &output));
    }

    // 2. Tabular input (JSON, TSV, YAML, XML) -> PDF or Image
    if is_tabular_ext(&in_ext) && (format == "pdf" || is_image_target(format)) {
        let sheets = read_any_tables(input)?;
        let wb = sheets_to_workbook(&sheets);
        let pdf_bytes = crate::office::sheet_pdf::to_pdf(&wb, Separators::system(), name)?;
        if format == "pdf" {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            fs::write(&output, pdf_bytes).map_err(save_err)?;
            return Ok(Converted {
                path: output.to_string_lossy().into_owned(),
                pages: 0,
                sizes: None,
                note: Some("built-in engine".into()),
            });
        }
        return render_pdf_bytes_to_images(&pdf_bytes, input, format, out_dir, options, on_page);
    }

    // 3. Document / Presentation / Tabular -> Document target (md, txt, html, rtf, epub, odt, docx, pdf, or image)
    let blocks = read_any_blocks(input)?;
    let title = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Document");

    match format {
        "md" => {
            let output = unique_output_path(input, out_dir, "converted", "md");
            fs::write(&output, blocks_to_markdown(&blocks)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "txt" => {
            let output = unique_output_path(input, out_dir, "converted", "txt");
            fs::write(&output, blocks_to_text(&blocks)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "html" => {
            let output = unique_output_path(input, out_dir, "converted", "html");
            fs::write(&output, blocks_to_html(&blocks, title)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "rtf" => {
            let output = unique_output_path(input, out_dir, "converted", "rtf");
            fs::write(&output, blocks_to_rtf(&blocks)).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "epub" => {
            let output = unique_output_path(input, out_dir, "converted", "epub");
            let bytes = blocks_to_epub(&blocks, title)?;
            fs::write(&output, bytes).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "odt" => {
            let output = unique_output_path(input, out_dir, "converted", "odt");
            let bytes = blocks_to_odt(&blocks, title)?;
            fs::write(&output, bytes).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "docx" => {
            let output = unique_output_path(input, out_dir, "converted", "docx");
            let bytes = blocks_to_docx(&blocks)?;
            fs::write(&output, bytes).map_err(save_err)?;
            Ok(Converted::file(input, &output))
        }
        "pdf" => {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            let pdf_bytes = blocks_to_pdf(&blocks)?;
            fs::write(&output, pdf_bytes).map_err(save_err)?;
            Ok(Converted {
                path: output.to_string_lossy().into_owned(),
                pages: 0,
                sizes: None,
                note: Some("built-in engine".into()),
            })
        }
        ext if is_image_target(ext) => {
            let pdf_bytes = blocks_to_pdf(&blocks)?;
            render_pdf_bytes_to_images(&pdf_bytes, input, ext, out_dir, options, on_page)
        }
        _ => Err(format!("\"{name}\": cannot convert to {format}")),
    }
}

fn is_image_target(format: &str) -> bool {
    matches!(
        format,
        "jpg" | "jpeg" | "png" | "webp" | "heic" | "bmp" | "tiff" | "gif" | "ico" | "icns" | "avif" | "tga" | "qoi" | "exr" | "hdr" | "ppm" | "svg"
    )
}

fn render_pdf_bytes_to_images(
    pdf_bytes: &[u8],
    input: &Path,
    ext: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let temp = std::env::temp_dir().join(format!("flipfile-doc-{}-{nanos}.pdf", std::process::id()));
    fs::write(&temp, pdf_bytes).map_err(save_err)?;
    let res = pdf_to_images(&temp, input, ext, out_dir, options, on_page);
    let _ = fs::remove_file(&temp);
    res
}
