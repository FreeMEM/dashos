//! FKey (S10, `SYS:Tools/Commodities/FKey`; `docs/bitacoras/commodities.md`): teclas que lanzan programas. No ve el
//! teclado: pide al compositor sus combinaciones (permiso `atajos`) y, cuando llega una, lanza su programa como ayudante
//! con su misma ficha (lo que puede leer y los permisos que tiene, ni uno más), como el Shell.
//!
//! Lo que hace cada tecla está en `Home:Prefs/FKey`, en texto, una línea por tecla: la combinación, espacios, el
//! programa y sus argumentos (`Ctrl+Alt+T SYS:System/Shell`). `#` empieza un comentario. Sin el fichero, Ctrl+Alt+S abre
//! el Shell. Una combinación que el compositor no da (reservada, ocupada por otra) se dice y se sigue con las demás.
//!
//! Con argumentos (desde el Shell), `FKey LISTA` dice qué teclas leería, sin ponerlas.
//!
//! Llaves: 1 `sesiones`, 2 archivos, 3 el compositor (4, el canal de arranque, si lo lanza el Shell).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../../usuario/src/ventana_cliente.rs"]
#[allow(dead_code)]
mod ventana_cliente;
#[path = "../../../usuario/src/commodity_cliente.rs"]
#[allow(dead_code)]
mod commodity_cliente;

use alloc::string::String;
use alloc::vec::Vec;
use commodity_cliente::{Cx, Evento};
use dashos::protocolo::sesiones as ps;
use ordenes::{di, Orden, SESIONES};
use shell::protocolo as p;
use wayland::atajos;

dashos::programa!(principal);
dashos::monton!();

const COMPOSITOR: u32 = 3;
const PREFS: &str = "Home:Prefs/FKey";
const DE_SERIE: &str = "Ctrl+Alt+S SYS:System/Shell\n";
/// Lo más que mide un programa que se lanza (lo que admite `sesiones`).
const MAX_PROGRAMA: usize = 4 << 20;
const VA_PROGRAMA: usize = 0x50_0000_0000;
const MAX_TECLAS: usize = 32;

/// Una tecla y lo que lanza.
struct Tecla {
    combinacion: String,
    programa: String,
    args: String,
}

/// Lee el fichero de teclas: las que valen y las líneas que no se entienden (número de línea y por qué).
fn leer(texto: &str) -> (Vec<Tecla>, Vec<(usize, &'static str)>) {
    let (mut teclas, mut malas) = (Vec::new(), Vec::new());
    for (i, linea) in texto.lines().enumerate() {
        let linea = linea.split('#').next().unwrap_or("").trim();
        if linea.is_empty() {
            continue;
        }
        let mut partes = linea.splitn(2, char::is_whitespace);
        let (comb, resto) = (partes.next().unwrap_or(""), partes.next().unwrap_or("").trim());
        let mut r = resto.splitn(2, char::is_whitespace);
        let (programa, args) = (r.next().unwrap_or(""), r.next().unwrap_or("").trim());
        if programa.is_empty() {
            malas.push((i + 1, "falta el programa"));
        } else if atajos::Combinacion::leer(comb).is_err() {
            malas.push((i + 1, "la combinación no se entiende"));
        } else if teclas.len() >= MAX_TECLAS {
            malas.push((i + 1, "demasiadas teclas"));
        } else {
            teclas.push(Tecla { combinacion: comb.into(), programa: programa.into(), args: args.into() });
        }
    }
    (teclas, malas)
}

/// Lanza `camino` con `args` como ayudante con la ficha de FKey (como el Shell, `LANZAR_AYUDANTE`): su número.
fn lanzar(o: &mut Orden, camino: &str, args: &str) -> Result<u64, i32> {
    use dashos::protocolo::archivos as a;
    let datos = o.s.leer_entero(camino, MAX_PROGRAMA)?;
    if datos.len() < 64 || &datos[..4] != b"\x7fELF" {
        return Err(a::MAL_FORMADO);
    }
    let mem = dashos::memoria_crear(datos.len());
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_PROGRAMA, true) != 0 {
        if (mem as i32) > 0 {
            dashos::cerrar(mem);
        }
        return Err(a::SIN_SITIO);
    }
    // SAFETY: recién proyectada aquí con `datos.len()` bytes; se quita en la línea siguiente. Lo mismo que hace el
    // Shell al cargar un programa (`ordenes/src/bin/shell.rs`, `cargar`).
    unsafe { core::slice::from_raw_parts_mut(VA_PROGRAMA as *mut u8, datos.len()) }.copy_from_slice(&datos);
    dashos::memoria_quitar(VA_PROGRAMA);
    let corto = camino.rsplit(['/', ':']).next().unwrap_or(camino);
    // El canal de arranque: el cajón actual, el nombre y los argumentos, sin Shell que lo atienda.
    let (nuestro, suyo) = dashos::canal_crear();
    let mut m = [0u8; 256];
    let marcas = p::ENTRADA_NO_INTERACTIVA | p::SALIDA_NO_CONSOLA;
    m[0..4].copy_from_slice(&p::ARRANQUE.to_le_bytes());
    m[4..8].copy_from_slice(&marcas.to_le_bytes());
    m[8..12].copy_from_slice(&(o.s.actual.len() as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(args.len() as u32).to_le_bytes());
    m[20..24].copy_from_slice(&(corto.len() as u32).to_le_bytes());
    let mut bien = o.s.actual.len() + corto.len() + args.len() <= p::MAX_ARRANQUE && dashos::canal_escribir(nuestro, &m[..p::LARGO_ARRANQUE], &[]) == 0;
    let mut todo = Vec::from(o.s.actual.as_bytes());
    todo.extend_from_slice(corto.as_bytes());
    todo.extend_from_slice(args.as_bytes());
    for trozo in todo.chunks(p::POR_DATOS) {
        m[0..4].copy_from_slice(&p::DATOS.to_le_bytes());
        m[4..4 + trozo.len()].copy_from_slice(trozo);
        bien = bien && dashos::canal_escribir(nuestro, &m[..4 + trozo.len()], &[]) == 0;
    }
    if !bien || corto.len() > 64 {
        for l in [mem, nuestro, suyo] {
            dashos::cerrar(l);
        }
        return Err(a::SIN_SITIO);
    }
    let mut pet = Vec::with_capacity(12 + corto.len());
    pet.extend_from_slice(&ps::LANZAR_AYUDANTE.to_le_bytes());
    pet.extend_from_slice(&(datos.len() as u32).to_le_bytes());
    pet.push(ps::SIN_RAIZ);
    pet.push(corto.len() as u8);
    pet.extend_from_slice(&0u16.to_le_bytes());
    pet.extend_from_slice(corto.as_bytes());
    let (lectura, respuesta) = dashos::canal_crear();
    let mut r = [0u8; 12];
    let n = if dashos::canal_escribir(SESIONES, &pet, &[respuesta, mem, suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 20_000_000).0
    } else {
        for l in [respuesta, mem, suyo] {
            dashos::cerrar(l);
        }
        -1
    };
    dashos::cerrar(lectura);
    dashos::cerrar(nuestro);
    if n != 12 {
        return Err(a::ERROR_DEL_DISCO);
    }
    match i32::from_le_bytes(r[0..4].try_into().unwrap()) {
        0 => Ok(u64::from_le_bytes(r[4..12].try_into().unwrap())),
        e => Err(e),
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"fkey");
    let mut o = Orden::arrancar("FKey");
    let texto = match o.s.leer_entero(PREFS, 16 << 10) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(_) => String::from(DE_SERIE),
    };
    let (teclas, malas) = leer(&texto);
    for (n, porque) in &malas {
        o.error(format_args!("{}, línea {}: {}", PREFS, n, porque));
    }
    if o.args.trim().eq_ignore_ascii_case("LISTA") {
        for t in &teclas {
            di!(o, "{}  {}{}{}\n", t.combinacion, t.programa, if t.args.is_empty() { "" } else { " " }, t.args);
        }
        return o.fin(shell::RC_BIEN);
    }
    let e = commodity_cliente::pedir_permiso(SESIONES, "atajos");
    if e != 0 {
        o.error(format_args!("sin el permiso de usar teclas rápidas en todo el sistema ({})", e));
        return o.fin(shell::RC_ERROR);
    }
    ventana_cliente::usar_llave(COMPOSITOR);
    let mut cx = match Cx::conectar() {
        Ok(c) => c,
        Err(e) => {
            o.error(format_args!("{}", e));
            return o.fin(shell::RC_FALLO);
        }
    };
    let Some(yo) = cx.registrar("FKey", "FKey", "Teclas que lanzan programas") else { return o.fin(shell::RC_FALLO) };
    for (i, t) in teclas.iter().enumerate() {
        cx.atajo(yo, i as u32, &t.combinacion);
    }
    for ev in cx.hasta_hecho(5_000_000) {
        if let Evento::Negado { numero, motivo, quien, .. } = ev
            && let Some(t) = teclas.get(numero as usize)
        {
            let porque = atajos::Motivo::de_numero(motivo).map_or("no", |m| m.texto());
            o.error(format_args!("{}: {}{}{}", t.combinacion, porque, if quien.is_empty() { "" } else { ": " }, quien));
        }
    }
    dashos::println!("  [fkey] en marcha con {} teclas", teclas.len());
    // Se queda en marcha: lo que escriba ya no tiene a quién ir si el Shell se cierra.
    o.vaciar();
    loop {
        match cx.evento(60_000_000) {
            Ok(Some(Evento::Pulsado { numero, .. })) => {
                if let Some(t) = teclas.get(numero as usize) {
                    match lanzar(&mut o, &t.programa, &t.args) {
                        Ok(pid) => dashos::println!("  [fkey] {}: {} (proceso {})", t.combinacion, t.programa, pid),
                        Err(e) => dashos::println!("  [fkey] {}: no se pudo lanzar {}: {}", t.combinacion, t.programa, ordenes::texto_error(e)),
                    }
                }
            }
            Ok(Some(Evento::Quitar { .. })) | Ok(Some(Evento::Retirado { .. })) => return o.fin(shell::RC_BIEN),
            Ok(Some(Evento::Error(t))) => {
                dashos::println!("  [fkey] el compositor cortó: {}", t);
                return o.fin(shell::RC_FALLO);
            }
            Err(()) => return o.fin(shell::RC_FALLO),
            _ => {}
        }
    }
}
