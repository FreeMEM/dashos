//! Puertos y mensajes.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Accion, Espera, Fallo, Memoria, Registros};
use comun::*;

fn mensaje(e: &mut caja_exec::Exec<caja_exec::MemoriaVec>, respuesta: u32) -> u32 {
    let m = e.alloc_mem(message::TAMANO + 8, memf::PUBLIC | memf::CLEAR).unwrap();
    e.mem.escribir32(m + message::MN_REPLYPORT, respuesta).unwrap();
    e.mem.escribir16(m + message::MN_LENGTH, (message::TAMANO + 8) as u16).unwrap();
    m
}

#[test]
fn create_msg_port() {
    let (mut e, t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    assert_ne!(p, 0);
    assert_eq!(l8(&e, p + node::LN_TYPE), nt::MSGPORT);
    assert_eq!(l8(&e, p + msgport::MP_FLAGS), msgport::PA_SIGNAL);
    assert_eq!(l8(&e, p + msgport::MP_SIGBIT), 31);
    assert_eq!(l32(&e, p + msgport::MP_SIGTASK), t);
    assert!(e.lista_vacia(p + msgport::MP_MSGLIST).unwrap());
    assert_eq!(l32(&e, t + task::TC_SIGALLOC) & (1 << 31), 1 << 31);
    e.delete_msg_port(p).unwrap();
    assert_eq!(l32(&e, t + task::TC_SIGALLOC) & (1 << 31), 0, "suelta la señal");
    assert_eq!(e.bloque(p), None);
}

#[test]
fn ida_y_vuelta_entre_dos_tareas() {
    let (mut e, cliente) = caja_con_tarea();
    let respuesta = e.create_msg_port().unwrap();
    let servidor = e.crear_tarea("servidor", 0, 2048, 0x1000).unwrap();
    // El servidor corre, crea su puerto y espera.
    assert!(e.fin_de_turno());
    assert_eq!(e.elegir_siguiente(), Some(servidor));
    let p = e.create_msg_port().unwrap();
    assert_eq!(e.wait_port(p).unwrap(), Espera::Bloquea);
    assert_eq!(e.elegir_siguiente(), Some(cliente));
    // El cliente manda un mensaje: el servidor despierta (misma prioridad, no cede).
    let m = mensaje(&mut e, respuesta);
    assert_eq!(e.put_msg(p, m).unwrap(), Accion::Seguir);
    assert_eq!(l8(&e, m + node::LN_TYPE), nt::MESSAGE);
    assert_eq!(e.wait_port(respuesta).unwrap(), Espera::Bloquea);
    assert_eq!(e.elegir_siguiente(), Some(servidor));
    // La repetición de WaitPort da el mensaje (sin sacarlo) y gasta la señal.
    assert_eq!(e.wait_port(p).unwrap(), Espera::Hecho(m));
    let bit = 1 << l8(&e, p + msgport::MP_SIGBIT);
    assert_eq!(l32(&e, servidor + task::TC_SIGRECVD) & bit, 0);
    assert_eq!(e.get_msg(p).unwrap(), m);
    assert_eq!(e.get_msg(p).unwrap(), 0);
    e.reply_msg(m).unwrap();
    assert_eq!(l8(&e, m + node::LN_TYPE), nt::REPLYMSG);
    assert_eq!(e.elegir_siguiente(), Some(servidor), "sigue el servidor hasta que espere o acabe su turno");
    assert!(e.fin_de_turno());
    assert_eq!(e.elegir_siguiente(), Some(cliente));
    assert_eq!(e.wait_port(respuesta).unwrap(), Espera::Hecho(m));
    assert_eq!(e.get_msg(respuesta).unwrap(), m);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

#[test]
fn wait_port_con_mensaje_ya_en_cola_no_espera() {
    let (mut e, t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    let m = mensaje(&mut e, 0);
    e.put_msg(p, m).unwrap();
    let bit = 1 << l8(&e, p + msgport::MP_SIGBIT);
    assert_eq!(l32(&e, t + task::TC_SIGRECVD) & bit, bit);
    assert_eq!(e.wait_port(p).unwrap(), Espera::Hecho(m));
    // Como en exec, la señal sigue puesta: el siguiente Wait volverá enseguida.
    assert_eq!(l32(&e, t + task::TC_SIGRECVD) & bit, bit);
}

#[test]
fn reply_sin_puerto_marca_el_mensaje_libre() {
    let (mut e, _t) = caja_con_tarea();
    let m = mensaje(&mut e, 0);
    assert_eq!(e.reply_msg(m).unwrap(), Accion::Seguir);
    assert_eq!(l8(&e, m + node::LN_TYPE), nt::FREEMSG);
}

#[test]
fn puertos_publicos() {
    let (mut e, _t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    let n = cadena(&mut e, "REXX");
    e.mem.escribir32(p + node::LN_NAME, n).unwrap();
    e.add_port(p).unwrap();
    let mut r = Registros::default();
    r.a[1] = cadena(&mut e, "REXX");
    llamar(&mut e, "FindPort", &mut r);
    assert_eq!(r.d[0], p);
    r.a[1] = p;
    llamar(&mut e, "RemPort", &mut r);
    r.a[1] = n;
    llamar(&mut e, "FindPort", &mut r);
    assert_eq!(r.d[0], 0);
}

#[test]
fn pa_ignore_no_despierta_a_nadie() {
    let (mut e, t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    e.mem.escribir8(p + msgport::MP_FLAGS, msgport::PA_IGNORE).unwrap();
    let m = mensaje(&mut e, 0);
    e.put_msg(p, m).unwrap();
    assert_eq!(l32(&e, t + task::TC_SIGRECVD), 0);
    assert_eq!(e.get_msg(p).unwrap(), m);
}

#[test]
fn por_la_tabla_de_llamadas() {
    let (mut e, _t) = caja_con_tarea();
    let mut r = Registros::default();
    llamar(&mut e, "CreateMsgPort", &mut r);
    let p = r.d[0];
    assert_ne!(p, 0);
    let m = mensaje(&mut e, p);
    r.a[0] = p;
    r.a[1] = m;
    assert_eq!(llamar(&mut e, "PutMsg", &mut r), Accion::Seguir);
    r.a[0] = p;
    assert_eq!(llamar(&mut e, "WaitPort", &mut r), Accion::Seguir);
    assert_eq!(r.d[0], m);
    llamar(&mut e, "GetMsg", &mut r);
    assert_eq!(r.d[0], m);
    r.a[1] = m;
    llamar(&mut e, "ReplyMsg", &mut r);
    r.a[0] = p;
    llamar(&mut e, "GetMsg", &mut r);
    assert_eq!(r.d[0], m, "vuelve a su puerto de respuesta");
    r.a[0] = p;
    llamar(&mut e, "DeleteMsgPort", &mut r);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

// --- Hostiles ---

#[test]
fn mensaje_a_un_puerto_borrado() {
    let (mut e, _t) = caja_con_tarea();
    let m = mensaje(&mut e, 0);
    let p = e.create_msg_port().unwrap();
    e.delete_msg_port(p).unwrap();
    assert_eq!(e.put_msg(p, m), Err(Fallo::PuertoMuerto { dir: p }));
    assert_eq!(e.get_msg(p), Err(Fallo::PuertoMuerto { dir: p }));
    assert_eq!(e.delete_msg_port(p), Err(Fallo::NoReservado { dir: p }));
    let mut r = Registros::default();
    r.a[0] = p;
    r.a[1] = m;
    assert_eq!(llamar(&mut e, "PutMsg", &mut r), Accion::Seguir);
    assert_eq!(avisos_de(&e, "PutMsg"), 1);
}

#[test]
fn respuesta_a_un_puerto_borrado() {
    let (mut e, _t) = caja_con_tarea();
    let rp = e.create_msg_port().unwrap();
    let p = e.create_msg_port().unwrap();
    let m = mensaje(&mut e, rp);
    e.put_msg(p, m).unwrap();
    e.get_msg(p).unwrap();
    e.delete_msg_port(rp).unwrap();
    assert_eq!(e.reply_msg(m), Err(Fallo::PuertoMuerto { dir: rp }));
}

#[test]
fn el_mismo_mensaje_dos_veces() {
    let (mut e, _t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    let q = e.create_msg_port().unwrap();
    let m = mensaje(&mut e, 0);
    e.put_msg(p, m).unwrap();
    assert_eq!(e.put_msg(q, m), Err(Fallo::MensajeEnCola { dir: m }));
    assert_eq!(e.put_msg(p, m), Err(Fallo::MensajeEnCola { dir: m }));
    assert_eq!(e.nodos(p + msgport::MP_MSGLIST).unwrap(), vec![m]);
    // Una vez sacado, se puede volver a mandar.
    e.get_msg(p).unwrap();
    e.put_msg(q, m).unwrap();
}

#[test]
fn mensajes_y_puertos_en_sitios_malos() {
    let (mut e, _t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    assert_eq!(e.put_msg(p, 0), Err(Fallo::PunteroNulo));
    assert_eq!(e.put_msg(0, 0x30_0000), Err(Fallo::PunteroNulo));
    assert_eq!(e.put_msg(p, 0x30_0000), Err(Fallo::MensajeMuerto { dir: 0x30_0000 }), "memoria libre");
    assert_eq!(e.put_msg(p, 0x8000_0000), Err(Fallo::MensajeMuerto { dir: 0x8000_0000 }));
    assert_eq!(e.put_msg(0x40, 0x30_0000), Err(Fallo::PuertoMuerto { dir: 0x40 }));
    assert_eq!(e.reply_msg(0), Err(Fallo::PunteroNulo));
    assert_eq!(e.wait_port(0xFFFF_FFF0), Err(Fallo::PuertoMuerto { dir: 0xFFFF_FFF0 }));
    assert!(e.lista_vacia(p + msgport::MP_MSGLIST).unwrap());
}

#[test]
fn puerto_sin_tipo_o_de_otra_tarea_avisa() {
    let (mut e, _t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    e.mem.escribir8(p + node::LN_TYPE, 0).unwrap();
    let m = mensaje(&mut e, 0);
    e.put_msg(p, m).unwrap();
    assert_eq!(avisos_de(&e, "PutMsg"), 1);
    // Su tarea ya no existe: el mensaje queda en la cola y se avisa.
    let p2 = e.create_msg_port().unwrap();
    e.mem.escribir32(p2 + msgport::MP_SIGTASK, 0x30_0000).unwrap();
    let m2 = mensaje(&mut e, 0);
    e.put_msg(p2, m2).unwrap();
    assert_eq!(avisos_de(&e, "PutMsg"), 2);
    assert_eq!(e.get_msg(p2).unwrap(), m2);
}

#[test]
fn delete_msg_port_aun_en_la_lista_publica() {
    let (mut e, _t) = caja_con_tarea();
    let p = e.create_msg_port().unwrap();
    e.add_port(p).unwrap();
    e.delete_msg_port(p).unwrap();
    assert!(e.nodos(e.base() + execbase::PORTLIST).unwrap().is_empty(), "no deja la lista rota");
    assert_eq!(avisos_de(&e, "DeleteMsgPort"), 1);
    // Borrar algo que no es un puerto.
    let x = e.alloc_mem(64, memf::CLEAR).unwrap();
    assert_eq!(e.delete_msg_port(x), Err(Fallo::Sospechoso { dir: x }));
    e.delete_msg_port(0).unwrap();
}

#[test]
fn sin_senales_libres_no_hay_puerto() {
    let (mut e, _t) = caja_con_tarea();
    while e.alloc_signal(-1).unwrap() != -1 {}
    let antes = e.avail_mem(memf::ANY).unwrap();
    assert_eq!(e.create_msg_port().unwrap(), 0);
    assert_eq!(e.avail_mem(memf::ANY).unwrap(), antes);
}
