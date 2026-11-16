//! Lo que usa una orden del Shell nativo (`docs/bitacoras/shell-nativa.md`), y el propio Shell: su arranque (el cajón
//! actual y los argumentos, por la llave 4), escribir y leer por el Shell que la lanzó, y los ficheros por su ruta con
//! nombre (`Home:Trabajo/nota.txt`, o relativa al cajón actual) sobre `dashos::asignaciones`.
//!
//! Llaves: 1 `sesiones`, 2 archivos, 3 el compositor y 4 el canal de arranque, si quien la lanza lo da.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use dashos::asignaciones::{self as asig, Sitios, Tabla};
use dashos::protocolo::archivos as a;
use shell::plantilla::{self, Args};
use shell::protocolo as p;
use shell::{patron, ruta};

pub mod base;

pub const SESIONES: u32 = 1;
pub const ARCHIVOS: u32 = 2;
/// Dónde se ve la ventana del servicio de archivos.
const VA_VENTANA: usize = 0x44_0000_0000;
/// Lo que se espera al servicio de archivos y al Shell (salvo al leer del teclado, que no tiene plazo).
const PLAZO_US: u64 = 20_000_000;
/// Lo más que se lee de un fichero entero a memoria (un guion, una variable).
pub const MAX_ENTERO: usize = 1 << 20;
/// Lo más que da un patrón.
pub const MAX_HALLADOS: usize = 20_000;

/// Los datos de un fichero o de un cajón.
#[derive(Clone, Copy, Debug, Default)]
pub struct Datos {
    pub tamano: u64,
    pub fecha: u64,
    pub modo: u16,
    pub tipo: u8,
    /// Los bits del Amiga: 1 archivado, 2 puro, 4 guion, 16 no borrar.
    pub bits: u16,
}

impl Datos {
    fn de(r: &asig::Resp) -> Datos {
        Datos { tamano: r.a, fecha: r.b, modo: (r.c & 0xffff) as u16, tipo: (r.c >> 16) as u8, bits: (r.c >> 32) as u16 }
    }

    pub fn cajon(&self) -> bool {
        self.tipo == a::CAJON
    }

    /// Los bits de protección como los enseña el Amiga: `-s-prwed` (guion, puro, archivado, leer, escribir, ejecutar,
    /// borrar), con los permisos del dueño.
    pub fn proteccion(&self) -> String {
        let mut s = String::new();
        for (letra, si) in [
            ('s', self.bits & 4 != 0),
            ('p', self.bits & 2 != 0),
            ('a', self.bits & 1 != 0),
            ('r', self.modo & 0o400 != 0),
            ('w', self.modo & 0o200 != 0),
            ('e', self.modo & 0o100 != 0),
            ('d', self.bits & 16 == 0),
        ] {
            s.push(if si { letra } else { '-' });
        }
        s
    }
}

/// Una entrada de un cajón.
#[derive(Clone, Debug)]
pub struct Entrada {
    pub nombre: String,
    pub datos: Datos,
    pub comentario: String,
}

/// Lo que dice un estado del servicio de archivos (o de los nombres), para el usuario.
pub fn texto_error(e: i32) -> &'static str {
    match e {
        a::MAL_FORMADO => "ese nombre no vale",
        a::NO_EXISTE => "no existe",
        a::NO_ES_CAJON => "no es un cajón",
        a::ES_CAJON => "es un cajón",
        a::ASA_MALA => "no se llega hasta ahí",
        a::SIN_SITIO => "sin sitio",
        a::ERROR_DEL_DISCO => "error del disco",
        a::SOLO_LECTURA => "es de solo lectura",
        a::YA_EXISTE => "ya existe",
        a::NO_VACIO => "el cajón no está vacío",
        a::LLENO => "el disco está lleno",
        a::PROTEGIDO => "está protegido",
        a::EN_USO => "está en uso",
        asig::BUCLE => "la asignación lleva a sí misma",
        asig::NO_CABE => "el nombre es demasiado largo",
        _ => "error",
    }
}

/// El cliente del servicio de archivos.
pub struct Archivos {
    lectura: u32,
    tam: usize,
}

impl Archivos {
    pub fn abrir() -> Option<Archivos> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut m = [0u8; a::CABECERA];
        m[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &m, &[suyo]) != 0 {
            dashos::cerrar(lectura);
            dashos::cerrar(suyo);
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, PLAZO_US);
        if n != 32 || k != 1 || i32::from_le_bytes(r[0..4].try_into().unwrap()) != a::BIEN || dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            dashos::cerrar(lectura);
            return None;
        }
        let tam = (u64::from_le_bytes(r[8..16].try_into().unwrap()) as usize).clamp(4096, a::VENTANA_MAX);
        Some(Archivos { lectura, tam })
    }

    pub fn ventana_mut(&mut self) -> &mut [u8] {
        // SAFETY: la ventana del servicio, proyectada en `abrir` con ese tamaño; solo la toca este hilo.
        unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, self.tam) }
    }

    pub fn orden(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> asig::Resp {
        let mut m = [0u8; 256];
        if nombre.len() > 224 {
            return asig::Resp { e: asig::NO_CABE, ..Default::default() };
        }
        m[0..4].copy_from_slice(&orden.to_le_bytes());
        m[4..8].copy_from_slice(&asa.to_le_bytes());
        m[8..16].copy_from_slice(&x.to_le_bytes());
        m[16..24].copy_from_slice(&y.to_le_bytes());
        m[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        m[32..32 + nombre.len()].copy_from_slice(nombre);
        let mut r = [0u8; 32];
        let mut intentos = 0;
        loop {
            let e = dashos::canal_escribir(ARCHIVOS, &m[..32 + nombre.len()], &[]);
            if e == 0 {
                break;
            }
            intentos += 1;
            if e != dashos::E_LIMITE || intentos > 2000 {
                return asig::Resp { e: a::ERROR_DEL_DISCO, ..Default::default() };
            }
            dashos::dormir_us(1_000);
        }
        if dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], PLAZO_US).0 != 32 {
            return asig::Resp { e: a::ERROR_DEL_DISCO, ..Default::default() };
        }
        let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        asig::Resp { e: i32::from_le_bytes(r[0..4].try_into().unwrap()), asa: u32::from_le_bytes(r[4..8].try_into().unwrap()), a: u(8), b: u(16), c: u(24) }
    }
}

impl asig::Fs for Archivos {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> asig::Resp {
        self.orden(orden, asa, x, y, nombre)
    }

    fn ventana(&self) -> &[u8] {
        // SAFETY: como `ventana_mut`.
        unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, self.tam) }
    }
}

/// Los ficheros por su ruta: el servicio, la tabla de nombres del proceso y el cajón actual.
pub struct Sistema {
    pub fs: Archivos,
    pub tabla: Tabla,
    /// El cajón actual, absoluto (`Home:Trabajo`).
    pub actual: String,
}

fn bien(r: asig::Resp) -> Result<asig::Resp, i32> {
    if r.e == a::BIEN { Ok(r) } else { Err(r.e) }
}

impl Sistema {
    pub fn nuevo() -> Option<Sistema> {
        let fs = Archivos::abrir()?;
        let tabla = asig::pedir_tabla(SESIONES).unwrap_or_default();
        // De serie, `Home:` si hay; si no, `SYS:`.
        let actual = if tabla.buscar(b"Home").is_some() { "Home:" } else { "SYS:" };
        Some(Sistema { fs, tabla, actual: String::from(actual) })
    }

    /// Vuelve a pedir la tabla de nombres (tras un `Assign`).
    pub fn refrescar(&mut self) {
        if let Some(t) = asig::pedir_tabla(SESIONES) {
            self.tabla = t;
        }
    }

    /// La ruta absoluta de `camino` desde el cajón actual.
    pub fn ruta(&self, camino: &str) -> Result<String, i32> {
        ruta::absoluta(&self.actual, camino).map_err(|e| match e {
            ruta::Error::Larga => asig::NO_CABE,
            ruta::Error::SobreLaRaiz => a::NO_EXISTE,
            ruta::Error::MalFormada => a::MAL_FORMADO,
        })
    }

    /// Dónde está de verdad `camino`: la raíz del servicio y el camino dentro de ella (la primera capa donde existe), y
    /// sus datos.
    pub fn donde(&mut self, camino: &str) -> Result<(u8, String, Datos), i32> {
        let r = self.ruta(camino)?;
        let mut s = Sitios::nuevos();
        self.tabla.resolver(r.as_bytes(), &mut s).map_err(asig::Error::estado)?;
        let mut primero = a::NO_EXISTE;
        for i in 0..s.len() {
            let (raiz, c) = s.sitio(i).unwrap();
            let x = self.fs.orden(a::BUSCAR, raiz as u32, 1, 0, c);
            if x.e == a::BIEN {
                return Ok((raiz, String::from(core::str::from_utf8(c).unwrap_or("")), Datos::de(&x)));
            }
            if x.e != a::NO_EXISTE && x.e != a::ASA_MALA {
                return Err(x.e);
            }
            if i == 0 && x.e != a::ASA_MALA {
                primero = x.e;
            }
        }
        Err(primero)
    }

    pub fn mirar(&mut self, camino: &str) -> Result<Datos, i32> {
        self.donde(camino).map(|d| d.2)
    }

    /// Abre `camino`: su asa (que hay que cerrar) y sus datos.
    pub fn abrir(&mut self, camino: &str) -> Result<(u32, Datos), i32> {
        let r = self.ruta(camino)?;
        let (x, _) = asig::abrir(&mut self.fs, &self.tabla, r.as_bytes())?;
        Ok((x.asa, Datos::de(&x)))
    }

    pub fn cerrar(&mut self, asa: u32) -> i32 {
        self.fs.orden(a::CERRAR, asa, 0, 0, b"").e
    }

    /// Lee del fichero abierto desde `desde`: cuántos bytes (0 al final).
    pub fn leer(&mut self, asa: u32, desde: u64, buf: &mut [u8]) -> Result<usize, i32> {
        let cuanto = buf.len().min(self.fs.tam);
        let r = bien(self.fs.orden(a::LEER, asa, desde, cuanto as u64, b""))?;
        let n = (r.a as usize).min(cuanto);
        buf[..n].copy_from_slice(&self.fs.ventana_mut()[..n]);
        Ok(n)
    }

    /// Escribe `datos` en el fichero abierto desde `desde`.
    pub fn escribir(&mut self, asa: u32, mut desde: u64, datos: &[u8]) -> Result<(), i32> {
        for trozo in datos.chunks(self.fs.tam) {
            self.fs.ventana_mut()[..trozo.len()].copy_from_slice(trozo);
            let r = bien(self.fs.orden(a::ESCRIBIR, asa, desde, trozo.len() as u64, b""))?;
            if r.a as usize != trozo.len() {
                return Err(a::LLENO);
            }
            desde += trozo.len() as u64;
        }
        Ok(())
    }

    /// El fichero entero, si no pasa de `tope` bytes (más, `SIN_SITIO`).
    pub fn leer_entero(&mut self, camino: &str, tope: usize) -> Result<Vec<u8>, i32> {
        let (asa, d) = self.abrir(camino)?;
        let r = (|| {
            if d.cajon() {
                return Err(a::ES_CAJON);
            }
            if d.tamano as usize > tope {
                return Err(a::SIN_SITIO);
            }
            let mut v = alloc::vec![0u8; d.tamano as usize];
            let mut hecho = 0;
            while hecho < v.len() {
                let n = self.leer(asa, hecho as u64, &mut v[hecho..])?;
                if n == 0 {
                    break;
                }
                hecho += n;
            }
            v.truncate(hecho);
            Ok(v)
        })();
        self.cerrar(asa);
        r
    }

    /// Crea un fichero (`a::FICHERO`) o un cajón (`a::CAJON`) y lo deja abierto: su asa.
    pub fn crear(&mut self, camino: &str, tipo: u8) -> Result<u32, i32> {
        let r = self.ruta(camino)?;
        asig::crear(&mut self.fs, &self.tabla, r.as_bytes(), tipo).map(|(x, _)| x.asa)
    }

    /// Abre un fichero para escribir en él: si existe, vacío (o como esté, con `anadir`); si no, nuevo. Su asa y lo
    /// que mide. Lo que no se puede escribir falla aquí, no en la primera escritura.
    pub fn abrir_para_escribir(&mut self, camino: &str, anadir: bool) -> Result<(u32, u64), i32> {
        match self.abrir(camino) {
            Ok((asa, d)) => {
                if d.cajon() {
                    self.cerrar(asa);
                    return Err(a::ES_CAJON);
                }
                let tam = if anadir { d.tamano } else { 0 };
                let r = self.fs.orden(a::TAMANO, asa, tam, 0, b"");
                if r.e != a::BIEN {
                    self.cerrar(asa);
                    return Err(r.e);
                }
                Ok((asa, tam))
            }
            Err(a::NO_EXISTE) => self.crear(camino, a::FICHERO).map(|asa| (asa, 0)),
            Err(e) => Err(e),
        }
    }

    /// Las entradas del cajón `camino` (la unión de sus capas), por orden alfabético.
    pub fn listar(&mut self, camino: &str) -> Result<Vec<Entrada>, i32> {
        let r = self.ruta(camino)?;
        let mut v = Vec::new();
        asig::listar(&mut self.fs, &self.tabla, r.as_bytes(), |_, e| {
            let ln = e.nombre.len();
            let bits = e.bytes.get(20..22).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
            v.push(Entrada {
                nombre: String::from_utf8_lossy(e.nombre).into_owned(),
                datos: Datos { tamano: e.tamano, fecha: e.fecha, modo: e.modo, tipo: e.tipo, bits },
                comentario: String::from_utf8_lossy(e.bytes.get(24 + ln..).unwrap_or(b"")).into_owned(),
            });
            v.len() < MAX_HALLADOS
        })?;
        v.sort_by(|x, y| x.nombre.to_lowercase().cmp(&y.nombre.to_lowercase()));
        Ok(v)
    }

    /// El cajón donde está `camino` (abierto: su asa) y su nombre dentro de él.
    fn padre(&mut self, camino: &str) -> Result<(u32, String), i32> {
        let (raiz, c, _) = self.donde(camino)?;
        let (dir, nombre) = match c.rfind('/') {
            Some(i) => (&c[..i], &c[i + 1..]),
            None => ("", c.as_str()),
        };
        if nombre.is_empty() {
            // Un volumen o la raíz de una asignación: no tiene padre que tocar.
            return Err(a::PROTEGIDO);
        }
        let x = bien(self.fs.orden(a::BUSCAR, raiz as u32, 0, 0, dir.as_bytes()))?;
        Ok((x.asa, String::from(nombre)))
    }

    /// Borra un fichero, o un cajón vacío.
    pub fn borrar(&mut self, camino: &str) -> Result<(), i32> {
        let (asa, nombre) = self.padre(camino)?;
        let r = self.fs.orden(a::BORRAR, asa, 0, 0, nombre.as_bytes());
        self.cerrar(asa);
        bien(r).map(|_| ())
    }

    /// Cambia de nombre o mueve (dentro del mismo volumen). No sobrescribe.
    pub fn renombrar(&mut self, de: &str, hacia: &str) -> Result<(), i32> {
        let destino = self.ruta(hacia)?;
        let (cajon, nuevo) = ruta::partir(&destino);
        if nuevo.is_empty() {
            return Err(a::MAL_FORMADO);
        }
        let (asa, viejo) = self.padre(de)?;
        let r = match self.abrir(&cajon) {
            Ok((asa2, _)) => {
                let mut nombres = Vec::from(viejo.as_bytes());
                nombres.push(0);
                nombres.extend_from_slice(nuevo.as_bytes());
                let r = self.fs.orden(a::RENOMBRAR, asa, 0, asa2 as u64, &nombres);
                self.cerrar(asa2);
                bien(r).map(|_| ())
            }
            Err(e) => Err(e),
        };
        self.cerrar(asa);
        r
    }

    /// El comentario de un fichero.
    pub fn comentario(&mut self, camino: &str) -> Result<String, i32> {
        let (asa, _) = self.abrir(camino)?;
        let r = self.fs.orden(a::COMENTARIO, asa, 0, 0, b"");
        let texto = if r.e == a::BIEN { String::from_utf8_lossy(&self.fs.ventana_mut()[..(r.a as usize).min(255)]).into_owned() } else { String::new() };
        self.cerrar(asa);
        bien(r).map(|_| texto)
    }

    pub fn comentar(&mut self, camino: &str, comentario: &str) -> Result<(), i32> {
        let (asa, _) = self.abrir(camino)?;
        let r = self.fs.orden(a::COMENTAR, asa, 0, 0, comentario.as_bytes());
        self.cerrar(asa);
        bien(r).map(|_| ())
    }

    /// Cambia los bits del Amiga y los permisos de Unix.
    pub fn proteger(&mut self, camino: &str, bits: u16, modo: u16) -> Result<(), i32> {
        let (asa, _) = self.abrir(camino)?;
        let r = self.fs.orden(a::PROTEGER, asa, bits as u64, modo as u64, b"");
        self.cerrar(asa);
        bien(r).map(|_| ())
    }

    /// El volumen donde está `camino`: (tamaño, libre, tamaño de bloque), en bytes.
    pub fn volumen(&mut self, camino: &str) -> Result<(u64, u64, u64), i32> {
        let (asa, _) = self.abrir(camino)?;
        let r = self.fs.orden(a::INFO, asa, 0, 0, b"");
        self.cerrar(asa);
        bien(r).map(|r| (r.a, r.b, r.c))
    }

    /// Lo que casa con `camino`, que puede llevar patrones del Amiga en cualquiera de sus partes.
    pub fn expandir(&mut self, camino: &str) -> Result<Vec<patron::Hallado>, i32> {
        let r = self.ruta(camino)?;
        let hallados = patron::expandir(self, &r, MAX_HALLADOS).map_err(|e| match e {
            patron::Error::MalFormado => a::MAL_FORMADO,
            patron::Error::Demasiados => a::SIN_SITIO,
            patron::Error::Listar(e) => e,
        })?;
        // Sin patrones, lo que no se encuentra tiene su porqué (no existe, está protegido): se dice ese.
        if hallados.is_empty() && !patron::es_patron(&r) {
            return Err(Sistema::mirar(self, &r).err().unwrap_or(a::NO_EXISTE));
        }
        Ok(hallados)
    }
}

impl patron::Listador for Sistema {
    fn listar(&mut self, cajon: &str) -> Result<Vec<(String, bool)>, i32> {
        Sistema::listar(self, cajon).map(|v| v.into_iter().map(|e| (e.nombre, e.datos.cajon())).collect())
    }

    fn mirar(&mut self, r: &str) -> Option<bool> {
        Sistema::mirar(self, r).ok().map(|d| d.cajon())
    }
}

// ---------------------------------------------------------------------------------------------------------------
// El arranque y el Shell que la lanzó.

/// Lo que llega por el canal de arranque.
pub struct Arranque {
    pub actual: String,
    /// El nombre con que se llamó (`Dir`), que puede no ser el del fichero.
    pub llamada: String,
    pub args: String,
    /// La puerta del Shell, si lo lanzó uno.
    pub puerta: Option<u32>,
    pub marcas: u32,
    pub columnas: usize,
    pub filas: usize,
}

/// Lee el arranque de la llave 4. Sin ella (o con algo que no cuadra), no hay argumentos ni Shell.
pub fn arranque() -> Option<Arranque> {
    let mut b = [0u8; 256];
    let mut ll = [0u32; 8];
    let (n, k, _) = dashos::canal_leer_plazo(p::LLAVE_ARRANQUE, &mut b, &mut ll, 200_000);
    let u32_en = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    if n < p::LARGO_ARRANQUE as i64 || u32_en(&b, 0) != p::ARRANQUE {
        ll[..k.min(8)].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
        return None;
    }
    let (marcas, la, lg, ln) = (u32_en(&b, 4), u32_en(&b, 8) as usize, u32_en(&b, 12) as usize, u32_en(&b, 20) as usize);
    let (columnas, filas) = (u16::from_le_bytes([b[16], b[17]]) as usize, u16::from_le_bytes([b[18], b[19]]) as usize);
    let puerta = (marcas & p::SERVIDO != 0 && k >= 1).then(|| ll[0]);
    ll[..k.min(8)].iter().skip(puerta.is_some() as usize).for_each(|&l| {
        dashos::cerrar(l);
    });
    if la + lg + ln > p::MAX_ARRANQUE {
        return None;
    }
    let mut datos = Vec::with_capacity(la + ln + lg);
    while datos.len() < la + ln + lg {
        let (n, k, _) = dashos::canal_leer_plazo(p::LLAVE_ARRANQUE, &mut b, &mut ll, 200_000);
        ll[..k.min(8)].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
        if n < 5 || u32_en(&b, 0) != p::DATOS {
            return None;
        }
        datos.extend_from_slice(&b[4..n as usize]);
    }
    datos.truncate(la + ln + lg);
    let args = String::from_utf8_lossy(&datos[la + ln..]).into_owned();
    let llamada = String::from_utf8_lossy(&datos[la..la + ln]).into_owned();
    datos.truncate(la);
    Some(Arranque { actual: String::from_utf8_lossy(&datos).into_owned(), llamada, args, puerta, marcas, columnas, filas })
}

/// El Shell al otro lado de la puerta.
pub struct Cli {
    puerta: u32,
    /// Si alguna respuesta dijo "roto" (Ctrl-C).
    pub roto: bool,
}

impl Cli {
    pub fn nuevo(puerta: u32) -> Cli {
        Cli { puerta, roto: false }
    }

    fn mandar(&self, orden: u32, cuerpo: &[u8]) -> bool {
        let mut m = [0u8; 256];
        let l = cuerpo.len().min(252);
        m[0..4].copy_from_slice(&orden.to_le_bytes());
        m[4..4 + l].copy_from_slice(&cuerpo[..l]);
        // La puerta es el buzón del Shell, que comparte con su ventana: si está lleno, se espera.
        for _ in 0..20_000 {
            match dashos::canal_escribir(self.puerta, &m[..4 + l], &[]) {
                0 => return true,
                dashos::E_LIMITE => dashos::dormir_us(1_000),
                _ => return false,
            }
        }
        false
    }

    /// Pide algo y espera la respuesta: el estado y lo que traiga. `None` si el Shell ya no está.
    pub fn pedir(&mut self, orden: u32, cuerpo: &[u8], plazo_us: u64) -> Option<(i32, Vec<u8>)> {
        if !self.mandar(orden, cuerpo) {
            return None;
        }
        let mut r = [0u8; 256];
        let mut ll = [0u32; 8];
        let (n, k, _) = dashos::canal_leer_plazo(p::LLAVE_ARRANQUE, &mut r, &mut ll, plazo_us);
        ll[..k.min(8)].iter().for_each(|&l| {
            dashos::cerrar(l);
        });
        if n < 8 {
            return None;
        }
        if u32::from_le_bytes(r[4..8].try_into().unwrap()) & p::ROTO != 0 {
            self.roto = true;
        }
        Some((i32::from_le_bytes(r[0..4].try_into().unwrap()), Vec::from(&r[8..n as usize])))
    }

    /// Dice al Shell que se acaba, con ese código.
    pub fn salir(&self, rc: i32, resultado2: i32) {
        let mut c = [0u8; 8];
        c[0..4].copy_from_slice(&rc.to_le_bytes());
        c[4..8].copy_from_slice(&resultado2.to_le_bytes());
        self.mandar(p::SALIR, &c);
    }
}

/// Una orden en marcha: sus argumentos, su salida y sus ficheros.
pub struct Orden {
    pub nombre: &'static str,
    /// El nombre con que la llamaron (vacío si no se dijo).
    pub llamada: String,
    pub s: Sistema,
    pub args: String,
    pub cli: Option<Cli>,
    /// Si la entrada es el teclado.
    pub interactiva: bool,
    /// Si lo que escribe va a una consola (y no a un fichero ni a otra orden): entonces puede llevar colores.
    pub a_consola: bool,
    /// El ancho de la consola, en letras.
    pub columnas: usize,
    bufer: String,
    /// Lo leído de la entrada y aún sin repartir en líneas.
    leido: Vec<u8>,
    vaciado_us: u64,
    /// El estado del último error (el segundo resultado, `$Result2`).
    pub resultado2: i32,
}

impl core::fmt::Write for Orden {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.bufer.push_str(s);
        if self.bufer.len() >= p::POR_ESCRITURA || dashos::ahora_us().saturating_sub(self.vaciado_us) > 50_000 {
            self.vaciar();
        }
        Ok(())
    }
}

/// Escribe en la salida de la orden, como `print!`.
#[macro_export]
macro_rules! di {
    ($o:expr, $($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = $o.write_fmt(format_args!($($arg)*));
    }};
}

impl Orden {
    /// Arranca la orden `nombre`. Sin el servicio de archivos no hay nada que hacer: acaba con 20.
    pub fn arrancar(nombre: &'static str) -> Orden {
        let arr = arranque();
        let Some(mut s) = Sistema::nuevo() else {
            dashos::println!("  [{}] sin el servicio de archivos", nombre);
            if let Some(puerta) = arr.as_ref().and_then(|x| x.puerta) {
                Cli::nuevo(puerta).salir(shell::RC_FALLO, 0);
            }
            dashos::salir(shell::RC_FALLO as u64)
        };
        let (mut args, mut cli, mut interactiva, mut columnas, mut llamada, mut a_consola) = (String::new(), None, false, 80, String::new(), false);
        if let Some(x) = arr {
            if !x.actual.is_empty() {
                s.actual = x.actual;
            }
            args = x.args;
            llamada = x.llamada;
            cli = x.puerta.map(Cli::nuevo);
            interactiva = cli.is_some() && x.marcas & p::ENTRADA_NO_INTERACTIVA == 0;
            a_consola = cli.is_some() && x.marcas & p::SALIDA_NO_CONSOLA == 0;
            if x.columnas > 0 {
                columnas = x.columnas;
            }
        }
        Orden { nombre, llamada, s, args, cli, interactiva, a_consola, columnas, bufer: String::new(), leido: Vec::new(), vaciado_us: dashos::ahora_us(), resultado2: 0 }
    }

    /// Si se pulsó Ctrl-C, la orden acaba aquí mismo (el Shell dice `***Break`).
    fn mirar_roto(&mut self) {
        if let Some(c) = &self.cli
            && c.roto
        {
            c.salir(p::RC_ROTO, 0);
            dashos::salir(p::RC_ROTO as u64);
        }
    }

    fn escribir_en(&mut self, donde: u32, texto: &str) {
        let Some(c) = &mut self.cli else {
            // Sin Shell (lanzada por la barra): al registro del sistema.
            dashos::escribir(texto);
            return;
        };
        let mut resto = texto;
        while !resto.is_empty() {
            let mut n = resto.len().min(p::POR_ESCRITURA);
            while !resto.is_char_boundary(n) {
                n -= 1;
            }
            let mut m = [0u8; 4 + p::POR_ESCRITURA];
            m[0..4].copy_from_slice(&donde.to_le_bytes());
            m[4..4 + n].copy_from_slice(&resto.as_bytes()[..n]);
            match c.pedir(p::ESCRIBIR, &m[..4 + n], 0) {
                // El Shell ya no está: no queda a quién escribir.
                None => dashos::salir(shell::RC_FALLO as u64),
                // Quien leía lo que se escribe (la orden siguiente de la tubería) acabó: no hay más que hacer.
                Some((p::SIN_LECTOR, _)) if !c.roto => {
                    c.salir(shell::RC_BIEN, 0);
                    dashos::salir(shell::RC_BIEN as u64)
                }
                Some(_) => {}
            }
            resto = &resto[n..];
        }
        self.mirar_roto();
    }

    /// Manda al Shell lo que haya escrito y aún no haya salido.
    pub fn vaciar(&mut self) {
        self.vaciado_us = dashos::ahora_us();
        if self.bufer.is_empty() {
            return;
        }
        let texto = core::mem::take(&mut self.bufer);
        self.escribir_en(p::SALIDA, &texto);
    }

    /// Un mensaje de error, con el nombre de la orden delante: `Copy: no existe: nota.txt`.
    pub fn error(&mut self, texto: core::fmt::Arguments) {
        self.vaciar();
        let m = alloc::format!("{}: {}\n", self.nombre, texto);
        self.escribir_en(p::ERRORES, &m);
    }

    /// Un error del servicio de archivos con `camino`; se recuerda como segundo resultado.
    pub fn error_de(&mut self, e: i32, camino: &str) {
        self.resultado2 = e;
        self.error(format_args!("{}: {}", camino, texto_error(e)));
    }

    /// Lee una línea de la entrada (sin su salto de línea). `None` al final.
    pub fn leer_linea(&mut self) -> Option<String> {
        self.vaciar();
        loop {
            // Una línea entera de lo ya leído (el Shell manda varias de una vez cuando no vienen del teclado).
            if let Some(i) = self.leido.iter().position(|&c| c == b'\n') {
                let linea: Vec<u8> = self.leido.drain(..=i).take(i).collect();
                return Some(String::from_utf8_lossy(&linea).into_owned());
            }
            if self.leido.len() > MAX_ENTERO {
                let linea = core::mem::take(&mut self.leido);
                return Some(String::from_utf8_lossy(&linea).into_owned());
            }
            let (n, datos) = self.cli.as_mut()?.pedir(p::LEER, &[], 0)?;
            self.mirar_roto();
            if n <= 0 {
                // El final: lo que quedara sin salto de línea es la última.
                let linea = core::mem::take(&mut self.leido);
                return (!linea.is_empty()).then(|| String::from_utf8_lossy(&linea).into_owned());
            }
            self.leido.extend_from_slice(&datos);
        }
    }

    /// Para lo que tarda sin escribir: mira si se pulsó Ctrl-C (y entonces acaba).
    pub fn pulso(&mut self) {
        if let Some(c) = &mut self.cli {
            self.vaciado_us = dashos::ahora_us();
            if !self.bufer.is_empty() {
                self.vaciar();
            } else if c.pedir(p::PULSO, &[], PLAZO_US).is_none() {
                dashos::salir(shell::RC_FALLO as u64);
            }
            self.mirar_roto();
        }
    }

    /// Reparte los argumentos con `plantilla`. Con `?`, enseña la plantilla y lee los argumentos. Si no cuadran, lo
    /// dice y la orden acaba con 20.
    pub fn repartir(&mut self, plantilla_: &str) -> Args {
        let mut linea = self.args.clone();
        for _ in 0..2 {
            match plantilla::leer(plantilla_, &linea) {
                Ok(a) => return a,
                Err(plantilla::Error::Ayuda) => {
                    di!(self, "{}: ", plantilla_);
                    match self.leer_linea() {
                        Some(l) => linea = l,
                        None => break,
                    }
                }
                Err(e) => {
                    self.error(format_args!("{}", e));
                    let rc = self.fin(shell::RC_FALLO);
                    dashos::salir(rc);
                }
            }
        }
        let rc = self.fin(shell::RC_FALLO);
        dashos::salir(rc)
    }

    /// Acaba: lo pendiente sale, y el Shell recibe el código. Devuelve el código para `principal`.
    pub fn fin(&mut self, rc: i32) -> u64 {
        self.vaciar();
        if let Some(c) = &self.cli {
            c.salir(rc, self.resultado2);
        }
        rc as u64
    }
}
