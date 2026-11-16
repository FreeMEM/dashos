//! `Permiso`: lo que has dejado hacer a cada programa (S6b, regla 3 de S36). Ver lo propio es libre; dar y quitar lo
//! confirma el usuario en la ventana del sistema (`guardian`), con el teclado: un guion no se concede nada solo.
//!
//! `Permiso` lista lo concedido a tus programas, con la frase de cada permiso y su nombre interno entre paréntesis.
//! `Permiso DAR programa permiso` y `Permiso QUITAR programa permiso` piden el cambio; quitar revoca en el acto lo que
//! el programa tuviera abierto. Lo concedido está en texto en `Home:Prefs/Permisos/concedidos`: se lee con `Type`, pero
//! solo lo escribe el sistema.

use crate::{di, Orden};
use alloc::vec::Vec;
use dashos::permisos_texto as pt;
use dashos::protocolo::sesiones as s;

/// Lo que se espera a `sesiones`; al dar o quitar, lo que tarda el usuario en contestar (la ventana espera 30 s).
const PLAZO_US: u64 = 10_000_000;
const PLAZO_PREGUNTA_US: u64 = 70_000_000;

fn pedir(m: &[u8], plazo: u64) -> Option<Vec<u8>> {
    let (lectura, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(crate::SESIONES, m, &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    let mut r = [0u8; 256];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], plazo);
    dashos::cerrar(lectura);
    (n >= 4).then(|| r[..n as usize].to_vec())
}

fn campo(b: &[u8]) -> &str {
    core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).unwrap_or("?")
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::PERMISO);
    let (dar, quitar) = (args.si("DAR"), args.si("QUITAR"));
    if !dar && !quitar {
        return listar(o);
    }
    if dar && quitar {
        o.error(format_args!("DAR o QUITAR, no los dos"));
        return o.fin(shell::RC_ERROR);
    }
    let (Some(programa), Some(permiso)) = (args.texto("PROGRAMA"), args.texto("PERMISO")) else {
        o.error(format_args!("falta el programa y el permiso: Permiso {} programa permiso", if dar { "DAR" } else { "QUITAR" }));
        return o.fin(shell::RC_ERROR);
    };
    let (p, q) = (programa.as_bytes(), permiso.as_bytes());
    if !pt::nombre_valido(p) || !pt::nombre_valido(q) {
        o.error(format_args!("\"{}\" o \"{}\" no es un nombre que valga", programa, permiso));
        return o.fin(shell::RC_ERROR);
    }
    let mut m = [0u8; 16 + 2 * pt::MAX_NOMBRE];
    m[0..4].copy_from_slice(&s::PERMISO_CAMBIAR.to_le_bytes());
    m[4..8].copy_from_slice(&(dar as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(p.len() as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(q.len() as u32).to_le_bytes());
    m[16..16 + p.len()].copy_from_slice(p);
    m[16 + p.len()..16 + p.len() + q.len()].copy_from_slice(q);
    di!(o, "Se pregunta en la ventana del sistema: {} a {} el permiso de {}...\n", if dar { "dar" } else { "quitar" }, programa, pt::frase(permiso));
    o.vaciar();
    let estado = pedir(&m[..16 + p.len() + q.len()], PLAZO_PREGUNTA_US).map(|r| i32::from_le_bytes(r[0..4].try_into().unwrap()));
    match estado {
        Some(s::BIEN) => {
            di!(o, "Hecho: {} {} {} ({}).\n", programa, if dar { "puede" } else { "ya no puede" }, pt::frase(permiso), permiso);
            o.fin(shell::RC_BIEN)
        }
        Some(s::NO_EXISTE) if quitar => {
            o.error(format_args!("{} no tiene el permiso \"{}\" (o ese permiso no existe)", programa, permiso));
            o.fin(shell::RC_AVISO)
        }
        Some(s::NO_EXISTE) => {
            o.error(format_args!("no hay un permiso \"{}\"", permiso));
            o.fin(shell::RC_ERROR)
        }
        Some(s::PROTEGIDO) => {
            o.error(format_args!("no se cambia: el usuario dijo que no, no contestó, o su sesión no está delante"));
            o.fin(shell::RC_ERROR)
        }
        r => {
            o.error(format_args!("no se cambia ({:?})", r));
            o.fin(shell::RC_ERROR)
        }
    }
}

/// Lo concedido a los programas del usuario, página a página.
fn listar(o: &mut Orden) -> u64 {
    let mut desde = 0u32;
    let mut todos: Vec<([u8; 16], [u8; 16])> = Vec::new();
    loop {
        let mut m = [0u8; 8];
        m[0..4].copy_from_slice(&s::PERMISOS_PROPIOS.to_le_bytes());
        m[4..8].copy_from_slice(&desde.to_le_bytes());
        let Some(r) = pedir(&m, PLAZO_US) else {
            o.error(format_args!("sesiones no contesta"));
            return o.fin(shell::RC_ERROR);
        };
        if r.len() < 16 || r[0..4] != [0; 4] {
            o.error(format_args!("sesiones no lo da ({})", i32::from_le_bytes(r[0..4].try_into().unwrap())));
            return o.fin(shell::RC_ERROR);
        }
        let total = u32::from_le_bytes(r[4..8].try_into().unwrap());
        let van = (u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize).min(s::POR_PAGINA_CONCEDIDOS);
        for i in 0..van {
            let Some(e) = r.get(16 + i * s::CONCEDIDO..16 + (i + 1) * s::CONCEDIDO) else { break };
            todos.push((e[..16].try_into().unwrap(), e[16..32].try_into().unwrap()));
        }
        desde += van as u32;
        if van == 0 || desde >= total || todos.len() > 1024 {
            break;
        }
    }
    if todos.is_empty() {
        di!(o, "Ningún programa tuyo tiene permisos concedidos.\n");
    }
    todos.sort();
    for (p, q) in &todos {
        let (p, q) = (campo(p), campo(q));
        di!(o, "{}: {} ({})\n", p, pt::frase(q), q);
    }
    o.fin(shell::RC_BIEN)
}
