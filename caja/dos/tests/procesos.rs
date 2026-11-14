//! `CreateNewProc`: las etiquetas (`caja_dos::procesos`), las ayudas para los ficheros y cerrojos de un proceso nuevo
//! y lo que pasa con los de una tarea que acaba (`traspasar_tarea`). Con los casos hostiles: NULL, etiquetas en
//! círculo, `TAG_SKIP` enorme, punteros malos, pilas absurdas.

mod comun;
use caja_dos::{Caja, Memoria};
use caja_dos::procesos::*;
use comun::*;

/// Una lista de etiquetas en la memoria de la caja.
fn lista(b: &mut Banco, et: &[(u32, u32)]) -> u32 {
    let d = b.hueco(8 * et.len() as u32 + 8);
    for (i, &(t, v)) in et.iter().enumerate() {
        b.caja.mem.escribir32(d + 8 * i as u32, t);
        b.caja.mem.escribir32(d + 8 * i as u32 + 4, v);
    }
    d
}

fn codigo(b: &mut Banco) -> u32 {
    let d = b.hueco(16);
    b.caja.mem.escribir16(d, 0x4E75);
    d
}

#[test]
fn lo_de_serie() {
    let mut b = Banco::nuevo();
    let e = codigo(&mut b);
    let l = lista(&mut b, &[(NP_ENTRY, e), (0, 0)]);
    let p = leer_etiquetas(&b.caja, l).unwrap();
    assert_eq!(p.entrada, e);
    assert_eq!(p.pila, PILA_DE_SERIE);
    assert_eq!(p.nombre, b"New Process");
    assert!(p.cerrar_entrada && p.cerrar_salida && !p.cerrar_error);
    assert_eq!((p.entrada_fh, p.salida_fh, p.error_fh), (None, None, None));
    assert_eq!((p.cajon, p.cajon_programa, p.prioridad), (None, None, None));
    assert!(!p.cli && p.copiar_variables && !p.soltar_seglist);
}

#[test]
fn todas_las_etiquetas() {
    let mut b = Banco::nuevo();
    let seg = b.hueco(32);
    b.caja.mem.escribir16(seg + 4, 0x4E75);
    let nombre = b.cad("Mi proceso");
    let args = b.cad("uno dos\n");
    let orden = b.cad("orden");
    let l = lista(
        &mut b,
        &[
            (NP_SEGLIST, seg >> 2),
            (NP_FREESEGLIST, 1),
            (NP_INPUT, 0x1234),
            (NP_OUTPUT, 0),
            (NP_CLOSEINPUT, 0),
            (NP_ERROR, 0x55),
            (NP_CLOSEERROR, 1),
            (NP_CURRENTDIR, 7),
            (NP_HOMEDIR, 9),
            (NP_STACKSIZE, 20000),
            (NP_NAME, nombre),
            (NP_PRIORITY, -3i32 as u32),
            (NP_CONSOLETASK, 0x100),
            (NP_WINDOWPTR, 0xFFFF_FFFF),
            (NP_COPYVARS, 0),
            (NP_CLI, 1),
            (NP_COMMANDNAME, orden),
            (NP_ARGUMENTS, args),
            (NP_PATH, 4),
            (NP_EXITCODE, 0x2000),
            (NP_EXITDATA, 42),
            (0, 0),
        ],
    );
    let p = leer_etiquetas(&b.caja, l).unwrap();
    assert_eq!(p.seglist, seg >> 2);
    assert_eq!(p.entrada, seg + 4, "el código, 4 bytes detrás del BPTR");
    assert!(p.soltar_seglist);
    assert_eq!(p.entrada_fh, Some(0x1234));
    assert_eq!(p.salida_fh, Some(0));
    assert!(!p.cerrar_entrada && p.cerrar_salida && p.cerrar_error);
    assert_eq!(p.error_fh, Some(0x55));
    assert_eq!((p.cajon, p.cajon_programa), (Some(7), Some(9)));
    assert_eq!(p.pila, 20000);
    assert_eq!(p.nombre, b"Mi proceso");
    assert_eq!(p.prioridad, Some(-3));
    assert_eq!(p.tarea_consola, Some(0x100));
    assert_eq!(p.ventana, Some(0xFFFF_FFFF));
    assert!(!p.copiar_variables && p.cli);
    assert_eq!(p.nombre_orden.as_deref(), Some(&b"orden"[..]));
    assert_eq!(p.argumentos.as_deref(), Some(&b"uno dos\n"[..]));
    assert_eq!((p.al_salir, p.datos_al_salir), (0x2000, 42));
    assert_eq!(p.ignoradas, 1, "NP_Path");
}

#[test]
fn etiquetas_de_control() {
    let mut b = Banco::nuevo();
    let e = codigo(&mut b);
    let segunda = lista(&mut b, &[(NP_PRIORITY, 2), (0, 0)]);
    let l = lista(
        &mut b,
        &[(1, 99), (3, 1), (NP_NAME, 0xDEAD), (NP_ENTRY, e), (2, segunda), (NP_STACKSIZE, 1)],
    );
    let p = leer_etiquetas(&b.caja, l).unwrap();
    // TAG_IGNORE no hace nada, TAG_SKIP 1 se salta el NP_Name malo y TAG_MORE sigue en la otra lista: lo de detrás
    // del TAG_MORE (la pila) no se lee.
    assert_eq!(p.prioridad, Some(2));
    assert_eq!(p.nombre, b"New Process");
    assert_eq!(p.pila, PILA_DE_SERIE);
}

#[test]
fn prioridades_y_pilas_en_sus_limites() {
    let mut b = Banco::nuevo();
    let e = codigo(&mut b);
    for (dato, pri) in [(1000u32, 127i8), ((-1000i32) as u32, -128), (0x7F, 127), (0x80, 127)] {
        let l = lista(&mut b, &[(NP_ENTRY, e), (NP_PRIORITY, dato), (0, 0)]);
        assert_eq!(leer_etiquetas(&b.caja, l).unwrap().prioridad, Some(pri), "{dato:#x}");
    }
    let l = lista(&mut b, &[(NP_ENTRY, e), (NP_STACKSIZE, 0), (0, 0)]);
    assert_eq!(leer_etiquetas(&b.caja, l).unwrap().pila, PILA_MINIMA);
    let l = lista(&mut b, &[(NP_ENTRY, e), (NP_STACKSIZE, 0xFFFF_FFFF), (0, 0)]);
    assert_eq!(leer_etiquetas(&b.caja, l), Err(MalProceso::PilaAbsurda { bytes: 0xFFFF_FFFF }));
}

// --- Hostiles ---

#[test]
fn sin_codigo_o_con_los_dos() {
    let mut b = Banco::nuevo();
    let e = codigo(&mut b);
    assert_eq!(leer_etiquetas(&b.caja, 0), Err(MalProceso::SinCodigo), "CreateNewProc(NULL)");
    let l = lista(&mut b, &[(0, 0)]);
    assert_eq!(leer_etiquetas(&b.caja, l), Err(MalProceso::SinCodigo));
    let l = lista(&mut b, &[(NP_ENTRY, e), (NP_SEGLIST, e >> 2), (0, 0)]);
    assert_eq!(leer_etiquetas(&b.caja, l), Err(MalProceso::SinCodigo));
}

#[test]
fn entradas_malas() {
    let mut b = Banco::nuevo();
    let e = codigo(&mut b);
    for mala in [0u32, 1, e + 1, 0x800, 0xFFFF_FFF0, 0x7000_0000] {
        let l = lista(&mut b, &[(NP_ENTRY, mala), (0, 0)]);
        assert!(matches!(leer_etiquetas(&b.caja, l), Err(MalProceso::EntradaMala { .. })), "{mala:#x}");
    }
    for mala in [0u32, 0x4000_0000, 0xFFFF_FFFF, 0x1000_0000] {
        let l = lista(&mut b, &[(NP_SEGLIST, mala), (0, 0)]);
        assert!(matches!(leer_etiquetas(&b.caja, l), Err(MalProceso::EntradaMala { .. })), "{mala:#x}");
    }
}

#[test]
fn listas_malas_no_cuelgan() {
    let mut b = Banco::nuevo();
    let e = codigo(&mut b);
    // TAG_MORE a sí misma, sin fin.
    let l = b.hueco(16);
    b.caja.mem.escribir32(l, 2);
    b.caja.mem.escribir32(l + 4, l);
    assert!(matches!(leer_etiquetas(&b.caja, l), Err(MalProceso::Lista(_))));
    // Dos listas que se apuntan.
    let a = b.hueco(24);
    let c = b.hueco(24);
    for (x, y) in [(a, c), (c, a)] {
        b.caja.mem.escribir32(x, NP_ENTRY);
        b.caja.mem.escribir32(x + 4, e);
        b.caja.mem.escribir32(x + 8, 2);
        b.caja.mem.escribir32(x + 12, y);
    }
    assert!(matches!(leer_etiquetas(&b.caja, a), Err(MalProceso::Lista(_))));
    // Punteros fuera, impares, en la página 0, y TAG_SKIP que se sale de la memoria.
    for mala in [1u32, 4, 0x800, 0xFFFF_FFF8, 0x7FFF_FFF0, (TAM_MEMORIA as u32) - 4] {
        assert!(leer_etiquetas(&b.caja, mala).is_err(), "{mala:#x}");
    }
    let l = lista(&mut b, &[(3, 0xFFFF_FFFF), (NP_ENTRY, e), (0, 0)]);
    assert!(matches!(leer_etiquetas(&b.caja, l), Err(MalProceso::Lista(_))));
    // Un TAG_MORE a 0 acaba la lista.
    let l = lista(&mut b, &[(NP_ENTRY, e), (2, 0)]);
    assert!(leer_etiquetas(&b.caja, l).is_ok());
    // Nombres y argumentos que no se leen, o sin fin.
    let largo = b.hueco(8192);
    b.caja.mem.trozo_mut(largo, 8192).unwrap().fill(b'x');
    let l = lista(&mut b, &[(NP_ENTRY, e), (NP_NAME, largo), (0, 0)]);
    assert!(leer_etiquetas(&b.caja, l).is_err(), "nombre sin 0");
    let l = lista(&mut b, &[(NP_ENTRY, e), (NP_ARGUMENTS, 0xFFFF_0000), (0, 0)]);
    assert!(leer_etiquetas(&b.caja, l).is_err());
    // Un nombre largo se corta.
    let n = b.cad(&"n".repeat(1000));
    let l = lista(&mut b, &[(NP_ENTRY, e), (NP_NAME, n), (0, 0)]);
    assert_eq!(leer_etiquetas(&b.caja, l).unwrap().nombre.len(), MAX_NOMBRE);
    // Etiquetas al azar: nunca un pánico.
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..20_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let t = match x % 5 {
            0 => (x >> 8) as u32 % 4,
            1 => NP_DUMMY + ((x >> 8) as u32 % 30),
            _ => (x >> 8) as u32,
        };
        let v = match (x >> 40) % 3 {
            0 => e,
            1 => l,
            _ => (x >> 16) as u32,
        };
        let l2 = lista(&mut b, &[(t, v), (NP_ENTRY, e), (t ^ 1, v), (0, 0)]);
        let _ = leer_etiquetas(&b.caja, l2);
    }
}

#[test]
fn ficheros_y_cerrojos_de_un_proceso_nuevo() {
    let mut b = Banco::nuevo();
    let antes = b.dos.ficheros_abiertos();
    let nil = b.dos.abrir_nil(&mut b.caja).unwrap();
    assert!(b.dos.es_fichero(nil));
    // Escribir en NIL: no hace nada y no falla.
    let d = b.cad("hola");
    assert_eq!(b.llamar(caja_dos::lvo::WRITE, &[nil, d, 4]), 4);
    let l = b.lock("SYS:C", -2);
    let otro = b.dos.duplicar_cerrojo(&mut b.caja, l).unwrap();
    assert_ne!(otro, l);
    assert_eq!(b.dos.duplicar_cerrojo(&mut b.caja, 0), Ok(0));
    assert!(b.dos.es_cerrojo(otro) && b.dos.es_cerrojo(0) && !b.dos.es_cerrojo(0x1234));
    // Pasan a otra tarea; al acabar esa, se cierran.
    let hija = 0x5_0000;
    assert!(b.dos.dar_fichero(nil, hija));
    assert!(b.dos.dar_cerrojo(otro, hija));
    assert!(!b.dos.dar_fichero(0x1234, hija));
    assert!(!b.dos.dar_cerrojo(0x4000_0000, hija));
    b.dos.acabar_tarea(&mut b.caja, hija);
    assert!(!b.dos.es_fichero(nil));
    assert!(!b.dos.es_cerrojo(otro));
    // Cerrar y soltar por BPTR, de quien sea; dos veces o basura no hace nada.
    let nil2 = b.dos.abrir_nil(&mut b.caja).unwrap();
    b.dos.cerrar_bptr(&mut b.caja, nil2);
    b.dos.cerrar_bptr(&mut b.caja, nil2);
    b.dos.cerrar_bptr(&mut b.caja, 0xFFFF_FFFF);
    b.dos.soltar_bptr(&mut b.caja, l);
    b.dos.soltar_bptr(&mut b.caja, l);
    b.dos.soltar_bptr(&mut b.caja, 0xFFFF_FFFF);
    assert_eq!(b.dos.ficheros_abiertos(), antes);
    b.limpio(0, antes);
}

#[test]
fn lo_de_una_hija_que_acaba_pasa_a_su_creadora() {
    let mut b = Banco::nuevo();
    let antes = b.dos.ficheros_abiertos();
    let padre = b.proceso;
    // La hija abre un fichero y un cerrojo y acaba sin cerrarlos.
    let hija = b.caja.reservar(caja_dos::estructuras::process::TAMANO);
    b.caja.mem.escribir8(hija + caja_dos::estructuras::process::LN_TYPE, caja_dos::estructuras::process::NT_PROCESS);
    b.caja.tarea = hija;
    let fh = b.open("RAM:dato", caja_dos::estructuras::MODE_NEWFILE);
    let l = b.lock("SYS:C", -2);
    b.caja.tarea = padre;
    b.dos.traspasar_tarea(hija, padre);
    // Siguen abiertos (los puede usar el padre)...
    assert_eq!(b.write(fh, b"abc"), 3);
    assert!(b.dos.es_cerrojo(l));
    // ... y se cierran cuando acaba el padre.
    b.dos.acabar_tarea(&mut b.caja, padre);
    assert!(!b.dos.es_fichero(fh));
    assert!(!b.dos.es_cerrojo(l));
    assert_eq!(b.dos.ficheros_abiertos(), antes - 1, "también la consola del padre");
}
