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
