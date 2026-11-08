// Benchmarks of the conversion pipeline on realistic inputs. Every scenario runs in a
// process of its own, so its peak memory is measured on its own:
//
//   npm run bench                                 every scenario
//   FLIPFILE_BENCH=heic,pdf npm run bench         only scenarios whose name contains one
//   FLIPFILE_BENCH_SAVE=before.tsv npm run bench  keep the results ...
//   FLIPFILE_BENCH_BASE=before.tsv npm run bench  ... and compare a later run with them
//
// The inputs are built once into a corpus folder that later runs reuse, so before/after
// numbers always come from identical files. Photos are cut from the macOS wallpapers
// (real HEIC photos from Apple's encoder) or FLIPFILE_BENCH_PHOTO, and are synthetic
// where neither exists. The engines run warm (PDF engine loaded, fonts scanned), as in
// the app after its first conversion.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgb, RgbImage, Rgba, RgbaImage};
use tauri::ipc::Channel;

use super::{preset_suffix, run_batch};
use crate::state::settings::Settings;

/// Bump when the corpus changes, so an old one is rebuilt instead of reused.
const CORPUS_VERSION: u32 = 1;
const BATCH: usize = 24;
const HEIC_BATCH: usize = 8;

enum Inputs {
    One(&'static str),
    /// The first n photos of the batch folder
    Photos(usize),
    Heics(usize),
    /// Copies of the Word report
    Docs(usize),
}

struct Scenario {
    name: &'static str,
    inputs: Inputs,
    format: &'static str,
    preset: Option<&'static str>,
    mac_only: bool,
}

fn scenarios() -> Vec<Scenario> {
    use Inputs::*;
    let s = |name, inputs, format, preset| Scenario { name, inputs, format, preset, mac_only: false };
    let mac = |name, inputs, format| Scenario { name, inputs, format, preset: None, mac_only: true };
    vec![
        s("photo 12mp jpg->jpg", One("photo.jpg"), "jpg", None),
        s("photo 12mp jpg->png", One("photo.jpg"), "png", None),
        s("photo 12mp jpg->webp", One("photo.jpg"), "webp", None),
        s("photo 12mp jpg->avif", One("photo.jpg"), "avif", None),
        s("photo 12mp jpg->pdf", One("photo.jpg"), "pdf", None),
        s("photo 12mp compress", One("photo.jpg"), "same", Some("compress")),
        s("photo 12mp resize 1080p", One("photo.jpg"), "same", Some("1080p")),
        s("photo 12mp cmp-200k", One("photo.jpg"), "cmp-200k", None),
        s("photo 12mp lossless", One("photo.jpg"), "cmp-lossless", None),
        s("photo 48mp jpg->jpg", One("photo48.jpg"), "jpg", None),
        mac("photo 12mp jpg->heic", One("photo.jpg"), "heic"),
        mac("photo 12mp heic->jpg", One("photo.heic"), "jpg"),
        mac("photo 36mp p3 heic->jpg", One("p3.heic"), "jpg"),
        s("screenshot 5k png->jpg", One("screenshot.png"), "jpg", None),
        s("screenshot 5k compress", One("screenshot.png"), "same", Some("compress")),
        s("screenshot 5k ->webp", One("screenshot.png"), "webp", None),
        s("screenshot 5k ->avif", One("screenshot.png"), "avif", None),
        s("logo 2k ico", One("logo.png"), "ico", None),
        s("logo 2k icns", One("logo.png"), "icns", None),
        s("batch24 resize 1080p", Photos(BATCH), "same", Some("1080p")),
        s("batch24 jpg->webp", Photos(BATCH), "webp", None),
        s("batch24 merge->pdf", Photos(BATCH), "pdf", None),
        s("batch12 gif-anim", Photos(12), "gif-anim", None),
        mac("batch8 heic->jpg", Heics(HEIC_BATCH), "jpg"),
        mac("upscale 0.4mp 4x", One("small.jpg"), "up-4x"),
        mac("upscale 1080p 2x", One("fullhd.jpg"), "up-2x"),
        mac("upscale 12mp 2x", One("photo.jpg"), "up-2x"),
        s("pdf 24p ->png", One("doc.pdf"), "png", None),
        s("pdf 24p ->jpg", One("doc.pdf"), "jpg", None),
        s("pdf 24p ->docx", One("doc.pdf"), "docx", None),
        s("docx ->pdf", One("report.docx"), "pdf", None),
        s("batch8 docx->pdf", Docs(8), "pdf", None),
        s("xlsx ->pdf", One("sheet.xlsx"), "pdf", None),
        s("csv 15k rows ->pdf", One("big.csv"), "pdf", None),
    ]
}

fn input_paths(corpus: &Path, inputs: &Inputs) -> Vec<String> {
    let numbered = |dir: &str, n: usize, ext: &str| -> Vec<String> {
        (1..=n).map(|i| corpus.join(dir).join(format!("p{i:02}.{ext}")).to_string_lossy().into_owned()).collect()
    };
    match inputs {
        Inputs::One(name) => vec![corpus.join(name).to_string_lossy().into_owned()],
        Inputs::Photos(n) => numbered("photos", *n, "jpg"),
        Inputs::Heics(n) => numbered("heics", *n, "heic"),
        Inputs::Docs(n) => numbered("docs", *n, "docx"),
    }
}

// ---------------------------------------------------------------------------
// Driver: one child process per scenario

#[test]
#[ignore]
fn bench() {
    let corpus = corpus();
    let filters: Vec<String> = std::env::var("FLIPFILE_BENCH")
        .map(|f| f.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let base = std::env::var("FLIPFILE_BENCH_BASE").ok().map(|p| read_results(Path::new(&p))).unwrap_or_default();
    let exe = std::env::current_exe().unwrap();

    println!("\ncorpus: {}\n", corpus.display());
    println!("{:<26} {:>9} {:>10} {:>11} {:>11}  result", "scenario", "time", "peak RAM", "output", "footprint");
    let mut results = vec![];
    for scenario in scenarios() {
        if scenario.mac_only && !cfg!(target_os = "macos") {
            continue;
        }
        if !filters.is_empty() && !filters.iter().any(|f| scenario.name.contains(f.as_str())) {
            continue;
        }
        let out = Command::new(&exe)
            .args(["convert::bench::bench_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
            .env("FLIPFILE_BENCH_RUN", scenario.name)
            .env("FLIPFILE_BENCH_CORPUS", &corpus)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let Some(result) = stdout.lines().find_map(BenchResult::parse) else {
            println!("{:<26} crashed: {}", scenario.name, String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("?"));
            continue;
        };
        let compare = |now: f64, before: Option<f64>| match before {
            Some(b) if b > 0.0 => format!(" ({:+.0}%)", (now - b) / b * 100.0),
            _ => String::new(),
        };
        let before = base.iter().find(|b| b.name == result.name);
        println!(
            "{:<26} {:>7.2}s{:<7} {:>6.0} MB{:<7} {:>8.2} MB{:<7} {:>6.0} MB{:<7}  {}",
            result.name,
            result.seconds,
            compare(result.seconds, before.map(|b| b.seconds)),
            result.peak_mb,
            compare(result.peak_mb, before.map(|b| b.peak_mb)),
            result.output_mb,
            compare(result.output_mb, before.map(|b| b.output_mb)),
            result.footprint_mb,
            compare(result.footprint_mb, before.map(|b| b.footprint_mb)),
            result.summary
        );
        results.push(result);
    }
    if let Ok(path) = std::env::var("FLIPFILE_BENCH_SAVE") {
        let lines: Vec<String> = results.iter().map(BenchResult::line).collect();
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        println!("\nsaved to {path}");
    }
}

/// Runs the scenario named by FLIPFILE_BENCH_RUN (started by `bench`, never on its own).
#[test]
#[ignore]
fn bench_child() {
    let Ok(name) = std::env::var("FLIPFILE_BENCH_RUN") else { return };
    let corpus = PathBuf::from(std::env::var("FLIPFILE_BENCH_CORPUS").unwrap());
    let scenario = scenarios().into_iter().find(|s| s.name == name).expect("unknown scenario");
    let paths = input_paths(&corpus, &scenario.inputs);
    let options = Settings::load(None).options(scenario.preset);
    let out = std::env::temp_dir().join(format!("flipfile-bench-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).unwrap();

    // Warm engines, as in a running app
    crate::pdf::render::set_library_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pdfium"));
    let _ = crate::pdf::render::pdfium();
    let _ = crate::office::typeset::has_font("Arial");

    let channel = Channel::new(|_| Ok(()));
    let start = Instant::now();
    let batch = run_batch(&paths, scenario.format, Some(&out), &options, preset_suffix(scenario.preset), &channel);
    let seconds = start.elapsed().as_secs_f64();
    let summary = match &batch {
        Ok(b) if b.errors.is_empty() => format!("ok, {} output(s)", b.outputs.len()),
        Ok(b) => format!("{} error(s): {}", b.errors.len(), b.errors[0]),
        Err(e) => format!("failed: {e}"),
    };
    let result = BenchResult {
        name: scenario.name.to_string(),
        seconds,
        peak_mb: peak_rss() as f64 / 1e6,
        output_mb: dir_size(&out) as f64 / 1e6,
        footprint_mb: peak_footprint() as f64 / 1e6,
        summary,
    };
    let _ = fs::remove_dir_all(&out);
    println!("{}", result.line());
}

struct BenchResult {
    name: String,
    seconds: f64,
    /// Peak resident memory
    peak_mb: f64,
    output_mb: f64,
    /// Peak footprint (0 in results saved before it was measured)
    footprint_mb: f64,
    summary: String,
}

impl BenchResult {
    fn line(&self) -> String {
        format!(
            "BENCH\t{}\t{:.4}\t{:.1}\t{:.4}\t{:.1}\t{}",
            self.name, self.seconds, self.peak_mb, self.output_mb, self.footprint_mb, self.summary
        )
    }

    fn parse(line: &str) -> Option<Self> {
        // The test harness prints "test … ... " in front of the child's output
        let mut f = line[line.find("BENCH\t")? + 6..].split('\t');
        let (name, seconds, peak_mb, output_mb) =
            (f.next()?.to_string(), f.next()?.parse().ok()?, f.next()?.parse().ok()?, f.next()?.parse().ok()?);
        let rest: Vec<&str> = f.collect();
        let (footprint_mb, summary) = match rest.first().and_then(|v| v.parse().ok()) {
            Some(footprint) => (footprint, rest[1..].join("\t")),
            None => (0.0, rest.join("\t")),
        };
        Some(BenchResult { name, seconds, peak_mb, output_mb, footprint_mb, summary })
    }
}

fn read_results(path: &Path) -> Vec<BenchResult> {
    fs::read_to_string(path).map(|t| t.lines().filter_map(BenchResult::parse).collect()).unwrap_or_default()
}

/// Highest memory footprint of this process so far, in bytes: what Activity Monitor
/// shows as "Memory" (not counting font files and libraries mapped from disk, which the
/// system can drop at any time). Peak resident memory elsewhere.
fn peak_footprint() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let buffer = (&mut info as *mut libc::rusage_info_v4).cast::<libc::rusage_info_t>();
        if unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V4, buffer) } == 0 {
            return info.ri_lifetime_max_phys_footprint;
        }
    }
    peak_rss()
}

/// Highest resident memory of this process so far, in bytes.
fn peak_rss() -> u64 {
    #[cfg(unix)]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        let max = usage.ru_maxrss as u64;
        // macOS reports bytes, Linux kilobytes
        if cfg!(target_os = "macos") { max } else { max * 1024 }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => dir_size(&e.path()),
                    _ => e.metadata().map(|m| m.len()).unwrap_or(0),
                })
                .sum()
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Corpus

fn corpus() -> PathBuf {
    let dir = std::env::var_os("FLIPFILE_BENCH_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("flipfile-bench-corpus-v{CORPUS_VERSION}")));
    if dir.join("ready").exists() {
        add_documents(&dir);
        return dir;
    }
    println!("building the corpus in {} (once)…", dir.display());
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("photos")).unwrap();

    let source = source_photo();
    // 12 MP (4032 × 3024, the size of most phone photos), cut from the source photo
    let photo = |i: u32| phone_photo(&source, i);
    write_jpeg(&photo(0), &dir.join("photo.jpg"));
    for i in 1..=BATCH {
        write_jpeg(&photo(i as u32), &dir.join(format!("photos/p{i:02}.jpg")));
    }
    // 48 MP, like the main camera of recent phones
    let big = photo(99).resize_exact(8064, 6048, FilterType::Triangle);
    write_jpeg(&add_grain(big, 7), &dir.join("photo48.jpg"));
    screenshot(&source).save(dir.join("screenshot.png")).unwrap();
    logo().save(dir.join("logo.png")).unwrap();
    fs::write(dir.join("doc.pdf"), document_pdf(&source)).unwrap();
    fs::write(dir.join("report.docx"), report_docx(&source)).unwrap();
    spreadsheet(&dir);
    add_documents(&dir);

    // HEIC through Apple's own encoder, as an iPhone writes them
    #[cfg(target_os = "macos")]
    {
        fs::create_dir_all(dir.join("heics")).unwrap();
        sips_heic(&dir.join("photo.jpg"), &dir.join("photo.heic"));
        for i in 1..=HEIC_BATCH {
            sips_heic(&dir.join(format!("photos/p{i:02}.jpg")), &dir.join(format!("heics/p{i:02}.heic")));
        }
        // A large Display P3 photo (wide-gamut, like every iPhone photo)
        let p3 = Path::new("/System/Library/Desktop Pictures/Sonoma.heic");
        if p3.exists() {
            fs::copy(p3, dir.join("p3.heic")).unwrap();
        } else {
            fs::copy(dir.join("photo.heic"), dir.join("p3.heic")).unwrap();
        }
    }
    fs::write(dir.join("ready"), b"").unwrap();
    dir
}

/// The macOS aerial landscape wallpaper (a real 4K photo), FLIPFILE_BENCH_PHOTO, or a
/// synthetic landscape.
fn source_photo() -> DynamicImage {
    if let Some(path) = std::env::var_os("FLIPFILE_BENCH_PHOTO") {
        return image::open(path).expect("FLIPFILE_BENCH_PHOTO is not an image");
    }
    #[cfg(target_os = "macos")]
    for wallpaper in [
        "/System/Library/Desktop Pictures/.wallpapers/Sonoma Horizon/Sonoma Horizon.heic",
        "/System/Library/Desktop Pictures/Sonoma.heic",
    ] {
        if let Ok(img) = crate::platform::system_codec::decode(Path::new(wallpaper)) {
            return img;
        }
    }
    synthetic_landscape(3840, 2160)
}

/// A 4:3 crop of `source` (shifted by `i`), at 4032 × 3024 with a little sensor grain.
fn phone_photo(source: &DynamicImage, i: u32) -> DynamicImage {
    let (w, h) = source.dimensions();
    let (cw, ch) = if w * 3 > h * 4 { (h * 4 / 3, h) } else { (w, w * 3 / 4) };
    let x = (i * 97) % (w - cw + 1);
    let y = (i * 53) % (h - ch + 1);
    let crop = source.crop_imm(x, y, cw, ch).resize_exact(4032, 3024, FilterType::Triangle);
    add_grain(crop, i)
}

/// Light noise, like a camera sensor's, so encoders see photo-like detail.
fn add_grain(img: DynamicImage, seed: u32) -> DynamicImage {
    let mut rgb = img.to_rgb8();
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    for p in rgb.pixels_mut() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let n = (state % 7) as i16 - 3;
        for c in p.0.iter_mut() {
            *c = (*c as i16 + n).clamp(0, 255) as u8;
        }
    }
    DynamicImage::ImageRgb8(rgb)
}

fn write_jpeg(img: &DynamicImage, path: &Path) {
    let file = fs::File::create(path).unwrap();
    img.to_rgb8().write_with_encoder(JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 92)).unwrap();
}

fn synthetic_landscape(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let ridge = 0.55 + 0.08 * (fx * 9.0).sin() + 0.04 * (fx * 23.0 + 1.0).sin();
        if fy < ridge {
            let sky = 1.0 - fy / ridge;
            Rgb([(120.0 + 90.0 * sky) as u8, (150.0 + 80.0 * sky) as u8, (200.0 + 50.0 * sky) as u8])
        } else {
            let t = ((x * 7 + y * 13) % 31) as f32 / 31.0;
            Rgb([(40.0 + 50.0 * t) as u8, (90.0 + 60.0 * t * (fy - ridge + 0.5)) as u8, (30.0 + 20.0 * t) as u8])
        }
    }))
}

/// A 5K desktop screenshot: window chrome, lines of "text", buttons and a photo.
fn screenshot(source: &DynamicImage) -> RgbaImage {
    let (w, h) = (5120u32, 2880u32);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([236, 236, 240, 255]));
    let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 4]| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                img.put_pixel(x, y, Rgba(c));
            }
        }
    };
    fill(0, 0, w, 56, [214, 214, 220, 255]); // menu bar
    fill(0, 56, 520, h, [246, 246, 248, 255]); // sidebar
    fill(600, 140, 4900, 2760, [255, 255, 255, 255]); // window
    let mut seed = 12345u32;
    let mut rand = move |m: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % m
    };
    // Paragraphs of words
    for line in 0..70 {
        let y = 260 + line * 34;
        let mut x = 680;
        while x < 2900 {
            let word = 30 + rand(110);
            fill(x, y, x + word, y + 16, [40, 40, 46, 255]);
            x += word + 14;
        }
    }
    // Sidebar items and colored buttons
    for i in 0..24 {
        fill(60, 120 + i * 60, 60 + 120 + rand(220), 140 + i * 60, [70, 70, 80, 255]);
    }
    for i in 0..6 {
        let c = [[0, 122, 255, 255], [52, 199, 89, 255], [255, 59, 48, 255]][(i % 3) as usize];
        fill(3100 + i * 260, 2600, 3330 + i * 260, 2680, c);
    }
    drop(fill);
    // A photo in the document
    let thumb = source.resize_exact(1700, 1100, FilterType::Triangle).to_rgba8();
    image::imageops::overlay(&mut img, &thumb, 3050, 300);
    img
}

/// A 2048 px app logo with a transparent background.
fn logo() -> RgbaImage {
    let s = 2048u32;
    RgbaImage::from_fn(s, s, |x, y| {
        let (dx, dy) = (x as f32 - 1024.0, y as f32 - 1024.0);
        let r = (dx * dx + dy * dy).sqrt();
        if r < 900.0 {
            let a = if r > 896.0 { ((900.0 - r) / 4.0 * 255.0) as u8 } else { 255 };
            Rgba([(40.0 + 160.0 * x as f32 / s as f32) as u8, 90, (220.0 - 120.0 * y as f32 / s as f32) as u8, a])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

/// 24 pages of text with headings, a table and photos, typeset by the built-in engine.
fn document_pdf(source: &DynamicImage) -> Vec<u8> {
    let mut jpeg = Vec::new();
    source
        .resize(1600, 1600, FilterType::Triangle)
        .to_rgb8()
        .write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, 85))
        .unwrap();
    let mut markup = String::from("#set page(paper: \"a4\", margin: 2cm)\n#set text(size: 11pt)\n");
    for page in 1..=24 {
        markup.push_str(&format!("= Chapter {page}\n\n#lorem(180)\n\n#lorem(120)\n\n"));
        if page % 3 == 0 {
            markup.push_str("#image(\"/img/photo.jpg\", width: 70%)\n\n");
        } else {
            markup.push_str("#table(columns: 4, [Item], [Qty], [Price], [Total], [Coffee], [2], [25000], [50000], [Tea], [3], [18000], [54000])\n\n");
        }
        markup.push_str("#pagebreak()\n");
    }
    crate::office::typeset::to_pdf(markup, vec![("/img/photo.jpg".into(), jpeg)]).unwrap()
}

/// A Word report of ~15 pages: headings, paragraphs, a table and a photo.
fn report_docx(source: &DynamicImage) -> Vec<u8> {
    use docx_rs::*;
    let mut png = Vec::new();
    source
        .resize(800, 800, FilterType::Triangle)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let text = "Revenue grew by twelve percent this quarter, driven by strong demand in every region. \
                Operating costs stayed flat while the team shipped three major releases on schedule. ";
    let mut docx = Docx::new();
    for section in 1..=15 {
        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(format!("Section {section}")).bold().size(32)));
        for _ in 0..6 {
            docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(text.repeat(3))));
        }
        let cell = |t: &str| TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(t)));
        let rows = (0..8).map(|r| TableRow::new(vec![cell(&format!("Item {r}")), cell("12"), cell("25.000"), cell("300.000")])).collect();
        docx = docx.add_table(Table::new(rows));
        if section % 5 == 0 {
            docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_image(Pic::new(&png))));
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    docx.build().pack(&mut out).unwrap();
    out.into_inner()
}

/// Copies of the report, for a batch of documents, and a long CSV (added to older corpora too)
fn add_documents(dir: &Path) {
    // Photos to upscale, sharp at their size: as small as old web photos, and Full HD
    for (name, w, h) in [("small.jpg", 800, 450), ("fullhd.jpg", 1920, 1080)] {
        if !dir.join(name).exists() {
            write_jpeg(&source_photo().resize_exact(w, h, FilterType::Lanczos3), &dir.join(name));
        }
    }
    let big = dir.join("big.csv");
    if !big.exists() {
        let mut text = String::from("Date,Account,Description,Region,Qty,Price,Tax,Total\n");
        for r in 0..15_000u64 {
            let (qty, price) = (r % 17 + 1, 1000 + (r * 37) % 90_000);
            text.push_str(&format!(
                "2026-{:02}-{:02},ACC-{:04},Payment for invoice {r},Region {},{qty},{price},{},{}\n",
                r % 12 + 1, r % 28 + 1, r % 97, r % 9, price * qty / 10, price * qty * 11 / 10
            ));
        }
        fs::write(&big, text).unwrap();
    }
    let docs = dir.join("docs");
    if !docs.exists() {
        fs::create_dir_all(&docs).unwrap();
        for i in 1..=8 {
            fs::copy(dir.join("report.docx"), docs.join(format!("p{i:02}.docx"))).unwrap();
        }
    }
}

/// 3000 rows × 8 columns, written to XLSX by the pipeline's own CSV -> XLSX conversion.
fn spreadsheet(dir: &Path) {
    let csv = dir.join("sheet.csv");
    let mut text = String::from("Date,Account,Description,Region,Qty,Price,Tax,Total\n");
    for r in 0..3000 {
        let qty = r % 17 + 1;
        let price = 1000 + (r * 37) % 90000;
        text.push_str(&format!(
            "2026-{:02}-{:02},ACC-{:04},Payment for invoice {r},Region {},{qty},{price},{},{}\n",
            r % 12 + 1,
            r % 28 + 1,
            r % 97,
            r % 9,
            price * qty / 10,
            price * qty * 11 / 10
        ));
    }
    fs::write(&csv, text).unwrap();
    let out = super::convert(csv.to_str().unwrap(), "xlsx", Some(dir), &Default::default(), "converted", &|_, _| {}).unwrap();
    fs::rename(&out.path, dir.join("sheet.xlsx")).unwrap();
    fs::remove_file(&csv).unwrap();
}

#[cfg(target_os = "macos")]
fn sips_heic(input: &Path, output: &Path) {
    let status = Command::new("sips").args(["-s", "format", "heic"]).arg(input).arg("--out").arg(output).output().unwrap();
    assert!(status.status.success(), "sips could not write HEIC");
}
