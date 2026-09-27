#include "common.h"

#include <errno.h>
#include <unistd.h>

static const char HELP[] =
    "The Great Wave — compact timing diagrams\n"
    "\n"
    "Usage: tgw [OPTIONS] [INPUT]\n"
    "\n"
    "INPUT                  Read .tgw or WaveJSON; omit or use - for stdin\n"
    "-i, --input PATH        Input file (alternative to positional INPUT)\n"
    "-o, --output PATH       Write output to a file; omit or use - for stdout\n"
    "--format FORMAT    Input syntax: auto (default), tgw, or json5\n"
    "--convert          Convert input to readable, canonical .tgw text\n"
    "-t, --indent N          Indent SVG output by N spaces (default: compact)\n"
    "-h, --help              Show this help\n"
    "-v, --version           Show version\n"
    "\n"
    "Examples:\n"
    "tgw diagram.tgw -o diagram.svg\n"
    "tgw legacy.json5 --convert -o diagram.tgw\n"
    "tgw --format tgw < diagram.tgw > diagram.svg\n";

typedef struct Options {
    const char *input;
    const char *output;
    TgwFormat format;
    unsigned indent;
    int action; /* 0 render, 1 convert, 2 help, 3 version */
} Options;

static int parse_u8(const char *s, unsigned *out) {
    if (!s || !*s) return -1;
    unsigned v = 0;
    for (const char *p = s; *p; p++) {
        if (*p < '0' || *p > '9') return -1;
        v = v * 10u + (unsigned)(*p - '0');
        if (v > 255) return -1;
    }
    *out = v;
    return 0;
}

static int options(int argc, char **argv, Options *o, char *err, size_t err_n) {
    memset(o, 0, sizeof *o);
    int positional_only = 0;
    for (int i = 1; i < argc; i++) {
        const char *arg = argv[i];
        if (!positional_only && strcmp(arg, "--") == 0) {
            positional_only = 1;
            continue;
        }
        int utf8 = utf8_ok((const uint8_t *)arg, strlen(arg));
        if (!positional_only && utf8 && arg[0] == '-' && strcmp(arg, "-") != 0) {
            const char *flag = arg;
            const char *inline_v = NULL;
            const char *eq = strchr(arg, '=');
            char flagbuf[128];
            if (eq) {
                size_t n = (size_t)(eq - arg);
                if (n >= sizeof flagbuf) n = sizeof flagbuf - 1;
                memcpy(flagbuf, arg, n);
                flagbuf[n] = 0;
                flag = flagbuf;
                inline_v = eq + 1;
            }
            if ((strcmp(flag, "-h") == 0 || strcmp(flag, "--help") == 0) && !inline_v) {
                o->action = 2;
                return 0;
            }
            if ((strcmp(flag, "-v") == 0 || strcmp(flag, "--version") == 0) && !inline_v) {
                o->action = 3;
                return 0;
            }
            if (strcmp(flag, "--convert") == 0 && !inline_v) {
                o->action = 1;
                continue;
            }
            if (strcmp(flag, "-i") == 0 || strcmp(flag, "--input") == 0 || strcmp(flag, "-o") == 0 ||
                strcmp(flag, "--output") == 0 || strcmp(flag, "--format") == 0 || strcmp(flag, "-t") == 0 ||
                strcmp(flag, "--indent") == 0) {
                const char *value = inline_v;
                if (!value) {
                    if (i + 1 >= argc) {
                        snprintf(err, err_n, "%s needs a value", flag);
                        return -1;
                    }
                    value = argv[++i];
                }
                if (!value[0]) {
                    snprintf(err, err_n, "%s needs a value", flag);
                    return -1;
                }
                if (strcmp(flag, "-i") == 0 || strcmp(flag, "--input") == 0) {
                    if (o->input) {
                        snprintf(err, err_n, "provide only one input file");
                        return -1;
                    }
                    o->input = value;
                } else if (strcmp(flag, "-o") == 0 || strcmp(flag, "--output") == 0) {
                    if (o->output) {
                        snprintf(err, err_n, "provide only one output file");
                        return -1;
                    }
                    o->output = value;
                } else if (strcmp(flag, "--format") == 0) {
                    if (strcmp(value, "auto") == 0)
                        o->format = TGW_AUTO;
                    else if (strcmp(value, "tgw") == 0)
                        o->format = TGW_TGW;
                    else if (strcmp(value, "json5") == 0)
                        o->format = TGW_JSON5;
                    else {
                        snprintf(err, err_n, "--format must be auto, tgw, or json5");
                        return -1;
                    }
                } else if (parse_u8(value, &o->indent)) {
                    snprintf(err, err_n, "--indent must be an integer from 0 to 255");
                    return -1;
                }
            } else {
                snprintf(err, err_n, "unknown option %s; use --help for usage", arg);
                return -1;
            }
        } else {
            if (o->input) {
                snprintf(err, err_n, "provide only one input file");
                return -1;
            }
            o->input = arg;
        }
    }
    return 0;
}

static int write_all(int fd, const uint8_t *p, size_t n) {
    while (n) {
        ssize_t w = write(fd, p, n);
        if (w < 0) {
            if (errno == EINTR) continue;
            if (errno == EPIPE) return 0;
            return -1;
        }
        p += (size_t)w;
        n -= (size_t)w;
    }
    return 0;
}

static int read_all(FILE *f, char **out, size_t *n, char *err, size_t err_n, const char *label) {
    size_t cap = 4096, len = 0;
    char *buf = malloc(cap);
    if (!buf) tgw_oom();
    for (;;) {
        if (len + 4096 + 1 > cap) {
            cap *= 2;
            char *p = realloc(buf, cap);
            if (!p) tgw_oom();
            buf = p;
        }
        size_t got = fread(buf + len, 1, 4096, f);
        len += got;
        if (got < 4096) {
            if (ferror(f)) {
                snprintf(err, err_n, "%s: read error", label);
                free(buf);
                return -1;
            }
            break;
        }
    }
    buf[len] = 0;
    if (!utf8_ok((uint8_t *)buf, len)) {
        snprintf(err, err_n, "%s: stream did not contain valid UTF-8", label);
        free(buf);
        return -1;
    }
    *out = buf;
    *n = len;
    return 0;
}

static void diagnostic(const char *label, const char *source, size_t slen, const TgwError *error, Buf *out) {
    size_t offset = error->offset > slen ? slen : error->offset;
    while (offset && ((uint8_t)source[offset] & 0xc0) == 0x80) offset--;
    size_t line = 1;
    size_t start = 0;
    for (size_t i = 0; i < offset; i++)
        if (source[i] == '\n') {
            line++;
            start = i + 1;
        }
    size_t end = offset;
    while (end < slen && source[end] != '\n') end++;
    size_t column = utf8_count(source + start, offset - start);
    size_t left = column > 60 ? column - 60 : 0;
    char gutter_src[32];
    char colbuf[32];
    snprintf(gutter_src, sizeof gutter_src, "%zu", line);
    snprintf(colbuf, sizeof colbuf, "%zu", column + 1);
    buf_puts(out, label);
    buf_push(out, ':');
    buf_puts(out, gutter_src);
    buf_push(out, ':');
    buf_puts(out, colbuf);
    BUF_LIT(out, ": ");
    buf_puts(out, error->message);
    buf_push(out, '\n');
    buf_push(out, ' ');
    buf_puts(out, gutter_src);
    BUF_LIT(out, " | ");
    size_t caret = left ? 1 : 0;
    size_t shown = caret;
    if (left) BUF_LIT(out, "…");
    size_t line_end = end;
    if (line_end && source[line_end - 1] == '\r') line_end--;
    size_t i = start, seen = 0, taken = 0;
    uint32_t cp;
    while (i < line_end) {
        size_t at = i;
        utf8_next(source, line_end, &i, &cp);
        if (seen++ < left) continue;
        if (taken >= 160) break;
        size_t width = 1;
        if (cp == '\t') {
            width = 4 - shown % 4;
            for (size_t k = 0; k < width; k++) buf_push(out, ' ');
        } else if (cp < 0x20 || cp == 0x7f) {
            BUF_LIT(out, "�");
        } else {
            buf_append(out, source + at, i - at);
        }
        if (seen - 1 < column) caret += width;
        shown += width;
        taken++;
    }
    if (utf8_count(source + start, line_end - start) > left + 160) BUF_LIT(out, "…");
    buf_push(out, '\n');
    buf_push(out, ' ');
    for (size_t k = 0; k < strlen(gutter_src); k++) buf_push(out, ' ');
    BUF_LIT(out, " | ");
    for (size_t k = 0; k < caret; k++) buf_push(out, ' ');
    buf_push(out, '^');
}

int main(int argc, char **argv) {
    Options opt;
    char err[512];
    if (options(argc, argv, &opt, err, sizeof err)) {
        fprintf(stderr, "tgw: %s\n", err);
        return 1;
    }
    if (opt.action == 2) {
        if (write_all(1, (const uint8_t *)HELP, sizeof HELP - 1)) return 1;
        return 0;
    }
    if (opt.action == 3) {
        char line[64];
        int n = snprintf(line, sizeof line, "%s\n", TGW_VERSION);
        if (write_all(1, (const uint8_t *)line, (size_t)n)) return 1;
        return 0;
    }
    const char *label = "<stdin>";
    char *source = NULL;
    size_t slen = 0;
    int input_file = opt.input && strcmp(opt.input, "-") != 0;
    if (input_file) {
        label = opt.input;
        FILE *f = fopen(opt.input, "rb");
        if (!f) {
            fprintf(stderr, "tgw: %s: %s\n", opt.input, strerror(errno));
            return 1;
        }
        if (read_all(f, &source, &slen, err, sizeof err, opt.input)) {
            fclose(f);
            fprintf(stderr, "tgw: %s\n", err);
            return 1;
        }
        fclose(f);
    } else {
        if (read_all(stdin, &source, &slen, err, sizeof err, "stdin")) {
            fprintf(stderr, "tgw: %s\n", err);
            return 1;
        }
    }
    TgwBuf output = {0};
    TgwError error;
    memset(&error, 0, sizeof error);
    int rc = opt.action == 1 ? tgw_convert(source, slen, &output, opt.format, &error)
                             : tgw_render(source, slen, &output, opt.indent, opt.format, &error);
    if (rc) {
        uint8_t stack[1024];
        Buf msg;
        buf_init(&msg, stack, sizeof stack);
        diagnostic(label, source, slen, &error, &msg);
        fprintf(stderr, "tgw: %.*s\n", (int)msg.len, (char *)msg.data);
        buf_free(&msg);
        free(source);
        tgw_buf_free(&output);
        return 1;
    }
    if (!output.len || output.data[output.len - 1] != '\n') {
        Buf b = {output.data, output.len, output.cap, output.data != NULL};
        buf_push(&b, '\n');
        output.data = b.data;
        output.len = b.len;
        output.cap = b.cap;
    }
    int status = 0;
    if (opt.output && strcmp(opt.output, "-") != 0) {
        FILE *f = fopen(opt.output, "wb");
        if (!f || fwrite(output.data, 1, output.len, f) != output.len) {
            fprintf(stderr, "tgw: %s: %s\n", opt.output, strerror(errno));
            status = 1;
        }
        if (f) fclose(f);
    } else if (write_all(1, output.data, output.len)) {
        fprintf(stderr, "tgw: stdout: %s\n", strerror(errno));
        status = 1;
    }
    free(source);
    tgw_buf_free(&output);
    return status;
}
