//! S10, S27 (oleada 3): el protocolo del servicio `avisos` (las notificaciones) y su historial. Lo usan el servicio, la
//! barra (que los enseña), el panel (no molestar) y los programas que avisan. Sin `alloc` ni nada de DashOS, para
//! probarlo en el PC tal cual: `rustc --edition 2024 --test usuario/src/protocolo/avisos.rs`.
//!
//! El modelo es el de DMS (DankMaterialShell, `Services/NotificationService.qml`, MIT, copyright (c) 2025 Avenge Media
//! LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`): un aviso es "actual" hasta que se descarta y
//! sigue en el historial hasta que se quita; puede traer acciones (la de identificador `default` es la del clic en el
//! cuerpo); los críticos no caducan; "no molestar" es estado de la sesión, con plazo o sin él.
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
//! - `AVISAR_CON`: lo mismo con dos u32 más tras los largos (el número del aviso suyo que sustituye, 0 si es nuevo, y
//!   cuántas acciones trae, hasta `MAX_ACCIONES`) y, tras el texto, cada acción: largo del identificador (u8, 1 a
//!   `MAX_ID`, solo letras ASCII, cifras, `_`, `-` y `.`, sin repetir), largo de la etiqueta (u8, 1 a `MAX_ETIQUETA`), el
//!   identificador y la etiqueta. Una segunda llave, si viene, es por donde el programa quiere oír las acciones pulsadas
//!   (`ACCION_PULSADA`: orden, número del aviso, largo del identificador y el identificador). Solo se sustituye un aviso
//!   propio: el de otro programa, `PROTEGIDO`.
//! - Solo de la barra (el nombre `barra` que da `inicio`): `SUSCRIBIR` (con una segunda llave, por la que el servicio
//!   toca el timbre: `TIMBRE`, uno solo pendiente hasta que la barra pida algo), `LEER` (el aviso visible más nuevo con
//!   número menor que el que se diga, `u32::MAX` para el más nuevo; un tercer u32 a 1 pide solo los actuales), `LEIDOS`
//!   (marca como leídos todos los visibles), `BORRAR_TODO` (quita los visibles, también del historial), `DESCARTAR_TODO`
//!   (los visibles dejan de ser actuales), `DESCARTAR` y `QUITAR` (uno, por su número), `ACCION` (número e índice de la
//!   acción, o `POR_DEFECTO`), `LEER_ACCIONES` (las etiquetas de un aviso) y `LEER_AJUSTES`. Contestan `CABECERA_LEER`
//!   bytes (estado, sin leer, visibles y la versión de los ajustes y del no molestar, u32) y lo que pidan.
//! - De la barra y del panel: `NO_MOLESTAR` (modo 0 o 1 y minutos, 0 sin plazo) y `ESTADO` (contesta la cabecera, el
//!   modo y los segundos que quedan, 0 sin plazo).
//! - Solo de la barra (a quien se los da el centro de control, que lee `ENV:Escritorio/avisos`): `AJUSTES` (largo y los
//!   bytes de `avisos_ajustes::Ajustes::escribir`). El servicio solo usa lo del historial; sube la versión y toca el
//!   timbre (la barra ya los tiene).
//!
//! Visibles: los del sistema y los de las personas que no tienen la sesión detrás (U6). Los de una sesión de detrás se
//! guardan y salen cuando su dueña vuelve delante.

/// Órdenes.
pub const AVISAR: u32 = 1;
pub const AVISAR_CON: u32 = 2;
pub const SUSCRIBIR: u32 = 10;
pub const LEER: u32 = 11;
pub const LEIDOS: u32 = 12;
pub const BORRAR_TODO: u32 = 13;
pub const ACCION: u32 = 14;
pub const LEER_ACCIONES: u32 = 15;
pub const DESCARTAR: u32 = 16;
pub const QUITAR: u32 = 17;
pub const DESCARTAR_TODO: u32 = 18;
pub const NO_MOLESTAR: u32 = 19;
pub const ESTADO: u32 = 20;
pub const AJUSTES: u32 = 21;
pub const LEER_AJUSTES: u32 = 22;

/// Lo que manda el servicio por la llave del timbre: hay algo nuevo que enseñar.
pub const TIMBRE: u32 = 0x4156_4953;
/// Lo que manda el servicio al programa que avisó cuando se pulsa una de sus acciones.
pub const ACCION_PULSADA: u32 = 0x4143_4349;
/// En `ACCION`, en vez del índice: la acción del clic en el cuerpo (la de identificador `POR_DEFECTO_ID`).
pub const POR_DEFECTO: u32 = u32::MAX;
pub const POR_DEFECTO_ID: &str = "default";

/// Urgencias.
pub const BAJA: u8 = 0;
pub const NORMAL: u8 = 1;
pub const URGENTE: u8 = 2;

/// Estados.
pub const BIEN: i32 = 0;
/// El mensaje no se entiende (largos que no cuadran, urgencia que no existe, sin título, acciones mal hechas).
pub const MAL: i32 = -1;
/// No le toca (lo de la barra, a otro; sustituir el aviso de otro programa).
pub const PROTEGIDO: i32 = -2;
/// Demasiados avisos seguidos de ese programa (`RITMO` por `VENTANA_RITMO_US`).
pub const DEMASIADOS: i32 = -3;
/// `LEER`: no hay más. `ACCION`, `DESCARTAR`...: no hay ese aviso, o nadie oye sus acciones.
pub const NO_HAY: i32 = -4;

/// Lo más largo de un título y de un texto, en bytes de UTF-8.
pub const MAX_TITULO: usize = 48;
pub const MAX_TEXTO: usize = 160;
/// Las acciones de un aviso: cuántas, y lo más largo del identificador y de la etiqueta.
pub const MAX_ACCIONES: usize = 4;
pub const MAX_ID: usize = 16;
pub const MAX_ETIQUETA: usize = 24;
/// La cabecera de `AVISAR` y la de `AVISAR_CON`.
pub const CABECERA_AVISAR: usize = 16;
pub const CABECERA_AVISAR_CON: usize = 24;
pub const RESPUESTA_AVISAR: usize = 24;
/// La cabecera de las respuestas a la barra y al panel: estado, sin leer, visibles y versión.
pub const CABECERA_LEER: usize = 16;
/// Lo fijo de un aviso al leerlo: número, urgencia, banderas, largos, hora (s), a quién sustituye y de quién.
pub const FIJO_AVISO: usize = 32;
/// Banderas de un aviso al leerlo.
pub const LEIDO: u8 = 1;
pub const SISTEMA: u8 = 2;
/// Se guardó con la sesión de su dueña detrás (U6): sale en el historial, no como aviso nuevo.
pub const RETENIDO: u8 = 4;
/// Sigue siendo actual (no se ha descartado).
pub const ACTUAL: u8 = 8;
/// Trae acciones con botón (se piden con `LEER_ACCIONES`).
pub const CON_ACCIONES: u8 = 16;
/// Trae la acción del clic en el cuerpo.
pub const CON_POR_DEFECTO: u8 = 32;
/// Lo más largo de los ajustes que guarda el servicio para la barra.
pub const MAX_AJUSTES: usize = 128;

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

/// Lo que cabe de `s` en `max` bytes, cortado en una letra entera.
fn corta(s: &str, max: usize) -> usize {
    let mut l = s.len().min(max);
    while !s.is_char_boundary(l) {
        l -= 1;
    }
    l
}

/// Escribe en `m` la petición `AVISAR`. Lo que no quepa del título o del texto se corta (en una letra entera). Devuelve
/// el largo del mensaje.
pub fn pedir_aviso(m: &mut [u8; 256], urgencia: u8, titulo: &str, texto: &str) -> usize {
    let (lt, lx) = (corta(titulo, MAX_TITULO), corta(texto, MAX_TEXTO));
    m[0..4].copy_from_slice(&AVISAR.to_le_bytes());
    m[4..8].copy_from_slice(&(urgencia as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(lt as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(lx as u32).to_le_bytes());
    m[16..16 + lt].copy_from_slice(&titulo.as_bytes()[..lt]);
    m[16 + lt..16 + lt + lx].copy_from_slice(&texto.as_bytes()[..lx]);
    CABECERA_AVISAR + lt + lx
}

/// Si `id` vale como identificador de una acción: de 1 a `MAX_ID` bytes, letras ASCII, cifras, `_`, `-` y `.`.
pub fn id_valido(id: &[u8]) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

/// Escribe en `m` la petición `AVISAR_CON`: con las acciones (identificador y etiqueta) y el número del aviso propio que
/// sustituye (0 si es nuevo). El título se corta como en `AVISAR`; el texto, lo que haga falta para que quepa todo.
/// `None` si las acciones no valen (demasiadas, identificadores malos o repetidos, etiquetas vacías o largas).
pub fn pedir_aviso_con(m: &mut [u8; 256], urgencia: u8, titulo: &str, texto: &str, reemplaza: u32, acciones: &[(&str, &str)]) -> Option<usize> {
    if acciones.len() > MAX_ACCIONES {
        return None;
    }
    let mut largo_acciones = 0;
    for (i, (id, etiqueta)) in acciones.iter().enumerate() {
        if !id_valido(id.as_bytes()) || etiqueta.is_empty() || etiqueta.len() > MAX_ETIQUETA || acciones[..i].iter().any(|x| x.0 == *id) {
            return None;
        }
        largo_acciones += 2 + id.len() + etiqueta.len();
    }
    let lt = corta(titulo, MAX_TITULO);
    let lx = corta(texto, MAX_TEXTO.min(256 - CABECERA_AVISAR_CON - lt - largo_acciones));
    m[0..4].copy_from_slice(&AVISAR_CON.to_le_bytes());
    m[4..8].copy_from_slice(&(urgencia as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(lt as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(lx as u32).to_le_bytes());
    m[16..20].copy_from_slice(&reemplaza.to_le_bytes());
    m[20..24].copy_from_slice(&(acciones.len() as u32).to_le_bytes());
    let mut n = CABECERA_AVISAR_CON;
    m[n..n + lt].copy_from_slice(&titulo.as_bytes()[..lt]);
    n += lt;
    m[n..n + lx].copy_from_slice(&texto.as_bytes()[..lx]);
    n += lx;
    for (id, etiqueta) in acciones {
        m[n] = id.len() as u8;
        m[n + 1] = etiqueta.len() as u8;
        n += 2;
        m[n..n + id.len()].copy_from_slice(id.as_bytes());
        n += id.len();
        m[n..n + etiqueta.len()].copy_from_slice(etiqueta.as_bytes());
        n += etiqueta.len();
    }
    Some(n)
}

/// Una petición de aviso leída (aún sin limpiar).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pedido<'a> {
    pub urgencia: u8,
    pub titulo: &'a [u8],
    pub texto: &'a [u8],
    pub reemplaza: u32,
    /// Identificador y etiqueta de cada acción.
    pub acciones: [(&'a [u8], &'a [u8]); MAX_ACCIONES],
    pub nacc: usize,
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

/// Lee una petición `AVISAR` o `AVISAR_CON`. `MAL` si algo no cuadra: además de lo de `leer_aviso`, más de
/// `MAX_ACCIONES`, un identificador que no vale o repetido, una etiqueta vacía o larga, o bytes de más o de menos.
pub fn leer_pedido(m: &[u8]) -> Result<Pedido<'_>, i32> {
    if u32_en(m, 0) == Some(AVISAR) {
        let (urgencia, titulo, texto) = leer_aviso(m)?;
        return Ok(Pedido { urgencia, titulo, texto, reemplaza: 0, acciones: [(&[], &[]); MAX_ACCIONES], nacc: 0 });
    }
    let (Some(AVISAR_CON), Some(u), Some(lt), Some(lx), Some(reemplaza), Some(nacc)) = (u32_en(m, 0), u32_en(m, 4), u32_en(m, 8), u32_en(m, 12), u32_en(m, 16), u32_en(m, 20)) else {
        return Err(MAL);
    };
    let (lt, lx, nacc) = (lt as usize, lx as usize, nacc as usize);
    if u > URGENTE as u32 || lt == 0 || lt > MAX_TITULO || lx > MAX_TEXTO || nacc > MAX_ACCIONES || m.len() < CABECERA_AVISAR_CON + lt + lx {
        return Err(MAL);
    }
    let mut p = Pedido { urgencia: u as u8, titulo: &m[24..24 + lt], texto: &m[24 + lt..24 + lt + lx], reemplaza, acciones: [(&[], &[]); MAX_ACCIONES], nacc };
    let mut i = CABECERA_AVISAR_CON + lt + lx;
    for k in 0..nacc {
        let (Some(&li), Some(&le)) = (m.get(i), m.get(i + 1)) else { return Err(MAL) };
        let (li, le) = (li as usize, le as usize);
        let Some(id) = m.get(i + 2..i + 2 + li) else { return Err(MAL) };
        let Some(etiqueta) = m.get(i + 2 + li..i + 2 + li + le) else { return Err(MAL) };
        if !id_valido(id) || le == 0 || le > MAX_ETIQUETA || p.acciones[..k].iter().any(|x| x.0 == id) {
            return Err(MAL);
        }
        p.acciones[k] = (id, etiqueta);
        i += 2 + li + le;
    }
    if i != m.len() {
        return Err(MAL);
    }
    Ok(p)
}

/// Un nombre de 16 bytes, rellenos de ceros, como texto (lo que no sea UTF-8, "?").
pub fn nombre16(n: &[u8; 16]) -> &str {
    core::str::from_utf8(&n[..n.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

/// Una acción guardada: su identificador (sin limpiar: ya se comprobó) y su etiqueta (limpia).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accion {
    pub li: u8,
    pub id: [u8; MAX_ID],
    pub le: u8,
    pub etiqueta: [u8; MAX_ETIQUETA],
}

impl Accion {
    pub const fn vacia() -> Accion {
        Accion { li: 0, id: [0; MAX_ID], le: 0, etiqueta: [0; MAX_ETIQUETA] }
    }

    pub fn id(&self) -> &str {
        core::str::from_utf8(&self.id[..self.li as usize]).unwrap_or("?")
    }

    pub fn etiqueta(&self) -> &str {
        core::str::from_utf8(&self.etiqueta[..self.le as usize]).unwrap_or("?")
    }

    /// Si es la del clic en el cuerpo (no lleva botón).
    pub fn por_defecto(&self) -> bool {
        self.id() == POR_DEFECTO_ID
    }
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
    /// Si sigue entre los actuales (no se ha descartado). Descartado, sigue en el historial.
    pub actual: bool,
    /// El número del aviso suyo que sustituyó (0 si es nuevo): la barra cambia ese emergente en su sitio.
    pub reemplaza: u32,
    /// De quién: su marca, su usuario (por debajo de `PRIMERA_PERSONA`, del sistema) y su nombre.
    pub marca: u64,
    pub uid: u32,
    pub de: [u8; 16],
    /// La hora del calendario (ns), si se sabe; si no, 0. Al leerla, va en segundos.
    pub hora: u64,
    pub lt: u8,
    pub lx: u8,
    pub titulo: [u8; MAX_TITULO],
    pub texto: [u8; MAX_TEXTO],
    pub nacc: u8,
    pub acciones: [Accion; MAX_ACCIONES],
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

    /// Las acciones que trae.
    pub fn acciones(&self) -> &[Accion] {
        &self.acciones[..(self.nacc as usize).min(MAX_ACCIONES)]
    }

    /// Si trae acciones con botón (las que no son la del clic en el cuerpo).
    pub fn con_botones(&self) -> bool {
        self.acciones().iter().any(|a| !a.por_defecto())
    }

    /// El índice de la acción del clic en el cuerpo, si la trae.
    pub fn por_defecto(&self) -> Option<usize> {
        self.acciones().iter().position(|a| a.por_defecto())
    }

    /// Pone las acciones de un pedido (las etiquetas, limpias).
    pub fn poner_acciones(&mut self, p: &Pedido) {
        self.nacc = p.nacc.min(MAX_ACCIONES) as u8;
        for (k, (id, etiqueta)) in p.acciones[..self.nacc as usize].iter().enumerate() {
            let mut a = Accion::vacia();
            a.li = id.len().min(MAX_ID) as u8;
            a.id[..a.li as usize].copy_from_slice(&id[..a.li as usize]);
            a.le = limpiar(etiqueta, &mut a.etiqueta) as u8;
            self.acciones[k] = a;
        }
    }

    fn banderas(&self) -> u8 {
        (self.leido as u8 * LEIDO) | (self.sistema as u8 * SISTEMA) | (self.retenido as u8 * RETENIDO) | (self.actual as u8 * ACTUAL)
            | (self.con_botones() as u8 * CON_ACCIONES) | (self.por_defecto().is_some() as u8 * CON_POR_DEFECTO)
    }

    /// Lo escribe en `m` como lo lee la barra (sin las acciones, que van aparte). Devuelve cuántos bytes.
    pub fn escribir(&self, m: &mut [u8]) -> usize {
        let (lt, lx) = (self.lt as usize, self.lx as usize);
        m[0..4].copy_from_slice(&self.numero.to_le_bytes());
        m[4] = self.urgencia;
        m[5] = self.banderas();
        m[6] = self.lt;
        m[7] = self.lx;
        m[8..12].copy_from_slice(&((self.hora / 1_000_000_000).min(u32::MAX as u64) as u32).to_le_bytes());
        m[12..16].copy_from_slice(&self.reemplaza.to_le_bytes());
        m[16..32].copy_from_slice(&self.de);
        m[32..32 + lt].copy_from_slice(&self.titulo[..lt]);
        m[32 + lt..32 + lt + lx].copy_from_slice(&self.texto[..lx]);
        FIJO_AVISO + lt + lx
    }

    /// Lo lee como lo escribe `escribir`, desconfiando: largos que no cuadran, fuera; los textos, otra vez limpios. Las
    /// acciones no vienen: con `CON_ACCIONES` o `CON_POR_DEFECTO`, `nacc` dice cuántas hay que pedir (1, sin saber
    /// cuántas: las pone `leer_acciones`).
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
        a.actual = m[5] & ACTUAL != 0;
        a.nacc = (m[5] & (CON_ACCIONES | CON_POR_DEFECTO) != 0) as u8;
        a.hora = u32_en(m, 8)? as u64 * 1_000_000_000;
        a.reemplaza = u32_en(m, 12)?;
        let mut de = [0u8; 16];
        let n = limpiar(m[16..32].split(|&c| c == 0).next().unwrap_or(b""), &mut de);
        de[n..].fill(0);
        a.de = de;
        a.lt = limpiar(&m[32..32 + lt], &mut a.titulo) as u8;
        a.lx = limpiar(&m[32 + lt..32 + lt + lx], &mut a.texto) as u8;
        Some(a)
    }

    /// Las acciones, como las lee la barra (`LEER_ACCIONES`): cuántas y, de cada una, largos, identificador y etiqueta.
    pub fn escribir_acciones(&self, m: &mut [u8]) -> usize {
        m[0] = self.nacc;
        let mut n = 1;
        for a in self.acciones() {
            m[n] = a.li;
            m[n + 1] = a.le;
            n += 2;
            m[n..n + a.li as usize].copy_from_slice(&a.id[..a.li as usize]);
            n += a.li as usize;
            m[n..n + a.le as usize].copy_from_slice(&a.etiqueta[..a.le as usize]);
            n += a.le as usize;
        }
        n
    }

    /// Pone las acciones leídas de `m` (como las escribe `escribir_acciones`), desconfiando: lo que no cuadra deja el
    /// aviso sin acciones y da `false`.
    pub fn leer_acciones(&mut self, m: &[u8]) -> bool {
        self.nacc = 0;
        let Some(&n) = m.first() else { return false };
        if n as usize > MAX_ACCIONES {
            return false;
        }
        let mut acciones = [Accion::vacia(); MAX_ACCIONES];
        let mut i = 1;
        for a in acciones.iter_mut().take(n as usize) {
            let (Some(&li), Some(&le)) = (m.get(i), m.get(i + 1)) else { return false };
            let (Some(id), Some(etiqueta)) = (m.get(i + 2..i + 2 + li as usize), m.get(i + 2 + li as usize..i + 2 + li as usize + le as usize)) else {
                return false;
            };
            if !id_valido(id) || le == 0 || le as usize > MAX_ETIQUETA {
                return false;
            }
            a.li = li;
            a.id[..li as usize].copy_from_slice(id);
            a.le = limpiar(etiqueta, &mut a.etiqueta) as u8;
            i += 2 + li as usize + le as usize;
        }
        if i != m.len() {
            return false;
        }
        self.acciones = acciones;
        self.nacc = n;
        true
    }

    pub const fn vacio() -> Aviso {
        Aviso {
            numero: 0,
            urgencia: 0,
            leido: false,
            sistema: false,
            retenido: false,
            actual: true,
            reemplaza: 0,
            marca: 0,
            uid: 0,
            de: [0; 16],
            hora: 0,
            lt: 0,
            lx: 0,
            titulo: [0; MAX_TITULO],
            texto: [0; MAX_TEXTO],
            nacc: 0,
            acciones: [Accion::vacia(); MAX_ACCIONES],
        }
    }
}

/// Lo que se manda al programa que avisó cuando se pulsa una acción: orden, número, largo del identificador y el
/// identificador. Devuelve cuántos bytes.
pub fn escribir_pulsada(numero: u32, a: &Accion, m: &mut [u8]) -> usize {
    m[0..4].copy_from_slice(&ACCION_PULSADA.to_le_bytes());
    m[4..8].copy_from_slice(&numero.to_le_bytes());
    m[8..12].copy_from_slice(&(a.li as u32).to_le_bytes());
    m[12..12 + a.li as usize].copy_from_slice(&a.id[..a.li as usize]);
    12 + a.li as usize
}

/// Lee una `ACCION_PULSADA`: número del aviso e identificador. `None` si no tiene esa forma.
pub fn leer_pulsada(m: &[u8]) -> Option<(u32, &str)> {
    let (Some(ACCION_PULSADA), Some(numero), Some(li)) = (u32_en(m, 0), u32_en(m, 4), u32_en(m, 8)) else { return None };
    let id = m.get(12..12 + li as usize)?;
    (m.len() == 12 + li as usize && id_valido(id)).then(|| (numero, core::str::from_utf8(id).unwrap_or("?")))
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

    /// Guarda un aviso (de `a` se usa todo menos el número, que pone el historial, `leido` y `actual`). Si ese programa
    /// ya tiene `POR_PROGRAMA`, se va el más viejo suyo; si el historial está lleno, el más viejo de todos. Devuelve su
    /// número.
    pub fn meter(&mut self, mut a: Aviso) -> u32 {
        a.numero = self.siguiente;
        a.leido = false;
        a.actual = true;
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

    /// Como `meter`, sustituyendo el aviso `reemplaza` si es de la misma marca (se va, y el nuevo lleva su número en
    /// `reemplaza`). Si es de otra marca, `PROTEGIDO` y no se guarda nada. Si ya no está (o es 0), es uno nuevo.
    pub fn meter_sustituyendo(&mut self, mut a: Aviso, reemplaza: u32) -> Result<u32, i32> {
        a.reemplaza = 0;
        if reemplaza != 0 && let Some(i) = self.avisos.iter().position(|x| x.is_some_and(|x| x.numero == reemplaza)) {
            if self.avisos[i].unwrap().marca != a.marca {
                return Err(PROTEGIDO);
            }
            self.avisos[i] = None;
            a.reemplaza = reemplaza;
        }
        Ok(self.meter(a))
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

    /// El visible más nuevo con número menor que `antes` (con `solo_actuales`, de los que no se han descartado).
    pub fn anterior(&self, antes: u32, detras: &[u32], solo_actuales: bool) -> Option<Aviso> {
        self.visibles(detras).find(|a| a.numero < antes && (a.actual || !solo_actuales)).copied()
    }

    /// El aviso visible con ese número.
    pub fn buscar(&self, numero: u32, detras: &[u32]) -> Option<&Aviso> {
        self.avisos.iter().flatten().find(|a| a.numero == numero && visible(a.uid, detras))
    }

    fn buscar_mut(&mut self, numero: u32, detras: &[u32]) -> Option<&mut Aviso> {
        self.avisos.iter_mut().flatten().find(|a| a.numero == numero && visible(a.uid, detras))
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

    /// Descarta uno visible: deja de ser actual (y se da por leído), y sigue en el historial. Si estaba.
    pub fn descartar(&mut self, numero: u32, detras: &[u32]) -> bool {
        self.buscar_mut(numero, detras).map(|a| {
            a.actual = false;
            a.leido = true;
        }).is_some()
    }

    /// Descarta todos los visibles.
    pub fn descartar_visibles(&mut self, detras: &[u32]) {
        for a in self.avisos.iter_mut().flatten().filter(|a| visible(a.uid, detras)) {
            a.actual = false;
            a.leido = true;
        }
    }

    /// Quita uno visible, también del historial. Si estaba.
    pub fn quitar(&mut self, numero: u32, detras: &[u32]) -> bool {
        match self.avisos.iter().position(|x| x.is_some_and(|a| a.numero == numero && visible(a.uid, detras))) {
            Some(i) => {
                self.avisos[i] = None;
                true
            }
            None => false,
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

    /// Quita los descartados con hora que llevan más de `dias` (como `notificationHistoryMaxAgeDays` de DMS; 0, nunca).
    /// Los actuales no se tocan. Devuelve cuántos.
    pub fn caducar(&mut self, ahora_ns: u64, dias: u32) -> usize {
        if dias == 0 || ahora_ns == 0 {
            return 0;
        }
        let tope = dias as u64 * 86_400 * 1_000_000_000;
        let mut n = 0;
        for x in self.avisos.iter_mut() {
            if x.is_some_and(|a| !a.actual && a.hora != 0 && ahora_ns.saturating_sub(a.hora) > tope) {
                *x = None;
                n += 1;
            }
        }
        n
    }

    /// Cuántos hay de `uid` (los de una sesión de detrás, guardados).
    pub fn de_usuario(&self, uid: u32) -> usize {
        self.avisos.iter().flatten().filter(|a| a.uid == uid).count()
    }

    /// Cuántos hay de `marca` (para saber si aún hace falta su llave de acciones).
    pub fn de_marca(&self, marca: u64) -> usize {
        self.avisos.iter().flatten().filter(|a| a.marca == marca).count()
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

/// "No molestar" (como `doNotDisturb` y `doNotDisturbUntil` de DMS): estado de la sesión, puesto a mano, con plazo o sin
/// él. El plazo va en el reloj que no salta (µs desde el arranque): no depende de que se sepa la hora.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NoMolestar {
    pub puesto: bool,
    /// Hasta cuándo (µs desde el arranque); 0, hasta que se quite.
    pub hasta_us: u64,
}

impl NoMolestar {
    /// Lo pone (con `minutos` de plazo, 0 sin plazo) o lo quita.
    pub fn poner(&mut self, puesto: bool, minutos: u32, ahora_us: u64) {
        self.puesto = puesto;
        self.hasta_us = if puesto && minutos > 0 { ahora_us.saturating_add(minutos as u64 * 60_000_000) } else { 0 };
    }

    /// Si está puesto ahora (con el plazo vencido, se quita: devuelve `false` y lo deja así).
    pub fn activo(&mut self, ahora_us: u64) -> bool {
        if self.puesto && self.hasta_us != 0 && ahora_us >= self.hasta_us {
            *self = NoMolestar::default();
        }
        self.puesto
    }

    /// Los segundos que quedan (0 sin plazo o sin poner).
    pub fn quedan_s(&self, ahora_us: u64) -> u32 {
        if !self.puesto || self.hasta_us == 0 {
            return 0;
        }
        (self.hasta_us.saturating_sub(ahora_us).div_ceil(1_000_000)).min(u32::MAX as u64) as u32
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
        a.hora = 123_000_000_000;
        a.reemplaza = 5;
        a.de[..7].copy_from_slice(b"archivo");
        a.lx = limpiar(b"Queda poco", &mut a.texto) as u8;
        let mut m = [0u8; 256];
        let n = a.escribir(&mut m);
        assert!(CABECERA_LEER + n <= 256);
        let b = Aviso::leer(&m[..n]).unwrap();
        assert_eq!((b.numero, b.urgencia, b.sistema, b.retenido, b.leido, b.actual, b.hora, b.reemplaza, b.de(), b.titulo(), b.texto()), (7, URGENTE, true, true, false, true, 123_000_000_000, 5, "archivo", "Disco lleno", "Queda poco"));
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
        assert_eq!(h.anterior(u32::MAX, &[], false).unwrap().numero, mas_nuevo);
        // `anterior` recorre del más nuevo al más viejo.
        let mut antes = u32::MAX;
        let mut vistos = 0;
        while let Some(a) = h.anterior(antes, &[], false) {
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
        assert_eq!(h.anterior(u32::MAX, &[1001], false).unwrap().titulo(), "de Ana");
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

    #[test]
    fn avisar_con_acciones() {
        let mut m = [0u8; 256];
        let n = pedir_aviso_con(&mut m, NORMAL, "Descarga acabada", "informe.pdf", 0, &[("default", "Abrir"), ("carpeta", "Ver en la carpeta")]).unwrap();
        let p = leer_pedido(&m[..n]).unwrap();
        assert_eq!((p.urgencia, p.titulo, p.texto, p.reemplaza, p.nacc), (NORMAL, &b"Descarga acabada"[..], &b"informe.pdf"[..], 0, 2));
        assert_eq!(p.acciones[1], (&b"carpeta"[..], &b"Ver en la carpeta"[..]));
        let mut a = Aviso::vacio();
        a.poner_acciones(&p);
        assert_eq!((a.con_botones(), a.por_defecto()), (true, Some(0)));
        // Las acciones, por su orden, ida y vuelta.
        let mut r = [0u8; 256];
        let k = a.escribir_acciones(&mut r);
        let mut b = Aviso::vacio();
        assert!(b.leer_acciones(&r[..k]));
        assert_eq!((b.nacc, b.acciones()[0].id(), b.acciones()[1].etiqueta()), (2, "default", "Ver en la carpeta"));
        // Con el texto más largo que quepa y cuatro acciones largas, el texto se corta para que todo quepa en 256.
        let largo = "ñ".repeat(200);
        let acc = [("a1234567890abcde", "etiqueta muy larga 01234"), ("b1234567890abcde", "etiqueta muy larga 01234"), ("c1", "x"), ("d1", "y")];
        let n = pedir_aviso_con(&mut m, URGENTE, &largo, &largo, 9, &acc).unwrap();
        assert!(n <= 256);
        let p = leer_pedido(&m[..n]).unwrap();
        assert!(core::str::from_utf8(p.texto).is_ok() && p.nacc == 4 && p.reemplaza == 9);
        // AVISAR de siempre también se lee como pedido, sin acciones.
        let n = pedir_aviso(&mut m, BAJA, "t", "x");
        assert_eq!(leer_pedido(&m[..n]).unwrap().nacc, 0);
    }

    #[test]
    fn acciones_falsas() {
        let mut m = [0u8; 256];
        // Lo que el cliente ni siquiera escribe: demasiadas, identificadores malos o repetidos, etiquetas vacías o largas.
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4"), ("e", "5")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("con espacio", "x")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("", "x")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("ñ", "x")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("a", "x"), ("a", "y")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("a", "")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("a", "1234567890123456789012345")]).is_none());
        assert!(pedir_aviso_con(&mut m, NORMAL, "t", "", 0, &[("a12345678901234567", "x")]).is_none());
        // Lo que un programa hostil escribe a mano: el servicio lo rechaza.
        let n = pedir_aviso_con(&mut m, NORMAL, "t", "texto", 0, &[("ok", "Vale")]).unwrap();
        let base = m;
        let malo = |f: &dyn Fn(&mut [u8; 256]) -> usize| {
            let mut x = base;
            let n = f(&mut x);
            leer_pedido(&x[..n]) == Err(MAL)
        };
        assert!(malo(&|x| { x[20..24].copy_from_slice(&5u32.to_le_bytes()); n }));
        assert!(malo(&|x| { x[20..24].copy_from_slice(&u32::MAX.to_le_bytes()); n }));
        assert!(malo(&|x| { x[20..24].copy_from_slice(&2u32.to_le_bytes()); n }));
        assert!(malo(&|_| n - 1));
        assert!(malo(&|_| n + 1));
        // El largo del identificador se sale del mensaje; la etiqueta, a cero; el identificador con un byte de control.
        let i = CABECERA_AVISAR_CON + 1 + 5;
        assert!(malo(&|x| { x[i] = 200; n }));
        assert!(malo(&|x| { x[i + 1] = 0; n }));
        assert!(malo(&|x| { x[i + 2] = 0x07; n }));
        // La etiqueta con bytes malos sí vale: se guarda limpia.
        let mut x = base;
        x[i + 4] = 0xff;
        let p = leer_pedido(&x[..n]).unwrap();
        let mut a = Aviso::vacio();
        a.poner_acciones(&p);
        assert_eq!(a.acciones()[0].etiqueta(), "?ale");
        // Leer acciones que no cuadran deja el aviso sin ellas.
        let mut b = Aviso::vacio();
        assert!(!b.leer_acciones(&[9]) && b.nacc == 0);
        assert!(!b.leer_acciones(&[1, 3, 1, b'a', b'b', b'c', b'x', 0]) && b.nacc == 0);
        assert!(!b.leer_acciones(&[]) && b.nacc == 0);
    }

    #[test]
    fn sustituir_solo_lo_propio() {
        let mut h = Historial::new();
        let uno = h.meter(aviso(1, 1, "copiando 10 %"));
        let otro = h.meter(aviso(2, 1, "de otro"));
        // El mismo programa sustituye el suyo: se va el viejo, el nuevo dice a cuál sustituye.
        let dos = h.meter_sustituyendo(aviso(1, 1, "copiando 50 %"), uno).unwrap();
        assert!(h.buscar(uno, &[]).is_none());
        assert_eq!(h.buscar(dos, &[]).unwrap().reemplaza, uno);
        // Otro programa no puede sustituirlo, ni el de un tercero: nada cambia.
        assert_eq!(h.meter_sustituyendo(aviso(2, 1, "secuestro"), dos), Err(PROTEGIDO));
        assert_eq!(h.meter_sustituyendo(aviso(3, 1, "secuestro"), otro), Err(PROTEGIDO));
        assert_eq!(h.cuenta(&[]).0, 2);
        // Uno que ya no está: es nuevo.
        let tres = h.meter_sustituyendo(aviso(1, 1, "hecho"), 999).unwrap();
        assert_eq!(h.buscar(tres, &[]).unwrap().reemplaza, 0);
    }

    #[test]
    fn actuales_e_historial() {
        let mut h = Historial::new();
        let a = h.meter(aviso(1, 1, "a"));
        let b = h.meter(aviso(1, 1, "b"));
        let c = h.meter(aviso(0x1_0000, 1000, "de Ana"));
        assert!(h.descartar(b, &[]));
        // Descartado: fuera de los actuales, dentro del historial.
        assert_eq!(h.anterior(u32::MAX, &[], true).unwrap().numero, c);
        assert_eq!(h.anterior(c, &[], true).unwrap().numero, a);
        assert_eq!(h.anterior(c, &[], false).unwrap().numero, b);
        // No se descarta ni se quita lo que no se ve.
        assert!(!h.descartar(c, &[1000]) && !h.quitar(c, &[1000]) && !h.descartar(999, &[]));
        assert!(h.quitar(a, &[]));
        assert_eq!(h.cuenta(&[]), (2, 1));
        h.descartar_visibles(&[]);
        assert!(h.anterior(u32::MAX, &[], true).is_none());
        // Caducar: solo los descartados con hora, de más de esos días.
        let mut h = Historial::new();
        let mut viejo = aviso(1, 1, "viejo");
        viejo.hora = 1_000_000_000;
        let v = h.meter(viejo);
        let mut viejo_actual = aviso(1, 1, "viejo pero actual");
        viejo_actual.hora = 1_000_000_000;
        h.meter(viejo_actual);
        h.meter(aviso(1, 1, "sin hora"));
        h.descartar(v, &[]);
        let ocho_dias = 8 * 86_400 * 1_000_000_000u64;
        assert_eq!(h.caducar(ocho_dias, 0), 0);
        assert_eq!(h.caducar(ocho_dias, 7), 1);
        assert_eq!(h.cuenta(&[]).0, 2);
    }

    #[test]
    fn pulsada_ida_y_vuelta() {
        let mut a = Accion::vacia();
        a.li = 7;
        a.id[..7].copy_from_slice(b"carpeta");
        let mut m = [0u8; 64];
        let n = escribir_pulsada(42, &a, &mut m);
        assert_eq!(leer_pulsada(&m[..n]), Some((42, "carpeta")));
        assert_eq!(leer_pulsada(&m[..n - 1]), None);
        let mut x = m;
        x[8] = 60;
        assert_eq!(leer_pulsada(&x[..n]), None);
    }

    #[test]
    fn no_molestar_con_plazo() {
        let mut d = NoMolestar::default();
        assert!(!d.activo(0));
        d.poner(true, 0, 1000);
        assert!(d.activo(u64::MAX / 2) && d.quedan_s(5) == 0);
        d.poner(true, 15, 1_000_000);
        assert_eq!(d.quedan_s(1_000_000), 900);
        assert!(d.activo(1_000_000 + 899_000_000));
        assert!(!d.activo(1_000_000 + 900_000_000));
        // Vencido se queda quitado.
        assert!(!d.activo(0) && !d.puesto);
        // Minutos enormes no desbordan.
        d.poner(true, u32::MAX, u64::MAX - 5);
        assert!(d.activo(u64::MAX - 1));
    }
}
