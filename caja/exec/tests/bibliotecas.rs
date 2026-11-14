//! Bibliotecas: abrir y cerrar, la tabla de saltos nativa, MakeLibrary, InitStruct y SetFunction.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Accion, Continuacion, Destino, Fallo, Memoria, Registros, lvo};
use comun::*;

fn opencnt(e: &caja_exec::Exec<caja_exec::MemoriaVec>, lib: u32) -> u16 {
    e.mem.leer16(lib + library::LIB_OPENCNT).unwrap()
}

#[test]
fn open_y_close_de_exec() {
    let (mut e, _t) = caja_con_tarea();
    let b = e.base();
    let mut r = Registros::default();
    r.a[1] = cadena(&mut e, "exec.library");
    r.d[0] = 37;
    assert_eq!(llamar(&mut e, "OpenLibrary", &mut r), Accion::Seguir);
    assert_eq!(r.d[0], b);
    assert_eq!(opencnt(&e, b), 1);
    r.d[0] = 41;
    llamar(&mut e, "OpenLibrary", &mut r);
    assert_eq!(r.d[0], 0, "pide una versión que no hay");
    llamar(&mut e, "OldOpenLibrary", &mut r);
    assert_eq!(r.d[0], b);
    assert_eq!(opencnt(&e, b), 2);
    r.a[1] = b;
    llamar(&mut e, "CloseLibrary", &mut r);
    llamar(&mut e, "CloseLibrary", &mut r);
    assert_eq!(opencnt(&e, b), 0);
    r.a[1] = cadena(&mut e, "nada.library");
    r.d[0] = 0;
    llamar(&mut e, "OpenLibrary", &mut r);
    assert_eq!(r.d[0], 0);
    r.a[1] = 0;
    llamar(&mut e, "CloseLibrary", &mut r);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

#[test]
fn biblioteca_nativa_ajena() {
    let (mut e, _t) = caja_con_tarea();
    let lib = e.crear_biblioteca_nativa("dos.library", 40, 3, &[300, 301, 302], 200).unwrap();
    assert_eq!(e.mem.leer16(lib + library::LIB_NEGSIZE).unwrap(), 44, "7 entradas, a múltiplo de 4");
    assert_eq!(e.mem.leer16(lib + library::LIB_POSSIZE).unwrap(), 200);
    // Open, Close... son las genéricas de exec; desde -30, las suyas.
    assert_eq!(e.entrada(lib - 6).unwrap(), caja_exec::Entrada::Nativa(1));
    assert_eq!(e.entrada(lib - 30).unwrap(), caja_exec::Entrada::Nativa(300));
    assert_eq!(e.entrada(lib - 42).unwrap(), caja_exec::Entrada::Nativa(302));
    assert_eq!(e.ajena(301), Some((lib, -36)));
    let mut r = Registros::default();
    assert_eq!(e.llamar(300, &mut r), Accion::Ajena);
    r.a[1] = cadena(&mut e, "dos.library");
    r.d[0] = 36;
    llamar(&mut e, "OpenLibrary", &mut r);
    assert_eq!(r.d[0], lib);
    assert_eq!(opencnt(&e, lib), 1);
    // RemLibrary: las nativas se quedan, con la marca de echarlas.
    r.a[1] = lib;
    llamar(&mut e, "RemLibrary", &mut r);
    assert_eq!(r.d[0], 0);
    assert_ne!(e.mem.leer8(lib + library::LIB_FLAGS).unwrap() & library::LIBF_DELEXP, 0);
    // Números repetidos o de exec, no.
    assert!(e.crear_biblioteca_nativa("x.library", 1, 0, &[300], 34).is_err());
    assert!(e.crear_biblioteca_nativa("y.library", 1, 0, &[33], 34).is_err());
}

#[test]
fn open_library_con_camino() {
    // Las clases de ReAction se abren como "gadgets/layout.gadget": vale el final del camino.
    let (mut e, _t) = caja_con_tarea();
    let lib = e.crear_biblioteca_nativa("layout.gadget", 47, 1, &[300], 34).unwrap();
    let mut r = Registros::default();
    for nombre in ["gadgets/layout.gadget", "SYS:Classes/Gadgets/layout.gadget", "layout.gadget"] {
        r.a[1] = cadena(&mut e, nombre);
        r.d[0] = 44;
        llamar(&mut e, "OpenLibrary", &mut r);
        assert_eq!(r.d[0], lib, "{nombre}");
    }
    for nombre in ["gadgets/", "gadgets/otra.gadget", "layout.gadget/", ":"] {
        r.a[1] = cadena(&mut e, nombre);
        r.d[0] = 0;
        llamar(&mut e, "OpenLibrary", &mut r);
        assert_eq!(r.d[0], 0, "{nombre}");
    }
    // La versión cuenta igual.
    r.a[1] = cadena(&mut e, "gadgets/layout.gadget");
    r.d[0] = 48;
    llamar(&mut e, "OpenLibrary", &mut r);
    assert_eq!(r.d[0], 0);
}

#[test]
fn set_function_sobre_una_funcion_nativa() {
    let (mut e, _t) = caja_con_tarea();
    let b = e.base();
    e.tomar_codigo_cambiado();
    let tr = e.trampolin(33).unwrap();
    let mut r = Registros::default();
    r.a[1] = b;
    r.a[0] = (-198i32) as u32;
    r.d[0] = 0x0012_0000;
    llamar(&mut e, "SetFunction", &mut r);
    // Devuelve el trampolín, que sigue llamando a la AllocMem nativa.
    assert_eq!(r.d[0], tr);
    assert_eq!(e.entrada(tr).unwrap(), caja_exec::Entrada::Nativa(33));
    assert_eq!(e.entrada(b - 198).unwrap(), caja_exec::Entrada::Salto(0x0012_0000));
    assert_eq!(e.tomar_codigo_cambiado(), vec![(b - 198, b - 192)]);
    // Un segundo parche devuelve el primero.
    r.a[1] = b;
    r.a[0] = (-198i32) as u32;
    r.d[0] = 0x0013_0000;
    llamar(&mut e, "SetFunction", &mut r);
    assert_eq!(r.d[0], 0x0012_0000);
    // La suma está al día: SumLibrary no avisa.
    e.sum_library(b).unwrap();
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

#[test]
fn open_parcheado_pasa_por_el_parche() {
    let (mut e, t) = caja_con_tarea();
    let b = e.base();
    e.set_function(b, -6, 0x0012_0000).unwrap();
    let mut r = Registros::default();
    r.a[1] = cadena(&mut e, "exec.library");
    r.d[0] = 33;
    let a = llamar(&mut e, "OpenLibrary", &mut r);
    let Accion::Llamar { destino, regs, tras } = a else { panic!("{a:?}") };
    assert_eq!(destino, Destino::Codigo68k(0x0012_0000));
    assert_eq!(regs.a[6], b);
    assert_eq!(regs.d[0], 33);
    assert_eq!(tras, Continuacion::AbrirBiblioteca { lib: b });
    // El parche (68k) devuelve la base en D0; la caja sigue.
    let mut hecho = regs;
    hecho.d[0] = b;
    assert_eq!(e.continuar(tras, &hecho, &mut r), Accion::Seguir);
    assert_eq!(r.d[0], b);
    // La apertura cuenta para la tarea: matarla la deshace (aquí el parche no subió OpenCnt; lo hace la nativa).
    let _ = t;
}

#[test]
fn make_library_con_vectores_absolutos_e_init_struct() {
    let (mut e, _t) = caja_con_tarea();
    let z = e.alloc_mem(256, memf::CLEAR).unwrap();
    // Vectores: Open, Close, Expunge, Reserved y una propia.
    let vectores = z;
    for (i, f) in [0x0010_0100u32, 0x0010_0200, 0x0010_0300, 0x0010_0400, 0x0010_0500, 0xFFFF_FFFF].iter().enumerate() {
        e.mem.escribir32(vectores + 4 * i as u32, *f).unwrap();
    }
    let nombre = cadena(&mut e, "prueba.library");
    // InitStruct: INITBYTE(LN_TYPE, NT_LIBRARY), INITLONG(LN_NAME, nombre), INITWORD(LIB_VERSION, 7).
    let tabla = z + 64;
    let bytes: Vec<u8> = [
        vec![0xA0, node::LN_TYPE as u8, nt::LIBRARY, 0],
        vec![0x80, node::LN_NAME as u8],
        nombre.to_be_bytes().to_vec(),
        vec![0x90, library::LIB_VERSION as u8, 0, 7],
        vec![0],
    ]
    .concat();
    e.mem.leer_mut(tabla, bytes.len() as u32).unwrap().copy_from_slice(&bytes);
    let mut r = Registros::default();
    r.a[0] = vectores;
    r.a[1] = tabla;
    r.a[2] = 0;
    r.d[0] = 50;
    r.d[1] = 0;
    assert_eq!(llamar(&mut e, "MakeLibrary", &mut r), Accion::Seguir);
    let lib = r.d[0];
    assert_ne!(lib, 0);
    assert_eq!(e.mem.leer16(lib + library::LIB_NEGSIZE).unwrap(), 32);
    assert_eq!(e.mem.leer16(lib + library::LIB_POSSIZE).unwrap(), 50);
    assert_eq!(e.mem.leer16(lib + library::LIB_VERSION).unwrap(), 7);
    assert_eq!(e.mem.leer8(lib + node::LN_TYPE).unwrap(), nt::LIBRARY);
    assert_eq!(e.entrada(lib - 6).unwrap(), caja_exec::Entrada::Salto(0x0010_0100));
    assert_eq!(e.entrada(lib - 30).unwrap(), caja_exec::Entrada::Salto(0x0010_0500));
    // Añadida, su Open es código 68k: exec se lo pide a la caja.
    r.a[1] = lib;
    llamar(&mut e, "AddLibrary", &mut r);
    r.a[1] = nombre;
    r.d[0] = 0;
    let a = llamar(&mut e, "OpenLibrary", &mut r);
    assert!(matches!(a, Accion::Llamar { destino: Destino::Codigo68k(0x0010_0100), .. }), "{a:?}");
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

#[test]
fn make_library_con_vectores_relativos_e_init() {
    let (mut e, _t) = caja_con_tarea();
    let z = e.alloc_mem(64, memf::CLEAR).unwrap();
    // -1, luego desplazamientos desde la tabla, y -1.
    for (i, w) in [0xFFFFu16, 0x0100, 0x0200, 0xFFFF].iter().enumerate() {
        e.mem.escribir16(z + 2 * i as u32, *w).unwrap();
    }
    let mut r = Registros::default();
    r.a[0] = z;
    r.a[1] = 0;
    r.a[2] = 0x0015_0000;
    r.d[0] = 34;
    r.d[1] = 0x1234;
    let a = llamar(&mut e, "MakeLibrary", &mut r);
    let Accion::Llamar { destino, regs, tras } = a else { panic!("{a:?}") };
    assert_eq!(destino, Destino::Codigo68k(0x0015_0000));
    assert_eq!(regs.a[0], 0x1234, "A0 = segList");
    assert_eq!(regs.a[6], e.base(), "A6 = SysBase");
    let lib = regs.d[0];
    assert_eq!(e.entrada(lib - 6).unwrap(), caja_exec::Entrada::Salto(z + 0x100));
    assert_eq!(e.entrada(lib - 12).unwrap(), caja_exec::Entrada::Salto(z + 0x200));
    assert_eq!(e.mem.leer16(lib + library::LIB_NEGSIZE).unwrap(), 12);
    // La función de inicio devuelve la base.
    assert_eq!(e.continuar(tras, &regs, &mut r), Accion::Seguir);
    assert_eq!(r.d[0], lib);
}

#[test]
fn init_struct_repetir_y_desplazamiento_largo() {
    let (mut e, _t) = caja_con_tarea();
    let mem = e.alloc_mem(0x200, 0).unwrap();
    e.mem.leer_mut(mem, 0x200).unwrap().fill(0xEE);
    let tabla = e.alloc_mem(64, memf::CLEAR).unwrap();
    let bytes: Vec<u8> = vec![
        // INITWORD(4, $ABCD): 10 01 0000 = $90, desplazamiento 4.
        0x90, 4, 0xAB, 0xCD,
        // Repetir el WORD $ABCD 2 veces donde quedó (6): 01 01 0001 = $51; el dato va alineado a 2.
        0x51, 0, 0xAB, 0xCD, // Repetir el BYTE $55 4 veces: 01 10 0011 = $63, sigue en 10.
        0x63, 0x55, // Desplazamiento de 24 bits (0x000150), un LONG: 11 00 0000 = $C0.
        0xC0, 0x00, 0x01, 0x50, 0x11, 0x22, 0x33, 0x44, // Dos BYTE seguidos: 00 10 0001 = $21.
        0x21, 0x77, 0x88, 0, 0,
    ];
    e.mem.leer_mut(tabla, bytes.len() as u32).unwrap().copy_from_slice(&bytes);
    e.init_struct(tabla, mem, 0x180).unwrap();
    assert_eq!(
        e.mem.leer(mem, 16).unwrap(),
        &[0, 0, 0, 0, 0xAB, 0xCD, 0xAB, 0xCD, 0xAB, 0xCD, 0x55, 0x55, 0x55, 0x55, 0, 0]
    );
    assert_eq!(e.mem.leer32(mem + 0x150).unwrap(), 0x1122_3344);
    assert_eq!(e.mem.leer(mem + 0x154, 2).unwrap(), &[0x77, 0x88]);
    assert_eq!(e.mem.leer8(mem + 0x17F).unwrap(), 0, "borra el tamaño pedido");
    assert_eq!(e.mem.leer8(mem + 0x180).unwrap(), 0xEE, "y nada más");
}

#[test]
fn matar_una_tarea_cierra_sus_bibliotecas() {
    let (mut e, _t) = caja_con_tarea();
    let b = e.base();
    let otra = e.crear_tarea("otra", 5, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(otra));
    let mut r = Registros::default();
    r.a[1] = cadena(&mut e, "exec.library");
    llamar(&mut e, "OldOpenLibrary", &mut r);
    llamar(&mut e, "OldOpenLibrary", &mut r);
    assert_eq!(opencnt(&e, b), 2);
    e.matar_tarea(otra).unwrap();
    assert_eq!(opencnt(&e, b), 0);
}

// --- Hostiles ---

#[test]
fn close_library_hostil() {
    let (mut e, _t) = caja_con_tarea();
    let b = e.base();
    let mut r = Registros::default();
    r.a[1] = b;
    llamar(&mut e, "CloseLibrary", &mut r);
    assert_eq!(e.avisos().last().unwrap().fallo, Fallo::CierreDeMas { dir: b });
    r.a[1] = 0x30_0000;
    llamar(&mut e, "CloseLibrary", &mut r);
    assert_eq!(e.avisos().last().unwrap().fallo, Fallo::BibliotecaDesconocida { dir: 0x30_0000 });
    r.a[1] = 0xFFFF_FFFF;
    llamar(&mut e, "CloseLibrary", &mut r);
    assert_eq!(opencnt(&e, b), 0);
    assert_eq!(avisos_de(&e, "CloseLibrary"), 3);
}

#[test]
fn open_library_hostil() {
    let (mut e, _t) = caja_con_tarea();
    let mut r = Registros::default();
    r.a[1] = 0;
    llamar(&mut e, "OpenLibrary", &mut r);
    assert_eq!(r.d[0], 0);
    r.a[1] = 0x8;
    llamar(&mut e, "OpenLibrary", &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(avisos_de(&e, "OpenLibrary"), 2);
}

#[test]
fn set_function_hostil() {
    let (mut e, _t) = caja_con_tarea();
    let b = e.base();
    assert_eq!(e.set_function(0, -30, 0x1000), Err(Fallo::PunteroNulo));
    assert_eq!(e.set_function(b, 0, 0x1000), Err(Fallo::LvoMalo { lvo: 0 }));
    assert_eq!(e.set_function(b, 30, 0x1000), Err(Fallo::LvoMalo { lvo: 30 }));
    assert_eq!(e.set_function(b, -31, 0x1000), Err(Fallo::LvoMalo { lvo: -31 }));
    assert_eq!(e.set_function(b, -828, 0x1000), Err(Fallo::LvoMalo { lvo: -828 }));
    assert!(e.set_function(0xFFFF_FFF0, -6, 0).is_err());
    // La tabla sigue intacta.
    for n in 1..=lvo::ENTRADAS {
        assert_eq!(e.entrada(b - 6 * n as u32).unwrap(), caja_exec::Entrada::Nativa(n));
    }
}

#[test]
fn suma_de_una_tabla_cambiada_a_mano() {
    let (mut e, _t) = caja_con_tarea();
    let b = e.base();
    // Un programa escribe en la tabla sin SetFunction ni LIBF_CHANGED.
    e.mem.escribir16(b - 198, 0x4E71).unwrap();
    e.sum_library(b).unwrap();
    assert_eq!(avisos_de(&e, "SumLibrary"), 1);
}

#[test]
fn tablas_malas() {
    let (mut e, _t) = caja_con_tarea();
    let z = e.alloc_mem(64, memf::CLEAR).unwrap();
    // ss = 11 no existe.
    e.mem.escribir8(z, 0x30).unwrap();
    assert!(matches!(e.init_struct(z, z + 32, 0), Err(Fallo::TablaMala { .. })));
    // Vectores sin -1 que acaben fuera de la memoria.
    let fin = TAMANO - 8;
    e.mem.escribir32(fin, 0x1000).unwrap();
    e.mem.escribir32(fin + 4, 0x1000).unwrap();
    let mut r = Registros::default();
    r.a[0] = fin;
    llamar(&mut e, "MakeLibrary", &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(avisos_de(&e, "MakeLibrary"), 1);
    assert!(e.make_functions(0x2000, 0xFFFF_FFFC, 0).is_err());
    assert!(e.init_struct(0xFFFF_FFFF, z, 0).is_err());
}

/// Una biblioteca de disco que lleva su propia cuenta de aperturas (locale y diskfont de 3.2 suman en un campo suyo,
/// no en `lib_OpenCnt`): CloseLibrary llama a su Close sin mirar `lib_OpenCnt`; cerrar más de lo que la tarea abrió es
/// `CierreDeMas` y su Close no se llama; lo que abre y cierra el código de una biblioteca de disco no se apunta a la
/// tarea (matarla no lo toca).
#[test]
fn biblioteca_con_cuenta_propia() {
    const OPEN: u32 = 0x0010_0100;
    const CLOSE: u32 = 0x0010_0200;
    const CUENTA: u32 = 34;
    let (mut e, _t) = caja_con_tarea();
    let z = e.alloc_mem(64, memf::CLEAR).unwrap();
    for (i, f) in [OPEN, CLOSE, 0x0010_0300, 0x0010_0400, 0xFFFF_FFFF].iter().enumerate() {
        e.mem.escribir32(z + 4 * i as u32, *f).unwrap();
    }
    let nombre = cadena(&mut e, "propia.library");
    let mut r = Registros::default();
    r.a[0] = z;
    r.d[0] = 40;
    llamar(&mut e, "MakeLibrary", &mut r);
    let lib = r.d[0];
    e.mem.escribir32(lib + node::LN_NAME, nombre).unwrap();
    r.a[1] = lib;
    llamar(&mut e, "AddLibrary", &mut r);
    // "Ejecuta" su Open y su Close como harían ellas: su contador en +34, `lib_OpenCnt` sin tocar.
    let mut cerrados = 0;
    let correr = |e: &mut caja_exec::Exec<caja_exec::MemoriaVec>, a: Accion, r: &mut Registros, cerrados: &mut u32| {
        let Accion::Llamar { destino: Destino::Codigo68k(pc), regs, tras } = a else { return a };
        let mut h = regs;
        let n = e.mem.leer16(lib + CUENTA).unwrap();
        if pc == OPEN {
            e.mem.escribir16(lib + CUENTA, n + 1).unwrap();
            h.d[0] = lib;
        } else {
            assert_eq!(pc, CLOSE);
            e.mem.escribir16(lib + CUENTA, n.wrapping_sub(1)).unwrap();
            *cerrados += 1;
            h.d[0] = 0;
        }
        e.continuar(tras, &h, r)
    };
    let abrir = |e: &mut caja_exec::Exec<caja_exec::MemoriaVec>, r: &mut Registros| {
        r.a[1] = nombre;
        r.d[0] = 0;
        llamar(e, "OpenLibrary", r)
    };
    let a = abrir(&mut e, &mut r);
    correr(&mut e, a, &mut r, &mut cerrados);
    assert_eq!(r.d[0], lib);
    assert_eq!((e.mem.leer16(lib + CUENTA).unwrap(), opencnt(&e, lib)), (1, 0));
    r.a[1] = lib;
    let a = llamar(&mut e, "CloseLibrary", &mut r);
    correr(&mut e, a, &mut r, &mut cerrados);
    assert_eq!((cerrados, e.mem.leer16(lib + CUENTA).unwrap()), (1, 0));
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
    // Cerrar de más: CierreDeMas, sin llamar a su Close.
    r.a[1] = lib;
    let a = llamar(&mut e, "CloseLibrary", &mut r);
    assert_eq!(a, Accion::Seguir);
    assert_eq!(e.avisos().last().unwrap().fallo, Fallo::CierreDeMas { dir: lib });
    // Desde el código de una biblioteca de disco: no se apunta a la tarea, y su cierre no se mira.
    e.memoria_del_sistema(true);
    let a = abrir(&mut e, &mut r);
    correr(&mut e, a, &mut r, &mut cerrados);
    e.memoria_del_sistema(false);
    let otra = e.crear_tarea("otra", 5, 1024, 0x1000).unwrap();
    // Matar una tarea que no la abrió no cambia su cuenta.
    e.matar_tarea(otra).unwrap();
    assert_eq!(e.mem.leer16(lib + CUENTA).unwrap(), 1);
    // La tarea no puede cerrar lo que abrió la biblioteca; la biblioteca, sí.
    r.a[1] = lib;
    let n_avisos = e.avisos().len();
    llamar(&mut e, "CloseLibrary", &mut r);
    assert_eq!(e.avisos().len(), n_avisos + 1);
    e.memoria_del_sistema(true);
    r.a[1] = lib;
    let a = llamar(&mut e, "CloseLibrary", &mut r);
    correr(&mut e, a, &mut r, &mut cerrados);
    e.memoria_del_sistema(false);
    assert_eq!((cerrados, e.mem.leer16(lib + CUENTA).unwrap()), (2, 0));
}
