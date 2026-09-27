#include "common.h"
#include "width_tables.h"

static int zero_width(uint32_t code) {
    if (code < 0xad) return 0;
    size_t lo = 0, hi = TGW_ZERO_WIDTH_LEN;
    while (lo < hi) {
        size_t mid = lo + (hi - lo) / 2;
        if (TGW_ZERO_WIDTH[mid][1] < code)
            lo = mid + 1;
        else
            hi = mid;
    }
    if (lo >= TGW_ZERO_WIDTH_LEN) return 0;
    return code >= TGW_ZERO_WIDTH[lo][0];
}

static int is_emoji(uint32_t code) {
    return (code >= 0x2300 && code <= 0x23ff) || (code >= 0x2600 && code <= 0x27bf) ||
           (code >= 0x1f000 && code <= 0x1faff);
}

static uint16_t advance_cp(uint32_t code) {
    switch (code) {
    case 0x9:
    case 0xa:
    case 0xd:
        return 278;
    case 0x2000:
    case 0x2002:
        return 500;
    case 0x2001:
    case 0x2003:
    case 0x3000:
        return 1000;
    case 0x2004:
        return 333;
    case 0x2005:
        return 250;
    case 0x2006:
        return 167;
    case 0x2007:
        return 640;
    case 0x2008:
        return 278;
    case 0x2009:
    case 0x202f:
        return 200;
    case 0x200a:
        return 100;
    default:
        break;
    }
    if (code >= 0x30 && code <= 0x39) return 640;
    if (code < TGW_WIDTHS_LEN) return TGW_WIDTHS[code];
    if (is_emoji(code)) return 1100;
    return 1000;
}

double tgw_text_width(const char *s, size_t n, double size) {
    if (!(isfinite(size) && size > 0.0)) size = 12.0;
    const uint8_t *u = (const uint8_t *)s;
    int ascii = 1;
    for (size_t i = 0; i < n; i++) {
        if (u[i] >= 0x80) {
            ascii = 0;
            break;
        }
    }
    uint64_t width = 0;
    if (ascii) {
        for (size_t i = 0; i < n; i++) width += advance_cp(u[i]);
        return (double)width * size * 1.08 / 1000.0;
    }
    uint16_t cluster = 0;
    int emoji_cluster = 0;
    int join_emoji = 0;
    int regional = 0;
    size_t i = 0;
    uint32_t code;
    while (utf8_next(s, n, &i, &code)) {
        if (code == 0x200d) {
            join_emoji = emoji_cluster;
            continue;
        }
        if ((code == 0xfe0f || code == 0x20e3) && cluster != 0) {
            uint16_t add = cluster < 1100 ? (uint16_t)(1100 - cluster) : 0;
            width += add;
            if (cluster < 1100) cluster = 1100;
            emoji_cluster = 1;
            continue;
        }
        if (zero_width(code) || (code >= 0x1f3fb && code <= 0x1f3ff)) continue;
        int next_regional = code >= 0x1f1e6 && code <= 0x1f1ff;
        int emoji = is_emoji(code);
        uint16_t adv = advance_cp(code);
        int joined = (join_emoji && emoji) || (regional && next_regional);
        if (joined) {
            uint16_t add = adv > cluster ? (uint16_t)(adv - cluster) : 0;
            width += add;
            if (adv > cluster) cluster = adv;
        } else {
            width += adv;
            cluster = adv;
        }
        emoji_cluster = emoji || (joined && emoji_cluster);
        join_emoji = 0;
        regional = next_regional && !regional;
    }
    return (double)width * size * 1.08 / 1000.0;
}
