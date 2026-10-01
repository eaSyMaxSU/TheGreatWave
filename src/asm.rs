//! Line-oriented ASM charts. Indentation is significant only in this module.
//!
//! A chart is a list of state boxes. Each box has Moore outputs and one exit:
//! a link, or a decision whose `0` path continues down and whose `1` path
//! leaves to the right. The text is the layout; nothing in the file is a
//! coordinate.

use crate::native::{split_comment, text, tokens};
use crate::scan::Remark;
use crate::Error;
use std::collections::HashMap;

const MAX_STATES: usize = 256;
const MAX_DEPTH: u32 = 32;
const MAX_LINES: usize = 32;

#[derive(Clone, Debug)]
pub(crate) struct Chart {
    pub title: Option<String>,
    pub footer: Option<String>,
    pub notes: Notes,
    pub states: Vec<State>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Notes {
    pub asm: Remark,
    pub title: Remark,
    pub footer: Remark,
    pub end: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct State {
    pub name: String,
    pub at: usize,
    pub outputs: Vec<Line>,
    pub exit: Exit,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub text: String,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Exit {
    None,
    Link(Link),
    Decision(Box<Decision>),
}

#[derive(Clone, Debug)]
pub(crate) struct Link {
    pub target: String,
    pub at: usize,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Decision {
    pub condition: String,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
    pub zero: Branch,
    pub one: Branch,
}

#[derive(Clone, Debug)]
pub(crate) struct Branch {
    pub cond: Option<String>,
    pub next: Next,
    pub leading: Vec<String>,
    pub trailing: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Next {
    Link(Link),
    Decision(Box<Decision>),
}

struct Raw {
    indent: usize,
    text: String,
    at: usize,
    leading: Vec<String>,
    trailing: Option<String>,
}

pub(crate) fn starts_with_asm(source: &str) -> bool {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    match first_code_line(source) {
        Some(line) => is_asm_directive(line),
        None => false,
    }
}

fn is_asm_directive(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("@asm") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

fn first_code_line(source: &str) -> Option<&str> {
    let mut offset = 0;
    for raw in source.split_inclusive('\n') {
        let at = offset;
        offset += raw.len();
        let (code, _) = split_comment(raw, at).ok()?;
        let line = code.trim();
        if !line.is_empty() {
            return Some(line);
        }
    }
    None
}

pub(crate) fn parse(source: &str) -> Result<Chart, Error> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let lines = raw_lines(source)?;
    let mut index = 0;
    let Some(first) = lines.first() else {
        return Err(err(0, "@asm takes no arguments"));
    };
    if !is_asm_directive(&first.text) || first.indent != 0 {
        return Err(err(first.at, "an ASM chart starts with @asm"));
    }
    if first.text != "@asm" {
        return Err(err(first.at, "@asm takes no arguments"));
    }
    let mut chart = Chart {
        title: None,
        footer: None,
        notes: Notes {
            asm: Remark {
                leading: first.leading.clone(),
                trailing: first.trailing.clone(),
            },
            ..Notes::default()
        },
        states: Vec::new(),
    };
    index += 1;
    while index < lines.len() {
        let line = &lines[index];
        if line.text.is_empty() {
            chart.notes.end = line.leading.clone();
            break;
        }
        if line.indent != 0 {
            return Err(err(
                line.at,
                "a state name starts at the beginning of the line",
            ));
        }
        if let Some(directive) = line.text.strip_prefix('@') {
            let (key, value) = directive
                .split_once(char::is_whitespace)
                .unwrap_or((directive, ""));
            let value = value.trim();
            let value_at = line.at + line.text.len() - value.len();
            match key {
                "asm" => return Err(err(line.at, "@asm is already started")),
                "title" => {
                    if chart.title.is_some() {
                        return Err(err(line.at, "duplicate @title"));
                    }
                    if !chart.states.is_empty() {
                        return Err(err(line.at, "directives belong before the states"));
                    }
                    chart.title = Some(if value.is_empty() {
                        String::new()
                    } else {
                        text(value, value_at)?
                    });
                    chart.notes.title = Remark {
                        leading: line.leading.clone(),
                        trailing: line.trailing.clone(),
                    };
                }
                "footer" => {
                    if chart.footer.is_some() {
                        return Err(err(line.at, "duplicate @footer"));
                    }
                    if !chart.states.is_empty() {
                        return Err(err(line.at, "directives belong before the states"));
                    }
                    chart.footer = Some(if value.is_empty() {
                        String::new()
                    } else {
                        text(value, value_at)?
                    });
                    chart.notes.footer = Remark {
                        leading: line.leading.clone(),
                        trailing: line.trailing.clone(),
                    };
                }
                "end" => return Err(err(line.at, "@end is not used in an ASM chart")),
                _ => return Err(err(line.at, &format!("unknown directive @{key}"))),
            }
            index += 1;
            continue;
        }
        if !line.text.ends_with(':') {
            return Err(err(line.at, "expected a state name followed by ':'"));
        }
        if chart.states.len() == MAX_STATES {
            return Err(err(line.at, "ASM chart exceeds 256 states"));
        }
        let name = state_name(&line.text, line.at)?;
        let at = line.at;
        let leading = line.leading.clone();
        let trailing = line.trailing.clone();
        index += 1;
        let (outputs, exit) = parse_body(&lines, &mut index, 0)?;
        let mut count = visual_lines(&name);
        for output in &outputs {
            count += visual_lines(&output.text);
        }
        if count > MAX_LINES {
            return Err(err(at, "state box exceeds 32 lines"));
        }
        chart.states.push(State {
            name,
            at,
            outputs,
            exit,
            leading,
            trailing,
        });
    }
    resolve_links(&chart)?;
    Ok(chart)
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
    if let Some(last) = lines.last() {
        if last.text.is_empty() {
            // Kept so the caller can read end comments. parse() skips it
            // because the main loop stops on real lines; drop it from the
            // directive scan by handling it in parse via notes.
        }
    }
    Ok(lines)
}

fn parse_body(lines: &[Raw], index: &mut usize, parent: usize) -> Result<(Vec<Line>, Exit), Error> {
    let mut outputs = Vec::new();
    let mut exit = None;
    while *index < lines.len() {
        let line = &lines[*index];
        if line.text.is_empty() {
            break;
        }
        if line.indent <= parent {
            break;
        }
        if line.text.starts_with('@') {
            return Err(err(line.at, "directives belong before the states"));
        }
        if starts_decision(&line.text) {
            if exit.is_some() {
                return Err(err(line.at, "a state has one exit"));
            }
            exit = Some(Exit::Decision(Box::new(parse_decision(lines, index, 1)?)));
            continue;
        }
        if starts_link(&line.text) {
            if exit.is_some() {
                return Err(err(line.at, "a state has one exit"));
            }
            let link = parse_unconditional(line)?;
            let indent = line.indent;
            *index += 1;
            if *index < lines.len()
                && !lines[*index].text.is_empty()
                && lines[*index].indent > indent
            {
                return Err(err(
                    lines[*index].at,
                    "a state link cannot contain nested lines",
                ));
            }
            exit = Some(Exit::Link(link));
            continue;
        }
        if exit.is_some() {
            return Err(err(line.at, "Moore outputs come before the exit"));
        }
        let body = if line.text.starts_with(['\'', '"']) {
            text(&line.text, line.at)?
        } else {
            line.text.clone()
        };
        outputs.push(Line {
            text: body,
            leading: line.leading.clone(),
            trailing: line.trailing.clone(),
        });
        *index += 1;
    }
    Ok((outputs, exit.unwrap_or(Exit::None)))
}

fn parse_decision(lines: &[Raw], index: &mut usize, depth: u32) -> Result<Decision, Error> {
    if depth > MAX_DEPTH {
        return Err(err(
            lines.get(*index).map_or(0, |line| line.at),
            "decision nesting exceeds 32 levels",
        ));
    }
    let line = &lines[*index];
    let condition = decision_condition(&line.text, line.at)?;
    let header_indent = line.indent;
    let at = line.at;
    let leading = line.leading.clone();
    let trailing = line.trailing.clone();
    *index += 1;
    if *index >= lines.len()
        || lines[*index].text.is_empty()
        || lines[*index].indent <= header_indent
    {
        return Err(err(at, "decision needs exits 0 and 1"));
    }
    let exit_indent = lines[*index].indent;
    let mut branches = Vec::new();
    while *index < lines.len()
        && !lines[*index].text.is_empty()
        && lines[*index].indent >= exit_indent
    {
        if lines[*index].indent != exit_indent {
            return Err(err(lines[*index].at, "decision exits must share an indent"));
        }
        if branches.len() == 2 {
            return Err(err(lines[*index].at, "decision has an extra exit"));
        }
        branches.push(parse_branch(lines, index, exit_indent, depth)?);
    }
    if branches.len() != 2 {
        return Err(err(at, "decision needs exits 0 and 1"));
    }
    let mut zero = None;
    let mut one = None;
    for (digit, branch) in branches {
        let slot = if digit == b'0' { &mut zero } else { &mut one };
        if slot.is_some() {
            return Err(err(at, "decision needs exits 0 and 1"));
        }
        *slot = Some(branch);
    }
    let zero = zero.ok_or_else(|| err(at, "decision needs exits 0 and 1"))?;
    let one = one.ok_or_else(|| err(at, "decision needs exits 0 and 1"))?;
    Ok(Decision {
        condition,
        leading,
        trailing,
        zero,
        one,
    })
}

fn parse_branch(
    lines: &[Raw],
    index: &mut usize,
    _indent: usize,
    depth: u32,
) -> Result<(u8, Branch), Error> {
    let line = &lines[*index];
    let (digit, cond, kind) = split_exit(&line.text, line.at)?;
    let leading = line.leading.clone();
    let trailing = line.trailing.clone();
    let at = line.at;
    *index += 1;
    let next = match kind {
        ExitKind::Target(target) => {
            if *index < lines.len()
                && !lines[*index].text.is_empty()
                && lines[*index].indent > line.indent
            {
                return Err(err(
                    lines[*index].at,
                    "a state link cannot contain nested lines",
                ));
            }
            Next::Link(Link {
                target,
                at,
                leading: Vec::new(),
                trailing: None,
            })
        }
        ExitKind::Nested(condition) => {
            if depth + 1 > MAX_DEPTH {
                return Err(err(at, "decision nesting exceeds 32 levels"));
            }
            // The nested header was on this exit line. Synthesize the scan
            // position so the exits that follow are parsed as its children.
            if *index >= lines.len()
                || lines[*index].text.is_empty()
                || lines[*index].indent <= line.indent
            {
                return Err(err(at, "decision needs exits 0 and 1"));
            }
            let saved = Raw {
                indent: line.indent,
                text: format!("? {condition}"),
                at,
                leading: Vec::new(),
                trailing: None,
            };
            // Parse using the following lines, with a virtual header already
            // consumed. Inline the exit loop at depth + 1.
            let decision = parse_nested_exits(lines, index, &saved, condition, depth + 1)?;
            Next::Decision(Box::new(decision))
        }
    };
    Ok((
        digit,
        Branch {
            cond,
            next,
            leading,
            trailing,
        },
    ))
}

fn parse_nested_exits(
    lines: &[Raw],
    index: &mut usize,
    header: &Raw,
    condition: String,
    depth: u32,
) -> Result<Decision, Error> {
    if depth > MAX_DEPTH {
        return Err(err(header.at, "decision nesting exceeds 32 levels"));
    }
    let exit_indent = lines[*index].indent;
    let mut branches = Vec::new();
    while *index < lines.len()
        && !lines[*index].text.is_empty()
        && lines[*index].indent > header.indent
    {
        if lines[*index].indent != exit_indent {
            return Err(err(lines[*index].at, "decision exits must share an indent"));
        }
        if branches.len() == 2 {
            return Err(err(lines[*index].at, "decision has an extra exit"));
        }
        branches.push(parse_branch(lines, index, exit_indent, depth)?);
    }
    if branches.len() != 2 {
        return Err(err(header.at, "decision needs exits 0 and 1"));
    }
    let mut zero = None;
    let mut one = None;
    for (digit, branch) in branches {
        let slot = if digit == b'0' { &mut zero } else { &mut one };
        if slot.is_some() {
            return Err(err(header.at, "decision needs exits 0 and 1"));
        }
        *slot = Some(branch);
    }
    Ok(Decision {
        condition,
        leading: Vec::new(),
        trailing: None,
        zero: zero.ok_or_else(|| err(header.at, "decision needs exits 0 and 1"))?,
        one: one.ok_or_else(|| err(header.at, "decision needs exits 0 and 1"))?,
    })
}

enum ExitKind {
    Target(String),
    Nested(String),
}

fn split_exit(text: &str, at: usize) -> Result<(u8, Option<String>, ExitKind), Error> {
    let digit = text.as_bytes().first().copied().unwrap_or(0);
    if digit != b'0' && digit != b'1' {
        return Err(err(at, "decision exits start with 0 or 1"));
    }
    let rest = &text[1..];
    if rest.is_empty() || !rest.starts_with(char::is_whitespace) {
        return Err(err(at, "put a space after the exit digit"));
    }
    let rest = rest.trim_start();
    if rest.is_empty() {
        return Err(err(at, "decision exit needs a state or a nested ?"));
    }
    let (cond, rest) = if rest.starts_with('(') {
        let (inner, consumed) = take_paren(rest, at + (text.len() - rest.len()))?;
        let after = rest[consumed..].trim_start();
        if after.is_empty() {
            return Err(err(at, "decision exit needs a state or a nested ?"));
        }
        (Some(inner), after)
    } else {
        (None, rest)
    };
    if let Some(condition) = rest.strip_prefix('?') {
        let condition = condition.trim();
        if condition.is_empty() {
            return Err(err(at, "decision needs a condition"));
        }
        let condition = if condition.starts_with(['\'', '"']) {
            text_one(condition, at)?
        } else {
            condition.to_string()
        };
        return Ok((digit, cond, ExitKind::Nested(condition)));
    }
    let (target, rest) = one_name(rest, at)?;
    if !rest.trim().is_empty() {
        return Err(err(at, "unexpected text after the state name"));
    }
    Ok((digit, cond, ExitKind::Target(target)))
}

fn parse_unconditional(line: &Raw) -> Result<Link, Error> {
    let rest = line.text[1..].trim();
    if rest.is_empty() {
        return Err(err(line.at, "a link needs a state name"));
    }
    if !line.text[1..].starts_with(char::is_whitespace) && !line.text[1..].is_empty() {
        // `>name` is accepted; `> name` is accepted. A missing separator is
        // fine because `>` is not part of the name.
    }
    let (target, rest) = one_name(rest, line.at)?;
    if !rest.trim().is_empty() {
        return Err(err(line.at, "unexpected text after the state name"));
    }
    Ok(Link {
        target,
        at: line.at,
        leading: line.leading.clone(),
        trailing: line.trailing.clone(),
    })
}

fn starts_decision(text: &str) -> bool {
    text == "?" || text.starts_with("? ") || text.starts_with("?\t")
}

fn starts_link(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('>') else {
        return false;
    };
    rest.is_empty() || !rest.starts_with(['=', '-'])
}

fn decision_condition(text: &str, at: usize) -> Result<String, Error> {
    let rest = text[1..].trim();
    if rest.is_empty() {
        return Err(err(at, "decision needs a condition"));
    }
    if rest.starts_with(['\'', '"']) {
        text_one(rest, at)
    } else {
        Ok(rest.to_string())
    }
}

fn state_name(line: &str, at: usize) -> Result<String, Error> {
    let body = line[..line.len() - 1].trim();
    if body.is_empty() {
        return Err(err(at, "state needs a name"));
    }
    if body.starts_with(['\'', '"']) {
        let name = text_one(body, at)?;
        if name.is_empty() {
            return Err(err(at, "state needs a name"));
        }
        Ok(name)
    } else {
        if body.chars().any(char::is_whitespace) {
            return Err(err(at, "a state name with spaces must be quoted"));
        }
        if body.contains(':') {
            return Err(err(at, "a state name containing ':' must be quoted"));
        }
        Ok(body.to_string())
    }
}

fn one_name(s: &str, at: usize) -> Result<(String, &str), Error> {
    let s = s.trim_start();
    if s.starts_with(['\'', '"']) {
        let parts = tokens(s, at)?;
        if parts.len() != 1 {
            return Err(err(at, "expected one state name"));
        }
        let name = parts.into_iter().next().unwrap().into_owned();
        if name.is_empty() {
            return Err(err(at, "state needs a name"));
        }
        // tokens consumes the quoted token. The remainder is not returned, so
        // a quoted name must be the whole string.
        Ok((name, ""))
    } else {
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        if end == 0 {
            return Err(err(at, "decision exit needs a state or a nested ?"));
        }
        Ok((s[..end].to_string(), &s[end..]))
    }
}

fn text_one(s: &str, at: usize) -> Result<String, Error> {
    let parts = tokens(s, at)?;
    if parts.len() != 1 {
        return Err(err(at, "expected one quoted string"));
    }
    Ok(parts.into_iter().next().unwrap().into_owned())
}

fn take_paren(s: &str, at: usize) -> Result<(String, usize), Error> {
    let mut i = 1;
    let mut depth = 1_i32;
    let mut quote = None;
    let mut escaped = false;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        let n = c.len_utf8();
        if escaped {
            escaped = false;
            i += n;
            continue;
        }
        if let Some(q) = quote {
            if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            i += n;
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                i += n;
                if depth == 0 {
                    let inside = s[1..i - 1].trim();
                    let text = if inside.starts_with(['\'', '"']) {
                        text_one(inside, at + 1)?
                    } else {
                        inside.to_string()
                    };
                    return Ok((text, i));
                }
                continue;
            }
            _ => {}
        }
        i += n;
    }
    Err(err(at, "unclosed conditional output"))
}

fn visual_lines(s: &str) -> usize {
    s.split('\n').count().max(1)
}

fn resolve_links(chart: &Chart) -> Result<(), Error> {
    let mut names = HashMap::with_capacity(chart.states.len());
    for (index, state) in chart.states.iter().enumerate() {
        if names.insert(state.name.as_str(), index).is_some() {
            return Err(err(
                state.at,
                &format!("duplicate state {}", quote_plain(&state.name)),
            ));
        }
    }
    let list = chart
        .states
        .iter()
        .map(|state| state.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    for state in &chart.states {
        check_exit(&state.exit, &names, &list)?;
    }
    Ok(())
}

fn check_exit(exit: &Exit, names: &HashMap<&str, usize>, list: &str) -> Result<(), Error> {
    match exit {
        Exit::None => Ok(()),
        Exit::Link(link) => check_link(link, names, list),
        Exit::Decision(decision) => {
            check_branch(&decision.zero, names, list)?;
            check_branch(&decision.one, names, list)
        }
    }
}

fn check_branch(branch: &Branch, names: &HashMap<&str, usize>, list: &str) -> Result<(), Error> {
    match &branch.next {
        Next::Link(link) => check_link(link, names, list),
        Next::Decision(decision) => {
            check_branch(&decision.zero, names, list)?;
            check_branch(&decision.one, names, list)
        }
    }
}

fn check_link(link: &Link, names: &HashMap<&str, usize>, list: &str) -> Result<(), Error> {
    if names.contains_key(link.target.as_str()) {
        return Ok(());
    }
    let known = if list.is_empty() {
        "there are no states".to_string()
    } else {
        format!("states are {list}")
    };
    Err(err(
        link.at,
        &format!("unknown state {}; {known}", quote_plain(&link.target)),
    ))
}

fn quote_plain(name: &str) -> String {
    format!("\"{name}\"")
}

fn err(offset: usize, message: &str) -> Error {
    Error {
        offset,
        message: message.into(),
    }
}

pub(crate) fn write(chart: &Chart) -> String {
    let mut out = String::new();
    write_comments(&mut out, 0, &chart.notes.asm.leading);
    out.push_str("@asm");
    write_trailing(&mut out, &chart.notes.asm.trailing);
    out.push('\n');
    if let Some(title) = &chart.title {
        write_directive(&mut out, &chart.notes.title, "title", title);
    }
    if let Some(footer) = &chart.footer {
        write_directive(&mut out, &chart.notes.footer, "footer", footer);
    }
    if !chart.states.is_empty() {
        out.push('\n');
    }
    for (index, state) in chart.states.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        write_comments(&mut out, 0, &state.leading);
        write_name(&mut out, &state.name);
        out.push(':');
        write_trailing(&mut out, &state.trailing);
        out.push('\n');
        for output in &state.outputs {
            write_comments(&mut out, 1, &output.leading);
            indent(&mut out, 1);
            write_output(&mut out, &output.text);
            write_trailing(&mut out, &output.trailing);
            out.push('\n');
        }
        match &state.exit {
            Exit::None => {}
            Exit::Link(link) => write_link_line(&mut out, 1, link),
            Exit::Decision(decision) => write_decision(&mut out, 1, decision),
        }
    }
    write_comments(&mut out, 0, &chart.notes.end);
    out
}

fn write_directive(out: &mut String, remark: &Remark, key: &str, value: &str) {
    write_comments(out, 0, &remark.leading);
    out.push('@');
    out.push_str(key);
    if !value.is_empty() {
        out.push(' ');
        write_phrase(out, value);
    }
    write_trailing(out, &remark.trailing);
    out.push('\n');
}

fn write_decision(out: &mut String, depth: usize, decision: &Decision) {
    write_comments(out, depth, &decision.leading);
    indent(out, depth);
    out.push_str("? ");
    write_phrase(out, &decision.condition);
    write_trailing(out, &decision.trailing);
    out.push('\n');
    write_branch(out, depth + 1, b'0', &decision.zero);
    write_branch(out, depth + 1, b'1', &decision.one);
}

fn write_branch(out: &mut String, depth: usize, digit: u8, branch: &Branch) {
    write_comments(out, depth, &branch.leading);
    indent(out, depth);
    out.push(digit as char);
    if let Some(cond) = &branch.cond {
        out.push(' ');
        write_paren(out, cond);
    }
    match &branch.next {
        Next::Link(link) => {
            out.push(' ');
            write_name(out, &link.target);
            write_trailing(out, &branch.trailing);
            out.push('\n');
        }
        Next::Decision(decision) => {
            out.push_str(" ? ");
            write_phrase(out, &decision.condition);
            write_trailing(out, &branch.trailing);
            out.push('\n');
            write_branch(out, depth + 1, b'0', &decision.zero);
            write_branch(out, depth + 1, b'1', &decision.one);
        }
    }
}

fn write_link_line(out: &mut String, depth: usize, link: &Link) {
    write_comments(out, depth, &link.leading);
    indent(out, depth);
    out.push_str("> ");
    write_name(out, &link.target);
    write_trailing(out, &link.trailing);
    out.push('\n');
}

fn write_output(out: &mut String, text: &str) {
    if output_needs_quotes(text) {
        quoted(out, text);
    } else {
        out.push_str(text);
    }
}

fn output_needs_quotes(text: &str) -> bool {
    text.is_empty()
        || text.trim() != text
        || text.starts_with(['?', '>', '@', '#', '"', '\''])
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '#' | '"' | '\'' | '\\'))
}

fn write_phrase(out: &mut String, text: &str) {
    if phrase_needs_quotes(text) {
        quoted(out, text);
    } else {
        out.push_str(text);
    }
}

fn phrase_needs_quotes(text: &str) -> bool {
    text.is_empty()
        || text.trim() != text
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '#' | '"' | '\'' | '\\'))
}

fn write_name(out: &mut String, name: &str) {
    if name.is_empty()
        || name.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(
                    c,
                    '#' | '"' | '\'' | '\\' | ':' | '@' | '?' | '>' | '(' | ')'
                )
        })
    {
        quoted(out, name);
    } else {
        out.push_str(name);
    }
}

fn write_paren(out: &mut String, text: &str) {
    out.push('(');
    if text.contains(['(', ')', '#', '"', '\'']) || text.trim() != text || text.is_empty() {
        quoted(out, text);
    } else {
        out.push_str(text);
    }
    out.push(')');
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

fn write_comments(out: &mut String, depth: usize, comments: &[String]) {
    for comment in comments {
        indent(out, depth);
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

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDSHAKE: &str = "\
@asm
@title Bus handshake

idle:
  req=0
  ack=0
  ? start
    0 idle
    1 (req=1) wait

wait:
  ack=1
  ? done
    0 ? hold
      0 wait
      1 idle
    1 idle
";

    #[test]
    fn canonical_form_is_stable() {
        let chart = parse(HANDSHAKE).unwrap();
        let text = write(&chart);
        assert_eq!(text, HANDSHAKE);
        assert_eq!(write(&parse(&text).unwrap()), text);
    }

    #[test]
    fn wider_indent_and_swapped_exits_canonicalize() {
        let source = "\
@asm
@title Bus handshake

idle:
    req=0
    ack=0
    ? start
        1 (req=1) wait
        0 idle

wait:
    ack=1
    ? done
        1 idle
        0 ? hold
            1 idle
            0 wait
";
        assert_eq!(write(&parse(source).unwrap()), HANDSHAKE);
    }

    #[test]
    fn comments_stay_with_their_lines() {
        let source = "\
# chart
@asm # kind
# heading
@title Bus handshake

# first
idle: # state
  # moore
  req=0
  ? start # test
    0 idle # loop
    1 (req=1) idle
# tail
";
        let text = write(&parse(source).unwrap());
        assert!(text.contains("# chart\n@asm # kind\n"), "{text}");
        assert!(text.contains("# heading\n@title Bus handshake\n"), "{text}");
        assert!(text.contains("# first\nidle: # state\n"), "{text}");
        assert!(text.contains("# moore\n  req=0\n"), "{text}");
        assert!(text.contains("? start # test\n"), "{text}");
        assert!(text.contains("0 idle # loop\n"), "{text}");
        assert!(text.contains("# tail\n"), "{text}");
    }

    #[test]
    fn duplicate_unknown_and_missing_exits_are_errors() {
        let duplicate = parse("@asm\ns:\n  > s\ns:\n  > s\n").unwrap_err();
        assert!(
            duplicate.message.contains("duplicate state"),
            "{}",
            duplicate.message
        );

        let unknown = parse("@asm\ns:\n  > missing\n").unwrap_err();
        assert!(unknown.message.contains("missing"), "{}", unknown.message);
        assert!(
            unknown.message.contains("states are s"),
            "{}",
            unknown.message
        );

        let missing = parse("@asm\ns:\n  ? a\n    0 s\n").unwrap_err();
        assert!(
            missing.message.contains("exits 0 and 1"),
            "{}",
            missing.message
        );

        let extra = parse("@asm\ns:\n  ? a\n    0 s\n    1 s\n    0 s\n").unwrap_err();
        assert!(extra.message.contains("extra exit"), "{}", extra.message);
    }

    #[test]
    fn caps_are_errors() {
        let mut deep = String::from("@asm\ns:\n  ? a\n");
        let mut pad = String::from("    ");
        for level in 0..33 {
            if level > 0 {
                deep.push_str(&pad);
                deep.push_str("0 ? a\n");
                pad.push_str("  ");
            }
        }
        deep.push_str(&pad);
        deep.push_str("0 s\n");
        deep.push_str(&pad);
        deep.push_str("1 s\n");
        // Close the outer exits as we unwind is not required if the deepest
        // errors first. The first decision still needs its 1 exit if nesting
        // is accepted. Exceeding 32 must fail while the nest is opened.
        let error = parse(&deep).unwrap_err();
        assert!(error.message.contains("32"), "{}\n{deep}", error.message);

        let mut many = String::from("@asm\n");
        for i in 0..=256 {
            many.push_str(&format!("s{i}:\n  > s0\n"));
        }
        let error = parse(&many).unwrap_err();
        assert!(error.message.contains("256"), "{}", error.message);

        let mut lines = String::from("@asm\ns:\n");
        for i in 0..32 {
            lines.push_str(&format!("  out{i}\n"));
        }
        lines.push_str("  > s\n");
        let error = parse(&lines).unwrap_err();
        assert!(error.message.contains("32 lines"), "{}", error.message);
    }
}
