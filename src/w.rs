pub(crate) fn push_i64(buf: &mut Vec<u8>, mut n: i64) {
    if n < 0 {
        buf.push(b'-');
        n = n.saturating_neg();
    }
    push_u64(buf, n as u64);
}

pub(crate) fn push_u64(buf: &mut Vec<u8>, mut n: u64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    loop {
        i -= 1;
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    buf.extend_from_slice(&tmp[i..]);
}

/// Append a finite number with at most two decimal places.
pub(crate) fn push_f64(buf: &mut Vec<u8>, n: f64) {
    if !n.is_finite() {
        buf.push(b'0');
        return;
    }
    let neg = n < 0.0;
    let v = if neg { -n } else { n };
    let scaled = (v * 100.0).round();
    if scaled > i64::MAX as f64 {
        push_i64(buf, if neg { i64::MIN } else { i64::MAX });
        return;
    }
    let mut scaled = scaled as i64;
    if neg && scaled != 0 {
        buf.push(b'-');
    }
    if scaled < 0 {
        scaled = -scaled;
    }
    push_i64(buf, scaled / 100);
    let frac = (scaled % 100) as u8;
    if frac != 0 {
        buf.push(b'.');
        buf.push(b'0' + frac / 10);
        if frac % 10 != 0 {
            buf.push(b'0' + frac % 10);
        }
    }
}

pub(crate) fn push_esc(buf: &mut Vec<u8>, s: &str) {
    for c in s.chars() {
        match c {
            '&' => buf.extend_from_slice(b"&amp;"),
            '<' => buf.extend_from_slice(b"&lt;"),
            '>' => buf.extend_from_slice(b"&gt;"),
            '"' => buf.extend_from_slice(b"&quot;"),
            _ => {
                let mut tmp = [0u8; 4];
                buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
            }
        }
    }
}

pub(crate) fn prettify(src: &[u8], ind: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + src.len() / 4);
    let mut depth = 0i32;
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'<' {
            let closing = i + 1 < src.len() && src[i + 1] == b'/';
            if closing {
                depth -= 1;
            }
            if !out.is_empty() {
                out.push(b'\n');
                let n = depth.max(0) as usize * ind as usize;
                out.extend(std::iter::repeat(b' ').take(n));
            }
            let start = i;
            while i < src.len() && src[i] != b'>' {
                i += 1;
            }
            if i < src.len() {
                i += 1;
            }
            let self_close = i >= 2 && src[i - 2] == b'/';
            out.extend_from_slice(&src[start..i]);
            if !closing && !self_close {
                depth += 1;
            }
        } else {
            let start = i;
            while i < src.len() && src[i] != b'<' {
                i += 1;
            }
            out.extend_from_slice(&src[start..i]);
        }
    }
    out.push(b'\n');
    out
}
