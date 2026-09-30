//! SVG document writer. Coordinates are appended as integers or short decimals.

use crate::geom::{self, Paint};
use crate::scan::{node_slots, Body, Cap, Doc, Edge, Slot, Tick};
use crate::scheme::Scheme;
use crate::w::{prettify, push_esc, push_f64, push_i64};
use crate::wave;
use crate::width::text_width;
use crate::{Error, TGO, XLABEL, XS, Y0, YM, YO, YS};

struct Ev {
    name: String,
    x: f64,
    y: f64,
}

pub(crate) fn write(doc: &Doc, out: &mut Vec<u8>, indent: u8) -> Result<(), Error> {
    write_themed(doc, out, indent, &crate::scheme::LIGHT)
}

pub(crate) fn write_themed(
    doc: &Doc,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let n = doc.lanes.len() as i64;
    let mut waves = Vec::with_capacity(doc.lanes.len());
    let mut node_origins = Vec::with_capacity(doc.lanes.len());
    let mut node_slots_per_lane = Vec::with_capacity(doc.lanes.len());
    let mut xmax = 0.0_f64;
    for lane in &doc.lanes {
        let wave = match &lane.body {
            Body::Wave(s) => Some(wave::compile_window(
                s,
                lane.period,
                doc.hscale,
                lane.phase * 2.0 + doc.xmin as f64,
                (doc.xmax_cfg - doc.xmin).max(0) as f64,
            )?),
            Body::Path(d) => {
                let extent = crate::path::extent(d)?;
                xmax = xmax.max(
                    extent * lane.period * doc.hscale as f64 * 2.0
                        - lane.phase * 2.0
                        - doc.xmin as f64,
                );
                None
            }
            Body::None => None,
        };
        if let Some(w) = &wave {
            xmax = xmax
                .max(wave::visible_bricks(w) as f64 + frac_dx(lane.phase, doc.xmin) / XS as f64);
        }
        let positions = match (&lane.node, &lane.body) {
            (Some(_), Body::Wave(s)) => wave::node_positions(s, lane.period, doc.hscale)?,
            _ => Vec::new(),
        };
        let slots = if let Some(node) = &lane.node {
            let slots = node_slots(node).map_err(|err| Error {
                offset: lane.node_at + err.rel,
                message: format!("lane {}: {}", lane_label(&lane.name), err.message),
            })?;
            let count = slots.len();
            let extent = if let Some(w) = &wave {
                w.len as f64
                    + count.saturating_sub(positions.len()) as f64
                        * lane.period
                        * doc.hscale as f64
                        * 2.0
            } else {
                count as f64 * lane.period * doc.hscale as f64 * 2.0
            };
            xmax = xmax.max(extent - lane.phase * 2.0 - doc.xmin as f64);
            Some(slots)
        } else {
            None
        };
        node_slots_per_lane.push(slots);
        node_origins.push(positions);
        waves.push(wave);
    }
    let xmax = xmax.max(0.0).min((doc.xmax_cfg - doc.xmin).max(0) as f64);
    let plot_width = xmax * XS as f64;
    for cap in [&doc.head, &doc.foot] {
        for tick in [&cap.tick, &cap.tock] {
            if let Tick::Series { offset, step, .. } = tick {
                if !(*offset + *step * (xmax / (2.0 * doc.hscale as f64) + 1.0)).is_finite() {
                    return Err(Error {
                        offset: 0,
                        message: "tick series exceeds finite range".into(),
                    });
                }
            }
        }
    }
    let max_label = doc
        .lanes
        .iter()
        .map(|lane| text_width(&lane.name, 12.0) + lane.indent as f64)
        .fold(0.0_f64, f64::max);
    let mut xg = ((max_label - TGO as f64 + 12.0) / XS as f64).ceil() * XS as f64;
    let (yh0, yh1) = margins(&doc.head);
    let (yf0, yf1) = margins(&doc.foot);
    let gy = n * YO;
    let caption_width = [&doc.head, &doc.foot]
        .iter()
        .filter_map(|cap| cap.text.as_ref())
        .map(|text| text_width(text, 14.0))
        .fold(0.0_f64, f64::max);
    xg = xg.max((caption_width - plot_width) / 2.0 + 16.0).ceil();
    let tick_room = [&doc.head, &doc.foot]
        .iter()
        .map(|cap| {
            let (ticks, tocks) = tick_counts(xmax, doc.hscale);
            tick_width(&cap.tick, ticks).max(tick_width(&cap.tock, tocks)) / 2.0 + 8.0
        })
        .fold(0.0_f64, f64::max);
    xg = xg.max(tick_room).ceil();
    let right = 20.0_f64
        .max((caption_width - plot_width) / 2.0 + 16.0)
        .max(tick_room);
    let width = (xg + plot_width + right).max(40.0);
    let yhead = 12 + yh0 + yh1;
    let height = (gy + yhead + yf0 + yf1 + 12).max(24);

    let mut paint = Paint::with(scheme);
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
        paint.body.extend_from_slice(b")\"><text x=\"");
        push_i64(&mut paint.body, TGO);
        paint.body.extend_from_slice(b"\" y=\"");
        push_i64(&mut paint.body, YM);
        paint
            .body
            .extend_from_slice(b"\" text-anchor=\"end\" fill=\"");
        paint.body.extend_from_slice(scheme.label.as_bytes());
        paint
            .body
            .extend_from_slice(b"\" font-weight=\"500\" xml:space=\"preserve\">");
        push_esc(&mut paint.body, &lane.name);
        paint.body.extend_from_slice(b"</text><g clip-path=\"url(#");
        paint
            .body
            .extend_from_slice(if lane.phase * 2.0 + doc.xmin as f64 > 0.0 {
                b"lane-crop"
            } else {
                b"lane-clip"
            });
        paint.body.extend_from_slice(b")\">");
        let dx = frac_dx(lane.phase, doc.xmin);
        paint.body.extend_from_slice(b"<g transform=\"translate(");
        push_f64(&mut paint.body, dx);
        paint.body.extend_from_slice(b")\">");
        if let Some(w) = &waves[idx] {
            let end = w.skip + ((plot_width - dx) / XS as f64).ceil().max(0.0) as i64;
            geom::paint_wave(&mut paint, &w.pats, w.skip, end);
            let from = w.unseen.min(lane.data.len());
            for (i, mx) in w.markers.iter().enumerate() {
                if *mx + dx < 0.0 || *mx + dx > plot_width {
                    continue;
                }
                if let Some(label) = lane.data.get(from + i) {
                    paint.body.extend_from_slice(b"<text x=\"");
                    push_f64(&mut paint.body, *mx);
                    paint.body.extend_from_slice(b"\" y=\"");
                    push_i64(&mut paint.body, YM);
                    paint
                        .body
                        .extend_from_slice(b"\" text-anchor=\"middle\" xml:space=\"preserve\"");
                    let room = (w.marker_widths[i] - 10.0).max(1.0);
                    if text_width(label, 12.0) > room {
                        paint.body.extend_from_slice(b" textLength=\"");
                        push_f64(&mut paint.body, room);
                        paint
                            .body
                            .extend_from_slice(b"\" lengthAdjust=\"spacingAndGlyphs\"");
                    }
                    paint.body.push(b'>');
                    push_esc(&mut paint.body, label);
                    paint.body.extend_from_slice(b"</text>");
                }
            }
        }
        paint.body.extend_from_slice(b"</g>");
        if let Body::Path(d) = &lane.body {
            // The path is stored in cycle units and scaled into the lane.
            // resvg ignores vector-effect, so a 1-unit stroke becomes a filled
            // band. The width is the inverse of the scale, which is 1px after
            // the transform in every renderer.
            let sx = 2.0 * XS as f64 * lane.period * doc.hscale as f64;
            let sy = -(YS as f64);
            let stroke = 1.0 / sx.abs().max(sy.abs());
            paint.body.extend_from_slice(b"<g transform=\"translate(");
            push_f64(
                &mut paint.body,
                -(lane.phase * 2.0 + doc.xmin as f64) * XS as f64,
            );
            paint.body.push(b',');
            push_i64(&mut paint.body, YS);
            paint.body.extend_from_slice(b") scale(");
            push_f64(&mut paint.body, sx);
            paint.body.push(b',');
            push_f64(&mut paint.body, sy);
            paint
                .body
                .extend_from_slice(b")\"><path fill=\"none\" stroke=\"");
            paint.body.extend_from_slice(scheme.ink.as_bytes());
            paint.body.extend_from_slice(b"\" stroke-width=\"");
            push_f64(&mut paint.body, stroke);
            paint.body.extend_from_slice(b"\" d=\"");
            push_esc(&mut paint.body, d);
            paint.body.extend_from_slice(b"\"/></g>");
        }
        if let Some(s) = &lane.over {
            draw_ou(
                &mut paint.body,
                s,
                false,
                lane.period * doc.hscale as f64,
                lane.phase + doc.xmin as f64 / 2.0,
                plot_width,
                scheme,
            );
        }
        if let Some(s) = &lane.under {
            draw_ou(
                &mut paint.body,
                s,
                true,
                lane.period * doc.hscale as f64,
                lane.phase + doc.xmin as f64 / 2.0,
                plot_width,
                scheme,
            );
        }
        if let Some(w) = &waves[idx] {
            for gx in &w.gaps {
                if *gx >= 0.0 && *gx <= plot_width {
                    draw_gap(&mut paint.body, *gx, scheme);
                }
            }
        }
        paint.body.extend_from_slice(b"</g></g>");
        if let Some(slots) = &node_slots_per_lane[idx] {
            let phase = lane.phase * 2.0 + doc.xmin as f64;
            let positions = &node_origins[idx];
            for (pos, slot) in slots.iter().enumerate() {
                let Slot::Name(name) = slot else {
                    continue;
                };
                let origin = positions.get(pos).copied().unwrap_or_else(|| {
                    let (start, extra) = if let Some(w) = &waves[idx] {
                        (
                            w.len as f64 * XS as f64,
                            pos.saturating_sub(positions.len()),
                        )
                    } else {
                        (0.0, pos)
                    };
                    start + XS as f64 * 2.0 * extra as f64 * lane.period * doc.hscale as f64
                });
                let x = origin - XS as f64 * phase + XLABEL as f64;
                let y = idx as f64 * YO as f64 + Y0 as f64 + YS as f64 * 0.5;
                events.push(Ev {
                    name: name.clone(),
                    x,
                    y,
                });
            }
        }
    }
    paint
        .body
        .extend_from_slice(b"<g clip-path=\"url(#plot-clip)\">");
    write_arcs(&mut paint.body, doc, &events, plot_width, scheme)?;
    if let Some(g) = &doc.gaps {
        draw_gap_string(
            &mut paint.body,
            g,
            n,
            doc.hscale,
            doc.xmin,
            plot_width,
            scheme,
        );
    }
    paint.body.extend_from_slice(b"</g>");
    write_caption(
        &mut paint.body,
        &doc.head,
        true,
        xmax,
        gy,
        yh0,
        doc.hscale,
        scheme,
    );
    write_caption(
        &mut paint.body,
        &doc.foot,
        false,
        xmax,
        gy,
        yf0,
        doc.hscale,
        scheme,
    );

    out.extend_from_slice(b"<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"tgw\" role=\"img\" aria-labelledby=\"diagram-title\" width=\"");
    push_f64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" viewBox=\"0 0 ");
    push_f64(out, width);
    out.push(b' ');
    push_i64(out, height);
    out.extend_from_slice(b"\" overflow=\"hidden\" font-family=\"Inter, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif\" font-size=\"12\" fill=\"");
    out.extend_from_slice(scheme.text.as_bytes());
    out.extend_from_slice(b"\" stroke-linejoin=\"round\"><title id=\"diagram-title\">");
    if let Some(title) = &doc.head.text {
        push_esc(out, title);
    } else {
        out.extend_from_slice(b"Signal timing diagram");
    }
    out.extend_from_slice(b"</title><desc>");
    push_i64(out, n);
    out.extend_from_slice(b" signal lanes");
    for lane in &doc.lanes {
        if !lane.name.is_empty() {
            out.extend_from_slice(b"; ");
            push_esc(out, &lane.name);
        }
    }
    out.extend_from_slice(b"</desc><defs>");
    if !doc.edges.is_empty() {
        write_markers(out, scheme);
    }
    if paint.hatch {
        out.extend_from_slice(
            b"<pattern id=\"xh\" width=\"6\" height=\"6\" patternUnits=\"userSpaceOnUse\"><rect width=\"6\" height=\"6\" fill=\"",
        );
        out.extend_from_slice(scheme.hatch_fill.as_bytes());
        out.extend_from_slice(
            b"\"/><path d=\"M-1,1 L1,-1 M0,6 L6,0 M5,7 L7,5\" fill=\"none\" stroke=\"",
        );
        out.extend_from_slice(scheme.hatch_line.as_bytes());
        out.extend_from_slice(b"\" stroke-width=\"0.7\"/></pattern>");
    }
    for (i, key) in paint.clocks.iter().enumerate() {
        geom::write_clock_pattern(out, i, key, scheme.ink);
    }
    clip_rect(
        out,
        "lane-clip",
        -4.0,
        -4.0,
        plot_width + 4.5,
        YS as f64 + 8.0,
    );
    clip_rect(
        out,
        "lane-crop",
        0.0,
        -4.0,
        plot_width + 0.5,
        YS as f64 + 8.0,
    );
    let left_clip = if doc.xmin > 0 { 0.0 } else { -4.0 };
    clip_rect(
        out,
        "plot-clip",
        left_clip,
        -4.0,
        plot_width - left_clip + 0.5,
        gy as f64 + 8.0,
    );
    if let Some(g) = &grid {
        out.extend_from_slice(b"<pattern id=\"gd\" width=\"");
        push_i64(out, g.step);
        out.extend_from_slice(b"\" height=\"");
        push_i64(out, g.height);
        out.extend_from_slice(b"\" patternUnits=\"userSpaceOnUse\"><path d=\"M0.5,0 V");
        push_i64(out, g.height);
        out.extend_from_slice(b"\" fill=\"none\" stroke=\"");
        out.extend_from_slice(scheme.grid.as_bytes());
        out.extend_from_slice(b"\" stroke-width=\"1\" stroke-dasharray=\"2,4\"/></pattern>");
    }
    out.extend_from_slice(b"</defs><rect width=\"");
    push_f64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(scheme.paper.as_bytes());
    out.extend_from_slice(b"\"/><g transform=\"translate(");
    push_f64(out, xg + 0.5);
    out.push(b',');
    push_f64(out, yhead as f64 + 0.5);
    out.extend_from_slice(b")\">");
    if let Some(g) = &grid {
        out.extend_from_slice(b"<rect x=\"0\" y=\"0\" width=\"");
        push_f64(out, g.width);
        out.extend_from_slice(b"\" height=\"");
        push_i64(out, g.height);
        out.extend_from_slice(b"\" fill=\"url(#gd)\" stroke=\"none\"/>");
    }
    out.extend_from_slice(&paint.body);
    out.extend_from_slice(b"</g>");
    write_groups(out, doc, yhead, scheme);
    out.extend_from_slice(b"</svg>");
    scope_ids(out);
    if indent > 0 {
        let pretty = prettify(out, indent);
        out.clear();
        out.extend_from_slice(&pretty);
    }
    Ok(())
}

fn clip_rect(out: &mut Vec<u8>, id: &str, x: f64, y: f64, width: f64, height: f64) {
    out.extend_from_slice(b"<clipPath id=\"");
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(b"\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"");
    push_f64(out, x);
    out.extend_from_slice(b"\" y=\"");
    push_f64(out, y);
    out.extend_from_slice(b"\" width=\"");
    push_f64(out, width.max(0.0));
    out.extend_from_slice(b"\" height=\"");
    push_f64(out, height.max(0.0));
    out.extend_from_slice(b"\"/></clipPath>");
}

// A stable document namespace prevents paint servers in different inline SVGs
// from resolving to one another. Identical diagrams can safely share definitions.
fn scope_ids(out: &mut Vec<u8>) {
    // Hash eight bytes at a time: a byte-by-byte dependent multiply dominated
    // rendering time for dense buses. The fixed little-endian hash is stable
    // across platforms and includes the complete rendered document.
    let mut lanes = [
        0xcbf29ce484222325_u64,
        0x9e3779b97f4a7c15,
        0x517cc1b727220a95,
        0x6eed0e9da4d94a4f,
    ];
    let (blocks, rest) = out.as_chunks::<32>();
    for block in blocks {
        for (state, word) in lanes.iter_mut().zip(block.as_chunks::<8>().0) {
            *state = (*state ^ u64::from_le_bytes(*word)).wrapping_mul(0x100000001b3);
        }
    }
    let mut hash = out.len() as u64;
    for state in lanes {
        hash = (hash ^ state).wrapping_mul(0x100000001b3);
    }
    for &byte in rest {
        hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
    }
    hash ^= hash >> 32;
    let svg = std::str::from_utf8(out).expect("SVG writer preserves UTF-8");
    let mut offsets = Vec::new();
    let header = &svg[..svg.find("</defs>").unwrap_or(svg.len())];
    for token in [" id=\"", " aria-labelledby=\""] {
        offsets.extend(header.match_indices(token).map(|(i, _)| i + token.len()));
    }
    let mut cursor = 0;
    let mut in_tag = false;
    for (i, _) in svg.match_indices("url(#") {
        if let Some(boundary) = svg[cursor..i].rfind(['<', '>']) {
            in_tag = svg.as_bytes()[cursor + boundary] == b'<';
        }
        if in_tag {
            offsets.push(i + 5);
        }
        cursor = i + 5;
    }
    offsets.sort_unstable();
    let prefix = format!("tgw-{hash:016x}-");
    let old_len = out.len();
    out.resize(old_len + offsets.len() * prefix.len(), 0);
    let mut end = old_len;
    for (index, start) in offsets.into_iter().enumerate().rev() {
        let shift = (index + 1) * prefix.len();
        out.copy_within(start..end, start + shift);
        out[start + index * prefix.len()..start + shift].copy_from_slice(prefix.as_bytes());
        end = start;
    }
}

fn margins(cap: &Cap) -> (i64, i64) {
    let tick = !matches!(cap.tick, Tick::Off) || !matches!(cap.tock, Tick::Off);
    let y0 = if tick { 20 } else { 0 };
    let y1 = if cap.text.is_some() { 36 } else { 0 };
    (y0, y1)
}

fn frac_dx(phase: f64, xmin: i64) -> f64 {
    let phase_bricks = phase * 2.0 + xmin as f64;
    (phase_bricks.max(0.0).floor() - phase_bricks) * XS as f64
}

struct Grid {
    step: i64,
    width: f64,
    height: i64,
}

fn grid_geom(hscale: i32, xmax: f64, gy: i64) -> Option<Grid> {
    let step = 2 * hscale as i64 * XS;
    if step <= 0 || gy <= 0 {
        return None;
    }
    Some(Grid {
        step,
        width: xmax * XS as f64 + 1.0,
        height: gy,
    })
}

fn write_markers(body: &mut Vec<u8>, scheme: &Scheme) {
    body.extend_from_slice(
        b"<marker id=\"arrowhead\" viewBox=\"0 -4 11 8\" refX=\"11\" refY=\"0\" markerWidth=\"8\" markerHeight=\"6\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,-4 L11,0 L0,4 Z\" fill=\"",
    );
    body.extend_from_slice(scheme.bracket.as_bytes());
    body.extend_from_slice(
        b"\"/></marker><marker id=\"arrowtail\" viewBox=\"-11 -4 11 8\" refX=\"-11\" refY=\"0\" markerWidth=\"8\" markerHeight=\"6\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,-4 L-11,0 L0,4 Z\" fill=\"",
    );
    body.extend_from_slice(scheme.bracket.as_bytes());
    body.extend_from_slice(
        b"\"/></marker><marker id=\"tee\" viewBox=\"0 0 2 6\" refX=\"1\" refY=\"3\" markerWidth=\"2\" markerHeight=\"6\" orient=\"auto\"><path d=\"M1,0 L1,6\" stroke=\"",
    );
    body.extend_from_slice(scheme.bracket.as_bytes());
    body.extend_from_slice(b"\" stroke-width=\"2\"/></marker>");
}

fn draw_gap(body: &mut Vec<u8>, x: f64, scheme: &Scheme) {
    body.extend_from_slice(b"<g transform=\"translate(");
    push_f64(body, x);
    body.extend_from_slice(b")\"><path d=\"M-5,22 C0,22 0,-2 5,-2\" fill=\"none\" stroke=\"");
    body.extend_from_slice(scheme.paper.as_bytes());
    body.extend_from_slice(
        b"\" stroke-width=\"6\"/><path d=\"M-7,22 C-2,22 -2,-2 3,-2\" fill=\"none\" stroke=\"",
    );
    body.extend_from_slice(scheme.ink.as_bytes());
    body.extend_from_slice(
        b"\" stroke-width=\"1\"/><path d=\"M-3,22 C2,22 2,-2 7,-2\" fill=\"none\" stroke=\"",
    );
    body.extend_from_slice(scheme.ink.as_bytes());
    body.extend_from_slice(b"\" stroke-width=\"1\"/></g>");
}

fn draw_ou(
    body: &mut Vec<u8>,
    text: &str,
    under: bool,
    period: f64,
    phase: f64,
    width: f64,
    scheme: &Scheme,
) {
    if text.is_empty() || width <= 0.0 {
        return;
    }
    let step = period * 2.0 * XS as f64;
    let xoff = -phase * 2.0 * XS as f64;
    body.extend_from_slice(b"<g transform=\"translate(0,");
    push_i64(body, if under { YS } else { 0 });
    body.extend_from_slice(b")\" fill=\"none\" stroke-width=\"3\">");
    let mut start = None;
    let mut color = scheme.ink_full;
    for (i, &symbol) in text.as_bytes().iter().enumerate() {
        let x = i as f64 * step + xoff;
        if x > width + 7.0 {
            if let Some(from) = start.take() {
                ou_span(body, from, x, color, false, width);
            }
            break;
        }
        if symbol != b'.' {
            if let Some(from) = start.take() {
                ou_span(body, from, x, color, !under, width);
            }
            if symbol != b'0' {
                start = Some(x + 12.0);
                color = scheme.mark(symbol);
            }
        }
    }
    if let Some(from) = start {
        ou_span(
            body,
            from,
            text.len() as f64 * step + xoff,
            color,
            false,
            width,
        );
    }
    body.extend_from_slice(b"</g>");
}

fn ou_span(body: &mut Vec<u8>, start: f64, end: f64, color: &str, arrow: bool, width: f64) {
    let x1 = start.max(0.0);
    let x2 = end.min(width);
    if x1 < x2 {
        body.extend_from_slice(b"<line stroke=\"");
        body.extend_from_slice(color.as_bytes());
        body.extend_from_slice(b"\" x1=\"");
        push_f64(body, x1);
        body.extend_from_slice(b"\" x2=\"");
        push_f64(body, x2);
        body.extend_from_slice(b"\" y1=\"0\" y2=\"0\"/>");
    }
    if arrow && end >= 0.0 && end <= width && start < end {
        body.extend_from_slice(b"<path fill=\"");
        body.extend_from_slice(color.as_bytes());
        body.extend_from_slice(b"\" stroke=\"none\" d=\"M");
        push_f64(body, end - 7.0);
        body.extend_from_slice(b",0 l7,7 v-7 z\"/>");
    }
}

fn write_arcs(
    body: &mut Vec<u8>,
    doc: &Doc,
    events: &[Ev],
    width: f64,
    scheme: &Scheme,
) -> Result<(), Error> {
    let mut lookup = std::collections::HashMap::with_capacity(events.len());
    let mut defined = Vec::new();
    for ev in events {
        if !lookup.contains_key(&ev.name) {
            defined.push(ev.name.clone());
        }
        lookup.entry(ev.name.clone()).or_insert(ev);
    }
    let mut labels = Vec::new();
    for edge in &doc.edges {
        let mut missing = Vec::new();
        if !lookup.contains_key(&edge.from) {
            missing.push(edge.from.as_str());
        }
        if edge.to != edge.from && !lookup.contains_key(&edge.to) {
            missing.push(edge.to.as_str());
        }
        if !missing.is_empty() {
            let nodes = if defined.is_empty() {
                "none".to_string()
            } else {
                defined.join(" ")
            };
            return Err(Error {
                offset: edge.offset,
                message: format!(
                    "{}: node {} is not defined; nodes are {nodes}",
                    edge_sentence(edge),
                    missing.join(" and ")
                ),
            });
        }
        let a = lookup[&edge.from];
        let b = lookup[&edge.to];
        if a.x.max(b.x) < 0.0 || a.x.min(b.x) > width {
            continue;
        }
        let (d, style, lx, ly) = arc_shape(
            &edge.shape,
            a.x,
            a.y,
            b.x,
            b.y,
            !edge.label.is_empty(),
            scheme,
        )
        .map_err(|_| Error {
            offset: edge.offset,
            message: format!("{}: unknown connector {}", edge_sentence(edge), edge.shape),
        })?;
        body.extend_from_slice(b"<path d=\"");
        body.extend_from_slice(d.as_bytes());
        body.extend_from_slice(b"\" style=\"");
        body.extend_from_slice(style.as_bytes());
        body.extend_from_slice(b"\"/>");
        if !edge.label.is_empty() {
            labels.push((lx, ly, edge.label.clone()));
        }
    }
    for ev in events {
        if show_node_label(&ev.name) && ev.x >= 0.0 && ev.x <= width {
            label(body, ev.x, ev.y, &ev.name, doc.arc_font, scheme);
        }
    }
    for (x, y, text) in labels {
        let half = text_width(&text, doc.arc_font) / 2.0 + 3.0;
        label(
            body,
            x.max(half).min((width - half).max(half)),
            y,
            &text,
            doc.arc_font,
            scheme,
        );
    }
    Ok(())
}

fn show_node_label(name: &str) -> bool {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => !ch.is_uppercase(),
        (Some(_), Some(_)) => true,
        _ => false,
    }
}

fn lane_label(name: &str) -> String {
    if name.trim().is_empty() {
        "unnamed".to_string()
    } else {
        format!("{name:?}")
    }
}

fn edge_sentence(edge: &Edge) -> String {
    let mut text = format!("@edge {}{}{}", edge.from, edge.shape, edge.to);
    if !edge.label.is_empty() {
        text.push(' ');
        text.push_str(&edge.label);
    }
    text
}

fn arc_shape(
    shape: &str,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    labeled: bool,
    scheme: &Scheme,
) -> Result<(String, String, f64, f64), ()> {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let mut lx = (x1 + x2) / 2.0;
    let ly = (y1 + y2) / 2.0;
    let straight = line(x1, y1, x2, y2);
    let blue = format!("fill:none;stroke:{};stroke-width:1", scheme.bracket);
    let arrow = format!(
        "marker-end:url(#arrowhead);stroke:{};stroke-width:1;fill:none",
        scheme.bracket
    );
    let both = format!(
        "marker-end:url(#arrowhead);marker-start:url(#arrowtail);stroke:{};stroke-width:1;fill:none",
        scheme.bracket
    );
    let (d, style) = match shape {
        "" | "-" => (straight, blue.to_string()),
        "~" => (
            curve(x1, y1, 0.7 * dx, 0.0, 0.3 * dx, dy, dx, dy),
            blue.to_string(),
        ),
        "-~" => {
            if labeled {
                lx = x1 + dx * 0.75;
            }
            (
                curve(x1, y1, 0.7 * dx, 0.0, dx, dy, dx, dy),
                blue.to_string(),
            )
        }
        "~-" => {
            if labeled {
                lx = x1 + dx * 0.25;
            }
            (
                curve(x1, y1, 0.0, 0.0, 0.3 * dx, dy, dx, dy),
                blue.to_string(),
            )
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
        "~>" => (
            curve(x1, y1, 0.7 * dx, 0.0, 0.3 * dx, dy, dx, dy),
            arrow.to_string(),
        ),
        "-~>" => {
            if labeled {
                lx = x1 + dx * 0.75;
            }
            (
                curve(x1, y1, 0.7 * dx, 0.0, dx, dy, dx, dy),
                arrow.to_string(),
            )
        }
        "~->" => {
            if labeled {
                lx = x1 + dx * 0.25;
            }
            (
                curve(x1, y1, 0.0, 0.0, 0.3 * dx, dy, dx, dy),
                arrow.to_string(),
            )
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
        "<~->" => {
            if labeled {
                lx = x1 + dx * 0.25;
            }
            (
                curve(x1, y1, 0.0, 0.0, 0.3 * dx, dy, dx, dy),
                both.to_string(),
            )
        }
        "<|->" => {
            if labeled {
                lx = x1;
            }
            (ortho(x1, y1, 0.0, dy, dx, 0.0), both.to_string())
        }
        "<~>" => (
            curve(x1, y1, 0.7 * dx, 0.0, 0.3 * dx, dy, dx, dy),
            both.to_string(),
        ),
        "<-~>" => {
            if labeled {
                lx = x1 + dx * 0.75;
            }
            (
                curve(x1, y1, 0.7 * dx, 0.0, dx, dy, dx, dy),
                both.to_string(),
            )
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
            format!(
                "marker-end:url(#tee);marker-start:url(#tee);fill:none;stroke:{};stroke-width:1",
                scheme.bracket
            ),
        ),
        _ => return Err(()),
    };
    Ok((d, style, lx, ly))
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

#[expect(
    clippy::too_many_arguments,
    reason = "SVG cubic coordinates follow the path command order"
)]
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

fn label(body: &mut Vec<u8>, x: f64, y: f64, text: &str, font: f64, scheme: &Scheme) {
    let font = if font > 0.0 { font } else { 11.0 };
    let w = text_width(text, font) + 6.0;
    body.extend_from_slice(b"<g transform=\"translate(");
    push_f64(body, x);
    body.push(b',');
    push_f64(body, y);
    body.extend_from_slice(b")\"><rect x=\"");
    push_f64(body, -(w / 2.0));
    body.extend_from_slice(b"\" y=\"");
    push_f64(body, -(font / 2.0) - 2.0);
    body.extend_from_slice(b"\" width=\"");
    push_f64(body, w);
    body.extend_from_slice(b"\" height=\"");
    push_f64(body, font + 4.0);
    body.extend_from_slice(b"\" rx=\"2\" fill=\"");
    body.extend_from_slice(scheme.paper.as_bytes());
    body.extend_from_slice(b"\"/><text text-anchor=\"middle\" y=\"");
    push_f64(body, (0.3 * font).round());
    body.extend_from_slice(b"\" font-size=\"");
    push_f64(body, font);
    body.extend_from_slice(b"\">");
    push_esc(body, text);
    body.extend_from_slice(b"</text></g>");
}

#[expect(
    clippy::too_many_arguments,
    reason = "the palette joins the caption's existing placement arguments"
)]
fn write_caption(
    body: &mut Vec<u8>,
    cap: &Cap,
    head: bool,
    xmax: f64,
    gy: i64,
    y_tick: i64,
    hscale: i32,
    scheme: &Scheme,
) {
    if let Some(text) = &cap.text {
        let y = if head {
            if y_tick != 0 {
                -33.0
            } else {
                -13.0
            }
        } else if y_tick != 0 {
            gy as f64 + 45.0
        } else {
            gy as f64 + 25.0
        };
        body.extend_from_slice(b"<text x=\"");
        push_f64(body, xmax * XS as f64 / 2.0);
        body.extend_from_slice(b"\" y=\"");
        push_f64(body, y);
        body.extend_from_slice(b"\" text-anchor=\"middle\" font-size=\"14\" font-weight=\"600\" xml:space=\"preserve\">");
        push_esc(body, text);
        body.extend_from_slice(b"</text>");
    }
    let mstep = 2.0 * hscale as f64 * XS as f64;
    let (ticks, tocks) = tick_counts(xmax, hscale);
    let y = if head { -5.0 } else { gy as f64 + 15.0 };
    write_ticks(body, &cap.tick, 0.0, mstep, y, ticks, cap.every, scheme);
    write_ticks(
        body,
        &cap.tock,
        mstep / 2.0,
        mstep,
        y,
        tocks,
        cap.every,
        scheme,
    );
}

fn tick_counts(xmax: f64, hscale: i32) -> (usize, usize) {
    let marks = xmax.max(0.0) / (2.0 * hscale as f64);
    let ticks = marks.floor() as usize + 1;
    let tocks = if marks >= 0.5 {
        (marks - 0.5).floor() as usize + 1
    } else {
        0
    };
    (ticks, tocks)
}

fn series_text(value: f64, dp: usize, fixed: bool) -> String {
    let mut out = String::new();
    append_series(&mut out, value, dp, fixed);
    out
}

fn append_series(out: &mut String, value: f64, dp: usize, fixed: bool) {
    use std::fmt::Write;
    if fixed {
        let _ = write!(out, "{value:.precision$}", precision = dp.min(15));
    } else {
        append_number(out, value);
    }
}

fn tick_width(tick: &Tick, count: usize) -> f64 {
    if count == 0 {
        return 0.0;
    }
    match tick {
        Tick::Off => 0.0,
        Tick::Series {
            offset,
            step,
            dp,
            fixed,
        } => {
            let last = *offset + *step * count.saturating_sub(1) as f64;
            text_width(&series_text(*offset, *dp, *fixed), 11.0)
                .max(text_width(&series_text(last, *dp, *fixed), 11.0))
        }
        Tick::Labels(labels) => labels
            .iter()
            .take(count)
            .map(|text| text_width(text, 11.0))
            .fold(0.0_f64, f64::max),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the palette joins the tick writer's existing placement arguments"
)]
fn write_ticks(
    body: &mut Vec<u8>,
    tick: &Tick,
    x0: f64,
    dx: f64,
    y: f64,
    count: usize,
    every: f64,
    scheme: &Scheme,
) {
    let count = match tick {
        Tick::Off => return,
        Tick::Labels(labels) => count.min(labels.len()),
        _ => count,
    };
    if count == 0 {
        return;
    }
    let offset = match tick {
        Tick::Series { offset, .. } => *offset,
        _ => 0.0,
    };
    let Some((first, period)) = tick_filter(offset, every) else {
        return;
    };
    let minimum = (((tick_width(tick, count) + 10.0) / dx).ceil() as usize)
        .max(count.div_ceil(10_000))
        .max(1);
    // Thin the already selected tick sequence, keeping its phase. Sampling the
    // unfiltered indices can alias `every` and accidentally remove every label.
    let stride = period.saturating_mul(minimum.div_ceil(period).max(1));
    let mut opened = false;
    let mut label = String::new();
    for i in (first..count).step_by(stride) {
        if every != 0.0 && !mod_zero(i as f64 + offset, every) {
            continue;
        }
        let text = match tick {
            Tick::Series {
                offset,
                step,
                dp,
                fixed,
            } => {
                label.clear();
                append_series(&mut label, *offset + *step * i as f64, *dp, *fixed);
                label.as_str()
            }
            Tick::Labels(labels) => labels[i].as_str(),
            Tick::Off => unreachable!(),
        };
        if !opened {
            body.extend_from_slice(b"<g fill=\"");
            body.extend_from_slice(scheme.tick.as_bytes());
            body.extend_from_slice(
                b"\" font-size=\"11\" text-anchor=\"middle\" xml:space=\"preserve\">",
            );
            opened = true;
        }
        tick_text(body, i as f64 * dx + x0, y, text);
    }
    if opened {
        body.extend_from_slice(b"</g>");
    }
}

// Return the first integral index and the repetition interval satisfying
// (index + offset) % every == 0. Rational intervals include fractional `every`
// values such as 1.5 (every third integral index), without scanning huge axes.
fn tick_filter(offset: f64, every: f64) -> Option<(usize, usize)> {
    let every = every.abs();
    if every <= 1e-9 {
        return Some((0, 1));
    }
    if every.fract() == 0.0 {
        let first = (-offset).rem_euclid(every);
        if (first - first.round()).abs() > 1e-9 || first > usize::MAX as f64 {
            return None;
        }
        return Some((first.round() as usize, (every as usize).max(1)));
    }
    let (p, q) = rational_interval(every)?;
    let remainder = offset.rem_euclid(every) * q as f64;
    if (remainder - remainder.round()).abs() > 1e-7 {
        return None;
    }
    let remainder = (remainder.round() as i128).rem_euclid(p);
    let k = if q == 1 {
        0
    } else {
        (remainder.rem_euclid(q) * inverse_mod(p.rem_euclid(q), q)).rem_euclid(q)
    };
    let first = ((k * p - remainder) / q).rem_euclid(p);
    Some((
        usize::try_from(first).ok()?,
        usize::try_from(p).unwrap_or(usize::MAX).max(1),
    ))
}

fn rational_interval(value: f64) -> Option<(i128, i128)> {
    let (mut p0, mut p1, mut q0, mut q1) = (0i128, 1i128, 1i128, 0i128);
    let mut rest = value;
    for _ in 0..32 {
        if !rest.is_finite() || rest > 1e30 {
            return None;
        }
        let whole = rest.floor() as i128;
        let p = whole.checked_mul(p1)?.checked_add(p0)?;
        let q = whole.checked_mul(q1)?.checked_add(q0)?;
        if q > 1_000_000_000 || p > 1_000_000_000_000_000_000_000_000_000_000i128 {
            return None;
        }
        if q > 0 && p > 0 && (p as f64 / q as f64 - value).abs() < 1e-12 {
            return Some((p, q));
        }
        (p0, p1, q0, q1) = (p1, p, q1, q);
        rest = 1.0 / (rest - whole as f64);
    }
    None
}

fn inverse_mod(value: i128, modulus: i128) -> i128 {
    let (mut a, mut b, mut x, mut y) = (value, modulus, 1i128, 0i128);
    while b != 0 {
        let quotient = a / b;
        (a, b) = (b, a - quotient * b);
        (x, y) = (y, x - quotient * y);
    }
    x.rem_euclid(modulus)
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

fn append_number(out: &mut String, v: f64) {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1.0e15 {
        append_i64(out, v.round() as i64);
        return;
    }
    // Tick values are data, not pixel coordinates: keep their precision rather
    // than applying the SVG coordinate writer's millipixel rounding.
    use std::fmt::Write;
    let _ = write!(out, "{v}");
}

fn append_i64(out: &mut String, n: i64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    let mut value = n.unsigned_abs();
    loop {
        i -= 1;
        tmp[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    if n < 0 {
        i -= 1;
        tmp[i] = b'-';
    }
    out.push_str(std::str::from_utf8(&tmp[i..]).unwrap_or("0"));
}

fn write_groups(body: &mut Vec<u8>, doc: &Doc, yhead: i64, scheme: &Scheme) {
    body.extend_from_slice(b"<g>");
    for g in &doc.groups {
        if g.height <= 0 {
            continue;
        }
        let x = g.x as f64 + 0.5;
        let y = g.y as f64 * YO as f64 + 3.5 + yhead as f64;
        let h = g.height as f64 * YO as f64 - 16.0;
        body.extend_from_slice(b"<path fill=\"none\" stroke=\"");
        body.extend_from_slice(scheme.bracket.as_bytes());
        body.extend_from_slice(b"\" stroke-width=\"1\" d=\"M");
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
            body.extend_from_slice(b") rotate(270)\"><text text-anchor=\"middle\" fill=\"");
            body.extend_from_slice(scheme.muted.as_bytes());
            body.extend_from_slice(b"\" xml:space=\"preserve\"");
            let room = (g.height as f64 * YO as f64 - 12.0).max(1.0);
            if text_width(name, 12.0) > room {
                body.extend_from_slice(b" textLength=\"");
                push_f64(body, room);
                body.extend_from_slice(b"\" lengthAdjust=\"spacingAndGlyphs\"");
            }
            body.push(b'>');
            push_esc(body, name);
            body.extend_from_slice(b"</text></g>");
        }
    }
    body.extend_from_slice(b"</g>");
}

fn draw_gap_string(
    body: &mut Vec<u8>,
    gaps: &str,
    nlanes: i64,
    hscale: i32,
    xmin: i64,
    width: f64,
    scheme: &Scheme,
) {
    if nlanes <= 0 || width <= 0.0 {
        return;
    }
    let scale = hscale as f64 * XS as f64 * 2.0;
    let height = nlanes * YO;
    for (i, c) in gaps.split_whitespace().enumerate() {
        if c == "." {
            continue;
        }
        let lower = c.to_lowercase();
        let offset = if c == lower { 0.5 } else { 0.0 };
        let x = scale * (i as f64 + offset) - xmin as f64 * XS as f64;
        if x < -6.0 {
            continue;
        }
        if x > width + 6.0 {
            break;
        }
        body.extend_from_slice(b"<g transform=\"translate(");
        push_f64(body, x);
        body.extend_from_slice(b")\">");
        match c {
            "1" | "|" => {
                backdrop(body, 4.0, height, scheme);
                vline(body, 0.0, height, scheme);
            }
            "2" => {
                backdrop(body, 4.0, height, scheme);
                vline(body, -2.0, height, scheme);
                vline(body, 2.0, height, scheme);
            }
            "3" => {
                backdrop(body, 6.0, height, scheme);
                vline(body, -3.0, height, scheme);
                vline(body, 0.0, height, scheme);
                vline(body, 3.0, height, scheme);
            }
            "[" => {
                backdrop(body, 4.0, height, scheme);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"");
                body.extend_from_slice(scheme.ink.as_bytes());
                body.extend_from_slice(b"\" d=\"M2,0 h-4 v");
                push_i64(body, height - 1);
                body.extend_from_slice(b" h4\"/>");
            }
            "]" => {
                backdrop(body, 4.0, height, scheme);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"");
                body.extend_from_slice(scheme.ink.as_bytes());
                body.extend_from_slice(b"\" d=\"M-2,0 h4 v");
                push_i64(body, height - 1);
                body.extend_from_slice(b" h-4\"/>");
            }
            "(" => {
                backdrop(body, 4.0, height, scheme);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"");
                body.extend_from_slice(scheme.ink.as_bytes());
                body.extend_from_slice(b"\" d=\"M2,0 a4,4 0 0 0 -4,4 v");
                push_i64(body, height - 9);
                body.extend_from_slice(b" a4,4 0 0 0 4,4\"/>");
            }
            ")" => {
                backdrop(body, 4.0, height, scheme);
                body.extend_from_slice(b"<path fill=\"none\" stroke=\"");
                body.extend_from_slice(scheme.ink.as_bytes());
                body.extend_from_slice(b"\" d=\"M-2,0 a4,4 0 0 1 4,4 v");
                push_i64(body, height - 9);
                body.extend_from_slice(b" a4,4 0 0 1 -4,4\"/>");
            }
            _ => backdrop(body, 4.0, height, scheme),
        }
        body.extend_from_slice(b"</g>");
    }
}

fn backdrop(body: &mut Vec<u8>, w: f64, h: i64, scheme: &Scheme) {
    body.extend_from_slice(b"<rect x=\"");
    push_f64(body, -w / 2.0);
    body.extend_from_slice(b"\" width=\"");
    push_f64(body, w);
    body.extend_from_slice(b"\" height=\"");
    push_i64(body, h);
    body.extend_from_slice(b"\" fill=\"");
    body.extend_from_slice(scheme.paper.as_bytes());
    body.extend_from_slice(b"\" fill-opacity=\"0.9\" stroke=\"none\"/>");
}

fn vline(body: &mut Vec<u8>, x: f64, h: i64, scheme: &Scheme) {
    body.extend_from_slice(b"<line x1=\"");
    push_f64(body, x);
    body.extend_from_slice(b"\" x2=\"");
    push_f64(body, x);
    body.extend_from_slice(b"\" y1=\"0\" y2=\"");
    push_i64(body, h);
    body.extend_from_slice(b"\" stroke=\"");
    body.extend_from_slice(scheme.ink.as_bytes());
    body.extend_from_slice(b"\" stroke-width=\"1\"/>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_stay_inside_fractional_plot_widths() {
        assert_eq!(tick_counts(0.5, 1), (1, 0));
        assert_eq!(tick_counts(1.5, 1), (1, 1));
        assert_eq!(tick_counts(3.0, 1), (2, 2));
        assert_eq!(tick_counts(4.0, 1), (3, 2));
    }

    #[test]
    fn thinning_keeps_the_requested_tick_phase() {
        let tick = Tick::Series {
            offset: 10001.0,
            step: 1.0,
            dp: 0,
            fixed: false,
        };
        let mut out = Vec::new();
        write_ticks(
            &mut out,
            &tick,
            0.0,
            40.0,
            0.0,
            20,
            2.0,
            &crate::scheme::LIGHT,
        );
        let svg = String::from_utf8(out).unwrap();
        assert!(svg.contains("x=\"40\" y=\"0\">10002</text>"));
        assert!(!svg.contains(">10001</text>"));
        assert!(svg.matches("<text ").count() >= 5);
    }

    #[test]
    fn fractional_tick_filters_match_a_direct_small_reference() {
        for every in [0.1, 0.2, 0.3, 0.5, 0.75, 1.5, 2.5, 3.2, 10.0] {
            for offset in [0.0, 0.1, 0.25, 0.5, 1.0, 2.0, -1.0, -0.5] {
                let expected: Vec<_> = (0..100)
                    .filter(|i| mod_zero(*i as f64 + offset, every))
                    .collect();
                let got: Vec<_> = tick_filter(offset, every)
                    .map_or_else(Vec::new, |(start, period)| {
                        (start..100).step_by(period).collect()
                    });
                assert_eq!(got, expected, "offset={offset}, every={every}");
            }
        }
    }

    #[test]
    fn fixed_tick_precision_controls_spacing_and_margins() {
        let tick = Tick::Series {
            offset: 0.0,
            step: 1.0,
            dp: 15,
            fixed: true,
        };
        let width = tick_width(&tick, 10);
        assert!(width > 80.0);
        let mut out = Vec::new();
        write_ticks(
            &mut out,
            &tick,
            0.0,
            40.0,
            0.0,
            10,
            0.0,
            &crate::scheme::LIGHT,
        );
        let svg = String::from_utf8(out).unwrap();
        assert!(svg.matches("<text ").count() <= 4);
        assert!(svg.contains("0.000000000000000"));
        let mut number = String::new();
        append_number(&mut number, 0.000001);
        assert_eq!(number, "0.000001");
    }

    #[test]
    fn huge_tick_axes_have_a_bounded_label_count() {
        let tick = Tick::Series {
            offset: 1.0,
            step: 1.0,
            dp: 0,
            fixed: false,
        };
        let mut out = Vec::new();
        write_ticks(
            &mut out,
            &tick,
            0.0,
            40.0,
            0.0,
            1_000_000_000,
            2.0,
            &crate::scheme::LIGHT,
        );
        let svg = String::from_utf8(out).unwrap();
        let count = svg.matches("<text ").count();
        assert!(count > 0 && count <= 10_000);
    }

    #[test]
    fn annotation_geometry_is_bounded_by_the_viewport() {
        let mut out = Vec::new();
        draw_ou(
            &mut out,
            &"2".repeat(10_000),
            false,
            1.0,
            5_000.0,
            80.0,
            &crate::scheme::LIGHT,
        );
        assert!(out.len() < 1_000);
        out.clear();
        draw_gap_string(
            &mut out,
            &"| ".repeat(10_000),
            2,
            1,
            10_000,
            80.0,
            &crate::scheme::LIGHT,
        );
        assert!(out.len() < 1_000);
        out.clear();
        draw_gap_string(&mut out, "( ) [ ]", 0, 1, 0, 80.0, &crate::scheme::LIGHT);
        assert!(out.is_empty());
    }
}
