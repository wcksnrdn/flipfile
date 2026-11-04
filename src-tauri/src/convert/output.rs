// Where results are written: unique, never-overwriting paths next to the input
// (or in the Downloads folder), and the shared "could not save" error.
//
// Files of one batch convert in parallel, so a name is reserved the moment it is handed
// out: `photo.png` and `photo.jpg` both converted to WEBP must not both pick
// `photo_converted.webp` before either file exists. A reservation lasts while the
// conversion that made it runs (see `Reservations`); after that the file itself is there.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static RESERVED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

thread_local! {
    /// Names reserved by this thread, in order, and how many `Reservations` are open
    static MINE: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static OPEN: Cell<usize> = const { Cell::new(0) };
}

/// While alive, names handed out on this thread are reserved; dropping it releases them
/// (their files are written by then, or the conversion failed).
pub(super) struct Reservations {
    from: usize,
}

impl Reservations {
    pub(super) fn start() -> Self {
        OPEN.with(|open| open.set(open.get() + 1));
        Reservations { from: MINE.with(|mine| mine.borrow().len()) }
    }
}

impl Drop for Reservations {
    fn drop(&mut self) {
        OPEN.with(|open| open.set(open.get() - 1));
        let done = MINE.with(|mine| mine.borrow_mut().split_off(self.from));
        let mut reserved = RESERVED.lock().unwrap();
        for path in &done {
            reserved.remove(path);
        }
    }
}

/// Takes `candidate` if no file has that name and no running conversion reserved it.
fn claim(candidate: &Path) -> bool {
    if OPEN.with(Cell::get) == 0 {
        return !candidate.exists();
    }
    let mut reserved = RESERVED.lock().unwrap();
    if candidate.exists() || reserved.contains(candidate) {
        return false;
    }
    reserved.insert(candidate.to_path_buf());
    MINE.with(|mine| mine.borrow_mut().push(candidate.to_path_buf()));
    true
}

pub(super) fn save_err(e: impl ToString) -> String {
    format!("Could not save: {}", e.to_string())
}

/// Like `unique_output_path`, for the folder that holds the pages of a PDF.
pub(super) fn unique_output_dir(input: &Path, out_dir: Option<&Path>, suffix: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let dir = out_dir.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")));
    let mut candidate = dir.join(format!("{stem}_{suffix}"));
    let mut n = 2;
    while !claim(&candidate) {
        candidate = dir.join(format!("{stem}_{suffix}_{n}"));
        n += 1;
    }
    candidate
}

/// Results never overwrite an existing file. By default they go next to the input
/// as `name_suffix.ext`; with `out_dir` (pasted clipboard images, whose source is a
/// temporary file) they go there under the plain name `name.ext`.
pub(super) fn unique_output_path(input: &Path, out_dir: Option<&Path>, suffix: &str, ext: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let (dir, base) = match out_dir {
        Some(dir) => (dir, stem.to_string()),
        None => (input.parent().unwrap_or(Path::new(".")), format!("{stem}_{suffix}")),
    };
    let mut candidate = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while !claim(&candidate) {
        candidate = dir.join(format!("{base}_{n}.{ext}"));
        n += 1;
    }
    candidate
}
