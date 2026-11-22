//! `Usuario`: las cuentas de usuario (S6b, "Dónde se guardan las cuentas"). Verlas es libre; crear, borrar, devolver y
//! cambiar grupos es una acción de administrador: la orden arma la tanda y la pide a `sesiones`, y la ventana del
//! sistema (`guardian`) enseña la lista entera y pide la contraseña de un administrador una vez (S36, decisión 1).
//!
//! - `Usuario` o `Usuario LISTA`: las cuentas de `Estado:Cuentas/usuarios`, con su número, su nombre completo, sus
//!   grupos y si están borradas (y hasta cuándo se pueden devolver).
//! - `Usuario CREAR Berta "Berta Gil" +admin`: una cuenta nueva con su volumen cifrado (su `Home:`) y, si se pide, en
//!   unos grupos, todo en la misma tanda. Su contraseña se teclea en la ventana del sistema, y allí sale su código de
//!   recuperación para apuntar en papel (esta orden no ve ninguno de los dos). `UID n` pide un número; sin él, el
//!   siguiente libre.
//! - `Usuario BORRAR Carla`: la cuenta queda borrada (no entra), con su volumen cerrado; se puede devolver durante
//!   unos días (`cuentas::PLAZO_RETIRADA_S`), y pasado el plazo se borra del todo. Ni la propia ni la del último
//!   administrador.
//! - `Usuario DEVOLVER Carla`: la vuelve a dejar como estaba.
//! - `Usuario GRUPOS Carla`: sus grupos; `Usuario GRUPOS Carla +fotos -admin`: la pone y la quita.
//! - `Usuario CLAVE` (rama `cuentas-2`): cambiar la propia contraseña. No es acción de administrador: la ventana del
//!   sistema pide la de ahora y la nueva dos veces (esta orden no ve ninguna). Solo la propia: `Usuario CLAVE Carla`,
//!   siendo otra persona, no se pregunta. Cambia la llave de su volumen sin reescribir lo que hay dentro.

use crate::{di, Orden};
use alloc::string::String;
use alloc::vec::Vec;
use dashos::cuentas as cu;
use dashos::tanda;

/// Dónde están las cuentas, para leerlas (todos las leen; solo `sesiones` las escribe).
const USUARIOS: &str = "SYS:Estado/Cuentas/usuarios";
const GRUPOS: &str = "SYS:Estado/Cuentas/grupos";

/// Las cuentas de `Estado:Cuentas/`, como las lee `sesiones`. `None` si no se pueden leer.
fn leer(o: &mut Orden) -> Option<cu::Cuentas> {
    let u = match o.s.leer_entero(USUARIOS, cu::MAX_FICHERO + 1) {
        Ok(b) => b,
        Err(e) => {
            o.error_de(e, USUARIOS);
            return None;
        }
    };
    let g = o.s.leer_entero(GRUPOS, cu::MAX_FICHERO + 1).unwrap_or_default();
    let (c, iu, ig) = cu::Cuentas::leer(&u, &g);
    if iu.malas + ig.malas > 0 {
        di!(o, "Aviso: {} líneas de las cuentas no se entienden; no cuentan.\n", iu.malas + ig.malas);
    }
    Some(c)
}

/// Lo que dice un estado de una acción de la tanda.
fn que_paso(e: i32) -> String {
    match e {
        0 => String::from("hecho"),
        dashos::protocolo::sesiones::NO_EXISTE => String::from("no existe"),
        dashos::protocolo::sesiones::PROTEGIDO => String::from("no se confirmó con la contraseña de un administrador"),
        dashos::protocolo::sesiones::OCUPADO => String::from("ya tiene otra tanda esperando"),
        tanda::NO_HECHA => String::from("no se hizo"),
        e => match cu::Fallo::de_estado(e) {
            Some(f) => alloc::format!("{}", f),
            None => String::from(crate::texto_error(e)),
        },
    }
}

/// Rama `cuentas-2`: lo que tarda el usuario en la ventana (dos minutos) y un poco más.
const PLAZO_CLAVE_US: u64 = 150_000_000;

/// `Usuario CLAVE [nombre]`: pide a `sesiones` cambiar la contraseña de la propia cuenta; la ventana del sistema la pide.
fn clave(o: &mut Orden, nombre: Option<&str>) -> u64 {
    use dashos::protocolo::sesiones as s;
    let n = nombre.unwrap_or("").as_bytes();
    if n.len() > cu::MAX_NOMBRE {
        o.error(format_args!("ese nombre no es de ninguna cuenta"));
        return o.fin(shell::RC_ERROR);
    }
    di!(o, "Se pide en la ventana del sistema tu contraseña de ahora y la nueva...\n");
    o.vaciar();
    let mut m = [0u8; 8 + cu::MAX_NOMBRE];
    m[0..4].copy_from_slice(&s::CLAVE_PROPIA.to_le_bytes());
    m[4..8].copy_from_slice(&(n.len() as u32).to_le_bytes());
    m[8..8 + n.len()].copy_from_slice(n);
    let (lectura, suyo) = dashos::canal_crear();
    let estado = if dashos::canal_escribir(crate::SESIONES, &m[..8 + n.len()], &[suyo]) == 0 {
        let mut r = [0u8; 4];
        let (leidos, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_CLAVE_US);
        (leidos == 4).then(|| i32::from_le_bytes(r))
    } else {
        dashos::cerrar(suyo);
        None
    };
    dashos::cerrar(lectura);
    match estado {
        Some(0) => {
            di!(o, "Contraseña cambiada. Lo que hay en tu Home: sigue igual; la de antes ya no vale.\n");
            o.fin(shell::RC_BIEN)
        }
        Some(s::PROTEGIDO) if !n.is_empty() => {
            o.error(format_args!("solo se cambia la propia: la de otra cuenta, con su código de recuperación"));
            o.fin(shell::RC_ERROR)
        }
        Some(s::PROTEGIDO) => {
            o.error(format_args!("no se cambió (se dijo que no, o tu sesión no está delante)"));
            o.fin(shell::RC_ERROR)
        }
        Some(s::NO_EXISTE) => {
            o.error(format_args!("este programa no es de una persona con cuenta"));
            o.fin(shell::RC_ERROR)
        }
        Some(s::OCUPADO) => {
            o.error(format_args!("ya hay otro cambio de contraseña esperando en la ventana del sistema"));
            o.fin(shell::RC_ERROR)
        }
        Some(e) => {
            o.error(format_args!("no se cambió: {}", que_paso(e)));
            o.fin(shell::RC_ERROR)
        }
        None => {
            o.error(format_args!("sesiones no contesta"));
            o.fin(shell::RC_ERROR)
        }
    }
}

fn listar(o: &mut Orden, solo: Option<&str>) -> u64 {
    let Some(c) = leer(o) else { return o.fin(shell::RC_ERROR) };
    let mut vistas = 0;
    for x in c.todas().filter(|x| solo.is_none_or(|n| x.nombre.igual(n.as_bytes()))) {
        vistas += 1;
        let grupos: Vec<&str> = c.grupos().filter(|g| g.miembros().contains(&x.uid)).map(|g| g.nombre.as_str()).collect();
        let estado = match x.estado {
            cu::Estado::Activa => String::new(),
            cu::Estado::Retirada(desde) => {
                let (a, m, d, ..) = shell::fecha::partes(desde + cu::PLAZO_RETIRADA_S);
                alloc::format!("  borrada: se puede devolver hasta el {}-{:02}-{:02}", a, m, d)
            }
        };
        di!(
            o,
            "{:<16} {:>5}  {:<24} {}{}\n",
            x.nombre.as_str(),
            x.uid,
            x.completo.as_str(),
            if grupos.is_empty() { String::from("-") } else { grupos.join(",") },
            estado
        );
    }
    if vistas == 0 {
        o.error(format_args!("no hay una cuenta \"{}\"", solo.unwrap_or("")));
        return o.fin(shell::RC_AVISO);
    }
    o.fin(shell::RC_BIEN)
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::USUARIO);
    let accion = args.texto("ACCION").unwrap_or("LISTA");
    let nombre = args.texto("NOMBRE");
    let mas: Vec<String> = args.varios("MAS").to_vec();
    let uid = args.numero("UID");
    // Los cambios de grupos: "+grupo" pone, "-grupo" quita.
    let grupos = |n: &str| -> Vec<String> {
        mas.iter()
            .filter(|m| m.starts_with('+') || m.starts_with('-'))
            .map(|g| alloc::format!("{} {} {}", if g.starts_with('+') { "meter" } else { "sacar" }, n, &g[1..]))
            .collect()
    };
    let lineas: Vec<String> = match (accion.to_ascii_uppercase().as_str(), nombre) {
        ("LISTA", n) => return listar(o, n),
        ("CLAVE", n) => return clave(o, n),
        ("GRUPOS", Some(n)) if mas.is_empty() => return listar(o, Some(n)),
        ("CREAR", Some(n)) => {
            let completo: Vec<&str> = mas.iter().filter(|m| !m.starts_with('+') && !m.starts_with('-')).map(|m| m.as_str()).collect();
            if completo.len() > 1 {
                o.error(format_args!("el nombre completo, entre comillas: Usuario CREAR {} \"{}\"", n, completo.join(" ")));
                return o.fin(shell::RC_ERROR);
            }
            let u = uid.unwrap_or(0).clamp(0, u32::MAX as i64);
            let mut v = alloc::vec![if completo.is_empty() { alloc::format!("crear {} {}", n, u) } else { alloc::format!("crear {} {} {}", n, u, completo[0]) }];
            v.extend(grupos(n));
            v
        }
        ("BORRAR", Some(n)) => alloc::vec![alloc::format!("borrar {}", n)],
        ("DEVOLVER", Some(n)) => alloc::vec![alloc::format!("devolver {}", n)],
        ("GRUPOS", Some(n)) => {
            if mas.iter().any(|m| !m.starts_with('+') && !m.starts_with('-')) {
                o.error(format_args!("los grupos van con + (poner) o - (quitar): Usuario GRUPOS {} +fotos -admin", n));
                return o.fin(shell::RC_ERROR);
            }
            grupos(n)
        }
        (a @ ("CREAR" | "BORRAR" | "DEVOLVER" | "GRUPOS"), None) => {
            o.error(format_args!("falta el nombre de la cuenta: Usuario {} nombre", a));
            return o.fin(shell::RC_ERROR);
        }
        (otra, _) => {
            o.error(format_args!("\"{}\" no es LISTA, CREAR, BORRAR, DEVOLVER, GRUPOS ni CLAVE", otra));
            return o.fin(shell::RC_ERROR);
        }
    };
    let lista = lineas.join("\n");
    // Se lee aquí también para decir en llano qué no vale sin molestar a nadie; `sesiones` la vuelve a leer, y prueba
    // cada cambio con sus cuentas (esto no le vale de nada a la seguridad).
    let mut acciones = [tanda::Accion::default(); tanda::MAX_ACCIONES];
    let n = match tanda::leer(lista.as_bytes(), &mut acciones) {
        Ok(n) => n,
        Err(mala) => {
            let por = match nombre {
                Some(x) if cu::Nombre::de(x.as_bytes()).is_none() && !cu::RESERVADOS.iter().any(|r| r.eq_ignore_ascii_case(x)) => alloc::format!("{}", cu::Fallo::NombreMalo),
                _ => alloc::format!("{}", mala),
            };
            o.error(format_args!("no se pide nada: {}", por));
            return o.fin(shell::RC_ERROR);
        }
    };
    if n == 0 {
        return listar(o, nombre);
    }
    di!(o, "Se pide en la ventana del sistema la contraseña de un administrador para {} {}...\n", n, if n == 1 { "acción" } else { "acciones" });
    o.vaciar();
    let Some(r) = crate::base::administrar::pedir_tanda(lista.as_bytes()) else {
        o.error(format_args!("sesiones no contesta"));
        return o.fin(shell::RC_ERROR);
    };
    if r.total == 0 || r.hechas == 0 && r.parada == 0 {
        o.error(format_args!("no se hace nada: {}", que_paso(r.estado)));
        return o.fin(shell::RC_ERROR);
    }
    for (i, a) in acciones[..n].iter().enumerate() {
        let (e, dato) = r.de_cada[i];
        let frase = tanda::Frase { accion: *a, programa: "", persona: "" };
        match (e, a.nombre()) {
            (0, "crear") => di!(o, "{}. {}: hecho, número {}; su código de recuperación salió en la ventana del sistema\n", i + 1, frase, dato),
            (0, _) => di!(o, "{}. {}: hecho\n", i + 1, frase),
            (e, _) => di!(o, "{}. {}: {}\n", i + 1, frase, que_paso(e)),
        }
    }
    if r.estado == 0 {
        o.fin(shell::RC_BIEN)
    } else {
        o.error(format_args!("se paró en la {}: {}", r.parada, que_paso(r.estado)));
        o.fin(shell::RC_ERROR)
    }
}
