//! One page

use super::*;

/// The page with only its decoration (and annotations: stamps, highlights, signatures)
/// left, rendered to a transparent PNG the size of the page
pub(super) fn background(doc: &PdfDocument, index: usize, keep: &[usize], underlines: &[Rect], media: &mut MediaStore) -> Option<usize> {
    let mut page = doc.pages().get(index as i32).ok()?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    let count = page.objects().len();
    for i in (0..count).rev() {
        if keep.binary_search(&i).is_err() {
            let _ = page.objects_mut().remove_object_at_index(i);
        }
    }
    // Text inside the forms kept for their graphics is real text in the document: it
    // must not show a second time in the background
    let page_h = page.height().value;
    for object in page.objects().iter() {
        if let Some(form) = object.as_x_object_form_object() {
            hide_text(form, form.matrix().ok(), page_h, underlines, 0);
        }
    }
    // 150 dpi, but at most 2500 px on the long side (slides can be huge pages)
    let longest = page.width().value.max(page.height().value).max(1.0);
    let scale = (BACKGROUND_DPI / 72.0).min(2500.0 / longest);
    let config = PdfRenderConfig::new().scale_page_by_factor(scale).set_clear_color(PdfColor::new(255, 255, 255, 0));
    let image = page.render_with_config(&config).ok()?.as_image().ok()?;
    // Nothing visible left (e.g. invisible annotations): no picture
    let rgba = image.to_rgba8();
    if !rgba.pixels().any(|p| p[3] > 8) {
        return None;
    }
    // Covering (nearly) the whole page, like a designed or photo backdrop: nothing to
    // see through, and JPEG is a fraction of the size
    let opaque = rgba.pixels().filter(|p| p[3] >= 250).count();
    if opaque as f64 > rgba.pixels().len() as f64 * 0.97 {
        let mut jpg = Vec::new();
        let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85);
        image::DynamicImage::ImageRgb8(rgb).write_with_encoder(encoder).ok()?;
        return Some(media.add(jpg, "jpeg"));
    }
    // Mostly sparse line art: grey when it has no colour, and the strongest compression
    let grey = rgba.pixels().all(|p| p[3] == 0 || (p[0] == p[1] && p[1] == p[2]));
    let picture = if grey { image::DynamicImage::ImageLumaA8(image::DynamicImage::ImageRgba8(rgba).to_luma_alpha8()) } else { image::DynamicImage::ImageRgba8(rgba) };
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(&mut png, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive);
    picture.write_with_encoder(encoder).ok()?;
    Some(media.add(png, "png"))
}

/// `shown`: the size the picture has on the page (points); more pixels than 300 dpi
/// at that size are not kept
/// Inside a form kept for the background: its text and the rules that became Word
/// underlines are made invisible (they are in the document itself)
fn hide_text(form: &PdfPageXObjectFormObject, matrix: Option<PdfMatrix>, page_h: f32, underlines: &[Rect], depth: u32) {
    for i in 0..form.len() {
        let Ok(mut child) = form.get(i) else { continue };
        if let Some(text) = child.as_text_object_mut() {
            let _ = text.set_render_mode(PdfPageTextRenderMode::Invisible);
            continue;
        }
        if child.object_type() == PdfPageObjectType::Path && !underlines.is_empty()
            && let Ok(b) = child.bounds() {
                // The form's own placement on the page
                let corners = [(b.left(), b.bottom()), (b.right(), b.top())].map(|(x, y)| match matrix {
                    Some(m) => m.apply_to_points(x, y),
                    None => (x, y),
                });
                let (l, r) = (corners[0].0.value.min(corners[1].0.value), corners[0].0.value.max(corners[1].0.value));
                let (t, bt) = (page_h - corners[0].1.value.max(corners[1].1.value), page_h - corners[0].1.value.min(corners[1].1.value));
                let rect = Rect { l, r, t, b: bt };
                let is_underline = rect.h() <= 3.5 && underlines.iter().any(|u| (u.cy() - rect.cy()).abs() < 2.0 && u.r.min(rect.r) - u.l.max(rect.l) > rect.w() * 0.5);
                if is_underline {
                    let _ = child.set_fill_color(PdfColor::new(0, 0, 0, 0));
                    let _ = child.set_stroke_color(PdfColor::new(0, 0, 0, 0));
                }
            }
        if depth < 4
            && let Some(inner) = child.as_x_object_form_object() {
                let m = match (inner.matrix().ok(), matrix) {
                    (Some(a), Some(b)) => Some(a.multiply(b)),
                    (a, b) => a.or(b),
                };
                hide_text(inner, m, page_h, underlines, depth + 1);
            }
    }
}

fn encode_image(img: &image::DynamicImage, shown: Option<Rect>, media: &mut MediaStore) -> Option<usize> {
    let mut owned;
    let mut img = img;
    if let Some(r) = shown {
        let max_w = (r.w() * 300.0 / 72.0).max(16.0) as u32;
        if img.width() > max_w + max_w / 7 {
            let h = ((img.height() as f32) * max_w as f32 / img.width() as f32).round().max(1.0) as u32;
            owned = img.resize_exact(max_w, h, image::imageops::FilterType::Lanczos3);
            img = &owned;
        }
    }
    // An alpha channel that is opaque everywhere is no transparency at all
    if img.color().has_alpha() && img.to_rgba8().pixels().all(|p| p[3] == 255) {
        owned = image::DynamicImage::ImageRgb8(img.to_rgb8());
        img = &owned;
    }
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    // Photos are far smaller as JPEG; keep PNG for screenshots, line art and transparency
    if png.len() > 150_000 && !img.color().has_alpha() {
        let mut jpg = Vec::new();
        let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90);
        if rgb.write_with_encoder(encoder).is_ok() && jpg.len() < png.len() * 2 / 3 {
            return Some(media.add(jpg, "jpeg"));
        }
    }
    Some(media.add(png, "png"))
}

pub(in crate::office) fn page_model(page: &PdfPage, document: &PdfDocument, media: &mut MediaStore) -> PageModel {
    let (page_w, page_h) = (page.width().value, page.height().value);
    let g = graphics(page, page_h);
    let segs = merge_segments(g.segs);
    let groups = segment_groups(&segs);
    let stacks = rule_stacks(&segs, &groups);
    // Underlines: thin horizontal rules that are not part of a grid (table borders)
    // nor of a stack of row rules
    let underlines: Vec<Seg> = groups
        .iter()
        .filter(|grp| grp.iter().all(|&i| segs[i].horizontal))
        .flat_map(|grp| grp.iter().copied())
        .filter(|i| !stacks.iter().any(|s| s.contains(i)))
        .map(|i| segs[i])
        .filter(|s| s.width <= 3.0)
        .collect();
    let all_glyphs = glyphs(page, page_h, &underlines);

    // Drawings (curves, diagonals) and the lines that touch them form pictures
    // Small curves (rounded corners of a card, icons, bullets) are not diagrams: they are
    // drawn in the page's background and the text around them stays text
    let mut regions: Vec<Rect> = vec![];
    for d in &g.drawings {
        if d.w().max(d.h()) >= 15.0 {
            regions.push(*d);
        }
    }
    let group_rects: Vec<Rect> = groups.iter().map(|grp| grp.iter().skip(1).fold(segs[grp[0]].rect(), |r, &i| r.union(&segs[i].rect()))).collect();
    let mut table_groups: Vec<usize> = vec![];
    loop {
        // Merge touching regions
        let mut merged = false;
        'outer: for i in 0..regions.len() {
            for j in i + 1..regions.len() {
                if regions[i].gap(&regions[j]) <= 3.0 {
                    regions[i] = regions[i].union(&regions[j]);
                    regions.remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    for (gi, grp) in groups.iter().enumerate() {
        let rect = group_rects[gi];
        let horizontal = grp.iter().filter(|&&i| segs[i].horizontal).count();
        let vertical = grp.len() - horizontal;
        let grid_like = horizontal >= 3 && vertical >= 3;
        let touching = regions.iter().position(|r| r.gap(&rect) <= 2.0);
        match touching {
            // Boxes joined to arrows: part of a diagram
            Some(ri) if !grid_like => regions[ri] = regions[ri].union(&rect),
            _ => table_groups.push(gi),
        }
    }
    // A "drawing" that is really a frame around lots of text stays text
    let page_area = page_w * page_h;
    regions.retain(|r| {
        if r.w() < 24.0 || r.h() < 12.0 || r.area() > page_area * 0.85 {
            return false;
        }
        let text_area: f32 = all_glyphs.iter().filter(|gl| r.contains(gl.cx(), gl.cy())).map(|gl| (gl.right - gl.left) * gl.style.size).sum();
        text_area < r.area() * 0.2
    });

    // Tables
    let mut tables: Vec<(Rect, Lattice)> = vec![];
    for gi in table_groups {
        let rect = group_rects[gi];
        if regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.5) {
            continue;
        }
        if let Some(lat) = lattice(&segs, &groups[gi], &g.fills) {
            let has_text = all_glyphs.iter().any(|gl| rect.contains(gl.cx(), gl.cy()));
            if lat.cells.len() >= 2 || has_text {
                tables.push((rect, lat));
            }
        }
    }
    for stack in &stacks {
        if let Some((rect, lat)) = rule_table(&segs, stack, &all_glyphs) {
            let taken = tables.iter().any(|(t, _)| t.overlap_area(&rect) > 0.0) || regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.3);
            if !taken {
                tables.push((rect, lat));
            }
        }
    }

    // Raster images. One covering most of the page (a cover, a photo backdrop, a
    // certificate's artwork) is part of the page's background, at its exact place;
    // a scan under an invisible OCR text layer is dropped (the text is what matters).
    let ocr_layer = page
        .objects()
        .iter()
        .filter_map(|o| o.as_text_object().map(|t| t.render_mode()))
        .take(50)
        .filter(|m| matches!(m, PdfPageTextRenderMode::Invisible))
        .count()
        > 10;
    let mut backdrops: Vec<usize> = vec![];
    let mut pictures: Vec<(Rect, image::DynamicImage)> = vec![];
    let mut images: Vec<ImageRef> = vec![];
    for (index, object) in page.objects().iter().enumerate() {
        let Some(image) = object.as_image_object() else { continue };
        let Ok(bounds) = object.bounds() else { continue };
        let rect = Rect { l: bounds.left().value, r: bounds.right().value, t: page_h - bounds.top().value, b: page_h - bounds.bottom().value };
        if rect.w() < 8.0 || rect.h() < 8.0 {
            continue; // decorative dots and lines
        }
        if regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.5) {
            continue; // drawn into the picture of that region
        }
        // Partly over a diagram picture: it stays whole, at its place, behind the text
        if regions.iter().any(|r| r.overlap_area(&rect) > rect.area() * 0.05) {
            backdrops.push(index);
            continue;
        }
        if rect.area() > page_area * 0.5 {
            if !ocr_layer {
                backdrops.push(index);
            }
            continue;
        }
        // Text written over the picture (a labelled box, a banner, a captioned photo):
        // the picture stays at its place behind the text, as part of the background
        let under_text = all_glyphs.iter().any(|gl| gl.ch != ' ' && rect.contains(gl.cx(), gl.cy()));
        if under_text && !ocr_layer {
            backdrops.push(index);
            continue;
        }
        let Ok(img) = image.get_processed_image(document).or_else(|_| image.get_raw_image()) else { continue };
        pictures.push((rect, img));
    }
    // Pictures that touch or overlap form one picture: a logo exported as strips, a
    // photo in tiles, a screenshot with a pop-up over it. Separate inline pictures
    // would show seams (or stack) in Word, so they are put together at full resolution.
    let mut group: Vec<usize> = (0..pictures.len()).collect();
    for i in 0..pictures.len() {
        for j in i + 1..pictures.len() {
            let (a, b) = (&pictures[i].0, &pictures[j].0);
            let touching = a.gap(b) <= 1.5 && (a.overlap_area(b) > 0.0 || (a.b.min(b.b) - a.t.max(b.t)) > 1.0 || (a.r.min(b.r) - a.l.max(b.l)) > 1.0);
            if touching {
                union(&mut group, i, j);
            }
        }
    }
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..pictures.len() {
        let root = find(&mut group, i);
        clusters.entry(root).or_default().push(i);
    }
    let mut clusters: Vec<Vec<usize>> = clusters.into_values().collect();
    clusters.sort_by_key(|c| c[0]);
    for members in clusters {
        if members.len() == 1 {
            let (rect, img) = &pictures[members[0]];
            if let Some(m) = encode_image(img, Some(*rect), media) {
                images.push(ImageRef { media: m, rect: *rect });
            }
            continue;
        }
        let bounds = members.iter().skip(1).fold(pictures[members[0]].0, |r, &i| r.union(&pictures[i].0));
        // The sharpest member sets the resolution (capped at 4000 px on the long side)
        let scale = members
            .iter()
            .map(|&i| pictures[i].1.width() as f32 / pictures[i].0.w().max(1.0))
            .fold(1.0f32, f32::max)
            .min(4000.0 / bounds.w().max(bounds.h()).max(1.0));
        let (cw, ch) = ((bounds.w() * scale).round().max(1.0) as u32, (bounds.h() * scale).round().max(1.0) as u32);
        let mut canvas = image::RgbaImage::new(cw, ch);
        for &i in &members {
            let (rect, img) = &pictures[i];
            let (w, h) = ((rect.w() * scale).round().max(1.0) as u32, (rect.h() * scale).round().max(1.0) as u32);
            let tile = image::imageops::resize(&img.to_rgba8(), w, h, image::imageops::FilterType::Triangle);
            let (x, y) = (((rect.l - bounds.l) * scale).round() as i64, ((rect.t - bounds.t) * scale).round() as i64);
            image::imageops::overlay(&mut canvas, &tile, x, y);
        }
        if let Some(m) = encode_image(&image::DynamicImage::ImageRgba8(canvas), Some(bounds), media) {
            images.push(ImageRef { media: m, rect: bounds });
        }
    }
    // Diagram regions become pictures, rendered from the page
    if !regions.is_empty() {
        let scale = DRAWING_DPI / 72.0;
        if let Ok(bitmap) = page.render_with_config(&PdfRenderConfig::new().scale_page_by_factor(scale))
            && let Ok(full) = bitmap.as_image() {
                for r in &regions {
                    let pad = 1.5;
                    let x = ((r.l - pad).max(0.0) * scale) as u32;
                    let y = ((r.t - pad).max(0.0) * scale) as u32;
                    let w = (((r.w() + pad * 2.0) * scale) as u32).min(full.width().saturating_sub(x));
                    let h = (((r.h() + pad * 2.0) * scale) as u32).min(full.height().saturating_sub(y));
                    if w < 4 || h < 4 {
                        continue;
                    }
                    let crop = full.crop_imm(x, y, w, h);
                    if let Some(m) = encode_image(&crop, None, media) {
                        let rect = Rect { l: x as f32 / scale, t: y as f32 / scale, r: (x + w) as f32 / scale, b: (y + h) as f32 / scale };
                        images.push(ImageRef { media: m, rect });
                    }
                }
            }
    }

    // Distribute text and pictures: pictures' own text, table cells, free flow
    // Tables inside a cell of another table (nested): smallest first, so text goes to
    // the innermost table, and each one knows the cell it sits in
    tables.sort_by(|a, b| a.0.area().partial_cmp(&b.0.area()).unwrap_or(std::cmp::Ordering::Equal));
    let parents: Vec<Option<(usize, usize)>> = (0..tables.len())
        .map(|i| {
            let r = tables[i].0;
            let j = (i + 1..tables.len()).find(|&j| {
                let o = tables[j].0;
                r.l >= o.l - 2.0 && r.r <= o.r + 2.0 && r.t >= o.t - 2.0 && r.b <= o.b + 2.0
            })?;
            let cell = tables[j].1.cells.iter().position(|c| c.rect.contains(r.cx(), r.cy()))?;
            Some((j, cell))
        })
        .collect();
    let mut free_glyphs: Vec<&Glyph> = vec![];
    let mut cell_glyphs: Vec<Vec<Vec<&Glyph>>> = tables.iter().map(|(_, l)| vec![vec![]; l.cells.len()]).collect();
    'glyph: for gl in &all_glyphs {
        let (x, y) = (gl.cx(), gl.cy());
        if regions.iter().any(|r| r.contains(x, y)) {
            continue;
        }
        for (ti, (rect, lat)) in tables.iter().enumerate() {
            if rect.contains(x, y)
                && let Some(ci) = lat.cells.iter().position(|c| c.rect.contains(x, y)) {
                    cell_glyphs[ti][ci].push(gl);
                    continue 'glyph;
                }
        }
        free_glyphs.push(gl);
    }
    let mut free_images: Vec<ImageRef> = vec![];
    let mut cell_images: Vec<Vec<Vec<ImageRef>>> = tables.iter().map(|(_, l)| vec![vec![]; l.cells.len()]).collect();
    'image: for img in images {
        let (x, y) = (img.rect.cx(), img.rect.cy());
        for (ti, (rect, lat)) in tables.iter().enumerate() {
            if rect.contains(x, y)
                && let Some(ci) = lat.cells.iter().position(|c| c.rect.contains(x, y)) {
                    cell_images[ti][ci].push(img);
                    continue 'image;
                }
        }
        free_images.push(img);
    }

    // Decoration: every vector object (and annotation) not turned into something else
    // above stays visible through a picture behind the text
    let underlined: Vec<&Glyph> = all_glyphs.iter().filter(|g| g.style.underline).collect();
    let mut decor = vec![];
    for (index, object) in page.objects().iter().enumerate() {
        if matches!(object.object_type(), PdfPageObjectType::Text | PdfPageObjectType::Image) {
            continue;
        }
        let Ok(b) = object.bounds() else { continue };
        let rect = Rect { l: b.left().value, r: b.right().value, t: page_h - b.top().value, b: page_h - b.bottom().value };
        if rect.w() < 0.3 && rect.h() < 0.3 {
            continue;
        }
        if let Some(path) = object.as_path_object() {
            let filled = path.fill_mode().map(|m| m != PdfPathFillMode::None).unwrap_or(false) && color_of(object.fill_color()).is_some_and(|c| !is_white(c));
            let stroked = path.is_stroked().unwrap_or(false) && color_of(object.stroke_color()).is_some_and(|c| !is_white(c));
            if !filled && !stroked {
                continue;
            }
        }
        let thin = rect.w() <= 3.5 || rect.h() <= 3.5;
        let (cx, cy) = (rect.cx(), rect.cy());
        let in_region = regions.iter().any(|r| r.contains(cx, cy));
        let table_part = tables.iter().any(|(t, lat)| {
            let inside = rect.l >= t.l - 2.0 && rect.r <= t.r + 2.0 && rect.t >= t.t - 2.0 && rect.b <= t.b + 2.0;
            inside && (thin || lat.cells.iter().any(|c| c.fill.is_some() && c.rect.overlap_area(&rect) > c.rect.area() * 0.6))
        });
        let underline = rect.h() <= 3.5
            && underlined.iter().any(|g| cy > g.baseline - 0.5 && cy < g.baseline + g.style.size * 0.35 && rect.l <= g.right && rect.r >= g.left);
        if !in_region && !table_part && !underline {
            decor.push(index);
        }
    }
    decor.extend(backdrops);
    decor.sort_unstable();

    let text_right = free_glyphs.iter().map(|g| g.right).fold(0.0, f32::max).max(page_w * 0.6);
    // Small vector shapes among the text (formulas, symbols drawn as outlines)
    let art: Vec<Rect> = decor
        .iter()
        .filter_map(|&i| page.objects().get(i).ok())
        .filter(|o| matches!(o.object_type(), PdfPageObjectType::Path | PdfPageObjectType::XObjectForm))
        .filter_map(|o| o.bounds().ok())
        .map(|b| Rect { l: b.left().value, r: b.right().value, t: page_h - b.top().value, b: page_h - b.bottom().value })
        .filter(|r| r.h() < 40.0 && r.w() < page_w * 0.5)
        .collect();
    // Tables first: they take part in the page's columns like any other block
    let mut table_blocks = vec![];
    let mut nested: HashMap<(usize, usize), Vec<Block>> = HashMap::new();
    for (ti, (rect, lat)) in tables.into_iter().enumerate() {
        let mut insets = vec![];
        let mut cells = lat.cells;
        for (ci, cell) in cells.iter_mut().enumerate() {
            let glyphs = std::mem::take(&mut cell_glyphs[ti][ci]);
            let imgs = std::mem::take(&mut cell_images[ti][ci]);
            let inner = nested.remove(&(ti, ci)).unwrap_or_default();
            cell.blocks = flow(&glyphs, imgs, cell.rect.r - 1.0, &art, inner);
            if let Some(first) = cell.blocks.first() {
                insets.push(first.rect.l - cell.rect.l);
                let top = first.rect.t - cell.rect.t;
                let bottom = cell.rect.b - cell.blocks.last().unwrap().rect.b;
                cell.valign = if top > 3.0 && (top - bottom).abs() < 3.0 + top * 0.15 {
                    VAlign::Center
                } else if bottom < top - 3.0 && bottom < 4.0 {
                    VAlign::Bottom
                } else {
                    VAlign::Top
                };
            }
        }
        // Small padding: Word then has at least the room the PDF had, so cells do not
        // wrap into more lines (indents keep the text where it was)
        let padding = insets.iter().copied().fold(f32::MAX, f32::min).clamp(0.5, 2.0);
        let block = Block { rect, content: Content::Table(Table { xs: lat.xs, ys: lat.ys, cells, padding }) };
        match parents[ti] {
            Some(cell) => nested.entry(cell).or_default().push(block),
            None => table_blocks.push(block),
        }
    }
    let blocks = flow(&free_glyphs, free_images, text_right, &art, table_blocks);

    // The rules drawn as underlines of text (Word underlines that text instead)
    let underline_rules: Vec<Rect> = underlines
        .iter()
        .filter(|s| underlined.iter().any(|g| s.pos > g.baseline - 0.5 && s.pos < g.baseline + g.style.size * 0.35 && s.a <= g.right && s.b >= g.left))
        .map(|s| s.rect())
        .collect();
    PageModel { width: page_w, height: page_h, header: vec![], body: blocks, footer: vec![], decor, background: None, number: 0, underline_rules, fills: g.fills }
}
