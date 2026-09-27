#include "common.h"

typedef __int128 i128;

typedef struct Ev {
    uint32_t ch;
    double x, y;
} Ev;

static int is_upper_cp(uint32_t cp) { return cp >= 'A' && cp <= 'Z'; }

static double frac_dx(double phase, int64_t xmin) {
    double bricks = phase * 2.0 + (double)xmin;
    double flo = floor(bricks > 0.0 ? bricks : 0.0);
    return (flo - bricks) * (double)TGW_XS;
}

static void format_number(Buf *b, double v) {
    if (isfinite(v) && v == trunc(v) && fabs(v) < 1.0e15) {
        push_i64(b, (int64_t)round(v));
        return;
    }
    fmt_rust_f64(b, v);
}

static void series_text(Buf *b, double value, int dp, int fixed) {
    if (fixed)
        fmt_fixed(b, value, dp);
    else
        format_number(b, value);
}

static double tick_width(const Tick *tick, size_t count) {
    if (!count || tick->kind == 0) return 0;
    if (tick->kind == 1) {
        double last = tick->offset + tick->step * (double)(count - 1);
        uint8_t sa[128], sb[128];
        Buf a, b;
        buf_init(&a, sa, sizeof sa);
        buf_init(&b, sb, sizeof sb);
        series_text(&a, tick->offset, tick->dp, tick->fixed);
        series_text(&b, last, tick->dp, tick->fixed);
        double w = tgw_text_width((char *)a.data, a.len, 11);
        double w2 = tgw_text_width((char *)b.data, b.len, 11);
        buf_free(&a);
        buf_free(&b);
        return w > w2 ? w : w2;
    }
    double m = 0;
    size_t n = count < tick->labels.len ? count : tick->labels.len;
    for (size_t i = 0; i < n; i++) {
        double w = tgw_text_width(tick->labels.data[i].p, tick->labels.data[i].n, 11);
        if (w > m) m = w;
    }
    return m;
}

static void tick_counts(double xmax, int32_t hscale, size_t *ticks, size_t *tocks) {
    double marks = (xmax > 0 ? xmax : 0) / (2.0 * (double)hscale);
    *ticks = (size_t)floor(marks) + 1;
    *tocks = marks >= 0.5 ? (size_t)floor(marks - 0.5) + 1 : 0;
}

static int mod_zero(double n, double every) {
    if (every == 0) return 1;
    double r = fmod(n, every);
    return fabs(r) < 1e-9 || fabs(r - every) < 1e-9 || fabs(r + every) < 1e-9;
}

static i128 rem_euclid_i(i128 a, i128 m) {
    i128 r = a % m;
    if (r < 0) r += m < 0 ? -m : m;
    return r;
}

static i128 inverse_mod(i128 value, i128 modulus) {
    i128 a = value, b = modulus, x = 1, y = 0;
    while (b != 0) {
        i128 q = a / b;
        i128 nb = a - q * b;
        a = b;
        b = nb;
        i128 ny = x - q * y;
        x = y;
        y = ny;
    }
    return rem_euclid_i(x, modulus);
}

static int rational_interval(double value, i128 *op, i128 *oq) {
    i128 p0 = 0, p1 = 1, q0 = 1, q1 = 0;
    double rest = value;
    for (int i = 0; i < 32; i++) {
        if (!isfinite(rest) || rest > 1e30) return 0;
        i128 whole = (i128)floor(rest);
        i128 p, q;
        if (__builtin_mul_overflow(whole, p1, &p) || __builtin_add_overflow(p, p0, &p)) return 0;
        if (__builtin_mul_overflow(whole, q1, &q) || __builtin_add_overflow(q, q0, &q)) return 0;
        /* 10^27, the Rust i128 bound. A decimal literal does not fit in long long. */
        const i128 pmax = ((i128)1000000000) * ((i128)1000000000) * ((i128)1000000000);
        if (q > 1000000000 || p > pmax) return 0;
        if (q > 0 && p > 0 && fabs((double)p / (double)q - value) < 1e-12) {
            *op = p;
            *oq = q;
            return 1;
        }
        p0 = p1;
        p1 = p;
        q0 = q1;
        q1 = q;
        rest = 1.0 / (rest - (double)whole);
    }
    return 0;
}

static int tick_filter(double offset, double every, size_t *first, size_t *period) {
    every = fabs(every);
    if (every <= 1e-9) {
        *first = 0;
        *period = 1;
        return 1;
    }
    if (every == trunc(every)) {
        double f = fmod(-offset, every);
        if (f < 0) f += every;
        if (fabs(f - round(f)) > 1e-9 || f > 1e19) return 0;
        *first = (size_t)round(f);
        size_t ev = (size_t)every;
        *period = ev ? ev : 1;
        return 1;
    }
    i128 p, q;
    if (!rational_interval(every, &p, &q)) return 0;
    double rem = fmod(offset, every);
    if (rem < 0) rem += every;
    rem *= (double)q;
    if (fabs(rem - round(rem)) > 1e-7) return 0;
    i128 remainder = rem_euclid_i((i128)round(rem), p);
    i128 k = 0;
    if (q != 1) k = rem_euclid_i(rem_euclid_i(remainder, q) * inverse_mod(rem_euclid_i(p, q), q), q);
    i128 fr = rem_euclid_i((k * p - remainder) / q, p);
    if (fr < 0) return 0;
    *first = (size_t)fr;
    size_t per = p > (i128)SIZE_MAX ? SIZE_MAX : (size_t)p;
    *period = per ? per : 1;
    return 1;
}

static void tick_text(Buf *b, double x, double y, const char *t, size_t n) {
    BUF_LIT(b, "<text x=\"");
    push_f64(b, x);
    BUF_LIT(b, "\" y=\"");
    push_f64(b, y);
    BUF_LIT(b, "\">");
    push_esc(b, t, n);
    BUF_LIT(b, "</text>");
}

static size_t div_ceil_sz(size_t a, size_t b) { return b ? a / b + (a % b != 0) : a; }

static void write_ticks(Buf *b, const Tick *tick, double x0, double dx, double y, size_t count, double every) {
    if (tick->kind == 0) return;
    if (tick->kind == 2 && count > tick->labels.len) count = tick->labels.len;
    if (!count) return;
    double offset = tick->kind == 1 ? tick->offset : 0;
    size_t first, period;
    if (!tick_filter(offset, every, &first, &period)) return;
    double tw = tick_width(tick, count);
    double cells = ceil((tw + 10.0) / dx);
    if (!isfinite(cells) || cells < 0) cells = 1;
    if (cells > 1e9) cells = 1e9;
    size_t minimum = (size_t)cells;
    size_t cap = div_ceil_sz(count, 10000);
    if (minimum < cap) minimum = cap;
    if (minimum < 1) minimum = 1;
    size_t mul = div_ceil_sz(minimum, period);
    if (mul < 1) mul = 1;
    size_t stride = period > SIZE_MAX / mul ? SIZE_MAX : period * mul;
    if (!stride) stride = 1;
    int opened = 0;
    for (size_t i = first; i < count; i += stride) {
        if (every != 0.0 && !mod_zero((double)i + offset, every)) {
            if (stride > count) break;
            continue;
        }
        uint8_t stack[128];
        Buf text;
        buf_init(&text, stack, sizeof stack);
        const char *tp;
        size_t tn;
        if (tick->kind == 1) {
            series_text(&text, tick->offset + tick->step * (double)i, tick->dp, tick->fixed);
            tp = (char *)text.data;
            tn = text.len;
        } else {
            tp = tick->labels.data[i].p;
            tn = tick->labels.data[i].n;
        }
        if (!opened) {
            BUF_LIT(b, "<g fill=\"#64748b\" font-size=\"11\" text-anchor=\"middle\" xml:space=\"preserve\">");
            opened = 1;
        }
        tick_text(b, (double)i * dx + x0, y, tp, tn);
        buf_free(&text);
        if (stride > count) break;
    }
    if (opened) BUF_LIT(b, "</g>");
}

static void margins(const Cap *cap, int64_t *y0, int64_t *y1) {
    int tick = cap->tick.kind != 0 || cap->tock.kind != 0;
    *y0 = tick ? 20 : 0;
    *y1 = cap->has_text ? 36 : 0;
}

static void clip_rect(Buf *b, const char *id, double x, double y, double w, double h) {
    BUF_LIT(b, "<clipPath id=\"");
    buf_puts(b, id);
    BUF_LIT(b, "\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"");
    push_f64(b, x);
    BUF_LIT(b, "\" y=\"");
    push_f64(b, y);
    BUF_LIT(b, "\" width=\"");
    push_f64(b, w > 0 ? w : 0);
    BUF_LIT(b, "\" height=\"");
    push_f64(b, h > 0 ? h : 0);
    BUF_LIT(b, "\"/></clipPath>");
}

static void draw_gap(Buf *b, double x) {
    BUF_LIT(b, "<g transform=\"translate(");
    push_f64(b, x);
    BUF_LIT(b, ")\"><path d=\"M-5,22 C0,22 0,-2 5,-2\" fill=\"none\" stroke=\"#fff\" stroke-width=\"6\"/>"
              "<path d=\"M-7,22 C-2,22 -2,-2 3,-2\" fill=\"none\" stroke=\"#000\" stroke-width=\"1\"/>"
              "<path d=\"M-3,22 C2,22 2,-2 7,-2\" fill=\"none\" stroke=\"#000\" stroke-width=\"1\"/></g>");
}

static const char *ou_color(uint8_t c) {
    switch (c) {
    case '2':
        return "#e90000";
    case '3':
        return "#3edd00";
    case '4':
        return "#0074cd";
    case '5':
        return "#ff15db";
    case '6':
        return "#af9800";
    case '7':
        return "#00864f";
    case '8':
        return "#a076ff";
    default:
        return "#000000";
    }
}

static void ou_span(Buf *b, double start, double end, const char *color, int arrow, double width) {
    double x1 = start > 0 ? start : 0;
    double x2 = end < width ? end : width;
    if (x1 < x2) {
        BUF_LIT(b, "<line stroke=\"");
        buf_puts(b, color);
        BUF_LIT(b, "\" x1=\"");
        push_f64(b, x1);
        BUF_LIT(b, "\" x2=\"");
        push_f64(b, x2);
        BUF_LIT(b, "\" y1=\"0\" y2=\"0\"/>");
    }
    if (arrow && end >= 0 && end <= width && start < end) {
        BUF_LIT(b, "<path fill=\"");
        buf_puts(b, color);
        BUF_LIT(b, "\" stroke=\"none\" d=\"M");
        push_f64(b, end - 7.0);
        BUF_LIT(b, ",0 l7,7 v-7 z\"/>");
    }
}

static void draw_ou(Buf *b, const char *text, size_t n, int under, double period, double phase, double width) {
    if (!n || width <= 0) return;
    double step = period * 2.0 * (double)TGW_XS;
    double xoff = -phase * 2.0 * (double)TGW_XS;
    BUF_LIT(b, "<g transform=\"translate(0,");
    push_i64(b, under ? TGW_YS : 0);
    BUF_LIT(b, ")\" fill=\"none\" stroke-width=\"3\">");
    int have = 0;
    double start = 0;
    const char *color = "#000000";
    for (size_t i = 0; i < n; i++) {
        double x = (double)i * step + xoff;
        uint8_t symbol = (uint8_t)text[i];
        if (x > width + 7.0) {
            if (have) ou_span(b, start, x, color, 0, width);
            have = 0;
            break;
        }
        if (symbol != '.') {
            if (have) ou_span(b, start, x, color, !under, width);
            have = 0;
            if (symbol != '0') {
                start = x + 12.0;
                color = ou_color(symbol);
                have = 1;
            }
        }
    }
    if (have) ou_span(b, start, (double)n * step + xoff, color, 0, width);
    BUF_LIT(b, "</g>");
}

static void append_num(Buf *b, double n) { push_f64(b, n); }

static void path_line(Buf *b, double x1, double y1, double x2, double y2) {
    buf_push(b, 'M');
    append_num(b, x1);
    buf_push(b, ',');
    append_num(b, y1);
    buf_push(b, ' ');
    append_num(b, x2);
    buf_push(b, ',');
    append_num(b, y2);
}

static void path_curve(Buf *b, double x, double y, double c1x, double c1y, double c2x, double c2y, double ex,
                       double ey) {
    buf_push(b, 'M');
    append_num(b, x);
    buf_push(b, ',');
    append_num(b, y);
    BUF_LIT(b, " c ");
    append_num(b, c1x);
    buf_push(b, ',');
    append_num(b, c1y);
    buf_push(b, ' ');
    append_num(b, c2x);
    buf_push(b, ',');
    append_num(b, c2y);
    buf_push(b, ' ');
    append_num(b, ex);
    buf_push(b, ',');
    append_num(b, ey);
}

static void path_ortho(Buf *b, double x, double y, double dx1, double dy1, double dx2, double dy2) {
    buf_push(b, 'm');
    append_num(b, x);
    buf_push(b, ',');
    append_num(b, y);
    buf_push(b, ' ');
    append_num(b, dx1);
    buf_push(b, ',');
    append_num(b, dy1);
    buf_push(b, ' ');
    append_num(b, dx2);
    buf_push(b, ',');
    append_num(b, dy2);
}

static void path_elbow(Buf *b, double x, double y, double dx, double dy) {
    path_ortho(b, x, y, dx / 2.0, 0, 0, dy);
    buf_push(b, ' ');
    append_num(b, dx / 2.0);
    BUF_LIT(b, ",0");
}

static void label_at(Buf *b, double x, double y, const char *text, size_t n, double font) {
    if (!(font > 0)) font = 11;
    double w = tgw_text_width(text, n, font) + 6.0;
    BUF_LIT(b, "<g transform=\"translate(");
    push_f64(b, x);
    buf_push(b, ',');
    push_f64(b, y);
    BUF_LIT(b, ")\"><rect x=\"");
    push_f64(b, -(w / 2.0));
    BUF_LIT(b, "\" y=\"");
    push_f64(b, -(font / 2.0) - 2.0);
    BUF_LIT(b, "\" width=\"");
    push_f64(b, w);
    BUF_LIT(b, "\" height=\"");
    push_f64(b, font + 4.0);
    BUF_LIT(b, "\" rx=\"2\" fill=\"#fff\"/><text text-anchor=\"middle\" y=\"");
    push_f64(b, round(0.3 * font));
    BUF_LIT(b, "\" font-size=\"");
    push_f64(b, font);
    BUF_LIT(b, "\">");
    push_esc(b, text, n);
    BUF_LIT(b, "</text></g>");
}

static int split_edge(Span edge, uint32_t *from, uint32_t *to, Span *shape, Span *label) {
    size_t n = edge.n;
    size_t i = 0;
    uint32_t cp;
    while (i < n) {
        size_t k = i;
        utf8_next(edge.p, n, &k, &cp);
        if (!cp_is_ws(cp)) break;
        i = k;
    }
    if (i >= n) return 0;
    size_t head = i;
    uint32_t first = 0, last = 0;
    size_t first_n = 0, last_n = 0, chars = 0;
    size_t j = i;
    while (j < n) {
        size_t k = j;
        utf8_next(edge.p, n, &k, &cp);
        if (cp_is_ws(cp)) break;
        if (!chars) {
            first = cp;
            first_n = k - j;
        }
        last = cp;
        last_n = k - j;
        chars++;
        j = k;
    }
    if (!chars || j - head < first_n + last_n) return 0;
    *from = first;
    *to = last;
    *shape = span_make(edge.p + head + first_n, (j - head) - first_n - last_n);
    size_t ls = j;
    while (ls < n) {
        size_t k = ls;
        utf8_next(edge.p, n, &k, &cp);
        if (!cp_is_ws(cp)) break;
        ls = k;
    }
    *label = span_make(edge.p + ls, n - ls);
    return 1;
}

static const char *BLUE = "fill:none;stroke:#0041c4;stroke-width:1";
static const char *ARROW = "marker-end:url(#arrowhead);stroke:#0041c4;stroke-width:1;fill:none";
static const char *BOTH =
    "marker-end:url(#arrowhead);marker-start:url(#arrowtail);stroke:#0041c4;stroke-width:1;fill:none";

static void arc_shape(Span shape, double x1, double y1, double x2, double y2, int labeled, Buf *d, const char **style,
                      double *lx, double *ly) {
    double dx = x2 - x1, dy = y2 - y1;
    *lx = (x1 + x2) / 2.0;
    *ly = (y1 + y2) / 2.0;
    const char *st = BLUE;
    int kind = 0;
    if (shape.n == 0 || span_eq(shape, "-"))
        kind = 1;
    else if (span_eq(shape, "~"))
        kind = 2;
    else if (span_eq(shape, "-~"))
        kind = 3;
    else if (span_eq(shape, "~-"))
        kind = 4;
    else if (span_eq(shape, "-|"))
        kind = 5;
    else if (span_eq(shape, "|-"))
        kind = 6;
    else if (span_eq(shape, "-|-"))
        kind = 7;
    else if (span_eq(shape, "->"))
        kind = 8;
    else if (span_eq(shape, "~>"))
        kind = 9;
    else if (span_eq(shape, "-~>"))
        kind = 10;
    else if (span_eq(shape, "~->"))
        kind = 11;
    else if (span_eq(shape, "-|>"))
        kind = 12;
    else if (span_eq(shape, "|->"))
        kind = 13;
    else if (span_eq(shape, "-|->"))
        kind = 14;
    else if (span_eq(shape, "<->"))
        kind = 15;
    else if (span_eq(shape, "<~->"))
        kind = 16;
    else if (span_eq(shape, "<|->"))
        kind = 17;
    else if (span_eq(shape, "<~>"))
        kind = 18;
    else if (span_eq(shape, "<-~>"))
        kind = 19;
    else if (span_eq(shape, "<-|>"))
        kind = 20;
    else if (span_eq(shape, "<-|->"))
        kind = 21;
    else if (span_eq(shape, "+"))
        kind = 22;
    if (kind == 8 || kind == 9 || kind == 10 || kind == 11 || kind == 12 || kind == 13 || kind == 14) st = ARROW;
    if (kind >= 15 && kind <= 21) st = BOTH;
    if (kind == 22) st = "marker-end:url(#tee);marker-start:url(#tee);fill:none;stroke:#0041c4;stroke-width:1";
    if (kind == 0) st = "fill:none;stroke:#F00;stroke-width:1";
    if (labeled) {
        if (kind == 3 || kind == 10 || kind == 19) *lx = x1 + dx * 0.75;
        if (kind == 4 || kind == 11 || kind == 16) *lx = x1 + dx * 0.25;
        if (kind == 5 || kind == 12 || kind == 20) *lx = x2;
        if (kind == 6 || kind == 13 || kind == 17) *lx = x1;
    }
    if (kind == 2 || kind == 9 || kind == 18)
        path_curve(d, x1, y1, 0.7 * dx, 0, 0.3 * dx, dy, dx, dy);
    else if (kind == 3 || kind == 10 || kind == 19)
        path_curve(d, x1, y1, 0.7 * dx, 0, dx, dy, dx, dy);
    else if (kind == 4 || kind == 11 || kind == 16)
        path_curve(d, x1, y1, 0, 0, 0.3 * dx, dy, dx, dy);
    else if (kind == 5 || kind == 12 || kind == 20)
        path_ortho(d, x1, y1, dx, 0, 0, dy);
    else if (kind == 6 || kind == 13 || kind == 17)
        path_ortho(d, x1, y1, 0, dy, dx, 0);
    else if (kind == 7 || kind == 14 || kind == 21)
        path_elbow(d, x1, y1, dx, dy);
    else
        path_line(d, x1, y1, x2, y2);
    *style = st;
}

static void write_arcs(Buf *b, const Doc *doc, Ev *ev, size_t en, double width) {
    typedef struct {
        double x, y;
        size_t text_i;
        uint32_t ch;
    } Lab;
    Lab *labs = NULL;
    size_t ln = 0, lc = 0;
    for (uint32_t ei = 0; ei < doc->edge_n; ei++) {
        uint32_t from, to;
        Span shape, text;
        if (!split_edge(doc->edges[ei], &from, &to, &shape, &text)) continue;
        const Ev *a = NULL, *c = NULL;
        for (size_t i = 0; i < en; i++)
            if (ev[i].ch == from) {
                a = &ev[i];
                break;
            }
        for (size_t i = 0; i < en; i++)
            if (ev[i].ch == to) {
                c = &ev[i];
                break;
            }
        if (!a || !c) continue;
        if ((a->x > c->x ? a->x : c->x) < 0 || (a->x < c->x ? a->x : c->x) > width) continue;
        uint8_t stack[640];
        Buf path;
        buf_init(&path, stack, sizeof stack);
        const char *style;
        double lx, ly;
        arc_shape(shape, a->x, a->y, c->x, c->y, text.n != 0, &path, &style, &lx, &ly);
        BUF_LIT(b, "<path d=\"");
        buf_append(b, path.data, path.len);
        BUF_LIT(b, "\" style=\"");
        buf_puts(b, style);
        BUF_LIT(b, "\"/>");
        buf_free(&path);
        if (text.n) {
            if (ln == lc) {
                size_t nc = lc ? lc * 2 : 4;
                Lab *p = realloc(labs, nc * sizeof(Lab));
                if (!p) tgw_oom();
                labs = p;
                lc = nc;
            }
            labs[ln].x = lx;
            labs[ln].y = ly;
            labs[ln].text_i = ei;
            labs[ln].ch = 0;
            ln++;
        }
    }
    for (size_t i = 0; i < en; i++) {
        if (!is_upper_cp(ev[i].ch) && ev[i].x >= 0 && ev[i].x <= width) {
            char tmp[4];
            size_t n = utf8_encode(ev[i].ch, tmp);
            label_at(b, ev[i].x, ev[i].y, tmp, n, doc->arc_font);
        }
    }
    for (size_t i = 0; i < ln; i++) {
        uint32_t from, to;
        Span shape, text;
        split_edge(doc->edges[labs[i].text_i], &from, &to, &shape, &text);
        double half = tgw_text_width(text.p, text.n, doc->arc_font) / 2.0 + 3.0;
        double x = labs[i].x;
        if (x < half) x = half;
        double lim = width - half;
        if (lim < half) lim = half;
        if (x > lim) x = lim;
        label_at(b, x, labs[i].y, text.p, text.n, doc->arc_font);
    }
    free(labs);
}

static void write_caption(Buf *b, const Cap *cap, int head, double xmax, int64_t gy, int64_t y_tick, int32_t hscale) {
    if (cap->has_text) {
        double y;
        if (head)
            y = y_tick ? -33.0 : -13.0;
        else
            y = y_tick ? (double)gy + 45.0 : (double)gy + 25.0;
        BUF_LIT(b, "<text x=\"");
        push_f64(b, xmax * (double)TGW_XS / 2.0);
        BUF_LIT(b, "\" y=\"");
        push_f64(b, y);
        BUF_LIT(b, "\" text-anchor=\"middle\" font-size=\"14\" font-weight=\"600\" xml:space=\"preserve\">");
        push_esc(b, cap->text.p, cap->text.n);
        BUF_LIT(b, "</text>");
    }
    double mstep = 2.0 * (double)hscale * (double)TGW_XS;
    size_t ticks, tocks;
    tick_counts(xmax, hscale, &ticks, &tocks);
    double y = head ? -5.0 : (double)gy + 15.0;
    write_ticks(b, &cap->tick, 0, mstep, y, ticks, cap->every);
    write_ticks(b, &cap->tock, mstep / 2.0, mstep, y, tocks, cap->every);
}

static void write_groups(Buf *b, const Doc *doc, int64_t yhead) {
    BUF_LIT(b, "<g>");
    for (uint32_t i = 0; i < doc->group_n; i++) {
        const Group *g = &doc->groups[i];
        if (g->height <= 0) continue;
        double x = (double)g->x + 0.5;
        double y = (double)g->y * (double)TGW_YO + 3.5 + (double)yhead;
        double h = (double)g->height * (double)TGW_YO - 16.0;
        BUF_LIT(b, "<path fill=\"none\" stroke=\"#0041c4\" stroke-width=\"1\" d=\"M");
        push_f64(b, x);
        buf_push(b, ',');
        push_f64(b, y);
        BUF_LIT(b, " c -3,0 -5,2 -5,5 l 0,");
        push_f64(b, h);
        BUF_LIT(b, " c 0,3 2,5 5,5\"/>");
        if (g->has_name) {
            double tx = (double)g->x - 10.0;
            double ty = (double)TGW_YO * ((double)g->y + (double)g->height / 2.0) + (double)yhead;
            BUF_LIT(b, "<g transform=\"translate(");
            push_f64(b, tx);
            buf_push(b, ',');
            push_f64(b, ty);
            BUF_LIT(b, ") rotate(270)\"><text text-anchor=\"middle\" fill=\"#475569\" xml:space=\"preserve\"");
            double room = (double)g->height * (double)TGW_YO - 12.0;
            if (room < 1) room = 1;
            if (tgw_text_width(g->name.p, g->name.n, 12) > room) {
                BUF_LIT(b, " textLength=\"");
                push_f64(b, room);
                BUF_LIT(b, "\" lengthAdjust=\"spacingAndGlyphs\"");
            }
            buf_push(b, '>');
            push_esc(b, g->name.p, g->name.n);
            BUF_LIT(b, "</text></g>");
        }
    }
    BUF_LIT(b, "</g>");
}

static void backdrop(Buf *b, double w, int64_t h) {
    BUF_LIT(b, "<rect x=\"");
    push_f64(b, -w / 2.0);
    BUF_LIT(b, "\" width=\"");
    push_f64(b, w);
    BUF_LIT(b, "\" height=\"");
    push_i64(b, h);
    BUF_LIT(b, "\" fill=\"#fff\" fill-opacity=\"0.9\" stroke=\"none\"/>");
}

static void vline(Buf *b, double x, int64_t h) {
    BUF_LIT(b, "<line x1=\"");
    push_f64(b, x);
    BUF_LIT(b, "\" x2=\"");
    push_f64(b, x);
    BUF_LIT(b, "\" y1=\"0\" y2=\"");
    push_i64(b, h);
    BUF_LIT(b, "\" stroke=\"#000\" stroke-width=\"1\"/>");
}

static void draw_gap_string(Buf *b, const char *gaps, size_t n, int64_t nlanes, int32_t hscale, int64_t xmin,
                            double width) {
    if (nlanes <= 0 || width <= 0) return;
    double scale = (double)hscale * (double)TGW_XS * 2.0;
    int64_t height = nlanes * TGW_YO;
    size_t i = 0, index = 0;
    while (i < n) {
        uint32_t cp;
        size_t k = i;
        utf8_next(gaps, n, &k, &cp);
        if (cp_is_ws(cp)) {
            i = k;
            continue;
        }
        size_t start = i;
        i = k;
        while (i < n) {
            k = i;
            utf8_next(gaps, n, &k, &cp);
            if (cp_is_ws(cp)) break;
            i = k;
        }
        const char *tok = gaps + start;
        size_t tn = i - start;
        if (!(tn == 1 && tok[0] == '.')) {
            int lower = 1;
            for (size_t c = 0; c < tn; c++)
                if ((unsigned char)tok[c] >= 'A' && (unsigned char)tok[c] <= 'Z') lower = 0;
            double offset = lower ? 0.5 : 0.0;
            double x = scale * ((double)index + offset) - (double)xmin * (double)TGW_XS;
            if (x >= -6.0) {
                if (x > width + 6.0) break;
                BUF_LIT(b, "<g transform=\"translate(");
                push_f64(b, x);
                BUF_LIT(b, ")\">");
                if ((tn == 1 && (tok[0] == '1' || tok[0] == '|')) ) {
                    backdrop(b, 4, height);
                    vline(b, 0, height);
                } else if (tn == 1 && tok[0] == '2') {
                    backdrop(b, 4, height);
                    vline(b, -2, height);
                    vline(b, 2, height);
                } else if (tn == 1 && tok[0] == '3') {
                    backdrop(b, 6, height);
                    vline(b, -3, height);
                    vline(b, 0, height);
                    vline(b, 3, height);
                } else if (tn == 1 && tok[0] == '[') {
                    backdrop(b, 4, height);
                    BUF_LIT(b, "<path fill=\"none\" stroke=\"#000\" d=\"M2,0 h-4 v");
                    push_i64(b, height - 1);
                    BUF_LIT(b, " h4\"/>");
                } else if (tn == 1 && tok[0] == ']') {
                    backdrop(b, 4, height);
                    BUF_LIT(b, "<path fill=\"none\" stroke=\"#000\" d=\"M-2,0 h4 v");
                    push_i64(b, height - 1);
                    BUF_LIT(b, " h-4\"/>");
                } else if (tn == 1 && tok[0] == '(') {
                    backdrop(b, 4, height);
                    BUF_LIT(b, "<path fill=\"none\" stroke=\"#000\" d=\"M2,0 a4,4 0 0 0 -4,4 v");
                    push_i64(b, height - 9);
                    BUF_LIT(b, " a4,4 0 0 0 4,4\"/>");
                } else if (tn == 1 && tok[0] == ')') {
                    backdrop(b, 4, height);
                    BUF_LIT(b, "<path fill=\"none\" stroke=\"#000\" d=\"M-2,0 a4,4 0 0 1 4,4 v");
                    push_i64(b, height - 9);
                    BUF_LIT(b, " a4,4 0 0 1 -4,4\"/>");
                } else
                    backdrop(b, 4, height);
                BUF_LIT(b, "</g>");
            }
        }
        index++;
    }
}

static uint64_t load_le64(const uint8_t *p) {
    uint64_t x;
    memcpy(&x, p, 8);
#if __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
    x = __builtin_bswap64(x);
#endif
    return x;
}

static void scope_ids(Buf *out) {
    uint64_t lanes[4] = {0xcbf29ce484222325ULL, 0x9e3779b97f4a7c15ULL, 0x517cc1b727220a95ULL, 0x6eed0e9da4d94a4fULL};
    size_t len = out->len;
    size_t n32 = len / 32;
    for (size_t c = 0; c < n32; c++) {
        const uint8_t *chunk = out->data + c * 32;
        for (int k = 0; k < 4; k++)
            lanes[k] = (lanes[k] ^ load_le64(chunk + (size_t)k * 8)) * 0x100000001b3ULL;
    }
    uint64_t hash = (uint64_t)len;
    for (int k = 0; k < 4; k++) hash = (hash ^ lanes[k]) * 0x100000001b3ULL;
    for (size_t i = n32 * 32; i < len; i++) hash = (hash ^ out->data[i]) * 0x100000001b3ULL;
    hash ^= hash >> 32;
    size_t header = len;
    const char *end = "\"</defs>";
    (void)end;
    for (size_t i = 0; i + 7 <= len; i++) {
        if (memcmp(out->data + i, "</defs>", 7) == 0) {
            header = i;
            break;
        }
    }
    size_t *offs = NULL;
    size_t on = 0, oc = 0;
    const char *toks[2] = {" id=\"", " aria-labelledby=\""};
    size_t tl[2] = {5, 18};
    for (int t = 0; t < 2; t++) {
        for (size_t i = 0; i + tl[t] <= header; i++) {
            if (memcmp(out->data + i, toks[t], tl[t]) == 0) {
                if (on == oc) {
                    oc = oc ? oc * 2 : 16;
                    size_t *p = realloc(offs, oc * sizeof(size_t));
                    if (!p) tgw_oom();
                    offs = p;
                }
                offs[on++] = i + tl[t];
                i += tl[t] - 1;
            }
        }
    }
    size_t cursor = 0;
    int in_tag = 0;
    for (size_t i = 0; i + 5 <= len; i++) {
        if (memcmp(out->data + i, "url(#", 5) != 0) continue;
        size_t boundary = (size_t)-1;
        for (size_t j = cursor; j < i; j++)
            if (out->data[j] == '<' || out->data[j] == '>') boundary = j;
        if (boundary != (size_t)-1) in_tag = out->data[boundary] == '<';
        if (in_tag) {
            if (on == oc) {
                oc = oc ? oc * 2 : 16;
                size_t *p = realloc(offs, oc * sizeof(size_t));
                if (!p) tgw_oom();
                offs = p;
            }
            offs[on++] = i + 5;
        }
        cursor = i + 5;
        i += 4;
    }
    for (size_t a = 1; a < on; a++) {
        size_t v = offs[a];
        size_t b = a;
        while (b && offs[b - 1] > v) {
            offs[b] = offs[b - 1];
            b--;
        }
        offs[b] = v;
    }
    char prefix[32];
    int plen = snprintf(prefix, sizeof prefix, "tgw-%016llx-", (unsigned long long)hash);
    size_t old = out->len;
    buf_reserve(out, (size_t)plen * on);
    out->len = old + (size_t)plen * on;
    size_t endp = old;
    for (size_t index = on; index-- > 0;) {
        size_t start = offs[index];
        size_t shift = (index + 1) * (size_t)plen;
        memmove(out->data + start + shift, out->data + start, endp - start);
        memcpy(out->data + start + index * (size_t)plen, prefix, (size_t)plen);
        endp = start;
    }
    free(offs);
}

static size_t tag_end(const uint8_t *src, size_t n, size_t start) {
    if (start + 4 <= n && memcmp(src + start, "<!--", 4) == 0) {
        for (size_t i = start + 4; i + 3 <= n; i++)
            if (memcmp(src + i, "-->", 3) == 0) return i + 3;
        return n;
    }
    if (start + 9 <= n && memcmp(src + start, "<![CDATA[", 9) == 0) {
        for (size_t i = start + 9; i + 3 <= n; i++)
            if (memcmp(src + i, "]]>", 3) == 0) return i + 3;
        return n;
    }
    int quote = 0;
    for (size_t i = start + 1; i < n; i++) {
        uint8_t c = src[i];
        if (quote && c == (uint8_t)quote)
            quote = 0;
        else if (!quote) {
            if (c == '\'' || c == '"')
                quote = c;
            else if (c == '>')
                return i + 1;
        }
    }
    return n;
}

static int name_is_text(const uint8_t *s, size_t n) {
    return (n == 4 && memcmp(s, "text", 4) == 0) || (n == 5 && memcmp(s, "tspan", 5) == 0) ||
           (n == 8 && memcmp(s, "textPath", 8) == 0) || (n == 5 && memcmp(s, "style", 5) == 0) ||
           (n == 5 && memcmp(s, "title", 5) == 0) || (n == 4 && memcmp(s, "desc", 4) == 0);
}

static void prettify(const Buf *src, unsigned ind, Buf *out) {
    size_t depth = 0;
    int text_set = 0;
    size_t text_depth = 0;
    size_t i = 0;
    while (i < src->len) {
        if (src->data[i] == '<') {
            int closing = i + 1 < src->len && src->data[i + 1] == '/';
            int special = i + 1 < src->len && (src->data[i + 1] == '!' || src->data[i + 1] == '?');
            if (closing && depth) depth--;
            if (out->len && !text_set) {
                buf_push(out, '\n');
                size_t spaces = depth * ind;
                for (size_t k = 0; k < spaces; k++) buf_push(out, ' ');
            }
            size_t start = i;
            i = tag_end(src->data, src->len, start);
            int self_close = i >= 2 && src->data[i - 2] == '/';
            buf_append(out, src->data + start, i - start);
            if (closing && text_set && text_depth == depth) text_set = 0;
            if (!closing && !self_close && !special) {
                size_t ns = start + 1;
                size_t ne = i;
                for (size_t k = ns; k < i; k++) {
                    uint8_t c = src->data[k];
                    if (c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '>' || c == '/') {
                        ne = k;
                        break;
                    }
                }
                if (!text_set && name_is_text(src->data + ns, ne - ns)) {
                    text_set = 1;
                    text_depth = depth;
                }
                depth++;
            }
        } else {
            size_t start = i;
            while (i < src->len && src->data[i] != '<') i++;
            buf_append(out, src->data + start, i - start);
        }
    }
    buf_push(out, '\n');
}

int tgw_emit(const Doc *doc, Buf *out, unsigned indent, Arena *arena, TgwError *err) {
    uint32_t nlanes = doc->lane_n;
    WaveOut *waves = nlanes ? calloc(nlanes, sizeof(WaveOut)) : NULL;
    if (nlanes && !waves) tgw_oom();
    double **origins = nlanes ? calloc(nlanes, sizeof(double *)) : NULL;
    size_t *origin_n = nlanes ? calloc(nlanes, sizeof(size_t)) : NULL;
    if (nlanes && (!origins || !origin_n)) tgw_oom();
    double xmax = 0;
    for (uint32_t i = 0; i < nlanes; i++) {
        const Lane *lane = &doc->lanes[i];
        if (lane->body == 1) {
            if (wave_compile(arena, &waves[i], lane->wave.p, lane->wave.n, lane->period, doc->hscale,
                             lane->phase * 2.0 + (double)doc->xmin, (double)((doc->xmax_cfg - doc->xmin) > 0
                                                                                  ? doc->xmax_cfg - doc->xmin
                                                                                  : 0),
                             err)) {
                free(waves);
                free(origins);
                free(origin_n);
                return -1;
            }
            double vis = (double)wave_visible(&waves[i]) + frac_dx(lane->phase, doc->xmin) / (double)TGW_XS;
            if (vis > xmax) xmax = vis;
        } else if (lane->body == 2) {
            double extent;
            if (tgw_extent(lane->wave.p, lane->wave.n, &extent, err)) {
                free(waves);
                free(origins);
                free(origin_n);
                return -1;
            }
            double v = extent * lane->period * (double)doc->hscale * 2.0 - lane->phase * 2.0 - (double)doc->xmin;
            if (v > xmax) xmax = v;
        }
        if (lane->has_node && lane->body == 1) {
            if (wave_nodes(arena, lane->wave.p, lane->wave.n, lane->period, doc->hscale, &origins[i], &origin_n[i],
                           err)) {
                free(waves);
                free(origins);
                free(origin_n);
                return -1;
            }
        }
        if (lane->has_node) {
            size_t count = utf8_count(lane->node.p, lane->node.n);
            double extent;
            if (lane->body == 1) {
                size_t extra = count > origin_n[i] ? count - origin_n[i] : 0;
                extent = (double)waves[i].len + (double)extra * lane->period * (double)doc->hscale * 2.0;
            } else {
                extent = (double)count * lane->period * (double)doc->hscale * 2.0;
            }
            double v = extent - lane->phase * 2.0 - (double)doc->xmin;
            if (v > xmax) xmax = v;
        }
    }
    double limit = (double)((doc->xmax_cfg - doc->xmin) > 0 ? doc->xmax_cfg - doc->xmin : 0);
    if (xmax < 0) xmax = 0;
    if (xmax > limit) xmax = limit;
    for (int pass = 0; pass < 2; pass++) {
        const Cap *cap = pass ? &doc->foot : &doc->head;
        const Tick *ts[2] = {&cap->tick, &cap->tock};
        for (int k = 0; k < 2; k++) {
            if (ts[k]->kind == 1) {
                double v = ts[k]->offset + ts[k]->step * (xmax / (2.0 * (double)doc->hscale) + 1.0);
                if (!isfinite(v)) {
                    free(waves);
                    free(origins);
                    free(origin_n);
                    err_set(err, 0, "tick series exceeds finite range");
                    return -1;
                }
            }
        }
    }
    double max_label = 0;
    for (uint32_t i = 0; i < nlanes; i++) {
        double w = tgw_text_width(doc->lanes[i].name.p, doc->lanes[i].name.n, 12) + (double)doc->lanes[i].indent;
        if (w > max_label) max_label = w;
    }
    double plot = xmax * (double)TGW_XS;
    double xg = ceil((max_label - (double)TGW_TGO + 12.0) / (double)TGW_XS) * (double)TGW_XS;
    int64_t yh0, yh1, yf0, yf1;
    margins(&doc->head, &yh0, &yh1);
    margins(&doc->foot, &yf0, &yf1);
    int64_t gy = (int64_t)nlanes * TGW_YO;
    double caption = 0;
    if (doc->head.has_text) {
        double w = tgw_text_width(doc->head.text.p, doc->head.text.n, 14);
        if (w > caption) caption = w;
    }
    if (doc->foot.has_text) {
        double w = tgw_text_width(doc->foot.text.p, doc->foot.text.n, 14);
        if (w > caption) caption = w;
    }
    double alt = (caption - plot) / 2.0 + 16.0;
    if (alt > xg) xg = alt;
    xg = ceil(xg);
    double tick_room = 0;
    for (int pass = 0; pass < 2; pass++) {
        const Cap *cap = pass ? &doc->foot : &doc->head;
        size_t ticks, tocks;
        tick_counts(xmax, doc->hscale, &ticks, &tocks);
        double w = tick_width(&cap->tick, ticks);
        double w2 = tick_width(&cap->tock, tocks);
        if (w2 > w) w = w2;
        w = w / 2.0 + 8.0;
        if (w > tick_room) tick_room = w;
    }
    if (tick_room > xg) xg = tick_room;
    xg = ceil(xg);
    double right = 20;
    if (alt > right) right = alt;
    if (tick_room > right) right = tick_room;
    double width = xg + plot + right;
    if (width < 40) width = 40;
    int64_t yhead = 12 + yh0 + yh1;
    int64_t height = gy + yhead + yf0 + yf1 + 12;
    if (height < 24) height = 24;

    Paint paint;
    paint_init(&paint);
    Ev *events = NULL;
    size_t en = 0, ec = 0;
    int grid = doc->marks && gy > 0;
    int64_t gstep = 2 * (int64_t)doc->hscale * TGW_XS;
    if (gstep <= 0 || gy <= 0) grid = 0;
    double gwidth = xmax * (double)TGW_XS + 1.0;

    for (uint32_t idx = 0; idx < nlanes; idx++) {
        const Lane *lane = &doc->lanes[idx];
        int64_t y = TGW_Y0 + (int64_t)idx * TGW_YO;
        BUF_LIT(&paint.body, "<g transform=\"translate(0,");
        push_i64(&paint.body, y);
        BUF_LIT(&paint.body, ")\"><text x=\"");
        push_i64(&paint.body, TGW_TGO);
        BUF_LIT(&paint.body, "\" y=\"");
        push_i64(&paint.body, TGW_YM);
        BUF_LIT(&paint.body, "\" text-anchor=\"end\" fill=\"#334155\" font-weight=\"500\" xml:space=\"preserve\">");
        push_esc(&paint.body, lane->name.p, lane->name.n);
        BUF_LIT(&paint.body, "</text><g clip-path=\"url(#");
        if (lane->phase * 2.0 + (double)doc->xmin > 0)
            BUF_LIT(&paint.body, "lane-crop");
        else
            BUF_LIT(&paint.body, "lane-clip");
        BUF_LIT(&paint.body, ")\">");
        double dx = frac_dx(lane->phase, doc->xmin);
        BUF_LIT(&paint.body, "<g transform=\"translate(");
        push_f64(&paint.body, dx);
        BUF_LIT(&paint.body, ")\">");
        if (lane->body == 1) {
            WaveOut *w = &waves[idx];
            double endf = ceil((plot - dx) / (double)TGW_XS);
            if (endf < 0) endf = 0;
            int64_t end = w->skip + (int64_t)endf;
            paint_wave(&paint, w->pats, w->pat_n, w->skip, end);
            size_t from = w->unseen < lane->data.len ? w->unseen : lane->data.len;
            for (uint32_t mi = 0; mi < w->mark_n; mi++) {
                double mx = w->markers[mi];
                if (mx + dx < 0 || mx + dx > plot) continue;
                if (from + mi >= lane->data.len) continue;
                Span label = lane->data.data[from + mi];
                BUF_LIT(&paint.body, "<text x=\"");
                push_f64(&paint.body, mx);
                BUF_LIT(&paint.body, "\" y=\"");
                push_i64(&paint.body, TGW_YM);
                BUF_LIT(&paint.body, "\" text-anchor=\"middle\" xml:space=\"preserve\"");
                double room = w->mwidth[mi] - 10.0;
                if (room < 1) room = 1;
                if (tgw_text_width(label.p, label.n, 12) > room) {
                    BUF_LIT(&paint.body, " textLength=\"");
                    push_f64(&paint.body, room);
                    BUF_LIT(&paint.body, "\" lengthAdjust=\"spacingAndGlyphs\"");
                }
                buf_push(&paint.body, '>');
                push_esc(&paint.body, label.p, label.n);
                BUF_LIT(&paint.body, "</text>");
            }
        }
        BUF_LIT(&paint.body, "</g>");
        if (lane->body == 2) {
            BUF_LIT(&paint.body, "<g transform=\"translate(");
            push_f64(&paint.body, -(lane->phase * 2.0 + (double)doc->xmin) * (double)TGW_XS);
            buf_push(&paint.body, ',');
            push_i64(&paint.body, TGW_YS);
            BUF_LIT(&paint.body, ") scale(");
            push_f64(&paint.body, 2.0 * (double)TGW_XS * lane->period * (double)doc->hscale);
            buf_push(&paint.body, ',');
            push_i64(&paint.body, -TGW_YS);
            BUF_LIT(&paint.body, ")\"><path fill=\"none\" stroke=\"#000\" stroke-width=\"1\" "
                                  "vector-effect=\"non-scaling-stroke\" d=\"");
            push_esc(&paint.body, lane->wave.p, lane->wave.n);
            BUF_LIT(&paint.body, "\"/></g>");
        }
        if (lane->has_over)
            draw_ou(&paint.body, lane->over.p, lane->over.n, 0, lane->period * (double)doc->hscale,
                    lane->phase + (double)doc->xmin / 2.0, plot);
        if (lane->has_under)
            draw_ou(&paint.body, lane->under.p, lane->under.n, 1, lane->period * (double)doc->hscale,
                    lane->phase + (double)doc->xmin / 2.0, plot);
        if (lane->body == 1) {
            for (uint32_t g = 0; g < waves[idx].gap_n; g++) {
                double gx = waves[idx].gaps[g];
                if (gx >= 0 && gx <= plot) draw_gap(&paint.body, gx);
            }
        }
        BUF_LIT(&paint.body, "</g></g>");
        if (lane->has_node) {
            double phase = lane->phase * 2.0 + (double)doc->xmin;
            size_t pos = 0, pi = 0;
            uint32_t cp;
            while (utf8_next(lane->node.p, lane->node.n, &pi, &cp)) {
                if (cp != '.') {
                    double origin;
                    if (pos < origin_n[idx])
                        origin = origins[idx][pos];
                    else if (lane->body == 1) {
                        size_t extra = pos >= origin_n[idx] ? pos - origin_n[idx] : 0;
                        origin = (double)waves[idx].len * (double)TGW_XS +
                                 (double)TGW_XS * 2.0 * (double)extra * lane->period * (double)doc->hscale;
                    } else {
                        origin = (double)TGW_XS * 2.0 * (double)pos * lane->period * (double)doc->hscale;
                    }
                    double x = origin - (double)TGW_XS * phase + (double)TGW_XLABEL;
                    double yy = (double)idx * (double)TGW_YO + (double)TGW_Y0 + (double)TGW_YS * 0.5;
                    if (en == ec) {
                        ec = ec ? ec * 2 : 8;
                        Ev *nw = realloc(events, ec * sizeof(Ev));
                        if (!nw) tgw_oom();
                        events = nw;
                    }
                    events[en].ch = cp;
                    events[en].x = x;
                    events[en].y = yy;
                    en++;
                }
                pos++;
            }
        }
    }
    BUF_LIT(&paint.body, "<g clip-path=\"url(#plot-clip)\">");
    write_arcs(&paint.body, doc, events, en, plot);
    if (doc->has_gaps)
        draw_gap_string(&paint.body, doc->gaps.p, doc->gaps.n, (int64_t)nlanes, doc->hscale, doc->xmin, plot);
    BUF_LIT(&paint.body, "</g>");
    write_caption(&paint.body, &doc->head, 1, xmax, gy, yh0, doc->hscale);
    write_caption(&paint.body, &doc->foot, 0, xmax, gy, yf0, doc->hscale);

    BUF_LIT(out, "<svg xmlns=\"http://www.w3.org/2000/svg\" class=\"tgw\" role=\"img\" "
                 "aria-labelledby=\"diagram-title\" width=\"");
    push_f64(out, width);
    BUF_LIT(out, "\" height=\"");
    push_i64(out, height);
    BUF_LIT(out, "\" viewBox=\"0 0 ");
    push_f64(out, width);
    buf_push(out, ' ');
    push_i64(out, height);
    BUF_LIT(out, "\" overflow=\"hidden\" font-family=\"Inter, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, "
                 "sans-serif\" font-size=\"12\" fill=\"#0f172a\" stroke-linejoin=\"round\"><title id=\"diagram-title\">");
    if (doc->head.has_text)
        push_esc(out, doc->head.text.p, doc->head.text.n);
    else
        BUF_LIT(out, "Signal timing diagram");
    BUF_LIT(out, "</title><desc>");
    push_i64(out, (int64_t)nlanes);
    BUF_LIT(out, " signal lanes");
    for (uint32_t i = 0; i < nlanes; i++) {
        if (doc->lanes[i].name.n) {
            BUF_LIT(out, "; ");
            push_esc(out, doc->lanes[i].name.p, doc->lanes[i].name.n);
        }
    }
    BUF_LIT(out, "</desc><defs>");
    if (doc->edge_n) {
        BUF_LIT(out, "<marker id=\"arrowhead\" viewBox=\"0 -4 11 8\" refX=\"11\" refY=\"0\" markerWidth=\"8\" "
                     "markerHeight=\"6\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,-4 L11,0 L0,4 Z\" "
                     "fill=\"#0041c4\"/></marker><marker id=\"arrowtail\" viewBox=\"-11 -4 11 8\" refX=\"-11\" refY=\"0\" "
                     "markerWidth=\"8\" markerHeight=\"6\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,-4 "
                     "L-11,0 L0,4 Z\" fill=\"#0041c4\"/></marker><marker id=\"tee\" viewBox=\"0 0 2 6\" refX=\"1\" "
                     "refY=\"3\" markerWidth=\"2\" markerHeight=\"6\" orient=\"auto\"><path d=\"M1,0 L1,6\" "
                     "stroke=\"#0041c4\" stroke-width=\"2\"/></marker>");
    }
    if (paint.hatch) {
        BUF_LIT(out, "<pattern id=\"xh\" width=\"6\" height=\"6\" patternUnits=\"userSpaceOnUse\"><rect width=\"6\" "
                     "height=\"6\" fill=\"#f1f5f9\"/><path d=\"M-1,1 L1,-1 M0,6 L6,0 M5,7 L7,5\" fill=\"none\" "
                     "stroke=\"#94a3b8\" stroke-width=\"0.7\"/></pattern>");
    }
    for (uint32_t i = 0; i < paint.clocks; i++)
        write_clock_pattern(out, i, paint.ks0[i], paint.ks1[i], paint.ks2[i], paint.ks3[i], paint.kextra[i]);
    clip_rect(out, "lane-clip", -4, -4, plot + 4.5, (double)TGW_YS + 8);
    clip_rect(out, "lane-crop", 0, -4, plot + 0.5, (double)TGW_YS + 8);
    double left_clip = doc->xmin > 0 ? 0 : -4;
    clip_rect(out, "plot-clip", left_clip, -4, plot - left_clip + 0.5, (double)gy + 8);
    if (grid) {
        BUF_LIT(out, "<pattern id=\"gd\" width=\"");
        push_i64(out, gstep);
        BUF_LIT(out, "\" height=\"");
        push_i64(out, gy);
        BUF_LIT(out, "\" patternUnits=\"userSpaceOnUse\"><path d=\"M0.5,0 V");
        push_i64(out, gy);
        BUF_LIT(out, "\" fill=\"none\" stroke=\"#cbd5e1\" stroke-width=\"1\" stroke-dasharray=\"2,4\"/></pattern>");
    }
    BUF_LIT(out, "</defs><rect width=\"");
    push_f64(out, width);
    BUF_LIT(out, "\" height=\"");
    push_i64(out, height);
    BUF_LIT(out, "\" fill=\"#fff\"/><g transform=\"translate(");
    push_f64(out, xg + 0.5);
    buf_push(out, ',');
    push_f64(out, (double)yhead + 0.5);
    BUF_LIT(out, ")\">");
    if (grid) {
        BUF_LIT(out, "<rect x=\"0\" y=\"0\" width=\"");
        push_f64(out, gwidth);
        BUF_LIT(out, "\" height=\"");
        push_i64(out, gy);
        BUF_LIT(out, "\" fill=\"url(#gd)\" stroke=\"none\"/>");
    }
    buf_append(out, paint.body.data, paint.body.len);
    BUF_LIT(out, "</g>");
    write_groups(out, doc, yhead);
    BUF_LIT(out, "</svg>");
    scope_ids(out);
    if (indent) {
        Buf pretty;
        buf_init(&pretty, NULL, 0);
        pretty.heap = 0;
        prettify(out, indent, &pretty);
        if (out->heap) free(out->data);
        *out = pretty;
        if (out->data && !out->heap) {
            /* prettify always grows from NULL, so heap is set once data exists. */
        }
    }
    paint_free(&paint);
    free(events);
    free(waves);
    free(origins);
    free(origin_n);
    return 0;
}
