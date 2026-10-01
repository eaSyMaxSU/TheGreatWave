//! One downward walk that places cycle rules, operator boxes, and wires.
//!
//! Every coordinate is an integer. A wire is a run of 1px rectangles that
//! stays on the border or in the gap, so the paper inside a box stays paper.

use crate::hls::{operand_name, Kind, Op, Operand, Schedule};
use crate::text_width;
use std::collections::HashMap;

const BAND: i64 = 80;
const RULE: i64 = 20;
const BOX_TOP: i64 = 28;
const BOX_H: i64 = 32;
const COL_GAP: i64 = 16;
const PORT: i64 = 8;
const STUB: i64 = 12;
const CAPTION: f64 = 11.0;

#[derive(Clone, Debug)]
pub(crate) struct Scene {
    pub width: i64,
    pub height: i64,
    pub title: Option<String>,
    pub title_at: (i64, i64),
    pub footer: Option<String>,
    pub footer_at: (i64, i64),
    pub rules: Vec<Span>,
    pub captions: Vec<Caption>,
    pub units: Vec<Unit>,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub x: i64,
    pub y: i64,
    pub w: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Caption {
    pub text: String,
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Unit {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Bar {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

struct Column {
    free: u32,
    w: i64,
    x: i64,
}

struct Placed {
    col: usize,
    text: String,
}

struct Use {
    from: usize,
    to: usize,
    right: bool,
}

pub(crate) fn layout(schedule: &Schedule) -> Scene {
    let head = if schedule.title.is_some() { 28 } else { 0 };
    let foot = if schedule.footer.is_some() { 28 } else { 0 };
    let captions = captions(schedule, head);
    let gutter = captions
        .iter()
        .map(|caption| text_width(&caption.text, CAPTION).ceil() as i64 + 12)
        .max()
        .unwrap_or(0);
    let (columns, placed) = columns(schedule);
    let mut x = gutter;
    let mut columns = columns;
    for column in &mut columns {
        column.x = x;
        x += column.w + COL_GAP;
    }
    let content = if columns.is_empty() {
        gutter + 48
    } else {
        x - COL_GAP
    };
    let mut width = content.max(64);
    if let Some(title) = &schedule.title {
        width = width.max(text_width(title, 14.0).ceil() as i64 + 16);
    }
    if let Some(footer) = &schedule.footer {
        width = width.max(text_width(footer, 14.0).ceil() as i64 + 16);
    }
    let units = units(schedule, &placed, &columns, head);
    let body = i64::from(schedule.cycle_count) * BAND;
    let height = (head + body + foot).max(32);
    let mut scene = Scene {
        width,
        height,
        title: schedule.title.clone(),
        title_at: (width / 2, 16),
        footer: schedule.footer.clone(),
        footer_at: (width / 2, head + body + 16),
        rules: rules(schedule, &units, head, width),
        captions,
        units,
        bars: Vec::new(),
    };
    scene.bars = wires(schedule, &scene.units);
    scene
}

fn captions(schedule: &Schedule, head: i64) -> Vec<Caption> {
    (0..schedule.cycle_count)
        .map(|cycle| {
            let (adds, muls, divs) = cover(schedule, cycle);
            Caption {
                text: format!("{cycle}  +{adds}  *{muls}  /{divs}"),
                x: 0,
                y: head + i64::from(cycle) * BAND + 12,
            }
        })
        .collect()
}

fn cover(schedule: &Schedule, cycle: u32) -> (u32, u32, u32) {
    let mut adds = 0;
    let mut muls = 0;
    let mut divs = 0;
    for op in &schedule.ops {
        if op.cycle <= cycle && cycle < op.cycle + op.latency {
            match op.kind {
                Kind::Add => adds += 1,
                Kind::Mul => muls += 1,
                Kind::Div => divs += 1,
            }
        }
    }
    (adds, muls, divs)
}

fn columns(schedule: &Schedule) -> (Vec<Column>, Vec<Placed>) {
    let mut columns: Vec<Column> = Vec::new();
    let mut placed = Vec::with_capacity(schedule.ops.len());
    for op in &schedule.ops {
        let text = label(op);
        let width = box_width(&text);
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
        placed.push(Placed { col, text });
    }
    (columns, placed)
}

fn units(schedule: &Schedule, placed: &[Placed], columns: &[Column], head: i64) -> Vec<Unit> {
    schedule
        .ops
        .iter()
        .zip(placed)
        .map(|(op, place)| {
            let column = &columns[place.col];
            Unit {
                x: column.x,
                y: head + i64::from(op.cycle) * BAND + BOX_TOP,
                w: column.w,
                h: i64::from(op.latency - 1) * BAND + BOX_H,
                text: place.text.clone(),
            }
        })
        .collect()
}

fn rules(schedule: &Schedule, units: &[Unit], head: i64, width: i64) -> Vec<Span> {
    let mut spans = Vec::new();
    for cycle in 0..schedule.cycle_count {
        let y = head + i64::from(cycle) * BAND + RULE;
        let mut cuts: Vec<(i64, i64)> = units
            .iter()
            .filter(|unit| unit.y <= y && y < unit.y + unit.h)
            .map(|unit| (unit.x, unit.x + unit.w))
            .collect();
        cuts.sort_unstable();
        let mut cursor = 0;
        for (start, end) in cuts {
            let start = start.clamp(0, width);
            let end = end.clamp(0, width);
            if start > cursor {
                spans.push(Span {
                    x: cursor,
                    y,
                    w: start - cursor,
                });
            }
            cursor = cursor.max(end);
        }
        if width > cursor {
            spans.push(Span {
                x: cursor,
                y,
                w: width - cursor,
            });
        }
    }
    spans
}

fn wires(schedule: &Schedule, units: &[Unit]) -> Vec<Bar> {
    let mut defined: HashMap<&str, usize> = HashMap::new();
    let mut uses = Vec::new();
    let mut used = vec![false; schedule.ops.len()];
    for (index, op) in schedule.ops.iter().enumerate() {
        for (right, operand) in [(false, &op.left), (true, &op.right)] {
            let Some(name) = operand_name(operand) else {
                continue;
            };
            if let Some(&from) = defined.get(name) {
                uses.push(Use {
                    from,
                    to: index,
                    right,
                });
                used[from] = true;
            }
        }
        defined.insert(op.result.as_str(), index);
    }
    let mut bars = Vec::new();
    for link in &uses {
        route(&units[link.from], &units[link.to], link.right, &mut bars);
    }
    for (index, live) in used.iter().enumerate() {
        if *live {
            continue;
        }
        let unit = &units[index];
        vbar(
            unit.x + unit.w / 2,
            unit.y + unit.h - 1,
            unit.y + unit.h - 1 + STUB,
            &mut bars,
        );
    }
    bars
}

fn route(prod: &Unit, cons: &Unit, right: bool, bars: &mut Vec<Bar>) {
    let port_x = if right {
        cons.x + cons.w - PORT
    } else {
        cons.x + PORT
    };
    if prod.x == cons.x {
        vbar(port_x, prod.y + prod.h - 1, cons.y, bars);
        return;
    }
    let exit_x = prod.x + prod.w / 2;
    let track = prod.y + prod.h + 3;
    let approach = cons.y - 4;
    let channel = if cons.x > prod.x {
        cons.x - COL_GAP / 2
    } else {
        cons.x + cons.w + COL_GAP / 2
    };
    vbar(exit_x, prod.y + prod.h - 1, track, bars);
    hbar(exit_x, channel, track, bars);
    vbar(channel, track, approach, bars);
    hbar(channel, port_x, approach, bars);
    vbar(port_x, approach, cons.y, bars);
}

fn hbar(x0: i64, x1: i64, y: i64, bars: &mut Vec<Bar>) {
    let x = x0.min(x1);
    let w = (x0 - x1).abs() + 1;
    if w > 0 {
        bars.push(Bar { x, y, w, h: 1 });
    }
}

fn vbar(x: i64, y0: i64, y1: i64, bars: &mut Vec<Bar>) {
    let y = y0.min(y1);
    let h = (y0 - y1).abs() + 1;
    if h > 0 {
        bars.push(Bar { x, y, w: 1, h });
    }
}

fn label(op: &Op) -> String {
    format!(
        "{} {} {} -> {}",
        op.kind.symbol(),
        show(&op.left),
        show(&op.right),
        op.result
    )
}

fn show(operand: &Operand) -> String {
    match operand {
        Operand::Const(value) => value.to_string(),
        Operand::Name(name) => name.clone(),
    }
}

fn box_width(text: &str) -> i64 {
    snap(text_width(text, 12.0).ceil() as i64 + 16).max(48)
}

fn snap(n: i64) -> i64 {
    (n.max(0) + 3) & !3
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

    #[test]
    fn divider_covers_three_bands_and_wires_stay_out_of_the_paper() {
        let schedule = hls::parse(SCALED).unwrap();
        let scene = layout(&schedule);
        assert_eq!(schedule.cycle_count, 6);
        let divider = scene
            .units
            .iter()
            .find(|unit| unit.text.contains("/ t n"))
            .unwrap();
        assert_eq!(divider.h, 2 * BAND + BOX_H);
        assert!(scene.captions[0].text.contains("+2"));
        assert!(scene.captions[0].text.contains("*1"));
        assert!(scene.captions[0].text.contains("/0"));
        for cycle in 3..6 {
            assert!(
                scene.captions[cycle as usize].text.contains("/1"),
                "{}",
                scene.captions[cycle as usize].text
            );
        }
        assert!(scene.captions[1].text.contains("+0"));
        for bar in &scene.bars {
            for unit in &scene.units {
                let x1 = bar.x + bar.w;
                let y1 = bar.y + bar.h;
                let crosses = x1 > unit.x + 1
                    && bar.x < unit.x + unit.w - 1
                    && y1 > unit.y + 1
                    && bar.y < unit.y + unit.h - 1;
                assert!(!crosses, "wire {bar:?} crosses {unit:?}");
            }
        }
    }
}
