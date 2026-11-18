//! Pictures

use super::*;

/// A chart waiting for its cells: (its place among `drawn`, where it sits, its XML)
pub type ChartPart = (usize, Anchor, String);

/// A sheet's drawing layer: pictures, charts (their XML, read once the cells are worked
/// out) and shapes
pub(super) fn read_drawing(pkg: &mut Package, sheet_path: &str, sheet_xml: &str, theme: &[Rgb]) -> (Vec<Picture>, Vec<ChartPart>, Vec<Drawn>) {
    let none = || (vec![], vec![], vec![]);
    let Ok(doc) = Document::parse(sheet_xml) else { return none() };
    let Some(drawing) = doc.root_element().children().find(|n| n.tag_name().name() == "drawing") else { return none() };
    let Some(rid) = drawing.attribute((REL, "id")) else { return none() };
    let rels = pkg.rels(sheet_path);
    let Some(dpath) = rels.get(rid).cloned() else { return none() };
    let Some(dxml) = pkg.text(&dpath) else { return none() };
    let drels = pkg.rels(&dpath);
    let Ok(ddoc) = Document::parse(&dxml) else { return none() };
    let (mut pictures, mut charts, mut drawn) = (vec![], vec![], vec![]);
    for anchor in ddoc.root_element().children().filter(|n| n.is_element()) {
        let kind = anchor.tag_name().name();
        if !matches!(kind, "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor") {
            continue;
        }
        let marker = |name: &str| -> Option<(u32, u32, f32, f32)> {
            let m = child(anchor, name)?;
            let num = |n: &str| child(m, n).and_then(|x| x.text()).and_then(|t| t.trim().parse::<f64>().ok());
            Some((num("row")? as u32, num("col")? as u32, num("rowOff").unwrap_or(0.0) as f32 * EMU, num("colOff").unwrap_or(0.0) as f32 * EMU))
        };
        let ext_size = anchor
            .descendants()
            .find(|n| n.has_tag_name((DRAW, "ext")) || n.has_tag_name((XDR, "ext")))
            .and_then(|e| Some((attr_f32(e, "cx")? * EMU, attr_f32(e, "cy")? * EMU)))
            .unwrap_or((0.0, 0.0));
        let place = match kind {
            "twoCellAnchor" => match (marker("from"), marker("to")) {
                (Some(from), Some(to)) => Anchor { from, to: Some(to), size: ext_size },
                _ => continue,
            },
            "oneCellAnchor" => match marker("from") {
                Some(from) => Anchor { from, to: None, size: ext_size },
                None => continue,
            },
            _ => continue,
        };
        // a picture
        if let Some(pic) = anchor.children().find(|n| n.has_tag_name((XDR, "pic"))) {
            let Some(embed) = pic.descendants().find(|n| n.has_tag_name((DRAW, "blip"))).and_then(|b| b.attribute((REL, "embed"))) else { continue };
            let Some(media) = drels.get(embed).cloned() else { continue };
            let ext = media.rsplit('.').next().unwrap_or("png").to_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp") {
                continue;
            }
            let Some(data) = pkg.read(&media) else { continue };
            pictures.push(Picture { from: place.from, to: place.to, size: place.size, data, ext });
            continue;
        }
        // a chart
        if let Some(frame) = anchor.children().find(|n| n.has_tag_name((XDR, "graphicFrame"))) {
            let chart_rel = frame.descendants().find(|n| n.tag_name().name() == "chart").and_then(|c| c.attribute((REL, "id")));
            if let Some(xml) = chart_rel.and_then(|id| drels.get(id).cloned()).and_then(|p| pkg.text(&p)) {
                charts.push((drawn.len(), place, xml));
            }
            continue;
        }
        // shapes, text boxes, lines and groups of them
        let shapes = crate::office::drawing::shapes(anchor, theme, place.size);
        if !shapes.is_empty() {
            drawn.push(Drawn::Shapes(place, shapes));
        }
    }
    (pictures, charts, drawn)
}
