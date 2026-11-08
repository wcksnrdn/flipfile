// Stress tests of the conversion pipeline: hostile and odd inputs against every target
// format, and parallel batches. A file flipfile cannot convert must come back as an
// error message for that file, never as a crash (which would close the app and lose
// the rest of the batch) or a conversion that never ends.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};
use tauri::ipc::Channel;

use super::*;

/// Every target the UI offers that runs in-process
const TARGETS: &[&str] = &[
    "jpg", "png", "webp", "avif", "tiff", "gif", "bmp", "ico", "icns", "tga", "exr", "hdr", "qoi", "ppm", "svg", "pdf",
    "heic", "clean", "rot-r", "rot-180", "flip-h", "grayscale", "square", "cmp-auto", "cmp-lossless", "cmp-100k",
    "docx", "xlsx", "csv", "tsv", "json", "yaml", "xml", "sql", "md-table", "md", "txt", "html", "rtf", "epub", "odt",
    "pptx", SAME_FORMAT,
];

/// System engines, slow to start
const SLOW_TARGETS: &[&str] = &["docx-ocr", "nobg", "up-2x", "up-4x"];

const LIMIT: Duration = Duration::from_secs(60);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("flipfile-stress-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Bytes that look like noise to every decoder, but always the same
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

fn png_bytes(img: &DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// A PNG whose header claims 100 000 × 100 000 pixels (40 GB once decoded)
fn png_bomb() -> Vec<u8> {
    let mut png = png_bytes(&DynamicImage::ImageRgb8(RgbImage::new(8, 8)));
    png[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    png[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A ZIP holding `entries`, for Office files that are almost right
fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Hostile and odd inputs: (file name, contents)
fn hostile_inputs() -> Vec<(String, Vec<u8>)> {
    let photo = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 48, |x, y| Rgb([(x * 4) as u8, (y * 5) as u8, 90])));
    let png = png_bytes(&photo);
    let mut jpeg = Vec::new();
    super::encode::encode_jpeg(&photo, 90, &mut jpeg).unwrap();
    let pdf = crate::pdf::writer::jpeg_pages_to_pdf(&[super::encode::pdf_page(&photo, 80).unwrap()]);
    let docx = small_docx();
    let mut v: Vec<(String, Vec<u8>)> = vec![];
    let ext_all = [
        "jpg", "png", "webp", "gif", "tiff", "bmp", "ico", "tga", "qoi", "exr", "hdr", "ppm", "avif", "heic", "pdf", "docx",
        "doc", "xlsx", "xls", "csv", "tsv", "json", "yaml", "xml", "md", "txt", "html", "rtf", "odt", "epub", "pptx",
        "ppt", "odp",
    ];
    for (i, ext) in ext_all.iter().enumerate() {
        v.push((format!("empty.{ext}"), vec![]));
        v.push((format!("noise.{ext}"), noise(4096, i as u32 + 1)));
    }
    // The right signature, then garbage
    v.push(("fake-sig.png".into(), [&png[..16], &noise(500, 7)[..]].concat()));
    v.push(("fake-sig.jpg".into(), [&jpeg[..20], &noise(500, 8)[..]].concat()));
    v.push(("fake-sig.heic".into(), [&b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"[..], &noise(800, 9)[..]].concat()));
    v.push(("fake-sig.pdf".into(), [&b"%PDF-1.7\n"[..], &noise(800, 10)[..]].concat()));
    v.push(("fake-sig.docx".into(), [&b"PK\x03\x04"[..], &noise(800, 11)[..]].concat()));
    v.push(("fake-sig.xlsx".into(), [&b"PK\x03\x04"[..], &noise(800, 12)[..]].concat()));
    v.push(("fake-sig.doc".into(), [&[0xD0, 0xCF, 0x11, 0xE0][..], &noise(800, 13)[..]].concat()));
    // Cut short
    v.push(("truncated.png".into(), png[..png.len() / 2].to_vec()));
    v.push(("truncated.jpg".into(), jpeg[..jpeg.len() / 2].to_vec()));
    v.push(("truncated.pdf".into(), pdf[..pdf.len() / 2].to_vec()));
    v.push(("truncated.docx".into(), docx[..docx.len() / 2].to_vec()));
    // Decompression bomb
    v.push(("bomb.png".into(), png_bomb()));
    // Office files with the right parts but nothing (or nonsense) in them
    v.push(("hollow.docx".into(), zip_bytes(&[("word/document.xml", b"<w:document/>")])));
    v.push(("garbage-xml.docx".into(), zip_bytes(&[("word/document.xml", b"<w:document><w:body><w:p><w:r><w:t>unclosed")])));
    v.push(("hollow.xlsx".into(), zip_bytes(&[("xl/workbook.xml", b"<workbook/>")])));
    v.push(("no-sheets.xlsx".into(), zip_bytes(&[("[Content_Types].xml", b"<Types/>")])));
    v.push(("hollow.epub".into(), zip_bytes(&[("mimetype", b"application/epub+zip")])));
    v.push(("hollow.odt".into(), zip_bytes(&[("content.xml", b"<office:document-content/>")])));
    v.push(("hollow.pptx".into(), zip_bytes(&[("ppt/presentation.xml", b"<p:presentation/>")])));
    v.extend(hostile_presentations());
    // Valid but odd data
    for (name, text) in [
        ("header-only.csv", "a,b,c\n"),
        ("blank-lines.csv", "\n\n\n"),
        ("ragged.csv", "a,b\n1\n1,2,3,4,5\n\"unterminated\n"),
        ("array.json", "[]"),
        ("object.json", "{}"),
        ("scalar.json", "\"just text\""),
        ("nested.json", "[[[[[[1]]]]], {\"a\": {\"b\": [null, true]}}]"),
        ("deep.json", &format!("{}{}", "[".repeat(5000), "]".repeat(5000))),
        ("list.yaml", "- 1\n- [2, 3]\n- a: b\n"),
        ("bad.yaml", "key: [unclosed\n  - : :\n"),
        ("rows.xml", "<root/>"),
        ("bad.xml", "<a><b></a>"),
        ("spaces.md", "   \n\t\n"),
        ("deep.md", &"> ".repeat(2000)),
        ("table.md", "| a |\n|---|\n| 1 | 2 | 3 |\n"),
        ("bad.html", "<html><body><table><tr><td>cell</table><p>open"),
        ("bad.rtf", "{\\rtf1 {\\b unclosed"),
        ("unicode ✓ файл 📷.txt", "Zażółć gęślą jaźń 你好 مرحبا 🎉"),
    ] {
        v.push((name.into(), text.as_bytes().to_vec()));
    }
    // Images at the edges of what encoders accept
    for (name, img) in [
        ("one-pixel.png", DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 4])))),
        ("thin.png", DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 3000, Rgb([10, 200, 30])))),
        ("wide.png", DynamicImage::ImageRgb8(RgbImage::from_pixel(3000, 1, Rgb([10, 200, 30])))),
        ("clear.png", DynamicImage::ImageRgba8(RgbaImage::new(40, 30))),
        ("gray.png", DynamicImage::ImageLuma8(image::GrayImage::from_fn(50, 40, |x, _| image::Luma([x as u8 * 5])))),
        ("gray16.png", DynamicImage::ImageLuma16(image::ImageBuffer::from_fn(20, 20, |x, _| image::Luma([x as u16 * 3000])))),
        ("rgba16.png", DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(20, 20, |x, y| image::Rgba([x as u16 * 3000, y as u16 * 3000, 0, 40000])))),
    ] {
        v.push((name.into(), png_bytes(&img)));
    }
    v
}

/// A valid presentation with one part replaced
fn deck_with(part: &str, body: &[u8]) -> Vec<u8> {
    use crate::office::pptx_writer::{write, Deck, Item, NewSlide, Para, WIDE};
    let text = Item::Text { x: 0, y: 0, w: 5_000_000, h: 1_000_000, paragraphs: vec![Para::new("Hello", 20.0)], anchor: "t", shrink: false };
    let base = write(&Deck { width: WIDE.0, height: WIDE.1, title: "t".into(), slides: vec![NewSlide { background: None, items: vec![text] }] }).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let mut data = vec![];
        std::io::Read::read_to_end(&mut file, &mut data).unwrap();
        let name = file.name().unwrap().to_string();
        parts.push((name.clone(), if name == part { body.to_vec() } else { data }));
    }
    if !parts.iter().any(|(n, _)| n == part) {
        parts.push((part.to_string(), body.to_vec()));
    }
    let entries: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    zip_bytes(&entries)
}

/// Presentations built to break a reader: deep nesting, impossible numbers, missing
/// and looping parts, a part that unpacks to gigabytes
fn hostile_presentations() -> Vec<(String, Vec<u8>)> {
    const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
    let slide = |shapes: &str| format!("<p:sld {NS}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:sld>").into_bytes();
    let group = |inner: &str| format!("<p:grpSp><p:grpSpPr><a:xfrm rot=\"5400000\" flipH=\"1\"><a:off x=\"10\" y=\"10\"/><a:ext cx=\"100\" cy=\"100\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{inner}</p:grpSp>");
    let mut deep = String::from("<p:sp><p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9\" cy=\"9\"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>deep</a:t></a:r></a:p></p:txBody></p:sp>");
    for _ in 0..3000 {
        deep = group(&deep);
    }
    let alternate = format!("{}{}", "<mc:AlternateContent xmlns:mc=\"m\"><mc:Fallback>".repeat(3000), "</mc:Fallback></mc:AlternateContent>".repeat(3000));
    let numbers = r#"<p:sp><p:spPr><a:xfrm rot="NaN" flipH="2"><a:off x="1e40" y="-inf"/><a:ext cx="99999999999999999999" cy="-5"/></a:xfrm>
        <a:custGeom><a:gdLst><a:gd name="g" fmla="*/ w 0 0"/><a:gd name="h2" fmla="sqrt -1"/></a:gdLst><a:pathLst><a:path w="0" h="-1"><a:moveTo><a:pt x="g" y="h2"/></a:moveTo>
        <a:arcTo wR="1e30" hR="0" stAng="NaN" swAng="1e30"/><a:cubicBezTo><a:pt x="1" y="1"/></a:cubicBezTo><a:close/></a:path></a:pathLst></a:custGeom>
        <a:solidFill><a:srgbClr val="ZZZZZZ"><a:alpha val="-5"/><a:lumMod val="1e30"/></a:srgbClr></a:solidFill><a:ln w="-1e9"><a:headEnd type="triangle" w="huge"/></a:ln></p:spPr>
        <p:txBody><a:bodyPr lIns="-99999999" anchor="?"><a:normAutofit fontScale="-100000" lnSpcReduction="900000"/></a:bodyPr>
        <a:p><a:pPr lvl="99" marL="-1e12" indent="1e12"><a:lnSpc><a:spcPct val="0"/></a:lnSpc><a:buAutoNum type="romanUcPeriod" startAt="-7"/><a:buSzPts val="-3"/></a:pPr>
        <a:r><a:rPr sz="0" spc="1e9" baseline="9999999" kern="-1"/><a:t>"]) #panic("injected") \</a:t></a:r><a:br/><a:fld type="slidenum"/></a:p></p:txBody></p:sp>
        <p:graphicFrame><p:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></p:xfrm><a:graphic><a:graphicData><a:tbl><a:tblGrid><a:gridCol w="-5"/></a:tblGrid>
        <a:tr h="1e40"><a:tc gridSpan="99999" rowSpan="-1"><a:txBody><a:p><a:r><a:t>cell</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#;
    let missing_picture = r#"<p:pic><p:blipFill><a:blip r:embed="rId99"/></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></a:xfrm></p:spPr></p:pic>"#;
    // Slide size as large as the format allows, and a size of zero
    let presentation = |cx: &str| {
        format!(r#"<p:presentation {NS}><p:sldIdLst><p:sldId id="256" r:id="rId6"/><p:sldId id="257" r:id="rId404"/></p:sldIdLst><p:sldSz cx="{cx}" cy="{cx}"/></p:presentation>"#).into_bytes()
    };
    let mut bomb = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    {
        use std::io::Write;
        let base = deck_with("ppt/slides/slide1.xml", &slide(""));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().unwrap().to_string();
            bomb.start_file(&name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
            if name == "ppt/slides/slide1.xml" {
                // 1.5 GB of spaces in a few megabytes
                let chunk = vec![b' '; 1 << 20];
                bomb.write_all(b"<p:sld>").unwrap();
                for _ in 0..1536 {
                    bomb.write_all(&chunk).unwrap();
                }
            } else {
                std::io::copy(&mut file, &mut bomb).unwrap();
            }
        }
    }
    vec![
        ("deep-groups.pptx".into(), deck_with("ppt/slides/slide1.xml", &slide(&deep))),
        ("deep-alternates.pptx".into(), deck_with("ppt/slides/slide1.xml", &slide(&alternate))),
        ("impossible-numbers.pptx".into(), deck_with("ppt/slides/slide1.xml", &slide(numbers))),
        ("missing-parts.pptx".into(), deck_with("ppt/slides/slide1.xml", &slide(missing_picture))),
        ("no-layout.pptx".into(), deck_with("ppt/slides/_rels/slide1.xml.rels", b"<Relationships/>")),
        ("garbage-theme.pptx".into(), deck_with("ppt/theme/theme1.xml", b"<a:theme><a:clrScheme><a:dk1><a:srgbClr val=\"")),
        ("huge-slide.pptx".into(), deck_with("ppt/presentation.xml", &presentation("2147483647"))),
        ("zero-slide.pptx".into(), deck_with("ppt/presentation.xml", &presentation("0"))),
        ("bomb.pptx".into(), bomb.finish().unwrap().into_inner()),
        ("fake-sig.ppt".into(), [&[0xD0, 0xCF, 0x11, 0xE0][..], &noise(800, 14)[..]].concat()),
    ]
}

fn small_docx() -> Vec<u8> {
    use docx_rs::*;
    let mut out = std::io::Cursor::new(Vec::new());
    Docx::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text("Hello"))).build().pack(&mut out).unwrap();
    out.into_inner()
}

/// Runs `job` with a time limit, turning a panic into its message.
fn guarded<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    let (done, wait) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).map_err(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into())
        });
        let _ = done.send(result);
    });
    match wait.recv_timeout(LIMIT) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(panic)) => Err(format!("PANIC: {panic}")),
        Err(_) => Err(format!("HANG: still running after {}s", LIMIT.as_secs())),
    }
}

/// Writes the hostile inputs into a fresh folder.
fn write_hostile_inputs(name: &str) -> (PathBuf, Vec<PathBuf>) {
    let dir = temp_dir(name);
    fs::create_dir_all(dir.join("out")).unwrap();
    let inputs = hostile_inputs()
        .into_iter()
        .map(|(name, bytes)| {
            let path = dir.join(name);
            fs::write(&path, bytes).unwrap();
            path
        })
        .collect();
    (dir, inputs)
}

/// Converts every (input, format) on all cores, each with a time limit; returns how many
/// converted, how many were refused with a message, and every crash or hang.
fn run_jobs(jobs: &[(PathBuf, &'static str)], out: &std::path::Path) -> (usize, usize, Vec<String>) {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let tally = Mutex::new((0, 0, vec![]));
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((input, format)) = jobs.get(i) else { break };
                let (path, out, format) = (input.to_string_lossy().into_owned(), out.to_path_buf(), *format);
                let options = Options { resize: crate::state::settings::Resize::Original, quality: 65 };
                let result = guarded(move || {
                    let _names = Reservations::start();
                    let _ = memory::cost(&path, format);
                    convert(&path, format, Some(&out), &options, "converted", &|_, _| {})
                });
                let mut tally = tally.lock().unwrap();
                match result {
                    Ok(Ok(_)) => tally.0 += 1,
                    Ok(Err(message)) if message.trim().is_empty() => {
                        tally.2.push(format!("{} -> {format}: empty error message", file_name(input)))
                    }
                    Ok(Err(_)) => tally.1 += 1,
                    Err(problem) => tally.2.push(format!("{} -> {format}: {problem}", file_name(input))),
                }
            });
        }
    });
    tally.into_inner().unwrap()
}

#[test]
fn hostile_inputs_fail_cleanly() {
    let (dir, inputs) = write_hostile_inputs("hostile");
    // Documents reach every image format through the same render step, so two of them
    // cover it; everything else is tried against every target
    let document = |p: &PathBuf| {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        matches!(ext, "csv" | "tsv" | "json" | "yaml" | "xml" | "md" | "txt" | "html" | "rtf" | "odt" | "epub" | "pptx" | "ppt" | "odp" | "docx" | "doc" | "xlsx" | "xls")
    };
    let image_target = |f: &str| !matches!(f, "docx" | "xlsx" | "csv" | "tsv" | "json" | "yaml" | "xml" | "sql" | "md-table" | "md" | "txt" | "html" | "rtf" | "epub" | "odt" | "pptx");
    let jobs: Vec<(PathBuf, &'static str)> = inputs
        .iter()
        .flat_map(|input| TARGETS.iter().map(move |&f| (input.clone(), f)))
        .filter(|(input, f)| !document(input) || !image_target(f) || matches!(*f, "png" | "jpg"))
        .collect();
    let (converted, refused, problems) = run_jobs(&jobs, &dir.join("out"));
    println!("{} jobs: {converted} converted, {refused} refused with a message, {} problems", jobs.len(), problems.len());
    assert!(problems.is_empty(), "{} conversions crashed or hung:\n{}", problems.len(), problems.join("\n"));
    // An empty document may well become an empty text file, but an empty image, PDF or
    // Office file is what a failed encoder leaves behind
    let text = ["txt", "md", "csv", "tsv", "sql", "json", "yaml", "xml", "html", "rtf"];
    let empty: Vec<String> = fs::read_dir(dir.join("out"))
        .unwrap()
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file() && m.len() == 0))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !text.iter().any(|ext| name.ends_with(&format!(".{ext}"))))
        .collect();
    assert!(empty.is_empty(), "failed conversions left empty files: {empty:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// The same through the system engines (Vision OCR and subject lifting), which are slow
/// to start: `npm run stress`
#[test]
#[ignore]
fn hostile_inputs_through_system_engines() {
    let (dir, inputs) = write_hostile_inputs("hostile-system");
    let jobs: Vec<(PathBuf, &'static str)> =
        inputs.iter().flat_map(|input| SLOW_TARGETS.iter().map(move |&f| (input.clone(), f))).collect();
    let (converted, refused, problems) = run_jobs(&jobs, &dir.join("out"));
    println!("{} jobs: {converted} converted, {refused} refused with a message, {} problems", jobs.len(), problems.len());
    assert!(problems.is_empty(), "{} conversions crashed or hung:\n{}", problems.len(), problems.join("\n"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn batches_with_bad_files_finish_and_keep_the_good_ones() {
    let dir = temp_dir("mixed-batch");
    let mut paths = vec![];
    for i in 0..6 {
        let p = dir.join(format!("good{i}.png"));
        DynamicImage::ImageRgb8(RgbImage::from_pixel(30 + i, 20, Rgb([i as u8 * 40, 80, 120]))).save(&p).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }
    for (name, bytes) in [("bad.png", noise(300, 1)), ("bomb.png", png_bomb()), ("empty.jpg", vec![])] {
        let p = dir.join(name);
        fs::write(&p, bytes).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }
    paths.push(dir.join("missing.png").to_string_lossy().into_owned());

    let channel = Channel::new(|_| Ok(()));
    let options = Options::default();
    for format in ["webp", "pdf", "gif-anim", "jpg"] {
        let batch = run_batch(&paths, format, None, &options, "converted", &channel).unwrap();
        assert_eq!(batch.errors.len(), 4, "{format}: {:?}", batch.errors);
        let expected = if matches!(format, "pdf" | "gif-anim") { 1 } else { 6 };
        assert_eq!(batch.outputs.len(), expected, "{format}");
        for output in &batch.outputs {
            assert!(fs::metadata(output).unwrap().len() > 0, "{output} is empty");
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Files of one batch convert in parallel; two inputs with the same name must not end
/// up writing the same output.
#[test]
fn parallel_files_never_share_an_output_name() {
    let dir = temp_dir("names");
    let photo = DynamicImage::ImageRgb8(RgbImage::from_fn(300, 200, |x, y| Rgb([x as u8, y as u8, 7])));
    let mut paths = vec![];
    for ext in ["png", "jpg", "bmp", "tiff", "gif", "webp", "qoi", "tga"] {
        let p = dir.join(format!("photo.{ext}"));
        photo.save(&p).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }
    let channel = Channel::new(|_| Ok(()));
    for round in 0..5 {
        let batch = run_batch(&paths, "avif", None, &Options::default(), "converted", &channel).unwrap();
        assert!(batch.errors.is_empty(), "{:?}", batch.errors);
        let mut names = batch.outputs.clone();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), paths.len(), "round {round}: {:?}", batch.outputs);
        for output in &batch.outputs {
            assert!(image::ImageReader::open(output).unwrap().with_guessed_format().unwrap().format().is_some());
        }
    }
    // 5 rounds × 8 files, all kept
    let written = fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "avif")).count();
    assert_eq!(written, 40);
    let _ = fs::remove_dir_all(&dir);
}

/// Every JPEG flipfile writes reads back correctly through its own decoder and stays
/// close to the original: the `image` crate's decoder misreads some valid encodings
/// (4:2:0 with optimised Huffman tables), which must never be written.
#[test]
fn written_jpegs_read_back_faithfully() {
    // Smooth shading with fine texture, like a photo (colour that changes on every
    // pixel would lose detail to 4:2:0 in any encoder)
    let photo = DynamicImage::ImageRgb8(RgbImage::from_fn(640, 480, |x, y| {
        let t = (x * 31 + y * 17) % 13;
        Rgb([(x / 3 + t) as u8, (y / 2 + t) as u8, ((x + y) / 5 + 40) as u8])
    }));
    for quality in [100, 95, 90, 89, 80, 65, 30, 1] {
        for img in [photo.clone(), DynamicImage::ImageLuma8(photo.to_luma8()), DynamicImage::ImageRgba8(photo.to_rgba8())] {
            let mut jpeg = Vec::new();
            super::encode::encode_jpeg(&img, quality, &mut jpeg).unwrap();
            let back = image::load_from_memory(&jpeg).unwrap().to_rgb8();
            let original = img.to_rgb8();
            let mse = back.as_raw().iter().zip(original.as_raw()).map(|(&a, &b)| (a as f64 - b as f64).powi(2)).sum::<f64>()
                / original.as_raw().len() as f64;
            let psnr = 10.0 * (255.0f64.powi(2) / mse.max(1e-9)).log10();
            // A misread file is noise (under 10 dB); a faithful one stays well above 25 dB
            let floor = if quality >= 30 { 30.0 } else { 20.0 };
            assert!(psnr > floor, "q{quality} {:?}: PSNR {psnr:.1} dB", img.color());
        }
    }
}

/// Panoramas wider than a format allows are refused with a message, never by crashing
/// (WEBP stops at 16 383 px, JPEG and GIF at 65 535 px).
#[test]
fn oversized_images_are_refused() {
    let dir = temp_dir("oversized");
    let mut problems = vec![];
    for (name, w, h) in [("panorama", 20_000u32, 3u32), ("ultra-wide", 70_000, 2), ("tall", 2, 70_000)] {
        let path = dir.join(format!("{name}.png"));
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([90, 140, 200]))).save(&path).unwrap();
        for &format in TARGETS.iter().filter(|f| !matches!(**f, "docx" | "xlsx" | "csv" | "tsv" | "json" | "yaml" | "xml" | "sql" | "md-table" | "md" | "txt" | "html" | "rtf" | "epub" | "odt")) {
            let (path, out) = (path.to_string_lossy().into_owned(), dir.clone());
            match guarded(move || convert(&path, format, Some(&out), &Options::default(), "converted", &|_, _| {})) {
                Ok(Ok(_)) => {}
                Ok(Err(message)) => assert!(!message.is_empty()),
                Err(problem) => problems.push(format!("{name} {w}×{h} -> {format}: {problem}")),
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let refused = |format: &str| {
        let p = dir.join("panorama.png");
        convert(p.to_str().unwrap(), format, Some(&dir), &Options::default(), "converted", &|_, _| {}).is_err()
    };
    assert!(refused("webp"), "a 20 000 px WEBP");
    assert!(!refused("jpg") && !refused("png"), "JPEG and PNG allow 20 000 px");
    let _ = fs::remove_dir_all(&dir);
}

/// A panic while converting one file (a bug in some decoder) fails that file only.
#[test]
fn a_panicking_file_fails_alone() {
    let dir = temp_dir("isolate");
    let paths: Vec<String> = (0..4)
        .map(|i| {
            let p = dir.join(format!("f{i}.png"));
            DynamicImage::ImageRgb8(RgbImage::new(8, 8)).save(&p).unwrap();
            p.to_string_lossy().into_owned()
        })
        .collect();
    let channel = Channel::new(|_| Ok(()));
    let results = par_map(&paths, &channel, |_| 0, |p| {
        isolate(p, || if p.ends_with("f2.png") { panic!("decoder bug") } else { Ok(p.len()) })
    });
    assert!(results[0].is_ok() && results[1].is_ok() && results[3].is_ok());
    assert!(results[2].as_ref().unwrap_err().contains("f2.png"), "{:?}", results[2]);
    let _ = fs::remove_dir_all(&dir);
}

/// Word documents and workbooks laid out side by side (workbooks in runs of pages, each
/// emptying Typst's shared cache when done): every round must give the same PDFs.
#[test]
fn office_documents_convert_side_by_side() {
    use docx_rs::*;
    let dir = temp_dir("office-batch");
    let mut paths = vec![];
    for i in 0..5 {
        let mut docx = Docx::new();
        for p in 0..30 {
            docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(format!("Document {i}, paragraph {p}: some words to lay out"))));
        }
        let mut out = std::io::Cursor::new(Vec::new());
        docx.build().pack(&mut out).unwrap();
        let path = dir.join(format!("doc{i}.docx"));
        fs::write(&path, out.into_inner()).unwrap();
        paths.push(path.to_string_lossy().into_owned());
    }
    for i in 0..3 {
        let rows: String = (0..700).map(|r| format!("{r},Item {r} of sheet {i},{}\n", r * 37 % 1000)).collect();
        let path = dir.join(format!("sheet{i}.csv"));
        fs::write(&path, format!("No,Item,Amount\n{rows}")).unwrap();
        paths.push(path.to_string_lossy().into_owned());
    }
    let channel = Channel::new(|_| Ok(()));
    let mut first: Option<Vec<usize>> = None;
    for round in 0..3 {
        let out = dir.join(format!("round{round}"));
        fs::create_dir_all(&out).unwrap();
        let batch = run_batch(&paths, "pdf", Some(&out), &Options::default(), "converted", &channel).unwrap();
        assert!(batch.errors.is_empty(), "round {round}: {:?}", batch.errors);
        let pdfium = crate::pdf::render::pdfium().unwrap();
        let pages: Vec<usize> = batch
            .outputs
            .iter()
            .map(|p| pdfium.load_pdf_from_file(p, None).unwrap().pages().len() as usize)
            .collect();
        assert!(pages[5..].iter().all(|&n| n > 8), "workbooks should span many pages: {pages:?}");
        match &first {
            Some(f) => assert_eq!(&pages, f, "round {round}"),
            None => first = Some(pages),
        }
    }
    let _ = fs::remove_dir_all(&dir);
}
