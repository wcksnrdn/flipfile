//! Presentations to everything, and anything to slides

use super::*;

#[test]
fn presentations_convert_to_everything() {
    let dir = temp_dir("pptx-out");
    let deck = sample_deck(&dir);
    let deck = deck.to_str().unwrap();

    let pdf = run(deck, "pdf", None, &Options::default()).unwrap();
    assert_eq!(crate::pdf::render::page_info(Path::new(&pdf)).unwrap().pages, 2);

    // A picture per slide
    let png = run(deck, "png", None, &Options::default()).unwrap();
    assert!(Path::new(&png).exists(), "{png}");

    // The outline keeps the order, the nesting and the table
    let md = fs::read_to_string(run(deck, "md", None, &Options::default()).unwrap()).unwrap();
    let revenue = md.find("- Revenue grew 12%").expect(&md);
    let nested = md.find("  - Mostly in Jakarta").expect(&md);
    assert!(revenue < nested);
    assert!(md.contains("Bandung"), "{md}");

    // The Word handout: a page per slide, its picture and title
    let docx = run(deck, "docx", None, &Options::default()).unwrap();
    let bytes = fs::read(&docx).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    assert!(zip.by_name("word/media/slide2.png").is_ok() || zip.by_name("word/media/slide2.jpeg").is_ok());
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("Slide 1"), "{xml}");
    assert!(crate::office::docx::read(&bytes).is_ok());

    assert!(run(deck, "pptx", None, &Options::default()).unwrap_err().contains("already"));
}

#[test]
fn anything_becomes_slides() {
    use crate::office::pptx::read;
    let dir = temp_dir("to-pptx");
    let slides = |path: &str| read(&fs::read(path).unwrap()).unwrap().slides.len();

    // A PDF: a slide per page, in the page's shape
    let pdf = dir.join("pages.pdf");
    let pages: Vec<Page> = (0..3).map(|_| pdf_page(&sample(200, 300), 80).unwrap()).collect();
    fs::write(&pdf, jpeg_pages_to_pdf(&pages)).unwrap();
    let out = run(pdf.to_str().unwrap(), "pptx", None, &Options::default()).unwrap();
    let deck = read(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(deck.slides.len(), 3);
    assert!(deck.height > deck.width, "{} × {}", deck.width, deck.height);

    // A document: headings start slides
    let md = dir.join("notes.md");
    fs::write(&md, "# Plan\n\nFor the team\n\n## Goals\n\n- Ship\n- Measure\n\n## Risks\n\nTime.\n").unwrap();
    assert_eq!(slides(&run(md.to_str().unwrap(), "pptx", None, &Options::default()).unwrap()), 3);

    // A spreadsheet: its table, on as many slides as it needs
    let csv = dir.join("data.csv");
    let mut text = String::from("name,value\n");
    for i in 0..25 {
        text.push_str(&format!("row {i},{i}\n"));
    }
    fs::write(&csv, text).unwrap();
    assert_eq!(slides(&run(csv.to_str().unwrap(), "pptx", None, &Options::default()).unwrap()), 4);

    // Several pictures: one deck, a slide each, in order
    let mut paths = vec![];
    for i in 0..3 {
        let p = dir.join(format!("photo{i}.png"));
        sample(120 + i * 10, 80).save(&p).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }
    let channel = Channel::new(|_| Ok(()));
    let result = run_batch(&paths, "pptx", None, &Options::default(), "converted", &channel).unwrap();
    assert_eq!(result.outputs.len(), 1);
    assert!(result.outputs[0].ends_with("photo0_merged.pptx"));
    let deck = read(&fs::read(&result.outputs[0]).unwrap()).unwrap();
    assert_eq!(deck.slides.len(), 3);
    assert_eq!(deck.media.len(), 3);
}
