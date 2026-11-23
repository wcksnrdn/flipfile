// The typefaces Upscale Text can type words in: open fonts bundled with flipfile
// (models/fonts: metric twins of Arial, Times, Courier, Georgia, Calibri and Cambria, and
// common screen faces), and the system's own Arial, Georgia, Verdana and others where it
// has them (the very faces most documents and screenshots use). Text is drawn from the
// glyph outlines (ttf-parser), anti-aliased (tiny-skia).

use std::sync::OnceLock;

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

/// One typeface in one weight
pub(super) struct Face {
    /// Its family's name (what the tests ask for; handy when tracing a choice)
    #[allow(dead_code)]
    pub name: String,
    pub bold: bool,
    data: Vec<u8>,
    index: u32,
}

/// Text drawn at a size: coverage 0 to 1 (row by row, `w` x `h`), placed with its top
/// left corner at (`left`, `top`) from the pen's start on the baseline (`top` negative:
/// above the baseline)
pub(super) struct Drawn {
    pub coverage: Vec<f32>,
    pub w: usize,
    pub h: usize,
    pub left: i32,
    pub top: i32,
}

impl Drawn {
    /// Cropped to where its coverage reaches `threshold` (None when it never does)
    pub fn ink(&self, threshold: f32) -> Option<Drawn> {
        let on = |x: usize, y: usize| self.coverage[y * self.w + x] >= threshold;
        let rows: Vec<usize> = (0..self.h).filter(|&y| (0..self.w).any(|x| on(x, y))).collect();
        let cols: Vec<usize> = (0..self.w).filter(|&x| (0..self.h).any(|y| on(x, y))).collect();
        let (&y0, &y1, &x0, &x1) = (rows.first()?, rows.last()?, cols.first()?, cols.last()?);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let coverage = (y0..=y1).flat_map(|y| self.coverage[y * self.w + x0..=y * self.w + x1].to_vec()).collect();
        Some(Drawn { coverage, w, h, left: self.left + x0 as i32, top: self.top + y0 as i32 })
    }
}

/// Collects one glyph's outline as a path, scaled to pixels and placed at `x`
struct Outline<'a> {
    path: &'a mut PathBuilder,
    scale: f32,
    x: f32,
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.x + x * self.scale, -y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.x + x * self.scale, -y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(self.x + x1 * self.scale, -y1 * self.scale, self.x + x * self.scale, -y * self.scale);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let s = self.scale;
        self.path.cubic_to(self.x + x1 * s, -y1 * s, self.x + x2 * s, -y2 * s, self.x + x * s, -y * s);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

impl Face {
    fn parsed(&self) -> Option<ttf_parser::Face<'_>> {
        ttf_parser::Face::parse(&self.data, self.index).ok()
    }

    /// The glyphs of `text` and where each starts, at `px` pixels per em (kerned)
    fn layout(&self, face: &ttf_parser::Face<'_>, text: &str, px: f32) -> (Vec<(ttf_parser::GlyphId, f32)>, f32) {
        let scale = px / f32::from(face.units_per_em());
        let kern = face.tables().kern;
        let (mut x, mut previous) = (0f32, None);
        let mut glyphs = vec![];
        for ch in text.chars() {
            let id = face.glyph_index(ch).unwrap_or(ttf_parser::GlyphId(0));
            if let (Some(left), Some(kern)) = (previous, kern) {
                let pair = kern.subtables.into_iter().filter(|t| t.horizontal).find_map(|t| t.glyphs_kerning(left, id));
                x += f32::from(pair.unwrap_or(0)) * scale;
            }
            glyphs.push((id, x));
            x += f32::from(face.glyph_hor_advance(id).unwrap_or(0)) * scale;
            previous = Some(id);
        }
        (glyphs, x)
    }

    /// How far the pen moves over `text` at `px` pixels per em
    pub fn advance(&self, text: &str, px: f32) -> f32 {
        self.parsed().map_or(0.0, |face| self.layout(&face, text, px).1)
    }

    /// `text` drawn at `px` pixels per em, the pen starting at (0, 0) on the baseline
    pub fn draw(&self, text: &str, px: f32) -> Option<Drawn> {
        let face = self.parsed()?;
        let scale = px / f32::from(face.units_per_em());
        let (glyphs, _) = self.layout(&face, text, px);
        let mut path = PathBuilder::new();
        for (id, x) in glyphs {
            face.outline_glyph(id, &mut Outline { path: &mut path, scale, x });
        }
        let path = path.finish()?;
        let bounds = path.bounds();
        let (left, top) = (bounds.left().floor() as i32 - 1, bounds.top().floor() as i32 - 1);
        let (w, h) = ((bounds.right().ceil() as i32 + 1 - left) as u32, (bounds.bottom().ceil() as i32 + 1 - top) as u32);
        let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::from_translate(-left as f32, -top as f32), None);
        let coverage = pixmap.pixels().iter().map(|p| f32::from(p.alpha()) / 255.0).collect();
        Some(Drawn { coverage, w: w as usize, h: h as usize, left, top })
    }
}

/// Fonts bundled with flipfile (models/fonts), as (file, name, bold)
const BUNDLED: &[(&str, &str, bool)] = &[
    ("LiberationSans-Regular.ttf", "Liberation Sans", false),
    ("LiberationSans-Bold.ttf", "Liberation Sans", true),
    ("LiberationSerif-Regular.ttf", "Liberation Serif", false),
    ("LiberationSerif-Bold.ttf", "Liberation Serif", true),
    ("LiberationMono-Regular.ttf", "Liberation Mono", false),
    ("Gelasio-Regular.ttf", "Gelasio", false),
    ("Gelasio-Bold.ttf", "Gelasio", true),
    ("Carlito-Regular.ttf", "Carlito", false),
    ("Carlito-Bold.ttf", "Carlito", true),
    ("Caladea-Regular.ttf", "Caladea", false),
    ("Caladea-Bold.ttf", "Caladea", true),
    ("DejaVuSans-Regular.ttf", "DejaVu Sans", false),
    ("DejaVuSans-Bold.ttf", "DejaVu Sans", true),
    ("Inter-Regular.ttf", "Inter", false),
    ("Inter-Bold.ttf", "Inter", true),
    ("Roboto-Regular.ttf", "Roboto", false),
    ("Roboto-Bold.ttf", "Roboto", true),
    ("NotoSans-Regular.ttf", "Noto Sans", false),
    ("NotoSans-Bold.ttf", "Noto Sans", true),
    ("OpenSans-Regular.ttf", "Open Sans", false),
    ("OpenSans-Bold.ttf", "Open Sans", true),
];

/// The system's own faces worth trying, where it has them: (file, face index, name, bold)
#[cfg(target_os = "macos")]
const SYSTEM: &[(&str, u32, &str, bool)] = &[
    ("/System/Library/Fonts/Supplemental/Arial.ttf", 0, "Arial", false),
    ("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 0, "Arial", true),
    ("/System/Library/Fonts/Helvetica.ttc", 0, "Helvetica", false),
    ("/System/Library/Fonts/Helvetica.ttc", 1, "Helvetica", true),
    ("/System/Library/Fonts/Supplemental/Georgia.ttf", 0, "Georgia", false),
    ("/System/Library/Fonts/Supplemental/Georgia Bold.ttf", 0, "Georgia", true),
    ("/System/Library/Fonts/Supplemental/Times New Roman.ttf", 0, "Times New Roman", false),
    ("/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf", 0, "Times New Roman", true),
    ("/System/Library/Fonts/Supplemental/Verdana.ttf", 0, "Verdana", false),
    ("/System/Library/Fonts/Supplemental/Verdana Bold.ttf", 0, "Verdana", true),
    ("/System/Library/Fonts/Supplemental/Tahoma.ttf", 0, "Tahoma", false),
    ("/System/Library/Fonts/Supplemental/Tahoma Bold.ttf", 0, "Tahoma", true),
    ("/System/Library/Fonts/Supplemental/Trebuchet MS.ttf", 0, "Trebuchet MS", false),
    ("/System/Library/Fonts/Supplemental/Trebuchet MS Bold.ttf", 0, "Trebuchet MS", true),
    ("/System/Library/Fonts/Supplemental/Courier New.ttf", 0, "Courier New", false),
];

#[cfg(windows)]
const SYSTEM: &[(&str, u32, &str, bool)] = &[
    ("C:\\Windows\\Fonts\\arial.ttf", 0, "Arial", false),
    ("C:\\Windows\\Fonts\\arialbd.ttf", 0, "Arial", true),
    ("C:\\Windows\\Fonts\\segoeui.ttf", 0, "Segoe UI", false),
    ("C:\\Windows\\Fonts\\segoeuib.ttf", 0, "Segoe UI", true),
    ("C:\\Windows\\Fonts\\calibri.ttf", 0, "Calibri", false),
    ("C:\\Windows\\Fonts\\calibrib.ttf", 0, "Calibri", true),
    ("C:\\Windows\\Fonts\\georgia.ttf", 0, "Georgia", false),
    ("C:\\Windows\\Fonts\\georgiab.ttf", 0, "Georgia", true),
    ("C:\\Windows\\Fonts\\times.ttf", 0, "Times New Roman", false),
    ("C:\\Windows\\Fonts\\timesbd.ttf", 0, "Times New Roman", true),
    ("C:\\Windows\\Fonts\\verdana.ttf", 0, "Verdana", false),
    ("C:\\Windows\\Fonts\\verdanab.ttf", 0, "Verdana", true),
    ("C:\\Windows\\Fonts\\tahoma.ttf", 0, "Tahoma", false),
    ("C:\\Windows\\Fonts\\tahomabd.ttf", 0, "Tahoma", true),
    ("C:\\Windows\\Fonts\\trebuc.ttf", 0, "Trebuchet MS", false),
    ("C:\\Windows\\Fonts\\trebucbd.ttf", 0, "Trebuchet MS", true),
    ("C:\\Windows\\Fonts\\cour.ttf", 0, "Courier New", false),
];

#[cfg(not(any(target_os = "macos", windows)))]
const SYSTEM: &[(&str, u32, &str, bool)] = &[];

/// Every face available here, loaded once
pub(super) fn faces() -> &'static [Face] {
    static FACES: OnceLock<Vec<Face>> = OnceLock::new();
    FACES.get_or_init(|| {
        let mut faces = vec![];
        let mut add = |path: std::path::PathBuf, index: u32, name: &str, bold: bool| {
            if let Some(data) = std::fs::read(&path).ok().filter(|data| ttf_parser::Face::parse(data, index).is_ok()) {
                faces.push(Face { name: name.to_string(), bold, data, index });
            }
        };
        for &(file, name, bold) in BUNDLED {
            if let Some(path) = crate::platform::upscale::model_path(&format!("fonts/{file}")) {
                add(path, 0, name, bold);
            }
        }
        for &(file, index, name, bold) in SYSTEM {
            add(std::path::PathBuf::from(file), index, name, bold);
        }
        faces
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_fonts_draw_text() {
        let faces = faces();
        assert!(faces.len() >= BUNDLED.len(), "{} faces", faces.len());
        let sans = faces.iter().find(|f| f.name == "Liberation Sans" && !f.bold).unwrap();
        let drawn = sans.draw("Hag", 100.0).unwrap().ink(0.3).unwrap();
        // Ascender above the baseline, descender (g) below it
        assert!(drawn.top < -60 && drawn.top + drawn.h as i32 > 15, "{} {}", drawn.top, drawn.h);
        assert!((sans.advance("Hag", 100.0) - drawn.w as f32).abs() < 25.0);
        // Bold is darker than regular
        let bold = faces.iter().find(|f| f.name == "Liberation Sans" && f.bold).unwrap();
        let ink = |d: Drawn| d.coverage.iter().sum::<f32>();
        assert!(ink(bold.draw("Hag", 100.0).unwrap()) > 1.2 * ink(sans.draw("Hag", 100.0).unwrap()));
    }
}
