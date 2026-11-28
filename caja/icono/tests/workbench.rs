//! workbench.library sin Workbench con iconos: los objetos App se dan y se llevan en su tabla, sus mensajes se pueden
//! entregar y vuelven; lo que necesita un Workbench de verdad dice que no lo encuentra.

mod comun;

use caja_exec::Fallo;
use caja_exec::estructuras::{memf, msgport, nt};
use caja_icono::errores::*;
use caja_icono::info::{self, Opciones};
use caja_icono::wb::{Argumento, TipoApp, appmessage, tags::*};
use comun::*;

fn puerto(c: &mut Caja) -> u32 {
    c.e.create_msg_port().unwrap()
}

#[test]
fn ventanas_iconos_y_menus_app() {
    let mut c = caja();
    let p = puerto(&mut c);
    let ventana = c.memoria(200);
    let aw = c.f("AddAppWindowA", &[ventana, p, 0], &[7, 99]);
    assert_ne!(aw, 0);
    let d = c.f("GetDefDiskObject", &[], &[info::WBTOOL as u32]);
    let texto = c.cadena(b"Mi icono");
    let ai = c.f("AddAppIconA", &[texto, p, 0, d, 0], &[8, 98]);
    assert_ne!(ai, 0);
    let (clave, item) = (c.memoria(4), c.cadena(b"Hacer algo"));
    let tecla = c.cadena(b"H");
    let l = c.etiquetas(&[(WBAPPMENUA_GETKEY, clave), (WBAPPMENUA_COMMANDKEYSTRING, tecla)]);
    let am = c.f("AddAppMenuItemA", &[item, p, l], &[9, 97]);
    assert_ne!(am, 0);
    assert_ne!(c.l32(clave), 0);
    let objetos = c.i.objetos_app().to_vec();
    assert_eq!(objetos.len(), 3);
    assert_eq!(
        (objetos[0].tipo, objetos[0].id, objetos[0].datos, objetos[0].ventana),
        (TipoApp::Ventana, 7, 99, ventana)
    );
    assert_eq!((objetos[1].texto.as_slice(), objetos[1].icono), (&b"Mi icono"[..], d));
    assert_eq!(objetos[2].tecla, b'H');
    // Una zona necesita sitio y tamaño.
    let l = c.etiquetas(&[(WBDZA_LEFT, 0), (WBDZA_TOP, 0)]);
    assert_eq!(c.f("AddAppWindowDropZoneA", &[aw, l], &[1, 2]), 0);
    assert_eq!(c.ioerr(), ERROR_REQUIRED_ARG_MISSING);
    let l = c.etiquetas(&[(WBDZA_LEFT, 10), (WBDZA_TOP, 10), (WBDZA_WIDTH, 50), (WBDZA_RELHEIGHT, (-20i32) as u32)]);
    let z = c.f("AddAppWindowDropZoneA", &[aw, l], &[1, 2]);
    assert_ne!(z, 0);
    let zona = c.i.objetos_app().iter().find(|o| o.dir == z).unwrap().clone();
    assert_eq!((zona.caja, zona.relativa), ([10, 10, 50, -20], [false, false, false, true]));
    // Quitar: la zona de otra ventana no; con la ventana se van sus zonas.
    assert_eq!(c.f("RemoveAppWindowDropZone", &[ai, z], &[]), 0);
    assert_eq!(c.f("RemoveAppWindow", &[aw], &[]), 1);
    assert!(c.e.bloque(z).is_none() && c.e.bloque(aw).is_none());
    assert_eq!(c.f("RemoveAppIcon", &[ai], &[]), 1);
    assert_eq!(c.f("RemoveAppMenuItem", &[am], &[]), 1);
    assert!(c.i.objetos_app().is_empty());
    // Dos veces, o con el tipo cambiado: FALSE y un aviso.
    c.avisos();
    assert_eq!(c.f("RemoveAppIcon", &[ai], &[]), 0);
    assert_eq!(c.f("RemoveAppWindow", &[0], &[]), 0);
    assert!(matches!(c.avisos()[..], [caja_exec::Aviso { fallo: Fallo::NoReservado { .. }, .. }]));
    // Sin Workbench (poner_abierto(false)): NULL, como dice el autodoc.
    c.i.poner_abierto(false);
    assert_eq!(c.f("AddAppWindowA", &[ventana, p, 0], &[7, 99]), 0);
    assert_eq!(c.f("AddAppIconA", &[texto, p, 0, d, 0], &[8, 98]), 0);
    c.i.poner_abierto(true);
    // Sin ventana o sin puerto: NULL.
    assert_eq!(c.f("AddAppWindowA", &[0, p, 0], &[]), 0);
    assert_eq!(c.f("AddAppIconA", &[texto, p, 0, 0, 0], &[]), 0);
    // Como mucho 62 elementos de menú.
    let mut n = 0;
    while c.f("AddAppMenuItemA", &[item, p, 0], &[]) != 0 {
        n += 1;
        assert!(n < 100);
    }
    assert_eq!(n, 62);
    // Si la tarea muere, se olvidan.
    c.i.tarea_muerta(c.tarea);
    assert!(c.i.objetos_app().is_empty());
}

#[test]
fn entregar_y_recoger_app_message() {
    let mut c = caja();
    let p = puerto(&mut c);
    let ventana = c.memoria(200);
    let aw = c.f("AddAppWindowA", &[ventana, p, 0], &[7, 99]);
    let args = [Argumento { cerrojo: 0x1234, nombre: b"Uno" }, Argumento { cerrojo: 0, nombre: b"Dos.txt" }];
    c.i.entregar(&mut c.e, aw, &args, 0, 15, 25, (100, 200)).unwrap();
    let m = c.e.get_msg(p).unwrap();
    assert_ne!(m, 0);
    assert_eq!(c.l16(m + appmessage::AM_TYPE), appmessage::AMTYPE_APPWINDOW);
    assert_eq!((c.l32(m + appmessage::AM_ID), c.l32(m + appmessage::AM_USERDATA)), (7, 99));
    assert_eq!(c.l32(m + appmessage::AM_NUMARGS), 2);
    assert_eq!((c.l16(m + appmessage::AM_MOUSEX), c.l16(m + appmessage::AM_MOUSEY)), (15, 25));
    assert_eq!((c.l32(m + appmessage::AM_SECONDS), c.l16(m + appmessage::AM_VERSION)), (100, 1));
    let lista = c.l32(m + appmessage::AM_ARGLIST);
    assert_eq!(c.l32(lista), 0x1234);
    assert_eq!(c.texto(c.l32(lista + 12)), b"Dos.txt");
    // Mientras el programa no lo conteste, sigue fuera.
    assert_eq!(c.i.recoger(&mut c.e), 0);
    let wb = c.l32(m + 14);
    assert_eq!(c.l8(wb + msgport::MP_FLAGS), msgport::PA_IGNORE);
    c.e.reply_msg(m).unwrap();
    assert_eq!(c.i.recoger(&mut c.e), 1);
    assert!(c.e.bloque(m).is_none());
    // A un objeto que no existe, no.
    assert!(c.i.entregar(&mut c.e, 0x5000, &[], 0, 0, 0, (0, 0)).is_err());
    let _ = (memf::PUBLIC, nt::MESSAGE);
}

#[test]
fn abrir_objetos_y_control() {
    let mut c = caja();
    // Un programa con su icono (la pila sale de él).
    c.poner("Prog", &[0, 0, 3, 0xF3, 0, 0, 0, 0]);
    c.poner("Prog.info", &bytes_clasico(&[]));
    let n = c.cadena(b"Prueba:Prog");
    let arg = c.cadena(b"Fichero");
    let l = c.etiquetas(&[(WBOPENA_ARGLOCK, 0x77), (WBOPENA_ARGNAME, arg)]);
    assert_eq!(c.f("OpenWorkbenchObjectA", &[n, l], &[]), 1);
    let (args, pila) = c.en.lanzados.pop().unwrap();
    assert_eq!(args, vec![(0, b"Prueba:Prog".to_vec()), (0x77, b"Fichero".to_vec())]);
    assert_eq!(pila, 8192);
    // Un proyecto: su herramienta, y él detrás.
    let mut pr = info::Info::vacio(info::WBPROJECT);
    pr.herramienta = Some(b"Prueba:Prog".to_vec());
    pr.imagen1 = Some(planar(4, 4, 1, |_, _| 1));
    c.poner("Doc", b"texto");
    c.poner("Doc.info", &info::escribir(&pr, Opciones::default()).unwrap());
    let n = c.cadena(b"Prueba:Doc");
    assert_eq!(c.f("OpenWorkbenchObjectA", &[n, 0], &[]), 1);
    let (args, pila) = c.en.lanzados.pop().unwrap();
    assert_eq!(args, vec![(0, b"Prueba:Prog".to_vec()), (0, b"Prueba:Doc".to_vec())]);
    assert_eq!(pila, 4096);
    // Un proyecto sin icono, un cajón y lo que no existe: FALSE con su IoErr.
    c.poner("Suelto", b"x");
    let n = c.cadena(b"Prueba:Suelto");
    assert_eq!(c.f("OpenWorkbenchObjectA", &[n, 0], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_OBJECT_WRONG_TYPE);
    c.en.dos.fs.poner_cajon(c.prueba, "Cajon");
    let n = c.cadena(b"Prueba:Cajon");
    assert_eq!(c.f("OpenWorkbenchObjectA", &[n, 0], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_NOT_IMPLEMENTED);
    let n = c.cadena(b"Prueba:Nada");
    assert_eq!(c.f("OpenWorkbenchObjectA", &[n, 0], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_OBJECT_NOT_FOUND);
    // Si la caja no puede lanzar, su error.
    c.en.lanzar_falla = Some(ERROR_NO_FREE_STORE);
    let n = c.cadena(b"Prueba:Prog");
    assert_eq!(c.f("OpenWorkbenchObjectA", &[n, 0], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_NO_FREE_STORE);
    // Lo que necesita un Workbench con ventanas.
    let n = c.cadena(b"Prueba:Cajon");
    for f in ["CloseWorkbenchObjectA", "MakeWorkbenchObjectVisibleA", "ChangeWorkbenchSelectionA"] {
        assert_eq!(c.f(f, &[n, 0, 0], &[]), 0, "{f}");
        assert_eq!(c.ioerr(), ERROR_OBJECT_NOT_FOUND);
    }
    assert_eq!(c.f("ChangeWorkbenchSelectionA", &[0, 0x1234, 0], &[]), 1);
    assert_eq!(c.f("WBInfo", &[0, n, 0], &[]), 0);
    assert_eq!(c.f("WhichWorkbenchObjectA", &[0, 0], &[10, 10]), 0);
    // WorkbenchControlA: lo de leer, con valores sensatos.
    let (abierto, pila, reinicio, lista, ocultos, camino) =
        (c.memoria(4), c.memoria(4), c.memoria(4), c.memoria(4), c.memoria(4), c.memoria(4));
    let df0 = c.cadena(b"DF0:");
    let l = c.etiquetas(&[
        (WBCTRLA_ISOPEN, abierto),
        (WBCTRLA_SETDEFAULTSTACKSIZE, 20000),
        (WBCTRLA_GETDEFAULTSTACKSIZE, pila),
        (WBCTRLA_GETTYPERESTARTTIME, reinicio),
        (WBCTRLA_GETPROGRAMLIST, lista),
        (WBCTRLA_ADDHIDDENDEVICENAME, df0),
        (WBCTRLA_GETHIDDENDEVICELIST, ocultos),
        (WBCTRLA_DUPLICATESEARCHPATH, camino),
    ]);
    assert_eq!(c.f("WorkbenchControlA", &[n, l], &[]), 1);
    assert_eq!((c.l32(abierto), c.l32(pila), c.l32(reinicio), c.l32(camino)), (0, 20000, 2, 0));
    let lp = c.l32(lista);
    assert_eq!(c.l32(lp), lp + 4);
    let lo = c.l32(ocultos);
    let primero = c.l32(lo);
    assert_eq!(c.texto(c.l32(primero + 10)), b"DF0:");
    let l = c.etiquetas(&[(WBCTRLA_FREEPROGRAMLIST, lp), (WBCTRLA_FREEHIDDENDEVICELIST, lo)]);
    assert_eq!(c.f("WorkbenchControlA", &[0, l], &[]), 1);
    assert!(c.e.bloque(lp).is_none() && c.e.bloque(primero).is_none());
    // Soltar una lista que no se dio: aviso.
    c.avisos();
    let l = c.etiquetas(&[(WBCTRLA_FREEPROGRAMLIST, lp)]);
    assert_eq!(c.f("WorkbenchControlA", &[0, l], &[]), 0);
    assert_eq!(c.avisos().len(), 1);
    // Las privadas: aviso y 0.
    assert_eq!(c.f("wbPrivate1", &[], &[]), 0);
    assert_eq!(c.avisos().len(), 1);
}
