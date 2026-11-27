//! Leer y escribir lo del fondo de escritorio (rama `fondos`): imágenes por su camino (`Home:Fondos/x.png`,
//! `SYS:Fondos/x.png`) por la tabla de nombres del proceso, cajones, y el fichero de ajustes `Escritorio/fondo` de `ENV:`
//! (en uso) y `ENVARC:` (guardado), con su vigilancia (avisos de cambio). Como el dock: `ENV:` se lee por la raíz de
//! `RAM:` que pone `sesiones` al entrar alguien (solo para leer); se escribe y se guarda pidiéndoselo a `sesiones`
//! (`ESCRIBIR_ENV`, `GUARDAR_ENV`), que solo deja el suyo.
//!
//! Antes de entrar nadie (sin tabla de nombres, o sin `RAM:`), `Home:` es la raíz propia (`C`) y no hay `ENV:`: vale lo
//! de `Prefs/fondo` (lo de antes de esta rama).
//!
//! Lo usan dos hilos (el principal y el de las miniaturas): cada uno con su ventana de `archivos` en su dirección.

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;

pub const ARCHIVOS: u32 = 2;
pub const SESIONES: u32 = 3;
/// Dónde se proyecta la ventana de `archivos` de cada hilo.
pub const VA_PRINCIPAL: usize = 0x50_0000_0000;
pub const VA_MINIATURAS: usize = 0x50_4000_0000;
const VA_ESCRIBIR: usize = 0x51_0000_0000;

/// El servicio de archivos lleva una sesión por cliente (por la marca de su llave): dos hilos del mismo programa con
/// una sesión cada uno a la vez se pisan la ventana (las miniaturas salían "no se abre (-1)"). Así que una a la vez:
/// la sesión toma este cerrojo al abrirse y lo suelta al cerrarse. Ninguna función de aquí abre dos a la vez.
pub static CERROJO: dashos::hilos::Cerrojo = dashos::hilos::Cerrojo::nuevo();

/// Una sesión con el servicio de archivos, con su ventana proyectada en `va` (se quita al soltarla).
pub struct Sesion {
    pub lectura: u32,
    va: usize,
}

impl Sesion {
    pub fn abrir(va: usize) -> Option<Sesion> {
        CERROJO.tomar();
        let s = Self::abrir_con_cerrojo(va);
        if s.is_none() {
            CERROJO.soltar();
        }
        s
    }

    fn abrir_con_cerrojo(va: usize) -> Option<Sesion> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut cab = [0u8; a::CABECERA];
        cab[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = if dashos::canal_escribir(ARCHIVOS, &cab, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 2_000_000) } else { (-1, 0, 0) };
        if n != 32 || k != 1 || dashos::memoria_proyectar(ll[0], va, true) != 0 {
            if k == 1 {
                dashos::cerrar(ll[0]);
            }
            dashos::cerrar(lectura);
            return None;
        }
        dashos::cerrar(ll[0]);
        Some(Sesion { lectura, va })
    }

    /// Una orden: (estado, asa, a, b, c) de la respuesta.
    pub fn pedir(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> (i32, u32, u64, u64, u64) {
        let mut p = [0u8; 32 + 256];
        let n = nombre.len().min(256);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(n as u32).to_le_bytes());
        p[32..32 + n].copy_from_slice(&nombre[..n]);
        let mut r = [0u8; 32];
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + n], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 5_000_000).0 != 32 {
            return (-1, 0, 0, 0, 0);
        }
        let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u(8), u(16), u(24))
    }

    pub fn ventana(&self) -> &[u8] {
        // SAFETY: proyectada en `abrir` (solo para leer, `a::VENTANA` bytes) y quitada al soltar la sesión.
        unsafe { core::slice::from_raw_parts(self.va as *const u8, a::VENTANA) }
    }

    /// Lo de la asa `asa` entero, si mide `max` como mucho (y la cierra). `Err` con el porqué.
    fn leer_asa(&self, asa: u32, tam: u64, max: u64) -> Result<Vec<u8>, String> {
        let cerrar = || self.pedir(a::CERRAR, asa, 0, 0, b"");
        if tam > max {
            cerrar();
            return Err(alloc::format!("demasiado grande ({} MB)", tam >> 20));
        }
        let mut datos = Vec::new();
        if datos.try_reserve_exact(tam as usize).is_err() {
            cerrar();
            return Err(String::from("no cabe en la memoria"));
        }
        while (datos.len() as u64) < tam {
            let cuanto = (tam - datos.len() as u64).min(a::VENTANA as u64);
            let (e, _, leidos, _, _) = self.pedir(a::LEER, asa, datos.len() as u64, cuanto, b"");
            if e != a::BIEN || leidos == 0 {
                cerrar();
                return Err(alloc::format!("no se lee ({})", e));
            }
            datos.extend_from_slice(&self.ventana()[..leidos.min(cuanto) as usize]);
        }
        cerrar();
        Ok(datos)
    }
}

impl Drop for Sesion {
    fn drop(&mut self) {
        dashos::memoria_quitar(self.va);
        dashos::cerrar(self.lectura);
        CERROJO.soltar();
    }
}

pub struct PorSesion<'a>(pub &'a Sesion);

impl dashos::asignaciones::Fs for PorSesion<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> dashos::asignaciones::Resp {
        let (e, asa, a, b, c) = self.0.pedir(orden, asa, x, y, nombre);
        dashos::asignaciones::Resp { e, asa, a, b, c }
    }

    fn ventana(&self) -> &[u8] {
        self.0.ventana()
    }
}

/// Un camino abierto: por la tabla de nombres si la hay; si no, `Home:` es la raíz propia. La asa, el tamaño y el tipo.
fn abrir(ses: &Sesion, camino: &str) -> Result<(u32, u64, u8), (bool, String)> {
    let tabla = dashos::asignaciones::pedir_tabla(SESIONES);
    let por_tabla = tabla.as_ref().map(|t| dashos::asignaciones::abrir(&mut PorSesion(ses), t, camino.as_bytes()));
    let r = match por_tabla {
        Some(Ok((r, _))) => Ok((r.asa, r.a, (r.c >> 16) as u8)),
        // Sin tabla, o sin `Home:` en ella (antes de entrar nadie): la raíz propia.
        otro => match camino.strip_prefix("Home:") {
            Some(resto) => {
                let (e, asa, tam, _, c) = ses.pedir(a::BUSCAR, 0, 0, 0, resto.as_bytes());
                if e == a::BIEN { Ok((asa, tam, (c >> 16) as u8)) } else { Err(e) }
            }
            None => Err(match otro {
                Some(Err(e)) => e,
                _ => return Err((false, String::from("sin tabla de nombres"))),
            }),
        },
    };
    r.map_err(|e| (e == a::NO_EXISTE, if e == a::NO_EXISTE { String::from("no está") } else { alloc::format!("no se abre ({})", e) }))
}

/// El fichero `camino` entero (como mucho `max` bytes). `Err` con el porqué.
pub fn leer(va: usize, camino: &str, max: u64) -> Result<Vec<u8>, String> {
    let ses = Sesion::abrir(va).ok_or_else(|| String::from("sin servicio de archivos"))?;
    let (asa, tam, tipo) = abrir(&ses, camino).map_err(|e| e.1)?;
    if tipo != a::FICHERO {
        ses.pedir(a::CERRAR, asa, 0, 0, b"");
        return Err(String::from("no es un fichero"));
    }
    ses.leer_asa(asa, tam, max)
}

/// Lo que hay en el cajón `camino`: (nombre, tamaño) de los ficheros, en orden, como mucho `max`. Sin los nombres
/// raros (".", "..", con '/', ':' o letras de control). `Err` con el porqué.
pub fn listar(va: usize, camino: &str, max: usize) -> Result<Vec<(String, u64)>, String> {
    let ses = Sesion::abrir(va).ok_or_else(|| String::from("sin servicio de archivos"))?;
    let (asa, _, tipo) = abrir(&ses, camino).map_err(|e| e.1)?;
    let mut v = Vec::new();
    if tipo == a::CAJON {
        let mut desde = 0u64;
        'fuera: loop {
            let (e, _, cuantos, siguiente, bytes) = ses.pedir(a::LISTAR, asa, desde, 0, b"");
            if e != a::BIEN {
                break;
            }
            let w = &ses.ventana()[..(bytes as usize).min(a::VENTANA)];
            let mut off = 0;
            for _ in 0..cuantos {
                if off + 24 > w.len() {
                    break;
                }
                let (t, largo, largo_com) = (w[off + 18], w[off + 19] as usize, w[off + 22] as usize);
                let total = 24 + largo + largo_com;
                if off + total > w.len() {
                    break;
                }
                if t == a::FICHERO as u8
                    && let Ok(n) = core::str::from_utf8(&w[off + 24..off + 24 + largo])
                    && !n.is_empty()
                    && n != "."
                    && n != ".."
                    && !n.contains(['/', ':'])
                    && !n.chars().any(char::is_control)
                {
                    v.push((String::from(n), u64::from_le_bytes(w[off..off + 8].try_into().unwrap())));
                    if v.len() >= max {
                        break 'fuera;
                    }
                }
                off += total.next_multiple_of(8);
            }
            if siguiente == u64::MAX || cuantos == 0 {
                break;
            }
            desde = siguiente;
        }
    }
    ses.pedir(a::CERRAR, asa, 0, 0, b"");
    if tipo != a::CAJON {
        return Err(String::from("no es un cajón"));
    }
    v.sort();
    Ok(v)
}

/// `Escritorio/fondo` de `ENV:` o, si no está, de `ENVARC:` (sin tabla: el de fábrica). `Ok(None)` si en ninguno.
pub fn leer_ajuste(fichero: &str) -> Result<Option<Vec<u8>>, String> {
    let max = kit::idioma::texto::MAX_FICHERO as u64;
    let ses = Sesion::abrir(VA_PRINCIPAL).ok_or_else(|| String::from("sin servicio de archivos"))?;
    let Some(tabla) = dashos::asignaciones::pedir_tabla(SESIONES) else { return Ok(None) };
    for asignacion in ["ENV", "ENVARC"] {
        let camino = alloc::format!("{}:{}", asignacion, fichero);
        match dashos::asignaciones::abrir(&mut PorSesion(&ses), &tabla, camino.as_bytes()) {
            Ok((r, _)) => return ses.leer_asa(r.asa, r.a, max).map(Some).map_err(|e| alloc::format!("{}: {}", camino, e)),
            Err(e) if e == a::NO_EXISTE => {}
            Err(e) => return Err(alloc::format!("{} no se deja leer ({})", camino, e)),
        }
    }
    Ok(None)
}

/// Escribe `bytes` en `ENV:<fichero>` por `sesiones` (`ESCRIBIR_ENV`), en un objeto de memoria. El estado.
pub fn escribir_env(fichero: &str, bytes: &[u8]) -> i32 {
    let tam = bytes.len().max(1).next_multiple_of(4096);
    let m = dashos::memoria_crear(tam);
    if (m as i32) <= 0 {
        return -1;
    }
    if dashos::memoria_proyectar(m, VA_ESCRIBIR, true) != 0 {
        dashos::cerrar(m);
        return -1;
    }
    // SAFETY: recién proyectada aquí con `tam` bytes (al menos `bytes.len()`); se quita en la línea siguiente.
    unsafe { core::slice::from_raw_parts_mut(VA_ESCRIBIR as *mut u8, bytes.len()) }.copy_from_slice(bytes);
    dashos::memoria_quitar(VA_ESCRIBIR);
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
    dashos::cerrar(m);
    if copia <= 0 {
        return -1;
    }
    let (lectura, suyo) = dashos::canal_crear();
    let mut p = [0u8; 12 + 128];
    let n = fichero.len().min(128);
    p[0..4].copy_from_slice(&s::ESCRIBIR_ENV.to_le_bytes());
    p[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    p[12..12 + n].copy_from_slice(&fichero.as_bytes()[..n]);
    let mut r = [0u8; 4];
    let leido = if dashos::canal_escribir(SESIONES, &p[..12 + n], &[suyo, copia as u32]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        dashos::cerrar(suyo);
        dashos::cerrar(copia as u32);
        -1
    };
    dashos::cerrar(lectura);
    if leido == 4 { i32::from_le_bytes(r) } else { -1 }
}

/// Que `sesiones` copie `ENV:<fichero>` al `ENVARC:` de quien está delante. El estado.
pub fn guardar_en_envarc(fichero: &str) -> i32 {
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8 + 128];
    let n = fichero.len().min(128);
    m[0..4].copy_from_slice(&s::GUARDAR_ENV.to_le_bytes());
    m[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    m[8..8 + n].copy_from_slice(&fichero.as_bytes()[..n]);
    let mut r = [0u8; 4];
    let leido = if dashos::canal_escribir(SESIONES, &m[..8 + n], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    if leido == 4 { i32::from_le_bytes(r) } else { -1 }
}

/// La vigilancia de `ENV:Escritorio` (avisos de cambio por la puerta de la conexión), y el temporizador de reintentar
/// (hasta que entre alguien no hay `ENV:`).
pub struct Vigila {
    pub id: u32,
    temporizador: u32,
}

pub const CADA_VIGILAR_US: u64 = 2_000_000;

impl Vigila {
    pub const fn nueva() -> Vigila {
        Vigila { id: 0, temporizador: 0 }
    }

    pub fn armar(&mut self, puerta: u32) {
        if puerta == 0 {
            return;
        }
        if self.id == 0
            && let (Some(tabla), Some(ses)) = (dashos::asignaciones::pedir_tabla(SESIONES), Sesion::abrir(VA_PRINCIPAL))
            && let Ok((r, _)) = dashos::asignaciones::abrir(&mut PorSesion(&ses), &tabla, b"ENV:Escritorio")
        {
            let copia = dashos::llave_duplicar(puerta, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, 0);
            if copia > 0 {
                let (e, id, _, _) = dashos::vigilar::vigilar(ARCHIVOS, ses.lectura, r.asa, copia as u32, 2_000_000);
                if e == a::BIEN {
                    self.id = id;
                    println!("  [fondo] vigila ENV:Escritorio (avisos de cambio)");
                }
            }
            ses.pedir(a::CERRAR, r.asa, 0, 0, b"");
        }
        if self.id == 0 && self.temporizador == 0 {
            let copia = dashos::llave_duplicar(puerta, dashos::ESCRIBIR, 0);
            if copia > 0 {
                let t = dashos::temporizador_crear(copia as u32, CADA_VIGILAR_US, CADA_VIGILAR_US);
                dashos::cerrar(copia as u32);
                self.temporizador = if t > 0 { t as u32 } else { 0 };
            }
        } else if self.id != 0 && self.temporizador != 0 {
            dashos::cerrar(self.temporizador);
            self.temporizador = 0;
        }
    }

    /// Pide los avisos (los pone a cero) y suelta la vigilancia si acabó. Si hubo alguno.
    pub fn avisos(&mut self) -> bool {
        let Some(ses) = Sesion::abrir(VA_PRINCIPAL) else { return false };
        let (e, _, n, _, _) = ses.pedir(a::AVISOS, 0, 0, 0, b"");
        if e != a::BIEN {
            return false;
        }
        for i in 0..n as usize {
            if let Some((id, marcas, _)) = a::leer_aviso(ses.ventana(), i)
                && id == self.id
                && marcas & (a::AVISO_BORRADO | a::AVISO_FUERA) != 0
            {
                self.id = 0;
            }
        }
        n > 0
    }
}
