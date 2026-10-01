//! One left-to-right walk that places gate boxes and the wires between them.
//!
//! Every coordinate is an integer. A wire is a run of 1px rectangles in the
//! gap, so the paper inside a box stays paper. Columns come from the parser:
//! `column = 1 + max driver column`, and primary inputs are not gates.

use crate::gtl::{operand_name, Gate, Netlist, Operand};
use crate::text_width;
use std::collections::HashMap;

const TEXT: f64 = 12.0;
const CAPTION: f64 = 11.0;
const PAD: i64 = 8;
const ROW: i64 = 20;
const TRACK: i64 = 4;
const STUB: i64 = 12;
const INVERT: i64 = 6;
const MIN_W: i64 = 48;
const MIN_H: i64 = 40;

#[derive(Clone, Debug)]
pub(crate) struct Scene {
    pub width: i64,
    pub height: i64,
    pub title: Option<String>,
    pub title_at: (i64, i64),
    pub footer: Option<String>,
    pub footer_at: (i64, i64),
    pub gates: Vec<GateBox>,
    pub squares: Vec<Square>,
    pub bars: Vec<Bar>,
    pub captions: Vec<Caption>,
}

#[derive(Clone, Debug)]
pub(crate) struct GateBox {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub label: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct Square {
    pub x: i64,
    pub y: i64,
    pub size: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Bar {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Caption {
    pub text: String,
    pub x: i64,
    pub y: i64,
    pub anchor: &'static str,
}

struct Draft {
    index: usize,
    w: i64,
    h: i64,
    label: &'static str,
    invert: bool,
    in_off: Vec<i64>,
    out_off: i64,
    names: Vec<Option<String>>,
    result: String,
    live: bool,
}

struct Link {
    from: usize,
    to: usize,
    port: usize,
    from_col: usize,
    to_col: usize,
    /// Vertical track in the gap after the producer. A sky link also takes
    /// `down` in the gap before the consumer, and a private `lane` above the boxes.
    up: usize,
    down: usize,
    lane: usize,
    sky: bool,
}

struct Columns {
    x: Vec<i64>,
    box_w: Vec<i64>,
    invert_pad: Vec<i64>,
    live_w: Vec<i64>,
}

#[derive(Clone)]
struct Placed {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

pub(crate) fn layout(net: &Netlist) -> Scene {
    let head = if net.title.is_some() { 28 } else { 0 };
    let foot = if net.footer.is_some() { 28 } else { 0 };
    let writers = writers(net);
    let drafts = drafts(net, &writers);
    let max_col = drafts
        .iter()
        .map(|draft| net.gates[draft.index].column)
        .max()
        .unwrap_or(0);
    let mut cols: Vec<Vec<usize>> = vec![Vec::new(); max_col as usize + 1];
    for (index, draft) in drafts.iter().enumerate() {
        cols[net.gates[draft.index].column as usize].push(index);
    }
    let links = links(net, &writers);
    let mut gap_links: Vec<Vec<usize>> = vec![Vec::new(); max_col as usize + 1];
    let mut routed: Vec<Link> = Vec::with_capacity(links.len());
    let mut sky_count = 0usize;
    for (from, to, port) in links {
        let from_col = net.gates[from].column as usize;
        let to_col = net.gates[to].column as usize;
        let sky = to_col > from_col + 1;
        let up = gap_links[from_col].len();
        gap_links[from_col].push(routed.len());
        let down = if sky {
            let down = gap_links[to_col - 1].len();
            gap_links[to_col - 1].push(routed.len());
            down
        } else {
            up
        };
        let lane = if sky {
            let lane = sky_count;
            sky_count += 1;
            lane
        } else {
            0
        };
        routed.push(Link {
            from,
            to,
            port,
            from_col,
            to_col,
            up,
            down,
            lane,
            sky,
        });
    }
    let sky_lanes = sky_count as i64;
    let name_w = name_widths(&drafts, max_col, net);
    let live_w = live_widths(&drafts, max_col, net);
    let mut box_w = vec![0i64; max_col as usize + 1];
    let mut invert_pad = vec![0i64; max_col as usize + 1];
    for col in 1..=max_col {
        let c = col as usize;
        box_w[c] = cols[c]
            .iter()
            .map(|&index| drafts[index].w)
            .max()
            .unwrap_or(MIN_W);
        if cols[c].iter().any(|&index| drafts[index].invert) {
            invert_pad[c] = INVERT - 1;
        }
    }
    let left = if max_col == 0 {
        8
    } else {
        name_w[1] + if name_w[1] > 0 { STUB + 16 } else { 8 }
    };
    let mut gap_w = vec![0i64; max_col as usize + 1];
    for col in 1..max_col {
        let c = col as usize;
        let tracks = gap_links[c].len() as i64;
        let track_zone = if tracks == 0 { 0 } else { 8 + tracks * TRACK };
        let names = name_w[c + 1];
        let labels = if names > 0 { 8 + names + STUB } else { 0 };
        let air = if tracks == 0 && names == 0 { 24 } else { 8 };
        gap_w[c] = track_zone + labels + air;
    }
    let mut col_x = vec![0i64; max_col as usize + 1];
    let mut x = left;
    for col in 1..=max_col {
        let c = col as usize;
        col_x[c] = x;
        let right_pad = if live_w[c] > 0 {
            STUB + 8 + live_w[c]
        } else {
            0
        };
        x += box_w[c] + invert_pad[c] + right_pad;
        if col < max_col {
            x += gap_w[c];
        }
    }
    let top = head + sky_lanes * TRACK + 8;
    let mut placed = vec![
        Placed {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
        drafts.len()
    ];
    let mut stack = top;
    for col in 1..=max_col {
        let mut y = top;
        for &index in &cols[col as usize] {
            let draft = &drafts[index];
            placed[index] = Placed {
                x: col_x[col as usize],
                y,
                w: box_w[col as usize],
                h: draft.h,
            };
            y += draft.h + ROW;
        }
        stack = stack.max(y);
    }
    let mut scene = Scene {
        width: x.max(64) + 8,
        height: stack.max(32),
        title: net.title.clone(),
        title_at: (0, 16),
        footer: net.footer.clone(),
        footer_at: (0, 0),
        gates: Vec::with_capacity(drafts.len()),
        squares: Vec::new(),
        bars: Vec::new(),
        captions: Vec::new(),
    };
    for (index, draft) in drafts.iter().enumerate() {
        let at = &placed[index];
        scene.gates.push(GateBox {
            x: at.x,
            y: at.y,
            w: at.w,
            h: at.h,
            label: draft.label,
        });
        let out_y = at.y + draft.out_off;
        let exit_x = if draft.invert {
            let sq_x = at.x + at.w - 1;
            let sq_y = out_y - (INVERT / 2) + 1;
            scene.squares.push(Square {
                x: sq_x,
                y: sq_y,
                size: INVERT,
            });
            sq_x + INVERT - 1
        } else {
            at.x + at.w - 1
        };
        for (port, name) in draft.names.iter().enumerate() {
            let Some(name) = name else {
                continue;
            };
            let port_y = at.y + draft.in_off[port];
            hbar(&mut scene.bars, at.x - STUB, at.x, port_y);
            scene.captions.push(Caption {
                text: name.clone(),
                x: at.x - STUB - 4,
                y: port_y + 4,
                anchor: "end",
            });
        }
        scene.captions.push(Caption {
            text: draft.result.clone(),
            x: at.x + at.w / 2,
            y: at.y + at.h + 14,
            anchor: "middle",
        });
        if draft.live {
            hbar(&mut scene.bars, exit_x, exit_x + STUB, out_y);
            scene.captions.push(Caption {
                text: draft.result.clone(),
                x: exit_x + STUB + 4,
                y: out_y + 4,
                anchor: "start",
            });
        }
    }
    let columns = Columns {
        x: col_x,
        box_w,
        invert_pad,
        live_w,
    };
    for link in &routed {
        route(&mut scene.bars, link, &drafts, &placed, &columns, head);
    }
    let mut right = scene.width;
    let mut bottom = scene.height;
    for gate in &scene.gates {
        right = right.max(gate.x + gate.w + 8);
        bottom = bottom.max(gate.y + gate.h + ROW);
    }
    for bar in &scene.bars {
        right = right.max(bar.x + bar.w + 8);
        bottom = bottom.max(bar.y + bar.h + 8);
    }
    for caption in &scene.captions {
        let tw = text_width(&caption.text, CAPTION).ceil() as i64;
        let end = match caption.anchor {
            "end" => caption.x + 8,
            "middle" => caption.x + tw / 2 + 8,
            _ => caption.x + tw + 8,
        };
        right = right.max(end);
        bottom = bottom.max(caption.y + 8);
    }
    if let Some(title) = &scene.title {
        right = right.max(text_width(title, 14.0).ceil() as i64 + 16);
    }
    if let Some(footer) = &scene.footer {
        right = right.max(text_width(footer, 14.0).ceil() as i64 + 16);
    }
    scene.width = right.max(64);
    scene.height = (bottom + foot).max(32);
    scene.title_at = (scene.width / 2, 16);
    scene.footer_at = (scene.width / 2, bottom + 16);
    scene
}

fn route(
    bars: &mut Vec<Bar>,
    link: &Link,
    drafts: &[Draft],
    placed: &[Placed],
    columns: &Columns,
    head: i64,
) {
    let from = &placed[link.from];
    let to = &placed[link.to];
    let from_draft = &drafts[link.from];
    let exit_y = from.y + from_draft.out_off;
    let exit_x = if from_draft.invert {
        from.x + from.w - 1 + INVERT - 1
    } else {
        from.x + from.w - 1
    };
    let enter_y = to.y + drafts[link.to].in_off[link.port];
    let enter_x = to.x;
    let up_x = track_x(columns, link.from_col, link.up);
    if link.sky {
        let down_x = track_x(columns, link.to_col - 1, link.down);
        let lane = head + 4 + link.lane as i64 * TRACK;
        hbar(bars, exit_x, up_x, exit_y);
        vbar(bars, up_x, exit_y, lane);
        hbar(bars, up_x, down_x, lane);
        vbar(bars, down_x, lane, enter_y);
        hbar(bars, down_x, enter_x, enter_y);
    } else {
        hbar(bars, exit_x, up_x, exit_y);
        vbar(bars, up_x, exit_y, enter_y);
        hbar(bars, up_x, enter_x, enter_y);
    }
}

fn track_x(columns: &Columns, col: usize, index: usize) -> i64 {
    let right_pad = if columns.live_w[col] > 0 {
        STUB + 8 + columns.live_w[col]
    } else {
        0
    };
    columns.x[col]
        + columns.box_w[col]
        + columns.invert_pad[col]
        + right_pad
        + 4
        + index as i64 * TRACK
}

fn hbar(bars: &mut Vec<Bar>, x0: i64, x1: i64, y: i64) {
    let (x, w) = if x1 >= x0 {
        (x0, x1 - x0 + 1)
    } else {
        (x1, x0 - x1 + 1)
    };
    bars.push(Bar { x, y, w, h: 1 });
}

fn vbar(bars: &mut Vec<Bar>, x: i64, y0: i64, y1: i64) {
    let (y, h) = if y1 >= y0 {
        (y0, y1 - y0 + 1)
    } else {
        (y1, y0 - y1 + 1)
    };
    bars.push(Bar { x, y, w: 1, h });
}

fn writers(net: &Netlist) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for (index, gate) in net.gates.iter().enumerate() {
        for out in &gate.outputs {
            map.insert(out.as_str(), index);
        }
    }
    map
}

fn drafts(net: &Netlist, writers: &HashMap<&str, usize>) -> Vec<Draft> {
    net.gates
        .iter()
        .enumerate()
        .map(|(index, gate)| {
            let label = gate.kind.label();
            let result = gate.outputs.first().cloned().unwrap_or_default();
            let result_px = text_width(&result, CAPTION).ceil() as i64;
            let label_px = text_width(label, TEXT).ceil() as i64;
            let w = snap(label_px.max(result_px) + PAD * 2).max(MIN_W);
            let n = gate.inputs.len().max(1);
            let h = snap(16 + n as i64 * 24).max(MIN_H);
            let names = gate
                .inputs
                .iter()
                .map(|input| primary_label(input, writers))
                .collect::<Vec<_>>();
            Draft {
                index,
                w,
                h,
                label,
                invert: gate.kind.invert(),
                in_off: port_rows(h, n),
                out_off: (h - 1) / 2,
                names,
                live: is_live(&result, &net.gates),
                result,
            }
        })
        .collect()
}

fn primary_label(input: &Operand, writers: &HashMap<&str, usize>) -> Option<String> {
    match input {
        Operand::Const(value) => Some(value.to_string()),
        Operand::Name(name) if !writers.contains_key(name.as_str()) => Some(name.clone()),
        Operand::Name(_) => None,
    }
}

fn is_live(result: &str, gates: &[Gate]) -> bool {
    !gates.iter().any(|gate| {
        gate.inputs
            .iter()
            .any(|input| operand_name(input) == Some(result))
    })
}

fn port_rows(h: i64, n: usize) -> Vec<i64> {
    let n = n.max(1) as i64;
    (1..=n).map(|i| (i * (h - 1)) / (n + 1)).collect()
}

fn name_widths(drafts: &[Draft], max_col: u32, net: &Netlist) -> Vec<i64> {
    let mut widths = vec![0i64; max_col as usize + 1];
    for draft in drafts {
        let col = net.gates[draft.index].column as usize;
        for name in draft.names.iter().flatten() {
            let px = text_width(name, CAPTION).ceil() as i64;
            widths[col] = widths[col].max(px);
        }
    }
    widths
}

fn live_widths(drafts: &[Draft], max_col: u32, net: &Netlist) -> Vec<i64> {
    let mut widths = vec![0i64; max_col as usize + 1];
    for draft in drafts {
        if !draft.live {
            continue;
        }
        let col = net.gates[draft.index].column as usize;
        let px = text_width(&draft.result, CAPTION).ceil() as i64;
        widths[col] = widths[col].max(px);
    }
    widths
}

fn links(net: &Netlist, writers: &HashMap<&str, usize>) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (index, gate) in net.gates.iter().enumerate() {
        for (port, input) in gate.inputs.iter().enumerate() {
            let Some(name) = operand_name(input) else {
                continue;
            };
            let Some(&src) = writers.get(name) else {
                continue;
            };
            if src < index {
                out.push((src, index, port));
            }
        }
    }
    out
}

fn snap(n: i64) -> i64 {
    (n.max(0) + 3) & !3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skipped_column_keeps_its_interior_paper() {
        let net = crate::gtl::parse(
            "\
@gtl
and a b -> p
not p -> q
xor p q -> r
",
        )
        .unwrap();
        let scene = layout(&net);
        assert!(scene.gates.len() == 3);
        assert!(scene.gates[0].x < scene.gates[1].x);
        assert!(scene.gates[1].x < scene.gates[2].x);
        assert_eq!(scene.squares.len(), 1);
        for bar in &scene.bars {
            for gate in &scene.gates {
                assert!(
                    !hits_interior(bar, gate),
                    "bar {},{} {}x{} crosses {},{}",
                    bar.x,
                    bar.y,
                    bar.w,
                    bar.h,
                    gate.x,
                    gate.y
                );
            }
        }
        let mux = crate::gtl::parse("@gtl\nmux s a b -> y\n").unwrap();
        let mux = layout(&mux);
        let gate = &mux.gates[0];
        let rows = port_rows(gate.h, 3);
        assert!(rows[0] < rows[1] && rows[1] < rows[2]);
        assert!(rows.windows(2).all(|pair| pair[1] - pair[0] >= 8));
    }

    fn hits_interior(bar: &Bar, gate: &GateBox) -> bool {
        let x1 = bar.x + bar.w;
        let y1 = bar.y + bar.h;
        let left = gate.x + 1;
        let top = gate.y + 1;
        let right = gate.x + gate.w - 1;
        let bottom = gate.y + gate.h - 1;
        bar.x < right && x1 > left && bar.y < bottom && y1 > top
    }
}
