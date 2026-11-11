//! A14: la tabla de particiones del disco, MBR o GPT. Antes el sistema de archivos empezaba siempre en el MB 1.
//!
//! Lo lee el servicio de disco (`nvme`) al arrancar: de aquí sale el tramo de `archivos`, la partición de DashOS, que
//! después ve como si fuera su disco entero. También lo usa `ver` (archivos/herramientas), que es donde están sus
//! pruebas, con tablas rotas a propósito.
//!
//! - MBR: la firma 0x55AA en el sector 0 y cuatro entradas. La de DashOS, del tipo 0x7F ("para desarrollo", sin dueño).
//! - GPT: un MBR protector (tipo 0xEE) y la cabecera en el sector 1, con sus dos CRC. La de DashOS, del tipo
//!   d0a18dbf-bbf4-4b0e-8627-ce5006b50017. La copia del final del disco no se mira.
//!
//! Desconfía de todo: entradas fuera del disco, que se pisan, de longitud cero, que tapan la tabla, dos de DashOS,
//! cabeceras o listas con el CRC mal, tamaños de entrada raros. Ante cualquiera, error: no se adivina.

/// El tipo de la partición de DashOS en un MBR.
pub const TIPO_MBR: u8 = 0x7F;
/// El de una GPT, como está en el disco (los tres primeros campos al revés).
pub const TIPO_GPT: [u8; 16] = [0xbf, 0x8d, 0xa1, 0xd0, 0xf4, 0xbb, 0x0e, 0x4b, 0x86, 0x27, 0xce, 0x50, 0x06, 0xb5, 0x00, 0x17];
/// El sector más grande que se lee.
pub const MAX_SECTOR: usize = 4096;
/// Más entradas de GPT que estas, no (lo normal son 128).
const MAX_ENTRADAS: u32 = 1024;
/// Ni más particiones usadas que estas en una GPT.
const MAX_USADAS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    /// Sin firma 0x55AA en el sector 0.
    Ninguna,
    Mbr,
    Gpt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tabla {
    pub clase: Clase,
    /// La partición de DashOS, en sectores: [desde, hasta).
    pub dashos: Option<(u64, u64)>,
    /// Dónde empieza la primera partición (o la GPT, que va desde el sector 1); `sectores` si no hay ninguna.
    pub primera: u64,
}

/// Lo que no cuadra en la tabla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// El disco no se pudo leer.
    Lectura,
    /// Tamaño de sector que no se admite.
    Sector,
    /// Una partición vacía, que acaba fuera del disco o que tapa la tabla.
    FueraDelDisco,
    /// Dos particiones que se pisan.
    Pisadas,
    /// Más particiones en la GPT de las que se miran.
    Demasiadas,
    /// Más de una partición de DashOS: no se elige.
    DosDeDashOS,
    /// La cabecera de la GPT no vale (firma, tamaño, sus sectores).
    Cabecera,
    /// El CRC de la cabecera o de la lista de entradas no casa.
    Crc,
}

/// CRC-32 (el de la GPT y el de zip), poco a poco: `crc` empieza en 0.
pub fn crc32(crc: u32, datos: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in datos {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn u32_en(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn u64_en(b: &[u8], i: usize) -> u64 {
    u32_en(b, i) as u64 | (u32_en(b, i + 4) as u64) << 32
}

/// Si los tramos [desde, hasta) `a` y `b` se pisan.
fn pisa(a: (u64, u64), b: (u64, u64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Lee la tabla del disco de `sectores` sectores de `tam_sector` bytes. `leer(sector, buf)` llena `buf` (de
/// `tam_sector` bytes) con ese sector; devuelve false si no puede.
pub fn leer_tabla(leer: &mut dyn FnMut(u64, &mut [u8]) -> bool, sectores: u64, tam_sector: usize) -> Result<Tabla, Fallo> {
    if !(512..=MAX_SECTOR).contains(&tam_sector) || !tam_sector.is_power_of_two() || sectores < 2 {
        return Err(Fallo::Sector);
    }
    let mut buf = [0u8; MAX_SECTOR];
    let s0 = &mut buf[..tam_sector];
    if !leer(0, s0) {
        return Err(Fallo::Lectura);
    }
    if s0[510] != 0x55 || s0[511] != 0xAA {
        return Ok(Tabla { clase: Clase::Ninguna, dashos: None, primera: sectores });
    }
    let mut entradas = [(0u8, 0u64, 0u64); 4];
    for (k, e) in entradas.iter_mut().enumerate() {
        let o = 446 + 16 * k;
        *e = (s0[o + 4], u32_en(s0, o + 8) as u64, u32_en(s0, o + 12) as u64);
    }
    if entradas.iter().any(|e| e.0 == 0xEE) {
        return gpt(leer, sectores, tam_sector, &mut buf);
    }
    let mut tabla = Tabla { clase: Clase::Mbr, dashos: None, primera: sectores };
    for (k, &(tipo, desde, cuantos)) in entradas.iter().enumerate() {
        if tipo == 0 {
            continue;
        }
        let hasta = desde + cuantos; // los dos de 32 bits: no se desborda
        if desde == 0 || cuantos == 0 || hasta > sectores {
            return Err(Fallo::FueraDelDisco);
        }
        if entradas[..k].iter().any(|o| o.0 != 0 && pisa((desde, hasta), (o.1, o.1 + o.2))) {
            return Err(Fallo::Pisadas);
        }
        tabla.primera = tabla.primera.min(desde);
        if tipo == TIPO_MBR {
            if tabla.dashos.is_some() {
                return Err(Fallo::DosDeDashOS);
            }
            tabla.dashos = Some((desde, hasta));
        }
    }
    Ok(tabla)
}

/// La GPT: cabecera en el sector 1 y su lista de entradas.
fn gpt(leer: &mut dyn FnMut(u64, &mut [u8]) -> bool, sectores: u64, ts: usize, buf: &mut [u8; MAX_SECTOR]) -> Result<Tabla, Fallo> {
    let c = &mut buf[..ts];
    if !leer(1, c) {
        return Err(Fallo::Lectura);
    }
    let tam_cab = u32_en(c, 12) as usize;
    if &c[..8] != b"EFI PART" || !(92..=ts).contains(&tam_cab) {
        return Err(Fallo::Cabecera);
    }
    let crc = u32_en(c, 16);
    c[16..20].fill(0);
    if crc32(0, &c[..tam_cab]) != crc {
        return Err(Fallo::Crc);
    }
    let (propio, primer_util, ultimo_util, lista) = (u64_en(c, 24), u64_en(c, 40), u64_en(c, 48), u64_en(c, 72));
    let (n, tam_e, crc_lista) = (u32_en(c, 80), u32_en(c, 84), u32_en(c, 88));
    // La lista: tras la cabecera, entera antes de lo utilizable, que a su vez cabe en el disco (la copia va al final).
    let bytes_lista = n as u64 * tam_e as u64;
    let sectores_lista = bytes_lista.div_ceil(ts as u64);
    if propio != 1
        || n == 0
        || n > MAX_ENTRADAS
        || tam_e < 128
        || !tam_e.is_power_of_two()
        || tam_e as usize > ts
        || lista < 2
        || lista.checked_add(sectores_lista).is_none_or(|f| f > primer_util)
        || primer_util > ultimo_util
        || ultimo_util >= sectores
    {
        return Err(Fallo::Cabecera);
    }
    // El CRC de la lista y cada entrada, que se apunta para ver que no se pisan. Las entradas no cruzan de un sector a
    // otro (su tamaño es potencia de dos y no mayor que el sector).
    let por_sector = ts / tam_e as usize;
    let mut crc = 0;
    let mut tabla = Tabla { clase: Clase::Gpt, dashos: None, primera: 1 };
    let mut usadas = [(0u64, 0u64); MAX_USADAS];
    let mut nu = 0;
    // Si una entrada no vale, se sigue leyendo para el CRC: con la lista estropeada, el fallo es el CRC.
    let mut fallo = None;
    for s in 0..sectores_lista {
        let b = &mut buf[..ts];
        if !leer(lista + s, b) {
            return Err(Fallo::Lectura);
        }
        let quedan = n as u64 - s * por_sector as u64;
        let aqui = (por_sector as u64).min(quedan) as usize;
        crc = crc32(crc, &b[..aqui * tam_e as usize]);
        for k in 0..aqui {
            let e = &b[k * tam_e as usize..][..128];
            if fallo.is_some() || e[..16].iter().all(|&x| x == 0) {
                continue;
            }
            fallo = mirar_entrada(e, primer_util, ultimo_util, &mut usadas, &mut nu, &mut tabla).err();
        }
    }
    if crc != crc_lista {
        return Err(Fallo::Crc);
    }
    if let Some(f) = fallo {
        return Err(f);
    }
    Ok(tabla)
}

/// Una entrada usada de la GPT: que caiga en lo utilizable, que no pise a las de antes y si es la de DashOS.
fn mirar_entrada(e: &[u8], primer_util: u64, ultimo_util: u64, usadas: &mut [(u64, u64); MAX_USADAS], nu: &mut usize, tabla: &mut Tabla) -> Result<(), Fallo> {
    let (desde, ultimo) = (u64_en(e, 32), u64_en(e, 40));
    if desde < primer_util || ultimo > ultimo_util || desde > ultimo {
        return Err(Fallo::FueraDelDisco);
    }
    let t = (desde, ultimo + 1);
    if usadas[..*nu].iter().any(|&o| pisa(t, o)) {
        return Err(Fallo::Pisadas);
    }
    if *nu == MAX_USADAS {
        return Err(Fallo::Demasiadas);
    }
    usadas[*nu] = t;
    *nu += 1;
    if e[..16] == TIPO_GPT {
        if tabla.dashos.is_some() {
            return Err(Fallo::DosDeDashOS);
        }
        tabla.dashos = Some(t);
    }
    Ok(())
}
