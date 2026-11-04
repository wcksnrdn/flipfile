// In-process typesetting with Typst: turns generated Typst markup (plus images held in
// memory) into a PDF. This is the built-in Word -> PDF engine, so no office suite has
// to be installed.

use std::collections::HashMap;
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::model::Document;
use typst_layout::PagedDocument;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::{FontPath, FontSource, FontStore};

/// The computer's own fonts, so documents use the fonts they were written with when
/// installed (and look-alikes otherwise). Scanned once per run.
fn fonts() -> &'static FontStore {
    static FONTS: OnceLock<FontStore> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut store = FontStore::new();
        store.extend(typst_kit::fonts::system().map(|(path, info)| (MappedFont(path), info)));
        // Microsoft Office on macOS keeps Calibri, Cambria, Aptos, … inside its app
        // bundles instead of the system font folder. Using them (only when Office is
        // installed) makes text exactly as wide as in Word.
        #[cfg(target_os = "macos")]
        for app in ["Microsoft Word", "Microsoft Excel", "Microsoft PowerPoint", "Microsoft Outlook"] {
            let dir = std::path::PathBuf::from(format!("/Applications/{app}.app/Contents/Resources/DFonts"));
            if dir.is_dir() {
                store.extend(typst_kit::fonts::scan(&dir).map(|(path, info)| (MappedFont(path), info)));
                break; // every Office app ships the same set
            }
        }
        store
    })
}

/// A font file mapped into memory rather than read into it: only the parts typesetting
/// touches take up memory, and the system can drop them again. Read whole, fonts stay
/// in memory for as long as flipfile runs, and the fallback for any missing glyph is
/// Apple Color Emoji, 188 MB.
struct MappedFont(FontPath);

impl FontSource for MappedFont {
    fn load(&self) -> Option<Font> {
        let file = std::fs::File::open(&self.0.path).ok()?;
        // Safety: font files are not changed while mapped (the system's are on a
        // read-only volume); a font that cannot be mapped is read instead.
        match unsafe { memmap2::Mmap::map(&file) } {
            Ok(map) => Font::new(Bytes::new(map), self.0.index),
            Err(_) => self.0.load(),
        }
    }
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
    let files = virtual_files(files)?;
    let pdf = compile(markup, files).and_then(|document| export(&document));
    trim_cache();
    pdf
}

/// Like `to_pdf` for markup split into runs of pages that need nothing from each other
/// (absolutely positioned, numbered by the caller): runs are laid out side by side, and
/// their pages make one PDF, with each font and picture in it once. Typst's cache is
/// emptied after every run, as runs share almost nothing: a 313-page workbook then
/// peaks at 1.1 GB instead of 2.4 GB, in 3.3 s instead of 6.2 s.
pub fn to_pdf_in_runs(runs: Vec<String>, files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let files = virtual_files(files)?;
    if runs.len() == 1 {
        let pdf = compile(runs.into_iter().next().unwrap(), files).and_then(|document| export(&document));
        trim_cache();
        return pdf;
    }
    let workers = MAX_LAYOUTS.min(runs.len());
    let next = AtomicUsize::new(0);
    let laid_out: Mutex<Vec<Option<Result<PagedDocument, String>>>> = Mutex::new((0..runs.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(markup) = runs.get(i) else { break };
                let document = compile(markup.clone(), files.clone());
                typst::comemo::evict(0);
                laid_out.lock().unwrap()[i] = Some(document);
            });
        }
    });
    let mut pages = vec![];
    let mut info = None;
    for document in laid_out.into_inner().unwrap() {
        let document = document.expect("every run was laid out")?;
        info.get_or_insert_with(|| document.info().clone());
        pages.extend_from_slice(document.pages());
    }
    let document = PagedDocument::new(pages.into_iter().collect(), info.unwrap_or_default());
    let pdf = export(&document);
    trim_cache();
    pdf
}

fn virtual_files(files: Vec<(String, Vec<u8>)>) -> Result<HashMap<FileId, Bytes>, String> {
    files.into_iter().map(|(path, data)| Ok((file_id(&path)?, Bytes::new(data)))).collect()
}

/// Layouts that run at the same time, in the whole app: Typst's layouts share one
/// cache, and beyond four they only slow each other down (measured on 8 cores).
const MAX_LAYOUTS: usize = 4;

fn compile(markup: String, files: HashMap<FileId, Bytes>) -> Result<PagedDocument, String> {
    use std::sync::{Condvar, Mutex};
    static RUNNING: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            *RUNNING.0.lock().unwrap() -= 1;
            RUNNING.1.notify_one();
        }
    }

    if fonts().book().families().next().is_none() {
        return Err("no fonts were found on this computer".into());
    }
    let world = DocWorld { main: Source::new(file_id("/main.typ")?, markup), files };
    let _slot = {
        let mut running = RUNNING.0.lock().unwrap();
        while *running >= MAX_LAYOUTS {
            running = RUNNING.1.wait(running).unwrap();
        }
        *running += 1;
        Slot
    };
    typst::compile::<PagedDocument>(&world).output.map_err(messages)
}

fn export(document: &PagedDocument) -> Result<Vec<u8>, String> {
    typst_pdf::pdf(document, &typst_pdf::PdfOptions::default()).map_err(messages)
}

fn messages(errors: impl IntoIterator<Item = typst::diag::SourceDiagnostic>) -> String {
    errors.into_iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; ")
}

/// Typst keeps its work in a global cache that only grows (12 MB more with every Word
/// document, kept until flipfile quits). What the last document did not reuse is
/// dropped, so a batch of similar documents stays fast and memory stays flat.
fn trim_cache() {
    typst::comemo::evict(1);
}

/// Empties Typst's cache, once a batch is done.
pub fn release_caches() {
    typst::comemo::evict(0);
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
