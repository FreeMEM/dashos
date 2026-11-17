//! Las pruebas de los `.info`: uno de cada formato hecho aquí (`tools/iconos_amiga_prueba.py`, en `src/pruebas/`), los
//! hostiles y mil mutados al azar con semilla. Con `DASHOS_ICONOS_REALES=carpeta` lee además todos los `.info` de esa
//! carpeta (iconos de verdad, que no van en git) y dice cuántos de cada formato; con `DASHOS_ICONOS_VOLCAR=carpeta`
//! deja en ella cada imagen leída en crudo (ancho, alto y ARGB), para mirarlas.

use crate::amiga::{self, Error, Formato, Icono, Tipo};
use alloc::vec::Vec;

extern crate std;
use std::string::String;

const CAJON: &[u8] = include_bytes!("pruebas/Cajon.info");
const DISCO: &[u8] = include_bytes!("pruebas/Disk.info");
const PROGRAMA: &[u8] = include_bytes!("pruebas/Programa.info");
const BRILLO: &[u8] = include_bytes!("pruebas/Brillo.info");
const CRISTAL: &[u8] = include_bytes!("pruebas/Cristal.info");
const ESTRELLA: &[u8] = include_bytes!("pruebas/Estrella.info");
const ROTO: &[u8] = include_bytes!("pruebas/Roto.info");
const TODOS: [(&str, &[u8]); 7] =
    [("Cajon", CAJON), ("Disk", DISCO), ("Programa", PROGRAMA), ("Brillo", BRILLO), ("Cristal", CRISTAL), ("Estrella", ESTRELLA), ("Roto", ROTO)];

fn px(i: &kit::imagen::Imagen, x: usize, y: usize) -> u32 {
    i.px[y * i.ancho + x]
}

fn volcar(nombre: &str, ic: &Icono) {
    let Ok(dir) = std::env::var("DASHOS_ICONOS_VOLCAR") else { return };
    for (sufijo, img) in [("", Some(&ic.normal)), (".sel", ic.seleccionado.as_ref())] {
        let Some(img) = img else { continue };
        let mut d = std::format!("{} {}\n", img.ancho, img.alto).into_bytes();
        for p in &img.px {
            d.extend_from_slice(&p.to_be_bytes());
        }
        let n: String = nombre.chars().map(|c| if c.is_alphanumeric() || c == '.' { c } else { '_' }).collect();
        std::fs::write(std::format!("{dir}/{n}{sufijo}.argb"), d).unwrap();
    }
}

#[test]
fn clasico_con_dos_imagenes() {
    let ic = amiga::leer(CAJON).unwrap();
    volcar("Cajon", &ic);
    assert_eq!((ic.formato, ic.tipo), (Formato::Clasico, Some(Tipo::Cajon)));
    assert_eq!((ic.normal.ancho, ic.normal.alto), (44, 22));
    // Fuera del cajón, el fondo (color 0): transparente. El frente, azul del Workbench; el borde de arriba, blanco.
    assert_eq!(px(&ic.normal, 0, 0), 0);
    assert_eq!(px(&ic.normal, 30, 16), 0xff66_88bb);
    assert_eq!(px(&ic.normal, 10, 3), 0xffff_ffff);
    // La segunda imagen es la suya (el cajón abierto: el hueco, negro).
    let sel = ic.seleccionado.unwrap();
    assert_eq!(px(&sel, 10, 5), 0xff00_0000);
    assert_eq!(px(&ic.normal, 10, 5), 0xff66_88bb);
}

#[test]
fn clasico_complemento() {
    let ic = amiga::leer(DISCO).unwrap();
    volcar("Disk", &ic);
    assert_eq!((ic.formato, ic.tipo), (Formato::Clasico, Some(Tipo::Disco)));
    assert_eq!((ic.normal.ancho, ic.normal.alto), (36, 20));
    // Seleccionado por complemento: todo el rectángulo al revés (el fondo, el color 3; el azul, el 0).
    let sel = ic.seleccionado.unwrap();
    assert_eq!(px(&sel, 0, 0), 0xff66_88bb);
    assert_eq!(px(&ic.normal, 6, 8), 0xff66_88bb);
    assert_eq!(px(&sel, 6, 8), 0);
    // El blanco de la etiqueta (2) pasa a negro (1).
    assert_eq!(px(&ic.normal, 12, 12), 0xffff_ffff);
    assert_eq!(px(&sel, 12, 12), 0xff00_0000);
}

#[test]
fn newicons() {
    let ic = amiga::leer(PROGRAMA).unwrap();
    volcar("Programa", &ic);
    assert_eq!((ic.formato, ic.tipo), (Formato::NewIcons, Some(Tipo::Programa)));
    assert_eq!((ic.normal.ancho, ic.normal.alto), (40, 40));
    assert_eq!(px(&ic.normal, 0, 0), 0, "la esquina, transparente");
    assert_eq!(px(&ic.normal, 20, 20), 0, "el agujero del engranaje, transparente");
    assert_eq!(px(&ic.normal, 19, 12), 0xff1e_1e28, "el engranaje");
    let sel = ic.seleccionado.unwrap();
    assert_eq!((sel.ancho, sel.alto), (40, 40));
    // La segunda, más clara en el degradado.
    assert!(px(&sel, 3, 20) & 0xff > px(&ic.normal, 3, 20) & 0xff);
}

#[test]
fn glowicons() {
    let ic = amiga::leer(BRILLO).unwrap();
    volcar("Brillo", &ic);
    assert_eq!((ic.formato, ic.tipo, ic.sin_marco), (Formato::GlowIcons, Some(Tipo::Proyecto), false));
    assert_eq!((ic.normal.ancho, ic.normal.alto), (48, 48));
    assert_eq!(px(&ic.normal, 2, 2), 0);
    assert_eq!(px(&ic.normal, 24, 20), 0xffff_c83c, "el centro de la bombilla");
    assert_eq!(px(&ic.normal, 10, 30), 0xfffa_faf5, "la hoja");
    // La segunda no trae paleta: usa la de la primera.
    let sel = ic.seleccionado.unwrap();
    assert_eq!(px(&sel, 24, 20), 0xffff_e678);
}

#[test]
fn argb_de_os4() {
    let ic = amiga::leer(CRISTAL).unwrap();
    volcar("Cristal", &ic);
    assert_eq!((ic.formato, ic.sin_marco), (Formato::Argb, true));
    assert_eq!((ic.normal.ancho, ic.normal.alto), (48, 48));
    assert_eq!(px(&ic.normal, 0, 0) >> 24, 0);
    assert_eq!(px(&ic.normal, 24, 24) >> 24, 255);
    let borde = px(&ic.normal, 24, 3) >> 24;
    assert!(borde > 0 && borde < 255, "alfa a medias en el borde: {borde}");
    assert!(ic.seleccionado.is_some());
}

#[test]
fn png_de_os4_con_segundo() {
    let ic = amiga::leer(ESTRELLA).unwrap();
    volcar("Estrella", &ic);
    assert_eq!((ic.formato, ic.tipo), (Formato::Png, None));
    assert_eq!((ic.normal.ancho, ic.normal.alto), (52, 52));
    assert_eq!(px(&ic.normal, 25, 26), 0xfff0_be1e);
    assert_eq!(px(&ic.seleccionado.unwrap(), 25, 26), 0xffdc_3c32);
    // Sin el segundo PNG (o con él roto), solo la normal.
    let corte = ESTRELLA.len() / 2 + 10;
    let solo = amiga::leer(&ESTRELLA[..corte]);
    assert!(solo.is_err() || solo.unwrap().seleccionado.is_none());
}

#[test]
fn hostiles_a_mano() {
    // Roto: dice 300 × 400 con 8 planos y se acaba.
    assert_eq!(amiga::leer(ROTO).err(), Some(Error::Corto));
    assert_eq!(amiga::leer(b"").err(), Some(Error::NoEsIcono));
    assert_eq!(amiga::leer(b"\xe3").err(), Some(Error::NoEsIcono));
    assert_eq!(amiga::leer(b"\xe3\x10\x00\x01").err(), Some(Error::Corto));
    // El `.info` de mentira del disco de pruebas (cabecera a cero y un nombre): sin imagen que leer.
    let mut falso = alloc::vec![0xe3, 0x10, 0, 1];
    falso.extend_from_slice(&[0; 74]);
    falso.extend_from_slice(b"saludo\0");
    assert!(amiga::leer(&falso).is_err());
    // Medidas negativas, profundidad 9, y una enorme.
    for (w, h, d, e) in [(-1i16, 4i16, 2i16, Error::MalFormado), (4, -4, 2, Error::MalFormado), (4, 4, 9, Error::MalFormado), (4, 4, -1, Error::MalFormado), (600, 4, 2, Error::Grande)] {
        let mut b = CAJON.to_vec();
        let i = 0x4e + 56;
        b[i + 4..i + 10].copy_from_slice(&[w.to_be_bytes(), h.to_be_bytes(), d.to_be_bytes()].concat());
        assert_eq!(amiga::leer(&b).err(), Some(e), "{w}x{h}x{d}");
    }
    // Profundidad 0 y sin datos: todo del PlaneOnOff (sin pánico).
    let mut b = CAJON.to_vec();
    let i = 0x4e + 56;
    b[i + 8..i + 10].copy_from_slice(&0u16.to_be_bytes());
    assert!(amiga::leer(&b).is_ok());
    // Un `FORM ICON` estropeado no se lleva la imagen clásica: queda la clásica.
    let mut b = BRILLO.to_vec();
    let f = b.windows(4).position(|w| w == b"IMAG").unwrap();
    b[f + 8 + 5] = 9; // 9 bits por píxel
    let ic = amiga::leer(&b).unwrap();
    assert_eq!(ic.formato, Formato::Clasico);
    // Los ToolTypes que dicen ser un millón.
    let mut b = PROGRAMA.to_vec();
    let cuenta = 0x4e + 20 + 16 * 8 * 2 / 8 * 2;
    b[cuenta..cuenta + 4].copy_from_slice(&4_000_000u32.to_be_bytes());
    let ic = amiga::leer(&b).unwrap();
    assert_eq!(ic.formato, Formato::Clasico);
    // Un ARGB con zlib que miente (da más de lo que cabe): se prueba el resto, no hay imagen ARGB.
    let mut b = CRISTAL.to_vec();
    let a = b.windows(4).position(|w| w == b"FACE").unwrap();
    b[a + 8] = 3; // FACE dice 4 de ancho: el zlib da más
    assert!(amiga::leer(&b).map(|i| i.formato) != Ok(Formato::Argb));
}

/// Un generador de números de siempre (xorshift), para que los mutados sean los mismos en cada pasada.
struct Azar(u64);

impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn mil_mutados_sin_panico() {
    let mut az = Azar(0x9e37_79b9_7f4a_7c15);
    let mut leidos = 0;
    for ronda in 0..1000 {
        let (_, base) = TODOS[ronda % TODOS.len()];
        let mut b: Vec<u8> = base.to_vec();
        match az.sig() % 4 {
            // Bytes cambiados al azar (más en la cabecera, donde están las medidas).
            0 => {
                for _ in 0..1 + az.sig() % 8 {
                    let i = (az.sig() as usize) % b.len().min(if az.sig() % 2 == 0 { 140 } else { usize::MAX });
                    b[i] = az.sig() as u8;
                }
            }
            // Cortado por cualquier sitio.
            1 => b.truncate((az.sig() as usize) % b.len()),
            // Un u16 o u32 de medida puesto a un valor extremo.
            2 => {
                let i = (az.sig() as usize) % b.len().saturating_sub(4).max(1);
                let v: [u8; 4] = [[0xff; 4], [0x7f, 0xff, 0xff, 0xff], [0, 0, 0, 0], [0x80, 0, 0, 0]][(az.sig() % 4) as usize];
                let n = (b.len() - i).min(4);
                b[i..i + n].copy_from_slice(&v[..n]);
            }
            // Trozos repetidos o basura añadida.
            _ => {
                let i = (az.sig() as usize) % b.len();
                let j = (i + (az.sig() as usize) % 200).min(b.len());
                let trozo = b[i..j].to_vec();
                b.splice(i..i, trozo);
            }
        }
        if let Ok(ic) = amiga::leer(&b) {
            leidos += 1;
            assert_eq!(ic.normal.px.len(), ic.normal.ancho * ic.normal.alto);
            assert!(ic.normal.ancho <= amiga::MAX_LADO && ic.normal.alto <= amiga::MAX_LADO);
            if let Some(s) = &ic.seleccionado {
                assert_eq!(s.px.len(), s.ancho * s.alto);
            }
        }
    }
    std::println!("mutados: {leidos} de 1000 se leen");
}

#[test]
fn iconos_reales() {
    let Ok(dir) = std::env::var("DASHOS_ICONOS_REALES") else { return };
    let mut cuenta = std::collections::BTreeMap::<String, usize>::new();
    let mut pila = alloc::vec![std::path::PathBuf::from(dir)];
    while let Some(d) = pila.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                pila.push(p);
            } else if p.extension().is_some_and(|x| x == "info") {
                let b = std::fs::read(&p).unwrap();
                let clave = match amiga::leer(&b) {
                    Ok(ic) => {
                        volcar(&std::format!("{}", p.display()), &ic);
                        String::from(ic.formato.nombre()) + if ic.seleccionado.is_some() { " (con seleccionado)" } else { "" }
                    }
                    Err(e) => std::format!("error {e:?}"),
                };
                if clave.starts_with("error") {
                    std::println!("{}: {clave}", p.display());
                }
                *cuenta.entry(clave).or_default() += 1;
            }
        }
    }
    for (k, v) in &cuenta {
        std::println!("{v:5} {k}");
    }
}

/// Con `DASHOS_CATALOGO=fichero` (un catálogo de `programas`, del puente) y `DASHOS_CATALOGO_PPM=salida`: lee los
/// iconos PNG del catálogo y los pinta a 48 en una rejilla, como los pintaría el lanzador.
#[test]
fn catalogo_de_verdad() {
    let (Ok(f), Ok(sal)) = (std::env::var("DASHOS_CATALOGO"), std::env::var("DASHOS_CATALOGO_PPM")) else { return };
    let cat = crate::escritorio::leer_catalogo(&std::fs::read(f).unwrap()).unwrap();
    let (cols, celda) = (12usize, 64usize);
    let filas = cat.len().div_ceil(cols).max(1);
    let (w, h) = (cols * celda, filas * celda);
    let mut px = alloc::vec![0xffee_eef2u32; w * h];
    let mut l = kit::lienzo::Lienzo::new(&mut px, w as i32, h as i32, w);
    let (mut bien, mut mal) = (0, 0);
    for (i, e) in cat.iter().enumerate() {
        if let crate::escritorio::Icono::Png(d) = &e.icono {
            match kit::png::leer(d, 1024 * 1024) {
                Ok(img) => {
                    bien += 1;
                    crate::dibujo::pintar(&mut l, ((i % cols) * celda + 8) as i32, ((i / cols) * celda + 8) as i32, 48, &img);
                }
                Err(_) => mal += 1,
            }
        }
    }
    let mut ppm = std::format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in &px {
        ppm.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(sal, ppm).unwrap();
    std::println!("catálogo: {} programas; {bien} iconos PNG leídos, {mal} no", cat.len());
}
