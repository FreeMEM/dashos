//! ahi.device por peticiones: el camino de AmigaAMP (dos `CMD_WRITE` encadenados con `ahir_Link`), las órdenes y lo
//! hostil.

mod comun;

use caja_exec::estructuras::node;
use caja_exec::{Accion, Fallo, Memoria, io};
use caja_sonido::*;
use comun::*;

const AHIST_M8S: u32 = 0;
const AHIST_S16S: u32 = 3;
const AHIST_M16S: u32 = 1;

/// Un búfer estéreo de 16 bits con `n` cuadros iguales.
fn estereo(c: &mut Caja, n: usize, l: i16, r: i16) -> u32 {
    let b: Vec<u8> = (0..n).flat_map(|_| [l.to_be_bytes(), r.to_be_bytes()].concat()).collect();
    c.bytes(&b)
}

#[allow(clippy::too_many_arguments)]
fn escribir(c: &mut Caja, ior: u32, datos: u32, largo: u32, tipo: u32, hz: u32, vol: u32, pos: u32, enlace: u32) {
    let m = &mut c.e.mem;
    m.escribir16(ior + io::IO_COMMAND, io::CMD_WRITE).unwrap();
    m.escribir32(ior + io::IO_DATA, datos).unwrap();
    m.escribir32(ior + io::IO_LENGTH, largo).unwrap();
    m.escribir32(ior + io::IO_OFFSET, 0).unwrap();
    m.escribir32(ior + AHIR_TYPE, tipo).unwrap();
    m.escribir32(ior + AHIR_FREQUENCY, hz).unwrap();
    m.escribir32(ior + AHIR_VOLUME, vol).unwrap();
    m.escribir32(ior + AHIR_POSITION, pos).unwrap();
    m.escribir32(ior + AHIR_LINK, enlace).unwrap();
}

fn orden(c: &mut Caja, ior: u32, o: u16) {
    c.e.mem.escribir16(ior + io::IO_COMMAND, o).unwrap();
}

#[test]
fn abrir_unidades_y_versiones() {
    let mut c = caja();
    let (_, ior, e) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    assert_eq!(e, 0);
    assert_eq!(c.e.mem.leer32(ior + io::IO_DEVICE).unwrap(), c.s.base_ahi());
    let u0 = c.e.mem.leer32(ior + io::IO_UNIT).unwrap();
    assert_ne!(u0, 0);
    assert_eq!(c.e.mem.leer16(u0 + 36).unwrap(), 1, "unit_OpenCnt");
    let (_, i2, e) = c.abrir("ahi.device", AHI_NO_UNIT, AHIR_TAMANO, 4);
    assert_eq!(e, 0);
    assert_ne!(c.e.mem.leer32(i2 + io::IO_UNIT).unwrap(), u0);
    assert_eq!(c.e.mem.leer16(c.s.base_ahi() + 32).unwrap(), 2, "lib_OpenCnt");
    // Una unidad que no existe, una versión de más.
    assert_eq!(c.abrir("ahi.device", 4, AHIR_TAMANO, 4).2, io::IOERR_OPENFAIL);
    assert_eq!(c.abrir("ahi.device", 0, AHIR_TAMANO, 7).2, io::IOERR_OPENFAIL);
    // Una IORequest corta con AHI_NO_UNIT (la biblioteca sola) vale.
    assert_eq!(c.abrir("ahi.device", AHI_NO_UNIT, 32, 0).2, 0);
    assert_eq!(c.io("CloseDevice", ior).0, Accion::Seguir);
    assert_eq!(c.e.mem.leer16(u0 + 36).unwrap(), 0);
    assert_eq!(c.e.mem.leer32(ior + io::IO_DEVICE).unwrap(), u32::MAX);
    assert_eq!(c.e.mem.leer16(c.s.base_ahi() + 32).unwrap(), 2);
    // audio.device también está, y abre sin asignar (ProTracker lo abre así al arrancar).
    let (_, ia, e) = c.abrir("audio.device", 0, IOA_TAMANO, 0);
    assert_eq!(e, 0);
    assert_eq!(c.e.mem.leer32(ia + io::IO_DEVICE).unwrap(), c.s.base_audio());
    c.sin_avisos();
}

/// AmigaAMP: dos peticiones que se turnan; cada una, al volver, se rellena y se manda enlazada a la otra.
#[test]
fn amigaamp_doble_bufer_sin_hueco_y_a_su_ritmo() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let b = c.copia(a, AHIR_TAMANO);
    const N: usize = 4800; // 100 ms
    let bufs = [estereo(&mut c, N, 1000, -1000), estereo(&mut c, N, 2000, -2000)];
    let iors = [a, b];
    escribir(&mut c, a, bufs[0], 4 * N as u32, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    escribir(&mut c, b, bufs[1], 4 * N as u32, AHIST_S16S, 48_000, 0x10000, 0x8000, a);
    c.io("SendIO", b);
    let inicio = c.ahora + 1_000; // el primer `atender` empieza a sonar
    let mut vueltas = Vec::new();
    let mut siguiente = 0; // la que debe volver
    let mut mandadas = 2;
    for _ in 0..1200 {
        c.pasar(1_000);
        while c.recoger(iors[siguiente]) {
            assert_eq!(c.error(iors[siguiente]), 0);
            assert_eq!(c.e.mem.leer32(iors[siguiente] + io::IO_ACTUAL).unwrap(), 4 * N as u32);
            vueltas.push(c.ahora);
            if mandadas < 10 {
                // Se rellena (otro valor, para ver el orden) y va detrás de la otra.
                let otra = iors[1 - siguiente];
                escribir(
                    &mut c,
                    iors[siguiente],
                    bufs[siguiente],
                    4 * N as u32,
                    AHIST_S16S,
                    48_000,
                    0x10000,
                    0x8000,
                    otra,
                );
                c.io("SendIO", iors[siguiente]);
                mandadas += 1;
            }
            siguiente = 1 - siguiente;
        }
    }
    assert_eq!(vueltas.len(), 10);
    // Cada una vuelve cuando su último cuadro ha sonado: ni antes, ni mucho después (un trozo y un paso).
    for (k, &t) in vueltas.iter().enumerate() {
        let debe = inicio + (k as u64 + 1) * 100_000;
        assert!(t >= debe && t <= debe + 7_000, "vuelta {k}: {t} y debía {debe}");
    }
    // Lo que suena: diez búferes seguidos, sin un cuadro de hueco, alternando (en el centro, la mitad a cada lado).
    let s = &c.salida.escritos;
    let primero = s.iter().position(|x| *x != [0, 0]).unwrap();
    assert_eq!(primero, 0);
    for k in 0..10 {
        let v = if k % 2 == 0 { [500, -500] } else { [1000, -1000] };
        assert!(s[k * N..(k + 1) * N].iter().all(|x| *x == v), "búfer {k}");
    }
    assert!(s[10 * N..].iter().all(|x| *x == [0, 0]));
    // Y no se mezcla de más: tras acabar, la salida para.
    assert!(s.len() < 10 * N + 2400 + 240);
    c.sin_avisos();
}

#[test]
fn con_el_reloj_del_sonido() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let d = estereo(&mut c, 4800, 100, 100);
    escribir(&mut c, a, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    // La salida dice que nada ha sonado aún: se llena hasta el adelanto (40 ms = 1920 cuadros) y no más, aunque el
    // reloj de la caja corra.
    c.salida.sonado = Some(0);
    for _ in 0..20 {
        c.ahora += 10_000;
        c.s.atender(&mut c.e, &mut c.salida, c.ahora, true);
    }
    assert_eq!(c.salida.escritos.len(), 1920);
    // Van sonando 1.000: se mezcla lo justo para seguir 40 ms por delante.
    c.salida.sonado = Some(1000);
    c.s.atender(&mut c.e, &mut c.salida, c.ahora, true);
    assert_eq!(c.salida.escritos.len(), 2920);
    // Suenan 4.799 cuadros: aún no vuelve; 4.800, sí.
    c.salida.sonado = Some(4799);
    c.s.atender(&mut c.e, &mut c.salida, c.ahora, true);
    assert!(!c.ha_vuelto(a));
    c.salida.sonado = Some(4800);
    c.s.atender(&mut c.e, &mut c.salida, c.ahora, true);
    assert!(c.recoger(a));
    assert!(c.salida.escritos.len() <= 4800 + TROZO, "acabó: no se mezcla silencio de más");
    // Una salida llena no recibe nada.
    c.salida.cabe = 0;
    escribir(&mut c, a, d, 4 * 960, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    let antes = c.salida.escritos.len();
    c.s.atender(&mut c.e, &mut c.salida, c.ahora, true);
    assert_eq!(c.salida.escritos.len(), antes);
    c.sin_avisos();
}

#[test]
fn varias_a_la_vez_y_frecuencia_distinta() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let b = c.copia(a, AHIR_TAMANO);
    // Mono de 8 bits a 24 kHz (cada muestra, dos cuadros) a la izquierda; mono de 16 a la derecha. Suenan juntas.
    let m8 = c.bytes(&[64; 240]);
    let m16 = c.bytes(&[0x10, 0x00].repeat(480));
    escribir(&mut c, a, m8, 240, AHIST_M8S, 24_000, 0x10000, 0, 0);
    escribir(&mut c, b, m16, 960, AHIST_M16S, 48_000, 0x10000, 0x10000, 0);
    c.io("SendIO", a);
    c.io("SendIO", b);
    c.pasar(30_000);
    assert!(c.recoger(a) && c.recoger(b));
    let s = &c.salida.escritos;
    assert!(s[..479].iter().all(|x| *x == [64 << 8, 0x1000]), "{:?}", &s[..4]);
    c.sin_avisos();
}

#[test]
fn doio_espera_a_que_suene() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let d = estereo(&mut c, 480, 1, 1);
    escribir(&mut c, a, d, 4 * 480, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    assert_eq!(c.io("DoIO", a).0, Accion::Bloquear);
    c.pasar(5_000);
    assert!(!c.ha_vuelto(a));
    c.pasar(10_000);
    assert_eq!(c.e.elegir_siguiente(), Some(c.tarea));
    assert_eq!(c.io("DoIO", a), (Accion::Seguir, 0));
    // Lo que acaba en el acto (CMD_UPDATE) con DoIO: rápido, sin pasar por el puerto.
    orden(&mut c, a, io::CMD_UPDATE);
    assert_eq!(c.io("DoIO", a), (Accion::Seguir, 0));
    c.sin_avisos();
}

#[test]
fn ordenes_flush_stop_start_read_y_consulta() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let b = c.copia(a, AHIR_TAMANO);
    let o = c.copia(a, AHIR_TAMANO);
    let d = estereo(&mut c, 4800, 7, 7);
    // CMD_STOP: las escrituras esperan hasta CMD_START (se anidan).
    orden(&mut c, o, io::CMD_STOP);
    assert_eq!(c.io("DoIO", o).1, 0);
    assert_eq!(c.io("DoIO", o).1, 0);
    escribir(&mut c, a, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    c.pasar(10_000);
    assert!(c.salida.escritos.is_empty());
    orden(&mut c, o, io::CMD_START);
    c.io("DoIO", o);
    c.pasar(10_000);
    assert!(c.salida.escritos.is_empty(), "aún queda un CMD_STOP");
    c.io("DoIO", o);
    c.pasar(10_000);
    assert!(!c.salida.escritos.is_empty());
    // CMD_FLUSH: todas vuelven abortadas; io_Actual, cuántas.
    escribir(&mut c, b, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, a);
    c.io("SendIO", b);
    orden(&mut c, o, io::CMD_FLUSH);
    assert_eq!(c.io("DoIO", o).1, 0);
    assert_eq!(c.e.mem.leer32(o + io::IO_ACTUAL).unwrap(), 2);
    assert!(c.recoger(a) && c.recoger(b));
    assert_eq!((c.error(a), c.error(b)), (io::IOERR_ABORTED, io::IOERR_ABORTED));
    // CMD_READ: no hay entrada.
    orden(&mut c, o, io::CMD_READ);
    assert_eq!(c.io("DoIO", o).1 as i8, io::IOERR_NOCMD);
    // Una orden que no existe.
    orden(&mut c, o, 0x7777);
    assert_eq!(c.io("DoIO", o).1 as i8, io::IOERR_NOCMD);
    // NSCMD_DEVICEQUERY.
    let q = c.reservar(16);
    orden(&mut c, o, NSCMD_DEVICEQUERY);
    c.e.mem.escribir32(o + io::IO_DATA, q).unwrap();
    c.e.mem.escribir32(o + io::IO_LENGTH, 16).unwrap();
    assert_eq!(c.io("DoIO", o).1, 0);
    assert_eq!(c.e.mem.leer32(q + 4).unwrap(), 16);
    let lista = c.e.mem.leer32(q + 12).unwrap();
    let ordenes: Vec<u16> = (0..10).map(|i| c.e.mem.leer16(lista + 2 * i).unwrap()).collect();
    assert!(ordenes.contains(&io::CMD_WRITE) && ordenes.contains(&NSCMD_DEVICEQUERY) && ordenes[9] == 0);
    c.e.mem.escribir32(o + io::IO_LENGTH, 8).unwrap();
    assert_eq!(c.io("DoIO", o).1 as i8, io::IOERR_BADLENGTH);
    // CMD_RESET deshace los CMD_STOP.
    orden(&mut c, o, io::CMD_STOP);
    c.io("DoIO", o);
    orden(&mut c, o, io::CMD_RESET);
    c.io("DoIO", o);
    escribir(&mut c, a, d, 4 * 480, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    // (Lo ya mezclado de las abortadas está en la salida y suena antes: hasta 40 ms.)
    c.io("SendIO", a);
    c.pasar(20_000);
    assert!(!c.ha_vuelto(a));
    c.pasar(50_000);
    assert!(c.recoger(a));
    c.sin_avisos();
}

#[test]
fn abortar_cerrar_y_tarea_que_muere() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let b = c.copia(a, AHIR_TAMANO);
    let d = estereo(&mut c, 4800, 3, 3);
    // AbortIO de lo que no está: -1, sin más.
    assert_eq!(c.io("AbortIO", a).1, u32::MAX);
    // AbortIO de la que suena: vuelve abortada y la enlazada detrás suena desde su principio.
    escribir(&mut c, a, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    escribir(&mut c, b, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, a);
    c.io("SendIO", b);
    c.pasar(10_000);
    assert_eq!(c.io("AbortIO", a).1, 0);
    assert!(c.recoger(a));
    assert_eq!(c.error(a), io::IOERR_ABORTED);
    assert_eq!(c.io("AbortIO", a).1, u32::MAX, "ya volvió");
    c.pasar(150_000);
    assert!(c.recoger(b));
    assert_eq!(c.error(b), 0);
    // Cerrar con escrituras en marcha: vuelven abortadas (con un aviso).
    escribir(&mut c, a, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    c.pasar(5_000);
    c.io("CloseDevice", a);
    assert!(c.recoger(a));
    assert_eq!(c.error(a), io::IOERR_ABORTED);
    assert_eq!(c.avisos().len(), 1);
    // Una tarea que muere con escrituras: se olvidan (no vuelven, ni avisan) y las de otra siguen.
    let (_, x, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let t2 = c.e.crear_tarea("otra", 0, 4096, 0x10_0000).unwrap();
    c.e.poner_en_marcha(t2).unwrap();
    let (_, y, _) = c.abrir("ahi.device", 1, AHIR_TAMANO, 4);
    escribir(&mut c, y, d, 4 * 4800, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", y);
    c.e.poner_en_marcha(c.tarea).unwrap();
    escribir(&mut c, x, d, 4 * 960, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", x);
    c.pasar(5_000);
    c.s.tarea_muerta(t2);
    c.e.matar_tarea(t2).unwrap();
    c.pasar(200_000);
    assert!(c.recoger(x));
    assert!(!c.ha_vuelto(y));
    assert!(!c.s.activo());
    c.sin_avisos();
}

#[test]
fn hostil() {
    let mut c = caja();
    let (_, a, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let d = estereo(&mut c, 100, 1, 1);
    let casos: [(u32, u32, u32, u32, i8); 9] = [
        // datos, largo, tipo, frecuencia, error
        (0x10, 400, AHIST_S16S, 48_000, io::IOERR_BADADDRESS), // la página 0
        (TAMANO - 100, 400, AHIST_S16S, 48_000, io::IOERR_BADADDRESS), // se sale de la memoria
        (0xFFFF_FF00, 400, AHIST_S16S, 48_000, io::IOERR_BADADDRESS),
        (d, 401, AHIST_S16S, 48_000, io::IOERR_BADLENGTH), // no es de cuadros enteros
        (d, 0x7FFF_FFFC, AHIST_S16S, 48_000, io::IOERR_BADLENGTH), // enorme
        (d, 400, 77, 48_000, AHIE_BADSAMPLETYPE as i8),
        (d, 400, AHIST_S16S, 0, AHIE_UNKNOWN as i8),
        (d, 0, AHIST_S16S, 48_000, 0), // nada que tocar: vuelve ya
        (0, 400, AHIST_S16S, 48_000, io::IOERR_BADADDRESS),
    ];
    for (k, &(datos, largo, tipo, hz, err)) in casos.iter().enumerate() {
        escribir(&mut c, a, datos, largo, tipo, hz, 0x10000, 0x8000, 0);
        assert_eq!(c.io("DoIO", a), (Accion::Seguir, err as i32 as u32), "caso {k}");
    }
    let avisos = c.avisos();
    assert_eq!(avisos.len(), 4, "{avisos:#?}");
    // Una frecuencia absurda: se recorta, con aviso, y suena.
    escribir(&mut c, a, d, 400, AHIST_S16S, 0xFFFF_FFFF, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    assert_eq!(c.avisos().len(), 1);
    // Mandarla otra vez mientras suena: aviso y la de antes sigue (vuelve una sola vez).
    c.io("SendIO", a);
    assert!(c.avisos().iter().any(|x| x.contains("MensajeEnCola")));
    c.pasar(10_000);
    assert!(c.recoger(a));
    assert!(!c.recoger(a), "una sola vez");
    // Enlazada a sí misma, o a algo que no es una petición: suena sola.
    escribir(&mut c, a, d, 400, AHIST_S16S, 48_000, 0x10000, 0x8000, a);
    c.io("SendIO", a);
    c.pasar(10_000);
    assert!(c.recoger(a));
    escribir(&mut c, a, d, 400, AHIST_S16S, 48_000, 0x10000, 0x8000, 0xDEAD_BEE0);
    c.io("SendIO", a);
    c.pasar(10_000);
    assert!(c.recoger(a));
    // Peticiones malas: nulas, en la página 0, fuera, cortas.
    for m in [0u32, 4, 0x800, TAMANO - 8, 0xFFFF_FFF0] {
        for f in ["SendIO", "DoIO", "AbortIO", "CloseDevice"] {
            let _ = c.io(f, m);
            let _ = c.e.elegir_siguiente();
        }
        for n in 0..NUMEROS {
            let mut r = caja_exec::Registros { d: [m; 8], a: [m; 8] };
            assert_eq!(c.s.llamar(&mut c.e, PRIMERO + n, &mut r), Accion::Seguir);
        }
    }
    c.avisos();
    // Una petición copiada de otra que no se abrió (su io_Unit no es una unidad): aviso y IOERR_NOCMD.
    let b = c.copia(a, AHIR_TAMANO);
    c.e.mem.escribir32(b + io::IO_UNIT, 0x1234).unwrap();
    escribir(&mut c, b, d, 400, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    assert_eq!(c.io("DoIO", b).1 as i8, io::IOERR_NOCMD);
    assert_eq!(c.avisos().len(), 1);
    // Soltar la petición mientras suena (sin AbortIO): al acabar, aviso y nada más.
    escribir(&mut c, a, d, 400, AHIST_S16S, 48_000, 0x10000, 0x8000, 0);
    c.io("SendIO", a);
    c.e.mem.escribir8(a + node::LN_TYPE, 0).unwrap();
    c.e.free_mem(a, AHIR_TAMANO).unwrap();
    c.pasar(10_000);
    let av = c.avisos();
    assert!(av.len() == 1 && av[0].contains("MensajeMuerto"), "{av:#?}");
    // Muchas a la vez (más que voces): las de más esperan y suenan después.
    let (_, base, _) = c.abrir("ahi.device", 0, AHIR_TAMANO, 4);
    let muchas: Vec<u32> = (0..VOCES as usize + 4).map(|_| c.copia(base, AHIR_TAMANO)).collect();
    let largo = estereo(&mut c, 4800, 1, 1);
    for &m in &muchas {
        escribir(&mut c, m, largo, 4 * 4800, AHIST_S16S, 48_000, 0x1000, 0x8000, 0);
        c.io("SendIO", m);
    }
    c.pasar(110_000);
    let vueltas = muchas.iter().filter(|&&m| c.ha_vuelto(m)).count();
    assert_eq!(vueltas, VOCES as usize);
    c.pasar(100_000);
    assert!(muchas.iter().all(|&m| c.ha_vuelto(m)));
    let _ = Fallo::SinSitio;
    c.sin_avisos();
}
