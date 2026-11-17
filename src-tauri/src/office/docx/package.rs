
use super::*;

// Package (zip) access

pub(super) struct Package {
    pub(super) files: HashMap<String, Vec<u8>>,
}

impl Package {
    pub(super) fn open(bytes: &[u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "not a valid Word document".to_string())?;
        let mut files = HashMap::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let Ok(name) = entry.name().map(|n| n.to_string()) else { continue };
            // Only what the converter reads: XML parts and media
            if !(name.ends_with(".xml") || name.ends_with(".rels") || name.starts_with("word/media/")) {
                continue;
            }
            let mut data = Vec::new();
            (&mut entry).take(MAX_PART_BYTES + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
            total += data.len() as u64;
            if data.len() as u64 > MAX_PART_BYTES || total > 4 * MAX_PART_BYTES {
                return Err("the document is too large".into());
            }
            files.insert(name, data);
        }
        Ok(Package { files })
    }

    pub(super) fn text(&self, name: &str) -> Option<String> {
        let text = self.files.get(name).map(|b| String::from_utf8_lossy(b).into_owned())?;
        // Nested too deep to parse safely: as good as missing
        (!crate::office::xml_too_deep(&text)).then_some(text)
    }
}

/// Relationship id -> target path inside the package.
pub(super) fn relationships(pkg: &Package, part: &str) -> HashMap<String, String> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_name = format!("{dir}/_rels/{file}.rels");
    let mut map = HashMap::new();
    let Some(text) = pkg.text(&rels_name) else { return map };
    let Ok(xml) = Xml::parse(&text) else { return map };
    for rel in xml.descendants().filter(|n| n.tag_name().name() == "Relationship") {
        if let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) {
            if rel.attribute("TargetMode") == Some("External") {
                continue; // never follow external links
            }
            map.insert(id.to_string(), resolve_path(dir, target));
        }
    }
    map
}

pub(super) fn resolve_path(dir: &str, target: &str) -> String {
    let joined = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("{dir}/{target}") };
    let mut parts: Vec<&str> = vec![];
    for p in joined.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

// ---------------------------------------------------------------------------
