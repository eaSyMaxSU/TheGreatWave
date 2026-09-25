//! Draw runs as patterns, rectangles, and short transition cells.

use crate::w::push_i64;
use crate::wave::{Code, Pat};
use crate::{XS, YS};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockKey {
    pub s0: Code,
    pub s1: Code,
    pub s2: Code,
    pub s3: Code,
    pub extra: i64,
}

pub(crate) struct Clip {
    pub x: i64,
    pub w: i64,
}

pub(crate) struct Paint {
    pub body: Vec<u8>,
    pub clocks: Vec<ClockKey>,
    pub clips: Vec<Clip>,
    pub hatch: bool,
}

impl Paint {
    pub(crate) fn new() -> Self {
        Self {
            body: Vec::new(),
            clocks: Vec::new(),
            clips: Vec::new(),
            hatch: false,
        }
    }
}

pub(crate) fn paint_wave(paint: &mut Paint, pats: &[Pat], skip: i64) {
    for p in pats {
        match *p {
            Pat::Fill { at, code, count } => {
                let lo = at.max(skip);
                let hi = at + count;
                if lo >= hi {
                    continue;
                }
                draw_span(paint, code, (lo - skip) * XS, (hi - lo) * XS);
            }
            Pat::Lead { at, lead, hold, holds } => {
                if at >= skip {
                    draw_one(paint, lead, (at - skip) * XS);
                }
                if holds > 0 {
                    let start = at + 1;
                    let lo = start.max(skip);
                    let hi = start + holds;
                    if lo < hi {
                        draw_span(paint, hold, (lo - skip) * XS, (hi - lo) * XS);
                    }
                }
            }
            Pat::Clock { at, s0, s1, s2, s3, extra, times } => {
                draw_clock(paint, at, s0, s1, s2, s3, extra, times, skip);
            }
        }
    }
}

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
) {
    let period = 2 * (extra + 1);
    let total = times * period;
    let hi = at + total;
    let lo = at.max(skip);
    if lo >= hi || total <= 0 {
        return;
    }
    let key = ClockKey { s0, s1, s2, s3, extra };
    let id = if let Some(i) = paint.clocks.iter().position(|c| *c == key) {
        i
    } else {
        paint.clocks.push(key);
        paint.clocks.len() - 1
    };
    let x_start = (at - skip) * XS;
    let total_w = total * XS;
    let clipped = lo > at;
    if clipped {
        let vis_x = (lo - skip) * XS;
        let vis_w = (hi - lo) * XS;
        paint.clips.push(Clip { x: vis_x, w: vis_w });
        let cid = paint.clips.len() - 1;
        paint.body.extend_from_slice(b"<g clip-path=\"url(#c");
        push_i64(&mut paint.body, cid as i64);
        paint.body.extend_from_slice(b")\">");
    }
    paint.body.extend_from_slice(b"<g transform=\"translate(");
    push_i64(&mut paint.body, x_start);
    paint.body.extend_from_slice(b")\"><rect width=\"");
    push_i64(&mut paint.body, total_w);
    paint.body.extend_from_slice(b"\" height=\"");
    push_i64(&mut paint.body, YS);
    paint.body.extend_from_slice(b"\" fill=\"url(#k");
    push_i64(&mut paint.body, id as i64);
    paint.body.extend_from_slice(b")\"/></g>");
    if clipped {
        paint.body.extend_from_slice(b"</g>");
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
            rect(&mut paint.body, x, 0, w, YS, "url(#xh)", false);
            hline(paint, x, w, 0, false);
            hline(paint, x, w, YS, false);
        }
        Code::Bus(n) => {
            rect(&mut paint.body, x, 0, w, YS, bus_fill(n), false);
            hline(paint, x, w, 0, false);
            hline(paint, x, w, YS, false);
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
    open(&mut paint.body);
    if rise {
        cmd_m(&mut paint.body, x, YS);
        cmd_v(&mut paint.body, 0);
        cmd_h(&mut paint.body, x + XS);
    } else {
        cmd_m(&mut paint.body, x, 0);
        cmd_v(&mut paint.body, YS);
        cmd_h(&mut paint.body, x + XS);
    }
    close_stroke(&mut paint.body, false);
    if arrow {
        arrow_at(&mut paint.body, x, rise);
    }
}

fn draw_soft(paint: &mut Paint, prev: u8, next: u8, x: i64) {
    let a = end_kind(prev);
    let b = end_kind(next);
    match (a, b) {
        (End::Bus(c1), End::Bus(c2)) => bus_bus(paint, x, c1, c2),
        (End::Line { y, .. }, End::Bus(c)) => line_bus(paint, x, y, c),
        (End::Bus(c), End::Line { y, .. }) => bus_line(paint, x, c, y),
        (End::X, End::Bus(c)) => line_bus(paint, x, YS / 2, c),
        (End::Bus(c), End::X) => bus_line(paint, x, c, YS / 2),
        (End::X, End::X) => draw_span(paint, Code::X, x, XS),
        (End::X, End::Line { y, .. }) => {
            draw_span(paint, Code::X, x, XS / 2);
            slope(paint, x, YS / 2, y, false);
        }
        (End::Line { y, dash }, End::X) => {
            slope(paint, x, y, YS / 2, dash);
            draw_span(paint, Code::X, x + XS / 2, XS / 2);
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
        b'z' => End::Line { y: YS / 2, dash: false },
        b'x' => End::X,
        b'=' | b'2' => End::Bus(2),
        b'3'..=b'9' => End::Bus(c - b'0'),
        _ => End::X,
    }
}

fn bus_bus(paint: &mut Paint, x: i64, c1: u8, c2: u8) {
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, 0);
    cmd_l(&mut paint.body, x + 3, 0);
    cmd_l(&mut paint.body, x + 6, 10);
    cmd_l(&mut paint.body, x + 3, YS);
    cmd_l(&mut paint.body, x, YS);
    close_fill(&mut paint.body, bus_fill(c1));
    open(&mut paint.body);
    cmd_m(&mut paint.body, x + XS, 0);
    cmd_l(&mut paint.body, x + 9, 0);
    cmd_l(&mut paint.body, x + 6, 10);
    cmd_l(&mut paint.body, x + 9, YS);
    cmd_l(&mut paint.body, x + XS, YS);
    close_fill(&mut paint.body, bus_fill(c2));
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, 0);
    cmd_l(&mut paint.body, x + 3, 0);
    cmd_l(&mut paint.body, x + 9, YS);
    cmd_l(&mut paint.body, x + XS, YS);
    cmd_m(&mut paint.body, x, YS);
    cmd_l(&mut paint.body, x + 3, YS);
    cmd_l(&mut paint.body, x + 9, 0);
    cmd_l(&mut paint.body, x + XS, 0);
    close_stroke(&mut paint.body, false);
}

fn line_bus(paint: &mut Paint, x: i64, y: i64, c: u8) {
    open(&mut paint.body);
    cmd_m(&mut paint.body, x + 9, 0);
    cmd_l(&mut paint.body, x + XS, 0);
    cmd_l(&mut paint.body, x + XS, YS);
    cmd_l(&mut paint.body, x + 3, YS);
    cmd_l(&mut paint.body, x + 9, 0);
    close_fill(&mut paint.body, bus_fill(c));
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, y);
    cmd_l(&mut paint.body, x + 3, y);
    cmd_l(&mut paint.body, x + 9, if y > 10 { 0 } else { YS });
    cmd_h(&mut paint.body, x + XS);
    close_stroke(&mut paint.body, false);
    hline(paint, x, XS, 0, false);
    hline(paint, x, XS, YS, false);
}

fn bus_line(paint: &mut Paint, x: i64, c: u8, y: i64) {
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, 0);
    cmd_l(&mut paint.body, x + 3, 0);
    cmd_l(&mut paint.body, x + 9, YS);
    cmd_l(&mut paint.body, x, YS);
    close_fill(&mut paint.body, bus_fill(c));
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, 0);
    cmd_l(&mut paint.body, x + 3, 0);
    cmd_l(&mut paint.body, x + 9, y);
    cmd_h(&mut paint.body, x + XS);
    close_stroke(&mut paint.body, false);
    hline(paint, x, XS, y, false);
}

fn slope(paint: &mut Paint, x: i64, y1: i64, y2: i64, dash: bool) {
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, y1);
    cmd_l(&mut paint.body, x + 3, y1);
    cmd_l(&mut paint.body, x + 9, y2);
    cmd_h(&mut paint.body, x + XS);
    close_stroke(&mut paint.body, dash);
}

fn bump(paint: &mut Paint, x: i64, y: i64) {
    let mid = if y == 0 { 10 } else { y - 10 };
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, y);
    cmd_l(&mut paint.body, x + 3, y);
    cmd_l(&mut paint.body, x + 6, mid);
    cmd_l(&mut paint.body, x + 9, y);
    cmd_h(&mut paint.body, x + XS);
    close_stroke(&mut paint.body, false);
}

fn hline(paint: &mut Paint, x: i64, w: i64, y: i64, dash: bool) {
    open(&mut paint.body);
    cmd_m(&mut paint.body, x, y);
    cmd_h(&mut paint.body, x + w);
    close_stroke(&mut paint.body, dash);
}

fn arrow_at(body: &mut Vec<u8>, x: i64, rise: bool) {
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
    close_fill(body, "#000");
}

pub(crate) fn bus_fill(n: u8) -> &'static str {
    match n {
        3 => "#ffffb4",
        4 => "#ffe0b9",
        5 => "#b9e0ff",
        6 => "#ccfdfe",
        7 => "#cdfdc5",
        8 => "#f0c1fb",
        9 => "#f5c2c0",
        _ => "#ffffff",
    }
}

pub(crate) fn write_clock_pattern(body: &mut Vec<u8>, id: usize, key: &ClockKey) {
    let half = (key.extra + 1) * XS;
    let full = half * 2;
    body.extend_from_slice(b"<pattern id=\"k");
    push_i64(body, id as i64);
    body.extend_from_slice(b"\" width=\"");
    push_i64(body, full);
    body.extend_from_slice(b"\" height=\"");
    push_i64(body, YS);
    body.extend_from_slice(b"\" patternUnits=\"userSpaceOnUse\">");
    open(body);
    edge_d(body, key.s0, 0, half);
    edge_d(body, key.s2, half, full);
    close_stroke(body, false);
    if matches!(key.s0, Code::RiseA) {
        arrow_at(body, 0, true);
    }
    if matches!(key.s0, Code::FallA) {
        arrow_at(body, 0, false);
    }
    if matches!(key.s2, Code::RiseA) {
        arrow_at(body, half, true);
    }
    if matches!(key.s2, Code::FallA) {
        arrow_at(body, half, false);
    }
    body.extend_from_slice(b"</pattern>");
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

fn rect(body: &mut Vec<u8>, x: i64, y: i64, w: i64, h: i64, fill: &str, stroke: bool) {
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
    if stroke {
        body.extend_from_slice(b"\" stroke=\"#000\" stroke-width=\"1\"/>");
    } else {
        body.extend_from_slice(b"\" stroke=\"none\"/>");
    }
}

fn open(body: &mut Vec<u8>) {
    body.extend_from_slice(b"<path d=\"");
}

fn close_stroke(body: &mut Vec<u8>, dash: bool) {
    body.extend_from_slice(
        b"\" fill=\"none\" stroke=\"#000\" stroke-width=\"1\" stroke-linecap=\"round\"",
    );
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

