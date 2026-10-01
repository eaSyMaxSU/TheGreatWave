//! Pixel-grid SVG for a gate netlist. A wire and a box border are filled
//! rectangles on whole pixels. An inverted output is a filled square.

use crate::gtl::Netlist;
use crate::gtl_layout::{self, Bar, Caption, GateBox, Scene, Square};
use crate::scheme::Scheme;
use crate::w::{prettify, push_esc, push_i64};
use crate::Error;

const MARGIN: i64 = 8;

pub(crate) fn write(
    net: &Netlist,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let scene = gtl_layout::layout(net);
    let width = scene.width + MARGIN * 2;
    let height = scene.height + MARGIN * 2;
    out.extend_from_slice(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"tgw gtl\" role=\"img\" aria-labelledby=\"diagram-title\" width=\"",
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
        out.extend_from_slice(b"Gate netlist");
    }
    out.extend_from_slice(b"</title><desc>");
    push_i64(out, net.gates.len() as i64);
    out.extend_from_slice(if net.gates.len() == 1 {
        b" gate"
    } else {
        b" gates"
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
    for bar in &scene.bars {
        fill_bar(out, bar, scheme.ink);
    }
    for gate in &scene.gates {
        fill_rect(out, gate.x, gate.y, gate.w, gate.h, scheme.ink);
        fill_rect(
            out,
            gate.x + 1,
            gate.y + 1,
            gate.w - 2,
            gate.h - 2,
            scheme.paper,
        );
    }
    for square in &scene.squares {
        fill_square(out, square, scheme.ink);
    }
    for gate in &scene.gates {
        gate_text(out, gate);
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

fn gate_text(out: &mut Vec<u8>, gate: &GateBox) {
    text(
        out,
        gate.x + gate.w / 2,
        gate.y + gate.h / 2 + 4,
        "middle",
        12,
        None,
        gate.label,
    );
}

fn caption_text(out: &mut Vec<u8>, caption: &Caption, fill: &str) {
    text(
        out,
        caption.x,
        caption.y,
        caption.anchor,
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

fn fill_bar(out: &mut Vec<u8>, bar: &Bar, fill: &str) {
    fill_rect(out, bar.x, bar.y, bar.w, bar.h, fill);
}

fn fill_square(out: &mut Vec<u8>, square: &Square, fill: &str) {
    fill_rect(out, square.x, square.y, square.size, square.size, fill);
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
