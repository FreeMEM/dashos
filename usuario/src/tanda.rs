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
//! Sin `alloc` ni nada del sistema: se prueba en el PC con `rustc --edition 2024 --test usuario/src/tanda.rs`.

/// Lo más que mide la lista en texto (lo que llega en la memoria del programa). Lo de más no se lee: la tanda no vale.
pub const MAX_TEXTO: usize = 4096;
/// Lo más que mide una línea de la lista.
pub const MAX_LINEA: usize = 120;
/// Cuántas acciones caben en una tanda: las que se pueden enseñar enteras en la ventana de `guardian` y las que caben,
/// con su resultado, en la respuesta (16 + 12 por acción, dentro de los 256 bytes de un mensaje).
pub const MAX_ACCIONES: usize = 16;
/// Lo que mide la respuesta a quien pidió la tanda.
pub const LARGO_RESPUESTA: usize = 16 + 12 * MAX_ACCIONES;

/// Lo que lleva cada acción detrás de su nombre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argumento {
    Ninguno,
    /// Un número (el de un proceso).
    Numero,
}

/// Las acciones de administrador que pueden ir en una tanda: la lista cerrada de S6b que existe hoy (las de
/// `dashos::protocolo::sesiones::ACCIONES`, con lo que lleva cada una). Lo que no esté aquí no es de administrador.
pub const ACCIONES: &[(&str, Argumento)] = &[("comprobar", Argumento::Ninguno), ("matar", Argumento::Numero), ("registro", Argumento::Ninguno)];

/// Una acción ya leída: cuál (su sitio en `ACCIONES`) y su argumento (0 si no lleva).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accion {
    pub cual: usize,
    pub objetivo: u64,
}

impl Accion {
    pub fn nombre(&self) -> &'static str {
        ACCIONES.get(self.cual).map_or("?", |a| a.0)
    }
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
        let mut partes = linea.split(' ');
        let nombre = partes.next().unwrap_or("");
        let arg = partes.next();
        // Un solo espacio entre las dos partes, y nada más detrás: lo que se enseña es justo lo que se lee.
        if nombre.is_empty() || partes.next().is_some() || arg == Some("") {
            return Err(Mala::Linea(numero));
        }
        let Some(cual) = ACCIONES.iter().position(|a| a.0 == nombre) else { return Err(Mala::NoEsDeAdministrador(numero)) };
        let objetivo = match (ACCIONES[cual].1, arg) {
            (Argumento::Ninguno, None) => 0,
            (Argumento::Numero, Some(a)) if a.bytes().all(|c| c.is_ascii_digit()) && a.len() <= 19 => a.parse::<u64>().map_err(|_| Mala::Argumento(numero))?,
            _ => return Err(Mala::Argumento(numero)),
        };
        if n == MAX_ACCIONES {
            return Err(Mala::Demasiadas(n + 1));
        }
        sal[n] = Accion { cual, objetivo };
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
        match self.accion.nombre() {
            "comprobar" => f.write_str("Comprobar el disco"),
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
            otro => write!(f, "\"{}\"", otro),
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
        let nombres: [&str; 3] = ["comprobar", "matar", "registro"];
        assert_eq!(ACCIONES.iter().map(|a| a.0).collect::<std::vec::Vec<_>>(), nombres);
    }

    #[test]
    fn una_lista_buena() {
        let (a, n) = lee("comprobar\nmatar 65540\n\nregistro\r\nmatar 7").unwrap();
        assert_eq!(n, 4);
        assert_eq!(a[0], Accion { cual: 0, objetivo: 0 });
        assert_eq!(a[1], Accion { cual: 1, objetivo: 65540 });
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
        assert_eq!(f(Accion { cual: 0, objetivo: 0 }, "", ""), "Comprobar el disco");
        assert_eq!(f(Accion { cual: 2, objetivo: 0 }, "", ""), "Leer el registro de todos los usuarios");
        assert_eq!(f(Accion { cual: 1, objetivo: 65540 }, "espera", "Ana"), "Cerrar \"espera\", de Ana (proceso 65540)");
        assert_eq!(f(Accion { cual: 1, objetivo: 3 }, "eco", ""), "Cerrar \"eco\" (proceso 3)");
        assert_eq!(f(Accion { cual: 1, objetivo: 3 }, "", ""), "Cerrar el proceso 3");
        // Un nombre con escapes no pinta nada raro.
        assert_eq!(f(Accion { cual: 1, objetivo: 3 }, "x\x1b[2J", "A\nB"), "Cerrar \"x?[2J\", de A?B (proceso 3)");
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
