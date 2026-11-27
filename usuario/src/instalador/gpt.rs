//! S32: la tabla de particiones entera, para el instalador: el MBR, la GPT con sus dos copias (la del principio y la del
//! final del disco) y lo que hay entre medias. `particiones.rs` (A14) lee solo lo que necesitan los servicios al
//! arrancar; esto lee todo lo que hace falta para decidir si se puede escribir, y escribe.
//!
//! Regla: solo se escribe en una GPT con sus dos copias buenas e iguales (o que solo difieren en una partición de
//! DashOS, que es una instalación nuestra cortada a medias). Ante cualquier otra cosa, se lee y se enseña, pero no se
//! toca.
//!
//! Orden al escribir (`escribir_gpt`), pensado para un corte de luz en cualquier punto:
//! 1. la lista de entradas de la copia del final y su cabecera;
//! 2. la lista de entradas de la copia del principio;
//! 3. la cabecera del principio.
//! Cortado entre 1 y 2, la del principio sigue siendo la de antes, entera y buena (el firmware y los demás sistemas la
//! usan); la del final ya es la nueva, y solo difiere en la partición de DashOS: el instalador lo reconoce y deja
//! repetir. Cortado entre 2 y 3, la cabecera del principio no casa con su lista (su CRC) y vale la del final, la nueva,
//! entera.

use super::disco::{Disco, crc32, leer_bytes, poner_u32, poner_u64, u16_en, u32_en, u64_en};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Los tipos de partición que se conocen por su GUID (en la forma del disco, los tres primeros campos al revés).
pub mod tipos {
    pub const DASHOS: [u8; 16] = crate_tipo(0xd0a18dbf, 0xbbf4, 0x4b0e, [0x86, 0x27, 0xce, 0x50, 0x06, 0xb5, 0x00, 0x17]);
    pub const CAJA_LINUX: [u8; 16] = crate_tipo(0xd0a18dbf, 0xbbf4, 0x4b0e, [0x86, 0x27, 0xce, 0x50, 0x06, 0xb5, 0x00, 0x18]);
    pub const ESP: [u8; 16] = crate_tipo(0xC12A7328, 0xF81F, 0x11D2, [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B]);
    pub const MS_DATOS: [u8; 16] = crate_tipo(0xEBD0A0A2, 0xB9E5, 0x4433, [0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7]);
    pub const MS_RESERVADA: [u8; 16] = crate_tipo(0xE3C9E316, 0x0B5C, 0x4DB8, [0x81, 0x7D, 0xF9, 0x2D, 0xF0, 0x02, 0x15, 0xAE]);
    pub const MS_RECUPERACION: [u8; 16] = crate_tipo(0xDE94BBA4, 0x06D1, 0x4D40, [0xA1, 0x6A, 0xBF, 0xD5, 0x01, 0x79, 0xD6, 0xAC]);
    pub const MS_LDM_META: [u8; 16] = crate_tipo(0x5808C8AA, 0x7E8F, 0x42E0, [0x85, 0xD2, 0xE1, 0xE9, 0x04, 0x34, 0xCF, 0xB3]);
    pub const MS_LDM_DATOS: [u8; 16] = crate_tipo(0xAF9B60A0, 0x1431, 0x4F62, [0xBC, 0x68, 0x33, 0x11, 0x71, 0x4A, 0x69, 0xAD]);
    pub const LINUX: [u8; 16] = crate_tipo(0x0FC63DAF, 0x8483, 0x4772, [0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4]);
    pub const LINUX_RAIZ_X86_64: [u8; 16] = crate_tipo(0x4F68BCE3, 0xE8CD, 0x4DB1, [0x96, 0xE7, 0xFB, 0xCA, 0xF9, 0x84, 0xB7, 0x09]);
    pub const LINUX_RAIZ_ARM64: [u8; 16] = crate_tipo(0xB921B045, 0x1DF0, 0x41C3, [0xAF, 0x44, 0x4C, 0x6F, 0x28, 0x0D, 0x3F, 0xAE]);
    pub const LINUX_HOME: [u8; 16] = crate_tipo(0x933AC7E1, 0x2EB4, 0x4F13, [0xB8, 0x44, 0x0E, 0x14, 0xE2, 0xAE, 0xF9, 0x15]);
    pub const LINUX_SWAP: [u8; 16] = crate_tipo(0x0657FD6D, 0xA4AB, 0x43C4, [0x84, 0xE5, 0x09, 0x33, 0xC8, 0x4B, 0x4F, 0x4F]);
    pub const LINUX_LUKS: [u8; 16] = crate_tipo(0xCA7D7CCB, 0x63ED, 0x4C53, [0x86, 0x1C, 0x17, 0x42, 0x53, 0x60, 0x59, 0xCC]);
    pub const LINUX_LVM: [u8; 16] = crate_tipo(0xE6D6D379, 0xF507, 0x44C2, [0xA2, 0x3C, 0x23, 0x8F, 0x2A, 0x3D, 0xF9, 0x28]);
    pub const LINUX_RAID: [u8; 16] = crate_tipo(0xA19D880F, 0x05FC, 0x4D3B, [0xA0, 0x06, 0x74, 0x3F, 0x0F, 0x84, 0x91, 0x1E]);
    pub const LINUX_XBOOTLDR: [u8; 16] = crate_tipo(0xBC13C2FF, 0x59E6, 0x4262, [0xA3, 0x52, 0xB2, 0x75, 0xFD, 0x6F, 0x71, 0x72]);
    pub const BIOS_ARRANQUE: [u8; 16] = crate_tipo(0x21686148, 0x6449, 0x6E6F, [0x74, 0x4E, 0x65, 0x65, 0x64, 0x45, 0x46, 0x49]);
    pub const APFS: [u8; 16] = crate_tipo(0x7C3457EF, 0x0000, 0x11AA, [0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC]);
    pub const APPLE_HFS: [u8; 16] = crate_tipo(0x48465300, 0x0000, 0x11AA, [0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC]);

    const fn crate_tipo(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
        let a = a.to_le_bytes();
        let b = b.to_le_bytes();
        let c = c.to_le_bytes();
        [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
    }

    /// Su nombre en llano, para el plan y la ventana.
    pub fn nombre(t: &[u8; 16]) -> &'static str {
        match *t {
            DASHOS => "DashOS",
            CAJA_LINUX => "caja Linux de DashOS",
            ESP => "partición de arranque EFI",
            MS_DATOS => "datos de Windows",
            MS_RESERVADA => "reservada de Windows",
            MS_RECUPERACION => "recuperación de Windows",
            MS_LDM_META | MS_LDM_DATOS => "disco dinámico de Windows",
            LINUX | LINUX_RAIZ_X86_64 | LINUX_RAIZ_ARM64 | LINUX_HOME | LINUX_XBOOTLDR => "Linux",
            LINUX_SWAP => "intercambio de Linux",
            LINUX_LUKS => "Linux cifrado (LUKS)",
            LINUX_LVM => "LVM de Linux",
            LINUX_RAID => "RAID de Linux",
            BIOS_ARRANQUE => "arranque BIOS de GRUB",
            APFS => "macOS (APFS)",
            APPLE_HFS => "macOS (HFS+)",
            _ => "desconocida",
        }
    }
}

/// Lo que no deja leer la tabla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    Lectura,
    Sector,
}

/// Cómo están las dos copias de la GPT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copias {
    /// Las dos buenas e iguales.
    Iguales,
    /// La del principio, buena; la del final, rota o falta.
    RespaldoRoto,
    /// La del principio, rota; la del final, buena.
    PrincipalRota,
    /// Las dos buenas, pero distintas en algo más que una partición de DashOS.
    Distintas,
    /// Las dos buenas, y solo difieren en particiones de DashOS: una instalación nuestra cortada.
    SoloDashOS,
    /// Ninguna buena.
    Ninguna,
}

/// La clase de tabla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    /// Sin firma 0x55AA ni GPT: disco vacío o sin tabla.
    Ninguna,
    Mbr,
    Gpt,
    /// GPT con un MBR que no es solo el protector (híbrido): no se toca.
    GptHibrida,
}

/// Una partición, de la tabla que sea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Particion {
    /// Su número en la tabla (1 = la primera entrada).
    pub numero: u32,
    /// [desde, hasta) en sectores.
    pub desde: u64,
    pub hasta: u64,
    /// Su tipo GPT, o el de MBR en el byte 0 (y el resto a cero) con `mbr = true`.
    pub tipo: [u8; 16],
    pub mbr: bool,
    /// El GUID único de la partición (GPT).
    pub guid: [u8; 16],
    pub atributos: u64,
    /// El nombre de la GPT (UTF-16 en el disco), o vacío.
    pub nombre: String,
}

/// La cabecera de una copia de la GPT, ya comprobada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cabecera {
    pub propio: u64,
    pub otro: u64,
    pub primer_util: u64,
    pub ultimo_util: u64,
    pub disco_guid: [u8; 16],
    pub lista: u64,
    pub n: u32,
    pub tam_e: u32,
    pub crc_lista: u32,
}

/// La tabla leída.
#[derive(Clone, Debug)]
pub struct Tabla {
    pub clase: Clase,
    pub copias: Copias,
    /// La cabecera que vale (la del principio si está buena).
    pub cabecera: Option<Cabecera>,
    /// La lista de entradas cruda de la copia que vale (n * tam_e bytes).
    pub lista: Vec<u8>,
    pub particiones: Vec<Particion>,
    /// Lo raro que se vio, en llano (una línea cada cosa).
    pub avisos: Vec<String>,
    /// La huella de la tabla (el CRC de los primeros 34 sectores y la cabecera del final): el plan la lleva para no
    /// aplicarse a un disco que cambió.
    pub huella: u32,
}

/// Más entradas que estas, no.
const MAX_ENTRADAS: u32 = 1024;

fn sumar_aviso(t: &mut Tabla, s: &str) {
    t.avisos.push(String::from(s));
}

/// Lee una cabecera de GPT del sector `lba` y su lista. `None` si algo no vale.
/// El tercer valor dice si la lista casa con el CRC de su cabecera.
fn leer_copia(d: &mut dyn Disco, lba: u64) -> Option<(Cabecera, Vec<u8>, bool)> {
    let ts = d.tam_sector();
    let sectores = d.sectores();
    let mut c = vec![0u8; ts];
    d.leer(lba, &mut c).ok()?;
    let tam_cab = u32_en(&c, 12) as usize;
    if &c[..8] != b"EFI PART" || !(92..=ts).contains(&tam_cab) || u32_en(&c, 8) != 0x0001_0000 {
        return None;
    }
    let crc = u32_en(&c, 16);
    c[16..20].fill(0);
    if crc32(0, &c[..tam_cab]) != crc {
        return None;
    }
    let mut g = [0u8; 16];
    g.copy_from_slice(&c[56..72]);
    let cab = Cabecera {
        propio: u64_en(&c, 24),
        otro: u64_en(&c, 32),
        primer_util: u64_en(&c, 40),
        ultimo_util: u64_en(&c, 48),
        disco_guid: g,
        lista: u64_en(&c, 72),
        n: u32_en(&c, 80),
        tam_e: u32_en(&c, 84),
        crc_lista: u32_en(&c, 88),
    };
    let bytes = cab.n as u64 * cab.tam_e as u64;
    let sl = bytes.div_ceil(ts as u64);
    if cab.propio != lba
        || cab.n == 0
        || cab.n > MAX_ENTRADAS
        || cab.tam_e < 128
        || !cab.tam_e.is_power_of_two()
        || cab.tam_e as usize > ts
        || cab.primer_util > cab.ultimo_util
        || cab.ultimo_util >= sectores
        || cab.lista < 2
        || cab.lista.checked_add(sl).is_none_or(|f| f > sectores)
    {
        return None;
    }
    // La lista no pisa lo utilizable ni la cabecera: la del principio va antes de lo utilizable, la del final después.
    if lba == 1 {
        if cab.lista + sl > cab.primer_util {
            return None;
        }
    } else if cab.lista <= cab.ultimo_util || cab.lista + sl > lba {
        return None;
    }
    let lista = leer_bytes(d, cab.lista * ts as u64, bytes as usize)?;
    let buena = crc32(0, &lista) == cab.crc_lista;
    Some((cab, lista, buena))
}

/// Las particiones usadas de una lista de entradas.
fn entradas(cab: &Cabecera, lista: &[u8]) -> Vec<Particion> {
    let mut v = Vec::new();
    for k in 0..cab.n as usize {
        let e = &lista[k * cab.tam_e as usize..][..128];
        if e[..16].iter().all(|&x| x == 0) {
            continue;
        }
        let mut tipo = [0u8; 16];
        tipo.copy_from_slice(&e[..16]);
        let mut guid = [0u8; 16];
        guid.copy_from_slice(&e[16..32]);
        let mut nombre = String::new();
        let mut u = Vec::new();
        for i in 0..36 {
            let c = u16_en(e, 56 + 2 * i);
            if c == 0 {
                break;
            }
            u.push(c);
        }
        for c in char::decode_utf16(u.iter().copied()) {
            nombre.push(c.unwrap_or('?'));
        }
        v.push(Particion { numero: k as u32 + 1, desde: u64_en(e, 32), hasta: u64_en(e, 40).wrapping_add(1), tipo, mbr: false, guid, atributos: u64_en(e, 48), nombre });
    }
    v
}

/// Si dos listas solo difieren en entradas de DashOS (una vacía en una y de DashOS en la otra, o de DashOS en las dos)
/// o en el final de una partición encogida para DashOS.
fn difieren_solo_dashos(a: &Cabecera, la: &[u8], b: &Cabecera, lb: &[u8]) -> bool {
    if a.n != b.n || a.tam_e != b.tam_e || a.disco_guid != b.disco_guid || a.primer_util != b.primer_util || a.ultimo_util != b.ultimo_util {
        return false;
    }
    let te = a.tam_e as usize;
    let mut alguna = false;
    for k in 0..a.n as usize {
        let (ea, eb) = (&la[k * te..][..te], &lb[k * te..][..te]);
        if ea == eb {
            continue;
        }
        let ok = |e: &[u8]| e.iter().all(|&x| x == 0) || e[..16] == tipos::DASHOS;
        // S32 (encoger): o la misma partición (tipo, GUID, principio, nombre), solo más corta en una de las dos: la
        // que se encogió para hacer sitio a DashOS en la misma escritura.
        let encogida = ea[..40] == eb[..40] && ea[48..] == eb[48..];
        if !(ok(ea) && ok(eb)) && !encogida {
            return false;
        }
        alguna = true;
    }
    alguna
}

/// Lee la tabla del disco entero.
pub fn leer(d: &mut dyn Disco) -> Result<Tabla, Fallo> {
    let ts = d.tam_sector();
    let sectores = d.sectores();
    if !(512..=4096).contains(&ts) || !ts.is_power_of_two() || sectores < 64 {
        return Err(Fallo::Sector);
    }
    let mut t = Tabla { clase: Clase::Ninguna, copias: Copias::Ninguna, cabecera: None, lista: Vec::new(), particiones: Vec::new(), avisos: Vec::new(), huella: 0 };
    // La huella: lo de las dos puntas del disco (34 sectores de 512 al principio, 33 al final).
    let cab_bytes = 34 * 512;
    let ini = leer_bytes(d, 0, cab_bytes.max(2 * ts)).ok_or(Fallo::Lectura)?;
    let fin_n = (33 * 512usize).div_ceil(ts) as u64;
    let fin = leer_bytes(d, (sectores - fin_n) * ts as u64, (fin_n as usize) * ts).ok_or(Fallo::Lectura)?;
    t.huella = crc32(crc32(0, &ini), &fin);

    let s0 = &ini[..ts];
    let firma = s0[510] == 0x55 && s0[511] == 0xAA;
    let mut mbr = [(0u8, 0u64, 0u64); 4];
    if firma {
        for (k, e) in mbr.iter_mut().enumerate() {
            let o = 446 + 16 * k;
            *e = (s0[o + 4], u32_en(s0, o + 8) as u64, u32_en(s0, o + 12) as u64);
        }
    }
    let protector = mbr.iter().any(|e| e.0 == 0xEE);
    // La GPT se mira aunque falte el MBR protector (hay discos así); si no hay firma, se dice.
    let principal = leer_copia(d, 1);
    let otro = principal.as_ref().map(|p| p.0.otro).filter(|&o| o < sectores).unwrap_or(sectores - 1);
    let mut respaldo = leer_copia(d, otro);
    if respaldo.is_none() && otro != sectores - 1 {
        respaldo = leer_copia(d, sectores - 1);
    }
    if principal.is_some() || respaldo.is_some() || protector {
        t.clase = Clase::Gpt;
        if !protector {
            sumar_aviso(&mut t, "La GPT no tiene su MBR protector.");
        }
        if mbr.iter().filter(|e| e.0 != 0).count() > 1 && protector {
            t.clase = Clase::GptHibrida;
            sumar_aviso(&mut t, "El MBR es híbrido (además del protector tiene otras particiones): no se toca.");
        }
        let buena = |c: &Option<(Cabecera, Vec<u8>, bool)>| c.as_ref().filter(|c| c.2).map(|c| (c.0, c.1.clone()));
        let (bp, br) = (buena(&principal), buena(&respaldo));
        // Una copia con su cabecera buena pero la lista sin casar con su CRC, junto a otra entera buena, de la que solo
        // difiere en entradas de DashOS: es una escritura nuestra cortada a medias (ver el orden arriba).
        let cortada = |a: &(Cabecera, Vec<u8>), o: &Option<(Cabecera, Vec<u8>, bool)>| {
            o.as_ref().is_some_and(|(c, l, ok)| !*ok && (l == &a.1 || difieren_solo_dashos(&a.0, &a.1, c, l)))
        };
        match (&bp, &br) {
            (Some((a, la)), Some((b, lb))) => {
                let iguales = la == lb && a.disco_guid == b.disco_guid && a.primer_util == b.primer_util && a.ultimo_util == b.ultimo_util && a.n == b.n && a.tam_e == b.tam_e && a.otro == b.propio && b.otro == 1;
                if iguales {
                    t.copias = Copias::Iguales;
                } else if difieren_solo_dashos(a, la, b, lb) {
                    t.copias = Copias::SoloDashOS;
                    sumar_aviso(&mut t, "Las dos copias de la GPT solo difieren en la partición de DashOS: una instalación cortada a medias.");
                } else {
                    t.copias = Copias::Distintas;
                    sumar_aviso(&mut t, "Las dos copias de la GPT (la del principio y la del final del disco) no son iguales: no se escribe en este disco.");
                }
                t.cabecera = Some(*a);
                t.lista = la.clone();
            }
            (Some(a), None) if cortada(a, &respaldo) => {
                t.copias = Copias::SoloDashOS;
                sumar_aviso(&mut t, "La copia de la GPT del final se quedó a medias en una instalación de DashOS cortada.");
                t.cabecera = Some(a.0);
                t.lista = a.1.clone();
            }
            (None, Some(b)) if cortada(b, &principal) => {
                t.copias = Copias::SoloDashOS;
                sumar_aviso(&mut t, "La GPT del principio se quedó a medias en una instalación de DashOS cortada (vale la del final).");
                // Se escribe como si la del principio fuera la del final, con su sitio de siempre.
                let p = principal.as_ref().unwrap().0;
                t.cabecera = Some(Cabecera { propio: 1, otro: b.0.propio, lista: p.lista, ..b.0 });
                t.lista = b.1.clone();
            }
            (Some((a, la)), None) => {
                t.copias = Copias::RespaldoRoto;
                sumar_aviso(&mut t, "La copia de la GPT del final del disco está rota o falta: no se escribe en este disco.");
                t.cabecera = Some(*a);
                t.lista = la.clone();
            }
            (None, Some((b, lb))) => {
                t.copias = Copias::PrincipalRota;
                sumar_aviso(&mut t, "La GPT del principio del disco está rota (vale la del final): no se escribe en este disco.");
                t.cabecera = Some(*b);
                t.lista = lb.clone();
            }
            (None, None) => {
                t.copias = Copias::Ninguna;
                sumar_aviso(&mut t, "El MBR dice que hay GPT, pero ninguna de sus dos copias vale: no se escribe en este disco.");
            }
        }
        if let Some(c) = t.cabecera {
            t.particiones = entradas(&c, &t.lista);
            if c.ultimo_util + 1 + (c.n as u64 * c.tam_e as u64).div_ceil(ts as u64) < sectores - 1 {
                sumar_aviso(&mut t, "La GPT no llega al final del disco (el disco creció): hay sitio al final que la tabla no cubre.");
            }
        }
    } else if firma && mbr.iter().any(|e| e.0 != 0) {
        t.clase = Clase::Mbr;
        for (k, &(tipo, desde, n)) in mbr.iter().enumerate() {
            if tipo == 0 || n == 0 {
                continue;
            }
            let mut tt = [0u8; 16];
            tt[0] = tipo;
            t.particiones.push(Particion { numero: k as u32 + 1, desde, hasta: desde + n, tipo: tt, mbr: true, guid: [0; 16], atributos: 0, nombre: String::new() });
        }
        if t.particiones.iter().any(|p| matches!(p.tipo[0], 0x05 | 0x0F | 0x85)) {
            sumar_aviso(&mut t, "El MBR tiene una partición extendida: las lógicas que hay dentro no se detallan.");
        }
    }
    // Lo que se pisa o se sale del disco: se dice, y no se escribe.
    let mut ps = t.particiones.clone();
    ps.sort_by_key(|p| p.desde);
    for w in ps.windows(2) {
        if w[1].desde < w[0].hasta {
            sumar_aviso(&mut t, "Hay particiones que se pisan: no se escribe en este disco.");
            t.copias = if t.clase == Clase::Gpt { Copias::Distintas } else { t.copias };
            break;
        }
    }
    if ps.iter().any(|p| p.hasta > sectores || p.desde >= p.hasta || p.desde == 0) {
        sumar_aviso(&mut t, "Hay particiones fuera del disco: no se escribe en este disco.");
        if t.clase == Clase::Gpt {
            t.copias = Copias::Distintas;
        }
    }
    Ok(t)
}

impl Tabla {
    /// Si se puede escribir en esta tabla (añadir una partición en un hueco).
    pub fn escribible(&self) -> bool {
        self.clase == Clase::Gpt && matches!(self.copias, Copias::Iguales | Copias::SoloDashOS) && self.cabecera.is_some()
    }

    /// Los huecos libres de la GPT, alineados a `alinear` sectores: [desde, hasta).
    pub fn huecos(&self, alinear: u64) -> Vec<(u64, u64)> {
        let Some(c) = self.cabecera else { return Vec::new() };
        let mut ps: Vec<(u64, u64)> = self.particiones.iter().map(|p| (p.desde, p.hasta)).collect();
        ps.sort();
        let mut v = Vec::new();
        let mut cur = c.primer_util;
        let fin = c.ultimo_util + 1;
        for (a, b) in ps.into_iter().chain(core::iter::once((fin, fin))) {
            let desde = cur.div_ceil(alinear) * alinear;
            let hasta = a.min(fin) / alinear * alinear;
            if hasta > desde {
                v.push((desde, hasta));
            }
            cur = cur.max(b);
        }
        v
    }
}

/// Una entrada nueva de la GPT.
#[derive(Clone, Copy, Debug)]
pub struct Nueva<'a> {
    pub tipo: [u8; 16],
    pub guid: [u8; 16],
    pub desde: u64,
    pub hasta: u64,
    pub nombre: &'a str,
}

/// Arma la entrada de 128 bytes.
fn entrada(n: &Nueva, e: &mut [u8]) {
    e[..128].fill(0);
    e[..16].copy_from_slice(&n.tipo);
    e[16..32].copy_from_slice(&n.guid);
    poner_u64(e, 32, n.desde);
    poner_u64(e, 40, n.hasta - 1);
    for (i, c) in n.nombre.encode_utf16().take(36).enumerate() {
        e[56 + 2 * i..58 + 2 * i].copy_from_slice(&c.to_le_bytes());
    }
}

/// Lo que puede salir mal al escribir la GPT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FalloEscribir {
    /// La tabla no se puede escribir (`Tabla::escribible`).
    NoEscribible,
    /// No hay entrada libre.
    SinEntrada,
    /// La partición nueva pisa otra o se sale de lo utilizable.
    NoCabe,
    /// El disco no dejó escribir: puede haber quedado a medias (ver el orden en la cabecera del módulo).
    Escritura,
}

/// Una cabecera de GPT de `ts` bytes: la copia en `propio`, la otra en `otro`, su lista en `lista_lba`.
fn cabecera_nueva(cab: &Cabecera, propio: u64, otro: u64, lista_lba: u64, crc_lista: u32, ts: usize) -> Vec<u8> {
    let mut c = vec![0u8; ts];
    c[..8].copy_from_slice(b"EFI PART");
    poner_u32(&mut c, 8, 0x0001_0000);
    poner_u32(&mut c, 12, 92);
    poner_u64(&mut c, 24, propio);
    poner_u64(&mut c, 32, otro);
    poner_u64(&mut c, 40, cab.primer_util);
    poner_u64(&mut c, 48, cab.ultimo_util);
    c[56..72].copy_from_slice(&cab.disco_guid);
    poner_u64(&mut c, 72, lista_lba);
    poner_u32(&mut c, 80, cab.n);
    poner_u32(&mut c, 84, cab.tam_e);
    poner_u32(&mut c, 88, crc_lista);
    let crc = crc32(0, &c[..92]);
    poner_u32(&mut c, 16, crc);
    c
}

/// La lista nueva: la de `t` con las entradas de DashOS quitadas (si `quitar_dashos`) y `nuevas` en los primeros
/// huecos de la lista. Comprueba que caben.
pub fn lista_nueva(t: &Tabla, nuevas: &[Nueva], quitar_dashos: bool) -> Result<Vec<u8>, FalloEscribir> {
    let Some(cab) = t.cabecera else { return Err(FalloEscribir::NoEscribible) };
    if !t.escribible() {
        return Err(FalloEscribir::NoEscribible);
    }
    let te = cab.tam_e as usize;
    let mut lista = t.lista.clone();
    if quitar_dashos {
        for k in 0..cab.n as usize {
            if lista[k * te..k * te + 16] == tipos::DASHOS {
                lista[k * te..(k + 1) * te].fill(0);
            }
        }
    }
    // Las que quedan, para ver que la nueva no pisa ninguna.
    let quedan: Vec<(u64, u64)> = (0..cab.n as usize)
        .filter(|k| lista[k * te..k * te + 16].iter().any(|&x| x != 0))
        .map(|k| (u64_en(&lista, k * te + 32), u64_en(&lista, k * te + 40) + 1))
        .collect();
    let mut puestas: Vec<(u64, u64)> = Vec::new();
    for n in nuevas {
        if n.desde < cab.primer_util || n.hasta > cab.ultimo_util + 1 || n.desde >= n.hasta {
            return Err(FalloEscribir::NoCabe);
        }
        if quedan.iter().chain(puestas.iter()).any(|&(a, b)| n.desde < b && a < n.hasta) {
            return Err(FalloEscribir::NoCabe);
        }
        let libre = (0..cab.n as usize).find(|k| lista[k * te..k * te + 16].iter().all(|&x| x == 0)).ok_or(FalloEscribir::SinEntrada)?;
        entrada(n, &mut lista[libre * te..(libre + 1) * te]);
        puestas.push((n.desde, n.hasta));
    }
    Ok(lista)
}

/// Escribe la GPT de `t` con la lista `lista` (de `lista_nueva`), en el orden de la cabecera del módulo, vaciando
/// entre paso y paso. Con `t.copias == PrincipalRota` no se llega aquí (no es escribible).
pub fn escribir_gpt(d: &mut dyn Disco, t: &Tabla, lista: &[u8]) -> Result<(), FalloEscribir> {
    let Some(cab) = t.cabecera else { return Err(FalloEscribir::NoEscribible) };
    if !t.escribible() || lista.len() != cab.n as usize * cab.tam_e as usize {
        return Err(FalloEscribir::NoEscribible);
    }
    let ts = d.tam_sector();
    let sl = (lista.len() as u64).div_ceil(ts as u64);
    let otro = cab.otro;
    // La lista de la copia del final, justo antes de su cabecera (como la dejan todos).
    let lista_final = otro - sl;
    if lista_final <= cab.ultimo_util {
        return Err(FalloEscribir::NoEscribible);
    }
    let mut rellena = lista.to_vec();
    rellena.resize(sl as usize * ts, 0);
    let crc_lista = crc32(0, lista);
    let e = |r: Result<(), ()>| r.map_err(|_| FalloEscribir::Escritura);
    // 1. La copia del final.
    e(d.escribir(lista_final, &rellena))?;
    e(d.escribir(otro, &cabecera_nueva(&cab, otro, 1, lista_final, crc_lista, ts)))?;
    e(d.vaciar())?;
    // 2. La lista del principio.
    e(d.escribir(cab.lista, &rellena))?;
    e(d.vaciar())?;
    // 3. La cabecera del principio.
    e(d.escribir(1, &cabecera_nueva(&cab, 1, otro, cab.lista, crc_lista, ts)))?;
    e(d.vaciar())?;
    Ok(())
}

/// Una GPT nueva y vacía para un disco entero (modo "disco entero"): MBR protector, las dos cabeceras y sus listas de
/// 128 entradas, con lo utilizable alineado al MiB. Devuelve la tabla en memoria tal como la leería `leer` (sin
/// escribir nada aún): `escribir_gpt` la escribe con su lista.
pub fn tabla_vacia(sectores: u64, ts: usize, disco_guid: [u8; 16]) -> Tabla {
    let sl = (128 * 128u64).div_ceil(ts as u64);
    let mib = (1 << 20) / ts as u64;
    let cab = Cabecera {
        propio: 1,
        otro: sectores - 1,
        primer_util: mib.max(2 + sl),
        ultimo_util: sectores - 2 - sl,
        disco_guid,
        lista: 2,
        n: 128,
        tam_e: 128,
        crc_lista: 0,
    };
    Tabla { clase: Clase::Gpt, copias: Copias::Iguales, cabecera: Some(cab), lista: vec![0; 128 * 128], particiones: Vec::new(), avisos: Vec::new(), huella: 0 }
}

/// El MBR protector de un disco de `sectores` sectores (para el modo "disco entero"; se escribe el último).
pub fn mbr_protector(sectores: u64, ts: usize) -> Vec<u8> {
    let mut s = vec![0u8; ts];
    let o = 446;
    s[o + 1] = 0x00;
    s[o + 2] = 0x02;
    s[o + 4] = 0xEE;
    s[o + 5] = 0xFF;
    s[o + 6] = 0xFF;
    s[o + 7] = 0xFF;
    poner_u32(&mut s, o + 8, 1);
    poner_u32(&mut s, o + 12, (sectores - 1).min(0xFFFF_FFFF) as u32);
    s[510] = 0x55;
    s[511] = 0xAA;
    s
}

/// S32 (encoger): la tabla `t` con la partición `numero` acabando en `nuevo_hasta` (exclusivo), en su lista y en sus
/// particiones. Solo encoge (nunca crece ni mueve el principio).
pub fn con_encogida(t: &Tabla, numero: u32, nuevo_hasta: u64) -> Result<Tabla, FalloEscribir> {
    let Some(cab) = t.cabecera else { return Err(FalloEscribir::NoEscribible) };
    let k = (numero as usize).checked_sub(1).filter(|&k| k < cab.n as usize).ok_or(FalloEscribir::NoCabe)?;
    let mut n = t.clone();
    let te = cab.tam_e as usize;
    let e = &mut n.lista[k * te..(k + 1) * te];
    let (desde, ultimo) = (u64_en(e, 32), u64_en(e, 40));
    if e[..16].iter().all(|&x| x == 0) || nuevo_hasta <= desde || nuevo_hasta > ultimo + 1 {
        return Err(FalloEscribir::NoCabe);
    }
    poner_u64(e, 40, nuevo_hasta - 1);
    for q in n.particiones.iter_mut().filter(|q| q.numero == numero) {
        q.hasta = nuevo_hasta;
    }
    Ok(n)
}
