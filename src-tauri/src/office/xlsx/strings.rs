//! Shared strings and their rich text runs

use super::*;

/// Shared strings: the plain text, and its runs when it mixes fonts
pub(super) fn shared_strings(xml: &str, colors: &Colors) -> Vec<(String, Option<Rich>)> {
    let Ok(doc) = Document::parse(xml) else { return vec![] };
    doc.root_element().children().filter(|n| n.has_tag_name((MAIN, "si"))).map(|si| (rich_text(si), runs(si, colors))).collect()
}

/// The runs of an `<si>` / `<is>` that sets fonts of its own (None for plain text)
pub(super) fn runs(n: Node, colors: &Colors) -> Option<Rich> {
    let rs: Vec<Node> = children(n, "r").collect();
    if rs.is_empty() || !rs.iter().any(|r| child(*r, "rPr").is_some()) {
        return None;
    }
    let out: Vec<RichRun> = rs
        .iter()
        .map(|r| {
            let text = child(*r, "t").and_then(|t| t.text()).unwrap_or("").to_string();
            let font = child(*r, "rPr").map(|p| {
                let val = |name: &str| child(p, name);
                let on = |name: &str| val(name).map(|n| n.attribute("val").map(|v| v != "0" && v != "false").unwrap_or(true));
                RunFont {
                    name: val("rFont").and_then(|n| n.attribute("val")).map(str::to_string),
                    size: val("sz").and_then(|n| attr_f32(n, "val")),
                    bold: on("b"),
                    italic: on("i"),
                    underline: val("u").map(|n| n.attribute("val") != Some("none")),
                    strike: on("strike"),
                    color: val("color").and_then(|c| colors.get(c)),
                    script: match val("vertAlign").and_then(|n| n.attribute("val")) {
                        Some("superscript") => 1,
                        Some("subscript") => -1,
                        _ => 0,
                    },
                }
            });
            RichRun { text, font: font.unwrap_or_default() }
        })
        .collect();
    Some(Arc::new(out))
}

/// The text of an `<si>` / `<is>`: its `<t>` runs, without phonetic hints
pub(super) fn rich_text(n: Node) -> String {
    let mut s = String::new();
    for t in n.descendants().filter(|d| d.has_tag_name((MAIN, "t"))) {
        if t.ancestors().any(|a| a.has_tag_name((MAIN, "rPh"))) {
            continue;
        }
        s.push_str(t.text().unwrap_or(""));
    }
    s
}
