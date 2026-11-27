//! S10, Commodities (`docs/bitacoras/commodities.md`): el lado del programa. Pedir el permiso a `sesiones`, conectarse
//! al compositor por `dashos_commodities_v1` (solo se anuncia con el permiso), darse de alta, pedir lo concreto (atajos,
//! inactividad, apagar la pantalla, comportamientos) y leer lo que llega. Exchange usa lo mismo con `vigilar`.
//!
//! Va con `ventana_cliente` (`#[path]`, como él): lo usan Blanker, NoCapsLock, la prueba de `virt` y, desde `ordenes`,
//! FKey y Exchange.

use alloc::string::String;
use alloc::vec::Vec;
use dashos::protocolo::sesiones as s;
use wayland::tablas::{self, dashos_commodities_v1 as g, dashos_commodity_v1 as c, dashos_exchange_v1 as x, wl_callback, wl_display, wl_registry};
use wayland::{leer_args, Escritor, Valor};

use crate::ventana_cliente::Cliente;

/// Los objetos: el gestor, las commodities (hasta 4) y el Exchange.
pub const GESTOR: u32 = 30;
pub const PRIMERA: u32 = 31;
pub const MAX_COMMODITIES: u32 = 4;
pub const EXCHANGE: u32 = 40;
/// Para los `sync`, de aquí a `FIN_SYNC` (los avisos de frame de `ventana_cliente` van desde el 100).
const SYNC: u32 = 60;
const FIN_SYNC: u32 = 100;

/// Pide a `sesiones` (por la llave `sesiones`) el permiso `nombre` (`atajos`, `fondo` o `commodities`). El estado: 0,
/// concedido (el compositor ya lo sabe); si no, lo que dijo `sesiones` (-2, no; -1, sin respuesta).
pub fn pedir_permiso(sesiones: u32, nombre: &str) -> i32 {
    let n = nombre.as_bytes();
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&s::SERVICIO.to_le_bytes());
    p[4..8].copy_from_slice(&(n.len() as u32).to_le_bytes());
    p[12..12 + n.len()].copy_from_slice(n);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 8];
    let mut ll = [0u32; 1];
    // Si `sesiones` lo pregunta al usuario, la respuesta espera: hasta un minuto.
    let (leido, k, _) = if dashos::canal_escribir(sesiones, &p[..12 + n.len()], &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 60_000_000)
    } else {
        dashos::cerrar(suyo);
        (-1, 0, 0)
    };
    dashos::cerrar(lectura);
    ll[..k.min(1)].iter().for_each(|&l| {
        dashos::cerrar(l);
    });
    if leido < 4 {
        return -1;
    }
    i32::from_le_bytes(r[0..4].try_into().unwrap())
}

/// Lo que llega del compositor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evento {
    Pulsado { commodity: u32, numero: u32 },
    Negado { commodity: u32, numero: u32, motivo: u32, quien: String },
    Activo { commodity: u32, si: bool },
    Ensenar { commodity: u32 },
    Quitar { commodity: u32 },
    Inactivo { commodity: u32, si: bool },
    Retirado { commodity: u32, permiso: u32 },
    XCommodity { id: u32, nombre: String, titulo: String, descripcion: String, programa: String, activa: bool },
    XAtajo { id: u32, numero: u32, combinacion: String, estado: u32 },
    XFuera { id: u32 },
    XAjuste { cual: u32, valor: u32 },
    XListo,
    /// Contestó un `sync`.
    Hecho(u32),
    /// Error de protocolo: el compositor cortó.
    Error(String),
    Otro,
}

/// Una conexión con el protocolo de commodities.
pub struct Cx {
    pub cli: Cliente,
    /// Los globales del compositor (número, interfaz, versión): Exchange abre con ellos su ventana.
    pub globales: Vec<(u32, String, u32)>,
    commodities: u32,
    sync: u32,
}

impl Cx {
    /// Se conecta y enlaza el global. `Err` si no hay compositor o no se anuncia (sin el permiso).
    pub fn conectar() -> Result<Cx, &'static str> {
        let mut cli = Cliente::conectar().ok_or("sin compositor")?;
        let globales = cli.globales().ok_or("el compositor no contesta")?;
        let Some(&(nombre, _, _)) = globales.iter().find(|g| g.1 == "dashos_commodities_v1") else {
            return Err("sin el permiso (el compositor no lo anuncia)");
        };
        let mut e = Escritor::new();
        e.empezar(2, wl_registry::req::BIND).uint(nombre).texto(Some("dashos_commodities_v1")).uint(1).uint(GESTOR).fin();
        cli.mandar(&e);
        Ok(Cx { cli, globales, commodities: 0, sync: SYNC })
    }

    fn una(&mut self, f: impl FnOnce(&mut Escritor)) {
        let mut e = Escritor::new();
        f(&mut e);
        self.cli.mandar(&e);
    }

    /// Se da de alta como commodity: su objeto (o `None` si ya van `MAX_COMMODITIES`).
    pub fn registrar(&mut self, nombre: &str, titulo: &str, descripcion: &str) -> Option<u32> {
        if self.commodities >= MAX_COMMODITIES {
            return None;
        }
        let o = PRIMERA + self.commodities;
        self.commodities += 1;
        self.una(|e| e.empezar(GESTOR, g::req::REGISTRAR).uint(o).texto(Some(nombre)).texto(Some(titulo)).texto(Some(descripcion)).fin());
        Some(o)
    }

    pub fn atajo(&mut self, commodity: u32, numero: u32, combinacion: &str) {
        self.una(|e| e.empezar(commodity, c::req::ATAJO).uint(numero).texto(Some(combinacion)).fin());
    }

    pub fn quitar_atajo(&mut self, commodity: u32, numero: u32) {
        self.una(|e| e.empezar(commodity, c::req::QUITAR_ATAJO).uint(numero).fin());
    }

    pub fn inactividad(&mut self, commodity: u32, segundos: u32) {
        self.una(|e| e.empezar(commodity, c::req::INACTIVIDAD).uint(segundos).fin());
    }

    pub fn oscurecer(&mut self, commodity: u32) {
        self.una(|e| e.empezar(commodity, c::req::OSCURECER).fin());
    }

    pub fn comportamiento(&mut self, commodity: u32, cual: u32, si: bool) {
        self.una(|e| e.empezar(commodity, c::req::COMPORTAMIENTO).uint(cual).uint(si as u32).fin());
    }

    /// Exchange: la lista llega detrás, con `XListo` al final.
    pub fn vigilar(&mut self) {
        self.una(|e| e.empezar(GESTOR, g::req::VIGILAR).uint(EXCHANGE).fin());
    }

    pub fn activar(&mut self, id: u32, si: bool) {
        self.una(|e| e.empezar(EXCHANGE, x::req::ACTIVAR).uint(id).uint(si as u32).fin());
    }

    pub fn ensenar(&mut self, id: u32) {
        self.una(|e| e.empezar(EXCHANGE, x::req::ENSENAR).uint(id).fin());
    }

    pub fn quitar(&mut self, id: u32) {
        self.una(|e| e.empezar(EXCHANGE, x::req::QUITAR).uint(id).fin());
    }

    pub fn ajuste(&mut self, cual: u32, valor: u32) {
        self.una(|e| e.empezar(EXCHANGE, x::req::AJUSTE).uint(cual).uint(valor).fin());
    }

    /// Un `sync`: cuando llegue `Hecho` con este número, el compositor ya atendió todo lo de antes.
    pub fn sync(&mut self) -> u32 {
        self.sync = if self.sync + 1 >= FIN_SYNC { SYNC + 1 } else { self.sync + 1 };
        let n = self.sync;
        self.una(|e| e.empezar(1, wl_display::req::SYNC).uint(n).fin());
        n
    }

    /// El siguiente evento, esperando como mucho `us`. `Ok(None)` si no llegó nada; `Err` si se cortó.
    pub fn evento(&mut self, us: u64) -> Result<Option<Evento>, ()> {
        let Some((o, orden, cuerpo)) = self.cli.evento_plazo(us)? else { return Ok(None) };
        Ok(Some(leer(o, orden, &cuerpo, self.commodities)))
    }

    /// Si un evento en crudo (de `cli`) es de las commodities o de `wl_display`: entonces, leído.
    pub fn suyo(&self, o: u32, orden: u16, cuerpo: &[u8]) -> Option<Evento> {
        (o == 1 || o == GESTOR || o == EXCHANGE || (SYNC..FIN_SYNC).contains(&o) || (PRIMERA..PRIMERA + self.commodities).contains(&o)).then(|| leer(o, orden, cuerpo, self.commodities))
    }

    /// Espera eventos hasta el `Hecho` de un `sync` (o `us` sin nada): los que llegaron antes.
    pub fn hasta_hecho(&mut self, us: u64) -> Vec<Evento> {
        let n = self.sync();
        let mut out = Vec::new();
        while let Ok(Some(e)) = self.evento(us) {
            match e {
                Evento::Hecho(m) if m == n => break,
                Evento::Error(_) => {
                    out.push(e);
                    break;
                }
                e => out.push(e),
            }
        }
        out
    }
}

/// Un evento leído: de qué objeto y orden, con sus argumentos.
fn leer(o: u32, orden: u16, cuerpo: &[u8], commodities: u32) -> Evento {
    let args = |m: &wayland::Message| -> Vec<Valor> { leer_args(m, cuerpo, &mut core::iter::empty()).unwrap_or_default() };
    let t = |v: &Valor| String::from(v.texto());
    if o == 1 && orden == wl_display::ev::ERROR {
        let a = args(&tablas::WL_DISPLAY.events[orden as usize]);
        return Evento::Error(a.get(2).map(t).unwrap_or_default());
    }
    if (SYNC..FIN_SYNC).contains(&o) && orden == wl_callback::ev::DONE {
        return Evento::Hecho(o);
    }
    if (PRIMERA..PRIMERA + commodities).contains(&o) {
        let Some(m) = tablas::DASHOS_COMMODITY_V1.events.get(orden as usize) else { return Evento::Otro };
        let a = args(m);
        if a.len() != m.args.len() {
            return Evento::Otro;
        }
        return match orden {
            c::ev::PULSADO => Evento::Pulsado { commodity: o, numero: a[0].uint() },
            c::ev::ATAJO_NEGADO => Evento::Negado { commodity: o, numero: a[0].uint(), motivo: a[1].uint(), quien: t(&a[2]) },
            c::ev::ACTIVO => Evento::Activo { commodity: o, si: a[0].uint() != 0 },
            c::ev::ENSENAR => Evento::Ensenar { commodity: o },
            c::ev::QUITAR => Evento::Quitar { commodity: o },
            c::ev::INACTIVO => Evento::Inactivo { commodity: o, si: a[0].uint() != 0 },
            c::ev::RETIRADO => Evento::Retirado { commodity: o, permiso: a[0].uint() },
            _ => Evento::Otro,
        };
    }
    if o == EXCHANGE {
        let Some(m) = tablas::DASHOS_EXCHANGE_V1.events.get(orden as usize) else { return Evento::Otro };
        let a = args(m);
        if a.len() != m.args.len() {
            return Evento::Otro;
        }
        return match orden {
            x::ev::COMMODITY => Evento::XCommodity {
                id: a[0].uint(),
                nombre: t(&a[1]),
                titulo: t(&a[2]),
                descripcion: t(&a[3]),
                programa: t(&a[4]),
                activa: a[5].uint() != 0,
            },
            x::ev::ATAJO => Evento::XAtajo { id: a[0].uint(), numero: a[1].uint(), combinacion: t(&a[2]), estado: a[3].uint() },
            x::ev::FUERA => Evento::XFuera { id: a[0].uint() },
            x::ev::AJUSTE => Evento::XAjuste { cual: a[0].uint(), valor: a[1].uint() },
            x::ev::LISTO => Evento::XListo,
            _ => Evento::Otro,
        };
    }
    Evento::Otro
}
