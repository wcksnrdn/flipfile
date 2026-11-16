// Animated GIFs at a compress level, kept animated GIFs (as a photo they lost every frame
// but the first). Each frame is stored as only what changed since the one before, cropped
// to the change, the rest left transparent. Colours come either from each frame's own
// palette or from one shared by the whole animation, whichever is smaller; Light keeps
// them exact. From Balanced on the shared palette may be learned (fewer colours) and
// changes too small to see are let go; Strong and Extreme scale the picture down, and
// Extreme also drops every other frame, its time going to the frame kept.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use std::collections::HashMap;

use color_quant::NeuQuant;
use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, RgbaImage};

use super::{Level, Made};
use crate::convert::encode::GIF_SPEED;
use crate::convert::input::{file_name, header};
use crate::convert::output::{save_err, unique_output_path};

/// Longest side, how far a pixel may drift before it counts as changed, and whether every
/// other frame is dropped
fn params(level: Level) -> (Option<u32>, u8, bool) {
    match level {
        Level::Light => (None, 0, false),
        Level::Balanced => (None, 6, false),
        Level::Strong => (Some(480), 14, false),
        Level::Extreme => (Some(320), 24, true),
    }
}

/// Animations whose decoded frames would take more than this are refused (memory)
const MAX_PIXELS: u64 = 400_000_000;
/// Pixels the shared palette is learned from, taken evenly from up to this many frames
const PALETTE_SAMPLE: usize = 1_500_000;
const PALETTE_FRAMES: u64 = 12;
/// The shared palette's slot for "unchanged"
const CLEAR: u8 = 255;

/// The frames of a GIF, without decoding them; 0 when it isn't one
fn frame_count(data: &[u8]) -> u64 {
    let mut options = gif::DecodeOptions::new();
    options.skip_frame_decoding(true);
    let Ok(mut decoder) = options.read_info(Cursor::new(data)) else { return 0 };
    let mut frames = 0;
    while let Ok(Some(_)) = decoder.read_next_frame() {
        frames += 1;
    }
    frames
}

/// True for a GIF with more than one frame
pub(super) fn is_animated_gif(path: &Path) -> bool {
    if !header(path).starts_with(b"GIF8") {
        return false;
    }
    let Ok(file) = fs::File::open(path) else { return false };
    let mut options = gif::DecodeOptions::new();
    options.skip_frame_decoding(true);
    let Ok(mut decoder) = options.read_info(std::io::BufReader::new(file)) else { return false };
    let mut frames = 0;
    while let Ok(Some(_)) = decoder.read_next_frame() {
        frames += 1;
        if frames > 1 {
            return true;
        }
    }
    false
}

pub(super) fn compress(input: &Path, level: Level, out_dir: Option<&Path>) -> Result<Made, String> {
    let name = file_name(input);
    let data = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;
    let bytes = encode_level(&data, level).map_err(|e| format!("\"{name}\": {e}"))?;
    let path = unique_output_path(input, out_dir, "compressed", "gif");
    fs::write(&path, bytes).map_err(save_err)?;
    Ok(Made { path, changed: None })
}

/// The animation at `level`, as a GIF. Both ways of colouring it are tried, and the
/// smaller kept: a shared palette suits photos and saves a palette per frame, each
/// frame's own suits drawings whose colours change over time.
pub(super) fn encode_level(data: &[u8], level: Level) -> Result<Vec<u8>, String> {
    let own = encode(data, level, false)?.ok_or_else(|| "this GIF could not be written".to_string())?;
    Ok(match encode(data, level, true)? {
        Some(shared) if shared.len() < own.len() => shared,
        _ => own,
    })
}

/// The animation, with a shared palette or each frame's own; `None` when a shared one
/// would lose colours at Light
fn encode(data: &[u8], level: Level, shared: bool) -> Result<Option<Vec<u8>>, String> {
    let damaged = |_| "this GIF is damaged".to_string();
    let unwritable = |_| "this GIF could not be written".to_string();
    let (max_edge, drift, drop_odd) = params(level);
    let repeat = gif::DecodeOptions::new().read_info(Cursor::new(data)).map(|d| d.repeat()).unwrap_or(gif::Repeat::Infinite);
    let (w, h) = {
        let decoder = GifDecoder::new(Cursor::new(data)).map_err(damaged)?;
        image::ImageDecoder::dimensions(&decoder)
    };
    let frames = frame_count(data);
    if frames * u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("this animation is too long to compress".into());
    }
    let scale = max_edge.map_or(1.0, |edge| (f64::from(edge) / f64::from(w.max(h))).min(1.0));
    let (ow, oh) = (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1));
    // A filter that doesn't ring: ringing would add colours along every sharp edge
    let fit = |pixels: RgbaImage| if (ow, oh) != (w, h) { image::imageops::resize(&pixels, ow, oh, image::imageops::FilterType::Triangle) } else { pixels };

    // Frames with see-through pixels can't be stored as changes: a pixel turning clear
    // again needs the frame cleared, so then every frame is stored whole. The shared
    // palette learns from a few frames spread over the animation.
    let every = (frames / PALETTE_FRAMES).max(1);
    let mut see_through = false;
    let mut sample: Vec<u8> = Vec::new();
    // Every colour of the animation, while they fit a palette
    let mut exact: Option<HashMap<[u8; 3], u8>> = shared.then(HashMap::new);
    for (i, frame) in GifDecoder::new(Cursor::new(data)).map_err(damaged)?.into_frames().enumerate() {
        let frame = frame.map_err(damaged)?;
        see_through |= frame.buffer().pixels().any(|p| p[3] < 128);
        if !shared {
            continue;
        }
        let pixels = fit(frame.into_buffer());
        if let Some(colours) = exact.as_mut() {
            for p in pixels.pixels().filter(|p| p[3] >= 128) {
                let next = colours.len() as u8;
                colours.entry([p[0], p[1], p[2]]).or_insert(next);
                if colours.len() > usize::from(CLEAR) {
                    break;
                }
            }
            if colours.len() > usize::from(CLEAR) {
                exact = None;
            }
        }
        if (i as u64).is_multiple_of(every) {
            let stride = (pixels.len() / 4 * PALETTE_FRAMES as usize / PALETTE_SAMPLE).max(1);
            for p in pixels.pixels().step_by(stride).filter(|p| p[3] >= 128) {
                sample.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
    }
    let colours = match exact {
        Some(exact) if !exact.is_empty() => {
            let mut rgb = vec![0; 256 * 3];
            for (colour, &index) in &exact {
                rgb[usize::from(index) * 3..][..3].copy_from_slice(colour);
            }
            Some(Palette { lookup: Lookup::Exact(exact), rgb })
        }
        _ if shared && level == Level::Light => return Ok(None),
        _ if shared && !sample.is_empty() => {
            let quant = NeuQuant::new(GIF_SPEED, 255, &sample);
            let mut rgb = quant.color_map_rgb();
            rgb.resize(256 * 3, 0);
            Some(Palette { lookup: Lookup::Learned(quant), rgb })
        }
        _ if shared => return Ok(None),
        _ => None,
    };

    let mut out = Vec::new();
    {
        let global: &[u8] = colours.as_ref().map_or(&[], |p| p.rgb.as_slice());
        let mut encoder = gif::Encoder::new(&mut out, ow as u16, oh as u16, global).map_err(unwritable)?;
        encoder.set_repeat(repeat).map_err(unwritable)?;
        let mut writer = Writer { encoder, colours, canvas: None, held: None, see_through, drift };
        let mut pending: Option<(RgbaImage, u32)> = None;
        for (i, frame) in GifDecoder::new(Cursor::new(data)).map_err(damaged)?.into_frames().enumerate() {
            let frame = frame.map_err(damaged)?;
            let (numer, denom) = frame.delay().numer_denom_ms();
            // Browsers play delays under 2 hundredths at 10: summed frames must keep that
            let cs = (numer / denom.max(1)) / 10;
            let cs = if cs < 2 { 10 } else { cs };
            if let Some((_, delay)) = pending.as_mut().filter(|_| drop_odd && i % 2 == 1) {
                *delay += cs;
                continue;
            }
            if let Some((pixels, delay)) = pending.take() {
                writer.push(pixels, delay)?;
            }
            pending = Some((fit(frame.into_buffer()), cs));
        }
        if let Some((pixels, delay)) = pending.take() {
            writer.push(pixels, delay)?;
        }
        writer.finish()?;
    }
    Ok(Some(out))
}

/// The palette all frames share: exactly the animation's colours, or learned from them
struct Palette {
    lookup: Lookup,
    /// 256 RGB entries
    rgb: Vec<u8>,
}

enum Lookup {
    Exact(HashMap<[u8; 3], u8>),
    Learned(NeuQuant),
}

impl Palette {
    fn index(&self, p: &image::Rgba<u8>) -> u8 {
        match &self.lookup {
            Lookup::Exact(colours) => colours.get(&[p[0], p[1], p[2]]).copied().unwrap_or(0),
            Lookup::Learned(quant) => quant.index_of(&[p[0], p[1], p[2], 255]) as u8,
        }
    }
}

struct Writer<W: std::io::Write> {
    encoder: gif::Encoder<W>,
    /// The shared palette, when there is one
    colours: Option<Palette>,
    /// What the viewer sees after the frames written so far
    canvas: Option<RgbaImage>,
    /// The last frame, held back so that a following frame with no change can add its
    /// time to it
    held: Option<gif::Frame<'static>>,
    see_through: bool,
    drift: u8,
}

impl<W: std::io::Write> Writer<W> {
    fn push(&mut self, pixels: RgbaImage, delay_cs: u32) -> Result<(), String> {
        let (w, h) = pixels.dimensions();
        let delay = delay_cs.min(u32::from(u16::MAX)) as u16;
        // Each pixel's slot in the shared palette, and the colour it shows as
        let indices: Option<Vec<u8>> =
            self.colours.as_ref().map(|palette| pixels.pixels().map(|p| if p[3] < 128 { CLEAR } else { palette.index(p) }).collect());
        let shown = |i: usize, p: &image::Rgba<u8>| -> [u8; 3] {
            match (&indices, &self.colours) {
                (Some(indices), Some(palette)) => {
                    let c = usize::from(indices[i]) * 3;
                    [palette.rgb[c], palette.rgb[c + 1], palette.rgb[c + 2]]
                }
                _ => [p[0], p[1], p[2]],
            }
        };
        // Unchanged: shows as what is there already, or close enough to it
        let unchanged = |x: u32, y: u32| -> bool {
            let (Some(canvas), false) = (&self.canvas, self.see_through) else { return false };
            let (p, b) = (pixels.get_pixel(x, y), canvas.get_pixel(x, y));
            p[3] >= 128 && (shown((y * w + x) as usize, p) == [b[0], b[1], b[2]] || (0..3).all(|c| p[c].abs_diff(b[c]) <= self.drift))
        };
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if !unchanged(x, y) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        let (left, top, right, bottom) = if x0 >= x1 {
            // Nothing to see: the frame before simply lasts longer
            if let Some(held) = self.held.as_mut() {
                held.delay = held.delay.saturating_add(delay);
                return Ok(());
            }
            (0, 0, 1, 1)
        } else {
            (x0, y0, x1, y1)
        };
        let (cw, ch) = (right - left, bottom - top);
        let mut frame = match &indices {
            Some(indices) => {
                let mut region = Vec::with_capacity((cw * ch) as usize);
                for y in top..bottom {
                    for x in left..right {
                        region.push(if unchanged(x, y) { CLEAR } else { indices[(y * w + x) as usize] });
                    }
                }
                gif::Frame::from_indexed_pixels(cw as u16, ch as u16, region, Some(CLEAR))
            }
            None => {
                let mut region = Vec::with_capacity((cw * ch * 4) as usize);
                for y in top..bottom {
                    for x in left..right {
                        let p = pixels.get_pixel(x, y);
                        if unchanged(x, y) || p[3] < 128 {
                            region.extend_from_slice(&[0, 0, 0, 0]);
                        } else {
                            region.extend_from_slice(&[p[0], p[1], p[2], 255]);
                        }
                    }
                }
                exact_palette(&region, cw as u16, ch as u16).unwrap_or_else(|| gif::Frame::from_rgba_speed(cw as u16, ch as u16, &mut region, GIF_SPEED))
            }
        };
        frame.left = left as u16;
        frame.top = top as u16;
        frame.delay = delay;
        frame.dispose = if self.see_through { gif::DisposalMethod::Background } else { gif::DisposalMethod::Keep };

        // The canvas, as the viewer will see it once this frame is drawn
        if !self.see_through {
            let palette = frame.palette.as_deref().or(self.colours.as_ref().map(|p| p.rgb.as_slice()));
            let canvas = self.canvas.get_or_insert_with(|| RgbaImage::new(w, h));
            if let Some(palette) = palette {
                for (i, &index) in frame.buffer.iter().enumerate() {
                    if Some(index) == frame.transparent {
                        continue;
                    }
                    let (x, y) = (left + i as u32 % cw, top + i as u32 / cw);
                    let c = usize::from(index) * 3;
                    if let Some(rgb) = palette.get(c..c + 3) {
                        canvas.put_pixel(x, y, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
                    }
                }
            }
        }
        if let Some(held) = self.held.replace(frame) {
            self.encoder.write_frame(&held).map_err(|_| "this GIF could not be written".to_string())?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(), String> {
        if let Some(held) = self.held.take() {
            self.encoder.write_frame(&held).map_err(|_| "this GIF could not be written".to_string())?;
        }
        Ok(())
    }
}

/// The frame with a palette of exactly its colours, when they fit in one (with a slot
/// left for transparency, when some of it is)
fn exact_palette(rgba: &[u8], w: u16, h: u16) -> Option<gif::Frame<'static>> {
    let mut colours: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
    let mut palette = Vec::new();
    let mut indices = Vec::with_capacity(rgba.len() / 4);
    let mut clear = false;
    for p in rgba.as_chunks::<4>().0 {
        if p[3] == 0 {
            clear = true;
            indices.push(u8::MAX);
            continue;
        }
        let key = [p[0], p[1], p[2]];
        let index = match colours.get(&key) {
            Some(&index) => index,
            None => {
                if colours.len() >= 256 {
                    return None;
                }
                let index = colours.len() as u8;
                colours.insert(key, index);
                palette.extend_from_slice(&key);
                index
            }
        };
        indices.push(index);
    }
    if clear && colours.len() == 256 {
        return None;
    }
    let transparent = clear.then_some(colours.len() as u8);
    if let Some(t) = transparent {
        palette.extend_from_slice(&[0, 0, 0]);
        indices.iter_mut().filter(|i| **i == u8::MAX).for_each(|i| *i = t);
    }
    if palette.is_empty() {
        palette.extend_from_slice(&[0, 0, 0]);
    }
    Some(gif::Frame::from_palette_pixels(w, h, indices, palette, transparent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ball moving over a still background of bands, 24 frames, each stored whole as
    /// simple tools write them
    fn animation(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut out, w as u16, h as u16, &[]).unwrap();
            encoder.set_repeat(gif::Repeat::Infinite).unwrap();
            for i in 0..24u32 {
                let frame = RgbaImage::from_fn(w, h, |x, y| {
                    let (cx, cy) = (40 + i * (w - 80) / 24, h / 2);
                    if (x as i64 - cx as i64).pow(2) + (y as i64 - cy as i64).pow(2) < 900 {
                        image::Rgba([220, 40, 40, 255])
                    } else {
                        image::Rgba([(x / 70 * 18) as u8, (y / 50 * 22) as u8, 120, 255])
                    }
                });
                let mut frame = exact_palette(frame.as_raw(), w as u16, h as u16).unwrap();
                frame.delay = 8;
                encoder.write_frame(&frame).unwrap();
            }
        }
        out
    }

    fn frames(data: &[u8]) -> Vec<image::Frame> {
        GifDecoder::new(Cursor::new(data)).unwrap().into_frames().collect_frames().unwrap()
    }

    /// The same over a grainy photo, each frame's colours learned anew, as most tools
    /// write them
    fn photo_animation(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 10);
            encoder.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
            for i in 0..24u32 {
                let mut seed = 5u32;
                let frame = RgbaImage::from_fn(w, h, |x, y| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    let noise = (seed >> 27) as u8;
                    let (cx, cy) = (40 + i * (w - 80) / 24, h / 2);
                    if (x as i64 - cx as i64).pow(2) + (y as i64 - cy as i64).pow(2) < 900 {
                        image::Rgba([220, 40, 40, 255])
                    } else {
                        image::Rgba([((x * 255 / w) as u8).wrapping_add(noise), ((y * 255 / h) as u8).wrapping_add(noise), 120, 255])
                    }
                });
                encoder.encode_frame(image::Frame::from_parts(frame, 0, 0, image::Delay::from_numer_denom_ms(80, 1))).unwrap();
            }
        }
        out
    }

    #[test]
    fn photo_animations_shrink_level_by_level() {
        let original = photo_animation(900, 450);
        let mut last = original.len();
        for level in [Level::Balanced, Level::Strong, Level::Extreme] {
            let out = encode_level(&original, level).unwrap();
            assert!(out.len() < last, "{level:?}: {} not below {last}", out.len());
            last = out.len();
        }
    }

    #[test]
    fn stays_animated_and_shrinks() {
        let original = animation(1000, 500);
        let mut last = original.len();
        for level in super::super::LEVELS {
            let out = encode_level(&original, level).unwrap();
            // Flat colours already exact at Light leave Balanced nothing more to let go
            if level == Level::Balanced {
                assert!(out.len() <= last, "{level:?}: {} above {last}", out.len());
            } else {
                assert!(out.len() < last, "{level:?}: {} not below {last}", out.len());
            }
            last = out.len();
            let decoded = frames(&out);
            let expected = if level == Level::Extreme { 12 } else { 24 };
            assert_eq!(decoded.len(), expected, "{level:?}");
            // The animation lasts as long as before
            let total: u32 = decoded.iter().map(|f| f.delay().numer_denom_ms().0 / f.delay().numer_denom_ms().1).sum();
            assert_eq!(total, 24 * 80, "{level:?}");
            let (w, _) = decoded[0].buffer().dimensions();
            assert_eq!(w, params(level).0.map_or(1000, |e| e.min(1000)), "{level:?}");
        }
        // Light loses nothing: every frame shows the same pixels as before
        let light = frames(&encode_level(&original, Level::Light).unwrap());
        for (a, b) in frames(&original).iter().zip(&light) {
            assert!(a.buffer().pixels().zip(b.buffer().pixels()).all(|(p, q)| p == q));
        }
    }
}
