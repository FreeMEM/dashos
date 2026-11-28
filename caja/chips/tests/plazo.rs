//! K8: `Chips::plazo`, lo que usa el contador hacia atrás de los motores. El nivel que piden los chips con el mapa
//! del Hardware Reference Manual (capítulo 7, "Interrupts") y la hora del siguiente suceso, con un motor de mentira
//! que solo vuelve a los chips cuando llega esa hora.

use caja_chips::cia::{cr, icr, reg};
use caja_chips::{CICLOS_POR_LINEA, Chips, Plazo, int};

const CUADRO_LARGO: u64 = 313 * CICLOS_POR_LINEA;

const INTENA: u32 = 0xDFF09A;
const INTREQ: u32 = 0xDFF09C;
const INTREQR: u32 = 0xDFF01E;

fn escribir16(c: &mut Chips, dir: u32, v: u16, hora: u64) {
    c.escribir(dir, u32::from(v), 2, hora);
}

fn escribir_a(c: &mut Chips, r: u8, v: u8, hora: u64) {
    c.escribir(0xBFE001 + u32::from(r) * 0x100, u32::from(v), 1, hora);
}

fn escribir_b(c: &mut Chips, r: u8, v: u8, hora: u64) {
    c.escribir(0xBFD000 + u32::from(r) * 0x100, u32::from(v), 1, hora);
}

/// El nivel de cada bit de `INTREQ` (0 a 13), del manual: 1 `TBE`, `DSKBLK`, `SOFT`; 2 `PORTS`; 3 `COPER`, `VERTB`,
/// `BLIT`; 4 `AUD0`-`AUD3`; 5 `RBF`, `DSKSYN`; 6 `EXTER`.
const NIVEL_DEL_BIT: [u8; 14] = [1, 1, 1, 2, 3, 3, 3, 4, 4, 4, 4, 5, 5, 6];

/// El nivel que tiene que salir con estos bits pedidos y habilitados: el más alto.
fn esperado(bits: u16) -> u8 {
    (0..14).filter(|b| bits & (1 << b) != 0).map(|b| NIVEL_DEL_BIT[b]).max().unwrap_or(0)
}

#[test]
fn el_nivel_de_cada_combinacion_de_bits() {
    // Las 16.384 combinaciones de INTREQ, con todo habilitado y con la mitad, y sin INTEN.
    for bits in 0u16..(1 << 14) {
        for habilitados in [0x3FFF, 0x2AAA, 0x1555] {
            let mut c = Chips::nuevo();
            escribir16(&mut c, INTREQ, 0x8000 | bits, 0);
            escribir16(&mut c, INTENA, 0x8000 | habilitados, 0);
            assert_eq!(c.plazo(0).ipl, 0, "sin INTEN: bits {bits:04x}");
            escribir16(&mut c, INTENA, 0x8000 | int::INTEN, 0);
            assert_eq!(c.plazo(0).ipl, esperado(bits & habilitados), "bits {bits:04x}, INTENA {habilitados:04x}");
        }
    }
}

#[test]
fn el_mapa_del_manual_bit_a_bit() {
    let casos = [
        (int::TBE, 1),
        (int::DSKBLK, 1),
        (int::SOFT, 1),
        (int::PORTS, 2),
        (int::COPER, 3),
        (int::VERTB, 3),
        (int::BLIT, 3),
        (int::AUD0, 4),
        (int::AUD1, 4),
        (int::AUD2, 4),
        (int::AUD3, 4),
        (int::RBF, 5),
        (int::DSKSYN, 5),
        (int::EXTER, 6),
    ];
    for (bit, nivel) in casos {
        assert_eq!(esperado(bit), nivel, "la tabla de la prueba, bit {bit:04x}");
        let mut c = Chips::nuevo();
        escribir16(&mut c, INTENA, 0x8000 | int::INTEN | bit, 0);
        escribir16(&mut c, INTREQ, 0x8000 | bit, 0);
        assert_eq!(c.plazo(0).ipl, nivel, "bit {bit:04x}");
    }
}

#[test]
fn sin_nada_el_plazo_es_el_fin_del_cuadro() {
    let mut c = Chips::nuevo();
    assert_eq!(c.plazo(0), Plazo { ipl: 0, siguiente: CUADRO_LARGO });
    assert_eq!(c.plazo(CUADRO_LARGO).siguiente, 2 * CUADRO_LARGO);
}

/// Un motor de mentira, como el de Emu68: cuenta 8 ciclos por instrucción, sale a mirar el plazo entre bloques de
/// `bloque` instrucciones y solo entonces vuelve a los chips. Corre hasta `hasta` y dice en qué horas volvió y con qué
/// nivel.
fn motor(c: &mut Chips, hasta: u64, bloque: u64) -> Vec<(u64, u8)> {
    let mut hora = 0;
    let mut p = c.plazo(0);
    let mut vueltas = vec![(0, p.ipl)];
    while hora < hasta {
        hora += 8 * bloque;
        if hora >= p.siguiente {
            p = c.plazo(hora);
            assert!(p.siguiente > hora, "el plazo siguiente, siempre después de la hora");
            vueltas.push((hora, p.ipl));
        }
    }
    vueltas
}

#[test]
fn el_contador_llega_al_fin_del_cuadro_y_pide_nivel_3() {
    for bloque in [1, 7, 64, 256] {
        let mut c = Chips::nuevo();
        escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::VERTB, 0);
        let v = motor(&mut c, 3 * CUADRO_LARGO, bloque);
        // Una vuelta por cuadro (no hay otros sucesos), en el primer bloque que pasa del fin del cuadro.
        assert_eq!(v.len(), 4, "bloque {bloque}: {v:?}");
        assert_eq!(v[0], (0, 0));
        for (k, &(hora, ipl)) in v.iter().enumerate().skip(1) {
            let fin = k as u64 * CUADRO_LARGO;
            assert!(hora >= fin && hora < fin + 8 * bloque, "bloque {bloque}: vuelta {k} a la hora {hora}");
            assert_eq!(ipl, 3, "bloque {bloque}: VERTB pide el nivel 3");
        }
    }
}

#[test]
fn sin_vertb_en_intena_el_fin_del_cuadro_no_pide_nada() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::BLIT, 0);
    let v = motor(&mut c, 2 * CUADRO_LARGO, 32);
    assert!(v.iter().all(|&(_, ipl)| ipl == 0), "{v:?}");
    assert_eq!(c.leer(INTREQR, 2, c.hora()) as u16 & int::VERTB, int::VERTB, "el bit sube igual");
}

#[test]
fn el_temporizador_a_del_cia_a_pide_nivel_2() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::PORTS, 0);
    escribir_a(&mut c, reg::ICR, icr::IR_SC | icr::TA, 0);
    // Una vez, latch 100: 3 pulsos de espera y 100 bajando (ver chips.rs, temporizador_una_vez).
    escribir_a(&mut c, reg::CRA, cr::RUNMODE, 0);
    escribir_a(&mut c, reg::TALO, 100, 0);
    escribir_a(&mut c, reg::TAHI, 0, 0);
    assert_eq!(c.plazo(0), Plazo { ipl: 0, siguiente: 1030 });
    assert_eq!(c.plazo(1029).ipl, 0);
    let p = c.plazo(1030);
    assert_eq!(p.ipl, 2);
    assert_eq!(p.siguiente, CUADRO_LARGO, "parado, el siguiente es el fin del cuadro");
}

#[test]
fn el_temporizador_b_del_cia_b_pide_nivel_6() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::EXTER | int::VERTB, 0);
    escribir_b(&mut c, reg::ICR, icr::IR_SC | icr::TB, 0);
    escribir_b(&mut c, reg::CRB, cr::RUNMODE, 0);
    escribir_b(&mut c, reg::TBLO, 100, 0);
    escribir_b(&mut c, reg::TBHI, 0, 0);
    let v = motor(&mut c, CUADRO_LARGO + 1, 4);
    assert_eq!(v[1], (1056, 6), "el primer bloque que pasa de 1030: {v:?}");
    assert_eq!(v.last().unwrap().1, 6, "con VERTB también: manda el 6");
}

#[test]
fn escribir_intena_o_intreq_cambia_el_nivel_enseguida() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::VERTB, 0);
    let h = CUADRO_LARGO + 10;
    assert_eq!(c.plazo(h).ipl, 3);
    escribir16(&mut c, INTENA, int::INTEN, h);
    assert_eq!(c.plazo(h).ipl, 0, "sin INTEN");
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN, h);
    assert_eq!(c.plazo(h).ipl, 3, "INTREQ sigue con VERTB");
    escribir16(&mut c, INTREQ, int::VERTB, h);
    assert_eq!(c.plazo(h), Plazo { ipl: 0, siguiente: 2 * CUADRO_LARGO }, "atendida");
    escribir16(&mut c, INTREQ, 0x8000 | int::SOFT, h);
    escribir16(&mut c, INTENA, 0x8000 | int::SOFT, h);
    assert_eq!(c.plazo(h).ipl, 1, "SOFT, puesta por el 68k");
}

#[test]
fn una_escritura_adelanta_el_plazo() {
    let mut c = Chips::nuevo();
    assert_eq!(c.plazo(100).siguiente, CUADRO_LARGO);
    // Un temporizador continuo de 9: el primer desborde a los 11 pulsos de reloj E desde que arranca.
    escribir_a(&mut c, reg::TALO, 9, 100);
    escribir_a(&mut c, reg::TAHI, 0, 100);
    escribir_a(&mut c, reg::CRA, cr::START, 100);
    assert_eq!(c.plazo(100).siguiente, 210);
}

#[test]
fn una_hora_anterior_no_cambia_nada() {
    let mut c = Chips::nuevo();
    escribir16(&mut c, INTENA, 0x8000 | int::INTEN | int::VERTB, 0);
    let p = c.plazo(CUADRO_LARGO + 500);
    assert_eq!(c.plazo(1000), p);
    assert!(p.siguiente > c.hora());
}
