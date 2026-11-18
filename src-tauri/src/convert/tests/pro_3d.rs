//! The pro and 3D photo formats (TGA, EXR, HDR, QOI, PPM, SVG) end to end

use super::*;

#[test]
fn test_pro_3d_all_conversions_roundtrip() {
    let dir = temp_dir("pro-3d-roundtrip");
    let src = dir.join("base_texture.png");
    sample(64, 48).save(&src).unwrap();

    let formats = ["tga", "qoi", "exr", "hdr", "ppm", "svg", "jpeg"];
    for fmt in formats {
        let out = run(src.to_str().unwrap(), fmt, None, &Options::default())
            .unwrap_or_else(|e| panic!("conversion to {fmt} failed: {e}"));
        let p = Path::new(&out);
        assert!(p.exists(), "output for {fmt} does not exist");
        assert!(fs::metadata(p).unwrap().len() > 0, "output for {fmt} is empty");

        if fmt == "svg" {
            let content = fs::read_to_string(p).unwrap();
            assert!(content.contains("<svg"), "svg should contain <svg tag");
            assert!(content.contains("data:image/png;base64,"), "svg should contain embedded base64 png");
        } else {
            let back = run(&out, "png", None, &Options::default())
                .unwrap_or_else(|e| panic!("reading {fmt} back to png failed: {e}"));
            let decoded = image::open(&back)
                .unwrap_or_else(|e| panic!("opening decoded {fmt} failed: {e}"));
            assert_eq!(decoded.dimensions(), (64, 48), "dimensions mismatch for {fmt}");
        }
    }
}

#[test]
fn test_pro_3d_edge_case_dimensions() {
    let dir = temp_dir("pro-3d-dims");
    let test_cases = [(1u32, 1u32), (13u32, 37u32), (256u32, 2u32)];

    for (w, h) in test_cases {
        let src = dir.join(format!("dim_{w}x{h}.png"));
        sample(w, h).save(&src).unwrap();

        for fmt in ["tga", "qoi", "exr", "hdr", "ppm", "svg"] {
            let out = run(src.to_str().unwrap(), fmt, None, &Options::default())
                .unwrap_or_else(|e| panic!("dim {w}x{h} to {fmt} failed: {e}"));
            let p = Path::new(&out);
            assert!(p.exists());
            assert!(fs::metadata(p).unwrap().len() > 0);

            if fmt != "svg" {
                let back = run(&out, "png", None, &Options::default())
                    .unwrap_or_else(|e| panic!("reading {w}x{h} {fmt} failed: {e}"));
                let decoded = image::open(&back).unwrap();
                assert_eq!(decoded.dimensions(), (w, h), "mismatch for {fmt} on {w}x{h}");
            }
        }
    }
}

#[test]
fn test_pro_3d_tools_transform() {
    let dir = temp_dir("pro-3d-tools");
    let src_png = dir.join("sample.png");
    sample(64, 48).save(&src_png).unwrap();

    let pro_fmts = ["tga", "qoi", "exr", "hdr", "ppm"];
    for fmt in pro_fmts {
        let orig = run(src_png.to_str().unwrap(), fmt, None, &Options::default()).unwrap();

        // 1. rot-r: 64x48 -> 48x64
        let rot_r = run(&orig, "rot-r", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} rot-r failed: {e}"));
        let decoded = image::open(&rot_r).unwrap();
        assert_eq!(decoded.dimensions(), (48, 64), "{fmt} rot-r dimensions mismatch");

        // 2. rot-180: 64x48 -> 64x48
        let rot_180 = run(&orig, "rot-180", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} rot-180 failed: {e}"));
        assert_eq!(image::open(&rot_180).unwrap().dimensions(), (64, 48));

        // 3. flip-h: 64x48 -> 64x48
        let flip_h = run(&orig, "flip-h", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} flip-h failed: {e}"));
        assert_eq!(image::open(&flip_h).unwrap().dimensions(), (64, 48));

        // 4. flip-v: 64x48 -> 64x48
        let flip_v = run(&orig, "flip-v", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} flip-v failed: {e}"));
        assert_eq!(image::open(&flip_v).unwrap().dimensions(), (64, 48));

        // 5. grayscale: success
        let gray = run(&orig, "grayscale", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} grayscale failed: {e}"));
        assert_eq!(image::open(&gray).unwrap().dimensions(), (64, 48));

        // 6. square: 64x48 -> 64x64
        let sq = run(&orig, "square", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} square failed: {e}"));
        assert_eq!(image::open(&sq).unwrap().dimensions(), (64, 64));

        // 7. cmp-auto: success
        let cmp = run(&orig, "cmp-auto", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} cmp-auto failed: {e}"));
        assert!(fs::metadata(&cmp).unwrap().len() > 0);

        // 8. cmp-500k: target budget
        let cmp_tgt = run(&orig, "cmp-500k", None, &Options::default())
            .unwrap_or_else(|e| panic!("{fmt} cmp-500k failed: {e}"));
        assert!(fs::metadata(&cmp_tgt).unwrap().len() <= 500_000);
    }
}

#[test]
fn test_pro_3d_cross_conversion_chain() {
    let dir = temp_dir("pro-3d-chain");
    let src = dir.join("start.png");
    sample(40, 30).save(&src).unwrap();

    // Chain: PNG -> TGA -> EXR -> HDR -> QOI -> PPM -> TGA -> PNG
    let tga1 = run(src.to_str().unwrap(), "tga", None, &Options::default()).unwrap();
    let exr = run(&tga1, "exr", None, &Options::default()).unwrap();
    let hdr = run(&exr, "hdr", None, &Options::default()).unwrap();
    let qoi = run(&hdr, "qoi", None, &Options::default()).unwrap();
    let ppm = run(&qoi, "ppm", None, &Options::default()).unwrap();
    let tga2 = run(&ppm, "tga", None, &Options::default()).unwrap();
    let final_png = run(&tga2, "png", None, &Options::default()).unwrap();

    let decoded = image::open(&final_png).unwrap();
    assert_eq!(decoded.dimensions(), (40, 30));
}

#[test]
fn test_pro_3d_slides_and_pdf_interop() {
    let dir = temp_dir("pro-3d-interop");
    let src = dir.join("texture.tga");
    sample(60, 40).save(&src).unwrap();

    // 1. TGA -> PPTX (photo becomes a slide)
    let pptx = run(src.to_str().unwrap(), "pptx", None, &Options::default())
        .unwrap_or_else(|e| panic!("TGA -> PPTX failed: {e}"));
    assert!(pptx.ends_with(".pptx"));
    assert!(fs::metadata(&pptx).unwrap().len() > 0);

    // 2. QOI -> PDF
    let qoi = run(src.to_str().unwrap(), "qoi", None, &Options::default()).unwrap();
    let pdf = run(&qoi, "pdf", None, &Options::default())
        .unwrap_or_else(|e| panic!("QOI -> PDF failed: {e}"));
    assert!(pdf.ends_with(".pdf"));
    assert!(fs::metadata(&pdf).unwrap().len() > 0);

    // 3. PDF page -> TGA, EXR, HDR, QOI, PPM
    for fmt in ["tga", "qoi", "exr", "hdr", "ppm"] {
        let out = run(&pdf, fmt, None, &Options::default())
            .unwrap_or_else(|e| panic!("PDF -> {fmt} failed: {e}"));
        assert!(Path::new(&out).exists());
    }
}

#[test]
fn test_pro_3d_corrupt_and_empty_inputs_fail_gracefully() {
    let dir = temp_dir("pro-3d-corrupt");

    let pro_exts = ["tga", "qoi", "exr", "hdr", "ppm"];
    for ext in pro_exts {
        // 0-byte file
        let empty = dir.join(format!("empty.{ext}"));
        fs::write(&empty, b"").unwrap();
        let res = run(empty.to_str().unwrap(), "png", None, &Options::default());
        assert!(res.is_err(), "0-byte .{ext} should return Err, got Ok");

        // Random garbage noise
        let garbage = dir.join(format!("garbage.{ext}"));
        fs::write(&garbage, b"INVALID_CORRUPTED_BYTES_FOR_TEXTURE_1234567890").unwrap();
        let res = run(garbage.to_str().unwrap(), "png", None, &Options::default());
        assert!(res.is_err(), "corrupt .{ext} should return Err, got Ok");
    }
}

#[test]
fn test_pro_3d_hdr_values_above_one() {
    let dir = temp_dir("pro-3d-hdr-high");
    let exr_path = dir.join("overexposed.exr");

    // Create an image with float values > 1.0 (e.g. 5.0, 10.0, 2.5)
    let mut fimg = image::Rgba32FImage::new(16, 16);
    for p in fimg.pixels_mut() {
        *p = image::Rgba([5.0, 10.0, 2.5, 1.0]);
    }
    DynamicImage::ImageRgba32F(fimg)
        .save_with_format(&exr_path, image::ImageFormat::OpenExr)
        .unwrap();

    // Convert overexposed EXR to PNG
    let png = run(exr_path.to_str().unwrap(), "png", None, &Options::default()).unwrap();
    let img = image::open(&png).unwrap();
    assert_eq!(img.dimensions(), (16, 16));

    // Convert overexposed EXR to HDR
    let hdr = run(exr_path.to_str().unwrap(), "hdr", None, &Options::default()).unwrap();
    assert!(Path::new(&hdr).exists());

    // Convert HDR back to JPG
    let jpg = run(&hdr, "jpg", None, &Options::default()).unwrap();
    assert!(Path::new(&jpg).exists());
}

#[test]
fn test_pro_3d_batch_conversion() {
    let dir = temp_dir("pro-3d-batch");
    let mut inputs = vec![];
    for i in 0..4 {
        let p = dir.join(format!("batch_{i}.png"));
        sample(32 + i * 8, 32).save(&p).unwrap();
        inputs.push(p.to_string_lossy().into_owned());
    }

    let channel = Channel::new(|_| Ok(()));
    for fmt in ["tga", "qoi", "exr", "hdr", "ppm", "svg"] {
        let res = run_batch(&inputs, fmt, None, &Options::default(), "converted", &channel)
            .unwrap_or_else(|e| panic!("batch conversion to {fmt} failed: {e}"));
        assert_eq!(res.outputs.len(), 4, "expected 4 outputs for {fmt}");
        for out in &res.outputs {
            assert!(Path::new(out).exists(), "batch output {out} missing");
            assert!(fs::metadata(out).unwrap().len() > 0);
        }
    }
}
