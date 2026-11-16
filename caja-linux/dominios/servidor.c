/* El "servidor" de prueba de un dominio de controladores (S35, paso 1): el /init de un Linux mínimo sin
 * distribución. No sirve nada todavía: abre los aparatos del dominio y los deja listos, que es lo que hace falta
 * para medir la memoria y el tiempo de arranque.
 *
 *  - dominio=red   (en la línea del kernel): espera la tarjeta con cable (controlador e1000e) y la de virtio-net
 *                  (el canal hacia DashOS), las levanta y abre un socket de paquetes (AF_PACKET) en cada una.
 *  - dominio=wifi  lo mismo con una interfaz de Wi-Fi (cualquiera con phy80211: en QEMU, la de mac80211_hwsim) y
 *                  la de virtio-net.
 *  - dominio=red+wifi  las tres.
 *  - trafico=1     además, después de la memoria en reposo, pasa 20000 tramas de la tarjeta con cable a la virtio-net
 *                  y vuelve a volcar /proc/meminfo entre "memoria2-inicio" y "memoria2-fin".
 *
 * Al tenerlas, escribe "listo" con el tiempo del kernel (/proc/uptime), la línea "Memory:" del registro del kernel
 * y, tres segundos después, /proc/meminfo entre "memoria-inicio" y "memoria-fin". Después se queda quieto.
 *
 * Sin biblioteca C (ni glibc ni musl): llamadas al sistema directas, para que el programa sea igual y mínimo en
 * aarch64 y en x86-64. Se compila con -nostdlib -static -ffreestanding (ver tools/medir_dominios.py).
 */

#include <linux/if.h>
#include <linux/if_ether.h>
#include <linux/if_packet.h>
#include <linux/sockios.h>
#include <asm/unistd.h>

typedef unsigned long u64;
typedef long i64;

/* ---- llamadas al sistema ---- */

#if defined(__aarch64__)
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
#elif defined(__x86_64__)
static i64 sc6(i64 n, i64 a, i64 b, i64 c, i64 d, i64 e, i64 f)
{
    register i64 r10 __asm__("r10") = d;
    register i64 r8 __asm__("r8") = e;
    register i64 r9 __asm__("r9") = f;
    i64 r;
    __asm__ volatile("syscall"
                     : "=a"(r)
                     : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
                     : "rcx", "r11", "memory");
    return r;
}
__asm__(".global _start\n_start:\n mov %rsp, %rdi\n and $-16, %rsp\n call principal\n");
#else
#error "solo aarch64 y x86-64"
#endif

#define sc(n, a, b, c) sc6((n), (i64)(a), (i64)(b), (i64)(c), 0, 0, 0)

#define AT_FDCWD (-100)
#ifndef AF_PACKET
#define AF_PACKET 17
#endif
#define SOCK_RAW_ 3
#define O_RDONLY 0
#define O_RDWR 2
#define O_NONBLOCK 04000
#if defined(__aarch64__)
#define O_DIRECTORY 040000
#else
#define O_DIRECTORY 0200000
#endif

static i64 abrir(const char *ruta, int flags) { return sc6(__NR_openat, AT_FDCWD, (i64)ruta, flags, 0, 0, 0); }
static i64 leer(int fd, void *b, u64 n) { return sc(__NR_read, fd, b, n); }
static i64 escribir_fd(int fd, const void *b, u64 n) { return sc(__NR_write, fd, b, n); }
static void cerrar(int fd) { sc(__NR_close, fd, 0, 0); }
static i64 montar(const char *fuente, const char *dest, const char *tipo)
{
    return sc6(__NR_mount, (i64)fuente, (i64)dest, (i64)tipo, 0, 0, 0);
}
static void dormir_ms(long ms)
{
    struct { i64 s, ns; } t = { ms / 1000, (ms % 1000) * 1000000L };
    sc(__NR_nanosleep, &t, 0, 0);
}

/* ---- texto ---- */

static u64 largo(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static void escribir(const char *s) { escribir_fd(1, s, largo(s)); }
static int empieza(const char *s, const char *p) { while (*p) if (*s++ != *p++) return 0; return 1; }
static int iguales(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static int contiene(const char *s, u64 n, const char *p)
{
    u64 m = largo(p);
    for (u64 i = 0; i + m <= n; i++) {
        u64 j = 0;
        while (j < m && s[i + j] == p[j]) j++;
        if (j == m) return 1;
    }
    return 0;
}
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

static char buf[16384];

/* Escribe un fichero entero (pequeño: /proc/meminfo, /proc/uptime) por la consola. */
static void volcar(const char *ruta)
{
    i64 fd = abrir(ruta, O_RDONLY);
    if (fd < 0) { escribir("(no se abre "); escribir(ruta); escribir(")\n"); return; }
    i64 n;
    while ((n = leer(fd, buf, sizeof buf)) > 0) escribir_fd(1, buf, n);
    cerrar(fd);
}

/* Las líneas del registro del kernel que contienen `clave` (cada read de /dev/kmsg da un mensaje entero). */
static void registro(const char *clave)
{
    i64 fd = abrir("/dev/kmsg", O_RDONLY | O_NONBLOCK);
    if (fd < 0) return;
    i64 n;
    while ((n = leer(fd, buf, sizeof buf - 1)) > 0 || n == -32 /* EPIPE: mensajes perdidos */) {
        if (n <= 0) continue;
        if (contiene(buf, n, clave)) { escribir("kmsg: "); escribir_fd(1, buf, n); }
    }
    cerrar(fd);
}

/* ---- interfaces de red ---- */

struct dirent64_ { u64 ino; i64 off; unsigned short reclen; unsigned char tipo; char nombre[]; };

/* El nombre del controlador de una interfaz (/sys/class/net/<if>/device/driver -> .../<controlador>), o "wifi" si
 * tiene phy80211 (las de mac80211_hwsim no cuelgan de un aparato con controlador). */
static void controlador(const char *intf, char *sal, u64 cap)
{
    char ruta[128], destino[256];
    copiar(ruta, "/sys/class/net/", sizeof ruta); unir(ruta, intf, sizeof ruta); unir(ruta, "/phy80211", sizeof ruta);
    i64 fd = abrir(ruta, O_RDONLY | O_DIRECTORY);
    if (fd >= 0) { cerrar(fd); copiar(sal, "wifi", cap); return; }
    copiar(ruta, "/sys/class/net/", sizeof ruta); unir(ruta, intf, sizeof ruta); unir(ruta, "/device/driver", sizeof ruta);
    i64 n = sc6(__NR_readlinkat, AT_FDCWD, (i64)ruta, (i64)destino, sizeof destino - 1, 0, 0);
    if (n <= 0) { copiar(sal, "?", cap); return; }
    destino[n] = 0;
    char *ultimo = destino;
    for (char *p = destino; *p; p++) if (*p == '/') ultimo = p + 1;
    copiar(sal, ultimo, cap);
}

/* Levanta la interfaz y devuelve el socket de paquetes atado a ella (o un error negativo). */
static int levantar(const char *intf)
{
    i64 s = sc(__NR_socket, AF_PACKET, SOCK_RAW_, 0);
    if (s < 0) return (int)s;
    struct ifreq r;
    for (u64 i = 0; i < sizeof r; i++) ((char *)&r)[i] = 0;
    copiar(r.ifr_name, intf, IFNAMSIZ);
    i64 e = sc(__NR_ioctl, s, SIOCGIFFLAGS, &r);
    if (e == 0) { r.ifr_flags |= IFF_UP; e = sc(__NR_ioctl, s, SIOCSIFFLAGS, &r); }
    if (e == 0) e = sc(__NR_ioctl, s, SIOCGIFINDEX, &r);
    if (e != 0) { cerrar(s); return (int)e; }
    /* El socket que usaría el servidor para pasar tramas entre el aparato y DashOS: se queda abierto. */
    struct sockaddr_ll ll;
    for (u64 i = 0; i < sizeof ll; i++) ((char *)&ll)[i] = 0;
    ll.sll_family = AF_PACKET;
    ll.sll_protocol = (unsigned short)((ETH_P_ALL >> 8) | ((ETH_P_ALL & 0xff) << 8));
    ll.sll_ifindex = r.ifr_ifindex;
    sc(__NR_bind, s, &ll, sizeof ll);
    return (int)s;
}

/* Con trafico=1: tramas de 1514 bytes por la tarjeta con cable, que en QEMU comparte concentrador con la virtio-net,
 * y recogidas por el socket de la virtio-net. Es lo que haría el servidor al pasar tramas. Mide la memoria con
 * tráfico, no el rendimiento. */
static void trafico(int sale, int entra, int tramas)
{
    static unsigned char t[1514];
    for (int i = 0; i < 6; i++) t[i] = 0xff;
    for (int i = 6; i < 12; i++) t[i] = 0x02;
    t[12] = 0x88; t[13] = 0xb5; /* tipo local de pruebas (IEEE 802) */
    long enviadas = 0, recibidas = 0;
    for (int i = 0; i < tramas; i++) {
        if (sc6(__NR_sendto, sale, (i64)t, sizeof t, 0, 0, 0) == (i64)sizeof t) enviadas++;
        while (sc6(__NR_recvfrom, entra, (i64)buf, sizeof buf, 0x40 /* MSG_DONTWAIT */, 0, 0) > 0) recibidas++;
    }
    for (int i = 0; i < 100; i++) {
        while (sc6(__NR_recvfrom, entra, (i64)buf, sizeof buf, 0x40, 0, 0) > 0) recibidas++;
        dormir_ms(10);
    }
    escribir("trafico enviadas="); numero(enviadas); escribir(" recibidas="); numero(recibidas); escribir("\n");
}

/* ---- principal ---- */

void principal(long *sp)
{
    long argc = sp[0];
    char **envp = (char **)(sp + 1 + argc + 1);
    const char *dominio = "red";
    for (char **e = envp; *e; e++) if (empieza(*e, "dominio=")) dominio = *e + 8;

    montar("devtmpfs", "/dev", "devtmpfs");
    montar("proc", "/proc", "proc");
    montar("sysfs", "/sys", "sysfs");
    /* Sin printk (variante sin-printk) no hay consola del kernel y /dev/console no se abre: el puerto serie directo. */
    i64 c = abrir("/dev/console", O_RDWR);
    if (c < 0) c = abrir("/dev/ttyAMA0", O_RDWR);
    if (c < 0) c = abrir("/dev/ttyS0", O_RDWR);
    if (c >= 0 && c != 1) { sc(__NR_dup3, c, 1, 0); sc(__NR_dup3, c, 2, 0); }

    int quiere_cable = iguales(dominio, "red") || iguales(dominio, "red+wifi");
    int quiere_wifi = iguales(dominio, "wifi") || iguales(dominio, "red+wifi");
    char cable[IFNAMSIZ] = "", wifi[IFNAMSIZ] = "", virtio[IFNAMSIZ] = "";

    /* Las interfaces aparecen cuando su controlador termina de sondear el aparato: se mira cada 5 ms, 20 s como mucho. */
    for (int intento = 0; intento < 4000; intento++) {
        i64 d = abrir("/sys/class/net", O_RDONLY | O_DIRECTORY);
        if (d >= 0) {
            i64 n;
            while ((n = sc(__NR_getdents64, d, buf, sizeof buf)) > 0) {
                for (i64 p = 0; p < n;) {
                    struct dirent64_ *de = (struct dirent64_ *)(buf + p);
                    p += de->reclen;
                    if (de->nombre[0] == '.' || iguales(de->nombre, "lo")) continue;
                    char ctl[64];
                    controlador(de->nombre, ctl, sizeof ctl);
                    if (iguales(ctl, "e1000e") && !cable[0]) copiar(cable, de->nombre, IFNAMSIZ);
                    else if (iguales(ctl, "wifi") && !wifi[0]) copiar(wifi, de->nombre, IFNAMSIZ);
                    else if (iguales(ctl, "virtio_net") && !virtio[0]) copiar(virtio, de->nombre, IFNAMSIZ);
                }
            }
            cerrar(d);
        }
        if (virtio[0] && (!quiere_cable || cable[0]) && (!quiere_wifi || wifi[0])) break;
        dormir_ms(5);
    }

    int mal = 0;
    if (!virtio[0]) { escribir("fallo: sin virtio-net\n"); mal = 1; }
    if (quiere_cable && !cable[0]) { escribir("fallo: sin e1000e\n"); mal = 1; }
    if (quiere_wifi && !wifi[0]) { escribir("fallo: sin interfaz de Wi-Fi\n"); mal = 1; }
    const char *lista[3] = { cable, wifi, virtio };
    int socks[3] = { -1, -1, -1 };
    int con_trafico = 0;
    for (char **e = envp; *e; e++) if (iguales(*e, "trafico=1")) con_trafico = 1;
    for (int i = 0; i < 3 && !mal; i++) {
        if (!lista[i][0]) continue;
        int e = levantar(lista[i]);
        socks[i] = e;
        if (e < 0) { escribir("fallo: no se levanta "); escribir(lista[i]); escribir(" ("); numero(e); escribir(")\n"); mal = 1; }
    }
    if (!mal) {
        escribir("listo dominio="); escribir(dominio);
        if (cable[0]) { escribir(" cable="); escribir(cable); }
        if (wifi[0]) { escribir(" wifi="); escribir(wifi); }
        escribir(" virtio="); escribir(virtio);
        escribir(" uptime=");
        volcar("/proc/uptime");
    }
    escribir("cpus=");
    volcar("/sys/devices/system/cpu/online");
    registro("Memory:");
    dormir_ms(3000);
    escribir("memoria-inicio\n");
    volcar("/proc/meminfo");
    escribir("memoria-fin\n");
    if (con_trafico && !mal && socks[0] >= 0) {
        trafico(socks[0], socks[2], 20000);
        escribir("memoria2-inicio\n");
        volcar("/proc/meminfo");
        escribir("memoria2-fin\n");
    }
    for (;;) dormir_ms(60000);
}
