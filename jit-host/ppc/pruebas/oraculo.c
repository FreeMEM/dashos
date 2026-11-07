/*
    DashOS, fase F1 de la caja PowerPC: el oráculo. Ejecuta un binario de prueba en qemu-ppc (Linux PPC de 32 bits)
    como lo hace el banco del JIT (../host_ppc.cpp): la memoria en 0x00100000..0x001fffff, el programa en 0x00140000,
    r1 = 0x001ff000, los demás GPR, CR, XER y FPSCR a 0, LR = 0, CTR = la entrada y los FPR a 0x7fffffffffffffff.
    El blr del final salta a la dirección 0: el SIGSEGV que da lo recoge un manejador, que escribe los registros
    (del contexto de la señal) y la suma de la memoria en el formato del banco.
*/
#define _GNU_SOURCE
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <ucontext.h>
#include <unistd.h>
#include <fcntl.h>

#define PPC_RAM      0x00100000UL
#define PPC_RAM_LEN  0x00100000UL
#define PPC_PROGRAMA 0x00140000UL
#define PPC_PILA     0x001ff000UL

static uint64_t suma(const uint8_t *p, size_t n)
{
    uint64_t h = 0xcbf29ce484222325ULL;
    while (n--) { h ^= *p++; h *= 0x100000001b3ULL; }
    return h;
}

/* r2 (el puntero de hilo de glibc) y r13: la prueba entra con todos los GPR a 0 */
static unsigned long r2_guardado, r13_guardado;
static const char *volcado;

static void manejador(int sig, siginfo_t *si, void *ucv)
{
    __asm__ volatile("mr 2,%0\n mr 13,%1" :: "r"(r2_guardado), "r"(r13_guardado));
    ucontext_t *uc = ucv;
    mcontext_t *m = uc->uc_mcontext.uc_regs;
    (void)sig; (void)si;
    /* una llamada nativa (F1f): el opcode 1 con el número de la función, las mismas que ../host_ppc.cpp */
    if (sig == SIGILL && m->gregs[32] != 0 && (*(uint32_t *)m->gregs[32] >> 26) == 1) {
        unsigned long *r = (unsigned long *)m->gregs;
        switch (*(uint32_t *)m->gregs[32] & 0x03ffffff) {
            case 1: r[3] = (uint32_t)(r[3] * 2); break;
            case 2: r[3] = (uint32_t)(r[3] + r[4]); break;
            case 3: { uint32_t t = 0; for (int i = 3; i <= 10; i++) t += (uint32_t)r[i]; r[3] = t; break; }
            default: r[3] = 0xffffffff;
        }
        m->gregs[32] += 4;
        return;
    }
    /* a trap (F1h, tw/twi): the same as the bench's vector 0x700 (../host_ppc.cpp): r20 += 1, r21 = the address of
       the instruction, and go on after it */
    if (sig == SIGTRAP) {
        m->gregs[20] = (uint32_t)(m->gregs[20] + 1);
        m->gregs[21] = m->gregs[32];
        m->gregs[32] += 4;
        return;
    }
    if (m->gregs[32] != 0) {
        printf("[REG] fallo en %08lx (dirección %p)\n", (unsigned long)m->gregs[32], si->si_addr);
        fflush(stdout);
        _exit(3);
    }
    for (int i = 0; i < 32; i++)
        printf("%s r%d=%08lx%s", (i % 8) ? "" : "[REG]", i, (unsigned long)m->gregs[i], (i % 8 == 7) ? "\n" : "");
    printf("[REG] CR=%08lx XER=%08lx CTR=%08lx\n", (unsigned long)m->gregs[38], (unsigned long)m->gregs[37],
           (unsigned long)m->gregs[35]);
    for (int i = 0; i < 32; i++) {
        uint64_t f;
        memcpy(&f, &m->fpregs.fpregs[i], 8);
        printf("%s f%d=%08x%08x%s", (i % 4) ? "" : "[REG]", i, (uint32_t)(f >> 32), (uint32_t)f, (i % 4 == 3) ? "\n" : "");
    }
    uint64_t fpscr;   /* F1e: a double in the context, FPSCR in its low word (converting the value gave 0) */
    memcpy(&fpscr, &m->fpregs.fpscr, 8);
    printf("[REG] FPSCR=%08x\n", (uint32_t)fpscr);
    uint64_t h = suma((const uint8_t *)PPC_RAM, PPC_RAM_LEN);
    printf("[REG] Suma de la memoria: %08x%08x\n", (uint32_t)(h >> 32), (uint32_t)h);
    fflush(stdout);
    if (volcado) {   /* la memoria del PPC, para ver dónde difiere */
        int f = open(volcado, O_WRONLY | O_CREAT | O_TRUNC, 0644);
        if (f >= 0) { write(f, (void *)PPC_RAM, PPC_RAM_LEN); close(f); }
    }
    _exit(0);
}

static const uint64_t nan_fpr = 0x7fffffffffffffffULL;

int main(int argc, char **argv)
{
    if (argc < 2) { fprintf(stderr, "uso: oraculo programa.bin [volcado]\n"); return 1; }
    if (argc > 2) volcado = argv[2];
    if (mmap((void *)PPC_RAM, PPC_RAM_LEN, PROT_READ | PROT_WRITE | PROT_EXEC,
             MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED, -1, 0) != (void *)PPC_RAM) { perror("mmap"); return 2; }
    int fd = open(argv[1], O_RDONLY);
    if (fd < 0) { perror(argv[1]); return 1; }
    ssize_t n = read(fd, (void *)PPC_PROGRAMA, PPC_PILA - 0x1000 - PPC_PROGRAMA);
    close(fd);
    if (n <= 0) return 1;
    for (uintptr_t a = PPC_PROGRAMA; a < PPC_PROGRAMA + (uintptr_t)n; a += 32)
        __asm__ volatile("dcbst 0,%0; sync; icbi 0,%0" :: "r"(a) : "memory");
    __asm__ volatile("sync; isync");

    static uint8_t pila_senal[65536];
    stack_t ss = { .ss_sp = pila_senal, .ss_size = sizeof(pila_senal), .ss_flags = 0 };
    sigaltstack(&ss, NULL);
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_sigaction = manejador;
    sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
    sigaction(SIGSEGV, &sa, NULL);
    sigaction(SIGILL, &sa, NULL);
    sigaction(SIGBUS, &sa, NULL);
    sigaction(SIGTRAP, &sa, NULL);

    __asm__ volatile("mr %0,2\n mr %1,13" : "=r"(r2_guardado), "=r"(r13_guardado));
    printf("[HOST] Programa %s (%d bytes)\n", argv[1], (int)n);
    fflush(stdout);
    /* todos los FPR a la vez desde la tabla, FPSCR a 0, y los GPR a 0 salvo r1; CTR = entrada, LR = 0 */
    __asm__ volatile(
        "mtctr %0\n"
        "lfd 0,0(%1)\n lfd 1,0(%1)\n lfd 2,0(%1)\n lfd 3,0(%1)\n lfd 4,0(%1)\n lfd 5,0(%1)\n lfd 6,0(%1)\n lfd 7,0(%1)\n"
        "lfd 8,0(%1)\n lfd 9,0(%1)\n lfd 10,0(%1)\n lfd 11,0(%1)\n lfd 12,0(%1)\n lfd 13,0(%1)\n lfd 14,0(%1)\n lfd 15,0(%1)\n"
        "lfd 16,0(%1)\n lfd 17,0(%1)\n lfd 18,0(%1)\n lfd 19,0(%1)\n lfd 20,0(%1)\n lfd 21,0(%1)\n lfd 22,0(%1)\n lfd 23,0(%1)\n"
        "lfd 24,0(%1)\n lfd 25,0(%1)\n lfd 26,0(%1)\n lfd 27,0(%1)\n lfd 28,0(%1)\n lfd 29,0(%1)\n lfd 30,0(%1)\n lfd 31,0(%1)\n"
        "mtfsfi 0,0\n mtfsfi 1,0\n mtfsfi 2,0\n mtfsfi 3,0\n mtfsfi 4,0\n mtfsfi 5,0\n mtfsfi 6,0\n mtfsfi 7,0\n"
        "lis 1,0x001f\n ori 1,1,0xf000\n"
        "li 0,0\n mtlr 0\n mtxer 0\n mtcrf 0xff,0\n"
        "li 2,0\n li 3,0\n li 4,0\n li 5,0\n li 6,0\n li 7,0\n li 8,0\n li 9,0\n li 10,0\n li 11,0\n li 12,0\n"
        "li 13,0\n li 14,0\n li 15,0\n li 16,0\n li 17,0\n li 18,0\n li 19,0\n li 20,0\n li 21,0\n li 22,0\n"
        "li 23,0\n li 24,0\n li 25,0\n li 26,0\n li 27,0\n li 28,0\n li 29,0\n li 30,0\n li 31,0\n"
        "bctr\n"
        :: "r"(PPC_PROGRAMA), "b"(&nan_fpr) : "memory");
    return 4;   /* no se llega */
}
