//! Rutas del Amiga: volúmenes, `:`, `/` para subir, el cajón actual, `PROGDIR:` y las rutas que no valen.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::nombres::{Paso, partir};
use caja_dos::*;

fn pasos(r: &str) -> (Option<String>, Vec<String>) {
    let p = partir(r.as_bytes()).unwrap();
    (
        p.dispositivo.map(|d| String::from_utf8(d).unwrap()),
        p.pasos
            .into_iter()
            .map(|x| match x {
                Paso::Subir => "/".to_string(),
                Paso::Nombre(n) => n,
            })
            .collect(),
    )
}

#[test]
fn partir_rutas() {
    assert_eq!(pasos(""), (None, vec![]));
    assert_eq!(pasos("a"), (None, vec!["a".into()]));
    assert_eq!(pasos("a/"), (None, vec!["a".into()]));
    assert_eq!(pasos("/"), (None, vec!["/".into()]));
    assert_eq!(pasos("//x"), (None, vec!["/".into(), "/".into(), "x".into()]));
    assert_eq!(pasos("a/b//c"), (None, vec!["a".into(), "b".into(), "/".into(), "c".into()]));
    assert_eq!(pasos("a//"), (None, vec!["a".into(), "/".into()]));
    assert_eq!(pasos("SYS:"), (Some("SYS".into()), vec![]));
    assert_eq!(pasos(":"), (Some("".into()), vec![]));
    assert_eq!(pasos(":C/Dir"), (Some("".into()), vec!["C".into(), "Dir".into()]));
    assert_eq!(pasos("Work:a b/c"), (Some("Work".into()), vec!["a b".into(), "c".into()]));
    // Latin-1 a UTF-8.
    assert_eq!(partir(b"RAM:\xF1u").unwrap().pasos, vec![Paso::Nombre("ñu".into())]);
    // Lo que no vale.
    for malo in ["a:b:c", "SYS::", "x:y/z:w", "a/b:c", "RAM:..", "RAM:.", "../x", "a/./b"] {
        assert!(partir(malo.as_bytes()).is_err(), "{malo}");
    }
    let largo = "x".repeat(108);
    assert!(partir(largo.as_bytes()).is_err());
    assert!(partir("x".repeat(107).as_bytes()).is_ok());
}

#[test]
fn volumenes_y_mayusculas() {
    let mut b = Banco::nuevo();
    for r in ["SYS:", "sys:", "SyS:S", "SYS:s/startup-sequence", "Work:", "RAM:", "CD0:Leeme"] {
        let l = b.lock(r, SHARED_LOCK);
        assert_ne!(l, 0, "{r}: {}", b.io_err());
        b.unlock(l);
    }
    // Un volumen que no hay.
    assert_eq!(b.lock("Nada:", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_DEVICE_NOT_MOUNTED);
    assert_eq!(b.lock("DF0:x", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_DEVICE_NOT_MOUNTED);
    // Algo que no hay en un volumen que sí.
    assert_eq!(b.lock("SYS:NoEsta", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    assert_eq!(b.lock("SYS:NoEsta/tampoco", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    // Pasar por un fichero como si fuera un cajón.
    assert_eq!(b.lock("SYS:C/Dir/x", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    b.limpio(0, 1);
}

#[test]
fn subir_y_cajon_actual() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "a/b/c/fichero", b"hola");
    // Sin cajón actual (0), lo relativo va a la raíz de SYS:.
    let l = b.lock("S", SHARED_LOCK);
    assert_eq!(b.name_from_lock(l), "SYS:S");
    b.unlock(l);
    let l = b.lock("", SHARED_LOCK);
    assert_eq!(b.name_from_lock(l), "SYS:");
    b.unlock(l);
    // CurrentDir a Work:a/b.
    let cd = b.lock("Work:a/b", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::CURRENT_DIR, &[cd]), 0);
    for (r, esperado) in [
        ("c", "Work:a/b/c"),
        ("c/fichero", "Work:a/b/c/fichero"),
        ("/", "Work:a"),
        ("//", "Work:"),
        ("/b/c", "Work:a/b/c"),
        ("c//", "Work:a/b"),
        ("c/fichero//", "Work:a/b/c"),
        (":", "Work:"),
        (":a", "Work:a"),
        ("", "Work:a/b"),
        ("SYS:S", "SYS:S"),
    ] {
        let l = b.lock(r, SHARED_LOCK);
        assert_ne!(l, 0, "{r}: {}", b.io_err());
        assert_eq!(b.name_from_lock(l), esperado, "{r}");
        b.unlock(l);
    }
    // Por encima de la raíz del volumen no se sube.
    for r in ["///", "////x", ":/", "Work:/", "SYS:/S", "/////"] {
        b.poner_io_err(0);
        assert_eq!(b.lock(r, SHARED_LOCK), 0, "{r}");
        assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND, "{r}");
    }
    assert_eq!(b.llamar(lvo::CURRENT_DIR, &[0]), cd);
    b.unlock(cd);
    b.limpio(0, 1);
}

#[test]
fn rutas_que_no_valen() {
    let mut b = Banco::nuevo();
    let largo = format!("RAM:{}", "n".repeat(108));
    let justo = format!("RAM:{}", "n".repeat(107));
    for r in ["SYS:S:x", "SYS::", "a:b:c", "RAM:..", "RAM:.", "RAM:a/../b", largo.as_str()] {
        assert_eq!(b.lock(r, SHARED_LOCK), 0, "{r}");
        assert_eq!(b.io_err(), ERROR_INVALID_COMPONENT_NAME, "{r}");
        assert_eq!(b.open(r, MODE_NEWFILE), 0, "{r}");
        assert_eq!(b.io_err(), ERROR_INVALID_COMPONENT_NAME, "{r}");
        let n = b.cad(r);
        assert_eq!(b.llamar(lvo::CREATE_DIR, &[n]), 0, "{r}");
        assert_eq!(b.io_err(), ERROR_INVALID_COMPONENT_NAME, "{r}");
    }
    // 107 caracteres sí.
    let f = b.open(&justo, MODE_NEWFILE);
    assert_ne!(f, 0);
    b.close(f);
    assert_eq!(b.listar("RAM:"), vec!["n".repeat(107)]);
    // Un puntero a la página 0 o fuera de la memoria: error y aviso, sin caerse.
    assert_eq!(b.llamar(lvo::LOCK, &[0x10, SHARED_LOCK as u32]), 0);
    assert_eq!(b.llamar(lvo::LOCK, &[0, SHARED_LOCK as u32]), 0);
    assert_eq!(b.llamar(lvo::LOCK, &[0xFFFF_FFF0, SHARED_LOCK as u32]), 0);
    b.caja.mem.datos[TAM_MEMORIA - 2..].fill(b'a');
    assert_eq!(b.llamar(lvo::OPEN, &[TAM_MEMORIA as u32 - 2, MODE_OLDFILE as u32]), 0);
    let avisos = b.avisos();
    assert_eq!(avisos.len(), 4);
    assert!(matches!(avisos[0].fallo, Fallo::PaginaCero { .. }));
    assert!(matches!(avisos[1].fallo, Fallo::PunteroNulo));
    assert!(matches!(avisos[2].fallo, Fallo::FueraDeMemoria { .. }));
    assert!(matches!(avisos[3].fallo, Fallo::FueraDeMemoria { .. } | Fallo::CadenaSinFin { .. }));
    assert_eq!(avisos[0].funcion, "Lock");
    assert_eq!(avisos[0].tarea, b.proceso);
    // Una cadena sin 0 en 4 KB.
    let sin_fin = b.bytes(&[b'a'; 5000]);
    b.caja.mem.datos[(sin_fin + 5000) as usize] = b'a';
    assert_eq!(b.llamar(lvo::LOCK, &[sin_fin, SHARED_LOCK as u32]), 0);
    assert!(matches!(b.avisos()[0].fallo, Fallo::CadenaSinFin { .. }));
    b.limpio(0, 1);
}

#[test]
fn nombres_largos_y_latin1_desde_fuera() {
    let mut b = Banco::nuevo();
    // Nombres que DashOS admite pero la caja no puede ver: con caracteres fuera de Latin-1 o de más de 107 bytes.
    b.dos.fs.fs.poner(b.work, "normal", b"");
    b.dos.fs.fs.poner(b.work, "ñandú", b"");
    b.dos.fs.fs.poner(b.work, "日本", b"");
    let largo = "l".repeat(150);
    b.dos.fs.fs.poner(b.work, &largo, b"");
    b.dos.fs.fs.poner(b.work, "último", b"");
    let v = b.listar("Work:");
    assert_eq!(v, vec!["normal".to_string(), "ñandú".into(), "último".into()]);
    // Y se abren con su nombre en Latin-1.
    let l = b.lock("Work:\u{f1}and\u{fa}", SHARED_LOCK);
    assert_ne!(l, 0);
    assert_eq!(b.name_from_lock(l), "Work:ñandú");
    b.unlock(l);
    b.limpio(0, 1);
}

#[test]
fn progdir() {
    let mut b = Banco::nuevo();
    assert_eq!(b.lock("PROGDIR:", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_DEVICE_NOT_MOUNTED);
    b.dos.fs.fs.poner(b.work, "Juego/datos/nivel1", b"N1");
    let h = b.lock("Work:Juego", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::SET_PROGRAM_DIR, &[h]), 0);
    assert_eq!(b.llamar(lvo::GET_PROGRAM_DIR, &[]), h);
    let f = b.open("progdir:datos/nivel1", MODE_OLDFILE);
    assert_ne!(f, 0);
    assert_eq!(b.read(f, 10).1, b"N1");
    b.close(f);
    let l = b.lock("PROGDIR:", SHARED_LOCK);
    assert_eq!(b.name_from_lock(l), "Work:Juego");
    b.unlock(l);
    // PROGDIR no se puede asignar.
    let n = b.cad("PROGDIR");
    let l = b.lock("RAM:", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::ASSIGN_LOCK, &[n, l]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_EXISTS);
    b.unlock(l);
    b.llamar(lvo::SET_PROGRAM_DIR, &[0]);
    b.unlock(h);
    b.limpio(0, 1);
}
