// Files the user actually handed to flipfile: dropped on the window (recorded from the
// native drag-drop event in lib.rs, before the UI even sees it) or pasted with Cmd/Ctrl+V
// (recorded by clipboard.rs). `convert_batch` refuses every other path.
//
// This keeps the backend safe even if the webview were ever compromised: injected script
// could not make flipfile read, convert or write next to files the user never chose.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct Grants(Mutex<HashSet<PathBuf>>);

impl Grants {
    /// A new drop or paste replaces the previous grant; conversions already running
    /// validated their paths when they started.
    pub fn replace(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut granted = self.0.lock().unwrap();
        granted.clear();
        granted.extend(paths);
    }

    /// Returns the first path that was not granted, if any. Exact match against what
    /// the OS handed over, no normalisation: a path spelled differently (`..`, a
    /// symlinked parent, other case) was not granted.
    pub fn check<'a>(&self, paths: &'a [String]) -> Result<(), &'a str> {
        let granted = self.0.lock().unwrap();
        match paths.iter().find(|p| !granted.contains(Path::new(p.as_str()))) {
            Some(denied) => Err(denied.as_str()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_granted_paths_pass() {
        let grants = Grants::default();
        assert!(grants.check(&["/a/photo.png".into()]).is_err()); // nothing granted yet

        grants.replace([PathBuf::from("/a/photo.png"), PathBuf::from("/b/doc.pdf")]);
        assert!(grants.check(&["/a/photo.png".into(), "/b/doc.pdf".into()]).is_ok());
        assert_eq!(grants.check(&["/a/photo.png".into(), "/etc/passwd".into()]), Err("/etc/passwd"));
        // Same file, different spelling: refused
        assert!(grants.check(&["/a/../a/photo.png".into()]).is_err());

        // A new drop replaces the old one
        grants.replace([PathBuf::from("/c/new.jpg")]);
        assert!(grants.check(&["/a/photo.png".into()]).is_err());
        assert!(grants.check(&["/c/new.jpg".into()]).is_ok());
    }
}
