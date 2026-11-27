/*
    K6 (docs/CHIPS_LOOP.md): el decodificador de ../fallo_a64.h contra la CPU, en Linux con qemu-user, en big-endian y
    en little-endian (`make -C caja/pruebas_fallo`; la lanza también `make -C kernel prueba`). Cada instrucción de la
    tabla se ejecuta de verdad sobre una memoria en $BF0000 y se imita con chips_atiende sobre una sombra con los mismos
    bytes, que caja_chip_leer y caja_chip_escribir leen y escriben en el orden del 68k (el byte de la dirección menor,
    el más alto). Los registros (salvo x6, el de la carga exclusiva que va antes de cada guardado exclusivo) y la
    memoria tienen que acabar iguales, con registros y memoria al azar, 20 vueltas. Al final, unas que no se
    decodifican: PRFM, LDR de la FPU, una atómica y LDAR.
*/
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/mman.h>

#define BASE 0xBF0000u
static uint8_t sombra[0x10000];
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
#include "../fallo_a64.h"

extern const uint8_t plantilla[], plantilla_fin[], tabla[], tabla_fin[];
__asm__(".text\n.balign 4\nplantilla:\n"
        "ldp x1, x2, [x0, #8]\n ldp x3, x4, [x0, #24]\n ldp x5, x6, [x0, #40]\n ldp x7, x8, [x0, #56]\n"
        "nop\n nop\n"
        "stp x1, x2, [x0, #8]\n stp x3, x4, [x0, #24]\n stp x5, x6, [x0, #40]\n stp x7, x8, [x0, #56]\n ret\n"
        "plantilla_fin:\n"
        ".section .rodata\n.balign 4\ntabla:\n"
        "ldr w1,[x3,#4]\n ldr x1,[x3,#8]\n ldrh w1,[x3,#2]\n ldrb w1,[x3,#1]\n ldrsh w1,[x3,#2]\n ldrsh x1,[x3,#2]\n"
        "ldrsb w1,[x3,#3]\n ldrsb x1,[x3]\n ldrsw x1,[x3,#4]\n"
        "ldur w1,[x3,#-3]\n ldurh w1,[x3,#-1]\n ldursh w1,[x3,#5]\n ldursh x1,[x3,#5]\n ldur x1,[x3,#7]\n ldursb w1,[x3,#-7]\n ldurb w1,[x3,#9]\n"
        "ldr w1,[x3,#4]!\n ldr w1,[x3],#-4\n ldrh w1,[x3,#-2]!\n ldrb w1,[x3],#1\n ldrsh w1,[x3,#2]!\n ldrsb x1,[x3],#-1\n"
        "ldrsh w1,[x3],#2\n ldr x1,[x3,#-8]!\n ldrsb w1,[x3,#1]!\n ldrh w1,[x3],#2\n"
        "ldr w1,[x3,x4]\n ldr w1,[x3,w7,uxtw]\n ldr w1,[x3,w8,sxtw #2]\n ldrh w1,[x3,x4,lsl #1]\n ldrb w1,[x3,x4]\n"
        "ldrsh w1,[x3,x4]\n ldr x1,[x3,x4,lsl #3]\n ldrsh x1,[x3,w8,sxtw]\n ldrb w1,[x3,w7,uxtw]\n"
        "str w1,[x3,#4]\n str x1,[x3,#8]\n strh w1,[x3,#2]\n strb w1,[x3,#1]\n stur w1,[x3,#-3]\n sturh w1,[x3,#1]\n"
        "sturb w1,[x3,#-1]\n stur x1,[x3,#-5]\n str w1,[x3,#-4]!\n str w1,[x3],#4\n strh w1,[x3],#2\n strb w1,[x3,#-1]!\n"
        "strh w1,[x3,#-2]!\n str x1,[x3],#-8\n strb w1,[x3],#1\n"
        "str w1,[x3,x4]\n strh w1,[x3,w7,uxtw #1]\n strb w1,[x3,x4]\n str x1,[x3,w8,sxtw]\n"
        "str wzr,[x3]\n strh wzr,[x3,#2]\n strb wzr,[x3,#3]\n"
        "ldp w1,w2,[x3,#8]\n ldp w1,w2,[x3,#-8]!\n ldp w1,w2,[x3],#8\n ldp x1,x2,[x3,#16]\n ldp x1,x2,[x3],#16\n"
        "ldp x1,x2,[x3,#-16]!\n ldpsw x1,x2,[x3,#8]\n ldnp w1,w2,[x3,#4]\n"
        "stp w1,w2,[x3,#8]\n stp w1,w2,[x3,#-8]!\n stp w1,w2,[x3],#8\n stp x1,x2,[x3,#-16]!\n stp x1,x2,[x3],#16\n"
        "stp x1,x2,[x3,#24]\n stp wzr,w2,[x3]\n"
        "ldxr w1,[x3]\n ldxrb w1,[x3]\n ldxrh w1,[x3]\n ldaxr x1,[x3]\n ldaxrh w1,[x3]\n"
        "stxr w5,w1,[x3]\n stlxr w5,w1,[x3]\n stlxrb w5,w1,[x3]\n stlxrh w5,w1,[x3]\n stxr w5,x1,[x3]\n stxrh w5,w1,[x3]\n"
        "tabla_fin:\n.text\n");

static uint64_t azar(void) { return ((uint64_t)rand() << 40) ^ ((uint64_t)rand() << 20) ^ (uint64_t)rand(); }
static uint32_t lee_ins(const uint8_t *p) { return p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24; }

int main(void)
{
    uint8_t *real = mmap((void *)(uintptr_t)BASE, 0x10000, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0);
    uint8_t *cod = mmap(0, 4096, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (real != (uint8_t *)(uintptr_t)BASE || cod == MAP_FAILED) { printf("mmap\n"); return 2; }
    unsigned n = (unsigned)(tabla_fin - tabla) / 4, mal = 0;
    for (int vuelta = 0; vuelta < 20; vuelta++)
    for (unsigned k = 0; k < n; k++) {
        uint32_t ins = lee_ins(tabla + 4 * k);
        for (unsigned j = 0; j < 0x10000; j++) real[j] = sombra[j] = (uint8_t)rand();
        uint64_t r[9];
        for (int j = 0; j < 9; j++) r[j] = azar();
        r[3] = BASE + 0x100; r[4] = 4; r[7] = 0xFFFFFFFF00000006ull; r[8] = 0x12345678FFFFFFF8ull;
        marco_fallo m; memset(&m, 0, sizeof m);
        for (int j = 1; j <= 8; j++) m.x[j] = r[j];
        /* la de verdad; antes de un guardado exclusivo, la carga exclusiva de su tamaño en x6 */
        memcpy(cod, plantilla, plantilla_fin - plantilla);
        uint32_t antes = 0xd503201f;
        if ((ins & 0x3fe07c00) == 0x08007c00) antes = 0x085f7c00 | (ins & 0xc0000000) | (3 << 5) | 6;
        memcpy(cod + 16, &(uint8_t[4]){ antes, antes >> 8, antes >> 16, antes >> 24 }, 4);
        memcpy(cod + 20, tabla + 4 * k, 4);
        __builtin___clear_cache((char *)cod, (char *)cod + 4096);
        ((void (*)(uint64_t *))cod)(r);
        struct acceso a;
        if (!decodifica(ins, &m, &a)) { printf("%08x: no se decodifica\n", ins); mal++; continue; }
        m.far = a.dir;
        int e = chips_atiende(&m, ins, 0, &a);
        int bien = e == 1 && memcmp(real, sombra, 0x10000) == 0 && m.pc == 4;
        for (int j = 1; j <= 8; j++) if (j != 6 && m.x[j] != r[j]) bien = 0;
        if (!bien) {
            mal++;
            printf("%08x: MAL (%d)", ins, e);
            for (int j = 1; j <= 8; j++) if (j != 6 && m.x[j] != r[j]) printf(" x%d %016llx/%016llx", j, (unsigned long long)m.x[j], (unsigned long long)r[j]);
            if (memcmp(real, sombra, 0x10000)) printf(" memoria");
            printf("\n");
        }
    }
    /* lo que no se decodifica */
    marco_fallo m; memset(&m, 0, sizeof m); struct acceso a;
    uint32_t no[] = { 0xf9800060 /* prfm */, 0xfd400061 /* ldr d1,[x3] */, 0xb8a40061 /* ldadd */, 0xc8dffc61 /* ldar */ };
    for (unsigned j = 0; j < sizeof no / 4; j++) if (decodifica(no[j], &m, &a)) { printf("%08x: decodificada y no debía\n", no[j]); mal++; }
    printf("fallo_a64 (%s): %u instrucciones, %u fallos\n", __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__ ? "big-endian" : "little-endian", n, mal);
    return mal != 0;
}
