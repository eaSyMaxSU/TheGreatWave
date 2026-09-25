//! SVG document writer. Coordinates are appended as integers or short decimals.

use crate::geom::{self, Paint};
use crate::scan::{Body, Cap, Doc, Tick};
use crate::w::{prettify, push_esc, push_f64, push_i64};
use crate::wave::{self, WaveOut};
use crate::width::text_width;
use crate::{Error, TGO, XLABEL, XS, Y0, YM, YO, YS};

struct Ev {
    ch: char,
    x: f64,
    y: f64,
}

pub(crate) fn write(doc: &Doc, out: &mut Vec<u8>, indent: u8) -> Result<(), Error> {
    let n = doc.lanes.len() as i64;
    let mut waves: Vec<Option<WaveOut>> = Vec::with_capacity(doc.lanes.len());
    let mut xmax = 0i64;
    for lane in &doc.lanes {
        let wave = match &lane.body {
            Body::Wave(s) => {
                let phase = if lane.phase != 0.0 { lane.phase * 2.0 } else { 0.0 };
                Some(wave::compile(s, lane.period, doc.hscale, phase + doc.xmin as f64)?)
            }
            _ => None,
        };
        if let Some(w) = &wave {
            xmax = xmax.max(wave::visible_bricks(w));
        }
        waves.push(wave);
    }
    let cap = doc.xmax_cfg - doc.xmin;
    if cap < xmax {
        xmax = cap.max(0);
    }
    let mut max_label = 0.0f64;
    for lane in &doc.lanes {
        max_label = max_label.max(text_width(&lane.name, 11.0) + lane.indent as f64);
    }
    let xg = ((max_label - TGO as f64) / XS as f64).ceil() * XS as f64;
    let (yh0, yh1) = margins(&doc.head);
    let (yf0, yf1) = margins(&doc.foot);
    let gy = n * YO;
    let width = (xg + XS as f64 * (xmax + 1) as f64).max(1.0);
    let height = (gy + yh0 + yh1 + yf0 + yf1).max(1);

    let mut paint = Paint::new();
    let mut events = Vec::new();
    let grid = if doc.marks && gy > 0 {
        grid_geom(doc.hscale, xmax, gy)
    } else {
        None
    };
    for (idx, lane) in doc.lanes.iter().enumerate() {
        let y = Y0 + idx as i64 * YO;
        paint.body.extend_from_slice(b"<g transform=\"translate(0,");
        push_i64(&mut paint.body, y);
        paint.body.extend_from_slice(b")\">");
        paint.body.extend_from_slice(
            b"<text x=\"",
        );
        push_i64(&mut paint.body, TGO);
        paint.body.extend_from_slice(
            b"\" y=\"",
        );
        push_i64(&mut paint.body, YM);
        paint.body.extend_from_slice(
            b"\" text-anchor=\"end\" class=\"info\" xml:space=\"preserve\">",
        );
        push_esc(&mut paint.body, &lane.name);
        paint.body.extend_from_slice(b"</text>");

        let dx = frac_dx(lane.phase, doc.xmin);
        let shifted = dx.abs() > 0.001;
        if shifted {
            paint.body.extend_from_slice(b"<g transform=\"translate(");
            push_f64(&mut paint.body, dx);
            paint.body.extend_from_slice(b")\">");
        }
        if let Some(w) = &waves[idx] {
            geom::paint_wave(&mut paint, &w.pats, w.skip);
            let from = w.unseen.min(lane.data.len());
            for (i, mx) in w.markers.iter().enumerate() {
                if let Some(label) = lane.data.get(from + i) {
                    paint.body.extend_from_slice(
                        b"<text x=\"",
                    );
                    push_f64(&mut paint.body, *mx);
                    paint.body.extend_from_slice(b"\" y=\"");
                    push_i64(&mut paint.body, YM);
                    paint.body.extend_from_slice(
                        b"\" text-anchor=\"middle\" xml:space=\"preserve\">",
                    );
                    push_esc(&mut paint.body, label);
                    paint.body.extend_from_slice(b"</text>");
                }
            }
        }
        if let Body::Path(d) = &lane.body {
            paint.body.extend_from_slice(b"<g transform=\"translate(0,");
            push_i64(&mut paint.body, YS);
            paint.body.extend_from_slice(b")\"><path fill=\"none\" stroke=\"#000\" stroke-width=\"1\" d=\"");
            push_esc(&mut paint.body, &scale_path(d));
            paint.body.extend_from_slice(b"\"/></g>");
        }
        if shifted {
            paint.body.extend_from_slice(b"</g>");
        }
        if let Some(s) = &lane.over {
            draw_ou(&mut paint.body, s, false, lane.period, lane.phase);
        }
        if let Some(s) = &lane.under {
            draw_ou(&mut paint.body, s, true, lane.period, lane.phase);
        }
        if let Some(w) = &waves[idx] {
            for gx in &w.gaps {
                draw_gap(&mut paint.body, *gx);
            }
        }
        paint.body.extend_from_slice(b"</g>");

        if let Some(node) = &lane.node {
            let phase = if lane.phase != 0.0 { lane.phase * 2.0 } else { 0.0 } + doc.xmin as f64;
            let mut pos = 0.0;
            for ch in node.chars() {
                if ch != '.' {
                    let x = XS as f64 * (2.0 * pos * lane.period * doc.hscale as f64 - phase) + XLABEL as f64;
                    let y = idx as f64 * YO as f64 + Y0 as f64 + YS as f64 * 0.5;
                    events.push(Ev { ch, x, y });
                }
                pos += 1.0;
            }
        }
    }

    write_arcs(&mut paint.body, doc, &events);
    if let Some(g) = &doc.gaps {
        draw_gap_string(&mut paint.body, g, n, doc.hscale);
    }
    write_caption(&mut paint.body, &doc.head, true, xmax, gy, yh0, doc.hscale);
    write_caption(&mut paint.body, &doc.foot, false, xmax, gy, yf0, doc.hscale);

    out.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"tgw\" width=\"");
    push_f64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" viewBox=\"0 0 ");
    push_f64(out, width);
    out.push(b' ');
    push_i64(out, height);
    out.extend_from_slice(
        b"\" overflow=\"hidden\"><style>text{font-family:Helvetica,sans-serif;font-size:11px;fill:#000}.info{fill:#0041c4}.muted{fill:#aaa}</style><defs>",
    );
    write_markers(out);
    if paint.hatch {
        out.extend_from_slice(
            b"<pattern id=\"xh\" width=\"6\" height=\"6\" patternUnits=\"userSpaceOnUse\"><path d=\"M0,6 L6,0\" fill=\"none\" stroke=\"#000\" stroke-width=\"0.5\"/></pattern>",
        );
    }
    for (i, key) in paint.clocks.iter().enumerate() {
        geom::write_clock_pattern(out, i, key);
    }
    for (i, clip) in paint.clips.iter().enumerate() {
        out.extend_from_slice(b"<clipPath id=\"c");
        push_i64(out, i as i64);
        out.extend_from_slice(b"\"><rect x=\"");
        push_i64(out, clip.x);
        out.extend_from_slice(b"\" y=\"0\" width=\"");
        push_i64(out, clip.w);
        out.extend_from_slice(b"\" height=\"");
        push_i64(out, YS);
        out.extend_from_slice(b"\"/></clipPath>");
    }
    if let Some(g) = &grid {
        out.extend_from_slice(b"<pattern id=\"gd\" width=\"");
        push_i64(out, g.step);
        out.extend_from_slice(b"\" height=\"");
        push_i64(out, g.height);
        out.extend_from_slice(
            b"\" patternUnits=\"userSpaceOnUse\"><path d=\"M0,0 V",
        );
        push_i64(out, g.height);
        out.extend_from_slice(
            b"\" fill=\"none\" stroke=\"#888\" stroke-width=\"0.5\" stroke-dasharray=\"1,3\"/></pattern>",
        );
    }
    out.extend_from_slice(b"</defs><rect width=\"");
    push_f64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" fill=\"#fff\"/><g transform=\"translate(");
    push_f64(out, xg + 0.5);
    out.push(b',');
    push_f64(out, (yh0 + yh1) as f64 + 0.5);
    out.extend_from_slice(b")\">");
    if let Some(g) = &grid {
        out.extend_from_slice(b"<rect x=\"0\" y=\"0\" width=\"");
        push_i64(out, g.width);
        out.extend_from_slice(b"\" height=\"");
        push_i64(out, g.height);
        out.extend_from_slice(b"\" fill=\"url(#gd)\" stroke=\"none\"/>");
    }
    out.extend_from_slice(&paint.body);
    out.extend_from_slice(b"</g>");
    write_groups(out, doc, yh0 + yh1);
    out.extend_from_slice(b"</svg>");
    if indent > 0 {
        let pretty = prettify(out, indent);
        out.clear();
        out.extend_from_slice(&pretty);
    }
    Ok(())
}

fn margins(cap: &Cap) -> (i64, i64) {
    let tick = !matches!(cap.tick, Tick::Off) || !matches!(cap.tock, Tick::Off);
    let y0 = if tick { 20 } else { 0 };
    let y1 = if cap.text.is_some() { 46 } else { 0 };
    (y0, y1)
}

fn frac_dx(phase: f64, xmin: i64) -> f64 {
    let mut xoff = if phase != 0.0 { phase } else { 0.0 } + xmin as f64 / 2.0;
    if xoff > 0.0 {
        xoff = (2.0 * xoff).ceil() - 2.0 * xoff;
    } else {
        xoff = -2.0 * xoff;
    }
    xoff * XS as f64
}

struct Grid {
    step: i64,
    width: i64,
    height: i64,
}

fn grid_geom(hscale: i32, xmax: i64, gy: i64) -> Option<Grid> {
    let step = 2 * hscale as i64 * XS;
    if step <= 0 || gy <= 0 {
        return None;
    }
    let marks = xmax as f64 / (2.0 * hscale as f64);
    let n = count_lt(marks + 1.0);
    if n <= 0 {
        return None;
    }
    Some(Grid {
        step,
        width: (n - 1) * step + 1,
        height: gy,
    })
}

fn count_lt(len: f64) -> i64 {
    if !(len > 0.0) || !len.is_finite() {
        return 0;
    }
    if len.fract() == 0.0 {
        len as i64
    } else {
        len.floor() as i64 + 1
    }
}

fn write_markers(body: &mut Vec<u8>) {
    body.extend_from_slice(
        b"<marker id=\"arrowhead\" viewBox=\"0 -4 11 8\" refX=\"11\" refY=\"0\" markerWidth=\"8\" markerHeight=\"6\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,-4 L11,0 L0,4 Z\" fill=\"#0041c4\"/></marker><marker id=\"arrowtail\" viewBox=\"-11 -4 11 8\" refX=\"-11\" refY=\"0\" markerWidth=\"8\" markerHeight=\"6\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,-4 L-11,0 L0,4 Z\" fill=\"#0041c4\"/></marker><marker id=\"tee\" viewBox=\"0 0 2 6\" refX=\"1\" refY=\"3\" markerWidth=\"2\" markerHeight=\"6\" orient=\"auto\"><path d=\"M1,0 L1,6\" stroke=\"#0041c4\" stroke-width=\"2\"/></marker>",
    );
}

fn draw_gap(body: &mut Vec<u8>, x: f64) {
    body.extend_from_slice(b"<g transform=\"translate(");
    push_f64(body, x);
    body.extend_from_slice(
        b")\"><path d=\"M-7,22 C-2,22 -2,-2 3,-2\" fill=\"none\" stroke=\"#000\" stroke-width=\"1\"/><path d=\"M-3,22 C2,22 2,-2 7,-2\" fill=\"none\" stroke=\"#000\" stroke-width=\"1\"/></g>",
    );
}

fn draw_ou(body: &mut Vec<u8>, text: &str, under: bool, period: f64, phase: f64) {
    if text.is_empty() {
        return;
    }
    let y = if under { YS } else { 0 };
    let step = period * 2.0 * XS as f64;
    let phase = if phase != 0.0 { phase } else { 0.0 };
    let xoff = -phase * 2.0 * XS as f64;
    body.extend_from_slice(b"<g transform=\"translate(");
    push_f64(body, xoff);
    body.push(b',');
    push_i64(body, y);
    body.extend_from_slice(b")\" fill=\"none\" stroke-width=\"3\">");
    let bytes = text.as_bytes();
    let mut start: Option<usize> = None;
    let mut color: &str = "#000000";
    let gap1 = 12.0;
    let serif = 7.0;
    for (i, &dot) in bytes.iter().enumerate() {
        if dot != b'.' {
            if let Some(s) = start {
                let x1 = step * s as f64 + gap1;
                let x2 = step * i as f64;
                body.extend_from_slice(b"<line stroke=\"");
                body.extend_from_slice(color.as_bytes());
                body.extend_from_slice(b"\" x1=\"");
                push_f64(body, x1);
                body.extend_from_slice(b"\" x2=\"");
                push_f64(body, x2);
                body.extend_from_slice(b"\" y1=\"0\" y2=\"0\"/>");
                if !under {
                    body.extend_from_slice(b"<path fill=\"");
                    body.extend_from_slice(color.as_bytes());
                    body.extend_from_slice(b"\" stroke=\"none\" d=\"M");
                    push_f64(body, step * i as f64 - serif);
                    body.extend_from_slice(b",0 l");
                    push_f64(body, serif);
                    body.push(b',');
                    push_f64(body, serif);
                    body.extend_from_slice(b" v-");
                    push_f64(body, serif);
                    body.extend_from_slice(b" z\"/>");
                }
            }
        }
        if dot == b'0' {
            start = None;
        } else if dot != b'.' {
            start = Some(i);
            color = ou_color(dot);
        }
    }
    if let Some(s) = start {
        let x1 = step * s as f64 + gap1;
        let x2 = step * bytes.len() as f64;
        body.extend_from_slice(b"<line stroke=\"");
        body.extend_from_slice(color.as_bytes());
        body.extend_from_slice(b"\" x1=\"");
        push_f64(body, x1);
        body.extend_from_slice(b"\" x2=\"");
        push_f64(body, x2);
        body.extend_from_slice(b"\" y1=\"0\" y2=\"0\"/>");
    }
    body.extend_from_slice(b"</g>");
}

fn ou_color(c: u8) -> &'static str {
    match c {
        b'2' => "#e90000",
        b'3' => "#3edd00",
        b'4' => "#0074cd",
        b'5' => "#ff15db",
        b'6' => "#af9800",
        b'7' => "#00864f",
        b'8' => "#a076ff",
        _ => "#000000",
    }
}

fn write_arcs(body: &mut Vec<u8>, doc: &Doc, events: &[Ev]) {
    for ev in events {
        if !ev.ch.is_uppercase() && ev.x > 0.0 {
            label(body, ev.x, ev.y, &ev.ch.to_string(), doc.arc_font);
        }
    }
    for edge in &doc.edges {
        let Some((from, to, shape, text)) = split_edge(edge) else {
            continue;
        };
        let Some(a) = events.iter().find(|e| e.ch == from) else {
            continue;
        };
        let Some(b) = events.iter().find(|e| e.ch == to) else {
            continue;
        };
        let (d, style, lx, ly) = arc_shape(&shape, a.x, a.y, b.x, b.y, !text.is_empty());
        body.extend_from_slice(b"<path d=\"");
        body.extend_from_slice(d.as_bytes());
        body.extend_from_slice(b"\" style=\"");
        body.extend_from_slice(style.as_bytes());
        body.extend_from_slice(b"\"/>");
        if !text.is_empty() {
            label(body, lx, ly, &text, doc.arc_font);
        }
    }
}

fn split_edge(s: &str) -> Option<(char, char, String, String)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let head = s.split_whitespace().next()?;
    let chars: Vec<char> = head.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let from = chars[0];
    let to = *chars.last()?;
    let shape: String = if chars.len() >= 2 {
        chars[1..chars.len() - 1].iter().collect()
    } else {
        String::new()
    };
    let label = if s.len() > head.len() + 1 {
        s[head.len() + 1..].to_string()
    } else {
        String::new()
    };
    Some((from, to, shape, label))
}

fn arc_shape(shape: &str, x1: f64, y1: f64, x2: f64, y2: f64, labeled: bool) -> (String, String, f64, f64) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let mut lx = (x1 + x2) / 2.0;
    let ly = (y1 + y2) / 2.0;
    let straight = line(x1, y1, x2, y2);
    let blue = "fill:none;stroke:#00F;stroke-width:1";
    let arrow = "marker-end:url(#arrowhead);stroke:#0041c4;stroke-width:1;fill:none";
    let both = "marker-end:url(#arrowhead);marker-start:url(#arrowtail);stroke:#0041c4;stroke-width:1;fill:none";
    let (d, style) = match shape {
        "" | "-" => (straight, blue.to_string()),
        "~" => (curve(x1, y1, 0.7 * dx, 0.0, 0.3 * dx, dy, dx, dy), blue.to_string()),
        "-~" => {
            if labeled {
                lx = x1 + dx * 0.75;
            }
            (curve(x1, y1, 0.7 * dx, 0.0, dx, dy, dx, dy), blue.to_string())
        }
        "~-" => {
            if labeled {
                lx = x1 + dx * 0.25;
            }
            (curve(x1, y1, 0.0, 0.0, 0.3 * dx, dy, dx, dy), blue.to_string())
        }
        "-|" => {
            if labeled {
                lx = x2;
            }
            (ortho(x1, y1, dx, 0.0, 0.0, dy), blue.to_string())
        }
        "|-" => {
            if labeled {
                lx = x1;
            }
            (ortho(x1, y1, 0.0, dy, dx, 0.0), blue.to_string())
        }
        "-|-" => (elbow(x1, y1, dx, dy), blue.to_string()),
        "->" => (straight, arrow.to_string()),
        "~>" => (curve(x1, y1, 0.7 * dx, 0.0, 0.3 * dx, dy, dx, dy), arrow.to_string()),
        "-~>" => {
            if labeled {
                lx = x1 + dx * 0.75;
            }
            (curve(x1, y1, 0.7 * dx, 0.0, dx, dy, dx, dy), arrow.to_string())
        }
        "~->" => {
            if labeled {
                lx = x1 + dx * 0.25;
            }
            (curve(x1, y1, 0.0, 0.0, 0.3 * dx, dy, dx, dy), arrow.to_string())
        }
        "-|>" => {
            if labeled {
                lx = x2;
            }
            (ortho(x1, y1, dx, 0.0, 0.0, dy), arrow.to_string())
        }
        "|->" => {
            if labeled {
                lx = x1;
            }
            (ortho(x1, y1, 0.0, dy, dx, 0.0), arrow.to_string())
        }
        "-|->" => (elbow(x1, y1, dx, dy), arrow.to_string()),
        "<->" => (straight, both.to_string()),
        "<~>" => (curve(x1, y1, 0.7 * dx, 0.0, 0.3 * dx, dy, dx, dy), both.to_string()),
        "<-~>" => {
            if labeled {
                lx = x1 + dx * 0.75;
            }
            (curve(x1, y1, 0.7 * dx, 0.0, dx, dy, dx, dy), both.to_string())
        }
        "<-|>" => {
            if labeled {
                lx = x2;
            }
            (ortho(x1, y1, dx, 0.0, 0.0, dy), both.to_string())
        }
        "<-|->" => (elbow(x1, y1, dx, dy), both.to_string()),
        "+" => (
            straight,
            "marker-end:url(#tee);marker-start:url(#tee);fill:none;stroke:#00F;stroke-width:1".to_string(),
        ),
        _ => (straight, "fill:none;stroke:#F00;stroke-width:1".to_string()),
    };
    (d, style, lx, ly)
}

fn append_num(s: &mut String, n: f64) {
    let mut buf = Vec::new();
    push_f64(&mut buf, n);
    s.push_str(std::str::from_utf8(&buf).unwrap_or("0"));
}

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
    let mut s = String::new();
    s.push('M');
    append_num(&mut s, x1);
    s.push(',');
    append_num(&mut s, y1);
    s.push(' ');
    append_num(&mut s, x2);
    s.push(',');
    append_num(&mut s, y2);
    s
}

fn curve(x: f64, y: f64, c1x: f64, c1y: f64, c2x: f64, c2y: f64, ex: f64, ey: f64) -> String {
    let mut s = String::new();
    s.push('M');
    append_num(&mut s, x);
    s.push(',');
    append_num(&mut s, y);
    s.push_str(" c ");
    append_num(&mut s, c1x);
    s.push(',');
    append_num(&mut s, c1y);
    s.push(' ');
    append_num(&mut s, c2x);
    s.push(',');
    append_num(&mut s, c2y);
    s.push(' ');
    append_num(&mut s, ex);
    s.push(',');
    append_num(&mut s, ey);
    s
}

fn ortho(x: f64, y: f64, dx1: f64, dy1: f64, dx2: f64, dy2: f64) -> String {
    let mut s = String::new();
    s.push('m');
    append_num(&mut s, x);
    s.push(',');
    append_num(&mut s, y);
    s.push(' ');
    append_num(&mut s, dx1);
    s.push(',');
    append_num(&mut s, dy1);
    s.push(' ');
    append_num(&mut s, dx2);
    s.push(',');
    append_num(&mut s, dy2);
    s
}

fn elbow(x: f64, y: f64, dx: f64, dy: f64) -> String {
    let mut s = ortho(x, y, dx / 2.0, 0.0, 0.0, dy);
    s.push(' ');
    append_num(&mut s, dx / 2.0);
    s.push_str(",0");
    s
}

fn label(body: &mut Vec<u8>, x: f64, y: f64, text: &str, font: f64) {
    let font = if font > 0.0 { font } else { 11.0 };
    let w = text_width(text, font) + 2.0;
    body.extend_from_slice(b"<g transform=\"translate(");
    push_f64(body, x);
    body.push(b',');
    push_f64(body, y);
    body.extend_from_slice(b")\"><rect x=\"");
    push_f64(body, -(w / 2.0));
    body.extend_from_slice(b"\" y=\"");
    push_f64(body, -(font / 2.0));
    body.extend_from_slice(b"\" width=\"");
    push_f64(body, w);
    body.extend_from_slice(b"\" height=\"");
    push_f64(body, font);
    body.extend_from_slice(b"\" fill=\"#fff\"/><text text-anchor=\"middle\" y=\"");
    push_f64(body, (0.3 * font).round());
    body.extend_from_slice(b"\" font-size=\"");
    push_f64(body, font);
    body.extend_from_slice(b"\">");
    push_esc(body, text);
    body.extend_from_slice(b"</text></g>");
}

fn write_caption(body: &mut Vec<u8>, cap: &Cap, head: bool, xmax: i64, gy: i64, y_tick: i64, hscale: i32) {
    if let Some(text) = &cap.text {
        let y = if head {
            if y_tick != 0 { -33.0 } else { -13.0 }
        } else if y_tick != 0 {
            gy as f64 + 45.0
        } else {
            gy as f64 + 25.0
        };
        body.extend_from_slice(b"<text x=\"");
        push_f64(body, xmax as f64 * XS as f64 / 2.0);
        body.extend_from_slice(b"\" y=\"");
        push_f64(body, y);
        body.extend_from_slice(b"\" text-anchor=\"middle\" xml:space=\"preserve\">");
        push_esc(body, text);
        body.extend_from_slice(b"</text>");
    }
    let mstep = 2.0 * hscale as f64 * XS as f64;
    let marks = if hscale == 0 {
        0.0
    } else {
        xmax as f64 / (2.0 * hscale as f64)
    };
    let y = if head { -5.0 } else { gy as f64 + 15.0 };
    write_ticks(body, &cap.tick, 0.0, mstep, y, marks + 1.0, cap.every);
    write_ticks(body, &cap.tock, mstep / 2.0, mstep, y, marks, cap.every);
}

fn write_ticks(body: &mut Vec<u8>, tick: &Tick, x0: f64, dx: f64, y: f64, len: f64, every: f64) {
    match tick {
        Tick::Off => {}
        Tick::Series { offset, step, dp, fixed } => {
            let mut opened = false;
            let mut i = 0i64;
            while (i as f64) < len && i < 1_000_000 {
                if every == 0.0 || mod_zero(i as f64 + *offset, every) {
                    let value = *step * i as f64 + *offset;
                    let text = if *fixed {
                        format_fixed(value, *dp)
                    } else {
                        format_number(value)
                    };
                    if !opened {
                        body.extend_from_slice(
                            b"<g class=\"muted\" text-anchor=\"middle\" xml:space=\"preserve\">",
                        );
                        opened = true;
                    }
                    tick_text(body, i as f64 * dx + x0, y, &text);
                }
                i += 1;
            }
            if opened {
                body.extend_from_slice(b"</g>");
            }
        }
        Tick::Labels(labels) => {
            if every != 0.0 || labels.is_empty() {
                return;
            }
            body.extend_from_slice(
                b"<g class=\"muted\" text-anchor=\"middle\" xml:space=\"preserve\">",
            );
            let mut i = 0i64;
            while (i as f64) < len {
                let Some(text) = labels.get(i as usize) else { break };
                tick_text(body, i as f64 * dx + x0, y, text);
                i += 1;
            }
            body.extend_from_slice(b"</g>");
        }
    }
}

fn tick_text(body: &mut Vec<u8>, x: f64, y: f64, text: &str) {
    body.extend_from_slice(b"<text x=\"");
    push_f64(body, x);
    body.extend_from_slice(b"\" y=\"");
    push_f64(body, y);
    body.extend_from_slice(b"\">");
    push_esc(body, text);
    body.extend_from_slice(b"</text>");
}

fn mod_zero(n: f64, every: f64) -> bool {
    if every == 0.0 {
        return true;
    }
    let r = n % every;
    r.abs() < 1e-9 || (r - every).abs() < 1e-9 || (r + every).abs() < 1e-9
}

fn format_number(v: f64) -> String {
    if v.is_finite() && (v - v.round()).abs() < 1e-9 && v.abs() < 1.0e15 {
        let mut b = Vec::new();
        push_i64(&mut b, v.round() as i64);
        return String::from_utf8(b).unwrap_or_else(|_| "0".to_string());
    }
    let mut b = Vec::new();
    push_f64(&mut b, v);
    String::from_utf8(b).unwrap_or_else(|_| "0".to_string())
}

fn format_fixed(v: f64, dp: usize) -> String {
    if dp == 0 {
        return format_number(v.round());
    }
    let mut scale: i64 = 1;
    for _ in 0..dp {
        scale = scale.saturating_mul(10);
    }
    let rounded = (v * scale as f64).round();
    let neg = rounded < 0.0;
    let r = rounded.abs() as u64;
    let scale_u = scale as u64;
    let mut b = Vec::new();
    if neg && r != 0 {
        b.push(b'-');
    }
    push_u64_local(&mut b, r / scale_u);
    b.push(b'.');
    let mut frac = r % scale_u;
    let mut digits = vec![b'0'; dp];
    for i in (0..dp).rev() {
        digits[i] = b'0' + (frac % 10) as u8;
        frac /= 10;
    }
    b.extend_from_slice(&digits);
    String::from_utf8(b).unwrap_or_else(|_| "0".to_string())
}

fn push_u64_local(buf: &mut Vec<u8>, mut n: u64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    buf.extend_from_slice(&tmp[i..]);
}

fn write_groups(body: &mut Vec<u8>, doc: &Doc, yhead: i64) {
    body.extend_from_slice(b"<g>");
    for g in &doc.groups {
        let x = g.x as f64 + 0.5;
        let y = g.y as f64 * YO as f64 + 3.5 + yhead as f64;
        let h = g.height as f64 * YO as f64 - 16.0;
        body.extend_from_slice(b"<path fill=\"none\" stroke=\"#0041c4\" stroke-width=\"1\" d=\"M");
        push_f64(body, x);
        body.push(b',');
        push_f64(body, y);
        body.extend_from_slice(b" c -3,0 -5,2 -5,5 l 0,");
        push_f64(body, h);
        body.extend_from_slice(b" c 0,3 2,5 5,5\"/>");
        if let Some(name) = &g.name {
            let tx = g.x as f64 - 10.0;
            let ty = YO as f64 * (g.y as f64 + g.height as f64 / 2.0) + yhead as f64;
            body.extend_from_slice(b"<g transform=\"translate(");
            push_f64(body, tx);
            body.push(b',');
            push_f64(body, ty);
            body.extend_from_slice(
                b") rotate(270)\"><text text-anchor=\"middle\" class=\"info\" xml:space=\"preserve\">",
            );
            push_esc(body, name);
            body.extend_from_slice(b"</text></g>");
        }
    }
    body.extend_from_slice(b"</g>");
}

fn scale_path(d: &str) -> String {
    let sx = 2.0 * XS as f64;
    let sy = -(YS as f64);
    let mut out = String::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let cmd = b[i] as char;
        if cmd.is_ascii_alphabetic() {
            out.push(cmd);
            i += 1;
        }
        let nums = take_nums(b, &mut i);
        match cmd {
            'H' | 'h' => {
                for n in nums {
                    out.push(' ');
                    append_num(&mut out, n * sx);
                }
            }
            'V' | 'v' => {
                for n in nums {
                    out.push(' ');
                    append_num(&mut out, n * sy);
                }
            }
            'A' | 'a' => {
                let mut k = 0;
                while k + 6 < nums.len() {
                    out.push(' ');
                    append_num(&mut out, nums[k] * sx);
                    out.push(',');
                    append_num(&mut out, nums[k + 1] * sy);
                    out.push(' ');
                    append_num(&mut out, nums[k + 2]);
                    out.push(' ');
                    append_num(&mut out, nums[k + 3]);
                    out.push(' ');
                    append_num(&mut out, nums[k + 4]);
                    out.push(' ');
                    append_num(&mut out, nums[k + 5] * sx);
                    out.push(',');
                    append_num(&mut out, nums[k + 6] * sy);
                    k += 7;
                }
            }
            _ => {
                let mut k = 0;
                while k + 1 < nums.len() {
                    out.push(' ');
                    append_num(&mut out, nums[k] * sx);
                    out.push(',');
                    append_num(&mut out, nums[k + 1] * sy);
                    k += 2;
                }
            }
        }
    }
    out
}

fn take_nums(b: &[u8], i: &mut usize) -> Vec<f64> {
    let mut nums = Vec::new();
    loop {
        while *i < b.len() && (b[*i].is_ascii_whitespace() || b[*i] == b',') {
            *i += 1;
        }
        if *i >= b.len() || b[*i].is_ascii_alphabetic() {
            break;
        }
        let start = *i;
        if b[*i] == b'+' || b[*i] == b'-' {
            *i += 1;
        }
        let mut ok = false;
        while *i < b.len() && b[*i].is_ascii_digit() {
            ok = true;
            *i += 1;
        }
        if *i < b.len() && b[*i] == b'.' {
            *i += 1;
            while *i < b.len() && b[*i].is_ascii_digit() {
                ok = true;
                *i += 1;
            }
        }
        if !ok {
            break;
        }
        if let Ok(t) = std::str::from_utf8(&b[start..*i]) {
            if let Ok(n) = t.parse::<f64>() {
                nums.push(n);
            }
        }
    }
    nums
}

fn draw_gap_string(body: &mut Vec<u8>, gaps: &str, nlanes: i64, hscale: i32) {
    let scale = hscale as f64 * XS as f64 * 2.0;
    let height = nlanes * YO;
    for (i, c) in gaps.split_whitespace().enumerate() {
        if c == "." {
            continue;
        }
        let lower = c.to_lowercase();
        let offset = if c == lower { 0.5 } else { 0.0 };
        let x = scale * (i as f64 + offset);
        body.extend_from_slice(b"<g transform=\"translate(");
        push_f64(body, x);
        body.extend_from_slice(b")\">");
        match c {
            "1" | "|" => {
                backdrop(body, 4.0, height);
                vline(body, 0.0, height);
            }
            "2" => {
                backdrop(body, 4.0, height);
                vline(body, -2.0, height);
                vline(body, 2.0, height);
            }
            "3" => {
                backdrop(body, 6.0, height);
                vline(body, -3.0, height);
                vline(body, 0.0, height);
                vline(body, 3.0, height);
            }
            "[" => {
                backdrop(body, 4.0, height);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"#000\" d=\"M2,0 h-4 v");
                push_i64(body, height - 1);
                body.extend_from_slice(b" h4\"/>");
            }
            "]" => {
                backdrop(body, 4.0, height);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"#000\" d=\"M-2,0 h4 v");
                push_i64(body, height - 1);
                body.extend_from_slice(b" h-4\"/>");
            }
            "(" => {
                backdrop(body, 4.0, height);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"#000\" d=\"M2,0 a4,4 0 0 0 -4,4 v");
                push_i64(body, height - 9);
                body.extend_from_slice(b" a4,4 0 0 0 4,4\"/>");
            }
            ")" => {
                backdrop(body, 4.0, height);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"#000\" d=\"M-2,0 a4,4 0 0 1 4,4 v");
                push_i64(body, height - 9);
                body.extend_from_slice(b" a4,4 0 0 1 -4,4\"/>");
            }
            _ => backdrop(body, 4.0, height),
        }
        body.extend_from_slice(b"</g>");
    }
}

fn backdrop(body: &mut Vec<u8>, w: f64, h: i64) {
    body.extend_from_slice(b"<rect x=\"");
    push_f64(body, -w / 2.0);
    body.extend_from_slice(b"\" width=\"");
    push_f64(body, w);
    body.extend_from_slice(b"\" height=\"");
    push_i64(body, h);
    body.extend_from_slice(b"\" fill=\"#ffffffcc\" stroke=\"none\"/>");
}

fn vline(body: &mut Vec<u8>, x: f64, h: i64) {
    body.extend_from_slice(b"<line x1=\"");
    push_f64(body, x);
    body.extend_from_slice(b"\" x2=\"");
    push_f64(body, x);
    body.extend_from_slice(b"\" y1=\"0\" y2=\"");
    push_i64(body, h);
    body.extend_from_slice(b"\" stroke=\"#000\" stroke-width=\"1\"/>");
}
