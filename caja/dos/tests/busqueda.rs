//! MatchFirst, MatchNext y MatchEnd.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::*;

const AP_BASE: u32 = 0;
const AP_LAST: u32 = 4;
const AP_FLAGS: u32 = 16;
const AP_STRLEN: u32 = 18;
const AP_INFO: u32 = 20;
const AP_BUF: u32 = 280;
const AN_LOCK: u32 = 8;
const APF_ITSWILD: u8 = 2;
const APF_DODIR: u8 = 4;
const APF_DIDDIR: u8 = 8;
const APF_DIRCHANGED: u8 = 64;

/// Un AnchorPath con `largo` bytes de ap_Buf.
fn anchor(b: &mut Banco, largo: u16) -> u32 {
    let ap = b.hueco(AP_BUF + largo as u32 + 4);
    b.caja.mem.escribir16(ap + AP_STRLEN, largo);
    ap
}

/// Todo lo que casa con `patron`: (nombre en ap_Info, ruta en ap_Buf). Acaba con MatchEnd.
fn buscar(b: &mut Banco, patron: &str) -> (Vec<(String, String)>, u32) {
    let ap = anchor(b, 128);
    let p = b.cad(patron);
    let mut v = Vec::new();
    let mut r = b.llamar(lvo::MATCH_FIRST, &[p, ap]);
    while r == 0 {
        v.push((b.nombre_fib(ap + AP_INFO), b.cadena_en(ap + AP_BUF)));
        assert!(v.len() < 1000, "no acaba");
        r = b.llamar(lvo::MATCH_NEXT, &[ap]);
    }
    b.llamar(lvo::MATCH_END, &[ap]);
    assert_eq!(b.l32(ap + AP_BASE), 0);
    (v, r)
}

fn assign_lock(b: &mut Banco, nombre: &str, ruta: &str) {
    let l = b.lock(ruta, SHARED_LOCK);
    assert_ne!(l, 0, "{ruta}");
    let n = b.cad(nombre);
    assert_eq!(b.llamar(lvo::ASSIGN_LOCK, &[n, l]), DOSTRUE);
}

#[test]
fn comodines_en_un_cajon() {
    let mut b = Banco::nuevo();
    for n in ["ILBM", "PNG", "JPEG", "ILBM.info", "Leeme.txt"] {
        b.dos.fs.fs.poner(b.work, &format!("DataTypes/{n}"), n.as_bytes());
    }
    b.dos.fs.fs.poner_cajon(b.work, "DataTypes/sub");
    let (v, r) = buscar(&mut b, "Work:DataTypes/~(#?.info)");
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    let mut nombres: Vec<_> = v.iter().map(|(n, _)| n.as_str()).collect();
    nombres.sort();
    assert_eq!(nombres, ["ILBM", "JPEG", "Leeme.txt", "PNG", "sub"]);
    assert!(v.iter().any(|(n, ruta)| n == "PNG" && ruta == "Work:DataTypes/PNG"), "{v:?}");
    // Sin distinguir mayúsculas, como ParsePatternNoCase.
    let (v, _) = buscar(&mut b, "work:datatypes/#?.INFO");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].0, "ILBM.info");
    // Un cajón vacío de lo que casa: no hay más (no "no existe").
    let (v, r) = buscar(&mut b, "Work:DataTypes/#?.zzz");
    assert!(v.is_empty());
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    b.limpio(0, 1);
}

#[test]
fn el_cerrojo_del_cajon_y_las_banderas() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "d/a", b"1");
    b.dos.fs.fs.poner(b.work, "d/b", b"2");
    let ap = anchor(&mut b, 0);
    let p = b.cad("Work:d/#?");
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), 0);
    let f = b.caja.mem.leer8(ap + AP_FLAGS).unwrap();
    assert_ne!(f & APF_ITSWILD, 0);
    assert_ne!(f & APF_DIRCHANGED, 0, "el primer cajón cambia el directorio");
    // an_Lock es un cerrojo de verdad del cajón: vale para CurrentDir y NameFromLock.
    let ach = b.l32(ap + AP_LAST);
    let l = b.l32(ach + AN_LOCK);
    assert_eq!(b.name_from_lock(l), "Work:d");
    assert_eq!(b.cadena_en(ap + AP_BUF), "", "sin ap_Strlen no se escribe la ruta");
    assert_eq!(b.llamar(lvo::MATCH_NEXT, &[ap]), 0);
    assert_eq!(b.caja.mem.leer8(ap + AP_FLAGS).unwrap() & APF_DIRCHANGED, 0);
    assert_eq!(b.llamar(lvo::MATCH_NEXT, &[ap]), ERROR_NO_MORE_ENTRIES);
    assert_eq!(b.io_err(), ERROR_NO_MORE_ENTRIES);
    b.llamar(lvo::MATCH_END, &[ap]);
    b.limpio(0, 1);
    assert!(b.avisos().is_empty());
}

#[test]
fn sin_comodines() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "d/Uno", b"1");
    let (v, r) = buscar(&mut b, "work:d/uno");
    assert_eq!(v, [("Uno".to_string(), "work:d/Uno".to_string())]);
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    let ap = anchor(&mut b, 0);
    let p = b.cad("Work:d/Uno");
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), 0);
    assert_eq!(b.caja.mem.leer8(ap + AP_FLAGS).unwrap() & APF_ITSWILD, 0);
    b.llamar(lvo::MATCH_END, &[ap]);
    let (v, r) = buscar(&mut b, "Work:d/dos");
    assert!(v.is_empty());
    assert_eq!(r, ERROR_OBJECT_NOT_FOUND);
    let (_, r) = buscar(&mut b, "Work:nohay/#?");
    assert_eq!(r, ERROR_OBJECT_NOT_FOUND);
    b.limpio(0, 1);
}

#[test]
fn asignacion_multiple() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "uno/ILBM", b"1");
    b.dos.fs.fs.poner(b.work, "uno/PNG", b"1");
    b.dos.fs.fs.poner(b.work, "dos/JPEG", b"2");
    b.dos.fs.fs.poner(b.ram, "fichero", b"no es cajon");
    assign_lock(&mut b, "DT", "Work:uno");
    for ruta in ["RAM:fichero", "Work:dos"] {
        let l = b.lock(ruta, SHARED_LOCK);
        let n = b.cad("DT");
        b.llamar(lvo::ASSIGN_ADD, &[n, l]);
    }
    let (v, r) = buscar(&mut b, "DT:#?");
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    let mut n: Vec<_> = v.iter().map(|(n, _)| n.clone()).collect();
    n.sort();
    assert_eq!(n, ["ILBM", "JPEG", "PNG"], "los dos cajones, y el que no es cajón se salta");
    // Sin comodines, en el segundo cajón.
    let (v, _) = buscar(&mut b, "DT:jpeg");
    assert_eq!(v.len(), 1);
}

#[test]
fn ap_buf_corto() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "cajon/nombre-largo", b"x");
    let ap = anchor(&mut b, 8);
    let p = b.cad("Work:cajon/#?");
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), ERROR_BUFFER_OVERFLOW);
    assert_eq!(b.cadena_en(ap + AP_BUF), "Work:ca", "cortado, con su cero");
    assert_eq!(b.nombre_fib(ap + AP_INFO), "nombre-largo", "ap_Info sí se rellena");
    b.llamar(lvo::MATCH_END, &[ap]);
    b.limpio(0, 1);
}

#[test]
fn hostil() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "d/a", b"1");
    let ap = anchor(&mut b, 32);
    let p = b.cad("Work:d/#?");
    // AnchorPath en la página cero o el patrón NULL: fallo, sin caerse.
    assert_ne!(b.llamar(lvo::MATCH_FIRST, &[p, 8]), 0);
    assert_ne!(b.llamar(lvo::MATCH_FIRST, &[0, ap]), 0);
    // MatchNext y MatchEnd sin MatchFirst.
    assert_eq!(b.llamar(lvo::MATCH_NEXT, &[ap]), ERROR_NO_MORE_ENTRIES);
    b.llamar(lvo::MATCH_END, &[ap]);
    b.llamar(lvo::MATCH_END, &[0]);
    // Patrón roto.
    let roto = b.cad("Work:d/(a|");
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[roto, ap]), ERROR_BAD_TEMPLATE);
    b.llamar(lvo::MATCH_END, &[ap]);
    // Comodines fuera del último trozo: aún no; se apunta como función que falta.
    let medio = b.cad("Work:#?/a");
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[medio, ap]), ERROR_NOT_IMPLEMENTED);
    b.llamar(lvo::MATCH_END, &[ap]);
    // El mismo AnchorPath dos veces sin MatchEnd: no deja cerrojos sueltos.
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), 0);
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), 0);
    b.llamar(lvo::MATCH_END, &[ap]);
    b.llamar(lvo::MATCH_END, &[ap]);
    // APF_DODIR sobre un fichero: no entra en nada y la búsqueda sigue.
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), 0);
    let f = b.caja.mem.leer8(ap + AP_FLAGS).unwrap();
    b.caja.mem.escribir8(ap + AP_FLAGS, f | APF_DODIR);
    assert_eq!(b.llamar(lvo::MATCH_NEXT, &[ap]), ERROR_NO_MORE_ENTRIES);
    assert_eq!(b.caja.mem.leer8(ap + AP_FLAGS).unwrap() & APF_DODIR, 0);
    b.llamar(lvo::MATCH_END, &[ap]);
    // Muchas búsquedas sin MatchEnd: hay tope y no se cae.
    let mut abiertas = Vec::new();
    let mut tope = false;
    for _ in 0..300 {
        let a = anchor(&mut b, 0);
        // MatchEnd también tras un fallo, como pide AmigaOS.
        abiertas.push(a);
        match b.llamar(lvo::MATCH_FIRST, &[p, a]) {
            0 => {}
            ERROR_NO_FREE_STORE => {
                tope = true;
                break;
            }
            r => panic!("{r}"),
        }
    }
    assert!(tope, "las búsquedas tienen tope");
    for a in abiertas {
        b.llamar(lvo::MATCH_END, &[a]);
    }
    b.limpio(0, 1);
    let avisos = b.avisos();
    assert!(avisos.iter().any(|a| a.funcion == "MatchFirst"), "{avisos:?}");
}

#[test]
fn la_tarea_acaba_sin_match_end() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "d/a", b"1");
    let ap = anchor(&mut b, 0);
    let p = b.cad("Work:d/#?");
    assert_eq!(b.llamar(lvo::MATCH_FIRST, &[p, ap]), 0);
    let t = b.proceso;
    b.dos.acabar_tarea(&mut b.caja, t);
    // Se sueltan su cerrojo y su búsqueda (y la consola, que también era suya).
    assert_eq!(b.dos.cerrojos_abiertos(), 0);
    assert_eq!(b.dos.fs.abiertas(), 0);
    b.sin_faltas();
}

/// Recorre `patron` entrando en todos los cajones (como Dir ALL): (nombre, DIDDIR, ap_Buf) de cada uno, y el código
/// del final. Comprueba que `ap_Last` lleva el cerrojo del cajón donde está lo dado.
fn recorrer(b: &mut Banco, patron: &str) -> (Vec<(String, bool, String)>, u32) {
    let ap = anchor(b, 128);
    let p = b.cad(patron);
    let mut v = Vec::new();
    let mut r = b.llamar(lvo::MATCH_FIRST, &[p, ap]);
    while r == 0 {
        let f = b.caja.mem.leer8(ap + AP_FLAGS).unwrap();
        let didir = f & APF_DIDDIR != 0;
        v.push((b.nombre_fib(ap + AP_INFO), didir, b.cadena_en(ap + AP_BUF)));
        assert!(v.len() < 100, "no acaba");
        let cajon = b.l32(ap + AP_INFO + 4) as i32 > 0;
        if didir {
            b.caja.mem.escribir8(ap + AP_FLAGS, f & !APF_DIDDIR);
        } else if cajon {
            b.caja.mem.escribir8(ap + AP_FLAGS, f | APF_DODIR);
        }
        r = b.llamar(lvo::MATCH_NEXT, &[ap]);
        if r == 0 && b.caja.mem.leer8(ap + AP_FLAGS).unwrap() & APF_DIDDIR == 0 && v.len() > 1 {
            let ach = b.l32(ap + AP_LAST);
            assert_ne!(b.l32(ach + AN_LOCK), 0, "ap_Last con su cerrojo");
        }
    }
    b.llamar(lvo::MATCH_END, &[ap]);
    (v, r)
}

fn t(n: &str, d: bool, buf: &str) -> (String, bool, String) {
    (n.to_string(), d, buf.to_string())
}

/// Lo que da el oráculo (pruebas68k/dosasl.c en FS-UAE con el Kickstart): al entrar en un cajón, todo lo de dentro
/// y, al salir, el cajón otra vez con APF_DIDDIR.
#[test]
fn entrar_en_cajones() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "pdir/sub/c.txt", b"123");
    let (v, r) = recorrer(&mut b, "Work:pdir");
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    assert_eq!(
        v,
        [
            t("pdir", false, "Work:pdir"),
            t("sub", false, "Work:pdir/sub"),
            t("c.txt", false, "Work:pdir/sub/c.txt"),
            t("sub", true, "Work:pdir/sub"),
            t("pdir", true, "Work:pdir"),
        ]
    );
    // Con comodines, dentro sale todo, case o no (sub no casa con p#?).
    b.dos.fs.fs.poner(b.work, "pdir/pz.txt", b"12");
    b.dos.fs.fs.borrar_por_fuera(b.work, "pdir/sub/c.txt");
    let (v, r) = recorrer(&mut b, "Work:p#?");
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    let mut dentro: Vec<_> = v[1..].iter().filter(|x| !x.1).map(|x| x.0.clone()).collect();
    dentro.sort();
    assert_eq!(v[0], t("pdir", false, "Work:pdir"));
    assert_eq!(dentro, ["pz.txt", "sub"]);
    assert!(v.contains(&t("sub", true, "Work:pdir/sub")) && v.last() == Some(&t("pdir", true, "Work:pdir")), "{v:?}");
    b.limpio(0, 1);
}

/// El patrón vacío es el cajón actual (aquí, la raíz del volumen de arranque), con el nombre vacío (el oráculo);
/// MatchNext sin APF_DODIR acaba ahí.
#[test]
fn patron_vacio() {
    let mut b = Banco::nuevo();
    let (v, r) = buscar(&mut b, "");
    assert_eq!(r, ERROR_NO_MORE_ENTRIES);
    assert_eq!(v, [(String::new(), String::new())]);
    let (v, _) = recorrer(&mut b, "");
    assert_eq!(v.first(), Some(&t("", false, "")));
    assert_eq!(v.last(), Some(&t("", true, "")));
    assert!(v.contains(&t("Startup-Sequence", false, "S/Startup-Sequence")), "{v:?}");
    b.limpio(0, 1);
}
