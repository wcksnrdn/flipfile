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
pub struct Grants {
    granted: Mutex<HashSet<PathBuf>>,
    /// Files being dragged over the window right now, not dropped yet: they may only be
    /// measured (the compress ring's size estimates), never converted or written next to
    hovering: Mutex<HashSet<PathBuf>>,
}

impl Grants {
    /// A new drop or paste replaces the previous grant; conversions already running
    /// validated their paths when they started.
    pub fn replace(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut granted = self.granted.lock().unwrap();
        granted.clear();
        granted.extend(paths);
    }

    /// Files dragged over the window (not dropped yet) replace the previous ones
    pub fn hover(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut hovering = self.hovering.lock().unwrap();
        hovering.clear();
        hovering.extend(paths);
    }

    /// Returns the first path that was not granted, if any. Exact match against what
    /// the OS handed over, no normalisation: a path spelled differently (`..`, a
    /// symlinked parent, other case) was not granted.
    pub fn check<'a>(&self, paths: &'a [String]) -> Result<(), &'a str> {
        let granted = self.granted.lock().unwrap();
        match paths.iter().find(|p| !granted.contains(Path::new(p.as_str()))) {
            Some(denied) => Err(denied.as_str()),
            None => Ok(()),
        }
    }

    /// Like `check`, also allowing the files being dragged over the window: only for
    /// reading them to measure, which writes nothing
    pub fn check_measure<'a>(&self, paths: &'a [String]) -> Result<(), &'a str> {
        let (granted, hovering) = (self.granted.lock().unwrap(), self.hovering.lock().unwrap());
        match paths.iter().find(|p| !granted.contains(Path::new(p.as_str())) && !hovering.contains(Path::new(p.as_str()))) {
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

        // Files being dragged over may be measured, not converted
        grants.hover([PathBuf::from("/d/video.mp4")]);
        assert!(grants.check(&["/d/video.mp4".into()]).is_err());
        assert!(grants.check_measure(&["/d/video.mp4".into(), "/c/new.jpg".into()]).is_ok());
        assert!(grants.check_measure(&["/etc/passwd".into()]).is_err());
    }
}
