//! B5: los CIA completos. Las dudas de K1 con lo que se midió en FS-UAE (`caja/pruebas68k/cia.c`, A1200 y A500 con el
//! Kickstart 3.1), el registro serie en los dos sentidos, el teclado con y sin saludo y los puertos.

use caja_chips::cia::{cr, icr, reg};
use caja_chips::teclado::{self, de_la_linea, en_la_linea};
use caja_chips::{CICLOS_POR_E, CICLOS_POR_LINEA, Chips, RELOJ_CPU_PAL, int};

const LINEA: u64 = CICLOS_POR_LINEA;
const CUADRO: u64 = 313 * LINEA;
const E: u64 = CICLOS_POR_E;
const INTREQR: u32 = 0xDFF01E;
const INTREQ: u32 = 0xDFF09C;
const VHPOSR: u32 = 0xDFF006;

fn us(n: u64) -> u64 {
    n * RELOJ_CPU_PAL / 1_000_000
}

fn a(r: u8) -> u32 {
    0xBFE001 + u32::from(r) * 0x100
}

fn b(r: u8) -> u32 {
    0xBFD000 + u32::from(r) * 0x100
}

fn leer_a(c: &mut Chips, r: u8, t: u64) -> u8 {
    c.leer(a(r), 1, t) as u8
}

fn escribir_a(c: &mut Chips, r: u8, v: u8, t: u64) {
    c.escribir(a(r), u32::from(v), 1, t);
}

fn leer_b(c: &mut Chips, r: u8, t: u64) -> u8 {
    c.leer(b(r), 1, t) as u8
}

fn escribir_b(c: &mut Chips, r: u8, v: u8, t: u64) {
    c.escribir(b(r), u32::from(v), 1, t);
}

// --- Lo que deja el Kickstart y los registros sencillos ---

#[test]
fn tras_el_arranque_como_fs_uae() {
    // FS-UAE (A1200 y A500, Kickstart 3.1): CIA-A PRA $FC, DDRA $03, DDRB $00; CIA-B PRB $FF, DDRA $C0, DDRB $FF. El PRA del
    // CIA-B da $07 en FS-UAE (las líneas DSR, CTS y CD del puerto serie, activas); aquí, sin nada conectado, $3F.
    let mut c = Chips::tras_el_arranque();
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0xFC);
    assert_eq!(leer_a(&mut c, reg::DDRA, 0), 0x03);
    assert_eq!(leer_a(&mut c, reg::DDRB, 0), 0x00);
    assert_eq!(leer_b(&mut c, reg::PRA, 0), 0x3F);
    assert_eq!(leer_b(&mut c, reg::PRB, 0), 0xFF);
    assert_eq!(leer_b(&mut c, reg::DDRA, 0), 0xC0);
    assert_eq!(leer_b(&mut c, reg::DDRB, 0), 0xFF);
    assert!(!c.ovl());
    assert!(c.led());
    // Recién encendidos, sin escribir DDRA, OVL se lee a 1 (la ROM en 0) y el LED está apagado.
    let c = Chips::nuevo();
    assert!(c.ovl());
    assert!(!c.led());
}

#[test]
fn el_registro_b_se_lee_0() {
    // FS-UAE: $00 en los dos (vAmiga da $FF).
    let mut c = Chips::nuevo();
    assert_eq!(c.leer(0xBFEB01, 1, 0), 0);
    assert_eq!(c.leer(0xBFDB00, 1, 0), 0);
    assert_eq!(c.accesos_a_nadie, 0);
}

#[test]
fn espejos_como_el_a500() {
    // Lo mismo que cia.c ("espejos"), con SDR. FS-UAE (A500): escribir $5A en $BFCC01 llega a los dos; $A5 en $BFEC00
    // (par), al A; $3C en $BFDC01 (impar), al B; $C3 en $BFFC01, a ninguno; $66 en $BFEC41, al A; $1234 en $BFCC00, $34
    // al A y $12 al B. Leer con A = $11 y B = $22: $BFCC00 $22, $BFCC01 $11, $BFEC81 $11, $BFDCFE $22; de 2 bytes,
    // $BFCC00 $2211, $BFEC00 $FF11, $BFDC00 $22FF. Un byte que no lleva ningún CIA es lo que quedó en el bus (en
    // FS-UAE, $00 o $21); aquí $FF, como la mitad vacía de un acceso de 2 bytes.
    let mut c = Chips::nuevo();
    let sdr = |c: &mut Chips| (leer_a(c, reg::SDR, 0), leer_b(c, reg::SDR, 0));
    c.escribir(0xBFCC01, 0x5A, 1, 0);
    assert_eq!(sdr(&mut c), (0x5A, 0x5A));
    c.escribir(0xBFEC00, 0xA5, 1, 0);
    assert_eq!(sdr(&mut c), (0xA5, 0x5A));
    c.escribir(0xBFDC01, 0x3C, 1, 0);
    assert_eq!(sdr(&mut c), (0xA5, 0x3C));
    c.escribir(0xBFFC01, 0xC3, 1, 0);
    assert_eq!(sdr(&mut c), (0xA5, 0x3C));
    assert_eq!(c.accesos_a_nadie, 1);
    c.escribir(0xBFEC41, 0x66, 1, 0);
    assert_eq!(sdr(&mut c), (0x66, 0x3C));
    c.escribir(0xBFCC00, 0x1234, 2, 0);
    assert_eq!(sdr(&mut c), (0x34, 0x12));
    escribir_a(&mut c, reg::SDR, 0x11, 0);
    escribir_b(&mut c, reg::SDR, 0x22, 0);
    assert_eq!(c.leer(0xBFCC00, 1, 0), 0x22);
    assert_eq!(c.leer(0xBFCC01, 1, 0), 0x11);
    assert_eq!(c.leer(0xBFEC81, 1, 0), 0x11);
    assert_eq!(c.leer(0xBFDCFE, 1, 0), 0x22);
    assert_eq!(c.leer(0xBFCC00, 2, 0), 0x2211);
    assert_eq!(c.leer(0xBFEC00, 2, 0), 0xFF11);
    assert_eq!(c.leer(0xBFDC00, 2, 0), 0x22FF);
    assert_eq!(c.leer(0xBFEC00, 1, 0), 0xFF);
    // Leer la mitad que no se toma también lee el registro: el ICR del CIA-A se borra leyendo $BFED00.
    escribir_a(&mut c, reg::TALO, 0, 0);
    escribir_a(&mut c, reg::CRA, cr::RUNMODE, 0);
    escribir_a(&mut c, reg::TAHI, 0, 0);
    c.leer(0xBFED00, 1, 100);
    assert_eq!(leer_a(&mut c, reg::ICR, 100), 0);
}

#[test]
fn el_bit_7_de_cra_no_existe() {
    // FS-UAE: CRA del CIA-B escrito $84 se lee $04; CRB escrito $84 se lee $84. LOAD se lee 0.
    let mut c = Chips::nuevo();
    escribir_b(&mut c, reg::CRA, 0x84, 0);
    assert_eq!(leer_b(&mut c, reg::CRA, 0), 0x04);
    escribir_b(&mut c, reg::CRB, 0x84, 0);
    assert_eq!(leer_b(&mut c, reg::CRB, 0), 0x84);
    escribir_b(&mut c, reg::CRB, 0x94, 0);
    assert_eq!(leer_b(&mut c, reg::CRB, 0), 0x84);
}

#[test]
fn el_bit_14_de_intreq_se_guarda() {
    // FS-UAE: $C000 en INTREQ deja $4040 en INTREQR (VERTB estaba), y $4000 lo quita.
    let mut c = Chips::nuevo();
    c.escribir(INTREQ, 0x8020, 2, 0);
    c.escribir(INTREQ, 0xC000, 2, 0);
    assert_eq!(c.leer(INTREQR, 2, 0), 0x4020);
    c.escribir(INTREQ, 0x4000, 2, 0);
    assert_eq!(c.leer(INTREQR, 2, 0), 0x0020);
    // No pide interrupción: no es de ningún nivel.
    c.escribir(0xDFF09A, 0xC000 | 0x3FFF, 2, 0);
    c.escribir(INTREQ, 0x7FFF, 2, 0);
    c.escribir(INTREQ, 0xC000, 2, 0);
    assert_eq!(c.ipl(), 0);
}

#[test]
fn intreq_por_nivel_como_en_fs_uae() {
    // FS-UAE ("nivel"): el temporizador A del CIA-B, una vez, con su máscara. EXTER sube; borrado sin leer el ICR
    // sigue a 1; el ICR da $81; tras leerlo y borrar, 0.
    let mut c = Chips::nuevo();
    escribir_b(&mut c, reg::ICR, icr::IR_SC | icr::TA, 0);
    escribir_b(&mut c, reg::CRA, 0, 0);
    escribir_b(&mut c, reg::TALO, 100, 0);
    escribir_b(&mut c, reg::TAHI, 0, 0);
    escribir_b(&mut c, reg::CRA, cr::RUNMODE | cr::LOAD | cr::START, 0);
    let t = c.siguiente_suceso();
    assert_eq!(c.leer(INTREQR, 2, t) as u16 & int::EXTER, int::EXTER);
    c.escribir(INTREQ, u32::from(int::EXTER), 2, t);
    assert_eq!(c.leer(INTREQR, 2, t) as u16 & int::EXTER, int::EXTER);
    assert_eq!(leer_b(&mut c, reg::ICR, t), 0x81);
    c.escribir(INTREQ, u32::from(int::EXTER), 2, t);
    assert_eq!(c.leer(INTREQR, 2, t) as u16 & int::EXTER, 0);
}

// --- Dónde caen VSYNC y HSYNC ---

/// La primera hora (de ciclo de color en ciclo de color desde `desde`) en que `f` da algo distinto.
fn primer_cambio(c: &mut Chips, desde: u64, f: impl Fn(&mut Chips, u64) -> u32) -> u64 {
    let v0 = f(c, desde);
    let mut t = desde;
    while f(c, t) == v0 {
        t += 2;
    }
    t
}

/// (línea, ciclo de color) de una hora, por VHPOSR (con V8 a 0 cerca del principio del cuadro).
fn posicion(c: &mut Chips, t: u64) -> (u16, u16) {
    let w = c.leer(VHPOSR, 2, t) as u16;
    (w >> 8, w & 0xFF)
}

#[test]
fn vsync_cae_en_la_linea_5_y_hsync_al_empezar_la_linea() {
    let mut c = Chips::tras_el_arranque();
    // FS-UAE: VERTB entre la línea 312 ($38 sin V8) y la 0 ciclo $14; el TOD del CIA-A cambia entre la línea 4,
    // ciclo $D3, y la 5, ciclo $0B (las tres medidas juntas).
    let desde = 2 * CUADRO + 200 * LINEA;
    c.escribir(INTREQ, u32::from(int::VERTB), 2, desde);
    let vertb = primer_cambio(&mut c, desde, |c, t| c.leer(INTREQR, 2, t) & u32::from(int::VERTB));
    assert_eq!(vertb, 3 * CUADRO);
    let tod = primer_cambio(&mut c, desde, |c, t| c.leer(a(reg::TODLO), 1, t));
    let (l, h) = posicion(&mut c, tod);
    assert!((l, h) > (4, 0xD3) && (l, h) <= (5, 0x0B), "TOD del CIA-A en ({l}, ${h:02X})");
    // FS-UAE: el TOD del CIA-B cambia entre el ciclo $DE de una línea y el $0C de la siguiente.
    for k in 0..8 {
        let desde = 3 * CUADRO + (10 + 7 * k) * LINEA + 0x30 * 2;
        let t = primer_cambio(&mut c, desde, |c, t| c.leer(b(reg::TODLO), 1, t));
        let (_, h) = posicion(&mut c, t);
        assert!(h <= 0x0C || h > 0xDE, "TOD del CIA-B en el ciclo ${h:02X}");
    }
}

// --- El TOD: parada y alarma ---

#[test]
fn el_tod_esta_parado_tras_el_reinicio() {
    let mut c = Chips::nuevo();
    assert_eq!(leer_a(&mut c, reg::TODLO, 10 * CUADRO), 0);
    escribir_a(&mut c, reg::TODLO, 0, 10 * CUADRO);
    assert_eq!(leer_a(&mut c, reg::TODLO, 12 * CUADRO), 2);
}

#[test]
fn escribir_el_byte_del_medio_no_lo_para() {
    let mut c = Chips::tras_el_arranque();
    escribir_b(&mut c, reg::TODMID, 0x50, 0);
    assert_eq!(leer_b(&mut c, reg::TODMID, 4 * LINEA), 0x50);
    assert_eq!(leer_b(&mut c, reg::TODLO, 4 * LINEA), 4);
}

/// Pone la alarma del CIA-B a `v`.
fn alarma_b(c: &mut Chips, v: u32, t: u64) {
    escribir_b(c, reg::CRB, cr::ALARM, t);
    escribir_b(c, reg::TODHI, (v >> 16) as u8, t);
    escribir_b(c, reg::TODMID, (v >> 8) as u8, t);
    escribir_b(c, reg::TODLO, v as u8, t);
    escribir_b(c, reg::CRB, 0, t);
}

fn tod_b_poner(c: &mut Chips, v: u32, t: u64) {
    escribir_b(c, reg::TODHI, (v >> 16) as u8, t);
    escribir_b(c, reg::TODMID, (v >> 8) as u8, t);
    escribir_b(c, reg::TODLO, v as u8, t);
}

#[test]
fn la_alarma_salta_por_flanco_tambien_al_escribir() {
    // Lo mismo que hace cia.c ("alarma"), en el CIA-B.
    let mut c = Chips::tras_el_arranque();
    let t = 10 * LINEA;
    alarma_b(&mut c, 0xFF_FFFF, t);
    tod_b_poner(&mut c, 0x00_4000, t);
    escribir_b(&mut c, reg::TODHI, 0, t);
    assert_eq!(leer_b(&mut c, reg::ICR, t), 0);
    // La alarma igual al TOD (parado): no salta (FS-UAE, A500: $00).
    alarma_b(&mut c, 0x00_4000, t);
    assert_eq!(leer_b(&mut c, reg::ICR, t), 0);
    // La misma otra vez: tampoco.
    escribir_b(&mut c, reg::CRB, cr::ALARM, t);
    escribir_b(&mut c, reg::TODLO, 0x00, t);
    escribir_b(&mut c, reg::CRB, 0, t);
    assert_eq!(leer_b(&mut c, reg::ICR, t), 0);
    alarma_b(&mut c, 0x00_4080, t);
    assert_eq!(leer_b(&mut c, reg::ICR, t), 0);
    // El TOD igual a la alarma: salta (FS-UAE: $84, con la máscara que tenía el sistema).
    tod_b_poner(&mut c, 0x00_4080, t);
    assert_eq!(leer_b(&mut c, reg::ICR, t), icr::ALRM);
    // Contando: salta al llegar.
    tod_b_poner(&mut c, 0x00_407E, t);
    leer_b(&mut c, reg::ICR, t);
    assert_eq!(leer_b(&mut c, reg::ICR, 14 * LINEA), icr::ALRM);
}

// --- Los temporizadores al arrancar y al parar ---

/// Lo que hace `seis_lecturas` de cia.c: escribe `v` en el registro `r` del CIA-B a la hora `t` y lee TALO cada 2
/// pulsos del reloj E, la primera 2 pulsos después.
fn seis_lecturas(c: &mut Chips, r: u8, v: u8, t: u64) -> [u8; 6] {
    escribir_b(c, r, v, t);
    core::array::from_fn(|i| leer_b(c, reg::TALO, t + (2 + 2 * i as u64) * E))
}

#[test]
fn el_retraso_al_arrancar() {
    let preparar = |c: &mut Chips, cra: u8, t: u64| {
        escribir_b(c, reg::CRA, 0, t);
        escribir_b(c, reg::TALO, 0x80, t);
        escribir_b(c, reg::TAHI, 0x10, t);
        escribir_b(c, reg::CRA, cra, t);
    };
    let mut c = Chips::nuevo();
    preparar(&mut c, 0, 0);
    assert_eq!(seis_lecturas(&mut c, reg::CRA, cr::START, 0), [0x80, 0x7E, 0x7C, 0x7A, 0x78, 0x76]);
    preparar(&mut c, 0, 1000);
    assert_eq!(seis_lecturas(&mut c, reg::CRA, cr::LOAD | cr::START, 1000), [0x80, 0x7F, 0x7D, 0x7B, 0x79, 0x77]);
    preparar(&mut c, cr::RUNMODE, 2000);
    assert_eq!(seis_lecturas(&mut c, reg::TAHI, 0x10, 2000), [0x80, 0x7F, 0x7D, 0x7B, 0x79, 0x77]);
    // En marcha, LOAD carga al momento y sigue bajando (FS-UAE: 7E 7C 7A las ocho veces).
    preparar(&mut c, cr::START, 3000);
    assert_eq!(seis_lecturas(&mut c, reg::CRA, cr::LOAD | cr::START, 3500), [0x7E, 0x7C, 0x7A, 0x78, 0x76, 0x74]);
    // Parado en marcha, ya no baja.
    let v = seis_lecturas(&mut c, reg::CRA, 0, 4000);
    assert!(v.iter().all(|&x| x == v[0]), "{v:02X?}");
}

// --- El registro serie ---

#[test]
fn serie_en_salida_con_el_temporizador_a() {
    // CIA-B: TA continuo con latch 7 (desborda cada 8 pulsos; el primero, 2 + 7 pulsos tras START). El byte escrito en
    // SDR sale desde el primer desborde, y SP sube en el 16.º contando ese. En FS-UAE, cia.c ("serie salida") midió
    // 132 y 133 pulsos del reloj E entre la lectura de TB de antes de escribir SDR y la de después de ver SP: aquí, de
    // la escritura (4 pulsos tras START) a SP, 9 + 15 × 8 - 4 = 125, más las lecturas de cia.c.
    let mut c = Chips::nuevo();
    escribir_b(&mut c, reg::TALO, 7, 0);
    escribir_b(&mut c, reg::TAHI, 0, 0);
    escribir_b(&mut c, reg::CRA, cr::SPMODE | cr::START, 0);
    escribir_b(&mut c, reg::SDR, 0xA5, 4 * E);
    let sp = (9 + 15 * 8) * E;
    assert_eq!(leer_b(&mut c, reg::ICR, sp - E) & icr::SP, 0);
    assert_eq!(leer_b(&mut c, reg::ICR, sp) & icr::SP, icr::SP);
    assert_eq!(leer_b(&mut c, reg::SDR, sp), 0xA5);
    // Sin otro byte, no vuelve a subir.
    assert_eq!(leer_b(&mut c, reg::ICR, sp + 1000 * E) & icr::SP, 0);
    // Dos seguidos: el segundo, escrito mientras sale el primero, empieza en el desborde siguiente al fin.
    escribir_b(&mut c, reg::SDR, 0x11, sp + 1000 * E);
    escribir_b(&mut c, reg::SDR, 0x22, sp + 1001 * E);
    let mut subidas = 0;
    let mut t = sp + 1001 * E;
    while t < sp + 1400 * E {
        t = c.siguiente_suceso();
        if leer_b(&mut c, reg::ICR, t) & icr::SP != 0 {
            subidas += 1;
        }
    }
    assert_eq!(subidas, 1, "el segundo byte pisó al primero en SDR antes de salir: solo sale uno");
}

#[test]
fn serie_en_entrada_y_cambio_de_sentido() {
    let mut c = Chips::nuevo();
    // El teclado, al encender, no manda nada hasta pasado un segundo.
    let mut cia = caja_chips::cia::Cia::nuevo();
    for i in (0..8).rev() {
        cia.entrada_serie((0xC5u8 >> i) & 1 != 0);
    }
    assert_eq!(cia.leer(reg::ICR), icr::SP);
    assert_eq!(cia.leer(reg::SDR), 0xC5);
    // A medias, pasar a salida y volver lo vacía.
    for _ in 0..3 {
        cia.entrada_serie(true);
    }
    cia.escribir(reg::CRA, cr::SPMODE);
    cia.entrada_serie(true);
    cia.escribir(reg::CRA, 0);
    for _ in 0..7 {
        cia.entrada_serie(false);
    }
    assert_eq!(cia.leer(reg::ICR), 0);
    cia.entrada_serie(false);
    assert_eq!(cia.leer(reg::ICR), icr::SP);
    assert_eq!(cia.leer(reg::SDR), 0x00);
    assert_eq!(leer_a(&mut c, reg::ICR, us(900_000)), 0);
}

// --- El teclado ---

/// Hace de programa: espera (de suceso en suceso, como mucho `plazo` ciclos desde ahora) a que suba SP en el ICR del CIA-A, lee
/// SDR y, si `saludo`, da el saludo de `largo` µs. Devuelve el código y la hora a la que lo vio.
fn recibir(c: &mut Chips, plazo: u64, saludo: bool, largo: u64) -> Option<(u8, u64)> {
    let hasta = c.hora() + plazo;
    loop {
        let t = c.siguiente_suceso();
        if t > hasta {
            return None;
        }
        if leer_a(c, reg::ICR, t) & icr::SP != 0 {
            let sdr = leer_a(c, reg::SDR, t);
            if saludo {
                escribir_a(c, reg::CRA, cr::SPMODE, t);
                escribir_a(c, reg::CRA, 0, t + us(largo));
            }
            return Some((de_la_linea(sdr), t));
        }
    }
}

#[test]
fn una_tecla_con_saludo() {
    let mut c = Chips::tras_el_arranque();
    c.avanzar(1000);
    c.tecla(0x20, true);
    let (codigo, t) = recibir(&mut c, us(10_000), true, 85).expect("llega");
    assert_eq!(codigo, 0x20);
    // El último flanco de KCLK, a 7 × 60 + 40 µs del principio.
    assert_eq!(t, 1000 + us(60) * 7 + us(40));
    assert_eq!(leer_a(&mut c, reg::SDR, t), en_la_linea(0x20));
    c.tecla(0x20, false);
    c.tecla(0x20, false);
    assert_eq!(recibir(&mut c, us(20_000), true, 85).map(|r| r.0), Some(0xA0));
    assert_eq!(recibir(&mut c, us(400_000), true, 85), None, "la subida repetida no se manda");
    // Varias seguidas: cada una tras el saludo de la anterior.
    for k in 0..5 {
        c.tecla(0x10 + k, true);
    }
    for k in 0..5 {
        assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(0x10 + k));
    }
}

#[test]
fn sin_saludo_resincroniza_y_repite() {
    let mut c = Chips::tras_el_arranque();
    c.tecla(0x45, true);
    let (codigo, t) = recibir(&mut c, us(10_000), false, 0).expect("llega");
    assert_eq!(codigo, 0x45);
    // Sin saludo: a los 143 ms manda un 1, y otro cada 143 ms. El CIA, que ya había acabado su byte, necesita 8 para
    // otro: un byte de unos ($00 en SDR, $FF al darle la vuelta). Ese sí lleva saludo.
    let (basura, t2) = recibir(&mut c, us(2_000_000), true, 85).expect("llega la basura");
    assert_eq!(basura, 0xFF);
    assert!(t2 > t + 8 * us(143_000) && t2 < t + 9 * us(143_100), "8 unos, uno cada 143 ms");
    // Con el saludo: $F9 y el código perdido otra vez.
    assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(teclado::PERDIDO));
    assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(0x45));
}

#[test]
fn un_saludo_corto_no_vale() {
    let mut c = Chips::tras_el_arranque();
    c.tecla(0x01, true);
    c.tecla(0x02, true);
    recibir(&mut c, us(10_000), true, 50).expect("llega");
    // Con 50 µs el teclado no lo ve: el siguiente no llega antes de los 143 ms.
    assert_eq!(recibir(&mut c, us(140_000), true, 85), None);
    // Con el mínimo bajado a 1 µs (como vAmiga), sí.
    let mut c = Chips::tras_el_arranque();
    c.teclado_saludo_minimo(us(1));
    c.tecla(0x01, true);
    c.tecla(0x02, true);
    recibir(&mut c, us(10_000), true, 50).expect("llega");
    assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(0x02));
}

#[test]
fn al_encender_sincroniza_y_manda_fd_y_fe() {
    let mut c = Chips::nuevo();
    // Un segundo de prueba, y después un 1 cada 143 ms: 8 unos son un byte para el CIA.
    let (basura, t) = recibir(&mut c, us(3_000_000), true, 85).expect("llega");
    assert_eq!(basura, 0xFF);
    assert!(t > us(1_000_000) + 7 * us(143_000) && t < us(1_000_000) + 8 * us(143_100));
    assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(teclado::INICIO_ARRANQUE));
    assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(teclado::FIN_ARRANQUE));
    // Con un saludo durante la prueba, sin esperar.
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::CRA, cr::SPMODE, us(100_000));
    escribir_a(&mut c, reg::CRA, 0, us(100_100));
    assert_eq!(recibir(&mut c, us(10_000), true, 85).map(|r| r.0), Some(teclado::INICIO_ARRANQUE));
}

#[test]
fn la_pila_llena_manda_fa() {
    let mut c = Chips::tras_el_arranque();
    // La primera sale enseguida; caben 10 más; la 12.ª se pierde.
    for k in 0..12 {
        c.tecla(0x20 + k, true);
    }
    let mut codigos = [0u8; 12];
    for cod in codigos.iter_mut() {
        *cod = recibir(&mut c, us(10_000), true, 85).expect("llega").0;
    }
    let mut esperados = [0u8; 12];
    for (k, e) in esperados.iter_mut().enumerate().take(11) {
        *e = 0x20 + k as u8;
    }
    esperados[11] = teclado::PILA_LLENA;
    assert_eq!(codigos, esperados);
    assert_eq!(recibir(&mut c, us(400_000), true, 85), None);
}

#[test]
fn el_teclado_no_pide_sucesos_si_esta_quieto() {
    let c = Chips::tras_el_arranque();
    assert_eq!(c.siguiente_suceso(), CUADRO);
}

// --- Los puertos ---

#[test]
fn botones_de_fuego_y_lineas_del_disco() {
    let mut c = Chips::tras_el_arranque();
    c.fuego(0, true);
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0xBC);
    c.fuego(1, true);
    c.fuego(0, false);
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0x7C);
    c.fuego(1, false);
    // /CHNG, /WPRO, /TK0 y /RDY, activas a 0.
    c.disco_senales(true, false, true, false);
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0xE8);
    c.disco_senales(false, true, false, true);
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0xD4);
    // Las de mando, en el puerto B del CIA-B: motor, unidad 0, cara de arriba, hacia dentro, paso.
    let l = c.disco_lineas();
    assert!(!l.motor() && !l.elegida(0) && !l.paso());
    escribir_b(&mut c, reg::PRB, 0x7F & !0x08 & !0x04 & !0x02 & !0x01, 0);
    let l = c.disco_lineas();
    assert!(l.motor() && l.elegida(0) && !l.elegida(1) && l.cara_arriba() && l.hacia_dentro() && l.paso());
    // Sin DDRB, los pines se leen a 1 aunque PRB diga otra cosa.
    escribir_b(&mut c, reg::DDRB, 0x00, 0);
    assert_eq!(c.disco_lineas().0, 0xFF);
    // OVL y el LED, por PRA del CIA-A.
    escribir_a(&mut c, reg::PRA, 0x03, 0);
    assert!(c.ovl() && !c.led());
}
