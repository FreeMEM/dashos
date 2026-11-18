//! Lo que un programa malo (o roto) le manda a bsdsocket.library: nunca un pánico ni un cuelgue; un error de BSD y,
//! si es un puntero malo, un aviso.

mod comun;
use comun::*;

use caja_bsdsocket::definiciones::{netdb, sbt};
use caja_exec::estructuras::{library, ts};
use caja_exec::{Accion, Fallo, LIMITE, Memoria};

#[test]
fn descriptores_malos() {
    let mut c = caja();
    let b = c.bloque(16);
    for fd in [u32::MAX, 64, 9999, 0x8000_0000, 3] {
        for (f, d, a) in [
            ("recv", vec![fd, 16, 0], vec![b]),
            ("send", vec![fd, 16, 0], vec![b]),
            ("CloseSocket", vec![fd], vec![]),
            ("IoctlSocket", vec![fd, 0x8004_667E], vec![b]),
            ("listen", vec![fd, 1], vec![]),
            ("shutdown", vec![fd, 2], vec![]),
            ("getsockname", vec![fd], vec![b, b]),
            ("Dup2Socket", vec![fd, 1], vec![]),
            ("ReleaseSocket", vec![fd, 1], vec![]),
        ] {
            assert_eq!(c.f(f, &d, &a) as i32, -1, "{f}({fd})");
            assert_eq!(c.errno(), EBADF, "{f}({fd})");
        }
    }
    // Dup2Socket a un descriptor fuera de la tabla.
    let fd = c.zocalo(1);
    assert_eq!(c.f("Dup2Socket", &[fd, 5000], &[]) as i32, -1);
    assert_eq!(c.errno(), EBADF);
    c.sin_avisos();
}

#[test]
fn punteros_fuera_en_la_pagina_cero_y_nulos() {
    let mut c = caja();
    let fd = c.zocalo(1);
    for (p, fallo_esperado) in [
        (0u32, Fallo::PunteroNulo),
        (0x10, Fallo::PaginaCero { dir: 0x10 }),
        (TAMANO + 0x100, Fallo::FueraDeMemoria { dir: TAMANO + 0x100, largo: 8 }),
        (0xFFFF_FFF8, Fallo::FueraDeMemoria { dir: 0xFFFF_FFF8, largo: 8 }),
    ] {
        c.s.tomar_avisos();
        assert_eq!(c.f("connect", &[fd, 16], &[p]) as i32, -1);
        assert_eq!(c.errno(), EFAULT);
        let a = c.s.tomar_avisos();
        assert_eq!(a.len(), 1, "{p:x}: {a:?}");
        if p != 0x10 {
            assert_eq!(a[0].fallo, fallo_esperado);
        } else {
            assert!(matches!(a[0].fallo, Fallo::PaginaCero { .. }));
        }
        assert_eq!(a[0].funcion, "connect");
    }
    // Un búfer que se sale de la memoria (o que da la vuelta): EFAULT, sin tocar nada.
    let (fd2, id) = c.conectado([10, 0, 0, 1], 80);
    c.red.llega(id, b"datos");
    let casi = TAMANO - 4;
    assert_eq!(c.f("recv", &[fd2, 16, 0], &[casi]) as i32, -1);
    assert_eq!(c.errno(), EFAULT);
    assert_eq!(c.f("recv", &[fd2, 16, 0], &[0x200]) as i32, -1);
    assert_eq!(c.f("send", &[fd2, 0x7FFF_FFFF, 0], &[0x10_0000]) as i32, -1);
    assert_eq!(c.errno(), EFAULT);
    assert_eq!(c.f("send", &[fd2, 16, 0], &[LIMITE - 8]) as i32, -1);
    assert_eq!(c.red.conn(id).entrada.len(), 5, "no se sacó nada");
    // Cadenas: nula, en la página 0, sin fin hasta el final de la memoria.
    assert_eq!(c.f("gethostbyname", &[], &[0]), 0);
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_HERRNO), 0).1 as i32, netdb::NETDB_INTERNAL);
    assert_eq!(c.f("inet_addr", &[], &[0x20]) as i32, -1);
    let sin_fin = TAMANO - 16;
    c.e.mem.leer_mut(sin_fin, 16).unwrap().fill(b'a');
    assert_eq!(c.f("gethostbyname", &[], &[sin_fin]), 0);
    assert_eq!(c.f("inet_addr", &[], &[sin_fin]), u32::MAX);
    // Escribir el resultado en la página 0.
    let (_, _) = (c.red.ultima(), 0);
    assert_eq!(c.f("getsockname", &[fd2], &[0x40, 0x44]) as i32, -1);
    assert_eq!(c.f("GetSocketEvents", &[], &[0x8]) as i32, -1);
    let ip = c.cadena("1.2.3.4");
    assert_eq!(c.f("inet_aton", &[], &[ip, 0x30]) as i32, -1);
    // errno a un puntero que no vale: la etiqueta falla con aviso, y no se escribe nada.
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_ERRNOLONGPTR), 0x100).0 as i32, -1);
    let q = c.bloque(4);
    c.f("SetErrnoPtr", &[3], &[q]);
    assert!(c.avisos_de("SetErrnoPtr") >= 1);
    // Nada de esto tocó exec.
    assert!(c.e.avisos().is_empty(), "{:?}", c.e.avisos());
}

#[test]
fn waitselect_hostil() {
    let mut c = caja();
    let (fd, _) = c.conectado([10, 0, 0, 1], 80);
    let set = c.bloque(256);
    let tv = c.bloque(8);
    // nfds negativo: EINVAL. Absurdo: se recorta a la tabla (64) y no lee más allá.
    assert_eq!(c.f("WaitSelect", &[u32::MAX, 0], &[set, 0, 0, tv]) as i32, -1);
    assert_eq!(c.errno(), EINVAL);
    c.e32(set, 1 << fd);
    assert_eq!(c.f("WaitSelect", &[0x7FFF_FFFF, 0], &[0, set, 0, tv]), 1);
    // Un conjunto fuera de la memoria o en la página 0.
    assert_eq!(c.f("WaitSelect", &[8, 0], &[0x10, 0, 0, tv]) as i32, -1);
    assert_eq!(c.errno(), EFAULT);
    assert_eq!(c.f("WaitSelect", &[64, 0], &[TAMANO - 4, 0, 0, tv]) as i32, -1);
    assert_eq!(c.errno(), EFAULT);
    // Un descriptor que no está abierto en el conjunto.
    c.e32(set, 1 << 9);
    assert_eq!(c.f("WaitSelect", &[10, 0], &[set, 0, 0, tv]) as i32, -1);
    assert_eq!(c.errno(), EBADF);
    // Un plazo que no vale.
    c.e32(tv + 4, 1_000_000);
    c.e32(set, 1 << fd);
    assert_eq!(c.f("WaitSelect", &[fd + 1, 0], &[set, 0, 0, tv]) as i32, -1);
    assert_eq!(c.errno(), EINVAL);
    c.e32(tv, 200_000_000);
    c.e32(tv + 4, 0);
    assert_eq!(c.f("WaitSelect", &[fd + 1, 0], &[set, 0, 0, tv]) as i32, -1);
    // Sin nada que esperar ni plazo, la máscara de señales en la página 0.
    assert_eq!(c.f("WaitSelect", &[0, 0x10], &[0, 0, 0, 0]) as i32, -1);
    assert_eq!(c.errno(), EFAULT);
}

#[test]
fn mil_zocalos() {
    let mut c = caja();
    let mut bien = 0;
    for _ in 0..1000 {
        if (c.f("socket", &[2, 1, 0], &[]) as i32) >= 0 {
            bien += 1;
        } else {
            assert_eq!(c.errno(), EMFILE);
        }
    }
    assert_eq!(bien, 64);
    // Con una tabla más grande caben; por debajo de lo abierto, no se deja.
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_DTABLESIZE), 10).0, 1);
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_DTABLESIZE), 1024).0, 0);
    for _ in 64..1000 {
        assert!((c.f("socket", &[2, 2, 0], &[]) as i32) >= 0);
    }
    assert_eq!(c.s.zocalos_abiertos(), 1000);
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_DTABLESIZE), 100_000).0, 1);
    // Cerrar la base los suelta todos.
    let b = c.base;
    c.cerrar(b);
    assert_eq!(c.s.zocalos_abiertos(), 0);
    c.sin_avisos();
}

#[test]
fn bases_que_no_son() {
    let mut c = caja();
    let lib = c.s.base();
    for base in [lib, 0, 0x1234_5678, c.e.base()] {
        c.base = base;
        let (acc, v) = c.una("socket", &[2, 1, 0], &[]);
        assert_eq!((acc, v as i32), (Accion::Seguir, -1));
        assert!(matches!(c.s.tomar_avisos()[0].fallo, Fallo::BibliotecaDesconocida { .. }));
    }
    // Cerrar dos veces la misma base: exec ya no la conoce.
    c.base = c.abrir();
    let b = c.base;
    c.cerrar(b);
    c.cerrar(b);
    assert!(c.e.avisos().iter().any(|a| a.fallo == Fallo::BibliotecaDesconocida { dir: b }));
    // CloseLibrary de la de LibList (la tiene abierta la primera base): el Close es suyo y no la suelta.
    c.e.tomar_avisos();
    c.cerrar(lib);
    assert_eq!(c.avisos_de("CloseLibrary"), 1);
    assert!(c.e.mem.leer16(lib + library::LIB_OPENCNT).unwrap() >= 1);
}

#[test]
fn etiquetas_hostiles() {
    let mut c = caja();
    // TAG_MORE a sí misma: no da vueltas sin fin.
    let t = c.bloque(16);
    c.e32(t, 2);
    c.e32(t + 4, t);
    assert_eq!(c.f("SocketBaseTagList", &[], &[t]) as i32, -1);
    assert!(c.s.avisos().iter().any(|a| matches!(a.fallo, Fallo::TablaMala { .. })));
    // Lista fuera de la memoria.
    assert_eq!(c.f("SocketBaseTagList", &[], &[TAMANO + 8]) as i32, -1);
    assert_eq!(c.errno(), EFAULT);
    // Nula: no hay nada que hacer.
    assert_eq!(c.f("SocketBaseTagList", &[], &[0]), 0);
    // Por referencia a la página 0.
    assert_eq!(c.etiqueta(sbt::getref(sbt::SBTC_ERRNO), 0x10).0 as i32, -1);
}

#[test]
fn llamadas_que_no_tocan() {
    let mut c = caja();
    let fd = c.zocalo(2);
    assert_eq!(c.f("listen", &[fd, 5], &[]) as i32, -1);
    assert_eq!(c.errno(), EOPNOTSUPP);
    let t = c.zocalo(1);
    assert_eq!(c.f("accept", &[t], &[0, 0]) as i32, -1);
    assert_eq!(c.errno(), EINVAL);
    let b = c.bloque(8);
    assert_eq!(c.f("recv", &[t, 8, 0], &[b]) as i32, -1);
    assert_eq!(c.errno(), ENOTCONN);
    assert_eq!(c.f("send", &[t, 8, 0], &[b]) as i32, -1);
    assert_eq!(c.errno(), ENOTCONN);
    assert_eq!(c.f("getpeername", &[t], &[b, b]) as i32, -1);
    // Una dirección de otra familia o corta.
    let sa = c.datos(&[16, 23, 0, 80, 1, 2, 3, 4]);
    assert_eq!(c.f("connect", &[t, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EAFNOSUPPORT);
    let sa = c.sockaddr([1, 2, 3, 4], 80);
    assert_eq!(c.f("connect", &[t, 4], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EINVAL);
    assert_eq!(c.f("connect", &[t, 0xFFFF_FFF0], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EINVAL);
    // bind a una dirección que no es de la caja.
    assert_eq!(c.f("bind", &[t, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EADDRNOTAVAIL);
    // sendmsg con mil millones de trozos.
    let m = c.bloque(28);
    c.e32(m + 8, b);
    c.e32(m + 12, 1_000_000_000);
    assert_eq!(c.f("sendmsg", &[fd, m, 0], &[m]) as i32, -1);
    assert_eq!(c.errno(), EMSGSIZE);
    // getaddrinfo sin dónde dejar el resultado.
    let h = c.cadena("1.2.3.4");
    assert_eq!(c.f("getaddrinfo", &[], &[h, 0, 0, 0]) as i32, netdb::EAI_SYSTEM);
    assert_eq!(c.errno(), EFAULT);
    // gethostname con largo 0 o negativo.
    assert_eq!(c.f("gethostname", &[0], &[b]) as i32, -1);
    assert_eq!(c.f("gethostname", &[u32::MAX], &[b]) as i32, -1);
}

/// Una tarea que espera en recv y hace otra cosa (código de excepción, por ejemplo): la espera se olvida, y la
/// repetición de la otra cosa no se confunde con la de recv.
#[test]
fn espera_olvidada_por_otra_llamada() {
    let mut c = caja();
    let (fd, id) = c.conectado([10, 0, 0, 1], 80);
    let b = c.bloque(8);
    let _ = c.bloquea("recv", &[fd, 8, 0], &[b]);
    c.e.despertar(c.tarea).unwrap();
    assert_eq!(c.e.elegir_siguiente(), Some(c.tarea));
    assert_eq!(c.f("gethostid", &[], &[]), 0xC0A8_0132);
    assert!(c.s.bloqueadas().is_empty());
    // Y el WaitPort de después no se cree una repetición.
    assert!(!c.e.vuelve_de_espera(c.tarea));
    c.red.llega(id, b"ok");
    assert_eq!(c.f("recv", &[fd, 8, 0], &[b]), 2);
    // Una tarea que muere esperando sin que nadie llame a tarea_muerta: atender no la despierta ni se cuelga.
    let t2 = c.e.crear_tarea("otra", 0, 2048, 0x10_0000).unwrap();
    let _ = c.bloquea("recv", &[fd, 8, 0], &[b]);
    assert_eq!(c.e.elegir_siguiente(), Some(t2));
    c.e.matar_tarea(c.tarea).unwrap();
    c.red.llega(id, b"x");
    assert_eq!(c.s.atender(&mut c.e, &mut c.red, c.ahora), 0);
    assert!(c.s.bloqueadas().is_empty());
    assert_eq!(c.estado(t2), ts::RUN);
}
