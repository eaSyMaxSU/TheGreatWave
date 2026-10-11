//! Line-oriented gate netlists.
//!
//! Each gate is `kind inputs -> outputs`. The kind reports how many ports are
//! legal. The lists are what a later kind with more ports will fill in.

use crate::format::quoted;
use crate::native::{raw_lines, split_comment, tokens, Raw};
use crate::scan::Remark;
use crate::Error;
use std::borrow::Cow;
use std::collections::HashMap;

const MAX_GATES: usize = 256;
const MAX_DEPTH: u32 = 64;
const MAX_PORTS: usize = 8;
const KINDS: &str = "and, or, not, nand, nor, xor, xnor, mux";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    And,
    Or,
    Not,
    Nand,
    Nor,
    Xor,
    Xnor,
    Mux,
}

impl Kind {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "and" => Self::And,
            "or" => Self::Or,
            "not" => Self::Not,
            "nand" => Self::Nand,
            "nor" => Self::Nor,
            "xor" => Self::Xor,
            "xnor" => Self::Xnor,
            "mux" => Self::Mux,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Not => "not",
            Self::Nand => "nand",
            Self::Nor => "nor",
            Self::Xor => "xor",
            Self::Xnor => "xnor",
            Self::Mux => "mux",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::And => "AND",
            Self::Or => "OR",
            Self::Not => "NOT",
            Self::Nand => "NAND",
            Self::Nor => "NOR",
            Self::Xor => "XOR",
            Self::Xnor => "XNOR",
            Self::Mux => "MUX",
        }
    }

    /// Legal input count for this kind. The parser asks; it does not assume two.
    pub(crate) fn input_count(self) -> usize {
        match self {
            Self::Not => 1,
            Self::Mux => 3,
            Self::And | Self::Or | Self::Nand | Self::Nor | Self::Xor | Self::Xnor => 2,
        }
    }

    /// Legal output count. Every kind has one today.
    pub(crate) fn output_count(self) -> usize {
        1
    }

    pub(crate) fn invert(self) -> bool {
        matches!(self, Self::Not | Self::Nand | Self::Nor | Self::Xnor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    Name(String),
    Const(i64),
}

#[derive(Clone, Debug)]
pub(crate) struct Gate {
    pub kind: Kind,
    pub inputs: Vec<Operand>,
    pub outputs: Vec<String>,
    /// Column in the left-to-right flow. Primary inputs are column 0, so the
    /// first rank of gates is column 1.
    pub column: u32,
    pub at: usize,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Netlist {
    pub title: Option<String>,
    pub footer: Option<String>,
    pub notes: Notes,
    pub gates: Vec<Gate>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Notes {
    pub gtl: Remark,
    pub title: Remark,
    pub footer: Remark,
    pub end: Vec<String>,
}

pub(crate) fn starts_with_gtl(source: &str) -> bool {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut offset = 0;
    for raw in source.split_inclusive('\n') {
        let at = offset;
        offset += raw.len();
        let Ok((code, _)) = split_comment(raw, at) else {
            return false;
        };
        let line = code.trim();
        if line.is_empty() {
            continue;
        }
        let rest = line.strip_prefix("@gtl");
        return rest.is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
    }
    false
}

pub(crate) fn parse(source: &str) -> Result<Netlist, Error> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let lines = raw_lines(source)?;
    let Some(first) = lines.first() else {
        return Err(err(0, "@gtl takes no arguments"));
    };
    if !is_gtl(&first.text) {
        return Err(err(first.at, "a gate netlist starts with @gtl"));
    }
    if first.text != "@gtl" {
        return Err(err(first.at, "@gtl takes no arguments"));
    }
    let mut net = Netlist {
        title: None,
        footer: None,
        notes: Notes {
            gtl: Remark {
                leading: first.leading.clone(),
                trailing: first.trailing.clone(),
            },
            ..Notes::default()
        },
        gates: Vec::new(),
    };
    for line in lines.iter().skip(1) {
        if line.text.is_empty() {
            net.notes.end = line.leading.clone();
            break;
        }
        if let Some(directive) = line.text.strip_prefix('@') {
            if !net.gates.is_empty() {
                return Err(err(line.at, "directives belong before the gates"));
            }
            apply_directive(&mut net, directive, line)?;
            continue;
        }
        if net.gates.len() == MAX_GATES {
            return Err(err(line.at, "netlist exceeds 256 gates"));
        }
        let mut gate = parse_gate(line)?;
        gate.leading = line.leading.clone();
        gate.trailing = line.trailing.clone();
        net.gates.push(gate);
    }
    assign_columns(&mut net.gates)?;
    Ok(net)
}

fn is_gtl(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("@gtl") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

fn apply_directive(net: &mut Netlist, directive: &str, line: &Raw) -> Result<(), Error> {
    let (key, value) = directive
        .split_once(char::is_whitespace)
        .unwrap_or((directive, ""));
    let value = value.trim();
    let value_at = line.at + line.text.len() - value.len();
    match key {
        "gtl" => Err(err(line.at, "duplicate @gtl")),
        "title" => {
            if net.title.is_some() {
                return Err(err(line.at, "duplicate @title"));
            }
            net.title = Some(if value.is_empty() {
                String::new()
            } else {
                crate::native::text(value, value_at)?
            });
            net.notes.title = Remark {
                leading: line.leading.clone(),
                trailing: line.trailing.clone(),
            };
            Ok(())
        }
        "footer" => {
            if net.footer.is_some() {
                return Err(err(line.at, "duplicate @footer"));
            }
            net.footer = Some(if value.is_empty() {
                String::new()
            } else {
                crate::native::text(value, value_at)?
            });
            net.notes.footer = Remark {
                leading: line.leading.clone(),
                trailing: line.trailing.clone(),
            };
            Ok(())
        }
        "end" | "asm" | "hls" | "wvf" => Err(err(
            line.at,
            &format!("@{key} is not used in a gate netlist"),
        )),
        _ => Err(err(line.at, &format!("unknown directive @{key}"))),
    }
}

fn parse_gate(line: &Raw) -> Result<Gate, Error> {
    let parts = tokens(&line.text, line.at)?;
    if parts.is_empty() {
        return Err(err(
            line.at,
            "a gate needs a kind, inputs, ->, and an output",
        ));
    }
    let kind = Kind::parse(&parts[0]).ok_or_else(|| {
        err(
            line.at,
            &format!("unknown gate \"{}\"; gates are {KINDS}", parts[0]),
        )
    })?;
    let arrow = parts
        .iter()
        .position(|part| part == "->")
        .ok_or_else(|| err(line.at, "a gate needs a kind, inputs, ->, and an output"))?;
    if arrow == 0 {
        return Err(err(
            line.at,
            "a gate needs a kind, inputs, ->, and an output",
        ));
    }
    let inputs = &parts[1..arrow];
    let outputs = &parts[arrow + 1..];
    if inputs.len() > MAX_PORTS {
        return Err(err(line.at, "a gate exceeds 8 inputs"));
    }
    if outputs.len() > MAX_PORTS {
        return Err(err(line.at, "a gate exceeds 8 outputs"));
    }
    if inputs.len() != kind.input_count() {
        return Err(err(
            line.at,
            &format!(
                "{} takes {}",
                kind.name(),
                count_phrase(kind.input_count(), "input")
            ),
        ));
    }
    if outputs.len() != kind.output_count() {
        return Err(err(
            line.at,
            &format!(
                "{} takes {}",
                kind.name(),
                count_phrase(kind.output_count(), "output")
            ),
        ));
    }
    let mut gate_inputs = Vec::with_capacity(inputs.len());
    for token in inputs {
        gate_inputs.push(operand(token, matches!(token, Cow::Owned(_))));
    }
    let mut gate_outputs = Vec::with_capacity(outputs.len());
    for token in outputs {
        if token.is_empty() {
            return Err(err(line.at, "result needs a name"));
        }
        if matches!(token, Cow::Borrowed(_)) && token.as_ref() == "->" {
            return Err(err(line.at, "result needs a name"));
        }
        gate_outputs.push(token.to_string());
    }
    Ok(Gate {
        kind,
        inputs: gate_inputs,
        outputs: gate_outputs,
        column: 0,
        at: line.at,
        leading: Vec::new(),
        trailing: None,
    })
}

fn count_phrase(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn operand(token: &str, quoted: bool) -> Operand {
    if !quoted {
        if let Some(value) = parse_const(token) {
            return Operand::Const(value);
        }
    }
    Operand::Name(token.to_string())
}

fn parse_const(token: &str) -> Option<i64> {
    let digits = token.strip_prefix('-').unwrap_or(token);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    token.parse().ok()
}

fn assign_columns(gates: &mut [Gate]) -> Result<(), Error> {
    let columns = {
        let mut writers: HashMap<&str, usize> = HashMap::new();
        for (index, gate) in gates.iter().enumerate() {
            for out in &gate.outputs {
                if writers.insert(out.as_str(), index).is_some() {
                    return Err(err(gate.at, &format!("duplicate result \"{out}\"")));
                }
            }
        }
        for (index, gate) in gates.iter().enumerate() {
            for input in &gate.inputs {
                let Some(name) = operand_name(input) else {
                    continue;
                };
                if gate.outputs.iter().any(|out| out == name) {
                    return Err(err(
                        gate.at,
                        &format!("gate reads its own output \"{name}\""),
                    ));
                }
                if let Some(&src) = writers.get(name) {
                    if src >= index {
                        let known = names_before(gates, index);
                        return Err(err(
                            gate.at,
                            &format!("result \"{name}\" is not defined yet; results are {known}"),
                        ));
                    }
                }
            }
        }
        let mut col = vec![0u32; gates.len()];
        for (index, gate) in gates.iter().enumerate() {
            let mut depth = 1u32;
            for input in &gate.inputs {
                let Some(name) = operand_name(input) else {
                    continue;
                };
                if let Some(&src) = writers.get(name) {
                    depth = depth.max(col[src] + 1);
                }
            }
            if depth > MAX_DEPTH {
                return Err(err(gate.at, "logic depth past 64"));
            }
            col[index] = depth;
        }
        col
    };
    for (gate, depth) in gates.iter_mut().zip(columns) {
        gate.column = depth;
    }
    Ok(())
}

fn names_before(gates: &[Gate], index: usize) -> String {
    let names: Vec<&str> = gates[..index]
        .iter()
        .flat_map(|gate| gate.outputs.iter().map(String::as_str))
        .collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

pub(crate) fn operand_name(operand: &Operand) -> Option<&str> {
    match operand {
        Operand::Name(name) => Some(name),
        Operand::Const(_) => None,
    }
}

fn err(offset: usize, message: &str) -> Error {
    Error {
        offset,
        message: message.into(),
    }
}

pub(crate) fn write(net: &Netlist) -> String {
    let mut out = String::new();
    write_comments(&mut out, &net.notes.gtl.leading);
    out.push_str("@gtl");
    write_trailing(&mut out, &net.notes.gtl.trailing);
    out.push('\n');
    if let Some(title) = &net.title {
        write_directive(&mut out, &net.notes.title, "title", title);
    }
    if let Some(footer) = &net.footer {
        write_directive(&mut out, &net.notes.footer, "footer", footer);
    }
    if !net.gates.is_empty() {
        out.push('\n');
    }
    for gate in &net.gates {
        write_comments(&mut out, &gate.leading);
        out.push_str(gate.kind.name());
        for input in &gate.inputs {
            out.push(' ');
            write_operand(&mut out, input);
        }
        out.push_str(" ->");
        for output in &gate.outputs {
            out.push(' ');
            write_name(&mut out, output);
        }
        write_trailing(&mut out, &gate.trailing);
        out.push('\n');
    }
    write_comments(&mut out, &net.notes.end);
    out
}

fn write_directive(out: &mut String, remark: &Remark, key: &str, value: &str) {
    write_comments(out, &remark.leading);
    out.push('@');
    out.push_str(key);
    if !value.is_empty() {
        out.push(' ');
        write_phrase(out, value);
    }
    write_trailing(out, &remark.trailing);
    out.push('\n');
}

fn write_operand(out: &mut String, operand: &Operand) {
    match operand {
        Operand::Const(value) => out.push_str(&value.to_string()),
        Operand::Name(name) => write_name(out, name),
    }
}

fn write_name(out: &mut String, name: &str) {
    if name_needs_quotes(name) {
        quoted(out, name);
    } else {
        out.push_str(name);
    }
}

fn name_needs_quotes(name: &str) -> bool {
    if name.is_empty() || parse_const(name).is_some() {
        return true;
    }
    name.chars().any(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                '#' | '"' | '\'' | '\\' | '+' | '*' | '/' | '!' | '>' | '@'
            )
    })
}

fn write_phrase(out: &mut String, text: &str) {
    if text.is_empty()
        || text.trim() != text
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '#' | '"' | '\'' | '\\'))
    {
        quoted(out, text);
    } else {
        out.push_str(text);
    }
}

fn write_comments(out: &mut String, comments: &[String]) {
    for comment in comments {
        out.push('#');
        if !comment.is_empty() {
            out.push(' ');
            out.push_str(comment);
        }
        out.push('\n');
    }
}

fn write_trailing(out: &mut String, trailing: &Option<String>) {
    if let Some(comment) = trailing {
        out.push_str(" #");
        if !comment.is_empty() {
            out.push(' ');
            out.push_str(comment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_round_trips_and_ranks_left_to_right() {
        let source = "\
@gtl
@title Select and invert

xor a b -> d
not d -> nd
mux nd a b -> y
";
        let net = parse(source).unwrap();
        assert_eq!(net.gates.len(), 3);
        assert_eq!(net.gates[0].column, 1);
        assert_eq!(net.gates[1].column, 2);
        assert_eq!(net.gates[2].column, 3);
        assert_eq!(net.gates[2].kind.input_count(), 3);
        assert_eq!(net.gates[1].kind.input_count(), 1);
        assert!(net.gates[1].kind.invert());
        assert!(!net.gates[0].kind.invert());
        assert_eq!(write(&net), source);
    }

    #[test]
    fn quoted_integer_stays_a_name() {
        let net = parse("@gtl\nand a \"1\" -> \"0\"\n").unwrap();
        assert_eq!(net.gates[0].inputs[1], Operand::Name("1".into()));
        assert_eq!(net.gates[0].outputs[0], "0");
        let tie = parse("@gtl\nand a 1 -> y\n").unwrap();
        assert_eq!(tie.gates[0].inputs[1], Operand::Const(1));
    }
}
