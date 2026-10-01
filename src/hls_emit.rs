//! Pixel-grid SVG for an HLS schedule. A cycle rule, a wire, and a box
//! border are filled rectangles on whole pixels.

use crate::hls::Schedule;
use crate::hls_layout::{self, Bar, Caption, Scene, Span, Unit};
use crate::scheme::Scheme;
use crate::w::{prettify, push_esc, push_i64};
use crate::Error;

const MARGIN: i64 = 8;

pub(crate) fn write(
    schedule: &Schedule,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let scene = hls_layout::layout(schedule);
    let width = scene.width + MARGIN * 2;
    let height = scene.height + MARGIN * 2;
    out.extend_from_slice(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"tgw hls\" role=\"img\" aria-labelledby=\"diagram-title\" width=\"",
    );
    push_i64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" viewBox=\"0 0 ");
    push_i64(out, width);
    out.push(b' ');
    push_i64(out, height);
    out.extend_from_slice(
        b"\" overflow=\"hidden\" font-family=\"Inter, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif\" font-size=\"12\" fill=\"",
    );
    out.extend_from_slice(scheme.text.as_bytes());
    out.extend_from_slice(b"\" stroke=\"none\"><title id=\"diagram-title\">");
    if let Some(title) = &scene.title {
        push_esc(out, title);
    } else {
        out.extend_from_slice(b"HLS schedule");
    }
    out.extend_from_slice(b"</title><desc>");
    push_i64(out, i64::from(schedule.cycle_count));
    out.extend_from_slice(if schedule.cycle_count == 1 {
        b" cycle"
    } else {
        b" cycles"
    });
    out.extend_from_slice(b", ");
    push_i64(out, schedule.ops.len() as i64);
    out.extend_from_slice(if schedule.ops.len() == 1 {
        b" operator"
    } else {
        b" operators"
    });
    out.extend_from_slice(
        b"</desc><defs><clipPath id=\"plot-clip\"><rect x=\"0\" y=\"0\" width=\"",
    );
    push_i64(out, scene.width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, scene.height);
    out.extend_from_slice(b"\"/></clipPath></defs><rect width=\"");
    push_i64(out, width);
    out.extend_from_slice(b"\" height=\"");
    push_i64(out, height);
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(scheme.paper.as_bytes());
    out.extend_from_slice(b"\"/><g transform=\"translate(");
    push_i64(out, MARGIN);
    out.push(b',');
    push_i64(out, MARGIN);
    out.extend_from_slice(b")\">");
    paint(&scene, out, scheme);
    out.extend_from_slice(b"</g></svg>");
    crate::emit::scope_ids(out);
    if indent > 0 {
        let pretty = prettify(out, indent);
        out.clear();
        out.extend_from_slice(&pretty);
    }
    Ok(())
}

fn paint(scene: &Scene, out: &mut Vec<u8>, scheme: &Scheme) {
    for rule in &scene.rules {
        fill_span(out, rule, scheme.ink);
    }
    for bar in &scene.bars {
        fill_bar(out, bar, scheme.ink);
    }
    for unit in &scene.units {
        fill_rect(out, unit.x, unit.y, unit.w, unit.h, scheme.ink);
        fill_rect(
            out,
            unit.x + 1,
            unit.y + 1,
            unit.w - 2,
            unit.h - 2,
            scheme.paper,
        );
    }
    for unit in &scene.units {
        unit_text(out, unit);
    }
    for caption in &scene.captions {
        caption_text(out, caption, scheme.muted);
    }
    if let Some(title) = &scene.title {
        heading(out, title, scene.title_at);
    }
    if let Some(footer) = &scene.footer {
        heading(out, footer, scene.footer_at);
    }
}

fn unit_text(out: &mut Vec<u8>, unit: &Unit) {
    text(
        out,
        unit.x + unit.w / 2,
        unit.y + 20,
        "middle",
        12,
        None,
        unit.text.as_str(),
    );
}

fn caption_text(out: &mut Vec<u8>, caption: &Caption, fill: &str) {
    text(
        out,
        caption.x,
        caption.y,
        "start",
        11,
        Some(fill),
        &caption.text,
    );
}

fn heading(out: &mut Vec<u8>, value: &str, at: (i64, i64)) {
    out.extend_from_slice(b"<text x=\"");
    push_i64(out, at.0);
    out.extend_from_slice(b"\" y=\"");
    push_i64(out, at.1);
    out.extend_from_slice(
        b"\" text-anchor=\"middle\" font-size=\"14\" font-weight=\"600\" xml:space=\"preserve\">",
    );
    push_esc(out, value);
    out.extend_from_slice(b"</text>");
}

fn text(
    out: &mut Vec<u8>,
    x: i64,
    y: i64,
    anchor: &str,
    size: i64,
    fill: Option<&str>,
    value: &str,
) {
    out.extend_from_slice(b"<text x=\"");
    push_i64(out, x);
    out.extend_from_slice(b"\" y=\"");
    push_i64(out, y);
    out.extend_from_slice(b"\" text-anchor=\"");
    out.extend_from_slice(anchor.as_bytes());
    out.extend_from_slice(b"\" font-size=\"");
    push_i64(out, size);
    out.push(b'"');
    if let Some(fill) = fill {
        out.extend_from_slice(b" fill=\"");
        out.extend_from_slice(fill.as_bytes());
        out.push(b'"');
    }
    out.extend_from_slice(b" xml:space=\"preserve\">");
    push_esc(out, value);
    out.extend_from_slice(b"</text>");
}

fn fill_span(out: &mut Vec<u8>, span: &Span, fill: &str) {
    fill_rect(out, span.x, span.y, span.w, 1, fill);
}

fn fill_bar(out: &mut Vec<u8>, bar: &Bar, fill: &str) {
    fill_rect(out, bar.x, bar.y, bar.w, bar.h, fill);
}

fn fill_rect(out: &mut Vec<u8>, x: i64, y: i64, w: i64, h: i64, fill: &str) {
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
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\"/>");
}
