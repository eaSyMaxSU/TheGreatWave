#include "common.h"

static void indent_at(Buf *b, size_t depth) {
    for (size_t i = 0; i < depth; i++) BUF_LIT(b, "  ");
}

static void quoted(Buf *b, const char *s, size_t n) {
    buf_push(b, '"');
    size_t i = 0;
    uint32_t cp;
    while (i < n) {
        size_t at = i;
        utf8_next(s, n, &i, &cp);
        if (cp == '"')
            BUF_LIT(b, "\\\"");
        else if (cp == '\\')
            BUF_LIT(b, "\\\\");
        else if (cp == '\n')
            BUF_LIT(b, "\\n");
        else if (cp == '\r')
            BUF_LIT(b, "\\r");
        else if (cp == '\t')
            BUF_LIT(b, "\\t");
        else if (cp < 0x20) {
            char tmp[16];
            snprintf(tmp, sizeof tmp, "\\u%04x", cp);
            buf_puts(b, tmp);
        } else
            buf_append(b, s + at, i - at);
    }
    buf_push(b, '"');
}

static int needs_quotes(const char *s, size_t n, int token) {
    if (!n) return 1;
    size_t a = 0, c = n;
    uint32_t cp;
    size_t i = 0;
    while (i < n) {
        size_t k = i;
        utf8_next(s, n, &k, &cp);
        if (!cp_is_ws(cp)) break;
        i = k;
    }
    a = i;
    while (c) {
        size_t j = c;
        while (j && ((uint8_t)s[j - 1] & 0xc0) == 0x80) j--;
        if (!j) break;
        size_t k = j - 1;
        uint32_t c2;
        size_t t = k;
        utf8_next(s, c, &t, &c2);
        if (!cp_is_ws(c2)) break;
        c = k;
    }
    if (a != 0 || c != n) return 1;
    if (s[0] == '@') return 1;
    for (size_t k = 0; k + 1 < n; k++)
        if (s[k] == '=' && s[k + 1] == '>') return 1;
    i = 0;
    while (i < n) {
        utf8_next(s, n, &i, &cp);
        if (cp < 0x20 || cp == 0x7f || cp == '#' || cp == '\'' || cp == '"' || cp == '\\' || cp == ';' || cp == ':')
            return 1;
        if (token && cp_is_ws(cp)) return 1;
    }
    return 0;
}

static void value(Buf *b, const char *s, size_t n, int token) {
    if (needs_quotes(s, n, token))
        quoted(b, s, n);
    else
        buf_append(b, s, n);
}

static void directive(Buf *b, const char *name, const char *text, size_t n) {
    buf_push(b, '@');
    buf_puts(b, name);
    buf_push(b, ' ');
    value(b, text, n, 0);
    buf_push(b, '\n');
}

static void ticks(Buf *b, const char *name, const Tick *tick, int64_t xmin) {
    if (tick->kind == 0) return;
    if (tick->kind == 2) {
        buf_push(b, '@');
        buf_puts(b, name);
        for (uint32_t i = 0; i < tick->labels.len; i++) {
            buf_push(b, ' ');
            quoted(b, tick->labels.data[i].p, tick->labels.data[i].n);
        }
        buf_push(b, '\n');
        return;
    }
    double start = tick->offset;
    if (fabs(tick->step - 1.0) < 1e-9 && tick->dp == 0) start = tick->offset - (double)xmin / 2.0;
    buf_push(b, '@');
    buf_puts(b, name);
    buf_push(b, ' ');
    fmt_rust_f64(b, start);
    if (tick->fixed || tick->step != 1.0 || tick->dp) {
        buf_push(b, ' ');
        uint8_t stack[64];
        Buf tmp;
        buf_init(&tmp, stack, sizeof stack);
        fmt_fixed(&tmp, tick->step, tick->dp);
        double back;
        if (parse_f64_slice((char *)tmp.data, tmp.len, &back) && back == tick->step)
            buf_append(b, tmp.data, tmp.len);
        else
            fmt_rust_f64(b, tick->step);
        buf_free(&tmp);
    }
    buf_push(b, '\n');
}

static void caption(Buf *b, const Cap *cap, int foot, int64_t xmin) {
    if (cap->has_text) directive(b, foot ? "footer" : "title", cap->text.p, cap->text.n);
    ticks(b, foot ? "foot-tick" : "tick", &cap->tick, xmin);
    ticks(b, foot ? "foot-tock" : "tock", &cap->tock, xmin);
    if (cap->every != 0) {
        buf_push(b, '@');
        buf_puts(b, foot ? "foot-every" : "every");
        buf_push(b, ' ');
        fmt_rust_f64(b, cap->every);
        buf_push(b, '\n');
    }
}

static int lane_line(Buf *b, const Lane *lane, size_t *at, size_t *cols) {
    int spacer = lane->name.n == 1 && lane->name.p[0] == ' ' && lane->body == 0 && !lane->data.len &&
                 lane->period == 1 && lane->phase == 0 && !lane->has_node && !lane->has_over && !lane->has_under;
    if (spacer) {
        BUF_LIT(b, "---\n");
        return 0;
    }
    size_t name_start = b->len;
    value(b, lane->name.p, lane->name.n, 0);
    buf_push(b, ':');
    int keep = lane->body != 0 || lane->data.len;
    if (keep) {
        *at = b->len;
        *cols = utf8_count((char *)b->data + name_start, b->len - name_start);
    }
    if (lane->body == 1) {
        buf_push(b, ' ');
        value(b, lane->wave.p, lane->wave.n, 0);
    } else if (lane->body == 2) {
        BUF_LIT(b, " path ");
        value(b, lane->wave.p, lane->wave.n, 0);
    }
    if (lane->data.len) {
        BUF_LIT(b, " =>");
        for (uint32_t i = 0; i < lane->data.len; i++) {
            buf_push(b, ' ');
            value(b, lane->data.data[i].p, lane->data.data[i].n, 1);
        }
    }
    if (lane->period != 1) {
        BUF_LIT(b, " ; period=");
        fmt_rust_f64(b, lane->period);
    }
    if (lane->phase != 0) {
        BUF_LIT(b, " ; phase=");
        fmt_rust_f64(b, lane->phase);
    }
    if (lane->has_node) {
        BUF_LIT(b, " ; node=");
        value(b, lane->node.p, lane->node.n, 0);
    }
    if (lane->has_over) {
        BUF_LIT(b, " ; over=");
        value(b, lane->over.p, lane->over.n, 0);
    }
    if (lane->has_under) {
        BUF_LIT(b, " ; under=");
        value(b, lane->under.p, lane->under.n, 0);
    }
    buf_push(b, '\n');
    return keep;
}

void tgw_write_tgw(const Doc *doc, Buf *out) {
    caption(out, &doc->head, 0, doc->xmin);
    caption(out, &doc->foot, 1, doc->xmin);
    if (doc->hscale != 1) {
        BUF_LIT(out, "@scale ");
        push_i64(out, doc->hscale);
        buf_push(out, '\n');
    }
    if (doc->xmin != 0 || doc->xmax_cfg != 1000000000000LL) {
        BUF_LIT(out, "@bounds ");
        push_i64(out, doc->xmin / 2);
        buf_push(out, ' ');
        push_i64(out, doc->xmax_cfg / 2);
        buf_push(out, '\n');
    }
    if (!doc->marks) BUF_LIT(out, "@grid off\n");
    if (doc->arc_font != 11.0) {
        BUF_LIT(out, "@arc-font ");
        fmt_rust_f64(out, doc->arc_font);
        buf_push(out, '\n');
    }
    if (doc->has_gaps) directive(out, "gaps", doc->gaps.p, doc->gaps.n);
    if (out->len && (doc->lane_n || doc->group_n)) buf_push(out, '\n');

    uint32_t ng = doc->group_n;
    uint32_t *order = ng ? calloc(ng, sizeof(uint32_t)) : NULL;
    if (ng && !order) tgw_oom();
    for (uint32_t i = 0; i < ng; i++) order[i] = i;
    for (uint32_t i = 1; i < ng; i++) {
        uint32_t v = order[i];
        uint32_t j = i;
        while (j) {
            const Group *a = &doc->groups[order[j - 1]];
            const Group *b = &doc->groups[v];
            int less = 0;
            if (a->y > b->y)
                less = 1;
            else if (a->y == b->y && a->x > b->x)
                less = 1;
            else if (a->y == b->y && a->x == b->x && a->height < b->height)
                less = 1;
            if (!less) break;
            order[j] = order[j - 1];
            j--;
        }
        order[j] = v;
    }
    size_t *ats = doc->lane_n ? calloc(doc->lane_n, sizeof(size_t)) : NULL;
    size_t *cols = doc->lane_n ? calloc(doc->lane_n, sizeof(size_t)) : NULL;
    uint8_t *keep = doc->lane_n ? calloc(doc->lane_n, 1) : NULL;
    if (doc->lane_n && (!ats || !cols || !keep)) tgw_oom();
    uint32_t next = 0;
    uint32_t open_n = 0;
    uint32_t *open = ng ? calloc(ng, sizeof(uint32_t)) : NULL;
    if (ng && !open) tgw_oom();
    uint32_t kept = 0;
    for (uint32_t row = 0; row <= doc->lane_n; row++) {
        while (open_n && doc->groups[open[open_n - 1]].y + doc->groups[open[open_n - 1]].height <= (int64_t)row) {
            open_n--;
            indent_at(out, open_n);
            BUF_LIT(out, "@end\n");
        }
        while (next < ng && doc->groups[order[next]].y <= (int64_t)row) {
            const Group *g = &doc->groups[order[next++]];
            indent_at(out, open_n);
            BUF_LIT(out, "@group");
            if (g->has_name) {
                buf_push(out, ' ');
                value(out, g->name.p, g->name.n, 0);
            }
            buf_push(out, '\n');
            if (g->height <= 0) {
                indent_at(out, open_n);
                BUF_LIT(out, "@end\n");
            } else if (open_n < ng) {
                open[open_n++] = order[next - 1];
            }
        }
        if (row < doc->lane_n) {
            indent_at(out, open_n);
            size_t at = 0, col = 0;
            if (lane_line(out, &doc->lanes[row], &at, &col)) {
                ats[kept] = at;
                cols[kept] = col + open_n * 2;
                keep[kept] = 1;
                kept++;
            }
        }
    }
    while (open_n) {
        open_n--;
        indent_at(out, open_n);
        BUF_LIT(out, "@end\n");
    }
    if (!doc->lane_n) BUF_LIT(out, "@empty\n");
    if (doc->edge_n) {
        if (out->len) buf_push(out, '\n');
        for (uint32_t i = 0; i < doc->edge_n; i++) directive(out, "edge", doc->edges[i].p, doc->edges[i].n);
    }
    size_t width = 0;
    for (uint32_t i = 0; i < kept; i++)
        if (cols[i] > width) width = cols[i];
    if (width > 40) width = 40;
    if (kept) {
        Buf aligned;
        buf_init(&aligned, NULL, 0);
        size_t previous = 0;
        for (uint32_t i = 0; i < kept; i++) {
            buf_append(&aligned, out->data + previous, ats[i] - previous);
            size_t pad = width > cols[i] ? width - cols[i] : 0;
            for (size_t k = 0; k < pad; k++) buf_push(&aligned, ' ');
            previous = ats[i];
        }
        buf_append(&aligned, out->data + previous, out->len - previous);
        if (out->heap) free(out->data);
        *out = aligned;
    }
    free(order);
    free(ats);
    free(cols);
    free(keep);
    free(open);
}
