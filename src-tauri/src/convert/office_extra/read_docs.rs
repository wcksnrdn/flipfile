//! Reading Documents (PDF, DOCX, MD, TXT, HTML, RTF, ODT, EPUB, PPTX/ODP, Tables)

use super::*;

pub(in crate::convert) fn read_any_blocks(input: &Path) -> Result<Vec<DocBlock>, String> {
    let name = file_name(input);
    let ext = ext_lower(input);
    let bytes = fs::read(input).map_err(|e| format!("\"{name}\" could not be read: {e}"))?;

    if pdf_render::is_pdf(&bytes) || ext == "pdf" {
        return read_pdf_blocks(&bytes, name);
    }

    match ext.as_str() {
        "docx" | "docm" | "dotx" | "dotm" => read_docx_blocks(&bytes),
        "md" | "markdown" => Ok(parse_markdown_blocks(&String::from_utf8_lossy(&bytes))),
        "txt" | "typ" | "tex" => Ok(parse_text_blocks(&String::from_utf8_lossy(&bytes))),
        "html" | "htm" => Ok(parse_html_blocks(&String::from_utf8_lossy(&bytes))),
        "rtf" => Ok(parse_rtf_blocks(&bytes)),
        "odt" => read_odt_blocks(&bytes),
        "epub" => read_epub_blocks(&bytes),
        "pptx" | "odp" => read_presentation_blocks(&bytes, &ext),
        "xlsx" | "xlsm" | "xltx" | "xltm" | "csv" | "tsv" | "tab" | "json" | "yaml" | "yml" | "xml" => {
            let sheets = read_any_tables(input)?;
            let mut blocks = Vec::new();
            for s in sheets {
                if !s.name.is_empty() {
                    blocks.push(DocBlock::Heading(2, s.name));
                }
                if !s.rows.is_empty() {
                    blocks.push(DocBlock::Table(s.rows));
                }
            }
            Ok(blocks)
        }
        _ => Err(format!("\"{name}\": unsupported document format")),
    }
}

fn read_pdf_blocks(bytes: &[u8], name: &str) -> Result<Vec<DocBlock>, String> {
    let pdfium = pdf_render::pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|_| format!("\"{name}\": the PDF could not be opened"))?;
    let mut blocks = Vec::new();
    for (idx, page) in doc.pages().iter().enumerate() {
        if idx > 0 {
            blocks.push(DocBlock::PageBreak);
        }
        let text = page.text().map(|t| t.all()).unwrap_or_default();
        for para in text.split("\n\n") {
            let clean: String = para
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !clean.is_empty() {
                blocks.push(DocBlock::Paragraph(clean));
            }
        }
    }
    if blocks.is_empty() {
        return Err(format!("\"{name}\" has no selectable text (use WORD OCR for scanned PDFs)"));
    }
    Ok(blocks)
}

fn read_docx_blocks(bytes: &[u8]) -> Result<Vec<DocBlock>, String> {
    let doc = docx::read(bytes)?;
    let mut out = Vec::new();
    for section in &doc.sections {
        for block in &section.blocks {
            collect_docx_block(block, &mut out);
        }
    }
    Ok(out)
}

fn collect_docx_block(block: &docx::Block, out: &mut Vec<DocBlock>) {
    match block {
        docx::Block::Paragraph(p) => {
            let mut text = String::new();
            for inline in &p.inlines {
                match inline {
                    docx::Inline::Text(t, _) => text.push_str(t),
                    docx::Inline::Tab | docx::Inline::AlignTab => text.push('\t'),
                    docx::Inline::LineBreak => text.push('\n'),
                    docx::Inline::PageBreak => out.push(DocBlock::PageBreak),
                    docx::Inline::TextBox(inner) => {
                        for b in inner {
                            collect_docx_block(b, out);
                        }
                    }
                    _ => {}
                }
            }
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                return;
            }
            if p.marker.is_some() {
                out.push(DocBlock::ListItem(trimmed));
                return;
            }
            if let Some(ref style) = p.props.style_id {
                let s = style.to_lowercase();
                if s.starts_with("heading") || s == "title" {
                    let lvl = s.chars().last().and_then(|c| c.to_digit(10)).unwrap_or(1).clamp(1, 6) as u8;
                    out.push(DocBlock::Heading(lvl, trimmed));
                    return;
                }
            }
            let max_size = p.inlines.iter().fold(0.0f32, |acc, i| match i {
                docx::Inline::Text(_, r) => acc.max(r.size),
                _ => acc,
            });
            if max_size >= 18.0 {
                out.push(DocBlock::Heading(1, trimmed));
            } else if max_size >= 14.0 {
                out.push(DocBlock::Heading(2, trimmed));
            } else {
                out.push(DocBlock::Paragraph(trimmed));
            }
        }
        docx::Block::Table(t) => {
            let mut rows = Vec::new();
            for row in &t.rows {
                let mut r_cells = Vec::new();
                for cell in &row.cells {
                    let mut sub = Vec::new();
                    for b in &cell.blocks {
                        collect_docx_block(b, &mut sub);
                    }
                    let joined = sub
                        .into_iter()
                        .filter_map(|b| match b {
                            DocBlock::Heading(_, s) | DocBlock::Paragraph(s) | DocBlock::ListItem(s) => Some(s),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    r_cells.push(joined);
                }
                if !r_cells.is_empty() {
                    rows.push(r_cells);
                }
            }
            if !rows.is_empty() {
                out.push(DocBlock::Table(rows));
            }
        }
    }
}

fn parse_markdown_blocks(md: &str) -> Vec<DocBlock> {
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut code_buf = String::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut para_buf = String::new();

    let flush_para = |para_buf: &mut String, blocks: &mut Vec<DocBlock>| {
        let t = para_buf.trim();
        if !t.is_empty() {
            blocks.push(DocBlock::Paragraph(clean_md_inline(t)));
        }
        para_buf.clear();
    };
    let flush_table = |table_rows: &mut Vec<Vec<String>>, blocks: &mut Vec<DocBlock>| {
        if !table_rows.is_empty() {
            blocks.push(DocBlock::Table(std::mem::take(table_rows)));
        }
    };

    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            flush_para(&mut para_buf, &mut blocks);
            flush_table(&mut table_rows, &mut blocks);
            if in_code {
                blocks.push(DocBlock::CodeBlock(std::mem::take(&mut code_buf)));
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(raw);
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2 {
            flush_para(&mut para_buf, &mut blocks);
            let cells: Vec<String> = trimmed[1..trimmed.len() - 1]
                .split('|')
                .map(|c| clean_md_inline(c.trim()))
                .collect();
            let is_sep = cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
            if !is_sep {
                table_rows.push(cells);
            }
            continue;
        } else {
            flush_table(&mut table_rows, &mut blocks);
        }

        if trimmed.is_empty() {
            flush_para(&mut para_buf, &mut blocks);
            continue;
        }

        if let Some(hashes) = trimmed.split_whitespace().next()
            && hashes.chars().all(|c| c == '#') && hashes.len() <= 6 {
                flush_para(&mut para_buf, &mut blocks);
                let rest = trimmed[hashes.len()..].trim();
                blocks.push(DocBlock::Heading(hashes.len() as u8, clean_md_inline(rest)));
                continue;
            }

        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ ")) {
            flush_para(&mut para_buf, &mut blocks);
            // Nested items keep their depth: two leading spaces a level
            let indent = line.len() - line.trim_start().len();
            let depth = line[..indent].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>() / 2;
            blocks.push(DocBlock::ListItem(format!("{}{}", "  ".repeat(depth.min(4)), clean_md_inline(item.trim()))));
            continue;
        }

        if !para_buf.is_empty() {
            para_buf.push(' ');
        }
        para_buf.push_str(trimmed);
    }
    flush_para(&mut para_buf, &mut blocks);
    flush_table(&mut table_rows, &mut blocks);
    if !code_buf.is_empty() {
        blocks.push(DocBlock::CodeBlock(code_buf));
    }
    blocks
}

fn clean_md_inline(s: &str) -> String {
    s.replace("**", "").replace("__", "").replace('`', "")
}

fn parse_text_blocks(txt: &str) -> Vec<DocBlock> {
    let mut blocks = Vec::new();
    let mut buf = String::new();
    for line in txt.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !buf.is_empty() {
                blocks.push(DocBlock::Paragraph(std::mem::take(&mut buf)));
            }
        } else {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(t);
        }
    }
    if !buf.is_empty() {
        blocks.push(DocBlock::Paragraph(buf));
    }
    blocks
}

fn parse_html_blocks(html: &str) -> Vec<DocBlock> {
    // Lightweight tag-aware parser that works on both strict XHTML and loose HTML5
    let mut blocks = Vec::new();
    // Markup nested too deep for the XML parser goes straight to the flat fallback below
    let parsed = if crate::office::xml_too_deep(html) { None } else { roxmltree::Document::parse(html).ok() };
    if let Some(doc) = parsed {
        collect_xml_html_nodes(doc.root(), &mut blocks);
        if !blocks.is_empty() {
            return blocks;
        }
    }
    // Loose HTML fallback: split by block tags and strip inline tags
    let normalized = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("</p>", "\n\n")
        .replace("</div>", "\n\n")
        .replace("</h1>", "\n\n")
        .replace("</h2>", "\n\n")
        .replace("</h3>", "\n\n")
        .replace("</li>", "\n");
    let mut plain = String::with_capacity(normalized.len());
    let mut in_tag = false;
    for ch in normalized.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    parse_text_blocks(&html_unescape(&plain))
}

fn collect_xml_html_nodes(node: roxmltree::Node, out: &mut Vec<DocBlock>) {
    for child in node.children().filter(|n| n.is_element()) {
        let tag = child.tag_name().name().to_ascii_lowercase();
        match tag.as_str() {
            "script" | "style" | "head" => {}
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let lvl = tag[1..].parse::<u8>().unwrap_or(1);
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::Heading(lvl, text));
                }
            }
            "p" => {
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::Paragraph(text));
                }
            }
            "li" => {
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::ListItem(text));
                }
            }
            "pre" => {
                let text = collect_node_text(child);
                if !text.is_empty() {
                    out.push(DocBlock::CodeBlock(text));
                }
            }
            "table" => {
                let mut rows = Vec::new();
                for tr in child.descendants().filter(|n| n.tag_name().name().eq_ignore_ascii_case("tr")) {
                    let cells: Vec<String> = tr
                        .children()
                        .filter(|n| matches!(n.tag_name().name().to_ascii_lowercase().as_str(), "td" | "th"))
                        .map(collect_node_text)
                        .collect();
                    if !cells.is_empty() {
                        rows.push(cells);
                    }
                }
                if !rows.is_empty() {
                    out.push(DocBlock::Table(rows));
                }
            }
            _ => collect_xml_html_nodes(child, out),
        }
    }
}

fn collect_node_text(node: roxmltree::Node) -> String {
    let mut s = String::new();
    for desc in node.descendants() {
        if desc.is_text()
            && let Some(t) = desc.text() {
                s.push_str(t);
            }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn parse_rtf_blocks(bytes: &[u8]) -> Vec<DocBlock> {
    let raw = String::from_utf8_lossy(bytes);
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    let mut depth = 0i32;
    let mut skip_depth: Option<i32> = None;

    while let Some(c) = chars.next() {
        match c {
            '{' => depth += 1,
            '}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                depth -= 1;
            }
            '\\' => {
                let mut word = String::new();
                if let Some(&next) = chars.peek() {
                    if matches!(next, '\\' | '{' | '}') {
                        if skip_depth.is_none() {
                            text.push(chars.next().unwrap());
                        }
                        continue;
                    }
                    if next == '*' {
                        skip_depth = Some(depth);
                        chars.next();
                        continue;
                    }
                }
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_alphabetic() {
                        word.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '-' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                if matches!(word.as_str(), "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict") && skip_depth.is_none() {
                    skip_depth = Some(depth);
                } else if skip_depth.is_none() {
                    match word.as_str() {
                        "par" | "line" => text.push_str("\n\n"),
                        "tab" => text.push('\t'),
                        _ => {}
                    }
                }
            }
            '\r' | '\n' => {}
            other if skip_depth.is_none() => text.push(other),
            _ => {}
        }
    }
    parse_text_blocks(&text)
}

fn read_odt_blocks(bytes: &[u8]) -> Result<Vec<DocBlock>, String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Could not open .odt archive")?;
    let mut xml = String::new();
    zip.by_name("content.xml")
        .map_err(|_| "Missing content.xml in .odt")?
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    if crate::office::xml_too_deep(&xml) {
        return Err("the document is nested too deeply to read".into());
    }
    let doc = roxmltree::Document::parse(&xml).map_err(|e| e.to_string())?;
    let mut blocks = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "h" => {
                let text = collect_node_text(node);
                if !text.is_empty() {
                    blocks.push(DocBlock::Heading(2, text));
                }
            }
            "p" => {
                let parent_tag = node.parent().map(|p| p.tag_name().name()).unwrap_or("");
                if parent_tag != "table-cell" {
                    let text = collect_node_text(node);
                    if !text.is_empty() {
                        blocks.push(DocBlock::Paragraph(text));
                    }
                }
            }
            "table" => {
                let mut rows = Vec::new();
                for tr in node.children().filter(|n| n.tag_name().name() == "table-row") {
                    let cells: Vec<String> = tr
                        .children()
                        .filter(|n| n.tag_name().name() == "table-cell")
                        .map(collect_node_text)
                        .collect();
                    if !cells.is_empty() {
                        rows.push(cells);
                    }
                }
                if !rows.is_empty() {
                    blocks.push(DocBlock::Table(rows));
                }
            }
            _ => {}
        }
    }
    Ok(blocks)
}

fn read_epub_blocks(bytes: &[u8]) -> Result<Vec<DocBlock>, String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "Could not open .epub archive")?;
    let mut html_files: Vec<String> = zip
        .file_names()
        .filter_map(|r| r.ok().map(String::from))
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            (l.ends_with(".xhtml") || l.ends_with(".html") || l.ends_with(".htm")) && !l.contains("nav") && !l.contains("toc")
        })
        .collect();
    html_files.sort();
    let mut blocks = Vec::new();
    for name in html_files {
        if let Ok(mut f) = zip.by_name(&name) {
            let mut content = String::new();
            if f.read_to_string(&mut content).is_ok() {
                blocks.extend(parse_html_blocks(&content));
            }
        }
    }
    if blocks.is_empty() {
        return Err("No readable chapters found in EPUB".into());
    }
    Ok(blocks)
}

fn read_presentation_blocks(bytes: &[u8], ext: &str) -> Result<Vec<DocBlock>, String> {
    if ext == "odp" {
        return read_odt_blocks(bytes);
    }
    let deck = crate::office::pptx::read(bytes)?;
    let mut blocks = Vec::new();
    for (i, slide) in crate::office::slides_pdf::shown(&deck).into_iter().enumerate() {
        if i > 0 {
            blocks.push(DocBlock::PageBreak);
        }
        blocks.extend(slide_blocks(slide, i + 1));
    }
    if blocks.is_empty() {
        return Err("No slide text found in presentation".into());
    }
    Ok(blocks)
}

/// A slide as an outline: its title, then its text in reading order (top to bottom,
/// left to right) with bullet levels, its tables, and the speaker's notes
fn slide_blocks(slide: &crate::office::pptx::Slide, number: usize) -> Vec<DocBlock> {
    use crate::office::pptx::Element;
    let title = slide.title.replace('\n', " ");
    let mut blocks = vec![DocBlock::Heading(2, if title.trim().is_empty() { format!("Slide {number}") } else { title.clone() })];
    let mut elements: Vec<&Element> = slide.elements.iter().collect();
    let top_left = |e: &Element| match e {
        Element::Shape { frame, .. } | Element::Picture { frame, .. } | Element::Table { frame, .. } => (frame.y, frame.x),
    };
    elements.sort_by(|a, b| {
        let (ay, ax) = top_left(a);
        let (by, bx) = top_left(b);
        // Side by side when their tops are within a few points
        if (ay - by).abs() < 6.0 { ax.total_cmp(&bx) } else { ay.total_cmp(&by) }
    });
    let mut title_skipped = false;
    for element in elements {
        match element {
            Element::Shape { text: Some(text), placeholder, .. } => {
                if matches!(placeholder.as_deref(), Some("dt" | "ftr" | "sldNum" | "hdr")) {
                    continue;
                }
                let is_title = matches!(placeholder.as_deref(), Some("title" | "ctrTitle"));
                if is_title && !title_skipped {
                    title_skipped = true;
                    continue;
                }
                for p in &text.paragraphs {
                    let line: String = p.runs.iter().map(|r| if r.line_break { "\n" } else { r.text.as_str() }).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if p.bullet.is_some() {
                        // Nested bullets keep their depth as indentation
                        let depth = ((p.margin_left / 28.0).round() as usize).saturating_sub(1).min(4);
                        blocks.push(DocBlock::ListItem(format!("{}{line}", "  ".repeat(depth))));
                    } else {
                        blocks.push(DocBlock::Paragraph(line.to_string()));
                    }
                }
            }
            Element::Table { rows, .. } => {
                let table: Vec<Vec<String>> = rows
                    .iter()
                    .map(|r| {
                        r.cells
                            .iter()
                            .filter(|c| !c.merged)
                            .map(|c| c.text.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join(" ").trim().to_string())
                            .collect()
                    })
                    .collect();
                if table.iter().any(|r| r.iter().any(|c| !c.is_empty())) {
                    blocks.push(DocBlock::Table(table));
                }
            }
            _ => {}
        }
    }
    let notes = slide.notes.trim();
    if !notes.is_empty() {
        blocks.push(DocBlock::Paragraph(format!("Notes: {}", notes.replace('\n', " "))));
    }
    blocks
}
