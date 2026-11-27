/* El /init de un dominio de red (S35, paso 2; docs/bitacoras/dominio-red.md y dominio-usb.md): pone para DashOS un
 * aparato virtio-net hecho con la tarjeta que le prestan.
 *
 * Lo que hace, sin biblioteca C (llamadas al sistema directas, como servidor.c), solo en aarch64:
 *
 *  - espera el canal con DashOS (el aparato "dashos,canal-dominio", por UIO: /dev/uio0);
 *  - si DashOS le presta un aparato USB (USB_DEVID en el canal), lo engancha en vhci-hcd (el anfitrión virtual de
 *    USB/IP de Linux): crea un par de sockets, da uno a vhci-hcd por sysfs ("attach") y pasa los bytes del otro al tubo
 *    (la memoria compartida con DashOS, por /dev/mem), en los dos sentidos. Linux lleva el aparato con su controlador
 *    (cdc_ether, el adaptador de red USB). Si DashOS se lo quita (se desenchufó), cierra el socket y vhci-hcd lo suelta;
 *    si vuelve, lo engancha otra vez;
 *  - espera la tarjeta (la que salga en /sys/class/net, que no sea lo), la levanta y abre un socket de paquetes en ella;
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
 * Con "solo=1" (para medir en QEMU sin DashOS, tools/dominio_red.py, con la e1000): no espera el canal; levanta la
 * tarjeta, dice "listo" y se queda quieto.
 *
 * Con "hostil=..." en la línea del kernel (S35, capas 4 y 5; varias, separadas por comas):
 *  - oferta: antes de la buena, una oferta que miente (tipo de disco, MAC de grupo); el servicio la rechaza.
 *  - descriptores: tras unas tramas, devuelve usados que no son del conductor (índices fuera, repetidos, largos
 *    imposibles) y una tormenta de avisos.
 *  - fuera: intenta leer donde no tiene nada (más allá de la ventana, más allá del tubo, donde estuvo su RAM con la
 *    e1000, fuera de todo), cada intento en un hijo: el hipervisor le da un aborto y el hijo muere con SIGBUS.
 * Y los del USB (S35, capas 4 y 5), que escriben USB/IP a mano en el tubo antes de enganchar vhci-hcd:
 *  - ajeno: peticiones para aparatos que no se le prestaron (otro devid): ENODEV, y no llegan al xHCI.
 *  - usbmalo: peticiones falsas o enormes (largos imposibles, una preparación que no cuadra, isócronas, un punto que no
 *    existe, SET_ADDRESS, una configuración que no existe, SET_DESCRIPTOR con basura, cancelar lo que no existe).
 *  - tormenta: miles de peticiones de control y de interrupción, y cancelarlas.
 *  - basura: una orden de USB/IP que no existe: el servicio da el dominio por roto y lo relanza.
 *  - caer: acaba nada más empezar (para las caídas seguidas y el aviso al usuario).
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
#define AF_UNIX_ 1
#define SOCK_STREAM_ 1
#define POLLOUT 4

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
#define R_TUBO 0x05c
#define R_TUBO_DIR 0x090
#define R_TUBO_TAM 0x098
#define R_USB_DEVID 0x09c
#define R_USB_VELOCIDAD 0x0a0
#define R_USB_CUENTA 0x0a4
#define MAGIA 0x4d445344u
#define E_AVISO 1
#define E_CAMBIO 2
#define E_TUBO 4
#define E_USB 8
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
static volatile u8 *tubo;      /* el tubo de USB/IP, proyectado (si hay) */
static u64 tubo_tam;
static int uio_fd = -1;

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
        else if (sock >= 0 && sc6(__NR_sendto, sock, (i64)(sale + 12), r - 12, 0, 0, 0) == r - 12) enviadas++;
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
    u64 sitios[] = { ven_base + TAM_VENTANA - 4096 /* la ventana, más allá de lo que dio el conductor */,
                     ven_base + TAM_VENTANA + tubo_tam + 4096 /* más allá del tubo */,
                     0x80000000UL /* donde estuvo su RAM con la e1000 */,
                     0xf0000000UL /* fuera de todo */ };
    for (int i = 0; i < 4; i++) {
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

/* El controlador de una interfaz ("e1000", "cdc_ether"...) en `nombre`; 0 si no tiene (lo). */
static int controlador(const char *intf, char *nombre, u64 cap)
{
    char ruta[128], destino[256];
    copiar(ruta, "/sys/class/net/", sizeof ruta); unir(ruta, intf, sizeof ruta); unir(ruta, "/device/driver", sizeof ruta);
    i64 n = sc6(__NR_readlinkat, AT_FDCWD, (i64)ruta, (i64)destino, sizeof destino - 1, 0, 0);
    if (n <= 0) return 0;
    destino[n] = 0;
    char *ultimo = destino;
    for (char *p = destino; *p; p++) if (*p == '/') ultimo = p + 1;
    copiar(nombre, ultimo, cap);
    return 1;
}

/* La primera interfaz con controlador, en `cable` (y su controlador en `cual`). */
static int buscar_tarjeta(char *cable, char *cual)
{
    i64 d = abrir("/sys/class/net", O_RDONLY | O_DIRECTORY);
    if (d < 0) return 0;
    i64 n;
    cable[0] = 0;
    while (!cable[0] && (n = sc(__NR_getdents64, d, buf, sizeof buf)) > 0)
        for (i64 p = 0; p < n && !cable[0];) {
            struct dirent64_ *de = (struct dirent64_ *)(buf + p);
            p += de->reclen;
            if (de->nombre[0] != '.' && controlador(de->nombre, cual, 32)) copiar(cable, de->nombre, IFNAMSIZ);
        }
    cerrar((int)d);
    return cable[0] != 0;
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

/* ---- el aparato USB prestado: USB/IP por el tubo (caja-linux/vmm/src/dominio/usbip.rs) ---- */

#define TUBO_CABECERA 4096u
#define TAM_ANILLO (128u * 1024)
#define T_CUENTA 0x00
#define T_ABIERTO 0x04
#define T_IDA_ESCRITO 0x08
#define T_IDA_LEIDO 0x0c
#define T_VUELTA_ESCRITO 0x10
#define T_VUELTA_LEIDO 0x14

static u32 usb_cuenta;         /* el préstamo que hay enganchado (0: ninguno) */
static int usock = -1;         /* nuestro lado del par de sockets; el otro lo tiene vhci-hcd */
static u32 ida_esc, vuelta_lei;
static long usb_bytes_ida, usb_bytes_vuelta;

static u32 t32(u32 off) { return *(volatile u32 *)(tubo + off); }
static void pt32(u32 off, u32 v) { *(volatile u32 *)(tubo + off) = v; }

static void anillo_escribir(u32 base, u32 pos, const u8 *d, u32 n)
{
    for (u32 i = 0; i < n; i++) tubo[base + (pos + i) % TAM_ANILLO] = d[i];
}
static void anillo_leer(u32 base, u32 pos, u8 *d, u32 n)
{
    for (u32 i = 0; i < n; i++) d[i] = tubo[base + (pos + i) % TAM_ANILLO];
}

/* Lo que cabe en la ida y lo que hay en la vuelta. */
static u32 ida_libre(void) { u32 o = ida_esc - t32(T_IDA_LEIDO); return o > TAM_ANILLO ? 0 : TAM_ANILLO - o; }
static u32 vuelta_hay(void) { u32 h = t32(T_VUELTA_ESCRITO) - vuelta_lei; barrera_lectura(); return h > TAM_ANILLO ? 0 : h; }

static void ida_poner(const u8 *d, u32 n)
{
    anillo_escribir(TUBO_CABECERA, ida_esc, d, n);
    barrera();
    ida_esc += n;
    pt32(T_IDA_ESCRITO, ida_esc);
}

/* Pasa bytes entre el socket de vhci-hcd y el tubo, en los dos sentidos, mientras se pueda. Devuelve si se movió algo
 * (y entonces avisa a DashOS). Si vhci-hcd cerró su lado, cierra el nuestro. */
static int bombear(void)
{
    if (usock < 0 || !tubo) return 0;
    static u8 tmp[16384];
    int movido = 0;
    for (;;) {
        u32 libre = ida_libre();
        if (!libre) break;
        i64 n = sc6(__NR_recvfrom, usock, (i64)tmp, libre < sizeof tmp ? libre : sizeof tmp, MSG_DONTWAIT, 0, 0);
        if (n == 0) { escribir("dominio-red: vhci-hcd soltó el aparato\n"); cerrar(usock); usock = -1; break; }
        if (n < 0) break;
        ida_poner(tmp, (u32)n);
        usb_bytes_ida += n;
        movido = 1;
    }
    for (;;) {
        u32 hay = usock >= 0 ? vuelta_hay() : 0;
        if (!hay) break;
        u32 pos = vuelta_lei % TAM_ANILLO, seguidos = TAM_ANILLO - pos;
        u32 k = hay < seguidos ? hay : seguidos;
        if (k > sizeof tmp) k = sizeof tmp;
        anillo_leer(TUBO_CABECERA + TAM_ANILLO, vuelta_lei, tmp, k);
        i64 n = sc6(__NR_sendto, usock, (i64)tmp, k, MSG_DONTWAIT, 0, 0);
        if (n <= 0) break;
        vuelta_lei += (u32)n;
        pt32(T_VUELTA_LEIDO, vuelta_lei);
        usb_bytes_vuelta += n;
        movido = 1;
    }
    if (movido) w32(R_TUBO, 1);
    return movido;
}

static void numero_en(char *d, u64 cap, u64 v)
{
    char b[24]; int i = 23; b[i] = 0;
    if (v == 0) b[--i] = '0';
    while (v) { b[--i] = '0' + v % 10; v /= 10; }
    unir(d, b + i, cap);
}

/* ---- hostil, del lado del USB: USB/IP a mano en el tubo ---- */

static void be(u8 *b, u32 v) { b[0] = v >> 24; b[1] = v >> 16; b[2] = v >> 8; b[3] = v; }
static u32 lee_be(const u8 *b) { return (u32)b[0] << 24 | (u32)b[1] << 16 | (u32)b[2] << 8 | b[3]; }

/* Un CMD_SUBMIT (48 bytes; los datos de salida, aparte). */
static void submit(u8 *c, u32 seq, u32 devid, u32 dir, u32 ep, u32 largo, const u8 setup[8])
{
    for (int i = 0; i < 48; i++) c[i] = 0;
    be(c, 1); be(c + 4, seq); be(c + 8, devid); be(c + 12, dir); be(c + 16, ep); be(c + 24, largo); be(c + 32, 0xffffffffu);
    if (setup) for (int i = 0; i < 8; i++) c[40 + i] = setup[i];
}

static void avisar_y_esperar(int ms)
{
    w32(R_TUBO, 1);
    struct { int fd; short ev, rev; } q = { uio_fd, POLLIN, 0 };
    struct { i64 s, ns; } plazo = { 0, (long)ms * 1000000L };
    if (sc6(__NR_ppoll, (i64)&q, 1, (i64)&plazo, 0, 8, 0) > 0) {
        u32 cuenta; leer(uio_fd, &cuenta, 4); r32(R_EVENTOS); u32 uno = 1; escribir_fd(uio_fd, &uno, 4);
    }
}
/* Las respuestas: cuántas de cada clase y el estado de las primeras. Las de entrada traen datos (los seqnum con el bit
 * 20 son de entrada: así los escribe este /init). */
static long r_submit, r_unlink, r_datos;
static int estados[64], n_estados;
static u32 largos[64];
static u8 ultimos_datos[256];

static void leer_respuestas(void)
{
    for (;;) {
        u32 hay = vuelta_hay();
        if (hay < 48) return;
        u8 c[48];
        anillo_leer(TUBO_CABECERA + TAM_ANILLO, vuelta_lei, c, 48);
        u32 orden = lee_be(c), seq = lee_be(c + 4), largo = lee_be(c + 24);
        int estado = (int)lee_be(c + 20);
        u32 datos = (orden == 3 && (seq & 0x100000u)) ? largo : 0;
        if (datos > 16384 || hay < 48 + datos) { if (datos > 16384) { escribir("hostil: respuesta imposible\n"); } return; }
        if (datos) { u32 k = datos < sizeof ultimos_datos ? datos : sizeof ultimos_datos; anillo_leer(TUBO_CABECERA + TAM_ANILLO, vuelta_lei + 48, ultimos_datos, k); r_datos += datos; }
        vuelta_lei += 48 + datos;
        pt32(T_VUELTA_LEIDO, vuelta_lei);
        if (orden == 3) r_submit++; else if (orden == 4) r_unlink++;
        if (n_estados < 64) { estados[n_estados] = estado; largos[n_estados] = orden == 3 ? largo : 0; n_estados++; }
    }
}

/* Pone `n` bytes en la ida, esperando sitio (y leyendo respuestas mientras, para que DashOS no se quede sin sitio en la
 * vuelta). */
static int inyectar(const u8 *d, u32 n)
{
    u64 hasta = ahora_ms() + 20000;
    while (ida_libre() < n) {
        leer_respuestas();
        avisar_y_esperar(5);
        if (ahora_ms() > hasta) { escribir("hostil: el tubo no se vacía\n"); return 0; }
    }
    ida_poner(d, n);
    return 1;
}

static int esperar_respuestas(long submits, long unlinks, int ms)
{
    u64 hasta = ahora_ms() + ms;
    for (;;) {
        leer_respuestas();
        if (r_submit >= submits && r_unlink >= unlinks) return 1;
        if (ahora_ms() > hasta) return 0;
        avisar_y_esperar(10);
    }
}

static void decir_estados(void)
{
    for (int i = 0; i < n_estados; i++) {
        if (i) escribir(",");
        numero(estados[i]);
        if (largos[i]) { escribir("("); numero(largos[i]); escribir(")"); }
    }
}

static void hostil_usb(u32 devid)
{
    u8 c[48 + 64];
    static const u8 descriptor[8] = { 0x80, 6, 0, 1, 0, 0, 18, 0 };
    if (en_lista(hostil, "ajeno")) {
        r_submit = r_unlink = n_estados = 0;
        u32 otros[4] = { devid + 1, devid ^ 0x00010000u, 0xffffffffu, 0 };
        for (int i = 0; i < 4; i++) { submit(c, 0x7101000u + i, otros[i], 1, 0, 18, descriptor); inyectar(c, 48); }
        int bien = esperar_respuestas(4, 0, 10000);
        escribir("hostil: 4 peticiones para aparatos que no se le prestaron: estados "); decir_estados();
        escribir(bien ? "\n" : " (faltan respuestas)\n");
    }
    if (en_lista(hostil, "usbmalo")) {
        r_submit = r_unlink = n_estados = 0;
        u8 s_enorme[8] = { 0x80, 6, 0, 1, 0, 0, 0xff, 0xff };
        u8 s_16k[8] = { 0x80, 6, 0, 1, 0, 0, 0x00, 0x40 };
        u8 s_dir[8] = { 0x00, 5, 7, 0, 0, 0, 0, 0 };
        u8 s_conf[8] = { 0x00, 9, 77, 0, 0, 0, 0, 0 };
        u8 s_setdesc[8] = { 0x00, 7, 0, 1, 0, 0, 18, 0 };
        u8 s_config[8] = { 0x80, 6, 0, 2, 0, 0, 9, 0 };
        int n = 0;
        submit(c, 0x7111000u + n++, devid, 1, 1, 0xffffffffu, 0); inyectar(c, 48);               /* largo imposible */
        submit(c, 0x7111000u + n++, devid, 1, 0, 18, s_enorme); inyectar(c, 48);                /* wLength no cuadra */
        submit(c, 0x7111000u + n++, devid, 1, 0, 0x4000, s_16k); inyectar(c, 48);               /* 16 KB: vale, corta */
        submit(c, 0x7111000u + n++, devid, 1, 16, 64, 0); inyectar(c, 48);                     /* el punto 16 */
        submit(c, 0x7010000u + n++, devid, 0, 1, 4, 0); be(c + 32, 3); inyectar(c, 48);
        { u8 iso[4 + 48]; for (int i = 0; i < 52; i++) iso[i] = 0xee; inyectar(iso, 52); }       /* isócronas */
        submit(c, 0x7010000u + n++, devid, 0, 0, 0, s_dir); inyectar(c, 48);                    /* SET_ADDRESS */
        submit(c, 0x7010000u + n++, devid, 0, 0, 0, s_conf); inyectar(c, 48);                   /* configuración 77 */
        submit(c, 0x7111000u + n++, devid, 1, 5, 512, 0); inyectar(c, 48);                      /* punto sin configurar */
        submit(c, 0x7010000u + n++, devid, 0, 0, 18, s_setdesc); inyectar(c, 48);
        { u8 basura[18]; for (int i = 0; i < 18; i++) basura[i] = 0xa5 ^ i; inyectar(basura, 18); } /* SET_DESCRIPTOR */
        for (int i = 0; i < 48; i++) c[i] = 0;
        be(c, 2); be(c + 4, 0x7012000u + n++); be(c + 8, devid); be(c + 20, 0x12345678u); inyectar(c, 48); /* cancelar nada */
        submit(c, 0x7010000u + n++, devid, 0, 0, 9, s_config); inyectar(c, 48);                 /* dirección al revés */
        { u8 nueve[9] = { 0 }; inyectar(nueve, 9); }
        int bien = esperar_respuestas(n - 1, 1, 15000);
        escribir("hostil: USB/IP falso o enorme ("); numero(n); escribir(" peticiones): estados "); decir_estados();
        escribir(bien ? "\n" : " (faltan respuestas)\n");
    }
    if (en_lista(hostil, "tormenta")) {
        r_submit = r_unlink = n_estados = 0;
        u64 t0 = ahora_ms();
        /* La configuración 1, y su descriptor para saber el punto de interrupción de entrada. */
        u8 s_conf[8] = { 0x00, 9, 1, 0, 0, 0, 0, 0 };
        u8 s_config[8] = { 0x80, 6, 0, 2, 0, 0, 255, 0 };
        submit(c, 0x7020000u, devid, 0, 0, 0, s_conf); inyectar(c, 48);
        submit(c, 0x7121001u, devid, 1, 0, 255, s_config); inyectar(c, 48);
        esperar_respuestas(2, 0, 10000);
        u32 ep = 0;
        u32 total = largos[1] < sizeof ultimos_datos ? largos[1] : sizeof ultimos_datos;
        for (u32 i = 0; i + 6 < total && ultimos_datos[i] >= 2; i += ultimos_datos[i])
            if (ultimos_datos[i + 1] == 5 && (ultimos_datos[i + 2] & 0x80) && (ultimos_datos[i + 3] & 3) == 3 && !ep) ep = ultimos_datos[i + 2] & 0xf;
        escribir("hostil: tormenta: configuración 1 (estado "); numero(estados[0]); escribir("), punto de interrupción "); numero(ep); escribir("\n");
        long controles = 0, ints = 0, unlinks = 0;
        r_submit = r_unlink = n_estados = 0;
        for (int tanda = 0; tanda < 20; tanda++) {
            for (int i = 0; i < 100; i++) { submit(c, 0x7131000u + controles, devid, 1, 0, 18, descriptor); controles++; if (!inyectar(c, 48)) break; }
            for (int i = 0; i < 100 && ep; i++) { submit(c, 0x7141000u + tanda * 100 + i, devid, 1, ep, 16, 0); ints++; if (!inyectar(c, 48)) break; }
            for (int i = 0; i < 100 && ep; i++) {
                for (int j = 0; j < 48; j++) c[j] = 0;
                be(c, 2); be(c + 4, 0x7052000u + unlinks); be(c + 8, devid); be(c + 20, 0x7141000u + tanda * 100 + i);
                unlinks++;
                if (!inyectar(c, 48)) break;
            }
        }
        int bien = esperar_respuestas(controles, unlinks, 60000);
        escribir("hostil: tormenta de "); numero(controles + ints + unlinks); escribir(" peticiones ("); numero(controles);
        escribir(" de control, "); numero(ints); escribir(" de interrupción y "); numero(unlinks); escribir(" cancelaciones): ");
        numero(r_submit); escribir(" RET_SUBMIT y "); numero(r_unlink); escribir(" RET_UNLINK en "); numero(ahora_ms() - t0);
        escribir(bien ? " ms\n" : " ms (faltan respuestas)\n");
        /* Lo que pudiera quedar (de interrupción que acabaron antes de cancelarse), fuera antes de vhci-hcd. */
        esperar_respuestas(1L << 40, 0, 500);
    }
    if (en_lista(hostil, "basura")) {
        for (int i = 0; i < 48; i++) c[i] = 0x77;
        escribir("hostil: una orden de USB/IP que no existe\n");
        inyectar(c, 48);
        w32(R_TUBO, 1);
        for (;;) dormir_ms(60000);
    }
}

/* El préstamo de ahora (el canal dice cuál): si cambió, se suelta el de antes (vhci-hcd lo suelta al cerrarse el
 * socket) y se engancha el nuevo. */
static void usb_mirar(void)
{
    u32 cuenta = r32(R_USB_CUENTA);
    if (cuenta == usb_cuenta) return;
    if (usock >= 0) { escribir("dominio-red: DashOS quita el aparato USB\n"); cerrar(usock); usock = -1; }
    usb_cuenta = cuenta;
    u32 devid = r32(R_USB_DEVID), vel = r32(R_USB_VELOCIDAD);
    if (!devid || !tubo) return;
    /* Abre el tubo para este préstamo: los índices a cero y, después, el número. */
    ida_esc = vuelta_lei = 0;
    pt32(T_IDA_ESCRITO, 0);
    pt32(T_VUELTA_LEIDO, 0);
    barrera();
    pt32(T_ABIERTO, cuenta);
    w32(R_TUBO, 1);
    static int hostil_hecho;
    if (hostil[0] && !hostil_hecho) { hostil_hecho = 1; hostil_usb(devid); }
    int sv[2] = { -1, -1 };
    if (sc6(__NR_socketpair, AF_UNIX_, SOCK_STREAM_, 0, (i64)sv, 0, 0) != 0) { escribir("fallo: sin par de sockets\n"); return; }
    /* vhci-hcd: los puertos de alta velocidad (y menos) primero, los de super velocidad detrás. */
    u32 base = vel >= 5 ? 8 : 0;
    i64 r = -1;
    u32 puerto = base;
    for (; puerto < base + 8; puerto++) {
        char orden[96] = "";
        numero_en(orden, sizeof orden, puerto); unir(orden, " ", sizeof orden);
        numero_en(orden, sizeof orden, (u64)sv[1]); unir(orden, " ", sizeof orden);
        numero_en(orden, sizeof orden, devid); unir(orden, " ", sizeof orden);
        numero_en(orden, sizeof orden, vel);
        i64 fd = abrir("/sys/devices/platform/vhci_hcd.0/attach", 1 /* O_WRONLY: el fichero solo se escribe */);
        if (fd < 0) { escribir("fallo: sin vhci-hcd ("); numero(fd); escribir(")\n"); r = fd; break; }
        r = escribir_fd((int)fd, orden, largo(orden));
        cerrar((int)fd);
        if (r > 0) break;
    }
    cerrar(sv[1]); /* lo tiene vhci-hcd */
    if (r <= 0) { escribir("fallo: vhci-hcd no engancha el aparato ("); numero(r); escribir(")\n"); cerrar(sv[0]); return; }
    usock = sv[0];
    escribir("dominio-red: aparato USB "); numero(devid >> 16); escribir("-"); numero(devid & 0xffff);
    escribir(" (velocidad "); numero(vel); escribir(") enganchado en vhci-hcd, puerto "); numero(puerto); escribir("\n");
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
    if (en_lista(hostil, "caer")) { escribir("hostil: me caigo nada más empezar\n"); salir(1); }

    char cable[IFNAMSIZ] = "", cual[32] = "";
    if (solo) {
        /* QEMU solo (la e1000): la tarjeta aparece cuando su controlador acaba de sondear; 20 s como mucho. */
        for (int intento = 0; intento < 4000 && !buscar_tarjeta(cable, cual); intento++) dormir_ms(5);
        if (!cable[0]) { escribir("fallo: sin tarjeta\n"); for (;;) dormir_ms(60000); }
        u8 mac[6];
        sock = levantar(cable, mac);
        long total = meminfo("MemTotal:"), libre = meminfo("MemFree:"), reloj = reloj_ms();
        escribir(sock < 0 ? "fallo: no se levanta la tarjeta\n" : "listo (solo) reloj_ms=");
        numero(reloj); escribir(" usada_kb="); numero(total - libre); escribir(" total_kb="); numero(total); escribir("\n");
        registro("Memory:");
        for (;;) dormir_ms(60000);
    }
    /* El canal aparece cuando uio_pdrv_genirq acaba de sondear. */
    i64 uio = -1;
    for (int intento = 0; intento < 4000 && uio < 0; intento++) {
        uio = abrir("/dev/uio0", O_RDWR);
        if (uio < 0) dormir_ms(5);
    }
    if (uio < 0) { escribir("fallo: sin canal con DashOS (/dev/uio0)\n"); for (;;) dormir_ms(60000); }
    uio_fd = (int)uio;
    reg = proyectar((int)uio, 0, 4096);
    mem_fd = (int)abrir("/dev/mem", O_RDWR);
    if (!reg || mem_fd < 0 || r32(R_MAGIA) != MAGIA) { escribir("fallo: el canal no contesta\n"); for (;;) dormir_ms(60000); }
    /* Si el canal ya avisaba al sondearse UIO (el préstamo viene puesto desde el arranque), esa interrupción llegó antes
     * de abrir /dev/uio0: UIO la deshabilitó y este /init no la ve. Se habilita otra vez: si sigue alta, vuelve a llegar. */
    { u32 uno = 1; escribir_fd((int)uio, &uno, 4); }
    u64 tubo_dir = r64(R_TUBO_DIR);
    tubo_tam = r32(R_TUBO_TAM);
    if (tubo_dir && tubo_tam >= TUBO_CABECERA + 2 * TAM_ANILLO) tubo = proyectar(mem_fd, tubo_dir, tubo_tam);
    if (tubo_dir && !tubo) escribir("fallo: no se proyecta el tubo\n");
    usb_mirar();

    u8 mac[6] = { 0 }, mac_ofrecida[6] = { 0 };
    int ofrecida = 0;
    u64 ultimo_latido = 0, ultima_busqueda = 0;
    int hecho_descriptores = 0, hecho_fuera = 0;
    for (;;) {
        /* La tarjeta: hasta que aparezca (o vuelva, si se desenchufó). */
        u64 t = ahora_ms();
        if (sock < 0 && t - ultima_busqueda >= 20) {
            ultima_busqueda = t;
            if (buscar_tarjeta(cable, cual)) {
                sock = levantar(cable, mac);
                if (sock < 0) { escribir("fallo: no se levanta "); escribir(cable); escribir("\n"); }
                else {
                    escribir("dominio-red: tarjeta "); escribir(cable); escribir(" ("); escribir(cual); escribir(")\n");
                    if (ofrecida) {
                        int igual = 1;
                        for (int i = 0; i < 6; i++) igual &= mac[i] == mac_ofrecida[i];
                        if (!igual) escribir("dominio-red: la tarjeta que vuelve tiene otra MAC; la red de DashOS sigue con la primera\n");
                    }
                }
            }
            if (sock >= 0 && !ofrecida) {
                if (en_lista(hostil, "oferta")) {
                    u8 grupo[6] = { 0x01, 0x00, 0x5e, 0x00, 0x00, 0x01 };
                    int r = ofrecer(2, grupo);
                    escribir("hostil: oferta de un disco con MAC de grupo: "); escribir(r == 2 ? "rechazada\n" : "¡aceptada!\n");
                }
                int r = ofrecer(1, mac);
                if (r != 1) { escribir("fallo: el servicio rechaza la oferta\n"); for (;;) dormir_ms(60000); }
                ofrecida = 1;
                for (int i = 0; i < 6; i++) mac_ofrecida[i] = mac[i];
                long total = meminfo("MemTotal:"), libre = meminfo("MemFree:"), reloj = reloj_ms();
                w32(R_MEDIDA_TOTAL, (u32)total); w32(R_MEDIDA_LIBRE, (u32)libre); w32(R_MEDIDA_RELOJ, (u32)reloj); w32(R_LISTO, 1);
                escribir("listo tarjeta="); escribir(cable);
                escribir(" mac="); for (int i = 0; i < 6; i++) { const char *h = "0123456789abcdef"; char x[4] = { h[mac[i] >> 4], h[mac[i] & 15], i < 5 ? ':' : 0, 0 }; escribir(x); }
                escribir(" reloj_ms="); numero(reloj); escribir(" usada_kb="); numero(total - libre); escribir(" total_kb="); numero(total); escribir("\n");
                registro("Memory:");
            }
        }
        struct { int fd; short ev, rev; } p[3] = { { (int)uio, POLLIN, 0 }, { sock, POLLIN, 0 }, { usock, POLLIN, 0 } };
        if (usock >= 0 && vuelta_hay()) p[2].ev |= POLLOUT;
        struct { i64 s, ns; } plazo = { 0, (sock < 0 ? 20 : 250) * 1000000L };
        sc6(__NR_ppoll, (i64)p, 3, (i64)&plazo, 0, 8, 0);
        t = ahora_ms();
        if (ofrecida && t - ultimo_latido >= 500) { w32(R_LATIDO, 1); ultimo_latido = t; }
        /* Cada minuto, las cuentas (lo que pasa por la tarjeta y por el tubo) y si la tarjeta tiene enlace. */
        static u64 ultimas_cuentas;
        if (t - ultimas_cuentas >= 60000) {
            ultimas_cuentas = t;
            char ruta[64] = "/sys/class/net/"; unir(ruta, cable, sizeof ruta); unir(ruta, "/carrier", sizeof ruta);
            char enlace[4] = "?";
            i64 fd = cable[0] ? abrir(ruta, O_RDONLY) : -1;
            if (fd >= 0) { i64 n = leer((int)fd, enlace, 1); enlace[n > 0 ? 1 : 0] = 0; cerrar((int)fd); }
            escribir("dominio-red: cuentas: "); numero(enviadas); escribir(" tramas enviadas, "); numero(recibidas);
            escribir(" recibidas ("); numero(entregadas); escribir(" entregadas, "); numero(sin_bufer); escribir(" sin búfer); USB/IP: ");
            numero(usb_bytes_ida); escribir(" bytes de ida y "); numero(usb_bytes_vuelta); escribir(" de vuelta; enlace "); escribir(enlace); escribir("\n");
        }
        if (p[0].rev & POLLIN) {
            u32 cuenta;
            leer((int)uio, &cuenta, 4);
            u32 ev = r32(R_EVENTOS);
            if (ev & E_AVISO) r32(R_AVISADAS);
            if (ev & E_CAMBIO) mirar_colas();
            if (ev & E_USB) usb_mirar();
            u32 uno = 1;
            escribir_fd((int)uio, &uno, 4); /* la interrupción, otra vez habilitada */
        }
        bombear();
        if (sock >= 0 && (p[1].rev & (8 /* POLLERR */ | 16 /* POLLHUP */ | 32 /* POLLNVAL */))) {
            escribir("dominio-red: la tarjeta "); escribir(cable); escribir(" se fue\n");
            cerrar(sock); sock = -1;
        }
        if (enviar()) w32(R_USADOS, 1);
        int nuevas = 0;
        for (int k = 0; k < 64 && sock >= 0; k++) {
            struct sockaddr_ll de; u32 tam_de = sizeof de;
            i64 n = sc6(__NR_recvfrom, sock, (i64)(trama + 12), 2048, MSG_DONTWAIT, (i64)&de, (i64)&tam_de);
            if (n == -100 /* ENETDOWN */ || n == -6 /* ENXIO */ || n == -19 /* ENODEV */) {
                escribir("dominio-red: la tarjeta "); escribir(cable); escribir(" se fue\n");
                cerrar(sock); sock = -1;
                break;
            }
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
