//! DNS (S13): hacer una pregunta A o AAAA y leer la respuesta, sin fiarse de ella. Lo usa el resolvedor del servicio de
//! red (`usuario/src/bin/red.rs`). Escrito con los RFC 1034, 1035, 2181 (TTL), 6891 (EDNS0) y 5452 (respuestas falsas).
//!
//! Lo que una respuesta tiene que cumplir para valer: el mismo identificador, ser respuesta, traer la misma pregunta
//! (nombre, tipo y clase), no venir truncada y decir algo del nombre preguntado o del final de su cadena de alias
//! (CNAME). Lo que hable de otros nombres no se usa. Los nombres comprimidos solo pueden apuntar hacia atrás y con un
//! tope de saltos: una respuesta con un bucle se da por mala, no se sigue.
//!
//! De dónde viene (dirección y puerto del servidor) lo mira el servicio, que es quien lo sabe.
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/dns.rs`.

pub const A: u16 = 1;
pub const AAAA: u16 = 28;
const CNAME: u16 = 5;
const OPT: u16 = 41;
const IN: u16 = 1;
/// Un nombre en texto, sin el punto final (RFC 1035: 255 bytes en el cable).
pub const MAX_NOMBRE: usize = 253;
pub const MAX_ETIQUETA: usize = 63;
/// Direcciones que se guardan de una respuesta; las demás se tiran.
pub const MAX_DIRS: usize = 8;
/// Lo más que mide una pregunta: cabecera, nombre, tipo y clase, y el registro OPT.
pub const MAX_PREGUNTA: usize = 12 + 255 + 4 + 11;
/// Lo que decimos que cabe en una respuesta por UDP (EDNS0; 1232 no se fragmenta ni en IPv6).
pub const MAX_RESPUESTA: usize = 1232;
/// Registros que se miran de una respuesta, alias seguidos y saltos al descomprimir un nombre.
const MAX_REGISTROS: usize = 64;
const MAX_ALIAS: usize = 8;
const MAX_SALTOS: usize = 32;
/// El TTL más largo que se cree (un día).
pub const MAX_TTL: u32 = 86_400;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    /// No llega ni a la cabecera, o un registro se sale del mensaje.
    Corta,
    OtroId,
    NoEsRespuesta,
    /// No trae la pregunta que se hizo.
    OtraPregunta,
    /// Vino cortada (bit TC): no vale; hay que preguntar por TCP.
    Truncada,
    /// El nombre no existe (NXDOMAIN).
    NoExiste,
    /// El servidor dice que no puede (su código).
    Servidor(u8),
    /// Un nombre con un bucle o hacia delante, demasiados alias, medidas imposibles.
    Mala,
    /// El nombre existe, pero no tiene direcciones de ese tipo.
    SinDatos,
}

/// Deja en `sal` el nombre en minúsculas y sin el punto final, si vale: de 1 a 253 bytes, etiquetas de 1 a 63, solo
/// letras, cifras, guion y guion bajo (sin IDN: un nombre con otras letras hay que pasarlo antes a punycode). Devuelve
/// su largo.
pub fn normalizar(nombre: &[u8], sal: &mut [u8; MAX_NOMBRE]) -> Option<usize> {
    let nombre = nombre.strip_suffix(b".").unwrap_or(nombre);
    if nombre.is_empty() || nombre.len() > MAX_NOMBRE {
        return None;
    }
    let mut etiqueta = 0;
    for (i, &b) in nombre.iter().enumerate() {
        if b == b'.' {
            if etiqueta == 0 {
                return None;
            }
            etiqueta = 0;
        } else {
            if !(b.is_ascii_alphanumeric() || b == b'-' || b == b'_') || etiqueta == MAX_ETIQUETA {
                return None;
            }
            etiqueta += 1;
        }
        sal[i] = b.to_ascii_lowercase();
    }
    (etiqueta > 0).then_some(nombre.len())
}

/// Escribe en `sal` la pregunta por `nombre` (ya normalizado) y devuelve su largo. Pide recursión y dice, con EDNS0,
/// que caben respuestas de `MAX_RESPUESTA` bytes.
pub fn pregunta(id: u16, nombre: &[u8], tipo: u16, sal: &mut [u8; MAX_PREGUNTA]) -> usize {
    sal[..12].copy_from_slice(&[(id >> 8) as u8, id as u8, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 1]);
    let mut n = 12;
    for etiqueta in nombre.split(|&b| b == b'.') {
        sal[n] = etiqueta.len() as u8;
        sal[n + 1..n + 1 + etiqueta.len()].copy_from_slice(etiqueta);
        n += 1 + etiqueta.len();
    }
    sal[n] = 0;
    sal[n + 1..n + 5].copy_from_slice(&[(tipo >> 8) as u8, tipo as u8, 0, IN as u8]);
    n += 5;
    // OPT: nombre raíz, tipo 41, la clase es el tamaño de UDP, sin más opciones.
    sal[n..n + 11].copy_from_slice(&[0, 0, OPT as u8, (MAX_RESPUESTA >> 8) as u8, MAX_RESPUESTA as u8, 0, 0, 0, 0, 0, 0]);
    n + 11
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Respuesta {
    /// Las direcciones: una IPv4 va en los 4 primeros bytes.
    pub dirs: [[u8; 16]; MAX_DIRS],
    pub n: usize,
    /// Lo que se puede guardar, en segundos: el menor de la cadena, con el tope de `MAX_TTL`.
    pub ttl: u32,
}

fn u16_en(m: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*m.get(i)?, *m.get(i + 1)?]))
}

/// Lee el nombre que empieza en `pos`, lo deja en `sal` (minúsculas, con puntos) y devuelve su largo y dónde sigue el
/// mensaje tras él. `None` si se sale, pasa de 253, apunta hacia delante o a sí mismo, o da demasiados saltos.
fn leer_nombre(m: &[u8], pos: usize, sal: &mut [u8; MAX_NOMBRE]) -> Option<(usize, usize)> {
    let (mut p, mut n, mut saltos) = (pos, 0usize, 0usize);
    let mut sigue = None;
    // Cada puntero tiene que ir a antes de donde empezó lo que se está leyendo: así no hay bucles.
    let mut tope = pos;
    loop {
        let largo = *m.get(p)? as usize;
        match largo & 0xc0 {
            0 if largo == 0 => return Some((n, sigue.unwrap_or(p + 1))),
            0 => {
                let etiqueta = m.get(p + 1..p + 1 + largo)?;
                let punto = if n > 0 { 1 } else { 0 };
                if n + punto + largo > MAX_NOMBRE {
                    return None;
                }
                if punto == 1 {
                    sal[n] = b'.';
                }
                for (i, &b) in etiqueta.iter().enumerate() {
                    // Un punto dentro de una etiqueta haría pasar "a.b" por dos etiquetas.
                    if b == b'.' {
                        return None;
                    }
                    sal[n + punto + i] = b.to_ascii_lowercase();
                }
                n += punto + largo;
                p += 1 + largo;
            }
            0xc0 => {
                let destino = ((largo & 0x3f) << 8) | *m.get(p + 1)? as usize;
                saltos += 1;
                if destino >= tope || saltos > MAX_SALTOS {
                    return None;
                }
                sigue.get_or_insert(p + 2);
                tope = destino;
                p = destino;
            }
            _ => return None,
        }
    }
}

/// Un registro de la sección de respuestas: dónde está su nombre, tipo, clase, TTL y sus datos.
struct Registro {
    nombre: usize,
    tipo: u16,
    clase: u16,
    ttl: u32,
    datos: usize,
    largo: usize,
}

/// Lee `m`, la respuesta a la pregunta `id` por `nombre` (normalizado) y `tipo`.
pub fn respuesta(m: &[u8], id: u16, nombre: &[u8], tipo: u16) -> Result<Respuesta, Error> {
    if m.len() < 12 {
        return Err(Error::Corta);
    }
    if u16_en(m, 0) != Some(id) {
        return Err(Error::OtroId);
    }
    let banderas = u16_en(m, 2).unwrap_or(0);
    // Respuesta (QR) a una consulta normal (código de operación 0).
    if banderas & 0x8000 == 0 || banderas & 0x7800 != 0 {
        return Err(Error::NoEsRespuesta);
    }
    let (preguntas, respuestas) = (u16_en(m, 4).unwrap_or(0), u16_en(m, 6).unwrap_or(0) as usize);
    if preguntas != 1 {
        return Err(Error::OtraPregunta);
    }
    let mut leido = [0u8; MAX_NOMBRE];
    let (n, mut p) = leer_nombre(m, 12, &mut leido).ok_or(Error::Mala)?;
    if &leido[..n] != nombre || u16_en(m, p) != Some(tipo) || u16_en(m, p + 2) != Some(IN) {
        return Err(Error::OtraPregunta);
    }
    p += 4;
    if banderas & 0x0200 != 0 {
        return Err(Error::Truncada);
    }
    match (banderas & 0xf) as u8 {
        0 => {}
        3 => return Err(Error::NoExiste),
        otro => return Err(Error::Servidor(otro)),
    }
    // Los registros de la respuesta, sin fiarse de la cuenta: los que quepan en el mensaje, con tope.
    let mut registros: [Option<Registro>; MAX_REGISTROS] = [const { None }; MAX_REGISTROS];
    let mut cuantos = 0;
    for hueco in registros.iter_mut().take(respuestas) {
        let (_, tras) = leer_nombre(m, p, &mut leido).ok_or(Error::Mala)?;
        let (Some(t), Some(c), Some(ttl_alto), Some(ttl_bajo), Some(largo)) = (u16_en(m, tras), u16_en(m, tras + 2), u16_en(m, tras + 4), u16_en(m, tras + 6), u16_en(m, tras + 8)) else {
            return Err(Error::Corta);
        };
        let datos = tras + 10;
        if datos + largo as usize > m.len() {
            return Err(Error::Corta);
        }
        let ttl = (ttl_alto as u32) << 16 | ttl_bajo as u32;
        // RFC 2181: un TTL con el bit alto vale 0.
        let ttl = if ttl & 0x8000_0000 != 0 { 0 } else { ttl.min(MAX_TTL) };
        *hueco = Some(Registro { nombre: p, tipo: t, clase: c, ttl, datos, largo: largo as usize });
        cuantos += 1;
        p = datos + largo as usize;
    }
    let registros = &registros[..cuantos];
    // La cadena de alias desde el nombre preguntado: en cada vuelta, el alias del nombre de ahora, si lo hay.
    let mut objetivo = [0u8; MAX_NOMBRE];
    objetivo[..nombre.len()].copy_from_slice(nombre);
    let mut n_objetivo = nombre.len();
    let mut ttl = MAX_TTL;
    let mut alias = 0;
    'cadena: loop {
        for r in registros.iter().flatten() {
            if r.tipo != CNAME || r.clase != IN {
                continue;
            }
            let (n, _) = leer_nombre(m, r.nombre, &mut leido).ok_or(Error::Mala)?;
            if leido[..n] != objetivo[..n_objetivo] {
                continue;
            }
            alias += 1;
            if alias > MAX_ALIAS {
                return Err(Error::Mala);
            }
            let (n, fin) = leer_nombre(m, r.datos, &mut leido).ok_or(Error::Mala)?;
            if fin > r.datos + r.largo {
                return Err(Error::Mala);
            }
            objetivo[..n].copy_from_slice(&leido[..n]);
            n_objetivo = n;
            ttl = ttl.min(r.ttl);
            continue 'cadena;
        }
        break;
    }
    let mut sal = Respuesta { dirs: [[0; 16]; MAX_DIRS], n: 0, ttl };
    let largo = if tipo == A { 4 } else { 16 };
    for r in registros.iter().flatten() {
        if r.tipo != tipo || r.clase != IN || r.largo != largo || sal.n == MAX_DIRS {
            continue;
        }
        let (n, _) = leer_nombre(m, r.nombre, &mut leido).ok_or(Error::Mala)?;
        if leido[..n] != objetivo[..n_objetivo] {
            continue;
        }
        sal.dirs[sal.n][..largo].copy_from_slice(&m[r.datos..r.datos + largo]);
        sal.n += 1;
        sal.ttl = sal.ttl.min(r.ttl);
    }
    if sal.n == 0 {
        return Err(Error::SinDatos);
    }
    Ok(sal)
}

/// Una dirección IPv4 escrita ("10.0.2.3"), si lo es: cuatro números de 0 a 255, sin ceros delante ni nada más.
pub fn ipv4_escrita(t: &[u8]) -> Option<[u8; 4]> {
    let mut sal = [0u8; 4];
    let mut partes = t.split(|&b| b == b'.');
    for hueco in sal.iter_mut() {
        let p = partes.next()?;
        if p.is_empty() || p.len() > 3 || (p.len() > 1 && p[0] == b'0') || !p.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let v = p.iter().fold(0u32, |v, &b| v * 10 + (b - b'0') as u32);
        *hueco = u8::try_from(v).ok()?;
    }
    partes.next().is_none().then_some(sal)
}

/// Una dirección IPv6 escrita ("fec0::2", "::1", entera o con un "::"), si lo es. Sin la forma con IPv4 al final.
pub fn ipv6_escrita(t: &[u8]) -> Option<[u8; 16]> {
    if t.len() < 2 || t.len() > 39 {
        return None;
    }
    let mut grupos = [0u16; 8];
    let (mut n, mut hueco) = (0usize, None);
    let mut i = 0;
    if t.starts_with(b"::") {
        hueco = Some(0);
        i = 2;
    } else if t[0] == b':' {
        return None;
    }
    while i < t.len() {
        let fin = t[i..].iter().position(|&b| b == b':').map_or(t.len(), |d| i + d);
        let g = &t[i..fin];
        if g.is_empty() || g.len() > 4 || n == 8 {
            return None;
        }
        let mut v = 0u16;
        for &b in g {
            v = v << 4 | (b as char).to_digit(16)? as u16;
        }
        grupos[n] = v;
        n += 1;
        i = fin;
        if i < t.len() {
            // Un ":" o, una sola vez, "::".
            if t[i + 1..].first() == Some(&b':') {
                if hueco.is_some() {
                    return None;
                }
                hueco = Some(n);
                i += 2;
            } else {
                i += 1;
                if i == t.len() {
                    return None;
                }
            }
        }
    }
    let mut sal = [0u8; 16];
    let poner = |sal: &mut [u8; 16], donde: usize, v: u16| sal[2 * donde..2 * donde + 2].copy_from_slice(&v.to_be_bytes());
    match hueco {
        None if n == 8 => (0..8).for_each(|k| poner(&mut sal, k, grupos[k])),
        Some(h) if n < 8 => {
            (0..h).for_each(|k| poner(&mut sal, k, grupos[k]));
            (h..n).for_each(|k| poner(&mut sal, 8 - n + k, grupos[k]));
        }
        _ => return None,
    }
    Some(sal)
}

/// Una línea de `hosts` ("dirección nombre nombre..."; lo que sigue a `#` no cuenta): la dirección (IPv4 en los 4
/// primeros bytes y `true`, o IPv6) y sus nombres, de uno en uno, para `cada`. Las líneas que no se entienden se saltan.
pub fn linea_hosts(linea: &[u8], mut cada: impl FnMut(&[u8], [u8; 16], bool)) {
    let linea = linea.split(|&b| b == b'#').next().unwrap_or(&[]);
    let mut partes = linea.split(|b| b.is_ascii_whitespace()).filter(|p| !p.is_empty());
    let Some(d) = partes.next() else { return };
    let (dir, v4) = match (ipv4_escrita(d), ipv6_escrita(d)) {
        (Some(a), _) => {
            let mut dir = [0u8; 16];
            dir[..4].copy_from_slice(&a);
            (dir, true)
        }
        (None, Some(a)) => (a, false),
        _ => return,
    };
    let mut nombre = [0u8; MAX_NOMBRE];
    for p in partes {
        if let Some(n) = normalizar(p, &mut nombre) {
            cada(&nombre[..n], dir, v4);
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn nombre_cable(v: &mut Vec<u8>, nombre: &str) {
        for e in nombre.split('.') {
            v.push(e.len() as u8);
            v.extend_from_slice(e.as_bytes());
        }
        v.push(0);
    }

    /// Una respuesta con la pregunta `nombre`/`tipo` y los registros dados: (nombre, tipo, ttl, datos).
    fn hacer(id: u16, banderas: u16, nombre: &str, tipo: u16, registros: &[(&str, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&id.to_be_bytes());
        v.extend_from_slice(&banderas.to_be_bytes());
        v.extend_from_slice(&[0, 1, 0, registros.len() as u8, 0, 0, 0, 0]);
        nombre_cable(&mut v, nombre);
        v.extend_from_slice(&tipo.to_be_bytes());
        v.extend_from_slice(&[0, 1]);
        for (n, t, ttl, d) in registros {
            nombre_cable(&mut v, n);
            v.extend_from_slice(&t.to_be_bytes());
            v.extend_from_slice(&[0, 1]);
            v.extend_from_slice(&ttl.to_be_bytes());
            v.extend_from_slice(&(d.len() as u16).to_be_bytes());
            v.extend_from_slice(d);
        }
        v
    }

    fn cname(a: &str) -> Vec<u8> {
        let mut v = Vec::new();
        nombre_cable(&mut v, a);
        v
    }

    #[test]
    fn nombres() {
        let mut s = [0u8; MAX_NOMBRE];
        assert_eq!(normalizar(b"WWW.Ejemplo.ORG.", &mut s).map(|n| s[..n].to_vec()), Some(b"www.ejemplo.org".to_vec()));
        assert_eq!(normalizar(b"a", &mut s), Some(1));
        for malo in [&b""[..], b".", b"a..b", b".a", b"a b", b"a/b", "ñ.es".as_bytes(), b"a\0b", &[b'a'; 64], &[b'a'; 254]] {
            assert_eq!(normalizar(malo, &mut s), None, "{:?}", malo);
        }
        // 253 justos: cuatro etiquetas de 63, 63, 63 y 61.
        let largo = format!("{}.{}.{}.{}", "a".repeat(63), "b".repeat(63), "c".repeat(63), "d".repeat(61));
        assert_eq!(largo.len(), 253);
        assert_eq!(normalizar(largo.as_bytes(), &mut s), Some(253));
        assert_eq!(normalizar(format!("{largo}e").as_bytes(), &mut s), None);
        // Y su pregunta cabe en `MAX_PREGUNTA`.
        let mut p = [0u8; MAX_PREGUNTA];
        assert_eq!(pregunta(7, largo.as_bytes(), AAAA, &mut p), MAX_PREGUNTA);
    }

    #[test]
    fn pregunta_y_respuesta() {
        let mut p = [0u8; MAX_PREGUNTA];
        let n = pregunta(0x1234, b"www.ejemplo.org", A, &mut p);
        assert_eq!(&p[..12], &[0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1]);
        assert_eq!(&p[12..n - 11], b"\x03www\x07ejemplo\x03org\x00\x00\x01\x00\x01");
        let r = hacer(0x1234, 0x8180, "www.ejemplo.org", A, &[("www.ejemplo.org", A, 300, vec![192, 0, 2, 1]), ("www.ejemplo.org", A, 200, vec![192, 0, 2, 2])]);
        let s = respuesta(&r, 0x1234, b"www.ejemplo.org", A).unwrap();
        assert_eq!((s.n, s.ttl, &s.dirs[0][..4], &s.dirs[1][..4]), (2, 200, &[192, 0, 2, 1][..], &[192, 0, 2, 2][..]));
        // Mayúsculas en la respuesta: el mismo nombre.
        let r = hacer(1, 0x8180, "WWW.Ejemplo.org", AAAA, &[("www.EJEMPLO.org", AAAA, 5, (1..=16).collect())]);
        let s = respuesta(&r, 1, b"www.ejemplo.org", AAAA).unwrap();
        assert_eq!((s.n, s.ttl, s.dirs[0][15]), (1, 5, 16));
    }

    #[test]
    fn alias() {
        // En desorden: la dirección antes que los alias.
        let r = hacer(1, 0x8180, "a.org", A, &[("c.org", A, 50, vec![1, 2, 3, 4]), ("b.org", CNAME, 30, cname("c.org")), ("a.org", CNAME, 90, cname("b.org"))]);
        let s = respuesta(&r, 1, b"a.org", A).unwrap();
        assert_eq!((s.n, s.ttl, &s.dirs[0][..4]), (1, 30, &[1, 2, 3, 4][..]));
        // Un bucle de alias no se sigue para siempre.
        let r = hacer(1, 0x8180, "a.org", A, &[("a.org", CNAME, 9, cname("b.org")), ("b.org", CNAME, 9, cname("a.org"))]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
        // Alias sin dirección al final.
        let r = hacer(1, 0x8180, "a.org", A, &[("a.org", CNAME, 9, cname("b.org"))]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::SinDatos));
    }

    /// Hostil: lo que no es la respuesta a nuestra pregunta no vale.
    #[test]
    fn falsas() {
        let buena = [("a.org", A, 60, vec![9, 9, 9, 9])];
        assert_eq!(respuesta(&hacer(2, 0x8180, "a.org", A, &buena), 1, b"a.org", A), Err(Error::OtroId));
        assert_eq!(respuesta(&hacer(1, 0x0100, "a.org", A, &buena), 1, b"a.org", A), Err(Error::NoEsRespuesta));
        assert_eq!(respuesta(&hacer(1, 0xa980, "a.org", A, &buena), 1, b"a.org", A), Err(Error::NoEsRespuesta));
        assert_eq!(respuesta(&hacer(1, 0x8180, "b.org", A, &buena), 1, b"a.org", A), Err(Error::OtraPregunta));
        assert_eq!(respuesta(&hacer(1, 0x8180, "a.org", AAAA, &buena), 1, b"a.org", A), Err(Error::OtraPregunta));
        assert_eq!(respuesta(&hacer(1, 0x8380, "a.org", A, &buena), 1, b"a.org", A), Err(Error::Truncada));
        assert_eq!(respuesta(&hacer(1, 0x8183, "a.org", A, &[]), 1, b"a.org", A), Err(Error::NoExiste));
        assert_eq!(respuesta(&hacer(1, 0x8182, "a.org", A, &[]), 1, b"a.org", A), Err(Error::Servidor(2)));
        assert_eq!(respuesta(&hacer(1, 0x8180, "a.org", A, &[]), 1, b"a.org", A), Err(Error::SinDatos));
        // Un registro de otro nombre (lo que mete quien envenena) no se usa, ni uno de otra clase o de otro largo.
        let r = hacer(1, 0x8180, "a.org", A, &[("banco.org", A, 60, vec![6, 6, 6, 6]), ("a.org", A, 60, vec![1, 1, 1])]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::SinDatos));
        // Sin pregunta, o con dos.
        let mut r = hacer(1, 0x8180, "a.org", A, &buena);
        r[5] = 0;
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::OtraPregunta));
        r[5] = 2;
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::OtraPregunta));
        // Más de 8 direcciones: se guardan 8. Un TTL enorme o con el bit alto: un día o 0.
        let muchas: Vec<_> = (0..20u8).map(|i| ("a.org", A, 4_000_000, vec![10, 0, 0, i])).collect();
        let s = respuesta(&hacer(1, 0x8180, "a.org", A, &muchas), 1, b"a.org", A).unwrap();
        assert_eq!((s.n, s.ttl), (MAX_DIRS, MAX_TTL));
        let s = respuesta(&hacer(1, 0x8180, "a.org", A, &[("a.org", A, 0x8000_0001, vec![1, 1, 1, 1])]), 1, b"a.org", A).unwrap();
        assert_eq!(s.ttl, 0);
    }

    /// Hostil: nombres comprimidos que apuntan a sí mismos, en círculo o hacia delante, y nombres que no acaban.
    #[test]
    fn compresion_en_bucle() {
        let cab = |id: u16, n: u8| vec![(id >> 8) as u8, id as u8, 0x81, 0x80, 0, 1, 0, n, 0, 0, 0, 0];
        // La pregunta apunta a sí misma (desplazamiento 12).
        let mut r = cab(1, 0);
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
        // La pregunta está bien; el nombre del registro apunta a sí mismo, y otro, en círculo con el siguiente.
        let pregunta_bien = |v: &mut Vec<u8>| v.extend_from_slice(b"\x01a\x03org\x00\x00\x01\x00\x01");
        let mut r = cab(1, 1);
        pregunta_bien(&mut r);
        let aqui = r.len() as u8;
        r.extend_from_slice(&[0xc0, aqui, 0, 1, 0, 1, 0, 0, 0, 9, 0, 4, 1, 2, 3, 4]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
        let mut r = cab(1, 1);
        pregunta_bien(&mut r);
        let aqui = r.len() as u8;
        r.extend_from_slice(&[1, b'x', 0xc0, aqui + 4, 0xc0, aqui, 0, 1, 0, 1, 0, 0, 0, 9, 0, 4, 1, 2, 3, 4]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
        // Bien comprimido (apunta a la pregunta): vale.
        let mut r = cab(1, 1);
        pregunta_bien(&mut r);
        r.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 9, 0, 4, 1, 2, 3, 4]);
        assert_eq!(respuesta(&r, 1, b"a.org", A).map(|s| s.n), Ok(1));
        // Una cadena de punteros hacia atrás muy larga, cada uno al anterior: acaba (tope de saltos).
        let mut r = cab(1, 1);
        pregunta_bien(&mut r);
        let base = r.len();
        for i in 0..100 {
            let a = if i == 0 { 12 } else { base + 2 * (i - 1) };
            r.extend_from_slice(&[0xc0 | (a >> 8) as u8, a as u8]);
        }
        let ultimo = base + 2 * 99;
        let mut leido = [0u8; MAX_NOMBRE];
        assert_eq!(leer_nombre(&r, base + 2 * 20, &mut leido), Some((5, base + 2 * 21)));
        assert_eq!(leer_nombre(&r, ultimo, &mut leido), None);
        // Un nombre de más de 253 a base de etiquetas, y etiquetas de tipos reservados (0x40, 0x80).
        let mut r = cab(1, 0);
        for _ in 0..5 {
            r.push(63);
            r.extend_from_slice(&[b'a'; 63]);
        }
        r.extend_from_slice(&[0, 0, 1, 0, 1]);
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
        for malo in [0x40u8, 0x80] {
            let mut r = cab(1, 0);
            r.extend_from_slice(&[malo, b'a', 0, 0, 1, 0, 1]);
            assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
        }
        // Una etiqueta con un punto dentro no pasa por dos etiquetas.
        let mut r = cab(1, 0);
        r.extend_from_slice(b"\x05a.org\x00\x00\x01\x00\x01");
        assert_eq!(respuesta(&r, 1, b"a.org", A), Err(Error::Mala));
    }

    /// Hostil: una respuesta buena cortada por cualquier sitio, y con bytes cambiados al azar: nunca revienta, y cortada
    /// nunca vale con más direcciones que la entera.
    #[test]
    fn cortadas_y_al_azar() {
        let r = hacer(1, 0x8180, "a.ejemplo.org", A, &[("a.ejemplo.org", CNAME, 30, cname("b.ejemplo.org")), ("b.ejemplo.org", A, 60, vec![1, 2, 3, 4]), ("b.ejemplo.org", A, 60, vec![5, 6, 7, 8])]);
        assert_eq!(respuesta(&r, 1, b"a.ejemplo.org", A).map(|s| s.n), Ok(2));
        for n in 0..r.len() {
            assert!(respuesta(&r[..n], 1, b"a.ejemplo.org", A).is_err(), "cortada en {n}");
        }
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut azar = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200_000 {
            let mut m = r.clone();
            for _ in 0..1 + azar() % 4 {
                let i = (azar() % m.len() as u64) as usize;
                m[i] = azar() as u8;
            }
            if azar() % 4 == 0 {
                m.truncate((azar() % (m.len() as u64 + 1)) as usize);
            }
            let _ = respuesta(&m, 1, b"a.ejemplo.org", A);
        }
        // Bytes del todo al azar.
        for _ in 0..50_000 {
            let m: Vec<u8> = (0..azar() % 600).map(|_| azar() as u8).collect();
            let _ = respuesta(&m, m.first().map_or(0, |&b| (b as u16) << 8) | m.get(1).map_or(0, |&b| b as u16), b"a.ejemplo.org", A);
        }
    }

    #[test]
    fn direcciones_escritas_y_hosts() {
        assert_eq!(ipv4_escrita(b"10.0.2.3"), Some([10, 0, 2, 3]));
        for malo in [&b"10.0.2"[..], b"10.0.2.3.4", b"256.0.0.1", b"1.2.3.04", b"1..2.3", b"a.b.c.d", b"", b"1.2.3.4 "] {
            assert_eq!(ipv4_escrita(malo), None, "{:?}", core::str::from_utf8(malo));
        }
        assert_eq!(ipv6_escrita(b"::1"), Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]));
        assert_eq!(ipv6_escrita(b"fec0::2").map(|a| (a[0], a[1], a[15])), Some((0xfe, 0xc0, 2)));
        assert_eq!(ipv6_escrita(b"1:2:3:4:5:6:7:8").map(|a| (a[1], a[15])), Some((1, 8)));
        assert_eq!(ipv6_escrita(b"2001:db8::"), Some([0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(ipv6_escrita(b"::"), Some([0; 16]));
        for malo in [&b"1:2:3:4:5:6:7"[..], b"1:2:3:4:5:6:7:8:9", b"1::2::3", b":1", b"1:", b"12345::", b"g::1", b"1:::2", b"1::2:3:4:5:6:7:8", b":"] {
            assert_eq!(ipv6_escrita(malo), None, "{:?}", core::str::from_utf8(malo));
        }
        let mut visto = Vec::new();
        for l in ["10.0.2.53  Hora.Prueba vecino.prueba # el vecino", "# nada", "", "fec0::53 vecino6.prueba", "no.es.dirección x", "1.2.3.4", "1.2.3.4 mal/nombre bien"] {
            linea_hosts(l.as_bytes(), |n, d, v4| visto.push((String::from_utf8(n.to_vec()).unwrap(), d[..4].to_vec(), v4)));
        }
        assert_eq!(visto, vec![
            ("hora.prueba".into(), vec![10, 0, 2, 53], true),
            ("vecino.prueba".into(), vec![10, 0, 2, 53], true),
            ("vecino6.prueba".into(), vec![0xfe, 0xc0, 0, 0], false),
            ("bien".into(), vec![1, 2, 3, 4], true),
        ]);
    }
}
