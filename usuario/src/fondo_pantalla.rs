//! Lo que se ve en cada pantalla del fondo de escritorio (rama `fondos`): sus franjas de capa de fondo, la memoria de
//! lo que se ve (`vista`, la que lee el compositor), la imagen quieta de debajo (`base`) y, mientras dura una
//! transición, la nueva (`nueva`); y lo que se mueve encima (un fondo animado o un efecto).
//!
//! Cada franja es una superficie con su capa (los objetos de memoria miden 4 MB como mucho: a 1920 × 1080, dos). Sus
//! tres juegos de memoria se proyectan seguidos, una franja cada 4 MB, y se ven como un `Marco` de `kit::fondos`.
//!
//! El ritmo: cada cuadro (de la transición o del animado) pide el aviso de `frame` de las franjas y no pinta el
//! siguiente hasta que llega alguno. El compositor no lo da a una capa de fondo que no se ve (tapada por ventanas
//! opacas, con la pantalla apagada o bloqueada): así se para sola, sin gastar nada, y sigue al verse otra vez. Se pinta
//! sobre la misma memoria que lee el compositor, siempre tras su aviso (ya compuso lo de antes): una ventana que se
//! mueve encima podría pillar a medias un cuadro, en lo poco que cambia; a cambio no hace falta otro juego de 8 MB.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use kit::fondos::animado::Animado;
use kit::fondos::ritmo::Ritmo;
use kit::fondos::transicion::{Clase, Transicion};
use kit::fondos::{Ajuste, Marco};
use kit::imagen::Imagen;
use kit::lienzo::Rect;
use wayland::tablas::{wl_buffer, wl_compositor, wl_shm, wl_shm_pool, wl_surface, zwlr_layer_shell_v1, zwlr_layer_surface_v1};
use wayland::Escritor;

/// Donde van las memorias: salida, juego (0 vista, 1 base, 2 nueva) y franja, cada una en su hueco de 4 MB.
pub const VA_FRANJAS: usize = 0x48_0000_0000;
pub const MAX_FRANJAS: usize = 16;
pub const MAX_SALIDAS: usize = 4;
const HUECO: usize = 4 << 20;
/// Lo que mide como mucho un objeto de memoria del kernel.
const MAX_MEMORIA: usize = 4 << 20;
const COMPOSITOR: u32 = crate::ventana_cliente::COMPOSITOR;
const SHM: u32 = crate::ventana_cliente::SHM;
pub const CAPAS: u32 = 21;
/// Los avisos de `frame`: números de este tramo, que se reusan (sin pisar uno que espera).
const PRIMER_AVISO: u32 = 0x0010_0000;
const AVISOS: u32 = 0x0001_0000;
/// Si el aviso de una transición no llega en este rato (la pantalla no se ve), se acaba de golpe. En QEMU, componer
/// un paso de un fundido a 1920 × 1080 tarda más de 100 ms: un segundo.
const SIN_AVISO_US: u64 = 1_000_000;

fn va(salida: usize, juego: usize, franja: usize) -> usize {
    VA_FRANJAS + ((salida * 3 + juego) * MAX_FRANJAS + franja) * HUECO
}

/// Un juego de memorias (una por franja), proyectadas.
pub struct Juego {
    llaves: Vec<u32>,
    base: usize,
    ancho: usize,
    alto: usize,
    por_franja: usize,
}

impl Juego {
    fn crear(salida: usize, juego: usize, ancho: usize, alto: usize, por_franja: usize) -> Option<Juego> {
        let n = alto.div_ceil(por_franja);
        let mut j = Juego { llaves: Vec::new(), base: va(salida, juego, 0), ancho, alto, por_franja };
        for k in 0..n {
            let filas = por_franja.min(alto - k * por_franja);
            let m = dashos::memoria_crear(ancho * filas * 4);
            if (m as i32) <= 0 {
                j.soltar();
                return None;
            }
            if dashos::memoria_proyectar(m, va(salida, juego, k), true) != 0 {
                dashos::cerrar(m);
                j.soltar();
                return None;
            }
            j.llaves.push(m);
        }
        Some(j)
    }

    fn soltar(&mut self) {
        for (k, m) in self.llaves.drain(..).enumerate() {
            dashos::memoria_quitar(self.base + k * HUECO);
            dashos::cerrar(m);
        }
    }

    /// Lo de dentro, como marco.
    pub fn marco(&mut self) -> Marco<'_> {
        let trozos = (0..self.llaves.len())
            .map(|k| {
                let filas = self.por_franja.min(self.alto - k * self.por_franja);
                // SAFETY: proyectada en `crear` con `ancho × filas` píxeles; el préstamo de `self` la sujeta.
                unsafe { core::slice::from_raw_parts_mut((self.base + k * HUECO) as *mut u32, self.ancho * filas) }
            })
            .collect();
        Marco::nuevo(self.ancho, self.alto, self.por_franja, trozos).expect("trozos del juego")
    }
}

impl Drop for Juego {
    fn drop(&mut self) {
        self.soltar();
    }
}

pub struct Franja {
    pub sup: u32,
    pub capa: u32,
    pub y: i32,
    pub alto: i32,
    pool: u32,
    bufer: u32,
    /// El aviso de `frame` que espera (0, ninguno).
    aviso: u32,
}

/// Lo que se ve: su nombre (el camino, o `animado:x`), su semilla (0 si no tiene), y lo que mide la imagen.
#[derive(Clone)]
pub struct Visto {
    pub nombre: String,
    pub semilla: u32,
    pub ancho: u32,
    pub alto: u32,
}

/// Lo nuevo que hay que poner: una imagen (con su ajuste y su color), o algo que se pinta solo (un animado con su
/// propia base, o la paleta animada); y, encima de una imagen, un efecto.
pub enum Contenido {
    Imagen(Imagen, Ajuste, u32),
    Animado(Box<dyn Animado>),
}

pub struct Salida {
    pub indice: usize,
    #[allow(dead_code)]
    pub global: u32,
    pub objeto: u32,
    pub nombre: String,
    pub ancho: i32,
    pub alto: i32,
    pub lista: bool,
    pub franjas: Vec<Franja>,
    vista: Option<Juego>,
    base: Option<Juego>,
    nueva: Option<Juego>,
    pub visto: Option<Visto>,
    trans: Option<(Transicion, u64)>,
    /// Lo que se mueve: el animado, cuándo empezó y si espera a acabar la transición.
    anim: Option<(Box<dyn Animado>, u64)>,
    anim_pendiente: Option<Box<dyn Animado>>,
    pub ritmo: Ritmo,
    /// Cuándo toca el próximo cuadro del animado (µs), y desde cuándo se espera un aviso.
    proximo_us: u64,
    esperando_desde: u64,
    /// Para medir: cuadros, píxeles dañados y µs de CPU, desde la última vez que se dijo.
    pub cuadros: u64,
    pub pixeles: u64,
    pub cpu_us: u64,
    pub pausas: u64,
}

impl Salida {
    pub fn nueva(indice: usize, global: u32, objeto: u32) -> Salida {
        Salida {
            indice,
            global,
            objeto,
            nombre: String::new(),
            ancho: 0,
            alto: 0,
            lista: false,
            franjas: Vec::new(),
            vista: None,
            base: None,
            nueva: None,
            visto: None,
            trans: None,
            anim: None,
            anim_pendiente: None,
            ritmo: Ritmo::nuevo(kit::fondos::ritmo::TOPE_DE_SERIE),
            proximo_us: 0,
            esperando_desde: 0,
            cuadros: 0,
            pixeles: 0,
            cpu_us: 0,
            pausas: 0,
        }
    }

    fn por_franja(&self) -> usize {
        (MAX_MEMORIA / (self.ancho as usize * 4)).max(1)
    }

    #[allow(dead_code)]
    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.ancho, self.alto)
    }

    /// Si hay algo en marcha (transición o animado) y cuándo le toca (µs; `u64::MAX`, nunca).
    pub fn plazo(&self, ahora: u64) -> u64 {
        if self.esperando_desde != 0 {
            // Esperando el aviso: una transición no espera más de `SIN_AVISO_US`.
            return if self.trans.is_some() { self.esperando_desde + SIN_AVISO_US } else { u64::MAX };
        }
        if self.trans.is_some() {
            return ahora;
        }
        if self.anim.is_some() { self.proximo_us } else { u64::MAX }
    }

    /// Crea las franjas (capas, memoria y búferes) y espera sus `configure`. `false` si no se puede.
    fn crear_franjas(&mut self, c: &mut crate::ventana_cliente::Cliente, siguiente: &mut u32, pendientes: &mut Vec<(u32, u16, Vec<u8>)>) -> bool {
        if self.indice >= MAX_SALIDAS || self.ancho <= 0 || self.alto <= 0 {
            return false;
        }
        let (w, h, pf) = (self.ancho as usize, self.alto as usize, self.por_franja());
        if h.div_ceil(pf) > MAX_FRANJAS {
            return false;
        }
        let (Some(vista), Some(base)) = (Juego::crear(self.indice, 0, w, h, pf), Juego::crear(self.indice, 1, w, h, pf)) else {
            println!("  [fondo] {}: sin memoria para la pantalla", self.nombre);
            return false;
        };
        let mut e = Escritor::new();
        let mut nuevo = || {
            *siguiente += 1;
            *siguiente
        };
        for (k, &m) in vista.llaves.iter().enumerate() {
            let (y, alto) = ((k * pf) as i32, pf.min(h - k * pf) as i32);
            let (sup, capa, pool, bufer) = (nuevo(), nuevo(), nuevo(), nuevo());
            e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(sup).fin();
            e.empezar(CAPAS, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(capa).uint(sup).uint(self.objeto).uint(0).texto(Some("fondo")).fin();
            e.empezar(capa, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(1 | 4 | 8).fin();
            e.empezar(capa, zwlr_layer_surface_v1::req::SET_SIZE).uint(0).uint(alto as u32).fin();
            e.empezar(capa, zwlr_layer_surface_v1::req::SET_MARGIN).int(y).int(0).int(0).int(0).fin();
            e.empezar(capa, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(-1).fin();
            e.empezar(sup, wl_surface::req::COMMIT).fin();
            let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
            let tam = w * alto as usize * 4;
            e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tam as i32).fin();
            e.empezar(pool, wl_shm_pool::req::CREATE_BUFFER).uint(bufer).int(0).int(w as i32).int(alto).int(w as i32 * 4).uint(wayland::shm::XRGB8888).fin();
            self.franjas.push(Franja { sup, capa, y, alto, pool, bufer, aviso: 0 });
        }
        c.mandar(&e);
        let mut faltan = self.franjas.len();
        // Hasta 5 s: un timbre de la puerta (el canal, la vigilancia de `ENV:`) no corta la espera.
        let hasta = dashos::ahora_us() + 5_000_000;
        while faltan > 0 {
            let ahora = dashos::ahora_us();
            let ev = if ahora < hasta { c.evento_plazo(hasta - ahora) } else { Ok(None) };
            let (o, orden, cuerpo) = match ev {
                Ok(Some(x)) => x,
                Ok(None) if ahora < hasta => continue,
                _ => {
                    println!("  [fondo] el compositor no configura las capas");
                    self.soltar(c);
                    return false;
                }
            };
            if orden == zwlr_layer_surface_v1::ev::CONFIGURE && self.franjas.iter().any(|f| f.capa == o) {
                let serie = u32::from_le_bytes(cuerpo[0..4].try_into().unwrap());
                let mut e = Escritor::new();
                e.empezar(o, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
                c.mandar(&e);
                faltan -= 1;
            } else if o == 1 && orden == wayland::tablas::wl_display::ev::ERROR {
                println!("  [fondo] error de protocolo al pedir las capas");
                self.franjas.clear();
                return false;
            } else {
                pendientes.push((o, orden, cuerpo));
            }
        }
        self.vista = Some(vista);
        self.base = Some(base);
        true
    }

    /// Manda lo pintado: en cada franja, sus daños (de `danos`, en la pantalla) y, si `con_aviso`, un aviso de
    /// `frame` (si no espera ya uno).
    fn mandar(&mut self, c: &mut crate::ventana_cliente::Cliente, danos: &[Rect], con_aviso: bool, avisos: &mut Avisos) {
        let mut e = Escritor::new();
        for f in &mut self.franjas {
            let r = Rect::new(0, f.y, self.ancho, f.alto);
            let suyos: Vec<Rect> = danos.iter().map(|d| d.cruce(r)).filter(|d| !d.vacio()).collect();
            let aviso = con_aviso && f.aviso == 0;
            if suyos.is_empty() && !aviso {
                continue;
            }
            if !suyos.is_empty() {
                e.empezar(f.sup, wl_surface::req::ATTACH).uint(f.bufer).int(0).int(0).fin();
                // Muchos trozos pequeños (las partículas): juntos los cercanos, como mucho 32 (el compositor guarda 32 por
                // superficie), sin uno que lo cubra todo.
                for d in kit::fondos::juntar_danos(suyos, 32) {
                    e.empezar(f.sup, wl_surface::req::DAMAGE_BUFFER).int(d.x).int(d.y - f.y).int(d.w).int(d.h).fin();
                }
            }
            if aviso && let Some(id) = avisos.tomar() {
                f.aviso = id;
                e.empezar(f.sup, wl_surface::req::FRAME).uint(id).fin();
            }
            e.empezar(f.sup, wl_surface::req::COMMIT).fin();
        }
        c.mandar(&e);
        if con_aviso {
            self.esperando_desde = dashos::ahora_us().max(1);
        }
        self.pixeles += danos.iter().map(|r| r.w.max(0) as u64 * r.h.max(0) as u64).sum::<u64>();
    }

    /// Llegó el aviso `id`: si era de una de sus franjas, verdad.
    pub fn aviso(&mut self, id: u32, avisos: &mut Avisos) -> bool {
        let Some(f) = self.franjas.iter_mut().find(|f| f.aviso == id) else { return false };
        f.aviso = 0;
        avisos.soltar(id);
        if self.esperando_desde != 0 {
            self.esperando_desde = 0;
        }
        true
    }

    /// Pone `nuevo` (con `visto`): la primera vez, de golpe; después, con la transición `clase` de `dur_ms`. Encima, el
    /// efecto `efecto` si lo hay. `false` si no se pudo (y se queda lo de antes).
    #[allow(clippy::too_many_arguments)]
    pub fn poner(
        &mut self,
        c: &mut crate::ventana_cliente::Cliente,
        siguiente: &mut u32,
        pendientes: &mut Vec<(u32, u16, Vec<u8>)>,
        avisos: &mut Avisos,
        nuevo: Contenido,
        efecto: Option<Box<dyn Animado>>,
        visto: Visto,
        clase: Clase,
        dur_ms: u32,
    ) -> bool {
        let primera = self.franjas.is_empty();
        if primera && !self.crear_franjas(c, siguiente, pendientes) {
            return false;
        }
        // Lo nuevo, en `nueva` (o, la primera vez, directamente en la base).
        let (w, h, pf) = (self.ancho as usize, self.alto as usize, self.por_franja());
        let destino = if primera { self.base.as_mut() } else {
            // Una transición a medias: lo que se ve se queda como está y la nueva empieza desde ahí.
            self.trans = None;
            if self.nueva.is_none() {
                self.nueva = Juego::crear(self.indice, 2, w, h, pf);
            }
            self.nueva.as_mut()
        };
        let Some(destino) = destino else {
            println!("  [fondo] {}: sin memoria para lo nuevo; se queda lo de antes", self.nombre);
            return false;
        };
        let anim = match nuevo {
            Contenido::Imagen(img, ajuste, color) => {
                kit::fondos::pintar_marco(&img, ajuste, color, &mut destino.marco());
                efecto
            }
            Contenido::Animado(a) => {
                a.pintar_base(&mut destino.marco());
                Some(a)
            }
        };
        self.visto = Some(visto);
        self.anim = None;
        if primera {
            let (Some(v), Some(b)) = (self.vista.as_mut(), self.base.as_mut()) else { return false };
            let todo = Rect::new(0, 0, w as i32, h as i32);
            v.marco().copiar_de(&b.marco(), todo);
            self.mandar(c, &[todo], false, avisos);
            self.empezar_animado(anim);
        } else {
            self.anim_pendiente = anim;
            self.trans = Some((Transicion::nueva(clase, dur_ms, w, h), dashos::ahora_us()));
        }
        true
    }

    fn empezar_animado(&mut self, anim: Option<Box<dyn Animado>>) {
        self.anim = anim.map(|mut a| {
            a.olvidar();
            (a, dashos::ahora_us())
        });
        self.proximo_us = dashos::ahora_us();
    }

    /// Lo que toca hacer ahora (si toca): un paso de la transición o un cuadro del animado.
    pub fn atender(&mut self, c: &mut crate::ventana_cliente::Cliente, avisos: &mut Avisos, ahora: u64) {
        if self.esperando_desde != 0 {
            if self.trans.is_some() && ahora >= self.esperando_desde + SIN_AVISO_US {
                // No se ve: se acaba de golpe (nadie se pierde nada), y se sigue esperando el aviso.
                if let Some((mut t, _)) = self.trans.take() {
                    let fin = t.dur_ms.saturating_add(1);
                    if let (Some(v), Some(n)) = (self.vista.as_mut(), self.nueva.as_mut()) {
                        let danos = t.paso(fin, &mut v.marco(), &n.marco());
                        self.pausas += 1;
                        self.mandar(c, &danos, false, avisos);
                    }
                    println!("  [fondo] {}: transición {} acabada de golpe (sin verse) en {} pasos", self.nombre, t.clase.nombre(), t.pasos);
                }
                self.acabar_transicion();
            }
            return;
        }
        if let Some((mut t, inicio)) = self.trans.take() {
            let ms = ((ahora.saturating_sub(inicio)) / 1000) as u32;
            let (Some(v), Some(n)) = (self.vista.as_mut(), self.nueva.as_mut()) else { return };
            let t0 = dashos::ahora_us();
            let danos = t.paso(ms, &mut v.marco(), &n.marco());
            let acabada = t.terminada();
            self.cpu_us += dashos::ahora_us().saturating_sub(t0);
            if acabada {
                println!("  [fondo] {}: transición {} acabada en {} pasos ({} ms)", self.nombre, t.clase.nombre(), t.pasos, (ahora - inicio) / 1000);
                self.mandar(c, &danos, false, avisos);
                self.acabar_transicion();
            } else {
                self.trans = Some((t, inicio));
                self.mandar(c, &danos, true, avisos);
            }
            return;
        }
        let Some((a, inicio)) = self.anim.as_mut() else { return };
        if ahora < self.proximo_us {
            return;
        }
        let (Some(v), Some(b)) = (self.vista.as_mut(), self.base.as_mut()) else { return };
        // Lo que cuesta, por el reloj: la llamada 52 suma al cambiar de hilo, y un cuadro corto no llega a verse.
        let t0 = dashos::ahora_us();
        let danos = a.paso((ahora - *inicio) / 1000, &b.marco(), &mut v.marco());
        let gasto = dashos::ahora_us().saturating_sub(t0);
        let px: u64 = danos.iter().map(|r| r.w.max(0) as u64 * r.h.max(0) as u64).sum();
        let deseado = a.intervalo_ms();
        self.ritmo.cuadro(gasto, px);
        self.cpu_us += gasto;
        self.cuadros += 1;
        self.proximo_us = ahora + self.ritmo.intervalo(deseado) as u64 * 1000;
        // Sin nada que mandar, igual se pide el aviso: así, tapado, se para.
        self.mandar(c, &danos, true, avisos);
    }

    fn acabar_transicion(&mut self) {
        self.trans = None;
        // La nueva pasa a ser la base, y la base de antes se suelta (su hueco de memoria es el de la base).
        if let (Some(b), Some(n)) = (self.base.as_mut(), self.nueva.as_mut()) {
            let todo = Rect::new(0, 0, self.ancho, self.alto);
            b.marco().copiar_de(&n.marco(), todo);
        }
        self.nueva = None;
        let a = self.anim_pendiente.take();
        self.empezar_animado(a);
    }

    /// Lo que gasta el animado, para decirlo: (nombre, cuadros, píxeles, CPU µs, ritmo de ahora en ms) desde la
    /// última vez, y lo pone a cero.
    pub fn medida(&mut self) -> Option<(&'static str, u64, u64, u64, u32, u64)> {
        let (a, _) = self.anim.as_ref()?;
        let r = (a.nombre(), self.cuadros, self.pixeles, self.cpu_us, self.ritmo.intervalo(a.intervalo_ms()), self.pausas);
        (self.cuadros, self.pixeles, self.cpu_us) = (0, 0, 0);
        Some(r)
    }

    /// Si hay un animado o efecto en marcha, su nombre.
    pub fn animado(&self) -> Option<&'static str> {
        // También el que espera a que acabe la transición (si no, un "guardado" en medio la repetía).
        self.anim.as_ref().map(|a| a.0.nombre()).or(self.anim_pendiente.as_ref().map(|a| a.nombre()))
    }

    /// El fondo pequeño y desenfocado de la pantalla de bloqueo sale de la base.
    pub fn base_px(&mut self) -> Option<Marco<'_>> {
        self.base.as_mut().map(|b| b.marco())
    }

    /// Suelta todo (la salida se fue; aún no se usa: las pantallas que se van y vienen están pendientes).
    #[allow(dead_code)]
    pub fn soltar(&mut self, c: &mut crate::ventana_cliente::Cliente) {
        let mut e = Escritor::new();
        for f in self.franjas.drain(..) {
            e.empezar(f.bufer, wl_buffer::req::DESTROY).fin();
            e.empezar(f.pool, wl_shm_pool::req::DESTROY).fin();
            e.empezar(f.capa, zwlr_layer_surface_v1::req::DESTROY).fin();
            e.empezar(f.sup, wl_surface::req::DESTROY).fin();
        }
        c.mandar(&e);
        (self.vista, self.base, self.nueva, self.trans, self.anim) = (None, None, None, None, None);
    }
}

/// Los números de los avisos de `frame`: de un tramo propio, sin dar uno que aún espera.
pub struct Avisos {
    siguiente: u32,
    vivos: Vec<u32>,
}

impl Avisos {
    pub const fn nuevos() -> Avisos {
        Avisos { siguiente: 0, vivos: Vec::new() }
    }

    pub fn tomar(&mut self) -> Option<u32> {
        if self.vivos.len() >= 4096 {
            return None;
        }
        loop {
            let id = PRIMER_AVISO + self.siguiente % AVISOS;
            self.siguiente = self.siguiente.wrapping_add(1);
            if !self.vivos.contains(&id) {
                self.vivos.push(id);
                return Some(id);
            }
        }
    }

    pub fn soltar(&mut self, id: u32) {
        self.vivos.retain(|&x| x != id);
    }

    pub fn es_suyo(id: u32) -> bool {
        (PRIMER_AVISO..PRIMER_AVISO + AVISOS).contains(&id)
    }
}
