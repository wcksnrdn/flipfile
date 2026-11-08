// AI upscaling's faces. An upscaler sharpens what is there; a face in an old or small
// photo has too little left to sharpen. So every face is restored by a face model
// (RestoreFormer++, src/platform/upscale.rs) that redraws it from a dictionary of
// high-quality face parts while keeping the person's identity, then blended into the
// upscaled picture.
//
// Each face is first aligned to the template the model was trained on: a 512 x 512 crop
// with the eyes, nose and mouth corners where FFHQ (the face dataset) puts them, found by
// fitting a similarity (scale, rotation, shift) to five landmarks. The restored face goes
// back the same way (bicubic: an upscaler on it would make skin look waxy), and fades in
// over an ellipse covering the face, so hair, ears and background stay the upscaler's.
// Where two faces' ellipses overlap, each pixel belongs to the face whose centre is
// nearest, with a soft hand-over between them.

use image::RgbImage;

use super::upscale::{Models, Work};
use crate::platform::faces::Face;
use crate::platform::upscale::FACE;

/// Where FFHQ's 512 px faces have their eyes, nose tip and mouth corners (as facexlib,
/// which the face models were trained with, aligns them)
const TEMPLATE: [[f32; 2]; 5] = [
    [192.981_38, 239.947_08],
    [318.902_77, 240.193_6],
    [256.634_16, 314.019_35],
    [201.261_17, 371.410_43],
    [313.089_05, 371.151_18],
];

/// The face, in the template: an ellipse from forehead to chin and cheek to cheek, and
/// the outer share of it over which the restored face fades out
const ELLIPSE: ([f32; 2], [f32; 2]) = ([256.0, 285.0], [205.0, 245.0]);
const FADE: f32 = 0.25;

/// Faces with eyes closer than this (pixels) are too small to tell a face model anything
const MIN_EYES: f32 = 8.0;

/// A similarity transform: (x, y) -> (a x - b y + tx, b x + a y + ty)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Similarity {
    a: f32,
    b: f32,
    tx: f32,
    ty: f32,
}

impl Similarity {
    /// The one taking `from` closest to `to` (least squares)
    pub(super) fn fit(from: &[[f32; 2]], to: &[[f32; 2]]) -> Similarity {
        let n = from.len() as f32;
        let mean = |p: &[[f32; 2]]| [p.iter().map(|q| q[0]).sum::<f32>() / n, p.iter().map(|q| q[1]).sum::<f32>() / n];
        let (mf, mt) = (mean(from), mean(to));
        let (mut dot, mut cross, mut norm) = (0.0, 0.0, 0.0);
        for (p, q) in from.iter().zip(to) {
            let (px, py, qx, qy) = (p[0] - mf[0], p[1] - mf[1], q[0] - mt[0], q[1] - mt[1]);
            dot += px * qx + py * qy;
            cross += px * qy - py * qx;
            norm += px * px + py * py;
        }
        let (a, b) = if norm > 0.0 { (dot / norm, cross / norm) } else { (1.0, 0.0) };
        Similarity { a, b, tx: mt[0] - (a * mf[0] - b * mf[1]), ty: mt[1] - (b * mf[0] + a * mf[1]) }
    }

    pub(super) fn apply(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [self.a * x - self.b * y + self.tx, self.b * x + self.a * y + self.ty]
    }

    pub(super) fn inverse(&self) -> Similarity {
        let d = self.a * self.a + self.b * self.b;
        let (a, b) = (self.a / d, -self.b / d);
        Similarity { a, b, tx: -(a * self.tx - b * self.ty), ty: -(b * self.tx + a * self.ty) }
    }

    /// How much it enlarges
    #[cfg(test)]
    fn scale(&self) -> f32 {
        (self.a * self.a + self.b * self.b).sqrt()
    }
}

/// One colour channel at a fractional position, bilinear; beyond the edge the picture
/// is mirrored, so a face near the border gets plausible surroundings
fn sample(pixels: &[u8], w: usize, h: usize, channels: usize, x: f32, y: f32, c: usize) -> f32 {
    let mirror = |v: f32, size: usize| {
        let max = (size - 1) as f32;
        if max <= 0.0 {
            return 0.0;
        }
        let period = 2.0 * max;
        let m = v.rem_euclid(period);
        if m > max { period - m } else { m }
    };
    let (x, y) = (mirror(x, w), mirror(y, h));
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| f32::from(pixels[(y * w + x) * channels + c]);
    let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
    let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// One colour channel at a fractional position, bicubic (Catmull-Rom): sharper than
/// bilinear where a 512 px face is shown larger
fn sample_bicubic(pixels: &[u8], w: usize, h: usize, x: f32, y: f32, c: usize) -> f32 {
    let weight = |t: f32| {
        let t = t.abs();
        if t < 1.0 {
            1.5 * t * t * t - 2.5 * t * t + 1.0
        } else if t < 2.0 {
            -0.5 * t * t * t + 2.5 * t * t - 4.0 * t + 2.0
        } else {
            0.0
        }
    };
    let (x0, y0) = (x.floor(), y.floor());
    let mut value = 0.0;
    for j in -1..=2 {
        let sy = (y0 as i64 + j).clamp(0, h as i64 - 1) as usize;
        let wy = weight(y - (y0 + j as f32));
        for i in -1..=2 {
            let sx = (x0 as i64 + i).clamp(0, w as i64 - 1) as usize;
            value += wy * weight(x - (x0 + i as f32)) * f32::from(pixels[(sy * w + sx) * 3 + c]);
        }
    }
    value
}

/// The face cut out of `image` and aligned to the template: planar RGB, -1 to 1
pub(super) fn aligned(image: &RgbImage, to_face: &Similarity) -> Vec<f32> {
    let from_face = to_face.inverse();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let mut out = vec![0f32; 3 * FACE * FACE];
    for v in 0..FACE {
        for u in 0..FACE {
            let [x, y] = from_face.apply([u as f32, v as f32]);
            for c in 0..3 {
                out[(c * FACE + v) * FACE + u] = sample(image.as_raw(), w, h, 3, x, y, c) / 127.5 - 1.0;
            }
        }
    }
    out
}

/// How much of the restored face is used at a point of the template: 1 inside the face,
/// fading to 0 at the ellipse's edge
pub(super) fn coverage([x, y]: [f32; 2]) -> f32 {
    let ([cx, cy], [rx, ry]) = ELLIPSE;
    let r = (((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2)).sqrt();
    ((1.0 - r) / FADE).clamp(0.0, 1.0)
}

/// A restored face ready to blend in: its pixels (the template's), and the mapping from
/// the result's pixels to the template
struct Layer {
    pixels: RgbImage,
    to_face: Similarity,
    centre: [f32; 2],
}

/// Whether a face is worth restoring: not too small to tell the model anything, and not
/// already more detailed than the model's 512 px face. `soft`: see `restore`.
pub(super) fn worth_restoring(face: &Face, soft: bool) -> bool {
    let template_eyes = TEMPLATE[1][0] - TEMPLATE[0][0];
    let eyes = ((face.points[1][0] - face.points[0][0]).powi(2) + (face.points[1][1] - face.points[0][1]).powi(2)).sqrt();
    let detail = if soft { eyes / 2.0 } else { eyes };
    eyes >= MIN_EYES && detail <= template_eyes * 1.6
}

/// Restores the faces of `original` (`factor` times smaller than `result`) into `result`.
/// `soft` says the picture holds about half the detail its size suggests (see
/// upscale.rs), which lets larger faces still gain from the face model. Returns how many
/// faces were restored.
pub(super) fn restore(
    original: &RgbImage,
    result: &mut RgbImage,
    faces: &[Face],
    factor: u32,
    soft: bool,
    models: &mut dyn Models,
    work: &mut Work,
) -> Result<usize, String> {
    let factor = factor as f32;
    let mut layers = vec![];
    for face in faces.iter().filter(|face| worth_restoring(face, soft)) {
        let to_template = Similarity::fit(&face.points, &TEMPLATE);
        let started = std::time::Instant::now();
        let restored = models.restore_face(&aligned(original, &to_template))?;
        let took = started.elapsed();
        if restored.len() != 3 * FACE * FACE {
            return Err("face restoration: the model gave a face of the wrong size".into());
        }
        let pixels = RgbImage::from_fn(FACE as u32, FACE as u32, |x, y| {
            let at = |c: usize| ((restored[(c * FACE + y as usize) * FACE + x as usize] + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
            image::Rgb([at(0), at(1), at(2)])
        });
        // Result pixel -> original pixel (centres aligned) -> template
        let to_original = Similarity { a: 1.0 / factor, b: 0.0, tx: 0.5 / factor - 0.5, ty: 0.5 / factor - 0.5 };
        let to_face = compose(&to_template, &to_original);
        let centre = to_face.inverse().apply(ELLIPSE.0);
        layers.push(Layer { pixels, to_face, centre });
        work.face_done(took);
    }
    if !layers.is_empty() {
        blend(result, &layers);
    }
    Ok(layers.len())
}

/// `outer` after `inner`
fn compose(outer: &Similarity, inner: &Similarity) -> Similarity {
    let [tx, ty] = outer.apply([inner.tx, inner.ty]);
    Similarity { a: outer.a * inner.a - outer.b * inner.b, b: outer.a * inner.b + outer.b * inner.a, tx, ty }
}

fn blend(result: &mut RgbImage, layers: &[Layer]) {
    let (w, h) = (result.width() as usize, result.height() as usize);
    let reach = 0.04 * w.max(h) as f32;
    // Only the pixels some face's template square lands on
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for layer in layers {
        let back = layer.to_face.inverse();
        for corner in [[0.0, 0.0], [FACE as f32, 0.0], [0.0, FACE as f32], [FACE as f32, FACE as f32]] {
            let [x, y] = back.apply(corner);
            x0 = x0.min(x.floor().max(0.0) as usize);
            y0 = y0.min(y.floor().max(0.0) as usize);
            x1 = x1.max((x.ceil().max(0.0) as usize + 1).min(w));
            y1 = y1.max((y.ceil().max(0.0) as usize + 1).min(h));
        }
    }
    let raw: &mut [u8] = result;
    let mut weights = vec![(0f32, 0f32, [0f32; 3]); layers.len()];
    for y in y0..y1 {
        for x in x0..x1 {
            let mut nearest = f32::MAX;
            let mut cover = 0f32;
            for (slot, layer) in weights.iter_mut().zip(layers) {
                let face = layer.to_face.apply([x as f32, y as f32]);
                let m = coverage(face);
                *slot = (m, 0.0, [0.0; 3]);
                if m <= 0.0 {
                    continue;
                }
                let distance = ((x as f32 - layer.centre[0]).powi(2) + (y as f32 - layer.centre[1]).powi(2)).sqrt();
                nearest = nearest.min(distance);
                cover = cover.max(m);
                let colour = [0, 1, 2].map(|c| sample_bicubic(layer.pixels.as_raw(), FACE, FACE, face[0], face[1], c));
                *slot = (m, distance, colour);
            }
            if cover <= 0.0 {
                continue;
            }
            let (mut total, mut mixed) = (0f32, [0f32; 3]);
            for &(m, distance, colour) in &weights {
                if m <= 0.0 {
                    continue;
                }
                let weight = m * (-(distance - nearest) / reach).exp();
                total += weight;
                for c in 0..3 {
                    mixed[c] += weight * colour[c];
                }
            }
            let at = (y * w + x) * 3;
            for c in 0..3 {
                let face = mixed[c] / total;
                raw[at + c] = (f32::from(raw[at + c]) * (1.0 - cover) + face * cover).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarities_are_fitted_and_inverted() {
        // Scale 2, a quarter turn, a shift
        let known = Similarity { a: 0.0, b: 2.0, tx: 10.0, ty: -4.0 };
        let from = [[1.0, 2.0], [5.0, 1.0], [3.0, 4.0], [0.0, 7.0], [6.0, 6.0]];
        let to: Vec<[f32; 2]> = from.iter().map(|&p| known.apply(p)).collect();
        let fitted = Similarity::fit(&from, &to);
        for (p, q) in from.iter().zip(&to) {
            let got = fitted.apply(*p);
            assert!((got[0] - q[0]).abs() < 1e-4 && (got[1] - q[1]).abs() < 1e-4, "{got:?} vs {q:?}");
        }
        assert!((fitted.scale() - 2.0).abs() < 1e-5);
        let back = fitted.inverse().apply(fitted.apply([3.5, -2.0]));
        assert!((back[0] - 3.5).abs() < 1e-4 && (back[1] + 2.0).abs() < 1e-4);
        let both = compose(&fitted, &known.inverse()).apply([7.0, 3.0]);
        assert!((both[0] - 7.0).abs() < 1e-3 && (both[1] - 3.0).abs() < 1e-3, "{both:?}");
    }

    #[test]
    fn the_face_ellipse_fades_out() {
        assert_eq!(coverage(ELLIPSE.0), 1.0);
        assert_eq!(coverage([0.0, 0.0]), 0.0, "the template's corner is background");
        let edge = coverage([ELLIPSE.0[0] + ELLIPSE.1[0] * 0.9, ELLIPSE.0[1]]);
        assert!(edge > 0.0 && edge < 1.0, "{edge}");
    }

    #[test]
    fn alignment_reads_the_face_where_the_landmarks_are() {
        // A picture whose red channel is x and green y: the aligned face's corner shows
        // where in the picture the template's corner was taken from
        let picture = RgbImage::from_fn(256, 256, |x, y| image::Rgb([x as u8, y as u8, 0]));
        let to_template = Similarity { a: 4.0, b: 0.0, tx: -100.0, ty: -60.0 };
        let face = aligned(&picture, &to_template);
        let red = |u: usize, v: usize| (face[v * FACE + u] + 1.0) * 127.5;
        let green = |u: usize, v: usize| (face[(FACE + v) * FACE + u] + 1.0) * 127.5;
        // Template (0, 0) comes from picture (25, 15); template (400, 200) from (125, 65)
        assert!((red(0, 0) - 25.0).abs() < 0.6 && (green(0, 0) - 15.0).abs() < 0.6);
        assert!((red(400, 200) - 125.0).abs() < 0.6 && (green(400, 200) - 65.0).abs() < 0.6);
    }
}
