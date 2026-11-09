/* Banco de medidas de la multitarea en Linux (paso L de docs/MULTITAREA_LOOP.md): las mismas pruebas que
 * usuario/src/bin/medir.rs, con la misma unidad de trabajo, para comparar DashOS con el kernel de Raspberry Pi OS en
 * el mismo QEMU raspi4b. Va como /init de un initramfs (tools/medir_linux/hacer.sh) y escribe líneas "[medir] ..."
 * que lee tools/medir.py. Procesos con fork y tuberías; el núcleo se fija con sched_setaffinity. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/reboot.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static uint64_t cuentas(void) {
    uint64_t t;
    __asm__ volatile("isb; mrs %0, cntvct_el0" : "=r"(t));
    return t;
}
static uint64_t frecuencia(void) {
    uint64_t f;
    __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(f));
    return f ? f : 1;
}
static uint64_t a_us(uint64_t c) { return (unsigned __int128)c * 1000000 / frecuencia(); }
static uint64_t de_us(uint64_t us) { return (unsigned __int128)us * frecuencia() / 1000000; }

/* La unidad de trabajo: 256 pasos de un congruencial de 32 bits, como en Rust, C y Dash de DashOS. */
__attribute__((noinline)) static uint32_t unidad(uint32_t x) {
    for (int i = 0; i < 256; i++) x = x * 1664525u + 1013904223u;
    return x;
}

static void fijar(int nucleo) {
    if (nucleo < 0) return;
    cpu_set_t c;
    CPU_ZERO(&c);
    CPU_SET(nucleo, &c);
    sched_setaffinity(0, sizeof c, &c);
}

struct calc { uint64_t unidades, hueco_max_us, huecos_us, n0, n1; };

static struct calc calcular(uint64_t ms) {
    struct calc r = {0};
    r.n0 = sched_getcpu();
    uint64_t umbral = de_us(200), t0 = cuentas(), fin = t0 + de_us(ms * 1000), ant = t0, hm = 0, hs = 0;
    uint32_t acc = 1;
    for (;;) {
        acc = unidad(acc);
        r.unidades++;
        uint64_t t = cuentas(), d = t - ant;
        if (d > umbral) { hs += d; if (d > hm) hm = d; }
        ant = t;
        if (t >= fin) break;
    }
    __asm__ volatile("" ::"r"(acc));
    r.hueco_max_us = a_us(hm);
    r.huecos_us = a_us(hs);
    r.n1 = sched_getcpu();
    return r;
}

/* Un hijo que calcula `ms` en el núcleo `nucleo` (-1: donde lo ponga Linux) y manda su resultado por `fd`. */
static pid_t hijo_calc(int nucleo, uint64_t ms, int fd, uint64_t dormir_antes_ms) {
    pid_t p = fork();
    if (p == 0) {
        fijar(nucleo);
        if (dormir_antes_ms) {
            struct timespec ts = {dormir_antes_ms / 1000, (dormir_antes_ms % 1000) * 1000000};
            nanosleep(&ts, 0);
        }
        struct calc r = calcular(ms);
        if (fd >= 0) write(fd, &r, sizeof r);
        _exit(0);
    }
    return p;
}

static void esperar_todos(void) {
    while (wait(0) > 0) {}
}

static void ociosos(void) {
    /* Interrupciones del temporizador por núcleo (/proc/interrupts, la línea de arch_timer) y tiempo ocioso
     * (/proc/stat), en un segundo sin nada que hacer. */
    uint64_t tics0[4] = {0}, tics1[4] = {0}, ocio0[4] = {0}, ocio1[4] = {0};
    for (int vuelta = 0; vuelta < 2; vuelta++) {
        uint64_t *tics = vuelta ? tics1 : tics0, *ocio = vuelta ? ocio1 : ocio0;
        FILE *f = fopen("/proc/interrupts", "r");
        char l[512];
        while (f && fgets(l, sizeof l, f)) {
            if (!strstr(l, "arch_timer")) continue;
            char *p = strchr(l, ':');
            for (int n = 0; p && n < 4; n++) tics[n] += strtoull(p + 1, &p, 10);
        }
        if (f) fclose(f);
        f = fopen("/proc/stat", "r");
        while (f && fgets(l, sizeof l, f)) {
            int n;
            unsigned long long u, ni, s, id;
            if (sscanf(l, "cpu%d %llu %llu %llu %llu", &n, &u, &ni, &s, &id) == 5 && n >= 0 && n < 4) ocio[n] = id;
        }
        if (f) fclose(f);
        if (!vuelta) sleep(1);
    }
    long hz = sysconf(_SC_CLK_TCK);
    printf("[medir] ociosos");
    for (int n = 0; n < 4; n++)
        printf(" tics%d=%llu ocio%d=%llu", n, (unsigned long long)(tics1[n] - tics0[n]), n,
               (unsigned long long)((ocio1[n] - ocio0[n]) * 1000 / hz));
    printf("\n");
}

/* P1: idas y vueltas de 8 bytes por dos tuberías. juntos: 0 = núcleos 2 y 3, 1 = los dos en el 1, 2 = donde los ponga
 * Linux (con `carga` calculando a la vez). */
static void ida_vuelta(int juntos, int carga, uint64_t rondas) {
    for (int i = 0; i < carga; i++) hijo_calc(-1, 4000, -1, 0);
    int ida[2], vuelta[2], res[2];
    pipe(ida); pipe(vuelta); pipe(res);
    int n_eco = juntos == 0 ? 2 : juntos == 1 ? 1 : -1, n_ping = juntos == 0 ? 3 : juntos == 1 ? 1 : -1;
    pid_t eco = fork();
    if (eco == 0) {
        fijar(n_eco);
        uint64_t m;
        while (read(ida[0], &m, 8) == 8) write(vuelta[1], &m, 8);
        _exit(0);
    }
    pid_t ping = fork();
    if (ping == 0) {
        fijar(n_ping);
        uint64_t m = 0, t0 = cuentas();
        for (uint64_t i = 0; i < rondas; i++) {
            m = i;
            write(ida[1], &m, 8);
            read(vuelta[0], &m, 8);
        }
        uint64_t r[2] = {a_us(cuentas() - t0), sched_getcpu()};
        write(res[1], r, sizeof r);
        _exit(0);
    }
    uint64_t r[2] = {0};
    read(res[0], r, sizeof r);
    close(ida[1]);
    close(ida[0]); close(vuelta[0]); close(vuelta[1]); close(res[0]); close(res[1]);
    kill(eco, SIGKILL);
    for (int i = 0; i < carga + 2; i++) wait(0);
    printf("[medir] ida-vuelta lenguaje=linux juntos=%d carga=%d rondas=%llu ns=%llu nucleos=%d-%llu\n", juntos, carga,
           (unsigned long long)rondas, (unsigned long long)(r[0] * 1000 / rondas), n_eco < 0 ? 9 : n_eco,
           (unsigned long long)r[1]);
}

/* P2: n hijos calculan ms a la vez, donde los ponga Linux. */
static void reparto(int n, uint64_t ms) {
    int t[2];
    pipe(t);
    for (int i = 0; i < n; i++) hijo_calc(-1, ms, t[1], 0);
    uint64_t total = 0, mn = UINT64_MAX, mx = 0, hueco = 0;
    char nucleos[64] = "";
    for (int i = 0; i < n; i++) {
        struct calc r;
        read(t[0], &r, sizeof r);
        total += r.unidades;
        if (r.unidades < mn) mn = r.unidades;
        if (r.unidades > mx) mx = r.unidades;
        if (r.hueco_max_us > hueco) hueco = r.hueco_max_us;
        char x[16];
        snprintf(x, sizeof x, r.n0 == r.n1 ? "%s%llu" : "%s%llu>%llu", i ? "," : "", (unsigned long long)r.n0,
                 (unsigned long long)r.n1);
        strncat(nucleos, x, sizeof nucleos - strlen(nucleos) - 1);
    }
    struct rusage ru;
    esperar_todos();
    getrusage(RUSAGE_CHILDREN, &ru);
    static long antes = 0;
    long cambios = ru.ru_nivcsw - antes;
    antes = ru.ru_nivcsw;
    close(t[0]); close(t[1]);
    printf("[medir] reparto lenguaje=linux n=%d ms=%llu total=%llu min=%llu max=%llu justo=%llu hueco_max_us=%llu "
           "cambios=%ld nucleos=%s\n", n, (unsigned long long)ms, (unsigned long long)total, (unsigned long long)mn,
           (unsigned long long)mx, (unsigned long long)(mx ? mn * 1000 / mx : 0), (unsigned long long)hueco, cambios,
           nucleos);
}

static int cmp(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return x < y ? -1 : x > y;
}

/* P3: veces esperas de us microsegundos (clock_nanosleep relativo), con carga hijos calculando. */
static void temporizador(uint64_t us, int veces, int carga, int imprimir) {
    for (int i = 0; i < carga; i++) hijo_calc(-1, veces * us / 1000 * 3 + 2000, -1, 0);
    static uint64_t r[512];
    for (int i = 0; i < veces; i++) {
        uint64_t t0 = cuentas();
        struct timespec ts = {0, us * 1000};
        clock_nanosleep(CLOCK_MONOTONIC, 0, &ts, 0);
        uint64_t d = a_us(cuentas() - t0);
        r[i] = d > us ? d - us : 0;
    }
    if (carga) {
        signal(SIGTERM, SIG_IGN);
        kill(0, SIGTERM); /* los de carga calculan de más: se les para */
        signal(SIGTERM, SIG_DFL);
    }
    esperar_todos();
    qsort(r, veces, sizeof r[0], cmp);
    if (imprimir)
        printf("[medir] temporizador us=%llu veces=%d carga=%d p50_us=%llu p99_us=%llu max_us=%llu\n",
               (unsigned long long)us, veces, carga, (unsigned long long)r[(veces - 1) * 50 / 100],
               (unsigned long long)r[(veces - 1) * 99 / 100], (unsigned long long)r[veces - 1]);
}

/* P4: en el mismo núcleo, uno calcula mientras otro duerme dormir_ms y luego calcula 1 s. */
static void dormilon(uint64_t dormir_ms) {
    int tv[2], td[2];
    pipe(tv); pipe(td);
    hijo_calc(1, dormir_ms + 2000, tv[1], 0);
    hijo_calc(1, 1000, td[1], dormir_ms);
    struct calc v, d;
    read(tv[0], &v, sizeof v);
    read(td[0], &d, sizeof d);
    esperar_todos();
    printf("[medir] dormilon dormir_ms=%llu hueco_max_us=%llu huecos_us=%llu unidades_dormilon=%llu nucleo=1\n",
           (unsigned long long)dormir_ms, (unsigned long long)v.hueco_max_us, (unsigned long long)v.huecos_us,
           (unsigned long long)d.unidades);
}

int main(void) {
    mount("proc", "/proc", "proc", 0, 0);
    /* Sin /dev/console en el initramfs, la salida no llega: devtmpfs y la consola a mano. */
    mount("devtmpfs", "/dev", "devtmpfs", 0, 0);
    int c = open("/dev/console", O_RDWR);
    if (c >= 0) { dup2(c, 0); dup2(c, 1); dup2(c, 2); }
    setvbuf(stdout, 0, _IOLBF, 0);
    printf("[medir] empieza\n");
    FILE *v = fopen("/proc/version", "r");
    char l[256];
    if (v && fgets(l, sizeof l, v)) printf("  %s", l);
    if (v) fclose(v);
    ociosos();
    ida_vuelta(0, 0, 20000);
    ida_vuelta(1, 0, 20000);
    ida_vuelta(2, 4, 5000);
    int ns[] = {1, 2, 3, 4, 5, 6, 8};
    for (unsigned i = 0; i < sizeof ns / sizeof ns[0]; i++) reparto(ns[i], 2000);
    temporizador(1000, 50, 0, 0);
    temporizador(1000, 300, 0, 1);
    temporizador(1000, 300, 4, 1);
    dormilon(2000);
    printf("[medir] fin\n");
    sync();
    reboot(RB_POWER_OFF);
    for (;;) pause();
}
