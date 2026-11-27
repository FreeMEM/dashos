//! Pruebas de los planos y de Denise por el bus, con una chip RAM de prueba y la imagen en un búfer
//! ([`ConPantalla`]).

use caja_chips::denise::{ALTO, ANCHO, ConPantalla, V0, X0, rgb};
use caja_chips::{CICLOS_POR_COLOR, CICLOS_POR_LINEA, Chips, MemoriaChip, dma};

const LINEA: u64 = CICLOS_POR_LINEA;
const CUADRO: u64 = 313 * LINEA;

const VPOSR: u32 = 0xDFF004;
const COP1LC: u32 = 0xDFF080;
const DMACON: u32 = 0xDFF096;
const SET: u16 = 0x8000;

// Los registros, por su desplazamiento.
const DIWSTRT: u16 = 0x08E;
const DIWSTOP: u16 = 0x090;
const DDFSTRT: u16 = 0x092;
const DDFSTOP: u16 = 0x094;
const BPL1PTH: u16 = 0x0E0;
const BPLCON0: u16 = 0x100;
const BPLCON1: u16 = 0x102;
const BPLCON2: u16 = 0x104;
const BPL1MOD: u16 = 0x108;
const BPL2MOD: u16 = 0x10A;
const BPL1DAT: u16 = 0x110;
const COLOR00: u16 = 0x180;

/// 512 KB de chip RAM, de palabra en palabra.
struct Memoria(Vec<u16>);

impl Memoria {
    fn nueva() -> Self {
        Memoria(vec![0; 256 * 1024])
    }

    fn poner(&mut self, dir: u32, palabras: &[u16]) {
        let i = dir as usize / 2;
        self.0[i..i + palabras.len()].copy_from_slice(palabras);
    }

    /// Pone los bytes de `bytes` desde `dir` (par).
    fn poner_bytes(&mut self, dir: u32, bytes: &[u8]) {
        let palabras: Vec<u16> = bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        self.poner(dir, &palabras);
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

// --- Las listas del Copper ---

fn mov(reg: u16, v: u16) -> [u16; 2] {
    [reg, v]
}

fn wait(v: u16, h: u16) -> [u16; 2] {
    [((v & 0xFF) << 8) | (h & 0xFE) | 1, 0xFFFE]
}

const FIN: [u16; 2] = [0xFFFF, 0xFFFE];

/// Dónde van la lista y los planos en la chip RAM de la prueba.
const LISTA: u32 = 0x1_0000;
const MAPA: u32 = 0x2_0000;

/// Los chips, su chip RAM y la imagen.
struct Amiga {
    c: Chips,
    m: Memoria,
    cuadro: Vec<u32>,
    cuadros: u64,
    largo: bool,
    hora: u64,
}

impl Amiga {
    fn nuevo() -> Self {
        Amiga {
            c: Chips::nuevo(),
            m: Memoria::nueva(),
            cuadro: vec![0; ANCHO * ALTO],
            cuadros: 0,
            largo: true,
            hora: 0,
        }
    }

    fn con<R>(&mut self, f: impl FnOnce(&mut Chips, &mut ConPantalla, u64) -> R) -> R {
        let mut p = ConPantalla::nueva(&mut self.m, &mut self.cuadro);
        p.cuadros = self.cuadros;
        let r = f(&mut self.c, &mut p, self.hora);
        self.cuadros = p.cuadros;
        self.largo = p.largo;
        r
    }

    fn w(&mut self, reg: u32, v: u16) {
        self.con(|c, p, h| c.escribir_con(reg, u32::from(v), 2, h, p));
    }

    fn l(&mut self, reg: u32, v: u32) {
        self.con(|c, p, h| c.escribir_con(reg, v, 4, h, p));
    }

    fn leer(&mut self, reg: u32) -> u16 {
        self.con(|c, p, h| c.leer_con(reg, 2, h, p)) as u16
    }

    /// Lleva la hora a `hora`.
    fn a(&mut self, hora: u64) -> &mut Self {
        self.hora = hora;
        self.con(|c, p, h| c.avanzar_con(h, p));
        self
    }

    /// Pone la lista y enciende el DMA del Copper y de los planos: la lista empieza en el cuadro siguiente. Deja la
    /// hora al final del segundo cuadro: la imagen es la del cuadro completo.
    fn mostrar(&mut self, lista: &[[u16; 2]]) {
        let palabras: Vec<u16> = lista.iter().flatten().copied().collect();
        self.m.poner(LISTA, &palabras);
        self.l(COP1LC, LISTA);
        self.w(DMACON, SET | dma::DMAEN | dma::COPEN | dma::BPLEN);
        let h = self.hora;
        self.a(h - h % CUADRO + 2 * CUADRO);
    }

    /// El píxel en la columna de alta `col` y la línea `v` del cuadro acabado.
    fn px(&self, col: usize, v: u64) -> u32 {
        self.cuadro[(v - V0) as usize * ANCHO + col]
    }
}

/// La columna (en píxeles de alta) de la posición horizontal `x` (en píxeles de baja, como `DIWSTRT`).
fn col(x: usize) -> usize {
    2 * (x - X0)
}

/// La lista de siempre: pantalla de 320 x 256 en baja desde `$2C81`, con `planos` planos seguidos de 40 bytes por
/// fila y 256 filas desde [`MAPA`], `bplcon0` y la paleta `colores`; y `resto` después.
fn lista_baja(bplcon0: u16, planos: usize, colores: &[u16], resto: &[[u16; 2]]) -> Vec<[u16; 2]> {
    let mut l = vec![
        mov(DIWSTRT, 0x2C81),
        mov(DIWSTOP, 0x2CC1),
        mov(DDFSTRT, 0x38),
        mov(DDFSTOP, 0xD0),
        mov(BPL1MOD, 0),
        mov(BPL2MOD, 0),
        mov(BPLCON0, bplcon0),
        mov(BPLCON1, 0),
        mov(BPLCON2, 0),
    ];
    for p in 0..planos as u16 {
        let d = MAPA + u32::from(p) * 40 * 256;
        l.push(mov(BPL1PTH + 4 * p, (d >> 16) as u16));
        l.push(mov(BPL1PTH + 4 * p + 2, d as u16));
    }
    for (i, &c) in colores.iter().enumerate() {
        l.push(mov(COLOR00 + 2 * i as u16, c));
    }
    l.extend_from_slice(resto);
    l.push(FIN);
    l
}

/// Llena el plano `p` (40 bytes por fila, 256 filas, desde [`MAPA`]) con `f(fila, byte)`.
fn llenar(a: &mut Amiga, p: u32, f: impl Fn(usize, usize) -> u8) {
    let bytes: Vec<u8> = (0..256 * 40).map(|i| f(i / 40, i % 40)).collect();
    a.m.poner_bytes(MAPA + p * 40 * 256, &bytes);
}

/// Una paleta de prueba: 32 colores distintos.
fn paleta() -> Vec<u16> {
    (0..32u16).map(|n| (n * 0x137 + 0x0A5) & 0xFFF).collect()
}

// --- Un plano, la ventana y el borde ---

#[test]
fn un_plano_en_baja() {
    let mut a = Amiga::nuevo();
    // Plano 1: la primera fila $F0 0F..., las demás $80 00 (un punto cada 16 píxeles).
    llenar(&mut a, 0, |f, b| if f == 0 { [0xF0, 0x0F][b % 2] } else { [0x80, 0x00][b % 2] });
    a.mostrar(&lista_baja(0x1200, 1, &[0x00F, 0xF00], &[]));
    let (azul, rojo) = (rgb(0x00F), rgb(0xF00));
    // El primer píxel de la ventana cae en $81, en la línea $2C; cada píxel de baja ocupa dos columnas.
    for x in 0..4 {
        assert_eq!(a.px(col(0x81 + x), 0x2C), rojo, "x {x}");
        assert_eq!(a.px(col(0x81 + x) + 1, 0x2C), rojo);
        assert_eq!(a.px(col(0x81 + 4 + x), 0x2C), azul);
        assert_eq!(a.px(col(0x81 + 12 + x), 0x2C), rojo);
    }
    assert_eq!(a.px(col(0x80), 0x2C), azul, "antes de la ventana, el borde");
    assert_eq!(a.px(col(0x81), 0x2B), azul, "la línea de antes, borde");
    assert_eq!(a.px(col(0x81), 0x2D), rojo);
    assert_eq!(a.px(col(0x82), 0x2D), azul);
    assert_eq!(a.px(col(0x81 + 16), 0x2D), rojo);
    // La última columna de la ventana es $1C0, y la última línea, $12B.
    assert_eq!(a.px(col(0x1C0 - 15), 0x12B), rojo);
    assert_eq!(a.px(col(0x1C1), 0x12B), azul);
    assert_eq!(a.px(col(0x81), 0x12C), azul);
    assert_eq!(a.cuadros, 2);
}

#[test]
fn cinco_planos_dan_el_indice() {
    let mut a = Amiga::nuevo();
    // El píxel x de la primera fila tiene el índice x % 32: el plano p tiene el bit p de x.
    for p in 0..5 {
        llenar(&mut a, p, |_, b| (0..8).fold(0u8, |v, k| v | ((((b * 8 + k) >> p) & 1) as u8) << (7 - k)));
    }
    let pal = paleta();
    a.mostrar(&lista_baja(0x5200, 5, &pal, &[]));
    for x in 0..320 {
        assert_eq!(a.px(col(0x81 + x), 0x50), rgb(pal[x % 32]), "x {x}");
    }
}

#[test]
fn alta_resolucion_cuatro_planos() {
    let mut a = Amiga::nuevo();
    let pal = paleta();
    let mut l = lista_baja(0xC200, 0, &pal, &[]);
    // DDF de alta: $3C a $D4, 80 bytes por fila.
    l[2] = mov(DDFSTRT, 0x3C);
    l[3] = mov(DDFSTOP, 0xD4);
    l.pop();
    for p in 0..4u16 {
        let d = MAPA + u32::from(p) * 80 * 256;
        l.push(mov(BPL1PTH + 4 * p, (d >> 16) as u16));
        l.push(mov(BPL1PTH + 4 * p + 2, d as u16));
    }
    l.push(FIN);
    for p in 0..4u32 {
        let bytes: Vec<u8> = (0..256 * 80)
            .map(|i| (0..8).fold(0u8, |v, k| v | (((((i % 80) * 8 + k) >> p) & 1) as u8) << (7 - k)))
            .collect();
        a.m.poner_bytes(MAPA + p * 80 * 256, &bytes);
    }
    a.mostrar(&l);
    // 640 píxeles, uno por columna, desde $81.
    for x in 0..640 {
        assert_eq!(a.px(col(0x81) + x, 0x60), rgb(pal[x % 16]), "x {x}");
    }
    assert_eq!(a.px(col(0x81) - 1, 0x60), rgb(pal[0]));
    assert_eq!(a.px(col(0x81) + 640, 0x60), rgb(pal[0]));
}

#[test]
fn ehb_a_media_luz() {
    let mut a = Amiga::nuevo();
    // Planos 1-5 a 0 y el 6 a 1: índice 32, la mitad de COLOR00; con el plano 1, el 33.
    llenar(&mut a, 0, |_, b| if b < 20 { 0x00 } else { 0xFF });
    llenar(&mut a, 5, |_, _| 0xFF);
    a.mostrar(&lista_baja(0x6200, 6, &[0xF8E, 0x5A3], &[]));
    assert_eq!(a.px(col(0x81), 0x40), rgb(0x747));
    assert_eq!(a.px(col(0x81 + 200), 0x40), rgb(0x251));
    assert_eq!(a.px(col(0x80), 0x40), rgb(0xF8E), "el borde, sin media luz");
}

#[test]
fn ham6() {
    let mut a = Amiga::nuevo();
    // Cada 8 píxeles: COLOR01 (00 0001), azul a $A (01 1010), rojo a $3 (10 0011), verde a $C (11 1100), y 4 iguales.
    let valores = [0b00_0001u8, 0b01_1010, 0b10_0011, 0b11_1100, 0b11_1100, 0b11_1100, 0b11_1100, 0b11_1100];
    for p in 0..6 {
        let byte = (0..8).fold(0u8, |v, k| v | ((valores[k] >> p) & 1) << (7 - k));
        llenar(&mut a, p, move |_, _| byte);
    }
    a.mostrar(&lista_baja(0x6A00, 6, &[0x000, 0x456], &[]));
    let v = 0x50;
    assert_eq!(a.px(col(0x81), v), rgb(0x456));
    assert_eq!(a.px(col(0x82), v), rgb(0x45A));
    assert_eq!(a.px(col(0x83), v), rgb(0x35A));
    assert_eq!(a.px(col(0x84), v), rgb(0x3CA));
    assert_eq!(a.px(col(0x88), v), rgb(0x3CA), "el siguiente grupo empieza con el verde de antes");
    assert_eq!(a.px(col(0x89), v), rgb(0x456));
}

#[test]
fn campos_dobles_y_prioridad() {
    let mut a = Amiga::nuevo();
    // Campo 1 (planos 1, 3, 5): índice 3 en la primera mitad de cada palabra. Campo 2 (2, 4, 6): índice 5 en los
    // bytes de 0 a 19.
    llenar(&mut a, 0, |_, b| if b % 2 == 0 { 0xFF } else { 0 });
    llenar(&mut a, 2, |_, b| if b % 2 == 0 { 0xFF } else { 0 });
    llenar(&mut a, 1, |_, b| if b < 20 { 0xFF } else { 0 });
    llenar(&mut a, 5, |_, b| if b < 20 { 0xFF } else { 0 });
    let pal = paleta();
    // En la línea $A0, PF2PRI: el campo 2 delante.
    a.mostrar(&lista_baja(0x6600, 6, &pal, &[wait(0xA0, 0x07), mov(BPLCON2, 0x0040)]));
    let v = 0x50;
    assert_eq!(a.px(col(0x81), v), rgb(pal[3]), "los dos: delante el 1");
    assert_eq!(a.px(col(0x81 + 8), v), rgb(pal[8 + 5]), "solo el 2: colores 8-15");
    assert_eq!(a.px(col(0x81 + 160), v), rgb(pal[3]), "solo el 1");
    assert_eq!(a.px(col(0x81 + 168), v), rgb(pal[0]), "ninguno: COLOR00");
    assert_eq!(a.px(col(0x81), 0xB0), rgb(pal[8 + 5]), "con PF2PRI, delante el 2");
    assert_eq!(a.px(col(0x81 + 160), 0xB0), rgb(pal[3]));
}

#[test]
fn modulos_impares_y_pares() {
    let mut a = Amiga::nuevo();
    // Un mapa de 60 bytes por fila; se leen 40, así que el módulo es 20. El plano 1 con BPL1MOD = 20 y el 2 con
    // BPL2MOD = -40 (repite siempre su primera fila).
    let fila = |f: usize| [f as u8; 60];
    let p1: Vec<u8> = (0..256).flat_map(fila).collect();
    a.m.poner_bytes(MAPA, &p1);
    let p2: Vec<u8> = (0..256 * 60).map(|i| if i < 40 { 0xFF } else { 0 }).collect();
    a.m.poner_bytes(MAPA + 0x8000, &p2);
    let l = vec![
        mov(DIWSTRT, 0x2C81),
        mov(DIWSTOP, 0x2CC1),
        mov(DDFSTRT, 0x38),
        mov(DDFSTOP, 0xD0),
        mov(BPL1MOD, 20),
        mov(BPL2MOD, (-40i16) as u16),
        mov(BPLCON0, 0x2200),
        mov(BPL1PTH, (MAPA >> 16) as u16),
        mov(BPL1PTH + 2, MAPA as u16),
        mov(BPL1PTH + 4, ((MAPA + 0x8000) >> 16) as u16),
        mov(BPL1PTH + 6, (MAPA + 0x8000) as u16),
        mov(COLOR00, 0x000),
        mov(COLOR00 + 2, 0x00F),
        mov(COLOR00 + 4, 0x0F0),
        mov(COLOR00 + 6, 0xF00),
        FIN,
    ];
    a.mostrar(&l);
    // La fila f del plano 1 es el byte f: su bit 7 (el primer píxel) es 1 si f >= 128; el bit 0 (el 8.º), si f es
    // impar.
    for f in [0u64, 1, 2, 127, 128, 129, 255] {
        let v = 0x2C + f;
        let primero = if f >= 128 { 0xF00 } else { 0x0F0 };
        let octavo = if f % 2 == 1 { 0xF00 } else { 0x0F0 };
        assert_eq!(a.px(col(0x81), v), rgb(primero), "fila {f}");
        assert_eq!(a.px(col(0x81 + 7), v), rgb(octavo), "fila {f}");
    }
    assert_eq!(a.planos().puntero(0), MAPA + 256 * 60, "BPL1PT acaba tras la última fila, con el módulo");
    assert_eq!(a.planos().puntero(1), MAPA + 0x8000);
}

impl Amiga {
    fn planos(&self) -> &caja_chips::denise::Planos {
        self.c.planos()
    }
}

#[test]
fn desplazamiento_fino_de_cada_campo() {
    let mut a = Amiga::nuevo();
    // Un punto al principio de cada palabra en los dos planos; el campo 1 (plano 1) va 5 píxeles a la derecha y el
    // 2 (plano 2), 3.
    llenar(&mut a, 0, |_, b| if b % 2 == 0 { 0x80 } else { 0 });
    llenar(&mut a, 1, |_, b| if b % 2 == 0 { 0x80 } else { 0 });
    let mut l = lista_baja(0x2200, 2, &[0x000, 0x00F, 0x0F0, 0xF00], &[]);
    l[7] = mov(BPLCON1, 0x0035);
    a.mostrar(&l);
    let v = 0x60;
    assert_eq!(a.px(col(0x81), v), rgb(0x000));
    assert_eq!(a.px(col(0x81 + 3), v), rgb(0x0F0), "el campo 2, 3 a la derecha");
    assert_eq!(a.px(col(0x81 + 5), v), rgb(0x00F), "el campo 1, 5");
    assert_eq!(a.px(col(0x81 + 16 + 3), v), rgb(0x0F0));
    assert_eq!(a.px(col(0x81 + 16 + 5), v), rgb(0x00F));
    assert_eq!(a.px(col(0x81 + 16 + 4), v), rgb(0x000));
}

#[test]
fn desplazamiento_en_alta_cuenta_de_dos_en_dos() {
    let mut a = Amiga::nuevo();
    let mut l = lista_baja(0x9200, 1, &[0x000, 0xFFF], &[]);
    l[2] = mov(DDFSTRT, 0x3C);
    l[3] = mov(DDFSTOP, 0xD4);
    l[7] = mov(BPLCON1, 0x000B);
    // 80 bytes por fila: punto al principio de cada palabra.
    let bytes: Vec<u8> = (0..80 * 256).map(|i| if i % 2 == 0 { 0x80 } else { 0 }).collect();
    a.m.poner_bytes(MAPA, &bytes);
    a.mostrar(&l);
    // En alta, el bit 3 no cuenta: 11 es 3, tres píxeles de baja, seis columnas.
    let v = 0x60;
    assert_eq!(a.px(col(0x81) + 6, v), rgb(0xFFF));
    assert_eq!(a.px(col(0x81) + 5, v), rgb(0x000));
    assert_eq!(a.px(col(0x81) + 7, v), rgb(0x000));
    assert_eq!(a.px(col(0x81) + 22, v), rgb(0xFFF));
}

#[test]
fn ventana_y_ddf_distintas() {
    let mut a = Amiga::nuevo();
    llenar(&mut a, 0, |_, _| 0xFF);
    // Ventana de $40 a $F0 en vertical y de $A1 a $181; DDF de $48 a $B8 (15 palabras: de $A1 a $191).
    let mut l = lista_baja(0x1200, 1, &[0x00F, 0xF00], &[]);
    l[0] = mov(DIWSTRT, 0x40A1);
    l[1] = mov(DIWSTOP, 0xF081);
    l[2] = mov(DDFSTRT, 0x48);
    l[3] = mov(DDFSTOP, 0xB8);
    a.mostrar(&l);
    let (azul, rojo) = (rgb(0x00F), rgb(0xF00));
    assert_eq!(a.px(col(0xA0), 0x40), azul);
    assert_eq!(a.px(col(0xA1), 0x40), rojo);
    assert_eq!(a.px(col(0x180), 0x40), rojo);
    assert_eq!(a.px(col(0x181), 0x40), azul, "la ventana acaba antes que los datos");
    assert_eq!(a.px(col(0xA1), 0x3F), azul);
    assert_eq!(a.px(col(0xA1), 0xEF), rojo);
    assert_eq!(a.px(col(0xA1), 0xF0), azul);
    // La ventana más ancha que los datos: tras la última palabra, COLOR00 (dentro de la ventana).
    let mut l = lista_baja(0x1200, 1, &[0x00F, 0xF00], &[]);
    l[2] = mov(DDFSTRT, 0x48);
    l[3] = mov(DDFSTOP, 0xB8);
    a.mostrar(&l);
    // Los datos empiezan en 2 * $48 + 17 = $A1 y son 15 palabras: hasta $190.
    assert_eq!(a.px(col(0xA0), 0x80), azul);
    assert_eq!(a.px(col(0xA1), 0x80), rojo);
    assert_eq!(a.px(col(0x190), 0x80), rojo);
    assert_eq!(a.px(col(0x191), 0x80), azul);
    assert_eq!(a.planos().puntero(0), MAPA + 256 * 30, "15 palabras por línea, sin módulo");
}

// --- El Copper ---

#[test]
fn barras_de_copper() {
    let mut a = Amiga::nuevo();
    let colores = [0xF00, 0x0F0, 0x00F, 0xFF0];
    let mut resto = Vec::new();
    for (i, &c) in colores.iter().enumerate() {
        resto.push(wait(0x30 + 4 * i as u16, 0x07));
        resto.push(mov(COLOR00, c));
    }
    // A mitad de línea: en la $50, desde el ciclo $80.
    resto.push(wait(0x50, 0x80));
    resto.push(mov(COLOR00, 0xFFF));
    resto.push(wait(0x51, 0x07));
    resto.push(mov(COLOR00, 0x000));
    a.mostrar(&lista_baja(0x0200, 0, &[0x000], &resto));
    for (i, &c) in colores.iter().enumerate() {
        let v = 0x30 + 4 * i as u64;
        // El WAIT a $07 despierta en el ciclo 5; el MOVE escribe en el 9: desde la posición $12 está el color.
        assert_eq!(a.px(col(0x70), v), rgb(c), "línea {v:#x}");
        assert_eq!(a.px(ANCHO - 1, v + 3), rgb(c));
        assert_eq!(a.px(col(0x70), v + 4), rgb(*colores.get(i + 1).unwrap_or(&0xFF0)), "línea {:#x}", v + 4);
    }
    // El WAIT a $80 despierta en el $7E: el MOVE lee en el $80 y escribe en el $82, en la posición $104.
    assert_eq!(a.px(col(0x103), 0x50), rgb(0xFF0));
    assert_eq!(a.px(col(0x104), 0x50), rgb(0xFFF));
    assert_eq!(a.px(ANCHO - 1, 0x50), rgb(0xFFF));
    assert_eq!(a.px(col(0x70), 0x51), rgb(0x000), "el MOVE de la $51 cae antes del principio de la imagen");
    assert_eq!(a.px(ANCHO - 1, 0x51), rgb(0x000));
}

#[test]
fn moves_seguidos_bandas_de_8_pixeles() {
    let mut a = Amiga::nuevo();
    let mut resto = vec![wait(0x40, 0x40)];
    for i in 0..8u16 {
        resto.push(mov(COLOR00, i * 0x222));
    }
    a.mostrar(&lista_baja(0x0200, 0, &[0x00F], &resto));
    // El primer MOVE escribe en el ciclo $42 (posición $84); cada uno, 4 ciclos (8 píxeles de baja) después.
    for i in 0..8 {
        let x = 0x84 + 8 * i;
        assert_eq!(a.px(col(x), 0x40), rgb(i as u16 * 0x222), "banda {i}");
        assert_eq!(a.px(col(x + 7), 0x40), rgb(i as u16 * 0x222));
    }
    assert_eq!(a.px(col(0x83), 0x40), rgb(0x00F));
}

#[test]
fn los_planos_retrasan_al_copper() {
    // En alta con 4 planos, Agnus usa todos los ciclos de $3C a $DB: un MOVE pedido en el $70 espera al final.
    let medir = |bplcon0: u16, ddf: (u16, u16)| {
        let mut a = Amiga::nuevo();
        let mut l = lista_baja(bplcon0, 0, &[0x000], &[wait(0x60, 0x70), mov(COLOR00, 0xFFF)]);
        l[2] = mov(DDFSTRT, ddf.0);
        l[3] = mov(DDFSTOP, ddf.1);
        a.mostrar(&l);
        (0..ANCHO).find(|&c| a.px(c, 0x60) == rgb(0xFFF)).map(|c| c / 2 + X0)
    };
    assert_eq!(medir(0x1200, (0x38, 0xD0)), Some(2 * 0x72), "baja, 1 plano: el Copper no espera");
    // Baja, 6 planos: el Copper solo tiene los ciclos 0 y 4 de cada unidad de 8 (desde $38). Despierta en el $70 (no
    // en el $6E, del plano 5), lee en el $74 (no en el $72, del 6) y escribe en el $78 (no en el $76, del 5).
    assert_eq!(medir(0x6200, (0x38, 0xD0)), Some(2 * 0x78), "baja, 6 planos");
    // Alta, 4 planos: la última unidad de lectura es la de $D4 a $DB. Despierta en el $DC, lee en el $DE y escribe en
    // el $E1 (el $E0 no lo puede usar).
    assert_eq!(medir(0xC200, (0x3C, 0xD4)), Some(2 * 0xE1), "alta, 4 planos");
}

#[test]
fn el_68k_cambia_un_color_a_mitad_de_linea() {
    let mut a = Amiga::nuevo();
    a.mostrar(&lista_baja(0x0200, 0, &[0x00F], &[]));
    // A mitad de la línea $80 del cuadro siguiente, ciclo $90.
    let inicio = a.hora - a.hora % CUADRO;
    a.a(inicio + 0x80 * LINEA + 0x90 * CICLOS_POR_COLOR).w(0xDFF180, 0xF0F);
    a.a(inicio + CUADRO);
    assert_eq!(a.px(col(2 * 0x90 - 1), 0x80), rgb(0x00F));
    assert_eq!(a.px(col(2 * 0x90), 0x80), rgb(0xF0F));
    assert_eq!(a.px(col(0x70), 0x81), rgb(0xF0F));
}

#[test]
fn bpl1dat_del_68k() {
    let mut a = Amiga::nuevo();
    // Sin DMA de planos, el 68k escribe BPL1DAT a mitad de línea: Denise lo dibuja en el siguiente ciclo de dibujo
    // (sin desplazamiento, los múltiplos de 8).
    let l = lista_baja(0x1200, 0, &[0x000, 0xFFF], &[]);
    let palabras: Vec<u16> = l.iter().flatten().copied().collect();
    a.m.poner(LISTA, &palabras);
    a.l(COP1LC, LISTA);
    a.w(DMACON, SET | dma::DMAEN | dma::COPEN);
    let inicio = a.hora - a.hora % CUADRO + CUADRO;
    a.a(inicio + 0x60 * LINEA + 0x51 * CICLOS_POR_COLOR).w(0xDFF000 + u32::from(BPL1DAT), 0xC003);
    a.a(inicio + CUADRO);
    // Escrito en el ciclo $51; se dibuja en el $58: desde la posición $B1.
    let x = 2 * 0x58 + 1;
    assert_eq!(a.px(col(x - 1), 0x60), rgb(0x000));
    assert_eq!(a.px(col(x), 0x60), rgb(0xFFF));
    assert_eq!(a.px(col(x + 1), 0x60), rgb(0xFFF));
    assert_eq!(a.px(col(x + 2), 0x60), rgb(0x000));
    assert_eq!(a.px(col(x + 15), 0x60), rgb(0xFFF));
    assert_eq!(a.px(col(x + 16), 0x60), rgb(0x000));
}

#[test]
fn entrelazado_alterna_los_campos() {
    let mut a = Amiga::nuevo();
    a.w(0xDFF000 + u32::from(BPLCON0), 0x0204);
    a.a(CUADRO + 1);
    let (n1, l1) = a.c.cuadros_pintados();
    a.a(CUADRO + 312 * LINEA + 1);
    let (n2, l2) = a.c.cuadros_pintados();
    assert_eq!((n1, n2), (1, 2));
    assert_ne!(l1, l2, "un campo largo y uno corto");
    assert_eq!(a.leer(VPOSR) >> 15 == 1, !l2);
}

#[test]
fn sin_imagen_no_pinta_pero_lee() {
    // Con SinMemoria (la caja sin pantalla) los punteros avanzan igual.
    let mut c = Chips::nuevo();
    for (r, v) in [(DIWSTRT, 0x2C81), (DIWSTOP, 0x2CC1), (DDFSTRT, 0x38), (DDFSTOP, 0xD0), (BPLCON0, 0x1200)] {
        c.escribir(0xDFF000 + u32::from(r), v, 2, 0);
    }
    c.escribir(DMACON, u32::from(SET | dma::DMAEN | dma::BPLEN), 2, 0);
    c.avanzar(CUADRO - 1);
    assert_eq!(c.planos().puntero(0), 256 * 40);
    assert_eq!(c.cuadros_pintados().0, 0);
    c.avanzar(CUADRO);
    assert_eq!(c.cuadros_pintados().0, 1);
}

#[test]
fn bplcon2_no_es_de_nadie() {
    let mut c = Chips::nuevo();
    c.escribir(0xDFF000 + u32::from(BPLCON2), 0x24, 2, 0);
    c.escribir(0xDFF000 + u32::from(BPLCON1), 0x11, 2, 0);
    assert_eq!(c.bplcon(), [0, 0x11, 0x24]);
    assert_eq!(c.accesos_a_nadie, 0);
}
