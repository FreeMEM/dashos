//! Dibujar y comprobar los puntos: líneas en todas las direcciones, rectángulos recortados, modos de dibujo,
//! texto, paletas, copias de bloques. En mapas nativos (ARGB) y planares (los del programa, en su memoria).

mod comun;

use caja_exec::Memoria;
use caja_intuition::estructuras::{bitmap, drmd, rastport, textfont};
use comun::*;

/// Una línea de Bresenham conocida: (0, 0)-(5, 2).
#[test]
fn linea_conocida() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 16, 4);
    pen(&mut c, rp, 1);
    mover(&mut c, rp, 0, 0);
    draw(&mut c, rp, 5, 2);
    assert_eq!(ver(&mut c, rp, 7, 3), ["##.....", "..##...", "....##."]);
    // La posición actual queda en el final.
    assert_eq!((l16(&c, rp + rastport::CP_X), l16(&c, rp + rastport::CP_Y)), (5, 2));
}

/// Líneas en los ocho octantes y en las cuatro direcciones rectas, en los dos tipos de mapa: los dos extremos,
/// un punto por columna (o fila) del eje largo y nada fuera de la caja de la línea.
#[test]
fn lineas_en_todas_las_direcciones() {
    let destinos = [
        (10, 3),
        (3, 10),
        (-3, 10),
        (-10, 3),
        (-10, -3),
        (-3, -10),
        (3, -10),
        (10, -3),
        (10, 0),
        (0, 10),
        (-10, 0),
        (0, -10),
        (7, 7),
        (-7, 7),
        (-7, -7),
        (7, -7),
    ];
    for nativo_si in [true, false] {
        for (dx, dy) in destinos {
            let mut c = caja();
            let (_, rp) = if nativo_si { nativo(&mut c, 32, 32) } else { planar(&mut c, 2, 32, 32) };
            pen(&mut c, rp, 1);
            let (x0, y0, x1, y1) = (15, 15, 15 + dx, 15 + dy);
            mover(&mut c, rp, x0, y0);
            draw(&mut c, rp, x1, y1);
            let mut puntos = Vec::new();
            for y in 0..32 {
                for x in 0..32 {
                    if pluma(&mut c, rp, x, y) == 1 {
                        puntos.push((x, y));
                    }
                }
            }
            let largo = dx.abs().max(dy.abs()) + 1;
            assert_eq!(puntos.len() as i32, largo, "({dx},{dy}) nativo={nativo_si}");
            assert!(puntos.contains(&(x0, y0)) && puntos.contains(&(x1, y1)));
            for &(x, y) in &puntos {
                assert!(x >= x0.min(x1) && x <= x0.max(x1) && y >= y0.min(y1) && y <= y0.max(y1));
            }
            if dx.abs() >= dy.abs() {
                for x in x0.min(x1)..=x0.max(x1) {
                    assert_eq!(puntos.iter().filter(|p| p.0 == x).count(), 1);
                }
            } else {
                for y in y0.min(y1)..=y0.max(y1) {
                    assert_eq!(puntos.iter().filter(|p| p.1 == y).count(), 1);
                }
            }
        }
    }
}

#[test]
fn linea_de_un_punto_y_fuera_del_mapa() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 16, 8);
    pen(&mut c, rp, 1);
    mover(&mut c, rp, 3, 3);
    draw(&mut c, rp, 3, 3);
    assert_eq!(pluma(&mut c, rp, 3, 3), 1);
    // Una línea que sale del mapa se recorta: lo de dentro se dibuja, lo de fuera no rompe nada.
    mover(&mut c, rp, -100, 5);
    draw(&mut c, rp, 100, 5);
    assert_eq!((0..16).filter(|&x| pluma(&mut c, rp, x, 5) == 1).count(), 16);
    mover(&mut c, rp, -32768, -32768);
    draw(&mut c, rp, 32767, 32767);
    assert!(c.1.avisos().is_empty());
}

#[test]
fn rectangulos_recortados() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 16, 10);
    pen(&mut c, rp, 1);
    rect(&mut c, rp, -5, -5, 2, 1);
    rect(&mut c, rp, 14, 8, 40, 40);
    assert_eq!(
        ver(&mut c, rp, 16, 10),
        [
            "###.............",
            "###.............",
            "................",
            "................",
            "................",
            "................",
            "................",
            "................",
            "..............##",
            "..............##",
        ]
    );
    // Del todo fuera, o al revés: nada.
    rect(&mut c, rp, 20, 20, 30, 30);
    rect(&mut c, rp, 8, 5, 4, 7);
    rect(&mut c, rp, -32768, -32768, -1, -1);
    assert_eq!(ver(&mut c, rp, 16, 10).concat().matches('#').count(), 10);
    // En uno nativo, lo mismo con el color de la pluma.
    let (bm, rp) = nativo(&mut c, 8, 8);
    pen(&mut c, rp, 2);
    rect(&mut c, rp, 6, 6, 100, 100);
    let l = c.1.lienzo(bm).unwrap();
    assert_eq!(l.pixel(7, 7), 0xFFFF_FFFF);
    assert_eq!(l.pixel(5, 5), 0xFFAA_AAAA);
}

#[test]
fn jam1_jam2_complement_e_inversvid_con_texto() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 2, 16, 8);
    let a = cadena(&mut c, "A");
    let topaz = c.1.topaz();
    // El glifo de la 'A' de la fuente, para comparar.
    let loc = l32(&c, topaz + textfont::TF_CHARLOC);
    let datos = l32(&c, topaz + textfont::TF_CHARDATA);
    let modulo = l16(&c, topaz + textfont::TF_MODULO) as u32;
    let off = l32(&c, loc + ('A' as u32 - 32) * 4) >> 16;
    let glifo: Vec<u8> = (0..8).map(|y| l8(&c, datos + y * modulo + off / 8)).collect();

    // JAM2: el dibujo con la pluma A (1) y el fondo con la B (2).
    g(&mut c, "SetABPenDrMd", &[1, 2, drmd::JAM2 as u32], &[0, rp]);
    mover(&mut c, rp, 0, 6);
    g(&mut c, "Text", &[1], &[a, rp]);
    for y in 0..8 {
        for x in 0..8 {
            let esperado = if glifo[y as usize] & (0x80 >> x) != 0 { 1 } else { 2 };
            assert_eq!(pluma(&mut c, rp, x, y), esperado, "({x},{y})");
        }
    }
    assert_eq!(l16(&c, rp + rastport::CP_X), 8, "avanza un carácter");

    // JAM1 sobre fondo 3: el fondo no se toca.
    g(&mut c, "SetRast", &[3], &[0, rp]);
    g(&mut c, "SetDrMd", &[drmd::JAM1 as u32], &[0, rp]);
    mover(&mut c, rp, 0, 6);
    g(&mut c, "Text", &[1], &[a, rp]);
    for y in 0..8 {
        for x in 0..8 {
            let esperado = if glifo[y as usize] & (0x80 >> x) != 0 { 1 } else { 3 };
            assert_eq!(pluma(&mut c, rp, x, y), esperado);
        }
    }

    // JAM2 | INVERSVID: al revés.
    g(&mut c, "SetDrMd", &[(drmd::JAM2 | drmd::INVERSVID) as u32], &[0, rp]);
    mover(&mut c, rp, 8, 6);
    g(&mut c, "Text", &[1], &[a, rp]);
    for y in 0..8 {
        for x in 0..8 {
            let esperado = if glifo[y as usize] & (0x80 >> x) != 0 { 2 } else { 1 };
            assert_eq!(pluma(&mut c, rp, 8 + x, y), esperado);
        }
    }

    // COMPLEMENT: invierte los bits del dibujo; dos veces, lo deja como estaba.
    g(&mut c, "SetRast", &[1], &[0, rp]);
    g(&mut c, "SetDrMd", &[drmd::COMPLEMENT as u32], &[0, rp]);
    mover(&mut c, rp, 0, 6);
    g(&mut c, "Text", &[1], &[a, rp]);
    for y in 0..8 {
        for x in 0..8 {
            let esperado = if glifo[y as usize] & (0x80 >> x) != 0 { 1 ^ 3 } else { 1 };
            assert_eq!(pluma(&mut c, rp, x, y), esperado);
        }
    }
    mover(&mut c, rp, 0, 6);
    g(&mut c, "Text", &[1], &[a, rp]);
    assert!((0..8).all(|y| (0..8).all(|x| pluma(&mut c, rp, x, y) == 1)));
}

#[test]
fn complement_en_nativo_y_esquinas_de_una_polilinea() {
    let mut c = caja();
    let (bm, rp) = nativo(&mut c, 16, 16);
    g(&mut c, "SetRast", &[0], &[0, rp]);
    let antes = c.1.lienzo(bm).unwrap().pixel(4, 4);
    g(&mut c, "SetDrMd", &[drmd::COMPLEMENT as u32], &[0, rp]);
    rect(&mut c, rp, 2, 2, 6, 6);
    assert_eq!(c.1.lienzo(bm).unwrap().pixel(4, 4), antes ^ 0x00FF_FFFF);
    rect(&mut c, rp, 2, 2, 6, 6);
    assert_eq!(c.1.lienzo(bm).unwrap().pixel(4, 4), antes);
    // Un cuadrado con Draw encadenados en COMPLEMENT: las esquinas de en medio no se invierten dos veces (la de
    // cierre, que es la del Move, sí: provisional, a comprobar con el oráculo).
    mover(&mut c, rp, 1, 1);
    for (x, y) in [(10, 1), (10, 10), (1, 10), (1, 1)] {
        draw(&mut c, rp, x, y);
    }
    let l = c.1.lienzo(bm).unwrap();
    for (x, y) in [(10, 1), (10, 10), (1, 10), (5, 1), (10, 5)] {
        assert_eq!(l.pixel(x, y), antes ^ 0x00FF_FFFF, "({x},{y})");
    }
}

#[test]
fn escribir_y_leer_puntos() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 3, 16, 8);
    pen(&mut c, rp, 5);
    assert_eq!(g(&mut c, "WritePixel", &[3, 4], &[0, rp]), 0);
    assert_eq!(pluma(&mut c, rp, 3, 4), 5);
    assert_eq!(g(&mut c, "WritePixel", &[16, 4], &[0, rp]) as i32, -1);
    assert_eq!(g(&mut c, "WritePixel", &[(-1i32) as u32, 0], &[0, rp]) as i32, -1);
    assert_eq!(pluma(&mut c, rp, 16, 0), -1);
    assert_eq!(pluma(&mut c, rp, 0, 8), -1);
    // En nativo, ReadPixel da la pluma del color.
    let (_, rp) = nativo(&mut c, 8, 8);
    pen(&mut c, rp, 3);
    g(&mut c, "WritePixel", &[1, 1], &[0, rp]);
    assert_eq!(pluma(&mut c, rp, 1, 1), 3);
}

#[test]
fn mascara_de_planos() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 2, 16, 4);
    g(&mut c, "SetWriteMask", &[1], &[rp]);
    pen(&mut c, rp, 3);
    rect(&mut c, rp, 0, 0, 3, 3);
    assert_eq!(pluma(&mut c, rp, 1, 1), 1, "solo el plano 0");
}

#[test]
fn patron_de_linea_y_de_area() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 16, 4);
    pen(&mut c, rp, 1);
    // SetDrPt (una macro): LinePtrn y el contador.
    e16(&mut c, rp + rastport::LINEPTRN, 0xF0F0);
    mover(&mut c, rp, 0, 0);
    draw(&mut c, rp, 15, 0);
    assert_eq!(ver(&mut c, rp, 16, 1), ["####....####...."]);
    // SetAfPt: AreaPtrn con 2 filas.
    let pat = reservar(&mut c, 4);
    e16(&mut c, pat, 0xAAAA);
    e16(&mut c, pat + 2, 0x5555);
    e32(&mut c, rp + rastport::AREAPTRN, pat);
    c.0.mem.leer_mut(rp + rastport::AREAPTSZ, 1).unwrap()[0] = 1;
    rect(&mut c, rp, 0, 2, 7, 3);
    assert_eq!(ver(&mut c, rp, 8, 4)[2..], ["#.#.#.#.", ".#.#.#.#"]);
}

#[test]
fn texto_largo_y_fuentes() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 64, 8);
    let s = cadena(&mut c, "Hola");
    assert_eq!(g(&mut c, "TextLength", &[4], &[s, rp]), 32);
    e16(&mut c, rp + rastport::TXSPACING, 2);
    assert_eq!(g(&mut c, "TextLength", &[4], &[s, rp]), 40);
    assert_eq!(g(&mut c, "TextLength", &[0], &[s, rp]), 0);
    // OpenFont: el nombre sin mirar mayúsculas y el tamaño más cercano.
    let topaz = c.1.topaz();
    let nombre = cadena(&mut c, "Topaz.font");
    let ta = reservar(&mut c, 8);
    e32(&mut c, ta, nombre);
    e16(&mut c, ta + 4, 9);
    let antes = l16(&c, topaz + textfont::TF_ACCESSORS);
    assert_eq!(g(&mut c, "OpenFont", &[], &[ta]), topaz);
    assert_eq!(l16(&c, topaz + textfont::TF_ACCESSORS), antes + 1);
    g(&mut c, "CloseFont", &[], &[0, topaz]);
    assert_eq!(l16(&c, topaz + textfont::TF_ACCESSORS), antes);
    let otra = cadena(&mut c, "nada.font");
    e32(&mut c, ta, otra);
    assert_eq!(g(&mut c, "OpenFont", &[], &[ta]), 0);
    // AskFont.
    let ta2 = reservar(&mut c, 8);
    g(&mut c, "AskFont", &[], &[ta2, rp]);
    assert_eq!(c.0.leer_cadena(l32(&c, ta2)).unwrap(), "topaz.font");
    assert_eq!(l16(&c, ta2 + 4), 8);
    // SetFont pone las medidas.
    e16(&mut c, rp + rastport::TXHEIGHT, 0);
    g(&mut c, "SetFont", &[], &[topaz, rp]);
    assert_eq!(l16(&c, rp + rastport::TXHEIGHT), 8);
    assert_eq!(l16(&c, rp + rastport::TXBASELINE), 6);
}

#[test]
fn paletas() {
    let mut c = caja();
    let s = c.1.workbench();
    let vp = s + 44;
    let cm = l32(&c, vp + 4);
    g(&mut c, "SetRGB4", &[5, 15, 8, 0], &[vp]);
    assert_eq!(g(&mut c, "GetRGB4", &[5], &[cm]), 0xF80);
    g(&mut c, "SetRGB32", &[6, 0x1200_0000, 0x3400_0000, 0x5600_0000], &[vp]);
    let t = reservar(&mut c, 24);
    g(&mut c, "GetRGB32", &[5, 2, 0], &[cm, t]);
    assert_eq!(l32(&c, t), 0xFFFF_FFFF);
    assert_eq!(l32(&c, t + 4), 0x8888_8888);
    assert_eq!(l32(&c, t + 12), 0x1212_1212);
    assert_eq!(l32(&c, t + 20), 0x5656_5656);
    // Dibujar con la pluma 6 en un mapa nativo da ese color.
    let (bm, rp) = nativo(&mut c, 4, 4);
    pen(&mut c, rp, 6);
    g(&mut c, "WritePixel", &[0, 0], &[0, rp]);
    assert_eq!(c.1.lienzo(bm).unwrap().pixel(0, 0), 0xFF12_3456);
    // LoadRGB4 y LoadRGB32.
    let tabla = reservar(&mut c, 4);
    e16(&mut c, tabla, 0x123);
    e16(&mut c, tabla + 2, 0x0F0);
    g(&mut c, "LoadRGB4", &[2], &[vp, tabla]);
    assert_eq!(g(&mut c, "GetRGB4", &[1], &[cm]), 0x0F0);
    let t32 = reservar(&mut c, 20);
    e32(&mut c, t32, 1 << 16 | 7);
    e32(&mut c, t32 + 4, 0xFF00_0000);
    e32(&mut c, t32 + 8, 0);
    e32(&mut c, t32 + 12, 0x8000_0000);
    e32(&mut c, t32 + 16, 0);
    g(&mut c, "LoadRGB32", &[], &[vp, t32]);
    assert_eq!(g(&mut c, "GetRGB4", &[7], &[cm]), 0xF08);
    assert_eq!(g(&mut c, "GetRGB4", &[300], &[cm]) as i32, -1);
    // FindColor y ObtainPen.
    assert_eq!(g(&mut c, "FindColor", &[0, 0xFF00_0000, 0, 0x8000_0000, 255], &[0, 0, 0, cm]), 7);
    let p = g(&mut c, "ObtainPen", &[(-1i32) as u32, 0x1100_0000, 0x2200_0000, 0x3300_0000, 0], &[cm]) as i32;
    assert!(p >= 4);
    assert_eq!(g(&mut c, "GetRGB4", &[p as u32], &[cm]), 0x123);
    let q = g(&mut c, "ObtainPen", &[p as u32, 0, 0, 0, 0], &[cm]) as i32;
    assert_eq!(q, -1, "ya está cogida");
    g(&mut c, "ReleasePen", &[p as u32], &[cm]);
    // GetColorMap y FreeColorMap.
    let cm2 = g(&mut c, "GetColorMap", &[32], &[]);
    assert_eq!(l16(&c, cm2 + 2), 32);
    g(&mut c, "SetRGB4CM", &[3, 1, 2, 3], &[cm2]);
    assert_eq!(g(&mut c, "GetRGB4", &[3], &[cm2]), 0x123);
    g(&mut c, "FreeColorMap", &[], &[cm2]);
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn copias_de_bloques() {
    let mut c = caja();
    // Un dibujo en un planar, copiado a uno nativo: las plumas pasan por la paleta.
    let (bp, rpp) = planar(&mut c, 2, 16, 8);
    pen(&mut c, rpp, 3);
    rect(&mut c, rpp, 0, 0, 3, 3);
    pen(&mut c, rpp, 2);
    rect(&mut c, rpp, 4, 0, 7, 3);
    let (bn, rpn) = nativo(&mut c, 16, 8);
    assert_eq!(g(&mut c, "BltBitMap", &[0, 0, 2, 2, 8, 4, 0xC0, 0xFF], &[bp, bn]), 8);
    let l = c.1.lienzo(bn).unwrap();
    assert_eq!(l.pixel(2, 2), 0xFF66_88BB);
    assert_eq!(l.pixel(6, 2), 0xFFFF_FFFF);
    assert_eq!(l.pixel(10, 2), 0xFFAA_AAAA);
    // De nativo a planar: el color más parecido.
    let (bp2, rpp2) = planar(&mut c, 2, 16, 8);
    g(&mut c, "BltBitMap", &[2, 2, 0, 0, 8, 4, 0xC0, 0xFF], &[bn, bp2]);
    assert_eq!(pluma(&mut c, rpp2, 0, 0), 3);
    assert_eq!(pluma(&mut c, rpp2, 4, 0), 2);
    // Dentro del mismo mapa, solapado: como si se copiara de una vez.
    g(&mut c, "BltBitMap", &[0, 0, 2, 0, 8, 4, 0xC0, 0xFF], &[bp, bp]);
    assert_eq!(ver(&mut c, rpp, 10, 1), ["##########"]);
    assert_eq!(pluma(&mut c, rpp, 2, 0), 3);
    assert_eq!(pluma(&mut c, rpp, 6, 0), 2);
    // Minitérmino 0x50: invierte el destino; recortado a los dos mapas.
    g(&mut c, "BltBitMap", &[12, 0, 12, 0, 100, 100, 0x50, 0xFF], &[bp, bp]);
    assert_eq!(pluma(&mut c, rpp, 15, 7), 3);
    assert_eq!(pluma(&mut c, rpp, 11, 0), 0);
    // ClipBlit entre RastPorts.
    let (_, rpd) = planar(&mut c, 2, 16, 8);
    g(&mut c, "ClipBlit", &[0, 0, 0, 0, 4, 4, 0xC0], &[rpp, rpd]);
    assert_eq!(pluma(&mut c, rpd, 0, 0), 3);
    assert_eq!(pluma(&mut c, rpd, 4, 0), 0);
    let _ = (rpn, bp2);
}

#[test]
fn scroll_raster_template_y_chunky() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 2, 16, 4);
    pen(&mut c, rp, 1);
    rect(&mut c, rp, 0, 0, 1, 3);
    g(&mut c, "SetBPen", &[2], &[0, rp]);
    // Dos a la izquierda... al revés: dx = -2 mueve el contenido a la derecha.
    g(&mut c, "ScrollRaster", &[(-2i32) as u32, 0, 0, 0, 7, 3], &[0, rp]);
    assert_eq!((0..8).map(|x| pluma(&mut c, rp, x, 0)).collect::<Vec<_>>(), [2, 2, 1, 1, 0, 0, 0, 0]);
    // BltTemplate: dos filas de una plantilla de 16 bits por fila, desde el bit 4.
    let t = reservar(&mut c, 4);
    e16(&mut c, t, 0x0F00);
    e16(&mut c, t + 2, 0x0A00);
    g(&mut c, "SetABPenDrMd", &[3, 0, drmd::JAM1 as u32], &[0, rp]);
    g(&mut c, "BltTemplate", &[4, 2, 0, 2, 4, 2], &[t, rp]);
    assert_eq!((0..4).map(|x| pluma(&mut c, rp, x, 2)).collect::<Vec<_>>(), [3, 3, 3, 3]);
    assert_eq!((0..4).map(|x| pluma(&mut c, rp, x, 3)).collect::<Vec<_>>(), [3, 2, 3, 1], "JAM1: los ceros no tocan");
    // WriteChunkyPixels: una pluma por byte, sin modo.
    let ch = reservar(&mut c, 8);
    c.0.mem.leer_mut(ch, 6).unwrap().copy_from_slice(&[1, 2, 3, 3, 2, 1]);
    g(&mut c, "WriteChunkyPixels", &[10, 0, 12, 1, 3], &[rp, 0, ch]);
    assert_eq!((10..13).map(|x| pluma(&mut c, rp, x, 0)).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!((10..13).map(|x| pluma(&mut c, rp, x, 1)).collect::<Vec<_>>(), [3, 2, 1]);
}

#[test]
fn mapas_de_alloc_bit_map() {
    let mut c = caja();
    // Planar entrelazado.
    let bm = g(&mut c, "AllocBitMap", &[20, 6, 3, bitmap::BMF_INTERLEAVED | bitmap::BMF_CLEAR], &[0]);
    assert_ne!(bm, 0);
    assert_eq!(l16(&c, bm + bitmap::BYTESPERROW), 12);
    assert_eq!(l32(&c, bm + 12) - l32(&c, bm + 8), 4);
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, bitmap::BMA_WIDTH], &[bm]), 32);
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, bitmap::BMA_DEPTH], &[bm]), 3);
    assert_ne!(g(&mut c, "GetBitMapAttr", &[0, bitmap::BMA_FLAGS], &[bm]) & bitmap::BMF_INTERLEAVED, 0);
    let rp = rastport(&mut c, bm);
    pen(&mut c, rp, 6);
    rect(&mut c, rp, 2, 1, 4, 3);
    assert_eq!(pluma(&mut c, rp, 3, 2), 6);
    assert_eq!(pluma(&mut c, rp, 5, 2), 0);
    g(&mut c, "FreeBitMap", &[], &[bm]);
    // Nativo, por profundidad o por amigo.
    let n = g(&mut c, "AllocBitMap", &[50, 40, 24, 0], &[0]);
    assert!(c.1.lienzo(n).is_some());
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, bitmap::BMA_DEPTH], &[n]), 32);
    assert_eq!(g(&mut c, "GetBitMapAttr", &[0, bitmap::BMA_HEIGHT], &[n]), 40);
    let amigo = g(&mut c, "AllocBitMap", &[10, 10, 4, 0], &[n]);
    assert!(c.1.lienzo(amigo).is_some());
    g(&mut c, "FreeBitMap", &[], &[amigo]);
    g(&mut c, "FreeBitMap", &[], &[n]);
    assert!(c.1.lienzo(n).is_none());
    assert!(c.1.avisos().is_empty(), "{:?}", c.1.avisos());
}

#[test]
fn planos_cero_y_unos() {
    let mut c = caja();
    let (bm, rp) = planar(&mut c, 2, 16, 4);
    // El plano 1 a "todo unos" (0xFFFFFFFF): se lee como 1 y no se escribe.
    e32(&mut c, bm + 12, 0xFFFF_FFFF);
    pen(&mut c, rp, 1);
    g(&mut c, "WritePixel", &[0, 0], &[0, rp]);
    assert_eq!(pluma(&mut c, rp, 0, 0), 3);
    e32(&mut c, bm + 12, 0);
    assert_eq!(pluma(&mut c, rp, 0, 0), 1);
    assert!(c.1.avisos().is_empty());
}

/// Lo que pedían MUI 5 y Tachyon (el registro de lo que falta): `TextExtent` y `SetRPAttrsA`.
#[test]
fn text_extent_y_set_rp_attrs() {
    let mut c = caja();
    let (_, rp) = planar(&mut c, 1, 64, 8);
    let s = cadena(&mut c, "Hola");
    let te = reservar(&mut c, 12);
    g(&mut c, "TextExtent", &[4], &[s, rp, te]);
    // topaz/8: 8 de ancho por letra, 8 de alto, la línea base en 6.
    assert_eq!(l16(&c, te), 32);
    assert_eq!(l16(&c, te + 2), 8);
    assert_eq!(l16(&c, te + 4) as i16, 0);
    assert_eq!(l16(&c, te + 6) as i16, -6);
    assert_eq!(l16(&c, te + 8) as i16, 31);
    assert_eq!(l16(&c, te + 10) as i16, 1);
    // Sin texto: ancho 0, el rectángulo sin anchura.
    g(&mut c, "TextExtent", &[0], &[s, rp, te]);
    assert_eq!(l16(&c, te), 0);
    // SetRPAttrsA: plumas y modo; lo que no se conoce, nada.
    let tags = reservar(&mut c, 40);
    for (k, (t, v)) in [(0x8000_0002u32, 3u32), (0x8000_0003, 2), (0x8000_0004, 1), (0x8000_0007, 9), (0, 0)].iter().enumerate() {
        e32(&mut c, tags + 8 * k as u32, *t);
        e32(&mut c, tags + 8 * k as u32 + 4, *v);
    }
    g(&mut c, "SetRPAttrsA", &[], &[rp, tags]);
    assert_eq!(g(&mut c, "GetAPen", &[], &[rp]), 3);
    assert_eq!(g(&mut c, "GetBPen", &[], &[rp]), 2);
    assert_eq!(g(&mut c, "GetDrMd", &[], &[rp]), 1);
    assert!(c.1.tomar_avisos().is_empty());
    // Hostil: un TextExtent fuera de la memoria y unas etiquetas fuera: avisos, nada roto.
    g(&mut c, "TextExtent", &[4], &[s, rp, 0xFFFF_0000]);
    g(&mut c, "SetRPAttrsA", &[], &[rp, 0xFFFF_0000]);
    assert_eq!(c.1.tomar_avisos().len(), 2);
}

/// MUI 5 pide el modo de la pantalla (`GetDisplayInfoData`) y diskfont compara fuentes (`WeighTAMatch`).
#[test]
fn display_info_y_weigh_ta_match() {
    let mut c = caja();
    let buf = reservar(&mut c, 128);
    // DTAG_DISP: 56 bytes, su cabecera y 8 bits por color; con un búfer corto, solo lo que cabe.
    assert_eq!(g(&mut c, "GetDisplayInfoData", &[128, 0x8000_0000, 0x29000], &[0, buf]), 56);
    assert_eq!(l32(&c, buf), 0x8000_0000);
    assert_eq!(l32(&c, buf + 4), 0x29000);
    assert_eq!(l8(&c, buf + 40), 8);
    assert_eq!(g(&mut c, "GetDisplayInfoData", &[20, 0x8000_1000, 0], &[0, buf]), 20);
    // DTAG_DIMS: el ancho nominal es el del escritorio (640 en la caja de las pruebas).
    assert_eq!(g(&mut c, "GetDisplayInfoData", &[128, 0x8000_1000, 0], &[0, buf]), 88);
    assert_eq!(l16(&c, buf + 26 + 4), 639);
    assert_eq!(g(&mut c, "GetDisplayInfoData", &[128, 0x8000_9000, 0], &[0, buf]), 0);
    // WeighTAMatch: la misma, 32767; un punto de alto menos, menos; el nombre no cuenta (diskfont pasa una TextFont
    // desplazada, con basura donde iría el nombre).
    let topaz = cadena(&mut c, "topaz.font");
    let topaz2 = cadena(&mut c, "Topaz.font");
    let otra = cadena(&mut c, "xen.font");
    let ta = |c: &mut Caja, n: u32, y: u16| {
        let d = reservar(c, 8);
        e32(c, d, n);
        e16(c, d + 4, y);
        d
    };
    let (a, b, d, e) = (ta(&mut c, topaz, 8), ta(&mut c, topaz2, 8), ta(&mut c, topaz, 9), ta(&mut c, otra, 8));
    assert_eq!(g(&mut c, "WeighTAMatch", &[], &[a, b, 0]), 32767);
    assert!(g(&mut c, "WeighTAMatch", &[], &[a, d, 0]) < 32767);
    assert_eq!(g(&mut c, "WeighTAMatch", &[], &[a, e, 0]), 32767);
    let basura = ta(&mut c, 52, 8);
    assert_eq!(g(&mut c, "WeighTAMatch", &[], &[a, basura, 0]), 32767);
    assert!(c.1.tomar_avisos().is_empty());
    // Hostil: un búfer fuera de la memoria, un TextAttr fuera: avisos, 0.
    assert_eq!(g(&mut c, "GetDisplayInfoData", &[56, 0x8000_0000, 0], &[0, 0xFFFF_0000]), 0);
    assert_eq!(g(&mut c, "WeighTAMatch", &[], &[0xFFFF_0000, a, 0]), 0);
    assert_eq!(c.1.tomar_avisos().len(), 2);
}
