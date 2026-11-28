//! Pruebas de los sprites y de los choques por el bus, con una chip RAM de prueba y la imagen en un búfer
//! ([`ConPantalla`]), como las de `tests/denise.rs`.

use caja_chips::denise::{ALTO, ANCHO, ConPantalla, V0, X0, rgb};
use caja_chips::{CICLOS_POR_COLOR, CICLOS_POR_LINEA, Chips, MemoriaChip, dma};

const LINEA: u64 = CICLOS_POR_LINEA;
const CUADRO: u64 = 313 * LINEA;

const COP1LC: u32 = 0xDFF080;
const DMACON: u32 = 0xDFF096;
const CLXDAT: u32 = 0xDFF00E;
const SET: u16 = 0x8000;

// Los registros, por su desplazamiento.
const CLXCON: u16 = 0x098;
const DIWSTRT: u16 = 0x08E;
const DIWSTOP: u16 = 0x090;
const DDFSTRT: u16 = 0x092;
const DDFSTOP: u16 = 0x094;
const BPL1PTH: u16 = 0x0E0;
const SPR0PTH: u16 = 0x120;
const SPR0POS: u16 = 0x140;
const BPLCON0: u16 = 0x100;
const BPLCON2: u16 = 0x104;
const BPL1MOD: u16 = 0x108;
const BPL2MOD: u16 = 0x10A;
const COLOR00: u16 = 0x180;

/// 512 KB de chip RAM, de palabra en palabra.
struct Memoria(Vec<u16>);

impl Memoria {
    fn poner(&mut self, dir: u32, palabras: &[u16]) {
        let i = dir as usize / 2;
        self.0[i..i + palabras.len()].copy_from_slice(palabras);
    }
}

impl MemoriaChip for Memoria {
    fn leer_palabra(&mut self, dir: u32) -> u16 {
        self.0.get(dir as usize / 2).copied().unwrap_or(0)
    }

    fn escribir_palabra(&mut self, dir: u32, valor: u16) {
        if let Some(w) = self.0.get_mut(dir as usize / 2) {
            *w = valor;
        }
    }
}

fn mov(reg: u16, v: u16) -> [u16; 2] {
    [reg, v]
}

fn wait(v: u16, h: u16) -> [u16; 2] {
    [((v & 0xFF) << 8) | (h & 0xFE) | 1, 0xFFFE]
}

const FIN: [u16; 2] = [0xFFFF, 0xFFFE];

/// Dónde van la lista, los planos y las listas de los sprites (la del canal `n`, en `SPR + $400 n`).
const LISTA: u32 = 0x1_0000;
const MAPA: u32 = 0x2_0000;
const SPR: u32 = 0x4_0000;

/// Los chips, su chip RAM y la imagen.
struct Amiga {
    c: Chips,
    m: Memoria,
    cuadro: Vec<u32>,
    hora: u64,
}

impl Amiga {
    fn nuevo() -> Self {
        Amiga { c: Chips::nuevo(), m: Memoria(vec![0; 256 * 1024]), cuadro: vec![0; ANCHO * ALTO], hora: 0 }
    }

    fn con<R>(&mut self, f: impl FnOnce(&mut Chips, &mut ConPantalla, u64) -> R) -> R {
        let mut p = ConPantalla::nueva(&mut self.m, &mut self.cuadro);
        f(&mut self.c, &mut p, self.hora)
    }

    fn w(&mut self, reg: u16, v: u16) -> &mut Self {
        self.con(|c, p, h| c.escribir_con(0xDFF000 + u32::from(reg), u32::from(v), 2, h, p));
        self
    }

    fn leer(&mut self, dir: u32) -> u16 {
        self.con(|c, p, h| c.leer_con(dir, 2, h, p)) as u16
    }

    fn a(&mut self, hora: u64) -> &mut Self {
        self.hora = hora;
        self.con(|c, p, h| c.avanzar_con(h, p));
        self
    }

    /// Pone la lista y enciende el DMA del Copper, de los planos y (si `sprites`) de los sprites. Deja la hora al
    /// final del segundo cuadro.
    fn mostrar_con(&mut self, lista: &[[u16; 2]], sprites: bool) {
        let palabras: Vec<u16> = lista.iter().flatten().copied().collect();
        self.m.poner(LISTA, &palabras);
        self.con(|c, p, h| c.escribir_con(COP1LC, LISTA, 4, h, p));
        let spren = if sprites { dma::SPREN } else { 0 };
        self.con(|c, p, h| {
            c.escribir_con(DMACON, u32::from(SET | dma::DMAEN | dma::COPEN | dma::BPLEN | spren), 2, h, p)
        });
        let h = self.hora;
        self.a(h - h % CUADRO + 2 * CUADRO);
    }

    fn mostrar(&mut self, lista: &[[u16; 2]]) {
        self.mostrar_con(lista, true);
    }

    /// El píxel en la posición `x` (en píxeles de baja, la primera mitad) y la línea `v` del cuadro acabado.
    fn px(&self, x: usize, v: u64) -> u32 {
        self.cuadro[(v - V0) as usize * ANCHO + 2 * (x - X0)]
    }

    /// La segunda mitad del píxel de baja `x` (la columna de alta siguiente).
    fn px2(&self, x: usize, v: u64) -> u32 {
        self.cuadro[(v - V0) as usize * ANCHO + 2 * (x - X0) + 1]
    }
}

/// Un sprite de una lista: `HSTART`, `VSTART`, las palabras (`DATA`, `DATB`) de cada línea y si va unido.
struct Spr<'a> {
    x: u16,
    y: u16,
    datos: &'a [(u16, u16)],
    attach: bool,
}

fn spr(x: u16, y: u16, datos: &[(u16, u16)]) -> Spr<'_> {
    Spr { x, y, datos, attach: false }
}

/// Las palabras de control de un sprite de `alto` líneas.
fn control(x: u16, y: u16, alto: u16, attach: bool) -> (u16, u16) {
    let fin = y + alto;
    let pos = (y & 0xFF) << 8 | (x >> 1 & 0xFF);
    let ctl = (fin & 0xFF) << 8 | if attach { 0x80 } else { 0 } | (y >> 8 & 1) << 2 | (fin >> 8 & 1) << 1 | (x & 1);
    (pos, ctl)
}

/// Pone la lista del canal `n` con sus sprites (y las dos palabras a 0 al final).
fn lista_sprites(a: &mut Amiga, n: u32, sprites: &[Spr]) {
    let mut w = Vec::new();
    for s in sprites {
        let (pos, ctl) = control(s.x, s.y, s.datos.len() as u16, s.attach);
        w.extend_from_slice(&[pos, ctl]);
        for &(d, e) in s.datos {
            w.extend_from_slice(&[d, e]);
        }
    }
    w.extend_from_slice(&[0, 0]);
    a.m.poner(SPR + 0x400 * n, &w);
}

/// La paleta de prueba: 32 colores distintos.
fn paleta() -> Vec<u16> {
    (0..32u16).map(|n| (n * 0x137 + 0x0A5) & 0xFFF).collect()
}

/// La lista de siempre: los ocho punteros de sprites, la pantalla de 320 x 256 en baja desde `$2C81` con `planos`
/// planos (sin ninguno, no hay `BPL1DAT` ni ventana: hace falta uno, aunque esté vacío) (40 bytes por fila, 256 filas, desde [`MAPA`]), `bplcon0`, `bplcon2`, la paleta de prueba y `resto`.
fn lista(bplcon0: u16, bplcon2: u16, planos: u16, resto: &[[u16; 2]]) -> Vec<[u16; 2]> {
    let mut l = Vec::new();
    for n in 0..8u16 {
        let d = SPR + 0x400 * u32::from(n);
        l.push(mov(SPR0PTH + 4 * n, (d >> 16) as u16));
        l.push(mov(SPR0PTH + 4 * n + 2, d as u16));
    }
    l.extend_from_slice(&[
        mov(DIWSTRT, 0x2C81),
        mov(DIWSTOP, 0x2CC1),
        mov(DDFSTRT, 0x38),
        mov(DDFSTOP, 0xD0),
        mov(BPL1MOD, 0),
        mov(BPL2MOD, 0),
        mov(BPLCON0, bplcon0),
        mov(BPLCON2, bplcon2),
    ]);
    for p in 0..planos {
        let d = MAPA + u32::from(p) * 40 * 256;
        l.push(mov(BPL1PTH + 4 * p, (d >> 16) as u16));
        l.push(mov(BPL1PTH + 4 * p + 2, d as u16));
    }
    for (i, c) in paleta().into_iter().enumerate() {
        l.push(mov(COLOR00 + 2 * i as u16, c));
    }
    l.extend_from_slice(resto);
    l.push(FIN);
    l
}

/// Llena el plano `p` (40 bytes por fila, 256 filas) con `f(fila, byte)`.
fn llenar(a: &mut Amiga, p: u32, f: impl Fn(usize, usize) -> u8) {
    let palabras: Vec<u16> =
        (0..256 * 20).map(|i| u16::from_be_bytes([f(i / 20, 2 * (i % 20)), f(i / 20, 2 * (i % 20) + 1)])).collect();
    a.m.poner(MAPA + p * 40 * 256, &palabras);
}

fn color(n: usize) -> u32 {
    rgb(paleta()[n])
}

// --- Un sprite ---

#[test]
fn un_sprite() {
    let mut a = Amiga::nuevo();
    // Cuatro líneas: el primer píxel color 1, el último color 3, en medio color 2 (DATB) en la segunda mitad.
    let d = [(0x8001, 0x00FF); 4];
    lista_sprites(&mut a, 0, &[spr(0xA0, 0x50, &d)]);
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    // HSTART $A0: el primer píxel en $A1.
    assert_eq!(a.px(0xA0, 0x50), color(0), "antes, el fondo");
    assert_eq!(a.px(0xA1, 0x50), color(17));
    assert_eq!(a.px2(0xA1, 0x50), color(17), "un píxel de baja son dos columnas");
    assert_eq!(a.px(0xA2, 0x50), color(0), "color 0: transparente");
    assert_eq!(a.px(0xA1 + 8, 0x50), color(18));
    assert_eq!(a.px(0xA1 + 15, 0x50), color(19));
    assert_eq!(a.px(0xA1 + 16, 0x50), color(0));
    for v in 0x50..0x54 {
        assert_eq!(a.px(0xA1, v), color(17), "línea {v:#x}");
    }
    assert_eq!(a.px(0xA1, 0x4F), color(0), "VSTART");
    assert_eq!(a.px(0xA1, 0x54), color(0), "VSTOP: no sale");
    // Agnus ha leído las palabras de control, las cuatro líneas y las dos palabras a 0.
    assert_eq!(a.c.sprites().puntero(0), SPR + 4 + 16 + 4);
    assert!(!a.c.sprites().armado(0), "las palabras a 0 lo desarman");
    assert_eq!(a.c.accesos_a_nadie, 0);
}

#[test]
fn sh0_mueve_un_pixel_de_baja() {
    let mut a = Amiga::nuevo();
    lista_sprites(&mut a, 0, &[spr(0xA1, 0x50, &[(0x8000, 0)])]);
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    assert_eq!(a.px(0xA1, 0x50), color(0));
    assert_eq!(a.px(0xA2, 0x50), color(17));
    assert_eq!(a.px(0xA3, 0x50), color(0));
}

#[test]
fn colores_de_cada_pareja_y_varios_en_una_linea() {
    let mut a = Amiga::nuevo();
    // Los ocho en la misma línea, cada uno con sus tres colores (1, 2 y 3 en sus tres primeros píxeles).
    let d = [(0xA000u16, 0x6000u16)];
    for n in 0..8u16 {
        lista_sprites(&mut a, u32::from(n), &[spr(0x90 + 0x20 * n, 0x60, &d)]);
    }
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    for n in 0..8usize {
        let x = 0x91 + 0x20 * n;
        let base = 16 + 4 * (n / 2);
        assert_eq!(a.px(x, 0x60), color(base + 1), "sprite {n}");
        assert_eq!(a.px(x + 1, 0x60), color(base + 2), "sprite {n}");
        assert_eq!(a.px(x + 2, 0x60), color(base + 3), "sprite {n}");
        assert_eq!(a.px(x + 3, 0x60), color(0), "sprite {n}");
    }
}

#[test]
fn reutilizar_el_canal() {
    let mut a = Amiga::nuevo();
    // Tres sprites en el canal 3: el segundo empieza en la línea siguiente al VSTOP del primero (que es la de leer
    // sus palabras de control); el tercero, más abajo y en otro sitio.
    lista_sprites(
        &mut a,
        3,
        &[
            spr(0xA0, 0x50, &[(0xFFFF, 0); 4]),
            spr(0xB0, 0x55, &[(0, 0xFFFF); 3]),
            spr(0xC0, 0x70, &[(0xFFFF, 0xFFFF); 2]),
        ],
    );
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    let (c1, c2, c3) = (color(21), color(22), color(23));
    assert_eq!(a.px(0xA1, 0x53), c1);
    assert_eq!(a.px(0xA1, 0x54), color(0), "la línea del VSTOP: lee el control");
    assert_eq!(a.px(0xB1, 0x54), color(0));
    for v in 0x55..0x58 {
        assert_eq!(a.px(0xB1, v), c2, "línea {v:#x}");
        assert_eq!(a.px(0xA1, v), color(0));
    }
    assert_eq!(a.px(0xB1, 0x58), color(0));
    assert_eq!(a.px(0xC1, 0x70), c3);
    assert_eq!(a.px(0xC1, 0x71), c3);
    assert_eq!(a.px(0xC1, 0x72), color(0));
}

#[test]
fn sprites_unidos_dan_15_colores() {
    let mut a = Amiga::nuevo();
    // El 2 con $AAAA/$CCCC y el 3 (unido) con $F0F0/$FF00: el píxel k tiene el color 15 - k (de 15 a 0).
    lista_sprites(&mut a, 2, &[spr(0xA0, 0x50, &[(0xAAAA, 0xCCCC)])]);
    lista_sprites(&mut a, 3, &[Spr { x: 0xA0, y: 0x50, datos: &[(0xF0F0, 0xFF00)], attach: true }]);
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    for k in 0..15 {
        assert_eq!(a.px(0xA1 + k, 0x50), color(16 + 15 - k), "píxel {k}");
    }
    assert_eq!(a.px(0xA1 + 15, 0x50), color(0), "el color 0 de los unidos también es transparente");
}

#[test]
fn entre_sprites_el_menor_delante() {
    let mut a = Amiga::nuevo();
    // El 0 en $A0, el 1 en $A4 y el 2 en $A8, cada uno de 16 píxeles de color 1, en la misma línea.
    for (n, x) in [(0, 0xA0), (1, 0xA4), (2, 0xA8)] {
        lista_sprites(&mut a, n, &[spr(x, 0x50, &[(0xFFFF, 0)])]);
    }
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    assert_eq!(a.px(0xA5, 0x50), color(17), "el 0 delante del 1");
    assert_eq!(a.px(0xB1, 0x50), color(17), "el 0 delante del 2");
    assert_eq!(a.px(0xB2, 0x50), color(17), "el 1 (mismos colores) delante del 2");
    assert_eq!(a.px(0xB5, 0x50), color(21), "el 2, solo");
}

// --- Prioridades con los campos ---

#[test]
fn prioridad_con_un_campo() {
    // El plano 1 lleno a la izquierda (color 1) y vacío a la derecha. Los sprites 0, 2, 4 y 6, mitad sobre cada lado.
    let ver = |bplcon2: u16| {
        let mut a = Amiga::nuevo();
        llenar(&mut a, 0, |_, b| if b < 20 { 0xFF } else { 0 });
        for n in 0..4u32 {
            lista_sprites(&mut a, 2 * n, &[spr(0x118, 0x50 + 4 * n as u16, &[(0xFFFF, 0)])]);
        }
        a.mostrar(&lista(0x1200, bplcon2, 1, &[]));
        (0..4u64)
            .map(|n| {
                let v = 0x50 + 4 * n;
                let mio = color(17 + 4 * n as usize);
                assert_eq!(a.px(0x121, v), mio, "sobre el color 0 siempre se ve");
                a.px(0x120, v) == mio
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(ver(0x00), [false; 4], "PF2P 0: el campo delante de todos");
    assert_eq!(ver(0x08), [true, false, false, false], "PF2P 1");
    assert_eq!(ver(0x18), [true, true, true, false], "PF2P 3");
    assert_eq!(ver(0x20), [true; 4], "PF2P 4: todos delante");
    assert_eq!(ver(0x07), [false; 4], "con un campo, PF1P no cuenta");
}

#[test]
fn prioridad_con_campos_dobles() {
    // Campo 1 (plano 1) en los bytes 0-9 y 20-29; campo 2 (plano 2) en los 10-29. El sprite 2 (pareja 1) cruza
    // todo en la línea $50.
    let ver = |bplcon2: u16| {
        [0x88u16, 0xD8, 0x128].map(|x| {
            let mut a = Amiga::nuevo();
            llenar(&mut a, 0, |_, b| if b < 10 || (20..30).contains(&b) { 0xFF } else { 0 });
            llenar(&mut a, 1, |_, b| if (10..30).contains(&b) { 0xFF } else { 0 });
            lista_sprites(&mut a, 2, &[spr(x, 0x50, &[(0xFFFF, 0)])]);
            a.mostrar(&lista(0x2600, bplcon2, 2, &[]));
            a.px(usize::from(x) + 1, 0x50) == color(21)
        })
    };
    // Los tramos: solo el campo 1; solo el 2; los dos.
    assert_eq!(ver(0x0A), [true, false, true], "PF1P 2 (la pareja 1 delante), PF2P 1: donde están los dos se ve el 1");
    assert_eq!(ver(0x11), [false, true, false], "PF1P 1, PF2P 2");
    assert_eq!(ver(0x51), [false, true, true], "con PF2PRI se ve el 2, y cuenta el suyo");
    assert_eq!(ver(0x24), [true, true, true]);
}

#[test]
fn sprites_sobre_ham_no_cambian_el_color_retenido() {
    let mut a = Amiga::nuevo();
    // HAM6: cada 16 píxeles, COLOR01 y 15 píxeles que solo cambian el azul a $A.
    let valores = |k: usize| if k.is_multiple_of(16) { 0b00_0001u8 } else { 0b01_1010 };
    for p in 0..6u32 {
        llenar(&mut a, p, move |_, b| (0..8).fold(0u8, |v, k| v | ((valores(b * 8 + k) >> p) & 1) << (7 - k)));
    }
    lista_sprites(&mut a, 0, &[spr(0xA0, 0x50, &[(0xFF00, 0)])]);
    a.mostrar(&lista(0x6A00, 0x24, 6, &[]));
    let ham = rgb(paleta()[1] & 0xFF0 | 0xA);
    assert_eq!(a.px(0xA1, 0x50), color(17));
    assert_eq!(a.px(0xA1 + 8, 0x50), ham, "tras el sprite, el color de HAM sigue como si no estuviera");
}

#[test]
fn ehb_no_oscurece_los_sprites() {
    let mut a = Amiga::nuevo();
    llenar(&mut a, 5, |_, _| 0xFF);
    lista_sprites(&mut a, 0, &[spr(0xA0, 0x50, &[(0xFFFF, 0)])]);
    a.mostrar(&lista(0x6200, 0x24, 6, &[]));
    assert_eq!(a.px(0xA1, 0x50), color(17));
    assert_eq!(a.px(0xB1, 0x50), rgb(paleta()[0] >> 1 & 0x777));
}

// --- El borde ---

#[test]
fn solo_dentro_de_la_ventana() {
    let mut a = Amiga::nuevo();
    let d = [(0xFFFF, 0); 8];
    // El 0 empieza a la izquierda de la ventana; el 1, a la derecha; el 2, encima; el 3, en el borde de abajo.
    lista_sprites(&mut a, 0, &[spr(0x78, 0x50, &d)]);
    lista_sprites(&mut a, 1, &[spr(0x1B8, 0x50, &d)]);
    lista_sprites(&mut a, 2, &[spr(0xA0, 0x28, &d)]);
    lista_sprites(&mut a, 3, &[spr(0xC0, 0x128, &d)]);
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    assert_eq!(a.px(0x80, 0x50), color(0), "a la izquierda, borde");
    assert_eq!(a.px(0x81, 0x50), color(17));
    assert_eq!(a.px(0x88, 0x50), color(17));
    assert_eq!(a.px(0x89, 0x50), color(0));
    assert_eq!(a.px(0x1C0, 0x50), color(17));
    assert_eq!(a.px(0x1C1, 0x50), color(0), "a la derecha, borde");
    assert_eq!(a.px(0xA1, 0x2B), color(0), "encima de la ventana, borde");
    assert_eq!(a.px(0xA1, 0x2C), color(21));
    assert_eq!(a.px(0xC1, 0x12B), color(21));
    assert_eq!(a.px(0xC1, 0x12C), color(0), "debajo, borde");
}

#[test]
fn sin_planos_no_se_ven() {
    let mut a = Amiga::nuevo();
    lista_sprites(&mut a, 0, &[spr(0xA0, 0x50, &[(0xFFFF, 0)])]);
    let l = lista(0x1200, 0x24, 1, &[]);
    let palabras: Vec<u16> = l.iter().flatten().copied().collect();
    a.m.poner(LISTA, &palabras);
    a.con(|c, p, h| c.escribir_con(COP1LC, LISTA, 4, h, p));
    a.w(0x096, SET | dma::DMAEN | dma::COPEN | dma::SPREN);
    a.a(2 * CUADRO);
    assert_eq!(a.px(0xA1, 0x50), color(0), "sin BPL1DAT en la línea no hay ventana");
    assert_eq!(a.c.sprites().puntero(0), SPR + 4 + 4 + 4, "pero Agnus lee igual");
}

// --- El DMA ---

#[test]
fn sin_spren_no_se_lee_nada() {
    let mut a = Amiga::nuevo();
    lista_sprites(&mut a, 0, &[spr(0xA0, 0x50, &[(0xFFFF, 0)])]);
    a.mostrar_con(&lista(0x1200, 0x24, 1, &[]), false);
    assert_eq!(a.px(0xA1, 0x50), color(0));
    assert_eq!(a.c.sprites().puntero(0), SPR);
}

#[test]
fn ddfstrt_temprano_se_come_los_ultimos_sprites() {
    let ver = |ddfstrt: u16| {
        let mut a = Amiga::nuevo();
        for n in 0..8u32 {
            lista_sprites(&mut a, n, &[spr(0x100, 0x50 + 2 * n as u16, &[(0xFFFF, 0)])]);
        }
        let mut l = lista(0x1200, 0x24, 1, &[]);
        let i = l.iter().position(|m| m[0] == DDFSTRT).unwrap();
        l[i] = mov(DDFSTRT, ddfstrt);
        a.mostrar(&l);
        (0..8).map(|n| a.px(0x101, 0x50 + 2 * n as u64) == color(17 + 4 * (n / 2))).collect::<Vec<_>>()
    };
    assert_eq!(ver(0x38), [true; 8]);
    assert_eq!(ver(0x30), [true, true, true, true, true, true, true, false], "el 7 se pierde");
    assert_eq!(ver(0x28), [true, true, true, true, true, false, false, false], "el 5, el 6 y el 7");
}

#[test]
fn ddfstrt_temprano_en_lineas_sin_planos_no_quita_nada() {
    let mut a = Amiga::nuevo();
    // El 7 encima de la ventana vertical: no hay lectura de planos, así que lee sus datos (aunque no se vea).
    lista_sprites(&mut a, 7, &[spr(0x100, 0x20, &[(0xFFFF, 0); 4])]);
    let mut l = lista(0x1200, 0x24, 1, &[]);
    let i = l.iter().position(|m| m[0] == DDFSTRT).unwrap();
    l[i] = mov(DDFSTRT, 0x28);
    a.mostrar(&l);
    assert_eq!(a.c.sprites().puntero(7), SPR + 0x1C00 + 4 + 16 + 4);
}

// --- A mano ---

#[test]
fn modo_manual_por_el_68k() {
    let mut a = Amiga::nuevo();
    a.mostrar(&lista(0x1200, 0x24, 1, &[]));
    let inicio = a.hora - a.hora % CUADRO;
    // En la línea $60 (ciclo $10): POS (HSTART $C0, VSTART 0), CTL, DATB y DATA, que lo arma.
    a.a(inicio + 0x60 * LINEA + 0x10 * CICLOS_POR_COLOR);
    let base = SPR0POS + 8 * 4;
    a.w(base, 0x0060).w(base + 2, 0).w(base + 6, 0x00FF).w(base + 4, 0xFF00);
    // Denise lo ve al traducir la línea: en la siguiente, ya está armado.
    a.a(inicio + 0x61 * LINEA + 0x10 * CICLOS_POR_COLOR);
    assert!(a.c.sprites().armado(4));
    // En la $70, CTL lo desarma.
    a.a(inicio + 0x70 * LINEA + 0x10 * CICLOS_POR_COLOR).w(base + 2, 0);
    a.a(inicio + CUADRO);
    assert_eq!(a.px(0xC1, 0x5F), color(0));
    for v in 0x60..0x70 {
        assert_eq!(a.px(0xC1, v), color(25), "línea {v:#x}");
        assert_eq!(a.px(0xC1 + 8, v), color(26));
    }
    assert_eq!(a.px(0xC1, 0x70), color(0), "desarmado");
    assert!(!a.c.sprites().armado(4));
}

#[test]
fn el_copper_mueve_un_sprite_a_mitad_de_linea() {
    let mut a = Amiga::nuevo();
    lista_sprites(&mut a, 1, &[spr(0xA0, 0x50, &[(0xFFFF, 0); 8])]);
    // En las líneas $52 y $53: en el ciclo $70 HSTART pasa a $100 (sale otra vez); en el $20 de la siguiente, vuelve.
    let mut resto = Vec::new();
    for v in [0x52u16, 0x53] {
        resto.push(wait(v, 0x70));
        resto.push(mov(SPR0POS + 8, 0x5080));
        resto.push(wait(v + 1, 0x20));
        resto.push(mov(SPR0POS + 8, 0x5050));
    }
    a.mostrar(&lista(0x1200, 0x24, 1, &resto));
    assert_eq!(a.px(0xA1, 0x51), color(17));
    assert_eq!(a.px(0x101, 0x51), color(0));
    for v in [0x52, 0x53] {
        assert_eq!(a.px(0xA1, v), color(17), "línea {v:#x}");
        assert_eq!(a.px(0x101, v), color(17), "línea {v:#x}: otra vez");
    }
    assert_eq!(a.px(0x101, 0x54), color(0));
}

// --- Los choques ---

/// Lee `CLXDAT` al principio del cuadro siguiente (los choques de un cuadro entero, tras borrarlo al empezar este).
fn clxdat_de_un_cuadro(a: &mut Amiga) -> u16 {
    let h = a.hora - a.hora % CUADRO;
    a.a(h + LINEA);
    a.leer(CLXDAT);
    a.a(h + CUADRO + LINEA);
    a.leer(CLXDAT)
}

#[test]
fn choques_entre_sprites() {
    let ver = |clxcon: u16| {
        let mut a = Amiga::nuevo();
        let d = [(0xFFFF, 0); 2];
        // 0 con 2; 3 con 5; 4 con 7; 1 con 6 encima de la ventana.
        for (n, x, y) in [(0, 0xA0, 0x60), (2, 0xA8, 0x60), (3, 0xC0, 0x60), (5, 0xC4, 0x60), (4, 0xE0, 0x60)] {
            lista_sprites(&mut a, n, &[spr(x, y, &d)]);
        }
        lista_sprites(&mut a, 7, &[spr(0xE8, 0x60, &d)]);
        lista_sprites(&mut a, 1, &[spr(0x100, 0x20, &d)]);
        lista_sprites(&mut a, 6, &[spr(0x100, 0x20, &d)]);
        // Sin planos que choquen: CLXCON con ENBP1 y MVBP1 a 0 sobre el plano 1 lleno.
        llenar(&mut a, 0, |_, _| 0xFF);
        a.mostrar(&lista(0x1200, 0x24, 1, &[mov(CLXCON, clxcon | 0x0040)]));
        clxdat_de_un_cuadro(&mut a) & 0x7E00
    };
    assert_eq!(ver(0), 1 << 9, "solo 0-1 con 2-3: los impares no cuentan");
    assert_eq!(ver(0xF000), 1 << 9 | 1 << 12 | 1 << 14);
    assert_eq!(ver(0x6000), 1 << 9 | 1 << 12, "ENSP3 y ENSP5");
    assert_eq!(ver(0x2000), 1 << 9, "solo ENSP3: el 5 no cuenta");
    assert_eq!(ver(0x8000), 1 << 9 | 1 << 14);
}

#[test]
fn choques_con_los_campos() {
    // Bandas de 32 píxeles con 2 planos: índice (x >> 5) & 3. Los sprites 0, 2, 4 y 6 sobre las bandas 1, 3, 2 y 0.
    let ver = |bplcon0: u16, clxcon: u16| {
        let mut a = Amiga::nuevo();
        llenar(&mut a, 0, |_, b| if b / 4 % 2 == 1 { 0xFF } else { 0 });
        llenar(&mut a, 1, |_, b| if b / 4 % 4 >= 2 { 0xFF } else { 0 });
        for (n, x) in [(0, 0xA8), (2, 0xE8), (4, 0xC8), (6, 0x88)] {
            lista_sprites(&mut a, n, &[spr(x, 0x60, &[(0xFFFF, 0xFFFF); 4])]);
        }
        a.mostrar(&lista(bplcon0, 0x24, 2, &[mov(CLXCON, clxcon)]));
        let v = clxdat_de_un_cuadro(&mut a);
        // Leerlo lo borra.
        assert_eq!(a.leer(CLXDAT), 0x8000);
        v
    };
    // Campo 1 si el plano 1 es 1; campo 2 si el plano 2 es 1. El 2 choca con los dos (bits 2 y 6); el 4 con el
    // campo 2 (bit 7); el 0 solo con el campo 1, pero con un campo eso no cuenta si no choca con el 2.
    assert_eq!(ver(0x2200, 0x00C3), 0x8000 | 1 | 1 << 2 | 1 << 6 | 1 << 7);
    assert_eq!(ver(0x2600, 0x00C3), 0x8000 | 1 | 1 << 1 | 1 << 2 | 1 << 6 | 1 << 7, "con campos dobles, sí");
    assert_eq!(ver(0x2200, 0), 0x81FF, "sin ENBP, todo choca");
    // Campo 1 si el plano 1 es 0 (bandas 0 y 2): el 4 y el 6 con el 1; todos con el 2.
    assert_eq!(ver(0x2200, 0x0040), 0x8000 | 1 | 1 << 3 | 1 << 4 | 0xF << 5);
}

#[test]
fn clxdat_al_encender() {
    let mut a = Amiga::nuevo();
    assert_eq!(a.leer(CLXDAT), 0x8000);
    a.w(CLXCON, 0x1234);
    assert_eq!(a.c.sprites().clxcon(), 0x1234);
    assert_eq!(a.c.accesos_a_nadie, 0);
}
