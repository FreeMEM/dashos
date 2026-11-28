//! `RunCommand`, `SystemTagList` y `Execute` de dos: ejecutar una orden (su lista de segmentos) en anidado, dentro de
//! la tarea que llama, con su pila y sus argumentos, y seguir cuando vuelve (`Continuacion::DeLaCaja`). Lo hace la
//! caja y no dos porque hay que ejecutar 68k. Lo comprobado contra el oráculo, en `caja_dos::ordenes` y
//! `pruebas68k/sistema.c`.
//!
//! - La orden recibe sus argumentos en A0 y D0 (con su salto de línea) y en `pr_Arguments` (GetArgStr, ReadArgs);
//!   al volver, los de quien llama vuelven como estaban.
//! - `SystemTagList` y `Execute` cargan la orden por su nombre (o de `C:`); si no existe, lo dice como el Shell y
//!   devuelve 10 (`Execute`, FALSE). `SYS_Input` y `SYS_Output` cambian la entrada y la salida mientras corre.
//! - Lo que falta: `SYS_Asynch` (lanzarla aparte; se avisa como "falta"), y el guion de entrada de `Execute` (no hay
//!   Shell que lo lea).

use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Continuacion, Memoria, Registros};

use crate::{Caja, CajaDos, Fin, MODO_DE_QUIEN_LLAMA};

/// Qué devuelve la llamada cuando vuelve la orden.
#[derive(Clone, Copy)]
pub(crate) enum Vuelta {
    /// El código de la orden (RunCommand, SystemTagList).
    Codigo,
    /// DOSTRUE (Execute).
    Cierto,
}

pub(crate) struct Orden {
    id: u32,
    pila: (u32, u32),
    args: (u32, u32),
    seg: u32,
    antes: (u32, Option<usize>),
    entrada_salida: Option<(u32, u32)>,
    nombre: Vec<u8>,
    vuelta: Vuelta,
}

const TAG_USER: u32 = 0x8000_0000;
const SYS_INPUT: u32 = TAG_USER + 32 + 1;
const SYS_OUTPUT: u32 = TAG_USER + 32 + 2;
const SYS_ASYNCH: u32 = TAG_USER + 32 + 3;
const PILA: u32 = 16 * 1024;
const FALLO: u32 = 10;
const MAX_ORDENES: usize = 32;

/// Una cadena de C de la memoria del 68k, en bytes (como mucho 1024), o None si no se puede leer.
fn cadena(c: &Caja, d: u32) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    for k in 0..1024u32 {
        let b = c.ex.mem.leer8(d.checked_add(k)?).ok()?;
        if b == 0 {
            return Some(v);
        }
        v.push(b);
    }
    Some(v)
}

/// Las etiquetas (hasta TAG_DONE; como mucho 64).
fn etiquetas(c: &Caja, mut t: u32) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    while t != 0 && v.len() < 64 {
        let (Ok(e), Ok(d)) = (c.ex.mem.leer32(t), c.ex.mem.leer32(t.wrapping_add(4))) else { break };
        match e {
            0 => break,
            1 => {} // TAG_IGNORE
            2 => {
                t = d;
                continue;
            } // TAG_MORE
            _ => v.push((e, d)),
        }
        t = t.wrapping_add(8);
    }
    v
}

/// RunCommand(seglist, stacksize, paramptr, paramlength): ejecuta la orden ya cargada.
pub(crate) fn run_command(c: &mut Caja, r: &mut Registros) -> Fin {
    let (seg, pila, args, largo) = (r.d[1], r.d[2], r.d[3], r.d[4]);
    lanzar(c, r, seg, pila.max(4096), (args, largo), (0, 0), 0, None, Vec::new(), Vuelta::Codigo)
}

/// SystemTagList(command, tags) y Execute(string, input, output): carga la orden de la línea y la ejecuta.
pub(crate) fn system(c: &mut Caja, r: &mut Registros, execute: bool) -> Fin {
    let Some(linea) = cadena(c, r.d[1]) else {
        r.d[0] = if execute { 0 } else { u32::MAX };
        return Fin::Seguir;
    };
    let (orden, resto) = caja_dos::ordenes::partir_orden(&linea);
    let mut entrada_salida = None;
    if execute {
        if r.d[3] != 0 {
            let (cis, _) = c.dos.entrada_salida(&CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false });
            entrada_salida = Some((cis, r.d[3]));
        }
    } else {
        let tags = etiquetas(c, r.d[2]);
        if tags.iter().any(|&(e, d)| e == SYS_ASYNCH && d != 0) {
            let q = crate::faltan::quien(c, c.pc_llamada);
            if let Some(t) = c.faltan.apuntar("dos.library", "SystemTagList con SYS_Asynch (va en anidado)", &q) {
                crate::di(&t);
            }
        }
        let entrada = tags.iter().find(|&&(e, _)| e == SYS_INPUT).map(|&(_, d)| d);
        let salida = tags.iter().find(|&&(e, _)| e == SYS_OUTPUT).map(|&(_, d)| d);
        if entrada.is_some() || salida.is_some() {
            let (cis, cos) = c.dos.entrada_salida(&CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false });
            entrada_salida = Some((entrada.unwrap_or(cis), salida.unwrap_or(cos)));
        }
    }
    if orden.is_empty() {
        r.d[0] = if execute { 0 } else { u32::MAX };
        return Fin::Seguir;
    }
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let seg = match c.dos.cargar_orden(&mut cd, &orden) {
        Ok(s) => s,
        Err(e) => {
            // Lo que dice el Shell (oráculo).
            let mut t = orden.clone();
            t.extend_from_slice(b": Unknown command\n");
            t.extend_from_slice(&orden);
            t.extend_from_slice(b" failed returncode 10\n");
            c.dos.decir_en_salida(&mut cd, &t);
            c.dos.poner_io_err_orden(&mut cd, e);
            r.d[0] = if execute { 0 } else { FALLO };
            return Fin::Seguir;
        }
    };
    // Los argumentos, con su salto de línea y su 0.
    let mut a = resto;
    a.push(b'\n');
    let largo = a.len() as u32;
    a.push(0);
    let m = c.ex.alloc_mem(a.len() as u32, memf::PUBLIC | memf::CLEAR).unwrap_or(0);
    if m == 0 || c.ex.mem.leer_mut(m, a.len() as u32).map(|b| b.copy_from_slice(&a)).is_err() {
        let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
        c.dos.descargar_orden(&mut cd, seg);
        r.d[0] = if execute { 0 } else { u32::MAX };
        return Fin::Seguir;
    }
    let vuelta = if execute { Vuelta::Cierto } else { Vuelta::Codigo };
    lanzar(c, r, seg, PILA, (m, largo), (m, a.len() as u32), seg, entrada_salida, orden, vuelta)
}

#[allow(clippy::too_many_arguments)]
fn lanzar(
    c: &mut Caja,
    r: &mut Registros,
    seg: u32,
    pila: u32,
    (args, largo): (u32, u32),
    args_mem: (u32, u32),
    seg_propio: u32,
    entrada_salida: Option<(u32, u32)>,
    nombre: Vec<u8>,
    vuelta: Vuelta,
) -> Fin {
    let entrada = (seg << 2).wrapping_add(4);
    let p = if c.ordenes.len() < MAX_ORDENES && seg != 0 && seg < 0x4000_0000 {
        c.ex.alloc_mem(pila, memf::PUBLIC | memf::CLEAR).unwrap_or(0)
    } else {
        0
    };
    if p == 0 {
        if args_mem.0 != 0 {
            let _ = c.ex.free_mem(args_mem.0, args_mem.1);
        }
        if seg_propio != 0 {
            let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
            c.dos.descargar_orden(&mut cd, seg_propio);
        }
        r.d[0] = u32::MAX;
        return Fin::Seguir;
    }
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    let antes = c.dos.argumentos_de_ahora(&cd);
    c.dos.poner_argumentos(&mut cd, args, None);
    let es_antes = c.dos.entrada_salida(&cd);
    if let Some((cis, cos)) = entrada_salida {
        c.dos.poner_entrada_salida(&mut cd, cis, cos);
    }
    c.siguiente_orden = c.siguiente_orden.wrapping_add(1).max(1);
    let id = c.siguiente_orden;
    c.ordenes.push(Orden {
        id,
        pila: (p, pila),
        args: args_mem,
        seg: seg_propio,
        antes,
        entrada_salida: entrada_salida.map(|_| es_antes),
        nombre,
        vuelta,
    });
    let mut regs = *r;
    regs.a[0] = args;
    regs.d[0] = largo;
    regs.a[7] = p + pila;
    Fin::Ejecutar { dir: entrada, regs, tras: Continuacion::DeLaCaja { id }, modo: MODO_DE_QUIEN_LLAMA }
}

/// Volvió la orden `id` (D0, su código en `r`): se suelta lo suyo y se deja lo que devuelve la llamada.
pub(crate) fn acabada(c: &mut Caja, id: u32, r: &mut Registros) -> Accion {
    let Some(k) = c.ordenes.iter().position(|o| o.id == id) else { return Accion::Seguir };
    let o = c.ordenes.remove(k);
    let codigo = r.d[0];
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    c.dos.poner_argumentos(&mut cd, o.antes.0, o.antes.1);
    if let Some((cis, cos)) = o.entrada_salida {
        c.dos.poner_entrada_salida(&mut cd, cis, cos);
    }
    if o.seg != 0 {
        if codigo as i32 >= FALLO as i32 {
            let mut t = o.nombre.clone();
            t.extend_from_slice(alloc::format!(" failed returncode {}\n", codigo as i32).as_bytes());
            c.dos.decir_en_salida(&mut cd, &t);
        }
        c.dos.descargar_orden(&mut cd, o.seg);
    }
    let _ = c.ex.free_mem(o.pila.0, o.pila.1);
    if o.args.0 != 0 {
        let _ = c.ex.free_mem(o.args.0, o.args.1);
    }
    r.d[0] = match o.vuelta {
        Vuelta::Codigo => codigo,
        Vuelta::Cierto => u32::MAX,
    };
    Accion::Seguir
}
