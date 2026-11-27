/*
    DashOS, X7: lo que en ARM da src/support.c de Emu68 a la caja, para x86-64: la salida con formato (vkprintf_pc: %d,
    %i, %u, %x, %X, %s, %c, %p y %%, con '-', '0', ancho y l, ll o z), kprintf (solo para los avisos de TLSF), las
    funciones de memoria y el montón general. Fichero propio de DashOS (MPL-2.0).
*/
#include "support.h"
#include "caja.h"

void *tlsf;

static void numero(putc_func p, void *d, uint64_t v, unsigned base, int mayus, int negativo, int ancho, int ceros,
                   int izquierda)
{
    char b[24];
    int n = 0;
    do {
        unsigned c = (unsigned)(v % base);
        b[n++] = (char)(c < 10 ? '0' + c : (mayus ? 'A' : 'a') + c - 10);
        v /= base;
    } while (v);
    int largo = n + negativo;
    if (negativo && ceros) p(d, '-');
    if (!izquierda)
        for (; largo < ancho; largo++) p(d, ceros ? '0' : ' ');
    if (negativo && !ceros) p(d, '-');
    while (n) p(d, b[--n]);
    if (izquierda)
        for (; largo < ancho; largo++) p(d, ' ');
}

void vkprintf_pc(putc_func p, void *d, const char *f, va_list a)
{
    for (; *f; f++) {
        if (*f != '%') { p(d, *f); continue; }
        f++;
        int izquierda = 0, ceros = 0, ancho = 0, largos = 0;
        for (;; f++) {
            if (*f == '-') izquierda = 1;
            else if (*f == '0') ceros = 1;
            else break;
        }
        while (*f >= '0' && *f <= '9') ancho = ancho * 10 + (*f++ - '0');
        while (*f == 'l' || *f == 'z') { largos++; f++; }
        switch (*f) {
        case 'd': case 'i': {
            int64_t v = largos ? va_arg(a, int64_t) : va_arg(a, int);
            numero(p, d, v < 0 ? (uint64_t)0 - (uint64_t)v : (uint64_t)v, 10, 0, v < 0, ancho, ceros, izquierda);
            break;
        }
        case 'u': numero(p, d, largos ? va_arg(a, uint64_t) : va_arg(a, unsigned), 10, 0, 0, ancho, ceros, izquierda); break;
        case 'x': case 'X':
            numero(p, d, largos ? va_arg(a, uint64_t) : va_arg(a, unsigned), 16, *f == 'X', 0, ancho, ceros, izquierda);
            break;
        case 'p':
            p(d, '0'); p(d, 'x');
            numero(p, d, (uintptr_t)va_arg(a, void *), 16, 0, 0, ancho, ceros, izquierda);
            break;
        case 's': {
            const char *s = va_arg(a, const char *);
            if (!s) s = "(null)";
            int n = 0;
            while (s[n]) n++;
            if (!izquierda) for (int i = n; i < ancho; i++) p(d, ' ');
            for (int i = 0; i < n; i++) p(d, s[i]);
            if (izquierda) for (int i = n; i < ancho; i++) p(d, ' ');
            break;
        }
        case 'c': p(d, (char)va_arg(a, int)); break;
        case '%': p(d, '%'); break;
        case 0: return;
        default: p(d, '%'); p(d, *f); break;
        }
    }
}

/* Solo los avisos de TLSF (los de la caja van por di). */
void kprintf(const char *format, ...)
{
    va_list v;
    va_start(v, format);
    for (const char *m = "  [caja-amiga] "; *m; m++) caja_pon_c(0, *m);
    vkprintf_pc(caja_pon_c, 0, format, v);
    va_end(v);
}

void *memcpy(void *d, const void *s, size_t n)
{
    uint8_t *x = d;
    const uint8_t *y = s;
    while (n--) *x++ = *y++;
    return d;
}

void *memmove(void *d, const void *s, size_t n)
{
    uint8_t *x = d;
    const uint8_t *y = s;
    if (x < y) while (n--) *x++ = *y++;
    else while (n--) x[n] = y[n];
    return d;
}

void bzero(void *d, size_t n) { memset(d, 0, n); }

void *memset(void *d, int c, size_t n)
{
    uint8_t *x = d;
    while (n--) *x++ = (uint8_t)c;
    return d;
}
