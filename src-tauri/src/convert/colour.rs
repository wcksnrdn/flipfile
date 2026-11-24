// Colour around the AI upscalers (src/convert/upscale.rs) and the Colorize tool:
// - `lock`: an upscaler shifts colours a little over large areas; that drift is taken
//   back, so the result keeps the photo's own colours. With `fresh`, Upscale Photo also
//   gives dull colours a little more life and the picture a little more local contrast
//   ("clarity"), never changing a hue: vivid colours, highlights and shadows are left
//   as they are.
// - `monochrome`, `grey_input`, `colorized`: Colorize (Photo > Tools) colours a black-
//   and-white or toned photo, the colours coming from a model (DDColor), its lightness
//   kept.
// Pixels are worked on a row at a time (results reach 200 MP); colours are compared in
// CIE Lab (D65), lightness 0 to 100.

use image::RgbImage;

/// sRGB (0 to 1) to Lab
pub(super) fn to_lab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let lin = |c: f32| if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Lab to sRGB, 0 to 1 (colours outside sRGB clipped)
pub(super) fn from_lab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let inv = |t: f32| if t > 0.206_893 { t * t * t } else { (t - 16.0 / 116.0) / 7.787 };
    let (x, y, z) = (inv(fx) * 0.950_47, inv(fy), inv(fz) * 1.088_83);
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    let gamma = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
    };
    [gamma(r), gamma(g), gamma(b)]
}

fn rgb(p: &[u8]) -> [f32; 3] {
    [f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0]
}

fn bytes([r, g, b]: [f32; 3]) -> [u8; 3] {
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(r), q(g), q(b)]
}

/// Runs `f` on every row of a `width`-pixel RGB buffer, the rows shared between threads
fn par_rows(buffer: &mut [u8], width: usize, f: impl Fn(usize, &mut [u8]) + Sync) {
    let row = width * 3;
    let rows = buffer.len() / row.max(1);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(rows).max(1);
    let per = rows.div_ceil(threads);
    std::thread::scope(|s| {
        for (i, chunk) in buffer.chunks_mut(per * row).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (j, line) in chunk.chunks_mut(row).enumerate() {
                    f(i * per + j, line);
                }
            });
        }
    });
}

/// Every `step`-th pixel of the picture, about 512 x 512 of them: enough to judge it
fn sample(img: &RgbImage) -> impl Iterator<Item = [f32; 3]> + '_ {
    let (w, h) = img.dimensions();
    let step = ((u64::from(w) * u64::from(h) / 262_144) as f64).sqrt().max(1.0) as u32;
    (0..h).step_by(step as usize).flat_map(move |y| (0..w).step_by(step as usize).map(move |x| rgb(&img.get_pixel(x, y).0)))
}

/// Black and white, or toned (sepia, cyanotype): its colours hardly vary, or all lie
/// along one hue and none is strong (a toned print's colour grows with its lightness)
pub(super) fn monochrome(img: &RgbImage) -> bool {
    let (mut n, mut sa, mut sb, mut saa, mut sbb) = (0f64, 0f64, 0f64, 0f64, 0f64);
    let (mut hue_x, mut hue_y, mut tinted, mut strong) = (0f64, 0f64, 0f64, 0f64);
    for p in sample(img) {
        let [_, a, b] = to_lab(p);
        let (a, b) = (f64::from(a), f64::from(b));
        n += 1.0;
        sa += a;
        sb += b;
        saa += a * a;
        sbb += b * b;
        let chroma = a.hypot(b);
        if chroma > 3.0 {
            tinted += 1.0;
            hue_x += a / chroma;
            hue_y += b / chroma;
        }
        if chroma > 30.0 {
            strong += 1.0;
        }
    }
    if n < 64.0 {
        return false;
    }
    let spread = ((saa / n - (sa / n).powi(2)) + (sbb / n - (sb / n).powi(2))).max(0.0).sqrt();
    // How closely the tinted pixels share one hue: 1 when all do
    let one_hue = if tinted > 0.0 { hue_x.hypot(hue_y) / tinted } else { 1.0 };
    spread < 4.0 || (one_hue > 0.97 && strong < 0.01 * n)
}

pub(super) use crate::platform::upscale::COLORIZE;

/// A photo's lightness as the colouring model reads it: grey, COLORIZE x COLORIZE,
/// planar RGB 0 to 1
pub(super) fn grey_input(img: &RgbImage) -> Vec<f32> {
    let small = image::imageops::resize(img, COLORIZE as u32, COLORIZE as u32, image::imageops::FilterType::Triangle);
    let mut out = vec![0f32; 3 * COLORIZE * COLORIZE];
    for (i, p) in small.pixels().enumerate() {
        let [l, _, _] = to_lab(rgb(&p.0));
        let [grey, _, _] = from_lab([l, 0.0, 0.0]);
        for c in 0..3 {
            out[c * COLORIZE * COLORIZE + i] = grey;
        }
    }
    out
}

/// The photo coloured: its own lightness, the model's colour (`ab`: 2 x COLORIZE x
/// COLORIZE planar Lab a and b) stretched over it
pub(super) fn colorized(img: &RgbImage, ab: &[f32]) -> RgbImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let side = COLORIZE;
    let at = |c: usize, fx: f32, fy: f32| {
        let (x1, y1) = (fx as usize, fy as usize);
        let (x2, y2) = ((x1 + 1).min(side - 1), (y1 + 1).min(side - 1));
        let (tx, ty) = (fx.fract(), fy.fract());
        let v = |x: usize, y: usize| ab[(c * side + y) * side + x];
        (v(x1, y1) * (1.0 - tx) + v(x2, y1) * tx) * (1.0 - ty) + (v(x1, y2) * (1.0 - tx) + v(x2, y2) * tx) * ty
    };
    let mut out = img.clone();
    par_rows(&mut out, w, |y, line| {
        let fy = ((y as f32 + 0.5) * side as f32 / h as f32 - 0.5).clamp(0.0, (side - 1) as f32);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * side as f32 / w as f32 - 0.5).clamp(0.0, (side - 1) as f32);
            let p = &mut line[x * 3..x * 3 + 3];
            let [l, _, _] = to_lab(rgb(p));
            p.copy_from_slice(&bytes(from_lab([l, at(0, fx, fy), at(1, fx, fy)])));
        }
    });
    out
}

/// The side, at most, of the pictures the colour drift is measured on
const DRIFT: u32 = 512;

/// How much more chroma a dull colour gets with `fresh` (a vivid one none)
const VIBRANCE: f32 = 0.15;
/// Chroma from which a colour counts as vivid, untouched
const VIVID: f32 = 55.0;
/// How much of the local contrast is added again with `fresh`
const CLARITY: f32 = 0.18;

/// Undoes the upscaler's colour drift: what `out` changed in colour and brightness over
/// large areas, compared with `reference` (what the upscaler read), is taken back; its
/// fine detail stays. With `fresh`, dull colours get a little more chroma and mid tones
/// a little more local contrast, hues unchanged.
pub(super) fn lock(out: &mut RgbImage, reference: &RgbImage, fresh: bool) {
    let (w, h) = out.dimensions();
    let scale = (f64::from(DRIFT) / f64::from(w.max(h))).min(1.0);
    let (sw, sh) = (((f64::from(w) * scale) as u32).max(1), ((f64::from(h) * scale) as u32).max(1));
    // Area averages, so neither picture's fine detail aliases into the comparison
    let shrink = |img: &RgbImage| super::encode::resize_exact(&image::DynamicImage::ImageRgb8(img.clone()), sw, sh).to_rgb8();
    let (small, base) = (shrink(out), shrink(reference));
    let mut drift: Vec<[f32; 3]> = small
        .pixels()
        .zip(base.pixels())
        .map(|(o, r)| {
            let (o, r) = (to_lab(rgb(&o.0)), to_lab(rgb(&r.0)));
            [o[0] - r[0], o[1] - r[1], o[2] - r[2]]
        })
        .collect();
    blur(&mut drift, sw as usize, sh as usize, f32::from(sw.max(sh) as u16) / 30.0);
    // The lightness around each pixel at a large radius, for clarity (of the result with
    // its drift taken back)
    let mut around: Vec<[f32; 3]> = small
        .pixels()
        .zip(&drift)
        .map(|(o, d)| [to_lab(rgb(&o.0))[0] - d[0], 0.0, 0.0])
        .collect();
    blur(&mut around, sw as usize, sh as usize, f32::from(sw.max(sh) as u16) / 60.0);
    let (sw, sh) = (sw as usize, sh as usize);
    let width = w as usize;
    par_rows(out, width, |y, line| {
        let fy = ((y as f32 + 0.5) * sh as f32 / h as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y1, ty) = (fy as usize, fy.fract());
        let y2 = (y1 + 1).min(sh - 1);
        for x in 0..width {
            let fx = ((x as f32 + 0.5) * sw as f32 / w as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x1, tx) = (fx as usize, fx.fract());
            let x2 = (x1 + 1).min(sw - 1);
            let here: [f32; 3] = std::array::from_fn(|c| {
                let d = |xx: usize, yy: usize| drift[yy * sw + xx][c];
                (d(x1, y1) * (1.0 - tx) + d(x2, y1) * tx) * (1.0 - ty) + (d(x1, y2) * (1.0 - tx) + d(x2, y2) * tx) * ty
            });
            // Less than half a step of Lab is no drift: the pixel stays exactly as it is
            if !fresh && here.iter().all(|d| d.abs() < 0.5) {
                continue;
            }
            let p = &mut line[x * 3..x * 3 + 3];
            let mut lab = to_lab(rgb(p));
            for c in 0..3 {
                lab[c] -= here[c];
            }
            if fresh {
                let l = lab[0];
                let near = |xx: usize, yy: usize| around[yy * sw + xx][0];
                let local = (near(x1, y1) * (1.0 - tx) + near(x2, y1) * tx) * (1.0 - ty) + (near(x1, y2) * (1.0 - tx) + near(x2, y2) * tx) * ty;
                let mid = 4.0 * (l / 100.0) * (1.0 - l / 100.0);
                lab[0] = (l + CLARITY * (l - local) * mid.clamp(0.0, 1.0)).clamp(0.0, 100.0);
                let gain = 1.0 + VIBRANCE * (1.0 - lab[1].hypot(lab[2]) / VIVID).clamp(0.0, 1.0);
                lab[1] *= gain;
                lab[2] *= gain;
            }
            p.copy_from_slice(&bytes(from_lab(lab)));
        }
    });
}

/// A gaussian blur of `sigma` pixels (separable, edges repeated)
fn blur(values: &mut [[f32; 3]], w: usize, h: usize, sigma: f32) {
    let radius = (sigma * 3.0).ceil().max(1.0) as isize;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = kernel.iter().sum();
    let pass = |values: &[[f32; 3]], horizontal: bool| -> Vec<[f32; 3]> {
        let mut out = vec![[0f32; 3]; values.len()];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0f32; 3];
                for (k, weight) in kernel.iter().enumerate() {
                    let o = k as isize - radius;
                    let (xx, yy) = if horizontal {
                        ((x as isize + o).clamp(0, w as isize - 1) as usize, y)
                    } else {
                        (x, (y as isize + o).clamp(0, h as isize - 1) as usize)
                    };
                    for c in 0..3 {
                        sum[c] += values[yy * w + xx][c] * weight;
                    }
                }
                out[y * w + x] = sum.map(|s| s / total);
            }
        }
        out
    };
    let across = pass(values, true);
    values.copy_from_slice(&pass(&across, false));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_round_trips() {
        for p in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.8, 0.2, 0.1], [0.1, 0.5, 0.9], [0.5, 0.5, 0.5]] {
            let back = from_lab(to_lab(p));
            for c in 0..3 {
                assert!((back[c] - p[c]).abs() < 1e-3, "{p:?} -> {back:?}");
            }
        }
        let [l, a, b] = to_lab([1.0, 1.0, 1.0]);
        assert!((l - 100.0).abs() < 0.01 && a.abs() < 0.01 && b.abs() < 0.01);
    }

    fn photo(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| image::Rgb(f(x, y)))
    }

    /// A colourful, well exposed scene: gradients in every channel over the full range
    fn scene(x: u32, y: u32) -> [u8; 3] {
        [(x * 255 / 199) as u8, (y * 255 / 149) as u8, ((x + y) * 255 / 348) as u8]
    }

    #[test]
    fn grey_and_sepia_are_monochrome_colour_is_not() {
        assert!(monochrome(&photo(200, 150, |x, y| { let v = ((x + y) % 256) as u8; [v, v, v] })));
        assert!(monochrome(&photo(200, 150, |x, y| {
            let v = f32::from(((x * 3 + y) % 200) as u8 + 30) / 255.0;
            bytes([v, v * 0.85, v * 0.65])
        })));
        assert!(!monochrome(&photo(200, 150, scene)));
    }

    #[test]
    fn colouring_keeps_the_lightness() {
        let grey = photo(64, 48, |x, _| { let v = (x * 4) as u8; [v, v, v] });
        let ab: Vec<f32> = (0..2 * COLORIZE * COLORIZE).map(|i| if i < COLORIZE * COLORIZE { 20.0 } else { -10.0 }).collect();
        let coloured = colorized(&grey, &ab);
        for (g, c) in grey.pixels().zip(coloured.pixels()).step_by(97) {
            let (lg, lc) = (to_lab(rgb(&g.0)), to_lab(rgb(&c.0)));
            assert!((lg[0] - lc[0]).abs() < 4.0, "{lg:?} {lc:?}");
        }
        let [_, a, b] = to_lab(rgb(&coloured.get_pixel(40, 20).0));
        assert!(a > 10.0 && b < -4.0, "{a} {b}");
        assert_eq!(grey_input(&grey).len(), 3 * COLORIZE * COLORIZE);
    }

    #[test]
    fn lock_takes_back_a_tint_and_keeps_detail() {
        let reference = photo(100, 80, scene);
        // Enlarged 2x, with a pink cast and a checkerboard of fine detail on top
        let mut out = photo(200, 160, |x, y| {
            let [r, g, b] = scene(x / 2, y / 2);
            let fine = if (x + y) % 2 == 0 { 6 } else { 0 };
            [r.saturating_add(25 + fine), g.saturating_add(fine), b.saturating_add(15 + fine)]
        });
        let detail = |img: &RgbImage| (i32::from(img.get_pixel(100, 80).0[1]) - i32::from(img.get_pixel(101, 80).0[1])).abs();
        let before = detail(&out);
        lock(&mut out, &reference, false);
        let (a, b) = (to_lab(rgb(&out.get_pixel(100, 80).0)), to_lab(rgb(&reference.get_pixel(50, 40).0)));
        assert!((a[1] - b[1]).abs() < 4.0 && (a[2] - b[2]).abs() < 4.0, "{a:?} vs {b:?}");
        assert!(detail(&out) >= before - 2, "fine detail kept");
    }

    #[test]
    fn fresh_livens_dull_colours_and_keeps_hues() {
        // A dull green field, a vivid red flag, a grey wall
        let img = photo(120, 90, |x, _| if x < 40 { [110, 130, 100] } else if x < 80 { [230, 20, 30] } else { [128, 128, 128] });
        let mut fresh = img.clone();
        lock(&mut fresh, &img, true);
        let lab = |im: &RgbImage, x: u32| to_lab(rgb(&im.get_pixel(x, 45).0));
        let (dull, dull2) = (lab(&img, 20), lab(&fresh, 20));
        let chroma = |l: [f32; 3]| l[1].hypot(l[2]);
        let hue = |l: [f32; 3]| l[2].atan2(l[1]);
        assert!(chroma(dull2) > chroma(dull) * 1.05, "{dull:?} -> {dull2:?}");
        assert!((hue(dull2) - hue(dull)).abs() < 0.05, "hue kept");
        let (vivid, vivid2) = (lab(&img, 60), lab(&fresh, 60));
        assert!((chroma(vivid2) - chroma(vivid)).abs() < 3.0, "vivid left alone: {vivid:?} -> {vivid2:?}");
        let (grey, grey2) = (lab(&img, 100), lab(&fresh, 100));
        assert!(chroma(grey2) < 1.5 && (grey2[0] - grey[0]).abs() < 4.0, "{grey:?} -> {grey2:?}");
    }
}
