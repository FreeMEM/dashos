//! Los avisos de cambio del servicio de archivos (como `StartNotify` del Amiga; S4b, "los servicios vigilan `ENV:`";
//! diseño en `docs/bitacoras/ajustes-texto.md`): quién vigila qué, qué ha cambiado y a quién hay que tocar el timbre.
//! El servicio (`bin/archivos.rs`) lo incluye y hace lo que sabe del disco y de los canales; esto, lo demás.
//!
//! Lo que promete:
//!
//! - **No se pierde ningún cambio.** Cada vigilancia cuenta los cambios desde la última vez que su cliente los pidió
//!   (`AVISOS`). El timbre (un mensaje corto por la puerta que dio el cliente) dice "hay algo"; los cambios se piden
//!   después. Un cambio que llega tras pedirlos vuelve a tocar el timbre. Quien pide y luego lee el fichero ve siempre
//!   lo último.
//! - **No se desborda nada.** Cada vigilancia tiene como mucho un timbre por contestar: mil cambios seguidos son un
//!   timbre y una cuenta de mil. Lo que espera un cliente es un número por vigilancia, no una cola. Si su canal está
//!   lleno, el timbre se guarda y se reintenta; si ya no lee nadie (el canal se cerró), la vigilancia se quita.
//! - **Topes**: `MAX_POR_CLIENTE` vigilancias por cliente. Con `MAX_CLIENTES` clientes, como mucho
//!   `MAX_POR_CLIENTE × MAX_CLIENTES` en todo el servicio: uno que pide miles no deja sin sitio a los demás.
//! - Lo vigilado que se borra (o el volumen que se quita o vuelve a una instantánea) avisa una última vez con su marca
//!   (`BORRADO`, `FUERA`) y la vigilancia acaba: el número de nodo puede volver a usarse para otra cosa.
//!
//! Qué cuenta como cambio: de un fichero, escribir, recortar, comentar, proteger, fechar, renombrarlo (`MOVIDO`) o
//! borrarlo; de un cajón, crear, borrar o renombrar algo en él, y los cambios de los ficheros que tiene dentro (de
//! los abiertos desde su cajón: el servicio sabe de qué cajón es cada asa que dio al buscar o crear).
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/vigilancia.rs`.

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

/// Lo más de vigilancias de un cliente.
pub const MAX_POR_CLIENTE: usize = 64;
/// Lo que mide una entrada de la respuesta de `AVISOS`: número (u32), marcas (u32) y cambios (u64).
pub const ENTRADA: usize = 16;
/// Lo que mide el timbre: `TIMBRE` (u32), el número de la vigilancia (u32) y cuántos cambios lleva (u64).
pub const LARGO_TIMBRE: usize = 16;
/// El primer u32 del timbre (`VIGI`), para quien lo recibe en un canal donde llegan más cosas.
pub const TIMBRE: u32 = u32::from_le_bytes(*b"VIGI");

/// Las marcas de una entrada de `AVISOS`.
pub const CAMBIO: u32 = 1;
/// Lo vigilado se borró: la vigilancia acaba.
pub const BORRADO: u32 = 2;
/// El volumen se quitó, se cerró o volvió a una instantánea: la vigilancia acaba (hay que vigilar de nuevo).
pub const FUERA: u32 = 4;
/// El fichero o el cajón vigilado cambió de nombre o de sitio (sigue vigilado).
pub const MOVIDO: u32 = 8;

/// Qué le pasó a un nodo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cambio {
    /// Su contenido, sus datos o (si es un cajón) lo que tiene dentro.
    Cambio,
    /// Cambió de nombre o de sitio.
    Movido,
    /// Se borró.
    Borrado,
}

/// Lo que dijo el canal al tocar un timbre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Envio {
    Hecho,
    /// Lleno: se reintenta.
    Lleno,
    /// Nadie lee ya: la vigilancia se quita.
    Cerrado,
}

/// Por qué no se puede vigilar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoSePuede {
    /// El cliente ya tiene `MAX_POR_CLIENTE`.
    SinSitio,
}

#[derive(Clone, Debug)]
struct Vigilancia {
    vol: u64,
    nodo: u32,
    /// La llave por la que se toca el timbre (de quien vigila; la cierra quien la quita).
    llave: u32,
    cambios: u64,
    marcas: u32,
    /// Se tocó el timbre y aún no ha pedido los cambios.
    sonado: bool,
    /// Acabó (borrado o fuera): ya no está en `por_nodo` y se suelta al dar sus cambios.
    acabada: bool,
}

/// Una vigilancia: (marca del cliente, su número).
type Clave = (u64, u32);

#[derive(Default)]
pub struct Vigilancias {
    todas: BTreeMap<Clave, Vigilancia>,
    /// (volumen, nodo) → quién lo vigila.
    por_nodo: BTreeMap<(u64, u32), Vec<Clave>>,
    /// Las que tienen que tocar el timbre (o reintentarlo).
    por_tocar: BTreeSet<Clave>,
    /// El siguiente número de cada cliente.
    siguiente: BTreeMap<u64, u32>,
}

impl Vigilancias {
    pub fn nuevas() -> Vigilancias {
        Vigilancias::default()
    }

    /// Cuántas tiene el cliente `marca`.
    pub fn de(&self, marca: u64) -> usize {
        self.todas.range((marca, 0)..=(marca, u32::MAX)).count()
    }

    /// Cuántas hay en todo el servicio.
    pub fn total(&self) -> usize {
        self.todas.len()
    }

    /// Si hay timbres que tocar.
    pub fn hay_por_tocar(&self) -> bool {
        !self.por_tocar.is_empty()
    }

    /// Si alguien vigila algo en el volumen `vol` (para no buscar nada cuando no hace falta).
    pub fn en_volumen(&self, vol: u64) -> bool {
        self.por_nodo.range((vol, 0)..=(vol, u32::MAX)).next().is_some()
    }

    /// Que el cliente `marca` vigile el nodo `nodo` del volumen `vol`, tocando el timbre por `llave`. Su número.
    pub fn vigilar(&mut self, marca: u64, vol: u64, nodo: u32, llave: u32) -> Result<u32, NoSePuede> {
        if self.de(marca) >= MAX_POR_CLIENTE {
            return Err(NoSePuede::SinSitio);
        }
        let s = self.siguiente.entry(marca).or_insert(1);
        // Un número que no tenga ya (tras dar la vuelta, los viejos pueden seguir).
        while self.todas.contains_key(&(marca, *s)) || *s == 0 {
            *s = s.wrapping_add(1);
        }
        let id = *s;
        *s = s.wrapping_add(1);
        self.todas.insert((marca, id), Vigilancia { vol, nodo, llave, cambios: 0, marcas: 0, sonado: false, acabada: false });
        self.por_nodo.entry((vol, nodo)).or_default().push((marca, id));
        Ok(id)
    }

    fn quitar_del_nodo(&mut self, clave: Clave, vol: u64, nodo: u32) {
        if let Some(v) = self.por_nodo.get_mut(&(vol, nodo)) {
            v.retain(|c| *c != clave);
            if v.is_empty() {
                self.por_nodo.remove(&(vol, nodo));
            }
        }
    }

    /// Deja de vigilar: la llave, para cerrarla. `None` si no era suya.
    pub fn dejar(&mut self, marca: u64, id: u32) -> Option<u32> {
        let v = self.todas.remove(&(marca, id))?;
        if !v.acabada {
            self.quitar_del_nodo((marca, id), v.vol, v.nodo);
        }
        self.por_tocar.remove(&(marca, id));
        Some(v.llave)
    }

    /// El cliente se fue: todas sus llaves, para cerrarlas.
    pub fn soltar(&mut self, marca: u64) -> Vec<u32> {
        let ids: Vec<u32> = self.todas.range((marca, 0)..=(marca, u32::MAX)).map(|(c, _)| c.1).collect();
        self.siguiente.remove(&marca);
        ids.into_iter().filter_map(|id| self.dejar(marca, id)).collect()
    }

    fn apuntar(&mut self, clave: Clave, marcas: u32, acaba: bool) {
        let Some(v) = self.todas.get_mut(&clave) else { return };
        if v.acabada {
            return;
        }
        v.cambios = v.cambios.saturating_add(1);
        v.marcas |= marcas;
        let (vol, nodo) = (v.vol, v.nodo);
        if acaba {
            v.acabada = true;
            self.quitar_del_nodo(clave, vol, nodo);
        }
        if !self.todas[&clave].sonado {
            self.por_tocar.insert(clave);
        }
    }

    /// Le pasó `que` al nodo `nodo` del volumen `vol`.
    pub fn cambio(&mut self, vol: u64, nodo: u32, que: Cambio) {
        let Some(quienes) = self.por_nodo.get(&(vol, nodo)) else { return };
        let (marcas, acaba) = match que {
            Cambio::Cambio => (CAMBIO, false),
            Cambio::Movido => (MOVIDO, false),
            Cambio::Borrado => (BORRADO, true),
        };
        for clave in quienes.clone() {
            self.apuntar(clave, marcas, acaba);
        }
    }

    /// Todo lo de `vol` cambió (se copió un árbol entero en él): un cambio en cada vigilancia de ese volumen.
    pub fn todo_cambio(&mut self, vol: u64) {
        let claves: Vec<Clave> = self.por_nodo.range((vol, 0)..=(vol, u32::MAX)).flat_map(|(_, v)| v.iter().copied()).collect();
        for c in claves {
            self.apuntar(c, CAMBIO, false);
        }
    }

    /// El volumen `vol` ya no es el que era (quitado, cerrado o vuelto a una instantánea): todas sus vigilancias
    /// acaban con `FUERA`.
    pub fn fuera(&mut self, vol: u64) {
        let claves: Vec<Clave> = self.por_nodo.range((vol, 0)..=(vol, u32::MAX)).flat_map(|(_, v)| v.iter().copied()).collect();
        for c in claves {
            self.apuntar(c, FUERA, true);
        }
    }

    /// Toca los timbres pendientes con `enviar(llave, mensaje)`. Devuelve las llaves de las vigilancias que se quitaron
    /// porque ya no lee nadie (para cerrarlas). Las que encontraron el canal lleno se quedan para la próxima vez.
    pub fn tocar(&mut self, mut enviar: impl FnMut(u32, &[u8; LARGO_TIMBRE]) -> Envio) -> Vec<u32> {
        let mut cerradas = Vec::new();
        let pendientes: Vec<Clave> = self.por_tocar.iter().copied().collect();
        for clave in pendientes {
            let Some(v) = self.todas.get(&clave) else {
                self.por_tocar.remove(&clave);
                continue;
            };
            let mut m = [0u8; LARGO_TIMBRE];
            m[0..4].copy_from_slice(&TIMBRE.to_le_bytes());
            m[4..8].copy_from_slice(&clave.1.to_le_bytes());
            m[8..16].copy_from_slice(&v.cambios.to_le_bytes());
            match enviar(v.llave, &m) {
                Envio::Hecho => {
                    self.por_tocar.remove(&clave);
                    self.todas.get_mut(&clave).unwrap().sonado = true;
                }
                Envio::Lleno => {}
                Envio::Cerrado => {
                    if let Some(l) = self.dejar(clave.0, clave.1) {
                        cerradas.push(l);
                    }
                }
            }
        }
        cerradas
    }

    /// Los cambios del cliente `marca`, en `sal` (entradas de `ENTRADA` bytes: número, marcas y cuántos), y se ponen a
    /// cero: el siguiente cambio vuelve a tocar el timbre. Las acabadas se dan una vez y se sueltan (sus llaves, para
    /// cerrar, en `cerrar`). Devuelve cuántas entradas y si quedaron más por dar (no cabían).
    pub fn avisos(&mut self, marca: u64, sal: &mut [u8], cerrar: &mut Vec<u32>) -> (usize, bool) {
        let cabe = sal.len() / ENTRADA;
        let con: Vec<u32> = self.todas.range((marca, 0)..=(marca, u32::MAX)).filter(|(_, v)| v.cambios > 0).map(|(c, _)| c.1).collect();
        let mut n = 0;
        for &id in con.iter().take(cabe) {
            let v = self.todas.get_mut(&(marca, id)).unwrap();
            let o = n * ENTRADA;
            sal[o..o + 4].copy_from_slice(&id.to_le_bytes());
            sal[o + 4..o + 8].copy_from_slice(&v.marcas.to_le_bytes());
            sal[o + 8..o + 16].copy_from_slice(&v.cambios.to_le_bytes());
            v.cambios = 0;
            v.marcas = 0;
            v.sonado = false;
            let acabada = v.acabada;
            self.por_tocar.remove(&(marca, id));
            if acabada && let Some(l) = self.dejar(marca, id) {
                cerrar.push(l);
            }
            n += 1;
        }
        // Las que sonaron y no tienen nada (alguien ya las pidió): pueden volver a sonar.
        for (_, v) in self.todas.range_mut((marca, 0)..=(marca, u32::MAX)) {
            if v.cambios == 0 {
                v.sonado = false;
            }
        }
        (n, con.len() > cabe)
    }
}

/// Lee una entrada de `AVISOS`: (número, marcas, cambios).
pub fn entrada(b: &[u8]) -> Option<(u32, u32, u64)> {
    (b.len() >= ENTRADA).then(|| (u32::from_le_bytes(b[0..4].try_into().unwrap()), u32::from_le_bytes(b[4..8].try_into().unwrap()), u64::from_le_bytes(b[8..16].try_into().unwrap())))
}

/// Lee un timbre: (número de la vigilancia, cambios que llevaba). `None` si no es un timbre.
pub fn timbre(b: &[u8]) -> Option<(u32, u64)> {
    (b.len() >= LARGO_TIMBRE && u32::from_le_bytes(b[0..4].try_into().unwrap()) == TIMBRE)
        .then(|| (u32::from_le_bytes(b[4..8].try_into().unwrap()), u64::from_le_bytes(b[8..16].try_into().unwrap())))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// Toca los timbres y devuelve los que salieron: (llave, número, cambios).
    fn tocar(v: &mut Vigilancias) -> Vec<(u32, u32, u64)> {
        let mut salieron = Vec::new();
        let cerradas = v.tocar(|l, m| {
            let (id, n) = timbre(m).unwrap();
            salieron.push((l, id, n));
            Envio::Hecho
        });
        assert!(cerradas.is_empty());
        salieron
    }

    fn avisos(v: &mut Vigilancias, marca: u64) -> Vec<(u32, u32, u64)> {
        let mut sal = [0u8; 64 * ENTRADA];
        let mut cerrar = Vec::new();
        let (n, mas) = v.avisos(marca, &mut sal, &mut cerrar);
        assert!(!mas);
        (0..n).map(|i| entrada(&sal[i * ENTRADA..]).unwrap()).collect()
    }

    #[test]
    fn rafaga_un_timbre_y_la_cuenta_entera() {
        let mut v = Vigilancias::nuevas();
        let a = v.vigilar(7, 0, 100, 55).unwrap();
        for _ in 0..1000 {
            v.cambio(0, 100, Cambio::Cambio);
        }
        // Un nodo que nadie vigila no hace nada.
        v.cambio(0, 101, Cambio::Cambio);
        v.cambio(1, 100, Cambio::Cambio);
        assert_eq!(tocar(&mut v), [(55, a, 1000)]);
        // Más cambios sin pedir: sin timbre nuevo, pero se cuentan.
        for _ in 0..500 {
            v.cambio(0, 100, Cambio::Cambio);
        }
        assert!(tocar(&mut v).is_empty());
        assert_eq!(avisos(&mut v, 7), [(a, CAMBIO, 1500)]);
        assert!(avisos(&mut v, 7).is_empty());
        // Tras pedirlos, el siguiente cambio vuelve a sonar.
        v.cambio(0, 100, Cambio::Cambio);
        assert_eq!(tocar(&mut v), [(55, a, 1)]);
    }

    #[test]
    fn canal_lleno_y_cerrado() {
        let mut v = Vigilancias::nuevas();
        let a = v.vigilar(7, 0, 100, 55).unwrap();
        let b = v.vigilar(8, 0, 100, 66).unwrap();
        v.cambio(0, 100, Cambio::Cambio);
        // El de 7 está lleno; el de 8 ya no lee nadie.
        let cerradas = v.tocar(|l, _| if l == 55 { Envio::Lleno } else { Envio::Cerrado });
        assert_eq!(cerradas, [66]);
        assert_eq!((v.de(7), v.de(8)), (1, 0));
        assert!(v.hay_por_tocar());
        // Se reintenta y sale, con lo que lleve para entonces.
        v.cambio(0, 100, Cambio::Cambio);
        assert_eq!(tocar(&mut v), [(55, a, 2)]);
        assert!(!v.hay_por_tocar());
        let _ = b;
    }

    #[test]
    fn borrado_y_fuera_acaban() {
        let mut v = Vigilancias::nuevas();
        let a = v.vigilar(7, 0, 100, 55).unwrap();
        let b = v.vigilar(7, 3, 200, 55).unwrap();
        let c = v.vigilar(7, 3, 201, 55).unwrap();
        v.cambio(0, 100, Cambio::Movido);
        v.cambio(0, 100, Cambio::Borrado);
        // Tras borrarlo, el número se puede usar para otro fichero: no avisa.
        v.cambio(0, 100, Cambio::Cambio);
        v.fuera(3);
        assert!(!v.en_volumen(3) && !v.en_volumen(0));
        assert_eq!(tocar(&mut v).len(), 3);
        let mut sal = [0u8; 4 * ENTRADA];
        let mut cerrar = Vec::new();
        let (n, _) = v.avisos(7, &mut sal, &mut cerrar);
        let e: Vec<_> = (0..n).map(|i| entrada(&sal[i * ENTRADA..]).unwrap()).collect();
        assert_eq!(e, [(a, MOVIDO | BORRADO, 2), (b, FUERA, 1), (c, FUERA, 1)]);
        assert_eq!(cerrar, [55, 55, 55]);
        assert_eq!(v.de(7), 0);
    }

    #[test]
    fn todo_cambio_y_dejar() {
        let mut v = Vigilancias::nuevas();
        let a = v.vigilar(7, 3, 1, 55).unwrap();
        let b = v.vigilar(7, 3, 2, 55).unwrap();
        v.todo_cambio(3);
        assert_eq!(avisos(&mut v, 7), [(a, CAMBIO, 1), (b, CAMBIO, 1)]);
        assert_eq!(v.dejar(7, a), Some(55));
        assert_eq!(v.dejar(7, a), None);
        // De otro cliente no se deja.
        assert_eq!(v.dejar(8, b), None);
        v.cambio(3, 1, Cambio::Cambio);
        assert!(!v.hay_por_tocar());
    }

    /// Hostil: uno que pide miles se queda en su tope; los demás siguen pudiendo; al irse suelta todo.
    #[test]
    fn miles_y_el_que_muere() {
        let mut v = Vigilancias::nuevas();
        let mut bien = 0;
        let mut mal = 0;
        for i in 0..5000u32 {
            match v.vigilar(7, 0, i, 55) {
                Ok(_) => bien += 1,
                Err(NoSePuede::SinSitio) => mal += 1,
            }
        }
        assert_eq!((bien, mal), (MAX_POR_CLIENTE, 5000 - MAX_POR_CLIENTE));
        assert!(v.vigilar(8, 0, 1, 66).is_ok());
        for i in 0..5000u32 {
            v.cambio(0, i, Cambio::Cambio);
        }
        // Un timbre por vigilancia, no por cambio.
        assert_eq!(tocar(&mut v).len(), MAX_POR_CLIENTE + 1);
        let llaves = v.soltar(7);
        assert_eq!(llaves.len(), MAX_POR_CLIENTE);
        assert_eq!((v.de(7), v.total()), (0, 1));
        // Un número de vigilancia no se repite mientras siga viva otra con él (tras dar la vuelta).
        let mut w = Vigilancias::nuevas();
        let x = w.vigilar(9, 0, 1, 1).unwrap();
        w.siguiente.insert(9, x);
        let y = w.vigilar(9, 0, 2, 1).unwrap();
        assert_ne!(x, y);
        // Lo que no cabe en la respuesta queda para la siguiente.
        let mut z = Vigilancias::nuevas();
        for i in 0..10 {
            z.vigilar(9, 0, i, 1).unwrap();
            z.cambio(0, i, Cambio::Cambio);
        }
        let mut sal = [0u8; 3 * ENTRADA];
        let mut cerrar = Vec::new();
        assert_eq!(z.avisos(9, &mut sal, &mut cerrar), (3, true));
        assert_eq!(z.avisos(9, &mut [0u8; 64 * ENTRADA], &mut cerrar), (7, false));
        assert!(timbre(&[0u8; 16]).is_none() && timbre(&[1u8; 3]).is_none());
    }
}
