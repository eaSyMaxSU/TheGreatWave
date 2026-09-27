#ifndef TGW_COMMON_H
#define TGW_COMMON_H

#define _GNU_SOURCE

#include "tgw.h"

#include <assert.h>
#include <locale.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef __APPLE__
#include <xlocale.h>
#endif

#define TGW_XS 20
#define TGW_YS 20
#define TGW_YO 36
#define TGW_YM 15
#define TGW_Y0 5
#define TGW_XLABEL 6
#define TGW_TGO (-10)
#define TGW_TOO_BIG 1.0e15

#define TGW_LIKELY(x) __builtin_expect(!!(x), 1)
#define TGW_UNLIKELY(x) __builtin_expect(!!(x), 0)

typedef struct Span {
    const char *p;
    uint32_t n;
} Span;

typedef struct SpanVec {
    Span *data;
    uint32_t len;
    uint32_t cap;
} SpanVec;

typedef struct Tick {
    uint8_t kind; /* 0 off, 1 series, 2 labels */
    double offset;
    double step;
    uint8_t dp;
    uint8_t fixed;
    SpanVec labels;
} Tick;

typedef struct Cap {
    int has_text;
    Span text;
    Tick tick;
    Tick tock;
    double every;
} Cap;

typedef struct Lane {
    Span name;
    uint8_t body; /* 0 none, 1 wave, 2 path */
    Span wave;
    SpanVec data;
    double period;
    double phase;
    int has_node;
    int has_over;
    int has_under;
    Span node;
    Span over;
    Span under;
    int64_t indent;
} Lane;

typedef struct Group {
    int64_t x;
    int64_t y;
    int64_t height;
    int has_name;
    Span name;
} Group;

typedef struct Doc {
    Lane small_lanes[8];
    Lane *lanes;
    uint32_t lane_n;
    uint32_t lane_cap;
    int lanes_heap;
    Group small_groups[8];
    Group *groups;
    uint32_t group_n;
    uint32_t group_cap;
    int groups_heap;
    Span *edges;
    uint32_t edge_n;
    uint32_t edge_cap;
    Cap head;
    Cap foot;
    int32_t hscale;
    int64_t xmin;
    int64_t xmax_cfg;
    int marks;
    double arc_font;
    int has_gaps;
    Span gaps;
} Doc;

typedef struct Chunk {
    struct Chunk *next;
    size_t used;
    size_t cap;
    char data[];
} Chunk;

typedef struct Arena {
    Chunk *head;
    char scratch[1 << 15];
    size_t scratch_used;
} Arena;

typedef struct Buf {
    uint8_t *data;
    size_t len;
    size_t cap;
    int heap; /* data is malloc'd and must be freed by the owner of the buffer */
} Buf;

static inline Span span_make(const char *p, size_t n) {
    Span s;
    s.p = p;
    s.n = (uint32_t)n;
    return s;
}

static inline Span span_cstr(const char *p) {
    return span_make(p, p ? strlen(p) : 0);
}

static inline int span_eq(Span a, const char *lit) {
    size_t n = strlen(lit);
    return a.n == n && (n == 0 || memcmp(a.p, lit, n) == 0);
}

static inline int span_eq_span(Span a, Span b) {
    return a.n == b.n && (a.n == 0 || memcmp(a.p, b.p, a.n) == 0);
}

void tgw_oom(void);

void arena_init(Arena *a);
void arena_free(Arena *a);
void *arena_alloc(Arena *a, size_t n);
char *arena_copy(Arena *a, const char *p, size_t n);
Span arena_span(Arena *a, const char *p, size_t n);

void buf_init(Buf *b, uint8_t *storage, size_t cap);
void buf_free(Buf *b);
void buf_clear(Buf *b);
void buf_reserve(Buf *b, size_t extra);
void buf_append(Buf *b, const void *p, size_t n);
void buf_push(Buf *b, uint8_t c);
void buf_puts(Buf *b, const char *s);

#define BUF_LIT(b, lit) buf_append((b), (lit), sizeof(lit) - 1)

void push_u64(Buf *b, uint64_t n);
void push_i64(Buf *b, int64_t n);
void push_f64(Buf *b, double n);
void push_esc(Buf *b, const char *s, size_t n);
void fmt_rust_f64(Buf *b, double v);
void fmt_fixed(Buf *b, double v, int dp);

locale_t tgw_c_locale(void);
int parse_f64_slice(const char *s, size_t n, double *out);
int utf8_ok(const uint8_t *s, size_t n);
int utf8_next(const char *s, size_t n, size_t *i, uint32_t *cp);
size_t utf8_encode(uint32_t cp, char out[4]);
size_t utf8_count(const char *s, size_t n);
int cp_is_ws(uint32_t cp);
int byte_is_ascii_ws(uint8_t c);

void spanvec_push(Arena *a, SpanVec *v, Span s);

void doc_init(Doc *d);
void doc_add_lane(Arena *a, Doc *d, Lane lane);
void doc_add_group(Arena *a, Doc *d, Group g);
void doc_add_edge(Arena *a, Doc *d, Span e);
Lane lane_blank(Span name, int64_t indent);

void err_set(TgwError *e, size_t off, const char *msg);
void err_fmt(TgwError *e, size_t off, const char *fmt, ...);

typedef struct Code {
    uint8_t kind;
    uint8_t a;
    uint8_t b;
} Code;

enum {
    CK_RISE = 1,
    CK_FALL,
    CK_RISEA,
    CK_FALLA,
    CK_LOW,
    CK_HIGH,
    CK_MID,
    CK_DASHL,
    CK_DASHH,
    CK_X,
    CK_BUS,
    CK_SOFT
};

enum { PAT_FILL = 1, PAT_LEAD, PAT_CLOCK };

typedef struct Pat {
    uint8_t kind;
    Code c0, c1, c2, c3;
    int64_t at;
    int64_t n0;
    int64_t n1;
} Pat;

typedef struct WaveOut {
    Pat small[4];
    Pat *pats;
    uint32_t pat_n;
    uint32_t pat_cap;
    int64_t len;
    int64_t skip;
    double *gaps;
    uint32_t gap_n;
    uint32_t gap_cap;
    double *markers;
    double *mwidth;
    uint32_t mark_n;
    uint32_t mark_cap;
    size_t unseen;
} WaveOut;

static inline Code code_k(uint8_t k) {
    Code c = {k, 0, 0};
    return c;
}
static inline Code code_bus(uint8_t n) {
    Code c = {CK_BUS, n, 0};
    return c;
}
static inline Code code_soft(uint8_t p, uint8_t n) {
    Code c = {CK_SOFT, p, n};
    return c;
}
static inline int code_eq(Code a, Code b) { return a.kind == b.kind && a.a == b.a && a.b == b.b; }

void wave_init(WaveOut *w);
int wave_compile(Arena *a, WaveOut *w, const char *wave, size_t n, double period, int32_t hscale,
                 double phase_bricks, double width_bricks, TgwError *err);
int wave_nodes(Arena *a, const char *wave, size_t n, double period, int32_t hscale, double **out,
               size_t *out_n, TgwError *err);
int64_t wave_visible(const WaveOut *w);

typedef struct Paint {
    Buf body;
    uint8_t stack[12288];
    Code ks0[64], ks1[64], ks2[64], ks3[64];
    int64_t kextra[64];
    uint32_t clocks;
    int hatch;
} Paint;

void paint_init(Paint *p);
void paint_free(Paint *p);
void paint_wave(Paint *p, const Pat *pats, uint32_t n, int64_t skip, int64_t end);
void write_clock_pattern(Buf *body, uint32_t id, Code s0, Code s1, Code s2, Code s3, int64_t extra);
const char *bus_fill(uint8_t n);

int tgw_parse(const char *src, size_t len, TgwFormat fmt, Doc *doc, Arena *arena, TgwError *err);
int tgw_parse_native(const char *src, size_t len, Doc *doc, Arena *arena, TgwError *err);
int tgw_parse_json5(const char *src, size_t len, Doc *doc, Arena *arena, TgwError *err);
int tgw_emit(const Doc *doc, Buf *out, unsigned indent, Arena *arena, TgwError *err);
void tgw_write_tgw(const Doc *doc, Buf *out);

double tgw_text_width(const char *s, size_t n, double size);

int tgw_extent(const char *path, size_t n, double *xmax, TgwError *err);

#endif
