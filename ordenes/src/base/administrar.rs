//! `Administrar`: una tanda de acciones de administrador (S6, S6b y S36, decisión 1 del 2026-11-16: "una tanda es una
//! acción"). La ventana del sistema (`guardian`) enseña la lista entera, en llano, y pide la contraseña de un
//! administrador una vez; se hacen solo las de la lista, en orden, y se para en la primera que falle.
//!
//! `Administrar comprobar "matar 65540" registro`, o `Administrar DESDE plan.txt` (una acción por línea). Las acciones
//! son las de la lista cerrada de S6b que existen hoy: `comprobar` (el disco), `matar <proceso>` (de otra persona o del
//! sistema) y `registro` (leer el de todos, mientras viva la orden). Lo que no es de administrador no se enseña ni se
//! hace: ni esa ni ninguna de la lista.

use crate::{di, Orden};
use alloc::string::String;
use dashos::protocolo::sesiones as s;
use dashos::tanda;

/// Dónde se pone la lista para mandarla (una memoria que se da a `sesiones`, solo para leer).
const VA_LISTA: usize = 0x46_0000_0000;
/// Lo que tarda el usuario en contestar (la ventana espera dos minutos) y lo que tardan las acciones (comprobar el
/// disco, hasta 20 s).
const PLAZO_US: u64 = 160_000_000;

/// Manda la tanda `lista` a `sesiones` y espera su respuesta. `None` si no contesta.
pub fn pedir_tanda(lista: &[u8]) -> Option<tanda::Resultado> {
    let mem = dashos::memoria_crear(lista.len().max(1));
    if (mem as i32) <= 0 {
        return None;
    }
    if dashos::memoria_proyectar(mem, VA_LISTA, true) != 0 {
        dashos::cerrar(mem);
        return None;
    }
    // SAFETY: recién proyectada aquí con al menos `lista.len()` bytes; se quita en la línea siguiente.
    unsafe { core::slice::from_raw_parts_mut(VA_LISTA as *mut u8, lista.len()) }.copy_from_slice(lista);
    dashos::memoria_quitar(VA_LISTA);
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&s::TANDA.to_le_bytes());
    m[4..8].copy_from_slice(&(lista.len() as u32).to_le_bytes());
    let r = if dashos::canal_escribir(crate::SESIONES, &m, &[suyo, mem]) == 0 {
        let mut r = [0u8; 256];
        let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_US);
        if n > 0 { tanda::Resultado::de_bytes(&r[..n as usize]) } else { None }
    } else {
        dashos::cerrar(suyo);
        dashos::cerrar(mem);
        None
    };
    dashos::cerrar(lectura);
    r
}

/// Lo que dice un estado de `sesiones` o de una acción.
fn que_paso(e: i32) -> &'static str {
    match e {
        0 => "hecho",
        s::NO_EXISTE => "no existe",
        s::PROTEGIDO => "no se confirmó con la contraseña de un administrador",
        s::OCUPADO => "ya tiene otra tanda esperando",
        tanda::NO_HECHA => "no se hizo",
        // Rama `cuentas`: lo que dice `cuentas::Fallo` (una cuenta repetida, la propia, el último administrador...).
        e => match dashos::cuentas::Fallo::de_estado(e) {
            Some(f) => f.texto(),
            None => crate::texto_error(e),
        },
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::ADMINISTRAR);
    let mut lista = String::new();
    if let Some(desde) = args.texto("DESDE") {
        match o.s.leer_entero(desde, tanda::MAX_TEXTO + 1) {
            Ok(b) => lista.push_str(&String::from_utf8_lossy(&b)),
            Err(e) => {
                o.error_de(e, desde);
                return o.fin(shell::RC_ERROR);
            }
        }
    }
    for a in args.varios("ACCION") {
        if !lista.is_empty() && !lista.ends_with('\n') {
            lista.push('\n');
        }
        lista.push_str(a);
    }
    // Se lee aquí también, para decir en llano qué se pide y qué no vale sin molestar a nadie; `sesiones` la vuelve a
    // leer (esta lectura no le vale de nada a la seguridad).
    let mut acciones = [tanda::Accion::default(); tanda::MAX_ACCIONES];
    let n = match tanda::leer(lista.as_bytes(), &mut acciones) {
        Ok(n) => n,
        Err(mala) => {
            o.error(format_args!("no se pide nada: {}", mala));
            return o.fin(shell::RC_ERROR);
        }
    };
    di!(o, "Se pide en la ventana del sistema la contraseña de un administrador para {} {}...\n", n, if n == 1 { "acción" } else { "acciones" });
    o.vaciar();
    let Some(r) = pedir_tanda(lista.as_bytes()) else {
        o.error(format_args!("sesiones no contesta"));
        return o.fin(shell::RC_ERROR);
    };
    if r.total == 0 || r.hechas == 0 && r.parada == 0 {
        o.error(format_args!("no se hace ninguna: {}", que_paso(r.estado)));
        return o.fin(shell::RC_ERROR);
    }
    for (i, a) in acciones[..n].iter().enumerate() {
        let (e, dato) = r.de_cada[i];
        let frase = tanda::Frase { accion: *a, programa: "", persona: "" };
        match e {
            0 if a.nombre() == "comprobar" && dato == 0 => di!(o, "{}. {}: hecho, sin problemas\n", i + 1, frase),
            0 if a.nombre() == "comprobar" => di!(o, "{}. {}: hecho, {} problemas\n", i + 1, frase, dato),
            0 => di!(o, "{}. {}: hecho\n", i + 1, frase),
            e => di!(o, "{}. {}: {}\n", i + 1, frase, que_paso(e)),
        }
    }
    if r.estado == 0 {
        di!(o, "Hechas las {}.\n", r.hechas);
        o.fin(shell::RC_BIEN)
    } else {
        o.error(format_args!("se paró en la {}: {}; {} hechas, {} sin hacer", r.parada, que_paso(r.estado), r.hechas, r.total - r.hechas - 1));
        o.fin(shell::RC_ERROR)
    }
}
