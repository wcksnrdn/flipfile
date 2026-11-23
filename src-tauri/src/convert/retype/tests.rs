use super::*;

#[test]
fn punctuation_is_split_from_words() {
    assert_eq!(split_affix("(alpha"), ("(", "alpha", ""));
    assert_eq!(split_affix("dimulai."), ("", "dimulai", "."));
    assert_eq!(split_affix("\"kata\","), ("\"", "kata", "\","));
    assert_eq!(split_affix("—"), ("—", "", ""));
    assert_eq!(split_affix("akademik@kampus.ac.id"), ("", "akademik@kampus.ac.id", ""));
}

#[cfg(target_os = "macos")]
#[test]
fn only_clear_words_are_trusted() {
    for word in ["Kepada", "mahasiswa,", "08/12/2026", "13:00", "R.301", "II", "akademik@kampus.ac.id", "www.kampus.ac.id/ujian", "print-an", "(021)", "Rp35.000"] {
        assert!(trusted(word), "{word}");
    }
    for word in ["paliog", "nitai", "Calatan", "Desembcr", "Kalkulusl", "tcrlambat"] {
        assert!(!trusted(word), "{word}");
    }
}

#[test]
fn words_are_found_between_spaces() {
    // Three words of ink, 10 rows tall, with gaps of 1 (inside a word) and 6 (between)
    let w = 40;
    let mut ink = vec![0f32; w * 10];
    for x in [2, 3, 5, 6, 14, 15, 16, 25, 26, 28] {
        for y in 2..8 {
            ink[y * w + x] = 1.0;
        }
    }
    assert_eq!(word_runs(&ink, w, 0, 10), [(2, 7), (14, 17), (25, 29)]);
}

#[test]
fn a_lines_ink_ignores_its_neighbours() {
    // A line in the middle, the foot of the line above at the very top
    let (w, h) = (30, 30);
    let mut ink = vec![0f32; w * h];
    for x in 5..25 {
        for y in 12..20 {
            ink[y * w + x] = 1.0;
        }
        ink[w + x] = 1.0;
    }
    assert_eq!(ink_box(&ink, w, h), Some([12, 20, 5, 25]));
}

#[test]
fn holes_are_filled_from_around() {
    // A pale blue panel with dark text in it: the text painted out, the panel stays
    let img = RgbImage::from_fn(40, 20, |x, y| if (15..25).contains(&x) && (5..15).contains(&y) { image::Rgb([20, 20, 20]) } else { image::Rgb([200, 220, 240]) });
    let hole: Vec<bool> = (0..40 * 20).map(|i| (15..25).contains(&(i % 40)) && (5..15).contains(&(i / 40))).collect();
    let filled = inpaint(&img, [0, 0, 40, 20], &hole);
    let p = filled[10 * 40 + 20];
    assert!((p[0] - 200.0 / 255.0).abs() < 0.02 && (p[2] - 240.0 / 255.0).abs() < 0.02, "{p:?}");
}

/// A notice as a phone forwards it: typed, shrunk to a third, saved as a poor JPEG
#[cfg(target_os = "macos")]
fn forwarded_notice() -> DynamicImage {
    let faces = faces();
    let sans = faces.iter().find(|f| f.name == "Liberation Sans" && !f.bold).expect("bundled fonts");
    let serif = faces.iter().find(|f| f.name == "Liberation Serif" && !f.bold).expect("bundled fonts");
    let mut page = RgbImage::from_pixel(1500, 600, image::Rgb([250, 248, 242]));
    let lines: [(&Face, &str, f32, i32); 4] = [
        (serif, "Kepada seluruh mahasiswa, ujian akhir semester ganjil", 40.0, 90),
        (serif, "akan dilaksanakan pada tanggal 8 sampai 19 Desember.", 40.0, 160),
        (sans, "Mata Kuliah   Tanggal   Ruang   Jam", 34.0, 290),
        (sans, "Kalkulus   08/12/2026   R.301   08:00", 34.0, 360),
    ];
    for (face, text, px, baseline) in lines {
        let drawn = face.draw(text, px).unwrap();
        for y in 0..drawn.h {
            for x in 0..drawn.w {
                let (px_, py) = (60 + drawn.left + x as i32, baseline + drawn.top + y as i32);
                let a = drawn.coverage[y * drawn.w + x];
                let p = page.get_pixel_mut(px_ as u32, py as u32);
                for v in &mut p.0 {
                    *v = (f32::from(*v) * (1.0 - a) + 25.0 * a) as u8;
                }
            }
        }
    }
    let small = resize_exact(&DynamicImage::ImageRgb8(page), 500, 200);
    let mut jpeg = vec![];
    small.to_rgb8().write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
    let mut jpeg_low = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_low, 55).encode_image(&small.to_rgb8()).unwrap();
    image::load_from_memory(&jpeg_low).unwrap()
}

/// The whole of Upscale Text on this Mac (its models, Vision, its dictionaries): the
/// words come back typed again and every one is read right
#[cfg(target_os = "macos")]
#[test]
fn a_forwarded_notice_reads_right_again() {
    if crate::platform::upscale::model_path("dejpeg.mlmodelc").is_none() {
        eprintln!("skipped: the models are not fetched here");
        return;
    }
    let notice = forwarded_notice();
    let (up, typed) = upscale_text(&notice, 4, &mut upscale::Bundled, &|_, _| {}).unwrap();
    assert_eq!((up.width(), up.height()), (2000, 800));
    assert!(typed.words >= 20, "{typed:?}");
    let read: String = ocr::recognize(&up.to_rgba8()).unwrap().iter().map(|l| l.text.clone() + " ").collect();
    for word in ["Kepada", "seluruh", "mahasiswa", "dilaksanakan", "Desember", "Kalkulus", "08/12/2026", "R.301", "08:00"] {
        assert!(read.contains(word), "{word} not in {read:?}");
    }
}
