//! SVG for an HLS schedule: zebra cycle bands, colour-coded operator cards,
//! rounded wires that cross each clock edge through a register mark, and a
//! usage column that counts busy units per cycle.

use crate::draw::{self, measure, Text};
use crate::hls::{Kind, Schedule};
use crate::hls_layout::{self, symbol, Scene, Unit, ARROW, BADGE, BOX_H, CELL_H, EXPR_X, SPACE};
use crate::scheme::Scheme;
use crate::Error;

const MARGIN: i64 = 8;

pub(crate) fn write(
    schedule: &Schedule,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let scene = hls_layout::layout(schedule);
    let cycles = schedule.cycle_count;
    let ops = schedule.ops.len();
    let desc = format!(
        "{cycles} {}, {ops} {}",
        if cycles == 1 { "cycle" } else { "cycles" },
        if ops == 1 { "operator" } else { "operators" }
    );
    draw::begin(
        out,
        "tgw hls",
        scene.width + MARGIN * 2,
        scene.height + MARGIN * 2,
        MARGIN,
        (scene.width, scene.height),
        scene.title.as_deref().unwrap_or("HLS schedule"),
        &desc,
        scheme,
    );
    paint(&scene, out, scheme);
    draw::finish(out, indent);
    Ok(())
}

pub(crate) fn ink(kind: Kind, scheme: &Scheme) -> &'static str {
    match kind {
        Kind::Add => scheme.add_ink,
        Kind::Mul => scheme.mul_ink,
        Kind::Div => scheme.div_ink,
    }
}

pub(crate) fn fill(kind: Kind, scheme: &Scheme) -> &'static str {
    match kind {
        Kind::Add => scheme.add_fill,
        Kind::Mul => scheme.mul_fill,
        Kind::Div => scheme.div_fill,
    }
}

const KINDS: [Kind; 3] = [Kind::Add, Kind::Mul, Kind::Div];

fn paint(scene: &Scene, out: &mut Vec<u8>, scheme: &Scheme) {
    let width = scene.width;
    for band in &scene.bands {
        if band.cycle % 2 == 1 {
            draw::rect(out, 0, band.y, width, band.h, 0, scheme.band);
        }
    }
    if let (Some(first), Some(last)) = (scene.bands.first(), scene.bands.last()) {
        let bottom = last.y + last.h;
        draw::rect(
            out,
            scene.usage_x - 12,
            first.y,
            1,
            bottom - first.y,
            0,
            scheme.rule,
        );
        for band in &scene.bands {
            draw::rect(out, 0, band.y, width, 1, 0, scheme.rule);
        }
        draw::rect(out, 0, bottom, width, 1, 0, scheme.rule);
    }
    for band in &scene.bands {
        cycle_badge(out, band.cycle, band.row, scheme);
        for (index, kind) in KINDS.into_iter().enumerate() {
            let count = band.counts[index];
            let x = scene.usage_x + index as i64 * (scene.cell_w + 4);
            let label = format!("{}{count}", symbol(kind));
            let center = x + scene.cell_w / 2;
            if count > 0 {
                draw::framed(
                    out,
                    x,
                    band.row - CELL_H / 2,
                    scene.cell_w,
                    CELL_H,
                    CELL_H / 2,
                    ink(kind, scheme),
                    fill(kind, scheme),
                );
                small(out, center, band.row + 4, 650, ink(kind, scheme), &label);
            } else {
                small(out, center, band.row + 4, 500, scheme.quiet, &label);
            }
        }
    }
    for wire in &scene.wires {
        draw::wire(out, &wire.points, 5, ink(wire.kind, scheme));
    }
    for tag in &scene.tags {
        draw::wire(
            out,
            &[(tag.x, tag.y - 2), (tag.x, tag.y + 7)],
            0,
            ink(tag.kind, scheme),
        );
    }
    for mark in &scene.registers {
        draw::framed(
            out,
            mark.x - 4,
            mark.y - 3,
            8,
            8,
            2,
            ink(mark.kind, scheme),
            scheme.paper,
        );
    }
    for dot in &scene.dots {
        draw::circle(out, dot.x, dot.y, 3, ink(dot.kind, scheme));
    }
    for unit in &scene.units {
        card(out, unit, scheme);
    }
    for wire in &scene.wires {
        draw::arrow_down(out, wire.tip.0, wire.tip.1, ink(wire.kind, scheme));
    }
    for tag in &scene.tags {
        let (ink, fill) = (ink(tag.kind, scheme), fill(tag.kind, scheme));
        draw::arrow_down(out, tag.x, tag.y + 12, ink);
        draw::framed(out, tag.x - tag.w / 2, tag.y + 13, tag.w, 18, 9, ink, fill);
        small(out, tag.x, tag.y + 26, 650, ink, &tag.text);
    }
    if let Some(title) = &scene.title {
        draw::heading(out, title, scene.title_at, 15, None);
    }
    if let Some(footer) = &scene.footer {
        draw::heading(out, footer, scene.footer_at, 12, Some(scheme.muted));
    }
}

fn cycle_badge(out: &mut Vec<u8>, cycle: u32, row: i64, scheme: &Scheme) {
    draw::framed(out, 6, row - 11, 32, 22, 11, scheme.rule, scheme.paper);
    small(out, 22, row + 4, 650, scheme.muted, &cycle.to_string());
}

fn card(out: &mut Vec<u8>, unit: &Unit, scheme: &Scheme) {
    let (ink, fill) = (ink(unit.kind, scheme), fill(unit.kind, scheme));
    draw::shadow(out, unit.x, unit.y, unit.w, unit.h, 8, scheme);
    draw::framed(out, unit.x, unit.y, unit.w, unit.h, 8, ink, fill);
    for &stage in &unit.stages {
        out.extend_from_slice(
            format!(
                "<path d=\"M{} {}.5H{}\" fill=\"none\" stroke=\"{ink}\" stroke-opacity=\"0.45\" stroke-dasharray=\"4 4\"/>",
                unit.x + 10,
                stage,
                unit.x + unit.w - 10
            )
            .as_bytes(),
        );
    }
    let middle = unit.y + BOX_H / 2;
    draw::circle(out, unit.x + 8 + BADGE, middle, BADGE, ink);
    draw::text(
        out,
        Text {
            x: unit.x + 8 + BADGE,
            y: middle + 5,
            anchor: "middle",
            size: 15,
            weight: Some(700),
            fill: Some(scheme.paper),
        },
        symbol(unit.kind),
    );
    let mut x = unit.x + EXPR_X;
    draw::text(
        out,
        Text {
            x,
            y: middle + 4,
            anchor: "start",
            size: 12,
            weight: None,
            fill: None,
        },
        &unit.expr,
    );
    x += measure(&unit.expr, 12.0, false) + SPACE;
    draw::text(
        out,
        Text {
            x,
            y: middle + 4,
            anchor: "start",
            size: 12,
            weight: None,
            fill: Some(scheme.tick),
        },
        "\u{2192}",
    );
    x += ARROW;
    draw::text(
        out,
        Text {
            x,
            y: middle + 4,
            anchor: "start",
            size: 12,
            weight: Some(700),
            fill: Some(ink),
        },
        &unit.result,
    );
    if unit.latency > 1 {
        draw::text(
            out,
            Text {
                x: unit.x + unit.w - 12,
                y: unit.y + unit.h - 12,
                anchor: "end",
                size: 10,
                weight: Some(600),
                fill: Some(ink),
            },
            &format!("{} cycles", unit.latency),
        );
    }
}

fn small(out: &mut Vec<u8>, x: i64, y: i64, weight: u16, fill: &str, value: &str) {
    draw::text(
        out,
        Text {
            x,
            y,
            anchor: "middle",
            size: 11,
            weight: Some(weight),
            fill: Some(fill),
        },
        value,
    );
}
