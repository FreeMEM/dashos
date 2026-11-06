/* U2: un "kernel" de prueba para el cargador UEFI. Se enlaza en 0x40200000 (donde lo deja el cargador en QEMU virt),
 * corre con la MMU apagada y escribe en el PL011 de virt dónde está, dónde le llegó el árbol (x0) y cuántas zonas
 * reservadas trae el árbol. Con la MMU apagada todo es memoria de dispositivo: nada de accesos sin alinear
 * (-mstrict-align). */
typedef unsigned long u64;
typedef unsigned int u32;

#define PL011 ((volatile u32 *)0x09000000)

static void byte(char c) {
    while (PL011[0x18 / 4] & (1 << 5)) {}
    PL011[0] = (u32)c;
}

static void texto(const char *s) {
    for (; *s; s++) {
        if (*s == '\n') byte('\r');
        byte(*s);
    }
}

static void hex(u64 v) {
    texto("0x");
    int empezado = 0;
    for (int i = 60; i >= 0; i -= 4) {
        int d = (v >> i) & 0xf;
        if (d || empezado || i == 0) {
            byte("0123456789abcdef"[d]);
            empezado = 1;
        }
    }
}

static u32 be32(const volatile unsigned char *p) {
    return ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | p[3];
}

void principal(u64 arbol, u64 aqui) {
    texto("DashOS: núcleo de prueba en ");
    hex(aqui);
    texto(", árbol en ");
    hex(arbol);
    const volatile unsigned char *f = (const volatile unsigned char *)arbol;
    if (be32(f) != 0xd00dfeed) {
        texto(": sin la magia del árbol\n");
        for (;;) {}
    }
    /* Las reservas: pares (dirección, tamaño) de 64 bits en big-endian, hasta un par a cero. */
    u32 off = be32(f + 16);
    int n = 0;
    for (;; n++) {
        const volatile unsigned char *e = f + off + 16 * n;
        u64 dir = ((u64)be32(e) << 32) | be32(e + 4), tam = ((u64)be32(e + 8) << 32) | be32(e + 12);
        if (dir == 0 && tam == 0) break;
    }
    texto(", ");
    hex(be32(f + 4));
    texto(" bytes, con ");
    byte('0' + n / 10);
    byte('0' + n % 10);
    texto(" zonas reservadas\nDashOS: U2 bien\n");
    for (;;) {
        __asm__ volatile("wfe");
    }
}

/* Entrada: x0 = árbol. Pila de 64 KB por encima de la imagen (el cargador reserva 2 MB más). */
__attribute__((naked, section(".text.entrada"))) void _start(void) {
    __asm__ volatile(
        "adr x1, _start\n"
        "add x2, x1, #0x100000\n"
        "mov sp, x2\n"
        "b principal\n");
}
