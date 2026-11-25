// The catalog of AI feature packs (packs/catalog.json, written by scripts/packs.mjs): the
// files each pack holds, with their SHA-256 and size. The copy built into the app is the
// floor; a newer one, signed with the updater's key, comes from the `packs` release, so
// a better model reaches people without a new flipfile.

use std::collections::BTreeMap;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

/// The catalog format this build's code runs (FORMAT in scripts/packs.mjs). A catalog of
/// another format is for another build and is never used.
pub const FORMAT: u32 = 1;

const BUILT_IN: &str = include_str!("../../packs/catalog.json");

/// This system's models: Core ML on a Mac, ONNX elsewhere
pub const SYSTEM: &str = if cfg!(target_os = "macos") { "coreml" } else { "onnx" };

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Catalog {
    pub format: u32,
    /// Goes up by one with every published change; the highest one known is used
    pub serial: u32,
    /// Where the files are: each at `<base>/<sha256>`
    pub base: String,
    pub packs: Vec<Pack>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    /// The ring targets that need it
    pub targets: Vec<String>,
    /// Its files for each system ("coreml", "onnx"), or for every system ("all")
    pub files: BTreeMap<String, Vec<File>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct File {
    /// Relative to the models folder: "upscale.mlmodelc/weights/weight.bin"
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

impl File {
    /// The model it belongs to (a Core ML model is a folder): what is installed as a whole
    pub fn model(&self) -> &str {
        model_of(&self.path)
    }
}

/// The model a path in the models folder belongs to: its first component
pub fn model_of(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

impl Pack {
    /// Its files on this system
    pub fn files(&self) -> impl Iterator<Item = &File> {
        self.files.get(SYSTEM).into_iter().chain(self.files.get("all")).flatten()
    }

    /// Its models on this system, each once, in order
    pub fn models(&self) -> Vec<&str> {
        let mut models: Vec<&str> = vec![];
        for file in self.files() {
            if !models.contains(&file.model()) {
                models.push(file.model());
            }
        }
        models
    }

    /// What it takes to download, in bytes
    pub fn size(&self) -> u64 {
        self.files().map(|f| f.size).sum()
    }
}

impl Catalog {
    pub fn built_in() -> Catalog {
        Catalog::parse(BUILT_IN.as_bytes()).expect("packs/catalog.json is a valid catalog")
    }

    /// A catalog, checked: of this build's format, with only safe paths and real hashes
    pub fn parse(json: &[u8]) -> Result<Catalog, String> {
        let catalog: Catalog = serde_json::from_slice(json).map_err(|e| format!("not a catalog ({e})"))?;
        if catalog.format != FORMAT {
            return Err(format!("a catalog of format {}, not {FORMAT}", catalog.format));
        }
        if !catalog.base.starts_with("https://") {
            return Err("a catalog whose files are not on HTTPS".into());
        }
        for file in catalog.packs.iter().flat_map(|p| p.files.values().flatten()) {
            if !safe_path(&file.path) {
                return Err(format!("a catalog with an unsafe path: {}", file.path));
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(format!("a catalog with a bad hash for {}", file.path));
            }
        }
        Ok(catalog)
    }

    pub fn url(&self, file: &File) -> String {
        format!("{}/{}", self.base.trim_end_matches('/'), file.sha256)
    }

    pub fn pack(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
    }

    /// The pack a model (or a file of one) belongs to on this system
    pub fn pack_of(&self, path: &str) -> Option<&Pack> {
        let model = model_of(path);
        self.packs.iter().find(|p| p.files().any(|f| f.model() == model))
    }

    /// The file at `path` on this system, as this catalog lists it
    pub fn file(&self, path: &str) -> Option<&File> {
        self.packs.iter().flat_map(Pack::files).find(|f| f.path == path)
    }
}

/// A relative path that stays inside the models folder: plain names, none hidden
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| !part.is_empty() && !part.starts_with('.'))
}

/// Checks `data` against a minisign signature made with the updater's key; both come
/// base64-encoded, as `tauri signer sign` writes them and tauri.conf.json holds the key
pub fn verify(data: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let decode = |text: &str| {
        let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    };
    let key = PublicKey::decode(&decode(public_key)?).map_err(|e| format!("bad key ({e})"))?;
    let signature = Signature::decode(&decode(signature)?).map_err(|e| format!("bad signature ({e})"))?;
    key.verify(data, &signature, true).map_err(|e| format!("signature does not match ({e})"))
}
