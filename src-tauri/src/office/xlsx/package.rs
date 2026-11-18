//! Package helpers

use super::*;

pub(super) struct Package {
    pub(super) zip: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Package {
    pub(super) fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let path = path.trim_start_matches('/');
        let mut f = self.zip.by_name(path).ok()?;
        // a corrupt or hostile file must not exhaust memory
        if f.size() > 512 * 1024 * 1024 {
            return None;
        }
        let mut out = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut out).ok()?;
        Some(out)
    }
    pub(super) fn text(&mut self, path: &str) -> Option<String> {
        let text = String::from_utf8(self.read(path)?).ok()?;
        // Nested too deep to parse safely: as good as missing
        (!crate::office::xml_too_deep(&text)).then_some(text)
    }
    /// Relationships of a part: (id, type, resolved target path), internal ones only
    pub(super) fn rels_list(&mut self, part: &str) -> Vec<(String, String, String)> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels_path = format!("{dir}/_rels/{file}.rels");
        let mut out = vec![];
        let Some(xml) = self.text(&rels_path) else { return out };
        let Ok(doc) = Document::parse(&xml) else { return out };
        for r in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
            if let (Some(id), Some(target)) = (r.attribute("Id"), r.attribute("Target")) {
                if r.attribute("TargetMode") == Some("External") {
                    continue;
                }
                out.push((id.to_string(), r.attribute("Type").unwrap_or("").to_string(), resolve(dir, target)));
            }
        }
        out
    }
    /// Relationships of a part: id -> resolved target path
    pub(super) fn rels(&mut self, part: &str) -> HashMap<String, String> {
        self.rels_list(part).into_iter().map(|(id, _, t)| (id, t)).collect()
    }
}

/// `xl/worksheets` + `../media/a.png` -> `xl/media/a.png`
pub(super) fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub(super) fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}
pub(super) fn children<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| c.tag_name().name() == name)
}
pub(super) fn attr_f32(n: Node, name: &str) -> Option<f32> {
    n.attribute(name).and_then(|v| v.parse().ok())
}
pub(super) fn attr_u32(n: Node, name: &str) -> Option<u32> {
    n.attribute(name).and_then(|v| v.parse().ok())
}
pub(super) fn attr_bool(n: Node, name: &str) -> Option<bool> {
    n.attribute(name).map(|v| v == "1" || v == "true")
}

/// "AB12" -> (row 11, col 27)
pub fn cell_ref(r: &str) -> Option<(u32, u32)> {
    let r = r.trim_start_matches('$');
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = r[letters.len()..].trim_start_matches('$');
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut col = 0u32;
    for c in letters.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = digits.parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

pub(super) fn col_letters(s: &str) -> Option<u32> {
    let s = s.trim_start_matches('$');
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut col = 0u32;
    for c in s.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    Some(col - 1)
}

/// "A1:C9" (also "$A$1:$C$9", or a single "B4")
pub fn range_ref(r: &str) -> Option<Range> {
    let (a, b) = r.split_once(':').unwrap_or((r, r));
    let (r1, c1) = cell_ref(a)?;
    let (r2, c2) = cell_ref(b)?;
    Some((r1.min(r2), c1.min(c2), r1.max(r2), c1.max(c2)))
}
