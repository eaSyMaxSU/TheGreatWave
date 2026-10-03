//! SVG for a gate netlist: distinctive-shape gate symbols with inversion
//! bubbles, rounded wires with junction dots, input and output pills, and the
//! name of every internal net on its wire.

use crate::draw::{self, Text};
use crate::gtl::{Kind, Netlist};
use crate::gtl_layout::{self, Pill, Role, Scene, Symbol, BUBBLE, PILL_H};
use crate::scheme::Scheme;
use crate::w::push_i64;
use crate::Error;

const MARGIN: i64 = 8;

pub(crate) fn write(
    net: &Netlist,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let scene = gtl_layout::layout(net);
    let gates = net.gates.len();
    let desc = format!("{gates} {}", if gates == 1 { "gate" } else { "gates" });
    draw::begin(
        out,
        "tgw gtl",
        scene.width + MARGIN * 2,
        scene.height + MARGIN * 2,
        MARGIN,
        (scene.width, scene.height),
        scene.title.as_deref().unwrap_or("Gate netlist"),
        &desc,
        scheme,
    );
    paint(&scene, out, scheme);
    draw::finish(out, indent);
    Ok(())
}

fn paint(scene: &Scene, out: &mut Vec<u8>, scheme: &Scheme) {
    for path in &scene.wires {
        draw::wire(out, path, 5, scheme.wire);
    }
    for &(x, y) in &scene.dots {
        draw::circle(out, x, y, 3, scheme.wire);
    }
    for gate in &scene.gates {
        draw::outline_shadow(
            out,
            &shape(gate.kind, gate.x, gate.y + 2, gate.w, gate.h),
            scheme,
        );
    }
    for gate in &scene.gates {
        symbol(out, gate, scheme);
    }
    for pill in &scene.pills {
        tag(out, pill, scheme);
    }
    for label in &scene.nets {
        draw::text(
            out,
            Text {
                x: label.x,
                y: label.y,
                anchor: "start",
                size: 10,
                weight: Some(500),
                fill: Some(scheme.muted),
            },
            &label.text,
        );
    }
    if let Some(title) = &scene.title {
        draw::heading(out, title, scene.title_at, 15, None);
    }
    if let Some(footer) = &scene.footer {
        draw::heading(out, footer, scene.footer_at, 12, Some(scheme.muted));
    }
}

fn symbol(out: &mut Vec<u8>, gate: &Symbol, scheme: &Scheme) {
    let (x, y, w, h) = (gate.x, gate.y, gate.w, gate.h);
    draw::outline(
        out,
        &shape(gate.kind, x, y, w, h),
        scheme.gate_fill,
        scheme.gate_ink,
    );
    if matches!(gate.kind, Kind::Xor | Kind::Xnor) {
        let mut d = Vec::new();
        d.push(b'M');
        push_pair(&mut d, x - 1, y + h);
        d.push(b'Q');
        push_pair(&mut d, x - 1 + (w - 8) * 3 / 10, y + h / 2);
        d.push(b' ');
        push_pair(&mut d, x - 1, y);
        draw::outline(
            out,
            std::str::from_utf8(&d).unwrap(),
            "none",
            scheme.gate_ink,
        );
    }
    if gate.kind.invert() {
        draw::ring(
            out,
            x + w + BUBBLE,
            y + h / 2,
            BUBBLE,
            scheme.paper,
            scheme.gate_ink,
        );
    }
    let center = match gate.kind {
        Kind::Not => x + w * 2 / 5,
        Kind::Mux => x + w / 2 + 2,
        Kind::Xor | Kind::Xnor => x + 8 + (w - 8) / 2,
        Kind::Or | Kind::Nor => x + w / 2,
        _ => x + w * 9 / 20,
    };
    draw::text(
        out,
        Text {
            x: center,
            y: y + h / 2 + 4,
            anchor: "middle",
            size: 10,
            weight: Some(700),
            fill: Some(scheme.gate_ink),
        },
        gate.label,
    );
    for (bit, &row) in gate.data.iter().enumerate() {
        draw::text(
            out,
            Text {
                x: x + 6,
                y: row + 3,
                anchor: "start",
                size: 9,
                weight: Some(600),
                fill: Some(scheme.tick),
            },
            if bit == 0 { "0" } else { "1" },
        );
    }
}

/// The distinctive outline of each gate inside its `w` by `h` box.
pub(crate) fn shape(kind: Kind, x: i64, y: i64, w: i64, h: i64) -> String {
    let mut d = Vec::new();
    let mid = y + h / 2;
    match kind {
        Kind::And | Kind::Nand => {
            let r = h / 2;
            d.push(b'M');
            push_pair(&mut d, x, y);
            d.push(b'H');
            push_i64(&mut d, x + w - r);
            d.extend_from_slice(b"A");
            push_pair(&mut d, r, r);
            d.extend_from_slice(b" 0 0 1 ");
            push_pair(&mut d, x + w - r, y + h);
            d.push(b'H');
            push_i64(&mut d, x);
            d.push(b'Z');
        }
        Kind::Or | Kind::Nor | Kind::Xor | Kind::Xnor => {
            let left = if matches!(kind, Kind::Xor | Kind::Xnor) {
                x + 8
            } else {
                x
            };
            let span = x + w - left;
            d.push(b'M');
            push_pair(&mut d, left, y);
            d.push(b'Q');
            push_pair(&mut d, left + span * 3 / 5, y);
            d.push(b' ');
            push_pair(&mut d, x + w, mid);
            d.push(b'Q');
            push_pair(&mut d, left + span * 3 / 5, y + h);
            d.push(b' ');
            push_pair(&mut d, left, y + h);
            d.push(b'Q');
            push_pair(&mut d, left + span * 3 / 10, mid);
            d.push(b' ');
            push_pair(&mut d, left, y);
            d.push(b'Z');
        }
        Kind::Not => {
            d.push(b'M');
            push_pair(&mut d, x, y);
            d.push(b'L');
            push_pair(&mut d, x + w, mid);
            d.push(b'L');
            push_pair(&mut d, x, y + h);
            d.push(b'Z');
        }
        Kind::Mux => {
            let slant = h / 5;
            d.push(b'M');
            push_pair(&mut d, x, y);
            d.push(b'L');
            push_pair(&mut d, x + w, y + slant);
            d.push(b'L');
            push_pair(&mut d, x + w, y + h - slant);
            d.push(b'L');
            push_pair(&mut d, x, y + h);
            d.push(b'Z');
        }
    }
    String::from_utf8(d).unwrap()
}

fn push_pair(d: &mut Vec<u8>, x: i64, y: i64) {
    push_i64(d, x);
    d.push(b' ');
    push_i64(d, y);
}

fn tag(out: &mut Vec<u8>, pill: &Pill, scheme: &Scheme) {
    let (ink, fill) = match pill.role {
        Role::Input => (scheme.in_ink, scheme.in_fill),
        Role::Const => (scheme.tick, scheme.hatch_fill),
        Role::Output => (scheme.out_ink, scheme.out_fill),
    };
    draw::framed(
        out,
        pill.x,
        pill.y - PILL_H / 2,
        pill.w,
        PILL_H,
        PILL_H / 2,
        ink,
        fill,
    );
    draw::text(
        out,
        Text {
            x: pill.x + pill.w / 2,
            y: pill.y + 4,
            anchor: "middle",
            size: 11,
            weight: Some(650),
            fill: Some(ink),
        },
        &pill.text,
    );
}
