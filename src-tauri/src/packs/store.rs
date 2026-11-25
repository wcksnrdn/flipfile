// The models folder the packs are installed in (<app data>/models), laid out like the
// bundled one so a model is found the same way wherever it is:
//
//   upscale-photo.mlmodelc/...   installed models, each put in place as a whole
//   installed.json               the SHA-256 of every installed file, so nothing is hashed
//                                again to know what is there
//   .downloads/<sha256>          downloaded files, there only once checked (<sha256>.part
//                                while downloading, picked up again after a stop)
//   .staging/<model>             a model being put together from them; <model>.ready when
//                                it is complete but couldn't be put in place yet (Windows
//                                keeps a model in use locked), done at the next start
//   .trash/                      models replaced or removed, deleted when nothing holds them

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::catalog::{model_of, Catalog, File, Pack};
use super::download::save_error;

/// Writes to installed.json, one at a time
static STATE: Mutex<()> = Mutex::new(());

pub type Installed = BTreeMap<String, String>;

#[derive(Clone)]
pub struct Store {
    /// Where packs are installed
    pub dir: PathBuf,
    /// Models that came with the app: the bundle (and in debug builds and tests,
    /// src-tauri/models), as the built-in catalog lists them
    pub bundled: Vec<PathBuf>,
}

/// Where a pack is, on this computer
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Presence {
    /// Every file, as the catalog has it
    Current,
    /// Every model is there, but one or more is older than the catalog's
    Outdated,
    /// A model is missing
    Missing,
}

impl Store {
    pub fn downloads(&self) -> PathBuf {
        self.dir.join(".downloads")
    }

    fn staging(&self) -> PathBuf {
        self.dir.join(".staging")
    }

    pub fn installed(&self) -> Installed {
        fs::read(self.dir.join("installed.json")).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
    }

    fn save_installed(&self, installed: &Installed) -> Result<(), String> {
        let path = self.dir.join("installed.json");
        let partial = self.dir.join("installed.json.partial");
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        fs::write(&partial, serde_json::to_vec_pretty(installed).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        fs::rename(&partial, &path).map_err(|e| e.to_string())
    }

    /// Where `file` is as the catalog has it: installed, or bundled the same
    pub fn current(&self, file: &File, built_in: &Catalog, installed: &Installed) -> Option<PathBuf> {
        let path = self.dir.join(&file.path);
        if installed.get(&file.path) == Some(&file.sha256) && path.is_file() {
            return Some(path);
        }
        let bundled_same = built_in.file(&file.path).is_some_and(|f| f.sha256 == file.sha256);
        self.bundled.iter().map(|dir| dir.join(&file.path)).find(|path| bundled_same && path.is_file())
    }

    /// Whether `model` is downloaded and waits for the next start to be put in place
    pub fn waiting(&self, model: &str) -> bool {
        self.staging().join(format!("{model}.ready")).is_file()
    }

    /// Whether `model` is anywhere, in any version
    pub fn has_model(&self, model: &str) -> bool {
        std::iter::once(&self.dir).chain(&self.bundled).any(|dir| dir.join(model).exists())
    }

    pub fn presence(&self, pack: &Pack, built_in: &Catalog, installed: &Installed) -> Presence {
        if !pack.models().iter().all(|m| self.has_model(m)) {
            Presence::Missing
        } else if pack.files().all(|f| self.current(f, built_in, installed).is_some()) {
            Presence::Current
        } else {
            Presence::Outdated
        }
    }

    /// The files of `pack` it has to download: neither current here nor downloaded yet
    pub fn to_download<'a>(&self, pack: &'a Pack, built_in: &Catalog, installed: &Installed) -> Vec<&'a File> {
        let mut files: Vec<&File> = vec![];
        for file in pack.files() {
            let needed = self.current(file, built_in, installed).is_none()
                && !self.downloads().join(&file.sha256).is_file()
                && !files.iter().any(|f| f.sha256 == file.sha256);
            if needed {
                files.push(file);
            }
        }
        files
    }

    /// Puts every model of `pack` that isn't current in place, from the files current
    /// here and those downloaded; true when all are in place, false when one waits for
    /// the next start (see `finish_pending`)
    pub fn assemble(&self, pack: &Pack, built_in: &Catalog) -> Result<bool, String> {
        let mut all_in_place = true;
        for model in pack.models() {
            let installed = self.installed();
            let files: Vec<&File> = pack.files().filter(|f| f.model() == model).collect();
            if files.iter().all(|f| self.current(f, built_in, &installed).is_some_and(|p| p.starts_with(&self.dir))) {
                continue;
            }
            // Bundled and current: nothing to install (the full app, as it came)
            if files.iter().all(|f| self.current(f, built_in, &installed).is_some()) && !self.dir.join(model).exists() {
                continue;
            }
            self.stage(model, &files, built_in, &installed)?;
            all_in_place &= self.commit(model, &files)?;
        }
        Ok(all_in_place)
    }

    /// Builds .staging/<model> from its files: the current ones where they are, the
    /// others from .downloads
    fn stage(&self, model: &str, files: &[&File], built_in: &Catalog, installed: &Installed) -> Result<(), String> {
        let staging = self.staging();
        let staged = staging.join(model);
        let _ = fs::remove_file(staging.join(format!("{model}.ready")));
        remove(&staged);
        for file in files {
            let to = staging.join(&file.path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let from = self.current(file, built_in, installed).unwrap_or_else(|| self.downloads().join(&file.sha256));
            if !from.is_file() {
                return Err(format!("{} was not downloaded", file.path));
            }
            // A link where it can be (no second copy on disk), never into the app bundle
            let linked = from.starts_with(&self.dir) && fs::hard_link(&from, &to).is_ok();
            if !linked {
                fs::copy(&from, &to).map_err(|e| save_error(&e))?;
            }
        }
        // What it holds, for `finish_pending`, which may run with another catalog
        let list = serde_json::to_vec(files).map_err(|e| e.to_string())?;
        fs::write(staging.join(format!("{model}.ready")), list).map_err(|e| save_error(&e))
    }

    /// Puts the staged `model` in place of the installed one, as a whole. False when the
    /// installed one is in use and locked (Windows): it is done at the next start.
    fn commit(&self, model: &str, files: &[&File]) -> Result<bool, String> {
        let staging = self.staging();
        let staged = staging.join(model);
        let dest = self.dir.join(model);
        let trash = self.dir.join(".trash");
        let _guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let old = trash.join(format!("{model}-{}", nanos()));
        if dest.exists() {
            fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
            if fs::rename(&dest, &old).is_err() {
                return Ok(false);
            }
        }
        if let Err(e) = fs::rename(&staged, &dest) {
            if old.exists() {
                let _ = fs::rename(&old, &dest);
            }
            return Err(format!("could not install {model} ({e})"));
        }
        let _ = fs::remove_file(staging.join(format!("{model}.ready")));
        let mut installed = self.installed();
        installed.retain(|path, _| model_of(path) != model);
        installed.extend(files.iter().map(|f| (f.path.clone(), f.sha256.clone())));
        self.save_installed(&installed)?;
        drop(_guard);
        remove(&old);
        for file in files {
            let _ = fs::remove_file(self.downloads().join(&file.sha256));
        }
        Ok(true)
    }

    /// At start, before any model is loaded: puts in place the models that were waiting,
    /// and empties the trash
    pub fn finish_pending(&self) {
        if let Ok(entries) = fs::read_dir(self.staging()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(model) = name.strip_suffix(".ready") else { continue };
                let files: Vec<File> = fs::read(entry.path()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default();
                let complete = !files.is_empty() && files.iter().all(|f| f.model() == model);
                if !complete || self.commit(model, &files.iter().collect::<Vec<_>>()).is_err() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        remove(&self.dir.join(".trash"));
    }

    /// Removes the installed models of `pack` no other pack still has in place, and its
    /// unfinished downloads. Bundled models stay: they are part of the app.
    pub fn remove(&self, pack: &Pack, catalog: &Catalog) -> Result<(), String> {
        let others: Vec<&str> = catalog
            .packs
            .iter()
            .filter(|p| p.id != pack.id && p.models().iter().all(|m| self.has_model(m)))
            .flat_map(Pack::models)
            .collect();
        let trash = self.dir.join(".trash");
        let _guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let mut installed = self.installed();
        let mut moved = vec![];
        for model in pack.models() {
            remove(&self.staging().join(model));
            let _ = fs::remove_file(self.staging().join(format!("{model}.ready")));
            let path = self.dir.join(model);
            if others.contains(&model) || !path.exists() {
                continue;
            }
            fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
            let old = trash.join(format!("{model}-{}", nanos()));
            fs::rename(&path, &old).map_err(|_| format!("{} is in use: quit flipfile and try again", pack.name))?;
            installed.retain(|p, _| model_of(p) != model);
            moved.push(old);
        }
        self.save_installed(&installed)?;
        drop(_guard);
        for old in moved {
            remove(&old);
        }
        for file in pack.files() {
            let _ = fs::remove_file(self.downloads().join(&file.sha256));
            let _ = fs::remove_file(self.downloads().join(format!("{}.part", file.sha256)));
        }
        Ok(())
    }
}

/// Deletes a file or folder, if it is there and nothing holds it
fn remove(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default()
}
