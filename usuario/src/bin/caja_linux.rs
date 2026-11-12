//! La caja Linux (H9, `docs/bitacoras/caja-linux.md`): el servicio que crea y corre máquinas virtuales.
//!
//! Primer paso: los invitados de prueba de `invitados/pruebas.s` (unas instrucciones AArch64 sin sistema) con un PL011
//! en 0x09000000 que emula el servicio: lo que escriben sale en la consola. Mide cuánto cuesta entrar y salir del
//! invitado y prueba los hostiles: leer y escribir fuera de su memoria, un bucle sin fin, apagar la máquina por SMC y
//! tocar el temporizador del kernel. Cada máquina que hace algo prohibido se para (se cierra su llave) y el sistema
//! sigue.
//!
//! Tercer paso: las interrupciones (`invitados/interrupciones.s`: el temporizador virtual, una SPI del servicio, PSCI,
//! y hostiles con el GIC), y un kernel de Linux con su initramfs, con los aparatos del crate `caja-linux/vmm`, hasta
//! busybox.
//!
//! Llave 1: el permiso de máquinas virtuales, que le da `inicio`. Llaves 2 y 3, si las hay: la Image de Linux y su
//! initramfs.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use caja_vmm::arbol;
use caja_vmm::arranque;
use caja_vmm::disposicion;
use caja_vmm::gic::{self, Distribuidor};
use caja_vmm::maquina::{Maquina as Vmm, Operacion, SalidaMmio};
use caja_vmm::memoria::{FueraDeMemoria, MemoriaInvitado, Ram};
use caja_vmm::pl011::Pl011;
use caja_vmm::psci::{self, AccionPsci, Psci};
use caja_vmm::virtio::bloque::{Bloque, Bloques, ErrorBloques};
use caja_vmm::virtio::consola::{Consola, ExtremoConsola};
use caja_vmm::virtio::mmio::TransporteMmio;
use core::cell::RefCell;
use dashos::maquina::{self as m, Informe};
use dashos::println;

dashos::programa!(principal);
dashos::monton!();

core::arch::global_asm!(include_str!("../../invitados/pruebas.s"));
core::arch::global_asm!(include_str!("../../invitados/interrupciones.s"));

unsafe extern "C" {
    static invitado_hola: u8;
    static invitado_hola_fin: u8;
    static invitado_eco: u8;
    static invitado_eco_fin: u8;
    static invitado_lee_fuera: u8;
    static invitado_lee_fuera_fin: u8;
    static invitado_escribe_fuera: u8;
    static invitado_escribe_fuera_fin: u8;
    static invitado_bucle: u8;
    static invitado_bucle_fin: u8;
    static invitado_apaga: u8;
    static invitado_apaga_fin: u8;
    static invitado_reloj: u8;
    static invitado_reloj_fin: u8;
    static invitado_irq: u8;
    static invitado_irq_fin: u8;
    static invitado_gic_hostil: u8;
    static invitado_gic_hostil_fin: u8;
    static invitado_tormenta: u8;
    static invitado_tormenta_fin: u8;
    static invitado_el2: u8;
    static invitado_el2_fin: u8;
}

const PERMISO: u32 = 1;
/// Dónde ve el invitado su RAM, y cuánta tiene.
const RAM_IPA: u64 = 0x4000_0000;
const RAM: usize = 64 * 1024;
/// Dónde la proyecta el servicio para cargar el invitado.
const VA: usize = 0x1000_0000;
/// El PL011 emulado: el registro de datos y el de estado.
const UART: u64 = 0x0900_0000;
const UART_FR: u64 = UART + 0x18;

/// Los bytes de un invitado, entre su etiqueta y la de su fin.
fn codigo(inicio: &'static u8, fin: &'static u8) -> &'static [u8] {
    let (i, f) = (inicio as *const u8 as usize, fin as *const u8 as usize);
    // SAFETY: las dos etiquetas son del mismo trozo de .rodata (pruebas.s), la de fin después.
    unsafe { core::slice::from_raw_parts(inicio as *const u8, f - i) }
}

/// Una máquina con `codigo` al principio de su RAM: la llave, o el error de `maquina_crear`.
fn maquina(codigo: &[u8]) -> Result<u32, i64> {
    let mem = dashos::memoria_crear(RAM) as i32;
    if mem <= 0 {
        return Err(mem as i64);
    }
    let mem = mem as u32;
    if dashos::memoria_proyectar(mem, VA, true) != 0 {
        dashos::cerrar(mem);
        return Err(-1);
    }
    // SAFETY: recién proyectada aquí, RAM bytes, y el invitado aún no existe.
    let ram = unsafe { core::slice::from_raw_parts_mut(VA as *mut u8, RAM) };
    ram[..codigo.len()].copy_from_slice(codigo);
    let r = dashos::maquina_crear(PERMISO, mem, RAM_IPA, RAM_IPA, 0);
    // La máquina sujeta su memoria: el servicio ya no la necesita.
    dashos::memoria_quitar(VA);
    dashos::cerrar(mem);
    if r > 0 { Ok(r as u32) } else { Err(r) }
}

/// Cómo acabó una máquina.
enum Fin {
    /// HVC #n con x0.
    Hvc(u64, u64),
    /// Un acceso a una dirección que no es suya ni del UART: (dirección, si escribía).
    Fuera(u64, bool),
    /// Seguía en marcha al pasar el plazo: cuántas veces la sacó el reloj.
    Bucle(u64),
    /// Tocó un registro del sistema atrapado: (su codificación, si escribía).
    Registro(u64, bool),
    /// Ya escribió todas las líneas que se le dejan.
    Lineas,
    /// Una trampa o un fallo: ESR y PC.
    Fallo(u64, u64),
    /// La llamada falló.
    Error(i64),
}

/// Cómo se corre una máquina.
struct Opciones<'a> {
    /// Lo que va delante de cada línea que escribe por el UART.
    prefijo: &'a str,
    /// Cuántas líneas se le dejan escribir.
    lineas: usize,
    /// Los registros atrapados (depuración, contadores) se leen como 0 y lo escrito se ignora, como hace falta para un
    /// kernel de Linux; si no, la máquina se para.
    registros: bool,
    plazo_us: u64,
}

/// Corre la máquina hasta que acaba o pasa el plazo, emulando el UART: cada línea que escribe sale en la consola.
fn correr(vm: u32, o: &Opciones) -> Fin {
    let mut inf = Informe::default();
    let mut linea = [0u8; 160];
    let mut n = 0;
    let mut lineas = 0;
    let mut valor = 0;
    let mut sacado = 0;
    let t0 = dashos::ahora_us();
    loop {
        let r = dashos::maquina_correr(vm, &mut inf, valor);
        valor = 0;
        match r {
            m::HVC => return Fin::Hvc(inf.tamano, inf.valor),
            m::ESCRITURA if inf.direccion == UART => {
                let c = inf.valor as u8;
                if c == b'\n' || n == linea.len() {
                    println!("  [caja-linux] {}{}", o.prefijo, core::str::from_utf8(&linea[..n]).unwrap_or("?"));
                    n = 0;
                    lineas += 1;
                    if lineas >= o.lineas {
                        return Fin::Lineas;
                    }
                }
                if c != b'\n' && c != b'\r' {
                    linea[n] = c;
                    n += 1;
                }
            }
            // FR: nunca lleno.
            m::LECTURA if inf.direccion == UART_FR => valor = 0,
            m::LECTURA => return Fin::Fuera(inf.direccion, false),
            m::ESCRITURA => return Fin::Fuera(inf.direccion, true),
            m::REGISTRO if o.registros => valor = 0,
            m::REGISTRO => return Fin::Registro(inf.direccion, inf.x1 == 0),
            m::WFI | m::INTERRUMPIDO => {
                if r == m::INTERRUMPIDO {
                    sacado += 1;
                }
                if dashos::ahora_us() - t0 > o.plazo_us {
                    return Fin::Bucle(sacado);
                }
            }
            m::FALLO => return Fin::Fallo(inf.esr, inf.pc),
            e => return Fin::Error(e),
        }
    }
}

/// Lo de siempre para los invitados de prueba.
fn prueba(plazo_us: u64) -> Opciones<'static> {
    Opciones { prefijo: "el invitado dice: ", lineas: 20, registros: false, plazo_us }
}

fn frecuencia() -> u64 {
    let f: u64;
    // SAFETY: CNTFRQ_EL0 se lee desde EL0.
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f.max(1)
}

/// Un invitado hostil: tiene que acabar como dice `esperado`, y entonces se para su máquina (se cierra su llave).
fn hostil(nombre: &str, codigo: &[u8], esperado: impl Fn(&Fin) -> bool) -> bool {
    let vm = match maquina(codigo) {
        Ok(v) => v,
        Err(e) => {
            println!("  [caja-linux] hostil ({}): no se pudo crear la máquina ({}); MAL", nombre, e);
            return false;
        }
    };
    let fin = correr(vm, &prueba(100_000));
    let bien = esperado(&fin);
    let que = match fin {
        Fin::Fuera(d, true) => texto(format_args!("escribe fuera de su memoria ({:#x})", d)),
        Fin::Fuera(d, false) => texto(format_args!("lee fuera de su memoria ({:#x})", d)),
        Fin::Bucle(n) => texto(format_args!("no acaba: el reloj lo sacó {} veces en 100 ms", n)),
        Fin::Registro(c, escribe) => texto(format_args!("{} un registro del sistema atrapado (codificación {:#x})", if escribe { "escribe en" } else { "lee" }, c)),
        Fin::Lineas => texto(format_args!("escribe demasiado")),
        Fin::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x}, clase {:#x})", pc, esr, esr >> 26)),
        Fin::Hvc(n, x0) => texto(format_args!("acaba con HVC #{} (x0 = {})", n, x0)),
        Fin::Error(e) => texto(format_args!("error {}", e)),
    };
    // Una máquina muerta en un fallo no vuelve a correr.
    let otra = dashos::maquina_correr(vm, &mut Informe::default(), 0);
    dashos::cerrar(vm);
    let muerta = !matches!(fin, Fin::Fallo(..)) || otra == dashos::E_CERRADO;
    println!("  [caja-linux] hostil ({}): {}; se para su máquina: {}", nombre, que.as_str(), if bien && muerta { "bien" } else { "MAL" });
    bien && muerta
}

/// Un texto corto con formato, sin montón.
struct Texto {
    b: [u8; 160],
    n: usize,
}

impl Texto {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("?")
    }
}

impl core::fmt::Write for Texto {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let k = s.len().min(self.b.len() - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s.as_bytes()[..k]);
        self.n += k;
        Ok(())
    }
}

fn texto(a: core::fmt::Arguments) -> Texto {
    let mut t = Texto { b: [0; 160], n: 0 };
    let _ = core::fmt::Write::write_fmt(&mut t, a);
    t
}

// --- Tercer paso: las interrupciones del invitado, y un Linux con sus aparatos ---
//
// El kernel da el GIC virtual del GICv2 (la GICV proyectada donde el invitado espera su GICC, los registros de lista y
// el temporizador virtual con su bit HW). El servicio emula el distribuidor y los aparatos con el crate
// `caja-linux/vmm` (virtio-mmio, PL011, PSCI, el árbol de dispositivos y la carga de la Image) y llena los registros
// de lista que le tocan con lo que diga el distribuidor.

/// Lo que se proyecta del GIC para el invitado: la interfaz de CPU en GICC; el distribuidor (GICD) lo emula el servicio.
const GICC: u64 = disposicion::GICC;
/// El timbre del invitado de interrupciones: escribir aquí pide al servicio la SPI 48.
const TIMBRE: u64 = 0x0a00_0000;

/// La RAM del invitado vista desde el servicio, con la barrera de verdad (las colas de virtio se comparten con él).
struct RamCaja(Ram<&'static mut [u8]>);

impl MemoriaInvitado for RamCaja {
    fn leer(&self, dir: u64, buf: &mut [u8]) -> Result<(), FueraDeMemoria> {
        self.0.leer(dir, buf)
    }
    fn escribir(&mut self, dir: u64, datos: &[u8]) -> Result<(), FueraDeMemoria> {
        self.0.escribir(dir, datos)
    }
    fn barrera(&self) {
        dashos::barrera();
    }
}

/// Cuántas veces salió el invitado, y por qué.
#[derive(Default)]
struct Cuentas {
    salidas: u64,
    aparatos: u64,
    abortos: u64,
    inyectadas: u64,
    wfi: u64,
    dormido_us: u64,
    interrumpido: u64,
    registros: u64,
    psci: u64,
}

/// Cómo acabó una máquina corrida con sus aparatos.
enum FinCaja {
    /// HVC #n (no de PSCI) con x0..x3.
    Hvc(u64, [u64; 4]),
    /// PSCI SYSTEM_OFF o CPU_OFF de su única vCPU.
    Apagada,
    Reinicio,
    Plazo,
    Fallo(u64, u64),
    Error(i64),
}

/// Una máquina con sus aparatos (`caja_vmm::maquina::Maquina`), su PSCI y sus registros de lista. Una vCPU.
struct Caja<M: MemoriaInvitado> {
    vm: u32,
    maq: Vmm<M>,
    psci: Psci,
    irq: m::Interrupciones,
    /// La interrupción que hay en cada registro de lista del servicio.
    cargadas: [Option<u32>; m::LRS],
    cuentas: Cuentas,
}

impl<M: MemoriaInvitado> Caja<M> {
    fn nueva(vm: u32, maq: Vmm<M>, ram: &[(u64, u64)]) -> Self {
        Self { vm, maq, psci: Psci::nuevo(1, ram), irq: m::Interrupciones::default(), cargadas: [None; m::LRS], cuentas: Cuentas::default() }
    }

    /// Los registros de lista que el invitado ya terminó, al distribuidor; los libres, con lo que tenga pendiente; y
    /// cómo tiene el temporizador virtual, para el kernel.
    fn llenar_lrs(&mut self) {
        let n = (self.irq.lrs as usize).min(m::LRS);
        for i in 0..n {
            if let Some(id) = self.cargadas[i]
                && gic::lr_libre(self.irq.lr[i] as u32)
            {
                self.maq.gic.terminada(0, id);
                self.cargadas[i] = None;
                self.irq.lr[i] = 0;
            }
        }
        let mut pendientes = self.maq.gic.pendientes(0).into_iter();
        for i in 0..n {
            if self.cargadas[i].is_none() {
                let Some(e) = pendientes.next() else { break };
                // Con EOI: cuando el invitado la termina, el GIC da la interrupción de mantenimiento, que lo saca, y el
                // servicio vuelve a mirar la línea (las de los aparatos son por nivel).
                self.irq.lr[i] = gic::lr_v2(&e, true) as u64;
                self.maq.gic.cargada(0, &e);
                self.cargadas[i] = Some(e.intid);
                self.cuentas.inyectadas += 1;
            }
        }
        self.irq.temporizador = match self.maq.gic.privada(0, INTID_TEMPORIZADOR) {
            Some((true, p, g1)) => 1 | (p as u64) << 8 | (g1 as u64) << 16,
            _ => 0,
        };
    }

    fn poner_x0(&self, v: u64) {
        let mut r = [0u64; m::REGISTROS];
        if dashos::maquina_registros(self.vm, &mut r) == 0 {
            r[0] = v;
            dashos::maquina_poner_registros(self.vm, &r);
        }
    }

    /// Un WFI: si no hay nada para el invitado, el servicio duerme hasta el plazo de su temporizador (como mucho 10 ms).
    fn esperar(&mut self, inf: &Informe) {
        if inf.senales & m::PENDIENTE != 0 || self.maq.gic.hay_para(0) {
            return;
        }
        let ahora = contador();
        let us = if inf.plazo == u64::MAX {
            10_000
        } else if inf.plazo <= ahora {
            return;
        } else {
            ((inf.plazo - ahora) as u128 * 1_000_000 / frecuencia() as u128).min(10_000) as u64
        };
        if us >= 100 {
            dashos::dormir_us(us);
            self.cuentas.dormido_us += us;
        }
    }

    /// Corre la vCPU hasta que acaba o pasa el plazo. Con `psci`, un HVC #0 es una llamada de PSCI (DEN 0022).
    fn correr(&mut self, plazo_us: u64, psci: bool) -> FinCaja {
        let mut inf = Informe::default();
        let mut valor = 0;
        let t0 = dashos::ahora_us();
        loop {
            self.llenar_lrs();
            let r = dashos::maquina_correr_irq(self.vm, &mut inf, valor, &mut self.irq);
            valor = 0;
            self.cuentas.salidas += 1;
            match r {
                m::LECTURA | m::ESCRITURA => {
                    self.cuentas.aparatos += 1;
                    let op = if r == m::LECTURA { Operacion::Leer } else { Operacion::Escribir(inf.valor) };
                    match self.maq.mmio(0, inf.direccion, inf.tamano as u8, op) {
                        SalidaMmio::Leido(v) => valor = v,
                        SalidaMmio::Escrito => {}
                        // Nada en esa dirección: un aborto externo, como el de un bus de verdad.
                        SalidaMmio::SinAparato => {
                            self.cuentas.abortos += 1;
                            dashos::maquina_aborto(self.vm);
                        }
                    }
                }
                m::HVC if psci && inf.tamano == 0 => {
                    self.cuentas.psci += 1;
                    match self.psci.atender(0, [inf.valor, inf.x1, inf.x2, inf.x3]) {
                        AccionPsci::Responder(v) => self.poner_x0(v),
                        AccionPsci::Suspender => self.poner_x0(0),
                        // Con una vCPU no hay otra que encender.
                        AccionPsci::Encender { .. } => self.poner_x0(psci::NO_ADMITIDA as i64 as u64),
                        AccionPsci::ApagarEsta | AccionPsci::ApagarMaquina => return FinCaja::Apagada,
                        AccionPsci::ReiniciarMaquina => return FinCaja::Reinicio,
                    }
                }
                m::HVC => return FinCaja::Hvc(inf.tamano, [inf.valor, inf.x1, inf.x2, inf.x3]),
                m::REGISTRO => {
                    // Depuración y contadores de rendimiento: de solo cero.
                    self.cuentas.registros += 1;
                    valor = 0;
                }
                m::WFI => {
                    self.cuentas.wfi += 1;
                    self.esperar(&inf);
                }
                m::INTERRUMPIDO => self.cuentas.interrumpido += 1,
                m::FALLO => return FinCaja::Fallo(inf.esr, inf.pc),
                e => return FinCaja::Error(e),
            }
            if dashos::ahora_us() - t0 > plazo_us {
                return FinCaja::Plazo;
            }
        }
    }
}

/// El temporizador virtual del invitado: PPI 11, INTID 27 (el kernel lo pone en su registro de lista).
const INTID_TEMPORIZADOR: u32 = 27;

fn contador() -> u64 {
    let c: u64;
    // SAFETY: CNTVCT_EL0 se lee desde EL0.
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) c) };
    c
}

/// Una máquina de prueba con su GICV. `None` (y lo dice) si no se puede.
fn maquina_gic(nombre: &str, codigo: &[u8]) -> Option<u32> {
    let vm = match maquina(codigo) {
        Ok(v) => v,
        Err(e) => {
            println!("  [caja-linux] {}: no se pudo crear la máquina ({}); MAL", nombre, e);
            return None;
        }
    };
    let r = dashos::maquina_gicv(vm, GICC);
    if r != 0 {
        println!("  [caja-linux] {}: sin GIC virtual ({}); MAL", nombre, r);
        dashos::cerrar(vm);
        return None;
    }
    Some(vm)
}

/// El temporizador virtual (con el bit HW), una SPI del servicio en un registro de lista (con la interrupción de
/// mantenimiento al terminarla) y PSCI contestado en x0.
fn prueba_interrupciones(codigo: &[u8]) -> bool {
    let Some(vm) = maquina_gic("interrupciones", codigo) else { return false };
    let mut inf = Informe::default();
    let mut irq = m::Interrupciones { temporizador: 1 | 0x80 << 8, ..Default::default() };
    let mut timbre = false;
    let mut terminada = false;
    let mut salidas = 0u64;
    let t0 = dashos::ahora_us();
    let fin = loop {
        let r = dashos::maquina_correr_irq(vm, &mut inf, 0, &mut irq);
        salidas += 1;
        if timbre && !terminada && gic::lr_libre(irq.lr[0] as u32) {
            terminada = true;
            irq.lr[0] = 0;
        }
        match r {
            m::ESCRITURA if inf.direccion == TIMBRE && irq.lrs > 0 => {
                let e = gic::Entrega { intid: 48, prioridad: 0x80, grupo1: false, origen: None };
                irq.lr[0] = gic::lr_v2(&e, true) as u64;
                timbre = true;
            }
            m::HVC if inf.tamano == 0 && inf.valor == psci::VERSION as u64 => {
                let mut r = [0u64; m::REGISTROS];
                dashos::maquina_registros(vm, &mut r);
                r[0] = 0x1_0000;
                dashos::maquina_poner_registros(vm, &r);
            }
            m::HVC => break Some((inf.tamano, inf.valor, inf.x1, inf.x2)),
            m::WFI | m::INTERRUMPIDO => {}
            _ => break None,
        }
        if dashos::ahora_us() - t0 > 2_000_000 {
            break None;
        }
    };
    dashos::cerrar(vm);
    let bien = matches!(fin, Some((1, 3, 1, 0x1_0000))) && terminada;
    match fin {
        Some((1, t, s, v)) => println!(
            "  [caja-linux] interrupciones: el temporizador virtual {} veces, la SPI 48 del servicio {} (su registro de lista, libre al terminarla: {}), PSCI {}.{} en x0, {} salidas: {}",
            t, s, if terminada { "sí" } else { "no" }, v >> 16, v & 0xffff, salidas, if bien { "bien" } else { "MAL" }
        ),
        Some((n, x0, _, _)) => println!("  [caja-linux] interrupciones: HVC #{} con x0 = {:#x} (una excepción inesperada); MAL", n, x0),
        None => println!(
            "  [caja-linux] interrupciones: no acaba como debe (motivo {}, PC {:#x}, dirección {:#x}, {} salidas); MAL",
            inf.motivo, inf.pc, inf.direccion, salidas
        ),
    }
    bien
}

/// Hostiles con el GIC: el invitado escribe al azar por su GICV y su distribuidor y toca la GICH del anfitrión; una
/// tormenta del temporizador; un registro de EL2. En todos, el sistema sigue.
fn prueba_gic_hostil(al_azar: &[u8], tormenta: &[u8], el2: &[u8]) -> bool {
    let mut bien = true;
    if let Some(vm) = maquina_gic("hostil (GIC al azar)", al_azar) {
        let maq = Vmm::nueva(Ram::nueva(RAM_IPA, 0), Distribuidor::nuevo(1, disposicion::NUM_SPIS), disposicion::GICD);
        let mut caja = Caja::nueva(vm, maq, &[(RAM_IPA, RAM as u64)]);
        let fin = caja.correr(5_000_000, false);
        dashos::cerrar(vm);
        let ok = matches!(fin, FinCaja::Hvc(0, [2, ..])) && caja.cuentas.abortos == 2;
        let que = match fin {
            FinCaja::Hvc(0, x) => texto(format_args!("acaba con {} abortos externos", x[0])),
            FinCaja::Hvc(n, _) => texto(format_args!("HVC #{}: una excepción inesperada", n)),
            FinCaja::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x})", pc, esr)),
            FinCaja::Plazo => texto(format_args!("no acaba")),
            _ => texto(format_args!("acaba de otra forma")),
        };
        println!(
            "  [caja-linux] hostil (GIC al azar): 4096 escrituras en su GICV y 4096 en su distribuidor, {} de ellas al servicio; toca la GICH del anfitrión: {}; {}",
            caja.cuentas.aparatos, que.as_str(), if ok { "bien" } else { "MAL" }
        );
        bien &= ok;
    } else {
        bien = false;
    }
    if let Some(vm) = maquina_gic("hostil (tormenta)", tormenta) {
        let mut inf = Informe::default();
        let mut irq = m::Interrupciones { temporizador: 1 | 0x80 << 8, ..Default::default() };
        let t0 = dashos::ahora_us();
        let mut sacado = 0;
        let mut otro = 0;
        while dashos::ahora_us() - t0 < 100_000 {
            match dashos::maquina_correr_irq(vm, &mut inf, 0, &mut irq) {
                m::INTERRUMPIDO => sacado += 1,
                m::WFI => {}
                r => {
                    otro = r;
                    break;
                }
            }
        }
        dashos::cerrar(vm);
        let ok = otro == 0 && sacado > 0;
        println!(
            "  [caja-linux] hostil (tormenta del temporizador): no la reconoce nunca; el reloj lo sacó {} veces en 100 ms: {}",
            sacado, if ok { "bien" } else { "MAL" }
        );
        bien &= ok;
    } else {
        bien = false;
    }
    bien &= hostil("registro de EL2", el2, |f| matches!(f, Fin::Fallo(esr, _) if esr >> 26 == 0x20));
    bien
}

// --- Linux ---

/// Llave 2, si la hay (`make arranque-virt-linux`): la Image de un kernel de Linux para arm64, solo de lectura. Llave 3:
/// su initramfs (busybox).
const LINUX: u32 = 2;
const INITRD: u32 = 3;
const VA_IMAGEN: usize = 0x2000_0000;
const VA_INITRD: usize = 0x3800_0000;
/// La RAM del invitado de Linux: 256 MB (lo de serie con 2 GB), en objetos de 16 MB (el tope de `memoria_crear`),
/// proyectados seguidos aquí.
const VA_RAM_LINUX: usize = 0x4000_0000;
const TROZO: usize = 16 << 20;
const RAM_LINUX: usize = 256 << 20;
const TROZOS: usize = RAM_LINUX / TROZO;
const RAM_MINIMA: usize = 128 << 20;
/// La consola temprana por el PL011; la de verdad, virtio-console (hvc0). `caja.escribir` pide al /init de prueba que
/// escriba en el disco.
const BOOTARGS: &str = "earlycon=pl011,0x09000000 console=hvc0 nokaslr panic=-1 caja.escribir";
/// El disco: 1 MB en memoria, con esto al principio. El /init escribe `ESCRITO` en el sector 8.
const MARCA_DISCO: &[u8] = b"DISCO-CAJA-LINUX";
const ESCRITO: &[u8] = b"ESCRITO-POR-LINUX";
const TAM_DISCO: usize = 1 << 20;
/// Como mucho se sacan tantas líneas de Linux por el registro.
const MAX_LINEAS: u64 = 2000;

#[derive(Default)]
struct EstadoConsola {
    lineas: u64,
    busybox: bool,
    fin: bool,
}

/// Un extremo de consola (PL011 o virtio-console) hacia el registro de DashOS: cada línea, con "linux: " delante.
struct Registro {
    linea: Vec<u8>,
    estado: Rc<RefCell<EstadoConsola>>,
}

impl Registro {
    fn nuevo(estado: &Rc<RefCell<EstadoConsola>>) -> Box<Self> {
        Box::new(Self { linea: Vec::new(), estado: estado.clone() })
    }

    fn sacar(&mut self) {
        let t = String::from_utf8_lossy(&self.linea);
        let mut e = self.estado.borrow_mut();
        e.lineas += 1;
        e.busybox |= t.contains("BusyBox v");
        e.fin |= t.contains("caja-linux: fin de la prueba");
        if e.lineas <= MAX_LINEAS {
            println!("  [caja-linux] linux: {}", t);
        }
        self.linea.clear();
    }
}

impl ExtremoConsola for Registro {
    fn salida(&mut self, datos: &[u8]) {
        for &c in datos {
            if c == b'\n' {
                self.sacar();
            } else if c != b'\r' {
                self.linea.push(c);
                if self.linea.len() >= 200 {
                    self.sacar();
                }
            }
        }
    }
    fn hay_entrada(&self) -> bool {
        false
    }
    fn entrada(&mut self, _buf: &mut [u8]) -> usize {
        0
    }
}

/// El disco virtio del invitado: un objeto del montón que el servicio puede mirar después.
struct Disco(Rc<RefCell<Vec<u8>>>);

impl Bloques for Disco {
    fn sectores(&self) -> u64 {
        self.0.borrow().len() as u64 / 512
    }
    fn leer(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), ErrorBloques> {
        let d = self.0.borrow();
        let ini = (sector as usize).checked_mul(512).ok_or(ErrorBloques)?;
        buf.copy_from_slice(d.get(ini..ini.checked_add(buf.len()).ok_or(ErrorBloques)?).ok_or(ErrorBloques)?);
        Ok(())
    }
    fn escribir(&mut self, sector: u64, datos: &[u8]) -> Result<(), ErrorBloques> {
        let mut d = self.0.borrow_mut();
        let ini = (sector as usize).checked_mul(512).ok_or(ErrorBloques)?;
        d.get_mut(ini..ini.checked_add(datos.len()).ok_or(ErrorBloques)?).ok_or(ErrorBloques)?.copy_from_slice(datos);
        Ok(())
    }
}

/// Un objeto de datos (llave `llave`) proyectado en `va`, solo de lectura.
fn datos(llave: u32, va: usize) -> Option<&'static [u8]> {
    let tam = dashos::memoria_tamano(llave);
    if tam <= 0 || dashos::memoria_proyectar(llave, va, false) != 0 {
        return None;
    }
    // SAFETY: recién proyectado aquí, solo de lectura, `tam` bytes.
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, tam as usize) })
}

/// Arranca la Image de la llave `LINUX` con el initramfs de la llave `INITRD` en una máquina de 256 MB con los aparatos
/// del crate (PL011, virtio-console, virtio-blk), hasta que Linux la apaga por PSCI o pasan 4 minutos.
fn linux() -> bool {
    let Some(imagen) = datos(LINUX, VA_IMAGEN) else {
        println!("  [caja-linux] linux: sin la Image; MAL");
        return false;
    };
    let initrd = datos(INITRD, VA_INITRD);
    // La RAM: hasta 256 MB, en trozos de 16 MB; si el cupo no da para tanto, lo que quepa dejando un trozo libre para
    // las tablas de la etapa 2 y el montón (con 128 MB como poco).
    let mut mems = [0u32; TROZOS];
    let mut n = 0;
    while n < TROZOS {
        let l = dashos::memoria_crear(TROZO) as i32;
        if l <= 0 {
            break;
        }
        if dashos::memoria_proyectar(l as u32, VA_RAM_LINUX + n * TROZO, true) != 0 {
            dashos::cerrar(l as u32);
            break;
        }
        mems[n] = l as u32;
        n += 1;
    }
    if n < TROZOS && n > 0 {
        n -= 1;
        dashos::memoria_quitar(VA_RAM_LINUX + n * TROZO);
        dashos::cerrar(mems[n]);
    }
    let tam_ram = n * TROZO;
    if tam_ram < RAM_MINIMA {
        println!("  [caja-linux] linux: sin memoria para el invitado ({} MB); MAL", tam_ram >> 20);
        return false;
    }
    let mems = &mems[..n];
    let plan = match arranque::leer_cabecera(imagen)
        .and_then(|cab| arranque::planificar(&cab, imagen.len() as u64, (disposicion::RAM, tam_ram as u64), initrd.map(|d| d.len() as u64)))
    {
        Ok(p) => p,
        Err(e) => {
            println!("  [caja-linux] linux: la Image no vale o no cabe en {} MB ({:?}); MAL", tam_ram >> 20, e);
            return false;
        }
    };
    // SAFETY: los objetos, proyectados seguidos aquí; solo los toca esta `Ram` (y el invitado).
    let ram = unsafe { core::slice::from_raw_parts_mut(VA_RAM_LINUX as *mut u8, tam_ram) };
    let estado = Rc::new(RefCell::new(EstadoConsola::default()));
    let mut disco = alloc::vec![0u8; TAM_DISCO];
    disco[..MARCA_DISCO.len()].copy_from_slice(MARCA_DISCO);
    let disco = Rc::new(RefCell::new(disco));
    let mut maq = Vmm::nueva(RamCaja(Ram { base: disposicion::RAM, datos: ram }), Distribuidor::nuevo(1, disposicion::NUM_SPIS), disposicion::GICD);
    let (v0, v1) = (disposicion::virtio(0), disposicion::virtio(1));
    let puestos = maq.añadir(disposicion::UART, disposicion::SPI_UART, Box::new(Pl011::nuevo(Registro::nuevo(&estado)))).is_ok()
        && maq.añadir(v0.base, v0.spi, Box::new(TransporteMmio::nuevo(Consola::nueva(Registro::nuevo(&estado))))).is_ok()
        && maq.añadir(v1.base, v1.spi, Box::new(TransporteMmio::nuevo(Bloque::nuevo(Box::new(Disco(disco.clone())))))).is_ok();
    let mut config = disposicion::config(tam_ram as u64, 1, 2, String::from(BOOTARGS), plan.initrd);
    let mut semilla = [0u8; 32];
    if dashos::azar(&mut semilla).is_some() {
        config.semilla = Some(semilla);
    }
    let dtb = arbol::generar_dtb(&config);
    let cargada = arranque::cargar(&mut maq.mem, &plan, imagen, initrd, &dtb);
    dashos::memoria_quitar(VA_IMAGEN);
    if initrd.is_some() {
        dashos::memoria_quitar(VA_INITRD);
    }
    if !puestos || cargada.is_err() {
        println!("  [caja-linux] linux: no se pudo cargar ({:?}); MAL", cargada.err());
        return false;
    }
    let regs = arranque::registros(&plan);
    let vm = dashos::maquina_crear(PERMISO, mems[0], disposicion::RAM, regs.pc, regs.x[0]);
    let mut bien = vm > 0;
    for (i, &mm) in mems.iter().enumerate().skip(1) {
        bien &= vm > 0 && dashos::maquina_memoria(vm as u32, mm, disposicion::RAM + (i * TROZO) as u64) == 0;
    }
    bien &= vm > 0 && dashos::maquina_gicv(vm as u32, GICC) == 0;
    if !bien {
        println!("  [caja-linux] linux: no se pudo crear la máquina ({}); MAL", vm);
        return false;
    }
    let vm = vm as u32;
    println!(
        "  [caja-linux] linux: Image de {} KB en {:#x}, initramfs de {} KB, árbol de {} bytes en {:#x}, {} MB de RAM, PL011, virtio-console y virtio-blk de {} KB; arranca",
        imagen.len() / 1024, plan.kernel, initrd.map_or(0, |d| d.len() / 1024), dtb.len(), plan.dtb, tam_ram >> 20, TAM_DISCO / 1024
    );
    let t0 = dashos::ahora_us();
    let mut caja = Caja::nueva(vm, maq, &config.memoria);
    let fin = caja.correr(240_000_000, true);
    let ms = (dashos::ahora_us() - t0) / 1000;
    dashos::cerrar(vm);
    let que = match fin {
        FinCaja::Apagada => texto(format_args!("Linux apagó la máquina por PSCI")),
        FinCaja::Reinicio => texto(format_args!("Linux pidió reiniciar")),
        FinCaja::Plazo => texto(format_args!("sigue en marcha a los 240 s")),
        FinCaja::Hvc(n, x) => texto(format_args!("HVC #{} con x0 = {:#x}", n, x[0])),
        FinCaja::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x}, clase {:#x})", pc, esr, esr >> 26)),
        FinCaja::Error(e) => texto(format_args!("error {}", e)),
    };
    let c = &caja.cuentas;
    println!("  [caja-linux] linux: se para la máquina a los {} ms: {}", ms, que.as_str());
    println!(
        "  [caja-linux] linux: {} salidas: {} a aparatos ({} abortos), {} PSCI, {} WFI ({} ms dormido), {} por el reloj, {} registros; {} interrupciones inyectadas",
        c.salidas, c.aparatos, c.abortos, c.psci, c.wfi, c.dormido_us / 1000, c.interrumpido, c.registros, c.inyectadas
    );
    let e = estado.borrow();
    let d = disco.borrow();
    let escrito = &d[4096..4096 + ESCRITO.len()] == ESCRITO;
    let ok = matches!(fin, FinCaja::Apagada) && e.busybox && e.fin && escrito;
    println!(
        "  [caja-linux] linux: {} líneas por la consola, busybox en marcha: {}, el disco escrito por Linux: {}: {}",
        e.lineas, if e.busybox { "sí" } else { "no" }, if escrito { "sí" } else { "no" }, if ok { "bien" } else { "MAL" }
    );
    for &mm in mems {
        dashos::cerrar(mm);
    }
    ok
}

fn principal() -> u64 {
    // Las llaves de Linux se miran antes de nada: luego las ocupan los objetos que crea el servicio (su montón).
    let hay_linux = dashos::memoria_tamano(LINUX) > 0;
    // SAFETY: etiquetas de pruebas.s.
    let (hola, eco, lee, escribe, bucle, apaga, reloj) = unsafe {
        (
            codigo(&invitado_hola, &invitado_hola_fin),
            codigo(&invitado_eco, &invitado_eco_fin),
            codigo(&invitado_lee_fuera, &invitado_lee_fuera_fin),
            codigo(&invitado_escribe_fuera, &invitado_escribe_fuera_fin),
            codigo(&invitado_bucle, &invitado_bucle_fin),
            codigo(&invitado_apaga, &invitado_apaga_fin),
            codigo(&invitado_reloj, &invitado_reloj_fin),
        )
    };
    // Una llave que no es el permiso no crea máquinas, aunque sea de memoria.
    let mem = dashos::memoria_crear(RAM);
    let tipo = dashos::maquina_crear(mem, mem, RAM_IPA, RAM_IPA, 0);
    dashos::cerrar(mem);
    if tipo != -6 {
        println!("  [caja-linux] con una llave de memoria por permiso: {} (se esperaba E_TIPO); MAL", tipo);
    }

    let vm = match maquina(hola) {
        Ok(v) => v,
        Err(dashos::E_NO_HAY) => {
            println!("  [caja-linux] no hay hipervisor: DashOS no arrancó en EL2 (en QEMU, falta virtualization=on); sin máquinas virtuales");
            return 0;
        }
        Err(e) => {
            println!("  [caja-linux] no se pudo crear la máquina: {}", e);
            return 1;
        }
    };
    let mut bien = tipo == -6;
    match correr(vm, &prueba(1_000_000)) {
        Fin::Hvc(0, 0) => println!("  [caja-linux] el invitado acaba con HVC #0: bien"),
        _ => {
            println!("  [caja-linux] el invitado no acaba como debe; MAL");
            bien = false;
        }
    }
    dashos::cerrar(vm);

    // Lo que cuesta entrar y salir: N vueltas de HVC, cada una del servicio al invitado y de vuelta (con la llamada al
    // sistema), y de ellas lo que pasa la CPU fuera del kernel (EL2 y el invitado).
    if let Ok(vm) = maquina(eco) {
        const N: u64 = 2000;
        let mut inf = Informe::default();
        let mut dentro = 0;
        let mut vueltas = 0;
        let t0 = dashos::ahora_ns();
        while vueltas < N {
            match dashos::maquina_correr(vm, &mut inf, 0) {
                m::HVC => {
                    vueltas += 1;
                    dentro += inf.cuentas;
                }
                m::INTERRUMPIDO => {}
                _ => break,
            }
        }
        let total = dashos::ahora_ns() - t0;
        dashos::cerrar(vm);
        let dentro_ns = (dentro as u128 * 1_000_000_000 / frecuencia() as u128) as u64;
        if vueltas == N {
            println!("  [caja-linux] ida y vuelta al invitado: {} ns de media ({} vueltas; {} ns de ellos en EL2 y el invitado)", total / N, N, dentro_ns / N);
        } else {
            println!("  [caja-linux] la medida se cortó a las {} vueltas; MAL", vueltas);
            bien = false;
        }
    }

    // Los hostiles.
    bien &= hostil("leer fuera", lee, |f| matches!(f, Fin::Fuera(0x7000_0000, false)));
    bien &= hostil("escribir en el kernel", escribe, |f| matches!(f, Fin::Fuera(0x4020_0000, true)));
    bien &= hostil("bucle sin fin", bucle, |f| matches!(f, Fin::Bucle(n) if *n > 0));
    bien &= hostil("apagar por SMC", apaga, |f| matches!(f, Fin::Fallo(esr, _) if esr >> 26 == 0x17));
    // CNTP_CTL_EL0: op0 3, op1 3, CRn 14, CRm 2, op2 1, como en el ESR.
    bien &= hostil("temporizador del kernel", reloj, |f| matches!(f, Fin::Registro(0x32_f804, true)));

    // Después, otra máquina sigue funcionando.
    let sigue = match maquina(hola) {
        Ok(vm) => {
            let r = matches!(correr(vm, &prueba(1_000_000)), Fin::Hvc(0, 0));
            dashos::cerrar(vm);
            r
        }
        Err(_) => false,
    };
    bien &= sigue;
    println!("  [caja-linux] tras los hostiles, una máquina nueva sigue en marcha: {}", if sigue { "bien" } else { "MAL" });
    println!("  [caja-linux] primer paso de la caja Linux: {}", if bien { "bien" } else { "MAL" });

    // Tercer paso: las interrupciones.
    // SAFETY: etiquetas de interrupciones.s.
    let (irq, gic_hostil, tormenta, el2) = unsafe {
        (
            codigo(&invitado_irq, &invitado_irq_fin),
            codigo(&invitado_gic_hostil, &invitado_gic_hostil_fin),
            codigo(&invitado_tormenta, &invitado_tormenta_fin),
            codigo(&invitado_el2, &invitado_el2_fin),
        )
    };
    let tercero = prueba_interrupciones(irq) & prueba_gic_hostil(gic_hostil, tormenta, el2);
    println!("  [caja-linux] interrupciones del invitado: {}", if tercero { "bien" } else { "MAL" });
    bien &= tercero;
    // Con la Image de un kernel de Linux (llave 2), Linux hasta busybox.
    if hay_linux {
        bien &= linux();
    }
    if bien { 0 } else { 1 }
}
