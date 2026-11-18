//! What is worked out once every cell is read: results of formulas stored without
//! one, and the charts that plot cells

use super::*;

/// Charts, once the cells they plot hold their values
pub(super) fn read_charts(sheets: &mut [Sheet], styles: &[Style], theme: &[Rgb], date1904: bool) {
    let parts: Vec<Vec<ChartPart>> = sheets.iter_mut().map(|s| std::mem::take(&mut s.chart_parts)).collect();
    if parts.iter().all(|p| p.is_empty()) {
        return;
    }
    let sep = numfmt::Separators::system();
    for (si, list) in parts.into_iter().enumerate() {
        let mut added = 0;
        for (at, anchor, xml) in list {
            let data = |f: &str| -> Vec<crate::office::chart::CellValue> {
                let crate::office::formula::Expr::Area(a) = crate::office::formula::parse(f) else { return vec![] };
                let target = match &a.sheet {
                    Some(name) => sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name)),
                    None => Some(si),
                };
                let Some(sheet) = target.map(|t| &sheets[t]) else { return vec![] };
                let last = sheet.cells.keys().next_back().map(|k| k.0).unwrap_or(0);
                let (r2, c2) = (a.r2.min(last), a.c2.min(a.c1 + 16_383));
                let mut out = vec![];
                for r in a.r1..=r2 {
                    for c in a.c1..=c2 {
                        let cell = sheet.cells.get(&(r, c));
                        let value = cell.and_then(|c| c.value.as_ref());
                        let n = match value {
                            Some(Value::Number(n)) => Some(*n),
                            _ => None,
                        };
                        let code = cell.map(|c| styles[c.style].num_fmt.as_str()).unwrap_or("General");
                        let shown = value.map(|v| numfmt::format(v, code, date1904, sep).text).unwrap_or_default();
                        let shown: String = shown.chars().filter(|c| *c != numfmt::HIDE_ON && *c != numfmt::HIDE_OFF).collect();
                        out.push((n, shown.trim().to_string()));
                        if out.len() > 100_000 {
                            return out;
                        }
                    }
                }
                out
            };
            if let Some(chart) = crate::office::chart::parse(&xml, theme, &data) {
                let at = (at + added).min(sheets[si].drawn.len());
                sheets[si].drawn.insert(at, Drawn::Chart(anchor, Box::new(chart)));
                added += 1;
            }
        }
    }
}

/// Fills in the results of formulas stored without one, top to bottom
pub(super) fn work_out_formulas(sheets: &mut [Sheet], date1904: bool) {
    if sheets.iter().all(|s| s.formulas.is_empty()) {
        return;
    }
    let results: Vec<(usize, (u32, u32), Option<Value>)> = {
        let book = crate::office::formula::Book::new(
            sheets.iter().map(|s| s.name.clone()).collect(),
            sheets.iter().map(|s| &s.cells).collect(),
            sheets.iter().map(|s| &s.formulas).collect(),
            date1904,
        );
        let mut out = vec![];
        for (si, s) in sheets.iter().enumerate() {
            let mut keys: Vec<(u32, u32)> = s.formulas.keys().copied().collect();
            keys.sort_unstable();
            out.extend(keys.into_iter().map(|(r, c)| (si, (r, c), book.result(si, r, c).into_value())));
        }
        out
    };
    for (si, pos, value) in results {
        if let Some(value) = value.filter(|v| !matches!(v, Value::Text(t) if t.is_empty())) {
            sheets[si].cells.entry(pos).or_insert(Cell { value: None, style: 0 }).value = Some(value);
        }
    }
    for s in sheets {
        s.formulas = HashMap::new();
    }
}

pub(super) fn strip_sheet(r: &str) -> &str {
    r.rsplit_once('!').map(|(_, r)| r).unwrap_or(r).trim()
}

pub(super) fn refers_to_sheet(text: &str, sheet: &str) -> bool {
    let quoted = format!("'{}'!", sheet.replace('\'', "''"));
    text.starts_with(&quoted) || text.starts_with(&format!("{sheet}!"))
}
