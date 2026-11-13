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
//! Cuarto paso: Arch Linux ARM (`make arranque-virt-arch`): sin initramfs, el disco del invitado es la partición de la
//! caja en el NVMe, por el servicio de disco ("disco"), y Linux arranca su systemd desde ella.
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
use caja_vmm::pl031::{Pl031, Reloj};
use caja_vmm::psci::{self, AccionPsci, Psci};
use caja_vmm::virtio::bloque::{Bloque, Bloques, ErrorBloques};
use caja_vmm::virtio::consola::{Consola, ExtremoConsola};
use caja_vmm::virtio::mmio::TransporteMmio;
use caja_vmm::virtio::vsock::Vsock;
use core::cell::{RefCell, UnsafeCell};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use dashos::maquina::{self as m, Informe};
use dashos::println;
use dashos::protocolo::red as rp;

#[path = "../ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../portapapeles_cliente.rs"]
mod portapapeles_cliente;
#[path = "../terminal.rs"]
#[allow(dead_code)]
mod terminal;
#[path = "../pantalla_caja.rs"]
mod pantalla_caja;
#[path = "../puente_caja.rs"]
mod puente_caja;

use kit::lienzo::{Lienzo, Rect};
use kit::teclado::{Tecla, Teclado};
use kit::texto::Fuente;
use terminal::Terminal;
use ventana_cliente::*;
use wayland::Escritor;
use wayland::tablas::{wl_callback, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};

dashos::programa!(principal);
dashos::monton!();

core::arch::global_asm!(include_str!("../../invitados/pruebas.s"));
core::arch::global_asm!(include_str!("../../invitados/interrupciones.s"));
core::arch::global_asm!(include_str!("../../invitados/smp.s"));

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
    static invitado_smp: u8;
    static invitado_smp_fin: u8;
}

/// El permiso de máquinas virtuales: lo da `sesiones` (en `virt`) o `inicio` (en la Pi 4, que aún no tiene `sesiones`).
static PERMISO: AtomicU32 = AtomicU32::new(0);

fn permiso() -> u32 {
    PERMISO.load(Ordering::Relaxed)
}
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
    maquina_x0(codigo, 0)
}

/// Como `maquina`, con `x0` en el x0 de su vCPU 0.
fn maquina_x0(codigo: &[u8], x0: u64) -> Result<u32, i64> {
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
    let r = dashos::maquina_crear(permiso(), mem, RAM_IPA, RAM_IPA, x0);
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

/// Una RAM vacía, para los invitados de prueba que el servicio no lee.
fn ram_vacia() -> RamCaja {
    RamCaja(Ram { base: RAM_IPA, datos: &mut [] })
}

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

/// Cuántas veces salió el invitado, y por qué (sumando todas sus vCPU).
#[derive(Default, Clone, Copy)]
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
    /// Patadas a otras vCPU (algo nuevo para ellas en el distribuidor).
    patadas: u64,
}

/// Cómo acabó una máquina corrida con sus aparatos.
#[derive(Clone, Copy)]
enum FinCaja {
    /// HVC #n (no de PSCI) con x0..x3.
    Hvc(u64, [u64; 4]),
    /// PSCI SYSTEM_OFF, o CPU_OFF de la última vCPU encendida.
    Apagada,
    Reinicio,
    Plazo,
    Fallo(u64, u64),
    Error(i64),
}

/// Cuántas vCPU puede tener una máquina de la caja.
const MAX_VCPUS: usize = m::MAX_VCPUS;

/// Un dato que comparten los hilos de la caja, con su cerrojo.
struct Compartida<T> {
    cerrojo: dashos::hilos::Cerrojo,
    dato: UnsafeCell<T>,
}

// SAFETY: `dato` solo se toca con el cerrojo tomado (`con`), y `con` no se anida.
unsafe impl<T> Sync for Compartida<T> {}

impl<T> Compartida<T> {
    const fn nueva(t: T) -> Self {
        Self { cerrojo: dashos::hilos::Cerrojo::nuevo(), dato: UnsafeCell::new(t) }
    }

    fn con<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.cerrojo.tomar();
        // SAFETY: con el cerrojo tomado nadie más lo toca.
        let r = f(unsafe { &mut *self.dato.get() });
        self.cerrojo.soltar();
        r
    }
}

/// Una máquina con sus aparatos (`caja_vmm::maquina::Maquina`), su PSCI y sus vCPU. La comparten los hilos de sus vCPU
/// (cada una corre en el suyo): todo lo que tocan los aparatos y el distribuidor va con el cerrojo de `Sistema::caja`.
struct Caja<M: MemoriaInvitado> {
    maq: Vmm<M>,
    psci: Psci,
    /// Un HVC #0 es una llamada de PSCI (DEN 0022); si no, acaba la máquina como cualquier otro HVC.
    con_psci: bool,
    /// Las llaves de sus vCPU (`n`).
    llaves: [u32; MAX_VCPUS],
    n: usize,
    /// Se le dio una patada por algo nuevo en el distribuidor y aún no ha llenado sus registros de lista: no hace falta
    /// otra.
    pateada: [bool; MAX_VCPUS],
    /// Lo que pidió PSCI CPU_ON para cada vCPU apagada: (entrada, contexto).
    arranques: [Option<(u64, u64)>; MAX_VCPUS],
    cuentas: Cuentas,
    fin: Option<FinCaja>,
}

impl<M: MemoriaInvitado> Caja<M> {
    /// `llaves`: la vCPU 0 (la que da `maquina_crear`) y las demás (`maquina_vcpu`).
    fn nueva(llaves: &[u32], maq: Vmm<M>, ram: &[(u64, u64)], con_psci: bool) -> Self {
        let n = llaves.len().clamp(1, MAX_VCPUS);
        let mut ll = [0; MAX_VCPUS];
        ll[..n].copy_from_slice(&llaves[..n]);
        Self {
            maq,
            psci: Psci::nuevo(n, ram),
            con_psci,
            llaves: ll,
            n,
            pateada: [false; MAX_VCPUS],
            arranques: [None; MAX_VCPUS],
            cuentas: Cuentas::default(),
            fin: None,
        }
    }

    /// Los registros de lista que el invitado ya terminó, al distribuidor; los libres, con lo que tenga pendiente para
    /// esta vCPU; y cómo tiene el temporizador virtual, para el kernel (y que sus WFI esperen allí).
    fn llenar_lrs(&mut self, v: &mut VcpuCaja) {
        let n = (v.irq.lrs as usize).min(m::LRS);
        for i in 0..n {
            if let Some(id) = v.cargadas[i]
                && gic::lr_libre(v.irq.lr[i] as u32)
            {
                self.maq.gic.terminada(v.num, id);
                v.cargadas[i] = None;
                v.irq.lr[i] = 0;
            }
        }
        let mut pendientes = self.maq.gic.pendientes(v.num).into_iter();
        for i in 0..n {
            if v.cargadas[i].is_none() {
                let Some(e) = pendientes.next() else { break };
                // Con EOI: cuando el invitado la termina, el GIC da la interrupción de mantenimiento, que lo saca, y el
                // servicio vuelve a mirar la línea (las de los aparatos son por nivel).
                v.irq.lr[i] = gic::lr_v2(&e, true) as u64;
                self.maq.gic.cargada(v.num, &e);
                v.cargadas[i] = Some(e.intid);
                self.cuentas.inyectadas += 1;
            }
        }
        self.pateada[v.num] = false;
        v.irq.temporizador = m::ESPERAR_WFI
            | match self.maq.gic.privada(v.num, INTID_TEMPORIZADOR) {
                Some((true, p, g1)) => 1 | (p as u64) << 8 | (g1 as u64) << 16,
                _ => 0,
            };
    }

    /// Las interrupciones que estaban en los registros de lista de una vCPU que se apaga vuelven al distribuidor
    /// (terminadas: una por nivel volverá a estar pendiente si su línea sigue arriba).
    fn soltar_lrs(&mut self, v: &mut VcpuCaja) {
        for i in 0..m::LRS {
            if let Some(id) = v.cargadas[i].take() {
                self.maq.gic.terminada(v.num, id);
            }
            v.irq.lr[i] = 0;
        }
    }

    /// Las vCPU (menos `yo`) que tienen algo nuevo en el distribuidor y aún no se han enterado: hay que patearlas (una
    /// máscara). Se apuntan como pateadas.
    fn por_patear(&mut self, yo: usize, encendidas: &[AtomicU32; MAX_VCPUS]) -> u32 {
        let mut mascara = 0u32;
        for j in 0..self.n {
            if j != yo && !self.pateada[j] && encendidas[j].load(Ordering::Acquire) != 0 && self.maq.gic.hay_para(j) {
                self.pateada[j] = true;
                mascara |= 1 << j;
            }
        }
        self.cuentas.patadas += mascara.count_ones() as u64;
        mascara
    }

    /// Si llegó algo de fuera (teclas de la terminal, tramas de la red), los aparatos lo pasan al invitado y suben su
    /// línea de interrupción.
    fn mirar_entradas(&mut self) -> bool {
        if COMPARTIDO.aviso.swap(false, Ordering::AcqRel) {
            self.maq.sondear();
            return true;
        }
        false
    }
}

/// Lo de cada vCPU que solo toca su hilo.
struct VcpuCaja {
    num: usize,
    irq: m::Interrupciones,
    /// La interrupción que hay en cada registro de lista del servicio.
    cargadas: [Option<u32>; m::LRS],
}

/// La caja en marcha: la máquina con su cerrojo y lo que los hilos de sus vCPU miran sin él.
struct Sistema {
    caja: Compartida<Option<Caja<RamCaja>>>,
    /// Por vCPU: 0 apagada (su hilo espera aquí), 1 encendida o a punto.
    encendidas: [AtomicU32; MAX_VCPUS],
    /// Se acabó la máquina: los hilos de las vCPU vuelven.
    acabada: AtomicBool,
}

/// La caja de las pruebas y de Linux (una cada vez).
static CAJA: Sistema = Sistema {
    caja: Compartida::nueva(None),
    encendidas: [const { AtomicU32::new(0) }; MAX_VCPUS],
    acabada: AtomicBool::new(false),
};

/// Lo que hace el hilo de una vCPU después de una salida, decidido con el cerrojo tomado.
enum Paso {
    Seguir(u64),
    Abortar,
    PonerX0(u64),
    Apagarse,
    Acabar(FinCaja),
}

impl Sistema {
    fn con<R>(&self, f: impl FnOnce(&mut Caja<RamCaja>) -> R) -> R {
        self.caja.con(|c| f(c.as_mut().expect("la caja está puesta")))
    }

    /// Pone `caja` para correrla: la vCPU 0 encendida, las demás apagadas.
    fn poner(&self, caja: Caja<RamCaja>) {
        self.acabada.store(false, Ordering::Release);
        for (i, e) in self.encendidas.iter().enumerate() {
            e.store((i == 0) as u32, Ordering::Release);
        }
        self.caja.con(|c| *c = Some(caja));
    }

    /// Quita la caja (ya acabada, con sus hilos unidos).
    fn quitar(&self) -> Option<Caja<RamCaja>> {
        self.caja.con(|c| c.take())
    }

    /// Acaba la máquina (la primera razón manda): para todas sus vCPU y despierta a las apagadas.
    fn acabar(&self, llave: u32, fin: FinCaja) {
        self.con(|c| {
            if c.fin.is_none() {
                c.fin = Some(fin);
            }
        });
        self.acabada.store(true, Ordering::Release);
        dashos::maquina_parar(llave);
        for e in &self.encendidas {
            dashos::hilos::despertar(e, 8);
        }
    }

    /// El hilo de la vCPU `num`: la corre hasta que acaba la máquina o pasa el plazo (desde `t0`). Apagada (las
    /// secundarias al empezar, o tras CPU_OFF), espera a que otra la encienda con CPU_ON.
    fn correr_vcpu(&self, num: usize, t0: u64, plazo_us: u64) {
        let llave = self.con(|c| c.llaves[num]);
        let llaves = self.con(|c| c.llaves);
        let mut v = VcpuCaja { num, irq: m::Interrupciones::default(), cargadas: [None; m::LRS] };
        let mut inf = Informe::default();
        let mut valor = 0;
        loop {
            if self.acabada.load(Ordering::Acquire) {
                return;
            }
            if dashos::ahora_us() - t0 > plazo_us {
                self.acabar(llave, FinCaja::Plazo);
                return;
            }
            if self.encendidas[num].load(Ordering::Acquire) == 0 {
                dashos::hilos::esperar(&self.encendidas[num], 0, 100_000);
                continue;
            }
            // Recién encendida por CPU_ON: como nueva, en su entrada y con su contexto.
            if let Some((entrada, contexto)) = self.con(|c| c.arranques[num].take()) {
                v.irq = m::Interrupciones::default();
                v.cargadas = [None; m::LRS];
                valor = 0;
                if dashos::maquina_reiniciar(llave, entrada, contexto) != 0 {
                    self.acabar(llave, FinCaja::Error(-1));
                    return;
                }
                self.con(|c| c.psci.encendida(num));
            }
            self.con(|c| c.llenar_lrs(&mut v));
            let r = dashos::maquina_correr_irq(llave, &mut inf, valor, &mut v.irq);
            valor = 0;
            if r == dashos::E_CERRADO || self.acabada.load(Ordering::Acquire) {
                // Parada desde otro hilo (o muerta en un fallo de otra vCPU).
                if !self.acabada.load(Ordering::Acquire) {
                    self.acabar(llave, FinCaja::Error(r));
                }
                return;
            }
            let encendidas = &self.encendidas;
            let (paso, patear) = self.con(|c| {
                c.cuentas.salidas += 1;
                c.cuentas.dormido_us += inf.dormido_us;
                let mut cambio = false;
                let paso = match r {
                    m::LECTURA | m::ESCRITURA => {
                        c.cuentas.aparatos += 1;
                        let op = if r == m::LECTURA { Operacion::Leer } else { Operacion::Escribir(inf.valor) };
                        cambio = r == m::ESCRITURA;
                        match c.maq.mmio(num, inf.direccion, inf.tamano as u8, op) {
                            SalidaMmio::Leido(v) => Paso::Seguir(v),
                            SalidaMmio::Escrito => Paso::Seguir(0),
                            // Nada en esa dirección: un aborto externo, como el de un bus de verdad.
                            SalidaMmio::SinAparato => {
                                c.cuentas.abortos += 1;
                                Paso::Abortar
                            }
                        }
                    }
                    m::HVC if c.con_psci && inf.tamano == 0 => {
                        c.cuentas.psci += 1;
                        match c.psci.atender(num, [inf.valor, inf.x1, inf.x2, inf.x3]) {
                            AccionPsci::Responder(v) => Paso::PonerX0(v),
                            AccionPsci::Suspender => Paso::PonerX0(0),
                            AccionPsci::Encender { cpu, entrada, contexto } if cpu < c.n => {
                                c.arranques[cpu] = Some((entrada, contexto));
                                encendidas[cpu].store(1, Ordering::Release);
                                dashos::hilos::despertar(&encendidas[cpu], 1);
                                Paso::PonerX0(0)
                            }
                            AccionPsci::Encender { .. } => Paso::PonerX0(psci::PARAMETROS_MALOS as i64 as u64),
                            AccionPsci::ApagarEsta => {
                                c.soltar_lrs(&mut v);
                                encendidas[num].store(0, Ordering::Release);
                                if (0..c.n).all(|j| c.psci.estado(j) == Some(psci::EstadoCpu::Apagada)) {
                                    Paso::Acabar(FinCaja::Apagada)
                                } else {
                                    Paso::Apagarse
                                }
                            }
                            AccionPsci::ApagarMaquina => Paso::Acabar(FinCaja::Apagada),
                            AccionPsci::ReiniciarMaquina => Paso::Acabar(FinCaja::Reinicio),
                        }
                    }
                    m::HVC => Paso::Acabar(FinCaja::Hvc(inf.tamano, [inf.valor, inf.x1, inf.x2, inf.x3])),
                    m::REGISTRO => {
                        // Depuración y contadores de rendimiento: de solo cero.
                        c.cuentas.registros += 1;
                        Paso::Seguir(0)
                    }
                    m::WFI | m::INTERRUMPIDO => {
                        if r == m::WFI {
                            c.cuentas.wfi += 1;
                        } else {
                            c.cuentas.interrumpido += 1;
                        }
                        cambio = c.mirar_entradas();
                        Paso::Seguir(0)
                    }
                    m::FALLO => Paso::Acabar(FinCaja::Fallo(inf.esr, inf.pc)),
                    e => Paso::Acabar(FinCaja::Error(e)),
                };
                let patear = if cambio { c.por_patear(num, encendidas) } else { 0 };
                (paso, patear)
            });
            for (j, &l) in llaves.iter().enumerate() {
                if patear & (1 << j) != 0 {
                    dashos::maquina_patear(l);
                }
            }
            match paso {
                Paso::Seguir(x) => valor = x,
                Paso::Abortar => {
                    dashos::maquina_aborto(llave);
                }
                Paso::PonerX0(x) => {
                    let mut r = [0u64; m::REGISTROS];
                    if dashos::maquina_registros(llave, &mut r) == 0 {
                        r[0] = x;
                        dashos::maquina_poner_registros(llave, &r);
                    }
                }
                Paso::Apagarse => {}
                Paso::Acabar(fin) => {
                    self.acabar(llave, fin);
                    return;
                }
            }
        }
    }

    /// Corre la máquina que hay puesta: la vCPU 0 en este hilo y cada una de las demás en uno suyo (con su pila del
    /// montón). Vuelve cuando acaba la máquina o pasa `plazo_us`, con todos sus hilos ya unidos: ninguno se queda
    /// colgando. Devuelve cómo acabó y cuántos hilos se unieron bien.
    fn correr(&'static self, plazo_us: u64) -> (FinCaja, usize) {
        let n = self.con(|c| c.n);
        let t0 = dashos::ahora_us();
        PLAZO.store(plazo_us, Ordering::Relaxed);
        T0.store(t0, Ordering::Relaxed);
        let mut pilas: Vec<Vec<u8>> = Vec::new();
        let mut hilos = [0u64; MAX_VCPUS];
        for (j, h) in hilos.iter_mut().enumerate().take(n).skip(1) {
            pilas.push(alloc::vec![0u8; PILA_VCPU]);
            let p = pilas.last_mut().map(|p| p.as_mut_slice()).unwrap_or(&mut []);
            let r = dashos::hilos::crear(hilo_vcpu, j as u64, p, 0);
            if r < 0 {
                println!("  [caja-linux] no se pudo crear el hilo de la vCPU {} ({})", j, r);
            } else {
                *h = r as u64;
            }
        }
        self.correr_vcpu(0, t0, plazo_us);
        // La vCPU 0 acabó: la máquina también (si no, la para), y los demás hilos salen.
        let llave = self.con(|c| c.llaves[0]);
        if !self.acabada.load(Ordering::Acquire) {
            self.acabar(llave, FinCaja::Error(0));
        }
        let mut unidos = 1;
        for &h in &hilos[1..n] {
            if h != 0 && dashos::hilos::unir(h).0 == 0 {
                unidos += 1;
            }
        }
        drop(pilas);
        (self.con(|c| c.fin.unwrap_or(FinCaja::Error(0))), unidos)
    }
}

/// La pila del hilo de cada vCPU secundaria.
const PILA_VCPU: usize = 128 * 1024;
static PLAZO: AtomicU64 = AtomicU64::new(0);
static T0: AtomicU64 = AtomicU64::new(0);

fn hilo_vcpu(num: u64) -> u64 {
    CAJA.correr_vcpu(num as usize, T0.load(Ordering::Relaxed), PLAZO.load(Ordering::Relaxed));
    0
}

/// El temporizador virtual del invitado: PPI 11, INTID 27 (el kernel lo pone en su registro de lista).
const INTID_TEMPORIZADOR: u32 = 27;

/// Una máquina de prueba con su GICV. `None` (y lo dice) si no se puede.
fn maquina_gic(nombre: &str, codigo: &[u8]) -> Option<u32> {
    maquina_gic_x0(nombre, codigo, 0)
}

/// Como `maquina_gic`, con `x0` en el x0 de su vCPU 0.
fn maquina_gic_x0(nombre: &str, codigo: &[u8], x0: u64) -> Option<u32> {
    let vm = match maquina_x0(codigo, x0) {
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
        let maq = Vmm::nueva(ram_vacia(), Distribuidor::nuevo(1, disposicion::NUM_SPIS), disposicion::GICD);
        CAJA.poner(Caja::nueva(&[vm], maq, &[(RAM_IPA, RAM as u64)], false));
        let (fin, _) = CAJA.correr(5_000_000);
        let cuentas = CAJA.quitar().map(|c| c.cuentas).unwrap_or_default();
        dashos::cerrar(vm);
        let ok = matches!(fin, FinCaja::Hvc(0, [2, ..])) && cuentas.abortos == 2;
        let que = match fin {
            FinCaja::Hvc(0, x) => texto(format_args!("acaba con {} abortos externos", x[0])),
            FinCaja::Hvc(n, _) => texto(format_args!("HVC #{}: una excepción inesperada", n)),
            FinCaja::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x})", pc, esr)),
            FinCaja::Plazo => texto(format_args!("no acaba")),
            _ => texto(format_args!("acaba de otra forma")),
        };
        println!(
            "  [caja-linux] hostil (GIC al azar): 4096 escrituras en su GICV y 4096 en su distribuidor, {} de ellas al servicio; toca la GICH del anfitrión: {}; {}",
            cuentas.aparatos, que.as_str(), if ok { "bien" } else { "MAL" }
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

/// Varias vCPU (`invitados/smp.s`, dos vCPU, cada una en su hilo): PSCI (CPU_ON a una que no existe y dos veces a la
/// misma, AFFINITY_INFO), lo que tarda una SGI de una vCPU a otra y vuelta, y dos hostiles: una tormenta de SGI sin fin
/// (modo 0) y una vCPU en un bucle sin fin con todo enmascarado (modo 1), las dos mientras la otra trabaja. Las dos
/// máquinas acaban con una vCPU aún dentro de su invitado: se paran y sus hilos se unen.
fn prueba_smp(codigo: &[u8], modo: u64) -> bool {
    let nombre = if modo == 0 { "varias vCPU" } else { "hostil (una vCPU en bucle)" };
    let Some(vm) = maquina_gic_x0(nombre, codigo, modo) else { return false };
    let otra = dashos::maquina_vcpu(vm, 1, 0, 0);
    // Una vCPU con un número que ya está, o fuera de rango: no.
    let repetida = dashos::maquina_vcpu(vm, 1, 0, 0);
    let fuera = dashos::maquina_vcpu(vm, m::MAX_VCPUS as u64, 0, 0);
    if otra <= 0 || repetida != dashos::E_ARGUMENTO || fuera != dashos::E_ARGUMENTO {
        println!("  [caja-linux] {}: la vCPU 1 da {}, otra vez {}, la {} da {}; MAL", nombre, otra, repetida, m::MAX_VCPUS, fuera);
        dashos::cerrar(vm);
        if otra > 0 {
            dashos::cerrar(otra as u32);
        }
        return false;
    }
    let otra = otra as u32;
    let maq = Vmm::nueva(ram_vacia(), Distribuidor::nuevo(2, disposicion::NUM_SPIS), disposicion::GICD);
    CAJA.poner(Caja::nueva(&[vm, otra], maq, &[(RAM_IPA, RAM as u64)], true));
    let (fin, unidos) = CAJA.correr(20_000_000);
    let cuentas = CAJA.quitar().map(|c| c.cuentas).unwrap_or_default();
    let mut r = [0u64; m::REGISTROS];
    dashos::maquina_registros(vm, &mut r);
    // Parada: ya no entra ninguna.
    let parada = dashos::maquina_correr(otra, &mut Informe::default(), 0) == dashos::E_CERRADO;
    dashos::cerrar(otra);
    dashos::cerrar(vm);
    let us = |c: u64| (c as u128 * 1_000_000 / frecuencia() as u128) as u64;
    let (sola, con_otra) = (us(r[4]), us(r[5]));
    let bien = match (modo, fin) {
        (0, FinCaja::Hvc(1, [suma, 100, 0, tormenta])) if tormenta > 0 && unidos == 2 && parada => {
            println!(
                "  [caja-linux] varias vCPU: PSCI (CPU_ON a una que no existe y dos veces a la misma, rechazados; AFFINITY_INFO), 100 idas y vueltas de SGI entre dos vCPU: {} µs de media",
                us(suma) / 100
            );
            println!(
                "  [caja-linux] hostil (tormenta de SGI): {} recibidas mientras trabaja (el trabajo, {} µs sola y {} µs con la tormenta); {} patadas; se para con la tormenta dentro, {} hilos unidos: bien",
                tormenta, sola, con_otra, cuentas.patadas, unidos
            );
            true
        }
        (1, FinCaja::Apagada) if unidos == 2 && parada => {
            println!(
                "  [caja-linux] hostil (una vCPU en bucle): la otra trabaja ({} µs sola, {} µs con ella en bucle) y apaga la máquina con ella dentro; {} hilos unidos: bien",
                sola, con_otra, unidos
            );
            true
        }
        _ => {
            let que = match fin {
                FinCaja::Hvc(n, x) => texto(format_args!("HVC #{} con {:?}", n, x)),
                FinCaja::Apagada => texto(format_args!("apagada")),
                FinCaja::Plazo => texto(format_args!("no acaba")),
                FinCaja::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x})", pc, esr)),
                FinCaja::Reinicio => texto(format_args!("reinicio")),
                FinCaja::Error(e) => texto(format_args!("error {}", e)),
            };
            println!("  [caja-linux] {}: {}, {} hilos unidos, parada: {}; MAL", nombre, que.as_str(), unidos, parada);
            false
        }
    };
    bien
}

// --- Linux ---

/// Lo que comparten el hilo de la vCPU y el de la ventana de la terminal (y, con la red, el de las tramas).
struct Compartido {
    /// Lo que escribe Linux por hvc0, para la ventana.
    salida: Tubo,
    /// Lo que se teclea en la ventana, para hvc0.
    entrada: Tubo,
    /// Hay algo nuevo para el invitado: los aparatos tienen que mirar (`Maquina::sondear`).
    aviso: AtomicBool,
    /// La llave de la vCPU 0 (0 si no hay máquina): quien deja algo para el invitado la patea, y ella lo mira.
    vcpu0: AtomicU32,
    /// La ventana: 0 aún no, 1 en pantalla, 2 no pudo.
    ventana: AtomicU32,
    /// Se apagó la máquina: la ventana dice que ha acabado.
    acabada: AtomicBool,
}

static COMPARTIDO: Compartido = Compartido {
    salida: Tubo::nuevo(256 * 1024),
    entrada: Tubo::nuevo(4096),
    aviso: AtomicBool::new(false),
    vcpu0: AtomicU32::new(0),
    ventana: AtomicU32::new(0),
    acabada: AtomicBool::new(false),
};

impl Compartido {
    /// Algo nuevo para el invitado: que lo miren los aparatos, y la vCPU 0 que salga a mirarlo (duerma en un WFI o
    /// esté dentro del invitado). Ella pasa la interrupción a la vCPU que toque.
    fn avisar(&self) {
        self.aviso.store(true, Ordering::Release);
        let v = self.vcpu0.load(Ordering::Acquire);
        if v != 0 {
            dashos::maquina_patear(v);
        }
    }
}

/// Bytes de un hilo a otro, con tope: lo que no cabe se pierde (y se cuenta). Así un invitado que inunda su consola no
/// hace crecer el montón del servicio.
struct Tubo {
    cerrojo: dashos::hilos::Cerrojo,
    datos: UnsafeCell<alloc::collections::VecDeque<u8>>,
    tope: usize,
    perdidos: AtomicU64,
}

// SAFETY: `datos` solo se toca con el cerrojo tomado.
unsafe impl Sync for Tubo {}

impl Tubo {
    const fn nuevo(tope: usize) -> Self {
        Self { cerrojo: dashos::hilos::Cerrojo::nuevo(), datos: UnsafeCell::new(alloc::collections::VecDeque::new()), tope, perdidos: AtomicU64::new(0) }
    }

    fn con<R>(&self, f: impl FnOnce(&mut alloc::collections::VecDeque<u8>) -> R) -> R {
        self.cerrojo.tomar();
        // SAFETY: con el cerrojo tomado, nadie más lo toca.
        let r = f(unsafe { &mut *self.datos.get() });
        self.cerrojo.soltar();
        r
    }

    fn meter(&self, d: &[u8]) {
        let perdidos = self.con(|q| {
            let n = d.len().min(self.tope.saturating_sub(q.len()));
            q.extend(&d[..n]);
            d.len() - n
        });
        if perdidos > 0 {
            self.perdidos.fetch_add(perdidos as u64, Ordering::Relaxed);
        }
    }

    fn sacar(&self, buf: &mut [u8]) -> usize {
        self.con(|q| {
            let n = buf.len().min(q.len());
            for (b, x) in buf.iter_mut().zip(q.drain(..n)) {
                *b = x;
            }
            n
        })
    }

    fn hay(&self) -> bool {
        self.con(|q| !q.is_empty())
    }
}

/// Llave 1: "sistema" (la RAM y si hay hipervisor). Después, en `virt`: la Image de un kernel de Linux para arm64 y su
/// initramfs (busybox), si los hay (`make arranque-virt-linux`), solo de lectura; y los canales "ventanas" (la
/// terminal), "disco" (el disco de Arch), "sesiones" (el permiso) y "red", en ese orden (el de `inicio`). En la Pi 4: el
/// permiso, que da `inicio`.
const SISTEMA: u32 = 1;
const VA_SISTEMA: usize = 0x1800_0000;
const VA_IMAGEN: usize = 0x2000_0000;
const VA_INITRD: usize = 0x3800_0000;
/// La RAM del invitado de Linux, en objetos de 16 MB (el tope de `memoria_crear`), proyectados seguidos aquí.
const VA_RAM_LINUX: usize = 0x40_0000_0000;
const TROZO: usize = 16 << 20;
/// Lo más que se le da: 2 GB (caja-linux-grandes: lo de serie llega a 512 MB; más, con `DASHOS_CAJA_LINUX_MB`). Las
/// direcciones del invitado acaban en 4 GB (`hiper::IPA_MAX`) y su RAM empieza en 1 GB.
const MAX_TROZOS: usize = 128;
const RAM_MINIMA: usize = 64 << 20;
/// La consola temprana por el PL011; la de verdad, virtio-console (hvc0). `caja.escribir` pide al /init de prueba que
/// escriba en el disco; `caja.terminal`, que deje una shell en hvc0 (hay ventana de terminal).
const BOOTARGS: &str = "earlycon=pl011,0x09000000 console=hvc0 panic=-1 caja.escribir caja.paralelo";
/// Con Arch Linux ARM: la raíz es el disco virtio (la partición de la caja) y el init, el de Arch (systemd).
const BOOTARGS_ARCH: &str = "earlycon=pl011,0x09000000 console=hvc0 root=/dev/vda rw rootwait panic=-1";
/// Las vCPU del invitado de Linux.
const VCPUS_LINUX: usize = 4;
/// El disco: 1 MB en memoria, con esto al principio. El /init escribe `ESCRITO` en el sector 8.
const MARCA_DISCO: &[u8] = b"DISCO-CAJA-LINUX";
const ESCRITO: &[u8] = b"ESCRITO-POR-LINUX";
const TAM_DISCO: usize = 1 << 20;
/// Lo que se saca de la consola de Linux por el registro de DashOS: como mucho `RAFAGA` líneas seguidas, a
/// `POR_SEGUNDO` por segundo. Si se pasa, calla hasta que Linux pase `CALMA_US` sin escribir líneas, y entonces dice
/// cuántas se calló. La ventana lo enseña todo.
const RAFAGA: u64 = 400;
const POR_SEGUNDO: u64 = 100;
const CALMA_US: u64 = 500_000;

/// Las llaves que tiene la caja, según dónde corre.
#[derive(Default)]
struct Llaves {
    linux: Option<u32>,
    initrd: Option<u32>,
    ventanas: Option<u32>,
    sesiones: Option<u32>,
    red: Option<u32>,
    /// El servicio de disco (la partición de la caja Linux: el disco de Arch).
    disco: Option<u32>,
    /// El permiso, en la Pi 4 (lo da `inicio`).
    permiso: Option<u32>,
}

/// Si existe la llave `k` (una copia sin derechos, que se cierra en el acto).
fn existe(k: u32) -> bool {
    let c = dashos::llave_duplicar(k, 0, 0);
    if c > 0 {
        dashos::cerrar(c as u32);
    }
    c > 0
}

/// Mira qué llaves tiene: tras "sistema", las de datos (Linux y su initramfs); después, o el permiso (en la Pi, y nada
/// más) o los cuatro canales (en `virt`).
fn llaves() -> Llaves {
    let mut l = Llaves::default();
    let mut k = 2;
    if dashos::memoria_tamano(k) > 0 {
        l.linux = Some(k);
        k += 1;
        if dashos::memoria_tamano(k) > 0 {
            l.initrd = Some(k);
            k += 1;
        }
    }
    // Los canales llegan en el orden en que `inicio` los crea (el de su primer nombre en su lista, no el de la lista de
    // la caja): "ventanas" (el compositor), "disco" (el servicio de disco), "sesiones" y "red".
    if existe(k + 3) {
        l.ventanas = Some(k);
        l.disco = Some(k + 1);
        l.sesiones = Some(k + 2);
        l.red = Some(k + 3);
    } else if existe(k + 1) {
        l.ventanas = Some(k);
        l.sesiones = Some(k + 1);
        l.red = existe(k + 2).then_some(k + 2);
    } else if existe(k) {
        l.permiso = Some(k);
    }
    l
}

/// Pide a `sesiones` el permiso de máquinas virtuales. Al arrancar, `sesiones` puede tardar (espera al servicio de
/// archivos hasta 15 s): se reintenta hasta 60 s. La llave, o el estado.
fn pedir_permiso(sesiones: u32) -> Result<u32, i64> {
    let nombre = b"maquinas";
    let mut m = [0u8; 20];
    m[0..4].copy_from_slice(&dashos::protocolo::sesiones::SERVICIO.to_le_bytes());
    m[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    m[12..20].copy_from_slice(nombre);
    let hasta = dashos::ahora_us() + 60_000_000;
    let mut ultimo = -1;
    while dashos::ahora_us() < hasta {
        let (lectura, suyo) = dashos::canal_crear();
        if dashos::canal_escribir(sesiones, &m, &[suyo]) != 0 {
            dashos::cerrar(lectura);
            dashos::cerrar(suyo);
            dashos::dormir_us(200_000);
            continue;
        }
        let mut r = [0u8; 4];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 5_000_000);
        dashos::cerrar(lectura);
        if n == 4 && k == 1 && i32::from_le_bytes(r) == 0 {
            return Ok(ll[0]);
        }
        if k == 1 {
            dashos::cerrar(ll[0]);
        }
        ultimo = if n == 4 { i32::from_le_bytes(r) as i64 } else { n };
        // Un "no" de verdad no cambia por insistir.
        if ultimo == -2 {
            break;
        }
    }
    Err(ultimo)
}

#[derive(Default)]
struct EstadoConsola {
    lineas: u64,
    busybox: bool,
    fin: bool,
    /// La shell de la terminal está lista (`caja-linux: terminal lista`).
    terminal: bool,
    /// Arch Linux ARM: systemd da la bienvenida, getty entra como root y sale el prompt de su shell.
    arch: bool,
    login: bool,
    prompt: bool,
    /// Para no inundar el registro: lo que queda de la ráfaga, cuándo se repuso y cuántas líneas no salieron.
    cupo: u64,
    repuesto_us: u64,
    sin_sacar: u64,
    callado: bool,
    ultima_us: u64,
}

/// Un extremo de consola (PL011 o virtio-console) hacia el registro de DashOS: cada línea, con "linux: " delante y sin
/// las secuencias de escape. hvc0 va además a la ventana de la terminal, si la hay.
struct Registro {
    linea: Vec<u8>,
    estado: Rc<RefCell<EstadoConsola>>,
    terminal: bool,
    /// Dentro de una secuencia de escape (para el registro): 1 tras ESC, 2 dentro de una CSI, 3 dentro de una cadena de
    /// control (OSC, DCS, SOS, PM o APC de ECMA-48, hasta BEL o ESC \: systemd manda OSC 3008 y una DCS al arrancar) y
    /// 4 tras un ESC dentro de ella.
    escape: u8,
}

impl Registro {
    fn nuevo(estado: &Rc<RefCell<EstadoConsola>>, terminal: bool) -> Box<Self> {
        Box::new(Self { linea: Vec::new(), estado: estado.clone(), terminal, escape: 0 })
    }

    fn sacar(&mut self) {
        let t = String::from_utf8_lossy(&self.linea);
        let mut e = self.estado.borrow_mut();
        e.lineas += 1;
        e.busybox |= t.contains("BusyBox v");
        e.fin |= t.contains("caja-linux: fin de la prueba");
        e.terminal |= t.contains("caja-linux: terminal lista");
        e.arch |= t.contains("Welcome to Arch Linux ARM");
        e.login |= t.contains("login: root (automatic login)");
        e.prompt |= t.contains("[root@caja ");
        // La ráfaga se repone a `POR_SEGUNDO` líneas por segundo.
        let ahora = dashos::ahora_us();
        if e.lineas == 1 {
            e.cupo = RAFAGA;
            e.repuesto_us = ahora;
        }
        let nuevas = (ahora - e.repuesto_us) * POR_SEGUNDO / 1_000_000;
        if nuevas > 0 {
            e.cupo = (e.cupo + nuevas).min(RAFAGA);
            e.repuesto_us = ahora;
        }
        if e.callado && ahora - e.ultima_us > CALMA_US {
            e.callado = false;
            println!("  [caja-linux] linux: ({} líneas sin sacar al registro: demasiadas seguidas)", e.sin_sacar);
            e.sin_sacar = 0;
        }
        e.ultima_us = ahora;
        if e.cupo == 0 {
            e.callado = true;
        }
        if e.callado {
            e.sin_sacar += 1;
        } else {
            e.cupo -= 1;
            println!("  [caja-linux] linux: {}", t);
        }
        self.linea.clear();
    }
}

impl ExtremoConsola for Registro {
    fn salida(&mut self, datos: &[u8]) {
        if self.terminal && COMPARTIDO.ventana.load(Ordering::Acquire) == 1 {
            COMPARTIDO.salida.meter(datos);
        }
        for &c in datos {
            match (self.escape, c) {
                (0, 0x1b) => self.escape = 1,
                (1, b'[') => self.escape = 2,
                (1, b']' | b'P' | b'X' | b'^' | b'_') => self.escape = 3,
                (1, _) => self.escape = 0,
                (2, 0x40..=0x7e) => self.escape = 0,
                (2, _) => {}
                (3, 0x07) => self.escape = 0,
                (3, 0x1b) => self.escape = 4,
                (3, _) => {}
                (4, b'\\') => self.escape = 0,
                (4, _) => self.escape = 3,
                (_, b'\n') => self.sacar(),
                (_, 0x08) => {
                    self.linea.pop();
                }
                (_, c) if c >= 0x20 || c == b'\t' => {
                    self.linea.push(c);
                    if self.linea.len() >= 200 {
                        self.sacar();
                    }
                }
                _ => {}
            }
        }
    }
    fn hay_entrada(&self) -> bool {
        self.terminal && COMPARTIDO.entrada.hay()
    }
    fn entrada(&mut self, buf: &mut [u8]) -> usize {
        if self.terminal { COMPARTIDO.entrada.sacar(buf) } else { 0 }
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

// --- El disco de Arch: la partición de la caja Linux en el NVMe, por el servicio de disco ---
//
// El servicio de disco (`nvme`, protocolo en `docs/referencia/servicios.md`) da a la caja, por su nombre, la partición
// de la caja Linux de la tabla GPT (`dashos::particiones`), como si fuera un disco entero: su sector 0 es el primero de
// la partición y no puede nombrar nada de fuera. Al abrir, da una ventana de memoria DMA de 128 KB que comparten los dos;
// cada lectura o escritura es una petición por el canal con los sectores y el sitio en la ventana, y el disco lee o
// escribe ahí sin copias. La caja copia entre la ventana y la RAM del invitado. Las peticiones van de una en una, con la
// vCPU parada en su escritura a QueueNotify (una vCPU: el invitado no hace nada mientras tanto).

const VA_VENTANA_DISCO: usize = 0x32_0000_0000;
/// Lo que se pide al disco de una vez (la mitad de la ventana; la otra es para lo que el servicio lee por adelantado).
const PIEZA_DISCO: usize = 64 * 1024;
const D_ABRIR: u32 = 0;
const D_LEER: u32 = 1;
const D_ESCRIBIR: u32 = 2;
const D_VACIAR: u32 = 5;
/// Lo que se espera una respuesta del servicio de disco: al abrir, puede estar arrancando todavía.
const PLAZO_ABRIR_US: u64 = 30_000_000;
const PLAZO_DISCO_US: u64 = 10_000_000;

/// Lecturas, escrituras (peticiones al servicio y KB) y errores del disco de Arch.
#[derive(Default, Clone, Copy)]
struct CuentasDisco {
    lecturas: u64,
    kb_leidos: u64,
    escrituras: u64,
    kb_escritos: u64,
    vaciados: u64,
    errores: u64,
}

struct DiscoNvme {
    canal: u32,
    lectura: u32,
    vuelta: u32,
    sectores: u64,
    cuentas: Rc<RefCell<CuentasDisco>>,
}

impl DiscoNvme {
    /// Una petición y su respuesta: (estado, sectores de su tramo, tamaño del sector, la llave que venga).
    fn pedir(&self, orden: u32, cuantos: u32, primero: u64, plazo_us: u64) -> Option<(i32, u64, u32, Option<u32>)> {
        let mut p = [0u8; 24];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&cuantos.to_le_bytes());
        p[8..16].copy_from_slice(&primero.to_le_bytes());
        let copia = dashos::llave_duplicar(self.vuelta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        if copia <= 0 {
            return None;
        }
        if dashos::canal_escribir(self.canal, &p, &[copia as u32]) != 0 {
            dashos::cerrar(copia as u32);
            return None;
        }
        let mut r = [0u8; 24];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(self.lectura, &mut r, &mut ll, plazo_us);
        if n != 24 {
            if k > 0 {
                dashos::cerrar(ll[0]);
            }
            return None;
        }
        let u32_en = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
        Some((u32_en(0) as i32, u32_en(8) as u64 | (u32_en(12) as u64) << 32, u32_en(16), (k > 0).then_some(ll[0])))
    }

    /// Abre la partición de la caja: el disco, o por qué no.
    fn abrir(canal: u32, cuentas: &Rc<RefCell<CuentasDisco>>) -> Result<Self, String> {
        let (lectura, vuelta) = dashos::canal_crear();
        let d = DiscoNvme { canal, lectura, vuelta, sectores: 0, cuentas: cuentas.clone() };
        let Some((estado, sectores, tam_sector, ventana)) = d.pedir(D_ABRIR, 0, 0, PLAZO_ABRIR_US) else {
            return Err(String::from("el servicio de disco no contesta"));
        };
        let Some(v) = ventana.filter(|_| estado == 0) else {
            // -6: sin tramo (no hay partición de la caja Linux en el disco).
            return Err(alloc::format!("el servicio de disco no lo abre (estado {})", estado));
        };
        if tam_sector != 512 || sectores == 0 {
            return Err(alloc::format!("sectores de {} bytes (se esperan de 512)", tam_sector));
        }
        if dashos::memoria_tamano(v) < 2 * PIEZA_DISCO as i64 || dashos::memoria_proyectar(v, VA_VENTANA_DISCO, true) != 0 {
            return Err(String::from("su ventana no se proyecta"));
        }
        Ok(DiscoNvme { sectores, ..d })
    }

    /// Lee o escribe `n` sectores desde `sector` por la ventana (de pieza en pieza). Para escribir, `copiar` pone los
    /// datos en la ventana antes; para leer, los saca después.
    fn mover(&mut self, orden: u32, sector: u64, buf_len: usize, mut copiar: impl FnMut(usize, &mut [u8])) -> Result<(), ErrorBloques> {
        let mut hecho = 0;
        while hecho < buf_len {
            let k = (buf_len - hecho).min(PIEZA_DISCO);
            // SAFETY: la ventana del servicio de disco, proyectada en `VA_VENTANA_DISCO` (al menos 128 KB); solo la
            // tocan el servicio (cuando se le pide) y este hilo.
            let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA_DISCO as *mut u8, k) };
            if orden == D_ESCRIBIR {
                copiar(hecho, ventana);
                dashos::barrera();
            }
            let r = self.pedir(orden, (k / 512) as u32, sector + (hecho / 512) as u64, PLAZO_DISCO_US);
            let mut c = self.cuentas.borrow_mut();
            if !matches!(r, Some((0, ..))) {
                c.errores += 1;
                return Err(ErrorBloques);
            }
            if orden == D_LEER {
                dashos::barrera();
                copiar(hecho, ventana);
                c.lecturas += 1;
                c.kb_leidos += k as u64 / 1024;
            } else {
                c.escrituras += 1;
                c.kb_escritos += k as u64 / 1024;
            }
            hecho += k;
        }
        Ok(())
    }
}

impl Bloques for DiscoNvme {
    fn sectores(&self) -> u64 {
        self.sectores
    }
    fn leer(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), ErrorBloques> {
        let n = buf.len();
        self.mover(D_LEER, sector, n, |o, v| buf[o..o + v.len()].copy_from_slice(v))
    }
    fn escribir(&mut self, sector: u64, datos: &[u8]) -> Result<(), ErrorBloques> {
        self.mover(D_ESCRIBIR, sector, datos.len(), |o, v| v.copy_from_slice(&datos[o..o + v.len()]))
    }
    fn vaciar(&mut self) -> Result<(), ErrorBloques> {
        self.cuentas.borrow_mut().vaciados += 1;
        match self.pedir(D_VACIAR, 0, 0, PLAZO_DISCO_US) {
            Some((0, ..)) => Ok(()),
            _ => {
                self.cuentas.borrow_mut().errores += 1;
                Err(ErrorBloques)
            }
        }
    }
    fn id(&self) -> [u8; 20] {
        *b"dashos-caja-arch\0\0\0\0"
    }
}

/// La hora del calendario de DashOS para el PL031 del invitado.
struct RelojDashOS;

impl Reloj for RelojDashOS {
    fn segundos(&self) -> u64 {
        dashos::hora_ns() / 1_000_000_000
    }
}

// --- La red del invitado: un puerto del servicio de red (H9, `protocolo::red::PUERTO`) ---
//
// La tarjeta virtio-net del invitado tiene su propia MAC y sale a la red por la tarjeta de verdad de DashOS: el servicio
// de red hace de puente (lo que llega para esa MAC o de difusión va al puerto; lo que sale del puerto, con esa MAC de
// origen, a la tarjeta). Las tramas van por una memoria que crea el servicio: ranuras de recibir (las escribe él) y de
// enviar (las escribe la caja). Un hilo espera en el canal sus avisos y despierta a la vCPU.

const VA_RED: usize = 0x31_0000_0000;
const PILA_RED: usize = 64 * 1024;

/// El lado de la caja del puerto: el `ExtremoRed` de la virtio-net del invitado.
struct RedCaja {
    red: u32,
    rx: u32,
    tx: u32,
    /// Tramas del invitado que no cupieron, y tramas del servicio con un largo imposible.
    perdidas: Rc<RefCell<(u64, u64, u64, u64)>>,
}

fn red_palabra(i: usize) -> u32 {
    // SAFETY: la memoria del puerto, proyectada en `VA_RED` mientras vive la caja; las palabras de la cabecera.
    unsafe { core::ptr::read_volatile((VA_RED + 4 * i) as *const u32) }
}

fn red_poner(i: usize, v: u32) {
    // SAFETY: como `red_palabra`.
    unsafe { core::ptr::write_volatile((VA_RED + 4 * i) as *mut u32, v) }
}

impl caja_vmm::virtio::red::ExtremoRed for RedCaja {
    fn enviar(&mut self, trama: &[u8]) {
        let escritas = red_palabra(rp::T_TX_ESCRITAS);
        let mut c = self.perdidas.borrow_mut();
        if escritas.wrapping_sub(red_palabra(rp::T_TX_LEIDAS)) >= self.tx || trama.len() > rp::MAX_TRAMA {
            c.0 += 1;
            return;
        }
        let ranura = VA_RED + 64 + (self.rx + escritas % self.tx) as usize * rp::RANURA;
        // SAFETY: una ranura de enviar del puerto, dentro de su memoria; la trama cabe (`MAX_TRAMA` < `RANURA` - 16).
        unsafe {
            core::ptr::copy_nonoverlapping(trama.as_ptr(), (ranura + rp::RANURA_DATOS) as *mut u8, trama.len());
            core::ptr::write_volatile(ranura as *mut u32, trama.len() as u32);
        }
        dashos::barrera();
        red_poner(rp::T_TX_ESCRITAS, escritas.wrapping_add(1));
        c.2 += 1;
        let m = rp::TRAMAS_PUERTO.to_le_bytes();
        dashos::canal_escribir(self.red, &m, &[]);
    }

    fn hay_trama(&self) -> bool {
        red_palabra(rp::T_RX_ESCRITAS) != red_palabra(rp::T_RX_LEIDAS)
    }

    fn recibir(&mut self) -> Option<Vec<u8>> {
        let (escritas, leidas) = (red_palabra(rp::T_RX_ESCRITAS), red_palabra(rp::T_RX_LEIDAS));
        if escritas == leidas {
            return None;
        }
        let mut c = self.perdidas.borrow_mut();
        // El servicio no puede tener más llenas que ranuras: si lo dice, se tira todo.
        if escritas.wrapping_sub(leidas) > self.rx {
            c.1 += 1;
            red_poner(rp::T_RX_LEIDAS, escritas);
            return None;
        }
        dashos::barrera();
        let ranura = VA_RED + 64 + (leidas % self.rx) as usize * rp::RANURA;
        // SAFETY: una ranura de recibir del puerto.
        let n = unsafe { core::ptr::read_volatile(ranura as *const u32) } as usize;
        let t = (rp::MIN_TRAMA..=rp::MAX_TRAMA).contains(&n).then(|| {
            let mut v = alloc::vec![0u8; n];
            // SAFETY: como arriba; `n` cabe en la ranura.
            unsafe { core::ptr::copy_nonoverlapping((ranura + rp::RANURA_DATOS) as *const u8, v.as_mut_ptr(), n) };
            v
        });
        dashos::barrera();
        red_poner(rp::T_RX_LEIDAS, leidas.wrapping_add(1));
        match t {
            Some(t) => {
                c.3 += 1;
                Some(t)
            }
            None => {
                c.1 += 1;
                // Una trama vacía: el aparato devuelve el búfer sin nada.
                Some(Vec::new())
            }
        }
    }
}

/// Pide al servicio de red un puerto para la MAC `mac` (5 s como mucho) y lanza el hilo que espera sus avisos.
fn abrir_red(red: u32, mac: [u8; 6], perdidas: &Rc<RefCell<(u64, u64, u64, u64)>>) -> Option<RedCaja> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 10];
    m[0..4].copy_from_slice(&rp::PUERTO.to_le_bytes());
    m[4..10].copy_from_slice(&mac);
    if dashos::canal_escribir(red, &m, &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    let mut r = [0u8; 8];
    let mut ll = [0u32; 1];
    let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 5_000_000);
    let estado = if n == 8 { i32::from_le_bytes(r[4..8].try_into().unwrap()) } else { n as i32 };
    if n != 8 || k != 1 || estado != 0 || dashos::memoria_proyectar(ll[0], VA_RED, true) != 0 {
        println!("  [caja-linux] red: el servicio de red no da un puerto ({}): sin red", estado);
        dashos::cerrar(lectura);
        return None;
    }
    let (rx, tx) = (red_palabra(rp::T_RANURAS_RX), red_palabra(rp::T_RANURAS_TX));
    let tam = dashos::memoria_tamano(ll[0]);
    if !(1..=256).contains(&rx) || !(1..=256).contains(&tx) || 64 + (rx + tx) as i64 * rp::RANURA as i64 > tam {
        println!("  [caja-linux] red: el puerto no cuadra ({} y {} ranuras en {} bytes): sin red", rx, tx, tam);
        return None;
    }
    let pila: &'static mut [u8] = Box::leak(alloc::vec![0u8; PILA_RED].into_boxed_slice());
    if dashos::hilos::crear(hilo_red, lectura as u64, pila, 0) < 0 {
        return None;
    }
    Some(RedCaja { red, rx, tx, perdidas: perdidas.clone() })
}

/// Espera los avisos del servicio de red (tramas para el invitado) y despierta a la vCPU.
fn hilo_red(lectura: u64) -> u64 {
    let mut b = [0u8; 16];
    let mut ll = [0u32; 4];
    loop {
        let (n, k) = dashos::canal_leer(lectura as u32, &mut b, &mut ll);
        for &l in &ll[..k.min(4)] {
            dashos::cerrar(l);
        }
        if n < 0 {
            return 0;
        }
        COMPARTIDO.avisar();
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

/// Arranca la Image de `l.linux` con el initramfs de `l.initrd` en una máquina con hasta `ram_invitado` bytes de RAM y
/// los aparatos del crate (PL011, PL031, virtio-console, virtio-blk), hasta que Linux la apaga por PSCI o pasan 4
/// minutos (10 con terminal). Con la llave de "ventanas", la consola sale también en una ventana de terminal, y lo que
/// se teclea en ella llega a hvc0.
///
/// Sin initramfs, Arch Linux ARM: el disco virtio es la partición de la caja en el NVMe (por "disco") y Linux arranca el
/// systemd de Arch desde ella; la prueba acaba cuando alguien lo apaga (`poweroff`) o a los 25 minutos.
fn linux(l: &Llaves, ram_invitado: usize) -> bool {
    let Some(imagen) = l.linux.and_then(|k| datos(k, VA_IMAGEN)) else {
        println!("  [caja-linux] linux: sin la Image; MAL");
        return false;
    };
    let initrd = l.initrd.and_then(|k| datos(k, VA_INITRD));
    // Sin initramfs: Arch Linux ARM, con su disco en el NVMe.
    let cuentas_disco = Rc::new(RefCell::new(CuentasDisco::default()));
    let disco_arch = if initrd.is_none() {
        let Some(d) = l.disco else {
            println!("  [caja-linux] linux: sin initramfs ni servicio de disco: nada que arrancar; MAL");
            return false;
        };
        match DiscoNvme::abrir(d, &cuentas_disco) {
            Ok(d) => {
                println!("  [caja-linux] arch: la partición de la caja Linux, {} MB, como disco del invitado", d.sectores >> 11);
                Some(d)
            }
            Err(e) => {
                println!("  [caja-linux] arch: sin disco para Arch Linux ARM: {}; MAL", e);
                return false;
            }
        }
    } else {
        None
    };
    let arch = disco_arch.is_some();
    // La RAM: la que le toca, en trozos de 16 MB; si el cupo no da para tanto, lo que quepa dejando un trozo libre para
    // las tablas de la etapa 2 y el montón.
    let quiere = (ram_invitado / TROZO).clamp(1, MAX_TROZOS);
    let mut mems = [0u32; MAX_TROZOS];
    let mut n = 0;
    while n < quiere {
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
    if n < quiere && n > 0 {
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
    // La ventana de la terminal, en su hilo, antes de arrancar: así no se pierde nada de hvc0.
    let terminal = l.ventanas.is_some_and(|v| abrir_terminal(v));
    // SAFETY: los objetos, proyectados seguidos aquí; solo los toca esta `Ram` (y el invitado).
    let ram = unsafe { core::slice::from_raw_parts_mut(VA_RAM_LINUX as *mut u8, tam_ram) };
    let estado = Rc::new(RefCell::new(EstadoConsola::default()));
    let mut disco = alloc::vec![0u8; TAM_DISCO];
    disco[..MARCA_DISCO.len()].copy_from_slice(MARCA_DISCO);
    let disco = Rc::new(RefCell::new(disco));
    let mut maq = Vmm::nueva(RamCaja(Ram { base: disposicion::RAM, datos: ram }), Distribuidor::nuevo(VCPUS_LINUX, disposicion::NUM_SPIS), disposicion::GICD);
    let (v0, v1, v2) = (disposicion::virtio(0), disposicion::virtio(1), disposicion::virtio(2));
    // La red, si el servicio de red da un puerto: una MAC local al azar (02:da:...).
    let cuentas_red = Rc::new(RefCell::new((0u64, 0u64, 0u64, 0u64)));
    let mut mac = [0x02, 0xda, 0x50, 0, 0, 0];
    let mut azar = [0u8; 3];
    let _ = dashos::azar(&mut azar);
    mac[3..].copy_from_slice(&azar);
    let red = l.red.and_then(|r| abrir_red(r, mac, &cuentas_red));
    let mut consola = Consola::nueva(Registro::nuevo(&estado, true));
    consola.tamaño(COLUMNAS as u16, FILAS as u16);
    let bloques: Box<dyn Bloques> = match disco_arch {
        Some(d) => Box::new(d),
        None => Box::new(Disco(disco.clone())),
    };
    let tam_disco_kb = bloques.sectores() / 2;
    let puestos = maq.añadir(disposicion::UART, disposicion::SPI_UART, Box::new(Pl011::nuevo(Registro::nuevo(&estado, false)))).is_ok()
        && maq.añadir(disposicion::RTC, disposicion::SPI_RTC, Box::new(Pl031::nuevo(Box::new(RelojDashOS)))).is_ok()
        && maq.añadir(v0.base, v0.spi, Box::new(TransporteMmio::nuevo(consola))).is_ok()
        && maq.añadir(v1.base, v1.spi, Box::new(TransporteMmio::nuevo(Bloque::nuevo(bloques)))).is_ok();
    let con_red = red.is_some();
    let puestos = puestos && red.is_none_or(|r| maq.añadir(v2.base, v2.spi, Box::new(TransporteMmio::nuevo(caja_vmm::virtio::red::Red::nueva(Box::new(r), mac)))).is_ok());
    // Con Arch y el compositor: vsock, para el puente de Wayland (los programas gráficos de Linux, en ventanas de
    // DashOS). Va en el transporte siguiente al último.
    let n_virtio = if con_red { 3 } else { 2 };
    // Con la RAM del invitado, el compositor lee sus búferes sin copia (vistas, llamada 50).
    let wayland = arch && l.ventanas.is_some_and(|v| puente_caja::abrir(v, &mems, disposicion::RAM));
    let v3 = disposicion::virtio(n_virtio);
    let puestos = puestos && (!wayland || maq.añadir(v3.base, v3.spi, Box::new(TransporteMmio::nuevo(Vsock::nuevo(Box::new(puente_caja::ExtremoPuente))))).is_ok());
    let n_virtio = n_virtio + wayland as u32;
    // Con el compositor, también una virtio-gpu en 2D (S25): la pantalla del invitado, que sale en una ventana de DashOS
    // cuando un programa de Linux la usa (por KMS).
    let v4 = disposicion::virtio(n_virtio);
    let gpu = caja_vmm::virtio::gpu::Gpu::nueva(Box::new(pantalla_caja::ExtremoPantalla), &[(pantalla_caja::ANCHO, pantalla_caja::ALTO)], pantalla_caja::TOPE_RECURSOS);
    let puestos = puestos && (!wayland || maq.añadir(v4.base, v4.spi, Box::new(TransporteMmio::nuevo(gpu))).is_ok());
    let n_virtio = n_virtio + wayland as u32;
    let bootargs = match (arch, terminal) {
        (true, _) => String::from(BOOTARGS_ARCH),
        (false, true) => alloc::format!("{} caja.terminal", BOOTARGS),
        (false, false) => String::from(BOOTARGS),
    };
    let mut config = disposicion::config(tam_ram as u64, VCPUS_LINUX as u32, n_virtio, bootargs, plan.initrd);
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
    let vm = dashos::maquina_crear(permiso(), mems[0], disposicion::RAM, regs.pc, regs.x[0]);
    let mut bien = vm > 0;
    for (i, &mm) in mems.iter().enumerate().skip(1) {
        bien &= vm > 0 && dashos::maquina_memoria(vm as u32, mm, disposicion::RAM + (i * TROZO) as u64) == 0;
    }
    bien &= vm > 0 && dashos::maquina_gicv(vm as u32, GICC) == 0;
    // Las vCPU secundarias, apagadas hasta que Linux las encienda por PSCI (CPU_ON).
    let mut llaves = [0u32; VCPUS_LINUX];
    llaves[0] = vm as u32;
    for (j, l) in llaves.iter_mut().enumerate().skip(1) {
        let r = if bien { dashos::maquina_vcpu(vm as u32, j as u64, 0, 0) } else { -1 };
        bien &= r > 0;
        *l = r.max(0) as u32;
    }
    if !bien {
        println!("  [caja-linux] linux: no se pudo crear la máquina ({}); MAL", vm);
        return false;
    }
    let vm = vm as u32;
    COMPARTIDO.vcpu0.store(vm, Ordering::Release);
    println!(
        "  [caja-linux] linux: Image de {} KB en {:#x}, initramfs de {} KB, árbol de {} bytes en {:#x}, {} MB de RAM, {} vCPU, PL011, PL031, virtio-console, virtio-blk de {} KB{}; arranca",
        imagen.len() / 1024, plan.kernel, initrd.map_or(0, |d| d.len() / 1024), dtb.len(), plan.dtb, tam_ram >> 20, VCPUS_LINUX, tam_disco_kb,
        if con_red { " y virtio-net por el servicio de red" } else { "" }
    );
    if wayland {
        println!("  [caja-linux] wayland: vsock en el puerto {} para el puente de Wayland de Linux (CID 3)", puente_caja::PUERTO);
        println!("  [caja-linux] gpu: virtio-gpu en 2D con una pantalla de {}x{} y {} MB para recursos", pantalla_caja::ANCHO, pantalla_caja::ALTO, pantalla_caja::TOPE_RECURSOS >> 20);
    }
    let t0 = dashos::ahora_us();
    CAJA.poner(Caja::nueva(&llaves, maq, &config.memoria, true));
    let plazo_s = if arch { 1500 } else if terminal { 600 } else { 240 };
    let (fin, unidos) = CAJA.correr(plazo_s * 1_000_000);
    let ms = (dashos::ahora_us() - t0) / 1000;
    COMPARTIDO.vcpu0.store(0, Ordering::Release);
    let caja = CAJA.quitar();
    let cuentas = caja.as_ref().map(|c| c.cuentas).unwrap_or_default();
    drop(caja);
    for &l in &llaves {
        dashos::cerrar(l);
    }
    COMPARTIDO.acabada.store(true, Ordering::Release);
    let que = match fin {
        FinCaja::Apagada => texto(format_args!("Linux apagó la máquina por PSCI")),
        FinCaja::Reinicio => texto(format_args!("Linux pidió reiniciar")),
        FinCaja::Plazo => texto(format_args!("sigue en marcha a los {} s", plazo_s)),
        FinCaja::Hvc(n, x) => texto(format_args!("HVC #{} con x0 = {:#x}", n, x[0])),
        FinCaja::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x}, clase {:#x})", pc, esr, esr >> 26)),
        FinCaja::Error(e) => texto(format_args!("error {}", e)),
    };
    let c = &cuentas;
    println!("  [caja-linux] linux: se para la máquina a los {} ms: {}", ms, que.as_str());
    println!(
        "  [caja-linux] linux: {} salidas: {} a aparatos ({} abortos), {} PSCI, {} WFI ({} ms dormido), {} por el reloj u otra vCPU, {} registros; {} interrupciones inyectadas, {} patadas entre vCPU",
        c.salidas, c.aparatos, c.abortos, c.psci, c.wfi, c.dormido_us / 1000, c.interrumpido, c.registros, c.inyectadas, c.patadas
    );
    println!("  [caja-linux] linux: hilos de las {} vCPU unidos al acabar: {}", VCPUS_LINUX, unidos);
    let e = estado.borrow();
    let d = disco.borrow();
    let escrito = &d[4096..4096 + ESCRITO.len()] == ESCRITO;
    let ok = unidos == VCPUS_LINUX
        && if arch {
            matches!(fin, FinCaja::Apagada) && e.arch && e.login && e.prompt
        } else {
            matches!(fin, FinCaja::Apagada) && e.busybox && e.fin && escrito && (!terminal || e.terminal)
        };
    if e.sin_sacar > 0 {
        println!("  [caja-linux] linux: ({} líneas sin sacar al registro: demasiadas seguidas)", e.sin_sacar);
    }
    if wayland {
        puente_caja::PUENTE.con(|p| {
            if let Some(p) = p.as_ref() {
                let c = p.cuentas();
                println!(
                    "  [caja-linux] wayland: {} transportes ({} cortados por tramas malas), {} clientes de Linux, {} KB de Wayland al compositor y {} KB al invitado, {} KB de búferes copiados, {} memorias ({} sin copia, vistas de la RAM del invitado; {} vistas no hechas), {} tubos del portapapeles ({} bytes), {} cosas malas tiradas, {} clientes cerrados",
                    p.transportes, p.cortados, c.clientes, c.bytes_al_compositor / 1024, c.bytes_al_invitado / 1024, c.bytes_de_memoria / 1024, c.memorias, c.vistas, c.vistas_no, c.tubos, c.bytes_de_tubos, c.malas, c.cerrados
                );
            }
        });
    }
    if wayland {
        pantalla_caja::informe();
    }
    if con_red {
        let r = cuentas_red.borrow();
        println!("  [caja-linux] red: {} tramas del invitado a la red, {} de la red al invitado; {} perdidas sin sitio, {} malas del servicio", r.2, r.3, r.0, r.1);
    }
    if terminal {
        println!(
            "  [caja-linux] terminal: {} bytes de la consola no cupieron en la ventana; {} teclas no cupieron hacia Linux",
            COMPARTIDO.salida.perdidos.load(Ordering::Relaxed), COMPARTIDO.entrada.perdidos.load(Ordering::Relaxed)
        );
    }
    let si = |b: bool| if b { "sí" } else { "no" };
    if arch {
        let c = cuentas_disco.borrow();
        println!(
            "  [caja-linux] arch: disco: {} lecturas ({} KB), {} escrituras ({} KB), {} vaciados, {} errores",
            c.lecturas, c.kb_leidos, c.escrituras, c.kb_escritos, c.vaciados, c.errores
        );
        println!(
            "  [caja-linux] arch: {} líneas por la consola; systemd de Arch Linux ARM: {}, root entra en hvc0: {}, su shell: {}: {}",
            e.lineas, si(e.arch), si(e.login), si(e.prompt), if ok { "bien" } else { "MAL" }
        );
    } else {
        println!(
            "  [caja-linux] linux: {} líneas por la consola, busybox en marcha: {}, el disco escrito por Linux: {}: {}",
            e.lineas, si(e.busybox), si(escrito), if ok { "bien" } else { "MAL" }
        );
    }
    for &mm in mems {
        dashos::cerrar(mm);
    }
    ok
}

// --- La terminal: una ventana con la consola del invitado ---
//
// Un hilo del servicio con su propio cliente de Wayland (por el canal "ventanas"). Lo que escribe Linux por hvc0 le
// llega por `COMPARTIDO.salida`; lo mete en una `Terminal` (usuario/src/terminal.rs) y pinta las filas que cambian, una
// vez por cuadro como mucho. Lo que se teclea va a `COMPARTIDO.entrada` y despierta a la vCPU.

const COLUMNAS: usize = 80;
const FILAS: usize = 25;
/// El tamaño de la letra (Roboto Mono) y el borde alrededor de la rejilla, en píxeles.
const LETRA: u32 = 13;
const BORDE: i32 = 8;
/// El fondo, la letra y el cursor (del tema oscuro del kit), y los 16 colores de la terminal.
const FONDO: u32 = 0xff14_1218;
const LETRA_COLOR: u32 = 0xffe6_e0e9;
const PALETA: [u32; 16] = [
    0xff1d1b20, 0xffd0605e, 0xff7fb36d, 0xffd9b85c, 0xff6f9bd6, 0xffb48ad9, 0xff5fb8b8, 0xffcac4cf, 0xff6b6670, 0xfff2898a,
    0xffa6d68f, 0xfff0d58a, 0xff9cbdf0, 0xffd0b0f0, 0xff8ad8d8, 0xffffffff,
];
const VA_TERMINAL: usize = 0x30_0000_0000;
const PILA_TERMINAL: usize = 256 * 1024;

/// Abre la ventana de la terminal en un hilo y espera a que esté en pantalla (como mucho 10 s). Falso si no pudo.
fn abrir_terminal(ventanas: u32) -> bool {
    ventana_cliente::usar_llave(ventanas);
    let pila: &'static mut [u8] = Box::leak(alloc::vec![0u8; PILA_TERMINAL].into_boxed_slice());
    if dashos::hilos::crear(hilo_terminal, 0, pila, 0) < 0 {
        println!("  [caja-linux] terminal: no se pudo crear su hilo");
        return false;
    }
    let hasta = dashos::ahora_us() + 10_000_000;
    while COMPARTIDO.ventana.load(Ordering::Acquire) == 0 && dashos::ahora_us() < hasta {
        dashos::dormir_us(10_000);
    }
    COMPARTIDO.ventana.load(Ordering::Acquire) == 1
}

fn hilo_terminal(_: u64) -> u64 {
    let r = terminal();
    if COMPARTIDO.ventana.load(Ordering::Acquire) == 0 {
        COMPARTIDO.ventana.store(2, Ordering::Release);
    }
    r
}

/// Lo que mandan las teclas a Linux (como un VT100: Intro es un retorno de carro, el retroceso es DEL).
fn bytes_tecla(t: Tecla, control: bool, sal: &mut Vec<u8>) {
    match t {
        Tecla::Letra(c) if control && c.is_ascii_alphabetic() => sal.push(c.to_ascii_lowercase() as u8 & 0x1f),
        Tecla::Letra(c) => {
            let mut b = [0u8; 4];
            sal.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
        }
        Tecla::Intro => sal.push(b'\r'),
        Tecla::Retroceso => sal.push(0x7f),
        Tecla::Tabulador | Tecla::TabuladorAtras => sal.push(b'\t'),
        Tecla::Escape => sal.push(0x1b),
        Tecla::Suprimir => sal.extend_from_slice(b"\x1b[3~"),
        Tecla::Arriba | Tecla::AltArriba => sal.extend_from_slice(b"\x1b[A"),
        Tecla::Abajo | Tecla::AltAbajo => sal.extend_from_slice(b"\x1b[B"),
        Tecla::Derecha => sal.extend_from_slice(b"\x1b[C"),
        Tecla::Izquierda => sal.extend_from_slice(b"\x1b[D"),
        Tecla::Inicio => sal.extend_from_slice(b"\x1b[H"),
        Tecla::Fin => sal.extend_from_slice(b"\x1b[F"),
    }
}

/// Las filas de una selección (bits, como `Terminal::tomar_sucias`).
fn filas_de(sel: Option<((usize, usize), (usize, usize))>) -> u64 {
    let Some((a, b)) = sel else { return 0 };
    (a.1.min(b.1)..=a.1.max(b.1)).fold(0, |m, y| m | 1 << y.min(63))
}

/// Pinta las filas `sucias` de la terminal (y la del cursor). Lo seleccionado (H9, el portapapeles), al revés.
fn pintar(l: &mut Lienzo, t: &Terminal, f: &mut Fuente, celda: (i32, i32, i32), sucias: u64, sel: Option<((usize, usize), (usize, usize))>) {
    let (cw, ch, base) = celda;
    let (cx, cy) = t.cursor();
    for y in 0..t.filas {
        if sucias & (1 << y.min(63)) == 0 {
            continue;
        }
        let py = BORDE + y as i32 * ch;
        l.borrar(Rect::new(0, py, l.ancho, ch), FONDO);
        for x in 0..t.columnas {
            let c = t.celda(x, y);
            let mut color = if c.color == terminal::DE_SERIE { LETRA_COLOR } else { PALETA[c.color as usize & 15] };
            let px = BORDE + x as i32 * cw;
            let inverso = (c.estilo & terminal::INVERSO != 0) != (x == cx && y == cy && !COMPARTIDO.acabada.load(Ordering::Relaxed));
            let inverso = inverso != sel.is_some_and(|(a, b)| Terminal::entre(x, y, a, b));
            if inverso {
                l.borrar(Rect::new(px, py, cw, ch), color);
                color = FONDO;
            }
            if c.c != ' ' {
                // Lo que la fuente no tiene sale como "?": así la caché de letras no crece sin fin.
                let letra = if f.tiene(c.c) { c.c } else { '?' };
                let mut b = [0u8; 4];
                f.dibujar(l, px, py + base, letra.encode_utf8(&mut b), LETRA, color);
            }
        }
    }
}

fn terminal() -> u64 {
    let mut fuente = Fuente::mono();
    let m = fuente.medir("M", LETRA);
    let cw = libm_ceil(fuente.avance('M', LETRA));
    // Filas juntas (la caja de la fuente deja mucho aire): la línea base a 3/4 de la celda.
    let ch = LETRA as i32 + 3;
    let base = (ch * 3 + 3) / 4;
    let _ = m;
    let (ancho, alto) = (COLUMNAS as i32 * cw + 2 * BORDE, FILAS as i32 * ch + 2 * BORDE);
    let Some(mut c) = Cliente::conectar() else {
        println!("  [caja-linux] terminal: no se puede conectar al compositor");
        return 1;
    };
    let Some(globales) = c.globales() else { return 1 };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else {
        println!("  [caja-linux] terminal: faltan globales");
        return 1;
    };
    let tamano = (ancho * alto * 4) as usize;
    let mem = dashos::memoria_crear(tamano);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_TERMINAL, true) != 0 {
        println!("  [caja-linux] terminal: sin memoria");
        return 1;
    }
    // SAFETY: recién proyectada aquí, `tamano` bytes; solo la pinta este hilo.
    let px = unsafe { core::slice::from_raw_parts_mut(VA_TERMINAL as *mut u32, (ancho * alto) as usize) };
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::TRANSFERIR, 0);
    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    // H9: el ratón (para seleccionar) y el portapapeles (copiar y pegar con los programas de Linux).
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    let mut pp = portapapeles_cliente::Portapapeles::default();
    let con_portapapeles = pp.enlazar(&mut e, &globales, REGISTRO, ASIENTO);
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ancho).int(alto).int(ancho * 4).uint(wayland::shm::XRGB8888).fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
    e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Linux (caja)")).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    let mut t = Terminal::nueva(COLUMNAS, FILAS);
    let mut teclado = Teclado::new();
    let mut control = 0u8;
    let mut configurada = false;
    // El aviso de frame que se espera (0: ninguno): no se pinta otra vez hasta que llega.
    let mut esperando = 0u32;
    let mut siguiente_aviso = AVISO;
    let mut buf = alloc::vec![0u8; 16 * 1024];
    let mut teclas = 0u64;
    let mut acabada_pintada = false;
    // La fila donde estaba el cursor la última vez que se pintó.
    let mut fila_cursor = 0usize;
    // H9: la selección (de dónde a dónde, en celdas), si se está arrastrando, las filas que hay que repintar por ella,
    // el ratón, mayúsculas y la serie del último evento (para poner la selección).
    let mut sel: Option<((usize, usize), (usize, usize))> = None;
    let mut arrastrando = false;
    let mut sucias_sel = 0u64;
    let mut raton = (0i32, 0i32);
    let mut mayusculas = 0u8;
    let celda_en = |(x, _): (i32, i32)| (((x - BORDE).max(0) / cw) as usize).min(COLUMNAS - 1);
    let fila_en = |(_, y): (i32, i32)| (((y - BORDE).max(0) / ch) as usize).min(FILAS - 1);
    let mut l = Lienzo::new(px, ancho, alto, ancho as usize);
    l.borrar(Rect::new(0, 0, ancho, alto), FONDO);
    // La pantalla de la virtio-gpu (S25) va en esta misma conexión, como otra ventana.
    let mut pantalla = pantalla_caja::VentanaPantalla::nueva();
    loop {
        pantalla.tic(&mut c);
        let plazo = if pantalla.pendiente() { 2_000 } else if pp.ocupado() { 5_000 } else if esperando != 0 { 100_000 } else { 15_000 };
        let ev = match c.evento_plazo(plazo) {
            Ok(ev) => ev,
            Err(()) => {
                println!("  [caja-linux] terminal: el compositor cerró la conexión");
                return 1;
            }
        };
        if let Some((o, orden, cuerpo)) = ev
            && !pantalla.evento(&mut c, o, orden, &cuerpo)
        {
            let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
            match (o, orden) {
                (XDG, xdg_wm_base::ev::PING) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                    c.mandar(&e);
                }
                (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                    let mut e = Escritor::new();
                    e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                    if !configurada {
                        configurada = true;
                        pintar(&mut l, &t, &mut fuente, (cw, ch, base), u64::MAX, None);
                        t.tomar_sucias();
                        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho).int(alto).fin();
                        e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(siguiente_aviso).fin();
                        esperando = siguiente_aviso;
                        siguiente_aviso += 1;
                        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                        println!("  [caja-linux] terminal: ventana en pantalla ({}x{} letras, {}x{} px, celdas de {}x{} desde ({}, {})){}", COLUMNAS, FILAS, ancho, alto, cw, ch, BORDE, BORDE, if con_portapapeles { ", con portapapeles" } else { "" });
                        COMPARTIDO.ventana.store(1, Ordering::Release);
                    }
                    c.mandar(&e);
                }
                (aviso, wl_callback::ev::DONE) if aviso == esperando => esperando = 0,
                (TECLADO, wl_keyboard::ev::KEYMAP) => {
                    if let Some(l) = c.juntador.tomar_llave() {
                        dashos::cerrar(l);
                    }
                }
                (TECLADO, wl_keyboard::ev::KEY) => {
                    let (codigo, pulsada) = (palabra(2), palabra(3) == 1);
                    if codigo == 42 || codigo == 54 {
                        mayusculas = if pulsada { mayusculas.saturating_add(1) } else { mayusculas.saturating_sub(1) };
                    }
                    // H9: Control+Mayúsculas+C copia lo seleccionado y Control+Mayúsculas+V pega (como en las
                    // terminales de Linux): no llegan a Linux.
                    if control > 0 && mayusculas > 0 && (codigo == 46 || codigo == 47) {
                        if pulsada && codigo == 46 {
                            if let Some((a, b)) = sel {
                                let texto = t.texto_entre(a, b);
                                println!("  [caja-linux] terminal: copiado \"{}\"", texto.chars().take(60).collect::<String>());
                                let mut e = Escritor::new();
                                pp.copiar(&mut e, false, texto, palabra(0));
                                c.mandar(&e);
                            }
                        } else if pulsada {
                            let mut e = Escritor::new();
                            if pp.pegar(&mut e, false) {
                                c.mandar(&e);
                            } else {
                                println!("  [caja-linux] terminal: nada que pegar");
                            }
                        }
                    } else if codigo == 29 || codigo == 97 {
                        control = if pulsada { control.saturating_add(1) } else { control.saturating_sub(1) };
                    } else if let Some(tk) = teclado.tecla(codigo, pulsada) {
                        let mut b = Vec::new();
                        bytes_tecla(tk, control > 0, &mut b);
                        if !b.is_empty() && !COMPARTIDO.acabada.load(Ordering::Relaxed) {
                            teclas += 1;
                            if teclas == 1 {
                                println!("  [caja-linux] terminal: primera tecla para Linux");
                            }
                            COMPARTIDO.entrada.meter(&b);
                            COMPARTIDO.avisar();
                        }
                    }
                }
                (TECLADO, wl_keyboard::ev::LEAVE) => {
                    control = 0;
                    mayusculas = 0;
                }
                // H9: seleccionar con el ratón (al soltar, a la selección primaria) y pegar la primaria con el central.
                (PUNTERO, wl_pointer::ev::ENTER) => raton = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8),
                (PUNTERO, wl_pointer::ev::MOTION) => {
                    raton = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                    if arrastrando && let Some((a, _)) = sel {
                        sucias_sel |= filas_de(sel);
                        sel = Some((a, (celda_en(raton), fila_en(raton))));
                        sucias_sel |= filas_de(sel);
                    }
                }
                (PUNTERO, wl_pointer::ev::BUTTON) => {
                    let (serie, boton, pulsado) = (palabra(0), palabra(2), palabra(3) == 1);
                    match (boton, pulsado) {
                        (272, true) => {
                            sucias_sel |= filas_de(sel);
                            let aqui = (celda_en(raton), fila_en(raton));
                            sel = Some((aqui, aqui));
                            arrastrando = true;
                            sucias_sel |= filas_de(sel);
                        }
                        (272, false) if arrastrando => {
                            arrastrando = false;
                            match sel {
                                Some((a, b)) if a != b => {
                                    let texto = t.texto_entre(a, b);
                                    println!("  [caja-linux] terminal: seleccionado \"{}\"", texto.chars().take(60).collect::<String>());
                                    let mut e = Escritor::new();
                                    pp.copiar(&mut e, true, texto, serie);
                                    c.mandar(&e);
                                }
                                _ => {
                                    sucias_sel |= filas_de(sel);
                                    sel = None;
                                }
                            }
                        }
                        (274, true) => {
                            let mut e = Escritor::new();
                            if pp.pegar(&mut e, true) {
                                c.mandar(&e);
                            }
                        }
                        _ => {}
                    }
                }
                (1, wl_display::ev::ERROR) => {
                    println!("  [caja-linux] terminal: error de protocolo");
                    return 1;
                }
                (o, orden) => {
                    let mut e = Escritor::new();
                    if pp.evento(&mut e, o, orden, &cuerpo, &mut || c.juntador.tomar_llave()) && !e.bytes.is_empty() {
                        c.mandar(&e);
                    }
                }
            }
        }
        // H9: lo pegado, a Linux (los saltos de línea, como Intro).
        if let Some(datos) = pp.mover() {
            println!("  [caja-linux] terminal: pegado ({} bytes)", datos.len());
            let datos: Vec<u8> = datos.iter().map(|&b| if b == b'\n' { b'\r' } else { b }).collect();
            if !datos.is_empty() && !COMPARTIDO.acabada.load(Ordering::Relaxed) {
                COMPARTIDO.entrada.meter(&datos);
                COMPARTIDO.avisar();
            }
        }
        // Lo nuevo de Linux, a la rejilla (como mucho 16 KB por vuelta: con una inundación, la ventana sigue viva).
        let n = COMPARTIDO.salida.sacar(&mut buf);
        if n > 0 {
            // Lo nuevo puede mover las filas: lo seleccionado deja de verse (lo copiado sigue).
            if sel.is_some() && !arrastrando {
                sucias_sel |= filas_de(sel);
                sel = None;
            }
            t.meter(&buf[..n]);
            if !t.respuesta.is_empty() {
                COMPARTIDO.entrada.meter(&core::mem::take(&mut t.respuesta));
                COMPARTIDO.avisar();
            }
        }
        let acabada = COMPARTIDO.acabada.load(Ordering::Relaxed);
        if acabada && !acabada_pintada {
            acabada_pintada = true;
            t.meter("\r\n\x1b[7m La máquina se ha apagado \x1b[0m".as_bytes());
            let mut e = Escritor::new();
            e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Linux (caja, apagada)")).fin();
            c.mandar(&e);
        }
        if configurada && esperando == 0 && (t.sucia() || sucias_sel != 0) {
            // La fila del cursor de antes y la de ahora también, para mover el cursor.
            let sucias = t.tomar_sucias() | 1 << t.cursor().1.min(63) | 1 << fila_cursor.min(63) | core::mem::take(&mut sucias_sel);
            fila_cursor = t.cursor().1;
            pintar(&mut l, &t, &mut fuente, (cw, ch, base), sucias, sel);
            let mut e = Escritor::new();
            e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho).int(alto).fin();
            e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(siguiente_aviso).fin();
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
            esperando = siguiente_aviso;
            siguiente_aviso += 1;
            c.mandar(&e);
        }
    }
}

fn libm_ceil(x: f32) -> i32 {
    let i = x as i32;
    if (i as f32) < x { i + 1 } else { i }
}

fn principal() -> u64 {
    // Las llaves se miran antes de nada: luego las ocupan los objetos que crea el servicio (su montón).
    let l = llaves();
    // SAFETY: recién proyectado aquí, solo de lectura; el objeto mide una página.
    let sistema = (dashos::memoria_proyectar(SISTEMA, VA_SISTEMA, false) == 0)
        .then(|| {
            let datos = unsafe { core::slice::from_raw_parts(VA_SISTEMA as *const u8, 24) };
            dashos::maquina::sistema(datos).map(|(ram, hiper)| (ram, hiper, dashos::maquina::memoria_pedida(datos)))
        })
        .flatten();
    let Some((ram, hiper, pedida)) = sistema else {
        println!("  [caja-linux] sin los datos del sistema; MAL");
        return 1;
    };
    if !hiper {
        println!("  [caja-linux] no hay hipervisor: DashOS no arrancó en EL2 (en QEMU, falta virtualization=on); sin máquinas virtuales");
        return 0;
    }
    // El permiso: en `virt`, de `sesiones` (sin preguntar: es la caja del sistema); en la Pi, de `inicio`.
    match (l.permiso, l.sesiones) {
        (Some(p), _) => PERMISO.store(p, Ordering::Relaxed),
        (None, Some(s)) => match pedir_permiso(s) {
            Ok(p) => {
                println!("  [caja-linux] sesiones le da el permiso de máquinas virtuales");
                PERMISO.store(p, Ordering::Relaxed);
            }
            Err(e) => {
                println!("  [caja-linux] sesiones no le da el permiso de máquinas virtuales ({}); MAL", e);
                return 1;
            }
        },
        (None, None) => {
            println!("  [caja-linux] sin el permiso de máquinas virtuales ni a quién pedirlo; MAL");
            return 1;
        }
    }
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
    // En la Pi (sin `sesiones`, con el permiso de `inicio`): que el hipervisor funciona, con un invitado que saluda.
    if l.permiso.is_some() {
        let bien = match maquina(hola) {
            Ok(vm) => {
                let r = matches!(correr(vm, &prueba(1_000_000)), Fin::Hvc(0, 0));
                dashos::cerrar(vm);
                r
            }
            Err(_) => false,
        };
        println!("  [caja-linux] hipervisor en marcha: un invitado saluda y acaba: {}", if bien { "bien" } else { "MAL" });
        return if bien { 0 } else { 1 };
    }
    // Una llave que no es el permiso no crea máquinas, aunque sea de memoria.
    let mem = dashos::memoria_crear(RAM);
    let tipo = dashos::maquina_crear(mem, mem, RAM_IPA, RAM_IPA, 0);
    dashos::cerrar(mem);
    if tipo != -6 {
        println!("  [caja-linux] con una llave de memoria por permiso: {} (se esperaba E_TIPO); MAL", tipo);
    }

    let vm = match maquina(hola) {
        Ok(v) => v,
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
    // Varias vCPU.
    // SAFETY: etiquetas de smp.s.
    let smp = unsafe { codigo(&invitado_smp, &invitado_smp_fin) };
    let cuarto = prueba_smp(smp, 0) & prueba_smp(smp, 1);
    println!("  [caja-linux] varias vCPU: {}", if cuarto { "bien" } else { "MAL" });
    bien &= cuarto;
    // Con la Image de un kernel de Linux, Linux hasta busybox (y, con ventana, una terminal con su shell).
    if l.linux.is_some() {
        let invitado = dashos::maquina::memoria_invitado_con(ram, pedida) as usize;
        println!("  [caja-linux] linux: la máquina tiene {} MB de RAM: hasta {} MB para el invitado", ram >> 20, invitado >> 20);
        bien &= linux(&l, invitado);
    }
    // Hecho: el hilo de la terminal sigue con su ventana; el proceso acaba entero.
    dashos::hilos::proceso_salir(if bien { 0 } else { 1 })
}
