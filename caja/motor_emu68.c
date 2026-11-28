/*
    DashOS, X7 (paso 2): el motor Emu68 de la caja Amiga (motor.h), en ARM. Lo que estaba cosido a Emu68 en caja.c, tal
    cual: su estado (M68KState), MainLoop y su anidado, las unidades traducidas que se fijan mientras una función nativa
    ejecuta 68k, el SR y las pilas en registros vectoriales, las interrupciones (INTF) y la coma flotante.

    La llamada nativa: el JIT convierte "$A0FF <índice>" en una llamada a dashos_native_call (parche 0002 de
    patches/emu68) con `regs` apuntando a una copia de x0..x30, donde viven D0-D7 (x19-x26) y A0-A7 (x13-x17, x27-x29);
    aquí pasan a motor_regs para caja_nativa y vuelven a la copia, que el JIT recarga al volver.
*/

#include <stdarg.h>
#include "M68k.h"
#include "support.h"
#include "tlsf.h"
#include "cache.h"
#include "config.h"
#include "caja.h"
#include "motor.h"

/* --- Lo que Emu68 define en start.c y disasm.c, y su salida por la consola de la caja --- */

struct M68KState *__m68k_state;
int debug_not_implemented = 0;
int dcache_mask_bits;

static int emu68_habla;
static int emu68_linea_nueva = 1;

static void pon_emu68(void *data, char c)
{
    if (!emu68_habla) return;
    if (emu68_linea_nueva) {
        const char *m = "  [caja-amiga] emu68: ";
        while (*m) caja_pon_c(data, *m++);
        emu68_linea_nueva = 0;
    }
    caja_pon_c(data, c);
    if (c == '\n') emu68_linea_nueva = 1;
}

void vkprintf(const char *format, va_list args) { vkprintf_pc(pon_emu68, NULL, format, args); }

void kprintf(const char *format, ...)
{
    va_list v;
    va_start(v, format);
    vkprintf_pc(pon_emu68, NULL, format, v);
    va_end(v);
}

void disasm_init(void) {}
void disasm_open(void) {}
void disasm_close(void) {}
void disasm_print(uint16_t *m68k_addr, uint16_t m68k_count, uint32_t *arm_addr, size_t arm_size, uint32_t *arm_start)
{
    (void)m68k_addr; (void)m68k_count; (void)arm_addr; (void)arm_size; (void)arm_start;
}

void M68K_PrintContext(struct M68KState *m68k)
{
    kprintf("D0-D7:");
    for (int i = 0; i < 8; i++) kprintf(" %08x", m68k->D[i].u32);
    kprintf("\nA0-A7:");
    for (int i = 0; i < 8; i++) kprintf(" %08x", m68k->A[i].u32);
    kprintf("\nPC=%08x SR=%04x\n", m68k->PC, m68k->SR);
}

/* La llama M68k_Translator.c (parche 0003) con cada bloque nuevo; en la caja no se guarda. */
void dashos_jit_unit(uint32_t m68k_addr, const uint32_t *arm, uint32_t bytes)
{
    (void)m68k_addr; (void)arm; (void)bytes;
}

void MainLoop(void);

static struct M68KState m68k;

const char *motor_nombre(void)
{
#if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
    return "JIT de Emu68 en un proceso de usuario little-endian (U7: gira los bytes del 68k)";
#else
    return "JIT de Emu68 en un proceso de usuario big-endian";
#endif
}

/* --- El código traducido: el mismo objeto para escribir y, en otra dirección, para ejecutar (W^X) --- */

#define JIT_RW   0x0000000100000000UL
#define JIT_RX   (JIT_RW | 0x0000001000000000UL)
#define JIT_LEN  (4UL << 20)
/* K7: los trampolines de la instrucción reescrita, en los últimos 64 KB del código traducido (fuera del tlsf del JIT):
   así quedan a menos de 128 MB de cualquier sitio, el alcance de un `b`. */
#include "trampolin_a64.h"
#define TRAMP_RW (JIT_RW + JIT_LEN - TRAMPOLINES_ZONA)
#define TRAMP_RX (JIT_RX + JIT_LEN - TRAMPOLINES_ZONA)

int motor_iniciar(void)
{
    long jit = caja_sc(SC_MEMORIA_CREAR, JIT_LEN, 0, 0, 0, 0);
    if (jit <= 0 || caja_sc(SC_MEMORIA_PROYECTAR, jit, JIT_RW, P_ESCRIBIR, 0, 0) != 0
        || caja_sc(SC_MEMORIA_PROYECTAR, jit, JIT_RX, P_EJECUTAR, 0, 0) != 0) {
        di("sin memoria para el código traducido\n");
        return 2;
    }
    /* Que la misma dirección no se pueda escribir y ejecutar: el kernel lo rechaza. */
    if (caja_sc(SC_MEMORIA_PROYECTAR, jit, JIT_RX + JIT_LEN, P_ESCRIBIR | P_EJECUTAR, 0, 0) >= 0) {
        di("MAL: el kernel deja escribir y ejecutar a la vez\n");
        return 3;
    }
    jit_tlsf = tlsf_init_with_memory((void *)JIT_RW, JIT_LEN - TRAMPOLINES_ZONA);
    trampolines_iniciar(TRAMP_RW, TRAMP_RX);

    uint64_t ctr;
    __asm__ volatile("mrs %0, CTR_EL0" : "=r"(ctr));
    dcache_mask_bits = 2 + ((ctr >> 16) & 15);

    /* El estado del 68k, como M68K_StartEmu sin PiStorm (lo mismo que jit-host) */
    __m68k_state = &m68k;
    cache_setup();
    M68K_InitializeCache();
    m68k.JIT_CACHE_TOTAL = tlsf_get_total_size(jit_tlsf);
    m68k.JIT_CACHE_FREE = tlsf_get_free_size(jit_tlsf);
    m68k.JIT_SOFTFLUSH_THRESH = EMU68_WEAK_CFLUSH_LIMIT;
    m68k.JIT_CONTROL = EMU68_WEAK_CFLUSH ? JCCF_SOFT : 0;
    m68k.JIT_CONTROL |= (EMU68_M68K_INSN_DEPTH & JCCB_INSN_DEPTH_MASK) << JCCB_INSN_DEPTH;
    m68k.JIT_CONTROL |= (EMU68_BRANCH_INLINE_DISTANCE & JCCB_INLINE_RANGE_MASK) << JCCB_INLINE_RANGE;
    m68k.JIT_CONTROL |= (EMU68_MAX_LOOP_COUNT & JCCB_LOOP_COUNT_MASK) << JCCB_LOOP_COUNT;
    m68k.JIT_CONTROL2 = (EMU68_CCR_SCAN_DEPTH << JC2B_CCR_SCAN_DEPTH);
    emu68_habla = 1;
    return 0;
}

uint32_t motor_unidades(void) { return m68k.JIT_UNIT_COUNT; }

/* K2: Emu68 no cuenta ciclos. Con su contador de instrucciones (EMU68_INSN_COUNTER, en CTX_INSN_COUNT: v20.d[0],
   fijo con -ffixed-v20), una estimación a tanto por instrucción, que es lo que se espera en K6 hasta que el traductor
   sume ciclos; sin él, 0. */
#define CICLOS_POR_INSTRUCCION 8
uint64_t motor_ciclos(void)
{
#if EMU68_INSN_COUNTER
    uint64_t n;
    __asm__ volatile("umov %0, " CTX_INSN_COUNT_ASM : "=r"(n));
    return n * CICLOS_POR_INSTRUCCION;
#else
    return 0;
#endif
}

/* --- K6 (docs/CHIPS_LOOP.md): los chips del Amiga, por el fallo de página --- */

/* En la caja de ARM no hay nada proyectado en $BF0000-$BFFFFF ni en $DF0000-$DFFFFF: la RAM del 68k empieza en
   $100000 y, con los chips encendidos, no crece hasta $BF0000 (caja_ampliar_ram); lo demás de la caja está por encima
   de 4 GB. Así un acceso del código traducido a los chips es un fallo de datos, que el kernel entrega al propio hilo
   (K5: llamadas 61 y 62, docs/referencia/kernel.md, "Fallos del propio proceso"). El manejador decodifica la carga o
   el guardado de A64, llama a caja_chip_leer o caja_chip_escribir, pone el registro y sigue en PC + 4.

   Lo que no es suyo (un fallo fuera del código traducido, fuera de las dos zonas, o una instrucción que no sabe
   decodificar) acaba como sin chips: quita el manejador y reanuda en la misma instrucción, que vuelve a fallar y el
   kernel termina el proceso con su aviso de siempre. Antes lo dice con di().

   K7: tras atender el fallo, el sitio se reescribe con un `b` a su trampolín (trampolin_a64.h), y los accesos
   siguientes de ese sitio ya no pasan por el kernel. */

#define SC_FALLO_MANEJADOR 61
#define SC_REANUDAR        62
#define CLASE_DATOS_EL0    0x24

/* La pila del manejador: en los datos de la caja, por encima de 4 GB (el 68k no llega). Arriba, el marco. */
#define PILA_FALLOS (64u << 10)
static uint8_t pila_fallos[PILA_FALLOS] __attribute__((aligned(16)));
static int chips_en;

uint64_t chips_fallos;   /* accesos atendidos (para las pruebas y para K7) */

/* Lo llama caja_entrada_fallo con el marco. 1: atendido; 0: no es de los chips (y lo dice, si caía en ellos). */
int chips_fallo(marco_fallo *m);
__attribute__((used, noipa)) int chips_fallo(marco_fallo *m)
{
    if (((m->esr >> 26) & 0x3f) != CLASE_DATOS_EL0 || !en_zona(m->far, 1)) return 0;
    if (m->pc < JIT_RX || m->pc >= JIT_RX + JIT_LEN || (m->pc & 3)) {
        di("chips: acceso a %08x fuera del código traducido (PC %p)\n", (uint32_t)m->far, (void *)m->pc);
        return 0;
    }
    /* Las instrucciones de A64 van siempre en little-endian, también en la caja big-endian. */
    const volatile uint8_t *p = (const volatile uint8_t *)m->pc;
    uint32_t ins = p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
    struct acceso a;
    uint64_t sitio = m->pc;
    uint64_t hora = motor_ciclos();
    int r = chips_atiende(m, ins, hora, &a);
    if (r == 0)
        di("chips: no sé decodificar la instrucción %08x (en %p, acceso a %08x)\n", ins, (void *)m->pc,
           (uint32_t)m->far);
    else if (r < 0)
        di("chips: la instrucción %08x (en %p) toca %u bytes en %p: no caben en los chips\n", ins, (void *)m->pc,
           a.tam * a.pares, (void *)a.dir);
    else {
        chips_fallos++;
        if (a.escribe) chips_rearmar(hora);   /* K8 */
        trampolin_poner(sitio, ins, &a);
    }
    return r > 0;
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
    "bl chips_fallo\n"
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

/* --- K8: el contador hacia atrás (motor.h, motor_ipl_chips) --- */

/* El plazo, en instrucciones del contador de Emu68 (la hora del motor / CICLOS_POR_INSTRUCCION, por arriba). Lo mira
   MainLoop al volver de cada bloque mientras INTF.CHIPS esté a 1 (parche 0017; las dos, débiles allí). */
uint64_t dashos_plazo = ~0ULL;
static volatile unsigned ipl_chips;
static uint64_t plazos_chips;

/* Pone los chips al día a la hora `hora` y arma el plazo siguiente. La llaman también los trampolines (tras una
   escritura): sin tocar los registros de la FPU y SIMD, que el código traducido tiene vivos (como trampolin_a64.c). */
__attribute__((target("general-regs-only"))) void chips_rearmar(uint64_t hora)
{
    struct caja_plazo p = caja_chips_plazo(hora);
    ipl_chips = p.ipl;
    dashos_plazo = p.siguiente == ~0ULL ? ~0ULL : p.siguiente / CICLOS_POR_INSTRUCCION
        + (p.siguiente % CICLOS_POR_INSTRUCCION != 0);
}

/* Desde MainLoop, con el contexto guardado: el contador llegó al plazo. */
void dashos_plazo_llegado(void)
{
    plazos_chips++;
    chips_rearmar(motor_ciclos());
}

unsigned motor_ipl_chips(void) { return ipl_chips; }
uint64_t motor_plazos_chips(void) { return plazos_chips; }

/* El plazo armado: INTF.CHIPS a 1 hace que MainLoop lo mire (INT64 deja de ser 0). */
static void plazo_marca(int armado) { __atomic_store_n(&m68k.INTF.CHIPS, (uint8_t)armado, __ATOMIC_RELEASE); }

/* K2 y K6: encendidos, el manejador de fallos del hilo de la caja (el que ejecuta el 68k) atiende las dos zonas;
   apagados, se quita. Sin chips, un acceso ahí acaba como siempre. K8: y el contador hacia atrás, armado o no. */
int motor_chips(int encendidos)
{
    encendidos = encendidos != 0;
    if (encendidos == chips_en) return 1;
    if (encendidos) {
        /* La RAM del 68k empieza en M68K_RAM y no tiene huecos: si llega a la primera zona, exec puede tener bloques
           ahí. */
        if (m68k_ram_fin > CHIPS_CIA) return 0;
        if (caja_sc(SC_FALLO_MANEJADOR, (long)(uintptr_t)caja_entrada_fallo, (long)(uintptr_t)pila_fallos, PILA_FALLOS, 0,
                    0) != 0) {
            di("chips: el kernel no da el manejador de fallos\n");
            return 0;
        }
        chips_fallos = chips_tramp_accesos = chips_tramp_ram = 0;
        chips_reescritos = chips_deshechos = 0;
        plazos_chips = 0;
        chips_rearmar(motor_ciclos());
        plazo_marca(1);
    } else {
        /* K7: que no quede nada reescrito (el sitio volvería a los chips apagados sin fallar). */
        trampolines_deshacer();
        caja_sc(SC_FALLO_MANEJADOR, 0, 0, 0, 0, 0);
        plazo_marca(0);
        dashos_plazo = ~0ULL;
        ipl_chips = 0;
    }
    chips_en = encendidos;
    return 1;
}
void motor_memoria(uint32_t desde, uint32_t hasta, int ram) { (void)desde; (void)hasta; (void)ram; }
void motor_zonas(uintptr_t z[MOTOR_ZONAS]) { z[0] = (uintptr_t)&m68k; z[1] = JIT_RW; z[2] = JIT_RX; }
uint32_t motor_d0(void) { return m68k.D[0].u32; }
void motor_volcar(void) { M68K_PrintContext(&m68k); }

void motor_cortar(void) { __atomic_store_n(&m68k.INTF.ARM_err, 1, __ATOMIC_RELEASE); }
void motor_interrumpir(void) { __atomic_store_n(&m68k.INTF.ARM, 1, __ATOMIC_RELEASE); }
void motor_interrupcion_atendida(void) { __atomic_store_n(&m68k.INTF.ARM, 0, __ATOMIC_RELEASE); }

/* --- Las unidades traducidas mientras una función nativa ejecuta 68k --- */

/* Cada función nativa corre dentro de una unidad traducida del JIT (la que tiene su $A0FF), y esa unidad sigue
   ejecutándose al volver. Si desde la función se ejecuta 68k otra vez (motor_anidado), hay un MainLoop dentro de
   otro: una pila de contextos. Cada nivel guarda lo que el MainLoop de dentro cambia (los registros fijos del 68k que
   no recarga el JIT al volver de la función: v8-v15, los de coma flotante, y v19-v31, el SR, las pilas, el puntero al
   contexto, la última PC...) y lo devuelve al acabar; los x0..x30 los recarga el propio JIT (parche 0002). Y saca su
   unidad de la lista LRU y de la tabla del JIT mientras tanto (fijar), para que nada la suelte: ni el JIT al quedarse
   sin sitio, ni al ver que su código cambió, ni motor_olvidar_en_nativa. */
#define MAX_NATIVAS 20                         /* los niveles de caja.c (16) y algo más */
static volatile int nativas;                   /* funciones nativas en curso */
static uintptr_t ret_jit[MAX_NATIVAS];         /* dónde vuelve cada una, en el código traducido */
static volatile uint32_t jit_generacion;       /* sube cada vez que el JIT lo olvida todo */

extern struct List LRU;
extern struct List ICache[EMU68_HASHSIZE];

static struct M68KTranslationUnit *unidad_de(struct Node *n)
{
    return (struct M68KTranslationUnit *)((char *)n - __builtin_offsetof(struct M68KTranslationUnit, mt_LRUNode));
}

/* La unidad que contiene `arm` (una dirección de vuelta en el código traducido, en su proyección de ejecutar), fuera
   de la lista LRU y de la tabla: así nada la suelta. NULL si no está (ya la fijó un nivel de fuera, o no es del JIT). */
static struct M68KTranslationUnit *fijar(uintptr_t arm)
{
    struct Node *n;
    ForeachNode(&LRU, n) {
        struct M68KTranslationUnit *u = unidad_de(n);
        uintptr_t ini = (uintptr_t)&u->mt_ARMCode[0] | 0x0000001000000000ULL;
        if (arm > ini && arm <= ini + 4 * ((uintptr_t)u->mt_ARMInsnCnt + 1)) {
            REMOVE(&u->mt_LRUNode);
            REMOVE(&u->mt_HashNode);
            return u;
        }
    }
    return NULL;
}

static void desfijar(struct M68KTranslationUnit *u)
{
    if (!u) return;
    ADDHEAD(&LRU, &u->mt_LRUNode);
    ADDHEAD(&ICache[(u->mt_M68kAddress >> EMU68_HASHSHIFT) & EMU68_HASHMASK], &u->mt_HashNode);
}

/* La unidad de la función nativa en curso, fijada (o NULL). */
static struct M68KTranslationUnit *fijar_la_de_ahora(void)
{
    int k = nativas;
    return k > 0 && k <= MAX_NATIVAS ? fijar(ret_jit[k - 1]) : NULL;
}

/* --- La llamada nativa --- */

void dashos_native_call(uint64_t *regs, uint32_t index)
{
    int k = nativas;
    if (k < MAX_NATIVAS) ret_jit[k] = (uintptr_t)__builtin_return_address(0);
    nativas = k + 1;
    motor_regs r;
    for (int i = 0; i < 8; i++) {
        r.d[i] = (uint32_t)regs[19 + i];
        r.a[i] = (uint32_t)regs[i < 5 ? 13 + i : 22 + i];
    }
    caja_nativa(&r, index);
    for (int i = 0; i < 8; i++) {
        regs[19 + i] = r.d[i];
        regs[i < 5 ? 13 + i : 22 + i] = r.a[i];
    }
    nativas = k;
}

/* El MainLoop de un nivel: guarda v8-v15 y v19-v31 (lo que el JIT no recarga), pone la última PC a "ninguna" (así no
   salta al código de la unidad de fuera) y los devuelve al acabar. Todo en un asm: entre medias, el compilador no puede
   tener nada en esos registros. Los x0..x30 se dan por perdidos: los guarda quien llama (los callee-saved, el
   compilador, por la lista) y el JIT al volver de la función nativa. Salvo x29: GCC no lo guarda aunque esté en la
   lista (es el puntero de marco), y MainLoop deja en él la A7 del 68k; lo guarda el asm. Sin eso, quien llama (Rust)
   seguía con la A7 donde tenía un puntero suyo: Ctrl-C a un programa en Wait rompía la caja. */
__attribute__((noipa)) static void main_loop_anidado(void)
{
    __asm__ volatile(
        "sub sp, sp, #352\n"
        "stp q8, q9, [sp, #0]\n"     "stp q10, q11, [sp, #32]\n"  "stp q12, q13, [sp, #64]\n"
        "stp q14, q15, [sp, #96]\n"  "stp q19, q20, [sp, #128]\n" "stp q21, q22, [sp, #160]\n"
        "stp q23, q24, [sp, #192]\n" "stp q25, q26, [sp, #224]\n" "stp q27, q28, [sp, #256]\n"
        "stp q29, q30, [sp, #288]\n" "str q31, [sp, #320]\n" "str x29, [sp, #336]\n"
        "mov w0, #-1\n"
        "mov " CTX_LAST_PC_ASM ", w0\n"
        "bl MainLoop\n"
        "ldr x29, [sp, #336]\n"
        "ldp q8, q9, [sp, #0]\n"     "ldp q10, q11, [sp, #32]\n"  "ldp q12, q13, [sp, #64]\n"
        "ldp q14, q15, [sp, #96]\n"  "ldp q19, q20, [sp, #128]\n" "ldp q21, q22, [sp, #160]\n"
        "ldp q23, q24, [sp, #192]\n" "ldp q25, q26, [sp, #224]\n" "ldp q27, q28, [sp, #256]\n"
        "ldp q29, q30, [sp, #288]\n" "ldr q31, [sp, #320]\n"
        "add sp, sp, #352\n"
        ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15",
            "x16", "x17", "x18", "x19", "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28", "x30",
            "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v16", "v17", "v18", "memory", "cc");
}

/* El SR del 68k que corre (el del registro, con C y V cambiados de sitio respecto al de M68KState). */
uint32_t motor_sr(void)
{
    uint32_t r;
    __asm__ volatile("umov %w0, " REG_SR_ASM : "=r"(r));
    return (r & ~3u) | ((r & 1) << 1) | ((r >> 1) & 1);
}

#define LEE_V21(i) ({ uint32_t _v; __asm__ volatile("mov %w0, v21.s[" #i "]" : "=r"(_v)); _v; })

void motor_pilas(uint32_t *usp, uint32_t *isp, uint32_t *msp)
{
    *usp = LEE_V21(1);
    *isp = LEE_V21(2);
    *msp = LEE_V21(3);
}

void motor_poner_usp(uint32_t usp) { __asm__ volatile("mov " REG_USP_ASM ", %w0" :: "r"(usp)); }

void motor_anidado(uint32_t pc, motor_regs *r, uint32_t sr, uint32_t usp, uint32_t isp, uint32_t msp)
{
    /* Lo arquitectónico del contexto de fuera: el MainLoop de dentro lo pisa al acabar. */
    struct { uint32_t d[8], a[8], pc, usp, isp, msp, vbr; uint16_t sr; } antes;
    for (int i = 0; i < 8; i++) { antes.d[i] = m68k.D[i].u32; antes.a[i] = m68k.A[i].u32; }
    antes.pc = m68k.PC; antes.usp = m68k.USP.u32; antes.isp = m68k.ISP.u32; antes.msp = m68k.MSP.u32;
    antes.vbr = m68k.VBR; antes.sr = m68k.SR;

    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = r->d[i]; m68k.A[i].u32 = r->a[i]; }
    m68k.USP.u32 = usp; m68k.ISP.u32 = isp; m68k.MSP.u32 = msp;
    m68k.SR = (uint16_t)sr;
    m68k.PC = pc;

    struct M68KTranslationUnit *u = fijar_la_de_ahora();
    uint32_t gen = jit_generacion;
    main_loop_anidado();
    desfijar(u);
    /* Si el JIT lo olvidó todo dentro, la unidad de la última PC de fuera ya no existe. */
    if (gen != jit_generacion) __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));

    for (int i = 0; i < 8; i++) { r->d[i] = m68k.D[i].u32; r->a[i] = m68k.A[i].u32; }
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = antes.d[i]; m68k.A[i].u32 = antes.a[i]; }
    m68k.PC = antes.pc; m68k.USP.u32 = antes.usp; m68k.ISP.u32 = antes.isp; m68k.MSP.u32 = antes.msp;
    m68k.VBR = antes.vbr; m68k.SR = antes.sr;
}

__attribute__((noipa)) void motor_arrancar(uint32_t pc, uint32_t a7, uint32_t a6, uint32_t a0, uint32_t d0,
                                           uint32_t super, uint32_t vbr)
{
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = 0; m68k.A[i].u32 = 0; }
    m68k.A[0].u32 = a0;
    m68k.D[0].u32 = d0;
    for (int fp = 0; fp < 8; fp++) m68k.FP[fp].u64 = 0x7fffffffffffffffULL;
    m68k.USP.u32 = a7;
    m68k.ISP.u32 = super;
    m68k.MSP.u32 = super;
    m68k.A[7].u32 = a7;
    m68k.A[6].u32 = a6;
    m68k.VBR = vbr;
    m68k.PC = pc;
    m68k.SR = 0;
    nativas = 0;
    m68k.CACR = 0x80008000;
    m68k.INT64 = 0;
    plazo_marca(chips_en);    /* K8: el plazo de los chips sigue armado si están encendidos */
    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    __asm__ volatile("mov " CTX_POINTER_ASM ", %0" ::"r"(&m68k));
    /* x29 lo guarda el asm: GCC no lo hace aunque esté en la lista (ver main_loop_anidado). */
    __asm__ volatile("str x29, [sp, #-16]!\n" "bl MainLoop\n" "ldr x29, [sp], #16"
                     ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10",
                     "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22",
                     "x23", "x24", "x25", "x26", "x27", "x28", "x30", "v0", "v1", "v2", "v3", "v4",
                     "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16", "v17",
                     "v18", "v27", "v28", "v29", "v30", "v31", "memory", "cc");
}

/* --- Olvidar lo traducido --- */

/* El JIT olvida todo lo traducido: un programa nuevo puede ir donde estaba otro (AllocMem da las mismas direcciones),
   y Emu68 encontraría en su caché lo traducido del de antes. Como el CINV/CPUSH del 68k (M68k_LINEF.c), y además
   suelta las unidades (como cuando se queda sin sitio, M68k_Translator.c). Solo entre programas, con el 68k parado:
   desde una llamada nativa soltaría el código que está corriendo. */
void motor_olvidar(void)
{
    jit_generacion++;
    /* K7: antes de soltar las unidades, sus sitios reescritos vuelven a su instrucción (también los de las unidades
       fijadas, que siguen vivas: si vuelven a los chips, fallan y se reescriben otra vez). */
    trampolines_deshacer();
    struct Node *n;
    while ((n = REMTAIL(&LRU)) != NULL) {
        struct M68KTranslationUnit *u =
            (struct M68KTranslationUnit *)((char *)n - __builtin_offsetof(struct M68KTranslationUnit, mt_LRUNode));
        REMOVE(&u->mt_HashNode);
        tlsf_free(jit_tlsf, u);
        m68k.JIT_UNIT_COUNT--;
    }
    LRU_InvalidateAll();
    cache_invalidate_all(ICACHE);
    M68K_ResetReturnStack();
    m68k.JIT_CACHE_FREE = tlsf_get_free_size(jit_tlsf);
}

/* Desde una función nativa (CacheClearU, SetFunction...): todo menos la unidad que la llamó, que sigue ejecutándose
   al volver (las de los niveles de fuera ya están fijadas). La última PC pasa a "ninguna": su unidad puede no existir. */
void motor_olvidar_en_nativa(void)
{
    struct M68KTranslationUnit *u = fijar_la_de_ahora();
    motor_olvidar();
    desfijar(u);
    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
}

/* --- La coma flotante del 68k --- */

/* FP0-FP7 en d8-d15 y FPSR, FPIAR y FPCR en v19 (M68k.h). v[0..7] = FP0-FP7, v[8] = FPSR, v[9] = FPIAR,
   v[10] = FPCR. Sin marco y sin decir al compilador que se tocan d8-d15: los callee-saved, quien llama (Rust, softfloat,
   no los usa; dashos_native_call tampoco: lo mira la bitácora) no los guarda ni los repone, así que lo que pone
   caja_fp_poner llega al JIT al volver de la llamada nativa. v19 es de los fijos (-ffixed-v19). */
__attribute__((naked)) void caja_fp_leer(uint64_t *v)
{
    __asm__ volatile(
        "stp d8, d9, [x0, #0]\n"   "stp d10, d11, [x0, #16]\n"
        "stp d12, d13, [x0, #32]\n" "stp d14, d15, [x0, #48]\n"
        "mov w1, " REG_FPSR_ASM "\n"  "str x1, [x0, #64]\n"
        "mov w1, " REG_FPIAR_ASM "\n" "str x1, [x0, #72]\n"
        "umov w1, " REG_FPCR_ASM "\n" "str x1, [x0, #80]\n"
        "ret\n");
}

__attribute__((naked)) void caja_fp_poner(const uint64_t *v)
{
    __asm__ volatile(
        "ldp d8, d9, [x0, #0]\n"   "ldp d10, d11, [x0, #16]\n"
        "ldp d12, d13, [x0, #32]\n" "ldp d14, d15, [x0, #48]\n"
        "ldr x1, [x0, #64]\n" "mov " REG_FPSR_ASM ", w1\n"
        "ldr x1, [x0, #72]\n" "mov " REG_FPIAR_ASM ", w1\n"
        "ldr x1, [x0, #80]\n" "mov " REG_FPCR_ASM ", w1\n"
        "ret\n");
}
