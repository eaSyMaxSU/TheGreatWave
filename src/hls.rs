//! Line-oriented high-level synthesis schedules.
//!
//! A cycle header is a clock edge. Operators under it run in that cycle.
//! `!N` keeps the unit busy for N cycles. The text is the schedule; nothing
//! in the file is a coordinate.

use crate::native::{split_comment, tokens};
use crate::scan::Remark;
use crate::Error;
use std::collections::HashMap;

const MAX_CYCLES: u32 = 64;
const MAX_STARTS: usize = 32;
const MAX_OPS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Add,
    Mul,
    Div,
}

impl Kind {
    pub(crate) fn symbol(self) -> char {
        match self {
            Kind::Add => '+',
            Kind::Mul => '*',
            Kind::Div => '/',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    Name(String),
    Const(i64),
}

#[derive(Clone, Debug)]
pub(crate) struct Op {
    pub kind: Kind,
    pub left: Operand,
    pub right: Operand,
    pub result: String,
    pub latency: u32,
    pub cycle: u32,
    pub at: usize,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Cycle {
    pub index: u32,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Schedule {
    pub title: Option<String>,
    pub footer: Option<String>,
    pub notes: Notes,
    pub cycles: Vec<Cycle>,
    pub ops: Vec<Op>,
    /// Drawn cycles, from 0 inclusive through this exclusive. Latency extends it.
    pub cycle_count: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Notes {
    pub hls: Remark,
    pub title: Remark,
    pub footer: Remark,
    pub end: Vec<String>,
}

struct Raw {
    indent: usize,
    text: String,
    at: usize,
    leading: Vec<String>,
    trailing: Option<String>,
}

pub(crate) fn starts_with_hls(source: &str) -> bool {
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
        let rest = line.strip_prefix("@hls");
        return rest.is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace));
    }
    false
}

pub(crate) fn parse(source: &str) -> Result<Schedule, Error> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let lines = raw_lines(source)?;
    let mut index = 0;
    let Some(first) = lines.first() else {
        return Err(err(0, "@hls takes no arguments"));
    };
    if first.indent != 0 || !is_hls(&first.text) {
        return Err(err(first.at, "an HLS schedule starts with @hls"));
    }
    if first.text != "@hls" {
        return Err(err(first.at, "@hls takes no arguments"));
    }
    let mut schedule = Schedule {
        title: None,
        footer: None,
        notes: Notes {
            hls: Remark {
                leading: first.leading.clone(),
                trailing: first.trailing.clone(),
            },
            ..Notes::default()
        },
        cycles: Vec::new(),
        ops: Vec::new(),
        cycle_count: 0,
    };
    index += 1;
    let mut current: Option<u32> = None;
    while index < lines.len() {
        let line = &lines[index];
        if line.text.is_empty() {
            schedule.notes.end = line.leading.clone();
            break;
        }
        if line.indent == 0 {
            if let Some(directive) = line.text.strip_prefix('@') {
                if !schedule.ops.is_empty() || !schedule.cycles.is_empty() {
                    return Err(err(line.at, "directives belong before the cycles"));
                }
                apply_directive(&mut schedule, directive, line)?;
                index += 1;
                continue;
            }
            let cycle = parse_cycle(&line.text, line.at)?;
            if let Some(prev) = schedule.cycles.last() {
                if cycle <= prev.index {
                    return Err(err(line.at, "cycle numbers must increase"));
                }
            }
            if cycle >= MAX_CYCLES {
                return Err(err(line.at, "schedule exceeds 64 cycles"));
            }
            schedule.cycles.push(Cycle {
                index: cycle,
                leading: line.leading.clone(),
                trailing: line.trailing.clone(),
            });
            current = Some(cycle);
            index += 1;
            continue;
        }
        if looks_like_cycle(&line.text) {
            return Err(err(line.at, "a cycle header starts at column 0"));
        }
        let Some(cycle) = current else {
            return Err(err(line.at, "an operator belongs to a cycle"));
        };
        if schedule.ops.len() == MAX_OPS {
            return Err(err(line.at, "schedule exceeds 256 operators"));
        }
        let started = schedule.ops.iter().filter(|op| op.cycle == cycle).count();
        if started == MAX_STARTS {
            return Err(err(line.at, "cycle exceeds 32 operators"));
        }
        let mut op = parse_op(&line.text, line.at, &schedule.ops)?;
        op.cycle = cycle;
        op.leading = line.leading.clone();
        op.trailing = line.trailing.clone();
        if op.cycle + op.latency > MAX_CYCLES {
            return Err(err(op.at, "schedule exceeds 64 cycles"));
        }
        schedule.ops.push(op);
        index += 1;
    }
    resolve(&schedule)?;
    schedule.cycle_count = schedule
        .cycles
        .iter()
        .map(|cycle| cycle.index + 1)
        .chain(schedule.ops.iter().map(|op| op.cycle + op.latency))
        .max()
        .unwrap_or(0);
    if schedule.cycle_count > MAX_CYCLES {
        return Err(err(0, "schedule exceeds 64 cycles"));
    }
    Ok(schedule)
}

fn looks_like_cycle(text: &str) -> bool {
    let Some(body) = text.strip_suffix(':') else {
        return false;
    };
    !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit())
}

fn is_hls(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("@hls") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

fn apply_directive(schedule: &mut Schedule, directive: &str, line: &Raw) -> Result<(), Error> {
    let (key, value) = directive
        .split_once(char::is_whitespace)
        .unwrap_or((directive, ""));
    let value = value.trim();
    let value_at = line.at + line.text.len() - value.len();
    match key {
        "hls" => Err(err(line.at, "@hls is already started")),
        "title" => {
            if schedule.title.is_some() {
                return Err(err(line.at, "duplicate @title"));
            }
            schedule.title = Some(if value.is_empty() {
                String::new()
            } else {
                crate::native::text(value, value_at)?
            });
            schedule.notes.title = Remark {
                leading: line.leading.clone(),
                trailing: line.trailing.clone(),
            };
            Ok(())
        }
        "footer" => {
            if schedule.footer.is_some() {
                return Err(err(line.at, "duplicate @footer"));
            }
            schedule.footer = Some(if value.is_empty() {
                String::new()
            } else {
                crate::native::text(value, value_at)?
            });
            schedule.notes.footer = Remark {
                leading: line.leading.clone(),
                trailing: line.trailing.clone(),
            };
            Ok(())
        }
        "end" | "asm" => Err(err(
            line.at,
            &format!("@{key} is not used in an HLS schedule"),
        )),
        _ => Err(err(line.at, &format!("unknown directive @{key}"))),
    }
}

fn parse_cycle(text: &str, at: usize) -> Result<u32, Error> {
    let Some(body) = text.strip_suffix(':') else {
        return Err(err(at, "expected a cycle number followed by ':'"));
    };
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return Err(err(at, "expected a cycle number followed by ':'"));
    }
    if body.len() > 1 && body.starts_with('0') {
        return Err(err(at, "cycle numbers do not have leading zeros"));
    }
    body.parse::<u32>()
        .map_err(|_| err(at, "schedule exceeds 64 cycles"))
}

fn parse_op(text: &str, at: usize, earlier: &[Op]) -> Result<Op, Error> {
    let mut chars = text.chars();
    let symbol = chars.next().unwrap_or(' ');
    let kind = match symbol {
        '+' => Kind::Add,
        '*' => Kind::Mul,
        '/' => Kind::Div,
        _ => {
            return Err(err(
                at,
                &format!("unknown operator \"{symbol}\"; operators are +, *, /"),
            ))
        }
    };
    let after = symbol.len_utf8();
    let rest = &text[after..];
    if !rest.starts_with(char::is_whitespace) {
        return Err(err(at, "operator needs two operands and a result"));
    }
    let rest = rest.trim_start();
    let rest_at = at + text.len() - rest.len();
    let quoted = quote_flags(rest);
    let parts = tokens(rest, rest_at)?;
    if parts.len() != 4 && parts.len() != 5 {
        return Err(err(at, "operator needs two operands and a result"));
    }
    if parts[2] != "->" {
        return Err(err(at, "expected -> before the result"));
    }
    let left = operand(
        &parts[0],
        quoted.first().copied().unwrap_or(false),
        rest_at,
        earlier,
    )?;
    let right = operand(
        &parts[1],
        quoted.get(1).copied().unwrap_or(false),
        rest_at,
        earlier,
    )?;
    let result = result_name(&parts[3], quoted.get(3).copied().unwrap_or(false), at)?;
    let latency = if parts.len() == 5 {
        parse_latency(&parts[4], at)?
    } else {
        1
    };
    Ok(Op {
        kind,
        left,
        right,
        result,
        latency,
        cycle: 0,
        at,
        leading: Vec::new(),
        trailing: None,
    })
}

fn operand(token: &str, quoted: bool, at: usize, earlier: &[Op]) -> Result<Operand, Error> {
    if !quoted && (token == "->" || token.starts_with('!') || matches!(token, "+" | "*" | "/")) {
        return Err(err(
            at,
            &format!(
                "unknown operand \"{token}\"; results are {}",
                name_list(earlier)
            ),
        ));
    }
    if !quoted {
        if let Some(value) = parse_const(token) {
            return Ok(Operand::Const(value));
        }
    }
    if token.is_empty() {
        return Err(err(at, "operand needs a name"));
    }
    Ok(Operand::Name(token.to_string()))
}

fn result_name(token: &str, quoted: bool, at: usize) -> Result<String, Error> {
    if token.is_empty() || (!quoted && (token == "->" || token.starts_with('!'))) {
        return Err(err(at, "result needs a name"));
    }
    Ok(token.to_string())
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

fn parse_latency(token: &str, at: usize) -> Result<u32, Error> {
    let Some(body) = token.strip_prefix('!') else {
        return Err(err(at, "expected !N latency or the end of the operator"));
    };
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) || body == "0" {
        return Err(err(at, "latency must be a positive integer"));
    }
    if body.len() > 1 && body.starts_with('0') {
        return Err(err(at, "latency must be a positive integer"));
    }
    body.parse::<u32>()
        .map_err(|_| err(at, "latency must be a positive integer"))
}

fn quote_flags(src: &str) -> Vec<bool> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let c = src[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            out.push(true);
            i += c.len_utf8();
            while i < src.len() {
                let c = src[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '\\' {
                    if let Some(escaped) = src[i..].chars().next() {
                        i += escaped.len_utf8();
                    }
                    continue;
                }
                if c == quote {
                    break;
                }
            }
        } else {
            out.push(false);
            while i < src.len() {
                let c = src[i..].chars().next().unwrap();
                if c.is_whitespace() {
                    break;
                }
                i += c.len_utf8();
            }
        }
    }
    out
}

fn resolve(schedule: &Schedule) -> Result<(), Error> {
    let mut ready: HashMap<&str, u32> = HashMap::new();
    for (index, op) in schedule.ops.iter().enumerate() {
        if ready.contains_key(op.result.as_str()) {
            return Err(err(op.at, &format!("duplicate result \"{}\"", op.result)));
        }
        let known = name_list(&schedule.ops[..index]);
        for operand in [&op.left, &op.right] {
            let Some(name) = operand_name(operand) else {
                continue;
            };
            let ready_at = ready.get(name).copied();
            let not_ready = match ready_at {
                Some(cycle) => cycle > op.cycle,
                None => name == op.result,
            };
            if not_ready {
                let cycle = ready_at.unwrap_or(op.cycle + op.latency);
                return Err(err(
                    op.at,
                    &format!(
                        "result \"{name}\" is not ready until cycle {cycle}; results are {known}"
                    ),
                ));
            }
        }
        ready.insert(op.result.as_str(), op.cycle + op.latency);
    }
    Ok(())
}

fn name_list(ops: &[Op]) -> String {
    if ops.is_empty() {
        return "none".to_string();
    }
    ops.iter()
        .map(|op| op.result.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn operand_name(operand: &Operand) -> Option<&str> {
    match operand {
        Operand::Name(name) => Some(name),
        Operand::Const(_) => None,
    }
}

fn raw_lines(source: &str) -> Result<Vec<Raw>, Error> {
    let mut lines = Vec::new();
    let mut pending = Vec::new();
    let mut offset = 0;
    for raw in source.split_inclusive('\n') {
        let at = offset;
        offset += raw.len();
        let (code, comment) = split_comment(raw, at)?;
        let trimmed = code.trim();
        if trimmed.is_empty() {
            if let Some(body) = comment {
                pending.push(body);
            }
            continue;
        }
        let indent = code.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let start = code.len() - code.trim_start().len();
        lines.push(Raw {
            indent,
            text: trimmed.to_string(),
            at: at + start,
            leading: std::mem::take(&mut pending),
            trailing: comment,
        });
    }
    if !pending.is_empty() {
        lines.push(Raw {
            indent: 0,
            text: String::new(),
            at: offset,
            leading: pending,
            trailing: None,
        });
    }
    Ok(lines)
}

fn err(offset: usize, message: &str) -> Error {
    Error {
        offset,
        message: message.into(),
    }
}

pub(crate) fn write(schedule: &Schedule) -> String {
    let mut out = String::new();
    write_comments(&mut out, &schedule.notes.hls.leading);
    out.push_str("@hls");
    write_trailing(&mut out, &schedule.notes.hls.trailing);
    out.push('\n');
    if let Some(title) = &schedule.title {
        write_directive(&mut out, &schedule.notes.title, "title", title);
    }
    if let Some(footer) = &schedule.footer {
        write_directive(&mut out, &schedule.notes.footer, "footer", footer);
    }
    if schedule.cycle_count > 0 {
        out.push('\n');
    }
    for cycle in 0..schedule.cycle_count {
        if let Some(header) = schedule.cycles.iter().find(|item| item.index == cycle) {
            write_comments(&mut out, &header.leading);
            out.push_str(&cycle.to_string());
            out.push(':');
            write_trailing(&mut out, &header.trailing);
            out.push('\n');
        } else {
            out.push_str(&cycle.to_string());
            out.push_str(":\n");
        }
        for op in schedule.ops.iter().filter(|op| op.cycle == cycle) {
            write_comments(&mut out, &op.leading);
            out.push_str("  ");
            out.push(op.kind.symbol());
            out.push(' ');
            write_operand(&mut out, &op.left);
            out.push(' ');
            write_operand(&mut out, &op.right);
            out.push_str(" -> ");
            write_result(&mut out, &op.result);
            if op.latency != 1 {
                out.push_str(" !");
                out.push_str(&op.latency.to_string());
            }
            write_trailing(&mut out, &op.trailing);
            out.push('\n');
        }
    }
    write_comments(&mut out, &schedule.notes.end);
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
        Operand::Name(name) => write_result(out, name),
    }
}

fn write_result(out: &mut String, name: &str) {
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

fn quoted(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0}'..='\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
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
4:
5:
";

    #[test]
    fn canonical_form_extends_the_divider_and_drops_unit_latency() {
        let schedule = parse(SCALED).unwrap();
        assert_eq!(schedule.cycle_count, 6);
        assert_eq!(write(&schedule), SCALED);
        assert_eq!(write(&parse(&write(&schedule)).unwrap()), SCALED);
    }

    #[test]
    fn wider_indent_canonicalizes() {
        let source = "\
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
        assert_eq!(write(&parse(source).unwrap()), SCALED);
    }

    #[test]
    fn comments_stay_with_their_lines() {
        let source = "\
# chart
@hls # kind
# heading
@title Scaled sum of products

# first
0: # edge
  # add
  + a b -> s0
";
        let text = write(&parse(source).unwrap());
        assert!(text.contains("# chart\n@hls # kind\n"), "{text}");
        assert!(
            text.contains("# heading\n@title Scaled sum of products\n"),
            "{text}"
        );
        assert!(text.contains("# first\n0: # edge\n"), "{text}");
        assert!(text.contains("# add\n  + a b -> s0\n"), "{text}");
    }

    #[test]
    fn same_cycle_duplicate_and_early_use_are_errors() {
        let early = parse("@hls\n0:\n  + a b -> s\n  + s c -> t\n").unwrap_err();
        assert!(early.message.contains("not ready"), "{}", early.message);
        assert!(early.message.contains("results are"), "{}", early.message);

        let duplicate = parse("@hls\n0:\n  + a b -> s\n1:\n  + c d -> s\n").unwrap_err();
        assert!(
            duplicate.message.contains("duplicate result"),
            "{}",
            duplicate.message
        );

        let unknown = parse("@hls\n0:\n  + a -> -> s\n").unwrap_err();
        assert!(
            unknown.message.contains("unknown operand"),
            "{}",
            unknown.message
        );

        let backwards = parse("@hls\n1:\n  + a b -> s\n0:\n  + s c -> t\n").unwrap_err();
        assert!(
            backwards.message.contains("must increase"),
            "{}",
            backwards.message
        );
    }

    #[test]
    fn caps_are_errors() {
        let mut many = String::from("@hls\n0:\n");
        for i in 0..33 {
            many.push_str(&format!("  + a b -> s{i}\n"));
        }
        let error = parse(&many).unwrap_err();
        assert!(error.message.contains("32"), "{}", error.message);

        let mut ops = String::from("@hls\n");
        for i in 0..257 {
            ops.push_str(&format!("{i}:\n  + a b -> s{i}\n"));
        }
        let error = parse(&ops).unwrap_err();
        assert!(
            error.message.contains("64") || error.message.contains("256"),
            "{}",
            error.message
        );

        let deep = "@hls\n60:\n  / a b -> y !8\n";
        let error = parse(deep).unwrap_err();
        assert!(error.message.contains("64"), "{}", error.message);
    }
}
