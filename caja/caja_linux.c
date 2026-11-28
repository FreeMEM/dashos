/*
    DashOS, la caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md, L3): la caja como programa de Linux, el "WINE de
    AmigaOS". `caja-amiga [-v] programa [argumentos...]` carga un ejecutable hunk de AmigaOS de los ficheros de Linux
    y lo ejecuta en el JIT (Emu68 en AArch64, RelevoJIT en x86-64) con las bibliotecas nativas de la caja. Lo que el
    programa escribe por la dos.library sale por la salida estándar; lo que dice la caja, por la de errores, y solo
    con -v. Acaba con el código de vuelta del programa (D0), o con 3 si cayó.

    Es la caja de DashOS sin sus pruebas: caja.c entra tal cual (su caja_main, la de las pruebas del arranque, queda
    sin usar y el enlazador la quita), con plataforma_linux.c debajo.
*/

#define caja_main caja_main_dashos
#include "caja.c"
#undef caja_main

extern int plat_detalle;   /* plataforma_linux.c: 1, lo que dice la caja sale por la salida de errores */

/* El programa, en memoria de la caja por encima de 4 GB (la ventana del servicio de archivos en DashOS). */
#define PROGRAMA     0x0000000400000000UL
#define PROGRAMA_MAX (64UL << 20)

/* El entorno de Linux (tras argv en la pila del arranque), para plat_entorno. */
static char **entorno;

/* El valor de la variable `nombre` del entorno, o NULL (lo usa pegamento/src/linux/archivos.rs). */
const char *plat_entorno(const char *nombre)
{
    for (char **e = entorno; e && *e; e++) {
        const char *a = *e, *b = nombre;
        while (*b && *a == *b) a++, b++;
        if (!*b && *a == '=') return a + 1;
    }
    return NULL;
}

/* Los argumentos tal como los da el CLI del Amiga: separados por espacios y con "\n" al final. */
static char linea_args[4096];

static void error(const char *a, const char *b)
{
    plat_error(a, largo_de(a));
    if (b) plat_error(b, largo_de(b));
    plat_error("\n", 1);
}

/* Pone en linea_args, desde `n`, el argumento `a` tal como lo ve el CLI del Amiga, y devuelve dónde acaba: una ruta
   absoluta de Linux que existe pasa a "Linux:..." (en el Amiga, "/" al principio es el cajón de arriba); con espacios
   o vacío, entre comillas (las de dentro, como *" de ReadArgs); y de UTF-8 a Latin-1 (lo que no cabe, '?'). Lo que no
   quepa en la línea se corta. */
static size_t poner_arg(size_t n, const char *a)
{
    const size_t max = sizeof linea_args - 2;   /* sitio para "\n" y el 0 */
    int comillas = !*a;
    for (const char *p = a; *p; p++)
        if (*p == ' ' || *p == '\t' || *p == '"') comillas = 1;
#define PON(c) do { if (n < max) linea_args[n++] = (char)(c); } while (0)
    if (comillas) PON('"');
    if (a[0] == '/' && plat_existe(a)) {
        for (const char *p = "Linux:"; *p; p++) PON(*p);
        a++;
    }
    for (const unsigned char *p = (const unsigned char *)a; *p; ) {
        unsigned c = *p;
        if (c < 0x80) {
            if (c == '"' && comillas) PON('*');
            PON(c);
            p++;
        } else if ((c == 0xC2 || c == 0xC3) && (p[1] & 0xC0) == 0x80) {
            PON(((c & 0x1F) << 6) | (p[1] & 0x3F));
            p += 2;
        } else {
            PON('?');
            p++;
            while ((*p & 0xC0) == 0x80) p++;
        }
    }
    if (comillas) PON('"');
#undef PON
    return n;
}

/* El nombre del programa para exec: lo que va tras la última barra. */
static const char *nombre_de(const char *ruta)
{
    const char *n = ruta;
    for (const char *p = ruta; *p; p++)
        if (*p == '/') n = p + 1;
    return n;
}

/* Ctrl-C en la terminal: SIGBREAKF_CTRL_C al programa, como en el CLI del Amiga (caja_tomar_ctrl_c, lo recoge el
   pegamento). Si no lo atiende, al tercero se acaba la caja. */
static volatile int interrupciones;

static void al_ctrl_c(void)
{
    if (++interrupciones >= 3) {
        static const char m[] = "\ncaja-amiga: el programa no atiende Ctrl-C; se acaba\n";
        plat_error(m, sizeof m - 1);
        plat_acabar(130);
    }
    __atomic_store_n(&ctrl_c_pendiente, 1, __ATOMIC_RELEASE);
    motor_interrumpir();
}

int caja_linux_main(void)
{
    /* Lo que dejó Linux en la pila al arrancar: argc y argv. */
    const uintptr_t *sp = (const uintptr_t *)pila_elf;
    int argc = (int)sp[0];
    char **argv = (char **)(sp + 1);
    entorno = argv + argc + 1;
    int i = 1;
    if (i < argc && argv[i][0] == '-' && argv[i][1] == 'v' && !argv[i][2]) {
        plat_detalle = 1;
        i++;
    }
    if (i >= argc) {
        error("uso: caja-amiga [-v] programa [argumentos...]", 0);
        error("  ejecuta un programa de AmigaOS (68k, ejecutable hunk) con las bibliotecas nativas de DashOS", 0);
        return 2;
    }
    const char *ruta = argv[i++];
    size_t n = 0;
    for (; i < argc; i++) {
        n = poner_arg(n, argv[i]);
        if (i + 1 < argc && n + 2 < sizeof linea_args) linea_args[n++] = ' ';
    }
    linea_args[n++] = '\n';
    linea_args[n] = 0;

    plat_iniciar();
    di("%s (la caja Amiga en Linux)\n", motor_nombre());
    if (plat_memoria(PROGRAMA, PROGRAMA_MAX, P_ESCRIBIR) <= 0) {
        error("caja-amiga: sin memoria para el programa", 0);
        return 4;
    }
    long largo = plat_fichero_leer(ruta, (void *)PROGRAMA, PROGRAMA_MAX);
    if (largo < 0) {
        error("caja-amiga: no se puede leer ", ruta);
        return 2;
    }
    if (largo == (long)PROGRAMA_MAX) {
        error("caja-amiga: el programa es demasiado grande: ", ruta);
        return 2;
    }

    if (proyecta(M68K_RAM, M68K_RAM_LEN, P_ESCRIBIR) <= 0 || proyecta(MONTON, MONTON_LEN, P_ESCRIBIR) <= 0
        || proyecta(MONTON_RUST, MONTON_RUST_LEN, P_ESCRIBIR) <= 0) {
        error("caja-amiga: sin memoria para el 68k (¿hay algo proyectado desde $100000?)", 0);
        return 4;
    }
    tlsf = tlsf_init_with_memory((void *)MONTON, MONTON_LEN);
    tlsf_rust = tlsf_init_with_memory((void *)MONTON_RUST, MONTON_RUST_LEN);
    int e = motor_iniciar();
    if (e) return e;
    prepara_vectores();

    /* La página 0: ExecBase en la 4, escrita por PAGINA0_RW; el 68k la ve en PAGINA0_VISTA. */
    long p0 = plat_pagina0_crear(PAGINA0_RW);
    if (p0 <= 0) {
        error("caja-amiga: sin memoria para la página 0", 0);
        return 4;
    }
    uint32_t execbase = caja_rust_arrancar(M68K_RAM, M68K_RAM_LEN, PAGINA0_RW, SISTEMA, FIN_SISTEMA, CHIP);
    if (!execbase) {
        error("caja-amiga: las bibliotecas nativas no arrancan", 0);
        return 5;
    }
    uint32_t super = caja_alloc_mem(SUPER_LEN, 0);
    if (!super) {
        error("caja-amiga: sin memoria para la pila de supervisor", 0);
        return 6;
    }
    super_tope = super + SUPER_LEN;
    pon32(SUPER_BAJO, super + 512);
    caja_poner_trampa((uint32_t)TRAMPA_DEFECTO);
    long r0 = plat_pagina0_poner(p0);
    if (r0) {
        error("caja-amiga: la página 0 no se puede poner", 0);
        return 4;
    }

    plat_al_interrumpir(al_ctrl_c);

    /* El reloj de los turnos (varias tareas), con su pila: para siempre. */
    static uint8_t *pila_reloj;
    pila_reloj = (uint8_t *)(PILAS + PILA_LEN + 0x30000);
    if (proyecta((uintptr_t)pila_reloj, PILA_VIGIA_LEN, P_ESCRIBIR) <= 0
        || plat_hilo_crear(reloj_turnos, (uintptr_t)(pila_reloj + PILA_VIGIA_LEN), 20000) < 0)
        di("sin el reloj de los turnos: una tarea que no llama a nada no soltará el testigo\n");

    uint32_t d0 = 0, sobras = 0;
    int llamadas = 0;
    tarea_args = linea_args;
    int v = ejecuta_hunk(nombre_de(ruta), (const uint8_t *)PROGRAMA, (size_t)largo, execbase, &d0, &llamadas, &sobras);
    tarea_args = NULL;
    if (v == -3) {
        error("caja-amiga: no es un ejecutable de AmigaOS: ", ruta);
        return 2;
    }
    if (v != -1) {
        char num[12];
        int k = 11;
        num[k] = 0;
        unsigned u = (unsigned)(v < 0 ? -v : v);
        do num[--k] = (char)('0' + u % 10); while ((u /= 10) && k > 0);
        error("caja-amiga: el programa acabó por la excepción del 68k número ", num + k);
        return 3;
    }
    di("%s acabó con %d, %d llamadas nativas\n", nombre_de(ruta), (int)d0, llamadas);
    return (int)(d0 & 0xff);
}
