//! Las cuentas de usuario (S6b, "Dónde se guardan las cuentas"): `Estado:Cuentas/usuarios` y `Estado:Cuentas/grupos`,
//! en texto. Solo las escribe `sesiones`; los demás las leen.
//!
//! - `usuarios`: una línea por cuenta, `nombre:uid:gid:nombre completo:volumen:estado`. El volumen es el cifrado de la
//!   persona (su `Home:`), que abre su contraseña: **no hay fichero de contraseñas**. El estado es `activa` o
//!   `retirada <segundos desde 1970>`: una cuenta borrada queda retirada unos días (`PLAZO_RETIRADA_S`), con su volumen
//!   cerrado, y se puede devolver; pasado el plazo se borra de verdad.
//! - `grupos`: `nombre:gid:miembros`, con los miembros por su nombre, separados por comas. `admin` (gid 10) es el de los
//!   administradores. El grupo propio de cada persona (su mismo número) no se escribe: es su gid.
//!
//! Lo que no se entiende no tumba nada: una línea que no vale (campos de más o de menos, un nombre con letras raras, un
//! número repetido, una línea cortada al final) se salta y se cuenta, y las buenas se quedan. Así un fichero dañado o
//! truncado deja entrar a quien sigue bien escrito, y `sesiones` lo dice.
//!
//! Sin `alloc` ni nada del sistema: se prueba en el PC con `rustc --edition 2024 --test usuario/src/cuentas.rs`.

/// El cajón, desde la raíz del disco (`Estado:Cuentas`), y sus dos ficheros.
pub const CAJON: &str = "Estado/Cuentas";
pub const USUARIOS: &str = "usuarios";
pub const GRUPOS: &str = "grupos";
/// Lo que lleva delante cada fichero: qué es y cómo se cambia.
pub const CABECERA_USUARIOS: &str = "# Cuentas de usuario: nombre:uid:gid:nombre completo:volumen:estado\n\
# Lo escribe el sistema (sesiones). Cambiarlo a mano no vale: la orden Usuario del Shell.\n";
pub const CABECERA_GRUPOS: &str = "# Grupos: nombre:gid:miembros (separados por comas). admin, los administradores.\n\
# Lo escribe el sistema (sesiones). Cambiarlo a mano no vale: la orden Usuario del Shell.\n";
/// Cuántas cuentas y grupos caben, y cuántos miembros por grupo.
pub const MAX_CUENTAS: usize = 32;
pub const MAX_GRUPOS: usize = 16;
pub const MAX_MIEMBROS: usize = 16;
/// Lo más que mide un nombre (de cuenta, de volumen o de grupo): los 16 bytes de las fichas de `sesiones`.
pub const MAX_NOMBRE: usize = 16;
/// Lo más que mide el nombre completo.
pub const MAX_COMPLETO: usize = 48;
/// Lo más que mide cada fichero que se lee: lo de más no se mira (la última línea queda cortada y no vale).
pub const MAX_FICHERO: usize = 4096;
/// Los números de las personas (S6b, "Números"): desde el 1000, y por debajo de 65.536 (`fib_OwnerUID`).
pub const PRIMERA_PERSONA: u32 = 1000;
pub const ULTIMA_PERSONA: u32 = 65535;
/// El grupo de los administradores.
pub const ADMIN: &str = "admin";
pub const GID_ADMIN: u32 = 10;
/// Los grupos para compartir cajones: del 100 al 999.
pub const PRIMER_GID_COMPARTIDO: u32 = 100;
pub const ULTIMO_GID_COMPARTIDO: u32 = 999;
/// Lo que dura una cuenta retirada antes de borrarse de verdad: 30 días. Pendiente del usuario (S6b: "el plazo, por
/// decidir").
pub const PLAZO_RETIRADA_S: u64 = 30 * 24 * 3600;

/// Nombres que no puede llevar una cuenta (ni su volumen), sin distinguir mayúsculas: los volúmenes y asignaciones del
/// sistema (S4b), que se confundirían con su `Home:`, y los que el registro usa por su cuenta.
pub const RESERVADOS: &[&str] = &[
    "SYS", "RAM", "Work", "Home", "Estado", "Programas", "Datos", "PROGDIR", "ENV", "ENVARC", "T", "C", "S", "L", "LIBS", "FONTS", "DEVS",
    "LOCALE", "CLASSES", "REXX", "HELP", "KEYMAPS", "PRINTERS", "NIL", "CON", "RAW", "PIPE", "Caja", "HomeC", "HomeLibs", "HomeFonts",
    "HomeClasses", "sistema", "nadie", "admin", "root",
];

/// Un nombre corto ya comprobado (cuenta, volumen o grupo).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nombre {
    b: [u8; MAX_NOMBRE],
    n: u8,
}

impl Nombre {
    /// Desde `n`, si vale (`nombre_valido`).
    pub fn de(n: &[u8]) -> Option<Nombre> {
        if !nombre_valido(n) {
            return None;
        }
        let mut b = [0u8; MAX_NOMBRE];
        b[..n.len()].copy_from_slice(n);
        Some(Nombre { b, n: n.len() as u8 })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.b[..self.n as usize]
    }

    pub fn as_str(&self) -> &str {
        // Solo ASCII (`nombre_valido`).
        core::str::from_utf8(self.bytes()).unwrap_or("?")
    }

    /// Si es el mismo nombre sin distinguir mayúsculas (como los volúmenes del Amiga).
    pub fn igual(&self, otro: &[u8]) -> bool {
        self.bytes().eq_ignore_ascii_case(otro)
    }
}

/// El nombre completo, ya comprobado.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Completo {
    b: [u8; MAX_COMPLETO],
    n: u8,
}

impl Completo {
    pub fn de(t: &[u8]) -> Option<Completo> {
        if !completo_valido(t) {
            return None;
        }
        let mut b = [0u8; MAX_COMPLETO];
        b[..t.len()].copy_from_slice(t);
        Some(Completo { b, n: t.len() as u8 })
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n as usize]).unwrap_or("?")
    }
}

/// Un nombre de cuenta, de volumen o de grupo: de 1 a 16 letras ASCII, cifras, `-` o `_`, que empieza por una letra, y
/// que no es de los reservados. Nada de `:` (separa los campos), `/`, espacios ni letras de control.
pub fn nombre_valido(n: &[u8]) -> bool {
    !n.is_empty()
        && n.len() <= MAX_NOMBRE
        && n[0].is_ascii_alphabetic()
        && n.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        && !RESERVADOS.iter().any(|r| r.as_bytes().eq_ignore_ascii_case(n))
}

/// El nombre completo: UTF-8, hasta 48 bytes, sin letras de control ni `:` (puede estar vacío).
pub fn completo_valido(t: &[u8]) -> bool {
    t.len() <= MAX_COMPLETO && core::str::from_utf8(t).is_ok_and(|s| !s.chars().any(|c| c.is_control() || c == ':'))
}

/// Un número en decimal, sin signo ni nada más.
fn numero(t: &[u8]) -> Option<u64> {
    if t.is_empty() || t.len() > 19 || !t.iter().all(u8::is_ascii_digit) {
        return None;
    }
    core::str::from_utf8(t).ok()?.parse().ok()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Estado {
    Activa,
    /// Retirada desde ese momento (segundos desde 1970).
    Retirada(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cuenta {
    pub nombre: Nombre,
    pub uid: u32,
    pub gid: u32,
    pub completo: Completo,
    pub volumen: Nombre,
    pub estado: Estado,
}

impl Cuenta {
    pub fn activa(&self) -> bool {
        self.estado == Estado::Activa
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Grupo {
    pub nombre: Nombre,
    pub gid: u32,
    miembros: [u32; MAX_MIEMBROS],
    n: usize,
}

impl Grupo {
    /// Los uid de sus miembros.
    pub fn miembros(&self) -> &[u32] {
        &self.miembros[..self.n]
    }

    fn tiene(&self, uid: u32) -> bool {
        self.miembros().contains(&uid)
    }
}

/// Por qué no se puede hacer algo con las cuentas (o por qué no vale una línea).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fallo {
    /// Un nombre de cuenta o de grupo que no vale (letras raras, largo, vacío).
    NombreMalo,
    /// Un nombre de los reservados.
    Reservado,
    /// Un nombre completo que no vale.
    CompletoMalo,
    /// Ya hay una cuenta (o un volumen) con ese nombre, sin distinguir mayúsculas.
    YaExiste,
    /// El número ya es de otra cuenta.
    UidUsado,
    /// El número no es de una persona (del 1000 al 65.535).
    UidMalo,
    /// No caben más (cuentas, grupos o miembros).
    Lleno,
    /// No hay esa cuenta (o ese grupo).
    NoExiste,
    /// Ya estaba retirada.
    YaRetirada,
    /// No estaba retirada (devolver).
    NoRetirada,
    /// Es la cuenta de quien lo pide.
    Propia,
    /// Dejaría el sistema sin ningún administrador.
    UltimoAdmin,
    /// Ya estaba en el grupo, o no estaba.
    YaEsta,
    NoEsta,
    /// Una línea de un fichero que no se entiende (campos, números o estado).
    Linea,
    /// La contraseña de una cuenta nueva es corta (`MIN_CLAVE`).
    ClaveCorta,
    /// El sistema no sabe la hora: el plazo de una cuenta borrada no se podría contar.
    SinHora,
    /// La persona tiene su sesión abierta.
    SesionAbierta,
    /// Rama `cuentas-2`: la contraseña nueva (al cambiarla, al recuperar la cuenta o en el primer arranque) es corta.
    NuevaCorta,
    /// Rama `cuentas-2`: ya hay cuentas: el asistente del primer arranque no se abre.
    HayCuentas,
}

/// Lo más corta que puede ser la contraseña de una cuenta nueva. Pendiente del usuario.
pub const MIN_CLAVE: usize = 8;

/// Los fallos como estados de una respuesta (los de `sesiones` y los de archivos son de -1 a -20).
const FALLOS: [Fallo; 20] = [
    Fallo::NombreMalo,
    Fallo::Reservado,
    Fallo::CompletoMalo,
    Fallo::YaExiste,
    Fallo::UidUsado,
    Fallo::UidMalo,
    Fallo::Lleno,
    Fallo::NoExiste,
    Fallo::YaRetirada,
    Fallo::NoRetirada,
    Fallo::Propia,
    Fallo::UltimoAdmin,
    Fallo::YaEsta,
    Fallo::NoEsta,
    Fallo::Linea,
    Fallo::ClaveCorta,
    Fallo::SinHora,
    Fallo::SesionAbierta,
    Fallo::NuevaCorta,
    Fallo::HayCuentas,
];

impl Fallo {
    /// Su estado en una respuesta: de -101 en adelante.
    pub fn estado(self) -> i32 {
        -101 - FALLOS.iter().position(|&f| f == self).unwrap_or(0) as i32
    }

    /// El fallo de un estado, si es uno de estos.
    pub fn de_estado(e: i32) -> Option<Fallo> {
        e.checked_neg().and_then(|n| n.checked_sub(101)).and_then(|i| FALLOS.get(i as usize)).copied()
    }
}

impl core::fmt::Display for Fallo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.texto())
    }
}

impl Fallo {
    /// Lo que dice, en llano.
    pub fn texto(&self) -> &'static str {
        match self {
            Fallo::NombreMalo => "el nombre no vale: de 1 a 16 letras sin acentos, cifras, - o _, y empieza por una letra",
            Fallo::Reservado => "ese nombre es del sistema",
            Fallo::CompletoMalo => "el nombre completo no vale: hasta 48 bytes, sin dos puntos ni letras de control",
            Fallo::YaExiste => "ya hay una cuenta con ese nombre",
            Fallo::UidUsado => "ese número ya es de otra cuenta",
            Fallo::UidMalo => "el número de una persona va del 1000 al 65535",
            Fallo::Lleno => "no caben más",
            Fallo::NoExiste => "no existe",
            Fallo::YaRetirada => "ya está borrada (se puede devolver)",
            Fallo::NoRetirada => "no está borrada",
            Fallo::Propia => "es tu propia cuenta",
            Fallo::UltimoAdmin => "es la última cuenta del grupo admin: el sistema se quedaría sin administradores",
            Fallo::YaEsta => "ya está en ese grupo",
            Fallo::NoEsta => "no está en ese grupo",
            Fallo::Linea => "la línea no se entiende",
            Fallo::ClaveCorta => "la contraseña de la cuenta nueva es corta: 8 letras o más",
            Fallo::SinHora => "el sistema no sabe la hora: el plazo para devolverla no se podría contar",
            Fallo::SesionAbierta => "tiene su sesión abierta",
            Fallo::NuevaCorta => "la contraseña nueva es corta: 8 letras o más",
            Fallo::HayCuentas => "ya hay cuentas: el primer arranque ya se hizo",
        }
    }
}

/// Lo que pasó al leer un fichero: cuántas líneas valen, cuántas no y la primera que no (contando desde 1), y si la
/// última estaba cortada (sin salto de línea al final: el fichero se truncó).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Informe {
    pub buenas: usize,
    pub malas: usize,
    pub primera_mala: usize,
    pub cortada: bool,
}

impl Informe {
    fn mala(&mut self, linea: usize) {
        if self.malas == 0 {
            self.primera_mala = linea;
        }
        self.malas += 1;
    }
}

/// Las cuentas y los grupos, en memoria.
#[derive(Clone, Copy)]
pub struct Cuentas {
    cuentas: [Option<Cuenta>; MAX_CUENTAS],
    grupos: [Option<Grupo>; MAX_GRUPOS],
}

/// Las líneas de `texto` con su número (desde 1), sin las vacías ni los comentarios, y si la última se cortó (no acaba
/// en salto de línea: no se da).
fn lineas(texto: &[u8]) -> (impl Iterator<Item = (usize, &[u8])>, bool) {
    let cortada = !texto.is_empty() && !texto.ends_with(b"\n");
    let completas = match texto.iter().rposition(|&c| c == b'\n') {
        Some(p) => &texto[..p + 1],
        None => &texto[..0],
    };
    let it = completas
        .split(|&c| c == b'\n')
        .enumerate()
        .map(|(i, l)| (i + 1, l.strip_suffix(b"\r").unwrap_or(l)))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with(b"#"));
    (it, cortada)
}

impl Cuentas {
    pub const fn vacia() -> Cuentas {
        Cuentas { cuentas: [None; MAX_CUENTAS], grupos: [None; MAX_GRUPOS] }
    }

    /// Lee `usuarios` y después `grupos` (los miembros se buscan entre las cuentas). Lo de antes se olvida.
    pub fn leer(usuarios: &[u8], grupos: &[u8]) -> (Cuentas, Informe, Informe) {
        let mut c = Cuentas::vacia();
        let iu = c.leer_usuarios(usuarios);
        let ig = c.leer_grupos(grupos);
        (c, iu, ig)
    }

    fn leer_usuarios(&mut self, texto: &[u8]) -> Informe {
        let mut inf = Informe::default();
        let (it, cortada) = lineas(&texto[..texto.len().min(MAX_FICHERO)]);
        inf.cortada = cortada || texto.len() > MAX_FICHERO;
        for (i, l) in it {
            match Self::una_cuenta(l).and_then(|c| self.anadir(c)) {
                Ok(()) => inf.buenas += 1,
                Err(_) => inf.mala(i),
            }
        }
        if inf.cortada {
            let n = texto[..texto.len().min(MAX_FICHERO)].split(|&c| c == b'\n').count();
            inf.mala(n);
        }
        inf
    }

    fn una_cuenta(l: &[u8]) -> Result<Cuenta, Fallo> {
        let mut p = l.split(|&c| c == b':');
        let (Some(nombre), Some(uid), Some(gid), Some(completo), Some(volumen), Some(estado), None) = (p.next(), p.next(), p.next(), p.next(), p.next(), p.next(), p.next()) else {
            return Err(Fallo::Linea);
        };
        let nombre = Nombre::de(nombre).ok_or(Fallo::NombreMalo)?;
        let uid = numero(uid).filter(|u| (PRIMERA_PERSONA as u64..=ULTIMA_PERSONA as u64).contains(u)).ok_or(Fallo::UidMalo)? as u32;
        // Su grupo propio, con su mismo número (S6b).
        if numero(gid) != Some(uid as u64) {
            return Err(Fallo::Linea);
        }
        let completo = Completo::de(completo).ok_or(Fallo::CompletoMalo)?;
        let volumen = Nombre::de(volumen).ok_or(Fallo::NombreMalo)?;
        let estado = match estado {
            b"activa" => Estado::Activa,
            e => match e.strip_prefix(b"retirada ").and_then(numero) {
                Some(desde) => Estado::Retirada(desde),
                None => return Err(Fallo::Linea),
            },
        };
        Ok(Cuenta { nombre, uid, gid: uid, completo, volumen, estado })
    }

    /// Añade una cuenta si no choca con ninguna: ni su nombre, ni su número, ni su volumen.
    fn anadir(&mut self, c: Cuenta) -> Result<(), Fallo> {
        for x in self.cuentas.iter().flatten() {
            if x.nombre.igual(c.nombre.bytes()) || x.volumen.igual(c.volumen.bytes()) || x.nombre.igual(c.volumen.bytes()) || x.volumen.igual(c.nombre.bytes()) {
                return Err(Fallo::YaExiste);
            }
            if x.uid == c.uid {
                return Err(Fallo::UidUsado);
            }
        }
        let h = self.cuentas.iter_mut().find(|h| h.is_none()).ok_or(Fallo::Lleno)?;
        *h = Some(c);
        Ok(())
    }

    fn leer_grupos(&mut self, texto: &[u8]) -> Informe {
        let mut inf = Informe::default();
        let (it, cortada) = lineas(&texto[..texto.len().min(MAX_FICHERO)]);
        inf.cortada = cortada || texto.len() > MAX_FICHERO;
        for (i, l) in it {
            let mut p = l.split(|&c| c == b':');
            let (Some(nombre), Some(gid), Some(miembros), None) = (p.next(), p.next(), p.next(), p.next()) else {
                inf.mala(i);
                continue;
            };
            let (Some(nombre), Some(gid)) = (Nombre::de(nombre).or_else(|| (nombre == ADMIN.as_bytes()).then(|| Self::nombre_admin())), numero(gid)) else {
                inf.mala(i);
                continue;
            };
            let es_admin = nombre.bytes() == ADMIN.as_bytes();
            let gid_vale = if es_admin { gid == GID_ADMIN as u64 } else { (PRIMER_GID_COMPARTIDO as u64..=ULTIMO_GID_COMPARTIDO as u64).contains(&gid) };
            let repetido = self.grupos.iter().flatten().any(|g| g.nombre.igual(nombre.bytes()) || g.gid as u64 == gid);
            let Some(h) = self.grupos.iter().position(|g| g.is_none()).filter(|_| gid_vale && !repetido) else {
                inf.mala(i);
                continue;
            };
            let mut g = Grupo { nombre, gid: gid as u32, miembros: [0; MAX_MIEMBROS], n: 0 };
            // Los miembros, por su nombre: uno que no es una cuenta (o repetido, o que no cabe) se salta, y la línea
            // cuenta como mala, pero el grupo se queda con los demás.
            let mut todos_bien = true;
            for m in miembros.split(|&c| c == b',').filter(|m| !m.is_empty()) {
                match self.por_nombre(m) {
                    Some(c) if !g.tiene(c.uid) && g.n < MAX_MIEMBROS => {
                        g.miembros[g.n] = c.uid;
                        g.n += 1;
                    }
                    _ => todos_bien = false,
                }
            }
            self.grupos[h] = Some(g);
            if todos_bien {
                inf.buenas += 1;
            } else {
                inf.mala(i);
            }
        }
        if inf.cortada {
            let n = texto[..texto.len().min(MAX_FICHERO)].split(|&c| c == b'\n').count();
            inf.mala(n);
        }
        inf
    }

    /// `admin` es un nombre reservado para una cuenta, pero es el nombre del grupo de los administradores.
    fn nombre_admin() -> Nombre {
        let mut b = [0u8; MAX_NOMBRE];
        b[..ADMIN.len()].copy_from_slice(ADMIN.as_bytes());
        Nombre { b, n: ADMIN.len() as u8 }
    }

    /// Las cuentas, activas y retiradas.
    pub fn todas(&self) -> impl Iterator<Item = &Cuenta> {
        self.cuentas.iter().flatten()
    }

    pub fn grupos(&self) -> impl Iterator<Item = &Grupo> {
        self.grupos.iter().flatten()
    }

    pub fn por_uid(&self, uid: u32) -> Option<&Cuenta> {
        self.todas().find(|c| c.uid == uid)
    }

    /// La cuenta con ese nombre, sin distinguir mayúsculas.
    pub fn por_nombre(&self, n: &[u8]) -> Option<&Cuenta> {
        self.todas().find(|c| c.nombre.igual(n))
    }

    /// La cuenta activa con ese nombre exacto (para entrar: "Ana", no "ana").
    pub fn para_entrar(&self, n: &[u8]) -> Option<&Cuenta> {
        self.todas().find(|c| c.nombre.bytes() == n && c.activa())
    }

    /// La cuenta activa del usuario `uid`.
    pub fn activa(&self, uid: u32) -> Option<&Cuenta> {
        self.por_uid(uid).filter(|c| c.activa())
    }

    pub fn grupo(&self, n: &[u8]) -> Option<&Grupo> {
        self.grupos().find(|g| g.nombre.igual(n))
    }

    /// Si `uid` es administrador: una cuenta activa del grupo `admin`.
    pub fn es_admin(&self, uid: u32) -> bool {
        self.activa(uid).is_some() && self.grupo(ADMIN.as_bytes()).is_some_and(|g| g.tiene(uid))
    }

    /// Los administradores activos.
    pub fn admins(&self) -> usize {
        self.grupo(ADMIN.as_bytes()).map_or(0, |g| g.miembros().iter().filter(|&&u| self.activa(u).is_some()).count())
    }

    /// El número que tendría una cuenta nueva: el siguiente al mayor que hay (no se reusa el de una retirada).
    pub fn siguiente_uid(&self) -> Option<u32> {
        let mayor = self.todas().map(|c| c.uid).max().unwrap_or(PRIMERA_PERSONA - 1);
        (mayor < ULTIMA_PERSONA).then_some(mayor + 1)
    }

    /// Mira si se puede crear la cuenta `nombre` (con el número `uid`, o el siguiente libre si es 0) y da su número.
    pub fn puede_crear(&self, nombre: &[u8], uid: u32, completo: &[u8]) -> Result<u32, Fallo> {
        let n = Self::nombre_para_crear(nombre)?;
        Completo::de(completo).ok_or(Fallo::CompletoMalo)?;
        if self.todas().any(|c| c.nombre.igual(n.bytes()) || c.volumen.igual(n.bytes())) {
            return Err(Fallo::YaExiste);
        }
        let uid = match uid {
            0 => self.siguiente_uid().ok_or(Fallo::Lleno)?,
            u if !(PRIMERA_PERSONA..=ULTIMA_PERSONA).contains(&u) => return Err(Fallo::UidMalo),
            u if self.por_uid(u).is_some() => return Err(Fallo::UidUsado),
            u => u,
        };
        if self.cuentas.iter().all(|h| h.is_some()) {
            return Err(Fallo::Lleno);
        }
        Ok(uid)
    }

    fn nombre_para_crear(nombre: &[u8]) -> Result<Nombre, Fallo> {
        match Nombre::de(nombre) {
            Some(n) => Ok(n),
            None if RESERVADOS.iter().any(|r| r.as_bytes().eq_ignore_ascii_case(nombre)) => Err(Fallo::Reservado),
            None => Err(Fallo::NombreMalo),
        }
    }

    /// Crea la cuenta (activa, con un volumen de su mismo nombre). Da su número.
    pub fn crear(&mut self, nombre: &[u8], uid: u32, completo: &[u8]) -> Result<u32, Fallo> {
        let uid = self.puede_crear(nombre, uid, completo)?;
        let n = Self::nombre_para_crear(nombre)?;
        let c = Cuenta { nombre: n, uid, gid: uid, completo: Completo::de(completo).ok_or(Fallo::CompletoMalo)?, volumen: n, estado: Estado::Activa };
        self.anadir(c)?;
        Ok(uid)
    }

    /// Mira si se puede retirar (borrar, que se deshace) la cuenta `nombre`, pedido por el usuario `pide`: ni la propia
    /// ni la del último administrador.
    pub fn puede_retirar(&self, nombre: &[u8], pide: u32) -> Result<u32, Fallo> {
        let c = self.por_nombre(nombre).ok_or(Fallo::NoExiste)?;
        if !c.activa() {
            return Err(Fallo::YaRetirada);
        }
        if c.uid == pide {
            return Err(Fallo::Propia);
        }
        if self.es_admin(c.uid) && self.admins() <= 1 {
            return Err(Fallo::UltimoAdmin);
        }
        Ok(c.uid)
    }

    /// La retira desde `ahora` (segundos desde 1970). Da su número.
    pub fn retirar(&mut self, nombre: &[u8], pide: u32, ahora: u64) -> Result<u32, Fallo> {
        let uid = self.puede_retirar(nombre, pide)?;
        for c in self.cuentas.iter_mut().flatten().filter(|c| c.uid == uid) {
            c.estado = Estado::Retirada(ahora);
        }
        Ok(uid)
    }

    pub fn puede_devolver(&self, nombre: &[u8]) -> Result<u32, Fallo> {
        let c = self.por_nombre(nombre).ok_or(Fallo::NoExiste)?;
        if c.activa() { Err(Fallo::NoRetirada) } else { Ok(c.uid) }
    }

    /// La vuelve a dejar activa, con sus grupos (no se quitaron). Da su número.
    pub fn devolver(&mut self, nombre: &[u8]) -> Result<u32, Fallo> {
        let uid = self.puede_devolver(nombre)?;
        for c in self.cuentas.iter_mut().flatten().filter(|c| c.uid == uid) {
            c.estado = Estado::Activa;
        }
        Ok(uid)
    }

    /// Las retiradas a las que se les acabó el plazo en `ahora`. Si el reloj dice una hora de antes de que se retirara
    /// (un reloj sin poner), no ha vencido: borrar de verdad no se deshace.
    pub fn vencida(c: &Cuenta, ahora: u64) -> bool {
        matches!(c.estado, Estado::Retirada(desde) if ahora >= desde && ahora - desde >= PLAZO_RETIRADA_S)
    }

    /// Quita del todo la cuenta (y de sus grupos). La da, para borrar su volumen.
    pub fn quitar(&mut self, uid: u32) -> Option<Cuenta> {
        let h = self.cuentas.iter().position(|c| c.is_some_and(|c| c.uid == uid))?;
        let c = self.cuentas[h].take();
        for g in self.grupos.iter_mut().flatten() {
            if let Some(p) = g.miembros().iter().position(|&u| u == uid) {
                g.miembros.copy_within(p + 1..g.n, p);
                g.n -= 1;
            }
        }
        // Un grupo para compartir que se queda sin nadie, fuera; `admin` se queda.
        for g in self.grupos.iter_mut() {
            if g.is_some_and(|g| g.n == 0 && g.gid != GID_ADMIN) {
                *g = None;
            }
        }
        c
    }

    /// Mira si se puede meter la cuenta en el grupo (que se crea si no está). Da (uid, gid, si el grupo es nuevo).
    pub fn puede_meter(&self, cuenta: &[u8], grupo: &[u8]) -> Result<(u32, u32, bool), Fallo> {
        let c = self.por_nombre(cuenta).ok_or(Fallo::NoExiste)?;
        let es_admin = grupo == ADMIN.as_bytes();
        if !es_admin && Nombre::de(grupo).is_none() {
            return Err(Fallo::NombreMalo);
        }
        match self.grupo(grupo) {
            Some(g) if g.tiene(c.uid) => Err(Fallo::YaEsta),
            Some(g) if g.n >= MAX_MIEMBROS => Err(Fallo::Lleno),
            Some(g) => Ok((c.uid, g.gid, false)),
            None if self.grupos.iter().all(|g| g.is_some()) => Err(Fallo::Lleno),
            None if es_admin => Ok((c.uid, GID_ADMIN, true)),
            None => {
                let gid = (PRIMER_GID_COMPARTIDO..=ULTIMO_GID_COMPARTIDO).find(|&g| self.grupos().all(|x| x.gid != g)).ok_or(Fallo::Lleno)?;
                Ok((c.uid, gid, true))
            }
        }
    }

    pub fn meter(&mut self, cuenta: &[u8], grupo: &[u8]) -> Result<(u32, u32), Fallo> {
        let (uid, gid, nuevo) = self.puede_meter(cuenta, grupo)?;
        if nuevo {
            let nombre = if grupo == ADMIN.as_bytes() { Self::nombre_admin() } else { Nombre::de(grupo).ok_or(Fallo::NombreMalo)? };
            let h = self.grupos.iter().position(|g| g.is_none()).ok_or(Fallo::Lleno)?;
            self.grupos[h] = Some(Grupo { nombre, gid, miembros: [0; MAX_MIEMBROS], n: 0 });
        }
        let g = self.grupos.iter_mut().flatten().find(|g| g.gid == gid).ok_or(Fallo::NoExiste)?;
        g.miembros[g.n] = uid;
        g.n += 1;
        Ok((uid, gid))
    }

    /// Mira si se puede sacar la cuenta del grupo: no al último administrador.
    pub fn puede_sacar(&self, cuenta: &[u8], grupo: &[u8]) -> Result<(u32, u32), Fallo> {
        let c = self.por_nombre(cuenta).ok_or(Fallo::NoExiste)?;
        let g = self.grupo(grupo).ok_or(Fallo::NoExiste)?;
        if !g.tiene(c.uid) {
            return Err(Fallo::NoEsta);
        }
        if g.gid == GID_ADMIN && self.es_admin(c.uid) && self.admins() <= 1 {
            return Err(Fallo::UltimoAdmin);
        }
        Ok((c.uid, g.gid))
    }

    pub fn sacar(&mut self, cuenta: &[u8], grupo: &[u8]) -> Result<(u32, u32), Fallo> {
        let (uid, gid) = self.puede_sacar(cuenta, grupo)?;
        let h = self.grupos.iter().position(|g| g.is_some_and(|g| g.gid == gid)).ok_or(Fallo::NoExiste)?;
        let g = self.grupos[h].as_mut().ok_or(Fallo::NoExiste)?;
        let p = g.miembros().iter().position(|&u| u == uid).ok_or(Fallo::NoEsta)?;
        g.miembros.copy_within(p + 1..g.n, p);
        g.n -= 1;
        if g.n == 0 && gid != GID_ADMIN {
            self.grupos[h] = None;
        }
        Ok((uid, gid))
    }

    /// El fichero `usuarios` entero en `sal`: la cabecera y una línea por cuenta. Lo que mide, o `None` si no cabe.
    pub fn escribir_usuarios(&self, sal: &mut [u8]) -> Option<usize> {
        let mut e = Escritor { sal, n: 0 };
        e.poner(CABECERA_USUARIOS.as_bytes())?;
        for c in self.todas() {
            e.poner(c.nombre.bytes())?;
            e.fmt(format_args!(":{}:{}:{}:", c.uid, c.gid, c.completo.as_str()))?;
            e.poner(c.volumen.bytes())?;
            match c.estado {
                Estado::Activa => e.poner(b":activa\n")?,
                Estado::Retirada(desde) => e.fmt(format_args!(":retirada {}\n", desde))?,
            }
        }
        Some(e.n)
    }

    /// El fichero `grupos` entero en `sal`.
    pub fn escribir_grupos(&self, sal: &mut [u8]) -> Option<usize> {
        let mut e = Escritor { sal, n: 0 };
        e.poner(CABECERA_GRUPOS.as_bytes())?;
        for g in self.grupos() {
            e.poner(g.nombre.bytes())?;
            e.fmt(format_args!(":{}:", g.gid))?;
            for (i, &u) in g.miembros().iter().enumerate() {
                if i > 0 {
                    e.poner(b",")?;
                }
                e.poner(self.por_uid(u).map_or(&b""[..], |c| c.nombre.bytes()))?;
            }
            e.poner(b"\n")?;
        }
        Some(e.n)
    }
}

struct Escritor<'a> {
    sal: &'a mut [u8],
    n: usize,
}

impl Escritor<'_> {
    fn poner(&mut self, b: &[u8]) -> Option<()> {
        self.sal.get_mut(self.n..self.n + b.len())?.copy_from_slice(b);
        self.n += b.len();
        Some(())
    }

    fn fmt(&mut self, a: core::fmt::Arguments) -> Option<()> {
        struct Cuenta<'x, 'y>(&'x mut Escritor<'y>, bool);
        impl core::fmt::Write for Cuenta<'_, '_> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                if self.0.poner(s.as_bytes()).is_none() {
                    self.1 = false;
                }
                Ok(())
            }
        }
        let mut c = Cuenta(self, true);
        let _ = core::fmt::Write::write_fmt(&mut c, a);
        c.1.then_some(())
    }
}

/// El código de recuperación de un volumen, para apuntar en papel (S6b): 24 letras y cifras sin las que se confunden
/// (ni I, O, 0 ni 1), en seis grupos de cuatro: `ABCD-EFGH-JKLM-NPQR-STUV-WXYZ`. Salen de 15 bytes de azar (120 bits).
pub const LARGO_CODIGO: usize = 29;
const LETRAS_CODIGO: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn codigo_recuperacion(azar: &[u8; 15]) -> [u8; LARGO_CODIGO] {
    let mut c = [b'-'; LARGO_CODIGO];
    // 120 bits en 24 trozos de 5, del primero al último.
    for i in 0..24 {
        let bit = i * 5;
        let dos = ((azar[bit / 8] as u16) << 8) | azar.get(bit / 8 + 1).copied().unwrap_or(0) as u16;
        let v = (dos >> (11 - bit % 8)) & 31;
        c[i + i / 4] = LETRAS_CODIGO[v as usize];
    }
    c
}

/// Lo que alguien teclea como código de recuperación, en su forma de siempre (`ABCD-EFGH-...`): se aceptan minúsculas,
/// espacios y guiones donde sea (en el papel puede estar escrito de cualquier manera), pero tienen que ser 24 letras del
/// código. `None` si no lo es (una letra que no está, como la O o el 1, o de más o de menos).
pub fn normalizar_codigo(t: &[u8]) -> Option<[u8; LARGO_CODIGO]> {
    let mut c = [b'-'; LARGO_CODIGO];
    let mut n = 0;
    for &b in t.iter().filter(|&&b| b != b' ' && b != b'-') {
        let b = b.to_ascii_uppercase();
        if n == 24 || !LETRAS_CODIGO.contains(&b) {
            return None;
        }
        c[n + n / 4] = b;
        n += 1;
    }
    (n == 24).then_some(c)
}

/// Contra probar contraseñas a ciegas (rama `cuentas-2`): tras `FALLOS_SIN_ESPERA` intentos malos seguidos con una
/// cuenta (entrar, volver de un bloqueo, la contraseña de administrador, la actual al cambiarla o el código de
/// recuperación), hay que esperar antes del siguiente: `PRIMERA_ESPERA_US` y el doble con cada fallo más, hasta
/// `MAX_ESPERA_US`. Mientras, no se prueba nada (ni la buena): se contesta cuánto falta. Uno bueno lo deja a cero. Los
/// nombres que no son de nadie van todos a la cuenta 0. Solo en memoria: al reiniciar, de cero.
pub const FALLOS_SIN_ESPERA: u32 = 3;
pub const PRIMERA_ESPERA_US: u64 = 5_000_000;
pub const MAX_ESPERA_US: u64 = 300_000_000;

/// Los intentos malos de cada cuenta: (usuario, malos seguidos, hasta cuándo espera en µs). Una por cuenta y la 0.
#[derive(Clone, Copy)]
pub struct Esperas {
    e: [(u32, u32, u64); MAX_CUENTAS + 1],
}

impl Esperas {
    pub const fn nuevas() -> Esperas {
        Esperas { e: [(u32::MAX, 0, 0); MAX_CUENTAS + 1] }
    }

    /// Los µs que le quedan de espera a la cuenta `uid` en `ahora`; `None` si puede probar.
    pub fn falta(&self, uid: u32, ahora: u64) -> Option<u64> {
        self.e.iter().find(|x| x.0 == uid).map(|x| x.2.saturating_sub(ahora)).filter(|&f| f > 0)
    }

    /// La espera tras `malos` intentos malos seguidos.
    pub fn espera(malos: u32) -> u64 {
        if malos < FALLOS_SIN_ESPERA {
            return 0;
        }
        PRIMERA_ESPERA_US.saturating_mul(1u64 << (malos - FALLOS_SIN_ESPERA).min(20)).min(MAX_ESPERA_US)
    }

    /// Un intento malo de `uid` en `ahora`: cuántos lleva seguidos y la espera que empieza (0 si aún ninguna).
    pub fn mala(&mut self, uid: u32, ahora: u64) -> (u32, u64) {
        let i = match self.e.iter().position(|x| x.0 == uid) {
            Some(i) => i,
            // Sin sitio (más cuentas de las que caben, no debería pasar): se reusa la que lleva menos.
            None => match self.e.iter().position(|x| x.0 == u32::MAX) {
                Some(i) => i,
                None => (0..self.e.len()).min_by_key(|&i| self.e[i].1).unwrap_or(0),
            },
        };
        let malos = if self.e[i].0 == uid { self.e[i].1.saturating_add(1) } else { 1 };
        let espera = Self::espera(malos);
        self.e[i] = (uid, malos, ahora.saturating_add(espera));
        (malos, espera)
    }

    /// Un intento bueno: a cero.
    pub fn buena(&mut self, uid: u32) {
        for x in self.e.iter_mut().filter(|x| x.0 == uid) {
            *x = (u32::MAX, 0, 0);
        }
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const ANA_CARLA: &str = "# cabecera\nAna:1000:1000:Ana:Ana:activa\nCarla:1001:1001:Carla Pérez:Carla:activa\n";
    const ADMINS: &str = "admin:10:Ana\n";

    fn de(u: &str, g: &str) -> (Cuentas, Informe, Informe) {
        Cuentas::leer(u.as_bytes(), g.as_bytes())
    }

    fn ida_y_vuelta(c: &Cuentas) -> Cuentas {
        let (mut u, mut g) = ([0u8; MAX_FICHERO], [0u8; MAX_FICHERO]);
        let (nu, ng) = (c.escribir_usuarios(&mut u).unwrap(), c.escribir_grupos(&mut g).unwrap());
        let (d, iu, ig) = Cuentas::leer(&u[..nu], &g[..ng]);
        assert_eq!((iu.malas, ig.malas), (0, 0), "{}", core::str::from_utf8(&u[..nu]).unwrap());
        d
    }

    #[test]
    fn las_de_las_pruebas() {
        let (c, iu, ig) = de(ANA_CARLA, ADMINS);
        assert_eq!((iu.buenas, iu.malas, ig.buenas, ig.malas), (2, 0, 1, 0));
        assert_eq!(c.para_entrar(b"Ana").unwrap().uid, 1000);
        assert_eq!(c.por_uid(1001).unwrap().completo.as_str(), "Carla Pérez");
        assert!(c.para_entrar(b"ana").is_none(), "para entrar, el nombre exacto");
        assert_eq!(c.por_nombre(b"ana").unwrap().uid, 1000);
        assert!(c.es_admin(1000) && !c.es_admin(1001) && !c.es_admin(5));
        assert_eq!(c.siguiente_uid(), Some(1002));
        let d = ida_y_vuelta(&c);
        assert_eq!(d.todas().count(), 2);
        assert!(d.es_admin(1000));
    }

    #[test]
    fn lineas_malas_no_tiran_las_buenas() {
        // Campos de más y de menos, un nombre con letras raras, un uid repetido, uno de sistema, el gid que no es el
        // suyo, un estado que no existe, un nombre repetido con otras mayúsculas, un volumen repetido.
        let u = "Ana:1000:1000:Ana:Ana:activa\n\
                 basura sin dos puntos\n\
                 Eva:1002:1002:Eva:Eva:activa:de más\n\
                 Eva:1002:1002:Eva:Eva\n\
                 E va:1003:1003:x:Eva:activa\n\
                 Rosa:1000:1000:Rosa:Rosa:activa\n\
                 Sis:5:5:Sis:Sis:activa\n\
                 Gil:1004:1000:Gil:Gil:activa\n\
                 Lia:1005:1005:Lia:Lia:dormida\n\
                 ANA:1006:1006:otra:Otra:activa\n\
                 Mar:1007:1007:Mar:Ana:activa\n\
                 Carla:1001:1001:Carla:Carla:retirada 1763300000\n";
        let (c, iu, _) = de(u, "");
        assert_eq!((iu.buenas, iu.malas, iu.primera_mala, iu.cortada), (2, 10, 2, false));
        assert_eq!(c.todas().count(), 2);
        assert_eq!(c.por_uid(1001).unwrap().estado, Estado::Retirada(1763300000));
        assert!(c.para_entrar(b"Carla").is_none(), "retirada: no entra");
    }

    #[test]
    fn fichero_truncado() {
        // Cortado a media línea: esa no vale (aunque se pudiera leer, podría ser una fecha a medias), las de antes sí.
        let entero = format!("{}Dora:1002:1002:Dora:Dora:retirada 1763300000\n", ANA_CARLA);
        for corte in [entero.len() - 1, entero.len() - 6, entero.len() - 30] {
            let (c, iu, _) = de(&entero[..corte], ADMINS);
            assert!(iu.cortada && iu.malas == 1, "{corte}");
            assert_eq!(c.todas().count(), 2, "{corte}");
        }
        // Vacío del todo, o solo una línea sin acabar: nada, y se dice.
        let (c, iu, _) = de("Ana:1000:1000:Ana:Ana:activa", "");
        assert!(iu.cortada && c.todas().count() == 0);
        let (c, iu, _) = de("", "");
        assert!(!iu.cortada && iu.malas == 0 && c.todas().count() == 0);
        // Más de lo que se lee: lo de después no se mira y la última, cortada.
        let largo = format!("{}{}", ANA_CARLA, "# relleno\n".repeat(500));
        let (c, iu, _) = de(&largo, "");
        assert!(iu.cortada && c.todas().count() == 2);
    }

    #[test]
    fn grupos_malos() {
        let g = "admin:10:Ana,Nadie,Ana\nfotos:100:Carla\nadmin:11:Carla\nfotos:101:Ana\nmusica:5:Ana\nvideo:100:Ana\nraro:200\n";
        let (c, _, ig) = de(ANA_CARLA, g);
        // admin con Ana (Nadie y la Ana repetida se saltan, y la línea cuenta como mala); fotos; lo demás, malo.
        assert_eq!((ig.buenas, ig.malas), (1, 6));
        assert_eq!(c.grupo(b"admin").unwrap().miembros(), &[1000]);
        assert_eq!(c.grupo(b"fotos").unwrap().miembros(), &[1001]);
        assert!(c.grupo(b"musica").is_none() && c.grupo(b"video").is_none());
        // Un admin con otro número no lo es.
        let (c, _, ig) = de(ANA_CARLA, "admin:11:Carla\n");
        assert!(ig.malas == 1 && c.admins() == 0 && !c.es_admin(1001));
    }

    #[test]
    fn nombres_hostiles() {
        let (mut c, ..) = de(ANA_CARLA, ADMINS);
        for (malo, fallo) in [
            ("", Fallo::NombreMalo),
            ("a:b", Fallo::NombreMalo),
            ("a\nb", Fallo::NombreMalo),
            ("Ana\n", Fallo::NombreMalo),
            ("diecisieteletrasx", Fallo::NombreMalo),
            ("1ana", Fallo::NombreMalo),
            ("Peña", Fallo::NombreMalo),
            ("a/b", Fallo::NombreMalo),
            ("a b", Fallo::NombreMalo),
            ("..", Fallo::NombreMalo),
            ("SYS", Fallo::Reservado),
            ("home", Fallo::Reservado),
            ("Admin", Fallo::Reservado),
            ("ANA", Fallo::YaExiste),
            ("carla", Fallo::YaExiste),
        ] {
            assert_eq!(c.crear(malo.as_bytes(), 0, b""), Err(fallo), "{malo:?}");
        }
        // 16 justas valen.
        assert!(c.puede_crear(b"diecisieteletras", 0, b"").is_ok());
        assert_eq!(c.crear(b"Eva", 0, b"con: dos puntos"), Err(Fallo::CompletoMalo));
        assert_eq!(c.crear(b"Eva", 0, b"salto\nde linea"), Err(Fallo::CompletoMalo));
        assert_eq!(c.crear(b"Eva", 0, "x".repeat(49).as_bytes()), Err(Fallo::CompletoMalo));
        assert_eq!(c.crear(b"Eva", 1001, b""), Err(Fallo::UidUsado));
        assert_eq!(c.crear(b"Eva", 999, b""), Err(Fallo::UidMalo));
        assert_eq!(c.crear(b"Eva", 70000, b""), Err(Fallo::UidMalo));
        assert_eq!(c.todas().count(), 2, "nada de lo malo se creó");
        assert_eq!(c.crear(b"Eva", 0, "Eva Núñez".as_bytes()), Ok(1002));
        assert_eq!(c.crear(b"Gil", 2000, b""), Ok(2000));
        assert_eq!(c.siguiente_uid(), Some(2001));
        let d = ida_y_vuelta(&c);
        assert_eq!(d.por_nombre(b"Eva").unwrap().completo.as_str(), "Eva Núñez");
    }

    #[test]
    fn llenas() {
        let (mut c, ..) = de(ANA_CARLA, ADMINS);
        for i in 0..MAX_CUENTAS - 2 {
            c.crear(format!("u{i}").as_bytes(), 0, b"").unwrap();
        }
        assert_eq!(c.crear(b"otra", 0, b""), Err(Fallo::Lleno));
        let d = ida_y_vuelta(&c);
        assert_eq!(d.todas().count(), MAX_CUENTAS);
        let (c, ..) = de("Ana:65535:65535:Ana:Ana:activa\n", "");
        assert_eq!(c.siguiente_uid(), None);
    }

    #[test]
    fn retirar_y_devolver() {
        let (mut c, ..) = de(ANA_CARLA, ADMINS);
        // Ni la propia, ni la del último administrador, ni una que no existe.
        assert_eq!(c.retirar(b"Ana", 1000, 5), Err(Fallo::Propia));
        assert_eq!(c.retirar(b"Ana", 1001, 5), Err(Fallo::UltimoAdmin));
        assert_eq!(c.retirar(b"Nadie", 1000, 5), Err(Fallo::NoExiste));
        assert_eq!(c.devolver(b"Carla"), Err(Fallo::NoRetirada));
        assert_eq!(c.retirar(b"carla", 1000, 1_000_000), Ok(1001));
        assert_eq!(c.retirar(b"Carla", 1000, 5), Err(Fallo::YaRetirada));
        assert!(c.para_entrar(b"Carla").is_none() && c.activa(1001).is_none());
        // Ni crear otra con su nombre ni con su número mientras está retirada.
        assert_eq!(c.crear(b"Carla", 0, b""), Err(Fallo::YaExiste));
        assert_eq!(c.crear(b"Otra", 1001, b""), Err(Fallo::UidUsado));
        let d = ida_y_vuelta(&c);
        assert_eq!(d.por_uid(1001).unwrap().estado, Estado::Retirada(1_000_000));
        // El plazo: ni antes, ni con un reloj de antes de retirarla.
        let carla = *c.por_uid(1001).unwrap();
        assert!(!Cuentas::vencida(&carla, 1_000_000 + PLAZO_RETIRADA_S - 1));
        assert!(Cuentas::vencida(&carla, 1_000_000 + PLAZO_RETIRADA_S));
        assert!(!Cuentas::vencida(&carla, 3));
        assert_eq!(c.devolver(b"Carla"), Ok(1001));
        assert!(c.para_entrar(b"Carla").is_some());
        // Con dos administradores, uno se puede retirar; y el administrador retirado ya no cuenta.
        c.meter(b"Carla", b"admin").unwrap();
        assert_eq!(c.admins(), 2);
        assert_eq!(c.retirar(b"Ana", 1001, 7), Ok(1000));
        assert!(!c.es_admin(1000) && c.admins() == 1);
        assert_eq!(c.retirar(b"Carla", 1001, 7), Err(Fallo::Propia));
        assert_eq!(c.retirar(b"Carla", 2000, 7), Err(Fallo::UltimoAdmin));
        assert_eq!(c.sacar(b"Carla", b"admin"), Err(Fallo::UltimoAdmin));
    }

    #[test]
    fn grupos_meter_y_sacar() {
        let (mut c, ..) = de(ANA_CARLA, ADMINS);
        assert_eq!(c.meter(b"Carla", b"fotos"), Ok((1001, 100)));
        assert_eq!(c.meter(b"Ana", b"fotos"), Ok((1000, 100)));
        assert_eq!(c.meter(b"Ana", b"fotos"), Err(Fallo::YaEsta));
        assert_eq!(c.meter(b"Ana", b"mus:ica"), Err(Fallo::NombreMalo));
        assert_eq!(c.meter(b"Nadie", b"fotos"), Err(Fallo::NoExiste));
        assert_eq!(c.meter(b"Carla", b"musica"), Ok((1001, 101)));
        assert_eq!(c.sacar(b"Ana", b"admin"), Err(Fallo::UltimoAdmin));
        assert_eq!(c.sacar(b"Carla", b"admin"), Err(Fallo::NoEsta));
        assert_eq!(c.sacar(b"Carla", b"nada"), Err(Fallo::NoExiste));
        let d = ida_y_vuelta(&c);
        assert_eq!(d.grupo(b"fotos").unwrap().miembros(), &[1001, 1000]);
        // Un grupo que se queda vacío, fuera (admin no).
        assert_eq!(c.sacar(b"Carla", b"musica"), Ok((1001, 101)));
        assert!(c.grupo(b"musica").is_none());
        // Quitar del todo a Carla la saca de sus grupos.
        c.retirar(b"Carla", 1000, 1).unwrap();
        let carla = c.quitar(1001).unwrap();
        assert_eq!(carla.volumen.as_str(), "Carla");
        assert_eq!(c.grupo(b"fotos").unwrap().miembros(), &[1000]);
        assert!(c.por_nombre(b"Carla").is_none());
        let d = ida_y_vuelta(&c);
        assert_eq!(d.todas().count(), 1);
    }

    #[test]
    fn basura_al_azar_no_rompe() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..5_000 {
            let mut u = Vec::new();
            let mut g = Vec::new();
            for v in [&mut u, &mut g] {
                for _ in 0..(x % 400) {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    v.push(match x % 9 {
                        0 => b'\n',
                        1 => b':',
                        2 => b',',
                        3 => b"Ana10Carlactivretirad "[(x >> 8) as usize % 22],
                        4 => b'0' + (x >> 9) as u8 % 10,
                        _ => (x >> 16) as u8,
                    });
                }
            }
            let (c, iu, ig) = Cuentas::leer(&u, &g);
            assert!(iu.buenas <= MAX_CUENTAS && ig.buenas <= MAX_GRUPOS);
            // Lo que se lee se escribe y se vuelve a leer igual.
            let d = ida_y_vuelta(&c);
            assert_eq!(c.todas().count(), d.todas().count());
            assert_eq!(c.grupos().count(), d.grupos().count());
        }
    }

    #[test]
    fn fallos_como_estados() {
        for f in FALLOS {
            assert_eq!(Fallo::de_estado(f.estado()), Some(f));
            assert!(f.estado() <= -101);
        }
        assert_eq!(Fallo::de_estado(0), None);
        assert_eq!(Fallo::de_estado(-12), None);
        assert_eq!(Fallo::de_estado(i32::MIN), None);
        assert_eq!(Fallo::de_estado(-200), None);
    }

    #[test]
    fn codigos_de_recuperacion() {
        let c = codigo_recuperacion(&[0; 15]);
        assert_eq!(&c, b"AAAA-AAAA-AAAA-AAAA-AAAA-AAAA");
        let c = codigo_recuperacion(&[0xff; 15]);
        assert_eq!(&c, b"9999-9999-9999-9999-9999-9999");
        let mut a = [0u8; 15];
        a[1] = 0b0100_0000; // el segundo trozo de 5 bits vale 1
        assert_eq!(&codigo_recuperacion(&a)[..4], b"ABAA");
        // Sin letras que se confunden, y distintos con azar distinto.
        let c = codigo_recuperacion(&core::array::from_fn(|i| (i * 37 + 11) as u8));
        assert!(c.iter().all(|&b| b == b'-' || LETRAS_CODIGO.contains(&b)) && !c.iter().any(|b| b"IO01".contains(b)));
        assert_ne!(c, codigo_recuperacion(&core::array::from_fn(|i| (i * 37 + 12) as u8)));
    }

    #[test]
    fn codigos_tecleados() {
        let c = *b"ABCD-EFGH-JKLM-NPQR-STUV-WX23";
        assert_eq!(normalizar_codigo(&c), Some(c));
        assert_eq!(normalizar_codigo(b"abcdefghjklmnpqrstuvwx23"), Some(c));
        assert_eq!(normalizar_codigo(b" abcd efgh-jklm  npqr-stuv wx23 "), Some(c));
        assert_eq!(normalizar_codigo(b"--ABCDEFGHJKLM----NPQRSTUVWX23-"), Some(c));
        // Letras que no están (O, I, 0, 1), de más, de menos, vacío, otras cosas.
        assert_eq!(normalizar_codigo(b"OBCD-EFGH-JKLM-NPQR-STUV-WX23"), None);
        assert_eq!(normalizar_codigo(b"1BCD-EFGH-JKLM-NPQR-STUV-WX23"), None);
        assert_eq!(normalizar_codigo(b"ABCD-EFGH-JKLM-NPQR-STUV-WX2"), None);
        assert_eq!(normalizar_codigo(b"ABCD-EFGH-JKLM-NPQR-STUV-WX233"), None);
        assert_eq!(normalizar_codigo(b""), None);
        assert_eq!(normalizar_codigo(b"ABCD:EFGH-JKLM-NPQR-STUV-WX23"), None);
        assert_eq!(normalizar_codigo("ÁBCD-EFGH-JKLM-NPQR-STUV-WX23".as_bytes()), None);
        assert_eq!(normalizar_codigo(&[b'A'; 5000]), None);
        // El de cualquier azar se lee igual.
        for k in 0..50u8 {
            let c = codigo_recuperacion(&core::array::from_fn(|i| (i as u8).wrapping_mul(k).wrapping_add(k)));
            assert_eq!(normalizar_codigo(&c), Some(c));
        }
    }

    #[test]
    fn esperas_crecientes() {
        let mut e = Esperas::nuevas();
        let s = 1_000_000;
        assert_eq!(e.falta(1000, 0), None);
        // Dos malas, sin espera; la tercera, 5 s; luego el doble, hasta 5 minutos.
        assert_eq!(e.mala(1000, 0), (1, 0));
        assert_eq!(e.mala(1000, 0), (2, 0));
        assert_eq!(e.falta(1000, 0), None);
        assert_eq!(e.mala(1000, 10 * s), (3, 5 * s));
        assert_eq!(e.falta(1000, 10 * s), Some(5 * s));
        assert_eq!(e.falta(1000, 14 * s), Some(s));
        assert_eq!(e.falta(1000, 15 * s), None);
        assert_eq!(e.mala(1000, 20 * s), (4, 10 * s));
        assert_eq!(e.mala(1000, 40 * s), (5, 20 * s));
        for _ in 0..40 {
            e.mala(1000, 100 * s);
        }
        assert_eq!(e.falta(1000, 100 * s), Some(300 * s));
        // Otra cuenta no espera por esta; una buena lo deja a cero.
        assert_eq!(e.falta(1001, 100 * s), None);
        e.buena(1000);
        assert_eq!(e.falta(1000, 100 * s), None);
        assert_eq!(e.mala(1000, 100 * s), (1, 0));
        // Muchas cuentas (más de las que caben): no se pierde la cuenta de nadie que ya estaba.
        let mut e = Esperas::nuevas();
        for _ in 0..3 {
            e.mala(1000, 0);
        }
        for u in 2000..2100 {
            e.mala(u, 0);
        }
        assert!(e.falta(1000, 0).is_some());
        // Números enormes de tiempo, sin desbordar.
        let mut e = Esperas::nuevas();
        for _ in 0..10 {
            e.mala(7, u64::MAX - 1);
        }
        assert_eq!(e.falta(7, u64::MAX - 1), Some(1));
    }
}
