//! S32: lo justo de FAT16 y FAT32 para poner el cargador de DashOS en la partición de arranque EFI (ESP) que ya hay,
//! sin tocar lo de los demás, y para hacer una ESP nueva (FAT32) cuando no hay. Escrito con la especificación de FAT
//! de Microsoft (FAT: General Overview of On-Disk Format, 1.03).
//!
//! Orden al escribir, para que un corte de luz deje la ESP buena (como mucho, con clusters perdidos, que no molestan a
//! nadie y que cualquier comprobador recupera):
//! 1. los datos del fichero (o del cajón nuevo) en clusters libres;
//! 2. la cadena en las FAT (todas las copias);
//! 3. la entrada del cajón (un solo sector): desde aquí el fichero existe, entero;
//! 4. si se sustituía uno, se liberan los clusters del de antes.
//! Un fichero que ya estaba se sustituye así, nunca escribiendo encima.

use super::disco::{Disco, escribir_bytes, leer_bytes, poner_u16, poner_u32, u16_en, u32_en};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Un volumen FAT abierto.
pub struct Fat {
    /// Byte del disco donde empieza el volumen, y su largo.
    base: u64,
    largo: u64,
    pub bits: u8,
    bps: u64,
    cluster: u64,
    /// Bytes de la primera FAT, cuántas hay y bytes de cada una.
    fat0: u64,
    nfats: u64,
    tam_fat: u64,
    /// FAT16: el cajón raíz fijo (byte y entradas). FAT32: su primer cluster.
    raiz_fija: (u64, u64),
    raiz_cluster: u32,
    /// Byte del cluster 2.
    datos: u64,
    /// Clusters de datos (los números van de 2 a clusters+1).
    pub clusters: u32,
    /// La FAT entera en memoria (la primera copia).
    tabla: Vec<u8>,
}

/// Dónde está un cajón: el raíz fijo de FAT16 (0) o su primer cluster.
pub type Cajon = u32;

/// Una entrada de un cajón.
#[derive(Clone, Debug)]
pub struct Entrada {
    pub nombre: String,
    pub corto: [u8; 11],
    pub atributos: u8,
    pub cluster: u32,
    pub tamano: u32,
    /// Byte (del disco) de la entrada corta, para cambiarla.
    pub donde: u64,
    /// Bytes de sus entradas de nombre largo (para borrarlas), en orden.
    pub largas: Vec<u64>,
}

const CAJON: u8 = 0x10;

impl Fat {
    pub fn abrir(d: &mut dyn Disco, desde: u64, hasta: u64) -> Result<Fat, &'static str> {
        let ts = d.tam_sector() as u64;
        let base = desde * ts;
        let largo = (hasta - desde) * ts;
        let b = leer_bytes(d, base, 512).ok_or("no se lee")?;
        if b[510] != 0x55 || b[511] != 0xAA {
            return Err("sin firma");
        }
        let bps = u16_en(&b, 11) as u64;
        let spc = b[13] as u64;
        let reservados = u16_en(&b, 14) as u64;
        let nfats = b[16] as u64;
        let raiz_n = u16_en(&b, 17) as u64;
        let total = if u16_en(&b, 19) != 0 { u16_en(&b, 19) as u64 } else { u32_en(&b, 32) as u64 };
        let spf = if u16_en(&b, 22) != 0 { u16_en(&b, 22) as u64 } else { u32_en(&b, 36) as u64 };
        if !bps.is_power_of_two() || !(512..=4096).contains(&bps) || spc == 0 || !spc.is_power_of_two() || reservados == 0 || !(1..=2).contains(&nfats) || spf == 0 {
            return Err("cabecera que no vale");
        }
        if total * bps > largo {
            return Err("el volumen dice ser mayor que su partición");
        }
        let raiz_sec = raiz_n * 32 / bps;
        let datos_sec = reservados + nfats * spf + raiz_sec;
        if datos_sec >= total {
            return Err("cabecera que no vale");
        }
        let clusters = (total - datos_sec) / spc;
        let bits = if clusters < 4085 {
            12
        } else if clusters < 65525 {
            16
        } else {
            32
        };
        if bits == 12 {
            return Err("FAT12 no se escribe");
        }
        if bits == 32 && (raiz_n != 0 || u16_en(&b, 42) != 0) {
            return Err("FAT32 que no vale");
        }
        if spf * bps * 8 / (bits as u64) < clusters + 2 {
            return Err("la FAT es más pequeña que el volumen");
        }
        let fat0 = reservados * bps;
        let tam_fat = spf * bps;
        let tabla = leer_bytes(d, base + fat0, ((clusters as usize + 2) * bits as usize / 8).next_multiple_of(bps as usize)).ok_or("no se lee la FAT")?;
        let raiz_cluster = if bits == 32 { u32_en(&b, 44) } else { 0 };
        let f = Fat {
            base,
            largo,
            bits,
            bps,
            cluster: spc * bps,
            fat0,
            nfats,
            tam_fat,
            raiz_fija: ((reservados + nfats * spf) * bps, raiz_n),
            raiz_cluster,
            datos: datos_sec * bps,
            clusters: clusters as u32,
            tabla,
        };
        if bits == 32 && !f.valido(raiz_cluster) {
            return Err("el cajón raíz no vale");
        }
        Ok(f)
    }

    fn valido(&self, c: u32) -> bool {
        c >= 2 && c < self.clusters + 2
    }

    /// El valor de la FAT para el cluster `c`.
    pub fn siguiente(&self, c: u32) -> u32 {
        if self.bits == 32 {
            u32_en(&self.tabla, c as usize * 4) & 0x0FFF_FFFF
        } else {
            u16_en(&self.tabla, c as usize * 2) as u32
        }
    }

    fn es_fin(&self, v: u32) -> bool {
        if self.bits == 32 { v >= 0x0FFF_FFF8 } else { v >= 0xFFF8 }
    }

    fn fin(&self) -> u32 {
        if self.bits == 32 { 0x0FFF_FFFF } else { 0xFFFF }
    }

    fn poner(&mut self, c: u32, v: u32) {
        if self.bits == 32 {
            let viejo = u32_en(&self.tabla, c as usize * 4);
            poner_u32(&mut self.tabla, c as usize * 4, (viejo & 0xF000_0000) | (v & 0x0FFF_FFFF));
        } else {
            poner_u16(&mut self.tabla, c as usize * 2, v as u16);
        }
    }

    /// La cadena de clusters desde `c` (sin bucles: como mucho tantos como clusters hay).
    pub fn cadena(&self, c: u32) -> Option<Vec<u32>> {
        let mut v = Vec::new();
        let mut c = c;
        while self.valido(c) {
            if v.len() > self.clusters as usize {
                return None;
            }
            v.push(c);
            c = self.siguiente(c);
        }
        if !self.es_fin(c) && !v.is_empty() {
            return None;
        }
        Some(v)
    }

    /// Clusters libres.
    pub fn libres(&self) -> u32 {
        (2..self.clusters + 2).filter(|&c| self.siguiente(c) == 0).count() as u32
    }

    pub fn bytes_cluster(&self) -> u64 {
        self.cluster
    }

    fn byte_cluster(&self, c: u32) -> u64 {
        self.base + self.datos + (c as u64 - 2) * self.cluster
    }

    /// Los tramos (byte, largo) de un cajón.
    fn tramos_cajon(&self, cajon: Cajon) -> Option<Vec<(u64, u64)>> {
        if cajon == 0 && self.bits == 16 {
            return Some(vec![(self.base + self.raiz_fija.0, self.raiz_fija.1 * 32)]);
        }
        let c = if cajon == 0 { self.raiz_cluster } else { cajon };
        Some(self.cadena(c)?.into_iter().map(|c| (self.byte_cluster(c), self.cluster)).collect())
    }

    /// Las entradas de un cajón (sin "." ni "..", ni borradas, ni la etiqueta).
    pub fn listar(&self, d: &mut dyn Disco, cajon: Cajon) -> Option<Vec<Entrada>> {
        let mut v = Vec::new();
        let mut largo: Vec<(u8, [u16; 13], u64)> = Vec::new();
        for (byte, n) in self.tramos_cajon(cajon)? {
            let b = leer_bytes(d, byte, n as usize)?;
            for (i, e) in b.chunks(32).enumerate() {
                let donde = byte + i as u64 * 32;
                if e[0] == 0 {
                    return Some(v);
                }
                if e[0] == 0xE5 {
                    largo.clear();
                    continue;
                }
                if e[11] == 0x0F {
                    let mut u = [0u16; 13];
                    for (k, o) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().enumerate() {
                        u[k] = u16_en(e, *o);
                    }
                    largo.push((e[13], u, donde));
                    continue;
                }
                if e[11] & 0x08 != 0 || e[0] == b'.' {
                    largo.clear();
                    continue;
                }
                let mut corto = [0u8; 11];
                corto.copy_from_slice(&e[..11]);
                let suma = suma_corto(&corto);
                let mut nombre = String::new();
                if !largo.is_empty() && largo.iter().all(|l| l.0 == suma) {
                    let mut u: Vec<u16> = Vec::new();
                    for l in largo.iter().rev() {
                        u.extend(l.1.iter().copied().take_while(|&c| c != 0 && c != 0xFFFF));
                    }
                    nombre = char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect();
                }
                if nombre.is_empty() {
                    nombre = nombre_corto(&corto);
                }
                let cl = (u16_en(e, 20) as u32) << 16 | u16_en(e, 26) as u32;
                let largas = largo.iter().map(|l| l.2).collect();
                largo.clear();
                v.push(Entrada { nombre, corto, atributos: e[11], cluster: if self.bits == 32 { cl } else { cl & 0xFFFF }, tamano: u32_en(e, 28), donde, largas });
            }
        }
        Some(v)
    }

    /// Busca `nombre` en el cajón (sin distinguir mayúsculas, por el nombre largo o el corto).
    pub fn buscar(&self, d: &mut dyn Disco, cajon: Cajon, nombre: &str) -> Option<Option<Entrada>> {
        Some(self.listar(d, cajon)?.into_iter().find(|e| e.nombre.eq_ignore_ascii_case(nombre) || nombre_corto(&e.corto).eq_ignore_ascii_case(nombre)))
    }

    /// Lee un fichero entero.
    pub fn leer(&self, d: &mut dyn Disco, e: &Entrada) -> Option<Vec<u8>> {
        let mut v = Vec::new();
        if e.tamano == 0 {
            return Some(v);
        }
        for c in self.cadena(e.cluster)? {
            v.extend_from_slice(&leer_bytes(d, self.byte_cluster(c), self.cluster as usize)?);
            if v.len() >= e.tamano as usize {
                break;
            }
        }
        if v.len() < e.tamano as usize {
            return None;
        }
        v.truncate(e.tamano as usize);
        Some(v)
    }

    /// Toma `n` clusters libres (sin escribir aún nada): la lista.
    fn tomar(&self, n: usize) -> Option<Vec<u32>> {
        let v: Vec<u32> = (2..self.clusters + 2).filter(|&c| self.siguiente(c) == 0).take(n).collect();
        (v.len() == n).then_some(v)
    }

    /// Escribe en el disco los sectores de la FAT (todas las copias) que tocan a estos clusters.
    fn guardar_fat(&self, d: &mut dyn Disco, clusters: &[u32]) -> Result<(), ()> {
        let tam = self.bits as u64 / 8;
        let mut sectores: Vec<u64> = clusters.iter().map(|&c| c as u64 * tam / self.bps).collect();
        sectores.sort();
        sectores.dedup();
        for k in 0..self.nfats {
            for &s in &sectores {
                let o = (s * self.bps) as usize;
                let trozo = &self.tabla[o..(o + self.bps as usize).min(self.tabla.len())];
                escribir_bytes(d, self.base + self.fat0 + k * self.tam_fat + s * self.bps, trozo)?;
            }
            d.vaciar()?;
        }
        Ok(())
    }

    /// Escribe `datos` en clusters nuevos encadenados (pasos 1 y 2). Devuelve el primero (0 si no hay datos).
    fn escribir_cadena(&mut self, d: &mut dyn Disco, datos: &[u8]) -> Result<u32, &'static str> {
        let n = (datos.len() as u64).div_ceil(self.cluster) as usize;
        if n == 0 {
            return Ok(0);
        }
        let cs = self.tomar(n).ok_or("no hay sitio en la partición de arranque EFI")?;
        for (i, &c) in cs.iter().enumerate() {
            let mut trozo = datos[i * self.cluster as usize..((i + 1) * self.cluster as usize).min(datos.len())].to_vec();
            trozo.resize(self.cluster as usize, 0);
            escribir_bytes(d, self.byte_cluster(c), &trozo).map_err(|_| "no se pudo escribir")?;
        }
        d.vaciar().map_err(|_| "no se pudo escribir")?;
        for (i, &c) in cs.iter().enumerate() {
            let sig = cs.get(i + 1).copied().unwrap_or(self.fin());
            self.poner(c, sig);
        }
        self.guardar_fat(d, &cs).map_err(|_| "no se pudo escribir la FAT")?;
        Ok(cs[0])
    }

    /// Un hueco para `n` entradas seguidas en el cajón: su byte. Si no hay, el cajón crece un cluster (FAT32 o un
    /// subcajón; el raíz fijo de FAT16 no crece).
    fn hueco(&mut self, d: &mut dyn Disco, cajon: Cajon, n: usize) -> Result<u64, &'static str> {
        let tramos = self.tramos_cajon(cajon).ok_or("cajón que no vale")?;
        let mut seguidas = 0;
        let mut primero = 0;
        for &(byte, largo) in &tramos {
            let b = leer_bytes(d, byte, largo as usize).ok_or("no se lee")?;
            for (i, e) in b.chunks(32).enumerate() {
                if e[0] == 0 || e[0] == 0xE5 {
                    if seguidas == 0 {
                        primero = byte + i as u64 * 32;
                    }
                    seguidas += 1;
                    if seguidas == n {
                        return Ok(primero);
                    }
                } else {
                    seguidas = 0;
                }
            }
            // Las entradas largas y su corta no cruzan de un cluster a otro (así es más simple y siempre vale).
            seguidas = 0;
        }
        if cajon == 0 && self.bits == 16 {
            return Err("el cajón raíz está lleno");
        }
        // Crece: un cluster a cero, encadenado al final.
        let ultimo = self.cadena(if cajon == 0 { self.raiz_cluster } else { cajon }).and_then(|v| v.last().copied()).ok_or("cajón que no vale")?;
        let c = self.tomar(1).ok_or("no hay sitio en la partición de arranque EFI")?[0];
        escribir_bytes(d, self.byte_cluster(c), &vec![0u8; self.cluster as usize]).map_err(|_| "no se pudo escribir")?;
        d.vaciar().map_err(|_| "no se pudo escribir")?;
        self.poner(c, self.fin());
        self.poner(ultimo, c);
        self.guardar_fat(d, &[c, ultimo]).map_err(|_| "no se pudo escribir la FAT")?;
        Ok(self.byte_cluster(c))
    }

    /// Escribe las entradas (largas y la corta) de `nombre` en el cajón, todas en un sector si se puede (paso 3).
    fn poner_entrada(&mut self, d: &mut dyn Disco, cajon: Cajon, nombre: &str, corto: [u8; 11], atributos: u8, cluster: u32, tamano: u32) -> Result<(), &'static str> {
        let largas = if nombre_corto(&corto) == nombre { Vec::new() } else { entradas_largas(nombre, &corto) };
        let n = largas.len() + 1;
        let donde = self.hueco(d, cajon, n)?;
        let mut b = Vec::with_capacity(n * 32);
        for l in &largas {
            b.extend_from_slice(l);
        }
        b.extend_from_slice(&entrada_corta(corto, atributos, cluster, tamano));
        escribir_bytes(d, donde, &b).map_err(|_| "no se pudo escribir el cajón")?;
        d.vaciar().map_err(|_| "no se pudo escribir")?;
        Ok(())
    }

    /// El nombre corto para `nombre` en el cajón: el suyo si ya es 8.3, o `BASE~N.EXT` sin repetir.
    fn corto_unico(&self, d: &mut dyn Disco, cajon: Cajon, nombre: &str) -> Result<[u8; 11], &'static str> {
        if let Some(c) = corto_de(nombre) {
            return Ok(c);
        }
        if nombre.is_empty() || nombre.len() > 255 || nombre.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)) {
            return Err("nombre que no vale");
        }
        let hay: Vec<[u8; 11]> = self.listar(d, cajon).ok_or("no se lee el cajón")?.into_iter().map(|e| e.corto).collect();
        let (base, ext) = nombre.rsplit_once('.').unwrap_or((nombre, ""));
        let limpio = |s: &str, n: usize| -> Vec<u8> { s.bytes().filter(|b| b.is_ascii_alphanumeric()).map(|b| b.to_ascii_uppercase()).take(n).collect() };
        let (b, e) = (limpio(base, 6), limpio(ext, 3));
        for n in 1..1000u32 {
            let sufijo = alloc::format!("~{}", n);
            let mut c = [b' '; 11];
            let largo = b.len().min(8 - sufijo.len());
            c[..largo].copy_from_slice(&b[..largo]);
            c[largo..largo + sufijo.len()].copy_from_slice(sufijo.as_bytes());
            c[8..8 + e.len()].copy_from_slice(&e);
            if !hay.contains(&c) {
                return Ok(c);
            }
        }
        Err("demasiados nombres parecidos")
    }

    /// Crea el cajón `nombre` en `padre` si no está; devuelve su cluster.
    pub fn cajon(&mut self, d: &mut dyn Disco, padre: Cajon, nombre: &str) -> Result<Cajon, &'static str> {
        if let Some(e) = self.buscar(d, padre, nombre).ok_or("no se lee el cajón")? {
            if e.atributos & CAJON == 0 || !self.valido(e.cluster) {
                return Err("hay un fichero con el nombre del cajón");
            }
            return Ok(e.cluster);
        }
        let corto = self.corto_unico(d, padre, nombre)?;
        let mut b = vec![0u8; self.cluster as usize];
        b[..32].copy_from_slice(&entrada_corta(*b".          ", CAJON, 0, 0));
        b[32..64].copy_from_slice(&entrada_corta(*b"..         ", CAJON, if self.bits == 32 && padre == self.raiz_cluster { 0 } else { padre }, 0));
        let c = self.escribir_cadena(d, &b)?;
        // "." apunta a sí mismo: se arregla ya escrito el cluster (aún no lo ve nadie).
        let mut punto = entrada_corta(*b".          ", CAJON, c, 0);
        escribir_bytes(d, self.byte_cluster(c), &punto).map_err(|_| "no se pudo escribir")?;
        punto.fill(0);
        self.poner_entrada(d, padre, nombre, corto, CAJON, c, 0)?;
        Ok(c)
    }

    /// Pone el fichero `nombre` con `datos` en el cajón, sustituyendo el que hubiera (en el orden de la cabecera).
    pub fn poner_fichero(&mut self, d: &mut dyn Disco, cajon: Cajon, nombre: &str, datos: &[u8]) -> Result<(), &'static str> {
        let antes = self.buscar(d, cajon, nombre).ok_or("no se lee el cajón")?;
        if let Some(e) = &antes {
            if e.atributos & CAJON != 0 {
                return Err("hay un cajón con el nombre del fichero");
            }
        }
        if datos.len() as u64 > u32::MAX as u64 {
            return Err("fichero demasiado grande");
        }
        let c = self.escribir_cadena(d, datos)?;
        match antes {
            None => {
                let corto = self.corto_unico(d, cajon, nombre)?;
                self.poner_entrada(d, cajon, nombre, corto, 0x20, c, datos.len() as u32)?;
            }
            Some(e) => {
                // La entrada corta, cambiada de una vez (32 bytes dentro de un sector), y luego se suelta lo de antes.
                let nueva = entrada_corta(e.corto, e.atributos | 0x20, c, datos.len() as u32);
                escribir_bytes(d, e.donde, &nueva).map_err(|_| "no se pudo escribir el cajón")?;
                d.vaciar().map_err(|_| "no se pudo escribir")?;
                if let Some(viejos) = self.cadena(e.cluster).filter(|_| self.valido(e.cluster)) {
                    for &v in &viejos {
                        self.poner(v, 0);
                    }
                    self.guardar_fat(d, &viejos).map_err(|_| "no se pudo escribir la FAT")?;
                }
            }
        }
        Ok(())
    }
}

/// La suma del nombre corto que llevan sus entradas largas.
fn suma_corto(c: &[u8; 11]) -> u8 {
    c.iter().fold(0u8, |s, &b| (s >> 1 | s << 7).wrapping_add(b))
}

/// "NOMBRE  EXT" -> "NOMBRE.EXT".
fn nombre_corto(c: &[u8; 11]) -> String {
    let base: String = c[..8].iter().map(|&b| b as char).collect::<String>().trim_end().into();
    let ext: String = c[8..].iter().map(|&b| b as char).collect::<String>().trim_end().into();
    if ext.is_empty() { base } else { alloc::format!("{}.{}", base, ext) }
}

/// El nombre corto de un nombre que ya lo es casi (letras ASCII, cifras, `_` y `-`, 8.3): en mayúsculas. Lo demás no
/// se admite (el instalador solo crea los suyos).
fn corto_de(n: &str) -> Option<[u8; 11]> {
    let (base, ext) = n.split_once('.').unwrap_or((n, ""));
    if base.is_empty() || base.len() > 8 || ext.len() > 3 || ext.contains('.') {
        return None;
    }
    let ok = |s: &str| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok(base) || !ok(ext) {
        return None;
    }
    let mut c = [b' '; 11];
    for (i, b) in base.bytes().enumerate() {
        c[i] = b.to_ascii_uppercase();
    }
    for (i, b) in ext.bytes().enumerate() {
        c[8 + i] = b.to_ascii_uppercase();
    }
    Some(c)
}

fn entrada_corta(corto: [u8; 11], atributos: u8, cluster: u32, tamano: u32) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[..11].copy_from_slice(&corto);
    e[11] = atributos;
    // Fecha y hora: 2026-11-16 a las 12:00 (no hay reloj fiable aquí; la de compilación del plan bastaría).
    let fecha: u16 = ((2026 - 1980) << 9) | (11 << 5) | 16;
    let hora: u16 = 12 << 11;
    poner_u16(&mut e, 14, hora);
    poner_u16(&mut e, 16, fecha);
    poner_u16(&mut e, 18, fecha);
    poner_u16(&mut e, 20, (cluster >> 16) as u16);
    poner_u16(&mut e, 22, hora);
    poner_u16(&mut e, 24, fecha);
    poner_u16(&mut e, 26, cluster as u16);
    poner_u32(&mut e, 28, tamano);
    e
}

/// Las entradas de nombre largo de `nombre`, en el orden en que van en el disco (la última primero).
fn entradas_largas(nombre: &str, corto: &[u8; 11]) -> Vec<[u8; 32]> {
    let u: Vec<u16> = nombre.encode_utf16().collect();
    let n = u.len().div_ceil(13);
    let suma = suma_corto(corto);
    let mut v = Vec::new();
    for k in (0..n).rev() {
        let mut e = [0u8; 32];
        e[0] = (k as u8 + 1) | if k == n - 1 { 0x40 } else { 0 };
        e[11] = 0x0F;
        e[13] = suma;
        for (i, o) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().enumerate() {
            let j = k * 13 + i;
            let c = if j < u.len() {
                u[j]
            } else if j == u.len() {
                0
            } else {
                0xFFFF
            };
            poner_u16(&mut e, *o, c);
        }
        v.push(e);
    }
    v
}

/// Formatea [desde, hasta) como FAT32 con clusters de 4 KiB (o menos, si no salen 65.525 clusters), con la etiqueta.
pub fn formatear_fat32(d: &mut dyn Disco, desde: u64, hasta: u64, etiqueta: &str, serie: u32) -> Result<(), &'static str> {
    let ts = d.tam_sector() as u64;
    let bps = 512u64.max(ts);
    let total = (hasta - desde) * ts / bps;
    let mut spc = (4096 / bps).max(1);
    let reservados = 32u64;
    let calcular = |spc: u64| {
        // spf: las FAT (2) para todos los clusters, cuatro bytes cada uno.
        let mut spf = 1u64;
        loop {
            let clusters = (total - reservados - 2 * spf) / spc;
            let necesita = ((clusters + 2) * 4).div_ceil(bps);
            if necesita <= spf {
                return (spf, clusters);
            }
            spf = necesita;
        }
    };
    let (mut spf, mut clusters) = calcular(spc);
    while clusters < 65525 && spc > 1 {
        spc /= 2;
        (spf, clusters) = calcular(spc);
    }
    if clusters < 65525 {
        return Err("la partición es demasiado pequeña para FAT32");
    }
    let base = desde * ts;
    let mut b = vec![0u8; bps as usize];
    b[..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    b[3..11].copy_from_slice(b"DASHOS  ");
    poner_u16(&mut b, 11, bps as u16);
    b[13] = spc as u8;
    poner_u16(&mut b, 14, reservados as u16);
    b[16] = 2;
    b[21] = 0xF8;
    poner_u16(&mut b, 24, 63);
    poner_u16(&mut b, 26, 255);
    poner_u32(&mut b, 28, desde as u32);
    poner_u32(&mut b, 32, total as u32);
    poner_u32(&mut b, 36, spf as u32);
    poner_u32(&mut b, 44, 2);
    poner_u16(&mut b, 48, 1);
    poner_u16(&mut b, 50, 6);
    b[64] = 0x80;
    b[66] = 0x29;
    poner_u32(&mut b, 67, serie);
    let mut et = [b' '; 11];
    for (i, c) in etiqueta.bytes().take(11).enumerate() {
        et[i] = c.to_ascii_uppercase();
    }
    b[71..82].copy_from_slice(&et);
    b[82..90].copy_from_slice(b"FAT32   ");
    b[510] = 0x55;
    b[511] = 0xAA;
    let mut info = vec![0u8; bps as usize];
    poner_u32(&mut info, 0, 0x4161_5252);
    poner_u32(&mut info, 484, 0x6141_7272);
    poner_u32(&mut info, 488, 0xFFFF_FFFF);
    poner_u32(&mut info, 492, 0xFFFF_FFFF);
    poner_u32(&mut info, 508, 0xAA55_0000);
    let e = |r: Result<(), ()>| r.map_err(|_| "no se pudo escribir");
    // Lo reservado y las FAT, a cero; luego el cluster raíz; la cabecera, lo último.
    let ceros = vec![0u8; (bps * 64) as usize];
    let mut s = 1;
    while s < reservados + 2 * spf {
        let n = (reservados + 2 * spf - s).min(64);
        e(escribir_bytes(d, base + s * bps, &ceros[..(n * bps) as usize]))?;
        s += n;
    }
    let mut fat = vec![0u8; bps as usize];
    poner_u32(&mut fat, 0, 0x0FFF_FFF8);
    poner_u32(&mut fat, 4, 0x0FFF_FFFF);
    poner_u32(&mut fat, 8, 0x0FFF_FFFF);
    for k in 0..2 {
        e(escribir_bytes(d, base + (reservados + k * spf) * bps, &fat))?;
    }
    let raiz = base + (reservados + 2 * spf) * bps;
    e(escribir_bytes(d, raiz, &vec![0u8; (spc * bps) as usize]))?;
    e(escribir_bytes(d, base + bps, &info))?;
    e(escribir_bytes(d, base + 7 * bps, &info))?;
    e(escribir_bytes(d, base + 6 * bps, &b))?;
    e(d.vaciar())?;
    e(escribir_bytes(d, base, &b))?;
    e(d.vaciar())?;
    Ok(())
}
