#include "common.h"

#include <stdarg.h>

void tgw_oom(void) {
    fprintf(stderr, "tgw: out of memory\n");
    abort();
}

static locale_t tgw_loc;
static void tgw_locale_free(void) __attribute__((destructor));
static void tgw_locale_free(void) {
    if (tgw_loc) {
        freelocale(tgw_loc);
        tgw_loc = (locale_t)0;
    }
}

locale_t tgw_c_locale(void) {
    if (!tgw_loc) tgw_loc = newlocale(LC_ALL_MASK, "C", (locale_t)0);
    return tgw_loc;
}

void arena_init(Arena *a) {
    a->head = NULL;
    a->scratch_used = 0;
}

void arena_free(Arena *a) {
    Chunk *c = a->head;
    while (c) {
        Chunk *n = c->next;
        free(c);
        c = n;
    }
    a->head = NULL;
    a->scratch_used = 0;
}

void *arena_alloc(Arena *a, size_t n) {
    if (n == 0) n = 1;
    size_t u = (a->scratch_used + 7u) & ~(size_t)7u;
    if (u + n <= sizeof a->scratch) {
        void *p = a->scratch + u;
        a->scratch_used = u + n;
        return p;
    }
    if (a->head) {
        size_t hu = (a->head->used + 7u) & ~(size_t)7u;
        if (hu + n <= a->head->cap) {
            void *p = a->head->data + hu;
            a->head->used = hu + n;
            return p;
        }
    }
    size_t cap = n < 8192 ? 8192 : n;
    Chunk *c = malloc(sizeof(Chunk) + cap);
    if (!c) tgw_oom();
    c->next = a->head;
    c->used = n;
    c->cap = cap;
    a->head = c;
    return c->data;
}

char *arena_copy(Arena *a, const char *p, size_t n) {
    char *d = arena_alloc(a, n + 1);
    if (n) memcpy(d, p, n);
    d[n] = 0;
    return d;
}

Span arena_span(Arena *a, const char *p, size_t n) {
    return span_make(arena_copy(a, p, n), n);
}

void buf_init(Buf *b, uint8_t *storage, size_t cap) {
    b->data = storage;
    b->len = 0;
    b->cap = cap;
    b->heap = 0;
}

void buf_free(Buf *b) {
    if (b->heap) free(b->data);
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
    b->heap = 0;
}

void buf_clear(Buf *b) { b->len = 0; }

void buf_reserve(Buf *b, size_t extra) {
    if (TGW_LIKELY(b->len + extra <= b->cap)) return;
    size_t cap = b->cap ? b->cap : 4096;
    size_t need = b->len + extra;
    while (cap < need) {
        if (cap > (SIZE_MAX / 2)) tgw_oom();
        cap *= 2;
    }
    uint8_t *p;
    if (b->heap) {
        p = realloc(b->data, cap);
        if (!p) tgw_oom();
    } else {
        p = malloc(cap);
        if (!p) tgw_oom();
        if (b->len) memcpy(p, b->data, b->len);
        b->heap = 1;
    }
    b->data = p;
    b->cap = cap;
}

void buf_append(Buf *b, const void *p, size_t n) {
    if (!n) return;
    buf_reserve(b, n);
    memcpy(b->data + b->len, p, n);
    b->len += n;
}

void buf_push(Buf *b, uint8_t c) {
    buf_reserve(b, 1);
    b->data[b->len++] = c;
}

void buf_puts(Buf *b, const char *s) { buf_append(b, s, strlen(s)); }

static const char DIG2[] =
    "00010203040506070809101112131415161718192021222324252627282930313233343536373839"
    "40414243444546474849505152535455565758596061626364656667686970717273747576777879"
    "8081828384858687888990919293949596979899";

void push_u64(Buf *b, uint64_t n) {
    char tmp[32];
    int i = 32;
    while (n >= 100) {
        uint64_t q = n / 100;
        unsigned r = (unsigned)(n - q * 100);
        i -= 2;
        tmp[i] = DIG2[r * 2];
        tmp[i + 1] = DIG2[r * 2 + 1];
        n = q;
    }
    if (n >= 10) {
        i -= 2;
        tmp[i] = DIG2[n * 2];
        tmp[i + 1] = DIG2[n * 2 + 1];
    } else {
        tmp[--i] = (char)('0' + (unsigned)n);
    }
    buf_append(b, tmp + i, (size_t)(32 - i));
}

void push_i64(Buf *b, int64_t n) {
    uint64_t u;
    if (n < 0) {
        buf_push(b, '-');
        u = 0u - (uint64_t)n;
    } else {
        u = (uint64_t)n;
    }
    push_u64(b, u);
}

static void expand_g(const char *in, char *out, size_t cap) {
    const char *e = strchr(in, 'e');
    if (!e) e = strchr(in, 'E');
    if (!e) {
        snprintf(out, cap, "%s", in);
        return;
    }
    int exp = atoi(e + 1);
    char digs[64];
    int neg = in[0] == '-';
    const char *s = in + neg;
    int nd = 0;
    for (const char *p = s; p < e && nd < 60; p++) {
        if (*p != '.') digs[nd++] = *p;
    }
    int point = 1 + exp;
    size_t o = 0;
    if (neg && o + 1 < cap) out[o++] = '-';
    if (point <= 0) {
        if (o + 2 < cap) {
            out[o++] = '0';
            out[o++] = '.';
        }
        for (int i = 0; i < -point && o + 1 < cap; i++) out[o++] = '0';
        for (int i = 0; i < nd && o + 1 < cap; i++) out[o++] = digs[i];
    } else if (point >= nd) {
        for (int i = 0; i < nd && o + 1 < cap; i++) out[o++] = digs[i];
        for (int i = nd; i < point && o + 1 < cap; i++) out[o++] = '0';
    } else {
        for (int i = 0; i < point && o + 1 < cap; i++) out[o++] = digs[i];
        if (o + 1 < cap) out[o++] = '.';
        for (int i = point; i < nd && o + 1 < cap; i++) out[o++] = digs[i];
    }
    out[o] = 0;
}

void fmt_rust_f64(Buf *b, double v) {
    if (!isfinite(v)) {
        buf_puts(b, isnan(v) ? "NaN" : (v < 0 ? "-inf" : "inf"));
        return;
    }
    if (v == 0.0) {
        buf_puts(b, signbit(v) ? "-0" : "0");
        return;
    }
    char raw[64];
    char plain[768];
    locale_t loc = tgw_c_locale();
    for (int prec = 1; prec <= 17; prec++) {
        snprintf_l(raw, sizeof raw, loc, "%.*g", prec, v);
        char *end = NULL;
        double back = strtod_l(raw, &end, loc);
        if (end && *end == 0 && back == v) {
            expand_g(raw, plain, sizeof plain);
            buf_puts(b, plain);
            return;
        }
    }
    snprintf_l(raw, sizeof raw, loc, "%.17g", v);
    expand_g(raw, plain, sizeof plain);
    buf_puts(b, plain);
}

void fmt_fixed(Buf *b, double v, int dp) {
    if (dp < 0) dp = 0;
    if (dp > 15) dp = 15;
    char tmp[128];
    snprintf_l(tmp, sizeof tmp, tgw_c_locale(), "%.*f", dp, v);
    buf_puts(b, tmp);
}

void push_f64(Buf *b, double n) {
    if (!isfinite(n)) {
        buf_push(b, '0');
        return;
    }
    int neg = n < 0.0;
    double v = neg ? -n : n;
    double scaled = round(v * 1000.0);
    if (!(scaled < 18446744073709551616.0)) {
        fmt_rust_f64(b, n);
        return;
    }
    uint64_t s = (uint64_t)scaled;
    if (neg && s != 0) buf_push(b, '-');
    push_u64(b, s / 1000);
    unsigned frac = (unsigned)(s % 1000);
    if (frac) {
        buf_push(b, '.');
        buf_push(b, (uint8_t)('0' + frac / 100));
        if (frac % 100) {
            buf_push(b, (uint8_t)('0' + (frac / 10) % 10));
            if (frac % 10) buf_push(b, (uint8_t)('0' + frac % 10));
        }
    }
}

void push_esc(Buf *b, const char *s, size_t n) {
    const uint8_t *u = (const uint8_t *)s;
    size_t i = 0;
    while (i < n) {
        size_t start = i;
        while (i < n) {
            uint8_t c = u[i];
            if (c == '&' || c == '<' || c == '>' || c == '"' || c == '\'' || c < 0x20 || c == 0xef)
                break;
            i++;
        }
        if (i > start) buf_append(b, u + start, i - start);
        if (i >= n) break;
        uint8_t c = u[i];
        /* U+FFFE / U+FFFF are EF BF BE / EF BF BF. Other EF sequences are copied. */
        if (c == 0xef && i + 2 < n && u[i + 1] == 0xbf && (u[i + 2] == 0xbe || u[i + 2] == 0xbf)) {
            BUF_LIT(b, "\xef\xbf\xbd");
            i += 3;
            continue;
        }
        if (c == 0xef) {
            buf_push(b, c);
            i++;
            continue;
        }
        const char *esc = NULL;
        switch (c) {
        case '&':
            esc = "&amp;";
            break;
        case '<':
            esc = "&lt;";
            break;
        case '>':
            esc = "&gt;";
            break;
        case '"':
            esc = "&quot;";
            break;
        case '\'':
            esc = "&apos;";
            break;
        case '\t':
            esc = "&#9;";
            break;
        case '\n':
            esc = "&#10;";
            break;
        case '\r':
            esc = "&#13;";
            break;
        default:
            if (c < 0x20) esc = "\xef\xbf\xbd";
            break;
        }
        if (esc)
            buf_puts(b, esc);
        else
            buf_push(b, c);
        i++;
    }
}

int utf8_ok(const uint8_t *s, size_t n) {
    size_t i = 0;
    while (i < n) {
        uint8_t c = s[i];
        if (c < 0x80) {
            i++;
            continue;
        }
        size_t need = c < 0xe0 ? 2 : c < 0xf0 ? 3 : 4;
        if (c < 0xc2 || c > 0xf4 || i + need > n) return 0;
        uint32_t cp;
        if (need == 2) {
            if ((s[i + 1] & 0xc0) != 0x80) return 0;
            cp = ((uint32_t)(c & 0x1f) << 6) | (s[i + 1] & 0x3f);
        } else if (need == 3) {
            if ((s[i + 1] & 0xc0) != 0x80 || (s[i + 2] & 0xc0) != 0x80) return 0;
            cp = ((uint32_t)(c & 0x0f) << 12) | ((uint32_t)(s[i + 1] & 0x3f) << 6) | (s[i + 2] & 0x3f);
            if (cp < 0x800 || (cp >= 0xd800 && cp <= 0xdfff)) return 0;
        } else {
            if ((s[i + 1] & 0xc0) != 0x80 || (s[i + 2] & 0xc0) != 0x80 || (s[i + 3] & 0xc0) != 0x80)
                return 0;
            cp = ((uint32_t)(c & 0x07) << 18) | ((uint32_t)(s[i + 1] & 0x3f) << 12) |
                 ((uint32_t)(s[i + 2] & 0x3f) << 6) | (s[i + 3] & 0x3f);
            if (cp < 0x10000 || cp > 0x10ffff) return 0;
        }
        i += need;
    }
    return 1;
}

int utf8_next(const char *s, size_t n, size_t *i, uint32_t *cp) {
    if (*i >= n) return 0;
    const uint8_t *u = (const uint8_t *)s;
    uint8_t c = u[*i];
    if (c < 0x80) {
        *cp = c;
        *i += 1;
        return 1;
    }
    size_t left = n - *i;
    if (c < 0xe0) {
        if (c < 0xc2 || left < 2 || (u[*i + 1] & 0xc0) != 0x80) goto bad;
        *cp = ((uint32_t)(c & 0x1f) << 6) | (u[*i + 1] & 0x3f);
        *i += 2;
        return 1;
    }
    if (c < 0xf0) {
        if (left < 3 || (u[*i + 1] & 0xc0) != 0x80 || (u[*i + 2] & 0xc0) != 0x80) goto bad;
        uint32_t v = ((uint32_t)(c & 0x0f) << 12) | ((uint32_t)(u[*i + 1] & 0x3f) << 6) | (u[*i + 2] & 0x3f);
        if (v < 0x800 || (v >= 0xd800 && v <= 0xdfff)) goto bad;
        *cp = v;
        *i += 3;
        return 1;
    }
    if (c <= 0xf4 && left >= 4 && (u[*i + 1] & 0xc0) == 0x80 && (u[*i + 2] & 0xc0) == 0x80 &&
        (u[*i + 3] & 0xc0) == 0x80) {
        uint32_t v = ((uint32_t)(c & 0x07) << 18) | ((uint32_t)(u[*i + 1] & 0x3f) << 12) |
                     ((uint32_t)(u[*i + 2] & 0x3f) << 6) | (u[*i + 3] & 0x3f);
        if (v >= 0x10000 && v <= 0x10ffff) {
            *cp = v;
            *i += 4;
            return 1;
        }
    }
bad:
    *cp = 0xfffd;
    *i += 1;
    return 1;
}

size_t utf8_encode(uint32_t cp, char out[4]) {
    if (cp < 0x80) {
        out[0] = (char)cp;
        return 1;
    }
    if (cp < 0x800) {
        out[0] = (char)(0xc0 | (cp >> 6));
        out[1] = (char)(0x80 | (cp & 0x3f));
        return 2;
    }
    if (cp < 0x10000) {
        out[0] = (char)(0xe0 | (cp >> 12));
        out[1] = (char)(0x80 | ((cp >> 6) & 0x3f));
        out[2] = (char)(0x80 | (cp & 0x3f));
        return 3;
    }
    out[0] = (char)(0xf0 | (cp >> 18));
    out[1] = (char)(0x80 | ((cp >> 12) & 0x3f));
    out[2] = (char)(0x80 | ((cp >> 6) & 0x3f));
    out[3] = (char)(0x80 | (cp & 0x3f));
    return 4;
}

size_t utf8_count(const char *s, size_t n) {
    size_t i = 0, k = 0;
    uint32_t cp;
    while (utf8_next(s, n, &i, &cp)) k++;
    return k;
}

int byte_is_ascii_ws(uint8_t c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

int cp_is_ws(uint32_t cp) {
    if (cp <= 0x20) return cp == 0x20 || cp == 0x09 || cp == 0x0a || cp == 0x0b || cp == 0x0c || cp == 0x0d;
    if (cp == 0x85 || cp == 0xa0 || cp == 0x1680) return 1;
    if (cp >= 0x2000 && cp <= 0x200a) return 1;
    return cp == 0x2028 || cp == 0x2029 || cp == 0x202f || cp == 0x205f || cp == 0x3000;
}

int parse_f64_slice(const char *s, size_t n, double *out) {
    if (!n || n > 4096) return 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (!((c >= '0' && c <= '9') || c == '+' || c == '-' || c == '.' || c == 'e' || c == 'E')) return 0;
    }
    char stack[128];
    char *buf = stack;
    char *heap = NULL;
    if (n >= sizeof stack) {
        heap = malloc(n + 1);
        if (!heap) tgw_oom();
        buf = heap;
    }
    memcpy(buf, s, n);
    buf[n] = 0;
    char *end = NULL;
    double v = strtod_l(buf, &end, tgw_c_locale());
    int ok = end == buf + n && isfinite(v);
    free(heap);
    if (!ok) return 0;
    *out = v;
    return 1;
}

void spanvec_push(Arena *a, SpanVec *v, Span s) {
    if (v->len == v->cap) {
        uint32_t cap = v->cap ? v->cap * 2 : 4;
        Span *p = arena_alloc(a, sizeof(Span) * cap);
        if (v->len) memcpy(p, v->data, sizeof(Span) * v->len);
        v->data = p;
        v->cap = cap;
    }
    v->data[v->len++] = s;
}

void doc_init(Doc *d) {
    memset(d, 0, sizeof *d);
    d->lanes = d->small_lanes;
    d->lane_cap = 8;
    d->groups = d->small_groups;
    d->group_cap = 8;
    d->hscale = 1;
    d->xmax_cfg = 1000000000000LL;
    d->marks = 1;
    d->arc_font = 11.0;
}

void doc_add_lane(Arena *a, Doc *d, Lane lane) {
    if (d->lane_n == d->lane_cap) {
        uint32_t cap = d->lane_cap * 2;
        Lane *p = arena_alloc(a, sizeof(Lane) * cap);
        memcpy(p, d->lanes, sizeof(Lane) * d->lane_n);
        d->lanes = p;
        d->lane_cap = cap;
        d->lanes_heap = 1;
    }
    d->lanes[d->lane_n++] = lane;
}

void doc_add_group(Arena *a, Doc *d, Group g) {
    if (d->group_n == d->group_cap) {
        uint32_t cap = d->group_cap * 2;
        Group *p = arena_alloc(a, sizeof(Group) * cap);
        memcpy(p, d->groups, sizeof(Group) * d->group_n);
        d->groups = p;
        d->group_cap = cap;
        d->groups_heap = 1;
    }
    d->groups[d->group_n++] = g;
}

void doc_add_edge(Arena *a, Doc *d, Span e) {
    if (d->edge_n == d->edge_cap) {
        uint32_t cap = d->edge_cap ? d->edge_cap * 2 : 4;
        Span *p = arena_alloc(a, sizeof(Span) * cap);
        if (d->edge_n) memcpy(p, d->edges, sizeof(Span) * d->edge_n);
        d->edges = p;
        d->edge_cap = cap;
    }
    d->edges[d->edge_n++] = e;
}

Lane lane_blank(Span name, int64_t indent) {
    Lane lane;
    memset(&lane, 0, sizeof lane);
    lane.name = name;
    lane.period = 1.0;
    lane.indent = indent;
    return lane;
}

void err_set(TgwError *e, size_t off, const char *msg) {
    if (!e) return;
    e->offset = off;
    snprintf(e->message, sizeof e->message, "%s", msg);
}

void err_fmt(TgwError *e, size_t off, const char *fmt, ...) {
    if (!e) return;
    e->offset = off;
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(e->message, sizeof e->message, fmt, ap);
    va_end(ap);
}
