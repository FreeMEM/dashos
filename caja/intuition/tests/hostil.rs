//! Lo malo: punteros nulos, en la página 0 o fuera de la memoria, mapas imposibles, tamaños negativos o enormes,
//! cerrar dos veces, mensajes sin contestar al cerrar, etiquetas en círculo, y llamadas con registros al azar.
//! Todo da un aviso o lo que dicen los autodocs; nada da un pánico.

mod comun;

use caja_exec::estructuras::{memf, msgport};
use caja_exec::{Memoria, lvo as exec_lvo};
use caja_intuition::estructuras::{bitmap, idcmp, rastport, tag, wa, wflg, window as win};
use caja_intuition::{Evento, Fallo, lvo};
use comun::*;

fn ultimo(c: &Caja) -> Fallo {
    c.1.avisos().last().expect("un aviso").fallo
}

#[test]
fn rastports_malos() {
    let mut c = caja();
    for rp in [0u32, 4, 0x800, TAMANO - 10, 0xFFFF_FFF0, 0x8000_0000] {
        let antes = c.1.avisos().len();
        for f in ["Move", "Draw", "RectFill", "SetRast", "Text", "SetAPen", "ClearScreen", "ScrollRaster"] {
            let mut r = regs(&[1, 2, 3, 4, 5, 6], &[cadena(&mut c, "x"), rp]);
            gfx(&mut c, f, &mut r);
            assert_eq!(r.d[0], 0, "{f} {rp:#x}");
        }
        assert_eq!(g(&mut c, "ReadPixel", &[0, 0], &[0, rp]) as i32, -1);
        assert_eq!(g(&mut c, "WritePixel", &[0, 0], &[0, rp]) as i32, -1);
        assert_eq!(c.1.avisos().len(), antes + 10);
    }
    assert!(matches!(c.1.avisos()[0].fallo, Fallo::RastPortMalo { dir: 0 }));
    assert_eq!(c.1.avisos()[0].funcion, "Move");
}

#[test]
fn bitmaps_malos() {
    let mut c = caja();
    // Más de 8 planos sin ser nativo.
    let bm = reservar(&mut c, 40);
    g(&mut c, "InitBitMap", &[9, 16, 16], &[bm]);
    let rp = rastport(&mut c, bm);
    rect(&mut c, rp, 0, 0, 5, 5);
    assert!(matches!(ultimo(&c), Fallo::BitMapMalo { .. }));
    // Un plano fuera de la memoria, o en la página 0.
    for p in [TAMANO - 4, 0x10, 0xFFF0_0000] {
        g(&mut c, "InitBitMap", &[1, 64, 64], &[bm]);
        e32(&mut c, bm + bitmap::PLANES, p);
        rect(&mut c, rp, 0, 0, 5, 5);
        assert!(matches!(ultimo(&c), Fallo::BitMapMalo { .. }), "{p:#x}");
    }
    // Filas enormes: el plano no cabe.
    g(&mut c, "InitBitMap", &[1, 65535, 65535], &[bm]);
    let r = g(&mut c, "AllocRaster", &[64, 64], &[]);
    e32(&mut c, bm + bitmap::PLANES, r);
    rect(&mut c, rp, 0, 0, 5, 5);
    assert!(matches!(ultimo(&c), Fallo::BitMapMalo { .. }));
    // Un mapa vacío (0 filas): no se dibuja nada, sin aviso.
    let n = c.1.avisos().len();
    g(&mut c, "InitBitMap", &[1, 0, 0], &[bm]);
    rect(&mut c, rp, 0, 0, 5, 5);
    assert_eq!(c.1.avisos().len(), n);
    // BitMap nulo.
    e32(&mut c, rp + rastport::BITMAP, 0);
    draw(&mut c, rp, 3, 3);
    assert!(matches!(ultimo(&c), Fallo::BitMapMalo { dir: 0 }));
    // BltBitMap con un mapa malo, y FreeBitMap de lo que no salió de AllocBitMap.
    assert_eq!(g(&mut c, "BltBitMap", &[0, 0, 0, 0, 4, 4, 0xC0, 0xFF], &[0x10, bm]), 0);
    g(&mut c, "FreeBitMap", &[], &[bm]);
    assert!(matches!(ultimo(&c), Fallo::NoReservado { .. }));
    g(&mut c, "FreeBitMap", &[], &[0]);
}

#[test]
fn tamanos_negativos_o_enormes() {
    let mut c = caja();
    for (an, al, prof) in [(0u32, 10u32, 8u32), (10, 0, 8), (0xFFFF_FFFF, 10, 32), (100_000, 100_000, 32), (10, 10, 0)]
    {
        assert_eq!(g(&mut c, "AllocBitMap", &[an, al, prof, 0], &[0]), 0);
        assert!(matches!(ultimo(&c), Fallo::TamanoMalo { .. }));
    }
    // Un planar que no cabe en la memoria: 0, sin aviso de tamaño.
    assert_eq!(g(&mut c, "AllocBitMap", &[8000, 8000, 8, 0], &[0]), 0);
    // Ventanas: negativas fallan; enormes se ajustan a la pantalla.
    let t = reservar(&mut c, 24);
    e32(&mut c, t, wa::WIDTH);
    e32(&mut c, t + 4, (-50i32) as u32);
    e32(&mut c, t + 8, tag::TAG_DONE);
    assert_eq!(i(&mut c, "OpenWindowTagList", &[], &[0, t]), 0);
    assert!(matches!(ultimo(&c), Fallo::TamanoMalo { .. }));
    e32(&mut c, t + 4, 30000);
    e32(&mut c, t + 8, wa::HEIGHT);
    e32(&mut c, t + 12, 30000);
    e32(&mut c, t + 16, tag::TAG_DONE);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    assert_ne!(w, 0);
    assert_eq!((l16(&c, w + win::WIDTH), l16(&c, w + win::HEIGHT)), (640, 480));
    // Coordenadas extremas en el dibujo: se recortan.
    let rp = l32(&c, w + win::RPORT);
    rect(&mut c, rp, -32768, -32768, 32767, 32767);
    g(&mut c, "ScrollRaster", &[32767, (-32768i32) as u32, 0, 0, 32767, 32767], &[0, rp]);
    g(&mut c, "BltTemplate", &[0, 2, 0, 0, 100, 100], &[0x10, rp]);
    assert!(matches!(ultimo(&c), Fallo::Exec(_)));
    // Un evento de tamaño absurdo: se queda en los límites.
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, Evento::Tamano { ancho: u32::MAX, alto: 0 });
    c.1.atender(&mut c.0);
    assert!(l16(&c, w + win::WIDTH) <= 640 && l16(&c, w + win::HEIGHT) >= 1);
}

#[test]
fn ventanas_malas_y_cerrar_dos_veces() {
    let mut c = caja();
    // CloseWindow(0) no hace nada; una que no es, aviso.
    i(&mut c, "CloseWindow", &[], &[0]);
    assert!(c.1.avisos().is_empty());
    for f in ["CloseWindow", "RefreshWindowFrame", "ModifyIDCMP", "SetWindowTitles", "MoveWindow", "ActivateWindow"] {
        i(&mut c, f, &[], &[0x2000]);
        assert!(matches!(ultimo(&c), Fallo::VentanaDesconocida { dir: 0x2000 }), "{f}");
    }
    let t = reservar(&mut c, 16);
    e32(&mut c, t, wa::IDCMP);
    e32(&mut c, t + 4, idcmp::CLOSEWINDOW);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    i(&mut c, "CloseWindow", &[], &[w]);
    let n = c.1.avisos().len();
    i(&mut c, "CloseWindow", &[], &[w]);
    assert_eq!(c.1.avisos().len(), n + 1);
    assert!(matches!(ultimo(&c), Fallo::VentanaDesconocida { .. }));
    // Dibujar en el RastPort de una ventana cerrada no rompe nada (su memoria está libre).
    // NewWindow fuera de la memoria, pantallas que no existen, etiquetas en círculo.
    assert_eq!(i(&mut c, "OpenWindow", &[], &[0xFFFF_0000]), 0);
    assert!(matches!(ultimo(&c), Fallo::Exec(_)));
    e32(&mut c, t, wa::CUSTOMSCREEN);
    e32(&mut c, t + 4, 0x1234);
    assert_eq!(i(&mut c, "OpenWindowTagList", &[], &[0, t]), 0);
    assert!(matches!(ultimo(&c), Fallo::PantallaDesconocida { .. }));
    e32(&mut c, t, tag::TAG_MORE);
    e32(&mut c, t + 4, t);
    assert_eq!(i(&mut c, "OpenWindowTagList", &[], &[0, t]), 0);
    assert!(matches!(ultimo(&c), Fallo::EtiquetasMalas { .. }));
    // Sin compositor: 0, y nada se queda reservado.
    let libre = c.0.avail_mem(memf::ANY).unwrap();
    c.1.comp.acepta = false;
    assert_eq!(i(&mut c, "OpenWindowTagList", &[], &[0, 0]), 0);
    assert_eq!(ultimo(&c), Fallo::SinCompositor);
    assert_eq!(c.0.avail_mem(memf::ANY).unwrap(), libre);
    assert!(c.1.ventanas().is_empty());
    assert_eq!(l32(&c, c.1.workbench() + 4), 0, "FirstWindow");
}

#[test]
fn mensajes_sin_contestar_al_cerrar() {
    let mut c = caja();
    let t = reservar(&mut c, 24);
    e32(&mut c, t, wa::IDCMP);
    e32(&mut c, t + 4, idcmp::CLOSEWINDOW | idcmp::MOUSEBUTTONS);
    e32(&mut c, t + 8, tag::TAG_DONE);
    let libre = c.0.avail_mem(memf::ANY).unwrap();
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    let s = c.1.superficie(w).unwrap();
    for _ in 0..3 {
        c.1.comp.meter(s, Evento::Cerrar);
    }
    c.1.atender(&mut c.0);
    // El programa coge uno y no lo contesta; los otros dos siguen en el puerto.
    let p = l32(&c, w + win::USERPORT);
    let m = c.0.get_msg(p).unwrap();
    assert_ne!(m, 0);
    i(&mut c, "CloseWindow", &[], &[w]);
    // Los del puerto se han soltado; la ventana espera al que tiene el programa.
    assert_eq!(c.1.cerradas_pendientes(), 1);
    assert!(c.1.ventanas().is_empty());
    // Lo contesta tarde: vuelve al WindowPort, que sigue vivo, y al atender se suelta todo.
    let mut r = regs(&[], &[0, m]);
    c.0.llamar(exec_lvo::por_nombre("ReplyMsg").unwrap(), &mut r);
    assert!(c.0.avisos().is_empty(), "{:?}", c.0.avisos());
    c.1.atender(&mut c.0);
    assert_eq!(c.1.cerradas_pendientes(), 0);
    assert_eq!(c.0.avail_mem(memf::ANY).unwrap(), libre);
    // Si la tarea muere con mensajes sin contestar, se sueltan igual.
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    let p = l32(&c, w + win::USERPORT);
    assert_ne!(c.0.get_msg(p).unwrap(), 0);
    c.1.tarea_muerta(&mut c.0, c.2);
    assert_eq!(c.1.cerradas_pendientes(), 0);
    assert!(c.1.ventanas().is_empty());
    assert_eq!(c.0.avail_mem(memf::ANY).unwrap(), libre);
}

#[test]
fn reply_de_lo_que_no_es_suyo() {
    let mut c = caja();
    let t = reservar(&mut c, 16);
    e32(&mut c, t, wa::IDCMP);
    e32(&mut c, t + 4, idcmp::CLOSEWINDOW);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    // Un mensaje cualquiera contestado al WindowPort de la ventana: aviso, y no se suelta.
    let m = reservar(&mut c, 20);
    let wp = l32(&c, w + win::WINDOWPORT);
    e32(&mut c, m + 14, wp);
    let mut r = regs(&[], &[0, m]);
    c.0.llamar(exec_lvo::por_nombre("ReplyMsg").unwrap(), &mut r);
    c.1.atender(&mut c.0);
    assert!(matches!(ultimo(&c), Fallo::Sospechoso { .. }));
    assert!(c.0.bloque(m).is_some());
    // Un puerto de usuario roto (la lista apunta a la página 0): los avisos de exec, sin pánico.
    let p = l32(&c, w + win::USERPORT);
    e32(&mut c, p + msgport::MP_MSGLIST, 0x10);
    let s = c.1.superficie(w).unwrap();
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    i(&mut c, "CloseWindow", &[], &[w]);
    assert!(!c.1.avisos().is_empty());
}

#[test]
fn fuentes_y_paletas_malas() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 16, 8);
    let s = cadena(&mut c, "abc");
    // Sin fuente: topaz y un aviso.
    e32(&mut c, rp + rastport::FONT, 0);
    g(&mut c, "Text", &[3], &[s, rp]);
    assert!(matches!(ultimo(&c), Fallo::FuenteMala { dir: 0 }));
    assert_eq!(l16(&c, rp + rastport::CP_X), 24);
    // Una TextFont rota (HiChar < LoChar, o CharLoc fuera).
    let f = reservar(&mut c, 52);
    e16(&mut c, f + 20, 8);
    c.0.mem.escribir8(f + 32, 200).unwrap();
    c.0.mem.escribir8(f + 33, 10).unwrap();
    assert_eq!(g(&mut c, "SetFont", &[], &[f, rp]), 0);
    assert!(matches!(ultimo(&c), Fallo::FuenteMala { .. }));
    c.0.mem.escribir8(f + 32, 32).unwrap();
    c.0.mem.escribir8(f + 33, 126).unwrap();
    e32(&mut c, f + 40, 0xFFFF_0000);
    e32(&mut c, rp + rastport::FONT, f);
    g(&mut c, "Text", &[3], &[s, rp]);
    assert!(matches!(ultimo(&c), Fallo::FuenteMala { .. }));
    // CloseFont de más.
    let topaz = c.1.topaz();
    g(&mut c, "CloseFont", &[], &[0, topaz]);
    g(&mut c, "CloseFont", &[], &[0, topaz]);
    assert!(matches!(ultimo(&c), Fallo::FuenteMala { .. }));
    // Un ViewPort con un ColorMap que no es de graphics.
    let vp = reservar(&mut c, 40);
    e32(&mut c, vp + 4, 0x5000);
    g(&mut c, "SetRGB4", &[1, 1, 1, 1], &[vp]);
    assert!(matches!(ultimo(&c), Fallo::ColorMapMalo { .. }));
    g(&mut c, "SetRGB4", &[1, 1, 1, 1], &[0]);
    assert!(matches!(ultimo(&c), Fallo::Exec(_)));
    // LoadRGB32 sin fin (tramos que no acaban en 0 dentro de lo razonable).
    let wb = c.1.workbench();
    let t = reservar(&mut c, 0x1000);
    for k in 0..256 {
        e32(&mut c, t + k * 16, 1 << 16);
    }
    g(&mut c, "LoadRGB32", &[], &[wb + 44, t]);
    assert!(!c.1.avisos().is_empty());
    // El ColorMap del Workbench no se puede soltar.
    let cm = l32(&c, wb + 48);
    g(&mut c, "FreeColorMap", &[], &[cm]);
    assert!(matches!(ultimo(&c), Fallo::Sospechoso { .. }));
}

#[test]
fn funciones_que_faltan() {
    let mut c = caja();
    assert_eq!(g(&mut c, "Flood", &[1, 2, 3], &[0, 0]), 0);
    assert!(matches!(ultimo(&c), Fallo::NoImplementada { .. }));
    assert_eq!(i(&mut c, "SetPrefs", &[], &[0x2000]), 0);
    assert_eq!(c.1.avisos().last().unwrap().funcion, "SetPrefs");
}

/// Un generador sencillo (xorshift) para las llamadas al azar.
struct Azar(u64);
impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn de(&mut self, v: &[u32]) -> u32 {
        v[(self.sig() % v.len() as u64) as usize]
    }
}

/// Todas las funciones con registros al azar: punteros buenos (los RastPort, BitMap, ventanas, fuentes y
/// ColorMap de verdad), malos y números pequeños o extremos, con eventos del compositor y la memoria de las
/// estructuras estropeada a ratos. Ningún pánico.
#[test]
fn llamadas_al_azar() {
    for semilla in [0x1234_5678_9ABC_DEF1u64, 0xDEAD_BEEF_0BAD_F00D, 0x0123_4567_89AB_CDEF, 0x7777_1111_3333_5555] {
        al_azar(semilla, 100_000);
    }
}

fn al_azar(semilla: u64, vueltas: u32) {
    let mut c = caja();
    let mut az = Azar(semilla);
    let mut buenos = vec![c.1.workbench(), c.1.topaz(), c.1.gfx_base(), c.1.intuition_base()];
    let (bn, rpn) = nativo(&mut c, 48, 32);
    let (bp, rpp) = planar(&mut c, 3, 48, 32);
    buenos.extend([bn, rpn, bp, rpp]);
    let t = reservar(&mut c, 64);
    e32(&mut c, t, wa::IDCMP);
    e32(&mut c, t + 4, 0xFFFF_FFFF);
    e32(&mut c, t + 8, wa::FLAGS);
    e32(&mut c, t + 12, wflg::REPORTMOUSE | wflg::CLOSEGADGET | wflg::SIZEGADGET);
    e32(&mut c, t + 16, wa::WIDTH);
    e32(&mut c, t + 20, 60);
    e32(&mut c, t + 24, wa::HEIGHT);
    e32(&mut c, t + 28, 40);
    let w = i(&mut c, "OpenWindowTagList", &[], &[0, t]);
    buenos.extend([w, l32(&c, w + win::RPORT), t, cadena(&mut c, "Workbench")]);
    let malos = [0u32, 4, 0x10, 0xFFF, TAMANO - 2, TAMANO, 0xFFFF_FFFF, 0x8000_0000];
    let numeros = [0u32, 1, 2, 3, 7, 8, 15, 31, 32, 255, 256, 0xFFFF, 0x8000, (-1i32) as u32, (-5i32) as u32];
    let (ng, ni) = (lvo::GRAPHICS.len() as u64, lvo::INTUITION.len() as u64);
    let total = ng + ni + lvo::GADTOOLS.len() as u64;
    let numero = |c: &Caja, f: u64| {
        if f < ng {
            c.1.numero_graphics(lvo::GRAPHICS[f as usize].0).unwrap()
        } else if f < ng + ni {
            c.1.numero_intuition(lvo::INTUITION[(f - ng) as usize].0).unwrap()
        } else {
            c.1.numero_gadtools(lvo::GADTOOLS[(f - ng - ni) as usize].0).unwrap()
        }
    };
    // Las que están hechas (las otras dan `NoImplementada` con cualquier cosa).
    let mut hechas = Vec::new();
    for f in 0..total {
        let n = numero(&c, f);
        let mut r = regs(&[], &[]);
        c.1.llamar(&mut c.0, n, &mut r);
        let no = c.1.tomar_avisos().iter().any(|a| matches!(a.fallo, Fallo::NoImplementada { .. }));
        if !no {
            hechas.push(f);
        }
    }
    assert!(hechas.len() > 90, "{}", hechas.len());
    let mut sin_aviso = 0;
    for k in 0..vueltas {
        let f = if az.sig().is_multiple_of(5) {
            az.sig() % total
        } else {
            hechas[(az.sig() % hechas.len() as u64) as usize]
        };
        let n = numero(&c, f);
        let mut r = regs(&[], &[]);
        for x in r.d.iter_mut().chain(r.a.iter_mut()) {
            *x = match az.sig() % 4 {
                0 => az.de(&buenos),
                1 => az.de(&malos),
                2 => (az.sig() % 64) as u32,
                _ => az.de(&numeros),
            };
        }
        // Nada de crear miles de ventanas o mapas.
        let nombre = c.1.numero_graphics("AllocBitMap");
        if Some(n) == nombre {
            r.d[0] &= 63;
            r.d[1] &= 63;
        }
        c.1.llamar(&mut c.0, n, &mut r);
        if c.1.ventanas().len() > 8 {
            for v in c.1.ventanas().into_iter().skip(1) {
                c.1.close_window(&mut c.0, v).ok();
            }
        }
        if k % 97 == 0 {
            for v in c.1.ventanas() {
                if let Some(s) = c.1.superficie(v) {
                    let e = match az.sig() % 6 {
                        0 => Evento::Raton { x: (az.sig() % 80) as i32 - 10, y: (az.sig() % 60) as i32 - 10 },
                        1 => {
                            Evento::Boton { boton: 0x110 + (az.sig() % 3) as u32, pulsado: az.sig().is_multiple_of(2) }
                        }
                        2 => Evento::Tecla {
                            codigo: (az.sig() % 140) as u32,
                            pulsada: az.sig().is_multiple_of(2),
                            repeticion: az.sig().is_multiple_of(4),
                            caracter: None,
                        },
                        3 => Evento::Tamano { ancho: (az.sig() % 100) as u32, alto: (az.sig() % 100) as u32 },
                        4 => Evento::Cerrar,
                        _ => Evento::Foco(az.sig().is_multiple_of(2)),
                    };
                    c.1.comp.meter(s, e);
                }
            }
            c.1.atender(&mut c.0);
            // Vaciar los puertos de vez en cuando, contestando.
            for v in c.1.ventanas() {
                if let Ok(p) = c.0.mem.leer32(v + win::USERPORT)
                    && p != 0
                {
                    for _ in 0..64 {
                        let Ok(m) = c.0.get_msg(p) else { break };
                        if m == 0 {
                            break;
                        }
                        let mut r = regs(&[], &[0, m]);
                        c.0.llamar(exec_lvo::por_nombre("ReplyMsg").unwrap(), &mut r);
                    }
                }
            }
        }
        // A ratos, estropear un byte de una estructura buena.
        if k % 1009 == 0 {
            let d = az.de(&buenos) + (az.sig() % 100) as u32;
            let v = az.sig() as u8;
            let _ = c.0.mem.escribir8(d, v);
        }
        if c.1.tomar_avisos().is_empty() {
            sin_aviso += 1;
        }
        c.0.tomar_avisos();
    }
    // Que no todo se quede en la puerta: una buena parte de las llamadas llega a hacer algo.
    assert!(sin_aviso > vueltas / 10, "{sin_aviso} de {vueltas}");
    eprintln!("semilla {semilla:#x}: {sin_aviso} de {vueltas} sin aviso");
}
