// Presentation handouts as Word documents: each slide's title, a picture of it and
// the speaker's notes (editable text), two widescreen slides to a page.

use std::fmt::Write as _;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub struct HandoutSlide {
    pub title: String,
    /// The slide as a JPEG or PNG, and its size in pixels
    pub picture: Vec<u8>,
    pub png: bool,
    pub width: u32,
    pub height: u32,
    pub notes: String,
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
/// A4 with 2 cm margins: the text width in EMU (17 cm)
const TEXT_WIDTH: i64 = 6_120_000;

fn text_run(text: &str) -> String {
    format!("<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(text))
}

pub fn write(slides: &[HandoutSlide]) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    let mut rels = String::from(
        "<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
    );
    for (i, slide) in slides.iter().enumerate() {
        let n = i + 1;
        let heading = if slide.title.trim().is_empty() { format!("Slide {n}") } else { format!("Slide {n}: {}", slide.title.replace('\n', " ")) };
        // Two widescreen slides share a page, as PowerPoint's handouts do; taller ones
        // get a page each
        let per_page = if slide.height as f32 <= slide.width as f32 * 0.6 { 2 } else { 1 };
        let page_break = if i > 0 && i % per_page == 0 { "<w:pageBreakBefore/>" } else if i > 0 { "<w:spacing w:before=\"480\"/>" } else { "" };
        let _ = write!(body, "<w:p><w:pPr><w:pStyle w:val=\"Heading2\"/>{page_break}</w:pPr>{}</w:p>", text_run(&heading));

        let (w, h) = (TEXT_WIDTH, TEXT_WIDTH * slide.height.max(1) as i64 / slide.width.max(1) as i64);
        // A tall slide still fits on the page
        let (w, h) = if h > 7_200_000 { (w * 7_200_000 / h, 7_200_000) } else { (w, h) };
        let rid = format!("rIdImg{n}");
        let file = format!("slide{n}.{}", if slide.png { "png" } else { "jpeg" });
        let _ = write!(
            rels,
            "<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{file}\"/>"
        );
        let _ = write!(
            body,
            "<w:p><w:pPr><w:jc w:val=\"center\"/><w:spacing w:after=\"240\"/></w:pPr><w:r><w:drawing>\
             <wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{w}\" cy=\"{h}\"/><wp:docPr id=\"{n}\" name=\"Slide {n}\"/>\
             <wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
             <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic>\
             <pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{file}\"/><pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>\
             <a:ln w=\"6350\"><a:solidFill><a:srgbClr val=\"BFBFBF\"/></a:solidFill></a:ln></pic:spPr>\
             </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        );
        let notes: Vec<&str> = slide.notes.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if !notes.is_empty() {
            let _ = write!(body, "<w:p><w:pPr><w:pStyle w:val=\"NotesHeading\"/></w:pPr>{}</w:p>", text_run("Speaker notes"));
            for line in notes {
                let _ = write!(body, "<w:p>{}</w:p>", text_run(line));
            }
        }
    }
    let document = format!(
        "{XML}<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
         xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
         xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{body}\
         <w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"567\" w:footer=\"567\" w:gutter=\"0\"/></w:sectPr>\
         </w:body></w:document>"
    );
    let styles = format!(
        "{XML}<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
         <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault>\
         <w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"264\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
         <w:pPr><w:keepNext/><w:spacing w:before=\"0\" w:after=\"200\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"1F3864\"/><w:sz w:val=\"30\"/><w:szCs w:val=\"30\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"NotesHeading\"><w:name w:val=\"Notes Heading\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/>\
         <w:pPr><w:keepNext/><w:spacing w:before=\"120\" w:after=\"60\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"595959\"/><w:sz w:val=\"20\"/></w:rPr></w:style></w:styles>"
    );

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let put = |zip: &mut ZipWriter<Cursor<Vec<u8>>>, name: &str, data: &[u8], options: SimpleFileOptions| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data).map_err(|e| e.to_string())
    };
    put(
        &mut zip,
        "[Content_Types].xml",
        format!(
            "{XML}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
             <Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
             <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
             <Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>"
        )
        .as_bytes(),
        deflate,
    )?;
    put(
        &mut zip,
        "_rels/.rels",
        format!(
            "{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
        )
        .as_bytes(),
        deflate,
    )?;
    put(&mut zip, "word/document.xml", document.as_bytes(), deflate)?;
    put(&mut zip, "word/styles.xml", styles.as_bytes(), deflate)?;
    put(
        &mut zip,
        "word/_rels/document.xml.rels",
        format!("{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>").as_bytes(),
        deflate,
    )?;
    for (i, slide) in slides.iter().enumerate() {
        let file = format!("word/media/slide{}.{}", i + 1, if slide.png { "png" } else { "jpeg" });
        put(&mut zip, &file, &slide.picture, stored)?;
    }
    zip.finish().map(|c| c.into_inner()).map_err(|e| e.to_string())
}
