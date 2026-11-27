//! Varias tareas en un solo hilo (`cambio.rs`): guardar y retomar contextos, el trampolín de repetir, quién creó a
//! quién, lo que pasa a la creadora cuando una tarea acaba, aparcar y los casos hostiles.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Accion, Contexto, Fallo, Memoria, Modo, Registros, lvo};
use comun::*;

fn estado(e: &caja_exec::Exec<caja_exec::MemoriaVec>, t: u32) -> u8 {
    l8(e, t + task::TC_STATE)
}

/// Los registros con que se para una tarea en una llamada: su pila es la suya, con la vuelta encima.
fn regs_de(e: &mut caja_exec::Exec<caja_exec::MemoriaVec>, t: u32, d0: u32) -> Registros {
    let mut r = Registros::default();
    let sp = l32(e, t + task::TC_SPUPPER) - 64;
    e.mem.escribir32(sp, 0x0012_3456).unwrap();
    r.a[7] = sp;
    r.d[0] = d0;
    r
}

#[test]
fn una_tarea_nueva_empieza_en_su_pc() {
    let mut e = caja();
    let t = e.crear_tarea("nueva", 0, 4096, 0x0012_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let (_, sp) = e.arranque(t).unwrap();
    let ctx = e.reanudar(t).unwrap();
    // El RTS de la llamada nativa en curso saca su PC; debajo, la vuelta al código de fin de tarea.
    assert_eq!(ctx.regs.a[7], sp - 4);
    assert_eq!(l32(&e, sp - 4), 0x0012_0000);
    assert_eq!(l32(&e, sp), l32(&e, e.base() + execbase::TASKEXITCODE));
    assert_eq!(ctx.regs.d, [0; 8]);
    assert_eq!(ctx.fp, [caja_exec::cambio::FP_INICIAL; 8]);
    assert_eq!(e.tomar_repeticion(t), None);
}

#[test]
fn volver_no_toca_la_pila() {
    let (mut e, t) = caja_con_tarea();
    let r = regs_de(&mut e, t, 77);
    let mut ctx = Contexto::nuevo(r, Modo::Volver);
    ctx.fp[3] = 0x4009_21fb_5444_2d18;
    ctx.fpcr = 0x10;
    e.guardar_contexto(t, ctx).unwrap();
    assert_eq!(e.contexto(t), Some(ctx));
    let antes = l32(&e, r.a[7] - 4);
    let v = e.reanudar(t).unwrap();
    assert_eq!(v.regs, r);
    assert_eq!(v.fp[3], 0x4009_21fb_5444_2d18);
    assert_eq!(v.fpcr, 0x10);
    assert_eq!(l32(&e, r.a[7] - 4), antes);
    assert_eq!(e.contexto(t), None, "el contexto se gasta");
    // Corrió y no se guardó: no se puede retomar (no vuelve a empezar en su PC).
    assert_eq!(e.reanudar(t), Err(Fallo::PilaMala { dir: t }));
}

#[test]
fn repetir_deja_el_trampolin_y_la_llamada() {
    let (mut e, t) = caja_con_tarea();
    let tr = e.trampolin_repetir();
    assert_ne!(tr, 0);
    // El trampolín es "$A0FF REPETIR RTS".
    assert_eq!(e.mem.leer16(tr).unwrap(), 0xA0FF);
    assert_eq!(e.mem.leer16(tr + 2).unwrap(), lvo::REPETIR);
    assert_eq!(e.mem.leer16(tr + 4).unwrap(), 0x4E75);
    let r = regs_de(&mut e, t, 1 << 12);
    e.guardar_contexto(t, Contexto::nuevo(r, Modo::Repetir(53))).unwrap();
    let v = e.reanudar(t).unwrap();
    assert_eq!(v.regs.a[7], r.a[7] - 4);
    assert_eq!(l32(&e, r.a[7] - 4), tr);
    assert_eq!(v.regs.d[0], 1 << 12);
    assert_eq!(e.tomar_repeticion(t), Some(53));
    assert_eq!(e.tomar_repeticion(t), None, "se gasta");
}

/// Dos tareas que se esperan la una a la otra, llevadas como lo hace la caja: Wait bloquea, se guarda con
/// `Repetir`, se elige la otra y se retoma; la repetición de Wait da la señal.
#[test]
fn ping_pong_con_wait_y_signal() {
    let mut e = caja();
    let a = e.crear_tarea("a", 0, 4096, 0x0012_0000).unwrap();
    let b = e.crear_tarea("b", 0, 4096, 0x0013_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(a));
    e.reanudar(a).unwrap();
    let sa = 1u32 << e.alloc_signal(-1).unwrap();
    // A espera su señal: se bloquea.
    let mut r = regs_de(&mut e, a, sa);
    let antes = r;
    assert_eq!(llamar(&mut e, "Wait", &mut r), Accion::Bloquear);
    e.guardar_contexto(a, Contexto::nuevo(antes, Modo::Repetir(lvo::por_nombre("Wait").unwrap()))).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(b));
    let vb = e.reanudar(b).unwrap();
    assert_eq!(l32(&e, vb.regs.a[7]), 0x0013_0000, "B es nueva");
    let sb = 1u32 << e.alloc_signal(-1).unwrap();
    // B señala a A (misma prioridad: no cede) y espera la suya.
    let mut r = Registros::default();
    r.a[1] = a;
    r.d[0] = sa;
    assert_eq!(llamar(&mut e, "Signal", &mut r), Accion::Seguir);
    assert_eq!(estado(&e, a), ts::READY);
    let mut r = regs_de(&mut e, b, sb);
    let antes_b = r;
    assert_eq!(llamar(&mut e, "Wait", &mut r), Accion::Bloquear);
    e.guardar_contexto(b, Contexto::nuevo(antes_b, Modo::Repetir(53))).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(a));
    let va = e.reanudar(a).unwrap();
    assert_eq!(e.tomar_repeticion(a), Some(53));
    // La caja repite la llamada con los registros de entonces (A7 ya sin el trampolín, como tras el RTS).
    let mut r = va.regs;
    r.a[7] += 4;
    assert_eq!(r, antes);
    assert_eq!(llamar(&mut e, "Wait", &mut r), Accion::Seguir);
    assert_eq!(r.d[0], sa);
    // Y al revés.
    let mut r = Registros::default();
    r.a[1] = b;
    r.d[0] = sb;
    assert_eq!(llamar(&mut e, "Signal", &mut r), Accion::Seguir);
    assert_eq!(estado(&e, b), ts::READY);
}

#[test]
fn una_tarea_que_se_senala_a_si_misma_no_espera() {
    let (mut e, t) = caja_con_tarea();
    let s = 1u32 << e.alloc_signal(-1).unwrap();
    let mut r = Registros::default();
    r.a[1] = t;
    r.d[0] = s;
    assert_eq!(llamar(&mut e, "Signal", &mut r), Accion::Seguir);
    let mut r = Registros::default();
    r.d[0] = s;
    assert_eq!(llamar(&mut e, "Wait", &mut r), Accion::Seguir);
    assert_eq!(r.d[0], s);
    assert_eq!(estado(&e, t), ts::RUN);
}

#[test]
fn quien_crea_a_quien() {
    let (mut e, p) = caja_con_tarea();
    assert_eq!(e.creadora(p), 0, "la de la prueba no es de nadie");
    // La principal crea h1; h1 crea n1; la principal crea h2.
    let h1 = e.crear_tarea("h1", 0, 1024, 0x1000).unwrap();
    let h2 = e.crear_tarea("h2", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.creadora(h1), p);
    e.aparcar(p).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(h1));
    let n1 = e.crear_tarea("n1", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.creadora(n1), h1);
    let mut d = e.descendientes(p);
    assert_eq!(d.iter().position(|&x| x == n1) < d.iter().position(|&x| x == h1), true, "las de abajo primero");
    d.sort();
    let mut esperado = vec![h1, h2, n1];
    esperado.sort();
    assert_eq!(d, esperado);
    assert_eq!(e.descendientes(h2), vec![]);
    // Lo que crea código de una biblioteca de disco es del sistema.
    e.memoria_del_sistema(true);
    let s = e.crear_tarea("sistema", 0, 1024, 0x1000).unwrap();
    e.memoria_del_sistema(false);
    assert_eq!(e.creadora(s), 0);
    assert!(!e.descendientes(h1).contains(&s));
}

#[test]
fn lo_de_una_hija_que_acaba_pasa_a_su_creadora() {
    let (mut e, p) = caja_con_tarea();
    let h = e.crear_tarea("hija", 0, 1024, 0x1000).unwrap();
    e.aparcar(p).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(h));
    // La hija reserva memoria y crea una nieta.
    let m = e.alloc_mem(100, 0).unwrap();
    assert_eq!(e.bloque(m).unwrap().dueno, h);
    let n = e.crear_tarea("nieta", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.creadora(n), h);
    let pila = l32(&e, h + task::TC_SPLOWER);
    // Acaba (vuelve de su función: RemTask(0)).
    assert_eq!(e.acabar_hija(0).unwrap(), Accion::Terminar);
    assert!(!e.tareas().contains(&h));
    // Su estructura y su pila (su tc_MemEntry) se sueltan; lo demás pasa a la principal.
    assert!(e.bloque(h).is_none());
    assert!(e.bloque(pila).is_none());
    assert_eq!(e.bloque(m).unwrap().dueno, p);
    assert_eq!(e.creadora(n), p);
    // Matar la principal (el programa acaba) suelta lo que heredó.
    for d in e.descendientes(p) {
        e.matar_tarea(d).unwrap();
    }
    e.matar_tarea(p).unwrap();
    assert!(e.bloque(m).is_none());
    assert!(e.tareas().is_empty());
}

#[test]
fn una_hija_del_sistema_deja_lo_suyo_en_memoria() {
    let (mut e, _p) = caja_con_tarea();
    e.memoria_del_sistema(true);
    let s = e.crear_tarea("sistema", 0, 1024, 0x1000).unwrap();
    e.memoria_del_sistema(false);
    e.desaparcar(s).unwrap();
    let m = e.alloc_mem(64, 0).unwrap();
    e.cambiar_dueno(m, s);
    assert_eq!(e.acabar_hija(s).unwrap(), Accion::Seguir, "no era la actual");
    assert_eq!(e.bloque(m).unwrap().dueno, 0);
}

#[test]
fn poner_en_marcha_sin_mirar_prioridades() {
    let mut e = caja();
    let alta = e.crear_tarea("alta", 20, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(alta));
    let prog = e.crear_tarea("programa", 0, 1024, 0x1000).unwrap();
    e.poner_en_marcha(prog).unwrap();
    assert_eq!(e.tarea_actual(), prog);
    assert_eq!(estado(&e, prog), ts::RUN);
    assert_eq!(estado(&e, alta), ts::READY);
    assert_eq!(e.nodos(e.base() + execbase::TASKREADY).unwrap(), vec![alta]);
    // Entre programas, nadie en marcha.
    e.sin_tarea_en_marcha();
    assert_eq!(e.tarea_actual(), 0);
    let mut listas = e.nodos(e.base() + execbase::TASKREADY).unwrap();
    listas.sort();
    let mut esperado = vec![alta, prog];
    esperado.sort();
    assert_eq!(listas, esperado);
}

#[test]
fn aparcar_y_desaparcar() {
    let (mut e, t) = caja_con_tarea();
    let otra = e.crear_tarea("otra", -1, 1024, 0x1000).unwrap();
    e.aparcar(t).unwrap();
    assert!(e.aparcada(t));
    assert_eq!(estado(&e, t), ts::WAIT);
    // Ninguna señal la despierta.
    e.signal(t, 0xFFFF_FFFF).unwrap();
    assert_eq!(estado(&e, t), ts::WAIT);
    assert_eq!(e.elegir_siguiente(), Some(otra));
    e.desaparcar(t).unwrap();
    assert!(!e.aparcada(t));
    assert_eq!(estado(&e, t), ts::READY);
    // Desaparcar una que no lo está no hace nada.
    e.desaparcar(otra).unwrap();
    assert_eq!(estado(&e, otra), ts::RUN);
}

// --- Hostiles ---

#[test]
fn contextos_con_pilas_malas() {
    let (mut e, t) = caja_con_tarea();
    for a7 in [0u32, 4, 0xFFFF_FFF0, 0x7000_0000, (4 << 20) + 0x100] {
        let mut r = Registros::default();
        r.a[7] = a7;
        assert!(e.guardar_contexto(t, Contexto::nuevo(r, Modo::Volver)).is_err(), "A7 = {:#x}", a7);
    }
    // Una pila que deja de valer entre guardar y retomar (la tarea la soltó o la estropeó otra): PilaMala.
    let p = e.alloc_mem(256, 0).unwrap();
    let mut r = Registros::default();
    r.a[7] = p + 4;
    e.guardar_contexto(t, Contexto::nuevo(r, Modo::Repetir(53))).unwrap();
    let mut r = Registros::default();
    r.a[7] = 2;
    // Saltar a 0 no vale.
    let otra = e.crear_tarea("otra", 0, 1024, 0x1000).unwrap();
    e.mem.escribir32(otra + task::TC_SPREG, 0).unwrap();
    let ro = regs_de(&mut e, otra, 0);
    e.guardar_contexto(otra, Contexto::nuevo(ro, Modo::Saltar(0))).unwrap();
    assert_eq!(e.reanudar(otra), Err(Fallo::PilaMala { dir: otra }));
    // Tareas que no existen.
    assert_eq!(e.reanudar(0x1234), Err(Fallo::TareaDesconocida { dir: 0x1234 }));
    assert_eq!(e.reanudar(0), Err(Fallo::PunteroNulo));
    assert!(e.guardar_contexto(0x5000, Contexto::nuevo(r, Modo::Volver)).is_err());
    assert!(e.acabar_hija(0x5000).is_err());
    assert!(e.aparcar(0x5000).is_err());
    assert!(e.poner_en_marcha(0x5000).is_err());
}

#[test]
fn creadoras_en_circulo_no_cuelgan() {
    let (mut e, p) = caja_con_tarea();
    let a = e.crear_tarea("a", 0, 1024, 0x1000).unwrap();
    let b = e.crear_tarea("b", 0, 1024, 0x1000).unwrap();
    e.poner_creadora(a, b);
    e.poner_creadora(b, a);
    let d = e.descendientes(a);
    assert_eq!(d, vec![b]);
    assert!(e.descendientes(p).is_empty());
    // Acabar una de las dos deja a la otra con una creadora que ya no existe: es del sistema.
    e.acabar_hija(a).unwrap();
    assert_eq!(e.creadora(b), 0);
}

/// Miles de tareas hasta que no cabe ninguna más: cada una con su estructura y su pila; al acabar todas, la
/// memoria vuelve.
#[test]
fn miles_de_tareas_hasta_el_tope() {
    let (mut e, p) = caja_con_tarea();
    let libre = e.avail_mem(0).unwrap();
    let mut hechas = Vec::new();
    loop {
        let t = e.crear_tarea("h", 0, 1024, 0x1000).unwrap();
        if t == 0 {
            break;
        }
        hechas.push(t);
        assert!(hechas.len() < 100_000);
    }
    assert!(hechas.len() > 2000, "{} tareas en 4 MB", hechas.len());
    assert_eq!(e.listas(), hechas.len());
    assert_eq!(e.descendientes(p).len(), hechas.len());
    for t in hechas {
        e.acabar_hija(t).unwrap();
    }
    assert_eq!(e.listas(), 0);
    assert_eq!(e.avail_mem(0).unwrap(), libre);
}

/// Una tarea mata a otra que espera: sale de las listas, se suelta su tc_MemEntry y su contexto se olvida.
#[test]
fn una_tarea_tumba_a_otra() {
    let (mut e, p) = caja_con_tarea();
    let v = e.crear_tarea("victima", 0, 1024, 0x1000).unwrap();
    e.aparcar(p).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(v));
    let r = regs_de(&mut e, v, 1 << 20);
    let mut rr = r;
    assert_eq!(llamar(&mut e, "Wait", &mut rr), Accion::Bloquear);
    e.guardar_contexto(v, Contexto::nuevo(r, Modo::Repetir(53))).unwrap();
    e.desaparcar(p).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(p));
    // RemTask(víctima) desde la principal.
    let mut r = Registros::default();
    r.a[1] = v;
    assert_eq!(llamar(&mut e, "RemTask", &mut r), Accion::Seguir);
    assert!(!e.tareas().contains(&v));
    assert!(e.esperando().is_empty());
    assert_eq!(e.contexto(v), None);
    assert!(e.reanudar(v).is_err());
    // Señalarla ya no hace nada malo.
    assert!(e.signal(v, 1).is_err());
}
