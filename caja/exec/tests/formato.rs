//! `RawDoFmt` (`formato.rs`): el formato del autodoc, con el `stuffChar` de 68k, con `PutChProc` a 0 y con otra
//! función de 68k (un carácter cada vez); y lo malo: destinos y formatos fuera de la memoria.

mod comun;

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Continuacion, Destino, Memoria, Registros};
use comun::*;

fn datos(e: &mut caja_exec::Exec<caja_exec::MemoriaVec>, palabras: &[u16]) -> u32 {
    let d = e.alloc_mem(64, memf::CLEAR).unwrap();
    for (k, w) in palabras.iter().enumerate() {
        e.mem.escribir16(d + 2 * k as u32, *w).unwrap();
    }
    d
}

fn texto(e: &caja_exec::Exec<caja_exec::MemoriaVec>, d: u32) -> String {
    (0..200).map(|k| e.mem.leer8(d + k).unwrap()).take_while(|&b| b != 0).map(|b| b as char).collect()
}

#[test]
fn con_stuff_char_y_sin_proc() {
    let (mut e, _t) = caja_con_tarea();
    let s = cadena(&mut e, "Fish");
    let bstr = e.alloc_mem(8, memf::CLEAR).unwrap();
    e.mem.escribir8(bstr, 3).unwrap();
    e.mem.leer_mut(bstr + 1, 3).unwrap().copy_from_slice(b"abc");
    let f = cadena(&mut e, "%s have %ld eyes; %d %u %04x %-5s| %5.2s| %c %b 100%% %y");
    // %s (32), %ld (32), %d (16), %u (16), %04x (16), %-5s (32), %5.2s (32), %c (16), %b (32)
    let (sh, sl) = ((s >> 16) as u16, s as u16);
    let (bh, bl) = ((bstr >> 2 >> 16) as u16, (bstr >> 2) as u16);
    let d = datos(&mut e, &[sh, sl, 0, 2, 0xFFFF, 40000, 0xAB, sh, sl, sh, sl, b'!' as u16, bh, bl]);
    let stuff = caja_exec::stuff_char(&mut e).unwrap();
    for proc in [stuff, 0] {
        let buf = e.alloc_mem(128, memf::CLEAR).unwrap();
        let mut r = Registros::default();
        (r.a[0], r.a[1], r.a[2], r.a[3]) = (f, d, proc, buf);
        assert_eq!(llamar(&mut e, "RawDoFmt", &mut r), Accion::Seguir);
        assert_eq!(texto(&e, buf), "Fish have 2 eyes; -1 40000 00AB Fish |    Fi| ! abc 100% %y");
        assert_eq!(r.d[0], d + 28, "el final de los datos");
    }
    assert!(e.tomar_avisos().is_empty());
}

#[test]
fn con_put_ch_proc_de_68k() {
    let (mut e, _t) = caja_con_tarea();
    let f = cadena(&mut e, "a%dz");
    let d = datos(&mut e, &[7]);
    // Una función de 68k que no es el stuffChar (aquí, un rts: la prueba hace lo que haría el 68k).
    let proc = e.alloc_mem(4, memf::CLEAR).unwrap();
    e.mem.escribir16(proc, 0x4E75).unwrap();
    let mut r = Registros::default();
    (r.a[0], r.a[1], r.a[2], r.a[3]) = (f, d, proc, 1000);
    let mut a = llamar(&mut e, "RawDoFmt", &mut r);
    let mut vistos = Vec::new();
    // Cada carácter, una llamada a la función con D0 = el carácter y A3 = lo que dejó la anterior (aquí, +1).
    for _ in 0..10 {
        let Accion::Llamar { destino: Destino::Codigo68k(p), regs, tras } = a else { break };
        assert_eq!(p, proc);
        assert!(matches!(tras, Continuacion::Entrega { .. }));
        vistos.push((regs.d[0] as u8, regs.a[3]));
        let mut hecho = regs;
        hecho.a[3] += 1;
        a = e.continuar(tras, &hecho, &mut r);
    }
    assert_eq!(a, Accion::Seguir);
    assert_eq!(vistos, vec![(b'a', 1000), (b'7', 1001), (b'z', 1002), (0, 1003)]);
    assert_eq!(r.d[0], d + 2);
}

#[test]
fn hostil() {
    let (mut e, _t) = caja_con_tarea();
    let f = cadena(&mut e, "%s%s%s%s");
    let stuff = caja_exec::stuff_char(&mut e).unwrap();
    // Destino en la página cero, fuera de la memoria; formato fuera; datos fuera; un %s que apunta fuera.
    let d = datos(&mut e, &[0xFFFF, 0, 0, 0]);
    for (fmt, dat, dest) in [(f, d, 0x10), (f, d, 0xFFFF_0000), (0xFFFF_0000, d, 0x1000), (f, 0xFFFF_FFF0, 0x1000), (f, d, 0x1000)] {
        let mut r = Registros::default();
        (r.a[0], r.a[1], r.a[2], r.a[3]) = (fmt, dat, stuff, dest);
        assert_eq!(llamar(&mut e, "RawDoFmt", &mut r), Accion::Seguir);
    }
    assert_eq!(e.tomar_avisos().len(), 5);
    // Un formato largo (más que el tope de las cadenas): un aviso, nada escrito. Uno que crece mucho (80 campos de
    // 1000): se corta en 64 KB.
    let largo = e.alloc_mem(70_000, memf::CLEAR).unwrap();
    e.mem.leer_mut(largo, 69_999).unwrap().fill(b'x');
    let buf = e.alloc_mem(70_000, memf::CLEAR).unwrap();
    assert!(largo != 0 && buf != 0);
    let mut r = Registros::default();
    (r.a[0], r.a[1], r.a[2], r.a[3]) = (largo, d, 0, buf);
    llamar(&mut e, "RawDoFmt", &mut r);
    assert_eq!(e.mem.leer8(buf).unwrap(), 0);
    assert_eq!(e.tomar_avisos().len(), 1);
    let crece = cadena(&mut e, &"%1000d".repeat(80));
    let mut r = Registros::default();
    (r.a[0], r.a[1], r.a[2], r.a[3]) = (crece, d, 0, buf);
    llamar(&mut e, "RawDoFmt", &mut r);
    assert_ne!(e.mem.leer8(buf + 65535).unwrap(), 0);
    assert_eq!(e.mem.leer8(buf + 65536).unwrap(), 0);
    assert!(e.tomar_avisos().is_empty());
}
