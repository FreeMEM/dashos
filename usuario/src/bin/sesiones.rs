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
/// Los servicios que reparte, con la llave sin marca de cada uno.
const REPARTE: &[(&str, u32)] = &[("eco", 2)];
/// Los permisos de cada programa (U4), provisionales hasta que salgan del paquete (S8) y de lo que diga el usuario.
const PERMISOS: &[(&str, &str)] = &[("pide", "eco")];
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

/// El registro de seguridad (U6): las últimas `REGISTRO_MAX` negativas y fallos, en memoria. (Al disco, cuando
/// `sesiones` tenga su cajón: hoy no puede escribir en el disco de pruebas.)
const REGISTRO_MAX: usize = 64;
struct Registro {
    entradas: [[u8; ENTRADA_REGISTRO]; REGISTRO_MAX],
    total: usize,
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
}

fn nombre(n: &[u8; 16]) -> &str {
    core::str::from_utf8(&n[..n.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

fn principal() -> u64 {
    let mut fichas: [Option<Ficha>; MAX] = [None; MAX];
    // Las llaves de servicios repartidas: a quién (marca), de qué servicio y su revocador.
    let mut dadas: [Option<(u64, usize, u32)>; MAX_DADAS] = [None; MAX_DADAS];
    let mut registro = Registro { entradas: [[0; ENTRADA_REGISTRO]; REGISTRO_MAX], total: 0 };
    let mut siguiente_pid = PRIMER_PID_PROPIO;
    // Las últimas bajas, para decir cuándo vuelve un servicio relanzado por `inicio`.
    let mut idos = [0u64; 8];
    let mut ido = 0;
    let mut b = [0u8; dashos::servicio::MAX_DATOS];
    let mut ll = [0u32; dashos::servicio::MAX_LLAVES];
    let archivos = Archivos::abrir();
    println!("  [sesiones] en marcha{}", if archivos.is_some() { ", con el servicio de archivos" } else { ", sin el servicio de archivos" });
    loop {
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
                println!("  [sesiones] baja de {} (clase {})", nombre(&f.nombre), u64_en(0));
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
                let f = Ficha { marca: u64_en(4), uid: u32_en(12), gid: u32_en(16), nombre: nombre_, grupo: ll[0] };
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
                    dashos::canal_escribir(respuesta, &estado.to_le_bytes(), &[]);
                    // La contraseña, fuera de la memoria en cuanto se usa (S6b, regla 9).
                    b.fill(0);
                }
                LANZAR => {
                    let guardian = yo.is_some_and(|f| GUARDIANES.contains(&nombre(&f.nombre)));
                    let nl = u32_en(4) as usize;
                    let camino = m.get(12..12 + nl).unwrap_or(b"");
                    let pide = yo.map(|f| f.nombre).unwrap_or([b'?'; 16]);
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
                                let p = if g > 0 { dashos::proceso_crear_en(g as u32, mem, tam, &[]) } else { g };
                                dashos::cerrar(mem);
                                if p <= 0 {
                                    if g > 0 {
                                        dashos::cerrar(g as u32);
                                    }
                                    (p as i32, 0)
                                } else {
                                    // Su ficha: el usuario de quien lo pide, y el nombre del fichero.
                                    let pid = siguiente_pid;
                                    siguiente_pid += 1;
                                    let corto = camino.rsplit(|&c| c == b'/').next().unwrap_or(camino);
                                    let mut n16 = [0u8; 16];
                                    n16[..corto.len().min(16)].copy_from_slice(&corto[..corto.len().min(16)]);
                                    let (uid, gid) = yo.map_or((1, 1), |f| (f.uid, f.gid));
                                    fichas[hueco] = Some(Ficha { marca: pid, uid, gid, nombre: n16, grupo: g as u32 });
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
                    let permitido = yo.is_some() && PERMISOS.iter().any(|p| p.0 == nombre(&programa) && p.1 == pedido);
                    let mut respuesta_llave = None;
                    let estado = match (u32_en(0), servicio) {
                        (_, None) => NO_EXISTE,
                        (SERVICIO, Some(s)) if permitido => {
                            // Una copia marcada con su número, que viaja una vez (S4) y llega sin el derecho de
                            // transferirla (regla 2), y revocable.
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
                                    dir[8..].copy_from_slice(&programa);
                                    dashos::canal_escribir(REPARTE[s].1, &dir, &[]);
                                    respuesta_llave = Some(r as u32);
                                    BIEN
                                }
                                _ => {
                                    if r > 0 {
                                        dashos::cerrar(r as u32);
                                        dashos::cerrar(revocador);
                                    }
                                    -1
                                }
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
