// Reads a Word document (.docx, Office Open XML) into a small layout model:
// page setup, paragraphs with resolved formatting, lists, tables, images, header and
// footer. Styles are resolved the way Word does it: document defaults, then the
// paragraph style chain, then character styles, then direct formatting.
//
// Anything not understood is skipped rather than failing the whole document.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use roxmltree::{Document as Xml, Node};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const V: &str = "urn:schemas-microsoft-com:vml";
const M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Refuses absurd inputs (zip bombs, gigantic embedded media).
const MAX_PART_BYTES: u64 = 200 * 1024 * 1024;


// The reader's parts. The submodules share one namespace (each starts with
// `use super::*`), as when this was a single file.
mod body;
mod fields;
mod model;
mod package;
mod props;
mod reader;
mod tables;
mod xml;

#[cfg(test)]
mod tests;

use fields::*;
use package::*;
use props::*;
use reader::*;
use xml::*;

// The document model `read` returns, and the numbering helpers the other writers share
pub use fields::{bookmark_label, format_number};
pub(crate) use fields::bullet_char;
pub use model::*;

pub fn read(bytes: &[u8]) -> Result<Document, String> {
    let pkg = Package::open(bytes)?;
    // The main document part is usually word/document.xml; follow the package rels to be sure
    let main = main_part(&pkg).ok_or("this file has no Word document inside")?;
    let text = pkg.text(&main).ok_or("this file has no Word document inside")?;
    let xml = Xml::parse(&text).map_err(|_| "the document text is damaged".to_string())?;
    let body = w(xml.root_element(), "body").ok_or("the document has no body")?;
    let rels = relationships(&pkg, &main);

    let mut reader = Reader::new(&pkg);
    let settings = pkg.text("word/settings.xml");
    let settings_xml = settings.as_deref().and_then(|t| Xml::parse(t).ok());
    let even_odd = settings_xml
        .as_ref()
        .is_some_and(|x| x.descendants().any(|n| n.tag_name().name() == "evenAndOddHeaders" && toggle(n)));
    let default_tab = settings_xml
        .as_ref()
        .and_then(|x| x.descendants().find(|n| n.tag_name().name() == "defaultTabStop").and_then(val).and_then(twips))
        .filter(|v| *v > 1.0)
        .unwrap_or(36.0);
    // No compatibility mode at all: an old document (Word treats it like Word 2007)
    let compat_mode = settings_xml
        .as_ref()
        .and_then(|x| {
            x.descendants()
                .find(|n| n.tag_name().name() == "compatSetting" && wattr(*n, "name") == Some("compatibilityMode"))
                .and_then(|n| wattr(n, "val"))
                .and_then(|v| v.parse::<u32>().ok())
        })
        .unwrap_or(12);
    reader.legacy_tables = compat_mode < 15;

    // A paragraph carrying a <w:sectPr> ends a section; the body's own sectPr is the last one
    let mut parts: Vec<(Vec<Block>, Option<Node>)> = vec![];
    let mut current: Vec<Block> = vec![];
    for child in body.children().filter(|c| c.is_element() && c.tag_name().namespace() == Some(W)) {
        match child.tag_name().name() {
            "p" => {
                reader.paragraph(child, &rels, &mut current);
                if let Some(sect) = w(child, "pPr").and_then(|p| w(p, "sectPr")) {
                    parts.push((std::mem::take(&mut current), Some(sect)));
                }
            }
            "tbl" => current.push(Block::Table(reader.table(child, &rels))),
            "sectPr" => parts.push((std::mem::take(&mut current), Some(child))),
            "sdt" | "customXml" | "ins" | "smartTag" => {
                let container = if child.tag_name().name() == "sdt" { w(child, "sdtContent") } else { Some(child) };
                if let Some(c) = container {
                    current.extend(reader.blocks(c, &rels));
                }
            }
            _ => {}
        }
    }
    if !current.is_empty() || parts.is_empty() {
        parts.push((current, None));
    }

    // Headers/footers are inherited per type from the previous section, as in Word
    let mut inherited: HashMap<(&str, &str), String> = HashMap::new();
    let mut cache: HashMap<String, Vec<Block>> = HashMap::new();
    let mut sections = vec![];
    for (blocks, sect) in parts {
        if let Some(sect) = sect {
            for kind in ["header", "footer"] {
                let tag = format!("{kind}Reference");
                for r in sect.children().filter(|c| c.tag_name().name() == tag) {
                    let which = match wattr(r, "type").unwrap_or("default") {
                        "first" => "first",
                        "even" => "even",
                        _ => "default",
                    };
                    if let Some(target) = r.attribute((R, "id")).and_then(|id| rels.get(id)) {
                        inherited.insert((kind, which), target.clone());
                    }
                }
            }
        }
        let title_page = sect.and_then(|s| w(s, "titlePg")).is_some_and(toggle);
        let mut load = |kind: &'static str, which: &'static str| -> Vec<Block> {
            let Some(target) = inherited.get(&(kind, which)).cloned() else { return vec![] };
            if let Some(b) = cache.get(&target) {
                return b.clone();
            }
            let b = reader.part_blocks(&target);
            cache.insert(target, b.clone());
            b
        };
        let mut hf = |kind: &'static str| HeaderFooter {
            default: load(kind, "default"),
            first: title_page.then(|| load(kind, "first")),
            even: even_odd.then(|| load(kind, "even")),
        };
        let header = hf("header");
        let footer = hf("footer");
        sections.push(Section {
            page: reader.page_setup(sect),
            header,
            footer,
            blocks,
            start: sect.map(Reader::section_start).unwrap_or(SectionStart::NextPage),
            page_start: sect.and_then(|s| w(s, "pgNumType")).and_then(|n| wattr(n, "start")).and_then(|v| v.parse().ok()),
        });
    }

    let (_, style_run) = reader.para_style_patches(None);
    let base = reader.base_run();
    let default_run = style_run.apply(&base, &reader.theme);
    let lang = document_language(&pkg);

    Ok(Document {
        sections,
        images: reader.images,
        default_font: default_run.font.unwrap_or_else(|| "Calibri".into()),
        default_size: default_run.size,
        lang,
        default_tab,
        legacy_layout: compat_mode < 15,
        justify_breaks: !settings_xml.as_ref().is_some_and(|x| x.descendants().any(|n| n.tag_name().name() == "doNotExpandShiftReturn" && toggle(n))),
        bookmarks: std::mem::take(&mut reader.bookmarks),
    })
}

fn main_part(pkg: &Package) -> Option<String> {
    let text = pkg.text("_rels/.rels")?;
    let xml = Xml::parse(&text).ok()?;
    xml.descendants()
        .filter(|n| n.tag_name().name() == "Relationship")
        .find(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/officeDocument")))
        .and_then(|n| n.attribute("Target"))
        .map(|t| resolve_path("", t))
        .filter(|p| pkg.files.contains_key(p))
        .or_else(|| pkg.files.contains_key("word/document.xml").then(|| "word/document.xml".to_string()))
}

fn document_language(pkg: &Package) -> Option<String> {
    let text = pkg.text("word/styles.xml")?;
    let xml = Xml::parse(&text).ok()?;
    xml.descendants()
        .find(|n| n.tag_name().name() == "lang" && n.tag_name().namespace() == Some(W))
        .and_then(val)
        .and_then(|l| l.split('-').next())
        .map(|l| l.to_lowercase())
        .filter(|l| l.len() == 2)
}
