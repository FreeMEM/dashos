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
