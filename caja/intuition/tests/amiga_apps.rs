//! Lo que pidieron programas de verdad en la rama amiga-apps (LightWave 5): `PrintIText` (las etiquetas de sus
//! botones), `GetPrefs` y `GetDefPrefs`, `AddFont` y `RemFont`, y `ModifyProp`/`NewModifyProp`. Con lo hostil.

mod comun;

use caja_exec::Memoria;
use caja_intuition::Evento;
use caja_intuition::compositor::BTN_LEFT;
use comun::*;

/// Un IntuiText: plumas, modo, sitio, fuente (0: la del RastPort), texto y el siguiente.
fn itext(c: &mut Caja, fp: u8, x: i16, y: i16, texto: &str, sig: u32) -> u32 {
    let it = reservar(c, 20);
    let t = cadena(c, texto);
    c.0.mem.escribir8(it, fp).unwrap();
    e16(c, it + 4, x as u16);
    e16(c, it + 6, y as u16);
    e32(c, it + 12, t);
    e32(c, it + 16, sig);
    it
}

#[test]
fn print_itext_pinta_cada_texto_en_su_sitio() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 2, 64, 32);
    let b = itext(&mut c, 2, 0, 16, "B", 0);
    let a = itext(&mut c, 1, 0, 0, "A", b);
    assert_eq!(i(&mut c, "PrintIText", &[4, 2], &[rp, a]), 0);
    // La "A" con la pluma 1 arriba (a partir de (4, 2)); la "B" con la 2, 16 puntos más abajo.
    let arriba: Vec<i32> = (2..10).flat_map(|y| (4..12).map(move |x| (x, y))).map(|(x, y)| pluma(&mut c, rp, x, y)).collect();
    let abajo: Vec<i32> = (18..26).flat_map(|y| (4..12).map(move |x| (x, y))).map(|(x, y)| pluma(&mut c, rp, x, y)).collect();
    assert!(arriba.contains(&1) && !arriba.contains(&2), "{arriba:?}");
    assert!(abajo.contains(&2) && !abajo.contains(&1), "{abajo:?}");
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn print_itext_hostil() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 32, 16);
    // Una cadena en círculo: se corta (256 textos), sin colgar.
    let a = itext(&mut c, 1, 0, 0, "x", 0);
    e32(&mut c, a + 16, a);
    assert_eq!(i(&mut c, "PrintIText", &[0, 0], &[rp, a]), 0);
    // Un texto fuera de la memoria o en la página 0, o un RastPort malo: aviso, sin pánico.
    for malo in [8u32, 0xFFFF_FFF0] {
        let n = c.1.avisos().len();
        i(&mut c, "PrintIText", &[0, 0], &[rp, malo]);
        assert!(c.1.avisos().len() > n);
    }
    i(&mut c, "PrintIText", &[0, 0], &[0, a]);
}

#[test]
fn get_prefs() {
    let mut c = caja();
    let buf = reservar(&mut c, 232);
    for nombre in ["GetPrefs", "GetDefPrefs"] {
        assert_eq!(i(&mut c, nombre, &[232], &[buf]), buf);
        assert_eq!(l8(&c, buf), 8, "FontHeight");
        assert_eq!(l32(&c, buf + 24), 500_000, "el doble clic");
        assert_ne!(l16(&c, buf + 226), 0, "wb_Width");
    }
    // Solo lo que cabe: con 4 bytes, el resto no se toca.
    let corto = reservar(&mut c, 8);
    e32(&mut c, corto + 4, 0xDEAD_BEEF);
    assert_eq!(i(&mut c, "GetPrefs", &[4], &[corto]), corto);
    assert_eq!(l32(&c, corto + 4), 0xDEAD_BEEF);
    // Hostil: búfer nulo (no hace nada), en la página 0 o fuera.
    assert_eq!(i(&mut c, "GetPrefs", &[232], &[0]), 0);
    i(&mut c, "GetPrefs", &[232], &[0x10]);
    i(&mut c, "GetPrefs", &[0xFFFF_FFFF], &[0xFFFF_FF00]);
    assert!(!c.1.avisos().is_empty());
}

#[test]
fn add_font_y_rem_font() {
    let mut c = caja();
    // Una copia de topaz con otro nombre: AddFont la hace visible para OpenFont; RemFont la quita.
    let ta = reservar(&mut c, 8);
    let topaz = cadena(&mut c, "topaz.font");
    e32(&mut c, ta, topaz);
    e16(&mut c, ta + 4, 8);
    let f = g(&mut c, "OpenFont", &[], &[ta]);
    assert_ne!(f, 0);
    let nueva = reservar(&mut c, 52);
    for k in 0..52 {
        let b = l8(&c, f + k);
        c.0.mem.escribir8(nueva + k, b).unwrap();
    }
    let nombre = cadena(&mut c, "ToasterFont.font");
    e32(&mut c, nueva + 10, nombre);
    e32(&mut c, nueva, 0);
    e32(&mut c, nueva + 4, 0);
    e32(&mut c, ta, nombre);
    assert_eq!(g(&mut c, "OpenFont", &[], &[ta]), 0);
    g(&mut c, "AddFont", &[], &[0, nueva]);
    assert_eq!(g(&mut c, "OpenFont", &[], &[ta]), nueva);
    // Dos veces: no (rompería la lista).
    let n = c.1.avisos().len();
    g(&mut c, "AddFont", &[], &[0, nueva]);
    assert!(c.1.avisos().len() > n);
    g(&mut c, "RemFont", &[], &[0, nueva]);
    assert_eq!(g(&mut c, "OpenFont", &[], &[ta]), 0);
    // Quitar lo que no está, o una fuente basura: aviso.
    let n = c.1.avisos().len();
    g(&mut c, "RemFont", &[], &[0, nueva]);
    g(&mut c, "AddFont", &[], &[0, 0x10]);
    assert!(c.1.avisos().len() >= n + 2);
}

#[test]
fn modify_prop() {
    let mut c = caja();
    let gad = reservar(&mut c, 44);
    let pi = reservar(&mut c, 22);
    e32(&mut c, gad + 34, pi);
    i(&mut c, "NewModifyProp", &[1, 0x8000, 0x4000, 0x1000, 0x2000, 1], &[gad, 0, 0]);
    assert_eq!((l16(&c, pi), l16(&c, pi + 2), l16(&c, pi + 4), l16(&c, pi + 6), l16(&c, pi + 8)), (1, 0x8000, 0x4000, 0x1000, 0x2000));
    // Sin PropInfo o con el gadget fuera: aviso.
    e32(&mut c, gad + 34, 0);
    let n = c.1.avisos().len();
    i(&mut c, "ModifyProp", &[0, 0, 0, 0, 0], &[gad, 0, 0]);
    i(&mut c, "ModifyProp", &[0, 0, 0, 0, 0], &[0xFFFF_FFF0, 0, 0]);
    assert_eq!(c.1.avisos().len(), n + 2);
}

#[test]
fn draw_image_planar() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 2, 32, 16);
    // Una imagen de 4 x 2, profundidad 2, con el plano 0 dado y el 1 por PlaneOnOff (encendido): plumas 3 y 2.
    let datos = reservar(&mut c, 4);
    e16(&mut c, datos, 0xA000); // fila 0: 1010
    e16(&mut c, datos + 2, 0x5000); // fila 1: 0101
    let im = reservar(&mut c, 20);
    e16(&mut c, im + 4, 4);
    e16(&mut c, im + 6, 2);
    e16(&mut c, im + 8, 2);
    e32(&mut c, im + 10, datos);
    c.0.mem.escribir8(im + 14, 1).unwrap();
    c.0.mem.escribir8(im + 15, 2).unwrap();
    i(&mut c, "DrawImage", &[3, 4], &[rp, im]);
    let fila = |c: &mut Caja, y| (3..7).map(|x| pluma(c, rp, x, y)).collect::<Vec<_>>();
    assert_eq!(fila(&mut c, 4), [3, 2, 3, 2]);
    assert_eq!(fila(&mut c, 5), [2, 3, 2, 3]);
    assert_eq!(pluma(&mut c, rp, 7, 4), 0, "nada fuera de la imagen");
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
    // Hostil: datos fuera de la memoria, una cadena en círculo, tamaños absurdos y una BOOPSI (-1): sin pánico.
    e32(&mut c, im + 10, 0xFFFF_FF00);
    let n = c.1.avisos().len();
    i(&mut c, "DrawImage", &[0, 0], &[rp, im]);
    assert!(c.1.avisos().len() > n);
    e32(&mut c, im + 10, datos);
    e32(&mut c, im + 16, im);
    i(&mut c, "DrawImage", &[0, 0], &[rp, im]);
    e16(&mut c, im + 4, 0x7FFF);
    e16(&mut c, im + 8, 0xFFFF);
    i(&mut c, "DrawImage", &[0, 0], &[rp, im]);
}

#[test]
fn display_alert() {
    use caja_exec::Accion;
    let mut c = caja();
    // Dos trozos: "Sin memoria" y, debajo, "para el módulo".
    let a = reservar(&mut c, 64);
    let b = |c: &mut Caja, d: u32, v: &[u8]| {
        for (k, x) in v.iter().enumerate() {
            c.0.mem.escribir8(d + k as u32, *x).unwrap();
        }
    };
    b(&mut c, a, b"\x00\x10\x10Sin memoria\x00\x01\x00\x10\x20para el modulo\x00\x00");
    let n = c.1.numero_intuition("DisplayAlert").unwrap();
    let mut r = regs(&[0x0001_0000, 40], &[a]);
    assert_eq!(c.1.llamar(&mut c.0, n, &mut r), Accion::Bloquear);
    assert_eq!(c.1.ventanas().len(), 1, "el requester de la alerta");
    let t = c.2;
    c.1.tarea_muerta(&mut c.0, t);
    assert!(c.1.ventanas().is_empty());
    // Hostil: el texto fuera de la memoria: FALSE y un aviso, sin ventana.
    let mut r = regs(&[0x8000_0000, 40], &[0xFFFF_FFF0]);
    assert_eq!(c.1.llamar(&mut c.0, n, &mut r), Accion::Seguir);
    assert_eq!(r.d[0], 0);
    assert!(c.1.ventanas().is_empty());
}

#[test]
fn pedidor_de_ficheros() {
    use caja_intuition::AccionAsl;
    use caja_intuition::estructuras::window as win;
    let mut c = caja();
    let t = c.2;
    let entradas = vec![(b"uno.mod".to_vec(), false), (b"Viejos".to_vec(), true), (b"dos.mod".to_vec(), false)];
    c.1.pedidor_abrir(&mut c.0, t, b"Carga un modulo", b"Cargar", b"Work:Musica", b"", entradas).unwrap();
    assert!(c.1.pedidor_abierto(t));
    let w = *c.1.ventanas().last().unwrap();
    let s = c.1.superficie(w).unwrap();
    let bi = c.0.mem.leer8(w + win::BORDERLEFT).unwrap() as i32;
    let ba = c.0.mem.leer8(w + win::BORDERTOP).unwrap() as i32;
    // Un clic en la primera fila (el cajón "Viejos", que va primero) y otro enseguida: entrar en él.
    let (x, y) = (bi + 30, ba + 8 + 4);
    let clic = |c: &mut Caja| {
        c.1.comp.meter(s, Evento::Raton { x, y });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: true });
        c.1.comp.meter(s, Evento::Boton { boton: BTN_LEFT, pulsado: false });
        c.1.atender(&mut c.0);
    };
    clic(&mut c);
    clic(&mut c);
    let (a, cajon, sel) = c.1.pedidor_accion(t).expect("una acción");
    assert_eq!(a, AccionAsl::Entrar(Some(0)));
    assert_eq!((cajon.as_slice(), sel.as_slice()), (&b"Work:Musica"[..], &b"Viejos"[..]));
    assert!(c.1.pedidor_accion(t).is_none(), "una vez");
    // Escribir un nombre e Intro: aceptar con lo escrito.
    c.1.pedidor_poner(&mut c.0, t, b"Work:Musica/Viejos", vec![(b"tres.mod".to_vec(), false)]);
    for ch in "x.mod".chars() {
        c.1.comp.meter(s, Evento::Tecla { codigo: 0, pulsada: true, repeticion: false, caracter: Some(ch) });
    }
    c.1.comp.meter(s, Evento::Tecla { codigo: 28, pulsada: true, repeticion: false, caracter: Some('\r') });
    c.1.atender(&mut c.0);
    let (a, cajon, sel) = c.1.pedidor_accion(t).expect("aceptar");
    assert_eq!(a, AccionAsl::Aceptar);
    assert_eq!((cajon.as_slice(), sel.as_slice()), (&b"Work:Musica/Viejos"[..], &b"x.mod"[..]));
    // Cerrar desde la barra: cancelar. Y cerrar el pedidor suelta su ventana.
    c.1.comp.meter(s, Evento::Cerrar);
    c.1.atender(&mut c.0);
    assert_eq!(c.1.pedidor_accion(t).unwrap().0, AccionAsl::Cancelar);
    c.1.pedidor_cerrar(&mut c.0, t);
    assert!(!c.1.pedidor_abierto(t));
    assert!(c.1.ventanas().is_empty());
    // Hostil: 10.000 entradas y nombres de 300 letras: se abre y se pinta sin pánico; la tarea muere: se cierra.
    let muchas = (0..10_000).map(|i| (format!("{i:0300}").into_bytes(), i % 7 == 0)).collect();
    c.1.pedidor_abrir(&mut c.0, t, &[b'T'; 400], b"", &[b'c'; 1000], &[b'f'; 1000], muchas).unwrap();
    c.1.tarea_muerta(&mut c.0, t);
    assert!(!c.1.pedidor_abierto(t));
}

#[test]
fn request_bloquea_la_ventana() {
    use caja_intuition::estructuras::window as win;
    let mut c = caja();
    let req = reservar(&mut c, 112);
    e32(&mut c, req, 0xDEAD_BEEF);
    i(&mut c, "InitRequester", &[], &[req]);
    assert_eq!(l32(&c, req), 0);
    // Una ventana cualquiera (la del pedidor sirve: es una ventana de intuition).
    let t = c.2;
    c.1.pedidor_abrir(&mut c.0, t, b"x", b"", b"", b"", vec![]).unwrap();
    let w = *c.1.ventanas().last().unwrap();
    assert_eq!(i(&mut c, "Request", &[], &[req, w]), 1);
    assert_eq!(l16(&c, w + win::REQCOUNT), 1);
    // Dos veces el mismo: no cuenta dos.
    i(&mut c, "Request", &[], &[req, w]);
    assert_eq!(l16(&c, w + win::REQCOUNT), 1);
    i(&mut c, "EndRequest", &[], &[req, w]);
    assert_eq!(l16(&c, w + win::REQCOUNT), 0);
    // Hostil: en una ventana que no es, o un requester fuera: aviso, sin pánico; cerrar la ventana con uno puesto.
    let n = c.1.avisos().len();
    i(&mut c, "Request", &[], &[req, 0x1234]);
    i(&mut c, "Request", &[], &[0xFFFF_FF00, w]);
    assert!(c.1.avisos().len() >= n + 2);
    i(&mut c, "Request", &[], &[req, w]);
    c.1.pedidor_cerrar(&mut c.0, t);
    assert!(c.1.ventanas().is_empty());
}
