//! La guardia de los ficheros de la caja (S28, `docs/diseño/S28-malware-amiga.md`): un `SistemaArchivos` que va
//! delante del de verdad y niega lo que hacían los virus del Amiga, aunque el servicio lo dejara.
//!
//! - **Volúmenes de solo lectura** (decisión 8 del usuario, 2026-11-13): `Dos::montar` con `solo_lectura` marca su
//!   raíz aquí, y todo lo que cambia el disco desde ella (o desde algo abierto desde ella) da `SoloLectura`, que dos
//!   convierte en `ERROR_DISK_WRITE_PROTECTED`. Sin mirar lo que diga el servicio.
//! - **Lo que no se cambia** aunque el volumen se pueda escribir, si ya existía (lo creado por esta asa, sí):
//!   - ejecutables de AmigaOS (empiezan por `$000003F3`, `HUNK_HEADER`): los virus de enlace se pegaban a ellos;
//!   - iconos (`.info`): un troyano cambiaba su herramienta por defecto;
//!   - los guiones de arranque (`Startup-Sequence`, `User-Startup`...) y `Disk-Validator`: ni cambiarlos, ni crearlos,
//!     ni ponerle ese nombre a otro fichero; los virus se metían ahí para arrancar con el sistema;
//!   - el bloque de arranque de las imágenes de disco (los primeros 1024 bytes de un `.adf`, los 8 KB de un `.hdf`,
//!     con la RDB): el resto de la imagen sí se escribe.
//!
//!   Escribir, recortar, borrar, renombrar o cambiar la protección de uno de esos da `Protegido`
//!   (`ERROR_WRITE_PROTECTED` o `ERROR_DELETE_PROTECTED`).
//!
//! Cada negativa se apunta (`tomar_negados`) para que la caja la diga. Lo que se lee pasa sin tocar.
//!
//! **Preguntar** (decisión del usuario del 2026-11-13): con un `Preguntador`, lo que no se cambia se le pregunta al
//! usuario (por `guardian`) en vez de negarlo, una vez por programa 68k y clase: la respuesta se recuerda mientras viva
//! la caja (y, si el usuario lo dice, la recuerda `sesiones`). Sin `Preguntador`, se niega. Lo de solo lectura no se
//! pregunta nunca.
//!
//! Lo que no hace (S28): versiones o papelera de lo que se borra o se escribe, cupos de espacio.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::{Asa, Datos, ErrorFs, InfoVolumen, Pagina, SistemaArchivos, Tipo};

/// Las asas 0 a 15 son raíces.
const RAICES: Asa = 16;
/// Los primeros 4 bytes de un ejecutable de AmigaOS: `HUNK_HEADER`.
pub const MAGIA_HUNK: [u8; 4] = [0, 0, 3, 0xF3];
/// El bloque de arranque de un disquete: los dos primeros sectores.
pub const ARRANQUE_ADF: u64 = 1024;
/// En un disco duro (HDF), la RDB va en los primeros 16 bloques.
pub const ARRANQUE_HDF: u64 = 8192;
/// Nombres de guiones y gestores que AmigaOS ejecuta solo al arrancar o al meter un disco.
const DE_ARRANQUE: &[&str] =
    &["startup-sequence", "user-startup", "shell-startup", "cli-startup", "network-startup", "disk-validator"];
/// Negativas que se guardan hasta que la caja las recoge; las de más solo se cuentan.
const MAX_NEGADOS: usize = 64;

/// Qué es un fichero para la guardia.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Clase {
    Normal,
    Ejecutable,
    Icono,
    Arranque,
    /// Una imagen de disco, con lo que mide su bloque de arranque.
    Imagen(u64),
}

impl Clase {
    /// Lo que no se cambia nunca si ya existe.
    fn intocable(self) -> bool {
        matches!(self, Clase::Ejecutable | Clase::Icono | Clase::Arranque)
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Clase::Normal => "fichero",
            Clase::Ejecutable => "ejecutable",
            Clase::Icono => "icono",
            Clase::Arranque => "guion de arranque",
            Clase::Imagen(_) => "bloque de arranque de una imagen de disco",
        }
    }
}

/// Lo que dice el nombre de un fichero (sin mirar dentro).
pub fn clase_por_nombre(nombre: &str) -> Clase {
    let n = nombre.to_ascii_lowercase();
    if DE_ARRANQUE.contains(&n.as_str()) {
        Clase::Arranque
    } else if n.ends_with(".info") {
        Clase::Icono
    } else if n.ends_with(".adf") {
        Clase::Imagen(ARRANQUE_ADF)
    } else if n.ends_with(".hdf") {
        Clase::Imagen(ARRANQUE_HDF)
    } else {
        Clase::Normal
    }
}

/// A quién se pregunta (S28): en la caja, al usuario por `sesiones` y `guardian`.
pub trait Preguntador {
    /// Si el programa 68k `programa` puede cambiar `nombre`, que es de la clase `clase`. `false` también si nadie
    /// contesta.
    fn preguntar(&mut self, programa: &str, clase: Clase, nombre: &str) -> bool;
}

/// Una negativa: la orden, el nombre del objeto y por qué.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negado {
    pub orden: &'static str,
    pub nombre: String,
    pub motivo: &'static str,
}

#[derive(Clone, Debug)]
struct InfoAsa {
    /// La raíz por la que se llegó.
    raiz: Asa,
    /// El último nombre del camino ("" para una raíz).
    nombre: String,
    /// Creada por esta asa: lo nuevo se puede escribir hasta que se cierra.
    nueva: bool,
    cajon: bool,
    /// Lo que es, cuando ya se ha mirado.
    clase: Option<Clase>,
}

pub struct Guardia<F: SistemaArchivos> {
    pub dentro: F,
    solo_lectura: BTreeSet<Asa>,
    asas: BTreeMap<Asa, InfoAsa>,
    negados: Vec<Negado>,
    /// Todas las negativas desde el principio.
    pub total_negados: u64,
    /// A quién preguntar; sin él, se niega.
    preguntador: Option<Box<dyn Preguntador>>,
    /// El programa 68k que corre (lo pone la caja al empezar cada uno).
    programa: String,
    /// Lo contestado: (programa, clase) → sí o no.
    respuestas: BTreeMap<(String, Clase), bool>,
    /// Cuántas veces se ha preguntado.
    pub preguntas: u64,
}

fn ultimo(camino: &str) -> &str {
    camino.rsplit('/').next().unwrap_or("")
}

impl<F: SistemaArchivos> Guardia<F> {
    pub fn nueva(dentro: F) -> Guardia<F> {
        Guardia {
            dentro,
            solo_lectura: BTreeSet::new(),
            asas: BTreeMap::new(),
            negados: Vec::new(),
            total_negados: 0,
            preguntador: None,
            programa: String::new(),
            respuestas: BTreeMap::new(),
            preguntas: 0,
        }
    }

    /// A quién preguntar desde ahora (sin él, se niega).
    pub fn poner_preguntador(&mut self, p: Box<dyn Preguntador>) {
        self.preguntador = Some(p);
    }

    /// El programa 68k que empieza a correr: lo que se pregunte va por él.
    pub fn programa(&mut self, nombre: &str) {
        self.programa = String::from(nombre);
    }

    /// Si se deja cambiar algo de la clase `clase` (`nombre`) al programa que corre: lo recordado o, si no, lo que
    /// conteste el preguntador (y se recuerda).
    fn dejar(&mut self, clase: Clase, nombre: &str) -> bool {
        let clave = (self.programa.clone(), clase);
        if let Some(&r) = self.respuestas.get(&clave) {
            return r;
        }
        let Some(p) = self.preguntador.as_mut() else { return false };
        self.preguntas += 1;
        let r = p.preguntar(&clave.0, clase, nombre);
        self.respuestas.insert(clave, r);
        r
    }

    /// Las negativas desde la última vez.
    pub fn tomar_negados(&mut self) -> Vec<Negado> {
        core::mem::take(&mut self.negados)
    }

    /// Asas que la guardia sigue (abiertas, sin contar las raíces).
    pub fn asas_seguidas(&self) -> usize {
        self.asas.len()
    }

    fn negar(&mut self, orden: &'static str, nombre: &str, motivo: &'static str, e: ErrorFs) -> ErrorFs {
        self.total_negados += 1;
        if self.negados.len() < MAX_NEGADOS {
            self.negados.push(Negado { orden, nombre: String::from(nombre), motivo });
        }
        e
    }

    /// La raíz de una asa; `None` si la guardia no la dio (el servicio diría `AsaMala`).
    fn raiz_de(&self, asa: Asa) -> Option<Asa> {
        if asa < RAICES { Some(asa) } else { self.asas.get(&asa).map(|i| i.raiz) }
    }

    fn nombre_de(&self, asa: Asa) -> String {
        self.asas.get(&asa).map(|i| i.nombre.clone()).unwrap_or_default()
    }

    /// Que se pueda cambiar algo por `asa`: la raíz, conocida y de escribir.
    fn escribible(&mut self, asa: Asa, orden: &'static str, nombre: &str) -> Result<(), ErrorFs> {
        match self.raiz_de(asa) {
            None => Err(ErrorFs::AsaMala),
            Some(r) if self.solo_lectura.contains(&r) => {
                Err(self.negar(orden, nombre, "volumen de solo lectura", ErrorFs::SoloLectura))
            }
            Some(_) => Ok(()),
        }
    }

    /// Lo que es lo abierto por `asa`: por el nombre y, si no dice nada, por sus primeros bytes.
    fn clase(&mut self, asa: Asa) -> Clase {
        let Some(i) = self.asas.get(&asa) else { return Clase::Normal };
        if let Some(c) = i.clase {
            return c;
        }
        let c = if i.cajon { Clase::Normal } else { self.mirar_dentro(asa, &i.nombre.clone()) };
        if let Some(i) = self.asas.get_mut(&asa) {
            i.clase = Some(c);
        }
        c
    }

    fn mirar_dentro(&mut self, asa: Asa, nombre: &str) -> Clase {
        let c = clase_por_nombre(nombre);
        if c != Clase::Normal {
            return c;
        }
        let mut b = [0u8; 4];
        match self.dentro.leer(asa, 0, &mut b) {
            Ok(4) if b == MAGIA_HUNK => Clase::Ejecutable,
            _ => Clase::Normal,
        }
    }

    /// Lo que es `nombre` dentro del cajón `asa`, sin dejar nada abierto.
    fn clase_en(&mut self, asa: Asa, nombre: &str) -> Clase {
        let c = clase_por_nombre(nombre);
        if c != Clase::Normal {
            return c;
        }
        match self.dentro.buscar(asa, nombre) {
            Ok((a, d)) => {
                let c = if d.tipo == Tipo::Cajon { Clase::Normal } else { self.mirar_dentro(a, nombre) };
                let _ = self.dentro.cerrar(a);
                c
            }
            Err(_) => Clase::Normal,
        }
    }

    /// Lo de `asa`, si ya existía y es de lo que no se cambia: `Protegido`.
    fn cambiable(&mut self, asa: Asa, orden: &'static str) -> Result<Clase, ErrorFs> {
        let nueva = self.asas.get(&asa).is_some_and(|i| i.nueva);
        if nueva {
            return Ok(Clase::Normal);
        }
        let c = self.clase(asa);
        if c.intocable() {
            let n = self.nombre_de(asa);
            if self.dejar(c, &n) {
                return Ok(Clase::Normal);
            }
            return Err(self.negar(orden, &n, c.nombre(), ErrorFs::Protegido));
        }
        Ok(c)
    }

    fn apuntar(&mut self, asa: Asa, raiz: Asa, nombre: &str, nueva: bool, d: &Datos) {
        self.asas.insert(
            asa,
            InfoAsa { raiz, nombre: String::from(nombre), nueva, cajon: d.tipo == Tipo::Cajon, clase: None },
        );
    }
}

impl<F: SistemaArchivos> SistemaArchivos for Guardia<F> {
    fn buscar(&mut self, asa: Asa, camino: &str) -> Result<(Asa, Datos), ErrorFs> {
        let raiz = self.raiz_de(asa).ok_or(ErrorFs::AsaMala)?;
        let r = self.dentro.buscar(asa, camino)?;
        let nombre = if camino.is_empty() { self.nombre_de(asa) } else { String::from(ultimo(camino)) };
        self.apuntar(r.0, raiz, &nombre, false, &r.1);
        Ok(r)
    }

    fn mirar(&mut self, asa: Asa, camino: &str) -> Result<Datos, ErrorFs> {
        self.dentro.mirar(asa, camino)
    }

    fn leer(&mut self, asa: Asa, desde: u64, buf: &mut [u8]) -> Result<usize, ErrorFs> {
        self.dentro.leer(asa, desde, buf)
    }

    fn escribir(&mut self, asa: Asa, desde: u64, datos: &[u8]) -> Result<usize, ErrorFs> {
        let n = self.nombre_de(asa);
        self.escribible(asa, "escribir", &n)?;
        if let Clase::Imagen(tope) = self.cambiable(asa, "escribir")?
            && desde < tope
            && !datos.is_empty()
            && !self.dejar(Clase::Imagen(tope), &n)
        {
            return Err(self.negar("escribir", &n, Clase::Imagen(tope).nombre(), ErrorFs::Protegido));
        }
        self.dentro.escribir(asa, desde, datos)
    }

    fn listar(&mut self, asa: Asa, desde: u64) -> Result<Pagina, ErrorFs> {
        self.dentro.listar(asa, desde)
    }

    fn datos(&mut self, asa: Asa) -> Result<Datos, ErrorFs> {
        self.dentro.datos(asa)
    }

    fn cerrar(&mut self, asa: Asa) -> Result<(), ErrorFs> {
        self.asas.remove(&asa);
        self.dentro.cerrar(asa)
    }

    fn crear(&mut self, asa: Asa, nombre: &str, cajon: bool) -> Result<(Asa, Datos), ErrorFs> {
        self.escribible(asa, "crear", nombre)?;
        if !cajon && clase_por_nombre(nombre) == Clase::Arranque && !self.dejar(Clase::Arranque, nombre) {
            return Err(self.negar("crear", nombre, Clase::Arranque.nombre(), ErrorFs::Protegido));
        }
        let raiz = self.raiz_de(asa).ok_or(ErrorFs::AsaMala)?;
        let r = self.dentro.crear(asa, nombre, cajon)?;
        self.apuntar(r.0, raiz, nombre, true, &r.1);
        Ok(r)
    }

    fn borrar(&mut self, asa: Asa, nombre: &str) -> Result<(), ErrorFs> {
        self.escribible(asa, "borrar", nombre)?;
        let c = self.clase_en(asa, nombre);
        if c != Clase::Normal && !self.dejar(c, nombre) {
            return Err(self.negar("borrar", nombre, c.nombre(), ErrorFs::Protegido));
        }
        self.dentro.borrar(asa, nombre)
    }

    fn renombrar(&mut self, asa: Asa, viejo: &str, asa_nueva: Asa, nuevo: &str) -> Result<(), ErrorFs> {
        self.escribible(asa, "renombrar", viejo)?;
        self.escribible(asa_nueva, "renombrar", nuevo)?;
        let c = self.clase_en(asa, viejo);
        if c != Clase::Normal && !self.dejar(c, viejo) {
            return Err(self.negar("renombrar", viejo, c.nombre(), ErrorFs::Protegido));
        }
        if clase_por_nombre(nuevo) == Clase::Arranque && !self.dejar(Clase::Arranque, nuevo) {
            return Err(self.negar("renombrar", nuevo, Clase::Arranque.nombre(), ErrorFs::Protegido));
        }
        self.dentro.renombrar(asa, viejo, asa_nueva, nuevo)
    }

    fn cambiar_tamano(&mut self, asa: Asa, tamano: u64) -> Result<u64, ErrorFs> {
        let n = self.nombre_de(asa);
        self.escribible(asa, "recortar", &n)?;
        if let Clase::Imagen(tope) = self.cambiable(asa, "recortar")?
            && tamano < tope
            && !self.dejar(Clase::Imagen(tope), &n)
        {
            return Err(self.negar("recortar", &n, Clase::Imagen(tope).nombre(), ErrorFs::Protegido));
        }
        self.dentro.cambiar_tamano(asa, tamano)
    }

    fn vaciar(&mut self) -> Result<(), ErrorFs> {
        self.dentro.vaciar()
    }

    fn info(&mut self, asa: Asa) -> Result<InfoVolumen, ErrorFs> {
        self.dentro.info(asa)
    }

    fn comentario(&mut self, asa: Asa) -> Result<String, ErrorFs> {
        self.dentro.comentario(asa)
    }

    fn comentar(&mut self, asa: Asa, comentario: &str) -> Result<(), ErrorFs> {
        let n = self.nombre_de(asa);
        self.escribible(asa, "comentar", &n)?;
        self.dentro.comentar(asa, comentario)
    }

    fn proteger(&mut self, asa: Asa, amiga: u16, modo: u16) -> Result<(), ErrorFs> {
        let n = self.nombre_de(asa);
        self.escribible(asa, "proteger", &n)?;
        self.cambiable(asa, "proteger")?;
        self.dentro.proteger(asa, amiga, modo)
    }

    fn fechar(&mut self, asa: Asa, fecha: u64) -> Result<(), ErrorFs> {
        let n = self.nombre_de(asa);
        self.escribible(asa, "fechar", &n)?;
        self.dentro.fechar(asa, fecha)
    }

    fn marcar_solo_lectura(&mut self, raiz: Asa) {
        if raiz < RAICES {
            self.solo_lectura.insert(raiz);
        }
        self.dentro.marcar_solo_lectura(raiz);
    }
}
