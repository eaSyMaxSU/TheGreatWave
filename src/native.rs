//! The line-oriented .tgw syntax parses directly into the render model.
//! No intermediate JSON, general-purpose value tree, or runtime dependencies.
use crate::scan::{Body, Cap, Doc, Group, Lane, Tick};
use crate::Error;
use std::borrow::Cow;

struct OpenGroup {
    x: i64,
    start: usize,
    name: Option<String>,
    offset: usize,
}

pub(crate) fn parse(source: &str) -> Result<Doc, Error> {
    let mut doc = Doc {
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
    };
    let mut groups: Vec<OpenGroup> = Vec::new();
    let mut offset = 0;
    let mut empty = false;
    for raw in source.split_inclusive('\n') {
        let line_offset = offset;
        offset += raw.len();
        let line = strip_comment(raw, line_offset)?.trim();
        if line.is_empty() {
            continue;
        }
        let at = line_offset + raw.len() - raw.trim_start().len();
        let error = |text: &str| err(at, text);
        if let Some(directive) = line.strip_prefix('@') {
            let (key, value) = directive
                .split_once(char::is_whitespace)
                .unwrap_or((directive, ""));
            let value = value.trim();
            match key {
                "empty" => {
                    if !value.is_empty() {
                        return Err(error("@empty takes no arguments"));
                    }
                    empty = true;
                }
                "title" => doc.head.text = Some(text(value, at)?),
                "footer" => doc.foot.text = Some(text(value, at)?),
                "tick" => doc.head.tick = tick(value, at)?,
                "tock" => doc.head.tock = tick(value, at)?,
                "foot-tick" => doc.foot.tick = tick(value, at)?,
                "foot-tock" => doc.foot.tock = tick(value, at)?,
                "every" => doc.head.every = number(value, at, -f64::MAX, f64::MAX)?,
                "foot-every" => doc.foot.every = number(value, at, -f64::MAX, f64::MAX)?,
                "scale" => {
                    let n = number(value, at, 1.0, 100.0)?;
                    if n.fract() != 0.0 {
                        return Err(error("@scale must be an integer from 1 to 100"));
                    }
                    doc.hscale = n as i32;
                }
                "bounds" => {
                    let parts = tokens(value, at)?;
                    if parts.len() != 2 {
                        return Err(error("@bounds needs two cycle numbers: @bounds 0 10"));
                    }
                    let lo = number(&parts[0], at, -1e15, 1e15)?;
                    let hi = number(&parts[1], at, -1e15, 1e15)?;
                    if lo >= hi || lo.fract() != 0.0 || hi.fract() != 0.0 {
                        return Err(error("@bounds needs increasing integer cycle numbers"));
                    }
                    doc.xmin = lo as i64 * 2;
                    doc.xmax_cfg = hi as i64 * 2;
                }
                "grid" => {
                    doc.marks = match value {
                        "on" => true,
                        "off" => false,
                        _ => return Err(error("@grid must be on or off")),
                    }
                }
                "arc-font" => {
                    doc.arc_font = number(value, at, 0.0, 1e6)?;
                    if doc.arc_font == 0.0 {
                        return Err(error("@arc-font must be positive"));
                    }
                }
                "gaps" => doc.gaps = Some(text(value, at)?),
                "edge" => {
                    let edge = text(value, at)?;
                    if edge
                        .split_whitespace()
                        .next()
                        .is_none_or(|s| s.chars().count() < 2)
                    {
                        return Err(error(
                            "@edge needs two nodes and a connector, for example a~>b label",
                        ));
                    }
                    doc.edges.push(edge);
                }
                "group" => {
                    if groups.len() >= 64 {
                        return Err(error("group nesting exceeds 64 levels"));
                    }
                    let name = if value.is_empty() {
                        None
                    } else {
                        Some(text(value, at)?)
                    };
                    let x =
                        groups.last().map_or(10, |g| g.x) + if name.is_some() { 25 } else { 10 };
                    groups.push(OpenGroup {
                        x,
                        start: doc.lanes.len(),
                        name,
                        offset: at,
                    });
                }
                "end" => {
                    if !value.is_empty() {
                        return Err(error("@end takes no arguments"));
                    }
                    let group = groups
                        .pop()
                        .ok_or_else(|| error("@end has no matching @group"))?;
                    doc.groups.push(Group {
                        x: group.x,
                        y: group.start as i64,
                        height: (doc.lanes.len() - group.start) as i64,
                        name: group.name,
                    });
                }
                _ => return Err(error(&format!("unknown directive @{key}"))),
            }
            continue;
        }
        let indent = groups.last().map_or(10, |g| g.x);
        if line == "---" {
            doc.lanes.push(lane(" ".into(), Body::None, indent));
            continue;
        }
        let colon = find_outside(line, ":")
            .ok_or_else(|| error("expected name: waveform (or an @directive)"))?;
        let name = text(line[..colon].trim(), at)?;
        let mut parts = split_outside(&line[colon + 1..], ';');
        let body = parts.next().unwrap_or("").trim();
        let (wave, data) = if let Some(pos) = find_outside(body, "=>") {
            (&body[..pos], Some(&body[pos + 2..]))
        } else {
            (body, None)
        };
        let wave = wave.trim();
        let body = if let Some(path) = wave.strip_prefix("path ") {
            Body::Path(text(path.trim(), at)?)
        } else if wave.is_empty() {
            Body::None
        } else {
            let mut compact = text(wave, at)?;
            let mut spaced = false;
            for (i, byte) in compact.bytes().enumerate() {
                if byte == b'.' {
                    continue;
                }
                if byte.is_ascii_whitespace() {
                    spaced = true;
                    continue;
                }
                if !matches!(
                    byte,
                    b'p' | b'n' | b'P' | b'N' | b'h' | b'l' | b'H' | b'L' | b'0'
                        ..=b'9' | b'x' | b'd' | b'u' | b'z' | b'=' | b'|' | b'<' | b'>'
                ) {
                    let c = compact[i..].chars().next().unwrap();
                    return Err(error(&format!("unknown wave symbol {c:?}")));
                }
            }
            if spaced {
                compact.retain(|c| !c.is_ascii_whitespace());
            }
            Body::Wave(compact)
        };
        let mut lane = lane(
            if name.is_empty() { " ".into() } else { name },
            body,
            indent,
        );
        if let Some(data) = data {
            lane.data = tokens(data.trim(), at)?
                .into_iter()
                .map(Cow::into_owned)
                .collect();
        }
        let mut seen = 0_u8;
        for option in parts {
            let option = option.trim();
            let (key, value) = option
                .split_once('=')
                .ok_or_else(|| error("lane options use ; key=value"))?;
            let key = key.trim();
            let value = value.trim();
            let bit = match key {
                "period" => 1,
                "phase" => 2,
                "node" => 4,
                "over" => 8,
                "under" => 16,
                _ => return Err(error(&format!("unknown lane option {key:?}"))),
            };
            if seen & bit != 0 {
                return Err(error(&format!("duplicate lane option {key:?}")));
            }
            seen |= bit;
            match key {
                "period" => {
                    lane.period = number(value, at, 0.0, 1e15)?;
                    if lane.period == 0.0 {
                        return Err(error("period must be positive"));
                    }
                }
                "phase" => lane.phase = number(value, at, -1e15, 1e15)?,
                "node" => lane.node = Some(text(value, at)?),
                "over" => lane.over = Some(text(value, at)?),
                "under" => lane.under = Some(text(value, at)?),
                _ => unreachable!(),
            }
        }
        doc.lanes.push(lane);
    }
    if let Some(group) = groups.last() {
        return Err(err(group.offset, "@group is missing @end"));
    }
    if doc.lanes.is_empty() && !empty {
        return Err(err(0, "at least one signal lane is required"));
    }
    if empty && !doc.lanes.is_empty() {
        return Err(err(0, "@empty cannot be combined with signal lanes"));
    }
    let dx = doc.xmin as f64 / 2.0;
    for cap in [&mut doc.head, &mut doc.foot] {
        for tick in [&mut cap.tick, &mut cap.tock] {
            if let Tick::Series {
                offset, step, dp, ..
            } = tick
            {
                if (*step - 1.0).abs() < 1e-9 && *dp == 0 {
                    *offset += dx;
                }
            }
        }
    }
    Ok(doc)
}

fn lane(name: String, body: Body, indent: i64) -> Lane {
    Lane {
        name,
        body,
        data: Vec::new(),
        period: 1.0,
        phase: 0.0,
        node: None,
        over: None,
        under: None,
        indent,
    }
}
fn err(offset: usize, message: &str) -> Error {
    Error {
        offset,
        message: message.into(),
    }
}
fn number(s: &str, at: usize, lo: f64, hi: f64) -> Result<f64, Error> {
    s.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= lo && *n <= hi)
        .ok_or_else(|| {
            err(
                at,
                &format!("expected a finite number between {lo} and {hi}"),
            )
        })
}

fn tick(s: &str, at: usize) -> Result<Tick, Error> {
    if s == "off" {
        return Ok(Tick::Off);
    }
    let parts = tokens(s, at)?;
    if parts.is_empty() {
        return Err(err(at, "tick needs a start [step], quoted labels, or off"));
    }
    if s.starts_with(['\'', '"']) {
        return Ok(Tick::Labels(
            parts.into_iter().map(Cow::into_owned).collect(),
        ));
    }
    if parts.len() > 2 {
        return Err(err(
            at,
            "tick needs start [step]; quote values for explicit labels",
        ));
    }
    let offset = number(&parts[0], at, -f64::MAX, f64::MAX)?;
    let step = if let Some(s) = parts.get(1) {
        number(s, at, -f64::MAX, f64::MAX)?
    } else {
        1.0
    };
    let precision = |s: &str| {
        let (m, e) = s.split_once(['e', 'E']).unwrap_or((s, "0"));
        (m.split_once('.').map_or(0, |(_, s)| s.len()) as i64)
            .saturating_sub(e.parse::<i64>().unwrap_or(0))
            .clamp(0, 15) as usize
    };
    let dp = precision(parts.get(1).unwrap_or(&parts[0]));
    Ok(Tick::Series {
        offset,
        step,
        dp,
        fixed: parts.len() == 2 || dp > 0,
    })
}

// All separators/comments are recognized outside quoted strings only.
fn strip_comment(s: &str, at: usize) -> Result<&str, Error> {
    if !s.contains('\'') && !s.contains('"') {
        return Ok(s.split_once('#').map_or(s, |(before, _)| before));
    }
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && c == '\\' {
            escaped = true;
            continue;
        }
        if quote == Some(c) {
            quote = None;
        } else if quote.is_none() {
            if c == '#' {
                return Ok(&s[..i]);
            }
            if c == '\'' || c == '"' {
                quote = Some(c);
            }
        }
    }
    if quote.is_some() {
        Err(err(
            at + s.len().saturating_sub(1),
            "unterminated quoted string",
        ))
    } else {
        Ok(s)
    }
}
fn find_outside(s: &str, needle: &str) -> Option<usize> {
    if !s.contains('\'') && !s.contains('"') {
        let position = s.find(needle)?;
        if needle != ":" || !s[..position].contains('[') {
            return Some(position);
        }
    }
    let mut brackets = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && c == '\\' {
            escaped = true;
            continue;
        }
        if quote == Some(c) {
            quote = None;
        } else if quote.is_none() {
            if c == '\'' || c == '"' {
                quote = Some(c);
            } else if needle == ":" && c == '[' {
                brackets = brackets.saturating_add(1);
            } else if needle == ":" && c == ']' {
                brackets = brackets.saturating_sub(1);
            } else if brackets == 0 && s[i..].starts_with(needle) {
                return Some(i);
            }
        }
    }
    None
}
fn split_outside(s: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut rest = Some(s);
    let separator = separator.to_string();
    std::iter::from_fn(move || {
        let current = rest?;
        if let Some(i) = find_outside(current, &separator) {
            rest = Some(&current[i + separator.len()..]);
            Some(&current[..i])
        } else {
            rest = None;
            Some(current)
        }
    })
}
fn text(s: &str, at: usize) -> Result<String, Error> {
    if s.starts_with(['\'', '"']) {
        let parts = tokens(s, at)?;
        if parts.len() != 1 {
            return Err(err(at, "expected one quoted string"));
        }
        Ok(parts.into_iter().next().unwrap().into_owned())
    } else {
        Ok(s.to_string())
    }
}
fn tokens(s: &str, at: usize) -> Result<Vec<Cow<'_, str>>, Error> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        if c != '\'' && c != '"' {
            let start = i;
            while i < s.len() && !s[i..].chars().next().unwrap().is_whitespace() {
                i += s[i..].chars().next().unwrap().len_utf8();
            }
            result.push(Cow::Borrowed(&s[start..i]));
            continue;
        }
        let quote = c;
        i += 1;
        let mut value = String::new();
        let mut closed = false;
        while i < s.len() {
            let c = s[i..].chars().next().unwrap();
            i += c.len_utf8();
            if c == quote {
                closed = true;
                break;
            }
            if c == '\\' {
                let escaped = s[i..]
                    .chars()
                    .next()
                    .ok_or_else(|| err(at + i, "unfinished escape"))?;
                i += escaped.len_utf8();
                value.push(match escaped {
                    'u' => unicode_escape(s, &mut i, at)?,
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '\'' => '\'',
                    '"' => '"',
                    _ => {
                        return Err(err(
                            at + i,
                            "unsupported escape; use n, r, t, a quote, or a backslash",
                        ))
                    }
                });
            } else {
                value.push(c);
            }
        }
        if !closed {
            return Err(err(at + i, "unterminated quoted string"));
        }
        if i < s.len() && !s[i..].chars().next().unwrap().is_whitespace() {
            return Err(err(at + i, "quoted labels must be separated by whitespace"));
        }
        result.push(Cow::Owned(value));
    }
    Ok(result)
}

fn unicode_escape(s: &str, i: &mut usize, at: usize) -> Result<char, Error> {
    fn hex(s: &str, i: &mut usize, at: usize) -> Result<u32, Error> {
        let digits = s
            .as_bytes()
            .get(*i..*i + 4)
            .filter(|digits| digits.iter().all(u8::is_ascii_hexdigit))
            .ok_or_else(|| err(at + *i, "Unicode escapes need four hex digits"))?;
        let value = u32::from_str_radix(std::str::from_utf8(digits).unwrap(), 16).unwrap();
        *i += 4;
        Ok(value)
    }
    let mut code = hex(s, i, at)?;
    if (0xd800..=0xdbff).contains(&code) {
        if !s[*i..].starts_with("\\u") {
            return Err(err(at + *i, "high surrogate needs a low surrogate"));
        }
        *i += 2;
        let low = hex(s, i, at)?;
        if !(0xdc00..=0xdfff).contains(&low) {
            return Err(err(at + *i, "invalid low surrogate"));
        }
        code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
    }
    char::from_u32(code).ok_or_else(|| err(at + *i, "invalid Unicode scalar"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_lanes_options_groups_and_comments() {
        let d = parse("# protocol\n@title Transfer\n@tick 0\n@scale 2\n@group Master\n  clk: P... ; period=2\n  data [7:0]: x3.4 => head \"hello #world; =>\" ; phase=.25 ; node=..a.\n@end\n---\n@edge a->b delay\n").unwrap();
        assert_eq!(d.lanes.len(), 3);
        assert_eq!(d.lanes[1].name, "data [7:0]");
        assert_eq!(d.lanes[1].data, ["head", "hello #world; =>"]);
        assert_eq!(d.lanes[1].phase, 0.25);
        assert_eq!(d.groups[0].height, 2);
    }
    #[test]
    fn quotes_paths_and_tick_labels() {
        let d = parse("@tick \"0\" \"1\" \"two words\"\n\"时钟: 🌊\": 0. 1.\nanalog: path M0,0 C1,0 1,1 2,1\n").unwrap();
        assert!(matches!(&d.head.tick, Tick::Labels(v) if v.len()==3));
        assert!(matches!(&d.lanes[0].body, Body::Wave(w) if w == "0.1."));
        assert!(matches!(&d.lanes[1].body, Body::Path(d) if d.ends_with("2,1")));
    }
    #[test]
    fn helpful_errors_and_bounded_nesting() {
        for input in [
            "",
            "clk p...",
            "clk: abc",
            "@end\nclk:p",
            "@group Missing\nclk:p",
            "@nope x\nclk:p",
            "clk:p;period=0",
            "clk:p;phase=NaN",
            "clk:p;phase=1;phase=2",
            "clk:p;typo=2",
            "@scale 1.5\nclk:p",
            "@bounds 2 1\nclk:p",
            "data: 2 => \"oops",
            "data: 2 => \"a\"b",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
        assert!(parse(&format!(
            "{}clk:p\n{}",
            "@group nested\n".repeat(65),
            "@end\n".repeat(65)
        ))
        .is_err());
    }
}
