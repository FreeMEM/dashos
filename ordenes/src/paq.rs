//! `Paq`, el gestor de paquetes (S8): la orden (`SYS:C/Paq`) y el servicio `paq` (lo lanza `inicio`). Lo de dentro es la
//! biblioteca `paq` (`paq/`); aquí, el disco de DashOS (`Sistema` sobre el servicio de archivos), lo que se escribe y
//! cómo se pide.
//!
//! - Lo de cada usuario (`Home:Paquetes`, con `HOME`) lo hace la orden, con los permisos de quien la usa: es suyo.
//! - Lo de todos (`Paquetes:`) solo lo escribe el servicio `paq`. La orden hace el plan y lo pide como una tanda de
//!   acciones de administrador (S6b, "un plan, una contraseña"): `guardian` enseña la lista entera y pide la contraseña
//!   una vez, y `sesiones` pasa al servicio las líneas confirmadas, que hace solo esas. El servicio solo atiende a
//!   `sesiones` (las llaves de su canal no las tiene nadie más) y lo vuelve a comprobar todo: firmas, huellas, el plan.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use dashos::asignaciones::Tabla;
use dashos::protocolo::archivos as a;
use paq::cambio::{self, Cambio};
use paq::config;
use paq::disco::{self as d, Disco};
use paq::generacion::Motivo;
use paq::motor::{self, Conservar, Estado, Nivel, Repos};
use paq::texto::{bonita, fecha_corta};
use paq::{Fallo, fallo};
use pkg2::plan::Maquina;

use crate::{Archivos, Orden, Sistema, di};

/// La plantilla de la orden.
pub const PLANTILLA: &str = "ACCION/A,NOMBRES/M,HOME/S,TODO/S";

// ---------------------------------------------------------------------------------------------------------------
// El disco de DashOS.

impl Disco for Sistema {
    fn leer(&mut self, ruta: &str, tope: usize) -> Result<Vec<u8>, i32> {
        self.leer_entero(ruta, tope).map_err(|e| if e == a::SIN_SITIO { d::LLENO } else { e })
    }

    fn guardar(&mut self, ruta: &str, datos: &[u8]) -> Result<(), i32> {
        if datos.len() <= self.ventana() {
            return Sistema::guardar(self, ruta, datos);
        }
        // Más que la ventana (un manifiesto enorme): por trozos. Lo que se guarda así va siempre antes de su ficha,
        // que sí es de una vez, y sin ficha no cuenta.
        Disco::escribir(self, ruta, datos, 0o644)
    }

    fn escribir(&mut self, ruta: &str, datos: &[u8], modo: u16) -> Result<(), i32> {
        if self.mirar(ruta).is_ok() {
            return Err(a::YA_EXISTE);
        }
        let asa = self.crear(ruta, a::FICHERO)?;
        let r = Sistema::escribir(self, asa, 0, datos);
        self.cerrar(asa);
        r?;
        if modo & 0o777 != 0o644 {
            self.proteger(ruta, 0, modo & 0o777)?;
        }
        Ok(())
    }

    fn hacer_cajon(&mut self, ruta: &str) -> Result<(), i32> {
        let asa = self.crear(ruta, a::CAJON)?;
        self.cerrar(asa);
        Ok(())
    }

    fn listar(&mut self, ruta: &str) -> Result<Vec<(String, bool)>, i32> {
        Sistema::listar(self, ruta).map(|v| v.into_iter().map(|e| (e.nombre, e.datos.cajon())).collect())
    }

    fn renombrar(&mut self, de: &str, hacia: &str) -> Result<(), i32> {
        Sistema::renombrar(self, de, hacia)
    }

    fn borrar(&mut self, ruta: &str) -> Result<(), i32> {
        Sistema::borrar(self, ruta)
    }

    fn mirar(&mut self, ruta: &str) -> Result<Option<bool>, i32> {
        match Sistema::mirar(self, ruta) {
            Ok(x) => Ok(Some(x.cajon())),
            Err(a::NO_EXISTE) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn libre(&mut self, ruta: &str) -> Option<u64> {
        self.volumen(ruta).ok().map(|v| v.1)
    }

    fn hora(&mut self) -> Option<u64> {
        hora_s()
    }
}

fn hora_s() -> Option<u64> {
    Some(dashos::hora_ns() / 1_000_000_000).filter(|h| *h >= paq::texto::CREIBLE_DESDE)
}

/// Lo que dice un cerrojo, para saber luego si es de un Paq que ya no está: quién, cuándo arrancó la máquina y cuándo
/// se cogió (segundos desde que arrancó).
fn texto_cerrojo(quien: &str) -> String {
    let desde = dashos::ahora_us() / 1_000_000;
    let arranque = hora_s().map_or(0, |h| h.saturating_sub(desde));
    format!("{quien}; arranque {arranque}; desde {desde}")
}

/// Si el cerrojo `t` es de un Paq que ya no puede estar trabajando: de otro arranque, o de hace más de diez minutos
/// (lo que hace Paq en `Home:` dura segundos). Sin hora creíble no se sabe de qué arranque es: solo por el tiempo.
fn cerrojo_viejo(t: &str) -> bool {
    let num = |clave: &str| t.split("; ").find_map(|p| p.strip_prefix(clave)).and_then(|x| x.trim().parse::<u64>().ok());
    let ahora = dashos::ahora_us() / 1_000_000;
    let (Some(arranque), Some(desde)) = (num("arranque "), num("desde ")) else { return true };
    let otro_arranque = match hora_s() {
        Some(h) if arranque > 0 => h.saturating_sub(ahora).abs_diff(arranque) > 5,
        _ => false,
    };
    otro_arranque || desde > ahora || ahora - desde > 600
}

// ---------------------------------------------------------------------------------------------------------------
// La orden.

/// La máquina, para los requisitos de máquina de `aarch64-dashos` (`hypervisor`, la memoria). Pendiente: saberla de
/// verdad (el kernel no lo dice aún a los programas); sin eso, un paquete que pide alguna de las dos no se instala.
fn maquina() -> Maquina {
    Maquina::default()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Que {
    Instalar,
    Quitar,
    Buscar,
    Info,
    Actualizar,
    Generaciones,
    Volver,
    Limpiar,
    Repos,
    Claves,
}

fn que_es(t: &str) -> Option<Que> {
    Some(match t.to_ascii_lowercase().as_str() {
        "instalar" | "install" | "in" => Que::Instalar,
        "quitar" | "remove" | "rm" => Que::Quitar,
        "buscar" | "search" | "se" => Que::Buscar,
        "info" => Que::Info,
        "actualizar" | "upgrade" | "up" => Que::Actualizar,
        "generaciones" | "generations" | "gen" => Que::Generaciones,
        "volver" | "rollback" | "back" => Que::Volver,
        "limpiar" | "clean" => Que::Limpiar,
        "repos" => Que::Repos,
        "claves" | "keys" => Que::Claves,
        _ => return None,
    })
}

fn donde(home: bool) -> (Nivel, &'static str) {
    if home { (Nivel::del_usuario(), "para ti (Home:Paquetes)") } else { (Nivel::de_todos(), "para todos (Paquetes:)") }
}

fn avisos(o: &mut Orden, v: &[String]) {
    for x in v {
        di!(o, "Aviso: {}.\n", x);
    }
}

fn repos_avisando(o: &mut Orden, nivel: &Nivel) -> Option<Repos> {
    let vistas = motor::leer_series(&mut o.s, nivel);
    match motor::cargar_repos(&mut o.s, &vistas) {
        Ok(rs) => {
            avisos(o, &rs.avisos);
            for (r, por) in &rs.malos {
                di!(o, "Aviso: el repositorio {} no se usa: {}.\n", r, por);
            }
            Some(rs)
        }
        Err(f) => {
            o.error(format_args!("{}", f));
            None
        }
    }
}

fn fallar(o: &mut Orden, f: &Fallo) -> u64 {
    o.error(format_args!("{}", f));
    o.resultado2 = f.estado();
    o.fin(shell::RC_ERROR)
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(PLANTILLA);
    let Some(que) = args.texto("ACCION").and_then(que_es) else {
        o.error(format_args!(
            "no sé qué hacer: Paq Instalar, Quitar, Buscar, Info, Actualizar, Generaciones, Volver, Limpiar, Repos o Claves (en inglés: install, remove, search, info, upgrade, generations, rollback, clean, repos, keys)"
        ));
        return o.fin(shell::RC_ERROR);
    };
    let nombres: Vec<String> = args.varios("NOMBRES").to_vec();
    let (home, todo) = (args.si("HOME"), args.si("TODO"));
    let (nivel, para) = donde(home);
    match que {
        Que::Repos => repos(o),
        Que::Claves => claves(o),
        Que::Buscar => buscar(o, &nombres.join(" ")),
        Que::Info => info(o, &nombres),
        Que::Generaciones => generaciones(o, &nivel, para),
        Que::Instalar | Que::Quitar | Que::Actualizar | Que::Volver | Que::Limpiar => {
            if matches!(que, Que::Instalar | Que::Quitar) && nombres.is_empty() {
                o.error(format_args!("¿qué paquete? Paq {} <nombre>", if que == Que::Instalar { "Instalar" } else { "Quitar" }));
                return o.fin(shell::RC_ERROR);
            }
            if home { en_home(o, que, &nombres, todo) } else { para_todos(o, que, &nombres, todo) }
        }
    }
}

/// El plan de `que` sobre lo que hay: la propuesta y la frase de la generación.
fn planear(o: &mut Orden, que: Que, e: &Estado, nivel: &Nivel, nombres: &[String]) -> Result<(motor::Propuesta, String), Fallo> {
    match que {
        Que::Instalar => {
            let rs = repos_avisando(o, nivel).ok_or(Fallo::AunNo("leer los repositorios".into()))?;
            let p = motor::plan_instalar(e, &rs, &maquina(), nivel, nombres)?;
            Ok((p, format!("Instalar {}", nombres.join(", "))))
        }
        Que::Actualizar => {
            let rs = repos_avisando(o, nivel).ok_or(Fallo::AunNo("leer los repositorios".into()))?;
            Ok((motor::plan_actualizar(e, &rs, &maquina(), nivel)?, "Actualizar".into()))
        }
        Que::Quitar => Ok((motor::plan_quitar(e, nivel, nombres)?, format!("Quitar {}", nombres.join(", ")))),
        Que::Volver => {
            let n = match nombres.first() {
                None => None,
                Some(x) => Some(x.parse::<u32>().map_err(|_| Fallo::Plan(alloc::vec![format!("{x} no es el número de una generación")]))?),
            };
            let (p, a) = motor::plan_volver(e, nivel, n)?;
            Ok((p, format!("Volver a la generación {a}")))
        }
        _ => unreachable!(),
    }
}

fn plan_en_pantalla(o: &mut Orden, para: &str, p: &motor::Propuesta) {
    avisos(o, &p.avisos);
    di!(o, "Plan {}:\n", para);
    for (i, c) in p.cambios.iter().enumerate() {
        di!(o, "  {}. {}\n", i + 1, c.frase());
    }
}

/// Lo del usuario: lo hace la orden, con su cerrojo.
fn en_home(o: &mut Orden, que: Que, nombres: &[String], todo: bool) -> u64 {
    let (nivel, para) = donde(true);
    let quien = texto_cerrojo("Paq");
    if let Err(Fallo::Ocupado(_)) = motor::cerrar(&mut o.s, &nivel, &quien) {
        match motor::quien_cierra(&mut o.s, &nivel) {
            Some(t) if cerrojo_viejo(&t) => {
                di!(o, "Aviso: un Paq anterior no acabó (su cerrojo es viejo): se sigue.\n");
                motor::abrir(&mut o.s, &nivel);
                if let Err(f) = motor::cerrar(&mut o.s, &nivel, &quien) {
                    return fallar(o, &f);
                }
            }
            _ => return fallar(o, &Fallo::Ocupado(nivel.raiz.clone())),
        }
    }
    let r = (|| -> Result<String, Fallo> {
        let (mut e, arreglo) = motor::poner_en_orden(&mut o.s, &nivel)?;
        avisos(o, &e.avisos);
        if arreglo.borrados + arreglo.movidos > 0 {
            di!(o, "Aviso: una transacción anterior se cortó: se termina la generación {} ({} cajones movidos, {} restos borrados).\n", e.generacion.numero, arreglo.movidos, arreglo.borrados);
        }
        if que == Que::Limpiar {
            let l = motor::limpiar(&mut o.s, &nivel, &mut e, if todo { Conservar::Actual } else { Conservar::Anterior })?;
            return Ok(format!("Limpio: {} generaciones y {} versiones guardadas fuera ({} KB).", l.generaciones, l.versiones, l.bytes / 1024));
        }
        let (p, _) = planear(o, que, &e, &nivel, nombres)?;
        if p.cambios.is_empty() {
            return Ok("No hay nada que cambiar.".into());
        }
        // Lo que cambió, como en lo de todos: una frase por cambio.
        let frase = p.cambios.iter().map(Cambio::frase).collect::<Vec<_>>().join("; ");
        plan_en_pantalla(o, para, &p);
        let vistas = motor::leer_series(&mut o.s, &nivel);
        let rs = motor::cargar_repos(&mut o.s, &vistas)?;
        let n = motor::aplicar(&mut o.s, &nivel, &mut e, &rs, &maquina(), &p.cambios, &p.pedidos, &frase)?;
        motor::guardar_series(&mut o.s, &nivel, &vistas, &rs.series)?;
        motor::limpiar(&mut o.s, &nivel, &mut e, Conservar::Solo)?;
        Ok(format!("Hecho: generación {n} de Home:Paquetes."))
    })();
    motor::abrir(&mut o.s, &nivel);
    match r {
        Ok(t) => {
            di!(o, "{}\n", t);
            o.fin(shell::RC_BIEN)
        }
        Err(f) => fallar(o, &f),
    }
}

/// Lo de todos: el plan aquí, y lo hace el servicio tras la contraseña de un administrador.
fn para_todos(o: &mut Orden, que: Que, nombres: &[String], todo: bool) -> u64 {
    let (nivel, para) = donde(false);
    let lineas: Vec<String> = if que == Que::Limpiar {
        alloc::vec![format!("limpiar {}", if todo { "todo" } else { "anterior" })]
    } else {
        let e = match motor::cargar(&mut o.s, &nivel) {
            Ok(e) => e,
            Err(f) => return fallar(o, &f),
        };
        avisos(o, &e.avisos);
        let (p, _) = match planear(o, que, &e, &nivel, nombres) {
            Ok(x) => x,
            Err(f) => return fallar(o, &f),
        };
        if p.cambios.is_empty() {
            avisos(o, &[]);
            di!(o, "No hay nada que cambiar.\n");
            return o.fin(shell::RC_BIEN);
        }
        plan_en_pantalla(o, para, &p);
        p.cambios.iter().map(Cambio::linea).collect()
    };
    if let Some(l) = lineas.iter().find(|l| l.len() > cambio::MAX_LINEA) {
        return fallar(o, &Fallo::AunNo(format!("pedir un cambio tan largo ({} letras)", l.len())));
    }
    di!(o, "Se pide en la ventana del sistema la contraseña de un administrador...\n");
    o.vaciar();
    let texto = lineas.join("\n");
    let Some(r) = crate::base::administrar::pedir_tanda(texto.as_bytes()) else {
        o.error(format_args!("sesiones no contesta"));
        return o.fin(shell::RC_ERROR);
    };
    if r.estado == 0 && r.hechas == r.total && r.total > 0 {
        let n = r.de_cada[0].1;
        if que == Que::Limpiar {
            di!(o, "Hecho: limpio.\n");
        } else {
            di!(o, "Hecho: generación {} de Paquetes:.\n", n);
        }
        return o.fin(shell::RC_BIEN);
    }
    let e = r.estado;
    let por = fallo::texto_de_estado(e).unwrap_or(match e {
        dashos::protocolo::sesiones::PROTEGIDO => "no se confirmó con la contraseña de un administrador",
        dashos::protocolo::sesiones::OCUPADO => "ya hay otra tanda esperando",
        dashos::protocolo::sesiones::NO_EXISTE => "sesiones no conoce esa acción",
        _ => "no se pudo",
    });
    o.error(format_args!("no se hace nada: {} (el detalle, en Registro)", por));
    o.resultado2 = e;
    o.fin(shell::RC_ERROR)
}

fn generaciones(o: &mut Orden, nivel: &Nivel, para: &str) -> u64 {
    let e = match motor::cargar(&mut o.s, nivel) {
        Ok(e) => e,
        Err(f) => return fallar(o, &f),
    };
    avisos(o, &e.avisos);
    if e.generaciones.is_empty() {
        di!(o, "Aún no hay ninguna generación {}.\n", para);
        return o.fin(shell::RC_BIEN);
    }
    di!(o, "Generaciones {}:\n", para);
    for g in &e.generaciones {
        let cuando = g.fecha.map_or_else(|| "sin fecha".to_string(), fecha_corta);
        let marca = if g.numero == e.generacion.numero { " (en uso)" } else { "" };
        di!(o, "  {:>3}  {}  {}: {} paquetes{}\n", g.numero, cuando, g.cambio, g.paquetes.len(), marca);
    }
    o.fin(shell::RC_BIEN)
}

fn repos(o: &mut Orden) -> u64 {
    let Some(rs) = repos_avisando(o, &Nivel::de_todos()) else { return o.fin(shell::RC_ERROR) };
    if rs.repos.is_empty() {
        di!(o, "No hay ningún repositorio en {}.\n", config::REPOSITORIOS);
    }
    for r in &rs.repos {
        let estado = match rs.fuentes.iter().find(|f| f.repo.nombre == r.nombre) {
            Some(f) => format!("índice bien firmado, serie {}, {} paquetes", f.indice.serie, f.indice.entradas.len()),
            None => "su índice no se usa".to_string(),
        };
        di!(o, "{}  {}  prioridad {}{}: {}\n", r.nombre, r.url, r.prioridad, if r.protegido { ", protegido" } else { "" }, estado);
    }
    di!(o, "(Añadir o quitar un repositorio es acción de administrador: la lista es {}.)\n", config::REPOSITORIOS);
    o.fin(shell::RC_BIEN)
}

fn claves(o: &mut Orden) -> u64 {
    let Some(rs) = repos_avisando(o, &Nivel::de_todos()) else { return o.fin(shell::RC_ERROR) };
    if rs.claves.is_empty() {
        di!(o, "No hay ninguna clave de confianza en {}.\n", config::CLAVES);
    }
    for k in &rs.claves {
        di!(o, "{}  {} para {} ({}): {}\n", k.id, k.uso, k.repo, k.fichero, k.nota);
    }
    o.fin(shell::RC_BIEN)
}

fn instalado_en(o: &mut Orden, nombre: &str) -> Vec<(&'static str, motor::Guardada, Motivo)> {
    let mut v = Vec::new();
    for (nivel, donde) in [(Nivel::de_todos(), "para todos"), (Nivel::del_usuario(), "para ti")] {
        let Ok(e) = motor::cargar(&mut o.s, &nivel) else { continue };
        if let Some(i) = e.instalado(nombre)
            && let Some(g) = e.guardada(nombre, &i.version)
        {
            v.push((donde, g.clone(), i.motivo));
        }
    }
    v
}

fn buscar(o: &mut Orden, texto: &str) -> u64 {
    let Some(rs) = repos_avisando(o, &Nivel::de_todos()) else { return o.fin(shell::RC_ERROR) };
    let t = texto.to_lowercase();
    let mut n = 0;
    let hallados: Vec<(String, String, String, String)> = rs
        .entradas()
        .filter(|(_, x)| t.is_empty() || x.nombre.contains(&t) || x.corto.to_lowercase().contains(&t))
        .map(|(f, x)| (x.nombre.clone(), bonita(&x.version), f.repo.nombre.clone(), x.corto.clone()))
        .collect();
    for (nombre, version, repo, corto) in hallados {
        let donde: Vec<&str> = instalado_en(o, &nombre).iter().filter(|x| bonita(&x.1.ficha.version) == version).map(|x| x.0).collect();
        let marca = if donde.is_empty() { String::new() } else { format!(" [instalado {}]", donde.join(" y ")) };
        di!(o, "{} {} ({}){}: {}\n", nombre, version, repo, marca, corto);
        n += 1;
    }
    if n == 0 {
        di!(o, "Nada con \"{}\".\n", texto);
    }
    o.fin(shell::RC_BIEN)
}

fn info(o: &mut Orden, nombres: &[String]) -> u64 {
    let Some(rs) = repos_avisando(o, &Nivel::de_todos()) else { return o.fin(shell::RC_ERROR) };
    for nombre in nombres {
        let en_repos: Vec<_> = rs.entradas().filter(|(_, x)| x.nombre == *nombre).map(|(f, x)| (f.repo.nombre.clone(), x.clone())).collect();
        let instalados = instalado_en(o, nombre);
        if en_repos.is_empty() && instalados.is_empty() {
            o.error(format_args!("no hay ningún paquete {nombre}"));
            continue;
        }
        di!(o, "{}\n", nombre);
        for (repo, x) in &en_repos {
            let tam = if x.tamano < 1024 { format!("{} bytes", x.tamano) } else { format!("{} KB", x.tamano / 1024) };
            di!(o, "  En {}: {} ({}): {}\n", repo, bonita(&x.version), tam, x.corto);
        }
        for (donde, g, motivo) in &instalados {
            let m = &g.manifiesto;
            di!(o, "  Instalado {}: {} (de {}{}), en el cajón {}\n", donde, bonita(&g.ficha.version), g.ficha.repo, if *motivo == Motivo::Dependencia { "; lo necesita otro" } else { "" }, g.ficha.cajon);
            di!(o, "  Permisos que pide: {}\n", if m.permisos.is_empty() { "ninguno".to_string() } else { m.permisos.join(", ") });
            if !m.assigns.is_empty() {
                let a: Vec<String> = m.assigns.iter().map(|(n, c)| format!("{n} {c}")).collect();
                di!(o, "  Asignaciones: {}\n", a.join(", "));
            }
            di!(o, "  Ficheros:\n");
            for f in &g.ficha.ficheros {
                di!(o, "    {} ({} bytes)\n", f.ruta, f.tamano);
            }
        }
    }
    o.fin(shell::RC_BIEN)
}

// ---------------------------------------------------------------------------------------------------------------
// El servicio.

/// Lo que le manda `sesiones` (sin marca, por la copia del canal que le da `inicio`): aplicar las líneas confirmadas
/// de una tanda. Cabecera de 32 bytes: orden (u32), lo que mide el texto (u32), el usuario que la pidió (u32) y el
/// nombre del programa (16); llaves: por dónde contestar y la memoria con el texto (las líneas, solo para leer).
pub const APLICAR: u32 = 1;
/// Lo más que mide el texto de una tanda (`dashos::tanda::MAX_TEXTO`).
const MAX_TEXTO: usize = 4096;
const VA_TEXTO: usize = 0x4A_0000_0000;

/// La respuesta: estado (i32: 0 o el de `Fallo`), cuántas líneas (u32), la generación nueva (u64) y, detrás, el
/// porqué en llano si no se hizo (hasta 200 bytes).
fn contestar(llave: u32, estado: i32, lineas: u32, generacion: u64, por: &str) {
    let mut r = [0u8; 224];
    r[0..4].copy_from_slice(&estado.to_le_bytes());
    r[4..8].copy_from_slice(&lineas.to_le_bytes());
    r[8..16].copy_from_slice(&generacion.to_le_bytes());
    let mut n = por.len().min(200);
    while !por.is_char_boundary(n) {
        n -= 1;
    }
    r[24..24 + n].copy_from_slice(&por.as_bytes()[..n]);
    dashos::canal_escribir(llave, &r[..24 + n], &[]);
    dashos::cerrar(llave);
}

/// Una tanda confirmada: sus líneas, en el nivel de todos.
fn hacer_tanda(s: &mut Sistema, texto: &str, quien: &str) -> Result<u32, Fallo> {
    let nivel = Nivel::de_todos();
    let primera = texto.lines().next().unwrap_or("");
    let limpiar = match primera {
        "limpiar todo" => Some(Conservar::Actual),
        "limpiar anterior" => Some(Conservar::Anterior),
        _ => None,
    };
    let cambios = match limpiar {
        Some(_) if texto.lines().filter(|l| !l.trim().is_empty()).count() == 1 => Vec::new(),
        Some(_) => return Err(Fallo::Plan(alloc::vec!["limpiar va solo en su tanda".into()])),
        None => cambio::leer_lineas(texto).ok_or_else(|| Fallo::Plan(alloc::vec!["una línea de la tanda no se entiende".into()]))?,
    };
    motor::cerrar(s, &nivel, &texto_cerrojo(quien))?;
    let r = (|| -> Result<u32, Fallo> {
        let (mut e, _) = motor::poner_en_orden(s, &nivel)?;
        if let Some(c) = limpiar {
            let l = motor::limpiar(s, &nivel, &mut e, c)?;
            dashos::println!("  [paq] limpio: {} generaciones y {} versiones fuera ({} KB)", l.generaciones, l.versiones, l.bytes / 1024);
            return Ok(e.generacion.numero);
        }
        let vistas = motor::leer_series(s, &nivel);
        let rs = motor::cargar_repos(s, &vistas)?;
        for (r, por) in &rs.malos {
            dashos::println!("  [paq] el repositorio {} no se usa: {}", r, por);
        }
        let pedidos: Vec<String> = cambios
            .iter()
            .filter(|c| matches!(c, Cambio::Volver { .. } | Cambio::Instalar { motivo: Motivo::Pedido, .. }))
            .map(|c| c.nombre().to_string())
            .collect();
        let frase = cambios.iter().map(Cambio::frase).collect::<Vec<_>>().join("; ");
        let n = motor::aplicar(s, &nivel, &mut e, &rs, &maquina(), &cambios, &pedidos, &frase)?;
        motor::guardar_series(s, &nivel, &vistas, &rs.series)?;
        motor::limpiar(s, &nivel, &mut e, Conservar::Solo)?;
        Ok(n)
    })();
    motor::abrir(s, &nivel);
    r
}

/// El servicio `paq`: llave 1, el servicio de archivos (raíz 0, `Paquetes`, para leer y escribir; raíz 1, el disco,
/// para leer: `SYS:`); llave 2, su canal, que solo tiene `sesiones`.
pub fn servicio(archivos: u32, canal: u32) -> u64 {
    let Some(fs) = Archivos::abrir_por(archivos) else {
        // Sin disco (o sin `Paquetes` en él) no hay nada que hacer; pero no se acaba, para que `inicio` no lo relance
        // una y otra vez: se queda contestando que no a lo que llegue.
        dashos::println!("  [paq] sin el servicio de archivos (o sin Paquetes en el disco): no instala nada");
        let mut m = [0u8; 256];
        let mut ll = [0u32; 4];
        loop {
            let (n, k, _) = dashos::canal_leer_marca(canal, &mut m, &mut ll);
            if n < 0 {
                dashos::dormir_us(1_000_000);
                continue;
            }
            for (i, &l) in ll[..k].iter().enumerate() {
                if i == 0 {
                    contestar(l, Fallo::Disco(a::NO_EXISTE, String::new()).estado(), 0, 0, "no hay disco para los paquetes");
                } else {
                    dashos::cerrar(l);
                }
            }
        }
    };
    let tabla = Tabla::desde(b"*Paquetes=@0\n*SYS=@1\n").unwrap_or_default();
    let mut s = Sistema::con(fs, tabla, "Paquetes:");
    let nivel = Nivel::de_todos();
    // Al arrancar: nadie más escribe en `Paquetes:`, así que un cerrojo que haya es de antes de reiniciar.
    if let Some(q) = motor::quien_cierra(&mut s, &nivel) {
        dashos::println!("  [paq] el cerrojo de antes de arrancar ({}) se quita", q);
        motor::abrir(&mut s, &nivel);
    }
    match motor::poner_en_orden(&mut s, &nivel) {
        Ok((e, ar)) => {
            for x in &e.avisos {
                dashos::println!("  [paq] aviso: {}", x);
            }
            dashos::println!(
                "  [paq] Paquetes: en orden: generación {}, {} paquetes; de una transacción cortada, {} restos borrados y {} cajones movidos",
                e.generacion.numero,
                e.generacion.paquetes.len(),
                ar.borrados,
                ar.movidos
            );
        }
        Err(f) => dashos::println!("  [paq] Paquetes: no se puede poner en orden: {}", f),
    }
    let mut m = [0u8; 256];
    let mut ll = [0u32; 4];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(canal, &mut m, &mut ll);
        if n < 0 {
            dashos::dormir_us(10_000);
            continue;
        }
        let (n, k) = (n as usize, k);
        // Solo `sesiones`, sin marca, con su cabecera y sus dos llaves.
        let de_sesiones = marca == 0 && n >= 32 && k == 2 && u32::from_le_bytes(m[0..4].try_into().unwrap()) == APLICAR;
        if !de_sesiones {
            dashos::println!("  [paq] un mensaje que no es de sesiones (marca {}): no se atiende", marca);
            for &l in &ll[..k] {
                dashos::cerrar(l);
            }
            continue;
        }
        let largo = u32::from_le_bytes(m[4..8].try_into().unwrap()) as usize;
        let uid = u32::from_le_bytes(m[8..12].try_into().unwrap());
        let programa = String::from_utf8_lossy(&m[12..28]).trim_end_matches('\0').to_string();
        let (respuesta, memoria) = (ll[0], ll[1]);
        let mut texto = alloc::vec![0u8; largo.min(MAX_TEXTO)];
        let leido = largo <= MAX_TEXTO && dashos::memoria_proyectar(memoria, VA_TEXTO, false) == 0 && {
            // SAFETY: recién proyectada aquí, para leer, con al menos `largo` bytes (lo comprobó `sesiones` al
            // copiarla); se quita en seguida.
            let n = texto.len();
            texto.copy_from_slice(unsafe { core::slice::from_raw_parts(VA_TEXTO as *const u8, n) });
            dashos::memoria_quitar(VA_TEXTO);
            true
        };
        dashos::cerrar(memoria);
        let lineas = String::from_utf8_lossy(&texto).lines().filter(|l| !l.trim().is_empty()).count() as u32;
        let r = match (leido, core::str::from_utf8(&texto)) {
            (true, Ok(t)) => hacer_tanda(&mut s, t, &format!("tanda de {programa}")),
            _ => Err(Fallo::Plan(alloc::vec!["la tanda no se pudo leer".into()])),
        };
        match r {
            Ok(g) => {
                dashos::println!("  [paq] tanda de {} (usuario {}): hecha, generación {}", programa, uid, g);
                contestar(respuesta, 0, lineas, g as u64, "");
            }
            Err(f) => {
                let por = format!("{}", f);
                dashos::println!("  [paq] tanda de {} (usuario {}): no se hace: {}", programa, uid, por);
                contestar(respuesta, f.estado(), lineas, 0, &por);
            }
        }
    }
}
