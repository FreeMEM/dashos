/*
    DashOS, fase F1 de la caja PowerPC (docs/PPC_LOOP.md): banco de pruebas del JIT PPC de Emu68 en Linux AArch64
    big-endian, sin el resto de Emu68 (arranque, MMU, ROM PPC, controladores). Se ejecuta con qemu-aarch64_be.

    jit-ppc programa.bin: carga un binario PPC plano en 0x00140000, con la memoria del PPC en 0x00100000..0x001fffff
    (las direcciones PPC son direcciones reales del proceso, como en el banco del 68k), pone a cero todos los GPR
    salvo r1 (la pila, 0x001ff000), LR a 0 (el blr del final vuelve a la dirección 0 y el bucle del JIT termina), CR,
    XER y CTR a 0 y los FPR a 0x7fffffffffffffff, y lo ejecuta. Escribe los registros y una suma de la memoria, en el
    formato de pruebas/oraculo.c, que hace lo mismo en qemu-ppc: ../comparar_ppc.py compara las dos salidas.

    No usa libc: las llamadas al sistema van a mano, y las funciones de cadena y memoria son las de src/support.c.
*/

#define restrict __restrict__
#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>
#include "PPC.h"
#include "support.h"
#include "tlsf.h"
#include "config.h"
#include "M68k.h"
#include "devicetree.h"
#include "cache.h"

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

#define SYS_openat       56
#define SYS_close        57
#define SYS_read         63
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

extern "C" void vkprintf(const char *format, va_list args) { vkprintf_pc(put_out, NULL, format, args); }

extern "C" void kprintf(const char *format, ...)
{
    va_list v;
    va_start(v, format);
    vkprintf_pc(put_out, NULL, format, v);
    va_end(v);
}

static void *map(uintptr_t addr, size_t len, int prot, int flags, int fd)
{
    long r = sys6(SYS_mmap, addr, len, prot, flags, fd, 0);
    if (r < 0 && r > -4096) {
        kprintf("[HOST] mmap(%p, %d) falló: %d\n", (void *)addr, (int)len, (int)r);
        flush_out();
        sys6(SYS_exit_group, 2, 0, 0, 0, 0, 0);
    }
    return (void *)r;
}

/* --- Memoria --- */

#define PPC_RAM      0x00100000UL
#define PPC_RAM_LEN  0x00100000UL
#define PPC_PROGRAMA 0x00140000UL
#define PPC_PILA     0x001ff000UL

/* Como en el banco del 68k: el código traducido se escribe en JIT_RW y se ejecuta en JIT_RW | bit 36. */
#define JIT_RW  0x2000000000UL
#define JIT_RX  0x3000000000UL
#define JIT_LEN (64UL << 20)
#define HEAP_LEN (16UL << 20)

/* --- Lo que Emu68 define en start.c, devicetree.c, mmu.c, intc.c y disasm.c --- */

extern "C" {
uint8_t host_flags;
int disasm;
int debug;
void *ppc_jit_virt_base = (void *)JIT_RW;
struct M68KState *__m68k_state;
uint16_t *framebuffer;
uint32_t pitch, fb_width, fb_height;
int debug_not_implemented;
int dcache_mask_bits;

of_node_t *dt_find_node(const char *key) { (void)key; return nullptr; }
uint32_t dt_get_property_value_u32(void *key, const char *propname, uint32_t def_val, int check_parent)
{
    (void)key; (void)def_val; (void)check_parent;
    /* tamaño del código traducido del PPC, en MB (PPC_Translator.cpp: jit_ppc) */
    if (propname && propname[0] == 'p') return JIT_LEN >> 20;
    return def_val;
}
uintptr_t mmu_virt2phys(uintptr_t addr) { return addr; }
void mmu_map(uintptr_t phys, uintptr_t virt, uintptr_t length, uint32_t attr_low, uint32_t attr_high)
{
    (void)phys; (void)virt; (void)length; (void)attr_low; (void)attr_high;
}
int gic_available() { return 0; }
void gic_local_init() {}
void gic_irq_eanble(unsigned int id) { (void)id; }
void gic_set_priority(unsigned int id, uint8_t prio) { (void)id; (void)prio; }
void disasm_init(void) {}
void disasm_open(void) {}
void disasm_close(void) {}
void disasm_print_ppc(uint32_t *ppc_addr, uint32_t ppc_count, uint32_t *arm_addr, size_t arm_size, uint32_t *arm_start)
{
    (void)ppc_addr; (void)ppc_count; (void)arm_addr; (void)arm_size; (void)arm_start;
}
void disasm_print_ppc_only(uint32_t *ppc_addr) { (void)ppc_addr; }
void disasm_print(uint16_t *m68k_addr, uint16_t m68k_count, uint32_t *arm_addr, size_t arm_size, uint32_t *arm_start)
{
    (void)m68k_addr; (void)m68k_count; (void)arm_addr; (void)arm_size; (void)arm_start;
}
void InitPPC();
}

namespace Emu68::PPC { void PPCMainLoop(); }

/* PPC_Translator.cpp (parche 0010): una instrucción que el JIT no traduce. El banco acaba con 5. */
extern "C" void dashos_ppc_unimplemented(uint32_t opcode, uint32_t pc)
{
    kprintf("[REG] sin traducir: %08x en %08x\n", opcode, pc);
    flush_out();
    sys6(SYS_exit_group, 5, 0, 0, 0, 0, 0);
}

/* Los constructores globales de C++ (las listas de la caché del JIT, jit_ppc): sin libc nadie los llama. */
typedef void (*ctor_t)(void);
extern "C" ctor_t __init_array_start[] __attribute__((weak));
extern "C" ctor_t __init_array_end[] __attribute__((weak));

static long carga(const char *ruta, uintptr_t donde, size_t max)
{
    long fd = sys6(SYS_openat, -100, (long)ruta, 0, 0, 0, 0);
    if (fd < 0) return -1;
    long total = 0, n;
    while ((size_t)total < max && (n = sys6(SYS_read, fd, (long)(donde + total), max - total, 0, 0, 0)) > 0) total += n;
    sys6(SYS_close, fd, 0, 0, 0, 0, 0);
    return total;
}

/* FNV-1a de 64 bits */
static uint64_t suma(const uint8_t *p, size_t n)
{
    uint64_t h = 0xcbf29ce484222325ULL;
    while (n--) { h ^= *p++; h *= 0x100000001b3ULL; }
    return h;
}

/* La salida que compara ../comparar_ppc.py: la misma que da pruebas/oraculo.c en qemu-ppc */
static void informe(const Emu68::PPC::PPCState *s)
{
    for (int i = 0; i < 32; i++)
        kprintf("%s r%d=%08x%s", (i % 8) ? "" : "[REG]", i, s->GPR[i], (i % 8 == 7) ? "\n" : "");
    kprintf("[REG] CR=%08x XER=%08x CTR=%08x\n", s->CR, s->XER, s->CTR);
    for (int i = 0; i < 32; i++)
        kprintf("%s f%d=%08x%08x%s", (i % 4) ? "" : "[REG]", i, (uint32_t)(s->FPR_u64[i] >> 32), (uint32_t)s->FPR_u64[i],
                (i % 4 == 3) ? "\n" : "");
    kprintf("[REG] FPSCR=%08x\n", s->FPSCR);
    uint64_t h = suma((const uint8_t *)PPC_RAM, PPC_RAM_LEN);
    kprintf("[REG] Suma de la memoria: %08x%08x\n", (uint32_t)(h >> 32), (uint32_t)h);
}

extern "C" void host_main(uint64_t *pila)
{
    int argc = (int)pila[0];
    char **argv = (char **)&pila[1];
    kprintf("[HOST] DashOS: JIT PPC de Emu68 en Linux AArch64 big-endian\n");
    if (argc < 2) {
        kprintf("[HOST] uso: jit-ppc programa.bin\n");
        flush_out();
        sys6(SYS_exit_group, 1, 0, 0, 0, 0, 0);
    }

    tlsf = tlsf_init_with_memory(map(0, HEAP_LEN, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1), HEAP_LEN);

    long fd = sys6(SYS_memfd_create, (long)"emu68-ppc-jit", 0, 0, 0, 0, 0);
    if (fd < 0) { kprintf("[HOST] memfd_create falló: %d\n", (int)fd); flush_out(); sys6(SYS_exit_group, 2, 0, 0, 0, 0, 0); }
    sys6(SYS_ftruncate, fd, JIT_LEN, 0, 0, 0, 0);
    map(JIT_RW, JIT_LEN, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_FIXED, fd);
    map(JIT_RX, JIT_LEN, PROT_READ | PROT_EXEC, MAP_SHARED | MAP_FIXED, fd);

    uint64_t ctr;
    __asm__ volatile("mrs %0, CTR_EL0" : "=r"(ctr));
    dcache_mask_bits = 2 + ((ctr >> 16) & 15);

    /* ahora sí: jit_ppc se construye sobre JIT_RW */
    for (ctor_t *c = __init_array_start; c && c < __init_array_end; c++) (*c)();

    map(PPC_RAM, PPC_RAM_LEN, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON | MAP_FIXED, -1);
    long n = carga(argv[1], PPC_PROGRAMA, PPC_PILA - 0x1000 - PPC_PROGRAMA);
    if (n <= 0) {
        kprintf("[HOST] no se puede leer %s\n", argv[1]);
        flush_out();
        sys6(SYS_exit_group, 1, 0, 0, 0, 0, 0);
    }

    cache_setup();
    InitPPC();

    static Emu68::PPC::PPCState ppc;
    for (int fp = 0; fp < 32; fp++) ppc.FPR_u64[fp] = 0x7fffffffffffffffULL;
    ppc.MSR = MSR_FP;
    ppc.JIT_CACHE_TOTAL = 0;
    ppc.JIT_CONTROL = (EMU68_M68K_INSN_DEPTH & JCCB_INSN_DEPTH_MASK) << JCCB_INSN_DEPTH;
    ppc.JIT_CONTROL |= (EMU68_BRANCH_INLINE_DISTANCE & JCCB_INLINE_RANGE_MASK) << JCCB_INLINE_RANGE;
    ppc.JIT_CONTROL |= (EMU68_MAX_LOOP_COUNT & JCCB_LOOP_COUNT_MASK) << JCCB_LOOP_COUNT;
    ppc.GPR[1] = (uint32_t)PPC_PILA;
    ppc.LR = 0;
    ppc.CTR = (uint32_t)PPC_PROGRAMA;   /* como el oráculo, que entra con bctr */
    ppc.PC = (uint32_t)PPC_PROGRAMA;

    kprintf("[HOST] Programa %s (%d bytes)\n", argv[1], (int)n);
    flush_out();

    __asm__ volatile("mov " CTX_LAST_PC_ASM ", %w0" ::"r"(0xffffffff));
    __asm__ volatile("mov " CTX_POINTER_ASM ", %0" ::"r"(&ppc));
    __asm__ volatile("bl _ZN5Emu683PPC11PPCMainLoopEv" ::: "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9",
                     "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22",
                     "x23", "x24", "x25", "x26", "x27", "x28", "x29", "x30", "v0", "v1", "v2", "v3", "v4",
                     "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15", "v16", "v17",
                     "v18", "v27", "v28", "v29", "v30", "v31", "memory", "cc");

    informe(&ppc);
    if (argc > 2) {   /* la memoria del PPC, para ver dónde difiere */
        long f = sys6(SYS_openat, -100, (long)argv[2], 01 | 0100 | 01000, 0644, 0, 0);
        if (f >= 0) { sys6(SYS_write, f, (long)PPC_RAM, PPC_RAM_LEN, 0, 0, 0); sys6(SYS_close, f, 0, 0, 0, 0, 0); }
    }
    kprintf("[HOST] Instrucciones PPC ejecutadas (aprox.): %d\n", (int)ppc.INSN_COUNT);
    flush_out();
    sys6(SYS_exit_group, 0, 0, 0, 0, 0, 0);
}
