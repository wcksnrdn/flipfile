// Upscale Photo's colour work, around the upscaler (src/convert/upscale.rs):
// - before: a faded or colour-cast photo gets its colours back (`correct`): each channel's
//   own black and white point (a slide's dyes fade unequally), the near-neutral mid tones
//   pulled to neutral, local contrast (CLAHE) and some saturation for dull colours. Each
//   step is sized by how much the photo needs it, so a well exposed photo is hardly
//   touched. A black-and-white or toned photo is coloured instead (`colorized`, the
//   colours coming from a model).
// - after: the upscaler's colour drift is undone (`lock`), and a fine grain added
//   (`grain`) so smooth areas don't look plastic.
// Everything works a row at a time (results reach 200 MP); colours are compared in CIE
// Lab (D65), lightness 0 to 100.

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

/// The value below which `share` of a histogram's count lies (0 to 1)
fn percentile(histogram: &[u64; 256], share: f64) -> f32 {
    let total: u64 = histogram.iter().sum();
    let wanted = (share * total as f64) as u64;
    let mut seen = 0;
    for (value, &count) in histogram.iter().enumerate() {
        seen += count;
        if seen > wanted {
            return value as f32 / 255.0;
        }
    }
    1.0
}

/// A faded or colour-cast photo's colours put right, `strength` 0 (nothing) to 1. Each
/// step is limited to what the photo shows it needs.
pub(super) fn correct(img: &RgbImage, strength: f32) -> RgbImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return img.clone();
    }

    // Each channel's black and white point, stretched where the channels disagree (dyes
    // that faded unequally) or are compressed (a faded print); a picture that already
    // spans the range in every channel keeps it
    let mut histograms = [[0u64; 256]; 3];
    for p in img.pixels() {
        for c in 0..3 {
            histograms[c][usize::from(p.0[c])] += 1;
        }
    }
    let lo: Vec<f32> = histograms.iter().map(|h| percentile(h, 0.003)).collect();
    let hi: Vec<f32> = histograms.iter().map(|h| percentile(h, 0.997)).collect();
    let spans: Vec<f32> = (0..3).map(|c| hi[c] - lo[c]).collect();
    let (widest, narrowest) = (spans.iter().copied().fold(0.0, f32::max), spans.iter().copied().fold(1.0, f32::min));
    let faded = (((widest - narrowest) / 0.15 + (0.92 - narrowest) / 0.3).clamp(0.0, 1.0)) * strength;
    let mut curves = [[0f32; 256]; 3];
    for c in 0..3 {
        let (l, top) = (lo[c] * faded, 1.0 - (1.0 - hi[c]) * faded);
        for (v, out) in curves[c].iter_mut().enumerate() {
            *out = ((v as f32 / 255.0 - l) / (top - l).max(0.35)).clamp(0.0, 1.0);
        }
    }
    let levelled = |p: &[u8]| [curves[0][usize::from(p[0])], curves[1][usize::from(p[1])], curves[2][usize::from(p[2])]];

    // The near-neutral mid tones (greys, whites in light, concrete) pulled to neutral, by
    // a gamma per channel; a picture without such greys is left as it is
    let (mut sum, mut neutral, mut seen) = ([0f64; 3], 0usize, 0usize);
    let step = ((u64::from(w) * u64::from(h) / 262_144) as f64).sqrt().max(1.0) as usize;
    for y in (0..h as usize).step_by(step) {
        for x in (0..w as usize).step_by(step) {
            let p = levelled(&img.get_pixel(x as u32, y as u32).0);
            let [l, a, b] = to_lab(p);
            seen += 1;
            if a.hypot(b) < 12.0 && (25.0..92.0).contains(&l) {
                neutral += 1;
                for c in 0..3 {
                    sum[c] += f64::from(p[c]);
                }
            }
        }
    }
    if neutral as f64 > 0.02 * seen as f64 {
        let mean: Vec<f32> = sum.iter().map(|s| (*s / neutral as f64) as f32).collect();
        let grey = (mean.iter().sum::<f32>() / 3.0).clamp(0.05, 0.95);
        for c in 0..3 {
            let gamma = grey.ln() / mean[c].clamp(0.05, 0.95).ln();
            let gamma = (1.0 + (gamma - 1.0) * 0.8 * strength).clamp(0.8, 1.25);
            for v in curves[c].iter_mut() {
                *v = v.powf(gamma);
            }
        }
    }
    let curved = |p: &[u8]| [curves[0][usize::from(p[0])], curves[1][usize::from(p[1])], curves[2][usize::from(p[2])]];

    // Lightness, for local contrast; and how dull the colours are, for vibrance
    let mut lightness = vec![0u8; w as usize * h as usize];
    let mut chroma = [0u64; 256];
    for (i, p) in img.pixels().enumerate() {
        let [l, a, b] = to_lab(curved(&p.0));
        lightness[i] = (l * 2.55).clamp(0.0, 255.0) as u8;
        chroma[(a.hypot(b) * 2.0).min(255.0) as usize] += 1;
    }
    let (mut mean, mut square) = (0f64, 0f64);
    for &l in &lightness {
        mean += f64::from(l);
        square += f64::from(l) * f64::from(l);
    }
    let n = lightness.len() as f64;
    let deviation = (square / n - (mean / n).powi(2)).max(0.0).sqrt() as f32;
    let flat = ((22.0 - deviation) / 12.0).clamp(0.0, 1.0) * strength;
    let mix = 0.25 + 0.35 * flat;
    let equalised = clahe(&lightness, w as usize, h as usize, 1.6);
    let median_chroma = percentile(&chroma, 0.5) * 255.0 / 2.0;
    let boost = strength * ((22.0 - median_chroma) / 15.0).clamp(0.0, 1.0) * 0.35;

    let mut out = img.clone();
    let width = w as usize;
    par_rows(&mut out, width, |y, line| {
        for x in 0..width {
            let p = &mut line[x * 3..x * 3 + 3];
            let [l, a, b] = to_lab(curved(p));
            let l = l * (1.0 - mix) + equalised[y * width + x] * mix;
            let gain = 1.0 + boost * (1.0 - a.hypot(b) / 60.0).clamp(0.0, 1.0);
            p.copy_from_slice(&bytes(from_lab([l, a * gain, b * gain])));
        }
    });
    out
}

/// Contrast-limited adaptive histogram equalisation of a lightness plane (0 to 255) on an
/// 8 x 8 grid of tiles, as Lab lightness (0 to 100): each tile's histogram is clipped at
/// `limit` times its average and equalised, and every pixel blends its four nearest
/// tiles' curves
fn clahe(plane: &[u8], w: usize, h: usize, limit: f32) -> Vec<f32> {
    const GRID: usize = 8;
    let (tw, th) = (w.div_ceil(GRID).max(1), h.div_ceil(GRID).max(1));
    let (gx, gy) = (w.div_ceil(tw), h.div_ceil(th));
    let mut curves = vec![[0f32; 256]; gx * gy];
    for ty in 0..gy {
        for tx in 0..gx {
            let mut histogram = [0u32; 256];
            let (x0, y0) = (tx * tw, ty * th);
            let (x1, y1) = ((x0 + tw).min(w), (y0 + th).min(h));
            for y in y0..y1 {
                for &v in &plane[y * w + x0..y * w + x1] {
                    histogram[usize::from(v)] += 1;
                }
            }
            let area = ((x1 - x0) * (y1 - y0)).max(1) as f32;
            let clip = (limit * area / 256.0).max(1.0) as u32;
            let mut excess = 0;
            for count in histogram.iter_mut() {
                if *count > clip {
                    excess += *count - clip;
                    *count = clip;
                }
            }
            let (share, rest) = (excess / 256, excess % 256);
            let mut sum = 0u32;
            for (v, count) in histogram.iter().enumerate() {
                sum += count + share + u32::from((v as u32) < rest);
                curves[ty * gx + tx][v] = sum as f32 / area * 100.0;
            }
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let fy = ((y as f32 + 0.5) / th as f32 - 0.5).clamp(0.0, (gy - 1) as f32);
        let (y1, ty) = (fy as usize, fy.fract());
        let y2 = (y1 + 1).min(gy - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / tw as f32 - 0.5).clamp(0.0, (gx - 1) as f32);
            let (x1, tx) = (fx as usize, fx.fract());
            let x2 = (x1 + 1).min(gx - 1);
            let v = usize::from(plane[y * w + x]);
            let top = curves[y1 * gx + x1][v] * (1.0 - tx) + curves[y1 * gx + x2][v] * tx;
            let bottom = curves[y2 * gx + x1][v] * (1.0 - tx) + curves[y2 * gx + x2][v] * tx;
            out[y * w + x] = top * (1.0 - ty) + bottom * ty;
        }
    }
    out
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

/// Undoes the upscaler's colour drift: what `out` changed in colour and brightness over
/// large areas, compared with `reference` (what the upscaler read), is taken back; its
/// fine detail stays
pub(super) fn lock(out: &mut RgbImage, reference: &RgbImage) {
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
            if here.iter().all(|d| d.abs() < 0.5) {
                continue;
            }
            let p = &mut line[x * 3..x * 3 + 3];
            let mut lab = to_lab(rgb(p));
            for c in 0..3 {
                lab[c] -= here[c];
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

/// A fine film-like grain: luminance noise about a pixel across, strongest in the mid
/// tones and none in deep shadow or highlight; the same for the same picture
pub(super) fn grain(out: &mut RgbImage, amount: f32) {
    // A normally distributed value fixed by the pixel's position
    fn noise(x: u32, y: u32) -> f32 {
        let hash = |mut v: u64| {
            v ^= v >> 33;
            v = v.wrapping_mul(0xff51_afd7_ed55_8ccd);
            v ^= v >> 33;
            v = v.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            v ^ (v >> 33)
        };
        let v = hash((u64::from(x) << 32) | u64::from(y));
        let (u1, u2) = (((v >> 11) as f64 + 1.0) / 9_007_199_254_740_993.0, ((hash(v) >> 11) as f64) / 9_007_199_254_740_992.0);
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
    // Blurred by a 3 x 3 gaussian (sigma 0.7), which leaves 0.424 of the noise's spread
    const WEIGHTS: [f32; 3] = [0.21, 0.58, 0.21];
    let (w, h) = out.dimensions();
    let width = w as usize;
    par_rows(out, width, |y, line| {
        let y = y as u32;
        for x in 0..w {
            let mut n = 0.0;
            for (dy, wy) in WEIGHTS.iter().enumerate() {
                for (dx, wx) in WEIGHTS.iter().enumerate() {
                    let xx = (x + dx as u32).saturating_sub(1).min(w - 1);
                    let yy = (y + dy as u32).saturating_sub(1).min(h - 1);
                    n += wy * wx * noise(xx, yy);
                }
            }
            let p = &mut line[x as usize * 3..x as usize * 3 + 3];
            let lum = (f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2])) / 765.0;
            let shift = amount * n / 0.424 * 4.0 * lum * (1.0 - lum) * 255.0;
            for v in p.iter_mut() {
                *v = (f32::from(*v) + shift).round().clamp(0.0, 255.0) as u8;
            }
        }
    });
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
    fn a_magenta_faded_slide_gets_its_neutrals_back() {
        // A grey wall and a gradient, faded: red lifted and compressed, blue sunk
        let faded = photo(200, 150, |x, y| {
            let v = if x < 100 { 0.5 } else { (x + y) as f32 / 350.0 };
            bytes([0.25 + v * 0.6, 0.1 + v * 0.55, 0.15 + v * 0.45])
        });
        let fixed = correct(&faded, 1.0);
        let wall = |img: &RgbImage| to_lab(rgb(&img.get_pixel(50, 75).0));
        let (before, after) = (wall(&faded), wall(&fixed));
        assert!(after[1].hypot(after[2]) < before[1].hypot(before[2]) * 0.5, "{before:?} -> {after:?}");
    }

    #[test]
    fn a_good_photo_is_hardly_touched() {
        let good = photo(200, 150, scene);
        let fixed = correct(&good, 1.0);
        let moved: f32 = good
            .pixels()
            .zip(fixed.pixels())
            .map(|(a, b)| (0..3).map(|c| (f32::from(a.0[c]) - f32::from(b.0[c])).abs()).sum::<f32>())
            .sum::<f32>()
            / (3.0 * 200.0 * 150.0);
        assert!(moved < 12.0, "moved {moved} levels on average");
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
        lock(&mut out, &reference);
        let (a, b) = (to_lab(rgb(&out.get_pixel(100, 80).0)), to_lab(rgb(&reference.get_pixel(50, 40).0)));
        assert!((a[1] - b[1]).abs() < 4.0 && (a[2] - b[2]).abs() < 4.0, "{a:?} vs {b:?}");
        assert!(detail(&out) >= before - 2, "fine detail kept");
    }

    #[test]
    fn grain_is_fine_and_spares_black_and_white() {
        let mut img = photo(64, 64, |x, _| if x < 16 { [0, 0, 0] } else if x < 32 { [255, 255, 255] } else { [128, 128, 128] });
        let copy = img.clone();
        grain(&mut img, 0.012);
        assert_eq!(img.get_pixel(5, 5), copy.get_pixel(5, 5));
        assert_eq!(img.get_pixel(20, 5), copy.get_pixel(20, 5));
        let changes: Vec<i32> = (32..64).map(|x| i32::from(img.get_pixel(x, 10).0[0]) - 128).collect();
        assert!(changes.iter().any(|&c| c != 0) && changes.iter().all(|c| c.abs() < 16), "{changes:?}");
        let mut again = copy.clone();
        grain(&mut again, 0.012);
        assert_eq!(again, img, "the same every time");
    }
}
