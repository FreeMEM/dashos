//! BOOPSI: clases de sistema, clases de usuario con su despachador "en 68k" (aquí, funciones de Rust que hacen lo que
//! haría el código del programa: llamar a `DoSuperMethodA`, que entra otra vez en intuition), interconexión
//! (`ICA_TARGET`, `ICA_MAP`, modelclass, `ICTARGET_IDCMP`), gadgets en una ventana con clics de verdad y lo malo:
//! clases que no existen, métodos desconocidos, despachadores que no vuelven o que se llaman sin fin, círculos de
//! interconexión, punteros malos.

mod comun;

use caja_exec::{Memoria, Registros};
use caja_intuition::boopsi::cte::*;
use caja_intuition::compositor::BTN_LEFT;
use caja_intuition::estructuras::{idcmp, tag, window as win};
use caja_intuition::{Corte68k, Evento, Hecho, Puente};
use comun::puente::*;
use comun::*;

#[test]
fn clases_de_sistema() {
    let mut p = prueba();
    for n in [
        "rootclass", "icclass", "modelclass", "imageclass", "frameiclass", "sysiclass", "fillrectclass",
        "itexticlass", "gadgetclass", "propgclass", "strgclass", "buttongclass", "frbuttonclass", "groupgclass",
    ] {
        let cl = p.c.1.clase_publica(n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!(rl32(&p, cl + iclass::FLAGS) & iclass::CLF_INLIST, 1, "{n} en la lista pública");
        let o = nuevo(&mut p, 0, n, &[]);
        assert_ne!(o, 0, "{n}");
        // OCLASS(o) es la clase, y el contador sube.
        assert_eq!(rl32(&p, o - 4), cl);
        assert_eq!(rl32(&p, cl + iclass::OBJECTCOUNT), 1);
        dispose(&mut p, o);
        assert_eq!(rl32(&p, cl + iclass::OBJECTCOUNT), 0);
    }
    // gadgetclass: el objeto es una struct Gadget (GTYP_CUSTOMGADGET) con sus atributos.
    let g = nuevo(&mut p, 0, "gadgetclass", &[(ga::LEFT, 10), (ga::TOP, 20), (ga::WIDTH, 30), (ga::HEIGHT, 40), (ga::ID, 7)]);
    assert_eq!(l16(&p.c, g + 16) & gtyp::GTYPEMASK, gtyp::CUSTOMGADGET);
    assert_eq!((l16(&p.c, g + 4), l16(&p.c, g + 6), l16(&p.c, g + 8), l16(&p.c, g + 10)), (10, 20, 30, 40));
    assert_eq!(get(&mut p, g, ga::ID), 7);
    set(&mut p, g, &[(ga::LEFT, 3), (ga::DISABLED, 1)]);
    assert_eq!(get(&mut p, g, ga::LEFT), 3);
    assert_eq!(get(&mut p, g, ga::DISABLED), 1);
    dispose(&mut p, g);
    // imageclass: una struct Image con Depth = CUSTOMIMAGEDEPTH.
    let i = nuevo(&mut p, 0, "frameiclass", &[(ia::WIDTH, 50), (ia::HEIGHT, 12)]);
    assert_eq!(l16(&p.c, i + image::DEPTH), image::CUSTOMIMAGEDEPTH);
    assert_eq!(get(&mut p, i, ia::WIDTH), 50);
    dispose(&mut p, i);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

// --- Una clase de usuario con su despachador en 68k ---

const ENTRADA_USUARIO: u32 = 0x00F0_0000;
const MI_ATRIBUTO: u32 = 0x8100_0001;
const MI_METODO: u32 = 0x0012_3456;

/// El despachador de "contador": una subclase de gadgetclass con un LONG de datos (INST_DATA) y un atributo.
fn despachador_contador(p: &mut Prueba, r: Registros) -> Result<Registros, Corte68k> {
    let (cl, obj, msg) = (r.a[0], r.a[2], r.a[1]);
    let id = rl32(p, msg);
    let off = l16(&p.c, cl + iclass::INSTOFFSET) as u32;
    let mut out = r;
    out.d[0] = match id {
        om::NEW => {
            let o = do_super(p, cl, obj, msg)?;
            if o != 0 {
                e32(&mut p.c, o + off, 100);
            }
            o
        }
        om::SET => {
            do_super(p, cl, obj, msg)?;
            let lista = rl32(p, msg + 4);
            let mut d = lista;
            while rl32(p, d) != tag::TAG_DONE {
                if rl32(p, d) == MI_ATRIBUTO {
                    let v = rl32(p, d + 4);
                    e32(&mut p.c, obj + off, v);
                }
                d += 8;
            }
            1
        }
        om::GET if rl32(p, msg + 4) == MI_ATRIBUTO => {
            let st = rl32(p, msg + 8);
            let v = rl32(p, obj + off);
            e32(&mut p.c, st, v);
            1
        }
        MI_METODO => {
            let v = rl32(p, obj + off) + 1;
            e32(&mut p.c, obj + off, v);
            v
        }
        _ => do_super(p, cl, obj, msg)?,
    };
    Ok(out)
}

fn hacer_clase(p: &mut Prueba, sup: &str, codigo: Codigo, entrada: u32) -> u32 {
    let s = cadena(&mut p.c, sup);
    let cl = ib(p, "MakeClass", &[4, 0], &[0, s, 0]);
    assert_ne!(cl, 0);
    e32(&mut p.c, cl + iclass::H_ENTRY, entrada);
    p.codigo.insert(entrada, codigo);
    cl
}

#[test]
fn clase_de_usuario_68k() {
    let mut p = prueba();
    let cl = hacer_clase(&mut p, "gadgetclass", despachador_contador, ENTRADA_USUARIO);
    let gc = p.c.1.clase_publica("gadgetclass").unwrap();
    // Sus datos van detrás de los de gadgetclass (la struct ExtGadget).
    assert_eq!(l16(&p.c, cl + iclass::INSTOFFSET) as u32, TAMANO_GADGET);
    let subclases = rl32(&p, gc + iclass::SUBCLASSCOUNT);
    assert!(subclases > 1, "las de sistema y la nuestra");
    let o = nuevo(&mut p, cl, "", &[(ga::ID, 9), (MI_ATRIBUTO, 41)]);
    assert_ne!(o, 0);
    assert_eq!(rl32(&p, o - 4), cl, "su clase de verdad");
    assert_eq!(get(&mut p, o, ga::ID), 9, "lo de gadgetclass, por DoSuperMethod");
    assert_eq!(get(&mut p, o, MI_ATRIBUTO), 100, "OM_NEW pone 100 tras la superclase");
    set(&mut p, o, &[(MI_ATRIBUTO, 41)]);
    assert_eq!(get(&mut p, o, MI_ATRIBUTO), 41);
    // Un método suyo, por DoMethod.
    let m = mensaje(&mut p, &[MI_METODO]);
    assert_eq!(do_method(&mut p, o, m), Ok(42));
    // FreeClass no puede con objetos vivos.
    assert_eq!(ib(&mut p, "FreeClass", &[], &[cl]), 0);
    // AddClass la hace pública: NewObject por su nombre (cl_ID).
    let id = cadena(&mut p.c, "contador.gadget");
    e32(&mut p.c, cl + iclass::ID, id);
    ib(&mut p, "AddClass", &[], &[cl]);
    let o2 = nuevo(&mut p, 0, "contador.gadget", &[]);
    assert_eq!(rl32(&p, o2 - 4), cl);
    ib(&mut p, "RemoveClass", &[], &[cl]);
    assert_eq!(nuevo(&mut p, 0, "contador.gadget", &[]), 0, "ya no es pública");
    dispose(&mut p, o);
    dispose(&mut p, o2);
    assert_eq!(ib(&mut p, "FreeClass", &[], &[cl]), 1);
    assert_eq!(rl32(&p, gc + iclass::SUBCLASSCOUNT), subclases - 1);
    assert!(p.llamadas_68k > 5);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

// --- Interconexión ---

#[test]
fn interconexion_prop_a_cadena() {
    let mut p = prueba();
    // propgclass → strgclass: PGA_Top pasa a STRINGA_LongVal (ICA_MAP), como el ejemplo del RKRM.
    let cad = nuevo(&mut p, 0, "strgclass", &[(stringa::MAXCHARS, 12), (stringa::LONGVAL, 0), (ga::ID, 2)]);
    let mapa = etiquetas(&mut p, &[(pga::TOP, stringa::LONGVAL)]);
    let prop = nuevo(&mut p, 0, "propgclass", &[(pga::TOTAL, 100), (pga::VISIBLE, 10), (ga::ID, 1), (ica::TARGET, cad), (ica::MAP, mapa)]);
    // Un OM_UPDATE al deslizador (lo que haría otro objeto) cambia su PGA_Top y lo notifica a la cadena.
    let t = etiquetas(&mut p, &[(pga::TOP, 37)]);
    let m = mensaje(&mut p, &[om::UPDATE, t, 0, 0]);
    do_method(&mut p, prop, m).unwrap();
    assert_eq!(get(&mut p, prop, pga::TOP), 37);
    assert_eq!(get(&mut p, cad, stringa::LONGVAL), 37);
    let buf = get(&mut p, cad, stringa::TEXTVAL);
    assert_eq!(c_str(&p, buf), "37");
    // modelclass en medio: deslizador → modelo → (miembro) cadena y (objetivo) otro deslizador.
    let modelo = nuevo(&mut p, 0, "modelclass", &[]);
    let prop2 = nuevo(&mut p, 0, "propgclass", &[(pga::TOTAL, 100), (pga::VISIBLE, 10)]);
    let ic = nuevo(&mut p, 0, "icclass", &[(ica::TARGET, cad), (ica::MAP, mapa)]);
    let mm = mensaje(&mut p, &[om::ADDMEMBER, ic]);
    do_method(&mut p, modelo, mm).unwrap();
    set(&mut p, modelo, &[(ica::TARGET, prop2)]);
    let t = etiquetas(&mut p, &[(pga::TOP, 55)]);
    let m = mensaje(&mut p, &[om::UPDATE, t, 0, 0]);
    do_method(&mut p, modelo, m).unwrap();
    assert_eq!(get(&mut p, prop2, pga::TOP), 55, "el objetivo del modelo");
    assert_eq!(get(&mut p, cad, stringa::LONGVAL), 55, "el miembro (icclass con su mapa)");
    // Disponer el modelo dispone sus miembros.
    dispose(&mut p, modelo);
    for o in [prop, prop2, cad] {
        dispose(&mut p, o);
    }
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

#[test]
fn interconexion_en_circulo() {
    let mut p = prueba();
    // A → B y B → A: un OM_UPDATE a A no da vueltas sin fin.
    let a = nuevo(&mut p, 0, "propgclass", &[(pga::TOTAL, 100), (pga::VISIBLE, 1)]);
    let b = nuevo(&mut p, 0, "propgclass", &[(pga::TOTAL, 100), (pga::VISIBLE, 1), (ica::TARGET, a)]);
    set(&mut p, a, &[(ica::TARGET, b)]);
    let t = etiquetas(&mut p, &[(pga::TOP, 12)]);
    let m = mensaje(&mut p, &[om::UPDATE, t, 0, 0]);
    do_method(&mut p, a, m).unwrap();
    assert_eq!(get(&mut p, a, pga::TOP), 12);
    assert_eq!(get(&mut p, b, pga::TOP), 12);
    // Uno que se tiene a sí mismo de objetivo, e icclass en círculo con tres.
    let c = nuevo(&mut p, 0, "icclass", &[]);
    set(&mut p, c, &[(ica::TARGET, c)]);
    let d = nuevo(&mut p, 0, "icclass", &[(ica::TARGET, c)]);
    let e = nuevo(&mut p, 0, "icclass", &[(ica::TARGET, d)]);
    set(&mut p, c, &[(ica::TARGET, e)]);
    let t = etiquetas(&mut p, &[(0x8100_0099, 1)]);
    let m = mensaje(&mut p, &[om::UPDATE, t, 0, 0]);
    do_method(&mut p, c, m).unwrap();
    for o in [a, b, c, d, e] {
        dispose(&mut p, o);
    }
    assert_eq!(p.c.1.objetos_boopsi(), 0);
}

// --- Lo malo ---

#[test]
fn hostiles() {
    let mut p = prueba();
    // Clases que no existen: por nombre, 0; por puntero, 0 con aviso.
    assert_eq!(nuevo(&mut p, 0, "noexiste.class", &[]), 0);
    let basura = reservar(&mut p.c, 64);
    assert_eq!(nuevo(&mut p, basura, "", &[]), 0);
    assert_eq!(nuevo(&mut p, 0xFFFF_FFF0, "", &[]), 0);
    assert_eq!(nuevo(&mut p, 0, "", &[]), 0);
    // DisposeObject, SetAttrs, GetAttr de algo que no es un objeto.
    ib(&mut p, "DisposeObject", &[], &[basura]);
    ib(&mut p, "DisposeObject", &[], &[0x10]);
    ib(&mut p, "SetAttrsA", &[], &[basura + 12, 0]);
    let st = reservar(&mut p.c, 4);
    assert_eq!(ib(&mut p, "GetAttr", &[ga::ID], &[basura, st]), 0);
    // Un objeto de verdad con lo malo: etiquetas que no acaban, un almacén que no se puede escribir, un método que no
    // existe, ICA_TARGET a la basura.
    let g = nuevo(&mut p, 0, "propgclass", &[(pga::TOTAL, 10)]);
    let circulo = reservar(&mut p.c, 16);
    e32(&mut p.c, circulo, tag::TAG_MORE);
    e32(&mut p.c, circulo + 4, circulo);
    ib(&mut p, "SetAttrsA", &[], &[g, circulo]);
    ib(&mut p, "SetAttrsA", &[], &[g, 0xFFFF_FFF0]);
    assert_eq!(ib(&mut p, "GetAttr", &[pga::TOP], &[g, 0x8]), 0);
    assert_eq!(ib(&mut p, "GetAttr", &[0x9999_9999], &[g, st]), 0);
    let m = mensaje(&mut p, &[0x7777_7777, 1, 2, 3]);
    assert_eq!(do_method(&mut p, g, m), Ok(0));
    set(&mut p, g, &[(ica::TARGET, basura + 12)]);
    let t = etiquetas(&mut p, &[(pga::TOP, 3)]);
    let m = mensaje(&mut p, &[om::UPDATE, t, 0, 0]);
    do_method(&mut p, g, m).unwrap();
    // Un mensaje que no se puede leer.
    let stub = rl32(&p, p.c.1.clase_publica("rootclass").unwrap() + iclass::H_ENTRY);
    let _ = stub;
    let cl = rl32(&p, g - 4);
    let mut r = Registros::default();
    r.a[0] = cl;
    r.a[1] = 0xFFFF_FFF0;
    r.a[2] = g;
    assert_eq!(caja_intuition::boopsi::llamar(&mut p, N_DESPACHO, &mut r), Hecho::Seguir);
    assert_eq!(r.d[0], 0);
    // El despachador nativo con una clase que no es una clase.
    r.a[0] = basura;
    r.a[1] = m;
    assert_eq!(caja_intuition::boopsi::llamar(&mut p, N_DESPACHO, &mut r), Hecho::Seguir);
    assert_eq!(r.d[0], 0);
    // FreeClass de una del sistema, o de la basura: no.
    let rc = p.c.1.clase_publica("rootclass").unwrap();
    assert_eq!(ib(&mut p, "FreeClass", &[], &[rc]), 0);
    assert_eq!(ib(&mut p, "FreeClass", &[], &[basura]), 0);
    // MakeClass con una superclase que no existe, o un tamaño absurdo.
    let s = cadena(&mut p.c, "noexiste.class");
    assert_eq!(ib(&mut p, "MakeClass", &[4, 0], &[0, s, 0]), 0);
    assert_eq!(ib(&mut p, "MakeClass", &[4, 0], &[0, 0, basura]), 0);
    assert_eq!(ib(&mut p, "MakeClass", &[0xFFFF_0000, 0], &[0, 0, rc]), 0);
    // NextObject de una lista vacía y de un puntero malo.
    assert_eq!(ib(&mut p, "NextObject", &[], &[st]), 0);
    dispose(&mut p, g);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert!(avisos(&p) > 5, "se apuntan: {}", avisos(&p));
}

/// Un despachador que no vuelve (el programa acaba dentro): el corte llega hasta arriba sin tocar nada más.
fn despachador_que_no_vuelve(_p: &mut Prueba, _r: Registros) -> Result<Registros, Corte68k> {
    Err(Corte68k::Acabado)
}

/// Uno que, para cualquier método, se lo manda otra vez a sí mismo por intuition (`SetAttrs` sobre sí mismo).
fn despachador_sin_fin(p: &mut Prueba, r: Registros) -> Result<Registros, Corte68k> {
    let (cl, obj, msg) = (r.a[0], r.a[2], r.a[1]);
    let id = rl32(p, msg);
    if id == om::NEW || id == om::DISPOSE {
        let v = do_super(p, cl, obj, msg)?;
        return Ok(Registros { d: [v, 0, 0, 0, 0, 0, 0, 0], a: r.a });
    }
    let t = etiquetas(p, &[(MI_ATRIBUTO, 1)]);
    let n = p.c.1.numero_intuition("SetAttrsA").unwrap();
    let mut rr = regs(&[], &[obj, t]);
    match caja_intuition::boopsi::llamar(p, n, &mut rr) {
        Hecho::Acabar => Err(Corte68k::Acabado),
        _ => Ok(r),
    }
}

/// Uno que se llama a sí mismo con DoSuperMethod a su propia clase (un error de programa: DoSuper(cl) en vez de la
/// superclase), en 68k puro: lo corta el tope de niveles del 68k.
fn despachador_recursivo_68k(p: &mut Prueba, r: Registros) -> Result<Registros, Corte68k> {
    let (cl, obj, msg) = (r.a[0], r.a[2], r.a[1]);
    let v = do_method_cl(p, cl, obj, msg)?;
    Ok(Registros { d: [v, 0, 0, 0, 0, 0, 0, 0], a: r.a })
}

#[test]
fn despachadores_hostiles() {
    let mut p = prueba();
    // El que no vuelve: NewObject da Acabar (la caja corta el programa) y no queda nada a medias.
    let cl = hacer_clase(&mut p, "rootclass", despachador_que_no_vuelve, 0x00F1_0000);
    let lista = etiquetas(&mut p, &[]);
    let (o, h) = intui_b(&mut p, "NewObjectA", &[], &[cl, 0, lista]);
    assert_eq!((o, h), (0, Hecho::Acabar));
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    // Un h_Entry que apunta a donde no hay código: igual.
    let cl2 = hacer_clase(&mut p, "rootclass", despachador_que_no_vuelve, 0x00F2_0000);
    e32(&mut p.c, cl2 + iclass::H_ENTRY, 0x00F3_0000);
    let (_, h) = intui_b(&mut p, "NewObjectA", &[], &[cl2, 0, lista]);
    assert_eq!(h, Hecho::Acabar);
    // Sin h_Entry (MakeClass sin poner el despachador), o impar: 0 con aviso.
    e32(&mut p.c, cl2 + iclass::H_ENTRY, 0);
    assert_eq!(intui_b(&mut p, "NewObjectA", &[], &[cl2, 0, lista]), (0, Hecho::Seguir));
    e32(&mut p.c, cl2 + iclass::H_ENTRY, 0x00F3_0001);
    assert_eq!(intui_b(&mut p, "NewObjectA", &[], &[cl2, 0, lista]), (0, Hecho::Seguir));
    // El que se manda métodos a sí mismo por intuition sin fin: el tope de niveles lo corta y vuelve.
    let cl3 = hacer_clase(&mut p, "rootclass", despachador_sin_fin, 0x00F4_0000);
    let o3 = nuevo(&mut p, cl3, "", &[]);
    assert_ne!(o3, 0);
    let antes = avisos(&p);
    set(&mut p, o3, &[(MI_ATRIBUTO, 5)]);
    assert!(avisos(&p) > antes, "el tope se apunta");
    dispose(&mut p, o3);
    // El que se llama a sí mismo en 68k: el tope de 16 niveles del 68k (NoSePudo: sigue con 0).
    let cl4 = hacer_clase(&mut p, "rootclass", despachador_recursivo_68k, 0x00F5_0000);
    assert_eq!(intui_b(&mut p, "NewObjectA", &[], &[cl4, 0, lista]), (0, Hecho::Seguir));
    assert_eq!(p.niveles, 0);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
}

#[test]
fn gadgets_en_una_ventana() {
    let mut p = prueba();
    let w = ventana(&mut p, 300, 160);
    let boton = nuevo(&mut p, 0, "frbuttonclass", &[(ga::LEFT, 20), (ga::TOP, 20), (ga::WIDTH, 80), (ga::HEIGHT, 14), (ga::ID, 1), (ga::RELVERIFY, 1)]);
    let texto = cadena(&mut p.c, "Pulsa");
    set(&mut p, boton, &[(ga::TEXT, texto)]);
    let cad = nuevo(
        &mut p,
        0,
        "strgclass",
        &[(ga::LEFT, 20), (ga::TOP, 50), (ga::WIDTH, 120), (ga::HEIGHT, 14), (ga::ID, 2), (ga::RELVERIFY, 1), (stringa::MAXCHARS, 20), (stringa::LONGVAL, 0), (ga::PREVIOUS, boton)],
    );
    let mapa = etiquetas(&mut p, &[(pga::TOP, stringa::LONGVAL)]);
    let prop = nuevo(
        &mut p,
        0,
        "propgclass",
        &[
            (ga::LEFT, 20),
            (ga::TOP, 80),
            (ga::WIDTH, 200),
            (ga::HEIGHT, 12),
            (ga::ID, 3),
            (ga::RELVERIFY, 1),
            (pga::FREEDOM, pga::FREEHORIZ),
            (pga::TOTAL, 100),
            (pga::VISIBLE, 10),
            (ica::TARGET, cad),
            (ica::MAP, mapa),
            (ga::PREVIOUS, cad),
        ],
    );
    // AddGList no pinta (como en el Amiga); RefreshGList sí: GM_RENDER al atender la cola.
    i(&mut p.c, "AddGList", &[u32::MAX, u32::MAX], &[w, boton, 0]);
    assert!(!p.c.1.boopsi_pendiente());
    i(&mut p.c, "RefreshGList", &[u32::MAX], &[boton, w, 0]);
    assert!(p.c.1.boopsi_pendiente());
    atender(&mut p);
    assert!(!p.c.1.boopsi_pendiente());
    // El botón está pintado (no es todo del color de fondo).
    let bm = rl32(&p, rl32(&p, w + win::RPORT) + 4);
    let l = p.c.1.lienzo(bm).unwrap();
    let fondo = l.pixel(5, 150);
    assert!((20..100).any(|x| l.pixel(x, 20) != fondo), "el borde del botón");
    // Un clic en el botón: GADGETUP con su IAddress.
    clic(&mut p, w, 50, 26);
    let m = mensajes(&mut p, w);
    assert!(m.contains(&(idcmp::GADGETUP, 1, boton)) || m.iter().any(|x| x.0 == idcmp::GADGETUP && x.2 == boton), "{m:?}");
    // Arrastrar el mando del deslizador: cambia PGA_Top, notifica a la cadena (ICA_MAP) y da GADGETUP.
    let antes = get(&mut p, prop, pga::TOP);
    arrastrar(&mut p, w, (30, 86), (150, 86));
    let despues = get(&mut p, prop, pga::TOP);
    assert!(despues > antes + 30, "{antes} → {despues}");
    assert_eq!(get(&mut p, cad, stringa::LONGVAL), despues);
    let m = mensajes(&mut p, w);
    assert!(m.iter().any(|x| x.0 == idcmp::GADGETUP && x.2 == prop), "{m:?}");
    // Escribir en la cadena: clic, teclas y Return.
    let vacia = cadena(&mut p.c, "");
    set_g(&mut p, cad, w, &[(stringa::TEXTVAL, vacia)]);
    clic(&mut p, w, 30, 56);
    for (k, ch) in [(0x02u32, '4'), (0x03, '2')] {
        tecla(&mut p, w, k, Some(ch));
    }
    tecla(&mut p, w, 28, Some('\r'));
    let buf = get(&mut p, cad, stringa::TEXTVAL);
    assert_eq!(c_str(&p, buf), "42");
    let m = mensajes(&mut p, w);
    assert!(m.iter().any(|x| x.0 == idcmp::GADGETUP && x.2 == cad), "{m:?}");
    // Quitar y cerrar.
    i(&mut p.c, "RemoveGList", &[u32::MAX], &[w, boton]);
    i(&mut p.c, "CloseWindow", &[], &[w]);
    for o in [boton, cad, prop] {
        dispose(&mut p, o);
    }
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

/// Un gancho de edición "en 68k": pasa a mayúsculas lo que se escribe (SGWork, SGH_KEY).
fn gancho_mayusculas(p: &mut Prueba, r: Registros) -> Result<Registros, Corte68k> {
    let (sg, msg) = (r.a[2], r.a[1]);
    assert_eq!(rl32(p, msg), sgw::SGH_KEY);
    let op = l16(&p.c, sg + sgw::EDITOP);
    if op == sgw::EO_INSERTCHAR {
        let wb = rl32(p, sg + sgw::WORKBUFFER);
        let pos = l16(&p.c, sg + sgw::BUFFERPOS) as u32;
        let c = l8(&p.c, wb + pos - 1).to_ascii_uppercase();
        p.c.0.mem.escribir8(wb + pos - 1, c).unwrap();
    }
    let mut out = r;
    out.d[0] = u32::MAX;
    Ok(out)
}

#[test]
fn gancho_de_edicion_68k() {
    let mut p = prueba();
    let w = ventana(&mut p, 200, 80);
    let h = reservar(&mut p.c, 20);
    e32(&mut p.c, h + 8, 0x00F6_0000);
    p.codigo.insert(0x00F6_0000, gancho_mayusculas);
    let cad = nuevo(
        &mut p,
        0,
        "strgclass",
        &[(ga::LEFT, 10), (ga::TOP, 20), (ga::WIDTH, 120), (ga::HEIGHT, 14), (ga::RELVERIFY, 1), (stringa::MAXCHARS, 16), (stringa::EDITHOOK, h)],
    );
    i(&mut p.c, "AddGList", &[u32::MAX, u32::MAX], &[w, cad, 0]);
    i(&mut p.c, "RefreshGList", &[u32::MAX], &[cad, w, 0]);
    atender(&mut p);
    clic(&mut p, w, 15, 26);
    for (k, ch) in [(0x10u32, 'q'), (0x12, 'e')] {
        tecla(&mut p, w, k, Some(ch));
    }
    let buf = get(&mut p, cad, stringa::TEXTVAL);
    assert_eq!(c_str(&p, buf), "QE");
    // Un gancho que no vuelve en mitad de la edición: corta.
    p.codigo.insert(0x00F6_0000, despachador_que_no_vuelve);
    let s = p.c.1.superficie(w).unwrap();
    p.c.1.comp.meter(s, Evento::Tecla { codigo: 0x11, pulsada: true, repeticion: false, caracter: Some('w') });
    let (ex, gr) = p.partes();
    gr.atender(ex);
    assert!(!caja_intuition::boopsi::atender(&mut p), "el programa acabó dentro del gancho");
    i(&mut p.c, "RemoveGList", &[u32::MAX], &[w, cad]);
    dispose(&mut p, cad);
    i(&mut p.c, "CloseWindow", &[], &[w]);
}

#[test]
fn objetivo_idcmp() {
    let mut p = prueba();
    let w = ventana(&mut p, 200, 80);
    let prop = nuevo(
        &mut p,
        0,
        "propgclass",
        &[(ga::LEFT, 10), (ga::TOP, 20), (ga::WIDTH, 150), (ga::HEIGHT, 12), (ga::ID, 5), (pga::FREEDOM, pga::FREEHORIZ), (pga::TOTAL, 50), (pga::VISIBLE, 5), (ica::TARGET, ica::TARGET_IDCMP)],
    );
    i(&mut p.c, "AddGList", &[u32::MAX, u32::MAX], &[w, prop, 0]);
    atender(&mut p);
    arrastrar(&mut p, w, (15, 26), (100, 26));
    let m = mensajes(&mut p, w);
    let upd: Vec<_> = m.iter().filter(|x| x.0 == idcmp::IDCMPUPDATE).collect();
    assert!(!upd.is_empty(), "{m:?}");
    // Las etiquetas del mensaje se sueltan al contestar: no queda memoria del sistema de más.
    i(&mut p.c, "RemoveGList", &[u32::MAX], &[w, prop]);
    i(&mut p.c, "CloseWindow", &[], &[w]);
    dispose(&mut p, prop);
}

#[test]
fn grupo_de_gadgets() {
    let mut p = prueba();
    let w = ventana(&mut p, 300, 120);
    let grupo = nuevo(&mut p, 0, "groupgclass", &[(ga::LEFT, 20), (ga::TOP, 20), (ga::ID, 9)]);
    let b1 = nuevo(&mut p, 0, "frbuttonclass", &[(ga::LEFT, 0), (ga::TOP, 0), (ga::WIDTH, 60), (ga::HEIGHT, 14), (ga::ID, 1), (ga::RELVERIFY, 1)]);
    let b2 = nuevo(&mut p, 0, "frbuttonclass", &[(ga::LEFT, 70), (ga::TOP, 0), (ga::WIDTH, 60), (ga::HEIGHT, 14), (ga::ID, 2), (ga::RELVERIFY, 1)]);
    for b in [b1, b2] {
        let m = mensaje(&mut p, &[om::ADDMEMBER, b]);
        do_method(&mut p, grupo, m).unwrap();
    }
    assert_eq!(l16(&p.c, b2 + 4), 90, "el miembro se coloca respecto al grupo");
    assert_eq!(l16(&p.c, grupo + 8), 130, "el grupo crece");
    i(&mut p.c, "AddGList", &[u32::MAX, u32::MAX], &[w, grupo, 0]);
    atender(&mut p);
    clic(&mut p, w, 100, 25);
    let m = mensajes(&mut p, w);
    assert!(m.iter().any(|x| x.0 == idcmp::GADGETUP && x.2 == b2), "{m:?}");
    i(&mut p.c, "RemoveGList", &[u32::MAX], &[w, grupo]);
    i(&mut p.c, "CloseWindow", &[], &[w]);
    // Disponer el grupo dispone a los miembros.
    dispose(&mut p, grupo);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
}

#[test]
fn imagenes() {
    let mut p = prueba();
    let (bm, rp) = nativo(&mut p.c, 100, 60);
    let _ = bm;
    let di = 0;
    for (n, t) in [
        ("frameiclass", vec![(ia::WIDTH, 40), (ia::HEIGHT, 20), (ia::RECESSED, 1)]),
        ("sysiclass", vec![(ia::SYSIA_WHICH, 0x0E)]),
        ("fillrectclass", vec![(ia::WIDTH, 10), (ia::HEIGHT, 10), (ia::FGPEN, 2)]),
    ] {
        let o = nuevo(&mut p, 0, n, &t);
        assert_ne!(o, 0, "{n}");
        let m = mensaje(&mut p, &[im::DRAW, rp, (5 << 16) | 5, ids::SELECTED, di]);
        do_method(&mut p, o, m).unwrap();
        // IM_HITTEST dentro y fuera.
        let m = mensaje(&mut p, &[im::HITTEST, (2 << 16) | 2]);
        assert_eq!(do_method(&mut p, o, m), Ok(1), "{n}");
        let m = mensaje(&mut p, &[im::HITTEST, (500 << 16) | 2]);
        assert_eq!(do_method(&mut p, o, m), Ok(0), "{n}");
        dispose(&mut p, o);
    }
    // Algo se pintó.
    let pintados = (0..60).flat_map(|y| (0..100).map(move |x| (x, y))).filter(|&(x, y)| pluma(&mut p.c, rp, x, y) != 0).count();
    assert!(pintados > 50, "{pintados}");
    // itexticlass con una IntuiText.
    let it = reservar(&mut p.c, 20);
    p.c.0.mem.escribir8(it, 1).unwrap();
    let s = cadena(&mut p.c, "Hola");
    e32(&mut p.c, it + 12, s);
    let o = nuevo(&mut p, 0, "itexticlass", &[(ia::DATA, it)]);
    let m = mensaje(&mut p, &[im::DRAW, rp, (5 << 16) | 40, 0, 0]);
    do_method(&mut p, o, m).unwrap();
    dispose(&mut p, o);
    assert_eq!(avisos(&p), 0, "{:?}", p.c.1.avisos());
}

#[test]
fn grupos_enormes_y_tamanos_absurdos() {
    let mut p = prueba();
    let w = ventana(&mut p, 200, 100);
    let grupo = nuevo(&mut p, 0, "groupgclass", &[(ga::LEFT, 0), (ga::TOP, 0)]);
    for k in 0..3000u32 {
        let g = nuevo(&mut p, 0, "buttongclass", &[(ga::LEFT, k % 200), (ga::TOP, k % 90), (ga::WIDTH, 0x7FFF), (ga::HEIGHT, 0xFFFF_8000)]);
        let m = mensaje(&mut p, &[om::ADDMEMBER, g]);
        do_method(&mut p, grupo, m).unwrap();
    }
    i(&mut p.c, "AddGList", &[u32::MAX, u32::MAX], &[w, grupo, 0]);
    atender(&mut p);
    clic(&mut p, w, 10, 10);
    i(&mut p.c, "RemoveGList", &[u32::MAX], &[w, grupo]);
    i(&mut p.c, "CloseWindow", &[], &[w]);
    dispose(&mut p, grupo);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
}

#[test]
fn la_tarea_muere_con_todo() {
    let mut p = prueba();
    let w = ventana(&mut p, 200, 100);
    let cl = hacer_clase(&mut p, "gadgetclass", despachador_contador, ENTRADA_USUARIO);
    let g = nuevo(&mut p, cl, "", &[(ga::LEFT, 10), (ga::TOP, 10), (ga::WIDTH, 30), (ga::HEIGHT, 10)]);
    i(&mut p.c, "AddGList", &[u32::MAX, u32::MAX], &[w, g, 0]);
    let s = p.c.1.superficie(w).unwrap();
    p.c.1.comp.meter(s, Evento::Raton { x: 12, y: 12 });
    p.c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
    let (ex, gr) = p.partes();
    gr.atender(ex);
    // Muere con el clic en la cola, la clase y el objeto vivos.
    let t = p.c.2;
    let (ex, gr) = p.partes();
    gr.tarea_muerta(ex, t);
    assert_eq!(p.c.1.objetos_boopsi(), 0);
    assert!(!p.c.1.boopsi_pendiente());
    assert!(caja_intuition::boopsi::atender(&mut p));
}
