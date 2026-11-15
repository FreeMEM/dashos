//! Open, Close, Read, Write, Seek, SetFileSize, la consola, NIL: y las funciones con búfer.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::fs::bits;
use caja_dos::*;

#[test]
fn abrir_escribir_leer() {
    let mut b = Banco::nuevo();
    let f = b.open("RAM:nota", MODE_NEWFILE);
    assert_ne!(f, 0);
    assert_eq!(b.write(f, b"hola, "), 6);
    assert_eq!(b.write(f, b"mundo"), 5);
    // Leer de un fichero abierto para escribir: en AmigaOS se puede; al final no hay nada.
    assert_eq!(b.read(f, 10), (0, vec![]));
    assert_eq!(b.llamar(lvo::SEEK, &[f, 0, OFFSET_BEGINNING as u32]), 11);
    assert_eq!(b.read(f, 4), (4, b"hola".to_vec()));
    assert_eq!(b.close(f), DOSTRUE);
    assert_eq!(b.dos.fs.fs.contenido(b.ram, "nota").unwrap(), b"hola, mundo");
    // OLDFILE: desde el principio, se lee y se escribe encima.
    let f = b.open("ram:NOTA", MODE_OLDFILE);
    assert_ne!(f, 0);
    assert_eq!(b.write(f, b"HOLA"), 4);
    assert_eq!(b.read(f, 100), (7, b", mundo".to_vec()));
    assert_eq!(b.read(f, 100), (0, vec![]));
    b.close(f);
    assert_eq!(b.dos.fs.fs.contenido(b.ram, "nota").unwrap(), b"HOLA, mundo");
    // El nombre se guarda como se escribió la primera vez.
    assert_eq!(b.dos.fs.fs.nombre_guardado(b.ram, "NOTA").unwrap(), "nota");
    // NEWFILE sobre uno que existe lo vacía.
    let f = b.open("RAM:nota", MODE_NEWFILE);
    b.close(f);
    assert_eq!(b.dos.fs.fs.contenido(b.ram, "nota").unwrap(), b"");
    // READWRITE: lo crea si no está, y si está no lo vacía.
    let f = b.open("RAM:rw", MODE_READWRITE);
    assert_ne!(f, 0);
    b.write(f, b"123");
    b.close(f);
    let f = b.open("RAM:rw", MODE_READWRITE);
    assert_eq!(b.read(f, 10).1, b"123");
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn errores_al_abrir() {
    let mut b = Banco::nuevo();
    assert_eq!(b.open("RAM:no", MODE_OLDFILE), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    // Un cajón no se abre como fichero.
    for m in [MODE_OLDFILE, MODE_NEWFILE, MODE_READWRITE] {
        assert_eq!(b.open("SYS:S", m), 0);
        assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
        assert_eq!(b.open("SYS:", m), 0);
        assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    }
    // Un modo que no existe.
    assert_eq!(b.open("RAM:x", 1234), 0);
    assert_eq!(b.io_err(), ERROR_ACTION_NOT_KNOWN);
    // Crear en un cajón que no existe.
    assert_eq!(b.open("RAM:nada/x", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    // Volumen de solo lectura.
    assert_eq!(b.open("CD0:nuevo", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_DISK_WRITE_PROTECTED);
    assert_eq!(b.open("CD0:Leeme", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_DISK_WRITE_PROTECTED);
    let f = b.open("CD0:Leeme", MODE_OLDFILE);
    assert_ne!(f, 0);
    assert_eq!(b.write(f, b"x") as i32, -1);
    assert_eq!(b.io_err(), ERROR_DISK_WRITE_PROTECTED);
    assert_eq!(b.read(f, 100).1, b"solo lectura");
    b.close(f);
    // Fichero protegido contra escritura y contra lectura.
    b.dos.fs.fs.poner(b.ram, "ro", b"abc");
    b.dos.fs.fs.poner_proteccion(b.ram, "ro", 0, 0o444);
    assert_eq!(b.open("RAM:ro", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_WRITE_PROTECTED);
    let f = b.open("RAM:ro", MODE_OLDFILE);
    assert_eq!(b.write(f, b"x") as i32, -1);
    assert_eq!(b.io_err(), ERROR_WRITE_PROTECTED);
    b.close(f);
    b.dos.fs.fs.poner_proteccion(b.ram, "ro", 0, 0o200);
    let f = b.open("RAM:ro", MODE_OLDFILE);
    assert_eq!(b.read(f, 10).0 as i32, -1);
    assert_eq!(b.io_err(), ERROR_READ_PROTECTED);
    b.close(f);
    // Disco lleno (RAM: es de 1 MB).
    let f = b.open("RAM:grande", MODE_NEWFILE);
    let datos = vec![7u8; 600 * 1024];
    assert_eq!(b.write(f, &datos), datos.len() as u32);
    assert_eq!(b.write(f, &datos) as i32, -1);
    assert_eq!(b.io_err(), ERROR_DISK_FULL);
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn cerrojos_de_ficheros_abiertos() {
    let mut b = Banco::nuevo();
    // NEWFILE es exclusivo: nadie más lo abre ni lo bloquea.
    let f = b.open("RAM:excl", MODE_NEWFILE);
    assert_eq!(b.open("RAM:excl", MODE_OLDFILE), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    assert_eq!(b.lock("RAM:EXCL", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    assert_eq!(b.llamar(lvo::DUP_LOCK_FROM_FH, &[f]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    b.close(f);
    // OLDFILE es compartido: dos a la vez, y un cerrojo compartido también.
    let f1 = b.open("RAM:excl", MODE_OLDFILE);
    let f2 = b.open("RAM:excl", MODE_OLDFILE);
    assert!(f1 != 0 && f2 != 0);
    let l = b.lock("RAM:excl", SHARED_LOCK);
    assert_ne!(l, 0);
    assert_eq!(b.lock("RAM:excl", EXCLUSIVE_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    // NEWFILE con alguien dentro: no.
    assert_eq!(b.open("RAM:excl", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    // Ni borrarlo.
    let n = b.cad("RAM:excl");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[n]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    b.unlock(l);
    b.close(f1);
    b.close(f2);
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[n]), DOSTRUE);
    b.limpio(0, 1);
}

#[test]
fn seek_y_tamano() {
    let mut b = Banco::nuevo();
    let f = b.open("RAM:s", MODE_NEWFILE);
    b.write(f, b"0123456789");
    let seek = |b: &mut Banco, p: i32, m: i32| b.llamar(lvo::SEEK, &[f, p as u32, m as u32]) as i32;
    assert_eq!(seek(&mut b, 0, OFFSET_CURRENT), 10);
    assert_eq!(seek(&mut b, -3, OFFSET_END), 10);
    assert_eq!(b.read(f, 10).1, b"789");
    assert_eq!(seek(&mut b, 2, OFFSET_BEGINNING), 10);
    assert_eq!(seek(&mut b, 2, OFFSET_CURRENT), 2);
    assert_eq!(b.read(f, 1).1, b"4");
    // Fuera del fichero: error y la posición no cambia.
    assert_eq!(seek(&mut b, 11, OFFSET_BEGINNING), -1);
    assert_eq!(b.io_err(), ERROR_SEEK_ERROR);
    assert_eq!(seek(&mut b, -6, OFFSET_CURRENT), -1);
    assert_eq!(seek(&mut b, 1, OFFSET_END), -1);
    assert_eq!(seek(&mut b, 0, 7), -1);
    assert_eq!(b.io_err(), ERROR_SEEK_ERROR);
    assert_eq!(seek(&mut b, 0, OFFSET_CURRENT), 5);
    // Al final justo sí.
    assert_eq!(seek(&mut b, 0, OFFSET_END), 5);
    assert_eq!(seek(&mut b, 0, OFFSET_CURRENT), 10);
    // SetFileSize: recorta, y la posición pasa al final si queda fuera.
    assert_eq!(b.llamar(lvo::SET_FILE_SIZE, &[f, 4, OFFSET_BEGINNING as u32]), 4);
    assert_eq!(seek(&mut b, 0, OFFSET_CURRENT), 4);
    assert_eq!(b.llamar(lvo::SET_FILE_SIZE, &[f, 2, OFFSET_END as u32]), 6);
    assert_eq!(b.llamar(lvo::SET_FILE_SIZE, &[f, (-10i32) as u32, OFFSET_END as u32]) as i32, -1);
    assert_eq!(b.io_err(), ERROR_SEEK_ERROR);
    b.close(f);
    assert_eq!(b.dos.fs.fs.contenido(b.ram, "s").unwrap(), b"0123\0\0");
    // Escribir quita el bit de archivado.
    b.dos.fs.fs.poner_proteccion(b.ram, "s", bits::ARCHIVADO, 0o644);
    let f = b.open("RAM:s", MODE_OLDFILE);
    b.write(f, b"x");
    b.close(f);
    assert_eq!(b.dos.fs.fs.proteccion(b.ram, "s").unwrap().0 & bits::ARCHIVADO, 0);
    b.limpio(0, 1);
}

#[test]
fn ficheros_grandes_en_trozos() {
    let mut b = Banco::nuevo();
    let datos: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let f = b.open("Work:grande", MODE_NEWFILE);
    assert_eq!(b.write(f, &datos), datos.len() as u32);
    b.llamar(lvo::SEEK, &[f, 0, OFFSET_BEGINNING as u32]);
    let (n, leido) = b.read(f, 300_000);
    assert_eq!(n, 200_000);
    assert_eq!(leido, datos);
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn asas_malas() {
    let mut b = Banco::nuevo();
    let f = b.open("RAM:x", MODE_NEWFILE);
    assert_eq!(b.close(f), DOSTRUE);
    // Cerrar dos veces: error del Amiga y aviso, sin caerse.
    assert_eq!(b.close(f), 0);
    assert_eq!(b.io_err(), ERROR_INVALID_LOCK);
    // Leer, escribir y mover un fichero cerrado o que nunca fue.
    assert_eq!(b.read(f, 4).0 as i32, -1);
    assert_eq!(b.write(f, b"a") as i32, -1);
    assert_eq!(b.llamar(lvo::SEEK, &[f, 0, 0]) as i32, -1);
    assert_eq!(b.llamar(lvo::FGET_C, &[f]) as i32, -1);
    assert_eq!(b.read(0x1234, 4).0 as i32, -1);
    assert_eq!(b.read(0xFFFF_FFFF, 4).0 as i32, -1);
    assert_eq!(b.io_err(), ERROR_BAD_NUMBER);
    let avisos = b.avisos();
    assert_eq!(avisos.len(), 7);
    assert!(matches!(avisos[0].fallo, Fallo::AsaDesconocida { .. }));
    assert_eq!(avisos[0].funcion, "Close");
    assert!(matches!(avisos[6].fallo, Fallo::BptrMalo { .. }));
    // Close(0) no hace nada.
    assert_eq!(b.close(0), DOSTRUE);
    // Un búfer fuera de la memoria: error, aviso, y el fichero sigue bien.
    let f = b.open("RAM:x", MODE_OLDFILE);
    b.write(f, b"abc");
    b.llamar(lvo::SEEK, &[f, 0, OFFSET_BEGINNING as u32]);
    assert_eq!(b.llamar(lvo::READ, &[f, TAM_MEMORIA as u32 - 1, 3]) as i32, -1);
    assert_eq!(b.llamar(lvo::READ, &[f, 0x100, 3]) as i32, -1);
    assert_eq!(b.llamar(lvo::WRITE, &[f, 0x100, 3]) as i32, -1);
    assert_eq!(b.avisos().len(), 3);
    assert_eq!(b.read(f, 3).1, b"abc");
    // Largo negativo.
    assert_eq!(b.llamar(lvo::READ, &[f, 0x20000, u32::MAX]) as i32, -1);
    assert_eq!(b.io_err(), ERROR_BAD_NUMBER);
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn consola_y_nil() {
    let mut b = Banco::nuevo();
    let s = b.cad("hola\n");
    assert_eq!(b.llamar(lvo::PUT_STR, &[s]), 0);
    let out = b.llamar(lvo::OUTPUT, &[]);
    assert_eq!(b.llamar(lvo::IS_INTERACTIVE, &[out]), DOSTRUE);
    assert_eq!(b.write(out, b"abc"), 3);
    let s = b.cad("xyz");
    assert_eq!(b.llamar(lvo::WRITE_CHARS, &[s, 2]), 2);
    assert_eq!(b.caja.salida, b"hola\nabcxy");
    // Leer de la consola.
    b.caja.entrada.extend(b"linea uno\nlinea dos\n");
    let inp = b.llamar(lvo::INPUT, &[]);
    let buf = b.hueco(64);
    assert_eq!(b.llamar(lvo::FGETS, &[inp, buf, 64]), buf);
    assert_eq!(b.cadena_en(buf), "linea uno\n");
    assert_eq!(b.read(inp, 64).1, b"linea dos\n");
    assert_eq!(b.read(inp, 64).0, 0);
    // "*" y CONSOLE: abren la consola.
    for n in ["*", "CONSOLE:", "console:"] {
        let f = b.open(n, MODE_OLDFILE);
        assert_ne!(f, 0);
        assert_eq!(b.llamar(lvo::IS_INTERACTIVE, &[f]), DOSTRUE);
        assert_eq!(b.llamar(lvo::SEEK, &[f, 0, 0]) as i32, -1);
        b.close(f);
    }
    // NIL: se traga todo y no da nada.
    let n = b.open("NIL:", MODE_NEWFILE);
    assert_eq!(b.write(n, b"nada"), 4);
    assert_eq!(b.read(n, 10).0, 0);
    assert_eq!(b.llamar(lvo::IS_INTERACTIVE, &[n]), 0);
    assert_eq!(b.llamar(lvo::FGET_C, &[n]) as i32, -1);
    b.close(n);
    // SelectOutput a un fichero.
    let f = b.open("RAM:salida", MODE_NEWFILE);
    let viejo = b.llamar(lvo::SELECT_OUTPUT, &[f]);
    assert_eq!(viejo, out);
    let s = b.cad("al fichero");
    b.llamar(lvo::PUT_STR, &[s]);
    assert_eq!(b.llamar(lvo::SELECT_OUTPUT, &[viejo]), f);
    b.close(f);
    assert_eq!(b.dos.fs.fs.contenido(b.ram, "salida").unwrap(), b"al fichero");
    // Sin Output (0): PutStr da error, sin caerse.
    b.llamar(lvo::SELECT_OUTPUT, &[0]);
    let s = b.cad("x");
    assert_eq!(b.llamar(lvo::PUT_STR, &[s]) as i32, -1);
    b.llamar(lvo::SELECT_OUTPUT, &[out]);
    b.limpio(0, 1);
}

#[test]
fn con_bufer() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.ram, "t", b"ab\ncd\nultima");
    let f = b.open("RAM:t", MODE_OLDFILE);
    assert_eq!(b.llamar(lvo::FGET_C, &[f]), b'a' as u32);
    // UnGetC(-1) devuelve el último; otro UnGetC sin leer no se puede.
    assert_ne!(b.llamar(lvo::UN_GET_C, &[f, u32::MAX]), 0);
    assert_eq!(b.llamar(lvo::UN_GET_C, &[f, b'z' as u32]), 0);
    assert_eq!(b.llamar(lvo::FGET_C, &[f]), b'a' as u32);
    let buf = b.hueco(10);
    assert_eq!(b.llamar(lvo::FGETS, &[f, buf, 10]), buf);
    assert_eq!(b.cadena_en(buf), "b\n");
    // Un búfer corto corta la línea.
    assert_eq!(b.llamar(lvo::FGETS, &[f, buf, 2]), buf);
    assert_eq!(b.cadena_en(buf), "c");
    assert_eq!(b.llamar(lvo::FGETS, &[f, buf, 10]), buf);
    assert_eq!(b.cadena_en(buf), "d\n");
    assert_eq!(b.llamar(lvo::FGETS, &[f, buf, 10]), buf);
    assert_eq!(b.cadena_en(buf), "ultima");
    assert_eq!(b.llamar(lvo::FGETS, &[f, buf, 10]), 0);
    assert_eq!(b.io_err(), 0);
    assert_eq!(b.llamar(lvo::FGET_C, &[f]) as i32, -1);
    // UnGetC de un carácter cualquiera y Seek: la posición cuenta lo devuelto.
    b.llamar(lvo::SEEK, &[f, 1, OFFSET_BEGINNING as u32]);
    b.llamar(lvo::FGET_C, &[f]);
    b.llamar(lvo::UN_GET_C, &[f, b'Q' as u32]);
    assert_eq!(b.llamar(lvo::SEEK, &[f, 0, OFFSET_CURRENT as u32]), 1);
    assert_eq!(b.llamar(lvo::FGET_C, &[f]), b'b' as u32);
    b.close(f);
    // FPutC, FPuts, FWrite, FRead, VFPrintf.
    let f = b.open("RAM:w", MODE_NEWFILE);
    assert_eq!(b.llamar(lvo::FPUT_C, &[f, 0x141]), 0x41);
    let s = b.cad("BC");
    assert_eq!(b.llamar(lvo::FPUTS, &[f, s]), 0);
    let blk = b.bytes(b"123456789");
    assert_eq!(b.llamar(lvo::FWRITE, &[f, blk, 4, 2]), 2);
    let fmt = b.cad("[%ld|%s]");
    let txt = b.cad("ok");
    let args = b.hueco(8);
    b.caja.mem.escribir32(args, (-5i32) as u32);
    b.caja.mem.escribir32(args + 4, txt);
    assert_eq!(b.llamar(lvo::VFPRINTF, &[f, fmt, args]), 7);
    assert_eq!(b.llamar(lvo::FLUSH, &[f]), DOSTRUE);
    b.llamar(lvo::SEEK, &[f, 0, OFFSET_BEGINNING as u32]);
    let dst = b.hueco(16);
    assert_eq!(b.llamar(lvo::FREAD, &[f, dst, 5, 3]), 3);
    assert_eq!(b.leer(dst, 15), b"ABC12345678[-5|");
    b.close(f);
    assert_eq!(b.dos.fs.fs.contenido(b.ram, "w").unwrap(), b"ABC12345678[-5|ok]");
    // FRead con tamaños que desbordan.
    let f = b.open("RAM:w", MODE_OLDFILE);
    assert_eq!(b.llamar(lvo::FREAD, &[f, dst, 0x10000, 0x10000]), 0);
    assert_eq!(b.io_err(), ERROR_BAD_NUMBER);
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn de_cerrojo_a_fichero_y_al_reves() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "cajon/dato", b"contenido");
    let l = b.lock("Work:cajon/dato", SHARED_LOCK);
    let f = b.llamar(lvo::OPEN_FROM_LOCK, &[l]);
    assert_ne!(f, 0);
    // El cerrojo ya no vale.
    let fib = b.fib();
    assert_eq!(b.llamar(lvo::EXAMINE, &[l, fib]), 0);
    assert_eq!(b.io_err(), ERROR_INVALID_LOCK);
    b.avisos();
    assert_eq!(b.read(f, 4).1, b"cont");
    // ExamineFH, NameFromFH, ParentOfFH, DupLockFromFH.
    assert_eq!(b.llamar(lvo::EXAMINE_FH, &[f, fib]), DOSTRUE);
    assert_eq!(b.nombre_fib(fib), "dato");
    assert_eq!(b.l32(fib + fib::FIB_SIZE), 9);
    let buf = b.hueco(64);
    assert_eq!(b.llamar(lvo::NAME_FROM_FH, &[f, buf, 64]), DOSTRUE);
    assert_eq!(b.cadena_en(buf), "Work:cajon/dato");
    assert_eq!(b.llamar(lvo::NAME_FROM_FH, &[f, buf, 10]), 0);
    assert_eq!(b.io_err(), ERROR_LINE_TOO_LONG);
    let p = b.llamar(lvo::PARENT_OF_FH, &[f]);
    assert_eq!(b.name_from_lock(p), "Work:cajon");
    let d = b.llamar(lvo::DUP_LOCK_FROM_FH, &[f]);
    assert_eq!(b.name_from_lock(d), "Work:cajon/dato");
    // Un cajón no se abre desde su cerrojo.
    assert_eq!(b.llamar(lvo::OPEN_FROM_LOCK, &[p]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    b.unlock(p);
    b.unlock(d);
    b.close(f);
    // ExamineFH de la consola.
    let out = b.llamar(lvo::OUTPUT, &[]);
    assert_eq!(b.llamar(lvo::EXAMINE_FH, &[out, fib]), 0);
    assert_eq!(b.io_err(), ERROR_ACTION_NOT_KNOWN);
    b.limpio(0, 1);
}

#[test]
fn sin_memoria_en_la_caja() {
    let mut b = Banco::nuevo();
    let n = b.cad("RAM:x");
    b.caja.reservas_restantes = Some(0);
    assert_eq!(b.llamar(lvo::OPEN, &[n, MODE_NEWFILE as u32]), 0);
    assert_eq!(b.io_err(), ERROR_NO_FREE_STORE);
    assert_eq!(b.llamar(lvo::LOCK, &[n, SHARED_LOCK as u32]), 0);
    assert_eq!(b.io_err(), ERROR_NO_FREE_STORE);
    b.caja.reservas_restantes = None;
    // No se quedó ninguna asa abierta en el servicio.
    b.limpio(0, 1);
}

#[test]
fn acabar_tarea_suelta_lo_suyo() {
    let mut b = Banco::nuevo();
    let f = b.open("RAM:a", MODE_NEWFILE);
    let l = b.lock("SYS:S", SHARED_LOCK);
    assert!(f != 0 && l != 0);
    let antes = b.caja.vivos.len();
    let t = b.proceso;
    b.dos.acabar_tarea(&mut b.caja, t);
    assert_eq!(b.caja.vivos.len(), antes - 3);
    assert_eq!(b.dos.cerrojos_abiertos(), 0);
    assert_eq!(b.dos.ficheros_abiertos(), 0);
    assert_eq!(b.dos.fs.abiertas(), 0);
    b.sin_faltas();
}
