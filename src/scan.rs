//! One pass from WaveJSON bytes into lanes. No generic value tree.

use crate::Error;

#[derive(Clone, Debug)]
pub(crate) struct Doc {
    pub lanes: Vec<Lane>,
    pub groups: Vec<Group>,
    pub edges: Vec<String>,
    pub head: Cap,
    pub foot: Cap,
    pub hscale: i32,
    pub xmin: i64,
    pub xmax_cfg: i64,
    pub marks: bool,
    pub arc_font: f64,
    pub gaps: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Lane {
    pub name: String,
    pub body: Body,
    pub data: Vec<String>,
    pub period: f64,
    pub phase: f64,
    pub node: Option<String>,
    pub over: Option<String>,
    pub under: Option<String>,
    pub indent: i64,
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
    Series { offset: f64, step: f64, dp: usize, fixed: bool },
    Labels(Vec<String>),
}

enum Node {
    Name(String),
    Lane(Lane),
    Group(Vec<Node>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

pub(crate) fn parse(source: &str) -> Result<Doc, Error> {
    let mut p = P { s: source.as_bytes(), i: 0 };
    p.ws();
    if p.peek() != Some(b'{') {
        return Err(p.err("expected an object"));
    }
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
    let mut signal: Option<Vec<Node>> = None;
    p.i += 1;
    loop {
        p.ws();
        if p.peek() == Some(b'}') {
            p.i += 1;
            break;
        }
        if p.peek() == Some(b',') {
            p.i += 1;
            p.ws();
            if p.peek() == Some(b'}') {
                p.i += 1;
                break;
            }
        }
        let key = p.key()?;
        p.ws();
        if p.peek() != Some(b':') {
            return Err(p.err("expected ':'"));
        }
        p.i += 1;
        match key.as_str() {
            "signal" => signal = Some(p.signal_array()?),
            "edge" => doc.edges = p.string_array()?,
            "head" => doc.head = p.cap()?,
            "foot" => doc.foot = p.cap()?,
            "config" => p.config(&mut doc)?,
            "gaps" => {
                p.ws();
                if matches!(p.peek(), Some(b'"' | b'\'')) {
                    doc.gaps = Some(p.string()?);
                } else {
                    p.skip()?;
                }
            }
            _ => p.skip()?,
        }
    }
    p.ws();
    if p.i < p.s.len() {
        return Err(p.err("trailing input"));
    }
    let nodes = signal.ok_or_else(|| Error {
        offset: 0,
        message: "signal array is required".to_string(),
    })?;
    let mut st = Walk { x: 0, y: 0, xx: 0, name: None, lanes: Vec::new(), groups: Vec::new() };
    walk(&nodes, &mut st);
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
    if let Tick::Series { offset, step, dp, .. } = tick {
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

fn walk(nodes: &[Node], st: &mut Walk) {
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
                });
            }
            Node::Lane(l) => {
                let mut lane = l.clone();
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
        Error { offset: self.i, message: message.to_string() }
    }

    fn ws(&mut self) {
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
                    if self.i + 1 < self.s.len() {
                        self.i += 2;
                    }
                }
                _ => break,
            }
        }
    }

    fn key(&mut self) -> Result<String, Error> {
        self.ws();
        match self.peek() {
            Some(b'"' | b'\'') => self.string(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => self.ident(),
            _ => Err(self.err("expected a key")),
        }
    }

    fn ident(&mut self) -> Result<String, Error> {
        let start = self.i;
        self.i += 1;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b'$') {
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
        while let Some(c) = self.peek() {
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
                        if self.i + 4 > self.s.len() {
                            return Err(self.err("bad unicode escape"));
                        }
                        let hex = std::str::from_utf8(&self.s[self.i..self.i + 4])
                            .map_err(|_| self.err("bad unicode escape"))?;
                        let code = u32::from_str_radix(hex, 16)
                            .map_err(|_| self.err("bad unicode escape"))?;
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        self.i += 4;
                    }
                    _ => return Err(self.err("bad escape")),
                }
            } else {
                // UTF-8 sequence starting at the byte we already consumed.
                let start = self.i - 1;
                let width = utf8_width(c);
                if width == 1 {
                    out.push(c as char);
                } else {
                    if start + width > self.s.len() {
                        return Err(self.err("bad utf-8"));
                    }
                    let text = std::str::from_utf8(&self.s[start..start + width])
                        .map_err(|_| self.err("bad utf-8"))?;
                    out.push_str(text);
                    self.i = start + width;
                }
            }
        }
        Err(self.err("unterminated string"))
    }

    fn number(&mut self) -> Result<f64, Error> {
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
        text.parse::<f64>().map_err(|_| self.err("bad number"))
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
        self.ws();
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                loop {
                    self.ws();
                    if self.peek() == Some(b'}') {
                        self.i += 1;
                        return Ok(());
                    }
                    if self.peek() == Some(b',') {
                        self.i += 1;
                        self.ws();
                        if self.peek() == Some(b'}') {
                            self.i += 1;
                            return Ok(());
                        }
                    }
                    let _ = self.key()?;
                    self.ws();
                    if self.peek() != Some(b':') {
                        return Err(self.err("expected ':'"));
                    }
                    self.i += 1;
                    self.skip()?;
                }
            }
            Some(b'[') => {
                self.i += 1;
                loop {
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        return Ok(());
                    }
                    if self.peek() == Some(b',') {
                        self.i += 1;
                        self.ws();
                        if self.peek() == Some(b']') {
                            self.i += 1;
                            return Ok(());
                        }
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
        self.ws();
        if self.peek() != Some(b'[') {
            return Err(self.err("signal must be an array"));
        }
        self.nodes()
    }

    fn nodes(&mut self) -> Result<Vec<Node>, Error> {
        self.i += 1;
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(b']') {
                self.i += 1;
                return Ok(out);
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(out);
                }
            }
            out.push(self.node()?);
        }
    }

    fn node(&mut self) -> Result<Node, Error> {
        self.ws();
        match self.peek() {
            Some(b'[') => Ok(Node::Group(self.nodes()?)),
            Some(b'{') => Ok(Node::Lane(self.lane()?)),
            Some(b'"' | b'\'') => Ok(Node::Name(self.string()?)),
            Some(b'-' | b'+' | b'.' | b'0'..=b'9') => {
                let n = self.number()?;
                Ok(Node::Name(num_name(n)))
            }
            Some(b'n') => {
                self.skip()?;
                Ok(Node::Lane(empty_lane()))
            }
            _ => Err(self.err("expected a signal")),
        }
    }

    fn lane(&mut self) -> Result<Lane, Error> {
        self.i += 1;
        let mut lane = empty_lane();
        let mut named = false;
        loop {
            self.ws();
            if self.peek() == Some(b'}') {
                self.i += 1;
                break;
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    break;
                }
            }
            let key = self.key()?;
            self.ws();
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
                    lane.period = if n == 0.0 { 1.0 } else { n };
                }
                "phase" => lane.phase = self.number()?,
                "node" => lane.node = Some(self.scalar_string()?),
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
        self.ws();
        match self.peek() {
            Some(b'"' | b'\'') => Ok(Body::Wave(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut kind = String::new();
                let mut path = None;
                let mut seen = false;
                loop {
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        break;
                    }
                    if self.peek() == Some(b',') {
                        self.i += 1;
                        self.ws();
                        if self.peek() == Some(b']') {
                            self.i += 1;
                            break;
                        }
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
        loop {
            self.ws();
            if self.peek() == Some(b'}') {
                self.i += 1;
                return Ok(d);
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(d);
                }
            }
            let key = self.key()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.i += 1;
            if key == "d" {
                self.ws();
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
        self.ws();
        match self.peek() {
            Some(b'"' | b'\'') => Ok(split_ws(&self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        return Ok(out);
                    }
                    if self.peek() == Some(b',') {
                        self.i += 1;
                        self.ws();
                        if self.peek() == Some(b']') {
                            self.i += 1;
                            return Ok(out);
                        }
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
        self.ws();
        match self.peek() {
            Some(b'"' | b'\'') => self.string(),
            Some(b't') | Some(b'f') => Ok(if self.boolean()? { "true".into() } else { "false".into() }),
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

    fn string_array(&mut self) -> Result<Vec<String>, Error> {
        self.ws();
        if self.peek() != Some(b'[') {
            return Err(self.err("expected an array"));
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(b']') {
                self.i += 1;
                return Ok(out);
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(out);
                }
            }
            self.ws();
            if matches!(self.peek(), Some(b'"' | b'\'')) {
                out.push(self.string()?);
            } else {
                self.skip()?;
            }
        }
    }

    fn cap(&mut self) -> Result<Cap, Error> {
        self.ws();
        if self.peek() != Some(b'{') {
            self.skip()?;
            return Ok(Cap::default());
        }
        self.i += 1;
        let mut cap = Cap::default();
        loop {
            self.ws();
            if self.peek() == Some(b'}') {
                self.i += 1;
                return Ok(cap);
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(cap);
                }
            }
            let key = self.key()?;
            self.ws();
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
        self.ws();
        match self.peek() {
            Some(b'"' | b'\'') => Ok(tick_from_parts(&split_ws(&self.string()?))),
            Some(b'[') => {
                self.i += 1;
                let mut parts = Vec::new();
                loop {
                    self.ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        break;
                    }
                    if self.peek() == Some(b',') {
                        self.i += 1;
                        self.ws();
                        if self.peek() == Some(b']') {
                            self.i += 1;
                            break;
                        }
                    }
                    parts.push(self.scalar_string()?);
                }
                Ok(tick_from_parts(&parts))
            }
            Some(b't') | Some(b'f') => {
                let v = if self.boolean()? { 1.0 } else { 0.0 };
                Ok(Tick::Series { offset: v, step: 1.0, dp: 0, fixed: false })
            }
            Some(b'n') => {
                self.skip()?;
                Ok(Tick::Off)
            }
            _ => {
                let n = self.number()?;
                Ok(Tick::Series { offset: n, step: 1.0, dp: 0, fixed: false })
            }
        }
    }

    fn config(&mut self, doc: &mut Doc) -> Result<(), Error> {
        self.ws();
        if self.peek() != Some(b'{') {
            self.skip()?;
            return Ok(());
        }
        self.i += 1;
        loop {
            self.ws();
            if self.peek() == Some(b'}') {
                self.i += 1;
                return Ok(());
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(());
                }
            }
            let key = self.key()?;
            self.ws();
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
                        let lo = a.floor();
                        let hi = b.ceil();
                        if lo < hi {
                            doc.xmin = 2 * lo as i64;
                            doc.xmax_cfg = 2 * hi.floor() as i64;
                        }
                    }
                }
                "marks" => {
                    self.ws();
                    if matches!(self.peek(), Some(b't' | b'f')) {
                        doc.marks = self.boolean()?;
                    } else {
                        self.skip()?;
                    }
                }
                "arcFontSize" => doc.arc_font = self.number()?,
                _ => self.skip()?,
            }
        }
    }

    fn num_pair(&mut self) -> Result<Option<(f64, f64)>, Error> {
        self.ws();
        if self.peek() != Some(b'[') {
            self.skip()?;
            return Ok(None);
        }
        self.i += 1;
        let mut nums = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(b']') {
                self.i += 1;
                break;
            }
            if self.peek() == Some(b',') {
                self.i += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    break;
                }
            }
            self.ws();
            if matches!(self.peek(), Some(b'-' | b'+' | b'.' | b'0'..=b'9')) {
                nums.push(self.number()?);
            } else {
                self.skip()?;
            }
        }
        if nums.len() >= 2 {
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
        over: None,
        under: None,
        indent: 0,
    }
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
            return Tick::Series { offset: n, step: 1.0, dp: 0, fixed: false };
        }
        return Tick::Labels(parts.to_vec());
    }
    if parts.len() == 2 {
        if let (Ok(offset), Ok(step)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
            let dp = parts[1].split('.').nth(1).map(|s| s.len()).unwrap_or(0);
            return Tick::Series { offset: step * offset, step, dp, fixed: true };
        }
    }
    Tick::Labels(parts.to_vec())
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

fn utf8_width(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b & 0b1110_0000 == 0b1100_0000 {
        2
    } else if b & 0b1111_0000 == 0b1110_0000 {
        3
    } else if b & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    }
}
