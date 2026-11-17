//! Volúmenes y asignaciones con nombre para los programas nativos (S4b, paso 2 de `docs/REVISION_LOOP.md`; diseño en
//! `docs/bitacoras/asignaciones.md`).
//!
//! Cada proceso tiene su tabla de nombres. Se la da `sesiones` (orden `ASIGNACIONES`) y la resuelve el propio programa:
//! los nombres son cosa del cliente (S6b, capa 2). La tabla no abre nada: lo que se alcanza lo decide el servicio de
//! archivos con las raíces del proceso. Un nombre solo dice "`SYS:` es la raíz 1".
//!
//! La tabla es texto, una línea por nombre:
//!
//! ```text
//! *SYS=@1            un volumen: la raíz 1 del servicio de archivos
//! *Programas=@1/Programas   un volumen con subcamino
//! PROGDIR=@0         la única asignación con raíz directa (la pone `sesiones`)
//! C=Home:C,SYS:C     una asignación con dos capas, que se prueban en orden
//! ```
//!
//! Las pruebas, en el PC: `rustc --edition 2024 --test usuario/src/asignaciones.rs`.

#[cfg(not(test))]
use crate::protocolo::archivos as a;
#[cfg(test)]
#[path = "protocolo/archivos.rs"]
#[allow(dead_code)]
mod a;

/// Lo más que mide una tabla entera.
pub const MAX_TABLA: usize = 1024;
/// Lo más que mide un nombre, sin los dos puntos.
pub const MAX_NOMBRE: usize = 30;
/// Las capas de una asignación.
pub const MAX_CAPAS: usize = 6;
/// Niveles de una asignación que lleva a otra: más es un bucle (el `ERROR_TOO_MANY_LEVELS` de dos).
pub const MAX_NIVELES: usize = 8;
/// Sitios de una ruta (la suma de las capas de todos los niveles).
pub const MAX_SITIOS: usize = 8;
/// Lo más que mide el camino de un sitio: lo que cabe en el nombre de una petición del servicio de archivos.
pub const MAX_CAMINO: usize = 224;
/// Las raíces del servicio de archivos: 0 a 15.
pub const RAICES: u8 = 16;

/// El nombre del cajón del programa (`PROGDIR:`), que no se puede asignar.
pub const PROGDIR: &[u8] = b"PROGDIR";
/// Los volúmenes que da `sesiones`: ninguna asignación se llama así, los tenga el proceso o no (si no, un programa
/// podría hacer que el `SYS:` de lo que lance fuera un cajón suyo).
pub const VOLUMENES: &[&[u8]] = &[b"SYS", b"Home", b"RAM", b"Work"];

/// Errores de los nombres, además de los del servicio de archivos (que son de -1 a -14).
pub const BUCLE: i32 = -30;
pub const NO_CABE: i32 = -31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Un nombre o una ruta que no se puede escribir así.
    MalFormado,
    /// No hay ese nombre (o ninguna de sus capas lleva a nada).
    NoExiste,
    /// Una asignación que acaba llevando a sí misma.
    Bucle,
    /// No cabe: la tabla, las capas, los sitios o un camino.
    NoCabe,
    /// Un nombre que no se puede cambiar: un volumen o `PROGDIR`.
    Protegido,
}

impl Error {
    /// El estado que se devuelve en lugar de uno del servicio de archivos.
    pub fn estado(self) -> i32 {
        match self {
            Error::MalFormado => a::MAL_FORMADO,
            Error::NoExiste => a::NO_EXISTE,
            Error::Bucle => BUCLE,
            Error::NoCabe => NO_CABE,
            Error::Protegido => a::PROTEGIDO,
        }
    }
}

/// Si dos nombres son el mismo: sin distinguir mayúsculas (las ASCII), como en el Amiga.
pub fn mismo(x: &[u8], y: &[u8]) -> bool {
    x.eq_ignore_ascii_case(y)
}

/// Si `n` vale como nombre de volumen o de asignación.
pub fn nombre_valido(n: &[u8]) -> bool {
    !n.is_empty() && n.len() <= MAX_NOMBRE && n.iter().all(|&c| c > b' ' && c != 0x7f && !b":/=,*@".contains(&c))
}

/// Si `c` vale como parte de un camino: no vacía, ni "." ni "..", sin ':' ni bytes de control.
fn parte_valida(c: &[u8]) -> bool {
    !c.is_empty() && c != b"." && c != b".." && c.iter().all(|&b| b >= b' ' && b != 0x7f && b != b':')
}

/// Si `c` vale como camino detrás de un nombre: vacío, o partes válidas separadas por '/'. Sin '/' al principio (en el
/// Amiga, subir), sin partes vacías. Una sola '/' al final se admite y no cuenta.
fn camino_valido(c: &[u8]) -> bool {
    if c.starts_with(b"/") {
        return false;
    }
    let c = c.strip_suffix(b"/").unwrap_or(c);
    c.is_empty() || c.split(|&b| b == b'/').all(parte_valida)
}

/// Una línea de la tabla.
#[derive(Clone, Copy, Debug)]
pub struct Linea<'a> {
    pub volumen: bool,
    pub nombre: &'a [u8],
    /// Las capas, separadas por ','.
    pub destinos: &'a [u8],
}

impl<'a> Linea<'a> {
    pub fn capas(&self) -> impl Iterator<Item = &'a [u8]> {
        self.destinos.split(|&c| c == b',')
    }
}

/// Lee una línea y la comprueba entera.
fn leer_linea(l: &[u8]) -> Result<Linea<'_>, Error> {
    let (volumen, l) = match l.strip_prefix(b"*") {
        Some(r) => (true, r),
        None => (false, l),
    };
    let i = l.iter().position(|&c| c == b'=').ok_or(Error::MalFormado)?;
    let (nombre, destinos) = (&l[..i], &l[i + 1..]);
    if !nombre_valido(nombre) || destinos.is_empty() {
        return Err(Error::MalFormado);
    }
    let linea = Linea { volumen, nombre, destinos };
    let mut capas = 0;
    let directa = volumen || mismo(nombre, PROGDIR);
    for d in linea.capas() {
        capas += 1;
        match destino(d)? {
            Destino::Raiz(..) if !directa => return Err(Error::MalFormado),
            Destino::Nombre(..) if directa => return Err(Error::MalFormado),
            _ => {}
        }
    }
    if capas > MAX_CAPAS || (directa && capas != 1) {
        return Err(Error::MalFormado);
    }
    Ok(linea)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destino<'a> {
    /// `@n/sub`: la raíz n y un subcamino.
    Raiz(u8, &'a [u8]),
    /// `Nombre:sub`: otro nombre y un camino detrás.
    Nombre(&'a [u8], &'a [u8]),
}

fn destino(d: &[u8]) -> Result<Destino<'_>, Error> {
    if let Some(r) = d.strip_prefix(b"@") {
        let (n, sub) = match r.iter().position(|&c| c == b'/') {
            Some(i) if i + 1 < r.len() => (&r[..i], &r[i + 1..]),
            Some(_) => return Err(Error::MalFormado),
            None => (r, &r[r.len()..]),
        };
        let n = core::str::from_utf8(n).ok().filter(|n| (1..=2).contains(&n.len()) && n.bytes().all(|c| c.is_ascii_digit()));
        let n = n.and_then(|n| n.parse::<u8>().ok()).filter(|&n| n < RAICES).ok_or(Error::MalFormado)?;
        if (!sub.is_empty() && !camino_valido(sub)) || sub.ends_with(b"/") {
            return Err(Error::MalFormado);
        }
        return Ok(Destino::Raiz(n, sub));
    }
    let i = d.iter().position(|&c| c == b':').ok_or(Error::MalFormado)?;
    let (n, sub) = (&d[..i], &d[i + 1..]);
    if !nombre_valido(n) || !camino_valido(sub) || sub.ends_with(b"/") {
        return Err(Error::MalFormado);
    }
    Ok(Destino::Nombre(n, sub))
}

/// Los sitios a los que lleva una ruta: raíz del servicio de archivos y camino desde ella, en orden.
pub struct Sitios {
    n: usize,
    raiz: [u8; MAX_SITIOS],
    largo: [usize; MAX_SITIOS],
    camino: [[u8; MAX_CAMINO]; MAX_SITIOS],
}

impl Sitios {
    pub fn nuevos() -> Self {
        Sitios { n: 0, raiz: [0; MAX_SITIOS], largo: [0; MAX_SITIOS], camino: [[0; MAX_CAMINO]; MAX_SITIOS] }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// El sitio `i`: su raíz y su camino.
    pub fn sitio(&self, i: usize) -> Option<(u8, &[u8])> {
        (i < self.n).then(|| (self.raiz[i], &self.camino[i][..self.largo[i]]))
    }

    /// Añade `raiz`, con `sub` y `resto` juntos por '/'.
    fn poner(&mut self, raiz: u8, sub: &[u8], resto: &[u8]) -> Result<(), Error> {
        if self.n >= MAX_SITIOS {
            return Err(Error::NoCabe);
        }
        let mut c = [0u8; MAX_CAMINO];
        let l = juntar(&mut c, sub, resto)?;
        self.raiz[self.n] = raiz;
        self.camino[self.n] = c;
        self.largo[self.n] = l;
        self.n += 1;
        Ok(())
    }
}

/// Junta `x` e `y` con '/' entre medias (si los dos tienen algo) en `sal`. Lo que mide.
fn juntar(sal: &mut [u8], x: &[u8], y: &[u8]) -> Result<usize, Error> {
    let y = y.strip_suffix(b"/").unwrap_or(y);
    let barra = usize::from(!x.is_empty() && !y.is_empty());
    let l = x.len() + barra + y.len();
    if l > sal.len() {
        return Err(Error::NoCabe);
    }
    sal[..x.len()].copy_from_slice(x);
    if barra == 1 {
        sal[x.len()] = b'/';
    }
    sal[x.len() + barra..l].copy_from_slice(y);
    Ok(l)
}

/// La tabla de nombres de un proceso.
#[derive(Clone)]
pub struct Tabla {
    b: [u8; MAX_TABLA],
    n: usize,
}

impl Default for Tabla {
    fn default() -> Self {
        Self::vacia()
    }
}

/// Cómo cambia `ASIGNAR` una asignación.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cambio {
    /// La pone (o la cambia entera): `Nombre=capa,capa`.
    Poner,
    /// Le añade capas al final (el `Assign ADD` del Amiga): `Nombre=capa`. Si no estaba, la pone.
    Anadir,
    /// Le añade capas delante (el `bind -b` de Plan 9). Si no estaba, la pone.
    Delante,
    /// La quita: `Nombre`.
    Quitar,
}

impl Cambio {
    pub fn desde(n: u32) -> Option<Self> {
        [Cambio::Poner, Cambio::Anadir, Cambio::Delante, Cambio::Quitar].get(n as usize).copied()
    }

    pub fn numero(self) -> u32 {
        self as u32
    }
}

impl Tabla {
    pub const fn vacia() -> Self {
        Tabla { b: [0; MAX_TABLA], n: 0 }
    }

    /// Desde su texto. Cada línea se comprueba; un nombre repetido, o una tabla con un bucle, no vale.
    pub fn desde(texto: &[u8]) -> Result<Self, Error> {
        if texto.len() > MAX_TABLA {
            return Err(Error::NoCabe);
        }
        let mut t = Tabla::vacia();
        for l in texto.split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
            let linea = leer_linea(l)?;
            if t.buscar(linea.nombre).is_some() {
                return Err(Error::MalFormado);
            }
            t.anadir_linea(l)?;
        }
        t.sin_bucles()?;
        Ok(t)
    }

    /// El texto, con un salto de línea detrás de cada una.
    pub fn texto(&self) -> &[u8] {
        &self.b[..self.n]
    }

    pub fn lineas(&self) -> impl Iterator<Item = Linea<'_>> {
        // Lo de dentro ya se comprobó al ponerlo.
        self.texto().split(|&c| c == b'\n').filter(|l| !l.is_empty()).filter_map(|l| leer_linea(l).ok())
    }

    /// Los volúmenes, en el orden de la tabla.
    pub fn volumenes(&self) -> impl Iterator<Item = Linea<'_>> {
        self.lineas().filter(|l| l.volumen)
    }

    pub fn buscar(&self, nombre: &[u8]) -> Option<Linea<'_>> {
        self.lineas().find(|l| mismo(l.nombre, nombre))
    }

    fn anadir_linea(&mut self, l: &[u8]) -> Result<(), Error> {
        if self.n + l.len() + 1 > MAX_TABLA {
            return Err(Error::NoCabe);
        }
        self.b[self.n..self.n + l.len()].copy_from_slice(l);
        self.b[self.n + l.len()] = b'\n';
        self.n += l.len() + 1;
        Ok(())
    }

    /// Quita la línea de `nombre`, si está. Si estaba.
    fn quitar_linea(&mut self, nombre: &[u8]) -> bool {
        let mut i = 0;
        while i < self.n {
            let fin = self.b[i..self.n].iter().position(|&c| c == b'\n').map_or(self.n, |p| i + p + 1);
            let l = &self.b[i..fin];
            let l = l.strip_suffix(b"\n").unwrap_or(l);
            let l = l.strip_prefix(b"*").unwrap_or(l);
            let n = &l[..l.iter().position(|&c| c == b'=').unwrap_or(l.len())];
            if mismo(n, nombre) {
                self.b.copy_within(fin..self.n, i);
                self.n -= fin - i;
                return true;
            }
            i = fin;
        }
        false
    }

    /// Pone un volumen (para `sesiones`, que es quien sabe las raíces): la raíz `raiz` con el subcamino `sub`. Si ya
    /// había ese nombre, lo cambia.
    pub fn poner_volumen(&mut self, nombre: &[u8], raiz: u8, sub: &[u8]) -> Result<(), Error> {
        self.poner_directa(b"*", nombre, raiz, sub)
    }

    /// Pone `PROGDIR:` en la raíz `raiz` (para `sesiones`).
    pub fn poner_progdir(&mut self, raiz: u8) -> Result<(), Error> {
        self.poner_directa(b"", PROGDIR, raiz, b"")
    }

    fn poner_directa(&mut self, marca: &[u8], nombre: &[u8], raiz: u8, sub: &[u8]) -> Result<(), Error> {
        let mut l = [0u8; MAX_NOMBRE + MAX_CAMINO + 8];
        let mut n = 0;
        let mut poner = |x: &[u8]| -> Result<(), Error> {
            l.get_mut(n..n + x.len()).ok_or(Error::NoCabe)?.copy_from_slice(x);
            n += x.len();
            Ok(())
        };
        let mut num = [0u8; 2];
        let num: &[u8] = if raiz >= 10 {
            num = [b'0' + raiz / 10, b'0' + raiz % 10];
            &num
        } else {
            num[0] = b'0' + raiz;
            &num[..1]
        };
        poner(marca)?;
        poner(nombre)?;
        poner(b"=@")?;
        poner(num)?;
        if !sub.is_empty() {
            poner(b"/")?;
            poner(sub)?;
        }
        leer_linea(&l[..n])?;
        let copia = self.clone();
        self.quitar_linea(nombre);
        if let Err(e) = self.anadir_linea(&l[..n]) {
            *self = copia;
            return Err(e);
        }
        Ok(())
    }

    /// Un cambio de una asignación, como lo pide un programa por `ASIGNAR`: solo capas con nombre (nunca `@n`: las
    /// raíces las pone `sesiones`), ni el nombre de un volumen ni `PROGDIR`. Si deja un bucle, no se hace.
    pub fn asignar(&mut self, cambio: Cambio, texto: &[u8]) -> Result<(), Error> {
        let (nombre, capas) = match texto.iter().position(|&c| c == b'=') {
            Some(i) => (&texto[..i], &texto[i + 1..]),
            None => (texto, &texto[texto.len()..]),
        };
        if !nombre_valido(nombre) {
            return Err(Error::MalFormado);
        }
        if mismo(nombre, PROGDIR) || VOLUMENES.iter().any(|v| mismo(v, nombre)) || self.buscar(nombre).is_some_and(|l| l.volumen) {
            return Err(Error::Protegido);
        }
        if cambio == Cambio::Quitar {
            if !capas.is_empty() {
                return Err(Error::MalFormado);
            }
            return if self.quitar_linea(nombre) { Ok(()) } else { Err(Error::NoExiste) };
        }
        if capas.is_empty() {
            return Err(Error::MalFormado);
        }
        for d in capas.split(|&c| c == b',') {
            if !matches!(destino(d)?, Destino::Nombre(..)) {
                return Err(Error::MalFormado);
            }
        }
        // La línea nueva: las capas de antes y las nuevas, en su orden.
        let mut l = [0u8; MAX_TABLA];
        let mut n = 0;
        let mut poner = |x: &[u8]| -> Result<(), Error> {
            l.get_mut(n..n + x.len()).ok_or(Error::NoCabe)?.copy_from_slice(x);
            n += x.len();
            Ok(())
        };
        let antes = self.buscar(nombre).map(|x| x.destinos);
        poner(nombre)?;
        poner(b"=")?;
        match (cambio, antes) {
            (Cambio::Anadir, Some(a)) => {
                poner(a)?;
                poner(b",")?;
                poner(capas)?;
            }
            (Cambio::Delante, Some(a)) => {
                poner(capas)?;
                poner(b",")?;
                poner(a)?;
            }
            _ => poner(capas)?,
        }
        leer_linea(&l[..n]).map_err(|_| Error::NoCabe)?;
        let copia = self.clone();
        self.quitar_linea(nombre);
        let hecho = self.anadir_linea(&l[..n]).and_then(|_| self.sin_bucles());
        if hecho.is_err() {
            *self = copia;
        }
        hecho
    }

    /// Que ningún nombre acabe llevando a sí mismo.
    fn sin_bucles(&self) -> Result<(), Error> {
        let mut s = Sitios::nuevos();
        for l in self.lineas() {
            s.n = 0;
            match self.resolver_en(l.nombre, b"", 0, &mut s) {
                Err(Error::Bucle) => return Err(Error::Bucle),
                _ => {}
            }
        }
        Ok(())
    }

    /// Los sitios de `camino` ("Nombre:resto"), en orden. Un nombre que no está, un camino que intenta salir de él
    /// ('/' al principio, "..", partes vacías) o un ':' de más, no valen.
    pub fn resolver(&self, camino: &[u8], sitios: &mut Sitios) -> Result<(), Error> {
        sitios.n = 0;
        let i = camino.iter().position(|&c| c == b':').ok_or(Error::MalFormado)?;
        let (nombre, resto) = (&camino[..i], &camino[i + 1..]);
        if !nombre_valido(nombre) || !camino_valido(resto) || resto.len() > MAX_CAMINO {
            return Err(Error::MalFormado);
        }
        self.resolver_en(nombre, resto, 0, sitios)?;
        if sitios.n == 0 { Err(Error::NoExiste) } else { Ok(()) }
    }

    fn resolver_en(&self, nombre: &[u8], resto: &[u8], nivel: usize, sitios: &mut Sitios) -> Result<(), Error> {
        if nivel >= MAX_NIVELES {
            return Err(Error::Bucle);
        }
        let linea = self.buscar(nombre).ok_or(Error::NoExiste)?;
        for d in linea.capas() {
            match destino(d)? {
                Destino::Raiz(r, sub) => sitios.poner(r, sub, resto)?,
                Destino::Nombre(n, sub) => {
                    let mut c = [0u8; MAX_CAMINO];
                    let l = juntar(&mut c, sub, resto)?;
                    match self.resolver_en(n, &c[..l], nivel + 1, sitios) {
                        // Una capa que lleva a un nombre que no hay (un volumen que este proceso no tiene) no está.
                        Err(Error::NoExiste) => {}
                        otro => otro?,
                    }
                }
            }
        }
        Ok(())
    }

    /// Quita las capas que pasan por `volumen` de las asignaciones `nombres` (perfil encerrado de S6b: sin la capa del
    /// usuario en `C:`, `LIBS:`, `FONTS:` y `CLASSES:`). Una asignación que se queda sin capas, fuera.
    pub fn sin_capa(&mut self, nombres: &[&[u8]], volumen: &[u8]) {
        for &n in nombres {
            let Some(linea) = self.buscar(n) else { continue };
            if linea.volumen {
                continue;
            }
            let mut l = [0u8; MAX_TABLA];
            let nombre_l = linea.nombre.len();
            l[..nombre_l].copy_from_slice(linea.nombre);
            l[nombre_l] = b'=';
            let mut k = nombre_l + 1;
            let mut quedan = 0;
            for d in linea.capas() {
                let pasa = matches!(destino(d), Ok(Destino::Nombre(v, _)) if mismo(v, volumen));
                if pasa {
                    continue;
                }
                if quedan > 0 {
                    l[k] = b',';
                    k += 1;
                }
                l[k..k + d.len()].copy_from_slice(d);
                k += d.len();
                quedan += 1;
            }
            let mut nombre = [0u8; MAX_NOMBRE];
            nombre[..nombre_l].copy_from_slice(linea.nombre);
            self.quitar_linea(&nombre[..nombre_l]);
            if quedan > 0 {
                // Cabe: es más corta que la que había.
                let _ = self.anadir_linea(&l[..k]);
            }
        }
    }

    /// La tabla de un hijo: los volúmenes y `PROGDIR` de `propia` (los pone `sesiones` según las raíces del hijo) y las
    /// asignaciones de `padre` (las que cambió, también). Lo que no quepa, fuera.
    pub fn heredar(padre: &Tabla, propia: &Tabla) -> Tabla {
        let mut t = Tabla::vacia();
        for l in propia.texto().split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
            let _ = t.anadir_linea(l);
        }
        for l in padre.texto().split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
            let Ok(linea) = leer_linea(l) else { continue };
            if linea.volumen || mismo(linea.nombre, PROGDIR) || t.buscar(linea.nombre).is_some() {
                continue;
            }
            let _ = t.anadir_linea(l);
        }
        t
    }
}

/// Las asignaciones del sistema de S4b para la sesión de una persona, con su capa encima (`Home:`). Sin volúmenes: los
/// pone `sesiones` según las raíces. Con `encerrado` (perfil aplicación o suelto), sin la capa del usuario en `C:`,
/// `LIBS:`, `FONTS:` y `CLASSES:`.
pub const DE_SESION: &[&[u8]] = &[
    b"C=Home:C,SYS:C",
    b"LIBS=Home:Libs,SYS:Libs",
    b"DEVS=SYS:Devs",
    b"FONTS=Home:Fonts,SYS:Fonts",
    b"LOCALE=SYS:Locale",
    b"S=Home:S,SYS:S",
    b"ENVARC=Home:Prefs/Env-Archive,SYS:Prefs/Env-Archive",
    b"CLASSES=Home:Classes,SYS:Classes",
    b"REXX=Home:Rexx,SYS:Rexx",
    b"L=SYS:L",
    b"KEYMAPS=SYS:Devs/Keymaps",
    b"PRINTERS=SYS:Devs/Printers",
    b"HELP=SYS:Locale/Help",
    b"ENV=RAM:Env",
    b"T=RAM:T",
    b"Programas=SYS:Programas",
    // S8 (rama `paq`): lo instalado con Paq para todos; solo lo escribe el servicio `paq`.
    b"Paquetes=SYS:Paquetes",
];

/// Las asignaciones en las que un programa encerrado no tiene la capa del usuario (S4b, S6b regla 6).
pub const SIN_CAPA_DE_USUARIO: &[&[u8]] = &[b"C", b"LIBS", b"FONTS", b"CLASSES"];

/// Pone en `t` las asignaciones de la sesión (`DE_SESION`).
pub fn de_sesion(t: &mut Tabla, encerrado: bool) -> Result<(), Error> {
    for l in DE_SESION {
        let i = l.iter().position(|&c| c == b'=').unwrap_or(0);
        t.asignar(Cambio::Poner, l).or_else(|e| if e == Error::Protegido && t.buscar(&l[..i]).is_some() { Ok(()) } else { Err(e) })?;
    }
    if encerrado {
        t.sin_capa(SIN_CAPA_DE_USUARIO, b"Home");
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Con el servicio de archivos.

/// La respuesta del servicio de archivos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resp {
    pub e: i32,
    pub asa: u32,
    pub a: u64,
    pub b: u64,
    pub c: u64,
}

/// Lo que hace falta del servicio de archivos: pedir y ver la ventana. Cada programa tiene su cliente; esto solo lo
/// envuelve.
pub trait Fs {
    fn pedir(&mut self, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8]) -> Resp;
    fn ventana(&self) -> &[u8];
}

/// Si con este error se pasa a la capa siguiente al buscar: no existe, o la raíz no está (una que este proceso no tiene,
/// o una capa que desapareció).
fn siguiente_al_buscar(e: i32) -> bool {
    e == a::NO_EXISTE || e == a::ASA_MALA
}

/// Si con este error se pasa a la capa siguiente al crear: además, sin el cajón padre, sin permiso o de solo lectura.
fn siguiente_al_crear(e: i32) -> bool {
    siguiente_al_buscar(e) || e == a::NO_ES_CAJON || e == a::PROTEGIDO || e == a::SOLO_LECTURA
}

/// Abre lo que hay en `camino`: en el primer sitio donde exista. La respuesta de `BUSCAR` (con la asa nueva, que hay
/// que cerrar) y en qué capa estaba.
pub fn abrir<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8]) -> Result<(Resp, usize), i32> {
    buscar_con(fs, t, camino, 0)
}

/// Como `abrir`, pero solo sus datos (como `stat`): sin asa.
pub fn mirar<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8]) -> Result<(Resp, usize), i32> {
    buscar_con(fs, t, camino, 1)
}

fn buscar_con<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8], solo_datos: u64) -> Result<(Resp, usize), i32> {
    let mut s = Sitios::nuevos();
    t.resolver(camino, &mut s).map_err(Error::estado)?;
    let mut primero = a::NO_EXISTE;
    for i in 0..s.len() {
        let (raiz, c) = s.sitio(i).unwrap();
        let r = fs.pedir(a::BUSCAR, raiz as u32, solo_datos, 0, c);
        if r.e == a::BIEN {
            return Ok((r, i));
        }
        if !siguiente_al_buscar(r.e) {
            return Err(r.e);
        }
        if i == 0 {
            primero = r.e;
        }
    }
    Err(if primero == a::ASA_MALA { a::NO_EXISTE } else { primero })
}

/// Crea `camino` (un fichero con `a::FICHERO`, un cajón con `a::CAJON`) en la primera capa en la que se pueda: la
/// primera que tenga el cajón padre y deje escribir. Si ya existe en esa, `YA_EXISTE`. Devuelve lo de `CREAR` (con la
/// asa abierta) y en qué capa.
pub fn crear<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8], tipo: u8) -> Result<(Resp, usize), i32> {
    en_primera_capa(fs, t, camino, a::CREAR, tipo as u64, 0)
}

/// Guarda un fichero nuevo con los `cuanto` primeros bytes de la ventana (orden `GUARDAR`), en la primera capa en la
/// que se pueda, como `crear`.
pub fn guardar<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8], cuanto: u64) -> Result<(Resp, usize), i32> {
    en_primera_capa(fs, t, camino, a::GUARDAR, 0, cuanto)
}

fn en_primera_capa<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8], orden: u32, x: u64, y: u64) -> Result<(Resp, usize), i32> {
    let mut s = Sitios::nuevos();
    t.resolver(camino, &mut s).map_err(Error::estado)?;
    // El error de la primera capa que tenía el cajón padre (protegido, solo lectura): es el que importa.
    let mut con_padre = None;
    for i in 0..s.len() {
        let (raiz, c) = s.sitio(i).unwrap();
        // Lo que se crea es la última parte; el padre, lo de antes (o la raíz misma).
        let (padre, ultimo) = match c.iter().rposition(|&b| b == b'/') {
            Some(p) => (&c[..p], &c[p + 1..]),
            None => (&c[..0], c),
        };
        if ultimo.is_empty() {
            // "Home:" no se crea: es el volumen.
            return Err(a::YA_EXISTE);
        }
        let p = fs.pedir(a::BUSCAR, raiz as u32, 0, 0, padre);
        if p.e != a::BIEN {
            if !siguiente_al_crear(p.e) {
                return Err(p.e);
            }
            continue;
        }
        let r = fs.pedir(orden, p.asa, x, y, ultimo);
        fs.pedir(a::CERRAR, p.asa, 0, 0, b"");
        if r.e == a::BIEN {
            return Ok((r, i));
        }
        if !siguiente_al_crear(r.e) {
            return Err(r.e);
        }
        con_padre.get_or_insert(r.e);
    }
    // Ninguna dejó: el error de la primera que tenía el padre, o "no existe" si ninguna lo tenía.
    Err(con_padre.unwrap_or(a::NO_EXISTE))
}

/// Una entrada de un cajón, como la da `LISTAR`.
#[derive(Clone, Copy, Debug)]
pub struct Entrada<'a> {
    pub nombre: &'a [u8],
    pub tipo: u8,
    pub tamano: u64,
    pub fecha: u64,
    pub modo: u16,
    /// Los bytes de la entrada tal cual (cabecera, nombre y comentario).
    pub bytes: &'a [u8],
}

/// Lo que mide la cabecera de una entrada de `LISTAR`.
const CAB_ENTRADA: usize = 24;
/// Lo más que mide una entrada: la cabecera, el nombre y el comentario.
const MAX_ENTRADA: usize = CAB_ENTRADA + 255 + 255;

fn entrada(b: &[u8]) -> Option<(Entrada<'_>, usize)> {
    let cab = b.get(..CAB_ENTRADA)?;
    let (ln, lc) = (cab[19] as usize, cab[22] as usize);
    let total = CAB_ENTRADA + ln + lc;
    let bytes = b.get(..total)?;
    let e = Entrada {
        nombre: &bytes[CAB_ENTRADA..CAB_ENTRADA + ln],
        tipo: cab[18],
        tamano: u64::from_le_bytes(cab[0..8].try_into().unwrap()),
        fecha: u64::from_le_bytes(cab[8..16].try_into().unwrap()),
        modo: u16::from_le_bytes(cab[16..18].try_into().unwrap()),
        bytes,
    };
    Some((e, total.next_multiple_of(8)))
}

/// Lista `camino` uniendo sus capas: cada nombre una vez, el de la primera capa en que sale (la del usuario tapa a la
/// del sistema). `cada` recibe la capa y la entrada, y dice si seguir. Una capa que no existe o no es un cajón no
/// cuenta; si ninguna existe, el error. Devuelve cuántas entradas dio.
pub fn listar<F: Fs>(fs: &mut F, t: &Tabla, camino: &[u8], mut cada: impl FnMut(usize, &Entrada) -> bool) -> Result<usize, i32> {
    let mut s = Sitios::nuevos();
    t.resolver(camino, &mut s).map_err(Error::estado)?;
    let mut dadas = 0;
    let mut alguna = false;
    let mut primero = a::NO_EXISTE;
    for i in 0..s.len() {
        let (raiz, c) = s.sitio(i).unwrap();
        let d = fs.pedir(a::BUSCAR, raiz as u32, 0, 0, c);
        if d.e != a::BIEN {
            if !siguiente_al_buscar(d.e) && d.e != a::NO_ES_CAJON {
                return Err(d.e);
            }
            if i == 0 && d.e != a::ASA_MALA {
                primero = d.e;
            }
            continue;
        }
        alguna = true;
        let mut desde = 0u64;
        let mut seguir = true;
        while seguir {
            let r = fs.pedir(a::LISTAR, d.asa, desde, 0, b"");
            if r.e != a::BIEN {
                // Un fichero, no un cajón: esta capa no cuenta.
                break;
            }
            let (cuantas, siguiente, usados) = (r.a as usize, r.b, r.c as usize);
            let mut o = 0;
            for _ in 0..cuantas {
                // La entrada, copiada: mirar si sale en una capa de antes pide al servicio (no toca la ventana).
                let mut copia = [0u8; MAX_ENTRADA];
                let Some((e, paso)) = fs.ventana().get(o..usados).and_then(entrada) else { break };
                let l = e.bytes.len();
                copia[..l].copy_from_slice(e.bytes);
                o += paso;
                let (e, _) = entrada(&copia[..l]).unwrap();
                let tapada = (0..i).any(|j| {
                    let (rj, cj) = s.sitio(j).unwrap();
                    let mut q = [0u8; MAX_CAMINO];
                    juntar(&mut q, cj, e.nombre).is_ok_and(|n| fs.pedir(a::BUSCAR, rj as u32, 1, 0, &q[..n]).e == a::BIEN)
                });
                if tapada {
                    continue;
                }
                dadas += 1;
                if !cada(i, &e) {
                    seguir = false;
                    break;
                }
            }
            if siguiente == u64::MAX || cuantas == 0 {
                break;
            }
            desde = siguiente;
        }
        fs.pedir(a::CERRAR, d.asa, 0, 0, b"");
        if !seguir {
            break;
        }
    }
    if alguna { Ok(dadas) } else { Err(primero) }
}

// ---------------------------------------------------------------------------------------------------------------
// Con `sesiones`.

/// Pide a `sesiones` (por `canal`) la tabla de este proceso, por páginas. `None` si no contesta o lo que llega no vale.
#[cfg(not(test))]
pub fn pedir_tabla(canal: u32) -> Option<Tabla> {
    use crate::protocolo::sesiones::{ASIGNACIONES, POR_PAGINA_ASIGNACIONES};
    let mut texto = [0u8; MAX_TABLA];
    let mut van = 0usize;
    loop {
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&ASIGNACIONES.to_le_bytes());
        p[4..8].copy_from_slice(&(van as u32).to_le_bytes());
        let mut r = [0u8; 8 + POR_PAGINA_ASIGNACIONES];
        let n = pedir_a_sesiones(canal, &p, &mut r)?;
        if n < 8 {
            return None;
        }
        let total = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        let aqui = (u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize).min(n - 8);
        if total > MAX_TABLA || van + aqui > total {
            return None;
        }
        texto[van..van + aqui].copy_from_slice(&r[8..8 + aqui]);
        van += aqui;
        if van >= total || aqui == 0 {
            return (van == total).then(|| Tabla::desde(&texto[..van]).ok()).flatten();
        }
    }
}

/// Cambia una asignación de este proceso en `sesiones` (y así la de lo que lance después). El estado: 0, o el de
/// `Error::estado`.
#[cfg(not(test))]
pub fn asignar_en_sesiones(canal: u32, cambio: Cambio, texto: &[u8]) -> i32 {
    use crate::protocolo::sesiones::ASIGNAR;
    if texto.len() > 240 {
        return NO_CABE;
    }
    let mut p = [0u8; 252];
    p[0..4].copy_from_slice(&ASIGNAR.to_le_bytes());
    p[4..8].copy_from_slice(&cambio.numero().to_le_bytes());
    p[8..12].copy_from_slice(&(texto.len() as u32).to_le_bytes());
    p[12..12 + texto.len()].copy_from_slice(texto);
    let mut r = [0u8; 4];
    match pedir_a_sesiones(canal, &p[..12 + texto.len()], &mut r) {
        Some(4) => i32::from_le_bytes(r),
        _ => a::MAL_FORMADO,
    }
}

#[cfg(not(test))]
fn pedir_a_sesiones(canal: u32, m: &[u8], r: &mut [u8]) -> Option<usize> {
    let (lectura, suyo) = crate::canal_crear();
    let n = if crate::canal_escribir(canal, m, &[suyo]) == 0 { crate::canal_leer_plazo(lectura, r, &mut [], 5_000_000).0 } else { -1 };
    crate::cerrar(lectura);
    usize::try_from(n).ok()
}

// ---------------------------------------------------------------------------------------------------------------

#[cfg(test)]
#[path = "asignaciones/pruebas.rs"]
mod pruebas;
