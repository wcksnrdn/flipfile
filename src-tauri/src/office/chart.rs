// Excel charts, drawn the way Excel draws them: column and bar (clustered, stacked,
// 100%), line, area, pie and doughnut, and scatter charts, with their title, legend,
// axes, gridlines and data labels, in the series' own colours or the theme's.
//
// A chart's numbers come from its cached copy of the cells it plots, or from the cells
// themselves when the file has no cache (charts written by libraries).

use std::fmt::Write;

use roxmltree::{Document, Node};

use super::drawing::{self, child, children};
use super::numfmt::{self, Separators, Value};
use super::xlsx::Rgb;

/// Excel's chart text grey and gridline grey
const TEXT: Rgb = [0x59, 0x59, 0x59];
const GRID: Rgb = [0xD9, 0xD9, 0xD9];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bar,
    Line,
    Area,
    Pie,
    Scatter,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grouping {
    Standard,
    Stacked,
    Percent,
}

#[derive(Clone, Debug, Default)]
pub struct Labels {
    pub value: bool,
    pub percent: bool,
    pub category: bool,
}

#[derive(Clone, Debug)]
pub struct Series {
    pub name: String,
    pub cats: Vec<String>,
    /// x values of a scatter chart
    pub xs: Vec<Option<f64>>,
    pub vals: Vec<Option<f64>>,
    pub fmt: String,
    pub color: Rgb,
    /// colours of single points (pie slices, highlighted bars)
    pub points: Vec<(usize, Rgb)>,
    pub line: Option<(Rgb, f32)>,
    pub marker: bool,
    pub labels: Labels,
}

#[derive(Clone, Debug)]
pub struct Plot {
    pub kind: Kind,
    pub grouping: Grouping,
    pub horizontal: bool,
    pub gap: f32,
    pub overlap: f32,
    pub vary_colors: bool,
    pub hole: f32,
    pub first_angle: f32,
    pub series: Vec<Series>,
}

#[derive(Clone, Debug, Default)]
pub struct Axis {
    pub deleted: bool,
    pub title: Option<String>,
    pub fmt: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub gridlines: bool,
    pub reversed: bool,
}

#[derive(Clone, Debug)]
pub struct Chart {
    pub title: Option<String>,
    pub title_size: f32,
    pub plots: Vec<Plot>,
    /// "r", "b", "t", "l" or None (no legend)
    pub legend: Option<String>,
    pub cat_axis: Axis,
    pub val_axis: Axis,
    pub background: Option<Rgb>,
    pub border: Option<Rgb>,
    pub text_size: f32,
    /// the theme's accent colours, which pie slices take in turn
    pub accents: Vec<Rgb>,
}

/// Reads a chart part. `data` returns the cells a reference like `Sheet1!$B$2:$B$9`
/// points at, as (number, shown text).
pub fn parse(xml: &str, theme: &[Rgb], data: &dyn Fn(&str) -> Vec<(Option<f64>, String)>) -> Option<Chart> {
    let doc = Document::parse(xml).ok()?;
    let space = doc.root_element();
    let chart = child(space, "chart")?;
    let area = child(chart, "plotArea")?;
    let palette = |i: usize| -> Rgb {
        let accent = theme.get(4 + i % 6).copied().unwrap_or([0x44, 0x72, 0xC4]);
        match i / 6 {
            0 => accent,
            1 => super::xlsx::tint(accent, -0.25),
            2 => super::xlsx::tint(accent, 0.4),
            _ => super::xlsx::tint(accent, -0.5),
        }
    };
    let mut plots = vec![];
    let mut index = 0;
    for p in area.children().filter(|c| c.is_element()) {
        let name = p.tag_name().name();
        let kind = match name {
            "barChart" | "bar3DChart" => Kind::Bar,
            "lineChart" | "line3DChart" | "stockChart" => Kind::Line,
            "areaChart" | "area3DChart" => Kind::Area,
            "pieChart" | "pie3DChart" | "doughnutChart" | "ofPieChart" => Kind::Pie,
            "scatterChart" => Kind::Scatter,
            _ => continue,
        };
        let val = |n: &str| child(p, n).and_then(|x| x.attribute("val"));
        let grouping = match val("grouping") {
            Some("stacked") => Grouping::Stacked,
            Some("percentStacked") => Grouping::Percent,
            _ => Grouping::Standard,
        };
        let chart_markers = val("marker").map(|v| v == "1" || v == "true").unwrap_or(false);
        let mut series = vec![];
        for s in children(p, "ser") {
            let color = palette(index);
            index += 1;
            let sp = child(s, "spPr");
            let fill = sp.and_then(|x| drawing::fill(x, theme)).flatten();
            let line = sp.and_then(|x| drawing::line(x, theme));
            let tx = child(s, "tx");
            let name = tx
                .and_then(|t| child(t, "v").and_then(|v| v.text()).map(str::to_string).or_else(|| child(t, "strRef").map(|r| values(r, data).into_iter().map(|(_, t)| t).collect::<Vec<_>>().join(" "))))
                .unwrap_or_else(|| format!("Series{index}"));
            let (cat_node, val_node) = if kind == Kind::Scatter { (child(s, "xVal"), child(s, "yVal")) } else { (child(s, "cat"), child(s, "val")) };
            let cat_data: Vec<(Option<f64>, String)> = cat_node.and_then(|c| c.children().find(|x| x.is_element())).map(|r| values(r, data)).unwrap_or_default();
            let val_data: Vec<(Option<f64>, String)> = val_node.and_then(|c| c.children().find(|x| x.is_element())).map(|r| values(r, data)).unwrap_or_default();
            let fmt = val_node
                .and_then(|v| v.descendants().find(|d| d.tag_name().name() == "formatCode"))
                .and_then(|f| f.text())
                .unwrap_or("General")
                .to_string();
            let marker = match child(s, "marker").and_then(|m| child(m, "symbol")).and_then(|x| x.attribute("val")) {
                Some("none") => false,
                Some(_) => true,
                None => chart_markers || kind == Kind::Scatter,
            };
            let points = children(s, "dPt")
                .filter_map(|d| {
                    let i: usize = child(d, "idx")?.attribute("val")?.parse().ok()?;
                    let c = child(d, "spPr").and_then(|x| drawing::fill(x, theme)).flatten()?;
                    Some((i, c))
                })
                .collect();
            let labels = labels(child(s, "dLbls").or_else(|| child(p, "dLbls")));
            let main = if kind == Kind::Line || kind == Kind::Scatter { line.flatten().map(|l| l.0).or(fill).unwrap_or(color) } else { fill.unwrap_or(color) };
            series.push(Series {
                name,
                cats: cat_data.iter().map(|(_, t)| t.clone()).collect(),
                xs: cat_data.iter().map(|(n, _)| *n).collect(),
                vals: val_data.iter().map(|(n, _)| *n).collect(),
                fmt,
                color: main,
                points,
                line: match line {
                    Some(Some(l)) => Some(l),
                    // "no line": scatter charts showing only their markers
                    Some(None) => None,
                    None => Some((main, if kind == Kind::Line || kind == Kind::Scatter { 2.25 } else { 0.0 })),
                },
                marker,
                labels,
            });
        }
        if series.is_empty() {
            continue;
        }
        let vary = val("varyColors").map(|v| v == "1" || v == "true").unwrap_or(kind == Kind::Pie);
        plots.push(Plot {
            kind,
            grouping,
            horizontal: val("barDir") == Some("bar"),
            gap: val("gapWidth").and_then(|v| v.parse().ok()).unwrap_or(150.0),
            overlap: val("overlap").and_then(|v| v.parse().ok()).unwrap_or(if grouping == Grouping::Standard { 0.0 } else { 100.0 }),
            vary_colors: vary && kind == Kind::Pie,
            hole: if name == "doughnutChart" { val("holeSize").and_then(|v| v.parse().ok()).unwrap_or(50.0) } else { 0.0 },
            first_angle: val("firstSliceAng").and_then(|v| v.parse().ok()).unwrap_or(0.0),
            series,
        });
    }
    if plots.is_empty() {
        return None;
    }
    let axis = |names: &[&str]| -> Axis {
        let Some(a) = area.children().find(|c| names.contains(&c.tag_name().name())) else { return Axis::default() };
        let v = |n: &str| child(a, n).and_then(|x| x.attribute("val"));
        let scaling = child(a, "scaling");
        let lim = |n: &str| scaling.and_then(|s| child(s, n)).and_then(|x| x.attribute("val")).and_then(|x| x.parse().ok());
        Axis {
            deleted: v("delete").is_some_and(|d| d == "1" || d == "true") || v("tickLblPos") == Some("none"),
            title: child(a, "title").and_then(|t| title_text(t, data)),
            fmt: child(a, "numFmt").filter(|f| f.attribute("sourceLinked") != Some("1")).and_then(|f| f.attribute("formatCode")).map(str::to_string),
            min: lim("min"),
            max: lim("max"),
            gridlines: child(a, "majorGridlines").is_some(),
            reversed: scaling.and_then(|s| child(s, "orientation")).and_then(|o| o.attribute("val")) == Some("maxMin"),
        }
    };
    let scatter = plots.iter().any(|p| p.kind == Kind::Scatter);
    let (cat_axis, val_axis) = if scatter {
        let mut axes = children(area, "valAx");
        let first = axes.next();
        let second = axes.next();
        let read = |a: Option<Node>| a.map(|a| {
            let v = |n: &str| child(a, n).and_then(|x| x.attribute("val"));
            Axis { deleted: v("delete") == Some("1"), title: child(a, "title").and_then(|t| title_text(t, data)), gridlines: child(a, "majorGridlines").is_some(), ..Default::default() }
        }).unwrap_or_default();
        (read(first), read(second))
    } else {
        (axis(&["catAx", "dateAx"]), axis(&["valAx"]))
    };
    let auto_deleted = child(chart, "autoTitleDeleted").and_then(|x| x.attribute("val")).is_some_and(|v| v == "1" || v == "true");
    let series_count: usize = plots.iter().map(|p| p.series.len()).sum();
    let title_node = child(chart, "title");
    let title = match title_node {
        Some(t) => title_text(t, data).or_else(|| Some(if series_count == 1 { plots[0].series[0].name.clone() } else { "Chart Title".into() })),
        None if !auto_deleted && series_count == 1 => Some(plots[0].series[0].name.clone()),
        None => None,
    };
    let size_of = |n: Option<Node>| n.and_then(|n| n.descendants().find(|d| d.tag_name().name() == "defRPr" || d.tag_name().name() == "rPr")).and_then(|r| r.attribute("sz")).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0);
    let space_pr = child(space, "spPr");
    let background = match space_pr.and_then(|s| drawing::fill(s, theme)) {
        Some(f) => f,
        None => Some([255, 255, 255]),
    };
    let border = match space_pr.and_then(|s| drawing::line(s, theme)) {
        Some(l) => l.map(|l| l.0),
        None => Some(GRID),
    };
    Some(Chart {
        title,
        title_size: size_of(title_node).unwrap_or(14.0),
        plots,
        legend: child(chart, "legend").map(|l| child(l, "legendPos").and_then(|p| p.attribute("val")).unwrap_or("r").to_string()),
        cat_axis,
        val_axis,
        background,
        border,
        text_size: size_of(child(space, "txPr")).unwrap_or(9.0),
        accents: (0..6).map(palette).collect(),
    })
}

fn labels(d: Option<Node>) -> Labels {
    let Some(d) = d else { return Labels::default() };
    let on = |n: &str| child(d, n).and_then(|x| x.attribute("val")).is_some_and(|v| v == "1" || v == "true");
    if on("delete") {
        return Labels::default();
    }
    Labels { value: on("showVal"), percent: on("showPercent"), category: on("showCatName") }
}

fn title_text(t: Node, data: &dyn Fn(&str) -> Vec<(Option<f64>, String)>) -> Option<String> {
    let tx = child(t, "tx")?;
    if let Some(rich) = child(tx, "rich") {
        let lines: Vec<String> = children(rich, "p").map(|p| p.descendants().filter(|d| d.tag_name().name() == "t").filter_map(|d| d.text()).collect::<String>()).collect();
        let s = lines.join("\n");
        return (!s.trim().is_empty()).then_some(s);
    }
    child(tx, "strRef").map(|r| values(r, data).into_iter().map(|(_, t)| t).collect::<Vec<_>>().join(" "))
}

/// The points of a `numRef` / `strRef` / `numLit` / `strLit` / `multiLvlStrRef`
fn values(r: Node, data: &dyn Fn(&str) -> Vec<(Option<f64>, String)>) -> Vec<(Option<f64>, String)> {
    let cache = r.children().find(|c| matches!(c.tag_name().name(), "numCache" | "strCache" | "multiLvlStrCache")).or_else(|| {
        matches!(r.tag_name().name(), "numLit" | "strLit").then_some(r)
    });
    if let Some(cache) = cache {
        // multi-level categories: the innermost level
        let level = children(cache, "lvl").next().unwrap_or(cache);
        let count = child(level, "ptCount").or_else(|| child(cache, "ptCount")).and_then(|c| c.attribute("val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        let fmt = child(cache, "formatCode").and_then(|f| f.text()).unwrap_or("General").to_string();
        let mut out = vec![(None, String::new()); count.min(100_000)];
        for pt in children(level, "pt") {
            let Some(i) = pt.attribute("idx").and_then(|v| v.parse::<usize>().ok()) else { continue };
            let v = child(pt, "v").and_then(|v| v.text()).unwrap_or("").to_string();
            let n = v.trim().parse::<f64>().ok();
            if i >= out.len() {
                if i > 100_000 {
                    continue;
                }
                out.resize(i + 1, (None, String::new()));
            }
            let shown = match n {
                Some(x) if r.tag_name().name() == "numRef" || r.tag_name().name() == "numLit" => visible(&numfmt::format(&Value::Number(x), &fmt, false, Separators::system()).text),
                _ => v.clone(),
            };
            out[i] = (n, shown);
        }
        if !out.is_empty() {
            return out;
        }
    }
    match child(r, "f").and_then(|f| f.text()) {
        Some(f) => data(f),
        None => vec![],
    }
}

fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut hidden = false;
    for c in s.chars() {
        match c {
            numfmt::HIDE_ON => hidden = true,
            numfmt::HIDE_OFF => hidden = false,
            c if !hidden => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

// ---------------------------------------------------------------------------
// Drawing

fn rgb(c: Rgb) -> String {
    format!("rgb({}, {}, {})", c[0], c[1], c[2])
}

fn esc(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Rough width of chart text (the document font is set by the sheet's printing)
fn text_w(s: &str, size: f32) -> f32 {
    ["Calibri", "Carlito", "Arial", "Helvetica"].iter().find_map(|f| super::typeset::text_width(f, size, false, false, s)).unwrap_or(s.chars().count() as f32 * size * 0.5)
}

/// Round numbers for a value axis: (min, max, step)
fn nice_scale(lo: f64, hi: f64, room: f32, fixed_min: Option<f64>, fixed_max: Option<f64>) -> (f64, f64, f64) {
    let (mut lo, mut hi) = (lo.min(hi), lo.max(hi));
    if lo >= 0.0 && !(lo > hi * 5.0 / 6.0) {
        lo = 0.0;
    }
    if hi <= 0.0 && !(hi < lo * 5.0 / 6.0) {
        hi = 0.0;
    }
    if let Some(m) = fixed_min {
        lo = m;
    }
    if let Some(m) = fixed_max {
        hi = m;
    }
    if hi - lo < 1e-12 {
        hi = lo + if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
    }
    let intervals = (room / 30.0).clamp(3.0, 10.0) as f64;
    let rough = (hi - lo) / intervals;
    let mag = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= rough).unwrap_or(10.0 * mag);
    let min = if fixed_min.is_some() { lo } else { (lo / step).floor() * step };
    // Excel keeps a little headroom above the highest value
    let max = if fixed_max.is_some() { hi } else { ((hi + (hi - min) * 0.05) / step).ceil() * step };
    (min, max, step)
}

struct Out {
    s: String,
}

impl Out {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgb, stroke: Option<(Rgb, f32)>) {
        let st = stroke.map(|(c, w)| format!("{w:.2}pt + {}", rgb(c))).unwrap_or_else(|| "none".into());
        let _ = writeln!(self.s, "#place(top + left, dx: {x:.2}pt, dy: {y:.2}pt, rect(width: {:.2}pt, height: {:.2}pt, fill: {}, stroke: {st}))", w.max(0.0), h.max(0.0), rgb(fill));
    }
    fn line(&mut self, a: (f32, f32), b: (f32, f32), color: Rgb, width: f32) {
        let _ = writeln!(self.s, "#place(top + left, line(start: ({:.2}pt, {:.2}pt), end: ({:.2}pt, {:.2}pt), stroke: {width:.2}pt + {}))", a.0, a.1, b.0, b.1, rgb(color));
    }
    /// text with its anchor point at (x, y): `h` is "left"/"center"/"right", `v` "top"/"horizon"/"bottom"
    fn text(&mut self, x: f32, y: f32, s: &str, size: f32, color: Rgb, h: &str, v: &str, bold: bool) {
        let w = text_w(s, size) + 2.0;
        let lines = s.split('\n').count() as f32;
        let ht = size * 1.25 * lines;
        let dx = match h {
            "center" => x - w / 2.0,
            "right" => x - w,
            _ => x,
        };
        let dy = match v {
            "horizon" => y - ht / 2.0,
            "bottom" => y - ht,
            _ => y,
        };
        let weight = if bold { ", weight: \"bold\"" } else { "" };
        let _ = writeln!(
            self.s,
            "#place(top + left, dx: {dx:.2}pt, dy: {dy:.2}pt, box(width: {w:.2}pt, height: {ht:.2}pt, align({h} + horizon, text(size: {size:.2}pt, fill: {}{weight}, top-edge: \"ascender\", bottom-edge: \"descender\", {}))))",
            rgb(color),
            esc(s)
        );
    }
    fn rotated_text(&mut self, cx: f32, cy: f32, s: &str, size: f32, color: Rgb, deg: f32) {
        let w = text_w(s, size) + 2.0;
        let _ = writeln!(
            self.s,
            "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, box(width: {w:.2}pt, height: {:.2}pt, rotate({deg:.1}deg, origin: center + horizon, align(center + horizon, text(size: {size:.2}pt, fill: {}, {})))))",
            cx - w / 2.0,
            cy - size * 0.65,
            size * 1.3,
            rgb(color),
            esc(s)
        );
    }
    fn polyline(&mut self, pts: &[(f32, f32)], color: Rgb, width: f32) {
        if pts.len() < 2 {
            return;
        }
        let p: Vec<String> = pts.iter().enumerate().map(|(i, (x, y))| format!("curve.{}(({x:.2}pt, {y:.2}pt))", if i == 0 { "move" } else { "line" })).collect();
        let _ = writeln!(self.s, "#place(top + left, curve(stroke: (paint: {}, thickness: {width:.2}pt, join: \"round\", cap: \"round\"), {}))", rgb(color), p.join(", "));
    }
    fn polygon(&mut self, pts: &[(f32, f32)], fill: Rgb, stroke: Option<(Rgb, f32)>) {
        if pts.len() < 3 {
            return;
        }
        let p: Vec<String> = pts.iter().map(|(x, y)| format!("({x:.2}pt, {y:.2}pt)")).collect();
        let st = stroke.map(|(c, w)| format!("{w:.2}pt + {}", rgb(c))).unwrap_or_else(|| "none".into());
        let _ = writeln!(self.s, "#place(top + left, polygon(fill: {}, stroke: {st}, {}))", rgb(fill), p.join(", "));
    }
    fn marker(&mut self, x: f32, y: f32, color: Rgb) {
        let r = 2.5;
        let _ = writeln!(self.s, "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, circle(radius: {r}pt, fill: {}, stroke: 0.75pt + {}))", x - r, y - r, rgb(color), rgb(color));
    }
}

/// A wedge of a pie (`r0` > 0: of a doughnut), angles in degrees clockwise from 12 o'clock
fn wedge(cx: f32, cy: f32, r: f32, r0: f32, a0: f32, a1: f32) -> Vec<(f32, f32)> {
    let steps = (((a1 - a0).abs() / 4.0).ceil() as usize).max(2);
    let at = |a: f32, rad: f32| {
        let t = (a - 90.0).to_radians();
        (cx + rad * t.cos(), cy + rad * t.sin())
    };
    let mut pts: Vec<(f32, f32)> = (0..=steps).map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, r)).collect();
    if r0 > 0.0 {
        pts.extend((0..=steps).rev().map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, r0)));
    } else {
        pts.push((cx, cy));
    }
    pts
}

/// Typst markup for the chart in a `w` × `h` point box (unscaled; the caller scales)
pub fn markup(c: &Chart, w: f32, h: f32, sep: Separators) -> String {
    let mut o = Out { s: String::new() };
    let fmt = |x: f64, code: &str| visible(&numfmt::format(&Value::Number(x), code, false, sep).text);
    if let Some(bg) = c.background {
        o.rect(0.0, 0.0, w, h, bg, c.border.map(|b| (b, 0.75)));
    } else if let Some(b) = c.border {
        let _ = writeln!(o.s, "#place(top + left, rect(width: {w:.2}pt, height: {h:.2}pt, stroke: 0.75pt + {}))", rgb(b));
    }
    let ts = c.text_size;
    let pad = 7.0;
    let (mut top, mut bottom, mut left, mut right) = (pad, h - pad, pad, w - pad);
    if let Some(t) = &c.title {
        o.text(w / 2.0, top, t, c.title_size, TEXT, "center", "top", false);
        top += c.title_size * 1.25 * t.split('\n').count() as f32 + 4.0;
    }
    let pie = c.plots.iter().all(|p| p.kind == Kind::Pie);
    // legend entries: series, or a pie's categories
    let entries: Vec<(String, Rgb, bool)> = if pie {
        let s = &c.plots[0].series[0];
        let n = s.vals.len().max(s.cats.len());
        (0..n).map(|i| (s.cats.get(i).cloned().unwrap_or_else(|| (i + 1).to_string()), point_color(&c.plots[0], s, i, &c.accents), false)).collect()
    } else {
        c.plots.iter().flat_map(|p| p.series.iter().map(move |s| (s.name.clone(), s.color, p.kind == Kind::Line || p.kind == Kind::Scatter))).collect()
    };
    if let Some(pos) = c.legend.as_deref() {
        let item_w = |e: &(String, Rgb, bool)| text_w(&e.0, ts) + ts * 1.6 + 8.0;
        match pos {
            "r" | "l" => {
                let lw = entries.iter().map(item_w).fold(0.0, f32::max).min(w * 0.4);
                let lh = entries.len() as f32 * ts * 1.5;
                let x = if pos == "r" { right - lw } else { left };
                let mut y = (top + bottom) / 2.0 - lh / 2.0;
                for e in &entries {
                    legend_item(&mut o, x, y, e, ts);
                    y += ts * 1.5;
                }
                if pos == "r" { right -= lw + 6.0 } else { left += lw + 6.0 }
            }
            _ => {
                // rows of entries centred, at the bottom (or top)
                let mut rows: Vec<Vec<&(String, Rgb, bool)>> = vec![vec![]];
                let mut used = 0.0;
                for e in &entries {
                    let iw = item_w(e);
                    if used + iw > right - left && !rows.last().unwrap().is_empty() {
                        rows.push(vec![]);
                        used = 0.0;
                    }
                    rows.last_mut().unwrap().push(e);
                    used += iw;
                }
                let lh = rows.len() as f32 * ts * 1.5;
                let mut y = if pos == "t" { top } else { bottom - lh };
                for row in &rows {
                    let total: f32 = row.iter().map(|e| item_w(e)).sum();
                    let mut x = (left + right) / 2.0 - total / 2.0;
                    for e in row {
                        legend_item(&mut o, x, y, e, ts);
                        x += item_w(e);
                    }
                    y += ts * 1.5;
                }
                if pos == "t" { top += lh + 4.0 } else { bottom -= lh + 4.0 }
            }
        }
    }
    if pie {
        draw_pie(&mut o, c, left, top, right, bottom, &fmt);
        return o.s;
    }
    draw_axes_chart(&mut o, c, left, top, right, bottom, &fmt);
    o.s
}

fn point_color(p: &Plot, s: &Series, i: usize, accents: &[Rgb]) -> Rgb {
    if let Some((_, c)) = s.points.iter().find(|(j, _)| *j == i) {
        return *c;
    }
    if p.vary_colors && !accents.is_empty() {
        let c = accents[i % accents.len()];
        return match i / accents.len() {
            0 => c,
            1 => super::xlsx::tint(c, -0.25),
            _ => super::xlsx::tint(c, 0.4),
        };
    }
    s.color
}

fn legend_item(o: &mut Out, x: f32, y: f32, e: &(String, Rgb, bool), ts: f32) {
    let sw = ts * 0.75;
    if e.2 {
        o.line((x, y + ts * 0.62), (x + ts * 1.3, y + ts * 0.62), e.1, 2.0);
    } else {
        o.rect(x + ts * 0.3, y + ts * 0.25, sw, sw, e.1, None);
    }
    o.text(x + ts * 1.6, y, &e.0, ts, TEXT, "left", "top", false);
}

fn draw_pie(o: &mut Out, c: &Chart, l: f32, t: f32, r: f32, b: f32, fmt: &dyn Fn(f64, &str) -> String) {
    let p = &c.plots[0];
    let s = &p.series[0];
    let total: f64 = s.vals.iter().flatten().filter(|v| **v > 0.0).sum();
    if total <= 0.0 {
        return;
    }
    let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
    let radius = ((r - l).min(b - t) / 2.0 * 0.92).max(4.0);
    let r0 = radius * p.hole / 100.0;
    let mut a = p.first_angle;
    let mut labels = vec![];
    for (i, v) in s.vals.iter().enumerate() {
        let Some(v) = v.filter(|v| *v > 0.0) else { continue };
        let sweep = (v / total * 360.0) as f32;
        let color = point_color(p, s, i, &c.accents);
        o.polygon(&wedge(cx, cy, radius, r0, a, a + sweep), color, Some(([255, 255, 255], 1.0)));
        if s.labels.value || s.labels.percent || s.labels.category {
            let mid = (a + sweep / 2.0 - 90.0).to_radians();
            let rr = if r0 > 0.0 { (radius + r0) / 2.0 } else { radius * 0.65 };
            let mut parts = vec![];
            if s.labels.category {
                parts.push(s.cats.get(i).cloned().unwrap_or_default());
            }
            if s.labels.value {
                parts.push(fmt(v, &s.fmt));
            }
            if s.labels.percent {
                parts.push(format!("{:.0}%", v / total * 100.0));
            }
            labels.push((cx + rr * mid.cos(), cy + rr * mid.sin(), parts.join("; ")));
        }
        a += sweep;
    }
    for (x, y, text) in labels {
        o.text(x, y, &text, c.text_size, [255, 255, 255], "center", "horizon", true);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_axes_chart(o: &mut Out, c: &Chart, mut l: f32, t: f32, r: f32, mut b: f32, fmt: &dyn Fn(f64, &str) -> String) {
    let ts = c.text_size;
    let scatter = c.plots.iter().any(|p| p.kind == Kind::Scatter);
    let horizontal = c.plots.iter().any(|p| p.kind == Kind::Bar && p.horizontal);
    let first = &c.plots[0].series[0];
    let cats: Vec<String> = c.plots.iter().flat_map(|p| p.series.iter()).map(|s| &s.cats).max_by_key(|c| c.len()).cloned().unwrap_or_default();
    let n = c.plots.iter().flat_map(|p| p.series.iter()).map(|s| s.vals.len()).max().unwrap_or(0).max(cats.len()).max(1);
    // the value range, with stacked series added up
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    let mut percent = false;
    for p in &c.plots {
        match p.grouping {
            Grouping::Standard => {
                for v in p.series.iter().flat_map(|s| s.vals.iter().flatten()) {
                    lo = lo.min(*v);
                    hi = hi.max(*v);
                }
            }
            Grouping::Stacked => {
                for i in 0..n {
                    let pos: f64 = p.series.iter().filter_map(|s| s.vals.get(i).copied().flatten()).filter(|v| *v > 0.0).sum();
                    let neg: f64 = p.series.iter().filter_map(|s| s.vals.get(i).copied().flatten()).filter(|v| *v < 0.0).sum();
                    lo = lo.min(neg);
                    hi = hi.max(pos);
                }
            }
            Grouping::Percent => {
                percent = true;
                lo = lo.min(0.0);
                hi = hi.max(1.0);
            }
        }
    }
    if lo > hi {
        (lo, hi) = (0.0, 1.0);
    }
    let val_fmt = c.val_axis.fmt.clone().unwrap_or_else(|| if percent { "0%".into() } else { first.fmt.clone() });
    let val_room = if horizontal { r - l } else { b - t };
    let (vmin, vmax, step) = if percent { (0.0, 1.0, 0.1_f64.max(if val_room < 150.0 { 0.2 } else { 0.1 })) } else { nice_scale(lo, hi, val_room, c.val_axis.min, c.val_axis.max) };
    let ticks: Vec<f64> = {
        let mut v = vec![];
        let mut x = vmin;
        while x <= vmax + step * 1e-6 && v.len() < 60 {
            v.push(if x.abs() < step * 1e-9 { 0.0 } else { x });
            x += step;
        }
        v
    };
    let tick_labels: Vec<String> = ticks.iter().map(|v| fmt(*v, &val_fmt)).collect();
    // scatter: x values on a value axis too
    let xs_range = scatter.then(|| {
        let xs: Vec<f64> = c.plots.iter().flat_map(|p| p.series.iter()).flat_map(|s| s.xs.iter().flatten().copied()).collect();
        let (a, z) = xs.iter().fold((f64::MAX, f64::MIN), |(a, z), x| (a.min(*x), z.max(*x)));
        if xs.is_empty() { (0.0, n as f64, 1.0) } else { nice_scale(a, z, r - l, None, None) }
    });
    // axis titles
    if let Some(title) = c.val_axis.title.as_ref().filter(|_| !c.val_axis.deleted) {
        if horizontal {
            o.text((l + r) / 2.0, b, title, ts, TEXT, "center", "bottom", false);
            b -= ts * 1.4;
        } else {
            o.rotated_text(l + ts * 0.7, (t + b) / 2.0, title, ts, TEXT, -90.0);
            l += ts * 1.5;
        }
    }
    if let Some(title) = c.cat_axis.title.as_ref().filter(|_| !c.cat_axis.deleted) {
        if horizontal {
            o.rotated_text(l + ts * 0.7, (t + b) / 2.0, title, ts, TEXT, -90.0);
            l += ts * 1.5;
        } else {
            o.text((l + r) / 2.0, b, title, ts, TEXT, "center", "bottom", false);
            b -= ts * 1.4;
        }
    }
    // room for the labels along each axis
    let cat_labels: Vec<String> = if let Some((a, z, st)) = xs_range {
        let mut v = vec![];
        let mut x = a;
        while x <= z + st * 1e-6 && v.len() < 60 {
            v.push(fmt(x, "General"));
            x += st;
        }
        v
    } else {
        (0..n).map(|i| cats.get(i).cloned().unwrap_or_else(|| (i + 1).to_string())).collect()
    };
    let (val_labels_shown, cat_labels_shown) = (!c.val_axis.deleted, !c.cat_axis.deleted);
    let val_label_w = if val_labels_shown { tick_labels.iter().map(|s| text_w(s, ts)).fold(0.0, f32::max) + 4.0 } else { 0.0 };
    let cat_label_w = cat_labels.iter().map(|s| text_w(s, ts)).fold(0.0, f32::max);
    let (mut plot_l, mut plot_b) = (l, b);
    let mut rotate = false;
    if horizontal {
        if cat_labels_shown {
            plot_l += cat_label_w.min((r - l) * 0.4) + 4.0;
        }
        if val_labels_shown {
            plot_b -= ts * 1.4;
        }
    } else {
        plot_l += val_label_w;
        if cat_labels_shown {
            let slot = (r - plot_l) / cat_labels.len().max(1) as f32;
            rotate = cat_label_w > slot * 0.95 && !scatter;
            plot_b -= if rotate { (cat_label_w * 0.72).min((b - t) * 0.4) + ts } else { ts * 1.4 };
        }
    }
    let (pl, pt, pr, pb) = (plot_l, t + ts * 0.5, r, plot_b);
    if pr - pl < 10.0 || pb - pt < 10.0 {
        return;
    }
    // value -> position along the value axis
    let vpos = |v: f64| -> f32 {
        let f = ((v - vmin) / (vmax - vmin)) as f32;
        if horizontal { pl + f * (pr - pl) } else { pb - f * (pb - pt) }
    };
    // gridlines and value labels
    for (v, label) in ticks.iter().zip(&tick_labels) {
        let p = vpos(*v);
        if horizontal {
            if c.val_axis.gridlines || c.val_axis.title.is_none() {
                o.line((p, pt), (p, pb), GRID, 0.75);
            }
            if val_labels_shown {
                o.text(p, pb + 3.0, label, ts, TEXT, "center", "top", false);
            }
        } else {
            if c.val_axis.gridlines || !scatter {
                o.line((pl, p), (pr, p), GRID, 0.75);
            }
            if val_labels_shown {
                o.text(pl - 4.0, p, label, ts, TEXT, "right", "horizon", false);
            }
        }
    }
    // category slots
    let slot = if horizontal { (pb - pt) / n as f32 } else { (pr - pl) / n as f32 };
    let reversed = c.cat_axis.reversed;
    // centre of category i along the category axis; horizontal bars start at the bottom
    let cpos = |i: f32| -> f32 {
        let i = if reversed { n as f32 - 1.0 - i } else { i };
        if horizontal { pb - (i + 0.5) * slot } else { pl + (i + 0.5) * slot }
    };
    let xpos = |x: f64| -> f32 {
        let (a, z, _) = xs_range.unwrap_or((0.0, 1.0, 1.0));
        pl + ((x - a) / (z - a).max(1e-12)) as f32 * (pr - pl)
    };
    // the category axis line, at zero
    let zero = vpos(0.0f64.clamp(vmin, vmax));
    if cat_labels_shown {
        if let Some((a, z, st)) = xs_range {
            let mut x = a;
            let mut i = 0;
            while x <= z + st * 1e-6 && i < cat_labels.len() {
                o.text(xpos(x), pb + 3.0, &cat_labels[i], ts, TEXT, "center", "top", false);
                if c.cat_axis.gridlines {
                    o.line((xpos(x), pt), (xpos(x), pb), GRID, 0.75);
                }
                x += st;
                i += 1;
            }
        } else {
            // every k-th label when they would collide
            let room = if horizontal { ts * 1.3 } else if rotate { ts * 1.3 } else { slot };
            let k = if horizontal || rotate { ((room / slot).ceil() as usize).max(1) } else { 1 };
            for (i, label) in cat_labels.iter().enumerate().step_by(k) {
                let p = cpos(i as f32);
                if horizontal {
                    o.text(pl - 4.0, p, label, ts, TEXT, "right", "horizon", false);
                } else if rotate {
                    let w = text_w(label, ts) + 2.0;
                    let _ = writeln!(
                        o.s,
                        "#place(top + left, dx: {:.2}pt, dy: {:.2}pt, rotate(-45deg, origin: right + horizon, box(width: {w:.2}pt, height: {:.2}pt, align(right + horizon, text(size: {ts:.2}pt, fill: {}, {})))))",
                        p - w,
                        pb + 2.0,
                        ts * 1.3,
                        rgb(TEXT),
                        esc(label)
                    );
                } else {
                    o.text(p, pb + 3.0, label, ts, TEXT, "center", "top", false);
                }
            }
        }
    }
    // the series
    for p in &c.plots {
        let k = p.series.len();
        match p.kind {
            Kind::Bar => {
                let group = slot / (1.0 + p.gap / 100.0);
                let stacked = p.grouping != Grouping::Standard;
                let overlap = (p.overlap / 100.0).clamp(-1.0, 1.0);
                let bw = if stacked { group } else { group / (k as f32 - (k as f32 - 1.0) * overlap).max(1.0) };
                for i in 0..n {
                    let total: f64 = p.series.iter().filter_map(|s| s.vals.get(i).copied().flatten()).map(f64::abs).sum();
                    let (mut up, mut down) = (0.0f64, 0.0f64);
                    for (si, s) in p.series.iter().enumerate() {
                        let Some(mut v) = s.vals.get(i).copied().flatten() else { continue };
                        if p.grouping == Grouping::Percent {
                            v = if total > 0.0 { v / total } else { 0.0 };
                        }
                        let (from, to) = if stacked {
                            if v >= 0.0 {
                                up += v;
                                (up - v, up)
                            } else {
                                down += v;
                                (down - v, down)
                            }
                        } else {
                            (0.0f64.clamp(vmin, vmax), v)
                        };
                        let offset = if stacked { 0.0 } else { si as f32 * bw * (1.0 - overlap) };
                        let start = cpos(i as f32) + if horizontal { group / 2.0 - offset - bw } else { -group / 2.0 + offset };
                        let (a, z) = (vpos(from.clamp(vmin, vmax)), vpos(to.clamp(vmin, vmax)));
                        let color = point_color(p, s, i, &c.accents);
                        if horizontal {
                            o.rect(a.min(z), start, (z - a).abs(), bw, color, None);
                        } else {
                            o.rect(start, a.min(z), bw, (z - a).abs(), color, None);
                        }
                        if s.labels.value {
                            let label = fmt(if p.grouping == Grouping::Percent { s.vals[i].unwrap_or(0.0) } else { v }, &s.fmt);
                            if horizontal {
                                o.text(z + 2.0, start + bw / 2.0, &label, ts, TEXT, "left", "horizon", false);
                            } else if stacked {
                                o.text(start + bw / 2.0, (a + z) / 2.0, &label, ts, [255, 255, 255], "center", "horizon", false);
                            } else {
                                o.text(start + bw / 2.0, z - 1.0, &label, ts, TEXT, "center", "bottom", false);
                            }
                        }
                    }
                }
            }
            Kind::Line | Kind::Area => {
                let mut base = vec![0.0f64; n];
                for s in &p.series {
                    let mut pts = vec![];
                    let mut lower = vec![];
                    for i in 0..n {
                        let Some(mut v) = s.vals.get(i).copied().flatten() else { continue };
                        if p.grouping == Grouping::Percent {
                            let total: f64 = p.series.iter().filter_map(|x| x.vals.get(i).copied().flatten()).sum();
                            v = if total != 0.0 { v / total } else { 0.0 };
                        }
                        let from = if p.grouping == Grouping::Standard { 0.0 } else { base[i] };
                        let to = from + v;
                        if p.grouping != Grouping::Standard {
                            base[i] = to;
                        }
                        let x = cpos(i as f32);
                        pts.push((x, vpos(to.clamp(vmin, vmax))));
                        lower.push((x, vpos(from.clamp(vmin, vmax))));
                    }
                    if p.kind == Kind::Area {
                        let mut poly = pts.clone();
                        poly.extend(lower.iter().rev());
                        o.polygon(&poly, s.color, None);
                    } else {
                        if let Some((lc, lw)) = s.line.filter(|l| l.1 > 0.0) {
                            o.polyline(&pts, lc, lw);
                        }
                        if s.marker {
                            for (x, y) in &pts {
                                o.marker(*x, *y, s.color);
                            }
                        }
                    }
                    if s.labels.value {
                        for (i, (x, y)) in pts.iter().enumerate() {
                            if let Some(v) = s.vals.get(i).copied().flatten() {
                                o.text(*x, *y - 4.0, &fmt(v, &s.fmt), ts, TEXT, "center", "bottom", false);
                            }
                        }
                    }
                }
            }
            Kind::Scatter => {
                for s in &p.series {
                    let pts: Vec<(f32, f32)> = s
                        .vals
                        .iter()
                        .enumerate()
                        .filter_map(|(i, v)| {
                            let y = (*v)?;
                            let x = s.xs.get(i).copied().flatten().unwrap_or(i as f64 + 1.0);
                            Some((xpos(x), vpos(y.clamp(vmin, vmax))))
                        })
                        .collect();
                    if let Some((lc, lw)) = s.line.filter(|l| l.1 > 0.0) {
                        o.polyline(&pts, lc, lw);
                    }
                    if s.marker {
                        for (x, y) in &pts {
                            o.marker(*x, *y, s.color);
                        }
                    }
                }
            }
            Kind::Pie => {}
        }
    }
    // the category axis line
    if horizontal {
        o.line((zero, pt), (zero, pb), [0xBF, 0xBF, 0xBF], 0.75);
    } else {
        o.line((pl, zero), (pr, zero), [0xBF, 0xBF, 0xBF], 0.75);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_scales_are_round() {
        assert_eq!(nice_scale(30.0, 170.0, 150.0, None, None), (0.0, 200.0, 50.0));
        let (lo, hi, step) = nice_scale(-12.0, 48.0, 200.0, None, None);
        assert!(lo <= -12.0 && hi >= 48.0 && (step == 10.0 || step == 20.0), "{lo} {hi} {step}");
    }

    #[test]
    fn charts_without_a_cache_read_their_cells() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:tx><c:strRef><c:f>S!$B$1</c:f></c:strRef></c:tx><c:cat><c:strRef><c:f>S!$A$2:$A$3</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>S!$B$2:$B$3</c:f></c:numRef></c:val></c:ser></c:barChart><c:catAx/><c:valAx/></c:plotArea><c:legend><c:legendPos val="b"/></c:legend></c:chart></c:chartSpace>"#;
        let data = |f: &str| -> Vec<(Option<f64>, String)> {
            match f {
                "S!$B$1" => vec![(None, "Sales".into())],
                "S!$A$2:$A$3" => vec![(None, "Jan".into()), (None, "Feb".into())],
                _ => vec![(Some(3.0), "3".into()), (Some(5.0), "5".into())],
            }
        };
        let c = parse(xml, &[], &data).unwrap();
        let s = &c.plots[0].series[0];
        assert_eq!((s.name.as_str(), s.cats.clone(), s.vals.clone()), ("Sales", vec!["Jan".to_string(), "Feb".into()], vec![Some(3.0), Some(5.0)]));
        // one series: its name is the title
        assert_eq!(c.title.as_deref(), Some("Sales"));
        let m = markup(&c, 300.0, 200.0, Separators::US);
        assert!(m.contains("\"Jan\"") && m.contains("rect("));
    }
}
