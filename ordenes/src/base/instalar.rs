//! `Instalar`: el instalador de DashOS desde el Shell (S32), lo mismo que la ventana "Instalar DashOS". Solo hay
//! instalador en vivo (arrancado del pendrive); el trabajo lo hace el servicio `instalador`.
//!
//! - `Instalar`: los discos del ordenador y lo que tienen (Windows, Linux, la partición de arranque EFI...), y lo que
//!   no se toca y por qué.
//! - `Instalar nvme0 hueco` (o `disco-entero`, `reinstalar`, `encoger 3`): el plan, con el dibujo del disco antes y
//!   después. No escribe nada. Se puede guardar: `Instalar nvme0 hueco >RAM:plan`.
//! - `Instalar PLAN RAM:plan`: aplica ese plan (que no haya cambiado el disco desde que se hizo). Antes, la ventana del
//!   sistema enseña "Instalar DashOS con el plan N" y pide la contraseña de un administrador (S6b): una sola pregunta.
//!   Escribe el registro de lo hecho.

use crate::{di, Orden};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use dashos::protocolo::sesiones as s;

const VA_TEXTO: usize = 0x47_0000_0000;
const MAX_TEXTO: usize = 64 * 1024;
const VER: u32 = 1;
const PLAN: u32 = 2;
const INSTALAR: u32 = 3;
/// Instalar puede tardar (copiar el sistema, crecerlo): diez minutos.
const PLAZO_US: u64 = 600_000_000;

/// La llave del servicio `instalador`, pedida a `sesiones`.
fn servicio(o: &mut Orden) -> Option<u32> {
    let nombre = b"instalador";
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&s::SERVICIO.to_le_bytes());
    p[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[12..12 + nombre.len()].copy_from_slice(nombre);
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(crate::SESIONES, &p[..12 + nombre.len()], &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 130_000_000)
    } else {
        (-1, 0, 0)
    };
    dashos::cerrar(lectura);
    if n != 4 || k != 1 {
        di!(o, "No hay instalador: solo está en DashOS arrancado del pendrive (en vivo).\n");
        return None;
    }
    Some(ll[0])
}

/// Una orden al instalador con `entrada`: (estado, texto de salida).
fn pedir(llave: u32, orden: u32, entrada: &str, plazo: u64) -> Option<(i32, String)> {
    let m = dashos::memoria_crear(MAX_TEXTO);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_TEXTO, true) != 0 {
        return None;
    }
    let n = entrada.len().min(MAX_TEXTO);
    // SAFETY: recién proyectada aquí con `MAX_TEXTO` bytes.
    unsafe { core::slice::from_raw_parts_mut(VA_TEXTO as *mut u8, n) }.copy_from_slice(&entrada.as_bytes()[..n]);
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let (lectura, suyo) = dashos::canal_crear();
    let mut p = [0u8; 8];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    let mut r = [0u8; 8];
    let res = if copia > 0 && dashos::canal_escribir(llave, &p, &[suyo, copia as u32]) == 0 {
        let (k, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], plazo);
        (k == 8).then(|| {
            let estado = i32::from_le_bytes(r[0..4].try_into().unwrap());
            let largo = (u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize).min(MAX_TEXTO);
            // SAFETY: sigue proyectada; el servicio ya escribió (contestó después).
            let t = unsafe { core::slice::from_raw_parts(VA_TEXTO as *const u8, largo) };
            (estado, String::from_utf8_lossy(t).into_owned())
        })
    } else {
        None
    };
    dashos::cerrar(lectura);
    dashos::memoria_quitar(VA_TEXTO);
    dashos::cerrar(m);
    res
}

/// El número del plan para la tanda: su CRC-32 (el instalador hace lo mismo y solo aplica el plan autorizado).
pub fn numero_del_plan(texto: &str) -> u32 {
    let mut c = !0u32;
    for &b in texto.as_bytes() {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::INSTALAR);
    let disco = args.texto("DISCO").map(String::from);
    let modo: Vec<String> = args.varios("MODO").to_vec();
    let plan = args.texto("PLAN").map(String::from);
    let Some(llave) = servicio(o) else { return o.fin(shell::RC_ERROR) };
    let r = if let Some(f) = plan {
        let texto = match o.s.leer_entero(&f, MAX_TEXTO) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                o.error_de(e, &f);
                return o.fin(shell::RC_ERROR);
            }
        };
        let n = numero_del_plan(&texto);
        di!(o, "Se pide la confirmación de un administrador para instalar con el plan {} ...\n", n);
        match crate::base::administrar::pedir_tanda(format!("instalar {}\n", n).as_bytes()) {
            Some(r) if r.estado == 0 => {}
            Some(r) => {
                di!(o, "No se instala: no se confirmó ({}).\n", r.estado);
                return o.fin(shell::RC_ERROR);
            }
            None => {
                di!(o, "No se instala: sesiones no contesta.\n");
                return o.fin(shell::RC_ERROR);
            }
        }
        di!(o, "Instalando (no apagues el ordenador) ...\n");
        pedir(llave, INSTALAR, &texto, PLAZO_US)
    } else if let Some(d) = disco {
        if modo.is_empty() {
            di!(o, "Falta qué hacer: hueco, disco-entero, reinstalar o encoger <partición>.\n");
            return o.fin(shell::RC_ERROR);
        }
        pedir(llave, PLAN, &format!("disco {}\npedido {}\n", d, modo.join(" ")), 30_000_000)
    } else {
        pedir(llave, VER, "", 30_000_000)
    };
    dashos::cerrar(llave);
    match r {
        Some((0, t)) => {
            di!(o, "{}", t);
            if !t.ends_with('\n') {
                di!(o, "\n");
            }
            o.fin(shell::RC_BIEN)
        }
        Some((_, t)) => {
            di!(o, "No: {}\n", t);
            o.fin(shell::RC_ERROR)
        }
        None => {
            di!(o, "El instalador no contesta.\n");
            o.fin(shell::RC_ERROR)
        }
    }
}
