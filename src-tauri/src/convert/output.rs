// Where results are written: unique, never-overwriting paths next to the input
// (or in the Downloads folder), and the shared "could not save" error.

use std::path::{Path, PathBuf};

pub(super) fn save_err(e: impl ToString) -> String {
    format!("Could not save: {}", e.to_string())
}

/// Like `unique_output_path`, for the folder that holds the pages of a PDF.
pub(super) fn unique_output_dir(input: &Path, out_dir: Option<&Path>, suffix: &str) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let dir = out_dir.unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")));
    let mut candidate = dir.join(format!("{stem}_{suffix}"));
    let mut n = 2;
    while candidate.exists() {
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
    while candidate.exists() {
        candidate = dir.join(format!("{base}_{n}.{ext}"));
        n += 1;
    }
    candidate
}
