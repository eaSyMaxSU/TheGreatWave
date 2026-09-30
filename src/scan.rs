//! One pass from WaveJSON bytes into lanes. No generic value tree.

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

enum Node {
    Name(String),
    Lane(Box<Lane>),
    Group(Vec<Node>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

pub(crate) fn parse(source: &str) -> Result<Doc, Error> {
    let mut p = P {
        s: source.as_bytes(),
        i: 0,
        depth: 0,
    };
    p.ws()?;
    if p.peek() != Some(b'{') {
        return Err(p.err("expected an object"));
    }
    let mut doc = Doc::default();
    let mut signal: Option<Vec<Node>> = None;
    p.i += 1;
    p.enter()?;
    let mut first = true;
    loop {
        if !p.item(b'}', &mut first)? {
            break;
        }
        let key = p.key()?;
        p.ws()?;
        if p.peek() != Some(b':') {
            return Err(p.err("expected ':'"));
        }
        p.i += 1;
        match key.as_str() {
            "signal" => signal = Some(p.signal_array()?),
            "edge" => doc.edges = p.edges()?,
            "head" => doc.head = p.cap()?,
            "foot" => doc.foot = p.cap()?,
            "config" => p.config(&mut doc)?,
            "gaps" => {
                p.ws()?;
                if matches!(p.peek(), Some(b'"' | b'\'')) {
                    doc.gaps = Some(p.string()?);
                } else {
                    p.skip()?;
                }
            }
            _ => p.skip()?,
        }
    }
    p.ws()?;
    if p.i < p.s.len() {
        return Err(p.err("trailing input"));
    }
    let nodes = signal.ok_or_else(|| Error {
        offset: 0,
        message: "signal array is required".to_string(),
    })?;
    let mut st = Walk {
        x: 0,
        y: 0,
        xx: 0,
        name: None,
        lanes: Vec::new(),
        groups: Vec::new(),
    };
    walk(nodes, &mut st);
    doc.lanes = st.lanes;
    doc.groups = st.groups;
    let dx = doc.xmin as f64 / 2.0;
    shift_tick(&mut doc.head.tick, dx);
    shift_tick(&mut doc.foot.tick, dx);
    shift_tick(&mut doc.head.tock, dx);
    shift_tick(&mut doc.foot.tock, dx);
    Ok(doc)
}

fn shift_tick(tick: &mut Tick, dx: f64) {
    if dx == 0.0 {
        return;
    }
    if let Tick::Series {
        offset, step, dp, ..
    } = tick
    {
        if (*step - 1.0).abs() < 1e-9 && *dp == 0 {
            *offset += dx;
        }
    }
}

struct Walk {
    x: i64,
    y: i64,
    xx: i64,
    name: Option<String>,
    lanes: Vec<Lane>,
    groups: Vec<Group>,
}

fn walk(nodes: Vec<Node>, st: &mut Walk) {
    let mut delta = 10i64;
    let mut name = None;
    if let Some(Node::Name(n)) = nodes.first() {
        name = Some(n.clone());
        delta = 25;
    }
    st.x += delta;
    for n in nodes {
        match n {
            Node::Group(ch) => {
                let old = st.y;
                walk(ch, st);
                st.groups.push(Group {
                    x: st.xx,
                    y: old,
                    height: st.y - old,
                    name: st.name.clone(),
                    leading: Vec::new(),
                    trailing: None,
                    end_leading: Vec::new(),
                    end_trailing: None,
                });
            }
            Node::Lane(lane) => {
                let mut lane = *lane;
                lane.indent = st.x;
                st.lanes.push(lane);
                st.y += 1;
            }
            Node::Name(_) => {}
        }
    }
    st.xx = st.x;
    st.x -= delta;
    st.name = name;
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn err(&self, message: &str) -> Error {
        Error {
            offset: self.i,
            message: message.to_string(),
        }
    }

    fn enter(&mut self) -> Result<(), Error> {
        // Bound both parser recursion and the later recursive group walk/drop.
        if self.depth >= 128 {
            return Err(self.err("nesting exceeds 128 levels"));
        }
        self.depth += 1;
        Ok(())
    }

    fn item(&mut self, end: u8, first: &mut bool) -> Result<bool, Error> {
        self.ws()?;
        if self.peek() == Some(end) {
            self.i += 1;
            self.depth -= 1;
            return Ok(false);
        }
        if !*first {
            if self.peek() != Some(b',') {
                return Err(self.err("expected ',' or closing delimiter"));
            }
            self.i += 1;
            self.ws()?;
            if self.peek() == Some(end) {
                self.i += 1;
                self.depth -= 1;
                return Ok(false);
            }
        }
        *first = false;
        Ok(true)
    }

    fn ws(&mut self) -> Result<(), Error> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.i += 1,
                Some(b'/') if self.s.get(self.i + 1) == Some(&b'/') => {
                    self.i += 2;
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                Some(b'/') if self.s.get(self.i + 1) == Some(&b'*') => {
                    self.i += 2;
                    while self.i + 1 < self.s.len()
                        && !(self.s[self.i] == b'*' && self.s[self.i + 1] == b'/')
                    {
                        self.i += 1;
                    }
                    if self.i + 1 >= self.s.len() {
                        return Err(self.err("unterminated block comment"));
                    }
                    self.i += 2;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn key(&mut self) -> Result<String, Error> {
        self.ws()?;
        match self.peek() {
            Some(b'"' | b'\'') => self.string(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => self.ident(),
            _ => Err(self.err("expected a key")),
        }
    }

    fn ident(&mut self) -> Result<String, Error> {
        let start = self.i;
        self.i += 1;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b'$')
        {
            self.i += 1;
        }
        Ok(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }

    fn string(&mut self) -> Result<String, Error> {
        let quote = self.peek().ok_or_else(|| self.err("expected a string"))?;
        if quote != b'"' && quote != b'\'' {
            return Err(self.err("expected a string"));
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            // Copy whole UTF-8 spans. Most waveform strings contain no escapes.
            let start = self.i;
            while matches!(self.peek(), Some(c) if c != quote && c != b'\\' && c >= 0x20) {
                self.i += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.err("bad utf-8"))?,
            );
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.i += 1;
            if c == quote {
                return Ok(out);
            }
            if c == b'\\' {
                let e = self.peek().ok_or_else(|| self.err("bad escape"))?;
                self.i += 1;
                match e {
                    b'"' | b'\'' | b'\\' | b'/' => out.push(e as char),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let mut code = self.hex_escape(4)?;
                        if (0xd800..=0xdbff).contains(&code) {
                            if !self.s[self.i..].starts_with(b"\\u") {
                                return Err(self.err("expected a low unicode surrogate"));
                            }
                            self.i += 2;
                            let low = self.hex_escape(4)?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return Err(self.err("expected a low unicode surrogate"));
                            }
                            code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
                        }
                        out.push(
                            char::from_u32(code)
                                .ok_or_else(|| self.err("unpaired unicode surrogate"))?,
                        );
                    }
                    b'x' => out.push(char::from_u32(self.hex_escape(2)?).unwrap()),
                    b'v' => out.push('\u{000b}'),
                    b'0' if !matches!(self.peek(), Some(b'0'..=b'9')) => out.push('\0'),
                    b'\n' => {}
                    b'\r' => {
                        if self.peek() == Some(b'\n') {
                            self.i += 1;
                        }
                    }
                    _ => return Err(self.err("bad escape")),
                }
            } else {
                return Err(self.err("unescaped control character in string"));
            }
        }
    }

    fn hex_escape(&mut self, width: usize) -> Result<u32, Error> {
        let bytes = self
            .s
            .get(self.i..self.i + width)
            .ok_or_else(|| self.err("bad unicode escape"))?;
        let mut code = 0;
        for &byte in bytes {
            let digit = (byte as char)
                .to_digit(16)
                .ok_or_else(|| self.err("bad unicode escape"))?;
            code = code * 16 + digit;
        }
        self.i += width;
        Ok(code)
    }

    fn number(&mut self) -> Result<f64, Error> {
        self.ws()?;
        let start = self.i;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let mut digits = false;
        if self.peek() == Some(b'.') {
            self.i += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                digits = true;
                self.i += 1;
            }
        } else {
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                digits = true;
                self.i += 1;
            }
            if self.peek() == Some(b'.') {
                self.i += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    digits = true;
                    self.i += 1;
                }
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            let mut exp = false;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                exp = true;
                self.i += 1;
            }
            if !exp {
                return Err(self.err("bad number"));
            }
        }
        if !digits || self.i == start {
            return Err(self.err("expected a number"));
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        let n = text.parse::<f64>().map_err(|_| self.err("bad number"))?;
        if !n.is_finite() {
            return Err(self.err("number must be finite"));
        }
        Ok(n)
    }

    fn boolean(&mut self) -> Result<bool, Error> {
        if self.s[self.i..].starts_with(b"true") {
            self.i += 4;
            Ok(true)
        } else if self.s[self.i..].starts_with(b"false") {
            self.i += 5;
            Ok(false)
        } else {
            Err(self.err("expected a boolean"))
        }
    }

    fn skip(&mut self) -> Result<(), Error> {
        self.ws()?;
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                self.enter()?;
                let mut first = true;
                loop {
                    if !self.item(b'}', &mut first)? {
                        return Ok(());
                    }
                    let _ = self.key()?;
                    self.ws()?;
                    if self.peek() != Some(b':') {
                        return Err(self.err("expected ':'"));
                    }
                    self.i += 1;
                    self.skip()?;
                }
            }
            Some(b'[') => {
                self.i += 1;
                self.enter()?;
                let mut first = true;
                loop {
                    if !self.item(b']', &mut first)? {
                        return Ok(());
                    }
                    self.skip()?;
                }
            }
            Some(b'"' | b'\'') => {
                self.string()?;
                Ok(())
            }
            Some(b't') | Some(b'f') => {
                self.boolean()?;
                Ok(())
            }
            Some(b'n') => {
                if self.s[self.i..].starts_with(b"null") {
                    self.i += 4;
                    Ok(())
                } else {
                    Err(self.err("expected a value"))
                }
            }
            Some(b'-' | b'+' | b'.' | b'0'..=b'9') => {
                self.number()?;
                Ok(())
            }
            _ => Err(self.err("expected a value")),
        }
    }

    fn signal_array(&mut self) -> Result<Vec<Node>, Error> {
        self.ws()?;
        if self.peek() != Some(b'[') {
            return Err(self.err("signal must be an array"));
        }
        self.nodes()
    }

    fn nodes(&mut self) -> Result<Vec<Node>, Error> {
        self.i += 1;
        let mut out = Vec::new();
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b']', &mut first)? {
                return Ok(out);
            }
            out.push(self.node()?);
        }
    }

    fn node(&mut self) -> Result<Node, Error> {
        self.ws()?;
        match self.peek() {
            Some(b'[') => Ok(Node::Group(self.nodes()?)),
            Some(b'{') => Ok(Node::Lane(Box::new(self.lane()?))),
            Some(b'"' | b'\'') => Ok(Node::Name(self.string()?)),
            Some(b'-' | b'+' | b'.' | b'0'..=b'9') => {
                let n = self.number()?;
                Ok(Node::Name(num_name(n)))
            }
            Some(b'n') => {
                self.skip()?;
                Ok(Node::Lane(Box::new(empty_lane())))
            }
            _ => Err(self.err("expected a signal")),
        }
    }

    fn lane(&mut self) -> Result<Lane, Error> {
        self.i += 1;
        let mut lane = empty_lane();
        let mut named = false;
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b'}', &mut first)? {
                break;
            }
            let key = self.key()?;
            self.ws()?;
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            match key.as_str() {
                "name" => {
                    lane.name = self.scalar_string()?;
                    named = true;
                }
                "wave" => lane.body = self.body()?,
                "data" => lane.data = self.data()?,
                "period" => {
                    let n = self.number()?;
                    if n < 0.0 {
                        return Err(self.err("period must be positive"));
                    }
                    if n > 1.0e15 {
                        return Err(self.err("period is out of range"));
                    }
                    lane.period = if n == 0.0 { 1.0 } else { n };
                }
                "phase" => {
                    lane.phase = self.number()?;
                    if lane.phase.abs() > 1.0e15 {
                        return Err(self.err("phase is out of range"));
                    }
                }
                "node" => {
                    let at = self.i;
                    lane.node = Some(self.scalar_string()?);
                    lane.node_at = at;
                }
                "over" => lane.over = Some(self.scalar_string()?),
                "under" => lane.under = Some(self.scalar_string()?),
                _ => self.skip()?,
            }
        }
        if !named || lane.name.is_empty() {
            lane.name = " ".to_string();
        }
        Ok(lane)
    }

    fn body(&mut self) -> Result<Body, Error> {
        self.ws()?;
        match self.peek() {
            Some(b'"' | b'\'') => Ok(Body::Wave(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut kind = String::new();
                let mut path = None;
                let mut seen = false;
                self.enter()?;
                let mut first = true;
                loop {
                    if !self.item(b']', &mut first)? {
                        break;
                    }
                    if !seen && matches!(self.peek(), Some(b'"' | b'\'')) {
                        kind = self.string()?;
                        seen = true;
                    } else if path.is_none() && self.peek() == Some(b'{') {
                        path = Some(self.d_object()?);
                        seen = true;
                    } else {
                        self.skip()?;
                        seen = true;
                    }
                }
                if kind == "pw" {
                    Ok(Body::Path(path.unwrap_or_default()))
                } else {
                    Ok(Body::None)
                }
            }
            _ => {
                self.skip()?;
                Ok(Body::None)
            }
        }
    }

    fn d_object(&mut self) -> Result<String, Error> {
        self.i += 1;
        let mut d = String::new();
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b'}', &mut first)? {
                return Ok(d);
            }
            let key = self.key()?;
            self.ws()?;
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            if key == "d" {
                self.ws()?;
                if matches!(self.peek(), Some(b'"' | b'\'')) {
                    d = self.string()?;
                } else {
                    self.skip()?;
                }
            } else {
                self.skip()?;
            }
        }
    }

    fn data(&mut self) -> Result<Vec<String>, Error> {
        self.ws()?;
        match self.peek() {
            Some(b'"' | b'\'') => Ok(split_ws(&self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                self.enter()?;
                let mut first = true;
                loop {
                    if !self.item(b']', &mut first)? {
                        return Ok(out);
                    }
                    out.push(self.scalar_string()?);
                }
            }
            Some(b'n') => {
                self.skip()?;
                Ok(Vec::new())
            }
            _ => {
                self.skip()?;
                Ok(Vec::new())
            }
        }
    }

    fn scalar_string(&mut self) -> Result<String, Error> {
        self.ws()?;
        match self.peek() {
            Some(b'"' | b'\'') => self.string(),
            Some(b't') | Some(b'f') => Ok(if self.boolean()? {
                "true".into()
            } else {
                "false".into()
            }),
            Some(b'n') => {
                self.skip()?;
                Ok(String::new())
            }
            Some(b'-' | b'+' | b'.' | b'0'..=b'9') => Ok(num_name(self.number()?)),
            _ => {
                self.skip()?;
                Ok(String::new())
            }
        }
    }

    fn edges(&mut self) -> Result<Vec<Edge>, Error> {
        self.ws()?;
        if self.peek() != Some(b'[') {
            return Err(self.err("expected an array"));
        }
        self.i += 1;
        let mut out = Vec::new();
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b']', &mut first)? {
                return Ok(out);
            }
            self.ws()?;
            if matches!(self.peek(), Some(b'"' | b'\'')) {
                let at = self.i;
                out.push(parse_edge(&self.string()?, at)?);
            } else {
                self.skip()?;
            }
        }
    }

    fn cap(&mut self) -> Result<Cap, Error> {
        self.ws()?;
        if self.peek() != Some(b'{') {
            self.skip()?;
            return Ok(Cap::default());
        }
        self.i += 1;
        let mut cap = Cap::default();
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b'}', &mut first)? {
                return Ok(cap);
            }
            let key = self.key()?;
            self.ws()?;
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            match key.as_str() {
                "text" => cap.text = Some(self.scalar_string()?),
                "tick" => cap.tick = self.tick()?,
                "tock" => cap.tock = self.tick()?,
                "every" => cap.every = self.number()?,
                _ => self.skip()?,
            }
        }
    }

    fn tick(&mut self) -> Result<Tick, Error> {
        self.ws()?;
        match self.peek() {
            Some(b'"' | b'\'') => Ok(tick_from_parts(&split_ws(&self.string()?))),
            Some(b'[') => {
                self.i += 1;
                let mut parts = Vec::new();
                self.enter()?;
                let mut first = true;
                loop {
                    if !self.item(b']', &mut first)? {
                        break;
                    }
                    parts.push(self.scalar_string()?);
                }
                Ok(tick_from_parts(&parts))
            }
            Some(b't') | Some(b'f') => {
                let v = if self.boolean()? { 1.0 } else { 0.0 };
                Ok(Tick::Series {
                    offset: v,
                    step: 1.0,
                    dp: 0,
                    fixed: false,
                })
            }
            Some(b'n') => {
                self.skip()?;
                Ok(Tick::Off)
            }
            _ => {
                let n = self.number()?;
                Ok(Tick::Series {
                    offset: n,
                    step: 1.0,
                    dp: 0,
                    fixed: false,
                })
            }
        }
    }

    fn config(&mut self, doc: &mut Doc) -> Result<(), Error> {
        self.ws()?;
        if self.peek() != Some(b'{') {
            self.skip()?;
            return Ok(());
        }
        self.i += 1;
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b'}', &mut first)? {
                return Ok(());
            }
            let key = self.key()?;
            self.ws()?;
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            match key.as_str() {
                "hscale" => {
                    let n = self.number()?;
                    let mut h = if n > 0.0 { n.round() as i32 } else { 1 };
                    if h > 100 {
                        h = 100;
                    }
                    if h > 0 {
                        doc.hscale = h;
                    }
                }
                "hbounds" => {
                    let pair = self.num_pair()?;
                    if let Some((a, b)) = pair {
                        if a.abs() > 1.0e15 || b.abs() > 1.0e15 {
                            return Err(self.err("horizontal bounds are out of range"));
                        }
                        let lo = a.floor();
                        let hi = b.ceil();
                        if lo < hi {
                            doc.xmin = 2 * lo as i64;
                            doc.xmax_cfg = 2 * hi.floor() as i64;
                        }
                    }
                }
                "marks" => {
                    self.ws()?;
                    if matches!(self.peek(), Some(b't' | b'f')) {
                        doc.marks = self.boolean()?;
                    } else {
                        self.skip()?;
                    }
                }
                "arcFontSize" => {
                    doc.arc_font = self.number()?;
                    if doc.arc_font <= 0.0 || doc.arc_font > 1.0e6 {
                        return Err(self.err("arc font size must be between 0 and 1000000"));
                    }
                }
                _ => self.skip()?,
            }
        }
    }

    fn num_pair(&mut self) -> Result<Option<(f64, f64)>, Error> {
        self.ws()?;
        if self.peek() != Some(b'[') {
            self.skip()?;
            return Ok(None);
        }
        self.i += 1;
        let mut nums = [0.0; 2];
        let mut count = 0;
        self.enter()?;
        let mut first = true;
        loop {
            if !self.item(b']', &mut first)? {
                break;
            }
            self.ws()?;
            if matches!(self.peek(), Some(b'-' | b'+' | b'.' | b'0'..=b'9')) {
                let n = self.number()?;
                if count < nums.len() {
                    nums[count] = n;
                    count += 1;
                }
            } else {
                self.skip()?;
            }
        }
        if count == 2 {
            Ok(Some((nums[0], nums[1])))
        } else {
            Ok(None)
        }
    }
}

fn empty_lane() -> Lane {
    Lane {
        name: " ".to_string(),
        body: Body::None,
        data: Vec::new(),
        period: 1.0,
        phase: 0.0,
        node: None,
        node_at: 0,
        over: None,
        under: None,
        indent: 0,
        leading: Vec::new(),
        trailing: None,
    }
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

fn split_ws(s: &str) -> Vec<String> {
    s.split_whitespace().map(|p| p.to_string()).collect()
}

fn tick_from_parts(parts: &[String]) -> Tick {
    if parts.is_empty() {
        return Tick::Off;
    }
    if parts.len() == 1 {
        if let Ok(n) = parts[0].parse::<f64>() {
            if n.is_finite() {
                return Tick::Series {
                    offset: n,
                    step: 1.0,
                    dp: 0,
                    fixed: false,
                };
            }
        }
        return Tick::Labels(parts.to_vec());
    }
    if parts.len() == 2 {
        if let (Ok(offset), Ok(step)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
            if offset.is_finite() && step.is_finite() && (offset * step).is_finite() {
                let dp = decimal_places(&parts[1]);
                return Tick::Series {
                    offset: step * offset,
                    step,
                    dp,
                    fixed: true,
                };
            }
        }
    }
    Tick::Labels(parts.to_vec())
}

fn decimal_places(s: &str) -> usize {
    let (mantissa, exponent) = s.split_once(['e', 'E']).unwrap_or((s, "0"));
    let fraction = mantissa.split_once('.').map_or(0, |(_, s)| s.len());
    let exponent = exponent.parse::<i64>().unwrap_or(0);
    // Extra precision does not add useful f64 digits and can allocate huge labels.
    (fraction.min(i64::MAX as usize) as i64)
        .saturating_sub(exponent)
        .clamp(0, 15) as usize
}

fn num_name(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1.0e15 {
        format!("{}", n as i64)
    } else {
        let mut s = format!("{n}");
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Body, Tick};

    #[test]
    fn numeric_fields_accept_whitespace_and_comments() {
        let doc = parse(
            r#"{
            signal: [{name: 'clk', wave: 'p.', period: /* cycles */ 2, phase: -0.25}],
            config: {hscale: 3, hbounds: [ -1, 5 ], arcFontSize: 12.5},
            head: {tick: 0, every: 2},
        }"#,
        )
        .unwrap();
        assert_eq!(doc.lanes[0].period, 2.0);
        assert_eq!(doc.lanes[0].phase, -0.25);
        assert_eq!((doc.hscale, doc.xmin, doc.xmax_cfg), (3, -2, 10));
        assert_eq!(doc.arc_font, 12.5);
        assert_eq!(doc.head.every, 2.0);
    }

    #[test]
    fn strings_preserve_unicode_and_decode_surrogate_pairs() {
        let doc = parse(r#"{signal:[{name:'时钟 \uD83C\uDF0A',wave:'01',data:['\x41\u03b1']}]}"#)
            .unwrap();
        assert_eq!(doc.lanes[0].name, "时钟 🌊");
        assert_eq!(doc.lanes[0].data, ["Aα"]);
        assert!(matches!(&doc.lanes[0].body, Body::Wave(wave) if wave == "01"));
        for bad in [r"\uD800", r"\uDC00", r"\uD800\u0041", r"\uZZZZ"] {
            assert!(parse(&format!("{{signal:[{{name:'{bad}'}}]}}")).is_err());
        }
        assert!(parse("{signal:[{name:'raw\nnewline'}]}").is_err());
        let continued = parse("{signal:[{name:'line\\\r\ncontinued'}]}").unwrap();
        assert_eq!(continued.lanes[0].name, "linecontinued");
    }

    #[test]
    fn separators_are_required_in_all_collections() {
        for bad in [
            "{signal:[] config:{}}",
            "{signal:[{} {}]}",
            "{signal:[,{}]}",
            "{signal:[{},,{}]}",
            "{signal:[{name:'a' wave:'0'}]}",
            "{signal:[],edge:['a' 'b']}",
            "{signal:[],head:{tick:[0 1]}}",
            "{signal:[],unknown:{a:true b:false}}",
            "{signal:[],unknown:[truefalse]}",
            "{signal:[],config:{hbounds:[0 1]}}",
            "{signal:[{wave:['pw' {d:'M0,0'}]}]}",
        ] {
            assert!(parse(bad).is_err(), "accepted malformed source: {bad}");
        }
        assert!(parse("{signal:[{name:'a',},],config:{marks:true,},}").is_ok());
    }

    #[test]
    fn malformed_comments_and_excessive_nesting_return_errors() {
        for bad in ["{signal:[]} /*", "{signal:[]} /*/", "{/* no end"] {
            assert!(parse(bad).unwrap_err().message.contains("comment"));
        }
        for prefix in ["{signal:", "{signal:[],unknown:"] {
            let nested = format!("{prefix}{}{} }}", "[".repeat(200), "]".repeat(200));
            assert!(parse(&nested).unwrap_err().message.contains("nesting"));
        }
        assert!(parse("{signal:[]} // trailing comment").is_ok());
    }

    #[test]
    fn invalid_numeric_geometry_returns_errors() {
        for bad in [
            "{signal:[{period:1e999}]}",
            "{signal:[{period:1e300,node:'a'}]}",
            "{signal:[{period:-1}]}",
            "{signal:[{phase:1e300}]}",
            "{signal:[],config:{hbounds:[-1e300,1e300]}}",
            "{signal:[],config:{arcFontSize:0}}",
            "{signal:[],config:{arcFontSize:1e300}}",
        ] {
            assert!(parse(bad).is_err(), "accepted invalid geometry: {bad}");
        }
        assert_eq!(parse("{signal:[{period:0}]}").unwrap().lanes[0].period, 1.0);
    }

    #[test]
    fn tick_precision_handles_exponents_and_is_bounded() {
        let doc = parse("{signal:[],head:{tick:'0 1e-3'},foot:{tick:'NaN'}}").unwrap();
        assert!(matches!(doc.head.tick, Tick::Series { dp: 3, step, .. } if step == 0.001));
        assert!(matches!(doc.foot.tick, Tick::Labels(_)));
        let source = format!("{{signal:[],head:{{tick:'0 0.{}1'}}}}", "0".repeat(1000));
        assert!(matches!(
            parse(&source).unwrap().head.tick,
            Tick::Series { dp: 15, .. }
        ));
    }

    #[test]
    fn nested_groups_keep_names_extents_and_lane_order() {
        let doc =
            parse("{signal:[['outer',{name:'a'},['inner',{name:'b'}],{name:'c'}],{name:'d'}]}")
                .unwrap();
        assert_eq!(
            doc.lanes
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        assert_eq!(doc.groups[0].name.as_deref(), Some("inner"));
        assert_eq!((doc.groups[0].y, doc.groups[0].height), (1, 1));
        assert_eq!(doc.groups[1].name.as_deref(), Some("outer"));
        assert_eq!((doc.groups[1].y, doc.groups[1].height), (0, 3));
    }
}
