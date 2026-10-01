//! Timing-diagram lanes, edges, and captions shared by the parser and the SVG writer.

use crate::Error;

#[derive(Clone, Debug)]
pub(crate) struct Doc {
    pub lanes: Vec<Lane>,
    pub groups: Vec<Group>,
    pub edges: Vec<Edge>,
    pub head: Cap,
    pub foot: Cap,
    pub hscale: i32,
    pub xmin: i64,
    pub xmax_cfg: i64,
    pub marks: bool,
    pub arc_font: f64,
    pub gaps: Option<String>,
    pub notes: Notes,
}

impl Default for Doc {
    fn default() -> Self {
        Self {
            lanes: Vec::new(),
            groups: Vec::new(),
            edges: Vec::new(),
            head: Cap::default(),
            foot: Cap::default(),
            hscale: 1,
            xmin: 0,
            xmax_cfg: 1_000_000_000_000,
            marks: true,
            arc_font: 11.0,
            gaps: None,
            notes: Notes::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Notes {
    pub wvf: Option<Remark>,
    pub title: Remark,
    pub footer: Remark,
    pub tick: Remark,
    pub tock: Remark,
    pub foot_tick: Remark,
    pub foot_tock: Remark,
    pub every: Remark,
    pub foot_every: Remark,
    pub scale: Remark,
    pub bounds: Remark,
    pub grid: Remark,
    pub arc_font: Remark,
    pub gaps: Remark,
    pub empty: Remark,
    pub end: Vec<String>,
}

/// Comment lines that belong to the next construct, plus an end-of-line note.
#[derive(Clone, Debug, Default)]
pub(crate) struct Remark {
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

impl Remark {
    pub(crate) fn is_empty(&self) -> bool {
        self.leading.is_empty() && self.trailing.is_none()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Edge {
    pub from: String,
    pub to: String,
    pub shape: String,
    pub label: String,
    pub offset: usize,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Lane {
    pub name: String,
    pub body: Body,
    pub data: Vec<String>,
    pub period: f64,
    pub phase: f64,
    pub node: Option<String>,
    pub node_at: usize,
    pub over: Option<String>,
    pub under: Option<String>,
    pub indent: i64,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Body {
    None,
    Wave(String),
    Path(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Group {
    pub x: i64,
    pub y: i64,
    pub height: i64,
    pub name: Option<String>,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
    pub end_leading: Vec<String>,
    pub end_trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Cap {
    pub text: Option<String>,
    pub tick: Tick,
    pub tock: Tick,
    pub every: f64,
}

impl Default for Cap {
    fn default() -> Self {
        Self {
            text: None,
            tick: Tick::Off,
            tock: Tick::Off,
            every: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Tick {
    Off,
    Series {
        offset: f64,
        step: f64,
        dp: usize,
        fixed: bool,
    },
    Labels(Vec<String>),
}

pub(crate) enum Slot {
    Skip,
    Name(String),
}

pub(crate) struct SlotError {
    pub rel: usize,
    pub message: String,
}

/// `.` skips a cycle, `[name]` is one word, and any other character is one letter.
pub(crate) fn node_slots(node: &str) -> Result<Vec<Slot>, SlotError> {
    let mut slots = Vec::new();
    let mut rest = node;
    let mut rel = 0;
    while !rest.is_empty() {
        let ch = rest.chars().next().unwrap();
        let width = ch.len_utf8();
        if ch == '.' {
            slots.push(Slot::Skip);
            rest = &rest[width..];
            rel += width;
            continue;
        }
        if ch == '[' {
            let start = rel;
            let Some(end) = rest.find(']') else {
                return Err(SlotError {
                    rel: start,
                    message: "unclosed '[' in node".to_string(),
                });
            };
            let name = rest[1..end].trim();
            if name.is_empty() {
                return Err(SlotError {
                    rel: start,
                    message: "empty node name".to_string(),
                });
            }
            if !is_word(name) {
                return Err(SlotError {
                    rel: start,
                    message: format!("node name {name:?} must be letters, digits, or underscore"),
                });
            }
            slots.push(Slot::Name(name.to_string()));
            let consumed = end + 1;
            rest = &rest[consumed..];
            rel += consumed;
            continue;
        }
        if ch == ']' {
            return Err(SlotError {
                rel,
                message: "unexpected ']' in node".to_string(),
            });
        }
        slots.push(Slot::Name(ch.to_string()));
        rest = &rest[width..];
        rel += width;
    }
    Ok(slots)
}

fn is_word(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphanumeric() || first == '_') && chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

fn is_edge_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if chars.next().is_none() {
        return !first.is_whitespace();
    }
    is_word(name)
}

const EDGE_SHAPES: &[&str] = &[
    "<-|->", "<-|>", "<-~>", "<~->", "<|->", "-|->", "<->", "-~>", "~->", "-|>", "|->", "<~>",
    "-|-", "->", "~>", "-~", "~-", "-|", "|-", "+", "-", "~",
];

pub(crate) fn parse_edge(text: &str, offset: usize) -> Result<Edge, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error {
            offset,
            message: "@edge needs two nodes and a connector, for example a~>b label".to_string(),
        });
    }
    let head = text.split_whitespace().next().unwrap();
    let label = edge_label(text[head.len()..].trim_start());
    let (from, shape, to) = split_connector(head).map_err(|message| Error { offset, message })?;
    Ok(Edge {
        from,
        to,
        shape,
        label,
        offset,
        leading: Vec::new(),
        trailing: None,
    })
}

fn edge_label(raw: &str) -> String {
    let raw = raw.trim();
    if raw.len() >= 2 {
        let quote = raw.chars().next().unwrap();
        let quote_len = quote.len_utf8();
        if (quote == '"' || quote == '\'') && raw.ends_with(quote) {
            let inner = &raw[quote_len..raw.len() - quote_len];
            if !inner.contains(quote) {
                return inner.to_string();
            }
        }
    }
    raw.to_string()
}

fn split_connector(head: &str) -> Result<(String, String, String), String> {
    let mut best: Option<(usize, usize, &str)> = None;
    for shape in EDGE_SHAPES {
        let mut from = 0;
        while let Some(found) = head[from..].find(shape) {
            let index = from + found;
            let left = &head[..index];
            let right = &head[index + shape.len()..];
            if is_edge_name(left) && is_edge_name(right) {
                let better = match best {
                    None => true,
                    Some((len, at, _)) => shape.len() > len || (shape.len() == len && index < at),
                };
                if better {
                    best = Some((shape.len(), index, shape));
                }
            }
            from = index + shape.len().max(1);
            if from >= head.len() {
                break;
            }
        }
    }
    if let Some((_, index, shape)) = best {
        return Ok((
            head[..index].to_string(),
            shape.to_string(),
            head[index + shape.len()..].to_string(),
        ));
    }
    let mut chars = head.chars();
    if let (Some(left), Some(right), None) = (chars.next(), chars.next(), chars.next()) {
        return Ok((left.to_string(), String::new(), right.to_string()));
    }
    Err(format!(
        "unknown connector in `{head}`; expected a name, one of {}, and a name",
        EDGE_SHAPES.join(" ")
    ))
}
