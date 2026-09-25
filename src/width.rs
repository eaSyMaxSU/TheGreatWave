//! Conservative advance estimates for the SVG's system sans-serif font stack.
//! These are layout metrics, not embedded fonts; the browser still shapes text.

pub(crate) fn text_width(s: &str, size: f64) -> f64 {
    let size = if size.is_finite() && size > 0.0 {
        size
    } else {
        12.0
    };
    let mut width = 0_u64;
    let mut cluster = 0_u16;
    let mut emoji_cluster = false;
    let mut join_emoji = false;
    let mut regional = false;
    for c in s.chars() {
        let code = c as u32;
        if code == 0x200d {
            join_emoji = emoji_cluster;
            continue;
        }
        // Emoji presentation and keycaps can widen the preceding base glyph.
        if matches!(code, 0xfe0f | 0x20e3) && cluster != 0 {
            width += 1100_u16.saturating_sub(cluster) as u64;
            cluster = cluster.max(1100);
            emoji_cluster = true;
            continue;
        }
        if zero_width(code) || (0x1f3fb..=0x1f3ff).contains(&code) {
            continue;
        }
        let next_regional = (0x1f1e6..=0x1f1ff).contains(&code);
        let emoji = is_emoji(code);
        let advance = advance(c);
        // ZWJ sequences and flag pairs occupy one glyph cluster. Ordinary
        // joining scripts still retain their per-letter width estimate.
        let joined = (join_emoji && emoji) || (regional && next_regional);
        if joined {
            width += advance.saturating_sub(cluster) as u64;
            cluster = cluster.max(advance);
        } else {
            width += advance as u64;
            cluster = advance;
        }
        emoji_cluster = emoji || (joined && emoji_cluster);
        join_emoji = false;
        regional = next_regional && !regional;
    }
    // Room for font weight, fallback differences, and glyph overhangs.
    width as f64 * size * 1.08 / 1000.0
}

fn advance(c: char) -> u16 {
    let code = c as u32;
    match code {
        0x9 | 0xa | 0xd => 278,
        0x2000 | 0x2002 => 500,
        0x2001 | 0x2003 | 0x3000 => 1000,
        0x2004 => 333,
        0x2005 => 250,
        0x2006 => 167,
        0x2007 => 640,
        0x2008 => 278,
        0x2009 | 0x202f => 200,
        0x200a => 100,
        0x30..=0x39 => 640,
        _ if (code as usize) < WIDTHS.len() => WIDTHS[code as usize],
        _ if is_emoji(code) => 1100,
        _ => 1000,
    }
}

fn is_emoji(code: u32) -> bool {
    matches!(code, 0x2300..=0x23ff | 0x2600..=0x27bf | 0x1f000..=0x1faff)
}

fn zero_width(code: u32) -> bool {
    if code < 0xad {
        return false;
    }
    let i = ZERO_WIDTH.partition_point(|&(_, end)| end < code);
    ZERO_WIDTH.get(i).is_some_and(|&(start, _)| code >= start)
}

// Thousandths of an em. Upper estimates from common system sans-serif fonts
// avoid the old table's approximately 4/3 inflation at every font size.
const WIDTHS: &[u16] = &[
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 260, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 278, 278, 408, 646, 604, 889, 732, 254, 333, 333, 551, 604, 278, 429, 278, 372, 606, 572,
    572, 592, 604, 585, 617, 572, 600, 617, 278, 278, 604, 604, 604, 556, 1015, 667, 667, 722, 730,
    667, 611, 778, 741, 339, 500, 667, 556, 907, 760, 781, 667, 781, 722, 667, 611, 731, 667, 944,
    667, 667, 617, 329, 372, 329, 604, 556, 500, 561, 615, 501, 615, 564, 344, 615, 618, 258, 258,
    534, 258, 935, 618, 605, 615, 615, 413, 500, 361, 618, 508, 786, 529, 510, 500, 380, 551, 380,
    604, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 278, 333, 604, 604, 604, 604, 551, 566, 580, 847, 370, 619, 604, 333, 832, 552, 428,
    604, 350, 350, 500, 623, 655, 333, 500, 350, 376, 619, 834, 834, 834, 611, 667, 667, 667, 667,
    667, 667, 1000, 722, 667, 667, 667, 667, 339, 339, 339, 339, 730, 760, 781, 781, 781, 781, 781,
    604, 781, 731, 731, 731, 731, 667, 667, 631, 561, 561, 561, 561, 561, 561, 889, 501, 564, 564,
    564, 564, 278, 278, 278, 278, 605, 618, 605, 605, 605, 605, 605, 604, 611, 618, 618, 618, 618,
    510, 615, 510, 667, 561, 667, 561, 667, 561, 722, 501, 722, 501, 722, 501, 722, 501, 730, 645,
    730, 617, 667, 564, 667, 564, 667, 564, 667, 564, 667, 564, 778, 615, 778, 615, 778, 615, 778,
    615, 741, 618, 744, 618, 339, 278, 339, 278, 339, 278, 339, 258, 339, 278, 735, 516, 500, 258,
    667, 534, 534, 556, 258, 556, 258, 556, 295, 556, 334, 556, 258, 760, 618, 760, 618, 760, 618,
    688, 760, 618, 781, 605, 781, 605, 781, 605, 1082, 946, 722, 413, 722, 413, 722, 413, 667, 500,
    667, 500, 667, 500, 667, 500, 611, 361, 611, 375, 611, 361, 731, 618, 731, 618, 731, 618, 731,
    618, 731, 618, 731, 618, 944, 786, 667, 510, 667, 617, 500, 617, 500, 617, 500, 326, 615, 758,
    656, 615, 656, 612, 722, 722, 503, 730, 810, 656, 615, 604, 667, 752, 604, 611, 556, 778, 624,
    934, 348, 339, 667, 534, 258, 542, 1030, 760, 618, 782, 857, 656, 1075, 845, 754, 615, 667,
    667, 500, 618, 380, 361, 611, 361, 611, 854, 674, 782, 731, 772, 547, 611, 500, 611, 611, 545,
    545, 570, 584, 490, 487, 587, 336, 555, 584, 278, 1333, 1222, 1085, 1062, 833, 516, 1222, 1018,
    876, 667, 561, 339, 258, 781, 605, 731, 618, 731, 618, 731, 618, 731, 618, 731, 618, 564, 667,
    561, 667, 561, 1000, 889, 778, 615, 778, 615, 667, 534, 781, 605, 781, 605, 611, 545, 258,
    1333, 1222, 1085, 778, 615, 1034, 661, 760, 618, 667, 561, 1000, 889, 781, 611, 667, 561, 667,
    561, 667, 564, 667, 564, 339, 278, 339, 278, 781, 605, 781, 605, 722, 413, 722, 413, 731, 618,
    731, 618, 667, 500, 611, 361, 575, 496, 741, 618, 741, 863, 671, 596, 611, 500, 667, 561, 667,
    564, 781, 605, 781, 605, 781, 605, 781, 605, 667, 510, 381, 745, 388, 258, 972, 972, 667, 722,
    501, 556, 611, 500, 500, 581, 471, 667, 731, 668, 667, 564, 500, 258, 767, 615, 722, 413, 667,
    510,
];

// Nonspacing/enclosing marks and formatting controls, including variation
// selectors and supplementary combining marks. Sorted for binary search.
const ZERO_WIDTH: &[(u32, u32)] = &[
    (0xad, 0xad),
    (0x300, 0x36f),
    (0x483, 0x489),
    (0x591, 0x5bd),
    (0x5bf, 0x5bf),
    (0x5c1, 0x5c2),
    (0x5c4, 0x5c5),
    (0x5c7, 0x5c7),
    (0x600, 0x605),
    (0x610, 0x61a),
    (0x61c, 0x61c),
    (0x64b, 0x65f),
    (0x670, 0x670),
    (0x6d6, 0x6dd),
    (0x6df, 0x6e4),
    (0x6e7, 0x6e8),
    (0x6ea, 0x6ed),
    (0x70f, 0x70f),
    (0x711, 0x711),
    (0x730, 0x74a),
    (0x7a6, 0x7b0),
    (0x7eb, 0x7f3),
    (0x7fd, 0x7fd),
    (0x816, 0x819),
    (0x81b, 0x823),
    (0x825, 0x827),
    (0x829, 0x82d),
    (0x859, 0x85b),
    (0x8d3, 0x902),
    (0x93a, 0x93a),
    (0x93c, 0x93c),
    (0x941, 0x948),
    (0x94d, 0x94d),
    (0x951, 0x957),
    (0x962, 0x963),
    (0x981, 0x981),
    (0x9bc, 0x9bc),
    (0x9c1, 0x9c4),
    (0x9cd, 0x9cd),
    (0x9e2, 0x9e3),
    (0x9fe, 0x9fe),
    (0xa01, 0xa02),
    (0xa3c, 0xa3c),
    (0xa41, 0xa42),
    (0xa47, 0xa48),
    (0xa4b, 0xa4d),
    (0xa51, 0xa51),
    (0xa70, 0xa71),
    (0xa75, 0xa75),
    (0xa81, 0xa82),
    (0xabc, 0xabc),
    (0xac1, 0xac5),
    (0xac7, 0xac8),
    (0xacd, 0xacd),
    (0xae2, 0xae3),
    (0xafa, 0xaff),
    (0xb01, 0xb01),
    (0xb3c, 0xb3c),
    (0xb3f, 0xb3f),
    (0xb41, 0xb44),
    (0xb4d, 0xb4d),
    (0xb55, 0xb56),
    (0xb62, 0xb63),
    (0xb82, 0xb82),
    (0xbc0, 0xbc0),
    (0xbcd, 0xbcd),
    (0xc00, 0xc00),
    (0xc04, 0xc04),
    (0xc3e, 0xc40),
    (0xc46, 0xc48),
    (0xc4a, 0xc4d),
    (0xc55, 0xc56),
    (0xc62, 0xc63),
    (0xc81, 0xc81),
    (0xcbc, 0xcbc),
    (0xcbf, 0xcbf),
    (0xcc6, 0xcc6),
    (0xccc, 0xccd),
    (0xce2, 0xce3),
    (0xd00, 0xd01),
    (0xd3b, 0xd3c),
    (0xd41, 0xd44),
    (0xd4d, 0xd4d),
    (0xd62, 0xd63),
    (0xd81, 0xd81),
    (0xdca, 0xdca),
    (0xdd2, 0xdd4),
    (0xdd6, 0xdd6),
    (0xe31, 0xe31),
    (0xe34, 0xe3a),
    (0xe47, 0xe4e),
    (0xeb1, 0xeb1),
    (0xeb4, 0xebc),
    (0xec8, 0xecd),
    (0xf18, 0xf19),
    (0xf35, 0xf35),
    (0xf37, 0xf37),
    (0xf39, 0xf39),
    (0xf71, 0xf7e),
    (0xf80, 0xf84),
    (0xf86, 0xf87),
    (0xf8d, 0xf97),
    (0xf99, 0xfbc),
    (0xfc6, 0xfc6),
    (0x102d, 0x1030),
    (0x1032, 0x1037),
    (0x1039, 0x103a),
    (0x103d, 0x103e),
    (0x1058, 0x1059),
    (0x105e, 0x1060),
    (0x1071, 0x1074),
    (0x1082, 0x1082),
    (0x1085, 0x1086),
    (0x108d, 0x108d),
    (0x109d, 0x109d),
    (0x135d, 0x135f),
    (0x1712, 0x1714),
    (0x1732, 0x1734),
    (0x1752, 0x1753),
    (0x1772, 0x1773),
    (0x17b4, 0x17b5),
    (0x17b7, 0x17bd),
    (0x17c6, 0x17c6),
    (0x17c9, 0x17d3),
    (0x17dd, 0x17dd),
    (0x180b, 0x180e),
    (0x1885, 0x1886),
    (0x18a9, 0x18a9),
    (0x1920, 0x1922),
    (0x1927, 0x1928),
    (0x1932, 0x1932),
    (0x1939, 0x193b),
    (0x1a17, 0x1a18),
    (0x1a1b, 0x1a1b),
    (0x1a56, 0x1a56),
    (0x1a58, 0x1a5e),
    (0x1a60, 0x1a60),
    (0x1a62, 0x1a62),
    (0x1a65, 0x1a6c),
    (0x1a73, 0x1a7c),
    (0x1a7f, 0x1a7f),
    (0x1ab0, 0x1ac0),
    (0x1b00, 0x1b03),
    (0x1b34, 0x1b34),
    (0x1b36, 0x1b3a),
    (0x1b3c, 0x1b3c),
    (0x1b42, 0x1b42),
    (0x1b6b, 0x1b73),
    (0x1b80, 0x1b81),
    (0x1ba2, 0x1ba5),
    (0x1ba8, 0x1ba9),
    (0x1bab, 0x1bad),
    (0x1be6, 0x1be6),
    (0x1be8, 0x1be9),
    (0x1bed, 0x1bed),
    (0x1bef, 0x1bf1),
    (0x1c2c, 0x1c33),
    (0x1c36, 0x1c37),
    (0x1cd0, 0x1cd2),
    (0x1cd4, 0x1ce0),
    (0x1ce2, 0x1ce8),
    (0x1ced, 0x1ced),
    (0x1cf4, 0x1cf4),
    (0x1cf8, 0x1cf9),
    (0x1dc0, 0x1df9),
    (0x1dfb, 0x1dff),
    (0x200b, 0x200f),
    (0x202a, 0x202e),
    (0x2060, 0x2064),
    (0x2066, 0x206f),
    (0x20d0, 0x20f0),
    (0x2cef, 0x2cf1),
    (0x2d7f, 0x2d7f),
    (0x2de0, 0x2dff),
    (0x302a, 0x302d),
    (0x3099, 0x309a),
    (0xa66f, 0xa672),
    (0xa674, 0xa67d),
    (0xa69e, 0xa69f),
    (0xa6f0, 0xa6f1),
    (0xa802, 0xa802),
    (0xa806, 0xa806),
    (0xa80b, 0xa80b),
    (0xa825, 0xa826),
    (0xa82c, 0xa82c),
    (0xa8c4, 0xa8c5),
    (0xa8e0, 0xa8f1),
    (0xa8ff, 0xa8ff),
    (0xa926, 0xa92d),
    (0xa947, 0xa951),
    (0xa980, 0xa982),
    (0xa9b3, 0xa9b3),
    (0xa9b6, 0xa9b9),
    (0xa9bc, 0xa9bd),
    (0xa9e5, 0xa9e5),
    (0xaa29, 0xaa2e),
    (0xaa31, 0xaa32),
    (0xaa35, 0xaa36),
    (0xaa43, 0xaa43),
    (0xaa4c, 0xaa4c),
    (0xaa7c, 0xaa7c),
    (0xaab0, 0xaab0),
    (0xaab2, 0xaab4),
    (0xaab7, 0xaab8),
    (0xaabe, 0xaabf),
    (0xaac1, 0xaac1),
    (0xaaec, 0xaaed),
    (0xaaf6, 0xaaf6),
    (0xabe5, 0xabe5),
    (0xabe8, 0xabe8),
    (0xabed, 0xabed),
    (0xfb1e, 0xfb1e),
    (0xfe00, 0xfe0f),
    (0xfe20, 0xfe2f),
    (0xfeff, 0xfeff),
    (0xfff9, 0xfffb),
    (0x101fd, 0x101fd),
    (0x102e0, 0x102e0),
    (0x10376, 0x1037a),
    (0x10a01, 0x10a03),
    (0x10a05, 0x10a06),
    (0x10a0c, 0x10a0f),
    (0x10a38, 0x10a3a),
    (0x10a3f, 0x10a3f),
    (0x10ae5, 0x10ae6),
    (0x10d24, 0x10d27),
    (0x10eab, 0x10eac),
    (0x10f46, 0x10f50),
    (0x11001, 0x11001),
    (0x11038, 0x11046),
    (0x1107f, 0x11081),
    (0x110b3, 0x110b6),
    (0x110b9, 0x110ba),
    (0x110bd, 0x110bd),
    (0x110cd, 0x110cd),
    (0x11100, 0x11102),
    (0x11127, 0x1112b),
    (0x1112d, 0x11134),
    (0x11173, 0x11173),
    (0x11180, 0x11181),
    (0x111b6, 0x111be),
    (0x111c9, 0x111cc),
    (0x111cf, 0x111cf),
    (0x1122f, 0x11231),
    (0x11234, 0x11234),
    (0x11236, 0x11237),
    (0x1123e, 0x1123e),
    (0x112df, 0x112df),
    (0x112e3, 0x112ea),
    (0x11300, 0x11301),
    (0x1133b, 0x1133c),
    (0x11340, 0x11340),
    (0x11366, 0x1136c),
    (0x11370, 0x11374),
    (0x11438, 0x1143f),
    (0x11442, 0x11444),
    (0x11446, 0x11446),
    (0x1145e, 0x1145e),
    (0x114b3, 0x114b8),
    (0x114ba, 0x114ba),
    (0x114bf, 0x114c0),
    (0x114c2, 0x114c3),
    (0x115b2, 0x115b5),
    (0x115bc, 0x115bd),
    (0x115bf, 0x115c0),
    (0x115dc, 0x115dd),
    (0x11633, 0x1163a),
    (0x1163d, 0x1163d),
    (0x1163f, 0x11640),
    (0x116ab, 0x116ab),
    (0x116ad, 0x116ad),
    (0x116b0, 0x116b5),
    (0x116b7, 0x116b7),
    (0x1171d, 0x1171f),
    (0x11722, 0x11725),
    (0x11727, 0x1172b),
    (0x1182f, 0x11837),
    (0x11839, 0x1183a),
    (0x1193b, 0x1193c),
    (0x1193e, 0x1193e),
    (0x11943, 0x11943),
    (0x119d4, 0x119d7),
    (0x119da, 0x119db),
    (0x119e0, 0x119e0),
    (0x11a01, 0x11a0a),
    (0x11a33, 0x11a38),
    (0x11a3b, 0x11a3e),
    (0x11a47, 0x11a47),
    (0x11a51, 0x11a56),
    (0x11a59, 0x11a5b),
    (0x11a8a, 0x11a96),
    (0x11a98, 0x11a99),
    (0x11c30, 0x11c36),
    (0x11c38, 0x11c3d),
    (0x11c3f, 0x11c3f),
    (0x11c92, 0x11ca7),
    (0x11caa, 0x11cb0),
    (0x11cb2, 0x11cb3),
    (0x11cb5, 0x11cb6),
    (0x11d31, 0x11d36),
    (0x11d3a, 0x11d3a),
    (0x11d3c, 0x11d3d),
    (0x11d3f, 0x11d45),
    (0x11d47, 0x11d47),
    (0x11d90, 0x11d91),
    (0x11d95, 0x11d95),
    (0x11d97, 0x11d97),
    (0x11ef3, 0x11ef4),
    (0x13430, 0x13438),
    (0x16af0, 0x16af4),
    (0x16b30, 0x16b36),
    (0x16f4f, 0x16f4f),
    (0x16f8f, 0x16f92),
    (0x16fe4, 0x16fe4),
    (0x1bc9d, 0x1bc9e),
    (0x1bca0, 0x1bca3),
    (0x1d167, 0x1d169),
    (0x1d173, 0x1d182),
    (0x1d185, 0x1d18b),
    (0x1d1aa, 0x1d1ad),
    (0x1d242, 0x1d244),
    (0x1da00, 0x1da36),
    (0x1da3b, 0x1da6c),
    (0x1da75, 0x1da75),
    (0x1da84, 0x1da84),
    (0x1da9b, 0x1da9f),
    (0x1daa1, 0x1daaf),
    (0x1e000, 0x1e006),
    (0x1e008, 0x1e018),
    (0x1e01b, 0x1e021),
    (0x1e023, 0x1e024),
    (0x1e026, 0x1e02a),
    (0x1e130, 0x1e136),
    (0x1e2ec, 0x1e2ef),
    (0x1e8d0, 0x1e8d6),
    (0x1e944, 0x1e94a),
    (0xe0001, 0xe0001),
    (0xe0020, 0xe007f),
    (0xe0100, 0xe01ef),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_metrics_are_proportional_and_scale_with_font_size() {
        let clock = text_width("clock", 12.0);
        assert!((27.0..34.0).contains(&clock), "{clock}");
        assert!(text_width("WWWW", 12.0) > 3.0 * text_width("iiii", 12.0));
        assert_eq!(text_width("clock", 24.0), clock * 2.0);
        assert_eq!(text_width("", 12.0), 0.0);
    }

    #[test]
    fn combining_marks_do_not_add_phantom_characters() {
        assert_eq!(text_width("é", 12.0), text_width("e\u{301}", 12.0));
        assert_eq!(text_width("a", 12.0), text_width("a\u{301}\u{323}", 12.0));
        assert_eq!(text_width("ab", 12.0), text_width("a\u{200b}b", 12.0));
        assert!(text_width("时钟", 12.0) >= 24.0);
    }

    #[test]
    fn emoji_sequences_use_one_cluster_width() {
        assert_eq!(text_width("👩‍💻", 12.0), text_width("👩", 12.0));
        assert_eq!(text_width("👩🏽", 12.0), text_width("👩", 12.0));
        assert_eq!(text_width("🇨🇳", 12.0), text_width("👩", 12.0));
        assert_eq!(text_width("🇨🇳🇺🇸", 12.0), 2.0 * text_width("👩", 12.0));
        assert!(text_width("1️⃣", 12.0) > text_width("1", 12.0));
    }

    #[test]
    fn invalid_sizes_use_a_finite_default() {
        for size in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert_eq!(text_width("clock", size), text_width("clock", 12.0));
        }
    }
}
