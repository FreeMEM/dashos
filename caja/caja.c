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
      $7000_0000 -             este ejecutable (ET_EXEC; Emu68 llama a sus funciones de C con direcciones de 32 bits)
      4 GB                     el código traducido, para escribir
      4 GB + 64 GB             el mismo código, para ejecutar (bit 36: lo pone Emu68 en mt_ARMEntryPoint)
      8 GB                     el montón general de Emu68 (TLSF)
      10 GB                    el montón de Rust (TLSF): el estado de exec, dos e intuition y los lienzos de las ventanas
      12 GB                    las pilas nativas (la del JIT y la del vigilante), fuera del alcance del 68k
      14 GB                    la página 0, para escribirla
      16 GB                    la ventana del servicio de archivos
      20 GB                    los búferes de las ventanas (wl_shm), 64 MB por ventana

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

/* La llama entrada.S antes que nada: el tope de una pila por encima de 4 GB, donde el 68k no llega. 0 si no hay. */
uintptr_t caja_pila(void)
{
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
static volatile int llamadas_nativas;
static volatile int rechazadas;
static volatile int cortadas;

static struct M68KState m68k;

void dashos_native_call(uint64_t *regs, uint32_t index)
{
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
}

/* --- Los programas 68k --- */

extern const uint8_t prog_hola[], prog_hola_fin[];
extern const uint8_t prog_lineaa[], prog_lineaa_fin[];
extern const uint8_t prog_bucle[], prog_bucle_fin[];
extern const uint8_t prog_mala[], prog_mala_fin[];
extern const uint8_t prog_saludo[], prog_saludo_fin[];
extern const uint8_t prog_ventana[], prog_ventana_fin[];
extern const uint8_t prog_hostil[], prog_hostil_fin[];

/* Las trampas: el vector i escribe i en MARCA y salta a COMUN, que pone la pila y vuelve a la dirección 0, donde
   MainLoop termina. Así un programa que provoca una excepción del 68k acaba sin tumbar la caja. Al volver de su
   función, cada tarea salta también a COMUN (su dirección de vuelta). */
static void prepara_vectores(void)
{
    for (uint32_t i = 0; i < 256; i++) {
        uintptr_t t = TRAMPAS + 14 * i;
        pon32(VECTORES + 4 * i, (uint32_t)t);
        pon16(t, 0x33fc); pon16(t + 2, (uint16_t)i);                          /* move.w #i,MARCA */
        pon32(t + 4, (uint32_t)MARCA);
        pon16(t + 8, 0x4ef9); pon32(t + 10, (uint32_t)COMUN);                 /* jmp COMUN */
    }
    pon16(COMUN, 0x4ff9); pon32(COMUN + 2, (uint32_t)(PILA_TOPE - 4));      /* lea PILA_TOPE-4,a7 */
    pon16(COMUN + 6, 0x4297);                                                  /* clr.l (a7) */
    pon16(COMUN + 8, 0x4e75);                                                  /* rts */
}

/* Ejecuta el 68k desde `pc` con A7 = `a7`, A6 = `a6`, A0 = `a0` y D0 = `d0` (el resto a 0), en supervisor y sin máscara
   de interrupciones, hasta que vuelve a la dirección 0. Devuelve el vector por el que acabó, o -1 si acabó con su RTS. */
__attribute__((noipa)) static int arranca(uint32_t pc, uint32_t a7, uint32_t a6, uint32_t a0, uint32_t d0)
{
    pon16(MARCA, 0xffff);
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = 0; m68k.A[i].u32 = 0; }
    m68k.A[0].u32 = a0;
    m68k.D[0].u32 = d0;
    for (int fp = 0; fp < 8; fp++) m68k.FP[fp].u64 = 0x7fffffffffffffffULL;
    m68k.ISP.u32 = a7;
    m68k.A[7].u32 = a7;
    m68k.A[6].u32 = a6;
    m68k.VBR = VECTORES;
    m68k.PC = pc;
    m68k.SR = SR_S;
    m68k.CACR = 0x80008000;
    m68k.INT64 = 0;
    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    __asm__ volatile("mov " CTX_POINTER_ASM ", %0" ::"r"(&m68k));
    __asm__ volatile("bl MainLoop" ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10",
                     "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22",
                     "x23", "x24", "x25", "x26", "x27", "x28", "x29", "x30", "v0", "v1", "v2", "v3", "v4",
                     "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16", "v17",
                     "v18", "v27", "v28", "v29", "v30", "v31", "memory", "cc");
    uint16_t marca = lee16(MARCA);
    return marca == 0xffff ? -1 : marca;
}

static size_t largo_de(const char *s) { size_t n = 0; while (s[n]) n++; return n; }

/* Ejecuta código 68k como un proceso de exec llamado `nombre`, con A6 = `a6`, A0 los argumentos y D0 su largo (como
   desde el CLI). Al acabar, la caja suelta lo que el programa dejara (ficheros, ventanas, memoria): en *sobras,
   cuántos bloques de memoria no soltó él. Devuelve el vector por el que acabó, -1 si con su RTS, o -2 si no se pudo
   crear el proceso. */
static int ejecuta_tarea(const char *nombre, uint32_t pc, uint32_t a6, uint32_t *sobras)
{
    static const char args[] = "\n";
    uint32_t args68k = caja_alloc_mem(sizeof(args), 0);
    if (args68k) for (unsigned i = 0; i < sizeof(args); i++) pon8(args68k + i, (uint8_t)args[i]);
    uint32_t sp = 0;
    uint32_t t = caja_tarea_nueva(nombre, largo_de(nombre), 16384, pc, (uint32_t)COMUN, args68k, &sp);
    if (!t) { if (args68k) caja_free_mem(args68k, sizeof(args)); return -2; }
    int v = arranca(pc, sp, a6, args68k, sizeof(args) - 1);
    uint32_t s = caja_tarea_fin(t);
    if (sobras) *sobras = s;
    if (args68k) caja_free_mem(args68k, sizeof(args));
    return v;
}

/* El JIT olvida todo lo traducido: un programa nuevo puede ir donde estaba otro (AllocMem da las mismas direcciones),
   y Emu68 encontraría en su caché lo traducido del de antes. Como el CINV/CPUSH del 68k (M68k_LINEF.c), y además
   suelta las unidades (como cuando se queda sin sitio, M68k_Translator.c). Solo entre programas, con el 68k parado:
   desde una llamada nativa soltaría el código que está corriendo. */
extern struct List LRU;
static void jit_olvidar(void)
{
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

/* Carga un programa plano en `donde` (su dirección de enlace, en la zona PLANOS) y lo ejecuta con A6 = DOSBase. */
static int ejecuta(const char *nombre, const uint8_t *desde, const uint8_t *hasta, uintptr_t donde)
{
    uint8_t *d = (uint8_t *)donde;
    for (const uint8_t *s = desde; s < hasta; ) *d++ = *s++;
    jit_olvidar();
    return ejecuta_tarea(nombre, (uint32_t)donde, caja_dosbase(), NULL);
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
    int v = ejecuta_tarea(nombre, c.entrada, execbase, sobras);
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
    }

    if (fallos) return 10 + fallos;

    /* 8. Hostil: el 68k escribe en la dirección 4, en la página 0 de solo lectura. El kernel termina la caja (sin el
       Enforcer, que aún no está: ver la bitácora); si sigue viva, mal. */
    di("el 68k escribe en la dirección 4 (ExecBase), en la página 0 de solo lectura: la caja tiene que morir\n");
    v = ejecuta("mala", prog_mala, prog_mala_fin, 0x00140000);
    di("MAL: la caja sigue viva (vector %d)\n", v);
    return 20;
}
