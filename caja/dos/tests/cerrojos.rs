//! Lock, UnLock, DupLock, Examine, ExNext, Info, ParentDir, CurrentDir, NameFromLock, SameLock, y los cambios por
//! nombre: CreateDir, DeleteFile, Rename, SetComment, SetProtection, SetFileDate.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::fs::bits;
use caja_dos::*;

#[test]
fn examine_y_exnext() {
    let mut b = Banco::nuevo();
    for n in ["uno", "dos", "Tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez", "once"] {
        b.dos.fs.fs.poner(b.work, &format!("cajon/{n}"), n.as_bytes());
    }
    b.dos.fs.fs.poner_cajon(b.work, "cajon/sub");
    let v = b.listar("Work:cajon");
    assert_eq!(v.len(), 12);
    assert_eq!(v[2], "Tres");
    assert_eq!(v[11], "sub");
    // Examine de un fichero y de un cajón.
    let l = b.lock("work:CAJON/tres", SHARED_LOCK);
    let (r, f) = b.examine(l);
    assert_eq!(r, DOSTRUE);
    assert_eq!(b.nombre_fib(f), "Tres", "el nombre real, no el tecleado");
    assert_eq!(b.l32(f + fib::FIB_DIRENTRYTYPE) as i32, st::FILE);
    assert_eq!(b.l32(f + fib::FIB_ENTRYTYPE) as i32, st::FILE);
    assert_eq!(b.l32(f + fib::FIB_SIZE), 4);
    assert_eq!(b.l32(f + fib::FIB_NUMBLOCKS), 1);
    assert_eq!(b.l32(f + fib::FIB_PROTECTION) & 0xF, fibf::EXECUTE, "0o644: sin E");
    // ExNext sobre un fichero: no hay entradas.
    assert_eq!(b.llamar(lvo::EX_NEXT, &[l, f]), 0);
    assert_eq!(b.io_err(), ERROR_NO_MORE_ENTRIES);
    assert_eq!(b.name_from_lock(l), "Work:cajon/Tres");
    b.unlock(l);
    let l = b.lock("Work:cajon/sub", SHARED_LOCK);
    let (_, f) = b.examine(l);
    assert_eq!(b.l32(f + fib::FIB_DIRENTRYTYPE) as i32, st::USERDIR);
    b.unlock(l);
    // La raíz: ST_ROOT y el nombre del volumen.
    let l = b.lock("Work:", SHARED_LOCK);
    let (_, f) = b.examine(l);
    assert_eq!(b.l32(f + fib::FIB_DIRENTRYTYPE) as i32, st::ROOT);
    assert_eq!(b.nombre_fib(f), "Work");
    b.unlock(l);
    // El cerrojo 0 es la raíz de SYS:.
    let (r, f) = b.examine(0);
    assert_eq!(r, DOSTRUE);
    assert_eq!(b.nombre_fib(f), "SYS");
    b.limpio(0, 1);
}

#[test]
fn exnext_mientras_se_borra() {
    let mut b = Banco::nuevo();
    for i in 0..20 {
        b.dos.fs.fs.poner(b.ram, &format!("t/f{i:02}"), b"x");
    }
    // El bucle de siempre: ExNext y DeleteFile de lo que sale. No debe saltarse ninguno.
    let l = b.lock("RAM:t", SHARED_LOCK);
    let (_, f) = b.examine(l);
    let cd = b.llamar(lvo::CURRENT_DIR, &[l]);
    let mut borrados = 0;
    while b.llamar(lvo::EX_NEXT, &[l, f]) == DOSTRUE {
        let n = b.nombre_fib(f);
        let nn = b.cad(&n);
        assert_eq!(b.llamar(lvo::DELETE_FILE, &[nn]), DOSTRUE, "{n}");
        borrados += 1;
    }
    assert_eq!(borrados, 20);
    b.llamar(lvo::CURRENT_DIR, &[cd]);
    b.unlock(l);
    assert_eq!(b.listar("RAM:t"), Vec::<String>::new());
    b.limpio(0, 1);
}

#[test]
fn exnext_con_fib_basura() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.ram, "a", b"");
    let l = b.lock("RAM:", SHARED_LOCK);
    let f = b.fib();
    // Sin Examine, con fib_DiskKey enorme: no hay más entradas, sin caerse.
    b.caja.mem.escribir32(f, 0xFFFF_FFF0);
    assert_eq!(b.llamar(lvo::EX_NEXT, &[l, f]), 0);
    assert_eq!(b.io_err(), ERROR_NO_MORE_ENTRIES);
    // Un fib fuera de la memoria.
    assert_eq!(b.llamar(lvo::EXAMINE, &[l, TAM_MEMORIA as u32 - 100]), 0);
    assert_eq!(b.llamar(lvo::EX_NEXT, &[l, 8]), 0);
    assert_eq!(b.avisos().len(), 2);
    b.unlock(l);
    b.limpio(0, 1);
}

#[test]
fn cerrojos_malos() {
    let mut b = Banco::nuevo();
    let l = b.lock("SYS:S", SHARED_LOCK);
    b.unlock(l);
    // UnLock dos veces, y de algo que nunca fue cerrojo: aviso, sin caerse.
    b.unlock(l);
    b.unlock(0x4000);
    let a = b.avisos();
    assert_eq!(a.len(), 2);
    assert!(matches!(a[0].fallo, Fallo::CerrojoDesconocido { .. }));
    assert_eq!(a[0].funcion, "UnLock");
    // Usar un cerrojo soltado.
    let f = b.fib();
    for (n, args) in [
        (lvo::EXAMINE, vec![l, f]),
        (lvo::EX_NEXT, vec![l, f]),
        (lvo::INFO, vec![l, f]),
        (lvo::DUP_LOCK, vec![l]),
        (lvo::PARENT_DIR, vec![l]),
        (lvo::OPEN_FROM_LOCK, vec![l]),
    ] {
        b.poner_io_err(0);
        assert_eq!(b.llamar(n, &args), 0, "{}", lvo::nombre(n));
        assert_eq!(b.io_err(), ERROR_INVALID_LOCK, "{}", lvo::nombre(n));
    }
    assert_eq!(b.avisos().len(), 6);
    // Un cerrojo soltado como cajón actual: lo relativo falla con ERROR_INVALID_LOCK.
    b.llamar(lvo::CURRENT_DIR, &[l]);
    assert_eq!(b.avisos().len(), 1);
    assert_eq!(b.lock("x", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_INVALID_LOCK);
    assert!(b.lock("SYS:S", SHARED_LOCK) != 0, "lo absoluto sí va");
    b.llamar(lvo::CURRENT_DIR, &[0]);
    b.dos.tomar_avisos();
    let viejo = b.lock("SYS:S", SHARED_LOCK);
    let _ = viejo;
    // (dos cerrojos abiertos: el de la línea de antes y este)
    assert_eq!(b.dos.cerrojos_abiertos(), 2);
}

#[test]
fn duplock_parentdir_samelock_info() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "a/b/c", b"x");
    let l = b.lock("Work:a/b/c", SHARED_LOCK);
    let d = b.llamar(lvo::DUP_LOCK, &[l]);
    assert_ne!(d, 0);
    assert_ne!(d, l);
    assert_eq!(b.llamar(lvo::SAME_LOCK, &[l, d]) as i32, LOCK_SAME);
    assert_eq!(b.llamar(lvo::DUP_LOCK, &[0]), 0);
    // ParentDir hasta la raíz, y de la raíz: 0 con IoErr() a 0.
    let p1 = b.llamar(lvo::PARENT_DIR, &[l]);
    assert_eq!(b.name_from_lock(p1), "Work:a/b");
    assert_eq!(b.llamar(lvo::SAME_LOCK, &[l, p1]) as i32, LOCK_SAME_VOLUME);
    let p2 = b.llamar(lvo::PARENT_DIR, &[p1]);
    let p3 = b.llamar(lvo::PARENT_DIR, &[p2]);
    assert_eq!(b.name_from_lock(p3), "Work:");
    b.poner_io_err(99);
    assert_eq!(b.llamar(lvo::PARENT_DIR, &[p3]), 0);
    assert_eq!(b.io_err(), 0);
    assert_eq!(b.llamar(lvo::PARENT_DIR, &[0]), 0);
    let s = b.lock("SYS:", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::SAME_LOCK, &[s, 0]) as i32, LOCK_SAME);
    assert_eq!(b.llamar(lvo::SAME_LOCK, &[s, l]) as i32, LOCK_DIFFERENT);
    assert_eq!(b.llamar(lvo::SAME_DEVICE, &[s, 0]), DOSTRUE);
    assert_eq!(b.llamar(lvo::SAME_DEVICE, &[s, l]), 0);
    // Un cerrojo exclusivo no se duplica.
    let e = b.lock("Work:a", EXCLUSIVE_LOCK);
    assert_eq!(e, 0, "Work:a tiene p2 encima");
    b.unlock(p2);
    let e = b.lock("Work:a", EXCLUSIVE_LOCK);
    assert_ne!(e, 0);
    assert_eq!(b.llamar(lvo::DUP_LOCK, &[e]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    // Info.
    let id = b.hueco(infodata::TAMANO);
    assert_eq!(b.llamar(lvo::INFO, &[l, id]), DOSTRUE);
    assert_eq!(b.l32(id + infodata::ID_DISKSTATE), infodata::ID_VALIDATED);
    assert_eq!(b.l32(id + infodata::ID_BYTESPERBLOCK), 512);
    assert_eq!(b.l32(id + infodata::ID_NUMBLOCKS), (1 << 30) / 512);
    assert!(b.l32(id + infodata::ID_NUMBLOCKSUSED) <= b.l32(id + infodata::ID_NUMBLOCKS));
    assert_eq!(b.l32(id + infodata::ID_DISKTYPE), infodata::ID_INTER_FFS_DISK);
    assert_eq!(b.l32(id + infodata::ID_INUSE), DOSTRUE);
    let dl = b.l32(id + infodata::ID_VOLUMENODE) << 2;
    assert_eq!(b.leer((b.l32(dl + doslist::DOL_NAME) << 2) + 1, 4), b"Work");
    let cd = b.lock("CD0:", SHARED_LOCK);
    b.llamar(lvo::INFO, &[cd, id]);
    assert_eq!(b.l32(id + infodata::ID_DISKSTATE), infodata::ID_WRITE_PROTECTED);
    for x in [l, d, p1, p3, s, e, cd] {
        b.unlock(x);
    }
    b.limpio(0, 1);
}

#[test]
fn cerrojo_de_algo_borrado_por_fuera() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "d/fichero", b"datos");
    let l = b.lock("Work:d/fichero", SHARED_LOCK);
    let ld = b.lock("Work:d", SHARED_LOCK);
    let f = b.open("Work:d/fichero", MODE_OLDFILE);
    // Otro programa de DashOS lo borra todo.
    b.dos.fs.fs.borrar_por_fuera(b.work, "d");
    // El cerrojo y el fichero siguen valiendo para lo que ya tenían.
    let (r, fib) = b.examine(l);
    assert_eq!(r, DOSTRUE);
    assert_eq!(b.nombre_fib(fib), "fichero");
    assert_eq!(b.read(f, 100).1, b"datos");
    // Lo que pasa por el nombre ya no lo encuentra.
    assert_eq!(b.lock("Work:d/fichero", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    let p = b.llamar(lvo::PARENT_DIR, &[l]);
    assert_eq!(p, 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    // Listar el cajón borrado: vacío o error, sin caerse.
    let (r, fib) = b.examine(ld);
    assert_eq!(r, DOSTRUE);
    let _ = b.llamar(lvo::EX_NEXT, &[ld, fib]);
    // Crear dentro: no se puede.
    let cd = b.llamar(lvo::CURRENT_DIR, &[ld]);
    assert_eq!(b.open("nuevo", MODE_NEWFILE), 0);
    b.llamar(lvo::CURRENT_DIR, &[cd]);
    let buf = b.hueco(64);
    assert_eq!(b.llamar(lvo::NAME_FROM_LOCK, &[l, buf, 64]), DOSTRUE);
    assert_eq!(b.cadena_en(buf), "Work:d/fichero");
    b.unlock(l);
    b.unlock(ld);
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn crear_y_borrar() {
    let mut b = Banco::nuevo();
    let n = b.cad("RAM:Cajon");
    let l = b.llamar(lvo::CREATE_DIR, &[n]);
    assert_ne!(l, 0);
    // Es exclusivo.
    assert_eq!(b.l32((l << 2) + filelock::FL_ACCESS) as i32, EXCLUSIVE_LOCK);
    assert_eq!(b.llamar(lvo::CREATE_DIR, &[n]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_EXISTS);
    // Con el cerrojo encima no se borra.
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[n]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    b.unlock(l);
    let f = b.open("RAM:cajon/dentro", MODE_NEWFILE);
    b.close(f);
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[n]), 0);
    assert_eq!(b.io_err(), ERROR_DIRECTORY_NOT_EMPTY);
    let d = b.cad("RAM:cajon/dentro");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[d]), DOSTRUE);
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[d]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[n]), DOSTRUE);
    // La raíz no se borra.
    let r = b.cad("RAM:");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[r]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    // "No borrar".
    b.dos.fs.fs.poner(b.ram, "fijo", b"");
    b.dos.fs.fs.poner_proteccion(b.ram, "fijo", bits::NO_BORRAR_DUENO, 0o644);
    let x = b.cad("RAM:fijo");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[x]), 0);
    assert_eq!(b.io_err(), ERROR_DELETE_PROTECTED);
    // En solo lectura.
    let x = b.cad("CD0:Leeme");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[x]), 0);
    assert_eq!(b.io_err(), ERROR_DISK_WRITE_PROTECTED);
    let x = b.cad("CD0:nuevo");
    assert_eq!(b.llamar(lvo::CREATE_DIR, &[x]), 0);
    assert_eq!(b.io_err(), ERROR_DISK_WRITE_PROTECTED);
    b.limpio(0, 1);
}

#[test]
fn renombrar() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "a/b/fichero", b"F");
    b.dos.fs.fs.poner(b.work, "otro", b"O");
    let ren = |b: &mut Banco, x: &str, y: &str| {
        let (x, y) = (b.cad(x), b.cad(y));
        b.llamar(lvo::RENAME, &[x, y])
    };
    // Con un cerrojo y un fichero dentro de lo que se mueve: siguen al objeto.
    let l = b.lock("Work:a/b/fichero", SHARED_LOCK);
    let f = b.open("Work:a/b/fichero", MODE_OLDFILE);
    assert_eq!(ren(&mut b, "Work:a/b", "Work:B2"), DOSTRUE);
    assert_eq!(b.name_from_lock(l), "Work:B2/fichero");
    let p = b.llamar(lvo::PARENT_DIR, &[l]);
    assert_eq!(b.name_from_lock(p), "Work:B2");
    b.unlock(p);
    assert_eq!(b.read(f, 5).1, b"F");
    b.close(f);
    // Cambiar solo mayúsculas.
    assert_eq!(ren(&mut b, "Work:b2", "Work:b2"), DOSTRUE);
    assert_eq!(ren(&mut b, "Work:b2", "Work:bb"), DOSTRUE);
    assert_eq!(ren(&mut b, "Work:bb", "Work:BB"), DOSTRUE);
    assert_eq!(b.dos.fs.fs.nombre_guardado(b.work, "bb").unwrap(), "BB");
    // Errores.
    assert_eq!(ren(&mut b, "Work:BB", "Work:otro"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_EXISTS);
    assert_eq!(ren(&mut b, "Work:BB", "RAM:BB"), 0);
    assert_eq!(b.io_err(), ERROR_RENAME_ACROSS_DEVICES);
    assert_eq!(ren(&mut b, "Work:a", "Work:a/dentro"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    assert_eq!(ren(&mut b, "Work:no", "Work:si"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    assert_eq!(ren(&mut b, "Work:", "Work:x"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    assert_eq!(ren(&mut b, "Work:otro", "Work:nada/otro"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    assert_eq!(ren(&mut b, "Work:otro", "Work:a:b"), 0);
    assert_eq!(b.io_err(), ERROR_INVALID_COMPONENT_NAME);
    // Mover a otro cajón del mismo volumen.
    assert_eq!(ren(&mut b, "Work:otro", "Work:a/otro2"), DOSTRUE);
    assert_eq!(b.dos.fs.fs.contenido(b.work, "a/otro2").unwrap(), b"O");
    b.unlock(l);
    b.limpio(0, 1);
}

#[test]
fn comentario_proteccion_fecha() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.ram, "f", b"");
    let n = b.cad("RAM:f");
    let c79 = b.cad(&"c".repeat(79));
    let c80 = b.cad(&"c".repeat(80));
    assert_eq!(b.llamar(lvo::SET_COMMENT, &[n, c79]), DOSTRUE);
    assert_eq!(b.dos.fs.fs.comentario_de(b.ram, "f").unwrap().len(), 79);
    assert_eq!(b.llamar(lvo::SET_COMMENT, &[n, c80]), 0);
    assert_eq!(b.io_err(), ERROR_COMMENT_TOO_BIG);
    let l = b.lock("RAM:f", SHARED_LOCK);
    let (_, fib) = b.examine(l);
    assert_eq!(b.cadena_en(fib + fib::FIB_COMMENT), "c".repeat(79));
    b.unlock(l);
    // Un comentario con acentos (Latin-1) va al servicio en UTF-8.
    let cm = b.cad("¡ojo!");
    b.llamar(lvo::SET_COMMENT, &[n, cm]);
    assert_eq!(b.dos.fs.fs.comentario_de(b.ram, "f").unwrap(), "¡ojo!");
    // SetProtection: W y D a 1 (no escribir, no borrar), S a 1.
    let p = fibf::WRITE | fibf::DELETE | fibf::SCRIPT | fibf::GRP_READ;
    assert_eq!(b.llamar(lvo::SET_PROTECTION, &[n, p]), DOSTRUE);
    let (amiga, modo) = b.dos.fs.fs.proteccion(b.ram, "f").unwrap();
    assert_eq!(modo, 0o540);
    assert_eq!(amiga, bits::NO_BORRAR_DUENO | bits::GUION | bits::NO_BORRAR_GRUPO | bits::NO_BORRAR_OTROS);
    let l = b.lock("RAM:f", SHARED_LOCK);
    let (_, fib) = b.examine(l);
    assert_eq!(b.l32(fib + fib::FIB_PROTECTION), p);
    b.unlock(l);
    // Ahora no se puede escribir.
    assert_eq!(b.open("RAM:f", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_WRITE_PROTECTED);
    // SetFileDate.
    let ds = b.hueco(12);
    b.caja.mem.escribir32(ds, 100);
    b.caja.mem.escribir32(ds + 4, 60);
    b.caja.mem.escribir32(ds + 8, 0);
    assert_eq!(b.llamar(lvo::SET_FILE_DATE, &[n, ds]), DOSTRUE);
    let l = b.lock("RAM:f", SHARED_LOCK);
    let (_, fib) = b.examine(l);
    assert_eq!(b.leer(fib + fib::FIB_DATE, 12), b.leer(ds, 12));
    b.unlock(l);
    // Algo que no hay.
    let x = b.cad("RAM:no");
    assert_eq!(b.llamar(lvo::SET_PROTECTION, &[x, 0]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    b.limpio(0, 1);
}
