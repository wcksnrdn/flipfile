// AI feature packs: the models of Upscale, Upscale Text, Colorize and Transcribe, which
// people download from Workshop for the features they want instead of every model coming
// with the app. Each file is checked against the catalog's SHA-256 (catalog.rs), the
// catalog itself is built in or signed with the updater's key, and a model is only put in
// place once all of it is there (store.rs). A model is found the same way wherever it is
// (`model_path`): downloaded, bundled with an app that still has it, or fetched for a
// debug build.
//
// The bridge from the apps that came with every model: an app that still has them in its
// bundle copies them here at start (`adopt`), so they stay when an update without them
// replaces the app.
//
// To try downloads in a debug build without publishing anything: `npm run dev:packs`
// (scripts/packs.mjs serves them and runs the app with FLIPFILE_PACKS_URL). Packs are then
// downloaded from there into <app data>/models-test, and no model counts as bundled; the
// fonts and words still come from src-tauri/models.

mod catalog;
mod download;
mod store;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use catalog::{Catalog, File, FORMAT};
use download::Failure;
use store::{Presence, Store};

/// Where packs are installed: <app data>/models
static DOWNLOADED: OnceLock<PathBuf> = OnceLock::new();
/// The app's own `models/` resource folder
static BUNDLED: OnceLock<PathBuf> = OnceLock::new();

/// Models that came with the app: the bundle; in debug builds and tests, the folder
/// scripts/fetch-models.mjs writes to instead (a debug build's resource folder,
/// target/debug, keeps the models of older builds that bundled them)
fn bundled_dirs() -> Vec<PathBuf> {
    if cfg!(debug_assertions) || cfg!(test) {
        vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models")]
    } else {
        BUNDLED.get().cloned().into_iter().collect()
    }
}

/// A debug build's local pack server (FLIPFILE_PACKS_URL), to try downloads with
fn test_server() -> Option<String> {
    if !cfg!(debug_assertions) || cfg!(test) {
        return None;
    }
    std::env::var("FLIPFILE_PACKS_URL").ok().filter(|url| url.starts_with("http://127.0.0.1:"))
}

/// Where a model (or a file the app ships, like the fonts) is: downloaded first, as a
/// pack may be newer than what came with the app. A Core ML model is a folder, an ONNX
/// model a file.
pub(crate) fn model_path(file: &str) -> Option<PathBuf> {
    let present = |path: &PathBuf| if file.ends_with(".mlmodelc") { path.join("coremldata.bin").is_file() } else { path.is_file() };
    // Trying downloads (FLIPFILE_PACKS_URL): a pack's models only as downloaded, the
    // fonts and words where they always are
    let bundled = if test_server().is_some() && Catalog::built_in().pack_of(file).is_some() { vec![] } else { bundled_dirs() };
    DOWNLOADED.get().cloned().into_iter().chain(bundled).map(|dir| dir.join(file)).find(present)
}

/// What to tell someone whose model isn't there: which pack to download
pub(crate) fn missing(file: &str) -> String {
    match Catalog::built_in().pack_of(file) {
        Some(pack) => format!("{} isn't downloaded yet: get it in Workshop", pack.name),
        None => "The AI models are missing. Please reinstall flipfile.".into(),
    }
}

/// Called once at startup, before any model is loaded
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().resource_dir() {
        let _ = BUNDLED.set(dir.join("models"));
    }
    let folder = if test_server().is_some() { "models-test" } else { "models" };
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("flipfile")).join(folder);
    let _ = DOWNLOADED.set(dir.clone());
    let key = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|key| key.as_str())
        .map(str::to_string);
    let bundled = if test_server().is_some() { vec![] } else { bundled_dirs() };
    let mut packs = Packs::new(Store { dir, bundled }, key);
    if let Some(url) = test_server() {
        packs.catalog.get_mut().unwrap_or_else(|e| e.into_inner()).base = url;
        packs.key = None;
    }
    packs.store.finish_pending();
    // A debug build's resource folder may hold models from an older build: never adopted
    if let Some(bundle) = BUNDLED.get().filter(|_| !cfg!(debug_assertions)).cloned() {
        let (store, built_in) = (packs.store.clone(), packs.built_in.clone());
        std::thread::spawn(move || {
            for pack in &built_in.packs {
                if let Err(e) = store.adopt(pack, &built_in, &bundle) {
                    eprintln!("Could not keep the bundled {}: {e}", pack.name);
                }
            }
        });
    }
    app.manage(packs);
}

pub struct Packs {
    store: Store,
    built_in: Catalog,
    /// The newest catalog known: built in, or a newer signed one
    catalog: Mutex<Catalog>,
    /// The updater's public key, which signs catalogs
    key: Option<String>,
    /// Packs downloading now
    active: Mutex<HashMap<String, Arc<Active>>>,
    /// When the published catalog was last looked at
    checked: Mutex<Option<Instant>>,
}

#[derive(Default)]
struct Active {
    cancel: AtomicBool,
    done: AtomicU64,
    total: AtomicU64,
}

/// A pack as Workshop and the ring show it
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackStatus {
    pub id: String,
    pub name: String,
    /// The ring targets that need it
    pub targets: Vec<String>,
    /// "missing", "installed", "update" (a newer version is out), "downloading", or
    /// "restart" (downloaded, put in place at the next start)
    pub state: &'static str,
    /// The whole pack here, in bytes
    pub size: u64,
    /// What is left to download for it
    pub download: u64,
    /// While downloading: bytes done of the total
    pub done: u64,
    pub total: u64,
    /// It has downloaded models that can be removed (an app that still bundles them keeps
    /// them, so there is nothing to free)
    pub removable: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress<'a> {
    id: &'a str,
    done: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Failed<'a> {
    id: &'a str,
    message: String,
}

/// The published catalog is looked at no more often than this
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

impl Packs {
    fn new(store: Store, key: Option<String>) -> Packs {
        let built_in = Catalog::built_in();
        let cached = key.as_deref().and_then(|key| cached_catalog(&store, key));
        let catalog = cached.filter(|c| c.serial > built_in.serial).unwrap_or_else(|| built_in.clone());
        Packs { store, built_in, catalog: Mutex::new(catalog), key, active: Mutex::default(), checked: Mutex::default() }
    }

    fn catalog(&self) -> Catalog {
        self.catalog.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn status(&self) -> Vec<PackStatus> {
        let catalog = self.catalog();
        let installed = self.store.installed();
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        catalog
            .packs
            .iter()
            .map(|pack| {
                let presence = self.store.presence(pack, &self.built_in, &installed);
                let download = self.store.to_download(pack, &self.built_in, &installed).iter().map(|f| f.size).sum();
                let waiting = pack.models().iter().any(|m| self.store.waiting(m));
                let running = active.get(&pack.id);
                let state = match (running, presence) {
                    (Some(_), _) => "downloading",
                    _ if waiting => "restart",
                    (None, Presence::Current) => "installed",
                    (None, Presence::Outdated) => "update",
                    (None, Presence::Missing) => "missing",
                };
                PackStatus {
                    id: pack.id.clone(),
                    name: pack.name.clone(),
                    targets: pack.targets.clone(),
                    state,
                    size: pack.size(),
                    download,
                    done: running.map_or(0, |a| a.done.load(Ordering::Relaxed)),
                    total: running.map_or(0, |a| a.total.load(Ordering::Relaxed)),
                    removable: pack.models().iter().any(|m| self.store.dir.join(m).exists()) && !self.store.bundles(pack),
                }
            })
            .collect()
    }

    /// Reads the published catalog, when it hasn't been for a while: a newer one, signed,
    /// replaces the one in use. Offline or not yet published, nothing changes.
    async fn refresh(&self) {
        let Some(key) = self.key.clone() else { return };
        {
            let mut checked = self.checked.lock().unwrap_or_else(|e| e.into_inner());
            if checked.is_some_and(|at| at.elapsed() < CHECK_EVERY) {
                return;
            }
            *checked = Some(Instant::now());
        }
        let url = format!("{}/catalog-{FORMAT}.json", self.built_in.base.trim_end_matches('/'));
        let Ok(Some((json, signature))) = fetch_catalog(&url).await else { return };
        if catalog::verify(&json, &signature, &key).is_err() {
            return;
        }
        let Ok(catalog) = Catalog::parse(&json) else { return };
        let mut current = self.catalog.lock().unwrap_or_else(|e| e.into_inner());
        if catalog.serial > current.serial {
            let _ = std::fs::create_dir_all(&self.store.dir);
            let _ = std::fs::write(self.store.dir.join("catalog.json"), &json);
            let _ = std::fs::write(self.store.dir.join("catalog.json.sig"), &signature);
            *current = catalog;
        }
    }
}

/// The catalog saved by an earlier `refresh`, checked again
fn cached_catalog(store: &Store, key: &str) -> Option<Catalog> {
    let json = std::fs::read(store.dir.join("catalog.json")).ok()?;
    let signature = std::fs::read_to_string(store.dir.join("catalog.json.sig")).ok()?;
    catalog::verify(&json, &signature, key).ok()?;
    Catalog::parse(&json).ok()
}

/// A published catalog and its signature; None when there is none (404)
async fn fetch_catalog(url: &str) -> Result<Option<(Vec<u8>, String)>, String> {
    let client = download::client()?;
    let get = |url: String| {
        let client = client.clone();
        async move {
            let response = client.get(&url).timeout(Duration::from_secs(15)).send().await.map_err(|e| download::cause(&e))?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let response = response.error_for_status().map_err(|e| e.to_string())?;
            // A catalog is a few hundred kilobytes at most
            if response.content_length().is_some_and(|n| n > 4 << 20) {
                return Err("the catalog is unexpectedly large".to_string());
            }
            response.bytes().await.map(|b| Some(b.to_vec())).map_err(|e| e.to_string())
        }
    };
    let Some(json) = get(url.to_string()).await? else { return Ok(None) };
    let Some(signature) = get(format!("{url}.sig")).await? else { return Ok(None) };
    Ok(Some((json, String::from_utf8(signature).map_err(|e| e.to_string())?)))
}

/// Downloads what `id` still needs and puts it in place; true when it is all in place,
/// false when a model waits for the next start
async fn install(packs: &Packs, id: &str, active: &Active, on_progress: &(dyn Fn(u64, u64) + Sync)) -> Result<bool, Failure> {
    let catalog = packs.catalog();
    let pack = catalog.pack(id).cloned().ok_or_else(|| format!("no pack \"{id}\""))?;
    let store = &packs.store;
    let files: Vec<File> = store.to_download(&pack, &packs.built_in, &store.installed()).into_iter().cloned().collect();
    let total = files.iter().map(|f| f.size).sum();
    active.total.store(total, Ordering::Relaxed);

    if !files.is_empty() {
        let client = download::client()?;
        let downloads = store.downloads();
        std::fs::create_dir_all(&downloads).map_err(|e| download::save_error(&e))?;
        let mut before = 0;
        for file in &files {
            let part = downloads.join(format!("{}.part", file.sha256));
            let mut progress = |have: u64| {
                active.done.store(before + have, Ordering::Relaxed);
                on_progress(before + have, total);
            };
            download::fetch(&client, &catalog.url(file), file, &part, &active.cancel, &mut progress).await?;
            std::fs::rename(&part, downloads.join(&file.sha256)).map_err(|e| download::save_error(&e))?;
            before += file.size;
        }
    }
    if active.cancel.load(Ordering::Relaxed) {
        return Err(Failure::Cancelled);
    }
    // Models in use let go first: on Windows they can't be replaced while loaded
    crate::platform::upscale::release();
    crate::platform::whisper::release();
    let built_in = packs.built_in.clone();
    let store = store.clone();
    tauri::async_runtime::spawn_blocking(move || store.assemble(&pack, &built_in))
        .await
        .map_err(|e| e.to_string())?
        .map_err(Failure::Failed)
}

/// Workshop and the ring: every pack and where it is. `refresh` also looks for newer
/// versions (at most every few hours).
#[tauri::command]
pub async fn packs_status(app: AppHandle, refresh: bool) -> Result<Vec<PackStatus>, String> {
    let packs = app.state::<Packs>();
    if refresh {
        packs.refresh().await;
    }
    Ok(packs.status())
}

/// Starts downloading pack `id` (or its newer version). Progress comes as `pack-progress`
/// events, every change as a `packs` event with the new status, a failure as `pack-failed`.
#[tauri::command]
pub fn pack_install(app: AppHandle, id: String) -> Result<(), String> {
    let packs = app.state::<Packs>();
    if packs.catalog().pack(&id).is_none() {
        return Err(format!("no pack \"{id}\""));
    }
    let active = Arc::new(Active::default());
    {
        let mut running = packs.active.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains_key(&id) {
            return Ok(());
        }
        running.insert(id.clone(), active.clone());
    }
    let _ = app.emit("packs", packs.status());
    tauri::async_runtime::spawn(async move {
        let packs = app.state::<Packs>();
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let on_progress = |done: u64, total: u64| {
            let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
            if last.elapsed() >= Duration::from_millis(100) || done == total {
                *last = Instant::now();
                let _ = app.emit("pack-progress", Progress { id: &id, done, total });
            }
        };
        let result = install(&packs, &id, &active, &on_progress).await;
        packs.active.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        if let Err(Failure::Failed(message) | Failure::Dropped(message)) = result {
            let _ = app.emit("pack-failed", Failed { id: &id, message });
        }
        let _ = app.emit("packs", packs.status());
    });
    Ok(())
}

/// Stops downloading pack `id`; what is downloaded so far is kept for next time
#[tauri::command]
pub fn pack_cancel(app: AppHandle, id: String) {
    if let Some(active) = app.state::<Packs>().active.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        active.cancel.store(true, Ordering::Relaxed);
    }
}

/// Removes pack `id`'s downloaded models, to free the space
#[tauri::command]
pub async fn pack_remove(app: AppHandle, id: String) -> Result<Vec<PackStatus>, String> {
    let packs = app.state::<Packs>();
    if packs.active.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id) {
        return Err("it is downloading: stop it first".into());
    }
    let catalog = packs.catalog();
    let pack = catalog.pack(&id).cloned().ok_or_else(|| format!("no pack \"{id}\""))?;
    crate::platform::upscale::release();
    crate::platform::whisper::release();
    let store = packs.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.remove(&pack, &catalog)).await.map_err(|e| e.to_string())??;
    let status = packs.status();
    let _ = app.emit("packs", status.clone());
    Ok(status)
}
