//! icon.library desde el 68k: cada función con su prueba, sobre ficheros de verdad (la dos.library de la caja sobre
//! ficheros en memoria) y con lo hostil.

mod comun;

use caja_exec::{Fallo, Memoria};
use caja_icono::dibujo::{IDS_NORMAL, IDS_SELECTED};
use caja_icono::diskobj::{dobj, drawerdata, gadget, image};
use caja_icono::errores::*;
use caja_icono::info::{self, Opciones};
use caja_icono::tags::*;
use caja_icono::wb::appmessage;
use caja_icono::*;
use comun::*;

#[test]
fn desplazamientos() {
    // Los de workbench/workbench.h e intuition/intuition.h del NDK 3.2 (sacados con m68k-amigaos-gcc y offsetof).
    let d = [
        dobj::DO_MAGIC,
        dobj::DO_VERSION,
        dobj::DO_GADGET,
        dobj::DO_TYPE,
        dobj::DO_DEFAULTTOOL,
        dobj::DO_TOOLTYPES,
        dobj::DO_CURRENTX,
        dobj::DO_CURRENTY,
        dobj::DO_DRAWERDATA,
        dobj::DO_TOOLWINDOW,
        dobj::DO_STACKSIZE,
        dobj::TAMANO,
    ];
    assert_eq!(d, [0, 2, 4, 48, 50, 54, 58, 62, 66, 70, 74, 78]);
    let g = [
        gadget::NEXTGADGET,
        gadget::LEFTEDGE,
        gadget::TOPEDGE,
        gadget::WIDTH,
        gadget::HEIGHT,
        gadget::FLAGS,
        gadget::ACTIVATION,
        gadget::GADGETTYPE,
        gadget::GADGETRENDER,
        gadget::SELECTRENDER,
        gadget::GADGETTEXT,
        gadget::MUTUALEXCLUDE,
        gadget::SPECIALINFO,
        gadget::GADGETID,
        gadget::USERDATA,
        gadget::TAMANO,
    ];
    assert_eq!(g, [0, 4, 6, 8, 10, 12, 14, 16, 18, 22, 26, 30, 34, 38, 40, 44]);
    let i = [
        image::LEFTEDGE,
        image::TOPEDGE,
        image::WIDTH,
        image::HEIGHT,
        image::DEPTH,
        image::IMAGEDATA,
        image::PLANEPICK,
        image::PLANEONOFF,
        image::NEXTIMAGE,
        image::TAMANO,
    ];
    assert_eq!(i, [0, 2, 4, 6, 8, 10, 14, 15, 16, 20]);
    let dd = [
        drawerdata::DD_NEWWINDOW,
        drawerdata::DD_CURRENTX,
        drawerdata::DD_CURRENTY,
        drawerdata::DD_FLAGS,
        drawerdata::DD_VIEWMODES,
        drawerdata::TAMANO,
        drawerdata::TAMANO_VIEJO,
    ];
    assert_eq!(dd, [0, 48, 52, 56, 60, 62, 56]);
    let am = [
        appmessage::AM_TYPE,
        appmessage::AM_USERDATA,
        appmessage::AM_ID,
        appmessage::AM_NUMARGS,
        appmessage::AM_ARGLIST,
        appmessage::AM_VERSION,
        appmessage::AM_CLASS,
        appmessage::AM_MOUSEX,
        appmessage::AM_SECONDS,
        appmessage::AM_RESERVED,
        appmessage::TAMANO,
        appmessage::WBARG,
    ];
    assert_eq!(am, [20, 22, 26, 30, 34, 38, 40, 42, 46, 54, 86, 8]);
    // Y en la memoria: un DiskObject dado tiene cada campo donde dice el include.
    let mut c = caja();
    let mut ic = clasico(&["A=1"]);
    ic.cajon = Some(info::Cajon::de_serie());
    ic.ventana = Some(b"CON:0/0/100/50".to_vec());
    c.poner("Herr.info", &info::escribir(&ic, Opciones::default()).unwrap());
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_ne!(d, 0);
    assert_eq!(c.l16(d), 0xE310);
    assert_eq!(c.l16(d + 2), 1);
    assert_eq!(c.l16(d + 4 + 8), 20);
    assert_eq!(c.l16(d + 4 + 10), 11);
    assert_eq!(c.l8(d + 48), info::WBTOOL);
    assert_eq!(c.texto(c.l32(d + 50)), b"SYS:C/Herramienta");
    assert_eq!((c.l32(d + 58), c.l32(d + 62)), (10, 20));
    let ddir = c.l32(d + 66);
    assert_eq!(c.l16(ddir + 4), 400);
    assert_eq!(c.texto(c.l32(d + 70)), b"CON:0/0/100/50");
    assert_eq!(c.l32(d + 74), 8192);
    let img = c.l32(d + 4 + 18);
    assert_eq!((c.l16(img + 4), c.l16(img + 6), c.l16(img + 8), c.l8(img + 14)), (20, 10, 2, 3));
    let datos = c.l32(img + 10);
    assert_eq!(datos % 2, 0);
    let p = ic.imagen1.as_ref().unwrap().datos.as_ref().unwrap();
    assert_eq!(c.e.mem.leer(datos, p.len() as u32).unwrap(), &p[..]);
    assert_ne!(c.l32(d + 4 + 22), 0);
}

#[test]
fn get_disk_object_y_tooltypes() {
    let mut c = caja();
    c.poner("Herr.info", &bytes_clasico(&["PUBSCREEN=Workbench", "DONOTWAIT", " ESPACIOS = si ", "MODO=Uno|DOS|tres"]));
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_ne!(d, 0);
    assert_eq!(c.i.iconos_vivos(), 1);
    let tt = c.l32(d + 54);
    assert_eq!(c.tooltypes(d).len(), 4);
    let buscar = |c: &mut Caja, s: &[u8]| {
        let k = c.cadena(s);
        c.f("FindToolType", &[tt, k], &[])
    };
    let v = buscar(&mut c, b"pubscreen");
    assert_eq!(c.texto(v), b"Workbench");
    let v = buscar(&mut c, b"DoNotWait");
    assert_ne!(v, 0);
    assert_eq!(c.texto(v), b"");
    let v = buscar(&mut c, b"espacios");
    assert_eq!(c.texto(v), b"si ");
    assert_eq!(buscar(&mut c, b"NADA"), 0);
    let v = buscar(&mut c, b"MODO");
    for (opcion, si) in [(&b"dos"[..], 1), (b"TRES", 1), (b"cuatro", 0), (b" uno ", 1)] {
        let o = c.cadena(opcion);
        assert_eq!(c.f("MatchToolValue", &[v, o], &[]), si, "{:?}", opcion);
    }
    // Un vector nulo no tiene nada.
    let k = c.cadena(b"X");
    assert_eq!(c.f("FindToolType", &[0, k], &[]), 0);
    // Relativo al cajón actual (la tarea no tiene: el volumen de arranque).
    let n2 = c.cadena(b"Herr");
    let d2 = c.f("GetDiskObject", &[n2], &[]);
    assert_ne!(d2, 0);
    // Soltar: la memoria vuelve; dos veces es un aviso y no rompe nada.
    c.f("FreeDiskObject", &[d], &[]);
    c.f("FreeDiskObject", &[d2], &[]);
    assert_eq!(c.i.iconos_vivos(), 0);
    assert!(c.e.bloque(d).is_none());
    c.avisos();
    c.f("FreeDiskObject", &[d], &[]);
    assert!(matches!(c.avisos()[..], [caja_exec::Aviso { fallo: Fallo::NoReservado { .. }, .. }]));
    c.f("FreeDiskObject", &[0], &[]);
    assert!(c.avisos().is_empty());
}

#[test]
fn sin_fichero_da_error_de_dos() {
    let mut c = caja();
    let n = c.cadena(b"Prueba:NoEsta");
    assert_eq!(c.f("GetDiskObject", &[n], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_OBJECT_NOT_FOUND);
    // Uno que no es un icono.
    c.poner("Texto.info", b"hola, no soy un icono");
    let n = c.cadena(b"Prueba:Texto");
    assert_eq!(c.f("GetDiskObject", &[n], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_OBJECT_WRONG_TYPE);
    // GetDiskObject(NULL): uno vacío.
    let d = c.f("GetDiskObject", &[0], &[]);
    assert_ne!(d, 0);
    assert_eq!(c.l16(d), 0xE310);
    assert_eq!(c.l32(d + 54), 0);
}

/// IconControlA con etiquetas que piden un valor: devuelve los valores en el orden de `tags`.
fn preguntar(c: &mut Caja, d: u32, tags: &[u32]) -> Vec<u32> {
    let sitio = c.memoria(4 * tags.len() as u32);
    let t: Vec<(u32, u32)> = tags.iter().enumerate().map(|(k, &t)| (t, sitio + 4 * k as u32)).collect();
    let l = c.etiquetas(&t);
    assert_eq!(c.f("IconControlA", &[d, l], &[]), tags.len() as u32);
    (0..tags.len()).map(|k| c.l32(sitio + 4 * k as u32)).collect()
}

#[test]
fn newicons_por_icon_control() {
    let mut c = caja();
    let (b, px, pal) = newicon();
    c.poner("Ni.info", &b);
    let n = c.cadena(b"Prueba:Ni");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_ne!(d, 0);
    assert_eq!(c.tooltypes(d), vec![b"PUBSCREEN=Workbench".to_vec()]);
    let v = preguntar(
        &mut c,
        d,
        &[
            ICONCTRLA_ISNEWICON,
            ICONCTRLA_ISPALETTEMAPPED,
            ICONCTRLA_GETWIDTH,
            ICONCTRLA_GETHEIGHT,
            ICONCTRLA_GETIMAGEDATA1,
            ICONCTRLA_GETPALETTE1,
            ICONCTRLA_GETPALETTESIZE1,
            ICONCTRLA_GETTRANSPARENTCOLOR1,
            ICONCTRLA_HASREALIMAGE2,
            ICONCTRLA_GETIMAGEMASK1,
            ICONCTRLA_ISNATIVEICON,
        ],
    );
    assert_eq!(&v[..4], &[1, 1, 16, 12]);
    assert_eq!(c.e.mem.leer(v[4], px.len() as u32).unwrap(), &px[..]);
    let p: Vec<u8> = pal.iter().flatten().copied().collect();
    assert_eq!(c.e.mem.leer(v[5], 15).unwrap(), &p[..]);
    assert_eq!((v[6], v[7], v[8]), (5, 0, 0));
    // La máscara: el color 0 es transparente (los puntos 0, 5, 10... de la primera fila).
    let fila0 = c.l16(v[9]);
    assert_eq!(fila0 & 0x8000, 0);
    assert_ne!(fila0 & 0x4000, 0);
    assert_eq!(v[10], 1);
}

#[test]
fn de_color_y_png_por_icon_control() {
    let mut c = caja();
    let (b, col) = de_color();
    c.poner("Color.info", &b);
    let n = c.cadena(b"Prueba:Color");
    let d = c.f("GetDiskObject", &[n], &[]);
    let v = preguntar(
        &mut c,
        d,
        &[
            ICONCTRLA_ISNEWICON,
            ICONCTRLA_ISPALETTEMAPPED,
            ICONCTRLA_GETFRAMELESS,
            ICONCTRLA_HASREALIMAGE2,
            ICONCTRLA_GETIMAGEDATA2,
            ICONCTRLA_GETPALETTESIZE2,
        ],
    );
    assert_eq!(&v[..4], &[0, 1, 1, 1]);
    let sel = col.img[1].as_ref().unwrap();
    assert_eq!(c.e.mem.leer(v[4], sel.indices.len() as u32).unwrap(), &sel.indices[..]);
    assert_eq!(v[5], 7);
    let asp = c.memoria(4);
    let l = c.etiquetas(&[(ICONCTRLA_GETASPECTRATIO, asp)]);
    c.f("IconControlA", &[d, l], &[]);
    assert_eq!(c.l8(asp), 0x11);
    // Sin pedir la de paleta (ICONGETA_GetPaletteMappedIcon FALSE): solo lo clásico.
    let l = c.etiquetas(&[(ICONGETA_GETPALETTEMAPPEDICON, 0)]);
    let d2 = c.f("GetIconTagList", &[n, l], &[]);
    assert_eq!(preguntar(&mut c, d2, &[ICONCTRLA_ISPALETTEMAPPED]), vec![0]);
    // Un PNG de un programa: su tipo sale del fichero (empieza por HUNK_HEADER).
    c.poner("Png.info", &icono_png());
    c.poner("Png", &[0, 0, 3, 0xF3, 0, 0, 0, 0]);
    let n = c.cadena(b"Prueba:Png");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_ne!(d, 0);
    assert_eq!(c.l8(d + 48), info::WBTOOL);
    assert_eq!(
        preguntar(
            &mut c,
            d,
            &[ICONCTRLA_ISPALETTEMAPPED, ICONCTRLA_GETWIDTH, ICONCTRLA_GETFRAMELESS, ICONCTRLA_HASREALIMAGE2]
        ),
        vec![0, 32, 1, 1]
    );
    // Lleva una imagen clásica para quien pinte GadgetRender.
    let img = c.l32(d + 4 + 18);
    assert_eq!((c.l16(img + 4), c.l16(img + 8)), (32, 2));
    // Sin el fichero, un PNG es un proyecto.
    c.poner("Suelto.info", &icono_png());
    let n = c.cadena(b"Prueba:Suelto");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_eq!(c.l8(d + 48), info::WBPROJECT);
}

#[test]
fn put_disk_object_y_vuelta_a_leer() {
    let mut c = caja();
    c.poner("Herr.info", &bytes_clasico(&["A=1", "B=2"]));
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    // El programa cambia el sitio y pone su propio vector de tooltypes.
    c.e.mem.escribir32(d + 58, 123).unwrap();
    let (t1, t2) = (c.cadena(b"NUEVO=s\xed"), c.cadena(b"OTRO"));
    let v = c.memoria(12);
    c.e.mem.escribir32(v, t1).unwrap();
    c.e.mem.escribir32(v + 4, t2).unwrap();
    c.e.mem.escribir32(d + 54, v).unwrap();
    let destino = c.cadena(b"Prueba:Copia");
    assert_eq!(c.f("PutDiskObject", &[destino, d], &[]), 1);
    let b = c.contenido("Copia.info").unwrap();
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert_eq!(i.x, 123);
    assert_eq!(i.tooltypes.unwrap(), vec![b"NUEVO=s\xed".to_vec(), b"OTRO".to_vec()]);
    assert_eq!(i.imagen1, clasico(&[]).imagen1);
    // Y se vuelve a leer con GetDiskObject.
    let d2 = c.f("GetDiskObject", &[destino], &[]);
    assert_eq!(c.l32(d2 + 58), 123);
    // FreeDiskObject suelta el bloque de icon.library, no el vector del programa.
    c.f("FreeDiskObject", &[d], &[]);
    assert!(c.e.bloque(v).is_some());
    // NewIcons y color se escriben como estaban.
    for (nombre, bytes) in [("Ni", newicon().0), ("Color", de_color().0)] {
        c.poner(&format!("{nombre}.info"), &bytes);
        let n = c.cadena(format!("Prueba:{nombre}").as_bytes());
        let d = c.f("GetDiskObject", &[n], &[]);
        let destino = c.cadena(format!("Prueba:{nombre}2").as_bytes());
        assert_eq!(c.f("PutDiskObject", &[destino, d], &[]), 1);
        let a = info::leer(&bytes, Opciones::default()).unwrap();
        let b = info::leer(&c.contenido(&format!("{nombre}2.info")).unwrap(), Opciones::default()).unwrap();
        assert_eq!(a.color, b.color, "{nombre}");
        assert_eq!(a.tooltypes, b.tooltypes, "{nombre}");
    }
}

#[test]
fn put_hostil_y_opciones() {
    let mut c = caja();
    // Sobre un PNG no se escribe (se perdería lo suyo): FALSE y ERROR_NOT_IMPLEMENTED.
    let png = icono_png();
    c.poner("Png.info", &png);
    let n = c.cadena(b"Prueba:Png");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_eq!(c.f("PutDiskObject", &[n, d], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_NOT_IMPLEMENTED);
    assert_eq!(c.contenido("Png.info").unwrap(), png);
    // A otro nombre, sí: como uno de OS 3.5.
    let otro = c.cadena(b"Prueba:DePng");
    assert_eq!(c.f("PutDiskObject", &[otro, d], &[]), 1);
    let b = c.contenido("DePng.info").unwrap();
    assert_eq!(&b[..2], &[0xE3, 0x10]);
    assert!(b.windows(4).any(|w| w == b"FORM"));
    // Solo el sitio: lo demás del fichero, igual.
    c.poner("Herr.info", &bytes_clasico(&["A=1"]));
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    c.e.mem.escribir32(d + 58, 77).unwrap();
    c.e.mem.escribir32(d + 54, 0).unwrap();
    let antes = c.contenido("Herr.info").unwrap();
    let l = c.etiquetas(&[(ICONPUTA_ONLYUPDATEPOSITION, 1)]);
    assert_eq!(c.f("PutIconTagList", &[n, d, l], &[]), 1);
    let despues = c.contenido("Herr.info").unwrap();
    assert_eq!(despues.len(), antes.len());
    let distintos: Vec<usize> = (0..antes.len()).filter(|&k| antes[k] != despues[k]).collect();
    assert!(distintos.iter().all(|&k| (58..62).contains(&k)), "{distintos:?}");
    // Quitar la imagen de color.
    c.poner("Color.info", &de_color().0);
    let n = c.cadena(b"Prueba:Color");
    let d = c.f("GetDiskObject", &[n], &[]);
    let destino = c.cadena(b"Prueba:SinColor");
    let err = c.memoria(4);
    let l = c.etiquetas(&[(ICONPUTA_DROPCHUNKYICONIMAGE, 1), (ICONA_ERRORCODE, err)]);
    assert_eq!(c.f("PutIconTagList", &[destino, d, l], &[]), 1);
    assert_eq!(c.l32(err), 0);
    assert!(!c.contenido("SinColor.info").unwrap().windows(4).any(|w| w == b"FORM"));
    // Sin icono, o con uno fuera de la memoria.
    assert_eq!(c.f("PutDiskObject", &[destino, 0], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_REQUIRED_ARG_MISSING);
    assert_eq!(c.f("PutDiskObject", &[destino, 0xFFFF_FF00], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_BAD_NUMBER);
    assert!(!c.avisos().is_empty());
    // Un DiskObject del programa con una imagen que dice medir 60000 de ancho.
    let falso = c.memoria(78);
    let img = c.memoria(20);
    c.e.mem.escribir16(img + 4, 60000).unwrap();
    c.e.mem.escribir32(falso + 4 + 18, img).unwrap();
    assert_eq!(c.f("PutDiskObject", &[destino, falso], &[]), 0);
    // Un vector de tooltypes que no acaba nunca (todo punteros a la misma cadena, sin nulo).
    let s = c.cadena(b"X");
    let v = c.memoria(4 * 20_000);
    for k in 0..20_000 {
        c.e.mem.escribir32(v + 4 * k, s).unwrap();
    }
    let falso = c.memoria(78);
    c.e.mem.escribir32(falso + 54, v).unwrap();
    assert_eq!(c.f("PutDiskObject", &[destino, falso], &[]), 0);
    let k = c.cadena(b"Y");
    assert_eq!(c.f("FindToolType", &[v, k], &[]), 0);
}

#[test]
fn iconos_de_serie() {
    let mut c = caja();
    for t in 1..=7u32 {
        let d = c.f("GetDefDiskObject", &[], &[t]);
        assert_ne!(d, 0, "tipo {t}");
        assert_eq!(c.l8(d + 48) as u32, t);
        assert_ne!(c.l32(d + 4 + 18), 0);
        assert_eq!(c.l32(d + 58), 0x8000_0000);
    }
    assert_eq!(c.f("GetDefDiskObject", &[], &[99]), 0);
    assert_eq!(c.ioerr(), ERROR_BAD_NUMBER);
    // Sin .info: GetDiskObjectNew da el de serie del objeto.
    c.poner("Programa", &[0, 0, 3, 0xF3, 1, 2, 3, 4]);
    c.poner("Texto", b"hola");
    c.en.dos.fs.poner_cajon(c.prueba, "Cajon");
    for (nombre, tipo) in [
        (&b"Prueba:Programa"[..], info::WBTOOL),
        (b"Prueba:Texto", info::WBPROJECT),
        (b"Prueba:Cajon", info::WBDRAWER),
        (b"Prueba:", info::WBDISK),
    ] {
        let n = c.cadena(nombre);
        assert_eq!(c.f("GetDiskObject", &[n], &[]), 0);
        let d = c.f("GetDiskObjectNew", &[n], &[]);
        assert_ne!(d, 0, "{:?}", nombre);
        assert_eq!(c.l8(d + 48), tipo, "{:?}", nombre);
    }
    // Ni .info ni objeto: nada.
    let n = c.cadena(b"Prueba:Fantasma");
    assert_eq!(c.f("GetDiskObjectNew", &[n], &[]), 0);
    // GetIconTagList: FailIfUnavailable, IsDefaultIcon e ICONA_ErrorCode.
    let (es, err) = (c.memoria(4), c.memoria(4));
    let n = c.cadena(b"Prueba:Programa");
    let l = c.etiquetas(&[(ICONGETA_FAILIFUNAVAILABLE, 0), (ICONGETA_ISDEFAULTICON, es), (ICONA_ERRORCODE, err)]);
    let d = c.f("GetIconTagList", &[n, l], &[]);
    assert_ne!(d, 0);
    assert_eq!((c.l32(es), c.l32(err)), (1, 0));
    let l = c.etiquetas(&[(ICONGETA_ISDEFAULTICON, es), (ICONA_ERRORCODE, err)]);
    assert_eq!(c.f("GetIconTagList", &[n, l], &[]), 0);
    assert_eq!((c.l32(es), c.l32(err)), (0, ERROR_OBJECT_NOT_FOUND));
    // El de serie de ENV:Sys/def_tool.info manda sobre el propio; PutDefDiskObject lo escribe.
    let mut propio = clasico(&["DEL_USUARIO"]);
    propio.tipo = info::WBTOOL;
    c.en.dos.fs.poner(c.env, "Sys/def_tool.info", &info::escribir(&propio, Opciones::default()).unwrap());
    let l = c.etiquetas(&[(ICONGETA_GETDEFAULTTYPE, info::WBTOOL as u32)]);
    let d = c.f("GetIconTagList", &[0, l], &[]);
    assert_eq!(c.tooltypes(d), vec![b"DEL_USUARIO".to_vec()]);
    let nombre = c.cadena(b"tool");
    let l = c.etiquetas(&[(ICONGETA_GETDEFAULTNAME, nombre)]);
    let d = c.f("GetIconTagList", &[0, l], &[]);
    assert_eq!(c.tooltypes(d), vec![b"DEL_USUARIO".to_vec()]);
    let nombre = c.cadena(b"nada");
    let l = c.etiquetas(&[(ICONGETA_GETDEFAULTNAME, nombre), (ICONGETA_GETDEFAULTTYPE, info::WBGARBAGE as u32)]);
    let d = c.f("GetIconTagList", &[0, l], &[]);
    assert_eq!(c.l8(d + 48), info::WBGARBAGE);
    let p = c.f("GetDefDiskObject", &[], &[info::WBPROJECT as u32]);
    assert_eq!(c.f("PutDefDiskObject", &[p], &[]), 1);
    assert!(c.en.dos.fs.contenido(c.env, "Sys/def_project.info").is_some());
    // NewDiskObject: vacío, del tipo pedido.
    let d = c.f("NewDiskObject", &[], &[info::WBDRAWER as u32]);
    assert_eq!((c.l8(d + 48), c.l32(d + 4 + 18)), (info::WBDRAWER, 0));
    assert_eq!(c.f("NewDiskObject", &[], &[200]), 0);
}

#[test]
fn delete_dup_y_nombres() {
    let mut c = caja();
    c.poner("Herr.info", &bytes_clasico(&["A=1"]));
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    let l = c.etiquetas(&[(ICONDUPA_DUPLICATETOOLTYPES, 0)]);
    let dup = c.f("DupDiskObjectA", &[d, l], &[]);
    assert_ne!(dup, 0);
    assert_ne!(dup, d);
    assert_eq!(c.l32(dup + 54), 0);
    assert_eq!(c.texto(c.l32(dup + 50)), b"SYS:C/Herramienta");
    assert_eq!(c.l16(c.l32(dup + 4 + 18) + 4), 20);
    // Uno montado por el programa no es de icon.library; su copia, sí.
    let estatico = c.memoria(78);
    c.e.mem.escribir8(estatico + 48, info::WBPROJECT).unwrap();
    let sitio = c.memoria(4);
    let l = c.etiquetas(&[(ICONCTRLA_ISNATIVEICON, sitio)]);
    assert_eq!(c.f("IconControlA", &[estatico, l], &[]), 1);
    assert_eq!(c.l32(sitio), 0);
    let copia = c.f("DupDiskObjectA", &[estatico, 0], &[]);
    assert!(c.i.es_icono(copia));
    assert_eq!(c.f("DeleteDiskObject", &[n], &[]), 1);
    assert!(c.contenido("Herr.info").is_none());
    assert_eq!(c.f("DeleteDiskObject", &[n], &[]), 0);
    assert_eq!(c.ioerr(), ERROR_OBJECT_NOT_FOUND);
    // BumpRevision y BumpRevisionLength.
    let (buf, viejo) = (c.memoria(64), c.cadena(b"copy 3 of Programa"));
    assert_eq!(c.f("BumpRevision", &[buf, viejo], &[]), buf);
    assert_eq!(c.texto(buf), b"copy 4 of Programa");
    let largo = c.cadena(b"un nombre de fichero bastante largo");
    c.f("BumpRevision", &[buf, largo], &[]);
    assert_eq!(c.texto(buf).len(), 30);
    assert_eq!(c.f("BumpRevisionLength", &[buf, largo], &[20]), 0);
    assert_eq!(c.f("BumpRevisionLength", &[buf, largo], &[60]), buf);
    assert_eq!(c.texto(buf), b"copy of un nombre de fichero bastante largo");
}

#[test]
fn icon_control_hace_un_icono_de_color() {
    let mut c = caja();
    let d = c.f("NewDiskObject", &[], &[info::WBPROJECT as u32]);
    let pal = c.memoria(9);
    c.e.mem.leer_mut(pal, 9).unwrap().copy_from_slice(&[1, 2, 3, 40, 50, 60, 200, 210, 220]);
    let datos = c.memoria(8);
    c.e.mem.leer_mut(datos, 8).unwrap().copy_from_slice(&[0, 1, 2, 1, 2, 2, 1, 0]);
    let l = c.etiquetas(&[
        (ICONCTRLA_SETWIDTH, 4),
        (ICONCTRLA_SETHEIGHT, 2),
        (ICONCTRLA_SETPALETTESIZE1, 3),
        (ICONCTRLA_SETPALETTE1, pal),
        (ICONCTRLA_SETIMAGEDATA1, datos),
        (ICONCTRLA_SETTRANSPARENTCOLOR1, 0),
        (ICONCTRLA_SETFRAMELESS, 1),
    ]);
    assert_eq!(c.f("IconControlA", &[d, l], &[]), 7);
    assert_eq!(
        preguntar(&mut c, d, &[ICONCTRLA_GETIMAGEDATA1, ICONCTRLA_GETPALETTE1, ICONCTRLA_ISPALETTEMAPPED]),
        vec![datos, pal, 1]
    );
    let n = c.cadena(b"Prueba:Hecho");
    assert_eq!(c.f("PutDiskObject", &[n, d], &[]), 1);
    let i = info::leer(&c.contenido("Hecho.info").unwrap(), Opciones::default()).unwrap();
    let col = i.color.unwrap();
    assert!(col.sin_marco);
    let m = col.img[0].as_ref().unwrap();
    assert_eq!(m.indices, vec![0, 1, 2, 1, 2, 2, 1, 0]);
    assert_eq!(m.paleta, vec![[1, 2, 3], [40, 50, 60], [200, 210, 220]]);
    assert_eq!(m.transparente, Some(0));
    // Las del icono sobre uno que no es de icon.library no se hacen: ERROR_ACTION_NOT_KNOWN.
    let estatico = c.memoria(78);
    let (err, que) = (c.memoria(4), c.memoria(4));
    let l = c.etiquetas(&[(ICONCTRLA_SETFRAMELESS, 1), (ICONA_ERRORCODE, err), (ICONA_ERRORTAGITEM, que)]);
    assert_eq!(c.f("IconControlA", &[estatico, l], &[]), 0);
    assert_eq!(c.l32(err), ERROR_ACTION_NOT_KNOWN);
    assert_eq!(c.l32(que), l);
    // Medidas imposibles y datos fuera de la memoria: avisos, sin contar.
    let l = c.etiquetas(&[
        (ICONCTRLA_SETWIDTH, 100_000),
        (ICONCTRLA_SETIMAGEDATA1, 0xFFFF_FFF0),
        (ICONCTRLA_SETPALETTE1, 8),
    ]);
    assert_eq!(c.f("IconControlA", &[d, l], &[]), 0);
    assert_eq!(c.avisos().len(), 3);
    // Las globales.
    let r = c.memoria(8);
    for (k, v) in [-2i16, -1, 2, 1].iter().enumerate() {
        c.e.mem.escribir16(r + 2 * k as u32, *v as u16).unwrap();
    }
    let l = c.etiquetas(&[
        (ICONCTRLA_SETGLOBALEMBOSSRECT, r),
        (ICONCTRLA_SETGLOBALFRAMELESS, 1),
        (ICONCTRLA_SETGLOBALMAXNAMELENGTH, 5),
    ]);
    assert_eq!(c.f("IconControlA", &[0, l], &[]), 3);
    assert_eq!(
        preguntar(
            &mut c,
            0,
            &[ICONCTRLA_GETGLOBALFRAMELESS, ICONCTRLA_GETGLOBALMAXNAMELENGTH, ICONCTRLA_GETGLOBALNEWICONSSUPPORT]
        ),
        vec![1, 25, 1]
    );
    let r2 = c.memoria(8);
    let l = c.etiquetas(&[(ICONCTRLA_GETGLOBALEMBOSSRECT, r2)]);
    c.f("IconControlA", &[0, l], &[]);
    assert_eq!(c.e.mem.leer(r2, 8).unwrap(), c.e.mem.leer(r, 8).unwrap());
    // LayoutIconA y la pantalla del icono.
    assert_eq!(c.f("LayoutIconA", &[d, 0x4000, 0], &[]), 1);
    assert_eq!(preguntar(&mut c, d, &[ICONCTRLA_GETSCREEN]), vec![0x4000]);
    assert_eq!(c.f("LayoutIconA", &[estatico, 0x4000, 0], &[]), 0);
    // ChangeToSelectedIconColor.
    let cr = c.memoria(4);
    c.e.mem.leer_mut(cr, 3).unwrap().copy_from_slice(&[90, 150, 255]);
    c.f("ChangeToSelectedIconColor", &[cr], &[]);
    assert_eq!(c.e.mem.leer(cr, 3).unwrap(), &[60, 100, 170]);
}

#[test]
fn free_list() {
    let mut c = caja();
    let fl = c.memoria(16);
    let m = c.f("FreeAlloc", &[fl, 100, 0], &[]);
    assert_ne!(m, 0);
    let m2 = c.memoria(64);
    assert_eq!(c.f("AddFreeList", &[fl, m2, 64], &[]), 1);
    let m3 = c.memoria(32);
    assert_eq!(c.f("AddFreeList", &[fl, m3, 32], &[]), 1);
    assert_eq!(c.l16(fl), 3);
    assert_eq!(c.f("FreeFree", &[fl, m3], &[]), 1);
    assert!(c.e.bloque(m3).is_none());
    assert_eq!(c.f("FreeFree", &[fl, m3], &[]), 0);
    c.f("FreeFreeList", &[fl], &[]);
    assert!(c.e.bloque(m).is_none() && c.e.bloque(m2).is_none());
    c.f("FreeFreeList", &[fl], &[]);
    c.f("FreeFreeList", &[0], &[]);
    // Un bloque que no es lo que dice (largo mal): aviso, no se rompe.
    let m4 = c.memoria(64);
    c.f("AddFreeList", &[fl, m4, 8000], &[]);
    c.avisos();
    c.f("FreeFreeList", &[fl], &[]);
    assert!(matches!(c.avisos()[..], [caja_exec::Aviso { fallo: Fallo::TamanoDistinto { .. }, .. }]));
    assert_eq!(c.f("AddFreeList", &[0, m4, 64], &[]), 0);
}

/// Un RastPort de mentira (la letra mide 8 de alto con la base en 6, la pluma A es la 5).
fn rastport(c: &mut Caja) -> u32 {
    let rp = c.memoria(100);
    c.e.mem.escribir8(rp + 25, 5).unwrap();
    c.e.mem.escribir16(rp + 58, 8).unwrap();
    c.e.mem.escribir16(rp + 62, 6).unwrap();
    rp
}

#[test]
fn draw_icon_state_clasico() {
    let mut c = caja();
    c.poner("Herr.info", &bytes_clasico(&["A=1"]));
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    let rp = rastport(&mut c);
    let nombre = c.cadena(b"Herr");
    c.f("DrawIconStateA", &[rp, d, nombre, 0], &[10, 20, IDS_NORMAL]);
    let ll = std::mem::take(&mut c.en.llamadas);
    let dibujo = ll.iter().find(|l| l.lib == "intuition" && l.lvo == -114).expect("DrawImage");
    assert_eq!((dibujo.r.a[0], dibujo.r.a[1], dibujo.r.d[0], dibujo.r.d[1]), (rp, c.l32(d + 4 + 18), 13, 23));
    let texto = ll.iter().find(|l| l.lib == "graphics" && l.lvo == -60).expect("Text");
    assert_eq!((texto.r.a[0], texto.r.d[0]), (nombre, 4));
    // La pluma de antes vuelve.
    let ultima_pluma = ll.iter().rev().find(|l| l.lvo == -342).unwrap();
    assert_eq!(ultima_pluma.r.d[0], 5);
    // Seleccionado con imagen propia (GADGHIMAGE): la segunda.
    c.f("DrawIconStateA", &[rp, d, 0, 0], &[0, 0, IDS_SELECTED]);
    let ll = std::mem::take(&mut c.en.llamadas);
    let dibujo = ll.iter().find(|l| l.lib == "intuition" && l.lvo == -114).unwrap();
    assert_eq!(dibujo.r.a[1], c.l32(d + 4 + 22));
    // Sin imagen propia: la normal y un complemento encima.
    c.e.mem.escribir16(d + 4 + 12, 4).unwrap();
    c.f("DrawIconStateA", &[rp, d, 0, 0], &[0, 0, IDS_SELECTED]);
    let ll = std::mem::take(&mut c.en.llamadas);
    assert!(ll.iter().any(|l| l.lvo == -354 && l.r.d[0] == 2));
    // GetIconRectangleA: 20 × 10 más los bordes de 3, y el nombre (8 puntos por letra) debajo.
    let r = c.memoria(8);
    let largo = c.cadena(b"Un nombre largo");
    assert_eq!(c.f("GetIconRectangleA", &[rp, d, 0, r, 0], &[]), 1);
    let rect = |c: &Caja| (0..4).map(|k| c.l16(r + 2 * k) as i16).collect::<Vec<_>>();
    assert_eq!(rect(&c), vec![0, 0, 25, 15]);
    assert_eq!(c.f("GetIconRectangleA", &[rp, d, largo, r, 0], &[]), 1);
    assert_eq!(rect(&c), vec![(26 - 120) / 2, 0, (26 - 120) / 2 + 119, 15 + 1 + 8]);
    let l = c.etiquetas(&[(ICONDRAWA_BORDERLESS, 1)]);
    assert_eq!(c.f("GetIconRectangleA", &[rp, d, 0, r, l], &[]), 1);
    assert_eq!(rect(&c), vec![0, 0, 19, 9]);
    assert_eq!(c.f("GetIconRectangleA", &[rp, d, 0, 0, 0], &[]), 0);
}

#[test]
fn draw_icon_state_de_color() {
    let mut c = caja();
    let (b, col) = de_color();
    c.poner("Color.info", &b);
    let n = c.cadena(b"Prueba:Color");
    let d = c.f("GetDiskObject", &[n], &[]);
    let rp = rastport(&mut c);
    c.en.fondo = 0xFF00_00FF;
    // Sin marco (el icono lo pide): la imagen en 3,3 (los bordes se guardan igual).
    c.f("DrawIconStateA", &[rp, d, 0, 0], &[0, 0, IDS_NORMAL]);
    let pal = &col.img[0].as_ref().unwrap().paleta;
    let k = 24 * 5 + 10;
    let i = col.img[0].as_ref().unwrap().indices[k] as usize;
    let esperado = 0xFF00_0000 | (pal[i][0] as u32) << 16 | (pal[i][1] as u32) << 8 | pal[i][2] as u32;
    assert_eq!(c.en.lienzo[&(3 + 10, 3 + 5)], esperado);
    // Lo transparente (las tres primeras columnas) deja el fondo.
    assert_eq!(c.en.lienzo[&(3, 3)], 0xFF00_00FF);
    // Seleccionado: la segunda imagen.
    c.f("DrawIconStateA", &[rp, d, 0, 0], &[0, 0, IDS_SELECTED]);
    let i = col.img[1].as_ref().unwrap().indices[k] as usize;
    let esperado = 0xFF00_0000 | (pal[i][0] as u32) << 16 | (pal[i][1] as u32) << 8 | pal[i][2] as u32;
    assert_eq!(c.en.lienzo[&(13, 8)], esperado);
    // Sin cybergraphics: la imagen clásica (DrawImage) y un aviso.
    c.en.sin_cgx = true;
    c.en.llamadas.clear();
    c.avisos();
    c.f("DrawIconStateA", &[rp, d, 0, 0], &[0, 0, IDS_NORMAL]);
    assert!(c.en.llamadas.iter().any(|l| l.lib == "intuition" && l.lvo == -114));
    assert_eq!(c.avisos().len(), 1);
    // Un PNG con alfa a medias: mezclado con el fondo.
    c.en.sin_cgx = false;
    c.poner("Png.info", &icono_png());
    let n = c.cadena(b"Prueba:Png");
    let d = c.f("GetDiskObject", &[n], &[]);
    c.en.lienzo.clear();
    c.f("DrawIconStateA", &[rp, d, 0, 0], &[0, 0, IDS_SELECTED]);
    // La de seleccionado: x < 4 es rojo con alfa 0x80 sobre azul.
    let p = c.en.lienzo[&(3, 3)];
    assert_eq!(p & 0xFF00_0000, 0xFF00_0000);
    assert!(((p >> 16) & 0xFF) > 0x70 && (p & 0xFF) > 0x70, "{p:08x}");
}

#[test]
fn hostil_punteros_y_pagina_cero() {
    let mut c = caja();
    c.poner("Herr.info", &bytes_clasico(&["A=1"]));
    let n = c.cadena(b"Prueba:Herr");
    let d = c.f("GetDiskObject", &[n], &[]);
    let malos = [0u32, 4, 0x10, 0xFFF, 0xFFFF_FF00, 0xFFFF_FFFF, TAMANO - 2, 0x8000_0000, d + 1];
    for f in FUNCIONES_ICON.iter().chain(FUNCIONES_WB.iter()) {
        for &m in &malos {
            for &m2 in &[0u32, m] {
                // Ninguna combinación tumba nada.
                c.f(f, &[m, m2, m, m2, m], &[m, m2, 1]);
            }
        }
    }
    // El icono de antes sigue bien y se puede soltar.
    assert!(c.i.es_icono(d));
    c.f("FreeDiskObject", &[d], &[]);
    // Leer de la página 0 es un aviso de página 0.
    c.avisos();
    c.f("GetDiskObject", &[0x10], &[]);
    assert!(matches!(c.avisos()[..], [caja_exec::Aviso { fallo: Fallo::PaginaCero { .. }, .. }]));
    // Una lista de etiquetas en círculo (TAG_MORE a sí misma).
    let l = c.memoria(16);
    c.e.mem.escribir32(l, 2).unwrap();
    c.e.mem.escribir32(l + 4, l).unwrap();
    assert_eq!(c.f("GetIconTagList", &[n, l], &[]), 0);
    assert!(matches!(c.avisos()[..], [caja_exec::Aviso { fallo: Fallo::ListaRota { .. }, .. }]));
}

#[test]
fn hostil_imagenes_enormes_y_tarea_muerta() {
    let mut c = caja();
    // 1024 × 1024 de 8 planos (1 MB de planos): se da.
    let mut i = info::Info::vacio(info::WBPROJECT);
    i.imagen1 = Some(planar(1024, 1024, 8, |x, y| (x ^ y) as u8));
    c.poner("Grande.info", &info::escribir(&i, Opciones::default()).unwrap());
    let n = c.cadena(b"Prueba:Grande");
    let d = c.f("GetDiskObject", &[n], &[]);
    assert_ne!(d, 0);
    c.f("FreeDiskObject", &[d], &[]);
    // Uno que no cabe en la memoria de la caja: 0 y ERROR_NO_FREE_STORE, sin romper.
    let mut muchos = Vec::new();
    loop {
        let d = c.f("GetDiskObject", &[n], &[]);
        if d == 0 {
            break;
        }
        muchos.push(d);
        assert!(muchos.len() < 100);
    }
    assert_eq!(c.ioerr(), ERROR_NO_FREE_STORE);
    for d in muchos {
        c.f("FreeDiskObject", &[d], &[]);
    }
    // 10.000 tooltypes: se dan, y FindToolType encuentra el último.
    let tt: Vec<String> = (0..10_000).map(|k| format!("CLAVE{k}=valor{k}")).collect();
    let refs: Vec<&str> = tt.iter().map(|s| s.as_str()).collect();
    c.poner("Muchos.info", &bytes_clasico(&refs));
    let n = c.cadena(b"Prueba:Muchos");
    let d = c.f("GetDiskObject", &[n], &[]);
    let k = c.cadena(b"clave9999");
    let v = c.f("FindToolType", &[c.l32(d + 54), k], &[]);
    assert_eq!(c.texto(v), b"valor9999");
    // Si la tarea muere, sus iconos se olvidan (exec suelta su memoria).
    c.i.tarea_muerta(c.tarea);
    assert_eq!(c.i.iconos_vivos(), 0);
}
