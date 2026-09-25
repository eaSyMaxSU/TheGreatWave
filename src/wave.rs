//! Wave strings become runs. A repeated clock or a held level is one run,
//! not a brick per half-cycle.

use crate::{Error, XLABEL, XS};

const TOO_BIG: f64 = 1.0e15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub unseen: usize,
}

pub(crate) fn compile(
    wave: &str,
    period: f64,
    hscale: i32,
    phase_bricks: f64,
) -> Result<WaveOut, Error> {
    let period = if period == 0.0 { 1.0 } else { period };
    let extra = period * hscale as f64 - 1.0;
    let bytes = wave.as_bytes();
    let mut b = Builder { pats: Vec::new(), len: 0 };
    if !bytes.is_empty() {
        let mut i = 0;
        let mut next = bytes[0];
        i += 1;
        let mut repeats = 1.0;
        while i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b'|') {
            i += 1;
            repeats += 1.0;
        }
        push_first(&mut b, next, extra, repeats)?;
        let mut sub = false;
        while i < bytes.len() {
            let top = next;
            next = bytes[i];
            i += 1;
            if next == b'<' {
                sub = true;
                if i >= bytes.len() {
                    break;
                }
                next = bytes[i];
                i += 1;
            }
            if next == b'>' {
                sub = false;
                if i >= bytes.len() {
                    break;
                }
                next = bytes[i];
                i += 1;
            }
            let mut rep = 1.0;
            while i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b'|') {
                i += 1;
                rep += 1.0;
            }
            if sub {
                push_trans(&mut b, top, next, 0.0, rep - period)?;
            } else {
                push_trans(&mut b, top, next, extra, rep)?;
            }
        }
    }
    let skip = js_iter(phase_bricks).map_err(|_| too_long())?;
    let (unseen, markers) = markers_of(&b.pats, skip);
    Ok(WaveOut {
        pats: b.pats,
        len: b.len,
        skip,
        gaps: gap_xs(bytes, period, hscale, phase_bricks),
        markers,
        unseen,
    })
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
    if !(limit > 0.0) {
        return Ok(0);
    }
    if limit > TOO_BIG {
        return Err(());
    }
    Ok(limit.ceil() as i64)
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
        b.add_clock(edge, s1, s2, s3, js_iter(extra).map_err(|_| too_long())?, js_iter(repeats).map_err(|_| too_long())?)
            .map_err(|_| too_long())?;
    } else if let Some(code) = x4(c) {
        b.add_fill(code, uniform_count(repeats, extra).map_err(|_| too_long())?)
            .map_err(|_| too_long())?;
    } else {
        b.add_fill(Code::X, uniform_count(repeats, extra).map_err(|_| too_long())?)
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
            b.add_clock(s0, s1, s2, s3, ex, t).map_err(|_| too_long())?;
        } else {
            let holds = hold_count(times, extra).map_err(|_| too_long())?;
            b.add_lead(s0, s1, holds).map_err(|_| too_long())?;
        }
        return Ok(());
    }
    if x2(next).is_none() || y1(prev).is_none() {
        b.add_fill(Code::X, uniform_count(times, extra).map_err(|_| too_long())?)
            .map_err(|_| too_long())?;
        return Ok(());
    }
    let hold = x4(next).unwrap_or(Code::X);
    b.add_lead(Code::Soft(prev, next), hold, hold_count(times, extra).map_err(|_| too_long())?)
        .map_err(|_| too_long())?;
    Ok(())
}

impl Builder {
    fn add_fill(&mut self, code: Code, count: i64) -> Result<(), ()> {
        if count <= 0 {
            return Ok(());
        }
        if let Some(Pat::Fill { code: c, count: n, .. }) = self.pats.last_mut() {
            if *c == code {
                *n = n.checked_add(count).ok_or(())?;
                self.len = self.len.checked_add(count).ok_or(())?;
                return Ok(());
            }
        }
        self.pats.push(Pat::Fill { at: self.len, code, count });
        self.len = self.len.checked_add(count).ok_or(())?;
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
        self.len = self.len.checked_add(n).ok_or(())?;
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
                self.len = self.len.checked_add(bricks).ok_or(())?;
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
        self.len = self.len.checked_add(bricks).ok_or(())?;
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

fn markers_of(pats: &[Pat], skip: i64) -> (usize, Vec<f64>) {
    let mut spans: Vec<(i64, i64)> = Vec::new();
    for p in pats {
        match *p {
            Pat::Fill { at, code: Code::Bus(_), count } => push_span(&mut spans, at, count),
            Pat::Lead { at, hold: Code::Bus(_), holds, .. } if holds > 0 => {
                push_span(&mut spans, at + 1, holds);
            }
            _ => {}
        }
    }
    let mut unseen = 0usize;
    let mut marks = Vec::new();
    for (s, len) in spans {
        let end = s + len;
        if end <= skip {
            unseen += 1;
            continue;
        }
        let (vis_start, vis_len) = if s >= skip {
            (s - skip, len)
        } else {
            (0, end - skip)
        };
        let marker = vis_start as f64 + (vis_len as f64 - 1.0) / 2.0;
        marks.push(marker * XS as f64 + XLABEL as f64);
    }
    (unseen, marks)
}

fn push_span(spans: &mut Vec<(i64, i64)>, at: i64, len: i64) {
    if len <= 0 {
        return;
    }
    if let Some((s, n)) = spans.last_mut() {
        if *s + *n == at {
            *n += len;
            return;
        }
    }
    spans.push((at, len));
}

fn gap_xs(wave: &[u8], period: f64, hscale: i32, phase: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut pos = 0.0;
    let mut sub = false;
    while i < wave.len() {
        let mut next = wave[i];
        i += 1;
        if next == b'<' {
            sub = true;
            if i >= wave.len() {
                break;
            }
            next = wave[i];
            i += 1;
        }
        if next == b'>' {
            sub = false;
            if i >= wave.len() {
                break;
            }
            next = wave[i];
            i += 1;
        }
        if sub {
            pos += 1.0;
        } else {
            pos += 2.0 * period;
        }
        if next == b'|' {
            let back = if sub { 0.0 } else { period };
            let x = XS as f64 * ((pos - back) * hscale as f64 - phase);
            out.push(x);
        }
    }
    out
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
            Pat::Lead { lead, hold, holds, .. } => {
                all.push(code_name(lead));
                let n = code_name(hold);
                for _ in 0..holds {
                    all.push(n.clone());
                }
            }
            Pat::Clock { s0, s1, s2, s3, extra, times, .. } => {
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
fn marker_index(px: f64) -> f64 {
    (px - XLABEL as f64) / XS as f64
}

#[cfg(test)]
fn check(wave: &str, period: f64, hscale: i32, phase: f64, unseen: usize, markers: &[f64], bricks: &[&str]) {
    let pb = if phase != 0.0 { phase * 2.0 } else { 0.0 };
    let w = compile(wave, period, hscale, pb).unwrap();
    let got = visible_names(&w);
    let expect: Vec<String> = bricks.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(got, expect, "bricks for {wave}");
    assert_eq!(w.unseen, unseen, "unseen for {wave}");
    let mi: Vec<f64> = w.markers.iter().copied().map(marker_index).collect();
    assert_eq!(mi.len(), markers.len(), "marker count for {wave}");
    for (a, b) in mi.iter().zip(markers) {
        assert!((a - b).abs() < 1e-9, "marker {a} != {b} for {wave}");
    }
}

#[test]
fn bricks_match_wavedrom() {
    check("p", 1.0, 1, 0.0, 0, &[], &["pclk", "nclk"]);
    check("p.....", 1.0, 1, 0.0, 0, &[], &["pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk", "nclk", "pclk", "nclk"]);
    check("1", 1.0, 1, 0.0, 0, &[], &["111", "111"]);
    check("10", 1.0, 1, 0.0, 0, &[], &["111", "111", "1m0", "000"]);
    check("2", 1.0, 1, 0.0, 0, &[0.5], &["vvv-2", "vvv-2"]);
    check("23", 1.0, 1, 0.0, 0, &[0.5, 3.0], &["vvv-2", "vvv-2", "vmv-2-3", "vvv-3"]);
    check("x.345x", 1.0, 1, 0.0, 0, &[5.0, 7.0, 9.0], &["xxx", "xxx", "xxx", "xxx", "xmv-3", "vvv-3", "vmv-3-4", "vvv-4", "vmv-4-5", "vvv-5", "vmx-5", "xxx"]);
    check("p", 1.0, 2, 0.0, 0, &[], &["pclk", "111", "nclk", "000"]);
    check("hp", 1.0, 1, 0.0, 0, &[], &["111", "111", "111", "nclk"]);
    check("pn", 1.0, 1, 0.0, 0, &[], &["pclk", "nclk", "nclk", "pclk"]);
    check("0.1", 1.0, 1, 0.0, 0, &[], &["000", "000", "000", "000", "0m1", "111"]);
    check("==", 1.0, 1, 0.0, 0, &[0.5, 3.0], &["vvv-2", "vvv-2", "vmv-2-2", "vvv-2"]);
    check("n", 1.0, 1, 0.0, 0, &[], &["nclk", "pclk"]);
    check("P", 1.0, 1, 0.0, 0, &[], &["Pclk", "nclk"]);
    check("hl", 1.0, 1, 0.0, 0, &[], &["111", "111", "nclk", "000"]);
    check("0<1", 1.0, 1, 0.0, 0, &[], &["000", "000", "0m1"]);
    check("0<1.1", 1.0, 1, 0.0, 0, &[], &["000", "000", "0m1", "111", "1m1"]);
    check("xx", 1.0, 1, 0.0, 0, &[], &["xxx", "xxx", "xmx", "xxx"]);
    check("zz", 1.0, 1, 0.0, 0, &[], &["zzz", "zzz", "zmz", "zzz"]);
    check("dd", 1.0, 1, 0.0, 0, &[], &["ddd", "ddd", "dmd", "ddd"]);
    check("2.2", 1.0, 1, 0.0, 0, &[1.5, 5.0], &["vvv-2", "vvv-2", "vvv-2", "vvv-2", "vmv-2-2", "vvv-2"]);
    check("phnlPHNL", 1.0, 1, 0.0, 0, &[], &["pclk", "nclk", "pclk", "111", "nclk", "pclk", "nclk", "000", "Pclk", "nclk", "Pclk", "111", "Nclk", "pclk", "Nclk", "000"]);
    check("hpHplnLn", 1.0, 1, 0.0, 0, &[], &["111", "111", "111", "nclk", "Pclk", "111", "111", "nclk", "000", "000", "000", "pclk", "Nclk", "000", "000", "pclk"]);
    check("nhNhplPl", 1.0, 1, 0.0, 0, &[], &["nclk", "pclk", "111", "111", "Nclk", "pclk", "111", "111", "111", "nclk", "000", "000", "Pclk", "nclk", "000", "000"]);
    check("x.34.5x", 1.0, 1, 0.0, 0, &[5.0, 8.0, 11.0], &["xxx", "xxx", "xxx", "xxx", "xmv-3", "vvv-3", "vmv-3-4", "vvv-4", "vvv-4", "vvv-4", "vmv-4-5", "vvv-5", "vmx-5", "xxx"]);
    check("0.1..0.", 1.0, 1, 0.0, 0, &[], &["000", "000", "000", "000", "0m1", "111", "111", "111", "111", "111", "1m0", "000", "000", "000"]);
    check("p<...>n", 1.0, 1, 0.0, 0, &[], &["pclk", "nclk", "xxx", "xxx", "xxx", "xxx", "nclk", "pclk"]);
    check("0<10", 1.0, 1, 0.0, 0, &[], &["000", "000", "0m1", "1m0"]);
    check("234", 1.0, 1, 1.0, 1, &[1.0, 3.0], &["vmv-2-3", "vvv-3", "vmv-3-4", "vvv-4"]);
    check("2.3.4", 1.0, 1, 0.5, 0, &[1.0, 5.0, 8.0], &["vvv-2", "vvv-2", "vvv-2", "vmv-2-3", "vvv-3", "vvv-3", "vvv-3", "vmv-3-4", "vvv-4"]);
    check("p", 2.0, 2, 0.0, 0, &[], &["pclk", "111", "111", "111", "nclk", "000", "000", "000"]);
    check("z.d.u", 1.0, 1, 0.0, 0, &[], &["zzz", "zzz", "zzz", "zzz", "zmd", "ddd", "ddd", "ddd", "dmu", "uuu"]);
    check("9", 1.0, 1, 0.0, 0, &[0.5], &["vvv-9", "vvv-9"]);
    check("19", 1.0, 1, 0.0, 0, &[3.0], &["111", "111", "1mv-9", "vvv-9"]);
    check("91", 1.0, 1, 0.0, 0, &[0.5], &["vvv-9", "vvv-9", "vm1-9", "111"]);
    check("x2", 1.0, 1, 0.0, 0, &[3.0], &["xxx", "xxx", "xmv-2", "vvv-2"]);
    check("2x", 1.0, 1, 0.0, 0, &[0.5], &["vvv-2", "vvv-2", "vmx-2", "xxx"]);
    check("d0", 1.0, 1, 0.0, 0, &[], &["ddd", "ddd", "dm0", "000"]);
    check("h.l.", 1.0, 1, 0.0, 0, &[], &["111", "111", "111", "111", "nclk", "000", "000", "000"]);
    check("H.L.", 1.0, 1, 0.0, 0, &[], &["111", "111", "111", "111", "Nclk", "000", "000", "000"]);
    check("2", 1.0, 1, 1.0, 1, &[], &[]);
    check("10", 1.0, 2, 0.0, 0, &[], &["111", "111", "111", "111", "1m0", "000", "000", "000"]);
    check("x.345x", 1.0, 1, 1.0, 0, &[3.0, 5.0, 7.0], &["xxx", "xxx", "xmv-3", "vvv-3", "vmv-3-4", "vvv-4", "vmv-4-5", "vvv-5", "vmx-5", "xxx"]);
}

#[test]
fn gap_on_bar() {
    let w = compile("p.|", 1.0, 1, 0.0).unwrap();
    assert_eq!(w.gaps, vec![100.0]);
}

#[test]
fn long_clock_is_one_run() {
    let mut wave = String::from("p");
    wave.extend(std::iter::repeat('.').take(9_999));
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
