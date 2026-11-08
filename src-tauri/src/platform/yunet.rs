// Faces for AI upscaling on Windows and Linux (macOS uses Vision; src/platform/faces.rs):
// YuNet (MIT, Shiqi Yu; OpenCV Zoo's face_detection_yunet), a small face detector that
// gives the same five landmarks the face model is aligned by. It runs on ONNX Runtime
// (src/platform/onnx.rs), on the CPU: a few hundredths of a second.
//
// YuNet finds faces about 10 to 300 px across, so a picture is searched at two sizes, at
// most 1280 px (small faces) and at most 640 px (large ones), and faces found at both
// are merged.

use image::{imageops::FilterType, RgbImage};

use super::faces::Face;
use super::onnx::{Model, Output};

static YUNET: Model = Model::new("yunet.onnx", false);

/// The sizes a picture is searched at (its longest side)
const SIZES: [u32; 2] = [1280, 640];
/// Confidence needed: restoring something that isn't a face would paint one on it
const SCORE: f32 = 0.8;
/// Finds overlapping more than this are the same face
const SAME: f32 = 0.3;
/// The grids YuNet predicts on, in pixels; its outputs come per stride
const STRIDES: [usize; 3] = [8, 16, 32];
const OUTPUTS: [&str; 12] =
    ["cls_8", "cls_16", "cls_32", "obj_8", "obj_16", "obj_32", "bbox_8", "bbox_16", "bbox_32", "kps_8", "kps_16", "kps_32"];

/// One face found: how sure YuNet is, its box (left, top, width, height) and landmarks
#[derive(Clone, Debug, PartialEq)]
struct Detection {
    score: f32,
    rect: [f32; 4],
    points: [[f32; 2]; 5],
}

/// Lets go of the model (see onnx.rs)
#[cfg_attr(test, allow(dead_code))]
pub fn release() {
    YUNET.release();
}

pub fn find_faces(image: &RgbImage) -> Result<Vec<Face>, String> {
    let (w, h) = image.dimensions();
    let longest = w.max(h).max(1);
    let mut found = vec![];
    let mut searched = vec![];
    for size in SIZES {
        let scale = (size as f32 / longest as f32).min(1.0);
        let (sw, sh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
        if searched.contains(&(sw, sh)) {
            continue;
        }
        searched.push((sw, sh));
        let resized;
        let picture = if (sw, sh) == (w, h) {
            image
        } else {
            resized = image::imageops::resize(image, sw, sh, FilterType::Triangle);
            &resized
        };
        let (fx, fy) = (w as f32 / sw as f32, h as f32 / sh as f32);
        found.extend(detect(picture)?.into_iter().map(|mut d| {
            d.rect = [d.rect[0] * fx, d.rect[1] * fy, d.rect[2] * fx, d.rect[3] * fy];
            d.points = d.points.map(|[x, y]| [x * fx, y * fy]);
            d
        }));
    }
    Ok(merge(found).into_iter().map(|d| face(d.points)).collect())
}

/// The landmarks in Vision's order: eyes, nose tip, mouth corners; left before right
fn face([right_eye, left_eye, nose, right_mouth, left_mouth]: [[f32; 2]; 5]) -> Face {
    let pair = |a: [f32; 2], b: [f32; 2]| if a[0] <= b[0] { [a, b] } else { [b, a] };
    let ([e0, e1], [m0, m1]) = (pair(right_eye, left_eye), pair(right_mouth, left_mouth));
    Face { points: [e0, e1, nose, m0, m1] }
}

/// YuNet on one picture: it reads BGR values 0 to 255, padded to a multiple of 32
fn detect(image: &RgbImage) -> Result<Vec<Detection>, String> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let (pw, ph) = (w.div_ceil(32) * 32, h.div_ceil(32) * 32);
    let mut input = vec![0f32; 3 * pw * ph];
    for (x, y, pixel) in image.enumerate_pixels() {
        let (x, y) = (x as usize, y as usize);
        for (c, value) in [pixel[2], pixel[1], pixel[0]].into_iter().enumerate() {
            input[(c * ph + y) * pw + x] = f32::from(value);
        }
    }
    let outputs = YUNET.run("input", &[1, 3, ph, pw], &input, &OUTPUTS)?;
    decode(&outputs, pw, ph)
}

/// The faces in YuNet's outputs (in the order of OUTPUTS) for a pw x ph input: per grid
/// cell, a score (geometric mean of class and objectness), a box as an offset of the
/// cell and a log size, and landmarks as offsets of the cell, all in strides
fn decode(outputs: &[Output], pw: usize, ph: usize) -> Result<Vec<Detection>, String> {
    let mut found = vec![];
    for (i, stride) in STRIDES.into_iter().enumerate() {
        let (cols, rows) = (pw / stride, ph / stride);
        let [cls, obj, rect, marks] = [i, i + 3, i + 6, i + 9].map(|o| &outputs[o].1);
        if cls.len() != cols * rows || obj.len() != cls.len() || rect.len() != 4 * cls.len() || marks.len() != 10 * cls.len() {
            return Err("face detection: unexpected output".into());
        }
        let s = stride as f32;
        for cell in 0..cols * rows {
            let score = (cls[cell].clamp(0.0, 1.0) * obj[cell].clamp(0.0, 1.0)).sqrt();
            if score < SCORE {
                continue;
            }
            let (col, row) = ((cell % cols) as f32, (cell / cols) as f32);
            let r = &rect[cell * 4..][..4];
            let (cx, cy) = ((col + r[0]) * s, (row + r[1]) * s);
            let (bw, bh) = (r[2].exp() * s, r[3].exp() * s);
            let k = &marks[cell * 10..][..10];
            let points = [0, 1, 2, 3, 4].map(|n| [(k[2 * n] + col) * s, (k[2 * n + 1] + row) * s]);
            found.push(Detection { score, rect: [cx - bw / 2.0, cy - bh / 2.0, bw, bh], points });
        }
    }
    Ok(found)
}

/// One find per face: the most confident of those overlapping it
fn merge(mut found: Vec<Detection>) -> Vec<Detection> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = vec![];
    for d in found {
        if kept.iter().all(|k| overlap(&k.rect, &d.rect) <= SAME) {
            kept.push(d);
        }
    }
    kept
}

/// Intersection over union of two boxes
fn overlap(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let w = ((a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0])).max(0.0);
    let h = ((a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1])).max(0.0);
    let both = w * h;
    let union = a[2] * a[3] + b[2] * b[3] - both;
    if union > 0.0 { both / union } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// YuNet's outputs for a 64 x 32 input with one face in the stride-8 cell at column
    /// 3, row 2, and noise below the score everywhere else
    fn outputs() -> Vec<Output> {
        let (pw, ph) = (64, 32);
        let mut out = vec![];
        for kind in 0..4 {
            for stride in STRIDES {
                let cells = (pw / stride) * (ph / stride);
                let per = [1, 1, 4, 10][kind];
                let mut values = vec![if kind < 2 { 0.3 } else { 0.0 }; cells * per];
                if stride == 8 {
                    let cell = 2 * (pw / 8) + 3;
                    match kind {
                        0 => values[cell] = 0.9,
                        1 => values[cell] = 1.0,
                        2 => values[cell * 4..][..4].copy_from_slice(&[0.5, 0.5, 2f32.ln(), 3f32.ln()]),
                        _ => values[cell * 10..][..10].copy_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 0.0, 1.0, 1.0, 1.0]),
                    }
                }
                out.push((vec![1, cells, per], values));
            }
        }
        out
    }

    #[test]
    fn decodes_a_face() {
        let found = decode(&outputs(), 64, 32).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        let d = &found[0];
        assert!((d.score - 0.9f32.sqrt()).abs() < 1e-6);
        // Centre (3.5, 2.5) cells = (28, 20) px; 2 x 3 cells = 16 x 24 px
        assert!(d.rect.iter().zip([20.0, 8.0, 16.0, 24.0]).all(|(a, b)| (a - b).abs() < 1e-4), "{:?}", d.rect);
        assert_eq!(d.points, [[24.0, 16.0], [32.0, 16.0], [28.0, 20.0], [24.0, 24.0], [32.0, 24.0]]);
    }

    #[test]
    fn wrong_outputs_are_an_error() {
        let mut bad = outputs();
        bad[0].1.pop();
        assert!(decode(&bad, 64, 32).is_err());
    }

    #[test]
    fn overlapping_finds_become_one() {
        let at = |x: f32, score: f32| Detection { score, rect: [x, 0.0, 10.0, 10.0], points: [[x, 0.0]; 5] };
        let kept = merge(vec![at(0.0, 0.85), at(1.0, 0.95), at(30.0, 0.9)]);
        assert_eq!(kept.iter().map(|d| d.score).collect::<Vec<_>>(), [0.95, 0.9]);
        assert!((overlap(&at(0.0, 1.0).rect, &at(5.0, 1.0).rect) - 50.0 / 150.0).abs() < 1e-6);
    }

    #[test]
    fn landmarks_come_left_first() {
        let f = face([[30.0, 10.0], [10.0, 10.0], [20.0, 20.0], [28.0, 30.0], [12.0, 30.0]]);
        assert_eq!(f.points, [[10.0, 10.0], [30.0, 10.0], [20.0, 20.0], [12.0, 30.0], [28.0, 30.0]]);
    }
}
