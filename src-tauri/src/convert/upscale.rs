// AI upscaling (Photo > Tools > Upscale 2x / 4x): the part every system shares. The
// models run in src/platform/upscale.rs; faces are restored by src/convert/faces.rs.
//
// The upscalers read a fixed-size tile at a time, so the picture is cut into tiles that
// each carry a margin of their neighbours: the margin is upscaled too but thrown away, so
// no seam shows where tiles meet. 2x is the model's 4x result halved by averaging each
// 2 x 2 block, which keeps more detail than enlarging 2x directly. Transparency is not
// something the models know: the alpha channel is enlarged on its own (Lanczos).
//
// Small pictures (the ones that most need it) go through the detailed upscaler when this
// computer is quick enough for it; large ones through the fast one, ten to twenty-five
// times quicker. How quick it is gets measured: computers range from a Mac's Neural
// Engine or a gaming GPU (a detailed tile in a fraction of a second) to a laptop's CPU
// (15 s). A first guess for the hardware is corrected by every tile and face timed; when
// the detailed upscaler turns out far slower than guessed, the picture starts over with
// the fast one after a few tiles.
//
// The upscalers sharpen detail a pixel or two across. A picture that is large but blurry
// (an old photo shown bigger than it was taken, a screenshot of one, an enlarged image)
// has its detail spread over several pixels, and comes back hardly changed; such a
// picture is halved first, so the model reads it at the size its detail really has.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use image::{DynamicImage, GrayImage, RgbImage, RgbaImage};

use super::encode::{has_transparency, resize_exact};
use super::faces;
use crate::platform::faces::Face;
use crate::platform::upscale::{Upscaler, SCALE, TILE};

/// The largest result (64 megapixels: a 256 MB image); larger ones are refused
pub(super) const MAX_PIXELS: u64 = 64_000_000;

/// The detailed upscaler is for pictures up to Full HD (what the model reads first; to 4K
/// at 2x), whose tiles should take at most this long here: Full HD is 45 tiles, 13 s on
/// an M1 Pro's Neural Engine
const DETAILED_PIXELS: u64 = 2_100_000;
const DETAILED_SECONDS: f32 = 20.0;

/// Context kept around each tile, in pixels of the original
const MARGIN: usize = 16;
const STEP: usize = TILE - 2 * MARGIN;

/// Below this, a picture counts as blurry (see `softness`). Measured: enlarged or
/// out-of-focus photos 0.20-0.27; photos at their own size, AI images, screenshots,
/// charts and icons 0.34-0.51; pictures enlarged only 1.5x (still detailed) 0.34-0.37.
const SOFT: f64 = 0.29;

/// The machine learning upscaling needs: on this computer, or stand-ins in tests
pub(super) trait Models {
    /// One tile (3 x TILE x TILE planar floats, 0 to 1) made SCALE times larger
    fn upscale_tile(&mut self, upscaler: Upscaler, tile: &[f32]) -> Result<Vec<f32>, String>;
    /// The faces in a picture; none when they can't be looked for
    fn find_faces(&mut self, image: &RgbImage) -> Vec<Face>;
    /// Gets the face model ready while the upscaler works: it is about to be needed
    fn prepare_faces(&mut self);
    /// One aligned face (3 x FACE x FACE planar floats, -1 to 1), restored
    fn restore_face(&mut self, face: &[f32]) -> Result<Vec<f32>, String>;
    /// What each step takes here, as known so far
    fn timings(&mut self) -> Timings;
    /// What each step took during an upscale, for the next one
    fn learned(&mut self, timings: Timings);
}

/// What each step takes on this computer, in seconds
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Timings {
    fast: f32,
    detailed: f32,
    face: f32,
    /// How long the detailed upscaler may take on a picture
    budget: f32,
}

impl Timings {
    fn tile(&self, upscaler: Upscaler) -> f32 {
        match upscaler {
            Upscaler::Fast => self.fast,
            Upscaler::Detailed => self.detailed,
        }
    }
}

/// Measured while the app runs; first the guess for this hardware
static TIMINGS: Mutex<Option<Timings>> = Mutex::new(None);

/// The models of this computer (src/platform)
pub(super) struct Bundled;

impl Models for Bundled {
    fn upscale_tile(&mut self, upscaler: Upscaler, tile: &[f32]) -> Result<Vec<f32>, String> {
        crate::platform::upscale::upscale_tile(upscaler, tile)
    }

    fn find_faces(&mut self, image: &RgbImage) -> Vec<Face> {
        crate::platform::faces::find_faces(image).unwrap_or_default()
    }

    fn prepare_faces(&mut self) {
        crate::platform::upscale::prepare_faces();
    }

    fn restore_face(&mut self, face: &[f32]) -> Result<Vec<f32>, String> {
        crate::platform::upscale::restore_face(face)
    }

    fn timings(&mut self) -> Timings {
        let mut timings = TIMINGS.lock().unwrap_or_else(|e| e.into_inner());
        *timings.get_or_insert_with(|| {
            let [fast, detailed, face] = crate::platform::upscale::expected_seconds();
            Timings { fast, detailed, face, budget: DETAILED_SECONDS }
        })
    }

    fn learned(&mut self, learned: Timings) {
        *TIMINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(learned);
    }
}

/// An upscale under way: how far it has got, reported as (done, total) in milliseconds
/// after every tile and face, and what those took
pub(super) struct Work<'a> {
    done: usize,
    total: usize,
    report: &'a dyn Fn(usize, usize),
    /// What a tile and a face count for in `total`, as planned
    costs: (usize, usize),
    timings: Timings,
    /// Tiles of each upscaler, and faces, done so far
    seen: [usize; 3],
}

impl<'a> Work<'a> {
    fn new(timings: Timings, report: &'a dyn Fn(usize, usize)) -> Self {
        Work { done: 0, total: 0, report, costs: (0, 0), timings, seen: [0; 3] }
    }

    /// What is left: `tiles` of `upscaler`, then `faces`
    fn plan(&mut self, upscaler: Upscaler, tiles: usize, faces: usize) {
        self.costs = (millis(self.timings.tile(upscaler)), millis(self.timings.face));
        self.total = self.done + tiles * self.costs.0 + faces * self.costs.1;
    }

    fn tile_done(&mut self, upscaler: Upscaler, took: Duration) {
        let (slot, kind) = match upscaler {
            Upscaler::Fast => (&mut self.timings.fast, 0),
            Upscaler::Detailed => (&mut self.timings.detailed, 1),
        };
        learn(slot, &mut self.seen[kind], took);
        self.step(self.costs.0);
    }

    pub(super) fn face_done(&mut self, took: Duration) {
        learn(&mut self.timings.face, &mut self.seen[2], took);
        self.step(self.costs.1);
    }

    /// Whether the detailed upscaler, as timed on the tiles done, would take far longer
    /// than it may on `tiles`
    fn too_slow(&self, tiles: usize) -> bool {
        self.seen[1] >= 3 && self.timings.detailed * tiles as f32 > 2.0 * self.timings.budget
    }

    fn step(&mut self, cost: usize) {
        self.done = (self.done + cost).min(self.total);
        (self.report)(self.done, self.total);
    }
}

fn millis(seconds: f32) -> usize {
    ((seconds * 1000.0).ceil() as usize).max(1)
}

/// Corrects what a step takes with one more timing. The first of a picture may include
/// loading the model, so it isn't counted; the next replaces the guess, later ones average in.
fn learn(slot: &mut f32, seen: &mut usize, took: Duration) {
    let took = took.as_secs_f32();
    match *seen {
        0 => {}
        1 => *slot = took,
        _ => *slot = 0.7 * *slot + 0.3 * took,
    }
    *seen += 1;
}

/// Why enlarging stopped
enum Stop {
    /// The detailed upscaler is far too slow on this computer
    TooSlow,
    Failed(String),
}

impl From<String> for Stop {
    fn from(message: String) -> Self {
        Stop::Failed(message)
    }
}

/// `img` made `factor` times larger (2 or 4), with the number of faces restored in it.
/// `report` hears how far it has got: (done, total).
pub(super) fn upscale(
    img: &DynamicImage,
    factor: u32,
    models: &mut dyn Models,
    report: &dyn Fn(usize, usize),
) -> Result<(DynamicImage, usize), String> {
    assert!(factor == 2 || factor == 4, "upscaling by {factor} is not supported");
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let (out_w, out_h) = (w * factor, h * factor);
    let soft = softness(&rgb).is_some_and(|soft| soft < SOFT);
    let half = soft.then(|| resize_exact(&DynamicImage::ImageRgb8(rgb.clone()), (w / 2).max(1), (h / 2).max(1)).to_rgb8());
    let source = half.as_ref().unwrap_or(&rgb);
    let pixels = u64::from(source.width()) * u64::from(source.height());
    let tiles = (source.width() as usize).div_ceil(STEP) * (source.height() as usize).div_ceil(STEP);
    let timings = models.timings();
    let detailed = pixels <= DETAILED_PIXELS && tiles as f32 * timings.detailed <= timings.budget;
    let upscaler = if detailed { Upscaler::Detailed } else { Upscaler::Fast };

    let found = models.find_faces(&rgb);
    let faces = found.iter().filter(|face| faces::worth_restoring(face, soft)).count();
    if faces > 0 {
        models.prepare_faces();
    }
    let mut work = Work::new(timings, report);
    work.plan(upscaler, tiles, faces);
    let factor_read = if half.is_some() { 4 } else { factor as usize };
    let result = match enlarge(source, factor_read, upscaler, models, &mut work) {
        Err(Stop::TooSlow) => {
            work.plan(Upscaler::Fast, tiles, faces);
            enlarge(source, factor_read, Upscaler::Fast, models, &mut work)
        }
        result => result,
    };
    let enlarged = result.map_err(|stop| match stop {
        Stop::Failed(message) => message,
        Stop::TooSlow => unreachable!("the fast upscaler is never given up on"),
    })?;

    // A halved picture read 4x is the original at 2x. For 4x, the rest of the way is
    // Lanczos (another pass of a model makes it look painted); an odd width or height
    // also lost its last pixel when halved.
    let mut larger = if enlarged.dimensions() == (out_w, out_h) {
        enlarged
    } else {
        resize_exact(&DynamicImage::ImageRgb8(enlarged), out_w, out_h).to_rgb8()
    };

    let restored = faces::restore(&rgb, &mut larger, &found, factor, soft, models, &mut work)?;
    models.learned(work.timings);

    if !has_transparency(img) {
        return Ok((DynamicImage::ImageRgb8(larger), restored));
    }
    let rgba = img.to_rgba8();
    let alpha = GrayImage::from_fn(w, h, |x, y| image::Luma([rgba.get_pixel(x, y)[3]]));
    let alpha = resize_exact(&DynamicImage::ImageLuma8(alpha), out_w, out_h).to_luma8();
    let combined = RgbaImage::from_fn(out_w, out_h, |x, y| {
        let [r, g, b] = larger.get_pixel(x, y).0;
        image::Rgba([r, g, b, alpha.get_pixel(x, y)[0]])
    });
    Ok((DynamicImage::ImageRgba8(combined), restored))
}

/// One pass of an upscaler over the whole picture, tile by tile: `factor` (2 or 4) times
/// larger. The detailed upscaler is given up on when it turns out far too slow here.
fn enlarge(rgb: &RgbImage, factor: usize, upscaler: Upscaler, models: &mut dyn Models, work: &mut Work) -> Result<RgbImage, Stop> {
    let shrink = SCALE / factor;
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let (out_w, out_h) = (w * factor, h * factor);
    let pixels = rgb.as_raw();
    let mut out = vec![0u8; out_w * out_h * 3];

    let step = STEP;
    let tiles = w.div_ceil(step) * h.div_ceil(step);
    let side = TILE * SCALE;
    let mut tile = vec![0f32; 3 * TILE * TILE];
    for top in (0..h).step_by(step) {
        for left in (0..w).step_by(step) {
            // The tile and its margin; beyond the picture's edge, its edge pixels repeat
            for y in 0..TILE {
                let row = (top + y).saturating_sub(MARGIN).min(h - 1) * w;
                for x in 0..TILE {
                    let at = (row + (left + x).saturating_sub(MARGIN).min(w - 1)) * 3;
                    for c in 0..3 {
                        tile[(c * TILE + y) * TILE + x] = f32::from(pixels[at + c]) / 255.0;
                    }
                }
            }
            let started = Instant::now();
            let up = models.upscale_tile(upscaler, &tile)?;
            let took = started.elapsed();
            if up.len() != 3 * side * side {
                return Err(Stop::Failed("upscaling: the model gave a tile of the wrong size".into()));
            }

            // Its middle, without the margin, at its place in the result
            let rows = step.min(h - top) * factor;
            let cols = step.min(w - left) * factor;
            let skip = MARGIN * SCALE;
            let norm = 255.0 / (shrink * shrink) as f32;
            for y in 0..rows {
                let line = &mut out[((top * factor + y) * out_w + left * factor) * 3..][..cols * 3];
                for x in 0..cols {
                    for c in 0..3 {
                        let mut sum = 0.0;
                        for dy in 0..shrink {
                            let row = &up[(c * side + skip + y * shrink + dy) * side + skip + x * shrink..][..shrink];
                            sum += row.iter().map(|v| v.clamp(0.0, 1.0)).sum::<f32>();
                        }
                        line[x * 3 + c] = (sum * norm).round() as u8;
                    }
                }
            }
            work.tile_done(upscaler, took);
            if upscaler == Upscaler::Detailed && work.too_slow(tiles) {
                return Err(Stop::TooSlow);
            }
        }
    }
    Ok(RgbImage::from_raw(out_w as u32, out_h as u32, out).expect("sized above"))
}

/// How much detail the picture holds for its size: low when it is blurry. Halved and
/// enlarged back, a blurry picture comes out almost the same, compared with how much its
/// brightness varies from pixel to pixel; a sharp one loses a good part of that variation.
/// Measured on the middle of the picture, at most 1024 x 1024 px; None when too small or
/// flat to tell.
fn softness(rgb: &RgbImage) -> Option<f64> {
    const CROP: u32 = 1024;
    let (w, h) = rgb.dimensions();
    if w < 64 || h < 64 {
        return None;
    }
    let (cw, ch) = ((w.min(CROP) & !1) as usize, (h.min(CROP) & !1) as usize);
    let (x0, y0) = (((w as usize) - cw) / 2, ((h as usize) - ch) / 2);
    let raw = rgb.as_raw();
    let luma: Vec<f32> = (0..ch)
        .flat_map(|y| (0..cw).map(move |x| (y, x)))
        .map(|(y, x)| {
            let at = ((y0 + y) * w as usize + x0 + x) * 3;
            0.299 * f32::from(raw[at]) + 0.587 * f32::from(raw[at + 1]) + 0.114 * f32::from(raw[at + 2])
        })
        .collect();

    // Halved: the average of each 2 x 2 block
    let (sw, sh) = (cw / 2, ch / 2);
    let small: Vec<f32> = (0..sh)
        .flat_map(|y| (0..sw).map(move |x| (y, x)))
        .map(|(y, x)| {
            let at = 2 * y * cw + 2 * x;
            (luma[at] + luma[at + 1] + luma[at + cw] + luma[at + cw + 1]) / 4.0
        })
        .collect();
    // Enlarged back: bilinear, pixel centres aligned
    let back = |x: usize, y: usize| {
        let fx = ((x as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (sw - 1) as f32);
        let fy = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (x1, y1) = (fx as usize, fy as usize);
        let (x2, y2) = ((x1 + 1).min(sw - 1), (y1 + 1).min(sh - 1));
        let (tx, ty) = (fx - x1 as f32, fy - y1 as f32);
        let top = small[y1 * sw + x1] * (1.0 - tx) + small[y1 * sw + x2] * tx;
        let bottom = small[y2 * sw + x1] * (1.0 - tx) + small[y2 * sw + x2] * tx;
        top * (1.0 - ty) + bottom * ty
    };

    let (mut lost, mut variation) = (0f64, 0f64);
    for y in 0..ch {
        for x in 0..cw {
            let value = luma[y * cw + x];
            lost += f64::from((value - back(x, y)).abs());
            if x + 1 < cw && y + 1 < ch {
                variation += f64::from((luma[y * cw + x + 1] - value).abs() + (luma[(y + 1) * cw + x] - value).abs());
            }
        }
    }
    (variation > 0.0).then(|| lost / variation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::upscale::FACE;

    /// Stand-ins for the models: every pixel becomes a SCALE x SCALE block of itself,
    /// faces come back as they went in; counts what was asked of them
    struct Nearest {
        tiles: Vec<Upscaler>,
        faces: Vec<Face>,
        prepared: bool,
        restored: usize,
        fail: bool,
        /// How long a detailed tile really takes
        detailed: Duration,
        timings: Timings,
    }

    /// As guessed for a Mac with a Neural Engine
    const MAC: Timings = Timings { fast: 0.027, detailed: 0.3, face: 0.5, budget: DETAILED_SECONDS };

    impl Default for Nearest {
        fn default() -> Self {
            Nearest { tiles: vec![], faces: vec![], prepared: false, restored: 0, fail: false, detailed: Duration::ZERO, timings: MAC }
        }
    }

    impl Models for Nearest {
        fn upscale_tile(&mut self, upscaler: Upscaler, tile: &[f32]) -> Result<Vec<f32>, String> {
            if self.fail {
                return Err("no model".into());
            }
            if upscaler == Upscaler::Detailed {
                std::thread::sleep(self.detailed);
            }
            self.tiles.push(upscaler);
            let side = TILE * SCALE;
            let mut out = vec![0f32; 3 * side * side];
            for c in 0..3 {
                for y in 0..side {
                    for x in 0..side {
                        out[(c * side + y) * side + x] = tile[(c * TILE + y / SCALE) * TILE + x / SCALE];
                    }
                }
            }
            Ok(out)
        }

        fn find_faces(&mut self, _image: &RgbImage) -> Vec<Face> {
            self.faces.clone()
        }

        fn prepare_faces(&mut self) {
            self.prepared = true;
        }

        fn restore_face(&mut self, face: &[f32]) -> Result<Vec<f32>, String> {
            assert!(self.prepared, "the face model is prepared before it is needed");
            assert_eq!(face.len(), 3 * FACE * FACE);
            self.restored += 1;
            Ok(face.to_vec())
        }

        fn timings(&mut self) -> Timings {
            self.timings
        }

        fn learned(&mut self, timings: Timings) {
            self.timings = timings;
        }
    }

    fn sample(w: u32, h: u32, alpha: bool) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x * 7 % 256) as u8, (y * 13 % 256) as u8, ((x + y) % 256) as u8, if alpha { (x % 256) as u8 } else { 255 }])
        }))
    }

    #[test]
    fn tiles_cover_the_image_without_seams() {
        // Wider and taller than a tile, and not a multiple of one
        let img = sample(530, 301, false);
        for factor in [4, 2] {
            let mut models = Nearest::default();
            let (up, faces) = upscale(&img, factor, &mut models, &|_, _| {}).unwrap();
            assert_eq!(faces, 0);
            assert!(matches!(up, DynamicImage::ImageRgb8(_)), "an opaque image stays without alpha");
            assert_eq!((up.width(), up.height()), (530 * factor, 301 * factor));
            assert!(models.tiles.iter().all(|&u| u == Upscaler::Detailed), "a small picture gets the detailed upscaler");
            let src = img.to_rgba8();
            let up = up.to_rgb8();
            for (x, y, pixel) in up.enumerate_pixels() {
                let original = src.get_pixel(x / factor, y / factor);
                assert_eq!(pixel.0, [original[0], original[1], original[2]], "{factor}x at {x},{y}");
            }
        }
    }

    #[test]
    fn large_pictures_get_the_fast_upscaler() {
        let mut models = Nearest::default();
        upscale(&sample(1800, 1200, false), 2, &mut models, &|_, _| {}).unwrap();
        assert!(!models.tiles.is_empty() && models.tiles.iter().all(|&u| u == Upscaler::Fast));
    }

    #[test]
    fn transparency_is_kept() {
        let img = sample(40, 30, true);
        let (up, _) = upscale(&img, 2, &mut Nearest::default(), &|_, _| {}).unwrap();
        let DynamicImage::ImageRgba8(up) = up else { panic!("alpha was dropped") };
        assert_eq!(up.dimensions(), (80, 60));
        // Left edge transparent, right edge nearly opaque, as in the original
        assert!(up.get_pixel(0, 30)[3] < 8);
        assert!(up.get_pixel(79, 30)[3] > 30);
    }

    #[test]
    fn a_single_pixel() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, image::Rgb([10, 200, 30])));
        let (up, _) = upscale(&img, 4, &mut Nearest::default(), &|_, _| {}).unwrap();
        let up = up.to_rgb8();
        assert_eq!(up.dimensions(), (4, 4));
        assert!(up.pixels().all(|p| p.0 == [10, 200, 30]));
    }

    /// Fine-grained texture: every pixel differs from its neighbours
    fn texture(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let v = ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).wrapping_mul(2_246_822_519) >> 24) as u8;
            image::Rgb([v, v / 2 + 60, 255 - v])
        })
    }

    #[test]
    fn blurry_pictures_are_recognised() {
        let sharp = texture(300, 200);
        assert!(softness(&sharp).unwrap() > SOFT, "{:?}", softness(&sharp));
        // The same texture shown three times larger: a third of the detail per pixel
        let enlarged = resize_exact(&DynamicImage::ImageRgb8(sharp), 900, 600).to_rgb8();
        assert!(softness(&enlarged).unwrap() < SOFT, "{:?}", softness(&enlarged));
        assert_eq!(softness(&RgbImage::from_pixel(200, 200, image::Rgb([9, 9, 9]))), None, "flat");
        assert_eq!(softness(&texture(40, 40)), None, "too small to tell");
    }

    #[test]
    fn blurry_pictures_are_halved_first() {
        let soft = DynamicImage::ImageRgb8(resize_exact(&DynamicImage::ImageRgb8(texture(150, 101)), 451, 303).to_rgb8());
        let mut models = Nearest::default();
        for factor in [2, 4] {
            let (up, _) = upscale(&soft, factor, &mut models, &|_, _| {}).unwrap();
            assert_eq!((up.width(), up.height()), (451 * factor, 303 * factor), "{factor}x keeps the exact size");
        }
        // The detailed upscaler over the halved picture (225 x 151: 2 tiles), for 2x and
        // for 4x; Lanczos does the rest of the way to 4x
        assert_eq!(models.tiles, [Upscaler::Detailed; 4]);
    }

    #[test]
    fn faces_are_restored_and_blended_in() {
        // A face as Vision would report it: eyes 60 px apart in a 400 x 300 picture
        let face = Face { points: [[170.0, 120.0], [230.0, 120.0], [200.0, 155.0], [175.0, 185.0], [225.0, 185.0]] };
        let tiny = Face { points: [[10.0, 10.0], [14.0, 10.0], [12.0, 13.0], [10.0, 15.0], [14.0, 15.0]] };
        let mut models = Nearest { faces: vec![face, tiny], ..Default::default() };
        let img = DynamicImage::ImageRgb8(texture(400, 300));
        let (up, faces) = upscale(&img, 4, &mut models, &|_, _| {}).unwrap();
        assert_eq!((faces, models.restored), (1, 1), "the face with eyes 4 px apart is too small to restore");
        assert_eq!((up.width(), up.height()), (1600, 1200));
        // Restored as it went in (stand-in model), the face blends back without a visible
        // change at its centre or its edge
        let (up, plain) = (up.to_rgb8(), resize_exact(&img, 1600, 1200).to_rgb8());
        let diff = |x: u32, y: u32| (0..3).map(|c| i32::from(up.get_pixel(x, y)[c]) - i32::from(plain.get_pixel(x, y)[c])).map(i32::abs).max().unwrap();
        assert!(diff(800, 600) < 70, "{}", diff(800, 600));
    }

    #[test]
    fn progress_moves_forward_to_the_end() {
        let face = Face { points: [[170.0, 120.0], [230.0, 120.0], [200.0, 155.0], [175.0, 185.0], [225.0, 185.0]] };
        let mut models = Nearest { faces: vec![face], ..Default::default() };
        let steps = std::cell::RefCell::new(vec![]);
        upscale(&DynamicImage::ImageRgb8(texture(500, 300)), 2, &mut models, &|done, total| steps.borrow_mut().push((done, total)))
            .unwrap();
        let steps = steps.into_inner();
        // 3 x 2 tiles, then the face
        assert_eq!(steps.len(), 7, "{steps:?}");
        assert!(steps.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 == w[1].1), "{steps:?}");
        let (done, total) = *steps.last().unwrap();
        assert_eq!(done, total);
        assert_eq!(total, 6 * millis(MAC.detailed) + millis(MAC.face), "planned with the timings known before");
    }

    #[test]
    fn a_slow_computer_gets_the_fast_upscaler() {
        // Guessed quick enough for the detailed upscaler, but its tiles turn out to take
        // 20 ms: 9 tiles would be 0.18 s, far beyond the 0.03 s it may take
        let timings = Timings { fast: 0.0001, detailed: 0.001, face: 0.001, budget: 0.03 };
        let mut models = Nearest { timings, detailed: Duration::from_millis(20), ..Default::default() };
        let steps = std::cell::RefCell::new(vec![]);
        let img = DynamicImage::ImageRgb8(texture(600, 600));
        let (up, _) = upscale(&img, 2, &mut models, &|done, total| steps.borrow_mut().push((done, total))).unwrap();
        assert_eq!((up.width(), up.height()), (1200, 1200));
        // Given up on after 3 tiles (the first isn't timed), then all 9 tiles again
        assert_eq!(models.tiles[..3], [Upscaler::Detailed; 3]);
        assert_eq!(models.tiles[3..], [Upscaler::Fast; 9]);
        let steps = steps.into_inner();
        assert!(steps.windows(2).all(|w| w[0].0 < w[1].0), "progress never goes back: {steps:?}");
        assert_eq!(steps.last().unwrap().0, steps.last().unwrap().1);
        // Remembered: the next picture goes to the fast upscaler straight away
        assert!(models.timings.detailed >= 0.02, "{:?}", models.timings);
        models.tiles.clear();
        upscale(&img, 2, &mut models, &|_, _| {}).unwrap();
        assert_eq!(models.tiles, [Upscaler::Fast; 9]);
    }

    #[test]
    fn timings_are_learned() {
        let mut slot = 9.0;
        let mut seen = 0;
        learn(&mut slot, &mut seen, Duration::from_secs(5));
        assert_eq!(slot, 9.0, "the first may include loading the model");
        learn(&mut slot, &mut seen, Duration::from_secs(2));
        assert_eq!(slot, 2.0, "the next replaces the guess");
        learn(&mut slot, &mut seen, Duration::from_secs(4));
        assert!((slot - 2.6).abs() < 1e-5, "{slot}");
    }

    #[test]
    fn no_faces_no_face_model() {
        let mut models = Nearest::default();
        upscale(&DynamicImage::ImageRgb8(texture(300, 200)), 2, &mut models, &|_, _| {}).unwrap();
        assert!(!models.prepared, "the face model is only loaded for a face");
    }

    #[test]
    fn model_errors_are_reported() {
        let err = upscale(&sample(8, 8, false), 4, &mut Nearest { fail: true, ..Default::default() }, &|_, _| {}).unwrap_err();
        assert_eq!(err, "no model");
    }

    /// The real models on this Mac, on the case they exist for: a sharp picture that only
    /// survives large but blurry (shrunk to a third, shown 3x larger again). Upscaled 2x,
    /// it must come closer to the sharp original than plain enlarging does.
    #[test]
    fn the_models_bring_back_sharpness() {
        if cfg!(target_os = "macos") || onnx_fetched() {
            brings_back_sharpness(&mut Bundled);
        }
    }

    /// The real face model draws a face from what it is given (with its 8-bit weights on
    /// the Neural Engine, it returns a flat grey image instead)
    #[test]
    fn the_face_model_follows_the_face() {
        if cfg!(target_os = "macos") || onnx_fetched() {
            follows_the_face(&mut Bundled);
        }
    }

    /// The Windows and Linux engine (ONNX Runtime and the ONNX models), here on a Mac
    #[cfg(target_os = "macos")]
    struct Onnx;

    #[cfg(target_os = "macos")]
    impl Models for Onnx {
        fn upscale_tile(&mut self, upscaler: Upscaler, tile: &[f32]) -> Result<Vec<f32>, String> {
            crate::platform::upscale::onnx_models::upscale_tile(upscaler, tile)
        }

        fn find_faces(&mut self, image: &RgbImage) -> Vec<Face> {
            crate::platform::yunet::find_faces(image).unwrap()
        }

        fn prepare_faces(&mut self) {
            crate::platform::upscale::onnx_models::prepare_faces();
        }

        fn restore_face(&mut self, face: &[f32]) -> Result<Vec<f32>, String> {
            crate::platform::upscale::onnx_models::restore_face(face)
        }

        fn timings(&mut self) -> Timings {
            let [fast, detailed, face] = crate::platform::upscale::onnx_models::expected_seconds();
            Timings { fast, detailed, face, budget: DETAILED_SECONDS }
        }

        fn learned(&mut self, _timings: Timings) {}
    }

    /// Whether the ONNX engine and models have been fetched (on a Mac:
    /// FLIPFILE_ONNX=1 npm run fetch-onnxruntime && FLIPFILE_ONNX=1 npm run fetch-models)
    fn onnx_fetched() -> bool {
        let fetched = crate::platform::onnx::available()
            && ["upscale.onnx", "upscale-large.onnx", "faces.onnx", "yunet.onnx"]
                .iter()
                .all(|m| crate::platform::upscale::model_path(m).is_some());
        if !fetched {
            eprintln!("skipped: the ONNX engine (Windows and Linux) is not fetched here");
        }
        fetched
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_onnx_models_bring_back_sharpness() {
        if onnx_fetched() {
            brings_back_sharpness(&mut Onnx);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_onnx_face_model_follows_the_face() {
        if onnx_fetched() {
            follows_the_face(&mut Onnx);
        }
    }

    fn brings_back_sharpness(models: &mut dyn Models) {
        // Bars and blocks with hard edges, like text or a logo
        let truth = RgbImage::from_fn(600, 240, |x, y| {
            let ink = (x / 30 % 3 == 0 && y / 20 % 2 == 0) || (x % 150 > 100 && y > 120 && y < 200);
            if ink { image::Rgb([30, 40, 90]) } else { image::Rgb([240, 236, 225]) }
        });
        let truth_img = DynamicImage::ImageRgb8(truth.clone());
        let blurry = resize_exact(&resize_exact(&truth_img, 100, 40), 300, 120);
        assert!(softness(&blurry.to_rgb8()).unwrap() < SOFT, "the test picture should count as blurry");

        let (up, _) = upscale(&blurry, 2, models, &|_, _| {}).unwrap();
        let up = up.to_rgb8();
        assert_eq!(up.dimensions(), (600, 240));
        let plain = resize_exact(&blurry, 600, 240).to_rgb8();
        let error = |img: &RgbImage| -> u64 {
            img.pixels().zip(truth.pixels()).map(|(a, b)| u64::from(a[0].abs_diff(b[0])) + u64::from(a[2].abs_diff(b[2]))).sum()
        };
        let (ai, lanczos) = (error(&up), error(&plain));
        assert!(ai < lanczos, "AI {ai} should be closer to the sharp original than Lanczos {lanczos}");
    }

    fn follows_the_face(models: &mut dyn Models) {
        // A rough face where the template has one: skin over a dark background, dark eyes and mouth
        let shade = |x: f32, y: f32| -> [f32; 3] {
            let inside = |cx: f32, cy: f32, rx: f32, ry: f32| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) < 1.0;
            if inside(193.0, 240.0, 22.0, 12.0) || inside(319.0, 240.0, 22.0, 12.0) {
                [-0.8, -0.85, -0.85]
            } else if inside(257.0, 372.0, 56.0, 12.0) {
                [0.1, -0.5, -0.5]
            } else if inside(256.0, 285.0, 190.0, 235.0) {
                [0.55, 0.2, 0.0]
            } else {
                [-0.5, -0.45, -0.4]
            }
        };
        let mut face = vec![0f32; 3 * FACE * FACE];
        for y in 0..FACE {
            for x in 0..FACE {
                let colour = shade(x as f32, y as f32);
                for c in 0..3 {
                    face[(c * FACE + y) * FACE + x] = colour[c];
                }
            }
        }
        let restored = models.restore_face(&face).unwrap();
        assert_eq!(restored.len(), face.len());
        // Brightness in and out: it varies, and where it went in
        let luma = |v: &[f32]| (0..FACE * FACE).map(|i| v[i] + v[FACE * FACE + i] + v[2 * FACE * FACE + i]).collect::<Vec<f32>>();
        let (a, b) = (luma(&face), luma(&restored));
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let (ma, mb) = (mean(&a), mean(&b));
        let cov: f32 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
        let var = |v: &[f32], m: f32| v.iter().map(|x| (x - m).powi(2)).sum::<f32>();
        let spread = (var(&b, mb) / b.len() as f32).sqrt();
        let correlation = cov / (var(&a, ma) * var(&b, mb)).sqrt();
        assert!(spread > 0.2, "the restored face is flat (spread {spread})");
        assert!(correlation > 0.6, "the restored face doesn't follow the input (correlation {correlation})");
    }
}
