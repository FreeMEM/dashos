//! Las pruebas de `reconocer`: el inventario de lo hecho a mano, la pregunta y su `Query`, el contador del `Nonce`, y
//! la respuesta de un servicio de mentira que hace lo mismo que el de Hangar (`servicio/src/api2.rs`, `identificar` y
//! `responder_texto`: la `Query` es la ruta con los parámetros, un espacio y el SHA-256 del cuerpo; la firma, sobre
//! `PKG-RESPONSE-2`, un byte 0 y el cuerpo, en `X-Pkg-Signature: <id> <firma>`), y sus trampas.

use super::*;
use crate::disco::memoria::Memoria;
use crate::firma::pruebas::{clave, firmar};
use crate::motor::pruebas::{Paq, disco};
use crate::motor::{cargar, poner_en_orden};
use alloc::vec;
use ed25519_dalek::SigningKey;

const URL: &str = "https://repo.example/pkg/v2/prueba";

fn elf(ver: &str, relleno: usize) -> Vec<u8> {
    let mut b = b"\x7fELF\x02\x01\x01".to_vec();
    b.resize(64 + relleno, 0);
    b.extend_from_slice(ver.as_bytes());
    b.push(0);
    b
}

fn hunk(ver: &str) -> Vec<u8> {
    let mut b = vec![0, 0, 3, 0xf3, 0, 0, 0, 0];
    b.extend_from_slice(ver.as_bytes());
    b.push(0);
    b
}

/// `disco()` (el repositorio de pruebas) con saludo instalado por Paq y, a mano, lo que hay que reconocer.
fn con_cosas_a_mano() -> Memoria {
    let mut m = disco();
    assert_eq!(Paq::nuevo().instalar(&mut m, &["saludo"]), Ok(1));
    // Un programa dentro del cajón de un paquete de Paq: no es de "a mano", no se manda.
    m.poner("Paquetes:Saludo/Prog", Some(&elf("$VER: Saludo 1.0", 0)));
    m.poner("Paquetes:Hecho/Hecho", Some(&elf("$VER: Hecho 2.1 (17.11.2026)", 10)));
    m.poner("Paquetes:Hecho/Hecho.info", Some(&elf("", 0)));
    m.poner("Paquetes:Hecho/LEEME", Some(b"$VER: Hecho 2.1\nUn texto, no un programa.\n"));
    m.poner("Paquetes:Hecho/Libs/hecho.library", Some(&hunk("$VER: hecho.library 3.0 (1.1.2026)")));
    m.poner("Paquetes:Viejo/C/viejo", Some(&hunk("sin cadena de versión")));
    m.poner("Paquetes:Hondo/a/b/c/dentro", Some(&elf("", 0)));
    m.poner("Paquetes:Hondo/a/b/c/d/demasiado", Some(&elf("", 0)));
    m.poner("Paquetes:Raro/con\ttabulador", Some(&elf("", 0)));
    m.poner("Paquetes:suelto", Some(&elf("$VER: Suelto v4.2", 0)));
    m
}

fn nombres(inv: &Inventario) -> Vec<&str> {
    inv.ficheros.iter().map(|f| f.ruta.as_str()).collect()
}

#[test]
fn cadenas_de_version() {
    assert_eq!(version_de(b"xx$VER: Hecho 2.1 (17.11.2026)\0"), Some(("Hecho".into(), "2.1".into())));
    assert_eq!(version_de(b"$VER: reqtools.library 38.1390 (8.6.98)\n"), Some(("reqtools".into(), "38.1390".into())));
    assert_eq!(version_de(b"$VER: Prog v1.2"), Some(("Prog".into(), "1.2".into())));
    // La primera que no se entiende no cuenta; se mira la siguiente.
    assert_eq!(version_de(b"$VER: \0$VER: Prog beta\0$VER: Prog 3"), Some(("Prog".into(), "3".into())));
    assert_eq!(version_de(b"$VER: Prog"), None);
    assert_eq!(version_de(b"$VER:"), None);
    assert_eq!(version_de(b"nada"), None);
    // Latin-1 (de Amiga) y cortada en un byte de control.
    assert_eq!(version_de(b"$VER: Espa\xf1a 1.0"), Some(("España".into(), "1.0".into())));
    assert_eq!(version_de(b"$VER: Prog\x0a1.0"), None);
    // Larguísima: solo se miran 120 bytes.
    let mut larga = b"$VER: ".to_vec();
    larga.extend(core::iter::repeat_n(b'a', 1000));
    larga.extend_from_slice(b" 1.0");
    assert_eq!(version_de(&larga), None);
    assert!(es_ejecutable(b"\x7fELF...") && es_ejecutable(&[0, 0, 3, 0xf3]) && !es_ejecutable(b"Echo hola") && !es_ejecutable(b""));
}

#[test]
fn inventario_de_lo_hecho_a_mano() {
    let mut m = con_cosas_a_mano();
    let (e, _) = poner_en_orden(&mut m, &Nivel::de_todos()).unwrap();
    let inv = inventario(&mut m, &Nivel::de_todos(), &e).unwrap();
    let mut rutas = nombres(&inv);
    rutas.sort();
    assert_eq!(rutas, ["Paquetes:Hecho/Hecho", "Paquetes:Hecho/Libs/hecho.library", "Paquetes:Hondo/a/b/c/dentro", "Paquetes:Viejo/C/viejo", "Paquetes:suelto"]);
    assert_eq!((inv.saltados, inv.cortado), (1, false));
    let h = inv.ficheros.iter().find(|f| f.fichero == "Hecho").unwrap();
    assert_eq!((h.tamano, h.nombre.as_str(), h.version.as_str()), (elf("$VER: Hecho 2.1 (17.11.2026)", 10).len() as u64, "Hecho", "2.1"));
    let l = inv.ficheros.iter().find(|f| f.fichero == "hecho.library").unwrap();
    assert_eq!((l.nombre.as_str(), l.version.as_str()), ("hecho", "3.0"));
    let v = inv.ficheros.iter().find(|f| f.fichero == "viejo").unwrap();
    assert_eq!((v.nombre.as_str(), v.version.as_str()), ("", ""));
    // El cuerpo: una línea por fichero, con tabuladores.
    assert!(cuerpo(&inv.ficheros).lines().any(|x| x == format!("Hecho\t{}\tHecho\t2.1", h.tamano)));
    assert!(cuerpo(&inv.ficheros).lines().any(|x| x == format!("viejo\t{}\t\t", hunk("sin cadena de versión").len())));
    // Lo de cada uno, igual en Home:Paquetes (vacío: nada).
    let mut m = Memoria::nueva(&["Home:"]);
    assert!(inventario(&mut m, &Nivel::del_usuario(), &Estado::default()).unwrap().ficheros.is_empty());
    m.poner("Home:Paquetes/Mio/mio", Some(&elf("$VER: Mio 1.0", 0)));
    assert_eq!(nombres(&inventario(&mut m, &Nivel::del_usuario(), &Estado::default()).unwrap()), ["Home:Paquetes/Mio/mio"]);
}

#[test]
fn inventario_con_limites() {
    // Más programas de los que lee el servicio: se mandan los primeros y se dice.
    let mut m = Memoria::nueva(&["Paquetes:"]);
    for i in 0..MAX_INVENTARIO + 10 {
        m.poner(&format!("Paquetes:Muchos/p{i}"), Some(b"\x7fELF"));
    }
    let inv = inventario(&mut m, &Nivel::de_todos(), &Estado::default()).unwrap();
    assert_eq!((inv.ficheros.len(), inv.cortado), (MAX_INVENTARIO, true));
    // Uno más grande de lo que se mira: no se manda (y se cuenta).
    let mut m = Memoria::nueva(&["Paquetes:"]);
    let mut grande = elf("$VER: Grande 1.0", 0);
    grande.resize(MAX_VER + 1, 0);
    m.poner("Paquetes:Grande/grande", Some(&grande));
    let inv = inventario(&mut m, &Nivel::de_todos(), &Estado::default()).unwrap();
    assert_eq!((inv.ficheros.len(), inv.saltados), (0, 1));
}

fn repo(url: &str) -> Repo {
    Repo { nombre: "prueba".into(), url: url.into(), prioridad: 20, protegido: false }
}

#[test]
fn la_pregunta_y_su_query() {
    let fs = vec![FicheroDisco { fichero: "Hecho".into(), tamano: 7, nombre: "Hecho".into(), version: "2.1".into(), ruta: "Paquetes:Hecho/Hecho".into() }];
    let n = nonce(3, &[0xab; 8]);
    assert_eq!(n, "0000000000000003abababababababab");
    let p = pregunta(&repo(URL), "aarch64-dashos", &n, &fs);
    assert_eq!(p.url, format!("{URL}/aarch64-dashos/identify?nonce={n}"));
    assert_eq!(p.cuerpo, b"Hecho\t7\tHecho\t2.1\n");
    assert_eq!(p.query, format!("/pkg/v2/prueba/aarch64-dashos/identify?nonce={n} {}", huella::hex(&huella::sha256(b"Hecho\t7\tHecho\t2.1\n"))));
    // Con la barra del final en la lista, igual.
    assert_eq!(pregunta(&repo(&format!("{URL}/")), "aarch64-dashos", &n, &fs).url, p.url);
    assert_eq!(ruta_de_url("http://h:8080/a/b?c=d"), "/a/b?c=d");
    assert_eq!(ruta_de_url("https://h"), "/");
    assert_eq!(ruta_de_url("/ya/es/ruta"), "/ya/es/ruta");
}

#[test]
fn el_nonce_sube_y_no_baja() {
    let mut m = Memoria::nueva(&["Paquetes:"]);
    m.poner("Paquetes:Paq/x", Some(b""));
    let n = Nivel::de_todos();
    let az = [0u8; 8];
    assert_eq!(siguiente_nonce(&mut m, &n, &az).unwrap(), nonce(1, &az));
    assert_eq!(siguiente_nonce(&mut m, &n, &az).unwrap(), nonce(2, &az));
    // Un corte entre escribir el nuevo y cambiarle el nombre: queda `Nonce.nuevo` y se usa (el más alto).
    m.poner("Paquetes:Paq/Nonce.nuevo", Some(b"10\n"));
    assert_eq!(siguiente_nonce(&mut m, &n, &az).unwrap(), nonce(11, &az));
    assert!(m.ver("Paquetes:Paq/Nonce.nuevo").is_none());
    // Cortes de luz en cada escritura: el siguiente que sale bien nunca repite uno dado.
    for corte in 0..4 {
        let mut c = m.clone();
        c.escrituras = 0;
        c.corte = Some(corte);
        let _ = siguiente_nonce(&mut c, &n, &az);
        c.corte = None;
        assert!(siguiente_nonce(&mut c, &n, &az).unwrap() > nonce(11, &az), "corte {corte}");
    }
    // Uno estropeado cuenta como 0; el azar de detrás evita que se repita (como en el cliente de Hangar).
    m.poner("Paquetes:Paq/Nonce", Some(b"basura"));
    let _ = siguiente_nonce(&mut m, &n, &[1; 8]).unwrap();
}

/// Las trampas del servicio de mentira.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trampa {
    Ninguna,
    /// Lo que se cambia antes de firmar (la firma vale, lo de dentro no).
    OtroNonce,
    /// Contesta a otra pregunta (otro cuerpo): una respuesta vieja repetida.
    OtraQuery,
    OtroRepo,
    OtraArquitectura,
    SerieVieja,
    FaltaUnaLinea,
    SobraUnaLinea,
    OtroFichero,
    EstadoRaro,
    SinId,
    RutaQueSeSale,
    QueryRepetida,
    /// Lo que se cambia después de firmar.
    UnBitCambiado,
    ConLaClaveDelIndice,
    SinFirma,
    FirmaConSalto,
    Enorme,
}

struct Servicio {
    clave: SigningKey,
    serie: u64,
    trampa: Trampa,
    /// Lo que le llegó: (url, cuerpo).
    preguntas: Vec<(String, Vec<u8>)>,
}

impl Servicio {
    fn nuevo(trampa: Trampa) -> Servicio {
        Servicio { clave: clave(2), serie: 6, trampa, preguntas: Vec::new() }
    }

    /// Como `reconocer_lineas` del servicio: por el nombre y el tamaño de un fichero de un paquete del catálogo.
    fn linea(fichero: &str, tamano: &str) -> String {
        let hecho = elf("$VER: Hecho 2.1 (17.11.2026)", 10).len().to_string();
        let libreria = hunk("$VER: hecho.library 3.0 (1.1.2026)").len().to_string();
        match (fichero, tamano) {
            ("Hecho", t) if t == hecho => format!("{fichero}\tigual\t77\tHecho\t2.1\t2.1\tHecho/Hecho"),
            ("hecho.library", t) if t == libreria => format!("{fichero}\tnueva\t77\tHecho\t2.0\t2.1\tHecho/Libs/hecho.library"),
            ("suelto", _) => format!("{fichero}\tlocal-mas-nueva\t12\tSuelto\t4.2\t4.0\t"),
            _ => format!("{fichero}\tdesconocido\t\t\t\t\t"),
        }
    }
}

impl Red for Servicio {
    fn enviar(&mut self, url: &str, cuerpo: &[u8], tope: usize) -> Result<Respuesta, String> {
        self.preguntas.push((url.to_string(), cuerpo.to_vec()));
        let t = self.trampa;
        let nonce = url.split_once("nonce=").map(|x| x.1).unwrap_or("");
        let crudo: &[u8] = if t == Trampa::OtraQuery { b"Otro\t1\t\t\n" } else { cuerpo };
        let query = format!("{} {}", ruta_de_url(url), huella::hex(&huella::sha256(crudo)));
        let (repo, arq) = (if t == Trampa::OtroRepo { "otro" } else { "prueba" }, if t == Trampa::OtraArquitectura { "m68k-amigaos" } else { "aarch64-dashos" });
        let serie = if t == Trampa::SerieVieja { 4 } else { self.serie };
        let nonce = if t == Trampa::OtroNonce { "00000000000000000000000000000000" } else { nonce };
        let mut lineas: Vec<String> = core::str::from_utf8(cuerpo).unwrap().lines().map(|l| {
            let p: Vec<&str> = l.split('\t').collect();
            Servicio::linea(p[0], p[1])
        }).collect();
        match t {
            Trampa::FaltaUnaLinea => {
                lineas.pop();
            }
            Trampa::SobraUnaLinea => lineas.push("Extra\tdesconocido\t\t\t\t\t".into()),
            Trampa::OtroFichero => lineas[0] = Servicio::linea("Cambiado", "1"),
            Trampa::EstadoRaro => lineas[0] = "Hecho\tinstalalo-ya\t77\tHecho\t2.1\t2.1\tHecho/Hecho".into(),
            Trampa::SinId => lineas[0] = "Hecho\tigual\t\tHecho\t2.1\t2.1\tHecho/Hecho".into(),
            Trampa::RutaQueSeSale => lineas[0] = "Hecho\tigual\t77\tHecho\t2.1\t2.1\t../../SYS/C/Hecho".into(),
            _ => {}
        }
        let mut texto = format!("PKG-RESPONSE 2\nRepository: {repo}\nArchitecture: {arq}\nSerial: {serie}\nQuery: {query}\nNonce: {nonce}\n");
        if t == Trampa::QueryRepetida {
            texto += "Query: /otra\n";
        }
        texto.push('\n');
        for l in lineas {
            texto += &l;
            texto.push('\n');
        }
        if t == Trampa::Enorme {
            texto += &"x".repeat(MAX_RESPUESTA);
        }
        let k = if t == Trampa::ConLaClaveDelIndice { clave(1) } else { self.clave.clone() };
        let linea_sig = firmar(&k, firma::RESPUESTA, texto.as_bytes());
        let mut firma = linea_sig.trim_end().strip_prefix("ed25519 ").unwrap().to_string();
        let mut texto = texto.into_bytes();
        match t {
            Trampa::UnBitCambiado => {
                let i = texto.len() - 3;
                texto[i] ^= 1;
            }
            Trampa::SinFirma => firma.clear(),
            Trampa::FirmaConSalto => firma = format!("{firma}\ned25519 {firma}"),
            _ => {}
        }
        if texto.len() > tope && t != Trampa::Enorme {
            return Err("demasiado grande".into());
        }
        Ok(Respuesta { cuerpo: texto, firma })
    }
}

/// Las claves de `disco()` (la de índice, clave(1)) más la de respuestas (clave(2)), y la lista con el repositorio en
/// la red.
fn claves_y_repo() -> (Vec<Clave>, Repo) {
    let t = |k: &SigningKey, uso: &str| format!("Repository: prueba\nUse: {uso}\nKey: {}\n", huella::hex(&k.verifying_key().to_bytes()));
    let claves = vec![config::leer_clave("prueba", &t(&clave(1), "index")).unwrap(), config::leer_clave("respuestas", &t(&clave(2), "response")).unwrap()];
    (claves, repo(URL))
}

fn reconocer_con(m: &mut Memoria, s: &mut Servicio, vistas: &[(String, u64)]) -> Result<Reconocimiento, Fallo> {
    let n = Nivel::de_todos();
    let (e, _) = poner_en_orden(m, &n).unwrap();
    let (claves, r) = claves_y_repo();
    reconocer(m, &n, &e, &r, &claves, vistas, s, &[7; 8])
}

#[test]
fn reconoce_con_el_servicio() {
    let mut m = con_cosas_a_mano();
    let mut s = Servicio::nuevo(Trampa::Ninguna);
    let r = reconocer_con(&mut m, &mut s, &[("prueba".into(), 5)]).unwrap();
    assert_eq!(r.serie, 6);
    assert_eq!(s.preguntas.len(), 1);
    let (url, cuerpo) = &s.preguntas[0];
    assert_eq!(url, &format!("{URL}/aarch64-dashos/identify?nonce={}", nonce(1, &[7; 8])));
    assert_eq!(cuerpo, &self::cuerpo(&r.inventario.ficheros).into_bytes());
    let de = |f: &str| r.reconocidos.iter().find(|x| x.fichero.fichero == f).unwrap();
    assert_eq!((de("Hecho").situacion, de("Hecho").id, de("Hecho").en_paquete.as_str()), (Situacion::Igual, Some(77), "Hecho/Hecho"));
    assert_eq!((de("hecho.library").situacion, de("hecho.library").local.as_str()), (Situacion::Nueva, "2.0"));
    assert_eq!((de("viejo").situacion, de("viejo").id, de("viejo").nombre.as_str()), (Situacion::Desconocido, None, ""));
    assert_eq!(de("suelto").situacion, Situacion::LocalMasNueva);
    let c = candidatos(&Nivel::de_todos(), &r.reconocidos);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].cajon.as_str(), c[0].id, c[0].ficheros), ("Hecho", 77, 2));
    assert_eq!((c[1].cajon.as_str(), c[1].id, c[1].nombre.as_str()), ("", 12, "Suelto"));
    // No se apunta nada como instalado: la generación sigue siendo la de saludo.
    assert_eq!(cargar(&mut m, &Nivel::de_todos()).unwrap().generacion.numero, 1);
    // La siguiente pregunta lleva otro Nonce.
    let _ = reconocer_con(&mut m, &mut s, &[]).unwrap();
    assert!(s.preguntas[1].0.ends_with(&nonce(2, &[7; 8])));
}

#[test]
fn respuestas_con_trampa() {
    use Trampa::*;
    for (t, dice) in [
        (OtroNonce, "no es la de esta pregunta"),
        (OtraQuery, "no es la de esta pregunta"),
        (OtroRepo, "de otro repositorio"),
        (OtraArquitectura, "otra arquitectura"),
        (SerieVieja, "más vieja que la última vista"),
        (FaltaUnaLinea, "líneas para"),
        (SobraUnaLinea, "líneas para"),
        (OtroFichero, "habla de Cambiado"),
        (EstadoRaro, "no se entiende"),
        (SinId, "Id que no se entiende"),
        (RutaQueSeSale, "ruta que no vale"),
        (QueryRepetida, "tiene dos"),
        (UnBitCambiado, "la firma de la respuesta no vale"),
        (ConLaClaveDelIndice, "no es la de respuestas"),
        (SinFirma, "no viene firmada"),
        (FirmaConSalto, "no viene firmada"),
        (Enorme, "demasiado grande"),
    ] {
        let mut m = con_cosas_a_mano();
        let mut s = Servicio::nuevo(t);
        let r = reconocer_con(&mut m, &mut s, &[("prueba".into(), 5)]);
        assert!(matches!(&r, Err(Fallo::Indice(x)) if x.contains(dice)), "{t:?}: {:?}", r.map(|x| x.reconocidos.len()));
    }
}

#[test]
fn sin_red_sin_clave_o_sin_nada_que_preguntar() {
    let n = Nivel::de_todos();
    let mut m = con_cosas_a_mano();
    let (e, _) = poner_en_orden(&mut m, &n).unwrap();
    let (claves, r) = claves_y_repo();
    let mut s = Servicio::nuevo(Trampa::Ninguna);
    // El repositorio de pruebas está en un disco: es estático y no contesta.
    let r_disco = reconocer(&mut m, &n, &e, &repo("SYS:Repos/prueba"), &claves, &[], &mut s, &[0; 8]);
    assert!(matches!(&r_disco, Err(Fallo::Plan(x)) if x[0].contains("estático")));
    // Sin la clave de respuestas (solo la de índice): no se pregunta.
    let r_sin = reconocer(&mut m, &n, &e, &r, &claves[..1], &[], &mut s, &[0; 8]);
    assert!(matches!(&r_sin, Err(Fallo::Indice(x)) if x.contains("clave de respuestas")));
    assert!(s.preguntas.is_empty());
    // Un servicio que no contesta.
    struct Caido;
    impl Red for Caido {
        fn enviar(&mut self, _: &str, _: &[u8], _: usize) -> Result<Respuesta, String> {
            Err("no hay red".into())
        }
    }
    let r_caido = reconocer(&mut m, &n, &e, &r, &claves, &[], &mut Caido, &[0; 8]);
    assert!(matches!(&r_caido, Err(Fallo::Indice(x)) if x.contains("no contesta: no hay red")));
    // Nada hecho a mano: no se pregunta.
    let mut m = disco();
    assert_eq!(Paq::nuevo().instalar(&mut m, &["saludo"]), Ok(1));
    let (e, _) = poner_en_orden(&mut m, &n).unwrap();
    let x = reconocer(&mut m, &n, &e, &r, &claves, &[], &mut s, &[0; 8]).unwrap();
    assert!(x.reconocidos.is_empty() && s.preguntas.is_empty());
}
