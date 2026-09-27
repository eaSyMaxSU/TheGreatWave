#include "common.h"

static void too_long(TgwError *e) { err_set(e, 0, "wave is too long"); }

void wave_init(WaveOut *w) {
    memset(w, 0, sizeof *w);
    w->pats = w->small;
    w->pat_cap = 4;
}

int64_t wave_visible(const WaveOut *w) {
    int64_t n = w->len - w->skip;
    return n > 0 ? n : 0;
}

static int js_iter(double limit, int64_t *out) {
    if (!isfinite(limit)) return -1;
    if (limit <= 0.0) {
        *out = 0;
        return 0;
    }
    if (limit > TGW_TOO_BIG) return -1;
    *out = (int64_t)ceil(limit);
    return 0;
}

static int bounded_add(int64_t a, int64_t b, int64_t *o) {
    if (__builtin_add_overflow(a, b, o)) return -1;
    if (*o > (int64_t)TGW_TOO_BIG) return -1;
    return 0;
}

static int uniform_count(double times, double extra, int64_t *o) {
    int64_t it;
    if (js_iter(times * 2.0 * (extra + 1.0) - 1.0, &it)) return -1;
    *o = 1 + it;
    return 0;
}

static int hold_count(double times, double extra, int64_t *o) {
    return js_iter(times * 2.0 * (extra + 1.0) - 1.0, o);
}

static int timing_scale(double period, int32_t hscale, double *scale, TgwError *err) {
    if (period == 0.0) period = 1.0;
    double s = period * (double)hscale;
    if (!isfinite(period) || period < 0.0 || hscale <= 0 || !isfinite(s) || s > TGW_TOO_BIG) {
        too_long(err);
        return -1;
    }
    *scale = s;
    return 0;
}

static int x1(uint8_t c, Code *o) {
    switch (c) {
    case 'p':
    case 'h':
        *o = code_k(CK_RISE);
        return 1;
    case 'n':
    case 'l':
        *o = code_k(CK_FALL);
        return 1;
    case 'P':
    case 'H':
        *o = code_k(CK_RISEA);
        return 1;
    case 'N':
    case 'L':
        *o = code_k(CK_FALLA);
        return 1;
    default:
        return 0;
    }
}

static int x5(uint8_t c, Code *o) {
    switch (c) {
    case 'p':
    case 'P':
        *o = code_k(CK_FALL);
        return 1;
    case 'n':
    case 'N':
        *o = code_k(CK_RISE);
        return 1;
    default:
        return 0;
    }
}

static Code x6(uint8_t c) { return code_k((c == 'n' || c == 'N') ? CK_HIGH : CK_LOW); }

static int x4(uint8_t c, Code *o) {
    switch (c) {
    case 'p':
    case 'P':
    case 'h':
    case 'H':
    case '1':
        *o = code_k(CK_HIGH);
        return 1;
    case 'n':
    case 'N':
    case 'l':
    case 'L':
    case '0':
        *o = code_k(CK_LOW);
        return 1;
    case 'x':
        *o = code_k(CK_X);
        return 1;
    case 'd':
        *o = code_k(CK_DASHL);
        return 1;
    case 'u':
        *o = code_k(CK_DASHH);
        return 1;
    case 'z':
        *o = code_k(CK_MID);
        return 1;
    case '=':
    case '2':
        *o = code_bus(2);
        return 1;
    default:
        if (c >= '3' && c <= '9') {
            *o = code_bus((uint8_t)(c - '0'));
            return 1;
        }
        return 0;
    }
}

static int xclude(uint8_t prev, uint8_t next, Code *o) {
    if ((prev == 'h' && next == 'p') || (prev == 'H' && next == 'p') || (prev == 'n' && next == 'h') ||
        (prev == 'N' && next == 'h')) {
        *o = code_k(CK_HIGH);
        return 1;
    }
    if ((prev == 'l' && next == 'n') || (prev == 'L' && next == 'n') || (prev == 'p' && next == 'l') ||
        (prev == 'P' && next == 'l')) {
        *o = code_k(CK_LOW);
        return 1;
    }
    return 0;
}

static int has_y1(uint8_t c) {
    switch (c) {
    case 'p':
    case 'P':
    case 'l':
    case 'L':
    case '0':
    case 'n':
    case 'N':
    case 'h':
    case 'H':
    case '1':
    case 'x':
    case 'd':
    case 'u':
    case 'z':
    case '=':
    case '2':
    case '3':
    case '4':
    case '5':
    case '6':
    case '7':
    case '8':
    case '9':
        return 1;
    default:
        return 0;
    }
}

static int has_x2(uint8_t c) {
    return c == '0' || c == '1' || c == 'x' || c == 'd' || c == 'u' || c == 'z' || c == '=' ||
           (c >= '2' && c <= '9');
}

static int is_flat(Code c) {
    return c.kind == CK_LOW || c.kind == CK_HIGH || c.kind == CK_MID || c.kind == CK_DASHL ||
           c.kind == CK_DASHH || c.kind == CK_X || c.kind == CK_BUS;
}

static void push_pat(Arena *a, WaveOut *w, Pat p) {
    if (w->pat_n == w->pat_cap) {
        uint32_t cap = w->pat_cap * 2;
        Pat *np = arena_alloc(a, sizeof(Pat) * cap);
        memcpy(np, w->pats, sizeof(Pat) * w->pat_n);
        w->pats = np;
        w->pat_cap = cap;
    }
    w->pats[w->pat_n++] = p;
}

static int add_fill(Arena *a, WaveOut *w, Code code, int64_t count) {
    if (count <= 0) return 0;
    if (w->pat_n) {
        Pat *last = &w->pats[w->pat_n - 1];
        if (last->kind == PAT_FILL && code_eq(last->c0, code)) {
            if (__builtin_add_overflow(last->n0, count, &last->n0)) return -1;
            return bounded_add(w->len, count, &w->len);
        }
    }
    Pat p;
    memset(&p, 0, sizeof p);
    p.kind = PAT_FILL;
    p.at = w->len;
    p.c0 = code;
    p.n0 = count;
    push_pat(a, w, p);
    return bounded_add(w->len, count, &w->len);
}

static int add_lead(Arena *a, WaveOut *w, Code lead, Code hold, int64_t holds) {
    if (holds < 0) holds = 0;
    if (is_flat(lead) && code_eq(lead, hold)) return add_fill(a, w, hold, 1 + holds);
    Pat p;
    memset(&p, 0, sizeof p);
    p.kind = PAT_LEAD;
    p.at = w->len;
    p.c0 = lead;
    p.c1 = hold;
    p.n0 = holds;
    push_pat(a, w, p);
    return bounded_add(w->len, 1 + holds, &w->len);
}

static int add_clock(Arena *a, WaveOut *w, Code s0, Code s1, Code s2, Code s3, int64_t extra, int64_t times) {
    if (times <= 0) return 0;
    int64_t bricks;
    if (__builtin_mul_overflow(times, 2, &bricks) || __builtin_mul_overflow(bricks, extra + 1, &bricks))
        return -1;
    if (w->pat_n) {
        Pat *last = &w->pats[w->pat_n - 1];
        if (last->kind == PAT_CLOCK && code_eq(last->c0, s0) && code_eq(last->c1, s1) &&
            code_eq(last->c2, s2) && code_eq(last->c3, s3) && last->n0 == extra) {
            if (__builtin_add_overflow(last->n1, times, &last->n1)) return -1;
            return bounded_add(w->len, bricks, &w->len);
        }
    }
    Pat p;
    memset(&p, 0, sizeof p);
    p.kind = PAT_CLOCK;
    p.at = w->len;
    p.c0 = s0;
    p.c1 = s1;
    p.c2 = s2;
    p.c3 = s3;
    p.n0 = extra;
    p.n1 = times;
    push_pat(a, w, p);
    return bounded_add(w->len, bricks, &w->len);
}

static int push_first(Arena *a, WaveOut *w, uint8_t c, double extra, double repeats) {
    Code edge, s1, s2;
    if (x5(c, &s2)) {
        if (!x1(c, &edge)) return -1;
        if (!x4(c, &s1)) s1 = code_k(CK_X);
        int64_t ex, t;
        if (js_iter(extra, &ex) || js_iter(repeats, &t)) return -1;
        return add_clock(a, w, edge, s1, s2, x6(c), ex, t);
    }
    Code code;
    if (!x4(c, &code)) code = code_k(CK_X);
    int64_t n;
    if (uniform_count(repeats, extra, &n)) return -1;
    return add_fill(a, w, code, n);
}

static int push_trans(Arena *a, WaveOut *w, uint8_t prev, uint8_t next, double extra, double times) {
    Code edge;
    if (x1(next, &edge)) {
        Code s0;
        if (!xclude(prev, next, &s0)) s0 = edge;
        Code s1;
        if (!x4(next, &s1)) s1 = code_k(CK_X);
        Code s2;
        if (x5(next, &s2)) {
            int64_t t, ex;
            if (js_iter(times, &t) || js_iter(extra, &ex)) return -1;
            if (!code_eq(s0, edge) && t > 0) {
                if (add_clock(a, w, s0, s1, s2, x6(next), ex, 1)) return -1;
                return add_clock(a, w, edge, s1, s2, x6(next), ex, t - 1);
            }
            return add_clock(a, w, s0, s1, s2, x6(next), ex, t);
        }
        int64_t holds;
        if (hold_count(times, extra, &holds)) return -1;
        return add_lead(a, w, s0, s1, holds);
    }
    if (!has_x2(next) || !has_y1(prev)) {
        int64_t n;
        if (uniform_count(times, extra, &n)) return -1;
        return add_fill(a, w, code_k(CK_X), n);
    }
    Code hold;
    if (!x4(next, &hold)) hold = code_k(CK_X);
    int64_t holds;
    if (hold_count(times, extra, &holds)) return -1;
    return add_lead(a, w, code_soft(prev, next), hold, holds);
}

/* Bytes that continue a hold: '.' or '|'. Word-at-a-time when every byte matches. */
static size_t scan_hold(const uint8_t *s, size_t n) {
    size_t i = 0;
    while (i + 8 <= n) {
        uint64_t w;
        memcpy(&w, s + i, 8);
        uint64_t dot = w ^ 0x2e2e2e2e2e2e2e2eULL;
        uint64_t bar = w ^ 0x7c7c7c7c7c7c7c7cULL;
        uint64_t zd = (dot - 0x0101010101010101ULL) & ~dot & 0x8080808080808080ULL;
        uint64_t zb = (bar - 0x0101010101010101ULL) & ~bar & 0x8080808080808080ULL;
        if ((zd | zb) != 0x8080808080808080ULL) break;
        i += 8;
    }
    while (i < n && (s[i] == '.' || s[i] == '|')) i++;
    return i;
}

typedef struct Seg {
    const uint8_t *sym;
    size_t n;
    uint8_t code;
    int continued;
    int sub;
} Seg;

static int next_seg(const uint8_t **rem, size_t *rn, uint8_t *prev, int *sub, Seg *seg) {
    const uint8_t *s = *rem;
    size_t n = *rn;
    size_t i = 0;
    while (i < n && (s[i] == '<' || s[i] == '>')) {
        *sub = s[i] == '<';
        i++;
    }
    if (i >= n) {
        *rem = s + n;
        *rn = 0;
        return 0;
    }
    uint8_t first = s[i];
    int continued = first == '.' || first == '|';
    uint8_t code = continued ? *prev : first;
    size_t len = 1 + scan_hold(s + i + 1, n - i - 1);
    seg->sym = s + i;
    seg->n = len;
    seg->code = code;
    seg->continued = continued;
    seg->sub = *sub;
    *prev = code;
    *rem = s + i + len;
    *rn = n - (i + len);
    return 1;
}

static double seg_extra(const Seg *seg, double scale) { return scale * (seg->sub ? 0.5 : 1.0) - 1.0; }

static int prefix_bricks(const Seg *seg, size_t count, double scale, int64_t *out) {
    if (count == 0) {
        *out = 0;
        return 0;
    }
    double extra = seg_extra(seg, scale);
    Code tmp;
    if (x5(seg->code, &tmp)) {
        int64_t half;
        if (js_iter(extra, &half)) return -1;
        half += 1;
        int64_t n = (int64_t)count;
        if (__builtin_mul_overflow(n, 2, &n) || __builtin_mul_overflow(n, half, &n)) return -1;
        *out = n;
        return 0;
    }
    return uniform_count((double)count, extra, out);
}

static void push_d(Arena *a, double **data, uint32_t *n, uint32_t *cap, double v) {
    if (*n == *cap) {
        uint32_t c = *cap ? *cap * 2 : 8;
        double *p = arena_alloc(a, sizeof(double) * c);
        if (*n) memcpy(p, *data, sizeof(double) * *n);
        *data = p;
        *cap = c;
    }
    (*data)[(*n)++] = v;
}

static void mark_push(Arena *a, WaveOut *w, double m, double width) {
    if (w->mark_n == w->mark_cap) {
        uint32_t c = w->mark_cap ? w->mark_cap * 2 : 8;
        double *mm = arena_alloc(a, sizeof(double) * c);
        double *ww = arena_alloc(a, sizeof(double) * c);
        if (w->mark_n) {
            memcpy(mm, w->markers, sizeof(double) * w->mark_n);
            memcpy(ww, w->mwidth, sizeof(double) * w->mark_n);
        }
        w->markers = mm;
        w->mwidth = ww;
        w->mark_cap = c;
    }
    w->markers[w->mark_n] = m;
    w->mwidth[w->mark_n] = width;
    w->mark_n++;
}

static void finish_span(Arena *a, WaveOut *w, int64_t start, int64_t end, double left, double right, int64_t skip) {
    if ((double)end <= left) {
        w->unseen++;
        return;
    }
    double vis_s = (double)start > left ? (double)start : left;
    double vis_e = (double)end < right ? (double)end : right;
    if (vis_s >= vis_e) return;
    mark_push(a, w, (vis_s + vis_e) / 2.0 - (double)(skip * TGW_XS), vis_e - vis_s);
}

static void build_markers(Arena *a, WaveOut *w, int64_t skip, double phase, double width) {
    double left = phase * (double)TGW_XS;
    double right = left + width * (double)TGW_XS;
    int pending = 0;
    int64_t pend_s = 0, pend_e = 0;
    for (uint32_t pi = 0; pi < w->pat_n; pi++) {
        Pat p = w->pats[pi];
        int64_t at, end;
        int bus = 0, has_lead = 0;
        Code lead = code_k(0);
        if (p.kind == PAT_FILL) {
            at = p.at;
            end = p.at + p.n0;
            bus = p.c0.kind == CK_BUS;
        } else if (p.kind == PAT_LEAD) {
            at = p.at;
            end = p.at + 1 + p.n0;
            bus = p.c1.kind == CK_BUS;
            has_lead = 1;
            lead = p.c0;
        } else {
            at = p.at;
            end = p.at + 2 * (p.n0 + 1) * p.n1;
        }
        at *= TGW_XS;
        end *= TGW_XS;
        if (bus && !has_lead && pending && pend_e == at) {
            pend_e = end;
            continue;
        }
        if (pending) {
            int64_t previous_end = pend_e;
            if (has_lead && lead.kind == CK_SOFT) {
                Code hold;
                if (x4(lead.a, &hold) && hold.kind == CK_BUS) previous_end = at + TGW_XLABEL;
            }
            finish_span(a, w, pend_s, previous_end, left, right, skip);
            pending = 0;
        }
        if (bus) {
            pend_s = at + (has_lead ? TGW_XLABEL : 0);
            pend_e = end;
            pending = 1;
        }
    }
    if (pending) finish_span(a, w, pend_s, pend_e, left, right, skip);
}

int wave_compile(Arena *a, WaveOut *w, const char *wave, size_t n, double period, int32_t hscale,
                 double phase_bricks, double width_bricks, TgwError *err) {
    wave_init(w);
    double scale;
    if (timing_scale(period, hscale, &scale, err)) return -1;
    if (!isfinite(phase_bricks) || fabs(phase_bricks) > TGW_TOO_BIG || !isfinite(width_bricks) ||
        width_bricks < 0.0 || width_bricks > TGW_TOO_BIG * 4.0) {
        too_long(err);
        return -1;
    }
    const uint8_t *rem = (const uint8_t *)wave;
    size_t rn = n;
    uint8_t prev = 'x';
    int sub = 0;
    int have_prev = 0;
    uint8_t prev_code = 'x';
    Seg seg;
    while (next_seg(&rem, &rn, &prev, &sub, &seg)) {
        double extra = seg_extra(&seg, scale);
        int64_t start = w->len;
        double repeats = (double)seg.n;
        int rc;
        if (have_prev && !seg.continued)
            rc = push_trans(a, w, prev_code, seg.code, extra, repeats);
        else
            rc = push_first(a, w, seg.code, extra, repeats);
        if (rc) {
            too_long(err);
            return -1;
        }
        for (size_t i = 0; i < seg.n; i++) {
            if (seg.sym[i] != '|') continue;
            int64_t lo, hi;
            if (prefix_bricks(&seg, i, scale, &lo) || prefix_bricks(&seg, i + 1, scale, &hi)) {
                too_long(err);
                return -1;
            }
            double center = (double)start + ((double)lo + (double)hi) / 2.0;
            if (w->gap_n == w->gap_cap) {
                uint32_t c = w->gap_cap ? w->gap_cap * 2 : 4;
                double *g = arena_alloc(a, sizeof(double) * c);
                if (w->gap_n) memcpy(g, w->gaps, sizeof(double) * w->gap_n);
                w->gaps = g;
                w->gap_cap = c;
            }
            w->gaps[w->gap_n++] = (center - phase_bricks) * (double)TGW_XS;
        }
        prev_code = seg.code;
        have_prev = 1;
    }
    w->skip = phase_bricks > 0.0 ? (int64_t)floor(phase_bricks) : 0;
    build_markers(a, w, w->skip, phase_bricks, width_bricks);
    return 0;
}

int wave_nodes(Arena *a, const char *wave, size_t n, double period, int32_t hscale, double **out,
               size_t *out_n, TgwError *err) {
    double scale;
    if (timing_scale(period, hscale, &scale, err)) return -1;
    const uint8_t *rem = (const uint8_t *)wave;
    size_t rn = n;
    uint8_t prev = 'x';
    int sub = 0;
    int64_t start = 0;
    double *pos = NULL;
    uint32_t pn = 0, pc = 0;
    Seg seg;
    while (next_seg(&rem, &rn, &prev, &sub, &seg)) {
        for (size_t i = 0; i < seg.n; i++) {
            int64_t off;
            if (prefix_bricks(&seg, i, scale, &off)) {
                too_long(err);
                return -1;
            }
            int64_t posb;
            if (bounded_add(start, off, &posb)) {
                too_long(err);
                return -1;
            }
            push_d(a, &pos, &pn, &pc, (double)posb * (double)TGW_XS);
        }
        int64_t all;
        if (prefix_bricks(&seg, seg.n, scale, &all) || bounded_add(start, all, &start)) {
            too_long(err);
            return -1;
        }
    }
    *out = pos;
    *out_n = pn;
    return 0;
}
