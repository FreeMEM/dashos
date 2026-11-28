/*
    DashOS, caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L1): la página 0 del 68k sin proyectarla en la dirección 0.

    Linux no deja proyectar nada por debajo de vm.mmap_min_addr (64 KB de serie) sin ser root, y el 68k lee ExecBase en
    la dirección 4. La página 0 (4 KB, de solo lectura, como en la caja de DashOS) vive en PAGINA0_COPIA. El JIT desvía
    allí al traducir las lecturas con dirección absoluta (`move.l 4.w,a6`; DASHOS_PAGINA0 en M68k_EA.c). Las que llegan
    por un registro (`move.l (a0),d0` con A0 = 4, un puntero nulo) dan SIGSEGV: este manejador decodifica la instrucción
    de A64 (decodifica() de caja/fallo_a64_decodifica.h), hace la lectura sobre la copia y sigue en la instrucción siguiente. Una
    escritura en la página 0, o un acceso al resto de los primeros 64 KB, es un fallo del programa, como en DashOS.
*/
#include <stdint.h>
#include "fallo_a64_decodifica.h"

/* Los bytes tal como los vería la instrucción (como el `trozo` de caja/fallo_a64.h, que aquí no hace falta entero). */
typedef union { uint8_t b[8]; uint16_t h; uint32_t w; uint64_t d; } trozo;

static inline uint64_t trozo_cargar(const trozo *t, unsigned tam)
{
    return tam == 1 ? t->b[0] : tam == 2 ? t->h : tam == 4 ? t->w : t->d;
}

#define SYS_rt_sigaction 134
#define SIGSEGV 11
#define SA_SIGINFO   0x00000004UL
#define SA_RESTORER  0x04000000UL
#define SA_NODEFER   0x40000000UL

/* Va incluido en host.c (usa sus sys6, kprintf y flush_out). */

uint64_t pagina0_copia;          /* dónde está la copia de la página 0 (la ve el 68k, de solo lectura) */
uint64_t pagina0_fallos;         /* cuántas lecturas ha imitado el manejador */
int pagina0_fatal;               /* 1: el último fallo no era de la página 0, y se acaba */

/* Lo que el kernel deja en la pila al entregar la señal (arch/arm64/include/uapi/asm/ucontext.h y sigcontext.h): la
   struct sigcontext empieza en el byte 176 de la ucontext. */
struct sigcontexto {
    uint64_t fault_address;
    uint64_t regs[31];
    uint64_t sp, pc, pstate;
};
#define UC_MCONTEXT 176

/* El regreso de la señal (rt_sigreturn), sin prólogo que mueva sp: en ensamblador suelto. */
void pagina0_restaura(void);
__asm__(".text\n.balign 4\npagina0_restaura:\n mov x8, #139\n svc #0\n");

static void manejador(int sig, void *info, void *uc)
{
    (void)sig;
    struct sigcontexto *sc = (struct sigcontexto *)((char *)uc + UC_MCONTEXT);
    uint64_t far = *(uint64_t *)((char *)info + 16);   /* si_addr */
    marco_fallo m;
    for (int i = 0; i < 31; i++) m.x[i] = sc->regs[i];
    m.sp = sc->sp; m.pc = sc->pc; m.pstate = sc->pstate; m.esr = 0; m.far = far;

    uint32_t ins = *(volatile uint32_t *)m.pc;
#ifdef __AARCH64EB__
    ins = __builtin_bswap32(ins);                     /* las instrucciones de A64 van siempre en little-endian */
#endif
    struct acceso a;
    if (far < 0x1000 && decodifica(ins, &m, &a) && !a.escribe && a.dir + a.tam * a.pares <= 0x1000 && far >= a.dir) {
        const uint8_t *p0 = (const uint8_t *)pagina0_copia;
        unsigned regs[2] = { a.rt, a.rt2 };
        for (int k = 0; k < a.pares; k++) {
            trozo t = { .d = 0 };
            for (unsigned b = 0; b < a.tam; b++) t.b[b] = p0[a.dir + k * a.tam + b];
            uint64_t v = trozo_cargar(&t, a.tam);
            if (a.signo) v = (uint64_t)extiende(v, 8 * a.tam);
            if (regs[k] != 31) sc->regs[regs[k]] = a.x ? v : (uint32_t)v;
        }
        if (a.actualiza) {
            if (a.rn == 31) sc->sp = a.base_nueva;
            else sc->regs[a.rn] = a.base_nueva;
        }
        if (a.estado >= 0 && a.estado != 31) sc->regs[a.estado] = 0;
        sc->pc += 4;
        pagina0_fallos++;
        return;
    }
    kprintf("[PAGINA0] fallo del programa en $%x (instrucción %08x en %p): %s\n", (unsigned)far, ins, (void *)m.pc,
            far < 0x1000 ? "escritura en la página 0" : "fuera de la memoria del 68k");
    flush_out();
    pagina0_fatal = 1;
    sys6(93 /* exit */, 3, 0, 0, 0, 0, 0);
}

/* Pone el manejador de SIGSEGV. 0 si fue bien. */
long pagina0_instala(uint64_t copia)
{
    pagina0_copia = copia;
    struct { void *h; unsigned long flags; void *restorer; uint64_t mask; } sa = {
        (void *)manejador, SA_SIGINFO | SA_RESTORER | SA_NODEFER, (void *)pagina0_restaura, 0 };
    return sys6(SYS_rt_sigaction, SIGSEGV, (long)&sa, 0, 8, 0, 0);
}
