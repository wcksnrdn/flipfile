//! Writing Documents (MD, TXT, HTML, RTF, EPUB, ODT, DOCX, PDF)

use super::*;

/// How deeply a list item is nested: two leading spaces a level
fn list_depth(item: &str) -> usize {
    (item.len() - item.trim_start().len()) / 2
}

pub(in crate::convert) fn blocks_to_markdown(blocks: &[DocBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let hashes = "#".repeat((*lvl as usize).clamp(1, 6));
                out.push_str(&format!("{hashes} {t}\n\n"));
            }
            DocBlock::Paragraph(t) => out.push_str(&format!("{t}\n\n")),
            // Leading spaces: a nested item (two per level)
            DocBlock::ListItem(t) => out.push_str(&format!("{}- {}\n", &t[..t.len() - t.trim_start().len()], t.trim_start())),
            DocBlock::CodeBlock(c) => out.push_str(&format!("```\n{c}\n```\n\n")),
            DocBlock::Table(rows) => {
                out.push_str(&rows_to_md_table(rows));
                out.push('\n');
            }
            DocBlock::PageBreak => out.push_str("\n---\n\n"),
        }
    }
    out
}

pub(super) fn blocks_to_text(blocks: &[DocBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(_, t) => out.push_str(&format!("{}\n{}\n\n", t, "=".repeat(t.chars().count().min(60)))),
            DocBlock::Paragraph(t) => out.push_str(&format!("{t}\n\n")),
            DocBlock::ListItem(t) => out.push_str(&format!("{}  * {}\n", &t[..t.len() - t.trim_start().len()], t.trim_start())),
            DocBlock::CodeBlock(c) => out.push_str(&format!("{c}\n\n")),
            DocBlock::Table(rows) => {
                for r in rows {
                    out.push_str(&r.join("\t"));
                    out.push('\n');
                }
                out.push('\n');
            }
            DocBlock::PageBreak => out.push_str("\n----------------------------------------\n\n"),
        }
    }
    out
}

pub(super) fn blocks_to_html(blocks: &[DocBlock], title: &str) -> String {
    let mut body = String::new();
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let l = (*lvl).clamp(1, 6);
                body.push_str(&format!("  <h{l}>{}</h{l}>\n", xml_escape(t)));
            }
            DocBlock::Paragraph(t) => body.push_str(&format!("  <p>{}</p>\n", xml_escape(t))),
            DocBlock::ListItem(t) => body.push_str(&format!("  <ul style=\"margin-left:{}em\"><li>{}</li></ul>\n", list_depth(t) * 2, xml_escape(t.trim_start()))),
            DocBlock::CodeBlock(c) => body.push_str(&format!("  <pre><code>{}</code></pre>\n", xml_escape(c))),
            DocBlock::Table(rows) => {
                body.push_str("  <table>\n");
                for (r_idx, r) in rows.iter().enumerate() {
                    body.push_str("    <tr>");
                    let tag = if r_idx == 0 && rows.len() > 1 { "th" } else { "td" };
                    for cell in r {
                        body.push_str(&format!("<{tag}>{}</{tag}>", xml_escape(cell)));
                    }
                    body.push_str("</tr>\n");
                }
                body.push_str("  </table>\n");
            }
            DocBlock::PageBreak => body.push_str("  <hr/>\n"),
        }
    }
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
  <title>{}</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 780px; margin: 2.5rem auto; padding: 0 1.25rem; line-height: 1.6; color: #1d1d1f; }}
    h1, h2, h3 {{ margin-top: 1.6em; }}
    table {{ border-collapse: collapse; width: 100%; margin: 1.2rem 0; }}
    th, td {{ border: 1px solid #d2d2d7; padding: 0.5rem 0.75rem; text-align: left; }}
    th {{ background: #f5f5f7; font-weight: 600; }}
    pre {{ background: #f5f5f7; padding: 1rem; border-radius: 6px; overflow-x: auto; }}
  </style>
</head>
<body>
{body}</body>
</html>
"#,
        xml_escape(title)
    )
}

pub(super) fn blocks_to_rtf(blocks: &[DocBlock]) -> String {
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Calibri;}}\\fs22\n");
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let sz = match lvl {
                    1 => 36,
                    2 => 30,
                    _ => 26,
                };
                out.push_str(&format!("\\pard\\sa160\\b\\fs{sz} {}\\b0\\fs22\\par\n", rtf_escape(t)));
            }
            DocBlock::Paragraph(t) => out.push_str(&format!("\\pard\\sa140 {}\\par\n", rtf_escape(t))),
            DocBlock::ListItem(t) => out.push_str(&format!("\\pard\\li{}\\sa80 \\u8226? {}\\par\n", 360 * (list_depth(t) + 1), rtf_escape(t.trim_start()))),
            DocBlock::CodeBlock(c) => out.push_str(&format!("\\pard\\sa140 {}\\par\n", rtf_escape(c))),
            DocBlock::Table(rows) => {
                for r in rows {
                    let line = r.iter().map(|c| rtf_escape(c)).collect::<Vec<_>>().join("\\tab ");
                    out.push_str(&format!("\\pard\\sa80 {line}\\par\n"));
                }
            }
            DocBlock::PageBreak => out.push_str("\\page\n"),
        }
    }
    out.push('}');
    out
}

fn rtf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\line "),
            c if c.is_ascii() => out.push(c),
            c => out.push_str(&format!("\\u{}?", c as i32)),
        }
    }
    out
}

pub(super) fn blocks_to_epub(blocks: &[DocBlock], title: &str) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // EPUB requires `mimetype` as the first, uncompressed entry
    zip.start_file("mimetype", stored).map_err(save_err)?;
    zip.write_all(b"application/epub+zip").map_err(save_err)?;

    zip.start_file("META-INF/container.xml", deflated).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )
    .map_err(save_err)?;

    let opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:flipfile-epub-001</dc:identifier>
    <dc:title>{}</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-11-03T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>"#,
        xml_escape(title)
    );
    zip.start_file("OEBPS/content.opf", deflated).map_err(save_err)?;
    zip.write_all(opf.as_bytes()).map_err(save_err)?;

    let nav = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>{t}</title></head>
<body>
  <nav epub:type="toc"><ol><li><a href="chapter1.xhtml">{t}</a></li></ol></nav>
</body>
</html>"#,
        t = xml_escape(title)
    );
    zip.start_file("OEBPS/nav.xhtml", deflated).map_err(save_err)?;
    zip.write_all(nav.as_bytes()).map_err(save_err)?;

    let mut chapter = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>{}</title></head>
<body>
"#,
        xml_escape(title)
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let l = (*lvl).clamp(1, 6);
                chapter.push_str(&format!("  <h{l}>{}</h{l}>\n", xml_escape(t)));
            }
            DocBlock::Paragraph(t) => chapter.push_str(&format!("  <p>{}</p>\n", xml_escape(t))),
            DocBlock::ListItem(t) => chapter.push_str(&format!("  <ul style=\"margin-left:{}em\"><li>{}</li></ul>\n", list_depth(t) * 2, xml_escape(t.trim_start()))),
            DocBlock::CodeBlock(c) => chapter.push_str(&format!("  <pre>{}</pre>\n", xml_escape(c))),
            DocBlock::Table(rows) => {
                chapter.push_str("  <table>\n");
                for r in rows {
                    chapter.push_str("    <tr>");
                    for cell in r {
                        chapter.push_str(&format!("<td>{}</td>", xml_escape(cell)));
                    }
                    chapter.push_str("</tr>\n");
                }
                chapter.push_str("  </table>\n");
            }
            DocBlock::PageBreak => chapter.push_str("  <hr/>\n"),
        }
    }
    chapter.push_str("</body>\n</html>");
    zip.start_file("OEBPS/chapter1.xhtml", deflated).map_err(save_err)?;
    zip.write_all(chapter.as_bytes()).map_err(save_err)?;

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

pub(super) fn blocks_to_odt(blocks: &[DocBlock], _title: &str) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).map_err(save_err)?;
    zip.write_all(b"application/vnd.oasis.opendocument.text").map_err(save_err)?;

    zip.start_file("META-INF/manifest.xml", deflated).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
  <manifest:file-entry manifest:full-path="/" manifest:version="1.2" manifest:media-type="application/vnd.oasis.opendocument.text"/>
  <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"#,
    )
    .map_err(save_err)?;

    let mut content = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" office:version="1.2">
  <office:body>
    <office:text>
"#,
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                content.push_str(&format!(
                    "      <text:h text:outline-level=\"{}\">{}</text:h>\n",
                    (*lvl).clamp(1, 6),
                    xml_escape(t)
                ));
            }
            DocBlock::Paragraph(t) | DocBlock::CodeBlock(t) => {
                content.push_str(&format!("      <text:p>{}</text:p>\n", xml_escape(t)));
            }
            DocBlock::ListItem(t) => {
                content.push_str(&format!("      <text:p>{}• {}</text:p>\n", "<text:tab/>".repeat(list_depth(t)), xml_escape(t.trim_start())));
            }
            DocBlock::Table(rows) => {
                content.push_str("      <table:table>\n");
                for r in rows {
                    content.push_str("        <table:table-row>\n");
                    for cell in r {
                        content.push_str(&format!(
                            "          <table:table-cell><text:p>{}</text:p></table:table-cell>\n",
                            xml_escape(cell)
                        ));
                    }
                    content.push_str("        </table:table-row>\n");
                }
                content.push_str("      </table:table>\n");
            }
            DocBlock::PageBreak => {}
        }
    }
    content.push_str("    </office:text>\n  </office:body>\n</office:document-content>");
    zip.start_file("content.xml", deflated).map_err(save_err)?;
    zip.write_all(content.as_bytes()).map_err(save_err)?;

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

pub(in crate::convert) fn blocks_to_docx(blocks: &[DocBlock]) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut buf);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", opts).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    )
    .map_err(save_err)?;

    zip.start_file("_rels/.rels", opts).map_err(save_err)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    )
    .map_err(save_err)?;

    let mut doc_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
"#,
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let sz = match lvl {
                    1 => 36,
                    2 => 28,
                    _ => 24,
                };
                doc_xml.push_str(&format!(
                    "    <w:p><w:r><w:rPr><w:b/><w:sz w:val=\"{sz}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>\n",
                    xml_escape(t)
                ));
            }
            DocBlock::Paragraph(t) | DocBlock::CodeBlock(t) => {
                doc_xml.push_str(&format!(
                    "    <w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>\n",
                    xml_escape(t)
                ));
            }
            DocBlock::ListItem(t) => {
                doc_xml.push_str(&format!(
                    "    <w:p><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr><w:r><w:t xml:space=\"preserve\">•\t{}</w:t></w:r></w:p>\n",
                    360 * (list_depth(t) + 1),
                    xml_escape(t.trim_start())
                ));
            }
            DocBlock::Table(rows) => {
                doc_xml.push_str("    <w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"CCCCCC\"/></w:tblBorders></w:tblPr>\n");
                for (r_idx, r) in rows.iter().enumerate() {
                    doc_xml.push_str("      <w:tr>");
                    for cell in r {
                        let bold = if r_idx == 0 && rows.len() > 1 { "<w:rPr><w:b/></w:rPr>" } else { "" };
                        doc_xml.push_str(&format!(
                            "<w:tc><w:p><w:r>{bold}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>",
                            xml_escape(cell)
                        ));
                    }
                    doc_xml.push_str("</w:tr>\n");
                }
                doc_xml.push_str("    </w:tbl>\n");
            }
            DocBlock::PageBreak => {
                doc_xml.push_str("    <w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n");
            }
        }
    }
    doc_xml.push_str("  </w:body>\n</w:document>");
    zip.start_file("word/document.xml", opts).map_err(save_err)?;
    zip.write_all(doc_xml.as_bytes()).map_err(save_err)?;

    zip.finish().map_err(save_err)?;
    Ok(buf.into_inner())
}

pub(in crate::convert) fn blocks_to_pdf(blocks: &[DocBlock]) -> Result<Vec<u8>, String> {
    let mut markup = String::from(
        r#"#set page(paper: "a4", margin: 2cm)
#set text(size: 11pt)
#set par(justify: false, leading: 0.65em)
"#,
    );
    for b in blocks {
        match b {
            DocBlock::Heading(lvl, t) => {
                let eqs = "=".repeat((*lvl as usize).clamp(1, 5));
                markup.push_str(&format!("{eqs} #{:?}\n\n", t));
            }
            DocBlock::Paragraph(t) => {
                markup.push_str(&format!("#{:?}\n\n", t));
            }
            DocBlock::ListItem(t) => {
                markup.push_str(&format!("{}- #{:?}\n", "  ".repeat(list_depth(t)), t.trim_start()));
            }
            DocBlock::CodeBlock(c) => {
                markup.push_str(&format!("#raw(block: true, {:?})\n\n", c));
            }
            DocBlock::Table(rows) => {
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                markup.push_str(&format!("#table(columns: {cols}, stroke: 0.5pt + rgb(\"#cccccc\"), inset: 6pt,\n"));
                for (r_idx, r) in rows.iter().enumerate() {
                    for c in 0..cols {
                        let cell = r.get(c).map(String::as_str).unwrap_or("");
                        if r_idx == 0 && rows.len() > 1 {
                            markup.push_str(&format!("  strong({:?}),\n", cell));
                        } else {
                            markup.push_str(&format!("  {:?},\n", cell));
                        }
                    }
                }
                markup.push_str(")\n\n");
            }
            DocBlock::PageBreak => markup.push_str("#pagebreak(weak: true)\n"),
        }
    }
    typeset::to_pdf(markup, vec![]).map_err(|e| format!("the document could not be laid out ({e})"))
}

pub(super) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && c != '\n' && c != '\r' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}
