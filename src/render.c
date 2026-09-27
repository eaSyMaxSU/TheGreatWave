#include "common.h"

const char *tgw_version(void) { return TGW_VERSION; }

void tgw_buf_free(TgwBuf *buf) {
    if (!buf) return;
    free(buf->data);
    buf->data = NULL;
    buf->len = 0;
    buf->cap = 0;
}

static int detect_json(const char *s, size_t n) {
    size_t i = 0;
    if (n >= 3 && (uint8_t)s[0] == 0xef && (uint8_t)s[1] == 0xbb && (uint8_t)s[2] == 0xbf) i = 3;
    while (i < n) {
        unsigned char c = (unsigned char)s[i];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
            i++;
            continue;
        }
        break;
    }
    /* trim_start_matches BOM then trim_start. Only ASCII ws here matches the detector's practical cases;
       also skip further UTF-8 BOM bytes the way trim_start_matches does for U+FEFF. */
    while (i + 3 <= n && (uint8_t)s[i] == 0xef && (uint8_t)s[i + 1] == 0xbb && (uint8_t)s[i + 2] == 0xbf) i += 3;
    if (i < n && (s[i] == '{' || s[i] == '[')) return 1;
    if (i + 1 < n && s[i] == '/' && (s[i + 1] == '/' || s[i + 1] == '*')) return 1;
    return 0;
}

int tgw_parse(const char *src, size_t len, TgwFormat fmt, Doc *doc, Arena *arena, TgwError *err) {
    if (!src) {
        src = "";
        len = 0;
    }
    size_t bom = 0;
    if (len >= 3 && (uint8_t)src[0] == 0xef && (uint8_t)src[1] == 0xbb && (uint8_t)src[2] == 0xbf) bom = 3;
    int json = fmt == TGW_JSON5 || (fmt == TGW_AUTO && detect_json(src, len));
    const char *body = src + bom;
    size_t n = len - bom;
    if (json) return tgw_parse_json5(body, n, doc, arena, err);
    return tgw_parse_native(body, n, doc, arena, err);
}

static void bind(Buf *b, TgwBuf *out) {
    b->data = out->data;
    b->len = 0;
    b->cap = out->cap;
    b->heap = out->data != NULL;
}

static void commit(Buf *b, TgwBuf *out) {
    out->data = b->data;
    out->len = b->len;
    out->cap = b->cap;
}

int tgw_render(const char *src, size_t len, TgwBuf *out, unsigned indent, TgwFormat fmt, TgwError *err) {
    if (!out) return -1;
    Buf b;
    bind(&b, out);
    Arena arena;
    arena_init(&arena);
    Doc doc;
    doc_init(&doc);
    int rc = tgw_parse(src, len, fmt, &doc, &arena, err);
    if (!rc) rc = tgw_emit(&doc, &b, indent > 255 ? 255 : indent, &arena, err);
    arena_free(&arena);
    if (rc) b.len = 0;
    commit(&b, out);
    return rc;
}

int tgw_convert(const char *src, size_t len, TgwBuf *out, TgwFormat fmt, TgwError *err) {
    if (!out) return -1;
    Buf b;
    bind(&b, out);
    Arena arena;
    arena_init(&arena);
    Doc doc;
    doc_init(&doc);
    int rc = tgw_parse(src, len, fmt, &doc, &arena, err);
    if (!rc) tgw_write_tgw(&doc, &b);
    arena_free(&arena);
    if (rc) b.len = 0;
    commit(&b, out);
    return rc;
}
