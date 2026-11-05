// Writes PowerPoint presentations (.pptx): slides of pictures, text boxes and tables on
// one blank layout with the standard Office theme, so the result opens in PowerPoint,
// Keynote, Google Slides and LibreOffice, and stays editable.

use std::fmt::Write as _;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// EMU per point
pub const EMU_PER_PT: f32 = 12_700.0;
/// 16:9, as PowerPoint's default (13.333 × 7.5 in)
pub const WIDE: (i64, i64) = (12_192_000, 6_858_000);

pub struct Deck {
    /// Slide size in EMU
    pub width: i64,
    pub height: i64,
    pub slides: Vec<NewSlide>,
    pub title: String,
}

#[derive(Default)]
pub struct NewSlide {
    pub background: Option<[u8; 3]>,
    pub items: Vec<Item>,
}

/// Positions and sizes in EMU
pub enum Item {
    Picture { x: i64, y: i64, w: i64, h: i64, data: Vec<u8>, ext: &'static str, name: String },
    Text { x: i64, y: i64, w: i64, h: i64, paragraphs: Vec<Para>, anchor: &'static str, shrink: bool },
    Table { x: i64, y: i64, w: i64, rows: Vec<Vec<String>>, header: bool, size: f32 },
}

#[derive(Clone)]
pub struct Para {
    pub text: String,
    /// Points
    pub size: f32,
    pub bold: bool,
    pub color: [u8; 3],
    /// Bulleted, at a level from 0
    pub bullet: Option<u8>,
    pub align: &'static str,
    /// Points of space before the paragraph
    pub space_before: f32,
}

impl Para {
    pub fn new(text: impl Into<String>, size: f32) -> Para {
        Para { text: text.into(), size, bold: false, color: [0x26, 0x26, 0x26], bullet: None, align: "l", space_before: 0.0 }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters XML 1.0 cannot hold
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("{r:02X}{g:02X}{b:02X}")
}

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

fn paragraphs_xml(paragraphs: &[Para]) -> String {
    let mut out = String::new();
    for p in paragraphs {
        out.push_str("<a:p><a:pPr");
        match p.bullet {
            Some(level) => {
                let indent = 285_750;
                let _ = write!(out, " marL=\"{}\" indent=\"-{indent}\" lvl=\"{}\"", indent * (level as i64 + 1), level.min(8));
            }
            None => out.push_str(" marL=\"0\" indent=\"0\""),
        }
        let _ = write!(out, " algn=\"{}\">", p.align);
        if p.space_before > 0.0 {
            let _ = write!(out, "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>", (p.space_before * 100.0) as i64);
        }
        if p.bullet.is_some() {
            out.push_str("<a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/>");
        } else {
            out.push_str("<a:buNone/>");
        }
        out.push_str("</a:pPr>");
        let rpr = format!(
            "lang=\"en-US\" sz=\"{}\" b=\"{}\" dirty=\"0\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
            (p.size * 100.0).round() as i64,
            if p.bold { 1 } else { 0 },
            hex(p.color)
        );
        // Line breaks inside a paragraph stay breaks
        for (i, line) in p.text.split('\n').enumerate() {
            if i > 0 {
                let _ = write!(out, "<a:br><a:rPr {rpr}</a:rPr></a:br>");
            }
            if !line.is_empty() {
                let _ = write!(out, "<a:r><a:rPr {rpr}</a:rPr><a:t>{}</a:t></a:r>", esc(line));
            }
        }
        let _ = write!(out, "<a:endParaRPr {rpr}</a:endParaRPr></a:p>");
    }
    if paragraphs.is_empty() {
        out.push_str("<a:p><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p>");
    }
    out
}

/// One slide's XML and its pictures' relationships (target file names)
fn slide_xml(slide: &NewSlide, media: &mut Vec<(String, Vec<u8>)>, pictures: &mut Vec<String>) -> String {
    let mut shapes = String::new();
    for (i, item) in slide.items.iter().enumerate() {
        let id = i + 2;
        match item {
            Item::Picture { x, y, w, h, data, ext, name } => {
                let file = format!("image{}.{ext}", media.len() + 1);
                media.push((file.clone(), data.clone()));
                pictures.push(file);
                let rid = format!("rId{}", pictures.len() + 1);
                let _ = write!(
                    shapes,
                    "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
                     <p:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
                     <p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
                    esc(name)
                );
            }
            Item::Text { x, y, w, h, paragraphs, anchor, shrink } => {
                let fit = if *shrink { "<a:normAutofit/>" } else { "" };
                let _ = write!(
                    shapes,
                    "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Text {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>\
                     <p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>\
                     <p:txBody><a:bodyPr wrap=\"square\" lIns=\"91440\" tIns=\"45720\" rIns=\"91440\" bIns=\"45720\" anchor=\"{anchor}\" rtlCol=\"0\">{fit}</a:bodyPr><a:lstStyle/>{}</p:txBody></p:sp>",
                    paragraphs_xml(paragraphs)
                );
            }
            Item::Table { x, y, w, rows, header, size } => {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                let col_w = w / cols as i64;
                let row_h = ((size * 2.2) * EMU_PER_PT) as i64;
                let mut grid = String::new();
                for _ in 0..cols {
                    let _ = write!(grid, "<a:gridCol w=\"{col_w}\"/>");
                }
                let mut body = String::new();
                for (ri, row) in rows.iter().enumerate() {
                    let _ = write!(body, "<a:tr h=\"{row_h}\">");
                    for ci in 0..cols {
                        let text = row.get(ci).map(String::as_str).unwrap_or("");
                        let mut para = Para::new(text, *size);
                        para.bold = *header && ri == 0;
                        para.color = if *header && ri == 0 { [255, 255, 255] } else { [0, 0, 0] };
                        let _ = write!(body, "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr/></a:tc>", paragraphs_xml(&[para]));
                    }
                    body.push_str("</a:tr>");
                }
                let first_row = if *header { " firstRow=\"1\"" } else { "" };
                let _ = write!(
                    shapes,
                    "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>\
                     <p:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm>\
                     <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl>\
                     <a:tblPr{first_row} bandRow=\"1\"><a:tableStyleId>{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}</a:tableStyleId></a:tblPr>\
                     <a:tblGrid>{grid}</a:tblGrid>{body}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
                    col_w * cols as i64,
                    row_h * rows.len() as i64
                );
            }
        }
    }
    let background = slide
        .background
        .map(|c| format!("<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>", hex(c)))
        .unwrap_or_default();
    format!(
        "{XML}<p:sld {NS}><p:cSld>{background}<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
         <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )
}

pub fn write(deck: &Deck) -> Result<Vec<u8>, String> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // Pictures are compressed already
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let put = |zip: &mut ZipWriter<Cursor<Vec<u8>>>, name: &str, data: &[u8], options: SimpleFileOptions| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data).map_err(|e| e.to_string())
    };

    let n = deck.slides.len().max(1);
    let mut overrides = String::new();
    for i in 1..=n {
        let _ = write!(overrides, "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>");
    }
    let content_types = format!(
        "{XML}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
         <Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/>\
         <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
         <Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
         <Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
         <Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
         <Override PartName=\"/ppt/presProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presProps+xml\"/>\
         <Override PartName=\"/ppt/viewProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml\"/>\
         <Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml\"/>\
         <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
         <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
         {overrides}</Types>"
    );
    put(&mut zip, "[Content_Types].xml", content_types.as_bytes(), deflate)?;
    put(
        &mut zip,
        "_rels/.rels",
        format!(
            "{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
             <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>"
        )
        .as_bytes(),
        deflate,
    )?;
    put(
        &mut zip,
        "docProps/core.xml",
        format!(
            "{XML}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
             <dc:title>{}</dc:title><dc:creator>flipfile</dc:creator></cp:coreProperties>",
            esc(&deck.title)
        )
        .as_bytes(),
        deflate,
    )?;
    put(
        &mut zip,
        "docProps/app.xml",
        format!(
            "{XML}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>flipfile</Application><Slides>{n}</Slides></Properties>"
        )
        .as_bytes(),
        deflate,
    )?;

    // presentation.xml and its relationships
    let mut slide_ids = String::new();
    let mut pres_rels = String::from(
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>\
         <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>\
         <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps\" Target=\"presProps.xml\"/>\
         <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps\" Target=\"viewProps.xml\"/>\
         <Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles\" Target=\"tableStyles.xml\"/>",
    );
    for i in 1..=n {
        let _ = write!(slide_ids, "<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 255 + i, 5 + i);
        let _ = write!(
            pres_rels,
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{i}.xml\"/>",
            5 + i
        );
    }
    let presentation = format!(
        "{XML}<p:presentation {NS} saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>\
         <p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/>\
         <p:defaultTextStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\"><a:defRPr sz=\"1800\" kern=\"1200\">\
         <a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle>\
         </p:presentation>",
        deck.width, deck.height
    );
    put(&mut zip, "ppt/presentation.xml", presentation.as_bytes(), deflate)?;
    put(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        format!("{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{pres_rels}</Relationships>").as_bytes(),
        deflate,
    )?;
    put(&mut zip, "ppt/presProps.xml", format!("{XML}<p:presentationPr {NS}/>").as_bytes(), deflate)?;
    put(
        &mut zip,
        "ppt/viewProps.xml",
        format!("{XML}<p:viewPr {NS}><p:normalViewPr><p:restoredLeft sz=\"15620\"/><p:restoredTop sz=\"94660\"/></p:normalViewPr><p:gridSpacing cx=\"76200\" cy=\"76200\"/></p:viewPr>").as_bytes(),
        deflate,
    )?;
    put(
        &mut zip,
        "ppt/tableStyles.xml",
        format!("{XML}<a:tblStyleLst xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" def=\"{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}\"/>").as_bytes(),
        deflate,
    )?;
    put(&mut zip, "ppt/theme/theme1.xml", THEME.as_bytes(), deflate)?;
    put(&mut zip, "ppt/slideMasters/slideMaster1.xml", master_xml().as_bytes(), deflate)?;
    put(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        format!(
            "{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/></Relationships>"
        )
        .as_bytes(),
        deflate,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            "{XML}<p:sldLayout {NS} type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
             <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>\
             <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
        )
        .as_bytes(),
        deflate,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        format!(
            "{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/></Relationships>"
        )
        .as_bytes(),
        deflate,
    )?;

    let empty = NewSlide::default();
    let mut media = vec![];
    for i in 0..n {
        let slide = deck.slides.get(i).unwrap_or(&empty);
        let mut pictures = vec![];
        let first_media = media.len();
        let xml = slide_xml(slide, &mut media, &mut pictures);
        put(&mut zip, &format!("ppt/slides/slide{}.xml", i + 1), xml.as_bytes(), deflate)?;
        let mut rels = String::from(
            "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>",
        );
        for (j, file) in pictures.iter().enumerate() {
            let _ = write!(
                rels,
                "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/{file}\"/>",
                j + 2
            );
        }
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
            format!("{XML}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>").as_bytes(),
            deflate,
        )?;
        // Written as soon as each slide is done, so only one slide's pictures are held twice
        for (file, data) in &mut media[first_media..] {
            put(&mut zip, &format!("ppt/media/{file}"), data, stored)?;
            *data = Vec::new();
        }
    }
    zip.finish().map(|c| c.into_inner()).map_err(|e| e.to_string())
}

fn master_xml() -> String {
    let level = |i: usize, size: u32| {
        let mar = 228_600 + i * 457_200;
        format!(
            "<a:lvl{n}pPr marL=\"{mar}\" indent=\"-228600\" algn=\"l\" defTabSz=\"914400\"><a:lnSpc><a:spcPct val=\"90000\"/></a:lnSpc><a:spcBef><a:spcPts val=\"1000\"/></a:spcBef>\
             <a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/><a:defRPr sz=\"{size}\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill>\
             <a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl{n}pPr>",
            n = i + 1
        )
    };
    let body: String = (0..5).map(|i| level(i, [2800, 2400, 2000, 1800, 1800][i])).collect();
    format!(
        "{XML}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
         <p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>\
         <p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>\
         <p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>\
         <p:txStyles><p:titleStyle><a:lvl1pPr algn=\"l\" defTabSz=\"914400\"><a:lnSpc><a:spcPct val=\"90000\"/></a:lnSpc><a:spcBef><a:spcPct val=\"0\"/></a:spcBef><a:buNone/>\
         <a:defRPr sz=\"4400\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/><a:ea typeface=\"+mj-ea\"/><a:cs typeface=\"+mj-cs\"/></a:defRPr></a:lvl1pPr></p:titleStyle>\
         <p:bodyStyle>{body}</p:bodyStyle><p:otherStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr></p:otherStyle></p:txStyles></p:sldMaster>"
    )
}

/// The Office theme: colours, Aptos-style fonts with common fallbacks, effects
const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="0E2841"/></a:dk2><a:lt2><a:srgbClr val="E8E8E8"/></a:lt2><a:accent1><a:srgbClr val="156082"/></a:accent1><a:accent2><a:srgbClr val="E97132"/></a:accent2><a:accent3><a:srgbClr val="196B24"/></a:accent3><a:accent4><a:srgbClr val="0F9ED5"/></a:accent4><a:accent5><a:srgbClr val="A02B93"/></a:accent5><a:accent6><a:srgbClr val="4EA72E"/></a:accent6><a:hlink><a:srgbClr val="467886"/></a:hlink><a:folHlink><a:srgbClr val="96607D"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:shade val="80000"/></a:schemeClr></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln><a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/></a:schemeClr></a:solidFill><a:solidFill><a:schemeClr val="phClr"><a:shade val="85000"/></a:schemeClr></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_decks_read_back() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(40, 20).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut title = Para::new("Hello & <welcome>", 40.0);
        title.bold = true;
        let mut bullet = Para::new("Point\nsecond line", 24.0);
        bullet.bullet = Some(1);
        let deck = Deck {
            width: WIDE.0,
            height: WIDE.1,
            title: "Test".into(),
            slides: vec![
                NewSlide { background: None, items: vec![Item::Text { x: 0, y: 0, w: 9_000_000, h: 1_000_000, paragraphs: vec![title, bullet], anchor: "t", shrink: true }] },
                NewSlide {
                    background: Some([0, 0, 0]),
                    items: vec![
                        Item::Picture { x: 0, y: 0, w: 4_000_000, h: 2_000_000, data: png, ext: "png", name: "p".into() },
                        Item::Table { x: 0, y: 3_000_000, w: 8_000_000, rows: vec![vec!["a".into(), "b".into()], vec!["1".into()]], header: true, size: 14.0 },
                    ],
                },
            ],
        };
        let bytes = write(&deck).unwrap();
        let read = super::super::pptx::read(&bytes).unwrap();
        assert_eq!(read.slides.len(), 2);
        assert_eq!(read.slides[1].elements.len(), 2);
        assert_eq!(read.media.len(), 1);
        let pdf = super::super::slides_pdf::to_pdf(&read).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }
}
