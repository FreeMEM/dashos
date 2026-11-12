//! La disposición de las estructuras (los números son los de `offsetof` en el compilador del 68k con los
//! includes del NDK), las bibliotecas en la lista de exec, sus tablas de saltos y la fuente.

mod comun;

use caja_exec::estructuras::{library, node, nt};
use caja_exec::{Accion, Memoria, lvo as exec_lvo};
use caja_intuition::estructuras::*;
use caja_intuition::lvo;
use comun::*;

#[test]
fn bitmap_rastport_y_texto() {
    assert_eq!(bitmap::TAMANO, 40);
    assert_eq!((bitmap::BYTESPERROW, bitmap::ROWS, bitmap::FLAGS, bitmap::DEPTH, bitmap::PAD), (0, 2, 4, 5, 6));
    assert_eq!(bitmap::PLANES, 8);
    assert_eq!(rastport::TAMANO, 100);
    let rp = [
        (rastport::LAYER, 0),
        (rastport::BITMAP, 4),
        (rastport::AREAPTRN, 8),
        (rastport::TMPRAS, 12),
        (rastport::AREAINFO, 16),
        (rastport::GELSINFO, 20),
        (rastport::MASK, 24),
        (rastport::FGPEN, 25),
        (rastport::BGPEN, 26),
        (rastport::AOLPEN, 27),
        (rastport::DRAWMODE, 28),
        (rastport::AREAPTSZ, 29),
        (rastport::LINPATCNT, 30),
        (rastport::DUMMY, 31),
        (rastport::FLAGS, 32),
        (rastport::LINEPTRN, 34),
        (rastport::CP_X, 36),
        (rastport::CP_Y, 38),
        (rastport::MINTERMS, 40),
        (rastport::PENWIDTH, 48),
        (rastport::PENHEIGHT, 50),
        (rastport::FONT, 52),
        (rastport::ALGOSTYLE, 56),
        (rastport::TXFLAGS, 57),
        (rastport::TXHEIGHT, 58),
        (rastport::TXWIDTH, 60),
        (rastport::TXBASELINE, 62),
        (rastport::TXSPACING, 64),
        (rastport::RP_USER, 66),
        (rastport::LONGRESERVED, 70),
        (rastport::WORDRESERVED, 78),
        (rastport::RESERVED, 92),
    ];
    for (a, b) in rp {
        assert_eq!(a, b);
    }
    assert_eq!(textattr::TAMANO, 8);
    assert_eq!((textattr::TA_YSIZE, textattr::TA_STYLE, textattr::TA_FLAGS), (4, 6, 7));
    assert_eq!(textfont::TAMANO, 52);
    let tf = [
        (textfont::TF_YSIZE, 20),
        (textfont::TF_STYLE, 22),
        (textfont::TF_FLAGS, 23),
        (textfont::TF_XSIZE, 24),
        (textfont::TF_BASELINE, 26),
        (textfont::TF_BOLDSMEAR, 28),
        (textfont::TF_ACCESSORS, 30),
        (textfont::TF_LOCHAR, 32),
        (textfont::TF_HICHAR, 33),
        (textfont::TF_CHARDATA, 34),
        (textfont::TF_MODULO, 38),
        (textfont::TF_CHARLOC, 40),
        (textfont::TF_CHARSPACE, 44),
        (textfont::TF_CHARKERN, 48),
    ];
    for (a, b) in tf {
        assert_eq!(a, b);
    }
}

#[test]
fn colormap_viewport_capa_y_gfxbase() {
    assert_eq!(colormap::TAMANO, 52);
    assert_eq!((colormap::TYPE, colormap::COUNT, colormap::COLORTABLE, colormap::LOWCOLORBITS), (1, 2, 4, 12));
    assert_eq!((colormap::CM_VP, colormap::VPMODEID, colormap::PALEXTRA, colormap::BP_1_BASE), (20, 36, 40, 50));
    assert_eq!(viewport::TAMANO, 40);
    assert_eq!((viewport::COLORMAP, viewport::DWIDTH, viewport::DHEIGHT, viewport::MODES), (4, 24, 26, 32));
    assert_eq!(viewport::RASINFO, 36);
    assert_eq!(layer::TAMANO, 160);
    assert_eq!((layer::RP, layer::BOUNDS, layer::FLAGS, layer::WINDOW), (12, 16, 30, 40));
    assert_eq!((layer::LAYERINFO, layer::LOCK, layer::BACKFILL, layer::WIDTH, layer::HEIGHT), (68, 72, 118, 134, 136));
    assert_eq!(layer::DAMAGELIST, 156);
    assert_eq!(layer_info::TAMANO, 102);
    assert_eq!(gfxbase::TAMANO, 552);
    assert_eq!((gfxbase::ACTIVIEW, gfxbase::TEXTFONTS, gfxbase::DEFAULTFONT), (34, 140, 154));
    assert_eq!((gfxbase::DISPLAYFLAGS, gfxbase::CHIPREVBITS0, gfxbase::MEMTYPE), (206, 236, 237));
    assert_eq!((gfxbase::NORMALDISPLAYROWS, gfxbase::NORMALDISPLAYCOLUMNS), (216, 218));
    assert_eq!((gfxbase::MONITORLIST, gfxbase::BP3BITS, gfxbase::GFXFLAGS), (384, 483, 498));
}

#[test]
fn screen_window_y_mensajes() {
    assert_eq!(screen::TAMANO, 346);
    let s = [
        (screen::FIRSTWINDOW, 4),
        (screen::LEFTEDGE, 8),
        (screen::WIDTH, 12),
        (screen::MOUSEY, 16),
        (screen::MOUSEX, 18),
        (screen::FLAGS, 20),
        (screen::TITLE, 22),
        (screen::DEFAULTTITLE, 26),
        (screen::BARHEIGHT, 30),
        (screen::WBORTOP, 35),
        (screen::WBORBOTTOM, 38),
        (screen::FONT, 40),
        (screen::VIEWPORT, 44),
        (screen::RASTPORT, 84),
        (screen::BITMAP, 184),
        (screen::LAYERINFO, 224),
        (screen::FIRSTGADGET, 326),
        (screen::DETAILPEN, 330),
        (screen::SAVECOLOR0, 332),
        (screen::BARLAYER, 334),
        (screen::EXTDATA, 338),
        (screen::USERDATA, 342),
    ];
    for (a, b) in s {
        assert_eq!(a, b);
    }
    assert_eq!(window::TAMANO, 136);
    let w = [
        (window::LEFTEDGE, 4),
        (window::WIDTH, 8),
        (window::MOUSEY, 12),
        (window::MOUSEX, 14),
        (window::MINWIDTH, 16),
        (window::MAXHEIGHT, 22),
        (window::FLAGS, 24),
        (window::MENUSTRIP, 28),
        (window::TITLE, 32),
        (window::REQCOUNT, 44),
        (window::WSCREEN, 46),
        (window::RPORT, 50),
        (window::BORDERLEFT, 54),
        (window::BORDERBOTTOM, 57),
        (window::BORDERRPORT, 58),
        (window::FIRSTGADGET, 62),
        (window::POINTER, 74),
        (window::IDCMPFLAGS, 82),
        (window::USERPORT, 86),
        (window::WINDOWPORT, 90),
        (window::MESSAGEKEY, 94),
        (window::DETAILPEN, 98),
        (window::CHECKMARK, 100),
        (window::SCREENTITLE, 104),
        (window::GZZMOUSEX, 108),
        (window::GZZHEIGHT, 114),
        (window::EXTDATA, 116),
        (window::USERDATA, 120),
        (window::WLAYER, 124),
        (window::IFONT, 128),
        (window::MOREFLAGS, 132),
    ];
    for (a, b) in w {
        assert_eq!(a, b);
    }
    assert_eq!((newwindow::TAMANO, newwindow::TAMANO_EXT, newwindow::EXTENSION), (48, 52, 48));
    assert_eq!((newwindow::IDCMPFLAGS, newwindow::FLAGS, newwindow::TITLE, newwindow::SCREEN), (10, 14, 26, 30));
    assert_eq!((newwindow::MINWIDTH, newwindow::MAXHEIGHT, newwindow::TYPE), (38, 44, 46));
    assert_eq!((intuimessage::TAMANO, intuimessage::TAMANO_EXT), (52, 56));
    let m = [
        (intuimessage::CLASS, 20),
        (intuimessage::CODE, 24),
        (intuimessage::QUALIFIER, 26),
        (intuimessage::IADDRESS, 28),
        (intuimessage::MOUSEX, 32),
        (intuimessage::MOUSEY, 34),
        (intuimessage::SECONDS, 36),
        (intuimessage::MICROS, 40),
        (intuimessage::IDCMPWINDOW, 44),
        (intuimessage::SPECIALLINK, 48),
    ];
    for (a, b) in m {
        assert_eq!(a, b);
    }
    assert_eq!(drawinfo::TAMANO, 50);
    assert_eq!((drawinfo::DRI_PENS, drawinfo::DRI_FONT, drawinfo::DRI_DEPTH, drawinfo::DRI_FLAGS), (4, 8, 12, 18));
    assert_eq!(intuitionbase::TAMANO, 80);
    assert_eq!((intuitionbase::ACTIVEWINDOW, intuitionbase::ACTIVESCREEN, intuitionbase::FIRSTSCREEN), (52, 56, 60));
    assert_eq!((intuitionbase::MOUSEY, intuitionbase::MOUSEX, intuitionbase::SECONDS), (68, 70, 72));
}

#[test]
fn etiquetas_y_banderas() {
    assert_eq!(wa::LEFT, 0x8000_0064);
    assert_eq!(wa::TITLE, 0x8000_006E);
    assert_eq!(wa::IDCMP, 0x8000_006A);
    assert_eq!(wa::FLAGS, 0x8000_006B);
    assert_eq!(wa::INNERWIDTH, 0x8000_0076);
    assert_eq!(wa::PUBSCREENNAME, 0x8000_0078);
    assert_eq!(wa::CLOSEGADGET, 0x8000_0084);
    assert_eq!(wa::ACTIVATE, 0x8000_0089);
    assert_eq!(wa::NOTIFYDEPTH, 0x8000_0095);
    assert_eq!(idcmp::CLOSEWINDOW, 0x200);
    assert_eq!(idcmp::VANILLAKEY, 0x0020_0000);
    assert_eq!(wflg::ACTIVATE, 0x1000);
    assert_eq!(ie::SELECTUP, 0xE8);
}

#[test]
fn tablas_de_saltos() {
    assert_eq!(lvo::GRAPHICS.len(), 172);
    assert_eq!(lvo::INTUITION.len(), 134);
    let lvo_de = |t: &[(&str, &str)], n: &str| lvo::lvo(t.iter().position(|x| x.0 == n).unwrap());
    // Algunos LVO conocidos de los includes de 3.1.
    assert_eq!(lvo_de(&lvo::GRAPHICS, "BltBitMap"), -30);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "Text"), -60);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "Move"), -240);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "Draw"), -246);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "RectFill"), -306);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "SetAPen"), -342);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "AllocBitMap"), -918);
    assert_eq!(lvo_de(&lvo::GRAPHICS, "WriteChunkyPixels"), -1056);
    assert_eq!(lvo_de(&lvo::INTUITION, "CloseWindow"), -72);
    assert_eq!(lvo_de(&lvo::INTUITION, "OpenWindow"), -204);
    assert_eq!(lvo_de(&lvo::INTUITION, "LockPubScreen"), -510);
    assert_eq!(lvo_de(&lvo::INTUITION, "OpenWindowTagList"), -606);
    assert_eq!(lvo_de(&lvo::INTUITION, "HelpControl"), -828);
}

#[test]
fn las_bibliotecas_se_abren_con_exec() {
    let mut c = caja();
    for (nombre, base) in [("graphics.library", c.1.gfx_base()), ("intuition.library", c.1.intuition_base())] {
        let s = cadena(&mut c, nombre);
        let mut r = regs(&[39], &[0, s]);
        let a = c.0.llamar(exec_lvo::por_nombre("OpenLibrary").unwrap(), &mut r);
        assert_eq!(a, Accion::Seguir);
        assert_eq!(r.d[0], base, "{nombre}");
        assert_eq!(l16(&c, base + library::LIB_VERSION), 40);
        assert_eq!(l8(&c, base + node::LN_TYPE), nt::LIBRARY);
        // La entrada de Move es nativa y la caja la lleva a graphics.
        if nombre == "graphics.library" {
            let e = base - 240;
            assert_eq!(l16(&c, e), 0xA0FF);
            let n = l16(&c, e + 2);
            assert_eq!(Some(n), c.1.numero_graphics("Move"));
            assert_eq!(c.0.ajena(n), Some((base, -240)));
            let mut r = regs(&[], &[]);
            assert_eq!(c.0.llamar(n, &mut r), Accion::Ajena);
        }
    }
    // Un número que no es de ellas.
    let mut r = regs(&[], &[]);
    assert_eq!(c.1.llamar(&mut c.0, 300, &mut r), Accion::Ajena);
}

#[test]
fn gfxbase_y_fuente() {
    let c = caja();
    let b = c.1.gfx_base();
    let f = c.1.topaz();
    assert_eq!(l32(&c, b + gfxbase::DEFAULTFONT), f);
    assert_eq!(l8(&c, b + gfxbase::CHIPREVBITS0) & 0x0C, 0x0C, "AGA");
    // La fuente está en la lista de fuentes del sistema, con su nombre.
    let lista = c.0.nodos(b + gfxbase::TEXTFONTS).unwrap();
    assert_eq!(lista, vec![f]);
    assert_eq!(c.0.leer_cadena(l32(&c, f + node::LN_NAME)).unwrap(), "topaz.font");
    assert_eq!(l8(&c, f + node::LN_TYPE), nt::FONT);
    assert_eq!(l16(&c, f + textfont::TF_YSIZE), 8);
    assert_eq!(l16(&c, f + textfont::TF_XSIZE), 8);
    assert_eq!(l16(&c, f + textfont::TF_BASELINE), 6);
    assert_eq!((l8(&c, f + textfont::TF_LOCHAR), l8(&c, f + textfont::TF_HICHAR)), (32, 255));
    // Cada carácter tiene su sitio de 8 de ancho en CharData.
    let loc = l32(&c, f + textfont::TF_CHARLOC);
    let modulo = l16(&c, f + textfont::TF_MODULO) as u32;
    for ch in 32..=256u32 {
        let v = l32(&c, loc + (ch - 32) * 4);
        assert_eq!(v & 0xFFFF, 8);
        assert!((v >> 16) / 8 < modulo);
    }
    // La 'A' y la 'B' son distintas, y el espacio está vacío.
    let datos = l32(&c, f + textfont::TF_CHARDATA);
    let glifo = |ch: u32| -> Vec<u8> {
        let off = l32(&c, loc + (ch - 32) * 4) >> 16;
        (0..8).map(|y| l8(&c, datos + y * modulo + off / 8)).collect()
    };
    assert_ne!(glifo('A' as u32), glifo('B' as u32));
    assert!(glifo(' ' as u32).iter().all(|&b| b == 0));
    // Lo de castellano tiene dibujo propio; lo que no, la caja (la misma que el "no definido").
    assert_ne!(glifo(0xF1), glifo(256));
    assert_ne!(glifo(0xBF), glifo(256));
    assert_eq!(glifo(0x80), glifo(256));
    // Todos los de ASCII tienen dibujo propio, distinto de la caja, y los visibles algo dibujado.
    for ch in 33..127u32 {
        assert_ne!(glifo(ch), glifo(256), "{}", ch as u8 as char);
        assert!(glifo(ch).iter().any(|&b| b != 0));
    }
}

#[test]
fn pantalla_de_workbench() {
    let c = caja();
    let s = c.1.workbench();
    assert_eq!((l16(&c, s + screen::WIDTH), l16(&c, s + screen::HEIGHT)), (640, 480));
    assert_ne!(l16(&c, s + screen::FLAGS) & screen::WBENCHSCREEN, 0);
    assert_eq!(c.0.leer_cadena(l32(&c, s + screen::TITLE)).unwrap(), "Workbench Screen");
    assert_eq!((l8(&c, s + screen::WBORLEFT), l8(&c, s + screen::WBORTOP)), (4, 2));
    // La TextAttr de la pantalla es topaz/8.
    let ta = l32(&c, s + screen::FONT);
    assert_eq!(c.0.leer_cadena(l32(&c, ta)).unwrap(), "topaz.font");
    assert_eq!(l16(&c, ta + 4), 8);
    // ViewPort con su ColorMap, y el RastPort apunta al BitMap de dentro.
    let cm = l32(&c, s + screen::VIEWPORT + viewport::COLORMAP);
    assert_ne!(cm, 0);
    assert_eq!(l16(&c, cm + colormap::COUNT), 256);
    assert_eq!(l32(&c, s + screen::RASTPORT + rastport::BITMAP), s + screen::BITMAP);
    assert!(c.1.lienzo(s + screen::BITMAP).is_some());
    let ib = c.1.intuition_base();
    assert_eq!(l32(&c, ib + intuitionbase::FIRSTSCREEN), s);
    assert_eq!(l32(&c, ib + intuitionbase::ACTIVESCREEN), s);
    // Los colores de 3.1 en las cuatro primeras plumas (ColorTable de 12 bits).
    let t = l32(&c, cm + colormap::COLORTABLE);
    let v: Vec<u16> = (0..4).map(|i| c.0.mem.leer16(t + 2 * i).unwrap()).collect();
    assert_eq!(v, [0xAAA, 0x000, 0xFFF, 0x68B]);
}
