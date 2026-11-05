// Presentations in the pipeline: PowerPoint -> PDF, images, a Word handout or an
// outline (Markdown, text, HTML, …), and anything -> PowerPoint: PDF pages, photos
// (several merge into one deck), documents (headings become slides) and spreadsheets
// (tables split over slides).

use std::fs;
use std::path::Path;

use image::DynamicImage;
use tauri::ipc::Channel;

use super::encode::{encode_jpeg, looks_like_graphic, resize};
use super::input::{file_name, open_oriented};
use super::office_extra::{convert_office, ext_lower, is_tabular_ext, read_any_blocks, read_any_tables, DocBlock};
use super::output::{save_err, unique_output_path};
use super::pdf::pdf_to_images;
use super::{isolate, memory, par_map, BatchResult, Converted, OnPage, Progress};
use crate::office::pptx_writer::{Deck, Item, NewSlide, Para, EMU_PER_PT, WIDE};
use crate::office::{self, apps};
use crate::pdf::render as pdf_render;
use crate::state::settings::Options;

/// `Some(false)` for PowerPoint's own .pptx family, `Some(true)` for files only
/// PowerPoint or LibreOffice open (.ppt, .pps, .odp), from content and extension
pub(super) fn slides_kind(path: &Path) -> Option<bool> {
    let head = super::input::header(path);
    match ext_lower(path).as_str() {
        "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" if head.starts_with(b"PK\x03\x04") => Some(false),
        "ppt" | "pps" | "pot" if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) => Some(true),
        "odp" if head.starts_with(b"PK\x03\x04") => Some(true),
        _ => None,
    }
}

fn temp_path(tag: &str, ext: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("flipfile-{tag}-{}-{nanos}.{ext}", std::process::id()))
}

/// Removes a temporary file when dropped, however the conversion ends
struct Temp(std::path::PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn engine_note(engine: apps::Engine) -> &'static str {
    match engine {
        apps::Engine::LibreOffice => "via LibreOffice",
        apps::Engine::BuiltIn => "built-in engine",
        _ => "via PowerPoint",
    }
}

const IMAGE_TARGETS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "bmp", "tiff", "gif", "ico", "icns", "avif", "tga", "qoi", "exr", "hdr", "ppm", "svg"];

/// A presentation -> PDF, images (one per slide), a Word handout or an outline
pub(super) fn from_slides(
    input: &Path,
    legacy: bool,
    format: &str,
    out_dir: Option<&Path>,
    options: &Options,
    on_page: OnPage,
) -> Result<Converted, String> {
    let name = file_name(input);
    match format {
        "pdf" => {
            let output = unique_output_path(input, out_dir, "converted", "pdf");
            let engine = apps::presentation_to_pdf(input, &output, legacy).map_err(|e| format!("\"{name}\": {e}"))?;
            Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some(engine_note(engine).into()) })
        }
        f if IMAGE_TARGETS.contains(&f) => {
            let temp = Temp(temp_path("slides", "pdf"));
            apps::presentation_to_pdf(input, &temp.0, legacy).map_err(|e| format!("\"{name}\": {e}"))?;
            pdf_to_images(&temp.0, input, format, out_dir, options, on_page)
        }
        "docx" | "docx-ocr" => handout(input, legacy, out_dir, on_page),
        "pptx" if !legacy => Err(format!("\"{name}\" is already a PowerPoint presentation")),
        "pptx" => {
            let output = unique_output_path(input, out_dir, "converted", "pptx");
            apps::presentation_to_pptx(input, &output).map_err(|e| format!("\"{name}\": {e}"))?;
            Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some("via LibreOffice".into()) })
        }
        // Outlines: .odp is read directly; .ppt goes through a .pptx first
        _ if legacy && ext_lower(input) != "odp" => {
            let temp = Temp(temp_path("slides", "pptx"));
            apps::presentation_to_pptx(input, &temp.0).map_err(|e| format!("\"{name}\": {e}"))?;
            let mut converted = convert_office(&temp.0, format, out_dir, options, on_page)?;
            // Named after the original, not the temporary copy
            let wanted = unique_output_path(input, out_dir, "converted", Path::new(&converted.path).extension().and_then(|e| e.to_str()).unwrap_or(format));
            fs::rename(&converted.path, &wanted).or_else(|_| fs::copy(&converted.path, &wanted).map(|_| ())).map_err(save_err)?;
            converted.path = wanted.to_string_lossy().into_owned();
            Ok(converted)
        }
        _ => convert_office(input, format, out_dir, options, on_page),
    }
}

/// A picture for a slide or handout: PNG keeps text and graphics sharp, but only
/// when it is not much larger than a JPEG (gradients and photos are many times larger)
fn picture_bytes(img: &DynamicImage) -> Result<(Vec<u8>, bool), String> {
    let mut jpeg = Vec::new();
    encode_jpeg(img, 88, &mut jpeg)?;
    if looks_like_graphic(img) {
        let mut png = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new_with_quality(&mut png, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive);
        let rgb = img.to_rgb8();
        image::ImageEncoder::write_image(encoder, rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
        if png.len() <= jpeg.len() * 3 / 2 {
            return Ok((png, true));
        }
    }
    Ok((jpeg, false))
}

/// Every page of a PDF as a picture no wider than `max_side`, handed over one by one
fn pdf_pictures(pdf: &Path, max_side: u32, on_page: OnPage, mut each: impl FnMut(usize, DynamicImage) -> Result<(), String>) -> Result<usize, String> {
    let total = pdf_render::page_info(pdf).map(|i| i.pages).unwrap_or(0);
    pdf_render::render_pages(pdf, |i, img| {
        on_page(i, total);
        let img = if img.width().max(img.height()) > max_side { img.resize(max_side, max_side, image::imageops::FilterType::Lanczos3) } else { img };
        each(i, img)
    })
    .inspect(|&n| on_page(n, n))
}

/// A presentation -> Word: a page per slide with its title, picture and notes
fn handout(input: &Path, legacy: bool, out_dir: Option<&Path>, on_page: OnPage) -> Result<Converted, String> {
    let name = file_name(input);
    let temp = Temp(temp_path("handout", "pdf"));
    let engine = apps::presentation_to_pdf(input, &temp.0, legacy).map_err(|e| format!("\"{name}\": {e}"))?;
    // Titles and notes come from the file itself (the slides the PDF shows)
    let (titles, notes): (Vec<String>, Vec<String>) = if legacy {
        (vec![], vec![])
    } else {
        let bytes = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
        office::pptx::read(&bytes)
            .map(|deck| office::slides_pdf::shown(&deck).iter().map(|s| (s.title.clone(), s.notes.clone())).unzip())
            .unwrap_or_default()
    };
    let mut slides = vec![];
    pdf_pictures(&temp.0, 1400, on_page, |i, img| {
        let (picture, png) = picture_bytes(&img)?;
        slides.push(office::handout::HandoutSlide {
            title: titles.get(i).cloned().unwrap_or_default(),
            picture,
            png,
            width: img.width(),
            height: img.height(),
            notes: notes.get(i).cloned().unwrap_or_default(),
        });
        Ok(())
    })
    .map_err(|e| format!("\"{name}\": {e}"))?;
    let docx = office::handout::write(&slides).map_err(|e| format!("\"{name}\": {e}"))?;
    let output = unique_output_path(input, out_dir, "converted", "docx");
    fs::write(&output, docx).map_err(save_err)?;
    Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some(engine_note(engine).into()) })
}

// ---------------------------------------------------------------------------
// Anything -> PowerPoint

/// Slide sizes PowerPoint accepts: 1 to 56 inches a side
fn slide_size(width_pt: f32, height_pt: f32) -> (i64, i64) {
    let (w, h) = (width_pt.max(1.0) * EMU_PER_PT, height_pt.max(1.0) * EMU_PER_PT);
    let scale = (51_206_400.0 / w.max(h)).min(1.0).max(914_400.0 / w.min(h));
    ((w * scale).round() as i64, (h * scale).round() as i64)
}

/// A picture centred on a slide, as large as fits
fn fitted(width: i64, height: i64, img_w: u32, img_h: u32) -> (i64, i64, i64, i64) {
    let scale = (width as f64 / img_w.max(1) as f64).min(height as f64 / img_h.max(1) as f64);
    let (w, h) = ((img_w as f64 * scale) as i64, (img_h as f64 * scale) as i64);
    ((width - w) / 2, (height - h) / 2, w, h)
}

fn save_deck(deck: &Deck, input: &Path, out_dir: Option<&Path>, suffix: &str) -> Result<std::path::PathBuf, String> {
    let bytes = office::pptx_writer::write(deck).map_err(|e| format!("\"{}\": {e}", file_name(input)))?;
    let output = unique_output_path(input, out_dir, suffix, "pptx");
    fs::write(&output, bytes).map_err(save_err)?;
    Ok(output)
}

fn title_of(input: &Path) -> String {
    input.file_stem().and_then(|s| s.to_str()).unwrap_or("Presentation").to_string()
}

/// One file -> a PowerPoint presentation
pub(super) fn to_pptx(input: &Path, out_dir: Option<&Path>, options: &Options, on_page: OnPage) -> Result<Converted, String> {
    let name = file_name(input);
    let done = |path: std::path::PathBuf| Converted { path: path.to_string_lossy().into_owned(), pages: 0, sizes: None, note: None };
    if pdf_render::is_pdf(&super::input::header(input)) {
        return pdf_to_pptx(input, out_dir, on_page).map(done);
    }
    let ext = ext_lower(input);
    if is_tabular_ext(&ext) {
        let sheets = read_any_tables(input)?;
        // A CSV's one table is named after the file: "data", not "data.csv"
        let file = file_name(input);
        let tables = sheets.into_iter().map(|s| (if s.name == file { title_of(input) } else { s.name }, s.rows)).collect();
        let deck = tables_deck(&title_of(input), tables);
        return save_deck(&deck, input, out_dir, "converted").map(done);
    }
    if super::word_kind(input) == Some(true) {
        return Err(format!("\"{name}\": old .doc files can't become slides; save it as .docx first"));
    }
    if super::word_kind(input).is_some() || super::office_extra::is_extra_office_input(input) {
        let blocks = read_any_blocks(input)?;
        let deck = blocks_deck(&title_of(input), &blocks);
        return save_deck(&deck, input, out_dir, "converted").map(done);
    }
    // A picture: a slide of its own
    let slide = image_slide(input, options)?;
    let deck = Deck { width: WIDE.0, height: WIDE.1, title: title_of(input), slides: vec![slide] };
    save_deck(&deck, input, out_dir, "converted").map(done)
}

/// A PDF -> a slide per page, the page as a picture filling it
fn pdf_to_pptx(input: &Path, out_dir: Option<&Path>, on_page: OnPage) -> Result<std::path::PathBuf, String> {
    let name = file_name(input);
    let mut slides = vec![];
    let mut size = None;
    pdf_pictures(input, 2000, on_page, |i, img| {
        // The first page sets the slide size, from its size at 200 dpi
        let (width, height) = *size.get_or_insert_with(|| slide_size(img.width() as f32 * 72.0 / 200.0, img.height() as f32 * 72.0 / 200.0));
        let (data, png) = picture_bytes(&img)?;
        let (x, y, w, h) = fitted(width, height, img.width(), img.height());
        slides.push(NewSlide {
            background: Some([255, 255, 255]),
            items: vec![Item::Picture { x, y, w, h, data, ext: if png { "png" } else { "jpeg" }, name: format!("Page {}", i + 1) }],
        });
        Ok(())
    })
    .map_err(|e| format!("\"{name}\": {e}"))?;
    let (width, height) = size.unwrap_or(WIDE);
    save_deck(&Deck { width, height, title: title_of(input), slides }, input, out_dir, "converted")
}

/// A picture on a black 16:9 slide, as large as fits. JPEG and PNG files are used as
/// they are (no quality lost), unless they must be turned upright or resized.
fn image_slide(input: &Path, options: &Options) -> Result<NewSlide, String> {
    let (img, orientation) = open_oriented(input)?;
    let ext = ext_lower(input);
    let keep = orientation == image::metadata::Orientation::NoTransforms
        && options.resize == crate::state::settings::Resize::Original
        && matches!(ext.as_str(), "jpg" | "jpeg" | "png")
        && super::input::header(input).starts_with(if ext == "png" { b"\x89PNG".as_slice() } else { b"\xFF\xD8".as_slice() });
    let img = resize(img, options.resize);
    let (data, file_ext) = if keep {
        (fs::read(input).map_err(|e| format!("\"{}\" could not be read: {e}", file_name(input)))?, if ext == "png" { "png" } else { "jpeg" })
    } else if img.color().has_alpha() {
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        (out, "png")
    } else {
        let mut out = Vec::new();
        encode_jpeg(&img, options.quality.max(85), &mut out)?;
        (out, "jpeg")
    };
    let (x, y, w, h) = fitted(WIDE.0, WIDE.1, img.width(), img.height());
    Ok(NewSlide { background: Some([0, 0, 0]), items: vec![Item::Picture { x, y, w, h, data, ext: file_ext, name: file_name(input).to_string() }] })
}

/// Several pictures -> one presentation, a slide each, in the order they were dropped
pub(super) fn merge_to_pptx(paths: &[String], progress: &Channel<Progress>, out_dir: Option<&Path>, options: &Options) -> Result<BatchResult, String> {
    let slides = par_map(paths, progress, |p| memory::cost(p, "jpg"), |p| isolate(p, || image_slide(Path::new(p), options)));
    let mut ok = vec![];
    let mut errors = vec![];
    for slide in slides {
        match slide {
            Ok(s) => ok.push(s),
            Err(e) => errors.push(e),
        }
    }
    if ok.is_empty() {
        return Ok(BatchResult { outputs: vec![], errors, pages: 0, bytes_before: 0, bytes_after: 0, note: None });
    }
    let first = Path::new(&paths[0]);
    let deck = Deck { width: WIDE.0, height: WIDE.1, title: title_of(first), slides: ok };
    let output = save_deck(&deck, first, out_dir, "merged")?;
    Ok(BatchResult { outputs: vec![output.to_string_lossy().into_owned()], errors, pages: 0, bytes_before: 0, bytes_after: 0, note: None })
}

// ---------------------------------------------------------------------------
// Documents and tables -> slides

const MARGIN: i64 = 548_640; // 0.6 in
const TITLE_TOP: i64 = 365_760;
const TITLE_HEIGHT: i64 = 1_005_840;
const BODY_TOP: i64 = 1_463_040;
const ACCENT: [u8; 3] = [0x15, 0x60, 0x82];

fn title_slide(title: &str, subtitle: Option<&str>) -> NewSlide {
    let mut t = Para::new(title, 44.0);
    t.bold = true;
    t.align = "ctr";
    t.color = ACCENT;
    let mut items = vec![Item::Text { x: MARGIN, y: 1_600_200, w: WIDE.0 - 2 * MARGIN, h: 2_286_000, paragraphs: vec![t], anchor: "b", shrink: true }];
    if let Some(sub) = subtitle.filter(|s| !s.trim().is_empty()) {
        let mut s = Para::new(sub, 22.0);
        s.align = "ctr";
        s.color = [0x59, 0x59, 0x59];
        items.push(Item::Text { x: MARGIN, y: 4_023_360, w: WIDE.0 - 2 * MARGIN, h: 1_371_600, paragraphs: vec![s], anchor: "t", shrink: true });
    }
    NewSlide { background: None, items }
}

fn content_slide(title: &str, body: Vec<Para>) -> NewSlide {
    let mut t = Para::new(title, 32.0);
    t.bold = true;
    t.color = ACCENT;
    let mut items = vec![Item::Text { x: MARGIN, y: TITLE_TOP, w: WIDE.0 - 2 * MARGIN, h: TITLE_HEIGHT, paragraphs: vec![t], anchor: "b", shrink: true }];
    if !body.is_empty() {
        items.push(Item::Text { x: MARGIN, y: BODY_TOP, w: WIDE.0 - 2 * MARGIN, h: WIDE.1 - BODY_TOP - MARGIN, paragraphs: body, anchor: "t", shrink: true });
    }
    NewSlide { background: None, items }
}

/// Lines a paragraph takes in the body box (about 80 characters of 20 pt text a line)
fn lines(p: &Para) -> usize {
    let per_line = (80.0 * 20.0 / p.size) as usize;
    p.text.split('\n').map(|l| l.chars().count().div_ceil(per_line.max(1)).max(1)).sum()
}

/// The body box holds about 12 lines of 20 pt text with spacing
const LINES_PER_SLIDE: usize = 12;

/// Headings (levels 1 and 2) start slides; text and lists fill them, continuing on
/// further slides when they don't fit; tables get slides of their own.
pub(super) fn blocks_deck(title: &str, blocks: &[DocBlock]) -> Deck {
    let mut slides = vec![];
    let mut blocks = blocks.iter().peekable();
    // A leading main heading (and a short line under it) makes the title slide
    let mut deck_title = title.to_string();
    if let Some(DocBlock::Heading(1, t)) = blocks.peek() {
        deck_title = t.clone();
        blocks.next();
    }
    let subtitle = match blocks.peek() {
        Some(DocBlock::Paragraph(p)) if p.chars().count() <= 160 => {
            let p = p.clone();
            blocks.next();
            Some(p)
        }
        _ => None,
    };
    slides.push(title_slide(&deck_title, subtitle.as_deref()));

    let mut current_title = deck_title.clone();
    let mut body: Vec<Para> = vec![];
    let mut used = 0;
    let mut continued = false;
    let flush = |slides: &mut Vec<NewSlide>, title: &str, body: &mut Vec<Para>, continued: bool| {
        if !body.is_empty() {
            let shown = if continued { format!("{title} (cont.)") } else { title.to_string() };
            slides.push(content_slide(&shown, std::mem::take(body)));
        }
    };
    for block in blocks {
        let para = match block {
            DocBlock::Heading(level, text) if *level <= 2 => {
                if !body.is_empty() {
                    flush(&mut slides, &current_title, &mut body, continued);
                } else if current_title != deck_title && !continued {
                    // A heading with nothing under it still gets its slide
                    slides.push(content_slide(&current_title, vec![]));
                }
                current_title = text.clone();
                used = 0;
                continued = false;
                continue;
            }
            DocBlock::Heading(_, text) => {
                let mut p = Para::new(text.clone(), 22.0);
                p.bold = true;
                p.space_before = 6.0;
                p
            }
            DocBlock::Paragraph(text) | DocBlock::CodeBlock(text) => {
                let mut p = Para::new(text.clone(), 18.0);
                p.space_before = 6.0;
                p
            }
            DocBlock::ListItem(text) => {
                let depth = (text.len() - text.trim_start().len()) / 2;
                let mut p = Para::new(text.trim_start().to_string(), if depth == 0 { 20.0 } else { 18.0 });
                p.bullet = Some(depth.min(4) as u8);
                p.space_before = 4.0;
                p
            }
            DocBlock::Table(rows) => {
                flush(&mut slides, &current_title, &mut body, continued);
                slides.extend(tables_deck("", vec![(current_title.clone(), rows.clone())]).slides);
                used = 0;
                continued = true;
                continue;
            }
            DocBlock::PageBreak => continue,
        };
        let need = lines(&para);
        if used + need > LINES_PER_SLIDE && !body.is_empty() {
            flush(&mut slides, &current_title, &mut body, continued);
            continued = true;
            used = 0;
        }
        used += need;
        body.push(para);
    }
    flush(&mut slides, &current_title, &mut body, continued);
    Deck { width: WIDE.0, height: WIDE.1, title: deck_title, slides }
}

/// Tables -> slides of at most 12 rows and 8 columns, the header row repeated on each
pub(super) fn tables_deck(title: &str, tables: Vec<(String, Vec<Vec<String>>)>) -> Deck {
    const ROWS: usize = 12;
    const COLS: usize = 8;
    let mut slides = vec![];
    if !title.is_empty() {
        slides.push(title_slide(title, None));
    }
    for (name, rows) in tables {
        let rows: Vec<Vec<String>> = rows.into_iter().filter(|r| r.iter().any(|c| !c.trim().is_empty())).collect();
        if rows.is_empty() {
            continue;
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let header = &rows[0];
        let body = &rows[1..];
        let row_chunks: Vec<&[Vec<String>]> = if body.is_empty() { vec![&[]] } else { body.chunks(ROWS - 1).collect() };
        let col_chunks: Vec<(usize, usize)> = (0..width).step_by(COLS).map(|c| (c, (c + COLS).min(width))).collect();
        let parts = row_chunks.len() * col_chunks.len();
        let mut part = 0;
        for chunk in &row_chunks {
            for &(c0, c1) in &col_chunks {
                part += 1;
                let cut = |r: &Vec<String>| (c0..c1).map(|c| r.get(c).cloned().unwrap_or_default()).collect::<Vec<_>>();
                let mut table = vec![cut(header)];
                table.extend(chunk.iter().map(cut));
                // Long cells get smaller text
                let longest = table.iter().flatten().map(|c| c.chars().count()).max().unwrap_or(0);
                let size = if c1 - c0 > 5 || longest > 40 { 12.0 } else { 14.0 };
                let shown = if parts > 1 { format!("{name} ({part}/{parts})") } else { name.clone() };
                let mut slide = content_slide(&shown, vec![]);
                slide.items.push(Item::Table { x: MARGIN, y: BODY_TOP, w: WIDE.0 - 2 * MARGIN, rows: table, header: true, size });
                slides.push(slide);
            }
        }
    }
    if slides.is_empty() {
        slides.push(title_slide(title, None));
    }
    Deck { width: WIDE.0, height: WIDE.1, title: title.to_string(), slides }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_become_slides() {
        let blocks = vec![
            DocBlock::Heading(1, "Annual report".into()),
            DocBlock::Paragraph("Prepared for the board".into()),
            DocBlock::Heading(2, "Revenue".into()),
            DocBlock::ListItem("Up 12%".into()),
            DocBlock::ListItem("  Mostly Jakarta".into()),
            DocBlock::Heading(2, "Long".into()),
        ];
        let mut blocks = blocks;
        for i in 0..30 {
            blocks.push(DocBlock::Paragraph(format!("Paragraph {i} with some words in it")));
        }
        blocks.push(DocBlock::Table(vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]]));
        let deck = blocks_deck("file", &blocks);
        // Title, Revenue, Long ×3 (30 one-line paragraphs, 12 a slide), table
        assert_eq!(deck.slides.len(), 6, "{}", deck.slides.len());
        assert_eq!(deck.title, "Annual report");
        let bytes = office::pptx_writer::write(&deck).unwrap();
        let read = office::pptx::read(&bytes).unwrap();
        assert_eq!(read.slides.len(), 6);
    }

    #[test]
    fn big_tables_split_over_slides() {
        let rows: Vec<Vec<String>> = (0..30).map(|r| (0..10).map(|c| format!("{r}-{c}")).collect()).collect();
        let deck = tables_deck("", vec![("Data".into(), rows)]);
        // 29 data rows in 11s -> 3 row groups; 10 columns in 8s -> 2 column groups
        assert_eq!(deck.slides.len(), 6);
    }
}
