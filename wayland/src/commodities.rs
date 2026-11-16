//! Commodities y Exchange (S10, `docs/bitacoras/commodities.md`), del lado del compositor: `dashos_commodities_v1`,
//! `dashos_commodity_v1` y `dashos_exchange_v1`.
//!
//! Nadie recibe toda la entrada. Una commodity pide lo concreto y el compositor le pasa solo lo suyo:
//! - **atajos** (permiso `atajos`): combinaciones que se comprueban con `crate::atajos` (nada con Super ni AltGr, ni el
//!   rescate, ni el volumen; siempre con Ctrl y Alt a la vez). Al pulsarla, `pulsado`, y la tecla no llega a la ventana. No
//!   llegan con la sesión bloqueada, con una ventana del sistema con el teclado (`guardian`, el selector) ni a una
//!   commodity de una sesión de detrás (U6). Dos de la misma persona con la misma combinación: la primera se la queda.
//! - **inactividad** (permiso `fondo`): un aviso tras N segundos (como poco `MIN_INACTIVIDAD_S`) y otro al volver.
//!   Con él, **oscurecer**: el compositor pinta la pantalla de negro hasta la siguiente entrada, que no llega a nadie.
//!   Solo si esa commodity está inactiva ahora: un programa no apaga la pantalla cuando quiere.
//! - **comportamientos** (permiso `fondo`): hoy, sin Bloq Mayús (NoCapsLock).
//! - **Exchange** (permiso `commodities`): la lista de las de su persona, activar, desactivar, enseñar, quitar, y los
//!   ajustes ClickToFront (doble clic para subir) y AutoPoint (el teclado sigue al ratón).
//!
//! Los permisos los dice `sesiones` sin marca (`AVISO_PODER`): `Escena::poner_poderes`. Quitar uno borra en el acto lo
//! que dependía de él.
//!
//! Hostil: topes por programa y en total; textos largos, permisos que no se tienen y comportamientos que no existen son
//! errores de protocolo; combinaciones malas, reservadas, ocupadas o de más se niegan con su motivo, sin cortar.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::{raiz, Cliente, Entrada, Escena, PorCliente, Rect};
use crate::atajos::{self, Combinacion, Motivo};
use crate::servidor::{Conexion, ErrorProtocolo};
use crate::shm::Memorias;
use crate::tablas::{dashos_commodities_v1, dashos_commodity_v1, dashos_exchange_v1};
use crate::Valor;

/// Los permisos de commodities de un cliente (lo que dice `sesiones`).
pub const PODER_ATAJOS: u32 = 1;
pub const PODER_FONDO: u32 = 2;
pub const PODER_COMMODITIES: u32 = 4;
pub const PODERES: u32 = PODER_ATAJOS | PODER_FONDO | PODER_COMMODITIES;

/// Topes.
pub const MAX_ATAJOS_PROGRAMA: usize = 32;
pub const MAX_ATAJOS: usize = 256;
/// Lo que se apunta de cada commodity (también lo negado, para enseñarlo en Exchange).
pub const MAX_APUNTADOS: usize = 64;
pub const MAX_COMMODITIES_PROGRAMA: usize = 4;
pub const MAX_EXCHANGE: usize = 8;
/// Clientes con permisos apuntados a la vez (la mayoría se apuntan antes de conectarse).
pub const MAX_PODERES: usize = 256;
pub const MAX_NOMBRE: usize = 32;
pub const MAX_TITULO: usize = 64;
pub const MAX_DESCRIPCION: usize = 128;
/// La inactividad más corta: con plazos cortos, "vuelve a haber entrada" diría cuándo se pulsa cada tecla.
pub const MIN_INACTIVIDAD_S: u32 = 10;
pub const MAX_INACTIVIDAD_S: u32 = 24 * 3600;
/// ClickToFront: dos clics en la misma ventana en menos de esto.
pub const DOBLE_CLIC_MS: u32 = 500;
/// Teclas tragadas a la vez (las que se sueltan después sin llegar a nadie).
const MAX_TRAGADAS: usize = 16;

/// Errores de `dashos_commodities_v1` (los mismos números valen en las otras dos).
pub const ERROR_SIN_PERMISO: u32 = 0;
pub const ERROR_DEMASIADOS: u32 = 1;
pub const ERROR_NO_VALE: u32 = 2;

/// Los comportamientos y los ajustes (los números del XML).
pub const SIN_BLOQ_MAYUS: u32 = 1;
pub const AJUSTE_CLICKTOFRONT: u32 = 1;
pub const AJUSTE_AUTOPOINT: u32 = 2;

const KEY_BLOQ_MAYUS: u32 = 58;

/// Una combinación pedida: su número, lo escrito y si es suya (0) o por qué no (el motivo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atajo {
    pub numero: u32,
    pub combinacion: Option<Combinacion>,
    pub texto: String,
    pub estado: u32,
}

/// El plazo de inactividad: cuánto, desde cuándo cuenta y si ya se avisó.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Plazo {
    ms: u32,
    desde_ms: u32,
    avisado: bool,
}

/// Una commodity.
#[derive(Clone, Debug)]
pub struct Commodity {
    pub id: u32,
    pub cliente: Cliente,
    pub objeto: u32,
    pub nombre: String,
    pub titulo: String,
    pub descripcion: String,
    pub activa: bool,
    pub atajos: Vec<Atajo>,
    plazo: Option<Plazo>,
    pub sin_bloq_mayus: bool,
    /// Si ya se apuntó en el registro que pide demasiados (una vez: uno que pide cien no llena el registro).
    dicho_demasiados: bool,
}

/// Lo que hay que mandar a un cliente (quien atiende lo escribe con `escribir_aviso`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Aviso {
    Pulsado { objeto: u32, numero: u32 },
    Negado { objeto: u32, numero: u32, motivo: u32, quien: String },
    Activo { objeto: u32, si: bool },
    Ensenar { objeto: u32 },
    Quitar { objeto: u32 },
    Inactivo { objeto: u32, si: bool },
    Retirado { objeto: u32, permiso: u32 },
    XCommodity { objeto: u32, id: u32, nombre: String, titulo: String, descripcion: String, programa: String, activa: bool },
    XAtajo { objeto: u32, id: u32, numero: u32, combinacion: String, estado: u32 },
    XFuera { objeto: u32, id: u32 },
    XAjuste { objeto: u32, cual: u32, valor: u32 },
    XListo { objeto: u32 },
}

/// Escribe un aviso en la conexión de su cliente.
pub fn escribir_aviso(con: &mut Conexion, a: &Aviso) {
    use dashos_commodity_v1::ev as c;
    use dashos_exchange_v1::ev as x;
    let s = &mut con.salida;
    match a {
        Aviso::Pulsado { objeto, numero } => s.empezar(*objeto, c::PULSADO).uint(*numero).fin(),
        Aviso::Negado { objeto, numero, motivo, quien } => s.empezar(*objeto, c::ATAJO_NEGADO).uint(*numero).uint(*motivo).texto(Some(quien)).fin(),
        Aviso::Activo { objeto, si } => s.empezar(*objeto, c::ACTIVO).uint(*si as u32).fin(),
        Aviso::Ensenar { objeto } => s.empezar(*objeto, c::ENSENAR).fin(),
        Aviso::Quitar { objeto } => s.empezar(*objeto, c::QUITAR).fin(),
        Aviso::Inactivo { objeto, si } => s.empezar(*objeto, c::INACTIVO).uint(*si as u32).fin(),
        Aviso::Retirado { objeto, permiso } => s.empezar(*objeto, c::RETIRADO).uint(*permiso).fin(),
        Aviso::XCommodity { objeto, id, nombre, titulo, descripcion, programa, activa } => s
            .empezar(*objeto, x::COMMODITY)
            .uint(*id)
            .texto(Some(nombre))
            .texto(Some(titulo))
            .texto(Some(descripcion))
            .texto(Some(programa))
            .uint(*activa as u32)
            .fin(),
        Aviso::XAtajo { objeto, id, numero, combinacion, estado } => {
            s.empezar(*objeto, x::ATAJO).uint(*id).uint(*numero).texto(Some(combinacion)).uint(*estado).fin()
        }
        Aviso::XFuera { objeto, id } => s.empezar(*objeto, x::FUERA).uint(*id).fin(),
        Aviso::XAjuste { objeto, cual, valor } => s.empezar(*objeto, x::AJUSTE).uint(*cual).uint(*valor).fin(),
        Aviso::XListo { objeto } => s.empezar(*objeto, x::LISTO).fin(),
    }
}

/// Lo de las commodities en la escena.
#[derive(Default)]
pub struct Estado {
    pub lista: Vec<Commodity>,
    siguiente: u32,
    /// Los Exchange (cliente y su objeto).
    pub exchanges: Vec<(Cliente, u32)>,
    /// Los permisos de cada cliente, por su marca.
    pub poderes: BTreeMap<Cliente, u32>,
    /// El nombre que dio el sistema a cada cliente (lo pone quien atiende): sale en Exchange.
    pub programas: BTreeMap<Cliente, String>,
    /// ClickToFront: el clic da el teclado y el doble clic sube la ventana (si no, un clic hace las dos cosas).
    pub clic_doble: bool,
    /// AutoPoint: la ventana bajo el ratón se lleva el teclado.
    pub autopoint: bool,
    /// La pantalla apagada (Blanker).
    pub oscuro: bool,
    /// Los modificadores pulsados, por lados (`atajos::mods_de_lados`).
    lados: u8,
    /// Teclas cuya pulsación no llegó a nadie: su soltar tampoco llega.
    tragadas: Vec<u32>,
    /// Los botones del ratón tal como llegan, y los que no llegan a nadie hasta soltarlos (el que despertó la pantalla).
    botones_crudos: u8,
    botones_tragados: u8,
    /// El último clic en una ventana, para el doble clic de ClickToFront.
    ultimo_clic: Option<((Cliente, u32), u32)>,
    /// Lo que hay que mandar.
    pub avisos: Vec<(Cliente, Aviso)>,
    /// Lo que pasó, en llano, para el registro del compositor (quien atiende lo escribe y lo vacía).
    pub registro: Vec<String>,
}

impl Estado {
    pub fn poderes_de(&self, c: Cliente) -> u32 {
        self.poderes.get(&c).copied().unwrap_or(0)
    }
}

impl<M: Memorias> Escena<M> {
    /// La persona de un cliente (`None`: del sistema).
    fn persona(&self, c: Cliente) -> Option<u32> {
        self.personas.get(&c).copied()
    }

    /// Si tiene el teclado una ventana del sistema (`guardian`, el selector).
    fn teclado_del_sistema(&self) -> bool {
        self.teclado.is_some_and(|(c, _)| self.del_sistema.contains(&c))
    }

    /// `sesiones` da (`dar`) o quita permisos de commodities a `cliente`. Devuelve los que tiene después. Lo que
    /// dependía de un permiso quitado se borra ya y se le dice (`retirado`).
    pub fn poner_poderes(&mut self, cliente: Cliente, bits: u32, dar: bool) -> u32 {
        let bits = bits & PODERES;
        let antes = self.cx.poderes_de(cliente);
        let despues = if dar { antes | bits } else { antes & !bits };
        if despues == 0 {
            self.cx.poderes.remove(&cliente);
        } else if self.cx.poderes.contains_key(&cliente) || self.cx.poderes.len() < MAX_PODERES {
            self.cx.poderes.insert(cliente, despues);
        } else {
            // Lleno: no se apunta (no verá el protocolo). Mejor sin permiso que con el de otro.
            self.cx.registro.push(alloc::format!("demasiados clientes con permisos de commodities: no se apunta {cliente}"));
            return antes;
        }
        let quitados = antes & !despues;
        if quitados & PODER_ATAJOS != 0 {
            for i in 0..self.cx.lista.len() {
                if self.cx.lista[i].cliente == cliente {
                    let objeto = self.cx.lista[i].objeto;
                    let numeros: Vec<u32> = self.cx.lista[i].atajos.iter().map(|a| a.numero).collect();
                    self.cx.lista[i].atajos.clear();
                    for n in numeros {
                        self.a_exchanges(i, |o, id| Aviso::XAtajo { objeto: o, id, numero: n, combinacion: String::new(), estado: 0 });
                    }
                    self.cx.avisos.push((cliente, Aviso::Retirado { objeto, permiso: PODER_ATAJOS }));
                }
            }
        }
        if quitados & PODER_FONDO != 0 {
            for c in self.cx.lista.iter_mut().filter(|c| c.cliente == cliente) {
                c.plazo = None;
                c.sin_bloq_mayus = false;
                self.cx.avisos.push((cliente, Aviso::Retirado { objeto: c.objeto, permiso: PODER_FONDO }));
            }
        }
        if quitados & PODER_COMMODITIES != 0 {
            self.cx.exchanges.retain(|e| e.0 != cliente);
        }
        despues
    }

    /// Manda a cada Exchange de la persona de la commodity `i` el aviso que hace `f` (con su objeto y el número de la
    /// commodity).
    fn a_exchanges(&mut self, i: usize, f: impl Fn(u32, u32) -> Aviso) {
        let (cl, id) = (self.cx.lista[i].cliente, self.cx.lista[i].id);
        let p = self.persona(cl);
        let suyos: Vec<(Cliente, u32)> = self.cx.exchanges.iter().copied().filter(|e| self.persona(e.0) == p).collect();
        for (ec, eo) in suyos {
            self.cx.avisos.push((ec, f(eo, id)));
        }
    }

    /// El aviso de una commodity para Exchange.
    fn aviso_commodity(&self, i: usize, objeto: u32) -> Aviso {
        let c = &self.cx.lista[i];
        Aviso::XCommodity {
            objeto,
            id: c.id,
            nombre: c.nombre.clone(),
            titulo: c.titulo.clone(),
            descripcion: c.descripcion.clone(),
            programa: self.cx.programas.get(&c.cliente).cloned().unwrap_or_default(),
            activa: c.activa,
        }
    }

    /// Un cliente se va: sus commodities (sus atajos quedan libres) y sus Exchange.
    pub(super) fn commodities_adios(&mut self, cliente: Cliente) {
        while let Some(i) = self.cx.lista.iter().position(|c| c.cliente == cliente) {
            self.a_exchanges(i, |o, id| Aviso::XFuera { objeto: o, id });
            let c = self.cx.lista.remove(i);
            self.cx.registro.push(alloc::format!("commodity {} se va con {} atajos", c.nombre, c.atajos.iter().filter(|a| a.estado == 0).count()));
        }
        self.cx.exchanges.retain(|e| e.0 != cliente);
        self.cx.poderes.remove(&cliente);
        self.cx.programas.remove(&cliente);
        self.cx.avisos.retain(|a| a.0 != cliente);
        if self.cx.ultimo_clic.is_some_and(|u| u.0.0 == cliente) {
            self.cx.ultimo_clic = None;
        }
    }

    /// Apaga la pantalla (Blanker) hasta la siguiente entrada.
    pub fn oscurecer(&mut self) {
        if !self.cx.oscuro {
            self.cx.oscuro = true;
            self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        }
    }

    /// La enciende otra vez.
    pub fn despertar(&mut self) {
        if self.cx.oscuro {
            self.cx.oscuro = false;
            self.danos.push(Rect { x: 0, y: 0, w: i32::MAX / 2, h: i32::MAX / 2 });
        }
    }

    /// Si Bloq Mayús no llega a nadie: alguna commodity activa de quien está delante lo pidió.
    fn sin_bloq_mayus(&self) -> bool {
        self.cx.lista.iter().any(|c| c.sin_bloq_mayus && c.activa && !self.oculto(c.cliente))
    }

    /// Una tecla, antes que nada (desde `tecla`): `true` si no llega a nadie. Lleva los modificadores por lados,
    /// despierta la pantalla, se traga Bloq Mayús (NoCapsLock) y busca un atajo.
    pub(super) fn commodities_tecla(&mut self, uso: u8, codigo: u32, pulsada: bool) -> bool {
        let modificador = (0xE0..=0xE7).contains(&uso);
        if modificador {
            let bit = 1u8 << (uso - 0xE0);
            if pulsada { self.cx.lados |= bit } else { self.cx.lados &= !bit }
        }
        if self.cx.oscuro && pulsada {
            self.despertar();
            if !modificador {
                if self.cx.tragadas.len() < MAX_TRAGADAS {
                    self.cx.tragadas.push(codigo);
                }
                return true;
            }
        }
        if !pulsada && let Some(i) = self.cx.tragadas.iter().position(|&t| t == codigo) {
            self.cx.tragadas.remove(i);
            return true;
        }
        if codigo == KEY_BLOQ_MAYUS && self.sin_bloq_mayus() {
            return true;
        }
        if !pulsada || modificador || codigo == 0 || self.bloqueo.is_some() || self.teclado_del_sistema() {
            return false;
        }
        let pulsada_ = Combinacion { mods: atajos::mods_de_lados(self.cx.lados), tecla: codigo };
        if pulsada_.comprobar().is_err() {
            return false;
        }
        let hallado = self.cx.lista.iter().find_map(|c| {
            (c.activa && !self.oculto(c.cliente) && self.cx.poderes_de(c.cliente) & PODER_ATAJOS != 0)
                .then(|| c.atajos.iter().find(|a| a.estado == 0 && a.combinacion == Some(pulsada_)).map(|a| (c.cliente, c.objeto, a.numero)))
                .flatten()
        });
        let Some((cl, objeto, numero)) = hallado else { return false };
        self.cx.avisos.push((cl, Aviso::Pulsado { objeto, numero }));
        if self.cx.tragadas.len() < MAX_TRAGADAS {
            self.cx.tragadas.push(codigo);
        }
        true
    }

    /// Los botones del ratón, antes que nada (desde `raton`): los que no llegan a nadie, fuera. Con la pantalla
    /// apagada, un botón la despierta y no llega (hasta soltarlo).
    pub(super) fn commodities_botones(&mut self, botones: u8) -> u8 {
        let nuevos = botones & !self.cx.botones_crudos;
        self.cx.botones_crudos = botones;
        if self.cx.oscuro {
            self.despertar();
            self.cx.botones_tragados |= nuevos;
        }
        self.cx.botones_tragados &= botones;
        botones & !self.cx.botones_tragados
    }

    /// AutoPoint: el puntero entra en otra superficie, sin botones. Si es una ventana (no una capa ni un emergente), se
    /// lleva el teclado, sin subir. Nunca se lo quita a una ventana del sistema ni con la sesión bloqueada.
    pub(super) fn autopoint(&mut self, (c, sup): (Cliente, u32), out: &mut Vec<(Cliente, Entrada)>) {
        if !self.cx.autopoint || self.bloqueo.is_some() || self.teclado_del_sistema() {
            return;
        }
        let Some(cli) = self.clientes.get(&c) else { return };
        let r = raiz(cli, sup);
        let ventana = cli
            .superficies
            .get(&r)
            .filter(|s| s.capa.is_none())
            .and_then(|s| s.rol)
            .and_then(|rol| cli.xdg.get(&rol))
            .is_some_and(|x| x.toplevel.is_some());
        if ventana && self.teclado != Some((c, r)) && self.orden.contains(&(c, r)) {
            self.dar_teclado((c, r), out);
        }
    }

    /// Un clic en una ventana: con ClickToFront de serie, la sube y le da el teclado; con "doble clic", el clic solo
    /// le da el teclado, y el segundo seguido (con el botón izquierdo) la sube.
    pub(super) fn pulsar_ventana(&mut self, v: (Cliente, u32), izquierdo: bool, out: &mut Vec<(Cliente, Entrada)>) {
        if !self.cx.clic_doble {
            self.activar(v, out);
            return;
        }
        self.dar_teclado(v, out);
        if !izquierdo {
            return;
        }
        let ahora = self.reloj_ms;
        if self.cx.ultimo_clic.is_some_and(|(u, t)| u == v && ahora.wrapping_sub(t) < DOBLE_CLIC_MS) {
            self.subir(v);
            self.cx.ultimo_clic = None;
        } else {
            self.cx.ultimo_clic = Some((v, ahora));
        }
    }

    /// Hubo entrada: los plazos de inactividad vuelven a contar; a las que ya avisaron, `inactivo(0)`.
    pub(super) fn commodities_actividad(&mut self) {
        let ahora = self.reloj_ms;
        for c in &mut self.cx.lista {
            if let Some(p) = c.plazo.as_mut() {
                p.desde_ms = ahora;
                if core::mem::take(&mut p.avisado) {
                    self.cx.avisos.push((c.cliente, Aviso::Inactivo { objeto: c.objeto, si: false }));
                }
            }
        }
    }

    /// Los plazos de inactividad que vencen: `inactivo(1)`. Lo que falta para el siguiente.
    pub(super) fn commodities_inactividad(&mut self) -> Option<u32> {
        let ahora = self.reloj_ms;
        let mut falta: Option<u32> = None;
        for c in &mut self.cx.lista {
            let Some(p) = c.plazo.as_mut().filter(|p| !p.avisado) else { continue };
            if !c.activa {
                continue;
            }
            let pasado = ahora.wrapping_sub(p.desde_ms);
            if pasado >= p.ms {
                p.avisado = true;
                self.cx.avisos.push((c.cliente, Aviso::Inactivo { objeto: c.objeto, si: true }));
            } else {
                let f = p.ms - pasado;
                falta = Some(falta.map_or(f, |x| x.min(f)));
            }
        }
        falta
    }
}

/// Un texto de un argumento, con su tope.
fn texto_con_tope(v: &Valor, max: usize, objeto: u32) -> Result<String, ErrorProtocolo> {
    let t = v.texto();
    if t.len() > max {
        return Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "texto demasiado largo"));
    }
    Ok(t.into())
}

impl<M: Memorias> PorCliente<'_, M> {
    /// Las peticiones de las tres interfaces. `None` si no son suyas.
    pub(super) fn commodities(&mut self, con: &mut Conexion, interfaz: &str, objeto: u32, orden: u16, args: &[Valor]) -> Option<Result<(), ErrorProtocolo>> {
        let r = match interfaz {
            "dashos_commodities_v1" => self.gestor(con, objeto, orden, args),
            "dashos_commodity_v1" => self.commodity(con, objeto, orden, args),
            "dashos_exchange_v1" => self.exchange(objeto, orden, args),
            _ => return None,
        };
        Some(r)
    }

    fn sin_permiso(&self, bits: u32, objeto: u32, que: &str) -> Result<(), ErrorProtocolo> {
        if self.escena.cx.poderes_de(self.cliente) & bits == 0 {
            return Err(ErrorProtocolo::new(objeto, ERROR_SIN_PERMISO, que));
        }
        Ok(())
    }

    fn gestor(&mut self, con: &mut Conexion, objeto: u32, orden: u16, args: &[Valor]) -> Result<(), ErrorProtocolo> {
        let cliente = self.cliente;
        match orden {
            dashos_commodities_v1::req::REGISTRAR => {
                self.sin_permiso(PODER_ATAJOS | PODER_FONDO, objeto, "una commodity necesita el permiso de atajos o el de fondo")?;
                let escena = &mut *self.escena;
                if escena.cx.lista.iter().filter(|c| c.cliente == cliente).count() >= MAX_COMMODITIES_PROGRAMA {
                    return Err(ErrorProtocolo::new(objeto, ERROR_DEMASIADOS, "demasiadas commodities"));
                }
                let nombre = texto_con_tope(&args[1], MAX_NOMBRE, objeto)?;
                let titulo = texto_con_tope(&args[2], MAX_TITULO, objeto)?;
                let descripcion = texto_con_tope(&args[3], MAX_DESCRIPCION, objeto)?;
                if nombre.is_empty() || nombre.chars().any(|c| c.is_control()) {
                    return Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "nombre de commodity que no vale"));
                }
                escena.cx.siguiente = escena.cx.siguiente.wrapping_add(1).max(1);
                let id = escena.cx.siguiente;
                escena.cx.registro.push(alloc::format!("commodity {nombre} ({id})"));
                escena.cx.lista.push(Commodity {
                    id,
                    cliente,
                    objeto: args[0].uint(),
                    nombre,
                    titulo,
                    descripcion,
                    activa: true,
                    atajos: Vec::new(),
                    plazo: None,
                    sin_bloq_mayus: false,
                    dicho_demasiados: false,
                });
                let i = escena.cx.lista.len() - 1;
                let p = escena.persona(cliente);
                let suyos: Vec<(Cliente, u32)> = escena.cx.exchanges.iter().copied().filter(|e| escena.persona(e.0) == p).collect();
                for (ec, eo) in suyos {
                    let a = escena.aviso_commodity(i, eo);
                    escena.cx.avisos.push((ec, a));
                }
            }
            dashos_commodities_v1::req::VIGILAR => {
                self.sin_permiso(PODER_COMMODITIES, objeto, "ver las commodities necesita el permiso de commodities")?;
                let escena = &mut *self.escena;
                if escena.cx.exchanges.len() >= MAX_EXCHANGE {
                    return Err(ErrorProtocolo::new(objeto, ERROR_DEMASIADOS, "demasiados Exchange"));
                }
                let eo = args[0].uint();
                escena.cx.exchanges.push((cliente, eo));
                // La lista de ahora, sus atajos y los ajustes; y `listo`.
                let p = escena.persona(cliente);
                for i in 0..escena.cx.lista.len() {
                    if escena.persona(escena.cx.lista[i].cliente) != p {
                        continue;
                    }
                    escribir_aviso(con, &escena.aviso_commodity(i, eo));
                    let c = &escena.cx.lista[i];
                    for a in &c.atajos {
                        escribir_aviso(con, &Aviso::XAtajo { objeto: eo, id: c.id, numero: a.numero, combinacion: a.texto.clone(), estado: a.estado });
                    }
                }
                escribir_aviso(con, &Aviso::XAjuste { objeto: eo, cual: AJUSTE_CLICKTOFRONT, valor: escena.cx.clic_doble as u32 });
                escribir_aviso(con, &Aviso::XAjuste { objeto: eo, cual: AJUSTE_AUTOPOINT, valor: escena.cx.autopoint as u32 });
                escribir_aviso(con, &Aviso::XListo { objeto: eo });
            }
            _ => {}
        }
        Ok(())
    }

    fn commodity(&mut self, con: &mut Conexion, objeto: u32, orden: u16, args: &[Valor]) -> Result<(), ErrorProtocolo> {
        let cliente = self.cliente;
        let Some(i) = self.escena.cx.lista.iter().position(|c| c.cliente == cliente && c.objeto == objeto) else { return Ok(()) };
        match orden {
            dashos_commodity_v1::req::DESTROY => {
                let escena = &mut *self.escena;
                escena.a_exchanges(i, |o, id| Aviso::XFuera { objeto: o, id });
                let c = escena.cx.lista.remove(i);
                escena.cx.registro.push(alloc::format!("commodity {} se da de baja", c.nombre));
            }
            dashos_commodity_v1::req::ATAJO => {
                self.sin_permiso(PODER_ATAJOS, objeto, "pedir un atajo necesita el permiso de atajos")?;
                let numero = args[0].uint();
                let texto_pedido = args[1].texto();
                if texto_pedido.len() > atajos::MAX_TEXTO {
                    return Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "combinación demasiado larga"));
                }
                let escena = &mut *self.escena;
                let p = escena.persona(cliente);
                let mut quien = String::new();
                let resultado = atajos::entender(texto_pedido).and_then(|comb| {
                    // Los suyos (sin el que se cambia) y los de todos.
                    let suyos = escena.cx.lista.iter().filter(|c| c.cliente == cliente).flat_map(|c| c.atajos.iter().filter(|a| a.estado == 0 && !(c.objeto == objeto && a.numero == numero))).count();
                    let todos = escena.cx.lista.iter().flat_map(|c| c.atajos.iter().filter(|a| a.estado == 0)).count();
                    if suyos >= MAX_ATAJOS_PROGRAMA || todos >= MAX_ATAJOS {
                        return Err(Motivo::Demasiados);
                    }
                    let otra = escena.cx.lista.iter().find(|c| {
                        escena.persona(c.cliente) == p
                            && c.atajos.iter().any(|a| a.estado == 0 && a.combinacion == Some(comb) && !(c.cliente == cliente && c.objeto == objeto && a.numero == numero))
                    });
                    if let Some(o) = otra {
                        quien = o.nombre.clone();
                        return Err(Motivo::Ocupado);
                    }
                    Ok(comb)
                });
                let c = &mut escena.cx.lista[i];
                let (estado, combinacion, texto) = match resultado {
                    Ok(comb) => (0, Some(comb), comb.escribir()),
                    Err(m) => (m as u32, None, String::from(texto_pedido)),
                };
                let nuevo = Atajo { numero, combinacion, texto: texto.clone(), estado };
                match c.atajos.iter().position(|a| a.numero == numero) {
                    Some(j) => c.atajos[j] = nuevo,
                    None if c.atajos.len() < MAX_APUNTADOS => c.atajos.push(nuevo),
                    // Sin sitio ni para apuntarlo: se niega y no se guarda.
                    None => {}
                }
                let nombre = c.nombre.clone();
                let callar = estado == Motivo::Demasiados as u32 && core::mem::replace(&mut c.dicho_demasiados, true);
                if estado == 0 {
                    escena.cx.registro.push(alloc::format!("atajo {texto} para {nombre}"));
                } else if callar {
                    escribir_aviso(con, &Aviso::Negado { objeto, numero, motivo: estado, quien });
                } else {
                    let m = Motivo::de_numero(estado).map_or("", |m| m.texto());
                    escena.cx.registro.push(alloc::format!("atajo {:?} para {nombre}: no ({m}{}{})", texto, if quien.is_empty() { "" } else { ": " }, quien));
                    escribir_aviso(con, &Aviso::Negado { objeto, numero, motivo: estado, quien });
                }
                escena.a_exchanges(i, |o, id| Aviso::XAtajo { objeto: o, id, numero, combinacion: texto.clone(), estado });
            }
            dashos_commodity_v1::req::QUITAR_ATAJO => {
                self.sin_permiso(PODER_ATAJOS, objeto, "quitar un atajo necesita el permiso de atajos")?;
                let numero = args[0].uint();
                let escena = &mut *self.escena;
                let c = &mut escena.cx.lista[i];
                let antes = c.atajos.len();
                c.atajos.retain(|a| a.numero != numero);
                if c.atajos.len() != antes {
                    escena.a_exchanges(i, |o, id| Aviso::XAtajo { objeto: o, id, numero, combinacion: String::new(), estado: 0 });
                }
            }
            dashos_commodity_v1::req::INACTIVIDAD => {
                self.sin_permiso(PODER_FONDO, objeto, "la inactividad necesita el permiso de fondo")?;
                let s = args[0].uint();
                let escena = &mut *self.escena;
                let desde_ms = escena.reloj_ms;
                escena.cx.lista[i].plazo = (s != 0).then(|| Plazo { ms: s.clamp(MIN_INACTIVIDAD_S, MAX_INACTIVIDAD_S) * 1000, desde_ms, avisado: false });
            }
            dashos_commodity_v1::req::OSCURECER => {
                self.sin_permiso(PODER_FONDO, objeto, "apagar la pantalla necesita el permiso de fondo")?;
                let escena = &mut *self.escena;
                let c = &escena.cx.lista[i];
                if c.activa && !escena.oculto(cliente) && c.plazo.is_some_and(|p| p.avisado) {
                    let nombre = c.nombre.clone();
                    escena.cx.registro.push(alloc::format!("{nombre} apaga la pantalla"));
                    escena.oscurecer();
                } else {
                    let nombre = c.nombre.clone();
                    escena.cx.registro.push(alloc::format!("{nombre} quiere apagar la pantalla sin estar inactiva: no"));
                }
            }
            dashos_commodity_v1::req::COMPORTAMIENTO => {
                self.sin_permiso(PODER_FONDO, objeto, "un comportamiento necesita el permiso de fondo")?;
                if args[0].uint() != SIN_BLOQ_MAYUS {
                    return Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "comportamiento que no existe"));
                }
                self.escena.cx.lista[i].sin_bloq_mayus = args[1].uint() != 0;
            }
            _ => {}
        }
        Ok(())
    }

    fn exchange(&mut self, objeto: u32, orden: u16, args: &[Valor]) -> Result<(), ErrorProtocolo> {
        let cliente = self.cliente;
        let escena = &mut *self.escena;
        if orden == dashos_exchange_v1::req::DESTROY {
            escena.cx.exchanges.retain(|e| *e != (cliente, objeto));
            return Ok(());
        }
        // Uno al que se le quitó el permiso ya no está en la lista: lo que pida no hace nada.
        if !escena.cx.exchanges.contains(&(cliente, objeto)) {
            return Ok(());
        }
        if orden == dashos_exchange_v1::req::AJUSTE {
            let (cual, valor) = (args[0].uint(), (args[1].uint() != 0) as u32);
            match cual {
                AJUSTE_CLICKTOFRONT => escena.cx.clic_doble = valor != 0,
                AJUSTE_AUTOPOINT => escena.cx.autopoint = valor != 0,
                _ => return Err(ErrorProtocolo::new(objeto, ERROR_NO_VALE, "ajuste que no existe")),
            }
            escena.cx.registro.push(alloc::format!("ajuste {} {}", if cual == AJUSTE_CLICKTOFRONT { "ClickToFront" } else { "AutoPoint" }, valor));
            let todos = escena.cx.exchanges.clone();
            for (ec, eo) in todos {
                escena.cx.avisos.push((ec, Aviso::XAjuste { objeto: eo, cual, valor }));
            }
            return Ok(());
        }
        // Lo demás, sobre una commodity de su misma persona; un número que ya no está no hace nada.
        let id = args[0].uint();
        let p = escena.persona(cliente);
        let Some(i) = escena.cx.lista.iter().position(|c| c.id == id && escena.persona(c.cliente) == p) else { return Ok(()) };
        let (cl, co) = (escena.cx.lista[i].cliente, escena.cx.lista[i].objeto);
        match orden {
            dashos_exchange_v1::req::ACTIVAR => {
                let si = args[1].uint() != 0;
                let c = &mut escena.cx.lista[i];
                if c.activa != si {
                    c.activa = si;
                    // Al volver, la inactividad cuenta desde ahora.
                    let ahora = escena.reloj_ms;
                    if let Some(pl) = c.plazo.as_mut() {
                        pl.desde_ms = ahora;
                        pl.avisado = false;
                    }
                    let nombre = c.nombre.clone();
                    escena.cx.registro.push(alloc::format!("commodity {nombre} {}", if si { "activada" } else { "desactivada" }));
                    escena.cx.avisos.push((cl, Aviso::Activo { objeto: co, si }));
                    let suyos: Vec<(Cliente, u32)> = escena.cx.exchanges.iter().copied().filter(|e| escena.persona(e.0) == p).collect();
                    for (ec, eo) in suyos {
                        let a = escena.aviso_commodity(i, eo);
                        escena.cx.avisos.push((ec, a));
                    }
                }
            }
            dashos_exchange_v1::req::ENSENAR => escena.cx.avisos.push((cl, Aviso::Ensenar { objeto: co })),
            dashos_exchange_v1::req::QUITAR => {
                let nombre = escena.cx.lista[i].nombre.clone();
                escena.cx.registro.push(alloc::format!("se pide a {nombre} que se vaya"));
                escena.cx.avisos.push((cl, Aviso::Quitar { objeto: co }));
            }
            _ => {}
        }
        Ok(())
    }
}
