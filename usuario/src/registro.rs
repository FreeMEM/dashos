//! El registro común (S6, "El registro: uno solo y legible"; regla 2 de S36): lo que el sistema para, reinicia, niega o
//! deshace deja una línea de texto en `Estado:Registros/`, con la hora, el programa, de quién es, la clase y el motivo.
//!
//! Lo escribe solo `sesiones` (es quien sabe quién es cada programa, por la marca de su llave): un programa le manda qué
//! pasó y el motivo, y `sesiones` pone la hora, el nombre del programa y su usuario. Nadie elige en nombre de quién
//! escribe ni en qué fichero, y nadie borra ni reescribe: solo se añade al final, y al llegar a `TOPE` el fichero pasa
//! a `<nombre>.1` (el `.1` de antes se pierde). Así cada fichero ocupa como mucho dos veces `TOPE`.
//!
//! Tres ficheros, para que lo que manda un programa no se coma lo de seguridad: `seguridad` (lo que decide `sesiones`:
//! permisos dados, quitados y negados, acciones de administrador, entradas), `sistema` (servicios que se caen y vuelven,
//! memoria) y `programas` (lo que apunta cada programa, con un ritmo máximo por programa).
//!
//! Una línea:
//!
//! ```text
//! 2026-11-16T08:41:30Z editor Ana permiso no se da "red": el usuario dijo que no
//! ```
//!
//! hora (UTC, o `arranque+12.345s` si el sistema aún no sabe la hora), programa, usuario (`sistema` para lo que no es
//! de ninguna persona), clase, y el texto: qué pasó y, tras ": ", el motivo. Los cuatro primeros campos no llevan
//! espacios. Lo que llega de un programa se limpia: sin letras de control (ni secuencias de escape), UTF-8 bien formado
//! y recortado a `MAX_TEXTO`.
//!
//! Leer: cada usuario, lo suyo y lo del sistema (como la lista de procesos, U6); todo, solo un administrador con su
//! contraseña (acción de administrador "registro"). Lo filtra `sesiones` antes de dar nada.
//!
//! Sin `alloc` ni nada del sistema: se prueba en el PC con `rustc --edition 2024 --test usuario/src/registro.rs`.

/// El cajón de los registros, desde la raíz del disco (`Estado:Registros/`, S4b).
pub const CAJON: &str = "Estado/Registros";
/// Los ficheros, por su número en el protocolo (`REGISTRO_LEER` de `protocolo::sesiones`).
pub const FICHEROS: [&str; 3] = ["seguridad", "sistema", "programas"];
pub const SEGURIDAD: usize = 0;
pub const SISTEMA: usize = 1;
pub const PROGRAMAS: usize = 2;
/// Lo que crece un fichero antes de pasar a `.1`.
pub const TOPE: u64 = 64 * 1024;
/// Lo más que mide una línea entera, con su salto.
pub const MAX_LINEA: usize = 384;
/// Lo más que mide el texto que manda un programa (qué y motivo juntos).
pub const MAX_TEXTO: usize = 256;
/// Lo más que miden el nombre de un programa y el de un usuario en la línea.
pub const MAX_NOMBRE: usize = 16;
/// El usuario de lo que no es de ninguna persona.
pub const DEL_SISTEMA: &str = "sistema";

/// Las clases de línea, con lo que son (para la ayuda de la orden `Registro`).
pub const CLASES: &[(&str, &str)] = &[
    ("permiso", "permisos dados, quitados y negados"),
    ("admin", "acciones de administrador"),
    ("entrada", "entrar, volver y salir de una sesión"),
    ("procesos", "lanzar y cerrar programas"),
    ("apagar", "apagar, reiniciar y cerrar la sesión"),
    ("ajustes", "guardar ajustes de ENV: en ENVARC: negado"),
    ("guardia", "lo que la guardia de la caja Amiga niega"),
    ("memoria", "memoria baja y programas que topan con su tope"),
    ("servicio", "servicios que se caen y vuelven"),
    ("registro", "el propio registro: líneas que no se apuntaron"),
    ("programa", "lo que apunta un programa"),
];

/// Las clases que puede pedir un programa del sistema al apuntar (los demás apuntan siempre "programa").
pub const CLASES_DEL_SISTEMA: &[&str] = &["guardia", "memoria", "servicio", "programa"];

/// Si `c` es una clase conocida.
pub fn es_clase(c: &str) -> bool {
    CLASES.iter().any(|x| x.0 == c)
}

/// Días desde 1970-01-01 a (año, mes, día), en el calendario gregoriano (el algoritmo de "días civiles", de dominio
/// público).
pub fn civil(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Lo contrario de `civil`.
pub fn dias_de(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Escribe `n` en decimal con `ancho` cifras (con ceros delante) desde `sal[o]`; devuelve dónde acaba.
fn cifras(sal: &mut [u8], mut o: usize, n: u64, ancho: usize) -> usize {
    let mut b = [0u8; 20];
    let mut k = 0;
    let mut x = n;
    loop {
        b[k] = b'0' + (x % 10) as u8;
        k += 1;
        x /= 10;
        if x == 0 {
            break;
        }
    }
    for _ in k..ancho {
        if o < sal.len() {
            sal[o] = b'0';
            o += 1;
        }
    }
    while k > 0 {
        k -= 1;
        if o < sal.len() {
            sal[o] = b[k];
            o += 1;
        }
    }
    o
}

/// La hora de una línea: `2026-11-16T08:41:30Z` con la hora del calendario (`ns` desde 1970, UTC); si no se sabe
/// (`ns` 0), `arranque+12.345s` con lo que lleva la máquina en marcha. Devuelve cuánto escribió (como mucho 24).
pub fn hora(ns: u64, desde_arranque_us: u64, sal: &mut [u8; 24]) -> usize {
    if ns == 0 {
        let pre = b"arranque+";
        sal[..pre.len()].copy_from_slice(pre);
        let s = desde_arranque_us / 1_000_000;
        let mut o = cifras(sal, pre.len(), s.min(9_999_999_999), 1);
        if o + 5 <= sal.len() {
            sal[o] = b'.';
            o = cifras(sal, o + 1, (desde_arranque_us / 1000) % 1000, 3);
            sal[o] = b's';
            o += 1;
        }
        return o;
    }
    let s = ns / 1_000_000_000;
    let (y, m, d) = civil((s / 86_400) as i64);
    let r = s % 86_400;
    let mut o = cifras(sal, 0, y.clamp(0, 9999) as u64, 4);
    for (sep, n) in [(b'-', m as u64), (b'-', d as u64), (b'T', r / 3600), (b':', (r / 60) % 60), (b':', r % 60)] {
        sal[o] = sep;
        o = cifras(sal, o + 1, n, 2);
    }
    sal[o] = b'Z';
    o + 1
}

/// Los segundos desde 1970 de una hora de línea (`2026-11-16T08:41:30Z`); `None` si no es una hora así (la de antes de
/// saber la hora, por ejemplo).
pub fn segundos(h: &str) -> Option<u64> {
    let b = h.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let n = |a: usize, z: usize| -> Option<u64> {
        let t = &b[a..z];
        if !t.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(t.iter().fold(0u64, |x, &c| x * 10 + (c - b'0') as u64))
    };
    let (y, m, d, hh, mm, ss) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let dias = dias_de(y as i64, m as u32, d as u32);
    (dias >= 0).then(|| dias as u64 * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Un nombre (de programa o de usuario) para la línea: solo letras y cifras ASCII y `.`, `_`, `-`, `+` y `:`; lo demás,
/// `_`. Hasta el primer 0 y `MAX_NOMBRE` como mucho. Vacío, `?`. Devuelve cuánto escribió.
pub fn limpiar_nombre(n: &[u8], sal: &mut [u8; MAX_NOMBRE]) -> usize {
    let n = &n[..n.iter().position(|&c| c == 0).unwrap_or(n.len())];
    let mut o = 0;
    for &c in n.iter().take(MAX_NOMBRE) {
        sal[o] = if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'+' | b':') { c } else { b'_' };
        o += 1;
    }
    if o == 0 {
        sal[0] = b'?';
        o = 1;
    }
    o
}

/// Copia `t` a `sal` desde `o` como texto de una línea: UTF-8 bien formado (lo que no lo es, `?`), sin letras de control
/// (ni las de C1, ni el salto de línea, ni ESC: nada que mueva el cursor o cambie colores al leerlo en una terminal),
/// que pasan a `?`, y hasta `max` bytes (cortando entre letras, con `…` si no cupo). Devuelve dónde acaba.
pub fn limpiar_texto(t: &[u8], sal: &mut [u8], o: usize, max: usize) -> usize {
    let tope = (o + max).min(sal.len());
    // Lo que mide limpio: si cabe entero, va entero; si no, hasta donde quepa con "…" detrás.
    let entero = letras(t).map(|(c, _)| c.len_utf8()).sum::<usize>();
    let hasta = if o + entero <= tope { tope } else { tope.saturating_sub(3) };
    let mut o = o;
    let mut cortado = false;
    for (c, _) in letras(t) {
        let mut b = [0u8; 4];
        let s = c.encode_utf8(&mut b).as_bytes();
        if o + s.len() > hasta {
            cortado = true;
            break;
        }
        sal[o..o + s.len()].copy_from_slice(s);
        o += s.len();
    }
    if cortado && o + 3 <= tope {
        sal[o..o + 3].copy_from_slice("…".as_bytes());
        o += 3;
    }
    o
}

/// Las letras de `t` ya limpias (lo que no es UTF-8 y las de control, `?`), con lo que medían en `t`.
fn letras(t: &[u8]) -> impl Iterator<Item = (char, usize)> + '_ {
    let mut i = 0;
    core::iter::from_fn(move || {
        if i >= t.len() {
            return None;
        }
        let (c, largo) = decodificar(&t[i..]).unwrap_or(('?', 1));
        i += largo;
        Some((if c.is_control() || c == '\u{2028}' || c == '\u{2029}' { '?' } else { c }, largo))
    })
}

/// La primera letra de `b` y lo que mide, si es UTF-8 bien formado.
fn decodificar(b: &[u8]) -> Option<(char, usize)> {
    let largo = match b.first()? {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let s = core::str::from_utf8(b.get(..largo)?).ok()?;
    Some((s.chars().next()?, largo))
}

/// Una línea entera, con su salto, en `sal`: hora, programa, usuario, clase y el texto (`que` y, si lo hay, ": " y el
/// `motivo`). Todo se limpia aquí: `programa` y `usuario` como nombres, `que` y `motivo` como texto. Una clase que no
/// existe pasa a "programa". Devuelve lo que mide.
pub fn linea(hora: &str, programa: &[u8], usuario: &[u8], clase: &str, que: &[u8], motivo: &[u8], sal: &mut [u8; MAX_LINEA]) -> usize {
    let mut o = 0;
    fn poner(sal: &mut [u8; MAX_LINEA], o: &mut usize, x: &[u8]) {
        let n = x.len().min(MAX_LINEA - 1 - *o);
        sal[*o..*o + n].copy_from_slice(&x[..n]);
        *o += n;
    }
    let mut h = [0u8; 24];
    let hn = limpiar_hora(hora, &mut h);
    poner(sal, &mut o, &h[..hn]);
    poner(sal, &mut o, b" ");
    let mut n = [0u8; MAX_NOMBRE];
    let k = limpiar_nombre(programa, &mut n);
    poner(sal, &mut o, &n[..k]);
    poner(sal, &mut o, b" ");
    let k = limpiar_nombre(usuario, &mut n);
    poner(sal, &mut o, &n[..k]);
    poner(sal, &mut o, b" ");
    poner(sal, &mut o, if es_clase(clase) { clase } else { "programa" }.as_bytes());
    poner(sal, &mut o, b" ");
    // El texto: qué y el motivo, juntos hasta `MAX_TEXTO`. El motivo se queda al menos con un tercio si los dos son largos.
    let para_que = if motivo.is_empty() { MAX_TEXTO } else { (MAX_TEXTO - 2).saturating_sub(motivo.len().min(MAX_TEXTO / 3)) };
    let antes = o;
    o = limpiar_texto(que, &mut sal[..MAX_LINEA - 1], o, para_que);
    if o == antes {
        o = limpiar_texto(b"?", &mut sal[..MAX_LINEA - 1], o, 1);
    }
    if !motivo.is_empty() {
        let usado = o - antes;
        poner(sal, &mut o, b": ");
        o = limpiar_texto(motivo, &mut sal[..MAX_LINEA - 1], o, MAX_TEXTO.saturating_sub(usado + 2).max(1));
    }
    sal[o] = b'\n';
    o + 1
}

/// La hora tal cual si es una de las que da `hora` (sin espacios ni nada raro); si no, `?`.
fn limpiar_hora(h: &str, sal: &mut [u8; 24]) -> usize {
    let b = h.as_bytes();
    if b.is_empty() || b.len() > 24 || !b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':' | b'.' | b'+')) {
        sal[0] = b'?';
        return 1;
    }
    sal[..b.len()].copy_from_slice(b);
    b.len()
}

/// Una línea leída del registro, por sus campos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Linea<'a> {
    pub hora: &'a str,
    pub programa: &'a str,
    pub usuario: &'a str,
    pub clase: &'a str,
    pub texto: &'a str,
}

/// Parte una línea (sin su salto) en sus campos; `None` si no tiene los cinco.
pub fn leer(l: &str) -> Option<Linea<'_>> {
    let l = l.strip_suffix('\n').unwrap_or(l);
    let mut p = l.splitn(5, ' ');
    let (hora, programa, usuario, clase, texto) = (p.next()?, p.next()?, p.next()?, p.next()?, p.next()?);
    if [hora, programa, usuario, clase].iter().any(|c| c.is_empty()) {
        return None;
    }
    Some(Linea { hora, programa, usuario, clase, texto })
}

/// Si quien lee como `lector` (el nombre de su cuenta) ve la línea: la suya y la del sistema; con `todo` (un
/// administrador que lo confirmó), todas. Una línea que no se entiende no la ve nadie más que un administrador.
pub fn ve(lector: &str, todo: bool, l: &str) -> bool {
    if todo {
        return true;
    }
    match leer(l) {
        Some(x) => x.usuario == DEL_SISTEMA || (!lector.is_empty() && x.usuario == lector),
        None => false,
    }
}

/// Lo que se pide al leer: por programa, por clase y por fecha (desde y hasta, por el principio de la hora: "2026-11",
/// "2026-11-16", "2026-11-16T08"). Con fecha, las líneas de antes de saber la hora no salen.
#[derive(Debug, Default, Clone, Copy)]
pub struct Filtro<'a> {
    pub programa: Option<&'a str>,
    pub clase: Option<&'a str>,
    pub desde: Option<&'a str>,
    pub hasta: Option<&'a str>,
}

impl Filtro<'_> {
    /// Si la línea pasa. `hora` es la que se compara (la de la línea, o la local que enseña la orden).
    pub fn pasa(&self, l: &Linea, hora: &str) -> bool {
        if self.programa.is_some_and(|p| !p.eq_ignore_ascii_case(l.programa)) {
            return false;
        }
        if self.clase.is_some_and(|c| !c.eq_ignore_ascii_case(l.clase)) {
            return false;
        }
        let con_fecha = hora.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if let Some(d) = self.desde {
            if !con_fecha || hora < d {
                return false;
            }
        }
        if let Some(h) = self.hasta {
            if !con_fecha || &hora[..hora.len().min(h.len())] > h {
                return false;
            }
        }
        true
    }
}

/// Si al añadir `largo` bytes a un fichero de `tam` se pasa de `TOPE` y toca empezar otro (el viejo, a `.1`). Uno vacío
/// no rota nunca, por larga que sea la línea.
pub fn rota(tam: u64, largo: usize) -> bool {
    tam > 0 && tam.saturating_add(largo as u64) > TOPE
}

/// El ritmo de lo que apunta cada programa: una ráfaga de `RAFAGA` líneas y luego una cada `CADA_US`. Lo que pasa de
/// ahí no se apunta, pero se cuenta, y la siguiente que entra dice cuántas se perdieron. Uno por clave (`sesiones` usa
/// el nombre del programa y su usuario: lanzar copias no da más), hasta `N`; el que lleva más tiempo sin apuntar deja
/// su sitio.
pub const RAFAGA: u32 = 20;
pub const CADA_US: u64 = 500_000;

pub struct Ritmo<const N: usize> {
    /// Marca, fichas que le quedan, cuándo se le repuso la última y cuántas se perdieron.
    e: [(u64, u32, u64, u32); N],
}

impl<const N: usize> Default for Ritmo<N> {
    fn default() -> Self {
        Self { e: [(0, 0, 0, 0); N] }
    }
}

impl<const N: usize> Ritmo<N> {
    /// Si `marca` puede apuntar ahora (`ahora_us`). Sí: cuántas se le perdieron antes (para decirlo). No: `None`.
    pub fn dejar(&mut self, marca: u64, ahora_us: u64) -> Option<u32> {
        let i = match self.e.iter().position(|x| x.0 == marca && marca != 0) {
            Some(i) => i,
            None => {
                let i = (0..N).min_by_key(|&i| if self.e[i].0 == 0 { 0 } else { self.e[i].2.max(1) }).unwrap_or(0);
                self.e[i] = (marca, RAFAGA, ahora_us, 0);
                i
            }
        };
        let x = &mut self.e[i];
        let nuevas = (ahora_us.saturating_sub(x.2) / CADA_US).min(RAFAGA as u64) as u32;
        if nuevas > 0 {
            x.1 = (x.1 + nuevas).min(RAFAGA);
            x.2 = ahora_us;
        }
        if x.1 == 0 {
            x.3 = x.3.saturating_add(1);
            return None;
        }
        x.1 -= 1;
        Some(core::mem::take(&mut x.3))
    }

    /// Olvida a `marca` (se fue).
    pub fn olvidar(&mut self, marca: u64) {
        for x in self.e.iter_mut().filter(|x| x.0 == marca) {
            *x = (0, 0, 0, 0);
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn l(hora_: &str, prog: &[u8], usu: &[u8], clase: &str, que: &[u8], motivo: &[u8]) -> String {
        let mut b = [0u8; MAX_LINEA];
        let n = linea(hora_, prog, usu, clase, que, motivo, &mut b);
        assert!(n <= MAX_LINEA);
        assert_eq!(b[n - 1], b'\n');
        String::from_utf8(b[..n].to_vec()).expect("siempre UTF-8")
    }

    #[test]
    fn horas() {
        let mut h = [0u8; 24];
        // 2026-11-16 08:41:30 UTC.
        let s = dias_de(2026, 11, 16) as u64 * 86_400 + 8 * 3600 + 41 * 60 + 30;
        let n = hora(s * 1_000_000_000 + 999, 0, &mut h);
        let t = core::str::from_utf8(&h[..n]).unwrap();
        assert_eq!(t, "2026-11-16T08:41:30Z");
        assert_eq!(segundos(t), Some(s));
        let n = hora(0, 12_345_678, &mut h);
        assert_eq!(core::str::from_utf8(&h[..n]).unwrap(), "arranque+12.345s");
        assert_eq!(segundos("arranque+12.345s"), None);
        let n = hora(0, u64::MAX, &mut h);
        assert!(n <= 24);
        // Fechas al borde: 1970, 29 de febrero, 2100 (no bisiesto), 9999.
        for (y, m, d) in [(1970, 1, 1), (2024, 2, 29), (2100, 3, 1), (2000, 2, 29), (9999, 12, 31)] {
            assert_eq!(civil(dias_de(y, m, d)), (y, m, d));
        }
        let n = hora(u64::MAX, 0, &mut h);
        assert!(segundos(core::str::from_utf8(&h[..n]).unwrap()).is_some());
        for malo in ["", "2026-13-01T00:00:00Z", "2026-11-16 08:41:30Z", "2026-11-16T08:41:30", "2026-11-16T25:00:00Z", "abcd-ef-ghTij:kl:mnZ"] {
            assert_eq!(segundos(malo), None, "{malo}");
        }
    }

    #[test]
    fn una_linea() {
        let t = l("2026-11-16T08:41:30Z", b"editor", b"Ana", "permiso", "no se da \"red\"".as_bytes(), b"el usuario dijo que no");
        assert_eq!(t, "2026-11-16T08:41:30Z editor Ana permiso no se da \"red\": el usuario dijo que no\n");
        let x = leer(&t).unwrap();
        assert_eq!((x.hora, x.programa, x.usuario, x.clase), ("2026-11-16T08:41:30Z", "editor", "Ana", "permiso"));
        assert_eq!(x.texto, "no se da \"red\": el usuario dijo que no");
        // Sin motivo, sin ": ".
        assert_eq!(l("h", b"a", b"sistema", "servicio", b"vuelve", b""), "h a sistema servicio vuelve\n");
    }

    #[test]
    fn lo_hostil_se_limpia() {
        // Secuencias de escape, saltos de línea, retornos, C1, bytes que no son UTF-8 y un 0 en medio.
        let t = l("h", b"ma\x1b[31mlo", b"An a\n", "programa", b"\x1b[2J\x1b]0;titulo\x07hola\r\nfalsa linea\x9b\xc2\x9b\xff\xfe\0fin", b"\n\n");
        assert_eq!(t.matches('\n').count(), 1, "{t:?}");
        assert!(!t.contains('\x1b') && !t.contains('\r') && !t.contains('\u{9b}') && !t.contains('\0'), "{t:?}");
        let x = leer(&t).unwrap();
        assert_eq!(x.programa, "ma__31mlo");
        assert_eq!(x.usuario, "An_a_");
        // Un programa no puede hacerse pasar por otro campo: los espacios del nombre pasan a "_".
        let t = l("h", b"falso sistema seguridad", b"Ana", "permiso", b"x", b"");
        assert_eq!(leer(&t).unwrap().programa, "falso_sistema_se");
        // Una clase inventada pasa a "programa"; una hora rara, a "?".
        let t = l("2026 mentira", b"p", b"u", "inventada", b"x", b"");
        assert!(t.starts_with("? p u programa x"), "{t:?}");
        // Vacíos.
        assert_eq!(l("h", b"", b"", "programa", b"", b""), "h ? ? programa ?\n");
    }

    #[test]
    fn lineas_enormes() {
        let enorme = "ñ".repeat(10_000);
        let t = l("2026-11-16T08:41:30Z", &[b'x'; 5000], &[b'y'; 5000], "programa", enorme.as_bytes(), enorme.as_bytes());
        assert!(t.len() <= MAX_LINEA, "{}", t.len());
        let x = leer(&t).unwrap();
        assert_eq!(x.programa.len(), MAX_NOMBRE);
        assert!(x.texto.contains('…'), "se dice que se cortó");
        assert!(x.texto.len() <= MAX_TEXTO + 2, "{}", x.texto.len());
        // El motivo no desaparece aunque el qué sea enorme.
        let t = l("h", b"p", b"u", "programa", &[b'a'; 4000], b"el motivo");
        assert!(t.contains(": el motivo"), "{t:?}");
        // Bytes sueltos de UTF-8 cortados al final.
        let t = l("h", b"p", b"u", "programa", b"abc\xe2\x82", b"");
        assert_eq!(leer(&t).unwrap().texto, "abc??");
        // Cualquier entrada da una línea que se lee.
        let mut x = 0x1234_5678u32;
        for _ in 0..2000 {
            let mut b = [0u8; 600];
            for c in b.iter_mut() {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *c = x as u8;
            }
            let n = (x as usize) % 600;
            let t = l("h", &b[..n % 40], &b[..n % 20], "programa", &b[..n], &b[n / 2..n]);
            assert!(leer(&t).is_some() && t.len() <= MAX_LINEA && t.matches('\n').count() == 1, "{t:?}");
        }
    }

    #[test]
    fn quien_ve_que() {
        let ana = "2026-11-16T08:41:30Z editor Ana permiso no\n";
        let sis = "2026-11-16T08:41:30Z red sistema servicio vuelve\n";
        let carla = "2026-11-16T08:41:30Z editor Carla permiso no\n";
        assert!(ve("Ana", false, ana) && ve("Ana", false, sis) && !ve("Ana", false, carla));
        assert!(ve("Carla", false, carla) && !ve("Carla", false, ana));
        assert!(ve("Ana", true, carla));
        // Sin cuenta (un programa del sistema que lee), solo lo del sistema; y "sistema" no es una cuenta.
        assert!(!ve("", false, ana) && ve("", false, sis));
        // Una línea rota no la ve nadie sin `todo`.
        assert!(!ve("Ana", false, "basura") && !ve("Ana", false, "") && ve("Ana", true, "basura"));
        // Un nombre de usuario dentro del texto no cuenta.
        assert!(!ve("Ana", false, "h p Carla programa Ana Ana Ana\n"));
    }

    #[test]
    fn filtros() {
        let x = leer("2026-11-16T08:41:30Z editor Ana permiso no").unwrap();
        let f = |p: Option<&'static str>, c: Option<&'static str>, d: Option<&'static str>, h: Option<&'static str>| Filtro { programa: p, clase: c, desde: d, hasta: h };
        assert!(f(None, None, None, None).pasa(&x, x.hora));
        assert!(f(Some("EDITOR"), Some("permiso"), None, None).pasa(&x, x.hora));
        assert!(!f(Some("otro"), None, None, None).pasa(&x, x.hora));
        assert!(!f(None, Some("memoria"), None, None).pasa(&x, x.hora));
        assert!(f(None, None, Some("2026-11-16"), Some("2026-11-16")).pasa(&x, x.hora));
        assert!(f(None, None, Some("2026-11"), Some("2026")).pasa(&x, x.hora));
        assert!(!f(None, None, Some("2026-11-17"), None).pasa(&x, x.hora));
        assert!(!f(None, None, None, Some("2026-11-15")).pasa(&x, x.hora));
        let y = leer("arranque+1.000s p sistema servicio x").unwrap();
        assert!(!f(None, None, Some("2026"), None).pasa(&y, y.hora));
        assert!(f(None, None, None, None).pasa(&y, y.hora));
    }

    #[test]
    fn rotar() {
        assert!(!rota(0, 10_000_000));
        assert!(!rota(TOPE - 10, 10));
        assert!(rota(TOPE - 10, 11));
        assert!(rota(u64::MAX - 1, 10));
    }

    #[test]
    fn ritmo() {
        let mut r = Ritmo::<2>::default();
        let mut si = 0;
        for _ in 0..100 {
            si += r.dejar(7, 1_000).is_some() as u32;
        }
        assert_eq!(si, RAFAGA);
        // Pasado un rato, vuelve a dejar y dice cuántas se perdieron.
        assert_eq!(r.dejar(7, 1_000 + CADA_US), Some(100 - RAFAGA));
        // Otro programa no se ve frenado por el primero; un tercero echa al que lleva más tiempo sin apuntar.
        assert!(r.dejar(8, 2_000).is_some());
        assert!(r.dejar(9, 3_000).is_some());
        assert_eq!(r.dejar(9, 3_000), Some(0));
        r.olvidar(9);
        assert!(r.dejar(9, 3_000).is_some());
        // El reloj que va hacia atrás no da fichas de más.
        let mut r = Ritmo::<1>::default();
        for _ in 0..RAFAGA {
            assert!(r.dejar(1, 10_000_000).is_some());
        }
        assert!(r.dejar(1, 0).is_none());
    }
}
