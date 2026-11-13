//! El bloque de arranque de un disquete (S28, decisión del usuario del 2026-11-13: "aviso sin firmas de bootblock
//! raro"). Los dos primeros bloques de un disco de AmigaDOS: `DOS` y sus banderas, la suma, el bloque raíz y, desde el
//! byte 12, el código que Kickstart ejecuta al arrancar del disco, si la suma cuadra. Ahí vivían los virus de arranque
//! (SCA, Byte Bandit, Lamer Exterminator).
//!
//! No hay firmas de virus: se mira qué hace el código, con lo que dicen los autodocs y el manual del 68000:
//!
//! - **No arranca**: no empieza por `DOS` o su suma no cuadra. Kickstart no lo ejecuta.
//! - **Sin código**: arranca, pero no hay nada desde el byte 12 (un disco que no es de arranque, formateado así).
//! - **Como el de AmigaDOS**: código corto que solo busca `dos.library` (`FindResident`, y quizá `OpenLibrary`) y
//!   vuelve. Es lo que deja `Install`.
//! - **Toca lo que tocaban los virus**: llama a `SetFunction`, `DoIO`, `AllocAbs`, `SumKickData`, `ColdReboot`,
//!   `Supervisor`, `SetIntVector`, `AddIntServer` o `CacheClearU`, o escribe en `ColdCapture`, `CoolCapture`,
//!   `WarmCapture`, `KickMemPtr`, `KickTagPtr` o `KickCheckSum` de ExecBase. Puede ser un virus; también un
//!   antivirus o un cargador de demo. Se dice qué toca.
//! - **Otro código**: ni lo uno ni lo otro (un juego o un cargador propio). No se sabe más.
//!
//! El código no se ejecuta nunca: solo se recorre como palabras de 16 bits. La suma es la de la FAQ del ADF: los 256
//! LONG sumados con el acarreo de vuelta, con la suma a 0; la que se guarda es el complemento.

use alloc::vec::Vec;

/// Lo que mide: dos bloques.
pub const TAMANO: usize = 1024;

/// Las funciones de exec que miran las llamadas `JSR d16(A6)` (desplazamiento del LVO, nombre).
const LLAMADAS: &[(i16, &str)] = &[
    (-420, "SetFunction"),
    (-456, "DoIO"),
    (-462, "SendIO"),
    (-204, "AllocAbs"),
    (-612, "SumKickData"),
    (-726, "ColdReboot"),
    (-30, "Supervisor"),
    (-162, "SetIntVector"),
    (-168, "AddIntServer"),
    (-636, "CacheClearU"),
];
/// Las de un bloque de arranque normal: `FindResident` y `OpenLibrary`/`OldOpenLibrary`, `CloseLibrary`.
const NORMALES: &[i16] = &[-96, -552, -408, -414];
/// Los campos de ExecBase de los virus residentes (desplazamiento, nombre).
const CAMPOS: &[(u16, &str)] = &[
    (42, "ColdCapture"),
    (46, "CoolCapture"),
    (50, "WarmCapture"),
    (546, "KickMemPtr"),
    (550, "KickTagPtr"),
    (554, "KickCheckSum"),
];
/// Lo más largo (en bytes desde el 12 hasta el último que no es 0) que mide un código "como el de AmigaDOS".
const CORTO: usize = 160;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arranque {
    NoArranca,
    SinCodigo,
    ComoAmigaDos,
    /// Lo que toca de lo de los virus (nombres de `LLAMADAS` y `CAMPOS`) y los bytes de código.
    Sospechoso { toca: Vec<&'static str>, bytes: usize },
    Otro { bytes: usize },
}

impl Arranque {
    /// Una frase para el usuario.
    pub fn frase(&self) -> alloc::string::String {
        use alloc::format;
        match self {
            Arranque::NoArranca => "no arranca (sin DOS o con la suma mal): Kickstart no lo ejecuta".into(),
            Arranque::SinCodigo => "arranca, sin código".into(),
            Arranque::ComoAmigaDos => "como el de AmigaDOS (busca dos.library y vuelve)".into(),
            Arranque::Sospechoso { toca, bytes } => format!(
                "AVISO: su código ({} bytes) toca lo que tocaban los virus de arranque: {}; puede ser un virus (o un antivirus o un cargador)",
                bytes,
                toca.join(", ")
            ),
            Arranque::Otro { bytes } => {
                format!("código propio ({} bytes): un juego o un cargador; no se sabe más", bytes)
            }
        }
    }
}

fn l(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes(b[off..off + 4].try_into().unwrap())
}

fn w(b: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([b[off], b[off + 1]])
}

/// La suma que va en los bytes 4 a 7 para que el bloque arranque.
pub fn suma(b: &[u8; TAMANO]) -> u32 {
    let mut s: u32 = 0;
    for i in (0..TAMANO).step_by(4) {
        let v = if i == 4 { 0 } else { l(b, i) };
        let (n, acarreo) = s.overflowing_add(v);
        s = n.wrapping_add(acarreo as u32);
    }
    !s
}

/// Qué es el bloque de arranque `b`.
pub fn analizar(b: &[u8; TAMANO]) -> Arranque {
    if &b[0..3] != b"DOS" || l(b, 4) != suma(b) {
        return Arranque::NoArranca;
    }
    let codigo = &b[12..];
    let Some(ultimo) = codigo.iter().rposition(|&x| x != 0) else { return Arranque::SinCodigo };
    let bytes = ultimo + 1;
    let mut toca: Vec<&'static str> = Vec::new();
    let mut solo_normales = true;
    let mut hay_llamadas = false;
    let mut i = 0;
    while i + 4 <= codigo.len() {
        let op = w(codigo, i);
        // JSR d16(A6): 4EAE dddd.
        if op == 0x4EAE {
            let d = w(codigo, i + 2) as i16;
            hay_llamadas = true;
            if let Some(&(_, n)) = LLAMADAS.iter().find(|c| c.0 == d)
                && !toca.contains(&n)
            {
                toca.push(n);
            }
            if !NORMALES.contains(&d) {
                solo_normales = false;
            }
        }
        // MOVE.L Dn/An,d16(A6): 2D40-2D4F dddd; MOVE.L #imm,d16(A6): 2D7C iiii iiii dddd.
        let campo = match op {
            0x2D40..=0x2D4F => Some(w(codigo, i + 2)),
            0x2D7C if i + 8 <= codigo.len() => Some(w(codigo, i + 6)),
            _ => None,
        };
        if let Some(c) = campo
            && let Some(&(_, n)) = CAMPOS.iter().find(|x| x.0 == c)
            && !toca.contains(&n)
        {
            toca.push(n);
        }
        i += 2;
    }
    let dos = codigo.windows(11).any(|v| v == b"dos.library");
    if !toca.is_empty() {
        Arranque::Sospechoso { toca, bytes }
    } else if dos && hay_llamadas && solo_normales && bytes <= CORTO {
        Arranque::ComoAmigaDos
    } else {
        Arranque::Otro { bytes }
    }
}
