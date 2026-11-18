//! Lo que no toca la memoria del 68k: las direcciones en texto (`inet_addr`, `inet_aton`, `inet_network`,
//! `inet_pton`, `Inet_NtoA`), las clases de red (`Inet_LnaOf`, `Inet_NetOf`, `Inet_MakeAddr`), las tablas pequeñas y
//! fijas de servicios, protocolos y redes, y los textos de los errores.
//!
//! Escrito desde los autodocs públicos de bsdsocket.library (la sección "INTERNET ADDRESSES" de `Inet_NtoA`) y las
//! páginas de manual de BSD como especificación. No se ha leído código de AmiTCP, Roadshow ni BSD.

use alloc::vec::Vec;

/// Un número de una parte de una dirección en texto, como en C: `0x` hexadecimal, `0` octal, si no decimal. Devuelve
/// el valor y lo que queda.
fn numero(s: &[u8]) -> Option<(u64, &[u8])> {
    let (base, mut i) = if s.len() > 1 && s[0] == b'0' && (s[1] == b'x' || s[1] == b'X') {
        (16, 2)
    } else if !s.is_empty() && s[0] == b'0' {
        (8, 1)
    } else {
        (10, 0)
    };
    let inicio = i;
    let mut v: u64 = 0;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => (c - b'0') as u64,
            c @ b'a'..=b'f' => (c - b'a' + 10) as u64,
            c @ b'A'..=b'F' => (c - b'A' + 10) as u64,
            _ => break,
        };
        if d >= base {
            return None;
        }
        v = v * base + d;
        if v > u32::MAX as u64 {
            return None;
        }
        i += 1;
    }
    // "0" solo (octal sin dígitos) es un cero; "0x" sin dígitos no vale.
    if i == inicio && base != 8 {
        return None;
    }
    Some((v, &s[i..]))
}

/// Las partes de una dirección con puntos (1 a 4). Detrás se admite un espacio o el fin.
fn partes(s: &[u8]) -> Option<Vec<u64>> {
    let mut v = Vec::new();
    let mut resto = s;
    loop {
        let (n, r) = numero(resto)?;
        v.push(n);
        if v.len() > 4 {
            return None;
        }
        match r.first() {
            Some(b'.') => resto = &r[1..],
            None => return Some(v),
            Some(c) if c.is_ascii_whitespace() => return Some(v),
            Some(_) => return None,
        }
    }
}

/// `inet_aton`: `a.b.c.d`, `a.b.c` (la última de 16 bits), `a.b` (de 24) o `a` (de 32), cada parte en decimal, octal o
/// hexadecimal. La dirección como número (el orden de red es el del 68k).
pub fn inet_aton(s: &[u8]) -> Option<u32> {
    let p = partes(s)?;
    let max = |bits: u32| (1u64 << bits) - 1;
    let v = match p.len() {
        1 => p[0],
        2 if p[0] <= 255 && p[1] <= max(24) => p[0] << 24 | p[1],
        3 if p[0] <= 255 && p[1] <= 255 && p[2] <= max(16) => p[0] << 24 | p[1] << 16 | p[2],
        4 if p.iter().all(|&x| x <= 255) => p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3],
        _ => return None,
    };
    Some(v as u32)
}

/// `inet_network`: un número de red, cada parte de 8 bits, la primera la más alta (`"10.1"` es `0x0A01`).
pub fn inet_network(s: &[u8]) -> Option<u32> {
    let p = partes(s)?;
    if p.iter().any(|&x| x > 255) {
        return None;
    }
    Some(p.iter().fold(0u64, |v, &x| v << 8 | x) as u32)
}

/// `inet_pton(AF_INET)`: solo `a.b.c.d` en decimal, sin ceros delante.
pub fn inet_pton4(s: &[u8]) -> Option<u32> {
    let mut v: u32 = 0;
    let mut n = 0;
    for parte in s.split(|&c| c == b'.') {
        if parte.is_empty() || parte.len() > 3 || !parte.iter().all(u8::is_ascii_digit) {
            return None;
        }
        if parte.len() > 1 && parte[0] == b'0' {
            return None;
        }
        let x = parte.iter().fold(0u32, |a, &c| a * 10 + (c - b'0') as u32);
        if x > 255 {
            return None;
        }
        v = v << 8 | x;
        n += 1;
    }
    if n == 4 { Some(v) } else { None }
}

/// `a.b.c.d`, sin el 0 del final.
pub fn ntoa(ip: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(15);
    for (i, b) in ip.to_be_bytes().iter().enumerate() {
        if i > 0 {
            v.push(b'.');
        }
        let mut b = *b;
        let mut d = [0u8; 3];
        let mut k = 0;
        loop {
            d[k] = b'0' + b % 10;
            k += 1;
            b /= 10;
            if b == 0 {
                break;
            }
        }
        v.extend(d[..k].iter().rev());
    }
    v
}

fn clase_a(i: u32) -> bool {
    i & 0x8000_0000 == 0
}
fn clase_b(i: u32) -> bool {
    i & 0xC000_0000 == 0x8000_0000
}

/// `Inet_LnaOf`: la parte del anfitrión, según la clase.
pub fn lnaof(i: u32) -> u32 {
    if clase_a(i) {
        i & 0x00FF_FFFF
    } else if clase_b(i) {
        i & 0xFFFF
    } else {
        i & 0xFF
    }
}

/// `Inet_NetOf`: el número de red, según la clase.
pub fn netof(i: u32) -> u32 {
    if clase_a(i) {
        i >> 24
    } else if clase_b(i) {
        i >> 16
    } else {
        i >> 8
    }
}

/// `Inet_MakeAddr`: red y anfitrión juntos.
pub fn makeaddr(red: u32, anfitrion: u32) -> u32 {
    if red < 128 {
        red << 24 | anfitrion & 0x00FF_FFFF
    } else if red < 65536 {
        red << 16 | anfitrion & 0xFFFF
    } else if red < 16_777_216 {
        red << 8 | anfitrion & 0xFF
    } else {
        red | anfitrion
    }
}

/// Un servicio de la tabla: nombre, alias, puerto y protocolo.
#[derive(Clone, Copy, Debug)]
pub struct Servicio {
    pub nombre: &'static str,
    pub alias: &'static [&'static str],
    pub puerto: u16,
    pub proto: &'static str,
}

const fn s(nombre: &'static str, alias: &'static [&'static str], puerto: u16, proto: &'static str) -> Servicio {
    Servicio { nombre, alias, puerto, proto }
}

/// Los servicios que conoce `getservbyname` (una tabla pequeña y fija: los de siempre).
pub const SERVICIOS: &[Servicio] = &[
    s("echo", &[], 7, "tcp"),
    s("echo", &[], 7, "udp"),
    s("discard", &["sink", "null"], 9, "tcp"),
    s("discard", &["sink", "null"], 9, "udp"),
    s("daytime", &[], 13, "tcp"),
    s("daytime", &[], 13, "udp"),
    s("ftp-data", &[], 20, "tcp"),
    s("ftp", &[], 21, "tcp"),
    s("ssh", &[], 22, "tcp"),
    s("telnet", &[], 23, "tcp"),
    s("smtp", &["mail"], 25, "tcp"),
    s("time", &["timserver"], 37, "tcp"),
    s("time", &["timserver"], 37, "udp"),
    s("domain", &["dns", "nameserver"], 53, "tcp"),
    s("domain", &["dns", "nameserver"], 53, "udp"),
    s("tftp", &[], 69, "udp"),
    s("gopher", &[], 70, "tcp"),
    s("finger", &[], 79, "tcp"),
    s("http", &["www", "www-http"], 80, "tcp"),
    s("pop3", &["pop-3"], 110, "tcp"),
    s("nntp", &["usenet"], 119, "tcp"),
    s("ntp", &[], 123, "udp"),
    s("imap", &["imap2"], 143, "tcp"),
    s("snmp", &[], 161, "udp"),
    s("irc", &[], 194, "tcp"),
    s("ldap", &[], 389, "tcp"),
    s("https", &[], 443, "tcp"),
    s("submissions", &["smtps"], 465, "tcp"),
    s("submission", &[], 587, "tcp"),
    s("ldaps", &[], 636, "tcp"),
    s("imaps", &[], 993, "tcp"),
    s("pop3s", &[], 995, "tcp"),
    s("socks", &[], 1080, "tcp"),
    s("ircd", &[], 6667, "tcp"),
    s("http-alt", &["webcache"], 8080, "tcp"),
];

fn igual(a: &[u8], b: &str) -> bool {
    a.eq_ignore_ascii_case(b.as_bytes())
}

/// `getservbyname`: por nombre o alias, y del protocolo si se da.
pub fn servicio_por_nombre(nombre: &[u8], proto: Option<&[u8]>) -> Option<&'static Servicio> {
    SERVICIOS.iter().find(|x| {
        (igual(nombre, x.nombre) || x.alias.iter().any(|a| igual(nombre, a))) && proto.is_none_or(|p| igual(p, x.proto))
    })
}

/// `getservbyport`.
pub fn servicio_por_puerto(puerto: u16, proto: Option<&[u8]>) -> Option<&'static Servicio> {
    SERVICIOS.iter().find(|x| x.puerto == puerto && proto.is_none_or(|p| igual(p, x.proto)))
}

/// Los protocolos (`getprotobyname`): nombre, número y alias.
pub const PROTOCOLOS: &[(&str, u32, &str)] =
    &[("ip", 0, "IP"), ("icmp", 1, "ICMP"), ("igmp", 2, "IGMP"), ("tcp", 6, "TCP"), ("udp", 17, "UDP")];

pub fn protocolo_por_nombre(n: &[u8]) -> Option<(&'static str, u32)> {
    PROTOCOLOS.iter().find(|p| igual(n, p.0) || igual(n, p.2)).map(|p| (p.0, p.1))
}

pub fn protocolo_por_numero(n: u32) -> Option<(&'static str, u32)> {
    PROTOCOLOS.iter().find(|p| p.1 == n).map(|p| (p.0, p.1))
}

/// Las redes (`getnetbyname`): solo la de la propia máquina.
pub const REDES: &[(&str, u32)] = &[("loopback", 127)];

pub fn red_por_nombre(n: &[u8]) -> Option<(&'static str, u32)> {
    REDES.iter().find(|r| igual(n, r.0)).copied()
}

pub fn red_por_numero(n: u32) -> Option<(&'static str, u32)> {
    REDES.iter().find(|r| r.1 == n).copied()
}

/// El texto de un `errno` (`SBTC_ERRNOSTRPTR`), de 0 a `ULTIMO_ERRNO`.
pub const ULTIMO_ERRNO: i32 = 80;
pub fn texto_errno(e: i32) -> &'static str {
    match e {
        0 => "Undefined error: 0",
        1 => "Operation not permitted",
        2 => "No such file or directory",
        3 => "No such process",
        4 => "Interrupted system call",
        5 => "Input/output error",
        6 => "Device not configured",
        7 => "Argument list too long",
        8 => "Exec format error",
        9 => "Bad file descriptor",
        10 => "No child processes",
        11 => "Resource deadlock avoided",
        12 => "Cannot allocate memory",
        13 => "Permission denied",
        14 => "Bad address",
        15 => "Block device required",
        16 => "Device busy",
        17 => "File exists",
        18 => "Cross-device link",
        19 => "Operation not supported by device",
        20 => "Not a directory",
        21 => "Is a directory",
        22 => "Invalid argument",
        23 => "Too many open files in system",
        24 => "Too many open files",
        25 => "Inappropriate ioctl for device",
        26 => "Text file busy",
        27 => "File too large",
        28 => "No space left on device",
        29 => "Illegal seek",
        30 => "Read-only file system",
        31 => "Too many links",
        32 => "Broken pipe",
        33 => "Numerical argument out of domain",
        34 => "Result too large",
        35 => "Resource temporarily unavailable",
        36 => "Operation now in progress",
        37 => "Operation already in progress",
        38 => "Socket operation on non-socket",
        39 => "Destination address required",
        40 => "Message too long",
        41 => "Protocol wrong type for socket",
        42 => "Protocol not available",
        43 => "Protocol not supported",
        44 => "Socket type not supported",
        45 => "Operation not supported",
        46 => "Protocol family not supported",
        47 => "Address family not supported by protocol family",
        48 => "Address already in use",
        49 => "Can't assign requested address",
        50 => "Network is down",
        51 => "Network is unreachable",
        52 => "Network dropped connection on reset",
        53 => "Software caused connection abort",
        54 => "Connection reset by peer",
        55 => "No buffer space available",
        56 => "Socket is already connected",
        57 => "Socket is not connected",
        58 => "Can't send after socket shutdown",
        59 => "Too many references: can't splice",
        60 => "Operation timed out",
        61 => "Connection refused",
        62 => "Too many levels of symbolic links",
        63 => "File name too long",
        64 => "Host is down",
        65 => "No route to host",
        66 => "Directory not empty",
        67 => "Too many processes",
        68 => "Too many users",
        69 => "Disc quota exceeded",
        70 => "Stale NFS file handle",
        71 => "Too many levels of remote in path",
        72 => "RPC struct is bad",
        73 => "RPC version wrong",
        74 => "RPC prog. not avail",
        75 => "Program version wrong",
        76 => "Bad procedure for program",
        77 => "No locks available",
        78 => "Function not implemented",
        79 => "Inappropriate file type or format",
        80 => "Authentication error",
        _ => "Unknown error",
    }
}

/// El texto de un `h_errno` (`SBTC_HERRNOSTRPTR`), de -1 a 4.
pub fn texto_herrno(h: i32) -> &'static str {
    match h {
        -1 => "Resolver internal error",
        0 => "Resolver Error 0 (no error)",
        1 => "Unknown host",
        2 => "Host name lookup failure",
        3 => "Unknown server error",
        4 => "No address associated with name",
        _ => "Unknown resolver error",
    }
}

/// El texto de un error de `getaddrinfo` (`gai_strerror`), de 0 a -13.
pub fn texto_gai(e: i32) -> &'static str {
    match e {
        0 => "Success",
        -1 => "Invalid value for ai_flags",
        -2 => "Name or service is not known",
        -3 => "Temporary failure in name resolution",
        -4 => "Non-recoverable failure in name resolution",
        -5 => "No address associated with name",
        -6 => "ai_family not supported",
        -7 => "ai_socktype not supported",
        -8 => "Service not supported for ai_socktype",
        -9 => "Address family for name not supported",
        -10 => "Memory allocation failure",
        -11 => "System error returned in errno",
        -12 => "Invalid value for hints",
        -13 => "Resolved protocol is unknown",
        _ => "Unknown error",
    }
}
