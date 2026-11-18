//! Smart layout: sheets nobody set up for printing

use super::*;

/// Smallest scale a sheet is shrunk to so that all its columns fit: a PDF is mostly read
/// zoomed on a screen, where one wide table beats its columns scattered over pages.
/// Wider sheets than that continue on more pages.
pub(super) const SMART_MIN_SCALE: f32 = 0.3;

/// Fitting a portrait page is worth shrinking to this scale before turning to landscape
const SMART_PORTRAIT_SCALE: f32 = 0.85;

/// Someone chose how the sheet prints: Excel's own layout is then followed exactly
pub(super) fn is_set_up(sheet: &Sheet) -> bool {
    sheet.print.explicit || sheet.print_area.is_some() || sheet.title_rows.is_some() || sheet.title_cols.is_some() || !sheet.row_breaks.is_empty() || !sheet.col_breaks.is_empty()
}

/// Landscape when the columns don't fit across a portrait page
pub(super) fn smart_landscape(p: &crate::office::xlsx::Print, total_w: f32) -> bool {
    let room = (p.paper.0.min(p.paper.1) - p.margins[0] - p.margins[1]).max(36.0);
    total_w * SMART_PORTRAIT_SCALE > room
}

/// The rows to repeat on every page: the frozen rows, or else a table's heading row near
/// the top (all text, bold or shaded, as wide as the table below it)
pub(super) fn heading_rows(sheet: &Sheet, styles: &[Style], range: Range) -> Option<(u32, u32)> {
    let frozen = sheet.frozen.0;
    if frozen > range.0 && frozen <= range.0 + 15 && frozen <= range.2 {
        return Some((range.0, frozen - 1));
    }
    let filled = |r: u32| sheet.cells.range((r, range.1)..=(r, range.3)).filter(|(_, c)| c.value.is_some()).count();
    let last = range.2.min(range.0 + 30);
    let widest = (range.0..=last).map(filled).max().unwrap_or(0);
    if widest < 2 {
        return None;
    }
    (range.0..=range.2.min(range.0 + 10)).filter(|&r| r < range.2).find(|&r| {
        let cells: Vec<_> = sheet.cells.range((r, range.1)..=(r, range.3)).filter(|(_, c)| c.value.is_some()).collect();
        cells.len() * 2 >= widest
            && cells.len() >= 2
            && filled(r + 1) >= 2
            && cells.iter().all(|(_, c)| {
                let s = &styles[c.style];
                matches!(c.value, Some(Value::Text(_))) && (s.font.bold || s.fill.is_some())
            })
    }).map(|r| (r, r))
}
