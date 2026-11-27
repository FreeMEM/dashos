//! Por qué no se hace una transacción de Paq, en llano, y su número: el que contesta el servicio `paq` a `sesiones` en
//! una tanda (de -301 en adelante, para no chocar con los de `archivos`, `sesiones` y las cuentas), con el que la orden
//! `Paq` dice qué pasó.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// El servicio de archivos dijo que no (el estado y la ruta).
    Disco(i32, String),
    /// Otro `Paq` está trabajando en ese sitio.
    Ocupado(String),
    /// Una firma que no vale (de qué).
    Firma(String),
    /// Un paquete que no es el del índice: otra huella u otro tamaño (cuál).
    Huella(String),
    /// Un paquete que no se instala: su nombre y por qué (rutas que se salen, enlaces, el manifiesto...).
    Paquete(String, String),
    /// El plan no vale: lo que rompería, lo que falta, lo que choca (una línea cada cosa).
    Plan(Vec<String>),
    /// No está en ningún repositorio (o no esa versión).
    NoEsta(String),
    /// Lo instalado cambió desde que se hizo el plan.
    PlanViejo(String),
    /// No cabe en el disco.
    Lleno(String),
    /// Lo de Paq en el disco está estropeado.
    Danado(String),
    /// Algo que Paq aún no hace.
    AunNo(String),
    /// Un índice que no vale (cortado, de otro repositorio, caducado, más viejo que el último visto).
    Indice(String),
}

/// Los números, en el orden de arriba.
const NUMEROS: [i32; 12] = [-301, -302, -303, -304, -305, -306, -307, -308, -309, -310, -311, -312];

impl Fallo {
    pub fn estado(&self) -> i32 {
        let i = match self {
            Fallo::Disco(..) => 0,
            Fallo::Ocupado(_) => 1,
            Fallo::Firma(_) => 2,
            Fallo::Huella(_) => 3,
            Fallo::Paquete(..) => 4,
            Fallo::Plan(_) => 5,
            Fallo::NoEsta(_) => 6,
            Fallo::PlanViejo(_) => 7,
            Fallo::Lleno(_) => 8,
            Fallo::Danado(_) => 9,
            Fallo::AunNo(_) => 10,
            Fallo::Indice(_) => 11,
        };
        NUMEROS[i]
    }
}

/// Lo que dice un estado de Paq sin el detalle (el de una tanda: el detalle lo apunta el servicio en el registro).
pub fn texto_de_estado(e: i32) -> Option<&'static str> {
    Some(match e {
        -301 => "el disco no dejó hacerlo",
        -302 => "otro Paq está trabajando",
        -303 => "una firma no vale",
        -304 => "un paquete no es el que dice el índice (está cambiado)",
        -305 => "un paquete está mal hecho y no se instala",
        -306 => "el plan no se puede hacer",
        -307 => "no está en ningún repositorio",
        -308 => "lo instalado cambió desde que se hizo el plan: pídelo otra vez",
        -309 => "no cabe en el disco",
        -310 => "lo de Paq en el disco está estropeado",
        -311 => "Paq aún no sabe hacer eso",
        -312 => "un índice no vale",
        _ => return None,
    })
}

impl core::fmt::Display for Fallo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Fallo::Disco(e, r) => write!(f, "no se puede con {r} (error {e} del disco)"),
            Fallo::Ocupado(r) => write!(f, "otro Paq está trabajando en {r}; espera a que acabe"),
            Fallo::Firma(q) => write!(f, "la firma de {q} no vale: no se toca nada"),
            Fallo::Huella(q) => write!(f, "{q} no es el paquete que dice el índice (está cambiado): no se toca nada"),
            Fallo::Paquete(n, por) => write!(f, "el paquete {n} no se instala: {por}"),
            Fallo::Plan(l) => {
                f.write_str("no se puede: ")?;
                for (i, x) in l.iter().enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    f.write_str(x)?;
                }
                Ok(())
            }
            Fallo::NoEsta(n) => write!(f, "no hay ningún paquete {n} en los repositorios"),
            Fallo::PlanViejo(q) => write!(f, "lo instalado cambió desde que se hizo el plan ({q}): pídelo otra vez"),
            Fallo::Lleno(q) => write!(f, "no cabe: {q}"),
            Fallo::Danado(q) => write!(f, "lo de Paq está estropeado: {q}"),
            Fallo::AunNo(q) => write!(f, "Paq aún no sabe {q}"),
            Fallo::Indice(q) => write!(f, "{q}"),
        }
    }
}
