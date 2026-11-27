//! S32: qué hay dentro de cada partición, por su firma (no por el tipo de la tabla, que cualquiera pone mal). Lo que no
//! se reconoce no se toca. De NTFS se mira además lo que impide encogerlo: hibernado (o con el inicio rápido de
//! Windows), sucio (comprobación pendiente), cifrado con BitLocker; y cuánto ocupa.

use super::disco::{Disco, leer_bytes, u16_en, u32_en, u64_en};
use alloc::string::String;
use alloc::vec::Vec;

/// El estado de un NTFS, leído de su `$Volume`, de su `$Bitmap` y del principio de `hiberfil.sys`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ntfs {
    /// El tamaño de cluster en bytes.
    pub cluster: u32,
    /// Los clusters del volumen.
    pub clusters: u64,
    /// Los clusters en uso según `$Bitmap` (`None` si no se pudo leer).
    pub usados: Option<u64>,
    /// El último cluster en uso + 1 (lo que se podría encoger sin mover nada).
    pub ultimo_usado: Option<u64>,
    /// La bandera de sucio de `$Volume` (comprobación pendiente o no se desmontó bien). `None`: no se pudo leer.
    pub sucio: Option<bool>,
    /// Si `hiberfil.sys` empieza por "hibr"/"HIBR" (hibernado o inicio rápido). `None`: no se pudo leer.
    pub hibernado: Option<bool>,
    /// La etiqueta del volumen.
    pub etiqueta: String,
}

/// Lo que hay en una partición.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Contenido {
    /// Todo ceros al principio.
    Vacia,
    Fat { bits: u8, etiqueta: String },
    ExFat,
    Ntfs(Ntfs),
    BitLocker,
    Ext { version: u8, limpio: bool, etiqueta: String },
    Btrfs,
    Swap,
    Luks,
    Lvm,
    Apfs,
    HfsPlus,
    DashFS,
    Desconocido,
}

impl Contenido {
    /// En llano, para el plan y la ventana.
    pub fn nombre(&self) -> &'static str {
        match self {
            Contenido::Vacia => "vacía",
            Contenido::Fat { bits: 12, .. } => "FAT12",
            Contenido::Fat { bits: 16, .. } => "FAT16",
            Contenido::Fat { .. } => "FAT32",
            Contenido::ExFat => "exFAT",
            Contenido::Ntfs(_) => "NTFS",
            Contenido::BitLocker => "BitLocker (cifrada)",
            Contenido::Ext { version: 4, .. } => "ext4",
            Contenido::Ext { version: 3, .. } => "ext3",
            Contenido::Ext { .. } => "ext2",
            Contenido::Btrfs => "btrfs",
            Contenido::Swap => "intercambio (swap)",
            Contenido::Luks => "LUKS (cifrada)",
            Contenido::Lvm => "LVM",
            Contenido::Apfs => "APFS",
            Contenido::HfsPlus => "HFS+",
            Contenido::DashFS => "DashFS",
            Contenido::Desconocido => "desconocido",
        }
    }
}

/// Mira la partición [desde, hasta) del disco.
pub fn reconocer(d: &mut dyn Disco, desde: u64, hasta: u64) -> Contenido {
    let ts = d.tam_sector() as u64;
    let base = desde * ts;
    let largo = (hasta - desde) * ts;
    let Some(c) = leer_bytes(d, base, 4096.min(largo as usize)) else { return Contenido::Desconocido };
    if c.len() < 1024 {
        return Contenido::Desconocido;
    }
    if &c[3..11] == b"-FVE-FS-" {
        return Contenido::BitLocker;
    }
    if &c[3..11] == b"NTFS    " {
        return Contenido::Ntfs(ntfs(d, base, largo).unwrap_or_default());
    }
    if &c[3..11] == b"EXFAT   " {
        return Contenido::ExFat;
    }
    if c.len() >= 4096 && &c[..6] == b"LUKS\xba\xbe" {
        return Contenido::Luks;
    }
    if &c[..8] == b"DashFS\0\0" || &c[..8] == b"RedoxFS\0" {
        return Contenido::DashFS;
    }
    if &c[32..36] == b"NXSB" {
        return Contenido::Apfs;
    }
    if c.len() >= 1536 && (&c[1024..1026] == b"H+" || &c[1024..1026] == b"HX") {
        return Contenido::HfsPlus;
    }
    if c.len() >= 4096 && (&c[4086..4096] == b"SWAPSPACE2" || &c[4086..4096] == b"SWAP-SPACE") {
        return Contenido::Swap;
    }
    for s in 0..4 {
        let o = s * 512;
        if c.len() >= o + 32 && &c[o..o + 8] == b"LABELONE" && &c[o + 24..o + 32] == b"LVM2 001" {
            return Contenido::Lvm;
        }
    }
    if c.len() >= 1024 + 136 && u16_en(&c, 1024 + 56) == 0xEF53 {
        let s = &c[1024..];
        let (compat, incompat) = (u32_en(s, 92), u32_en(s, 96));
        let version = if incompat & (0x40 | 0x80 | 0x200) != 0 { 4 } else if compat & 0x4 != 0 { 3 } else { 2 };
        return Contenido::Ext { version, limpio: u16_en(s, 58) & 1 == 1 && u16_en(s, 58) & 2 == 0, etiqueta: texto(&s[120..136]) };
    }
    if largo >= 0x10000 + 0x48 {
        if let Some(b) = leer_bytes(d, base + 0x10040, 8) {
            if &b == b"_BHRfS_M" {
                return Contenido::Btrfs;
            }
        }
    }
    if c[510] == 0x55 && c[511] == 0xAA && u16_en(&c, 11).is_power_of_two() && u16_en(&c, 11) >= 512 && c[13] != 0 && c[13].is_power_of_two() {
        if &c[82..87] == b"FAT32" {
            return Contenido::Fat { bits: 32, etiqueta: texto(&c[71..82]) };
        }
        if &c[54..59] == b"FAT16" {
            return Contenido::Fat { bits: 16, etiqueta: texto(&c[43..54]) };
        }
        if &c[54..59] == b"FAT12" {
            return Contenido::Fat { bits: 12, etiqueta: texto(&c[43..54]) };
        }
    }
    if c.iter().all(|&b| b == 0) {
        return Contenido::Vacia;
    }
    Contenido::Desconocido
}

/// Un texto de largo fijo (ASCII, relleno de espacios o ceros).
fn texto(b: &[u8]) -> String {
    let mut s: String = b.iter().take_while(|&&c| c != 0).map(|&c| if c.is_ascii_graphic() || c == b' ' { c as char } else { '?' }).collect();
    while s.ends_with(' ') {
        s.pop();
    }
    s
}

/// Lo que hace falta del NTFS para leer sus ficheros de sistema.
struct Vol {
    base: u64,
    largo: u64,
    cluster: u64,
    registro: usize,
    indice: usize,
    /// Los tramos del `$MFT`: (VCN inicial, LCN, clusters).
    mft: Vec<(u64, u64, u64)>,
}

/// Un atributo de un registro del MFT.
struct Atributo<'a> {
    tipo: u32,
    nombre_vacio: bool,
    residente: bool,
    /// El valor (residente) o la cabecera entera (no residente).
    datos: &'a [u8],
}

/// Aplica la corrección de secuencia (los dos últimos bytes de cada sector de 512 del registro). `false` si no casa.
fn arreglar(r: &mut [u8]) -> bool {
    if r.len() < 48 {
        return false;
    }
    let (off, n) = (u16_en(r, 4) as usize, u16_en(r, 6) as usize);
    if n == 0 || off + 2 * n > r.len() || (n - 1) * 512 > r.len() {
        return false;
    }
    let usn = [r[off], r[off + 1]];
    for i in 1..n {
        let fin = i * 512 - 2;
        if r[fin..fin + 2] != usn {
            return false;
        }
        r[fin] = r[off + 2 * i];
        r[fin + 1] = r[off + 2 * i + 1];
    }
    true
}

/// Los atributos de un registro de fichero ya arreglado.
fn atributos(r: &[u8]) -> Vec<Atributo<'_>> {
    let mut v = Vec::new();
    if &r[..4] != b"FILE" {
        return v;
    }
    let mut o = u16_en(r, 0x14) as usize;
    while o + 16 <= r.len() {
        let tipo = u32_en(r, o);
        if tipo == 0xFFFF_FFFF {
            break;
        }
        let largo = u32_en(r, o + 4) as usize;
        if largo < 16 || o + largo > r.len() {
            break;
        }
        let a = &r[o..o + largo];
        let residente = a[8] == 0;
        let nombre_vacio = a[9] == 0;
        let datos = if residente {
            let (vl, vo) = (u32_en(a, 0x10) as usize, u16_en(a, 0x14) as usize);
            if vo + vl > a.len() {
                break;
            }
            &a[vo..vo + vl]
        } else {
            a
        };
        v.push(Atributo { tipo, nombre_vacio, residente, datos });
        o += largo;
    }
    v
}

/// Los tramos de un atributo no residente: (VCN inicial, LCN, clusters). Los huecos (dispersos) no se devuelven.
fn tramos(a: &[u8]) -> Option<Vec<(u64, u64, u64)>> {
    if a.len() < 0x40 {
        return None;
    }
    let mut o = u16_en(a, 0x20) as usize;
    let mut vcn = u64_en(a, 0x10);
    let mut lcn: i64 = 0;
    let mut v = Vec::new();
    while o < a.len() && a[o] != 0 {
        let (nl, no) = ((a[o] & 0xF) as usize, (a[o] >> 4) as usize);
        if nl == 0 || nl > 8 || no > 8 || o + 1 + nl + no > a.len() {
            return None;
        }
        let mut largo = 0u64;
        for i in 0..nl {
            largo |= (a[o + 1 + i] as u64) << (8 * i);
        }
        if no > 0 {
            let mut d: i64 = 0;
            for i in 0..no {
                d |= (a[o + 1 + nl + i] as i64) << (8 * i);
            }
            if a[o + nl + no] & 0x80 != 0 && no < 8 {
                d -= 1i64 << (8 * no);
            }
            lcn = lcn.checked_add(d)?;
            if lcn < 0 {
                return None;
            }
            v.push((vcn, lcn as u64, largo));
        }
        vcn += largo;
        o += 1 + nl + no;
    }
    Some(v)
}

impl Vol {
    /// Lee `n` bytes desde el VCN*cluster `desde` de un fichero con esos tramos.
    fn leer_de(&self, d: &mut dyn Disco, t: &[(u64, u64, u64)], desde: u64, n: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(n);
        let mut pos = desde;
        while out.len() < n {
            let vcn = pos / self.cluster;
            let &(v0, l0, c) = t.iter().find(|&&(v0, _, c)| vcn >= v0 && vcn < v0 + c)?;
            let dentro = pos - v0 * self.cluster;
            let quedan_tramo = c * self.cluster - dentro;
            let k = (n - out.len()).min(quedan_tramo as usize).min(1 << 20);
            let byte = (l0 * self.cluster).checked_add(dentro)?;
            if byte + k as u64 > self.largo {
                return None;
            }
            out.extend_from_slice(&leer_bytes(d, self.base + byte, k)?);
            pos += k as u64;
        }
        Some(out)
    }

    /// El registro `n` del MFT, arreglado.
    fn registro(&self, d: &mut dyn Disco, n: u64) -> Option<Vec<u8>> {
        let mut r = self.leer_de(d, &self.mft, n * self.registro as u64, self.registro)?;
        if &r[..4] != b"FILE" || !arreglar(&mut r) {
            return None;
        }
        Some(r)
    }
}

/// El estado de un NTFS que empieza en el byte `base` y mide `largo`.
fn ntfs(d: &mut dyn Disco, base: u64, largo: u64) -> Option<Ntfs> {
    let b = leer_bytes(d, base, 512)?;
    let bps = u16_en(&b, 0x0B) as u64;
    let spc = if b[0x0D] > 0x80 { 1u64 << (256 - b[0x0D] as u32).min(31) } else { b[0x0D] as u64 };
    if !bps.is_power_of_two() || !(256..=4096).contains(&bps) || spc == 0 {
        return None;
    }
    let cluster = bps * spc;
    let total = u64_en(&b, 0x28);
    let tam_reg = |x: u8| if (x as i8) < 0 { 1usize << (-(x as i8) as u32).min(20) } else { x as usize * cluster as usize };
    let (registro, indice) = (tam_reg(b[0x40]), tam_reg(b[0x44]));
    if !(256..=65536).contains(&registro) || !(256..=65536).contains(&indice) {
        return None;
    }
    let mft_lcn = u64_en(&b, 0x30);
    let mut est = Ntfs { cluster: cluster as u32, clusters: total * bps / cluster, ..Default::default() };
    // El registro 0 ($MFT) desde su LCN, para saber dónde están los demás.
    let mut r0 = leer_bytes(d, base + mft_lcn.checked_mul(cluster)?, registro)?;
    if !arreglar(&mut r0) {
        return Some(est);
    }
    let mft = atributos(&r0).iter().find(|a| a.tipo == 0x80 && a.nombre_vacio && !a.residente).and_then(|a| tramos(a.datos))?;
    let v = Vol { base, largo, cluster, registro, indice, mft };
    // $Volume (3): la etiqueta (0x60) y la bandera de sucio (0x70, banderas en el byte 0x0A del valor).
    if let Some(r) = v.registro(d, 3) {
        for a in atributos(&r) {
            if a.tipo == 0x60 && a.residente {
                let u: Vec<u16> = a.datos.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                est.etiqueta = char::decode_utf16(u).map(|c| c.unwrap_or('?')).collect();
            }
            if a.tipo == 0x70 && a.residente && a.datos.len() >= 12 {
                est.sucio = Some(u16_en(a.datos, 0x0A) & 0x0001 != 0);
            }
        }
    }
    // $Bitmap (6): los clusters usados y el último.
    if let Some(r) = v.registro(d, 6) {
        if let Some(a) = atributos(&r).into_iter().find(|a| a.tipo == 0x80 && a.nombre_vacio) {
            let bm = if a.residente {
                Some(a.datos.to_vec())
            } else {
                let t = tramos(a.datos);
                let n = ((est.clusters + 7) / 8) as usize;
                t.and_then(|t| v.leer_de(d, &t, 0, n))
            };
            if let Some(bm) = bm {
                let mut usados = 0u64;
                let mut ultimo = 0u64;
                for (i, &byte) in bm.iter().enumerate() {
                    if byte != 0 {
                        usados += byte.count_ones() as u64;
                        ultimo = i as u64 * 8 + 8 - byte.leading_zeros() as u64;
                    }
                }
                est.usados = Some(usados);
                est.ultimo_usado = Some(ultimo.min(est.clusters));
            }
        }
    }
    est.hibernado = hibernado(d, &v);
    Some(est)
}

/// Busca `hiberfil.sys` en el cajón raíz (registro 5) y mira sus primeros bytes.
fn hibernado(d: &mut dyn Disco, v: &Vol) -> Option<bool> {
    let r = v.registro(d, 5)?;
    let mut refs = None;
    let mirar = |entradas: &[u8]| -> Option<u64> {
        let mut o = 0;
        while o + 16 <= entradas.len() {
            let largo = u16_en(entradas, o + 8) as usize;
            let clave = u16_en(entradas, o + 10) as usize;
            let banderas = u32_en(entradas, o + 12);
            if largo < 16 || o + largo > entradas.len() {
                return None;
            }
            if banderas & 2 != 0 {
                break;
            }
            if clave >= 0x42 && o + 16 + clave <= entradas.len() {
                let k = &entradas[o + 16..o + 16 + clave];
                let n = k[0x40] as usize;
                if 0x42 + 2 * n <= k.len() {
                    let nombre: String = char::decode_utf16((0..n).map(|i| u16_en(k, 0x42 + 2 * i))).map(|c| c.unwrap_or('?')).collect();
                    if nombre.eq_ignore_ascii_case("hiberfil.sys") {
                        return Some(u64_en(entradas, o) & 0xFFFF_FFFF_FFFF);
                    }
                }
            }
            o += largo;
        }
        None
    };
    for a in atributos(&r) {
        if a.tipo == 0x90 && a.residente && a.datos.len() >= 0x20 {
            // $INDEX_ROOT: 16 bytes de raíz y la cabecera del índice en 0x10.
            let h = &a.datos[0x10..];
            let (desde, total) = (u32_en(h, 0) as usize, u32_en(h, 4) as usize);
            if desde < total && total <= h.len() {
                if let Some(x) = mirar(&h[desde..total]) {
                    refs = Some(x);
                }
            }
        }
        if refs.is_none() && a.tipo == 0xA0 && !a.residente {
            let t = tramos(a.datos)?;
            let tam = u64_en(a.datos, 0x30);
            let mut pos = 0u64;
            while pos < tam && refs.is_none() {
                let mut blk = v.leer_de(d, &t, pos, v.indice)?;
                if &blk[..4] == b"INDX" && arreglar(&mut blk) {
                    let h = &blk[0x18..];
                    let (desde, total) = (u32_en(h, 0) as usize, u32_en(h, 4) as usize);
                    if desde < total && total <= h.len() {
                        refs = mirar(&h[desde..total]);
                    }
                }
                pos += v.indice as u64;
            }
        }
    }
    let Some(n) = refs else { return Some(false) };
    let r = v.registro(d, n)?;
    let a = atributos(&r).into_iter().find(|a| a.tipo == 0x80 && a.nombre_vacio)?;
    let cab = if a.residente {
        a.datos.get(..4)?.to_vec()
    } else {
        if u64_en(a.datos, 0x30) == 0 {
            return Some(false);
        }
        let t = tramos(a.datos)?;
        v.leer_de(d, &t, 0, 4)?
    };
    Some(cab.eq_ignore_ascii_case(b"hibr"))
}
