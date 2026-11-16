//! La rejilla de permisos de la Información (S10), para el kit: la usan el Workbench y el explorador.
//! Sus textos van en el catálogo del kit (`kit::textos::tr`; `locale/programas.txt` la cuenta con las fuentes del kit).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use kit::controles::Contexto;
use kit::grupos::Medida;
use kit::lienzo::{Lienzo, Rect};
use kit::objetos::{atr, Atr, Clase, Valor};
use kit::tema::{self, con_opacidad, Estado};
use kit::texto::Fuente;
use kit::textos::tr;

fn centrado(f: &mut Fuente, l: &mut Lienzo, cx: i32, base: i32, texto: &str, tam: u32, color: kit::lienzo::Color) {
    let w = f.medir(texto, tam).ancho;
    f.dibujar(l, cx - w / 2, base, texto, tam, color);
}

/// Los permisos de Unix y los bits del Amiga juntos, en una rejilla de casillas (Material 3): filas Dueño, Grupo y Otros
/// con Leer, Escribir, Ejecutar y No borrar; y una fila más con Guion, Puro, Archivado y Residente. `VALOR` lleva los
/// bits del Amiga en los bits 16 y siguientes y el modo (12 bits) abajo. Al pulsar se apunta la casilla en `CURSOR` y
/// al soltar dentro se cambia.
pub struct Permisos;

const PERM_ETIQUETA: i32 = 76;
const PERM_COL: i32 = 80;
const PERM_CAB: i32 = 24;
const PERM_FILA: i32 = 40;

/// Las cabeceras de las columnas (las de Unix y las del Amiga) y los nombres de las cuatro filas, en el idioma del
/// usuario.
fn columnas() -> [&'static str; 4] {
    [tr(14, "Leer"), tr(15, "Escribir"), tr(16, "Ejecutar"), tr(17, "No borrar")]
}

fn columnas_amiga() -> [&'static str; 4] {
    [tr(18, "Guion"), tr(19, "Puro"), tr(20, "Archivado"), tr(21, "Residente")]
}

fn filas() -> [&'static str; 4] {
    [tr(22, "Dueño"), tr(23, "Grupo"), tr(24, "Otros"), tr(25, "Amiga")]
}

impl Permisos {
    /// El bit de la casilla (fila 0 a 3, columna 0 a 3) en el valor junto.
    fn bit(fila: usize, col: usize) -> i64 {
        match (fila, col) {
            (0..=2, 0..=2) => 0o400 >> (3 * fila + col),
            (0..=2, 3) => (16 << fila) << 16,
            (3, _) => ([4, 2, 1, 8][col]) << 16,
            _ => 0,
        }
    }

    /// El centro de la casilla, dentro de `r`.
    pub fn centro(r: Rect, fila: usize, col: usize) -> (i32, i32) {
        let y = if fila < 3 { PERM_CAB + fila as i32 * PERM_FILA } else { 2 * PERM_CAB + 3 * PERM_FILA };
        (r.x + PERM_ETIQUETA + col as i32 * PERM_COL + PERM_COL / 2, r.y + y + PERM_FILA / 2)
    }

    fn casilla_en(r: Rect, x: i32, y: i32) -> i64 {
        for fila in 0..4 {
            for col in 0..4 {
                let (cx, cy) = Self::centro(r, fila, col);
                if (x - cx).abs() <= 20 && (y - cy).abs() <= 20 {
                    return (fila * 4 + col) as i64;
                }
            }
        }
        -1
    }
}

impl Clase for Permisos {
    fn nombre(&self) -> &'static str {
        "permisos"
    }

    fn medida(&self, _attrs: &BTreeMap<Atr, Valor>, _ctx: &mut Contexto) -> Medida {
        Medida::fija(PERM_ETIQUETA + 4 * PERM_COL, 2 * PERM_CAB + 4 * PERM_FILA)
    }

    fn dibujar(&self, attrs: &BTreeMap<Atr, Valor>, l: &mut Lienzo, r: Rect, ctx: &mut Contexto, e: Estado) {
        let t = ctx.tema;
        let valor = attrs.get(&atr::VALOR).map_or(0, Valor::int);
        let pulsada = if e.pulsado { attrs.get(&atr::CURSOR).map_or(-1, Valor::int) } else { -1 };
        let suave = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_variante_superficie };
        for (cab, nombres) in [(0, columnas()), (PERM_CAB + 3 * PERM_FILA, columnas_amiga())] {
            for (c, n) in nombres.iter().enumerate() {
                let cx = r.x + PERM_ETIQUETA + c as i32 * PERM_COL + PERM_COL / 2;
                centrado(&mut ctx.medium, l, cx, r.y + cab + 16, n, 12, suave);
            }
        }
        let tinta = if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_superficie };
        for (fila, n) in filas().iter().enumerate() {
            let (_, cy) = Self::centro(r, fila, 0);
            ctx.regular.dibujar(l, r.x, cy + 5, n, 14, tinta);
            for col in 0..4 {
                let (cx, cy) = Self::centro(r, fila, col);
                let marcada = valor & Self::bit(fila, col) != 0;
                let caja = Rect::new(cx - 9, cy - 9, 18, 18);
                if pulsada == (fila * 4 + col) as i64 {
                    l.redondeado(Rect::new(cx - 20, cy - 20, 40, 40), 20, con_opacidad(if marcada { t.sobre_superficie } else { t.primario }, 26));
                }
                if marcada {
                    l.redondeado(caja, 2, if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.primario });
                    let marca = if e.desactivado { t.superficie } else { t.sobre_primario };
                    let p = |x: i32, y: i32| (caja.x * 8 + x, caja.y * 8 + y);
                    l.trazo(p(30, 74), p(58, 102), 16, marca);
                    l.trazo(p(58, 102), p(115, 45), 16, marca);
                } else {
                    l.borde(caja, 2, 2, if e.desactivado { tema::apagado(t.sobre_superficie) } else { t.sobre_variante_superficie });
                }
            }
        }
    }

    fn pulsable(&self) -> bool {
        true
    }

    fn arrastrar(&self, _attrs: &BTreeMap<Atr, Valor>, r: Rect, x: i32, y: i32) -> Vec<(Atr, Valor)> {
        alloc::vec![(atr::CURSOR, Valor::Int(Self::casilla_en(r, x, y)))]
    }

    fn clic(&self, attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        let k = attrs.get(&atr::CURSOR).map_or(-1, Valor::int);
        if !(0..16).contains(&k) {
            return Vec::new();
        }
        let v = attrs.get(&atr::VALOR).map_or(0, Valor::int);
        alloc::vec![(atr::VALOR, Valor::Int(v ^ Self::bit(k as usize / 4, k as usize % 4)))]
    }
}

