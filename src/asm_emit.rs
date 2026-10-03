//! SVG for an ASM chart: state cards with a coloured name header, amber
//! decision diamonds, rounded conditional-output pills, and rounded 2px wires
//! with a dot wherever paths into one state merge.

use crate::asm::Chart;
use crate::asm_layout::{self, Digit, Kind, Node, Seg, HEADER};
use crate::draw::{self, Text};
use crate::scheme::Scheme;
use crate::Error;

const MARGIN: i64 = 8;
const LINE: i64 = 16;

pub(crate) fn write(
    chart: &Chart,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let scene = asm_layout::layout(chart);
    let states = chart.states.len();
    let mut desc = format!("{states} {}", if states == 1 { "state" } else { "states" });
    for state in &chart.states {
        desc.push_str("; ");
        desc.push_str(&state.name);
    }
    draw::begin(
        out,
        "tgw asm",
        scene.width + MARGIN * 2,
        scene.height + MARGIN * 2,
        MARGIN,
        (scene.width, scene.height),
        scene.title.as_deref().unwrap_or("ASM chart"),
        &desc,
        scheme,
    );
    let paths = polylines(&scene.segs);
    for (points, arrow) in &paths {
        if *arrow {
            draw::wire(out, &trimmed(points), 6, scheme.wire);
        } else {
            draw::wire(out, points, 6, scheme.wire);
        }
    }
    let mut tips: Vec<(i64, i64)> = paths
        .iter()
        .filter(|(_, arrow)| *arrow)
        .map(|(points, _)| *points.last().unwrap())
        .collect();
    tips.sort_unstable();
    tips.dedup();
    for tip in &tips {
        let into: Vec<Vec<(i64, i64)>> = paths
            .iter()
            .filter(|(points, arrow)| *arrow && points.last() == Some(tip))
            .map(|(points, _)| points.clone())
            .collect();
        for (x, y) in draw::junctions(&into) {
            draw::circle(out, x, y, 3, scheme.wire);
        }
    }
    for node in &scene.nodes {
        shape(out, node, scheme);
    }
    for (points, arrow) in &paths {
        if *arrow {
            let n = points.len();
            draw::arrow(out, points[n - 2], points[n - 1], scheme.wire);
        }
    }
    for node in &scene.nodes {
        label(out, node, scheme);
    }
    for digit in &scene.digits {
        exit_digit(out, digit, scheme);
    }
    if let Some(title) = &scene.title {
        draw::heading(out, title, scene.title_at, 15, None);
    }
    if let Some(footer) = &scene.footer {
        draw::heading(out, footer, scene.footer_at, 12, Some(scheme.muted));
    }
    draw::finish(out, indent);
    Ok(())
}

/// Join touching segments into wires. A wire ends where an arrow lands.
fn polylines(segs: &[Seg]) -> Vec<(Vec<(i64, i64)>, bool)> {
    let mut paths: Vec<(Vec<(i64, i64)>, bool)> = Vec::new();
    for seg in segs {
        let (from, to) = ((seg.x0, seg.y0), (seg.x1, seg.y1));
        match paths.last_mut() {
            Some((points, false)) if points.last() == Some(&from) => {
                points.push(to);
                if seg.arrow {
                    paths.last_mut().unwrap().1 = true;
                }
            }
            _ => paths.push((vec![from, to], seg.arrow)),
        }
    }
    paths
}

/// Stop an arrowed wire under its arrowhead.
fn trimmed(points: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut points = points.to_vec();
    let n = points.len();
    if n >= 2 {
        let (a, b) = (points[n - 2], points[n - 1]);
        let back = 6.min((a.0 - b.0).abs() + (a.1 - b.1).abs());
        points[n - 1] = (
            b.0 - (b.0 - a.0).signum() * back,
            b.1 - (b.1 - a.1).signum() * back,
        );
    }
    points
}

fn shape(out: &mut Vec<u8>, node: &Node, scheme: &Scheme) {
    let (x, y, w, h) = (node.x, node.y, node.w, node.h);
    match node.kind {
        Kind::State => {
            draw::shadow(out, x, y, w, h, 6, scheme);
            if node.head >= node.lines.len() {
                draw::framed(out, x, y, w, h, 6, scheme.gate_ink, scheme.gate_ink);
            } else {
                draw::framed(out, x, y, w, h, 6, scheme.gate_ink, scheme.gate_fill);
                let header = HEADER + (node.head.max(1) as i64 - 1) * LINE;
                draw::rect(out, x + 1, y + 1, w - 2, header - 1, 5, scheme.gate_ink);
                draw::rect(out, x + 1, y + header - 6, w - 2, 6, 0, scheme.gate_ink);
            }
        }
        Kind::Cond => {
            let rx = (h / 2).min(14);
            draw::shadow(out, x, y, w, h, rx, scheme);
            draw::framed(out, x, y, w, h, rx, scheme.in_ink, scheme.in_fill);
        }
        Kind::Diamond => {
            let cx = x + w / 2;
            let cy = y + h / 2;
            let shadow = [(cx, y + 2), (x + w, cy + 2), (cx, y + h + 2), (x, cy + 2)];
            out.extend_from_slice(b"<polygon points=\"");
            for (index, (px, py)) in shadow.iter().enumerate() {
                if index > 0 {
                    out.push(b' ');
                }
                out.extend_from_slice(format!("{px},{py}").as_bytes());
            }
            out.extend_from_slice(b"\" fill=\"");
            out.extend_from_slice(scheme.shadow.as_bytes());
            out.extend_from_slice(b"\" fill-opacity=\"0.12\"/>");
            draw::polygon(
                out,
                &[(cx, y), (x + w, cy), (cx, y + h), (x, cy)],
                scheme.div_ink,
            );
            draw::polygon(
                out,
                &[(cx, y + 2), (x + w - 3, cy), (cx, y + h - 2), (x + 3, cy)],
                scheme.div_fill,
            );
        }
    }
}

fn label(out: &mut Vec<u8>, node: &Node, scheme: &Scheme) {
    if node.lines.is_empty() {
        return;
    }
    let x = node.x + node.w / 2;
    let line = |out: &mut Vec<u8>, y: i64, weight: Option<u16>, fill: Option<&str>, value: &str| {
        draw::text(
            out,
            Text {
                x,
                y,
                anchor: "middle",
                size: 12,
                weight,
                fill,
            },
            value,
        );
    };
    match node.kind {
        Kind::State => {
            for (index, value) in node.lines.iter().enumerate() {
                if index < node.head {
                    let y = node.y + 16 + index as i64 * LINE;
                    line(out, y, Some(700), Some(scheme.paper), value);
                } else {
                    let header = HEADER + (node.head.max(1) as i64 - 1) * LINE;
                    let body = node.h - header;
                    let count = (node.lines.len() - node.head) as i64;
                    let top = node.y + header + (body - count * LINE) / 2;
                    let y = top + 12 + (index - node.head) as i64 * LINE;
                    line(out, y, None, None, value);
                }
            }
        }
        Kind::Cond | Kind::Diamond => {
            let count = node.lines.len() as i64;
            let pad = (node.h - count * LINE) / 2;
            let (weight, fill) = if node.kind == Kind::Cond {
                (Some(600), Some(scheme.in_ink))
            } else {
                (Some(600), None)
            };
            for (index, value) in node.lines.iter().enumerate() {
                line(
                    out,
                    node.y + pad + 12 + index as i64 * LINE,
                    weight,
                    fill,
                    value,
                );
            }
        }
    }
}

fn exit_digit(out: &mut Vec<u8>, digit: &Digit, scheme: &Scheme) {
    draw::text(
        out,
        Text {
            x: digit.x,
            y: digit.y,
            anchor: if digit.end { "end" } else { "start" },
            size: 11,
            weight: Some(700),
            fill: Some(scheme.div_ink),
        },
        digit.text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheme;

    #[test]
    fn geometry_stays_on_integers_and_joins_the_next_state() {
        let source = "\
@asm
@title Bus handshake

idle:
  req=0
  ? start
    0 idle
    1 (req=1) wait

wait:
  > idle
";
        let mut out = Vec::new();
        write(
            &crate::asm::parse(source).unwrap(),
            &mut out,
            0,
            &scheme::LIGHT,
        )
        .unwrap();
        let svg = String::from_utf8(out).unwrap();
        assert!(svg.contains("class=\"tgw asm\""));
        assert!(svg.contains(">req=1<"));
        assert!(svg.contains(">idle<"));
        assert!(svg.contains("<polygon "));
        assert!(!svg.contains("<g><rect"));
        for token in svg.split("<rect ").skip(1) {
            let tag = token.split('>').next().unwrap();
            for key in ["x=\"", "y=\"", "width=\"", "height=\""] {
                if let Some(value) = tag
                    .split(key)
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
                {
                    assert!(value.parse::<i64>().is_ok(), "non-integer {key}{value}");
                }
            }
        }
        for token in svg.split("<polygon ").skip(1) {
            let tag = token.split('>').next().unwrap();
            let points = tag
                .split("points=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            for number in points.split([',', ' ']) {
                assert!(number.parse::<i64>().is_ok(), "non-integer point {number}");
            }
        }
    }

    #[test]
    fn segments_join_into_wires_that_end_at_arrows() {
        let segs = [
            Seg {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 10,
                arrow: false,
            },
            Seg {
                x0: 0,
                y0: 10,
                x1: 20,
                y1: 10,
                arrow: true,
            },
            Seg {
                x0: 20,
                y0: 10,
                x1: 20,
                y1: 30,
                arrow: true,
            },
        ];
        let paths = polylines(&segs);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], (vec![(0, 0), (0, 10), (20, 10)], true));
        assert_eq!(trimmed(&paths[0].0), vec![(0, 0), (0, 10), (14, 10)]);
    }
}
