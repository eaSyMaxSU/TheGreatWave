//! Validate SVG path data and measure its horizontal extent. Keep the original
//! path for native SVG transforms, including rotated elliptical arcs.
use crate::Error;
use std::f64::consts::{PI, TAU};

type Point = (f64, f64);
const MAX_COORDINATE: f64 = 1e9;

pub(crate) fn extent(path: &str) -> Result<f64, Error> {
    let mut p = Parser {
        bytes: path.as_bytes(),
        pos: 0,
        can_comma: false,
    };
    let mut command = b'M';
    let mut previous = b' ';
    let mut point = (0.0, 0.0);
    let mut start = point;
    let mut control = point;
    let mut xmax = 0.0_f64;
    while p.more() {
        if p.bytes[p.pos].is_ascii_alphabetic() {
            command = p.bytes[p.pos];
            p.pos += 1;
            p.can_comma = false;
        } else if previous == b' ' {
            return Err(p.err("path must begin with M"));
        } else if previous == b'Z' {
            return Err(p.err("expected a path command"));
        }
        let kind = command.to_ascii_uppercase();
        if previous == b' ' && kind != b'M' {
            return Err(p.err("path must begin with M"));
        }
        let relative = command.is_ascii_lowercase();
        let origin = if relative { point } else { (0.0, 0.0) };
        let old = point;
        match kind {
            b'M' | b'L' | b'T' => {
                point = p.point(origin)?;
                if kind == b'M' {
                    start = point;
                }
                if kind == b'T' {
                    let c = if matches!(previous, b'Q' | b'T') {
                        reflect(control, old)
                    } else {
                        old
                    };
                    xmax = xmax.max(quad_max(old.0, c.0, point.0));
                    control = c;
                }
            }
            b'H' => point.0 = p.number()? + origin.0,
            b'V' => point.1 = p.number()? + origin.1,
            b'C' | b'S' => {
                let c1 = if kind == b'C' {
                    p.point(origin)?
                } else if matches!(previous, b'C' | b'S') {
                    reflect(control, old)
                } else {
                    old
                };
                let c2 = p.point(origin)?;
                point = p.point(origin)?;
                xmax = xmax.max(cubic_max(old.0, c1.0, c2.0, point.0));
                control = c2;
            }
            b'Q' => {
                control = p.point(origin)?;
                point = p.point(origin)?;
                xmax = xmax.max(quad_max(old.0, control.0, point.0));
            }
            b'A' => {
                let rx = p.number()?;
                let ry = p.number()?;
                if rx < 0.0 || ry < 0.0 {
                    return Err(p.err("arc radii must be nonnegative"));
                }
                let angle = p.number()?;
                let large = p.flag()?;
                let sweep = p.flag()?;
                point = p.point(origin)?;
                let max = arc_max(old, point, rx, ry, angle, large, sweep)
                    .ok_or_else(|| p.err("arc geometry is out of range"))?;
                xmax = xmax.max(max);
            }
            b'Z' => point = start,
            _ => return Err(p.err("unsupported SVG path command")),
        }
        p.check_point(point)?;
        xmax = xmax.max(point.0);
        previous = kind;
        if kind == b'M' {
            command = if relative { b'l' } else { b'L' };
        }
    }
    Ok(xmax)
}

fn reflect(control: Point, point: Point) -> Point {
    (2.0 * point.0 - control.0, 2.0 * point.1 - control.1)
}

fn quad_max(a: f64, b: f64, c: f64) -> f64 {
    let mut xmax = a.max(c);
    let den = (b - a) - (c - b);
    if den != 0.0 {
        let t = (b - a) / den;
        if t > 0.0 && t < 1.0 {
            xmax = xmax.max((1.0 - t).powi(2) * a + 2.0 * (1.0 - t) * t * b + t * t * c);
        }
    }
    xmax
}

fn cubic_max(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let mut xmax = a.max(d);
    let d0 = b - a;
    let d1 = c - b;
    let d2 = d - c;
    let aa = d0 - 2.0 * d1 + d2;
    let bb = 2.0 * (d1 - d0);
    let cc = d0;
    let scale = aa.abs().max(bb.abs()).max(cc.abs());
    if scale == 0.0 {
        return xmax;
    }
    let (aa, bb, cc) = (aa / scale, bb / scale, cc / scale);
    let mut visit = |t: f64| {
        if t > 0.0 && t < 1.0 {
            let u = 1.0 - t;
            xmax =
                xmax.max(u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d);
        }
    };
    if aa == 0.0 {
        if bb != 0.0 {
            visit(-cc / bb);
        }
    } else {
        let disc = bb * bb - 4.0 * aa * cc;
        if disc >= 0.0 {
            // Avoid cancellation when the two derivative roots differ greatly.
            let q = -0.5 * (bb + disc.sqrt().copysign(bb));
            visit(q / aa);
            if q != 0.0 {
                visit(cc / q);
            }
        }
    }
    xmax
}

fn arc_max(
    a: Point,
    b: Point,
    mut rx: f64,
    mut ry: f64,
    degrees: f64,
    large: bool,
    sweep: bool,
) -> Option<f64> {
    let mut xmax = a.0.max(b.0);
    if rx == 0.0 || ry == 0.0 || a == b {
        return Some(xmax);
    }
    let phi = (degrees % 360.0).to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = (a.0 - b.0) / 2.0;
    let dy = (a.1 - b.1) / 2.0;
    let xp = cos * dx + sin * dy;
    let yp = -sin * dx + cos * dy;
    // SVG endpoint-to-center conversion, normalized to avoid squaring very
    // large ratios or dividing by their underflowed squares.
    // https://www.w3.org/TR/SVG2/implnote.html#ArcImplementationNotes
    let correction = (xp / rx).hypot(yp / ry);
    if correction > 1.0 {
        if correction.is_finite() {
            rx *= correction;
            ry *= correction;
        } else {
            // Tiny radii can require a scale larger than f64 while the corrected
            // radii themselves remain ordinary finite numbers.
            let lx = xp.abs().ln() - rx.ln();
            let ly = yp.abs().ln() - ry.ln();
            let peak = lx.max(ly);
            let log_scale =
                peak + 0.5 * ((2.0 * (lx - peak)).exp() + (2.0 * (ly - peak)).exp()).ln();
            rx = (rx.ln() + log_scale).exp();
            ry = (ry.ln() + log_scale).exp();
        }
    }
    if !rx.is_finite() || !ry.is_finite() || rx.max(ry) > 1e12 {
        return None;
    }
    let (ux, uy) = (xp / rx, yp / ry);
    let norm = ux.hypot(uy);
    if norm == 0.0 || !norm.is_finite() {
        return None;
    }
    let k = (1.0 - norm * norm).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cxp = k * rx * (uy / norm);
    let cyp = -k * ry * (ux / norm);
    let cx = cos * cxp - sin * cyp + (a.0 + b.0) / 2.0;
    let begin = ((yp - cyp) / ry).atan2((xp - cxp) / rx);
    let end = ((-yp - cyp) / ry).atan2((-xp - cxp) / rx);
    let mut delta = if sweep {
        (end - begin).rem_euclid(TAU)
    } else {
        -(begin - end).rem_euclid(TAU)
    };
    if large && delta == 0.0 {
        // The endpoints of a nearly complete ellipse may round to one angle.
        delta = TAU;
    }
    let angle = (-ry * sin).atan2(rx * cos);
    for theta in [angle, angle + PI] {
        let distance = if sweep {
            (theta - begin).rem_euclid(TAU)
        } else {
            (begin - theta).rem_euclid(TAU)
        };
        if distance <= delta.abs() + 64.0 * f64::EPSILON {
            xmax = xmax.max(cx + rx * cos * theta.cos() - ry * sin * theta.sin());
        }
    }
    Some(xmax)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    can_comma: bool,
}
impl Parser<'_> {
    fn err(&self, text: &str) -> Error {
        Error {
            offset: self.pos,
            message: format!("invalid piecewise path: {text}"),
        }
    }
    fn more(&mut self) -> bool {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        self.pos < self.bytes.len()
    }
    fn separator(&mut self) -> Result<(), Error> {
        self.more();
        if self.bytes.get(self.pos) == Some(&b',') {
            if !self.can_comma {
                return Err(self.err("unexpected comma after a path command"));
            }
            self.pos += 1;
            self.more();
        }
        Ok(())
    }
    fn number(&mut self) -> Result<f64, Error> {
        self.separator()?;
        let start = self.pos;
        if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = 0;
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
            digits += 1;
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                digits += 1;
                self.pos += 1;
            }
        }
        if digits == 0 {
            return Err(self.err("expected a number"));
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exponent = self.pos;
            while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
            }
            if self.pos == exponent {
                return Err(self.err("expected exponent digits"));
            }
        }
        let value = std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && n.abs() <= MAX_COORDINATE)
            .ok_or_else(|| self.err("coordinate is out of range"))?;
        self.can_comma = true;
        Ok(value)
    }
    fn point(&mut self, origin: Point) -> Result<Point, Error> {
        let point = (self.number()? + origin.0, self.number()? + origin.1);
        self.check_point(point)?;
        Ok(point)
    }
    fn check_point(&self, point: Point) -> Result<(), Error> {
        if !point.0.is_finite()
            || !point.1.is_finite()
            || point.0.abs().max(point.1.abs()) > MAX_COORDINATE
        {
            return Err(self.err("coordinate is out of range"));
        }
        Ok(())
    }
    fn flag(&mut self) -> Result<bool, Error> {
        self.separator()?;
        let value = match self.bytes.get(self.pos) {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(self.err("arc flag must be 0 or 1")),
        };
        self.pos += 1;
        self.can_comma = true;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_commands_and_exponents() {
        assert_eq!(extent("M0,0 L1e1,.5 h2 v-.5 l-1 0 Z").unwrap(), 12.0);
        assert_eq!(extent("m1 0 2 1").unwrap(), 3.0);
        assert_eq!(extent("M0 0 C4 0 4 1 0 1").unwrap(), 3.0);
        assert_eq!(extent("M0 0 Q4 0 0 1").unwrap(), 2.0);
        assert!(extent("M0 0 A1 1 45 1 1 1 1").unwrap() >= 1.0);
    }
    #[test]
    fn malformed_paths_terminate_with_errors() {
        for d in [
            "!",
            "0 0",
            ",0 0",
            "M,0 0",
            "M0 0 L,1 1",
            "M0 0,",
            "M0 0,,1 1",
            "M0 0,L1 1",
            "M?",
            "M0",
            "M0,0 !",
            "M0 0 L1e 0",
            "M0 0 Z 1 2",
            "M0 0 A1 1 0 2 0 1 1",
            "L1 1",
            "M0 0 L1e999 0",
        ] {
            assert!(extent(d).is_err(), "{d}");
        }
    }

    #[test]
    fn compact_numbers_flags_and_repeated_segments_follow_svg_grammar() {
        assert_eq!(extent("M.6.5L1-1 2-2").unwrap(), 2.0);
        assert_eq!(extent("M0 0 A1 1 0 011 1").unwrap(), 1.0);
        assert_eq!(extent("M0 0H1,2,3zM4 0h1").unwrap(), 5.0);
        assert_eq!(extent(" \n\t").unwrap(), 0.0);
    }

    #[test]
    fn reflected_controls_and_tiny_beziers_have_correct_extrema() {
        assert_eq!(extent("M0 0 Q-4 0 0 1 T0 2").unwrap(), 2.0);
        assert_eq!(extent("M0 0 Q-4 0 0 1 L0 1 T0 2").unwrap(), 0.0);
        assert_eq!(extent("M0 0 C-4 0 -4 1 0 1 S4 2 0 2").unwrap(), 3.0);
        let cubic = extent("M0 0 C4e-200 0 4e-200 1 0 1").unwrap();
        assert!((cubic / 3e-200 - 1.0).abs() < 1e-14);
        let quadratic = extent("M0 0 Q4e-200 0 0 1").unwrap();
        assert!((quadratic / 2e-200 - 1.0).abs() < 1e-14);
    }

    #[test]
    fn arc_extents_cover_rotation_sweep_and_radius_correction() {
        for (path, expected) in [
            ("M0 -1 A1 1 0 0 1 0 1", 1.0),
            ("M0 -1 A1 1 0 0 0 0 1", 0.0),
            ("M0 0 A1 1 0 1 1 1 1", 2.0),
            ("M0 0 A1 1 0 0 1 1 1", 1.0),
            ("M0 -1 A.1 .1 0 0 1 0 1", 1.0),
            ("M0 -1 A5e-324 5e-324 0 0 1 0 1", 1.0),
            ("M0 0 A1 1 0 1 1 1e-300 0", 1.0),
            ("M0 0 A0 1 0 1 1 1 1", 1.0),
            ("M0 0 A1 1 0 1 1 0 0", 0.0),
            ("M2.1213203435596424 2.1213203435596424 A3 1 45 0 0 -2.1213203435596424 -2.1213203435596424", 5.0_f64.sqrt()),
        ] {
            let actual = extent(path).unwrap();
            assert!((actual - expected).abs() < 1e-7, "{path}: {actual} != {expected}");
        }
    }

    #[test]
    fn accumulated_coordinates_and_extreme_arc_aspect_ratios_are_bounded() {
        for path in [
            "M1e9 0 h1",
            "M0 1e9 v1",
            "M1e9 0 l1 0",
            "M1e9 0 c1 0 0 1 0 2",
            "M0 0 A1e-300 1 0 1 1 1 1",
            "M0 0 A1e9 1e9 0 1 1 5e-324 0",
        ] {
            assert!(
                extent(path).is_err(),
                "accepted out-of-range geometry: {path}"
            );
        }
    }
}
