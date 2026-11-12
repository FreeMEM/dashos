//! Charcos de memoria (CreatePool y compañía), semáforos, SetExcept y StackSwap: lo que pide el runtime de Dash.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Accion, Fallo, Memoria, Registros};
use comun::*;

fn libre(e: &caja_exec::Exec<caja_exec::MemoriaVec>) -> u32 {
    e.avail_mem(memf::ANY).unwrap()
}

/// Por los registros, como el JIT.
fn pool_llamar(e: &mut caja_exec::Exec<caja_exec::MemoriaVec>, f: &str, d: [u32; 3], a: [u32; 2]) -> u32 {
    let mut r = Registros::default();
    r.d[..3].copy_from_slice(&d);
    r.a[..2].copy_from_slice(&a);
    assert_eq!(llamar(e, f, &mut r), Accion::Seguir);
    r.d[0]
}

#[test]
fn charco_reparte_y_suelta() {
    let (mut e, t) = caja_con_tarea();
    let antes = libre(&e);
    let p = pool_llamar(&mut e, "CreatePool", [memf::ANY, 4096, 1024], [0, 0]);
    assert_ne!(p, 0);
    assert_eq!(e.bloque(p).unwrap().dueno, t);
    // La cabecera empieza con una lista vacía de charcos.
    assert_eq!(l32(&e, p), p + 4);
    let a = pool_llamar(&mut e, "AllocPooled", [100, 0, 0], [p, 0]);
    let b = pool_llamar(&mut e, "AllocPooled", [12, 0, 0], [p, 0]);
    assert!(a != 0 && b != 0 && a != b);
    // Un charco: un MemHeader en la lista del charco, con los dos trozos dentro.
    let mh = l32(&e, p);
    assert_eq!(l8(&e, mh + node::LN_TYPE), nt::MEMORY);
    let (abajo, arriba) = (l32(&e, mh + memheader::MH_LOWER), l32(&e, mh + memheader::MH_UPPER));
    assert!(a >= abajo && b >= abajo && a + 104 <= arriba && b + 16 <= arriba);
    assert_eq!(abajo, mh + 40, "8 bytes sin usar tras el MemHeader");
    // Lo que escribe el runtime de Dash tras el MemHeader no rompe nada.
    e.mem.escribir32(mh + 32, 0).unwrap();
    // Uno grande (más que threshSize) va aparte.
    let g = pool_llamar(&mut e, "AllocPooled", [2000, 0, 0], [p, 0]);
    assert!(g != 0 && (g < abajo || g >= arriba));
    // Muchos: hacen falta más charcos.
    let v: Vec<u32> = (0..100).map(|_| pool_llamar(&mut e, "AllocPooled", [200, 0, 0], [p, 0])).collect();
    assert!(v.iter().all(|&x| x != 0));
    assert!(e.nodos(p).unwrap().len() >= 5);
    for &x in &v {
        pool_llamar(&mut e, "FreePooled", [200, 0, 0], [p, x]);
    }
    pool_llamar(&mut e, "FreePooled", [100, 0, 0], [p, a]);
    pool_llamar(&mut e, "FreePooled", [12, 0, 0], [p, b]);
    pool_llamar(&mut e, "FreePooled", [2000, 0, 0], [p, g]);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
    // Los charcos vacíos vuelven al sistema, salvo uno.
    assert_eq!(e.nodos(p).unwrap().len(), 1);
    pool_llamar(&mut e, "DeletePool", [0, 0, 0], [p, 0]);
    assert_eq!(libre(&e), antes);
    assert_eq!(e.charcos_vivos(), 0);
}

#[test]
fn charco_con_memf_clear_y_borrar_con_todo_dentro() {
    let (mut e, _) = caja_con_tarea();
    let antes = libre(&e);
    let p = e.create_pool(memf::CLEAR, 1024, 256).unwrap();
    let a = e.alloc_pooled(p, 64).unwrap();
    e.mem.leer_mut(a, 64).unwrap().fill(0xAA);
    e.free_pooled(p, a, 64).unwrap();
    let b = e.alloc_pooled(p, 64).unwrap();
    assert!(e.mem.leer(b, 64).unwrap().iter().all(|&x| x == 0));
    for _ in 0..30 {
        assert_ne!(e.alloc_pooled(p, 100).unwrap(), 0);
    }
    assert_ne!(e.alloc_pooled(p, 5000).unwrap(), 0);
    // DeletePool lo suelta todo de una vez.
    e.delete_pool(p).unwrap();
    assert_eq!(libre(&e), antes);
}

#[test]
fn create_pool_con_tamanos_malos() {
    let (mut e, _) = caja_con_tarea();
    assert_eq!(e.create_pool(0, 1024, 2048).unwrap(), 0, "threshSize mayor que puddleSize");
    assert_eq!(e.create_pool(0, 0, 0).unwrap(), 0);
    assert_eq!(e.create_pool(0, 1024, 0).unwrap(), 0);
    assert_eq!(e.create_pool(0, u32::MAX, 16).unwrap(), 0);
    assert_eq!(e.create_pool(0, 128 << 20, 16).unwrap(), 0, "más que la memoria");
    let p = e.create_pool(0, 1024, 512).unwrap();
    assert_eq!(e.alloc_pooled(p, 0).unwrap(), 0);
    assert_eq!(e.alloc_pooled(p, u32::MAX).unwrap(), 0);
    assert_eq!(e.alloc_pooled(p, 0xFFFF_FFF9).unwrap(), 0);
    e.delete_pool(p).unwrap();
}

#[test]
fn hostil_charcos() {
    let (mut e, _) = caja_con_tarea();
    let antes = libre(&e);
    let p = e.create_pool(0, 1024, 512).unwrap();
    let a = e.alloc_pooled(p, 40).unwrap();
    let g = e.alloc_pooled(p, 800).unwrap();
    // Lo que no es un charco.
    let falso = e.alloc_mem(64, memf::CLEAR).unwrap();
    assert!(matches!(e.alloc_pooled(falso, 16), Err(Fallo::NoEsCharco { .. })));
    assert!(matches!(e.alloc_pooled(0, 16), Err(Fallo::PunteroNulo)));
    assert!(matches!(e.alloc_pooled(4, 16), Err(Fallo::NoEsCharco { .. })));
    assert!(matches!(e.free_pooled(falso, a, 40), Err(Fallo::NoEsCharco { .. })));
    assert!(matches!(e.delete_pool(falso), Err(Fallo::NoEsCharco { .. })));
    assert!(e.delete_pool(0).is_ok());
    // Soltar lo que no es suyo, con otro tamaño, a medias o dos veces: nada se toca.
    let libre_charco = l32(&e, l32(&e, p) + memheader::MH_FREE);
    assert!(matches!(e.free_pooled(p, falso, 64), Err(Fallo::NoReservado { .. })));
    assert!(matches!(e.free_pooled(p, a, 48), Err(Fallo::TamanoDistinto { .. })));
    assert!(matches!(e.free_pooled(p, a + 8, 32), Err(Fallo::NoReservado { .. })));
    assert!(matches!(e.free_pooled(p, g, 100), Err(Fallo::TamanoDistinto { .. })));
    assert!(matches!(e.free_pooled(p, 0xFFFF_FFF0, 8), Err(Fallo::NoReservado { .. })));
    assert_eq!(l32(&e, l32(&e, p) + memheader::MH_FREE), libre_charco);
    e.free_pooled(p, a, 40).unwrap();
    assert!(matches!(e.free_pooled(p, a, 40), Err(Fallo::NoReservado { .. })), "dos veces");
    e.free_pooled(p, g, 800).unwrap();
    assert!(matches!(e.free_pooled(p, g, 800), Err(Fallo::NoReservado { .. })));
    // El programa estropea el MemHeader de su charco: lo nota antes de repartir fuera.
    let mh = l32(&e, p);
    e.mem.escribir32(mh + memheader::MH_UPPER, 0x30_0000).unwrap();
    assert!(matches!(e.alloc_pooled(p, 16), Err(Fallo::ListaRota { .. })));
    e.mem.escribir32(mh + memheader::MH_UPPER, mh + 40 + 1024).unwrap();
    // Una lista de trozos libres que apunta fuera del charco.
    let primero = l32(&e, mh + memheader::MH_FIRST);
    e.mem.escribir32(mh + memheader::MH_FIRST, falso).unwrap();
    assert!(e.alloc_pooled(p, 16).is_err());
    e.mem.escribir32(mh + memheader::MH_FIRST, primero).unwrap();
    // Borrado, ya no es un charco.
    e.delete_pool(p).unwrap();
    assert!(matches!(e.alloc_pooled(p, 16), Err(Fallo::NoEsCharco { .. })));
    assert!(matches!(e.delete_pool(p), Err(Fallo::NoEsCharco { .. })));
    e.free_mem(falso, 64).unwrap();
    assert_eq!(libre(&e), antes);
}

#[test]
fn matar_la_tarea_suelta_sus_charcos() {
    let mut e = caja();
    let antes = libre(&e);
    let t = e.crear_tarea("con charco", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let p = e.create_pool(0, 2048, 1024).unwrap();
    for _ in 0..40 {
        e.alloc_pooled(p, 300).unwrap();
    }
    e.alloc_pooled(p, 1500).unwrap();
    e.matar_tarea(t).unwrap();
    assert_eq!(e.charcos_vivos(), 0);
    assert_eq!(libre(&e), antes);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

#[test]
fn semaforo_exclusivo_anidado_y_compartido() {
    let (mut e, t) = caja_con_tarea();
    let s = e.alloc_mem(signalsemaphore::TAMANO, memf::CLEAR).unwrap();
    let mut r = Registros::default();
    r.a[0] = s;
    llamar(&mut e, "InitSemaphore", &mut r);
    assert_eq!(l8(&e, s + node::LN_TYPE), nt::SIGNALSEM);
    assert_eq!(e.mem.leer16(s + signalsemaphore::SS_QUEUECOUNT).unwrap(), 0xFFFF);
    assert_eq!(llamar(&mut e, "ObtainSemaphore", &mut r), Accion::Seguir);
    assert_eq!(llamar(&mut e, "ObtainSemaphore", &mut r), Accion::Seguir, "la misma tarea, otra vez");
    assert_eq!(l32(&e, s + signalsemaphore::SS_OWNER), t);
    assert_eq!(e.mem.leer16(s + signalsemaphore::SS_NESTCOUNT).unwrap(), 2);
    llamar(&mut e, "ReleaseSemaphore", &mut r);
    llamar(&mut e, "ReleaseSemaphore", &mut r);
    assert_eq!(l32(&e, s + signalsemaphore::SS_OWNER), 0);
    assert_eq!(e.mem.leer16(s + signalsemaphore::SS_QUEUECOUNT).unwrap(), 0xFFFF);
    // Compartido: el dueño queda a 0.
    assert_eq!(llamar(&mut e, "ObtainSemaphoreShared", &mut r), Accion::Seguir);
    assert_eq!(l32(&e, s + signalsemaphore::SS_OWNER), 0);
    llamar(&mut e, "AttemptSemaphoreShared", &mut r);
    assert_eq!(r.d[0], 1);
    llamar(&mut e, "ReleaseSemaphore", &mut r);
    llamar(&mut e, "ReleaseSemaphore", &mut r);
    // Soltar de más: aviso, sin tocar nada.
    llamar(&mut e, "ReleaseSemaphore", &mut r);
    assert_eq!(avisos_de(&e, "ReleaseSemaphore"), 1);
    assert_eq!(e.mem.leer16(s + signalsemaphore::SS_NESTCOUNT).unwrap(), 0);
}

#[test]
fn semaforo_de_otra_tarea_espera_y_despierta() {
    let mut e = caja();
    let a = e.crear_tarea("a", 0, 1024, 0x1000).unwrap();
    let b = e.crear_tarea("b", 0, 1024, 0x1000).unwrap();
    let s = e.alloc_mem(signalsemaphore::TAMANO, memf::CLEAR).unwrap();
    e.init_semaphore(s).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(a));
    let mut r = Registros::default();
    r.a[0] = s;
    assert_eq!(llamar(&mut e, "ObtainSemaphore", &mut r), Accion::Seguir);
    // b lo pide: espera.
    e.mem.escribir8(a + task::TC_STATE, ts::READY).unwrap();
    e.mem.escribir32(e.base() + execbase::THISTASK, b).unwrap();
    e.remove(b).unwrap();
    e.mem.escribir8(b + task::TC_STATE, ts::RUN).unwrap();
    e.enqueue(e.base() + execbase::TASKREADY, a).unwrap();
    assert_eq!(llamar(&mut e, "AttemptSemaphore", &mut r.clone()), Accion::Seguir);
    let mut rb = r;
    assert_eq!(llamar(&mut e, "ObtainSemaphore", &mut rb), Accion::Bloquear);
    assert_eq!(l8(&e, b + task::TC_STATE), ts::WAIT);
    // a lo suelta: b despierta con SIGF_SINGLE, repite y se lo queda, sin la señal.
    assert_eq!(e.elegir_siguiente(), Some(a));
    llamar(&mut e, "ReleaseSemaphore", &mut r.clone());
    assert_eq!(l8(&e, b + task::TC_STATE), ts::READY);
    e.rem_task(a).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(b));
    let mut rb = r;
    assert_eq!(llamar(&mut e, "ObtainSemaphore", &mut rb), Accion::Seguir);
    assert_eq!(l32(&e, s + signalsemaphore::SS_OWNER), b);
    assert_eq!(l32(&e, b + task::TC_SIGRECVD) & sig::SIGF_SINGLE, 0);
}

#[test]
fn semaforo_de_una_tarea_muerta() {
    let mut e = caja();
    let a = e.crear_tarea("a", 0, 1024, 0x1000).unwrap();
    let s = e.alloc_mem(signalsemaphore::TAMANO, memf::CLEAR).unwrap();
    e.init_semaphore(s).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(a));
    assert!(matches!(e.obtain_semaphore(s, false), Ok(caja_exec::Espera::Hecho(_))));
    e.matar_tarea(a).unwrap();
    let b = e.crear_tarea("b", 0, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(b));
    assert!(matches!(e.obtain_semaphore(s, false), Ok(caja_exec::Espera::Hecho(_))), "se lo queda, con un aviso");
    assert_eq!(l32(&e, s + signalsemaphore::SS_OWNER), b);
}

#[test]
fn hostil_semaforos() {
    let (mut e, _) = caja_con_tarea();
    for d in [0, 4, 0x800, TAMANO - 10, 0xFFFF_FFF0, u32::MAX] {
        let mut r = Registros::default();
        r.a[0] = d;
        r.a[1] = d;
        for f in [
            "InitSemaphore",
            "ObtainSemaphore",
            "ReleaseSemaphore",
            "AttemptSemaphore",
            "ObtainSemaphoreShared",
            "AttemptSemaphoreShared",
            "AddSemaphore",
            "RemSemaphore",
            "FindSemaphore",
        ] {
            let a = llamar(&mut e, f, &mut r);
            assert_eq!(a, Accion::Seguir, "{} con {:#x}", f, d);
        }
    }
    assert!(e.avisos().len() >= 6 * 9 - 2);
    // Un semáforo en la lista pública, buscado por su nombre.
    let s = nodo(&mut e, "dash.cpu", 0);
    let s2 = e.alloc_mem(signalsemaphore::TAMANO, memf::CLEAR).unwrap();
    let nombre = l32(&e, s + node::LN_NAME);
    e.mem.escribir32(s2 + node::LN_NAME, nombre).unwrap();
    e.add_semaphore(s2).unwrap();
    assert_eq!(e.find_semaphore(nombre).unwrap(), s2);
    e.rem_semaphore(s2).unwrap();
    assert_eq!(e.find_semaphore(nombre).unwrap(), 0);
    assert!(e.rem_semaphore(s2).is_err(), "dos veces");
}

#[test]
fn set_except_apunta_las_senales() {
    let (mut e, t) = caja_con_tarea();
    let mut r = Registros::default();
    r.d[0] = sig::SIGBREAKF_CTRL_C;
    r.d[1] = sig::SIGBREAKF_CTRL_C;
    llamar(&mut e, "SetExcept", &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(l32(&e, t + task::TC_SIGEXCEPT), sig::SIGBREAKF_CTRL_C);
    r.d[0] = 0;
    r.d[1] = sig::SIGBREAKF_CTRL_C;
    llamar(&mut e, "SetExcept", &mut r);
    assert_eq!(r.d[0], sig::SIGBREAKF_CTRL_C);
    assert_eq!(l32(&e, t + task::TC_SIGEXCEPT), 0);
}

#[test]
fn stack_swap_y_vuelta() {
    let (mut e, t) = caja_con_tarea();
    let (bajo, alto) = (l32(&e, t + task::TC_SPLOWER), l32(&e, t + task::TC_SPUPPER));
    // La pila de la tarea, con la dirección de vuelta de la llamada encima.
    let a7 = alto - 16;
    e.mem.escribir32(a7, 0x12_3456).unwrap();
    let nueva = e.alloc_mem(8192, memf::CLEAR).unwrap();
    let sss = e.alloc_mem(12, memf::CLEAR).unwrap();
    e.mem.escribir32(sss, nueva).unwrap();
    e.mem.escribir32(sss + 4, nueva + 8192).unwrap();
    e.mem.escribir32(sss + 8, nueva + 8192).unwrap();
    let mut r = Registros::default();
    r.a[0] = sss;
    r.a[7] = a7;
    llamar(&mut e, "StackSwap", &mut r);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
    assert_eq!(r.a[7], nueva + 8188);
    assert_eq!(l32(&e, r.a[7]), 0x12_3456, "la vuelta, en la pila nueva");
    assert_eq!((l32(&e, t + task::TC_SPLOWER), l32(&e, t + task::TC_SPUPPER)), (nueva, nueva + 8192));
    assert_eq!((l32(&e, sss), l32(&e, sss + 4), l32(&e, sss + 8)), (bajo, alto, a7 + 4));
    // Y de vuelta (tras el RTS, A7 = nueva + 8192; otra llamada empuja su vuelta).
    let a7 = nueva + 8192 - 4;
    e.mem.escribir32(a7, 0x65_4320).unwrap();
    r.a[7] = a7;
    llamar(&mut e, "StackSwap", &mut r);
    assert_eq!(r.a[7], alto - 16);
    assert_eq!(l32(&e, r.a[7]), 0x65_4320);
    assert_eq!((l32(&e, t + task::TC_SPLOWER), l32(&e, t + task::TC_SPUPPER)), (bajo, alto));
}

#[test]
fn hostil_stack_swap() {
    let (mut e, t) = caja_con_tarea();
    let (bajo, alto) = (l32(&e, t + task::TC_SPLOWER), l32(&e, t + task::TC_SPUPPER));
    let sss = e.alloc_mem(12, memf::CLEAR).unwrap();
    let a7 = alto - 16;
    for (lo, hi, p) in [
        (0, 0, 0),
        (0, 4096, 4096),
        (0x20_0000, 0x10_0000, 0x10_0000),
        (0x20_0000, 0x20_1000, 0x20_1001),
        (0x20_0000, 0x20_1000, 0x20_2000),
        (0xFFFF_0000, 0xFFFF_FFFF, 0xFFFF_FFF0),
        (TAMANO - 64, TAMANO + 64, TAMANO + 64),
    ] {
        e.mem.escribir32(sss, lo).unwrap();
        e.mem.escribir32(sss + 4, hi).unwrap();
        e.mem.escribir32(sss + 8, p).unwrap();
        let mut r = Registros::default();
        r.a[0] = sss;
        r.a[7] = a7;
        llamar(&mut e, "StackSwap", &mut r);
        assert_eq!(r.a[7], a7, "nada cambia con {:#x}", p);
        assert_eq!((l32(&e, t + task::TC_SPLOWER), l32(&e, t + task::TC_SPUPPER)), (bajo, alto));
    }
    for d in [0, 4, 0xFFFF_FFF8] {
        let mut r = Registros::default();
        r.a[0] = d;
        r.a[7] = a7;
        llamar(&mut e, "StackSwap", &mut r);
        assert_eq!(r.a[7], a7);
    }
    // A7 fuera de la memoria: la vuelta no se puede leer.
    let mut r = Registros::default();
    r.a[0] = sss;
    r.a[7] = 0xFFFF_FFFE;
    llamar(&mut e, "StackSwap", &mut r);
    assert_eq!(r.a[7], 0xFFFF_FFFE);
}

#[test]
fn muchos_trozos_vivos_sin_ir_lento() {
    // Un programa de Dash tiene miles de textos y objetos vivos en su charco: repartir y soltar no puede costar más
    // cuantos más hay (antes se copiaba la tabla de trozos en cada llamada).
    let (mut e, _) = caja_con_tarea();
    let p = e.create_pool(0, 16384, 4096).unwrap();
    let t = std::time::Instant::now();
    let v: Vec<u32> = (0..10_000).map(|i| e.alloc_pooled(p, 16 + (i % 5) * 8).unwrap()).collect();
    assert!(v.iter().all(|&x| x != 0));
    for (i, &x) in v.iter().enumerate().rev() {
        e.free_pooled(p, x, 16 + (i as u32 % 5) * 8).unwrap();
    }
    assert!(t.elapsed().as_secs() < 10, "{:?}", t.elapsed());
    assert!(e.avisos().is_empty());
    e.delete_pool(p).unwrap();
}
