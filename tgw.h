#ifndef TGW_H
#define TGW_H

#include <stddef.h>
#include <stdint.h>

#define TGW_VERSION "0.1.0"

typedef struct TgwBuf {
    unsigned char *data;
    size_t len;
    size_t cap;
} TgwBuf;

typedef struct TgwError {
    size_t offset;
    char message[384];
} TgwError;

typedef enum TgwFormat {
    TGW_AUTO = 0,
    TGW_TGW = 1,
    TGW_JSON5 = 2
} TgwFormat;

/* Render source into out. out is cleared on entry and on failure; its
   capacity is kept so callers can reuse the allocation. Returns 0 on success. */
int tgw_render(const char *src, size_t len, TgwBuf *out, unsigned indent, TgwFormat fmt,
               TgwError *err);

/* Convert source to canonical .tgw text. Same buffer contract as tgw_render. */
int tgw_convert(const char *src, size_t len, TgwBuf *out, TgwFormat fmt, TgwError *err);

void tgw_buf_free(TgwBuf *buf);

const char *tgw_version(void);

#endif
