//! El explorador y el servicio de archivos: listar un cajón, abrir un lugar paso a paso y los trabajos largos (copiar,
//! mover, tirar a la papelera, restaurar, borrar y buscar), que avanzan a trozos para que la ventana no se cuelgue y se
//! pueden cancelar.
//!
//! Todo va por asas y un nombre cada vez (S6b): nunca un camino de texto con "/" ni "..". Los nombres que no valen
//! (`logica::nombre_valido`) ni se enseñan ni se tocan. Copiar es leer y escribir por la ventana del servicio, con los
//! permisos de quien copia: el servicio no hace nada que su ficha no deje.
//!
//! El servicio se ve por el rasgo `Fs`: en el programa, el de verdad; en las pruebas, uno falso en memoria.

use crate::archivos as a;
use crate::logica::{nombre_libre, nombre_valido, Entrada, Ficha, Lugar, MAX_HONDO, PAPELERA_ARCHIVOS, PAPELERA_INFO};
use alloc::string::String;
use alloc::vec::Vec;

/// Lo que contesta el servicio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resp {
    pub e: i32,
    pub asa: u32,
    pub a: u64,
    pub b: u64,
    pub c: u64,
}

impl Resp {
    pub fn tipo(&self) -> u8 {
        ((self.c >> 16) & 0xff) as u8
    }
}

pub trait Fs {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp;
    fn ventana(&self) -> &[u8];
    fn ventana_mut(&mut self) -> &mut [u8];
    /// Segundos desde 1970.
    fn ahora(&self) -> u64;
}

/// Suelta una asa (las raíces no).
pub fn cerrar(fs: &mut impl Fs, asa: u32) {
    if asa >= a::RAICES {
        fs.pedir(a::CERRAR, asa, 0, 0, b"");
    }
}

/// Lo que hay en el cajón de la asa, con los nombres raros fuera (los cuenta). Hasta `max` entradas.
pub fn listar(fs: &mut impl Fs, asa: u32, max: usize) -> Result<(Vec<Entrada>, usize), i32> {
    let mut entradas = Vec::new();
    let mut raros = 0;
    let mut desde = 0u64;
    loop {
        let r = fs.pedir(a::LISTAR, asa, desde, 0, b"");
        if r.e != a::BIEN {
            return Err(r.e);
        }
        let (cuantos, siguiente, bytes) = (r.a, r.b, r.c);
        let v = fs.ventana();
        let v = &v[..(bytes as usize).min(v.len())];
        let mut off = 0;
        for _ in 0..cuantos {
            if off + 24 > v.len() {
                break;
            }
            let (tipo, largo, largo_com) = (v[off + 18], v[off + 19] as usize, v[off + 22] as usize);
            let total = 24 + largo + largo_com;
            if off + total > v.len() {
                break;
            }
            if tipo == a::FICHERO || tipo == a::CAJON {
                match core::str::from_utf8(&v[off + 24..off + 24 + largo]) {
                    Ok(n) if nombre_valido(n) => entradas.push(Entrada {
                        nombre: String::from(n),
                        cajon: tipo == a::CAJON,
                        tam: u64::from_le_bytes(v[off..off + 8].try_into().unwrap()),
                        fecha: u64::from_le_bytes(v[off + 8..off + 16].try_into().unwrap()),
                        modo: u16::from_le_bytes([v[off + 16], v[off + 17]]),
                        bits: u16::from_le_bytes([v[off + 20], v[off + 21]]),
                    }),
                    _ => raros += 1,
                }
            } else {
                raros += 1;
            }
            off += total.next_multiple_of(8);
        }
        if siguiente == u64::MAX || cuantos == 0 || entradas.len() >= max {
            break;
        }
        desde = siguiente;
    }
    Ok((entradas, raros))
}

/// Abre un nombre de un cajón (un solo paso; el nombre tiene que valer) y mira que sea un cajón.
pub fn abrir_cajon(fs: &mut impl Fs, dir: u32, nombre: &str) -> Result<u32, i32> {
    if !nombre_valido(nombre) {
        return Err(a::MAL_FORMADO);
    }
    let r = fs.pedir(a::BUSCAR, dir, 0, 0, nombre.as_bytes());
    if r.e != a::BIEN {
        return Err(r.e);
    }
    if r.tipo() != a::CAJON {
        cerrar(fs, r.asa);
        return Err(a::NO_ES_CAJON);
    }
    Ok(r.asa)
}

/// Abre un lugar paso a paso desde su raíz. La asa de un lugar sin nombres es la raíz (no se cierra).
pub fn abrir(fs: &mut impl Fs, l: &Lugar) -> Result<u32, i32> {
    if !l.valido() {
        return Err(a::MAL_FORMADO);
    }
    let mut asa = l.raiz;
    for n in &l.nombres {
        let r = abrir_cajon(fs, asa, n);
        cerrar(fs, asa);
        asa = r?;
    }
    Ok(asa)
}

/// Lee un fichero pequeño entero por su nombre (hasta la ventana).
pub fn leer_pequeno(fs: &mut impl Fs, dir: u32, nombre: &str) -> Result<Vec<u8>, i32> {
    if !nombre_valido(nombre) {
        return Err(a::MAL_FORMADO);
    }
    let max = fs.ventana().len() as u64;
    let r = fs.pedir(a::LEER_NOMBRE, dir, 0, max, nombre.as_bytes());
    if r.e != a::BIEN {
        return Err(r.e);
    }
    let n = (r.a as usize).min(fs.ventana().len());
    Ok(fs.ventana()[..n].to_vec())
}

/// Lee un fichero entero por trozos (hasta `max` bytes; si pasa, error).
pub fn leer_entero(fs: &mut impl Fs, dir: u32, nombre: &str, max: u64) -> Result<Vec<u8>, i32> {
    if !nombre_valido(nombre) {
        return Err(a::MAL_FORMADO);
    }
    let r = fs.pedir(a::BUSCAR, dir, 0, 0, nombre.as_bytes());
    if r.e != a::BIEN {
        return Err(r.e);
    }
    let asa = r.asa;
    if r.tipo() != a::FICHERO || r.a > max {
        cerrar(fs, asa);
        return Err(if r.a > max { a::SIN_SITIO } else { a::ES_CAJON });
    }
    let tam = r.a;
    let mut datos = Vec::with_capacity(tam as usize);
    let trozo = fs.ventana().len() as u64;
    while (datos.len() as u64) < tam {
        let r = fs.pedir(a::LEER, asa, datos.len() as u64, trozo.min(tam - datos.len() as u64), b"");
        if r.e != a::BIEN || r.a == 0 {
            cerrar(fs, asa);
            return Err(if r.e != a::BIEN { r.e } else { a::ERROR_DEL_DISCO });
        }
        let n = (r.a as usize).min(fs.ventana().len());
        datos.extend_from_slice(&fs.ventana()[..n]);
    }
    cerrar(fs, asa);
    Ok(datos)
}

/// Guarda un fichero pequeño nuevo (no sobrescribe).
pub fn guardar(fs: &mut impl Fs, dir: u32, nombre: &str, datos: &[u8]) -> i32 {
    if !nombre_valido(nombre) || datos.len() > fs.ventana().len() {
        return a::MAL_FORMADO;
    }
    fs.ventana_mut()[..datos.len()].copy_from_slice(datos);
    fs.pedir(a::GUARDAR, dir, 0, datos.len() as u64, nombre.as_bytes()).e
}

/// Renombra `viejo` del cajón `dir` a `nuevo` en el cajón `destino` (no sobrescribe).
pub fn renombrar(fs: &mut impl Fs, dir: u32, viejo: &str, destino: u32, nuevo: &str) -> i32 {
    if !nombre_valido(viejo) || !nombre_valido(nuevo) {
        return a::MAL_FORMADO;
    }
    let mut n = Vec::with_capacity(viejo.len() + 1 + nuevo.len());
    n.extend_from_slice(viejo.as_bytes());
    n.push(0);
    n.extend_from_slice(nuevo.as_bytes());
    fs.pedir(a::RENOMBRAR, dir, 0, destino as u64, &n).e
}

/// El texto de un error del servicio, para la persona.
pub fn texto_error(e: i32) -> &'static str {
    match e {
        a::NO_EXISTE => "ya no existe",
        a::NO_ES_CAJON => "no es una carpeta",
        a::ES_CAJON => "es una carpeta",
        a::SIN_SITIO => "no caben más cosas abiertas",
        a::ERROR_DEL_DISCO => "error del disco",
        a::SOLO_LECTURA => "el disco es de solo lectura",
        a::YA_EXISTE => "ya hay uno con ese nombre",
        a::NO_VACIO => "la carpeta no está vacía",
        a::LLENO => "no queda sitio en el disco",
        a::PROTEGIDO => "no tienes permiso",
        a::EN_USO => "está en uso",
        a::MAL_FORMADO => "el nombre no vale",
        a::ASA_MALA => "el lugar ya no vale",
        _ => "no se pudo",
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Los trabajos largos.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tarea {
    Copiar,
    Mover,
    Papelera,
    Restaurar,
    Borrar,
}

/// Algo sobre lo que se trabaja: su nombre en el cajón `desde`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elemento {
    pub desde: Lugar,
    pub nombre: String,
    pub cajon: bool,
}

/// Un paso del plan de copiar o borrar: el camino desde el elemento (el primero, su nombre) y lo que es.
#[derive(Clone, Debug)]
struct Plan {
    ruta: Vec<String>,
    cajon: bool,
    tam: u64,
    modo: u16,
    bits: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fase {
    Preparar,
    Escanear,
    Ejecutar,
    Fin,
}

/// Los cajones abiertos a lo largo de un camino (el primero, el de la base).
#[derive(Default)]
struct Pila {
    base: u32,
    abiertos: Vec<(String, u32)>,
}

impl Pila {
    /// La asa del cajón `ruta` (desde la base), abriendo lo que falte y cerrando lo que sobre.
    fn situar(&mut self, fs: &mut impl Fs, ruta: &[String]) -> Result<u32, i32> {
        let comun = self.abiertos.iter().zip(ruta).take_while(|(x, n)| x.0 == **n).count();
        while self.abiertos.len() > comun {
            let (_, asa) = self.abiertos.pop().unwrap();
            cerrar(fs, asa);
        }
        for n in &ruta[comun..] {
            let padre = self.abiertos.last().map_or(self.base, |x| x.1);
            let asa = abrir_cajon(fs, padre, n)?;
            self.abiertos.push((n.clone(), asa));
        }
        Ok(self.abiertos.last().map_or(self.base, |x| x.1))
    }

    fn soltar(&mut self, fs: &mut impl Fs) {
        while let Some((_, asa)) = self.abiertos.pop() {
            cerrar(fs, asa);
        }
        cerrar(fs, self.base);
        self.base = 0;
    }
}

/// Un trabajo: copiar o mover a `destino`, tirar a la papelera, sacar de ella o borrar de verdad.
pub struct Trabajo {
    pub tarea: Tarea,
    pub elementos: Vec<Elemento>,
    pub destino: Option<Lugar>,
    /// El cajón ".Papelera" (para tirar, restaurar y borrar de ella).
    pub papelera: Option<Lugar>,
    /// Las raíces que valen en las fichas de la papelera.
    pub raices: Vec<u32>,
    pub cancelar: bool,
    /// Elementos acabados (bien o mal) de `elementos.len()`.
    pub hechos: usize,
    pub bytes: u64,
    pub bytes_total: u64,
    pub ficheros_total: usize,
    pub errores: Vec<String>,
    /// Lo que quedó hecho: (dónde, con qué nombre). Para elegirlo en la vista y para deshacer.
    pub resultado: Vec<(Lugar, String)>,
    pub raros: usize,
    fase: Fase,
    k: usize,
    planes: Vec<Vec<Plan>>,
    ip: usize,
    /// El escaneo: (asa del cajón, sus entradas, por cuál va, su ruta).
    escaneo: Vec<(u32, Vec<Entrada>, usize, Vec<String>)>,
    origen: Pila,
    llegada: Pila,
    /// Los nombres del cajón de destino (para no repetir) y el nombre que toma cada elemento.
    nombres_destino: Vec<String>,
    nuevo: String,
    /// La copia de un fichero a medias: (origen, destino, desde, tamaño, nombre en el destino).
    fichero: Option<(u32, u32, u64, u64, String)>,
    /// Papelera: las asas de "archivos" e "info".
    pap: Option<(u32, u32)>,
}

impl Trabajo {
    pub fn nuevo(tarea: Tarea, elementos: Vec<Elemento>, destino: Option<Lugar>, papelera: Option<Lugar>, raices: Vec<u32>) -> Self {
        Trabajo {
            tarea,
            elementos,
            destino,
            papelera,
            raices,
            cancelar: false,
            hechos: 0,
            bytes: 0,
            bytes_total: 0,
            ficheros_total: 0,
            errores: Vec::new(),
            resultado: Vec::new(),
            raros: 0,
            fase: Fase::Preparar,
            k: 0,
            planes: Vec::new(),
            ip: 0,
            escaneo: Vec::new(),
            origen: Pila::default(),
            llegada: Pila::default(),
            nombres_destino: Vec::new(),
            nuevo: String::new(),
            fichero: None,
            pap: None,
        }
    }

    pub fn acabado(&self) -> bool {
        self.fase == Fase::Fin
    }

    /// Lo que va hecho, de 0 a 1000 (por bytes si se copian, si no por elementos).
    pub fn milesimas(&self) -> u32 {
        if self.fase == Fase::Fin {
            return 1000;
        }
        if self.tarea == Tarea::Copiar && self.bytes_total > 0 {
            return (self.bytes.min(self.bytes_total) * 1000 / self.bytes_total) as u32;
        }
        (self.hechos * 1000 / self.elementos.len().max(1)) as u32
    }

    pub fn preparando(&self) -> bool {
        matches!(self.fase, Fase::Preparar | Fase::Escanear)
    }

    fn fallo(&mut self, que: &str, e: i32) {
        if self.errores.len() < 50 {
            self.errores.push(alloc::format!("{}: {}", que, texto_error(e)));
        }
    }

    /// Para todo y suelta lo abierto; la copia de un fichero a medias se borra.
    fn terminar(&mut self, fs: &mut impl Fs) {
        if let Some((src, dst, _, _, nombre)) = self.fichero.take() {
            cerrar(fs, src);
            cerrar(fs, dst);
            let padre = self.llegada.abiertos.last().map_or(self.llegada.base, |x| x.1);
            fs.pedir(a::BORRAR, padre, 0, 0, nombre.as_bytes());
        }
        for (asa, _, _, _) in self.escaneo.drain(..) {
            cerrar(fs, asa);
        }
        self.origen.soltar(fs);
        self.llegada.soltar(fs);
        if let Some((x, y)) = self.pap.take() {
            cerrar(fs, x);
            cerrar(fs, y);
        }
        self.fase = Fase::Fin;
    }

    /// Avanza hasta `presupuesto` peticiones al servicio. Verdad si acabó.
    pub fn paso(&mut self, fs: &mut impl Fs, presupuesto: usize) -> bool {
        let mut hechas = 0;
        while hechas < presupuesto && self.fase != Fase::Fin {
            if self.cancelar {
                self.errores.push(String::from("Cancelado"));
                self.terminar(fs);
                break;
            }
            hechas += 1;
            match self.fase {
                Fase::Preparar => self.preparar(fs),
                Fase::Escanear => self.escanear(fs),
                Fase::Ejecutar => self.ejecutar(fs),
                Fase::Fin => {}
            }
        }
        self.fase == Fase::Fin
    }

    fn preparar(&mut self, fs: &mut impl Fs) {
        self.elementos.retain(|e| nombre_valido(&e.nombre) && e.desde.valido());
        if self.elementos.is_empty() {
            self.terminar(fs);
            return;
        }
        // El destino (copiar, mover) o la papelera (tirar, restaurar), abiertos para todo el trabajo.
        if let Some(d) = self.destino.clone() {
            // Una carpeta dentro de sí misma, ni copiar ni mover (el servicio tampoco deja moverla).
            if let Some(e) = self.elementos.iter().find(|e| e.cajon && d.dentro_de(&e.desde.hijo(&e.nombre))) {
                let n = e.nombre.clone();
                self.errores.push(alloc::format!("\"{}\": no se puede meter una carpeta dentro de sí misma", n));
                self.terminar(fs);
                return;
            }
            match abrir(fs, &d) {
                Ok(asa) => self.llegada.base = asa,
                Err(e) => {
                    self.fallo("El destino", e);
                    self.terminar(fs);
                    return;
                }
            }
            match listar(fs, self.llegada.base, 100_000) {
                Ok((v, _)) => self.nombres_destino = v.into_iter().map(|e| e.nombre).collect(),
                Err(e) => {
                    self.fallo("El destino", e);
                    self.terminar(fs);
                    return;
                }
            }
        }
        if matches!(self.tarea, Tarea::Papelera | Tarea::Restaurar) || self.tarea == Tarea::Borrar && self.desde_papelera() {
            let Some(p) = self.papelera.clone() else {
                self.errores.push(String::from("No hay papelera"));
                self.terminar(fs);
                return;
            };
            let r = abrir(fs, &p).and_then(|asa| {
                let x = abrir_cajon(fs, asa, PAPELERA_ARCHIVOS);
                let y = abrir_cajon(fs, asa, PAPELERA_INFO);
                cerrar(fs, asa);
                match (x, y) {
                    (Ok(x), Ok(y)) => Ok((x, y)),
                    (x, y) => {
                        for z in [x, y].into_iter().flatten() {
                            cerrar(fs, z);
                        }
                        Err(a::NO_EXISTE)
                    }
                }
            });
            match r {
                Ok(p) => self.pap = Some(p),
                Err(e) => {
                    self.fallo("La papelera", e);
                    self.terminar(fs);
                    return;
                }
            }
            if self.tarea == Tarea::Papelera {
                match listar(fs, self.pap.unwrap().0, 100_000) {
                    Ok((v, _)) => self.nombres_destino = v.into_iter().map(|e| e.nombre).collect(),
                    Err(e) => {
                        self.fallo("La papelera", e);
                        self.terminar(fs);
                        return;
                    }
                }
            }
        }
        self.fase = if matches!(self.tarea, Tarea::Copiar | Tarea::Borrar) { Fase::Escanear } else { Fase::Ejecutar };
        self.k = 0;
    }

    /// Si lo que se borra está en la papelera (entonces se borra también su ficha).
    fn desde_papelera(&self) -> bool {
        let Some(p) = &self.papelera else { return false };
        let archivos = p.hijo(PAPELERA_ARCHIVOS);
        self.elementos.first().is_some_and(|e| e.desde == archivos)
    }

    /// Recorre lo que hay que copiar o borrar y hace el plan de cada elemento (en orden: cada cajón antes que lo suyo).
    fn escanear(&mut self, fs: &mut impl Fs) {
        if let Some((asa, entradas, i, ruta)) = self.escaneo.last_mut() {
            let (asa, i0) = (*asa, *i);
            if i0 >= entradas.len() {
                cerrar(fs, asa);
                self.escaneo.pop();
                return;
            }
            *i += 1;
            let e = entradas[i0].clone();
            let mut r = ruta.clone();
            r.push(e.nombre.clone());
            if e.cajon {
                if r.len() > MAX_HONDO {
                    self.errores.push(alloc::format!("\"{}\": demasiado honda, no se sigue", e.nombre));
                    return;
                }
                match abrir_cajon(fs, asa, &e.nombre).and_then(|x| listar(fs, x, 10_000).map(|l| (x, l)).inspect_err(|_| cerrar(fs, x))) {
                    Ok((x, (v, raros))) => {
                        self.raros += raros;
                        self.planes.last_mut().unwrap().push(Plan { ruta: r.clone(), cajon: true, tam: 0, modo: e.modo, bits: e.bits });
                        self.escaneo.push((x, v, 0, r));
                    }
                    Err(err) => self.fallo(&alloc::format!("\"{}\"", e.nombre), err),
                }
            } else {
                self.ficheros_total += 1;
                self.bytes_total += e.tam;
                self.planes.last_mut().unwrap().push(Plan { ruta: r, cajon: false, tam: e.tam, modo: e.modo, bits: e.bits });
            }
            return;
        }
        // El elemento siguiente.
        if self.k >= self.elementos.len() {
            self.fase = Fase::Ejecutar;
            self.k = 0;
            self.ip = 0;
            return;
        }
        if self.planes.len() <= self.k {
            self.planes.push(Vec::new());
            let el = self.elementos[self.k].clone();
            let r = abrir(fs, &el.desde).and_then(|dir| {
                let x = fs.pedir(a::BUSCAR, dir, 0, 0, el.nombre.as_bytes());
                if x.e != a::BIEN {
                    cerrar(fs, dir);
                    return Err(x.e);
                }
                Ok((dir, x))
            });
            match r {
                Ok((dir, x)) => {
                    let (tam, modo, bits) = (x.a, (x.c & 0o7777) as u16, ((x.c >> 32) & 0xffff) as u16);
                    let ruta = alloc::vec![el.nombre.clone()];
                    if x.tipo() == a::CAJON {
                        self.planes[self.k].push(Plan { ruta: ruta.clone(), cajon: true, tam: 0, modo, bits });
                        match listar(fs, x.asa, 10_000) {
                            Ok((v, raros)) => {
                                self.raros += raros;
                                self.escaneo.push((x.asa, v, 0, ruta));
                            }
                            Err(e) => {
                                cerrar(fs, x.asa);
                                self.fallo(&alloc::format!("\"{}\"", el.nombre), e);
                            }
                        }
                    } else {
                        cerrar(fs, x.asa);
                        self.ficheros_total += 1;
                        self.bytes_total += tam;
                        self.planes[self.k].push(Plan { ruta, cajon: false, tam, modo, bits });
                    }
                    cerrar(fs, dir);
                }
                Err(e) => self.fallo(&alloc::format!("\"{}\"", el.nombre), e),
            }
        }
        self.k += 1;
    }

    fn siguiente_elemento(&mut self, fs: &mut impl Fs) {
        self.hechos += 1;
        self.k += 1;
        self.ip = 0;
        self.origen.soltar(fs);
        // La llegada se queda en su base (el destino), con lo de dentro cerrado.
        while let Some((_, asa)) = self.llegada.abiertos.pop() {
            cerrar(fs, asa);
        }
    }

    fn ejecutar(&mut self, fs: &mut impl Fs) {
        if self.k >= self.elementos.len() {
            self.terminar(fs);
            return;
        }
        match self.tarea {
            Tarea::Copiar => self.copiar(fs),
            Tarea::Mover => self.mover(fs),
            Tarea::Papelera => self.tirar(fs),
            Tarea::Restaurar => self.restaurar(fs),
            Tarea::Borrar => self.borrar(fs),
        }
    }

    fn copiar(&mut self, fs: &mut impl Fs) {
        let el = self.elementos[self.k].clone();
        // Un fichero a medias: el trozo siguiente, por la ventana del servicio.
        if let Some((src, dst, desde, tam, nombre)) = self.fichero.clone() {
            if desde >= tam {
                let p = &self.planes[self.k][self.ip - 1];
                fs.pedir(a::PROTEGER, dst, p.bits as u64, p.modo as u64 & 0o777, b"");
                cerrar(fs, src);
                cerrar(fs, dst);
                self.fichero = None;
                return;
            }
            let trozo = (fs.ventana().len() as u64).min(tam - desde);
            let r = fs.pedir(a::LEER, src, desde, trozo, b"");
            if r.e != a::BIEN || r.a == 0 {
                self.fallo(&alloc::format!("\"{}\"", nombre), if r.e != a::BIEN { r.e } else { a::ERROR_DEL_DISCO });
                self.terminar(fs);
                return;
            }
            let w = fs.pedir(a::ESCRIBIR, dst, desde, r.a, b"");
            if w.e != a::BIEN || w.a != r.a {
                // Sin sitio (o lo que sea): se para todo y lo copiado a medias se borra.
                self.fallo(&alloc::format!("\"{}\"", nombre), if w.e != a::BIEN { w.e } else { a::LLENO });
                self.terminar(fs);
                return;
            }
            self.bytes += r.a;
            self.fichero = Some((src, dst, desde + r.a, tam, nombre));
            return;
        }
        let Some(p) = self.planes.get(self.k).and_then(|v| v.get(self.ip)).cloned() else {
            if self.ip > 0 || self.planes.get(self.k).is_some_and(|v| v.is_empty()) {
                self.siguiente_elemento(fs);
            } else {
                self.siguiente_elemento(fs);
            }
            return;
        };
        self.ip += 1;
        // El nombre en el destino: el primero, uno libre; lo de dentro, el mismo.
        if p.ruta.len() == 1 {
            let nd = core::mem::take(&mut self.nombres_destino);
            self.nuevo = nombre_libre(&el.nombre, p.cajon, |n| nd.iter().any(|x| crate::logica::plano(x) == crate::logica::plano(n)), "copia");
            self.nombres_destino = nd;
            if self.nuevo.is_empty() {
                self.fallo(&alloc::format!("\"{}\"", el.nombre), a::YA_EXISTE);
                self.ip = usize::MAX;
                return;
            }
            self.nombres_destino.push(self.nuevo.clone());
            self.origen.soltar(fs);
            match abrir(fs, &el.desde) {
                Ok(b) => self.origen.base = b,
                Err(e) => {
                    self.fallo(&alloc::format!("\"{}\"", el.nombre), e);
                    self.ip = usize::MAX;
                    return;
                }
            }
        }
        let mut destino_ruta = p.ruta.clone();
        destino_ruta[0] = self.nuevo.clone();
        let (padre_o, padre_d) = (&p.ruta[..p.ruta.len() - 1], &destino_ruta[..destino_ruta.len() - 1]);
        let nombre = p.ruta.last().unwrap().clone();
        let nombre_d = destino_ruta.last().unwrap().clone();
        let (o, d) = match (self.origen.situar(fs, padre_o), self.llegada.situar(fs, padre_d)) {
            (Ok(o), Ok(d)) => (o, d),
            (Err(e), _) | (_, Err(e)) => {
                self.fallo(&alloc::format!("\"{}\"", nombre), e);
                return;
            }
        };
        if p.cajon {
            let r = fs.pedir(a::CREAR, d, a::CAJON as u64, 0, nombre_d.as_bytes());
            if r.e != a::BIEN {
                self.fallo(&alloc::format!("\"{}\"", nombre_d), r.e);
                if r.e == a::LLENO {
                    self.terminar(fs);
                } else if p.ruta.len() == 1 {
                    self.ip = usize::MAX;
                }
                return;
            }
            fs.pedir(a::PROTEGER, r.asa, p.bits as u64, p.modo as u64 & 0o777, b"");
            cerrar(fs, r.asa);
        } else {
            let s = fs.pedir(a::BUSCAR, o, 0, 0, nombre.as_bytes());
            if s.e != a::BIEN || s.tipo() != a::FICHERO {
                if s.e == a::BIEN {
                    cerrar(fs, s.asa);
                }
                self.fallo(&alloc::format!("\"{}\"", nombre), if s.e == a::BIEN { a::ES_CAJON } else { s.e });
                return;
            }
            let r = fs.pedir(a::CREAR, d, a::FICHERO as u64, 0, nombre_d.as_bytes());
            if r.e != a::BIEN {
                cerrar(fs, s.asa);
                self.fallo(&alloc::format!("\"{}\"", nombre_d), r.e);
                if r.e == a::LLENO {
                    self.terminar(fs);
                }
                return;
            }
            // Lo que mide ahora (pudo cambiar desde que se contó).
            self.fichero = Some((s.asa, r.asa, 0, s.a, nombre_d.clone()));
        }
        if p.ruta.len() == 1 {
            self.resultado.push((self.destino.clone().unwrap_or(Lugar::raiz(0)), self.nuevo.clone()));
        }
    }

    fn mover(&mut self, fs: &mut impl Fs) {
        let el = self.elementos[self.k].clone();
        let destino = self.destino.clone().unwrap();
        if el.desde == destino {
            // Al mismo sitio: nada.
            self.siguiente_elemento(fs);
            return;
        }
        let e = match abrir(fs, &el.desde) {
            Ok(dir) => {
                let e = if self.nombres_destino.iter().any(|x| crate::logica::plano(x) == crate::logica::plano(&el.nombre)) {
                    a::YA_EXISTE
                } else {
                    renombrar(fs, dir, &el.nombre, self.llegada.base, &el.nombre)
                };
                cerrar(fs, dir);
                e
            }
            Err(e) => e,
        };
        if e == a::BIEN {
            self.nombres_destino.push(el.nombre.clone());
            self.resultado.push((destino, el.nombre.clone()));
        } else {
            self.fallo(&alloc::format!("\"{}\"", el.nombre), e);
        }
        self.siguiente_elemento(fs);
    }

    fn tirar(&mut self, fs: &mut impl Fs) {
        let el = self.elementos[self.k].clone();
        let (archivos, info) = self.pap.unwrap();
        let nd = core::mem::take(&mut self.nombres_destino);
        let libre = nombre_libre(&el.nombre, el.cajon, |n| nd.iter().any(|x| crate::logica::plano(x) == crate::logica::plano(n)), "");
        self.nombres_destino = nd;
        let e = match abrir(fs, &el.desde) {
            Ok(dir) => {
                let mut e = renombrar(fs, dir, &el.nombre, archivos, &libre);
                if e == a::BIEN {
                    // La ficha; si no se puede guardar, se deshace (sin ficha no se sabría devolverlo).
                    fs.pedir(a::BORRAR, info, 0, 0, libre.as_bytes());
                    let f = Ficha { origen: el.desde.clone(), nombre: el.nombre.clone(), fecha: fs.ahora() };
                    let g = guardar(fs, info, &libre, f.texto().as_bytes());
                    if g != a::BIEN {
                        renombrar(fs, archivos, &libre, dir, &el.nombre);
                        e = g;
                    }
                }
                cerrar(fs, dir);
                e
            }
            Err(e) => e,
        };
        if e == a::BIEN {
            self.nombres_destino.push(libre.clone());
            self.resultado.push((self.papelera.clone().unwrap().hijo(PAPELERA_ARCHIVOS), libre));
        } else {
            self.fallo(&alloc::format!("\"{}\"", el.nombre), e);
        }
        self.siguiente_elemento(fs);
    }

    fn restaurar(&mut self, fs: &mut impl Fs) {
        let el = self.elementos[self.k].clone();
        let (archivos, info) = self.pap.unwrap();
        let ficha = leer_pequeno(fs, info, &el.nombre).ok().and_then(|d| Ficha::leer(&d, &self.raices));
        let e = match ficha {
            None => Err(alloc::format!("\"{}\": no se sabe de dónde vino", el.nombre)),
            Some(f) => match abrir(fs, &f.origen) {
                Err(e) => Err(alloc::format!("\"{}\": la carpeta de donde vino {}", f.nombre, texto_error(e))),
                Ok(dir) => {
                    let r = match listar(fs, dir, 100_000) {
                        Ok((v, _)) => {
                            let nuevo = nombre_libre(&f.nombre, el.cajon, |n| v.iter().any(|x| crate::logica::plano(&x.nombre) == crate::logica::plano(n)), "restaurado");
                            match renombrar(fs, archivos, &el.nombre, dir, &nuevo) {
                                a::BIEN => {
                                    fs.pedir(a::BORRAR, info, 0, 0, el.nombre.as_bytes());
                                    self.resultado.push((f.origen.clone(), nuevo));
                                    Ok(())
                                }
                                e => Err(alloc::format!("\"{}\": {}", f.nombre, texto_error(e))),
                            }
                        }
                        Err(e) => Err(alloc::format!("\"{}\": {}", f.nombre, texto_error(e))),
                    };
                    cerrar(fs, dir);
                    r
                }
            },
        };
        if let Err(t) = e {
            self.errores.push(t);
        }
        self.siguiente_elemento(fs);
    }

    /// Borra de verdad: el plan al revés (lo de dentro antes que su cajón).
    fn borrar(&mut self, fs: &mut impl Fs) {
        let el = self.elementos[self.k].clone();
        let plan = &self.planes[self.k];
        if self.ip >= plan.len() {
            if !plan.is_empty() {
                if let Some((_, info)) = self.pap {
                    fs.pedir(a::BORRAR, info, 0, 0, el.nombre.as_bytes());
                }
                self.resultado.push((el.desde.clone(), el.nombre.clone()));
            }
            self.siguiente_elemento(fs);
            return;
        }
        if self.ip == 0 {
            self.origen.soltar(fs);
            match abrir(fs, &el.desde) {
                Ok(b) => self.origen.base = b,
                Err(e) => {
                    self.fallo(&alloc::format!("\"{}\"", el.nombre), e);
                    self.planes[self.k].clear();
                    self.siguiente_elemento(fs);
                    return;
                }
            }
        }
        let p = plan[plan.len() - 1 - self.ip].clone();
        self.ip += 1;
        let padre = &p.ruta[..p.ruta.len() - 1];
        let nombre = p.ruta.last().unwrap();
        match self.origen.situar(fs, padre) {
            Ok(dir) => {
                let e = fs.pedir(a::BORRAR, dir, 0, 0, nombre.as_bytes()).e;
                if e != a::BIEN {
                    self.fallo(&alloc::format!("\"{}\"", nombre), e);
                    if p.ruta.len() == 1 {
                        // No se borró el elemento: su ficha se queda.
                        self.planes[self.k].clear();
                    }
                } else if !p.cajon {
                    self.bytes += p.tam;
                }
            }
            Err(e) => self.fallo(&alloc::format!("\"{}\"", nombre), e),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// La búsqueda en las subcarpetas, también a trozos.

pub struct Busqueda {
    pub busca: String,
    pub desde: Lugar,
    /// Lo encontrado: dónde y qué.
    pub encontrado: Vec<(Lugar, Entrada)>,
    pub acabada: bool,
    pub ocultos: bool,
    pila: Vec<(u32, Lugar, Vec<Entrada>, usize)>,
    empezada: bool,
}

pub const MAX_ENCONTRADOS: usize = 500;

impl Busqueda {
    pub fn nueva(busca: &str, desde: Lugar, ocultos: bool) -> Self {
        Busqueda { busca: String::from(busca), desde, encontrado: Vec::new(), acabada: false, ocultos, pila: Vec::new(), empezada: false }
    }

    pub fn parar(&mut self, fs: &mut impl Fs) {
        for (asa, _, _, _) in self.pila.drain(..) {
            cerrar(fs, asa);
        }
        self.acabada = true;
    }

    /// Avanza hasta `presupuesto` peticiones. Verdad si encontró algo nuevo.
    pub fn paso(&mut self, fs: &mut impl Fs, presupuesto: usize) -> bool {
        let antes = self.encontrado.len();
        if !self.empezada {
            self.empezada = true;
            match abrir(fs, &self.desde).and_then(|asa| match listar(fs, asa, 10_000) {
                Ok((v, _)) => Ok((asa, v)),
                Err(e) => {
                    cerrar(fs, asa);
                    Err(e)
                }
            }) {
                Ok((asa, v)) => self.pila.push((asa, self.desde.clone(), v, 0)),
                Err(_) => self.acabada = true,
            }
        }
        let mut hechas = 0;
        while hechas < presupuesto && !self.acabada {
            let Some((asa, lugar, v, i)) = self.pila.last_mut() else {
                self.acabada = true;
                break;
            };
            if *i >= v.len() {
                let asa = *asa;
                cerrar(fs, asa);
                self.pila.pop();
                continue;
            }
            let e = v[*i].clone();
            *i += 1;
            if !self.ocultos && crate::logica::es_oculto(&e.nombre) {
                continue;
            }
            let (asa, lugar) = (*asa, lugar.clone());
            if crate::logica::casa(&e.nombre, &self.busca) {
                self.encontrado.push((lugar.clone(), e.clone()));
                if self.encontrado.len() >= MAX_ENCONTRADOS {
                    self.parar(fs);
                    break;
                }
            }
            if e.cajon && lugar.nombres.len() < MAX_HONDO {
                hechas += 1;
                if let Ok(x) = abrir_cajon(fs, asa, &e.nombre) {
                    hechas += 1;
                    match listar(fs, x, 10_000) {
                        Ok((v, _)) => self.pila.push((x, lugar.hijo(&e.nombre), v, 0)),
                        Err(_) => cerrar(fs, x),
                    }
                }
            }
        }
        self.encontrado.len() != antes
    }
}
