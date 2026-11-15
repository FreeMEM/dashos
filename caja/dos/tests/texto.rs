//! ReadArgs, FindArg, StrToLong, Fault, PrintFault, FilePart, PathPart, AddPart, SplitName, VPrintf, variables,
//! LoadSeg, AllocDosObject y lo del proceso.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::*;

/// ReadArgs con `linea` en pr_Arguments. Devuelve el RDArgs y la tabla.
fn read_args(b: &mut Banco, plantilla: &str, linea: &str, n: u32) -> (u32, u32) {
    let a = b.cad(linea);
    b.caja.mem.escribir32(b.proceso + process::PR_ARGUMENTS, a);
    let t = b.cad(plantilla);
    let tabla = b.hueco(4 * n.max(1));
    let r = b.llamar(lvo::READ_ARGS, &[t, tabla, 0]);
    (r, tabla)
}

fn cad_de(b: &Banco, tabla: u32, i: u32) -> String {
    b.cadena_en(b.l32(tabla + 4 * i))
}

#[test]
fn readargs() {
    let mut b = Banco::nuevo();
    let (r, t) = read_args(&mut b, "FROM/A/M,TO/A,ALL/S,QUIET/S,BUF=BUFFER/K/N", "a b \"c d\" dest ALL buf=20\n", 5);
    assert_ne!(r, 0, "{}", b.io_err());
    let lista = b.l32(t);
    let desde: Vec<String> =
        (0..).map(|i| b.l32(lista + 4 * i)).take_while(|&p| p != 0).map(|p| b.cadena_en(p)).collect();
    assert_eq!(desde, ["a", "b", "c d"]);
    assert_eq!(cad_de(&b, t, 1), "dest");
    assert_eq!(b.l32(t + 8), DOSTRUE);
    assert_eq!(b.l32(t + 12), 0);
    assert_eq!(b.l32(b.l32(t + 16)), 20);
    b.llamar(lvo::FREE_ARGS, &[r]);
    // Palabra clave con =, en cualquier sitio, y comillas con escapes.
    let (r, t) = read_args(&mut b, "NAME/A,TEXT/K", "TEXT=\"di *\"hola*\"*N\" fichero", 2);
    assert_ne!(r, 0);
    assert_eq!(cad_de(&b, t, 0), "fichero");
    assert_eq!(cad_de(&b, t, 1), "di \"hola\"\n");
    b.llamar(lvo::FREE_ARGS, &[r]);
    // /F: el resto de la línea.
    let (r, t) = read_args(&mut b, "CMD/A,ARGS/F", "run  esto es  todo  \n", 2);
    assert_ne!(r, 0);
    assert_eq!(cad_de(&b, t, 0), "run");
    assert_eq!(cad_de(&b, t, 1), "esto es  todo");
    b.llamar(lvo::FREE_ARGS, &[r]);
    // Lo que no aparece no se toca.
    let a = b.cad("");
    b.caja.mem.escribir32(b.proceso + process::PR_ARGUMENTS, a);
    let tpl = b.cad("X/K,Y/S");
    let tabla = b.hueco(8);
    b.caja.mem.escribir32(tabla, 0x1234);
    let r = b.llamar(lvo::READ_ARGS, &[tpl, tabla, 0]);
    assert_ne!(r, 0);
    assert_eq!(b.l32(tabla), 0x1234);
    b.llamar(lvo::FREE_ARGS, &[r]);
    // Errores.
    for (plantilla, linea, error) in [
        ("A/A", "", ERROR_REQUIRED_ARG_MISSING),
        ("A", "uno dos", ERROR_TOO_MANY_ARGS),
        ("K/K", "K", ERROR_KEY_NEEDS_ARG),
        ("N/N", "12x", ERROR_BAD_NUMBER),
        ("A,,B", "x", ERROR_BAD_TEMPLATE),
        ("A/Q", "x", ERROR_BAD_TEMPLATE),
        ("A", "\"sin cerrar", ERROR_UNMATCHED_QUOTES),
        ("M/M/A", "", ERROR_REQUIRED_ARG_MISSING),
    ] {
        let vivos = b.caja.vivos.len();
        let (r, _) = read_args(&mut b, plantilla, linea, 4);
        assert_eq!(r, 0, "{plantilla} {linea}");
        assert_eq!(b.io_err(), error, "{plantilla} {linea}");
        assert_eq!(b.caja.vivos.len(), vivos + 3, "{plantilla}: no se queda nada reservado");
    }
    // Con un RDArgs propio y su CSource.
    let rda = b.llamar(lvo::ALLOC_DOS_OBJECT, &[dos_obj::DOS_RDARGS, 0]);
    let src = b.cad("desde CSource\n");
    b.caja.mem.escribir32(rda + rdargs::CS_BUFFER, src);
    b.caja.mem.escribir32(rda + rdargs::CS_LENGTH, 14);
    b.caja.mem.escribir32(rda + rdargs::CS_CURCHR, 6);
    let tpl = b.cad("X/A");
    let tabla = b.hueco(4);
    assert_eq!(b.llamar(lvo::READ_ARGS, &[tpl, tabla, rda]), rda);
    assert_eq!(cad_de(&b, tabla, 0), "CSource");
    b.llamar(lvo::FREE_ARGS, &[rda]);
    b.llamar(lvo::FREE_DOS_OBJECT, &[dos_obj::DOS_RDARGS, rda]);
    // FreeArgs de algo que no es.
    b.llamar(lvo::FREE_ARGS, &[0x8000]);
    assert_eq!(b.avisos().len(), 1);
    // /M/N y /A tras /M.
    let (r, t) = read_args(&mut b, "NUMS/M/N,ULTIMO/A", "1 2 3 4", 2);
    assert_ne!(r, 0);
    let l = b.l32(t);
    let v: Vec<i32> = (0..).map(|i| b.l32(l + 4 * i)).take_while(|&p| p != 0).map(|p| b.l32(p) as i32).collect();
    assert_eq!(v, [1, 2, 3]);
    assert_eq!(cad_de(&b, t, 1), "4");
    b.llamar(lvo::FREE_ARGS, &[r]);
}

#[test]
fn findarg_strtolong() {
    let mut b = Banco::nuevo();
    let t = b.cad("FROM/A,TO=AS/K,ALL/S");
    for (k, i) in [("from", 0), ("AS", 1), ("to", 1), ("all", 2), ("nada", -1)] {
        let k = b.cad(k);
        assert_eq!(b.llamar(lvo::FIND_ARG, &[k, t]) as i32, i);
    }
    let v = b.hueco(4);
    for (s, n, valor) in [("123", 3, 123), ("  -45x", 5, -45), ("+7", 2, 7), ("abc", -1, 0), ("", -1, 0)] {
        b.caja.mem.escribir32(v, 0);
        let s = b.cad(s);
        assert_eq!(b.llamar(lvo::STR_TO_LONG, &[s, v]) as i32, n);
        assert_eq!(b.l32(v) as i32, valor);
    }
}

#[test]
fn fault_y_partes() {
    let mut b = Banco::nuevo();
    let buf = b.hueco(100);
    let h = b.cad("Copy");
    assert_eq!(b.llamar(lvo::FAULT, &[ERROR_OBJECT_NOT_FOUND, h, buf, 100]), 22);
    assert_eq!(b.cadena_en(buf), "Copy: object not found");
    assert_eq!(b.llamar(lvo::FAULT, &[ERROR_DISK_FULL, 0, buf, 100]), 12);
    assert_eq!(b.cadena_en(buf), "disk is full");
    assert_eq!(b.llamar(lvo::FAULT, &[9999, 0, buf, 100]), 15);
    assert_eq!(b.cadena_en(buf), "Error code 9999");
    assert_eq!(b.llamar(lvo::FAULT, &[ERROR_DISK_FULL, 0, buf, 5]), 4);
    assert_eq!(b.cadena_en(buf), "disk");
    assert_eq!(b.llamar(lvo::FAULT, &[ERROR_DISK_FULL, 0, buf, 0]), 0);
    assert_eq!(b.llamar(lvo::PRINT_FAULT, &[ERROR_BREAK, h]), DOSTRUE);
    assert_eq!(b.caja.salida, b"Copy: ***Break\n");
    assert_eq!(b.io_err(), ERROR_BREAK);
    // FilePart, PathPart.
    for (r, fp, pp) in [
        ("xxx:yyy/zzz/qqq", "qqq", "/qqq"),
        ("xxx:yyy", "yyy", "yyy"),
        ("yyy", "yyy", "yyy"),
        ("a/", "", "/"),
        ("xxx:", "", ""),
    ] {
        let s = b.cad(r);
        let x = b.llamar(lvo::FILE_PART, &[s]);
        assert_eq!(b.cadena_en(x), fp, "FilePart {r}");
        let x = b.llamar(lvo::PATH_PART, &[s]);
        assert_eq!(b.cadena_en(x), pp, "PathPart {r}");
    }
    // AddPart.
    for (d, n, esperado) in [
        ("SYS:", "C", "SYS:C"),
        ("SYS:C", "Dir", "SYS:C/Dir"),
        ("SYS:C/", "Dir", "SYS:C/Dir"),
        ("", "x", "x"),
        ("SYS:C", "Work:y", "Work:y"),
    ] {
        let dir = b.hueco(64);
        b.caja.mem.trozo_mut(dir, d.len() as u32).unwrap().copy_from_slice(d.as_bytes());
        let nn = b.cad(n);
        assert_eq!(b.llamar(lvo::ADD_PART, &[dir, nn, 64]), DOSTRUE);
        assert_eq!(b.cadena_en(dir), esperado);
    }
    let dir = b.cad("SYS:abc");
    let nn = b.cad("defgh");
    assert_eq!(b.llamar(lvo::ADD_PART, &[dir, nn, 12]), 0);
    assert_eq!(b.io_err(), ERROR_LINE_TOO_LONG);
    assert_eq!(b.cadena_en(dir), "SYS:abc");
    // SplitName.
    let s = b.cad("uno/dos/tres");
    let buf = b.hueco(8);
    assert_eq!(b.llamar(lvo::SPLIT_NAME, &[s, b'/' as u32, buf, 0, 8]), 4);
    assert_eq!(b.cadena_en(buf), "uno");
    assert_eq!(b.llamar(lvo::SPLIT_NAME, &[s, b'/' as u32, buf, 4, 8]), 8);
    assert_eq!(b.cadena_en(buf), "dos");
    assert_eq!(b.llamar(lvo::SPLIT_NAME, &[s, b'/' as u32, buf, 8, 3]) as i32, -1);
    assert_eq!(b.cadena_en(buf), "tr");
}

#[test]
fn vprintf() {
    let mut b = Banco::nuevo();
    let pr = |b: &mut Banco, f: &str, args: &[u8]| -> String {
        b.caja.salida.clear();
        let f = b.cad(f);
        let a = b.bytes(args);
        let n = b.llamar(lvo::VPRINTF, &[f, a]);
        let s = String::from_utf8(b.caja.salida.clone()).unwrap();
        assert_eq!(n as usize, s.len());
        s
    };
    assert_eq!(pr(&mut b, "%d|%u|%x", &[0xFF, 0xFE, 0, 7, 0, 255]), "-2|7|ff");
    assert_eq!(pr(&mut b, "%ld %lX", &[0xFF, 0xFF, 0xFF, 0xFE, 0, 0, 0xBE, 0xEF]), "-2 BEEF");
    assert_eq!(
        pr(&mut b, "[%5ld][%-5ld][%05ld]", &[0, 0, 0, 42, 0, 0, 0, 42, 0xFF, 0xFF, 0xFF, 0xFF]),
        "[   42][42   ][-0001]"
    );
    assert_eq!(pr(&mut b, "%c%c 100%%", &[0, b'o', 0, b'k']), "ok 100%");
    let s = b.cad("cadena");
    let p = s.to_be_bytes();
    assert_eq!(pr(&mut b, "<%s> <%.3s> <%8s>", &[p, p, p].concat()), "<cadena> <cad> <  cadena>");
    assert_eq!(pr(&mut b, "<%s>", &[0, 0, 0, 0]), "<>");
    // BSTR.
    let bs = b.bytes(&[0, 0, 0, 0, 3, b'a', b'b', b'c']);
    let bp = ((bs + 7) & !3) as usize;
    b.caja.mem.datos[bp..bp + 4].copy_from_slice(&[3, b'a', b'b', b'c']);
    assert_eq!(pr(&mut b, "%b", &((bp as u32) >> 2).to_be_bytes()), "abc");
    // Un %s que apunta fuera: error, sin caerse.
    let f = b.cad("%s");
    let a = b.bytes(&[0xFF, 0xFF, 0x00, 0x00]);
    assert_eq!(b.llamar(lvo::VPRINTF, &[f, a]) as i32, -1);
    assert_eq!(b.avisos().len(), 1);
}

#[test]
fn variables() {
    let mut b = Banco::nuevo();
    let n = b.cad("Idioma");
    let v = b.cad("español\nresto");
    let buf = b.hueco(32);
    // Sin ENV: asignada: la global no va.
    assert_eq!(b.llamar(lvo::SET_VAR, &[n, v, u32::MAX, gvf::GLOBAL_ONLY]), 0);
    assert_eq!(b.io_err(), ERROR_DEVICE_NOT_MOUNTED);
    let r = b.cad("RAM:");
    b.dos.fs.fs.poner_cajon(b.ram, "Env");
    let e = b.cad("ENV");
    let l = b.lock("RAM:Env", SHARED_LOCK);
    let _ = r;
    assert_eq!(b.llamar(lvo::ASSIGN_LOCK, &[e, l]), DOSTRUE);
    assert_eq!(b.llamar(lvo::SET_VAR, &[n, v, u32::MAX, gvf::GLOBAL_ONLY]), DOSTRUE);
    assert_eq!(
        b.dos.fs.fs.contenido(b.ram, "Env/Idioma").unwrap(),
        "español\nresto".chars().map(|c| c as u8).collect::<Vec<_>>()
    );
    // GetVar corta en el salto de línea, salvo con GVF_BINARY_VAR; IoErr() da el largo entero.
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 32, gvf::GLOBAL_ONLY]), 7);
    assert_eq!(b.cadena_en(buf), "español");
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 32, gvf::GLOBAL_ONLY | gvf::BINARY_VAR]), 13);
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 4, 0]), 3);
    assert_eq!(b.cadena_en(buf), "esp");
    assert_eq!(b.io_err(), 7);
    // Una local tapa a la global.
    let loc = b.cad("local");
    assert_eq!(b.llamar(lvo::SET_VAR, &[n, loc, 5, 0]), DOSTRUE);
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 32, 0]), 5);
    assert_eq!(b.cadena_en(buf), "local");
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 32, gvf::GLOBAL_ONLY]), 7);
    assert_eq!(b.llamar(lvo::DELETE_VAR, &[n, gvf::LOCAL_ONLY]), DOSTRUE);
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 32, gvf::LOCAL_ONLY]) as i32, -1);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    assert_eq!(b.llamar(lvo::DELETE_VAR, &[n, gvf::GLOBAL_ONLY]), DOSTRUE);
    assert_eq!(b.llamar(lvo::GET_VAR, &[n, buf, 32, 0]) as i32, -1);
    // Un nombre con ':' no sale de ENV:.
    let malo = b.cad("SYS:S/Startup-Sequence");
    assert_eq!(b.llamar(lvo::GET_VAR, &[malo, buf, 32, gvf::GLOBAL_ONLY]) as i32, -1);
    assert_eq!(b.io_err(), ERROR_INVALID_COMPONENT_NAME);
    b.limpio(1, 1);
}

#[test]
fn loadseg_y_objetos() {
    let mut b = Banco::nuevo();
    let n = b.cad("SYS:C/Dir");
    let seg = b.llamar(lvo::LOAD_SEG, &[n]);
    assert_eq!(seg, 0x1234);
    assert_eq!(b.caja.cargado[0], [0, 0, 3, 0xF3, 1, 2, 3, 4]);
    assert_eq!(b.llamar(lvo::UN_LOAD_SEG, &[seg]), DOSTRUE);
    assert_eq!(b.caja.descargado, [0x1234]);
    b.llamar(lvo::UN_LOAD_SEG, &[seg]);
    assert_eq!(b.avisos().len(), 1);
    let n = b.cad("SYS:S/Startup-Sequence");
    assert_eq!(b.llamar(lvo::LOAD_SEG, &[n]), 0);
    assert_eq!(b.io_err(), ERROR_BAD_HUNK);
    let n = b.cad("SYS:C");
    assert_eq!(b.llamar(lvo::LOAD_SEG, &[n]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    // AllocDosObject y FreeDosObject.
    let f = b.llamar(lvo::ALLOC_DOS_OBJECT, &[dos_obj::DOS_FIB, 0]);
    assert_ne!(f, 0);
    assert_eq!(b.caja.vivos[&f], fib::TAMANO);
    b.llamar(lvo::FREE_DOS_OBJECT, &[dos_obj::DOS_RDARGS, f]);
    assert_eq!(b.avisos().len(), 1, "tipo cambiado");
    b.llamar(lvo::FREE_DOS_OBJECT, &[dos_obj::DOS_FIB, f]);
    b.llamar(lvo::FREE_DOS_OBJECT, &[dos_obj::DOS_FIB, f]);
    assert_eq!(b.avisos().len(), 1, "liberar dos veces");
    assert_eq!(b.llamar(lvo::ALLOC_DOS_OBJECT, &[77, 0]), 0);
    // Funciones que aún no están: aviso y error, sin caerse.
    assert_eq!(b.llamar(lvo::SYSTEM_TAG_LIST, &[0, 0]) as i32, -1);
    assert_eq!(b.io_err(), ERROR_NOT_IMPLEMENTED);
    let a = b.avisos();
    assert!(matches!(a[a.len() - 1].fallo, Fallo::NoImplementada { .. }));
    // Números que no son de dos.
    let mut r = Registros::default();
    for n in [0, 1, 2, 3, 4, 167, 1000, u16::MAX] {
        assert_eq!(b.dos.llamar(&mut b.caja, n, &mut r), Accion::Ajena);
    }
}

#[test]
fn proceso_y_cli() {
    let mut b = Banco::nuevo();
    // IoErr y SetIoErr.
    assert_eq!(b.llamar(lvo::SET_IO_ERR, &[123]), 0);
    assert_eq!(b.llamar(lvo::IO_ERR, &[]), 123);
    // Sin CLI.
    assert_eq!(b.llamar(lvo::CLI, &[]), 0);
    let buf = b.hueco(32);
    assert_eq!(b.llamar(lvo::GET_PROGRAM_NAME, &[buf, 32]), 0);
    // Con CLI: el nombre del mandato en un BSTR.
    let cli = b.hueco(cli::TAMANO);
    let nombre = b.hueco(16);
    b.caja.mem.trozo_mut(nombre, 5).unwrap().copy_from_slice(b"\x04Copy");
    b.caja.mem.escribir32(cli + cli::CLI_COMMANDNAME, nombre >> 2);
    b.caja.mem.escribir32(b.proceso + process::PR_CLI, cli >> 2);
    assert_eq!(b.llamar(lvo::CLI, &[]), cli);
    assert_eq!(b.llamar(lvo::GET_PROGRAM_NAME, &[buf, 32]), DOSTRUE);
    assert_eq!(b.cadena_en(buf), "Copy");
    assert_eq!(b.llamar(lvo::GET_PROGRAM_NAME, &[buf, 3]), 0);
    assert_eq!(b.io_err(), ERROR_LINE_TOO_LONG);
    // GetArgStr, SetArgStr.
    let a = b.cad("args");
    assert_eq!(b.llamar(lvo::SET_ARG_STR, &[a]), 0);
    assert_eq!(b.llamar(lvo::GET_ARG_STR, &[]), a);
    // CheckSignal (Ctrl-C).
    b.caja.senales = 1 << 12;
    assert_eq!(b.llamar(lvo::CHECK_SIGNAL, &[1 << 12]), 1 << 12);
    assert_eq!(b.llamar(lvo::CHECK_SIGNAL, &[1 << 12]), 0);
    // Una tarea que no es un proceso: dos guarda sus campos aparte y no se cae.
    let t = b.hueco(92);
    b.caja.tarea = t;
    assert_eq!(b.llamar(lvo::SET_IO_ERR, &[7]), 0);
    assert_eq!(b.llamar(lvo::IO_ERR, &[]), 7);
    assert_eq!(b.llamar(lvo::OUTPUT, &[]), 0);
    let s = b.cad("x");
    assert_eq!(b.llamar(lvo::PUT_STR, &[s]) as i32, -1);
    let l = b.lock("SYS:S", SHARED_LOCK);
    assert_ne!(l, 0);
    b.unlock(l);
    b.dos.acabar_tarea(&mut b.caja, t);
    // Y la tarea en la página 0 o fuera de la memoria.
    for t in [0, 4, TAM_MEMORIA as u32 - 8, 0xFFFF_FFF0] {
        b.caja.tarea = t;
        assert_eq!(b.llamar(lvo::IO_ERR, &[]), 0);
        b.llamar(lvo::SET_IO_ERR, &[1]);
    }
}
