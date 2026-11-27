//! Pruebas del Blitter por el bus, como lo ve el 68k: casos a mano del manual y el oráculo (los blits de
//! `caja/pruebas68k/blitter_casos.h`, con las sumas que da `blitter.exe` en FS-UAE con el Kickstart 3.1).

use caja_chips::memoria::MemoriaChip;
use caja_chips::{CICLOS_POR_LINEA, Chips, dma, int};

const DMACONR: u32 = 0xDFF002;
const INTREQR: u32 = 0xDFF01E;
const BLTCON0: u32 = 0xDFF040;
const BLTCON1: u32 = 0xDFF042;
const BLTAFWM: u32 = 0xDFF044;
const BLTALWM: u32 = 0xDFF046;
const BLTCPT: u32 = 0xDFF048;
const BLTBPT: u32 = 0xDFF04C;
const BLTAPT: u32 = 0xDFF050;
const BLTAPTL: u32 = 0xDFF052;
const BLTDPT: u32 = 0xDFF054;
const BLTSIZE: u32 = 0xDFF058;
const BLTCMOD: u32 = 0xDFF060;
const BLTBMOD: u32 = 0xDFF062;
const BLTAMOD: u32 = 0xDFF064;
const BLTDMOD: u32 = 0xDFF066;
const BLTCDAT: u32 = 0xDFF070;
const BLTBDAT: u32 = 0xDFF072;
const BLTADAT: u32 = 0xDFF074;
const DMACON: u32 = 0xDFF096;
const INTREQ: u32 = 0xDFF09C;

/// 512 KB de chip RAM, en palabras.
struct Memoria(Vec<u16>);

impl Memoria {
    fn nueva() -> Self {
        Memoria(vec![0; 256 * 1024])
    }
    fn p(&self, dir: u32) -> u16 {
        self.0[dir as usize / 2]
    }
    fn poner(&mut self, dir: u32, v: &[u16]) {
        let i = dir as usize / 2;
        self.0[i..i + v.len()].copy_from_slice(v);
    }
    fn trozo(&self, dir: u32, n: usize) -> &[u16] {
        &self.0[dir as usize / 2..dir as usize / 2 + n]
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

/// Los chips con la memoria, y la hora, que avanza un poco en cada escritura.
struct Amiga {
    c: Chips,
    m: Memoria,
    hora: u64,
}

impl Amiga {
    /// Con el DMA del Blitter encendido.
    fn nuevo() -> Self {
        let mut a = Amiga { c: Chips::nuevo(), m: Memoria::nueva(), hora: 0 };
        a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::BLTEN);
        a
    }
    fn w(&mut self, dir: u32, v: u16) {
        self.hora += 4;
        self.c.escribir_con(dir, u32::from(v), 2, self.hora, &mut self.m);
    }
    fn l(&mut self, dir: u32, v: u32) {
        self.hora += 8;
        self.c.escribir_con(dir, v, 4, self.hora, &mut self.m);
    }
    fn r(&mut self, dir: u32) -> u16 {
        self.hora += 4;
        self.c.leer(dir, 2, self.hora) as u16
    }
    /// Espera a que baje BBUSY, como un programa: lee DMACONR hasta que acaba.
    fn espera(&mut self) {
        let mut n = 0;
        while self.r(DMACONR) & dma::BBUSY != 0 {
            n += 1;
            assert!(n < 1_000_000, "BBUSY no baja");
        }
    }
    fn bzero(&mut self) -> bool {
        self.r(DMACONR) & dma::BZERO != 0
    }
    /// Un blit de copia con todo lo que hace falta.
    #[allow(clippy::too_many_arguments)]
    fn copia(&mut self, con0: u16, con1: u16, masc: (u16, u16), ptr: [u32; 4], md: [i16; 4], dat: [u16; 3], size: u16) {
        self.w(BLTCON0, con0);
        self.w(BLTCON1, con1);
        self.w(BLTAFWM, masc.0);
        self.w(BLTALWM, masc.1);
        self.l(BLTAPT, ptr[0]);
        self.l(BLTBPT, ptr[1]);
        self.l(BLTCPT, ptr[2]);
        self.l(BLTDPT, ptr[3]);
        self.w(BLTAMOD, md[0] as u16);
        self.w(BLTBMOD, md[1] as u16);
        self.w(BLTCMOD, md[2] as u16);
        self.w(BLTDMOD, md[3] as u16);
        self.w(BLTADAT, dat[0]);
        self.w(BLTBDAT, dat[1]);
        self.w(BLTCDAT, dat[2]);
        self.w(BLTSIZE, size);
        self.espera();
    }
    /// Una línea como `linea()` de `blitter.c`, en un mapa de `fila` bytes por fila desde `base`.
    #[allow(clippy::too_many_arguments)]
    fn linea(
        &mut self,
        base: u32,
        fila: u16,
        (x1, y1): (i32, i32),
        (x2, y2): (i32, i32),
        mt: u16,
        tex: (u16, u16),
        sing: bool,
    ) {
        let (dx, dy) = (x2 - x1, y2 - y1);
        let (ax, ay) = (dx.abs(), dy.abs());
        let (dmax, dmin, oct) = if ax >= ay {
            (ax, ay, 0x10 | if dx < 0 { 4 } else { 0 } | if dy < 0 { 8 } else { 0 })
        } else {
            (ay, ax, if dy < 0 { 4 } else { 0 } | if dx < 0 { 8 } else { 0 })
        };
        let acc = 4 * dmin - 2 * dmax;
        let p = base + (y1 as u32) * u32::from(fila) + (x1 as u32 >> 4) * 2;
        self.w(BLTCON0, ((x1 as u16 & 15) << 12) | 0x0B00 | mt);
        self.w(BLTCON1, (tex.1 << 12) | oct | if acc < 0 { 0x40 } else { 0 } | if sing { 2 } else { 0 } | 1);
        self.w(BLTAFWM, 0xFFFF);
        self.w(BLTALWM, 0xFFFF);
        self.l(BLTCPT, p);
        self.l(BLTDPT, p);
        self.w(BLTAPTL, acc as u16);
        self.w(BLTAMOD, (4 * (dmin - dmax)) as u16);
        self.w(BLTBMOD, (4 * dmin) as u16);
        self.w(BLTCMOD, fila);
        self.w(BLTDMOD, fila);
        self.w(BLTADAT, 0x8000);
        self.w(BLTBDAT, tex.0);
        self.w(BLTSIZE, (((dmax + 1) as u16) << 6) | 2);
        self.espera();
    }
}

/// Si el punto (x, y) está puesto en el mapa de `fila` bytes por fila desde `base`.
fn punto(m: &Memoria, base: u32, fila: u32, x: u32, y: u32) -> bool {
    m.p(base + y * fila + (x >> 4) * 2) & (0x8000 >> (x & 15)) != 0
}

fn puntos(m: &Memoria, base: u32, fila: u32, filas: u32) -> u32 {
    (0..filas * fila / 2).map(|i| m.p(base + 2 * i).count_ones()).sum()
}

// --- Casos a mano ---

#[test]
fn copia_simple() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[1, 2, 3, 0xFF, 4, 5, 6, 0xFF]);
    // Tres palabras de dos filas, con módulo 2 en A y 4 en D.
    a.copia(0x09F0, 0, (0xFFFF, 0xFFFF), [0x1000, 0, 0, 0x2000], [2, 0, 0, 4], [0; 3], (2 << 6) | 3);
    assert_eq!(a.m.trozo(0x2000, 8), &[1, 2, 3, 0, 0, 4, 5, 6]);
    assert!(!a.bzero());
}

#[test]
fn copia_con_desplazamiento_y_mascaras() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[0x1234, 0x5678, 0x9ABC]);
    // A desplazada 4 a la derecha: cada palabra lleva detrás los 4 bits bajos de la anterior (la primera, ceros).
    a.copia(0x49F0, 0, (0x0FFF, 0xFF00), [0x1000, 0, 0, 0x2000], [0; 4], [0; 3], (1 << 6) | 3);
    // Máscaras antes de desplazar: 0x0234, 0x5678, 0x9A00.
    assert_eq!(a.m.trozo(0x2000, 3), &[0x0023, 0x4567, 0x89A0]);
}

#[test]
fn una_palabra_lleva_las_dos_mascaras() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[0xFFFF, 0xFFFF]);
    a.copia(0x09F0, 0, (0x0FF0, 0x3C3C), [0x1000, 0, 0, 0x2000], [0; 4], [0; 3], (2 << 6) | 1);
    assert_eq!(a.m.trozo(0x2000, 2), &[0x0C30, 0x0C30]);
}

#[test]
fn copia_descendente() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[0x1234, 0x5678]);
    // Desde la última palabra hacia atrás; A desplazada 4 a la izquierda.
    a.copia(0x49F0, 0x0002, (0xFFFF, 0xFFFF), [0x1002, 0, 0, 0x2002], [0; 4], [0; 3], (1 << 6) | 2);
    assert_eq!(a.m.trozo(0x2000, 2), &[0x2345, 0x6780]);
}

#[test]
fn descendente_solapado_mueve_hacia_arriba() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[1, 2, 3, 4, 5, 0, 0]);
    // Mover cinco palabras dos más arriba en la memoria, sobre sí mismas: solo sale bien en descendente.
    a.copia(0x09F0, 0x0002, (0xFFFF, 0xFFFF), [0x1008, 0, 0, 0x100C], [0; 4], [0; 3], (1 << 6) | 5);
    assert_eq!(a.m.trozo(0x1000, 7), &[1, 2, 1, 2, 3, 4, 5]);
}

#[test]
fn galleta_con_las_cuatro() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[0xFF00]); // A: la máscara
    a.m.poner(0x1100, &[0xAAAA]); // B: el dibujo
    a.m.poner(0x1200, &[0x1234]); // C: el fondo
    a.copia(0x0FCA, 0, (0xFFFF, 0xFFFF), [0x1000, 0x1100, 0x1200, 0x1200], [0; 4], [0; 3], (1 << 6) | 1);
    assert_eq!(a.m.p(0x1200), 0xAA34);
}

#[test]
fn bzero_sin_d() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[0x00F0, 0x0F00]);
    a.m.poner(0x1100, &[0x0F00, 0x00F0]);
    // A y C sin D: no escribe, pero BZERO dice si se tocan.
    a.copia(0x0AA0, 0, (0xFFFF, 0xFFFF), [0x1000, 0, 0x1100, 0x3000], [0; 4], [0; 3], (1 << 6) | 2);
    assert!(a.bzero());
    a.m.poner(0x1100, &[0x0F00, 0x0F00]);
    a.copia(0x0AA0, 0, (0xFFFF, 0xFFFF), [0x1000, 0, 0x1100, 0x3000], [0; 4], [0; 3], (1 << 6) | 2);
    assert!(!a.bzero());
    assert_eq!(a.m.p(0x3000), 0);
}

#[test]
fn relleno_inclusivo_y_exclusivo() {
    let mut a = Amiga::nuevo();
    // Dos palabras por fila; el arrastre pasa de la de la derecha a la de la izquierda.
    let fila = [0x0100, 0x0010];
    a.m.poner(0x1000, &fila);
    a.copia(0x09F0, 0x000A, (0xFFFF, 0xFFFF), [0x1002, 0, 0, 0x2002], [0; 4], [0; 3], (1 << 6) | 2);
    assert_eq!(a.m.trozo(0x2000, 2), &[0x01FF, 0xFFF0]);
    a.copia(0x09F0, 0x0012, (0xFFFF, 0xFFFF), [0x1002, 0, 0, 0x2002], [0; 4], [0; 3], (1 << 6) | 2);
    assert_eq!(a.m.trozo(0x2000, 2), &[0x00FF, 0xFFF0]);
    // Con FCI, el arrastre empieza a 1: se rellena desde la derecha hasta el primer 1.
    a.copia(0x09F0, 0x000E, (0xFFFF, 0xFFFF), [0x1002, 0, 0, 0x2002], [0; 4], [0; 3], (1 << 6) | 2);
    assert_eq!(a.m.trozo(0x2000, 2), &[0xFF00, 0x001F]);
    a.copia(0x09F0, 0x0016, (0xFFFF, 0xFFFF), [0x1002, 0, 0, 0x2002], [0; 4], [0; 3], (1 << 6) | 2);
    assert_eq!(a.m.trozo(0x2000, 2), &[0xFF00, 0x000F]);
}

#[test]
fn relleno_vuelve_a_fci_en_cada_fila() {
    let mut a = Amiga::nuevo();
    // Una fila con un solo 1 deja el arrastre a 1; la siguiente empieza otra vez a 0.
    a.m.poner(0x1000, &[0x0100, 0x0000]);
    a.copia(0x09F0, 0x000A, (0xFFFF, 0xFFFF), [0x1002, 0, 0, 0x2002], [0; 4], [0; 3], (2 << 6) | 1);
    assert_eq!(a.m.trozo(0x2000, 2), &[0xFF00, 0x0000]);
}

/// Las ocho direcciones desde (100, 16): cada línea pone dmax + 1 puntos, el primero y el último en su sitio.
#[test]
fn lineas_en_los_ocho_octantes() {
    for (x2, y2) in [(130, 20), (130, 12), (70, 20), (70, 12), (104, 30), (104, 2), (96, 30), (96, 2)] {
        let mut a = Amiga::nuevo();
        a.linea(0x4000, 64, (100, 16), (x2, y2), 0xCA, (0xFFFF, 0), false);
        let n = (x2 - 100i32).abs().max((y2 - 16i32).abs()) as u32 + 1;
        assert!(punto(&a.m, 0x4000, 64, 100, 16), "el primero, hacia ({x2}, {y2})");
        assert!(punto(&a.m, 0x4000, 64, x2 as u32, y2 as u32), "el último, hacia ({x2}, {y2})");
        assert_eq!(puntos(&a.m, 0x4000, 64, 32), n, "hacia ({x2}, {y2})");
        // Todos entre los dos extremos.
        for y in 0..32 {
            for x in 0..512 {
                if punto(&a.m, 0x4000, 64, x, y) {
                    assert!(x as i32 >= 100.min(x2) && x as i32 <= 100.max(x2), "({x}, {y})");
                    assert!(y as i32 >= 16.min(y2) && y as i32 <= 16.max(y2), "({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn linea_a_45_grados_y_horizontal() {
    let mut a = Amiga::nuevo();
    a.linea(0x4000, 64, (10, 0), (25, 15), 0xCA, (0xFFFF, 0), false);
    for i in 0..16 {
        assert!(punto(&a.m, 0x4000, 64, 10 + i, i));
    }
    assert_eq!(puntos(&a.m, 0x4000, 64, 32), 16);
    let mut a = Amiga::nuevo();
    a.linea(0x4000, 64, (5, 3), (40, 3), 0xCA, (0xFFFF, 0), false);
    assert_eq!(a.m.trozo(0x4000 + 3 * 64, 3), &[0x07FF, 0xFFFF, 0xFF80]);
}

#[test]
fn linea_con_sing_un_punto_por_fila() {
    let mut a = Amiga::nuevo();
    // Casi horizontal: 41 puntos en 5 filas; con SING, uno por fila.
    a.linea(0x4000, 64, (0, 0), (40, 4), 0x4A, (0xFFFF, 0), true);
    assert_eq!(puntos(&a.m, 0x4000, 64, 32), 5);
    assert!(punto(&a.m, 0x4000, 64, 0, 0));
}

#[test]
fn linea_con_textura() {
    let mut a = Amiga::nuevo();
    // El patrón 0xF0F0 desde su bit 15: cuatro puntos sí, cuatro no.
    a.linea(0x4000, 64, (0, 0), (15, 0), 0xCA, (0xF0F0, 15), false);
    assert_eq!(a.m.p(0x4000), 0xF0F0);
    let mut a = Amiga::nuevo();
    a.linea(0x4000, 64, (0, 0), (15, 0), 0xCA, (0xF0F0, 11), false);
    assert_eq!(a.m.p(0x4000), 0x0F0F);
}

#[test]
fn bbusy_y_blit_a_su_hora() {
    let mut a = Amiga::nuevo();
    a.w(INTREQ, int::BLIT);
    // 16 filas de 20 palabras con A y D: 2 ciclos de bus por palabra.
    a.w(BLTCON0, 0x09F0);
    a.w(BLTCON1, 0);
    a.l(BLTAPT, 0x1000);
    a.l(BLTDPT, 0x2000);
    a.w(BLTSIZE, (16 << 6) | 20);
    let escrito = a.hora;
    assert_ne!(a.r(DMACONR) & dma::BBUSY, 0);
    assert_eq!(a.r(INTREQR) & int::BLIT, 0);
    let fin = a.c.siguiente_suceso();
    assert!(fin > escrito && fin < escrito + CICLOS_POR_LINEA * 10, "fin {fin}, escrito {escrito}");
    assert_eq!(fin - escrito, (2 + 16 * 20 * 2) * 2);
    a.c.avanzar(fin - 1);
    assert_ne!(a.c.leer(DMACONR, 2, fin - 1) & 0x4000, 0);
    a.c.avanzar(fin);
    assert_eq!(a.c.leer(DMACONR, 2, fin) & 0x4000, 0);
    assert_ne!(a.c.leer(INTREQR, 2, fin) as u16 & int::BLIT, 0);
}

#[test]
fn sin_dma_espera_a_que_se_encienda() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &[0xBEEF]);
    a.w(DMACON, dma::BLTEN);
    a.w(BLTCON0, 0x09F0);
    a.w(BLTCON1, 0);
    a.w(BLTAFWM, 0xFFFF);
    a.w(BLTALWM, 0xFFFF);
    a.l(BLTAPT, 0x1000);
    a.l(BLTDPT, 0x2000);
    a.w(BLTSIZE, (1 << 6) | 1);
    let h = a.hora + 100 * CICLOS_POR_LINEA;
    a.c.avanzar(h);
    a.hora = h;
    assert_ne!(a.r(DMACONR) & dma::BBUSY, 0);
    assert_eq!(a.m.p(0x2000), 0);
    a.w(DMACON, dma::SET_CLR | dma::BLTEN);
    a.espera();
    assert_eq!(a.m.p(0x2000), 0xBEEF);
}

#[test]
fn los_registros_del_blitter_no_se_leen() {
    let mut a = Amiga::nuevo();
    a.w(BLTCON0, 0x1234);
    assert_eq!(a.r(BLTCON0), 0);
    assert_eq!(a.c.accesos_a_nadie, 1);
}

#[test]
fn sin_memoria_no_escribe_nada() {
    // `escribir` (sin chip RAM): el blit se hace sobre nada, BBUSY y BLIT igual.
    let mut c = Chips::nuevo();
    c.escribir(DMACON, u32::from(dma::SET_CLR | dma::DMAEN | dma::BLTEN), 2, 0);
    c.escribir(BLTCON0, 0x01FF, 2, 4);
    c.escribir(BLTSIZE, (1 << 6) | 1, 2, 8);
    assert_ne!(c.leer(DMACONR, 2, 8) & 0x4000, 0);
    assert_eq!(c.leer(DMACONR, 2, 8) & 0x2000, 0);
    let fin = c.siguiente_suceso();
    c.avanzar(fin);
    assert_ne!(c.leer(INTREQR, 2, fin) as u16 & int::BLIT, 0);
}

// --- El oráculo ---

const CASOS: &str = include_str!("../../pruebas68k/blitter_casos.h");
const TAM: u32 = 4096;
/// Donde va el trozo de 4096 bytes en la chip RAM de la prueba (en FS-UAE, donde lo dé AllocMem).
const TROZO: u32 = 0x3_0000;

/// Lo que da `blitter.exe` en FS-UAE con el Kickstart 3.1 (A1200), caso a caso: CRC32 y BZERO.
const ORACULO: &[(u32, bool)] = &[
    (0x2A666A2E, false),
    (0xBC78D3E5, false),
    (0x3B54183B, false),
    (0xCB8E2A4E, false),
    (0xDE2497D8, false),
    (0x96D42087, false),
    (0x84216F15, false),
    (0x4F6034F2, false),
    (0xC71C0011, true),
    (0x661D27BD, false),
    (0x462A09E0, false),
    (0x95F36CC8, false),
    (0x6662D759, false),
    (0x4F329CBA, false),
    (0x4A1B1EC8, false),
    (0x828D43F6, false),
    (0x8445ED30, false),
    (0x8FB4C04E, false),
    (0x52021496, false),
    (0x7FCAA260, false),
    (0xF2DF98FB, false),
    (0xBDDFD549, false),
    (0x91618529, false),
    (0x0889A1BB, false),
    (0x49569278, false),
    (0xF292FE3D, false),
    (0x7F6310D6, false),
    (0xE3396F41, false),
    (0xA6F00C75, false),
    (0x66F83541, false),
    (0x4C7E98D9, false),
    (0x9E6AA168, false),
    (0xC7FEC236, false),
    (0xECBF931F, false),
    (0xF39372B5, false),
    (0xF9C7576A, false),
    (0xFF3CEF32, false),
    (0x4FF2D1E7, false),
    (0x27D6E62C, false),
    (0xBD11BF8E, false),
    (0xCC07B70C, false),
    (0xCF7C89FF, false),
];

fn numeros(args: &str) -> Vec<i64> {
    args.split(',')
        .map(|n| {
            let n = n.trim();
            let (neg, n) = n.strip_prefix('-').map_or((false, n), |n| (true, n));
            let v = n.strip_prefix("0x").map_or_else(|| n.parse().unwrap(), |h| i64::from_str_radix(h, 16).unwrap());
            if neg { -v } else { v }
        })
        .collect()
}

/// El generador de `blitter.c`.
struct Azar(u32);
impl Azar {
    fn sig(&mut self) -> u16 {
        self.0 = self.0.wrapping_mul(1103515245).wrapping_add(12345);
        (self.0 >> 16) as u16
    }
}

fn llenar(m: &mut Memoria, relleno: i64, semilla: i64) {
    let mut az = Azar(semilla as u32);
    for i in 0..TAM / 2 {
        let w = match relleno {
            0 => 0,
            1 => az.sig(),
            _ => az.sig() & az.sig() & az.sig(),
        };
        m.0[(TROZO / 2 + i) as usize] = w;
    }
}

fn crc32(palabras: &[u16]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in palabras.iter().flat_map(|w| w.to_be_bytes()) {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
        }
    }
    !c
}

/// Los registros de un caso COPIA de `blitter_casos.h` (los números de su línea), hasta `BLTSIZE`, en el orden de
/// `copia()` de `blitter.c`.
fn escribir_copia(a: &mut Amiga, v: &[i64]) {
    assert_eq!(v.len(), 18, "{v:?}");
    let w = |i: usize| v[i] as u16;
    a.w(BLTCON0, w(2));
    a.w(BLTCON1, w(3));
    a.w(BLTAFWM, w(4));
    a.w(BLTALWM, w(5));
    for (i, reg) in [(8, BLTCPT), (7, BLTBPT), (6, BLTAPT), (9, BLTDPT)] {
        if v[i] >= 0 {
            a.l(reg, TROZO + v[i] as u32);
        }
    }
    a.w(BLTAMOD, w(10));
    a.w(BLTBMOD, w(11));
    a.w(BLTCMOD, w(12));
    a.w(BLTDMOD, w(13));
    a.w(BLTCDAT, w(16));
    a.w(BLTBDAT, w(15));
    a.w(BLTADAT, w(14));
    a.w(BLTSIZE, w(17));
}

/// Los casos de `blitter_casos.h`, hechos como los hace `blitter.exe`: (CRC32, BZERO) de cada uno.
fn hacer_casos(a: &mut Amiga) -> Vec<(u32, bool)> {
    let mut sumas = Vec::new();
    for l in CASOS.lines() {
        let (copia, resto) = if let Some(r) = l.strip_prefix("COPIA(") {
            (true, r)
        } else if let Some(r) = l.strip_prefix("LINEA(") {
            (false, r)
        } else {
            continue;
        };
        let v = numeros(resto.trim_end().strip_suffix(')').unwrap());
        if v[0] >= 0 {
            llenar(&mut a.m, v[0], v[1]);
        }
        a.w(INTREQ, int::BLIT);
        if copia {
            escribir_copia(a, &v);
            a.espera();
        } else {
            assert_eq!(v.len(), 10, "{l}");
            let i = |k: usize| v[k] as i32;
            a.linea(TROZO, 64, (i(2), i(3)), (i(4), i(5)), v[6] as u16, (v[7] as u16, v[8] as u16), v[9] != 0);
        }
        assert_ne!(a.r(INTREQR) & int::BLIT, 0, "{l}");
        let z = a.bzero();
        sumas.push((crc32(a.m.trozo(TROZO, TAM as usize / 2)), z));
    }
    sumas
}

#[test]
fn oraculo() {
    let mut a = Amiga::nuevo();
    let sumas = hacer_casos(&mut a);
    for (i, s) in sumas.iter().enumerate() {
        println!("blitter: caso {}: crc {:08X} bzero {}", i + 1, s.0, u8::from(s.1));
    }
    assert_eq!(sumas.len(), ORACULO.len());
    let mal: Vec<_> = (0..sumas.len()).filter(|&i| sumas[i] != ORACULO[i]).map(|i| i + 1).collect();
    assert!(mal.is_empty(), "casos distintos de FS-UAE: {mal:?}");

    // Sin DMA (el final de blitter.c): el caso 1 otra vez con BLTEN apagado no hace nada, con BBUSY a 1, hasta
    // encenderlo. FS-UAE: "bbusy 1, crc antes B69924D7, despues 2A666A2E bzero 0".
    let caso1: Vec<i64> = CASOS
        .lines()
        .find_map(|l| l.strip_prefix("COPIA("))
        .map(|r| numeros(r.trim_end().strip_suffix(')').unwrap()))
        .unwrap();
    llenar(&mut a.m, 1, 1);
    a.w(DMACON, dma::BLTEN);
    escribir_copia(&mut a, &caso1);
    a.hora += 20 * CICLOS_POR_LINEA;
    assert_ne!(a.r(DMACONR) & dma::BBUSY, 0);
    assert_eq!(crc32(a.m.trozo(TROZO, TAM as usize / 2)), 0xB69924D7);
    a.w(DMACON, dma::SET_CLR | dma::BLTEN);
    a.espera();
    assert_eq!((crc32(a.m.trozo(TROZO, TAM as usize / 2)), a.bzero()), (0x2A666A2E, false));
}
