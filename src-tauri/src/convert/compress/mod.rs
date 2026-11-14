// Compress levels (the Option/Alt ring): Light, Balanced, Strong and Extreme, for photos,
// PDFs, videos and recordings alike, each in its own format where that can get smaller.
// Before the user lets go, `compress_estimate` works out what each level would make of
// the files, and says what it costs ("4.2 MB → 610 KB · fine detail softens").
//
// - photo: re-encoded at the level's quality and size (lossless formats become JPG, or
//   WebP when transparent, from Balanced on)
// - pdf:   the pictures inside re-encoded at the level's resolution; text and layout stay
// - av:    video and audio at the level's bitrate and resolution (ffmpeg, else the
//          system's own engines)
// - estimate: the live estimates

mod av;
pub mod estimate;
mod pdf;
mod photo;

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
    Video,
    Audio,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    if pdf_render::is_pdf(&header(path)) {
        return Some(Kind::Pdf);
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
    super::input::same_format_ext(path).is_ok().then_some(Kind::Photo)
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
    let kind = kind_of(input).ok_or_else(|| format!("\"{name}\": photos, PDFs, videos and recordings can be compressed"))?;
    let result = match kind {
        Kind::Photo => photo::compress(input, level, out_dir)?,
        Kind::Pdf => pdf::compress(input, level, out_dir)?,
        Kind::Video => av::compress_video(input, level, out_dir, on_step)?,
        Kind::Audio => av::compress_audio(input, level, out_dir)?,
    };
    finish(input, result, kind, level, out_dir)
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
