/*
    DashOS, H4: la caja Amiga, primer paso.

    Un proceso de usuario big-endian con el JIT de Emu68 dentro. Ejecuta programas 68k planos (de pruebas68k/,
    metidos en el ejecutable) y ejecutables hunk de AmigaOS (hunk.c), cada uno como un proceso de exec, con ExecBase
    en la dirección 4 (la página 0, de solo lectura, con el permiso de su llave 1). Las bibliotecas son nativas, en
    Rust (pegamento/ enlaza los crates exec, dos e intuition): cada entrada de sus tablas de saltos es
    "$A0FF <número> RTS", y el JIT convierte la línea A en una llamada a dashos_native_call (parche 0002 de
    patches/emu68), que la pasa a caja_llamada. dos va sobre el servicio de archivos (llave 3) e intuition sobre el
    compositor por Wayland (llave 2): una ventana Amiga es una ventana del escritorio.

    Memoria del proceso (docs/referencia/kernel.md, mapa de direcciones):
      $0000_0000 - $0000_0FFF  la página 0, de solo lectura: ExecBase en la dirección 4 (llamada 49 del kernel)
      $0010_0000 - $004F_FFFF  memoria del 68k (4 MB): vectores, la zona del sistema de exec, los programas planos y,
                               desde $0015_0000, lo que reparte AllocMem (caja.h)
      4 GB                     el código traducido, para escribir
      4 GB + 64 GB             el mismo código, para ejecutar (bit 36: lo pone Emu68 en mt_ARMEntryPoint)
      8 GB                     el montón general de Emu68 (TLSF)
      10 GB                    el montón de Rust (TLSF): el estado de exec, dos e intuition y los lienzos de las ventanas
      12 GB                    las pilas nativas (la del JIT y la del vigilante), fuera del alcance del 68k
      14 GB                    la página 0, para escribirla
      16 GB                    la ventana del servicio de archivos
      20 GB                    los búferes de las ventanas (wl_shm), 64 MB por ventana
      128 - 256 GB             este ejecutable: un PIE estático, en una base al azar (enlace.ld; Emu68 carga las
                               direcciones de sus funciones con 64 bits, parche 0014)
      256 - 384 GB             la pila con que lo arranca el kernel (entrada.S pasa enseguida a la de 12 GB)
    En los 4 GB bajos, los que ve el 68k, no hay nada de la caja (S28): lo comprueba caja_fuera_del_68k.

    Sin libc: las llamadas al sistema van a mano, y las funciones de cadena y memoria son las de src/support.c
    de Emu68.
*/

#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>
#include "support.h"
#include "tlsf.h"
#include "config.h"
#include "caja.h"
#include "hunk.h"

/* --- Llamadas al sistema de DashOS (docs/referencia/kernel.md) --- */

#if defined(__x86_64__)
/* x86-64 (X7): rax el número; rdi, rsi, rdx, r10 y r8 los argumentos; el resultado en rax (syscall pisa rcx y r11). */
long caja_sc(long n, long a, long b, long c, long d, long e)
{
    register long r10 __asm__("r10") = d;
    register long r8 __asm__("r8") = e;
    long r = n;
    __asm__ volatile("syscall" : "+a"(r), "+D"(a), "+S"(b), "+d"(c), "+r"(r10), "+r"(r8) :: "rcx", "r11", "memory");
    return r;
}
#else
long caja_sc(long n, long a, long b, long c, long d, long e)
{
    register long x8 __asm__("x8") = n;
    register long x0 __asm__("x0") = a;
    register long x1 __asm__("x1") = b;
    register long x2 __asm__("x2") = c;
    register long x3 __asm__("x3") = d;
    register long x4 __asm__("x4") = e;
    __asm__ volatile("svc #0" : "+r"(x0), "+r"(x1) : "r"(x8), "r"(x2), "r"(x3), "r"(x4) : "memory");
    return x0;
}
#endif
#define sc caja_sc

#define LLAVE_PAGINA0 1   /* la llave 1 de la caja: el permiso de página 0 que le da `inicio` */

#if defined(__x86_64__)
/* La frecuencia del TSC: la da el kernel en rdi al arrancar (entrada_x86.S la guarda aquí). */
uint64_t caja_frecuencia_tsc;
/* Y la de la biblioteca de Rust (dashos::FRECUENCIA), que no arranca por su `programa!` (pegamento/). */
void caja_rust_frecuencia(uint64_t f);

static uint64_t ahora_us(void)
{
    uint32_t bajo, alto;
    __asm__ volatile("lfence; rdtsc" : "=a"(bajo), "=d"(alto));
    uint64_t t = (uint64_t)alto << 32 | bajo, f = caja_frecuencia_tsc;
    return f ? t / f * 1000000 + t % f * 1000000 / f : 0;
}

static inline uintptr_t pila_de_ahora(void)
{
    uintptr_t sp;
    __asm__ volatile("mov %%rsp, %0" : "=r"(sp));
    return sp;
}
#else
static uint64_t ahora_us(void)
{
    uint64_t t, f;
    __asm__ volatile("isb; mrs %0, cntvct_el0; mrs %1, cntfrq_el0" : "=r"(t), "=r"(f));
    return f ? t / f * 1000000 + t % f * 1000000 / f : 0;   /* sin división de 128 bits (no hay libgcc) */
}

static inline uintptr_t pila_de_ahora(void)
{
    uintptr_t sp;
    __asm__ volatile("mov %0, sp" : "=r"(sp));
    return sp;
}
#endif

/* --- Salida por la consola de DashOS --- */

static char linea[256];
static int largo;

static void vaciar(void)
{
    int hecho = 0;
    while (hecho < largo) {
        long n = sc(SC_ESCRIBIR, (long)(linea + hecho), largo - hecho, 0, 0, 0);
        if (n <= 0) break;
        hecho += (int)n;
    }
    largo = 0;
}

void caja_pon_c(void *data, char c)
{
    (void)data;
    linea[largo++] = c;
    if (c == '\n' || largo == (int)sizeof(linea)) vaciar();
}

/* Lo que dice la caja: "  [caja-amiga] ..." */
void di(const char *formato, ...)
{
    va_list v;
    for (const char *m = "  [caja-amiga] "; *m; m++) caja_pon_c(NULL, *m);
    va_start(v, formato);
    vkprintf_pc(caja_pon_c, NULL, formato, v);
    va_end(v);
    if (largo) vaciar();
}

/* Lo que dice Emu68 (kprintf): callado mientras arranca, que habla mucho; después, con su marca. */
/* --- Memoria --- */

#define MONTON   0x0000000200000000UL
#define MONTON_LEN (4UL << 20)
#define MONTON_RUST 0x0000000280000000UL   /* el montón de Rust, aparte del de Emu68 */
#define MONTON_RUST_LEN (8UL << 20)
#define MONTON_RUST_MAX (512UL << 20)      /* hasta dónde puede crecer (ver caja_monton_reservar) */
#define PILAS    0x0000000300000000UL
#define PILA_LEN (1UL << 20)
#define PILA_VIGIA_LEN (64UL << 10)
#define PAGINA0_RW 0x0000000380000000UL   /* la página 0, para escribirla: por encima de 4 GB, el 68k no llega */


volatile uint32_t m68k_ram_fin = M68K_RAM + M68K_RAM_LEN;

/* Un objeto de memoria nuevo, proyectado en `va`. Devuelve su llave, o un error negativo. */
static long proyecta(uintptr_t va, size_t len, int permisos)
{
    long m = sc(SC_MEMORIA_CREAR, (long)len, 0, 0, 0, 0);
    if (m <= 0) return m ? m : -1;
    long r = sc(SC_MEMORIA_PROYECTAR, m, (long)va, permisos, 0, 0);
    return r ? r : m;
}

/* La memoria del 68k crece (lo pide exec cuando un AllocMem no cabe): `memoria_crear` reserva entera la memoria del
   objeto y la cobra al cupo del proceso, así que se crece a trozos, justo detrás de lo que hay: lo que haga falta
   (`minimo` y algo más), y al menos una cuarta parte de lo que ya hay (para no pedir de poco en poco), en objetos de
   16 MB como mucho (el máximo de un objeto). Nunca más allá de `tope` bytes desde M68K_RAM ni de M68K_RAM_MAX. Si el
   cupo no da, se queda en lo que haya conseguido. Devuelve el final nuevo (el de antes si no se pudo nada). Lo
   crecido libre lo vuelve a repartir exec; con memoria baja en DashOS (aviso-cajas), lo de arriba que esté libre se
   devuelve (`caja_encoger_ram`). */
#define TROZO_RAM     (1UL << 20)
#define TROZO_RAM_MAX (16UL << 20)
/* aviso-cajas: los objetos con que ha crecido la memoria del 68k, en orden (dirección, largo y llave). */
#define MAX_CRECIDOS 256
static struct { uint64_t va, largo; long llave; } crecidos[MAX_CRECIDOS];
static unsigned n_crecidos;
uint32_t caja_ampliar_ram(uint32_t minimo, uint32_t tope)
{
    uint64_t fin = m68k_ram_fin;
    uint64_t techo = (uint64_t)M68K_RAM + tope;
    if (techo > M68K_RAM_MAX) techo = M68K_RAM_MAX;
    /* Lo que no cabe ni con todo lo que queda, ni se intenta: un AllocMem absurdo no hace crecer la caja hasta el tope. */
    if (fin >= techo || (uint64_t)minimo > techo - fin) return (uint32_t)fin;
    uint64_t quiero = ((uint64_t)minimo + (64UL << 10) + TROZO_RAM - 1) & ~(uint64_t)(TROZO_RAM - 1);
    uint64_t cuarto = ((fin - M68K_RAM) / 4 + TROZO_RAM - 1) & ~(uint64_t)(TROZO_RAM - 1);
    if (quiero < cuarto) quiero = cuarto;
    if (fin + quiero > techo) quiero = techo - fin;
    while (quiero > 0) {
        uint64_t n = quiero < TROZO_RAM_MAX ? quiero : TROZO_RAM_MAX;
        long m = proyecta((uintptr_t)fin, (size_t)n, P_ESCRIBIR);
        if (m <= 0) break;
        /* Sin sitio para apuntarlo, no se podrá devolver (ni lo de debajo): se queda, como antes. */
        if (n_crecidos < MAX_CRECIDOS) {
            crecidos[n_crecidos].va = fin;
            crecidos[n_crecidos].largo = n;
            crecidos[n_crecidos].llave = m;
        }
        n_crecidos++;
        fin += n;
        quiero -= n;
    }
    if (fin > m68k_ram_fin) motor_memoria(m68k_ram_fin, (uint32_t)fin, 1);
    __atomic_store_n(&m68k_ram_fin, (uint32_t)fin, __ATOMIC_RELEASE);
    return (uint32_t)fin;
}

/* aviso-cajas: devuelve a DashOS los objetos crecidos de arriba del todo que empiezan en `desde` o más arriba (exec ya
   no reparte esa memoria: está libre). Primero baja el final (nadie mira ya ahí) y después los quita y cierra su llave:
   sus páginas vuelven al sistema. Devuelve el final nuevo. */
uint32_t caja_encoger_ram(uint32_t desde)
{
    uint64_t fin = m68k_ram_fin;
    unsigned n = n_crecidos;
    while (n > 0 && n <= MAX_CRECIDOS && crecidos[n - 1].va >= desde && crecidos[n - 1].va + crecidos[n - 1].largo == fin) {
        fin = crecidos[n - 1].va;
        n--;
    }
    if (n == n_crecidos) return (uint32_t)fin;
    motor_memoria((uint32_t)fin, m68k_ram_fin, 0);
    __atomic_store_n(&m68k_ram_fin, (uint32_t)fin, __ATOMIC_RELEASE);
    while (n_crecidos > n) {
        n_crecidos--;
        sc(SC_MEMORIA_QUITAR, (long)crecidos[n_crecidos].va, 0, 0, 0, 0);
        sc(SC_CERRAR, crecidos[n_crecidos].llave, 0, 0, 0, 0);
    }
    return (uint32_t)fin;
}

/* El SP con que arrancó el kernel la caja: el tope de la pila del ELF. */
static uintptr_t pila_elf;

/* La llama entrada.S antes que nada (con el SP del kernel): el tope de una pila propia, en 12 GB. 0 si no hay. */
uintptr_t caja_pila(uintptr_t sp_elf)
{
    pila_elf = sp_elf;
    return proyecta(PILAS, PILA_LEN, P_ESCRIBIR) > 0 ? PILAS + PILA_LEN : 0;
}

/* Lo que Rust (pegamento/) usa como memoria: un TLSF propio, para que lo suyo no se mezcle con lo del JIT. Empieza con
   MONTON_RUST_LEN y crece como la memoria del 68k (los píxeles de las ventanas y de los BitMap nativos viven aquí:
   Tachyon pide varios de 1,5 MB): si lo pedido no cabe, objetos nuevos justo detrás de lo que hay, lo que haga falta y
   al menos la mitad de lo que ya hay, en objetos de 16 MB como mucho, hasta MONTON_RUST_MAX. Si el cupo no da, NULL:
   Rust lo trata como falta de memoria (un lienzo sin píxeles, una función que falla) sin caerse. */
static void *tlsf_rust;
static uint64_t monton_rust_fin = MONTON_RUST + MONTON_RUST_LEN;

static int monton_rust_crecer(size_t tam, size_t alin)
{
    uint64_t techo = MONTON_RUST + MONTON_RUST_MAX;
    uint64_t quiero = ((uint64_t)tam + alin + (64UL << 10) + TROZO_RAM - 1) & ~(uint64_t)(TROZO_RAM - 1);
    uint64_t mitad = ((monton_rust_fin - MONTON_RUST) / 2 + TROZO_RAM - 1) & ~(uint64_t)(TROZO_RAM - 1);
    if (quiero < mitad) quiero = mitad;
    if (monton_rust_fin >= techo || (uint64_t)tam > techo - monton_rust_fin) return 0;
    if (quiero > techo - monton_rust_fin) quiero = techo - monton_rust_fin;
    uint64_t desde = monton_rust_fin, hecho = 0;
    while (hecho < quiero) {
        uint64_t n = quiero - hecho < TROZO_RAM_MAX ? quiero - hecho : TROZO_RAM_MAX;
        if (proyecta((uintptr_t)(desde + hecho), (size_t)n, P_ESCRIBIR) <= 0) break;
        hecho += n;
    }
    if (hecho == 0) return 0;
    tlsf_add_memory(tlsf_rust, (void *)desde, hecho);
    monton_rust_fin = desde + hecho;
    return 1;
}

void *caja_monton_reservar(size_t tam, size_t alin)
{
    if (!tlsf_rust) return NULL;
    if (alin < 8) alin = 8;
    void *p = tlsf_malloc_aligned(tlsf_rust, tam, alin);
    if (!p && monton_rust_crecer(tam, alin)) p = tlsf_malloc_aligned(tlsf_rust, tam, alin);
    return p;
}

void caja_monton_soltar(void *p)
{
    if (p && tlsf_rust) tlsf_free(tlsf_rust, p);
}

/* Lo que core de Rust pide y support.c de Emu68 no tiene. */
int memcmp(const void *a, const void *b, size_t n)
{
    const uint8_t *x = a, *y = b;
    for (size_t i = 0; i < n; i++)
        if (x[i] != y[i]) return x[i] < y[i] ? -1 : 1;
    return 0;
}

/* --- Las llamadas nativas: todas a las bibliotecas de Rust --- */

/* volatile: los cambia dashos_native_call, que llama el código traducido desde dentro del asm de `arranca`; sin él, gcc
   (análisis entre funciones) da por hecho que `arranca` no los toca y no los vuelve a leer. */
/* CAJA_INTERACTIVA=1 al compilar (make -C caja CAJA_INTERACTIVA=1): las pruebas que esperan a alguien, para sacar
   capturas: GadTools espera 90 s y un programa de Dash sin consola saca su error en un requester. */
#ifndef CAJA_INTERACTIVA
#define CAJA_INTERACTIVA 0
#endif
#define GT_NOMBRE (CAJA_INTERACTIVA ? "gadtools-90" : "gadtools-5")
/* BOOPSI y ReAction (boopsi.c): sin esperar; en el modo de las capturas, su ventana de ReAction 20 s con cada aspecto
   (el segundo, con la excepción "original" de DASHOS_PREFS para ese nombre). */
#define BOOPSI_NOMBRE (CAJA_INTERACTIVA ? "boopsi-20" : "boopsi-0")

/* Lo más que una tarea espera sin que llegue nada antes de que la caja la corte (pegamento/: esperar). */
uint64_t caja_espera_maxima_us(void) { return (CAJA_INTERACTIVA ? 600ULL : 60ULL) * 1000000ULL; }

static volatile int llamadas_nativas;
static volatile int rechazadas;
static volatile int cortadas;

/* --- Llamadas anidadas: código 68k desde una función nativa --- */

/* Desde una función nativa se ejecuta 68k otra vez (caja_ejecutar_68k): un nivel más, que el motor corre dentro del
   de fuera (motor_anidado) y del que vuelve con el de fuera como estaba.

   El nivel acaba cuando el 68k vuelve a VUELTA_ANIDADA (la dirección de vuelta que se le pone): la función nativa
   N_VUELTA lo apunta y el código salta a la dirección 0, donde acaba ese nivel. Si acaba de otra forma (el programa
   acabó dentro, una excepción sin tc_TrapCode, el vigilante lo cortó), el programa entero se da por acabado y se
   deshacen los niveles de fuera: cada uno vuelve con ANIDADA_ACABADO, su función nativa corta el programa (la NMI) y
   su nivel acaba por el vector 31; ninguna llamada nativa más se ejecuta entretanto. */
#define MAX_ANIDADO 16
static volatile int anidado;                  /* niveles abiertos */
static uint32_t dueno_nivel[MAX_ANIDADO + 1];  /* la tarea 68k que abrió cada nivel */
static volatile int turnos_activos;           /* hay varias tareas: el reloj mira el turno (más abajo) */
static volatile int vuelto[MAX_ANIDADO + 1];  /* 1: volvió a su VUELTA; 2: volvió con la pila más arriba (rota) */
static uint32_t a7_esperado[MAX_ANIDADO + 1];
static volatile int programa_acabado;         /* acabó dentro de un nivel: se deshacen todos */
static volatile uint32_t fin_marca, fin_d0;   /* cómo acabó (MARCA y D0 de ese momento) */
static volatile int pilas_raras;
static volatile int ctrl_c_pendiente;

void caja_nativa(motor_regs *regs, uint32_t index)
{
    if (index == N_VUELTA) {
        /* Vuelve el nivel de dentro. Con la pila donde se le dejó, bien; más abajo (dejó algo en ella), se acepta: la
           de fuera está intacta. Más arriba, ha deshecho marcos de fuera (un longjmp, otro nivel): se corta. Sin
           niveles (un programa que salta aquí), su código sigue a la dirección 0 y el programa acaba.
           Varias tareas: el nivel de arriba es de la tarea que lo abrió. Si llega aquí otra (vuelve de un nivel suyo
           de más abajo, con otra tarea corriendo dentro del de arriba), se aparca hasta que se cierren los de encima,
           y corre otra (caja_aparcar_en_vuelta, con sus registros). Lo que sigue a la llamada es un RTS
           (sistema.s): para acabar el nivel, la pila lleva un 0 encima. */
        int n = anidado;
        if (n > 0 && !programa_acabado) {
            uint32_t t = caja_tarea_en_marcha();
            if (t != dueno_nivel[n]) {
                if (caja_aparcar_en_vuelta(regs, (uint32_t)VUELTA_ANIDADA) == 0) return;
                cortadas++;
                motor_cortar();
            } else {
                uint32_t a7 = regs->a[7];
                if (a7 > a7_esperado[n]) vuelto[n] = 2;
                else { vuelto[n] = 1; if (a7 < a7_esperado[n]) pilas_raras++; }
            }
        }
        uint32_t a7 = regs->a[7];
        if (a7 >= 8 && en_ram68k(a7 - 4, 4)) a7 -= 4;
        else a7 = (uint32_t)(PILA_TOPE - 4);
        pon32(a7, 0);
        regs->a[7] = a7;
        return;
    }
    if (index == N_CAIDA) {
        /* Una tarea cayó por una excepción sin tc_TrapCode (caida, en modo supervisor, con [n.l][marco] en la pila). Si
           cayó en modo de usuario y no es la del programa, la caja la quita y deja en regs los registros de la
           siguiente: su pila de usuario pasa a USP y, encima del marco de la excepción (que puede ser largo: se mira su
           formato), uno corto que vuelve en modo de usuario al RTS_SUELTO, que la retoma. Así la pila de supervisor
           queda como antes de la excepción. */
        uint32_t sp = regs->a[7];
        uint32_t marco = sp + 4;
        static const uint8_t largos[16] = { 8, 8, 12, 12, 16, 0, 0, 60, 0, 20, 32, 92, 0, 0, 0, 0 };
        if (programa_acabado || !en_ram68k(marco, 8) || (lee16(marco) & SR_S)) return;
        uint32_t largo = largos[lee16(marco + 6) >> 12];
        if (!largo || !en_ram68k(marco, largo)) return;
        if (caja_caida(regs, lee16(sp + 2)) != 1) return;
        uint32_t usp = regs->a[7];
        uint32_t f = marco + largo - 8;
        pon16(f, 0);                                   /* SR: usuario, sin máscara */
        pon32(f + 2, (uint32_t)RTS_SUELTO);
        pon16(f + 6, 0);                               /* formato 0 */
        regs->a[7] = f;
        motor_poner_usp(usp);
        pon16(MARCA, 0xffff);
        pon16(CAMBIO, 1);
        return;
    }
    if (programa_acabado) {
        /* Se está deshaciendo: nada más se ejecuta, y este nivel acaba en cuanto vuelva a su bucle. */
        regs->d[0] = 0;
        motor_cortar();
        return;
    }
    if (index == N_INTERRUPCION) motor_interrupcion_atendida();
    llamadas_nativas++;
    switch (caja_llamada(regs, index)) {
        case 1:   /* nadie tiene ese número: rechazada, como una función que falla, y el programa sigue */
            rechazadas++;
            llamadas_nativas--;
            break;
        case 2:   /* la tarea se quitó o esperaba sin remedio: la interrupción de nivel 7 corta el programa */
            cortadas++;
            motor_cortar();
            break;
        default:
            /* La interrupción de la caja con una tarea en modo de usuario (el marco, encima de la pila de supervisor:
               SR, PC, formato): si acabó su turno, la caja le quita el testigo y deja los registros de la siguiente;
               el marco pasa a volver en modo de usuario al RTS_SUELTO, que la retoma, con su pila en USP (como en
               caida). */
            if (index == N_INTERRUPCION && turnos_activos) {
                uint32_t f = regs->a[7], usp_antes, isp, msp;
                motor_pilas(&usp_antes, &isp, &msp);
                if (en_ram68k(f, 8) && !(lee16(f) & SR_S) && (lee16(f + 6) >> 12) <= 1
                    && caja_turno(regs, usp_antes, lee32(f + 2), lee16(f), (uint32_t)RTR_SUELTO) == 1) {
                    uint32_t usp = regs->a[7];
                    pon16(f, 0);
                    pon32(f + 2, (uint32_t)RTS_SUELTO);
                    pon16(f + 6, 0);
                    regs->a[7] = f;
                    motor_poner_usp(usp);
                }
            }
            break;
    }
}

int caja_ejecutar_68k(uint32_t pc, uint32_t *r, int usuario)
{
    if (programa_acabado) return ANIDADA_ACABADO;
    if (anidado >= MAX_ANIDADO) return ANIDADA_TOPE;
    uint32_t sr = motor_sr(), usp, isp, msp;
    motor_pilas(&usp, &isp, &msp);
    uint32_t a7 = r[15];
    if (usuario && (sr & SR_S)) { a7 = usp; sr &= ~(SR_S | SR_M); }
    a7 = (a7 - 4) & ~1u;
    /* Sitio en la pila para la vuelta y algo más: si no, no se ejecuta (el 68k escribiría fuera de su memoria). */
    if (a7 < M68K_RAM + 256 || !en_ram68k(a7, 4)) return ANIDADA_PILA;
    pon32(a7, (uint32_t)VUELTA_ANIDADA);

    motor_regs regs;
    for (int i = 0; i < 8; i++) { regs.d[i] = r[i]; regs.a[i] = r[8 + i]; }
    regs.a[7] = a7;
    if (sr & SR_S) { if (sr & SR_M) msp = a7; else isp = a7; } else usp = a7;

    uint32_t dueno = caja_tarea_en_marcha();
    int n = ++anidado;
    dueno_nivel[n] = dueno;
    vuelto[n] = 0;
    a7_esperado[n] = a7 + 4;
    motor_anidado(pc, &regs, sr, usp, isp, msp);
    anidado = n - 1;
    /* Cerrado este nivel, la dueña del de debajo puede estar aparcada en su vuelta: ya puede seguir. */
    if (n - 1 > 0) caja_desaparcar(dueno_nivel[n - 1]);

    for (int i = 0; i < 8; i++) { r[i] = regs.d[i]; r[8 + i] = regs.a[i]; }
    int v = vuelto[n];
    if (v != 1 && !programa_acabado) {
        programa_acabado = 1;
        fin_marca = v == 2 ? 0xfffe : lee16(MARCA);
        fin_d0 = regs.d[0];
    }
    return v == 1 ? ANIDADA_BIEN : ANIDADA_ACABADO;
}

int caja_tomar_ctrl_c(void) { return __atomic_exchange_n(&ctrl_c_pendiente, 0, __ATOMIC_ACQ_REL); }

/* El reloj de los turnos: con varias tareas, un hilo levanta la interrupción de nivel 6 cada 20 ms; en ella, la caja
   mira si la tarea en marcha acabó su turno (una que no llama a nada no lo suelta de otra forma). */
static volatile int turnos_activos;
void caja_turnos(int si) { turnos_activos = si; }
static void reloj_turnos(uint64_t us)
{
    for (;;) {
        sc(SC_DORMIR, (long)us, 0, 0, 0, 0);
        if (turnos_activos) motor_interrumpir();
    }
}

int caja_tomar_pilas_raras(void) { int n = pilas_raras; pilas_raras = 0; return n; }

/* --- Varias tareas 68k: lo que el planificador de pegamento/ (multitarea.rs) pide al hilo del JIT --- */

/* 1 si el 68k está en modo de usuario (solo entonces se cambia de tarea en una llamada nativa). */
int caja_modo_usuario(void) { return !(motor_sr() & SR_S); }

/* 1 si la tarea `t` tiene abierto algún nivel de llamadas anidadas (no se la puede quitar: su MainLoop está en la pila
   del hilo). */
int caja_tiene_nivel(uint32_t t)
{
    for (int i = 1; i <= anidado && i <= MAX_ANIDADO; i++)
        if (dueno_nivel[i] == t) return 1;
    return 0;
}

/* --- Los programas 68k --- */

extern const uint8_t prog_hola[], prog_hola_fin[];
extern const uint8_t prog_lineaa[], prog_lineaa_fin[];
extern const uint8_t prog_bucle[], prog_bucle_fin[];
extern const uint8_t prog_mala[], prog_mala_fin[];
extern const uint8_t prog_saludo[], prog_saludo_fin[];
extern const uint8_t prog_ventana[], prog_ventana_fin[];
extern const uint8_t prog_hostil[], prog_hostil_fin[];
extern const uint8_t prog_dash_hola[], prog_dash_hola_fin[];
extern const uint8_t prog_dash_texto[], prog_dash_texto_fin[];
extern const uint8_t prog_dash_texto_gcc[], prog_dash_texto_gcc_fin[];
extern const uint8_t prog_dash_ventana[], prog_dash_ventana_fin[];
extern const uint8_t prog_dash_interp[], prog_dash_interp_fin[];
extern const uint8_t prog_dash_listas[], prog_dash_listas_fin[];
extern const uint8_t prog_anidado[], prog_anidado_fin[];
extern const uint8_t prog_gadtools[], prog_gadtools_fin[];
extern const uint8_t prog_boopsi[], prog_boopsi_fin[];
extern const uint8_t prog_ramlib[], prog_ramlib_fin[];
extern const uint8_t prog_capa[], prog_capa_fin[];
extern const uint8_t prog_glotona[], prog_glotona_fin[];
extern const uint8_t prog_memh[], prog_memh_fin[];
extern const uint8_t prog_procesos[], prog_procesos_fin[];
extern const uint8_t prog_ils[], prog_ils_fin[];
extern const uint8_t prog_dtprueba[], prog_dtprueba_fin[];
extern const uint8_t prog_cgx[], prog_cgx_fin[];
extern const uint8_t prog_dash_test_reaction[], prog_dash_test_reaction_fin[];
extern const uint8_t prog_dash_test_doc_reaction[], prog_dash_test_doc_reaction_fin[];
extern const uint8_t prog_dash_ctrl_c[], prog_dash_ctrl_c_fin[];
extern const uint8_t prog_dash_ctrl_c_gcc[], prog_dash_ctrl_c_gcc_fin[];
extern const uint8_t prog_dash_ctrl_c_for[], prog_dash_ctrl_c_for_fin[];
extern const uint8_t prog_dash_trap_ilegal[], prog_dash_trap_ilegal_fin[];
extern const uint8_t prog_dash_trap_divcero[], prog_dash_trap_divcero_fin[];
extern const uint8_t prog_dash_test_gadtools[], prog_dash_test_gadtools_fin[];

/* Los vectores. Las excepciones del procesador van al despacho de sistema.s ("move.l #i,-(sp); jmp DESPACHO"), que
   hace lo de exec: a tc_TrapCode de la tarea con el número encima del marco; si la tarea no tiene, el programa acaba
   por esa excepción (TRAMPA_DEFECTO: MARCA = i y a COMUN). Las interrupciones de los niveles 1 a 5 y 7 (el vigilante)
   acaban el programa sin más ("move.w #i,MARCA; jmp COMUN"); la de nivel 6 es la de la caja (INTERRUPCION68K). COMUN
   pone una pila y vuelve a la dirección 0, donde MainLoop termina. Al volver de su función, cada tarea salta también a
   COMUN (su dirección de vuelta). */
extern const uint8_t prog_sistema[], prog_sistema_fin[];
/* La foto de los vectores (S28): dos tramos que solo escribe prepara_vectores: los vectores, las trampas y COMUN
   ([VECTORES, COMUN + 16); MARCA y la pila de COMUN cambian siempre) y el código de sistema.s ([SISTEMA68K, SUPER_BAJO);
   SUPER_BAJO lo pone caja_main después). */
static const struct { uintptr_t desde, hasta; } TRAMOS_FOTO[] = {
    { VECTORES, COMUN + 16 }, { SISTEMA68K, SUPER_BAJO }, { CAIDA68K, FIN_SISTEMA68K },
};
static uint8_t vectores_foto[(COMUN + 16 - VECTORES) + (SUPER_BAJO - SISTEMA68K) + (FIN_SISTEMA68K - CAIDA68K)];
static int vectores_deshechos;
static void prepara_vectores(void)
{
    for (uint32_t i = 0; i < 256; i++) {
        uintptr_t t = TRAMPAS + 14 * i;
        pon32(VECTORES + 4 * i, (uint32_t)t);
        if (i >= 24 && i <= 31) {
            pon16(t, 0x33fc); pon16(t + 2, (uint16_t)i);                      /* move.w #i,MARCA */
            pon32(t + 4, (uint32_t)MARCA);
            pon16(t + 8, 0x4ef9); pon32(t + 10, (uint32_t)COMUN);             /* jmp COMUN */
        } else {
            pon16(t, 0x2f3c); pon32(t + 2, i);                                 /* move.l #i,-(sp) */
            pon16(t + 6, 0x4ef9); pon32(t + 8, (uint32_t)DESPACHO);           /* jmp DESPACHO */
        }
    }
    pon32(VECTORES + 4 * 30, (uint32_t)INTERRUPCION68K);
    pon16(COMUN, 0x4ff9); pon32(COMUN + 2, (uint32_t)(PILA_TOPE - 4));      /* lea PILA_TOPE-4,a7 */
    pon16(COMUN + 6, 0x4297);                                                  /* clr.l (a7) */
    pon16(COMUN + 8, 0x4e75);                                                  /* rts */
    uint8_t *d = (uint8_t *)SISTEMA68K;
    for (const uint8_t *p = prog_sistema; p < prog_sistema_fin && d < (uint8_t *)FIN_SISTEMA68K; ) *d++ = *p++;
    pon16(CAMBIO, 0);
    uint32_t k = 0;
    for (unsigned t = 0; t < 3; t++)
        for (uintptr_t d = TRAMOS_FOTO[t].desde; d < TRAMOS_FOTO[t].hasta; d++) vectores_foto[k++] = *(volatile uint8_t *)d;
}

/* S28: los vectores del 68k, las trampas, COMUN y el código de sistema.s, como los dejó prepara_vectores. Un programa
   los puede cambiar (están en la memoria del 68k, que no tiene protección dentro de la caja): al acabar, vuelven a como
   estaban. Cuántas palabras cambió. */
static int vectores_deshacer(void)
{
    int n = 0;
    uint32_t k = 0;
    for (unsigned t = 0; t < 3; t++)
        for (uintptr_t d = TRAMOS_FOTO[t].desde; d < TRAMOS_FOTO[t].hasta; d += 4, k += 4) {
            uint32_t f = (uint32_t)vectores_foto[k] << 24 | (uint32_t)vectores_foto[k + 1] << 16
                | (uint32_t)vectores_foto[k + 2] << 8 | vectores_foto[k + 3];
            if (lee32(d) != f) { pon32(d, f); n++; }
        }
    return n;
}

/* La pila de supervisor: la de las excepciones y las interrupciones (los programas corren en modo de usuario, como
   las tareas de exec). De AllocMem, una vez. */
#define SUPER_LEN 16384
static uint32_t super_tope;

/* Ejecuta el 68k desde `pc` con A7 = `a7`, A6 = `a6`, A0 = `a0` y D0 = `d0` (el resto a 0), en modo de usuario (la
   pila de supervisor, la de la caja) y sin máscara de interrupciones, hasta que vuelve a la dirección 0. Devuelve el
   vector por el que acabó, -1 si acabó con su RTS, o -2 si una llamada anidada volvió deshaciendo marcos de fuera. Si
   el programa acabó dentro de una llamada anidada, lo de ese momento (su vector y su D0). */
/* D0 al acabar el último programa (el de dentro de una llamada anidada, si acabó ahí). */
static uint32_t d0_final;

static int arranca(uint32_t pc, uint32_t a7, uint32_t a6, uint32_t a0, uint32_t d0)
{
    pon16(MARCA, 0xffff);
    anidado = 0;
    programa_acabado = 0;
    motor_arrancar(pc, a7, a6, a0, d0, super_tope, VECTORES);
    d0_final = motor_d0();
    uint16_t marca = lee16(MARCA);
    if (programa_acabado) {
        marca = (uint16_t)fin_marca;
        d0_final = fin_d0;
        if (marca == 0xfffe) return -2;
    }
    return marca == 0xffff ? -1 : marca;
}

static size_t largo_de(const char *s) { size_t n = 0; while (s[n]) n++; return n; }

/* Ejecuta código 68k como un proceso de exec llamado `nombre`, con A6 = `a6`, A0 los argumentos y D0 su largo (como
   desde el CLI). Al acabar, la caja suelta lo que el programa dejara (ficheros, ventanas, memoria): en *sobras,
   cuántos bloques de memoria no soltó él. Devuelve el vector por el que acabó, -1 si con su RTS, o -2 si no se pudo
   crear el proceso. */
static volatile int cerrar_pendiente;   /* (más abajo, con manda_ctrl_c) */
/* Los argumentos del programa siguiente (NULL: ninguno, solo "\n"), y si lo que deje pasa al sistema en vez de
   soltarse (caja_memoria_al_sistema: AddDataTypes deja sus tipos a datatypes); en tarea_pasadas, cuántos bloques. */
static const char *tarea_args;
static int tarea_al_sistema;
static uint32_t tarea_pasadas;
uint32_t caja_memoria_al_sistema(uint32_t t);
int caja_fichero_de_dos(const char *ruta, const uint8_t **datos, uint32_t *largo);
static int ejecuta_tarea(const char *nombre, uint32_t pc, uint32_t a6, uint32_t seglist, uint32_t *sobras)
{
    const char *args = tarea_args ? tarea_args : "\n";
    uint32_t largo_args = (uint32_t)largo_de(args) + 1;
    uint32_t args68k = caja_alloc_mem(largo_args, 0);
    if (args68k) for (unsigned i = 0; i < largo_args; i++) pon8(args68k + i, (uint8_t)args[i]);
    uint32_t sp = 0;
    /* Lo que se pidió para el programa de antes no es para este. */
    __atomic_store_n(&ctrl_c_pendiente, 0, __ATOMIC_RELEASE);
    __atomic_store_n(&cerrar_pendiente, 0, __ATOMIC_RELEASE);
    uint32_t t = caja_tarea_nueva(nombre, largo_de(nombre), 16384, pc, (uint32_t)COMUN, args68k, seglist, &sp);
    if (!t) { if (args68k) caja_free_mem(args68k, largo_args); return -2; }
    int v = arranca(pc, sp, a6, args68k, largo_args - 1);
    vectores_deshechos = vectores_deshacer();
    if (vectores_deshechos) di("defensa: %d vectores del 68k cambiados por el programa: deshechos\n", vectores_deshechos);
    tarea_pasadas = tarea_al_sistema ? caja_memoria_al_sistema(t) : 0;
    uint32_t s = caja_tarea_fin(t);
    if (sobras) *sobras = s;
    if (args68k) caja_free_mem(args68k, largo_args);
    return v;
}

/* El JIT olvida lo traducido desde una función nativa (lo pide pegamento/). */
void caja_jit_olvidar(void) { motor_olvidar_en_nativa(); }

/* Carga un programa plano en `donde` (su dirección de enlace, en la zona PLANOS) y lo ejecuta con A6 = DOSBase. */
static int ejecuta(const char *nombre, const uint8_t *desde, const uint8_t *hasta, uintptr_t donde)
{
    uint8_t *d = (uint8_t *)donde;
    for (const uint8_t *s = desde; s < hasta; ) *d++ = *s++;
    motor_olvidar();
    return ejecuta_tarea(nombre, (uint32_t)donde, caja_dosbase(), 0, NULL);
}

/* --- Ejecutables hunk (hunk.c): su memoria, de AllocMem de exec --- */

static int hunks_vivos;

static uint32_t hunks_reservar(void *ctx, uint32_t bytes, uint32_t atributos)
{
    (void)ctx;
    uint32_t d = caja_alloc_mem(bytes, atributos);
    if (d) hunks_vivos++;
    return d;
}

static void hunks_liberar(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    hunks_vivos--;
    caja_free_mem(d, bytes);
}

static uint8_t *hunks_ver(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    return en_ram68k(d, bytes) ? (uint8_t *)(uintptr_t)d : NULL;
}

static const struct hunk_ops hunks_ops = { NULL, hunks_reservar, hunks_liberar, hunks_ver };

/* Para las pruebas hostiles del cargador (decenas de miles de cargas): un reparto que solo avanza, dentro de un bloque
   de AllocMem, como el de antes de exec. Con AllocMem en cada carga tardaban segundos, y con la máquina cargada eso
   retrasaba a los demás servicios del arranque. Una tanda más corta va con AllocMem de verdad. */
static uint32_t rapido_inicio, rapido_fin, rapido_libre;

static uint32_t rapido_reservar(void *ctx, uint32_t bytes, uint32_t atributos)
{
    (void)ctx; (void)atributos;
    if (bytes > rapido_fin - rapido_libre) return 0;
    uint32_t d = rapido_libre;
    rapido_libre += (bytes + 7) & ~7u;
    hunks_vivos++;
    return d;
}

static void rapido_liberar(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    hunks_vivos--;
    if (d + ((bytes + 7) & ~7u) == rapido_libre) rapido_libre = d;
}

static const struct hunk_ops rapido_ops = { NULL, rapido_reservar, rapido_liberar, hunks_ver };
#define MAX_HUNKS 256
static struct hunk_seg hunks_segs[MAX_HUNKS];

/* Una copia de un ejecutable para estropearla (las pruebas hostiles). */
static uint8_t hunk_copia[16384];
static uint64_t semilla = 0x9E3779B97F4A7C15ULL;
static uint32_t azar(void) { semilla ^= semilla << 13; semilla ^= semilla >> 7; semilla ^= semilla << 17; return (uint32_t)semilla; }

/* Carga un ejecutable hunk y lo ejecuta como un proceso, con A6 = ExecBase. Devuelve el vector como ejecuta_tarea, o
   -3 si no carga. En *d0 lo que devolvió el programa; en *llamadas, sus llamadas nativas. */
static int ejecuta_hunk(const char *nombre, const uint8_t *datos, size_t largo, uint32_t execbase, uint32_t *d0,
                        int *llamadas, uint32_t *sobras)
{
    struct hunk_carga c;
    int e = hunk_cargar(datos, largo, &hunks_ops, hunks_segs, MAX_HUNKS, &c);
    if (e != HUNK_BIEN || hunks_segs[0].tipo != HUNK_CODE) {
        di("%s no carga: %s\n", nombre, hunk_error(e));
        return -3;
    }
    di("%s: %u hunks (código %u, datos %u, BSS %u bytes) en $%x, %u reubicaciones\n", nombre, c.cuantos,
       hunks_segs[0].bytes, c.cuantos > 1 ? hunks_segs[1].bytes : 0, c.cuantos > 2 ? hunks_segs[2].bytes : 0,
       c.entrada, c.relocs);
    llamadas_nativas = 0;
    motor_olvidar();
    int v = ejecuta_tarea(nombre, c.entrada, execbase, c.seglist, sobras);
    *d0 = d0_final;
    *llamadas = llamadas_nativas;
    hunk_soltar(&hunks_ops, &c);
    return v;
}

/* --- El vigilante: corta un programa 68k que no acaba --- */

/* Un hilo aparte: duerme y después levanta la interrupción de nivel 7 del 68k (INTF.ARM_err, la NMI). El bucle
   traducido mira INT64 en cada vuelta y vuelve a MainLoop, que salta al vector 31: su trampa acaba el programa. */
static void vigila(uint64_t us)
{
    sc(SC_DORMIR, (long)us, 0, 0, 0, 0);
    motor_cortar();
    sc(SC_SALIR, 0, 0, 0, 0, 0);
    for (;;) {}
}

/* --- S28: los virus de laboratorio (pruebas68k/malware/, docs/diseño/S28-malware-amiga.md) --- */

extern const uint8_t prog_mal_arranque[], prog_mal_arranque_fin[];
extern const uint8_t prog_mal_enlace[], prog_mal_enlace_fin[];
extern const uint8_t prog_mal_parche[], prog_mal_parche_fin[];
extern const uint8_t prog_mal_residente[], prog_mal_residente_fin[];
extern const uint8_t prog_mal_guion[], prog_mal_guion_fin[];
extern const uint8_t prog_mal_troyano[], prog_mal_troyano_fin[];
extern const uint8_t prog_mal_memoria[], prog_mal_memoria_fin[];
extern const uint8_t prog_actualiza[], prog_actualiza_fin[];
/* De pegamento/ (Rust): lo que se deshizo tras el último programa, y las negativas de la guardia de sus ficheros; y
   cuántas veces preguntó la guardia al usuario (por `sesiones`) y cómo se decidió la última. */
void caja_defensa(uint32_t *entradas, uint32_t *campos);
uint32_t caja_negados(void);
uint32_t caja_preguntas(uint32_t *como);

/* Cómo se decidió una pregunta (usuario/src/preguntas_ficheros.rs). */
enum { COMO_SIN_PREGUNTAR, COMO_RECORDADO, COMO_SIEMPRE, COMO_ESTA_VEZ, COMO_DIJO_NO, COMO_OCUPADO, COMO_NADA = 0xff };
static const char *como_texto(uint32_t c)
{
    switch (c) {
    case COMO_SIN_PREGUNTAR: return "nadie a quien preguntar: no";
    case COMO_RECORDADO: return "sí, lo recuerda sesiones";
    case COMO_SIEMPRE: return "el usuario dice sí, siempre";
    case COMO_ESTA_VEZ: return "el usuario dice sí, solo esta vez";
    case COMO_DIJO_NO: return "el usuario dice no";
    case COMO_OCUPADO: return "ya preguntaba otra cosa: no";
    case COMO_NADA: return "sin preguntas";
    default: return "?";
    }
}
static int como_deja(uint32_t c) { return c == COMO_RECORDADO || c == COMO_SIEMPRE || c == COMO_ESTA_VEZ; }

/* Donde los virus de laboratorio cuentan cuántas veces se usaron sus parches y su código residente: una zona que exec
   no reparte (la de los programas planos, sin usar aquí). */
#define ZONA_LIBRE 0x0014F000UL

/* S28: nada de la caja al alcance del 68k. El 68k ve los 4 GB bajos del proceso tal cual (sus direcciones son de 32
   bits y el JIT las usa sin más); la caja va toda por encima. Comprueba dos cosas:
   - que el ejecutable (de __caja_imagen a __caja_fin), la pila del kernel, la pila de ahora, el estado del 68k, el
     código traducido, los montones, las pilas y la página 0 para escribir están por encima de 4 GB (en *bajas, cuántos
     no);
   - que en los 4 GB bajos no hay más memoria que la página 0 y la del 68k: se lo pregunta al kernel con la llamada 42
     (esperar), que da E_ARGUMENTO donde no hay memoria que leer. Una página de cada 64 KB (65.536 preguntas, medio
     segundo en QEMU; las 4 KB de todas, 8 s): lo que la caja proyecta, lo pone ella en sus direcciones fijas, y el
     ejecutable y su pila los pone el kernel por encima de 128 GB (lo de arriba). En *antes, si hay algo en $7000_0000,
     donde estuvo la caja hasta S28.
   Devuelve en cuántos trozos de 64 KB de los 4 GB bajos hay algo de más (0 es bien); en *trozos, cuántos miró, y en
   *ms, lo que tardó. */
extern char __caja_imagen[], __caja_fin[];
int caja_main(void);
static uint32_t caja_fuera_del_68k(uint32_t *bajas, int *antes, uint32_t *ms, uint32_t *trozos)
{
    uintptr_t sp = pila_de_ahora();
    uintptr_t z[MOTOR_ZONAS];
    motor_zonas(z);
    const uintptr_t suyas[] = {
        (uintptr_t)__caja_imagen, (uintptr_t)__caja_fin - 1, (uintptr_t)caja_main, z[0], pila_elf - 1, sp,
        z[1], z[2], MONTON, MONTON_RUST, PILAS, PAGINA0_RW,
    };
    *bajas = 0;
    for (unsigned i = 0; i < sizeof(suyas) / sizeof(suyas[0]); i++)
        *bajas += suyas[i] < 0x100000000UL;
    uint64_t t0 = ahora_us();
    uint32_t de_mas = 0;
    *trozos = 0;
    for (uintptr_t p = 0x10000; p < 0x100000000UL; p += 0x10000) {
        if (p >= M68K_RAM && p < m68k_ram_fin)
            continue;
        ++*trozos;
        de_mas += sc(SC_ESPERAR, (long)p, 0x5A5A5A5A, 1, 0, 0) != E_ARGUMENTO;
    }
    *antes = sc(SC_ESPERAR, 0x70000000L, 0x5A5A5A5A, 1, 0, 0) != E_ARGUMENTO;
    *ms = (uint32_t)((ahora_us() - t0) / 1000);
    return de_mas;
}

/* Los siete, uno tras otro. Cada uno intenta lo de su familia y devuelve cuántas cosas consiguió que no debía (0); la
   caja comprueba además que acaba bien, que no deja memoria y que lo que cambió en exec (tablas de saltos, ExecBase,
   vectores del 68k) se deshizo. Al final, que nada de lo suyo se usó después (sus parches, su código residente).
   Devuelve cuántos fallos. */
static int prueba_malware(uint32_t execbase, uint32_t sistema)
{
    static const struct { const char *nombre; const uint8_t *desde, *hasta; } virus[] = {
        { "arranque.exe", prog_mal_arranque, prog_mal_arranque_fin },
        { "enlace.exe", prog_mal_enlace, prog_mal_enlace_fin },
        { "parche.exe", prog_mal_parche, prog_mal_parche_fin },
        { "residente.exe", prog_mal_residente, prog_mal_residente_fin },
        { "guion.exe", prog_mal_guion, prog_mal_guion_fin },
        { "troyano.exe", prog_mal_troyano, prog_mal_troyano_fin },
        { "memoria.exe", prog_mal_memoria, prog_mal_memoria_fin },
    };
    static const char prefijo[] = "  [caja-amiga] malware: ";
    int fallos = 0;
    pon32(ZONA_LIBRE, 0);
    pon32(ZONA_LIBRE + 4, 0);
    for (unsigned i = 0; i < sizeof(virus) / sizeof(virus[0]); i++) {
        uint32_t d0 = 0, sobras = 0, entradas = 0, campos = 0;
        int llamadas = 0;
        caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
        int v = ejecuta_hunk(virus[i].nombre, virus[i].desde, (size_t)(virus[i].hasta - virus[i].desde), execbase, &d0,
                             &llamadas, &sobras);
        caja_consola_prefijo(NULL, 0);
        caja_defensa(&entradas, &campos);
        int bien = v == -1 && d0 == 0 && sobras == 0 && caja_bloques(0) == sistema;
        if (virus[i].desde == prog_mal_memoria) {
            uint32_t bajas = 0, ms = 0, trozos = 0;
            int antes = 1;
            uint32_t de_mas = caja_fuera_del_68k(&bajas, &antes, &ms, &trozos);
            int fuera = de_mas == 0 && bajas == 0 && !antes && trozos > 65000;
            di("malware: memoria: la caja, toda por encima de 4 GB (%u direcciones suyas por debajo); en los 4 GB del "
               "68k, el kernel no ve más que la página 0 y la memoria del 68k (%u de %u trozos de 64 KB con algo de "
               "más; en $70000000, %s; %u ms): %s\n", bajas, de_mas, trozos, antes ? "algo" : "nada", ms, fuera ? "bien" : "MAL");
            bien = bien && fuera;
        }
        /* S28: la guardia pregunta al usuario una vez como mucho por virus (tras un no, nada más de ese programa), y
           ningún virus recibe un sí. */
        uint32_t como = COMO_NADA, preguntas = caja_preguntas(&como);
        bien = bien && preguntas <= 1 && !como_deja(como);
        if (virus[i].desde == prog_mal_parche) bien = bien && entradas == 6;
        else if (virus[i].desde == prog_mal_residente) bien = bien && campos == 2 && vectores_deshechos == 2;
        else bien = bien && entradas == 0 && campos == 0 && vectores_deshechos == 0;
        di("malware %s: %d conseguidos (vector %d), %u negativas de la guardia (%u preguntas: %s), %u avisos del "
           "Enforcer; deshecho: %u entradas de tablas, %u grupos de ExecBase, %d vectores del 68k: %s\n", virus[i].nombre,
           (int)d0, v, caja_negados(), preguntas, como_texto(como), caja_avisos(), entradas, campos, vectores_deshechos,
           bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }
    }
    /* Después de los siete: los parches de parche.exe solo se usaron en su programa (2 veces), el código residente de
       residente.exe nunca, y ExecBase no tiene vectores de reinicio. */
    uint32_t usos = lee32(ZONA_LIBRE), reinicio = lee32(ZONA_LIBRE + 4);
    int limpio = lee32(execbase + 42) == 0 && lee32(execbase + 46) == 0 && lee32(execbase + 50) == 0
        && lee32(execbase + 546) == 0 && lee32(execbase + 550) == 0;
    int bien = usos == 2 && reinicio == 0 && limpio;
    di("malware: tras los siete, los parches de parche.exe se usaron %u veces (las 2 de su programa), el código residente "
       "%u, y ExecBase sin vectores de reinicio: %s\n", usos, reinicio, limpio ? "bien" : "MAL");
    if (!bien) fallos++;
    return fallos;
}

/* S28: un programa honrado que actualiza otro (actualiza.exe reescribe Caja:C/viejo, un ejecutable), tres vueltas. La
   guardia pregunta al usuario por `sesiones` y `guardian`; en la prueba, el guion contesta "solo esta vez" la primera
   y "siempre" la segunda, y la tercera ya lo recuerda `sesiones` (sin el usuario). En cada vuelta, una pregunta, y el
   programa actualiza si y solo si le dejaron. Sin `sesiones` ni `guardian` (o sin Caja:), no le dejan: también bien. */
static int prueba_actualiza(uint32_t execbase, uint32_t sistema)
{
    static const char prefijo[] = "  [caja-amiga] ";
    int fallos = 0;
    for (int vuelta = 1; vuelta <= 3; vuelta++) {
        uint32_t d0 = 0, sobras = 0, como = COMO_NADA;
        int llamadas = 0;
        caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
        int v = ejecuta_hunk("actualiza.exe", prog_actualiza, (size_t)(prog_actualiza_fin - prog_actualiza), execbase,
                             &d0, &llamadas, &sobras);
        caja_consola_prefijo(NULL, 0);
        uint32_t preguntas = caja_preguntas(&como);
        int bien = v == -1 && sobras == 0 && caja_bloques(0) == sistema && preguntas <= 1
            && (d0 == 0) == (preguntas == 1 && como_deja(como));
        di("actualiza.exe, vuelta %d: %u preguntas (%s): %s: %s\n", vuelta, preguntas, como_texto(como),
           d0 == 0 ? "actualizado" : "no le dejan", bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }
    }
    return fallos;
}

/* Ctrl-C desde fuera (como el de una consola): otro hilo, a los `us`, lo pide y levanta la interrupción de nivel 6
   del 68k; la caja lo entrega como la señal SIGBREAKF_CTRL_C de la tarea. Si el programa no acaba en 10 s más, la
   NMI lo corta (el vigilante de siempre). Con `us` = 0, solo eso: un vigilante de 10 s, sin Ctrl-C. */
static volatile int ctrl_c_corriendo;
/* En vez de Ctrl-C, cerrar sus ventanas como con el botón de cerrar de la barra (un programa que solo espera a sus
   ventanas en Wait, como el ejemplo de ReAction de Dash, no ve Ctrl-C hasta que algo lo despierta). */
static volatile int cerrar_en_vez, cerrar_pendiente;
int caja_tomar_cerrar(void) { return __atomic_exchange_n(&cerrar_pendiente, 0, __ATOMIC_ACQ_REL); }
static void manda_ctrl_c(uint64_t us)
{
    if (us) {
        sc(SC_DORMIR, (long)us, 0, 0, 0, 0);
        /* Solo si su programa sigue: si no, el Ctrl-C quedaba pendiente y llegaba al siguiente (al de después de
           GadTools, test_reaction, que acababa con "***Break"). */
        if (!ctrl_c_corriendo) ;
        else if (cerrar_en_vez) __atomic_store_n(&cerrar_pendiente, 1, __ATOMIC_RELEASE);
        else {
            __atomic_store_n(&ctrl_c_pendiente, 1, __ATOMIC_RELEASE);
            motor_interrumpir();
        }
    }
    for (int i = 0; i < 100 && ctrl_c_corriendo; i++) sc(SC_DORMIR, 100000, 0, 0, 0, 0);
    if (ctrl_c_corriendo) motor_cortar();
    sc(SC_SALIR, 0, 0, 0, 0, 0);
    for (;;) {}
}

/* --- Principal --- */

int caja_main(void)
{
#if defined(__x86_64__)
    caja_rust_frecuencia(caja_frecuencia_tsc);
#endif
    di("%s (H4; X7: la interfaz de motor)\n", motor_nombre());
    if (proyecta(M68K_RAM, M68K_RAM_LEN, P_ESCRIBIR) <= 0 || proyecta(MONTON, MONTON_LEN, P_ESCRIBIR) <= 0
        || proyecta(MONTON_RUST, MONTON_RUST_LEN, P_ESCRIBIR) <= 0) {
        di("sin memoria para el 68k\n");
        return 1;
    }
    tlsf = tlsf_init_with_memory((void *)MONTON, MONTON_LEN);
    tlsf_rust = tlsf_init_with_memory((void *)MONTON_RUST, MONTON_RUST_LEN);
    int e = motor_iniciar();
    if (e) return e;
    prepara_vectores();
    int fallos = 0;

    /* La página 0 (S3): un objeto de una página, escrito por una dirección por encima de 4 GB y proyectado de solo
       lectura en la dirección 0, con el permiso de página 0 (la llave 1, de `inicio`). Exec escribe ExecBase en la
       dirección 4 por la proyección de escribir; después se proyecta en 0. */
    long p0 = sc(SC_MEMORIA_CREAR, 4096, 0, 0, 0, 0);
    long r0 = p0 > 0 ? sc(SC_MEMORIA_PROYECTAR, p0, (long)PAGINA0_RW, P_ESCRIBIR, 0, 0) : -1;
    if (r0 != 0) {
        di("sin memoria para la página 0\n");
        return 4;
    }
    uint32_t execbase = caja_rust_arrancar(M68K_RAM, M68K_RAM_LEN, PAGINA0_RW, SISTEMA, FIN_SISTEMA, CHIP);
    if (!execbase) {
        di("las bibliotecas nativas no arrancan\n");
        return 5;
    }
    uint32_t super = caja_alloc_mem(SUPER_LEN, 0);
    if (!super) {
        di("sin memoria para la pila de supervisor\n");
        return 6;
    }
    super_tope = super + SUPER_LEN;
    pon32(SUPER_BAJO, super + 512);
    caja_poner_trampa((uint32_t)TRAMPA_DEFECTO);
    r0 = sc(SC_PAGINA_CERO, LLAVE_PAGINA0, p0, 0, 0, 0);
    /* La caja la lee (EL0 puede leerla); otra vez, o en el resto de los primeros 64 KB, el kernel no deja. */
    int pagina0 = r0 == 0 && lee32(4) == execbase && sc(SC_PAGINA_CERO, LLAVE_PAGINA0, p0, 0, 0, 0) < 0
        && sc(SC_MEMORIA_PROYECTAR, p0, 0x1000, P_LEER, 0, 0) < 0 && sc(SC_MEMORIA_PROYECTAR, p0, 0, P_LEER, 0, 0) < 0;
    di("página 0: ExecBase en la dirección 4 ($%x), solo de lectura; otra vez o en los primeros 64 KB, rechazada: %s\n",
       pagina0 ? lee32(4) : 0, pagina0 ? "bien" : "MAL");
    if (!pagina0) { di("página 0: la llamada 49 dio %d\n", (int)r0); fallos++; }

    /* 1. La suma de 1 a 100.000 y su texto, por la dos.library nativa. */
    uint64_t t0 = ahora_us();
    llamadas_nativas = 0;
    int v = ejecuta("hola", prog_hola, prog_hola_fin, 0x00110000);
    uint64_t t1 = ahora_us();
    uint32_t esperado = 0;
    for (uint32_t i = 1; i <= 100000; i++) esperado += i;
    int bien = v < 0 && d0_final == esperado && llamadas_nativas == 4;
    di("el 68k dio %u (esperado %u) con %d llamadas nativas, %u unidades traducidas, en %u ms: %s\n",
       d0_final, esperado, llamadas_nativas, motor_unidades(), (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
    if (!bien) { motor_volcar(); fallos++; }

    /* 2. Hostil: una función nativa que no existe (se rechaza y el programa sigue) y una línea A que no es una
       llamada (excepción 10 del 68k: el programa acaba, la caja sigue). */
    llamadas_nativas = 0;
    v = ejecuta("lineaa", prog_lineaa, prog_lineaa_fin, 0x00120000);
    bien = v == 10 && rechazadas == 1 && llamadas_nativas == 1;
    di("línea A desconocida: excepción %d del 68k, %d rechazada, %d llamada buena: %s\n", v, rechazadas,
       llamadas_nativas, bien ? "bien" : "MAL");
    if (!bien) { motor_volcar(); fallos++; }

    /* 3. Hostil: un bucle sin fin. El vigilante, en otro hilo, lo corta a los 300 ms. */
    static uint8_t *pila_vigia;
    pila_vigia = (uint8_t *)(PILAS + PILA_LEN + 0x10000);
    if (proyecta((uintptr_t)pila_vigia, PILA_VIGIA_LEN, P_ESCRIBIR) <= 0) {
        di("sin memoria para el vigilante\n");
        return 4;
    }
    /* El reloj de los turnos (varias tareas), con su pila: para siempre. */
    static uint8_t *pila_reloj;
    pila_reloj = (uint8_t *)(PILAS + PILA_LEN + 0x30000);
    if (proyecta((uintptr_t)pila_reloj, PILA_VIGIA_LEN, P_ESCRIBIR) <= 0
        || sc(SC_HILO_CREAR, (long)reloj_turnos, (long)(pila_reloj + PILA_VIGIA_LEN), 20000, 0, 0) < 0)
        di("sin el reloj de los turnos: una tarea que no llama a nada no soltará el testigo\n");
    long hilo = sc(SC_HILO_CREAR, (long)vigila, (long)(pila_vigia + PILA_VIGIA_LEN), 300000, 0, 0);
    if (hilo < 0) {
        di("no se pudo crear el vigilante: %d\n", (int)hilo);
        return 5;
    }
    t0 = ahora_us();
    v = ejecuta("bucle", prog_bucle, prog_bucle_fin, 0x00130000);
    t1 = ahora_us();
    sc(SC_HILO_UNIR, hilo, 0, 0, 0, 0);
    bien = v == 31 && d0_final > 1000;
    di("bucle sin fin: cortado por el vigilante a los %u ms tras %u vueltas (vector %d): %s\n",
       (uint32_t)((t1 - t0) / 1000), d0_final, v, bien ? "bien" : "MAL");
    if (!bien) { motor_volcar(); fallos++; }

    if (pagina0) {
        /* 4. Un ejecutable hunk de AmigaOS (m68k-amigaos-gcc): cargado con sus reubicaciones en memoria de AllocMem,
           toma ExecBase de la dirección 4, abre la dos.library con OpenLibrary y escribe. */
        uint32_t d0 = 0, sobras = 0;
        int llamadas = 0;
        size_t largo = (size_t)(prog_saludo_fin - prog_saludo);
        uint32_t sistema = caja_bloques(0);
        t0 = ahora_us();
        v = ejecuta_hunk("saludo.exe", prog_saludo, largo, execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && llamadas == 7 && lee16(caja_dosbase() + 32) == 0 && sobras == 0
            && caja_bloques(0) == sistema && caja_avisos() == 0;
        di("saludo.exe acabó con %d (vector %d), %d llamadas nativas, en %u ms, sin memoria perdida: %s\n", (int)d0, v,
           llamadas, (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 5. Hostil: el mismo ejecutable cortado por cada byte y estropeado al azar. Ningún caso rompe la caja ni
           escribe fuera de lo que reserva; los que fallan devuelven toda su memoria (los cortes y 20.000 mutaciones con el
           reparto rápido; 1.000 más con AllocMem de exec, que no pierde ningún bloque). */
        int errores = 0, cargan = 0, perdidas = 0;
        struct hunk_carga c;
        int e;
        uint32_t n = largo < sizeof(hunk_copia) ? (uint32_t)largo : 0;
        const uint32_t rapido_largo = 256 << 10;
        rapido_inicio = caja_alloc_mem(rapido_largo, 0);
        rapido_fin = rapido_inicio + rapido_largo;
        t0 = ahora_us();
        for (uint32_t corte = 0; corte < n && rapido_inicio; corte++) {
            hunks_vivos = 0;
            rapido_libre = rapido_inicio;
            e = hunk_cargar(prog_saludo, corte, &rapido_ops, hunks_segs, MAX_HUNKS, &c);
            if (e == HUNK_BIEN) { cargan++; hunk_soltar(&rapido_ops, &c); } else errores++;
            perdidas += hunks_vivos != 0;
        }
        const int mutaciones = 20000, con_exec = 1000;
        for (int k = 0; k < mutaciones + con_exec && n && rapido_inicio; k++) {
            for (uint32_t i = 0; i < n; i++) hunk_copia[i] = prog_saludo[i];
            for (int j = 1 + azar() % 4; j > 0; j--) {
                uint32_t p = (azar() % n) & ~3u;
                static const uint32_t raros[] = { 0, 1, 0x7FFFFFFF, 0xFFFFFFFF, 0x3FFFFFFF, 0x40000000, HUNK_END, HUNK_CODE,
                                                  HUNK_RELOC32 };
                uint32_t x = azar() % 2 ? azar() : raros[azar() % (sizeof(raros) / 4)];
                if (p + 4 <= n) { hunk_copia[p] = x >> 24; hunk_copia[p + 1] = x >> 16; hunk_copia[p + 2] = x >> 8; hunk_copia[p + 3] = x; }
            }
            const struct hunk_ops *ops = k < mutaciones ? &rapido_ops : &hunks_ops;
            hunks_vivos = 0;
            rapido_libre = rapido_inicio;
            e = hunk_cargar(hunk_copia, n, ops, hunks_segs, MAX_HUNKS, &c);
            if (e == HUNK_BIEN) { cargan++; hunk_soltar(ops, &c); } else errores++;
            perdidas += hunks_vivos != 0;
        }
        t1 = ahora_us();
        if (rapido_inicio) caja_free_mem(rapido_inicio, rapido_largo);
        bien = n && rapido_inicio && perdidas == 0 && errores > (int)n && caja_bloques(0) == sistema;
        di("hunk hostil: %u cortes y %d mutaciones de saludo.exe (%d con AllocMem): %d con error, %d cargan, %d pierden "
           "memoria, en %u ms: %s\n", n, mutaciones + con_exec, con_exec, errores, cargan, perdidas,
           (uint32_t)((t1 - t0) / 1000), bien ? "bien" : "MAL");
        if (!bien) fallos++;

        /* 6. Una ventana Amiga en el escritorio: ventana.exe la abre con OpenWindowTags, lee un fichero por dos, dibuja
           y espera a IDCMP_CLOSEWINDOW (el guion de la prueba pulsa su gadget de cerrar) o 40 s. */
        t0 = ahora_us();
        v = ejecuta_hunk("ventana.exe", prog_ventana, (size_t)(prog_ventana_fin - prog_ventana), execbase, &d0,
                         &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && (d0 == 0 || d0 == 5) && sobras == 0 && caja_bloques(0) == sistema;
        di("ventana.exe acabó con %d (%s), %d llamadas nativas, en %u ms, %u avisos, sin memoria perdida: %s\n", (int)d0,
           d0 == 0 ? "cerrada con su gadget" : d0 == 5 ? "cerrada sola por tiempo" : "?", llamadas,
           (uint32_t)((t1 - t0) / 1000), caja_avisos(), bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 7. Hostil: punteros malos a OpenWindow, OpenWindowTagList, las de dibujo y las de dos; dibujar fuera de la
           ventana. La caja sigue, y el compositor también. */
        v = ejecuta_hunk("hostil.exe", prog_hostil, (size_t)(prog_hostil_fin - prog_hostil), execbase, &d0, &llamadas,
                         &sobras);
        int abiertas = caja_compositor_sigue();
        bien = v == -1 && d0 == 0 && caja_avisos() >= 10 && abiertas == 0 && caja_bloques(0) == sistema;
        di("hostil.exe acabó con %d tras %d llamadas nativas: %u avisos del Enforcer, %u bloques sin soltar (los soltó la "
           "caja), el compositor contesta con %d ventanas de la caja abiertas: %s\n", (int)d0, llamadas, caja_avisos(),
           sobras, abiertas, bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* S28: los virus de laboratorio. Después, los programas de Dash sirven además de prueba de que la caja sigue
           bien (y de que los parches de los virus ya no están: pasan por OpenLibrary y Open). */
        fallos += prueba_malware(execbase, sistema);
        fallos += prueba_actualiza(execbase, sistema);

        /* 8. Programas de Dash, el lenguaje de FreeMEMLang (compilados con `dash` al hacer la caja; ver el Makefile):
           lo que escriben sale con "  [caja-amiga] dash: " delante. Tienen que acabar con 0, sin avisos del Enforcer y
           sin dejar memoria. */
        static const struct { const char *nombre; const uint8_t *desde, *hasta; } dash[] = {
            { "texto.exe", prog_dash_texto, prog_dash_texto_fin },
            { "texto_gcc.exe", prog_dash_texto_gcc, prog_dash_texto_gcc_fin },
            { "test_interp_comillas", prog_dash_interp, prog_dash_interp_fin },
            { "test_lista_listas_literal", prog_dash_listas, prog_dash_listas_fin },
            { "hola.exe", prog_dash_hola, prog_dash_hola_fin },
            { "ventana.exe", prog_dash_ventana, prog_dash_ventana_fin },
        };
        static const char prefijo[] = "  [caja-amiga] dash: ";
        for (unsigned i = 0; i < sizeof(dash) / sizeof(dash[0]); i++) {
            size_t n = (size_t)(dash[i].hasta - dash[i].desde);
            if (n == 0) {
                di("Dash %s: no está (la caja se compiló sin ~/Projects/FreeMEMLang/dash, o no compiló)\n", dash[i].nombre);
                continue;
            }
            caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
            t0 = ahora_us();
            v = ejecuta_hunk(dash[i].nombre, dash[i].desde, n, execbase, &d0, &llamadas, &sobras);
            t1 = ahora_us();
            caja_consola_prefijo(NULL, 0);
            bien = v == -1 && d0 == 0 && caja_avisos() == 0 && sobras == 0 && caja_bloques(0) == sistema;
            di("Dash %s acabó con %d (vector %d), %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
               dash[i].nombre, (int)d0, v, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras,
               bien ? "bien" : "MAL");
            if (!bien) { motor_volcar(); fallos++; }
        }
        /* 8b. BOOPSI y ReAction (boopsi.c), antes que GadTools (que en el modo de las capturas espera 90 s y la sesión se
           bloquea):  una clase de usuario con su despachador en 68k (que vuelve a intuition
           con DoSuperMethodA), interconexión con propgclass, strgclass, icclass y modelclass, y una ventana de
           window.class con su layout. Se comprueba solo y acaba con 0. */
        static const char *const boopsi_nombres[] = { BOOPSI_NOMBRE, "boopsi-orig-20" };
        for (unsigned i = 0; i < (CAJA_INTERACTIVA ? 2u : 1u); i++) {
            t0 = ahora_us();
            v = ejecuta_hunk(boopsi_nombres[i], prog_boopsi, (size_t)(prog_boopsi_fin - prog_boopsi), execbase, &d0,
                             &llamadas, &sobras);
            t1 = ahora_us();
            uint32_t hechas = 0, no = 0, exc = 0;
            caja_cuentas_anidadas(&hechas, &no, &exc);
            bien = v == -1 && d0 == 0 && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema && no == 0;
            di("boopsi.exe acabó con %d, %d llamadas nativas, %u despachadores 68k, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
               (int)d0, llamadas, hechas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
            if (!bien) { motor_volcar(); fallos++; }
        }

        /* 8b2. ramlib (ramlib.c): abre bibliotecas de disco (Caja:Libs, su LIBS:), las usa y las cierra, y lo malo (un
           RomTag falso, una que miente con su versión, un fichero que no es ejecutable, caminos raros, lowlevel.library) y layers.library (la nativa).
           Al acabar no queda nada suyo en memoria: los bloques del sistema, como antes. */
        t0 = ahora_us();
        v = ejecuta_hunk("ramlib", prog_ramlib, (size_t)(prog_ramlib_fin - prog_ramlib), execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema;
        di("ramlib.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar, %u bloques del sistema de más: %s\n",
           (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, caja_bloques(0) - sistema, bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 8b3. El AmigaOS del usuario como capa (capa.c): si la caja lo montó (SYS:C/Prefs/amigaos), abre sus
           bibliotecas de software; las del hardware fallan limpias. Sin él, lo dice y acaba. Lo que se queda en memoria
           (una biblioteca que no se deja echar) y los avisos (funciones que aún faltan) se cuentan, sin fallar. */
        t0 = ahora_us();
        v = ejecuta_hunk("capa", prog_capa, (size_t)(prog_capa_fin - prog_capa), execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && sobras == 0;
        di("capa.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar, %d bloques del sistema de más: %s\n",
           (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, (int)(caja_bloques(0) - sistema), bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }
        sistema = caja_bloques(0);

        /* 8b3a. Los tipos de datatypes: con el AmigaOS del usuario (su AmigaOS:C/AddDataTypes; su C: no se asigna), `AddDataTypes REFRESH QUIET`,
           como su Startup-Sequence. Recorre DEVS:DataTypes con MatchFirst y deja los tipos a datatypes: lo que no
           suelta pasa al sistema (si se soltara, datatypes se quedaría con memoria libre). A los 20 s, el vigilante lo
           corta. Sin él, lo dice y sigue. No cuenta como fallo: DashOS no trae AddDataTypes. */
        {
            const uint8_t *adt = 0;
            uint32_t adt_largo = 0;
            if (caja_fichero_de_dos("AmigaOS:C/AddDataTypes", &adt, &adt_largo)) {
                ctrl_c_corriendo = 1;
                long h = sc(SC_HILO_CREAR, (long)manda_ctrl_c, (long)(pila_vigia + PILA_VIGIA_LEN), 20000L * 1000, 0, 0);
                tarea_args = "REFRESH QUIET\n";
                tarea_al_sistema = 1;
                t0 = ahora_us();
                v = ejecuta_hunk("AddDataTypes", adt, adt_largo, execbase, &d0, &llamadas, &sobras);
                t1 = ahora_us();
                tarea_args = 0;
                tarea_al_sistema = 0;
                ctrl_c_corriendo = 0;
                if (h >= 0) sc(SC_HILO_UNIR, h, 0, 0, 0, 0);
                di("AddDataTypes REFRESH acabó con %d (vector %d), %d llamadas nativas, en %u ms, %u avisos, %u bloques pasan al sistema\n",
                   (int)d0, v, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), tarea_pasadas);
                sistema = caja_bloques(0);
                /* Y que datatypes tenga los tipos: una ILBM del AmigaOS del usuario por NewDTObject (dtprueba.c). No
                   cuenta como fallo: depende del AmigaOS del usuario. */
                v = ejecuta_hunk("dtprueba", prog_dtprueba, (size_t)(prog_dtprueba_fin - prog_dtprueba), execbase, &d0,
                                 &llamadas, &sobras);
                di("dtprueba.exe acabó con %d (vector %d), %d llamadas nativas, %u avisos, %u bloques sin soltar\n",
                   (int)d0, v, llamadas, caja_avisos(), sobras);
                sistema = caja_bloques(0);
            } else {
                di("AddDataTypes: no está (sin el AmigaOS del usuario): datatypes sin tipos\n");
            }
        }

        /* 8b3b. La memoria del 68k crece según la pidan (glotona.c, hostil): con un tope de prueba de 48 MB (el del
           ajuste es de 256), pide 1 MB tras otro hasta que no le dan más; no pasa del tope, nada se cuelga y, soltado
           todo, la memoria crecida se vuelve a repartir. Lo crecido se queda (no se devuelve al sistema). */
        caja_tope_de_prueba(48);
        t0 = ahora_us();
        v = ejecuta_hunk("glotona", prog_glotona, (size_t)(prog_glotona_fin - prog_glotona), execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        caja_tope_de_prueba(0);
        bien = v == -1 && d0 == 0 && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema && m68k_ram_fin <= M68K_RAM + (48UL << 20);
        di("glotona.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar, la memoria del 68k acaba en $%x: %s\n",
           (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, m68k_ram_fin, bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 8b3c. aviso-cajas: memoria baja. Con la memoria del 68k crecida y libre (lo que dejó glotona), memh.c pone un
           manejador de memoria baja (AddMemHandler) que suelta 2 MB de "caché" y espera. La caja dice a `presion` (canal
           de la prueba) que tiene algo que soltar; `presion` llena la memoria de DashOS, el kernel avisa a `sesiones` y
           `sesiones` a la caja: en la siguiente llamada a exec de memh se llama a su manejador y se devuelve a DashOS lo
           crecido que está libre. Sin el canal (otras imágenes), no se prueba. */
        if (caja_presion_listo()) {
            t0 = ahora_us();
            v = ejecuta_hunk("memh", prog_memh, (size_t)(prog_memh_fin - prog_memh), execbase, &d0, &llamadas, &sobras);
            t1 = ahora_us();
            caja_atender_memoria();
            bien = v == -1 && d0 == 0 && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema;
            di("memh.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar, la memoria del 68k acaba en $%x: %s\n",
               (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, m68k_ram_fin, bien ? "bien" : "MAL");
            if (!bien) { motor_volcar(); fallos++; }
        } else {
            di("memh.exe: sin la prueba del aviso de memoria baja (no hay a quién pedirlo)\n");
        }

        /* 8b3d. InternalLoadSeg con un flujo propio (ils.c), como AddDataTypes con el código de un descriptor: lo lee
           la función de leer 68k del programa, y lo hostil (-1, de más, basura, sin función, vacío) da 0. */
        t0 = ahora_us();
        v = ejecuta_hunk("ils", prog_ils, (size_t)(prog_ils_fin - prog_ils), execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && sobras == 0 && caja_bloques(0) == sistema;
        di("ils.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
           (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 8b3e. cybergraphics.library y Picasso96API.library nativas (cgx.c): píxeles, LockBitMap, RenderInfo y lo
           hostil (los avisos que da no cuentan como fallo); un mapa cogido que se suelta sin soltar su cerrojo. */
        t0 = ahora_us();
        v = ejecuta_hunk("cgx", prog_cgx, (size_t)(prog_cgx_fin - prog_cgx), execbase, &d0, &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && d0 == 0 && sobras == 0 && caja_bloques(0) == sistema;
        di("cgx.exe acabó con %d, %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
           (int)d0, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 8b4. Programas de AmigaOS del usuario (SYS:C/Prefs/programas-amiga, con DASHOS_PROGRAMAS_AMIGA): las demos de
           MUI, Tachyon... Para ver hasta dónde llegan y qué les falta (el registro de lo que falta, faltan.rs): a los 6 s
           (en el modo de las capturas, 30 s) se cierran sus ventanas como con el botón de la barra; si no acaban en 10
           s más, el vigilante los corta. No cuentan como fallo: DashOS no trae ninguno. */
        for (uint32_t i = 0; i < 64; i++) {
            char pnom[128];
            const uint8_t *pdat = 0;
            uint32_t plar = 0;
            if (!caja_programa_de_disco(i, pnom, sizeof(pnom), &pdat, &plar)) break;
            ctrl_c_corriendo = 1;
            cerrar_en_vez = 1;
            long h = sc(SC_HILO_CREAR, (long)manda_ctrl_c, (long)(pila_vigia + PILA_VIGIA_LEN),
                        (long)(CAJA_INTERACTIVA ? 30000 : 6000) * 1000, 0, 0);
            t0 = ahora_us();
            v = ejecuta_hunk(pnom, pdat, plar, execbase, &d0, &llamadas, &sobras);
            t1 = ahora_us();
            ctrl_c_corriendo = 0;
            cerrar_en_vez = 0;
            if (h >= 0) sc(SC_HILO_UNIR, h, 0, 0, 0, 0);
            di("programa del usuario %s acabó con %d (vector %d), %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar\n",
               pnom, (int)d0, v, llamadas, (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras);
            sistema = caja_bloques(0);
        }

        /* 8c. GadTools (gadtools.c): una ventana con un gadget de cada tipo y menús. Comprueba sola sus atributos y
           espera clics: el guion pulsa varios gadgets y elige Proyecto > Salir con el botón derecho (0), si su ventana
           se ve entera; si no, se cierra sola (5). En la prueba de siempre espera 5 s: más tiempo abierta tapaba los
           pasos del guion de los demás (Ajustes, el centro de control). Con CAJA_INTERACTIVA (las capturas), 90 s. */
        t0 = ahora_us();
        v = ejecuta_hunk(GT_NOMBRE, prog_gadtools, (size_t)(prog_gadtools_fin - prog_gadtools), execbase, &d0,
                         &llamadas, &sobras);
        t1 = ahora_us();
        bien = v == -1 && (d0 == 0 || d0 == 5) && sobras == 0 && caja_avisos() == 0 && caja_bloques(0) == sistema;
        di("gadtools.exe acabó con %d (%s), %d llamadas nativas, en %u ms, %u avisos, %u bloques sin soltar: %s\n",
           (int)d0, d0 == 0 ? "cerrada por el programa" : d0 == 5 ? "cerrada sola por tiempo" : "?", llamadas,
           (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
        if (!bien) { motor_volcar(); fallos++; }

        /* 9. Llamadas anidadas: 68k desde una función nativa (anidado.c; el modo, en la letra del nombre). a: Open de una
           biblioteca 68k, sin fin (el tope), con la pila de vuelta más abajo, tc_ExceptCode, CallHookPkt y un
           tc_TrapCode que recupera: acaba con 0. b: vuelve con la pila más arriba: cortado (-2). c: instrucción
           ilegal dentro: acaba por el vector 4 dentro de la anidada. d: excepción dentro de la excepción, sin fin:
           vector 4 al acabarse la pila de supervisor. */
        static const struct { const char *nombre; int vector; } anid[] = {
            { "anidado-a", -1 }, { "anidado-b", -2 }, { "anidado-c", 4 }, { "anidado-d", 4 },
        };
        for (unsigned i = 0; i < sizeof(anid) / sizeof(anid[0]); i++) {
            v = ejecuta_hunk(anid[i].nombre, prog_anidado, (size_t)(prog_anidado_fin - prog_anidado), execbase, &d0,
                             &llamadas, &sobras);
            uint32_t hechas = 0, no = 0, exc = 0;
            caja_cuentas_anidadas(&hechas, &no, &exc);
            bien = v == anid[i].vector && (v != -1 || d0 == 0) && caja_bloques(0) == sistema && anidado == 0;
            di("%s acabó por el vector %d (D0 %d), %u llamadas anidadas (%u no), %u excepciones de tarea, %u avisos: %s\n",
               anid[i].nombre, v, (int)d0, hechas, no, exc, caja_avisos(), bien ? "bien" : "MAL");
            if (!bien) { motor_volcar(); fallos++; }
        }

        /* 10. Ctrl-C y las excepciones del procesador en programas de Dash: el bucle sin fin de caja_ctrl_c.dash (por los
           dos caminos) recibe Ctrl-C de otro hilo a los 700 ms y acaba con "***Break" y 5; test_ctrl_c_for se lo
           manda él (SetSignal) y acaba igual; test_trap_illegal y test_trap_divzero acaban con el mensaje del
           tc_TrapCode de Dash y 20. Y el ejemplo de GadTools de FreeMEMLang (examples/test_gadtools.dash): el guion
           pulsa su "Cancel" (0) si llega; si no, Ctrl-C a los 6 s (5): su ventana no puede tapar mucho rato las de
           los demás pasos de la prueba. */
        static const struct { const char *nombre; const uint8_t *desde, *hasta; int ctrl_c; uint32_t d0, otro; } dash2[] = {
            { "ctrl_c.exe", prog_dash_ctrl_c, prog_dash_ctrl_c_fin, 700, 5, 5 },
            { "ctrl_c_gcc.exe", prog_dash_ctrl_c_gcc, prog_dash_ctrl_c_gcc_fin, 700, 5, 5 },
            { "test_ctrl_c_for", prog_dash_ctrl_c_for, prog_dash_ctrl_c_for_fin, 0, 5, 5 },
            { "test_trap_illegal", prog_dash_trap_ilegal, prog_dash_trap_ilegal_fin, 0, 20, 20 },
            { "test_trap_divzero", prog_dash_trap_divcero, prog_dash_trap_divcero_fin, 0, 20, 20 },
            { "test_gadtools", prog_dash_test_gadtools, prog_dash_test_gadtools_fin, 6000, 0, 5 },
            /* ReAction (window.class y layout.gadget nativos): la prueba de la documentación acaba sola; el ejemplo
               espera a que pulsen "OK" o "Cancel" o cierren su ventana: a los 4 s (en el modo de las capturas, 30 s)
               se cierra como con el botón de la barra (un negativo: cerrar en vez de Ctrl-C) y acaba con 0. */
            { "test_doc_reaction", prog_dash_test_doc_reaction, prog_dash_test_doc_reaction_fin, 0, 0, 0 },
            { "test_reaction", prog_dash_test_reaction, prog_dash_test_reaction_fin, CAJA_INTERACTIVA ? -30000 : -4000, 0, 0 },
        };
        for (unsigned i = 0; i < sizeof(dash2) / sizeof(dash2[0]); i++) {
            size_t n = (size_t)(dash2[i].hasta - dash2[i].desde);
            if (n == 0) {
                di("Dash %s: no está (la caja se compiló sin ~/Projects/FreeMEMLang/dash, o no compiló)\n", dash2[i].nombre);
                continue;
            }
            ctrl_c_corriendo = 1;
            cerrar_en_vez = dash2[i].ctrl_c < 0;
            long h = sc(SC_HILO_CREAR, (long)manda_ctrl_c, (long)(pila_vigia + PILA_VIGIA_LEN),
                        (long)(dash2[i].ctrl_c < 0 ? -dash2[i].ctrl_c : dash2[i].ctrl_c) * 1000, 0, 0);
            caja_consola_prefijo(prefijo, sizeof(prefijo) - 1);
            t0 = ahora_us();
            v = ejecuta_hunk(dash2[i].nombre, dash2[i].desde, n, execbase, &d0, &llamadas, &sobras);
            t1 = ahora_us();
            caja_consola_prefijo(NULL, 0);
            ctrl_c_corriendo = 0;
            if (h >= 0) sc(SC_HILO_UNIR, h, 0, 0, 0, 0);
            uint32_t hechas = 0, no = 0, exc = 0;
            caja_cuentas_anidadas(&hechas, &no, &exc);
            bien = v == -1 && (d0 == dash2[i].d0 || d0 == dash2[i].otro) && caja_avisos() == 0 && sobras == 0 && caja_bloques(0) == sistema;
            di("Dash %s acabó con %d (vector %d) en %u ms, %u excepciones de tarea, %u avisos, %u bloques sin soltar: %s\n",
               dash2[i].nombre, (int)d0, v, (uint32_t)((t1 - t0) / 1000), exc, caja_avisos(), sobras, bien ? "bien" : "MAL");
            if (!bien) { motor_volcar(); fallos++; }
        }
        /* 11. Varias tareas 68k a la vez (procesos.c; el modo, en la letra del nombre). a: Wait y Signal, puertos,
           prioridades, Forbid, Delay y el turno entre tareas: acaba con 0 sin avisos. b: hostil (miles de tareas,
           tareas que no acaban, un puerto borrado con mensajes, RemTask de otra...): acaba con 0. c: una tarea quita
           la del programa: el programa acaba cortado (vector 31). d: tareas que caen por una excepción: se quitan y
           el programa sigue. En todos, nada del programa queda en memoria. Van al final: con ellos antes, la ventana de test_gadtools seguía
           abierta cuando el guion de la prueba pulsaba en el centro de control. */
        static const struct { const char *nombre; int vector; } proc[] = {
            { "procesos-a", -1 }, { "procesos-b", -1 }, { "procesos-c", 31 }, { "procesos-d", -1 }, { "procesos-e", -1 },
        };
        for (unsigned i = 0; i < sizeof(proc) / sizeof(proc[0]); i++) {
            ctrl_c_corriendo = 1;
            cerrar_en_vez = 0;
            long hv = sc(SC_HILO_CREAR, (long)manda_ctrl_c, (long)(pila_vigia + PILA_VIGIA_LEN), 0, 0, 0);
            t0 = ahora_us();
            v = ejecuta_hunk(proc[i].nombre, prog_procesos, (size_t)(prog_procesos_fin - prog_procesos), execbase, &d0,
                             &llamadas, &sobras);
            t1 = ahora_us();
            ctrl_c_corriendo = 0;
            if (hv >= 0) sc(SC_HILO_UNIR, hv, 0, 0, 0, 0);
            uint32_t cambios = 0, acabadas = 0, aparcadas = 0;
            caja_cuentas_tareas(&cambios, &acabadas, &aparcadas);
            bien = v == proc[i].vector && (v != -1 || d0 == 0) && caja_bloques(0) == sistema && anidado == 0
                && (i != 0 || caja_avisos() == 0) && (i == 2 || sobras == 0);
            di("%s acabó por el vector %d (D0 %d), %u cambios de tarea, %u tareas acabadas, %u llamadas nativas, en %u ms, "
               "%u avisos, %u bloques sin soltar: %s\n", proc[i].nombre, v, (int)d0, cambios, acabadas, llamadas,
               (uint32_t)((t1 - t0) / 1000), caja_avisos(), sobras, bien ? "bien" : "MAL");
            if (!bien) { motor_volcar(); fallos++; }
            if (proc[i].nombre[9] == 'e' && cambios)
                di("procesos: medida: %u cambios de tarea en %u ms, %u ns por cambio (con su Wait y su Signal)\n", cambios,
                   (uint32_t)((t1 - t0) / 1000), (uint32_t)((t1 - t0) * 1000 / cambios));
            if (aparcadas >> 16) di("procesos: %u turnos quitados por el reloj\n", aparcadas >> 16);
        }

#if CAJA_INTERACTIVA
        /* El error de Dash en un requester: test_trap_illegal sin consola (Output() es 0, como desde el Workbench). El
           guion lo captura (CAPTURA_REQ) y pulsa su botón; si no llega, el requester sigue abierto hasta el final de la
           prueba (la espera, de 600 s en este modo) y sale en la captura final (CAPTURA). */
        if (prog_dash_trap_ilegal_fin > prog_dash_trap_ilegal) {
            di("test_trap_illegal (sin consola): su error, en un requester\n");
            caja_sin_consola(1);
            v = ejecuta_hunk("test_trap_illegal", prog_dash_trap_ilegal,
                             (size_t)(prog_dash_trap_ilegal_fin - prog_dash_trap_ilegal), execbase, &d0, &llamadas, &sobras);
            caja_sin_consola(0);
            di("Dash test_trap_illegal (sin consola) acabó con %d (vector %d), %u avisos\n", (int)d0, v, caja_avisos());
        }
#endif
    }

    if (fallos) return 10 + fallos;

    /* 9. Hostil: el 68k escribe en la dirección 4, en la página 0 de solo lectura. El kernel termina la caja (sin el
       Enforcer, que aún no está: ver la bitácora); si sigue viva, mal. */
    di("el 68k escribe en la dirección 4 (ExecBase), en la página 0 de solo lectura: la caja tiene que morir\n");
    v = ejecuta("mala", prog_mala, prog_mala_fin, 0x00140000);
    di("MAL: la caja sigue viva (vector %d)\n", v);
    return 20;
}
