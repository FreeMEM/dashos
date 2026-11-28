/*
    DashOS, la caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L3): la capa de plataforma (plataforma.h) sobre Linux, en
    x86-64 y en AArch64. Sin libc: las llamadas al sistema van a mano, con los números y las estructuras del ABI de
    Linux (las cabeceras uapi), sin código de Linux.

    - La memoria: mmap con MAP_FIXED_NOREPLACE en la dirección que pide la caja (nada de la caja en los 4 GB bajos,
      salvo la RAM del 68k desde $100000). El código traducido, un memfd proyectado dos veces (escribir y ejecutar).
    - La página 0: Linux no deja proyectar nada por debajo de vm.mmap_min_addr (64 KB). Es un memfd de 4 KB, para
      escribir en `rw` y de solo lectura en PAGINA0_VISTA ($10000). En ARM, Emu68 desvía allí las lecturas con
      dirección absoluta (parche 0018); las que llegan por un registro dan SIGSEGV, y el manejador las hace sobre la
      copia. En x86, RelevoJIT la lee por pagina_leer (motor_relevo.c), sin fallos.
    - Los fallos: un manejador de SIGSEGV para todo el proceso. En ARM rellena un marco_fallo desde el ucontext (el
      mismo orden que la sigcontext de arm64: x0-x30, sp, pc, pstate; y el ESR de su registro en __reserved), atiende
      la página 0 y pasa lo demás a quien haya puesto plat_fallos (los chips, motor_emu68.c).
    - Los hilos: clone con la pila que da la caja; plat_hilo_unir espera con futex a que el kernel borre su palabra.
*/

#include <stdint.h>
#include <stddef.h>
#include "caja.h"
#include "plataforma.h"
#if defined(__aarch64__)
#include "fallo_a64_decodifica.h"
#endif

/* --- Las llamadas al sistema --- */

#if defined(__x86_64__)
#define NR_read          0
#define NR_write         1
#define NR_close         3
#define NR_mmap          9
#define NR_munmap        11
#define NR_rt_sigaction  13
#define NR_rt_sigreturn  15
#define NR_mincore       27
#define NR_nanosleep     35
#define NR_clone         56
#define NR_exit          60
#define NR_ftruncate     77
#define NR_futex         202
#define NR_clock_gettime 228
#define NR_exit_group    231
#define NR_openat        257
#define NR_faccessat     269
#define NR_memfd_create  319

static long sys6(long n, long a, long b, long c, long d, long e, long f)
{
    register long r10 __asm__("r10") = d;
    register long r8 __asm__("r8") = e;
    register long r9 __asm__("r9") = f;
    long r = n;
    __asm__ volatile("syscall" : "+a"(r), "+D"(a), "+S"(b), "+d"(c), "+r"(r10), "+r"(r8), "+r"(r9) :: "rcx", "r11", "memory");
    return r;
}
#else
#define NR_openat        56
#define NR_faccessat     48
#define NR_close         57
#define NR_read          63
#define NR_write         64
#define NR_ftruncate     46
#define NR_exit          93
#define NR_exit_group    94
#define NR_futex         98
#define NR_nanosleep     101
#define NR_clock_gettime 113
#define NR_rt_sigaction  134
#define NR_rt_sigreturn  139
#define NR_munmap        215
#define NR_clone         220
#define NR_mmap          222
#define NR_mincore       232
#define NR_memfd_create  279

static long sys6(long n, long a, long b, long c, long d, long e, long f)
{
    register long x8 __asm__("x8") = n;
    register long x0 __asm__("x0") = a;
    register long x1 __asm__("x1") = b;
    register long x2 __asm__("x2") = c;
    register long x3 __asm__("x3") = d;
    register long x4 __asm__("x4") = e;
    register long x5 __asm__("x5") = f;
    __asm__ volatile("svc #0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5) : "memory");
    return x0;
}
#endif

#define sys(n, a, b, c) sys6((n), (long)(a), (long)(b), (long)(c), 0, 0, 0)

#define PROT_READ   1
#define PROT_WRITE  2
#define PROT_EXEC   4
#define MAP_SHARED  0x01
#define MAP_PRIVATE 0x02
#define MAP_FIXED   0x10
#define MAP_ANONYMOUS 0x20
#define MAP_FIXED_NOREPLACE 0x100000
#define CLOCK_REALTIME  0
#define CLOCK_MONOTONIC 1
#define FUTEX_WAIT 0
#define ENOMEM 12

/* Lo que se pide a mmap en una dirección fija: ahí o nada (MAP_FIXED_NOREPLACE no pisa lo que haya). Algunos kernels
   viejos lo toman como una pista y dan otra dirección: también es un error. */
static long proyectar(uintptr_t va, size_t largo, int prot, int flags, long fd)
{
    long r = sys6(NR_mmap, (long)va, (long)largo, prot, flags | MAP_FIXED_NOREPLACE, fd, 0);
    if (r < 0 && r > -4096) return r;
    if ((uintptr_t)r != va) {
        sys(NR_munmap, r, largo, 0);
        return -17;   /* EEXIST */
    }
    return 0;
}

static int prot_de(int permisos)
{
    return (permisos & 1 ? PROT_READ : 0) | (permisos == P_ESCRIBIR ? PROT_WRITE : 0)
        | (permisos == P_EJECUTAR ? PROT_EXEC : 0);
}

/* --- Consola, reloj, final --- */

void plat_iniciar(void)
{
}

/* Lo que dice la caja (di(), y en Rust `di` y `decir!`): a la salida de errores, y solo si se pide (-v). */
int plat_detalle;

long plat_escribir(const char *s, size_t n)
{
    return plat_detalle ? sys(NR_write, 2, s, n) : (long)n;
}

long plat_error(const char *s, size_t n)
{
    return sys(NR_write, 2, s, n);
}

/* Lo que lee el programa de la consola: de la entrada estándar. */
long plat_consola_leer(char *s, size_t n)
{
    long r;
    do r = sys(NR_read, 0, s, n); while (r == -4);   /* EINTR (un Ctrl-C mientras espera) */
    return r;
}

/* Lo que escribe el programa por la dos.library: a la salida estándar. */
long plat_consola(const char *s, size_t n)
{
    return sys(NR_write, 1, s, n);
}

static uint64_t reloj(int cual)
{
    struct { int64_t s, ns; } t = { 0, 0 };
    sys(NR_clock_gettime, cual, &t, 0);
    return (uint64_t)t.s * 1000000000u + (uint64_t)t.ns;
}

uint64_t plat_ahora_us(void)
{
    return reloj(CLOCK_MONOTONIC) / 1000;
}

uint64_t plat_hora_ns(void)
{
    return reloj(CLOCK_REALTIME);
}

void plat_dormir(uint64_t us)
{
    struct { int64_t s, ns; } t = { (int64_t)(us / 1000000), (int64_t)(us % 1000000) * 1000 };
    sys(NR_nanosleep, &t, 0, 0);
}

void plat_acabar(int codigo)
{
    sys(NR_exit_group, codigo, 0, 0);
    for (;;) {}
}

/* --- Memoria --- */

/* Lo proyectado por plat_memoria, para quitarlo (munmap quiere el largo): la llave es el sitio en la tabla + 1. */
#define TROZOS 256
static struct { uintptr_t va; size_t largo; } trozos[TROZOS];

long plat_memoria(uintptr_t va, size_t largo, int permisos)
{
    int i = 0;
    while (i < TROZOS && trozos[i].largo) i++;
    if (i == TROZOS) return -ENOMEM;
    long r = proyectar(va, largo, prot_de(permisos), MAP_PRIVATE | MAP_ANONYMOUS, -1);
    if (r) return r;
    trozos[i].va = va;
    trozos[i].largo = largo;
    return i + 1;
}

void plat_memoria_quitar(uintptr_t va, long llave)
{
    if (llave < 1 || llave > TROZOS || trozos[llave - 1].va != va) return;
    sys(NR_munmap, va, trozos[llave - 1].largo, 0);
    trozos[llave - 1].largo = 0;
}

int plat_hay_memoria(uintptr_t p)
{
    unsigned char v;
    return sys(NR_mincore, p & ~4095UL, 4096, &v) != -ENOMEM;
}

/* El código traducido: un memfd, para escribir en `rw` y para ejecutar en `rx`. */
long plat_codigo(uintptr_t rw, uintptr_t rx, size_t largo)
{
    long fd = sys(NR_memfd_create, "caja-codigo", 0, 0);
    if (fd < 0) return fd;
    long r = sys(NR_ftruncate, fd, largo, 0);
    if (!r) r = proyectar(rw, largo, PROT_READ | PROT_WRITE, MAP_SHARED, fd);
    if (!r) r = proyectar(rx, largo, PROT_READ | PROT_EXEC, MAP_SHARED, fd);
    if (r) {
        sys(NR_close, fd, 0, 0);
        return r;
    }
    return fd + 1;   /* > 0 también si fd fuera 0 */
}

/* Linux no impide proyectar algo para escribir y ejecutar a la vez: no se comprueba (es cosa del sistema). */
int plat_codigo_wx(long llave, uintptr_t va)
{
    (void)llave;
    (void)va;
    return 0;
}

#if defined(__x86_64__)
/* La memoria de código de RelevoJIT (relevo_code_memory, la arma motor_relevo.c; en DashOS la da pegamento/src/jit.rs):
   para escribir en 384 GB y para ejecutar en 388 GB, como en DashOS. x86 ve lo escrito sin más. */
#define CODIGO_RW  0x0000006000000000UL
#define CODIGO_RX  0x0000006100000000UL
#define CODIGO_MAX (16UL << 20)
static size_t codigo_tam;

int caja_codigo_proyectar(void *ctx, size_t tam, void **rw, void **rx)
{
    (void)ctx;
    if (codigo_tam || tam > CODIGO_MAX || plat_codigo(CODIGO_RW, CODIGO_RX, tam) <= 0) return -1;
    codigo_tam = tam;
    *rw = (void *)CODIGO_RW;
    *rx = (void *)CODIGO_RX;
    return 0;
}

void caja_codigo_quitar(void *ctx, void *rw, void *rx, size_t tam)
{
    (void)ctx;
    (void)rw;
    (void)rx;
    (void)tam;
    if (!codigo_tam) return;
    sys(NR_munmap, CODIGO_RW, codigo_tam, 0);
    sys(NR_munmap, CODIGO_RX, codigo_tam, 0);
    codigo_tam = 0;
}

void caja_codigo_escrito(void *ctx, size_t desde, size_t tam)
{
    (void)ctx;
    (void)desde;
    (void)tam;
    __asm__ volatile("" ::: "memory");
}
#endif

/* --- La página 0 --- */

static uintptr_t pagina0_rw;

long plat_pagina0_crear(uintptr_t rw)
{
    long fd = sys(NR_memfd_create, "caja-pagina0", 0, 0);
    if (fd < 0) return fd;
    long r = sys(NR_ftruncate, fd, 4096, 0);
    if (!r) r = proyectar(rw, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd);
    if (r) {
        sys(NR_close, fd, 0, 0);
        return r;
    }
    pagina0_rw = rw;
    return fd + 1;
}

static long instalar_manejador(void);

long plat_pagina0_poner(long llave)
{
    long r = proyectar(PAGINA0_VISTA, 4096, PROT_READ, MAP_SHARED, llave - 1);
    return r ? r : instalar_manejador();
}

/* Que nadie pueda proyectar nada en la dirección 0 ni en el resto de los primeros 64 KB (vm.mmap_min_addr). */
int plat_pagina0_cerrada(long llave)
{
    return proyectar(0, 4096, PROT_READ, MAP_SHARED, llave - 1) != 0
        && proyectar(0x1000, 4096, PROT_READ, MAP_SHARED, llave - 1) != 0;
}

/* --- Hilos --- */

#define CLONE_VM             0x00000100
#define CLONE_FS             0x00000200
#define CLONE_FILES          0x00000400
#define CLONE_SIGHAND        0x00000800
#define CLONE_THREAD         0x00010000
#define CLONE_SYSVSEM        0x00040000
#define CLONE_CHILD_CLEARTID 0x00200000

/* clone(flags, pila, palabra, f, arg): el hijo corre f(arg) en la pila nueva y acaba el hilo. */
long plat_clone(long flags, uintptr_t pila, volatile int *palabra, void (*f)(uint64_t), uint64_t arg);
#if defined(__x86_64__)
/* clone en x86-64: rdi flags, rsi pila, rdx ptid, r10 ctid, r8 tls. */
__asm__(".text\n.balign 16\nplat_clone:\n"
        " sub $16, %rsi\n mov %rcx, (%rsi)\n mov %r8, 8(%rsi)\n"
        " mov %rdx, %r10\n xor %edx, %edx\n xor %r8d, %r8d\n"
        " mov $56, %eax\n syscall\n test %rax, %rax\n jnz 1f\n"
        " pop %rax\n pop %rdi\n and $-16, %rsp\n call *%rax\n"
        " xor %edi, %edi\n mov $60, %eax\n syscall\n ud2\n"
        "1: ret\n");
#else
/* clone en arm64: x0 flags, x1 pila, x2 ptid, x3 tls, x4 ctid. */
__asm__(".text\n.balign 16\nplat_clone:\n"
        " stp x3, x4, [x1, #-16]!\n mov x4, x2\n mov x2, #0\n mov x3, #0\n"
        " mov x8, #220\n svc #0\n cbnz x0, 1f\n"
        " ldp x3, x0, [sp], #16\n blr x3\n"
        " mov x0, #0\n mov x8, #93\n svc #0\n udf #0\n"
        "1: ret\n");
#endif

#define HILOS 16
static volatile int vivos[HILOS];   /* 1 mientras el hilo vive; el kernel la pone a 0 al acabar (CLONE_CHILD_CLEARTID) */

long plat_hilo_crear(void (*f)(uint64_t), uintptr_t pila_tope, uint64_t arg)
{
    int i = 0;
    while (i < HILOS && vivos[i]) i++;
    if (i == HILOS) return -11;   /* EAGAIN */
    vivos[i] = 1;
    long r = plat_clone(CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM
                        | CLONE_CHILD_CLEARTID, pila_tope & ~15UL, &vivos[i], f, arg);
    if (r < 0) {
        vivos[i] = 0;
        return r;
    }
    return i;
}

void plat_hilo_unir(long h)
{
    if (h < 0 || h >= HILOS) return;
    while (vivos[h]) sys6(NR_futex, (long)&vivos[h], FUTEX_WAIT, 1, 0, 0, 0);
}

void plat_hilo_salir(void)
{
    sys(NR_exit, 0, 0, 0);
    for (;;) {}
}

/* --- Ficheros (para cargar el programa 68k que se pide) --- */

#define AT_FDCWD (-100)

long plat_fichero_leer(const char *ruta, void *buf, size_t max)
{
    long fd = sys6(NR_openat, AT_FDCWD, (long)ruta, 0, 0, 0, 0);
    if (fd < 0) return fd;
    size_t hecho = 0;
    while (hecho < max) {
        long n = sys(NR_read, fd, (uint8_t *)buf + hecho, max - hecho);
        if (n < 0) {
            sys(NR_close, fd, 0, 0);
            return n;
        }
        if (n == 0) break;
        hecho += (size_t)n;
    }
    sys(NR_close, fd, 0, 0);
    return (long)hecho;
}

/* 1 si `ruta` existe (sin mirar permisos). */
int plat_existe(const char *ruta)
{
    return sys6(NR_faccessat, AT_FDCWD, (long)ruta, 0, 0, 0, 0) == 0;
}

/* --- Los fallos (SIGSEGV) --- */

#define SIGSEGV 11
#define SA_SIGINFO  0x00000004UL
#define SA_ONSTACK  0x08000000UL
#define SA_RESTORER 0x04000000UL
#define SA_NODEFER  0x40000000UL

static void decir(const char *s)
{
    size_t n = 0;
    while (s[n]) n++;
    plat_error(s, n);
}

static void decir_hex(uint64_t v)
{
    char b[19] = "0x";
    for (int i = 0; i < 16; i++) b[2 + i] = "0123456789abcdef"[(v >> (60 - 4 * i)) & 15];
    b[18] = 0;
    decir(b);
}

#if defined(__aarch64__)
static int (*atiende_fallo)(struct marco_fallo *m);
uint64_t plat_pagina0_fallos;

long plat_fallos(int (*atiende)(struct marco_fallo *m))
{
    atiende_fallo = atiende;
    return 0;
}

/* La sigcontext de arm64, desde el byte 176 de la ucontext: fault_address, x0-x30, sp, pc, pstate y, alineado a 16,
   __reserved: registros con cabecera {magic, size}; el del ESR, ESR_MAGIC. */
#define UC_SIGCONTEXT 176
#define ESR_MAGIC 0x45535201u
struct sigcontexto {
    uint64_t fault_address;
    uint64_t regs[31];
    uint64_t sp, pc, pstate;
    uint8_t reservado[4096] __attribute__((aligned(16)));
};

static uint64_t esr_de(const struct sigcontexto *sc)
{
    const uint8_t *p = sc->reservado, *fin = sc->reservado + sizeof sc->reservado;
    while (p + 8 <= fin) {
        uint32_t magic = *(const uint32_t *)p, largo = *(const uint32_t *)(p + 4);
        if (!magic || largo < 8) break;
        if (magic == ESR_MAGIC) return *(const uint64_t *)(p + 8);
        p += largo;
    }
    return 0;
}

/* Una lectura de la página 0 (por un registro), sobre la copia: 1 si lo era. */
static int pagina0_atiende(marco_fallo *m, uint32_t ins)
{
    struct acceso a;
    if (m->far >= 0x1000 || !decodifica(ins, m, &a) || a.escribe || a.dir + a.tam * a.pares > 0x1000 || m->far < a.dir)
        return 0;
    const uint8_t *p0 = (const uint8_t *)pagina0_rw;
    unsigned regs[2] = { a.rt, a.rt2 };
    for (int k = 0; k < a.pares; k++) {
        union { uint8_t b[8]; uint16_t h; uint32_t w; uint64_t d; } t = { .d = 0 };
        for (unsigned b = 0; b < a.tam; b++) t.b[b] = p0[a.dir + k * a.tam + b];
        uint64_t v = a.tam == 1 ? t.b[0] : a.tam == 2 ? t.h : a.tam == 4 ? t.w : t.d;
        if (a.signo) v = (uint64_t)extiende(v, 8 * a.tam);
        if (regs[k] != 31) m->x[regs[k]] = a.x ? v : (uint32_t)v;
    }
    if (a.actualiza) {
        if (a.rn == 31) m->sp = a.base_nueva;
        else m->x[a.rn] = a.base_nueva;
    }
    if (a.estado >= 0 && a.estado != 31) m->x[a.estado] = 0;
    m->pc += 4;
    plat_pagina0_fallos++;
    return 1;
}

static void manejador(int sig, void *info, void *uc)
{
    (void)sig;
    (void)info;
    struct sigcontexto *sc = (struct sigcontexto *)((uint8_t *)uc + UC_SIGCONTEXT);
    marco_fallo m;
    for (int i = 0; i < 31; i++) m.x[i] = sc->regs[i];
    m.sp = sc->sp;
    m.pc = sc->pc;
    m.pstate = sc->pstate;
    m.esr = esr_de(sc);
    m.far = sc->fault_address;
    uint32_t ins = *(const volatile uint32_t *)m.pc;
#ifdef __AARCH64EB__
    ins = __builtin_bswap32(ins);                     /* las instrucciones de A64 van siempre en little-endian */
#endif
    if (pagina0_atiende(&m, ins) || (atiende_fallo && atiende_fallo(&m))) {
        for (int i = 0; i < 31; i++) sc->regs[i] = m.x[i];
        sc->sp = m.sp;
        sc->pc = m.pc;
        sc->pstate = m.pstate;
        return;
    }
    decir("  [caja-amiga] fallo del programa en ");
    decir_hex(m.far);
    decir(m.far < 0x10000 ? (m.far < 0x1000 ? " (escritura en la página 0)" : " (los primeros 64 KB)") : "");
    decir(", PC ");
    decir_hex(m.pc);
    decir("\n");
    plat_acabar(3);
}
#else
long plat_fallos(int (*atiende)(struct marco_fallo *m))
{
    (void)atiende;
    return -1;   /* en x86, RelevoJIT no usa el fallo de página (los chips y la página 0 van por sus páginas) */
}

static void manejador(int sig, void *info, void *uc)
{
    (void)sig;
    (void)uc;
    uint64_t far = *(const uint64_t *)((const uint8_t *)info + 16);   /* siginfo.si_addr */
    decir("  [caja-amiga] fallo en ");
    decir_hex(far);
    decir("\n");
    plat_acabar(3);
}
#endif

/* El regreso de la señal (rt_sigreturn), sin prólogo: en ensamblador suelto. */
void plat_restaura(void);
#if defined(__x86_64__)
__asm__(".text\n.balign 16\nplat_restaura:\n mov $15, %eax\n syscall\n ud2\n");
#else
__asm__(".text\n.balign 16\nplat_restaura:\n mov x8, #139\n svc #0\n udf #0\n");
#endif

#define SIGINT 2
static void (*al_interrumpir)(void);

static void manejador_sigint(int sig, void *info, void *uc)
{
    (void)sig;
    (void)info;
    (void)uc;
    if (al_interrumpir) al_interrumpir();
}

/* Ctrl-C en la terminal (SIGINT): llama a `f` (en el hilo que lo reciba, como un manejador de señal). */
long plat_al_interrumpir(void (*f)(void))
{
    al_interrumpir = f;
    struct { void *h; unsigned long flags; void *restorer; uint64_t mask; } sa = {
        (void *)manejador_sigint, SA_SIGINFO | SA_RESTORER, (void *)plat_restaura, 0 };
    return sys6(NR_rt_sigaction, SIGINT, (long)&sa, 0, 8, 0, 0);
}

static long instalar_manejador(void)
{
    struct { void *h; unsigned long flags; void *restorer; uint64_t mask; } sa = {
        (void *)manejador, SA_SIGINFO | SA_RESTORER | SA_NODEFER, (void *)plat_restaura, 0 };
    return sys6(NR_rt_sigaction, SIGSEGV, (long)&sa, 0, 8, 0, 0);
}
