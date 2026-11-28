/*
    K7 (docs/CHIPS_LOOP.md): los trampolines de ../trampolin_a64.c contra la CPU, en Linux con qemu-user, en big-endian
    y en little-endian (`make -C caja/pruebas_fallo`; la lanza también `make -C kernel prueba`).

    Un arnés pone x1-x8, x30, NZCV y q0-q31 al azar, ejecuta una instrucción en su sitio y los guarda. Primero con la
    instrucción original (lo de referencia); después con el sitio reescrito por trampolin_poner, como en la caja. Con la
    base en $BF0100 la de referencia va a una memoria de verdad en $BF0000 y el trampolín, a los chips de mentira (la
    sombra, con los mismos bytes); con la base en la RAM ($600100), los dos van a la misma RAM (el camino normal del
    trampolín). Tienen que acabar iguales los registros, NZCV, la memoria, y q0-q31 como estaban (el trampolín no los
    guarda). Además: deshacer devuelve el sitio a su instrucción, y muchos accesos seguidos a la RAM también.
*/
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/mman.h>
#include "../trampolin_a64.h"

#define BASE 0xBF0000u
#define RAMB 0x600000u
static uint8_t sombra[0x10000];

/* Los chips de mentira y la hora: sin la FPU, como caja/chips (Rust softfloat) y motor_ciclos. */
#pragma GCC push_options
#pragma GCC target("general-regs-only")
uint32_t caja_chip_leer(uint32_t dir, unsigned tam, uint64_t hora)
{
    (void)hora; uint32_t v = 0;
    for (unsigned k = 0; k < tam; k++) v = v << 8 | sombra[dir - BASE + k];
    return v;
}
void caja_chip_escribir(uint32_t dir, uint32_t valor, unsigned tam, uint64_t hora)
{
    (void)hora;
    for (unsigned k = 0; k < tam; k++) sombra[dir - BASE + k] = (uint8_t)(valor >> (8 * (tam - 1 - k)));
}
uint64_t motor_ciclos(void) { return 0; }
#pragma GCC pop_options

/* El arnés: x0 = r (uint64_t[160]): r[1..8] = x1-x8, r[9] = x30, r[10] = NZCV, q0-q31 de r[16..79] a la entrada y en
   r[80..143] a la salida. */
extern const uint8_t arnes[], arnes_sitio[], arnes_fin[];
__asm__(".text\n.balign 4\narnes:\n"
        "stp x29, x30, [sp, #-80]!\n"
        "stp d8, d9, [sp, #16]\n stp d10, d11, [sp, #32]\n stp d12, d13, [sp, #48]\n stp d14, d15, [sp, #64]\n"
        "ldr x9, [x0, #80]\n msr nzcv, x9\n"
        "add x9, x0, #128\n"
        "ldp q0, q1, [x9, #0]\n ldp q2, q3, [x9, #32]\n ldp q4, q5, [x9, #64]\n ldp q6, q7, [x9, #96]\n"
        "ldp q8, q9, [x9, #128]\n ldp q10, q11, [x9, #160]\n ldp q12, q13, [x9, #192]\n ldp q14, q15, [x9, #224]\n"
        "ldp q16, q17, [x9, #256]\n ldp q18, q19, [x9, #288]\n ldp q20, q21, [x9, #320]\n ldp q22, q23, [x9, #352]\n"
        "ldp q24, q25, [x9, #384]\n ldp q26, q27, [x9, #416]\n ldp q28, q29, [x9, #448]\n ldp q30, q31, [x9, #480]\n"
        "ldp x1, x2, [x0, #8]\n ldp x3, x4, [x0, #24]\n ldp x5, x6, [x0, #40]\n ldp x7, x8, [x0, #56]\n"
        "ldr x30, [x0, #72]\n"
        "arnes_sitio:\n nop\n"
        "stp x1, x2, [x0, #8]\n stp x3, x4, [x0, #24]\n stp x5, x6, [x0, #40]\n stp x7, x8, [x0, #56]\n"
        "str x30, [x0, #72]\n"
        "mrs x9, nzcv\n str x9, [x0, #80]\n"
        "add x9, x0, #640\n"
        "stp q0, q1, [x9, #0]\n stp q2, q3, [x9, #32]\n stp q4, q5, [x9, #64]\n stp q6, q7, [x9, #96]\n"
        "stp q8, q9, [x9, #128]\n stp q10, q11, [x9, #160]\n stp q12, q13, [x9, #192]\n stp q14, q15, [x9, #224]\n"
        "stp q16, q17, [x9, #256]\n stp q18, q19, [x9, #288]\n stp q20, q21, [x9, #320]\n stp q22, q23, [x9, #352]\n"
        "stp q24, q25, [x9, #384]\n stp q26, q27, [x9, #416]\n stp q28, q29, [x9, #448]\n stp q30, q31, [x9, #480]\n"
        "ldp d8, d9, [sp, #16]\n ldp d10, d11, [sp, #32]\n ldp d12, d13, [sp, #48]\n ldp d14, d15, [sp, #64]\n"
        "ldp x29, x30, [sp], #80\n"
        "ret\n"
        "arnes_fin:\n");

/* Las formas de K6 que se reescriben (no las exclusivas), y algunas con x30. */
extern const uint8_t tabla[], tabla_fin[];
__asm__(".section .rodata\n.balign 4\ntabla:\n"
        "ldr w1,[x3,#4]\n ldr x1,[x3,#8]\n ldrh w1,[x3,#2]\n ldrb w1,[x3,#1]\n ldrsh w1,[x3,#2]\n ldrsh x1,[x3,#2]\n"
        "ldrsb w1,[x3,#3]\n ldrsb x1,[x3]\n ldrsw x1,[x3,#4]\n"
        "ldur w1,[x3,#-3]\n ldurh w1,[x3,#-1]\n ldursh w1,[x3,#5]\n ldur x1,[x3,#7]\n ldurb w1,[x3,#9]\n"
        "ldr w1,[x3,#4]!\n ldr w1,[x3],#-4\n ldrh w1,[x3,#-2]!\n ldrb w1,[x3],#1\n ldrsb x1,[x3],#-1\n ldr x1,[x3,#-8]!\n"
        "ldr w1,[x3,x4]\n ldr w1,[x3,w7,uxtw]\n ldr w1,[x3,w8,sxtw #2]\n ldrh w1,[x3,x4,lsl #1]\n ldr x1,[x3,x4,lsl #3]\n"
        "str w1,[x3,#4]\n str x1,[x3,#8]\n strh w1,[x3,#2]\n strb w1,[x3,#1]\n stur w1,[x3,#-3]\n stur x1,[x3,#-5]\n"
        "str w1,[x3,#-4]!\n strh w1,[x3],#2\n str x1,[x3],#-8\n str w1,[x3,x4]\n strh w1,[x3,w7,uxtw #1]\n"
        "str x1,[x3,w8,sxtw]\n str wzr,[x3]\n"
        "ldp w1,w2,[x3,#8]\n ldp w1,w2,[x3,#-8]!\n ldp x1,x2,[x3],#16\n ldpsw x1,x2,[x3,#8]\n ldnp w1,w2,[x3,#4]\n"
        "stp w1,w2,[x3,#8]\n stp x1,x2,[x3,#-16]!\n stp w1,w2,[x3],#8\n stp wzr,w2,[x3]\n"
        "ldr x30,[x3,#8]\n ldrh w30,[x3],#2\n str x30,[x3,#4]\n ldp w1,w30,[x3,#8]\n stp x30,x1,[x3,#-16]!\n"
        "tabla_fin:\n.text\n");

static uint64_t azar(void) { return ((uint64_t)rand() << 40) ^ ((uint64_t)rand() << 20) ^ (uint64_t)rand(); }
static uint32_t lee_ins(const uint8_t *p) { return p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }
static void pon(uint8_t *p, uint32_t i) { p[0] = (uint8_t)i; p[1] = (uint8_t)(i >> 8); p[2] = (uint8_t)(i >> 16); p[3] = (uint8_t)(i >> 24); }

static uint8_t *cod, *sitio;
static uint64_t r_ini[160];

/* Prepara el arnés con `ins` en el sitio (y, si `reescribe`, la reescribe con un trampolín) y lo ejecuta con r. */
static int corre(uint32_t ins, int reescribe, uint64_t *r)
{
    trampolines_deshacer();
    pon(sitio, ins);
    __builtin___clear_cache((char *)sitio, (char *)sitio + 4);
    if (reescribe) {
        marco_fallo m; memset(&m, 0, sizeof m);
        for (int j = 1; j <= 8; j++) m.x[j] = r[j];
        m.x[30] = r[9];
        struct acceso a;
        if (!decodifica(ins, &m, &a) || !trampolin_poner((uint64_t)(uintptr_t)sitio, ins, &a)) return 0;
        if ((lee_ins(sitio) & 0xfc000000u) != 0x14000000u) return 0;
    }
    ((void (*)(uint64_t *))cod)(r);
    return 1;
}

static void regs_al_azar(uint64_t base)
{
    for (int j = 0; j < 160; j++) r_ini[j] = azar();
    r_ini[3] = base + 0x100; r_ini[4] = 4; r_ini[7] = 0xFFFFFFFF00000006ull; r_ini[8] = 0x12345678FFFFFFF8ull;
    r_ini[10] = azar() & 0xf0000000u;
}

static unsigned compara(uint32_t ins, const char *donde, const uint64_t *ref, const uint64_t *tra, int mem_bien)
{
    int bien = mem_bien;
    for (int j = 1; j <= 10; j++) if (ref[j] != tra[j]) bien = 0;
    if (memcmp(tra + 80, r_ini + 16, 512) || memcmp(ref + 80, r_ini + 16, 512)) bien = 0;
    if (bien) return 0;
    printf("%08x (%s): MAL", ins, donde);
    for (int j = 1; j <= 10; j++)
        if (ref[j] != tra[j]) printf(" r%d %016llx/%016llx", j, (unsigned long long)tra[j], (unsigned long long)ref[j]);
    for (int j = 0; j < 64; j++)
        if (tra[80 + j] != r_ini[16 + j] || ref[80 + j] != r_ini[16 + j]) {
            printf(" q%d (mitad %d) %016llx/%016llx, era %016llx", j / 2, j % 2, (unsigned long long)tra[80 + j],
                   (unsigned long long)ref[80 + j], (unsigned long long)r_ini[16 + j]);
            break;
        }
    if (!mem_bien) printf(" memoria");
    printf("\n");
    return 1;
}

int main(void)
{
    uint8_t *real = mmap((void *)(uintptr_t)BASE, 0x10000, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    uint8_t *ram = mmap((void *)(uintptr_t)RAMB, 0x10000, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    cod = mmap(0, 4096 + TRAMPOLINES_ZONA, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (real != (uint8_t *)(uintptr_t)BASE || ram != (uint8_t *)(uintptr_t)RAMB || cod == MAP_FAILED) { printf("mmap\n"); return 2; }
    memcpy(cod, arnes, arnes_fin - arnes);
    sitio = cod + (arnes_sitio - arnes);
    trampolines_iniciar((uintptr_t)cod + 4096, (uintptr_t)cod + 4096);
    static uint8_t ram_ini[0x10000], ram_ref[0x10000];
    static uint64_t ref[160], tra[160];
    unsigned n = (unsigned)(tabla_fin - tabla) / 4, mal = 0;
    uint64_t chips_antes = chips_tramp_accesos, ram_antes = chips_tramp_ram;
    for (int vuelta = 0; vuelta < 20; vuelta++)
    for (unsigned k = 0; k < n; k++) {
        uint32_t ins = lee_ins(tabla + 4 * k);
        /* los chips: la de verdad en `real`, el trampolín en la sombra */
        for (unsigned j = 0; j < 0x10000; j++) real[j] = sombra[j] = (uint8_t)rand();
        regs_al_azar(BASE);
        memcpy(ref, r_ini, sizeof ref); memcpy(tra, r_ini, sizeof tra);
        if (!corre(ins, 0, ref) || !corre(ins, 1, tra)) { printf("%08x: no se reescribe\n", ins); mal++; continue; }
        mal += compara(ins, "chips", ref, tra, memcmp(real, sombra, 0x10000) == 0);
        /* la RAM: las dos en la misma memoria, desde los mismos bytes */
        for (unsigned j = 0; j < 0x10000; j++) ram_ini[j] = (uint8_t)rand();
        regs_al_azar(RAMB);
        memcpy(ref, r_ini, sizeof ref); memcpy(tra, r_ini, sizeof tra);
        memcpy(ram, ram_ini, 0x10000);
        corre(ins, 0, ref);
        memcpy(ram_ref, ram, 0x10000);
        memcpy(ram, ram_ini, 0x10000);
        corre(ins, 1, tra);
        mal += compara(ins, "RAM", ref, tra, memcmp(ram, ram_ref, 0x10000) == 0);
    }
    if (chips_tramp_accesos - chips_antes != 20ull * n || chips_tramp_ram - ram_antes != 20ull * n) {
        printf("cuentas: %llu por los chips y %llu por la RAM (esperaba %u de cada)\n",
               (unsigned long long)(chips_tramp_accesos - chips_antes), (unsigned long long)(chips_tramp_ram - ram_antes), 20 * n);
        mal++;
    }

    /* deshacer devuelve la instrucción; muchas vueltas seguidas a la RAM, también */
    uint32_t ins = lee_ins(tabla);
    regs_al_azar(RAMB);
    memcpy(tra, r_ini, sizeof tra);
    corre(ins, 1, tra);
    trampolines_deshacer();
    if (lee_ins(sitio) != ins) { printf("deshacer: el sitio no vuelve a su instrucción\n"); mal++; }
    uint32_t deshechos = chips_deshechos;
    corre(ins, 1, tra);
    for (unsigned v = 1; v < TRAMPOLIN_RAM_SEGUIDAS; v++) ((void (*)(uint64_t *))cod)(tra);
    if (lee_ins(sitio) != ins || chips_deshechos != deshechos + 1) { printf("la RAM seguida no deshace el sitio\n"); mal++; }
    /* lo que no se reescribe: una exclusiva y la base SP */
    marco_fallo m; memset(&m, 0, sizeof m); struct acceso a;
    uint32_t no[] = { 0x885f7c61 /* ldxr w1,[x3] */, 0xb94007e1 /* ldr w1,[sp,#4] */ };
    for (unsigned j = 0; j < sizeof no / 4; j++)
        if (!decodifica(no[j], &m, &a) || trampolin_poner((uint64_t)(uintptr_t)sitio, no[j], &a)) {
            printf("%08x: reescrita y no debía\n", no[j]); mal++;
        }
    printf("trampolin_a64 (%s): %u instrucciones, chips y RAM, %u fallos\n",
           __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__ ? "big-endian" : "little-endian", n, mal);
    return mal != 0;
}
