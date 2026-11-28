/*
    DashOS, X7 (paso 2; S31, sección 3): la interfaz de motor de la caja Amiga. La caja (caja.c, y las bibliotecas de
    Rust de pegamento/) no sabe qué ejecuta el 68k: lo que necesita de él pasa por aquí. Hoy, Emu68 en ARM
    (motor_emu68.c); en x86, RelevoJIT.

    Una sola CPU 68k para toda la caja: las tareas de exec cambian de registros dentro de una llamada nativa, no son
    hilos. Las direcciones del 68k son las del proceso (base 0).
*/
#ifndef DASHOS_MOTOR_H
#define DASHOS_MOTOR_H

#include <stdint.h>

/* Los registros que ve una función nativa: D0-D7 y A0-A7. Es el `Registros` de Rust (caja/exec/src/registros.rs,
   #[repr(C)]): las funciones de pegamento/ los reciben así, sin saber del motor. */
typedef struct motor_regs {
    uint32_t d[8];
    uint32_t a[8];
} motor_regs;

/* --- Lo que la caja da al motor --- */

/* Una llamada nativa: el 68k ejecutó "$A0FF <índice>" (la tabla de saltos de una biblioteca). Con los registros en
   `r`; lo que cambie en ellos, el 68k lo ve al volver. */
void caja_nativa(motor_regs *r, uint32_t indice);
/* K2 (docs/CHIPS_LOOP.md): con los chips encendidos (motor_chips), un acceso del 68k a $BF0000-$BFFFFF (los CIA) o a
   $DF0000-$DFFFFF (los registros de los chips): `tam` bytes (1, 2 o 4) en `dir`, a la hora `hora` (motor_ciclos, la
   del acceso). Los valores, en el orden del 68k (el byte de la dirección menor, el más alto). Los da pegamento/
   (chips.rs, sobre caja/chips). Lo que no se imita lee 0 y no escribe. */
uint32_t caja_chip_leer(uint32_t dir, unsigned tam, uint64_t hora);
void caja_chip_escribir(uint32_t dir, uint32_t valor, unsigned tam, uint64_t hora);
/* K8: pone los chips al día hasta `hora` (la del motor) y dice cuándo tiene que volver el motor (la hora del siguiente
   suceso de los chips: fin de cuadro, un temporizador de un CIA, el fin del blit...; siempre posterior a `hora`) y el
   nivel de interrupción que piden (0 a 6: los bits de INTREQ habilitados en INTENA, con INTEN, con el mapa de niveles
   del Amiga). Con los chips apagados, { UINT64_MAX, 0 }. La da pegamento/ (chips.rs). */
struct caja_plazo { uint64_t siguiente; uint32_t ipl; };
struct caja_plazo caja_chips_plazo(uint64_t hora);

/* --- Lo que el motor da a la caja --- */

/* Lo que dice al arrancar ("JIT de Emu68 en un proceso de usuario big-endian"...). */
const char *motor_nombre(void);
/* Prepara el motor (su memoria de código, su estado). 0 bien; si no, un código distinto de 0 (ya dicho con di). */
int motor_iniciar(void);
/* Ejecuta el 68k desde `pc`, en modo de usuario y sin máscara de interrupciones, con D0 = `d0`, A0 = `a0`,
   A6 = `a6`, A7 = USP = `a7`, ISP = MSP = `super` y VBR = `vbr` (el resto a 0, la FPU a NaN), hasta que vuelve a la
   dirección 0. */
void motor_arrancar(uint32_t pc, uint32_t a7, uint32_t a6, uint32_t a0, uint32_t d0, uint32_t super, uint32_t vbr);
/* Desde una función nativa: ejecuta el 68k desde `pc` con los registros de `r` y con este SR y estas pilas (A7 en
   r->a[7] es la de ese SR), hasta que vuelve a la dirección 0. Deja en `r` los D y A del final, y el resto del estado
   (PC, SR, pilas, VBR) como estaba antes: la función nativa sigue donde estaba. */
void motor_anidado(uint32_t pc, motor_regs *r, uint32_t sr, uint32_t usp, uint32_t isp, uint32_t msp);
/* El estado del 68k que corre, desde una función nativa. */
uint32_t motor_sr(void);
void motor_pilas(uint32_t *usp, uint32_t *isp, uint32_t *msp);
void motor_poner_usp(uint32_t usp);
/* D0 al acabar motor_arrancar. */
uint32_t motor_d0(void);
/* Desde cualquier hilo: la interrupción de nivel 7 (la NMI: corta el programa) o la de nivel 6 (la de la caja). */
void motor_cortar(void);
void motor_interrumpir(void);
/* Desde la función nativa de la interrupción de nivel 6: atendida. */
void motor_interrupcion_atendida(void);
/* Olvida todo lo traducido: entre programas, con el 68k parado. */
void motor_olvidar(void);
/* Lo mismo desde una función nativa (CacheClearU, SetFunction): menos lo que se está ejecutando. */
void motor_olvidar_en_nativa(void);
/* La memoria del 68k cambió: [desde, hasta) pasa a ser RAM (`ram`) o deja de serlo (antes de quitarla). La caja la
   amplía y la encoge (caja_ampliar_ram, caja_encoger_ram); un motor que no mira la MMU del proceso (Emu68) no hace
   nada. */
void motor_memoria(uint32_t desde, uint32_t hasta, int ram);
/* Cuántos bloques traducidos tiene (para las pruebas). */
uint32_t motor_unidades(void);
/* S28: dónde tiene el motor lo suyo (su estado y su código traducido, para escribir y para ejecutar): la caja
   comprueba que nada de eso está al alcance del 68k (por encima de 4 GB). */
#define MOTOR_ZONAS 3
void motor_zonas(uintptr_t z[MOTOR_ZONAS]);
/* K2: la hora del 68k, en ciclos de 68000 (7.093.790 por segundo en PAL) desde que arrancó el motor: siempre crece.
   Exacta con un intérprete que cuenta los ciclos; aproximada con un JIT que los estima (o que cuenta instrucciones). */
uint64_t motor_ciclos(void);
/* K2: los chips del Amiga, encendidos (`encendidos` distinto de 0) o apagados, con el 68k parado (entre programas).
   Encendidos, el motor manda a caja_chip_leer y caja_chip_escribir los accesos a $BF0000-$BFFFFF y $DF0000-$DFFFFF;
   apagados (de serie), ahí no hay nada, como hasta ahora: un acceso es un fallo (el Enforcer, o el proceso muere).
   Devuelve 1 si quedan como se pide; 0 si el motor no sabe capturar esos accesos o si la RAM del 68k llega a $BF0000
   (no se pueden quitar de debajo de exec). Cómo los captura cada motor: RelevoJIT, con sus páginas del anfitrión;
   Emu68 (K6), por el fallo de página que el kernel entrega al hilo (fallo_a64.h y motor_emu68.c), y después (K7) con
   la instrucción reescrita a un trampolín (trampolin_a64.h); al apagarlos no queda nada reescrito. Con ellos
   encendidos, la caja no crece la RAM hasta ahí (caja_ampliar_ram).
   Quién los enciende: la caja (chips, en caja.c), entre programas, para el que los necesita: primero los de
   pegamento/ (caja_chips_poner, con su hora 0 en motor_ciclos()) y después este. Hoy solo para pruebas68k/chips.c, que
   no hace nada para encenderlos: el mismo binario corre en un Amiga de verdad (K4). Más adelante, una marca del
   programa. */
int motor_chips(int encendidos);
/* K8: el contador hacia atrás. Con los chips encendidos, el motor vuelve a ellos (caja_chips_plazo) cuando su hora
   llega al plazo que le dieron, entre bloques traducidos (como mira las interrupciones), y tras cada escritura del 68k
   en los chips (INTENA, INTREQ, un temporizador... cambian el nivel o el plazo); cada vez guarda el nivel que piden y
   arma el plazo nuevo. Con los chips apagados no se arma: nada cambia ni cuesta.
   - Emu68 (ARM): el plazo, en instrucciones (motor_ciclos / 8), en dashos_plazo; MainLoop lo mira al volver de cada
     bloque si INTF.CHIPS está a 1 (parche 0017 de Emu68). Con INTF.CHIPS a 1, INT64 no es 0, así que los
     bucles internos de una unidad también vuelven a MainLoop en cada vuelta: con los chips encendidos no se los salta.
   - RelevoJIT (x86): el presupuesto de instrucciones de relevo_run hasta el plazo (con una estimación por lo bajo
     de los ciclos de cada instrucción); si una escritura lo adelanta, relevo_request_stop.
   El nivel NO se entrega al 68k: en la caja nativa los niveles 6 y 7 son de la caja (motor_interrumpir, la de la caja,
   y motor_cortar, la NMI) y exec es nativo, sin vectores de nivel 1 a 6 que atender. motor_ipl_chips() lo da para las
   pruebas (caja.c, ipl.bin) y para la fase 2 de S18 (la máquina entera, con el Kickstart o la ROM de AROS en el 68k):
   allí el motor lo entregará como el IPL del 68000, el más alto entre el de los chips y el suyo: en Emu68, en el nivel
   que calcula MainLoop (otro cambio en el parche 0017: sin PiStorm, MainLoop no usa INTF.IPL); en RelevoJIT, con relevo_set_ipl, sin
   pisar el 6 y el 7 de la caja mientras la caja los use. Y tendrá que mirar el plazo también con el 68k en STOP
   (hoy, en STOP, la hora no corre) y acortar los bloques (S18, "La exactitud de las interrupciones"). */
unsigned motor_ipl_chips(void);
/* K8: cuántas veces ha llegado el contador a su plazo desde que se encendieron los chips (para las pruebas). */
uint64_t motor_plazos_chips(void);
/* Escribe los registros del 68k en la consola (cuando una prueba sale mal). */
void motor_volcar(void);
/* La coma flotante del 68k para pegamento/ (multitarea), en 13 u64: FPSR, FPIAR y FPCR en v[8..10]. FP0-FP7 en
   v[0..7]: con Emu68, el double (y v[11..12] sin tocar); con un motor de 80 bits, la mantisa, y el signo y el
   exponente en v[11] (FP0-FP3) y v[12] (FP4-FP7), 16 bits cada uno, FP0 en los de abajo. */
void caja_fp_leer(uint64_t *v);
void caja_fp_poner(const uint64_t *v);

#endif
