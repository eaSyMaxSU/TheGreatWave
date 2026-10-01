//! Pixel-grid SVG for an ASM chart. Borders are filled rings, so a 1px edge
//! stays on whole pixels instead of straddling a centered stroke.

use crate::asm::Chart;
use crate::asm_layout::{self, Digit, Kind, Node, Seg};
use crate::scheme::Scheme;
use crate::w::{prettify, push_esc, push_i64};
use crate::Error;

const MARGIN: i64 = 8;

pub(crate) fn write(
    chart: &Chart,
    out: &mut Vec<u8>,
    indent: u8,
    scheme: &'static Scheme,
) -> Result<(), Error> {
    let scene = asm_layout::layout(chart);
    let width = scene.width + MARGIN * 2;
    let height = scene.height + MARGIN * 2;
    out.extend_from_slice(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"tgw asm\" role=\"img\" aria-labelledby=\"diagram-title\" width=\"",
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
        out.extend_from_slice(b"ASM chart");
    }
    out.extend_from_slice(b"</title><desc>");
    push_i64(out, chart.states.len() as i64);
    out.extend_from_slice(if chart.states.len() == 1 {
        b" state"
    } else {
        b" states"
    });
    for state in &chart.states {
        out.extend_from_slice(b"; ");
        push_esc(out, &state.name);
    }
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
    for seg in &scene.segs {
        if !seg.arrow {
            draw_bar(out, seg, scheme.ink);
        }
    }
    for node in &scene.nodes {
        draw_node(out, node, scheme);
    }
    for seg in &scene.segs {
        if seg.arrow {
            draw_arrow(out, seg, scheme.ink);
        }
    }
    for node in &scene.nodes {
        draw_node_text(out, node);
    }
    for digit in &scene.digits {
        draw_digit(out, digit, scheme.muted);
    }
    if let Some(title) = &scene.title {
        draw_caption(out, title, scene.title_at);
    }
    if let Some(footer) = &scene.footer {
        draw_caption(out, footer, scene.footer_at);
    }
    out.extend_from_slice(b"</g></svg>");
    crate::emit::scope_ids(out);
    if indent > 0 {
        let pretty = prettify(out, indent);
        out.clear();
        out.extend_from_slice(&pretty);
    }
    Ok(())
}

fn draw_node(out: &mut Vec<u8>, node: &Node, scheme: &Scheme) {
    match node.kind {
        Kind::State | Kind::Cond => {
            fill_rect(out, node.x, node.y, node.w, node.h, scheme.ink);
            fill_rect(
                out,
                node.x + 1,
                node.y + 1,
                node.w - 2,
                node.h - 2,
                scheme.paper,
            );
        }
        Kind::Diamond => {
            let cx = node.x + node.w / 2;
            let cy = node.y + node.h / 2;
            polygon(
                out,
                &[
                    (cx, node.y),
                    (node.x + node.w, cy),
                    (cx, node.y + node.h),
                    (node.x, cy),
                ],
                scheme.ink,
            );
            polygon(
                out,
                &[
                    (cx, node.y + 1),
                    (node.x + node.w - 1, cy),
                    (cx, node.y + node.h - 1),
                    (node.x + 1, cy),
                ],
                scheme.paper,
            );
        }
    }
}

fn draw_node_text(out: &mut Vec<u8>, node: &Node) {
    if node.lines.is_empty() {
        return;
    }
    let count = node.lines.len() as i64;
    let pad = (node.h - count * 16) / 2;
    let x = node.x + node.w / 2;
    for (index, line) in node.lines.iter().enumerate() {
        let y = node.y + pad + 12 + index as i64 * 16;
        text(out, x, y, TextStyle::plain("middle", 12), line);
    }
}

fn draw_digit(out: &mut Vec<u8>, digit: &Digit, fill: &str) {
    text(
        out,
        digit.x,
        digit.y,
        TextStyle {
            anchor: if digit.end { "end" } else { "start" },
            size: 11,
            weight: None,
            fill: Some(fill),
        },
        digit.text,
    );
}

fn draw_caption(out: &mut Vec<u8>, caption: &str, at: (i64, i64)) {
    text(
        out,
        at.0,
        at.1,
        TextStyle {
            anchor: "middle",
            size: 14,
            weight: Some(600),
            fill: None,
        },
        caption,
    );
}

fn draw_bar(out: &mut Vec<u8>, seg: &Seg, ink: &str) {
    bar(out, seg.x0, seg.y0, seg.x1, seg.y1, ink);
}

fn draw_arrow(out: &mut Vec<u8>, seg: &Seg, ink: &str) {
    let (x1, y1) = pull_back(seg, 6);
    if (x1, y1) != (seg.x0, seg.y0) {
        bar(out, seg.x0, seg.y0, x1, y1, ink);
    }
    let tip = (seg.x1, seg.y1);
    let points: [(i64, i64); 3] = if seg.x0 == seg.x1 {
        if seg.y1 >= seg.y0 {
            [
                (tip.0, tip.1),
                (tip.0 - 3, tip.1 - 6),
                (tip.0 + 3, tip.1 - 6),
            ]
        } else {
            [
                (tip.0, tip.1),
                (tip.0 - 3, tip.1 + 6),
                (tip.0 + 3, tip.1 + 6),
            ]
        }
    } else if seg.x1 >= seg.x0 {
        [
            (tip.0, tip.1),
            (tip.0 - 6, tip.1 - 3),
            (tip.0 - 6, tip.1 + 3),
        ]
    } else {
        [
            (tip.0, tip.1),
            (tip.0 + 6, tip.1 - 3),
            (tip.0 + 6, tip.1 + 3),
        ]
    };
    polygon(out, &points, ink);
}

fn pull_back(seg: &Seg, by: i64) -> (i64, i64) {
    if seg.x0 == seg.x1 {
        if seg.y1 >= seg.y0 {
            (seg.x1, seg.y0.max(seg.y1 - by))
        } else {
            (seg.x1, seg.y0.min(seg.y1 + by))
        }
    } else if seg.y0 == seg.y1 {
        if seg.x1 >= seg.x0 {
            (seg.x0.max(seg.x1 - by), seg.y1)
        } else {
            (seg.x0.min(seg.x1 + by), seg.y1)
        }
    } else {
        (seg.x1, seg.y1)
    }
}

fn bar(out: &mut Vec<u8>, x0: i64, y0: i64, x1: i64, y1: i64, ink: &str) {
    if x0 == x1 {
        let y = y0.min(y1);
        fill_rect(out, x0, y, 1, (y0 - y1).abs() + 1, ink);
    } else if y0 == y1 {
        let x = x0.min(x1);
        fill_rect(out, x, y0, (x0 - x1).abs() + 1, 1, ink);
    }
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

fn polygon(out: &mut Vec<u8>, points: &[(i64, i64)], fill: &str) {
    out.extend_from_slice(b"<polygon points=\"");
    for (index, (x, y)) in points.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        push_i64(out, *x);
        out.push(b',');
        push_i64(out, *y);
    }
    out.extend_from_slice(b"\" fill=\"");
    out.extend_from_slice(fill.as_bytes());
    out.extend_from_slice(b"\"/>");
}

struct TextStyle<'a> {
    anchor: &'a str,
    size: i64,
    weight: Option<u16>,
    fill: Option<&'a str>,
}

impl<'a> TextStyle<'a> {
    fn plain(anchor: &'a str, size: i64) -> Self {
        Self {
            anchor,
            size,
            weight: None,
            fill: None,
        }
    }
}

fn text(out: &mut Vec<u8>, x: i64, y: i64, style: TextStyle<'_>, value: &str) {
    out.extend_from_slice(b"<text x=\"");
    push_i64(out, x);
    out.extend_from_slice(b"\" y=\"");
    push_i64(out, y);
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
}
