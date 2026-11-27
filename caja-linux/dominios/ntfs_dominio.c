/* El /init del dominio Linux de x86 para NTFS (docs/bitacoras/x86-dominio.md): ejecuta ntfsresize sobre la partición
 * NTFS que DashOS le presta (/dev/vda, entera, sin tabla) y dice el resultado.
 *
 * Sin biblioteca C (llamadas al sistema directas, como servidor.c y dominio_red.c), solo x86-64. Lo que hace:
 *
 *  - monta devtmpfs en /dev y proc en /proc, y abre /dev/kmsg: todo lo que dice va ahí, una línea por registro, con
 *    el prefijo "dominio: " (sale por la consola temprana del kernel; no usa /dev/console, que no existe);
 *  - lee la orden de /proc/cmdline:
 *      dashos.ntfs=info            ntfsresize --info --no-progress-bar /dev/vda
 *      dashos.ntfs=encoger:<bytes> primero en seco (-n -s <bytes>) y, si sale bien, de verdad (-s <bytes>),
 *                                  contestando "y" a su pregunta por la entrada estándar
 *    Nunca con --force: si el volumen está hibernado, sucio o con el registro sin vaciar, ntfsresize se niega y eso
 *    es lo que se dice. En info y en la prueba en seco la entrada estándar es /dev/null (a una pregunta, "no");
 *  - cada línea que escribe ntfsresize (salida y errores, juntos) va a /dev/kmsg como "dominio: <línea>";
 *  - al acabar: "dominio: memoria ..." (lo máximo que ocupó ntfsresize y la memoria del dominio) y
 *    "dominio: fin <paso> <código de salida>", con paso info, prueba o encoger (u "orden" si la orden falta o no
 *    se entiende, con código 2). Si ntfsresize muere por una señal, el código es 128 + la señal; si no se puede
 *    ejecutar, 127;
 *  - sync y reboot(RB_POWER_OFF). Sin ACPI el kernel no sabe apagar y se para (CLI y HLT); el monitor lo ve.
 */

#include <asm/unistd.h>

#if !defined(__x86_64__)
#error "solo x86-64"
#endif

typedef unsigned long u64;
typedef long i64;

/* ---- llamadas al sistema ---- */

static i64 sc6(i64 n, i64 a, i64 b, i64 c, i64 d, i64 e, i64 f)
{
    register i64 r10 __asm__("r10") = d;
    register i64 r8 __asm__("r8") = e;
    register i64 r9 __asm__("r9") = f;
    i64 r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c), "r"(r10), "r"(r8), "r"(r9)
                     : "rcx", "r11", "memory");
    return r;
}
/* La pila llega alineada a 16 (apunta a argc); `call` deja la dirección de vuelta como espera la función. */
__asm__(".global _start\n_start:\n xor %ebp, %ebp\n mov %rsp, %rdi\n and $-16, %rsp\n call principal\n hlt\n");

#define sc(n, a, b, c) sc6((n), (i64)(a), (i64)(b), (i64)(c), 0, 0, 0)

#define AT_FDCWD (-100)
#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CLOEXEC 02000000
#define EINTR 4
#define REBOOT_MAGIC1 0xfee1dead
#define REBOOT_MAGIC2 672274793
#define REBOOT_POWER_OFF 0x4321fedc

static i64 abrir(const char *ruta, int flags) { return sc6(__NR_openat, AT_FDCWD, (i64)ruta, flags, 0, 0, 0); }
static i64 leer(int fd, void *b, u64 n) { return sc(__NR_read, fd, b, n); }
static i64 escribir_fd(int fd, const void *b, u64 n) { return sc(__NR_write, fd, b, n); }
static void cerrar(int fd) { sc(__NR_close, fd, 0, 0); }
static void dormir_ms(long ms)
{
    struct { i64 s, ns; } t = { ms / 1000, (ms % 1000) * 1000000L };
    sc(__NR_nanosleep, &t, 0, 0);
}

/* GCC puede llamar a estas dos por su cuenta (al iniciar un vector con una cadena más corta, por ejemplo). */
void *memset(void *d, int c, u64 n) { unsigned char *p = d; while (n--) *p++ = (unsigned char)c; return d; }
void *memcpy(void *d, const void *s, u64 n) { unsigned char *p = d; const unsigned char *q = s; while (n--) *p++ = *q++; return d; }

/* ---- texto ---- */

static u64 largo(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static int empieza(const char *s, const char *p) { while (*p) if (*s++ != *p++) return 0; return 1; }
static void unir(char *d, const char *s, u64 cap) { u64 n = largo(d), i = 0; while (s[i] && n + 1 < cap) d[n++] = s[i++]; d[n] = 0; }
static void unir_num(char *d, long v, u64 cap)
{
    char b[24]; int i = 23; b[i] = 0;
    if (v == 0) b[--i] = '0';
    int neg = v < 0; if (neg) v = -v;
    while (v) { b[--i] = '0' + v % 10; v /= 10; }
    if (neg) b[--i] = '-';
    unir(d, b + i, cap);
}

/* ---- /dev/kmsg ---- */

static int kmsg = -1;
#define PREFIJO "<5>dominio: " /* nivel 5 (notice): sale en la consola con el nivel de registro de serie */
#define MAX_LINEA 900           /* por debajo del máximo de un registro del kernel (1024 menos su cabecera) */

/* Una línea al registro, con el prefijo. Si /dev/kmsg no se pudo abrir, no hay a dónde decirlo. */
static void decir(const char *s, u64 n)
{
    static char l[sizeof PREFIJO + MAX_LINEA];
    if (kmsg < 0) return;
    if (n > MAX_LINEA) n = MAX_LINEA;
    u64 p = sizeof PREFIJO - 1;
    for (u64 i = 0; i < p; i++) l[i] = PREFIJO[i];
    for (u64 i = 0; i < n; i++) l[p + i] = s[i];
    escribir_fd(kmsg, l, p + n);
}
static void decir_txt(const char *s) { decir(s, largo(s)); }

/* ---- ntfsresize ---- */

struct rusage_ { struct { i64 s, us; } utime, stime; i64 maxrss, resto[13]; };
static long max_rss_kb;

/* Ejecuta /bin/ntfsresize con `args` sobre /dev/vda. Si `si` es distinto de 0, su entrada estándar es "y\n"; si no,
 * /dev/null. Lo que escribe (salida y errores) va al registro línea a línea. Devuelve el código de salida. */
static int ntfsresize(const char *const *args, int si)
{
    static char *argv[16];
    static char *envp[] = { "PATH=/bin", "LANG=C", 0 };
    int n = 0;
    argv[n++] = "/bin/ntfsresize";
    for (int i = 0; args[i] && n < 14; i++) argv[n++] = (char *)args[i];
    argv[n++] = "/dev/vda";
    argv[n] = 0;

    char orden[256] = "ntfsresize";
    for (int i = 1; argv[i]; i++) { unir(orden, " ", sizeof orden); unir(orden, argv[i], sizeof orden); }
    decir_txt(orden);

    int salida[2], entrada[2] = { -1, -1 };
    if (sc(__NR_pipe2, salida, O_CLOEXEC, 0) < 0) { decir_txt("no se pudo crear la tubería"); return 127; }
    if (si && sc(__NR_pipe2, entrada, O_CLOEXEC, 0) < 0) { decir_txt("no se pudo crear la tubería"); return 127; }

    i64 pid = sc(__NR_fork, 0, 0, 0);
    if (pid < 0) { decir_txt("no se pudo crear el proceso"); return 127; }
    if (pid == 0) {
        int fd0 = si ? entrada[0] : (int)abrir("/dev/null", O_RDONLY);
        if (fd0 >= 0) sc(__NR_dup2, fd0, 0, 0);
        sc(__NR_dup2, salida[1], 1, 0);
        sc(__NR_dup2, salida[1], 2, 0);
        sc(__NR_execve, argv[0], argv, envp);
        static const char m[] = "no se pudo ejecutar /bin/ntfsresize\n";
        escribir_fd(1, m, sizeof m - 1);
        for (;;) sc(__NR_exit, 127, 0, 0);
    }
    cerrar(salida[1]);
    if (si) {
        cerrar(entrada[0]);
        escribir_fd(entrada[1], "y\n", 2);
        cerrar(entrada[1]);
    }

    /* Línea a línea; "\r" también corta (por si acaso hay barra de progreso). */
    static char b[4096], l[MAX_LINEA];
    u64 m = 0;
    for (;;) {
        i64 r = leer(salida[0], b, sizeof b);
        if (r == -EINTR) continue;
        if (r <= 0) break;
        for (i64 i = 0; i < r; i++) {
            char c = b[i];
            if (c == '\n' || c == '\r') { if (m) decir(l, m); m = 0; }
            else { l[m++] = c; if (m == sizeof l) { decir(l, m); m = 0; } }
        }
    }
    if (m) decir(l, m);
    cerrar(salida[0]);

    int estado = 0;
    struct rusage_ ru;
    ru.maxrss = 0;
    while (sc6(__NR_wait4, pid, (i64)&estado, 0, (i64)&ru, 0, 0) == -EINTR) {}
    if (ru.maxrss > max_rss_kb) max_rss_kb = ru.maxrss;
    if ((estado & 0x7f) == 0) return (estado >> 8) & 0xff;  /* acabó: su código */
    return 128 + (estado & 0x7f);                             /* una señal */
}

/* ---- memoria ---- */

static char buf[4096];

static long meminfo(const char *clave)
{
    i64 fd = abrir("/proc/meminfo", O_RDONLY);
    if (fd < 0) return -1;
    i64 n = leer((int)fd, buf, sizeof buf - 1);
    cerrar((int)fd);
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

/* ---- principal ---- */

static void fin(const char *paso, int codigo)
{
    char l[160] = "memoria ntfsresize_max_kb=";
    unir_num(l, max_rss_kb, sizeof l);
    unir(l, " total_kb=", sizeof l);
    unir_num(l, meminfo("MemTotal:"), sizeof l);
    unir(l, " libre_kb=", sizeof l);
    unir_num(l, meminfo("MemFree:"), sizeof l);
    decir_txt(l);
    char f[64] = "fin ";
    unir(f, paso, sizeof f);
    unir(f, " ", sizeof f);
    unir_num(f, codigo, sizeof f);
    decir_txt(f);
    sc(__NR_sync, 0, 0, 0);
    sc6(__NR_reboot, REBOOT_MAGIC1, REBOOT_MAGIC2, REBOOT_POWER_OFF, 0, 0, 0);
    for (;;) dormir_ms(1000);  /* si el apagado vuelve: el init no puede acabar (sería un pánico) */
}

void principal(u64 *pila)
{
    (void)pila;
    sc(__NR_mkdir, "/dev", 0755, 0);
    sc(__NR_mkdir, "/proc", 0555, 0);
    sc6(__NR_mount, (i64)"devtmpfs", (i64)"/dev", (i64)"devtmpfs", 0, 0, 0);
    sc6(__NR_mount, (i64)"proc", (i64)"/proc", (i64)"proc", 0, 0, 0);
    kmsg = (int)abrir("/dev/kmsg", O_WRONLY | O_CLOEXEC);

    /* La orden, de /proc/cmdline. */
    static char linea[4096], valor[64];
    i64 fd = abrir("/proc/cmdline", O_RDONLY), n = 0;
    if (fd >= 0) { n = leer((int)fd, linea, sizeof linea - 1); cerrar((int)fd); }
    linea[n > 0 ? n : 0] = 0;
    valor[0] = 0;
    for (i64 i = 0; i < n; i++) {
        if ((i == 0 || linea[i - 1] == ' ') && empieza(linea + i, "dashos.ntfs=")) {
            i += 12;
            u64 k = 0;
            while (i < n && linea[i] != ' ' && linea[i] != '\n' && k + 1 < sizeof valor) valor[k++] = linea[i++];
            valor[k] = 0;
            break;
        }
    }

    /* El disco: lo crea el kernel antes de lanzar el init, pero se espera un poco por si acaso. */
    for (int i = 0; i < 50; i++) {
        i64 d = abrir("/dev/vda", O_RDONLY | O_CLOEXEC);
        if (d >= 0) { cerrar((int)d); break; }
        if (i == 49) decir_txt("no aparece /dev/vda");
        dormir_ms(40);
    }

    if (valor[0] == 'i' && empieza(valor, "info") && valor[4] == 0) {
        static const char *const a[] = { "--info", "--no-progress-bar", 0 };
        fin("info", ntfsresize(a, 0));
    }
    if (empieza(valor, "encoger:")) {
        const char *bytes = valor + 8;
        u64 k = 0;
        while (bytes[k] >= '0' && bytes[k] <= '9') k++;
        if (k > 0 && k <= 19 && bytes[k] == 0) {
            const char *const prueba[] = { "--no-progress-bar", "-n", "-s", bytes, 0 };
            int r = ntfsresize(prueba, 0);
            if (r != 0) fin("prueba", r);
            const char *const deverdad[] = { "--no-progress-bar", "-s", bytes, 0 };
            fin("encoger", ntfsresize(deverdad, 1));
        }
    }
    decir_txt("orden desconocida: se espera dashos.ntfs=info o dashos.ntfs=encoger:<bytes>");
    fin("orden", 2);
}
