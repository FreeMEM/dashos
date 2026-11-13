/*
    DashOS, H4: la caja Amiga, primer paso.

    Un proceso de usuario big-endian con el JIT de Emu68 dentro. Ejecuta programas 68k planos (de pruebas68k/,
    metidos en el ejecutable) y ejecutables hunk de AmigaOS (hunk.c), cada uno como un proceso de exec, con ExecBase
    en la dirección 4 (la página 0, de solo lectura, con el permiso de su llave 1). Las bibliotecas son nativas, en
    Rust (pegamento/ enlaza los crates exec, dos e intuition): cada entrada de sus tablas de saltos es
    "$A0FF <número> RTS", y el JIT convierte la línea A en una llamada a dashos_native_call (parche 0002 de
    patches/emu68), que la pasa a caja_llamada. dos va sobre el servicio de archivos (llave 3) e intuition sobre el
    compositor por Wayland (llave 2): una ventana Amiga es una ventana del escritorio.

    Memoria del proceso (docs/referencia/kernel.md, mapa de direcciones):
      $0000_0000 - $0000_0FFF  la página 0, de solo lectura: ExecBase en la dirección 4 (llamada 49 del kernel)
      $0010_0000 - $004F_FFFF  memoria del 68k (4 MB): vectores, la zona del sistema de exec, los programas planos y,
                               desde $0015_0000, lo que reparte AllocMem (caja.h)
      4 GB                     el código traducido, para escribir
      4 GB + 64 GB             el mismo código, para ejecutar (bit 36: lo pone Emu68 en mt_ARMEntryPoint)
      8 GB                     el montón general de Emu68 (TLSF)
      10 GB                    el montón de Rust (TLSF): el estado de exec, dos e intuition y los lienzos de las ventanas
      12 GB                    las pilas nativas (la del JIT y la del vigilante), fuera del alcance del 68k
      14 GB                    la página 0, para escribirla
      16 GB                    la ventana del servicio de archivos
      20 GB                    los búferes de las ventanas (wl_shm), 64 MB por ventana
      128 - 256 GB             este ejecutable: un PIE estático, en una base al azar (enlace.ld; Emu68 carga las
                               direcciones de sus funciones con 64 bits, parche 0014)
      256 - 384 GB             la pila con que lo arranca el kernel (entrada.S pasa enseguida a la de 12 GB)
    En los 4 GB bajos, los que ve el 68k, no hay nada de la caja (S28): lo comprueba caja_fuera_del_68k.

    Sin libc: las llamadas al sistema van a mano, y las funciones de cadena y memoria son las de src/support.c
    de Emu68.
*/

#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>
#include "M68k.h"
#include "support.h"
#include "tlsf.h"
#include "cache.h"
#include "config.h"
#include "caja.h"
#include "hunk.h"

/* --- Llamadas al sistema de DashOS (docs/referencia/kernel.md) --- */

static long sc(long n, long a, long b, long c, long d, long e)
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

#define SC_ESCRIBIR          1
#define SC_SALIR             2
#define SC_MEMORIA_CREAR     7
#define SC_MEMORIA_PROYECTAR 8
#define SC_DORMIR            30
#define SC_HILO_CREAR        40
#define SC_HILO_UNIR         41
#define SC_PROCESO_SALIR     44
#define SC_PAGINA_CERO       49
#define SC_ESPERAR           42
#define E_ARGUMENTO          (-1L)

#define P_LEER     1
#define LLAVE_PAGINA0 1   /* la llave 1 de la caja: el permiso de página 0 que le da `inicio` */
#define P_ESCRIBIR 3
#define P_EJECUTAR 5

static uint64_t ahora_us(void)
{
    uint64_t t, f;
    __asm__ volatile("isb; mrs %0, cntvct_el0; mrs %1, cntfrq_el0" : "=r"(t), "=r"(f));
    return f ? t / f * 1000000 + t % f * 1000000 / f : 0;   /* sin división de 128 bits (no hay libgcc) */
}

/* --- Salida por la consola de DashOS --- */

static char linea[256];
static int largo;

static void vaciar(void)
{
    int hecho = 0;
    while (hecho < largo) {
        long n = sc(SC_ESCRIBIR, (long)(linea + hecho), largo - hecho, 0, 0, 0);
        if (n <= 0) break;
        hecho += (int)n;
    }
    largo = 0;
}

static void pon_c(void *data, char c)
{
    (void)data;
    linea[largo++] = c;
    if (c == '\n' || largo == (int)sizeof(linea)) vaciar();
}

/* Lo que dice la caja: "  [caja-amiga] ..." */
void di(const char *formato, ...)
{
    va_list v;
    for (const char *m = "  [caja-amiga] "; *m; m++) pon_c(NULL, *m);
    va_start(v, formato);
    vkprintf_pc(pon_c, NULL, formato, v);
    va_end(v);
    if (largo) vaciar();
}

/* Lo que dice Emu68 (kprintf): callado mientras arranca, que habla mucho; después, con su marca. */
static int emu68_habla;
static int emu68_linea_nueva = 1;

static void pon_emu68(void *data, char c)
{
    if (!emu68_habla) return;
    if (emu68_linea_nueva) {
        const char *m = "  [caja-amiga] emu68: ";
        while (*m) pon_c(data, *m++);
        emu68_linea_nueva = 0;
    }
    pon_c(data, c);
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

/* --- Lo que Emu68 define en start.c y disasm.c --- */

struct M68KState *__m68k_state;
int debug_not_implemented = 0;
int dcache_mask_bits;

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

/* --- Memoria --- */

#define JIT_RW   0x0000000100000000UL
#define JIT_RX   (JIT_RW | 0x0000001000000000UL)
#define JIT_LEN  (4UL << 20)
#define MONTON   0x0000000200000000UL
#define MONTON_LEN (4UL << 20)
#define MONTON_RUST 0x0000000280000000UL   /* el montón de Rust, aparte del de Emu68 */
#define MONTON_RUST_LEN (8UL << 20)
#define PILAS    0x0000000300000000UL
#define PILA_LEN (1UL << 20)
#define PILA_VIGIA_LEN (64UL << 10)
#define PAGINA0_RW 0x0000000380000000UL   /* la página 0, para escribirla: por encima de 4 GB, el 68k no llega */


/* Un objeto de memoria nuevo, proyectado en `va`. Devuelve su llave, o un error negativo. */
static long proyecta(uintptr_t va, size_t len, int permisos)
{
    long m = sc(SC_MEMORIA_CREAR, (long)len, 0, 0, 0, 0);
    if (m <= 0) return m ? m : -1;
    long r = sc(SC_MEMORIA_PROYECTAR, m, (long)va, permisos, 0, 0);
    return r ? r : m;
}

/* El SP con que arrancó el kernel la caja: el tope de la pila del ELF. */
static uintptr_t pila_elf;

/* La llama entrada.S antes que nada (con el SP del kernel): el tope de una pila propia, en 12 GB. 0 si no hay. */
uintptr_t caja_pila(uintptr_t sp_elf)
{
    pila_elf = sp_elf;
    return proyecta(PILAS, PILA_LEN, P_ESCRIBIR) > 0 ? PILAS + PILA_LEN : 0;
}

/* Lo que Rust (pegamento/) usa como memoria: un TLSF propio, para que lo suyo no se mezcle con lo del JIT. */
static void *tlsf_rust;

void *caja_monton_reservar(size_t tam, size_t alin)
{
    return tlsf_rust ? tlsf_malloc_aligned(tlsf_rust, tam, alin < 8 ? 8 : alin) : NULL;
}

void caja_monton_soltar(void *p)
{
    if (p && tlsf_rust) tlsf_free(tlsf_rust, p);
}

/* Lo que core de Rust pide y support.c de Emu68 no tiene. */
int memcmp(const void *a, const void *b, size_t n)
{
    const uint8_t *x = a, *y = b;
    for (size_t i = 0; i < n; i++)
        if (x[i] != y[i]) return x[i] < y[i] ? -1 : 1;
    return 0;
}

/* --- Las llamadas nativas: todas a las bibliotecas de Rust --- */

/* volatile: los cambia dashos_native_call, que llama el código traducido desde dentro del asm de `arranca`; sin él, gcc
   (análisis entre funciones) da por hecho que `arranca` no los toca y no los vuelve a leer. */
/* CAJA_INTERACTIVA=1 al compilar (make -C caja CAJA_INTERACTIVA=1): las pruebas que esperan a alguien, para sacar
   capturas: GadTools espera 90 s y un programa de Dash sin consola saca su error en un requester. */
#ifndef CAJA_INTERACTIVA
#define CAJA_INTERACTIVA 0
#endif
#define GT_NOMBRE (CAJA_INTERACTIVA ? "gadtools-90" : "gadtools-5")
/* BOOPSI y ReAction (boopsi.c): sin esperar; en el modo de las capturas, su ventana de ReAction 20 s con cada aspecto
   (el segundo, con la excepción "original" de DASHOS_PREFS para ese nombre). */
#define BOOPSI_NOMBRE (CAJA_INTERACTIVA ? "boopsi-20" : "boopsi-0")

/* Lo más que una tarea espera sin que llegue nada antes de que la caja la corte (pegamento/: esperar). */
uint64_t caja_espera_maxima_us(void) { return (CAJA_INTERACTIVA ? 600ULL : 60ULL) * 1000000ULL; }

static volatile int llamadas_nativas;
static volatile int rechazadas;
static volatile int cortadas;

static struct M68KState m68k;

/* --- Llamadas anidadas: código 68k desde una función nativa --- */

/* Cada función nativa corre dentro de una unidad traducida del JIT (la que tiene su $A0FF), y esa unidad sigue
   ejecutándose al volver. Si desde la función se ejecuta 68k otra vez (caja_ejecutar_68k), hay un MainLoop dentro de
   otro: una pila de contextos. Cada nivel guarda lo que el MainLoop de dentro cambia (los registros fijos del 68k que
   no recarga el JIT al volver de la función: v8-v15, los de coma flotante, y v19-v31, el SR, las pilas, el puntero al
   contexto, la última PC...) y lo devuelve al acabar; los x0..x30 los recarga el propio JIT (parche 0002). Y saca su
   unidad de la lista LRU y de la tabla del JIT mientras tanto (fijar), para que nada la suelte: ni el JIT al quedarse
   sin sitio, ni al ver que su código cambió, ni caja_jit_olvidar.

   El nivel acaba cuando el 68k vuelve a VUELTA_ANIDADA (la dirección de vuelta que se le pone): la función nativa
   N_VUELTA lo apunta y el código salta a la dirección 0, donde acaba ese MainLoop. Si acaba de otra forma (el programa
   acabó dentro, una excepción sin tc_TrapCode, el vigilante lo cortó), el programa entero se da por acabado y se
   deshacen los niveles de fuera: cada uno vuelve con ANIDADA_ACABADO, su función nativa corta el programa (la NMI) y
   su MainLoop acaba por el vector 31; ninguna llamada nativa más se ejecuta entretanto. */
#define MAX_ANIDADO 16
#define MAX_NATIVAS (MAX_ANIDADO + 4)
static volatile int anidado;                  /* niveles abiertos */
static volatile int vuelto[MAX_ANIDADO + 1];  /* 1: volvió a su VUELTA; 2: volvió con la pila más arriba (rota) */
static uint32_t a7_esperado[MAX_ANIDADO + 1];
static volatile int programa_acabado;         /* acabó dentro de un nivel: se deshacen todos */
static volatile uint32_t fin_marca, fin_d0;   /* cómo acabó (MARCA y D0 de ese momento) */
static volatile int nativas;                  /* funciones nativas en curso (una por nivel, más la de arriba) */
static uintptr_t ret_jit[MAX_NATIVAS];        /* dónde vuelve cada una, en el código traducido */
static volatile uint32_t jit_generacion;      /* sube cada vez que el JIT lo olvida todo */
static volatile int pilas_raras;
static volatile int ctrl_c_pendiente;

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

void dashos_native_call(uint64_t *regs, uint32_t index)
{
    if (index == N_VUELTA) {
        /* Vuelve el nivel de dentro. Con la pila donde se le dejó, bien; más abajo (dejó algo en ella), se acepta: la
           de fuera está intacta. Más arriba, ha deshecho marcos de fuera (un longjmp, otro nivel): se corta. Sin
           niveles (un programa que salta aquí), su código sigue a la dirección 0 y el programa acaba. */
        int n = anidado;
        if (n > 0 && !programa_acabado) {
            uint32_t a7 = lee_r(regs, R_A(7));
            if (a7 > a7_esperado[n]) vuelto[n] = 2;
            else { vuelto[n] = 1; if (a7 < a7_esperado[n]) pilas_raras++; }
        }
        return;
    }
    if (programa_acabado) {
        /* Se está deshaciendo: nada más se ejecuta, y este nivel acaba en cuanto vuelva a su MainLoop. */
        pon_r(regs, R_D(0), 0);
        __atomic_store_n(&m68k.INTF.ARM_err, 1, __ATOMIC_RELEASE);
        return;
    }
    if (index == N_INTERRUPCION) __atomic_store_n(&m68k.INTF.ARM, 0, __ATOMIC_RELEASE);
    int k = nativas;
    if (k < MAX_NATIVAS) ret_jit[k] = (uintptr_t)__builtin_return_address(0);
    nativas = k + 1;
    llamadas_nativas++;
    switch (caja_llamada(regs, index)) {
        case 1:   /* nadie tiene ese número: rechazada, como una función que falla, y el programa sigue */
            rechazadas++;
            llamadas_nativas--;
            break;
        case 2:   /* la tarea se quitó o esperaba sin remedio: la interrupción de nivel 7 corta el programa */
            cortadas++;
            __atomic_store_n(&m68k.INTF.ARM_err, 1, __ATOMIC_RELEASE);
            break;
        default:
            break;
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
static uint32_t sr_vivo(void)
{
    uint32_t r;
    __asm__ volatile("umov %w0, " REG_SR_ASM : "=r"(r));
    return (r & ~3u) | ((r & 1) << 1) | ((r >> 1) & 1);
}

#define LEE_V21(i) ({ uint32_t _v; __asm__ volatile("mov %w0, v21.s[" #i "]" : "=r"(_v)); _v; })

int caja_ejecutar_68k(uint32_t pc, uint32_t *r, int usuario)
{
    if (programa_acabado) return ANIDADA_ACABADO;
    if (anidado >= MAX_ANIDADO) return ANIDADA_TOPE;
    uint32_t sr = sr_vivo(), usp = LEE_V21(1), isp = LEE_V21(2), msp = LEE_V21(3);
    uint32_t a7 = r[15];
    if (usuario && (sr & SR_S)) { a7 = usp; sr &= ~(SR_S | SR_M); }
    a7 = (a7 - 4) & ~1u;
    /* Sitio en la pila para la vuelta y algo más: si no, no se ejecuta (el 68k escribiría fuera de su memoria). */
    if (a7 < M68K_RAM + 256 || !en_ram68k(a7, 4)) return ANIDADA_PILA;
    pon32(a7, (uint32_t)VUELTA_ANIDADA);

    /* Lo arquitectónico del contexto de fuera: el MainLoop de dentro lo pisa al acabar. */
    struct { uint32_t d[8], a[8], pc, usp, isp, msp, vbr; uint16_t sr; } antes;
    for (int i = 0; i < 8; i++) { antes.d[i] = m68k.D[i].u32; antes.a[i] = m68k.A[i].u32; }
    antes.pc = m68k.PC; antes.usp = m68k.USP.u32; antes.isp = m68k.ISP.u32; antes.msp = m68k.MSP.u32;
    antes.vbr = m68k.VBR; antes.sr = m68k.SR;

    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = r[i]; m68k.A[i].u32 = r[8 + i]; }
    m68k.A[7].u32 = a7;
    if (sr & SR_S) { if (sr & SR_M) msp = a7; else isp = a7; } else usp = a7;
    m68k.USP.u32 = usp; m68k.ISP.u32 = isp; m68k.MSP.u32 = msp;
    m68k.SR = (uint16_t)sr;
    m68k.PC = pc;

    int n = ++anidado;
    vuelto[n] = 0;
    a7_esperado[n] = a7 + 4;
    struct M68KTranslationUnit *u = nativas > 0 && nativas <= MAX_NATIVAS ? fijar(ret_jit[nativas - 1]) : NULL;
    uint32_t gen = jit_generacion;
    main_loop_anidado();
    desfijar(u);
    /* Si el JIT lo olvidó todo dentro, la unidad de la última PC de fuera ya no existe. */
    if (gen != jit_generacion) __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    anidado = n - 1;

    for (int i = 0; i < 8; i++) { r[i] = m68k.D[i].u32; r[8 + i] = m68k.A[i].u32; }
    int v = vuelto[n];
    if (v != 1 && !programa_acabado) {
        programa_acabado = 1;
        fin_marca = v == 2 ? 0xfffe : lee16(MARCA);
        fin_d0 = m68k.D[0].u32;
    }
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = antes.d[i]; m68k.A[i].u32 = antes.a[i]; }
    m68k.PC = antes.pc; m68k.USP.u32 = antes.usp; m68k.ISP.u32 = antes.isp; m68k.MSP.u32 = antes.msp;
    m68k.VBR = antes.vbr; m68k.SR = antes.sr;
    return v == 1 ? ANIDADA_BIEN : ANIDADA_ACABADO;
}

int caja_tomar_ctrl_c(void) { return __atomic_exchange_n(&ctrl_c_pendiente, 0, __ATOMIC_ACQ_REL); }

int caja_tomar_pilas_raras(void) { int n = pilas_raras; pilas_raras = 0; return n; }

/* --- Los programas 68k --- */

extern const uint8_t prog_hola[], prog_hola_fin[];
extern const uint8_t prog_lineaa[], prog_lineaa_fin[];
extern const uint8_t prog_bucle[], prog_bucle_fin[];
extern const uint8_t prog_mala[], prog_mala_fin[];
extern const uint8_t prog_saludo[], prog_saludo_fin[];
extern const uint8_t prog_ventana[], prog_ventana_fin[];
extern const uint8_t prog_hostil[], prog_hostil_fin[];
extern const uint8_t prog_dash_hola[], prog_dash_hola_fin[];
extern const uint8_t prog_dash_texto[], prog_dash_texto_fin[];
extern const uint8_t prog_dash_texto_gcc[], prog_dash_texto_gcc_fin[];
extern const uint8_t prog_dash_ventana[], prog_dash_ventana_fin[];
extern const uint8_t prog_dash_interp[], prog_dash_interp_fin[];
extern const uint8_t prog_dash_listas[], prog_dash_listas_fin[];
extern const uint8_t prog_anidado[], prog_anidado_fin[];
extern const uint8_t prog_gadtools[], prog_gadtools_fin[];
extern const uint8_t prog_boopsi[], prog_boopsi_fin[];
extern const uint8_t prog_ramlib[], prog_ramlib_fin[];
extern const uint8_t prog_dash_test_reaction[], prog_dash_test_reaction_fin[];
extern const uint8_t prog_dash_test_doc_reaction[], prog_dash_test_doc_reaction_fin[];
extern const uint8_t prog_dash_ctrl_c[], prog_dash_ctrl_c_fin[];
extern const uint8_t prog_dash_ctrl_c_gcc[], prog_dash_ctrl_c_gcc_fin[];
extern const uint8_t prog_dash_ctrl_c_for[], prog_dash_ctrl_c_for_fin[];
extern const uint8_t prog_dash_trap_ilegal[], prog_dash_trap_ilegal_fin[];
extern const uint8_t prog_dash_trap_divcero[], prog_dash_trap_divcero_fin[];
extern const uint8_t prog_dash_test_gadtools[], prog_dash_test_gadtools_fin[];

/* Los vectores. Las excepciones del procesador van al despacho de sistema.s ("move.l #i,-(sp); jmp DESPACHO"), que
   hace lo de exec: a tc_TrapCode de la tarea con el número encima del marco; si la tarea no tiene, el programa acaba
   por esa excepción (TRAMPA_DEFECTO: MARCA = i y a COMUN). Las interrupciones de los niveles 1 a 5 y 7 (el vigilante)
   acaban el programa sin más ("move.w #i,MARCA; jmp COMUN"); la de nivel 6 es la de la caja (INTERRUPCION68K). COMUN
   pone una pila y vuelve a la dirección 0, donde MainLoop termina. Al volver de su función, cada tarea salta también a
   COMUN (su dirección de vuelta). */
extern const uint8_t prog_sistema[], prog_sistema_fin[];
/* La foto de los vectores (S28): dos tramos que solo escribe prepara_vectores: los vectores, las trampas y COMUN
   ([VECTORES, COMUN + 16); MARCA y la pila de COMUN cambian siempre) y el código de sistema.s ([SISTEMA68K, SUPER_BAJO);
   SUPER_BAJO lo pone caja_main después). */
static const struct { uintptr_t desde, hasta; } TRAMOS_FOTO[] = { { VECTORES, COMUN + 16 }, { SISTEMA68K, SUPER_BAJO } };
static uint8_t vectores_foto[(COMUN + 16 - VECTORES) + (SUPER_BAJO - SISTEMA68K)];
static int vectores_deshechos;
static void prepara_vectores(void)
{
    for (uint32_t i = 0; i < 256; i++) {
        uintptr_t t = TRAMPAS + 14 * i;
        pon32(VECTORES + 4 * i, (uint32_t)t);
        if (i >= 24 && i <= 31) {
            pon16(t, 0x33fc); pon16(t + 2, (uint16_t)i);                      /* move.w #i,MARCA */
            pon32(t + 4, (uint32_t)MARCA);
            pon16(t + 8, 0x4ef9); pon32(t + 10, (uint32_t)COMUN);             /* jmp COMUN */
        } else {
            pon16(t, 0x2f3c); pon32(t + 2, i);                                 /* move.l #i,-(sp) */
            pon16(t + 6, 0x4ef9); pon32(t + 8, (uint32_t)DESPACHO);           /* jmp DESPACHO */
        }
    }
    pon32(VECTORES + 4 * 30, (uint32_t)INTERRUPCION68K);
    pon16(COMUN, 0x4ff9); pon32(COMUN + 2, (uint32_t)(PILA_TOPE - 4));      /* lea PILA_TOPE-4,a7 */
    pon16(COMUN + 6, 0x4297);                                                  /* clr.l (a7) */
    pon16(COMUN + 8, 0x4e75);                                                  /* rts */
    uint8_t *d = (uint8_t *)SISTEMA68K;
    for (const uint8_t *p = prog_sistema; p < prog_sistema_fin && d < (uint8_t *)SUPER_BAJO; ) *d++ = *p++;
    uint32_t k = 0;
    for (unsigned t = 0; t < 2; t++)
        for (uintptr_t d = TRAMOS_FOTO[t].desde; d < TRAMOS_FOTO[t].hasta; d++) vectores_foto[k++] = *(volatile uint8_t *)d;
}

/* S28: los vectores del 68k, las trampas, COMUN y el código de sistema.s, como los dejó prepara_vectores. Un programa
   los puede cambiar (están en la memoria del 68k, que no tiene protección dentro de la caja): al acabar, vuelven a como
   estaban. Cuántas palabras cambió. */
static int vectores_deshacer(void)
{
    int n = 0;
    uint32_t k = 0;
    for (unsigned t = 0; t < 2; t++)
        for (uintptr_t d = TRAMOS_FOTO[t].desde; d < TRAMOS_FOTO[t].hasta; d += 4, k += 4) {
            uint32_t f = (uint32_t)vectores_foto[k] << 24 | (uint32_t)vectores_foto[k + 1] << 16
                | (uint32_t)vectores_foto[k + 2] << 8 | vectores_foto[k + 3];
            if (lee32(d) != f) { pon32(d, f); n++; }
        }
    return n;
}

/* La pila de supervisor: la de las excepciones y las interrupciones (los programas corren en modo de usuario, como
   las tareas de exec). De AllocMem, una vez. */
#define SUPER_LEN 16384
static uint32_t super_tope;

/* Ejecuta el 68k desde `pc` con A7 = `a7`, A6 = `a6`, A0 = `a0` y D0 = `d0` (el resto a 0), en modo de usuario (la
   pila de supervisor, la de la caja) y sin máscara de interrupciones, hasta que vuelve a la dirección 0. Devuelve el
   vector por el que acabó, -1 si acabó con su RTS, o -2 si una llamada anidada volvió deshaciendo marcos de fuera. Si
   el programa acabó dentro de una llamada anidada, lo de ese momento (su vector y su D0). */
__attribute__((noipa)) static int arranca(uint32_t pc, uint32_t a7, uint32_t a6, uint32_t a0, uint32_t d0)
{
    pon16(MARCA, 0xffff);
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = 0; m68k.A[i].u32 = 0; }
    m68k.A[0].u32 = a0;
    m68k.D[0].u32 = d0;
    for (int fp = 0; fp < 8; fp++) m68k.FP[fp].u64 = 0x7fffffffffffffffULL;
    m68k.USP.u32 = a7;
    m68k.ISP.u32 = super_tope;
    m68k.MSP.u32 = super_tope;
    m68k.A[7].u32 = a7;
    m68k.A[6].u32 = a6;
    m68k.VBR = VECTORES;
    m68k.PC = pc;
    m68k.SR = 0;
    anidado = 0;
    programa_acabado = 0;
    nativas = 0;
    m68k.CACR = 0x80008000;
    m68k.INT64 = 0;
    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    __asm__ volatile("mov " CTX_POINTER_ASM ", %0" ::"r"(&m68k));
    /* x29 lo guarda el asm: GCC no lo hace aunque esté en la lista (ver main_loop_anidado). */
    __asm__ volatile("str x29, [sp, #-16]!\n" "bl MainLoop\n" "ldr x29, [sp], #16"
                     ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10",
                     "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22",
                     "x23", "x24", "x25", "x26", "x27", "x28", "x30", "v0", "v1", "v2", "v3", "v4",
                     "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16", "v17",
                     "v18", "v27", "v28", "v29", "v30", "v31", "memory", "cc");
    uint16_t marca = lee16(MARCA);
    if (programa_acabado) {
        marca = (uint16_t)fin_marca;
        m68k.D[0].u32 = fin_d0;
        if (marca == 0xfffe) return -2;
    }
    return marca == 0xffff ? -1 : marca;
}

static size_t largo_de(const char *s) { size_t n = 0; while (s[n]) n++; return n; }

/* Ejecuta código 68k como un proceso de exec llamado `nombre`, con A6 = `a6`, A0 los argumentos y D0 su largo (como
   desde el CLI). Al acabar, la caja suelta lo que el programa dejara (ficheros, ventanas, memoria): en *sobras,
   cuántos bloques de memoria no soltó él. Devuelve el vector por el que acabó, -1 si con su RTS, o -2 si no se pudo
   crear el proceso. */
static int ejecuta_tarea(const char *nombre, uint32_t pc, uint32_t a6, uint32_t seglist, uint32_t *sobras)
{
    static const char args[] = "\n";
    uint32_t args68k = caja_alloc_mem(sizeof(args), 0);
    if (args68k) for (unsigned i = 0; i < sizeof(args); i++) pon8(args68k + i, (uint8_t)args[i]);
    uint32_t sp = 0;
    uint32_t t = caja_tarea_nueva(nombre, largo_de(nombre), 16384, pc, (uint32_t)COMUN, args68k, seglist, &sp);
    if (!t) { if (args68k) caja_free_mem(args68k, sizeof(args)); return -2; }
    int v = arranca(pc, sp, a6, args68k, sizeof(args) - 1);
    vectores_deshechos = vectores_deshacer();
    if (vectores_deshechos) di("defensa: %d vectores del 68k cambiados por el programa: deshechos\n", vectores_deshechos);
    uint32_t s = caja_tarea_fin(t);
    if (sobras) *sobras = s;
    if (args68k) caja_free_mem(args68k, sizeof(args));
    return v;
}

/* El JIT olvida todo lo traducido: un programa nuevo puede ir donde estaba otro (AllocMem da las mismas direcciones),
   y Emu68 encontraría en su caché lo traducido del de antes. Como el CINV/CPUSH del 68k (M68k_LINEF.c), y además
   suelta las unidades (como cuando se queda sin sitio, M68k_Translator.c). Solo entre programas, con el 68k parado:
   desde una llamada nativa soltaría el código que está corriendo. */
static void jit_olvidar(void)
{
    jit_generacion++;
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
void caja_jit_olvidar(void)
{
    struct M68KTranslationUnit *u = nativas > 0 && nativas <= MAX_NATIVAS ? fijar(ret_jit[nativas - 1]) : NULL;
    jit_olvidar();
    desfijar(u);
    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
}

/* Carga un programa plano en `donde` (su dirección de enlace, en la zona PLANOS) y lo ejecuta con A6 = DOSBase. */
static int ejecuta(const char *nombre, const uint8_t *desde, const uint8_t *hasta, uintptr_t donde)
{
    uint8_t *d = (uint8_t *)donde;
    for (const uint8_t *s = desde; s < hasta; ) *d++ = *s++;
    jit_olvidar();
    return ejecuta_tarea(nombre, (uint32_t)donde, caja_dosbase(), 0, NULL);
}

/* --- Ejecutables hunk (hunk.c): su memoria, de AllocMem de exec --- */

static int hunks_vivos;

static uint32_t hunks_reservar(void *ctx, uint32_t bytes, uint32_t atributos)
{
    (void)ctx;
    uint32_t d = caja_alloc_mem(bytes, atributos);
    if (d) hunks_vivos++;
    return d;
}

static void hunks_liberar(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    hunks_vivos--;
    caja_free_mem(d, bytes);
}

static uint8_t *hunks_ver(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    return en_ram68k(d, bytes) ? (uint8_t *)(uintptr_t)d : NULL;
}

static const struct hunk_ops hunks_ops = { NULL, hunks_reservar, hunks_liberar, hunks_ver };

/* Para las pruebas hostiles del cargador (decenas de miles de cargas): un reparto que solo avanza, dentro de un bloque
   de AllocMem, como el de antes de exec. Con AllocMem en cada carga tardaban segundos, y con la máquina cargada eso
   retrasaba a los demás servicios del arranque. Una tanda más corta va con AllocMem de verdad. */
static uint32_t rapido_inicio, rapido_fin, rapido_libre;

static uint32_t rapido_reservar(void *ctx, uint32_t bytes, uint32_t atributos)
{
    (void)ctx; (void)atributos;
    if (bytes > rapido_fin - rapido_libre) return 0;
    uint32_t d = rapido_libre;
    rapido_libre += (bytes + 7) & ~7u;
    hunks_vivos++;
    return d;
}

static void rapido_liberar(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    hunks_vivos--;
    if (d + ((bytes + 7) & ~7u) == rapido_libre) rapido_libre = d;
}

static const struct hunk_ops rapido_ops = { NULL, rapido_reservar, rapido_liberar, hunks_ver };
#define MAX_HUNKS 256
static struct hunk_seg hunks_segs[MAX_HUNKS];

/* Una copia de un ejecutable para estropearla (las pruebas hostiles). */
static uint8_t hunk_copia[16384];
static uint64_t semilla = 0x9E3779B97F4A7C15ULL;
static uint32_t azar(void) { semilla ^= semilla << 13; semilla ^= semilla >> 7; semilla ^= semilla << 17; return (uint32_t)semilla; }

/* Carga un ejecutable hunk y lo ejecuta como un proceso, con A6 = ExecBase. Devuelve el vector como ejecuta_tarea, o
   -3 si no carga. En *d0 lo que devolvió el programa; en *llamadas, sus llamadas nativas. */
static int ejecuta_hunk(const char *nombre, const uint8_t *datos, size_t largo, uint32_t execbase, uint32_t *d0,
                        int *llamadas, uint32_t *sobras)
{
    struct hunk_carga c;
    int e = hunk_cargar(datos, largo, &hunks_ops, hunks_segs, MAX_HUNKS, &c);
    if (e != HUNK_BIEN || hunks_segs[0].tipo != HUNK_CODE) {
        di("%s no carga: %s\n", nombre, hunk_error(e));
        return -3;
    }
    di("%s: %u hunks (código %u, datos %u, BSS %u bytes) en $%x, %u reubicaciones\n", nombre, c.cuantos,
       hunks_segs[0].bytes, c.cuantos > 1 ? hunks_segs[1].bytes : 0, c.cuantos > 2 ? hunks_segs[2].bytes : 0,
       c.entrada, c.relocs);
    llamadas_nativas = 0;
    jit_olvidar();
    int v = ejecuta_tarea(nombre, c.entrada, execbase, c.seglist, sobras);
    *d0 = m68k.D[0].u32;
    *llamadas = llamadas_nativas;
    hunk_soltar(&hunks_ops, &c);
    return v;
}

/* --- El vigilante: corta un programa 68k que no acaba --- */

/* Un hilo aparte: duerme y después levanta la interrupción de nivel 7 del 68k (INTF.ARM_err, la NMI). El bucle
   traducido mira INT64 en cada vuelta y vuelve a MainLoop, que salta al vector 31: su trampa acaba el programa. */
static void vigila(uint64_t us)
{
    sc(SC_DORMIR, (long)us, 0, 0, 0, 0);
    __atomic_store_n(&m68k.INTF.ARM_err, 1, __ATOMIC_RELEASE);
    sc(SC_SALIR, 0, 0, 0, 0, 0);
    for (;;) {}
}

/* --- S28: los virus de laboratorio (pruebas68k/malware/, docs/diseño/S28-malware-amiga.md) --- */

extern const uint8_t prog_mal_arranque[], prog_mal_arranque_fin[];
extern const uint8_t prog_mal_enlace[], prog_mal_enlace_fin[];
extern const uint8_t prog_mal_parche[], prog_mal_parche_fin[];
extern const uint8_t prog_mal_residente[], prog_mal_residente_fin[];
extern const uint8_t prog_mal_guion[], prog_mal_guion_fin[];
extern const uint8_t prog_mal_troyano[], prog_mal_troyano_fin[];
extern const uint8_t prog_mal_memoria[], prog_mal_memoria_fin[];
extern const uint8_t prog_actualiza[], prog_actualiza_fin[];
/* De pegamento/ (Rust): lo que se deshizo tras el último programa, y las negativas de la guardia de sus ficheros; y
   cuántas veces preguntó la guardia al usuario (por `sesiones`) y cómo se decidió la última. */
void caja_defensa(uint32_t *entradas, uint32_t *campos);
uint32_t caja_negados(void);
uint32_t caja_preguntas(uint32_t *como);

/* Cómo se decidió una pregunta (usuario/src/preguntas_ficheros.rs). */
enum { COMO_SIN_PREGUNTAR, COMO_RECORDADO, COMO_SIEMPRE, COMO_ESTA_VEZ, COMO_DIJO_NO, COMO_OCUPADO, COMO_NADA = 0xff };
static const char *como_texto(uint32_t c)
{
    switch (c) {
    case COMO_SIN_PREGUNTAR: return "nadie a quien preguntar: no";
    case COMO_RECORDADO: return "sí, lo recuerda sesiones";
    case COMO_SIEMPRE: return "el usuario dice sí, siempre";
    case COMO_ESTA_VEZ: return "el usuario dice sí, solo esta vez";
    case COMO_DIJO_NO: return "el usuario dice no";
    case COMO_OCUPADO: return "ya preguntaba otra cosa: no";
    case COMO_NADA: return "sin preguntas";
    default: return "?";
    }
}
static int como_deja(uint32_t c) { return c == COMO_RECORDADO || c == COMO_SIEMPRE || c == COMO_ESTA_VEZ; }

/* Donde los virus de laboratorio cuentan cuántas veces se usaron sus parches y su código residente: una zona que exec
   no reparte (la de los programas planos, sin usar aquí). */
#define ZONA_LIBRE 0x0014F000UL

/* S28: nada de la caja al alcance del 68k. El 68k ve los 4 GB bajos del proceso tal cual (sus direcciones son de 32
   bits y el JIT las usa sin más); la caja va toda por encima. Comprueba dos cosas:
   - que el ejecutable (de __caja_imagen a __caja_fin), la pila del kernel, la pila de ahora, el estado del 68k, el
     código traducido, los montones, las pilas y la página 0 para escribir están por encima de 4 GB (en *bajas, cuántos
     no);
   - que en los 4 GB bajos no hay más memoria que la página 0 y la del 68k: se lo pregunta al kernel con la llamada 42
     (esperar), que da E_ARGUMENTO donde no hay memoria que leer. Una página de cada 64 KB (65.536 preguntas, medio
     segundo en QEMU; las 4 KB de todas, 8 s): lo que la caja proyecta, lo pone ella en sus direcciones fijas, y el
     ejecutable y su pila los pone el kernel por encima de 128 GB (lo de arriba). En *antes, si hay algo en $7000_0000,
     donde estuvo la caja hasta S28.
   Devuelve en cuántos trozos de 64 KB de los 4 GB bajos hay algo de más (0 es bien); en *trozos, cuántos miró, y en
   *ms, lo que tardó. */
extern char __caja_imagen[], __caja_fin[];
int caja_main(void);
static uint32_t caja_fuera_del_68k(uint32_t *bajas, int *antes, uint32_t *ms, uint32_t *trozos)
{
    uintptr_t sp;
    __asm__ volatile("mov %0, sp" : "=r"(sp));
    const uintptr_t suyas[] = {
        (uintptr_t)__caja_imagen, (uintptr_t)__caja_fin - 1, (uintptr_t)caja_main, (uintptr_t)&m68k, pila_elf - 1, sp,
        JIT_RW, JIT_RX, MONTON, MONTON_RUST, PILAS, PAGINA0_RW,
    };
    *bajas = 0;
    for (unsigned i = 0; i < sizeof(suyas) / sizeof(suyas[0]); i++)
        *bajas += suyas[i] < 0x100000000UL;
    uint64_t t0 = ahora_us();
    uint32_t de_mas = 0;
    *trozos = 0;
    for (uintptr_t p = 0x10000; p < 0x100000000UL; p += 0x10000) {
        if (p >= M68K_RAM && p < M68K_RAM + M68K_RAM_LEN)
            continue;
        ++*trozos;
        de_mas += sc(SC_ESPERAR, (long)p, 0x5A5A5A5A, 1, 0, 0) != E_ARGUMENTO;
    }
    *antes = sc(SC_ESPERAR, 0x70000000L, 0x5A5A5A5A, 1, 0, 0) != E_ARGUMENTO;
    *ms = (uint32_t)((ahora_us() - t0) / 1000);
    return de_mas;
}

/* Los siete, uno tras otro. Cada uno intenta lo de su familia y devuelve cuántas cosas consiguió que no debía (0); la
   caja comprueba además que acaba bien, que no deja memoria y que lo que cambió en exec (tablas de saltos, ExecBase,
   vectores del 68k) se deshizo. Al final, que nada de lo suyo se usó después (sus parches, su código residente).
   Devuelve cuántos fallos. */
static int prueba_malware(uint32_t execbase, uint32_t sistema)
{
    static const struct { const char *nombre; const uint8_t *desde, *hasta; } virus[] = {
        { "arranque.exe", prog_mal_arranque, prog_mal_arranque_fin },
        { "enlace.exe", prog_mal_enlace, prog_mal_enlace_fin },
        { "parche.exe", prog_mal_parche, prog_mal_parche_fin },
        { "residente.exe", prog_mal_residente, prog_mal_residente_fin },
        { "guion.exe", prog_mal_guion, prog_mal_guion_fin },
        { "troyano.exe", prog_mal_troyano, prog_mal_troyano_fin },
        { "memoria.exe", prog_mal_memoria, prog_mal_memoria_fin },
    };
    static const char prefijo[] = "  [caja-amiga] malware: ";
    int fallos = 0;
    pon32(ZONA_LIBRE, 0);
    pon32(ZONA_LIBRE + 4, 0);
    for (unsigned i = 0; i < sizeof(virus) / sizeof(virus[0]); i++) {
        uint32_t d0 = 0, sobras = 0, entradas = 0, campos = 0;
        int llamadas = 0;
        caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
        int v = ejecuta_hunk(virus[i].nombre, virus[i].desde, (size_t)(virus[i].hasta - virus[i].desde), execbase, &d0,
                             &llamadas, &sobras);
        caja_consola_prefijo(NULL, 0);
        caja_defensa(&entradas, &campos);
        int bien = v == -1 && d0 == 0 && sobras == 0 && caja_bloques(0) == sistema;
        if (virus[i].desde == prog_mal_memoria) {
            uint32_t bajas = 0, ms = 0, trozos = 0;
            int antes = 1;
            uint32_t de_mas = caja_fuera_del_68k(&bajas, &antes, &ms, &trozos);
            int fuera = de_mas == 0 && bajas == 0 && !antes && trozos > 65000;
            di("malware: memoria: la caja, toda por encima de 4 GB (%u direcciones suyas por debajo); en los 4 GB del "
               "68k, el kernel no ve más que la página 0 y la memoria del 68k (%u de %u trozos de 64 KB con algo de "
               "más; en $70000000, %s; %u ms): %s\n", bajas, de_mas, trozos, antes ? "algo" : "nada", ms, fuera ? "bien" : "MAL");
            bien = bien && fuera;
        }
        /* S28: la guardia pregunta al usuario una vez como mucho por virus (tras un no, nada más de ese programa), y
           ningún virus recibe un sí. */
        uint32_t como = COMO_NADA, preguntas = caja_preguntas(&como);
        bien = bien && preguntas <= 1 && !como_deja(como);
        if (virus[i].desde == prog_mal_parche) bien = bien && entradas == 6;
        else if (virus[i].desde == prog_mal_residente) bien = bien && campos == 2 && vectores_deshechos == 2;
        else bien = bien && entradas == 0 && campos == 0 && vectores_deshechos == 0;
        di("malware %s: %d conseguidos (vector %d), %u negativas de la guardia (%u preguntas: %s), %u avisos del "
           "Enforcer; deshecho: %u entradas de tablas, %u grupos de ExecBase, %d vectores del 68k: %s\n", virus[i].nombre,
           (int)d0, v, caja_negados(), preguntas, como_texto(como), caja_avisos(), entradas, campos, vectores_deshechos,
           bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }
    }
    /* Después de los siete: los parches de parche.exe solo se usaron en su programa (2 veces), el código residente de
       residente.exe nunca, y ExecBase no tiene vectores de reinicio. */
    uint32_t usos = lee32(ZONA_LIBRE), reinicio = lee32(ZONA_LIBRE + 4);
    int limpio = lee32(execbase + 42) == 0 && lee32(execbase + 46) == 0 && lee32(execbase + 50) == 0
        && lee32(execbase + 546) == 0 && lee32(execbase + 550) == 0;
    int bien = usos == 2 && reinicio == 0 && limpio;
    di("malware: tras los siete, los parches de parche.exe se usaron %u veces (las 2 de su programa), el código residente "
       "%u, y ExecBase sin vectores de reinicio: %s\n", usos, reinicio, limpio ? "bien" : "MAL");
    if (!bien) fallos++;
    return fallos;
}

/* S28: un programa honrado que actualiza otro (actualiza.exe reescribe Caja:C/viejo, un ejecutable), tres vueltas. La
   guardia pregunta al usuario por `sesiones` y `guardian`; en la prueba, el guion contesta "solo esta vez" la primera
   y "siempre" la segunda, y la tercera ya lo recuerda `sesiones` (sin el usuario). En cada vuelta, una pregunta, y el
   programa actualiza si y solo si le dejaron. Sin `sesiones` ni `guardian` (o sin Caja:), no le dejan: también bien. */
static int prueba_actualiza(uint32_t execbase, uint32_t sistema)
{
    static const char prefijo[] = "  [caja-amiga] ";
    int fallos = 0;
    for (int vuelta = 1; vuelta <= 3; vuelta++) {
        uint32_t d0 = 0, sobras = 0, como = COMO_NADA;
        int llamadas = 0;
        caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
        int v = ejecuta_hunk("actualiza.exe", prog_actualiza, (size_t)(prog_actualiza_fin - prog_actualiza), execbase,
                             &d0, &llamadas, &sobras);
        caja_consola_prefijo(NULL, 0);
        uint32_t preguntas = caja_preguntas(&como);
        int bien = v == -1 && sobras == 0 && caja_bloques(0) == sistema && preguntas <= 1
            && (d0 == 0) == (preguntas == 1 && como_deja(como));
        di("actualiza.exe, vuelta %d: %u preguntas (%s): %s: %s\n", vuelta, preguntas, como_texto(como),
           d0 == 0 ? "actualizado" : "no le dejan", bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }
    }
    return fallos;
}

/* Ctrl-C desde fuera (como el de una consola): otro hilo, a los `us`, lo pide y levanta la interrupción de nivel 6
   del 68k; la caja lo entrega como la señal SIGBREAKF_CTRL_C de la tarea. Si el programa no acaba en 10 s más, la
   NMI lo corta (el vigilante de siempre). Con `us` = 0, solo eso: un vigilante de 10 s, sin Ctrl-C. */
static volatile int ctrl_c_corriendo;
/* En vez de Ctrl-C, cerrar sus ventanas como con el botón de cerrar de la barra (un programa que solo espera a sus
   ventanas en Wait, como el ejemplo de ReAction de Dash, no ve Ctrl-C hasta que algo lo despierta). */
static volatile int cerrar_en_vez, cerrar_pendiente;
int caja_tomar_cerrar(void) { return __atomic_exchange_n(&cerrar_pendiente, 0, __ATOMIC_ACQ_REL); }
static void manda_ctrl_c(uint64_t us)
{
    if (us) {
        sc(SC_DORMIR, (long)us, 0, 0, 0, 0);
        if (cerrar_en_vez) __atomic_store_n(&cerrar_pendiente, 1, __ATOMIC_RELEASE);
        else {
            __atomic_store_n(&ctrl_c_pendiente, 1, __ATOMIC_RELEASE);
            __atomic_store_n(&m68k.INTF.ARM, 1, __ATOMIC_RELEASE);
        }
    }
    for (int i = 0; i < 100 && ctrl_c_corriendo; i++) sc(SC_DORMIR, 100000, 0, 0, 0, 0);
    if (ctrl_c_corriendo) __atomic_store_n(&m68k.INTF.ARM_err, 1, __ATOMIC_RELEASE);
    sc(SC_SALIR, 0, 0, 0, 0, 0);
    for (;;) {}
}

/* --- Principal --- */

int caja_main(void)
{
    di("JIT de Emu68 en un proceso de usuario big-endian (H4, primer paso)\n");
    if (proyecta(M68K_RAM, M68K_RAM_LEN, P_ESCRIBIR) <= 0 || proyecta(MONTON, MONTON_LEN, P_ESCRIBIR) <= 0
        || proyecta(MONTON_RUST, MONTON_RUST_LEN, P_ESCRIBIR) <= 0) {
        di("sin memoria para el 68k\n");
        return 1;
    }
    /* El código traducido: el mismo objeto para escribir y, en otra dirección, para ejecutar (W^X). */
    long jit = sc(SC_MEMORIA_CREAR, JIT_LEN, 0, 0, 0, 0);
    if (jit <= 0 || sc(SC_MEMORIA_PROYECTAR, jit, JIT_RW, P_ESCRIBIR, 0, 0) != 0
        || sc(SC_MEMORIA_PROYECTAR, jit, JIT_RX, P_EJECUTAR, 0, 0) != 0) {
        di("sin memoria para el código traducido\n");
        return 2;
    }
    /* Que la misma dirección no se pueda escribir y ejecutar: el kernel lo rechaza. */
    if (sc(SC_MEMORIA_PROYECTAR, jit, JIT_RX + JIT_LEN, P_ESCRIBIR | P_EJECUTAR, 0, 0) >= 0) {
        di("MAL: el kernel deja escribir y ejecutar a la vez\n");
        return 3;
    }
    tlsf = tlsf_init_with_memory((void *)MONTON, MONTON_LEN);
    jit_tlsf = tlsf_init_with_memory((void *)JIT_RW, JIT_LEN);
    tlsf_rust = tlsf_init_with_memory((void *)MONTON_RUST, MONTON_RUST_LEN);

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
    prepara_vectores();
    int fallos = 0;

    /* La página 0 (S3): un objeto de una página, escrito por una dirección por encima de 4 GB y proyectado de solo
       lectura en la dirección 0, con el permiso de página 0 (la llave 1, de `inicio`). Exec escribe ExecBase en la
       dirección 4 por la proyección de escribir; después se proyecta en 0. */
    long p0 = sc(SC_MEMORIA_CREAR, 4096, 0, 0, 0, 0);
    long r0 = p0 > 0 ? sc(SC_MEMORIA_PROYECTAR, p0, (long)PAGINA0_RW, P_ESCRIBIR, 0, 0) : -1;
    if (r0 != 0) {
        di("sin memoria para la página 0\n");
        return 4;
    }
    uint32_t execbase = caja_rust_arrancar(M68K_RAM, M68K_RAM_LEN, PAGINA0_RW, SISTEMA, FIN_SISTEMA, CHIP);
    if (!execbase) {
        di("las bibliotecas nativas no arrancan\n");
        return 5;
    }
    uint32_t super = caja_alloc_mem(SUPER_LEN, 0);
    if (!super) {
        di("sin memoria para la pila de supervisor\n");
        return 6;
    }
    super_tope = super + SUPER_LEN;
    pon32(SUPER_BAJO, super + 512);
    caja_poner_trampa((uint32_t)TRAMPA_DEFECTO);
    r0 = sc(SC_PAGINA_CERO, LLAVE_PAGINA0, p0, 0, 0, 0);
    /* La caja la lee (EL0 puede leerla); otra vez, o en el resto de los primeros 64 KB, el kernel no deja. */
    int pagina0 = r0 == 0 && lee32(4) == execbase && sc(SC_PAGINA_CERO, LLAVE_PAGINA0, p0, 0, 0, 0) < 0
        && sc(SC_MEMORIA_PROYECTAR, p0, 0x1000, P_LEER, 0, 0) < 0 && sc(SC_MEMORIA_PROYECTAR, p0, 0, P_LEER, 0, 0) < 0;
    di("página 0: ExecBase en la dirección 4 ($%x), solo de lectura; otra vez o en los primeros 64 KB, rechazada: %s\n",
       pagina0 ? lee32(4) : 0, pagina0 ? "bien" : "MAL");
    if (!pagina0) { di("página 0: la llamada 49 dio %d\n", (int)r0); fallos++; }

    /* 1. La suma de 1 a 100.000 y su texto, por la dos.library nativa. */
    uint64_t t0 = ahora_us();
    llamadas_nativas = 0;
    int v = ejecuta("hola", prog_hola, prog_hola_fin, 0x00110000);
    uint64_t t1 = ahora_us();
    uint32_t esperado = 0;
    for (uint32_t i = 1; i <= 100000; i++) esperado += i;
    int bien = v < 0 && m68k.D[0].u32 == esperado && llamadas_nativas == 4;
    di("el 68k dio %u (esperado %u) con %d llamadas nativas, %u unidades traducidas, en %u ms: %s\n",
       m68k.D[0].u32, esperado, llamadas_nativas, m68k.JIT_UNIT_COUNT, (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
    if (!bien) { M68K_PrintContext(&m68k); fallos++; }

    /* 2. Hostil: una función nativa que no existe (se rechaza y el programa sigue) y una línea A que no es una
       llamada (excepción 10 del 68k: el programa acaba, la caja sigue). */
    llamadas_nativas = 0;
    v = ejecuta("lineaa", prog_lineaa, prog_lineaa_fin, 0x00120000);
    bien = v == 10 && rechazadas == 1 && llamadas_nativas == 1;
    di("línea A desconocida: excepción %d del 68k, %d rechazada, %d llamada buena: %s\n", v, rechazadas,
       llamadas_nativas, bien ? "bien" : "MAL");
    if (!bien) { M68K_PrintContext(&m68k); fallos++; }

    /* 3. Hostil: un bucle sin fin. El vigilante, en otro hilo, lo corta a los 300 ms. */
    static uint8_t *pila_vigia;
    pila_vigia = (uint8_t *)(PILAS + PILA_LEN + 0x10000);
    if (proyecta((uintptr_t)pila_vigia, PILA_VIGIA_LEN, P_ESCRIBIR) <= 0) {
        di("sin memoria para el vigilante\n");
        return 4;
    }
    long hilo = sc(SC_HILO_CREAR, (long)vigila, (long)(pila_vigia + PILA_VIGIA_LEN), 300000, 0, 0);
    if (hilo < 0) {
        di("no se pudo crear el vigilante: %d\n", (int)hilo);
        return 5;
    }
    t0 = ahora_us();
    v = ejecuta("bucle", prog_bucle, prog_bucle_fin, 0x00130000);
    t1 = ahora_us();
    sc(SC_HILO_UNIR, hilo, 0, 0, 0, 0);
    bien = v == 31 && m68k.D[0].u32 > 1000;
    di("bucle sin fin: cortado por el vigilante a los %u ms tras %u vueltas (vector %d): %s\n",
       (uint32_t)((t1 - t0) / 1000), m68k.D[0].u32, v, bien ? "bien" : "MAL");
    if (!bien) { M68K_PrintContext(&m68k); fallos++; }

    if (pagina0) {
        /* 4. Un ejecutable hunk de AmigaOS (m68k-amigaos-gcc): cargado con sus reubicaciones en memoria de AllocMem,
           toma ExecBase de la dirección 4, abre la dos.library con OpenLibrary y escribe. */
        uint32_t d0 = 0, sobras = 0;
        int llamadas = 0;
        size_t largo = (size_t)(prog_saludo_fin - prog_saludo);
        uint32_t sistema = caja_bloques(0);
        t0 = ahora_us();
        v = ejecuta_hunk("saludo.exe", prog_saludo, largo, execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && llamadas == 7 && lee16(caja_dosbase() + 32) == 0 && sobras == 0
            && caja_bloques(0) == sistema && caja_avisos() == 0;
        di("saludo.exe acabó con %d (vector %d), %d llamadas nativas, en %u ms, sin memoria perdida: %s\n", (int)d0, v,
           llamadas, (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }

        /* 5. Hostil: el mismo ejecutable cortado por cada byte y estropeado al azar. Ningún caso rompe la caja ni
           escribe fuera de lo que reserva; los que fallan devuelven toda su memoria (los cortes y 20.000 mutaciones con el
           reparto rápido; 1.000 más con AllocMem de exec, que no pierde ningún bloque). */
        int errores = 0, cargan = 0, perdidas = 0;
        struct hunk_carga c;
        int e;
        uint32_t n = largo < sizeof(hunk_copia) ? (uint32_t)largo : 0;
        const uint32_t rapido_largo = 256 << 10;
        rapido_inicio = caja_alloc_mem(rapido_largo, 0);
        rapido_fin = rapido_inicio + rapido_largo;
        t0 = ahora_us();
        for (uint32_t corte = 0; corte < n && rapido_inicio; corte++) {
            hunks_vivos = 0;
            rapido_libre = rapido_inicio;
            e = hunk_cargar(prog_saludo, corte, &rapido_ops, hunks_segs, MAX_HUNKS, &c);
            if (e == HUNK_BIEN) { cargan++; hunk_soltar(&rapido_ops, &c); } else errores++;
            perdidas += hunks_vivos != 0;
        }
        const int mutaciones = 20000, con_exec = 1000;
        for (int k = 0; k < mutaciones + con_exec && n && rapido_inicio; k++) {
            for (uint32_t i = 0; i < n; i++) hunk_copia[i] = prog_saludo[i];
            for (int j = 1 + azar() % 4; j > 0; j--) {
                uint32_t p = (azar() % n) & ~3u;
                static const uint32_t raros[] = { 0, 1, 0x7FFFFFFF, 0xFFFFFFFF, 0x3FFFFFFF, 0x40000000, HUNK_END, HUNK_CODE,
                                                  HUNK_RELOC32 };
                uint32_t x = azar() % 2 ? azar() : raros[azar() % (sizeof(raros) / 4)];
                if (p + 4 <= n) { hunk_copia[p] = x >> 24; hunk_copia[p + 1] = x >> 16; hunk_copia[p + 2] = x >> 8; hunk_copia[p + 3] = x; }
            }
            const struct hunk_ops *ops = k < mutaciones ? &rapido_ops : &hunks_ops;
            hunks_vivos = 0;
            rapido_libre = rapido_inicio;
            e = hunk_cargar(hunk_copia, n, ops, hunks_segs, MAX_HUNKS, &c);
            if (e == HUNK_BIEN) { cargan++; hunk_soltar(ops, &c); } else errores++;
            perdidas += hunks_vivos != 0;
        }
        t1 = ahora_us();
        if (rapido_inicio) caja_free_mem(rapido_inicio, rapido_largo);
        bien = n && rapido_inicio && perdidas == 0 && errores > (int)n && caja_bloques(0) == sistema;
        di("hunk hostil: %u cortes y %d mutaciones de saludo.exe (%d con AllocMem): %d con error, %d cargan, %d pierden "
           "memoria, en %u ms: %s\n", n, mutaciones + con_exec, con_exec, errores, cargan, perdidas,
           (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
        if (!bien) fallos++;

        /* 6. Una ventana Amiga en el escritorio: ventana.exe la abre con OpenWindowTags, lee un fichero por dos, dibuja
           y espera a IDCMP_CLOSEWINDOW (el guion de la prueba pulsa su gadget de cerrar) o 40 s. */
        t0 = ahora_us();
        v = ejecuta_hunk("ventana.exe", prog_ventana, (size_t)(prog_ventana_fin - prog_ventana), execbase, &d0,
                         &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && (d0 == 0 || d0 == 5) && sobras == 0 && caja_bloques(0) == sistema;
        di("ventana.exe acabó con %d (%s), %d llamadas nativas, en %u ms, %u avisos, sin memoria perdida: %s\n", (int)d0,
           d0 == 0 ? "cerrada con su gadget" : d0 == 5 ? "cerrada sola por tiempo" : "?", llamadas,
           (uint32_t)((t1 - t0) / 1000), caja_avisos(), bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }

        /* 7. Hostil: punteros malos a OpenWindow, OpenWindowTagList, las de dibujo y las de dos; dibujar fuera de la
           ventana. La caja sigue, y el compositor también. */
        v = ejecuta_hunk("hostil.exe", prog_hostil, (size_t)(prog_hostil_fin - prog_hostil), execbase, &d0, &llamadas,
                         &sobras);
        int abiertas = caja_compositor_sigue();
        bien = v == -1 && d0 == 0 && caja_avisos() >= 10 && abiertas == 0 && caja_bloques(0) == sistema;
        di("hostil.exe acabó con %d tras %d llamadas nativas: %u avisos del Enforcer, %u bloques sin soltar (los soltó la "
           "caja), el compositor contesta con %d ventanas de la caja abiertas: %s\n", (int)d0, llamadas, caja_avisos(),
           sobras, abiertas, bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }

        /* S28: los virus de laboratorio. Después, los programas de Dash sirven además de prueba de que la caja sigue
           bien (y de que los parches de los virus ya no están: pasan por OpenLibrary y Open). */
        fallos += prueba_malware(execbase, sistema);
        fallos += prueba_actualiza(execbase, sistema);

        /* 8. Programas de Dash, el lenguaje de FreeMEMLang (compilados con `dash` al hacer la caja; ver el Makefile):
           lo que escriben sale con "  [caja-amiga] dash: " delante. Tienen que acabar con 0, sin avisos del Enforcer y
           sin dejar memoria. */
        static const struct { const char *nombre; const uint8_t *desde, *hasta; } dash[] = {
            { "texto.exe", prog_dash_texto, prog_dash_texto_fin },
            { "texto_gcc.exe", prog_dash_texto_gcc, prog_dash_texto_gcc_fin },
            { "test_interp_comillas", prog_dash_interp, prog_dash_interp_fin },
            { "test_lista_listas_literal", prog_dash_listas, prog_dash_listas_fin },
            { "hola.exe", prog_dash_hola, prog_dash_hola_fin },
            { "ventana.exe", prog_dash_ventana, prog_dash_ventana_fin },
        };
        static const char prefijo[] = "  [caja-amiga] dash: ";
        for (unsigned i = 0; i < sizeof(dash) / sizeof(dash[0]); i++) {
            size_t n = (size_t)(dash[i].hasta - dash[i].desde);
            if (n == 0) {
                di("Dash %s: no está (la caja se compiló sin ~/Projects/FreeMEMLang/dash, o no compiló)\n", dash[i].nombre);
                continue;
            }
            caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
            t0 = ahora_us();
            v = ejecuta_hunk(dash[i].nombre, dash[i].desde, n, execbase, &d0, &llamadas, &sobras);
            t1 = ahora_us();
            caja_consola_prefijo(NULL, 0);
            bien = v == -1 && d0 == 0 && caja_avisos() == 0 && sobras == 0 && caja_bloques(0) == sistema;
            di("Dash %s acabó con %d (vector %d), %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
               dash[i].nombre, (int)d0, v, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras,
               bien ? "bien" : "MAL");
            if (!bien) { M68K_PrintContext(&m68k); fallos++; }
        }
        /* 8b. BOOPSI y ReAction (boopsi.c), antes que GadTools (que en el modo de las capturas espera 90 s y la sesión se
           bloquea):  una clase de usuario con su despachador en 68k (que vuelve a intuition
           con DoSuperMethodA), interconexión con propgclass, strgclass, icclass y modelclass, y una ventana de
           window.class con su layout. Se comprueba solo y acaba con 0. */
        static const char *const boopsi_nombres[] = { BOOPSI_NOMBRE, "boopsi-orig-20" };
        for (unsigned i = 0; i < (CAJA_INTERACTIVA ? 2u : 1u); i++) {
            t0 = ahora_us();
            v = ejecuta_hunk(boopsi_nombres[i], prog_boopsi, (size_t)(prog_boopsi_fin - prog_boopsi), execbase, &d0,
                             &llamadas, &sobras);
            t1 = ahora_us();
            uint32_t hechas = 0, no = 0, exc = 0;
            caja_cuentas_anidadas(&hechas, &no, &exc);
            bien = v == -1 && d0 == 0 && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema && no == 0;
            di("boopsi.exe acabó con %d, %d llamadas nativas, %u despachadores 68k, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
               (int)d0, llamadas, hechas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
            if (!bien) { M68K_PrintContext(&m68k); fallos++; }
        }

        /* 8b2. ramlib (ramlib.c): abre bibliotecas de disco (Caja:Libs, su LIBS:), las usa y las cierra, y lo malo (un
           RomTag falso, una que miente con su versión, un fichero que no es ejecutable, caminos raros, layers.library).
           Al acabar no queda nada suyo en memoria: los bloques del sistema, como antes. */
        t0 = ahora_us();
        v = ejecuta_hunk("ramlib", prog_ramlib, (size_t)(prog_ramlib_fin - prog_ramlib), execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema;
        di("ramlib.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar, %u bloques del sistema de más: %s\n",
           (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, caja_bloques(0) - sistema, bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }

        /* 8c. GadTools (gadtools.c): una ventana con un gadget de cada tipo y menús. Comprueba sola sus atributos y
           espera clics: el guion pulsa varios gadgets y elige Proyecto > Salir con el botón derecho (0), si su ventana
           se ve entera; si no, se cierra sola (5). En la prueba de siempre espera 5 s: más tiempo abierta tapaba los
           pasos del guion de los demás (Ajustes, el centro de control). Con CAJA_INTERACTIVA (las capturas), 90 s. */
        t0 = ahora_us();
        v = ejecuta_hunk(GT_NOMBRE, prog_gadtools, (size_t)(prog_gadtools_fin - prog_gadtools), execbase, &d0,
                         &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && (d0 == 0 || d0 == 5) && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema;
        di("gadtools.exe acabó con %d (%s), %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
           (int)d0, d0 == 0 ? "cerrada por el programa" : d0 == 5 ? "cerrada sola por tiempo" : "?", llamadas,
           (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
        if (!bien) { M68K_PrintContext(&m68k); fallos++; }

        /* 9. Llamadas anidadas: 68k desde una función nativa (anidado.c; el modo, en la letra del nombre). a: Open de una
           biblioteca 68k, sin fin (el tope), con la pila de vuelta más abajo, tc_ExceptCode, CallHookPkt y un
           tc_TrapCode que recupera: acaba con 0. b: vuelve con la pila más arriba: cortado (-2). c: instrucción
           ilegal dentro: acaba por el vector 4 dentro de la anidada. d: excepción dentro de la excepción, sin fin:
           vector 4 al acabarse la pila de supervisor. */
        static const struct { const char *nombre; int vector; } anid[] = {
            { "anidado-a", -1 }, { "anidado-b", -2 }, { "anidado-c", 4 }, { "anidado-d", 4 },
        };
        for (unsigned i = 0; i < sizeof(anid) / sizeof(anid[0]); i++) {
            v = ejecuta_hunk(anid[i].nombre, prog_anidado, (size_t)(prog_anidado_fin - prog_anidado), execbase, &d0,
                             &llamadas, &sobras);
            uint32_t hechas = 0, no = 0, exc = 0;
            caja_cuentas_anidadas(&hechas, &no, &exc);
            bien = v == anid[i].vector && (v != -1 || d0 == 0) && caja_bloques(0) == sistema && anidado == 0;
            di("%s acabó por el vector %d (D0 %d), %u llamadas anidadas (%u no), %u excepciones de tarea, %u avisos: %s\n",
               anid[i].nombre, v, (int)d0, hechas, no, exc, caja_avisos(), bien ? "bien" : "MAL");
            if (!bien) { M68K_PrintContext(&m68k); fallos++; }
        }

        /* 10. Ctrl-C y las excepciones del procesador en programas de Dash: el bucle sin fin de caja_ctrl_c.dash (por los
           dos caminos) recibe Ctrl-C de otro hilo a los 700 ms y acaba con "***Break" y 5; test_ctrl_c_for se lo
           manda él (SetSignal) y acaba igual; test_trap_illegal y test_trap_divzero acaban con el mensaje del
           tc_TrapCode de Dash y 20. Y el ejemplo de GadTools de FreeMEMLang (examples/test_gadtools.dash): el guion
           pulsa su "Cancel" (0) si llega; si no, Ctrl-C a los 6 s (5): su ventana no puede tapar mucho rato las de
           los demás pasos de la prueba. */
        static const struct { const char *nombre; const uint8_t *desde, *hasta; int ctrl_c; uint32_t d0, otro; } dash2[] = {
            { "ctrl_c.exe", prog_dash_ctrl_c, prog_dash_ctrl_c_fin, 700, 5, 5 },
            { "ctrl_c_gcc.exe", prog_dash_ctrl_c_gcc, prog_dash_ctrl_c_gcc_fin, 700, 5, 5 },
            { "test_ctrl_c_for", prog_dash_ctrl_c_for, prog_dash_ctrl_c_for_fin, 0, 5, 5 },
            { "test_trap_illegal", prog_dash_trap_ilegal, prog_dash_trap_ilegal_fin, 0, 20, 20 },
            { "test_trap_divzero", prog_dash_trap_divcero, prog_dash_trap_divcero_fin, 0, 20, 20 },
            { "test_gadtools", prog_dash_test_gadtools, prog_dash_test_gadtools_fin, 6000, 0, 5 },
            /* ReAction (window.class y layout.gadget nativos): la prueba de la documentación acaba sola; el ejemplo
               espera a que pulsen "OK" o "Cancel" o cierren su ventana: a los 4 s (en el modo de las capturas, 30 s)
               se cierra como con el botón de la barra (un negativo: cerrar en vez de Ctrl-C) y acaba con 0. */
            { "test_doc_reaction", prog_dash_test_doc_reaction, prog_dash_test_doc_reaction_fin, 0, 0, 0 },
            { "test_reaction", prog_dash_test_reaction, prog_dash_test_reaction_fin, CAJA_INTERACTIVA ? -30000 : -4000, 0, 0 },
        };
        for (unsigned i = 0; i < sizeof(dash2) / sizeof(dash2[0]); i++) {
            size_t n = (size_t)(dash2[i].hasta - dash2[i].desde);
            if (n == 0) {
                di("Dash %s: no está (la caja se compiló sin ~/Projects/FreeMEMLang/dash, o no compiló)\n", dash2[i].nombre);
                continue;
            }
            ctrl_c_corriendo = 1;
            cerrar_en_vez = dash2[i].ctrl_c < 0;
            long h = sc(SC_HILO_CREAR, (long)manda_ctrl_c, (long)(pila_vigia + PILA_VIGIA_LEN),
                        (long)(dash2[i].ctrl_c < 0 ? -dash2[i].ctrl_c : dash2[i].ctrl_c) * 1000, 0, 0);
            caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
            t0 = ahora_us();
            v = ejecuta_hunk(dash2[i].nombre, dash2[i].desde, n, execbase, &d0, &llamadas, &sobras);
            t1 = ahora_us();
            caja_consola_prefijo(NULL, 0);
            ctrl_c_corriendo = 0;
            if (h >= 0) sc(SC_HILO_UNIR, h, 0, 0, 0, 0);
            uint32_t hechas = 0, no = 0, exc = 0;
            caja_cuentas_anidadas(&hechas, &no, &exc);
            bien = v == -1 && (d0 == dash2[i].d0 || d0 == dash2[i].otro) && caja_avisos() == 0 && sobras == 0 && caja_bloques(0) == sistema;
            di("Dash %s acabó con %d (vector %d) en %u ms, %u excepciones de tarea, %u avisos, %u bloques sin soltar: %s\n",
               dash2[i].nombre, (int)d0, v, (uint32_t)((t1 - t0) / 1000), exc, caja_avisos(), sobras, bien ? "bien" : "MAL");
            if (!bien) { M68K_PrintContext(&m68k); fallos++; }
        }
#if CAJA_INTERACTIVA
        /* El error de Dash en un requester: test_trap_illegal sin consola (Output() es 0, como desde el Workbench). El
           guion lo captura (CAPTURA_REQ) y pulsa su botón; si no llega, el requester sigue abierto hasta el final de la
           prueba (la espera, de 600 s en este modo) y sale en la captura final (CAPTURA). */
        if (prog_dash_trap_ilegal_fin > prog_dash_trap_ilegal) {
            di("test_trap_illegal (sin consola): su error, en un requester\n");
            caja_sin_consola(1);
            v = ejecuta_hunk("test_trap_illegal", prog_dash_trap_ilegal,
                             (size_t)(prog_dash_trap_ilegal_fin - prog_dash_trap_ilegal), execbase, &d0, &llamadas, &sobras);
            caja_sin_consola(0);
            di("Dash test_trap_illegal (sin consola) acabó con %d (vector %d), %u avisos\n", (int)d0, v, caja_avisos());
        }
#endif
    }

    if (fallos) return 10 + fallos;

    /* 9. Hostil: el 68k escribe en la dirección 4, en la página 0 de solo lectura. El kernel termina la caja (sin el
       Enforcer, que aún no está: ver la bitácora); si sigue viva, mal. */
    di("el 68k escribe en la dirección 4 (ExecBase), en la página 0 de solo lectura: la caja tiene que morir\n");
    v = ejecuta("mala", prog_mala, prog_mala_fin, 0x00140000);
    di("MAL: la caja sigue viva (vector %d)\n", v);
    return 20;
}
