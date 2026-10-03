//! Paint shared by the schedule and the netlist: framed cards, rounded wires,
//! junction dots, arrowheads, pills, and labels.
//!
//! Every rectangle and polygon sits on whole pixels. A wire is a 2px stroke
//! centred on an integer coordinate, so its straight runs cover exactly two
//! pixel columns or rows at any whole-number scale. Only a corner, a curve,
//! and text are antialiased.

use crate::scheme::Scheme;
use crate::w::{prettify, push_esc, push_i64};

pub(crate) const FONT: &str =
    "Inter, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif";

/// Width of a wire stroke.
pub(crate) const WIRE: i64 = 2;

/// The root `<svg>`, the paper, and the translated group every picture draws in.
#[allow(clippy::too_many_arguments)]
pub(crate) fn begin(
    out: &mut Vec<u8>,
    class: &str,
    width: i64,
    height: i64,
    margin: i64,
    plot: (i64, i64),
    title: &str,
    desc: &str,
    scheme: &Scheme,
) {
    out.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"");
    out.extend_from_slice(class.as_bytes());
    out.extend_from_slice(b"\" role=\"img\" aria-labelledby=\"diagram-title\" width=\"");
    push_i64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" viewBox=\"0 0 ");
    push_i64(out, width);
    out.push(b' ');
    push_i64(out, height);
    out.extend_from_slice(b"\" overflow=\"hidden\" font-family=\"");
    out.extend_from_slice(FONT.as_bytes());
    out.extend_from_slice(b"\" font-size=\"12\" fill=\"");
    out.extend_from_slice(scheme.text.as_bytes());
    out.extend_from_slice(b"\" stroke=\"none\"><title id=\"diagram-title\">");
    push_esc(out, title);
    out.extend_from_slice(b"</title><desc>");
    push_esc(out, desc);
    out.extend_from_slice(
        b"</desc><defs><clipPath id=\"plot-clip\"><rect x=\"0\" y=\"0\" width=\"",
    );
    push_i64(out, plot.0);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, plot.1);
    out.extend_from_slice(b"\"/></clipPath></defs><rect width=\"");
    push_i64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(scheme.paper.as_bytes());
    out.extend_from_slice(b"\"/><g transform=\"translate(");
    push_i64(out, margin);
    out.push(b',');
    push_i64(out, margin);
    out.extend_from_slice(b")\">");
}

/// Close the picture, scope its ids, and pretty-print it when asked.
pub(crate) fn finish(out: &mut Vec<u8>, indent: u8) {
    out.extend_from_slice(b"</g></svg>");
    crate::emit::scope_ids(out);
    if indent > 0 {
        let pretty = prettify(out, indent);
        out.clear();
        out.extend_from_slice(&pretty);
    }
}

pub(crate) fn rect(out: &mut Vec<u8>, x: i64, y: i64, w: i64, h: i64, rx: i64, fill: &str) {
    if w <= 0 || h <= 0 {
        return;
    }
    out.extend_from_slice(b"<rect x=\"");
    push_i64(out, x);
    out.extend_from_slice(b"\" y=\"");
    push_i64(out, y);
    out.extend_from_slice(b"\" width=\"");
    push_i64(out, w);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, h);
    if rx > 0 {
        out.extend_from_slice(b"\" rx=\"");
        push_i64(out, rx.min(w / 2).min(h / 2));
    }
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\"/>");
}

/// A soft drop shadow under a card: the same rounded shape, nudged down.
pub(crate) fn shadow(out: &mut Vec<u8>, x: i64, y: i64, w: i64, h: i64, rx: i64, scheme: &Scheme) {
    if w <= 0 || h <= 0 {
        return;
    }
    out.extend_from_slice(b"<rect x=\"");
    push_i64(out, x);
    out.extend_from_slice(b"\" y=\"");
    push_i64(out, y + 2);
    out.extend_from_slice(b"\" width=\"");
    push_i64(out, w);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, h);
    out.extend_from_slice(b"\" rx=\"");
    push_i64(out, rx);
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(scheme.shadow.as_bytes());
    out.extend_from_slice(b"\" fill-opacity=\"0.12\"/>");
}

/// A card with a 1px border: an ink rectangle and a fill inset by one pixel.
#[allow(clippy::too_many_arguments)]
pub(crate) fn framed(
    out: &mut Vec<u8>,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    rx: i64,
    ink: &str,
    fill: &str,
) {
    rect(out, x, y, w, h, rx, ink);
    rect(out, x + 1, y + 1, w - 2, h - 2, (rx - 1).max(0), fill);
}

pub(crate) struct Text<'a> {
    pub x: i64,
    pub y: i64,
    pub anchor: &'static str,
    pub size: i64,
    pub weight: Option<u16>,
    pub fill: Option<&'a str>,
}

pub(crate) fn text(out: &mut Vec<u8>, style: Text<'_>, value: &str) {
    out.extend_from_slice(b"<text x=\"");
    push_i64(out, style.x);
    out.extend_from_slice(b"\" y=\"");
    push_i64(out, style.y);
    out.extend_from_slice(b"\" text-anchor=\"");
    out.extend_from_slice(style.anchor.as_bytes());
    out.extend_from_slice(b"\" font-size=\"");
    push_i64(out, style.size);
    out.push(b'"');
    if let Some(weight) = style.weight {
        out.extend_from_slice(b" font-weight=\"");
        push_i64(out, i64::from(weight));
        out.push(b'"');
    }
    if let Some(fill) = style.fill {
        out.extend_from_slice(b" fill=\"");
        out.extend_from_slice(fill.as_bytes());
        out.push(b'"');
    }
    out.extend_from_slice(b" xml:space=\"preserve\">");
    push_esc(out, value);
    out.extend_from_slice(b"</text>");
}

/// The title or footer, centred over the picture.
pub(crate) fn heading(
    out: &mut Vec<u8>,
    value: &str,
    at: (i64, i64),
    size: i64,
    fill: Option<&str>,
) {
    text(
        out,
        Text {
            x: at.0,
            y: at.1,
            anchor: "middle",
            size,
            weight: Some(if size >= 14 { 650 } else { 500 }),
            fill,
        },
        value,
    );
}

pub(crate) fn polygon(out: &mut Vec<u8>, points: &[(i64, i64)], fill: &str) {
    out.extend_from_slice(b"<polygon points=\"");
    for (index, &(x, y)) in points.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        push_i64(out, x);
        out.push(b',');
        push_i64(out, y);
    }
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\"/>");
}

/// A downward arrowhead whose tip touches `(x, y)`.
pub(crate) fn arrow_down(out: &mut Vec<u8>, x: i64, y: i64, fill: &str) {
    polygon(out, &[(x - 4, y - 7), (x + 4, y - 7), (x, y + 1)], fill);
}

/// An arrowhead at `to`, pointing along the run from `from`.
pub(crate) fn arrow(out: &mut Vec<u8>, from: (i64, i64), to: (i64, i64), fill: &str) {
    let (x, y) = to;
    let points = if from.0 == to.0 && from.1 <= to.1 {
        [(x - 4, y - 7), (x + 4, y - 7), (x, y + 1)]
    } else if from.0 == to.0 {
        [(x - 4, y + 7), (x + 4, y + 7), (x, y - 1)]
    } else if from.0 < to.0 {
        [(x - 7, y - 4), (x - 7, y + 4), (x + 1, y)]
    } else {
        [(x + 7, y - 4), (x + 7, y + 4), (x - 1, y)]
    };
    polygon(out, &points, fill);
}

pub(crate) fn circle(out: &mut Vec<u8>, cx: i64, cy: i64, r: i64, fill: &str) {
    out.extend_from_slice(b"<circle cx=\"");
    push_i64(out, cx);
    out.extend_from_slice(b"\" cy=\"");
    push_i64(out, cy);
    out.extend_from_slice(b"\" r=\"");
    push_i64(out, r);
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\"/>");
}

/// A stroked outline, such as a gate body.
pub(crate) fn outline(out: &mut Vec<u8>, d: &str, fill: &str, stroke: &str) {
    out.extend_from_slice(b"<path d=\"");
    out.extend_from_slice(d.as_bytes());
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\" stroke=\"");
    out.extend_from_slice(stroke.as_bytes());
    out.extend_from_slice(b"\" stroke-width=\"2\" stroke-linejoin=\"round\"/>");
}

/// The soft shadow of an outline: the caller passes the shape nudged down.
pub(crate) fn outline_shadow(out: &mut Vec<u8>, d: &str, scheme: &Scheme) {
    out.extend_from_slice(b"<path d=\"");
    out.extend_from_slice(d.as_bytes());
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(scheme.shadow.as_bytes());
    out.extend_from_slice(b"\" fill-opacity=\"0.12\"/>");
}

/// A stroked circle, such as an inversion bubble.
pub(crate) fn ring(out: &mut Vec<u8>, cx: i64, cy: i64, r: i64, fill: &str, stroke: &str) {
    out.extend_from_slice(b"<circle cx=\"");
    push_i64(out, cx);
    out.extend_from_slice(b"\" cy=\"");
    push_i64(out, cy);
    out.extend_from_slice(b"\" r=\"");
    push_i64(out, r);
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\" stroke=\"");
    out.extend_from_slice(stroke.as_bytes());
    out.extend_from_slice(b"\" stroke-width=\"2\"/>");
}

/// An orthogonal wire through `points`, with each bend rounded by up to `radius`.
pub(crate) fn wire(out: &mut Vec<u8>, points: &[(i64, i64)], radius: i64, stroke: &str) {
    let points = simplify(points);
    if points.len() < 2 {
        return;
    }
    out.extend_from_slice(b"<path d=\"");
    out.extend_from_slice(rounded(&points, radius).as_bytes());
    out.extend_from_slice(b"\" fill=\"none\" stroke=\"");
    out.extend_from_slice(stroke.as_bytes());
    out.extend_from_slice(b"\" stroke-width=\"");
    push_i64(out, WIRE);
    out.extend_from_slice(b"\" stroke-linejoin=\"round\"/>");
}

/// Drop repeated points and the middle of straight runs.
pub(crate) fn simplify(points: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut kept: Vec<(i64, i64)> = Vec::with_capacity(points.len());
    for &point in points {
        if kept.last() == Some(&point) {
            continue;
        }
        if kept.len() >= 2 {
            let a = kept[kept.len() - 2];
            let b = kept[kept.len() - 1];
            if (a.0 == b.0 && b.0 == point.0) || (a.1 == b.1 && b.1 == point.1) {
                kept.pop();
            }
        }
        kept.push(point);
    }
    kept
}

fn rounded(points: &[(i64, i64)], radius: i64) -> String {
    use std::fmt::Write;
    let last = points.len() - 1;
    let mut d = format!("M{} {}", points[0].0, points[0].1);
    for i in 1..last {
        let (px, py) = points[i - 1];
        let (cx, cy) = points[i];
        let (nx, ny) = points[i + 1];
        let before = (cx - px).abs() + (cy - py).abs();
        let after = (nx - cx).abs() + (ny - cy).abs();
        let room_before = if i == 1 { before } else { before / 2 };
        let room_after = if i + 1 == last { after } else { after / 2 };
        let r = radius.min(room_before).min(room_after).max(0);
        let ax = cx - (cx - px).signum() * r;
        let ay = cy - (cy - py).signum() * r;
        let bx = cx + (nx - cx).signum() * r;
        let by = cy + (ny - cy).signum() * r;
        let _ = write!(d, "L{ax} {ay}");
        if r > 0 {
            let _ = write!(d, "Q{cx} {cy} {bx} {by}");
        }
    }
    let _ = write!(d, "L{} {}", points[last].0, points[last].1);
    d
}

/// Points where one net branches: three or more arms meet. A corner has two
/// arms, and two different nets that cross are never passed in together.
pub(crate) fn junctions(paths: &[Vec<(i64, i64)>]) -> Vec<(i64, i64)> {
    // Maximal horizontal runs keyed by y, vertical runs keyed by x.
    let mut flat: Vec<(i64, i64, i64)> = Vec::new();
    let mut tall: Vec<(i64, i64, i64)> = Vec::new();
    for path in paths {
        let path = simplify(path);
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.1 == b.1 {
                flat.push((a.1, a.0.min(b.0), a.0.max(b.0)));
            } else if a.0 == b.0 {
                tall.push((a.0, a.1.min(b.1), a.1.max(b.1)));
            }
        }
    }
    let flat = merge(flat);
    let tall = merge(tall);
    let mut candidates: Vec<(i64, i64)> = Vec::new();
    for &(y, x0, x1) in &flat {
        candidates.push((x0, y));
        candidates.push((x1, y));
    }
    for &(x, y0, y1) in &tall {
        candidates.push((x, y0));
        candidates.push((x, y1));
    }
    candidates.sort_unstable();
    candidates.dedup();
    let arms = |(px, py): (i64, i64)| -> usize {
        let mut count = 0;
        for &(y, x0, x1) in &flat {
            if y == py && x0 <= px && px <= x1 {
                count += if px == x0 || px == x1 { 1 } else { 2 };
            }
        }
        for &(x, y0, y1) in &tall {
            if x == px && y0 <= py && py <= y1 {
                count += if py == y0 || py == y1 { 1 } else { 2 };
            }
        }
        count
    };
    candidates.into_iter().filter(|&p| arms(p) >= 3).collect()
}

fn merge(mut runs: Vec<(i64, i64, i64)>) -> Vec<(i64, i64, i64)> {
    runs.sort_unstable();
    let mut merged: Vec<(i64, i64, i64)> = Vec::with_capacity(runs.len());
    for run in runs {
        if let Some(last) = merged.last_mut() {
            if last.0 == run.0 && run.1 <= last.2 {
                last.2 = last.2.max(run.2);
                continue;
            }
        }
        merged.push(run);
    }
    merged
}

/// Text plus padding, snapped outward to 4px.
pub(crate) fn snap(n: i64) -> i64 {
    (n.max(0) + 3) & !3
}

/// Advance of `value` at `size`, with room for a bold weight.
pub(crate) fn measure(value: &str, size: f64, bold: bool) -> i64 {
    let width = crate::text_width(value, size);
    (if bold { width * 1.06 } else { width }).ceil() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_round_and_branches_are_dotted() {
        let d = rounded(&[(0, 0), (0, 10), (20, 10)], 4);
        assert_eq!(d, "M0 0L0 6Q0 10 4 10L20 10");
        let short = rounded(&[(0, 0), (0, 10), (2, 10), (2, 20)], 4);
        assert!(short.contains("Q0 10 1 10"), "{short}");
        let tee = junctions(&[
            vec![(10, 0), (10, 10), (0, 10), (0, 20)],
            vec![(10, 0), (10, 10), (30, 10), (30, 20)],
        ]);
        assert_eq!(tee, vec![(10, 10)]);
        let along = junctions(&[
            vec![(0, 0), (0, 10), (10, 10), (10, 20)],
            vec![(0, 0), (0, 10), (30, 10), (30, 20)],
        ]);
        assert_eq!(along, vec![(10, 10)]);
        assert!(junctions(&[vec![(0, 0), (0, 10), (10, 10)]]).is_empty());
        assert_eq!(
            simplify(&[(0, 0), (0, 5), (0, 9), (0, 9), (4, 9)]),
            vec![(0, 0), (0, 9), (4, 9)]
        );
    }
}
