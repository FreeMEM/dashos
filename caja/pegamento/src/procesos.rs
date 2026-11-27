//! Procesos nuevos de dos (`CreateNewProc`, `CreateProc`): la estructura `Process`, su pila, sus ficheros y su
//! cajón, la tarea en exec, y lo que hay que hacer cuando acaban. Las etiquetas las lee y comprueba
//! `caja_dos::procesos`.
//!
//! La estructura, la pila, la copia de los argumentos y la CLI van en un `MemList` de `tc_MemEntry`: exec los suelta
//! cuando el proceso acaba (`RemTask`). Su dueña es quien llama (o el sistema, si llama código de una biblioteca de
//! disco: así el proceso de datatypes sigue cuando acaba el programa que la abrió).

use alloc::format;
use alloc::vec::Vec;

use caja_dos::estructuras::process as pr;
use caja_dos::procesos::{MalProceso, Proceso, leer_etiquetas};
use caja_exec::estructuras::{memf, memlist, msgport, node, nt, task};
use caja_exec::{Contexto, Fallo, Memoria, Modo, Registros};

use crate::{Caja, CajaDos, di};

/// Lo que hay que hacer cuando acabe un proceso de `CreateNewProc`.
#[derive(Clone, Debug, Default)]
pub(crate) struct AlAcabar {
    /// Ficheros que cerrar (`NP_CloseInput`, `NP_CloseOutput`, `NP_CloseError`, y los `NIL:` que abrió la caja).
    cerrar: Vec<u32>,
    /// Cerrojos que soltar (su cajón y el de su programa).
    soltar: Vec<u32>,
    /// La lista de segmentos que soltar (`NP_FreeSeglist`).
    seglist: u32,
}

/// El proceso `t` acaba (o lo quitan): lo que pidió al crearse.
pub(crate) fn al_acabar(c: &mut Caja, t: u32) {
    let Some(a) = c.al_acabar.remove(&t) else { return };
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false };
    for f in a.cerrar {
        c.dos.cerrar_bptr(&mut cd, f);
    }
    for l in a.soltar {
        c.dos.soltar_bptr(&mut cd, l);
    }
    if a.seglist != 0 {
        let mut r = caja_dos::Registros::default();
        r.d[1] = a.seglist;
        let _ = c.dos.llamar(&mut cd, caja_dos::lvo::UN_LOAD_SEG, &mut r);
    }
}

/// `CreateNewProc(tags)(d1)`: el proceso nuevo, o 0. Con `Ceder` si es de más prioridad que quien llama.
pub(crate) fn create_new_proc(c: &mut Caja, tags: u32) -> (u32, bool) {
    let p = match leer_etiquetas(&CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema: false }, tags) {
        Ok(p) => p,
        Err(e) => {
            di(&format!("CreateNewProc: {}: no se crea", texto_mal(e)));
            return (0, false);
        }
    };
    crear(c, p)
}

/// `CreateProc(name, pri, segList, stackSize)(d1/d2/d3/d4)`: como `CreateNewProc` con esas cuatro; devuelve el
/// `pr_MsgPort` del proceso, o 0.
pub(crate) fn create_proc(c: &mut Caja, r: &Registros) -> (u32, bool) {
    let nombre = c.ex.leer_cadena(r.d[1]).map(|s| s.bytes().take(caja_dos::procesos::MAX_NOMBRE).collect());
    let mut p = Proceso {
        seglist: r.d[3],
        pila: r.d[4].max(caja_dos::procesos::PILA_MINIMA),
        nombre: nombre.unwrap_or_else(|_| caja_dos::procesos::NOMBRE_DE_SERIE.to_vec()),
        prioridad: Some(r.d[2] as i32 as i8),
        cerrar_entrada: true,
        cerrar_salida: true,
        copiar_variables: true,
        ..Default::default()
    };
    if r.d[4] > caja_dos::procesos::PILA_MAXIMA || p.seglist == 0 || p.seglist > 0x3FFF_FFFF {
        di(&format!("CreateProc: lista de segmentos ${:x} o pila de {} bytes: no se crea", r.d[3], r.d[4]));
        return (0, false);
    }
    p.entrada = (p.seglist << 2) + 4;
    if c.ex.mem.leer16(p.entrada).is_err() {
        di(&format!("CreateProc: el código en ${:x} no está en la memoria: no se crea", p.entrada));
        return (0, false);
    }
    let (t, ceder) = crear(c, p);
    (if t != 0 { t + pr::PR_MSGPORT } else { 0 }, ceder)
}

fn texto_mal(e: MalProceso) -> alloc::string::String {
    match e {
        MalProceso::Lista(f) => format!("la lista de etiquetas no se lee ({:?})", f),
        MalProceso::SinCodigo => alloc::string::String::from("sin NP_Seglist ni NP_Entry (o con los dos)"),
        MalProceso::EntradaMala { dir } => format!("el código en ${:x} no vale", dir),
        MalProceso::PilaAbsurda { bytes } => format!("una pila de {} bytes", bytes),
    }
}

/// La CLI de un proceso (lo mismo que la de un programa de la caja, `crear_proceso`).
const CLI_NOMBRE: u32 = 64;
const CLI_DIR: u32 = CLI_NOMBRE + 256;
const CLI_INDICADOR: u32 = CLI_DIR + 256;
const CLI_TAMANO: u32 = CLI_INDICADOR + 64;

fn crear(c: &mut Caja, p: Proceso) -> (u32, bool) {
    match montar(c, &p) {
        Ok(t) => {
            let ceder = c.ex.tras_anadir();
            (t, ceder)
        }
        Err(f) => {
            di(&format!("CreateNewProc \"{}\": no se crea ({:?})", nombre(&p.nombre), f));
            (0, false)
        }
    }
}

fn nombre(n: &[u8]) -> alloc::string::String {
    n.iter().map(|&b| b as char).collect()
}

/// Reserva para el proceso nuevo, y lo apunta para soltarlo si algo falla después.
fn reservar(c: &mut Caja, hechas: &mut Vec<(u32, u32)>, largo: u32, req: u32) -> Result<u32, Fallo> {
    let d = c.ex.alloc_mem(largo, req)?;
    if d == 0 {
        return Err(Fallo::SinSitio);
    }
    hechas.push((d, largo));
    Ok(d)
}

fn montar(c: &mut Caja, p: &Proceso) -> Result<u32, Fallo> {
    let mut hechas: Vec<(u32, u32)> = Vec::new();
    let r = montar_dentro(c, p, &mut hechas);
    if r.is_err() {
        for (d, l) in hechas {
            let _ = c.ex.free_mem(d, l);
        }
    }
    r
}

fn montar_dentro(c: &mut Caja, p: &Proceso, hechas: &mut Vec<(u32, u32)>) -> Result<u32, Fallo> {
    let padre = c.ex.tarea_actual();
    let padre_es_proceso = padre != 0 && c.ex.mem.leer8(padre + node::LN_TYPE) == Ok(nt::PROCESS);
    let del_padre = |c: &Caja, campo: u32| if padre_es_proceso { c.ex.mem.leer32(padre + campo).unwrap_or(0) } else { 0 };
    let pri = p.prioridad.unwrap_or_else(|| if padre != 0 { c.ex.mem.leer8(padre + node::LN_PRI).unwrap_or(0) as i8 } else { 0 });
    let pila = (p.pila + 3) & !3;
    let largo_t = pr::TAMANO + p.nombre.len() as u32 + 1;
    let n_args = p.argumentos.as_ref().map_or(0, |a| a.len() as u32 + 1);
    // Lo de tc_MemEntry: la estructura, la pila, los argumentos y la CLI.
    let entradas = 2 + (n_args > 0) as u32 + p.cli as u32;
    let largo_ml = memlist::ML_ME + entradas * memlist::ME_TAMANO;
    let t = reservar(c, hechas, largo_t, memf::PUBLIC | memf::CLEAR)?;
    let pi = reservar(c, hechas, pila, memf::CLEAR)?;
    let args = if n_args > 0 { reservar(c, hechas, n_args, memf::PUBLIC | memf::CLEAR)? } else { 0 };
    let cli = if p.cli { reservar(c, hechas, CLI_TAMANO, memf::PUBLIC | memf::CLEAR)? } else { 0 };
    let ml = reservar(c, hechas, largo_ml, memf::PUBLIC | memf::CLEAR)?;
    let ex = &mut c.ex;
    // El nodo y la tarea.
    let n = t + pr::TAMANO;
    ex.mem.leer_mut(n, p.nombre.len() as u32)?.copy_from_slice(&p.nombre);
    ex.mem.escribir8(t + node::LN_TYPE, nt::PROCESS)?;
    ex.mem.escribir8(t + node::LN_PRI, pri as u8)?;
    ex.mem.escribir32(t + node::LN_NAME, n)?;
    ex.mem.escribir32(t + task::TC_SPLOWER, pi)?;
    ex.mem.escribir32(t + task::TC_SPUPPER, pi + pila)?;
    ex.mem.escribir32(t + task::TC_SPREG, pi + pila)?;
    ex.mem.escribir32(t + task::TC_TRAPCODE, c.trampa)?;
    ex.new_list(t + task::TC_MEMENTRY, nt::MEMORY)?;
    ex.mem.escribir16(ml + memlist::ML_NUMENTRIES, entradas as u16)?;
    let mut k = 0;
    for (d, l) in [(t, largo_t), (pi, pila), (args, n_args), (cli, if cli != 0 { CLI_TAMANO } else { 0 })] {
        if d != 0 {
            let e = ml + memlist::ML_ME + k * memlist::ME_TAMANO;
            ex.mem.escribir32(e + memlist::ME_ADDR, d)?;
            ex.mem.escribir32(e + memlist::ME_LENGTH, l)?;
            k += 1;
        }
    }
    ex.add_head(t + task::TC_MEMENTRY, ml)?;
    // pr_MsgPort, con la señal de dos (8).
    let mp = t + pr::PR_MSGPORT;
    ex.mem.escribir8(mp + node::LN_TYPE, nt::MSGPORT)?;
    ex.mem.escribir8(mp + msgport::MP_FLAGS, msgport::PA_SIGNAL)?;
    ex.mem.escribir8(mp + msgport::MP_SIGBIT, 8)?;
    ex.mem.escribir32(mp + msgport::MP_SIGTASK, t)?;
    ex.new_list(mp + msgport::MP_MSGLIST, nt::MESSAGE)?;
    ex.mem.escribir32(t + pr::PR_STACKSIZE, pila)?;
    ex.mem.escribir32(t + pr::PR_STACKBASE, (pi + pila) >> 2)?;
    if args != 0 {
        let a = p.argumentos.as_deref().unwrap_or(&[]);
        ex.mem.leer_mut(args, a.len() as u32)?.copy_from_slice(a);
    }
    ex.mem.escribir32(t + pr::PR_ARGUMENTS, args)?;
    let consola = p.tarea_consola.unwrap_or_else(|| del_padre(c, pr::PR_CONSOLETASK));
    let ventana = p.ventana.unwrap_or_else(|| del_padre(c, pr::PR_WINDOWPTR));
    let ex = &mut c.ex;
    ex.mem.escribir32(t + pr::PR_CONSOLETASK, consola)?;
    ex.mem.escribir32(t + pr::PR_WINDOWPTR, ventana)?;
    // La tarea, en exec: empieza en su código con A0 = los argumentos y D0 = su largo (como desde la CLI).
    ex.add_task(t, p.entrada, 0)?;
    let mut r = Registros::default();
    r.a[7] = ex.mem.leer32(t + task::TC_SPREG)?;
    ex.mem.escribir32(t + pr::PR_RETURNADDR, r.a[7])?;
    if args != 0 {
        r.a[0] = args;
        r.d[0] = n_args - 1;
    }
    ex.guardar_contexto(t, Contexto::nuevo(r, Modo::Saltar(p.entrada)))?;
    // Desde aquí la tarea existe: lo que falle se queda en "sin fichero" en vez de deshacerla.
    let mut al = AlAcabar { seglist: if p.soltar_seglist { p.seglist } else { 0 }, ..Default::default() };
    let cajon_padre = del_padre(c, pr::PR_CURRENTDIR);
    let programa_padre = del_padre(c, pr::PR_HOMEDIR);
    // Lo que reserve dos para él (sus FileHandle y FileLock) es de quien sea dueño de lo nuevo: del sistema si lo
    // crea una biblioteca de disco (sigue cuando acabe el programa que la abrió).
    let sistema = c.ex.dueno_de_lo_nuevo() == 0;
    let mut cd = CajaDos { ex: &mut c.ex, consola: &mut c.consola, sistema };
    let mut fichero = |dos: &mut caja_dos::Dos<caja_dos::guardia::Guardia<crate::archivos::FsDashos>>, cd: &mut CajaDos, dado: Option<u32>, cerrar: bool| -> u32 {
        match dado {
            // Lo de serie, NIL:, lo abre la caja para él: se cierra al acabar.
            None => match dos.abrir_nil(cd) {
                Ok(f) => {
                    dos.dar_fichero(f, t);
                    al.cerrar.push(f);
                    f
                }
                Err(_) => 0,
            },
            Some(f) => {
                if cerrar && f != 0 && dos.es_fichero(f) {
                    dos.dar_fichero(f, t);
                    al.cerrar.push(f);
                }
                f
            }
        }
    };
    let cis = fichero(&mut c.dos, &mut cd, p.entrada_fh, p.cerrar_entrada);
    let cos = fichero(&mut c.dos, &mut cd, p.salida_fh, p.cerrar_salida);
    let ces = match p.error_fh {
        None => 0,
        e => fichero(&mut c.dos, &mut cd, e, p.cerrar_error),
    };
    let mut cerrojo = |dos: &mut caja_dos::Dos<caja_dos::guardia::Guardia<crate::archivos::FsDashos>>, cd: &mut CajaDos, dado: Option<u32>, del_padre: u32| -> u32 {
        let l = match dado {
            Some(l) => l,
            None => dos.duplicar_cerrojo(cd, del_padre).unwrap_or(0),
        };
        if l != 0 && dos.dar_cerrojo(l, t) {
            al.soltar.push(l);
        }
        l
    };
    let cajon = cerrojo(&mut c.dos, &mut cd, p.cajon, cajon_padre);
    let programa = cerrojo(&mut c.dos, &mut cd, p.cajon_programa, programa_padre);
    let ex = &mut c.ex;
    ex.mem.escribir32(t + pr::PR_CIS, cis)?;
    ex.mem.escribir32(t + pr::PR_COS, cos)?;
    ex.mem.escribir32(t + pr::PR_CES, ces)?;
    ex.mem.escribir32(t + pr::PR_CURRENTDIR, cajon)?;
    ex.mem.escribir32(t + pr::PR_HOMEDIR, programa)?;
    if cli != 0 {
        use caja_dos::estructuras::cli as cl;
        let orden = p.nombre_orden.as_deref().unwrap_or(&p.nombre);
        let bstr = |ex: &mut caja_exec::Exec<crate::memoria::MemoriaCaja>, en: u32, texto: &[u8]| -> Result<(), Fallo> {
            let n = texto.len().min(255) as u32;
            ex.mem.escribir8(en, n as u8)?;
            ex.mem.leer_mut(en + 1, n)?.copy_from_slice(&texto[..n as usize]);
            Ok(())
        };
        bstr(ex, cli + CLI_NOMBRE, orden)?;
        bstr(ex, cli + CLI_DIR, b"SYS:")?;
        bstr(ex, cli + CLI_INDICADOR, b"%N> ")?;
        for (campo, v) in [
            (cl::CLI_SETNAME, (cli + CLI_DIR) >> 2),
            (cl::CLI_COMMANDNAME, (cli + CLI_NOMBRE) >> 2),
            (cl::CLI_FAILLEVEL, 10),
            (cl::CLI_PROMPT, (cli + CLI_INDICADOR) >> 2),
            (cl::CLI_STANDARDINPUT, cis),
            (cl::CLI_CURRENTINPUT, cis),
            (cl::CLI_CURRENTOUTPUT, cos),
            (cl::CLI_DEFAULTSTACK, pila / 4),
            (cl::CLI_STANDARDOUTPUT, cos),
            (cl::CLI_MODULE, p.seglist),
        ] {
            ex.mem.escribir32(cli + campo, v)?;
        }
        ex.mem.escribir32(t + pr::PR_CLI, cli >> 2)?;
    }
    if p.al_salir != 0 {
        di(&format!("CreateNewProc \"{}\": NP_ExitCode aún no se llama al acabar", nombre(&p.nombre)));
    }
    c.al_acabar.insert(t, al);
    c.procesos_creados += 1;
    Ok(t)
}
