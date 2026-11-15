//! El descriptor de informe de HID (HID 1.11, 6.2.2), lo justo para una tableta: un puntero absoluto, el de las
//! máquinas virtuales (la `usb-tablet` de QEMU), las pantallas táctiles en modo ratón y algunas tabletas gráficas. Con
//! él, el ratón de una máquina virtual va a donde está el del PC, sin capturarlo ni escalas que no cuadran.
//!
//! `tableta` recorre los elementos del descriptor y busca, en los informes de entrada, la X y la Y absolutas de la
//! página de escritorio (0x01, usos 0x30 y 0x31), los botones (página 0x09) y la rueda (0x38, relativa). Devuelve dónde
//! está cada cosa en el informe, en bits. `leer` saca de un informe la posición (0 a `ESCALA`), los botones y la rueda.
//!
//! El descriptor lo da el aparato: no se cree nada. Largos y cuentas con tope, nada fuera del informe, y si algo no
//! cuadra, `None` (el aparato se queda sin usar, como hasta ahora).
//!
//! Sin dependencias: se prueba en el PC con `rustc --test`.

/// La posición que da `leer`: de 0 a `ESCALA`, en cada eje (la del protocolo del compositor).
pub const ESCALA: u32 = 32767;

/// Lo más largo que se mira de un descriptor.
pub const MAX_DESCRIPTOR: usize = 1024;

/// Lo más largo, en bytes, que puede ser un informe que se entienda (los del xHCI se leen hasta 16).
pub const MAX_INFORME: u32 = 16;

/// Un campo del informe: dónde empieza (en bits, tras el número de informe si lo hay), cuántos bits y sus límites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Campo {
    pub bit: u32,
    pub bits: u32,
    pub min: i32,
    pub max: i32,
}

/// Dónde están las cosas de una tableta en su informe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tableta {
    /// El número de informe (el primer byte), si el aparato los usa.
    pub id: Option<u8>,
    pub x: Campo,
    pub y: Campo,
    /// Los botones: el primero y cuántos (hasta 8).
    pub botones: Option<(u32, u32)>,
    pub rueda: Option<Campo>,
}

/// Lo que se saca de un informe: posición (0 a `ESCALA`), botones (bit 0 izquierdo, 1 derecho, 2 central) y rueda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lectura {
    pub x: u32,
    pub y: u32,
    pub botones: u8,
    pub rueda: i32,
}

/// Lo de un informe de entrada mientras se recorre: su número y cuántos bits lleva.
struct Informe {
    id: u8,
    bits: u32,
}

/// Busca una tableta en el descriptor. `None` si no hay X e Y absolutas en el mismo informe, o si el descriptor
/// miente (largos, cuentas o campos que no caben en `MAX_INFORME`).
pub fn tableta(d: &[u8]) -> Option<Tableta> {
    let d = &d[..d.len().min(MAX_DESCRIPTOR)];
    // Lo global (página, mínimo, máximo, tamaño, cuenta, número) y lo local (usos) de HID.
    let (mut pagina, mut min, mut max, mut tam, mut cuenta, mut id) = (0u32, 0i32, 0i32, 0u32, 0u32, 0u8);
    let mut usos: [u32; 16] = [0; 16];
    let (mut n_usos, mut uso_min, mut uso_max) = (0usize, None::<u32>, None::<u32>);
    let mut informes: [Option<Informe>; 8] = [const { None }; 8];
    // Lo hallado, por informe: (id, campo).
    let (mut x, mut y) = (None::<(u8, Campo)>, None::<(u8, Campo)>);
    // Los botones y la rueda de cada informe (por su sitio en `informes`): se queda el del informe de la X.
    let mut botones: [Option<(u32, u32)>; 8] = [None; 8];
    let mut rueda: [Option<Campo>; 8] = [None; 8];
    let mut i = 0;
    while i < d.len() {
        let b = d[i];
        if b == 0xfe {
            // Elemento largo: su largo en el byte siguiente; no se usan.
            let largo = *d.get(i + 1)? as usize;
            i += 3 + largo;
            continue;
        }
        let n = [0, 1, 2, 4][(b & 3) as usize];
        let datos = d.get(i + 1..i + 1 + n)?;
        let sin = datos.iter().rev().fold(0u32, |a, &v| (a << 8) | v as u32);
        // Con signo (mínimo y máximo lógicos), según su tamaño.
        let con = match n {
            1 => sin as u8 as i8 as i32,
            2 => sin as u16 as i16 as i32,
            4 => sin as i32,
            _ => 0,
        };
        let (tipo, etiqueta) = ((b >> 2) & 3, b >> 4);
        match (tipo, etiqueta) {
            // Global.
            (1, 0) => pagina = sin,
            (1, 1) => min = con,
            (1, 2) => max = con,
            (1, 7) => tam = sin,
            (1, 8) => id = sin as u8,
            (1, 9) => cuenta = sin,
            // Local: usos (con la página en los 16 bits altos si vienen de 4 bytes).
            (2, 0) => {
                let u = if n == 4 { sin } else { (pagina << 16) | sin };
                if n_usos < usos.len() {
                    usos[n_usos] = u;
                    n_usos += 1;
                }
            }
            (2, 1) => uso_min = Some(if n == 4 { sin } else { (pagina << 16) | sin }),
            (2, 2) => uso_max = Some(if n == 4 { sin } else { (pagina << 16) | sin }),
            // Main: entrada (8). Salida, rasgo y colección no cuentan; los locales se olvidan tras cada main.
            (0, e) => {
                if e == 8 {
                    // Un campo enorme no se entiende; uno sin bits no ocupa nada (y no se recorre: su cuenta puede mentir).
                    if tam > 32 || cuenta > 64 {
                        return None;
                    }
                    let cuenta = if tam == 0 { 0 } else { cuenta };
                    let k = informes.iter().position(|r| r.as_ref().is_some_and(|r| r.id == id)).or_else(|| {
                        let k = informes.iter().position(Option::is_none)?;
                        informes[k] = Some(Informe { id, bits: 0 });
                        Some(k)
                    })?;
                    let inf = informes[k].as_mut()?;
                    let constante = sin & 1 != 0;
                    let relativo = sin & 4 != 0;
                    for f in 0..cuenta {
                        let bit = inf.bits + f * tam;
                        let uso = if n_usos > 0 {
                            Some(usos[(f as usize).min(n_usos - 1)])
                        } else {
                            uso_min.map(|m| m.saturating_add(f)).filter(|&u| uso_max.is_none_or(|mx| u <= mx))
                        };
                        if constante {
                            continue;
                        }
                        let campo = Campo { bit, bits: tam, min, max };
                        match uso {
                            Some(0x0001_0030) if !relativo && x.is_none() => x = Some((id, campo)),
                            Some(0x0001_0031) if !relativo && y.is_none() => y = Some((id, campo)),
                            Some(0x0001_0038) if relativo && rueda[k].is_none() => rueda[k] = Some(campo),
                            Some(u) if u >> 16 == 9 && tam == 1 => {
                                let b = botones[k].get_or_insert((bit, 0));
                                if b.0 + b.1 == bit && b.1 < 8 {
                                    b.1 += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    inf.bits = inf.bits.checked_add(tam.checked_mul(cuenta)?)?;
                    if inf.bits > MAX_INFORME * 8 {
                        return None;
                    }
                }
                n_usos = 0;
                uso_min = None;
                uso_max = None;
            }
            _ => {}
        }
        i += 1 + n;
    }
    let ((xi, x), (yi, y)) = (x?, y?);
    if xi != yi || x.max <= x.min || y.max <= y.min {
        return None;
    }
    let k = informes.iter().position(|r| r.as_ref().is_some_and(|r| r.id == xi))?;
    // Con número de informe, todo lo de la tableta es de ese informe; los botones y la rueda de otro, no.
    let usa_id = informes.iter().flatten().any(|r| r.id != 0);
    Some(Tableta {
        id: usa_id.then_some(xi),
        x,
        y,
        botones: botones[k],
        rueda: rueda[k],
    })
}

/// Los `bits` desde el bit `desde` de `d` (de menos a más significativo, como HID). `None` si no caben.
fn bits(d: &[u8], desde: u32, n: u32) -> Option<u32> {
    if n == 0 || n > 32 || (desde as u64 + n as u64) > d.len() as u64 * 8 {
        return None;
    }
    let mut v = 0u64;
    for k in 0..n {
        let b = desde + k;
        if d[(b / 8) as usize] >> (b % 8) & 1 != 0 {
            v |= 1 << k;
        }
    }
    Some(v as u32)
}

/// Con signo si el mínimo es negativo (extiende el bit más alto).
fn valor(d: &[u8], c: &Campo) -> Option<i32> {
    let v = bits(d, c.bit, c.bits)?;
    Some(if c.min < 0 && c.bits < 32 && v >> (c.bits - 1) & 1 != 0 { (v | (u32::MAX << c.bits)) as i32 } else { v as i32 })
}

/// Lo que dice un informe de la tableta `t`. `None` si es de otro informe o no trae la posición.
pub fn leer(t: &Tableta, informe: &[u8]) -> Option<Lectura> {
    let d = match t.id {
        Some(id) => {
            if *informe.first()? != id {
                return None;
            }
            &informe[1..]
        }
        None => informe,
    };
    let escalar = |c: &Campo| -> Option<u32> {
        let v = valor(d, c)?.clamp(c.min, c.max) as i64;
        Some(((v - c.min as i64) * ESCALA as i64 / (c.max as i64 - c.min as i64)) as u32)
    };
    let botones = t.botones.and_then(|(desde, n)| bits(d, desde, n.min(8))).unwrap_or(0) as u8;
    let rueda = t.rueda.as_ref().and_then(|c| valor(d, c)).unwrap_or(0).clamp(-127, 127);
    Some(Lectura { x: escalar(&t.x)?, y: escalar(&t.y)?, botones, rueda })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    /// El descriptor de la `usb-tablet` de QEMU (escrito a partir de la norma: 3 botones, relleno, X e Y de 16 bits de 0
    /// a 0x7fff, rueda de 8 bits relativa), sin número de informe.
    const QEMU: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, // escritorio, ratón, colección, puntero
        0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, // 3 botones
        0x95, 0x01, 0x75, 0x05, 0x81, 0x01, // relleno de 5
        0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x26, 0xff, 0x7f, 0x35, 0x00, 0x46, 0xff, 0x7f, 0x75, 0x10, 0x95, 0x02,
        0x81, 0x02, // X e Y absolutas
        0x05, 0x01, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x35, 0x00, 0x45, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06, // rueda
        0xc0, 0xc0,
    ];

    #[test]
    fn la_de_qemu() {
        let t = tableta(QEMU).expect("es una tableta");
        assert_eq!(t.id, None);
        assert_eq!(t.botones, Some((0, 3)));
        assert_eq!(t.x, Campo { bit: 8, bits: 16, min: 0, max: 0x7fff });
        assert_eq!(t.y.bit, 24);
        assert_eq!(t.rueda.map(|r| r.bit), Some(40));
        // Botón izquierdo, en el centro, rueda hacia abajo.
        let l = leer(&t, &[1, 0xff, 0x3f, 0xff, 0x3f, 0xff]).unwrap();
        assert_eq!((l.botones, l.x, l.y, l.rueda), (1, 0x3fff, 0x3fff, -1));
        // Las esquinas.
        assert_eq!(leer(&t, &[0, 0, 0, 0xff, 0x7f, 0]).map(|l| (l.x, l.y)), Some((0, ESCALA)));
        // Un informe corto: sin posición.
        assert_eq!(leer(&t, &[1, 2, 3]), None);
    }

    #[test]
    fn un_raton_no_es_tableta() {
        // Ratón relativo: X e Y con la marca de relativo.
        let raton = [0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06];
        assert_eq!(tableta(&raton), None);
        // Un teclado.
        let teclado = [0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02];
        assert_eq!(tableta(&teclado), None);
    }

    #[test]
    fn con_numero_de_informe() {
        // Informe 2, con X e Y de 12 bits de 0 a 4095 y un botón; el informe 1 (otro aparato del mismo) se ignora.
        let d = [
            0x85, 0x01, 0x05, 0x09, 0x09, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, // informe 1: botones de otro
            0x85, 0x02, 0x05, 0x09, 0x09, 0x01, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, 0x75, 0x07, 0x81, 0x01, 0x05, 0x01,
            0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x26, 0xff, 0x0f, 0x75, 0x0c, 0x95, 0x02, 0x81, 0x02,
        ];
        let t = tableta(&d).unwrap();
        assert_eq!(t.id, Some(2));
        assert_eq!(t.botones, Some((0, 1)));
        assert_eq!(t.x.bit, 8);
        // X = 4095 (abajo del todo), Y = 0: 12 + 12 bits tras el byte de botones.
        let l = leer(&t, &[2, 1, 0xff, 0x0f, 0x00]).unwrap();
        assert_eq!((l.x, l.y, l.botones), (ESCALA, 0, 1));
        assert_eq!(leer(&t, &[1, 1, 0xff, 0x0f, 0x00]), None, "otro informe");
    }

    /// Hostil: descriptores que mienten (largos de más, tamaños enormes, cuentas sin fin, máximo menor que el mínimo,
    /// elementos largos cortados) y basura de todos los largos: ni un pánico, y nada se lee fuera del informe.
    #[test]
    fn descriptores_hostiles() {
        for n in 0..QEMU.len() {
            let _ = tableta(&QEMU[..n]);
        }
        // Campo enorme: 32 bits × 64.
        assert_eq!(tableta(&[0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x26, 0xff, 0x7f, 0x75, 0x20, 0x95, 0x40, 0x81, 0x02]), None);
        // Máximo menor que el mínimo.
        assert_eq!(tableta(&[0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x10, 0x25, 0x05, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02]), None);
        // Elemento largo que dice medir más de lo que queda.
        let _ = tableta(&[0xfe, 0xff, 0x00]);
        // Un informe que dice tener la X en un sitio que no llega: leer no se sale.
        let mut t = tableta(QEMU).unwrap();
        t.x.bit = 1000;
        assert_eq!(leer(&t, &[0; 6]), None);
        t.x = Campo { bit: 0, bits: 33, min: 0, max: 1 };
        assert_eq!(leer(&t, &[0; 6]), None);
        // Basura.
        let mut s = 0x1234_5678u32;
        let mut buf = [0u8; 300];
        for _ in 0..20_000 {
            let n = (s % 300) as usize;
            for b in buf.iter_mut().take(n) {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                *b = s as u8;
            }
            if let Some(t) = tableta(&buf[..n]) {
                let _ = leer(&t, &buf[..(s % 20) as usize]);
            }
        }
    }
}
