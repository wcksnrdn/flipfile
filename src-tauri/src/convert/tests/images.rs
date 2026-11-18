//! Photos and images: every format, EXIF, stripping, resizing, quality, compression,
//! colour, and the upscale limit

use super::*;

#[test]
fn converts_to_every_format() {
    let dir = temp_dir("formats");
    let src = dir.join("photo.png");
    sample(300, 200).save(&src).unwrap();
    for ext in ["jpg", "jpeg", "png", "webp", "bmp", "tiff", "gif", "ico", "avif", "pdf"] {
        let out = run(src.to_str().unwrap(), ext, None, &Options::default()).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert!(out.ends_with(&format!("photo_converted.{ext}")), "{out}");
        if !matches!(ext, "pdf" | "avif") {
            image::open(&out).unwrap_or_else(|e| panic!("{ext} unreadable: {e}"));
        }
    }
    // Never overwrites
    assert!(run(src.to_str().unwrap(), "png", None, &Options::default()).unwrap().ends_with("photo_converted_2.png"));
}

#[test]
fn ico_is_square_and_at_most_256() {
    let dir = temp_dir("ico");
    let src = dir.join("wide.png");
    sample(1000, 400).save(&src).unwrap();
    let ico = image::open(run(src.to_str().unwrap(), "ico", None, &Options::default()).unwrap()).unwrap();
    assert_eq!((ico.width(), ico.height()), (256, 256));
}

#[test]
fn applies_exif_orientation() {
    let dir = temp_dir("orient");
    let src = dir.join("phone.jpg");
    fs::write(&src, rotated_jpeg(40, 20)).unwrap();
    let out = image::open(run(src.to_str().unwrap(), "png", None, &Options::default()).unwrap()).unwrap();
    assert_eq!((out.width(), out.height()), (20, 40));
}

#[test]
fn clean_strips_exif_losslessly() {
    let dir = temp_dir("clean");
    let src = dir.join("plain.jpg");
    let data = rotated_jpeg(40, 20);
    // Same file but without rotation, so the lossless path is taken
    let stripped_once = strip_jpeg(&data).unwrap();
    assert!(stripped_once.len() < data.len());
    assert!(!stripped_once.windows(6).any(|w| w == b"Exif\0\0"));
    fs::write(&src, &stripped_once).unwrap();
    let out = clean(&src, None).unwrap();
    assert!(out.ends_with("plain_clean.jpg"));
    assert_eq!(fs::read(&out).unwrap(), stripped_once);

    // Rotated photo: re-encoded upright, EXIF gone
    let rotated = dir.join("rotated.jpg");
    fs::write(&rotated, &data).unwrap();
    let out = clean(&rotated, None).unwrap();
    let bytes = fs::read(&out).unwrap();
    assert!(!bytes.windows(6).any(|w| w == b"Exif\0\0"));
    let img = image::open(&out).unwrap();
    assert_eq!((img.width(), img.height()), (20, 40));
}

#[test]
fn png_strip_drops_text_chunks() {
    let dir = temp_dir("png");
    let src = dir.join("a.png");
    sample(10, 10).save(&src).unwrap();
    let mut data = fs::read(&src).unwrap();
    // Insert a tEXt chunk right after IHDR (8 sig + 25 IHDR)
    let text = b"Comment\0secret";
    let mut chunk = (text.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(text);
    chunk.extend_from_slice(&[0, 0, 0, 0]); // CRC is not validated by strip_png
    data.splice(33..33, chunk);
    let out = strip_png(&data).unwrap();
    assert!(!out.windows(6).any(|w| w == b"secret"));
    image::load_from_memory(&out).unwrap();
}

#[test]
fn merges_images_into_one_pdf() {
    let dir = temp_dir("merge");
    let mut paths = vec![];
    for (i, (w, h)) in [(100, 50), (60, 90), (80, 80)].into_iter().enumerate() {
        let p = dir.join(format!("img{i}.png"));
        sample(w, h).save(&p).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }
    paths.push(dir.join("missing.png").to_string_lossy().into_owned());
    let channel = Channel::new(|_| Ok(()));
    let result = merge_to_pdf(&paths, &channel, None, &Options::default()).unwrap();
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.errors.len(), 1);
    assert!(result.outputs[0].ends_with("img0_merged.pdf"));
    let pdf = fs::read(&result.outputs[0]).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 3"));
    assert_eq!(text.matches("/Type /Page ").count(), 3);
}

#[test]
fn out_dir_uses_plain_names() {
    let dir = temp_dir("outdir");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let src = dir.join("Clipboard 2026-10-20 at 14.03.12.png");
    sample(20, 20).save(&src).unwrap();
    let a = run(src.to_str().unwrap(), "jpg", Some(&out), &Options::default()).unwrap();
    let b = run(src.to_str().unwrap(), "jpg", Some(&out), &Options::default()).unwrap();
    assert_eq!(Path::new(&a), out.join("Clipboard 2026-10-20 at 14.03.12.jpg"));
    assert_eq!(Path::new(&b), out.join("Clipboard 2026-10-20 at 14.03.12_2.jpg"));
}

#[test]
fn resizes_without_upscaling() {
    let img = sample(400, 200);
    assert_eq!(resize(img.clone(), Resize::Original).dimensions(), (400, 200));
    assert_eq!(resize(img.clone(), Resize::Scale(0.5)).dimensions(), (200, 100));
    assert_eq!(resize(img.clone(), Resize::MaxEdge(100)).dimensions(), (100, 50));
    assert_eq!(resize(img, Resize::MaxEdge(1920)).dimensions(), (400, 200));
}

#[test]
fn lower_quality_gives_smaller_jpeg() {
    let dir = temp_dir("quality");
    let src = dir.join("noise.png");
    DynamicImage::ImageRgb8(RgbImage::from_fn(300, 300, |x, y| Rgb([((x * 7) ^ (y * 13)) as u8, (x * y) as u8, (x + y) as u8])))
        .save(&src)
        .unwrap();
    let big = run(src.to_str().unwrap(), "jpg", None, &Options { resize: Resize::Original, quality: 95 }).unwrap();
    let small = run(src.to_str().unwrap(), "jpg", None, &Options { resize: Resize::Original, quality: 60 }).unwrap();
    assert!(fs::metadata(&small).unwrap().len() < fs::metadata(&big).unwrap().len());
}

// Uses the PDFium library downloaded by `npm run fetch-pdfium`.
#[test]
fn renders_pdf_pages_to_images() {
    let dir = temp_dir("pdfrender");
    let pages: Vec<Page> = [(200, 100), (100, 200), (150, 150)]
        .into_iter()
        .map(|(w, h)| pdf_page(&sample(w, h), 90).unwrap())
        .collect();
    let doc = dir.join("doc.pdf");
    fs::write(&doc, jpeg_pages_to_pdf(&pages)).unwrap();
    let one = dir.join("single.pdf");
    fs::write(&one, jpeg_pages_to_pdf(&pages[..1])).unwrap();

    // Multi-page: a folder with one image per page, at 200 DPI (page size is 0.75 pt/px)
    let reported = Mutex::new(vec![]);
    let on_page = |done: usize, total: usize| reported.lock().unwrap().push((done, total));
    let c = convert(doc.to_str().unwrap(), "png", None, &Options::default(), "converted", &on_page).unwrap();
    assert_eq!(*reported.lock().unwrap(), vec![(0, 3), (1, 3), (2, 3), (3, 3)]);
    assert_eq!(c.pages, 3);
    assert!(c.path.ends_with("doc_pages"));
    let first = image::open(Path::new(&c.path).join("page-01.png")).unwrap();
    let expected = (200.0 * 0.75 * 200.0 / 72.0_f32).round() as u32;
    assert!((first.width() as i64 - expected as i64).abs() <= 2, "{} vs {expected}", first.width());
    assert!(first.width() > first.height());
    assert!(Path::new(&c.path).join("page-03.png").exists());

    // Single page: a plain file; resize settings apply to rendered pages too
    let small = Options { resize: Resize::MaxEdge(100), quality: 90 };
    let c = convert(one.to_str().unwrap(), "jpg", None, &small, "converted", &|_, _| {}).unwrap();
    assert!(c.path.ends_with("single_converted.jpg"));
    assert_eq!(image::open(&c.path).unwrap().width(), 100);

    // PDF -> PDF is refused with a clear message
    assert!(convert(doc.to_str().unwrap(), "pdf", None, &Options::default(), "converted", &|_, _| {}).is_err());
}

#[test]
fn preset_drop_keeps_the_format() {
    let dir = temp_dir("same");
    let noisy = |w: u32, h: u32| {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| Rgb([((x * 7) ^ (y * 13)) as u8, (x * y) as u8, (x + y) as u8])))
    };
    let photo = dir.join("photo.jpeg");
    noisy(400, 300).write_with_encoder(JpegEncoder::new_with_quality(&mut File::create(&photo).unwrap(), 98)).unwrap();
    let shot = dir.join("shot.png");
    noisy(400, 300).save(&shot).unwrap();
    let compress = Options { resize: Resize::Original, quality: COMPRESS_QUALITY };
    let none = &|_: usize, _: usize| {};

    // Compress: same format and spelling, smaller file, sizes reported
    let c = convert(photo.to_str().unwrap(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(c.path.ends_with("photo_compressed.jpeg"), "{}", c.path);
    let (before, after) = c.sizes.unwrap();
    assert!(after < before, "{after} >= {before}");

    // PNG stays PNG (lossless, max compression)
    let c = convert(shot.to_str().unwrap(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(c.path.ends_with("shot_compressed.png"));
    assert_eq!(image::open(&c.path).unwrap().to_rgb8(), image::open(&shot).unwrap().to_rgb8());

    // Resize preset: same format, smaller dimensions
    let half = Options { resize: Resize::Scale(0.5), quality: 90 };
    let c = convert(shot.to_str().unwrap(), SAME_FORMAT, None, &half, "resized", none).unwrap();
    assert!(c.path.ends_with("shot_resized.png"));
    assert_eq!(image::open(&c.path).unwrap().dimensions(), (200, 150));
}

#[test]
fn bmp_and_tiff_get_as_small_as_their_format_allows() {
    let dir = temp_dir("rawformats");
    let photo = dir.join("photo.png");
    // Opaque, but stored as RGBA (like most screenshots)
    DynamicImage::ImageRgba8(RgbaImage::from_fn(300, 200, |x, y| image::Rgba([(x * 3) as u8, (y * 5) as u8, 90, 255])))
        .save(&photo)
        .unwrap();
    let none = &|_: usize, _: usize| {};
    let plain = Options::default();
    let compress = Options { resize: Resize::Original, quality: COMPRESS_QUALITY };

    // BMP: 24-bit for opaque images (3 bytes per pixel + header), with an honest note on Compress
    let bmp = run(photo.to_str().unwrap(), "bmp", None, &plain).unwrap();
    assert!(fs::metadata(&bmp).unwrap().len() < 300 * 200 * 3 + 1024);
    let c = convert(bmp.as_str(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(c.path.ends_with("_compressed.bmp"));
    assert_eq!(c.note.as_deref(), Some("BMP can't be compressed further"));
    assert_eq!(image::open(&c.path).unwrap().to_rgb8(), image::open(&photo).unwrap().to_rgb8());

    // Transparent images keep their alpha channel in BMP
    let clear = dir.join("clear.png");
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 40, image::Rgba([10, 20, 30, 0]))).save(&clear).unwrap();
    let bmp = run(clear.to_str().unwrap(), "bmp", None, &plain).unwrap();
    assert!(image::open(&bmp).unwrap().color().has_alpha());

    // TIFF: Compress uses lossless Deflate, so it is smaller and pixel-identical
    let tif = run(photo.to_str().unwrap(), "tiff", None, &plain).unwrap();
    let small = convert(tif.as_str(), SAME_FORMAT, None, &compress, "compressed", none).unwrap();
    assert!(small.path.ends_with("_compressed.tiff"));
    let (before, after) = small.sizes.unwrap();
    assert!(after < before / 2, "{after} vs {before}");
    assert_eq!(image::open(&small.path).unwrap().to_rgb8(), image::open(&photo).unwrap().to_rgb8());
}

#[test]
fn webp_is_lossy_and_respects_quality() {
    let dir = temp_dir("webp-lossy");
    let src = dir.join("noisy.png");
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(400, 300, |x, y| {
        Rgb([((x * 17) ^ (y * 31)) as u8, (x * y) as u8, (x * 3 + y * 5) as u8])
    }));
    img.save(&src).unwrap();

    let high = run(src.to_str().unwrap(), "webp", None, &Options { resize: Resize::Original, quality: 92 }).unwrap();
    let low = run(src.to_str().unwrap(), "webp", None, &Options { resize: Resize::Original, quality: 55 }).unwrap();
    let high_len = fs::metadata(&high).unwrap().len();
    let low_len = fs::metadata(&low).unwrap().len();
    let png_len = fs::metadata(&src).unwrap().len();
    assert!(low_len < high_len, "low {low_len} >= high {high_len}");
    assert!(high_len < png_len, "webp {high_len} >= png {png_len}");
}

#[test]
fn ico_multisize_and_icns_are_valid() {
    let dir = temp_dir("icons");
    let src = dir.join("app.png");
    sample(512, 320).save(&src).unwrap();

    // Multi-size ICO has 6 entries (16, 32, 48, 64, 128, 256) in the ICONDIR header
    let ico_path = run(src.to_str().unwrap(), "ico", None, &Options::default()).unwrap();
    let ico_bytes = fs::read(&ico_path).unwrap();
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]);
    assert_eq!(count, 6);
    let decoded = image::open(&ico_path).unwrap();
    assert_eq!(decoded.dimensions(), (256, 256));

    // ICNS starts with "icns" and matches its big-endian length header
    let icns_path = run(src.to_str().unwrap(), "icns", None, &Options::default()).unwrap();
    let icns_bytes = fs::read(&icns_path).unwrap();
    assert_eq!(&icns_bytes[0..4], b"icns");
    let total_len = u32::from_be_bytes([icns_bytes[4], icns_bytes[5], icns_bytes[6], icns_bytes[7]]) as usize;
    assert_eq!(total_len, icns_bytes.len());
}

#[cfg(target_os = "macos")]
#[test]
fn encodes_and_decodes_heic() {
    let dir = temp_dir("heic-out");
    let src = dir.join("photo.png");
    sample(120, 80).save(&src).unwrap();
    let out = run(src.to_str().unwrap(), "heic", None, &Options::default()).unwrap();
    assert!(out.ends_with(".heic"));
    let head = fs::read(&out).unwrap();
    assert!(crate::platform::system_codec::is_heif_family(&head));
    let decoded = crate::platform::system_codec::decode(Path::new(&out)).unwrap();
    assert_eq!(decoded.dimensions(), (120, 80));
}

#[test]
fn photo_tools_transform_square_and_compress_to_target() {
    let dir = temp_dir("photo-tools");
    let src = dir.join("rect.jpg");
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(600, 400, |x, y| {
        Rgb([((x * 13) ^ (y * 29)) as u8, (x + y * 3) as u8, (x ^ y) as u8])
    }));
    img.write_with_encoder(JpegEncoder::new_with_quality(&mut File::create(&src).unwrap(), 96)).unwrap();

    // Rotate 90 CW swaps width and height
    let rot = run(src.to_str().unwrap(), "rot-r", None, &Options::default()).unwrap();
    assert_eq!(image::open(&rot).unwrap().dimensions(), (400, 600));

    // Flip and grayscale preserve dimensions
    let gray = run(src.to_str().unwrap(), "grayscale", None, &Options::default()).unwrap();
    assert_eq!(image::open(&gray).unwrap().dimensions(), (600, 400));

    // Square pads 600x400 to 600x600
    let sq = run(src.to_str().unwrap(), "square", None, &Options::default()).unwrap();
    assert_eq!(image::open(&sq).unwrap().dimensions(), (600, 600));

    // Target-size compression fits strictly under 100 KB
    let c100 = run(src.to_str().unwrap(), "cmp-100k", None, &Options::default()).unwrap();
    let size = fs::metadata(&c100).unwrap().len();
    assert!(size <= 100_000, "expected <= 100 KB, got {size}");

    // Lossless compression works on PNG
    let png = dir.join("graphic.png");
    sample(200, 150).save(&png).unwrap();
    let lossless = run(png.to_str().unwrap(), "cmp-lossless", None, &Options::default()).unwrap();
    assert_eq!(image::open(&lossless).unwrap().to_rgb8(), image::open(&png).unwrap().to_rgb8());
}

#[test]
fn encodes_and_decodes_pro_3d_photo_formats() {
    let dir = temp_dir("photo-pro");
    let src = dir.join("texture.png");
    sample(64, 48).save(&src).unwrap();

    for fmt in ["tga", "qoi", "exr", "hdr", "ppm"] {
        let out = run(src.to_str().unwrap(), fmt, None, &Options::default()).unwrap();
        assert!(out.ends_with(&format!(".{fmt}")), "expected .{fmt}, got {out}");
        let back = run(&out, "png", None, &Options::default()).unwrap();
        assert_eq!(image::open(&back).unwrap().dimensions(), (64, 48));
    }

    let svg = run(src.to_str().unwrap(), "svg", None, &Options::default()).unwrap();
    assert!(svg.ends_with(".svg"));
    let svg_text = fs::read_to_string(&svg).unwrap();
    assert!(svg_text.contains("<svg") && svg_text.contains("data:image/png;base64,"));
}

#[test]
fn photos_and_graphics_are_told_apart() {
    // Sensor-like noise on a gradient: neighbours almost never repeat exactly
    let mut seed = 7u32;
    let photo = DynamicImage::ImageRgb8(RgbImage::from_fn(400, 300, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let n = (seed % 7) as u8;
        Rgb([(x / 2) as u8 + n, (y / 2) as u8 + n, 90 + n])
    }));
    // A window: flat panels, lines of "text"
    let ui = DynamicImage::ImageRgba8(RgbaImage::from_fn(400, 300, |x, y| {
        let text = y % 20 < 8 && x % 37 < 30 && x > 40;
        if text { image::Rgba([30, 30, 30, 255]) } else if x < 40 { image::Rgba([240, 240, 245, 255]) } else { image::Rgba([255, 255, 255, 255]) }
    }));
    assert!(!looks_like_graphic(&photo));
    assert!(looks_like_graphic(&ui));
    assert!(!looks_like_graphic(&DynamicImage::ImageRgb8(RgbImage::new(1, 1))));
}

#[test]
fn gray_images_are_stored_as_one_channel() {
    let dir = temp_dir("gray");
    // RGB pixels that are all exactly gray, like a rendered text page
    let src = dir.join("page.png");
    DynamicImage::ImageRgb8(RgbImage::from_fn(200, 100, |x, y| {
        let v = if (x / 7 + y / 9) % 3 == 0 { 20 } else { 250 };
        Rgb([v, v, v])
    }))
    .save(&src)
    .unwrap();
    let png = run(src.to_str().unwrap(), "png", Some(&dir.join("png")).filter(|d| fs::create_dir_all(d).is_ok()).map(|d| d.as_path()), &Options::default());
    let png = png.unwrap();
    assert_eq!(image::open(&png).unwrap().color(), image::ColorType::L8);
    let jpg = run(src.to_str().unwrap(), "jpg", None, &Options::default()).unwrap();
    let bytes = fs::read(&jpg).unwrap();
    // SOF0: … precision, height, width, number of components
    let sof = bytes.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    assert_eq!(bytes[sof + 9], 1, "a gray JPEG has one component");
    // A colour image keeps its colour
    let color = dir.join("color.png");
    sample(50, 50).save(&color).unwrap();
    let out = run(color.to_str().unwrap(), "png", None, &Options::default()).unwrap();
    assert!(image::open(&out).unwrap().color().has_color());
}

// Display P3 pixels (what Mac screenshots and iPhone JPEGs hold) only mean the right
// colour with their profile, which converted files do not keep: they are converted to sRGB.
#[cfg(target_os = "macos")]
#[test]
fn embedded_colour_profiles_become_srgb() {
    let dir = temp_dir("icc");
    let red = dir.join("red.png");
    RgbImage::from_pixel(16, 16, Rgb([255, 0, 0])).save(&red).unwrap();
    for ext in ["png", "jpeg", "tiff"] {
        let p3 = dir.join(format!("red-p3.{ext}"));
        let sips = std::process::Command::new("sips")
            .args(["-m", "/System/Library/ColorSync/Profiles/Display P3.icc", "-s", "format", ext])
            .arg(&red)
            .arg("--out")
            .arg(&p3)
            .output()
            .unwrap();
        assert!(sips.status.success());
        // the file itself holds P3 values (~234, 51, 35)...
        let raw = image::open(&p3).unwrap().to_rgb8().get_pixel(8, 8).0;
        assert!(raw[0] < 245, "{ext}: {raw:?}");
        // ...that decode as sRGB red
        let (img, _) = open_oriented(&p3).unwrap();
        let px = img.to_rgb8().get_pixel(8, 8).0;
        assert!(px[0] >= 250 && px[1] <= 8 && px[2] <= 8, "{ext}: {px:?}");
    }
    // An sRGB profile changes nothing
    let srgb = dir.join("srgb.png");
    let sips = std::process::Command::new("sips")
        .args(["-m", "/System/Library/ColorSync/Profiles/sRGB Profile.icc"])
        .arg(&red)
        .arg("--out")
        .arg(&srgb)
        .output()
        .unwrap();
    assert!(sips.status.success());
    assert_eq!(open_oriented(&srgb).unwrap().0.to_rgb8().get_pixel(8, 8).0, [255, 0, 0]);
}

#[test]
fn avif_keeps_transparency_and_size() {
    let dir = temp_dir("avif");
    let src = dir.join("logo.png");
    DynamicImage::ImageRgba8(RgbaImage::from_fn(120, 80, |x, _| image::Rgba([200, 40, 40, if x < 60 { 0 } else { 255 }])))
        .save(&src)
        .unwrap();
    let out = run(src.to_str().unwrap(), "avif", None, &Options::default()).unwrap();
    let bytes = fs::read(&out).unwrap();
    assert!(crate::platform::system_codec::is_heif_family(&bytes), "not an AVIF file");
    if cfg!(target_os = "macos") {
        let back = crate::platform::system_codec::decode(Path::new(&out)).unwrap();
        assert_eq!(back.dimensions(), (120, 80));
        let rgba = back.to_rgba8();
        assert!(rgba.get_pixel(10, 40)[3] < 20 && rgba.get_pixel(110, 40)[3] > 235);
    }
}

#[test]
fn target_size_works_through_webp() {
    let dir = temp_dir("target-webp");
    // Transparent, so the budget is met with WEBP
    let src = dir.join("cutout.png");
    // noise, so the PNG itself is far over budget
    let mut seed = 99u32;
    DynamicImage::ImageRgba8(RgbaImage::from_fn(900, 700, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let n = (seed % 40) as u8;
        image::Rgba([((x / 5) as u8).saturating_add(n), ((y / 4) as u8).saturating_add(n), 120 + n, if (x / 50 + y / 50) % 4 == 0 { 0 } else { 255 }])
    }))
    .save(&src)
    .unwrap();
    let c = convert(src.to_str().unwrap(), "cmp-100k", None, &Options::default(), "converted", &|_, _| {}).unwrap();
    assert!(c.path.ends_with(".webp"), "{}", c.path);
    assert!(fs::metadata(&c.path).unwrap().len() <= 100_000);
    assert!(image::open(&c.path).unwrap().dimensions().0 > 0);
}

/// A photo too large to upscale is refused before any AI model runs (so this doesn't
/// need the real models, and passes on every OS); the limit scales with the factor, so a
/// photo too big for 4x can still be fine for 2x. A real-world case this guards: a
/// 20 MP photo (3648 x 5472, a common phone/camera resolution) must pass at 2x.
#[test]
fn oversized_photos_are_refused_before_the_model_runs() {
    let dir = temp_dir("upscale-huge");
    // Just over the 4x limit (currently ~12.5 MP input), comfortably under the 2x one
    let side = (upscale::MAX_PIXELS as f64 / 16.0).sqrt() as u32 + 10;
    let path = dir.join("huge.png");
    DynamicImage::ImageRgb8(RgbImage::from_pixel(side, side, Rgb([10, 20, 30]))).save(&path).unwrap();

    let err = run(path.to_str().unwrap(), "up-4x", None, &Options::default()).unwrap_err();
    assert!(err.contains("too large to upscale"), "{err}");
    assert!(err.contains("4×"), "{err}");

    // A 20 MP photo (3648 x 5472): must fit at 2x under the current limit
    let photo = dir.join("photo20mp.png");
    DynamicImage::ImageRgb8(RgbImage::from_pixel(3648, 5472, Rgb([10, 20, 30]))).save(&photo).unwrap();
    let pixels_2x = 3648u64 * 5472 * 4;
    assert!(pixels_2x <= upscale::MAX_PIXELS, "a 20 MP photo no longer fits at 2x ({pixels_2x} px)");

    let _ = fs::remove_dir_all(&dir);
}
