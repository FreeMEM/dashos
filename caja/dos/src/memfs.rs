//! Un sistema de archivos en memoria con el comportamiento del servicio `archivos`, para las pruebas (y quizá para
//! `RAM:` dentro de la caja). Lo que imita:
//!
//! - Asas: 0 a 15 son raíces (una por volumen hecho con `nuevo_volumen`); las demás, desde la 16; hasta `max_asas`
//!   abiertas (si no, `SinSitio`). Por encima de una raíz no se llega: no hay "cajón padre".
//! - Nombres sin distinguir mayúsculas, guardados como se escribieron. Un nombre vacío, `.`, `..`, con `/`, `:` o un
//!   0, o de más de 224 bytes, es `MalFormado`.
//! - Lo borrado mientras alguien lo tiene abierto se sigue leyendo por esa asa hasta que se cierra.
//! - Permisos de Unix del dueño (leer 0o400, escribir 0o200) y "no borrar"; volúmenes de solo lectura; disco lleno.
//! - Escribir o cambiar el tamaño quita el bit de archivado.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::fs::*;

const POR_PAGINA: usize = 8;

#[derive(Clone, Debug)]
struct Nodo {
    nombre: String,
    padre: usize,
    cajon: bool,
    datos: Vec<u8>,
    hijos: Vec<usize>,
    modo: u16,
    amiga: u16,
    comentario: String,
    fecha: u64,
    /// Sigue en su cajón (no se ha borrado).
    enlazado: bool,
    vol: usize,
}

#[derive(Clone, Debug)]
struct VolMem {
    solo_lectura: bool,
    tamano: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MemFs {
    nodos: Vec<Nodo>,
    vols: Vec<VolMem>,
    asas: BTreeMap<Asa, usize>,
    siguiente: Asa,
    /// Asas abiertas a la vez como mucho (sin contar las raíces). El servicio da 256.
    pub max_asas: usize,
    /// La fecha que se pone a lo que se crea o escribe (segundos desde 1970).
    pub reloj: u64,
    /// Cuántas órdenes ha recibido, por si una prueba quiere contarlas.
    pub ordenes: u64,
}

fn clave(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}

fn nombre_valido(n: &str) -> bool {
    !(n.is_empty() || n == "." || n == ".." || n.len() > LARGO_CAMINO || n.contains(['/', ':', '\0']))
}

impl MemFs {
    pub fn nuevo() -> MemFs {
        MemFs { siguiente: 16, max_asas: 256, reloj: 1_700_000_000, ..Default::default() }
    }

    /// Hace un volumen y devuelve su raíz (una asa de 0 a 15).
    pub fn nuevo_volumen(&mut self, solo_lectura: bool, tamano: u64) -> Asa {
        let vol = self.vols.len();
        let n = self.nodos.len();
        self.nodos.push(Nodo {
            nombre: String::new(),
            padre: n,
            cajon: true,
            datos: Vec::new(),
            hijos: Vec::new(),
            modo: 0o755,
            amiga: 0,
            comentario: String::new(),
            fecha: self.reloj,
            enlazado: true,
            vol,
        });
        self.vols.push(VolMem { solo_lectura, tamano });
        let asa = vol as Asa;
        assert!(asa < 16, "como mucho 16 raíces");
        self.asas.insert(asa, n);
        asa
    }

    pub fn solo_lectura(&mut self, raiz: Asa, si: bool) {
        if let Some(&n) = self.asas.get(&raiz) {
            let v = self.nodos[n].vol;
            self.vols[v].solo_lectura = si;
        }
    }

    /// Asas abiertas además de las raíces (para ver que dos no se deja ninguna).
    pub fn asas_abiertas(&self) -> usize {
        self.asas.keys().filter(|&&a| a >= 16).count()
    }

    // --- Para preparar y mirar las pruebas desde fuera de dos ---

    fn buscar_nodo(&self, desde: usize, camino: &str) -> Result<usize, ErrorFs> {
        let mut n = desde;
        if camino.is_empty() {
            return Ok(n);
        }
        for parte in camino.split('/') {
            if !nombre_valido(parte) {
                return Err(ErrorFs::MalFormado);
            }
            if !self.nodos[n].cajon {
                return Err(ErrorFs::NoEsCajon);
            }
            if !self.nodos[n].enlazado {
                return Err(ErrorFs::NoExiste);
            }
            let k = clave(parte);
            n = *self.nodos[n].hijos.iter().find(|&&h| clave(&self.nodos[h].nombre) == k).ok_or(ErrorFs::NoExiste)?;
        }
        Ok(n)
    }

    fn hijo_nuevo(&mut self, padre: usize, nombre: &str, cajon: bool) -> usize {
        let n = self.nodos.len();
        let vol = self.nodos[padre].vol;
        self.nodos.push(Nodo {
            nombre: nombre.to_string(),
            padre,
            cajon,
            datos: Vec::new(),
            hijos: Vec::new(),
            modo: if cajon { 0o755 } else { 0o644 },
            amiga: 0,
            comentario: String::new(),
            fecha: self.reloj,
            enlazado: true,
            vol,
        });
        self.nodos[padre].hijos.push(n);
        n
    }

    /// Pone un fichero con su contenido en `camino` desde la raíz, con los cajones que hagan falta.
    pub fn poner(&mut self, raiz: Asa, camino: &str, contenido: &[u8]) {
        let mut n = self.asas[&raiz];
        let partes: Vec<&str> = camino.split('/').collect();
        for (i, p) in partes.iter().enumerate() {
            let ultimo = i + 1 == partes.len();
            let k = clave(p);
            n = match self.nodos[n].hijos.iter().find(|&&h| clave(&self.nodos[h].nombre) == k) {
                Some(&h) => h,
                None => self.hijo_nuevo(n, p, !ultimo),
            };
        }
        self.nodos[n].datos = contenido.to_vec();
    }

    /// Pone un cajón (y los de antes) en `camino` desde la raíz.
    pub fn poner_cajon(&mut self, raiz: Asa, camino: &str) {
        let mut n = self.asas[&raiz];
        for p in camino.split('/') {
            let k = clave(p);
            n = match self.nodos[n].hijos.iter().find(|&&h| clave(&self.nodos[h].nombre) == k) {
                Some(&h) => h,
                None => self.hijo_nuevo(n, p, true),
            };
        }
    }

    /// El contenido de un fichero, si existe.
    pub fn contenido(&self, raiz: Asa, camino: &str) -> Option<Vec<u8>> {
        let n = self.buscar_nodo(*self.asas.get(&raiz)?, camino).ok()?;
        if self.nodos[n].cajon { None } else { Some(self.nodos[n].datos.clone()) }
    }

    pub fn existe(&self, raiz: Asa, camino: &str) -> bool {
        self.asas.get(&raiz).is_some_and(|&r| self.buscar_nodo(r, camino).is_ok())
    }

    /// El nombre tal como está guardado.
    pub fn nombre_guardado(&self, raiz: Asa, camino: &str) -> Option<String> {
        let n = self.buscar_nodo(*self.asas.get(&raiz)?, camino).ok()?;
        Some(self.nodos[n].nombre.clone())
    }

    /// Bits del Amiga y modo de Unix de algo.
    pub fn proteccion(&self, raiz: Asa, camino: &str) -> Option<(u16, u16)> {
        let n = self.buscar_nodo(*self.asas.get(&raiz)?, camino).ok()?;
        Some((self.nodos[n].amiga, self.nodos[n].modo))
    }

    pub fn comentario_de(&self, raiz: Asa, camino: &str) -> Option<String> {
        let n = self.buscar_nodo(*self.asas.get(&raiz)?, camino).ok()?;
        Some(self.nodos[n].comentario.clone())
    }

    /// Cambia los permisos por fuera de dos.
    pub fn poner_proteccion(&mut self, raiz: Asa, camino: &str, amiga: u16, modo: u16) {
        let n = self.buscar_nodo(self.asas[&raiz], camino).unwrap();
        self.nodos[n].amiga = amiga;
        self.nodos[n].modo = modo;
    }

    /// Borra algo por fuera de dos (otro programa de DashOS), aunque tenga asas abiertas o hijos.
    pub fn borrar_por_fuera(&mut self, raiz: Asa, camino: &str) {
        if let Ok(n) = self.buscar_nodo(self.asas[&raiz], camino) {
            let p = self.nodos[n].padre;
            self.nodos[p].hijos.retain(|&h| h != n);
            self.nodos[n].enlazado = false;
        }
    }

    // --- Ayudas de las órdenes ---

    fn nodo(&self, asa: Asa) -> Result<usize, ErrorFs> {
        self.asas.get(&asa).copied().ok_or(ErrorFs::AsaMala)
    }

    fn datos_de(&self, n: usize) -> Datos {
        let x = &self.nodos[n];
        Datos {
            tamano: if x.cajon { 0 } else { x.datos.len() as u64 },
            fecha: x.fecha,
            modo: x.modo,
            tipo: if x.cajon { Tipo::Cajon } else { Tipo::Fichero },
            amiga: x.amiga,
        }
    }

    fn nueva_asa(&mut self, n: usize) -> Result<Asa, ErrorFs> {
        if self.asas_abiertas() >= self.max_asas {
            return Err(ErrorFs::SinSitio);
        }
        while self.asas.contains_key(&self.siguiente) || self.siguiente < 16 {
            self.siguiente = self.siguiente.wrapping_add(1);
        }
        let a = self.siguiente;
        self.siguiente = self.siguiente.wrapping_add(1);
        self.asas.insert(a, n);
        Ok(a)
    }

    fn escribible(&self, n: usize) -> Result<(), ErrorFs> {
        if self.vols[self.nodos[n].vol].solo_lectura {
            return Err(ErrorFs::SoloLectura);
        }
        Ok(())
    }

    fn usado(&self, vol: usize) -> u64 {
        self.nodos.iter().filter(|n| n.vol == vol).map(|n| n.datos.len() as u64 + 64).sum()
    }

    fn cabe(&self, vol: usize, mas: u64) -> Result<(), ErrorFs> {
        if self.usado(vol).saturating_add(mas) > self.vols[vol].tamano { Err(ErrorFs::Lleno) } else { Ok(()) }
    }

    fn hijo(&self, dir: usize, nombre: &str) -> Option<usize> {
        let k = clave(nombre);
        self.nodos[dir].hijos.iter().copied().find(|&h| clave(&self.nodos[h].nombre) == k)
    }

    fn cajon_vivo(&self, n: usize) -> Result<(), ErrorFs> {
        if !self.nodos[n].cajon {
            return Err(ErrorFs::NoEsCajon);
        }
        if !self.nodos[n].enlazado {
            return Err(ErrorFs::NoExiste);
        }
        Ok(())
    }
}

impl SistemaArchivos for MemFs {
    fn buscar(&mut self, asa: Asa, camino: &str) -> Result<(Asa, Datos), ErrorFs> {
        self.ordenes += 1;
        let n = self.buscar_nodo(self.nodo(asa)?, camino)?;
        let a = self.nueva_asa(n)?;
        Ok((a, self.datos_de(n)))
    }

    fn mirar(&mut self, asa: Asa, camino: &str) -> Result<Datos, ErrorFs> {
        self.ordenes += 1;
        let n = self.buscar_nodo(self.nodo(asa)?, camino)?;
        Ok(self.datos_de(n))
    }

    fn leer(&mut self, asa: Asa, desde: u64, buf: &mut [u8]) -> Result<usize, ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        let x = &self.nodos[n];
        if x.cajon {
            return Err(ErrorFs::EsCajon);
        }
        if x.modo & 0o400 == 0 {
            return Err(ErrorFs::Protegido);
        }
        let d = desde.min(x.datos.len() as u64) as usize;
        let k = buf.len().min(x.datos.len() - d);
        buf[..k].copy_from_slice(&x.datos[d..d + k]);
        Ok(k)
    }

    fn escribir(&mut self, asa: Asa, desde: u64, datos: &[u8]) -> Result<usize, ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        if self.nodos[n].cajon {
            return Err(ErrorFs::EsCajon);
        }
        self.escribible(n)?;
        if self.nodos[n].modo & 0o200 == 0 {
            return Err(ErrorFs::Protegido);
        }
        let fin = desde.checked_add(datos.len() as u64).ok_or(ErrorFs::MalFormado)?;
        let largo = self.nodos[n].datos.len() as u64;
        if fin > largo {
            self.cabe(self.nodos[n].vol, fin - largo)?;
        }
        if fin > 1 << 32 {
            return Err(ErrorFs::Lleno);
        }
        let x = &mut self.nodos[n];
        if (fin as usize) > x.datos.len() {
            x.datos.resize(fin as usize, 0);
        }
        x.datos[desde as usize..fin as usize].copy_from_slice(datos);
        x.amiga &= !bits::ARCHIVADO;
        x.fecha = self.reloj;
        Ok(datos.len())
    }

    fn listar(&mut self, asa: Asa, desde: u64) -> Result<Pagina, ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        if !self.nodos[n].cajon {
            return Err(ErrorFs::NoEsCajon);
        }
        if self.nodos[n].modo & 0o400 == 0 {
            return Err(ErrorFs::Protegido);
        }
        let hijos = &self.nodos[n].hijos;
        let d = desde.min(hijos.len() as u64) as usize;
        let fin = (d + POR_PAGINA).min(hijos.len());
        let entradas = hijos[d..fin]
            .iter()
            .map(|&h| Entrada {
                nombre: self.nodos[h].nombre.clone(),
                comentario: self.nodos[h].comentario.clone(),
                datos: self.datos_de(h),
            })
            .collect();
        Ok(Pagina { entradas, siguiente: if fin < hijos.len() { Some(fin as u64) } else { None } })
    }

    fn datos(&mut self, asa: Asa) -> Result<Datos, ErrorFs> {
        self.ordenes += 1;
        Ok(self.datos_de(self.nodo(asa)?))
    }

    fn cerrar(&mut self, asa: Asa) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        if asa < 16 {
            return Err(ErrorFs::AsaMala);
        }
        self.asas.remove(&asa).map(|_| ()).ok_or(ErrorFs::AsaMala)
    }

    fn crear(&mut self, asa: Asa, nombre: &str, cajon: bool) -> Result<(Asa, Datos), ErrorFs> {
        self.ordenes += 1;
        let dir = self.nodo(asa)?;
        self.cajon_vivo(dir)?;
        if !nombre_valido(nombre) {
            return Err(ErrorFs::MalFormado);
        }
        self.escribible(dir)?;
        if self.nodos[dir].modo & 0o200 == 0 {
            return Err(ErrorFs::Protegido);
        }
        if self.hijo(dir, nombre).is_some() {
            return Err(ErrorFs::YaExiste);
        }
        self.cabe(self.nodos[dir].vol, 64)?;
        if self.asas_abiertas() >= self.max_asas {
            return Err(ErrorFs::SinSitio);
        }
        let n = self.hijo_nuevo(dir, nombre, cajon);
        let a = self.nueva_asa(n)?;
        Ok((a, self.datos_de(n)))
    }

    fn borrar(&mut self, asa: Asa, nombre: &str) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        let dir = self.nodo(asa)?;
        self.cajon_vivo(dir)?;
        if !nombre_valido(nombre) {
            return Err(ErrorFs::MalFormado);
        }
        let n = self.hijo(dir, nombre).ok_or(ErrorFs::NoExiste)?;
        self.escribible(n)?;
        if self.nodos[dir].modo & 0o200 == 0 || self.nodos[n].amiga & bits::NO_BORRAR_DUENO != 0 {
            return Err(ErrorFs::Protegido);
        }
        if !self.nodos[n].hijos.is_empty() {
            return Err(ErrorFs::NoVacio);
        }
        self.nodos[dir].hijos.retain(|&h| h != n);
        self.nodos[n].enlazado = false;
        Ok(())
    }

    fn renombrar(&mut self, asa: Asa, viejo: &str, asa_nueva: Asa, nuevo: &str) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        let d1 = self.nodo(asa)?;
        let d2 = self.nodo(asa_nueva)?;
        self.cajon_vivo(d1)?;
        self.cajon_vivo(d2)?;
        if !nombre_valido(viejo) || !nombre_valido(nuevo) {
            return Err(ErrorFs::MalFormado);
        }
        if self.nodos[d1].vol != self.nodos[d2].vol {
            return Err(ErrorFs::MalFormado);
        }
        let n = self.hijo(d1, viejo).ok_or(ErrorFs::NoExiste)?;
        self.escribible(n)?;
        if self.nodos[d1].modo & 0o200 == 0 || self.nodos[d2].modo & 0o200 == 0 {
            return Err(ErrorFs::Protegido);
        }
        if let Some(otro) = self.hijo(d2, nuevo)
            && otro != n
        {
            return Err(ErrorFs::YaExiste);
        }
        // No meter un cajón en sí mismo.
        let mut p = d2;
        loop {
            if p == n {
                return Err(ErrorFs::MalFormado);
            }
            let q = self.nodos[p].padre;
            if q == p {
                break;
            }
            p = q;
        }
        self.nodos[d1].hijos.retain(|&h| h != n);
        self.nodos[d2].hijos.push(n);
        self.nodos[n].padre = d2;
        self.nodos[n].nombre = nuevo.to_string();
        Ok(())
    }

    fn cambiar_tamano(&mut self, asa: Asa, tamano: u64) -> Result<u64, ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        if self.nodos[n].cajon {
            return Err(ErrorFs::EsCajon);
        }
        self.escribible(n)?;
        if self.nodos[n].modo & 0o200 == 0 {
            return Err(ErrorFs::Protegido);
        }
        let largo = self.nodos[n].datos.len() as u64;
        if tamano > largo {
            self.cabe(self.nodos[n].vol, tamano - largo)?;
        }
        if tamano > 1 << 32 {
            return Err(ErrorFs::Lleno);
        }
        let x = &mut self.nodos[n];
        x.datos.resize(tamano as usize, 0);
        x.amiga &= !bits::ARCHIVADO;
        Ok(tamano)
    }

    fn vaciar(&mut self) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        Ok(())
    }

    fn info(&mut self, asa: Asa) -> Result<InfoVolumen, ErrorFs> {
        self.ordenes += 1;
        let v = self.nodos[self.nodo(asa)?].vol;
        let usado = self.usado(v);
        Ok(InfoVolumen { tamano: self.vols[v].tamano, libre: self.vols[v].tamano.saturating_sub(usado), bloque: 512 })
    }

    fn comentario(&mut self, asa: Asa) -> Result<String, ErrorFs> {
        self.ordenes += 1;
        Ok(self.nodos[self.nodo(asa)?].comentario.clone())
    }

    fn comentar(&mut self, asa: Asa, comentario: &str) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        self.escribible(n)?;
        if comentario.len() > 255 {
            return Err(ErrorFs::MalFormado);
        }
        self.nodos[n].comentario = comentario.to_string();
        Ok(())
    }

    fn proteger(&mut self, asa: Asa, amiga: u16, modo: u16) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        self.escribible(n)?;
        self.nodos[n].amiga = amiga & 0x7F;
        self.nodos[n].modo = modo & 0o7777;
        Ok(())
    }

    fn fechar(&mut self, asa: Asa, fecha: u64) -> Result<(), ErrorFs> {
        self.ordenes += 1;
        let n = self.nodo(asa)?;
        self.escribible(n)?;
        self.nodos[n].fecha = fecha;
        Ok(())
    }
}
