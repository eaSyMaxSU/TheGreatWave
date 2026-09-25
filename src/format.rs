//! A stable, readable native representation of parsed timing diagrams.
use crate::scan::{Body, Cap, Doc, Group, Lane, Tick};
use std::fmt::Write;

pub(crate) fn write(doc: &Doc) -> String {
    let mut out = String::new();
    caption(&mut out, &doc.head, false, doc.xmin);
    caption(&mut out, &doc.foot, true, doc.xmin);
    if doc.hscale != 1 {
        writeln!(out, "@scale {}", doc.hscale).unwrap();
    }
    if doc.xmin != 0 || doc.xmax_cfg != 1_000_000_000_000 {
        writeln!(out, "@bounds {} {}", doc.xmin / 2, doc.xmax_cfg / 2).unwrap();
    }
    if !doc.marks {
        out.push_str("@grid off\n");
    }
    if doc.arc_font != 11.0 {
        writeln!(out, "@arc-font {}", doc.arc_font).unwrap();
    }
    if let Some(gaps) = &doc.gaps {
        directive(&mut out, "gaps", gaps);
    }
    if !out.is_empty() && (!doc.lanes.is_empty() || !doc.groups.is_empty()) {
        out.push('\n');
    }

    let mut groups: Vec<&Group> = doc.groups.iter().collect();
    groups.sort_by(|a, b| {
        a.y.cmp(&b.y)
            .then(a.x.cmp(&b.x))
            .then(b.height.cmp(&a.height))
    });
    let mut next = 0;
    let mut open: Vec<&Group> = Vec::new();
    let mut columns = Vec::new();
    for row in 0..=doc.lanes.len() {
        while open.last().is_some_and(|g| g.y + g.height <= row as i64) {
            open.pop();
            indentation(&mut out, open.len());
            out.push_str("@end\n");
        }
        while next < groups.len() && groups[next].y <= row as i64 {
            let group = groups[next];
            next += 1;
            indentation(&mut out, open.len());
            out.push_str("@group");
            if let Some(name) = &group.name {
                out.push(' ');
                value(&mut out, name, false);
            }
            out.push('\n');
            if group.height <= 0 {
                indentation(&mut out, open.len());
                out.push_str("@end\n");
            } else {
                open.push(group);
            }
        }
        if let Some(lane) = doc.lanes.get(row) {
            indentation(&mut out, open.len());
            if let Some((at, width)) = lane_line(&mut out, lane) {
                columns.push((at, width + open.len() * 2));
            }
        }
    }
    while open.pop().is_some() {
        indentation(&mut out, open.len());
        out.push_str("@end\n");
    }
    if doc.lanes.is_empty() {
        out.push_str("@empty\n");
    }
    if !doc.edges.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        for edge in &doc.edges {
            directive(&mut out, "edge", edge);
        }
    }
    // Pad in one linear copy, avoiding repeated insertion into the output string.
    let width = columns
        .iter()
        .map(|(_, width)| *width)
        .max()
        .unwrap_or(0)
        .min(40);
    let mut aligned = String::with_capacity(out.len() + columns.len() * 4);
    let mut previous = 0;
    for (at, column) in columns {
        aligned.push_str(&out[previous..at]);
        aligned.extend(std::iter::repeat_n(' ', width.saturating_sub(column)));
        previous = at;
    }
    aligned.push_str(&out[previous..]);
    aligned
}

fn indentation(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
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
            '\u{0}'..='\u{1f}' => write!(out, "\\u{:04x}", c as u32).unwrap(),
            _ => out.push(c),
        }
    }
    out.push('"');
}

fn value(out: &mut String, text: &str, token: bool) {
    let needs_quotes = text.is_empty()
        || text.trim() != text
        || text.starts_with('@')
        || text.contains("=>")
        || text.chars().any(|c| {
            c.is_control()
                || matches!(c, '#' | '\'' | '"' | '\\' | ';' | ':')
                || (token && c.is_whitespace())
        });
    if needs_quotes {
        quoted(out, text);
    } else {
        out.push_str(text);
    }
}

fn directive(out: &mut String, name: &str, text: &str) {
    write!(out, "@{name} ").unwrap();
    value(out, text, false);
    out.push('\n');
}

fn caption(out: &mut String, cap: &Cap, foot: bool, xmin: i64) {
    if let Some(text) = &cap.text {
        directive(out, if foot { "footer" } else { "title" }, text);
    }
    ticks(
        out,
        if foot { "foot-tick" } else { "tick" },
        &cap.tick,
        xmin,
    );
    ticks(
        out,
        if foot { "foot-tock" } else { "tock" },
        &cap.tock,
        xmin,
    );
    if cap.every != 0.0 {
        writeln!(
            out,
            "@{} {}",
            if foot { "foot-every" } else { "every" },
            cap.every
        )
        .unwrap();
    }
}

fn ticks(out: &mut String, name: &str, tick: &Tick, xmin: i64) {
    match tick {
        Tick::Off => {}
        Tick::Labels(labels) => {
            write!(out, "@{name}").unwrap();
            for label in labels {
                out.push(' ');
                quoted(out, label);
            }
            out.push('\n');
        }
        Tick::Series {
            offset,
            step,
            dp,
            fixed,
        } => {
            let start = if (*step - 1.0).abs() < 1e-9 && *dp == 0 {
                offset - xmin as f64 / 2.0
            } else {
                *offset
            };
            write!(out, "@{name} {start}").unwrap();
            if *fixed || *step != 1.0 || *dp != 0 {
                let padded = format!("{:.*}", *dp, step);
                let spelling = if padded.parse::<f64>().ok() == Some(*step) {
                    padded
                } else {
                    step.to_string()
                };
                write!(out, " {spelling}").unwrap();
            }
            out.push('\n');
        }
    }
}

fn lane_line(out: &mut String, lane: &Lane) -> Option<(usize, usize)> {
    let spacer = lane.name == " "
        && matches!(lane.body, Body::None)
        && lane.data.is_empty()
        && lane.period == 1.0
        && lane.phase == 0.0
        && lane.node.is_none()
        && lane.over.is_none()
        && lane.under.is_none();
    if spacer {
        out.push_str("---\n");
        return None;
    }
    let name_start = out.len();
    value(out, &lane.name, false);
    out.push(':');
    let column = if !matches!(lane.body, Body::None) || !lane.data.is_empty() {
        Some((out.len(), out[name_start..].chars().count()))
    } else {
        None
    };
    match &lane.body {
        Body::None => {}
        Body::Wave(wave) => {
            out.push(' ');
            value(out, wave, false);
        }
        Body::Path(path) => {
            out.push_str(" path ");
            value(out, path, false);
        }
    }
    if !lane.data.is_empty() {
        out.push_str(" =>");
        for label in &lane.data {
            out.push(' ');
            value(out, label, true);
        }
    }
    if lane.period != 1.0 {
        write!(out, " ; period={}", lane.period).unwrap();
    }
    if lane.phase != 0.0 {
        write!(out, " ; phase={}", lane.phase).unwrap();
    }
    for (key, option) in [
        ("node", &lane.node),
        ("over", &lane.over),
        ("under", &lane.under),
    ] {
        if let Some(option) = option {
            write!(out, " ; {key}=").unwrap();
            value(out, option, false);
        }
    }
    out.push('\n');
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_keeps_groups_metadata_and_escaped_labels() {
        let doc = crate::scan::parse(r#"{signal:[['bus',{name:'a:b',wave:'2.',data:['line\n\"two\"'],node:'a.'},null],{name:'analog',wave:['pw',{d:'M0,0 L1,1'}],period:2,phase:.5}],edge:['a~>b delay'],head:{text:'demo'}}"#).unwrap();
        assert_eq!(write(&doc), "@title demo\n\n@group bus\n  \"a:b\": 2. => \"line\\n\\\"two\\\"\" ; node=a.\n  ---\n@end\nanalog:  path M0,0 L1,1 ; period=2 ; phase=0.5\n\n@edge a~>b delay\n");
    }

    #[test]
    fn conversion_restores_tick_origin_and_keeps_explicit_precision() {
        let doc = crate::scan::parse("{signal:[],head:{tick:0,tock:'2 0.010'},foot:{tick:['zero','one']},config:{hbounds:[2,5]}}").unwrap();
        assert_eq!(
            write(&doc),
            "@tick 0\n@tock 0.02 0.010\n@foot-tick \"zero\" \"one\"\n@bounds 2 5\n@empty\n"
        );
    }
}
