//! Line charts of the AHPSort II local priorities, drawn with a Slint `Path`.
//!
//! Everything is computed here as plain numbers and SVG path commands in the coordinates of a
//! `width x height` plot box (y pointing down, as in Slint), so it can be unit tested.

/// Size of the plot box of every local-priority chart (logical pixels).
pub const PLOT_WIDTH: f64 = 230.0;
pub const PLOT_HEIGHT: f64 = 130.0;
/// Margin between the plot box border and the outermost point (room for the markers).
const PAD: f64 = 6.0;
/// Half the size of a point marker.
const MARKER: f64 = 3.5;
/// Ticks per axis (both ends included).
const TICKS: usize = 3;

/// A tick of an axis: position along the axis inside the plot box and its label.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub pos: f64,
    pub label: String,
}

/// Geometry of a chart inside its plot box.
#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub width: f64,
    pub height: f64,
    /// Polyline through the points, in the given order.
    pub line: String,
    /// One small diamond per point.
    pub markers: String,
    pub x_ticks: Vec<Tick>,
    pub y_ticks: Vec<Tick>,
}

/// Linear map of the interval `[lo, hi]` onto `[from, to]`; a degenerate interval (a single
/// value) maps to the middle.
#[derive(Debug, Clone, Copy)]
struct Axis {
    lo: f64,
    hi: f64,
    from: f64,
    to: f64,
}

impl Axis {
    fn degenerate(&self) -> bool {
        (self.hi - self.lo).abs() <= 1e-12 * self.hi.abs().max(1.0)
    }

    fn map(&self, v: f64) -> f64 {
        if self.degenerate() {
            (self.from + self.to) / 2.0
        } else {
            self.from + (v - self.lo) / (self.hi - self.lo) * (self.to - self.from)
        }
    }

    /// Evenly spaced tick values (just one for a degenerate axis).
    fn ticks(&self, label: impl Fn(f64) -> String) -> Vec<Tick> {
        let values: Vec<f64> = if self.degenerate() {
            vec![self.lo]
        } else {
            (0..TICKS)
                .map(|k| self.lo + (self.hi - self.lo) * k as f64 / (TICKS - 1) as f64)
                .collect()
        };
        values
            .into_iter()
            .map(|v| Tick {
                pos: self.map(v),
                label: label(v),
            })
            .collect()
    }
}

fn bounds(values: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    values.fold(None, |acc, v| match acc {
        None => Some((v, v)),
        Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
    })
}

/// Axes of the points inside a `width x height` box: X spans the values, Y goes from 0 (or
/// the minimum, if negative) to the maximum, bottom to top.
fn axes(points: &[(f64, f64)], width: f64, height: f64) -> (Axis, Axis) {
    let (x_lo, x_hi) = bounds(points.iter().map(|p| p.0)).unwrap_or((0.0, 0.0));
    let (y_lo, y_hi) = bounds(points.iter().map(|p| p.1)).unwrap_or((0.0, 0.0));
    let x = Axis {
        lo: x_lo,
        hi: x_hi,
        from: PAD,
        to: width - PAD,
    };
    let y = Axis {
        lo: y_lo.min(0.0),
        hi: y_hi,
        from: height - PAD,
        to: PAD,
    };
    (x, y)
}

/// Positions of the (finite) points inside a `width x height` box, in the given order.
pub fn map_points(points: &[(f64, f64)], width: f64, height: f64) -> Vec<(f64, f64)> {
    let points = finite(points);
    let (x, y) = axes(&points, width, height);
    points
        .iter()
        .map(|&(px, py)| (x.map(px), y.map(py)))
        .collect()
}

fn finite(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    points
        .iter()
        .copied()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect()
}

fn num(v: f64) -> String {
    format!("{v:.2}")
}

/// `M x0 y0 L x1 y1 ...` through `mapped`. Never empty, so Slint always gets a valid path.
pub fn line_path(mapped: &[(f64, f64)]) -> String {
    if mapped.is_empty() {
        return "M 0 0".into();
    }
    mapped
        .iter()
        .enumerate()
        .map(|(k, &(x, y))| format!("{} {} {}", if k == 0 { "M" } else { "L" }, num(x), num(y)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One closed diamond of half-size `r` around each point.
pub fn marker_path(mapped: &[(f64, f64)], r: f64) -> String {
    if mapped.is_empty() {
        return "M 0 0".into();
    }
    mapped
        .iter()
        .map(|&(x, y)| {
            format!(
                "M {} {} L {} {} L {} {} L {} {} Z",
                num(x),
                num(y - r),
                num(x + r),
                num(y),
                num(x),
                num(y + r),
                num(x - r),
                num(y)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Label of an X tick (original value): integers from 100 on, otherwise up to 2 decimals.
fn x_label(v: f64) -> String {
    if v.abs() >= 100.0 {
        format!("{}", v.round() as i64)
    } else {
        trim(format!("{v:.2}"))
    }
}

/// Label of a Y tick (priority): up to 3 decimals.
fn y_label(v: f64) -> String {
    trim(format!("{v:.3}"))
}

fn trim(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Chart of `points` (`(x, y)`, drawn in the given order) in the standard plot box.
pub fn chart(points: &[(f64, f64)]) -> Chart {
    chart_in(points, PLOT_WIDTH, PLOT_HEIGHT)
}

pub fn chart_in(points: &[(f64, f64)], width: f64, height: f64) -> Chart {
    let points = finite(points);
    let (x, y) = axes(&points, width, height);
    let mapped = map_points(&points, width, height);
    Chart {
        width,
        height,
        line: line_path(&mapped),
        markers: marker_path(&mapped, MARKER),
        x_ticks: if points.is_empty() {
            Vec::new()
        } else {
            x.ticks(x_label)
        },
        y_ticks: if points.is_empty() {
            Vec::new()
        } else {
            y.ticks(y_label)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 200.0;
    const H: f64 = 100.0;

    fn inside(p: (f64, f64)) -> bool {
        (PAD - 1e-9..=W - PAD + 1e-9).contains(&p.0) && (PAD - 1e-9..=H - PAD + 1e-9).contains(&p.1)
    }

    #[test]
    fn points_fill_the_box_with_y_pointing_up() {
        let pts = [(0.0, 0.1), (50.0, 0.3), (100.0, 0.05), (206.0, 0.02)];
        let m = map_points(&pts, W, H);
        assert!(m.iter().all(|&p| inside(p)), "{m:?}");
        // X: first point on the left edge, last one on the right edge, ascending.
        assert!((m[0].0 - PAD).abs() < 1e-9);
        assert!((m[3].0 - (W - PAD)).abs() < 1e-9);
        assert!(m.windows(2).all(|w| w[0].0 < w[1].0));
        // Y: the maximum is at the top, a higher priority is drawn higher (smaller y).
        assert!((m[1].1 - PAD).abs() < 1e-9);
        assert!(m[0].1 > m[1].1 && m[2].1 > m[0].1);
        // Y starts at 0: 0.02 is not at the bottom edge.
        assert!(m[3].1 < H - PAD);
    }

    #[test]
    fn line_and_markers_have_one_command_group_per_point() {
        let m = map_points(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.5)], W, H);
        let line = line_path(&m);
        assert!(line.starts_with("M 6.00 94.00 L 100.00 6.00"), "{line}");
        assert_eq!(line.matches('L').count(), 2);
        let markers = marker_path(&m, 3.0);
        assert_eq!(markers.matches('M').count(), 3);
        assert_eq!(markers.matches('Z').count(), 3);
    }

    #[test]
    fn degenerate_inputs_stay_inside_the_box() {
        // A single point: centred horizontally, at the top (it is the maximum).
        let one = map_points(&[(5.0, 0.4)], W, H);
        assert_eq!(one.len(), 1);
        assert!((one[0].0 - W / 2.0).abs() < 1e-9);
        assert!(inside(one[0]));

        // Every X equal: all on the vertical middle line.
        let same_x = map_points(&[(3.0, 0.1), (3.0, 0.2)], W, H);
        assert!(same_x.iter().all(|p| (p.0 - W / 2.0).abs() < 1e-9));

        // Every Y equal to 0: on the horizontal middle line, no NaN.
        let flat = map_points(&[(0.0, 0.0), (10.0, 0.0)], W, H);
        assert!(flat.iter().all(|p| (p.1 - H / 2.0).abs() < 1e-9));

        // Non-finite points are dropped; no points give a valid, empty-looking path.
        let m = map_points(&[(f64::NAN, 1.0), (1.0, f64::INFINITY), (2.0, 0.5)], W, H);
        assert_eq!(m.len(), 1);
        let empty = chart_in(&[], W, H);
        assert_eq!(empty.line, "M 0 0");
        assert!(empty.x_ticks.is_empty() && empty.y_ticks.is_empty());
    }

    #[test]
    fn ticks_label_both_axes() {
        let c = chart_in(&[(0.0, 0.05), (254572.0, 0.2)], W, H);
        let labels: Vec<_> = c.x_ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["0", "127286", "254572"]);
        assert!((c.x_ticks[0].pos - PAD).abs() < 1e-9);
        let labels: Vec<_> = c.y_ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["0", "0.1", "0.2"]);
        // The 0 tick is at the bottom, the maximum at the top.
        assert!(c.y_ticks[0].pos > c.y_ticks[2].pos);

        let single = chart_in(&[(1.5, 0.25)], W, H);
        assert_eq!(single.x_ticks.len(), 1);
        assert_eq!(single.x_ticks[0].label, "1.5");
    }
}
