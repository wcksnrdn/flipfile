// Compress levels (the Option/Alt ring): Light, Balanced, Strong and Extreme, for photos,
// PDFs, documents, books, animations, videos and recordings alike, each in its own format
// where that can get smaller.
// Before the user lets go, `compress_estimate` works out what each level would make of
// the files, and says what it costs ("4.2 MB → 610 KB · fine detail softens").
//
// - photo: re-encoded at the level's quality and size (lossless formats become JPG, or
//   WebP when transparent, from Balanced on)
// - pdf:   the pictures inside re-encoded at the level's resolution; text and layout stay
// - package: Word, Excel, PowerPoint, OpenDocument and EPUB: the pictures inside at the
//          level's resolution where they are shown, videos inside at its bitrate
// - animation: animated GIFs, kept animated
// - av:    video and audio at the level's bitrate and resolution (ffmpeg, else the
//          system's own engines)
// - estimate: the live estimates

mod animation;
mod av;
pub mod estimate;
mod package;
mod pdf;
mod photo;
pub(super) use photo::is_source_only;

use std::path::Path;

use super::input::{file_name, header};
use super::media::is_video_file;
use super::output::{save_err, unique_output_path};
use super::{Converted, OnPage};
use crate::pdf::render as pdf_render;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Light,
    Balanced,
    Strong,
    Extreme,
}

pub const LEVELS: [Level; 4] = [Level::Light, Level::Balanced, Level::Strong, Level::Extreme];

impl Level {
    pub fn from_format(format: &str) -> Option<Level> {
        match format {
            "cmp-light" => Some(Level::Light),
            "cmp-balanced" => Some(Level::Balanced),
            "cmp-strong" => Some(Level::Strong),
            "cmp-extreme" => Some(Level::Extreme),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Level::Light => "cmp-light",
            Level::Balanced => "cmp-balanced",
            Level::Strong => "cmp-strong",
            Level::Extreme => "cmp-extreme",
        }
    }
}

/// What kind of file a level is applied to
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Photo,
    Pdf,
    /// Word, Excel, PowerPoint and OpenDocument files
    Document,
    /// EPUB
    Book,
    /// An animated GIF
    Animation,
    Video,
    Audio,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    if pdf_render::is_pdf(&header(path)) {
        return Some(Kind::Pdf);
    }
    match package::family(path) {
        Some(package::Family::Book) => return Some(Kind::Book),
        Some(_) => return Some(Kind::Document),
        None => {}
    }
    if animation::is_animated_gif(path) {
        return Some(Kind::Animation);
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if is_video_file(path) {
        return Some(Kind::Video);
    }
    if matches!(
        ext.as_str(),
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "opus" | "aiff" | "aif" | "caf" | "wma" | "amr" | "m4b" | "alac"
    ) {
        return Some(Kind::Audio);
    }
    (super::input::same_format_ext(path).is_ok() || photo::is_source_only(path)).then_some(Kind::Photo)
}

/// True for a document compressed as a ZIP package (Office, OpenDocument, EPUB)
pub(super) fn is_package(path: &Path) -> bool {
    package::family(path).is_some()
}

/// What a level does to a kind of file, in a few words: shown before the drop, and as
/// the note after it
pub fn consequence(kind: Kind, level: Level) -> &'static str {
    match (kind, level) {
        (Kind::Photo, Level::Light) => "Looks the same",
        (Kind::Photo, Level::Balanced) => "Hard to tell apart",
        (Kind::Photo, Level::Strong) => "Fine detail softens · max 1920 px",
        (Kind::Photo, Level::Extreme) => "Visibly soft, blocky edges · max 1280 px",
        (Kind::Pdf, Level::Light) => "Pictures at 200 dpi · text untouched",
        (Kind::Pdf, Level::Balanced) => "Pictures at 150 dpi · fine on screens",
        (Kind::Pdf, Level::Strong) => "Pictures at 110 dpi · soft when printed",
        (Kind::Pdf, Level::Extreme) => "Pictures at 72 dpi, blurry · text stays sharp",
        (Kind::Document, Level::Light) => "Pictures at 200 dpi · text untouched",
        (Kind::Document, Level::Balanced) => "Pictures at 150 dpi · fine on screens",
        (Kind::Document, Level::Strong) => "Pictures at 110 dpi · soft when printed",
        (Kind::Document, Level::Extreme) => "Pictures at 72 dpi, blurry · text untouched",
        (Kind::Book, Level::Light) => "Pictures up to 2400 px · text untouched",
        (Kind::Book, Level::Balanced) => "Pictures up to 1800 px · fine on tablets",
        (Kind::Book, Level::Strong) => "Pictures up to 1400 px · fine on phones",
        (Kind::Book, Level::Extreme) => "Pictures up to 1000 px, soft · text untouched",
        (Kind::Animation, Level::Light) => "Looks the same · every frame kept",
        (Kind::Animation, Level::Balanced) => "Hard to tell apart · fewer colours",
        (Kind::Animation, Level::Strong) => "Fewer details · max 480 px",
        (Kind::Animation, Level::Extreme) => "Choppier, half the frames · max 320 px",
        (Kind::Video, Level::Light) => "Looks the same · up to 1080p",
        (Kind::Video, Level::Balanced) => "Slightly softer · up to 1080p",
        (Kind::Video, Level::Strong) => "Softer, fine on phones · 720p",
        (Kind::Video, Level::Extreme) => "Blocky in motion · 480p",
        (Kind::Audio, Level::Light) => "Sounds the same · 160 kbps",
        (Kind::Audio, Level::Balanced) => "Fine for music · 128 kbps",
        (Kind::Audio, Level::Strong) => "Duller highs · 96 kbps",
        (Kind::Audio, Level::Extreme) => "Voice only · 48 kbps mono",
    }
}

/// Compresses one file at `format`'s level; the result keeps the file's format where
/// that format can get smaller, and is never larger than the original. `on_step` hears
/// the progress of long ones (video) in percent.
pub(super) fn compress_file(input: &Path, format: &str, out_dir: Option<&Path>, on_step: OnPage) -> Result<Converted, String> {
    let name = file_name(input);
    let level = Level::from_format(format).ok_or_else(|| format!("\"{name}\": unknown compress level"))?;
    let kind = kind_of(input).ok_or_else(|| unsupported(input))?;
    let result = match kind {
        Kind::Photo => photo::compress(input, level, out_dir)?,
        Kind::Pdf => pdf::compress(input, level, out_dir)?,
        Kind::Document | Kind::Book => package::compress(input, level, out_dir, on_step)?,
        Kind::Animation => animation::compress(input, level, out_dir)?,
        Kind::Video => av::compress_video(input, level, out_dir, on_step)?,
        Kind::Audio => av::compress_audio(input, level, out_dir)?,
    };
    finish(input, result, kind, level, out_dir)
}

/// Why a file can't be compressed
fn unsupported(input: &Path) -> String {
    let name = file_name(input);
    let ext = input.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    match ext.as_str() {
        "doc" | "dot" | "xls" | "xlt" | "ppt" | "pps" | "pot" => {
            format!("\"{name}\" is an old Office file: save it as .{}x first, then compress that", ext.chars().take(3).collect::<String>())
        }
        _ => format!("\"{name}\": photos, PDFs, Office and OpenDocument files, EPUBs, GIFs, videos and recordings can be compressed"),
    }
}

/// A result before it is checked against the original
pub(super) struct Made {
    pub path: std::path::PathBuf,
    /// The format changed: "PNG → JPG"
    pub changed: Option<String>,
}

/// Never larger than the original: a result that isn't smaller is replaced by a copy of
/// the original, in its own format
fn finish(input: &Path, made: Made, kind: Kind, level: Level, out_dir: Option<&Path>) -> Result<Converted, String> {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let (before, after) = (size(input), size(&made.path));
    let mut path = made.path;
    let note = if after == 0 || after >= before {
        let _ = std::fs::remove_file(&path);
        let own = input.extension().and_then(|e| e.to_str()).unwrap_or("bin").to_string();
        path = unique_output_path(input, out_dir, "compressed", &own);
        std::fs::copy(input, &path).map_err(save_err)?;
        "Already compact".to_string()
    } else {
        match made.changed {
            Some(changed) => format!("{changed} · {}", consequence(kind, level)),
            None => consequence(kind, level).to_string(),
        }
    };
    let mut converted = Converted::file(input, &path);
    converted.note = Some(note);
    Ok(converted)
}

/// "PNG → JPG" when the extension changes
pub(super) fn changed(input: &Path, ext: &str) -> Option<String> {
    let own = input.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    let same = own == ext || (own == "jpeg" && ext == "jpg") || (own == "jpg" && ext == "jpeg") || (own == "tif" && ext == "tiff");
    (!same).then(|| format!("{} → {}", if own.is_empty() { "?".into() } else { own.to_uppercase() }, ext.to_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::encode::encode_jpeg;
    use crate::convert::office_extra::{blocks_to_pdf, DocBlock};
    use crate::pdf::writer::{jpeg_pages_to_pdf, Page};
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flipfile-test-compress-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A photo-like picture: gradients with grain, which JPEG can't shrink to nothing
    fn photo(w: u32, h: u32) -> image::DynamicImage {
        let mut seed = 11u32;
        image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let noise = (seed >> 25) as u8 / 4;
            image::Rgb([((x * 255 / w) as u8).wrapping_add(noise), ((y * 255 / h) as u8).wrapping_add(noise), (((x + y) / 9) as u8).wrapping_add(noise)])
        }))
    }

    fn run(path: &Path, level: Level) -> Converted {
        compress_file(path, level.id(), None, &|_, _| {}).unwrap_or_else(|e| panic!("{level:?}: {e}"))
    }

    fn bytes(path: &str) -> u64 {
        fs::metadata(path).unwrap().len()
    }

    #[test]
    fn photos_shrink_level_by_level() {
        let dir = temp_dir("photo");
        let jpg = dir.join("holiday.jpg");
        let mut data = Vec::new();
        encode_jpeg(&photo(3000, 2000), 95, &mut data).unwrap();
        fs::write(&jpg, &data).unwrap();
        let mut last = bytes(jpg.to_str().unwrap());
        for level in LEVELS {
            let out = run(&jpg, level);
            assert!(out.path.ends_with(".jpg"), "{}", out.path);
            let size = bytes(&out.path);
            assert!(size < last, "{level:?}: {size} not below {last}");
            last = size;
            let img = image::open(&out.path).unwrap();
            let longest = img.width().max(img.height());
            match level {
                Level::Light | Level::Balanced => assert_eq!(longest, 3000),
                Level::Strong => assert_eq!(longest, 1920),
                Level::Extreme => assert_eq!(longest, 1280),
            }
            assert_eq!(out.note.as_deref(), Some(consequence(Kind::Photo, level)));
        }

        // A PNG stays a lossless PNG at Light, and becomes a JPG from Balanced on
        let png = dir.join("scan.png");
        photo(1600, 1200).save(&png).unwrap();
        let light = run(&png, Level::Light);
        assert!(light.path.ends_with(".png"), "{}", light.path);
        let balanced = run(&png, Level::Balanced);
        assert!(balanced.path.ends_with(".jpg"), "{}", balanced.path);
        assert!(balanced.note.as_deref().unwrap().starts_with("PNG → JPG"), "{:?}", balanced.note);
        // Transparent pictures keep their transparency, as WebP
        let logo = dir.join("logo.png");
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(800, 600, |x, y| image::Rgba([x as u8, y as u8, 90, if x < 400 { 0 } else { 255 }])))
            .save(&logo)
            .unwrap();
        let strong = run(&logo, Level::Strong);
        assert!(strong.path.ends_with(".webp"), "{}", strong.path);
        assert!(image::open(&strong.path).unwrap().color().has_alpha());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Nothing ever gets bigger: an already tiny file comes back as a copy of itself
    #[test]
    fn never_bigger() {
        let dir = temp_dir("tiny");
        let png = dir.join("dot.png");
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([200, 30, 30]))).save(&png).unwrap();
        let out = run(&png, Level::Extreme);
        assert!(out.path.ends_with("dot_compressed.png"), "{}", out.path);
        assert_eq!(fs::read(&out.path).unwrap(), fs::read(&png).unwrap());
        assert_eq!(out.note.as_deref(), Some("Already compact"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// A scanned PDF (pages that are pictures) shrinks, keeps its pages, and the pictures
    /// get the level's resolution
    #[test]
    fn pdf_pictures_shrink_and_pages_stay() {
        let dir = temp_dir("pdf");
        let pages: Vec<Page> = (0..3)
            .map(|i| {
                let mut jpeg = Vec::new();
                encode_jpeg(&photo(2480 + i, 3508), 92, &mut jpeg).unwrap();
                Page { jpeg, width: 2480 + i, height: 3508, gray: false }
            })
            .collect();
        let pdf = dir.join("scan.pdf");
        fs::write(&pdf, jpeg_pages_to_pdf(&pages)).unwrap();
        let before = bytes(pdf.to_str().unwrap());
        let mut last = before;
        for level in LEVELS {
            let out = run(&pdf, level);
            assert!(out.path.ends_with(".pdf"));
            let size = bytes(&out.path);
            assert!(size < last, "{level:?}: {size} not below {last}");
            last = size;
            let reopened = crate::pdf::render::pdfium().unwrap().load_pdf_from_file(&out.path, None).unwrap();
            assert_eq!(reopened.pages().len(), 3);
        }
        assert!(last * 10 < before, "Extreme should take a scan down a lot: {before} -> {last}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A text PDF keeps its text, readable and selectable
    #[test]
    fn pdf_text_stays() {
        let dir = temp_dir("pdf-text");
        let blocks = vec![
            DocBlock::Heading(1, "Quarterly report".into()),
            DocBlock::Paragraph("Revenue grew by twelve percent while costs stayed flat. ".repeat(40)),
        ];
        let pdf = dir.join("report.pdf");
        fs::write(&pdf, blocks_to_pdf(&blocks).unwrap()).unwrap();
        let out = run(&pdf, Level::Extreme);
        let text = crate::convert::text::extract(Path::new(&out.path), &|_, _| {}).unwrap();
        assert!(text.text.contains("Quarterly report") && text.text.contains("Revenue grew by twelve percent"), "{}", text.text);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A ZIP package with these parts, deflated as Office writes them
    fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in parts {
            let method = if *name == "mimetype" { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
            zip.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(method)).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn part(path: &str, name: &str) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut file = zip.by_name(name).ok()?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        Some(data)
    }

    fn png_bytes(img: &image::DynamicImage) -> Vec<u8> {
        let mut data = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut data), image::ImageFormat::Png).unwrap();
        data
    }

    /// A Word document with a photo pasted as PNG (shown 6 x 4 in) and a camera JPEG
    /// (shown 3 x 2.25 in): pictures are scaled to where they're shown, the PNG photo
    /// becomes a JPEG from Balanced on with its references, and the text stays
    #[test]
    fn word_pictures_shrink_to_where_they_are_shown() {
        let dir = temp_dir("docx");
        let mut jpeg = Vec::new();
        encode_jpeg(&photo(4000, 3000), 95, &mut jpeg).unwrap();
        let drawing = |rid: &str, cx: u64, cy: u64| {
            format!(
                r#"<w:r><w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="{rid}"/></pic:blipFill><pic:spPr><a:xfrm><a:ext cx="{cx}" cy="{cy}"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
            )
        };
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:r><w:t>Site visit report</w:t></w:r></w:p><w:p>{}</w:p><w:p>{}</w:p></w:body></w:document>"#,
            drawing("rId1", 6 * 914_400, 4 * 914_400),
            drawing("rId2", 3 * 914_400, 2_057_400)
        );
        let docx = package(&[
            ("[Content_Types].xml", br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
            ("_rels/.rels", br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
            ("word/document.xml", document.as_bytes()),
            ("word/_rels/document.xml.rels", br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image2.jpeg"/></Relationships>"#),
            ("word/media/image1.png", &png_bytes(&photo(3000, 2000))),
            ("word/media/image2.jpeg", &jpeg),
        ]);
        let path = dir.join("report.docx");
        fs::write(&path, &docx).unwrap();
        let mut last = docx.len() as u64;
        for level in LEVELS {
            let out = run(&path, level);
            assert!(out.path.ends_with(".docx"), "{}", out.path);
            let size = bytes(&out.path);
            assert!(size < last, "{level:?}: {size} not below {last}");
            last = size;
            assert_eq!(out.note.as_deref(), Some(consequence(Kind::Document, level)));
            let dpi = match level {
                Level::Light => 200,
                Level::Balanced => 150,
                Level::Strong => 110,
                Level::Extreme => 72,
            };
            let rels = String::from_utf8(part(&out.path, "word/_rels/document.xml.rels").unwrap()).unwrap();
            let pasted = if level == Level::Light {
                assert!(rels.contains("media/image1.png"), "{rels}");
                part(&out.path, "word/media/image1.png").unwrap()
            } else {
                assert!(rels.contains(r#"Target="media/image1.jpeg""#) && !rels.contains("image1.png"), "{rels}");
                assert!(part(&out.path, "word/media/image1.png").is_none());
                part(&out.path, "word/media/image1.jpeg").unwrap()
            };
            assert_eq!(image::load_from_memory(&pasted).unwrap().width(), 6 * dpi, "{level:?}");
            let camera = image::load_from_memory(&part(&out.path, "word/media/image2.jpeg").unwrap()).unwrap();
            assert_eq!(camera.width(), 3 * dpi, "{level:?}");
            let text = crate::convert::text::extract(Path::new(&out.path), &|_, _| {}).unwrap();
            assert!(text.text.contains("Site visit report"), "{}", text.text);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// OpenDocument and EPUB keep `mimetype` first and stored, and their manifests follow
    /// a picture that becomes a JPEG
    #[test]
    fn opendocument_and_books_stay_valid() {
        let dir = temp_dir("odf");
        let picture = png_bytes(&photo(2400, 1600));
        let odt = package(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", br#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink"><office:body><office:text><text:p>Holiday</text:p><draw:frame svg:width="10.16cm" svg:height="6.773cm"><draw:image xlink:href="Pictures/beach.png"/></draw:frame></office:text></office:body></office:document-content>"#),
            ("META-INF/manifest.xml", br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="Pictures/beach.png" manifest:media-type="image/png"/></manifest:manifest>"#),
            ("Pictures/beach.png", &picture),
        ]);
        let epub = package(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
            ("OEBPS/book.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/><item id="p1" href="images/beach.png" media-type="image/png"/></manifest></package>"#),
            ("OEBPS/ch1.xhtml", br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>Chapter one</p><img src="images/beach.png" alt=""/></body></html>"#),
            ("OEBPS/images/beach.png", &picture),
        ]);
        for (name, data, picture_dir, manifest) in [("trip.odt", &odt, "Pictures", "META-INF/manifest.xml"), ("trip.epub", &epub, "OEBPS/images", "OEBPS/book.opf")] {
            let path = dir.join(name);
            fs::write(&path, data).unwrap();
            let out = run(&path, Level::Balanced);
            assert!(bytes(&out.path) < data.len() as u64 / 2, "{name}: {}", bytes(&out.path));
            let mut zip = zip::ZipArchive::new(fs::File::open(&out.path).unwrap()).unwrap();
            let first = zip.by_index(0).unwrap();
            assert_eq!((first.name().unwrap().as_ref(), first.compression()), ("mimetype", zip::CompressionMethod::Stored), "{name}");
            drop(first);
            let listed = String::from_utf8(part(&out.path, manifest).unwrap()).unwrap();
            assert!(listed.contains("beach.jpg\" manifest:media-type=\"image/jpeg\"") || listed.contains("beach.jpg\" media-type=\"image/jpeg\""), "{name}: {listed}");
            let shrunk = image::load_from_memory(&part(&out.path, &format!("{picture_dir}/beach.jpg")).unwrap()).unwrap();
            // The ODT shows it 4 in wide (600 px at 150 dpi); books fit 1800 px
            assert_eq!(shrunk.width(), if name.ends_with(".odt") { 600 } else { 1800 }, "{name}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// Old binary Office files get a way forward rather than "can't be compressed"
    #[test]
    fn old_office_files_say_what_to_do() {
        let dir = temp_dir("doc");
        let doc = dir.join("letter.doc");
        fs::write(&doc, [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0]).unwrap();
        let Err(err) = compress_file(&doc, "cmp-strong", None, &|_, _| {}) else { panic!("an old .doc compressed") };
        assert!(err.contains(".docx"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A recording becomes a smaller M4A (built-in engine on the Mac)
    #[cfg(target_os = "macos")]
    #[test]
    fn audio_shrinks() {
        let dir = temp_dir("audio");
        let rate = 44_100u32;
        let mut data = Vec::new();
        for i in 0..rate as usize * 6 {
            let v = ((i as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin() * 9000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
            data.extend_from_slice(&v.to_le_bytes());
        }
        let wav = dir.join("memo.wav");
        fs::write(&wav, crate::convert::transcribe::audio::tests::wav_bytes(rate, 2, &data)).unwrap();
        let out = run(&wav, Level::Strong);
        assert!(out.path.ends_with(".m4a"), "{}", out.path);
        assert!(bytes(&out.path) * 8 < bytes(wav.to_str().unwrap()), "{}", bytes(&out.path));
        assert!(out.note.as_deref().unwrap().starts_with("WAV → M4A"), "{:?}", out.note);
        let _ = fs::remove_dir_all(&dir);
    }

    /// `FLIPFILE_FILES=a.docx:b.gif FLIPFILE_OUT=dir cargo test --profile bench --lib compresses_files -- --ignored --nocapture`:
    /// every level of real files, their sizes, times and estimates
    #[test]
    #[ignore]
    fn compresses_files() {
        let out_dir = std::env::var("FLIPFILE_OUT").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
        for path in std::env::var("FLIPFILE_FILES").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
            let input = Path::new(path);
            let before = fs::metadata(input).unwrap().len();
            let estimates = estimate::tests_estimate(&[input.to_path_buf()]);
            for (level, estimate) in LEVELS.iter().zip(estimates) {
                let started = std::time::Instant::now();
                match compress_file(input, level.id(), Some(&out_dir), &|_, _| {}) {
                    Ok(out) => eprintln!(
                        "{} {level:?}: {before} -> {} bytes ({:.1}x) in {:.1} s, estimated {estimate:?}; {:?} -> {}",
                        file_name(input),
                        bytes(&out.path),
                        before as f64 / bytes(&out.path) as f64,
                        started.elapsed().as_secs_f64(),
                        out.note,
                        out.path
                    ),
                    Err(e) => eprintln!("{} {level:?}: {e}", file_name(input)),
                }
            }
        }
    }

    /// `FLIPFILE_VIDEO=clip.mp4 cargo test --lib compresses_video -- --ignored --nocapture`:
    /// every level of a real video, its size, time and estimate
    #[test]
    #[ignore]
    fn compresses_video() {
        let Ok(path) = std::env::var("FLIPFILE_VIDEO") else { return };
        let input = Path::new(&path);
        let before = fs::metadata(input).unwrap().len();
        let estimates = estimate::tests_estimate(&[input.to_path_buf()]);
        for (level, estimate) in LEVELS.iter().zip(estimates) {
            let started = std::time::Instant::now();
            let out = run(input, *level);
            eprintln!(
                "{level:?}: {} -> {} bytes ({:.1}x) in {:.1} s, estimated {:?}; {:?}",
                before,
                bytes(&out.path),
                before as f64 / bytes(&out.path) as f64,
                started.elapsed().as_secs_f64(),
                estimate,
                out.note
            );
            if std::env::var_os("FLIPFILE_KEEP").is_none() {
                let _ = fs::remove_file(&out.path);
            }
        }
    }
}
