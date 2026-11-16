/* El /init de un dominio de red (S35, paso 2; docs/bitacoras/dominio-red.md): pone para DashOS un aparato virtio-net
 * hecho con la tarjeta que le prestan.
 *
 * Lo que hace, sin biblioteca C (llamadas al sistema directas, como servidor.c), solo en aarch64:
 *
 *  - espera la tarjeta (la e1000 que DashOS le pasa) y el canal con DashOS (el aparato "dashos,canal-dominio", por
 *    UIO: /dev/uio0), levanta la tarjeta y abre un socket de paquetes en ella;
 *  - ofrece por el canal un aparato virtio-net (VERSION_1 y MAC, dos colas) con la MAC de la tarjeta, y dice "listo"
 *    con su reloj y su memoria;
 *  - cuando el conductor de DashOS pone sus colas, proyecta la ventana (las colas y búferes del conductor, que el
 *    hipervisor pone en su memoria) con /dev/mem, y desde ahí pasa tramas: lo que llega por la tarjeta va a un búfer
 *    de la cola de recibir (0) con la cabecera de virtio-net (12 bytes), y lo de la cola de enviar (1) sale por la
 *    tarjeta. Avisa por el canal (USADOS) y late cada medio segundo (LATIDO).
 *
 * Todo lo que lee de la ventana lo comprueba (direcciones, largos, cadenas): el conductor no le hace daño por error.
 * Pero DashOS no se fía de él: el conductor comprueba lo que devuelve.
 *
 * Con "solo=1" (para medir en QEMU sin DashOS, tools/dominio_red.py): no espera el canal; levanta la tarjeta, dice
 * "listo" y se queda quieto.
 *
 * Con "hostil=..." en la línea del kernel (S35, capas 4 y 5; varias, separadas por comas):
 *  - oferta: antes de la buena, una oferta que miente (tipo de disco, MAC de grupo); el servicio la rechaza.
 *  - descriptores: tras unas tramas, devuelve usados que no son del conductor (índices fuera, repetidos, largos
 *    imposibles) y una tormenta de avisos.
 *  - fuera: intenta leer donde no tiene nada (donde DashOS tiene su kernel en la máquina, más allá de la ventana, fuera
 *    de todo), cada intento en un hijo: el hipervisor le da un aborto y el hijo muere con SIGBUS.
 *  - colgarse: tras 30 tramas enviadas, deja de atender y de latir.
 *  - morir: tras 40 tramas enviadas, acaba (el kernel entra en pánico por quedarse sin init y reinicia la máquina).
 */

#include <linux/if.h>
#include <linux/if_ether.h>
#include <linux/if_packet.h>
#include <linux/sockios.h>
#include <asm/unistd.h>

#if !defined(__aarch64__)
#error "solo aarch64"
#endif

typedef unsigned long u64;
typedef long i64;
typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned char u8;

/* ---- llamadas al sistema ---- */

static i64 sc6(i64 n, i64 a, i64 b, i64 c, i64 d, i64 e, i64 f)
{
    register i64 x8 __asm__("x8") = n;
    register i64 x0 __asm__("x0") = a;
    register i64 x1 __asm__("x1") = b;
    register i64 x2 __asm__("x2") = c;
    register i64 x3 __asm__("x3") = d;
    register i64 x4 __asm__("x4") = e;
    register i64 x5 __asm__("x5") = f;
    __asm__ volatile("svc 0" : "+r"(x0) : "r"(x8), "r"(x1), "r"(x2), "r"(x3), "r"(x4), "r"(x5) : "memory");
    return x0;
}
__asm__(".global _start\n_start:\n mov x0, sp\n bl principal\n");

#define sc(n, a, b, c) sc6((n), (i64)(a), (i64)(b), (i64)(c), 0, 0, 0)

#define AT_FDCWD (-100)
#define SOCK_RAW_ 3
#define O_RDONLY 0
#define O_RDWR 2
#define O_NONBLOCK 04000
#define O_DIRECTORY 040000
#define PROT_RW 3
#define MAP_SHARED 1
#define POLLIN 1
#ifndef MSG_DONTWAIT
#define MSG_DONTWAIT 0x40
#endif
#define SIGCHLD 17

static i64 abrir(const char *ruta, int flags) { return sc6(__NR_openat, AT_FDCWD, (i64)ruta, flags, 0, 0, 0); }
static i64 leer(int fd, void *b, u64 n) { return sc(__NR_read, fd, b, n); }
static i64 escribir_fd(int fd, const void *b, u64 n) { return sc(__NR_write, fd, b, n); }
static void cerrar(int fd) { sc(__NR_close, fd, 0, 0); }
static void salir(int c) { for (;;) sc(__NR_exit, c, 0, 0); }
static void dormir_ms(long ms)
{
    struct { i64 s, ns; } t = { ms / 1000, (ms % 1000) * 1000000L };
    sc(__NR_nanosleep, &t, 0, 0);
}
static u64 ahora_ms(void)
{
    struct { i64 s, ns; } t;
    sc(__NR_clock_gettime, 1 /* CLOCK_MONOTONIC */, &t, 0);
    return (u64)t.s * 1000 + (u64)t.ns / 1000000;
}
static void *proyectar(int fd, u64 off, u64 tam)
{
    i64 r = sc6(__NR_mmap, 0, (i64)tam, PROT_RW, MAP_SHARED, fd, (i64)off);
    return (r < 0 && r > -4096) ? 0 : (void *)r;
}

/* ---- texto ---- */

static u64 largo(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static void escribir(const char *s) { escribir_fd(1, s, largo(s)); }
static int empieza(const char *s, const char *p) { while (*p) if (*s++ != *p++) return 0; return 1; }
static int iguales(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static void copiar(char *d, const char *s, u64 cap) { u64 i = 0; while (s[i] && i + 1 < cap) { d[i] = s[i]; i++; } d[i] = 0; }
static void unir(char *d, const char *s, u64 cap) { u64 n = largo(d); copiar(d + n, s, cap - n); }
static void numero(long v)
{
    char b[24]; int i = 23; b[i] = 0;
    if (v == 0) b[--i] = '0';
    int neg = v < 0; if (neg) v = -v;
    while (v) { b[--i] = '0' + v % 10; v /= 10; }
    if (neg) b[--i] = '-';
    escribir(b + i);
}
static void hex(u64 v)
{
    char b[19]; int n = 0; b[n++] = '0'; b[n++] = 'x';
    int i = 60; while (i > 0 && !((v >> i) & 15)) i -= 4;
    for (; i >= 0; i -= 4) { int d = (v >> i) & 15; b[n++] = d < 10 ? '0' + d : 'a' + d - 10; }
    b[n] = 0; escribir(b);
}
/* Contiene `p` como palabra de una lista separada por comas. */
static int en_lista(const char *lista, const char *p)
{
    u64 m = largo(p);
    for (const char *s = lista; *s;) {
        u64 i = 0;
        while (s[i] && s[i] != ',') i++;
        if (i == m) { u64 j = 0; while (j < m && s[j] == p[j]) j++; if (j == m) return 1; }
        s += i; if (*s == ',') s++;
    }
    return 0;
}

static char buf[16384];

/* Un número de /proc/meminfo ("MemTotal:" ...), en KB. */
static long meminfo(const char *clave)
{
    i64 fd = abrir("/proc/meminfo", O_RDONLY);
    if (fd < 0) return -1;
    i64 n = leer(fd, buf, sizeof buf - 1);
    cerrar(fd);
    if (n <= 0) return -1;
    buf[n] = 0;
    u64 m = largo(clave);
    for (i64 i = 0; i + (i64)m < n; i++) {
        if ((i == 0 || buf[i - 1] == '\n') && empieza(buf + i, clave)) {
            long v = 0; i += m;
            while (buf[i] == ' ') i++;
            while (buf[i] >= '0' && buf[i] <= '9') v = v * 10 + (buf[i++] - '0');
            return v;
        }
    }
    return -1;
}

/* La línea del registro del kernel que contiene `clave` (cada read de /dev/kmsg da un mensaje entero), por la consola. */
static void registro(const char *clave)
{
    i64 fd = abrir("/dev/kmsg", O_RDONLY | O_NONBLOCK);
    if (fd < 0) return;
    i64 n;
    u64 m = largo(clave);
    while ((n = leer((int)fd, buf, sizeof buf - 1)) > 0 || n == -32 /* EPIPE: mensajes perdidos */) {
        if (n <= 0) continue;
        for (i64 i = 0; i + (i64)m <= n; i++) {
            u64 j = 0;
            while (j < m && buf[i + j] == clave[j]) j++;
            if (j == m) { escribir("kmsg: "); escribir_fd(1, buf + i, n - i); break; }
        }
    }
    cerrar((int)fd);
}

/* /proc/uptime en milisegundos. */
static long reloj_ms(void)
{
    i64 fd = abrir("/proc/uptime", O_RDONLY);
    if (fd < 0) return -1;
    i64 n = leer(fd, buf, 64);
    cerrar(fd);
    long s = 0, cs = 0; i64 i = 0;
    while (i < n && buf[i] >= '0' && buf[i] <= '9') s = s * 10 + (buf[i++] - '0');
    if (i < n && buf[i] == '.') { i++; for (int k = 0; k < 2 && i < n && buf[i] >= '0' && buf[i] <= '9'; k++) cs = cs * 10 + (buf[i++] - '0'); }
    return s * 1000 + cs * 10;
}

/* ---- el canal con DashOS (caja-linux/vmm/src/dominio/canal.rs) ---- */

#define R_MAGIA 0x000
#define R_EVENTOS 0x008
#define R_AVISADAS 0x00c
#define R_ESTADO 0x010
#define R_VENTANA 0x01c
#define R_VENTANA_TAM 0x024
#define R_COLA_SEL 0x028
#define R_COLA_TAM 0x02c
#define R_COLA_DESC 0x030
#define R_COLA_DISP 0x038
#define R_COLA_USADOS 0x040
#define R_COLA_LISTA 0x048
#define R_USADOS 0x050
#define R_LATIDO 0x054
#define R_OFERTA_TIPO 0x060
#define R_OFERTA_FUNC 0x064
#define R_OFERTA_COLAS 0x06c
#define R_OFERTA_TAM_COLA 0x070
#define R_OFERTA 0x074
#define R_MEDIDA_TOTAL 0x080
#define R_MEDIDA_LIBRE 0x084
#define R_MEDIDA_RELOJ 0x088
#define R_LISTO 0x08c
#define R_CONFIG 0x100
#define MAGIA 0x4d445344u
#define E_AVISO 1
#define E_CAMBIO 2
#define S_LISTO 4 /* DRIVER_OK */
#define TAM_COLA 256
#define TAM_VENTANA (1UL << 20)

static volatile u32 *reg;
static u32 r32(u32 off) { return reg[off / 4]; }
static void w32(u32 off, u32 v) { reg[off / 4] = v; }
static u64 r64(u32 off) { return r32(off) | (u64)r32(off + 4) << 32; }

/* ---- la ventana y las colas (virtio 1.2, §2.7) ---- */

static u8 *ven;        /* proyectada */
static u64 ven_base, ven_tam;
static int mem_fd = -1;

/* Un puntero a [dir, dir + n) de la ventana, o 0 si se sale. */
static void *en_ventana(u64 dir, u64 n)
{
    if (!ven || dir < ven_base || n > ven_tam || dir - ven_base > ven_tam - n) return 0;
    return ven + (dir - ven_base);
}

struct desc { u64 dir; u32 largo; u16 marcas; u16 sig; };
#define D_SIGUE 1
#define D_ESCRIBE 2
#define D_INDIRECTA 4

struct cola {
    int lista;
    u32 tam;
    u64 desc, disp, usados;
    u16 visto;   /* siguiente entrada del anillo del conductor por leer */
    u16 puesto;  /* siguiente entrada del anillo de usados */
};
static struct cola colas[2];

static void barrera(void) { __asm__ volatile("dmb ish" ::: "memory"); }
static void barrera_lectura(void) { __asm__ volatile("dmb ishld" ::: "memory"); }

/* La siguiente cadena que ofrece el conductor, o -1. */
static int siguiente(struct cola *c)
{
    volatile u16 *d = en_ventana(c->disp, 4 + 2 * c->tam);
    if (!d) return -1;
    if (d[1] == c->visto) return -1;
    barrera_lectura();
    int cabeza = d[2 + c->visto % c->tam];
    c->visto++;
    return cabeza;
}

static void devolver(struct cola *c, u32 id, u32 n)
{
    volatile u32 *u = en_ventana(c->usados, 4 + 8 * c->tam);
    if (!u) return;
    volatile u16 *u16p = (volatile u16 *)u;
    u[1 + 2 * (c->puesto % c->tam)] = id;
    u[2 + 2 * (c->puesto % c->tam)] = n;
    barrera();
    c->puesto++;
    u16p[1] = c->puesto;
}

/* Copia sin desalinear (la ventana es memoria normal, pero así vale también si no lo fuera). */
static void copiar_bytes(volatile u8 *d, const volatile u8 *s, u64 n) { for (u64 i = 0; i < n; i++) d[i] = s[i]; }

static long malas; /* cadenas que no se pudieron usar */

/* Recorre la cadena `cabeza`: con `escribir`, escribe `datos` (n bytes) en los descriptores que se escriben; si no, lee
 * de los que se leen a `datos` (hasta `cap`). Devuelve los bytes, o -1 si la cadena no vale. */
static long cadena(struct cola *c, int cabeza, int escribir, u8 *datos, long n_o_cap)
{
    volatile struct desc *t = en_ventana(c->desc, 16UL * c->tam);
    if (!t || cabeza < 0 || (u32)cabeza >= c->tam) return -1;
    long hecho = 0;
    u32 i = cabeza;
    for (u32 vueltas = 0; vueltas < c->tam; vueltas++) {
        struct desc d = { t[i].dir, t[i].largo, t[i].marcas, t[i].sig };
        if (d.marcas & D_INDIRECTA) return -1;
        if (!!(d.marcas & D_ESCRIBE) != escribir) return -1;
        volatile u8 *p = en_ventana(d.dir, d.largo);
        if (!p) return -1;
        long resto = n_o_cap - hecho;
        long k = (long)d.largo < resto ? (long)d.largo : resto;
        if (k > 0) {
            if (escribir) copiar_bytes(p, datos + hecho, k);
            else copiar_bytes(datos + hecho, p, k);
            hecho += k;
        }
        if (!(d.marcas & D_SIGUE)) return (escribir && hecho < n_o_cap) ? -1 : hecho;
        i = d.sig;
        if (i >= c->tam) return -1;
    }
    return -1; /* en bucle */
}

static u8 trama[12 + 2048];
static long recibidas, entregadas, sin_bufer, enviadas;
static int sock = -1;

/* Una trama de la tarjeta, al conductor (cola 0). */
static int entregar(long n)
{
    struct cola *c = &colas[0];
    if (!c->lista) return 0;
    int cab = siguiente(c);
    if (cab < 0) { sin_bufer++; return 0; }
    /* La cabecera de virtio-net (§5.1.6) con VERSION_1: 12 bytes; num_buffers = 1. */
    for (int i = 0; i < 12; i++) trama[i] = 0;
    trama[10] = 1;
    long r = cadena(c, cab, 1, trama, 12 + n);
    if (r < 0) { malas++; devolver(c, cab, 0); return 1; }
    devolver(c, cab, (u32)r);
    entregadas++;
    return 1;
}

/* Lo que el conductor quiere enviar (cola 1), por la tarjeta. */
static int enviar(void)
{
    struct cola *c = &colas[1];
    if (!c->lista) return 0;
    int hubo = 0, cab;
    static u8 sale[12 + 2048];
    while ((cab = siguiente(c)) >= 0) {
        long r = cadena(c, cab, 0, sale, sizeof sale);
        if (r < 12 + 14 || r > 12 + 1514) malas++;
        else if (sc6(__NR_sendto, sock, (i64)(sale + 12), r - 12, 0, 0, 0) == r - 12) enviadas++;
        devolver(c, cab, 0);
        hubo = 1;
    }
    return hubo;
}

static void mirar_colas(void)
{
    u32 estado = r32(R_ESTADO);
    if (!(estado & S_LISTO)) {
        if (colas[0].lista || colas[1].lista) escribir("dominio-red: el conductor reinicia el aparato\n");
        colas[0].lista = colas[1].lista = 0;
        return;
    }
    u64 base = r64(R_VENTANA), tam = r32(R_VENTANA_TAM);
    if (tam && (base != ven_base || tam != ven_tam)) {
        if (ven) sc(__NR_munmap, ven, TAM_VENTANA, 0);
        ven = proyectar(mem_fd, base, TAM_VENTANA);
        ven_base = base; ven_tam = tam > TAM_VENTANA ? TAM_VENTANA : tam;
        escribir("dominio-red: ventana del conductor en "); hex(base); escribir(", "); numero(tam); escribir(" bytes\n");
    }
    for (u32 q = 0; q < 2; q++) {
        w32(R_COLA_SEL, q);
        int lista = r32(R_COLA_LISTA) == 1;
        if (lista && !colas[q].lista) {
            struct cola *c = &colas[q];
            c->tam = r32(R_COLA_TAM);
            c->desc = r64(R_COLA_DESC); c->disp = r64(R_COLA_DISP); c->usados = r64(R_COLA_USADOS);
            c->visto = 0; c->puesto = 0;
            c->lista = c->tam && c->tam <= 32768 && !(c->tam & (c->tam - 1));
            escribir("dominio-red: cola "); numero(q); escribir(" de "); numero(c->tam); escribir(c->lista ? "\n" : ": no vale\n");
        } else if (!lista) {
            colas[q].lista = 0;
        }
    }
}

/* ---- hostil ---- */

static const char *hostil = "";
static int solo;

/* Devuelve usados que no son del conductor, y una tormenta de avisos. */
static void hostil_descriptores(void)
{
    escribir("hostil: usados que no son del conductor y una tormenta de avisos\n");
    for (int q = 0; q < 2; q++) {
        struct cola *c = &colas[q];
        if (!c->lista) continue;
        u32 ids[] = { c->tam + 5, 0xffff, 0x7fffffff, 3, 3, 0 };
        u32 largos[] = { 64, 0xffffffff, 100, 70000, 2, 0xffffffff };
        for (int i = 0; i < 6; i++) devolver(c, ids[i], largos[i]);
        w32(R_USADOS, q);
    }
    for (int i = 0; i < 20000; i++) w32(R_USADOS, i & 3);
}

/* Intenta leer donde no tiene nada, en un hijo cada vez. */
static void hostil_fuera(void)
{
    u64 sitios[] = { 0x40200000UL /* donde DashOS tiene su kernel en la máquina */,
                     ven_base + TAM_VENTANA - 4096 /* la ventana, más allá de lo que dio el conductor */,
                     0xf0000000UL /* fuera de todo */ };
    for (int i = 0; i < 3; i++) {
        i64 h = sc6(__NR_clone, SIGCHLD, 0, 0, 0, 0, 0);
        if (h == 0) {
            volatile u32 *p = proyectar(mem_fd, sitios[i] & ~4095UL, 4096);
            if (!p) salir(2);
            u32 v = p[(sitios[i] & 4095) / 4];
            p[0] = v ^ 0x5a5a5a5a;
            salir(0); /* no debería llegar */
        }
        int estado = 0;
        sc6(__NR_wait4, h, (i64)&estado, 0, 0, 0, 0);
        escribir("hostil: leer en "); hex(sitios[i]);
        if ((estado & 0x7f) == 7) escribir(": el hijo muere con SIGBUS\n");
        else if ((estado & 0x7f) == 0) { escribir(": el hijo acaba con "); numero((estado >> 8) & 0xff); escribir("\n"); }
        else { escribir(": el hijo muere con la señal "); numero(estado & 0x7f); escribir("\n"); }
    }
}

/* ---- la tarjeta ---- */

struct dirent64_ { u64 ino; i64 off; unsigned short reclen; unsigned char tipo; char nombre[]; };

static int controlador_e1000(const char *intf)
{
    char ruta[128], destino[256];
    copiar(ruta, "/sys/class/net/", sizeof ruta); unir(ruta, intf, sizeof ruta); unir(ruta, "/device/driver", sizeof ruta);
    i64 n = sc6(__NR_readlinkat, AT_FDCWD, (i64)ruta, (i64)destino, sizeof destino - 1, 0, 0);
    if (n <= 0) return 0;
    destino[n] = 0;
    char *ultimo = destino;
    for (char *p = destino; *p; p++) if (*p == '/') ultimo = p + 1;
    return iguales(ultimo, "e1000") || iguales(ultimo, "e1000e");
}

static int levantar(const char *intf, u8 mac[6])
{
    i64 s = sc(__NR_socket, AF_PACKET, SOCK_RAW_, 0);
    if (s < 0) return (int)s;
    struct ifreq r;
    for (u64 i = 0; i < sizeof r; i++) ((char *)&r)[i] = 0;
    copiar(r.ifr_name, intf, IFNAMSIZ);
    i64 e = sc(__NR_ioctl, s, SIOCGIFFLAGS, &r);
    if (e == 0) { r.ifr_flags |= IFF_UP; e = sc(__NR_ioctl, s, SIOCSIFFLAGS, &r); }
    if (e == 0) e = sc(__NR_ioctl, s, SIOCGIFHWADDR, &r);
    if (e == 0) for (int i = 0; i < 6; i++) mac[i] = (u8)r.ifr_hwaddr.sa_data[i];
    if (e == 0) e = sc(__NR_ioctl, s, SIOCGIFINDEX, &r);
    if (e != 0) { cerrar(s); return (int)e; }
    struct sockaddr_ll ll;
    for (u64 i = 0; i < sizeof ll; i++) ((char *)&ll)[i] = 0;
    ll.sll_family = AF_PACKET;
    ll.sll_protocol = (unsigned short)((ETH_P_ALL >> 8) | ((ETH_P_ALL & 0xff) << 8));
    ll.sll_ifindex = r.ifr_ifindex;
    e = sc(__NR_bind, s, &ll, sizeof ll);
    if (e != 0) { cerrar(s); return (int)e; }
    return (int)s;
}

/* ---- la oferta ---- */

static int ofrecer(u32 tipo, const u8 mac[6])
{
    w32(R_OFERTA_TIPO, tipo);
    w32(R_OFERTA_FUNC, 1u << 5);    /* MAC */
    w32(R_OFERTA_FUNC + 4, 1);      /* VERSION_1 (la 32) */
    w32(R_OFERTA_COLAS, 2);
    w32(R_OFERTA_TAM_COLA, TAM_COLA);
    w32(R_CONFIG, mac[0] | mac[1] << 8 | mac[2] << 16 | (u32)mac[3] << 24);
    w32(R_CONFIG + 4, mac[4] | mac[5] << 8);
    w32(R_OFERTA, 1);
    return (int)r32(R_OFERTA);
}

/* ---- principal ---- */

void principal(long *sp)
{
    long argc = sp[0];
    char **envp = (char **)(sp + 1 + argc + 1);
    for (char **e = envp; *e; e++) {
        if (empieza(*e, "hostil=")) hostil = *e + 7;
        if (iguales(*e, "solo=1")) solo = 1;
    }

    sc6(__NR_mount, (i64)"devtmpfs", (i64)"/dev", (i64)"devtmpfs", 0, 0, 0);
    sc6(__NR_mount, (i64)"proc", (i64)"/proc", (i64)"proc", 0, 0, 0);
    sc6(__NR_mount, (i64)"sysfs", (i64)"/sys", (i64)"sysfs", 0, 0, 0);
    i64 c = abrir("/dev/console", O_RDWR);
    if (c >= 0 && c != 1) { sc(__NR_dup3, c, 1, 0); sc(__NR_dup3, c, 2, 0); }
    escribir("dominio-red: en marcha"); if (hostil[0]) { escribir(", hostil="); escribir(hostil); } escribir("\n");

    /* La tarjeta y el canal aparecen cuando sus controladores acaban de sondear: se miran cada 5 ms, 20 s como mucho. */
    char cable[IFNAMSIZ] = "";
    i64 uio = -1;
    for (int intento = 0; intento < 4000 && (!cable[0] || (uio < 0 && !solo)); intento++) {
        if (uio < 0 && !solo) uio = abrir("/dev/uio0", O_RDWR);
        i64 d = abrir("/sys/class/net", O_RDONLY | O_DIRECTORY);
        if (d >= 0 && !cable[0]) {
            i64 n;
            while ((n = sc(__NR_getdents64, d, buf, sizeof buf)) > 0)
                for (i64 p = 0; p < n;) {
                    struct dirent64_ *de = (struct dirent64_ *)(buf + p);
                    p += de->reclen;
                    if (de->nombre[0] != '.' && controlador_e1000(de->nombre)) copiar(cable, de->nombre, IFNAMSIZ);
                }
        }
        if (d >= 0) cerrar(d);
        if (!cable[0] || (uio < 0 && !solo)) dormir_ms(5);
    }
    if (!cable[0]) { escribir("fallo: sin tarjeta\n"); for (;;) dormir_ms(60000); }
    if (solo) {
        u8 mac[6];
        sock = levantar(cable, mac);
        long total = meminfo("MemTotal:"), libre = meminfo("MemFree:"), reloj = reloj_ms();
        escribir(sock < 0 ? "fallo: no se levanta la tarjeta\n" : "listo (solo) reloj_ms=");
        numero(reloj); escribir(" usada_kb="); numero(total - libre); escribir(" total_kb="); numero(total); escribir("\n");
        registro("Memory:");
        for (;;) dormir_ms(60000);
    }
    if (uio < 0) { escribir("fallo: sin canal con DashOS (/dev/uio0)\n"); for (;;) dormir_ms(60000); }
    reg = proyectar((int)uio, 0, 4096);
    mem_fd = (int)abrir("/dev/mem", O_RDWR);
    if (!reg || mem_fd < 0 || r32(R_MAGIA) != MAGIA) { escribir("fallo: el canal no contesta\n"); for (;;) dormir_ms(60000); }
    u8 mac[6];
    sock = levantar(cable, mac);
    if (sock < 0) { escribir("fallo: no se levanta "); escribir(cable); escribir("\n"); for (;;) dormir_ms(60000); }

    if (en_lista(hostil, "oferta")) {
        u8 grupo[6] = { 0x01, 0x00, 0x5e, 0x00, 0x00, 0x01 };
        int r = ofrecer(2, grupo);
        escribir("hostil: oferta de un disco con MAC de grupo: "); escribir(r == 2 ? "rechazada\n" : "¡aceptada!\n");
    }
    int r = ofrecer(1, mac);
    if (r != 1) { escribir("fallo: el servicio rechaza la oferta\n"); for (;;) dormir_ms(60000); }

    long total = meminfo("MemTotal:"), libre = meminfo("MemFree:"), reloj = reloj_ms();
    w32(R_MEDIDA_TOTAL, (u32)total); w32(R_MEDIDA_LIBRE, (u32)libre); w32(R_MEDIDA_RELOJ, (u32)reloj); w32(R_LISTO, 1);
    escribir("listo tarjeta="); escribir(cable);
    escribir(" mac="); for (int i = 0; i < 6; i++) { const char *h = "0123456789abcdef"; char x[4] = { h[mac[i] >> 4], h[mac[i] & 15], i < 5 ? ':' : 0, 0 }; escribir(x); }
    escribir(" reloj_ms="); numero(reloj); escribir(" usada_kb="); numero(total - libre); escribir(" total_kb="); numero(total); escribir("\n");
    registro("Memory:");

    u64 ultimo_latido = 0;
    int hecho_descriptores = 0, hecho_fuera = 0;
    for (;;) {
        struct { int fd; short ev, rev; } p[2] = { { (int)uio, POLLIN, 0 }, { sock, POLLIN, 0 } };
        struct { i64 s, ns; } plazo = { 0, 250 * 1000000L };
        sc6(__NR_ppoll, (i64)p, 2, (i64)&plazo, 0, 8, 0);
        u64 t = ahora_ms();
        if (t - ultimo_latido >= 500) { w32(R_LATIDO, 1); ultimo_latido = t; }
        if (p[0].rev & POLLIN) {
            u32 cuenta;
            leer((int)uio, &cuenta, 4);
            u32 ev = r32(R_EVENTOS);
            if (ev & E_AVISO) r32(R_AVISADAS);
            if (ev & E_CAMBIO) mirar_colas();
            u32 uno = 1;
            escribir_fd((int)uio, &uno, 4); /* la interrupción, otra vez habilitada */
        }
        if (enviar()) w32(R_USADOS, 1);
        int nuevas = 0;
        for (int k = 0; k < 64; k++) {
            struct sockaddr_ll de; u32 tam_de = sizeof de;
            i64 n = sc6(__NR_recvfrom, sock, (i64)(trama + 12), 2048, MSG_DONTWAIT, (i64)&de, (i64)&tam_de);
            if (n <= 0) break;
            if (de.sll_pkttype == 4 /* PACKET_OUTGOING */) continue;
            recibidas++;
            if (n >= 14 && n <= 1514) nuevas += entregar(n);
        }
        if (nuevas) w32(R_USADOS, 0);

        if (en_lista(hostil, "descriptores") && !hecho_descriptores && entregadas >= 8 && enviadas >= 4) {
            hecho_descriptores = 1;
            hostil_descriptores();
        }
        if (en_lista(hostil, "fuera") && !hecho_fuera && ven) {
            hecho_fuera = 1;
            hostil_fuera();
        }
        if (en_lista(hostil, "colgarse") && enviadas >= 30) {
            escribir("hostil: me cuelgo (sin atender ni latir)\n");
            for (;;) dormir_ms(60000);
        }
        if (en_lista(hostil, "morir") && enviadas >= 40) {
            escribir("hostil: me muero a media transferencia\n");
            salir(1);
        }
    }
}
