//! Wave strings become runs. A repeated clock or a held level is one run,
//! not a brick per half-cycle.

use crate::{Error, XLABEL, XS};

const TOO_BIG: f64 = 1.0e15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Code {
    Rise,
    Fall,
    RiseA,
    FallA,
    Low,
    High,
    Mid,
    DashL,
    DashH,
    X,
    Bus(u8),
    Soft(u8, u8),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Pat {
    Fill {
        at: i64,
        code: Code,
        count: i64,
    },
    Lead {
        at: i64,
        lead: Code,
        hold: Code,
        holds: i64,
    },
    Clock {
        at: i64,
        s0: Code,
        s1: Code,
        s2: Code,
        s3: Code,
        extra: i64,
        times: i64,
    },
}

pub(crate) struct WaveOut {
    pub pats: Vec<Pat>,
    pub len: i64,
    pub skip: i64,
    pub gaps: Vec<f64>,
    pub markers: Vec<f64>,
    pub marker_widths: Vec<f64>,
    pub unseen: usize,
}

#[cfg(test)]
pub(crate) fn compile(
    wave: &str,
    period: f64,
    hscale: i32,
    phase_bricks: f64,
) -> Result<WaveOut, Error> {
    compile_window(wave, period, hscale, phase_bricks, TOO_BIG * 4.0)
}

pub(crate) fn compile_window(
    wave: &str,
    period: f64,
    hscale: i32,
    phase_bricks: f64,
    width_bricks: f64,
) -> Result<WaveOut, Error> {
    let scale = timing_scale(period, hscale)?;
    if !phase_bricks.is_finite()
        || phase_bricks.abs() > TOO_BIG
        || !width_bricks.is_finite()
        || !(0.0..=TOO_BIG * 4.0).contains(&width_bricks)
    {
        return Err(too_long());
    }
    let mut b = Builder {
        pats: Vec::new(),
        len: 0,
    };
    let mut gaps = Vec::new();
    let mut prev = None;
    for segment in Segments::new(wave) {
        let extra = segment.extra(scale);
        let start = b.len;
        let repeats = segment.symbols.len() as f64;
        match prev {
            Some(previous) if !segment.continued => {
                push_trans(&mut b, previous, segment.code, extra, repeats)?
            }
            _ => push_first(&mut b, segment.code, extra, repeats)?,
        }
        for (i, symbol) in segment.symbols.iter().enumerate() {
            if *symbol == b'|' {
                let lo = segment.prefix_bricks(i, scale)?;
                let hi = segment.prefix_bricks(i + 1, scale)?;
                let center = start as f64 + (lo as f64 + hi as f64) / 2.0;
                let x = (center - phase_bricks) * XS as f64;
                if x >= 0.0 && x <= width_bricks * XS as f64 {
                    gaps.push(x);
                }
            }
        }
        prev = Some(segment.code);
    }
    // Keep the partially visible first brick; the writer translates and clips it.
    let skip = phase_bricks.max(0.0).floor() as i64;
    let (unseen, markers, marker_widths) = markers_of(&b.pats, skip, phase_bricks, width_bricks);
    Ok(WaveOut {
        pats: b.pats,
        len: b.len,
        skip,
        gaps,
        markers,
        marker_widths,
        unseen,
    })
}

fn timing_scale(period: f64, hscale: i32) -> Result<f64, Error> {
    let period = if period == 0.0 { 1.0 } else { period };
    let scale = period * hscale as f64;
    if !period.is_finite() || period < 0.0 || hscale <= 0 || !scale.is_finite() || scale > TOO_BIG {
        return Err(too_long());
    }
    Ok(scale)
}

/// Actual symbol origins, without phase/crop, for event annotations. Only callers
/// with nodes need this allocation; a long unannotated clock stays one run.
pub(crate) fn node_positions(wave: &str, period: f64, hscale: i32) -> Result<Vec<f64>, Error> {
    let scale = timing_scale(period, hscale)?;
    let mut positions = Vec::new();
    let mut start = 0i64;
    for segment in Segments::new(wave) {
        for i in 0..segment.symbols.len() {
            let pos = start
                .checked_add(segment.prefix_bricks(i, scale)?)
                .ok_or_else(too_long)?;
            positions.push(pos as f64 * XS as f64);
        }
        start = bounded_add(start, segment.prefix_bricks(segment.symbols.len(), scale)?)
            .map_err(|_| too_long())?;
    }
    Ok(positions)
}

struct Segment<'a> {
    code: u8,
    continued: bool,
    sub: bool,
    symbols: &'a [u8],
}

impl Segment<'_> {
    fn extra(&self, scale: f64) -> f64 {
        scale * if self.sub { 0.5 } else { 1.0 } - 1.0
    }

    fn prefix_bricks(&self, count: usize, scale: f64) -> Result<i64, Error> {
        if count == 0 {
            return Ok(0);
        }
        let extra = self.extra(scale);
        if x5(self.code).is_some() {
            let half = 1 + js_iter(extra).map_err(|_| too_long())?;
            (count as i64)
                .checked_mul(2)
                .and_then(|n| n.checked_mul(half))
                .ok_or_else(too_long)
        } else {
            uniform_count(count as f64, extra).map_err(|_| too_long())
        }
    }
}

struct Segments<'a> {
    remaining: &'a [u8],
    prev: u8,
    sub: bool,
}

impl<'a> Segments<'a> {
    fn new(wave: &'a str) -> Self {
        Self {
            remaining: wave.as_bytes(),
            prev: b'x',
            sub: false,
        }
    }
}

impl<'a> Iterator for Segments<'a> {
    type Item = Segment<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // Controls have no duration, including at the beginning and end of a lane.
        while let Some(&control @ (b'<' | b'>')) = self.remaining.first() {
            self.sub = control == b'<';
            self.remaining = &self.remaining[1..];
        }
        let &first = self.remaining.first()?;
        let continued = matches!(first, b'.' | b'|');
        let code = if continued { self.prev } else { first };
        let mut len = 1;
        while self
            .remaining
            .get(len)
            .is_some_and(|c| matches!(c, b'.' | b'|'))
        {
            len += 1;
        }
        let symbols = &self.remaining[..len];
        self.remaining = &self.remaining[len..];
        self.prev = code;
        Some(Segment {
            code,
            continued,
            sub: self.sub,
            symbols,
        })
    }
}

pub(crate) fn visible_bricks(w: &WaveOut) -> i64 {
    (w.len - w.skip).max(0)
}

struct Builder {
    pats: Vec<Pat>,
    len: i64,
}

fn too_long() -> Error {
    Error {
        offset: 0,
        message: "wave is too long".to_string(),
    }
}

fn js_iter(limit: f64) -> Result<i64, ()> {
    if !limit.is_finite() {
        return Err(());
    }
    if limit <= 0.0 {
        return Ok(0);
    }
    if limit > TOO_BIG {
        return Err(());
    }
    Ok(limit.ceil() as i64)
}

fn bounded_add(a: i64, b: i64) -> Result<i64, ()> {
    a.checked_add(b).filter(|n| *n <= TOO_BIG as i64).ok_or(())
}

fn uniform_count(times: f64, extra: f64) -> Result<i64, ()> {
    let n = times * 2.0 * (extra + 1.0);
    Ok(1 + js_iter(n - 1.0)?)
}

fn hold_count(times: f64, extra: f64) -> Result<i64, ()> {
    js_iter(times * 2.0 * (extra + 1.0) - 1.0)
}

fn push_first(b: &mut Builder, c: u8, extra: f64, repeats: f64) -> Result<(), Error> {
    if x5(c).is_some() {
        let edge = x1(c).ok_or_else(too_long)?;
        let s1 = x4(c).unwrap_or(Code::X);
        let s2 = x5(c).ok_or_else(too_long)?;
        let s3 = x6(c);
        b.add_clock(
            edge,
            s1,
            s2,
            s3,
            js_iter(extra).map_err(|_| too_long())?,
            js_iter(repeats).map_err(|_| too_long())?,
        )
        .map_err(|_| too_long())?;
    } else if let Some(code) = x4(c) {
        b.add_fill(code, uniform_count(repeats, extra).map_err(|_| too_long())?)
            .map_err(|_| too_long())?;
    } else {
        b.add_fill(
            Code::X,
            uniform_count(repeats, extra).map_err(|_| too_long())?,
        )
        .map_err(|_| too_long())?;
    }
    Ok(())
}

fn push_trans(b: &mut Builder, prev: u8, next: u8, extra: f64, times: f64) -> Result<(), Error> {
    if let Some(edge) = x1(next) {
        let s0 = xclude(prev, next).unwrap_or(edge);
        let s1 = x4(next).unwrap_or(Code::X);
        if let Some(s2) = x5(next) {
            let s3 = x6(next);
            let t = js_iter(times).map_err(|_| too_long())?;
            let ex = js_iter(extra).map_err(|_| too_long())?;
            // A gated entry only suppresses the first edge, never subsequent
            // repetitions of the clock.
            if s0 != edge && t > 0 {
                b.add_clock(s0, s1, s2, s3, ex, 1).map_err(|_| too_long())?;
                b.add_clock(edge, s1, s2, s3, ex, t - 1)
                    .map_err(|_| too_long())?;
            } else {
                b.add_clock(s0, s1, s2, s3, ex, t).map_err(|_| too_long())?;
            }
        } else {
            let holds = hold_count(times, extra).map_err(|_| too_long())?;
            b.add_lead(s0, s1, holds).map_err(|_| too_long())?;
        }
        return Ok(());
    }
    if x2(next).is_none() || y1(prev).is_none() {
        b.add_fill(
            Code::X,
            uniform_count(times, extra).map_err(|_| too_long())?,
        )
        .map_err(|_| too_long())?;
        return Ok(());
    }
    let hold = x4(next).unwrap_or(Code::X);
    b.add_lead(
        Code::Soft(prev, next),
        hold,
        hold_count(times, extra).map_err(|_| too_long())?,
    )
    .map_err(|_| too_long())?;
    Ok(())
}

impl Builder {
    fn add_fill(&mut self, code: Code, count: i64) -> Result<(), ()> {
        if count <= 0 {
            return Ok(());
        }
        if let Some(Pat::Fill {
            code: c, count: n, ..
        }) = self.pats.last_mut()
        {
            if *c == code {
                *n = n.checked_add(count).ok_or(())?;
                self.len = bounded_add(self.len, count)?;
                return Ok(());
            }
        }
        self.pats.push(Pat::Fill {
            at: self.len,
            code,
            count,
        });
        self.len = bounded_add(self.len, count)?;
        Ok(())
    }

    fn add_lead(&mut self, lead: Code, hold: Code, holds: i64) -> Result<(), ()> {
        let holds = holds.max(0);
        if is_flat(lead) && lead == hold {
            return self.add_fill(hold, 1 + holds);
        }
        let n = 1 + holds;
        self.pats.push(Pat::Lead {
            at: self.len,
            lead,
            hold,
            holds,
        });
        self.len = bounded_add(self.len, n)?;
        Ok(())
    }

    fn add_clock(
        &mut self,
        s0: Code,
        s1: Code,
        s2: Code,
        s3: Code,
        extra: i64,
        times: i64,
    ) -> Result<(), ()> {
        if times <= 0 {
            return Ok(());
        }
        let bricks = times
            .checked_mul(2)
            .and_then(|n| n.checked_mul(extra + 1))
            .ok_or(())?;
        if let Some(Pat::Clock {
            s0: a,
            s1: b,
            s2: c,
            s3: d,
            extra: e,
            times: t,
            ..
        }) = self.pats.last_mut()
        {
            if *a == s0 && *b == s1 && *c == s2 && *d == s3 && *e == extra {
                *t = t.checked_add(times).ok_or(())?;
                self.len = bounded_add(self.len, bricks)?;
                return Ok(());
            }
        }
        self.pats.push(Pat::Clock {
            at: self.len,
            s0,
            s1,
            s2,
            s3,
            extra,
            times,
        });
        self.len = bounded_add(self.len, bricks)?;
        Ok(())
    }
}

fn is_flat(c: Code) -> bool {
    matches!(
        c,
        Code::Low | Code::High | Code::Mid | Code::DashL | Code::DashH | Code::X | Code::Bus(_)
    )
}

fn x1(c: u8) -> Option<Code> {
    Some(match c {
        b'p' | b'h' => Code::Rise,
        b'n' | b'l' => Code::Fall,
        b'P' | b'H' => Code::RiseA,
        b'N' | b'L' => Code::FallA,
        _ => return None,
    })
}

fn x5(c: u8) -> Option<Code> {
    Some(match c {
        b'p' | b'P' => Code::Fall,
        b'n' | b'N' => Code::Rise,
        _ => return None,
    })
}

fn x6(c: u8) -> Code {
    match c {
        b'n' | b'N' => Code::High,
        _ => Code::Low,
    }
}

fn x4(c: u8) -> Option<Code> {
    Some(match c {
        b'p' | b'P' | b'h' | b'H' | b'1' => Code::High,
        b'n' | b'N' | b'l' | b'L' | b'0' => Code::Low,
        b'x' => Code::X,
        b'd' => Code::DashL,
        b'u' => Code::DashH,
        b'z' => Code::Mid,
        b'=' | b'2' => Code::Bus(2),
        b'3'..=b'9' => Code::Bus(c - b'0'),
        _ => return None,
    })
}

fn xclude(prev: u8, next: u8) -> Option<Code> {
    match (prev, next) {
        (b'h', b'p') | (b'H', b'p') | (b'n', b'h') | (b'N', b'h') => Some(Code::High),
        (b'l', b'n') | (b'L', b'n') | (b'p', b'l') | (b'P', b'l') => Some(Code::Low),
        _ => None,
    }
}

fn y1(c: u8) -> Option<&'static str> {
    Some(match c {
        b'p' | b'P' | b'l' | b'L' | b'0' => "0",
        b'n' | b'N' | b'h' | b'H' | b'1' => "1",
        b'x' => "x",
        b'd' => "d",
        b'u' => "u",
        b'z' => "z",
        b'=' | b'2'..=b'9' => "v",
        _ => return None,
    })
}

fn x2(c: u8) -> Option<&'static str> {
    Some(match c {
        b'0' => "0",
        b'1' => "1",
        b'x' => "x",
        b'd' => "d",
        b'u' => "u",
        b'z' => "z",
        b'=' | b'2'..=b'9' => "v",
        _ => return None,
    })
}

#[cfg(test)]
fn suf(c: u8) -> &'static str {
    match c {
        b'=' | b'2' => "-2",
        b'3' => "-3",
        b'4' => "-4",
        b'5' => "-5",
        b'6' => "-6",
        b'7' => "-7",
        b'8' => "-8",
        b'9' => "-9",
        _ => "",
    }
}

#[cfg(test)]
fn code_name(c: Code) -> String {
    match c {
        Code::Rise => "pclk".to_string(),
        Code::Fall => "nclk".to_string(),
        Code::RiseA => "Pclk".to_string(),
        Code::FallA => "Nclk".to_string(),
        Code::Low => "000".to_string(),
        Code::High => "111".to_string(),
        Code::Mid => "zzz".to_string(),
        Code::DashL => "ddd".to_string(),
        Code::DashH => "uuu".to_string(),
        Code::X => "xxx".to_string(),
        Code::Bus(n) => format!("vvv-{n}"),
        Code::Soft(p, n) => {
            let a = y1(p).unwrap_or("x");
            let b = x2(n).unwrap_or("x");
            format!("{a}m{b}{}{}", suf(p), suf(n))
        }
    }
}

fn markers_of(pats: &[Pat], skip: i64, phase: f64, width: f64) -> (usize, Vec<f64>, Vec<f64>) {
    let mut unseen = 0usize;
    let mut marks = Vec::new();
    let mut widths = Vec::new();
    let left = phase * XS as f64;
    let right = left + width * XS as f64;
    let mut finish = |(start, end): (i64, i64)| {
        if end as f64 <= left {
            unseen += 1;
            return;
        }
        let vis_start = (start as f64).max(left);
        let vis_end = (end as f64).min(right);
        if vis_start >= vis_end {
            return;
        }
        marks.push((vis_start + vis_end) / 2.0 - (skip * XS) as f64);
        widths.push(vis_end - vis_start);
    };
    let mut pending: Option<(i64, i64)> = None;
    for p in pats {
        let (at, end, bus, lead) = match *p {
            Pat::Fill { at, code, count } => (at, at + count, matches!(code, Code::Bus(_)), None),
            Pat::Lead {
                at,
                lead,
                hold,
                holds,
            } => (at, at + 1 + holds, matches!(hold, Code::Bus(_)), Some(lead)),
            Pat::Clock {
                at, extra, times, ..
            } => (at, at + 2 * (extra + 1) * times, false, None),
        };
        let at = at * XS;
        let end = end * XS;
        if bus && lead.is_none() {
            if let Some((_, previous_end)) = pending.as_mut() {
                if *previous_end == at {
                    *previous_end = end;
                    continue;
                }
            }
        }
        if let Some((start, mut previous_end)) = pending.take() {
            // A bus continues into the outgoing transition up to its crossing.
            if matches!(lead, Some(Code::Soft(prev, _)) if matches!(x4(prev), Some(Code::Bus(_)))) {
                previous_end = at + XLABEL;
            }
            finish((start, previous_end));
        }
        if bus {
            // The first value has a square left edge; later values start at
            // the incoming transition crossing, including one-brick buses.
            pending = Some((at + if lead.is_some() { XLABEL } else { 0 }, end));
        }
    }
    if let Some(span) = pending {
        finish(span);
    }
    (unseen, marks, widths)
}

#[cfg(test)]
fn visible_names(w: &WaveOut) -> Vec<String> {
    let mut all = Vec::new();
    for p in &w.pats {
        match *p {
            Pat::Fill { code, count, .. } => {
                let n = code_name(code);
                for _ in 0..count {
                    all.push(n.clone());
                }
            }
            Pat::Lead {
                lead, hold, holds, ..
            } => {
                all.push(code_name(lead));
                let n = code_name(hold);
                for _ in 0..holds {
                    all.push(n.clone());
                }
            }
            Pat::Clock {
                s0,
                s1,
                s2,
                s3,
                extra,
                times,
                ..
            } => {
                for _ in 0..times {
                    all.push(code_name(s0));
                    for _ in 0..extra {
                        all.push(code_name(s1));
                    }
                    all.push(code_name(s2));
                    for _ in 0..extra {
                        all.push(code_name(s3));
                    }
                }
            }
        }
    }
    let skip = w.skip.max(0) as usize;
    if skip >= all.len() {
        Vec::new()
    } else {
        all.split_off(skip)
    }
}

#[cfg(test)]
fn check(wave: &str, period: f64, hscale: i32, phase: f64, bricks: &[&str]) {
    let w = compile(wave, period, hscale, phase * 2.0).unwrap();
    let got = visible_names(&w);
    let expect: Vec<String> = bricks.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(got, expect, "bricks for {wave}");
}

#[test]
fn bricks_match_wavedrom() {
    check("p", 1.0, 1, 0.0, &["pclk", "nclk"]);
    check(
        "p.....",
        1.0,
        1,
        0.0,
        &[
            "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk",
            "nclk",
        ],
    );
    check("1", 1.0, 1, 0.0, &["111", "111"]);
    check("10", 1.0, 1, 0.0, &["111", "111", "1m0", "000"]);
    check("2", 1.0, 1, 0.0, &["vvv-2", "vvv-2"]);
    check("23", 1.0, 1, 0.0, &["vvv-2", "vvv-2", "vmv-2-3", "vvv-3"]);
    check(
        "x.345x",
        1.0,
        1,
        0.0,
        &[
            "xxx", "xxx", "xxx", "xxx", "xmv-3", "vvv-3", "vmv-3-4", "vvv-4", "vmv-4-5", "vvv-5",
            "vmx-5", "xxx",
        ],
    );
    check("p", 1.0, 2, 0.0, &["pclk", "111", "nclk", "000"]);
    check("hp", 1.0, 1, 0.0, &["111", "111", "111", "nclk"]);
    check("pn", 1.0, 1, 0.0, &["pclk", "nclk", "nclk", "pclk"]);
    check(
        "0.1",
        1.0,
        1,
        0.0,
        &["000", "000", "000", "000", "0m1", "111"],
    );
    check("==", 1.0, 1, 0.0, &["vvv-2", "vvv-2", "vmv-2-2", "vvv-2"]);
    check("n", 1.0, 1, 0.0, &["nclk", "pclk"]);
    check("P", 1.0, 1, 0.0, &["Pclk", "nclk"]);
    check("hl", 1.0, 1, 0.0, &["111", "111", "nclk", "000"]);
    check("0<1", 1.0, 1, 0.0, &["000", "000", "0m1"]);
    check("0<1.1", 1.0, 1, 0.0, &["000", "000", "0m1", "111", "1m1"]);
    check("xx", 1.0, 1, 0.0, &["xxx", "xxx", "xmx", "xxx"]);
    check("zz", 1.0, 1, 0.0, &["zzz", "zzz", "zmz", "zzz"]);
    check("dd", 1.0, 1, 0.0, &["ddd", "ddd", "dmd", "ddd"]);
    check(
        "2.2",
        1.0,
        1,
        0.0,
        &["vvv-2", "vvv-2", "vvv-2", "vvv-2", "vmv-2-2", "vvv-2"],
    );
    check(
        "phnlPHNL",
        1.0,
        1,
        0.0,
        &[
            "pclk", "nclk", "pclk", "111", "nclk", "pclk", "nclk", "000", "Pclk", "nclk", "Pclk",
            "111", "Nclk", "pclk", "Nclk", "000",
        ],
    );
    check(
        "hpHplnLn",
        1.0,
        1,
        0.0,
        &[
            "111", "111", "111", "nclk", "Pclk", "111", "111", "nclk", "000", "000", "000", "pclk",
            "Nclk", "000", "000", "pclk",
        ],
    );
    check(
        "nhNhplPl",
        1.0,
        1,
        0.0,
        &[
            "nclk", "pclk", "111", "111", "Nclk", "pclk", "111", "111", "111", "nclk", "000",
            "000", "Pclk", "nclk", "000", "000",
        ],
    );
    check(
        "x.34.5x",
        1.0,
        1,
        0.0,
        &[
            "xxx", "xxx", "xxx", "xxx", "xmv-3", "vvv-3", "vmv-3-4", "vvv-4", "vvv-4", "vvv-4",
            "vmv-4-5", "vvv-5", "vmx-5", "xxx",
        ],
    );
    check(
        "0.1..0.",
        1.0,
        1,
        0.0,
        &[
            "000", "000", "000", "000", "0m1", "111", "111", "111", "111", "111", "1m0", "000",
            "000", "000",
        ],
    );
    check("0<10", 1.0, 1, 0.0, &["000", "000", "0m1", "1m0"]);
    check(
        "234",
        1.0,
        1,
        1.0,
        &["vmv-2-3", "vvv-3", "vmv-3-4", "vvv-4"],
    );
    check(
        "2.3.4",
        1.0,
        1,
        0.5,
        &[
            "vvv-2", "vvv-2", "vvv-2", "vmv-2-3", "vvv-3", "vvv-3", "vvv-3", "vmv-3-4", "vvv-4",
        ],
    );
    check(
        "p",
        2.0,
        2,
        0.0,
        &["pclk", "111", "111", "111", "nclk", "000", "000", "000"],
    );
    check(
        "z.d.u",
        1.0,
        1,
        0.0,
        &[
            "zzz", "zzz", "zzz", "zzz", "zmd", "ddd", "ddd", "ddd", "dmu", "uuu",
        ],
    );
    check("9", 1.0, 1, 0.0, &["vvv-9", "vvv-9"]);
    check("19", 1.0, 1, 0.0, &["111", "111", "1mv-9", "vvv-9"]);
    check("91", 1.0, 1, 0.0, &["vvv-9", "vvv-9", "vm1-9", "111"]);
    check("x2", 1.0, 1, 0.0, &["xxx", "xxx", "xmv-2", "vvv-2"]);
    check("2x", 1.0, 1, 0.0, &["vvv-2", "vvv-2", "vmx-2", "xxx"]);
    check("d0", 1.0, 1, 0.0, &["ddd", "ddd", "dm0", "000"]);
    check(
        "h.l.",
        1.0,
        1,
        0.0,
        &["111", "111", "111", "111", "nclk", "000", "000", "000"],
    );
    check(
        "H.L.",
        1.0,
        1,
        0.0,
        &["111", "111", "111", "111", "Nclk", "000", "000", "000"],
    );
    check("2", 1.0, 1, 1.0, &[]);
    check(
        "10",
        1.0,
        2,
        0.0,
        &["111", "111", "111", "111", "1m0", "000", "000", "000"],
    );
    check(
        "x.345x",
        1.0,
        1,
        1.0,
        &[
            "xxx", "xxx", "xmv-3", "vvv-3", "vmv-3-4", "vvv-4", "vmv-4-5", "vvv-5", "vmx-5", "xxx",
        ],
    );
}

#[test]
fn gap_on_bar() {
    let w = compile("p.|", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.gaps, vec![100.0]);
}

#[test]
fn long_clock_is_one_run() {
    let mut wave = String::from("p");
    wave.extend(std::iter::repeat_n('.', 9_999));
    let w = compile(&wave, 1.0, 1, 0.0).unwrap();
    assert_eq!(w.pats.len(), 1);
    assert_eq!(w.len, 20_000);
    match w.pats[0] {
        Pat::Clock { times, extra, .. } => {
            assert_eq!(times, 10_000);
            assert_eq!(extra, 0);
        }
        _ => panic!("expected a clock run"),
    }
}

#[test]
fn gated_clock_only_suppresses_the_entry_edge() {
    check(
        "hp..",
        1.0,
        1,
        0.0,
        &["111", "111", "111", "nclk", "pclk", "nclk", "pclk", "nclk"],
    );
    check(
        "ln..",
        1.0,
        1,
        0.0,
        &["000", "000", "000", "pclk", "nclk", "pclk", "nclk", "pclk"],
    );
    let wave = format!("hp{}", ".".repeat(9_998));
    let w = compile(&wave, 1.0, 1, 0.0).unwrap();
    assert_eq!(
        w.pats.len(),
        3,
        "one held level, one gated cycle, one repeated clock"
    );
}

#[test]
fn fractional_phase_keeps_the_visible_part_of_a_brick() {
    let w = compile("01", 1.0, 1, 0.5).unwrap();
    assert_eq!(w.skip, 0);
    assert_eq!(visible_names(&w), ["000", "000", "0m1", "111"]);
    let w = compile("01", 1.0, 1, 1.5).unwrap();
    assert_eq!(w.skip, 1);
    assert_eq!(visible_names(&w), ["000", "0m1", "111"]);
    let w = compile("01", 1.0, 1, -1.5).unwrap();
    assert_eq!(w.skip, 0);
}

#[test]
fn subcycles_are_half_duration_and_controls_take_no_time() {
    check(
        "<0.1..>0",
        1.0,
        1,
        0.0,
        &["000", "000", "0m1", "111", "111", "1m0", "000"],
    );
    let w = compile("0<1..>0", 1.0, 2, 0.0).unwrap();
    assert_eq!(w.len, 14);
    let w = compile("0<1..>0", 2.0, 1, 0.0).unwrap();
    assert_eq!(w.len, 14);
    assert_eq!(compile("<><>", 1.0, 1, 0.0).unwrap().len, 0);
    assert_eq!(compile("0<>", 1.0, 1, 0.0).unwrap().len, 2);
}

#[test]
fn repeats_across_controls_continue_the_previous_state() {
    check("0<..>.", 1.0, 1, 0.0, &["000"; 6]);
    check(
        "p<...>n",
        1.0,
        1,
        0.0,
        &[
            "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "nclk", "pclk",
        ],
    );
}

#[test]
fn annotations_follow_quantized_and_subcycle_symbol_positions() {
    let w = compile("p.|", 1.25, 1, 0.5).unwrap();
    assert_eq!(w.len, 12);
    assert_eq!(w.gaps, [190.0]);
    assert_eq!(node_positions("p.|", 1.25, 1).unwrap(), [0.0, 80.0, 160.0]);
    let w = compile("0<1|>0|", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.gaps, [70.0, 140.0]);
    assert_eq!(
        node_positions("0<1|>0|", 1.0, 1).unwrap(),
        [0.0, 40.0, 60.0, 80.0, 120.0]
    );
    assert_eq!(node_positions("0.1", 1.25, 1).unwrap(), [0.0, 60.0, 100.0]);
}

#[test]
fn short_bus_values_each_retain_their_data_slot() {
    let w = compile("<234", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.len, 3);
    assert_eq!(w.markers, [13.0, 36.0, 53.0]);
    assert_eq!(w.marker_widths, [26.0, 20.0, 14.0]);
    let w = compile("<234", 1.0, 1, 1.5).unwrap();
    assert_eq!(w.unseen, 1);
    assert_eq!(w.markers, [18.0, 33.0]);
    let w = compile("2<..>.", 1.0, 1, 0.0).unwrap();
    assert_eq!(
        w.markers.len(),
        1,
        "holding a value across controls keeps one label"
    );
}

#[test]
fn bus_labels_are_centered_between_drawn_transition_crossings() {
    let w = compile("2", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.markers, [20.0]);
    assert_eq!(w.marker_widths, [40.0]);
    let w = compile("234", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.markers, [23.0, 66.0, 103.0]);
    assert_eq!(w.marker_widths, [46.0, 40.0, 34.0]);
    let w = compile("x2x", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.markers, [66.0]);
    assert_eq!(w.marker_widths, [40.0]);
}

#[test]
fn labels_fit_visible_bus_spans_and_keep_data_indices_when_cropped() {
    let w = compile_window("234", 1.0, 1, 2.5, 2.0).unwrap();
    assert_eq!(w.unseen, 1);
    assert_eq!(w.markers, [28.0, 48.0]);
    assert_eq!(w.marker_widths, [36.0, 4.0]);
    let w = compile_window("2....", 1.0, 1, 2.5, 2.0).unwrap();
    assert_eq!(w.unseen, 0);
    assert_eq!(w.markers, [30.0]);
    assert_eq!(w.marker_widths, [40.0]);
    let w = compile("2", 1.0, 1, -2.5).unwrap();
    assert_eq!(w.markers, [20.0]);
    assert_eq!(w.marker_widths, [40.0]);
    let w = compile_window("2", 1.0, 1, 0.0, 0.0).unwrap();
    assert!(w.markers.is_empty());
}

#[test]
fn timing_rejects_nonfinite_and_overflowing_dimensions() {
    for period in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 1e20] {
        assert!(compile("p", period, 1, 0.0).is_err());
    }
    for phase in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e20, -1e20] {
        assert!(compile("p", 1.0, 1, phase).is_err());
    }
    assert!(compile("p", 1.0, 0, 0.0).is_err());
    assert!(compile("p", 1.0, -1, 0.0).is_err());
    assert!(compile("p...........", 1e14, 1, 0.0).is_err());
    assert!(compile("010101010101", 1e14, 1, 0.0).is_err());
    assert!(compile("p", 0.0, 1, 0.0).is_ok());
}
