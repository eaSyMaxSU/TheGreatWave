#include "common.h"

typedef struct P {
    const uint8_t *s;
    size_t n, i;
    int depth;
    Arena *a;
    TgwError *err;
} P;

typedef struct Node Node;
struct Node {
    uint8_t kind; /* 1 name, 2 lane, 3 group */
    Span name;
    Lane lane;
    Node *kids;
    uint32_t kid_n, kid_cap;
};

static int perr(P *p, const char *m) {
    err_set(p->err, p->i, m);
    return -1;
}

static int peek(P *p) { return p->i < p->n ? p->s[p->i] : -1; }

static int enter(P *p) {
    if (p->depth >= 128) return perr(p, "nesting exceeds 128 levels");
    p->depth++;
    return 0;
}

static int ws(P *p) {
    for (;;) {
        int c = peek(p);
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
            p->i++;
            continue;
        }
        if (c == '/' && p->i + 1 < p->n && p->s[p->i + 1] == '/') {
            p->i += 2;
            while (p->i < p->n) {
                uint8_t ch = p->s[p->i++];
                if (ch == '\n') break;
            }
            continue;
        }
        if (c == '/' && p->i + 1 < p->n && p->s[p->i + 1] == '*') {
            p->i += 2;
            while (p->i + 1 < p->n && !(p->s[p->i] == '*' && p->s[p->i + 1] == '/')) p->i++;
            if (p->i + 1 >= p->n) return perr(p, "unterminated block comment");
            p->i += 2;
            continue;
        }
        break;
    }
    return 0;
}

static int item(P *p, uint8_t end, int *first) {
    if (ws(p)) return -1;
    if (peek(p) == end) {
        p->i++;
        p->depth--;
        return 0;
    }
    if (!*first) {
        if (peek(p) != ',') return perr(p, "expected ',' or closing delimiter");
        p->i++;
        if (ws(p)) return -1;
        if (peek(p) == end) {
            p->i++;
            p->depth--;
            return 0;
        }
    }
    *first = 0;
    return 1;
}

static int hex_escape(P *p, int width, uint32_t *out) {
    if (p->i + (size_t)width > p->n) return perr(p, "bad unicode escape");
    uint32_t code = 0;
    for (int k = 0; k < width; k++) {
        uint8_t c = p->s[p->i + (size_t)k];
        uint32_t d;
        if (c >= '0' && c <= '9')
            d = c - '0';
        else if (c >= 'a' && c <= 'f')
            d = c - 'a' + 10;
        else if (c >= 'A' && c <= 'F')
            d = c - 'A' + 10;
        else
            return perr(p, "bad unicode escape");
        code = code * 16 + d;
    }
    p->i += (size_t)width;
    *out = code;
    return 0;
}

static int parse_string(P *p, Span *out) {
    int q = peek(p);
    if (q != '"' && q != '\'') return perr(p, "expected a string");
    p->i++;
    size_t start = p->i;
    int simple = 1;
    size_t i = start;
    for (; i < p->n; i++) {
        uint8_t c = p->s[i];
        if (c == (uint8_t)q) break;
        if (c == '\\' || c < 0x20) {
            simple = 0;
            break;
        }
    }
    if (simple && i < p->n && p->s[i] == (uint8_t)q) {
        if (!utf8_ok(p->s + start, i - start)) return perr(p, "bad utf-8");
        *out = span_make((const char *)p->s + start, i - start);
        p->i = i + 1;
        return 0;
    }
    p->i = start;
    Buf b;
    uint8_t stack[256];
    buf_init(&b, stack, sizeof stack);
    for (;;) {
        size_t chunk = p->i;
        while (p->i < p->n) {
            uint8_t c = p->s[p->i];
            if (c == (uint8_t)q || c == '\\' || c < 0x20) break;
            p->i++;
        }
        if (p->i > chunk) {
            if (!utf8_ok(p->s + chunk, p->i - chunk)) {
                buf_free(&b);
                return perr(p, "bad utf-8");
            }
            buf_append(&b, p->s + chunk, p->i - chunk);
        }
        if (p->i >= p->n) {
            buf_free(&b);
            return perr(p, "unterminated string");
        }
        uint8_t c = p->s[p->i++];
        if (c == (uint8_t)q) {
            *out = arena_span(p->a, (char *)b.data, b.len);
            buf_free(&b);
            return 0;
        }
        if (c == '\\') {
            if (p->i >= p->n) {
                buf_free(&b);
                return perr(p, "bad escape");
            }
            uint8_t e = p->s[p->i++];
            uint32_t put = e;
            int raw = 1;
            switch (e) {
            case '"':
            case '\'':
            case '\\':
            case '/':
                put = e;
                break;
            case 'b':
                put = 0x08;
                break;
            case 'f':
                put = 0x0c;
                break;
            case 'n':
                put = '\n';
                break;
            case 'r':
                put = '\r';
                break;
            case 't':
                put = '\t';
                break;
            case 'v':
                put = 0x0b;
                break;
            case '0':
                if (peek(p) >= '0' && peek(p) <= '9') {
                    buf_free(&b);
                    return perr(p, "bad escape");
                }
                put = 0;
                break;
            case '\n':
                raw = 0;
                break;
            case '\r':
                raw = 0;
                if (peek(p) == '\n') p->i++;
                break;
            case 'x': {
                uint32_t code;
                if (hex_escape(p, 2, &code)) {
                    buf_free(&b);
                    return -1;
                }
                put = code;
                break;
            }
            case 'u': {
                uint32_t code;
                if (hex_escape(p, 4, &code)) {
                    buf_free(&b);
                    return -1;
                }
                if (code >= 0xd800 && code <= 0xdbff) {
                    if (p->i + 2 > p->n || p->s[p->i] != '\\' || p->s[p->i + 1] != 'u') {
                        buf_free(&b);
                        return perr(p, "expected a low unicode surrogate");
                    }
                    p->i += 2;
                    uint32_t low;
                    if (hex_escape(p, 4, &low)) {
                        buf_free(&b);
                        return -1;
                    }
                    if (low < 0xdc00 || low > 0xdfff) {
                        buf_free(&b);
                        return perr(p, "expected a low unicode surrogate");
                    }
                    code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
                }
                if (code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) {
                    buf_free(&b);
                    return perr(p, "unpaired unicode surrogate");
                }
                put = code;
                break;
            }
            default:
                buf_free(&b);
                return perr(p, "bad escape");
            }
            if (raw) {
                char tmp[4];
                buf_append(&b, tmp, utf8_encode(put, tmp));
            }
        } else {
            buf_free(&b);
            return perr(p, "unescaped control character in string");
        }
    }
}

static int ident(P *p, Span *out) {
    size_t start = p->i++;
    while (p->i < p->n) {
        uint8_t c = p->s[p->i];
        if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '$')
            p->i++;
        else
            break;
    }
    *out = span_make((const char *)p->s + start, p->i - start);
    return 0;
}

static int key(P *p, Span *out) {
    if (ws(p)) return -1;
    int c = peek(p);
    if (c == '"' || c == '\'') return parse_string(p, out);
    if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || c == '$') return ident(p, out);
    return perr(p, "expected a key");
}

static int parse_number(P *p, double *out) {
    if (ws(p)) return -1;
    size_t start = p->i;
    if (peek(p) == '+' || peek(p) == '-') p->i++;
    int digits = 0;
    if (peek(p) == '.') {
        p->i++;
        while (peek(p) >= '0' && peek(p) <= '9') {
            digits = 1;
            p->i++;
        }
    } else {
        while (peek(p) >= '0' && peek(p) <= '9') {
            digits = 1;
            p->i++;
        }
        if (peek(p) == '.') {
            p->i++;
            while (peek(p) >= '0' && peek(p) <= '9') {
                digits = 1;
                p->i++;
            }
        }
    }
    if (peek(p) == 'e' || peek(p) == 'E') {
        p->i++;
        if (peek(p) == '+' || peek(p) == '-') p->i++;
        int exp = 0;
        while (peek(p) >= '0' && peek(p) <= '9') {
            exp = 1;
            p->i++;
        }
        if (!exp) return perr(p, "bad number");
    }
    if (!digits || p->i == start) return perr(p, "expected a number");
    if (!parse_f64_slice((const char *)p->s + start, p->i - start, out)) return perr(p, "bad number");
    if (!isfinite(*out)) return perr(p, "number must be finite");
    return 0;
}

static int parse_bool(P *p, int *out) {
    if (p->i + 4 <= p->n && memcmp(p->s + p->i, "true", 4) == 0) {
        p->i += 4;
        *out = 1;
        return 0;
    }
    if (p->i + 5 <= p->n && memcmp(p->s + p->i, "false", 5) == 0) {
        p->i += 5;
        *out = 0;
        return 0;
    }
    return perr(p, "expected a boolean");
}

static int skip(P *p);

static int skip(P *p) {
    if (ws(p)) return -1;
    int c = peek(p);
    if (c == '{') {
        p->i++;
        if (enter(p)) return -1;
        int first = 1;
        for (;;) {
            int r = item(p, '}', &first);
            if (r < 0) return -1;
            if (r == 0) return 0;
            Span k;
            if (key(p, &k)) return -1;
            if (ws(p)) return -1;
            if (peek(p) != ':') return perr(p, "expected ':'");
            p->i++;
            if (skip(p)) return -1;
        }
    }
    if (c == '[') {
        p->i++;
        if (enter(p)) return -1;
        int first = 1;
        for (;;) {
            int r = item(p, ']', &first);
            if (r < 0) return -1;
            if (r == 0) return 0;
            if (skip(p)) return -1;
        }
    }
    if (c == '"' || c == '\'') {
        Span s;
        return parse_string(p, &s);
    }
    if (c == 't' || c == 'f') {
        int b;
        return parse_bool(p, &b);
    }
    if (c == 'n') {
        if (p->i + 4 <= p->n && memcmp(p->s + p->i, "null", 4) == 0) {
            p->i += 4;
            return 0;
        }
        return perr(p, "expected a value");
    }
    if (c == '-' || c == '+' || c == '.' || (c >= '0' && c <= '9')) {
        double n;
        return parse_number(p, &n);
    }
    return perr(p, "expected a value");
}

static void num_name(Arena *a, double n, Span *out) {
    if (n == trunc(n) && fabs(n) < 1e15 && isfinite(n)) {
        uint8_t stack[32];
        Buf b;
        buf_init(&b, stack, sizeof stack);
        push_i64(&b, (int64_t)n);
        *out = arena_span(a, (char *)b.data, b.len);
        buf_free(&b);
        return;
    }
    uint8_t stack[64];
    Buf b;
    buf_init(&b, stack, sizeof stack);
    fmt_rust_f64(&b, n);
    if (b.len && memchr(b.data, '.', b.len)) {
        while (b.len && b.data[b.len - 1] == '0') b.len--;
        if (b.len && b.data[b.len - 1] == '.') b.len--;
    }
    *out = arena_span(a, (char *)b.data, b.len);
    buf_free(&b);
}

static void split_ws(Arena *a, Span s, SpanVec *out) {
    size_t i = 0;
    while (i < s.n) {
        uint32_t cp;
        size_t k = i;
        utf8_next(s.p, s.n, &k, &cp);
        if (cp_is_ws(cp)) {
            i = k;
            continue;
        }
        size_t start = i;
        i = k;
        while (i < s.n) {
            k = i;
            utf8_next(s.p, s.n, &k, &cp);
            if (cp_is_ws(cp)) break;
            i = k;
        }
        spanvec_push(a, out, span_make(s.p + start, i - start));
    }
}

static int scalar_string(P *p, Span *out);

static int decimal_places(Span s) {
    const char *t = s.p;
    size_t n = s.n, e = n, dot = n;
    for (size_t i = 0; i < n; i++)
        if (t[i] == 'e' || t[i] == 'E') {
            e = i;
            break;
        }
    for (size_t i = 0; i < e; i++)
        if (t[i] == '.') {
            dot = i;
            break;
        }
    int64_t frac = dot < e ? (int64_t)(e - dot - 1) : 0;
    int64_t exp = 0;
    if (e < n && e + 1 < n) {
        char buf[32];
        size_t m = n - (e + 1);
        if (m > 31) m = 31;
        memcpy(buf, t + e + 1, m);
        buf[m] = 0;
        exp = strtoll(buf, NULL, 10);
    } else if (e < n) {
        exp = 0;
    }
    int64_t dp = frac - exp;
    if (dp < 0) dp = 0;
    if (dp > 15) dp = 15;
    return (int)dp;
}

static int tick_from_parts(Arena *a, SpanVec parts, Tick *tick) {
    memset(tick, 0, sizeof *tick);
    if (!parts.len) return 0;
    if (parts.len == 1) {
        double n;
        if (parse_f64_slice(parts.data[0].p, parts.data[0].n, &n) && isfinite(n)) {
            tick->kind = 1;
            tick->offset = n;
            tick->step = 1;
            return 0;
        }
        tick->kind = 2;
        tick->labels = parts;
        return 0;
    }
    if (parts.len == 2) {
        double offset, step;
        if (parse_f64_slice(parts.data[0].p, parts.data[0].n, &offset) &&
            parse_f64_slice(parts.data[1].p, parts.data[1].n, &step) && isfinite(offset) && isfinite(step) &&
            isfinite(offset * step)) {
            tick->kind = 1;
            tick->offset = step * offset;
            tick->step = step;
            tick->dp = (uint8_t)decimal_places(parts.data[1]);
            tick->fixed = 1;
            return 0;
        }
    }
    (void)a;
    tick->kind = 2;
    tick->labels = parts;
    return 0;
}

static int parse_tick(P *p, Tick *tick) {
    if (ws(p)) return -1;
    int c = peek(p);
    if (c == '"' || c == '\'') {
        Span s;
        if (parse_string(p, &s)) return -1;
        SpanVec parts = {0};
        split_ws(p->a, s, &parts);
        return tick_from_parts(p->a, parts, tick);
    }
    if (c == '[') {
        p->i++;
        if (enter(p)) return -1;
        SpanVec parts = {0};
        int first = 1;
        for (;;) {
            int r = item(p, ']', &first);
            if (r < 0) return -1;
            if (!r) break;
            Span s;
            if (scalar_string(p, &s)) return -1;
            spanvec_push(p->a, &parts, s);
        }
        return tick_from_parts(p->a, parts, tick);
    }
    if (c == 't' || c == 'f') {
        int b;
        if (parse_bool(p, &b)) return -1;
        memset(tick, 0, sizeof *tick);
        tick->kind = 1;
        tick->offset = b ? 1 : 0;
        tick->step = 1;
        return 0;
    }
    if (c == 'n') {
        if (skip(p)) return -1;
        memset(tick, 0, sizeof *tick);
        return 0;
    }
    double n;
    if (parse_number(p, &n)) return -1;
    memset(tick, 0, sizeof *tick);
    tick->kind = 1;
    tick->offset = n;
    tick->step = 1;
    return 0;
}

static int scalar_string(P *p, Span *out) {
    if (ws(p)) return -1;
    int c = peek(p);
    if (c == '"' || c == '\'') return parse_string(p, out);
    if (c == 't' || c == 'f') {
        int b;
        if (parse_bool(p, &b)) return -1;
        *out = span_cstr(b ? "true" : "false");
        return 0;
    }
    if (c == 'n') {
        if (skip(p)) return -1;
        *out = span_make("", 0);
        return 0;
    }
    if (c == '-' || c == '+' || c == '.' || (c >= '0' && c <= '9')) {
        double n;
        if (parse_number(p, &n)) return -1;
        num_name(p->a, n, out);
        return 0;
    }
    if (skip(p)) return -1;
    *out = span_make("", 0);
    return 0;
}

static int d_object(P *p, Span *out) {
    p->i++;
    if (enter(p)) return -1;
    Span d = span_make("", 0);
    int first = 1;
    for (;;) {
        int r = item(p, '}', &first);
        if (r < 0) return -1;
        if (!r) {
            *out = d;
            return 0;
        }
        Span k;
        if (key(p, &k)) return -1;
        if (ws(p)) return -1;
        if (peek(p) != ':') return perr(p, "expected ':'");
        p->i++;
        if (span_eq(k, "d")) {
            if (ws(p)) return -1;
            if (peek(p) == '"' || peek(p) == '\'') {
                if (parse_string(p, &d)) return -1;
            } else if (skip(p))
                return -1;
        } else if (skip(p))
            return -1;
    }
}

static int parse_body(P *p, Lane *lane) {
    if (ws(p)) return -1;
    int c = peek(p);
    if (c == '"' || c == '\'') {
        if (parse_string(p, &lane->wave)) return -1;
        lane->body = 1;
        return 0;
    }
    if (c == '[') {
        p->i++;
        if (enter(p)) return -1;
        Span kind = span_make("", 0);
        Span path = span_make("", 0);
        int have_path = 0, seen = 0;
        int first = 1;
        for (;;) {
            int r = item(p, ']', &first);
            if (r < 0) return -1;
            if (!r) break;
            if (ws(p)) return -1;
            if (!seen && (peek(p) == '"' || peek(p) == '\'')) {
                if (parse_string(p, &kind)) return -1;
                seen = 1;
            } else if (!have_path && peek(p) == '{') {
                if (d_object(p, &path)) return -1;
                have_path = 1;
                seen = 1;
            } else {
                if (skip(p)) return -1;
                seen = 1;
            }
        }
        if (span_eq(kind, "pw")) {
            lane->body = 2;
            lane->wave = path;
        }
        return 0;
    }
    if (skip(p)) return -1;
    return 0;
}

static int parse_data(P *p, Lane *lane) {
    if (ws(p)) return -1;
    int c = peek(p);
    if (c == '"' || c == '\'') {
        Span s;
        if (parse_string(p, &s)) return -1;
        split_ws(p->a, s, &lane->data);
        return 0;
    }
    if (c == '[') {
        p->i++;
        if (enter(p)) return -1;
        int first = 1;
        for (;;) {
            int r = item(p, ']', &first);
            if (r < 0) return -1;
            if (!r) return 0;
            Span s;
            if (scalar_string(p, &s)) return -1;
            spanvec_push(p->a, &lane->data, s);
        }
    }
    if (skip(p)) return -1;
    return 0;
}

static int parse_lane(P *p, Lane *lane) {
    p->i++;
    *lane = lane_blank(span_cstr(" "), 0);
    int named = 0;
    if (enter(p)) return -1;
    int first = 1;
    for (;;) {
        int r = item(p, '}', &first);
        if (r < 0) return -1;
        if (!r) break;
        Span k;
        if (key(p, &k)) return -1;
        if (ws(p)) return -1;
        if (peek(p) != ':') return perr(p, "expected ':'");
        p->i++;
        if (span_eq(k, "name")) {
            if (scalar_string(p, &lane->name)) return -1;
            named = 1;
        } else if (span_eq(k, "wave")) {
            if (parse_body(p, lane)) return -1;
        } else if (span_eq(k, "data")) {
            if (parse_data(p, lane)) return -1;
        } else if (span_eq(k, "period")) {
            double n;
            if (parse_number(p, &n)) return -1;
            if (n < 0) return perr(p, "period must be positive");
            if (n > 1e15) return perr(p, "period is out of range");
            lane->period = n == 0 ? 1 : n;
        } else if (span_eq(k, "phase")) {
            if (parse_number(p, &lane->phase)) return -1;
            if (fabs(lane->phase) > 1e15) return perr(p, "phase is out of range");
        } else if (span_eq(k, "node")) {
            if (scalar_string(p, &lane->node)) return -1;
            lane->has_node = 1;
        } else if (span_eq(k, "over")) {
            if (scalar_string(p, &lane->over)) return -1;
            lane->has_over = 1;
        } else if (span_eq(k, "under")) {
            if (scalar_string(p, &lane->under)) return -1;
            lane->has_under = 1;
        } else if (skip(p))
            return -1;
    }
    if (!named || !lane->name.n) lane->name = span_cstr(" ");
    return 0;
}

static void node_push(Arena *a, Node **kids, uint32_t *n, uint32_t *cap, Node node) {
    if (*n == *cap) {
        uint32_t c = *cap ? *cap * 2 : 4;
        Node *p = arena_alloc(a, sizeof(Node) * c);
        if (*n) memcpy(p, *kids, sizeof(Node) * *n);
        *kids = p;
        *cap = c;
    }
    (*kids)[(*n)++] = node;
}

static int parse_nodes(P *p, Node **out, uint32_t *out_n);

static int parse_node(P *p, Node *node) {
    if (ws(p)) return -1;
    memset(node, 0, sizeof *node);
    int c = peek(p);
    if (c == '[') {
        node->kind = 3;
        return parse_nodes(p, &node->kids, &node->kid_n);
    }
    if (c == '{') {
        node->kind = 2;
        return parse_lane(p, &node->lane);
    }
    if (c == '"' || c == '\'') {
        node->kind = 1;
        return parse_string(p, &node->name);
    }
    if (c == '-' || c == '+' || c == '.' || (c >= '0' && c <= '9')) {
        double n;
        if (parse_number(p, &n)) return -1;
        node->kind = 1;
        num_name(p->a, n, &node->name);
        return 0;
    }
    if (c == 'n') {
        if (skip(p)) return -1;
        node->kind = 2;
        node->lane = lane_blank(span_cstr(" "), 0);
        return 0;
    }
    return perr(p, "expected a signal");
}

static int parse_nodes(P *p, Node **out, uint32_t *out_n) {
    p->i++;
    if (enter(p)) return -1;
    Node *kids = NULL;
    uint32_t n = 0, cap = 0;
    int first = 1;
    for (;;) {
        int r = item(p, ']', &first);
        if (r < 0) return -1;
        if (!r) {
            *out = kids;
            *out_n = n;
            return 0;
        }
        Node node;
        if (parse_node(p, &node)) return -1;
        node_push(p->a, &kids, &n, &cap, node);
    }
}

typedef struct Walk {
    int64_t x, y, xx;
    int has_name;
    Span name;
    Doc *doc;
    Arena *a;
} Walk;

static void walk(Node *nodes, uint32_t n, Walk *st) {
    int64_t delta = 10;
    int has = 0;
    Span name = {0};
    if (n && nodes[0].kind == 1) {
        name = nodes[0].name;
        has = 1;
        delta = 25;
    }
    st->x += delta;
    for (uint32_t i = 0; i < n; i++) {
        if (nodes[i].kind == 3) {
            int64_t old = st->y;
            walk(nodes[i].kids, nodes[i].kid_n, st);
            Group g;
            memset(&g, 0, sizeof g);
            g.x = st->xx;
            g.y = old;
            g.height = st->y - old;
            g.has_name = st->has_name;
            g.name = st->name;
            doc_add_group(st->a, st->doc, g);
        } else if (nodes[i].kind == 2) {
            nodes[i].lane.indent = st->x;
            doc_add_lane(st->a, st->doc, nodes[i].lane);
            st->y++;
        }
    }
    st->xx = st->x;
    st->x -= delta;
    st->has_name = has;
    st->name = name;
}

static int parse_cap(P *p, Cap *cap) {
    if (ws(p)) return -1;
    if (peek(p) != '{') {
        if (skip(p)) return -1;
        return 0;
    }
    p->i++;
    if (enter(p)) return -1;
    int first = 1;
    for (;;) {
        int r = item(p, '}', &first);
        if (r < 0) return -1;
        if (!r) return 0;
        Span k;
        if (key(p, &k)) return -1;
        if (ws(p)) return -1;
        if (peek(p) != ':') return perr(p, "expected ':'");
        p->i++;
        if (span_eq(k, "text")) {
            if (scalar_string(p, &cap->text)) return -1;
            cap->has_text = 1;
        } else if (span_eq(k, "tick")) {
            if (parse_tick(p, &cap->tick)) return -1;
        } else if (span_eq(k, "tock")) {
            if (parse_tick(p, &cap->tock)) return -1;
        } else if (span_eq(k, "every")) {
            if (parse_number(p, &cap->every)) return -1;
        } else if (skip(p))
            return -1;
    }
}

static int num_pair(P *p, int *ok, double *a, double *b) {
    if (ws(p)) return -1;
    *ok = 0;
    if (peek(p) != '[') return skip(p);
    p->i++;
    if (enter(p)) return -1;
    double nums[2];
    int count = 0;
    int first = 1;
    for (;;) {
        int r = item(p, ']', &first);
        if (r < 0) return -1;
        if (!r) break;
        if (ws(p)) return -1;
        int c = peek(p);
        if (c == '-' || c == '+' || c == '.' || (c >= '0' && c <= '9')) {
            double n;
            if (parse_number(p, &n)) return -1;
            if (count < 2) nums[count++] = n;
        } else if (skip(p))
            return -1;
    }
    if (count == 2) {
        *ok = 1;
        *a = nums[0];
        *b = nums[1];
    }
    return 0;
}

static int parse_config(P *p, Doc *doc) {
    if (ws(p)) return -1;
    if (peek(p) != '{') return skip(p);
    p->i++;
    if (enter(p)) return -1;
    int first = 1;
    for (;;) {
        int r = item(p, '}', &first);
        if (r < 0) return -1;
        if (!r) return 0;
        Span k;
        if (key(p, &k)) return -1;
        if (ws(p)) return -1;
        if (peek(p) != ':') return perr(p, "expected ':'");
        p->i++;
        if (span_eq(k, "hscale")) {
            double n;
            if (parse_number(p, &n)) return -1;
            int h = n > 0 ? (int)round(n) : 1;
            if (h > 100) h = 100;
            if (h > 0) doc->hscale = h;
        } else if (span_eq(k, "hbounds")) {
            int ok;
            double a, b;
            if (num_pair(p, &ok, &a, &b)) return -1;
            if (ok) {
                if (fabs(a) > 1e15 || fabs(b) > 1e15) return perr(p, "horizontal bounds are out of range");
                double lo = floor(a), hi = ceil(b);
                if (lo < hi) {
                    doc->xmin = 2 * (int64_t)lo;
                    doc->xmax_cfg = 2 * (int64_t)floor(hi);
                }
            }
        } else if (span_eq(k, "marks")) {
            if (ws(p)) return -1;
            if (peek(p) == 't' || peek(p) == 'f') {
                int b;
                if (parse_bool(p, &b)) return -1;
                doc->marks = b;
            } else if (skip(p))
                return -1;
        } else if (span_eq(k, "arcFontSize")) {
            if (parse_number(p, &doc->arc_font)) return -1;
            if (doc->arc_font <= 0 || doc->arc_font > 1e6)
                return perr(p, "arc font size must be between 0 and 1000000");
        } else if (skip(p))
            return -1;
    }
}

static int string_array(P *p, Doc *doc) {
    if (ws(p)) return -1;
    if (peek(p) != '[') return perr(p, "expected an array");
    p->i++;
    if (enter(p)) return -1;
    int first = 1;
    for (;;) {
        int r = item(p, ']', &first);
        if (r < 0) return -1;
        if (!r) return 0;
        if (ws(p)) return -1;
        if (peek(p) == '"' || peek(p) == '\'') {
            Span s;
            if (parse_string(p, &s)) return -1;
            doc_add_edge(p->a, doc, s);
        } else if (skip(p))
            return -1;
    }
}

static void shift_tick(Tick *tick, double dx) {
    if (dx == 0) return;
    if (tick->kind == 1 && fabs(tick->step - 1.0) < 1e-9 && tick->dp == 0) tick->offset += dx;
}

int tgw_parse_json5(const char *src, size_t len, Doc *doc, Arena *arena, TgwError *err) {
    P p = {(const uint8_t *)src, len, 0, 0, arena, err};
    if (ws(&p)) return -1;
    if (peek(&p) != '{') return perr(&p, "expected an object");
    p.i++;
    if (enter(&p)) return -1;
    int have_signal = 0;
    Node *signal = NULL;
    uint32_t signal_n = 0;
    int first = 1;
    for (;;) {
        int r = item(&p, '}', &first);
        if (r < 0) return -1;
        if (!r) break;
        Span k;
        if (key(&p, &k)) return -1;
        if (ws(&p)) return -1;
        if (peek(&p) != ':') return perr(&p, "expected ':'");
        p.i++;
        if (span_eq(k, "signal")) {
            if (ws(&p)) return -1;
            if (peek(&p) != '[') return perr(&p, "signal must be an array");
            if (parse_nodes(&p, &signal, &signal_n)) return -1;
            have_signal = 1;
        } else if (span_eq(k, "edge")) {
            if (string_array(&p, doc)) return -1;
        } else if (span_eq(k, "head")) {
            if (parse_cap(&p, &doc->head)) return -1;
        } else if (span_eq(k, "foot")) {
            if (parse_cap(&p, &doc->foot)) return -1;
        } else if (span_eq(k, "config")) {
            if (parse_config(&p, doc)) return -1;
        } else if (span_eq(k, "gaps")) {
            if (ws(&p)) return -1;
            if (peek(&p) == '"' || peek(&p) == '\'') {
                if (parse_string(&p, &doc->gaps)) return -1;
                doc->has_gaps = 1;
            } else if (skip(&p))
                return -1;
        } else if (skip(&p))
            return -1;
    }
    if (ws(&p)) return -1;
    if (p.i < p.n) return perr(&p, "trailing input");
    if (!have_signal) {
        err_set(err, 0, "signal array is required");
        return -1;
    }
    Walk st = {0, 0, 0, 0, {0}, doc, arena};
    walk(signal, signal_n, &st);
    double dx = (double)doc->xmin / 2.0;
    shift_tick(&doc->head.tick, dx);
    shift_tick(&doc->foot.tick, dx);
    shift_tick(&doc->head.tock, dx);
    shift_tick(&doc->foot.tock, dx);
    return 0;
}
