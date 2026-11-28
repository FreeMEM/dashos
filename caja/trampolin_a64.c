/*
    DashOS, K7 (docs/CHIPS_LOOP.md): los trampolines de la instrucción reescrita (trampolin_a64.h). Se compila con
    -mgeneral-regs-only: ni esto ni lo que llama (caja_chip_leer y caja_chip_escribir, Rust softfloat; motor_ciclos,
    que solo lee v20; chips_rearmar, K8, también general-regs-only) toca los registros de la FPU y SIMD, que el código
    traducido tiene vivos. Sin memcpy ni memset (el de la caja es C normal y puede usar SIMD): se comprueba con
    objdump (docs/CHIPS_LOOP.md, K7).
*/
#include "trampolin_a64.h"

uint64_t chips_tramp_accesos, chips_tramp_ram;
uint32_t chips_reescritos, chips_deshechos;
int chips_reescribe = 1;

static uintptr_t zona_rw, zona_rx;
static unsigned usados;
static struct trampolin trampolines[TRAMPOLINES];

void trampolines_iniciar(uintptr_t rw, uintptr_t rx)
{
    zona_rw = rw;
    zona_rx = rx;
    usados = 0;
}

int en_trampolines(uint64_t pc) { return zona_rx && pc >= zona_rx && pc < zona_rx + TRAMPOLINES_ZONA; }

/* Las instrucciones de A64 van siempre en little-endian, también en la caja big-endian. */
static uint32_t lee_ins(uint64_t rw)
{
    const volatile uint8_t *p = (const volatile uint8_t *)(uintptr_t)rw;
    return p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

static void pon_ins(uint64_t rw, uint32_t ins)
{
    volatile uint8_t *p = (volatile uint8_t *)(uintptr_t)rw;
    p[0] = (uint8_t)ins; p[1] = (uint8_t)(ins >> 8); p[2] = (uint8_t)(ins >> 16); p[3] = (uint8_t)(ins >> 24);
}

/* Lo escrito por la proyección de escribir, a la de ejecutar: limpia los datos hasta el punto de unificación y anula
   las instrucciones (EL0 puede: Emu68 lo hace igual, support.c). */
static void limpia(uint64_t rw, uint64_t rx, unsigned largo)
{
    uint64_t ctr;
    __asm__ volatile("mrs %0, ctr_el0" : "=r"(ctr));
    uint64_t dl = 4u << ((ctr >> 16) & 15), il = 4u << (ctr & 15);
    for (uint64_t a = rw & ~(dl - 1); a < rw + largo; a += dl) __asm__ volatile("dc cvau, %0" ::"r"(a) : "memory");
    __asm__ volatile("dsb ish" ::: "memory");
    for (uint64_t a = rx & ~(il - 1); a < rx + largo; a += il) __asm__ volatile("ic ivau, %0" ::"r"(a) : "memory");
    __asm__ volatile("dsb ish\n isb" ::: "memory");
}

/* La proyección de escribir de una dirección de ejecutar del código traducido. */
static uint64_t rw_de(uint64_t rx) { return rx - (zona_rx - zona_rw); }

static uint32_t salto(uint64_t desde, uint64_t hasta) { return 0x14000000u | ((uint32_t)((hasta - desde) >> 2) & 0x3ffffffu); }

static void pon_en_sitio(uint64_t sitio, uint32_t ins)
{
    pon_ins(rw_de(sitio), ins);
    limpia(rw_de(sitio), sitio, 4);
}

/* ¿Sigue el sitio de `t` (el k) saltando a su trampolín? */
static int sigue(const struct trampolin *t, unsigned k)
{
    return t->sitio && lee_ins(rw_de(t->sitio)) == salto(t->sitio, zona_rx + (uint64_t)k * TRAMPOLIN_BYTES);
}

void trampolines_deshacer(void)
{
    for (unsigned k = 0; k < usados; k++) {
        struct trampolin *t = &trampolines[k];
        if (sigue(t, k)) pon_en_sitio(t->sitio, t->ins);
        t->sitio = 0;
    }
    usados = 0;
}

void caja_trampolin_comun(void);

int trampolin_poner(uint64_t sitio, uint32_t ins, const struct acceso *a)
{
    if (!chips_reescribe || !zona_rx || en_trampolines(sitio)) return 0;
    if ((ins & 0x3f000000u) == 0x08000000u || a->rn == 31) return 0;   /* exclusivas; base SP */
    int64_t lejos = (int64_t)(zona_rx - sitio);
    if (lejos >= (128 << 20) - (int64_t)TRAMPOLINES_ZONA || lejos <= -(128 << 20)) return 0;
    if (usados == TRAMPOLINES) trampolines_deshacer();
    unsigned k = usados++;
    uint64_t rx = zona_rx + (uint64_t)k * TRAMPOLIN_BYTES, rw = zona_rw + (uint64_t)k * TRAMPOLIN_BYTES;
    struct trampolin *t = &trampolines[k];
    t->sitio = sitio;
    t->ins = ins;
    t->ram_seguidas = 0;
    static const uint32_t fijo[] = { 0xf81f0ffe, 0x580000fe, 0xd63f03c0, 0xf84107fe, 0, 0xf84107fe };
    for (unsigned j = 0; j < 6; j++) pon_ins(rw + 4 * j, fijo[j]);
    pon_ins(rw + 16, salto(rx + 16, sitio + 4));
    pon_ins(rw + 24, ins);
    pon_ins(rw + 28, salto(rx + 28, sitio + 4));
    *(volatile uint64_t *)(uintptr_t)(rw + 32) = (uint64_t)(uintptr_t)caja_trampolin_comun;
    *(volatile uint64_t *)(uintptr_t)(rw + 40) = (uint64_t)(uintptr_t)t;
    for (unsigned j = 48; j < TRAMPOLIN_BYTES; j += 4) pon_ins(rw + j, 0);   /* udf #0 */
    limpia(rw, rx, TRAMPOLIN_BYTES);
    pon_en_sitio(sitio, salto(sitio, rx));
    chips_reescritos++;
    return 1;
}

/* Lo llama caja_trampolin_comun con el marco (los registros del sitio; pc, la vuelta al trampolín) y la descripción.
   1: hecho con los chips (el marco trae lo que cambió); 0: no es de los chips, hay que hacer el acceso normal. */
int caja_trampolin_c(marco_fallo *m, struct trampolin *t);
__attribute__((used, noipa)) int caja_trampolin_c(marco_fallo *m, struct trampolin *t)
{
    struct acceso a;
    if (!decodifica(t->ins, m, &a)) return 0;
    if (!en_zona(a.dir, a.tam * a.pares)) {
        /* RAM (o algo que cruza: el acceso normal falla y K6 lo trata como siempre). */
        chips_tramp_ram++;
        if (++t->ram_seguidas >= TRAMPOLIN_RAM_SEGUIDAS) {
            /* El trampolín sigue entero (lo está usando quien llama): solo el sitio vuelve a lo de antes. */
            pon_en_sitio(t->sitio, t->ins);
            t->ram_seguidas = 0;
            chips_deshechos++;
        }
        return 0;
    }
    t->ram_seguidas = 0;
    uint64_t pc = m->pc;
    m->far = a.dir;
    uint64_t hora = motor_ciclos();
    chips_atiende(m, t->ins, hora, &a);
    if (a.escribe) chips_rearmar(hora);   /* K8: una escritura puede cambiar el nivel o el plazo */
    m->pc = pc;
    chips_tramp_accesos++;
    return 1;
}

/* La rutina común. Entra con x30 = trampolín + 12 y, en [sp], el x30 del sitio (SP del sitio = sp + 16). Arma el marco
   (288 bytes, el de K5: x0-x30, sp, pc, pstate), llama a caja_trampolin_c, repone NZCV, deja en la pila el x30 final y
   vuelve al trampolín: a +12 si fue de los chips, a +20 (la instrucción original) si no. */
__asm__(
    ".text\n"
    ".balign 16\n"
    ".globl caja_trampolin_comun\n"
    ".type caja_trampolin_comun, %function\n"
    "caja_trampolin_comun:\n"
    "sub sp, sp, #288\n"
    "stp x0, x1, [sp, #0]\n"     "stp x2, x3, [sp, #16]\n"    "stp x4, x5, [sp, #32]\n"
    "stp x6, x7, [sp, #48]\n"    "stp x8, x9, [sp, #64]\n"    "stp x10, x11, [sp, #80]\n"
    "stp x12, x13, [sp, #96]\n"  "stp x14, x15, [sp, #112]\n" "stp x16, x17, [sp, #128]\n"
    "stp x18, x19, [sp, #144]\n" "stp x20, x21, [sp, #160]\n" "stp x22, x23, [sp, #176]\n"
    "stp x24, x25, [sp, #192]\n" "stp x26, x27, [sp, #208]\n" "stp x28, x29, [sp, #224]\n"
    "ldr x0, [sp, #288]\n"
    "add x1, sp, #304\n"
    "stp x0, x1, [sp, #240]\n"
    "mrs x2, nzcv\n"
    "stp x30, x2, [sp, #256]\n"
    "ldur x1, [x30, #28]\n"
    "mov x0, sp\n"
    "bl caja_trampolin_c\n"
    "ldp x1, x2, [sp, #256]\n"
    "msr nzcv, x2\n"
    "cbnz w0, 1f\n"
    "add x1, x1, #8\n"
    "1:\n"
    "mov x30, x1\n"
    "ldr x2, [sp, #240]\n"
    "str x2, [sp, #288]\n"
    "ldp x0, x1, [sp, #0]\n"     "ldp x2, x3, [sp, #16]\n"    "ldp x4, x5, [sp, #32]\n"
    "ldp x6, x7, [sp, #48]\n"    "ldp x8, x9, [sp, #64]\n"    "ldp x10, x11, [sp, #80]\n"
    "ldp x12, x13, [sp, #96]\n"  "ldp x14, x15, [sp, #112]\n" "ldp x16, x17, [sp, #128]\n"
    "ldp x18, x19, [sp, #144]\n" "ldp x20, x21, [sp, #160]\n" "ldp x22, x23, [sp, #176]\n"
    "ldp x24, x25, [sp, #192]\n" "ldp x26, x27, [sp, #208]\n" "ldp x28, x29, [sp, #224]\n"
    "add sp, sp, #288\n"
    "ret\n"
    ".size caja_trampolin_comun, . - caja_trampolin_comun\n");
