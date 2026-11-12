//! La pantalla de la caja Linux (S25): lo que el invitado muestra por su virtio-gpu (`caja_vmm::virtio::gpu`), en una
//! ventana de DashOS.
//!
//! Los hilos: el aparato corre en los de las vCPU, con el cerrojo de la caja tomado. Cuando el invitado vacía un
//! recurso que está en pantalla (`RESOURCE_FLUSH`), `ExtremoPantalla::mostrar` pasa lo dañado a XRGB en la memoria de
//! la ventana (la de su búfer de `wl_shm`) y apunta el daño; la primera vez crea esa memoria. La ventana la lleva el
//! hilo de la terminal de la caja, con su misma conexión al compositor ([`VentanaPantalla`]): un programa solo tiene una
//! conexión directa por su llave "ventanas" (dos se mezclarían). Cuando hay daño y el compositor ya pidió otro cuadro,
//! manda `damage`, `frame` y `commit`. Solo escribe la memoria el aparato; el hilo solo habla con el compositor. Si el
//! invitado pinta más deprisa de lo que compone DashOS, los vaciados se juntan en un cuadro (se ve el último).

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use caja_vmm::virtio::gpu::{self, Cuentas, ExtremoGpu, Rect, Vista};
use dashos::println;
use wayland::Escritor;
use wayland::tablas::{wl_callback, wl_compositor, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};

use super::ventana_cliente::{COMPOSITOR, Cliente, SHM, XDG};

// Los objetos de la ventana, en la conexión de la terminal (que usa del 2 al 14 y sus avisos desde el 100).
const POOL: u32 = 40;
const BUFER: u32 = 41;
const SUPERFICIE: u32 = 42;
const XDG_SUPERFICIE: u32 = 43;
const VENTANA: u32 = 44;
/// Sus avisos de frame, desde aquí (lejos de los de la terminal).
const AVISO: u32 = 0x0100_0000;

/// La pantalla que ve el invitado (`GET_DISPLAY_INFO`): la de la ventana.
pub const ANCHO: u32 = 640;
pub const ALTO: u32 = 480;
/// Memoria para los recursos del invitado (en el montón de la caja): con 640 x 480, unos 6 búferes y el cursor.
pub const TOPE_RECURSOS: u64 = 8 << 20;
const VA_PANTALLA: usize = 0x33_0000_0000;
const FONDO: u32 = 0xff10_1014;

/// Lo que comparten el aparato y el hilo de la ventana.
struct Estado {
    /// 0 sin ventana; 1 creada la memoria; 2 en pantalla; 3 no se pudo.
    ventana: AtomicU32,
    memoria: AtomicU32,
    /// Lo dañado desde el último cuadro (x0, y0, x1, y1) y si hay algo.
    d: [AtomicU32; 4],
    sucia: AtomicBool,
    apagada: AtomicBool,
    /// Cuadros que el hilo mandó al compositor, y vaciados que llegaron del invitado.
    cuadros: AtomicU64,
    vaciados: AtomicU64,
    /// Las cuentas del aparato (copiadas tras cada aviso del invitado).
    ordenes: AtomicU64,
    errores: AtomicU64,
    malas: AtomicU64,
    transferencias: AtomicU64,
    bytes: AtomicU64,
    recursos: AtomicU64,
    /// Del primer vaciado al último, para los cuadros por segundo del invitado.
    t_primero: AtomicU64,
    t_ultimo: AtomicU64,
}

static ESTADO: Estado = Estado {
    ventana: AtomicU32::new(0),
    memoria: AtomicU32::new(0),
    d: [AtomicU32::new(u32::MAX), AtomicU32::new(u32::MAX), AtomicU32::new(0), AtomicU32::new(0)],
    sucia: AtomicBool::new(false),
    apagada: AtomicBool::new(false),
    cuadros: AtomicU64::new(0),
    vaciados: AtomicU64::new(0),
    ordenes: AtomicU64::new(0),
    errores: AtomicU64::new(0),
    malas: AtomicU64::new(0),
    transferencias: AtomicU64::new(0),
    bytes: AtomicU64::new(0),
    recursos: AtomicU64::new(0),
    t_primero: AtomicU64::new(0),
    t_ultimo: AtomicU64::new(0),
};

/// La memoria de la ventana, ya proyectada (solo la escribe el aparato, desde las vCPU, con el cerrojo de la caja).
fn pixeles() -> Option<&'static mut [u32]> {
    if !matches!(ESTADO.ventana.load(Ordering::Acquire), 1 | 2) {
        return None;
    }
    // SAFETY: proyectada en `abrir` con ANCHO x ALTO píxeles y nunca quitada; solo se escribe con el cerrojo de la
    // caja tomado (los hilos de las vCPU), así que no hay dos `&mut` a la vez.
    Some(unsafe { core::slice::from_raw_parts_mut(VA_PANTALLA as *mut u32, (ANCHO * ALTO) as usize) })
}

/// Crea la memoria de la ventana (el hilo de la terminal la ve y abre la ventana). Falso si no se pudo (se intenta una
/// sola vez).
fn abrir() -> bool {
    if ESTADO.ventana.load(Ordering::Acquire) != 0 {
        return matches!(ESTADO.ventana.load(Ordering::Acquire), 1 | 2);
    }
    let tam = (ANCHO * ALTO * 4) as usize;
    let mem = dashos::memoria_crear(tam);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_PANTALLA, true) != 0 {
        println!("  [caja-linux] gpu: sin memoria para la ventana de la pantalla");
        ESTADO.ventana.store(3, Ordering::Release);
        return false;
    }
    ESTADO.memoria.store(mem, Ordering::Release);
    ESTADO.ventana.store(1, Ordering::Release);
    if let Some(px) = pixeles() {
        px.fill(FONDO);
    }
    true
}

pub struct ExtremoPantalla;

impl ExtremoGpu for ExtremoPantalla {
    fn mostrar(&mut self, _pantalla: u32, vista: &Vista<'_>, d: Rect) {
        // Una sola ventana: lo de la pantalla 0 (la única que se ofrece), desde su esquina.
        if !abrir() {
            return;
        }
        let Some(px) = pixeles() else { return };
        let d = d.cruce(&Rect { x: 0, y: 0, ancho: ANCHO, alto: ALTO });
        if d.vacio() {
            return;
        }
        for y in d.y..d.y + d.alto {
            let desde = (y * ANCHO + d.x) as usize;
            gpu::a_xrgb(vista.formato, vista.fila(d.x, y, d.ancho), &mut px[desde..desde + d.ancho as usize]);
        }
        ESTADO.d[0].fetch_min(d.x, Ordering::Relaxed);
        ESTADO.d[1].fetch_min(d.y, Ordering::Relaxed);
        ESTADO.d[2].fetch_max(d.x + d.ancho, Ordering::Relaxed);
        ESTADO.d[3].fetch_max(d.y + d.alto, Ordering::Relaxed);
        ESTADO.sucia.store(true, Ordering::Release);
        ESTADO.apagada.store(false, Ordering::Relaxed);
        let ahora = dashos::ahora_us();
        if ESTADO.vaciados.fetch_add(1, Ordering::Relaxed) == 0 {
            ESTADO.t_primero.store(ahora, Ordering::Relaxed);
            println!("  [caja-linux] gpu: Linux muestra algo en su pantalla ({}x{} desde ({}, {}), formato {})", vista.trozo.ancho, vista.trozo.alto, vista.trozo.x, vista.trozo.y, vista.formato);
        }
        ESTADO.t_ultimo.store(ahora, Ordering::Relaxed);
    }

    fn apagar(&mut self, _pantalla: u32) {
        ESTADO.apagada.store(true, Ordering::Relaxed);
    }

    fn cuentas(&mut self, c: &Cuentas) {
        ESTADO.ordenes.store(c.ordenes, Ordering::Relaxed);
        ESTADO.errores.store(c.errores, Ordering::Relaxed);
        ESTADO.malas.store(c.malas, Ordering::Relaxed);
        ESTADO.transferencias.store(c.transferencias, Ordering::Relaxed);
        ESTADO.bytes.store(c.bytes_transferidos, Ordering::Relaxed);
        ESTADO.recursos.store(c.recursos, Ordering::Relaxed);
    }
}

/// Lo de la pantalla para el registro, al acabar la máquina.
pub fn informe() {
    let o = |a: &AtomicU64| a.load(Ordering::Relaxed);
    let (vaciados, t) = (o(&ESTADO.vaciados), o(&ESTADO.t_ultimo).saturating_sub(o(&ESTADO.t_primero)));
    println!(
        "  [caja-linux] gpu: {} órdenes ({} con error, {} malas), {} recursos creados, {} transferencias ({} KB), {} vaciados a la pantalla ({} por segundo mientras pintó), {} cuadros en la ventana",
        o(&ESTADO.ordenes),
        o(&ESTADO.errores),
        o(&ESTADO.malas),
        o(&ESTADO.recursos),
        o(&ESTADO.transferencias),
        o(&ESTADO.bytes) / 1024,
        vaciados,
        if t > 0 { vaciados * 1_000_000 / t } else { 0 },
        o(&ESTADO.cuadros)
    );
}

/// La ventana de la pantalla, dentro del cliente de Wayland de la terminal (su hilo la llama en cada vuelta). Sale cuando
/// Linux muestra algo en su pantalla y se quita cuando la apaga (un programa de KMS que acaba).
pub struct VentanaPantalla {
    /// El pool y el búfer, creados (una vez).
    bufer: bool,
    /// La superficie y la ventana, vivas.
    visible: bool,
    configurada: bool,
    esperando: u32,
    siguiente: u32,
}

impl VentanaPantalla {
    pub const fn nueva() -> Self {
        Self { bufer: false, visible: false, configurada: false, esperando: 0, siguiente: AVISO }
    }

    /// Un evento del compositor. Verdadero si era de esta ventana.
    pub fn evento(&mut self, c: &mut Cliente, o: u32, orden: u16, cuerpo: &[u8]) -> bool {
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        match (o, orden) {
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) if self.visible => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                if !self.configurada {
                    self.configurada = true;
                    // El primer cuadro, entero.
                    ESTADO.d[0].store(0, Ordering::Relaxed);
                    ESTADO.d[1].store(0, Ordering::Relaxed);
                    ESTADO.d[2].store(ANCHO, Ordering::Relaxed);
                    ESTADO.d[3].store(ALTO, Ordering::Relaxed);
                    ESTADO.sucia.store(true, Ordering::Release);
                    e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                    if ESTADO.ventana.swap(2, Ordering::AcqRel) != 2 {
                        println!("  [caja-linux] gpu: la pantalla de Linux, en una ventana de {}x{}", ANCHO, ALTO);
                    }
                }
                c.mandar(&e);
                true
            }
            (aviso, wl_callback::ev::DONE) if aviso >= AVISO => {
                if aviso == self.esperando {
                    self.esperando = 0;
                }
                true
            }
            (VENTANA, _) | (XDG_SUPERFICIE, _) | (SUPERFICIE, _) | (BUFER, _) | (POOL, _) => true,
            _ => false,
        }
    }

    /// Abre la ventana cuando Linux muestra algo, la quita cuando apaga su pantalla, y manda lo dañado si el
    /// compositor ya pidió otro cuadro.
    pub fn tic(&mut self, c: &mut Cliente) {
        if !matches!(ESTADO.ventana.load(Ordering::Acquire), 1 | 2) {
            return;
        }
        let apagada = ESTADO.apagada.load(Ordering::Acquire);
        let mut e = Escritor::new();
        if !self.bufer {
            self.bufer = true;
            let (ancho, alto) = (ANCHO as i32, ALTO as i32);
            let copia = dashos::llave_duplicar(ESTADO.memoria.load(Ordering::Acquire), dashos::LEER | dashos::TRANSFERIR, 0);
            e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(ancho * alto * 4).fin();
            e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER).uint(BUFER).int(0).int(ancho).int(alto).int(ancho * 4).uint(wayland::shm::XRGB8888).fin();
        }
        if !self.visible && !apagada {
            self.visible = true;
            self.configurada = false;
            e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
            e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
            e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
            e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some("Linux (pantalla de la caja)")).fin();
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        } else if self.visible && apagada {
            self.visible = false;
            self.configurada = false;
            self.esperando = 0;
            e.empezar(VENTANA, xdg_toplevel::req::DESTROY).fin();
            e.empezar(XDG_SUPERFICIE, xdg_surface::req::DESTROY).fin();
            e.empezar(SUPERFICIE, wl_surface::req::DESTROY).fin();
            println!("  [caja-linux] gpu: Linux apagó su pantalla; se quita su ventana");
        } else if self.configurada && self.esperando == 0 && ESTADO.sucia.swap(false, Ordering::AcqRel) {
            let x0 = ESTADO.d[0].swap(u32::MAX, Ordering::Relaxed).min(ANCHO);
            let y0 = ESTADO.d[1].swap(u32::MAX, Ordering::Relaxed).min(ALTO);
            let x1 = ESTADO.d[2].swap(0, Ordering::Relaxed).min(ANCHO);
            let y1 = ESTADO.d[3].swap(0, Ordering::Relaxed).min(ALTO);
            let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
            if w > 0 && h > 0 {
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(x0 as i32).int(y0 as i32).int(w as i32).int(h as i32).fin();
            }
            e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(self.siguiente).fin();
            e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
            self.esperando = self.siguiente;
            self.siguiente += 1;
            ESTADO.cuadros.fetch_add(1, Ordering::Relaxed);
        }
        if !e.bytes.is_empty() {
            c.mandar(&e);
        }
    }

    /// Si tiene algo que mandar en cuanto pueda (para que el hilo no duerma mucho).
    pub fn pendiente(&self) -> bool {
        if !matches!(ESTADO.ventana.load(Ordering::Acquire), 1 | 2) {
            return false;
        }
        let apagada = ESTADO.apagada.load(Ordering::Acquire);
        !self.bufer || self.visible == apagada || (self.configurada && self.esperando == 0 && ESTADO.sucia.load(Ordering::Acquire))
    }
}
