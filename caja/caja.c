/*
    DashOS, H4: la caja Amiga, primer paso.

    Un proceso de usuario big-endian con el JIT de Emu68 dentro. Ejecuta programas 68k planos (de
    pruebas68k/, metidos en el ejecutable) y les da una dos.library nativa mínima: cada entrada de su tabla de
    saltos es "$A0FF <número> RTS", y el JIT convierte la línea A en una llamada a dashos_native_call (parche
    0002 de patches/emu68). Desde el segundo paso, también ejecutables hunk de AmigaOS (hunk.c), con ExecBase en
    la dirección 4 (la página 0, de solo lectura, con el permiso de su llave 1) y una exec mínima y provisional
    (exec_minima.c) que abre la dos.library.

    Memoria del proceso (docs/referencia/kernel.md, mapa de direcciones):
      $0000_0000 - $0000_0FFF  la página 0, de solo lectura: ExecBase en la dirección 4 (llamada 45 del kernel)
      $0010_0000 - $001F_FFFF  memoria del 68k (1 MB): vectores, las bibliotecas, los programas y su pila
      $7000_0000 -             este ejecutable (ET_EXEC; Emu68 llama a sus funciones de C con direcciones de 32 bits)
      4 GB                     el código traducido, para escribir
      4 GB + 64 GB             el mismo código, para ejecutar (bit 36: lo pone Emu68 en mt_ARMEntryPoint)
      8 GB                     el montón general de Emu68 (TLSF)
      12 GB                    las pilas nativas (la del JIT y la del vigilante), fuera del alcance del 68k

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
#define SC_PAGINA_CERO       45

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

/* --- La dos.library nativa (lo mínimo para escribir) --- */

#define SALIDA 0x00000001u   /* el único fichero: la consola */

/* volatile: los cambia dashos_native_call, que llama el código traducido desde dentro del asm de `arranca`; sin él, gcc
   (análisis entre funciones) da por hecho que `arranca` no los toca y no los vuelve a leer. */
static volatile int llamadas_nativas;
static volatile int rechazadas;

/* Escribe n bytes de la memoria del 68k en la consola. -1 si no son suyos. */
static long escribe68k(uint32_t a, uint32_t n)
{
    if (!en_ram68k(a, n)) return -1;
    if (largo) vaciar();
    for (uint32_t i = 0; i < n; i++) pon_c(NULL, *(const char *)(uintptr_t)(a + i));
    if (largo) vaciar();
    return n;
}

long cadena68k(uint32_t a)
{
    for (uint32_t n = 0; n < 4096; n++) {
        if (!en_ram68k(a, n + 1)) return -1;
        if (*(const char *)(uintptr_t)(a + n) == 0) return n;
    }
    return -1;
}

static void pon_numero(char **p, char *fin, uint32_t v, int base, int con_signo, int ancho, char relleno)
{
    char t[12];
    int n = 0, negativo = 0;
    if (con_signo && (int32_t)v < 0) { negativo = 1; v = -v; }
    do { t[n++] = "0123456789ABCDEF"[v % base]; v /= base; } while (v);
    if (negativo) t[n++] = '-';
    for (int i = n; i < ancho && *p < fin; i++) *(*p)++ = relleno;
    while (n && *p < fin) *(*p)++ = t[--n];
}

/* VPrintf(formato D1, argumentos D2) como RawDoFmt: %[-][0][ancho][l](d|u|x|X|s|c|%); sin 'l', los números son de 16
   bits. Devuelve los bytes escritos, o -1 si algo cae fuera de la memoria del 68k. */
static long vprintf68k(uint32_t formato, uint32_t argv)
{
    static char buf[1024];
    char *p = buf, *fin = buf + sizeof(buf);
    long n = cadena68k(formato);
    if (n < 0) return -1;
    for (const char *f = (const char *)(uintptr_t)formato; *f && p < fin; f++) {
        if (*f != '%') { *p++ = *f; continue; }
        f++;
        int izquierda = 0, ancho = 0, largo32 = 0;
        char relleno = ' ';
        if (*f == '-') { izquierda = 1; f++; }
        if (*f == '0') { relleno = '0'; f++; }
        while (*f >= '0' && *f <= '9') ancho = ancho * 10 + (*f++ - '0');
        if (*f == 'l') { largo32 = 1; f++; }
        (void)izquierda;
        if (*f == '%') { *p++ = '%'; continue; }
        if (*f == 0) break;
        uint32_t tam = (largo32 || *f == 's') ? 4 : 2;
        if (!en_ram68k(argv, tam)) return -1;
        uint32_t v = tam == 4 ? lee32(argv) : lee16(argv);
        argv += tam;
        switch (*f) {
            case 'd': pon_numero(&p, fin, tam == 4 ? v : (uint32_t)(int32_t)(int16_t)v, 10, 1, ancho, relleno); break;
            case 'u': pon_numero(&p, fin, v, 10, 0, ancho, relleno); break;
            case 'x': case 'X': pon_numero(&p, fin, v, 16, 0, ancho, relleno); break;
            case 'c': *p++ = (char)v; break;
            case 's': {
                long m = cadena68k(v);
                if (m < 0) return -1;
                for (long i = 0; i < m && p < fin; i++) *p++ = *(const char *)(uintptr_t)(v + i);
                break;
            }
            default: break;
        }
    }
    if (largo) vaciar();
    for (char *q = buf; q < p; q++) pon_c(NULL, *q);
    if (largo) vaciar();
    return p - buf;
}

/* Las funciones nativas: el número que va tras $A0FF en la tabla de saltos. */
enum { N_OUTPUT = 1, N_WRITE, N_PUTSTR, N_VPRINTF };

void dashos_native_call(uint64_t *regs, uint32_t index)
{
    llamadas_nativas++;
    switch (index) {
        case N_OUTPUT:  /* Output() */
            pon_r(regs, R_D(0), SALIDA);
            break;
        case N_WRITE:   /* Write(fichero D1, búfer D2, largo D3) */
            pon_r(regs, R_D(0), lee_r(regs, R_D(1)) == SALIDA
                ? (uint32_t)escribe68k(lee_r(regs, R_D(2)), lee_r(regs, R_D(3))) : 0xffffffffu);
            break;
        case N_PUTSTR: { /* PutStr(cadena D1): 0 si bien */
            long n = cadena68k(lee_r(regs, R_D(1)));
            pon_r(regs, R_D(0), n >= 0 && escribe68k(lee_r(regs, R_D(1)), n) >= 0 ? 0 : 0xffffffffu);
            break;
        }
        case N_VPRINTF: /* VPrintf(formato D1, argumentos D2) */
            pon_r(regs, R_D(0), (uint32_t)vprintf68k(lee_r(regs, R_D(1)), lee_r(regs, R_D(2))));
            break;
        default:
            if (index >= N_EXEC && exec_minima_llamada(regs, index)) break;
            /* Un número que no es de nadie: se rechaza, como una función que falla, y el programa sigue. */
            rechazadas++;
            llamadas_nativas--;
            di("función nativa %u: no existe; rechazada (D0 = -1)\n", index);
            pon_r(regs, R_D(0), 0xffffffffu);
    }
}

void pon_biblioteca(uintptr_t base, const char *nombre, uintptr_t nombre_en, uint16_t neg, uint16_t pos, uint16_t version)
{
    uintptr_t n = nombre_en;
    do pon8(n++, *nombre); while (*nombre++);
    pon8(base + 8, 9);                    /* ln_Type = NT_LIBRARY */
    pon32(base + 10, (uint32_t)nombre_en);/* ln_Name */
    pon16(base + 16, neg);                /* lib_NegSize */
    pon16(base + 18, pos);                /* lib_PosSize */
    pon16(base + 20, version);            /* lib_Version */
}

/* La tabla de saltos: cada entrada, "$A0FF <número> RTS", en DOS_BASE - LVO. */
static const struct { uint16_t lvo, numero; } dos_tabla[] = {
    { 48, N_WRITE }, { 60, N_OUTPUT }, { 948, N_PUTSTR }, { 954, N_VPRINTF },
};

/* --- Los programas 68k --- */

extern const uint8_t prog_hola[], prog_hola_fin[];
extern const uint8_t prog_lineaa[], prog_lineaa_fin[];
extern const uint8_t prog_bucle[], prog_bucle_fin[];
extern const uint8_t prog_mala[], prog_mala_fin[];
extern const uint8_t prog_saludo[], prog_saludo_fin[];

static struct M68KState m68k;

/* Las trampas: el vector i escribe i en MARCA y salta a COMUN, que pone la pila y vuelve a la dirección 0, donde
   MainLoop termina. Así un programa que provoca una excepción del 68k acaba sin tumbar la caja. */
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
    for (size_t i = 0; i < sizeof(dos_tabla) / sizeof(dos_tabla[0]); i++) pon_salto(DOS_BASE, dos_tabla[i].lvo, dos_tabla[i].numero);
    /* La cabecera de la dos.library (para OpenLibrary y quien mire su versión): 40, la de Kickstart 3.1. */
    for (uintptr_t a = DOS_BASE; a < DOS_BASE + 0x40; a++) pon8(a, 0);
    pon_biblioteca(DOS_BASE, "dos.library", DOS_BASE + 0x40, 960, 0x40, 40);
}

/* Ejecuta el 68k desde `pc` con A6 = `a6`, A0 = `a0` y D0 = `d0` (el resto a 0), en supervisor y sin máscara de interrupciones, hasta que vuelve a la
   dirección 0. Devuelve el vector por el que acabó, o -1 si acabó con su RTS. */
__attribute__((noipa)) static int arranca(uint32_t pc, uint32_t a6, uint32_t a0, uint32_t d0)
{
    pon16(MARCA, 0xffff);
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = 0; m68k.A[i].u32 = 0; }
    m68k.A[0].u32 = a0;
    m68k.D[0].u32 = d0;
    for (int fp = 0; fp < 8; fp++) m68k.FP[fp].u64 = 0x7fffffffffffffffULL;
    pon32(PILA_TOPE - 4, 0);                /* el RTS final va a la dirección 0: MainLoop vuelve */
    m68k.ISP.u32 = PILA_TOPE - 4;
    m68k.A[7].u32 = PILA_TOPE - 4;
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

/* Carga un programa plano en `donde` y lo ejecuta con A6 = DOSBase. */
static int ejecuta(const uint8_t *desde, const uint8_t *hasta, uintptr_t donde)
{
    uint8_t *d = (uint8_t *)donde;
    for (const uint8_t *s = desde; s < hasta; ) *d++ = *s++;
    return arranca((uint32_t)donde, DOS_BASE, 0, 0);
}

/* --- Ejecutables hunk (hunk.c) --- */

/* El reparto de memoria para los hunks, provisional hasta AllocMem de la exec de verdad: avanza, y retrocede si se
   devuelve el último bloque; `hunks_vacia` lo deja todo libre. */
static uint32_t hunks_libre = HUNKS;
static int hunks_vivos;

static uint32_t hunks_reservar(void *ctx, uint32_t bytes, uint32_t atributos)
{
    (void)ctx; (void)atributos;   /* CHIP y FAST: aquí toda la memoria es igual */
    if (bytes > HUNKS_FIN - hunks_libre) return 0;
    uint32_t d = hunks_libre;
    hunks_libre += (bytes + 7) & ~7u;
    hunks_vivos++;
    return d;
}

static void hunks_liberar(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    hunks_vivos--;
    if (d + ((bytes + 7) & ~7u) == hunks_libre) hunks_libre = d;
}

static uint8_t *hunks_ver(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    return en_ram68k(d, bytes) ? (uint8_t *)(uintptr_t)d : NULL;
}

static void hunks_vacia(void) { hunks_libre = HUNKS; hunks_vivos = 0; }

static const struct hunk_ops hunks_ops = { NULL, hunks_reservar, hunks_liberar, hunks_ver };
#define MAX_HUNKS 256
static struct hunk_seg hunks_segs[MAX_HUNKS];

/* Una copia de un ejecutable para estropearla (las pruebas hostiles). */
static uint8_t hunk_copia[16384];
static uint64_t semilla = 0x9E3779B97F4A7C15ULL;
static uint32_t azar(void) { semilla ^= semilla << 13; semilla ^= semilla >> 7; semilla ^= semilla << 17; return (uint32_t)semilla; }

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
    if (proyecta(M68K_RAM, M68K_RAM_LEN, P_ESCRIBIR) <= 0 || proyecta(MONTON, MONTON_LEN, P_ESCRIBIR) <= 0) {
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
       lectura en la dirección 0, con el permiso de página 0 (la llave 1, de `inicio`). En la dirección 4, ExecBase. */
    uint32_t execbase = exec_minima_preparar();
    long p0 = sc(SC_MEMORIA_CREAR, 4096, 0, 0, 0, 0);
    long r0 = p0 > 0 ? sc(SC_MEMORIA_PROYECTAR, p0, (long)PAGINA0_RW, P_ESCRIBIR, 0, 0) : -1;
    if (r0 == 0) {
        pon32(PAGINA0_RW + 4, execbase);
        r0 = sc(SC_PAGINA_CERO, LLAVE_PAGINA0, p0, 0, 0, 0);
    }
    /* La caja la lee (EL0 puede leerla); otra vez, o en el resto de los primeros 64 KB, el kernel no deja. */
    int pagina0 = r0 == 0 && lee32(4) == execbase && sc(SC_PAGINA_CERO, LLAVE_PAGINA0, p0, 0, 0, 0) < 0
        && sc(SC_MEMORIA_PROYECTAR, p0, 0x1000, P_LEER, 0, 0) < 0 && sc(SC_MEMORIA_PROYECTAR, p0, 0, P_LEER, 0, 0) < 0;
    di("página 0: ExecBase en la dirección 4 ($%x), solo de lectura; otra vez o en los primeros 64 KB, rechazada: %s\n",
       pagina0 ? lee32(4) : 0, pagina0 ? "bien" : "MAL");
    if (!pagina0) { di("página 0: la llamada 45 dio %d\n", (int)r0); fallos++; }

    /* 1. La suma de 1 a 100.000 y su texto, por la dos.library nativa. */
    uint64_t t0 = ahora_us();
    int v = ejecuta(prog_hola, prog_hola_fin, 0x00110000);
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
    v = ejecuta(prog_lineaa, prog_lineaa_fin, 0x00120000);
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
    v = ejecuta(prog_bucle, prog_bucle_fin, 0x00130000);
    t1 = ahora_us();
    sc(SC_HILO_UNIR, hilo, 0, 0, 0, 0);
    bien = v == 31 && m68k.D[0].u32 > 1000;
    di("bucle sin fin: cortado por el vigilante a los %u ms tras %u vueltas (vector %d): %s\n",
       (uint32_t)((t1 - t0) / 1000), m68k.D[0].u32, v, bien ? "bien" : "MAL");
    if (!bien) { M68K_PrintContext(&m68k); fallos++; }

    /* 4. Un ejecutable hunk de AmigaOS (m68k-amigaos-gcc): cargado con sus reubicaciones, toma ExecBase de la
       dirección 4, abre la dos.library con OpenLibrary y escribe. Entra como desde el CLI: A0 los argumentos, D0 su
       largo, A6 = ExecBase. */
    if (pagina0) {
        struct hunk_carga c;
        size_t largo = (size_t)(prog_saludo_fin - prog_saludo);
        t0 = ahora_us();
        int e = hunk_cargar(prog_saludo, largo, &hunks_ops, hunks_segs, MAX_HUNKS, &c);
        if (e != HUNK_BIEN || hunks_segs[0].tipo != HUNK_CODE) {
            di("saludo.exe no carga: %s\n", hunk_error(e));
            fallos++;
        } else {
            di("saludo.exe: %u hunks (código %u, datos %u, BSS %u bytes) en $%x, %u reubicaciones\n", c.cuantos,
               hunks_segs[0].bytes, c.cuantos > 1 ? hunks_segs[1].bytes : 0, c.cuantos > 2 ? hunks_segs[2].bytes : 0,
               c.entrada, c.relocs);
            llamadas_nativas = 0;
            static const char args[] = "\n";
            uint32_t args68k = HUNKS_FIN - 16;
            for (unsigned i = 0; i < sizeof(args); i++) pon8(args68k + i, (uint8_t)args[i]);
            v = arranca(c.entrada, execbase, args68k, sizeof(args) - 1);
            t1 = ahora_us();
            bien = v < 0 && m68k.D[0].u32 == 0 && llamadas_nativas == 7 && lee16(DOS_BASE + 32) == 0;
            di("saludo.exe acabó con %d (vector %d), %d llamadas nativas, en %u ms: %s\n", (int)m68k.D[0].u32, v,
               llamadas_nativas, (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
            if (!bien) { M68K_PrintContext(&m68k); fallos++; }
            hunk_soltar(&hunks_ops, &c);
        }

        /* 5. Hostil: el mismo ejecutable cortado por cada byte y estropeado al azar. Ningún caso rompe la caja ni
           escribe fuera de lo que reserva; los que fallan devuelven toda su memoria. */
        int errores = 0, cargan = 0, perdidas = 0;
        uint32_t n = largo < sizeof(hunk_copia) ? (uint32_t)largo : 0;
        for (uint32_t corte = 0; corte < n; corte++) {
            hunks_vacia();
            e = hunk_cargar(prog_saludo, corte, &hunks_ops, hunks_segs, MAX_HUNKS, &c);
            if (e == HUNK_BIEN) { cargan++; hunk_soltar(&hunks_ops, &c); } else errores++;
            perdidas += hunks_vivos != 0;
        }
        const int mutaciones = 20000;
        for (int k = 0; k < mutaciones && n; k++) {
            for (uint32_t i = 0; i < n; i++) hunk_copia[i] = prog_saludo[i];
            for (int j = 1 + azar() % 4; j > 0; j--) {
                uint32_t p = (azar() % n) & ~3u;
                static const uint32_t raros[] = { 0, 1, 0x7FFFFFFF, 0xFFFFFFFF, 0x3FFFFFFF, 0x40000000, HUNK_END, HUNK_CODE,
                                                  HUNK_RELOC32 };
                uint32_t x = azar() % 2 ? azar() : raros[azar() % (sizeof(raros) / 4)];
                if (p + 4 <= n) { hunk_copia[p] = x >> 24; hunk_copia[p + 1] = x >> 16; hunk_copia[p + 2] = x >> 8; hunk_copia[p + 3] = x; }
            }
            hunks_vacia();
            e = hunk_cargar(hunk_copia, n, &hunks_ops, hunks_segs, MAX_HUNKS, &c);
            if (e == HUNK_BIEN) { cargan++; hunk_soltar(&hunks_ops, &c); } else errores++;
            perdidas += hunks_vivos != 0;
        }
        hunks_vacia();
        bien = n && perdidas == 0 && errores > (int)n;
        di("hunk hostil: %u cortes y %d mutaciones de saludo.exe: %d con error, %d cargan, %d pierden memoria: %s\n", n,
           mutaciones, errores, cargan, perdidas, bien ? "bien" : "MAL");
        if (!bien) fallos++;
    }

    if (fallos) return 10 + fallos;

    /* 6. Hostil: el 68k escribe en la dirección 4, en la página 0 de solo lectura. El kernel termina la caja (sin el
       Enforcer, que aún no está: ver la bitácora); si sigue viva, mal. */
    di("el 68k escribe en la dirección 4 (ExecBase), en la página 0 de solo lectura: la caja tiene que morir\n");
    v = ejecuta(prog_mala, prog_mala_fin, 0x00140000);
    di("MAL: la caja sigue viva (vector %d)\n", v);
    return 20;
}
