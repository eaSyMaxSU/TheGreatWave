pub(crate) fn push_i64(buf: &mut Vec<u8>, n: i64) {
    if n < 0 {
        buf.push(b'-');
    }
    push_u64(buf, n.unsigned_abs());
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

/// Append a finite coordinate with millipixel precision and no negative zero.
pub(crate) fn push_f64(buf: &mut Vec<u8>, n: f64) {
    if !n.is_finite() {
        buf.push(b'0');
        return;
    }
    let neg = n < 0.0;
    let v = if neg { -n } else { n };
    let scaled = (v * 1000.0).round();
    if scaled >= u64::MAX as f64 {
        // Keep the value rather than silently replacing large coordinates with
        // an integer sentinel. At this magnitude f64 has no fractional digits.
        buf.extend_from_slice(n.to_string().as_bytes());
        return;
    }
    let scaled = scaled as u64;
    if neg && scaled != 0 {
        buf.push(b'-');
    }
    push_u64(buf, scaled / 1000);
    let frac = (scaled % 1000) as u16;
    if frac != 0 {
        buf.push(b'.');
        buf.push(b'0' + (frac / 100) as u8);
        if !frac.is_multiple_of(100) {
            buf.push(b'0' + (frac / 10 % 10) as u8);
            if !frac.is_multiple_of(10) {
                buf.push(b'0' + (frac % 10) as u8);
            }
        }
    }
}

pub(crate) fn push_esc(buf: &mut Vec<u8>, s: &str) {
    let mut start = 0;
    for (i, c) in s.char_indices() {
        let escaped: &[u8] = match c {
            '&' => b"&amp;",
            '<' => b"&lt;",
            '>' => b"&gt;",
            '"' => b"&quot;",
            '\'' => b"&apos;",
            '\t' => b"&#9;",
            '\n' => b"&#10;",
            '\r' => b"&#13;",
            // XML 1.0 forbids these characters, even as numeric references.
            '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => b"\xef\xbf\xbd",
            _ => continue,
        };
        buf.extend_from_slice(&s.as_bytes()[start..i]);
        buf.extend_from_slice(escaped);
        start = i + c.len_utf8();
    }
    buf.extend_from_slice(&s.as_bytes()[start..]);
}

fn tag_end(src: &[u8], start: usize) -> usize {
    if src[start..].starts_with(b"<!--") {
        return src[start + 4..]
            .windows(3)
            .position(|s| s == b"-->")
            .map_or(src.len(), |i| start + 4 + i + 3);
    }
    if src[start..].starts_with(b"<![CDATA[") {
        return src[start + 9..]
            .windows(3)
            .position(|s| s == b"]]>")
            .map_or(src.len(), |i| start + 9 + i + 3);
    }
    let mut quote = None;
    for (i, &byte) in src.iter().enumerate().skip(start + 1) {
        if quote == Some(byte) {
            quote = None;
        } else if quote.is_none() {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'>' => return i + 1,
                _ => {}
            }
        }
    }
    src.len()
}

pub(crate) fn prettify(src: &[u8], ind: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + src.len() / 4);
    let mut depth = 0usize;
    let mut text_depth = None;
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'<' {
            let closing = i + 1 < src.len() && src[i + 1] == b'/';
            let special = matches!(src.get(i + 1), Some(b'!' | b'?'));
            if closing {
                depth = depth.saturating_sub(1);
            }
            if !out.is_empty() && text_depth.is_none() {
                out.push(b'\n');
                let n = depth * ind as usize;
                out.extend(std::iter::repeat_n(b' ', n));
            }
            let start = i;
            i = tag_end(src, start);
            let self_close = i >= 2 && src[i - 2] == b'/';
            out.extend_from_slice(&src[start..i]);
            if closing && text_depth == Some(depth) {
                text_depth = None;
            }
            if !closing && !self_close && !special {
                let name_start = start + 1;
                let name_end = src[name_start..i]
                    .iter()
                    .position(|c| c.is_ascii_whitespace() || matches!(c, b'>' | b'/'))
                    .map_or(i, |n| name_start + n);
                if text_depth.is_none()
                    && matches!(
                        &src[name_start..name_end],
                        b"text" | b"tspan" | b"textPath" | b"style" | b"title" | b"desc"
                    )
                {
                    text_depth = Some(depth);
                }
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

#[cfg(test)]
mod tests {
    use super::{prettify, push_esc, push_f64, push_i64};

    #[test]
    fn integer_writer_handles_the_full_signed_range() {
        for n in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX] {
            let mut out = Vec::new();
            push_i64(&mut out, n);
            assert_eq!(String::from_utf8(out).unwrap(), n.to_string());
        }
    }

    #[test]
    fn coordinates_keep_subpixel_precision_and_large_finite_values() {
        for (value, expected) in [
            (0.0, "0"),
            (-0.0001, "0"),
            (0.005, "0.005"),
            (-0.125, "-0.125"),
            (10.01, "10.01"),
            (0.9999, "1"),
            (f64::INFINITY, "0"),
            (f64::NAN, "0"),
        ] {
            let mut out = Vec::new();
            push_f64(&mut out, value);
            assert_eq!(out, expected.as_bytes());
        }
        let mut out = Vec::new();
        push_f64(&mut out, 1e100);
        assert_eq!(
            std::str::from_utf8(&out).unwrap().parse::<f64>().unwrap(),
            1e100
        );
    }

    #[test]
    fn escaping_produces_valid_xml_and_preserves_valid_unicode() {
        let mut out = Vec::new();
        push_esc(&mut out, "<>&\"'\0\u{8}\u{fffe}\u{ffff}\n\r\t 时钟 🌊");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "&lt;&gt;&amp;&quot;&apos;����&#10;&#13;&#9; 时钟 🌊"
        );
    }

    #[test]
    fn pretty_printing_preserves_svg_text_and_mixed_content() {
        let source = br#"<svg><g><text xml:space="preserve"> A <tspan>B</tspan> C </text><text>next</text></g><style>text{fill:red}</style></svg>"#;
        let pretty = String::from_utf8(prettify(source, 2)).unwrap();
        assert_eq!(pretty, "<svg>\n  <g>\n    <text xml:space=\"preserve\"> A <tspan>B</tspan> C </text>\n    <text>next</text>\n  </g>\n  <style>text{fill:red}</style>\n</svg>\n");
    }

    #[test]
    fn pretty_printing_handles_quotes_comments_and_declarations() {
        let source = br#"<?xml version="1.0"?><svg><!-- > --><path data-label="a>b"/><text><![CDATA[a>b]]></text></svg>"#;
        let pretty = String::from_utf8(prettify(source, 2)).unwrap();
        assert_eq!(pretty, "<?xml version=\"1.0\"?>\n<svg>\n  <!-- > -->\n  <path data-label=\"a>b\"/>\n  <text><![CDATA[a>b]]></text>\n</svg>\n");
    }
}
