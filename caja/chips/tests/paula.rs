//! Pruebas del audio de Paula por el bus, como lo ve el 68k: la hora de cada `AUDx`, los bloques, el cambio de
//! `AUDxLC` y `AUDxLEN` a mitad, el modo manual, la modulación, las muestras, y el oráculo (las líneas entre
//! interrupciones que mide `caja/pruebas68k/paula.c` en FS-UAE con el Kickstart 3.1).

use caja_chips::memoria::MemoriaChip;
use caja_chips::{CICLOS_POR_LINEA, Chips, dma, int};

const LINEA: u64 = CICLOS_POR_LINEA;
const ADKCONR: u32 = 0xDFF010;
const INTREQR: u32 = 0xDFF01E;
const DMACONR: u32 = 0xDFF002;
const DMACON: u32 = 0xDFF096;
const INTREQ: u32 = 0xDFF09C;
const ADKCON: u32 = 0xDFF09E;

fn lc(x: u32) -> u32 {
    0xDFF0A0 + 0x10 * x
}
fn len(x: u32) -> u32 {
    0xDFF0A4 + 0x10 * x
}
fn per(x: u32) -> u32 {
    0xDFF0A6 + 0x10 * x
}
fn vol(x: u32) -> u32 {
    0xDFF0A8 + 0x10 * x
}
fn dat(x: u32) -> u32 {
    0xDFF0AA + 0x10 * x
}
fn bit(x: u32) -> u16 {
    int::AUD0 << x
}

/// 512 KB de chip RAM, en palabras, que apunta qué direcciones lee el DMA.
struct Memoria {
    m: Vec<u16>,
    leidas: Vec<u32>,
}

impl MemoriaChip for Memoria {
    fn leer_palabra(&mut self, dir: u32) -> u16 {
        self.leidas.push(dir);
        self.m.get(dir as usize / 2).copied().unwrap_or(0)
    }
    fn escribir_palabra(&mut self, dir: u32, valor: u16) {
        if let Some(w) = self.m.get_mut(dir as usize / 2) {
            *w = valor;
        }
    }
}

/// Los chips con la memoria y la hora del 68k.
struct Amiga {
    c: Chips,
    m: Memoria,
    hora: u64,
}

impl Amiga {
    fn nuevo() -> Self {
        Amiga { c: Chips::nuevo(), m: Memoria { m: vec![0; 256 * 1024], leidas: Vec::new() }, hora: 0 }
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
        self.c.leer_con(dir, 2, self.hora, &mut self.m) as u16
    }
    /// Va al principio de la línea siguiente.
    fn a_linea(&mut self) {
        self.hora = (self.hora / LINEA + 1) * LINEA;
        self.c.avanzar_con(self.hora, &mut self.m);
    }
    /// Pone las palabras en `dir`.
    fn poner(&mut self, dir: u32, v: &[u16]) {
        let i = dir as usize / 2;
        self.m.m[i..i + v.len()].copy_from_slice(v);
    }
    /// Avanza de suceso en suceso hasta que alguno de los `bits` sube en `INTREQ` (como mucho, `limite` ciclos). Da la
    /// hora a la que subió y cuáles subieron.
    fn espera(&mut self, bits: u16, limite: u64) -> Option<(u64, u16)> {
        let fin = self.hora + limite;
        while self.hora < fin {
            let t = self.c.siguiente_suceso().min(fin);
            assert!(t > self.c.hora());
            self.hora = t;
            self.c.avanzar_con(t, &mut self.m);
            let rq = self.c.leer_con(INTREQR, 2, t, &mut self.m) as u16 & bits;
            if rq != 0 {
                return Some((t, rq));
            }
        }
        None
    }
    /// Un canal con DMA: `AUDxLC`, `AUDxLEN`, `AUDxPER` y `AUDxVOL`.
    fn canal(&mut self, x: u32, dir: u32, l: u16, p: u16, v: u16) {
        self.l(lc(x), dir);
        self.w(len(x), l);
        self.w(per(x), p);
        self.w(vol(x), v);
    }
}

/// Las horas de las `n` primeras `AUDx` del canal `x`, tras encender su DMA al principio de una línea (más 20
/// ciclos). Cada una se borra 20 ciclos después. Con `len2`, tras la segunda se escribe ese `AUDxLEN`.
fn horas_irq(x: u32, l: u16, p: u16, n: usize, len2: Option<u16>) -> (u64, Vec<u64>) {
    let mut a = Amiga::nuevo();
    a.canal(x, 0x1000, l, p, 0);
    a.a_linea();
    a.hora += 16;
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | (1 << x));
    let inicio = a.hora;
    let mut horas = Vec::new();
    while horas.len() < n {
        let (t, _) = a.espera(bit(x), 1000 * 313 * LINEA).expect("sin AUDx");
        horas.push(t);
        a.hora += 20;
        a.w(INTREQ, bit(x));
        if horas.len() == 2
            && let Some(l2) = len2
        {
            a.w(len(x), l2);
        }
    }
    (inicio, horas)
}

#[test]
fn la_hora_de_cada_audx_con_dma() {
    // Encendido a la hora T: la primera palabra (la que no suena) se lee en la línea siguiente, en el ciclo de color
    // $0D del canal 0, y AUD0 sube un ciclo de color después. La segunda, una línea más tarde, empieza a sonar; la
    // última del bloque empieza LEN - 1 palabras después y sube AUD0. Luego, cada LEN palabras.
    for (l, p) in [(4u16, 200u16), (1, 300), (64, 227), (10, 1000), (3, 124)] {
        let (inicio, h) = horas_irq(0, l, p, 5, None);
        let linea_sig = (inicio / LINEA + 1) * LINEA;
        let primera = linea_sig + 0x0D * 2;
        assert_eq!(h[0], primera + 2, "LEN {l} PER {p}");
        let palabra = 2 * 2 * u64::from(p);
        // Con LEN 1, la palabra que recarga es la segunda: el primer bloque suena como si tuviera dos.
        assert_eq!(h[1], primera + LINEA + (u64::from(l.max(2)) - 1) * palabra + 2, "LEN {l} PER {p}");
        for k in 2..5 {
            assert_eq!(h[k] - h[k - 1], u64::from(l) * palabra, "LEN {l} PER {p}");
        }
    }
}

#[test]
fn cada_canal_lee_en_su_ciclo() {
    for x in 0..4 {
        let (inicio, h) = horas_irq(x, 8, 300, 1, None);
        let primera = (inicio / LINEA + 1) * LINEA + (0x0D + 2 * u64::from(x)) * 2;
        assert_eq!(h[0], primera + 2, "canal {x}");
    }
}

#[test]
fn len_y_per_cero_son_65536() {
    // LEN 0: 65536 palabras por bloque. Con PER 200, 65536 × 800 ciclos.
    let (_, h) = horas_irq(0, 0, 200, 3, None);
    assert_eq!(h[2] - h[1], 65536 * 800);
}

#[test]
fn varios_bloques_leen_lc_cada_vez() {
    let mut a = Amiga::nuevo();
    a.canal(0, 0x2000, 3, 150, 64);
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::AUD0EN);
    for _ in 0..4 {
        a.espera(int::AUD0, 313 * LINEA).unwrap();
        a.w(INTREQ, int::AUD0);
    }
    // La primera lectura es con el puntero de antes (0) y no suena; después, LC, LC+2 y LC+4 en cada bloque.
    let l = &a.m.leidas;
    assert_eq!(l[0], 0);
    for (i, d) in l[1..10].iter().enumerate() {
        assert_eq!(*d, 0x2000 + 2 * (i as u32 % 3), "lectura {}", i + 1);
    }
}

#[test]
fn lc_y_len_nuevos_se_toman_en_la_vuelta_siguiente() {
    let mut a = Amiga::nuevo();
    a.canal(0, 0x2000, 4, 200, 64);
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::AUD0EN);
    let (t1, _) = a.espera(int::AUD0, 313 * LINEA).unwrap();
    a.w(INTREQ, int::AUD0);
    let (t2, _) = a.espera(int::AUD0, 313 * LINEA).unwrap();
    a.w(INTREQ, int::AUD0);
    // Tras la segunda: el bloque que empieza aún es el viejo; el siguiente, el nuevo.
    a.l(lc(0), 0x3000);
    a.w(len(0), 2);
    let (t3, _) = a.espera(int::AUD0, 313 * LINEA).unwrap();
    a.w(INTREQ, int::AUD0);
    let (t4, _) = a.espera(int::AUD0, 313 * LINEA).unwrap();
    a.w(INTREQ, int::AUD0);
    let (t5, _) = a.espera(int::AUD0, 313 * LINEA).unwrap();
    let palabra = 4 * 200;
    assert!(t2 > t1);
    assert_eq!(t3 - t2, 4 * palabra);
    assert_eq!(t4 - t3, 2 * palabra);
    assert_eq!(t5 - t4, 2 * palabra);
    let l = &a.m.leidas;
    // Lectura 0 la que no suena; 1-4 el primer bloque; 5-8 el segundo (aún de $2000); luego de $3000, dos por vuelta.
    let esperadas: Vec<u32> =
        [0, 0x2000, 0x2002, 0x2004, 0x2006, 0x2000, 0x2002, 0x2004, 0x2006, 0x3000, 0x3002, 0x3000]
            .into_iter()
            .collect();
    assert_eq!(&l[..esperadas.len()], &esperadas[..]);
}

#[test]
fn apagar_el_dma_para_el_canal() {
    let mut a = Amiga::nuevo();
    a.canal(1, 0x2000, 2, 200, 64);
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::AUD1EN);
    a.espera(int::AUD1, 313 * LINEA).unwrap();
    a.w(INTREQ, int::AUD1);
    assert_eq!(a.r(DMACONR) & 0xF, dma::AUD1EN);
    a.w(DMACON, dma::AUD1EN);
    // Acaba la palabra que suena y suena una más (AUD1 a 0: modo manual), que sube AUD1; luego, con AUD1 a 1, se para.
    let (_, rq) = a.espera(int::AUD1, 313 * LINEA).unwrap();
    assert_eq!(rq, int::AUD1);
    assert!(a.espera(int::AUD1 | int::AUD0, 2 * 313 * LINEA).is_none() || a.r(INTREQR) & int::AUD1 != 0);
    let antes = a.m.leidas.len();
    a.hora += 10 * 313 * LINEA;
    a.c.avanzar_con(a.hora, &mut a.m);
    assert_eq!(a.m.leidas.len(), antes);
}

#[test]
fn modo_manual() {
    let mut a = Amiga::nuevo();
    a.w(per(2), 300);
    a.w(vol(2), 64);
    a.w(dat(2), 0x7F80);
    let t0 = a.hora;
    // Sube AUD2 al empezar (un ciclo de color después) y otra vez a cada palabra si el programa la borra.
    let (t, rq) = a.espera(int::AUD2, LINEA).unwrap();
    assert_eq!((t, rq), (t0 + 2, int::AUD2));
    let mut antes = t;
    for _ in 0..5 {
        a.w(INTREQ, int::AUD2);
        a.w(dat(2), 0x0102);
        let (t, _) = a.espera(int::AUD2, 313 * LINEA).unwrap();
        assert_eq!(t - antes, 2 * 2 * 300);
        antes = t;
    }
    // Sin borrarla, se para al acabar la palabra: ninguna más, y siguiente_suceso no la anuncia.
    a.hora += 2000;
    a.c.avanzar_con(a.hora, &mut a.m);
    a.w(INTREQ, int::AUD2);
    assert!(a.espera(int::AUD2, 313 * LINEA).is_none());
    // Escribir AUDxDAT con AUD2 a 1 no la pone en marcha.
    a.w(INTREQ, int::SET_CLR | int::AUD2);
    a.w(dat(2), 1);
    a.w(INTREQ, int::AUD2);
    assert!(a.espera(int::AUD2, 313 * LINEA).is_none());
}

#[test]
fn las_interrupciones_suben_sin_intena() {
    let mut a = Amiga::nuevo();
    a.canal(3, 0x2000, 2, 200, 0);
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::AUD3EN);
    let s = a.c.siguiente_suceso();
    a.c.avanzar_con(s, &mut a.m);
    assert_eq!(a.c.leer_con(INTREQR, 2, s, &mut a.m) as u16, int::AUD3);
    assert_eq!(a.c.ipl(), 0);
    a.w(0xDFF09A, int::SET_CLR | int::INTEN | int::AUD3);
    assert_eq!(a.c.ipl(), 4);
}

#[test]
fn sin_dmaen_no_hay_dma() {
    let mut a = Amiga::nuevo();
    a.canal(0, 0x2000, 2, 200, 0);
    a.w(DMACON, dma::SET_CLR | dma::AUD0EN);
    assert!(a.espera(int::AUD0, 313 * LINEA).is_none());
    assert!(a.m.leidas.is_empty());
    a.w(DMACON, dma::SET_CLR | dma::DMAEN);
    assert!(a.espera(int::AUD0, 313 * LINEA).is_some());
}

#[test]
fn adkcon_se_lee_en_adkconr() {
    let mut a = Amiga::nuevo();
    a.w(ADKCON, 0x8000 | 0x1101);
    assert_eq!(a.r(ADKCONR), 0x1101);
    a.w(ADKCON, 0x0100);
    assert_eq!(a.r(ADKCONR), 0x1001);
}

/// Las muestras de la izquierda o la derecha, sin repetir las seguidas iguales.
fn distintas(buf: &[i16], lado: usize) -> Vec<i16> {
    let mut v: Vec<i16> = Vec::new();
    for m in buf.chunks_exact(2) {
        if v.last() != Some(&m[lado]) {
            v.push(m[lado]);
        }
    }
    v
}

#[test]
fn las_muestras_son_las_del_bloque_con_su_volumen() {
    let mut a = Amiga::nuevo();
    // Canal 0 a la izquierda, volumen 32; canal 1 a la derecha, volumen 64. Un byte dura 2000 ciclos de color: unas
    // 27 muestras a 48 kHz.
    a.poner(0x2000, &[0x0102, 0x0304]);
    a.poner(0x3000, &[0xFF80, 0x7F10]);
    a.canal(0, 0x2000, 2, 2000, 32);
    a.canal(1, 0x3000, 2, 2000, 64);
    a.c.sonido(48000, false);
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::AUD0EN | dma::AUD1EN);
    let mut buf = vec![0i16; 2 * 4096];
    let mut todas = Vec::new();
    for _ in 0..4 {
        a.hora += 20_000;
        let n = a.c.muestras(a.hora, &mut a.m, &mut buf);
        todas.extend_from_slice(&buf[..2 * n]);
    }
    // 80000 ciclos a 48 kHz: unas 541 muestras.
    assert!((540..=542).contains(&(todas.len() / 2)), "{}", todas.len() / 2);
    let izq = distintas(&todas, 0);
    let der = distintas(&todas, 1);
    // Mezcla: (canal 0 + canal 3) × 2 a la izquierda; el byte con signo por el volumen.
    let i: Vec<i16> = [0i16, 1, 2, 3, 4, 1, 2, 3, 4].iter().map(|b| b * 32 * 2).collect();
    assert_eq!(&izq[..9], &i[..]);
    let d: Vec<i16> = [0i16, -1, -128, 127, 16, -1, -128].iter().map(|b| b * 64 * 2).collect();
    assert_eq!(&der[..7], &d[..]);
    assert_eq!(a.c.muestras_perdidas(), 0);
}

#[test]
fn el_volumen_nuevo_va_en_la_palabra_siguiente_y_se_pierden_las_que_no_se_sacan() {
    let mut a = Amiga::nuevo();
    a.w(per(3), 1000);
    a.w(vol(3), 10);
    a.c.sonido(48000, false);
    a.w(dat(3), 0x0101);
    a.w(INTREQ, int::AUD3);
    a.w(vol(3), 20);
    a.hora += 6000;
    let mut buf = vec![0i16; 2 * 4096];
    let n = a.c.muestras(a.hora, &mut a.m, &mut buf);
    assert_eq!(distintas(&buf[..2 * n], 0), vec![0, 20, 40]);
    // Sin sacarlas, el anillo se llena y se pierden las viejas.
    a.hora += 2 * caja_chips::RELOJ_CPU_PAL;
    a.c.avanzar_con(a.hora, &mut a.m);
    assert!(a.c.muestras_perdidas() > 90_000);
    let n = a.c.muestras(a.hora, &mut a.m, &mut buf);
    assert_eq!(n, caja_chips::paula::MUESTRAS_ANILLO);
}

#[test]
fn el_filtro_suaviza() {
    let mut a = Amiga::nuevo();
    a.w(per(0), 3000);
    a.w(vol(0), 64);
    a.c.sonido(48000, true);
    a.w(dat(0), 0x7F7F);
    a.hora += 4000;
    let mut buf = vec![0i16; 2 * 4096];
    let n = a.c.muestras(a.hora, &mut a.m, &mut buf);
    let izq: Vec<i32> = buf[..2 * n].chunks_exact(2).map(|m| i32::from(m[0])).collect();
    // Sube poco a poco hacia 127 × 64 × 2 sin pasarse.
    let fin: i32 = 127 * 64 * 2;
    assert!(izq[1] > 0 && izq[1] < fin / 2, "{:?}", &izq[..4]);
    assert!(izq.windows(2).all(|w| w[1] >= w[0]));
    assert!(*izq.last().unwrap() > fin * 9 / 10 && *izq.last().unwrap() <= fin);
}

#[test]
fn modulacion_de_volumen() {
    let mut a = Amiga::nuevo();
    // El canal 0 modula el volumen del 1 (USE0V1): sus palabras son volúmenes y él no suena.
    a.poner(0x2000, &[10, 40]);
    a.poner(0x3000, &[0x0101]);
    a.w(ADKCON, 0x8000 | 0x0001);
    a.canal(0, 0x2000, 2, 1000, 64);
    a.canal(1, 0x3000, 1, 250, 0);
    a.c.sonido(48000, false);
    a.w(DMACON, dma::SET_CLR | dma::DMAEN | dma::AUD0EN | dma::AUD1EN);
    a.hora += 40_000;
    let mut buf = vec![0i16; 2 * 4096];
    let n = a.c.muestras(a.hora, &mut a.m, &mut buf);
    assert!(distintas(&buf[..2 * n], 0).iter().all(|&m| m == 0));
    let d = distintas(&buf[..2 * n], 1);
    assert!(d.contains(&20) && d.contains(&80), "{d:?}");
}

/// Lo que mide `paula.c` en FS-UAE (A1200, Kickstart 3.1): por caso, canal, LEN, PER, el LEN que se escribe tras la
/// segunda (o 0), y las líneas hasta la primera AUDx y entre las siguientes.
const ORACULO: &[(u32, u16, u16, u16, [u64; 5])] = &[
    (0, 64, 227, 0, [1, 127, 128, 128, 128]),
    (0, 100, 300, 0, [1, 263, 264, 265, 264]),
    (1, 32, 454, 0, [1, 125, 128, 128, 128]),
    (2, 500, 124, 0, [1, 546, 546, 546, 547]),
    (3, 10, 1000, 0, [1, 80, 88, 89, 88]),
    (0, 1, 2000, 0, [1, 19, 17, 18, 18]),
    (0, 50, 400, 20, [1, 174, 176, 70, 71]),
    // Por debajo del mínimo del DMA (124): una palabra por línea.
    (0, 40, 100, 0, [1, 41, 40, 40, 39]),
    (1, 40, 60, 0, [1, 40, 41, 40, 39]),
    // Los dos a la vez (caso 10).
    (1, 64, 227, 0, [1, 127, 128, 128, 128]),
    (2, 20, 500, 0, [1, 85, 88, 88, 88]),
];

/// El modo manual en `paula.c`: canal, PER y, en FS-UAE, las líneas hasta la primera `AUDx` y las de 20 palabras.
const ORACULO_MANUAL: &[(u32, u16, u64, u64)] = &[(0, 1000, 0, 177), (3, 300, 0, 53)];

/// Las líneas que cuenta `paula.c` entre `desde` y `hasta`: los cambios de línea del haz.
fn lineas(desde: u64, hasta: u64) -> u64 {
    hasta / LINEA - desde / LINEA
}

#[test]
fn oraculo_lineas_entre_interrupciones() {
    for &(x, l, p, l2, fs) in ORACULO {
        let (inicio, h) = horas_irq(x, l, p, 5, (l2 != 0).then_some(l2));
        let mias =
            [lineas(inicio, h[0]), lineas(h[0], h[1]), lineas(h[1], h[2]), lineas(h[2], h[3]), lineas(h[3], h[4])];
        for k in 0..5 {
            assert!(mias[k].abs_diff(fs[k]) <= 1, "canal {x} LEN {l} PER {p}: {mias:?} frente a {fs:?}");
        }
    }
}

#[test]
fn oraculo_modo_manual() {
    for &(x, p, fs_primera, fs_veinte) in ORACULO_MANUAL {
        let mut a = Amiga::nuevo();
        a.w(per(x), p);
        a.w(vol(x), 0);
        a.a_linea();
        a.hora += 16;
        let inicio = a.hora;
        a.w(dat(x), 0x1234);
        let (t0, _) = a.espera(bit(x), LINEA).unwrap();
        let mut t = t0;
        for _ in 0..20 {
            a.hora += 20;
            a.w(INTREQ, bit(x));
            a.w(dat(x), 0x1234);
            t = a.espera(bit(x), 313 * LINEA).unwrap().0;
        }
        assert!(lineas(inicio, t0).abs_diff(fs_primera) <= 1, "canal {x} PER {p}");
        assert!(lineas(t0, t).abs_diff(fs_veinte) <= 1, "canal {x} PER {p}: {}", lineas(t0, t));
    }
}
