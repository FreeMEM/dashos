//! Tareas, señales, Forbid/Disable y el planificador.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Accion, Fallo, Memoria, Registros};
use comun::*;

fn estado(e: &caja_exec::Exec<caja_exec::MemoriaVec>, t: u32) -> u8 {
    l8(e, t + task::TC_STATE)
}

#[test]
fn crear_tarea_como_create_task() {
    let mut e = caja();
    let t = e.crear_tarea("hija", 5, 4096, 0x12_3456).unwrap();
    assert_eq!(e.leer_cadena(l32(&e, t + node::LN_NAME)).unwrap(), "hija");
    assert_eq!(l8(&e, t + node::LN_TYPE), nt::TASK);
    assert_eq!(l8(&e, t + node::LN_PRI), 5);
    assert_eq!(estado(&e, t), ts::READY);
    assert_eq!(l32(&e, t + task::TC_SIGALLOC), sig::DEL_SISTEMA);
    // En la pila, la vuelta al código de fin de tarea de exec.
    let (pc, sp) = e.arranque(t).unwrap();
    assert_eq!(pc, 0x12_3456);
    assert_eq!(sp, l32(&e, t + task::TC_SPUPPER) - 4);
    assert_eq!(l32(&e, sp), l32(&e, e.base() + execbase::TASKEXITCODE));
    assert_eq!(e.nodos(e.base() + execbase::TASKREADY).unwrap(), vec![t]);
    // Su memoria es suya.
    assert_eq!(e.bloque(t).unwrap().dueno, t);
}

#[test]
fn el_planificador_elige_la_de_mas_prioridad() {
    let mut e = caja();
    let baja = e.crear_tarea("baja", -5, 1024, 0x1000).unwrap();
    let alta = e.crear_tarea("alta", 10, 1024, 0x1000).unwrap();
    let media = e.crear_tarea("media", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(alta));
    assert_eq!(e.tarea_actual(), alta);
    assert_eq!(estado(&e, alta), ts::RUN);
    // Sigue la misma mientras no espere.
    assert_eq!(e.elegir_siguiente(), Some(alta));
    assert!(!e.fin_de_turno(), "las demás son de menos prioridad");
    assert_eq!(e.elegir_siguiente(), Some(alta));
    assert_eq!(e.nodos(e.base() + execbase::TASKREADY).unwrap(), vec![media, baja]);
}

#[test]
fn turnos_entre_iguales() {
    let mut e = caja();
    let a = e.crear_tarea("a", 0, 1024, 0x1000).unwrap();
    let b = e.crear_tarea("b", 0, 1024, 0x1000).unwrap();
    let c = e.crear_tarea("c", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(a));
    let mut orden = vec![];
    for _ in 0..6 {
        assert!(e.fin_de_turno());
        orden.push(e.elegir_siguiente().unwrap());
    }
    assert_eq!(orden, vec![b, c, a, b, c, a]);
}

#[test]
fn wait_y_signal() {
    let (mut e, t) = caja_con_tarea();
    let s = e.alloc_signal(-1).unwrap();
    assert_eq!(s, 31, "la libre más alta");
    let m = 1u32 << s;
    // Sin la señal, se bloquea y pasa a TaskWait.
    assert_eq!(e.wait(m).unwrap(), caja_exec::Espera::Bloquea);
    assert_eq!(estado(&e, t), ts::WAIT);
    assert_eq!(l32(&e, t + task::TC_SIGWAIT), m);
    assert_eq!(e.nodos(e.base() + execbase::TASKWAIT).unwrap(), vec![t]);
    assert_eq!(e.elegir_siguiente(), None, "nadie listo: la caja duerme");
    assert_eq!(e.tarea_actual(), 0);
    // Otra señal no la despierta.
    assert_eq!(e.signal(t, 1 << 30).unwrap(), Accion::Seguir);
    assert_eq!(estado(&e, t), ts::WAIT);
    // La suya sí.
    e.signal(t, m).unwrap();
    assert_eq!(estado(&e, t), ts::READY);
    assert_eq!(e.elegir_siguiente(), Some(t));
    // La repetición de Wait devuelve las señales esperadas y las quita.
    assert_eq!(e.wait(m | 1 << 29).unwrap(), caja_exec::Espera::Hecho(m));
    assert_eq!(l32(&e, t + task::TC_SIGRECVD), 1 << 30);
}

#[test]
fn wait_por_la_tabla_bloquea_y_se_repite() {
    let (mut e, t) = caja_con_tarea();
    let mut r = Registros::default();
    r.d[0] = sig::SIGBREAKF_CTRL_C;
    assert_eq!(llamar(&mut e, "Wait", &mut r), Accion::Bloquear);
    assert_eq!(e.elegir_siguiente(), None);
    e.senal_ctrl_c(t).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    // La caja repite la llamada con los mismos registros.
    assert_eq!(llamar(&mut e, "Wait", &mut r), Accion::Seguir);
    assert_eq!(r.d[0], sig::SIGBREAKF_CTRL_C);
}

#[test]
fn signal_a_una_de_mas_prioridad_cede() {
    let (mut e, _t) = caja_con_tarea();
    let alta = e.crear_tarea("alta", 20, 1024, 0x1000).unwrap();
    // Recién añadida y con más prioridad: AddTask por la tabla dice que hay que ceder.
    assert_eq!(e.elegir_siguiente(), Some(alta));
    let s = e.alloc_signal(-1).unwrap();
    assert!(matches!(e.wait(1 << s).unwrap(), caja_exec::Espera::Bloquea));
    let principal = e.elegir_siguiente().unwrap();
    assert_eq!(e.signal(alta, 1 << s).unwrap(), Accion::Ceder);
    assert_eq!(e.elegir_siguiente(), Some(alta));
    assert_eq!(estado(&e, principal), ts::READY);
}

#[test]
fn add_task_por_la_tabla_cede_si_es_mas_prioritaria() {
    let (mut e, _t) = caja_con_tarea();
    let nueva = e.alloc_mem(task::TAMANO, memf::CLEAR).unwrap();
    let pila = e.alloc_mem(512, 0).unwrap();
    e.mem.escribir8(nueva + node::LN_PRI, 3).unwrap();
    e.mem.escribir32(nueva + task::TC_SPLOWER, pila).unwrap();
    e.mem.escribir32(nueva + task::TC_SPUPPER, pila + 512).unwrap();
    e.mem.escribir32(nueva + task::TC_SPREG, pila + 512).unwrap();
    let mut r = Registros::default();
    r.a[1] = nueva;
    r.a[2] = 0x4000;
    r.a[3] = 0x5000;
    assert_eq!(llamar(&mut e, "AddTask", &mut r), Accion::Ceder);
    assert_eq!(r.d[0], nueva);
    assert_eq!(l32(&e, pila + 508), 0x5000, "finalPC en la pila");
    assert_eq!(e.elegir_siguiente(), Some(nueva));
}

#[test]
fn forbid_retiene_el_testigo() {
    let (mut e, t) = caja_con_tarea();
    let alta = e.crear_tarea("alta", 9, 1024, 0x1000).unwrap();
    // Sin Forbid, la nueva se lleva el testigo enseguida. Con Forbid, no.
    let _ = alta;
    e.forbid().unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    assert!(e.prohibido());
    e.forbid().unwrap();
    assert_eq!(e.permit().unwrap(), Accion::Seguir);
    // Signal dentro de Forbid deja la replanificación para el Permit.
    let s = e.alloc_signal(-1).unwrap();
    let _ = s;
    assert!(!e.fin_de_turno());
    assert_eq!(e.elegir_siguiente(), Some(t));
    assert_eq!(e.permit().unwrap(), Accion::Ceder, "al salir del Forbid, hay una de más prioridad");
    assert_eq!(e.elegir_siguiente(), Some(alta));
}

#[test]
fn forbid_a_mano_como_las_macros() {
    // exec/ables.i sube TDNestCnt con un ADDQ.B: exec lo tiene que ver.
    let (mut e, t) = caja_con_tarea();
    e.crear_tarea("alta", 9, 1024, 0x1000).unwrap();
    let b = e.base();
    e.mem.escribir8(b + execbase::TDNESTCNT, 0).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    e.mem.escribir8(b + execbase::TDNESTCNT, 0xFF).unwrap();
    assert_ne!(e.elegir_siguiente(), Some(t));
}

#[test]
fn wait_dentro_de_forbid_rompe_y_recupera_el_forbid() {
    let (mut e, t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    e.forbid().unwrap();
    let s = e.alloc_signal(-1).unwrap();
    e.wait(1 << s).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(otra));
    assert!(!e.prohibido(), "la otra corre sin el Forbid");
    e.signal(t, 1 << s).unwrap();
    assert!(e.fin_de_turno());
    assert_eq!(e.elegir_siguiente(), Some(t));
    assert!(e.prohibido(), "vuelve con su Forbid");
    assert_eq!(e.permit().unwrap(), Accion::Seguir);
}

#[test]
fn disable_para_las_interrupciones_de_la_caja() {
    let (mut e, _t) = caja_con_tarea();
    assert!(e.interrupciones_activas());
    e.disable().unwrap();
    assert!(!e.interrupciones_activas());
    assert!(e.prohibido());
    e.enable().unwrap();
    assert!(e.interrupciones_activas());
}

#[test]
fn set_task_pri() {
    let (mut e, t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.set_task_pri(t, -10).unwrap(), (0, Accion::Ceder));
    assert_eq!(e.elegir_siguiente(), Some(otra));
    assert_eq!(e.set_task_pri(t, 50).unwrap(), (-10, Accion::Ceder));
    assert_eq!(e.elegir_siguiente(), Some(t));
    let mut r = Registros::default();
    r.a[1] = otra;
    r.d[0] = 0xFFFF_FFFB; // -5
    llamar(&mut e, "SetTaskPri", &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(l8(&e, otra + node::LN_PRI) as i8, -5);
}

#[test]
fn find_task() {
    let (mut e, t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.find_task(0).unwrap(), t);
    let n = cadena(&mut e, "otra");
    assert_eq!(e.find_task(n).unwrap(), otra);
    let n = cadena(&mut e, "principal");
    assert_eq!(e.find_task(n).unwrap(), t);
    let n = cadena(&mut e, "nadie");
    assert_eq!(e.find_task(n).unwrap(), 0);
}

#[test]
fn set_signal_y_alloc_signal() {
    let (mut e, t) = caja_con_tarea();
    assert_eq!(e.set_signal(0x0001_0000, 0x0001_0000).unwrap(), 0);
    assert_eq!(e.set_signal(0, 0x0001_0000).unwrap(), 0x0001_0000);
    assert_eq!(e.alloc_signal(20).unwrap(), 20);
    assert_eq!(e.alloc_signal(20).unwrap(), -1, "ya reservada");
    assert_eq!(e.alloc_signal(3).unwrap(), -1, "las del sistema ya están");
    assert_eq!(e.alloc_signal(40).unwrap(), -1);
    let mut r = Registros::default();
    r.d[0] = 0xFF; // -1 en un BYTE
    llamar(&mut e, "AllocSignal", &mut r);
    assert_eq!(r.d[0], 31);
    e.free_signal(31).unwrap();
    e.free_signal(-1).unwrap();
    // Se reservan todas las de arriba y ya no queda ninguna.
    for _ in 0..15 {
        assert!(e.alloc_signal(-1).unwrap() >= 16);
    }
    assert_eq!(e.alloc_signal(-1).unwrap(), -1);
    assert_eq!(l32(&e, t + task::TC_SIGALLOC), 0xFFFF_FFFF);
}

#[test]
fn una_tarea_que_acaba_vuelve_al_codigo_de_fin() {
    let (mut e, t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    let antes = e.avail_mem(memf::ANY).unwrap();
    // La función de la tarea hace RTS: va al código de fin, que es la llamada nativa 200.
    let mut r = Registros::default();
    assert_eq!(e.llamar(caja_exec::lvo::FIN_DE_TAREA, &mut r), Accion::Terminar);
    assert_eq!(e.tomar_quitadas(), vec![t]);
    assert!(e.avail_mem(memf::ANY).unwrap() > antes, "se suelta su tc_MemEntry");
    assert_eq!(e.elegir_siguiente(), Some(otra));
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

#[test]
fn rem_task_de_otra() {
    let (mut e, _t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.rem_task(otra).unwrap(), Accion::Seguir);
    assert!(e.nodos(e.base() + execbase::TASKREADY).unwrap().is_empty());
    assert_eq!(e.tomar_quitadas(), vec![otra]);
    assert_eq!(e.rem_task(otra), Err(Fallo::TareaDesconocida { dir: otra }));
}

#[test]
fn matar_una_tarea_suelta_lo_suyo() {
    let (mut e, t) = caja_con_tarea();
    let antes = e.avail_mem(memf::ANY).unwrap();
    let otra = e.crear_tarea("otra", 0, 2048, 0x1000).unwrap();
    // La otra corre, reserva memoria y monta un puerto público.
    assert!(e.fin_de_turno());
    assert_eq!(e.elegir_siguiente(), Some(otra));
    let a = e.alloc_mem(1000, 0).unwrap();
    let p = e.create_msg_port().unwrap();
    let nombre = cadena(&mut e, "PUERTO");
    e.mem.escribir32(p + node::LN_NAME, nombre).unwrap();
    e.add_port(p).unwrap();
    e.forbid().unwrap();
    assert!(!e.fin_de_turno());
    assert_eq!(e.matar_tarea(0).unwrap(), Accion::Terminar);
    assert!(!e.prohibido(), "su Forbid se va con ella");
    assert_eq!(e.bloque(a), None);
    assert!(e.nodos(e.base() + execbase::PORTLIST).unwrap().is_empty(), "su puerto sale de la lista");
    assert_eq!(e.elegir_siguiente(), Some(t));
    assert_eq!(e.avail_mem(memf::ANY).unwrap(), antes);
    assert!(e.bloques_de(otra).is_empty());
}

// --- Hostiles ---

#[test]
fn senal_a_una_tarea_que_no_existe() {
    let (mut e, _t) = caja_con_tarea();
    assert_eq!(e.signal(0, 1), Err(Fallo::PunteroNulo));
    assert_eq!(e.signal(0x30_0000, 1), Err(Fallo::TareaDesconocida { dir: 0x30_0000 }));
    let mut r = Registros::default();
    r.a[1] = 0xDEAD_BEEF;
    r.d[0] = 1;
    assert_eq!(llamar(&mut e, "Signal", &mut r), Accion::Seguir);
    assert_eq!(avisos_de(&e, "Signal"), 1);
}

#[test]
fn free_signal_no_reservada() {
    let (mut e, _t) = caja_con_tarea();
    assert_eq!(e.free_signal(25), Err(Fallo::SenalNoReservada { n: 25 }));
    assert_eq!(e.free_signal(32), Err(Fallo::SenalMala { n: 32 }));
    assert_eq!(e.free_signal(-7), Err(Fallo::SenalMala { n: -7 }));
}

#[test]
fn esperar_una_senal_no_reservada_avisa() {
    let (mut e, _t) = caja_con_tarea();
    e.wait(1 << 25).unwrap();
    assert_eq!(avisos_de(&e, "Wait"), 1);
    // Wait(0) duerme para siempre, como en exec, y avisa.
    let (mut e, _t) = caja_con_tarea();
    assert_eq!(e.wait(0).unwrap(), caja_exec::Espera::Bloquea);
    assert_eq!(avisos_de(&e, "Wait"), 1);
}

#[test]
fn sin_tarea_en_marcha() {
    let mut e = caja();
    assert_eq!(e.wait(1), Err(Fallo::SinTarea));
    assert_eq!(e.alloc_signal(-1), Err(Fallo::SinTarea));
    assert_eq!(e.set_signal(0, 0), Err(Fallo::SinTarea));
    assert_eq!(e.create_msg_port(), Err(Fallo::SinTarea));
    assert_eq!(e.rem_task(0), Err(Fallo::SinTarea));
    assert_eq!(e.find_task(0).unwrap(), 0);
}

#[test]
fn add_task_hostil() {
    let (mut e, t) = caja_con_tarea();
    assert_eq!(e.add_task(0, 0x1000, 0), Err(Fallo::PunteroNulo));
    assert_eq!(e.add_task(t, 0x1000, 0), Err(Fallo::TareaRepetida { dir: t }));
    let sin_pila = e.alloc_mem(task::TAMANO, memf::CLEAR).unwrap();
    assert_eq!(e.add_task(sin_pila, 0x1000, 0), Err(Fallo::PilaMala { dir: sin_pila }));
    assert!(e.add_task(TAMANO - 10, 0x1000, 0).is_err());
    assert!(e.add_task(0x200, 0x1000, 0).is_err());
    // Pila fuera de sus límites.
    e.mem.escribir32(sin_pila + task::TC_SPLOWER, 0x30_0000).unwrap();
    e.mem.escribir32(sin_pila + task::TC_SPUPPER, 0x30_0100).unwrap();
    e.mem.escribir32(sin_pila + task::TC_SPREG, 0x30_0200).unwrap();
    assert_eq!(e.add_task(sin_pila, 0x1000, 0), Err(Fallo::PilaMala { dir: sin_pila }));
}

#[test]
fn permit_y_enable_de_mas() {
    let (mut e, _t) = caja_con_tarea();
    assert_eq!(e.permit(), Err(Fallo::SinPareja));
    assert_eq!(e.enable(), Err(Fallo::SinPareja));
    assert!(!e.prohibido());
    let mut r = Registros::default();
    assert_eq!(llamar(&mut e, "Permit", &mut r), Accion::Seguir);
    assert_eq!(avisos_de(&e, "Permit"), 1);
    // Y Forbid sin fin no da la vuelta.
    for _ in 0..128 {
        e.forbid().unwrap();
    }
    assert_eq!(e.forbid(), Err(Fallo::SinPareja));
    assert!(e.prohibido());
}

#[test]
fn una_lista_de_tareas_estropeada_no_tumba_el_planificador() {
    let (mut e, _t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    let b = e.base();
    // Un programa escribe basura en TaskReady.
    e.mem.escribir32(b + execbase::TASKREADY, 0x0BAD_0000).unwrap();
    let _ = otra;
    assert!(!e.fin_de_turno());
    assert_eq!(e.elegir_siguiente(), Some(_t), "la que corría sigue");
    assert_eq!(avisos_de(&e, "(planificador)"), 1);
}
