//! Draw runs as patterns, rectangles, and short transition cells.

use crate::scheme::Scheme;
use crate::w::push_i64;
use crate::wave::{Code, Pat};
use crate::{XS, YS};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ClockKey {
    pub s0: Code,
    pub s1: Code,
    pub s2: Code,
    pub s3: Code,
    pub extra: i64,
}

pub(crate) struct Paint {
    pub body: Vec<u8>,
    pub clocks: Vec<ClockKey>,
    clock_ids: HashMap<ClockKey, usize>,
    pub hatch: bool,
    pub scheme: &'static Scheme,
    /// Solid or dashed subpaths waiting to share one `<path>`.
    pending: Vec<u8>,
    pending_dash: Option<bool>,
}

impl Paint {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with(&crate::scheme::LIGHT)
    }

    pub(crate) fn with(scheme: &'static Scheme) -> Self {
        Self {
            body: Vec::new(),
            clocks: Vec::new(),
            clock_ids: HashMap::new(),
            hatch: false,
            scheme,
            pending: Vec::new(),
            pending_dash: None,
        }
    }
}

fn flush_pending(paint: &mut Paint) {
    let Some(dash) = paint.pending_dash.take() else {
        return;
    };
    if paint.pending.is_empty() {
        return;
    }
    paint.body.extend_from_slice(b"<path d=\"");
    paint.body.append(&mut paint.pending);
    paint.body.push(b'"');
    if dash {
        paint.body.extend_from_slice(b" stroke-dasharray=\"1,3\"");
    }
    paint.body.extend_from_slice(b"/>");
}

fn stroke(paint: &mut Paint, dash: bool, draw: impl FnOnce(&mut Vec<u8>)) {
    if paint.pending_dash != Some(dash) {
        flush_pending(paint);
        paint.pending_dash = Some(dash);
    }
    draw(&mut paint.pending);
}

pub(crate) fn paint_wave(paint: &mut Paint, pats: &[Pat], skip: i64, end: i64) {
    if end <= skip {
        return;
    }
    paint.body.extend_from_slice(b"<g fill=\"none\" stroke=\"");
    paint.body.extend_from_slice(paint.scheme.ink.as_bytes());
    paint.body.extend_from_slice(
        b"\" stroke-width=\"1\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
    );
    for p in pats {
        let at = match *p {
            Pat::Fill { at, .. } | Pat::Lead { at, .. } | Pat::Clock { at, .. } => at,
        };
        if at >= end {
            break;
        }
        match *p {
            Pat::Fill { at, code, count } => {
                let lo = at.max(skip);
                let hi = (at + count).min(end);
                if lo >= hi {
                    continue;
                }
                draw_span(paint, code, (lo - skip) * XS, (hi - lo) * XS);
            }
            Pat::Lead {
                at,
                lead,
                hold,
                holds,
            } => {
                if at >= skip {
                    draw_one(paint, lead, (at - skip) * XS);
                }
                if holds > 0 {
                    let start = at + 1;
                    let lo = start.max(skip);
                    let hi = (start + holds).min(end);
                    if lo < hi {
                        draw_span(paint, hold, (lo - skip) * XS, (hi - lo) * XS);
                    }
                }
            }
            Pat::Clock {
                at,
                s0,
                s1,
                s2,
                s3,
                extra,
                times,
            } => {
                draw_clock(paint, at, s0, s1, s2, s3, extra, times, skip, end);
            }
        }
    }
    flush_pending(paint);
    paint.body.extend_from_slice(b"</g>");
}

#[expect(
    clippy::too_many_arguments,
    reason = "Destructured clock run plus the visible interval"
)]
fn draw_clock(
    paint: &mut Paint,
    at: i64,
    s0: Code,
    s1: Code,
    s2: Code,
    s3: Code,
    extra: i64,
    times: i64,
    skip: i64,
    end: i64,
) {
    let period = 2 * (extra + 1);
    let total = times * period;
    let hi = (at + total).min(end);
    let lo = at.max(skip);
    if lo >= hi || total <= 0 {
        return;
    }
    let key = ClockKey {
        s0,
        s1,
        s2,
        s3,
        extra,
    };
    let id = *paint.clock_ids.entry(key).or_insert_with(|| {
        paint.clocks.push(key);
        paint.clocks.len() - 1
    });
    let x_start = (at - skip) * XS;
    let x_end = (hi - skip) * XS;

    // Put tile seams on the flat part of the waveform, away from vertical
    // edges and arrows. The two small caps prevent the rectangle from clipping
    // the initial edge or painting an edge belonging to the *next* cycle.
    const PAD: i64 = 4;
    if at >= skip {
        clock_edge(paint, s0, x_start, x_start + PAD);
    }
    let left = (x_start + PAD).max(0);
    let complete = at + total <= end;
    let right = if complete { x_end - PAD } else { x_end };
    // Keep the pattern origin near the viewport even after skipping millions
    // of cycles. This also avoids unnecessarily large renderer transforms.
    let origin = if at < skip {
        -((skip - at) % period) * XS
    } else {
        x_start
    };
    flush_pending(paint);
    paint.body.extend_from_slice(b"<g transform=\"translate(");
    push_i64(&mut paint.body, origin);
    paint.body.extend_from_slice(b")\"><rect x=\"");
    push_i64(&mut paint.body, left - origin);
    paint.body.extend_from_slice(b"\" y=\"-2\" width=\"");
    push_i64(&mut paint.body, right - left);
    paint.body.extend_from_slice(b"\" height=\"");
    push_i64(&mut paint.body, YS + 4);
    paint.body.extend_from_slice(b"\" fill=\"url(#k");
    push_i64(&mut paint.body, id as i64);
    paint.body.extend_from_slice(b")\" stroke=\"none\"/></g>");
    if complete {
        draw_span(paint, s3, right, PAD);
    }
}

fn draw_span(paint: &mut Paint, code: Code, x: i64, w: i64) {
    if w <= 0 {
        return;
    }
    match code {
        Code::High | Code::DashH => hline(paint, x, w, 0, matches!(code, Code::DashH)),
        Code::Low | Code::DashL => hline(paint, x, w, YS, matches!(code, Code::DashL)),
        Code::Mid => hline(paint, x, w, YS / 2, false),
        Code::X => {
            paint.hatch = true;
            rect(paint, x, 0, w, YS, "url(#xh)");
            rails(paint, x, w);
        }
        Code::Bus(n) => {
            let fill = paint.scheme.bus(n);
            rect(paint, x, 0, w, YS, fill);
            rails(paint, x, w);
        }
        other => draw_one(paint, other, x),
    }
}

fn draw_one(paint: &mut Paint, code: Code, x: i64) {
    match code {
        Code::Rise => edge(paint, x, true, false),
        Code::Fall => edge(paint, x, false, false),
        Code::RiseA => edge(paint, x, true, true),
        Code::FallA => edge(paint, x, false, true),
        Code::Soft(prev, next) => draw_soft(paint, prev, next, x),
        other => draw_span(paint, other, x, XS),
    }
}

fn edge(paint: &mut Paint, x: i64, rise: bool, arrow: bool) {
    let code = match (rise, arrow) {
        (true, true) => Code::RiseA,
        (true, false) => Code::Rise,
        (false, true) => Code::FallA,
        (false, false) => Code::Fall,
    };
    clock_edge(paint, code, x, x + XS);
}

fn draw_soft(paint: &mut Paint, prev: u8, next: u8, x: i64) {
    let a = end_kind(prev);
    let b = end_kind(next);
    let scheme = paint.scheme;
    match (a, b) {
        (End::Bus(c1), End::Bus(c2)) => band_band(paint, x, scheme.bus(c1), scheme.bus(c2)),
        (End::Line { y, dash }, End::Bus(c)) => line_band(paint, x, y, dash, scheme.bus(c)),
        (End::Bus(c), End::Line { y, dash }) => band_line(paint, x, scheme.bus(c), y, dash),
        (End::X, End::Bus(c)) => {
            paint.hatch = true;
            band_band(paint, x, "url(#xh)", scheme.bus(c));
        }
        (End::Bus(c), End::X) => {
            paint.hatch = true;
            band_band(paint, x, scheme.bus(c), "url(#xh)");
        }
        (End::X, End::X) => draw_span(paint, Code::X, x, XS),
        (End::X, End::Line { y, dash }) => {
            paint.hatch = true;
            band_line(paint, x, "url(#xh)", y, dash);
        }
        (End::Line { y, dash }, End::X) => {
            paint.hatch = true;
            line_band(paint, x, y, dash, "url(#xh)");
        }
        (End::Line { y: y1, dash: d1 }, End::Line { y: y2, dash: d2 }) => {
            if y1 == y2 && !d1 && !d2 && (y1 == 0 || y1 == YS) {
                bump(paint, x, y1);
            } else if y1 == y2 {
                hline(paint, x, XS, y1, d1 || d2);
            } else {
                slope(paint, x, y1, y2, d1 || d2);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum End {
    Line { y: i64, dash: bool },
    X,
    Bus(u8),
}

fn end_kind(c: u8) -> End {
    match c {
        b'0' | b'l' | b'L' | b'p' | b'P' => End::Line { y: YS, dash: false },
        b'd' => End::Line { y: YS, dash: true },
        b'1' | b'h' | b'H' | b'n' | b'N' => End::Line { y: 0, dash: false },
        b'u' => End::Line { y: 0, dash: true },
        b'z' => End::Line {
            y: YS / 2,
            dash: false,
        },
        b'x' => End::X,
        b'=' | b'2' => End::Bus(2),
        b'3'..=b'9' => End::Bus(c - b'0'),
        _ => End::X,
    }
}

// A transition occupies x+3..x+9, with its crossing at x+6. Keeping the
// same crossing for every state makes bus labels and timing nodes line up.
fn band_band(paint: &mut Paint, x: i64, fill1: &str, fill2: &str) {
    flush_pending(paint);
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, 0);
    cmd_l(&mut paint.body, x + 3, 0);
    cmd_l(&mut paint.body, x + 6, YS / 2);
    cmd_l(&mut paint.body, x + 3, YS);
    cmd_l(&mut paint.body, x, YS);
    close_fill(&mut paint.body, fill1);
    open(&mut paint.body);
    cmd_m(&mut paint.body, x + XS, 0);
    cmd_l(&mut paint.body, x + 9, 0);
    cmd_l(&mut paint.body, x + 6, YS / 2);
    cmd_l(&mut paint.body, x + 9, YS);
    cmd_l(&mut paint.body, x + XS, YS);
    close_fill(&mut paint.body, fill2);
    stroke(paint, false, |body| {
        cmd_m(body, x, 0);
        cmd_l(body, x + 3, 0);
        cmd_l(body, x + 9, YS);
        cmd_l(body, x + XS, YS);
        cmd_m(body, x, YS);
        cmd_l(body, x + 3, YS);
        cmd_l(body, x + 9, 0);
        cmd_l(body, x + XS, 0);
    });
}

fn line_band(paint: &mut Paint, x: i64, y: i64, dash: bool, fill: &str) {
    flush_pending(paint);
    open(&mut paint.body);
    cmd_m(&mut paint.body, x + 3, y);
    cmd_l(&mut paint.body, x + 9, 0);
    cmd_l(&mut paint.body, x + XS, 0);
    cmd_l(&mut paint.body, x + XS, YS);
    cmd_l(&mut paint.body, x + 9, YS);
    paint.body.push(b'Z');
    close_fill(&mut paint.body, fill);
    hline(paint, x, 3, y, dash);
    stroke(paint, false, |body| {
        cmd_m(body, x + XS, 0);
        cmd_l(body, x + 9, 0);
        cmd_l(body, x + 3, y);
        cmd_l(body, x + 9, YS);
        cmd_h(body, x + XS);
    });
}

fn band_line(paint: &mut Paint, x: i64, fill: &str, y: i64, dash: bool) {
    flush_pending(paint);
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, 0);
    cmd_l(&mut paint.body, x + 3, 0);
    cmd_l(&mut paint.body, x + 9, y);
    cmd_l(&mut paint.body, x + 3, YS);
    cmd_l(&mut paint.body, x, YS);
    paint.body.push(b'Z');
    close_fill(&mut paint.body, fill);
    stroke(paint, false, |body| {
        cmd_m(body, x, 0);
        cmd_l(body, x + 3, 0);
        cmd_l(body, x + 9, y);
        cmd_l(body, x + 3, YS);
        cmd_h(body, x);
    });
    hline(paint, x + 9, XS - 9, y, dash);
}

fn slope(paint: &mut Paint, x: i64, y1: i64, y2: i64, dash: bool) {
    stroke(paint, dash, |body| {
        cmd_m(body, x, y1);
        cmd_l(body, x + 3, y1);
        cmd_l(body, x + 9, y2);
        cmd_h(body, x + XS);
    });
}

fn bump(paint: &mut Paint, x: i64, y: i64) {
    let mid = if y == 0 { 10 } else { y - 10 };
    stroke(paint, false, |body| {
        cmd_m(body, x, y);
        cmd_l(body, x + 3, y);
        cmd_l(body, x + 6, mid);
        cmd_l(body, x + 9, y);
        cmd_h(body, x + XS);
    });
}

fn hline(paint: &mut Paint, x: i64, w: i64, y: i64, dash: bool) {
    stroke(paint, dash, |body| {
        cmd_m(body, x, y);
        cmd_h(body, x + w);
    });
}

fn rails(paint: &mut Paint, x: i64, w: i64) {
    stroke(paint, false, |body| {
        cmd_m(body, x, 0);
        cmd_h(body, x + w);
        cmd_m(body, x, YS);
        cmd_h(body, x + w);
    });
}

fn arrow_at(body: &mut Vec<u8>, x: i64, rise: bool, ink: &str) {
    open(body);
    if rise {
        cmd_m(body, x - 3, 12);
        cmd_l(body, x, 3);
        cmd_l(body, x + 3, 12);
        body.extend_from_slice(b"Z");
    } else {
        cmd_m(body, x - 3, 8);
        cmd_l(body, x, 17);
        cmd_l(body, x + 3, 8);
        body.extend_from_slice(b"Z");
    }
    close_fill(body, ink);
}

pub(crate) fn write_clock_pattern(body: &mut Vec<u8>, id: usize, key: &ClockKey, ink: &str) {
    let half = (key.extra + 1) * XS;
    let full = half * 2;
    body.extend_from_slice(b"<pattern id=\"k");
    push_i64(body, id as i64);
    body.extend_from_slice(b"\" x=\"-4\" y=\"-2\" width=\"");
    push_i64(body, full);
    body.extend_from_slice(b"\" height=\"");
    push_i64(body, YS + 4);
    body.extend_from_slice(b"\" patternUnits=\"userSpaceOnUse\" fill=\"none\" stroke=\"");
    body.extend_from_slice(ink.as_bytes());
    body.extend_from_slice(
        b"\" stroke-width=\"1\" stroke-linecap=\"round\" stroke-linejoin=\"round\" viewBox=\"-4 -2 ",
    );
    push_i64(body, full);
    body.push(b' ');
    push_i64(body, YS + 4);
    body.extend_from_slice(b"\">");
    // All stroke and arrow geometry is inside the tile; overflow=visible is
    // deliberately unnecessary, since SVG renderers clip pattern paint.
    open(body);
    edge_d(body, key.s3, -4, 0);
    edge_d(body, key.s0, 0, half);
    edge_d(body, key.s2, half, full - 4);
    close_stroke(body, false);
    if matches!(key.s0, Code::RiseA) {
        arrow_at(body, 0, true, ink);
    }
    if matches!(key.s0, Code::FallA) {
        arrow_at(body, 0, false, ink);
    }
    if matches!(key.s2, Code::RiseA) {
        arrow_at(body, half, true, ink);
    }
    if matches!(key.s2, Code::FallA) {
        arrow_at(body, half, false, ink);
    }
    body.extend_from_slice(b"</pattern>");
}

fn clock_edge(paint: &mut Paint, code: Code, x0: i64, x1: i64) {
    let dash = matches!(code, Code::DashH | Code::DashL);
    stroke(paint, dash, |body| edge_d(body, code, x0, x1));
    if matches!(code, Code::RiseA | Code::FallA) {
        flush_pending(paint);
        arrow_at(
            &mut paint.body,
            x0,
            matches!(code, Code::RiseA),
            paint.scheme.ink,
        );
    }
}

fn edge_d(body: &mut Vec<u8>, code: Code, x0: i64, x1: i64) {
    match code {
        Code::Rise | Code::RiseA => {
            cmd_m(body, x0, YS);
            cmd_v(body, 0);
            cmd_h(body, x1);
        }
        Code::Fall | Code::FallA => {
            cmd_m(body, x0, 0);
            cmd_v(body, YS);
            cmd_h(body, x1);
        }
        Code::Low | Code::DashL => {
            cmd_m(body, x0, YS);
            cmd_h(body, x1);
        }
        Code::Mid => {
            cmd_m(body, x0, YS / 2);
            cmd_h(body, x1);
        }
        _ => {
            cmd_m(body, x0, 0);
            cmd_h(body, x1);
        }
    }
}

fn rect(paint: &mut Paint, x: i64, y: i64, w: i64, h: i64, fill: &str) {
    flush_pending(paint);
    let body = &mut paint.body;
    body.extend_from_slice(b"<rect x=\"");
    push_i64(body, x);
    body.extend_from_slice(b"\" y=\"");
    push_i64(body, y);
    body.extend_from_slice(b"\" width=\"");
    push_i64(body, w);
    body.extend_from_slice(b"\" height=\"");
    push_i64(body, h);
    body.extend_from_slice(b"\" fill=\"");
    body.extend_from_slice(fill.as_bytes());
    body.extend_from_slice(b"\" stroke=\"none\"/>");
}

fn open(body: &mut Vec<u8>) {
    body.extend_from_slice(b"<path d=\"");
}

fn close_stroke(body: &mut Vec<u8>, dash: bool) {
    body.push(b'"');
    if dash {
        body.extend_from_slice(b" stroke-dasharray=\"1,3\"");
    }
    body.extend_from_slice(b"/>");
}

fn close_fill(body: &mut Vec<u8>, fill: &str) {
    body.extend_from_slice(b"\" fill=\"");
    body.extend_from_slice(fill.as_bytes());
    body.extend_from_slice(b"\" stroke=\"none\"/>");
}

fn cmd_m(body: &mut Vec<u8>, x: i64, y: i64) {
    body.push(b'M');
    push_i64(body, x);
    body.push(b' ');
    push_i64(body, y);
}

fn cmd_l(body: &mut Vec<u8>, x: i64, y: i64) {
    body.push(b'L');
    push_i64(body, x);
    body.push(b' ');
    push_i64(body, y);
}

fn cmd_h(body: &mut Vec<u8>, x: i64) {
    body.push(b'H');
    push_i64(body, x);
}

fn cmd_v(body: &mut Vec<u8>, y: i64) {
    body.push(b'V');
    push_i64(body, y);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(paint: &Paint) -> &str {
        std::str::from_utf8(&paint.body).unwrap()
    }

    #[test]
    fn unknown_bus_transitions_keep_both_bands() {
        for (prev, next) in [(b'x', b'3'), (b'3', b'x')] {
            let mut paint = Paint::new();
            draw_soft(&mut paint, prev, next, 40);
            flush_pending(&mut paint);
            let svg = body(&paint);
            assert!(
                paint.hatch,
                "unknown band must request the hatch definition"
            );
            assert!(svg.contains("fill=\"url(#xh)\""));
            assert!(svg.contains("fill=\"#ffffb4\""));
            // Both rails cross between the same endpoints as a bus-to-bus
            // transition; an unknown state must not collapse into high-Z.
            assert!(svg.contains("M40 0L43 0L49 20L60 20M40 20L43 20L49 0L60 0"));
        }
    }

    #[test]
    fn bus_line_transitions_taper_to_the_actual_level() {
        for (code, y) in [(b'0', YS), (b'1', 0), (b'z', YS / 2)] {
            let mut paint = Paint::new();
            draw_soft(&mut paint, b'3', code, 0);
            flush_pending(&mut paint);
            assert!(body(&paint).contains(&format!("M0 0L3 0L9 {y}L3 20H0")));
            assert!(body(&paint).contains(&format!("M9 {y}H20")));

            let mut paint = Paint::new();
            draw_soft(&mut paint, code, b'3', 0);
            flush_pending(&mut paint);
            assert!(body(&paint).contains(&format!("M20 0L9 0L3 {y}L9 20H20")));
            assert!(body(&paint).contains(&format!("M0 {y}H3")));
        }
    }

    #[test]
    fn clocks_stay_compact_and_phase_origins_stay_small() {
        let clock = |paint: &mut Paint, times, skip| {
            draw_clock(
                paint,
                0,
                Code::RiseA,
                Code::High,
                Code::Fall,
                Code::Low,
                0,
                times,
                skip,
                i64::MAX,
            );
        };
        let mut short = Paint::new();
        clock(&mut short, 4, 0);
        let mut long = Paint::new();
        clock(&mut long, 1_000_000, 0);
        assert!(long.body.len() < short.body.len() + 40);
        assert_eq!(long.clocks.len(), 1);

        for skip in 0..8 {
            let mut paint = Paint::new();
            clock(&mut paint, 4, skip);
            assert!(!body(&paint).contains("width=\"-"));
            assert!(!body(&paint).contains("height=\"-"));
        }
        let mut cropped = Paint::new();
        clock(&mut cropped, 1_000_000, 1_999_997);
        assert!(body(&cropped).contains("translate(-20)"));
        assert!(!body(&cropped).contains("1999997"));

        let mut hidden = Paint::new();
        clock(&mut hidden, 4, 8);
        assert!(hidden.body.is_empty());
        assert!(hidden.clocks.is_empty());
    }

    #[test]
    fn clock_tiles_keep_arrows_and_horizontal_strokes_inside_the_tile() {
        let mut svg = Vec::new();
        write_clock_pattern(
            &mut svg,
            0,
            &ClockKey {
                s0: Code::RiseA,
                s1: Code::High,
                s2: Code::Fall,
                s3: Code::Low,
                extra: 0,
            },
            crate::scheme::LIGHT.ink,
        );
        let svg = std::str::from_utf8(&svg).unwrap();
        assert!(svg.contains("viewBox=\"-4 -2 40 24\""));
        assert!(svg.contains("M-3 12L0 3L3 12Z"));
        assert!(svg.contains("M-4 20H0M0 20V0H20M20 0V20H36"));
        assert!(!svg.contains("overflow="));
    }

    #[test]
    fn toggling_levels_share_one_stroke_path() {
        let wave = crate::wave::compile("010101", 1.0, 1, 0.0).unwrap();
        let mut paint = Paint::new();
        paint_wave(&mut paint, &wave.pats, 0, i64::MAX);
        let svg = body(&paint);
        assert_eq!(svg.matches("<path ").count(), 1, "{svg}");
        assert!(svg.contains("M0 20H40M40 20L43 20L49 0H60"), "{svg}");
        assert!(svg.contains("M80 0L83 0L89 20H100"), "{svg}");
    }
}
