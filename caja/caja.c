/*
    DashOS, H4: la caja Amiga, primer paso.

    Un proceso de usuario big-endian con el JIT de Emu68 dentro. Ejecuta programas 68k planos (de
    pruebas68k/, metidos en el ejecutable) y les da una dos.library nativa mínima: cada entrada de su tabla de
    saltos es "$A0FF <número> RTS", y el JIT convierte la línea A en una llamada a dashos_native_call (parche
    0002 de patches/emu68).

    Memoria del proceso (docs/referencia/kernel.md, mapa de direcciones):
      $0010_0000 - $001F_FFFF  memoria del 68k (1 MB): vectores, la biblioteca, los programas y su pila
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

#define P_LEER     1
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
static void di(const char *formato, ...)
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

#define M68K_RAM     0x00100000UL
#define M68K_RAM_LEN 0x00100000UL
#define VECTORES     0x00100000UL   /* VBR: 256 vectores */
#define TRAMPAS      0x00100400UL   /* una trampa de 14 bytes por vector */
#define COMUN        0x00101400UL   /* lo que acaba un programa: vuelve a la dirección 0 */
#define MARCA        0x00101500UL   /* el vector por el que acabó, o $FFFF */
#define DOS_BASE     0x00104000UL   /* la dos.library nativa; su tabla de saltos, por debajo */
#define PILA_TOPE    0x001F0000UL

#define JIT_RW   0x0000000100000000UL
#define JIT_RX   (JIT_RW | 0x0000001000000000UL)
#define JIT_LEN  (4UL << 20)
#define MONTON   0x0000000200000000UL
#define MONTON_LEN (4UL << 20)
#define PILAS    0x0000000300000000UL
#define PILA_LEN (1UL << 20)
#define PILA_VIGIA_LEN (64UL << 10)

static inline void pon16(uintptr_t a, uint16_t v) { *(volatile uint16_t *)a = v; }   /* el proceso es big-endian */
static inline void pon32(uintptr_t a, uint32_t v) { *(volatile uint32_t *)a = v; }
static inline uint16_t lee16(uintptr_t a) { return *(volatile uint16_t *)a; }
static inline uint32_t lee32(uintptr_t a) { return *(volatile uint32_t *)a; }

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

/* ¿Está [a, a + n) dentro de la memoria del 68k? Las direcciones del 68k son direcciones reales del proceso: lo que
   caiga fuera es memoria de la caja, y una función nativa no la toca por el 68k. */
static int en_ram68k(uint32_t a, uint32_t n)
{
    return a >= M68K_RAM && n <= M68K_RAM_LEN && a - M68K_RAM <= M68K_RAM_LEN - n;
}

/* --- La dos.library nativa (lo mínimo para escribir) --- */

/* Registros del 68k en la copia de x0..x30 que deja el JIT (EMIT_lineA, parche 0002). */
#define R_D(n) (19 + (n))
#define R_A(n) ((n) < 5 ? 13 + (n) : 22 + (n))
static uint32_t lee_r(uint64_t *regs, int r) { return (uint32_t)regs[r]; }
static void pon_r(uint64_t *regs, int r, uint32_t v) { regs[r] = v; }

#define SALIDA 0x00000001u   /* el único fichero: la consola */

static int llamadas_nativas;
static int rechazadas;

/* Escribe n bytes de la memoria del 68k en la consola. -1 si no son suyos. */
static long escribe68k(uint32_t a, uint32_t n)
{
    if (!en_ram68k(a, n)) return -1;
    if (largo) vaciar();
    for (uint32_t i = 0; i < n; i++) pon_c(NULL, *(const char *)(uintptr_t)(a + i));
    if (largo) vaciar();
    return n;
}

/* Lo que mide una cadena del 68k terminada en 0, o -1 si se sale de su memoria (o pasa de 4 KB). */
static long cadena68k(uint32_t a)
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
            /* Un número que no es de nadie: se rechaza, como una función que falla, y el programa sigue. */
            rechazadas++;
            llamadas_nativas--;
            di("función nativa %u: no existe; rechazada (D0 = -1)\n", index);
            pon_r(regs, R_D(0), 0xffffffffu);
    }
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
    for (size_t i = 0; i < sizeof(dos_tabla) / sizeof(dos_tabla[0]); i++) {
        uintptr_t e = DOS_BASE - dos_tabla[i].lvo;
        pon16(e, 0xa0ff); pon16(e + 2, dos_tabla[i].numero); pon16(e + 4, 0x4e75);
    }
}

/* Carga un programa plano en `donde` y lo ejecuta con A6 = DOSBase, en supervisor y sin máscara de interrupciones,
   hasta que vuelve a la dirección 0. Devuelve el vector por el que acabó, o -1 si acabó con su RTS. */
static int ejecuta(const uint8_t *desde, const uint8_t *hasta, uintptr_t donde)
{
    uint8_t *d = (uint8_t *)donde;
    for (const uint8_t *s = desde; s < hasta; ) *d++ = *s++;
    pon16(MARCA, 0xffff);
    for (int i = 0; i < 8; i++) { m68k.D[i].u32 = 0; m68k.A[i].u32 = 0; }
    for (int fp = 0; fp < 8; fp++) m68k.FP[fp].u64 = 0x7fffffffffffffffULL;
    pon32(PILA_TOPE - 4, 0);                /* el RTS final va a la dirección 0: MainLoop vuelve */
    m68k.ISP.u32 = PILA_TOPE - 4;
    m68k.A[7].u32 = PILA_TOPE - 4;
    m68k.A[6].u32 = DOS_BASE;
    m68k.VBR = VECTORES;
    m68k.PC = (uint32_t)donde;
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

    if (fallos) return 10 + fallos;

    /* 4. Hostil: el 68k escribe fuera de su memoria. El kernel termina la caja; si sigue viva, mal. */
    di("el 68k escribe en $DEAD0000, fuera de su memoria: la caja tiene que morir\n");
    v = ejecuta(prog_mala, prog_mala_fin, 0x00140000);
    di("MAL: la caja sigue viva (vector %d)\n", v);
    return 20;
}
