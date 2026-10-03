//! One downward walk that places cycle bands, operator cards, and wires.
//!
//! Time runs down. Each band is one cycle: a row of cards, then a routing
//! zone where results travel sideways on their own tracks before they drop
//! across the next clock edge. A value that waits more than one cycle rides a
//! private lane in the gap between columns, so no two wires share a run and
//! no wire passes through a card. Every coordinate is an integer.

use crate::draw::{measure, snap};
use crate::hls::{operand_name, Kind, Op, Operand, Schedule};
use std::collections::HashMap;

/// Left column with the cycle numbers.
const GUTTER: i64 = 48;
/// From a clock edge to the top of the cards in that cycle.
const PAD_TOP: i64 = 18;
pub(crate) const BOX_H: i64 = 40;
/// Spacing of parallel horizontal tracks and vertical lanes.
const TRACK: i64 = 8;
/// Arrow and pill under a result that nothing reads.
const TAG_ZONE: i64 = 34;
const MIN_BAND: i64 = 84;
/// Room between the last column and the usage cells.
const USAGE_GAP: i64 = 24;
pub(crate) const CELL_H: i64 = 20;
const CELL_GAP: i64 = 4;
/// Text sizes.
const LABEL: f64 = 12.0;
const CELL: f64 = 11.0;

#[derive(Clone, Debug)]
pub(crate) struct Scene {
    pub width: i64,
    pub height: i64,
    pub title: Option<String>,
    pub title_at: (i64, i64),
    pub footer: Option<String>,
    pub footer_at: (i64, i64),
    pub bands: Vec<Band>,
    /// Left edge of the first usage cell, and each cell's width.
    pub usage_x: i64,
    pub cell_w: i64,
    pub units: Vec<Unit>,
    pub wires: Vec<Wire>,
    pub dots: Vec<Mark>,
    pub registers: Vec<Mark>,
    pub tags: Vec<Tag>,
}

#[derive(Clone, Debug)]
pub(crate) struct Band {
    pub y: i64,
    pub h: i64,
    pub cycle: u32,
    /// Busy adders, multipliers, and dividers in this cycle.
    pub counts: [u32; 3],
    /// Vertical centre of the card row.
    pub row: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Unit {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub kind: Kind,
    pub expr: String,
    pub result: String,
    pub latency: u32,
    /// Clock edges inside a multi-cycle card.
    pub stages: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Wire {
    pub points: Vec<(i64, i64)>,
    pub kind: Kind,
    /// Arrow tip: the top border of the consumer.
    pub tip: (i64, i64),
}

#[derive(Clone, Debug)]
pub(crate) struct Mark {
    pub x: i64,
    pub y: i64,
    pub kind: Kind,
}

#[derive(Clone, Debug)]
pub(crate) struct Tag {
    /// Centre of the pill and the top of its arrow.
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub text: String,
    pub kind: Kind,
}

/// Card body: glyph badge, expression, arrow, and result.
pub(crate) const BADGE: i64 = 11;
pub(crate) const EXPR_X: i64 = 36;
pub(crate) const SPACE: i64 = 6;
/// Advance of the `→` between the expression and the result.
pub(crate) const ARROW: i64 = 13;

struct Column {
    free: u32,
    w: i64,
    x: i64,
}

struct Link {
    from: usize,
    to: usize,
    right: bool,
    /// Last cycle the producer is busy.
    end: u32,
    /// Gap and lane for a value that waits.
    gap: usize,
    lane: usize,
    long: bool,
}

pub(crate) fn symbol(kind: Kind) -> &'static str {
    match kind {
        Kind::Add => "+",
        Kind::Mul => "\u{d7}",
        Kind::Div => "\u{f7}",
    }
}

pub(crate) fn layout(schedule: &Schedule) -> Scene {
    let head = if schedule.title.is_some() { 36 } else { 0 };
    let foot = if schedule.footer.is_some() { 30 } else { 0 };
    let cycles = schedule.cycle_count as usize;
    let ops = &schedule.ops;

    let (mut columns, col_of) = columns(schedule);
    let mut links = links(schedule);
    let mut used = vec![false; ops.len()];
    for link in &links {
        used[link.from] = true;
    }

    // Lanes: a value that skips a cycle drops through the gap beside its reader.
    let gaps = columns.len() + 1;
    let mut gap_lanes: Vec<Vec<(usize, u32, u32)>> = vec![Vec::new(); gaps];
    let mut order: Vec<usize> = (0..links.len()).filter(|&i| links[i].long).collect();
    order.sort_by_key(|&i| (links[i].end, ops[links[i].to].cycle));
    for index in order {
        let link = &links[index];
        let (from_col, to_col) = (col_of[link.from], col_of[link.to]);
        let gap = if from_col < to_col || (from_col == to_col && !link.right) {
            to_col
        } else {
            to_col + 1
        };
        let span = (link.end, ops[link.to].cycle - 1);
        let lane = first_free(&mut gap_lanes[gap], span);
        links[index].gap = gap;
        links[index].lane = lane;
    }
    let lane_count: Vec<i64> = gap_lanes.iter().map(|lanes| lane_total(lanes)).collect();

    // Tracks: one per producer in each routing zone it crosses.
    let mut zone_nets: Vec<Vec<usize>> = vec![Vec::new(); cycles];
    let mut zone_tag = vec![false; cycles];
    for link in &links {
        push_once(&mut zone_nets[link.end as usize], link.from);
        if link.long {
            push_once(&mut zone_nets[ops[link.to].cycle as usize - 1], link.from);
        }
    }
    for (index, op) in ops.iter().enumerate() {
        if !used[index] {
            zone_tag[(op.cycle + op.latency - 1) as usize] = true;
        }
    }

    // Bands.
    let mut bands = Vec::with_capacity(cycles);
    let mut y = head;
    let mut track_y: Vec<HashMap<usize, i64>> = vec![HashMap::new(); cycles];
    for cycle in 0..cycles {
        let tracks = zone_nets[cycle].len() as i64;
        let tag = if zone_tag[cycle] { TAG_ZONE } else { 0 };
        let zone = tag
            + if tracks > 0 {
                12 + (tracks - 1) * TRACK + 14
            } else {
                0
            };
        let h = (PAD_TOP + BOX_H + zone + 10).max(MIN_BAND);
        let start = y + PAD_TOP + BOX_H + tag;
        let room = y + h - start;
        let first = start + (room - (tracks - 1).max(0) * TRACK) / 2;
        for (k, &net) in zone_nets[cycle].iter().enumerate() {
            track_y[cycle].insert(net, first + k as i64 * TRACK);
        }
        bands.push(Band {
            y,
            h,
            cycle: cycle as u32,
            counts: cover(schedule, cycle as u32),
            row: y + PAD_TOP + BOX_H / 2,
        });
        y += h;
    }
    let body_end = y;

    // Columns and gaps.
    let mut gap_x = vec![0i64; gaps];
    let mut gap_w = vec![0i64; gaps];
    let mut x = GUTTER;
    for gap in 0..gaps {
        let lanes = lane_count[gap];
        let w = if lanes > 0 {
            16 + (lanes - 1) * TRACK + 16
        } else if gap == 0 || gap == gaps - 1 {
            8
        } else {
            28
        };
        gap_x[gap] = x;
        gap_w[gap] = w;
        x += w;
        if gap < columns.len() {
            columns[gap].x = x;
            x += columns[gap].w;
        }
    }
    let lane_x = |gap: usize, lane: usize| -> i64 {
        let lanes = lane_count[gap];
        gap_x[gap] + (gap_w[gap] - (lanes - 1) * TRACK) / 2 + lane as i64 * TRACK
    };

    // Usage cells.
    let cell_w = bands
        .iter()
        .flat_map(|band| band.counts.iter().map(|&n| n.to_string()))
        .map(|count| measure(&count, CELL, true) + measure("\u{f7}", CELL, true) + 14)
        .max()
        .unwrap_or(30)
        .max(30);
    let usage_x = x + USAGE_GAP;
    let mut width = usage_x + 3 * cell_w + 2 * CELL_GAP + 8;
    if cycles == 0 {
        width = GUTTER + 48;
    }
    if let Some(title) = &schedule.title {
        width = width.max(measure(title, 15.0, true) + 16);
    }
    if let Some(footer) = &schedule.footer {
        width = width.max(measure(footer, 12.0, false) + 16);
    }
    width = width.max(64);

    // Cards.
    let units: Vec<Unit> = ops
        .iter()
        .enumerate()
        .map(|(index, op)| {
            let column = &columns[col_of[index]];
            let first = &bands[op.cycle as usize];
            let last = &bands[(op.cycle + op.latency - 1) as usize];
            let top = first.y + PAD_TOP;
            Unit {
                x: column.x,
                y: top,
                w: column.w,
                h: last.y + PAD_TOP + BOX_H - top,
                kind: op.kind,
                expr: expression(op),
                result: op.result.clone(),
                latency: op.latency,
                stages: (op.cycle + 1..op.cycle + op.latency)
                    .map(|cycle| bands[cycle as usize].y)
                    .collect(),
            }
        })
        .collect();

    // Wires.
    let mut wires = Vec::with_capacity(links.len());
    let mut nets: HashMap<usize, Vec<Vec<(i64, i64)>>> = HashMap::new();
    for link in &links {
        let from = &units[link.from];
        let to = &units[link.to];
        let exit = (from.x + from.w / 2, from.y + from.h - 2);
        let port_x = if link.right {
            to.x + (to.w * 2) / 3
        } else {
            to.x + to.w / 3
        };
        let out_track = track_y[link.end as usize][&link.from];
        let mut points = vec![exit, (exit.0, out_track)];
        if link.long {
            let lane = lane_x(link.gap, link.lane);
            let in_track = track_y[ops[link.to].cycle as usize - 1][&link.from];
            points.push((lane, out_track));
            points.push((lane, in_track));
            points.push((port_x, in_track));
        } else {
            points.push((port_x, out_track));
        }
        points.push((port_x, to.y - 6));
        nets.entry(link.from).or_default().push(points.clone());
        wires.push(Wire {
            points,
            kind: from.kind,
            tip: (port_x, to.y),
        });
    }

    // Branch dots, and a register wherever a value crosses a clock edge.
    let mut dots = Vec::new();
    let mut registers: Vec<Mark> = Vec::new();
    let mut producers: Vec<usize> = nets.keys().copied().collect();
    producers.sort_unstable();
    for net in producers {
        let paths = &nets[&net];
        let kind = units[net].kind;
        for (x, y) in crate::draw::junctions(paths) {
            dots.push(Mark { x, y, kind });
        }
        for path in paths {
            for pair in path.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if a.0 != b.0 {
                    continue;
                }
                let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
                for band in bands.iter().skip(1) {
                    if y0 < band.y
                        && band.y < y1
                        && !registers
                            .iter()
                            .any(|mark| mark.x == a.0 && mark.y == band.y)
                    {
                        registers.push(Mark {
                            x: a.0,
                            y: band.y,
                            kind,
                        });
                    }
                }
            }
        }
    }

    // A result that nothing reads leaves through a pill under its card.
    let tags = units
        .iter()
        .enumerate()
        .filter(|(index, _)| !used[*index])
        .map(|(_, unit)| Tag {
            x: unit.x + unit.w / 2,
            y: unit.y + unit.h,
            w: snap(measure(&unit.result, CELL, true) + 16).max(28),
            text: unit.result.clone(),
            kind: unit.kind,
        })
        .collect();

    let height = (body_end + foot).max(32);
    Scene {
        width,
        height,
        title: schedule.title.clone(),
        title_at: (width / 2, 20),
        footer: schedule.footer.clone(),
        footer_at: (width / 2, body_end + 20),
        bands,
        usage_x,
        cell_w,
        units,
        wires,
        dots,
        registers,
        tags,
    }
}

/// Lowest lane in a gap whose busy cycles miss `span`.
fn first_free(lanes: &mut Vec<(usize, u32, u32)>, span: (u32, u32)) -> usize {
    let mut lane = 0;
    while lanes
        .iter()
        .any(|&(owner, a, b)| owner == lane && a <= span.1 && span.0 <= b)
    {
        lane += 1;
    }
    lanes.push((lane, span.0, span.1));
    lane
}

fn lane_total(lanes: &[(usize, u32, u32)]) -> i64 {
    lanes
        .iter()
        .map(|&(lane, _, _)| lane as i64 + 1)
        .max()
        .unwrap_or(0)
}

fn push_once(list: &mut Vec<usize>, value: usize) {
    if !list.contains(&value) {
        list.push(value);
    }
}

fn links(schedule: &Schedule) -> Vec<Link> {
    let mut defined: HashMap<&str, usize> = HashMap::new();
    let mut links = Vec::new();
    for (index, op) in schedule.ops.iter().enumerate() {
        for (right, operand) in [(false, &op.left), (true, &op.right)] {
            let Some(name) = operand_name(operand) else {
                continue;
            };
            if let Some(&from) = defined.get(name) {
                let producer = &schedule.ops[from];
                let end = producer.cycle + producer.latency - 1;
                links.push(Link {
                    from,
                    to: index,
                    right,
                    end,
                    gap: 0,
                    lane: 0,
                    long: op.cycle > end + 1,
                });
            }
        }
        defined.insert(op.result.as_str(), index);
    }
    links
}

pub(crate) fn cover(schedule: &Schedule, cycle: u32) -> [u32; 3] {
    let mut counts = [0; 3];
    for op in &schedule.ops {
        if op.cycle <= cycle && cycle < op.cycle + op.latency {
            counts[kind_index(op.kind)] += 1;
        }
    }
    counts
}

pub(crate) fn kind_index(kind: Kind) -> usize {
    match kind {
        Kind::Add => 0,
        Kind::Mul => 1,
        Kind::Div => 2,
    }
}

fn columns(schedule: &Schedule) -> (Vec<Column>, Vec<usize>) {
    let mut columns: Vec<Column> = Vec::new();
    let mut col_of = Vec::with_capacity(schedule.ops.len());
    for op in &schedule.ops {
        let width = card_width(&expression(op), &op.result, op.latency);
        let slot = columns.iter().position(|column| column.free <= op.cycle);
        let col = if let Some(slot) = slot {
            let column = &mut columns[slot];
            column.free = op.cycle + op.latency;
            column.w = column.w.max(width);
            slot
        } else {
            columns.push(Column {
                free: op.cycle + op.latency,
                w: width,
                x: 0,
            });
            columns.len() - 1
        };
        col_of.push(col);
    }
    (columns, col_of)
}

fn expression(op: &Op) -> String {
    format!("{} {} {}", show(&op.left), symbol(op.kind), show(&op.right))
}

fn show(operand: &Operand) -> String {
    match operand {
        Operand::Const(value) => value.to_string(),
        Operand::Name(name) => name.clone(),
    }
}

pub(crate) fn card_width(expr: &str, result: &str, latency: u32) -> i64 {
    let body =
        EXPR_X + measure(expr, LABEL, false) + SPACE + ARROW + measure(result, LABEL, true) + 14;
    let busy = if latency > 1 {
        measure(&format!("{latency} cycles"), 10.0, false) + 24
    } else {
        0
    };
    snap(body.max(busy)).max(84)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hls;

    const SCALED: &str = "\
@hls
@title Scaled sum of products

0:
  + a b -> s0
  + c d -> s1
  * e f -> p
1:
  * s0 s1 -> m
2:
  + m p -> t
3:
  / t n -> y !3
";

    fn crosses(points: &[(i64, i64)], unit: &Unit) -> bool {
        // A wire starts two pixels inside its producer, under the card.
        let mut points = points.to_vec();
        points[0].1 += 3;
        points.windows(2).any(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let (x0, x1) = (a.0.min(b.0) - 1, a.0.max(b.0) + 1);
            let (y0, y1) = (a.1.min(b.1) - 1, a.1.max(b.1) + 1);
            x1 > unit.x + 1
                && x0 < unit.x + unit.w - 1
                && y1 > unit.y + 1
                && y0 < unit.y + unit.h - 1
        })
    }

    #[test]
    fn divider_covers_three_bands_and_wires_stay_out_of_the_cards() {
        let schedule = hls::parse(SCALED).unwrap();
        let scene = layout(&schedule);
        assert_eq!(schedule.cycle_count, 6);
        let divider = scene
            .units
            .iter()
            .find(|unit| unit.expr == "t \u{f7} n")
            .unwrap();
        assert_eq!(divider.stages.len(), 2);
        assert_eq!(divider.y + divider.h, scene.bands[5].y + PAD_TOP + BOX_H);
        assert_eq!(scene.bands[0].counts, [2, 1, 0]);
        assert_eq!(scene.bands[1].counts, [0, 1, 0]);
        for cycle in 3..6 {
            assert_eq!(scene.bands[cycle].counts, [0, 0, 1]);
        }
        assert_eq!(scene.wires.len(), 5);
        for wire in &scene.wires {
            for unit in &scene.units {
                assert!(
                    !crosses(&wire.points, unit),
                    "wire {wire:?} crosses {unit:?}"
                );
            }
        }
        // p waits through cycle 1, so it is registered twice on its lane.
        let p = scene
            .wires
            .iter()
            .find(|wire| wire.kind == Kind::Mul && wire.points.len() > 4)
            .expect("p rides a lane");
        let lane_x = p.points[2].0;
        assert_eq!(
            scene
                .registers
                .iter()
                .filter(|mark| mark.kind == Kind::Mul && (mark.x == lane_x || mark.x == p.tip.0))
                .count(),
            2
        );
        assert_eq!(scene.tags.len(), 1);
        assert_eq!(scene.tags[0].text, "y");
    }

    #[test]
    fn lanes_never_share_a_run() {
        let schedule = hls::parse(
            "@hls\n0:\n  + a b -> x\n  + c d -> y\n  + e f -> z\n1:\n  + g h -> q\n3:\n  + x y -> u\n  * z q -> v\n",
        )
        .unwrap();
        let scene = layout(&schedule);
        let mut runs: Vec<(i64, i64, i64, usize)> = Vec::new();
        for (index, wire) in scene.wires.iter().enumerate() {
            for pair in wire.points.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if a.0 == b.0 && a.1 != b.1 {
                    runs.push((a.0, a.1.min(b.1), a.1.max(b.1), index));
                }
            }
        }
        for (i, a) in runs.iter().enumerate() {
            for b in &runs[i + 1..] {
                let same_net = scene.wires[a.3].points[0] == scene.wires[b.3].points[0];
                if a.0 == b.0 && !same_net {
                    assert!(a.2 <= b.1 || b.2 <= a.1, "{a:?} overlaps {b:?}");
                }
            }
        }
        for wire in &scene.wires {
            for unit in &scene.units {
                assert!(
                    !crosses(&wire.points, unit),
                    "wire {wire:?} crosses {unit:?}"
                );
            }
        }
    }
}
