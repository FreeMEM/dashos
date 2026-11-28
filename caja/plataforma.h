/*
    DashOS, la caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L2): lo que la caja pide al sistema.

    Dos versiones: plataforma_dashos.c (las llamadas al kernel de DashOS, en AArch64 y en x86-64) y, para la caja como
    programa de Linux, plataforma_linux.c (L3). caja.c solo habla con el sistema por aquí.

    Los permisos de memoria son los de caja.h (P_LEER, P_ESCRIBIR).
*/
#ifndef DASHOS_PLATAFORMA_H
#define DASHOS_PLATAFORMA_H

#include <stdint.h>
#include <stddef.h>

/* Lo primero de caja_main, antes de que hable nadie (en DashOS x86-64, la frecuencia del TSC para Rust). */
void plat_iniciar(void);

/* Escribe en la consola; lo escrito (puede ser menos de `n`), o un error negativo. */
long plat_escribir(const char *s, size_t n);

/* El reloj monótono, en microsegundos. */
uint64_t plat_ahora_us(void);

/* Duerme el hilo que llama. */
void plat_dormir(uint64_t us);

/* Memoria nueva, a cero, proyectada en `va` con `permisos`: su llave (> 0), o un error (<= 0). */
long plat_memoria(uintptr_t va, size_t largo, int permisos);

/* Quita la memoria de plat_memoria que empieza en `va` y suelta su llave. */
void plat_memoria_quitar(uintptr_t va, long llave);

/* 1 si hay algo proyectado en la dirección `p` (para comprobar que en los 4 GB del 68k no hay nada de la caja). */
int plat_hay_memoria(uintptr_t p);

/* Un hilo que corre `f(arg)` con la pila que acaba en `pila_tope`: su número (>= 0), o un error negativo. El hilo
   acaba con plat_hilo_salir. */
long plat_hilo_crear(void (*f)(uint64_t), uintptr_t pila_tope, uint64_t arg);

/* Espera a que acabe el hilo `h`. */
void plat_hilo_unir(long h);

/* Acaba el hilo que llama. */
__attribute__((noreturn)) void plat_hilo_salir(void);

/* La página 0 del 68k (4 KB; ExecBase en la dirección 4): se crea proyectada para escribir en `rw` (por encima de los
   4 GB del 68k) y devuelve su llave (> 0), o un error. */
long plat_pagina0_crear(uintptr_t rw);

/* La deja ver al 68k, de solo lectura (en DashOS, en la dirección 0, con el permiso de página 0): 0 si fue bien. */
long plat_pagina0_poner(long llave);

/* 1 si el sistema no deja hacerlo otra vez ni proyectarla en otro sitio de los primeros 64 KB. */
int plat_pagina0_cerrada(long llave);

#endif
