//! S10: el servicio de avisos (las notificaciones). Los programas con su llave mandan avisos; el servicio los guarda en
//! un historial acotado y toca el timbre de la barra, que los enseña. Protocolo e historial en
//! `usuario/src/protocolo/avisos.rs`.
//!
//! - Quién avisa lo pone el servicio, por la marca: el nombre que le dio `inicio` (entrada de directorio) o `sesiones`
//!   (`AVISO_PERSONA`, antes de lanzar un programa del disco). Solo valen sin marca: un programa no se cambia el nombre.
//!   Un programa que el servicio no conoce no avisa.
//! - Un programa que inunda: como mucho `RITMO` avisos por minuto, y `POR_PROGRAMA` en el historial. No tapa los de los
//!   demás ni le gasta memoria al servicio (todo es de tamaño fijo).
//! - U6: los avisos de una persona con la sesión detrás (`AVISO_DETRAS`, de `sesiones`) se guardan sin enseñarlos. Al
//!   volver ella delante salen en el historial, sin leer y marcados como retenidos (la barra no los saca como toast).
//! - Solo la barra (el nombre `barra` que da `inicio`) se suscribe, lee el historial, lo marca como leído o lo borra.
//!
//! Llave 1: el extremo de lectura de su canal "avisos".

#![no_std]
#![no_main]

#[path = "../lanzables.rs"]
#[allow(dead_code)]
mod lanzables;

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

#[derive(Clone, Copy)]
struct Nombre {
    marca: u64,
    uid: u32,
    nombre: [u8; 16],
    /// Si ya se dijo que inunda (para no repetirlo en cada aviso rechazado).
    inunda: bool,
}

struct Servicio {
    nombres: [Option<Nombre>; MAX_NOMBRES],
    detras: [u32; s::MAX_DETRAS],
    historial: Historial,
    ritmo: Ritmo,
    /// La llave del timbre de la barra y si ya hay uno sin atender.
    timbre: u32,
    pendiente: bool,
}

impl Servicio {
    fn nombre(&self, marca: u64) -> Option<Nombre> {
        self.nombres.iter().flatten().find(|n| n.marca == marca).copied()
    }

    /// Apunta (o cambia) el nombre de `marca`. Si no cabe, se va uno de lo lanzado del disco.
    fn apuntar(&mut self, marca: u64, uid: Option<u32>, nombre: &[u8]) {
        let mut n16 = [0u8; 16];
        let l = limpiar(nombre.split(|&c| c == 0).next().unwrap_or(b""), &mut n16);
        n16[l..].fill(0);
        if let Some(n) = self.nombres.iter_mut().flatten().find(|n| n.marca == marca) {
            n.nombre = n16;
            if let Some(u) = uid {
                n.uid = u;
            }
            return;
        }
        let hueco = self.nombres.iter().position(|x| x.is_none())
            .or_else(|| self.nombres.iter().position(|x| x.is_some_and(|n| n.marca >= lanzables::PRIMER_PID_PROPIO)));
        if let Some(i) = hueco {
            self.nombres[i] = Some(Nombre { marca, uid: uid.unwrap_or(1), nombre: n16, inunda: false });
        }
    }

    fn es_lector(&self, marca: u64) -> bool {
        self.nombre(marca).is_some_and(|n| lanzables::del_sistema(marca, nombre16(&n.nombre), LECTORES))
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

    /// La cabecera de las respuestas a la barra: estado, sin leer y visibles.
    fn cabecera(&self, r: &mut [u8], estado: i32) {
        let (total, sin) = self.historial.cuenta(self.detras());
        r[0..4].copy_from_slice(&estado.to_le_bytes());
        r[4..8].copy_from_slice(&sin.to_le_bytes());
        r[8..12].copy_from_slice(&total.to_le_bytes());
    }
}

fn principal() -> u64 {
    let mut sv = Servicio {
        nombres: [None; MAX_NOMBRES],
        detras: [0; s::MAX_DETRAS],
        historial: Historial::new(),
        ritmo: Ritmo::new(),
        timbre: 0,
        pendiente: false,
    };
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
                    // Su baja: si es de lo lanzado, se olvida (sus avisos guardan su nombre).
                    if de >= lanzables::PRIMER_PID_PROPIO {
                        sv.nombres.iter_mut().filter(|x| x.is_some_and(|n| n.marca == de)).for_each(|x| *x = None);
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
            }
        } else if marca != u64::MAX && marca >> 63 == 0 && k >= 1 {
            let respuesta = ll[0];
            usadas = 1;
            match u32_en(0) {
                AVISAR => {
                    let mut r = [0u8; RESPUESTA_AVISAR];
                    let quien = sv.nombre(marca);
                    let estado = match (quien, leer_aviso(m)) {
                        (None, _) => PROTEGIDO,
                        (Some(_), Err(e)) => e,
                        (Some(q), Ok(_)) if !sv.ritmo.puede(marca, dashos::ahora_us()) => {
                            if !q.inunda {
                                println!("  [avisos] {}: demasiados avisos seguidos; los siguientes no se guardan", nombre16(&q.nombre));
                                sv.nombres.iter_mut().flatten().filter(|n| n.marca == marca).for_each(|n| n.inunda = true);
                            }
                            DEMASIADOS
                        }
                        (Some(q), Ok((urgencia, titulo, texto))) => {
                            sv.nombres.iter_mut().flatten().filter(|n| n.marca == marca).for_each(|n| n.inunda = false);
                            let mut a = Aviso::vacio();
                            a.urgencia = urgencia;
                            a.marca = marca;
                            a.uid = q.uid;
                            a.sistema = marca < lanzables::PRIMER_PID_PROPIO;
                            a.de = q.nombre;
                            a.hora = dashos::hora_ns();
                            a.retenido = !visible(q.uid, sv.detras());
                            a.lt = limpiar(titulo, &mut a.titulo) as u8;
                            a.lx = limpiar(texto, &mut a.texto) as u8;
                            let numero = sv.historial.meter(a);
                            r[4..8].copy_from_slice(&numero.to_le_bytes());
                            if visible(q.uid, sv.detras()) {
                                println!("  [avisos] {} avisa: \"{}\" (número {})", nombre16(&q.nombre), core::str::from_utf8(&a.titulo[..a.lt as usize]).unwrap_or("?"), numero);
                                sv.tocar();
                            } else {
                                println!("  [avisos] {} avisa con su sesión detrás (usuario {}): se guarda para cuando vuelva", nombre16(&q.nombre), q.uid);
                            }
                            BIEN
                        }
                    };
                    if let Some(q) = quien {
                        r[8..24].copy_from_slice(&q.nombre);
                    }
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                SUSCRIBIR | LEER | LEIDOS | BORRAR_TODO if !sv.es_lector(marca) => {
                    let mut r = [0u8; CABECERA_LEER];
                    r[0..4].copy_from_slice(&PROTEGIDO.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                SUSCRIBIR if k >= 2 => {
                    if sv.timbre != 0 {
                        dashos::cerrar(sv.timbre);
                    }
                    sv.timbre = ll[1];
                    sv.pendiente = false;
                    usadas = 2;
                    let mut r = [0u8; CABECERA_LEER];
                    sv.cabecera(&mut r, BIEN);
                    println!("  [avisos] la barra se suscribe: {} avisos, {} sin leer", u32_en_r(&r, 8), u32_en_r(&r, 4));
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                LEER => {
                    sv.pendiente = false;
                    let mut r = [0u8; MAX_DATOS];
                    let antes = u32_en(4);
                    let largo = match sv.historial.anterior(antes, sv.detras()) {
                        Some(a) => {
                            sv.cabecera(&mut r, BIEN);
                            CABECERA_LEER + a.escribir(&mut r[CABECERA_LEER..])
                        }
                        None => {
                            sv.cabecera(&mut r, NO_HAY);
                            CABECERA_LEER
                        }
                    };
                    dashos::canal_escribir(respuesta, &r[..largo], &[]);
                }
                LEIDOS | BORRAR_TODO => {
                    sv.pendiente = false;
                    let (d, nd) = (sv.detras, sv.detras().len());
                    if u32_en(0) == LEIDOS {
                        sv.historial.leidos(&d[..nd]);
                    } else {
                        sv.historial.borrar_visibles(&d[..nd]);
                        println!("  [avisos] la barra borra el historial; quedan {} guardados de sesiones de detrás",
                            d[..nd].iter().map(|&u| sv.historial.de_usuario(u)).sum::<usize>());
                    }
                    let mut r = [0u8; CABECERA_LEER];
                    sv.cabecera(&mut r, BIEN);
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

fn u32_en_r(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[i..i + 4].try_into().unwrap())
}
