//! El servicio de sonido (S19; desde `sonido-pro`, el grafo de S19b, fase E1): los flujos de los programas son nodos de
//! un grafo que acaba en las salidas, y el servicio lo corre al ritmo del aparato.
//!
//! Por el canal "sonido" le llega todo (un proceso solo espera en un canal): los mensajes de `inicio` (sin marca: qué
//! marca es de quién), los del controlador y los de los programas, cada uno con su marca. El protocolo, en
//! `docs/referencia/servicios.md` y `usuario/src/protocolo/sonido.rs`:
//!
//! - El controlador (solo el que `inicio` llama "virtio-sonido") manda `SALIDA` con una memoria compartida: cabecera
//!   (escritos, leídos, ranuras, cuadros por período) y las ranuras desde el byte 64, cada una un período en estéreo de
//!   16 bits. Después, un `TIC` cada vez que el aparato devuelve un búfer: el reloj del hardware manda (S19). Con cada
//!   tic, el servicio corre el grafo hasta tener los períodos de delante listos.
//! - El grafo de cada salida: los flujos → la mezcla de la salida (su volumen es el de la salida) → ecualizador →
//!   espacial → limitador → la salida. El limitador va el último: lo que sale nunca satura.
//! - Un programa manda `ABRIR` (formato) con la memoria de su anillo y el extremo de escritura de un canal suyo
//!   (`usuario/src/sonido_cliente.rs`); `VOLUMEN` y `CERRAR`. Hasta `FLUJOS_POR_PROGRAMA` flujos por programa (las
//!   cajas abren varios), cada uno con su índice (el orden en que los abrió).
//! - El mezclador (el panel, la ventana "Sonido", Ajustes: por el nombre que da `inicio`) cambia el volumen, el
//!   silencio y la salida de cada flujo, la salida del sistema, los efectos y el período. `GENERAL` (f32) sigue siendo
//!   el volumen de la salida del sistema (del panel).
//! - Quien enseña el sonido (el mezclador, la barra, el dock, la pantalla de bloqueo) se suscribe al estado (una
//!   memoria suya, `SUSCRIBIR`) que el servicio rellena unas 30 veces por segundo: el grafo con sus niveles (los
//!   vúmetros), los efectos, lo que suena ahora (`MEDIOS`) y el espectro, este solo si alguien lo mira.
//! - Los programas dicen lo que suena (`MEDIOS`); el mezclador les manda órdenes (`MEDIOS_ORDEN`) por el servicio.
//! - Los sonidos del sistema (`SONAR`) se sintetizan aquí (`sonido::sonidos`) y suenan por su propio flujo.
//!
//! U6, el cambio rápido de usuario: `sesiones` manda, sin marca, de qué persona es cada programa y qué sesiones están
//! detrás. Un flujo de una sesión de detrás sigue leyéndose a su ritmo, pero suena con volumen 0. Al volver delante,
//! otra vez con su volumen.
//!
//! Cuando el anillo de un programa baja de la mitad, se le despierta por su canal. Si el canal está cerrado, el
//! programa murió: su flujo sale con el fundido del grafo (A5).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};
use dashos::println;
#[path = "../sonido_estado.rs"]
mod sonido_estado;
/// El protocolo del sonido: las constantes (de la biblioteca) y el estado con textos y listas.
mod p {
    pub use crate::sonido_estado::*;
    pub use dashos::protocolo::sonido::*;
}
use sonido::efectos;
use sonido::espectro::Analizador;
use sonido::grafo::{Clase, Grafo, Id};
use sonido::sonidos::{Evento, Sintetizador};
use sonido::{Cuadro, Formato, Muestra, UNO_VOL};

dashos::programa!(principal);
dashos::monton!();

/// Quién puede cambiar el volumen general, por el nombre que da `inicio`.
const PONEN_GENERAL: &[&str] = &["panel", "mezclador", "centro"];
/// Quién puede tocar el grafo de los demás (volumen, silencio y salida de cada flujo; efectos; período).
const MEZCLAN: &[&str] = &["panel", "mezclador", "centro"];
/// Quién puede suscribirse al estado (ve los nombres de lo que suena y sus niveles).
const LEEN: &[&str] = &["panel", "mezclador", "centro", "barra", "dock", "guardian"];
/// Quién puede pedir sonidos del sistema.
const SUENAN: &[&str] = &["panel", "mezclador", "centro", "barra", "dock", "guardian", "avisos", "sesiones"];
/// Quién puede pedir que se abra la ventana "Sonido".
const ABREN: &[&str] = &["panel", "centro", "barra", "dock"];
const E_CERRADO: i64 = -4;

const VA_SALIDA: usize = 0x40_0000_0000;
const VA_ANILLOS: usize = 0x41_0000_0000;
const VA_ESTADOS: usize = 0x42_0000_0000;
/// Sitio de cada anillo: 1 MB. Y de cada estado, 64 KB.
const HUECO: usize = 1 << 20;
const HUECO_ESTADO: usize = 1 << 16;
const DATOS: usize = 64;
/// Lo más que abre un programa (las cajas: los cuatro canales de Paula y algo más), y en total.
const FLUJOS_POR_PROGRAMA: usize = 8;
const MAX_FLUJOS: usize = 64;
const MAX_SUSCRIPTORES: usize = 8;
/// Cuadros que se procuran tener en la cola de cada flujo antes de correr el grafo.
const RESERVA: usize = 1024;
/// Cada cuántos tics se rellena el estado (6 × 5,3 ms: unas 30 veces por segundo).
const CADA_ESTADO: u32 = 6;
/// Tras cuánto sin latido se deja de calcular el espectro.
const LATIDO_US: u64 = 600_000;

/// Períodos mezclados por delante del controlador, por modo (normal, para tocar, ahorro).
const ADELANTE: [u32; 3] = [4, 2, 8];

fn palabra(va: usize, i: usize) -> u32 {
    unsafe { read_volatile((va + 4 * i) as *const u32) }
}

fn poner(va: usize, i: usize, v: u32) {
    unsafe { write_volatile((va + 4 * i) as *mut u32, v) }
}

struct Cliente {
    /// El nombre para los mensajes (el de `inicio` o el del fichero que lanzó `sesiones`).
    nombre: String,
    /// El volumen que pidió el programa y el que puso el mezclador: suena el producto.
    volumen: (f32, f32),
    mezcla: [i64; 2],
    /// U6: en silencio porque su sesión está detrás, y lo que se leyó de su anillo así (bytes).
    silencio: bool,
    leido_en_silencio: u64,
    nodo: Id,
    va: usize,
    tam: u32,
    aviso: u32,
    memoria: u32,
    avisado: bool,
    ultima: u32,
    cerrando: bool,
    faltas: u64,
}

/// Una salida de hardware y su cadena.
struct Salida {
    nombre: String,
    nodo: Id,
    mezcla: Id,
    eq: Id,
    espacial: Id,
    limitador: Id,
    ranuras: u32,
    cuadros: u32,
    /// Cuándo se escribió cada ranura (para medir la latencia: hasta que el aparato la devuelve).
    escrita_us: Vec<u64>,
    leidos_vistos: u32,
}

struct Suscriptor {
    marca: u64,
    nombre: String,
    va: usize,
    memoria: u32,
    aviso: u32,
    latido: u32,
    latido_us: u64,
    ventana_dicha: u32,
}

struct MedioP {
    marca: u64,
    datos: p::Medio,
    orden: u32,
}

/// Lo que hay en el anillo (sin fiarse de la cabecera, que escribe el programa).
fn disponibles(c: &Cliente) -> u32 {
    palabra(c.va, 0).wrapping_sub(palabra(c.va, 1)).min(c.tam)
}

/// Pasa al grafo lo que haga falta del anillo del programa.
fn recoger(g: &mut Grafo, c: &mut Cliente, tmp: &mut Vec<u8>) {
    if g.pendientes(c.nodo).unwrap_or(usize::MAX) >= RESERVA {
        return;
    }
    let n = disponibles(c).min(4096) as usize;
    if n == 0 {
        return;
    }
    fence(Ordering::SeqCst);
    let cola = palabra(c.va, 1);
    // En dos trozos (el anillo da la vuelta), de una vez: tras la valla, lo que hay entre la cola y la cabeza ya está
    // escrito (antes se copiaba byte a byte con lecturas volátiles: era lo que más costaba de un tic).
    let ini = cola as usize & (c.tam as usize - 1);
    let primero = n.min(c.tam as usize - ini);
    tmp.clear();
    // SAFETY: el anillo está proyectado en `c.va` con `DATOS + tam` bytes (se comprobó al abrirlo) y los dos trozos
    // caen dentro; el programa solo escribe fuera de lo disponible.
    unsafe {
        tmp.extend_from_slice(core::slice::from_raw_parts((c.va + DATOS + ini) as *const u8, primero));
        tmp.extend_from_slice(core::slice::from_raw_parts((c.va + DATOS) as *const u8, n - primero));
    }
    let usados = g.escribir(c.nodo, tmp);
    poner(c.va, 1, cola.wrapping_add(usados as u32));
    if c.silencio {
        c.leido_en_silencio += usados as u64;
    }
}

/// La clase de plazo (S2, A6): hasta 2 ms de CPU cada período de 5,3 ms. Antes pedía 3 (el 56 %), y con el
/// repartidor del núcleo ya en la clase de plazo (30 %) pasaba del 80 % que admite un núcleo: el kernel lo negaba y el
/// servicio iba en la justa ("sin clase de plazo"), con vacíos en la salida cuando había carga (se vio en `revision`
/// también). Con el grafo, un período cuesta de media unos 0,2 ms en QEMU: 2 ms sobran.
const PLAZO_US: (u64, u64) = (2_000, 5_333);

fn a_fijo(v: f32) -> i64 {
    sonido::a_volumen(v)
}

/// El volumen que suena de un cliente: el suyo por el del mezclador, o 0 si su sesión está detrás.
fn aplicar_volumen(g: &mut Grafo, c: &Cliente) {
    let propio = [a_fijo(c.volumen.0), a_fijo(c.volumen.1)];
    let v = if c.silencio { [0, 0] } else { [0, 1].map(|k| (propio[k] * c.mezcla[k]) >> 16) };
    let _ = g.volumen_fijo(c.nodo, v);
}

/// U6: pone cada flujo en silencio o con su volumen según su sesión esté detrás o no, y lo dice si cambia.
fn seguir_sesiones(g: &mut Grafo, clientes: &mut BTreeMap<(u64, u8), Cliente>, personas: &BTreeMap<u64, u32>, detras: &[u32]) {
    for ((mc, _), c) in clientes.iter_mut() {
        let silencio = personas.get(mc).is_some_and(|u| detras.contains(u));
        if silencio == c.silencio || c.cerrando {
            continue;
        }
        c.silencio = silencio;
        aplicar_volumen(g, c);
        if silencio {
            c.leido_en_silencio = 0;
            println!("  [sonido] {}: en silencio, su sesión está detrás", c.nombre);
        } else {
            println!("  [sonido] {}: suena otra vez; en silencio se leyeron {} KB de su anillo", c.nombre, c.leido_en_silencio / 1024);
        }
    }
}

/// Los ajustes del servicio que se ven en el estado.
struct Ajustes {
    defecto: Option<usize>,
    periodo_modo: u32,
    eq_preajuste: i32,
    eq_activo: bool,
    limitador: bool,
    espacial: (u32, u32),
    tema: u32,
    silenciados: bool,
    no_molestar: bool,
    volumen_sonidos: u32,
    ventana: u32,
}

struct Servicio {
    g: Grafo,
    clientes: BTreeMap<(u64, u8), Cliente>,
    salidas: Vec<Salida>,
    subs: Vec<Suscriptor>,
    medios: Vec<MedioP>,
    siguiente_medio: u32,
    ajustes: Ajustes,
    huecos: u64,
    huecos_estado: u64,
    analizador: Analizador,
    sintetizador: Option<Sintetizador>,
    sistema: Option<Id>,
    /// E9: lo que cuesta correr el grafo por período (µs): suma, cuántos y el peor.
    mezcla_us: u64,
    periodos: u64,
    peor_us: u64,
    /// La latencia medida de la salida (µs): suma, cuántas y la peor.
    latencia: (u64, u64, u64),
    imagen: Vec<u32>,
    tics: u32,
    /// Los avisos de "tope" ya dichos, por marca (para no llenar el registro).
    topes_dichos: Vec<u64>,
}

impl Servicio {
    /// Mueve un flujo a la mezcla de otra salida (fundido cruzado).
    fn mover(&mut self, nodo: Id, a: usize) {
        for d in self.g.destinos(nodo) {
            let _ = self.g.desconectar(nodo, d);
        }
        let _ = self.g.conectar(nodo, self.salidas[a].mezcla);
    }

    /// Un nodo nuevo de un programa, conectado a la salida del sistema.
    fn conectar_a_defecto(&mut self, nodo: Id) {
        if let Some(d) = self.ajustes.defecto {
            let _ = self.g.conectar(nodo, self.salidas[d].mezcla);
        }
    }

    fn adelante(&self) -> u32 {
        ADELANTE[self.ajustes.periodo_modo as usize % 3]
    }

    fn poner_periodo(&mut self, modo: u32) {
        let modo = modo.min(2);
        self.ajustes.periodo_modo = modo;
        let cuadros = p::CUADROS_PERIODO[modo as usize] as usize;
        // El cuanto del grafo: el período del modo, sin pasar del de la salida (que lo fija su aparato).
        let tope = self.salidas.iter().map(|s| s.cuadros as usize).min().unwrap_or(cuadros);
        self.g.poner_cuanto(cuadros.min(tope));
    }

    fn efecto(&mut self, que: u32, valor: i32) -> bool {
        let Some(d) = self.ajustes.defecto else { return false };
        let (eq, esp, lim) = (self.salidas[d].eq, self.salidas[d].espacial, self.salidas[d].limitador);
        let mut ok = true;
        match que {
            p::EF_EQ_PREAJUSTE => {
                for s in self.salidas.iter().map(|s| s.eq).collect::<Vec<_>>() {
                    ok &= self.g.efecto_de(s).is_some_and(|e| e.poner(efectos::P_PREAJUSTE, valor));
                }
                if ok {
                    self.ajustes.eq_preajuste = valor;
                }
            }
            p::EF_EQ_ACTIVO => {
                self.ajustes.eq_activo = valor != 0;
                let _ = self.g.activar(eq, valor != 0);
            }
            p::EF_LIMITADOR => {
                self.ajustes.limitador = valor != 0;
                let _ = self.g.activar(lim, valor != 0);
            }
            p::EF_LIMITADOR_UMBRAL => ok = self.g.efecto_de(lim).is_some_and(|e| e.poner(0, valor)),
            p::EF_ESPACIAL_MODO => {
                ok = self.g.efecto_de(esp).is_some_and(|e| e.poner(0, valor));
                self.ajustes.espacial.0 = self.g.efecto_de(esp).and_then(|e| e.leer(0)).unwrap_or(0) as u32;
            }
            p::EF_ESPACIAL_CANTIDAD => {
                ok = self.g.efecto_de(esp).is_some_and(|e| e.poner(1, valor));
                self.ajustes.espacial.1 = self.g.efecto_de(esp).and_then(|e| e.leer(1)).unwrap_or(0) as u32;
            }
            q if (p::EF_EQ_BANDA..p::EF_EQ_BANDA + efectos::NUM_BANDAS as u32).contains(&q) => {
                let b = q - p::EF_EQ_BANDA;
                ok = self.g.efecto_de(eq).is_some_and(|e| e.poner(4 * b + 1, valor));
                self.ajustes.eq_preajuste = -1;
            }
            _ => ok = false,
        }
        ok
    }

    /// Un sonido del sistema, por su propio flujo (se abre la primera vez).
    fn sonar(&mut self, e: Evento) {
        let a = &self.ajustes;
        if a.silenciados || a.volumen_sonidos == 0 {
            return;
        }
        // "No molestar": calla todo menos el tic del volumen, que lo pide quien lo está tocando.
        if a.no_molestar && e != Evento::Volumen {
            return;
        }
        let Some(_) = self.ajustes.defecto else { return };
        let fs = self.g.frecuencia();
        let sint = self.sintetizador.get_or_insert_with(|| Sintetizador::nuevo(fs));
        let muestras = sint.generar(self.ajustes.tema, e, self.ajustes.volumen_sonidos);
        if self.sistema.is_none_or(|id| !self.g.existe(id)) {
            let f = Formato { muestra: Muestra::I16, canales: 1, frecuencia: fs };
            match self.g.flujo("sistema", 0, f) {
                Ok(id) => {
                    self.sistema = Some(id);
                    self.conectar_a_defecto(id);
                }
                Err(_) => return,
            }
        }
        let id = self.sistema.unwrap();
        // Ráfagas: si ya hay más de medio segundo esperando, este no suena.
        if self.g.pendientes(id).unwrap_or(0) > fs as usize / 2 {
            return;
        }
        let bytes: Vec<u8> = muestras.iter().flat_map(|m| m.to_le_bytes()).collect();
        self.g.escribir(id, &bytes);
    }

    /// Rellena la imagen del estado y la copia en la memoria de cada suscriptor; les despierta si piden la ventana.
    fn publicar(&mut self, medir: bool) {
        if self.subs.is_empty() {
            return;
        }
        let mut imagen = core::mem::take(&mut self.imagen);
        let w = &mut imagen;
        w.clear();
        w.resize(p::TAM_ESTADO / 4, 0);
        w[p::W_VERSION] = p::VERSION;
        let defecto = self.ajustes.defecto;
        w[p::W_DEFECTO] = defecto.map_or(0, |d| self.salidas[d].nodo);
        w[p::W_CUANTO] = self.g.cuanto() as u32;
        w[p::W_PERIODO_MODO] = self.ajustes.periodo_modo;
        w[p::W_VACIOS] = self.salidas.first().map_or(0, |_| palabra(VA_SALIDA, 4));
        w[p::W_EQ_PREAJUSTE] = self.ajustes.eq_preajuste as u32;
        w[p::W_EQ_ACTIVO] = self.ajustes.eq_activo as u32;
        w[p::W_LIMITADOR] = self.ajustes.limitador as u32;
        w[p::W_ESPACIAL_MODO] = self.ajustes.espacial.0;
        w[p::W_ESPACIAL_CANTIDAD] = self.ajustes.espacial.1;
        w[p::W_TEMA_SONIDOS] = self.ajustes.tema;
        w[p::W_SONIDOS_FLAGS] = self.ajustes.silenciados as u32 | (self.ajustes.no_molestar as u32) << 1;
        w[p::W_SONIDOS_VOLUMEN] = self.ajustes.volumen_sonidos;
        w[p::W_VENTANA] = self.ajustes.ventana;
        if self.periodos > 0 {
            w[p::W_MEZCLA_US] = (self.mezcla_us / self.periodos) as u32;
            w[p::W_MEZCLA_PEOR_US] = self.peor_us as u32;
        }
        if self.latencia.1 > 0 {
            w[p::W_LATENCIA_US] = (self.latencia.0 / self.latencia.1) as u32;
        }
        if let Some(d) = defecto {
            let eq = self.salidas[d].eq;
            if let Some(e) = self.g.efecto_de(eq) {
                for b in 0..efectos::NUM_BANDAS {
                    w[p::W_EQ_BANDAS + b] = e.leer(4 * b as u32 + 1).unwrap_or(0) as u32;
                }
            }
        }
        // El espectro, si alguien lo mira.
        let ahora = dashos::ahora_us();
        let mirando = self.subs.iter().any(|s| ahora.saturating_sub(s.latido_us) < LATIDO_US);
        if mirando && medir {
            let mut bandas = [0u16; 24];
            self.analizador.bandas(&mut bandas);
            w[p::W_BANDAS] = bandas.len() as u32;
            for (i, b) in bandas.iter().enumerate() {
                w[p::W_ESPECTRO + i / 2] |= (*b as u32) << (16 * (i % 2));
            }
            let mut onda = [0i16; p::ONDA];
            self.analizador.onda(&mut onda);
            for (i, o) in onda.iter().enumerate() {
                w[p::W_ONDA + i / 2] |= (*o as u16 as u32) << (16 * (i % 2));
            }
        }
        // Los nodos que se enseñan: salidas, sus mezclas y los flujos (los efectos van en sus palabras propias).
        let mut i = 0;
        for f in self.g.fichas() {
            if i >= p::MAX_NODOS || f.clase == Clase::Efecto {
                continue;
            }
            let medida = if medir { self.g.medida(f.id).unwrap_or_default() } else { Default::default() };
            let (clase, destino, dueno, mut marcas, periodo, nombre) = match f.clase {
                Clase::Salida => {
                    let s = self.salidas.iter().find(|s| s.nodo == f.id);
                    (p::CLASE_SALIDA, 0, p::DUENO_SISTEMA, 0, s.map_or(0, |s| s.cuadros), s.map_or(f.nombre.clone(), |s| s.nombre.clone()))
                }
                Clase::Mezcla => {
                    let s = self.salidas.iter().find(|s| s.mezcla == f.id);
                    (p::CLASE_MEZCLA, s.map_or(0, |s| s.nodo), p::DUENO_SISTEMA, 0, 0, s.map_or(f.nombre.clone(), |s| s.nombre.clone()))
                }
                _ => {
                    let destinos = self.g.destinos(f.id);
                    let destino = self.salidas.iter().find(|s| destinos.contains(&s.mezcla)).map_or(0, |s| s.nodo);
                    let dueno = match f.nombre.as_str() {
                        "caja-amiga" => p::DUENO_AMIGA,
                        "caja-linux" => p::DUENO_LINUX,
                        "sistema" => p::DUENO_SISTEMA,
                        _ => p::DUENO_PROGRAMA,
                    };
                    let detras = self.clientes.values().any(|c| c.nodo == f.id && c.silencio);
                    (p::CLASE_FLUJO, destino, dueno, if detras { p::N_DETRAS } else { 0 }, 0, f.nombre.clone())
                }
            };
            if f.saliendo {
                marcas |= p::N_SALIENDO;
            }
            // El volumen que se enseña de un flujo es el del mezclador (el del programa va aparte).
            let volumen = match self.clientes.values().find(|c| c.nodo == f.id) {
                Some(c) => c.mezcla.map(|x| x as u32),
                None => f.volumen.map(|x| x as u32),
            };
            let n = p::Nodo {
                id: f.id,
                clase,
                destino,
                volumen,
                silencio: f.silencio,
                pico: medida.pico.map(|x| x.max(0) as u32),
                rms: medida.rms.map(|x| x.max(0) as u32),
                dueno,
                marcas,
                periodo,
                nombre,
            };
            p::escribir_nodo(w, i, &n);
            i += 1;
        }
        w[p::W_NODOS] = i as u32;
        for (k, m) in self.medios.iter().take(p::MAX_MEDIOS).enumerate() {
            p::escribir_medio(w, k, &m.datos);
        }
        w[p::W_MEDIOS] = self.medios.len().min(p::MAX_MEDIOS) as u32;
        self.imagen = imagen;
        // A cada suscriptor: secuencia impar, todo, secuencia par.
        let mut fuera = Vec::new();
        for (k, s) in self.subs.iter_mut().enumerate() {
            let seq = palabra(s.va, p::W_SECUENCIA).wrapping_add(1) | 1;
            poner(s.va, p::W_SECUENCIA, seq);
            fence(Ordering::SeqCst);
            // Solo lo usado: la cabecera, los nodos que hay y los medios que hay (no las 4.096 palabras).
            let nodos = p::W_TABLA + self.imagen[p::W_NODOS] as usize * p::PALABRAS_NODO;
            let medios = p::W_TABLA_MEDIOS..p::W_TABLA_MEDIOS + self.imagen[p::W_MEDIOS] as usize * p::PALABRAS_MEDIO;
            for j in (1..nodos).chain(medios) {
                if j != p::W_LATIDO {
                    poner(s.va, j, self.imagen[j]);
                }
            }
            fence(Ordering::SeqCst);
            poner(s.va, p::W_SECUENCIA, seq.wrapping_add(1));
            // El latido lo escribe él: si cambió, está mirando.
            let l = palabra(s.va, p::W_LATIDO);
            if l != s.latido {
                s.latido = l;
                s.latido_us = ahora;
            }
            // Despertar al mezclador si se pidió la ventana (a los demás no les importa: no se les despierta).
            if s.nombre == "mezclador" && s.ventana_dicha != self.ajustes.ventana {
                s.ventana_dicha = self.ajustes.ventana;
                if dashos::canal_escribir(s.aviso, &p::VENTANA.to_le_bytes(), &[]) == E_CERRADO {
                    fuera.push(k);
                }
            }
        }
        for k in fuera.into_iter().rev() {
            let s = self.subs.remove(k);
            println!("  [sonido] {} ya no mira el estado", s.nombre);
            self.soltar_suscriptor(s);
        }
    }

    fn soltar_suscriptor(&mut self, s: Suscriptor) {
        dashos::memoria_quitar(s.va);
        dashos::cerrar(s.memoria);
        dashos::cerrar(s.aviso);
        self.huecos_estado &= !(1 << ((s.va - VA_ESTADOS) / HUECO_ESTADO));
    }

    /// Un programa ya no está: sus medios se olvidan.
    fn olvidar_medios(&mut self, marca: u64) {
        let antes = self.medios.len();
        self.medios.retain(|m| {
            if m.marca == marca {
                if m.orden != 0 {
                    dashos::cerrar(m.orden);
                }
                false
            } else {
                true
            }
        });
        if self.medios.len() != antes {
            self.publicar(false);
        }
    }
}

/// Crea la cadena de una salida en el grafo.
fn nueva_salida(g: &mut Grafo, nombre: &str, ranuras: u32, cuadros: u32) -> Option<Salida> {
    let fs = g.frecuencia();
    let mezcla = g.mezcla(nombre).ok()?;
    let eq = g.efecto("eq", efectos::crear("eq", fs)?).ok()?;
    let espacial = g.efecto("espacial", efectos::crear("espacial", fs)?).ok()?;
    let limitador = g.efecto("limitador", efectos::crear("limitador", fs)?).ok()?;
    let nodo = g.salida(nombre, cuadros as usize).ok()?;
    for (a, b) in [(mezcla, eq), (eq, espacial), (espacial, limitador), (limitador, nodo)] {
        g.conectar(a, b).ok()?;
    }
    Some(Salida { nombre: String::from(nombre), nodo, mezcla, eq, espacial, limitador, ranuras, cuadros, escrita_us: alloc::vec![0; ranuras as usize], leidos_vistos: 0 })
}

fn principal() -> u64 {
    // Llaves: 1 el permiso de la clase de plazo (S3) y 2 el canal "sonido".
    let canal = 2;
    let r = dashos::plazo(1, PLAZO_US.0, PLAZO_US.1);
    if r != 0 {
        println!("  [sonido] sin clase de plazo ({}): va en la justa", r);
    }
    let mut sv = Servicio {
        g: Grafo::nuevo(48_000, 256),
        clientes: BTreeMap::new(),
        salidas: Vec::new(),
        subs: Vec::new(),
        medios: Vec::new(),
        siguiente_medio: 1,
        ajustes: Ajustes {
            defecto: None,
            periodo_modo: p::PERIODO_NORMAL,
            eq_preajuste: 0,
            eq_activo: true,
            limitador: true,
            espacial: (0, 50),
            tema: 0,
            silenciados: false,
            no_molestar: false,
            volumen_sonidos: 70,
            ventana: 0,
        },
        huecos: 0,
        huecos_estado: 0,
        analizador: Analizador::nuevo(48_000),
        sintetizador: None,
        sistema: None,
        mezcla_us: 0,
        periodos: 0,
        peor_us: 0,
        latencia: (0, 0, 0),
        imagen: Vec::new(),
        tics: 0,
        topes_dichos: Vec::new(),
    };
    let mut nombres: BTreeMap<u64, String> = BTreeMap::new();
    // U6: de qué persona es cada programa, sus nombres (los de lo que lanza `sesiones`, solo para los mensajes) y las
    // personas con la sesión detrás.
    let mut personas: BTreeMap<u64, u32> = BTreeMap::new();
    let mut lanzados: BTreeMap<u64, String> = BTreeMap::new();
    let mut detras: Vec<u32> = Vec::new();
    let mut ultimo_tic = 0u64;
    let mut periodo: Vec<Cuadro> = Vec::new();
    let mut tmp: Vec<u8> = Vec::new();
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        let (n, k, marca) = match dashos::servicio::recibir(canal, &mut buf, &mut ll) {
            dashos::servicio::Recibido::Mensaje { n, k, marca } => (n, k, marca),
            dashos::servicio::Recibido::Cerrado => return 0,
            dashos::servicio::Recibido::Nada => continue,
        };
        let llaves: Vec<u32> = ll[..k].to_vec();
        let cerrar_llaves = |ll: &[u32]| ll.iter().for_each(|&l| { dashos::cerrar(l); });
        if marca == 0 {
            use dashos::protocolo::sesiones as s;
            let w = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
            if n == 24 {
                let mm = u64::from_le_bytes(buf[..8].try_into().unwrap());
                let fin = buf[8..24].iter().position(|&b| b == 0).unwrap_or(16);
                nombres.insert(mm, String::from(core::str::from_utf8(&buf[8..8 + fin]).unwrap_or("?")));
            } else if n == s::LARGO_AVISO_PERSONA && w(0) == s::AVISO_PERSONA {
                // U6: de quién es un programa (de `sesiones`; solo él e `inicio` escriben sin marca).
                let (uid, mm) = (w(4), u64::from_le_bytes(buf[8..16].try_into().unwrap()));
                let fin = buf[16..32].iter().position(|&b| b == 0).unwrap_or(16);
                if uid >= s::PRIMERA_PERSONA {
                    personas.insert(mm, uid);
                } else {
                    personas.remove(&mm);
                }
                if uid == 0 {
                    lanzados.remove(&mm);
                } else if !nombres.contains_key(&mm) {
                    lanzados.insert(mm, String::from(core::str::from_utf8(&buf[16..16 + fin]).unwrap_or("?")));
                }
                seguir_sesiones(&mut sv.g, &mut sv.clientes, &personas, &detras);
            } else if n == s::LARGO_AVISO_DETRAS && w(0) == s::AVISO_DETRAS {
                let cuantas = (w(4) as usize).min(s::MAX_DETRAS);
                detras = (0..cuantas).map(|i| w(8 + 4 * i)).filter(|&u| u >= s::PRIMERA_PERSONA).collect();
                seguir_sesiones(&mut sv.g, &mut sv.clientes, &personas, &detras);
            }
            cerrar_llaves(&llaves);
            continue;
        }
        if n < 4 {
            cerrar_llaves(&llaves);
            continue;
        }
        let w = |i: usize| if 4 * i + 4 <= n { u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap()) } else { 0 };
        // Los poderes (el volumen general, ser el controlador, mezclar), solo por el nombre que da `inicio`.
        let nombre = nombres.get(&marca).cloned().unwrap_or_else(|| String::from("?"));
        let para_mensajes = nombres.get(&marca).or_else(|| lanzados.get(&marca)).cloned().unwrap_or_else(|| String::from("?"));
        let mezcla = MEZCLAN.contains(&nombre.as_str());
        match w(0) {
            p::SALIDA if n == 16 && k == 1 && nombre == "virtio-sonido" && sv.salidas.is_empty() => {
                let (ranuras, cuadros, ritmo) = (w(1), w(2), w(3));
                let cabe = (DATOS as i64 + ranuras as i64 * cuadros as i64 * 4) <= dashos::memoria_tamano(llaves[0]);
                if ritmo != 48_000 || !(1..=64).contains(&ranuras) || !(16..=4096).contains(&cuadros) || !cabe || dashos::memoria_proyectar(llaves[0], VA_SALIDA, true) != 0 {
                    println!("  [sonido] la salida de {} no vale", nombre);
                    cerrar_llaves(&llaves);
                    continue;
                }
                periodo.resize(cuadros as usize, [0; 2]);
                let Some(s) = nueva_salida(&mut sv.g, "Altavoces", ranuras, cuadros) else {
                    println!("  [sonido] sin sitio en el grafo para la salida");
                    continue;
                };
                sv.salidas.push(s);
                sv.ajustes.defecto = Some(0);
                sv.poner_periodo(sv.ajustes.periodo_modo);
                // Los flujos que esperaban sin salida, a esta.
                let sueltos: Vec<Id> = sv.clientes.values().map(|c| c.nodo).filter(|&id| sv.g.destinos(id).is_empty()).collect();
                for id in sueltos {
                    sv.conectar_a_defecto(id);
                }
                println!("  [sonido] salida por {}: {} ranuras de {} cuadros a 48 kHz", nombre, ranuras, cuadros);
                println!("  [sonido] el grafo: flujos -> mezcla -> eq -> espacial -> limitador -> Altavoces, en cuantos de {}", sv.g.cuanto());
                // La primera salida, al arrancar, no suena "conectar": eso es para lo que se enchufa después.
            }
            // Solo el controlador marca el ritmo de la mezcla (S8).
            p::TIC if nombre == "virtio-sonido" => {
                cerrar_llaves(&llaves);
                let Some(s) = sv.salidas.first() else { continue };
                let (ranuras, cuadros, nodo_salida) = (s.ranuras, s.cuadros, s.nodo);
                // Medida de A6: tics separados más de 15 ms, o un tic que tarda más de 3 ms en mezclar.
                let t0 = dashos::ahora_us();
                if ultimo_tic != 0 && t0 - ultimo_tic > 15_000 {
                    println!("  [sonido] {} ms sin tics", (t0 - ultimo_tic) / 1000);
                }
                ultimo_tic = t0;
                // La latencia: lo que tardó cada ranura que el aparato acaba de devolver desde que se escribió.
                {
                    let leidos = palabra(VA_SALIDA, 1);
                    let s = &mut sv.salidas[0];
                    while s.leidos_vistos != leidos {
                        let t = s.escrita_us[(s.leidos_vistos % ranuras) as usize];
                        if t != 0 {
                            let l = t0.saturating_sub(t);
                            sv.latencia.0 += l;
                            sv.latencia.1 += 1;
                            sv.latencia.2 = sv.latencia.2.max(l);
                        }
                        s.leidos_vistos = s.leidos_vistos.wrapping_add(1);
                    }
                }
                let adelante = sv.adelante();
                loop {
                    let (escritos, leidos) = (palabra(VA_SALIDA, 0), palabra(VA_SALIDA, 1));
                    if escritos.wrapping_sub(leidos) >= adelante.min(ranuras) {
                        break;
                    }
                    for c in sv.clientes.values_mut() {
                        recoger(&mut sv.g, c, &mut tmp);
                    }
                    let tm = dashos::ahora_us();
                    sv.g.llenar(nodo_salida, cuadros as usize);
                    let hechos = sv.g.sacar(nodo_salida, &mut periodo);
                    let dm = dashos::ahora_us() - tm;
                    if sv.clientes.len() + sv.sistema.is_some() as usize > 0 {
                        sv.mezcla_us += dm;
                        sv.periodos += 1;
                        sv.peor_us = sv.peor_us.max(dm);
                    }
                    if hechos < periodo.len() {
                        periodo[hechos..].fill([0; 2]);
                    }
                    sv.analizador.alimentar(&periodo);
                    let base = VA_SALIDA + DATOS + (escritos % ranuras) as usize * cuadros as usize * 4;
                    for (i, c) in periodo.iter().enumerate() {
                        let [l, r] = sonido::a_i16(*c);
                        unsafe {
                            write_volatile((base + 4 * i) as *mut i16, l);
                            write_volatile((base + 4 * i + 2) as *mut i16, r);
                        }
                    }
                    fence(Ordering::SeqCst);
                    sv.salidas[0].escrita_us[(escritos % ranuras) as usize] = dashos::ahora_us();
                    poner(VA_SALIDA, 0, escritos.wrapping_add(1));
                }
                let t1 = dashos::ahora_us();
                if t1 - t0 > 3_000 {
                    println!("  [sonido] un tic tardó {} us en mezclar", t1 - t0);
                }
                // Despertar a quien tenga el anillo por debajo de la mitad; quitar los flujos acabados.
                let mut fuera = Vec::new();
                for (clave, c) in sv.clientes.iter_mut() {
                    let disp = disponibles(c);
                    let cabeza = palabra(c.va, 0);
                    if cabeza != c.ultima {
                        c.ultima = cabeza;
                        c.avisado = false;
                    }
                    if c.cerrando {
                        if disp == 0 && sv.g.pendientes(c.nodo) == Some(0) {
                            let _ = sv.g.quitar(c.nodo);
                        }
                        if !sv.g.existe(c.nodo) {
                            fuera.push(*clave);
                        }
                    } else if !c.avisado && disp < c.tam / 2 {
                        if dashos::canal_escribir(c.aviso, &[1, 0, 0, 0], &[]) == E_CERRADO {
                            println!("  [sonido] {} ya no está: su flujo sale con fundido", c.nombre);
                            c.faltas = sv.g.faltas(c.nodo).unwrap_or(0);
                            let _ = sv.g.quitar(c.nodo);
                            c.cerrando = true;
                        }
                        c.avisado = true;
                    }
                }
                for clave in fuera {
                    if let Some(c) = sv.clientes.remove(&clave) {
                        println!("  [sonido] {}: flujo cerrado; {} faltas, la salida lleva {} vacíos", c.nombre, c.faltas, palabra(VA_SALIDA, 4));
                        if sv.periodos > 0 {
                            println!("  [sonido] el grafo: {} períodos con sonido, media {} us, el peor {} us", sv.periodos, sv.mezcla_us / sv.periodos, sv.peor_us);
                        }
                        if sv.latencia.1 > 0 {
                            println!(
                                "  [sonido] latencia de la salida: media {} us, la peor {} us ({} períodos de {} cuadros por delante)",
                                sv.latencia.0 / sv.latencia.1,
                                sv.latencia.2,
                                sv.adelante(),
                                cuadros
                            );
                        }
                        dashos::memoria_quitar(c.va);
                        dashos::cerrar(c.memoria);
                        dashos::cerrar(c.aviso);
                        sv.huecos &= !(1 << ((c.va - VA_ANILLOS) / HUECO));
                        if !sv.clientes.keys().any(|(m, _)| *m == clave.0) {
                            sv.olvidar_medios(clave.0);
                        }
                    }
                }
                sv.tics = sv.tics.wrapping_add(1);
                if sv.tics % CADA_ESTADO == 0 {
                    sv.publicar(true);
                }
            }
            p::ABRIR if n == 16 && k == 2 => {
                let propios = sv.clientes.keys().filter(|(m, _)| *m == marca).count();
                if propios >= FLUJOS_POR_PROGRAMA || sv.clientes.len() >= MAX_FLUJOS {
                    if !sv.topes_dichos.contains(&marca) {
                        sv.topes_dichos.push(marca);
                        println!("  [sonido] {}: tope de {} flujos; no se abren más", para_mensajes, if propios >= FLUJOS_POR_PROGRAMA { FLUJOS_POR_PROGRAMA } else { MAX_FLUJOS });
                    }
                    cerrar_llaves(&llaves);
                    continue;
                }
                let indice = (0..FLUJOS_POR_PROGRAMA as u8).find(|i| !sv.clientes.contains_key(&(marca, *i))).unwrap_or(0);
                let f = Muestra::desde(w(1)).map(|muestra| Formato { muestra, canales: w(2) as u8, frecuencia: w(3) });
                let hueco = (0..64).find(|h| sv.huecos & (1 << h) == 0);
                let va = hueco.map(|h| VA_ANILLOS + h * HUECO).unwrap_or(0);
                let tamano = dashos::memoria_tamano(llaves[0]);
                let ok = f.is_some_and(|f| f.valido() && w(2) <= 2)
                    && hueco.is_some()
                    && tamano > DATOS as i64
                    && tamano as usize <= HUECO
                    && dashos::memoria_proyectar(llaves[0], va, true) == 0;
                let tam = if ok { palabra(va, 2) } else { 0 };
                if !ok || !tam.is_power_of_two() || tam as i64 + DATOS as i64 > tamano {
                    println!("  [sonido] {} pide un flujo que no vale", para_mensajes);
                    if ok {
                        dashos::memoria_quitar(va);
                    }
                    cerrar_llaves(&llaves);
                    continue;
                }
                let f = f.unwrap();
                let Ok(nodo) = sv.g.flujo(&para_mensajes, marca, f) else {
                    println!("  [sonido] {}: el grafo está lleno", para_mensajes);
                    dashos::memoria_quitar(va);
                    cerrar_llaves(&llaves);
                    continue;
                };
                sv.conectar_a_defecto(nodo);
                sv.huecos |= 1 << hueco.unwrap();
                println!("  [sonido] {} abre un flujo: {:?}, {} canales, {} Hz", para_mensajes, f.muestra, f.canales, f.frecuencia);
                sv.clientes.insert((marca, indice), Cliente {
                    nombre: para_mensajes,
                    volumen: (1.0, 1.0),
                    mezcla: [UNO_VOL; 2],
                    silencio: false,
                    leido_en_silencio: 0,
                    nodo,
                    va,
                    tam,
                    aviso: llaves[1],
                    memoria: llaves[0],
                    avisado: false,
                    ultima: 0,
                    cerrando: false,
                    faltas: 0,
                });
                // U6: si su sesión está detrás, en silencio desde el principio.
                seguir_sesiones(&mut sv.g, &mut sv.clientes, &personas, &detras);
                sv.publicar(false);
            }
            // El volumen propio de un flujo: [izq, der] (el primero) o [izq, der, índice].
            p::VOLUMEN if n == 12 || n == 16 => {
                cerrar_llaves(&llaves);
                let indice = if n == 16 { w(3).min(255) as u8 } else { 0 };
                if let Some(c) = sv.clientes.get_mut(&(marca, indice)) {
                    c.volumen = (f32::from_bits(w(1)), f32::from_bits(w(2)));
                    aplicar_volumen(&mut sv.g, c);
                }
            }
            p::GENERAL if n == 8 => {
                cerrar_llaves(&llaves);
                if PONEN_GENERAL.contains(&nombre.as_str()) {
                    let v = f32::from_bits(w(1));
                    if let Some(d) = sv.ajustes.defecto {
                        let _ = sv.g.volumen(sv.salidas[d].mezcla, v, v);
                    }
                    let nombre_salida = sv.ajustes.defecto.map(|d| sv.salidas[d].nombre.as_str()).unwrap_or("ninguna");
                    println!("  [sonido] volumen general: {} % (lo pide {}), salida {}", (v.clamp(0.0, 4.0) * 100.0 + 0.5) as u32, nombre, nombre_salida);
                    sv.publicar(false);
                } else {
                    println!("  [sonido] {} pide el volumen general: no le toca", nombre);
                }
            }
            p::CERRAR => {
                cerrar_llaves(&llaves);
                let indice = if n >= 8 { w(1).min(255) as u8 } else { 0 };
                // Las faltas, de antes de cerrar: al acabarse el anillo, el último período siempre se queda corto.
                if let Some(c) = sv.clientes.get_mut(&(marca, indice)) {
                    c.faltas = sv.g.faltas(c.nodo).unwrap_or(0);
                    c.cerrando = true;
                }
            }
            p::SUSCRIBIR if k == 2 => {
                if !LEEN.contains(&nombre.as_str()) {
                    println!("  [sonido] {} quiere ver el estado: no le toca", para_mensajes);
                    cerrar_llaves(&llaves);
                    continue;
                }
                // Si ya estaba, se quita lo viejo.
                if let Some(i) = sv.subs.iter().position(|s| s.marca == marca) {
                    let s = sv.subs.remove(i);
                    sv.soltar_suscriptor(s);
                }
                let hueco = (0..MAX_SUSCRIPTORES).find(|h| sv.huecos_estado & (1 << h) == 0);
                let va = hueco.map_or(0, |h| VA_ESTADOS + h * HUECO_ESTADO);
                let tamano = dashos::memoria_tamano(llaves[0]);
                if hueco.is_none() || tamano < p::TAM_ESTADO as i64 || tamano as usize > HUECO_ESTADO || dashos::memoria_proyectar(llaves[0], va, true) != 0 {
                    println!("  [sonido] {}: el estado no vale", nombre);
                    cerrar_llaves(&llaves);
                    continue;
                }
                sv.huecos_estado |= 1 << hueco.unwrap();
                let ventana = sv.ajustes.ventana;
                sv.subs.push(Suscriptor { marca, nombre: nombre.clone(), va, memoria: llaves[0], aviso: llaves[1], latido: 0, latido_us: 0, ventana_dicha: ventana });
                println!("  [sonido] {} mira el estado", nombre);
                sv.publicar(false);
            }
            p::NODO_VOLUMEN | p::NODO_SILENCIO | p::NODO_SALIDA | p::POR_DEFECTO | p::EFECTO | p::PERIODO if !mezcla => {
                cerrar_llaves(&llaves);
                println!("  [sonido] {} quiere tocar el mezclador: no le toca", para_mensajes);
            }
            p::NODO_VOLUMEN if n == 16 => {
                let (id, i, d) = (w(1), f32::from_bits(w(2)), f32::from_bits(w(3)));
                let v = [a_fijo(i), a_fijo(d)];
                if let Some(c) = sv.clientes.values_mut().find(|c| c.nodo == id) {
                    c.mezcla = v;
                    aplicar_volumen(&mut sv.g, c);
                } else if sv.salidas.iter().any(|s| s.mezcla == id) || sv.sistema == Some(id) {
                    let _ = sv.g.volumen_fijo(id, v);
                }
                sv.publicar(false);
            }
            p::NODO_SILENCIO if n == 12 => {
                let (id, si) = (w(1), w(2) != 0);
                let valido = sv.clientes.values().any(|c| c.nodo == id) || sv.salidas.iter().any(|s| s.mezcla == id) || sv.sistema == Some(id);
                if valido {
                    let _ = sv.g.silencio(id, si);
                }
                sv.publicar(false);
            }
            p::NODO_SALIDA if n == 12 => {
                let (id, salida) = (w(1), w(2));
                let es_flujo = sv.clientes.values().any(|c| c.nodo == id) || sv.sistema == Some(id);
                if let (true, Some(a)) = (es_flujo, sv.salidas.iter().position(|s| s.nodo == salida)) {
                    sv.mover(id, a);
                    println!("  [sonido] un flujo pasa a sonar por {}", sv.salidas[a].nombre);
                }
                sv.publicar(false);
            }
            p::POR_DEFECTO if n == 8 => {
                if let Some(a) = sv.salidas.iter().position(|s| s.nodo == w(1)) {
                    let antes = sv.ajustes.defecto;
                    sv.ajustes.defecto = Some(a);
                    // Los flujos que iban por la salida de antes (no los que eligieron otra), a la nueva.
                    if let Some(b) = antes.filter(|&b| b != a) {
                        let mezcla_vieja = sv.salidas[b].mezcla;
                        let mover: Vec<Id> = sv.clientes.values().map(|c| c.nodo).chain(sv.sistema).filter(|&id| sv.g.destinos(id).contains(&mezcla_vieja)).collect();
                        for id in mover {
                            sv.mover(id, a);
                        }
                    }
                    println!("  [sonido] la salida del sistema es {}", sv.salidas[a].nombre);
                }
                sv.publicar(false);
            }
            p::EFECTO if n == 12 => {
                let (que, valor) = (w(1), w(2) as i32);
                if !sv.efecto(que, valor) {
                    println!("  [sonido] efecto {} = {}: no existe", que, valor);
                }
                sv.publicar(false);
            }
            p::PERIODO if n == 8 => {
                sv.poner_periodo(w(1));
                println!("  [sonido] período: {} (cuanto del grafo {} cuadros, {} por delante)", ["normal", "para tocar", "ahorro"][sv.ajustes.periodo_modo as usize], sv.g.cuanto(), sv.adelante());
                sv.publicar(false);
            }
            p::VENTANA => {
                cerrar_llaves(&llaves);
                if ABREN.contains(&nombre.as_str()) {
                    sv.ajustes.ventana = sv.ajustes.ventana.wrapping_add(1);
                    sv.publicar(false);
                } else {
                    println!("  [sonido] {} pide abrir la ventana del sonido: no le toca", para_mensajes);
                }
            }
            p::MEDIOS if n >= 28 => {
                let (lt, la) = (w(5) as usize, w(6) as usize);
                let texto = |a: usize, l: usize| -> Option<String> {
                    if l > 96 || a + l > n {
                        return None;
                    }
                    core::str::from_utf8(&buf[a..a + l]).ok().map(String::from)
                };
                let (Some(titulo), Some(artista)) = (texto(28, lt), texto(28 + lt, la)) else {
                    println!("  [sonido] {}: lo que suena no vale", para_mensajes);
                    cerrar_llaves(&llaves);
                    continue;
                };
                let flujo = sv.clientes.iter().find(|((m, _), _)| *m == marca).map_or(0, |(_, c)| c.nodo);
                let datos = p::Medio {
                    id: 0,
                    estado: w(1).min(2),
                    capacidades: w(2) & 7,
                    posicion_ms: w(3),
                    duracion_ms: w(4),
                    flujo,
                    titulo,
                    artista,
                    programa: para_mensajes.clone(),
                };
                if let Some(m) = sv.medios.iter_mut().find(|m| m.marca == marca) {
                    m.datos = p::Medio { id: m.datos.id, ..datos };
                    if let Some(&l) = llaves.first() {
                        if m.orden != 0 {
                            dashos::cerrar(m.orden);
                        }
                        m.orden = l;
                    }
                } else if sv.medios.len() < p::MAX_MEDIOS {
                    let id = sv.siguiente_medio;
                    sv.siguiente_medio += 1;
                    println!("  [sonido] {} dice lo que suena: {} ({})", para_mensajes, datos.titulo, datos.artista);
                    sv.medios.push(MedioP { marca, datos: p::Medio { id, ..datos }, orden: llaves.first().copied().unwrap_or(0) });
                } else {
                    cerrar_llaves(&llaves);
                }
                sv.publicar(false);
            }
            p::MEDIOS_FIN => {
                cerrar_llaves(&llaves);
                sv.olvidar_medios(marca);
            }
            p::MEDIOS_ORDEN if n == 12 => {
                cerrar_llaves(&llaves);
                if !LEEN.contains(&nombre.as_str()) {
                    println!("  [sonido] {} quiere mandar a otro programa: no le toca", para_mensajes);
                    continue;
                }
                let (id, orden) = (w(1), w(2));
                if let Some(m) = sv.medios.iter().find(|m| m.datos.id == id && m.orden != 0) {
                    let r = dashos::canal_escribir(m.orden, &[p::MEDIOS_ORDEN.to_le_bytes(), orden.min(3).to_le_bytes()].concat(), &[]);
                    println!("  [sonido] {} manda {} a {}", nombre, ["?", "reproducir o pausar", "siguiente", "anterior"][orden.min(3) as usize], m.datos.programa);
                    if r == E_CERRADO {
                        let marca_m = m.marca;
                        sv.olvidar_medios(marca_m);
                    }
                }
            }
            p::SONAR if n == 8 => {
                cerrar_llaves(&llaves);
                if !SUENAN.contains(&nombre.as_str()) {
                    println!("  [sonido] {} pide un sonido del sistema: no le toca", para_mensajes);
                    continue;
                }
                if let Some(e) = Evento::desde(w(1)) {
                    sv.sonar(e);
                }
            }
            p::SONIDOS if n == 16 => {
                cerrar_llaves(&llaves);
                if !mezcla {
                    println!("  [sonido] {} quiere cambiar los sonidos del sistema: no le toca", para_mensajes);
                    continue;
                }
                sv.ajustes.tema = w(1).min(sonido::sonidos::TEMAS.len() as u32 - 1);
                sv.ajustes.silenciados = w(2) != 0;
                sv.ajustes.volumen_sonidos = w(3).min(100);
                println!("  [sonido] sonidos del sistema: tema {}, {}, volumen {} %", sonido::sonidos::TEMAS[sv.ajustes.tema as usize], if sv.ajustes.silenciados { "silenciados" } else { "activos" }, sv.ajustes.volumen_sonidos);
                sv.publicar(false);
            }
            p::NO_MOLESTAR if n == 8 => {
                cerrar_llaves(&llaves);
                if nombre == "panel" {
                    sv.ajustes.no_molestar = w(1) != 0;
                    sv.publicar(false);
                }
            }
            _ => cerrar_llaves(&llaves),
        }
    }
}
