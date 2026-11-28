/*
    DashOS, X7 (paso 3) y X8: el motor RelevoJIT de la caja Amiga (motor.h), en x86-64. RelevoJIT es un crate de Rust
    que va enlazado con pegamento/ y da su interfaz de C (relevo.h). Con su JIT (X8) o con su intérprete (X7): se elige
    al compilar (`make -C caja x86 MOTOR=jit|interprete`, que define MOTOR_JIT a 1 o a 0).

    - Un 68040 con su FPU, con la memoria del 68k en las mismas direcciones que en el proceso (RELEVO_OPT_BASE_ZERO):
      RAM desde M68K_RAM hasta m68k_ram_fin (motor_memoria la sigue cuando crece o encoge) y, en los primeros 64 KB,
      una página del anfitrión (pagina_*: la página 0 de verdad del proceso mide 4 KB, de solo lectura, con ExecBase
      en la dirección 4; las páginas de RelevoJIT miden 64 KB), que hace el acceso de verdad: como con Emu68, escribir
      ahí termina la caja. Lo demás, sin memoria: un acceso da error de bus al 68k (con Emu68, el proceso muere).
    - La llamada nativa "$A0FF <índice>": la línea A del anfitrión lee el índice, deja el PC detrás y llama a
      caja_nativa con D0-D7 y A0-A7.
    - Se corre hasta que el PC llega a 0 (RELEVO_OPT_EXIT_ON_PC_ZERO); desde una función nativa, otra vez (la entrada
      anidada de RelevoJIT), con los registros de fuera guardados aquí.
    - Las interrupciones: la de la caja es el nivel 6 y la NMI (cortar el programa) el 7, por relevo_set_ipl, que vale
      desde cualquier hilo.
    - La FPU para pegamento/ (multitarea): los registros de 80 bits de RelevoJIT pasan tal cual (la mantisa y, aparte,
      el signo y el exponente; motor.h): la precisión extendida no se pierde al cambiar de tarea.
    - El JIT: su memoria de código la da pegamento/ (jit.rs: un objeto de memoria de DashOS proyectado dos veces, para
      escribir en 384 GB y para ejecutar en 388 GB, fuera del alcance del 68k; motor_zonas lo dice). Lo que el 68k
      escribe en su código lo ve él solo; lo que escribe el anfitrión (el cargador, SetFunction...) lo avisa la caja con
      motor_olvidar y motor_olvidar_en_nativa. La línea A, las páginas del anfitrión y las excepciones van por su
      intérprete, y las interrupciones entran entre bloques traducidos.
    - K3: los chips del Amiga (motor_chips). Encendidos, sus dos páginas ($BF0000, los CIA, y $DF0000, los registros
      de los chips) pasan a ser del anfitrión, como la primera: anfitrion_leer y anfitrion_escribir mandan los accesos
      a caja_chip_leer y caja_chip_escribir con la hora de relevo_cycles (los ciclos de 68000 que cuenta el intérprete;
      el JIT los estima). Apagados, vuelven a no tener nada.
    - K8: el contador hacia atrás de los chips (motor.h, motor_ipl_chips): con ellos encendidos, cada relevo_run lleva
      de presupuesto las instrucciones que caben hasta el plazo (por lo bajo); al llegar, correr() pone los chips al
      día y arma el plazo siguiente. Tras una escritura en los chips, también; si el plazo se adelanta,
      relevo_request_stop corta la corrida y correr() vuelve a calcular el presupuesto. El nivel que piden no se
      entrega: relevo_set_ipl sigue siendo solo de la caja (6 y 7).
*/

#include <stdint.h>
#include "caja.h"
#include "motor.h"
#include "plataforma.h"
#include "relevo.h"

#ifndef MOTOR_JIT
#define MOTOR_JIT 1
#endif

static relevo_cpu *cpu;
static volatile int nmi;    /* se pidió cortar: el nivel se queda en 7 hasta el programa siguiente */
static int chips;           /* K3: los chips del Amiga, encendidos (motor_chips) */

/* K8: el plazo de los chips (en la hora del motor: relevo_cycles), el nivel que piden, cuántas veces se llegó al plazo y
   la hora hasta la que se calculó el presupuesto de la corrida en curso. */
static uint64_t plazo = UINT64_MAX;
static unsigned ipl_chips;
static uint64_t plazos_chips;
static uint64_t fin_corrida = UINT64_MAX;
/* Para el presupuesto: los ciclos de una instrucción, por lo bajo (las más cortas del 68000 son de 4; la media, más
   de 8). Con más ciclos, la corrida acaba antes del plazo y correr() da otra; si una tanda de instrucciones largas
   (MULU, DIVU...) pasa de esta media, el plazo llega algo tarde. */
#define CICLOS_POR_INSTRUCCION_MIN 16

static void chips_rearmar(uint64_t hora)
{
    struct caja_plazo p = caja_chips_plazo(hora);
    ipl_chips = p.ipl;
    plazo = p.siguiente;
}

unsigned motor_ipl_chips(void) { return ipl_chips; }
uint64_t motor_plazos_chips(void) { return plazos_chips; }

const char *motor_nombre(void)
{
    return MOTOR_JIT ? "JIT de RelevoJIT en un proceso de usuario de x86-64 (X8)"
                     : "Intérprete de RelevoJIT en un proceso de usuario de x86-64 (U7)";
}

/* --- La memoria de código del JIT (pegamento/src/jit.rs) --- */

int caja_codigo_proyectar(void *ctx, size_t tam, void **rw, void **rx);
void caja_codigo_quitar(void *ctx, void *rw, void *rx, size_t tam);
void caja_codigo_escrito(void *ctx, size_t desde, size_t tam);

/* 16 MB, lo más que da el kernel en un objeto: si se llena, RelevoJIT tira lo traducido y empieza de nuevo. */
#define TAM_CODIGO (16u << 20)
static void *codigo_rw, *codigo_rx;

static int codigo_proyectar(void *ctx, size_t tam, void **rw, void **rx)
{
    int e = caja_codigo_proyectar(ctx, tam, rw, rx);
    if (e == 0) {
        codigo_rw = *rw;
        codigo_rx = *rx;
    }
    return e;
}

static const relevo_code_memory codigo = {
    .size = TAM_CODIGO,
    .map = codigo_proyectar,
    .unmap = caja_codigo_quitar,
    .end_write = caja_codigo_escrito,
};

/* Las estadísticas del JIT (de tests/hooks.h de RelevoJIT, que no es su interfaz pública): los bloques traducidos,
   para motor_unidades. */
typedef struct jit_stats {
    uint64_t blocks, insns, runs, bails, interp, flushes, links, dropped, host;
} jit_stats;
void jit_get_stats(const relevo_cpu *cpu, jit_stats *out);

static void volcar(relevo_regs *r)
{
    di("D0-D7: %08x %08x %08x %08x %08x %08x %08x %08x\n", r->d[0], r->d[1], r->d[2], r->d[3], r->d[4], r->d[5], r->d[6],
       r->d[7]);
    di("A0-A7: %08x %08x %08x %08x %08x %08x %08x %08x\n", r->a[0], r->a[1], r->a[2], r->a[3], r->a[4], r->a[5], r->a[6],
       r->a[7]);
    di("PC=%08x SR=%04x USP=%08x ISP=%08x\n", r->pc, r->sr, r->usp, r->isp);
}

/* --- Lo que el anfitrión hace por el 68k --- */

/* La primera página de 64 KB: el acceso se hace de verdad en el proceso, como con Emu68. Los 4 KB de la página 0 se
   leen (ExecBase en la dirección 4); escribir en ellos, o tocar lo de más arriba (sin proyectar), es un fallo de página
   y el kernel termina la caja (la prueba 9 de caja.c lo espera). */
static uint32_t pagina_leer(void *ctx, uint32_t dir, unsigned largo)
{
    (void)ctx;
    uint32_t v = 0;
    /* Por su proyección de escribir, que es la misma memoria (en Linux no está en la dirección 0). */
    for (unsigned i = 0; i < largo; i++) v = v << 8 | *(volatile uint8_t *)(uintptr_t)(PAGINA0_RW + dir + i);
    return v;
}

static void pagina_escribir(void *ctx, uint32_t dir, uint32_t v, unsigned largo)
{
    (void)ctx;
    for (unsigned i = 0; i < largo; i++) *(volatile uint8_t *)(uintptr_t)(dir + i) = (uint8_t)(v >> (8 * (largo - 1 - i)));
}

/* K3: las páginas de los chips del Amiga, cada una de 64 KB, como las de RelevoJIT. */
#define CHIPS_CIA    0x00BF0000u    /* los CIA: CIA-A en $BFE001, CIA-B en $BFD000 */
#define CHIPS_CUSTOM 0x00DF0000u    /* los registros de los chips: $DFF000-$DFF1FF */

/* Los accesos a las páginas del anfitrión: la primera (pagina_*) y, con los chips encendidos, las suyas. */
static uint32_t anfitrion_leer(void *ctx, uint32_t dir, unsigned largo)
{
    if (dir < RELEVO_PAGE_SIZE) return pagina_leer(ctx, dir, largo);
    return caja_chip_leer(dir, largo, relevo_cycles(cpu));
}

static void anfitrion_escribir(void *ctx, uint32_t dir, uint32_t v, unsigned largo)
{
    if (dir < RELEVO_PAGE_SIZE) {
        pagina_escribir(ctx, dir, v, largo);
        return;
    }
    uint64_t hora = relevo_cycles(cpu);
    caja_chip_escribir(dir, v, largo, hora);
    /* K8: una escritura puede cambiar el nivel o adelantar el plazo (INTENA, un temporizador...). */
    chips_rearmar(hora);
    if (plazo < fin_corrida) relevo_request_stop(cpu);
}

static relevo_trap_result linea_a(void *ctx, relevo_cpu *c, uint16_t op)
{
    (void)ctx;
    if (op != 0xA0FF) return RELEVO_TRAP_EXCEPTION;
    relevo_regs *r = relevo_get_regs(c);
    uint32_t indice = relevo_read(c, r->pc, 2);
    r->pc += 2;
    motor_regs m;
    for (int i = 0; i < 8; i++) { m.d[i] = r->d[i]; m.a[i] = r->a[i]; }
    caja_nativa(&m, indice);
    for (int i = 0; i < 8; i++) { r->d[i] = m.d[i]; r->a[i] = m.a[i]; }
    return RELEVO_TRAP_HANDLED;
}

int motor_iniciar(void)
{
    relevo_config cfg = { 0 };
    cfg.model = RELEVO_68040;
    cfg.fpu = RELEVO_FPU_INTERNAL;
    cfg.engine = MOTOR_JIT ? RELEVO_ENGINE_JIT : RELEVO_ENGINE_INTERP;
    cfg.code_memory = MOTOR_JIT ? &codigo : 0;
    cfg.memory = 0;
    cfg.memory_size = 0x100000000ULL - RELEVO_PAGE_SIZE;   /* todo lo que se puede ver; solo lo que se mapea vale */
    cfg.options = RELEVO_OPT_EXIT_ON_PC_ZERO | RELEVO_OPT_BASE_ZERO;
    cfg.host.read = anfitrion_leer;
    cfg.host.write = anfitrion_escribir;
    cfg.host.line_a = linea_a;
    cpu = relevo_create(&cfg);
    if (!cpu) {
        di("RelevoJIT no arranca\n");
        return 2;
    }
    /* relevo_create lo da todo por RAM: nada, la página 0 del anfitrión y la RAM del 68k. */
    if (relevo_map(cpu, 0, cfg.memory_size, RELEVO_PAGE_NONE) != 0
        || relevo_map(cpu, 0, RELEVO_PAGE_SIZE, RELEVO_PAGE_HOST) != 0) {
        di("RelevoJIT: el mapa de páginas no vale\n");
        return 3;
    }
    motor_memoria(M68K_RAM, m68k_ram_fin, 1);
    return 0;
}

void motor_memoria(uint32_t desde, uint32_t hasta, int ram)
{
    if (!cpu || hasta <= desde) return;
    uint32_t a = desde & ~(RELEVO_PAGE_SIZE - 1);
    uint64_t b = ((uint64_t)hasta + RELEVO_PAGE_SIZE - 1) & ~(uint64_t)(RELEVO_PAGE_SIZE - 1);
    if (relevo_map(cpu, a, b - a, ram ? RELEVO_PAGE_RAM : RELEVO_PAGE_NONE) != 0)
        di("RelevoJIT: no se puede mapear [%x, %x)\n", a, (uint32_t)b);
}

uint64_t motor_ciclos(void) { return cpu ? relevo_cycles(cpu) : 0; }

int motor_chips(int encendidos)
{
    if (!cpu) return 0;
    encendidos = encendidos != 0;
    if (encendidos == chips) return 1;
    /* La RAM del 68k empieza en M68K_RAM y no tiene huecos: si llega a la primera página de los chips, exec puede
       tener bloques ahí. */
    if (encendidos && m68k_ram_fin > CHIPS_CIA) return 0;
    relevo_page_kind k = encendidos ? RELEVO_PAGE_HOST : RELEVO_PAGE_NONE;
    if (relevo_map(cpu, CHIPS_CIA, RELEVO_PAGE_SIZE, k) != 0 || relevo_map(cpu, CHIPS_CUSTOM, RELEVO_PAGE_SIZE, k) != 0) {
        di("RelevoJIT: no se pueden mapear las páginas de los chips\n");
        relevo_map(cpu, CHIPS_CIA, RELEVO_PAGE_SIZE, RELEVO_PAGE_NONE);
        relevo_map(cpu, CHIPS_CUSTOM, RELEVO_PAGE_SIZE, RELEVO_PAGE_NONE);
        chips = 0;
        return !encendidos;
    }
    /* Lo traducido con las páginas de antes no vale. */
    relevo_invalidate_all(cpu);
    chips = encendidos;
    /* K8: el contador hacia atrás, armado o no. */
    plazos_chips = 0;
    if (encendidos) chips_rearmar(relevo_cycles(cpu));
    else {
        plazo = UINT64_MAX;
        ipl_chips = 0;
    }
    return 1;
}

/* Corre hasta que el PC llega a 0. Un doble error de bus (el 68k parado) acaba el programa por el vector 2. */
static void correr(void)
{
    for (;;) {
        uint64_t presupuesto = 1u << 22;
        if (chips) {
            /* K8: el contador hacia atrás. */
            uint64_t ahora = relevo_cycles(cpu);
            if (ahora >= plazo) {
                plazos_chips++;
                chips_rearmar(ahora);
            }
            uint64_t n = plazo > ahora ? (plazo - ahora) / CICLOS_POR_INSTRUCCION_MIN : 0;
            if (n == 0) n = 1;
            if (n < presupuesto) presupuesto = n;
            fin_corrida = plazo;
        }
        relevo_status st = relevo_run(cpu, presupuesto);
        if (st == RELEVO_EXIT_PC_ZERO) return;
        if (st == RELEVO_HALTED || st == RELEVO_BAD_NESTING) {
            di("RelevoJIT: el 68k se paró (%u)\n", (unsigned)st);
            volcar(relevo_get_regs(cpu));
            pon16(MARCA, 2);
            return;
        }
        if (st == RELEVO_STOPPED) plat_dormir(1000);
    }
}

void motor_arrancar(uint32_t pc, uint32_t a7, uint32_t a6, uint32_t a0, uint32_t d0, uint32_t super, uint32_t vbr)
{
    relevo_regs *r = relevo_get_regs(cpu);
    for (int i = 0; i < 8; i++) {
        r->d[i] = 0;
        r->a[i] = 0;
        r->fp[i].sign_exponent = 0x7fff;          /* NaN, como Emu68 */
        r->fp[i].mantissa = ~0ULL;
    }
    r->d[0] = d0;
    r->a[0] = a0;
    r->a[6] = a6;
    r->a[7] = a7;
    r->usp = a7;
    r->isp = super;
    r->msp = super;
    r->vbr = vbr;
    r->sr = 0;
    r->pc = pc;
    r->cacr = 0x80008000;
    r->fpcr = r->fpsr = r->fpiar = 0;
    nmi = 0;
    relevo_set_ipl(cpu, 0);
    relevo_invalidate_all(cpu);
    correr();
}

void motor_anidado(uint32_t pc, motor_regs *m, uint32_t sr, uint32_t usp, uint32_t isp, uint32_t msp)
{
    relevo_regs *r = relevo_get_regs(cpu);
    relevo_regs antes = *r;
    for (int i = 0; i < 8; i++) { r->d[i] = m->d[i]; r->a[i] = m->a[i]; }
    r->usp = usp;
    r->isp = isp;
    r->msp = msp;
    r->sr = (uint16_t)sr;
    r->pc = pc;
    correr();
    for (int i = 0; i < 8; i++) { m->d[i] = r->d[i]; m->a[i] = r->a[i]; }
    *r = antes;
    /* K8: la corrida de fuera tiene el presupuesto de antes; que lo vuelva a calcular con el plazo de ahora. */
    if (chips) relevo_request_stop(cpu);
}

uint32_t motor_sr(void) { return relevo_get_regs(cpu)->sr; }

void motor_pilas(uint32_t *usp, uint32_t *isp, uint32_t *msp)
{
    relevo_regs *r = relevo_get_regs(cpu);
    *usp = r->usp;
    *isp = r->isp;
    *msp = r->msp;
    /* La activa está en A7 (la copia de su campo es vieja). */
    if (!(r->sr & RELEVO_SR_S)) *usp = r->a[7];
    else if (r->sr & RELEVO_SR_M) *msp = r->a[7];
    else *isp = r->a[7];
}

void motor_poner_usp(uint32_t usp)
{
    relevo_regs *r = relevo_get_regs(cpu);
    if (r->sr & RELEVO_SR_S) r->usp = usp;
    else r->a[7] = usp;
}

uint32_t motor_d0(void) { return relevo_get_regs(cpu)->d[0]; }

void motor_cortar(void)
{
    nmi = 1;
    relevo_set_ipl(cpu, 7);
}

void motor_interrumpir(void)
{
    if (!nmi) relevo_set_ipl(cpu, 6);
}

void motor_interrupcion_atendida(void)
{
    if (!nmi) relevo_set_ipl(cpu, 0);
}

void motor_olvidar(void) { relevo_invalidate_all(cpu); }
void motor_olvidar_en_nativa(void) { relevo_invalidate_all(cpu); }

uint32_t motor_unidades(void)
{
    if (!MOTOR_JIT) return 0;
    jit_stats s;
    jit_get_stats(cpu, &s);
    return (uint32_t)s.blocks;
}

/* Su estado y, con el JIT, su código para escribir y para ejecutar; con el intérprete, la variable y su código. */
void motor_zonas(uintptr_t z[MOTOR_ZONAS])
{
    z[0] = (uintptr_t)cpu;
    z[1] = MOTOR_JIT ? (uintptr_t)codigo_rw : (uintptr_t)&cpu;
    z[2] = MOTOR_JIT ? (uintptr_t)codigo_rx : (uintptr_t)motor_iniciar;
}

void motor_volcar(void) { volcar(relevo_get_regs(cpu)); }

/* --- La coma flotante: 80 bits, tal cual (la mantisa en v[i] y el signo y el exponente en v[11..12], motor.h) --- */

void caja_fp_leer(uint64_t *v)
{
    relevo_regs *r = relevo_get_regs(cpu);
    v[11] = v[12] = 0;
    for (int i = 0; i < 8; i++) {
        v[i] = r->fp[i].mantissa;
        v[11 + i / 4] |= (uint64_t)r->fp[i].sign_exponent << (16 * (i % 4));
    }
    v[8] = r->fpsr;
    v[9] = r->fpiar;
    v[10] = r->fpcr;
}

void caja_fp_poner(const uint64_t *v)
{
    relevo_regs *r = relevo_get_regs(cpu);
    for (int i = 0; i < 8; i++) {
        r->fp[i].mantissa = v[i];
        r->fp[i].sign_exponent = (uint16_t)(v[11 + i / 4] >> (16 * (i % 4)));
    }
    r->fpsr = (uint32_t)v[8];
    r->fpiar = (uint32_t)v[9];
    r->fpcr = (uint32_t)v[10];
}
