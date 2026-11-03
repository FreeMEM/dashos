/*
    DashOS: banco de pruebas del JIT de Emu68 en Linux AArch64 big-endian.

    Ejecuta un trozo de código 68k con el traductor de Emu68, sin el resto de Emu68
    (arranque, MMU, controladores). Se ejecuta en shiva con qemu-aarch64_be.

    No usa libc: las llamadas al sistema van a mano, y las funciones de cadena y memoria
    son las de src/support.c de Emu68.
*/

#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>
#include "M68k.h"
#include "support.h"
#include "tlsf.h"
#include "cache.h"
#include "config.h"

/* --- Llamadas al sistema de Linux AArch64 --- */

static long sys6(long n, long a, long b, long c, long d, long e, long f)
{
    register long x8 __asm__("x8") = n;
    register long x0 __asm__("x0") = a;
    register long x1 __asm__("x1") = b;
    register long x2 __asm__("x2") = c;
    register long x3 __asm__("x3") = d;
    register long x4 __asm__("x4") = e;
    register long x5 __asm__("x5") = f;
    __asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5) : "memory");
    return x0;
}

#define SYS_write        64
#define SYS_exit_group   94
#define SYS_mmap         222
#define SYS_ftruncate    46
#define SYS_memfd_create 279

#define PROT_READ   1
#define PROT_WRITE  2
#define PROT_EXEC   4
#define MAP_SHARED  0x01
#define MAP_PRIVATE 0x02
#define MAP_FIXED   0x10
#define MAP_ANON    0x20

static void *map(uintptr_t addr, size_t len, int prot, int flags, int fd)
{
    long r = sys6(SYS_mmap, addr, len, prot, flags, fd, 0);
    if (r < 0 && r > -4096) {
        kprintf("[HOST] mmap(%p, %d) falló: %d\n", (void *)addr, (int)len, (int)r);
        sys6(SYS_exit_group, 2, 0, 0, 0, 0, 0);
    }
    return (void *)r;
}

/* --- Salida de texto --- */

static char outbuf[4096];
static int outlen;

static void flush_out(void)
{
    if (outlen) sys6(SYS_write, 1, (long)outbuf, outlen, 0, 0, 0);
    outlen = 0;
}

static void put_out(void *data, char c)
{
    (void)data;
    outbuf[outlen++] = c;
    if (c == '\n' || outlen == sizeof(outbuf)) flush_out();
}

void vkprintf(const char *format, va_list args) { vkprintf_pc(put_out, NULL, format, args); }

void kprintf(const char *format, ...)
{
    va_list v;
    va_start(v, format);
    vkprintf_pc(put_out, NULL, format, v);
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
    kprintf("[JIT]");
    for (int i = 0; i < 8; i++) kprintf(" D%d=%08x", i, m68k->D[i].u32);
    kprintf("\n[JIT]");
    for (int i = 0; i < 8; i++) kprintf(" A%d=%08x", i, m68k->A[i].u32);
    kprintf("\n[JIT] PC=%08x SR=%04x\n", m68k->PC, m68k->SR);
}

void MainLoop(void);

/* --- Llamadas nativas desde el 68k (C1) --- */

/* Registros del 68k en la copia de x0..x30 que deja el JIT (ver EMIT_lineA en Emu68). */
#define R_D(n) (19 + (n))
static uint32_t lee(uint64_t *regs, int r) { return (uint32_t)regs[r]; }
static void pon(uint64_t *regs, int r, uint32_t v) { regs[r] = v; }

static int llamadas_nativas;

void dashos_native_call(uint64_t *regs, uint32_t index)
{
    llamadas_nativas++;
    switch (index)
    {
        case 1: /* D0 = D0 * 2, y avisa */
            kprintf("[NATIVO] función 1 llamada con D0=%d\n", lee(regs, R_D(0)));
            pon(regs, R_D(0), lee(regs, R_D(0)) * 2);
            break;
        case 2: /* D0 = D0 + D1 */
            kprintf("[NATIVO] función 2 llamada con D0=%d D1=%d\n", lee(regs, R_D(0)), lee(regs, R_D(1)));
            pon(regs, R_D(0), lee(regs, R_D(0)) + lee(regs, R_D(1)));
            break;
        default:
            kprintf("[NATIVO] función %d desconocida\n", index);
    }
}

/* --- Memoria --- */

/* La memoria del 68k empieza aquí. Las direcciones 68k son direcciones reales del proceso. */
#define M68K_RAM     0x00100000UL
#define M68K_RAM_LEN 0x00100000UL
#define M68K_RESULT  0x00180000UL

/*
    Emu68 escribe el código traducido en una dirección y lo ejecuta en la misma dirección con el
    bit 36 puesto (M68k_Translator.c: mt_ARMEntryPoint | 0x1000000000). En el kernel lo hace con
    dos entradas de MMU; aquí, con un memfd proyectado dos veces.
*/
#define JIT_RW  0x2000000000UL
#define JIT_RX  0x3000000000UL
#define JIT_LEN (64UL << 20)

#define HEAP_LEN (16UL << 20)

/* El programa 68k de prueba: suma de 1 a 100000 en D0, que se guarda en M68K_RESULT. */
static const uint16_t prueba[] = {
    0x7000,                         /*        moveq   #0,d0           */
    0x223c, 0x0001, 0x86a0,         /*        move.l  #100000,d1      */
    0xd081,                         /* bucle: add.l   d1,d0           */
    0x5381,                         /*        subq.l  #1,d1           */
    0x66fa,                         /*        bne.s   bucle           */
    0x41f9, 0x0018, 0x0000,         /*        lea     $180000,a0      */
    0x2080,                         /*        move.l  d0,(a0)         */
    0x4e75,                         /*        rts                     */
};

/* Programa 68k de C1: llama dos veces a una "biblioteca" nativa. */
#define LIB_BASE 0x00110100UL
static const uint16_t prueba_nativa[] = {
    0x2c7c, 0x0011, 0x0100,         /*        movea.l #LIB_BASE,a6    */
    0x7005,                         /*        moveq   #5,d0           */
    0x4eae, 0xfffa,                 /*        jsr     -6(a6)  ; 1     */
    0x2200,                         /*        move.l  d0,d1   ; 10    */
    0x7007,                         /*        moveq   #7,d0           */
    0x4eae, 0xfff4,                 /*        jsr     -12(a6) ; 2     */
    0x41f9, 0x0018, 0x0004,         /*        lea     $180004,a0      */
    0x2080,                         /*        move.l  d0,(a0) ; 17    */
    0x4e75,                         /*        rts                     */
};

static void prepara(struct M68KState *m68k, uint32_t pc)
{
    for (int i = 0; i < 8; i++) { m68k->D[i].u32 = 0; m68k->A[i].u32 = 0; }
    for (int fp = 0; fp < 8; fp++) m68k->FP[fp].u64 = 0x7fffffffffffffffULL;
    uint32_t sp = (uint32_t)(0x00100000UL + 0x00100000UL - 4096);
    m68k->ISP.u32 = sp - 4;
    *(uint32_t *)(uintptr_t)(sp - 4) = 0;
    m68k->PC = pc;
    m68k->SR = SR_S | SR_IPL;
}

static void ejecuta(struct M68KState *m68k)
{
    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    __asm__ volatile("mov " CTX_POINTER_ASM ", %0" ::"r"(m68k));
    __asm__ volatile("bl MainLoop" ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10",
                     "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22",
                     "x23", "x24", "x25", "x26", "x27", "x28", "x29", "x30", "v0", "v1", "v2", "v3", "v4",
                     "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16", "v17",
                     "v18", "v27", "v28", "v29", "v30", "v31", "memory", "cc");
}

void host_main(void)
{
    kprintf("[HOST] DashOS: JIT de Emu68 en Linux AArch64 big-endian\n");

    /* Montón general de Emu68 (tlsf) */
    tlsf = tlsf_init_with_memory(map(0, HEAP_LEN, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1), HEAP_LEN);

    /* Memoria del código traducido: la misma memoria en dos direcciones */
    long fd = sys6(SYS_memfd_create, (long)"emu68-jit", 0, 0, 0, 0, 0);
    if (fd < 0) { kprintf("[HOST] memfd_create falló: %d\n", (int)fd); flush_out(); sys6(SYS_exit_group, 2, 0, 0, 0, 0, 0); }
    sys6(SYS_ftruncate, fd, JIT_LEN, 0, 0, 0, 0);
    map(JIT_RW, JIT_LEN, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_FIXED, fd);
    map(JIT_RX, JIT_LEN, PROT_READ | PROT_EXEC, MAP_SHARED | MAP_FIXED, fd);
    jit_tlsf = tlsf_init_with_memory((void *)JIT_RW, JIT_LEN);

    /* Memoria del 68k */
    map(M68K_RAM, M68K_RAM_LEN, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON | MAP_FIXED, -1);
    uint16_t *code = (uint16_t *)M68K_RAM;
    for (size_t i = 0; i < sizeof(prueba) / sizeof(prueba[0]); i++) code[i] = prueba[i];

    uint64_t ctr;
    __asm__ volatile("mrs %0, CTR_EL0" : "=r"(ctr));
    dcache_mask_bits = 2 + ((ctr >> 16) & 15);

    /* Estado del 68k, como M68K_StartEmu sin PiStorm */
    static struct M68KState m68k;
    __m68k_state = &m68k;
    cache_setup();
    M68K_InitializeCache();

    for (int fp = 0; fp < 8; fp++) m68k.FP[fp].u64 = 0x7fffffffffffffffULL;
    uint32_t sp = (uint32_t)(M68K_RAM + M68K_RAM_LEN - 4096);
    m68k.ISP.u32 = sp - 4;
    *(uint32_t *)(uintptr_t)(sp - 4) = 0;   /* rts a la dirección 0: MainLoop vuelve */
    m68k.PC = (uint32_t)M68K_RAM;
    m68k.SR = SR_S | SR_IPL;
    m68k.CACR = 0x80008000;                 /* caché de instrucciones del JIT activa */
    m68k.JIT_CACHE_TOTAL = tlsf_get_total_size(jit_tlsf);
    m68k.JIT_CACHE_FREE = tlsf_get_free_size(jit_tlsf);
    m68k.JIT_SOFTFLUSH_THRESH = EMU68_WEAK_CFLUSH_LIMIT;
    m68k.JIT_CONTROL = EMU68_WEAK_CFLUSH ? JCCF_SOFT : 0;
    m68k.JIT_CONTROL |= (EMU68_M68K_INSN_DEPTH & JCCB_INSN_DEPTH_MASK) << JCCB_INSN_DEPTH;
    m68k.JIT_CONTROL |= (EMU68_BRANCH_INLINE_DISTANCE & JCCB_INLINE_RANGE_MASK) << JCCB_INLINE_RANGE;
    m68k.JIT_CONTROL |= (EMU68_MAX_LOOP_COUNT & JCCB_LOOP_COUNT_MASK) << JCCB_LOOP_COUNT;
    m68k.JIT_CONTROL2 = (EMU68_CCR_SCAN_DEPTH << JC2B_CCR_SCAN_DEPTH);

    kprintf("[HOST] Antes:\n");
    M68K_PrintContext(&m68k);
    flush_out();

    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    __asm__ volatile("mov " CTX_POINTER_ASM ", %0" ::"r"(&m68k));
    __asm__ volatile("bl MainLoop" ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10",
                     "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22",
                     "x23", "x24", "x25", "x26", "x27", "x28", "x29", "x30", "v0", "v1", "v2", "v3", "v4",
                     "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16", "v17",
                     "v18", "v27", "v28", "v29", "v30", "v31", "memory", "cc");

    kprintf("[HOST] Después:\n");
    M68K_PrintContext(&m68k);
    uint32_t esperado = 0;
    for (uint32_t i = 1; i <= 100000; i++) esperado += i;
    uint32_t leido = *(volatile uint32_t *)M68K_RESULT;
    kprintf("[HOST] Memoria en $180000: %08x, esperado %08x: %s\n", leido, esperado,
            (leido == esperado && m68k.D[0].u32 == esperado) ? "BIEN" : "MAL");
    kprintf("[HOST] Instrucciones 68k ejecutadas (aprox.): %d\n", (int)m68k.INSN_COUNT);
    int fallos = (leido == esperado && m68k.D[0].u32 == esperado) ? 0 : 1;

    /* C1: la biblioteca nativa. Tabla de saltos: $A0FF <n> RTS en LIB_BASE-6*n. */
    uint16_t *lib = (uint16_t *)LIB_BASE;
    for (int n = 1; n <= 2; n++) {
        uint16_t *e = lib - 3 * n;
        e[0] = 0xa0ff; e[1] = (uint16_t)n; e[2] = 0x4e75;
    }
    uint16_t *codigo2 = (uint16_t *)(M68K_RAM + 0x8000);
    for (size_t i = 0; i < sizeof(prueba_nativa) / sizeof(prueba_nativa[0]); i++) codigo2[i] = prueba_nativa[i];
    prepara(&m68k, (uint32_t)(uintptr_t)codigo2);
    m68k.CACR = 0x80008000;
    kprintf("[HOST] Prueba C1: llamadas nativas desde el 68k\n");
    flush_out();
    ejecuta(&m68k);
    uint32_t r2 = *(volatile uint32_t *)(M68K_RESULT + 4);
    kprintf("[HOST] Llamadas nativas: %d; resultado en $180004: %d (esperado 17); D0=%d: %s\n",
            llamadas_nativas, r2, m68k.D[0].u32,
            (llamadas_nativas == 2 && r2 == 17 && m68k.D[0].u32 == 17) ? "BIEN" : "MAL");
    if (!(llamadas_nativas == 2 && r2 == 17)) fallos++;
    flush_out();
    sys6(SYS_exit_group, fallos, 0, 0, 0, 0, 0);
}
