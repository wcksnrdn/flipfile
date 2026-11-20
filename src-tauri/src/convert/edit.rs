// The Photo Editor's backend (src/editor): opens a dropped photo at full resolution,
// hands the UI a smaller preview to edit live, and applies the same edits to the
// full-resolution photo when it is saved as `name_edited.ext` next to the original.
//
// The UI only ever sends the edits (a few numbers), never pixels or a path to write:
// what is saved is always the photo the user dropped, kept here since it was opened.
// The edits are applied in the same order and with the same arithmetic as the preview
// (src/editor/lib/pipeline.ts), so the saved photo looks like what was on screen.

use image::metadata::Orientation;
use image::DynamicImage;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

use super::compress::is_source_only;
use super::encode::{encode, has_transparency, resize_exact, resize_to_fit};
use super::input::{file_name, open_oriented, same_format_ext};
use super::output::unique_output_path;
use crate::state::grants::Grants;
use crate::state::history::History;
use crate::state::settings::Settings;

/// Longest side of the preview the UI edits live: sharp on a Retina screen, and small
/// enough (16 MB of pixels at most) to redraw on every slider step
const PREVIEW_MAX: u32 = 2048;

/// Warmth at ±100 shifts red up and blue down (or the reverse) by this many levels
const WARMTH_LEVELS: f32 = 25.0;

struct Opened {
    path: PathBuf,
    image: Arc<DynamicImage>,
    /// A pasted clipboard image (a temporary file): saved to Downloads instead
    to_downloads: bool,
}

/// The photo open in the editor
#[derive(Default)]
pub struct Session {
    opened: Mutex<Option<Opened>>,
    /// Bumped by every open, so a slow decode never replaces a photo opened after it
    generation: AtomicU64,
}

/// Every edit, as fractions and levels that do not depend on the photo's resolution
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Edits {
    /// Clockwise, in quarter turns: 0, 90, 180 or 270
    rotation: u16,
    flip_h: bool,
    flip_v: bool,
    /// Of the rotated and flipped photo; None keeps all of it
    crop: Option<Crop>,
    /// 0 < scale <= 1 of the cropped size
    scale: f64,
    adjust: Adjust,
}

/// Fractions (0..1) of the photo's width and height
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// -100..100 each, 0 leaves the photo as it is
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Adjust {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    warmth: f32,
}

impl Adjust {
    fn is_identity(&self) -> bool {
        *self == Adjust::default()
    }
}

/// Opens a dropped (or pasted) photo for editing. Returns the preview as raw bytes:
/// its width, height, then the original's width and height (u32 little-endian each),
/// then the preview's RGBA pixels. (The webview may not load the file itself: its
/// content policy only shows the app's own images.)
#[tauri::command]
pub async fn editor_open(app: AppHandle, path: String, to_downloads: bool) -> Result<tauri::ipc::Response, String> {
    // Only files the user dropped or pasted (see grants.rs)
    if let Err(denied) = app.state::<Grants>().check(std::slice::from_ref(&path)) {
        return Err(format!("\"{}\" was not given to flipfile", file_name(Path::new(denied))));
    }
    let generation = app.state::<Session>().generation.fetch_add(1, Ordering::SeqCst) + 1;
    let path = PathBuf::from(path);
    let decode_path = path.clone();
    let (image, preview) = tauri::async_runtime::spawn_blocking(move || {
        let (image, _) = open_oriented(&decode_path)?;
        let preview = preview_bytes(&image);
        Ok::<_, String>((image, preview))
    })
    .await
    .map_err(|e| e.to_string())??;

    let session = app.state::<Session>();
    let mut opened = session.opened.lock().unwrap();
    if session.generation.load(Ordering::SeqCst) != generation {
        return Err("Another photo was opened".into());
    }
    *opened = Some(Opened { path, image: Arc::new(image), to_downloads });
    Ok(tauri::ipc::Response::new(preview))
}

/// Saves the open photo with `edits` applied, keeping its format; returns the new file
#[tauri::command]
pub async fn editor_save(app: AppHandle, edits: Edits) -> Result<String, String> {
    let (path, image, to_downloads) = {
        let session = app.state::<Session>();
        let opened = session.opened.lock().unwrap();
        let opened = opened.as_ref().ok_or("No photo is open")?;
        (opened.path.clone(), Arc::clone(&opened.image), opened.to_downloads)
    };
    let out_dir = if to_downloads {
        Some(app.path().download_dir().map_err(|_| "The Downloads folder could not be found".to_string())?)
    } else {
        None
    };
    let quality = app.state::<Mutex<Settings>>().lock().unwrap().options(None).quality;

    let output = tauri::async_runtime::spawn_blocking(move || {
        let edited = apply(&image, &edits);
        let ext = save_format(&path, &edited)?;
        let output = unique_output_path(&path, out_dir.as_deref(), "edited", &ext);
        encode(&edited, &ext, &output, quality)?;
        Ok::<_, String>(output.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())??;

    app.state::<Mutex<History>>().lock().unwrap().record(std::slice::from_ref(&output));
    crate::platform::tray::refresh(&app);
    Ok(output)
}

/// The editor closed: the full-resolution photo is let go
#[tauri::command]
pub fn editor_close(app: AppHandle) {
    let session = app.state::<Session>();
    session.generation.fetch_add(1, Ordering::SeqCst);
    *session.opened.lock().unwrap() = None;
}

/// The photo's own format; a camera RAW, Photoshop or JPEG 2000 file, which flipfile reads
/// but can't write, becomes a JPG (PNG when it has transparency), as when compressing
fn save_format(path: &Path, edited: &DynamicImage) -> Result<String, String> {
    if is_source_only(path) {
        return Ok(if has_transparency(edited) { "png" } else { "jpg" }.into());
    }
    same_format_ext(path)
}

fn preview_bytes(image: &DynamicImage) -> Vec<u8> {
    let rgba = if image.width().max(image.height()) > PREVIEW_MAX {
        resize_to_fit(image, PREVIEW_MAX, PREVIEW_MAX).to_rgba8()
    } else {
        image.to_rgba8()
    };
    let mut bytes = Vec::with_capacity(16 + rgba.len());
    for side in [rgba.width(), rgba.height(), image.width(), image.height()] {
        bytes.extend_from_slice(&side.to_le_bytes());
    }
    bytes.extend_from_slice(rgba.as_raw());
    bytes
}

/// Rotate, flip, crop, scale, then adjust: the preview's order (pipeline.ts)
fn apply(image: &DynamicImage, edits: &Edits) -> DynamicImage {
    let mut img = match edits.rotation % 360 {
        90 => image.rotate90(),
        180 => image.rotate180(),
        270 => image.rotate270(),
        _ => image.clone(),
    };
    if edits.flip_h {
        img.apply_orientation(Orientation::FlipHorizontal);
    }
    if edits.flip_v {
        img.apply_orientation(Orientation::FlipVertical);
    }
    if let Some(crop) = edits.crop {
        let (x, y, w, h) = crop_pixels(&crop, img.width(), img.height());
        if (x, y, w, h) != (0, 0, img.width(), img.height()) {
            img = img.crop_imm(x, y, w, h);
        }
    }
    let scale = if edits.scale.is_finite() { edits.scale.clamp(0.01, 1.0) } else { 1.0 };
    if scale < 1.0 {
        let side = |s: u32| ((f64::from(s) * scale).round() as u32).max(1);
        img = resize_exact(&img, side(img.width()), side(img.height()));
    }
    if !edits.adjust.is_identity() {
        adjust(&mut img, &edits.adjust);
    }
    img
}

/// The crop in whole pixels, always inside the photo and at least 1×1 (cropPixels in pipeline.ts)
fn crop_pixels(crop: &Crop, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let frac = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let (w, h) = (f64::from(width), f64::from(height));
    let x = ((frac(crop.x) * w).round() as u32).min(width.saturating_sub(1));
    let y = ((frac(crop.y) * h).round() as u32).min(height.saturating_sub(1));
    let cw = ((frac(crop.width) * w).round() as u32).clamp(1, width - x);
    let ch = ((frac(crop.height) * h).round() as u32).clamp(1, height - y);
    (x, y, cw, ch)
}

/// Brightness, contrast and warmth per channel (a lookup table), then saturation
/// around the pixel's luminance. Alpha is left as it is.
struct Tone {
    lut: [[f32; 256]; 3],
    saturation: f32,
}

impl Tone {
    fn new(a: &Adjust) -> Self {
        let gain = 2f32.powf(a.brightness / 100.0);
        let contrast = 1.0 + a.contrast / 100.0;
        let warmth = a.warmth / 100.0 * WARMTH_LEVELS;
        let shift = [warmth, 0.0, -warmth];
        let mut lut = [[0.0; 256]; 3];
        for (channel, table) in lut.iter_mut().enumerate() {
            for (v, out) in table.iter_mut().enumerate() {
                *out = (v as f32 * gain - 128.0) * contrast + 128.0 + shift[channel];
            }
        }
        Tone { lut, saturation: 1.0 + a.saturation / 100.0 }
    }

    fn pixel(&self, p: &mut [u8]) {
        let (r, g, b) = (self.lut[0][p[0] as usize], self.lut[1][p[1] as usize], self.lut[2][p[2] as usize]);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let s = self.saturation;
        p[0] = to_level(luma + (r - luma) * s);
        p[1] = to_level(luma + (g - luma) * s);
        p[2] = to_level(luma + (b - luma) * s);
    }
}

fn to_level(v: f32) -> u8 {
    (v.clamp(0.0, 255.0) + 0.5) as u8
}

/// Adjusts in 8 bits per channel (deeper photos are brought down to it), on every core
fn adjust(img: &mut DynamicImage, a: &Adjust) {
    let tone = Tone::new(a);
    let alpha = img.color().has_alpha();
    if !matches!(img, DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_)) {
        *img = if alpha { DynamicImage::ImageRgba8(img.to_rgba8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
    }
    let (channels, data): (usize, &mut [u8]) = match img {
        DynamicImage::ImageRgb8(p) => (3, p),
        DynamicImage::ImageRgba8(p) => (4, p),
        _ => unreachable!(),
    };
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get());
    let pixels_per_worker = (data.len() / channels).div_ceil(workers).max(1);
    std::thread::scope(|s| {
        for chunk in data.chunks_mut(pixels_per_worker * channels) {
            let tone = &tone;
            s.spawn(move || chunk.chunks_exact_mut(channels).for_each(|p| tone.pixel(p)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn edits() -> Edits {
        Edits { rotation: 0, flip_h: false, flip_v: false, crop: None, scale: 1.0, adjust: Adjust::default() }
    }

    /// 4×2: the top-left pixel is red, the rest black
    fn marked() -> DynamicImage {
        let mut img = RgbImage::new(4, 2);
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        DynamicImage::ImageRgb8(img)
    }

    fn red_at(img: &DynamicImage) -> (u32, u32) {
        let rgb = img.to_rgb8();
        let (x, y, _) = rgb.enumerate_pixels().find(|(_, _, p)| p.0 == [255, 0, 0]).unwrap();
        (x, y)
    }

    #[test]
    fn preview_bytes_are_sizes_then_pixels() {
        // Parsed by openPhoto in src/editor/lib/backend.ts
        let big = DynamicImage::ImageRgb8(RgbImage::new(4096, 1024));
        let bytes = preview_bytes(&big);
        let side = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        assert_eq!((side(0), side(4), side(8), side(12)), (2048, 512, 4096, 1024));
        assert_eq!(bytes.len(), 16 + 2048 * 512 * 4);
        // Small photos are sent as they are
        assert_eq!(preview_bytes(&marked()).len(), 16 + 4 * 2 * 4);
    }

    #[test]
    fn no_edits_keep_the_photo() {
        let img = marked();
        assert_eq!(apply(&img, &edits()), img);
    }

    #[test]
    fn rotation_then_flip_in_the_rotated_frame() {
        let img = marked();
        // A quarter turn clockwise: the top-left corner goes to the top-right
        let turned = apply(&img, &Edits { rotation: 90, ..edits() });
        assert_eq!((turned.width(), turned.height()), (2, 4));
        assert_eq!(red_at(&turned), (1, 0));
        // Then flipped horizontally (of what is on screen): back to the left
        let flipped = apply(&img, &Edits { rotation: 90, flip_h: true, ..edits() });
        assert_eq!(red_at(&flipped), (0, 0));
        // Flipped vertically: bottom
        let down = apply(&img, &Edits { flip_v: true, ..edits() });
        assert_eq!(red_at(&down), (0, 1));
    }

    #[test]
    fn crop_and_scale_by_fractions() {
        let img = DynamicImage::ImageRgb8(RgbImage::new(400, 200));
        let crop = Crop { x: 0.25, y: 0.5, width: 0.5, height: 0.5 };
        let cropped = apply(&img, &Edits { crop: Some(crop), ..edits() });
        assert_eq!((cropped.width(), cropped.height()), (200, 100));
        let halved = apply(&img, &Edits { crop: Some(crop), scale: 0.5, ..edits() });
        assert_eq!((halved.width(), halved.height()), (100, 50));
    }

    #[test]
    fn crop_stays_inside_the_photo() {
        // Out of range, nonsense or empty: still a real rectangle in the photo
        assert_eq!(crop_pixels(&Crop { x: 0.9, y: 0.9, width: 0.5, height: 0.5 }, 100, 50), (90, 45, 10, 5));
        assert_eq!(crop_pixels(&Crop { x: f64::NAN, y: -1.0, width: 0.0, height: 2.0 }, 100, 50), (0, 0, 1, 50));
        assert_eq!(crop_pixels(&Crop { x: 1.0, y: 1.0, width: 1.0, height: 1.0 }, 100, 50), (99, 49, 1, 1));
    }

    #[test]
    fn adjustments_match_the_preview() {
        // The same numbers as pipeline.test.ts
        let mut img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 128])));
        adjust(&mut img, &Adjust { brightness: 0.0, contrast: 0.0, saturation: -100.0, warmth: 0.0 });
        assert_eq!(img.to_rgba8().get_pixel(0, 0).0, [118, 118, 118, 128]); // gray, alpha kept

        let mut img = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, Rgb([200, 100, 50])));
        adjust(&mut img, &Adjust { brightness: 100.0, contrast: 0.0, saturation: 0.0, warmth: 0.0 });
        assert_eq!(img.to_rgb8().get_pixel(0, 0).0, [255, 200, 100]); // twice as bright, clipped

        let mut img = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, Rgb([200, 100, 50])));
        adjust(&mut img, &Adjust { brightness: 0.0, contrast: 50.0, saturation: 0.0, warmth: 40.0 });
        assert_eq!(img.to_rgb8().get_pixel(0, 0).0, [246, 86, 1]);
    }

    #[test]
    fn photos_keep_their_format_and_raw_becomes_jpg() {
        let dir = std::env::temp_dir().join(format!("flipfile-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("logo.png");
        marked().save(&png).unwrap();
        assert_eq!(save_format(&png, &marked()).unwrap(), "png");
        // A RAW file is only read (the system's codec), so it is saved as a JPG
        #[cfg(any(target_os = "macos", windows))]
        {
            let raw = dir.join("IMG_0001.CR3");
            std::fs::write(&raw, b"not decoded here").unwrap();
            assert_eq!(save_format(&raw, &marked()).unwrap(), "jpg");
            let clear = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 0])));
            assert_eq!(save_format(&raw, &clear).unwrap(), "png");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deep_photos_are_adjusted_in_8_bits() {
        let mut img = DynamicImage::ImageRgb16(image::ImageBuffer::from_pixel(2, 2, Rgb([65535u16, 0, 0])));
        adjust(&mut img, &Adjust { saturation: -100.0, ..Adjust::default() });
        assert!(matches!(img, DynamicImage::ImageRgb8(_)));
        assert_eq!(img.to_rgb8().get_pixel(1, 1).0, [54, 54, 54]);
    }
}
