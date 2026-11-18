//! El formato de los `.info` sin la caja: leer y escribir el clásico, NewIcons, el de color de OS 3.5 y el PNG, los
//! tooltypes y los nombres de las copias; y lo hostil (ficheros cortados, medidas absurdas, cuentas que mienten).

mod comun;

use caja_icono::color::{self, ImagenColor};
use caja_icono::info::{self, Error, Opciones};
use caja_icono::tooltypes::{casa_valor, siguiente_copia, valor_de};
use comun::*;

#[test]
fn clasico_ida_y_vuelta() {
    let b = bytes_clasico(&["PUBSCREEN=Workbench", "DONOTWAIT", "(DESACTIVADO=1)"]);
    assert_eq!(&b[0..4], &[0xE3, 0x10, 0, 1]);
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert_eq!(i, clasico(&["PUBSCREEN=Workbench", "DONOTWAIT", "(DESACTIVADO=1)"]));
    assert_eq!(info::escribir(&i, Opciones::default()).unwrap(), b);
    // Un cajón lleva su DrawerData (56 bytes delante y 6 detrás, con la revisión 1).
    let mut c = info::Info::vacio(info::WBDRAWER);
    c.imagen1 = Some(planar(8, 4, 1, |x, _| (x & 1) as u8));
    let mut cj = info::Cajon::de_serie();
    cj.flags = 2;
    cj.modos = 3;
    c.cajon = Some(cj);
    let b = info::escribir(&c, Opciones::default()).unwrap();
    assert_eq!(b.len(), 78 + 56 + 20 + 2 * 4 + 6);
    let l = info::leer(&b, Opciones::default()).unwrap();
    assert_eq!(l.cajon.as_ref().map(|c| (c.flags, c.modos)), Some((2, 3)));
    assert_eq!(l.user_data & 0xFF, 1);
}

#[test]
fn newicons_se_leen_y_se_quitan_de_los_tooltypes() {
    let (b, px, pal) = newicon();
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert!(i.es_newicon);
    let c = i.color.as_ref().unwrap();
    assert_eq!((c.ancho, c.alto), (16, 12));
    let m = c.img[0].as_ref().unwrap();
    assert_eq!(m.paleta, pal);
    assert_eq!(m.indices, px);
    assert_eq!(m.transparente, Some(0));
    // Los tooltypes que ve el programa son los suyos; los de la imagen se guardan aparte.
    assert_eq!(i.tooltypes.as_ref().unwrap(), &vec![b"PUBSCREEN=Workbench".to_vec()]);
    assert!(i.newicons.len() >= 4 && i.newicons[0] == b" ");
    // Al escribir vuelven detrás (y no se añade FORM ICON: la imagen ya va en ellos).
    let otra = info::escribir(&i, Opciones::default()).unwrap();
    assert!(!otra.windows(4).any(|w| w == b"FORM"));
    let j = info::leer(&otra, Opciones::default()).unwrap();
    assert_eq!(j.color, i.color);
    // Con ICONPUTA_DropNewIconToolTypes, no.
    let sin = info::escribir(&i, Opciones { sin_newicons: true, sin_color: true, ..Opciones::default() }).unwrap();
    assert!(info::leer(&sin, Opciones::default()).unwrap().color.is_none());
    // Sin la opción de NewIcons, los tooltypes se quedan como están.
    let crudo = info::leer(&b, Opciones { newicons: false, ..Opciones::default() }).unwrap();
    assert!(crudo.color.is_none() && crudo.tooltypes.unwrap().len() > 3);
}

#[test]
fn de_color_ida_y_vuelta_con_repeticiones() {
    let (b, c) = de_color();
    let f = b.windows(4).position(|w| w == b"FORM").unwrap();
    assert_eq!(&b[f + 8..f + 12], b"ICON");
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert!(!i.es_newicon);
    assert_eq!(i.color.as_ref().unwrap(), &c);
    // Las repeticiones: lo escrito ocupa menos que un byte por punto.
    let img = &b[f..];
    assert!(img.len() < 24 * 18 * 2, "{} bytes", img.len());
    // La segunda imagen, con la misma paleta, no la repite.
    assert_eq!(img.windows(4).filter(|w| w == b"IMAG").count(), 2);
    // Sin color: solo lo clásico.
    let i2 = info::leer(&b, Opciones { color: false, ..Opciones::default() }).unwrap();
    assert!(i2.color.is_none() && i2.imagen1.is_some());
}

#[test]
fn imag_sin_comprimir_y_de_256_colores() {
    // Un FORM ICON hecho a mano: IMAG sin comprimir (un byte por punto) y paleta tal cual.
    let mut d = Vec::new();
    let trozo = |v: &mut Vec<u8>, id: &[u8], c: &[u8]| {
        v.extend_from_slice(id);
        v.extend_from_slice(&(c.len() as u32).to_be_bytes());
        v.extend_from_slice(c);
        if c.len() % 2 == 1 {
            v.push(0);
        }
    };
    let mut cuerpo = Vec::new();
    trozo(&mut cuerpo, b"FACE", &[3, 1, 0, 0, 0, 8]);
    let mut imag = vec![0, 2, 2, 0, 0, 2, 0, 7, 0, 8];
    imag.extend_from_slice(&[0, 1, 2, 1, 0, 1, 2, 1]);
    imag.extend_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80, 90]);
    trozo(&mut cuerpo, b"IMAG", &imag);
    trozo(&mut cuerpo, b"RARO", &[1, 2, 3]);
    d.extend_from_slice(b"FORM");
    d.extend_from_slice(&(cuerpo.len() as u32 + 4).to_be_bytes());
    d.extend_from_slice(b"ICON");
    d.extend_from_slice(&cuerpo);
    let c = color::leer_glow(&d).unwrap();
    assert_eq!((c.ancho, c.alto), (4, 2));
    assert_eq!(c.img[0].as_ref().unwrap().indices, vec![0, 1, 2, 1, 0, 1, 2, 1]);
    assert_eq!(c.img[0].as_ref().unwrap().paleta[2], [70, 80, 90]);
    // El trozo que no se entiende se guarda y se vuelve a escribir.
    assert_eq!(c.extra, vec![(*b"RARO", vec![1, 2, 3])]);
    let otra = color::leer_glow(&color::escribir_glow(&c, true)).unwrap();
    assert_eq!(otra, c);
    // 256 colores y 256 × 256 (el máximo de FACE): se escribe y se lee igual.
    let pal = paleta(256);
    let px: Vec<u8> = (0..256 * 256).map(|k| (k * 7 % 256) as u8).collect();
    let g = color::Color {
        ancho: 256,
        alto: 256,
        img: [Some(ImagenColor { indices: px, paleta: pal, transparente: None }), None],
        ..Default::default()
    };
    assert_eq!(color::leer_glow(&color::escribir_glow(&g, true)).unwrap(), g);
}

#[test]
fn png_con_seleccionado() {
    let b = icono_png();
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert!(i.png);
    let c = i.color.as_ref().unwrap();
    assert_eq!((c.ancho, c.alto), (32, 32));
    let a = c.argb[0].as_ref().unwrap();
    assert_eq!(a[0] >> 24, 0);
    assert_eq!(a[5], 0xFF00_0000 | (5 * 8) << 16 | 0x40);
    assert_eq!(c.argb[1].as_ref().unwrap()[10], 0xFFFF_0000);
    // Lleva una imagen planar sacada de la de color (para quien pinte GadgetRender), de 2 planos.
    let p = i.imagen1.as_ref().unwrap();
    assert_eq!((p.ancho, p.alto, p.prof), (32, 32, 2));
    // Al escribirlo se convierte en uno de OS 3.5 (lo clásico y el FORM ICON).
    let w = info::escribir(&i, Opciones::default()).unwrap();
    let j = info::leer(&w, Opciones::default()).unwrap();
    assert!(!j.png);
    // Más de 255 colores: pasan por el cubo de 6 × 6 × 6 (cada canal, a menos de 26 del de verdad).
    let cj = j.color.unwrap();
    let q = cj.img[0].as_ref().unwrap().argb();
    for (k, (&x, &y)) in q.iter().zip(a.iter()).enumerate() {
        assert_eq!(x >> 24 == 0, y >> 24 == 0, "punto {k}");
        for s in [0, 8, 16] {
            assert!((((x >> s) & 0xFF) as i32 - ((y >> s) & 0xFF) as i32).abs() <= 26 || y >> 24 == 0, "punto {k}");
        }
    }
}

#[test]
fn tooltypes_y_copias() {
    assert_eq!(valor_de(b"PUBSCREEN=Workbench", b"pubscreen"), Some(10));
    assert_eq!(valor_de(b"  PUBSCREEN = Workbench", b"PUBSCREEN"), Some(14));
    assert_eq!(valor_de(b"DONOTWAIT", b"donotwait"), Some(9));
    assert_eq!(valor_de(b"(DONOTWAIT)", b"donotwait"), None);
    assert_eq!(valor_de(b"PUBSCREENX=1", b"PUBSCREEN"), None);
    assert_eq!(valor_de(b"=1", b""), None);
    assert!(casa_valor(b"Uno|DOS|tres", b"dos"));
    assert!(casa_valor(b"uno | dos", b" dos "));
    assert!(!casa_valor(b"uno|dos", b"do"));
    assert!(casa_valor(b"SOLO", b"solo"));
    // Lo que da el oráculo (caja/pruebas68k/icono.c, Kickstart 40.68).
    assert_eq!(siguiente_copia(b"Fichero", 30), b"Copy_of_Fichero");
    assert_eq!(siguiente_copia(b"copy of Fichero", 30), b"Copy_2_of_Fichero");
    assert_eq!(siguiente_copia(b"copy 2 of Fichero", 30), b"Copy_3_of_Fichero");
    assert_eq!(siguiente_copia(b"copy 9 of F", 30), b"Copy_10_of_F");
    assert_eq!(siguiente_copia(b"copy of", 30), b"Copy_of_copy of");
    assert_eq!(siguiente_copia(b"copy x of F", 30), b"Copy_of_copy x of F");
    assert_eq!(siguiente_copia(b"Un nombre muy muy largo de treinta", 30), b"Copy_of_Un nombre muy muy larg");
    assert_eq!(siguiente_copia(b"Copy_2_of_F", 30), b"Copy_3_of_F");
    assert_eq!(siguiente_copia(b"copy 99999999999 of X", 40), b"Copy_of_copy 99999999999 of X");
    assert_eq!(siguiente_copia(b"un nombre bastante largo de verdad", 30).len(), 30);
}

#[test]
fn hostil_cortado_en_cada_byte() {
    for b in [bytes_clasico(&["A=1", "B=2"]), newicon().0, de_color().0] {
        for n in 0..b.len() {
            // Nunca un pánico; lo clásico cortado es un error, el color cortado se ignora.
            let _ = info::leer(&b[..n], Opciones::default());
        }
    }
    let p = icono_png();
    for n in (0..p.len()).step_by(7) {
        let _ = info::leer(&p[..n], Opciones::default());
    }
    assert_eq!(info::leer(&[0xE3], Opciones::default()), Err(Error::NoEsIcono));
    assert_eq!(info::leer(&bytes_clasico(&[])[..100], Opciones::default()), Err(Error::Corto));
}

#[test]
fn hostil_medidas_y_cuentas_absurdas() {
    let mut b = bytes_clasico(&["A=1"]);
    // La imagen dice medir 30000 × 30000.
    let img = 78;
    b[img + 4..img + 6].copy_from_slice(&30000u16.to_be_bytes());
    b[img + 6..img + 8].copy_from_slice(&30000u16.to_be_bytes());
    assert_eq!(info::leer(&b, Opciones::default()), Err(Error::Grande));
    // Profundidad negativa.
    let mut b = bytes_clasico(&["A=1"]);
    b[img + 8..img + 10].copy_from_slice(&0xFFFFu16.to_be_bytes());
    assert_eq!(info::leer(&b, Opciones::default()), Err(Error::MalFormado));
    // Los tooltypes dicen ser mil millones.
    let i = clasico(&["A=1"]);
    let mut b = info::escribir(&i, Opciones::default()).unwrap();
    let pos = b.windows(4).position(|w| w == 8u32.to_be_bytes()).unwrap();
    b[pos..pos + 4].copy_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
    assert!(info::leer(&b, Opciones::default()).is_err());
    // Un texto que dice medir 4 GB.
    let mut b = bytes_clasico(&[]);
    let pos = b.windows(4).position(|w| w == 18u32.to_be_bytes()).unwrap();
    b[pos..pos + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(info::leer(&b, Opciones::default()).is_err());
    // Un FORM ICON con trozos que dicen medir más que todo: se queda lo clásico.
    let (mut b, _) = de_color();
    let f = b.windows(4).position(|w| w == b"FACE").unwrap();
    b[f + 4..f + 8].copy_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert!(i.color.is_none() && i.imagen1.is_some());
    // Un IMAG con repeticiones cortadas.
    let (mut b, _) = de_color();
    let f = b.windows(4).position(|w| w == b"IMAG").unwrap();
    let largo = u32::from_be_bytes([b[f + 4], b[f + 5], b[f + 6], b[f + 7]]) as usize;
    for k in f + 18..f + 8 + largo {
        b[k] = 0x7F;
    }
    let _ = info::leer(&b, Opciones::default());
    // Un PNG que dice medir 100000 × 100000.
    let mut p = png(4, 4, |_, _| 0xFFFF_FFFF);
    p[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    p[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    assert!(info::leer(&p, Opciones::default()).is_err());
    // Un NewIcon con la cabecera rota o los píxeles que no llegan: sin color, con sus tooltypes.
    let mut i = clasico(&["IM1=B!!!!", "IM1=\u{1}"]);
    i.tooltypes.as_mut().unwrap().push(latin1("IM1=Cÿÿ!%"));
    let b = info::escribir(&i, Opciones::default()).unwrap();
    let l = info::leer(&b, Opciones::default()).unwrap();
    assert!(l.color.is_none());
    // Más de 2 MB no se lee.
    assert_eq!(info::leer(&vec![0xE3; 3 << 20], Opciones::default()), Err(Error::Grande));
}

#[test]
fn diez_mil_tooltypes() {
    let tt: Vec<String> = (0..10_000).map(|k| format!("CLAVE{k}=valor{k}")).collect();
    let refs: Vec<&str> = tt.iter().map(|s| s.as_str()).collect();
    let b = bytes_clasico(&refs);
    let i = info::leer(&b, Opciones::default()).unwrap();
    assert_eq!(i.tooltypes.as_ref().unwrap().len(), 10_000);
    assert_eq!(i.tooltypes.as_ref().unwrap()[9_999], b"CLAVE9999=valor9999");
    // Más de los que se dejan, no.
    let tt: Vec<String> = (0..20_000).map(|k| format!("{k}")).collect();
    let refs: Vec<&str> = tt.iter().map(|s| s.as_str()).collect();
    let mut i = clasico(&[]);
    i.tooltypes = Some(refs.iter().map(|s| s.as_bytes().to_vec()).collect());
    assert_eq!(info::escribir(&i, Opciones::default()), Err(Error::Grande));
}

#[test]
fn cuantizar_y_planar() {
    // Pocos colores: exactos, con el transparente en el 0.
    let px = [0u32, 0xFF11_2233, 0xFF11_2233, 0xFF44_5566];
    let m = color::cuantizar(&px);
    assert_eq!(m.transparente, Some(0));
    assert_eq!(m.argb(), px.to_vec());
    // Muchos: el cubo, y nunca un índice fuera de la paleta.
    let px: Vec<u32> = (0..70_000u32).map(|k| 0xFF00_0000 | k * 239).collect();
    let m = color::cuantizar(&px);
    assert!(m.paleta.len() <= 256 && m.indices.iter().all(|&i| (i as usize) < m.paleta.len()));
    let p = color::planar_de(&[0, 0xFF00_0000, 0xFFFF_FFFF, 0xFF66_88BB], 4, 1);
    assert_eq!(caja_icono::diskobj::indices_planar(&p), vec![0, 1, 2, 3]);
}
