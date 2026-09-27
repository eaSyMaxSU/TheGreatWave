#include "tgw.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int g_fail;

static void fail(const char *msg) {
    fprintf(stderr, "FAIL %s\n", msg);
    g_fail++;
}

static char *slurp(const char *path, size_t *n) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        fprintf(stderr, "open %s\n", path);
        exit(1);
    }
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    rewind(f);
    char *s = malloc((size_t)sz + 1);
    if (!s) exit(1);
    if (fread(s, 1, (size_t)sz, f) != (size_t)sz) exit(1);
    fclose(f);
    s[sz] = 0;
    *n = (size_t)sz;
    return s;
}

static int render_src(const char *s, size_t n, TgwBuf *out, unsigned indent, TgwFormat fmt, TgwError *err) {
    return tgw_render(s, n, out, indent, fmt, err);
}

static int contains(const TgwBuf *b, const char *lit) {
    size_t n = strlen(lit);
    if (n > b->len) return 0;
    for (size_t i = 0; i + n <= b->len; i++)
        if (memcmp(b->data + i, lit, n) == 0) return 1;
    return 0;
}

static size_t count_lit(const TgwBuf *b, const char *lit) {
    size_t n = strlen(lit), c = 0;
    if (n > b->len) return 0;
    for (size_t i = 0; i + n <= b->len; i++)
        if (memcmp(b->data + i, lit, n) == 0) c++;
    return c;
}

static double dimension(const TgwBuf *b, const char *attr) {
    char key[32];
    snprintf(key, sizeof key, "%s=\"", attr);
    size_t n = strlen(key);
    for (size_t i = 0; i + n < b->len; i++) {
        if (memcmp(b->data + i, key, n) == 0) {
            return strtod((char *)b->data + i + n, NULL);
        }
    }
    return -1;
}

static void expect_eq_mem(const char *name, const void *a, size_t an, const void *b, size_t bn) {
    if (an == bn && memcmp(a, b, an) == 0) return;
    fprintf(stderr, "FAIL %s bytes got %zu exp %zu\n", name, an, bn);
    size_t m = an < bn ? an : bn;
    size_t i = 0;
    const unsigned char *x = a, *y = b;
    for (; i < m; i++)
        if (x[i] != y[i]) break;
    fprintf(stderr, "  at %zu\n  got ", i);
    for (size_t k = i > 40 ? i - 40 : 0; k < i + 60 && k < an; k++) fputc(x[k], stderr);
    fprintf(stderr, "\n  exp ");
    for (size_t k = i > 40 ? i - 40 : 0; k < i + 60 && k < bn; k++) fputc(y[k], stderr);
    fputc('\n', stderr);
    g_fail++;
}

static const char *NAMES[] = {"step4", "arcs", "arcs1", "clocks", "gaps", "bundles", "marks", "precision"};

static void check_fixtures(void) {
    TgwBuf out = {0};
    TgwError err;
    for (size_t i = 0; i < sizeof NAMES / sizeof NAMES[0]; i++) {
        char path[128], svgpath[128], jpath[128];
        snprintf(path, sizeof path, "tests/fixtures/%s.tgw", NAMES[i]);
        snprintf(svgpath, sizeof svgpath, "tests/fixtures/%s.svg", NAMES[i]);
        snprintf(jpath, sizeof jpath, "tests/fixtures/legacy/%s.json5", NAMES[i]);
        size_t n, sn, jn;
        char *src = slurp(path, &n);
        char *exp = slurp(svgpath, &sn);
        char *json = slurp(jpath, &jn);
        memset(&err, 0, sizeof err);
        if (render_src(src, n, &out, 0, TGW_AUTO, &err)) {
            fprintf(stderr, "FAIL render %s: %s\n", NAMES[i], err.message);
            g_fail++;
        } else {
            char label[64];
            snprintf(label, sizeof label, "native %s", NAMES[i]);
            expect_eq_mem(label, out.data, out.len, exp, sn);
        }
        memset(&err, 0, sizeof err);
        if (render_src(json, jn, &out, 0, TGW_AUTO, &err)) {
            fprintf(stderr, "FAIL json %s: %s\n", NAMES[i], err.message);
            g_fail++;
        } else {
            char label[64];
            snprintf(label, sizeof label, "json5 %s", NAMES[i]);
            expect_eq_mem(label, out.data, out.len, exp, sn);
        }
        TgwBuf conv = {0};
        memset(&err, 0, sizeof err);
        if (tgw_convert(json, jn, &conv, TGW_AUTO, &err)) {
            fprintf(stderr, "FAIL convert %s: %s\n", NAMES[i], err.message);
            g_fail++;
        } else {
            TgwBuf again = {0};
            if (tgw_convert((char *)conv.data, conv.len, &again, TGW_TGW, &err) || again.len != conv.len ||
                memcmp(again.data, conv.data, conv.len)) {
                fail("conversion is not idempotent");
            }
            memset(&err, 0, sizeof err);
            if (render_src((char *)conv.data, conv.len, &out, 0, TGW_TGW, &err)) {
                fprintf(stderr, "FAIL render converted %s: %s\n", NAMES[i], err.message);
                g_fail++;
            } else {
                char label[64];
                snprintf(label, sizeof label, "converted %s", NAMES[i]);
                expect_eq_mem(label, out.data, out.len, exp, sn);
            }
            tgw_buf_free(&again);
        }
        tgw_buf_free(&conv);
        free(src);
        free(exp);
        free(json);
    }
    tgw_buf_free(&out);
}

static void check_format_strings(void) {
    const char *a =
        "{signal:[['bus',{name:'a:b',wave:'2.',data:['line\\n\\\"two\\\"'],node:'a.'},null],"
        "{name:'analog',wave:['pw',{d:'M0,0 L1,1'}],period:2,phase:.5}],edge:['a~>b delay'],head:{text:'demo'}}";
    const char *exp_a =
        "@title demo\n\n@group bus\n  \"a:b\": 2. => \"line\\n\\\"two\\\"\" ; node=a.\n  ---\n@end\n"
        "analog:  path M0,0 L1,1 ; period=2 ; phase=0.5\n\n@edge a~>b delay\n";
    const char *b = "{signal:[],head:{tick:0,tock:'2 0.010'},foot:{tick:['zero','one']},config:{hbounds:[2,5]}}";
    const char *exp_b = "@tick 0\n@tock 0.02 0.010\n@foot-tick \"zero\" \"one\"\n@bounds 2 5\n@empty\n";
    TgwBuf out = {0};
    TgwError err;
    memset(&err, 0, sizeof err);
    if (tgw_convert(a, strlen(a), &out, TGW_JSON5, &err)) {
        fprintf(stderr, "FAIL format a: %s\n", err.message);
        g_fail++;
    } else
        expect_eq_mem("groups", out.data, out.len, exp_a, strlen(exp_a));
    memset(&err, 0, sizeof err);
    if (tgw_convert(b, strlen(b), &out, TGW_JSON5, &err)) {
        fprintf(stderr, "FAIL format b: %s\n", err.message);
        g_fail++;
    } else
        expect_eq_mem("ticks", out.data, out.len, exp_b, strlen(exp_b));
    tgw_buf_free(&out);
}

static void check_behavior(void) {
    TgwBuf a = {0}, b = {0};
    TgwError err;
    char dots[12000];
    dots[0] = 'p';
    memset(dots + 1, '.', 9999);
    dots[10000] = 0;
    char src[14000];
    snprintf(src, sizeof src, "{signal:[{name:'clk',wave:'%s'}]}", dots);
    memset(&err, 0, sizeof err);
    if (tgw_render(src, strlen(src), &b, 0, TGW_JSON5, &err)) {
        fprintf(stderr, "FAIL long clock: %s\n", err.message);
        g_fail++;
    } else if (b.len >= 8000 || contains(&b, "<use") || !contains(&b, "-k0\"") || !contains(&b, "H400000")) {
        fprintf(stderr, "FAIL long clock shape len=%zu\n", b.len);
        g_fail++;
    }
    const char *short_clk = "{signal:[{name:'clk',wave:'p...'}]}";
    tgw_render(short_clk, strlen(short_clk), &a, 0, TGW_JSON5, &err);
    if (count_lit(&a, "<pattern") != count_lit(&b, "<pattern") || count_lit(&a, "<rect") != count_lit(&b, "<rect") ||
        count_lit(&a, "<path") != count_lit(&b, "<path") || count_lit(&b, "<use")) {
        fail("long clock tag counts");
    }
    dots[0] = '1';
    snprintf(src, sizeof src, "{signal:[{name:'held',wave:'%s'}]}", dots);
    tgw_render(src, strlen(src), &b, 0, TGW_JSON5, &err);
    const char *short_hold = "{signal:[{name:'held',wave:'1'}]}";
    tgw_render(short_hold, strlen(short_hold), &a, 0, TGW_JSON5, &err);
    if (!contains(&b, "H400000") || contains(&b, "<use") || count_lit(&a, "<path") != count_lit(&b, "<path") ||
        count_lit(&b, "H400000") != 1) {
        fail("long hold");
    }
    const char *prefixed = "{signal:[{name:'c',wave:'xp'}]}";
    tgw_render(prefixed, strlen(prefixed), &a, 0, TGW_JSON5, &err);
    if (!contains(&a, "translate(40)") || !contains(&a, "-k0)")) fail("clock prefix");

    const char *indented = "{signal:[{name:'a',wave:'01'}]}";
    tgw_render(indented, strlen(indented), &a, 2, TGW_JSON5, &err);
    if (!contains(&a, "\n")) fail("indent");
    if (!tgw_render("[", 1, &a, 0, TGW_JSON5, &err) || err.offset != 0) fail("bare array offset");
    const char *nosig = "{config:{hscale:1}}";
    if (!tgw_render(nosig, strlen(nosig), &a, 0, TGW_JSON5, &err) || !strstr(err.message, "signal"))
        fail("signal required");
    if (a.len != 0) fail("error keeps output");

    unsigned char *kept = NULL;
    tgw_render(short_clk, strlen(short_clk), &a, 0, TGW_JSON5, &err);
    size_t first_len = a.len;
    kept = malloc(first_len);
    memcpy(kept, a.data, first_len);
    size_t cap = a.cap;
    memcpy(a.data + a.len, "junk", 4);
    a.len += 4;
    tgw_render(short_clk, strlen(short_clk), &a, 0, TGW_JSON5, &err);
    if (a.len != first_len || memcmp(a.data, kept, first_len) || a.cap < cap) fail("buffer reuse");
    free(kept);

    const char *pw = "{\n// a comment\nsignal:[{name:'pw',wave:['pw',{d:'M0,0 L1,1'}],},],}";
    if (tgw_render(pw, strlen(pw), &a, 0, TGW_JSON5, &err) || !contains(&a, "<path") || !contains(&a, "M0,0"))
        fail("piecewise comment");

    tgw_render(short_clk, strlen(short_clk), &a, 0, TGW_JSON5, &err);
    const char *delayed = "{signal:[{name:'clk',wave:'p...',phase:-0.25}]}";
    const char *advanced = "{signal:[{name:'clk',wave:'p...',phase:0.125}]}";
    tgw_render(delayed, strlen(delayed), &b, 0, TGW_JSON5, &err);
    TgwBuf c = {0};
    tgw_render(advanced, strlen(advanced), &c, 0, TGW_JSON5, &err);
    double dw = dimension(&b, "width") - dimension(&a, "width");
    double aw = dimension(&a, "width") - dimension(&c, "width");
    if (fabs(dw - 10.0) > 1e-9 || fabs(aw - 5.0) > 1e-9 || !contains(&c, "translate(-5)") ||
        !contains(&c, "-lane-crop)")) {
        fprintf(stderr, "FAIL phase extents %g %g\n", dw, aw);
        g_fail++;
    }

    char wave[10001];
    for (int i = 0; i < 1250; i++) memcpy(wave + i * 8, "23456789", 8);
    wave[10000] = 0;
    snprintf(src, sizeof src, "{signal:[{name:'data',wave:'%s'}],config:{hbounds:[5000,5005]}}", wave);
    if (tgw_render(src, strlen(src), &a, 0, TGW_JSON5, &err) || a.len >= 12000 || count_lit(&a, "<path") >= 40) {
        fprintf(stderr, "FAIL crop len=%zu paths=%zu\n", a.len, count_lit(&a, "<path"));
        g_fail++;
    }

    const char *ida = "{signal:[{name:'url(#k0)',wave:'p...'}]}";
    const char *idb = "{signal:[{name:'second',wave:'n...'}]}";
    tgw_render(ida, strlen(ida), &a, 0, TGW_JSON5, &err);
    tgw_render(idb, strlen(idb), &b, 0, TGW_JSON5, &err);
    if (!contains(&a, ">url(#k0)<") || !contains(&a, "role=\"img\"") || contains(&a, "<style")) fail("isolate");
    size_t ha = 0, hb = 0;
    for (size_t i = 0; i + 4 <= a.len; i++)
        if (memcmp(a.data + i, "tgw-", 4) == 0) {
            ha = i;
            break;
        }
    for (size_t i = 0; i + 4 <= b.len; i++)
        if (memcmp(b.data + i, "tgw-", 4) == 0) {
            hb = i;
            break;
        }
    if (!ha || !hb || (a.len >= ha + 20 && b.len >= hb + 20 && memcmp(a.data + ha, b.data + hb, 20) == 0))
        fail("paint ids collide");

    const char *analog =
        "{signal:[{name:'analog',period:2,wave:['pw',{d:'M0,0 C1,0 1,1 2,1 A1,1 30 0 1 3,0'}]}],config:{hscale:2}}";
    if (tgw_render(analog, strlen(analog), &a, 0, TGW_JSON5, &err) || !contains(&a, "scale(160,-20)") ||
        !contains(&a, "vector-effect=\"non-scaling-stroke\"") || !contains(&a, "A1,1 30 0 1 3,0") ||
        dimension(&a, "width") <= 480)
        fail("path scale");

    const char *bad[] = {
        "{signal:[{wave:'p',period:-1}]}",
        "{signal:[{wave:['pw',{d:'M0 0 !'}]}]}",
        "{signal:[{wave:'p'}],head:{tick:'0 1e308'}}",
        "{signal:[] /*",
    };
    for (size_t i = 0; i < 4; i++) {
        a.len = 4;
        if (a.cap < 8) {
            a.data = realloc(a.data, 64);
            a.cap = 64;
        }
        memcpy(a.data, "junk", 4);
        if (!tgw_render(bad[i], strlen(bad[i]), &a, 0, TGW_JSON5, &err) || a.len != 0) {
            fprintf(stderr, "FAIL error clear %s (%s)\n", bad[i], err.message);
            g_fail++;
        }
    }
    const char *comment = "{signal:[] /* unterminated";
    if (!tgw_render(comment, strlen(comment), &a, 0, TGW_JSON5, &err) || !strstr(err.message, "comment"))
        fail("comment error");
    char nest[400];
    strcpy(nest, "{signal:");
    for (int i = 0; i < 140; i++) strcat(nest, "[");
    if (!tgw_render(nest, strlen(nest), &a, 0, TGW_JSON5, &err) || !strstr(err.message, "nesting")) fail("nesting");

    const char *rounds[] = {
        "{signal:[{name:'a:b; \\uD83C\\uDF0A',wave:'23',data:['a # b','line\\n\\u0001\\uD83C\\uDF0A']}]}",
        "{signal:[]}",
        "{signal:[{wave:'p...'}],head:{tick:0,every:-2}}",
        "{signal:[['outer',['inner',{name:'a',wave:'01'}]]],config:{hbounds:[1,2]},head:{tick:'0.25 0.1'}}",
        "{signal:[{name:'p',wave:['pw',{d:'M0,0 L1,1'}]}]}",
        "{signal:[{node:'a...b'}],edge:['a<->b label']}",
    };
    for (size_t i = 0; i < 6; i++) {
        TgwBuf conv = {0}, r1 = {0}, r2 = {0};
        if (tgw_convert(rounds[i], strlen(rounds[i]), &conv, TGW_JSON5, &err) ||
            tgw_render(rounds[i], strlen(rounds[i]), &r1, 0, TGW_JSON5, &err) ||
            tgw_render((char *)conv.data, conv.len, &r2, 0, TGW_TGW, &err) || r1.len != r2.len ||
            memcmp(r1.data, r2.data, r1.len)) {
            fprintf(stderr, "FAIL round render %zu: %s\n", i, err.message);
            g_fail++;
        }
        tgw_buf_free(&conv);
        tgw_buf_free(&r1);
        tgw_buf_free(&r2);
    }

    const char *native = "clk: P...\ndata: x3.4 => ready \"two words\" ; phase=.25\n";
    if (tgw_render(native, strlen(native), &a, 2, TGW_TGW, &err) || !contains(&a, "two words")) fail("explicit tgw");
    a.len = 3;
    if (!tgw_render(native, strlen(native), &a, 0, TGW_JSON5, &err) || a.len) fail("forced json5 rejects tgw");
    const char bom[] = "\xef\xbb\xbf"
                       "clk: p...\n";
    if (tgw_render(bom, sizeof bom - 1, &a, 0, TGW_AUTO, &err)) fail("bom");

    tgw_buf_free(&a);
    tgw_buf_free(&b);
    tgw_buf_free(&c);
}

static void check_cli(void) {
    if (system("./build/tgw --help >/tmp/tgw-help.out") != 0) fail("help");
    if (system("./build/tgw -v >/tmp/tgw-ver.out") != 0) fail("version");
    FILE *f = fopen("/tmp/tgw-ver.out", "rb");
    char ver[32] = {0};
    if (!f || !fgets(ver, sizeof ver, f) || strcmp(ver, "0.1.0\n")) fail("version text");
    if (f) fclose(f);
    int bad = 0;
    const char *args[] = {"a b",
                          "a -i b",
                          "-o a -o b",
                          "--input",
                          "--format xml",
                          "--indent -1",
                          "--indent=256",
                          "--unknown",
                          "--convert=yes"};
    for (size_t i = 0; i < sizeof args / sizeof args[0]; i++) {
        char cmd[128];
        snprintf(cmd, sizeof cmd, "./build/tgw %s >/dev/null 2>&1", args[i]);
        if (system(cmd) == 0) {
            fprintf(stderr, "FAIL cli accepted %s\n", args[i]);
            bad++;
        }
    }
    g_fail += bad;
    FILE *w = fopen("/tmp/clock.tgw", "wb");
    fputs("@title test\n时钟: p?\n", w);
    fclose(w);
    if (system("./build/tgw /tmp/clock.tgw >/tmp/tgw-diag.out 2>/tmp/tgw-diag.err") == 0) fail("diag should fail");
    size_t n;
    char *err = slurp("/tmp/tgw-diag.err", &n);
    if (!strstr(err, "/tmp/clock.tgw:2:1:") || !strstr(err, "2 | 时钟: p?") || !strstr(err, "^")) {
        fprintf(stderr, "FAIL diag:\n%s\n", err);
        g_fail++;
    }
    free(err);
    w = fopen("/tmp/clock.tgw", "wb");
    fputs("@title test\n     ?\n", w);
    fclose(w);
    system("./build/tgw /tmp/clock.tgw >/dev/null 2>/tmp/tgw-diag.err");
    err = slurp("/tmp/tgw-diag.err", &n);
    if (!strstr(err, "/tmp/clock.tgw:2:6:") || !strstr(err, "2 |      ?") || !strstr(err, "     ^")) {
        fprintf(stderr, "FAIL column diag:\n%s\n", err);
        g_fail++;
    }
    free(err);
    w = fopen("/tmp/longline.tgw", "wb");
    for (int i = 0; i < 10000; i++) fputc('x', w);
    fclose(w);
    system("./build/tgw /tmp/longline.tgw >/dev/null 2>/tmp/tgw-long.err");
    err = slurp("/tmp/tgw-long.err", &n);
    if (n >= 400) {
        fprintf(stderr, "FAIL long diag %zu\n", n);
        g_fail++;
    }
    free(err);
}

int main(void) {
    check_fixtures();
    check_format_strings();
    check_behavior();
    check_cli();
    /* Exercise success and error paths again so a leak tool sees repeated alloc/free. */
    TgwBuf out = {0};
    TgwError err;
    for (int i = 0; i < 30; i++) {
        size_t n;
        char *s = slurp("tests/fixtures/bundles.tgw", &n);
        tgw_render(s, n, &out, 0, TGW_TGW, &err);
        tgw_render(s, n, &out, 2, TGW_TGW, &err);
        tgw_convert(s, n, &out, TGW_TGW, &err);
        tgw_render("{", 1, &out, 0, TGW_JSON5, &err);
        tgw_render("clk: \"oops", 10, &out, 0, TGW_TGW, &err);
        free(s);
    }
    tgw_buf_free(&out);
    if (g_fail) {
        fprintf(stderr, "%d failure(s)\n", g_fail);
        return 1;
    }
    printf("ok\n");
    return 0;
}
