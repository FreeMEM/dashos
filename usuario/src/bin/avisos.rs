//! S10, S27 (oleada 3): el servicio de avisos (las notificaciones). Los programas con su llave mandan avisos; el
//! servicio los guarda en un historial acotado y toca el timbre de la barra, que los enseña. Protocolo e historial en
//! `usuario/src/protocolo/avisos.rs`; el modelo (actuales e historial, acciones, "no molestar") es el de DMS
//! (DankMaterialShell, `Services/NotificationService.qml`, MIT, copyright (c) 2025 Avenge Media LLC; aviso entero en
//! `docs/licencias/DankMaterialShell-MIT.txt`).
//!
//! - Quién avisa lo pone el servicio, por la marca: el nombre que le dio `inicio` (entrada de directorio) o `sesiones`
//!   (`AVISO_PERSONA`, antes de lanzar un programa del disco). Solo valen sin marca: un programa no se cambia el nombre.
//!   Un programa que el servicio no conoce no avisa.
//! - Un programa que inunda: como mucho `RITMO` avisos por minuto, y `POR_PROGRAMA` en el historial. No tapa los de los
//!   demás ni le gasta memoria al servicio (todo es de tamaño fijo).
//! - Acciones: un aviso puede traer hasta `MAX_ACCIONES`, y el programa da una llave por la que oírlas (una por
//!   programa: la última que dio). Al pulsar una, el servicio le manda `ACCION_PULSADA` por esa llave y el aviso deja de
//!   ser actual. Un programa solo sustituye sus avisos, y solo la barra pulsa acciones.
//! - U6: los avisos de una persona con la sesión detrás (`AVISO_DETRAS`, de `sesiones`) se guardan sin enseñarlos. Al
//!   volver ella delante salen en el historial, sin leer y marcados como retenidos (la barra no los saca como emergente).
//! - Solo la barra (el nombre `barra` que da `inicio`) se suscribe, lee, descarta o borra. "No molestar" lo ponen la
//!   barra y el panel; los ajustes (`ENV:Escritorio/avisos`, en bytes) solo los da la barra (se los pasa el centro de
//!   control, que es quien lee los ficheros).
//!
//! - S27, oleada 6: lo que se ve en la pantalla de bloqueo lo pide `sesiones` sin marca (`AVISO_RESUMEN`, con la llave de
//!   `guardian`): cuántos actuales, o también de quién, según el ajuste `bloqueo`; nunca el título ni el texto.
//!
//! Llave 1: el extremo de lectura de su canal "avisos".

#![no_std]
#![no_main]

#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;
#[path = "../avisos_ajustes.rs"]
#[allow(dead_code)]
mod avisos_ajustes;

use dashos::println;
use dashos::protocolo::avisos::*;
use dashos::protocolo::sesiones as s;
use dashos::servicio::{cerrar_llaves, recibir, Recibido, MAX_DATOS, MAX_LLAVES};

dashos::programa!(principal);

const CANAL: u32 = 1;
/// Los nombres que conoce, a la vez: los de `inicio` y lo que lanza `sesiones`.
const MAX_NOMBRES: usize = 64;
/// Quién lee el historial.
const LECTORES: &[&str] = &["barra"];
/// Quién pone y mira "no molestar".
const NO_MOLESTAN: &[&str] = &["barra", "panel"];
/// Quién da los ajustes: la barra, que se los da el centro de control (que lee `ENV:Escritorio/avisos`).
const AJUSTAN: &[&str] = &["barra"];

#[derive(Clone, Copy)]
struct Nombre {
    marca: u64,
    uid: u32,
    nombre: [u8; 16],
    /// Si ya se dijo que inunda (para no repetirlo en cada aviso rechazado).
    inunda: bool,
    /// La llave por la que oye sus acciones (0, ninguna).
    escucha: u32,
}

struct Servicio {
    nombres: [Option<Nombre>; MAX_NOMBRES],
    detras: [u32; s::MAX_DETRAS],
    historial: Historial,
    ritmo: Ritmo,
    /// La llave del timbre de la barra y si ya hay uno sin atender.
    timbre: u32,
    pendiente: bool,
    no_molestar: NoMolestar,
    /// Los ajustes en bytes (como los dio el centro) y lo que de ellos usa el servicio.
    ajustes: [u8; MAX_AJUSTES],
    largo_ajustes: usize,
    historial_si: bool,
    dias_historial: u32,
    /// Sube con cada cambio de los ajustes o del "no molestar": la barra los vuelve a pedir.
    version: u32,
}

impl Servicio {
    fn nombre(&self, marca: u64) -> Option<Nombre> {
        self.nombres.iter().flatten().find(|n| n.marca == marca).copied()
    }

    fn nombre_mut(&mut self, marca: u64) -> Option<&mut Nombre> {
        self.nombres.iter_mut().flatten().find(|n| n.marca == marca)
    }

    /// Apunta (o cambia) el nombre de `marca`. Si no cabe, se va uno de lo lanzado del disco.
    fn apuntar(&mut self, marca: u64, uid: Option<u32>, nombre: &[u8]) {
        let mut n16 = [0u8; 16];
        let l = limpiar(nombre.split(|&c| c == 0).next().unwrap_or(b""), &mut n16);
        n16[l..].fill(0);
        if let Some(n) = self.nombre_mut(marca) {
            n.nombre = n16;
            if let Some(u) = uid {
                n.uid = u;
            }
            return;
        }
        let hueco = self.nombres.iter().position(|x| x.is_none())
            .or_else(|| self.nombres.iter().position(|x| x.is_some_and(|n| n.marca >= lanzables::PRIMER_PID_PROPIO)));
        if let Some(i) = hueco {
            if let Some(viejo) = self.nombres[i] && viejo.escucha != 0 {
                dashos::cerrar(viejo.escucha);
            }
            self.nombres[i] = Some(Nombre { marca, uid: uid.unwrap_or(1), nombre: n16, inunda: false, escucha: 0 });
        }
    }

    fn es(&self, marca: u64, quienes: &[&str]) -> bool {
        self.nombre(marca).is_some_and(|n| lanzables::del_sistema(marca, nombre16(&n.nombre), quienes))
    }

    fn detras(&self) -> &[u32] {
        let n = self.detras.iter().take_while(|&&u| u != 0).count();
        &self.detras[..n]
    }

    /// Toca el timbre de la barra, si no hay ya uno sin atender.
    fn tocar(&mut self) {
        if self.timbre == 0 || self.pendiente {
            return;
        }
        match dashos::canal_escribir(self.timbre, &TIMBRE.to_le_bytes(), &[]) {
            0 => self.pendiente = true,
            dashos::E_CERRADO => {
                dashos::cerrar(self.timbre);
                self.timbre = 0;
                println!("  [avisos] la barra ya no está");
            }
            // Cola llena: la barra se lo encontrará en la próxima.
            _ => {}
        }
    }

    /// La cabecera de las respuestas a la barra y al panel: estado, sin leer, visibles y versión.
    fn cabecera(&self, r: &mut [u8], estado: i32) {
        let (total, sin) = self.historial.cuenta(self.detras());
        r[0..4].copy_from_slice(&estado.to_le_bytes());
        r[4..8].copy_from_slice(&sin.to_le_bytes());
        r[8..12].copy_from_slice(&total.to_le_bytes());
        r[12..16].copy_from_slice(&self.version.to_le_bytes());
    }

    /// Quita lo descartado que ya caducó (los días del historial).
    fn caducar(&mut self) {
        let n = self.historial.caducar(dashos::hora_ns(), self.dias_historial);
        if n > 0 {
            println!("  [avisos] {} avisos del historial caducan (más de {} días)", n, self.dias_historial);
        }
    }

    /// Descarta uno: deja de ser actual; sin historial, se quita del todo.
    fn descartar(&mut self, numero: u32) -> bool {
        let (d, nd) = (self.detras, self.detras().len());
        if self.historial_si { self.historial.descartar(numero, &d[..nd]) } else { self.historial.quitar(numero, &d[..nd]) }
    }

    /// Un aviso de `marca`: lo guarda (sustituyendo el suyo, si lo pide) y toca el timbre. El estado y su número.
    fn avisar(&mut self, marca: u64, m: &[u8], escucha: Option<u32>) -> (i32, u32) {
        let Some(q) = self.nombre(marca) else { return (PROTEGIDO, 0) };
        let p = match leer_pedido(m) {
            Ok(p) => p,
            Err(e) => return (e, 0),
        };
        if !self.ritmo.puede(marca, dashos::ahora_us()) {
            if !q.inunda {
                println!("  [avisos] {}: demasiados avisos seguidos; los siguientes no se guardan", nombre16(&q.nombre));
                if let Some(n) = self.nombre_mut(marca) {
                    n.inunda = true;
                }
            }
            return (DEMASIADOS, 0);
        }
        let mut a = Aviso::vacio();
        a.urgencia = p.urgencia;
        a.marca = marca;
        a.uid = q.uid;
        a.sistema = marca < lanzables::PRIMER_PID_PROPIO;
        a.de = q.nombre;
        a.hora = dashos::hora_ns();
        a.retenido = !visible(q.uid, self.detras());
        a.lt = limpiar(p.titulo, &mut a.titulo) as u8;
        a.lx = limpiar(p.texto, &mut a.texto) as u8;
        a.poner_acciones(&p);
        let numero = match self.historial.meter_sustituyendo(a, p.reemplaza) {
            Ok(n) => n,
            Err(e) => {
                println!("  [avisos] {} quiere sustituir el aviso {}, que no es suyo: no", nombre16(&q.nombre), p.reemplaza);
                return (e, 0);
            }
        };
        if let Some(n) = self.nombre_mut(marca) {
            n.inunda = false;
            // La llave para oír sus acciones: la última que dio.
            if let Some(e) = escucha {
                if n.escucha != 0 {
                    dashos::cerrar(n.escucha);
                }
                n.escucha = e;
            }
        }
        let titulo = core::str::from_utf8(&a.titulo[..a.lt as usize]).unwrap_or("?");
        if visible(q.uid, self.detras()) {
            let sustituye = if p.reemplaza != 0 && self.historial.buscar(numero, &[]).is_some_and(|x| x.reemplaza != 0) {
                Some(p.reemplaza)
            } else {
                None
            };
            match (p.nacc, sustituye) {
                (0, None) => println!("  [avisos] {} avisa: \"{}\" (número {})", nombre16(&q.nombre), titulo, numero),
                (k, None) => println!("  [avisos] {} avisa: \"{}\" (número {}, {} acciones)", nombre16(&q.nombre), titulo, numero, k),
                (k, Some(v)) => println!("  [avisos] {} avisa: \"{}\" (número {}, {} acciones; sustituye al {})", nombre16(&q.nombre), titulo, numero, k, v),
            }
            self.tocar();
        } else {
            println!("  [avisos] {} avisa con su sesión detrás (usuario {}): se guarda para cuando vuelva", nombre16(&q.nombre), q.uid);
        }
        (BIEN, numero)
    }

    /// La barra pulsa la acción `indice` (o `POR_DEFECTO`) del aviso `numero`: se le dice al programa que avisó, por su
    /// llave, y el aviso deja de ser actual.
    fn accion(&mut self, numero: u32, indice: u32) -> i32 {
        let Some(a) = self.historial.buscar(numero, self.detras()).copied() else { return NO_HAY };
        let k = if indice == POR_DEFECTO { a.por_defecto() } else { (indice < a.nacc as u32).then_some(indice as usize) };
        let Some(k) = k else { return MAL };
        let acc = a.acciones()[k];
        let Some(q) = self.nombre(a.marca).filter(|q| q.escucha != 0) else {
            println!("  [avisos] acción \"{}\" del aviso {}: {} no la oye", acc.id(), numero, a.de());
            self.descartar(numero);
            return NO_HAY;
        };
        let mut m = [0u8; 32];
        let n = escribir_pulsada(numero, &acc, &mut m);
        let e = dashos::canal_escribir(q.escucha, &m[..n], &[]);
        if e == dashos::E_CERRADO {
            dashos::cerrar(q.escucha);
            if let Some(x) = self.nombre_mut(a.marca) {
                x.escucha = 0;
            }
        }
        self.descartar(numero);
        println!("  [avisos] acción \"{}\" del aviso {} a {}: {}", acc.id(), numero, a.de(), if e == 0 { "dada" } else { "no llega" });
        if e == 0 { BIEN } else { NO_HAY }
    }
}

/// El servicio entero (el historial, con sus acciones, pasa de 20 KB): en memoria estática, no en la pila del programa,
/// que se quedaría corta.
static mut SERVICIO: Servicio = Servicio {
    nombres: [None; MAX_NOMBRES],
    detras: [0; s::MAX_DETRAS],
    historial: Historial::new(),
    ritmo: Ritmo::new(),
    timbre: 0,
    pendiente: false,
    no_molestar: NoMolestar { puesto: false, hasta_us: 0 },
    ajustes: [0; MAX_AJUSTES],
    largo_ajustes: 0,
    historial_si: true,
    dias_historial: avisos_ajustes::Ajustes::DE_SERIE.dias_historial as u32,
    version: 1,
};

fn principal() -> u64 {
    // Seguro: el programa tiene un solo hilo y esta es la única referencia que se saca de `SERVICIO`.
    let sv: &mut Servicio = unsafe { &mut *core::ptr::addr_of_mut!(SERVICIO) };
    let mut b = [0u8; MAX_DATOS];
    let mut ll = [0u32; MAX_LLAVES];
    println!("  [avisos] en marcha: historial de {}, {} por programa, {} por minuto", MAX_HISTORIAL, POR_PROGRAMA, RITMO);
    loop {
        let (n, k, marca) = match recibir(CANAL, &mut b, &mut ll) {
            Recibido::Mensaje { n, k, marca } => (n, k, marca),
            Recibido::Cerrado => return 0,
            Recibido::Nada => continue,
        };
        let m = &b[..n];
        let u32_en = |i: usize| m.get(i..i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
        let u64_en = |i: usize| m.get(i..i + 8).map_or(0, |x| u64::from_le_bytes(x.try_into().unwrap()));
        let mut usadas = 0;
        if marca == 0 {
            // De `inicio` o de `sesiones`: quién es quién y qué sesiones están detrás.
            if n == 24 {
                sv.apuntar(u64_en(0), None, &m[8..24]);
            } else if n == s::LARGO_AVISO_PERSONA && u32_en(0) == s::AVISO_PERSONA {
                let (uid, de) = (u32_en(4), u64_en(8));
                if uid == 0 {
                    // Su baja: si es de lo lanzado, se olvida (sus avisos guardan su nombre) y se suelta su llave.
                    if de >= lanzables::PRIMER_PID_PROPIO {
                        for x in sv.nombres.iter_mut().filter(|x| x.is_some_and(|n| n.marca == de)) {
                            if let Some(n) = x && n.escucha != 0 {
                                dashos::cerrar(n.escucha);
                            }
                            *x = None;
                        }
                    }
                } else {
                    sv.apuntar(de, Some(uid), &m[16..32]);
                }
            } else if n == s::LARGO_AVISO_DETRAS && u32_en(0) == s::AVISO_DETRAS {
                let antes = sv.detras;
                let cuantas = (u32_en(4) as usize).min(s::MAX_DETRAS);
                sv.detras = [0; s::MAX_DETRAS];
                for i in 0..cuantas {
                    sv.detras[i] = u32_en(8 + 4 * i);
                }
                // Las que vuelven delante, con lo que se les guardó.
                for &u in antes.iter().filter(|&&u| u != 0 && !sv.detras.contains(&u)) {
                    let guardados = sv.historial.de_usuario(u);
                    if guardados > 0 {
                        println!("  [avisos] vuelve delante el usuario {}: {} avisos guardados", u, guardados);
                    }
                }
                sv.tocar();
            } else if n == s::LARGO_AVISO_RESUMEN && u32_en(0) == s::AVISO_RESUMEN && k >= 1 {
                // S27, oleada 6: la pantalla de bloqueo (por `sesiones`, con la llave de `guardian`): solo lo que deja
                // el ajuste `bloqueo` (de serie, cuántos).
                let bloqueo = avisos_ajustes::Ajustes::leer(&sv.ajustes[..sv.largo_ajustes]).map_or(avisos_ajustes::Bloqueo::Contador, |a| a.bloqueo);
                let modo = match bloqueo {
                    avisos_ajustes::Bloqueo::Nada => 0,
                    avisos_ajustes::Bloqueo::Contador => 1,
                    avisos_ajustes::Bloqueo::Nombres => 2,
                    avisos_ajustes::Bloqueo::Todo => 3,
                };
                let mut r = [0u8; LARGO_RESUMEN];
                let largo = sv.historial.resumen_bloqueo(&sv.detras, modo, &mut r);
                dashos::canal_escribir(ll[0], &r[..largo], &[]);
                dashos::cerrar(ll[0]);
                usadas = 1;
            }
        } else if marca != u64::MAX && marca >> 63 == 0 && k >= 1 {
            let respuesta = ll[0];
            usadas = 1;
            let orden = u32_en(0);
            let lector = sv.es(marca, LECTORES);
            match orden {
                AVISAR | AVISAR_CON => {
                    let escucha = (orden == AVISAR_CON && k >= 2).then(|| ll[1]);
                    if escucha.is_some() {
                        usadas = 2;
                    }
                    let (estado, numero) = sv.avisar(marca, m, escucha);
                    // Si no se quedó con la llave de oír (no se guardó el aviso), se cierra.
                    if estado != BIEN && let Some(e) = escucha {
                        dashos::cerrar(e);
                    }
                    let mut r = [0u8; RESPUESTA_AVISAR];
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    r[4..8].copy_from_slice(&numero.to_le_bytes());
                    if let Some(q) = sv.nombre(marca) {
                        r[8..24].copy_from_slice(&q.nombre);
                    }
                    dashos::canal_escribir(respuesta, &r, &[]);
                    sv.caducar();
                }
                NO_MOLESTAR | ESTADO if sv.es(marca, NO_MOLESTAN) => {
                    let ahora = dashos::ahora_us();
                    if orden == NO_MOLESTAR {
                        let (puesto, minutos) = (u32_en(4) != 0, u32_en(8));
                        sv.no_molestar.poner(puesto, minutos, ahora);
                        sv.version = sv.version.wrapping_add(1);
                        let quien = sv.nombre(marca).map_or([0; 16], |q| q.nombre);
                        match (puesto, minutos) {
                            (false, _) => println!("  [avisos] no molestar: quitado (lo pide {})", nombre16(&quien)),
                            (true, 0) => println!("  [avisos] no molestar: puesto hasta que se quite (lo pide {})", nombre16(&quien)),
                            (true, m) => println!("  [avisos] no molestar: puesto {} minutos (lo pide {})", m, nombre16(&quien)),
                        }
                        sv.pendiente = false;
                        sv.tocar();
                    }
                    let activo = sv.no_molestar.activo(ahora);
                    let mut r = [0u8; CABECERA_LEER + 8];
                    sv.cabecera(&mut r, BIEN);
                    r[16..20].copy_from_slice(&(activo as u32).to_le_bytes());
                    r[20..24].copy_from_slice(&sv.no_molestar.quedan_s(ahora).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                AJUSTES if sv.es(marca, AJUSTAN) => {
                    let largo = u32_en(4) as usize;
                    let estado = match m.get(8..8 + largo).filter(|x| x.len() == n - 8 && largo <= MAX_AJUSTES).map(avisos_ajustes::Ajustes::leer) {
                        Some(Some(a)) => {
                            sv.ajustes[..largo].copy_from_slice(&m[8..8 + largo]);
                            sv.largo_ajustes = largo;
                            sv.historial_si = a.historial;
                            sv.dias_historial = a.dias_historial as u32;
                            sv.version = sv.version.wrapping_add(1);
                            println!("  [avisos] ajustes (de la barra): emergentes {}, animación {} ms, barra de tiempo {}, historial {} días",
                                a.posicion.nombre(), a.animacion_ms, if a.barra_tiempo { "sí" } else { "no" }, a.dias_historial);
                            sv.caducar();
                            sv.pendiente = false;
                            sv.tocar();
                            BIEN
                        }
                        _ => MAL,
                    };
                    let mut r = [0u8; CABECERA_LEER];
                    sv.cabecera(&mut r, estado);
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                SUSCRIBIR | LEER | LEIDOS | BORRAR_TODO | ACCION | LEER_ACCIONES | DESCARTAR | QUITAR | DESCARTAR_TODO | LEER_AJUSTES if lector => {
                    let mut r = [0u8; MAX_DATOS];
                    let (d, nd) = (sv.detras, sv.detras().len());
                    let detras = &d[..nd];
                    let mut largo = CABECERA_LEER;
                    let estado = match orden {
                        SUSCRIBIR if k >= 2 => {
                            if sv.timbre != 0 {
                                dashos::cerrar(sv.timbre);
                            }
                            sv.timbre = ll[1];
                            sv.pendiente = false;
                            usadas = 2;
                            let (total, sin) = sv.historial.cuenta(detras);
                            println!("  [avisos] la barra se suscribe: {} avisos, {} sin leer", total, sin);
                            BIEN
                        }
                        SUSCRIBIR => MAL,
                        LEER => {
                            sv.pendiente = false;
                            match sv.historial.anterior(u32_en(4), detras, u32_en(8) == 1) {
                                Some(a) => {
                                    largo += a.escribir(&mut r[CABECERA_LEER..]);
                                    BIEN
                                }
                                None => NO_HAY,
                            }
                        }
                        LEER_ACCIONES => match sv.historial.buscar(u32_en(4), detras) {
                            Some(a) => {
                                largo += a.escribir_acciones(&mut r[CABECERA_LEER..]);
                                BIEN
                            }
                            None => NO_HAY,
                        },
                        LEER_AJUSTES => {
                            sv.pendiente = false;
                            r[16..20].copy_from_slice(&(sv.largo_ajustes as u32).to_le_bytes());
                            r[20..20 + sv.largo_ajustes].copy_from_slice(&sv.ajustes[..sv.largo_ajustes]);
                            largo += 4 + sv.largo_ajustes;
                            BIEN
                        }
                        ACCION => sv.accion(u32_en(4), u32_en(8)),
                        DESCARTAR => if sv.descartar(u32_en(4)) { BIEN } else { NO_HAY },
                        QUITAR => if sv.historial.quitar(u32_en(4), detras) { BIEN } else { NO_HAY },
                        _ => {
                            sv.pendiente = false;
                            match orden {
                                LEIDOS => sv.historial.leidos(detras),
                                DESCARTAR_TODO if sv.historial_si => sv.historial.descartar_visibles(detras),
                                _ => {
                                    sv.historial.borrar_visibles(detras);
                                    println!("  [avisos] la barra borra el historial; quedan {} guardados de sesiones de detrás",
                                        detras.iter().map(|&u| sv.historial.de_usuario(u)).sum::<usize>());
                                }
                            }
                            BIEN
                        }
                    };
                    sv.cabecera(&mut r, estado);
                    dashos::canal_escribir(respuesta, &r[..largo], &[]);
                }
                SUSCRIBIR | LEER | LEIDOS | BORRAR_TODO | ACCION | LEER_ACCIONES | DESCARTAR | QUITAR | DESCARTAR_TODO | LEER_AJUSTES | NO_MOLESTAR | ESTADO | AJUSTES => {
                    let mut r = [0u8; CABECERA_LEER];
                    r[0..4].copy_from_slice(&PROTEGIDO.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                _ => {
                    dashos::canal_escribir(respuesta, &MAL.to_le_bytes(), &[]);
                }
            }
        }
        cerrar_llaves(&ll[usadas..k]);
    }
}
