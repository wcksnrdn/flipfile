// Shape outlines for slides: DrawingML's preset shapes (with their adjust values),
// custom geometry, the area each preset keeps its text in, and line ends (arrowheads).
// Everything is in points inside the shape's box, y pointing down, angles clockwise.

use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Debug)]
pub enum Geometry {
    Rect,
    /// Corner radii: top-left, top-right, bottom-right, bottom-left
    RoundRect([f32; 4]),
    Ellipse,
    Paths(Vec<SubPath>),
}

#[derive(Clone, Debug)]
pub struct SubPath {
    pub commands: Vec<PathCommand>,
    pub fill: PathFill,
    pub stroke: bool,
    /// Holes by the even-odd rule (freeforms) rather than by direction (presets)
    pub even_odd: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathFill {
    /// The shape's fill
    Shape,
    /// The outline's colour (arrowheads)
    Line,
    None,
}

#[derive(Clone, Copy, Debug)]
pub enum PathCommand {
    Move(f32, f32),
    Line(f32, f32),
    Cubic([f32; 6]),
    Close,
}

impl Geometry {
    /// A shape that is only a line (connectors, arcs): nothing inside to fill
    pub fn is_open(&self) -> bool {
        matches!(self, Geometry::Paths(p) if p.iter().all(|s| s.fill == PathFill::None))
    }
}

/// Builds a path, keeping the current point for arcs
#[derive(Default)]
struct Pen {
    commands: Vec<PathCommand>,
    at: (f32, f32),
}

impl Pen {
    fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand::Move(x, y));
        self.at = (x, y);
        self
    }

    fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.commands.push(PathCommand::Line(x, y));
        self.at = (x, y);
        self
    }

    fn cubic(&mut self, c: [f32; 6]) -> &mut Self {
        self.commands.push(PathCommand::Cubic(c));
        self.at = (c[4], c[5]);
        self
    }

    fn quad(&mut self, qx: f32, qy: f32, x: f32, y: f32) -> &mut Self {
        let (sx, sy) = self.at;
        self.cubic([sx + 2.0 / 3.0 * (qx - sx), sy + 2.0 / 3.0 * (qy - sy), x + 2.0 / 3.0 * (qx - x), y + 2.0 / 3.0 * (qy - y), x, y])
    }

    /// DrawingML's arcTo: from the current point, which lies on an ellipse with radii
    /// (rx, ry) at angle `start`, around it by `sweep` degrees
    fn arc_to(&mut self, rx: f32, ry: f32, start: f32, sweep: f32) -> &mut Self {
        if rx <= 0.0 || ry <= 0.0 || sweep == 0.0 {
            return self;
        }
        let t0 = param(rx, ry, start);
        let (cx, cy) = (self.at.0 - rx * t0.cos(), self.at.1 - ry * t0.sin());
        let mut dt = param(rx, ry, start + sweep) - t0;
        if sweep.abs() >= 360.0 {
            dt = TAU * sweep.signum();
        } else if sweep > 0.0 {
            while dt <= 1e-6 {
                dt += TAU;
            }
        } else {
            while dt >= -1e-6 {
                dt -= TAU;
            }
        }
        let segments = (dt.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
        let step = dt / segments as f32;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let mut a = t0;
        for _ in 0..segments {
            let b = a + step;
            self.cubic([
                cx + rx * (a.cos() - k * a.sin()),
                cy + ry * (a.sin() + k * a.cos()),
                cx + rx * (b.cos() + k * b.sin()),
                cy + ry * (b.sin() - k * b.cos()),
                cx + rx * b.cos(),
                cy + ry * b.sin(),
            ]);
            a = b;
        }
        self
    }

    fn close(&mut self) -> &mut Self {
        self.commands.push(PathCommand::Close);
        self
    }

    fn polygon(points: &[(f32, f32)]) -> Pen {
        let mut pen = Pen::default();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                pen.move_to(x, y);
            } else {
                pen.line_to(x, y);
            }
        }
        pen.close();
        pen
    }

    fn filled(self) -> SubPath {
        SubPath { commands: self.commands, fill: PathFill::Shape, stroke: true, even_odd: false }
    }

    fn open(self) -> SubPath {
        SubPath { commands: self.commands, fill: PathFill::None, stroke: true, even_odd: false }
    }
}

/// The ellipse parameter for a visual angle (DrawingML angles are seen from the centre)
fn param(rx: f32, ry: f32, degrees: f32) -> f32 {
    let a = degrees.to_radians();
    (rx * a.sin()).atan2(ry * a.cos())
}

/// The point of an ellipse centred in the box at a visual angle
fn on_ellipse(cx: f32, cy: f32, rx: f32, ry: f32, degrees: f32) -> (f32, f32) {
    let t = param(rx, ry, degrees);
    (cx + rx * t.cos(), cy + ry * t.sin())
}

fn paths(subpaths: Vec<SubPath>) -> Geometry {
    Geometry::Paths(subpaths)
}

fn polygon(points: &[(f32, f32)]) -> Geometry {
    paths(vec![Pen::polygon(points).filled()])
}

/// Sweep from `start` to `end` clockwise, in degrees (0..360]
fn sweep(start: f32, end: f32) -> f32 {
    let s = (end - start).rem_euclid(360.0);
    if s == 0.0 { 360.0 } else { s }
}

/// A preset shape in a w × h box. `adj(name, default)` gives an adjust value as
/// stored (fractions in 1/100000, angles in 1/60000 degree).
pub fn preset(name: &str, adj: &dyn Fn(&str, f32) -> f32, w: f32, h: f32) -> Option<Geometry> {
    let ss = w.min(h);
    let (hc, vc) = (w / 2.0, h / 2.0);
    let frac = |name: &str, default: f32, max: f32| (adj(name, default) / 100_000.0).clamp(0.0, max);
    let angle = |name: &str, default: f32| adj(name, default) / 60_000.0;
    Some(match name {
        "rect" | "flowChartProcess" | "flowChartPredefinedProcess" | "flowChartInternalStorage" | "textBox" => Geometry::Rect,
        "roundRect" => Geometry::RoundRect([ss * frac("adj", 16_667.0, 0.5); 4]),
        "flowChartAlternateProcess" => Geometry::RoundRect([ss / 6.0; 4]),
        "flowChartTerminator" => Geometry::RoundRect([h.min(w) / 2.0; 4]),
        "round1Rect" => Geometry::RoundRect([0.0, ss * frac("adj", 16_667.0, 0.5), 0.0, 0.0]),
        "round2SameRect" => {
            let (a, b) = (ss * frac("adj1", 16_667.0, 0.5), ss * frac("adj2", 0.0, 0.5));
            Geometry::RoundRect([a, a, b, b])
        }
        "round2DiagRect" => {
            let (a, b) = (ss * frac("adj1", 16_667.0, 0.5), ss * frac("adj2", 0.0, 0.5));
            Geometry::RoundRect([a, b, a, b])
        }
        "ellipse" | "flowChartConnector" | "cloud" | "cloudCallout" | "flowChartOr" | "flowChartSummingJunction" => Geometry::Ellipse,
        "snip1Rect" => {
            let d = ss * frac("adj", 16_667.0, 0.5);
            polygon(&[(0.0, 0.0), (w - d, 0.0), (w, d), (w, h), (0.0, h)])
        }
        "snip2SameRect" => {
            let (a, b) = (ss * frac("adj1", 16_667.0, 0.5), ss * frac("adj2", 0.0, 0.5));
            polygon(&[(a, 0.0), (w - a, 0.0), (w, a), (w, h - b), (w - b, h), (b, h), (0.0, h - b), (0.0, a)])
        }
        "snip2DiagRect" => {
            let (a, b) = (ss * frac("adj1", 0.0, 0.5), ss * frac("adj2", 16_667.0, 0.5));
            polygon(&[(a, 0.0), (w - b, 0.0), (w, b), (w, h - a), (w - a, h), (b, h), (0.0, h - b), (0.0, a)])
        }
        "snipRoundRect" => {
            let (r, d) = (ss * frac("adj1", 16_667.0, 0.5), ss * frac("adj2", 16_667.0, 0.5));
            let mut pen = Pen::default();
            pen.move_to(r, 0.0).line_to(w - d, 0.0).line_to(w, d).line_to(w, h).line_to(0.0, h).line_to(0.0, r).arc_to(r, r, 180.0, 90.0).close();
            paths(vec![pen.filled()])
        }
        "triangle" | "flowChartExtract" | "flowChartMerge" => {
            let x = w * frac("adj", 50_000.0, 1.0);
            if name == "flowChartMerge" { polygon(&[(0.0, 0.0), (w, 0.0), (hc, h)]) } else { polygon(&[(x, 0.0), (w, h), (0.0, h)]) }
        }
        "rtTriangle" => polygon(&[(0.0, 0.0), (w, h), (0.0, h)]),
        "diamond" | "flowChartDecision" => polygon(&[(hc, 0.0), (w, vc), (hc, h), (0.0, vc)]),
        "parallelogram" | "flowChartInputOutput" => {
            let x = if name == "parallelogram" { ss * frac("adj", 25_000.0, w / ss) } else { w / 5.0 };
            polygon(&[(0.0, h), (x, 0.0), (w, 0.0), (w - x, h)])
        }
        "trapezoid" => {
            let x = ss * frac("adj", 25_000.0, w / ss / 2.0);
            polygon(&[(0.0, h), (x, 0.0), (w - x, 0.0), (w, h)])
        }
        "flowChartManualOperation" => polygon(&[(0.0, 0.0), (w, 0.0), (w * 0.8, h), (w * 0.2, h)]),
        "flowChartManualInput" => polygon(&[(0.0, h / 5.0), (w, 0.0), (w, h), (0.0, h)]),
        "flowChartOffpageConnector" => polygon(&[(0.0, 0.0), (w, 0.0), (w, h * 0.8), (hc, h), (0.0, h * 0.8)]),
        "flowChartPreparation" => polygon(&[(0.0, vc), (w / 5.0, 0.0), (w * 0.8, 0.0), (w, vc), (w * 0.8, h), (w / 5.0, h)]),
        "flowChartPunchedCard" => polygon(&[(w / 5.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, h / 5.0)]),
        "flowChartDocument" => {
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).line_to(w, 0.0).line_to(w, h * 0.8).cubic([w * 0.75, h * 0.6, w * 0.5, h * 1.05, 0.0, h * 0.9]).close();
            paths(vec![pen.filled()])
        }
        "flowChartDelay" => {
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).line_to(hc, 0.0).arc_to(hc, vc, 270.0, 180.0).line_to(0.0, h).close();
            paths(vec![pen.filled()])
        }
        "pentagon" => polygon(&[(hc, 0.0), (w, h * 0.382), (w * 0.809, h), (w * 0.191, h), (0.0, h * 0.382)]),
        "homePlate" => {
            let x = w - ss * frac("adj", 50_000.0, w / ss);
            polygon(&[(0.0, 0.0), (x, 0.0), (w, vc), (x, h), (0.0, h)])
        }
        "chevron" => {
            let x = ss * frac("adj", 50_000.0, w / ss);
            polygon(&[(0.0, 0.0), (w - x, 0.0), (w, vc), (w - x, h), (0.0, h), (x, vc)])
        }
        "hexagon" => {
            let x = ss * frac("adj", 25_000.0, w / ss / 2.0);
            polygon(&[(0.0, vc), (x, 0.0), (w - x, 0.0), (w, vc), (w - x, h), (x, h)])
        }
        "octagon" => {
            let x = ss * frac("adj", 29_289.0, 0.5);
            polygon(&[(x, 0.0), (w - x, 0.0), (w, x), (w, h - x), (w - x, h), (x, h), (0.0, h - x), (0.0, x)])
        }
        "decagon" | "dodecagon" | "heptagon" => {
            let n = match name {
                "heptagon" => 7,
                "decagon" => 10,
                _ => 12,
            };
            let pts: Vec<(f32, f32)> = (0..n).map(|i| on_ellipse(hc, vc, hc, vc, -90.0 + 360.0 * i as f32 / n as f32)).collect();
            polygon(&pts)
        }
        "star4" | "star5" | "star6" | "star7" | "star8" | "star10" | "star12" | "star16" | "star24" | "star32" => {
            let n: usize = name[4..].parse().ok()?;
            let default = match n {
                4 => 12_500.0,
                5 => 19_098.0,
                6 => 28_868.0,
                7 => 34_601.0,
                8 => 38_250.0,
                10 => 42_533.0,
                _ => 37_500.0,
            };
            let inner = frac("adj", default, 0.5) * 2.0;
            let (hf, vf) = match n {
                5 => (1.05146, 1.10557),
                6 => (1.1547, 1.0),
                7 => (1.02572, 1.05210),
                10 => (1.05146, 1.0),
                _ => (1.0, 1.0),
            };
            let (rx, ry, cy) = (hc * hf, vc * vf, vc * vf);
            let mut pts = vec![];
            for i in 0..n * 2 {
                let a = (-90.0 + 180.0 * i as f32 / n as f32).to_radians();
                let r = if i % 2 == 0 { 1.0 } else { inner };
                pts.push((hc + rx * r * a.cos(), cy + ry * r * a.sin()));
            }
            polygon(&pts)
        }
        "plus" | "mathPlus" | "cross" => {
            let x = ss * frac("adj", if name == "mathPlus" { 23_520.0 } else { 25_000.0 }, 0.5);
            let (x1, y1) = if name == "mathPlus" { (hc - x / 2.0, vc - x / 2.0) } else { (x, x) };
            let (x2, y2) = if name == "mathPlus" { (hc + x / 2.0, vc + x / 2.0) } else { (w - x, h - x) };
            polygon(&[(0.0, y1), (x1, y1), (x1, 0.0), (x2, 0.0), (x2, y1), (w, y1), (w, y2), (x2, y2), (x2, h), (x1, h), (x1, y2), (0.0, y2)])
        }
        "mathMinus" => {
            let d = h * frac("adj1", 23_520.0, 1.0) / 2.0;
            polygon(&[(w * 0.07, vc - d), (w * 0.93, vc - d), (w * 0.93, vc + d), (w * 0.07, vc + d)])
        }
        "corner" => {
            let (dy, dx) = (ss * frac("adj1", 50_000.0, 1.0), ss * frac("adj2", 50_000.0, 1.0));
            polygon(&[(0.0, 0.0), (dx, 0.0), (dx, h - dy), (w, h - dy), (w, h), (0.0, h)])
        }
        "frame" => {
            let d = ss * frac("adj1", 12_500.0, 0.5);
            let outer = Pen::polygon(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
            let mut sub = outer.filled();
            sub.commands.extend(Pen::polygon(&[(d, d), (d, h - d), (w - d, h - d), (w - d, d)]).commands);
            paths(vec![sub])
        }
        "donut" | "noSmoking" => {
            let d = ss * frac("adj", 25_000.0, 0.5);
            let mut pen = Pen::default();
            pen.move_to(0.0, vc).arc_to(hc, vc, 180.0, 360.0).close();
            let (rx, ry) = ((hc - d).max(0.0), (vc - d).max(0.0));
            pen.move_to(d, vc).arc_to(rx, ry, 180.0, -360.0).close();
            paths(vec![pen.filled()])
        }
        "blockArc" => {
            let (st, end) = (angle("adj1", 10_800_000.0), angle("adj2", 0.0));
            let d = ss * frac("adj3", 25_000.0, 0.5);
            let sw = sweep(st, end);
            let (rx, ry) = ((hc - d).max(0.0), (vc - d).max(0.0));
            let (x0, y0) = on_ellipse(hc, vc, hc, vc, st);
            let (x1, y1) = on_ellipse(hc, vc, rx, ry, st + sw);
            let mut pen = Pen::default();
            pen.move_to(x0, y0).arc_to(hc, vc, st, sw).line_to(x1, y1).arc_to(rx, ry, st + sw, -sw).close();
            paths(vec![pen.filled()])
        }
        "pie" | "chord" | "arc" => {
            let (st, end) = match name {
                "pie" => (angle("adj1", 0.0), angle("adj2", 16_200_000.0)),
                "chord" => (angle("adj1", 2_700_000.0), angle("adj2", 16_200_000.0)),
                _ => (angle("adj1", 16_200_000.0), angle("adj2", 0.0)),
            };
            let sw = sweep(st, end);
            let (x0, y0) = on_ellipse(hc, vc, hc, vc, st);
            let mut pen = Pen::default();
            if name == "pie" {
                pen.move_to(hc, vc).line_to(x0, y0);
            } else {
                pen.move_to(x0, y0);
            }
            pen.arc_to(hc, vc, st, sw);
            if name == "arc" {
                return Some(paths(vec![pen.open()]));
            }
            pen.close();
            paths(vec![pen.filled()])
        }
        "heart" => {
            let (dx1, dx2, y1) = (w * 49.0 / 48.0, w * 10.0 / 48.0, -h / 3.0);
            let mut pen = Pen::default();
            pen.move_to(hc, h / 4.0)
                .cubic([hc + dx2, y1, hc + dx1, h / 4.0, hc, h])
                .cubic([hc - dx1, h / 4.0, hc - dx2, y1, hc, h / 4.0])
                .close();
            paths(vec![pen.filled()])
        }
        "teardrop" => {
            let a = frac("adj", 100_000.0, 2.0);
            let r = std::f32::consts::SQRT_2 * a / 2.0;
            let (tx, ty) = (hc + hc * r * std::f32::consts::SQRT_2, vc - vc * r * std::f32::consts::SQRT_2);
            let mut pen = Pen::default();
            pen.move_to(0.0, vc).arc_to(hc, vc, 180.0, 90.0).quad((hc + tx) / 2.0, ty.min(0.0) / 2.0, tx, ty).quad(w - (w - tx).min(0.0) / 2.0, (vc + ty) / 2.0, w, vc).arc_to(hc, vc, 0.0, 180.0).close();
            paths(vec![pen.filled()])
        }
        "moon" => {
            let a = frac("adj", 50_000.0, 0.875);
            let mut pen = Pen::default();
            pen.move_to(w, h).arc_to(w, vc, 90.0, 180.0);
            let inner = w - ss * a * 2.0;
            pen.arc_to(inner.max(0.0) / 1.0 + 0.001, vc, 270.0, -180.0).close();
            paths(vec![pen.filled()])
        }
        "can" | "flowChartMagneticDisk" => {
            let ry = if name == "can" { ss * frac("adj", 25_000.0, 0.5) / 2.0 } else { h / 6.0 };
            let mut body = Pen::default();
            body.move_to(0.0, ry).arc_to(hc, ry, 180.0, -180.0).line_to(w, h - ry).arc_to(hc, ry, 0.0, 180.0).close();
            let mut top = Pen::default();
            top.move_to(0.0, ry).arc_to(hc, ry, 180.0, 360.0).close();
            paths(vec![body.filled(), top.filled()])
        }
        "cube" => {
            let d = ss * frac("adj", 25_000.0, 1.0);
            paths(vec![
                Pen::polygon(&[(0.0, d), (w - d, d), (w - d, h), (0.0, h)]).filled(),
                Pen::polygon(&[(0.0, d), (d, 0.0), (w, 0.0), (w - d, d)]).filled(),
                Pen::polygon(&[(w - d, d), (w, 0.0), (w, h - d), (w - d, h)]).filled(),
            ])
        }
        "rightArrow" | "leftArrow" | "notchedRightArrow" | "stripedRightArrow" => {
            let dy = h * frac("adj1", 50_000.0, 1.0) / 2.0;
            let head = ss * frac("adj2", 50_000.0, w / ss);
            let (y1, y2) = (vc - dy, vc + dy);
            let mut subs = vec![];
            let pts = if name == "leftArrow" {
                vec![(w, y1), (head, y1), (head, 0.0), (0.0, vc), (head, h), (head, y2), (w, y2)]
            } else {
                let x = w - head;
                let start = if name == "stripedRightArrow" { ss * 5.0 / 32.0 } else { 0.0 };
                let mut pts = vec![(start, y1), (x, y1), (x, 0.0), (w, vc), (x, h), (x, y2), (start, y2)];
                if name == "notchedRightArrow" {
                    pts.push((dy * head / vc.max(0.01), vc));
                }
                if name == "stripedRightArrow" {
                    let s = ss / 32.0;
                    subs.push(Pen::polygon(&[(0.0, y1), (s, y1), (s, y2), (0.0, y2)]).filled());
                    subs.push(Pen::polygon(&[(2.0 * s, y1), (4.0 * s, y1), (4.0 * s, y2), (2.0 * s, y2)]).filled());
                }
                pts
            };
            subs.push(Pen::polygon(&pts).filled());
            paths(subs)
        }
        "upArrow" | "downArrow" => {
            let dx = w * frac("adj1", 50_000.0, 1.0) / 2.0;
            let head = ss * frac("adj2", 50_000.0, h / ss);
            let (x1, x2) = (hc - dx, hc + dx);
            if name == "upArrow" {
                polygon(&[(x1, h), (x1, head), (0.0, head), (hc, 0.0), (w, head), (x2, head), (x2, h)])
            } else {
                let y = h - head;
                polygon(&[(x1, 0.0), (x1, y), (0.0, y), (hc, h), (w, y), (x2, y), (x2, 0.0)])
            }
        }
        "leftRightArrow" => {
            let dy = h * frac("adj1", 50_000.0, 1.0) / 2.0;
            let head = ss * frac("adj2", 50_000.0, w / ss / 2.0);
            let (y1, y2) = (vc - dy, vc + dy);
            polygon(&[(0.0, vc), (head, 0.0), (head, y1), (w - head, y1), (w - head, 0.0), (w, vc), (w - head, h), (w - head, y2), (head, y2), (head, h)])
        }
        "upDownArrow" => {
            let dx = w * frac("adj1", 50_000.0, 1.0) / 2.0;
            let head = ss * frac("adj2", 50_000.0, h / ss / 2.0);
            let (x1, x2) = (hc - dx, hc + dx);
            polygon(&[(hc, 0.0), (w, head), (x2, head), (x2, h - head), (w, h - head), (hc, h), (0.0, h - head), (x1, h - head), (x1, head), (0.0, head)])
        }
        "bentArrow" => {
            let t = ss * frac("adj1", 25_000.0, 0.5);
            let head_w = ss * frac("adj2", 25_000.0, 0.5);
            let head = ss * frac("adj3", 25_000.0, 0.5);
            let r = ss * frac("adj4", 43_750.0, 1.0);
            let top = head_w - t / 2.0;
            let y_mid = head_w;
            let mut pen = Pen::default();
            pen.move_to(0.0, h).line_to(0.0, top + r.max(0.0));
            pen.arc_to(r, r, 180.0, 90.0).line_to(w - head, top).line_to(w - head, 0.0).line_to(w, y_mid).line_to(w - head, 2.0 * head_w).line_to(w - head, top + t);
            let ri = (r - t).max(0.0);
            pen.line_to(t + ri, top + t);
            if ri > 0.0 {
                pen.arc_to(ri, ri, 270.0, -90.0);
            }
            pen.line_to(t, h).close();
            paths(vec![pen.filled()])
        }
        "wedgeRectCallout" | "wedgeRoundRectCallout" | "wedgeEllipseCallout" => {
            let tip = (hc + w * adj("adj1", -20_833.0) / 100_000.0, vc + h * adj("adj2", 62_500.0) / 100_000.0);
            if name == "wedgeEllipseCallout" {
                let mut pen = Pen::default();
                pen.move_to(0.0, vc).arc_to(hc, vc, 180.0, 360.0).close();
                let a = (tip.1 - vc).atan2(tip.0 - hc).to_degrees();
                let (b1, b2) = (on_ellipse(hc, vc, hc, vc, a - 12.0), on_ellipse(hc, vc, hc, vc, a + 12.0));
                return Some(paths(vec![pen.filled(), Pen::polygon(&[b1, tip, b2]).filled()]));
            }
            let r = if name == "wedgeRoundRectCallout" { ss * frac("adj3", 16_667.0, 0.5) } else { 0.0 };
            paths(vec![callout(w, h, r, tip).filled()])
        }
        "line" | "straightConnector1" => paths(vec![{
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).line_to(w, h);
            pen.open()
        }]),
        "bentConnector2" => paths(vec![{
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).line_to(w, 0.0).line_to(w, h);
            pen.open()
        }]),
        "bentConnector3" => paths(vec![{
            let x = w * adj("adj1", 50_000.0) / 100_000.0;
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).line_to(x, 0.0).line_to(x, h).line_to(w, h);
            pen.open()
        }]),
        "bentConnector4" => paths(vec![{
            let x = w * adj("adj1", 50_000.0) / 100_000.0;
            let y = h * adj("adj2", 50_000.0) / 100_000.0;
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).line_to(x, 0.0).line_to(x, y).line_to(w, y).line_to(w, h);
            pen.open()
        }]),
        "curvedConnector2" => paths(vec![{
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).cubic([hc, 0.0, w, vc, w, h]);
            pen.open()
        }]),
        "curvedConnector3" => paths(vec![{
            let x2 = w * adj("adj1", 50_000.0) / 100_000.0;
            let mut pen = Pen::default();
            pen.move_to(0.0, 0.0).cubic([x2 / 2.0, 0.0, x2, h / 4.0, x2, vc]).cubic([x2, h * 0.75, (w + x2) / 2.0, h, w, h]);
            pen.open()
        }]),
        "leftBracket" | "rightBracket" => paths(vec![{
            let r = (h * frac("adj", 8_333.0, 0.5)).min(w);
            let mut pen = Pen::default();
            if name == "leftBracket" {
                pen.move_to(w, h).arc_to(w, r, 90.0, 90.0).line_to(0.0, r).arc_to(w, r, 180.0, 90.0);
            } else {
                pen.move_to(0.0, 0.0).arc_to(w, r, 270.0, 90.0).line_to(w, h - r).arc_to(w, r, 0.0, 90.0);
            }
            pen.open()
        }]),
        "leftBrace" | "rightBrace" => paths(vec![{
            let r = (h * frac("adj1", 8_333.0, 0.25)).min(w);
            let mid = h * frac("adj2", 50_000.0, 1.0);
            let half = w / 2.0;
            let mut pen = Pen::default();
            if name == "leftBrace" {
                pen.move_to(w, h).arc_to(half, r, 90.0, 90.0).line_to(half, mid + r).arc_to(half, r, 0.0, -90.0).arc_to(half, r, 90.0, -90.0).line_to(half, r).arc_to(half, r, 180.0, 90.0);
            } else {
                pen.move_to(0.0, 0.0).arc_to(half, r, 270.0, 90.0).line_to(half, mid - r).arc_to(half, r, 180.0, -90.0).arc_to(half, r, 270.0, -90.0).line_to(half, h - r).arc_to(half, r, 0.0, 90.0);
            }
            pen.open()
        }]),
        "bracketPair" | "bracePair" => {
            let r = ss * frac("adj", 16_667.0, 0.5);
            let mut left = Pen::default();
            left.move_to(r, h).arc_to(r, r, 90.0, 90.0).line_to(0.0, r).arc_to(r, r, 180.0, 90.0);
            let mut right = Pen::default();
            right.move_to(w - r, 0.0).arc_to(r, r, 270.0, 90.0).line_to(w, h - r).arc_to(r, r, 0.0, 90.0);
            paths(vec![left.open(), right.open()])
        }
        _ => return None,
    })
}

/// A (rounded) rectangle with a pointer to `tip`, starting on the side it points from
fn callout(w: f32, h: f32, r: f32, tip: (f32, f32)) -> Pen {
    let (dx, dy) = (tip.0 - w / 2.0, tip.1 - h / 2.0);
    let inside = (0.0..=w).contains(&tip.0) && (0.0..=h).contains(&tip.1);
    // Which side the pointer leaves from, and where its base sits on that side
    let side = if inside {
        None
    } else if dy.abs() * w > dx.abs() * h {
        Some(if dy > 0.0 { 2 } else { 0 })
    } else {
        Some(if dx > 0.0 { 1 } else { 3 })
    };
    let base = |len: f32, toward: f32| if toward < 0.0 { (len / 12.0 * 2.0, len / 12.0 * 5.0) } else { (len / 12.0 * 7.0, len / 12.0 * 10.0) };
    let mut pen = Pen::default();
    pen.move_to(r, 0.0);
    if side == Some(0) {
        let (a, b) = base(w, dx);
        pen.line_to(a, 0.0).line_to(tip.0, tip.1).line_to(b, 0.0);
    }
    pen.line_to(w - r, 0.0).arc_to(r, r, 270.0, 90.0);
    if side == Some(1) {
        let (a, b) = base(h, dy);
        pen.line_to(w, a).line_to(tip.0, tip.1).line_to(w, b);
    }
    pen.line_to(w, h - r).arc_to(r, r, 0.0, 90.0);
    if side == Some(2) {
        let (a, b) = base(w, dx);
        pen.line_to(b, h).line_to(tip.0, tip.1).line_to(a, h);
    }
    pen.line_to(r, h).arc_to(r, r, 90.0, 90.0);
    if side == Some(3) {
        let (a, b) = base(h, dy);
        pen.line_to(0.0, b).line_to(tip.0, tip.1).line_to(0.0, a);
    }
    pen.line_to(0.0, r).arc_to(r, r, 180.0, 90.0).close();
    pen
}

/// Where a preset shape keeps its text: distances from the left, top, right and
/// bottom of its box
pub fn text_rect(name: &str, adj: &dyn Fn(&str, f32) -> f32, w: f32, h: f32) -> Option<[f32; 4]> {
    let ss = w.min(h);
    let frac = |name: &str, default: f32| (adj(name, default) / 100_000.0).clamp(0.0, 2.0);
    Some(match name {
        "ellipse" | "flowChartConnector" | "donut" | "cloud" | "wedgeEllipseCallout" => [w * 0.1464, h * 0.1464, w * 0.1464, h * 0.1464],
        "roundRect" => {
            let r = frac("adj", 16_667.0).min(0.5) * ss * 0.2929;
            [r, r, r, r]
        }
        "triangle" => {
            let x1 = w * frac("adj", 50_000.0).min(1.0) / 2.0;
            [x1, h / 2.0, w - x1 - w / 2.0, 0.0]
        }
        "diamond" | "flowChartDecision" => [w / 4.0, h / 4.0, w / 4.0, h / 4.0],
        "rightArrow" | "leftArrow" => {
            let band = h * (1.0 - frac("adj1", 50_000.0).min(1.0)) / 2.0;
            let head = ss * frac("adj2", 50_000.0) / 2.0;
            if name == "rightArrow" { [0.0, band, head, band] } else { [head, band, 0.0, band] }
        }
        "upArrow" | "downArrow" => {
            let band = w * (1.0 - frac("adj1", 50_000.0).min(1.0)) / 2.0;
            let head = ss * frac("adj2", 50_000.0) / 2.0;
            if name == "downArrow" { [band, 0.0, band, head] } else { [band, head, band, 0.0] }
        }
        "parallelogram" | "flowChartInputOutput" => [w / 6.0, h / 12.0, w / 6.0, h / 12.0],
        "hexagon" | "octagon" => [w * 0.15, h * 0.15, w * 0.15, h * 0.15],
        "chevron" | "homePlate" => [ss * 0.25, 0.0, ss * frac("adj", 50_000.0) / 2.0, 0.0],
        "pentagon" => [w * 0.19, h * 0.38, w * 0.19, 0.0],
        "star5" => [w * 0.31, h * 0.38, w * 0.31, h * 0.25],
        "heart" => [w * 0.17, h * 0.25, w * 0.17, h * 0.3],
        "can" => [0.0, ss * frac("adj", 25_000.0), 0.0, ss * frac("adj", 25_000.0) / 2.0],
        _ => return None,
    })
}

/// Custom geometry's paths (each: its width and height units, filled, stroked, steps)
/// as subpaths in the w × h box
pub fn custom(paths_list: &[(f32, f32, bool, bool, Vec<CustomStep>)], w: f32, h: f32) -> Option<Geometry> {
    let mut out = vec![];
    for (pw, ph, fill, stroke, steps) in paths_list {
        let (sx, sy) = (if *pw > 0.0 { w / pw } else { 1.0 }, if *ph > 0.0 { h / ph } else { 1.0 });
        let mut pen = Pen::default();
        for step in steps {
            match *step {
                CustomStep::Move(x, y) => {
                    pen.move_to(x * sx, y * sy);
                }
                CustomStep::Line(x, y) => {
                    pen.line_to(x * sx, y * sy);
                }
                CustomStep::Cubic(c) => {
                    pen.cubic([c[0] * sx, c[1] * sy, c[2] * sx, c[3] * sy, c[4] * sx, c[5] * sy]);
                }
                CustomStep::Quad(qx, qy, x, y) => {
                    pen.quad(qx * sx, qy * sy, x * sx, y * sy);
                }
                CustomStep::Arc(rx, ry, start, sweep) => {
                    pen.arc_to(rx * sx, ry * sy, start, sweep);
                }
                CustomStep::Close => {
                    pen.close();
                }
            }
        }
        if !pen.commands.is_empty() {
            out.push(SubPath { commands: pen.commands, fill: if *fill { PathFill::Shape } else { PathFill::None }, stroke: *stroke, even_odd: true });
        }
    }
    (!out.is_empty()).then_some(Geometry::Paths(out))
}

#[derive(Clone, Copy, Debug)]
pub enum CustomStep {
    Move(f32, f32),
    Line(f32, f32),
    Cubic([f32; 6]),
    Quad(f32, f32, f32, f32),
    /// Radii, start angle and sweep in degrees
    Arc(f32, f32, f32, f32),
    Close,
}

/// How a line ends
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineEnd {
    Triangle,
    Stealth,
    Arrow,
    Diamond,
    Oval,
}

impl LineEnd {
    pub fn parse(kind: &str) -> Option<LineEnd> {
        Some(match kind {
            "triangle" => LineEnd::Triangle,
            "stealth" => LineEnd::Stealth,
            "arrow" => LineEnd::Arrow,
            "diamond" => LineEnd::Diamond,
            "oval" => LineEnd::Oval,
            _ => return None,
        })
    }
}

type Pt = (f32, f32);

/// The first and last points of a geometry's first stroked path with where each
/// end comes from (for its arrowhead's direction)
fn ends(geometry: &Geometry) -> Option<[(Pt, Pt); 2]> {
    let Geometry::Paths(subs) = geometry else { return None };
    let sub = subs.iter().find(|s| s.stroke)?;
    let mut points = vec![];
    for c in &sub.commands {
        match *c {
            PathCommand::Move(x, y) | PathCommand::Line(x, y) => points.push((x, y)),
            PathCommand::Cubic(c) => {
                points.push((c[0], c[1]));
                points.push((c[2], c[3]));
                points.push((c[4], c[5]));
            }
            PathCommand::Close => return None,
        }
    }
    let distinct = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1) > 0.01;
    let first = *points.first()?;
    let from_first = points.iter().copied().find(|&p| distinct(p, first))?;
    let last = *points.last()?;
    let from_last = points.iter().rev().copied().find(|&p| distinct(p, last))?;
    Some([(first, from_first), (last, from_last)])
}

/// Arrowheads for a line `width` points wide: `head` at the path's start, `tail` at
/// its end, each with its width and length multiples of the line width
pub fn arrowheads(geometry: &Geometry, width: f32, head: Option<(LineEnd, f32, f32)>, tail: Option<(LineEnd, f32, f32)>) -> Vec<SubPath> {
    let Some(ends) = ends(geometry) else { return vec![] };
    let mut out = vec![];
    for (end, (tip, from)) in [head, tail].into_iter().zip(ends) {
        let Some((kind, wm, lm)) = end else { continue };
        let (half, len) = (width.max(0.75) * wm / 2.0, width.max(0.75) * lm);
        let angle = (tip.1 - from.1).atan2(tip.0 - from.0);
        let (c, s) = (angle.cos(), angle.sin());
        // local (along, across) -> box coordinates
        let at = |along: f32, across: f32| (tip.0 + along * c - across * s, tip.1 + along * s + across * c);
        let sub = match kind {
            LineEnd::Triangle => Pen::polygon(&[at(0.0, 0.0), at(-len, half), at(-len, -half)]),
            LineEnd::Stealth => Pen::polygon(&[at(0.0, 0.0), at(-len, half), at(-len * 0.6, 0.0), at(-len, -half)]),
            LineEnd::Diamond => Pen::polygon(&[at(len / 2.0, 0.0), at(0.0, half), at(-len / 2.0, 0.0), at(0.0, -half)]),
            LineEnd::Oval => {
                let mut pen = Pen::default();
                let (cx, cy) = at(0.0, 0.0);
                pen.move_to(cx + len / 2.0, cy).arc_to(len / 2.0, half, 0.0, 360.0).close();
                pen
            }
            LineEnd::Arrow => {
                let mut pen = Pen::default();
                let (a, b, t) = (at(-len, half), at(-len, -half), at(0.0, 0.0));
                pen.move_to(a.0, a.1).line_to(t.0, t.1).line_to(b.0, b.1);
                out.push(pen.open());
                continue;
            }
        };
        out.push(SubPath { commands: sub.commands, fill: PathFill::Line, stroke: false, even_odd: false });
    }
    out
}

/// Size multiples PowerPoint uses for "sm", "med" and "lg" line ends
pub fn end_size(v: Option<&str>) -> f32 {
    match v {
        Some("sm") => 2.0,
        Some("lg") => 5.0,
        _ => 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(g: &Geometry) -> (f32, f32, f32, f32) {
        let Geometry::Paths(subs) = g else { panic!("not a path") };
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut at = (0.0, 0.0);
        for s in subs {
            for c in &s.commands {
                // Curves are sampled along their length (control points lie outside)
                let pts: Vec<(f32, f32)> = match *c {
                    PathCommand::Move(x, y) | PathCommand::Line(x, y) => vec![(x, y)],
                    PathCommand::Cubic(c) => (1..=8)
                        .map(|i| {
                            let t = i as f32 / 8.0;
                            let u = 1.0 - t;
                            let f = |p0: f32, p1: f32, p2: f32, p3: f32| u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3;
                            (f(at.0, c[0], c[2], c[4]), f(at.1, c[1], c[3], c[5]))
                        })
                        .collect(),
                    PathCommand::Close => vec![],
                };
                if let Some(&last) = pts.last() {
                    at = last;
                }
                for (x, y) in pts {
                    b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                }
            }
        }
        b
    }

    #[test]
    fn presets_fill_their_box() {
        let none = |_: &str, d: f32| d;
        for name in ["hexagon", "chevron", "rightArrow", "heart", "donut", "star5", "wedgeRectCallout", "trapezoid", "octagon"] {
            let g = preset(name, &none, 200.0, 100.0).unwrap();
            let (x0, y0, x1, y1) = bounds(&g);
            assert!(x0 >= -2.0 && y0 >= -40.0 && x1 <= 202.0 && y1 <= 140.0, "{name}: {x0} {y0} {x1} {y1}");
            assert!(x1 - x0 > 150.0, "{name} is too narrow");
        }
        // A half ring from 180° to 0° stays in the top half
        let g = preset("blockArc", &none, 100.0, 100.0).unwrap();
        let (_, y0, _, y1) = bounds(&g);
        assert!(y0 < 1.0 && y1 <= 50.5, "{y0} {y1}");
        // A quarter pie (0° to 270°) reaches the right, bottom and left
        let (x0, _, x1, y1) = bounds(&preset("pie", &none, 100.0, 100.0).unwrap());
        assert!(x0 < 1.0 && x1 > 99.0 && y1 > 99.0);
    }
}
