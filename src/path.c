#include "common.h"

#define MAX_COORD 1.0e9

typedef struct Pt {
    double x, y;
} Pt;

typedef struct Parser {
    const uint8_t *s;
    size_t n, pos;
    int can_comma;
    TgwError *err;
} Parser;

static int perr(Parser *p, const char *text) {
    err_fmt(p->err, p->pos, "invalid piecewise path: %s", text);
    return -1;
}

static int more(Parser *p) {
    while (p->pos < p->n && (p->s[p->pos] == ' ' || p->s[p->pos] == '\t' || p->s[p->pos] == '\n' ||
                             p->s[p->pos] == '\r' || p->s[p->pos] == '\f' || p->s[p->pos] == '\v'))
        p->pos++;
    return p->pos < p->n;
}

static int separator(Parser *p) {
    more(p);
    if (p->pos < p->n && p->s[p->pos] == ',') {
        if (!p->can_comma) return perr(p, "unexpected comma after a path command");
        p->pos++;
        more(p);
    }
    return 0;
}

static int number(Parser *p, double *out) {
    if (separator(p)) return -1;
    size_t start = p->pos;
    if (p->pos < p->n && (p->s[p->pos] == '+' || p->s[p->pos] == '-')) p->pos++;
    int digits = 0;
    while (p->pos < p->n && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') {
        digits++;
        p->pos++;
    }
    if (p->pos < p->n && p->s[p->pos] == '.') {
        p->pos++;
        while (p->pos < p->n && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') {
            digits++;
            p->pos++;
        }
    }
    if (!digits) return perr(p, "expected a number");
    if (p->pos < p->n && (p->s[p->pos] == 'e' || p->s[p->pos] == 'E')) {
        p->pos++;
        if (p->pos < p->n && (p->s[p->pos] == '+' || p->s[p->pos] == '-')) p->pos++;
        size_t exp = p->pos;
        while (p->pos < p->n && p->s[p->pos] >= '0' && p->s[p->pos] <= '9') p->pos++;
        if (p->pos == exp) return perr(p, "expected exponent digits");
    }
    if (!parse_f64_slice((const char *)p->s + start, p->pos - start, out) || fabs(*out) > MAX_COORD)
        return perr(p, "coordinate is out of range");
    p->can_comma = 1;
    return 0;
}

static int check_pt(Parser *p, Pt pt) {
    if (!isfinite(pt.x) || !isfinite(pt.y) || fmax(fabs(pt.x), fabs(pt.y)) > MAX_COORD)
        return perr(p, "coordinate is out of range");
    return 0;
}

static int read_point(Parser *p, Pt origin, Pt *out) {
    double x, y;
    if (number(p, &x) || number(p, &y)) return -1;
    out->x = x + origin.x;
    out->y = y + origin.y;
    return check_pt(p, *out);
}

static int flag(Parser *p, int *out) {
    if (separator(p)) return -1;
    if (p->pos >= p->n || (p->s[p->pos] != '0' && p->s[p->pos] != '1')) return perr(p, "arc flag must be 0 or 1");
    *out = p->s[p->pos] == '1';
    p->pos++;
    p->can_comma = 1;
    return 0;
}

static Pt reflect(Pt control, Pt point) {
    Pt r = {2.0 * point.x - control.x, 2.0 * point.y - control.y};
    return r;
}

static double quad_max(double a, double b, double c) {
    double xmax = a > c ? a : c;
    double den = (b - a) - (c - b);
    if (den != 0.0) {
        double t = (b - a) / den;
        if (t > 0.0 && t < 1.0) {
            double u = (1.0 - t) * (1.0 - t) * a + 2.0 * (1.0 - t) * t * b + t * t * c;
            if (u > xmax) xmax = u;
        }
    }
    return xmax;
}

static double cubic_max(double a, double b, double c, double d) {
    double xmax = a > d ? a : d;
    double d0 = b - a, d1 = c - b, d2 = d - c;
    double aa = d0 - 2.0 * d1 + d2;
    double bb = 2.0 * (d1 - d0);
    double cc = d0;
    double scale = fabs(aa);
    if (fabs(bb) > scale) scale = fabs(bb);
    if (fabs(cc) > scale) scale = fabs(cc);
    if (scale == 0.0) return xmax;
    aa /= scale;
    bb /= scale;
    cc /= scale;
    if (aa == 0.0) {
        if (bb != 0.0) {
            double t = -cc / bb;
            if (t > 0.0 && t < 1.0) {
                double u = 1.0 - t;
                double v = u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
                if (v > xmax) xmax = v;
            }
        }
    } else {
        double disc = bb * bb - 4.0 * aa * cc;
        if (disc >= 0.0) {
            double q = -0.5 * (bb + copysign(sqrt(disc), bb));
            double ts[2];
            int nt = 0;
            ts[nt++] = q / aa;
            if (q != 0.0) ts[nt++] = cc / q;
            for (int i = 0; i < nt; i++) {
                double t = ts[i];
                if (t > 0.0 && t < 1.0) {
                    double u = 1.0 - t;
                    double v = u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
                    if (v > xmax) xmax = v;
                }
            }
        }
    }
    return xmax;
}

static double rem_euclid(double a, double m) {
    double r = fmod(a, m);
    if (r < 0.0) r += fabs(m);
    return r;
}

static int arc_max(Pt a, Pt b, double rx, double ry, double degrees, int large, int sweep, double *xmax) {
    *xmax = a.x > b.x ? a.x : b.x;
    if (rx == 0.0 || ry == 0.0 || (a.x == b.x && a.y == b.y)) return 0;
    double phi = fmod(degrees, 360.0) * (3.14159265358979323846 / 180.0);
    double sn = sin(phi), cs = cos(phi);
    double dx = (a.x - b.x) / 2.0, dy = (a.y - b.y) / 2.0;
    double xp = cs * dx + sn * dy;
    double yp = -sn * dx + cs * dy;
    double correction = hypot(xp / rx, yp / ry);
    if (correction > 1.0) {
        if (isfinite(correction)) {
            rx *= correction;
            ry *= correction;
        } else {
            double lx = log(fabs(xp)) - log(rx);
            double ly = log(fabs(yp)) - log(ry);
            double peak = lx > ly ? lx : ly;
            double log_scale = peak + 0.5 * log(exp(2.0 * (lx - peak)) + exp(2.0 * (ly - peak)));
            rx = exp(log(rx) + log_scale);
            ry = exp(log(ry) + log_scale);
        }
    }
    if (!isfinite(rx) || !isfinite(ry) || (rx > ry ? rx : ry) > 1e12) return -1;
    double ux = xp / rx, uy = yp / ry;
    double norm = hypot(ux, uy);
    if (norm == 0.0 || !isfinite(norm)) return -1;
    double k = sqrt(fmax(0.0, 1.0 - norm * norm)) * (large == sweep ? -1.0 : 1.0);
    double cxp = k * rx * (uy / norm);
    double cyp = -k * ry * (ux / norm);
    double cx = cs * cxp - sn * cyp + (a.x + b.x) / 2.0;
    double begin = atan2((yp - cyp) / ry, (xp - cxp) / rx);
    double end = atan2((-yp - cyp) / ry, (-xp - cxp) / rx);
    const double TAU = 6.28318530717958647692;
    const double PI = 3.14159265358979323846;
    double delta = sweep ? rem_euclid(end - begin, TAU) : -rem_euclid(begin - end, TAU);
    if (large && delta == 0.0) delta = TAU;
    double angle = atan2(-ry * sn, rx * cs);
    double thetas[2] = {angle, angle + PI};
    for (int i = 0; i < 2; i++) {
        double theta = thetas[i];
        double distance = sweep ? rem_euclid(theta - begin, TAU) : rem_euclid(begin - theta, TAU);
        if (distance <= fabs(delta) + 64.0 * 2.2204460492503131e-16) {
            double v = cx + rx * cs * cos(theta) - ry * sn * sin(theta);
            if (v > *xmax) *xmax = v;
        }
    }
    return 0;
}

int tgw_extent(const char *path, size_t n, double *xmax, TgwError *err) {
    Parser p = {(const uint8_t *)path, n, 0, 0, err};
    uint8_t command = 'M';
    uint8_t previous = ' ';
    Pt point = {0, 0}, start = {0, 0}, control = {0, 0};
    double xm = 0;
    while (more(&p)) {
        if ((p.s[p.pos] >= 'A' && p.s[p.pos] <= 'Z') || (p.s[p.pos] >= 'a' && p.s[p.pos] <= 'z')) {
            command = p.s[p.pos];
            p.pos++;
            p.can_comma = 0;
        } else if (previous == ' ') {
            return perr(&p, "path must begin with M");
        } else if (previous == 'Z') {
            return perr(&p, "expected a path command");
        }
        uint8_t kind = (uint8_t)((command >= 'a' && command <= 'z') ? command - 32 : command);
        if (previous == ' ' && kind != 'M') return perr(&p, "path must begin with M");
        int relative = command >= 'a' && command <= 'z';
        Pt origin = relative ? point : (Pt){0, 0};
        Pt old = point;
        if (kind == 'M' || kind == 'L' || kind == 'T') {
            if (read_point(&p, origin, &point)) return -1;
            if (kind == 'M') start = point;
            if (kind == 'T') {
                Pt c = (previous == 'Q' || previous == 'T') ? reflect(control, old) : old;
                double m = quad_max(old.x, c.x, point.x);
                if (m > xm) xm = m;
                control = c;
            }
        } else if (kind == 'H') {
            double v;
            if (number(&p, &v)) return -1;
            point.x = v + origin.x;
        } else if (kind == 'V') {
            double v;
            if (number(&p, &v)) return -1;
            point.y = v + origin.y;
        } else if (kind == 'C' || kind == 'S') {
            Pt c1;
            if (kind == 'C') {
                if (read_point(&p, origin, &c1)) return -1;
            } else if (previous == 'C' || previous == 'S') {
                c1 = reflect(control, old);
            } else {
                c1 = old;
            }
            Pt c2;
            if (read_point(&p, origin, &c2) || read_point(&p, origin, &point)) return -1;
            double m = cubic_max(old.x, c1.x, c2.x, point.x);
            if (m > xm) xm = m;
            control = c2;
        } else if (kind == 'Q') {
            if (read_point(&p, origin, &control) || read_point(&p, origin, &point)) return -1;
            double m = quad_max(old.x, control.x, point.x);
            if (m > xm) xm = m;
        } else if (kind == 'A') {
            double rx, ry, angle;
            int large, sweep;
            if (number(&p, &rx) || number(&p, &ry)) return -1;
            if (rx < 0.0 || ry < 0.0) return perr(&p, "arc radii must be nonnegative");
            if (number(&p, &angle) || flag(&p, &large) || flag(&p, &sweep) || read_point(&p, origin, &point)) return -1;
            double m;
            if (arc_max(old, point, rx, ry, angle, large, sweep, &m)) return perr(&p, "arc geometry is out of range");
            if (m > xm) xm = m;
        } else if (kind == 'Z') {
            point = start;
        } else {
            return perr(&p, "unsupported SVG path command");
        }
        if (check_pt(&p, point)) return -1;
        if (point.x > xm) xm = point.x;
        previous = kind;
        if (kind == 'M') command = relative ? 'l' : 'L';
    }
    *xmax = xm;
    return 0;
}
