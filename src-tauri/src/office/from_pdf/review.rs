//! Manual review tools (ignored by default): dump what the analysis sees on a page

use super::*;

/// What the analysis sees on one page:
/// `FLIPFILE_PDF=in.pdf FLIPFILE_PAGE=2 cargo test dump_pdf_page -- --ignored --nocapture`
#[test]
#[ignore]
fn dump_pdf_page() {
    let pdfium = crate::pdf::render::pdfium().unwrap();
    let doc = pdfium.load_pdf_from_file(&std::env::var("FLIPFILE_PDF").unwrap(), None).unwrap();
    let index: i32 = std::env::var("FLIPFILE_PAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let page = doc.pages().get(index - 1).unwrap();
    let text = page.text().unwrap();
    let mut modes: HashMap<String, usize> = HashMap::new();
    for c in text.chars().iter() {
        *modes.entry(format!("{:?}", c.render_mode().ok())).or_default() += 1;
    }
    println!("render modes: {modes:?}");
    let mut kinds: HashMap<String, usize> = HashMap::new();
    for o in page.objects().iter() {
        *kinds.entry(format!("{:?}", o.object_type())).or_default() += 1;
    }
    println!("objects: {kinds:?}");
    if let Ok(probe) = std::env::var("FLIPFILE_CHARS") {
        // Raw characters of the first line containing `probe`'s first letter run
        for c in text.chars().iter().take(400) {
            let ch = c.unicode_char().unwrap_or('?');
            if probe.contains(ch) || ch == ' ' {
                let b = c.loose_bounds().ok();
                println!("{ch:?} gen={:?} l={:?} r={:?}", c.is_generated().ok(), b.map(|b| b.left().value), b.map(|b| b.right().value));
            }
        }
    }
    let mut media = MediaStore::default();
    let model = page_model(&page, &doc, &mut media);
    if let Ok(needle) = std::env::var("FLIPFILE_LINES") {
        // Baselines of every line containing the needle's words, tables included
        fn walk(blocks: &[Block], needle: &str) {
            for b in blocks {
                match &b.content {
                    Content::Paragraph(lines) => {
                        for l in lines.iter().filter(|l| needle.split('|').any(|n| l.text().contains(n))) {
                            println!("{:7.2} {:6.2} size {:4.1}  {}  [{}]", l.baseline, l.left, l.size, l.text(), l.runs.first().map(|r| r.1.font.as_str()).unwrap_or(""));
                        }
                    }
                    Content::Table(t) => t.cells.iter().for_each(|c| walk(&c.blocks, needle)),
                    Content::Images(_) => {}
                }
            }
        }
        walk(&model.body, &needle);
        walk(&model.header, &needle);
        return;
    }
    for b in &model.body {
        let kind = match &b.content {
            Content::Paragraph(lines) => format!("P {:?}", lines.iter().map(|l| l.text()).collect::<Vec<_>>()),
            Content::Table(t) => format!("T {}x{} cells {} rows at {:?}", t.xs.len() - 1, t.ys.len() - 1, t.cells.len(), t.ys.iter().map(|y| (y * 10.0).round() / 10.0).collect::<Vec<_>>()),
            Content::Images(i) => format!("I {}", i.len()),
        };
        println!("{:7.1} {:7.1} {:7.1} {:7.1}  {}", b.rect.l, b.rect.t, b.rect.r, b.rect.b, kind.chars().take(160).collect::<String>());
    }
}
