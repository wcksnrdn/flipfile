// PDFs at a compress level: the pictures inside are scaled down to the level's resolution
// and re-encoded as JPEG; text, fonts, vectors and layout are left exactly as they are, so
// the text stays sharp and selectable. Then every stream is compressed and the objects are
// packed into object streams. The result is opened again by PDFium before it is kept.
//
// Only pictures that are safe to rewrite are touched: 8 bits per component, grey or RGB
// (plain or ICC-based), stored raw, Flate/LZW or as JPEG. Masks, CMYK, JPEG 2000, JBIG2
// and fax pictures are kept as they are.

use std::fs;
use std::path::Path;

use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use super::{Level, Made};
use crate::convert::encode::{encode_jpeg, resize_exact};
use crate::convert::input::file_name;
use crate::convert::output::{save_err, unique_output_path};
use crate::pdf::render as pdf_render;

/// Resolution of pictures (at the size of the largest page) and JPEG quality per level
fn params(level: Level) -> (f32, u8) {
    match level {
        Level::Light => (200.0, 80),
        Level::Balanced => (150.0, 70),
        Level::Strong => (110.0, 55),
        Level::Extreme => (72.0, 40),
    }
}

/// A picture is only rewritten when that saves at least this share of its bytes
const WORTH_IT: f64 = 0.9;
/// Pictures larger than this many pixels when decoded are left alone (memory)
const MAX_PIXELS: u64 = 120_000_000;

pub(super) fn compress(input: &Path, level: Level, out_dir: Option<&Path>) -> Result<Made, String> {
    let name = file_name(input);
    let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
    let bytes = compress_bytes(&data, level).map_err(|e| format!("\"{name}\": {e}"))?;
    let path = unique_output_path(input, out_dir, "compressed", "pdf");
    fs::write(&path, bytes).map_err(save_err)?;
    Ok(Made { path, changed: None })
}

/// The compressed PDF, checked to open with as many pages as the original
pub(super) fn compress_bytes(data: &[u8], level: Level) -> Result<Vec<u8>, String> {
    let mut doc = Document::load_mem(data).map_err(|_| "this PDF could not be read".to_string())?;
    if doc.is_encrypted() {
        return Err("password-protected PDFs can't be compressed".into());
    }
    let pages = doc.get_pages().len();
    let (dpi, quality) = params(level);
    let longest_page = doc.get_pages().values().filter_map(|&id| page_long_side(&doc, id)).fold(0.0f32, f32::max);
    let longest_page = if longest_page > 0.0 { longest_page } else { 842.0 };
    let max_side = ((longest_page / 72.0) * dpi).round().max(64.0) as u32;

    let images: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, object)| matches!(object, Object::Stream(s) if is_image(&s.dict)))
        .map(|(&id, _)| id)
        .collect();
    for id in images {
        let replacement = match doc.objects.get(&id) {
            Some(Object::Stream(stream)) => recompress(&doc, stream, max_side, quality),
            _ => None,
        };
        if let (Some((jpeg, w, h)), Some(Object::Stream(stream))) = (replacement, doc.objects.get_mut(&id)) {
            stream.dict.set("Width", w as i64);
            stream.dict.set("Height", h as i64);
            stream.dict.set("BitsPerComponent", 8);
            stream.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
            stream.dict.remove(b"DecodeParms");
            stream.set_content(jpeg);
        }
    }
    doc.prune_objects();
    doc.compress();

    // Object streams pack the rest tightly; should PDFium not open that, the classic layout
    let mut modern = Vec::new();
    if doc.clone().save_modern(&mut modern).is_ok() && opens(&modern, pages) {
        return Ok(modern);
    }
    let mut classic = Vec::new();
    doc.save_to(&mut classic).map_err(|_| "this PDF could not be written".to_string())?;
    if opens(&classic, pages) {
        return Ok(classic);
    }
    Err("this PDF couldn't be compressed safely".into())
}

fn is_image(dict: &Dictionary) -> bool {
    dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image".as_slice())
}

/// True when PDFium opens `pdf` with `pages` pages
fn opens(pdf: &[u8], pages: usize) -> bool {
    let Ok(pdfium) = pdf_render::pdfium() else { return true };
    pdfium.load_pdf_from_byte_slice(pdf, None).is_ok_and(|doc| doc.pages().len() as usize == pages)
}

/// The longer side of a page's MediaBox in points, inherited from its parents if need be
fn page_long_side(doc: &Document, page: ObjectId) -> Option<f32> {
    let mut id = page;
    for _ in 0..32 {
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(object) = dict.get(b"MediaBox") {
            let (_, object) = doc.dereference(object).ok()?;
            let values: Vec<f32> = object.as_array().ok()?.iter().filter_map(|v| doc.dereference(v).ok()?.1.as_float().ok()).collect();
            if values.len() == 4 {
                return Some((values[2] - values[0]).abs().max((values[3] - values[1]).abs()));
            }
            return None;
        }
        id = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}

/// Colour channels of a picture's colour space, when it is grey or RGB
fn channels(doc: &Document, dict: &Dictionary) -> Option<u8> {
    let (_, space) = doc.dereference(dict.get(b"ColorSpace").ok()?).ok()?;
    match space {
        Object::Name(name) => match name.as_slice() {
            b"DeviceRGB" => Some(3),
            b"DeviceGray" => Some(1),
            _ => None,
        },
        Object::Array(array) if array.len() == 2 && array[0].as_name().ok() == Some(b"ICCBased".as_slice()) => {
            let (_, profile) = doc.dereference(&array[1]).ok()?;
            match profile.as_stream().ok()?.dict.get(b"N").and_then(Object::as_i64).ok()? {
                1 => Some(1),
                3 => Some(3),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The picture as a smaller JPEG, with its new width and height, when that is safe and
/// worth it
fn recompress(doc: &Document, stream: &Stream, max_side: u32, quality: u8) -> Option<(Vec<u8>, u32, u32)> {
    let dict = &stream.dict;
    if dict.get(b"ImageMask").and_then(Object::as_bool).unwrap_or(false) || dict.has(b"Decode") || dict.has(b"Mask") {
        return None;
    }
    if dict.get(b"BitsPerComponent").and_then(Object::as_i64).ok()? != 8 {
        return None;
    }
    // A mask whose colours were premultiplied (Matte) must keep the picture's size
    if let Ok(mask) = dict.get(b"SMask").and_then(Object::as_reference) {
        if doc.get_object(mask).ok()?.as_stream().ok()?.dict.has(b"Matte") {
            return None;
        }
    }
    let n = channels(doc, dict)?;
    let w = u32::try_from(dict.get(b"Width").and_then(Object::as_i64).ok()?).ok()?;
    let h = u32::try_from(dict.get(b"Height").and_then(Object::as_i64).ok()?).ok()?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let filters: Vec<Vec<u8>> = stream.filters().map(|f| f.into_iter().map(<[u8]>::to_vec).collect()).unwrap_or_default();
    let image = match filters.iter().map(Vec::as_slice).collect::<Vec<_>>().as_slice() {
        [b"DCTDecode"] => {
            let decoded = image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg).ok()?;
            if decoded.width() != w || decoded.height() != h {
                return None;
            }
            if n == 1 { DynamicImage::ImageLuma8(decoded.to_luma8()) } else { DynamicImage::ImageRgb8(decoded.to_rgb8()) }
        }
        rest if rest.iter().all(|f| matches!(*f, b"FlateDecode" | b"LZWDecode")) => {
            let expected = w as usize * h as usize * n as usize;
            let raw = stream.decompressed_content_with_limit(expected + 16).ok()?;
            if raw.len() < expected {
                return None;
            }
            let raw = raw[..expected].to_vec();
            if n == 1 {
                DynamicImage::ImageLuma8(GrayImage::from_raw(w, h, raw)?)
            } else {
                DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, raw)?)
            }
        }
        _ => return None,
    };
    let scale = (max_side as f32 / w.max(h) as f32).min(1.0);
    let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    let image = if (nw, nh) != (w, h) { resize_exact(&image, nw, nh) } else { image };
    let mut jpeg = Vec::new();
    encode_jpeg(&image, quality, &mut jpeg).ok()?;
    ((jpeg.len() as f64) < stream.content.len() as f64 * WORTH_IT).then_some((jpeg, nw, nh))
}
