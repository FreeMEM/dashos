//! Pruebas de PC de los fondos (rama `fondos`), con las hostiles: imágenes raras, transiciones a ráfagas, paletas mal
//! hechas, ficheros de ajustes rotos, medidas absurdas.

use super::animado::{self, Animado};
use super::ciclos::{self, Ciclos, Tramo};
use super::ritmo::Ritmo;
use super::transicion::{Clase, Lado, Transicion};
use super::*;
use crate::png::Indexada;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

fn imagen(w: usize, h: usize) -> Imagen {
    let px = (0..w * h).map(|i| 0xff00_0000 | (i as u32).wrapping_mul(2654435761) & 0xff_ffff).collect();
    Imagen { ancho: w, alto: h, px }
}

/// Que todo píxel que cambió de `antes` a `despues` caiga en algún rectángulo de `danos`.
fn cubre(antes: &[u32], despues: &[u32], w: usize, danos: &[Rect]) -> bool {
    antes.iter().zip(despues).enumerate().all(|(i, (a, d))| a == d || danos.iter().any(|r| r.contiene((i % w) as i32, (i / w) as i32)))
}

#[test]
fn marco_comprueba_sus_trozos() {
    let mut a = vec![0u32; 10 * 4];
    let mut b = vec![0u32; 10 * 3];
    assert!(Marco::nuevo(10, 7, 4, vec![&mut a[..], &mut b[..]]).is_some());
    let mut a = vec![0u32; 10 * 4];
    let mut b = vec![0u32; 10 * 2];
    // El último trozo, corto: no vale.
    assert!(Marco::nuevo(10, 7, 4, vec![&mut a[..], &mut b[..]]).is_none());
    let mut a = vec![0u32; 40];
    assert!(Marco::nuevo(0, 7, 4, vec![&mut a[..]]).is_none());
    assert!(Marco::nuevo(10, 4, 0, vec![]).is_none());
    // Por trozos sale lo mismo que entero.
    let img = imagen(37, 23);
    for ajuste in Ajuste::TODOS {
        let mut entero = vec![0u32; 64 * 41];
        pintar_marco(&img, ajuste, 0xff10_2030, &mut Marco::entero(64, 41, &mut entero).unwrap());
        let mut t: Vec<Vec<u32>> = (0..6).map(|k| vec![0u32; 64 * 8.min(41 - k * 8)]).collect();
        let trozos: Vec<&mut [u32]> = t.iter_mut().map(|v| &mut v[..]).collect();
        let mut m = Marco::nuevo(64, 41, 8, trozos).unwrap();
        pintar_marco(&img, ajuste, 0xff10_2030, &mut m);
        for y in 0..41 {
            assert_eq!(m.fila_de(y), &entero[y * 64..(y + 1) * 64], "{:?} fila {}", ajuste, y);
        }
    }
}

#[test]
fn ajustes_con_imagenes_raras() {
    let fondo = 0xff10_2030;
    for (iw, ih) in [(1, 1), (1, 900), (900, 1), (64, 41), (3000, 2000), (7, 5)] {
        let img = imagen(iw, ih);
        for (w, h) in [(1, 1), (64, 41), (200, 37), (37, 200)] {
            for ajuste in Ajuste::TODOS {
                let mut px = vec![0u32; w * h];
                pintar_marco(&img, ajuste, fondo, &mut Marco::entero(w, h, &mut px).unwrap());
                assert!(px.iter().all(|p| p >> 24 == 0xff), "{:?} {}x{} en {}x{}: sin pintar", ajuste, iw, ih, w, h);
            }
        }
    }
    // Encajar deja bandas del color; centrar una pequeña también; mosaico repite.
    let img = imagen(10, 10);
    let mut px = vec![0u32; 40 * 20];
    pintar_marco(&img, Ajuste::Encajar, fondo, &mut Marco::entero(40, 20, &mut px).unwrap());
    assert_eq!(px[0], fondo);
    assert_ne!(px[20 + 10 * 40], fondo);
    pintar_marco(&img, Ajuste::Centrar, fondo, &mut Marco::entero(40, 20, &mut px).unwrap());
    assert_eq!(px[0], fondo);
    assert_eq!(px[15 + 5 * 40], img.px[0]);
    pintar_marco(&img, Ajuste::Mosaico, fondo, &mut Marco::entero(40, 20, &mut px).unwrap());
    assert_eq!(px[0], px[10]);
    assert_eq!(px[3 + 2 * 40], px[33 + 12 * 40]);
    // Una imagen vacía (0 × 0): el color, sin pánico.
    let vacia = Imagen { ancho: 0, alto: 0, px: Vec::new() };
    pintar_marco(&vacia, Ajuste::Encajar, fondo, &mut Marco::entero(40, 20, &mut px).unwrap());
    assert!(px.iter().all(|&p| p == fondo));
    for a in Ajuste::TODOS {
        assert_eq!(Ajuste::de_nombre(a.nombre()), Some(a));
    }
    assert_eq!(Ajuste::de_nombre("LLENAR "), Some(Ajuste::Llenar));
    assert_eq!(Ajuste::de_nombre("rellenar"), None);
}

fn dos(w: usize, h: usize) -> (Vec<u32>, Vec<u32>) {
    (vec![0xff00_0000; w * h], (0..w * h).map(|i| 0xff00_0000 | (i as u32).wrapping_mul(7919) & 0xff_ffff).collect())
}

#[test]
fn transiciones_acaban_en_lo_nuevo_y_dicen_lo_que_cambian() {
    let (w, h) = (97, 61);
    let clases = [
        Clase::Ninguna,
        Clase::Fundido,
        Clase::Barrido(Lado::Izquierda),
        Clase::Barrido(Lado::Derecha),
        Clase::Barrido(Lado::Arriba),
        Clase::Barrido(Lado::Abajo),
        Clase::Circulo(30, 20),
        Clase::Circulo(-500, 9000),
        Clase::Circulo(96, 60),
        Clase::Disolver,
    ];
    for clase in clases {
        let (mut v, mut n) = dos(w, h);
        let nueva = Marco::entero(w, h, &mut n).unwrap();
        let mut t = Transicion::nueva(clase, 700, w, h);
        let mut ms = 0;
        while !t.terminada() {
            ms += 33;
            let antes = v.clone();
            let danos = {
                let mut vista = Marco::entero(w, h, &mut v).unwrap();
                t.paso(ms, &mut vista, &nueva)
            };
            assert!(cubre(&antes, &v, w, &danos), "{:?} en {} ms: cambió fuera de los daños", clase, ms);
            assert!(ms < 2000, "{:?} no acaba", clase);
        }
        // Pasos de más no hacen nada.
        let mut vista = Marco::entero(w, h, &mut v).unwrap();
        assert!(t.paso(ms + 100, &mut vista, &nueva).is_empty());
        drop(nueva);
        assert_eq!(v, n, "{:?}: no acaba en lo nuevo", clase);
    }
}

#[test]
fn barrido_y_circulo_tocan_poco() {
    // A 1920 × 1080, un paso del círculo o del barrido no daña toda la pantalla.
    let (w, h) = (1920, 1080);
    let (mut v, mut n) = dos(w, h);
    let nueva = Marco::entero(w, h, &mut n).unwrap();
    for clase in [Clase::Circulo(960, 540), Clase::Barrido(Lado::Izquierda)] {
        let mut t = Transicion::nueva(clase, 1000, w, h);
        let mut vista = Marco::entero(w, h, &mut v).unwrap();
        t.paso(400, &mut vista, &nueva);
        let danos = t.paso(433, &mut vista, &nueva);
        let area: i64 = danos.iter().map(|r| r.w as i64 * r.h as i64).sum();
        assert!(area < (w * h) as i64 / 3, "{:?}: {} píxeles dañados en un paso", clase, area);
    }
}

#[test]
fn transiciones_a_rafagas() {
    // Hostil: cambiar de fondo a ráfagas. Cada transición empieza sobre lo que dejó a medias la anterior; la última
    // acaba en su fondo.
    let (w, h) = (64, 48);
    let mut v = vec![0xff00_0000u32; w * h];
    let mut fondos: Vec<Vec<u32>> = (0..40u32).map(|k| vec![0xff00_0000 | k.wrapping_mul(0x01_0305); w * h]).collect();
    let n = fondos.len();
    for (k, f) in fondos.iter_mut().enumerate() {
        let nueva = Marco::entero(w, h, f).unwrap();
        let clase = [Clase::Fundido, Clase::Circulo(k as i32, 3), Clase::Disolver, Clase::Barrido(Lado::Abajo)][k % 4];
        let mut t = Transicion::nueva(clase, 500, w, h);
        let mut vista = Marco::entero(w, h, &mut v).unwrap();
        if k + 1 < n {
            t.paso(40, &mut vista, &nueva);
        } else {
            let mut ms = 0;
            while !t.terminada() {
                ms += 50;
                t.paso(ms, &mut vista, &nueva);
            }
        }
    }
    assert_eq!(v, fondos[n - 1]);
    // Medidas que no casan: no hace nada y se da por acabada.
    let (mut a, mut b) = (vec![0u32; 10 * 10], vec![0u32; 12 * 10]);
    let mut t = Transicion::nueva(Clase::Fundido, 500, 10, 10);
    let nueva = Marco::entero(12, 10, &mut b).unwrap();
    assert!(t.paso(100, &mut Marco::entero(10, 10, &mut a).unwrap(), &nueva).is_empty());
    assert!(t.terminada());
    assert_eq!(Clase::de_nombre("círculo", (1, 2), 0), Some(Clase::Circulo(1, 2)));
    assert_eq!(Clase::de_nombre("vaya", (1, 2), 0), None);
    for k in 0..12 {
        assert!(Clase::de_nombre("azar", (0, 0), k).is_some());
    }
}

#[test]
fn seno_de_tabla() {
    assert_eq!(seno(0), 0);
    assert_eq!(seno(1024), 1024);
    assert_eq!(seno(2048), 0);
    assert_eq!(seno(3072), -1024);
    assert!((seno(512) - 724).abs() <= 2);
    assert_eq!(seno(-1024), -1024);
    assert_eq!(seno(4096 * 1000 + 1024), 1024);
}

/// Un fondo animado, muchos cuadros: lo que cambia cae en los daños, y lo que no tapa ninguna partícula es la base.
fn probar_animado(a: &mut dyn Animado, w: usize, h: usize) {
    let mut base = vec![0u32; w * h];
    let mut b = Marco::entero(w, h, &mut base).unwrap();
    if a.tiene_base() {
        a.pintar_base(&mut b);
    } else {
        pintar_marco(&imagen(50, 30), Ajuste::Llenar, 0, &mut b);
    }
    let mut v = base.clone();
    let mut ms = 0u64;
    for k in 0..120 {
        // Tiempos raros: saltos enormes, hacia atrás y repetidos.
        ms = match k {
            40 => ms + 3_600_000,
            41 => ms.saturating_sub(5000),
            42 => ms,
            _ => ms + a.intervalo_ms() as u64,
        };
        let antes = v.clone();
        let danos = a.paso(ms, &Marco::entero(w, h, &mut base.clone()).unwrap(), &mut Marco::entero(w, h, &mut v).unwrap());
        assert!(cubre(&antes, &v, w, &danos), "{}: cambió fuera de los daños en el cuadro {}", a.nombre(), k);
        assert!(danos.iter().all(|r| r.x >= 0 && r.y >= 0 && r.x + r.w <= w as i32 && r.y + r.h <= h as i32), "{}: daño fuera", a.nombre());
    }
    // Tras olvidar y repintar la base, sigue sin salirse.
    a.olvidar();
    v.copy_from_slice(&base);
    let danos = a.paso(ms + 100, &Marco::entero(w, h, &mut base.clone()).unwrap(), &mut Marco::entero(w, h, &mut v).unwrap());
    assert!(cubre(&base, &v, w, &danos));
    assert!(a.intervalo_ms() >= 16);
}

#[test]
fn animados_no_se_salen() {
    for nombre in animado::NOMBRES {
        for (w, h) in [(320, 200), (1, 1), (3, 500), (500, 3)] {
            let mut a = animado::crear(nombre, w, h).unwrap();
            probar_animado(a.as_mut(), w, h);
        }
        assert!(animado::crear(nombre, 0, 10).is_none());
        assert!(animado::crear(nombre, 100_000, 10).is_none());
        assert!(animado::crear(nombre, 64, 64).unwrap().shader().is_some(), "{} sin su versión para la GPU", nombre);
    }
    assert!(animado::crear("fuego", 64, 64).is_none());
}

#[test]
fn particulas_tocan_poco_a_fhd() {
    for nombre in ["luciernagas", "nieve"] {
        let (w, h) = (1920, 1080);
        let mut a = animado::crear(nombre, w, h).unwrap();
        let mut base = vec![0u32; w * h];
        a.pintar_base(&mut Marco::entero(w, h, &mut base).unwrap());
        let mut v = base.clone();
        let mut area = 0i64;
        for k in 1..=20 {
            let danos = a.paso(k * 50, &Marco::entero(w, h, &mut base).unwrap(), &mut Marco::entero(w, h, &mut v).unwrap());
            area += danos.iter().map(|r| r.w as i64 * r.h as i64).sum::<i64>();
        }
        assert!(area / 20 < 100_000, "{}: {} píxeles por cuadro", nombre, area / 20);
    }
}

fn indexada(w: usize, h: usize, n: usize) -> Indexada {
    Indexada {
        ancho: w,
        alto: h,
        indices: (0..w * h).map(|i| (i % 64) as u8).collect(),
        paleta: (0..n as u32).map(|i| 0xff00_0000 | i * 0x01_0101).collect(),
        textos: Vec::new(),
    }
}

#[test]
fn tramos_de_ciclos_hostiles() {
    let (t, malos) = ciclos::leer_tramos("32-47 8; 48-55 2.5 atras\n# un comentario\n0-3 0,5");
    assert_eq!(malos, 0);
    assert_eq!(t, vec![Tramo { desde: 32, hasta: 47, ritmo: 800, atras: false }, Tramo { desde: 48, hasta: 55, ritmo: 250, atras: true }, Tramo { desde: 0, hasta: 3, ritmo: 50, atras: false }]);
    for malo in ["47-32 8", "5-5 1", "0-300 1", "1-2", "1-2 0", "1-2 61", "1-2 abc", "1-2 1 de lado", "-1-2 1", "1-2 1.234", "1-2 1 atras sobra", "x", "1-2 99999999999"] {
        let (t, m) = ciclos::leer_tramos(malo);
        assert!(t.is_empty() && m == 1, "{:?} vale y no debería", malo);
    }
    let muchos: String = (0..40).map(|_| "1-2 1;").collect();
    assert_eq!(ciclos::leer_tramos(&muchos).0.len(), ciclos::MAX_TRAMOS);
    // Paleta corta: el tramo que se sale de ella no cuenta; sin ninguno, no hay animación.
    assert!(Ciclos::nuevos(indexada(8, 8, 16), vec![Tramo { desde: 20, hasta: 30, ritmo: 100, atras: false }], Ajuste::Llenar, 0, 32, 32).is_none());
    // Índices que no casan con la medida: no.
    let mut mal = indexada(8, 8, 64);
    mal.indices.pop();
    assert!(Ciclos::nuevos(mal, vec![Tramo { desde: 0, hasta: 3, ritmo: 100, atras: false }], Ajuste::Llenar, 0, 32, 32).is_none());
}

#[test]
fn ciclos_giran_y_vuelven() {
    // Índices fuera de la paleta (de 32 a 63, con una de 32): negros, sin pánico.
    for ajuste in Ajuste::TODOS {
        for atras in [false, true] {
            let tramos = vec![Tramo { desde: 4, hasta: 11, ritmo: 1000, atras }, Tramo { desde: 20, hasta: 23, ritmo: 300, atras: !atras }];
            let mut c = Ciclos::nuevos(indexada(16, 12, 32), tramos, ajuste, 0xff11_2233, 50, 37).unwrap();
            let (w, h) = (50, 37);
            let mut base = vec![0u32; w * h];
            c.pintar_base(&mut Marco::entero(w, h, &mut base).unwrap());
            let mut v = base.clone();
            let mut ms = 0;
            for _ in 0..30 {
                ms += c.intervalo_ms() as u64;
                let antes = v.clone();
                let danos = c.paso(ms, &Marco::entero(w, h, &mut base).unwrap(), &mut Marco::entero(w, h, &mut v).unwrap());
                assert!(cubre(&antes, &v, w, &danos), "{:?}: cambió fuera de los daños", ajuste);
            }
            // Tras una vuelta entera de los dos tramos (8 pasos a 10/s y 4 a 3/s: 4 s), como al principio.
            let mut vuelta = base.clone();
            c.olvidar();
            c.paso(4000, &Marco::entero(w, h, &mut base).unwrap(), &mut Marco::entero(w, h, &mut vuelta).unwrap());
            assert_eq!(vuelta, base, "{:?}: tras una vuelta no es como al principio", ajuste);
            assert!(c.pixeles_que_giran > 0);
        }
    }
}

#[test]
fn png_de_paleta_con_sus_ciclos() {
    let png = png_paleta(5, 3, &[(b"DashOS-ciclos", b"1-3 4 atras")]);
    let i = crate::png::leer_indexada(&png, 1 << 20).unwrap();
    assert_eq!((i.ancho, i.alto), (5, 3));
    assert_eq!(i.indices[..5], [0, 1, 2, 3, 0]);
    assert_eq!(i.paleta.len(), 4);
    assert_eq!(i.textos, vec![(String::from("DashOS-ciclos"), String::from("1-3 4 atras"))]);
    // La misma imagen por `leer`: igual que antes.
    let img = crate::png::leer(&png, 1 << 20).unwrap();
    assert_eq!(img.px[1], i.paleta[1]);
    // Uno que no es de paleta.
    let rgb = include_bytes!("../pruebas/color.png");
    assert_eq!(crate::png::leer_indexada(rgb, 1 << 20).err(), Some(crate::png::Error::NoEsPaleta));
    // Cien trozos de texto: se guardan los primeros 16.
    let muchos: Vec<(&[u8], &[u8])> = (0..100).map(|_| (&b"k"[..], &b"v"[..])).collect();
    let png = png_paleta(2, 2, &muchos);
    assert_eq!(crate::png::leer_indexada(&png, 1 << 20).unwrap().textos.len(), crate::png::MAX_TEXTOS);
}

/// Un PNG de paleta de 4 colores (8 bits), sin comprimir (deflate "guardado"), con trozos de texto.
fn png_paleta(w: usize, h: usize, textos: &[(&[u8], &[u8])]) -> Vec<u8> {
    let trozo = |out: &mut Vec<u8>, tipo: &[u8], datos: &[u8]| {
        out.extend_from_slice(&(datos.len() as u32).to_be_bytes());
        let mut t = tipo.to_vec();
        t.extend_from_slice(datos);
        out.extend_from_slice(&t);
        out.extend_from_slice(&crate::png::crc32(&t).to_be_bytes());
    };
    let mut crudo = Vec::new();
    for y in 0..h {
        crudo.push(0);
        crudo.extend((0..w).map(|x| ((x + y * w) % 4) as u8));
    }
    let mut z = vec![0x78, 0x01, 1];
    z.extend_from_slice(&(crudo.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(crudo.len() as u16)).to_le_bytes());
    z.extend_from_slice(&crudo);
    let (mut a, mut b) = (1u32, 0u32);
    for &c in &crudo {
        a = (a + c as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 3, 0, 0, 0]);
    trozo(&mut out, b"IHDR", &ihdr);
    trozo(&mut out, b"PLTE", &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    for (k, v) in textos {
        let mut d = k.to_vec();
        d.push(0);
        d.extend_from_slice(v);
        trozo(&mut out, b"tEXt", &d);
    }
    trozo(&mut out, b"IDAT", &z);
    trozo(&mut out, b"IEND", &[]);
    out
}

#[test]
fn ritmo_con_tope() {
    let mut r = Ritmo::nuevo(30);
    // Un cuadro de 1 ms de CPU y 10 000 píxeles (lo del compositor, por píxel y por cuadro): a 3 % de un núcleo.
    for _ in 0..50 {
        r.cuadro(1000, 10_000);
    }
    let i = r.intervalo(16);
    let coste = 1000 + 10_000 * super::ritmo::COMPONER_NS_POR_PX / 1000 + super::ritmo::COMPONER_US_POR_CUADRO;
    let esperado = (coste * 1000 / 30 / 1000) as u32;
    assert!(i >= esperado && i <= esperado + 2, "{} y no {}", i, esperado);
    assert!(r.gasto(16) <= 30);
    assert_eq!(r.intervalo(1000), 1000);
    // Ocupada: cuatro veces más despacio, y no vuelve en el borde.
    r.ocio(200);
    assert!(r.ocupada);
    assert_eq!(r.intervalo(1000), 4000);
    r.ocio(300);
    assert!(r.ocupada);
    r.ocio(500);
    assert!(!r.ocupada);
    // Costes absurdos: el tope de 10 s.
    r.cuadro(u64::MAX, u64::MAX);
    for _ in 0..50 {
        r.cuadro(u64::MAX, u64::MAX);
    }
    assert_eq!(r.intervalo(16), super::ritmo::MAX_INTERVALO_MS);
    assert_eq!(Ritmo::nuevo(0).tope, 1);
}

#[test]
fn ajustes_del_fondo() {
    use super::ajustes::{self, Config};
    let texto = b"# mio\nfondo = noche.png\noscuro = SYS:Fondos/luna.png\nfondo-DASHOS-2 = animado:aurora\najuste = mosaico\ncolor = 112233\ntransicion = disolver\nduracion = 400\npase = 5\ncpu = 2\nefecto = nieve\notra = cosa\n";
    let (c, avisos) = ajustes::de_texto(texto);
    assert!(avisos.is_empty(), "{:?}", avisos);
    assert_eq!(c.fondo, "Home:Fondos/noche.png");
    assert_eq!(c.elegir("DASHOS-1", false), "Home:Fondos/noche.png");
    assert_eq!(c.elegir("DASHOS-1", true), "SYS:Fondos/luna.png");
    assert_eq!(c.elegir("dashos-2", true), "animado:aurora");
    assert_eq!((c.ajuste, c.color, c.duracion, c.pase, c.cpu), (Ajuste::Mosaico, 0xff11_2233, 400, 5, 20));
    assert_eq!(Config::default().cpu, super::ritmo::TOPE_DE_SERIE);
    assert_eq!(c.efecto.as_deref(), Some("nieve"));
    // Escribirlo de nuevo no pierde lo que no es suyo, y se vuelve a leer igual.
    let t = c.a_texto(Some(texto));
    let t = core::str::from_utf8(&t).unwrap();
    assert!(t.contains("# mio") && t.contains("otra = cosa"));
    assert_eq!(ajustes::de_texto(t.as_bytes()).0, c);
    // Malos: cada uno se queda en lo de serie y se avisa.
    let malo = b"fondo = ../../C/secreto\noscuro = Home:Fondos/../x\najuste = rellenar\ncolor = rojo\ntransicion = explosion\nduracion = 0\npase = -1\ncpu = 99\nefecto = fuego\ncarpeta = animado:aurora\nfondo-X = animado:nada\n";
    let (c, avisos) = ajustes::de_texto(malo);
    assert_eq!(c, Config::default());
    assert_eq!(avisos.len(), 11, "{:?}", avisos);
    // Basura al azar: nunca pánico.
    let mut z = Azar(7);
    for n in 0..2000 {
        let b: Vec<u8> = (0..z.hasta(300) as usize).map(|_| if n % 2 == 0 { z.hasta(256) as u8 } else { b"fondo=ajuste:/.-\n x"[z.hasta(19) as usize] }).collect();
        let (c, _) = ajustes::de_texto(&b);
        assert!(ajustes::nombre_vale(&c.fondo) || c.fondo == ajustes::DE_SERIE);
    }
    let mut c = Config::default();
    c.poner(Some("DASHOS-3"), true, "x.png");
    c.poner(None, false, "SYS:Fondos/y.png");
    assert_eq!(c.elegir("DASHOS-3", true), "Home:Fondos/x.png");
    assert_eq!(c.elegir("DASHOS-3", false), "SYS:Fondos/y.png");
    // Los dos modos a la vez: el oscuro deja de ser otro.
    c.poner(None, true, "noche.png");
    c.poner_los_dos(None, "dia.png");
    assert_eq!((c.elegir("X", false), c.elegir("X", true)), ("Home:Fondos/dia.png", "Home:Fondos/dia.png"));
    c.poner_los_dos(Some("DASHOS-3"), "z.png");
    assert_eq!(c.elegir("DASHOS-3", true), "Home:Fondos/z.png");
    assert!(!ajustes::nombre_vale("animado:fuego") && !ajustes::nombre_vale("") && !ajustes::nombre_vale("a\u{7}b") && !ajustes::nombre_vale("Home:"));
}

#[test]
fn juntar_danos_sin_cubrir_de_mas() {
    // 48 partículas de 21 × 21 repartidas por 1920 × 1080: juntas en como mucho 32, sin pasar de 6 veces su área.
    let mut z = Azar(3);
    let v: Vec<Rect> = (0..48).map(|_| Rect::new(z.hasta(1900) as i32, z.hasta(1060) as i32, 21, 21)).collect();
    let area: i64 = v.iter().map(|r| (r.w * r.h) as i64).sum();
    let j = juntar_danos(v.clone(), 32);
    assert!(j.len() <= 32);
    let area_j: i64 = j.iter().map(|r| (r.w * r.h) as i64).sum();
    assert!(area_j < area * 6, "{} contra {}", area_j, area);
    // Todo lo de antes sigue cubierto.
    for r in &v {
        for (x, y) in [(r.x, r.y), (r.x + r.w - 1, r.y + r.h - 1)] {
            assert!(j.iter().any(|d| d.contiene(x, y)));
        }
    }
    // Hostil: mil iguales, vacíos, y un tope de 1.
    let mil = alloc::vec![Rect::new(5, 5, 3, 3); 1000];
    assert_eq!(juntar_danos(mil, 4), alloc::vec![Rect::new(5, 5, 3, 3)]);
    assert!(juntar_danos(alloc::vec![Rect::default(); 10], 2).is_empty());
    let v: Vec<Rect> = (0..200).map(|k| Rect::new((k * 37) % 1900, (k * 91) % 1000, 5, 5)).collect();
    assert_eq!(juntar_danos(v, 1).len(), 1);
}
