// Documents that are ZIP packages, at a compress level: Word, Excel and PowerPoint (DOCX,
// XLSX, PPTX and their macro and template variants), OpenDocument (ODT, ODS, ODP, ODG)
// and EPUB books. Their bytes are almost always in what was pasted into them: photos at
// camera resolution, screenshots, videos. So, as Word's own "Compress Pictures" does:
// - each picture is scaled down to the level's resolution at the largest size it is shown
//   in the document (crops included); one whose size isn't known is fitted to a page, a
//   book's to a screen
// - photos stored losslessly (PNG, BMP, TIFF) become JPEG from Balanced on; drawings,
//   screenshots and transparent pictures stay PNG, BMP and TIFF become PNG
// - videos and recordings get the level's bitrate, as compressing a video does
// - every part is deflated at the highest level
// Text, styles, formulas, charts, macros and layout aren't touched. A picture that changes
// format is renamed (image3.png → image3.jpeg) and its references and content type are
// rewritten; one whose references can't all be found safely keeps its format. Signed
// documents are refused: any change would break the signature.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{av, Level, Made};
use crate::convert::encode::{encode_jpeg, encode_png_oxipng_bytes, has_transparency, looks_like_graphic, resize_exact};
use crate::convert::input::{file_name, header, to_srgb};
use crate::convert::output::{save_err, unique_output_path};
use crate::convert::OnPage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Word, Excel and PowerPoint
    Office,
    /// ODT, ODS, ODP, ODG
    OpenDocument,
    /// EPUB
    Book,
}

pub fn family(path: &Path) -> Option<Family> {
    if !header(path).starts_with(b"PK\x03\x04") {
        return None;
    }
    match ext_of(&path.to_string_lossy()).as_str() {
        "docx" | "docm" | "dotx" | "dotm" | "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "pptx" | "pptm" | "ppsx" | "ppsm" | "potx"
        | "potm" => Some(Family::Office),
        "odt" | "ott" | "ods" | "ots" | "odp" | "otp" | "odg" | "otg" => Some(Family::OpenDocument),
        "epub" => Some(Family::Book),
        _ => None,
    }
}

/// Resolution of pictures where they are shown (pixels per inch), JPEG quality, and the
/// longest side of a book's pictures
fn params(level: Level) -> (f64, u8, u32) {
    match level {
        Level::Light => (200.0, 80, 2400),
        Level::Balanced => (150.0, 70, 1800),
        Level::Strong => (110.0, 55, 1400),
        Level::Extreme => (72.0, 40, 1000),
    }
}

/// A picture whose shown size isn't known is fitted to this many inches: a widescreen
/// slide, wider than any page
const FALLBACK_INCHES: f64 = 13.33;
/// A picture is only rewritten when that saves at least this share of its bytes
const WORTH_IT: f64 = 0.9;
/// Pictures larger than this many pixels when decoded are left alone (memory)
const MAX_PIXELS: u64 = 120_000_000;
/// Videos and recordings smaller than this aren't worth re-encoding
const MIN_VIDEO: usize = 1024 * 1024;
const MIN_AUDIO: usize = 256 * 1024;
/// Packages that unpack to more than this are refused (memory, and ZIP bombs)
const MAX_UNPACKED: u64 = 3 * 1024 * 1024 * 1024;
/// Work units of a video and a recording, against 1 for a picture (progress)
const VIDEO_WORK: usize = 20;
const AUDIO_WORK: usize = 3;

const PICTURES: &[&str] = &["jpg", "jpeg", "jpe", "png", "bmp", "dib", "tif", "tiff"];
const VIDEOS: &[&str] = &["mp4", "m4v", "mov"];
const RECORDINGS: &[&str] = &["m4a", "mp3", "wav", "aac", "aif", "aiff"];

/// What becomes of embedded videos and recordings
pub enum Media<'a> {
    /// Re-encoded, with progress
    Encode(OnPage<'a>),
    /// Kept, and their saving worked out from their length (the live estimates)
    Estimate,
}

pub struct Packed {
    pub bytes: Vec<u8>,
    /// What the embedded videos and recordings would lose, with `Media::Estimate`
    pub media_saving: u64,
}

pub(super) fn compress(input: &Path, level: Level, out_dir: Option<&Path>, on_step: OnPage) -> Result<Made, String> {
    let name = file_name(input);
    let family = family(input).ok_or_else(|| format!("\"{name}\" is not a document that can be compressed"))?;
    let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
    let packed = compress_bytes(&data, family, level, Media::Encode(on_step)).map_err(|e| format!("\"{name}\": {e}"))?;
    let own = ext_of(&input.to_string_lossy());
    let path = unique_output_path(input, out_dir, "compressed", &own);
    fs::write(&path, packed.bytes).map_err(save_err)?;
    Ok(Made { path, changed: None })
}

struct Entry {
    name: String,
    data: Vec<u8>,
    modified: Option<zip::DateTime>,
    dir: bool,
}

/// What a picture, video or recording became: its bytes, and its new extension when that
/// changed
type Shrunk = Option<(Vec<u8>, Option<&'static str>)>;

pub fn compress_bytes(data: &[u8], family: Family, level: Level, media: Media) -> Result<Packed, String> {
    let mut entries = read_entries(data)?;
    if signed(family, &entries) {
        return Err("signed documents can't be compressed without breaking their signature".into());
    }
    let (dpi, quality, book_edge) = params(level);
    let shown = shown_sizes(family, &entries);
    let texts: Vec<usize> = (0..entries.len()).filter(|&i| is_text_part(family, &entries[i].name)).collect();
    let names: HashSet<String> = entries.iter().map(|e| e.name.to_lowercase()).collect();
    let mut basenames: HashMap<String, usize> = HashMap::new();
    for entry in &entries {
        *basenames.entry(basename(&entry.name).to_lowercase()).or_default() += 1;
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Job {
        Picture,
        Video,
        Recording,
    }
    let mut jobs: Vec<(usize, Job, bool)> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let ext = ext_of(&entry.name);
        let job = if PICTURES.contains(&ext.as_str()) {
            Job::Picture
        } else if VIDEOS.contains(&ext.as_str()) && entry.data.len() >= MIN_VIDEO {
            Job::Video
        } else if RECORDINGS.contains(&ext.as_str()) && entry.data.len() >= MIN_AUDIO {
            Job::Recording
        } else {
            continue;
        };
        if entry.dir || (entry.name.starts_with("META-INF/") && family != Family::Office) {
            continue;
        }
        // Renamed only when its name is plain and its own, the new names are free, and
        // every reference to it is one that can be rewritten
        let base = basename(&entry.name);
        let stem = &entry.name[..entry.name.len() - ext.len()];
        let may_rename = base.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            && basenames.get(&base.to_lowercase()) == Some(&1)
            && ["jpeg", "jpg", "png", "mp4", "m4a"].iter().all(|new| new == &ext || !names.contains(&format!("{stem}{new}").to_lowercase()))
            && {
                let (inside, outside) = texts.iter().fold((0, 0), |(i, o), &t| {
                    let (a, b) = count_refs(&entries[t], base);
                    (i + a, o + b)
                });
                inside > 0 && outside == 0
            };
        jobs.push((i, job, may_rename));
    }

    let total_work: usize = jobs
        .iter()
        .map(|(_, job, _)| match job {
            Job::Picture => 1,
            Job::Video => VIDEO_WORK,
            Job::Recording => AUDIO_WORK,
        })
        .sum();
    let done = AtomicUsize::new(0);
    let report = |units: usize| {
        if let Media::Encode(on_step) = &media {
            on_step(done.fetch_add(units, Ordering::Relaxed) + units, total_work.max(1));
        }
    };

    // Pictures, side by side
    let pictures: Vec<(usize, bool)> = jobs.iter().filter(|(_, j, _)| *j == Job::Picture).map(|&(i, _, r)| (i, r)).collect();
    let results: Mutex<Vec<(usize, Shrunk)>> = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 4).min(pictures.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(&(i, may_rename)) = pictures.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let entry = &entries[i];
                    let target = match family {
                        Family::Book => Target::Edge(book_edge),
                        _ => match shown.get(&entry.name) {
                            Some(Some((w, h))) => Target::Shown(w * dpi, h * dpi),
                            _ => Target::Edge((FALLBACK_INCHES * dpi).round() as u32),
                        },
                    };
                    let jpeg_ext = if family == Family::Office { "jpeg" } else { "jpg" };
                    let shrunk = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shrink_picture(&entry.data, &ext_of(&entry.name), target, quality, level, may_rename, jpeg_ext)))
                        .ok()
                        .flatten();
                    results.lock().unwrap().push((i, shrunk));
                    report(1);
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();

    // Videos and recordings one at a time: each already uses every core
    let mut media_saving = 0u64;
    for &(i, job, may_rename) in jobs.iter().filter(|(_, j, _)| *j != Job::Picture) {
        let units = if job == Job::Video { VIDEO_WORK } else { AUDIO_WORK };
        let entry = &entries[i];
        match &media {
            Media::Estimate => media_saving += estimate_media(entry, job == Job::Video, level).unwrap_or(0),
            Media::Encode(on_step) => {
                let base = done.load(Ordering::Relaxed);
                let step = |d: usize, t: usize| on_step(base + units * d / t.max(1), total_work.max(1));
                let shrunk = shrink_media(entry, job == Job::Video, level, may_rename, &step);
                results.push((i, shrunk));
            }
        }
        report(units);
    }

    // The results go in; renamed parts take their references along
    let mut renames: Vec<Rename> = Vec::new();
    for (i, shrunk) in results {
        let Some((bytes, new_ext)) = shrunk else { continue };
        let entry = &mut entries[i];
        if let Some(new_ext) = new_ext {
            let ext = ext_of(&entry.name);
            let renamed = format!("{}{new_ext}", &entry.name[..entry.name.len() - ext.len()]);
            renames.push(Rename { old: basename(&entry.name).to_string(), new: basename(&renamed).to_string(), mime: mime_of(new_ext) });
            entry.name = renamed;
        }
        entry.data = bytes;
    }
    if !renames.is_empty() {
        for &t in &texts {
            let entry = &mut entries[t];
            let Ok(text) = std::str::from_utf8(&entry.data) else { continue };
            if let Some(rewritten) = rewrite(text, &renames, ext_of(&entry.name) == "css") {
                if is_xml(&entry.name) && roxmltree::Document::parse(text).is_ok() && roxmltree::Document::parse(&rewritten).is_err() {
                    return Err("this document couldn't be compressed safely".into());
                }
                entry.data = rewritten.into_bytes();
            }
        }
        if family == Family::Office {
            add_content_types(&mut entries, &renames)?;
        }
    }

    let count = entries.len();
    let bytes = write_package(&mut entries, family)?;
    let reopened = ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|_| "this document couldn't be compressed safely".to_string())?;
    if reopened.len() != count {
        return Err("this document couldn't be compressed safely".into());
    }
    Ok(Packed { bytes, media_saving })
}

fn read_entries(data: &[u8]) -> Result<Vec<Entry>, String> {
    let damaged = || "this document is damaged or isn't a document".to_string();
    let mut zip = ZipArchive::new(Cursor::new(data)).map_err(|_| damaged())?;
    let mut entries = Vec::with_capacity(zip.len());
    let mut unpacked = 0u64;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|_| damaged())?;
        if file.encrypted() {
            return Err("password-protected documents can't be compressed".into());
        }
        unpacked += file.size();
        if unpacked > MAX_UNPACKED {
            return Err("this document is too large to compress".into());
        }
        let name = file.name().map_err(|_| damaged())?.to_string();
        let mut data = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut data).map_err(|_| damaged())?;
        entries.push(Entry { dir: file.is_dir(), modified: file.last_modified(), name, data });
    }
    Ok(entries)
}

fn signed(family: Family, entries: &[Entry]) -> bool {
    entries.iter().any(|e| match family {
        Family::Office => e.name.starts_with("_xmlsignatures/"),
        Family::OpenDocument => matches!(e.name.as_str(), "META-INF/documentsignatures.xml" | "META-INF/macrosignatures.xml"),
        Family::Book => e.name == "META-INF/signatures.xml",
    })
}

/// The ZIP, every part deflated at the highest level; `mimetype` first and stored, as
/// OpenDocument and EPUB require
fn write_package(entries: &mut [Entry], family: Family) -> Result<Vec<u8>, String> {
    let failed = |_| "this document couldn't be written".to_string();
    if let Some(at) = entries.iter().position(|e| e.name == "mimetype").filter(|_| family != Family::Office) {
        entries[..=at].rotate_right(1);
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for entry in entries.iter() {
        let mut options = SimpleFileOptions::default();
        if let Some(modified) = entry.modified {
            options = options.last_modified_time(modified);
        }
        if entry.dir {
            zip.add_directory(entry.name.as_str(), options).map_err(failed)?;
            continue;
        }
        options = if entry.name == "mimetype" && family != Family::Office {
            options.compression_method(CompressionMethod::Stored)
        } else {
            // Pictures and videos are compressed already: deflating them hard gains nothing
            let packed = PICTURES.contains(&ext_of(&entry.name).as_str()) || VIDEOS.contains(&ext_of(&entry.name).as_str());
            options.compression_method(CompressionMethod::Deflated).compression_level(Some(if packed { 6 } else { 9 }))
        };
        options = options.large_file(entry.data.len() as u64 >= u64::from(u32::MAX));
        zip.start_file(entry.name.as_str(), options).map_err(failed)?;
        zip.write_all(&entry.data).map_err(|_| "this document couldn't be written".to_string())?;
    }
    Ok(zip.finish().map_err(failed)?.into_inner())
}

// ─── Pictures ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    /// Pixels wide and high where it is shown
    Shown(f64, f64),
    /// The longest side at most
    Edge(u32),
}

/// The picture at the level, when that is safe and worth it
fn shrink_picture(data: &[u8], ext: &str, target: Target, quality: u8, level: Level, may_rename: bool, jpeg_ext: &'static str) -> Shrunk {
    let is_jpeg = matches!(ext, "jpg" | "jpeg" | "jpe");
    // CMYK JPEGs (from print workflows) would come back with the wrong colours
    if is_jpeg && jpeg_components(data) != Some(3) && jpeg_components(data) != Some(1) {
        return None;
    }
    let reader = ImageReader::new(Cursor::new(data)).with_guessed_format().ok()?;
    if !matches!(reader.format()?, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Tiff) || (reader.format()? == ImageFormat::Jpeg) != is_jpeg {
        return None;
    }
    let mut decoder = reader.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    // Office doesn't agree with itself on turned photos: those are left as they are
    if decoder.orientation().unwrap_or(Orientation::NoTransforms) != Orientation::NoTransforms {
        return None;
    }
    let profile = decoder.icc_profile().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(profile) = profile {
        to_srgb(&mut img, &profile);
    }
    let transparent = has_transparency(&img);
    img = match img {
        DynamicImage::ImageLuma8(_) | DynamicImage::ImageRgb8(_) => img,
        DynamicImage::ImageRgba8(_) if transparent => img,
        other if transparent => DynamicImage::ImageRgba8(other.to_rgba8()),
        other if !other.color().has_color() => DynamicImage::ImageLuma8(other.to_luma8()),
        other => DynamicImage::ImageRgb8(other.to_rgb8()),
    };

    let scale = match target {
        Target::Shown(sw, sh) => (sw / f64::from(w)).max(sh / f64::from(h)),
        Target::Edge(edge) => f64::from(edge) / f64::from(w.max(h)),
    };
    // Not worth a resample for a few per cent
    if scale < 0.9 {
        let (nw, nh) = (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1));
        img = resize_exact(&img, nw, nh);
    }

    let as_jpeg = is_jpeg || (!transparent && level != Level::Light && may_rename && !looks_like_graphic(&img));
    let out_ext: &'static str = if as_jpeg { jpeg_ext } else { "png" };
    let renamed = !(is_jpeg && matches!(out_ext, "jpg" | "jpeg")) && out_ext != ext;
    if renamed && !may_rename {
        return None;
    }
    let mut bytes = Vec::new();
    match out_ext {
        "png" => bytes = encode_png_oxipng_bytes(&img).ok()?,
        _ => encode_jpeg(&img, quality, &mut bytes).ok()?,
    }
    ((bytes.len() as f64) < data.len() as f64 * WORTH_IT).then_some((bytes, renamed.then_some(out_ext)))
}

/// Colour channels of a JPEG, from its frame header
fn jpeg_components(data: &[u8]) -> Option<u8> {
    let mut at = 2;
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    while at + 4 <= data.len() {
        if data[at] != 0xFF {
            return None;
        }
        let marker = data[at + 1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]]));
        // Start of frame (any kind but the huffman, arithmetic and JPEG-extension markers)
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return data.get(at + 9).copied();
        }
        at += 2 + len;
    }
    None
}

// ─── Videos and recordings ───────────────────────────────────────────────────

fn temp_dir() -> Result<std::path::PathBuf, String> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("flipfile-pkg-{}-{nanos}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// The video or recording at the level, through the engines a lone video uses; it
/// keeps its format, or becomes MP4 / M4A when it may be renamed
fn shrink_media(entry: &Entry, video: bool, level: Level, may_rename: bool, on_step: OnPage) -> Shrunk {
    let dir = temp_dir().ok()?;
    let result = (|| {
        let ext = ext_of(&entry.name);
        let input = dir.join(format!("media.{ext}"));
        fs::write(&input, &entry.data).ok()?;
        let made = if video { av::compress_video(&input, level, Some(&dir), on_step) } else { av::compress_audio(&input, level, Some(&dir)) }.ok()?;
        let new_ext = ext_of(&made.path.to_string_lossy());
        let new_ext: &'static str = match new_ext.as_str() {
            "mp4" => "mp4",
            "m4a" => "m4a",
            "mp3" => "mp3",
            _ => return None,
        };
        let renamed = new_ext != ext;
        if renamed && !may_rename {
            return None;
        }
        let bytes = fs::read(&made.path).ok()?;
        ((bytes.len() as f64) < entry.data.len() as f64 * WORTH_IT).then_some((bytes, renamed.then_some(new_ext)))
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// What re-encoding the video or recording would save, from its length and the level's
/// bitrate
fn estimate_media(entry: &Entry, video: bool, level: Level) -> Option<u64> {
    let dir = temp_dir().ok()?;
    let path = dir.join(format!("media.{}", ext_of(&entry.name)));
    let seconds = fs::write(&path, &entry.data).ok().and_then(|_| av::duration(&path));
    let _ = fs::remove_dir_all(&dir);
    let seconds = seconds?;
    let kbps = if video {
        let plan = av::plan_video(level, entry.data.len() as u64, Some(seconds));
        plan.video_kbps + plan.audio_kbps
    } else {
        av::plan_audio(level).0
    };
    let after = seconds * f64::from(kbps) * 1000.0 / 8.0 * 1.02;
    let before = entry.data.len() as f64;
    (after < before * WORTH_IT).then_some((before - after) as u64)
}

// ─── Where pictures are shown ────────────────────────────────────────────────

const EMU_PER_INCH: f64 = 914_400.0;
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XLINK: &str = "http://www.w3.org/1999/xlink";
const SVG_COMPAT: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";

/// The largest size, in inches, each picture is shown at; `None` when one of the places it
/// is shown doesn't say
fn shown_sizes(family: Family, entries: &[Entry]) -> HashMap<String, Option<(f64, f64)>> {
    let mut sizes: HashMap<String, Option<(f64, f64)>> = HashMap::new();
    let mut add = |path: String, size: Option<(f64, f64)>| {
        let slot = sizes.entry(path).or_insert(Some((0.0, 0.0)));
        *slot = match (*slot, size) {
            (Some((w, h)), Some((sw, sh))) => Some((w.max(sw), h.max(sh))),
            _ => None,
        };
    };
    let by_name: HashMap<&str, &Entry> = entries.iter().map(|e| (e.name.as_str(), e)).collect();
    match family {
        Family::Office => {
            for entry in entries.iter().filter(|e| e.name.ends_with(".xml") && !e.name.contains("_rels/")) {
                let (dir, file) = entry.name.rsplit_once('/').unwrap_or(("", entry.name.as_str()));
                let rels_name = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
                let Some(rels) = by_name.get(rels_name.as_str()) else { continue };
                let targets = relationship_targets(rels, dir);
                if targets.is_empty() {
                    continue;
                }
                let Some(doc) = std::str::from_utf8(&entry.data).ok().and_then(|t| roxmltree::Document::parse(t).ok()) else { continue };
                for node in doc.descendants().filter(|n| n.is_element()) {
                    for attr in node.attributes() {
                        if attr.namespace() != Some(RELATIONSHIPS) && !matches!(attr.name(), "relid") {
                            continue;
                        }
                        let Some(target) = targets.get(attr.value()) else { continue };
                        let size = match node.tag_name().name() {
                            "blip" if attr.name() == "embed" => blip_size(node),
                            "imagedata" => node.parent().and_then(vml_size),
                            _ => None,
                        };
                        add(target.clone(), size);
                    }
                }
            }
        }
        Family::OpenDocument => {
            for entry in entries.iter().filter(|e| e.name.ends_with("content.xml") || e.name.ends_with("styles.xml")) {
                let dir = entry.name.rsplit_once('/').map_or("", |(d, _)| d);
                let Some(doc) = std::str::from_utf8(&entry.data).ok().and_then(|t| roxmltree::Document::parse(t).ok()) else { continue };
                for node in doc.descendants().filter(|n| n.is_element()) {
                    let Some(href) = node.attribute((XLINK, "href")) else { continue };
                    if href.contains("://") {
                        continue;
                    }
                    let path = resolve(dir, href);
                    let size = if node.tag_name().name() == "image" { node.parent().and_then(frame_size) } else { None };
                    add(path, size);
                }
            }
        }
        Family::Book => {}
    }
    sizes
}

/// A part's relationships: id → the part it points to
fn relationship_targets(rels: &Entry, dir: &str) -> HashMap<String, String> {
    let Some(doc) = std::str::from_utf8(&rels.data).ok().and_then(|t| roxmltree::Document::parse(t).ok()) else { return HashMap::new() };
    doc.descendants()
        .filter(|n| n.tag_name().name() == "Relationship" && n.attribute("TargetMode") != Some("External"))
        .filter_map(|n| {
            let target = n.attribute("Target")?;
            let path = match target.strip_prefix('/') {
                Some(absolute) => absolute.to_string(),
                None => resolve(dir, target),
            };
            Some((n.attribute("Id")?.to_string(), path))
        })
        .collect()
}

/// `href` from a part in `dir`, as a path in the package
fn resolve(dir: &str, href: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The size a DrawingML picture is shown at, in inches, from the nearest shape around it
/// that has one; a cropped picture is shown larger than its frame
fn blip_size(blip: roxmltree::Node) -> Option<(f64, f64)> {
    let fill = blip.parent()?;
    let (mut fx, mut fy) = (1.0f64, 1.0f64);
    for child in fill.children().filter(|c| c.is_element()) {
        match child.tag_name().name() {
            "srcRect" => {
                let edge = |name: &str| child.attribute(name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 100_000.0;
                fx = 1.0 - edge("l") - edge("r");
                fy = 1.0 - edge("t") - edge("b");
            }
            // A tiled picture's size says nothing about the tile's
            "tile" => return None,
            _ => {}
        }
    }
    let (fx, fy) = (fx.clamp(0.02, 1.0), fy.clamp(0.02, 1.0));
    let mut node = fill;
    for _ in 0..8 {
        node = node.parent()?;
        if let Some((cx, cy)) = extent(node) {
            return Some((cx / EMU_PER_INCH / fx, cy / EMU_PER_INCH / fy));
        }
    }
    None
}

/// `cx`/`cy` of a shape: `wp:extent`, `xdr:ext`, or `spPr/xfrm/ext`
fn extent(node: roxmltree::Node) -> Option<(f64, f64)> {
    let size = |n: roxmltree::Node| {
        let cx = n.attribute("cx")?.parse::<f64>().ok()?;
        let cy = n.attribute("cy")?.parse::<f64>().ok()?;
        (cx > 0.0 && cy > 0.0).then_some((cx, cy))
    };
    fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        n.children().find(|c| c.is_element() && c.tag_name().name() == name)
    }
    for c in node.children().filter(|c| c.is_element()) {
        let found = match c.tag_name().name() {
            "extent" | "ext" => size(c),
            "spPr" => child(c, "xfrm").and_then(|x| child(x, "ext")).and_then(size),
            "xfrm" => child(c, "ext").and_then(size),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A VML shape's size from its style ("width:120pt;height:80pt")
fn vml_size(shape: roxmltree::Node) -> Option<(f64, f64)> {
    let style = shape.attribute("style")?;
    let mut size = (None, None);
    for declaration in style.split(';') {
        let Some((key, value)) = declaration.split_once(':') else { continue };
        match key.trim() {
            "width" => size.0 = length_inches(value.trim()),
            "height" => size.1 = length_inches(value.trim()),
            _ => {}
        }
    }
    Some((size.0?, size.1?))
}

/// An OpenDocument frame's size (`svg:width`, `svg:height`)
fn frame_size(frame: roxmltree::Node) -> Option<(f64, f64)> {
    Some((length_inches(frame.attribute((SVG_COMPAT, "width"))?)?, length_inches(frame.attribute((SVG_COMPAT, "height"))?)?))
}

/// "6.5in", "16.51cm", "120mm", "72pt", "1pc", "96px" in inches
fn length_inches(value: &str) -> Option<f64> {
    let split = value.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(value.len());
    let number: f64 = value[..split].parse().ok()?;
    let per_inch = match value[split..].trim() {
        "in" | "inch" => 1.0,
        "cm" => 2.54,
        "mm" => 25.4,
        "pt" | "" => 72.0,
        "pc" => 6.0,
        "px" => 96.0,
        _ => return None,
    };
    (number > 0.0).then(|| number / per_inch)
}

// ─── Renaming ────────────────────────────────────────────────────────────────

struct Rename {
    /// File names, without their folders
    old: String,
    new: String,
    mime: &'static str,
}

fn mime_of(ext: &str) -> &'static str {
    match ext {
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "mp4" => "video/mp4",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// The parts that refer to other parts by name: Office's relationships and content types,
/// OpenDocument's XML, a book's pages, style sheets and package files
fn is_text_part(family: Family, name: &str) -> bool {
    match family {
        Family::Office => name.ends_with(".rels") || name == "[Content_Types].xml",
        Family::OpenDocument => matches!(ext_of(name).as_str(), "xml" | "rdf"),
        Family::Book => matches!(ext_of(name).as_str(), "xml" | "opf" | "ncx" | "xhtml" | "html" | "htm" | "css" | "svg" | "smil"),
    }
}

fn is_xml(name: &str) -> bool {
    matches!(ext_of(name).as_str(), "xml" | "rels" | "opf" | "ncx" | "rdf" | "xhtml" | "svg" | "smil")
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.-%".contains(&b)
}

/// Where `name` appears in `text` as a whole file name
fn find_name(text: &str, name: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    text.match_indices(name)
        .map(|(at, _)| at)
        .filter(|&at| (at == 0 || !is_name_byte(bytes[at - 1])) && bytes.get(at + name.len()).is_none_or(|&b| !is_name_byte(b)))
        .collect()
}

/// True when `at` is inside a tag (an attribute), not in the text between tags
fn inside_tag(text: &str, at: usize) -> bool {
    let before = &text[..at];
    match (before.rfind('<'), before.rfind('>')) {
        (Some(open), close) => close.is_none_or(|close| open > close),
        _ => false,
    }
}

/// References to `name` in a part: those inside tags (or anywhere in a style sheet), and
/// those elsewhere, which a rename can't safely follow
fn count_refs(entry: &Entry, name: &str) -> (usize, usize) {
    let Ok(text) = std::str::from_utf8(&entry.data) else { return (0, usize::from(entry.data.windows(name.len()).any(|w| w == name.as_bytes()))) };
    let css = ext_of(&entry.name) == "css";
    find_name(text, name).into_iter().fold((0, 0), |(i, o), at| if css || inside_tag(text, at) { (i + 1, o) } else { (i, o + 1) })
}

/// `text` with every reference to a renamed file renamed, and the content type in the
/// same tag (`ContentType="image/png"`, `media-type="image/png"`) changed with it;
/// `None` when nothing in it refers to them
fn rewrite(text: &str, renames: &[Rename], css: bool) -> Option<String> {
    let mut edits: Vec<(usize, usize, &str)> = Vec::new();
    for rename in renames {
        for at in find_name(text, &rename.old) {
            if !css && !inside_tag(text, at) {
                continue;
            }
            edits.push((at, at + rename.old.len(), &rename.new));
            if css {
                continue;
            }
            let open = text[..at].rfind('<').unwrap_or(0);
            let close = text[at..].find('>').map_or(text.len(), |c| at + c);
            let family = &rename.mime[..rename.mime.find('/').unwrap_or(0) + 1];
            let tag = &text[open..close];
            for (start, _) in tag.match_indices(family) {
                let start = open + start;
                let end = start + text[start..close].find(|c: char| !(c.is_ascii_alphanumeric() || "/.+-".contains(c))).unwrap_or(close - start);
                if &text[start..end] != rename.mime {
                    edits.push((start, end, rename.mime));
                }
            }
        }
    }
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|e| e.0);
    let mut out = String::with_capacity(text.len() + 64);
    let mut at = 0;
    for (start, end, replacement) in edits {
        if start < at {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(replacement);
        at = end;
    }
    out.push_str(&text[at..]);
    Some(out)
}

/// Office's content types: a default for each new extension
fn add_content_types(entries: &mut [Entry], renames: &[Rename]) -> Result<(), String> {
    let Some(types) = entries.iter_mut().find(|e| e.name == "[Content_Types].xml") else {
        return Err("this document has no content types".into());
    };
    let mut text = String::from_utf8(types.data.clone()).map_err(|_| "this document's content types are damaged".to_string())?;
    let mut added = HashSet::new();
    for rename in renames {
        let ext = ext_of(&rename.new);
        if !added.insert(ext.clone()) {
            continue;
        }
        let known = roxmltree::Document::parse(&text)
            .map_err(|_| "this document's content types are damaged".to_string())?
            .descendants()
            .any(|n| n.tag_name().name() == "Default" && n.attribute("Extension").is_some_and(|e| e.eq_ignore_ascii_case(&ext)));
        if known {
            continue;
        }
        let at = text.rfind("</Types>").ok_or_else(|| "this document's content types are damaged".to_string())?;
        text.insert_str(at, &format!("<Default Extension=\"{ext}\" ContentType=\"{}\"/>", rename.mime));
    }
    types.data = text.into_bytes();
    Ok(())
}

fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn ext_of(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_in_inches() {
        assert_eq!(length_inches("6.5in"), Some(6.5));
        assert!((length_inches("2.54cm").unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(length_inches("144pt"), Some(2.0));
        assert_eq!(length_inches("auto"), None);
    }

    #[test]
    fn paths_resolve() {
        assert_eq!(resolve("word", "media/image1.png"), "word/media/image1.png");
        assert_eq!(resolve("ppt/slides", "../media/image2.png"), "ppt/media/image2.png");
        assert_eq!(resolve("", "./Pictures/a.png"), "Pictures/a.png");
    }

    #[test]
    fn renames_follow_references_and_types() {
        let renames = [Rename { old: "image1.png".into(), new: "image1.jpeg".into(), mime: "image/jpeg" }];
        let rels = r#"<Relationships><Relationship Id="rId4" Target="media/image1.png"/><Relationship Id="rId5" Target="media/image11.png"/></Relationships>"#;
        let out = rewrite(rels, &renames, false).unwrap();
        assert!(out.contains(r#"Target="media/image1.jpeg""#) && out.contains("image11.png"), "{out}");
        let manifest = r#"<manifest:file-entry manifest:full-path="Pictures/image1.png" manifest:media-type="image/png"/>"#;
        let out = rewrite(manifest, &renames, false).unwrap();
        assert_eq!(out, r#"<manifest:file-entry manifest:full-path="Pictures/image1.jpeg" manifest:media-type="image/jpeg"/>"#);
        // Text that happens to name the file isn't a reference
        assert!(rewrite("<p>see image1.png</p>", &renames, false).is_none());
        assert_eq!(rewrite("a { background: url(../img/image1.png) }", &renames, true).unwrap(), "a { background: url(../img/image1.jpeg) }");
    }

    #[test]
    fn jpeg_channels() {
        let mut jpeg = Vec::new();
        encode_jpeg(&DynamicImage::ImageRgb8(image::RgbImage::from_pixel(16, 16, image::Rgb([200, 30, 30]))), 80, &mut jpeg).unwrap();
        assert_eq!(jpeg_components(&jpeg), Some(3));
        assert_eq!(jpeg_components(b"not a jpeg"), None);
    }
}
