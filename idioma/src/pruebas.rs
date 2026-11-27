//! Las pruebas del PC (`cargo test`): lo normal y lo hostil (ficheros cortados, largos enormes, números repetidos,
//! catálogos de otro programa o de otra versión, `locale.prefs` roto, nombres que quieren salir del cajón, UTF-8 malo).

use crate::catalogo::{self, Catalogo, Error, Pedido};
use crate::fecha::{self, cadenas, Momento};
use crate::formato;
use crate::iff;
use crate::letras::{self, Juego};
use crate::pais::{self, Pais, Signo};
use crate::prefs::{self, Prefs};
use crate::{Local, Puesto};

const PEDIDO: Pedido = Pedido { programa: "prueba", version: 1 };

fn de_prueba(juego: Juego) -> Vec<u8> {
    catalogo::escribir("prueba.catalog", 1, 2, "15.11.2026", "english", juego, &[(1, "Open"), (2, "Señor ñandú"), (7, "{} items"), (3, "")])
}

/// Un catálogo con los trozos que se le den, tal cual.
fn a_mano(trozos: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut cuerpo = Vec::new();
    for (id, datos) in trozos {
        iff::trozo(id, datos, &mut cuerpo);
    }
    iff::escribir_forma(b"CTLG", &cuerpo)
}

fn texto_strs(textos: &[(u32, &[u8])]) -> Vec<u8> {
    let mut s = Vec::new();
    for (n, t) in textos {
        s.extend_from_slice(&n.to_be_bytes());
        s.extend_from_slice(&(t.len() as u32 + 1).to_be_bytes());
        s.extend_from_slice(t);
        s.push(0);
        s.resize(s.len().next_multiple_of(4), 0);
    }
    s
}

const UTF8: [u8; 32] = {
    let mut c = [0u8; 32];
    c[3] = 106;
    c
};

#[test]
fn catalogo_utf8_y_latin1_dan_lo_mismo() {
    for juego in [Juego::Utf8, Juego::Latin1, Juego::Latin9] {
        let c = Catalogo::leer(&de_prueba(juego), PEDIDO).unwrap();
        assert_eq!((c.nombre.as_str(), c.version, c.revision, c.idioma.as_str()), ("prueba.catalog", 1, 2, "english"));
        assert_eq!(c.texto(1), Some("Open"));
        assert_eq!(c.texto(2), Some("Señor ñandú"), "{juego:?}");
        assert_eq!(c.texto(3), Some(""));
        assert_eq!(c.texto(4), None);
        assert_eq!(c.o(4, "de siempre"), "de siempre");
        assert_eq!(c.cuantos(), 4);
        assert_eq!(c.numeros().collect::<Vec<_>>(), [1, 2, 3, 7]);
    }
}

#[test]
fn catalogo_del_amiga_con_euro_en_latin9_y_sin_cset() {
    // Sin CSET es Latin-1: 0xA4 es `¤`. Con CSET 111, el euro.
    let strs = texto_strs(&[(1, b"10 \xa4"), (2, b"ma\xf1ana")]);
    let c = Catalogo::leer(&a_mano(&[(b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert_eq!((c.texto(1), c.texto(2)), (Some("10 ¤"), Some("mañana")));
    let mut cset = [0u8; 32];
    cset[3] = 111;
    let c = Catalogo::leer(&a_mano(&[(b"CSET", &cset), (b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert_eq!(c.texto(1), Some("10 €"));
}

#[test]
fn catalogo_cortado_en_cualquier_sitio_no_vale_ni_rompe() {
    let bueno = de_prueba(Juego::Utf8);
    for n in 0..bueno.len() {
        // Cortado por cualquier byte: o no vale, o (si solo falta el relleno del final) da lo mismo que entero.
        match Catalogo::leer(&bueno[..n], PEDIDO) {
            Err(_) => {}
            Ok(c) => panic!("cortado en {n} de {} y vale con {} textos", bueno.len(), c.cuantos()),
        }
    }
    assert!(Catalogo::leer(&bueno, PEDIDO).is_ok());
    assert_eq!(Catalogo::leer(b"", PEDIDO).err(), Some(Error::Formato(iff::Error::NoEsIff)));
    assert_eq!(Catalogo::leer(b"GIF89a, no un catalogo", PEDIDO).err(), Some(Error::Formato(iff::Error::NoEsIff)));
}

#[test]
fn catalogo_con_largos_enormes() {
    // El FORM dice medir 4 GB.
    let mut c = de_prueba(Juego::Utf8);
    c[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(Catalogo::leer(&c, PEDIDO).err(), Some(Error::Formato(iff::Error::Truncado)));
    // Un trozo dice medir 4 GB.
    let mut c = de_prueba(Juego::Utf8);
    c[16..20].copy_from_slice(&0xffff_fff0u32.to_be_bytes());
    assert_eq!(Catalogo::leer(&c, PEDIDO).err(), Some(Error::Formato(iff::Error::Truncado)));
    // Un texto dice medir 4 GB (y otro, justo uno más de lo que queda).
    for largo in [u32::MAX, 9] {
        let mut strs = texto_strs(&[(1, b"hola")]);
        strs[4..8].copy_from_slice(&largo.to_be_bytes());
        assert_eq!(Catalogo::leer(&a_mano(&[(b"CSET", &UTF8), (b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).err(), Some(Error::TextoCortado), "{largo}");
    }
    // Media cabecera de texto al final.
    let mut strs = texto_strs(&[(1, b"hola")]);
    strs.extend_from_slice(&[0, 0, 0, 2, 0]);
    assert_eq!(Catalogo::leer(&a_mano(&[(b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).err(), Some(Error::TextoCortado));
    // Un fichero de más de 1 MB ni se mira.
    let grande = vec![0u8; catalogo::MAX_BYTES + 1];
    assert_eq!(Catalogo::leer(&grande, PEDIDO).err(), Some(Error::Grande));
    // Y uno de 1 MB justo, lleno de textos vacíos, se lee sin tardar ni reventar.
    let muchos: Vec<u8> = (0..(catalogo::MAX_BYTES as u32 - 64) / 12).flat_map(|i| [i.to_be_bytes(), 1u32.to_be_bytes(), [0; 4]].concat()).collect();
    let c = Catalogo::leer(&a_mano(&[(b"STRS", &muchos)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert!(c.cuantos() > 80_000);
}

#[test]
fn catalogo_con_numeros_repetidos_vale_el_primero() {
    let strs = texto_strs(&[(5, b"primero"), (2, b"dos"), (5, b"segundo"), (5, b"tercero"), (2, b"otro dos")]);
    let c = Catalogo::leer(&a_mano(&[(b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert_eq!((c.texto(5), c.texto(2), c.cuantos(), c.repetidos), (Some("primero"), Some("dos"), 2, 3));
    // Y de los trozos repetidos, también el primero.
    let otro = texto_strs(&[(5, b"de otro trozo")]);
    let c = Catalogo::leer(&a_mano(&[(b"STRS", &strs), (b"STRS", &otro)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert_eq!(c.texto(5), Some("primero"));
}

#[test]
fn catalogo_de_otro_programa_o_de_otra_version() {
    let c = de_prueba(Juego::Utf8);
    assert_eq!(Catalogo::leer(&c, Pedido { programa: "explorador", version: 1 }).err(), Some(Error::DeOtro));
    assert_eq!(Catalogo::leer(&c, Pedido { programa: "prueba", version: 2 }).err(), Some(Error::Version { pedida: 2, tiene: 1 }));
    // Sin mirar mayúsculas, y con versión 0 vale cualquiera.
    assert!(Catalogo::leer(&c, Pedido { programa: "Prueba", version: 0 }).is_ok());
    // Uno sin `$VER:` es de la versión 0: solo vale para quien no pide ninguna.
    let sin = a_mano(&[(b"STRS", &texto_strs(&[(1, b"hola")]))]);
    assert_eq!(Catalogo::leer(&sin, PEDIDO).err(), Some(Error::Version { pedida: 1, tiene: 0 }));
    assert!(Catalogo::leer(&sin, Pedido { programa: "prueba", version: 0 }).is_ok());
    // Un `$VER:` con basura por versión.
    let raro = a_mano(&[(b"FVER", b"$VER: prueba.catalog muchas.veces\0"), (b"STRS", &texto_strs(&[(1, b"hola")]))]);
    assert_eq!(Catalogo::leer(&raro, PEDIDO).err(), Some(Error::Version { pedida: 1, tiene: 0 }));
    // Otro tipo de IFF (un locale.prefs) no es un catálogo.
    assert_eq!(Catalogo::leer(&Prefs::default().escribir(), PEDIDO).err(), Some(Error::Formato(iff::Error::OtroTipo)));
}

#[test]
fn catalogo_con_utf8_invalido_o_juego_desconocido() {
    // Un texto con UTF-8 roto se salta; los demás valen.
    let strs = texto_strs(&[(1, b"bien"), (2, b"mal \xff\xfe"), (3, b"cortado \xc3"), (4, "también bien".as_bytes()), (5, b"largo de m\xe1s \xed\xa0\x80")]);
    let c = Catalogo::leer(&a_mano(&[(b"CSET", &UTF8), (b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert_eq!((c.texto(1), c.texto(2), c.texto(3), c.texto(4), c.texto(5), c.malos), (Some("bien"), None, None, Some("también bien"), None, 3));
    assert_eq!(c.o(2, "el de siempre"), "el de siempre");
    // Un juego que no sabemos leer (KOI8-R, 2084): el catálogo entero no vale.
    let mut cset = [0u8; 32];
    cset[0..4].copy_from_slice(&2084u32.to_be_bytes());
    assert_eq!(Catalogo::leer(&a_mano(&[(b"CSET", &cset), (b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).err(), Some(Error::Juego(2084)));
    // Un 0 en medio de un texto: acaba ahí.
    let strs = texto_strs(&[(1, b"antes\0despu\xc3\xa9s")]);
    let c = Catalogo::leer(&a_mano(&[(b"CSET", &UTF8), (b"STRS", &strs)]), Pedido { programa: "x", version: 0 }).unwrap();
    assert_eq!(c.texto(1), Some("antes"));
}

#[test]
fn puesto_da_el_texto_o_el_de_siempre() {
    static P: Puesto = Puesto::nuevo();
    assert_eq!(P.texto(1, "Abrir"), "Abrir");
    assert_eq!(P.con(7, "{} elementos", &[&3]), "3 elementos");
    P.poner(Some(Catalogo::leer(&de_prueba(Juego::Utf8), PEDIDO).unwrap()));
    let abierto = P.texto(1, "Abrir");
    assert_eq!(abierto, "Open");
    assert_eq!(P.texto(99, "Sin traducir"), "Sin traducir");
    // El 0 es "sin número".
    assert_eq!(P.texto(0, "Sin número"), "Sin número");
    assert_eq!(P.con(7, "{} elementos", &[&3]), "3 items");
    // Una traducción que pide un valor que el original no da: sale el original.
    assert_eq!(P.con(7, "sin huecos", &[]), "sin huecos");
    assert_eq!(P.con(1, "Abrir {}", &[&"x"]), "Open");
    // Al quitar el catálogo, lo que ya se dio sigue valiendo.
    P.poner(None);
    assert_eq!((abierto, P.texto(1, "Abrir")), ("Open", "Abrir"));
}

#[test]
fn formato_con_huecos() {
    assert_eq!(formato::poner("{} de {}", &[&1, &"dos"]), "1 de dos");
    assert_eq!(formato::poner("{1} de {0}, {0}", &[&1, &"dos"]), "dos de 1, 1");
    assert_eq!(formato::poner("{{}} y {{{}}}", &[&5]), "{} y {5}");
    // Hostil: huecos de más, llaves sueltas, posiciones enormes.
    assert_eq!(formato::poner("{} {} {}", &[&1]), "1  ");
    assert_eq!(formato::poner("{9} {99} {999999999999999999999}", &[&1]), "  {999999999999999999999}");
    assert_eq!(formato::poner("{ {a} } {", &[&1]), "{ {a} } {");
    assert_eq!(formato::poner("ñ{}ñ", &[&"á"]), "ñáñ");
    assert_eq!(formato::poner("", &[]), "");
    assert!(formato::casa("{} de {}", "{1} of {0}"));
    assert!(formato::casa("{} de {}", "all"));
    assert!(!formato::casa("{}", "{} {}"));
    assert!(!formato::casa("nada", "{5}"));
}

#[test]
fn prefs_ida_y_vuelta_y_el_layout_del_amiga() {
    let p = Prefs { pais: "España".into(), idiomas: vec!["english".into(), "español".into()], gmt: -60, flags: 0, datos: Some(Pais::espana()) };
    let b = p.escribir();
    // 12 de FORM y PREF, 8 + 6 de PRHD, 8 + 860 de LCLE: lo que mide el de AmigaOS.
    assert_eq!(b.len(), 894);
    assert_eq!(&b[0..4], b"FORM");
    assert_eq!(&b[26..30], b"LCLE");
    // El país en Latin-1 (ñ = 0xF1) en su sitio, y el primer idioma 32 bytes después.
    assert_eq!(&b[0x32..0x39], b"Espa\xf1a\0");
    assert_eq!(&b[0x52..0x5a], b"english\0");
    assert_eq!(&b[0x17e..0x182], &(-60i32).to_be_bytes());
    assert_eq!(Prefs::leer(&b).unwrap(), p);
    // El euro cabe (Latin-9).
    assert_eq!(Prefs::leer(&b).unwrap().datos.unwrap().moneda, "€");
    // Un país entero, ida y vuelta, y como fichero .country (12 + 14 + 8 + 504).
    let c = Pais::espana().escribir();
    assert_eq!(c.len(), 538);
    assert_eq!(Pais::leer(&c), Some(Pais::espana()));
}

#[test]
fn prefs_rotas() {
    let bueno = Prefs { pais: "España".into(), idiomas: vec!["english".into()], gmt: 0, flags: 0, datos: Some(Pais::espana()) }.escribir();
    for n in 0..bueno.len() {
        assert!(Prefs::leer(&bueno[..n]).is_err(), "cortado en {n}");
    }
    assert!(Prefs::leer(b"esto no es un fichero de preferencias").is_err());
    // Un LCLE corto.
    let mut cuerpo = Vec::new();
    iff::trozo(b"LCLE", &[0; 100], &mut cuerpo);
    assert_eq!(Prefs::leer(&iff::escribir_forma(b"PREF", &cuerpo)).err(), Some(prefs::Error::SinDatos));
    // Sin LCLE (un font.prefs).
    let mut cuerpo = Vec::new();
    iff::trozo(b"FONT", &[0; 900], &mut cuerpo);
    assert_eq!(Prefs::leer(&iff::escribir_forma(b"PREF", &cuerpo)).err(), Some(prefs::Error::SinDatos));
    // Todo a 0xFF: vale como fichero, pero sin idiomas ni país que valgan ni datos (y un minuto de diferencia horaria).
    let mut cuerpo = Vec::new();
    iff::trozo(b"LCLE", &[0xff; 860], &mut cuerpo);
    let p = Prefs::leer(&iff::escribir_forma(b"PREF", &cuerpo)).unwrap();
    assert_eq!((p.pais.as_str(), p.idiomas.len(), p.gmt, p.datos.is_some()), ("", 0, -1, false));
    // Todo a ceros: castellano y España.
    let mut cuerpo = Vec::new();
    iff::trozo(b"LCLE", &[0; 860], &mut cuerpo);
    let p = Prefs::leer(&iff::escribir_forma(b"PREF", &cuerpo)).unwrap();
    assert_eq!(p, Prefs::default());
    let l = Local::de_prefs(p);
    assert_eq!((l.idioma(), l.pais_nombre.as_str(), l.pais == Pais::espana()), ("español", "España", true));
}

#[test]
fn prefs_con_nombres_que_quieren_salir_del_cajon() {
    let malos = ["../../Estado", "a/b", "SYS:x", "..", ".oculto", "con\ttab", "a\\b", "x*", "muy largo muy largo muy largo 30"];
    let mut p = Prefs { pais: "../Paises".into(), idiomas: malos.iter().map(|s| (*s).into()).collect(), gmt: 99_999, flags: 0, datos: None };
    p.idiomas.push("english".into());
    let leido = Prefs::leer(&p.escribir()).unwrap();
    // Solo queda el que vale (el de 31 letras se cortó a 29 al escribirlo y entonces vale: se comprueba aparte).
    assert_eq!(leido.pais, "");
    assert_eq!(leido.gmt, 0);
    assert!(leido.idiomas.iter().all(|i| letras::nombre_valido(i)), "{:?}", leido.idiomas);
    assert!(leido.idiomas.contains(&"english".to_string()));
    assert!(!leido.idiomas.iter().any(|i| i.contains(['/', ':', '\\', '*']) || i.starts_with('.')), "{:?}", leido.idiomas);
    for m in malos {
        assert!(!letras::nombre_valido(m), "{m}");
    }
    assert!(!letras::nombre_valido(""));
    assert!(letras::nombre_valido("español") && letras::nombre_valido("great_britain"));
    // Repetidos (sin mirar mayúsculas) cuentan una vez; la lista acaba en el primer hueco.
    let p = Prefs { idiomas: vec!["English".into(), "english".into(), "ENGLISH".into(), "français".into()], ..Prefs::default() };
    assert_eq!(Prefs::leer(&p.escribir()).unwrap().idiomas, ["English", "français"]);
    let mut b = Prefs { idiomas: vec!["english".into()], ..Prefs::default() }.escribir();
    b[0x52 + 30 + 1..0x52 + 30 + 8].copy_from_slice(b"nglish\0"); // basura tras un hueco, como deja el editor del Amiga
    assert_eq!(Prefs::leer(&b).unwrap().idiomas, ["english"]);
}

#[test]
fn idioma_que_no_existe_y_orden_de_busqueda() {
    let l = Local::de_prefs(Prefs { idiomas: vec!["klingon".into(), "english".into(), "Español".into(), "deutsch".into()], ..Prefs::default() });
    // Se busca en los preferidos hasta llegar al del programa; lo de detrás no se mira.
    assert_eq!(l.donde_buscar("español").collect::<Vec<_>>(), ["klingon", "english"]);
    // Un programa escrito en inglés no busca catálogo si el inglés va antes.
    assert_eq!(l.donde_buscar("klingon").count(), 0);
    assert_eq!(Local::de_serie().donde_buscar("español").count(), 0);
    assert_eq!(l.idioma(), "klingon");
    assert_eq!(l.idioma_de_textos(), Some("klingon"));
    // Sin los textos del idioma, los del castellano.
    assert_eq!(l.cadena(cadenas::DIA_1 + 1), "lunes");
    assert_eq!((l.cadena(0), l.cadena(51), l.cadena(u32::MAX)), ("", "", ""));
}

#[test]
fn fechas() {
    // 2026-11-15 17:05:09 UTC, domingo.
    let t = 1_794_762_309;
    let m = Momento::de_unix(t);
    assert_eq!((m.ano, m.mes, m.dia, m.hora, m.minuto, m.segundo, m.dia_semana, m.dia_ano), (2026, 11, 15, 17, 5, 9, 0, 319));
    assert_eq!(Momento::de_unix(0), Momento { ano: 1970, mes: 1, dia: 1, hora: 0, minuto: 0, segundo: 0, dia_semana: 4, dia_ano: 1 });
    // Bisiestos y bordes.
    assert_eq!((Momento::de_unix(951_782_400).mes, Momento::de_unix(951_782_400).dia, Momento::de_unix(951_782_400).dia_ano), (2, 29, 60));
    assert_eq!((Momento::de_unix(-1).ano, Momento::de_unix(-1).segundo, Momento::de_unix(-1).dia_semana), (1969, 59, 3));
    assert_eq!(Momento::de_unix(i64::MAX).ano, 9999);
    assert_eq!(Momento::de_unix(i64::MIN).ano, 1);
    // España (de serie): una hora más que UTC.
    let l = Local::de_serie();
    assert_eq!(l.fecha_larga(t), "domingo, 15 de noviembre de 2026");
    assert_eq!(l.fecha_corta(t), "15/11/2026");
    assert_eq!(l.hora_corta(t), "18:05");
    assert_eq!(l.hora_larga(t), "18:05:09");
    assert_eq!(l.fecha_hora_corta(t), "15/11/2026 18:05");
    // Un día de una cifra no deja dos espacios.
    assert_eq!(l.fecha_larga(t - 12 * 86_400), "martes, 3 de noviembre de 2026");
    // Todos los códigos, y lo hostil: un % al final, códigos que no existen, una plantilla que quiere crecer.
    let c = |n: u32| fecha::CASTELLANO[n as usize - 1];
    let f = |p: &str| fecha::formatear(p, &m, &c);
    assert_eq!(f("%a %A %b %B %h"), "dom domingo nov noviembre nov");
    assert_eq!(f("%d %e %j %m %y %Y %w %U"), "15 15 319 11 26 2026 0 46");
    assert_eq!(f("%H %I %M %S %p %q %Q"), "17 05 05 09 p. m. 17 5");
    assert_eq!(f("%D|%x|%T|%X|%R|%r"), "11/15/26|11/15/26|17:05:09|17:05:09|17:05|05:05:09 p. m.");
    assert_eq!(f("%c"), "dom nov 15 17:05:09 2026");
    assert_eq!(f("100%% %z %"), "100% %z %");
    assert_eq!(f("%n%t"), "\n\t");
    assert!(f(&"%c%C".repeat(1000)).len() < fecha::MAX_SALIDA + 64);
    assert_eq!(fecha::formatear("%e", &Momento::de_unix(0), &c), " 1");
    // Medianoche y mediodía en 12 horas.
    assert_eq!(fecha::formatear("%I %p %Q", &Momento::de_unix(0), &c), "12 a. m. 12");
    assert_eq!(fecha::formatear("%I %p", &Momento::de_unix(12 * 3600), &c), "12 p. m.");
}

#[test]
fn numeros_y_dinero() {
    let es = Pais::espana();
    assert_eq!(es.numero(0), "0");
    assert_eq!(es.numero(999), "999");
    assert_eq!(es.numero(1_000), "1.000");
    assert_eq!(es.numero(-1_234_567), "-1.234.567");
    assert_eq!(es.numero(i64::MIN), "-9.223.372.036.854.775.808");
    assert_eq!(es.decimal(123_456, 2), "1.234,56");
    assert_eq!(es.decimal(5, 2), "0,05");
    assert_eq!(es.decimal(-5, 0), "-5");
    assert_eq!(es.decimal(1, 40), "0,000000000000000001");
    assert_eq!(es.moneda(123_456), "1.234,56 €");
    assert_eq!(es.moneda(-5), "-0,05 €");
    // A la inglesa y a la americana, con los signos en cada sitio.
    let mut gb = Pais::espana();
    gb.numeros.decimal = ".".into();
    gb.numeros.grupo = ",".into();
    gb.dinero = gb.numeros.clone();
    gb.moneda = "£".into();
    gb.positivo = Signo { signo: String::new(), espacio: false, donde: pais::SIGNO_DELANTE, moneda_detras: false };
    gb.negativo = Signo { signo: "-".into(), ..gb.positivo.clone() };
    assert_eq!((gb.moneda(123_456).as_str(), gb.moneda(-123_456).as_str()), ("£1,234.56", "-£1,234.56"));
    gb.negativo.donde = pais::SIGNO_PARENTESIS;
    assert_eq!(gb.moneda(-100), "(£1.00)");
    gb.negativo.donde = pais::SIGNO_DETRAS;
    assert_eq!(gb.moneda(-100), "£1.00-");
    gb.negativo.donde = pais::SIGNO_ANTES_DE_MONEDA;
    assert_eq!(gb.moneda(-100), "-£1.00");
    gb.negativo.donde = pais::SIGNO_TRAS_MONEDA;
    assert_eq!(gb.moneda(-100), "£-1.00");
    // Grupos raros: a la india (3 y luego de 2 en 2), sin grupos, un solo grupo, y basura.
    let mut x = Pais::espana();
    x.numeros.agrupar = [3, 2, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(x.numero(123_456_789), "12.34.56.789");
    x.numeros.agrupar = [3, 255, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(x.numero(123_456_789), "123456.789");
    x.numeros.agrupar = [0; 10];
    assert_eq!(x.numero(123_456_789), "123456789");
    x.numeros.agrupar = [1; 10];
    assert_eq!(x.numero(12_345), "1.2.3.4.5");
    x.numeros.agrupar = [200, 1, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(x.numero(u32::MAX as i64), "4294967295");
    x.numeros.agrupar = [3; 10];
    x.numeros.grupo = String::new();
    assert_eq!(x.numero(1_234_567), "1234567");
    // Un país con los decimales del dinero en 255 no revienta.
    x.decimales = 255;
    assert!(x.moneda(1).starts_with("0,000000000000000001"));
}

#[test]
fn pais_roto() {
    assert_eq!(Pais::leer(b"nada"), None);
    assert_eq!(Pais::de_datos(&[0; 503]), None);
    // A ceros: se lee, pero no está completo, y como fichero no vale.
    assert!(!Pais::de_datos(&[0; 504]).unwrap().completo());
    let mut cuerpo = Vec::new();
    iff::trozo(b"CTRY", &[0; 504], &mut cuerpo);
    assert_eq!(Pais::leer(&iff::escribir_forma(b"PREF", &cuerpo)), None);
    // Campos sin 0 final (todo letras): no se sale del campo.
    let p = Pais::de_datos(&[b'x'; 504]).unwrap();
    assert_eq!((p.fecha_hora.len(), p.fecha.len(), p.moneda.len()), (80, 40, 10));
    let l = Local { pais: p, ..Local::de_serie() };
    assert_eq!(l.fecha_corta(0), "x".repeat(40));
    assert!(!l.numero(1_000_000).is_empty() && !l.moneda(-1).is_empty());
    let bueno = Pais::espana().escribir();
    for n in 0..bueno.len() {
        assert_eq!(Pais::leer(&bueno[..n]), None, "cortado en {n}");
    }
}

#[test]
fn textos_del_idioma_desde_un_catalogo() {
    let dias: Vec<(u32, &str)> = vec![(1, "Sunday"), (2, "Monday"), (25, "November"), (41, "AM"), (42, "PM")];
    let c = Catalogo::leer(&catalogo::escribir("english.language", 1, 0, "15.11.2026", "english", Juego::Utf8, &dias), Pedido { programa: "english.language", version: 0 }).unwrap();
    let mut l = Local::de_prefs(Prefs { idiomas: vec!["english".into()], ..Prefs::default() });
    l.textos = Some(c);
    l.pais.fecha = "%A %e %B %Y, %I:%M %p".into();
    l.gmt = 0;
    assert_eq!(l.fecha_larga(1_794_762_309), "Sunday 15 November 2026, 05:05 PM");
    // Lo que el catálogo del idioma no trae, en castellano.
    assert_eq!(l.cadena(cadenas::DIA_1 + 2), "martes");
}

// ---------------------------------------------------------------------------------------------------------------
// La sesión: qué se lee y en qué orden, con un lector de mentira.

use crate::sesion::{Falta, Firma, Idiomas, Lector, MAX_ZONA, PREFS, PREFS_GUARDADAS, TEXTO, TEXTO_GUARDADO, ZONA};
use std::collections::BTreeMap;

#[derive(Default)]
struct Disco {
    /// camino → (bytes, fecha).
    ficheros: BTreeMap<String, (Vec<u8>, u64)>,
    /// Lo que no se deja leer.
    protegidos: Vec<String>,
    /// Lo que se pidió, en orden.
    pedidos: Vec<String>,
}

impl Disco {
    fn poner(&mut self, camino: &str, bytes: Vec<u8>) {
        let fecha = self.ficheros.get(camino).map_or(100, |f| f.1 + 1);
        self.ficheros.insert(camino.into(), (bytes, fecha));
    }

    fn ingles() -> Disco {
        let mut d = Disco::default();
        let gb = Pais { fecha_corta: "%d/%m/%Y".into(), moneda: "£".into(), ..Pais::espana() };
        d.poner(PREFS, Prefs { pais: "great_britain".into(), idiomas: vec!["english".into()], gmt: 0, flags: 0, datos: Some(gb) }.escribir());
        d.poner("LOCALE:Catalogs/english/prueba.catalog", de_prueba(Juego::Utf8));
        d.poner("LOCALE:Languages/english.language", catalogo::escribir("english.language", 1, 0, "15.11.2026", "english", Juego::Utf8, &[(1, "Sunday")]));
        d
    }
}

impl Lector for Disco {
    fn leer(&mut self, camino: &str, tope: usize) -> Result<(Vec<u8>, Firma), Falta> {
        self.pedidos.push(camino.into());
        if self.protegidos.iter().any(|p| p == camino) {
            return Err(Falta::Otra(-12));
        }
        match self.ficheros.get(camino) {
            Some((b, _)) if b.len() > tope => Err(Falta::Otra(-11)),
            Some((b, fecha)) => Ok((b.clone(), (*fecha, b.len() as u64, 0))),
            None => Err(Falta::NoExiste),
        }
    }

    fn firma(&mut self, camino: &str) -> Option<Firma> {
        self.ficheros.get(camino).map(|(b, fecha)| (*fecha, b.len() as u64, 0))
    }
}

#[test]
fn sesion_sin_nada_es_castellano_y_no_busca_catalogos() {
    let mut d = Disco::default();
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idioma(), i.local.pais_nombre.as_str(), i.origen), ("español", "España", None));
    assert_eq!(d.pedidos, [TEXTO, PREFS, TEXTO_GUARDADO, PREFS_GUARDADAS, ZONA]);
    assert!(i.catalogo(&mut d, "prueba", 1, "español").is_none());
    // Ni una pregunta más: en castellano no se busca nada.
    assert_eq!(d.pedidos.len(), 5);
    assert!(!i.ha_cambiado(&mut d));
}

#[test]
fn sesion_en_ingles() {
    let mut d = Disco::ingles();
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idioma(), i.local.pais_nombre.as_str(), i.origen), ("english", "great_britain", Some(PREFS)));
    assert_eq!(i.local.cadena(cadenas::DIA_1), "Sunday");
    assert_eq!(i.local.moneda(150), "1,50 £");
    static P: Puesto = Puesto::nuevo();
    assert!(i.poner(&mut d, &P, "prueba", 1));
    assert_eq!(P.texto(1, "Abrir"), "Open");
    // Primero se mira en el cajón del programa.
    assert!(d.pedidos.contains(&"PROGDIR:Catalogs/english/prueba.catalog".to_string()));
    // El catálogo del cajón del programa gana al del sistema.
    d.poner("PROGDIR:Catalogs/english/prueba.catalog", catalogo::escribir("prueba.catalog", 1, 0, "", "english", Juego::Utf8, &[(1, "Open it")]));
    assert_eq!(i.catalogo(&mut d, "prueba", 1, "español").unwrap().texto(1), Some("Open it"));
    // Y si el del cajón del programa está roto o es de otra versión, vale el del sistema.
    d.poner("PROGDIR:Catalogs/english/prueba.catalog", b"FORM\0\0\0\x08CTLGroto".to_vec());
    assert_eq!(i.catalogo(&mut d, "prueba", 1, "español").unwrap().texto(1), Some("Open"));
    d.poner("PROGDIR:Catalogs/english/prueba.catalog", catalogo::escribir("prueba.catalog", 9, 0, "", "english", Juego::Utf8, &[(1, "Otra versión")]));
    assert_eq!(i.catalogo(&mut d, "prueba", 1, "español").unwrap().texto(1), Some("Open"));
    // Un programa sin catálogo, o con un nombre que quiere salir del cajón: los textos de siempre.
    assert!(!i.poner(&mut d, &P, "otro", 1));
    assert_eq!(P.texto(1, "Abrir"), "Abrir");
    let antes = d.pedidos.len();
    assert!(i.catalogo(&mut d, "../../Estado/x", 0, "español").is_none());
    assert!(i.catalogo(&mut d, "", 0, "español").is_none());
    assert_eq!(d.pedidos.len(), antes);
}

#[test]
fn sesion_con_idioma_que_no_existe_pasa_al_siguiente() {
    let mut d = Disco::ingles();
    d.poner(PREFS, Prefs { idiomas: vec!["klingon".into(), "english".into()], ..Prefs::default() }.escribir());
    let i = Idiomas::cargar(&mut d);
    assert_eq!(i.local.idioma(), "klingon");
    // No hay klingon: el catálogo y los días salen del inglés, que va detrás.
    assert_eq!(i.catalogo(&mut d, "prueba", 1, "español").unwrap().texto(1), Some("Open"));
    assert_eq!(i.local.cadena(cadenas::DIA_1), "Sunday");
    // Solo klingon: todo de siempre, en castellano.
    d.poner(PREFS, Prefs { idiomas: vec!["klingon".into()], ..Prefs::default() }.escribir());
    let i = Idiomas::cargar(&mut d);
    assert!(i.catalogo(&mut d, "prueba", 1, "español").is_none());
    assert_eq!(i.local.cadena(cadenas::DIA_1), "domingo");
    // Castellano delante del inglés: no se busca nada.
    d.poner(PREFS, Prefs { idiomas: vec!["español".into(), "english".into()], ..Prefs::default() }.escribir());
    let i = Idiomas::cargar(&mut d);
    let antes = d.pedidos.len();
    assert!(i.catalogo(&mut d, "prueba", 1, "español").is_none());
    assert_eq!((d.pedidos.len(), i.local.cadena(cadenas::DIA_1)), (antes, "domingo"));
}

#[test]
fn sesion_con_prefs_rotas_o_que_no_se_dejan_leer() {
    // Roto en ENV:, bueno en ENVARC:: manda el de ENV:, y como está roto, lo de serie.
    let mut d = Disco::ingles();
    let bueno = d.ficheros[PREFS].0.clone();
    d.poner(PREFS_GUARDADAS, bueno.clone());
    for roto in [b"basura".to_vec(), bueno[..400].to_vec(), vec![0xff; 894], Vec::new()] {
        d.poner(PREFS, roto);
        let i = Idiomas::cargar(&mut d);
        assert_eq!((i.local.idioma(), i.local.pais_nombre.as_str(), i.origen), ("español", "España", Some(PREFS)));
        assert!(i.catalogo(&mut d, "prueba", 1, "español").is_none());
    }
    // Enorme: ni se lee.
    d.poner(PREFS, vec![0; 1 << 20]);
    assert_eq!(Idiomas::cargar(&mut d).local.idioma(), "español");
    // Protegido: lo de serie.
    d.poner(PREFS, bueno.clone());
    d.protegidos.push(PREFS.into());
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idioma(), i.origen), ("español", Some(PREFS)));
    // Sin ENV:, el guardado.
    d.ficheros.remove(PREFS);
    d.protegidos.clear();
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idioma(), i.origen), ("english", Some(PREFS_GUARDADAS)));
    // Un catálogo protegido o enorme no rompe nada.
    d.protegidos.push("LOCALE:Catalogs/english/prueba.catalog".into());
    assert!(i.catalogo(&mut d, "prueba", 1, "español").is_none());
    d.protegidos.clear();
    d.poner("LOCALE:Catalogs/english/prueba.catalog", vec![0; catalogo::MAX_BYTES + 1]);
    assert!(i.catalogo(&mut d, "prueba", 1, "español").is_none());
}

#[test]
fn sesion_pais_desde_su_fichero() {
    // Un locale.prefs sin los datos del país (hecho a mano): se leen del .country, en minúsculas si hace falta.
    let mut d = Disco::default();
    d.poner(PREFS, Prefs { pais: "Great_Britain".into(), idiomas: vec![], gmt: 0, flags: 0, datos: None }.escribir());
    let gb = Pais { moneda: "£".into(), ..Pais::espana() };
    d.poner("LOCALE:Countries/great_britain.country", gb.escribir());
    let i = Idiomas::cargar(&mut d);
    assert_eq!(i.local.pais.moneda, "£");
    // Un .country roto, o que no está: España.
    d.poner("LOCALE:Countries/great_britain.country", b"FORM\0\0\0\x04PREF".to_vec());
    assert_eq!(Idiomas::cargar(&mut d).local.pais, Pais::espana());
    d.ficheros.remove("LOCALE:Countries/great_britain.country");
    assert_eq!(Idiomas::cargar(&mut d).local.pais, Pais::espana());
}

#[test]
fn sesion_se_entera_del_cambio() {
    let mut d = Disco::ingles();
    let i = Idiomas::cargar(&mut d);
    assert!(!i.ha_cambiado(&mut d));
    // Se reescribe (otra fecha): ha cambiado, y al cargar otra vez sale el idioma nuevo.
    d.poner(PREFS, Prefs { idiomas: vec!["español".into()], ..Prefs::default() }.escribir());
    assert!(i.ha_cambiado(&mut d));
    let i = Idiomas::cargar(&mut d);
    assert_eq!(i.local.idioma(), "español");
    assert!(!i.ha_cambiado(&mut d));
    // Se borra: ha cambiado (vuelve a lo de serie).
    d.ficheros.remove(PREFS);
    assert!(i.ha_cambiado(&mut d));
    let i = Idiomas::cargar(&mut d);
    assert!(i.origen.is_none() && !i.ha_cambiado(&mut d));
    // Aparece uno donde no había: ha cambiado. Y si aparece en ENV: habiendo salido de ENVARC:, también.
    d.poner(PREFS_GUARDADAS, Prefs::default().escribir());
    assert!(i.ha_cambiado(&mut d));
    let i = Idiomas::cargar(&mut d);
    assert!(!i.ha_cambiado(&mut d));
    d.poner(PREFS, Prefs::default().escribir());
    assert!(i.ha_cambiado(&mut d));
}

/// La zona horaria del sistema (`SYS:Prefs/zona`, la de la barra) manda sobre los minutos de `locale.prefs`, con su
/// cambio de hora. El momento de la prueba de `cambio` en QEMU: 2026-11-03 17:05:09 UTC.
#[test]
fn sesion_zona_del_sistema() {
    let noviembre = 1_793_725_509;
    // 2026-07-15 17:05:09 UTC: verano.
    let julio = 1_784_135_109;
    // Sin zona: Carla (Reino Unido, gmt 0) va en UTC; Ana (de serie, gmt -60), en UTC+1 todo el año.
    let mut d = Disco::ingles();
    let carla = Idiomas::cargar(&mut d);
    assert!(carla.local.zona.is_none());
    assert_eq!((carla.local.hora_corta(noviembre), carla.local.hora_corta(julio)), ("17:05".into(), "17:05".into()));
    let ana = Idiomas::cargar(&mut Disco::default());
    assert_eq!((ana.local.hora_corta(noviembre), ana.local.hora_corta(julio)), ("18:05".into(), "18:05".into()));
    // Con la zona de Madrid en el sistema: las dos en la hora de Madrid, UTC+1 en noviembre y UTC+2 en julio.
    d.poner(ZONA, b"Europe/Madrid\n".to_vec());
    let carla = Idiomas::cargar(&mut d);
    assert_eq!(carla.local.zona.map(|z| z.nombre), Some("Europe/Madrid"));
    assert_eq!(carla.local.fecha_hora_corta(noviembre), "03/11/2026 18:05");
    assert_eq!(carla.local.hora_larga(julio), "19:05:09");
    let mut d_ana = Disco::default();
    d_ana.poner(ZONA, b"Europe/Madrid\n".to_vec());
    let ana = Idiomas::cargar(&mut d_ana);
    assert_eq!((ana.local.fecha_hora_corta(noviembre), ana.local.fecha_hora_corta(julio)), ("03/11/2026 18:05".into(), "15/07/2026 19:05".into()));
    // El cambio de hora de 2026 en Madrid: el 29 de marzo, a la 01:00 UTC, las 02:00 pasan a ser las 03:00; el 25 de
    // octubre, a la 01:00 UTC, las 03:00 vuelven a ser las 02:00. Y el día cambia con la hora local.
    let marzo = 1_774_746_000;
    assert_eq!((ana.local.hora_larga(marzo - 1), ana.local.hora_larga(marzo)), ("01:59:59".into(), "03:00:00".into()));
    let octubre = 1_792_890_000;
    assert_eq!((ana.local.hora_larga(octubre - 1), ana.local.hora_larga(octubre)), ("02:59:59".into(), "02:00:00".into()));
    // 2026-07-15 22:00 UTC: en Madrid ya es el 16.
    assert_eq!((ana.local.fecha_corta(1_784_152_800 - 1), ana.local.fecha_corta(1_784_152_800)), ("15/07/2026".into(), "16/07/2026".into()));
    // Londres: UTC en noviembre (lo que esperaba la prueba de Carla) y UTC+1 en julio.
    d.poner(ZONA, b"Europe/London".to_vec());
    let carla = Idiomas::cargar(&mut d);
    assert_eq!((carla.local.fecha_hora_corta(noviembre), carla.local.hora_corta(julio)), ("03/11/2026 17:05".into(), "18:05".into()));
    // Antes de 1970 o en el límite: sin reventar.
    assert_eq!(carla.local.momento(i64::MIN).ano, 1);
    assert_eq!(carla.local.momento(i64::MAX).ano, 9999);
    assert_eq!(carla.local.momento(-1).segundo, 59);
}

/// Hostil: una zona que no existe y un fichero de zona roto, enorme o que no se deja leer no rompen nada: valen los
/// minutos de `locale.prefs` (de serie, UTC+1 todo el año).
#[test]
fn sesion_zona_rota_o_que_no_existe() {
    let julio = 1_784_135_109;
    let malos: [Vec<u8>; 9] = [
        b"Marte/Olympus\n".to_vec(),
        b"Europe/Madri".to_vec(),
        Vec::new(),
        b"\n\nEurope/Madrid".to_vec(),
        vec![0xff, 0xfe, 0x00, 0x41],
        b"Europe/Madrid\xc3".to_vec(),
        vec![0; 200],
        b"../../Estado/RAM/Ana/zona".to_vec(),
        [b"Europe/Madrid\n".as_slice(), &[b'#'; MAX_ZONA]].concat(),
    ];
    for malo in malos {
        let mut d = Disco::default();
        d.poner(ZONA, malo.clone());
        let i = Idiomas::cargar(&mut d);
        assert!(i.local.zona.is_none(), "{malo:?}");
        assert_eq!(i.local.hora_corta(julio), "18:05", "{malo:?}");
        assert!(!i.ha_cambiado(&mut d), "{malo:?}");
    }
    // Que no se deja leer: lo mismo, y no se toma por un cambio.
    let mut d = Disco::default();
    d.poner(ZONA, b"Europe/Helsinki\n".to_vec());
    d.protegidos.push(ZONA.into());
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.zona, i.local.hora_corta(julio)), (None, "18:05".into()));
    assert!(!i.ha_cambiado(&mut d));
    // Al cambiar el fichero, se nota; al quitarlo, también.
    d.protegidos.clear();
    d.poner(ZONA, b"Europe/Helsinki\n".to_vec());
    assert!(i.ha_cambiado(&mut d));
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.zona.map(|z| z.nombre), i.local.hora_corta(julio)), (Some("Europe/Helsinki"), "20:05".into()));
    assert!(!i.ha_cambiado(&mut d));
    d.ficheros.remove(ZONA);
    assert!(i.ha_cambiado(&mut d));
    // Al azar: cualquier contenido da una zona de la tabla o ninguna, sin reventar.
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..20_000 {
        let mut b = b"Europe/Madrid\n".to_vec();
        for _ in 0..2 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let k = (x % b.len() as u64) as usize;
            b[k] = (x >> 40) as u8;
        }
        let mut d = Disco::default();
        d.poner(ZONA, b);
        let i = Idiomas::cargar(&mut d);
        let _ = i.local.fecha_hora_corta((x >> 20) as i64 - (1 << 43));
    }
}

/// Ajustes en texto (S36): `ENV:Sys/locale` manda sobre el IFF; uno sin idiomas ni país deja paso al IFF de su capa; uno
/// roto manda (lo de serie); editado a mano vale; y lo que escribe `a_texto` se lee igual y no pierde comentarios.
#[test]
fn sesion_texto_manda() {
    let mut d = Disco::ingles();
    d.poner("LOCALE:Countries/españa.country", Pais::espana().escribir());
    // Solo el IFF (inglés): inglés.
    assert_eq!(Idiomas::cargar(&mut d).local.idioma(), "english");
    // Un texto con solo comentarios: sigue el IFF de su capa.
    d.poner(TEXTO, b"# nada\n".to_vec());
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idioma(), i.origen), ("english", Some(PREFS)));
    // El texto, a mano, en castellano: manda sobre el IFF inglés de la misma capa.
    d.poner(TEXTO, b"# a mano\nidiomas = espa\xc3\xb1ol, english\npais = espa\xc3\xb1a\n".to_vec());
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idioma(), i.local.pais_nombre.as_str(), i.origen), ("español", "españa", Some(TEXTO)));
    assert!(!i.ha_cambiado(&mut d));
    // Cambia el texto: se nota; cambia el IFF de detrás: no importa (manda el texto).
    d.poner(PREFS, Prefs { idiomas: vec!["klingon".into()], ..Prefs::default() }.escribir());
    assert!(!i.ha_cambiado(&mut d));
    d.poner(TEXTO, b"idiomas = english\n".to_vec());
    assert!(i.ha_cambiado(&mut d));
    // Roto (no es texto, o enorme): manda igual, y es lo de serie.
    for roto in [vec![0xff, 0xfe, b'x'], vec![b'#'; 70_000]] {
        d.poner(TEXTO, roto);
        let i = Idiomas::cargar(&mut d);
        assert_eq!((i.local.idioma(), i.origen), ("español", Some(TEXTO)));
    }
    // Un texto con idiomas malos: los malos no entran.
    d.poner(TEXTO, b"idiomas = ../../Prefs, english, :x, english\npais = a/b\n".to_vec());
    let i = Idiomas::cargar(&mut d);
    assert_eq!((i.local.idiomas.clone(), i.local.pais_nombre.as_str()), (vec![String::from("english")], "España"));
    // El guardado (ENVARC:) en texto vale si en ENV: no hay nada.
    d.ficheros.remove(TEXTO);
    d.ficheros.remove(PREFS);
    d.poner(TEXTO_GUARDADO, b"idiomas = english\n".to_vec());
    assert_eq!(Idiomas::cargar(&mut d).origen, Some(TEXTO_GUARDADO));
    // a_texto sobre uno a mano: sus comentarios se quedan y se lee igual; nuevo, con su cabecera.
    let p = Prefs { idiomas: vec!["english".into(), "español".into()], pais: "great_britain".into(), ..Prefs::default() };
    let t = p.a_texto(Some(b"# mio\npais = x\notra = 1\n"));
    assert_eq!(String::from_utf8(t.clone()).unwrap(), "# mio\npais = great_britain\notra = 1\nidiomas = english, español\n");
    let leido = Prefs::de_texto(&t).unwrap().unwrap();
    assert_eq!((leido.idiomas, leido.pais), (p.idiomas.clone(), p.pais.clone()));
    let nuevo = String::from_utf8(p.a_texto(None)).unwrap();
    assert!(nuevo.starts_with("# Idioma y país") && nuevo.ends_with("idiomas = english, español\npais = great_britain\n"));
    // La lista de la orden: comas, espacios, repetidos; y lo que no vale.
    assert_eq!(crate::prefs::lista_de_idiomas(" english ,español,,English").unwrap(), ["english", "español"]);
    assert_eq!(crate::prefs::lista_de_idiomas("english, ../x").err(), Some(String::from("../x")));
    assert!(crate::prefs::lista_de_idiomas(&(0..11).map(|i| format!("i{i}")).collect::<Vec<_>>().join(",")).is_err());
}
