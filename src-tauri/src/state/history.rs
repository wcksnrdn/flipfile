// Recent conversions, persisted as JSON in the app data folder so the tray menu
// can list them and undo the last batch across restarts.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 50;

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub output: String,
    pub batch: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    entries: Vec<Entry>, // newest first
    next_batch: u64,
}

pub struct History {
    data: Stored,
    file: Option<PathBuf>,
}

impl History {
    pub fn load(file: Option<PathBuf>) -> Self {
        let data = file
            .as_ref()
            .and_then(|f| fs::read(f).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { data, file }
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.data) {
            let _ = fs::write(file, json);
        }
    }

    /// Records one conversion batch (all outputs of a single drop).
    pub fn record(&mut self, outputs: &[String]) {
        if outputs.is_empty() {
            return;
        }
        let batch = self.data.next_batch;
        self.data.next_batch += 1;
        for output in outputs.iter().rev() {
            self.data.entries.insert(0, Entry { output: output.clone(), batch });
        }
        self.data.entries.truncate(MAX_ENTRIES);
        self.save();
    }

    /// Most recent outputs that still exist on disk.
    pub fn recent(&self, limit: usize) -> Vec<String> {
        self.data
            .entries
            .iter()
            .filter(|e| Path::new(&e.output).exists())
            .take(limit)
            .map(|e| e.output.clone())
            .collect()
    }

    /// Outputs of the newest batch that still exist on disk.
    pub fn last_batch(&self) -> Vec<String> {
        let Some(first) = self.data.entries.first() else { return vec![] };
        self.data
            .entries
            .iter()
            .filter(|e| e.batch == first.batch && Path::new(&e.output).exists())
            .map(|e| e.output.clone())
            .collect()
    }

    pub fn forget_last_batch(&mut self) {
        if let Some(batch) = self.data.entries.first().map(|e| e.batch) {
            self.data.entries.retain(|e| e.batch != batch);
            self.save();
        }
    }

    pub fn clear(&mut self) {
        self.data.entries.clear();
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_batches_and_undoes_the_newest() {
        let dir = std::env::temp_dir().join(format!("flipfile-history-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = |n: &str| {
            let p = dir.join(n);
            fs::write(&p, b"x").unwrap();
            p.to_string_lossy().into_owned()
        };
        let (a, b, c) = (file("a.png"), file("b.png"), file("c.png"));
        let store = dir.join("history.json");

        let mut h = History::load(Some(store.clone()));
        h.record(&[a.clone()]);
        h.record(&[b.clone(), c.clone()]);
        assert_eq!(h.recent(10), vec![b.clone(), c.clone(), a.clone()]);
        assert_eq!(h.last_batch(), vec![b.clone(), c.clone()]);

        // Persisted across restarts
        let mut h = History::load(Some(store));
        h.forget_last_batch();
        assert_eq!(h.last_batch(), vec![a.clone()]);

        // Files deleted outside the app are skipped
        fs::remove_file(&a).unwrap();
        assert!(h.recent(10).is_empty());
        assert!(h.last_batch().is_empty());
    }
}
