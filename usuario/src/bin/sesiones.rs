//! `sesiones` (U2 de `docs/diseño/S6b-usuarios-y-permisos.md`), primera parte: una ficha por proceso y, con ella,
//! la lista de procesos y matar. Aún no lanza desde disco ni reparte llaves (eso sigue en `inicio`).
//!
//! Llave 1: el extremo de lectura del canal "sesiones". Por él llegan:
//! - De `inicio`, sin marca: el **alta** de cada programa que lanza (orden 1: marca u64, uid u32, gid u32 y 16 bytes
//!   de nombre), con la llave de su grupo de recursos, para poder matarlo entero.
//! - De los programas, con su marca: **procesos** (orden 1, desde qué ficha en u64) y **matar** (orden 2, la marca
//!   del otro en u64). Cada petición trae una llave: por dónde contestar. Procesos contesta, como cabe en un mensaje
//!   (256 bytes), hasta 7 fichas: cuántas van (u32), cuántas hay (u32) y las fichas de 32 bytes (marca, uid, gid y
//!   nombre). Matar contesta un i32: 0 hecho, -1 no existe, -2 es de otro usuario.
//!
//! - Del kernel, con la marca de la ficha y el bit alto puesto (que solo pone `inicio` al pedir el aviso de fin, K7):
//!   16 bytes, clase y código del fin. Es la **baja**: la ficha se quita.
//!
//! Solo se mata lo del mismo usuario (más adelante, lo de otro con una acción de administrador, U5). El usuario es el
//! de la ficha de quien pide, que sale de la marca: ningún programa puede decir que es otro.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::*;

dashos::programa!(principal);

/// Llave 1: el canal del servicio de archivos; 2: el de `eco`, sin marca, para repartirlo (U4); 3: el suyo (los canales
/// van en el orden en que se nombran).
const ARCHIVOS: u32 = 1;
const CANAL: u32 = 3;
/// Llave 4: una copia sin marca de su propio canal, para escribir en él (U2): de ella salen la llave con que nace lo
/// que lanza (marcada con su número) y su aviso de fin.
const PROPIO: u32 = 4;
/// Llave 5: una copia sin marca del canal de archivos (U2): las altas de lo que lanza, con sus raíces, y su llave
/// marcada con su número.
const ARCHIVOS_ALTAS: u32 = 5;
/// Las raíces con que nace lo que lanza: la 0, el cajón de donde salió (`PROGDIR:`); la 1, el disco para leer
/// (`SYS:`, `Programas:`); la 2, el volumen de su usuario (`Home:`), si tiene cuenta.
const RAIZ_SYS: u8 = 1;
const RAIZ_HOME: u8 = 2;
/// Los servicios que reparte, con la llave sin marca de cada uno.
const REPARTE: &[(&str, u32)] = &[("eco", 2)];
/// Los permisos de cada programa (U4), provisionales hasta que salgan del paquete (S8). Lo que no esté aquí se le
/// pregunta a `guardian`, si atiende preguntas, y lo que diga que sí se guarda (`CAMINO_PERMISOS`).
const PERMISOS: &[(&str, &str)] = &[];
const CAMINO_PERMISOS: &[u8] = b"Estado/Servicios/sesiones/permisos";
/// Permisos concedidos por el usuario, a la vez, y preguntas sin contestar.
const MAX_CONCEDIDOS: usize = 32;
const MAX_PREGUNTAS: usize = 8;
/// Llaves repartidas que se pueden revocar, a la vez.
const MAX_DADAS: usize = 64;
/// Dónde ve su ventana del servicio de archivos, y dónde copia un programa que lanza.
const VA_VENTANA: usize = 0x40_0000_0000;
const VA_PROGRAMA: usize = 0x48_0000_0000;
/// Lo más que mide un programa que se lanza desde el disco (lo que admite `proceso_crear`).
const MAX_PROGRAMA: usize = 4 << 20;
/// Los números de proceso de lo que lanza `sesiones` empiezan aquí: los de `inicio` son el número del programa más 1.
const PRIMER_PID_PROPIO: u64 = 0x1_0000;
/// Lo que se da a cada programa que lanza (su grupo de recursos).
const PRESUPUESTO_LANZADO: u64 = 4 << 20;
/// Quién puede pedir entrar y salir (U3): `guardian`, y `entrada`, que hace de él en las pruebas.
const GUARDIANES: &[&str] = &["guardian", "entrada"];

/// El registro de seguridad (U6): las últimas `REGISTRO_MAX` negativas y fallos en memoria, y en el disco, en su
/// cajón (`CAMINO_REGISTRO`): la cuenta (u64) y un anillo de `DISCO_MAX` entradas. Al arrancar lee lo que hubiera.
const REGISTRO_MAX: usize = 64;
const DISCO_MAX: usize = 1024;
const CAJON: &[u8] = b"Estado/Servicios/sesiones";
const CAMINO_REGISTRO: &[u8] = b"Estado/Servicios/sesiones/registro";
struct Registro {
    entradas: [[u8; ENTRADA_REGISTRO]; REGISTRO_MAX],
    total: usize,
    /// Las últimas que faltan por llevar al disco, y la asa del fichero (0 si no hay).
    pendientes: usize,
    asa: u32,
}

impl Registro {
    fn apuntar(&mut self, tipo: u32, estado: i32, quien: &[u8], que: &[u8]) {
        let e = &mut self.entradas[self.total % REGISTRO_MAX];
        e.fill(0);
        e[0..4].copy_from_slice(&tipo.to_le_bytes());
        e[4..8].copy_from_slice(&estado.to_le_bytes());
        let q = quien.iter().position(|&c| c == 0).map_or(quien, |n| &quien[..n]);
        e[8..8 + q.len().min(16)].copy_from_slice(&q[..q.len().min(16)]);
        e[24..24 + que.len().min(16)].copy_from_slice(&que[..que.len().min(16)]);
        self.total += 1;
        self.pendientes = (self.pendientes + 1).min(REGISTRO_MAX);
    }

    /// Abre (o crea) el fichero del registro y trae a memoria las últimas entradas que tenía.
    fn abrir_disco(&mut self, a: &Archivos) {
        let Some((asa, tam)) = Self::abrir_fichero(a) else { return };
        self.asa = asa;
        self.leer_disco(a, tam);
    }

    /// La asa del fichero del registro, creándolo si no está, y su tamaño.
    fn abrir_fichero(a: &Archivos) -> Option<(u32, u64)> {
        use dashos::protocolo::archivos as x;
        let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_REGISTRO);
        let asa = if e == x::BIEN {
            asa
        } else {
            let (e, cajon, _) = a.orden(x::BUSCAR, 0, 0, 0, CAJON);
            if e != x::BIEN {
                println!("  [sesiones] sin su cajón ({}): el registro, solo en memoria", e);
                return None;
            }
            let (e, asa, _) = a.orden(x::CREAR, cajon, x::FICHERO as u64, 0, b"registro");
            a.orden(x::CERRAR, cajon, 0, 0, b"");
            if e != x::BIEN {
                println!("  [sesiones] no crea el registro ({}): solo en memoria", e);
                return None;
            }
            asa
        };
        Some((asa, if e == x::BIEN { tam } else { 0 }))
    }

    fn leer_disco(&mut self, a: &Archivos, tam: u64) {
        use dashos::protocolo::archivos as x;
        let asa = self.asa;
        if tam < 8 || a.orden(x::LEER, asa, 0, 8, b"").2 != 8 {
            return;
        }
        let total = unsafe { core::ptr::read_unaligned(VA_VENTANA as *const u64) } as usize;
        let desde = total.saturating_sub(REGISTRO_MAX.min(DISCO_MAX));
        for i in desde..total {
            let o = 8 + (i % DISCO_MAX) * ENTRADA_REGISTRO;
            if a.orden(x::LEER, asa, o as u64, ENTRADA_REGISTRO as u64, b"").2 == ENTRADA_REGISTRO as u64 {
                let v = unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, ENTRADA_REGISTRO) };
                self.entradas[i % REGISTRO_MAX].copy_from_slice(v);
            }
        }
        self.total = total;
        println!("  [sesiones] registro de seguridad en el disco: {} entradas de antes", total);
    }

    /// Lleva al disco lo que falte: cada entrada a su sitio del anillo, y la cuenta.
    fn guardar(&mut self, a: &Archivos) {
        use dashos::protocolo::archivos as x;
        if self.asa == 0 {
            self.pendientes = 0;
            return;
        }
        let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, ENTRADA_REGISTRO) };
        ventana[..8].copy_from_slice(&(self.total as u64).to_le_bytes());
        if a.orden(x::ESCRIBIR, self.asa, 0, 8, b"").0 == x::ASA_MALA {
            // Se perdió la asa (volvieron a una instantánea del volumen, que suelta las de todos): se reabre sin leer
            // lo de antes (sería más viejo) y se escribe todo lo que hay en memoria.
            match Self::abrir_fichero(a) {
                Some((asa, _)) => self.asa = asa,
                None => {
                    self.asa = 0;
                    return;
                }
            }
            self.pendientes = self.total.min(REGISTRO_MAX);
            ventana[..8].copy_from_slice(&(self.total as u64).to_le_bytes());
            a.orden(x::ESCRIBIR, self.asa, 0, 8, b"");
        }
        for i in self.total - self.pendientes..self.total {
            ventana.copy_from_slice(&self.entradas[i % REGISTRO_MAX]);
            a.orden(x::ESCRIBIR, self.asa, (8 + (i % DISCO_MAX) * ENTRADA_REGISTRO) as u64, ENTRADA_REGISTRO as u64, b"");
        }
        self.pendientes = 0;
    }

    /// Las entradas que se guardan, de la más vieja a la más nueva.
    fn entrada(&self, i: usize) -> Option<&[u8; ENTRADA_REGISTRO]> {
        let guardadas = self.total.min(REGISTRO_MAX);
        (i < guardadas).then(|| &self.entradas[(self.total - guardadas + i) % REGISTRO_MAX])
    }
}

/// Su sesión con el servicio de archivos: el canal por el que le contesta.
struct Archivos {
    lectura: u32,
}

impl Archivos {
    fn abrir() -> Option<Archivos> {
        use dashos::protocolo::archivos as a;
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k) = dashos::canal_leer(lectura, &mut r, &mut ll);
        if n != 32 || k != 1 || i32::from_le_bytes(r[0..4].try_into().unwrap()) != 0 || dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            return None;
        }
        Some(Archivos { lectura })
    }

    /// Una orden entera: (estado, asa, a) de la respuesta.
    fn orden(&self, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8]) -> (i32, u32, u64) {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(256 - 32);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&a.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 {
            return (-1, 0, 0);
        }
        let mut r = [0u8; 32];
        let (n, _) = dashos::canal_leer(self.lectura, &mut r, &mut []);
        if n != 32 {
            return (-1, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    }

    /// Lee el fichero `camino` entero a un objeto de memoria nuevo: (llave, tamaño), o el estado si no se pudo.
    fn leer_entero(&self, camino: &[u8]) -> Result<(u32, usize), i32> {
        use dashos::protocolo::archivos as a;
        let (e, asa, tam) = self.orden(a::BUSCAR, 0, 0, 0, camino);
        if e != a::BIEN {
            return Err(e);
        }
        let tam = tam as usize;
        let r = (|| {
            if tam == 0 || tam > MAX_PROGRAMA {
                return Err(a::MAL_FORMADO);
            }
            let m = dashos::memoria_crear(tam);
            if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA_PROGRAMA, true) != 0 {
                return Err(a::SIN_SITIO);
            }
            let mut hecho = 0;
            while hecho < tam {
                let (e, _, n) = self.orden(a::LEER, asa, hecho as u64, a::VENTANA as u64, b"");
                if e != a::BIEN || n == 0 {
                    dashos::memoria_quitar(VA_PROGRAMA);
                    dashos::cerrar(m as u32);
                    return Err(if e != a::BIEN { e } else { a::ERROR_DEL_DISCO });
                }
                let n = (n as usize).min(tam - hecho);
                unsafe { core::ptr::copy_nonoverlapping(VA_VENTANA as *const u8, (VA_PROGRAMA + hecho) as *mut u8, n) };
                hecho += n;
            }
            dashos::memoria_quitar(VA_PROGRAMA);
            Ok((m as u32, tam))
        })();
        self.orden(a::CERRAR, asa, 0, 0, b"");
        r
    }

    /// Una orden con `nombre` y `b`; el estado de la respuesta.
    fn pedir(&self, orden: u32, b: u64, nombre: &[u8]) -> i32 {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(256 - 32);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 {
            return -1;
        }
        let mut r = [0u8; 32];
        let (n, _) = dashos::canal_leer(self.lectura, &mut r, &mut []);
        if n != 32 { -1 } else { i32::from_le_bytes(r[0..4].try_into().unwrap()) }
    }

    /// Abre el volumen `nombre` con `clave` (o, con `solo_comprobar`, mira si es la suya sin abrirlo): la clave va por la
    /// ventana, no en el mensaje (A13), y la borra el servicio.
    fn abrir_volumen(&self, nombre: &[u8], clave: &[u8], solo_comprobar: bool) -> i32 {
        let clave = &clave[..clave.len().min(128)];
        let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, 128) };
        ventana[..clave.len()].copy_from_slice(clave);
        use dashos::protocolo::archivos::{ABRIR_VOLUMEN, COMPROBAR_CLAVE};
        let r = self.pedir(if solo_comprobar { COMPROBAR_CLAVE } else { ABRIR_VOLUMEN }, clave.len() as u64, nombre);
        // Por si el servicio no llegó a leerla.
        ventana.fill(0);
        r
    }
}
const MAX: usize = 64;

#[derive(Clone, Copy)]
struct Ficha {
    marca: u64,
    uid: u32,
    gid: u32,
    nombre: [u8; 16],
    grupo: u32,
    /// El cajón del disco de donde se lanzó (U2, `PROGDIR:`); vacío si no salió del disco.
    progdir: [u8; 24],
}

/// Las cuentas, provisionales (luego, `Estado:Cuentas/usuarios`): usuario y nombre, que es también el de su volumen
/// cifrado y su `Home:`.
const CUENTAS: &[(u32, &str)] = &[(1000, "Ana"), (1001, "Carla")];

/// El alta en archivos (orden 100, sin marca) de lo que lanza: su usuario y su grupo, sin derechos, la raíz 0 en el
/// cajón de donde sale y sus otras raíces: el disco para leer y, si tiene cuenta, su volumen para leer y escribir.
fn alta_archivos(pid: u64, uid: u32, gid: u32, dir: &[u8], cuenta: Option<&[u8]>) {
    let mut m = [0u8; 160];
    let dir = &dir[..dir.len().min(64)];
    m[0..4].copy_from_slice(&dashos::protocolo::archivos::ALTA.to_le_bytes());
    m[8..16].copy_from_slice(&pid.to_le_bytes());
    m[16..20].copy_from_slice(&uid.to_le_bytes());
    m[20..24].copy_from_slice(&gid.to_le_bytes());
    m[24..28].copy_from_slice(&(dir.len() as u32).to_le_bytes());
    let mut o = 32;
    m[o..o + dir.len()].copy_from_slice(dir);
    o += dir.len();
    let mut raiz = |hueco: u8, tope: u8, vol: &[u8], cam: &[u8]| {
        m[o..o + 4].copy_from_slice(&[hueco, tope, vol.len() as u8, cam.len() as u8]);
        m[o + 4..o + 4 + vol.len()].copy_from_slice(vol);
        m[o + 4 + vol.len()..o + 4 + vol.len() + cam.len()].copy_from_slice(cam);
        o += 4 + vol.len() + cam.len();
    };
    raiz(RAIZ_SYS, 4, b"", b"");
    if let Some(c) = cuenta {
        raiz(RAIZ_HOME, 6, &c[..c.len().min(16)], b"");
    }
    dashos::canal_escribir(ARCHIVOS_ALTAS, &m[..o], &[]);
}

/// Da a `marca` una llave del servicio `s` (U4): una copia marcada con su número, que viaja una vez (S4) y llega sin
/// el derecho de transferirla (regla 2), y revocable. Apunta su revocador y su entrada en el directorio del servicio.
fn dar_servicio(dadas: &mut [Option<(u64, usize, u32)>], s: usize, marca: u64, programa: &[u8; 16]) -> (i32, Option<u32>) {
    let d = dashos::llave_duplicar(REPARTE[s].1, 7 | dashos::UNA_VEZ, marca);
    let (r, revocador) = if d > 0 { dashos::llave_revocable(d as u32) } else { (-1, 0) };
    if d > 0 {
        dashos::cerrar(d as u32);
    }
    match dadas.iter().position(|x| x.is_none()) {
        Some(i) if r > 0 => {
            dadas[i] = Some((marca, s, revocador));
            // Su entrada en el directorio del servicio, como hace `inicio`.
            let mut dir = [0u8; 24];
            dir[..8].copy_from_slice(&marca.to_le_bytes());
            dir[8..].copy_from_slice(programa);
            dashos::canal_escribir(REPARTE[s].1, &dir, &[]);
            (BIEN, Some(r as u32))
        }
        _ => {
            if r > 0 {
                dashos::cerrar(r as u32);
                dashos::cerrar(revocador);
            }
            (-1, None)
        }
    }
}

/// Lee los permisos concedidos (U4): una línea "programa servicio" por permiso, en el cajón de `sesiones`.
fn leer_permisos(a: &Archivos, concedidos: &mut [Option<([u8; 16], usize)>]) {
    use dashos::protocolo::archivos as x;
    let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    if e != x::BIEN {
        return;
    }
    let (e, _, n) = a.orden(x::LEER, asa, 0, tam.min(4096), b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
    if e != x::BIEN {
        return;
    }
    let texto = unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, n as usize) };
    let mut van = 0;
    for linea in texto.split(|&c| c == b'\n') {
        let mut partes = linea.splitn(2, |&c| c == b' ');
        let (Some(p), Some(sv)) = (partes.next(), partes.next()) else { continue };
        let Some(s) = REPARTE.iter().position(|r| r.0.as_bytes() == sv) else { continue };
        if p.is_empty() || p.len() > 16 || van >= concedidos.len() {
            continue;
        }
        let mut nombre_ = [0u8; 16];
        nombre_[..p.len()].copy_from_slice(p);
        concedidos[van] = Some((nombre_, s));
        van += 1;
    }
    println!("  [sesiones] permisos concedidos en el disco: {}", van);
}

/// Reescribe el fichero de permisos con lo concedido que queda.
fn reescribir_permisos(a: &Archivos, concedidos: &[Option<([u8; 16], usize)>]) {
    use dashos::protocolo::archivos as x;
    let (e, asa, _) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    if e != x::BIEN {
        return;
    }
    a.orden(x::TAMANO, asa, 0, 0, b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
    for (p, s) in concedidos.iter().flatten() {
        guardar_permiso(a, nombre(p), REPARTE[*s].0);
    }
}

/// Añade al fichero de permisos lo que el usuario concedió.
fn guardar_permiso(a: &Archivos, programa: &str, servicio: &str) {
    use dashos::protocolo::archivos as x;
    let (e, asa, tam) = a.orden(x::BUSCAR, 0, 0, 0, CAMINO_PERMISOS);
    let (asa, tam) = if e == x::BIEN {
        (asa, tam)
    } else {
        let (e, cajon, _) = a.orden(x::BUSCAR, 0, 0, 0, CAJON);
        if e != x::BIEN {
            return;
        }
        let (e, asa, _) = a.orden(x::CREAR, cajon, x::FICHERO as u64, 0, b"permisos");
        a.orden(x::CERRAR, cajon, 0, 0, b"");
        if e != x::BIEN {
            return;
        }
        (asa, 0)
    };
    let linea_ = [programa.as_bytes(), b" ", servicio.as_bytes(), b"\n"];
    let ventana = unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, 64) };
    let mut n = 0;
    for parte in linea_ {
        ventana[n..n + parte.len()].copy_from_slice(parte);
        n += parte.len();
    }
    a.orden(x::ESCRIBIR, asa, tam, n as u64, b"");
    a.orden(x::CERRAR, asa, 0, 0, b"");
}

fn nombre(n: &[u8; 16]) -> &str {
    core::str::from_utf8(&n[..n.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

fn principal() -> u64 {
    let mut fichas: [Option<Ficha>; MAX] = [None; MAX];
    // Las llaves de servicios repartidas: a quién (marca), de qué servicio y su revocador.
    let mut dadas: [Option<(u64, usize, u32)>; MAX_DADAS] = [None; MAX_DADAS];
    // U4: lo que el usuario concedió (programa y servicio), las preguntas a `guardian` sin contestar (número, llave de
    // respuesta del programa, su marca, su nombre y el servicio) y por dónde se le pregunta.
    let mut concedidos: [Option<([u8; 16], usize)>; MAX_CONCEDIDOS] = [None; MAX_CONCEDIDOS];
    let mut preguntas: [Option<(u32, u32, u64, [u8; 16], usize)>; MAX_PREGUNTAS] = [None; MAX_PREGUNTAS];
    let mut canal_guardian = 0u32;
    let mut siguiente_pregunta = 1u32;
    let mut registro = Registro { entradas: [[0; ENTRADA_REGISTRO]; REGISTRO_MAX], total: 0, pendientes: 0, asa: 0 };
    let mut siguiente_pid = PRIMER_PID_PROPIO;
    // Las últimas bajas, para decir cuándo vuelve un servicio relanzado por `inicio`.
    // Los usuarios con la sesión abierta (U2/U3): solo como ellos puede `guardian` lanzar algo.
    let mut abiertas = [0u32; 8];
    let mut idos = [0u64; 8];
    let mut ido = 0;
    let mut b = [0u8; dashos::servicio::MAX_DATOS];
    let mut ll = [0u32; dashos::servicio::MAX_LLAVES];
    let archivos = Archivos::abrir();
    println!("  [sesiones] en marcha{}", if archivos.is_some() { ", con el servicio de archivos" } else { ", sin el servicio de archivos" });
    if let Some(a) = &archivos {
        registro.abrir_disco(a);
        leer_permisos(a, &mut concedidos);
    }
    loop {
        // Lo que se apuntó en la vuelta anterior, al disco antes de esperar (U6).
        if registro.pendientes > 0 {
            if let Some(a) = &archivos {
                registro.guardar(a);
            }
        }
        let (n, k, marca) = match dashos::servicio::recibir(CANAL, &mut b, &mut ll) {
            dashos::servicio::Recibido::Mensaje { n, k, marca } => (n, k, marca),
            dashos::servicio::Recibido::Cerrado => return 0,
            dashos::servicio::Recibido::Nada => continue,
        };
        let m = &b[..n];
        let u32_en = |i: usize| m.get(i..i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
        let u64_en = |i: usize| m.get(i..i + 8).map_or(0, |x| u64::from_le_bytes(x.try_into().unwrap()));
        // Las llaves que sobren se cierran siempre (E2).
        let mut usadas = 0;
        if marca >> 63 == 1 && k == 0 && n == 16 {
            // La baja: el proceso acabó.
            let de = marca & !(1 << 63);
            if let Some(i) = fichas.iter().position(|x| x.is_some_and(|x| x.marca == de)) {
                let f = fichas[i].take().unwrap();
                dashos::cerrar(f.grupo);
                // Lo que lanzó `sesiones` tenía alta en archivos: que la suelte, con lo que dejara abierto.
                if f.marca >= PRIMER_PID_PROPIO {
                    let mut b = [0u8; dashos::protocolo::archivos::CABECERA];
                    b[0..4].copy_from_slice(&dashos::protocolo::archivos::BAJA.to_le_bytes());
                    b[8..16].copy_from_slice(&f.marca.to_le_bytes());
                    dashos::canal_escribir(ARCHIVOS_ALTAS, &b, &[]);
                }
                if f.marca >= PRIMER_PID_PROPIO {
                    println!("  [sesiones] baja de {} (proceso {}, clase {})", nombre(&f.nombre), f.marca, u64_en(0));
                } else {
                    println!("  [sesiones] baja de {} (clase {})", nombre(&f.nombre), u64_en(0));
                }
                if u64_en(0) == 2 {
                    registro.apuntar(R_FALLO, 2, &f.nombre, b"");
                }
                idos[ido % idos.len()] = f.marca;
                ido += 1;
            }
        } else if marca == 0 {
            // De `inicio`: un alta, con la llave del grupo.
            if u32_en(0) == ALTA && m.len() >= 36 && k >= 1 {
                let mut nombre_ = [0u8; 16];
                nombre_.copy_from_slice(&m[20..36]);
                let f = Ficha { marca: u64_en(4), uid: u32_en(12), gid: u32_en(16), nombre: nombre_, grupo: ll[0], progdir: [0; 24] };
                let sitio = fichas.iter().position(|x| x.is_some_and(|x| x.marca == f.marca)).or_else(|| fichas.iter().position(|x| x.is_none()));
                if let Some(i) = sitio {
                    if let Some(vieja) = fichas[i] {
                        dashos::cerrar(vieja.grupo);
                    }
                    if let Some(j) = idos.iter().position(|&m| m == f.marca && m != 0) {
                        idos[j] = 0;
                        println!("  [sesiones] vuelve {}, con su grupo nuevo", nombre(&f.nombre));
                    }
                    fichas[i] = Some(f);
                    usadas = 1;
                }
            }
        } else if k >= 1 {
            // De un programa: contestar por la llave que trae.
            let respuesta = ll[0];
            usadas = 1;
            let yo = fichas.iter().flatten().find(|f| f.marca == marca).copied();
            match u32_en(0) {
                PROCESOS => {
                    let desde = u64_en(4) as usize;
                    let total = fichas.iter().flatten().count();
                    let mut r = [0u8; 8 + 32 * POR_PAGINA];
                    let mut i = 0;
                    for f in fichas.iter().flatten().skip(desde).take(POR_PAGINA) {
                        let o = 8 + 32 * i;
                        r[o..o + 8].copy_from_slice(&f.marca.to_le_bytes());
                        r[o + 8..o + 12].copy_from_slice(&f.uid.to_le_bytes());
                        r[o + 12..o + 16].copy_from_slice(&f.gid.to_le_bytes());
                        r[o + 16..o + 32].copy_from_slice(&f.nombre);
                        i += 1;
                    }
                    r[0..4].copy_from_slice(&(i as u32).to_le_bytes());
                    r[4..8].copy_from_slice(&(total as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..8 + 32 * i], &[]);
                }
                ENTRAR | SALIR | VOLVER => {
                    // Solo de `guardian` (U3): nadie más abre ni cierra el volumen de un usuario.
                    let guardian = yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre)));
                    let (nl, cl) = (u32_en(4) as usize, u32_en(8) as usize);
                    let estado = match (&archivos, guardian, m.get(12..12 + nl), m.get(12 + nl..12 + nl + cl)) {
                        (_, false, ..) => dashos::protocolo::archivos::PROTEGIDO,
                        (Some(a), true, Some(usuario), Some(clave)) if u32_en(0) == ENTRAR => a.abrir_volumen(usuario, clave, false),
                        (Some(a), true, Some(usuario), Some(clave)) if u32_en(0) == VOLVER => a.abrir_volumen(usuario, clave, true),
                        // Salir con programas suyos aún vivos (los que lanzó `sesiones` como él), no: -13, como con
                        // algo abierto en su volumen.
                        (Some(_), true, Some(usuario), _)
                            if CUENTAS.iter().find(|c| c.1.as_bytes() == usuario).is_some_and(|&(uid, _)| {
                                fichas.iter().flatten().any(|f| f.uid == uid && f.marca >= PRIMER_PID_PROPIO)
                            }) =>
                        {
                            -13
                        }
                        (Some(a), true, Some(usuario), _) => a.pedir(dashos::protocolo::archivos::CERRAR_VOLUMEN, 0, usuario),
                        _ => -1,
                    };
                    let quien = m.get(12..12 + nl).and_then(|u| core::str::from_utf8(u).ok()).unwrap_or("?");
                    let que = match u32_en(0) { ENTRAR => "entrar", VOLVER => "volver", _ => "salir" };
                    // Tras una contraseña mala, un rato sin contestar: probar a ciegas sale caro.
                    if estado == dashos::protocolo::archivos::PROTEGIDO && guardian {
                        dashos::dormir_us(300_000);
                    }
                    let pide = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    println!("  [sesiones] {} pide {} como {}: {}", nombre(&pide), que, quien, estado);
                    if estado == dashos::protocolo::archivos::PROTEGIDO {
                        registro.apuntar(R_ENTRAR, estado, &pide, quien.as_bytes());
                    }
                    if estado == BIEN && guardian {
                        if let Some(&(uid, _)) = CUENTAS.iter().find(|c| c.1.as_bytes() == quien.as_bytes()) {
                            match u32_en(0) {
                                ENTRAR if !abiertas.contains(&uid) => {
                                    if let Some(x) = abiertas.iter_mut().find(|x| **x == 0) {
                                        *x = uid;
                                    }
                                }
                                SALIR => abiertas.iter_mut().filter(|x| **x == uid).for_each(|x| *x = 0),
                                _ => {}
                            }
                        }
                    }
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                    // La contraseña, fuera de la memoria en cuanto se usa (S6b, regla 9).
                    b.fill(0);
                }
                LANZAR => {
                    // Como quién (bytes 8 a 11): 0, como quien pide; si no, ese usuario, que tiene que haber entrado.
                    let como = u32_en(8);
                    let guardian = yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre))) && (como == 0 || abiertas.contains(&como));
                    let nl = u32_en(4) as usize;
                    let camino = m.get(12..12 + nl).unwrap_or(b"");
                    let pide = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let yo = if como == 0 { yo } else { yo.map(|f| Ficha { uid: como, gid: como, ..f }) };
                    let (estado, pid) = match (&archivos, guardian, fichas.iter().position(|x| x.is_none())) {
                        (_, false, _) => {
                            registro.apuntar(R_LANZAR, PROTEGIDO, &pide, camino);
                            (PROTEGIDO, 0)
                        }
                        (None, ..) | (_, _, None) => (-1, 0),
                        (Some(a), true, Some(hueco)) => match a.leer_entero(camino) {
                            Err(e) => (e, 0),
                            Ok((mem, tam)) => {
                                // Su grupo sale del cupo de `sesiones` (16 MB): 4 MB para cada uno, de momento.
                                let g = dashos::grupo_crear(PRESUPUESTO_LANZADO);
                                // Nace con una llave: su canal con `sesiones`, marcado con su número (que viaja una vez).
                                let pid = siguiente_pid;
                                let canal_suyo = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                                // Su alta en archivos, antes que él: su usuario, y sus raíces (U2).
                                let (uid, gid) = yo.map_or((1, 1), |f| (f.uid, f.gid));
                                let corto_ = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                                let dir = &camino[..camino.len() - corto_.len()];
                                let dir = dir.strip_suffix(b"/").unwrap_or(dir);
                                let cuenta = CUENTAS.iter().find(|c| c.0 == uid).map(|c| c.1.as_bytes());
                                alta_archivos(pid, uid, gid, dir, cuenta);
                                // Y su llave de archivos, marcada con su número: va la segunda.
                                let archivos_suyo = dashos::llave_duplicar(ARCHIVOS_ALTAS, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, pid);
                                let todas = [canal_suyo as u32, archivos_suyo as u32];
                                let llaves: &[u32] = match (canal_suyo > 0, archivos_suyo > 0) {
                                    (true, true) => &todas,
                                    (true, false) => &todas[..1],
                                    _ => &[],
                                };
                                let p = if g > 0 { dashos::proceso_crear_en(g as u32, mem, tam, llaves) } else { g };
                                if p <= 0 {
                                    for &l in llaves {
                                        dashos::cerrar(l);
                                    }
                                }
                                dashos::cerrar(mem);
                                if p <= 0 {
                                    if g > 0 {
                                        dashos::cerrar(g as u32);
                                    }
                                    (p as i32, 0)
                                } else {
                                    // Su ficha: el usuario de quien lo pide, y el nombre del fichero. Y su baja, con el aviso
                                    // de fin (K7) por su propio canal, marcado con el bit alto como los de `inicio`.
                                    siguiente_pid += 1;
                                    let aviso = dashos::llave_duplicar(PROPIO, dashos::ESCRIBIR, (1 << 63) | pid);
                                    if aviso > 0 {
                                        dashos::proceso_avisar(p as u32, aviso as u32);
                                        dashos::cerrar(aviso as u32);
                                    }
                                    let corto = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                                    let mut n16 = [0u8; 16];
                                    n16[..corto.len().min(16)].copy_from_slice(&corto[..corto.len().min(16)]);
                                    let (uid, gid) = yo.map_or((1, 1), |f| (f.uid, f.gid));
                                    let dir = &camino[..camino.len() - corto.len()];
                                    let dir = dir.strip_suffix(b"/").unwrap_or(dir);
                                    let mut progdir = [0u8; 24];
                                    progdir[..dir.len().min(24)].copy_from_slice(&dir[..dir.len().min(24)]);
                                    fichas[hueco] = Some(Ficha { marca: pid, uid, gid, nombre: n16, grupo: g as u32, progdir });
                                    dashos::cerrar(p as u32);
                                    (BIEN, pid)
                                }
                            }
                        },
                    };
                    println!("  [sesiones] {} lanza {}: {} (proceso {})", nombre(&pide), core::str::from_utf8(camino).unwrap_or("?"), estado, pid);
                    let mut r = [0u8; 12];
                    r[0..4].copy_from_slice(&estado.to_le_bytes());
                    r[4..12].copy_from_slice(&pid.to_le_bytes());
                    dashos::canal_escribir(respuesta, &r, &[]);
                }
                NOMBRES => {
                    // Su tabla: la de todos, más `Home:` si su usuario tiene cuenta y `PROGDIR:` si salió del disco.
                    // Sin ficha, vacía.
                    let mut r = [0u8; 4 + ENTRADA_NOMBRE * MAX_NOMBRES];
                    let mut van = 0;
                    let mut poner = |nombre: &[u8], raiz: u8, vol: &[u8], sub: &[u8]| {
                        let o = 4 + ENTRADA_NOMBRE * van;
                        r[o..o + nombre.len().min(12)].copy_from_slice(&nombre[..nombre.len().min(12)]);
                        r[o + 12] = raiz;
                        r[o + 13..o + 13 + vol.len().min(11)].copy_from_slice(&vol[..vol.len().min(11)]);
                        r[o + 24..o + 24 + sub.len().min(24)].copy_from_slice(&sub[..sub.len().min(24)]);
                        van += 1;
                    };
                    if let Some(f) = yo {
                        // Lo lanzado desde el disco nace con sus raíces (su alta en archivos); lo demás, sin ellas, va
                        // por volumen y subcamino desde su raíz 0.
                        let lanzado = f.progdir[0] != 0;
                        let (sys, home, progdir) = if lanzado { (RAIZ_SYS, RAIZ_HOME, 0) } else { (SIN_RAIZ, SIN_RAIZ, SIN_RAIZ) };
                        poner(b"SYS", sys, b"", b"");
                        poner(b"Programas", sys, b"", b"Programas");
                        if let Some(&(_, cuenta)) = CUENTAS.iter().find(|c| c.0 == f.uid) {
                            poner(b"Home", home, cuenta.as_bytes(), b"");
                        }
                        if lanzado {
                            let largo = f.progdir.iter().position(|&c| c == 0).unwrap_or(24);
                            poner(b"PROGDIR", progdir, b"", if progdir == SIN_RAIZ { &f.progdir[..largo] } else { b"" });
                        }
                    }
                    r[0..4].copy_from_slice(&(van as u32).to_le_bytes());
                    dashos::canal_escribir(respuesta, &r[..4 + ENTRADA_NOMBRE * van], &[]);
                }
                REGISTRO => {
                    // Solo `guardian` lee el registro (U6).
                    if !yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre))) {
                        dashos::canal_escribir(respuesta, &PROTEGIDO.to_le_bytes(), &[]);
                    } else {
                        let desde = u64_en(4) as usize;
                        let mut r = [0u8; 8 + ENTRADA_REGISTRO * POR_PAGINA_REGISTRO];
                        let mut van = 0;
                        while van < POR_PAGINA_REGISTRO {
                            let Some(e) = registro.entrada(desde + van) else { break };
                            r[8 + ENTRADA_REGISTRO * van..8 + ENTRADA_REGISTRO * (van + 1)].copy_from_slice(e);
                            van += 1;
                        }
                        r[0..4].copy_from_slice(&(van as u32).to_le_bytes());
                        r[4..8].copy_from_slice(&(registro.total.min(REGISTRO_MAX) as u32).to_le_bytes());
                        dashos::canal_escribir(respuesta, &r[..8 + ENTRADA_REGISTRO * van], &[]);
                    }
                }
                SERVICIO | DEJAR => {
                    let nl = u32_en(4) as usize;
                    let pedido = m.get(12..12 + nl).and_then(|n| core::str::from_utf8(n).ok()).unwrap_or("");
                    let servicio = REPARTE.iter().position(|s| s.0 == pedido);
                    let programa = yo.map(|f| f.nombre).unwrap_or([0; 16]);
                    let permitido = yo.is_some()
                        && (PERMISOS.iter().any(|p| p.0 == nombre(&programa) && p.1 == pedido)
                            || servicio.is_some_and(|s| concedidos.iter().flatten().any(|c| c.0 == programa && c.1 == s)));
                    let mut respuesta_llave = None;
                    let mut en_espera = false;
                    let estado = match (u32_en(0), servicio) {
                        (_, None) => NO_EXISTE,
                        (SERVICIO, Some(s)) if permitido => {
                            let (e, l) = dar_servicio(&mut dadas, s, marca, &programa);
                            respuesta_llave = l;
                            e
                        }
                        // Sin permiso: si `guardian` atiende preguntas, se le pregunta al usuario y la respuesta
                        // espera (sin parar a los demás).
                        (SERVICIO, Some(s)) if yo.is_some() && canal_guardian != 0 && preguntas.iter().any(|p| p.is_none()) => {
                            let id = siguiente_pregunta;
                            siguiente_pregunta = siguiente_pregunta.wrapping_add(1).max(1);
                            let mut q = [0u8; PREGUNTA];
                            q[0..4].copy_from_slice(&id.to_le_bytes());
                            q[4..8].copy_from_slice(&yo.map_or(0, |f| f.uid).to_le_bytes());
                            q[8..24].copy_from_slice(&programa);
                            q[24..24 + pedido.len().min(16)].copy_from_slice(&pedido.as_bytes()[..pedido.len().min(16)]);
                            if dashos::canal_escribir(canal_guardian, &q, &[]) == 0 {
                                let hueco = preguntas.iter().position(|p| p.is_none()).unwrap();
                                preguntas[hueco] = Some((id, respuesta, marca, programa, s));
                                en_espera = true;
                                BIEN
                            } else {
                                registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes());
                                PROTEGIDO
                            }
                        }
                        (SERVICIO, Some(_)) => {
                            registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, pedido.as_bytes());
                            PROTEGIDO
                        }
                        (_, Some(s)) => {
                            // Dejar: se revocan todas las suyas de ese servicio.
                            let mut alguna = false;
                            for x in dadas.iter_mut() {
                                if let Some((mc, sv, rev)) = *x {
                                    if mc == marca && sv == s {
                                        dashos::revocar(rev);
                                        dashos::cerrar(rev);
                                        *x = None;
                                        alguna = true;
                                    }
                                }
                            }
                            if alguna { BIEN } else { NO_EXISTE }
                        }
                    };
                    let que = if u32_en(0) == SERVICIO { "pide" } else { "deja" };
                    if en_espera {
                        println!("  [sesiones] {} pide {}: se pregunta al usuario", nombre(&programa), pedido);
                        continue;
                    }
                    println!("  [sesiones] {} {} {}: {}", nombre(&programa), que, pedido, estado);
                    match respuesta_llave {
                        Some(l) => {
                            if dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[l]) != 0 {
                                dashos::cerrar(l);
                            }
                        }
                        None => {
                            dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                        }
                    }
                }
                PREGUNTAS if k >= 2 && yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre))) => {
                    // `guardian` atiende las preguntas al usuario (U4) por la llave que manda.
                    if canal_guardian != 0 {
                        dashos::cerrar(canal_guardian);
                    }
                    canal_guardian = ll[1];
                    usadas = 2;
                    println!("  [sesiones] {} atiende las preguntas al usuario", nombre(&yo.unwrap().nombre));
                    dashos::canal_escribir(respuesta, &BIEN.to_le_bytes(), &[]);
                }
                RESPUESTA if yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre))) => {
                    dashos::canal_escribir(respuesta, &BIEN.to_le_bytes(), &[]);
                    let (id, si) = (u32_en(4), u32_en(8) == 1);
                    if let Some(i) = preguntas.iter().position(|p| p.is_some_and(|p| p.0 == id)) {
                        let (_, a_quien, de, programa, s) = preguntas[i].take().unwrap();
                        let (estado, llave) = if si {
                            // Concedido: se apunta y se guarda, y no se vuelve a preguntar.
                            if let Some(c) = concedidos.iter_mut().find(|c| c.is_none()) {
                                *c = Some((programa, s));
                            }
                            if let Some(a) = &archivos {
                                guardar_permiso(a, nombre(&programa), REPARTE[s].0);
                            }
                            dar_servicio(&mut dadas, s, de, &programa)
                        } else {
                            registro.apuntar(R_SERVICIO, PROTEGIDO, &programa, REPARTE[s].0.as_bytes());
                            (PROTEGIDO, None)
                        };
                        println!("  [sesiones] {} pide {}: el usuario dice {} ({})", nombre(&programa), REPARTE[s].0, if si { "sí" } else { "no" }, estado);
                        match llave {
                            Some(l) => {
                                if dashos::canal_escribir(a_quien, &estado.to_le_bytes(), &[l]) != 0 {
                                    dashos::cerrar(l);
                                }
                            }
                            None => {
                                dashos::canal_escribir(a_quien, &estado.to_le_bytes(), &[]);
                            }
                        }
                        dashos::cerrar(a_quien);
                    }
                }
                RETIRAR => {
                    let guardian = yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre)));
                    let (lp, ls) = (u32_en(4) as usize, u32_en(8) as usize);
                    let (prog, serv) = (m.get(12..12 + lp).unwrap_or(b""), m.get(12 + lp..12 + lp + ls).unwrap_or(b""));
                    let quien = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
                    let estado = if !guardian {
                        registro.apuntar(R_SERVICIO, PROTEGIDO, &quien, prog);
                        PROTEGIDO
                    } else {
                        let s = REPARTE.iter().position(|r| r.0.as_bytes() == serv);
                        let mut quitado = false;
                        for c in concedidos.iter_mut() {
                            if c.is_some_and(|c| Some(c.1) == s && &c.0[..c.0.iter().position(|&x| x == 0).unwrap_or(16)] == prog) {
                                *c = None;
                                quitado = true;
                            }
                        }
                        if quitado {
                            // Las llaves que ya tenga dejan de servir: el permiso se retira ya, no al pedirlo otra vez.
                            for x in dadas.iter_mut() {
                                if let Some((mc, sv, rev)) = *x {
                                    let de_el = fichas.iter().flatten().any(|f| f.marca == mc && nombre(&f.nombre).as_bytes() == prog);
                                    if Some(sv) == s && de_el {
                                        dashos::revocar(rev);
                                        dashos::cerrar(rev);
                                        *x = None;
                                    }
                                }
                            }
                            if let Some(a) = &archivos {
                                reescribir_permisos(a, &concedidos);
                            }
                            BIEN
                        } else {
                            NO_EXISTE
                        }
                    };
                    println!("  [sesiones] {} retira {} {}: {}", nombre(&quien), core::str::from_utf8(prog).unwrap_or("?"), core::str::from_utf8(serv).unwrap_or("?"), estado);
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                MATAR => {
                    let objetivo = u64_en(4);
                    let i = fichas.iter().position(|x| x.is_some_and(|x| x.marca == objetivo));
                    let estado = match (yo, i) {
                        (_, None) => NO_EXISTE,
                        (None, _) => PROTEGIDO,
                        (Some(yo), Some(i)) => {
                            let f = fichas[i].unwrap();
                            if f.uid != yo.uid {
                                println!("  [sesiones] {} (usuario {}) no puede matar a {} (usuario {})", nombre(&yo.nombre), yo.uid, nombre(&f.nombre), f.uid);
                                registro.apuntar(R_MATAR, PROTEGIDO, &yo.nombre, &f.nombre);
                                PROTEGIDO
                            } else {
                                dashos::grupo_matar(f.grupo);
                                dashos::cerrar(f.grupo);
                                fichas[i] = None;
                                println!("  [sesiones] {} mata a {} (los dos, usuario {})", nombre(&yo.nombre), nombre(&f.nombre), f.uid);
                                BIEN
                            }
                        }
                    };
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                }
                _ => {}
            }
            dashos::cerrar(respuesta);
        }
        for &l in &ll[usadas..k] {
            dashos::cerrar(l);
        }
    }
}
