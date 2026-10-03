//! Integer layout for an ASM chart.
//!
//! Each state is a block. Inside a block, exit 0 continues down the spine and
//! exit 1 leaves to the right. Blocks stack in source order. A spine link into
//! the next block is a straight arrow; every other link is an orthogonal
//! polyline in a side channel.

use crate::asm::{Branch, Chart, Decision, Exit, Next};
use crate::text_width;
use std::collections::HashMap;

const FONT: f64 = 12.0;
const LINE: i64 = 16;
const GAP: i64 = 16;
const H_GAP: i64 = 24;
const BLOCK_GAP: i64 = 40;
/// From the bottom of a block to its first outgoing row, between rows, and
/// from the last row to where a wire turns into the next state.
const CLEAR: i64 = 12;
const ROW: i64 = 8;
const APPROACH: i64 = 14;
/// Height of a state's name header.
pub(crate) const HEADER: i64 = 24;
const PAD: i64 = 8;
const CHANNEL: i64 = 14;
const OUTSET: i64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    State,
    Diamond,
    Cond,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub lines: Vec<String>,
    /// Leading lines that name a state; the rest are its outputs.
    pub head: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Seg {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub arrow: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Digit {
    pub text: &'static str,
    pub x: i64,
    pub y: i64,
    pub end: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Scene {
    pub nodes: Vec<Node>,
    pub segs: Vec<Seg>,
    pub digits: Vec<Digit>,
    pub title: Option<String>,
    pub footer: Option<String>,
    pub title_at: (i64, i64),
    pub footer_at: (i64, i64),
    pub width: i64,
    pub height: i64,
}

struct Piece {
    nodes: Vec<Node>,
    segs: Vec<Seg>,
    exits: Vec<Port>,
    digits: Vec<Digit>,
    /// Where the parent wire lands. Local `(0, 0)` is the parent port.
    attach: (i64, i64),
}

struct Port {
    x: i64,
    y: i64,
    target: usize,
    spine: bool,
}

struct Block {
    nodes: Vec<Node>,
    segs: Vec<Seg>,
    digits: Vec<Digit>,
    exits: Vec<Port>,
    entry_y: i64,
    bottom: i64,
}

pub(crate) fn layout(chart: &Chart) -> Scene {
    if chart.states.is_empty() {
        return empty_scene(chart);
    }
    let names: HashMap<&str, usize> = chart
        .states
        .iter()
        .enumerate()
        .map(|(index, state)| (state.name.as_str(), index))
        .collect();
    let mut blocks: Vec<Block> = chart
        .states
        .iter()
        .map(|state| layout_block(state, &names))
        .collect();

    let mut y = 24;
    for (index, block) in blocks.iter_mut().enumerate() {
        let bottom = block_bottom(block);
        translate_block(block, 0, y);
        block.entry_y = y;
        block.bottom = y + bottom;
        let rows = block
            .exits
            .iter()
            .filter(|exit| !straight(exit, index))
            .count() as i64;
        y = block.bottom + BLOCK_GAP.max(CLEAR + (rows - 1).max(0) * ROW + 10 + APPROACH);
    }

    let mut segs = Vec::new();
    if let Some(first) = blocks.first() {
        segs.push(Seg {
            x0: 0,
            y0: first.entry_y - 20,
            x1: 0,
            y1: first.entry_y,
            arrow: true,
        });
    }
    route(&blocks, &mut segs);

    let mut nodes = Vec::new();
    let mut digits = Vec::new();
    for block in blocks {
        nodes.extend(block.nodes);
        segs.extend(block.segs);
        digits.extend(block.digits);
    }
    place_caption(chart, nodes, segs, digits)
}

fn empty_scene(chart: &Chart) -> Scene {
    let title = caption(&chart.title);
    let footer = caption(&chart.footer);
    let title_w = title
        .as_ref()
        .map(|text| phrase_width(text, 14.0))
        .unwrap_or(0);
    let footer_w = footer
        .as_ref()
        .map(|text| phrase_width(text, 14.0))
        .unwrap_or(0);
    let width = title_w.max(footer_w).max(32) + PAD * 2;
    let mut height = PAD * 2;
    let mut title_at = (width / 2, 0);
    let mut footer_at = (width / 2, 0);
    if title.is_some() {
        title_at.1 = PAD + 16;
        height += 28;
    }
    if footer.is_some() {
        footer_at.1 = height + 16;
        height += 28;
    }
    height = height.max(32);
    Scene {
        nodes: Vec::new(),
        segs: Vec::new(),
        digits: Vec::new(),
        title,
        footer,
        title_at,
        footer_at,
        width,
        height,
    }
}

fn caption(text: &Option<String>) -> Option<String> {
    text.as_ref().filter(|text| !text.is_empty()).cloned()
}

fn place_caption(
    chart: &Chart,
    mut nodes: Vec<Node>,
    mut segs: Vec<Seg>,
    mut digits: Vec<Digit>,
) -> Scene {
    let title = caption(&chart.title);
    let footer = caption(&chart.footer);
    let (x0, y0, x1, y1) = geometry_bounds(&nodes, &segs, &digits);
    let content_w = (x1 - x0).max(1);
    let content_h = (y1 - y0).max(1);
    let title_w = title
        .as_ref()
        .map(|text| phrase_width(text, 14.0))
        .unwrap_or(0);
    let footer_w = footer
        .as_ref()
        .map(|text| phrase_width(text, 14.0))
        .unwrap_or(0);
    let inner_w = content_w.max(title_w).max(footer_w);
    let title_h = if title.is_some() { 28 } else { 0 };
    let footer_h = if footer.is_some() { 28 } else { 0 };
    let width = inner_w + PAD * 2;
    let height = content_h + title_h + footer_h + PAD * 2;
    let dx = PAD + (inner_w - content_w) / 2 - x0;
    let dy = PAD + title_h - y0;
    translate_nodes(&mut nodes, dx, dy);
    translate_segs(&mut segs, dx, dy);
    translate_digits(&mut digits, dx, dy);
    let title_at = (width / 2, PAD + 16);
    let footer_at = (width / 2, height - PAD - 6);
    Scene {
        nodes,
        segs,
        digits,
        title,
        footer,
        title_at,
        footer_at,
        width,
        height,
    }
}

fn layout_block(state: &crate::asm::State, names: &HashMap<&str, usize>) -> Block {
    let mut lines = Vec::new();
    push_visual(&mut lines, &state.name);
    let head = lines.len();
    for output in &state.outputs {
        push_visual(&mut lines, &output.text);
    }
    let (w, h) = state_size(&lines, head);
    let mut piece = Piece {
        nodes: vec![Node {
            kind: Kind::State,
            x: -w / 2,
            y: 0,
            w,
            h,
            lines,
            head,
        }],
        segs: Vec::new(),
        exits: Vec::new(),
        digits: Vec::new(),
        attach: (0, 0),
    };
    match &state.exit {
        Exit::None => {}
        Exit::Link(link) => piece.exits.push(Port {
            x: 0,
            y: h,
            target: names[link.target.as_str()],
            spine: true,
        }),
        Exit::Decision(decision) => {
            let mut child = layout_decision(decision, true, names);
            child.translate(0, h + GAP);
            connect(&mut piece.segs, (0, h), (0, h + GAP));
            piece.absorb(child);
        }
    }
    Block {
        nodes: piece.nodes,
        segs: piece.segs,
        digits: piece.digits,
        exits: piece.exits,
        entry_y: 0,
        bottom: 0,
    }
}

fn layout_decision(decision: &Decision, spine: bool, names: &HashMap<&str, usize>) -> Piece {
    let lines = visual(&decision.condition);
    let (dw, dh) = diamond_size(&lines);
    let mut piece = Piece {
        nodes: vec![Node {
            kind: Kind::Diamond,
            x: -dw / 2,
            y: 0,
            w: dw,
            h: dh,
            lines,
            head: 0,
        }],
        segs: Vec::new(),
        exits: Vec::new(),
        digits: vec![
            Digit {
                text: "0",
                x: -7,
                y: dh + 13,
                end: true,
            },
            Digit {
                text: "1",
                x: dw / 2 + 5,
                y: dh / 2 - 7,
                end: false,
            },
        ],
        attach: (0, 0),
    };
    let mut zero = layout_branch(&decision.zero, false, spine, names);
    zero.translate(0, dh);
    connect(&mut piece.segs, (0, dh), zero.attach);
    let zero_right = node_max_x(&zero.nodes).unwrap_or(dw / 2);
    piece.absorb(zero);

    let tip = (dw / 2, dh / 2);
    let mut one = layout_branch(&decision.one, true, false, names);
    one.translate(tip.0, tip.1);
    if let Some(left) = node_min_x(&one.nodes) {
        let limit = zero_right.max(dw / 2);
        let gap = limit + H_GAP - left;
        if gap > 0 {
            one.translate(gap, 0);
        }
    }
    connect(&mut piece.segs, tip, one.attach);
    piece.absorb(one);
    piece
}

fn layout_branch(branch: &Branch, right: bool, spine: bool, names: &HashMap<&str, usize>) -> Piece {
    let mut piece = Piece::bare();
    if let Some(cond) = &branch.cond {
        let lines = visual(cond);
        let (bw, bh) = box_size(&lines);
        let (x, y, attach, cont) = if right {
            (H_GAP, -bh / 2, (H_GAP, 0), (H_GAP + bw / 2, bh / 2))
        } else {
            (-bw / 2, GAP, (0, GAP), (0, GAP + bh))
        };
        piece.nodes.push(Node {
            kind: Kind::Cond,
            x,
            y,
            w: bw,
            h: bh,
            lines,
            head: 0,
        });
        piece.attach = attach;
        attach_next(&mut piece, cont, &branch.next, spine && !right, names);
        return piece;
    }
    match &branch.next {
        Next::Link(link) => {
            piece.attach = (0, 0);
            piece.exits.push(Port {
                x: 0,
                y: 0,
                target: names[link.target.as_str()],
                spine,
            });
        }
        Next::Decision(decision) => {
            let mut child = layout_decision(decision, spine && !right, names);
            if right {
                let min_x = node_min_x(&child.nodes).unwrap_or(0);
                let dx = H_GAP - min_x;
                child.translate(dx, GAP);
                piece.attach = (dx, GAP);
            } else {
                child.translate(0, GAP);
                piece.attach = (0, GAP);
            }
            piece.absorb(child);
        }
    }
    piece
}

fn attach_next(
    piece: &mut Piece,
    cont: (i64, i64),
    next: &Next,
    spine: bool,
    names: &HashMap<&str, usize>,
) {
    match next {
        Next::Link(link) => piece.exits.push(Port {
            x: cont.0,
            y: cont.1,
            target: names[link.target.as_str()],
            spine,
        }),
        Next::Decision(decision) => {
            let mut child = layout_decision(decision, spine, names);
            child.translate(cont.0, cont.1 + GAP);
            connect(&mut piece.segs, cont, (cont.0, cont.1 + GAP));
            piece.absorb(child);
        }
    }
}

struct Wire {
    forward: bool,
    y_lo: i64,
    y_hi: i64,
    from: (i64, i64),
    /// Into the very next block: drop straight to it, no side channel.
    next: bool,
    /// Column the wire drops down, when it steps right first.
    via: Option<i64>,
    entry: (i64, i64),
    y_clear: i64,
    approach: i64,
    /// Source block and target: wires that share one also share a lane.
    group: (usize, usize),
    lane: usize,
}

/// A spine exit straight into the next state needs no side channel.
fn straight(exit: &Port, index: usize) -> bool {
    exit.spine && exit.x == 0 && exit.target == index + 1
}

fn route(blocks: &[Block], segs: &mut Vec<Seg>) {
    let mut wires = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let mut leaving: Vec<&Port> = Vec::new();
        for exit in &block.exits {
            if straight(exit, index) {
                segs.push(Seg {
                    x0: exit.x,
                    y0: exit.y,
                    x1: 0,
                    y1: blocks[exit.target].entry_y,
                    arrow: true,
                });
            } else {
                leaving.push(exit);
            }
        }
        // Exits bound for one state merge: they share a drop column and one
        // row under the block. A right-hand exit steps out past every node
        // below it, so its drop never runs down a diamond tip or a box.
        let mut targets: Vec<usize> = Vec::new();
        for exit in &leaving {
            if !targets.contains(&exit.target) {
                targets.push(exit.target);
            }
        }
        targets.sort_by_key(|&target| {
            let x = leaving
                .iter()
                .filter(|exit| exit.target == target)
                .map(|exit| exit.x)
                .max()
                .unwrap_or(0);
            if target > index {
                (1, -x)
            } else {
                (0, x)
            }
        });
        let mut last_column = i64::MIN;
        for (row, &target) in targets.iter().enumerate() {
            let group: Vec<&&Port> = leaving
                .iter()
                .filter(|exit| exit.target == target)
                .collect();
            let column = group
                .iter()
                .filter(|exit| exit.x > 0)
                .map(|exit| drop_column(&block.nodes, exit))
                .max()
                .map(|x| x.max(last_column + ROW));
            if let Some(x) = column {
                last_column = x;
            }
            let y_clear = block.bottom + CLEAR + row as i64 * ROW;
            let approach = blocks[target].entry_y - APPROACH;
            for exit in group {
                let via = column.filter(|&x| exit.x > 0 && clear_run(&block.nodes, exit, x));
                wires.push(Wire {
                    forward: target > index,
                    next: target == index + 1,
                    y_lo: y_clear.min(approach),
                    y_hi: y_clear.max(approach),
                    from: (exit.x, exit.y),
                    via,
                    entry: (0, blocks[target].entry_y),
                    y_clear,
                    approach,
                    group: (index, target),
                    lane: 0,
                });
            }
        }
    }
    assign_lanes(&mut wires, true);
    assign_lanes(&mut wires, false);
    let (min_x, _, max_x, _) = block_bounds(blocks, segs);
    for wire in wires {
        let channel = if wire.forward {
            max_x + OUTSET + wire.lane as i64 * CHANNEL
        } else {
            min_x - OUTSET - wire.lane as i64 * CHANNEL
        };
        let drop = wire.via.unwrap_or(wire.from.0);
        if wire.next {
            push_polyline(
                segs,
                &[
                    wire.from,
                    (drop, wire.from.1),
                    (drop, wire.approach),
                    (wire.entry.0, wire.approach),
                    wire.entry,
                ],
            );
            continue;
        }
        push_polyline(
            segs,
            &[
                wire.from,
                (drop, wire.from.1),
                (drop, wire.y_clear),
                (channel, wire.y_clear),
                (channel, wire.approach),
                (wire.entry.0, wire.approach),
                wire.entry,
            ],
        );
    }
}

/// A column right of every node that reaches below `exit`.
fn drop_column(nodes: &[Node], exit: &Port) -> i64 {
    nodes
        .iter()
        .filter(|node| node.y + node.h > exit.y + 1 && node.x + node.w > exit.x - 4)
        .map(|node| node.x + node.w)
        .fold(exit.x, i64::max)
        + CHANNEL
}

/// Whether the step from `exit` right to column `x` misses every node.
fn clear_run(nodes: &[Node], exit: &Port, x: i64) -> bool {
    !nodes
        .iter()
        .any(|node| node.y < exit.y && exit.y < node.y + node.h && node.x > exit.x && node.x < x)
}

fn assign_lanes(wires: &mut [Wire], forward: bool) {
    let mut order: Vec<usize> = (0..wires.len())
        .filter(|&index| wires[index].forward == forward && !wires[index].next)
        .collect();
    order.sort_by_key(|&index| wires[index].y_lo);
    let mut lane_end: Vec<i64> = Vec::new();
    let mut taken: HashMap<(usize, usize), usize> = HashMap::new();
    for index in order {
        if let Some(&lane) = taken.get(&wires[index].group) {
            wires[index].lane = lane;
            continue;
        }
        let start = wires[index].y_lo;
        let end = wires[index].y_hi;
        let mut chosen = None;
        for (lane, last) in lane_end.iter_mut().enumerate() {
            if *last <= start {
                *last = end;
                chosen = Some(lane);
                break;
            }
        }
        wires[index].lane = chosen.unwrap_or_else(|| {
            lane_end.push(end);
            lane_end.len() - 1
        });
        taken.insert(wires[index].group, wires[index].lane);
    }
}

fn block_bounds(blocks: &[Block], segs: &[Seg]) -> (i64, i64, i64, i64) {
    let mut x0 = i64::MAX;
    let mut y0 = i64::MAX;
    let mut x1 = i64::MIN;
    let mut y1 = i64::MIN;
    let mut point = |x: i64, y: i64| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    for block in blocks {
        for node in &block.nodes {
            point(node.x, node.y);
            point(node.x + node.w, node.y + node.h);
        }
        for digit in &block.digits {
            point(digit.x, digit.y);
        }
    }
    for seg in segs {
        point(seg.x0, seg.y0);
        point(seg.x1, seg.y1);
    }
    if x0 == i64::MAX {
        (0, 0, 1, 1)
    } else {
        (x0, y0, x1, y1)
    }
}

fn push_polyline(segs: &mut Vec<Seg>, points: &[(i64, i64)]) {
    let mut compact = Vec::with_capacity(points.len());
    for point in points {
        if compact.last() != Some(point) {
            compact.push(*point);
        }
    }
    for pair in compact.windows(2) {
        let last = pair[1] == *compact.last().unwrap();
        segs.push(Seg {
            x0: pair[0].0,
            y0: pair[0].1,
            x1: pair[1].0,
            y1: pair[1].1,
            arrow: last,
        });
    }
}

fn connect(segs: &mut Vec<Seg>, from: (i64, i64), to: (i64, i64)) {
    if from == to {
        return;
    }
    if from.0 == to.0 || from.1 == to.1 {
        segs.push(Seg {
            x0: from.0,
            y0: from.1,
            x1: to.0,
            y1: to.1,
            arrow: false,
        });
        return;
    }
    let mid = (to.0, from.1);
    segs.push(Seg {
        x0: from.0,
        y0: from.1,
        x1: mid.0,
        y1: mid.1,
        arrow: false,
    });
    segs.push(Seg {
        x0: mid.0,
        y0: mid.1,
        x1: to.0,
        y1: to.1,
        arrow: false,
    });
}

impl Piece {
    fn bare() -> Self {
        Self {
            nodes: Vec::new(),
            segs: Vec::new(),
            exits: Vec::new(),
            digits: Vec::new(),
            attach: (0, 0),
        }
    }

    fn translate(&mut self, dx: i64, dy: i64) {
        if dx == 0 && dy == 0 {
            return;
        }
        self.attach.0 += dx;
        self.attach.1 += dy;
        translate_nodes(&mut self.nodes, dx, dy);
        translate_segs(&mut self.segs, dx, dy);
        translate_digits(&mut self.digits, dx, dy);
        for port in &mut self.exits {
            port.x += dx;
            port.y += dy;
        }
    }

    fn absorb(&mut self, other: Piece) {
        self.nodes.extend(other.nodes);
        self.segs.extend(other.segs);
        self.exits.extend(other.exits);
        self.digits.extend(other.digits);
    }
}

fn translate_block(block: &mut Block, dx: i64, dy: i64) {
    translate_nodes(&mut block.nodes, dx, dy);
    translate_segs(&mut block.segs, dx, dy);
    translate_digits(&mut block.digits, dx, dy);
    for port in &mut block.exits {
        port.x += dx;
        port.y += dy;
    }
}

fn translate_nodes(nodes: &mut [Node], dx: i64, dy: i64) {
    for node in nodes {
        node.x += dx;
        node.y += dy;
    }
}

fn translate_segs(segs: &mut [Seg], dx: i64, dy: i64) {
    for seg in segs {
        seg.x0 += dx;
        seg.y0 += dy;
        seg.x1 += dx;
        seg.y1 += dy;
    }
}

fn translate_digits(digits: &mut [Digit], dx: i64, dy: i64) {
    for digit in digits {
        digit.x += dx;
        digit.y += dy;
    }
}

fn block_bottom(block: &Block) -> i64 {
    let (_, _, _, y1) = geometry_bounds(&block.nodes, &block.segs, &block.digits);
    let exit_y = block.exits.iter().map(|port| port.y).max().unwrap_or(0);
    y1.max(exit_y)
}

fn geometry_bounds(nodes: &[Node], segs: &[Seg], digits: &[Digit]) -> (i64, i64, i64, i64) {
    let mut x0 = i64::MAX;
    let mut y0 = i64::MAX;
    let mut x1 = i64::MIN;
    let mut y1 = i64::MIN;
    let mut point = |x: i64, y: i64| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    for node in nodes {
        point(node.x, node.y);
        point(node.x + node.w, node.y + node.h);
    }
    for seg in segs {
        point(seg.x0, seg.y0);
        point(seg.x1, seg.y1);
    }
    for digit in digits {
        let w = text_width(digit.text, 11.0).ceil() as i64 + 2;
        let x = if digit.end { digit.x - w } else { digit.x };
        point(x, digit.y - 12);
        point(x + w, digit.y + 4);
    }
    if x0 == i64::MAX {
        (0, 0, 1, 1)
    } else {
        (x0, y0, x1, y1)
    }
}

fn node_min_x(nodes: &[Node]) -> Option<i64> {
    nodes.iter().map(|node| node.x).min()
}

fn node_max_x(nodes: &[Node]) -> Option<i64> {
    nodes.iter().map(|node| node.x + node.w).max()
}

fn state_size(lines: &[String], head: usize) -> (i64, i64) {
    let widest = lines
        .iter()
        .enumerate()
        .map(|(index, line)| text_width(line, FONT) * if index < head { 1.08 } else { 1.0 })
        .fold(0.0, f64::max);
    let w = snap(widest.ceil() as i64 + 32).max(64);
    let name = HEADER + (head.max(1) as i64 - 1) * LINE;
    let body = lines.len().saturating_sub(head) as i64;
    let h = if body == 0 {
        name
    } else {
        name + snap(body * LINE + 12).max(28)
    };
    (w, h)
}

fn box_size(lines: &[String]) -> (i64, i64) {
    let widest = lines
        .iter()
        .map(|line| text_width(line, FONT))
        .fold(0.0, f64::max);
    let count = (lines.len() as i64).max(1);
    let w = snap(widest.ceil() as i64 + 32).max(40);
    let h = snap(count * LINE + 12).max(28);
    (w, h)
}

fn diamond_size(lines: &[String]) -> (i64, i64) {
    let widest = lines
        .iter()
        .map(|line| text_width(line, FONT))
        .fold(0.0, f64::max);
    let count = (lines.len() as i64).max(1);
    let w = snap(((widest.ceil() as i64 + 28) * 3) / 2).max(64);
    let h = snap(count * LINE + 32).max(48);
    (w, h)
}

fn snap(n: i64) -> i64 {
    (n.max(0) + 3) & !3
}

fn visual(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

fn push_visual(lines: &mut Vec<String>, text: &str) {
    lines.extend(text.split('\n').map(str::to_string));
}

fn phrase_width(text: &str, size: f64) -> i64 {
    text.split('\n')
        .map(|line| text_width(line, size).ceil() as i64)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm;

    #[test]
    fn spine_link_is_a_straight_arrow_into_the_next_state() {
        let chart = asm::parse("@asm\na:\n  > b\nb:\n").unwrap();
        let scene = layout(&chart);
        let states: Vec<_> = scene
            .nodes
            .iter()
            .filter(|node| node.kind == Kind::State)
            .collect();
        assert_eq!(states.len(), 2);
        let from = states[0];
        let to = states[1];
        assert_eq!(from.x + from.w / 2, to.x + to.w / 2);
        let spine = from.x + from.w / 2;
        assert!(scene.segs.iter().any(|seg| {
            seg.arrow
                && seg.x0 == spine
                && seg.x1 == spine
                && seg.y1 == to.y
                && seg.y0 == from.y + from.h
        }));
    }

    #[test]
    fn handshake_keeps_conditional_output_and_a_left_return() {
        let chart = asm::parse(
            "@asm\n@title Bus handshake\n\nidle:\n  req=0\n  ack=0\n  ? start\n    0 idle\n    1 (req=1) wait\n\nwait:\n  ack=1\n  ? done\n    0 ? hold\n        0 wait\n        1 idle\n    1 idle\n",
        )
        .unwrap();
        let scene = layout(&chart);
        assert!(scene.nodes.iter().any(|node| node.kind == Kind::Diamond));
        assert!(scene
            .nodes
            .iter()
            .any(|node| node.kind == Kind::Cond
                && node.lines.iter().any(|line| line.contains("req=1"))));
        let idle = scene
            .nodes
            .iter()
            .find(|node| node.kind == Kind::State && node.lines[0] == "idle")
            .unwrap();
        let center = idle.x + idle.w / 2;
        assert!(
            scene
                .segs
                .iter()
                .any(|seg| seg.x0 < center - 8 || seg.x1 < center - 8),
            "a back-edge uses the left channel"
        );
        for node in &scene.nodes {
            assert!(node.w >= 4 && node.h >= 4);
        }
    }
}
