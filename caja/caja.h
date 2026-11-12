/*
    DashOS, H4: lo que comparten las piezas de la caja Amiga (caja.c, exec_minima.c): el mapa de la memoria del 68k,
    el acceso a ella y los registros del 68k en una llamada nativa.
*/
#ifndef DASHOS_CAJA_H
#define DASHOS_CAJA_H

#include <stdint.h>
#include <stddef.h>

/* La memoria del 68k (1 MB). Las direcciones del 68k son direcciones reales del proceso. */
#define M68K_RAM     0x00100000UL
#define M68K_RAM_LEN 0x00100000UL
#define VECTORES     0x00100000UL   /* VBR: 256 vectores */
#define TRAMPAS      0x00100400UL   /* una trampa de 14 bytes por vector */
#define COMUN        0x00101400UL   /* lo que acaba un programa: vuelve a la dirección 0 */
#define MARCA        0x00101500UL   /* el vector por el que acabó, o $FFFF */
#define DOS_BASE     0x00104000UL   /* la dos.library nativa; su tabla de saltos, por debajo */
#define EXEC_BASE    0x00106000UL   /* ExecBase (exec mínima y provisional); su tabla de saltos, por debajo */
#define HUNKS        0x00150000UL   /* donde carga los ejecutables hunk (reparto provisional, sin AllocMem) */
#define HUNKS_FIN    0x001E0000UL
#define PILA_TOPE    0x001F0000UL

static inline void pon8(uintptr_t a, uint8_t v) { *(volatile uint8_t *)a = v; }
static inline void pon16(uintptr_t a, uint16_t v) { *(volatile uint16_t *)a = v; }   /* el proceso es big-endian */
static inline void pon32(uintptr_t a, uint32_t v) { *(volatile uint32_t *)a = v; }
static inline uint16_t lee16(uintptr_t a) { return *(volatile uint16_t *)a; }
static inline uint32_t lee32(uintptr_t a) { return *(volatile uint32_t *)a; }

/* ¿Está [a, a + n) dentro de la memoria del 68k? Lo que caiga fuera es memoria de la caja, y una función nativa no la
   toca por el 68k. */
static inline int en_ram68k(uint32_t a, uint32_t n)
{
    return a >= M68K_RAM && n <= M68K_RAM_LEN && a - M68K_RAM <= M68K_RAM_LEN - n;
}

/* Lo que mide una cadena del 68k terminada en 0, o -1 si se sale de su memoria (o pasa de 4 KB). */
long cadena68k(uint32_t a);

/* Registros del 68k en la copia de x0..x30 que deja el JIT (EMIT_lineA, parche 0002). */
#define R_D(n) (19 + (n))
#define R_A(n) ((n) < 5 ? 13 + (n) : 22 + (n))
static inline uint32_t lee_r(uint64_t *regs, int r) { return (uint32_t)regs[r]; }
static inline void pon_r(uint64_t *regs, int r, uint32_t v) { regs[r] = v; }

/* Una entrada de una tabla de saltos: "$A0FF <número> RTS" en base - LVO. */
static inline void pon_salto(uintptr_t base, uint16_t lvo, uint16_t numero)
{
    uintptr_t e = base - lvo;
    pon16(e, 0xa0ff); pon16(e + 2, numero); pon16(e + 4, 0x4e75);
}

/* La cabecera de una biblioteca (struct Library, exec/libraries.h) en `base`: tipo NT_LIBRARY, nombre, tamaños y
   versión. El nombre se copia a `nombre_en`, en la memoria del 68k. */
void pon_biblioteca(uintptr_t base, const char *nombre, uintptr_t nombre_en, uint16_t neg, uint16_t pos, uint16_t version);

/* Lo que dice la caja: "  [caja-amiga] ..." */
void di(const char *formato, ...);

/* --- exec mínima y provisional (exec_minima.c). La exec de verdad (rama caja-amiga-exec, crate caja/exec) la
   sustituye: monta ExecBase en la memoria del 68k y atiende los números de función desde N_EXEC. --- */
#define N_EXEC 0x100
/* Monta ExecBase en EXEC_BASE con su tabla de saltos. Devuelve la dirección de ExecBase (lo que va en la dirección 4). */
uint32_t exec_minima_preparar(void);
/* Una llamada nativa de exec (n >= N_EXEC). 0 si no es de exec. */
int exec_minima_llamada(uint64_t *regs, uint32_t n);

#endif
