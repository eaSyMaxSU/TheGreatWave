#include "tgw.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static double now_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e6 + (double)ts.tv_nsec / 1e3;
}

static int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

static void measure(const char *name, const char *source, TgwFormat fmt) {
    TgwBuf buf = {0};
    TgwError err;
    size_t n = strlen(source);
    for (int i = 0; i < 10; i++) {
        if (tgw_render(source, n, &buf, 0, fmt, &err)) {
            fprintf(stderr, "%s: %s\n", name, err.message);
            exit(1);
        }
    }
    double samples[200];
    int count = 0;
    double started = now_us();
    while (count < 200 && (count < 30 || now_us() - started < 1e6)) {
        double t = now_us();
        if (tgw_render(source, n, &buf, 0, fmt, &err)) exit(1);
        samples[count++] = now_us() - t;
    }
    qsort(samples, (size_t)count, sizeof(double), cmp_double);
    double median = samples[count / 2];
    double p95 = samples[(count - 1) * 95 / 100];
    printf("%s: %zu bytes, median %.1f us, p95 %.1f us (%d samples)\n", name, buf.len, median, p95, count);
    tgw_buf_free(&buf);
}

int main(void) {
    char *clock = malloc(10001);
    clock[0] = 'p';
    memset(clock + 1, '.', 9999);
    clock[10000] = 0;
    char *held = malloc(10001);
    held[0] = '1';
    memset(held + 1, '.', 9999);
    held[10000] = 0;
    char *bus = malloc(10001);
    for (int i = 0; i < 10000; i++) bus[i] = (char)('2' + (i % 8));
    bus[10000] = 0;

    char *clock_json = malloc(10080);
    char *held_json = malloc(10080);
    char *bus_json = malloc(1200);
    char *crop_json = malloc(10240);
    char *clock_nat = malloc(10016);
    char *held_nat = malloc(10016);
    char *bus_nat = malloc(1100);
    char *crop_nat = malloc(10240);
    snprintf(clock_json, 10080, "{signal:[{name:'clk',wave:'%s'}]}", clock);
    snprintf(held_json, 10080, "{signal:[{name:'held',wave:'%s'}]}", held);
    snprintf(bus_json, 1200, "{signal:[{name:'bus',wave:'%.*s'}]}", 1000, bus);
    snprintf(crop_json, 10240, "{signal:[{name:'bus',wave:'%s'}],config:{hbounds:[4990,5010]}}", bus);
    snprintf(clock_nat, 10016, "clk: %s", clock);
    snprintf(held_nat, 10016, "held: %s", held);
    snprintf(bus_nat, 1100, "bus: %.*s", 1000, bus);
    snprintf(crop_nat, 10240, "@bounds 4990 5010\nbus: %s", bus);

    puts("Repeated release-mode renders; warmed caller-owned output buffer.");
    measure("clock 10,000 cycles [JSON5]", clock_json, TGW_JSON5);
    measure("clock 10,000 cycles [native]", clock_nat, TGW_TGW);
    measure("held 10,000 cycles [JSON5]", held_json, TGW_JSON5);
    measure("held 10,000 cycles [native]", held_nat, TGW_TGW);
    measure("bus 1,000 values [JSON5]", bus_json, TGW_JSON5);
    measure("bus 1,000 values [native]", bus_nat, TGW_TGW);
    measure("bus 10,000 values, 20 visible [JSON5]", crop_json, TGW_JSON5);
    measure("bus 10,000 values, 20 visible [native]", crop_nat, TGW_TGW);
    return 0;
}
