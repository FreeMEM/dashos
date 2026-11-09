/* Hijo del banco de medidas de la multitarea (M0, docs/MULTITAREA_LOOP.md), en C sin biblioteca: habla con el
   orquestador `medir` igual que usuario/src/bin/medir_hijo.rs (órdenes eco, ping, calc y fin; las demás, solo en
   Rust). Llave 1: el canal con el orquestador; llave 2 (o la que llegue con una orden): el del otro hijo.

   clang --target=aarch64-none-elf -O2 -ffreestanding -nostdlib -fno-pic, enlazado con ld.lld en 0x400000. */

typedef unsigned long u64;
typedef long i64;
typedef unsigned int u32;

static i64 svc(u64 n, u64 a, u64 b, u64 c, u64 d, u64 e)
{
    register u64 x8 __asm__("x8") = n;
    register u64 x0 __asm__("x0") = a;
    register u64 x1 __asm__("x1") = b;
    register u64 x2 __asm__("x2") = c;
    register u64 x3 __asm__("x3") = d;
    register u64 x4 __asm__("x4") = e;
    __asm__ volatile("svc #0" : "+r"(x0), "+r"(x1) : "r"(x8), "r"(x2), "r"(x3), "r"(x4) : "memory");
    return (i64)x0;
}

static i64 escribir(u32 llave, const void *p, u64 n) { return svc(4, llave, (u64)p, n, 0, 0); }
static i64 leer(u32 llave, void *p, u64 n) { return svc(5, llave, (u64)p, n, 0, 0); }
/* Como leer, y deja en *llave la primera llave que llegue con el mensaje (si llega alguna). */
static i64 leer_llave(u32 canal, void *p, u64 n, u32 *llave)
{
    register u64 x8 __asm__("x8") = 5;
    register u64 x0 __asm__("x0") = canal;
    register u64 x1 __asm__("x1") = (u64)p;
    register u64 x2 __asm__("x2") = n;
    register u64 x3 __asm__("x3") = (u64)llave;
    register u64 x4 __asm__("x4") = 1;
    __asm__ volatile("svc #0" : "+r"(x0), "+r"(x1) : "r"(x8), "r"(x2), "r"(x3), "r"(x4) : "memory");
    return (i64)x0;
}
static void cerrar(u32 llave) { svc(6, llave, 0, 0, 0, 0); }
static u64 nucleo(void) { return (u64)svc(29, 0, 0, 0, 0, 0); }

static u64 cuentas(void)
{
    u64 t;
    __asm__ volatile("isb; mrs %0, cntvct_el0" : "=r"(t));
    return t;
}

static u64 frecuencia(void)
{
    u64 f;
    __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(f));
    return f ? f : 1;
}

/* Sin divisiones de 128 bits (no hay libgcc): la frecuencia es de kHz enteros en la Pi y en QEMU. */
static u64 a_us(u64 c) { return c * 1000 / (frecuencia() / 1000); }
static u64 de_us(u64 us) { return us * (frecuencia() / 1000) / 1000; }

__attribute__((noinline)) static u32 unidad(u32 x)
{
    for (int i = 0; i < 256; i++)
        x = x * 1664525u + 1013904223u;
    return x;
}

/* ---- texto ---- */
static char sal[256];
static int nsal;

static void poner(const char *s)
{
    while (*s && nsal < 255)
        sal[nsal++] = *s++;
}

static void poner_num(u64 v)
{
    char d[24];
    int k = 0;
    do {
        d[k++] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    while (k && nsal < 255)
        sal[nsal++] = d[--k];
}

static void responder(void)
{
    escribir(1, sal, (u64)nsal);
    nsal = 0;
}

static u64 numero(const char *p, int n, int cual)
{
    int i = 0, palabra = 0;
    while (i < n) {
        while (i < n && p[i] == ' ')
            i++;
        if (palabra == cual) {
            u64 v = 0;
            while (i < n && p[i] >= '0' && p[i] <= '9')
                v = v * 10 + (u64)(p[i++] - '0');
            return v;
        }
        while (i < n && p[i] != ' ')
            i++;
        palabra++;
    }
    return 0;
}

static int empieza(const char *p, int n, const char *s)
{
    int i = 0;
    while (s[i]) {
        if (i >= n || p[i] != s[i])
            return 0;
        i++;
    }
    return i == n || p[i] == ' ';
}

static int principal(void)
{
    char b[256];
    u32 otro = 2;
    for (;;) {
        i64 n = leer_llave(1, b, sizeof b, &otro);
        if (n < 0)
            return 0;
        u64 x = numero(b, (int)n, 1);
        if (empieza(b, (int)n, "eco")) {
            char m[256];
            u64 veces = 0;
            for (;;) {
                i64 k = leer(otro, m, sizeof m);
                if (k < 0 || escribir(otro, m, (u64)k) != 0)
                    break;
                veces++;
            }
            poner("eco ");
            poner_num(veces);
            responder();
        } else if (empieza(b, (int)n, "ping")) {
            u64 m = 0;
            u64 t0 = cuentas();
            for (u64 i = 0; i < x; i++) {
                m = i;
                escribir(otro, &m, 8);
                leer(otro, &m, 8);
            }
            u64 us = a_us(cuentas() - t0);
            cerrar(otro);
            poner("ping "); poner_num(x); poner(" "); poner_num(us); poner(" "); poner_num(nucleo());
            responder();
        } else if (empieza(b, (int)n, "calc")) {
            u64 n0 = nucleo(), umbral = de_us(200), t0 = cuentas(), fin = t0 + de_us(x * 1000), ant = t0;
            u64 unidades = 0, hueco_max = 0, huecos = 0;
            u32 acc = 1;
            for (;;) {
                acc = unidad(acc);
                unidades++;
                u64 t = cuentas(), d = t - ant;
                if (d > umbral) {
                    huecos += d;
                    if (d > hueco_max)
                        hueco_max = d;
                }
                ant = t;
                if (t >= fin)
                    break;
            }
            __asm__ volatile("" : : "r"(acc));
            poner("calc "); poner_num(unidades); poner(" "); poner_num(a_us(hueco_max)); poner(" ");
            poner_num(a_us(huecos)); poner(" "); poner_num(n0); poner(" "); poner_num(nucleo());
            responder();
        } else if (empieza(b, (int)n, "nucleo")) {
            poner("nucleo ");
            poner_num(nucleo());
            responder();
        } else {
            return 0;
        }
    }
}

void _start(void)
{
    int r = principal();
    for (;;)
        svc(2, (u64)r, 0, 0, 0, 0);
}
