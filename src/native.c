#include "common.h"

static size_t trim_start(const char *s, size_t n) {
    size_t i = 0;
    uint32_t cp;
    size_t j = 0;
    while (j < n) {
        size_t k = j;
        if (!utf8_next(s, n, &k, &cp) || !cp_is_ws(cp)) break;
        i = k;
        j = k;
    }
    return i;
}

static size_t trim_end(const char *s, size_t n) {
    while (n) {
        size_t i = n;
        while (i && ((uint8_t)s[i - 1] & 0xc0) == 0x80) i--;
        if (!i) break;
        i--;
        uint32_t cp;
        size_t k = i;
        utf8_next(s, n, &k, &cp);
        if (!cp_is_ws(cp)) break;
        n = i;
    }
    return n;
}

static int ascii_unquoted(const char *s, size_t n) {
    return !memchr(s, '\'', n) && !memchr(s, '"', n);
}

static int find_outside(const char *s, size_t n, const char *needle, size_t nn, size_t *out) {
    int colon = nn == 1 && needle[0] == ':';
    if (nn && ascii_unquoted(s, n)) {
        if (nn == 1 && !colon) {
            const void *hit = memchr(s, needle[0], n);
            if (!hit) return 0;
            *out = (size_t)((const char *)hit - s);
            return 1;
        }
        if (nn == 2) {
            unsigned char a = (unsigned char)needle[0];
            size_t i = 0;
            while (i + 1 < n) {
                const void *hit = memchr(s + i, a, n - i);
                if (!hit) return 0;
                i = (size_t)((const char *)hit - s);
                if ((unsigned char)s[i + 1] == (unsigned char)needle[1]) {
                    *out = i;
                    return 1;
                }
                i++;
            }
            return 0;
        }
        if (colon) {
            size_t i = 0;
            while (i < n) {
                const void *hit = memchr(s + i, ':', n - i);
                if (!hit) return 0;
                i = (size_t)((const char *)hit - s);
                if (!memchr(s, '[', i)) {
                    *out = i;
                    return 1;
                }
                break;
            }
        }
    }
    int quote = 0;
    int escaped = 0;
    uint32_t brackets = 0;
    size_t i = 0;
    uint32_t cp;
    while (i < n) {
        size_t at = i;
        utf8_next(s, n, &i, &cp);
        if (escaped) {
            escaped = 0;
            continue;
        }
        if (quote && cp == '\\') {
            escaped = 1;
            continue;
        }
        if (quote && cp == (uint32_t)quote) {
            quote = 0;
        } else if (!quote) {
            if (cp == '\'' || cp == '"')
                quote = (int)cp;
            else if (nn == 1 && needle[0] == ':' && cp == '[')
                brackets++;
            else if (nn == 1 && needle[0] == ':' && cp == ']') {
                if (brackets) brackets--;
            } else if (brackets == 0 && at + nn <= n && memcmp(s + at, needle, nn) == 0) {
                *out = at;
                return 1;
            }
        }
    }
    return 0;
}

static int strip_comment(const char *s, size_t n, size_t at, size_t *out_n, TgwError *err) {
    if (ascii_unquoted(s, n)) {
        const void *hash = memchr(s, '#', n);
        *out_n = hash ? (size_t)((const char *)hash - s) : n;
        return 0;
    }
    int quote = 0, escaped = 0;
    size_t i = 0;
    uint32_t cp;
    while (i < n) {
        size_t atb = i;
        utf8_next(s, n, &i, &cp);
        if (escaped) {
            escaped = 0;
            continue;
        }
        if (quote && cp == '\\') {
            escaped = 1;
            continue;
        }
        if (quote && cp == (uint32_t)quote)
            quote = 0;
        else if (!quote) {
            if (cp == '#') {
                *out_n = atb;
                return 0;
            }
            if (cp == '\'' || cp == '"') quote = (int)cp;
        }
    }
    if (quote) {
        err_set(err, at + (n ? n - 1 : 0), "unterminated quoted string");
        return -1;
    }
    *out_n = n;
    return 0;
}

static int hex4(const char *s, size_t n, size_t *i, size_t at, uint32_t *out, TgwError *err) {
    if (*i + 4 > n) {
        err_set(err, at + *i, "Unicode escapes need four hex digits");
        return -1;
    }
    uint32_t v = 0;
    for (int k = 0; k < 4; k++) {
        unsigned char c = (unsigned char)s[*i + (size_t)k];
        uint32_t d;
        if (c >= '0' && c <= '9')
            d = c - '0';
        else if (c >= 'a' && c <= 'f')
            d = c - 'a' + 10;
        else if (c >= 'A' && c <= 'F')
            d = c - 'A' + 10;
        else {
            err_set(err, at + *i, "Unicode escapes need four hex digits");
            return -1;
        }
        v = v * 16 + d;
    }
    *i += 4;
    *out = v;
    return 0;
}

static int unicode_escape(const char *s, size_t n, size_t *i, size_t at, uint32_t *out, TgwError *err) {
    uint32_t code;
    if (hex4(s, n, i, at, &code, err)) return -1;
    if (code >= 0xd800 && code <= 0xdbff) {
        if (*i + 2 > n || s[*i] != '\\' || s[*i + 1] != 'u') {
            err_set(err, at + *i, "high surrogate needs a low surrogate");
            return -1;
        }
        *i += 2;
        uint32_t low;
        if (hex4(s, n, i, at, &low, err)) return -1;
        if (low < 0xdc00 || low > 0xdfff) {
            err_set(err, at + *i, "invalid low surrogate");
            return -1;
        }
        code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
    }
    if (code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) {
        err_set(err, at + *i, "invalid Unicode scalar");
        return -1;
    }
    *out = code;
    return 0;
}

static int tokens(Arena *a, const char *s, size_t n, size_t at, SpanVec *out, TgwError *err) {
    size_t i = 0;
    while (i < n) {
        uint32_t cp;
        size_t j = i;
        utf8_next(s, n, &j, &cp);
        if (cp_is_ws(cp)) {
            i = j;
            continue;
        }
        if (cp != '\'' && cp != '"') {
            size_t start = i;
            while (i < n) {
                size_t k = i;
                utf8_next(s, n, &k, &cp);
                if (cp_is_ws(cp)) break;
                i = k;
            }
            spanvec_push(a, out, span_make(s + start, i - start));
            continue;
        }
        uint32_t quote = cp;
        i = j;
        Buf b;
        uint8_t stack[256];
        buf_init(&b, stack, sizeof stack);
        int closed = 0;
        while (i < n) {
            size_t before = i;
            utf8_next(s, n, &i, &cp);
            if (cp == quote) {
                closed = 1;
                break;
            }
            if (cp == '\\') {
                if (i >= n) {
                    buf_free(&b);
                    err_set(err, at + i, "unfinished escape");
                    return -1;
                }
                uint32_t e;
                utf8_next(s, n, &i, &e);
                uint32_t put = e;
                if (e == 'u') {
                    if (unicode_escape(s, n, &i, at, &put, err)) {
                        buf_free(&b);
                        return -1;
                    }
                } else if (e == 'n')
                    put = '\n';
                else if (e == 'r')
                    put = '\r';
                else if (e == 't')
                    put = '\t';
                else if (e == '\\' || e == '\'' || e == '"')
                    put = e;
                else {
                    buf_free(&b);
                    err_set(err, at + i, "unsupported escape; use n, r, t, a quote, or a backslash");
                    return -1;
                }
                char tmp[4];
                buf_append(&b, tmp, utf8_encode(put, tmp));
                (void)before;
            } else {
                buf_append(&b, s + before, i - before);
            }
        }
        if (!closed) {
            buf_free(&b);
            err_set(err, at + i, "unterminated quoted string");
            return -1;
        }
        if (i < n) {
            size_t k = i;
            utf8_next(s, n, &k, &cp);
            if (!cp_is_ws(cp)) {
                buf_free(&b);
                err_set(err, at + i, "quoted labels must be separated by whitespace");
                return -1;
            }
        }
        spanvec_push(a, out, arena_span(a, (char *)b.data, b.len));
        buf_free(&b);
    }
    return 0;
}

static int text_span(Arena *a, const char *s, size_t n, size_t at, Span *out, TgwError *err) {
    if (n && (s[0] == '\'' || s[0] == '"')) {
        SpanVec parts = {0};
        if (tokens(a, s, n, at, &parts, err)) return -1;
        if (parts.len != 1) {
            err_set(err, at, "expected one quoted string");
            return -1;
        }
        *out = parts.data[0];
        return 0;
    }
    *out = span_make(s, n);
    return 0;
}

static int parse_number(const char *s, size_t n, size_t at, double lo, double hi, double *out, TgwError *err) {
    double v;
    if (!parse_f64_slice(s, n, &v) || v < lo || v > hi) {
        uint8_t stack[160];
        Buf b;
        buf_init(&b, stack, sizeof stack);
        BUF_LIT(&b, "expected a finite number between ");
        fmt_rust_f64(&b, lo);
        BUF_LIT(&b, " and ");
        fmt_rust_f64(&b, hi);
        buf_push(&b, 0);
        err_set(err, at, (char *)b.data);
        buf_free(&b);
        return -1;
    }
    *out = v;
    return 0;
}

static int decimal_places_token(const char *s, size_t n) {
    size_t e = n;
    for (size_t i = 0; i < n; i++)
        if (s[i] == 'e' || s[i] == 'E') {
            e = i;
            break;
        }
    size_t dot = e;
    for (size_t i = 0; i < e; i++)
        if (s[i] == '.') {
            dot = i;
            break;
        }
    int64_t frac = dot < e ? (int64_t)(e - dot - 1) : 0;
    int64_t exp = 0;
    if (e < n) {
        size_t k = e + 1;
        int sign = 1;
        if (k < n && (s[k] == '+' || s[k] == '-')) {
            if (s[k] == '-') sign = -1;
            k++;
        }
        for (; k < n; k++) {
            if (s[k] < '0' || s[k] > '9') break;
            exp = exp * 10 + (s[k] - '0');
        }
        exp *= sign;
    }
    int64_t dp = frac - exp;
    if (dp < 0) dp = 0;
    if (dp > 15) dp = 15;
    return (int)dp;
}

static int parse_tick(Arena *a, const char *s, size_t n, size_t at, Tick *tick, TgwError *err) {
    memset(tick, 0, sizeof *tick);
    if (n == 3 && memcmp(s, "off", 3) == 0) return 0;
    SpanVec parts = {0};
    if (tokens(a, s, n, at, &parts, err)) return -1;
    if (!parts.len) {
        err_set(err, at, "tick needs a start [step], quoted labels, or off");
        return -1;
    }
    if (s[0] == '\'' || s[0] == '"') {
        tick->kind = 2;
        tick->labels = parts;
        return 0;
    }
    if (parts.len > 2) {
        err_set(err, at, "tick needs start [step]; quote values for explicit labels");
        return -1;
    }
    double offset, step = 1;
    if (parse_number(parts.data[0].p, parts.data[0].n, at, -HUGE_VAL, HUGE_VAL, &offset, err)) return -1;
    if (parts.len == 2) {
        if (parse_number(parts.data[1].p, parts.data[1].n, at, -HUGE_VAL, HUGE_VAL, &step, err)) return -1;
    }
    int dp = decimal_places_token(parts.len == 2 ? parts.data[1].p : parts.data[0].p,
                                  parts.len == 2 ? parts.data[1].n : parts.data[0].n);
    tick->kind = 1;
    tick->offset = offset;
    tick->step = step;
    tick->dp = (uint8_t)dp;
    tick->fixed = parts.len == 2 || dp > 0;
    return 0;
}

static int wave_ok(uint8_t c) {
    switch (c) {
    case 'p':
    case 'n':
    case 'P':
    case 'N':
    case 'h':
    case 'l':
    case 'H':
    case 'L':
    case 'x':
    case 'd':
    case 'u':
    case 'z':
    case '=':
    case '|':
    case '<':
    case '>':
        return 1;
    default:
        return c >= '0' && c <= '9';
    }
}

static void sym_debug(uint32_t cp, char *out, size_t cap) {
    if (cp >= 32 && cp < 127 && cp != '\\' && cp != '\'')
        snprintf(out, cap, "'%c'", (char)cp);
    else
        snprintf(out, cap, "'\\u{%x}'", cp);
}

typedef struct OpenGroup {
    int64_t x;
    size_t start;
    int has_name;
    Span name;
    size_t offset;
} OpenGroup;

int tgw_parse_native(const char *src, size_t len, Doc *doc, Arena *arena, TgwError *err) {
    OpenGroup groups[64];
    int depth = 0;
    int empty = 0;
    size_t offset = 0;
    while (offset < len) {
        size_t line_off = offset;
        size_t end = offset;
        while (end < len && src[end] != '\n') end++;
        size_t raw_n = end - offset;
        int nl = end < len;
        offset = end + (nl ? 1 : 0);
        size_t cut;
        if (strip_comment(src + line_off, raw_n, line_off, &cut, err)) return -1;
        size_t ts = trim_start(src + line_off, cut);
        size_t te = trim_end(src + line_off + ts, cut - ts);
        if (!te) continue;
        const char *line = src + line_off + ts;
        size_t ln = te;
        size_t at = line_off + ts;
        if (line[0] == '@') {
            size_t ks = 1;
            while (ks < ln && !cp_is_ws((uint8_t)line[ks])) ks++;
            const char *key = line + 1;
            size_t kn = ks - 1;
            size_t vs = ks;
            while (vs < ln && cp_is_ws((uint8_t)line[vs])) vs++;
            const char *val = line + vs;
            size_t vn = ln - vs;
            if (kn == 5 && memcmp(key, "empty", 5) == 0) {
                if (vn) {
                    err_set(err, at, "@empty takes no arguments");
                    return -1;
                }
                empty = 1;
            } else if (kn == 5 && memcmp(key, "title", 5) == 0) {
                if (text_span(arena, val, vn, at, &doc->head.text, err)) return -1;
                doc->head.has_text = 1;
            } else if (kn == 6 && memcmp(key, "footer", 6) == 0) {
                if (text_span(arena, val, vn, at, &doc->foot.text, err)) return -1;
                doc->foot.has_text = 1;
            } else if (kn == 4 && memcmp(key, "tick", 4) == 0) {
                if (parse_tick(arena, val, vn, at, &doc->head.tick, err)) return -1;
            } else if (kn == 4 && memcmp(key, "tock", 4) == 0) {
                if (parse_tick(arena, val, vn, at, &doc->head.tock, err)) return -1;
            } else if (kn == 9 && memcmp(key, "foot-tick", 9) == 0) {
                if (parse_tick(arena, val, vn, at, &doc->foot.tick, err)) return -1;
            } else if (kn == 9 && memcmp(key, "foot-tock", 9) == 0) {
                if (parse_tick(arena, val, vn, at, &doc->foot.tock, err)) return -1;
            } else if (kn == 5 && memcmp(key, "every", 5) == 0) {
                if (parse_number(val, vn, at, -HUGE_VAL, HUGE_VAL, &doc->head.every, err)) return -1;
            } else if (kn == 10 && memcmp(key, "foot-every", 10) == 0) {
                if (parse_number(val, vn, at, -HUGE_VAL, HUGE_VAL, &doc->foot.every, err)) return -1;
            } else if (kn == 5 && memcmp(key, "scale", 5) == 0) {
                double n;
                if (parse_number(val, vn, at, 1, 100, &n, err)) return -1;
                if (n != trunc(n)) {
                    err_set(err, at, "@scale must be an integer from 1 to 100");
                    return -1;
                }
                doc->hscale = (int32_t)n;
            } else if (kn == 6 && memcmp(key, "bounds", 6) == 0) {
                SpanVec parts = {0};
                if (tokens(arena, val, vn, at, &parts, err)) return -1;
                if (parts.len != 2) {
                    err_set(err, at, "@bounds needs two cycle numbers: @bounds 0 10");
                    return -1;
                }
                double lo, hi;
                if (parse_number(parts.data[0].p, parts.data[0].n, at, -1e15, 1e15, &lo, err)) return -1;
                if (parse_number(parts.data[1].p, parts.data[1].n, at, -1e15, 1e15, &hi, err)) return -1;
                if (lo >= hi || lo != trunc(lo) || hi != trunc(hi)) {
                    err_set(err, at, "@bounds needs increasing integer cycle numbers");
                    return -1;
                }
                doc->xmin = (int64_t)lo * 2;
                doc->xmax_cfg = (int64_t)hi * 2;
            } else if (kn == 4 && memcmp(key, "grid", 4) == 0) {
                if (vn == 2 && memcmp(val, "on", 2) == 0)
                    doc->marks = 1;
                else if (vn == 3 && memcmp(val, "off", 3) == 0)
                    doc->marks = 0;
                else {
                    err_set(err, at, "@grid must be on or off");
                    return -1;
                }
            } else if (kn == 8 && memcmp(key, "arc-font", 8) == 0) {
                if (parse_number(val, vn, at, 0, 1e6, &doc->arc_font, err)) return -1;
                if (doc->arc_font == 0) {
                    err_set(err, at, "@arc-font must be positive");
                    return -1;
                }
            } else if (kn == 4 && memcmp(key, "gaps", 4) == 0) {
                if (text_span(arena, val, vn, at, &doc->gaps, err)) return -1;
                doc->has_gaps = 1;
            } else if (kn == 4 && memcmp(key, "edge", 4) == 0) {
                Span edge;
                if (text_span(arena, val, vn, at, &edge, err)) return -1;
                size_t i = 0;
                uint32_t cp;
                while (i < edge.n && cp_is_ws((unsigned char)edge.p[i] < 128 ? (uint8_t)edge.p[i] : 0)) {
                    size_t k = i;
                    utf8_next(edge.p, edge.n, &k, &cp);
                    if (!cp_is_ws(cp)) break;
                    i = k;
                }
                size_t start = i;
                uint32_t first = 0, last = 0;
                int chars = 0;
                while (i < edge.n) {
                    size_t k = i;
                    utf8_next(edge.p, edge.n, &k, &cp);
                    if (cp_is_ws(cp)) break;
                    if (!chars) first = cp;
                    last = cp;
                    chars++;
                    i = k;
                }
                (void)first;
                (void)last;
                if (chars < 2) {
                    err_set(err, at, "@edge needs two nodes and a connector, for example a~>b label");
                    return -1;
                }
                (void)start;
                doc_add_edge(arena, doc, edge);
            } else if (kn == 5 && memcmp(key, "group", 5) == 0) {
                if (depth >= 64) {
                    err_set(err, at, "group nesting exceeds 64 levels");
                    return -1;
                }
                OpenGroup g;
                memset(&g, 0, sizeof g);
                g.offset = at;
                g.start = doc->lane_n;
                if (vn) {
                    if (text_span(arena, val, vn, at, &g.name, err)) return -1;
                    g.has_name = 1;
                }
                int64_t base = depth ? groups[depth - 1].x : 10;
                g.x = base + (g.has_name ? 25 : 10);
                groups[depth++] = g;
            } else if (kn == 3 && memcmp(key, "end", 3) == 0) {
                if (vn) {
                    err_set(err, at, "@end takes no arguments");
                    return -1;
                }
                if (!depth) {
                    err_set(err, at, "@end has no matching @group");
                    return -1;
                }
                OpenGroup g = groups[--depth];
                Group out;
                memset(&out, 0, sizeof out);
                out.x = g.x;
                out.y = (int64_t)g.start;
                out.height = (int64_t)doc->lane_n - (int64_t)g.start;
                out.has_name = g.has_name;
                out.name = g.name;
                doc_add_group(arena, doc, out);
            } else {
                err_fmt(err, at, "unknown directive @%.*s", (int)kn, key);
                return -1;
            }
            continue;
        }
        int64_t indent = depth ? groups[depth - 1].x : 10;
        if (ln == 3 && memcmp(line, "---", 3) == 0) {
            doc_add_lane(arena, doc, lane_blank(span_cstr(" "), indent));
            continue;
        }
        size_t colon;
        if (!find_outside(line, ln, ":", 1, &colon)) {
            err_set(err, at, "expected name: waveform (or an @directive)");
            return -1;
        }
        size_t name_n = trim_end(line, colon);
        size_t name_s = trim_start(line, name_n);
        Span name;
        if (text_span(arena, line + name_s, name_n - name_s, at, &name, err)) return -1;
        if (!name.n) name = span_cstr(" ");
        const char *rest = line + colon + 1;
        size_t rest_n = ln - colon - 1;
        size_t semi = 0;
        int have_semi = find_outside(rest, rest_n, ";", 1, &semi);
        size_t body_n = have_semi ? semi : rest_n;
        const char *body = rest;
        size_t bs = trim_start(body, body_n);
        size_t be = trim_end(body + bs, body_n - bs);
        const char *bt = body + bs;
        Lane lane = lane_blank(name, indent);
        size_t arrow;
        const char *wave = bt;
        size_t wave_n = be;
        if (find_outside(bt, be, "=>", 2, &arrow)) {
            wave_n = trim_end(bt, arrow);
            const char *data = bt + arrow + 2;
            size_t data_n = be - (arrow + 2);
            size_t ds = trim_start(data, data_n);
            if (tokens(arena, data + ds, data_n - ds, at, &lane.data, err)) return -1;
        }
        size_t ws = trim_start(wave, wave_n);
        size_t we = trim_end(wave + ws, wave_n - ws);
        wave += ws;
        wave_n = we;
        if (wave_n >= 5 && memcmp(wave, "path ", 5) == 0) {
            const char *path = wave + 5;
            size_t pn = wave_n - 5;
            size_t ps = trim_start(path, pn);
            if (text_span(arena, path + ps, pn - ps, at, &lane.wave, err)) return -1;
            lane.body = 2;
        } else if (!wave_n) {
            lane.body = 0;
        } else {
            Span wtxt;
            if (text_span(arena, wave, wave_n, at, &wtxt, err)) return -1;
            int spaced = 0;
            for (uint32_t i = 0; i < wtxt.n; i++) {
                uint8_t c = (uint8_t)wtxt.p[i];
                if (c == '.') continue;
                if (byte_is_ascii_ws(c)) {
                    spaced = 1;
                    continue;
                }
                if (!wave_ok(c)) {
                    uint32_t cp;
                    size_t k = i;
                    utf8_next(wtxt.p, wtxt.n, &k, &cp);
                    char dbg[32];
                    sym_debug(cp, dbg, sizeof dbg);
                    err_fmt(err, at, "unknown wave symbol %s", dbg);
                    return -1;
                }
            }
            if (spaced) {
                char *d = arena_alloc(arena, wtxt.n + 1);
                size_t m = 0;
                for (uint32_t i = 0; i < wtxt.n; i++)
                    if (!byte_is_ascii_ws((uint8_t)wtxt.p[i])) d[m++] = wtxt.p[i];
                d[m] = 0;
                lane.wave = span_make(d, m);
            } else {
                lane.wave = wtxt;
            }
            lane.body = 1;
        }
        uint8_t seen = 0;
        const char *opt = have_semi ? rest + semi + 1 : NULL;
        size_t opt_n = have_semi ? rest_n - semi - 1 : 0;
        while (opt && opt_n) {
            size_t sep;
            size_t piece_n = opt_n;
            if (find_outside(opt, opt_n, ";", 1, &sep)) piece_n = sep;
            size_t os = trim_start(opt, piece_n);
            size_t oe = trim_end(opt + os, piece_n - os);
            const char *piece = opt + os;
            size_t eq = 0;
            int got = 0;
            for (size_t i = 0; i < oe; i++)
                if (piece[i] == '=') {
                    eq = i;
                    got = 1;
                    break;
                }
            if (!got) {
                err_set(err, at, "lane options use ; key=value");
                return -1;
            }
            size_t key_n = trim_end(piece, eq);
            size_t val_s = eq + 1;
            while (val_s < oe && byte_is_ascii_ws((uint8_t)piece[val_s])) val_s++;
            size_t val_n = trim_end(piece + val_s, oe - val_s);
            const char *okey = piece;
            const char *oval = piece + val_s;
            uint8_t bit = 0;
            if (key_n == 6 && memcmp(okey, "period", 6) == 0)
                bit = 1;
            else if (key_n == 5 && memcmp(okey, "phase", 5) == 0)
                bit = 2;
            else if (key_n == 4 && memcmp(okey, "node", 4) == 0)
                bit = 4;
            else if (key_n == 4 && memcmp(okey, "over", 4) == 0)
                bit = 8;
            else if (key_n == 5 && memcmp(okey, "under", 5) == 0)
                bit = 16;
            else {
                err_fmt(err, at, "unknown lane option \"%.*s\"", (int)key_n, okey);
                return -1;
            }
            if (seen & bit) {
                err_fmt(err, at, "duplicate lane option \"%.*s\"", (int)key_n, okey);
                return -1;
            }
            seen |= bit;
            if (bit == 1) {
                if (parse_number(oval, val_n, at, 0, 1e15, &lane.period, err)) return -1;
                if (lane.period == 0) {
                    err_set(err, at, "period must be positive");
                    return -1;
                }
            } else if (bit == 2) {
                if (parse_number(oval, val_n, at, -1e15, 1e15, &lane.phase, err)) return -1;
            } else if (bit == 4) {
                if (text_span(arena, oval, val_n, at, &lane.node, err)) return -1;
                lane.has_node = 1;
            } else if (bit == 8) {
                if (text_span(arena, oval, val_n, at, &lane.over, err)) return -1;
                lane.has_over = 1;
            } else {
                if (text_span(arena, oval, val_n, at, &lane.under, err)) return -1;
                lane.has_under = 1;
            }
            if (piece_n == opt_n) break;
            opt += piece_n + 1;
            opt_n -= piece_n + 1;
        }
        doc_add_lane(arena, doc, lane);
    }
    if (depth) {
        err_set(err, groups[depth - 1].offset, "@group is missing @end");
        return -1;
    }
    if (!doc->lane_n && !empty) {
        err_set(err, 0, "at least one signal lane is required");
        return -1;
    }
    if (empty && doc->lane_n) {
        err_set(err, 0, "@empty cannot be combined with signal lanes");
        return -1;
    }
    double dx = (double)doc->xmin / 2.0;
    Tick *ticks[4] = {&doc->head.tick, &doc->head.tock, &doc->foot.tick, &doc->foot.tock};
    for (int i = 0; i < 4; i++) {
        Tick *t = ticks[i];
        if (t->kind == 1 && fabs(t->step - 1.0) < 1e-9 && t->dp == 0) t->offset += dx;
    }
    return 0;
}
