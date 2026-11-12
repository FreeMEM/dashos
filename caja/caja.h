/*
    DashOS, H4: lo que comparten las piezas de la caja Amiga: el mapa de la memoria del 68k, el acceso a ella, los
    registros del 68k en una llamada nativa y las funciones de las bibliotecas nativas (en Rust, pegamento/).
*/
#ifndef DASHOS_CAJA_H
#define DASHOS_CAJA_H

#include <stdint.h>
#include <stddef.h>

/* La memoria del 68k (4 MB). Las direcciones del 68k son direcciones reales del proceso. */
#define M68K_RAM     0x00100000UL
#define M68K_RAM_LEN 0x00400000UL
#define VECTORES     0x00100000UL   /* VBR: 256 vectores */
#define TRAMPAS      0x00100400UL   /* una trampa de 14 bytes por vector */
#define COMUN        0x00101400UL   /* lo que acaba un programa: vuelve a la dirección 0 */
#define MARCA        0x00101500UL   /* el vector por el que acabó, o $FFFF */
#define PILA_TOPE    0x00102000UL   /* la pila de COMUN, solo para su RTS final */
#define SISTEMA      0x00102000UL   /* la zona del sistema de exec (ExecBase, sus tablas): [SISTEMA, FIN_SISTEMA) */
#define FIN_SISTEMA  0x00110000UL
#define PLANOS       0x00110000UL   /* los programas planos de prueba, cada uno enlazado en su dirección */
#define CHIP         0x00150000UL   /* desde aquí reparte exec (AllocMem): "chip" hasta los 2 MB, "fast" el resto */

static inline void pon8(uintptr_t a, uint8_t v) { *(volatile uint8_t *)a = v; }
static inline void pon16(uintptr_t a, uint16_t v) { *(volatile uint16_t *)a = v; }   /* el proceso es big-endian */
static inline void pon32(uintptr_t a, uint32_t v) { *(volatile uint32_t *)a = v; }
static inline uint16_t lee16(uintptr_t a) { return *(volatile uint16_t *)a; }
static inline uint32_t lee32(uintptr_t a) { return *(volatile uint32_t *)a; }

/* ¿Está [a, a + n) dentro de la memoria del 68k? */
static inline int en_ram68k(uint32_t a, uint32_t n)
{
    return a >= M68K_RAM && n <= M68K_RAM_LEN && a - M68K_RAM <= M68K_RAM_LEN - n;
}

/* Registros del 68k en la copia de x0..x30 que deja el JIT (EMIT_lineA, parche 0002). */
#define R_D(n) (19 + (n))
#define R_A(n) ((n) < 5 ? 13 + (n) : 22 + (n))
static inline uint32_t lee_r(uint64_t *regs, int r) { return (uint32_t)regs[r]; }
static inline void pon_r(uint64_t *regs, int r, uint32_t v) { regs[r] = v; }

/* Lo que dice la caja: "  [caja-amiga] ..." */
void di(const char *formato, ...);

/* --- Las bibliotecas nativas: exec, dos, graphics e intuition, en Rust (pegamento/, crates de caja/). --- */
/* Monta exec en la memoria del 68k (con ExecBase en la dirección 4, escrita por la proyección `pagina0` de la página
   0), dos sobre el servicio de archivos y graphics e intuition sobre el compositor. Devuelve ExecBase, o 0. */
uint32_t caja_rust_arrancar(uint32_t ram, uint32_t largo, uint64_t pagina0, uint32_t sistema, uint32_t fin_sistema,
                            uint32_t chip);
uint32_t caja_dosbase(void);
/* Una llamada nativa: 0 bien, 1 nadie tiene ese número (D0 = -1), 2 hay que cortar el programa. */
int caja_llamada(uint64_t *regs, uint32_t n);
uint32_t caja_alloc_mem(uint32_t bytes, uint32_t atributos);
void caja_free_mem(uint32_t dir, uint32_t bytes);
uint32_t caja_bloques(uint32_t dueno);
/* Un proceso de AmigaOS que empieza en `pc` y al volver salta a `vuelta`, con una CLI (`seglist`, su cli_Module); su
   pila para A7, en *sp. 0 si no. */
uint32_t caja_tarea_nueva(const char *nombre, size_t largo, uint32_t pila, uint32_t pc, uint32_t vuelta, uint32_t args,
                          uint32_t seglist, uint32_t *sp);
/* Lo que escriba el programa en la consola, con `prefijo` al principio de cada línea (largo 0: sin prefijo). */
void caja_consola_prefijo(const char *prefijo, size_t largo);
/* La tarea acabó: se suelta todo lo suyo. Devuelve los bloques que el programa no soltó. */
uint32_t caja_tarea_fin(uint32_t t);
uint32_t caja_avisos(void);
int caja_compositor_sigue(void);

#endif
