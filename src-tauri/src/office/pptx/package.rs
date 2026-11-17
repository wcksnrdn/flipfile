//! Package: parts and their relationships

use super::*;

/// The parts of the package, unpacked when asked for, within a budget: a small
/// hostile file cannot expand into gigabytes
pub(super) struct Package {
    pub(super) zip: std::cell::RefCell<zip::ZipArchive<Cursor<Vec<u8>>>>,
    /// Part name (without a leading slash) -> its index in the archive
    pub(super) names: HashMap<String, usize>,
    pub(super) budget: std::cell::Cell<u64>,
}

/// Unpacked bytes one presentation may use, all parts together
const UNPACK_BUDGET: u64 = 1 << 30;

impl Package {
    pub(super) fn open(bytes: &[u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).map_err(|_| "this is not a PowerPoint (.pptx) file".to_string())?;
        let mut names = HashMap::new();
        for i in 0..zip.len() {
            let Ok(file) = zip.by_index_raw(i) else { continue };
            if file.is_dir() {
                continue;
            }
            if let Ok(name) = file.name() {
                names.insert(name.trim_start_matches('/').to_string(), i);
            }
        }
        Ok(Package { zip: std::cell::RefCell::new(zip), names, budget: std::cell::Cell::new(UNPACK_BUDGET) })
    }

    pub(super) fn get(&self, name: &str) -> Option<Vec<u8>> {
        let index = *self.names.get(name)?;
        let mut zip = self.zip.borrow_mut();
        let file = zip.by_index(index).ok()?;
        let left = self.budget.get();
        // Honest sizes beyond the budget are refused before anything is unpacked
        if file.size() > left {
            return None;
        }
        let mut data = Vec::new();
        file.take(left + 1).read_to_end(&mut data).ok()?;
        if data.len() as u64 > left {
            self.budget.set(0);
            return None;
        }
        self.budget.set(left - data.len() as u64);
        Some(data)
    }

    pub(super) fn text(&self, name: &str) -> Option<String> {
        let text = self.get(name).map(|b| String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))?;
        // Nested too deep to parse safely: as good as missing
        (!crate::office::xml_too_deep(&text)).then_some(text)
    }

    /// The part's relationships: id -> (type, target path)
    pub(super) fn rels(&self, part: &str) -> HashMap<String, (String, String)> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels_path = if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") };
        let mut out = HashMap::new();
        let Some(xml) = self.text(&rels_path) else { return out };
        let Ok(doc) = Document::parse(&xml) else { return out };
        for rel in doc.descendants().filter(|n| n.tag_name().name() == "Relationship") {
            let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) else { continue };
            if rel.attribute("TargetMode") == Some("External") {
                continue;
            }
            let kind = rel.attribute("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
            out.insert(id.to_string(), (kind, resolve(dir, target)));
        }
        out
    }
}

/// `../media/image1.png` relative to `ppt/slides` -> `ppt/media/image1.png`
pub(super) fn resolve(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { vec![] } else { dir.split('/').filter(|p| !p.is_empty()).collect() };
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}
