/*
    DashOS, la caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L2): la capa de plataforma (plataforma.h) sobre el kernel
    de DashOS (docs/referencia/kernel.md), en AArch64 y en x86-64. Sin libc: las llamadas al sistema van a mano.
*/

#include <stdint.h>
#include <stddef.h>
#include "caja.h"
#include "plataforma.h"

#if defined(__x86_64__)
/* x86-64 (X7): rax el número; rdi, rsi, rdx, r10 y r8 los argumentos; el resultado en rax (syscall pisa rcx y r11). */
long caja_sc(long n, long a, long b, long c, long d, long e)
{
    register long r10 __asm__("r10") = d;
    register long r8 __asm__("r8") = e;
    long r = n;
    __asm__ volatile("syscall" : "+a"(r), "+D"(a), "+S"(b), "+d"(c), "+r"(r10), "+r"(r8) :: "rcx", "r11", "memory");
    return r;
}
#else
long caja_sc(long n, long a, long b, long c, long d, long e)
{
    register long x8 __asm__("x8") = n;
    register long x0 __asm__("x0") = a;
    register long x1 __asm__("x1") = b;
    register long x2 __asm__("x2") = c;
    register long x3 __asm__("x3") = d;
    register long x4 __asm__("x4") = e;
    __asm__ volatile("svc #0" : "+r"(x0), "+r"(x1) : "r"(x8), "r"(x2), "r"(x3), "r"(x4) : "memory");
    return x0;
}
#endif
#define sc caja_sc

#define LLAVE_PAGINA0 1   /* la llave 1 de la caja: el permiso de página 0 que le da `inicio` */

#if defined(__x86_64__)
/* La frecuencia del TSC: la da el kernel en rdi al arrancar (entrada_x86.S la guarda aquí). */
uint64_t caja_frecuencia_tsc;
/* Y la de la biblioteca de Rust (dashos::FRECUENCIA), que no arranca por su `programa!` (pegamento/). */
void caja_rust_frecuencia(uint64_t f);

void plat_iniciar(void)
{
    caja_rust_frecuencia(caja_frecuencia_tsc);
}

uint64_t plat_ahora_us(void)
{
    uint32_t bajo, alto;
    __asm__ volatile("lfence; rdtsc" : "=a"(bajo), "=d"(alto));
    uint64_t t = (uint64_t)alto << 32 | bajo, f = caja_frecuencia_tsc;
    return f ? t / f * 1000000 + t % f * 1000000 / f : 0;
}
#else
void plat_iniciar(void)
{
}

uint64_t plat_ahora_us(void)
{
    uint64_t t, f;
    __asm__ volatile("isb; mrs %0, cntvct_el0; mrs %1, cntfrq_el0" : "=r"(t), "=r"(f));
    return f ? t / f * 1000000 + t % f * 1000000 / f : 0;   /* sin división de 128 bits (no hay libgcc) */
}
#endif

long plat_escribir(const char *s, size_t n)
{
    return sc(SC_ESCRIBIR, (long)s, (long)n, 0, 0, 0);
}

void plat_dormir(uint64_t us)
{
    sc(SC_DORMIR, (long)us, 0, 0, 0, 0);
}

/* Un objeto de memoria nuevo, proyectado en `va`. Devuelve su llave, o un error negativo. */
long plat_memoria(uintptr_t va, size_t largo, int permisos)
{
    long m = sc(SC_MEMORIA_CREAR, (long)largo, 0, 0, 0, 0);
    if (m <= 0) return m ? m : -1;
    long r = sc(SC_MEMORIA_PROYECTAR, m, (long)va, permisos, 0, 0);
    return r ? r : m;
}

void plat_memoria_quitar(uintptr_t va, long llave)
{
    sc(SC_MEMORIA_QUITAR, (long)va, 0, 0, 0, 0);
    sc(SC_CERRAR, llave, 0, 0, 0, 0);
}

/* `esperar` (llamada 42) sobre una palabra que no vale lo que se le dice vuelve en el acto si la dirección es memoria
   del proceso, y da E_ARGUMENTO si no lo es. */
int plat_hay_memoria(uintptr_t p)
{
    return sc(SC_ESPERAR, (long)p, 0x5A5A5A5A, 1, 0, 0) != E_ARGUMENTO;
}

long plat_hilo_crear(void (*f)(uint64_t), uintptr_t pila_tope, uint64_t arg)
{
    return sc(SC_HILO_CREAR, (long)f, (long)pila_tope, (long)arg, 0, 0);
}

void plat_hilo_unir(long h)
{
    sc(SC_HILO_UNIR, h, 0, 0, 0, 0);
}

void plat_hilo_salir(void)
{
    sc(SC_SALIR, 0, 0, 0, 0, 0);
    for (;;) {}
}

/* La hora (llamada 39): 0 si el kernel no la sabe. */
uint64_t plat_hora_ns(void)
{
    return (uint64_t)sc(39, 0, 0, 0, 0, 0);
}

/* Acaba el proceso entero (llamada 44): el resto del sistema sigue. */
void plat_acabar(int codigo)
{
    sc(SC_PROCESO_SALIR, codigo, 0, 0, 0, 0);
    for (;;) {}
}

/* El código traducido (X7a): un objeto de memoria proyectado para escribir en `rw` y para ejecutar en `rx`. */
long plat_codigo(uintptr_t rw, uintptr_t rx, size_t largo)
{
    long m = sc(SC_MEMORIA_CREAR, (long)largo, 0, 0, 0, 0);
    if (m <= 0) return m ? m : -1;
    if (sc(SC_MEMORIA_PROYECTAR, m, (long)rw, P_ESCRIBIR, 0, 0) != 0 || sc(SC_MEMORIA_PROYECTAR, m, (long)rx, P_EJECUTAR, 0, 0) != 0)
        return -1;
    return m;
}

int plat_codigo_wx(long llave, uintptr_t va)
{
    return sc(SC_MEMORIA_PROYECTAR, llave, (long)va, P_ESCRIBIR | P_EJECUTAR, 0, 0) >= 0;
}

#if defined(__x86_64__)
/* En x86, RelevoJIT no usa el fallo de página: los chips y la página 0 van por sus páginas del anfitrión. */
long plat_fallos(int (*atiende)(struct marco_fallo *m))
{
    (void)atiende;
    return -1;
}
#else
/* K5 y K6 (docs/CHIPS_LOOP.md): el manejador de fallos del propio hilo (llamadas 61 y 62, docs/referencia/kernel.md,
   "Fallos del propio proceso"). El kernel entrega el fallo en caja_entrada_fallo, con su pila y el marco arriba; esta
   llama a plat_fallo_atiende, y esta a quien puso plat_fallos. */
#define SC_FALLO_MANEJADOR 61
#define SC_REANUDAR        62

/* La pila del manejador: en los datos de la caja, por encima de 4 GB (el 68k no llega). Arriba, el marco. */
#define PILA_FALLOS (64u << 10)
static uint8_t pila_fallos[PILA_FALLOS] __attribute__((aligned(16)));
static int (*atiende_fallo)(struct marco_fallo *m);

int plat_fallo_atiende(struct marco_fallo *m);
__attribute__((used, noipa)) int plat_fallo_atiende(struct marco_fallo *m)
{
    return atiende_fallo ? atiende_fallo(m) : 0;
}

/* La entrada del manejador (K5): x0 = SP = el marco. Los registros de la FPU y SIMD no van en el marco y el JIT tiene
   vivos los suyos (v8-v15, la FPU del 68k; v19-v31, el SR, las pilas, el contador...): se guardan todos, con FPCR y
   FPSR, antes de entrar en C (que puede usar v0-v7 y v16-v18), y se reponen antes de `reanudar`. v20, el contador de
   instrucciones, sigue como estaba al fallar: motor_ciclos() lo lee dentro. En un asm suelto: GCC no hace `naked` en
   AArch64. Si no es de los chips, quita el manejador y reanuda igual: la instrucción vuelve a fallar, ya sin
   manejador, y el kernel termina el proceso como siempre. Si `reanudar` volviera (un marco que no vale), el UDF es un
   fallo dentro del manejador: también lo termina. */
void caja_entrada_fallo(void);
__asm__(
    ".text\n"
    ".balign 16\n"
    ".type caja_entrada_fallo, %function\n"
    "caja_entrada_fallo:\n"
    "sub sp, sp, #528\n"
    "mrs x1, fpcr\n" "mrs x2, fpsr\n" "stp x1, x2, [sp, #0]\n"
    "stp q0, q1, [sp, #16]\n"    "stp q2, q3, [sp, #48]\n"    "stp q4, q5, [sp, #80]\n"
    "stp q6, q7, [sp, #112]\n"   "stp q8, q9, [sp, #144]\n"   "stp q10, q11, [sp, #176]\n"
    "stp q12, q13, [sp, #208]\n" "stp q14, q15, [sp, #240]\n" "stp q16, q17, [sp, #272]\n"
    "stp q18, q19, [sp, #304]\n" "stp q20, q21, [sp, #336]\n" "stp q22, q23, [sp, #368]\n"
    "stp q24, q25, [sp, #400]\n" "stp q26, q27, [sp, #432]\n" "stp q28, q29, [sp, #464]\n"
    "stp q30, q31, [sp, #496]\n"
    "mov x19, x0\n"
    "bl plat_fallo_atiende\n"
    "cbnz w0, 1f\n"
    "mov x0, #0\n" "mov x1, #0\n" "mov x2, #0\n" "mov x8, #61\n" "svc #0\n"   /* fallo_manejador(0) */
    "1:\n"
    "ldp x1, x2, [sp, #0]\n" "msr fpcr, x1\n" "msr fpsr, x2\n"
    "ldp q0, q1, [sp, #16]\n"    "ldp q2, q3, [sp, #48]\n"    "ldp q4, q5, [sp, #80]\n"
    "ldp q6, q7, [sp, #112]\n"   "ldp q8, q9, [sp, #144]\n"   "ldp q10, q11, [sp, #176]\n"
    "ldp q12, q13, [sp, #208]\n" "ldp q14, q15, [sp, #240]\n" "ldp q16, q17, [sp, #272]\n"
    "ldp q18, q19, [sp, #304]\n" "ldp q20, q21, [sp, #336]\n" "ldp q22, q23, [sp, #368]\n"
    "ldp q24, q25, [sp, #400]\n" "ldp q26, q27, [sp, #432]\n" "ldp q28, q29, [sp, #464]\n"
    "ldp q30, q31, [sp, #496]\n"
    "mov x0, x19\n"
    "mov x8, #62\n"                                               /* reanudar(marco) */
    "svc #0\n"
    "udf #0\n"
    ".size caja_entrada_fallo, . - caja_entrada_fallo\n");

long plat_fallos(int (*atiende)(struct marco_fallo *m))
{
    if (!atiende) {
        long r = sc(SC_FALLO_MANEJADOR, 0, 0, 0, 0, 0);
        atiende_fallo = 0;
        return r;
    }
    atiende_fallo = atiende;
    return sc(SC_FALLO_MANEJADOR, (long)(uintptr_t)caja_entrada_fallo, (long)(uintptr_t)pila_fallos, PILA_FALLOS, 0, 0);
}
#endif

/* La página 0 (S3): un objeto de una página, escrito por una dirección por encima de 4 GB y proyectado de solo lectura
   en la dirección 0, con el permiso de página 0 (la llave 1, de `inicio`). */
long plat_pagina0_crear(uintptr_t rw)
{
    long p0 = sc(SC_MEMORIA_CREAR, 4096, 0, 0, 0, 0);
    long r0 = p0 > 0 ? sc(SC_MEMORIA_PROYECTAR, p0, (long)rw, P_ESCRIBIR, 0, 0) : -1;
    return r0 == 0 ? p0 : (r0 ? r0 : -1);
}

long plat_pagina0_poner(long llave)
{
    return sc(SC_PAGINA_CERO, LLAVE_PAGINA0, llave, 0, 0, 0);
}

/* Otra vez, o en el resto de los primeros 64 KB, el kernel no deja. */
int plat_pagina0_cerrada(long llave)
{
    return sc(SC_PAGINA_CERO, LLAVE_PAGINA0, llave, 0, 0, 0) < 0 && sc(SC_MEMORIA_PROYECTAR, llave, 0x1000, P_LEER, 0, 0) < 0
        && sc(SC_MEMORIA_PROYECTAR, llave, 0, P_LEER, 0, 0) < 0;
}
