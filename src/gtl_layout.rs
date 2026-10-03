//! One left-to-right walk that places gate symbols and the wires between them.
//!
//! Columns come from the parser: `column = 1 + max driver column`, and primary
//! inputs are not gates. Inside a column, each gate slides down to line its
//! input up with the wire that feeds it, so a chain reads as one straight
//! line. A wire that skips columns runs straight through when nothing is in
//! its way, and otherwise steps to the nearest free row to cross them. Every
//! track in a gap belongs to one net, so wires cross but never share a run.

use crate::draw::{measure, snap};
use crate::gtl::{operand_name, Gate, Kind, Netlist, Operand};
use std::collections::HashMap;

const LABEL: f64 = 11.0;
const NET: f64 = 10.0;
const TRACK: i64 = 8;
const STUB: i64 = 12;
/// Space between two gates stacked in one column.
const ROW: i64 = 28;
pub(crate) const PILL_H: i64 = 18;
/// Inversion bubble radius.
pub(crate) const BUBBLE: i64 = 4;
/// How far a mux select runs under the body before it turns up.
const SELECT: i64 = 16;

#[derive(Clone, Debug)]
pub(crate) struct Scene {
    pub width: i64,
    pub height: i64,
    pub title: Option<String>,
    pub title_at: (i64, i64),
    pub footer: Option<String>,
    pub footer_at: (i64, i64),
    pub gates: Vec<Symbol>,
    /// Gate-to-gate wires, then pin stubs.
    pub wires: Vec<Vec<(i64, i64)>>,
    pub dots: Vec<(i64, i64)>,
    pub pills: Vec<Pill>,
    pub nets: Vec<NetLabel>,
}

#[derive(Clone, Debug)]
pub(crate) struct Symbol {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub kind: Kind,
    pub label: &'static str,
    /// Port rows for the `0` and `1` marks inside a mux.
    pub data: Vec<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Input,
    Const,
    Output,
}

#[derive(Clone, Debug)]
pub(crate) struct Pill {
    pub x: i64,
    /// Vertical centre.
    pub y: i64,
    pub w: i64,
    pub text: String,
    pub role: Role,
}

#[derive(Clone, Debug)]
pub(crate) struct NetLabel {
    pub x: i64,
    pub y: i64,
    pub text: String,
}

struct Draft {
    w: i64,
    h: i64,
    /// Height kept clear below the gate's top, including a mux select.
    stack: i64,
    in_off: Vec<i64>,
    out_off: i64,
    names: Vec<Option<(String, Role)>>,
    result: String,
    live: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    /// Into the next column.
    Next,
    /// Straight across skipped columns on the output row.
    Through,
    /// Up or down to a free row, across, then to the port.
    Detour,
}

struct Link {
    from: usize,
    to: usize,
    port: usize,
    path: Path,
    corridor: i64,
    up: usize,
    down: usize,
}

pub(crate) fn body(kind: Kind) -> (i64, i64) {
    match kind {
        Kind::Not => (40, 32),
        Kind::Mux => (44, 60),
        Kind::Or | Kind::Nor | Kind::Xor | Kind::Xnor => (60, 40),
        _ => (56, 40),
    }
}

/// Where a wire ends on the left of the gate, relative to its top.
fn entry_offsets(kind: Kind, h: i64, n: usize) -> Vec<i64> {
    if kind == Kind::Mux {
        let mut rows = vec![h + SELECT];
        rows.push(h * 3 / 10);
        rows.push(h - h * 3 / 10);
        rows.truncate(n.max(1));
        return rows;
    }
    let n = n.max(1) as i64;
    (1..=n).map(|i| (2 * i - 1) * h / (2 * n)).collect()
}

/// Exit x of a gate, past its bubble when it inverts.
fn exit_dx(kind: Kind, w: i64) -> i64 {
    if kind.invert() {
        w + 2 * BUBBLE
    } else {
        w
    }
}

/// The run from a port's entry into the body: a select turns up from below.
pub(crate) fn tail(
    kind: Kind,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    port: usize,
    entry: i64,
) -> Vec<(i64, i64)> {
    if kind == Kind::Mux && port == 0 {
        let mid = x + w / 2;
        return vec![(mid, entry), (mid, y + h - 8)];
    }
    let depth = match kind {
        Kind::Or | Kind::Nor | Kind::Xor | Kind::Xnor => 12,
        _ => 4,
    };
    vec![(x + depth, entry)]
}

pub(crate) fn layout(net: &Netlist) -> Scene {
    let head = if net.title.is_some() { 36 } else { 0 };
    let foot = if net.footer.is_some() { 30 } else { 0 };
    let writers = writers(net);
    let drafts = drafts(net, &writers);
    let column = |index: usize| net.gates[index].column as usize;
    let max_col = (0..drafts.len()).map(column).max().unwrap_or(0);
    let raw_links = links(net, &writers);

    // Vertical placement, column by column.
    let mut top = vec![0i64; drafts.len()];
    let mut cols: Vec<Vec<usize>> = vec![Vec::new(); max_col + 1];
    for index in 0..drafts.len() {
        cols[column(index)].push(index);
    }
    let mut feeds: Vec<Vec<(usize, usize)>> = vec![Vec::new(); drafts.len()];
    for &(from, to, port) in &raw_links {
        feeds[to].push((from, port));
    }
    for (col, members) in cols.iter_mut().enumerate().skip(1) {
        let mut keyed: Vec<(i64, usize)> = Vec::with_capacity(members.len());
        let mut previous = i64::MIN / 4;
        let mut wanted = vec![None; drafts.len()];
        for &index in members.iter() {
            let feed = &feeds[index];
            // Line up with one driver exactly, preferring the previous column,
            // so at least one input is a straight wire.
            let want = feed
                .iter()
                .find(|&&(from, _)| column(from) + 1 == col)
                .or_else(|| feed.first())
                .map(|&(from, port)| top[from] + drafts[from].out_off - drafts[index].in_off[port]);
            let key = want.unwrap_or(previous);
            previous = key;
            wanted[index] = want;
            keyed.push((key, index));
        }
        keyed.sort_by_key(|&(key, _)| key);
        let mut floor: Option<i64> = None;
        let mut order = Vec::with_capacity(keyed.len());
        for (_, index) in keyed {
            let y = match (wanted[index], floor) {
                (Some(want), Some(floor)) => want.max(floor),
                (Some(want), None) => want,
                (None, Some(floor)) => floor,
                (None, None) => 0,
            };
            top[index] = y;
            floor = Some(y + drafts[index].stack + ROW);
            order.push(index);
        }
        *members = order;
    }

    // A wire that skips columns rides one horizontal corridor across them:
    // its own output row when that is free, else the nearest free row.
    let mut corridor = vec![0i64; raw_links.len()];
    let mut taken: Vec<(i64, usize, usize, usize)> = Vec::new();
    for (index, &(from, to, port)) in raw_links.iter().enumerate() {
        let (from_col, to_col) = (column(from), column(to));
        if to_col == from_col + 1 {
            continue;
        }
        let exit_y = top[from] + drafts[from].out_off;
        let enter_y = top[to] + drafts[to].in_off[port];
        let mut blocked: Vec<(i64, i64)> = Vec::new();
        for &g in cols[from_col + 1..to_col].iter().flatten() {
            blocked.push((top[g] - 12, top[g] + drafts[g].stack + 12));
        }
        let mut candidates = vec![exit_y, enter_y];
        for &(lo, hi) in &blocked {
            for k in 0..8 {
                candidates.push(lo - 1 - k * TRACK);
                candidates.push(hi + 1 + k * TRACK);
            }
        }
        let floor = blocked.iter().map(|&(_, hi)| hi).max().unwrap_or(exit_y) + 1;
        candidates.extend((0..64).map(|k| floor + k * TRACK));
        let free = |y: i64| {
            blocked.iter().all(|&(lo, hi)| y < lo || y > hi)
                && taken.iter().all(|&(other, a, b, net)| {
                    // One net may share its corridor; two nets keep a track apart.
                    (net == from && other == y)
                        || (other - y).abs() >= TRACK
                        || b < from_col
                        || to_col - 1 < a
                })
        };
        let cost = |y: i64| {
            (y - exit_y).abs()
                + (y - enter_y).abs()
                + i64::from(y != exit_y)
                + i64::from(y != enter_y)
        };
        let y = candidates
            .into_iter()
            .filter(|&y| free(y))
            .min_by_key(|&y| (cost(y), y))
            .unwrap_or(floor);
        taken.push((y, from_col, to_col - 1, from));
        corridor[index] = y;
    }

    let mut routed: Vec<Link> = raw_links
        .iter()
        .enumerate()
        .map(|(index, &(from, to, port))| {
            let path = if column(to) == column(from) + 1 {
                Path::Next
            } else if corridor[index] == top[from] + drafts[from].out_off {
                Path::Through
            } else {
                Path::Detour
            };
            Link {
                from,
                to,
                port,
                path,
                corridor: corridor[index],
                up: 0,
                down: 0,
            }
        })
        .collect();

    // Tracks: one per net climbing out of a gap, one per wire dropping in.
    let mut gap_tracks: Vec<Vec<(usize, bool)>> = vec![Vec::new(); max_col + 1];
    for link in &mut routed {
        let (from_col, to_col) = (column(link.from), column(link.to));
        if link.path != Path::Through {
            let key = (link.from, true);
            let tracks = &mut gap_tracks[from_col];
            link.up = tracks.iter().position(|&t| t == key).unwrap_or_else(|| {
                tracks.push(key);
                tracks.len() - 1
            });
        }
        if link.path != Path::Next {
            let tracks = &mut gap_tracks[to_col - 1];
            tracks.push((link.to * 16 + link.port, false));
            link.down = tracks.len() - 1;
        }
    }
    order_tracks(&mut routed, &gap_tracks, &top, &drafts, &column);
    let highest = (0..drafts.len())
        .map(|i| top[i])
        .chain(
            routed
                .iter()
                .filter(|l| l.path == Path::Detour)
                .map(|l| l.corridor - 8),
        )
        .min()
        .unwrap_or(0);
    let shift = head + 8 - highest;
    for y in &mut top {
        *y += shift;
    }
    for link in &mut routed {
        link.corridor += shift;
    }

    // Column widths and gaps.
    let mut box_w = vec![0i64; max_col + 1];
    let mut right_pad = vec![0i64; max_col + 1];
    let mut left_pad = vec![0i64; max_col + 1];
    for (index, draft) in drafts.iter().enumerate() {
        let c = column(index);
        let kind = net.gates[index].kind;
        box_w[c] = box_w[c].max(exit_dx(kind, draft.w));
        let tag = if draft.live {
            STUB + pill_w(&draft.result) + 6
        } else {
            measure(&draft.result, NET, false) + 16
        };
        right_pad[c] = right_pad[c].max(tag);
        for (name, _) in draft.names.iter().flatten() {
            left_pad[c] = left_pad[c].max(pill_w(name) + STUB + 10);
        }
    }
    let mut col_x = vec![0i64; max_col + 1];
    let mut track_base = vec![0i64; max_col + 1];
    let mut x = if max_col == 0 { 8 } else { left_pad[1].max(8) };
    for col in 1..=max_col {
        col_x[col] = x;
        x += box_w[col];
        if col < max_col {
            x += right_pad[col];
            let tracks = gap_tracks[col].len() as i64;
            track_base[col] = x + 6;
            x += if tracks > 0 {
                6 + (tracks - 1) * TRACK + 14
            } else {
                16
            };
            x += left_pad[col + 1].max(8);
        } else {
            x += right_pad[col];
        }
    }

    let mut scene = Scene {
        width: x.max(64),
        height: 0,
        title: net.title.clone(),
        title_at: (0, 20),
        footer: net.footer.clone(),
        footer_at: (0, 0),
        gates: Vec::with_capacity(drafts.len()),
        wires: Vec::new(),
        dots: Vec::new(),
        pills: Vec::new(),
        nets: Vec::new(),
    };
    for (index, draft) in drafts.iter().enumerate() {
        let gate = &net.gates[index];
        let (gx, gy) = (col_x[column(index)], top[index]);
        scene.gates.push(Symbol {
            x: gx,
            y: gy,
            w: draft.w,
            h: draft.h,
            kind: gate.kind,
            label: gate.kind.label(),
            data: if gate.kind == Kind::Mux {
                draft.in_off[1..].iter().map(|off| gy + off).collect()
            } else {
                Vec::new()
            },
        });
    }

    // Gate-to-gate wires, with a dot wherever one net branches.
    let mut by_net: HashMap<usize, Vec<Vec<(i64, i64)>>> = HashMap::new();
    for link in &routed {
        let (from, to) = (&drafts[link.from], &drafts[link.to]);
        let (fc, tc) = (column(link.from), column(link.to));
        let kind = net.gates[link.from].kind;
        let fx = col_x[fc];
        let exit_y = top[link.from] + from.out_off;
        let start = (
            fx + exit_dx(kind, from.w) - if kind.invert() { BUBBLE } else { 2 },
            exit_y,
        );
        let tx = col_x[tc];
        let enter_y = top[link.to] + to.in_off[link.port];
        let up_x = track_base[fc] + link.up as i64 * TRACK;
        let down_x = if tc > 0 {
            track_base[tc - 1] + link.down as i64 * TRACK
        } else {
            0
        };
        let mut points = vec![start];
        match link.path {
            Path::Next => {
                points.push((up_x, exit_y));
                points.push((up_x, enter_y));
            }
            Path::Through => {
                points.push((down_x, exit_y));
                points.push((down_x, enter_y));
            }
            Path::Detour => {
                points.push((up_x, exit_y));
                points.push((up_x, link.corridor));
                points.push((down_x, link.corridor));
                points.push((down_x, enter_y));
            }
        }
        points.push((tx, enter_y));
        let to_kind = net.gates[link.to].kind;
        points.extend(tail(
            to_kind,
            tx,
            top[link.to],
            to.w,
            to.h,
            link.port,
            enter_y,
        ));
        by_net.entry(link.from).or_default().push(points);
    }
    let mut producers: Vec<usize> = by_net.keys().copied().collect();
    producers.sort_unstable();
    for producer in producers {
        let paths = by_net.remove(&producer).unwrap();
        scene.dots.extend(crate::draw::junctions(&paths));
        scene.wires.extend(paths);
    }

    // Pins: inputs and tie-offs on the left, live-outs and net names on the right.
    for (index, draft) in drafts.iter().enumerate() {
        let gate = &net.gates[index];
        let (gx, gy) = (col_x[column(index)], top[index]);
        for (port, name) in draft.names.iter().enumerate() {
            let Some((name, role)) = name else {
                continue;
            };
            let entry = gy + draft.in_off[port];
            let w = pill_w(name);
            let right = gx - STUB;
            scene.pills.push(Pill {
                x: right - w,
                y: entry,
                w,
                text: name.clone(),
                role: *role,
            });
            let mut stub = vec![(right - 2, entry), (gx, entry)];
            stub.extend(tail(gate.kind, gx, gy, draft.w, draft.h, port, entry));
            scene.wires.push(stub);
        }
        let exit = gx + exit_dx(gate.kind, draft.w);
        let exit_y = gy + draft.out_off;
        if draft.live {
            let from = exit - if gate.kind.invert() { BUBBLE } else { 2 };
            scene
                .wires
                .push(vec![(from, exit_y), (exit + STUB + 2, exit_y)]);
            scene.pills.push(Pill {
                x: exit + STUB,
                y: exit_y,
                w: pill_w(&draft.result),
                text: draft.result.clone(),
                role: Role::Output,
            });
        } else {
            scene.nets.push(NetLabel {
                x: exit + 6,
                y: exit_y - 6,
                text: draft.result.clone(),
            });
        }
    }

    // Bounds.
    let mut right = scene.width;
    let mut bottom = 32;
    for gate in &scene.gates {
        right = right.max(gate.x + exit_dx(gate.kind, gate.w) + 8);
        bottom = bottom.max(gate.y + gate.h + 10);
    }
    for (index, draft) in drafts.iter().enumerate() {
        bottom = bottom.max(top[index] + draft.stack + 10);
    }
    for path in &scene.wires {
        for &(px, py) in path {
            right = right.max(px + 8);
            bottom = bottom.max(py + 10);
        }
    }
    for pill in &scene.pills {
        right = right.max(pill.x + pill.w + 4);
        bottom = bottom.max(pill.y + PILL_H / 2 + 8);
    }
    for label in &scene.nets {
        right = right.max(label.x + measure(&label.text, NET, false) + 6);
    }
    if let Some(title) = &scene.title {
        right = right.max(measure(title, 15.0, true) + 16);
    }
    if let Some(footer) = &scene.footer {
        right = right.max(measure(footer, 12.0, false) + 16);
    }
    if scene.gates.is_empty() {
        bottom = head.max(32);
    }
    scene.width = right.max(64);
    scene.height = (bottom + foot).max(32);
    scene.title_at = (scene.width / 2, 20);
    scene.footer_at = (scene.width / 2, bottom + 20);
    scene
}

/// Within a gap, a wire that arrives from the left at some row must turn
/// onto its track before any other net's track that leaves for the right on
/// that row, or the two runs would overlap. Reorder the tracks to keep that.
fn order_tracks(
    routed: &mut [Link],
    gap_tracks: &[Vec<(usize, bool)>],
    top: &[i64],
    drafts: &[Draft],
    column: &dyn Fn(usize) -> usize,
) {
    // (gap, track, row, arrives from the left, net)
    let mut pieces: Vec<(usize, usize, i64, bool, usize)> = Vec::new();
    for link in routed.iter() {
        let (fc, tc) = (column(link.from), column(link.to));
        let exit_y = top[link.from] + drafts[link.from].out_off;
        let enter_y = top[link.to] + drafts[link.to].in_off[link.port];
        let net = link.from;
        match link.path {
            Path::Next => {
                pieces.push((fc, link.up, exit_y, true, net));
                pieces.push((fc, link.up, enter_y, false, net));
            }
            Path::Through => {
                pieces.push((tc - 1, link.down, exit_y, true, net));
                pieces.push((tc - 1, link.down, enter_y, false, net));
            }
            Path::Detour => {
                pieces.push((fc, link.up, exit_y, true, net));
                pieces.push((fc, link.up, link.corridor, false, net));
                pieces.push((tc - 1, link.down, link.corridor, true, net));
                pieces.push((tc - 1, link.down, enter_y, false, net));
            }
        }
    }
    for (gap, tracks) in gap_tracks.iter().enumerate() {
        let n = tracks.len();
        if n < 2 {
            continue;
        }
        let mut before = vec![Vec::new(); n];
        let mut needs = vec![0usize; n];
        let here: Vec<_> = pieces.iter().filter(|piece| piece.0 == gap).collect();
        for a in &here {
            for b in &here {
                if a.3
                    && !b.3
                    && a.4 != b.4
                    && a.1 != b.1
                    && (a.2 - b.2).abs() < TRACK
                    && !before[a.1].contains(&b.1)
                {
                    before[a.1].push(b.1);
                    needs[b.1] += 1;
                }
            }
        }
        let mut placed = vec![usize::MAX; n];
        let mut next = 0;
        while next < n {
            let pick = (0..n)
                .find(|&t| placed[t] == usize::MAX && needs[t] == 0)
                .or_else(|| (0..n).find(|&t| placed[t] == usize::MAX))
                .unwrap();
            placed[pick] = next;
            next += 1;
            for &later in &before[pick] {
                needs[later] = needs[later].saturating_sub(1);
            }
        }
        for link in routed.iter_mut() {
            if link.path != Path::Through && column(link.from) == gap {
                link.up = placed[link.up];
            }
            if link.path != Path::Next && column(link.to) - 1 == gap {
                link.down = placed[link.down];
            }
        }
    }
}

pub(crate) fn pill_w(text: &str) -> i64 {
    snap(measure(text, LABEL, true) + 16).max(24)
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
        .map(|gate| {
            let (w, h) = body(gate.kind);
            let n = gate.inputs.len().max(1);
            let in_off = entry_offsets(gate.kind, h, n);
            let stack = in_off.iter().copied().max().unwrap_or(0).max(h);
            let result = gate.outputs.first().cloned().unwrap_or_default();
            Draft {
                w,
                h,
                stack,
                in_off,
                out_off: h / 2,
                names: gate
                    .inputs
                    .iter()
                    .map(|input| primary_label(input, writers))
                    .collect(),
                live: is_live(&result, &net.gates),
                result,
            }
        })
        .collect()
}

fn primary_label(input: &Operand, writers: &HashMap<&str, usize>) -> Option<(String, Role)> {
    match input {
        Operand::Const(value) => Some((value.to_string(), Role::Const)),
        Operand::Name(name) if !writers.contains_key(name.as_str()) => {
            Some((name.clone(), Role::Input))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hits_body(path: &[(i64, i64)], gate: &Symbol, own: bool) -> bool {
        // A wire starts a few pixels inside its driver, under the body or bubble.
        let mut path = path.to_vec();
        path[0].0 += BUBBLE + 1;
        path.windows(2).any(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let (x0, x1) = (a.0.min(b.0) - 1, a.0.max(b.0) + 1);
            let (y0, y1) = (a.1.min(b.1) - 1, a.1.max(b.1) + 1);
            // A wire may reach a few pixels into its own gate, under the fill.
            let left = gate.x + if own { 14 } else { 0 };
            x1 > left && x0 < gate.x + gate.w && y1 > gate.y && y0 < gate.y + gate.h
        })
    }

    #[test]
    fn chains_line_up_and_wires_stay_out_of_other_gates() {
        let net = crate::gtl::parse("@gtl\nand a b -> p\nnot p -> q\nxor p q -> r\n").unwrap();
        let scene = layout(&net);
        assert_eq!(scene.gates.len(), 3);
        assert!(scene.gates[0].x < scene.gates[1].x);
        assert!(scene.gates[1].x < scene.gates[2].x);
        // NOT's input lines up with AND's output: the wire is one straight run.
        let and = &scene.gates[0];
        let not = &scene.gates[1];
        assert_eq!(and.y + and.h / 2, not.y + not.h / 2);
        for path in &scene.wires {
            for gate in &scene.gates {
                let own = path.last().is_some_and(|end| {
                    end.0 >= gate.x
                        && end.0 <= gate.x + gate.w
                        && end.1 >= gate.y
                        && end.1 <= gate.y + gate.h + SELECT
                });
                assert!(!hits_body(path, gate, own), "{path:?} crosses {gate:?}");
            }
        }
        // p fans out to NOT and XOR, so it branches once.
        assert_eq!(scene.dots.len(), 1);
        assert_eq!(
            scene
                .pills
                .iter()
                .filter(|pill| pill.role == Role::Output)
                .count(),
            1
        );
        assert_eq!(
            scene
                .pills
                .iter()
                .filter(|pill| pill.role == Role::Input)
                .count(),
            2
        );
    }

    #[test]
    fn mux_select_enters_from_below() {
        let net = crate::gtl::parse("@gtl\nmux s a b -> y\n").unwrap();
        let scene = layout(&net);
        let gate = &scene.gates[0];
        let select = scene.pills.iter().find(|pill| pill.text == "s").unwrap();
        assert!(select.y > gate.y + gate.h);
        assert_eq!(gate.data.len(), 2);
        assert!(gate.data[1] - gate.data[0] >= 16);
    }
}
