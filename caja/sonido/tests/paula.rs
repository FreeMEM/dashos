//! audio.device: asignar canales (al abrir y con ADCMD_ALLOCATE), escribir (las muestras se leen de la memoria del
//! 68k al mezclar), robar un canal, las órdenes de ciclo, ProTracker y lo hostil.

mod comun;

use audiodevice::{
    ADCMD_ALLOCATE, ADCMD_FINISH, ADCMD_FREE, ADCMD_PERVOL, ADIOERR_ALLOCFAILED, ADIOERR_CHANNELSTOLEN,
    ADIOERR_NOALLOCATION, ADIOF_NOWAIT, ADIOF_PERVOL, ADIOF_SYNCCYCLE, ADIOF_WRITEMESSAGE,
};
use caja_exec::estructuras::{message, node};
use caja_exec::{Accion, Memoria, Registros, io};
use caja_sonido::*;
use comun::*;

/// OpenDevice de audio.device con estas combinaciones (vacío: sin asignar) y precedencia. (puerto, petición, error).
fn abrir_audio(c: &mut Caja, combinaciones: &[u8], prec: i8) -> (u32, u32, i8) {
    let p = c.e.create_msg_port().unwrap();
    let ior = c.e.create_io_request(p, IOA_TAMANO).unwrap();
    if !combinaciones.is_empty() {
        let d = c.bytes(combinaciones);
        c.e.mem.escribir32(ior + IOA_DATA, d).unwrap();
        c.e.mem.escribir32(ior + IOA_LENGTH, combinaciones.len() as u32).unwrap();
    }
    c.e.mem.escribir8(ior + node::LN_PRI, prec as u8).unwrap();
    let nombre = c.cadena("audio.device");
    let mut r = Registros::default();
    r.a[0] = nombre;
    r.a[1] = ior;
    assert_eq!(c.exec("OpenDevice", &mut r), Accion::Seguir);
    (p, ior, r.d[0] as i8)
}

#[allow(clippy::too_many_arguments)]
fn escribir(c: &mut Caja, ior: u32, datos: u32, largo: u32, periodo: u16, volumen: u16, ciclos: u16, banderas: u8) {
    let m = &mut c.e.mem;
    m.escribir16(ior + io::IO_COMMAND, audiodevice::CMD_WRITE).unwrap();
    m.escribir8(ior + io::IO_FLAGS, banderas | ADIOF_PERVOL).unwrap();
    m.escribir32(ior + IOA_DATA, datos).unwrap();
    m.escribir32(ior + IOA_LENGTH, largo).unwrap();
    m.escribir16(ior + IOA_PERIOD, periodo).unwrap();
    m.escribir16(ior + IOA_VOLUME, volumen).unwrap();
    m.escribir16(ior + IOA_CYCLES, ciclos).unwrap();
}

/// `BeginIO` como lo llama un programa (amiga.lib): directo al dispositivo, con sus banderas (`SendIO` y `DoIO`
/// las borran, y audio.device las necesita: `ADIOF_PERVOL`...).
fn begin(c: &mut Caja, ior: u32) {
    c.e.mem.escribir8(ior + node::LN_TYPE, caja_exec::estructuras::nt::MESSAGE).unwrap();
    let n = c.s.numero("audio.device BeginIO").unwrap();
    let mut r = Registros::default();
    r.a[1] = ior;
    r.a[6] = c.s.base_audio();
    c.s.llamar(&mut c.e, n, &mut r);
}

fn orden(c: &mut Caja, ior: u32, o: u16, banderas: u8) {
    c.e.mem.escribir16(ior + io::IO_COMMAND, o).unwrap();
    c.e.mem.escribir8(ior + io::IO_FLAGS, banderas).unwrap();
}

fn unidad(c: &Caja, ior: u32) -> u32 {
    c.e.mem.leer32(ior + io::IO_UNIT).unwrap()
}

fn llave(c: &Caja, ior: u32) -> u16 {
    c.e.mem.leer16(ior + IOA_ALLOCKEY).unwrap()
}

/// Período para unas 4.800 muestras por segundo (10 cuadros cada una).
const PERIODO: u16 = 739;

#[test]
fn asignar_al_abrir_escribir_y_volver_a_tiempo() {
    let mut c = caja();
    let (_, ior, e) = abrir_audio(&mut c, &[1, 2, 4, 8], 0);
    assert_eq!(e, 0);
    assert_eq!(unidad(&c, ior), 1, "el primer canal libre");
    assert_ne!(llave(&c, ior), 0);
    assert_eq!(c.e.mem.leer32(ior + io::IO_DEVICE).unwrap(), c.s.base_audio());
    // 32 muestras, 10 ciclos: 320 muestras a 3.546.895 / 739 Hz = 66,67 ms.
    let w = c.copia(ior, IOA_TAMANO);
    let m = c.bytes(&[64; 32]);
    escribir(&mut c, w, m, 32, PERIODO, 64, 10, 0);
    let t0 = c.ahora + 1_000;
    begin(&mut c, w);
    c.pasar(60_000);
    assert!(!c.ha_vuelto(w));
    let mut cuando = 0;
    for _ in 0..20 {
        c.pasar(1_000);
        if c.recoger(w) {
            cuando = c.ahora;
            break;
        }
    }
    assert!(cuando >= t0 + 66_666 && cuando <= t0 + 66_666 + 7_000, "{}", cuando - t0);
    assert_eq!(c.error(w), 0);
    // A la izquierda (canal 0), a toda escala: 64 × 64 × 2.
    assert_eq!(c.salida.escritos[100], [8192, 0]);
    c.sin_avisos();
}

#[test]
fn las_muestras_se_leen_de_la_memoria_chip_al_mezclar() {
    let mut c = caja();
    let (_, ior, _) = abrir_audio(&mut c, &[2], 0);
    assert_eq!(unidad(&c, ior), 2);
    let w = c.copia(ior, IOA_TAMANO);
    let m = c.bytes(&[32; 64]);
    escribir(&mut c, w, m, 64, PERIODO, 64, 0, 0);
    begin(&mut c, w);
    c.pasar(20_000);
    let ya = c.salida.escritos.len();
    // El programa rellena su anillo mientras suena (ciclos 0, sin fin): se oye lo nuevo.
    c.e.mem.leer_mut(m, 64).unwrap().fill(-16i8 as u8);
    c.pasar(20_000);
    let s = &c.salida.escritos;
    assert!(s[..ya].iter().all(|x| *x == [0, 32 * 64 * 2]));
    assert!(s[ya..].iter().all(|x| *x == [0, -16 * 64 * 2]));
    // CMD_READ: la escritura que suena.
    let r = c.copia(ior, IOA_TAMANO);
    orden(&mut c, r, audiodevice::CMD_READ, io::IOF_QUICK);
    assert_eq!(c.io("DoIO", r).1, 0);
    assert_eq!(c.e.mem.leer32(r + IOA_DATA).unwrap(), w);
    // ADCMD_PERVOL en el acto (sin SYNCCYCLE): con DoIO, rápida.
    let pv = c.copia(ior, IOA_TAMANO);
    orden(&mut c, pv, ADCMD_PERVOL, 0);
    c.e.mem.escribir16(pv + IOA_PERIOD, PERIODO).unwrap();
    c.e.mem.escribir16(pv + IOA_VOLUME, 32).unwrap();
    assert_eq!(c.io("DoIO", pv), (Accion::Seguir, 0));
    let n = c.salida.escritos.len();
    c.pasar(10_000);
    assert_eq!(c.salida.escritos[n], [0, -16 * 32 * 2]);
    // ADCMD_FINISH con SYNCCYCLE: al acabar el ciclo, la escritura sin fin vuelve abortada.
    let f = c.copia(ior, IOA_TAMANO);
    orden(&mut c, f, ADCMD_FINISH, ADIOF_SYNCCYCLE);
    c.io("SendIO", f);
    c.pasar(30_000);
    assert!(c.recoger(w) && c.recoger(f));
    assert_eq!(c.error(w), io::IOERR_ABORTED);
    c.pasar(50_000);
    assert!(!c.s.activo());
    c.sin_avisos();
}

#[test]
fn doio_y_writemessage() {
    let mut c = caja();
    let (_, ior, _) = abrir_audio(&mut c, &[1], 0);
    let w = c.copia(ior, IOA_TAMANO);
    let m = c.bytes(&[1; 32]);
    let p2 = c.e.create_msg_port().unwrap();
    escribir(&mut c, w, m, 32, PERIODO, 64, 2, ADIOF_WRITEMESSAGE);
    c.e.mem.escribir32(w + IOA_WRITEMSG + message::MN_REPLYPORT, p2).unwrap();
    begin(&mut c, w);
    assert_eq!(c.io("WaitIO", w).0, Accion::Bloquear);
    // Cuando empieza a sonar, su ioa_WriteMsg vuelve a su puerto.
    c.pasar(2_000);
    assert_eq!(c.e.get_msg(p2).unwrap(), w + IOA_WRITEMSG);
    c.pasar(20_000);
    assert_eq!(c.e.elegir_siguiente(), Some(c.tarea));
    assert_eq!(c.io("WaitIO", w), (Accion::Seguir, 0));
    // DoIO (sin banderas, como lo deja exec): espera a que suene y no se manda dos veces.
    escribir(&mut c, w, m, 32, PERIODO, 64, 1, 0);
    assert_eq!(c.io("DoIO", w).0, Accion::Bloquear);
    c.pasar(20_000);
    assert_eq!(c.e.elegir_siguiente(), Some(c.tarea));
    assert_eq!(c.io("DoIO", w), (Accion::Seguir, 0));
    c.sin_avisos();
}

#[test]
fn robar_un_canal() {
    let mut c = caja();
    let (_, a, _) = abrir_audio(&mut c, &[2], 0);
    let wa = c.copia(a, IOA_TAMANO);
    let m = c.bytes(&[10; 32]);
    escribir(&mut c, wa, m, 32, PERIODO, 64, 0, 0);
    begin(&mut c, wa);
    c.pasar(10_000);
    // Otro, con más precedencia, se lo lleva: lo que sonaba vuelve con ADIOERR_CHANNELSTOLEN, en el acto.
    let (_, b, e) = abrir_audio(&mut c, &[], 0);
    assert_eq!(e, 0);
    let d = c.bytes(&[2]);
    orden(&mut c, b, ADCMD_ALLOCATE, ADIOF_NOWAIT);
    c.e.mem.escribir32(b + IOA_DATA, d).unwrap();
    c.e.mem.escribir32(b + IOA_LENGTH, 1).unwrap();
    c.e.mem.escribir8(b + node::LN_PRI, 10).unwrap();
    assert_eq!(c.io("DoIO", b), (Accion::Seguir, 0));
    assert_eq!(unidad(&c, b), 2);
    assert_ne!(llave(&c, b), llave(&c, a));
    assert!(c.recoger(wa));
    assert_eq!(c.error(wa), ADIOERR_CHANNELSTOLEN);
    // El de antes ya no puede escribir en él.
    escribir(&mut c, wa, m, 32, PERIODO, 64, 1, 0);
    assert_eq!(c.io("DoIO", wa).1 as i8, ADIOERR_NOALLOCATION);
    // Con menos precedencia y sin esperar, no se lo quita.
    let (_, x, e) = abrir_audio(&mut c, &[2], -5);
    assert_eq!(e, ADIOERR_ALLOCFAILED);
    assert_eq!(c.e.mem.leer32(x + io::IO_DEVICE).unwrap(), 0);
    c.sin_avisos();
}

/// ProTracker: abre audio.device sin asignar al arrancar, después se queda los cuatro canales; al cerrar, se sueltan.
#[test]
fn protracker_abre_sin_asignar_y_se_queda_los_cuatro() {
    let mut c = caja();
    let (_, ior, e) = abrir_audio(&mut c, &[], 0);
    assert_eq!(e, 0);
    let d = c.bytes(&[15]);
    orden(&mut c, ior, ADCMD_ALLOCATE, ADIOF_NOWAIT);
    c.e.mem.escribir32(ior + IOA_DATA, d).unwrap();
    c.e.mem.escribir32(ior + IOA_LENGTH, 1).unwrap();
    c.e.mem.escribir8(ior + node::LN_PRI, 127u8).unwrap();
    assert_eq!(c.io("DoIO", ior), (Accion::Seguir, 0));
    assert_eq!(unidad(&c, ior), 15);
    assert_eq!(abrir_audio(&mut c, &[1, 2, 4, 8], 0).2, ADIOERR_ALLOCFAILED);
    c.io("CloseDevice", ior);
    assert_eq!(abrir_audio(&mut c, &[15], 0).2, 0);
    // Una asignación que espera (sin NOWAIT) se queda en marcha y vuelve cuando se sueltan.
    let (_, y, _) = abrir_audio(&mut c, &[], 0);
    orden(&mut c, y, ADCMD_ALLOCATE, 0);
    let d1 = c.bytes(&[1]);
    c.e.mem.escribir32(y + IOA_DATA, d1).unwrap();
    c.e.mem.escribir32(y + IOA_LENGTH, 1).unwrap();
    c.io("SendIO", y);
    assert!(!c.ha_vuelto(y));
    let todas = c.e.mem.leer32(c.e.mem.leer32(y + message::MN_REPLYPORT).unwrap()).unwrap();
    let _ = todas;
    c.sin_avisos();
}

#[test]
fn cerrar_abortar_y_tarea_que_muere() {
    let mut c = caja();
    let (_, a, _) = abrir_audio(&mut c, &[1], 0);
    let w = c.copia(a, IOA_TAMANO);
    let m = c.bytes(&[5; 32]);
    assert_eq!(c.io("AbortIO", w).1, u32::MAX);
    escribir(&mut c, w, m, 32, PERIODO, 64, 0, 0);
    begin(&mut c, w);
    c.pasar(5_000);
    assert_eq!(c.io("AbortIO", w).1, 0);
    assert!(c.recoger(w));
    assert_eq!(c.error(w), io::IOERR_ABORTED);
    // Cerrar con una escritura en marcha: vuelve abortada y el canal queda libre.
    escribir(&mut c, w, m, 32, PERIODO, 64, 0, 0);
    begin(&mut c, w);
    c.pasar(5_000);
    c.io("CloseDevice", a);
    assert!(c.recoger(w));
    assert_eq!(c.error(w), io::IOERR_ABORTED);
    // Otra tarea se queda el canal 0 y escribe; muere: se olvida y el canal se suelta; una asignación de la primera
    // que esperaba ese canal vuelve en el siguiente atender.
    let t2 = c.e.crear_tarea("otra", 0, 4096, 0x10_0000).unwrap();
    c.e.poner_en_marcha(t2).unwrap();
    let (_, b, e) = abrir_audio(&mut c, &[1], 0);
    assert_eq!(e, 0);
    let wb = c.copia(b, IOA_TAMANO);
    escribir(&mut c, wb, m, 32, PERIODO, 64, 0, 0);
    begin(&mut c, wb);
    c.e.poner_en_marcha(c.tarea).unwrap();
    let (_, y, _) = abrir_audio(&mut c, &[], 0);
    orden(&mut c, y, ADCMD_ALLOCATE, 0);
    let d1 = c.bytes(&[1]);
    c.e.mem.escribir32(y + IOA_DATA, d1).unwrap();
    c.e.mem.escribir32(y + IOA_LENGTH, 1).unwrap();
    c.io("SendIO", y);
    c.pasar(5_000);
    assert!(!c.ha_vuelto(y));
    c.s.tarea_muerta(t2);
    c.e.matar_tarea(t2).unwrap();
    c.pasar(5_000);
    assert!(c.recoger(y));
    assert_eq!((c.error(y), unidad(&c, y)), (0, 1));
    assert!(!c.ha_vuelto(wb));
    // ADCMD_FREE suelta.
    orden(&mut c, y, ADCMD_FREE, 0);
    assert_eq!(c.io("DoIO", y).1, 0);
    c.pasar(50_000);
    assert!(!c.s.activo());
    c.sin_avisos();
}

#[test]
fn hostil() {
    let mut c = caja();
    let (_, a, _) = abrir_audio(&mut c, &[1], 0);
    let w = c.copia(a, IOA_TAMANO);
    let m = c.bytes(&[1; 64]);
    let casos: [(u32, u32, i8); 6] = [
        (0x10, 32, io::IOERR_BADADDRESS),        // la página 0
        (TAMANO - 16, 32, io::IOERR_BADADDRESS), // se sale
        (0xFFFF_FFF0, 32, io::IOERR_BADADDRESS),
        (m, 200_000, io::IOERR_BADLENGTH),    // más de lo que lee Paula
        (m, 1, audiodevice::IOERR_BADLENGTH), // un byte (Paula lee palabras)
        (m, 0xFFFF_FFFF, io::IOERR_BADLENGTH),
    ];
    for (k, &(d, l, err)) in casos.iter().enumerate() {
        escribir(&mut c, w, d, l, PERIODO, 64, 1, 0);
        assert_eq!(c.io("DoIO", w), (Accion::Seguir, err as i32 as u32), "caso {k}");
    }
    assert_eq!(c.avisos().len(), 3);
    // Un largo impar: suena lo par. Período 0 y volumen 1.000: recortados por el corazón.
    escribir(&mut c, w, m, 3, 0, 1000, 1, 0);
    begin(&mut c, w);
    c.pasar(5_000);
    assert!(c.recoger(w));
    assert_eq!(c.error(w), 0);
    // Al final justo de la memoria, vale.
    let fin = TAMANO - 2;
    escribir(&mut c, w, fin, 2, PERIODO, 64, 1, 0);
    begin(&mut c, w);
    c.pasar(5_000);
    assert!(c.recoger(w));
    // Mandarla dos veces mientras suena: aviso, una sola vuelta.
    escribir(&mut c, w, m, 64, PERIODO, 64, 2, 0);
    begin(&mut c, w);
    begin(&mut c, w);
    assert!(c.avisos().iter().any(|x| x.contains("MensajeEnCola")));
    c.pasar(50_000);
    assert!(c.recoger(w));
    assert!(!c.recoger(w));
    // Combinaciones en un puntero malo: IOERR_BADADDRESS. Una lista vacía con NOWAIT: falla.
    orden(&mut c, w, ADCMD_ALLOCATE, ADIOF_NOWAIT);
    c.e.mem.escribir32(w + IOA_DATA, 0x20).unwrap();
    c.e.mem.escribir32(w + IOA_LENGTH, 4).unwrap();
    assert_eq!(c.io("DoIO", w).1 as i8, io::IOERR_BADADDRESS);
    // (Con DoIO, exec deja io_Flags en IOF_QUICK: sin ADIOF_NOWAIT, esperaría para siempre, como en un Amiga.)
    c.e.mem.escribir32(w + IOA_LENGTH, 0).unwrap();
    orden(&mut c, w, ADCMD_ALLOCATE, ADIOF_NOWAIT);
    begin(&mut c, w);
    assert!(c.recoger(w));
    assert_eq!(c.error(w), ADIOERR_ALLOCFAILED);
    assert_eq!(c.avisos().len(), 1);
    // Una orden que no existe.
    orden(&mut c, w, 0x55, 0);
    assert_eq!(c.io("DoIO", w).1 as i8, io::IOERR_NOCMD);
    // Abrir con una petición demasiado corta, o fuera: aviso y no se abre.
    let p = c.e.create_msg_port().unwrap();
    let corta = c.e.create_io_request(p, 32).unwrap();
    let nombre = c.cadena("audio.device");
    for ior in [corta, 0x10, 0xFFFF_FFF8] {
        let mut r = Registros::default();
        r.a[0] = nombre;
        r.a[1] = ior;
        c.exec("OpenDevice", &mut r);
    }
    assert!(!c.avisos().is_empty());
    // Todos los números con registros basura: avisos, nunca un pánico.
    for v in [0u32, 4, 0x800, TAMANO - 8, 0xFFFF_FFF0, u32::MAX] {
        for n in N_PAULA {
            let mut r = Registros { d: [v; 8], a: [v; 8] };
            let num = c.s.numero(n).unwrap();
            assert_eq!(c.s.llamar(&mut c.e, num, &mut r), Accion::Seguir);
        }
    }
    c.avisos();
    // Soltar la petición mientras suena (sin AbortIO): al acabar, aviso y nada más.
    escribir(&mut c, w, m, 64, PERIODO, 64, 1, 0);
    begin(&mut c, w);
    c.e.mem.escribir8(w + node::LN_TYPE, 0).unwrap();
    c.e.free_mem(w, IOA_TAMANO).unwrap();
    c.pasar(30_000);
    let av = c.avisos();
    assert!(av.len() == 1 && av[0].contains("MensajeMuerto"), "{av:#?}");
}

const N_PAULA: [&str; 4] = ["audio.device Open", "audio.device Close", "audio.device BeginIO", "audio.device AbortIO"];
