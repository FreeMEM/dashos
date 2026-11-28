/*
    DashOS, H4: lo que comparten las piezas de la caja Amiga: el mapa de la memoria del 68k, el acceso a ella, los
    registros del 68k en una llamada nativa y las funciones de las bibliotecas nativas (en Rust, pegamento/).
*/
#ifndef DASHOS_CAJA_H
#define DASHOS_CAJA_H

#include <stdint.h>
#include <stddef.h>
#include "motor.h"

/* La memoria del 68k: empieza con 4 MB y crece según la pidan sus programas (caja_ampliar_ram), hasta el tope de su
   ajuste (Prefs/cajas, `memoria amiga`; de serie, 256 MB). Las direcciones del 68k son direcciones reales del proceso. */
#define M68K_RAM     0x00100000UL
#define M68K_RAM_LEN 0x00400000UL   /* lo de al principio */
#define M68K_RAM_MAX 0xC0000000UL   /* hasta donde puede crecer, como mucho (deja sitio arriba) */
/* La página 0, para escribirla: por encima de 4 GB, el 68k no llega. La misma memoria la ve el 68k de solo lectura:
   en DashOS en la dirección 0; en Linux, que no deja proyectar nada en los primeros 64 KB, en PAGINA0_VISTA
   (docs/CAJA_LINUX_LOOP.md, L1; en la caja no hay nada entre $1000 y M68K_RAM). */
#define PAGINA0_RW    0x0000000380000000UL
#define PAGINA0_VISTA 0x00010000UL
extern volatile uint32_t m68k_ram_fin;   /* dónde acaba ahora */
uint32_t caja_ampliar_ram(uint32_t minimo, uint32_t tope);
#define VECTORES     0x00100000UL   /* VBR: 256 vectores */
#define TRAMPAS      0x00100400UL   /* una trampa de 14 bytes por vector */
#define COMUN        0x00101400UL   /* lo que acaba un programa: vuelve a la dirección 0 */
#define MARCA        0x00101500UL   /* el vector por el que acabó, o $FFFF */
#define PILA_TOPE    0x00102000UL   /* la pila de COMUN, solo para su RTS final */
#define SISTEMA      0x00102000UL   /* la zona del sistema de exec (ExecBase, sus tablas): [SISTEMA, FIN_SISTEMA) */
#define FIN_SISTEMA  0x00110000UL
#define PLANOS       0x00110000UL   /* los programas planos de prueba, cada uno enlazado en su dirección */
#define CHIP         0x00150000UL   /* desde aquí reparte exec (AllocMem): "chip" hasta los 2 MB, "fast" el resto */

/* El código 68k de la caja (pruebas68k/sistema.s, en sistema.bin): sus piezas, en direcciones fijas. */
#define SISTEMA68K      0x00101600UL
#define DESPACHO        (SISTEMA68K + 0x00)   /* las excepciones del procesador, a tc_TrapCode de la tarea */
#define TRAMPA_DEFECTO  (SISTEMA68K + 0x40)   /* el tc_TrapCode de las tareas: el programa acaba por esa excepción */
#define VUELTA_ANIDADA  (SISTEMA68K + 0x60)   /* adonde vuelve el 68k que la caja llama desde una función nativa */
#define INTERRUPCION68K (SISTEMA68K + 0x70)   /* el vector 30 (nivel 6): la caja lo levanta desde otro hilo */
#define SUPER_BAJO      (SISTEMA68K + 0x7c)   /* el límite de la pila de supervisor (lo mira el despacho) */
#define CAIDA68K        (SISTEMA68K + 0x80)   /* tras trampa_defecto: acaba el programa, o sigue otra tarea */
#define FIN_SISTEMA68K  (SISTEMA68K + 0xa0)
#define RTS_SUELTO      (VUELTA_ANIDADA + 4)  /* un RTS (el de la vuelta anidada) */
#define RTR_SUELTO      (SISTEMA68K + 0x9c)   /* un RTR: retoma una tarea que perdió el testigo por su turno */
#define CAMBIO          (MARCA + 2)           /* 1: caida cambió de tarea (RTE en vez de COMUN) */
/* Funciones nativas de la propia caja (no son de ninguna biblioteca). */
#define N_VUELTA        0xff00
#define N_INTERRUPCION  0xff01
#define N_CAIDA         0xff02

static inline void pon8(uintptr_t a, uint8_t v) { *(volatile uint8_t *)a = v; }
/* La memoria del 68k es big-endian. En ARM el proceso de la caja también lo es; en x86 (X7) no: se da la vuelta. */
#if __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
#define CAJA_BE16(v) (v)
#define CAJA_BE32(v) (v)
#else
#define CAJA_BE16(v) __builtin_bswap16(v)
#define CAJA_BE32(v) __builtin_bswap32(v)
#endif
static inline void pon16(uintptr_t a, uint16_t v) { *(volatile uint16_t *)a = CAJA_BE16(v); }
static inline void pon32(uintptr_t a, uint32_t v) { *(volatile uint32_t *)a = CAJA_BE32(v); }
static inline uint16_t lee16(uintptr_t a) { return CAJA_BE16(*(volatile uint16_t *)a); }
static inline uint32_t lee32(uintptr_t a) { return CAJA_BE32(*(volatile uint32_t *)a); }

/* Los bits del SR del 68k que mira la caja (en ARM también los da M68k.h de Emu68). */
#ifndef SR_S
#define SR_S 0x2000
#endif
#ifndef SR_M
#define SR_M 0x1000
#endif

/* ¿Está [a, a + n) dentro de la memoria del 68k? */
static inline int en_ram68k(uint32_t a, uint32_t n)
{
    uint32_t largo = m68k_ram_fin - M68K_RAM;
    return a >= M68K_RAM && n <= largo && a - M68K_RAM <= largo - n;
}

/* Lo que dice la caja: "  [caja-amiga] ..."; y un carácter suelto (la salida de los motores). */
void di(const char *formato, ...);
void caja_pon_c(void *data, char c);

/* Las llamadas al sistema de DashOS (docs/referencia/kernel.md), para la caja y sus motores. */
long caja_sc(long n, long a, long b, long c, long d, long e);
#define SC_ESCRIBIR          1
#define SC_SALIR             2
#define SC_CERRAR            6
#define SC_MEMORIA_CREAR     7
#define SC_MEMORIA_PROYECTAR 8
#define SC_MEMORIA_QUITAR    21
#define SC_DORMIR            30
#define SC_HILO_CREAR        40
#define SC_HILO_UNIR         41
#define SC_ESPERAR           42
#define SC_PROCESO_SALIR     44
#define SC_PAGINA_CERO       49
#define E_ARGUMENTO          (-1L)
#define P_LEER     1
#define P_ESCRIBIR 3
#define P_EJECUTAR 5

/* --- Las bibliotecas nativas: exec, dos, graphics e intuition, en Rust (pegamento/, crates de caja/). --- */
/* Monta exec en la memoria del 68k (con ExecBase en la dirección 4, escrita por la proyección `pagina0` de la página
   0), dos sobre el servicio de archivos y graphics e intuition sobre el compositor. Devuelve ExecBase, o 0. */
uint32_t caja_rust_arrancar(uint32_t ram, uint32_t largo, uint64_t pagina0, uint32_t sistema, uint32_t fin_sistema,
                            uint32_t chip);
uint32_t caja_dosbase(void);
/* Una llamada nativa: 0 bien, 1 nadie tiene ese número (D0 = -1), 2 hay que cortar el programa. */
int caja_llamada(motor_regs *regs, uint32_t n);
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
/* Pone el tc_TrapCode por defecto (ExecBase->TaskTrapCode y el de las tareas nuevas). */
void caja_poner_trampa(uint32_t defecto);
/* Las llamadas anidadas de la última tarea (hechas y no hechas) y sus excepciones de tarea entregadas. */
/* Los procesos nuevos, sin consola (pr_CIS y pr_COS a 0, como desde el Workbench) o con ella. */
void caja_sin_consola(int si);
void caja_tope_de_prueba(uint32_t mb);
/* aviso-cajas: el aviso de memoria baja (pegamento/src/aviso.rs). */
int caja_presion_listo(void);
void caja_atender_memoria(void);
int caja_programa_de_disco(uint32_t i, char *nombre, uint32_t max, const uint8_t **datos, uint32_t *largo);
void caja_cuentas_anidadas(uint32_t *hechas, uint32_t *no, uint32_t *excepciones);

/* Varias tareas (pegamento/src/multitarea.rs): la tarea en marcha; aparcar la que llega a la vuelta de un nivel que no
   es suyo (guarda sus registros, corre otra y deja los de la otra en regs: 0, o 1 si hay que cortar el programa); y
   dejar lista la dueña de un nivel cuando se cierran los de encima. */
uint32_t caja_tarea_en_marcha(void);
int caja_aparcar_en_vuelta(motor_regs *regs, uint32_t vuelta);
void caja_desaparcar(uint32_t t);
/* Una tarea cae por una excepción sin tc_TrapCode (en modo de usuario, por `caida`, con su número en `vector`): si es
   la del programa, 0 (el programa acaba). Si es otra, la caja la quita, corre otra y deja sus registros en regs: 1. */
int caja_caida(motor_regs *regs, uint32_t vector);
/* La interrupción de la caja llegó con una tarea en modo de usuario (en `pc`, con `sr` y su pila en `usp`): si acabó
   su turno y hay otra, la caja le quita el testigo (guarda su CCR y su PC en su pila, para el RTR_SUELTO) y deja en
   regs los registros de la siguiente: 1. Si no, 0. */
int caja_turno(motor_regs *regs, uint32_t usp, uint32_t pc, uint32_t sr, uint32_t rtr);
/* Si hay varias tareas: el reloj de la caja levanta su interrupción cada cuadro para mirar el turno. */
void caja_turnos(int si);
/* Desde el último programa: cambios de tarea, tareas acabadas y aparcadas en la vuelta de un nivel (y las pone a 0). */
void caja_cuentas_tareas(uint32_t *cambios, uint32_t *acabadas, uint32_t *aparcadas);

/* --- Lo que la caja (caja.c) da a Rust --- */
/* Ejecuta código 68k desde una función nativa (una llamada anidada): desde `pc`, con los registros r[0..7] = D0-D7
   y r[8..15] = A0-A7, hasta que vuelve con su RTS. Con `usuario`, en el modo de usuario y en su pila (como exec con
   tc_ExceptCode); si no, en el modo y la pila de quien llama. Deja en r los registros al volver. */
int caja_ejecutar_68k(uint32_t pc, uint32_t *r, int usuario);
#define ANIDADA_BIEN     0
#define ANIDADA_ACABADO  1   /* el programa acabó dentro (o se cortó): hay que deshacer todo */
#define ANIDADA_TOPE     2   /* demasiados niveles: no se ejecuta */
#define ANIDADA_PILA     3   /* la pila 68k de quien llama no vale: no se ejecuta */
/* El JIT olvida lo traducido, desde una función nativa (CacheClearU, SetFunction): menos lo que se está ejecutando
   (motor_olvidar_en_nativa). */
void caja_jit_olvidar(void);
/* 1 si alguien ha pedido Ctrl-C para el programa (y lo apaga). */
int caja_tomar_ctrl_c(void);
/* Cuántas veces volvió una llamada anidada con la pila más abajo de lo debido (lo apaga). */
int caja_tomar_pilas_raras(void);

#endif
