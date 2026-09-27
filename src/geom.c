#include "common.h"

const char *bus_fill(uint8_t n) {
    switch (n) {
    case 3:
        return "#ffffb4";
    case 4:
        return "#ffe0b9";
    case 5:
        return "#b9e0ff";
    case 6:
        return "#ccfdfe";
    case 7:
        return "#cdfdc5";
    case 8:
        return "#f0c1fb";
    case 9:
        return "#f5c2c0";
    default:
        return "#ffffff";
    }
}

void paint_init(Paint *p) {
    memset(p, 0, sizeof *p);
    buf_init(&p->body, p->stack, sizeof p->stack);
}

void paint_free(Paint *p) {
    if (p->body.heap) free(p->body.data);
    p->body.data = NULL;
    p->body.heap = 0;
    p->body.len = 0;
    p->body.cap = 0;
}

static void open_p(Buf *b) { BUF_LIT(b, "<path d=\""); }

static void close_stroke(Buf *b, int dash) {
    buf_push(b, '"');
    if (dash) BUF_LIT(b, " stroke-dasharray=\"1,3\"");
    BUF_LIT(b, "/>");
}

static void close_fill(Buf *b, const char *fill) {
    BUF_LIT(b, "\" fill=\"");
    buf_puts(b, fill);
    BUF_LIT(b, "\" stroke=\"none\"/>");
}

static void cmd_m(Buf *b, int64_t x, int64_t y) {
    buf_push(b, 'M');
    push_i64(b, x);
    buf_push(b, ' ');
    push_i64(b, y);
}
static void cmd_l(Buf *b, int64_t x, int64_t y) {
    buf_push(b, 'L');
    push_i64(b, x);
    buf_push(b, ' ');
    push_i64(b, y);
}
static void cmd_h(Buf *b, int64_t x) {
    buf_push(b, 'H');
    push_i64(b, x);
}
static void cmd_v(Buf *b, int64_t y) {
    buf_push(b, 'V');
    push_i64(b, y);
}

static void rect(Buf *b, int64_t x, int64_t y, int64_t w, int64_t h, const char *fill) {
    BUF_LIT(b, "<rect x=\"");
    push_i64(b, x);
    BUF_LIT(b, "\" y=\"");
    push_i64(b, y);
    BUF_LIT(b, "\" width=\"");
    push_i64(b, w);
    BUF_LIT(b, "\" height=\"");
    push_i64(b, h);
    BUF_LIT(b, "\" fill=\"");
    buf_puts(b, fill);
    BUF_LIT(b, "\" stroke=\"none\"/>");
}

static void edge_d(Buf *b, Code code, int64_t x0, int64_t x1) {
    switch (code.kind) {
    case CK_RISE:
    case CK_RISEA:
        cmd_m(b, x0, TGW_YS);
        cmd_v(b, 0);
        cmd_h(b, x1);
        break;
    case CK_FALL:
    case CK_FALLA:
        cmd_m(b, x0, 0);
        cmd_v(b, TGW_YS);
        cmd_h(b, x1);
        break;
    case CK_LOW:
    case CK_DASHL:
        cmd_m(b, x0, TGW_YS);
        cmd_h(b, x1);
        break;
    case CK_MID:
        cmd_m(b, x0, TGW_YS / 2);
        cmd_h(b, x1);
        break;
    default:
        cmd_m(b, x0, 0);
        cmd_h(b, x1);
        break;
    }
}

static void arrow_at(Buf *b, int64_t x, int rise) {
    open_p(b);
    if (rise) {
        cmd_m(b, x - 3, 12);
        cmd_l(b, x, 3);
        cmd_l(b, x + 3, 12);
        buf_push(b, 'Z');
    } else {
        cmd_m(b, x - 3, 8);
        cmd_l(b, x, 17);
        cmd_l(b, x + 3, 8);
        buf_push(b, 'Z');
    }
    close_fill(b, "#000");
}

static void clock_edge(Buf *b, Code code, int64_t x0, int64_t x1) {
    open_p(b);
    edge_d(b, code, x0, x1);
    close_stroke(b, code.kind == CK_DASHH || code.kind == CK_DASHL);
    if (code.kind == CK_RISEA || code.kind == CK_FALLA) arrow_at(b, x0, code.kind == CK_RISEA);
}

void write_clock_pattern(Buf *body, uint32_t id, Code s0, Code s1, Code s2, Code s3, int64_t extra) {
    int64_t half = (extra + 1) * TGW_XS;
    int64_t full = half * 2;
    (void)s1;
    BUF_LIT(body, "<pattern id=\"k");
    push_i64(body, (int64_t)id);
    BUF_LIT(body, "\" x=\"-4\" y=\"-2\" width=\"");
    push_i64(body, full);
    BUF_LIT(body, "\" height=\"");
    push_i64(body, TGW_YS + 4);
    BUF_LIT(body, "\" patternUnits=\"userSpaceOnUse\" fill=\"none\" stroke=\"#000\" stroke-width=\"1\" "
                  "stroke-linecap=\"round\" stroke-linejoin=\"round\" viewBox=\"-4 -2 ");
    push_i64(body, full);
    buf_push(body, ' ');
    push_i64(body, TGW_YS + 4);
    BUF_LIT(body, "\">");
    open_p(body);
    edge_d(body, s3, -4, 0);
    edge_d(body, s0, 0, half);
    edge_d(body, s2, half, full - 4);
    close_stroke(body, 0);
    if (s0.kind == CK_RISEA) arrow_at(body, 0, 1);
    if (s0.kind == CK_FALLA) arrow_at(body, 0, 0);
    if (s2.kind == CK_RISEA) arrow_at(body, half, 1);
    if (s2.kind == CK_FALLA) arrow_at(body, half, 0);
    BUF_LIT(body, "</pattern>");
}

static void hline(Paint *p, int64_t x, int64_t w, int64_t y, int dash) {
    open_p(&p->body);
    cmd_m(&p->body, x, y);
    cmd_h(&p->body, x + w);
    close_stroke(&p->body, dash);
}

static void rails(Paint *p, int64_t x, int64_t w) {
    open_p(&p->body);
    cmd_m(&p->body, x, 0);
    cmd_h(&p->body, x + w);
    cmd_m(&p->body, x, TGW_YS);
    cmd_h(&p->body, x + w);
    close_stroke(&p->body, 0);
}

static void draw_span(Paint *p, Code code, int64_t x, int64_t w);

static void band_band(Paint *p, int64_t x, const char *fill1, const char *fill2) {
    Buf *b = &p->body;
    open_p(b);
    cmd_m(b, x, 0);
    cmd_l(b, x + 3, 0);
    cmd_l(b, x + 6, TGW_YS / 2);
    cmd_l(b, x + 3, TGW_YS);
    cmd_l(b, x, TGW_YS);
    close_fill(b, fill1);
    open_p(b);
    cmd_m(b, x + TGW_XS, 0);
    cmd_l(b, x + 9, 0);
    cmd_l(b, x + 6, TGW_YS / 2);
    cmd_l(b, x + 9, TGW_YS);
    cmd_l(b, x + TGW_XS, TGW_YS);
    close_fill(b, fill2);
    open_p(b);
    cmd_m(b, x, 0);
    cmd_l(b, x + 3, 0);
    cmd_l(b, x + 9, TGW_YS);
    cmd_l(b, x + TGW_XS, TGW_YS);
    cmd_m(b, x, TGW_YS);
    cmd_l(b, x + 3, TGW_YS);
    cmd_l(b, x + 9, 0);
    cmd_l(b, x + TGW_XS, 0);
    close_stroke(b, 0);
}

static void line_band(Paint *p, int64_t x, int64_t y, int dash, const char *fill) {
    Buf *b = &p->body;
    open_p(b);
    cmd_m(b, x + 3, y);
    cmd_l(b, x + 9, 0);
    cmd_l(b, x + TGW_XS, 0);
    cmd_l(b, x + TGW_XS, TGW_YS);
    cmd_l(b, x + 9, TGW_YS);
    buf_push(b, 'Z');
    close_fill(b, fill);
    hline(p, x, 3, y, dash);
    open_p(b);
    cmd_m(b, x + TGW_XS, 0);
    cmd_l(b, x + 9, 0);
    cmd_l(b, x + 3, y);
    cmd_l(b, x + 9, TGW_YS);
    cmd_h(b, x + TGW_XS);
    close_stroke(b, 0);
}

static void band_line(Paint *p, int64_t x, const char *fill, int64_t y, int dash) {
    Buf *b = &p->body;
    open_p(b);
    cmd_m(b, x, 0);
    cmd_l(b, x + 3, 0);
    cmd_l(b, x + 9, y);
    cmd_l(b, x + 3, TGW_YS);
    cmd_l(b, x, TGW_YS);
    buf_push(b, 'Z');
    close_fill(b, fill);
    open_p(b);
    cmd_m(b, x, 0);
    cmd_l(b, x + 3, 0);
    cmd_l(b, x + 9, y);
    cmd_l(b, x + 3, TGW_YS);
    cmd_h(b, x);
    close_stroke(b, 0);
    hline(p, x + 9, TGW_XS - 9, y, dash);
}

static void slope(Paint *p, int64_t x, int64_t y1, int64_t y2, int dash) {
    Buf *b = &p->body;
    open_p(b);
    cmd_m(b, x, y1);
    cmd_l(b, x + 3, y1);
    cmd_l(b, x + 9, y2);
    cmd_h(b, x + TGW_XS);
    close_stroke(b, dash);
}

static void bump(Paint *p, int64_t x, int64_t y) {
    int64_t mid = y == 0 ? 10 : y - 10;
    Buf *b = &p->body;
    open_p(b);
    cmd_m(b, x, y);
    cmd_l(b, x + 3, y);
    cmd_l(b, x + 6, mid);
    cmd_l(b, x + 9, y);
    cmd_h(b, x + TGW_XS);
    close_stroke(b, 0);
}

enum { END_LINE = 1, END_X, END_BUS };

static void end_kind(uint8_t c, int *kind, int64_t *y, int *dash, uint8_t *bus) {
    *dash = 0;
    *bus = 0;
    switch (c) {
    case '0':
    case 'l':
    case 'L':
    case 'p':
    case 'P':
        *kind = END_LINE;
        *y = TGW_YS;
        return;
    case 'd':
        *kind = END_LINE;
        *y = TGW_YS;
        *dash = 1;
        return;
    case '1':
    case 'h':
    case 'H':
    case 'n':
    case 'N':
        *kind = END_LINE;
        *y = 0;
        return;
    case 'u':
        *kind = END_LINE;
        *y = 0;
        *dash = 1;
        return;
    case 'z':
        *kind = END_LINE;
        *y = TGW_YS / 2;
        return;
    case 'x':
        *kind = END_X;
        return;
    case '=':
    case '2':
        *kind = END_BUS;
        *bus = 2;
        return;
    default:
        if (c >= '3' && c <= '9') {
            *kind = END_BUS;
            *bus = (uint8_t)(c - '0');
            return;
        }
        *kind = END_X;
    }
}

static void draw_soft(Paint *p, uint8_t prev, uint8_t next, int64_t x) {
    int ka, kb, da, db;
    int64_t ya, yb;
    uint8_t ba, bb;
    end_kind(prev, &ka, &ya, &da, &ba);
    end_kind(next, &kb, &yb, &db, &bb);
    if (ka == END_BUS && kb == END_BUS) {
        band_band(p, x, bus_fill(ba), bus_fill(bb));
        return;
    }
    if (ka == END_LINE && kb == END_BUS) {
        line_band(p, x, ya, da, bus_fill(bb));
        return;
    }
    if (ka == END_BUS && kb == END_LINE) {
        band_line(p, x, bus_fill(ba), yb, db);
        return;
    }
    if (ka == END_X && kb == END_BUS) {
        p->hatch = 1;
        band_band(p, x, "url(#xh)", bus_fill(bb));
        return;
    }
    if (ka == END_BUS && kb == END_X) {
        p->hatch = 1;
        band_band(p, x, bus_fill(ba), "url(#xh)");
        return;
    }
    if (ka == END_X && kb == END_X) {
        draw_span(p, code_k(CK_X), x, TGW_XS);
        return;
    }
    if (ka == END_X && kb == END_LINE) {
        p->hatch = 1;
        band_line(p, x, "url(#xh)", yb, db);
        return;
    }
    if (ka == END_LINE && kb == END_X) {
        p->hatch = 1;
        line_band(p, x, ya, da, "url(#xh)");
        return;
    }
    if (ya == yb && !da && !db && (ya == 0 || ya == TGW_YS))
        bump(p, x, ya);
    else if (ya == yb)
        hline(p, x, TGW_XS, ya, da || db);
    else
        slope(p, x, ya, yb, da || db);
}

static void draw_one(Paint *p, Code code, int64_t x);

static void draw_span(Paint *p, Code code, int64_t x, int64_t w) {
    if (w <= 0) return;
    switch (code.kind) {
    case CK_HIGH:
    case CK_DASHH:
        hline(p, x, w, 0, code.kind == CK_DASHH);
        break;
    case CK_LOW:
    case CK_DASHL:
        hline(p, x, w, TGW_YS, code.kind == CK_DASHL);
        break;
    case CK_MID:
        hline(p, x, w, TGW_YS / 2, 0);
        break;
    case CK_X:
        p->hatch = 1;
        rect(&p->body, x, 0, w, TGW_YS, "url(#xh)");
        rails(p, x, w);
        break;
    case CK_BUS:
        rect(&p->body, x, 0, w, TGW_YS, bus_fill(code.a));
        rails(p, x, w);
        break;
    default:
        draw_one(p, code, x);
        break;
    }
}

static void edge(Paint *p, int64_t x, int rise, int arrow) {
    Code code = code_k(rise ? (arrow ? CK_RISEA : CK_RISE) : (arrow ? CK_FALLA : CK_FALL));
    clock_edge(&p->body, code, x, x + TGW_XS);
}

static void draw_one(Paint *p, Code code, int64_t x) {
    switch (code.kind) {
    case CK_RISE:
        edge(p, x, 1, 0);
        break;
    case CK_FALL:
        edge(p, x, 0, 0);
        break;
    case CK_RISEA:
        edge(p, x, 1, 1);
        break;
    case CK_FALLA:
        edge(p, x, 0, 1);
        break;
    case CK_SOFT:
        draw_soft(p, code.a, code.b, x);
        break;
    default:
        draw_span(p, code, x, TGW_XS);
        break;
    }
}

static uint32_t clock_id(Paint *p, Code s0, Code s1, Code s2, Code s3, int64_t extra) {
    for (uint32_t i = 0; i < p->clocks; i++) {
        if (code_eq(p->ks0[i], s0) && code_eq(p->ks1[i], s1) && code_eq(p->ks2[i], s2) &&
            code_eq(p->ks3[i], s3) && p->kextra[i] == extra)
            return i;
    }
    if (p->clocks < 64) {
        uint32_t id = p->clocks++;
        p->ks0[id] = s0;
        p->ks1[id] = s1;
        p->ks2[id] = s2;
        p->ks3[id] = s3;
        p->kextra[id] = extra;
        return id;
    }
    return 0;
}

static void draw_clock(Paint *p, int64_t at, Code s0, Code s1, Code s2, Code s3, int64_t extra, int64_t times,
                       int64_t skip, int64_t end) {
    int64_t period = 2 * (extra + 1);
    int64_t total = times * period;
    int64_t hi = at + total < end ? at + total : end;
    int64_t lo = at > skip ? at : skip;
    if (lo >= hi || total <= 0) return;
    uint32_t id = clock_id(p, s0, s1, s2, s3, extra);
    int64_t x_start = (at - skip) * TGW_XS;
    int64_t x_end = (hi - skip) * TGW_XS;
    const int64_t PAD = 4;
    if (at >= skip) clock_edge(&p->body, s0, x_start, x_start + PAD);
    int64_t left = x_start + PAD;
    if (left < 0) left = 0;
    int complete = at + total <= end;
    int64_t right = complete ? x_end - PAD : x_end;
    int64_t origin = at < skip ? -((skip - at) % period) * TGW_XS : x_start;
    BUF_LIT(&p->body, "<g transform=\"translate(");
    push_i64(&p->body, origin);
    BUF_LIT(&p->body, ")\"><rect x=\"");
    push_i64(&p->body, left - origin);
    BUF_LIT(&p->body, "\" y=\"-2\" width=\"");
    push_i64(&p->body, right - left);
    BUF_LIT(&p->body, "\" height=\"");
    push_i64(&p->body, TGW_YS + 4);
    BUF_LIT(&p->body, "\" fill=\"url(#k");
    push_i64(&p->body, (int64_t)id);
    BUF_LIT(&p->body, ")\" stroke=\"none\"/></g>");
    if (complete) draw_span(p, s3, right, PAD);
}

void paint_wave(Paint *p, const Pat *pats, uint32_t n, int64_t skip, int64_t end) {
    if (end <= skip) return;
    BUF_LIT(&p->body, "<g fill=\"none\" stroke=\"#000\" stroke-width=\"1\" stroke-linecap=\"round\" "
                      "stroke-linejoin=\"round\">");
    for (uint32_t i = 0; i < n; i++) {
        Pat pat = pats[i];
        if (pat.at >= end) break;
        if (pat.kind == PAT_FILL) {
            int64_t lo = pat.at > skip ? pat.at : skip;
            int64_t hi = pat.at + pat.n0 < end ? pat.at + pat.n0 : end;
            if (lo >= hi) continue;
            draw_span(p, pat.c0, (lo - skip) * TGW_XS, (hi - lo) * TGW_XS);
        } else if (pat.kind == PAT_LEAD) {
            if (pat.at >= skip) draw_one(p, pat.c0, (pat.at - skip) * TGW_XS);
            if (pat.n0 > 0) {
                int64_t start = pat.at + 1;
                int64_t lo = start > skip ? start : skip;
                int64_t hi = start + pat.n0 < end ? start + pat.n0 : end;
                if (lo < hi) draw_span(p, pat.c1, (lo - skip) * TGW_XS, (hi - lo) * TGW_XS);
            }
        } else if (pat.kind == PAT_CLOCK) {
            draw_clock(p, pat.at, pat.c0, pat.c1, pat.c2, pat.c3, pat.n0, pat.n1, skip, end);
        }
    }
    BUF_LIT(&p->body, "</g>");
}
