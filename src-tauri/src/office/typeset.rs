// In-process typesetting with Typst: turns generated Typst markup (plus images held in
// memory) into a PDF. This is the built-in Word -> PDF engine, so no office suite has
// to be installed.

use std::collections::HashMap;
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst_layout::PagedDocument;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;

/// The computer's own fonts, so documents use the fonts they were written with when
/// installed (and look-alikes otherwise). Scanned once per run.
fn fonts() -> &'static FontStore {
    static FONTS: OnceLock<FontStore> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut store = FontStore::new();
        store.extend(typst_kit::fonts::system());
        // Microsoft Office on macOS keeps Calibri, Cambria, Aptos, … inside its app
        // bundles instead of the system font folder. Using them (only when Office is
        // installed) makes text exactly as wide as in Word.
        #[cfg(target_os = "macos")]
        for app in ["Microsoft Word", "Microsoft Excel", "Microsoft PowerPoint", "Microsoft Outlook"] {
            let dir = std::path::PathBuf::from(format!("/Applications/{app}.app/Contents/Resources/DFonts"));
            if dir.is_dir() {
                store.extend(typst_kit::fonts::scan(&dir));
                break; // every Office app ships the same set
            }
        }
        store
    })
}

/// Word's line box for a font, in ems: (ascent, descent) from its Windows metrics.
/// A "single" line is ascent + descent tall (Calibri 1.22, Arial 1.15, …) with the
/// baseline `ascent` below its top. Uses the first of `families` that is installed.
pub fn line_metrics(families: &[String]) -> (f32, f32) {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, (f32, f32)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = families.join("|");
    if let Some(v) = cache.lock().ok().and_then(|c| c.get(&key).copied()) {
        return v;
    }
    let store = fonts();
    let value = families
        .iter()
        .find_map(|f| {
            let index = store.book().select_family(&f.to_lowercase()).next()?;
            let font = store.font(index)?;
            let face = ttf_parser::Face::parse(font.data().as_slice(), font.index()).ok()?;
            let upem = face.units_per_em() as f32;
            let os2 = face.tables().os2?;
            let ascent = os2.windows_ascender() as f32 / upem;
            let descent = -(os2.windows_descender() as f32) / upem;
            (upem > 0.0 && ascent > 0.5 && descent >= 0.0 && ascent + descent < 2.5).then_some((ascent, descent))
        })
        // Typical of the common Office fonts
        .unwrap_or((0.95, 0.25));
    if let Ok(mut c) = cache.lock() {
        c.insert(key, value);
    }
    value
}

/// Whether a font family is installed (or bundled with Office) on this computer
pub fn has_font(family: &str) -> bool {
    fonts().book().select_family(&family.to_lowercase()).next().is_some()
}

/// Natural width of `text` set in `family` (or `None` when that font is not installed),
/// in points: what Word needs for it without any squeezing.
pub fn text_width(family: &str, size: f32, bold: bool, italic: bool, text: &str) -> Option<f32> {
    use typst::text::{FontStretch, FontStyle, FontVariant, FontWeight};
    let store = fonts();
    let variant = FontVariant::new(
        if italic { FontStyle::Italic } else { FontStyle::Normal },
        if bold { FontWeight::BOLD } else { FontWeight::REGULAR },
        FontStretch::NORMAL,
    );
    let index = store.book().select(&family.to_lowercase(), variant)?;
    let font = store.font(index)?;
    let face = ttf_parser::Face::parse(font.data().as_slice(), font.index()).ok()?;
    let upem = face.units_per_em() as f32;
    let units: f32 = text
        .chars()
        .map(|c| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).unwrap_or((upem * 0.5) as u16) as f32)
        .sum();
    Some(units / upem * size)
}

fn library() -> &'static LazyHash<Library> {
    static LIBRARY: OnceLock<LazyHash<Library>> = OnceLock::new();
    LIBRARY.get_or_init(|| LazyHash::new(Library::default()))
}

fn file_id(path: &str) -> Result<FileId, String> {
    let vpath = VirtualPath::new(path).map_err(|e| format!("{e:?}"))?;
    Ok(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

struct DocWorld {
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl World for DocWorld {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        fonts().book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    // Only the images taken from the document exist; nothing on disk is reachable.
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// Compiles `markup` to PDF. `files` are virtual files the markup can reference by
/// path (e.g. `/img/1.png`).
pub fn to_pdf(markup: String, files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    let main = Source::new(file_id("/main.typ")?, markup);
    let mut map = HashMap::new();
    for (path, data) in files {
        map.insert(file_id(&path)?, Bytes::new(data));
    }
    if fonts().book().families().next().is_none() {
        return Err("no fonts were found on this computer".into());
    }
    let world = DocWorld { main, files: map };

    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))?;
    typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|errors| errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesets_markup_to_pdf() {
        let pdf = to_pdf(
            "#set page(width: 210mm, height: 297mm)\n#text(size: 14pt, weight: \"bold\", \"Hello flipfile\")\n\n#lorem(200)".into(),
            vec![],
        )
        .unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.len() > 1000);
    }
}
