//! Una tanda de acciones de administrador (S6, S6b y S36, decisión 1 del 2026-11-16: "una tanda es una acción").
//!
//! Un programa pide a `sesiones` una lista de acciones de administrador (comprobar el disco, cerrar varios programas,
//! leer el registro de todos...). `guardian` enseña la lista entera, en llano, y pide la contraseña de un administrador
//! una vez; `sesiones` hace solo las de la lista enseñada, en orden, cada una por el servicio que la tiene a su cargo.
//! Si una falla, se para: las de después no se hacen, y quien pidió sabe cuál falló y qué pasó con las demás.
//!
//! Por qué se para y no sigue: el usuario dijo que sí a la lista entera y en ese orden. Las de después pueden contar con
//! las de antes (crear un usuario y luego ponerle grupos; instalar lo que otro paquete necesita y luego ese paquete), y
//! seguir tras un fallo haría algo que nadie vio así. Lo hecho se queda hecho (cerrar un programa no se deshace).
//!
//! La lista llega en texto, una acción por línea: el nombre de la acción y, si lo lleva, su argumento (`matar 65540`).
//! Se lee entera antes de preguntar nada, y si una línea no vale (una acción que no es de administrador, un argumento
//! que falta o sobra, letras de control, una lista enorme), no se enseña ni se hace ninguna.
//!
//! Las cuentas de usuario (rama `cuentas`, S6b): `crear <nombre> <uid> [nombre completo]` (uid 0, el siguiente libre),
//! `borrar <nombre>` (la retira: se puede devolver unos días), `devolver <nombre>`, `meter <nombre> <grupo>` y
//! `sacar <nombre> <grupo>`. Una cuenta nueva por tanda como mucho: `guardian` pide su contraseña en la misma ventana.
//!
//! Sin `alloc` ni nada del sistema: se prueba en el PC con `rustc --edition 2024 --test usuario/src/tanda.rs` (con
//! `cuentas.rs` al lado).

#[cfg(test)]
#[path = "cuentas.rs"]
#[allow(dead_code)]
mod cuentas;
#[cfg(not(test))]
use crate::cuentas;

/// Lo más que mide la lista en texto (lo que llega en la memoria del programa). Lo de más no se lee: la tanda no vale.
pub const MAX_TEXTO: usize = 4096;
/// Lo más que mide una línea de la lista.
pub const MAX_LINEA: usize = 120;
/// Cuántas acciones caben en una tanda: las que se pueden enseñar enteras en la ventana de `guardian` y las que caben,
/// con su resultado, en la respuesta (16 + 12 por acción, dentro de los 256 bytes de un mensaje).
pub const MAX_ACCIONES: usize = 16;
/// Lo que mide la respuesta a quien pidió la tanda.
pub const LARGO_RESPUESTA: usize = 16 + 12 * MAX_ACCIONES;
/// Cuántas cuentas nuevas por tanda: una (su contraseña va en la ventana de `guardian`).
pub const MAX_CREAR: usize = 1;

/// Lo que lleva cada acción detrás de su nombre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argumento {
    Ninguno,
    /// Un número (el de un proceso).
    Numero,
    /// Una cuenta: su nombre.
    Cuenta,
    /// Una cuenta y un grupo.
    CuentaYGrupo,
    /// Una cuenta nueva: su nombre, su número (0, el siguiente libre) y, si lo lleva, su nombre completo (el resto de la
    /// línea).
    Crear,
}

/// Las acciones de administrador que pueden ir en una tanda: la lista cerrada de S6b que existe hoy (las de
/// `dashos::protocolo::sesiones::ACCIONES`, con lo que lleva cada una). Lo que no esté aquí no es de administrador.
/// Rama `cuentas`: y las de las cuentas de usuario.
pub const ACCIONES: &[(&str, Argumento)] = &[
    ("comprobar", Argumento::Ninguno),
    ("matar", Argumento::Numero),
    ("registro", Argumento::Ninguno),
    ("crear", Argumento::Crear),
    ("borrar", Argumento::Cuenta),
    ("devolver", Argumento::Cuenta),
    ("meter", Argumento::CuentaYGrupo),
    ("sacar", Argumento::CuentaYGrupo),
    // S32: instalar DashOS con un plan (su número: el CRC-32 de su texto). Lo hace el servicio `instalador`, que solo
    // está en vivo (arrancado del pendrive), y solo con el plan de ese número.
    ("instalar", Argumento::Numero),
];

/// Una acción ya leída: cuál (su sitio en `ACCIONES`) y su argumento: el número (de `matar`, o el de la cuenta de
/// `crear`; 0 si no lleva), la cuenta y lo otro (el grupo de `meter` y `sacar`, o el nombre completo de `crear`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accion {
    pub cual: usize,
    pub objetivo: u64,
    cuenta: [u8; cuentas::MAX_NOMBRE],
    lc: u8,
    otro: [u8; cuentas::MAX_COMPLETO],
    lo: u8,
}

impl Default for Accion {
    fn default() -> Self {
        Accion { cual: 0, objetivo: 0, cuenta: [0; cuentas::MAX_NOMBRE], lc: 0, otro: [0; cuentas::MAX_COMPLETO], lo: 0 }
    }
}

impl Accion {
    /// Una acción sin cuenta (comprobar, matar, registro).
    pub fn de(cual: usize, objetivo: u64) -> Accion {
        Accion { cual, objetivo, ..Accion::default() }
    }

    pub fn nombre(&self) -> &'static str {
        ACCIONES.get(self.cual).map_or("?", |a| a.0)
    }

    /// La cuenta de la acción (vacía si no lleva).
    pub fn cuenta(&self) -> &[u8] {
        &self.cuenta[..self.lc as usize]
    }

    /// El grupo (`meter`, `sacar`) o el nombre completo (`crear`).
    pub fn otro(&self) -> &[u8] {
        &self.otro[..self.lo as usize]
    }

    fn poner(&mut self, cuenta: &[u8], otro: &[u8]) {
        self.cuenta[..cuenta.len()].copy_from_slice(cuenta);
        self.lc = cuenta.len() as u8;
        self.otro[..otro.len()].copy_from_slice(otro);
        self.lo = otro.len() as u8;
    }
}

/// Lo que puede ser el nombre de una cuenta o de un grupo en la lista: lo de `cuentas::nombre_valido` sin mirar los
/// reservados (eso lo dice `sesiones`, con su motivo).
fn nombre_de_lista(n: &str) -> bool {
    !n.is_empty() && n.len() <= cuentas::MAX_NOMBRE && n.as_bytes()[0].is_ascii_alphabetic() && n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// Por qué no vale una lista. Las líneas se cuentan desde 1, sin las vacías.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mala {
    /// Sin ninguna acción.
    Vacia,
    /// Más de `MAX_TEXTO` bytes.
    Grande,
    /// Más de `MAX_ACCIONES` acciones (cuántas había, al menos).
    Demasiadas(usize),
    /// Una línea que no se entiende: larga, con letras de control, sin UTF-8 o con espacios de más.
    Linea(usize),
    /// Una acción que no es de administrador (no está en `ACCIONES`).
    NoEsDeAdministrador(usize),
    /// Un argumento que falta, que sobra o que no es un número.
    Argumento(usize),
    /// Más de una cuenta nueva (`MAX_CREAR`): la línea de la segunda.
    DosCuentas(usize),
}

impl core::fmt::Display for Mala {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Mala::Vacia => f.write_str("la lista está vacía"),
            Mala::Grande => write!(f, "la lista mide más de {} bytes", MAX_TEXTO),
            Mala::Demasiadas(n) => write!(f, "{} acciones o más: caben {}", n, MAX_ACCIONES),
            Mala::Linea(l) => write!(f, "la línea {} no se entiende", l),
            Mala::NoEsDeAdministrador(l) => write!(f, "la línea {} no es una acción de administrador", l),
            Mala::Argumento(l) => write!(f, "la línea {} no lleva lo que pide su acción", l),
            Mala::DosCuentas(l) => write!(f, "la línea {} crea otra cuenta: una por tanda", l),
        }
    }
}

/// Lee la lista `texto` en `sal`. Devuelve cuántas acciones hay, o por qué no vale (y entonces no se hace ninguna).
pub fn leer(texto: &[u8], sal: &mut [Accion; MAX_ACCIONES]) -> Result<usize, Mala> {
    if texto.len() > MAX_TEXTO {
        return Err(Mala::Grande);
    }
    let mut n = 0;
    let mut numero = 0;
    let mut crear = 0;
    for linea in texto.split(|&c| c == b'\n') {
        let linea = linea.strip_suffix(b"\r").unwrap_or(linea);
        if linea.iter().all(|&c| c == b' ') {
            continue;
        }
        numero += 1;
        if linea.len() > MAX_LINEA || linea.iter().any(|&c| c < 0x20 || c == 0x7f) {
            return Err(Mala::Linea(numero));
        }
        let Ok(linea) = core::str::from_utf8(linea) else { return Err(Mala::Linea(numero)) };
        // Un solo espacio entre las partes, ninguno delante ni detrás: lo que se enseña es justo lo que se lee. Lo que
        // va tras el número de `crear` (el nombre completo) es el resto de la línea, tal cual.
        let nombre = linea.split(' ').next().unwrap_or("");
        let Some(cual) = ACCIONES.iter().position(|a| a.0 == nombre) else {
            return Err(if nombre.is_empty() || linea.contains("  ") || linea.ends_with(' ') { Mala::Linea(numero) } else { Mala::NoEsDeAdministrador(numero) });
        };
        let cuantas = match ACCIONES[cual].1 {
            Argumento::Ninguno => 1,
            Argumento::Numero | Argumento::Cuenta => 2,
            Argumento::CuentaYGrupo => 3,
            Argumento::Crear => 4,
        };
        // Las partes: la de `crear`, hasta cuatro (el resto de la línea es el nombre completo); las demás, todas, y si
        // sobra alguna la línea no vale.
        let crea = ACCIONES[cual].1 == Argumento::Crear;
        let partes: [Option<&str>; 4] = {
            let it: &mut dyn Iterator<Item = &str> = if crea { &mut linea.splitn(4, ' ') } else { &mut linea.split(' ') };
            core::array::from_fn(|_| it.next())
        };
        let sobran = !crea && linea.split(' ').count() > cuantas.max(2);
        if sobran || partes.iter().flatten().any(|p| p.is_empty()) || partes[1..].iter().flatten().any(|p| p.starts_with(' ') || p.ends_with(' ')) {
            return Err(Mala::Linea(numero));
        }
        let numero_de = |a: &str| (a.bytes().all(|c| c.is_ascii_digit()) && a.len() <= 19).then(|| a.parse::<u64>().ok()).flatten();
        let mut accion = Accion::de(cual, 0);
        match (ACCIONES[cual].1, partes[1], partes[2], partes[3]) {
            (Argumento::Ninguno, None, ..) => {}
            (Argumento::Numero, Some(a), None, _) => accion.objetivo = numero_de(a).ok_or(Mala::Argumento(numero))?,
            (Argumento::Cuenta, Some(c), None, _) if nombre_de_lista(c) => accion.poner(c.as_bytes(), b""),
            (Argumento::CuentaYGrupo, Some(c), Some(g), None) if nombre_de_lista(c) && nombre_de_lista(g) => accion.poner(c.as_bytes(), g.as_bytes()),
            (Argumento::Crear, Some(c), Some(u), completo) if nombre_de_lista(c) && cuentas::completo_valido(completo.unwrap_or("").as_bytes()) => {
                accion.objetivo = numero_de(u).filter(|&u| u <= u32::MAX as u64).ok_or(Mala::Argumento(numero))?;
                accion.poner(c.as_bytes(), completo.unwrap_or("").as_bytes());
                crear += 1;
                if crear > MAX_CREAR {
                    return Err(Mala::DosCuentas(numero));
                }
            }
            _ => return Err(Mala::Argumento(numero)),
        }
        if n == MAX_ACCIONES {
            return Err(Mala::Demasiadas(n + 1));
        }
        sal[n] = accion;
        n += 1;
    }
    if n == 0 { Err(Mala::Vacia) } else { Ok(n) }
}

/// La frase en llano de una acción, para la ventana de `guardian`, el registro y la orden `Administrar`. `programa` y
/// `persona` son los del proceso que cierra `matar` (vacíos en las demás). Lo que llega de fuera, sin letras de control.
pub struct Frase<'a> {
    pub accion: Accion,
    pub programa: &'a str,
    pub persona: &'a str,
}

fn limpio(f: &mut core::fmt::Formatter<'_>, t: &str) -> core::fmt::Result {
    for c in t.chars() {
        f.write_fmt(format_args!("{}", if c.is_control() { '?' } else { c }))?;
    }
    Ok(())
}

impl core::fmt::Display for Frase<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let a = &self.accion;
        // Los nombres de cuenta y de grupo ya son ASCII sin nada raro (`leer`); el nombre completo puede llevar de todo
        // menos letras de control y dos puntos, y va entre paréntesis.
        let cuenta = core::str::from_utf8(a.cuenta()).unwrap_or("?");
        let otro = core::str::from_utf8(a.otro()).unwrap_or("?");
        match a.nombre() {
            "crear" => {
                write!(f, "Crear la cuenta \"{}\"", cuenta)?;
                if !otro.is_empty() {
                    f.write_str(" (")?;
                    limpio(f, otro)?;
                    f.write_str(")")?;
                }
                match a.objetivo {
                    0 => f.write_str(", con el primer número libre y su volumen cifrado"),
                    u => write!(f, ", número {}, con su volumen cifrado", u),
                }
            }
            "borrar" => write!(f, "Borrar la cuenta \"{}\" (se puede devolver durante {} días)", cuenta, cuentas::PLAZO_RETIRADA_S / 86400),
            "devolver" => write!(f, "Devolver la cuenta \"{}\", que estaba borrada", cuenta),
            "meter" if otro == cuentas::ADMIN => write!(f, "Hacer administradora a \"{}\" (grupo admin)", cuenta),
            "meter" => write!(f, "Poner a \"{}\" en el grupo \"{}\"", cuenta, otro),
            "sacar" if otro == cuentas::ADMIN => write!(f, "Quitar a \"{}\" de administradora (grupo admin)", cuenta),
            "sacar" => write!(f, "Quitar a \"{}\" del grupo \"{}\"", cuenta, otro),
            "comprobar" => f.write_str("Comprobar el disco"),
            "instalar" => write!(f, "Instalar DashOS en el disco, con el plan {} (lo que dice el plan se escribe; lo de otros sistemas no se toca)", a.objetivo),
            "registro" => f.write_str("Leer el registro de todos los usuarios"),
            // Sin el nombre (quien lo escribe no lo sabe: la orden `Administrar`), por su número.
            "matar" if self.programa.is_empty() => write!(f, "Cerrar el proceso {}", self.accion.objetivo),
            "matar" => {
                f.write_str("Cerrar \"")?;
                limpio(f, self.programa)?;
                f.write_str("\"")?;
                if !self.persona.is_empty() {
                    f.write_str(", de ")?;
                    limpio(f, self.persona)?;
                }
                write!(f, " (proceso {})", self.accion.objetivo)
            }
            x => write!(f, "\"{}\"", x),
        }
    }
}

/// La respuesta a quien pidió la tanda: el estado (0 si se hicieron todas; si no, el de la que falló, o el de no
/// confirmarse), cuántas se hicieron, cuántas había, en cuál se paró (1 en adelante; 0 si en ninguna) y, de cada una,
/// su estado y su dato. Las que no se llegaron a hacer llevan `NO_HECHA`.
pub const NO_HECHA: i32 = i32::MIN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resultado {
    pub estado: i32,
    pub hechas: usize,
    pub total: usize,
    pub parada: usize,
    pub de_cada: [(i32, u64); MAX_ACCIONES],
}

impl Resultado {
    pub fn nuevo(total: usize) -> Self {
        Resultado { estado: 0, hechas: 0, total: total.min(MAX_ACCIONES), parada: 0, de_cada: [(NO_HECHA, 0); MAX_ACCIONES] }
    }

    pub fn en_bytes(&self) -> [u8; LARGO_RESPUESTA] {
        let mut r = [0u8; LARGO_RESPUESTA];
        r[0..4].copy_from_slice(&self.estado.to_le_bytes());
        r[4..8].copy_from_slice(&(self.hechas as u32).to_le_bytes());
        r[8..12].copy_from_slice(&(self.total as u32).to_le_bytes());
        r[12..16].copy_from_slice(&(self.parada as u32).to_le_bytes());
        for (i, (e, d)) in self.de_cada.iter().enumerate() {
            let o = 16 + 12 * i;
            r[o..o + 4].copy_from_slice(&e.to_le_bytes());
            r[o + 4..o + 12].copy_from_slice(&d.to_le_bytes());
        }
        r
    }

    /// Desde lo que llega; lo que no cuadre (corto, cuentas que no caben), `None`.
    pub fn de_bytes(r: &[u8]) -> Option<Self> {
        let u = |i: usize| r.get(i..i + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()));
        let (estado, hechas, total, parada) = (u(0)? as i32, u(4)? as usize, u(8)? as usize, u(12)? as usize);
        if total > MAX_ACCIONES || hechas > total || parada > total || r.len() < 16 + 12 * total {
            return None;
        }
        let mut de_cada = [(NO_HECHA, 0); MAX_ACCIONES];
        for (i, x) in de_cada.iter_mut().enumerate().take(total) {
            let o = 16 + 12 * i;
            *x = (u(o)? as i32, u64::from_le_bytes(r[o + 4..o + 12].try_into().unwrap()));
        }
        Some(Resultado { estado, hechas, total, parada, de_cada })
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn lee(t: &str) -> Result<([Accion; MAX_ACCIONES], usize), Mala> {
        let mut a = [Accion::default(); MAX_ACCIONES];
        leer(t.as_bytes(), &mut a).map(|n| (a, n))
    }

    #[test]
    fn las_acciones_son_las_de_sesiones() {
        let nombres: [&str; 9] = ["comprobar", "matar", "registro", "crear", "borrar", "devolver", "meter", "sacar", "instalar"];
        assert_eq!(ACCIONES.iter().map(|a| a.0).collect::<std::vec::Vec<_>>(), nombres);
    }

    #[test]
    fn una_lista_buena() {
        let (a, n) = lee("comprobar\nmatar 65540\n\nregistro\r\nmatar 7").unwrap();
        assert_eq!(n, 4);
        assert_eq!(a[0], Accion::de(0, 0));
        assert_eq!(a[1], Accion::de(1, 65540));
        assert_eq!(a[2].nombre(), "registro");
        assert_eq!(a[3].objetivo, 7);
        // La misma acción dos veces vale: se hace dos veces, en orden.
        assert_eq!(lee("matar 1\nmatar 1").unwrap().1, 2);
    }

    #[test]
    fn lo_que_no_vale() {
        assert_eq!(lee("").unwrap_err(), Mala::Vacia);
        assert_eq!(lee("\n \n").unwrap_err(), Mala::Vacia);
        // Lo que no es de administrador: ni lo de un programa cualquiera ni un nombre parecido.
        for no in ["bloquear", "dar red", "Matar 3", "comprobar2", "apagar", "lanzar Programas/hola", "permiso"] {
            assert_eq!(lee(no).unwrap_err(), Mala::NoEsDeAdministrador(1), "{}", no);
        }
        // Una mala en medio tira la lista entera (con su número de línea, sin contar las vacías).
        assert_eq!(lee("registro\n\nbloquear\ncomprobar").unwrap_err(), Mala::NoEsDeAdministrador(2));
        // Argumentos que faltan, sobran o no son números.
        for (malo, linea) in [("matar", 1), ("comprobar 3", 1), ("registro x", 1), ("matar -1", 1), ("matar 0x10", 1), ("matar 1e3", 1), ("registro\nmatar 99999999999999999999", 2)] {
            assert_eq!(lee(malo).unwrap_err(), Mala::Argumento(linea), "{}", malo);
        }
        // Lo que no se entiende: espacios de más, tres partes, letras de control, escapes, no UTF-8, larga.
        for malo in ["matar  3", " matar 3", "matar 3 ", "matar 3 4", "comprobar\t", "registro\x1b[2J", "matar 3\0"] {
            assert_eq!(lee(malo).unwrap_err(), Mala::Linea(1), "{:?}", malo);
        }
        let mut a = [Accion::default(); MAX_ACCIONES];
        assert_eq!(leer(b"matar \xff", &mut a).unwrap_err(), Mala::Linea(1));
        let larga = std::format!("matar {}", "1".repeat(MAX_LINEA));
        assert_eq!(lee(&larga).unwrap_err(), Mala::Linea(1));
    }

    #[test]
    fn acciones_de_cuentas() {
        let (a, n) = lee("crear Berta 0 Berta Gil Núñez\nmeter Berta admin\nsacar Carla fotos\nborrar Carla\ndevolver Carla").unwrap();
        assert_eq!(n, 5);
        assert_eq!((a[0].nombre(), a[0].cuenta(), a[0].otro(), a[0].objetivo), ("crear", &b"Berta"[..], "Berta Gil Núñez".as_bytes(), 0));
        assert_eq!((a[1].nombre(), a[1].cuenta(), a[1].otro()), ("meter", &b"Berta"[..], &b"admin"[..]));
        assert_eq!((a[3].nombre(), a[3].cuenta(), a[3].otro()), ("borrar", &b"Carla"[..], &b""[..]));
        // Sin nombre completo, y con un número pedido.
        let (a, _) = lee("crear Dora 1500").unwrap();
        assert_eq!((a[0].cuenta(), a[0].otro(), a[0].objetivo), (&b"Dora"[..], &b""[..], 1500));
        // Lo hostil: nombres con dos puntos, saltos de línea (otra línea, que no es una acción), largos, con espacios de
        // más, números que no lo son, una segunda cuenta nueva.
        for (malo, mala) in [
            ("crear a:b 0", Mala::Argumento(1)),
            ("crear Ana 0 con: dos puntos", Mala::Argumento(1)),
            ("crear Ana 0\nAna:1000:1000:x:Ana:activa", Mala::NoEsDeAdministrador(2)),
            ("crear diecisieteletrasx 0", Mala::Argumento(1)),
            ("crear Ana", Mala::Argumento(1)),
            ("crear Ana x", Mala::Argumento(1)),
            ("crear Ana -1", Mala::Argumento(1)),
            ("crear Ana 99999999999", Mala::Argumento(1)),
            ("crear Ana 0 ", Mala::Linea(1)),
            ("crear Ana 0  doble", Mala::Linea(1)),
            ("crear Peña 0", Mala::Argumento(1)),
            ("borrar", Mala::Argumento(1)),
            ("borrar Ana Carla", Mala::Linea(1)),
            ("borrar 1ana", Mala::Argumento(1)),
            ("meter Ana", Mala::Argumento(1)),
            ("meter Ana ad:min", Mala::Argumento(1)),
            ("sacar Ana admin x", Mala::Linea(1)),
            ("crear Eva 0\ncrear Gil 0", Mala::DosCuentas(2)),
        ] {
            assert_eq!(lee(malo).unwrap_err(), mala, "{:?}", malo);
        }
        // Los reservados pasan la lista: los dice `sesiones`, con su motivo.
        assert!(lee("crear SYS 0").is_ok());
    }

    #[test]
    fn frases_de_cuentas() {
        let f = |t: &str| {
            let (a, _) = lee(t).unwrap();
            std::format!("{}", Frase { accion: a[0], programa: "", persona: "" })
        };
        assert_eq!(f("crear Berta 0 Berta Gil"), "Crear la cuenta \"Berta\" (Berta Gil), con el primer número libre y su volumen cifrado");
        assert_eq!(f("crear Berta 1002"), "Crear la cuenta \"Berta\", número 1002, con su volumen cifrado");
        assert_eq!(f("borrar Carla"), "Borrar la cuenta \"Carla\" (se puede devolver durante 30 días)");
        assert_eq!(f("devolver Carla"), "Devolver la cuenta \"Carla\", que estaba borrada");
        assert_eq!(f("meter Carla admin"), "Hacer administradora a \"Carla\" (grupo admin)");
        assert_eq!(f("meter Carla fotos"), "Poner a \"Carla\" en el grupo \"fotos\"");
        assert_eq!(f("sacar Carla admin"), "Quitar a \"Carla\" de administradora (grupo admin)");
        assert_eq!(f("sacar Carla fotos"), "Quitar a \"Carla\" del grupo \"fotos\"");
    }

    #[test]
    fn listas_enormes() {
        let justa = "registro\n".repeat(MAX_ACCIONES);
        assert_eq!(lee(&justa).unwrap().1, MAX_ACCIONES);
        let una_mas = "registro\n".repeat(MAX_ACCIONES + 1);
        assert_eq!(lee(&una_mas).unwrap_err(), Mala::Demasiadas(MAX_ACCIONES + 1));
        // Más de lo que se lee: ni se mira.
        let enorme = "\n".repeat(MAX_TEXTO + 1);
        assert_eq!(lee(&enorme).unwrap_err(), Mala::Grande);
        let mut a = [Accion::default(); MAX_ACCIONES];
        assert_eq!(leer(&std::vec![b'x'; 1 << 20], &mut a).unwrap_err(), Mala::Grande);
    }

    #[test]
    fn basura_al_azar_no_rompe() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut a = [Accion::default(); MAX_ACCIONES];
        for _ in 0..20_000 {
            let mut b = std::vec::Vec::new();
            for _ in 0..(x % 300) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                b.push(match x % 7 {
                    0 => b'\n',
                    1 => b' ',
                    2 => b"matar"[(x >> 8) as usize % 5],
                    3 => b'0' + (x >> 9) as u8 % 10,
                    _ => (x >> 16) as u8,
                });
            }
            if let Ok(n) = leer(&b, &mut a) {
                assert!(n >= 1 && n <= MAX_ACCIONES);
                assert!(a[..n].iter().all(|a| a.cual < ACCIONES.len()));
            }
        }
    }

    #[test]
    fn frases_en_llano() {
        let f = |a: Accion, p: &str, q: &str| std::format!("{}", Frase { accion: a, programa: p, persona: q });
        assert_eq!(f(Accion::de(0, 0), "", ""), "Comprobar el disco");
        assert_eq!(f(Accion::de(2, 0), "", ""), "Leer el registro de todos los usuarios");
        assert_eq!(f(Accion::de(1, 65540), "espera", "Ana"), "Cerrar \"espera\", de Ana (proceso 65540)");
        assert_eq!(f(Accion::de(1, 3), "eco", ""), "Cerrar \"eco\" (proceso 3)");
        assert_eq!(f(Accion::de(1, 3), "", ""), "Cerrar el proceso 3");
        // Un nombre con escapes no pinta nada raro.
        assert_eq!(f(Accion::de(1, 3), "x\x1b[2J", "A\nB"), "Cerrar \"x?[2J\", de A?B (proceso 3)");
    }

    #[test]
    fn respuesta_ida_y_vuelta() {
        let mut r = Resultado::nuevo(3);
        r.estado = -1;
        r.hechas = 1;
        r.parada = 2;
        r.de_cada[0] = (0, 65540);
        r.de_cada[1] = (-1, 9);
        let b = r.en_bytes();
        assert_eq!(Resultado::de_bytes(&b), Some(r));
        assert_eq!(Resultado::de_bytes(&b).unwrap().de_cada[2].0, NO_HECHA);
        // Lo que no cuadra.
        assert_eq!(Resultado::de_bytes(&b[..10]), None);
        let mut mal = b;
        mal[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(Resultado::de_bytes(&mal), None);
        let mut mal = b;
        mal[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(Resultado::de_bytes(&mal), None);
    }
}
