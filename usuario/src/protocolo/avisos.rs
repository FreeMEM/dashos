//! S10: el protocolo del servicio `avisos` (las notificaciones) y su historial. Lo usan el servicio, la barra (que los
//! enseña) y los programas que avisan. Sin `alloc` ni nada de DashOS, para probarlo en el PC tal cual:
//! `rustc --edition 2024 --test usuario/src/protocolo/avisos.rs`.
//!
//! Quién avisa no lo dice el programa: lo pone el servicio por la marca de la llave con que escribe. El nombre de cada
//! marca le llega solo sin marca (de `inicio` o de `sesiones`): la entrada de directorio de `inicio` (24 bytes: marca y
//! nombre) y `AVISO_PERSONA` de `sesiones` (marca, usuario y nombre). Lo que mande un programa con esa forma, con su
//! marca, no cambia nada.
//!
//! Mensajes (little-endian, uno por mensaje de 256 bytes como mucho; cada petición trae la llave por la que contestar):
//! - `AVISAR`, de cualquier programa con la llave: orden, urgencia (u32: `BAJA`, `NORMAL`, `URGENTE`), largo del título
//!   (u32, hasta `MAX_TITULO`), largo del texto (u32, hasta `MAX_TEXTO`), el título y el texto. Contesta `RESPUESTA_AVISAR`
//!   bytes: estado (i32), número del aviso (u32) y 16 bytes con el nombre que puso el servicio.
//! - Solo de la barra (el nombre `barra` que da `inicio`): `SUSCRIBIR` (con una segunda llave, por la que el servicio
//!   toca el timbre: `TIMBRE`, uno solo pendiente hasta que la barra pida algo), `LEER` (el aviso visible más nuevo con
//!   número menor que el que se diga; `u32::MAX` para el más nuevo), `LEIDOS` (marca como leídos todos los visibles) y
//!   `BORRAR_TODO` (borra los visibles). Contestan `CABECERA_LEER` bytes (estado, sin leer y visibles, u32) y, en `LEER`,
//!   el aviso (`Aviso::escribir`).
//!
//! Visibles: los del sistema y los de las personas que no tienen la sesión detrás (U6). Los de una sesión de detrás se
//! guardan y salen cuando su dueña vuelve delante.

/// Órdenes.
pub const AVISAR: u32 = 1;
pub const SUSCRIBIR: u32 = 10;
pub const LEER: u32 = 11;
pub const LEIDOS: u32 = 12;
pub const BORRAR_TODO: u32 = 13;

/// Lo que manda el servicio por la llave del timbre: hay algo nuevo que enseñar.
pub const TIMBRE: u32 = 0x4156_4953;

/// Urgencias.
pub const BAJA: u8 = 0;
pub const NORMAL: u8 = 1;
pub const URGENTE: u8 = 2;

/// Estados.
pub const BIEN: i32 = 0;
/// El mensaje no se entiende (largos que no cuadran, urgencia que no existe, sin título).
pub const MAL: i32 = -1;
/// No le toca (lo de la barra, a otro).
pub const PROTEGIDO: i32 = -2;
/// Demasiados avisos seguidos de ese programa (`RITMO` por `VENTANA_RITMO_US`).
pub const DEMASIADOS: i32 = -3;
/// `LEER`: no hay más.
pub const NO_HAY: i32 = -4;

/// Lo más largo de un título y de un texto, en bytes de UTF-8.
pub const MAX_TITULO: usize = 48;
pub const MAX_TEXTO: usize = 160;
/// La cabecera de `AVISAR`.
pub const CABECERA_AVISAR: usize = 16;
pub const RESPUESTA_AVISAR: usize = 24;
pub const CABECERA_LEER: usize = 12;
/// Lo fijo de un aviso al leerlo: número, urgencia, banderas, largos, hora y de quién.
pub const FIJO_AVISO: usize = 32;
/// Banderas de un aviso al leerlo.
pub const LEIDO: u8 = 1;
pub const SISTEMA: u8 = 2;
/// Se guardó con la sesión de su dueña detrás (U6): sale en el historial, no como aviso nuevo.
pub const RETENIDO: u8 = 4;

/// El historial: cuántos avisos guarda, y cuántos de un mismo programa (un programa que inunda solo se pisa a sí mismo).
pub const MAX_HISTORIAL: usize = 50;
pub const POR_PROGRAMA: usize = 8;
/// El ritmo: como mucho `RITMO` avisos de un programa en `VENTANA_RITMO_US`; los demás, `DEMASIADOS`.
pub const RITMO: u32 = 10;
pub const VENTANA_RITMO_US: u64 = 60_000_000;
/// Los programas que lleva el ritmo a la vez (con más, se olvida el que lleva más tiempo sin avisar).
pub const MAX_REMITENTES: usize = 32;
/// Las personas tienen el usuario 1000 o más (S6b), como en `sesiones`.
pub const PRIMERA_PERSONA: u32 = 1000;

fn u32_en(m: &[u8], i: usize) -> Option<u32> {
    m.get(i..i + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()))
}

/// Copia `entrada` en `salida` como texto que se puede enseñar: UTF-8 válido, sin letras de control (un salto de línea
/// pasa a espacio; lo demás, y lo que no es UTF-8, a "?"), cortado en una letra entera si no cabe. Devuelve cuántos
/// bytes escribió.
pub fn limpiar(entrada: &[u8], salida: &mut [u8]) -> usize {
    let mut n = 0;
    let mut resto = entrada;
    while !resto.is_empty() {
        let (valido, malo) = match core::str::from_utf8(resto) {
            Ok(s) => (s, 0),
            Err(e) => {
                let v = core::str::from_utf8(&resto[..e.valid_up_to()]).unwrap_or("");
                (v, e.error_len().unwrap_or(resto.len() - e.valid_up_to()))
            }
        };
        for c in valido.chars() {
            let c = match c {
                '\n' | '\t' => ' ',
                c if c.is_control() => '?',
                c => c,
            };
            let mut b = [0u8; 4];
            let s = c.encode_utf8(&mut b).as_bytes();
            if n + s.len() > salida.len() {
                return n;
            }
            salida[n..n + s.len()].copy_from_slice(s);
            n += s.len();
        }
        resto = &resto[valido.len()..];
        if malo > 0 {
            if n + 1 > salida.len() {
                return n;
            }
            salida[n] = b'?';
            n += 1;
            resto = &resto[malo..];
        }
    }
    n
}

/// Escribe en `m` la petición `AVISAR`. Lo que no quepa del título o del texto se corta (en una letra entera). Devuelve
/// el largo del mensaje.
pub fn pedir_aviso(m: &mut [u8; 256], urgencia: u8, titulo: &str, texto: &str) -> usize {
    let corta = |s: &str, max: usize| {
        let mut l = s.len().min(max);
        while !s.is_char_boundary(l) {
            l -= 1;
        }
        l
    };
    let (lt, lx) = (corta(titulo, MAX_TITULO), corta(texto, MAX_TEXTO));
    m[0..4].copy_from_slice(&AVISAR.to_le_bytes());
    m[4..8].copy_from_slice(&(urgencia as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(lt as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(lx as u32).to_le_bytes());
    m[16..16 + lt].copy_from_slice(&titulo.as_bytes()[..lt]);
    m[16 + lt..16 + lt + lx].copy_from_slice(&texto.as_bytes()[..lx]);
    CABECERA_AVISAR + lt + lx
}

/// Lee una petición `AVISAR`: urgencia, título y texto (aún sin limpiar). `MAL` si los largos no cuadran con el
/// mensaje, pasan del máximo, la urgencia no existe o no hay título.
pub fn leer_aviso(m: &[u8]) -> Result<(u8, &[u8], &[u8]), i32> {
    let (Some(AVISAR), Some(u), Some(lt), Some(lx)) = (u32_en(m, 0), u32_en(m, 4), u32_en(m, 8), u32_en(m, 12)) else {
        return Err(MAL);
    };
    let (lt, lx) = (lt as usize, lx as usize);
    if u > URGENTE as u32 || lt == 0 || lt > MAX_TITULO || lx > MAX_TEXTO || m.len() != CABECERA_AVISAR + lt + lx {
        return Err(MAL);
    }
    Ok((u as u8, &m[16..16 + lt], &m[16 + lt..]))
}

/// Un nombre de 16 bytes, rellenos de ceros, como texto (lo que no sea UTF-8, "?").
pub fn nombre16(n: &[u8; 16]) -> &str {
    core::str::from_utf8(&n[..n.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

/// Un aviso guardado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aviso {
    pub numero: u32,
    pub urgencia: u8,
    pub leido: bool,
    /// Si lo mandó un programa de `inicio` (del sistema); si no, uno lanzado del disco, con el nombre de su fichero.
    pub sistema: bool,
    /// Si llegó con la sesión de su dueña detrás.
    pub retenido: bool,
    /// De quién: su marca, su usuario (por debajo de `PRIMERA_PERSONA`, del sistema) y su nombre.
    pub marca: u64,
    pub uid: u32,
    pub de: [u8; 16],
    /// La hora del calendario (ns), si se sabe; si no, 0.
    pub hora: u64,
    pub lt: u8,
    pub lx: u8,
    pub titulo: [u8; MAX_TITULO],
    pub texto: [u8; MAX_TEXTO],
}

impl Aviso {
    pub fn titulo(&self) -> &str {
        core::str::from_utf8(&self.titulo[..self.lt as usize]).unwrap_or("?")
    }

    pub fn texto(&self) -> &str {
        core::str::from_utf8(&self.texto[..self.lx as usize]).unwrap_or("?")
    }

    pub fn de(&self) -> &str {
        nombre16(&self.de)
    }

    /// Lo escribe en `m` como lo lee la barra. Devuelve cuántos bytes.
    pub fn escribir(&self, m: &mut [u8]) -> usize {
        let (lt, lx) = (self.lt as usize, self.lx as usize);
        m[0..4].copy_from_slice(&self.numero.to_le_bytes());
        m[4] = self.urgencia;
        m[5] = (self.leido as u8 * LEIDO) | (self.sistema as u8 * SISTEMA) | (self.retenido as u8 * RETENIDO);
        m[6] = self.lt;
        m[7] = self.lx;
        m[8..16].copy_from_slice(&self.hora.to_le_bytes());
        m[16..32].copy_from_slice(&self.de);
        m[32..32 + lt].copy_from_slice(&self.titulo[..lt]);
        m[32 + lt..32 + lt + lx].copy_from_slice(&self.texto[..lx]);
        FIJO_AVISO + lt + lx
    }

    /// Lo lee como lo escribe `escribir`, desconfiando: largos que no cuadran, fuera; los textos, otra vez limpios.
    pub fn leer(m: &[u8]) -> Option<Aviso> {
        if m.len() < FIJO_AVISO {
            return None;
        }
        let (lt, lx) = (m[6] as usize, m[7] as usize);
        if lt > MAX_TITULO || lx > MAX_TEXTO || m.len() != FIJO_AVISO + lt + lx {
            return None;
        }
        let mut a = Aviso::vacio();
        a.numero = u32_en(m, 0)?;
        a.urgencia = m[4].min(URGENTE);
        a.leido = m[5] & LEIDO != 0;
        a.sistema = m[5] & SISTEMA != 0;
        a.retenido = m[5] & RETENIDO != 0;
        a.hora = u64::from_le_bytes(m[8..16].try_into().unwrap());
        let mut de = [0u8; 16];
        let n = limpiar(&m[16..32].split(|&c| c == 0).next().unwrap_or(b""), &mut de);
        de[n..].fill(0);
        a.de = de;
        a.lt = limpiar(&m[32..32 + lt], &mut a.titulo) as u8;
        a.lx = limpiar(&m[32 + lt..32 + lt + lx], &mut a.texto) as u8;
        Some(a)
    }

    pub const fn vacio() -> Aviso {
        Aviso { numero: 0, urgencia: 0, leido: false, sistema: false, retenido: false, marca: 0, uid: 0, de: [0; 16], hora: 0, lt: 0, lx: 0, titulo: [0; MAX_TITULO], texto: [0; MAX_TEXTO] }
    }
}

/// Si un aviso de `uid` se ve con las personas `detras` con su sesión detrás (U6): los del sistema siempre; los de una
/// persona, si no está detrás.
pub fn visible(uid: u32, detras: &[u32]) -> bool {
    uid < PRIMERA_PERSONA || !detras.contains(&uid)
}

/// El historial: los últimos `MAX_HISTORIAL` avisos, como mucho `POR_PROGRAMA` de cada marca.
pub struct Historial {
    avisos: [Option<Aviso>; MAX_HISTORIAL],
    siguiente: u32,
}

impl Default for Historial {
    fn default() -> Self {
        Self::new()
    }
}

impl Historial {
    pub const fn new() -> Historial {
        Historial { avisos: [None; MAX_HISTORIAL], siguiente: 1 }
    }

    /// Guarda un aviso (de `a` se usa todo menos el número, que pone el historial, y `leido`). Si ese programa ya tiene
    /// `POR_PROGRAMA`, se va el más viejo suyo; si el historial está lleno, el más viejo de todos. Devuelve su número.
    pub fn meter(&mut self, mut a: Aviso) -> u32 {
        a.numero = self.siguiente;
        a.leido = false;
        self.siguiente = self.siguiente.wrapping_add(1).max(1);
        let suyos = self.avisos.iter().flatten().filter(|x| x.marca == a.marca).count();
        let hueco = if suyos >= POR_PROGRAMA {
            self.mas_viejo(|x| x.marca == a.marca)
        } else {
            self.avisos.iter().position(|x| x.is_none()).or_else(|| self.mas_viejo(|_| true))
        };
        if let Some(i) = hueco {
            self.avisos[i] = Some(a);
        }
        a.numero
    }

    fn mas_viejo(&self, f: impl Fn(&Aviso) -> bool) -> Option<usize> {
        (0..MAX_HISTORIAL).filter(|&i| self.avisos[i].is_some_and(|x| f(&x))).min_by_key(|&i| self.avisos[i].unwrap().numero)
    }

    /// Los que se ven con `detras`, del más nuevo al más viejo.
    pub fn visibles<'a>(&'a self, detras: &'a [u32]) -> impl Iterator<Item = &'a Aviso> + 'a {
        let mut v: [Option<&Aviso>; MAX_HISTORIAL] = [None; MAX_HISTORIAL];
        let mut n = 0;
        for a in self.avisos.iter().flatten().filter(|a| visible(a.uid, detras)) {
            v[n] = Some(a);
            n += 1;
        }
        v[..n].sort_unstable_by_key(|a| core::cmp::Reverse(a.unwrap().numero));
        v.into_iter().take(n).flatten()
    }

    /// El visible más nuevo con número menor que `antes`.
    pub fn anterior(&self, antes: u32, detras: &[u32]) -> Option<Aviso> {
        self.visibles(detras).find(|a| a.numero < antes).copied()
    }

    /// Cuántos visibles hay y cuántos sin leer.
    pub fn cuenta(&self, detras: &[u32]) -> (u32, u32) {
        let (mut total, mut sin) = (0, 0);
        for a in self.visibles(detras) {
            total += 1;
            sin += !a.leido as u32;
        }
        (total, sin)
    }

    /// Marca como leídos los visibles.
    pub fn leidos(&mut self, detras: &[u32]) {
        for a in self.avisos.iter_mut().flatten().filter(|a| visible(a.uid, detras)) {
            a.leido = true;
        }
    }

    /// Borra los visibles; lo de las sesiones de detrás se queda.
    pub fn borrar_visibles(&mut self, detras: &[u32]) {
        for x in self.avisos.iter_mut() {
            if x.is_some_and(|a| visible(a.uid, detras)) {
                *x = None;
            }
        }
    }

    /// Cuántos hay de `uid` (los de una sesión de detrás, guardados).
    pub fn de_usuario(&self, uid: u32) -> usize {
        self.avisos.iter().flatten().filter(|a| a.uid == uid).count()
    }
}

/// El ritmo de cada programa: cuántos avisos lleva en su ventana de tiempo.
pub struct Ritmo {
    /// Marca, inicio de su ventana (µs) y cuántos en ella.
    r: [(u64, u64, u32); MAX_REMITENTES],
}

impl Default for Ritmo {
    fn default() -> Self {
        Self::new()
    }
}

impl Ritmo {
    pub const fn new() -> Ritmo {
        Ritmo { r: [(0, 0, 0); MAX_REMITENTES] }
    }

    /// Si `marca` puede avisar ahora (`ahora_us`); si puede, lo cuenta.
    pub fn puede(&mut self, marca: u64, ahora_us: u64) -> bool {
        let i = match self.r.iter().position(|x| x.0 == marca && x.2 > 0) {
            Some(i) => i,
            None => {
                // Uno libre, o el que empezó su ventana hace más.
                let i = self.r.iter().position(|x| x.2 == 0).unwrap_or_else(|| (0..MAX_REMITENTES).min_by_key(|&i| self.r[i].1).unwrap());
                self.r[i] = (marca, ahora_us, 0);
                i
            }
        };
        let e = &mut self.r[i];
        if ahora_us.saturating_sub(e.1) >= VENTANA_RITMO_US {
            *e = (marca, ahora_us, 0);
        }
        if e.2 >= RITMO {
            return false;
        }
        e.2 += 1;
        true
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn aviso(marca: u64, uid: u32, titulo: &str) -> Aviso {
        let mut a = Aviso::vacio();
        a.marca = marca;
        a.uid = uid;
        a.lt = limpiar(titulo.as_bytes(), &mut a.titulo) as u8;
        a
    }

    #[test]
    fn ida_y_vuelta_de_avisar() {
        let mut m = [0u8; 256];
        let n = pedir_aviso(&mut m, NORMAL, "Copia terminada", "3 ficheros en Home:");
        let (u, t, x) = leer_aviso(&m[..n]).unwrap();
        assert_eq!((u, t, x), (NORMAL, &b"Copia terminada"[..], &b"3 ficheros en Home:"[..]));
        // Lo que no cabe se corta en una letra entera.
        let largo = "ñ".repeat(100);
        let n = pedir_aviso(&mut m, BAJA, &largo, &largo);
        let (_, t, x) = leer_aviso(&m[..n]).unwrap();
        assert_eq!((t.len(), x.len()), (MAX_TITULO, MAX_TEXTO));
        assert!(core::str::from_utf8(t).is_ok() && core::str::from_utf8(x).is_ok());
        assert!(n <= 256);
    }

    #[test]
    fn avisar_mal_formado() {
        let mut m = [0u8; 256];
        let n = pedir_aviso(&mut m, URGENTE, "a", "b");
        // Mensajes cortos, largos que no cuadran o enormes, urgencia que no existe, sin título u otra orden.
        assert_eq!(leer_aviso(&m[..n - 1]), Err(MAL));
        assert_eq!(leer_aviso(&m[..3]), Err(MAL));
        assert_eq!(leer_aviso(&[]), Err(MAL));
        let mut x = m;
        x[12..16].copy_from_slice(&60_000u32.to_le_bytes());
        assert_eq!(leer_aviso(&x[..n]), Err(MAL));
        let mut x = m;
        x[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(leer_aviso(&x[..n]), Err(MAL));
        let mut x = m;
        x[4..8].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(leer_aviso(&x[..n]), Err(MAL));
        let n0 = pedir_aviso(&mut x, NORMAL, "", "texto");
        assert_eq!(leer_aviso(&x[..n0]), Err(MAL));
        let mut x = m;
        x[0..4].copy_from_slice(&LEER.to_le_bytes());
        assert_eq!(leer_aviso(&x[..n]), Err(MAL));
    }

    #[test]
    fn limpiar_textos() {
        let mut s = [0u8; 16];
        let n = limpiar(b"hola\nmundo\x07", &mut s);
        assert_eq!(&s[..n], b"hola mundo?");
        let n = limpiar(b"a\xff\xfeb\xc3", &mut s);
        assert_eq!(&s[..n], b"a??b?");
        // Corta en una letra entera.
        let mut s = [0u8; 5];
        let n = limpiar("ññññ".as_bytes(), &mut s);
        assert_eq!(&s[..n], "ññ".as_bytes());
        let mut s = [0u8; 0];
        assert_eq!(limpiar(b"abc", &mut s), 0);
    }

    #[test]
    fn ida_y_vuelta_de_leer() {
        let mut a = aviso(5, 1, "Disco lleno");
        a.numero = 7;
        a.urgencia = URGENTE;
        a.sistema = true;
        a.retenido = true;
        a.hora = 123;
        a.de[..7].copy_from_slice(b"archivo");
        a.lx = limpiar(b"Queda poco", &mut a.texto) as u8;
        let mut m = [0u8; 256];
        let n = a.escribir(&mut m);
        assert!(CABECERA_LEER + n <= 256);
        let b = Aviso::leer(&m[..n]).unwrap();
        assert_eq!((b.numero, b.urgencia, b.sistema, b.retenido, b.leido, b.hora, b.de(), b.titulo(), b.texto()), (7, URGENTE, true, true, false, 123, "archivo", "Disco lleno", "Queda poco"));
        // Desconfía: largos que no cuadran, fuera; bytes malos, limpios.
        assert!(Aviso::leer(&m[..n - 1]).is_none());
        let mut x = m;
        x[6] = 200;
        assert!(Aviso::leer(&x[..n]).is_none());
        let mut x = m;
        x[32] = 0xff;
        x[16] = 0x07;
        let c = Aviso::leer(&x[..n]).unwrap();
        assert!(c.titulo().starts_with('?') && c.de().starts_with('?'));
    }

    #[test]
    fn el_historial_tiene_tope() {
        let mut h = Historial::new();
        for i in 0..100 {
            h.meter(aviso(3, 1, if i % 2 == 0 { "par" } else { "impar" }));
        }
        // Un programa que inunda solo ocupa POR_PROGRAMA, y son los últimos.
        assert_eq!(h.cuenta(&[]), (POR_PROGRAMA as u32, POR_PROGRAMA as u32));
        assert_eq!(h.visibles(&[]).next().unwrap().numero, 100);
        let n = h.meter(aviso(4, 1, "otro"));
        assert_eq!(h.visibles(&[]).next().unwrap().numero, n);
        // Muchos programas: el historial, MAX_HISTORIAL como mucho, sin perder el más nuevo.
        for m in 10..200 {
            h.meter(aviso(m, 1, "uno"));
        }
        assert_eq!(h.cuenta(&[]).0 as usize, MAX_HISTORIAL);
        let mas_nuevo = h.meter(aviso(9, 1, "último"));
        assert_eq!(h.anterior(u32::MAX, &[]).unwrap().numero, mas_nuevo);
        // `anterior` recorre del más nuevo al más viejo.
        let mut antes = u32::MAX;
        let mut vistos = 0;
        while let Some(a) = h.anterior(antes, &[]) {
            assert!(a.numero < antes);
            antes = a.numero;
            vistos += 1;
        }
        assert_eq!(vistos, MAX_HISTORIAL);
    }

    #[test]
    fn la_sesion_de_detras() {
        let mut h = Historial::new();
        h.meter(aviso(1, 1, "del sistema"));
        h.meter(aviso(0x1_0000, 1000, "de Ana"));
        h.meter(aviso(0x1_0001, 1001, "de Carla"));
        // Con Ana detrás: no se ve lo suyo, ni se borra ni se marca.
        let ana_detras = [1000];
        assert_eq!(h.cuenta(&ana_detras), (2, 2));
        assert!(h.visibles(&ana_detras).all(|a| a.uid != 1000));
        h.leidos(&ana_detras);
        h.borrar_visibles(&ana_detras);
        assert_eq!(h.cuenta(&ana_detras), (0, 0));
        assert_eq!(h.de_usuario(1000), 1);
        // Vuelve Ana: lo suyo sale, sin leer.
        assert_eq!(h.cuenta(&[1001]), (1, 1));
        assert_eq!(h.anterior(u32::MAX, &[1001]).unwrap().titulo(), "de Ana");
    }

    #[test]
    fn el_ritmo() {
        let mut r = Ritmo::new();
        for _ in 0..RITMO {
            assert!(r.puede(7, 1000));
        }
        assert!(!r.puede(7, 2000));
        // Los demás siguen pudiendo, aunque sean muchos.
        for m in 100..100 + 2 * MAX_REMITENTES as u64 {
            assert!(r.puede(m, 3000));
        }
        assert!(r.puede(8, 3000));
        // Pasada la ventana, otra vez.
        assert!(r.puede(7, 1000 + VENTANA_RITMO_US));
    }
}
