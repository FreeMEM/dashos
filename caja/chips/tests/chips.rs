//! Pruebas de los chips por el bus, como los ve el 68k.

use caja_chips::cia::{cr, icr, reg};
use caja_chips::{CICLOS_POR_LINEA, Chips, int};

const LINEA: u64 = CICLOS_POR_LINEA;
const CUADRO_LARGO: u64 = 313 * LINEA;
const CUADRO_CORTO: u64 = 312 * LINEA;

const DMACONR: u32 = 0xDFF002;
const VPOSR: u32 = 0xDFF004;
const VHPOSR: u32 = 0xDFF006;
const INTENAR: u32 = 0xDFF01C;
const INTREQR: u32 = 0xDFF01E;
const VPOSW: u32 = 0xDFF02A;
const VHPOSW: u32 = 0xDFF02C;
const DMACON: u32 = 0xDFF096;
const INTENA: u32 = 0xDFF09A;
const INTREQ: u32 = 0xDFF09C;
const BPLCON0: u32 = 0xDFF100;

fn a(r: u8) -> u32 {
    0xBFE001 + u32::from(r) * 0x100
}

fn b(r: u8) -> u32 {
    0xBFD000 + u32::from(r) * 0x100
}

fn leer_a(c: &mut Chips, r: u8, hora: u64) -> u8 {
    c.leer(a(r), 1, hora) as u8
}

fn escribir_a(c: &mut Chips, r: u8, v: u8, hora: u64) {
    c.escribir(a(r), u32::from(v), 1, hora);
}

fn leer16(c: &mut Chips, dir: u32, hora: u64) -> u16 {
    c.leer(dir, 2, hora) as u16
}

fn escribir16(c: &mut Chips, dir: u32, v: u16, hora: u64) {
    c.escribir(dir, u32::from(v), 2, hora);
}

/// Pone en marcha el temporizador A del CIA-A con `latch`, en el modo `modo` (`cr::RUNMODE` o 0).
fn arrancar_ta(c: &mut Chips, latch: u16, modo: u8, hora: u64) {
    escribir_a(c, reg::CRA, modo, hora);
    escribir_a(c, reg::TALO, latch as u8, hora);
    escribir_a(c, reg::TAHI, (latch >> 8) as u8, hora);
    escribir_a(c, reg::CRA, modo | cr::START, hora);
}

fn tod_a(c: &mut Chips, hora: u64) -> u32 {
    let hi = u32::from(leer_a(c, reg::TODHI, hora));
    let mid = u32::from(leer_a(c, reg::TODMID, hora));
    let lo = u32::from(leer_a(c, reg::TODLO, hora));
    (hi << 16) | (mid << 8) | lo
}

// --- El haz ---

#[test]
fn constantes_pal() {
    assert_eq!(LINEA, 454);
    assert_eq!(CUADRO_LARGO, 142_102);
    assert_eq!(caja_chips::RELOJ_CPU_PAL / CUADRO_LARGO, 49);
}

#[test]
fn vposr_avanza_con_la_hora() {
    let mut c = Chips::nuevo();
    assert_eq!(leer16(&mut c, VPOSR, 0), 0x8000, "cuadro largo, Agnus OCS PAL, línea 0");
    assert_eq!(leer16(&mut c, VHPOSR, 0), 0x0000);
    assert_eq!(leer16(&mut c, VHPOSR, 100 * LINEA + 20), 0x640A);
    assert_eq!(leer16(&mut c, VHPOSR, 101 * LINEA - 1), 0x64E2, "último ciclo de color de la línea 100");
    assert_eq!(leer16(&mut c, VPOSR, 300 * LINEA), 0x8001, "V8");
    assert_eq!(leer16(&mut c, VHPOSR, 300 * LINEA), 0x2C00);
    assert_eq!(leer16(&mut c, VHPOSR, 312 * LINEA), 0x3800);
    assert_eq!(leer16(&mut c, INTREQR, 312 * LINEA) & int::VERTB, 0);
}

#[test]
fn el_cuadro_da_la_vuelta_y_sube_vertb() {
    let mut c = Chips::nuevo();
    assert_eq!(c.siguiente_suceso(), CUADRO_LARGO);
    assert_eq!(leer16(&mut c, INTREQR, CUADRO_LARGO - 1), 0);
    assert_eq!(leer16(&mut c, INTREQR, CUADRO_LARGO), int::VERTB);
    assert_eq!(leer16(&mut c, VPOSR, CUADRO_LARGO), 0x8000);
    assert_eq!(leer16(&mut c, VHPOSR, CUADRO_LARGO + 5 * LINEA), 0x0500);
    assert_eq!(c.siguiente_suceso(), 2 * CUADRO_LARGO);
    // Muchos cuadros de golpe.
    assert_eq!(leer16(&mut c, VHPOSR, 1000 * CUADRO_LARGO + 7 * LINEA + 4), 0x0702);
}

#[test]
fn cuadro_corto_con_lof_a_cero() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, VPOSW, 0x0000, 0);
    assert_eq!(leer16(&mut c, VPOSR, 0), 0x0000);
    assert_eq!(c.siguiente_suceso(), CUADRO_CORTO);
    assert_eq!(leer16(&mut c, VHPOSR, CUADRO_CORTO - 1) >> 8, 311 & 0xFF);
    assert_eq!(leer16(&mut c, VHPOSR, CUADRO_CORTO), 0x0000);
    assert_eq!(leer16(&mut c, VPOSR, CUADRO_CORTO), 0x0000, "sin entrelazado, LOF no cambia");
}

#[test]
fn con_entrelazado_lof_cambia_en_cada_cuadro() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, BPLCON0, 0x0004, 0);
    assert_eq!(leer16(&mut c, VPOSR, CUADRO_LARGO) >> 15, 0);
    assert_eq!(c.siguiente_suceso(), CUADRO_LARGO + CUADRO_CORTO);
    assert_eq!(leer16(&mut c, VPOSR, CUADRO_LARGO + CUADRO_CORTO) >> 15, 1);
    // Diez pares más, de golpe: LOF vuelve a 1.
    let t = 11 * (CUADRO_LARGO + CUADRO_CORTO);
    assert_eq!(leer16(&mut c, VPOSR, t), 0x8000);
    assert_eq!(leer16(&mut c, VHPOSR, t), 0x0000);
}

#[test]
fn escribir_la_posicion_mueve_el_haz() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, VHPOSW, 0x2010, 1000);
    assert_eq!(leer16(&mut c, VHPOSR, 1000), 0x2010);
    assert_eq!(leer16(&mut c, VHPOSR, 1000 + LINEA), 0x2110);
    escribir16(&mut c, VPOSW, 0x8001, 2000);
    let (v, h) = {
        let w = leer16(&mut c, VHPOSR, 2000);
        (w >> 8, w & 0xFF)
    };
    assert_eq!(leer16(&mut c, VPOSR, 2000), 0x8001);
    // Mil ciclos después: dos líneas y 46 ciclos de color más.
    assert_eq!((v, h), (0x22, 0x10 + 46));
    // Desde la línea $122 = 290 quedan 23 líneas hasta el final del cuadro largo.
    let fin = 2000 + (313 - 290) * LINEA - u64::from(h) * 2;
    assert_eq!(c.siguiente_suceso(), fin);
    assert_eq!(leer16(&mut c, INTREQR, fin) & int::VERTB, int::VERTB);
}

#[test]
fn escribir_la_posicion_al_principio_no_se_desborda() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, VHPOSW, 0x6400, 10);
    assert_eq!(leer16(&mut c, VHPOSR, 10), 0x6400);
    assert_eq!(c.siguiente_suceso(), 10 + (313 - 100) * LINEA);
}

// --- Interrupciones ---

#[test]
fn intena_e_intreq_con_set_clr() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0xC028, 0);
    assert_eq!(leer16(&mut c, INTENAR, 0), 0x4028);
    escribir16(&mut c, INTENA, 0x0008, 0);
    assert_eq!(leer16(&mut c, INTENAR, 0), 0x4020);
    escribir16(&mut c, INTREQ, 0x8004, 0);
    escribir16(&mut c, INTREQ, 0x8020, 0);
    assert_eq!(leer16(&mut c, INTREQR, 0), 0x0024);
    escribir16(&mut c, INTREQ, 0x0004, 0);
    assert_eq!(leer16(&mut c, INTREQR, 0), 0x0020);
    escribir16(&mut c, INTREQ, 0x7FFF, 0);
    assert_eq!(leer16(&mut c, INTREQR, 0), 0);
}

#[test]
fn ipl_sigue_el_mapa_del_manual() {
    let casos: [(u16, u8); 14] = [
        (int::TBE, 1),
        (int::DSKBLK, 1),
        (int::SOFT, 1),
        (int::PORTS, 2),
        (int::COPER, 3),
        (int::VERTB, 3),
        (int::BLIT, 3),
        (int::AUD0, 4),
        (int::AUD3, 4),
        (int::RBF, 5),
        (int::DSKSYN, 5),
        (int::EXTER, 6),
        (int::EXTER | int::TBE, 6),
        (int::PORTS | int::VERTB, 3),
    ];
    for (bits, nivel) in casos {
        let mut c = Chips::nuevo();
        escribir16(&mut c, INTREQ, 0x8000 | bits, 0);
        assert_eq!(c.ipl(), 0, "sin INTENA");
        escribir16(&mut c, INTENA, 0x8000 | bits, 0);
        assert_eq!(c.ipl(), 0, "sin INTEN");
        escribir16(&mut c, INTENA, 0x8000 | int::INTEN, 0);
        assert_eq!(c.ipl(), nivel, "bits {bits:04x}");
    }
}

#[test]
fn vertb_pide_nivel_3() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::VERTB, 0);
    c.avanzar(CUADRO_LARGO - 1);
    assert_eq!(c.ipl(), 0);
    c.avanzar(CUADRO_LARGO);
    assert_eq!(c.ipl(), 3);
}

#[test]
fn dmacon_guardado() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, DMACON, 0x8000 | 0x7FFF, 0);
    assert_eq!(leer16(&mut c, DMACONR, 0), 0x07FF, "BBUSY y BZERO no se escriben");
    escribir16(&mut c, DMACON, 0x0200, 0);
    assert_eq!(leer16(&mut c, DMACONR, 0), 0x05FF);
}

// --- Tamaños y direcciones ---

#[test]
fn accesos_de_1_2_y_4_bytes() {
    let mut c = Chips::nuevo();
    let t = 100 * LINEA + 20;
    assert_eq!(c.leer(VHPOSR, 1, t), 0x64);
    assert_eq!(c.leer(VHPOSR + 1, 1, t), 0x0A);
    assert_eq!(c.leer(VPOSR, 4, t), 0x8000_640A);
    // Un byte a un registro de los chips va con el byte repetido: $C0 en INTENA es $C0C0.
    c.escribir(INTENA, 0xC0, 1, t);
    assert_eq!(leer16(&mut c, INTENAR, t), 0x40C0);
    // Uno de 4 son dos de 2: INTENA y luego INTREQ.
    c.escribir(INTENA, 0x7FFF_8024, 4, t);
    assert_eq!(leer16(&mut c, INTENAR, t), 0);
    assert_eq!(leer16(&mut c, INTREQR, t), 0x0024);
    assert_eq!(c.accesos_a_nadie, 0);
}

#[test]
fn accesos_de_2_bytes_a_los_cia() {
    let mut c = Chips::nuevo();
    // El byte alto de $BFE000 no es de nadie; el bajo es PRA del CIA-A.
    c.escribir(a(reg::DDRA) - 1, 0x1234, 2, 0);
    assert_eq!(leer_a(&mut c, reg::DDRA, 0), 0x34);
    assert_eq!(c.accesos_a_nadie, 1);
    assert_eq!(c.ultimo_a_nadie, 0xBFE200);
    // En el CIA-B es al revés: el alto es el registro y el bajo es de nadie.
    c.escribir(b(reg::DDRA), 0x5678, 2, 0);
    assert_eq!(c.leer(b(reg::DDRA), 1, 0), 0x56);
    assert_eq!(c.leer(b(reg::DDRA), 2, 0), 0x5600);
    assert_eq!(c.ultimo_a_nadie, 0xBFD201);
    // Uno de 4 a $BFE000: nadie, PRA (DDRA = $34, entradas a 1), nadie, nadie.
    assert_eq!(c.leer(0xBFE000, 4, 0), 0x00CB_0000);
    assert_eq!(c.ultimo_a_nadie, 0xBFE003);
}

#[test]
fn direcciones_de_nadie() {
    let mut c = Chips::nuevo();
    assert_eq!(c.leer(0xDFF180, 2, 0), 0, "COLOR00 es de solo escritura: se guarda, pero no se lee");
    assert_eq!(c.accesos_a_nadie, 1);
    c.escribir(0xDFF020, 0x0001, 2, 0);
    assert_eq!(c.accesos_a_nadie, 2, "DSKPTH no se imita (aún no hay disco)");
    assert_eq!(c.leer(0xBFE003, 1, 0), 0, "espejo del CIA-A");
    assert_eq!(c.leer(0xBFC001, 1, 0), 0);
    // Fuera de los chips, uno de 2 bytes son dos de 1.
    assert_eq!(c.leer(0xDFF200, 2, 0), 0);
    assert_eq!(c.accesos_a_nadie, 6);
    assert_eq!(c.ultimo_a_nadie, 0xDFF201);
    assert_eq!(c.leer(VPOSR, 3, 0), 0, "tamaño que no existe");
    assert_eq!(c.accesos_a_nadie, 7);
    // VPOSR no se escribe.
    escribir16(&mut c, VPOSR, 0, 0);
    assert_eq!(c.accesos_a_nadie, 8);
}

#[test]
fn la_hora_no_va_hacia_atras() {
    let mut c = Chips::nuevo();
    assert_eq!(leer16(&mut c, VHPOSR, 10 * LINEA), 0x0A00);
    assert_eq!(leer16(&mut c, VHPOSR, 5 * LINEA), 0x0A00);
    assert_eq!(c.hora(), 10 * LINEA);
}

// --- CIA: puertos y registros sencillos ---

#[test]
fn puertos_y_direcciones() {
    let mut c = Chips::nuevo();
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0xFF, "sin nada conectado, entradas a 1");
    escribir_a(&mut c, reg::DDRA, 0x03, 0);
    escribir_a(&mut c, reg::PRA, 0x00, 0);
    assert_eq!(leer_a(&mut c, reg::PRA, 0), 0xFC);
    assert_eq!(leer_a(&mut c, reg::DDRA, 0), 0x03);
    c.escribir(b(reg::DDRB), 0xFF, 1, 0);
    c.escribir(b(reg::PRB), 0x5A, 1, 0);
    assert_eq!(c.leer(b(reg::PRB), 1, 0), 0x5A);
    assert_eq!(c.leer(b(reg::DDRB), 1, 0), 0xFF);
    assert_eq!(leer_a(&mut c, reg::PRB, 0), 0xFF, "los dos CIA son distintos");
}

#[test]
fn sdr_se_guarda() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::SDR, 0x42, 0);
    assert_eq!(leer_a(&mut c, reg::SDR, 0), 0x42);
}

// --- CIA: temporizadores ---

#[test]
fn temporizador_una_vez() {
    let mut c = Chips::nuevo();
    arrancar_ta(&mut c, 100, cr::RUNMODE, 0);
    assert_eq!(c.siguiente_suceso(), 101 * 10);
    // A mitad: el contador ha bajado.
    let t = 50 * 10;
    let lo = leer_a(&mut c, reg::TALO, t);
    let hi = leer_a(&mut c, reg::TAHI, t);
    assert_eq!(u16::from_be_bytes([hi, lo]), 50);
    assert_eq!(leer_a(&mut c, reg::ICR, 1009), 0);
    assert_eq!(leer_a(&mut c, reg::ICR, 1010), icr::TA, "sin máscara, IR a 0");
    assert_eq!(leer_a(&mut c, reg::ICR, 1010), 0, "leer ICR lo borra");
    assert_eq!(leer_a(&mut c, reg::CRA, 1010) & cr::START, 0, "se para al desbordar");
    assert_eq!(leer_a(&mut c, reg::TALO, 5000), 100, "vuelve a cargar el latch");
    assert_eq!(leer_a(&mut c, reg::ICR, 100_000), 0, "no vuelve a desbordar");
    assert_eq!(c.siguiente_suceso(), CUADRO_LARGO);
}

#[test]
fn una_vez_arranca_al_escribir_el_byte_alto() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::CRA, cr::RUNMODE, 0);
    escribir_a(&mut c, reg::TALO, 9, 0);
    escribir_a(&mut c, reg::TAHI, 0, 0);
    assert_eq!(leer_a(&mut c, reg::CRA, 0) & cr::START, cr::START);
    assert_eq!(leer_a(&mut c, reg::ICR, 100), icr::TA);
}

#[test]
fn temporizador_continuo() {
    let mut c = Chips::nuevo();
    arrancar_ta(&mut c, 9, 0, 0);
    assert_eq!(c.siguiente_suceso(), 100);
    assert_eq!(leer_a(&mut c, reg::TALO, 90), 0);
    assert_eq!(leer_a(&mut c, reg::ICR, 99), 0);
    assert_eq!(leer_a(&mut c, reg::ICR, 100), icr::TA);
    assert_eq!(leer_a(&mut c, reg::TALO, 100), 9);
    assert_eq!(c.siguiente_suceso(), 200);
    assert_eq!(leer_a(&mut c, reg::ICR, 199), 0);
    assert_eq!(leer_a(&mut c, reg::ICR, 250), icr::TA);
    assert_eq!(leer_a(&mut c, reg::TALO, 250), 4);
    assert_eq!(leer_a(&mut c, reg::CRA, 250), cr::START, "sigue en marcha");
}

#[test]
fn escribir_el_byte_alto_en_marcha_solo_cambia_el_latch() {
    let mut c = Chips::nuevo();
    arrancar_ta(&mut c, 9, 0, 0);
    escribir_a(&mut c, reg::TAHI, 1, 30);
    assert_eq!(leer_a(&mut c, reg::TALO, 30), 6);
    assert_eq!(leer_a(&mut c, reg::TAHI, 30), 0);
    // Tras el desborde carga el latch nuevo, $0109.
    assert_eq!(leer_a(&mut c, reg::TAHI, 100), 1);
    assert_eq!(leer_a(&mut c, reg::TALO, 100), 9);
}

#[test]
fn escribir_el_byte_alto_parado_carga_el_contador() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::TALO, 0x34, 0);
    escribir_a(&mut c, reg::TAHI, 0x12, 0);
    assert_eq!(leer_a(&mut c, reg::TAHI, 0), 0x12);
    assert_eq!(leer_a(&mut c, reg::TALO, 0), 0x34);
    assert_eq!(leer_a(&mut c, reg::CRA, 0), 0, "en continuo no arranca");
    assert_eq!(leer_a(&mut c, reg::TALO, 1000), 0x34, "parado no cuenta");
}

#[test]
fn carga_forzada() {
    let mut c = Chips::nuevo();
    arrancar_ta(&mut c, 99, 0, 0);
    assert_eq!(leer_a(&mut c, reg::TALO, 500), 49);
    escribir_a(&mut c, reg::CRA, cr::START | cr::LOAD, 500);
    assert_eq!(leer_a(&mut c, reg::TALO, 500), 99);
    assert_eq!(leer_a(&mut c, reg::CRA, 500), cr::START, "LOAD no se guarda");
    assert_eq!(c.siguiente_suceso(), 500 + 100 * 10);
}

#[test]
fn b_cuenta_los_desbordes_de_a() {
    let mut c = Chips::nuevo();
    // B: latch 2, cuenta desbordes del A (CRB bits 5-6 = 10).
    escribir_a(&mut c, reg::TBLO, 2, 0);
    escribir_a(&mut c, reg::TBHI, 0, 0);
    escribir_a(&mut c, reg::CRB, 0x40 | cr::START, 0);
    arrancar_ta(&mut c, 9, 0, 0);
    assert_eq!(c.siguiente_suceso(), 100);
    assert_eq!(leer_a(&mut c, reg::TBLO, 100), 1);
    assert_eq!(leer_a(&mut c, reg::ICR, 299), icr::TA);
    assert_eq!(leer_a(&mut c, reg::TBLO, 299), 0);
    assert_eq!(leer_a(&mut c, reg::ICR, 300), icr::TA | icr::TB);
    assert_eq!(leer_a(&mut c, reg::TBLO, 300), 2);
}

#[test]
fn siguiente_suceso_de_b_contando_a() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::ICR, 0x7F, 0);
    escribir_a(&mut c, reg::TBLO, 4, 0);
    escribir_a(&mut c, reg::TBHI, 0, 0);
    escribir_a(&mut c, reg::CRB, 0x40 | cr::START, 0);
    // A, latch 999: desborda cada 1000 pulsos (10.000 ciclos); B a los 5 desbordes.
    arrancar_ta(&mut c, 999, 0, 0);
    // A se para: B no llega nunca. Solo queda el fin de cuadro.
    escribir_a(&mut c, reg::CRA, 0, 0);
    assert_eq!(c.siguiente_suceso(), CUADRO_LARGO);
    // B solo, contando el reloj E.
    escribir_a(&mut c, reg::CRB, cr::START, 0);
    assert_eq!(c.siguiente_suceso(), 50);
}

#[test]
fn b_con_el_reloj_e() {
    let mut c = Chips::nuevo();
    c.escribir(b(reg::TBLO), 19, 1, 0);
    c.escribir(b(reg::TBHI), 0, 1, 0);
    c.escribir(b(reg::CRB), u32::from(cr::START), 1, 0);
    assert_eq!(c.siguiente_suceso(), 200);
    assert_eq!(c.leer(b(reg::ICR), 1, 200) as u8, icr::TB);
}

// --- CIA: ICR, máscara e INTREQ ---

#[test]
fn mascara_del_icr() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::ICR, icr::IR_SC | icr::TA | icr::TB, 0);
    escribir_a(&mut c, reg::ICR, icr::TB, 0);
    arrancar_ta(&mut c, 9, 0, 0);
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::PORTS, 0);
    c.avanzar(99);
    assert_eq!(c.ipl(), 0);
    c.avanzar(100);
    assert_eq!(c.ipl(), 2);
    assert_eq!(leer16(&mut c, INTREQR, 100), int::PORTS);
    assert_eq!(leer_a(&mut c, reg::ICR, 100), icr::IR_SC | icr::TA);
    escribir16(&mut c, INTREQ, int::PORTS, 100);
    assert_eq!(c.ipl(), 0);
}

#[test]
fn sin_mascara_no_sube_a_intreq() {
    let mut c = Chips::nuevo();
    arrancar_ta(&mut c, 9, 0, 0);
    assert_eq!(leer16(&mut c, INTREQR, 150), 0);
    // Poner la máscara con el bit ya puesto sube la línea.
    escribir_a(&mut c, reg::ICR, icr::IR_SC | icr::TA, 150);
    assert_eq!(leer16(&mut c, INTREQR, 150), int::PORTS);
}

#[test]
fn intreq_vuelve_a_subir_mientras_el_cia_la_pide() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::ICR, icr::IR_SC | icr::TA, 0);
    arrancar_ta(&mut c, 9, cr::RUNMODE, 0);
    assert_eq!(leer16(&mut c, INTREQR, 100), int::PORTS);
    escribir16(&mut c, INTREQ, int::PORTS, 100);
    assert_eq!(leer16(&mut c, INTREQR, 100), int::PORTS);
    leer_a(&mut c, reg::ICR, 100);
    escribir16(&mut c, INTREQ, int::PORTS, 100);
    assert_eq!(leer16(&mut c, INTREQR, 100), 0);
}

#[test]
fn cia_b_sube_exter_nivel_6() {
    let mut c = Chips::nuevo();
    c.escribir(b(reg::ICR), u32::from(icr::IR_SC | icr::TA), 1, 0);
    c.escribir(b(reg::CRA), u32::from(cr::RUNMODE), 1, 0);
    c.escribir(b(reg::TALO), 0, 1, 0);
    c.escribir(b(reg::TAHI), 0, 1, 0);
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::EXTER | int::PORTS, 0);
    assert_eq!(c.siguiente_suceso(), 10);
    c.avanzar(10);
    assert_eq!(c.ipl(), 6);
    assert_eq!(leer16(&mut c, INTREQR, 10), int::EXTER);
}

// --- CIA: TOD ---

#[test]
fn tod_de_a_cuenta_cuadros() {
    let mut c = Chips::nuevo();
    assert_eq!(tod_a(&mut c, CUADRO_LARGO - 1), 0);
    assert_eq!(tod_a(&mut c, CUADRO_LARGO), 1);
    assert_eq!(tod_a(&mut c, 3 * CUADRO_LARGO + 100), 3);
    assert_eq!(tod_a(&mut c, 300 * CUADRO_LARGO), 300);
    // En cuadros cortos, cada 312 líneas.
    let mut c = Chips::nuevo();
    escribir16(&mut c, VPOSW, 0, 0);
    assert_eq!(tod_a(&mut c, CUADRO_CORTO - 1), 0);
    assert_eq!(tod_a(&mut c, CUADRO_CORTO), 1);
    assert_eq!(tod_a(&mut c, 50 * CUADRO_CORTO), 50);
}

#[test]
fn tod_de_b_cuenta_lineas() {
    let mut c = Chips::nuevo();
    let tod_b = |c: &mut Chips, t: u64| {
        let hi = c.leer(b(reg::TODHI), 1, t);
        let mid = c.leer(b(reg::TODMID), 1, t);
        let lo = c.leer(b(reg::TODLO), 1, t);
        (hi << 16) | (mid << 8) | lo
    };
    assert_eq!(tod_b(&mut c, LINEA - 1), 0);
    assert_eq!(tod_b(&mut c, LINEA), 1);
    assert_eq!(tod_b(&mut c, 2 * CUADRO_LARGO + 3 * LINEA), 2 * 313 + 3);
    assert_eq!(tod_b(&mut c, 1000 * LINEA), 1000);
}

#[test]
fn lectura_congelada_del_tod() {
    let mut c = Chips::nuevo();
    let t1 = 0x123 * CUADRO_LARGO;
    assert_eq!(leer_a(&mut c, reg::TODHI, t1), 0);
    let t2 = t1 + 5 * CUADRO_LARGO;
    assert_eq!(leer_a(&mut c, reg::TODMID, t2), 0x01, "congelado");
    assert_eq!(leer_a(&mut c, reg::TODMID, t2), 0x01, "sigue congelado");
    assert_eq!(leer_a(&mut c, reg::TODLO, t2), 0x23, "el bajo lo suelta");
    assert_eq!(leer_a(&mut c, reg::TODLO, t2), 0x28, "ya en vivo");
    assert_eq!(leer_a(&mut c, reg::TODMID, t2), 0x01);
}

#[test]
fn escribir_el_tod_lo_para_hasta_el_byte_bajo() {
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::TODHI, 0x01, 0);
    escribir_a(&mut c, reg::TODMID, 0x02, 0);
    assert_eq!(tod_a(&mut c, 10 * CUADRO_LARGO), 0x010200, "parado");
    escribir_a(&mut c, reg::TODLO, 0x03, 10 * CUADRO_LARGO);
    assert_eq!(tod_a(&mut c, 12 * CUADRO_LARGO), 0x010205);
}

#[test]
fn alarma_del_tod() {
    let mut c = Chips::nuevo();
    // Con CRB bit 7, se escribe la alarma; el TOD sigue contando.
    escribir_a(&mut c, reg::CRB, cr::ALARM, 0);
    escribir_a(&mut c, reg::TODHI, 0, 0);
    escribir_a(&mut c, reg::TODMID, 0, 0);
    escribir_a(&mut c, reg::TODLO, 5, 0);
    escribir_a(&mut c, reg::CRB, 0, 0);
    escribir_a(&mut c, reg::ICR, icr::IR_SC | icr::ALRM, 0);
    assert_eq!(tod_a(&mut c, 2 * CUADRO_LARGO), 2, "escribir la alarma no toca el TOD");
    assert_eq!(c.siguiente_suceso(), 3 * CUADRO_LARGO);
    c.avanzar(3 * CUADRO_LARGO);
    assert_eq!(c.siguiente_suceso(), 4 * CUADRO_LARGO);
    assert_eq!(leer_a(&mut c, reg::ICR, 5 * CUADRO_LARGO - 1), 0);
    assert_eq!(c.siguiente_suceso(), 5 * CUADRO_LARGO);
    assert_eq!(leer16(&mut c, INTREQR, 5 * CUADRO_LARGO), int::VERTB | int::PORTS);
    assert_eq!(leer_a(&mut c, reg::ICR, 5 * CUADRO_LARGO), icr::IR_SC | icr::ALRM);
    // Saltándola de golpe también salta.
    let mut c = Chips::nuevo();
    escribir_a(&mut c, reg::CRB, cr::ALARM, 0);
    escribir_a(&mut c, reg::TODLO, 7, 0);
    assert_eq!(leer_a(&mut c, reg::ICR, 100 * CUADRO_LARGO), icr::ALRM);
}

#[test]
fn alarma_del_tod_de_b_por_lineas() {
    let mut c = Chips::nuevo();
    c.escribir(b(reg::CRB), u32::from(cr::ALARM), 1, 0);
    c.escribir(b(reg::TODMID), 0x01, 1, 0);
    c.escribir(b(reg::TODLO), 0x00, 1, 0);
    c.escribir(b(reg::CRB), 0, 1, 0);
    let t = 0x100 * LINEA;
    c.avanzar(t - LINEA);
    assert_eq!(c.siguiente_suceso(), t);
    assert_eq!(c.leer(b(reg::ICR), 1, t - 1) as u8, 0);
    assert_eq!(c.leer(b(reg::ICR), 1, t) as u8, icr::ALRM);
}

#[test]
fn alarma_con_entrelazado() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, BPLCON0, 0x0004, 0);
    escribir_a(&mut c, reg::CRB, cr::ALARM, 0);
    escribir_a(&mut c, reg::TODLO, 4, 0);
    // Largo, corto, largo, corto.
    assert_eq!(c.siguiente_suceso(), CUADRO_LARGO);
    c.avanzar(CUADRO_LARGO);
    let fin = 2 * CUADRO_LARGO + 2 * CUADRO_CORTO;
    let mut t = CUADRO_LARGO;
    while t < fin {
        t = c.siguiente_suceso();
        c.avanzar(t);
    }
    assert_eq!(t, fin);
    assert_eq!(leer_a(&mut c, reg::ICR, fin), icr::ALRM);
}
