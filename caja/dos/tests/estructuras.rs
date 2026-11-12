//! Disposición de las estructuras de dos.library 40 (NDK 3.1), la tabla de saltos y las conversiones.

mod comun;
use comun::*;

use caja_dos::estructuras::*;
use caja_dos::fs::bits;
use caja_dos::*;

#[test]
fn tamanos() {
    assert_eq!(datestamp::TAMANO, 12);
    assert_eq!(fib::TAMANO, 260);
    assert_eq!(filehandle::TAMANO, 44);
    assert_eq!(filelock::TAMANO, 20);
    assert_eq!(infodata::TAMANO, 36);
    assert_eq!(doslist::TAMANO, 44);
    assert_eq!(assignlist::TAMANO, 8);
    assert_eq!(process::TAMANO, 228);
    assert_eq!(cli::TAMANO, 64);
    assert_eq!(rdargs::TAMANO, 32);
    assert_eq!(rdargs::CSOURCE_TAMANO, 12);
}

#[test]
fn desplazamientos_fib() {
    assert_eq!(fib::FIB_DISKKEY, 0);
    assert_eq!(fib::FIB_DIRENTRYTYPE, 4);
    assert_eq!(fib::FIB_FILENAME, 8);
    assert_eq!(fib::FIB_FILENAME + fib::LARGO_NOMBRE, fib::FIB_PROTECTION);
    assert_eq!(fib::FIB_PROTECTION, 116);
    assert_eq!(fib::FIB_ENTRYTYPE, 120);
    assert_eq!(fib::FIB_SIZE, 124);
    assert_eq!(fib::FIB_NUMBLOCKS, 128);
    assert_eq!(fib::FIB_DATE, 132);
    assert_eq!(fib::FIB_DATE + datestamp::TAMANO, fib::FIB_COMMENT);
    assert_eq!(fib::FIB_COMMENT + fib::LARGO_COMENTARIO, fib::FIB_OWNERUID);
    assert_eq!(fib::FIB_OWNERUID, 224);
    assert_eq!(fib::FIB_OWNERGID, 226);
    assert_eq!(fib::FIB_RESERVED + 32, fib::TAMANO);
}

#[test]
fn desplazamientos_filehandle_filelock_infodata() {
    let fh = [
        filehandle::FH_LINK,
        filehandle::FH_PORT,
        filehandle::FH_TYPE,
        filehandle::FH_BUF,
        filehandle::FH_POS,
        filehandle::FH_END,
        filehandle::FH_FUNCS,
        filehandle::FH_FUNC2,
        filehandle::FH_FUNC3,
        filehandle::FH_ARGS,
        filehandle::FH_ARG2,
    ];
    for (i, d) in fh.iter().enumerate() {
        assert_eq!(*d, 4 * i as u32);
    }
    let fl = [filelock::FL_LINK, filelock::FL_KEY, filelock::FL_ACCESS, filelock::FL_TASK, filelock::FL_VOLUME];
    for (i, d) in fl.iter().enumerate() {
        assert_eq!(*d, 4 * i as u32);
    }
    let id = [
        infodata::ID_NUMSOFTERRORS,
        infodata::ID_UNITNUMBER,
        infodata::ID_DISKSTATE,
        infodata::ID_NUMBLOCKS,
        infodata::ID_NUMBLOCKSUSED,
        infodata::ID_BYTESPERBLOCK,
        infodata::ID_DISKTYPE,
        infodata::ID_VOLUMENODE,
        infodata::ID_INUSE,
    ];
    for (i, d) in id.iter().enumerate() {
        assert_eq!(*d, 4 * i as u32);
    }
    assert_eq!(infodata::ID_INTER_FFS_DISK, u32::from_be_bytes(*b"DOS\x03"));
}

#[test]
fn desplazamientos_doslist_process_cli() {
    assert_eq!(doslist::DOL_NEXT, 0);
    assert_eq!(doslist::DOL_TYPE, 4);
    assert_eq!(doslist::DOL_TASK, 8);
    assert_eq!(doslist::DOL_LOCK, 12);
    assert_eq!(doslist::DOL_VOLUMEDATE, 16);
    assert_eq!(doslist::DOL_LOCKLIST, 28);
    assert_eq!(doslist::DOL_DISKTYPE, 32);
    assert_eq!(doslist::DOL_ASSIGNNAME, 16);
    assert_eq!(doslist::DOL_LIST, 20);
    assert_eq!(doslist::DOL_GLOBVEC, 36);
    assert_eq!(doslist::DOL_NAME, 40);
    // Process: Task (92) y MsgPort (34) delante.
    assert_eq!(process::PR_MSGPORT, 92);
    assert_eq!(process::PR_PAD, 92 + 34);
    assert_eq!(process::PR_SEGLIST, 128);
    assert_eq!(process::PR_RESULT2, 148);
    assert_eq!(process::PR_CURRENTDIR, 152);
    assert_eq!(process::PR_CIS, 156);
    assert_eq!(process::PR_COS, 160);
    assert_eq!(process::PR_CLI, 172);
    assert_eq!(process::PR_WINDOWPTR, 184);
    assert_eq!(process::PR_HOMEDIR, 188);
    assert_eq!(process::PR_ARGUMENTS, 204);
    assert_eq!(process::PR_LOCALVARS, 208);
    assert_eq!(process::PR_SHELLPRIVATE, 220);
    assert_eq!(process::PR_CES, 224);
    assert_eq!(process::PR_CES + 4, process::TAMANO);
    assert_eq!(cli::CLI_COMMANDNAME, 16);
    assert_eq!(cli::CLI_PROMPT, 24);
    assert_eq!(cli::CLI_MODULE + 4, cli::TAMANO);
    assert_eq!(rdargs::RDA_DALIST, 12);
    assert_eq!(rdargs::RDA_FLAGS + 4, rdargs::TAMANO);
}

#[test]
fn tabla_de_saltos() {
    use lvo::*;
    let casos: &[(u16, i32, &str, &str)] = &[
        (OPEN, -30, "Open", "d1/d2"),
        (CLOSE, -36, "Close", "d1"),
        (READ, -42, "Read", "d1/d2/d3"),
        (LOCK, -84, "Lock", "d1/d2"),
        (IO_ERR, -132, "IoErr", ""),
        (LOAD_SEG, -150, "LoadSeg", "d1"),
        (SET_PROTECTION, -186, "SetProtection", "d1/d2"),
        (PARENT_DIR, -210, "ParentDir", "d1"),
        (ALLOC_DOS_OBJECT, -228, "AllocDosObject", "d1/d2"),
        (FGET_C, -306, "FGetC", "d1"),
        (VFPRINTF, -354, "VFPrintf", "d1/d2/d3"),
        (NAME_FROM_LOCK, -402, "NameFromLock", "d1/d2/d3"),
        (SET_FILE_SIZE, -456, "SetFileSize", "d1/d2/d3"),
        (SET_IO_ERR, -462, "SetIoErr", "d1"),
        (CLI, -492, "Cli", ""),
        (CREATE_NEW_PROC, -498, "CreateNewProc", "d1"),
        (SYSTEM_TAG_LIST, -606, "SystemTagList", "d1/d2"),
        (ASSIGN_LOCK, -612, "AssignLock", "d1/d2"),
        (LOCK_DOS_LIST, -654, "LockDosList", "d1"),
        (INTERNAL_LOAD_SEG, -756, "InternalLoadSeg", "d0/a0/a1/a2"),
        (READ_ARGS, -798, "ReadArgs", "d1/d2/d3"),
        (MATCH_FIRST, -822, "MatchFirst", "d1/d2"),
        (FREE_ARGS, -858, "FreeArgs", "d1"),
        (FILE_PART, -870, "FilePart", "d1"),
        (SET_VAR, -900, "SetVar", "d1/d2/d3/d4"),
        (GET_VAR, -906, "GetVar", "d1/d2/d3/d4"),
        (PUT_STR, -948, "PutStr", "d1"),
        (VPRINTF, -954, "VPrintf", "d1/d2"),
        (SAME_DEVICE, -984, "SameDevice", "d1/d2"),
        (EXALL_END, -990, "ExAllEnd", "d1/d2/d3/d4/d5"),
        (SET_OWNER, -996, "SetOwner", "d1/d2"),
    ];
    for &(n, l, nombre_fn, regs) in casos {
        assert_eq!(lvo::lvo(n), l, "{nombre_fn}");
        assert_eq!(lvo::entrada(l), Some(n));
        assert_eq!(lvo::nombre(n), nombre_fn);
        assert_eq!(FUNCIONES[(n - PRIMERA) as usize].1, regs, "{nombre_fn}");
    }
    assert_eq!(ULTIMA, 166);
    assert_eq!(lvo::entrada(-24), None);
    assert_eq!(lvo::entrada(-1002), None);
    assert_eq!(lvo::entrada(-31), None);
}

#[test]
fn proteccion_ida_y_vuelta() {
    // rwxr-x--- sin nada del Amiga: RWED del dueño permitidos (a 0), grupo r-x y "borrar" de grupo y otros.
    let p = proteccion_de(0o750, 0);
    assert_eq!(p & 0xF, 0);
    assert_eq!(p & fibf::GRP_READ, fibf::GRP_READ);
    assert_eq!(p & fibf::GRP_WRITE, 0);
    assert_eq!(p & fibf::GRP_EXECUTE, fibf::GRP_EXECUTE);
    assert_eq!(p & (fibf::GRP_DELETE | fibf::OTR_DELETE), fibf::GRP_DELETE | fibf::OTR_DELETE);
    assert_eq!(p & (fibf::OTR_READ | fibf::OTR_WRITE | fibf::OTR_EXECUTE), 0);
    // Solo lectura y "no borrar": W y D a 1.
    let p = proteccion_de(0o444, bits::NO_BORRAR_DUENO | bits::GUION);
    assert_eq!(p & 0xF, fibf::WRITE | fibf::EXECUTE | fibf::DELETE);
    assert_eq!(p & fibf::SCRIPT, fibf::SCRIPT);
    for modo in [0o000, 0o644, 0o755, 0o777, 0o4711, 0o1777] {
        for amiga in [0, 1, 2, 4, 8, 16, 32, 64, 0x7F] {
            let p = proteccion_de(modo, amiga);
            assert_eq!(de_proteccion(p, modo), (amiga, modo), "{modo:o} {amiga:#x}");
        }
    }
}

#[test]
fn estructuras_en_la_caja() {
    let mut b = Banco::nuevo();
    // Un FileLock: alineado, con su acceso, su puerto y su DosList (con el nombre del volumen en BSTR).
    let l = b.lock("SYS:S", EXCLUSIVE_LOCK);
    assert_ne!(l, 0);
    let fl = l << 2;
    assert_eq!(b.l32(fl + filelock::FL_ACCESS) as i32, EXCLUSIVE_LOCK);
    let dl = b.l32(fl + filelock::FL_VOLUME) << 2;
    assert_eq!(b.l32(dl + doslist::DOL_TYPE), doslist::DLT_VOLUME);
    assert_eq!(b.l32(dl + doslist::DOL_TASK), b.l32(fl + filelock::FL_TASK));
    assert_ne!(b.l32(fl + filelock::FL_TASK), 0);
    let nombre = b.l32(dl + doslist::DOL_NAME) << 2;
    assert_eq!(b.leer(nombre, 4), b"\x03SYS");
    assert_eq!(b.l32(dl + doslist::DOL_DISKTYPE), infodata::ID_INTER_FFS_DISK);
    // fl_Key: el mismo para dos cerrojos del mismo objeto, otro para otro.
    let l2 = b.lock("sys:s", SHARED_LOCK);
    assert_eq!(l2, 0, "exclusivo: no se puede otro");
    b.unlock(l);
    let l1 = b.lock("SYS:S", SHARED_LOCK);
    let l2 = b.lock("sys:s/", SHARED_LOCK);
    let l3 = b.lock("SYS:C", SHARED_LOCK);
    assert_eq!(b.l32((l1 << 2) + filelock::FL_KEY), b.l32((l2 << 2) + filelock::FL_KEY));
    assert_ne!(b.l32((l1 << 2) + filelock::FL_KEY), b.l32((l3 << 2) + filelock::FL_KEY));
    for x in [l1, l2, l3] {
        b.unlock(x);
    }
    // FileHandle: la consola es interactiva; NIL: no tiene puerto.
    let cons = b.llamar(lvo::OUTPUT, &[]);
    assert_eq!(b.l32((cons << 2) + filehandle::FH_PORT), DOSTRUE);
    let nil = b.open("NIL:", MODE_NEWFILE);
    assert_ne!(nil, 0);
    assert_eq!(b.l32((nil << 2) + filehandle::FH_TYPE), 0);
    assert_eq!(b.l32((nil << 2) + filehandle::FH_PORT), 0);
    b.close(nil);
    let f = b.open("SYS:S/Startup-Sequence", MODE_OLDFILE);
    assert_ne!(b.l32((f << 2) + filehandle::FH_TYPE), 0);
    assert_eq!(b.l32((f << 2) + filehandle::FH_PORT), 0);
    b.close(f);
    b.limpio(0, 1);
}

#[test]
fn datestamp_y_fechas() {
    let mut b = Banco::nuevo();
    // 1978-01-01 00:00:00 es el día 0.
    b.caja.hora = 2922 * 86400 + 86400 + 3600 + 61;
    let ds = b.hueco(12);
    assert_eq!(b.llamar(lvo::DATE_STAMP, &[ds]), ds);
    assert_eq!(b.l32(ds), 1);
    assert_eq!(b.l32(ds + 4), 61);
    assert_eq!(b.l32(ds + 8), 50);
    // CompareDates: <0 si la primera es más tarde.
    let ds2 = b.hueco(12);
    b.caja.hora += 1;
    b.llamar(lvo::DATE_STAMP, &[ds2]);
    assert_eq!(b.llamar(lvo::COMPARE_DATES, &[ds2, ds]) as i32, -1);
    assert_eq!(b.llamar(lvo::COMPARE_DATES, &[ds, ds2]) as i32, 1);
    assert_eq!(b.llamar(lvo::COMPARE_DATES, &[ds, ds]), 0);
    // La fecha de un fichero sale en su FileInfoBlock.
    b.dos.fs.fs.reloj = 2922 * 86400 + 2 * 86400 + 120;
    let f = b.open("RAM:x", MODE_NEWFILE);
    b.close(f);
    let l = b.lock("RAM:x", SHARED_LOCK);
    let (_, fib) = b.examine(l);
    assert_eq!(b.l32(fib + fib::FIB_DATE), 2);
    assert_eq!(b.l32(fib + fib::FIB_DATE + 4), 2);
    assert_eq!(b.l32(fib + fib::FIB_DATE + 8), 0);
    b.unlock(l);
    // Delay: la caja duerme a la tarea.
    let mut r = Registros::default();
    r.d[1] = 50;
    assert_eq!(b.dos.llamar(&mut b.caja, lvo::DELAY, &mut r), Accion::Dormir { tics: 50 });
}
