// Writes the page models rebuilt from a PDF (see `from_pdf`) as a .docx package.
//
// Every PDF page becomes one Word page: pages with the same size and the same running
// header/footer share a section, and each page starts with a page break. Positions are
// kept by measuring: spacing before every block is the gap on the PDF page, lines use
// the measured pitch as exact line height, tab stops sit where the text was.
//
// The XML follows the element order of the OOXML schema, which Word enforces.

use super::from_pdf::{line_pitch, signature, split_numbers, Block, Border, Cell, Content, ImageRef, Line, MediaStore, PageModel, Piece, Style, Table, VAlign};
use std::cell::Cell as Counter;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;

fn twips(points: f32) -> i32 {
    (points * 20.0).round() as i32
}

fn emu(points: f32) -> i64 {
    (points * 12700.0).round() as i64
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters XML 1.0 does not allow
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

fn hex(c: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Where page numbers in a header/footer become fields: (page shown, page count)
#[derive(Clone, Copy)]
struct Fields {
    page: usize,
    total: usize,
}

/// One XML part being written (document, header or footer) and the pictures it uses
#[derive(Default)]
struct Part {
    images: Vec<usize>,
}

impl Part {
    fn image_rel(&mut self, media: usize) -> String {
        if !self.images.contains(&media) {
            self.images.push(media);
        }
        format!("rIdM{media}")
    }
}

struct Writer<'a> {
    media: &'a MediaStore,
    next_id: Counter<u32>,
    /// Factor for the gaps between the body's blocks on the page being written (below
    /// 1 on a page filled to its last point, to leave Word a little room)
    gap_scale: Counter<f32>,
    /// Writing the content of a layout column (a borderless cell of our own): its gaps
    /// may shrink with the page's
    in_layout: Counter<bool>,
}

impl Writer<'_> {
    fn run_props(s: &Style) -> String {
        let font = esc(&s.font);
        let mut x = format!(r#"<w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}" w:eastAsia="{font}"/>"#);
        if s.bold {
            x.push_str("<w:b/><w:bCs/>");
        }
        if s.italic {
            x.push_str("<w:i/><w:iCs/>");
        }
        if let Some(c) = s.color {
            let _ = write!(x, r#"<w:color w:val="{}"/>"#, hex(c));
        }
        if s.spacing.abs() > 0.05 {
            let _ = write!(x, r#"<w:spacing w:val="{}"/>"#, twips(s.spacing));
        }
        // Word draws super/subscripts at 2/3 of the run's size: the run is given the
        // size that comes out as the PDF's
        let size = if s.vert != 0 { s.size * 1.5 } else { s.size };
        let half_points = (size * 2.0).round().max(2.0) as i32;
        let _ = write!(x, r#"<w:sz w:val="{half_points}"/><w:szCs w:val="{half_points}"/>"#);
        if s.underline {
            x.push_str(r#"<w:u w:val="single"/>"#);
        }
        match s.vert {
            1 => x.push_str(r#"<w:vertAlign w:val="superscript"/>"#),
            -1 => x.push_str(r#"<w:vertAlign w:val="subscript"/>"#),
            _ => {}
        }
        x.push_str("</w:rPr>");
        x
    }

    /// '\t' becomes a tab, '\n' a line break
    fn text_run(text: &str, s: &Style) -> String {
        let props = Self::run_props(s);
        let mut x = String::new();
        for (l, line) in text.split('\n').enumerate() {
            if l > 0 {
                let _ = write!(x, "<w:r>{props}<w:br/></w:r>");
            }
            for (i, part) in line.split('\t').enumerate() {
                if i > 0 {
                    let _ = write!(x, "<w:r>{props}<w:tab/></w:r>");
                }
                if !part.is_empty() {
                    let _ = write!(x, r#"<w:r>{props}<w:t xml:space="preserve">{}</w:t></w:r>"#, esc(part));
                }
            }
        }
        x
    }

    fn field(instr: &str, shown: &str, s: &Style) -> String {
        let props = Self::run_props(s);
        format!(
            r#"<w:r>{props}<w:fldChar w:fldCharType="begin"/></w:r><w:r>{props}<w:instrText xml:space="preserve"> {instr} </w:instrText></w:r><w:r>{props}<w:fldChar w:fldCharType="separate"/></w:r><w:r>{props}<w:t>{shown}</w:t></w:r><w:r>{props}<w:fldChar w:fldCharType="end"/></w:r>"#
        )
    }

    fn runs(text: &str, s: &Style, fields: Option<Fields>) -> String {
        let Some(f) = fields else { return Self::text_run(text, s) };
        split_numbers(text, f.page, f.total)
            .into_iter()
            .map(|p| match p {
                Piece::Text(t) => Self::text_run(t, s),
                Piece::Page => Self::field("PAGE", &f.page.to_string(), s),
                Piece::Pages => Self::field("NUMPAGES", &f.total.to_string(), s),
            })
            .collect()
    }

    /// An empty paragraph of a given height: spacing, page breaks and section ends
    fn spacer(height: f32, page_break: bool, sect: &str) -> String {
        let mut x = String::from("<w:p><w:pPr>");
        if page_break {
            x.push_str("<w:pageBreakBefore/>");
        }
        let _ = write!(x, r#"<w:spacing w:before="0" w:after="0" w:line="{}" w:lineRule="exact"/>"#, twips(height.max(0.5)).max(10));
        x.push_str(r#"<w:rPr><w:sz w:val="2"/><w:szCs w:val="2"/></w:rPr>"#);
        x.push_str(sect);
        x.push_str("</w:pPr></w:p>");
        x
    }

    /// `keep_lines`: the PDF's line breaks stay (table cells: Word then needs exactly as
    /// many lines as the PDF, whatever the font metrics); otherwise the text reflows.
    #[allow(clippy::too_many_arguments)]
    fn paragraph(&self, lines: &[Line], col_left: f32, col_right: f32, before: f32, page_break: bool, fields: Option<Fields>, keep_lines: bool) -> String {
        let first = &lines[0];
        let pitch = line_pitch(lines);

        // Alignment from where the lines sit
        let widest = lines.iter().map(|l| l.right - l.left).fold(0.0, f32::max);
        let column_width = (col_right - col_left).max(1.0);
        // Centred: equal room on both sides of every line, clearly away from the margin,
        // and (for several lines) ragged on both sides rather than sharing one left edge
        // In a table cell (kept lines) text is often centred in a narrow column, where
        // the widest line nearly touches both sides
        let balanced = lines.iter().all(|l| {
            let (left_room, right_room) = (l.left - col_left, col_right - l.right);
            let tolerance = if keep_lines { (l.size * 1.2).max((left_room + right_room) * 0.25) } else { l.size * 1.2 };
            (left_room - right_room).abs() < tolerance && (left_room > l.size * 2.0 || (keep_lines && lines.len() > 1))
        });
        let lefts_vary = lines.len() == 1 || {
            let min = lines.iter().map(|l| l.left).fold(f32::MAX, f32::min);
            let max = lines.iter().map(|l| l.left).fold(f32::MIN, f32::max);
            max - min > first.size * 0.5
        };
        let tabbed = lines.iter().any(|l| !l.tabs.is_empty());
        let centred = !tabbed && balanced && lefts_vary && widest < column_width * 0.95;
        let right = !centred && !tabbed && lines.iter().all(|l| (col_right - l.right).abs() < l.size) && first.left > col_left + first.size * 2.0;
        let justified = lines.len() > 2 && lines[..lines.len() - 1].iter().all(|l| (col_right - l.right).abs() < l.size * 0.6);
        let jc = if centred {
            "center"
        } else if right {
            "right"
        } else if justified {
            "both"
        } else {
            "left"
        };

        let mut ppr = String::new();
        if page_break {
            ppr.push_str("<w:pageBreakBefore/>");
        }
        // Tab stops where the text after each tab started (or ended, for right-aligned
        // page numbers and anything with dot leaders)
        let mut stops: Vec<(i32, &str, bool)> = vec![];
        for (i, t) in first.tabs.iter().enumerate() {
            let last = i + 1 == first.tabs.len();
            let to_right = t.leader || (last && col_right - t.right < first.size * 1.5 && t.right - t.left < column_width * 0.3);
            let pos = if to_right { twips(t.right - col_left) } else { twips(t.left - col_left) };
            if stops.last().is_some_and(|(p, _, _)| *p >= pos) || pos <= 0 {
                continue;
            }
            stops.push((pos, if to_right { "right" } else { "left" }, t.leader));
        }
        if !stops.is_empty() {
            ppr.push_str("<w:tabs>");
            for (pos, kind, leader) in &stops {
                let leader = if *leader { r#" w:leader="dot""# } else { "" };
                let _ = write!(ppr, r#"<w:tab w:val="{kind}"{leader} w:pos="{pos}"/>"#);
            }
            ppr.push_str("</w:tabs>");
        }
        let _ = write!(ppr, r#"<w:spacing w:before="{}" w:after="0" w:line="{}" w:lineRule="exact"/>"#, twips(before.max(0.0)), twips(pitch).max(20));
        if jc == "left" || jc == "both" {
            // Indents relative to the text column: hanging for list items, first-line otherwise
            let text_left = if keep_lines && lines.len() > 1 {
                // Every kept line must fit: indent to the leftmost of the wrapped lines
                // (the first line keeps its own start through a hanging/first-line indent)
                lines.iter().skip(1).map(|l| l.left).fold(f32::MAX, f32::min)
            } else if lines.len() > 1 {
                lines[1].left
            } else if first.tabs.len() == 1 && stops.len() == 1 && stops[0].1 == "left" && !first.tabs[0].leader {
                // A single list item line: wrapped text would align after the marker
                first.tabs[0].left
            } else {
                first.left
            };
            let left = twips((text_left - col_left).max(0.0));
            let offset = twips(first.left - text_left);
            let special = if offset < -10 {
                format!(r#" w:hanging="{}""#, -offset.max(-left))
            } else if offset > 10 {
                format!(r#" w:firstLine="{offset}""#)
            } else {
                String::new()
            };
            if left > 10 || !special.is_empty() {
                let _ = write!(ppr, r#"<w:ind w:left="{left}"{special}/>"#);
            }
        }
        let _ = write!(ppr, r#"<w:jc w:val="{jc}"/>"#);

        // Join lines; undo end-of-line hyphenation ("conver-" + "sion")
        let mut runs: Vec<(String, Style)> = vec![];
        for (i, line) in lines.iter().enumerate() {
            for (j, (text, style)) in line.runs.iter().enumerate() {
                let mut text = text.clone();
                if j == 0 && i > 0 && keep_lines {
                    text.insert(0, '\n');
                } else if j == 0 && i > 0 {
                    let prev_hyphen = runs.last().is_some_and(|(t, _)| t.ends_with('-') && t.len() > 1);
                    let next_lower = text.chars().next().is_some_and(char::is_lowercase);
                    if prev_hyphen && next_lower {
                        if let Some((t, _)) = runs.last_mut() {
                            t.pop();
                        }
                    } else {
                        text.insert(0, ' ');
                    }
                }
                match runs.last_mut() {
                    Some((t, s)) if s == style => t.push_str(&text),
                    _ => runs.push((text, style.clone())),
                }
            }
        }
        let mut x = format!("<w:p><w:pPr>{ppr}</w:pPr>");
        for (text, style) in &runs {
            x.push_str(&Self::runs(text, style, fields));
        }
        x.push_str("</w:p>");
        x
    }

    fn picture(&self, part: &mut Part, img: &ImageRef, width: f32, height: f32) -> String {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let rel = part.image_rel(img.media);
        let (cx, cy) = (emu(width), emu(height));
        let ext = self.media.items[img.media].ext;
        format!(
            r#"<w:r><w:rPr><w:sz w:val="2"/><w:szCs w:val="2"/></w:rPr><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="Picture {id}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="image{}.{ext}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rel}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
            img.media
        )
    }

    /// A page-sized picture behind the text, fixed to the page's top-left corner
    fn background(&self, part: &mut Part, media: usize, width: f32, height: f32) -> String {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let rel = part.image_rel(media);
        let (cx, cy) = (emu(width), emu(height));
        format!(
            r#"<w:r><w:drawing><wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="1" behindDoc="1" locked="1" layoutInCell="0" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="page"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="page"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapNone/><wp:docPr id="{id}" name="Background {id}"/><wp:cNvGraphicFramePr/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{id}" name="background{media}.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rel}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"#
        )
    }

    /// Pictures side by side: one paragraph, the pictures placed with tab stops
    fn pictures(&self, part: &mut Part, row: &[ImageRef], col_left: f32, col_right: f32, before: f32, page_break: bool) -> String {
        let width = (col_right - col_left).max(36.0);
        // Too wide for the column (pictures into the margin): scale everything down
        let span = row.last().map(|i| i.rect.r).unwrap_or(0.0) - row[0].rect.l.min(col_left.max(row[0].rect.l));
        let k = if span > width && span > 0.0 { width / span } else { 1.0 };
        // A hair smaller (1.5 pt in height): a page filled to the last point with
        // pictures in the PDF must not spill onto the next page in Word
        let tallest = row.iter().map(|i| i.rect.h()).fold(0.0, f32::max) * k;
        let k = if tallest > 20.0 { k * (tallest - 1.5) / tallest } else { k };
        let mut ppr = String::new();
        if page_break {
            ppr.push_str("<w:pageBreakBefore/>");
        }
        let first_left = ((row[0].rect.l - col_left) * k).max(0.0);
        if row.len() > 1 {
            ppr.push_str("<w:tabs>");
            let mut last = twips(first_left);
            for img in &row[1..] {
                let pos = twips(((img.rect.l - col_left) * k).max(0.0)).max(last + 20);
                let _ = write!(ppr, r#"<w:tab w:val="left" w:pos="{pos}"/>"#);
                last = pos;
            }
            ppr.push_str("</w:tabs>");
        }
        let _ = write!(ppr, r#"<w:spacing w:before="{}" w:after="0" w:line="240" w:lineRule="auto"/>"#, twips(before.max(0.0)));
        if first_left > 0.5 {
            let _ = write!(ppr, r#"<w:ind w:left="{}"/>"#, twips(first_left));
        }
        ppr.push_str(r#"<w:jc w:val="left"/><w:rPr><w:sz w:val="2"/><w:szCs w:val="2"/></w:rPr>"#);
        let mut x = format!("<w:p><w:pPr>{ppr}</w:pPr>");
        for (i, img) in row.iter().enumerate() {
            if i > 0 {
                x.push_str(r#"<w:r><w:rPr><w:sz w:val="2"/></w:rPr><w:tab/></w:r>"#);
            }
            x.push_str(&self.picture(part, img, img.rect.w() * k, img.rect.h() * k));
        }
        x.push_str("</w:p>");
        x
    }

    fn border(side: &str, b: Option<Border>) -> String {
        match b {
            Some(b) => format!(r#"<w:{side} w:val="single" w:sz="{}" w:space="0" w:color="{}"/>"#, ((b.width * 8.0).round() as i32).clamp(2, 96), hex(b.color)),
            None => format!(r#"<w:{side} w:val="nil"/>"#),
        }
    }

    fn table(&self, part: &mut Part, t: &Table, col_left: f32, fields: Option<Fields>) -> String {
        let ncols = t.xs.len() - 1;
        let nrows = t.ys.len() - 1;
        // Grid columns in twips, rounded so they add up to the table width exactly
        let edges: Vec<i32> = t.xs.iter().map(|x| twips(*x)).collect();
        let cols: Vec<i32> = edges.windows(2).map(|w| (w[1] - w[0]).max(20)).collect();
        let total: i32 = cols.iter().sum();
        let pad = twips(t.padding);
        let mut x = String::from("<w:tbl><w:tblPr>");
        let _ = write!(x, r#"<w:tblW w:w="{total}" w:type="dxa"/><w:tblInd w:w="{}" w:type="dxa"/>"#, twips(t.xs[0] - col_left));
        // No table-level borders at all (the default table style has none): written as
        // "nil", other apps (Pages, Quick Look, …) let them hide every cell's own border
        let _ = write!(
            x,
            r#"<w:tblLayout w:type="fixed"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="{pad}" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="{pad}" w:type="dxa"/></w:tblCellMar><w:tblLook w:val="0000" w:firstRow="0" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="0"/></w:tblPr><w:tblGrid>"#
        );
        for c in &cols {
            let _ = write!(x, r#"<w:gridCol w:w="{c}"/>"#);
        }
        x.push_str("</w:tblGrid>");

        for r in 0..nrows {
            // The PDF's rows run from border centre to border centre; Word adds half of
            // the borders to a row's minimum height, so they are taken off here (plus a
            // hair, so rounding never pushes a full page onto the next one)
            let border = |c: &Cell, side: usize| c.borders[side].map(|b| b.width).unwrap_or(0.0);
            let in_row = t.cells.iter().filter(|c| c.row <= r && r < c.row + c.rows);
            let (top_w, bottom_w) = in_row.fold((0.0f32, 0.0f32), |(a, b), c| (a.max(if c.row == r { border(c, 0) } else { 0.0 }), b.max(if c.row + c.rows == r + 1 { border(c, 2) } else { 0.0 })));
            let height = t.ys[r + 1] - t.ys[r] - (top_w + bottom_w) / 2.0 - 0.3;
            // A layout table (borderless columns of our own) takes the height of its
            // content: no minimum that could outgrow a page filled to its last point
            let layout = t.cells.iter().all(|c| c.text_right.is_some());
            if layout {
                x.push_str("<w:tr>");
            } else {
                let _ = write!(x, r#"<w:tr><w:trPr><w:trHeight w:val="{}" w:hRule="atLeast"/></w:trPr>"#, twips(height).max(20));
            }
            let mut row: Vec<&Cell> = t.cells.iter().filter(|c| c.row <= r && r < c.row + c.rows).collect();
            row.sort_by_key(|c| c.col);
            let mut next_col = 0;
            for cell in row {
                // Gaps in the grid (should not happen): an empty cell keeps rows aligned
                while next_col < cell.col {
                    let _ = write!(x, r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr>{}</w:tc>"#, cols[next_col], Self::spacer(1.0, false, ""));
                    next_col += 1;
                }
                next_col = cell.col + cell.cols;
                let width: i32 = cols[cell.col..(cell.col + cell.cols).min(ncols)].iter().sum();
                let starts = r == cell.row;
                let ends = r + 1 == cell.row + cell.rows;
                let _ = write!(x, r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>"#);
                if cell.cols > 1 {
                    let _ = write!(x, r#"<w:gridSpan w:val="{}"/>"#, cell.cols);
                }
                if cell.rows > 1 {
                    x.push_str(if starts { r#"<w:vMerge w:val="restart"/>"# } else { "<w:vMerge/>" });
                }
                let [top, right, bottom, left] = cell.borders;
                x.push_str("<w:tcBorders>");
                x.push_str(&Self::border("top", if starts { top } else { None }));
                x.push_str(&Self::border("left", left));
                x.push_str(&Self::border("bottom", if ends { bottom } else { None }));
                x.push_str(&Self::border("right", right));
                x.push_str("</w:tcBorders>");
                if let Some(fill) = cell.fill {
                    let _ = write!(x, r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#, hex(fill));
                }
                match cell.valign {
                    VAlign::Center => x.push_str(r#"<w:vAlign w:val="center"/>"#),
                    VAlign::Bottom => x.push_str(r#"<w:vAlign w:val="bottom"/>"#),
                    VAlign::Top => {}
                }
                x.push_str("</w:tcPr>");
                if starts && !cell.blocks.is_empty() {
                    let inner_left = cell.rect.l + t.padding;
                    let inner_right = cell.text_right.unwrap_or(cell.rect.r - t.padding).min(cell.rect.r - t.padding);
                    // Text starts below the cell's top border
                    let start = if cell.valign == VAlign::Top { Some(cell.rect.t + top.map(|b| b.width).unwrap_or(0.0)) } else { None };
                    let layout = cell.text_right.is_some();
                    let outer = self.in_layout.replace(layout);
                    // A layout column holds running text: judged like the page's text
                    // (only a real table cell keeps its exact line breaks)
                    let body = self.blocks(part, &cell.blocks, inner_left, inner_right, start, false, fields, !layout);
                    self.in_layout.set(outer);
                    x.push_str(&body);
                    // A cell must end with a paragraph
                    if matches!(cell.blocks.last().map(|b| &b.content), Some(Content::Table(_))) {
                        x.push_str(&Self::spacer(1.0, false, ""));
                    }
                } else {
                    x.push_str(&Self::spacer(1.0, false, ""));
                }
                x.push_str("</w:tc>");
            }
            while next_col < ncols {
                let _ = write!(x, r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr>{}</w:tc>"#, cols[next_col], Self::spacer(1.0, false, ""));
                next_col += 1;
            }
            x.push_str("</w:tr>");
        }
        x.push_str("</w:tbl>");
        x
    }

    /// Blocks of one area, top to bottom, each placed by its gap to the one above.
    /// `start`: where the area's content begins (None: the first block gets no gap).
    #[allow(clippy::too_many_arguments)]
    fn blocks(&self, part: &mut Part, blocks: &[Block], col_left: f32, col_right: f32, start: Option<f32>, page_break: bool, fields: Option<Fields>, keep_lines: bool) -> String {
        let mut x = String::new();
        let mut previous = start;
        let mut pending_break = page_break;
        let mut after_table = false;
        // Overlaps (negative gaps) are carried on, so they do not add up into drift
        let mut debt = 0.0f32;
        for block in blocks {
            let raw = previous.map(|p| block.rect.t - p).unwrap_or(0.0) + debt;
            debt = raw.min(0.0);
            let gap = raw.max(0.0) * if !keep_lines || self.in_layout.get() { self.gap_scale.get() } else { 1.0 };
            match &block.content {
                Content::Paragraph(lines) => {
                    x.push_str(&self.paragraph(lines, col_left, col_right, gap, pending_break, fields, keep_lines));
                    after_table = false;
                }
                Content::Images(row) => {
                    x.push_str(&self.pictures(part, row, col_left, col_right, gap, pending_break));
                    after_table = false;
                }
                Content::Table(t) => {
                    // Tables take no "space before": an empty paragraph of that height
                    // instead. One is also needed between two tables (Word would join
                    // them) and to carry a page break.
                    if gap >= 1.0 || after_table || pending_break {
                        x.push_str(&Self::spacer(gap.max(1.0), pending_break, ""));
                    }
                    x.push_str(&self.table(part, t, col_left, fields));
                    after_table = true;
                }
            }
            pending_break = false;
            previous = Some(block.rect.b);
        }
        if pending_break {
            x.push_str(&Self::spacer(1.0, true, ""));
        }
        x
    }
}

/// Page setup shared by consecutive pages
struct Section {
    pages: Vec<usize>,
    width: f32,
    height: f32,
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
    header_distance: f32,
    footer_distance: f32,
    header: usize,
    footer: usize,
    /// Number printed on the section's first page
    first_number: usize,
}

fn running_signature(blocks: &[Block], page: usize, total: usize) -> String {
    blocks.iter().map(|b| signature(b, page, total, true)).collect::<Vec<_>>().join("\u{1}")
}

pub(super) fn write(pages: &[PageModel], media: &MediaStore) -> Result<Vec<u8>, String> {
    let total = pages.len();
    let w = Writer { media, next_id: Counter::new(1), gap_scale: Counter::new(1.0), in_layout: Counter::new(false) };

    // Header/footer parts, shared by pages showing the same content. Part 0 is empty,
    // so that a section without one does not inherit the previous section's.
    let mut header_parts: Vec<(String, Part)> = vec![(String::new(), Part::default())];
    let mut footer_parts: Vec<(String, Part)> = vec![(String::new(), Part::default())];
    let mut header_index: HashMap<String, usize> = HashMap::new();
    let mut footer_index: HashMap<String, usize> = HashMap::new();

    // Group pages into sections
    let mut sections: Vec<Section> = vec![];
    for (i, p) in pages.iter().enumerate() {
        let head_sig = running_signature(&p.header, p.number, total);
        let foot_sig = running_signature(&p.footer, p.number, total);
        let same = sections.last().is_some_and(|s| {
            let q = &pages[s.pages[0]];
            (q.width - p.width).abs() < 1.0
                && (q.height - p.height).abs() < 1.0
                && running_signature(&q.header, q.number, total) == head_sig
                && running_signature(&q.footer, q.number, total) == foot_sig
        });
        if same {
            sections.last_mut().unwrap().pages.push(i);
            continue;
        }
        sections.push(Section {
            pages: vec![i],
            width: p.width,
            height: p.height,
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
            header_distance: 0.0,
            footer_distance: 0.0,
            header: 0,
            footer: 0,
            first_number: p.number,
        });
    }

    // Margins: the body's extent on the section's pages
    for s in &mut sections {
        let (pw, ph) = (s.width, s.height);
        let body = || s.pages.iter().flat_map(|&i| pages[i].body.iter());
        // The text column must hold the header and footer too (a footer note often
        // sits further right than any body text)
        let all = || s.pages.iter().flat_map(|&i| pages[i].body.iter().chain(&pages[i].header).chain(&pages[i].footer));
        let left = all().map(|b| b.rect.l).fold(f32::MAX, f32::min);
        let right = all().map(|b| b.rect.r).fold(f32::MIN, f32::max);
        let top = s.pages.iter().filter_map(|&i| pages[i].body.first().map(|b| b.rect.t)).fold(f32::MAX, f32::min);
        let bottom = body().map(|b| b.rect.b).fold(f32::MIN, f32::max);
        s.left = if left == f32::MAX { 72.0 } else { left.clamp(7.2, pw / 3.0) };
        // 3 pt more room on the right than the PDF's text used: a line that just fitted
        // there (justified text) must not wrap into an extra line in Word
        s.right = if right == f32::MIN { 72.0 } else { (pw - right - 3.0).clamp(7.2, pw / 3.0) };
        s.top = if top == f32::MAX { 72.0 } else { top.clamp(7.2, ph / 2.0) };
        let first = &pages[s.pages[0]];
        let header_top = first.header.iter().map(|b| b.rect.t).fold(f32::MAX, f32::min);
        let footer_top = first.footer.iter().map(|b| b.rect.t).fold(f32::MAX, f32::min);
        let footer_bottom = first.footer.iter().map(|b| b.rect.b).fold(f32::MIN, f32::max);
        s.header_distance = if header_top == f32::MAX { 0.0 } else { header_top.max(0.0) };
        // The footer sits a hair lower (3 pt) than in the PDF, which gives a page filled
        // to the last point in the PDF that much room to spare in Word
        const SLACK: f32 = 3.0;
        s.footer_distance = if footer_bottom == f32::MIN { 0.0 } else { (ph - footer_bottom - SLACK).max(0.0) };
        // As much room at the bottom as possible, so a slightly longer page in Word does
        // not spill onto a page of its own; but never into the footer
        let free = if bottom == f32::MIN { ph / 2.0 } else { ph - bottom - 1.0 };
        s.bottom = if footer_top == f32::MAX { 7.2f32.min(free) } else { (ph - footer_top + 0.5 - SLACK).min(free) }.max(2.0);
        // Without a header/footer the (empty) one must not push the body away
        if header_top == f32::MAX {
            s.header_distance = (s.top - 2.0).max(0.0);
        }
        if footer_bottom == f32::MIN {
            s.footer_distance = (s.bottom - 2.0).max(0.0);
        }

        let first_page = pages[s.pages[0]].number;
        let fields = Some(Fields { page: first_page, total });
        let (col_left, col_right) = (s.left, pw - s.right);
        if !first.header.is_empty() {
            // Shared only by sections with the same text column: indents are relative to it
            let sig = format!("{}|{col_left:.1}|{col_right:.1}", running_signature(&first.header, first_page, total));
            s.header = match header_index.get(&sig) {
                Some(&i) => i,
                None => {
                    let mut part = Part::default();
                    let xml = w.blocks(&mut part, &first.header, col_left, col_right, Some(s.header_distance), false, fields, false);
                    header_parts.push((xml, part));
                    header_index.insert(sig, header_parts.len() - 1);
                    header_parts.len() - 1
                }
            };
        }
        if !first.footer.is_empty() {
            let sig = format!("{}|{col_left:.1}|{col_right:.1}", running_signature(&first.footer, first_page, total));
            s.footer = match footer_index.get(&sig) {
                Some(&i) => i,
                None => {
                    let mut part = Part::default();
                    let xml = w.blocks(&mut part, &first.footer, col_left, col_right, Some(footer_top), false, fields, false);
                    footer_parts.push((xml, part));
                    footer_index.insert(sig, footer_parts.len() - 1);
                    footer_parts.len() - 1
                }
            };
        }
    }

    // Body
    let mut doc_part = Part::default();
    let mut body = String::new();
    for (si, s) in sections.iter().enumerate() {
        let (col_left, col_right) = (s.left, s.width - s.right);
        for (k, &pi) in s.pages.iter().enumerate() {
            let page_break = k > 0;
            let page = &pages[pi];
            let blocks = &page.body;
            // A page filled almost to its bottom edge: the gaps between its blocks shrink
            // just enough to leave 10 pt to spare (text and pictures keep their size)
            let bottom = blocks.iter().map(|b| b.rect.b).fold(f32::MIN, f32::max);
            let limit = s.height - s.bottom - 10.0;
            let mut scale = 1.0;
            if bottom > limit {
                fn gaps(blocks: &[Block], start: f32) -> f32 {
                    let mut previous = start;
                    blocks
                        .iter()
                        .map(|b| {
                            let mut g = (b.rect.t - previous).max(0.0);
                            previous = previous.max(b.rect.b);
                            // Layout columns shrink with the page: their tallest column counts
                            if let Content::Table(t) = &b.content {
                                // (the one reaching lowest is the one that must fit)
                                let lowest = t
                                    .cells
                                    .iter()
                                    .filter(|c| c.text_right.is_some())
                                    .max_by(|a, b| {
                                        let bottom = |c: &&Cell| c.blocks.iter().map(|b| b.rect.b).fold(f32::MIN, f32::max);
                                        bottom(a).partial_cmp(&bottom(b)).unwrap_or(std::cmp::Ordering::Equal)
                                    });
                                g += lowest.map(|c| gaps(&c.blocks, c.rect.t)).unwrap_or(0.0);
                            }
                            g
                        })
                        .sum()
                }
                let gaps = gaps(blocks, s.top);
                if gaps > 0.0 {
                    scale = ((gaps - (bottom - limit)) / gaps).clamp(0.5, 1.0);
                }
            }
            w.gap_scale.set(scale);
            let mut xml = if blocks.is_empty() {
                Writer::spacer(1.0, page_break, "")
            } else {
                w.blocks(&mut doc_part, blocks, col_left, col_right, Some(s.top), page_break, None, false)
            };
            w.gap_scale.set(1.0);
            // The page's decoration rides on its first paragraph (a table needs one
            // in front of it to carry it)
            if let Some(media) = page.background {
                let anchor = w.background(&mut doc_part, media, page.width, page.height);
                if !xml.starts_with("<w:p>") {
                    xml = format!("{}{xml}", Writer::spacer(1.0, page_break && !xml.contains("<w:pageBreakBefore/>"), ""));
                }
                if let Some(at) = xml.find("</w:pPr>") {
                    xml.insert_str(at + "</w:pPr>".len(), &anchor);
                }
            }
            body.push_str(&xml);
        }
        let sect = section_xml(s);
        if si + 1 < sections.len() {
            // A section ends with a paragraph that carries its settings
            body.push_str(&Writer::spacer(1.0, false, &sect));
        } else {
            body.push_str(&sect);
        }
    }

    // Package
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let doc_xml = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W_NS}><w:body>{body}</w:body></w:document>"#);
    files.push(("word/document.xml".into(), doc_xml.into_bytes()));
    let mut doc_rels = String::from(
        r#"<Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rIdSettings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>"#,
    );
    let mut overrides = String::new();
    for (kind, parts) in [("header", &header_parts), ("footer", &footer_parts)] {
        for (i, (xml, part)) in parts.iter().enumerate() {
            let name = format!("{kind}{}.xml", i + 1);
            let tag = if kind == "header" { "hdr" } else { "ftr" };
            let content = if xml.is_empty() { Writer::spacer(1.0, false, "") } else { xml.clone() };
            files.push((format!("word/{name}"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{tag} {W_NS}>{content}</w:{tag}>"#).into_bytes()));
            let _ = write!(
                doc_rels,
                r#"<Relationship Id="rId{}{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{name}"/>"#,
                if kind == "header" { "H" } else { "F" },
                i
            );
            let _ = write!(overrides, r#"<Override PartName="/word/{name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml"/>"#);
            if !part.images.is_empty() {
                files.push((format!("word/_rels/{name}.rels"), rels_xml(&image_rels(part, media))));
            }
        }
    }
    doc_rels.push_str(&image_rels(&doc_part, media));
    files.push(("word/_rels/document.xml.rels".into(), rels_xml(&doc_rels)));
    for (i, m) in media.items.iter().enumerate() {
        files.push((format!("word/media/image{i}.{}", m.ext), m.data.clone()));
    }
    files.push(("word/styles.xml".into(), STYLES.as_bytes().to_vec()));
    files.push(("word/settings.xml".into(), SETTINGS.as_bytes().to_vec()));
    files.push((
        "[Content_Types].xml".into(),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>{overrides}</Types>"#
        )
        .into_bytes(),
    ));
    files.push((
        "_rels/.rels".into(),
        rels_xml(r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#),
    ));

    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in files {
            // Pictures are compressed already
            let opts = if name.starts_with("word/media/") { stored } else { options };
            zip.start_file(name, opts).map_err(|e| format!("the Word file could not be written ({e})"))?;
            zip.write_all(&data).map_err(|e| format!("the Word file could not be written ({e})"))?;
        }
        zip.finish().map_err(|e| format!("the Word file could not be written ({e})"))?;
    }
    Ok(out.into_inner())
}

fn image_rels(part: &Part, media: &MediaStore) -> String {
    let mut x = String::new();
    for &m in &part.images {
        let _ = write!(
            x,
            r#"<Relationship Id="rIdM{m}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{m}.{}"/>"#,
            media.items[m].ext
        );
    }
    x
}

fn rels_xml(inner: &str) -> Vec<u8> {
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{inner}</Relationships>"#).into_bytes()
}

fn section_xml(s: &Section) -> String {
    let orient = if s.width > s.height { r#" w:orient="landscape""# } else { "" };
    format!(
        r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdH{}"/><w:footerReference w:type="default" r:id="rIdF{}"/><w:type w:val="nextPage"/><w:pgSz w:w="{}" w:h="{}"{orient}/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="0"/><w:pgNumType w:start="{}"/><w:cols w:space="720"/><w:docGrid w:linePitch="360"/></w:sectPr>"#,
        s.header,
        s.footer,
        twips(s.width),
        twips(s.height),
        twips(s.top),
        twips(s.right),
        twips(s.bottom),
        twips(s.left),
        twips(s.header_distance),
        twips(s.footer_distance),
        s.first_number
    )
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri" w:eastAsia="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:widowControl w:val="0"/><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style></w:styles>"#;

const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>"#;
