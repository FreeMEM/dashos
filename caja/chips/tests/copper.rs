//! Pruebas del Copper por el bus, con una chip RAM de prueba.

use caja_chips::{CICLOS_POR_LINEA, Chips, MemoriaChip, dma, int};

const LINEA: u64 = CICLOS_POR_LINEA;
const CUADRO: u64 = 313 * LINEA;

const VPOSR: u32 = 0xDFF004;
const VHPOSR: u32 = 0xDFF006;
const INTREQR: u32 = 0xDFF01E;
const VHPOSW: u32 = 0xDFF02C;
const COPCON: u32 = 0xDFF02E;
const COP1LC: u32 = 0xDFF080;
const COP2LC: u32 = 0xDFF084;
const COPJMP1: u32 = 0xDFF088;
const COPJMP2: u32 = 0xDFF08A;
const DMACON: u32 = 0xDFF096;
const INTREQ: u32 = 0xDFF09C;

/// Registros por su desplazamiento, para las listas.
const R_JOYTEST: u16 = 0x036;
const R_BLTAFWM: u16 = 0x044;
const R_DSKSYNC: u16 = 0x07E;
const R_COP2LCH: u16 = 0x084;
const R_COP2LCL: u16 = 0x086;
const R_COPJMP2: u16 = 0x08A;
const R_DMACON: u16 = 0x096;
const R_INTREQ: u16 = 0x09C;
const R_BPLCON0: u16 = 0x100;
const R_BPLCON1: u16 = 0x102;
const R_BPLCON2: u16 = 0x104;
const R_COLOR00: u16 = 0x180;

const SET: u16 = 0x8000;

/// 512 KB de chip RAM, de palabra en palabra.
struct Memoria(Vec<u16>);

impl Memoria {
    fn nueva() -> Self {
        Memoria(vec![0; 256 * 1024])
    }

    /// Pone la lista `palabras` en `dir`.
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

// --- Las instrucciones ---

fn mov(reg: u16, v: u16) -> [u16; 2] {
    [reg, v]
}

fn wait(v: u16, h: u16) -> [u16; 2] {
    [((v & 0xFF) << 8) | (h & 0xFE) | 1, 0xFFFE]
}

fn skip(v: u16, h: u16) -> [u16; 2] {
    [((v & 0xFF) << 8) | (h & 0xFE) | 1, 0xFFFF]
}

const FIN: [u16; 2] = [0xFFFF, 0xFFFE];

fn lista(ins: &[[u16; 2]]) -> Vec<u16> {
    ins.iter().flatten().copied().collect()
}

// --- El 68k de prueba ---

/// Los chips y su chip RAM, con la hora del 68k.
struct Amiga {
    c: Chips,
    m: Memoria,
    hora: u64,
}

impl Amiga {
    fn nuevo() -> Self {
        Amiga { c: Chips::nuevo(), m: Memoria::nueva(), hora: 0 }
    }

    fn leer16(&mut self, dir: u32) -> u16 {
        self.c.leer_con(dir, 2, self.hora, &mut self.m) as u16
    }

    fn escribir16(&mut self, dir: u32, v: u16) {
        self.c.escribir_con(dir, u32::from(v), 2, self.hora, &mut self.m);
    }

    fn escribir32(&mut self, dir: u32, v: u32) {
        self.c.escribir_con(dir, v, 4, self.hora, &mut self.m);
    }

    fn a(&mut self, hora: u64) -> &mut Self {
        self.hora = hora;
        self
    }

    /// El DMA del Copper encendido, como lo deja el sistema.
    fn dma(&mut self) {
        self.escribir16(DMACON, SET | dma::DMAEN | dma::COPEN);
    }

    /// La línea del haz (9 bits).
    fn linea(&mut self) -> u16 {
        let alto = self.leer16(VPOSR) & 1;
        (alto << 8) | (self.leer16(VHPOSR) >> 8)
    }

    fn intreq(&mut self) -> u16 {
        self.leer16(INTREQR)
    }

    /// Como `espera` de copper.c: lee INTREQR cada 20 ciclos hasta ver `bit`; dice la línea y si estaba el otro bit, y
    /// lo borra. `None` si no llega en dos cuadros.
    fn espera(&mut self, bit: u16) -> Option<(u16, bool)> {
        let limite = self.hora + 2 * CUADRO;
        loop {
            let r = self.intreq();
            if r & bit != 0 {
                let l = self.linea();
                self.escribir16(INTREQ, bit);
                return Some((l, r & (int::SOFT | int::COPER) & !bit != 0));
            }
            self.hora += 20;
            if self.hora > limite {
                return None;
            }
        }
    }

    /// Espera al principio de un cuadro (la línea vuelve atrás).
    fn espera_cuadro(&mut self) {
        let mut antes = self.linea();
        loop {
            self.hora += 20;
            let l = self.linea();
            if l < antes {
                return;
            }
            antes = l;
        }
    }

    /// La hora de (línea, ciclo de color) en el cuadro `n` (0, el primero).
    fn hora_de(n: u64, v: u64, h: u64) -> u64 {
        n * CUADRO + v * LINEA + h * 2
    }
}

/// Una lista en `$1000`, con el DMA encendido y COPJMP1 a la hora `hora`.
fn arrancar(ins: &[[u16; 2]], hora: u64) -> Amiga {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &lista(ins));
    a.a(hora);
    a.dma();
    a.escribir32(COP1LC, 0x1000);
    a.escribir16(COPJMP1, 0);
    a
}

#[test]
fn move_escribe_cada_4_ciclos_de_color() {
    let mut a = arrancar(
        &[mov(R_COLOR00, 0x0F00), mov(R_COLOR00 + 62, 0xF0F0), mov(R_BPLCON1, 0x0011), mov(R_BPLCON2, 0x0024), FIN],
        0,
    );
    // COPJMP1 en el ciclo 0; lee la primera palabra en el 2 y escribe en el 4, y así cada 4 ciclos de color.
    a.a(7).intreq();
    assert_eq!(a.c.color(0), 0);
    a.a(8).intreq();
    assert_eq!(a.c.color(0), 0x0F00);
    a.a(15).intreq();
    assert_eq!(a.c.color(31), 0);
    a.a(16).intreq();
    assert_eq!(a.c.color(31), 0x00F0, "COLOR31, en 12 bits");
    a.a(100).intreq();
    assert_eq!(a.c.bplcon(), [0, 0x0011, 0x0024]);
    assert_eq!(a.c.accesos_a_nadie, 0);
    assert_eq!(a.c.copper().pc(), 0x1000 + 20, "leyó el WAIT final y espera");
}

#[test]
fn move_a_intreq_y_bplcon0() {
    let mut a = arrancar(&[mov(R_INTREQ, SET | int::COPER), mov(R_BPLCON0, 0x0004), FIN], 0);
    assert_eq!(a.a(20).intreq(), int::COPER);
    assert_eq!(a.c.bplcon()[0], 0x0004);
}

#[test]
fn wait_en_una_linea_y_posicion() {
    let mut a = arrancar(&[wait(100, 0x40), mov(R_INTREQ, SET | int::SOFT), FIN], 0);
    // Despierta 2 ciclos antes ($3E), lee el MOVE en el $40 y escribe en el $42.
    assert_eq!(a.a(Amiga::hora_de(0, 100, 0x41)).intreq() & int::SOFT, 0);
    assert_eq!(a.a(Amiga::hora_de(0, 100, 0x42)).intreq() & int::SOFT, int::SOFT);
}

#[test]
fn wait_ya_pasado_sigue_enseguida() {
    let mut a = arrancar(&[wait(30, 0x07), mov(R_INTREQ, SET | int::SOFT), FIN], Amiga::hora_de(0, 100, 0));
    assert_eq!(a.a(Amiga::hora_de(0, 100, 20)).intreq() & int::SOFT, int::SOFT);
}

#[test]
fn wait_tras_la_linea_255() {
    let mut a =
        arrancar(&[wait(255, 0xDE), wait(30, 0x07), mov(R_INTREQ, SET | int::SOFT), FIN], Amiga::hora_de(0, 100, 0));
    assert_eq!(a.a(Amiga::hora_de(0, 286, 0)).intreq() & int::SOFT, 0, "en la 256 se compara como la 0");
    assert_eq!(a.a(Amiga::hora_de(0, 286, 20)).intreq() & int::SOFT, int::SOFT);
}

#[test]
fn wait_sin_ffdf_tras_la_255_no_espera() {
    // Sin el WAIT $FFDF, un WAIT a la línea 30 desde la 200 ya se cumple: la comparación es de 8 bits, pero 200 > 30.
    let mut a = arrancar(&[wait(30, 0x07), mov(R_INTREQ, SET | int::SOFT), FIN], Amiga::hora_de(0, 200, 0));
    assert_eq!(a.a(Amiga::hora_de(0, 200, 20)).intreq() & int::SOFT, int::SOFT);
}

#[test]
fn wait_que_no_llega_espera_al_cuadro_siguiente() {
    let mut a = arrancar(&[FIN], 0);
    a.a(Amiga::hora_de(0, 1, 0)).intreq();
    assert!(a.c.copper().parado());
    // Sin nada que haga el Copper, el siguiente suceso es el fin del cuadro.
    assert_eq!(a.c.siguiente_suceso(), CUADRO);
}

#[test]
fn wait_con_mascara() {
    // Con la máscara de línea a $0F (más el bit 7, siempre), se compara la línea & $8F con 5: desde la 18 ($12, que da
    // 2), se cumple en la 21 ($15, que da 5).
    let mut a = arrancar(&[[0x0501, 0x0FFE], mov(R_INTREQ, SET | int::SOFT), FIN], Amiga::hora_de(0, 18, 0));
    assert_eq!(a.a(Amiga::hora_de(0, 20, 200)).intreq() & int::SOFT, 0);
    assert_eq!(a.a(Amiga::hora_de(0, 21, 20)).intreq() & int::SOFT, int::SOFT);
}

#[test]
fn skip_que_se_cumple_y_que_no() {
    let mut a = arrancar(
        &[
            wait(80, 0x07),
            skip(70, 0x07),
            mov(R_INTREQ, SET | int::COPER),
            skip(90, 0x07),
            mov(R_INTREQ, SET | int::SOFT),
            FIN,
        ],
        0,
    );
    assert_eq!(a.a(Amiga::hora_de(0, 81, 0)).intreq(), int::SOFT);
}

#[test]
fn copjmp2_desde_el_copper() {
    let mut a = arrancar(
        &[mov(R_COP2LCH, 0), mov(R_COP2LCL, 0x2000), mov(R_COPJMP2, 0), mov(R_INTREQ, SET | int::COPER), FIN],
        0,
    );
    a.m.poner(0x2000, &lista(&[mov(R_INTREQ, SET | int::SOFT), FIN]));
    assert_eq!(a.a(200).intreq(), int::SOFT);
    assert_eq!(a.c.copper().cop2lc(), 0x2000);
}

#[test]
fn copjmp2_desde_el_68k() {
    let mut a = arrancar(&[FIN], 0);
    a.m.poner(0x2000, &lista(&[mov(R_INTREQ, SET | int::SOFT), FIN]));
    a.a(1000);
    a.escribir32(COP2LC, 0x2000);
    assert_eq!(a.intreq(), 0);
    a.escribir16(COPJMP2, 0);
    assert_eq!(a.a(1000 + 7).intreq(), 0);
    assert_eq!(a.a(1000 + 8).intreq(), int::SOFT);
}

/// Un MOVE a `reg` y después SOFT: dice si llega SOFT.
fn mover_a(reg: u16, cdang: bool) -> bool {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &lista(&[mov(reg, 0xFFFF), mov(R_INTREQ, SET | int::SOFT), FIN]));
    a.escribir16(COPCON, if cdang { 2 } else { 0 });
    a.dma();
    a.escribir32(COP1LC, 0x1000);
    a.escribir16(COPJMP1, 0);
    a.a(200).intreq() & int::SOFT != 0
}

#[test]
fn move_prohibido_sin_cdang() {
    assert!(!mover_a(R_JOYTEST, false));
    assert!(!mover_a(R_BLTAFWM, false));
    assert!(!mover_a(R_DSKSYNC, false));
    assert!(mover_a(0x080 + 0x00C, false), "COPINS ($08C) ya vale");
}

#[test]
fn move_prohibido_con_cdang() {
    assert!(!mover_a(R_JOYTEST, true), "en OCS, por debajo de $040 tampoco con CDANG");
    assert!(!mover_a(0x03E, true));
    assert!(mover_a(R_BLTAFWM, true));
    assert!(mover_a(R_DSKSYNC, true));
}

#[test]
fn el_prohibido_para_hasta_el_cuadro_siguiente() {
    let mut a = arrancar(&[mov(R_INTREQ, SET | int::COPER), mov(R_JOYTEST, 0), mov(R_INTREQ, SET | int::SOFT), FIN], 0);
    assert_eq!(a.a(Amiga::hora_de(0, 300, 0)).intreq(), int::COPER);
    a.escribir16(INTREQ, int::COPER);
    // En el cuadro siguiente vuelve a COP1LC y se para otra vez en el mismo sitio.
    assert_eq!(a.a(Amiga::hora_de(1, 0, 0)).intreq() & int::COPER, 0);
    assert_eq!(a.a(Amiga::hora_de(1, 1, 0)).intreq() & (int::COPER | int::SOFT), int::COPER);
}

#[test]
fn vuelve_a_cop1lc_en_cada_cuadro() {
    let mut a = arrancar(&[mov(R_INTREQ, SET | int::SOFT), FIN], Amiga::hora_de(0, 10, 0));
    assert_eq!(a.a(Amiga::hora_de(0, 11, 0)).intreq() & int::SOFT, int::SOFT);
    a.escribir16(INTREQ, int::SOFT);
    for n in 1..4 {
        // En la línea 0 lee el MOVE en el ciclo 2 y escribe en el 4.
        assert_eq!(a.a(Amiga::hora_de(n, 0, 3)).intreq() & int::SOFT, 0);
        assert_eq!(a.a(Amiga::hora_de(n, 0, 4)).intreq() & int::SOFT, int::SOFT);
        a.escribir16(INTREQ, int::SOFT);
    }
}

#[test]
fn sin_dma_no_hace_nada() {
    let mut a = Amiga::nuevo();
    a.m.poner(0x1000, &lista(&[mov(R_INTREQ, SET | int::SOFT), FIN]));
    a.escribir32(COP1LC, 0x1000);
    a.escribir16(COPJMP1, 0);
    a.escribir16(DMACON, SET | dma::COPEN);
    assert_eq!(a.a(5 * CUADRO + 1000).intreq() & int::SOFT, 0, "sin DMAEN");
    assert_eq!(a.c.siguiente_suceso(), 6 * CUADRO);
    // Al encender DMAEN sigue desde COP1LC (el cuadro empezó sin su DMA: escribir COP1LC mueve el contador).
    a.escribir16(DMACON, SET | dma::DMAEN);
    assert_eq!(a.a(5 * CUADRO + 1100).intreq() & int::SOFT, int::SOFT);
}

#[test]
fn un_move_a_dmacon_lo_apaga() {
    let mut a = arrancar(&[mov(R_DMACON, dma::COPEN), mov(R_INTREQ, SET | int::SOFT), FIN], 0);
    assert_eq!(a.a(Amiga::hora_de(0, 100, 0)).intreq() & int::SOFT, 0);
    assert_eq!(a.c.siguiente_suceso(), CUADRO);
}

#[test]
fn siguiente_suceso_cuenta_con_el_copper() {
    let mut a = arrancar(&[wait(100, 0x40), mov(R_INTREQ, SET | int::SOFT), FIN], 0);
    a.a(Amiga::hora_de(0, 50, 0)).intreq();
    // El siguiente paso es el despertar del WAIT, en (100, $3E).
    assert_eq!(a.c.siguiente_suceso(), Amiga::hora_de(0, 100, 0x3E));
    // Y el motor que vuelve a esa hora y avanza de suceso en suceso ve SOFT a su hora.
    let mut t = a.c.siguiente_suceso();
    while a.c.leer_con(INTREQR, 2, a.c.hora(), &mut a.m) as u16 & int::SOFT == 0 {
        a.c.avanzar_con(t, &mut a.m);
        t = a.c.siguiente_suceso();
    }
    assert_eq!(a.c.hora(), Amiga::hora_de(0, 100, 0x42));
}

#[test]
fn mover_el_haz_despierta_el_wait() {
    let mut a = arrancar(&[wait(100, 0x07), mov(R_INTREQ, SET | int::SOFT), FIN], 0);
    a.a(Amiga::hora_de(0, 10, 0)).intreq();
    a.escribir16(VHPOSW, 0x6300);
    assert_eq!(a.linea(), 99);
    assert_eq!(a.a(Amiga::hora_de(0, 11, 20)).intreq() & int::SOFT, int::SOFT, "la línea 100 llega 89 líneas antes");
}

// --- La lista de copper.c (el oráculo) ---

/// La misma lista y el mismo orden que caja/pruebas68k/copper.c: en FS-UAE con el Kickstart 3.1 da estas líneas.
#[test]
fn oraculo_copper_c() {
    const SOFT: u16 = int::SOFT;
    const COPER: u16 = int::COPER;
    let (l1, l2, l3, l4) = (0x1000u32, 0x1060u32, 0x1080u32, 0x10A0u32);
    let mut a = Amiga::nuevo();
    a.m.poner(
        l1,
        &lista(&[
            wait(40, 0x07),
            mov(R_INTREQ, SET | SOFT),
            wait(80, 0x07),
            skip(70, 0x07),
            mov(R_INTREQ, SET | COPER),
            skip(90, 0x07),
            mov(R_INTREQ, SET | SOFT),
            wait(120, 0x07),
            mov(R_INTREQ, SET | COPER),
            wait(255, 0xDE),
            wait(30, 0x07),
            mov(R_INTREQ, SET | SOFT),
            mov(R_COP2LCH, (l2 >> 16) as u16),
            mov(R_COP2LCL, l2 as u16),
            mov(R_COPJMP2, 0),
            mov(R_INTREQ, SET | COPER),
            FIN,
        ]),
    );
    a.m.poner(l2, &lista(&[wait(300, 0x07), mov(R_INTREQ, SET | COPER), FIN]));
    a.m.poner(l3, &lista(&[wait(40, 0x07), mov(R_BLTAFWM, 0xFFFF), mov(R_INTREQ, SET | SOFT), FIN]));
    a.m.poner(l4, &lista(&[wait(40, 0x07), mov(R_BLTAFWM, 0xFFFF), mov(R_INTREQ, SET | SOFT), FIN]));

    // El sistema: el Copper en marcha con su lista (aquí, una que no hace nada), y el 68k empieza a mitad de cuadro.
    a.m.poner(0x3000, &FIN);
    a.dma();
    a.escribir32(COP1LC, 0x3000);
    a.escribir16(COPJMP1, 0);
    a.a(Amiga::hora_de(0, 150, 33));

    // Cuadros A y B.
    a.escribir16(INTREQ, SOFT | COPER);
    a.escribir32(COP1LC, l1);
    let avisos = [(SOFT, 40), (SOFT, 80), (COPER, 120), (SOFT, 286), (COPER, 300), (SOFT, 40)];
    let mut vistos = Vec::new();
    for (bit, linea) in avisos {
        let (l, otro) = a.espera(bit).expect("el aviso llega");
        assert!(!otro, "el otro bit, con el aviso de la línea {linea}");
        assert!(l == linea || l == linea + 1, "aviso de la línea {linea}, visto en la {l}");
        vistos.push(l);
    }
    assert_eq!(vistos, [40, 80, 120, 286, 300, 40]);

    // Cuadro C: lista 3, sin CDANG: el Copper se para.
    a.escribir32(COP1LC, l3);
    a.espera_cuadro();
    a.escribir16(INTREQ, SOFT | COPER);
    while a.linea() < 200 {
        a.hora += 20;
    }
    assert_eq!(a.intreq() & SOFT, 0, "sin CDANG, SOFT no llega");

    // Cuadro D: lista 4, con CDANG.
    a.escribir16(COPCON, 2);
    a.escribir32(COP1LC, l4);
    let (l, otro) = a.espera(SOFT).expect("con CDANG, SOFT llega");
    assert!(!otro);
    assert_eq!(l, 40);
}
