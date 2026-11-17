//! Vector graphics: ruling lines, shaded rectangles and drawings

use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Seg {
    pub(super) horizontal: bool,
    /// y of a horizontal segment, x of a vertical one
    pub(super) pos: f32,
    pub(super) a: f32,
    pub(super) b: f32,
    pub(super) width: f32,
    pub(super) color: [u8; 3],
}

impl Seg {
    pub(super) fn rect(&self) -> Rect {
        let half = self.width.max(0.5) / 2.0;
        if self.horizontal {
            Rect { l: self.a, r: self.b, t: self.pos - half, b: self.pos + half }
        } else {
            Rect { l: self.pos - half, r: self.pos + half, t: self.a, b: self.b }
        }
    }
}

#[derive(Default)]
pub(super) struct Graphics {
    pub(super) segs: Vec<Seg>,
    pub(super) fills: Vec<(Rect, [u8; 3])>,
    pub(super) drawings: Vec<Rect>,
}

pub(super) fn graphics(page: &PdfPage, page_h: f32) -> Graphics {
    let mut g = Graphics::default();
    for object in page.objects().iter() {
        collect_graphics(&object, None, page_h, &mut g, 0);
    }
    g
}

fn collect_graphics(object: &PdfPageObject, parent: Option<PdfMatrix>, page_h: f32, g: &mut Graphics, depth: u32) {
    if let Some(form) = object.as_x_object_form_object() {
        if depth < 4 {
            let matrix = match (form.matrix().ok(), parent) {
                (Some(m), Some(p)) => Some(m.multiply(p)),
                (m, p) => m.or(p),
            };
            for child in form.iter() {
                collect_graphics(&child, matrix, page_h, g, depth + 1);
            }
        }
        return;
    }
    let Some(path) = object.as_path_object() else { return };
    let filled = path.fill_mode().map(|m| m != PdfPathFillMode::None).unwrap_or(false);
    let stroked = path.is_stroked().unwrap_or(false);
    let fill = color_of(object.fill_color()).filter(|c| filled && !is_white(*c));
    let stroke = color_of(object.stroke_color()).filter(|c| stroked && !is_white(*c));
    if fill.is_none() && stroke.is_none() {
        return; // invisible or white (backgrounds, masks)
    }
    let matrix = match (path.matrix().ok(), parent) {
        (Some(m), Some(p)) => Some(m.multiply(p)),
        (m, p) => m.or(p),
    };
    let stroke_width = object.stroke_width().map(|w| w.value).unwrap_or(1.0).max(0.25);
    let scale = matrix.map(|m| ((m.a() * m.a() + m.b() * m.b()).sqrt()).max(0.01)).unwrap_or(1.0);
    let stroke_width = stroke_width * scale;

    // Split into subpaths of points (top-down coordinates)
    let mut subpaths: Vec<(Vec<(f32, f32)>, bool)> = vec![];
    let segments = path.segments();
    for seg in segments.iter() {
        let (x, y) = seg.point();
        let (x, y) = match matrix {
            Some(m) => m.apply_to_points(x, y),
            None => (x, y),
        };
        let p = (x.value, page_h - y.value);
        match seg.segment_type() {
            PdfPathSegmentType::MoveTo => subpaths.push((vec![p], false)),
            PdfPathSegmentType::BezierTo => {
                if subpaths.is_empty() {
                    subpaths.push((vec![], false));
                }
                let last = subpaths.last_mut().unwrap();
                last.0.push(p);
                last.1 = true;
            }
            _ => {
                if subpaths.is_empty() {
                    subpaths.push((vec![], false));
                }
                subpaths.last_mut().unwrap().0.push(p);
            }
        }
        if seg.is_close()
            && let Some(first) = subpaths.last().and_then(|s| s.0.first().copied()) {
                subpaths.last_mut().unwrap().0.push(first);
            }
    }

    for (points, curved) in subpaths {
        if points.len() < 2 {
            continue;
        }
        let bbox = points.iter().fold(Rect { l: f32::MAX, t: f32::MAX, r: f32::MIN, b: f32::MIN }, |r, &(x, y)| Rect {
            l: r.l.min(x),
            t: r.t.min(y),
            r: r.r.max(x),
            b: r.b.max(y),
        });
        let axis = points.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.6 || (w[0].1 - w[1].1).abs() < 0.6);
        if curved || !axis {
            if bbox.w() > 4.0 || bbox.h() > 4.0 {
                g.drawings.push(bbox);
            }
            continue;
        }
        if let Some(color) = fill {
            if bbox.h() <= 2.5 && bbox.w() > 2.0 {
                g.segs.push(Seg { horizontal: true, pos: bbox.cy(), a: bbox.l, b: bbox.r, width: bbox.h().max(0.25), color });
            } else if bbox.w() <= 2.5 && bbox.h() > 2.0 {
                g.segs.push(Seg { horizontal: false, pos: bbox.cx(), a: bbox.t, b: bbox.b, width: bbox.w().max(0.25), color });
            } else if bbox.w() > 2.0 && bbox.h() > 2.0 {
                g.fills.push((bbox, color));
            }
        }
        if let Some(color) = stroke {
            for w in points.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if (y0 - y1).abs() < 0.6 && (x0 - x1).abs() > 1.0 {
                    g.segs.push(Seg { horizontal: true, pos: (y0 + y1) / 2.0, a: x0.min(x1), b: x0.max(x1), width: stroke_width, color });
                } else if (x0 - x1).abs() < 0.6 && (y0 - y1).abs() > 1.0 {
                    g.segs.push(Seg { horizontal: false, pos: (x0 + x1) / 2.0, a: y0.min(y1), b: y0.max(y1), width: stroke_width, color });
                }
            }
        }
    }
}

/// Joins collinear pieces of the same line (tables are often drawn cell by cell)
pub(super) fn merge_segments(mut segs: Vec<Seg>) -> Vec<Seg> {
    segs.sort_by(|a, b| (a.horizontal, a.pos, a.a).partial_cmp(&(b.horizontal, b.pos, b.a)).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Seg> = vec![];
    for s in segs {
        // Compare with recent segments on (almost) the same line
        let found = out.iter_mut().rev().take(12).find(|o| o.horizontal == s.horizontal && (o.pos - s.pos).abs() <= 1.0 && s.a <= o.b + 2.0 && s.b >= o.a - 2.0);
        match found {
            Some(o) => {
                o.a = o.a.min(s.a);
                o.b = o.b.max(s.b);
                o.width = o.width.max(s.width);
            }
            None => out.push(s),
        }
    }
    out
}

pub(super) fn find(parent: &mut [usize], i: usize) -> usize {
    let mut i = i;
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

pub(super) fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Groups of touching segments: each group is a candidate table
pub(super) fn segment_groups(segs: &[Seg]) -> Vec<Vec<usize>> {
    let n = segs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let tol = 2.5;
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&segs[i], &segs[j]);
            let touch = if a.horizontal != b.horizontal {
                let (h, v) = if a.horizontal { (a, b) } else { (b, a) };
                v.pos >= h.a - tol && v.pos <= h.b + tol && h.pos >= v.a - tol && h.pos <= v.b + tol
            } else {
                false
            };
            if touch {
                union(&mut parent, i, j);
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]);
    out
}

fn cluster(mut values: Vec<f32>, tol: f32) -> Vec<f32> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f32, usize)> = vec![];
    for v in values {
        match out.last_mut() {
            Some((c, n)) if v - *c / *n as f32 <= tol => {
                *c += v;
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out.into_iter().map(|(c, n)| c / n as f32).collect()
}

/// Stacks of horizontal rules of the same length, one under the other, with no
/// vertical lines: the row rules of a table drawn without column lines (common in
/// journals and reports). Returns indexes into `segs`, top to bottom.
pub(super) fn rule_stacks(segs: &[Seg], groups: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut lone: Vec<usize> = groups
        .iter()
        .filter(|g| g.len() == 1 && segs[g[0]].horizontal && segs[g[0]].b - segs[g[0]].a >= 40.0)
        .map(|g| g[0])
        .collect();
    lone.sort_by(|&a, &b| segs[a].pos.partial_cmp(&segs[b].pos).unwrap_or(std::cmp::Ordering::Equal));
    let mut stacks: Vec<Vec<usize>> = vec![];
    for i in lone {
        let s = &segs[i];
        let fits = |stack: &Vec<usize>| {
            let last = &segs[*stack.last().unwrap()];
            (last.a - s.a).abs() <= 3.0 && (last.b - s.b).abs() <= 3.0 && s.pos - last.pos > 4.0 && s.pos - last.pos < 220.0
        };
        match stacks.iter_mut().find(|st| fits(st)) {
            Some(st) => st.push(i),
            None => stacks.push(vec![i]),
        }
    }
    stacks.retain(|st| st.len() >= 3);
    stacks
}

/// A stack of row rules -> table: rows between the rules, columns where the text in
/// between leaves a vertical gap all the way down
pub(super) fn rule_table(segs: &[Seg], stack: &[usize], glyphs: &[Glyph]) -> Option<(Rect, Lattice)> {
    let first = &segs[stack[0]];
    let ys: Vec<f32> = stack.iter().map(|&i| segs[i].pos).collect();
    let (l, r) = (stack.iter().map(|&i| segs[i].a).fold(f32::MAX, f32::min), stack.iter().map(|&i| segs[i].b).fold(f32::MIN, f32::max));
    let rect = Rect { l, r, t: ys[0], b: *ys.last()? };
    let inside: Vec<&Glyph> = glyphs.iter().filter(|g| g.ch != ' ' && rect.contains(g.cx(), g.cy())).collect();
    if inside.len() < 4 {
        return None;
    }
    // Columns: horizontal spans covered by text, merged; their gaps split the columns
    let mut spans: Vec<(f32, f32)> = inside.iter().map(|g| (g.left, g.right)).collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let size = inside.iter().map(|g| g.style.size).fold(0.0, f32::max);
    let mut merged: Vec<(f32, f32)> = vec![];
    for (a, b) in spans {
        match merged.last_mut() {
            Some(m) if a <= m.1 + size * 0.6 => m.1 = m.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut xs = vec![l];
    for w in merged.windows(2) {
        xs.push((w[0].1 + w[1].0) / 2.0);
    }
    xs.push(r);
    // One column is a ruled box of text, not a table
    if xs.len() < 3 {
        return None;
    }
    let border = Some(Border { width: first.width, color: first.color });
    let mut cells = vec![];
    for row in 0..ys.len() - 1 {
        for col in 0..xs.len() - 1 {
            let cell = Rect { l: xs[col], r: xs[col + 1], t: ys[row], b: ys[row + 1] };
            cells.push(Cell { row, col, rows: 1, cols: 1, rect: cell, borders: [border, None, border, None], fill: None, valign: VAlign::Top, blocks: vec![], text_right: None });
        }
    }
    Some((rect, Lattice { xs, ys, cells }))
}

/// A grid of ruling lines -> table structure (cells with spans and borders)
pub(super) struct Lattice {
    pub(super) xs: Vec<f32>,
    pub(super) ys: Vec<f32>,
    pub(super) cells: Vec<Cell>,
}

pub(super) fn lattice(segs: &[Seg], group: &[usize], fills: &[(Rect, [u8; 3])]) -> Option<Lattice> {
    let hs: Vec<&Seg> = group.iter().map(|&i| &segs[i]).filter(|s| s.horizontal).collect();
    let vs: Vec<&Seg> = group.iter().map(|&i| &segs[i]).filter(|s| !s.horizontal).collect();
    if hs.len() < 2 || vs.len() < 2 {
        return None;
    }
    let xs = cluster(vs.iter().map(|s| s.pos).collect(), 2.0);
    let ys = cluster(hs.iter().map(|s| s.pos).collect(), 2.0);
    if xs.len() < 2 || ys.len() < 2 {
        return None;
    }
    let (nr, nc) = (ys.len() - 1, xs.len() - 1);
    // Is the boundary at grid line `y` drawn between x0 and x1 (or at `x` between y0 and y1)?
    let h_at = |k: usize, c: usize| -> Option<Border> {
        let (y, x0, x1) = (ys[k], xs[c], xs[c + 1]);
        let need = (x1 - x0) * 0.6;
        hs.iter()
            .filter(|s| (s.pos - y).abs() <= 2.0)
            .find(|s| s.b.min(x1) - s.a.max(x0) >= need)
            .map(|s| Border { width: s.width, color: s.color })
    };
    let v_at = |k: usize, r: usize| -> Option<Border> {
        let (x, y0, y1) = (xs[k], ys[r], ys[r + 1]);
        let need = (y1 - y0) * 0.6;
        vs.iter()
            .filter(|s| (s.pos - x).abs() <= 2.0)
            .find(|s| s.b.min(y1) - s.a.max(y0) >= need)
            .map(|s| Border { width: s.width, color: s.color })
    };

    let mut taken = vec![vec![false; nc]; nr];
    let mut cells = vec![];
    for r in 0..nr {
        for c in 0..nc {
            if taken[r][c] {
                continue;
            }
            // Grow right while no vertical line separates, then down while no horizontal one does
            let mut w = 1;
            while c + w < nc && !taken[r][c + w] && v_at(c + w, r).is_none() {
                w += 1;
            }
            let mut h = 1;
            while r + h < nr
                && (c..c + w).all(|cc| !taken[r + h][cc] && h_at(r + h, cc).is_none())
                && (c + 1..c + w).all(|k| v_at(k, r + h).is_none())
            {
                h += 1;
            }
            for row in taken.iter_mut().skip(r).take(h) {
                for t in row.iter_mut().skip(c).take(w) {
                    *t = true;
                }
            }
            let rect = Rect { l: xs[c], r: xs[c + w], t: ys[r], b: ys[r + h] };
            let side = |found: Vec<Option<Border>>| -> Option<Border> {
                let n = found.len();
                let present: Vec<Border> = found.into_iter().flatten().collect();
                (present.len() * 2 >= n && !present.is_empty()).then(|| present[0])
            };
            let borders = [
                side((c..c + w).map(|cc| h_at(r, cc)).collect()),
                side((r..r + h).map(|rr| v_at(c + w, rr)).collect()),
                side((c..c + w).map(|cc| h_at(r + h, cc)).collect()),
                side((r..r + h).map(|rr| v_at(c, rr)).collect()),
            ];
            // Shading: a coloured rectangle covering most of the cell
            let fill = fills
                .iter()
                .rfind(|(f, _)| f.overlap_area(&rect) > rect.area() * 0.6)
                .map(|(_, c)| *c);
            cells.push(Cell { row: r, col: c, rows: h, cols: w, rect, borders, fill, valign: VAlign::Top, blocks: vec![], text_right: None });
        }
    }
    Some(Lattice { xs, ys, cells })
}
