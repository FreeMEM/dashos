//! Asignaciones: AssignLock, AssignAdd, AssignLate, AssignPath, RemAssignList, bucles, las capas de S4b y la lista de
//! DosList que ven los programas.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::*;

fn assign_lock(b: &mut Banco, nombre: &str, ruta: &str) -> u32 {
    let l = if ruta.is_empty() { 0 } else { b.lock(ruta, SHARED_LOCK) };
    let n = b.cad(nombre);
    let r = b.llamar(lvo::ASSIGN_LOCK, &[n, l]);
    if r == 0 && l != 0 {
        b.unlock(l);
    }
    r
}

#[test]
fn assign_lock_y_multiasignacion() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.work, "uno/solo1", b"1");
    b.dos.fs.fs.poner(b.work, "uno/comun", b"de uno");
    b.dos.fs.fs.poner(b.work, "dos/solo2", b"2");
    b.dos.fs.fs.poner(b.work, "dos/comun", b"de dos");
    assert_eq!(assign_lock(&mut b, "X", "Work:uno"), DOSTRUE);
    let f = b.open("x:solo1", MODE_OLDFILE);
    assert_eq!(b.read(f, 9).1, b"1");
    b.close(f);
    assert_eq!(b.open("X:solo2", MODE_OLDFILE), 0);
    // AssignAdd: se busca en orden.
    let l = b.lock("Work:dos", SHARED_LOCK);
    let n = b.cad("X");
    assert_eq!(b.llamar(lvo::ASSIGN_ADD, &[n, l]), DOSTRUE);
    for (r, esperado) in [("X:solo1", "1"), ("X:solo2", "2"), ("X:comun", "de uno")] {
        let f = b.open(r, MODE_OLDFILE);
        assert_ne!(f, 0, "{r}");
        assert_eq!(b.read(f, 9).1, esperado.as_bytes());
        b.close(f);
    }
    // Lo nuevo va al primero.
    let f = b.open("X:nuevo", MODE_NEWFILE);
    b.close(f);
    assert!(b.dos.fs.fs.existe(b.work, "uno/nuevo"));
    // El cerrojo de una asignación es de dos: el programa no lo suelta.
    b.unlock(l);
    assert!(matches!(b.avisos()[0].fallo, Fallo::CerrojoDeAsignacion { .. }));
    let f = b.open("X:solo2", MODE_OLDFILE);
    assert_ne!(f, 0);
    b.close(f);
    // Un cajón con una asignación encima no se borra.
    let w = b.cad("Work:dos/solo2");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[w]), DOSTRUE);
    let w = b.cad("Work:dos");
    assert_eq!(b.llamar(lvo::DELETE_FILE, &[w]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_IN_USE);
    // RemAssignList: quita el segundo.
    let otro = b.lock("Work:dos", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::REM_ASSIGN_LIST, &[n, otro]), DOSTRUE);
    assert_eq!(b.llamar(lvo::REM_ASSIGN_LIST, &[n, otro]), 0);
    b.unlock(otro);
    assert_eq!(b.open("X:solo2", MODE_OLDFILE), 0);
    // Quitar la asignación.
    assert_eq!(assign_lock(&mut b, "X", ""), DOSTRUE);
    assert_eq!(b.lock("X:", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_DEVICE_NOT_MOUNTED);
    assert_eq!(b.dos.cerrojos_abiertos(), 0);
    // AssignAdd a una que no existe.
    let l = b.lock("Work:uno", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::ASSIGN_ADD, &[n, l]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    b.unlock(l);
    b.limpio(0, 1);
}

#[test]
fn asignaciones_que_no_valen() {
    let mut b = Banco::nuevo();
    // Con el nombre de un volumen.
    assert_eq!(assign_lock(&mut b, "Work", "SYS:S"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_EXISTS);
    assert_eq!(assign_lock(&mut b, "ram:", "SYS:S"), 0);
    // Nombres raros.
    for n in ["", "a:b", "a/b", ":"] {
        assert_eq!(assign_lock(&mut b, n, "SYS:S"), 0, "{n:?}");
        assert_eq!(b.io_err(), ERROR_INVALID_COMPONENT_NAME, "{n:?}");
    }
    // A un fichero.
    assert_eq!(assign_lock(&mut b, "F", "SYS:C/Dir"), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    // Con un cerrojo que no es de dos.
    let n = b.cad("Z");
    assert_eq!(b.llamar(lvo::ASSIGN_LOCK, &[n, 0x5000]), 0);
    assert_eq!(b.io_err(), ERROR_INVALID_LOCK);
    assert_eq!(b.avisos().len(), 1);
    // El mismo cerrojo en dos asignaciones.
    let l = b.lock("SYS:S", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::ASSIGN_LOCK, &[n, l]), DOSTRUE);
    let m = b.cad("Z2");
    assert_eq!(b.llamar(lvo::ASSIGN_LOCK, &[m, l]), 0);
    assert!(matches!(b.avisos()[0].fallo, Fallo::CerrojoDeAsignacion { .. }));
    // Cambiar una asignación suelta el cerrojo de antes.
    assert_eq!(assign_lock(&mut b, "Z", "SYS:C"), DOSTRUE);
    assert_eq!(b.dos.cerrojos_abiertos(), 1);
    let x = b.lock("Z:", SHARED_LOCK);
    assert_eq!(b.name_from_lock(x), "SYS:C");
    b.unlock(x);
    b.limpio(1, 1);
}

#[test]
fn tarde_y_ruta() {
    let mut b = Banco::nuevo();
    let n = b.cad("LATE");
    let r = b.cad("Work:luego");
    assert_eq!(b.llamar(lvo::ASSIGN_LATE, &[n, r]), DOSTRUE);
    // Aún no existe: falla al usarla, y sigue siendo tardía.
    assert_eq!(b.lock("LATE:", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_NOT_FOUND);
    b.dos.fs.fs.poner(b.work, "luego/f", b"L");
    let f = b.open("LATE:f", MODE_OLDFILE);
    assert_ne!(f, 0);
    b.close(f);
    // Ya está fijada: aunque cambie de nombre, sigue al cajón.
    let (x, y) = (b.cad("Work:luego"), b.cad("Work:movido"));
    assert_eq!(b.llamar(lvo::RENAME, &[x, y]), DOSTRUE);
    let l = b.lock("LATE:", SHARED_LOCK);
    assert_eq!(b.name_from_lock(l), "Work:movido");
    b.unlock(l);
    // AssignPath se resuelve cada vez.
    let n = b.cad("CAMINO");
    let r = b.cad("Work:cambia");
    assert_eq!(b.llamar(lvo::ASSIGN_PATH, &[n, r]), DOSTRUE);
    b.dos.fs.fs.poner(b.work, "cambia/a", b"1");
    let l = b.lock("CAMINO:a", SHARED_LOCK);
    assert_ne!(l, 0);
    b.unlock(l);
    b.dos.fs.fs.borrar_por_fuera(b.work, "cambia");
    b.dos.fs.fs.poner(b.work, "cambia/b", b"2");
    assert_eq!(b.lock("CAMINO:a", SHARED_LOCK), 0);
    let l = b.lock("CAMINO:b", SHARED_LOCK);
    assert_ne!(l, 0);
    b.unlock(l);
    b.limpio(1, 1);
}

#[test]
fn bucles_de_asignaciones() {
    let mut b = Banco::nuevo();
    let path = |b: &mut Banco, n: &str, r: &str, late: bool| {
        let (n, r) = (b.cad(n), b.cad(r));
        b.llamar(if late { lvo::ASSIGN_LATE } else { lvo::ASSIGN_PATH }, &[n, r])
    };
    assert_eq!(path(&mut b, "A", "B:", false), DOSTRUE);
    assert_eq!(path(&mut b, "B", "A:", false), DOSTRUE);
    assert_eq!(b.lock("A:x", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), ERROR_TOO_MANY_LEVELS);
    assert_eq!(path(&mut b, "YO", "YO:sub", true), DOSTRUE);
    assert_eq!(b.open("YO:x", MODE_NEWFILE), 0);
    assert_eq!(b.io_err(), ERROR_TOO_MANY_LEVELS);
    // Una cadena larga pero sin bucle sí llega.
    assert_eq!(path(&mut b, "C0", "RAM:", false), DOSTRUE);
    for i in 1..10 {
        assert_eq!(path(&mut b, &format!("C{i}"), &format!("C{}:", i - 1), false), DOSTRUE);
    }
    let l = b.lock("C9:", SHARED_LOCK);
    assert_ne!(l, 0);
    assert_eq!(b.name_from_lock(l), "RAM:");
    b.unlock(l);
    // Un bucle a través de una multiasignación.
    let l = b.lock("RAM:", SHARED_LOCK);
    let n = b.cad("M");
    b.llamar(lvo::ASSIGN_LOCK, &[n, l]);
    assert_eq!(path(&mut b, "N", "M:", false), DOSTRUE);
    let f = b.open("N:hola", MODE_NEWFILE);
    assert_ne!(f, 0);
    b.close(f);
    b.limpio(1, 1);
}

#[test]
fn lista_de_dos() {
    let mut b = Banco::nuevo();
    assert_eq!(assign_lock(&mut b, "LIBS", "SYS:Libs"), DOSTRUE);
    let l = b.lock("SYS:C", SHARED_LOCK);
    let n = b.cad("LIBS");
    b.llamar(lvo::ASSIGN_ADD, &[n, l]);
    let (x, y) = (b.cad("TARDE"), b.cad("RAM:t"));
    b.llamar(lvo::ASSIGN_LATE, &[x, y]);
    let cab = b.llamar(lvo::LOCK_DOS_LIST, &[ldf::ALL | ldf::READ]);
    assert_ne!(cab, 0);
    let nombre = |b: &Banco, dl: u32| {
        let s = b.l32(dl + doslist::DOL_NAME) << 2;
        let n = b.caja.mem.leer8(s).unwrap() as u32;
        String::from_utf8(b.leer(s + 1, n)).unwrap()
    };
    // Volúmenes.
    let mut vols = Vec::new();
    let mut d = cab;
    loop {
        d = b.llamar(lvo::NEXT_DOS_ENTRY, &[d, ldf::VOLUMES]);
        if d == 0 {
            break;
        }
        assert_eq!(b.l32(d + doslist::DOL_TYPE), doslist::DLT_VOLUME);
        vols.push(nombre(&b, d));
    }
    assert_eq!(vols, ["SYS", "Work", "RAM", "CD0"]);
    // Asignaciones.
    let mut asig = Vec::new();
    let mut d = cab;
    loop {
        d = b.llamar(lvo::NEXT_DOS_ENTRY, &[d, ldf::ASSIGNS]);
        if d == 0 {
            break;
        }
        asig.push((nombre(&b, d), b.l32(d + doslist::DOL_TYPE)));
    }
    assert_eq!(asig, [("LIBS".to_string(), doslist::DLT_DIRECTORY), ("TARDE".to_string(), doslist::DLT_LATE)]);
    // La multiasignación: dol_Lock y un AssignList con el segundo.
    let n = b.cad("libs:");
    let libs = b.llamar(lvo::FIND_DOS_ENTRY, &[cab, n, ldf::ASSIGNS]);
    assert_ne!(libs, 0);
    let l1 = b.l32(libs + doslist::DOL_LOCK);
    assert_eq!(b.name_from_lock(l1), "SYS:Libs");
    let al = b.l32(libs + doslist::DOL_LIST);
    assert_ne!(al, 0);
    assert_eq!(b.l32(al + assignlist::AL_NEXT), 0);
    assert_eq!(b.l32(al + assignlist::AL_LOCK), l);
    // La tardía lleva su ruta.
    let n = b.cad("TARDE");
    let t = b.llamar(lvo::FIND_DOS_ENTRY, &[cab, n, ldf::ASSIGNS]);
    assert_eq!(b.cadena_en(b.l32(t + doslist::DOL_ASSIGNNAME)), "RAM:t");
    // Buscar un volumen con FindDosEntry, y lo que no hay.
    let n = b.cad("work");
    let w = b.llamar(lvo::FIND_DOS_ENTRY, &[cab, n, ldf::VOLUMES]);
    assert_eq!(nombre(&b, w), "Work");
    assert_eq!(b.llamar(lvo::FIND_DOS_ENTRY, &[cab, n, ldf::ASSIGNS]), 0);
    // Una entrada que no es de la lista.
    assert_eq!(b.llamar(lvo::NEXT_DOS_ENTRY, &[0x7000, ldf::ALL]), 0);
    b.llamar(lvo::UN_LOCK_DOS_LIST, &[ldf::ALL | ldf::READ]);
    // MakeDosEntry y FreeDosEntry.
    let n = b.cad("NUEVO");
    let e = b.llamar(lvo::MAKE_DOS_ENTRY, &[n, doslist::DLT_DIRECTORY]);
    assert_ne!(e, 0);
    assert_eq!(nombre(&b, e), "NUEVO");
    b.llamar(lvo::FREE_DOS_ENTRY, &[e]);
    b.llamar(lvo::FREE_DOS_ENTRY, &[e]);
    assert_eq!(b.avisos().len(), 1);
    b.limpio(2, 1);
}

#[test]
fn capas_de_s4b() {
    let mut fs = MemFs::nuevo();
    let sys = fs.nuevo_volumen(true, 1 << 30);
    let usuarios = fs.nuevo_volumen(false, 1 << 30);
    let paquetes = fs.nuevo_volumen(true, 1 << 30);
    let ram = fs.nuevo_volumen(false, 1 << 20);
    for c in ["C", "Libs", "Devs", "Fonts", "Locale", "S", "Prefs/Env-Archive/Sys"] {
        fs.poner_cajon(sys, c);
    }
    fs.poner(sys, "Libs/mui.library", b"de SYS");
    fs.poner(sys, "Libs/vieja.library", b"de SYS");
    fs.poner(sys, "Prefs/Env-Archive/Sys/font.prefs", b"fabrica");
    fs.poner(paquetes, "MUI/Libs/mui.library", b"del paquete");
    fs.poner(paquetes, "Otro/C/Mandato", b"m");
    fs.poner(usuarios, "ana/Libs/vieja.library", b"de ana");
    fs.poner_cajon(usuarios, "ana/Prefs/Env-Archive");
    let mut caja = CajaPrueba::nueva();
    let p = caja.reservar(process::TAMANO);
    caja.mem.escribir8(p + process::LN_TYPE, process::NT_PROCESS);
    caja.tarea = p;
    let mut dos = Dos::nuevo(fs);
    dos.montar(&mut caja, "SYS", sys, true).unwrap();
    dos.montar(&mut caja, "Usuarios", usuarios, false).unwrap();
    dos.montar(&mut caja, "Paquetes", paquetes, true).unwrap();
    dos.montar(&mut caja, "RAM", ram, false).unwrap();
    assert_eq!(dos.montar(&mut caja, "ram", ram, false), Err(ERROR_OBJECT_EXISTS));
    let fallos =
        dos.asignaciones_s4b(&mut caja, &CapasS4b { usuario: "ana", paquetes: &["MUI", "Otro"], encerrado: false });
    assert_eq!(fallos, vec![]);
    let leer = |dos: &mut Dos<MemFs>, caja: &mut CajaPrueba, ruta: &str| -> Option<Vec<u8>> {
        let n = caja.reservar(ruta.len() as u32 + 1);
        caja.mem.trozo_mut(n, ruta.len() as u32).unwrap().copy_from_slice(ruta.as_bytes());
        let mut r = Registros::default();
        r.d[1] = n;
        r.d[2] = MODE_OLDFILE as u32;
        dos.llamar(caja, lvo::OPEN, &mut r);
        let fh = r.d[0];
        if fh == 0 {
            return None;
        }
        let buf = caja.reservar(64);
        r.d = [0, fh, buf, 64, 0, 0, 0, 0];
        dos.llamar(caja, lvo::READ, &mut r);
        let v = caja.mem.trozo(buf, r.d[0]).unwrap().to_vec();
        r.d[1] = fh;
        dos.llamar(caja, lvo::CLOSE, &mut r);
        Some(v)
    };
    // Usuario, paquetes, sistema: en ese orden.
    assert_eq!(leer(&mut dos, &mut caja, "LIBS:vieja.library").unwrap(), b"de ana");
    assert_eq!(leer(&mut dos, &mut caja, "LIBS:mui.library").unwrap(), b"del paquete");
    assert_eq!(leer(&mut dos, &mut caja, "C:Mandato").unwrap(), b"m");
    assert_eq!(leer(&mut dos, &mut caja, "ENVARC:Sys/font.prefs").unwrap(), b"fabrica");
    // Lo que se escribe va a la primera capa que se deja (la del usuario), y ya se lee de ahí.
    let mut r = Registros::default();
    let ruta = "ENVARC:font.prefs";
    let n = caja.reservar(64);
    caja.mem.trozo_mut(n, ruta.len() as u32).unwrap().copy_from_slice(ruta.as_bytes());
    r.d[1] = n;
    r.d[2] = MODE_NEWFILE as u32;
    dos.llamar(&mut caja, lvo::OPEN, &mut r);
    assert_ne!(r.d[0], 0);
    r.d[1] = r.d[0];
    dos.llamar(&mut caja, lvo::CLOSE, &mut r);
    assert!(dos.fs.existe(usuarios, "ana/Prefs/Env-Archive/font.prefs"));
    // Si la capa del usuario no tiene el cajón, cae en la de SYS:, que es de solo lectura.
    let ruta = "ENVARC:Sys/font.prefs";
    caja.mem.trozo_mut(n, ruta.len() as u32 + 1).unwrap()[..ruta.len()].copy_from_slice(ruta.as_bytes());
    caja.mem.escribir8(n + ruta.len() as u32, 0);
    r.d[1] = n;
    r.d[2] = MODE_NEWFILE as u32;
    dos.llamar(&mut caja, lvo::OPEN, &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(caja.mem.leer32(p + process::PR_RESULT2).unwrap(), ERROR_DISK_WRITE_PROTECTED);
    // ENV: y T: en RAM:, creados.
    assert!(dos.fs.existe(ram, "Env"));
    assert!(dos.fs.existe(ram, "T"));
    // Home:.
    assert_eq!(leer(&mut dos, &mut caja, "Home:Libs/vieja.library").unwrap(), b"de ana");
    // Encerrado: sin la capa del usuario en LIBS:.
    let fallos = dos.asignaciones_s4b(&mut caja, &CapasS4b { usuario: "ana", paquetes: &["MUI"], encerrado: true });
    assert_eq!(fallos, vec![]);
    assert_eq!(leer(&mut dos, &mut caja, "LIBS:vieja.library").unwrap(), b"de SYS");
    // DEVS: no tiene capa de usuario nunca; con un paquete que no tiene Devs se salta.
    assert!(leer(&mut dos, &mut caja, "DEVS:").is_none());
    assert!(dos.avisos().is_empty(), "{:?}", dos.avisos());
}

/// `asignar_creando`: el `T:` y el `ENV:` de la caja, en su cajón de escribir; el cajón se crea si falta, y si ya está
/// no se toca. En un volumen de solo lectura, o con un nombre malo, falla sin asignar.
#[test]
fn asignar_creando_el_cajon() {
    let mut fs = MemFs::nuevo();
    let caja_v = fs.nuevo_volumen(false, 1 << 20);
    let sys = fs.nuevo_volumen(true, 1 << 20);
    fs.poner(caja_v, "Env/ya", b"x");
    let mut caja = CajaPrueba::nueva();
    let p = caja.reservar(process::TAMANO);
    caja.mem.escribir8(p + process::LN_TYPE, process::NT_PROCESS);
    caja.tarea = p;
    let mut dos = Dos::nuevo(fs);
    dos.montar(&mut caja, "Caja", caja_v, false).unwrap();
    dos.montar(&mut caja, "SYS", sys, true).unwrap();
    assert_eq!(dos.asignar_creando(&mut caja, "T", "Caja:T"), Ok(()));
    assert_eq!(dos.asignar_creando(&mut caja, "ENV", "Caja:Env"), Ok(()));
    assert!(dos.fs.existe(caja_v, "T"));
    assert!(dos.fs.existe(caja_v, "Env/ya"));
    assert!(dos.asignar_creando(&mut caja, "X", "SYS:Nuevo").is_err());
    assert!(!dos.fs.existe(sys, "Nuevo"));
    assert!(dos.asignar_creando(&mut caja, "Y:Z", "Caja:Otro").is_err());
    assert!(!dos.fs.existe(caja_v, "Otro"), "un nombre malo no crea el cajón");
}
