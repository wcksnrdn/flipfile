//! Paragraphs: font size, line height, spacing, indents, borders and shading

use super::*;

impl<'d> Gen<'d> {
    pub(super) fn main_size(&self, p: &Paragraph) -> f32 {
        p.inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Text(_, r) => Some(r.size),
                _ => None,
            })
            .fold(None, |acc: Option<f32>, s| Some(acc.map_or(s, |a| a.max(s))))
            .or(p.marker.as_ref().map(|m| m.run.size))
            .unwrap_or(p.mark.size)
    }

    /// Word's line box for this paragraph: (height of every line, distance from the
    /// line's top to the baseline), in points. Word makes each line exactly this tall
    /// and puts the extra height of 1.5/double/exact spacing above the text; the text
    /// edges below reproduce that, so Typst lines need no leading at all.
    pub(super) fn line_box(&self, p: &Paragraph) -> (f32, f32) {
        let size = self.main_size(p);
        let font = p
            .inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Text(_, r) if (r.size - size).abs() < 0.01 => r.font.clone(),
                _ => None,
            })
            .next()
            .or_else(|| p.mark.font.clone())
            .unwrap_or_else(|| self.doc.default_font.clone());
        let (ascent, descent) = crate::office::typeset::line_metrics(&font_names(&font));
        let single = (ascent + descent) * size;
        let height = match p.props.line {
            LineSpacing::Multiple(m) => m * single,
            LineSpacing::Exact(h) => h,
            LineSpacing::AtLeast(h) => h.max(single),
        };
        (height, (height - descent * size).max(0.0))
    }

    pub(super) fn paragraph(&self, p: &Paragraph) -> String {
        let props = &p.props;
        let (line_height, baseline) = self.line_box(p);
        self.justify_breaks.set(p.props.align == Align::Justify && self.doc.justify_breaks);

        if p.inlines.is_empty() && p.marker.is_none() {
            // Empty paragraph: keeps its line height, like pressing Enter in Word
            return format!("#block(height: {}, width: 100%)[]", pt(line_height));
        }

        let align = match props.align {
            Align::Center => "center",
            Align::Right => "right",
            _ => "left",
        };
        let mut set = format!(
            "#set text(top-edge: {}, bottom-edge: {})\n#set par(leading: 0pt, justify: {}",
            pt(baseline),
            pt(baseline - line_height),
            props.align == Align::Justify
        );
        let left = props.indent_left.max(0.0);
        let right = props.indent_right.max(0.0);
        let (pad_left, first) = if props.first_line < 0.0 {
            let hang = -props.first_line;
            let _ = write!(set, ", hanging-indent: {}", pt(hang.min(left.max(hang))));
            ((left - hang).max(0.0), 0.0)
        } else {
            (left, props.first_line)
        };
        set.push(')');

        // A list number/bullet is just the start of the first line, followed by a tab
        // (or a space), exactly as Word lays it out.
        let mut inlines = p.inlines.clone();
        if let Some(marker) = &p.marker {
            let mut run = marker.run.clone();
            if run.font.as_deref().is_some_and(is_symbol_font) {
                run.font = None;
            }
            let mut lead = vec![Inline::Text(marker.text.clone(), run.clone())];
            match marker.suffix {
                MarkerSuffix::Tab => lead.push(Inline::Tab),
                MarkerSuffix::Space => lead.push(Inline::Text(" ".into(), run)),
                MarkerSuffix::Nothing => {}
            }
            lead.extend(inlines);
            inlines = lead;
        }
        let extra_stop = (props.first_line < 0.0).then_some(left);
        let content = self.paragraph_content(&inlines, &props.tabs, extra_stop, pad_left + first);
        // First-line indent as fixed space: Typst's own first-line indent skips a
        // block's only paragraph, and every Word paragraph is its own block here
        let indent = if first > 0.0 { format!("#box(width: {})", pt(first)) } else { String::new() };
        // An explicit `par`: Typst ignores paragraph indents for a lone paragraph in a block
        // A strut: every line is at least Word's line box, even one holding only a
        // floating picture or a tab
        let strut = format!("#box(width: 0pt, height: {}, baseline: {})", pt(line_height), pt(line_height - baseline));
        let body = format!("#pad(left: {}, right: {})[{set}\n#align({align})[#par[{strut}{indent}{content}]]]", pt(pad_left), pt(right));

        match props.fill {
            Some(fill) => format!("#block(width: 100%, fill: {}, inset: (x: 2pt, y: 1pt))[{body}]", rgb(fill)),
            None => format!("#block(width: 100%)[{body}]"),
        }
    }
}
