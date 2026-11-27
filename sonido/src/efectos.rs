//! Los efectos del grafo (S19b, E1; encargo `sonido-pro`, paso 3): ecualizador paramétrico y gráfico con preajustes,
//! limitador (para que la salida nunca sature) y sonido espacial para auriculares (ensanchar y crossfeed), más una
//! ganancia sencilla.
//!
//! Todo en coma fija, como la mezcla (E9): las muestras con 1,0 = `UNO` (Q15 con margen). Los coeficientes se
//! calculan en coma flotante (`mates`) solo al cambiar un parámetro. Los parámetros son enteros con su unidad fija
//! (Hz, centésimas de decibelio...), y se recortan a su rango: un valor absurdo no rompe nada. Los cambios van en
//! rampa, sin chasquidos: el ecualizador recalcula sus coeficientes en tramos de 32 cuadros moviéndose hacia lo
//! pedido; el espacial mueve su cantidad cuadro a cuadro; el limitador suaviza su ganancia por construcción.

use crate::mates::{coseno, db_a_lineal, raiz, seno};
use crate::{Cuadro, UNO};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

/// Un efecto: procesa un período en su sitio.
pub trait Efecto {
    /// El nombre corto para las listas ("eq", "limitador"...).
    fn nombre(&self) -> &'static str;
    fn procesar(&mut self, buf: &mut [Cuadro]);
    /// Cambia un parámetro (se recorta a su rango). `false` si el parámetro no existe.
    fn poner(&mut self, parametro: u32, valor: i32) -> bool;
    /// El valor pedido de un parámetro.
    fn leer(&self, parametro: u32) -> Option<i32>;
}

/// Crea un efecto por su nombre, a la frecuencia de la salida.
pub fn crear(nombre: &str, frecuencia: u32) -> Option<Box<dyn Efecto>> {
    Some(match nombre {
        "eq" => Box::new(Ecualizador::nuevo(frecuencia)),
        "limitador" => Box::new(Limitador::nuevo(frecuencia)),
        "espacial" => Box::new(Espacial::nuevo(frecuencia)),
        "ganancia" => Box::new(Ganancia::nuevo()),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Ganancia

/// Una ganancia en centésimas de decibelio (parámetro 0, de -6000 a +2400), en rampa de un período.
pub struct Ganancia {
    pedida: i32,
    actual: i64,
    /// La pedida en lineal (Q16), calculada al cambiar (no en cada cuanto: es coma flotante por software).
    objetivo: i64,
}

impl Ganancia {
    pub fn nuevo() -> Self {
        Ganancia { pedida: 0, actual: 1 << 16, objetivo: 1 << 16 }
    }
}

fn centesimas_a_q16(cdb: i32) -> i64 {
    (db_a_lineal(cdb as f64 / 100.0) * 65_536.0) as i64
}

impl Efecto for Ganancia {
    fn nombre(&self) -> &'static str {
        "ganancia"
    }
    fn procesar(&mut self, buf: &mut [Cuadro]) {
        let objetivo = self.objetivo;
        if objetivo == 1 << 16 && self.actual == objetivo {
            return;
        }
        let n = buf.len().max(1) as i64;
        let a = self.actual;
        for (i, c) in buf.iter_mut().enumerate() {
            let g = a + (objetivo - a) * (i as i64 + 1) / n;
            *c = c.map(|x| ((x as i64 * g) >> 16).clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        }
        self.actual = objetivo;
    }
    fn poner(&mut self, parametro: u32, valor: i32) -> bool {
        if parametro != 0 {
            return false;
        }
        self.pedida = valor.clamp(-6000, 2400);
        self.objetivo = centesimas_a_q16(self.pedida);
        true
    }
    fn leer(&self, parametro: u32) -> Option<i32> {
        (parametro == 0).then_some(self.pedida)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Ecualizador

/// El tipo de cada banda.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tipo {
    Pico = 0,
    Graves = 1,
    Agudos = 2,
}

/// Los parámetros de una banda: frecuencia en Hz, ganancia en centésimas de dB, Q en centésimas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Banda {
    pub frecuencia: i32,
    pub ganancia: i32,
    pub q: i32,
    pub tipo: Tipo,
}

/// Las diez bandas del ecualizador gráfico (por octavas, ISO).
pub const BANDAS_GRAFICO: [i32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
pub const NUM_BANDAS: usize = 10;
/// Parámetros: banda `b` en `4b + {0 frecuencia, 1 ganancia, 2 Q, 3 tipo}`; 100 la preganancia (centésimas de dB);
/// 101 el preajuste (pone las diez bandas en modo gráfico; leerlo da el último puesto, o -1 si se tocó a mano).
pub const P_PREGANANCIA: u32 = 100;
pub const P_PREAJUSTE: u32 = 101;
const GANANCIA_MAX: i32 = 1500;

/// Preajustes del gráfico: nombre y ganancia de cada banda en dB.
pub const PREAJUSTES: [(&str, [i8; 10]); 8] = [
    ("Plano", [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    ("Graves", [6, 5, 4, 2, 0, 0, 0, 0, 0, 0]),
    ("Agudos", [0, 0, 0, 0, 0, 1, 2, 4, 5, 6]),
    ("Voz", [-4, -3, -1, 1, 3, 4, 3, 1, 0, -2]),
    ("Rock", [5, 4, 2, -1, -2, -1, 2, 3, 4, 4]),
    ("Electrónica", [6, 5, 1, 0, -2, 1, 0, 1, 4, 5]),
    ("Clásica", [3, 2, 0, 0, 0, 0, -1, -1, 2, 3]),
    ("Noche", [-3, -2, 0, 1, 2, 2, 1, 0, -2, -4]),
];

/// Coeficientes de un biquad en Q28 (b0, b1, b2, a1, a2; a0 normalizado a 1).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Coef([i64; 5]);

const Q_COEF: u32 = 28;
/// Bits de más con que van las muestras dentro del filtro (de Q15 a Q23): menos ruido en los graves.
const EXTRA: u32 = 8;

impl Coef {
    const PLANO: Coef = Coef([1 << Q_COEF, 0, 0, 0, 0]);

    /// Las fórmulas del "Audio EQ Cookbook" de Robert Bristow-Johnson (documento público).
    fn calcular(b: &Banda, frecuencia: u32) -> Coef {
        if b.ganancia == 0 {
            return Coef::PLANO;
        }
        let fs = frecuencia as f64;
        let f0 = (b.frecuencia as f64).clamp(10.0, fs * 0.45);
        let q = (b.q as f64 / 100.0).clamp(0.1, 10.0);
        let a = db_a_lineal(b.ganancia as f64 / 200.0); // raíz de la ganancia lineal
        let w0 = 2.0 * core::f64::consts::PI * f0 / fs;
        let (sw, cw) = (seno(w0), coseno(w0));
        let alfa = sw / (2.0 * q);
        let [b0, b1, b2, a0, a1, a2] = match b.tipo {
            Tipo::Pico => [1.0 + alfa * a, -2.0 * cw, 1.0 - alfa * a, 1.0 + alfa / a, -2.0 * cw, 1.0 - alfa / a],
            Tipo::Graves | Tipo::Agudos => {
                let sa = 2.0 * raiz(a) * alfa;
                let s = if b.tipo == Tipo::Graves { 1.0 } else { -1.0 };
                let (ap1, am1) = (a + 1.0, a - 1.0);
                [
                    a * (ap1 - s * am1 * cw + sa),
                    s * 2.0 * a * (am1 - s * ap1 * cw),
                    a * (ap1 - s * am1 * cw - sa),
                    ap1 + s * am1 * cw + sa,
                    -s * 2.0 * (am1 + s * ap1 * cw),
                    ap1 + s * am1 * cw - sa,
                ]
            }
        };
        let k = (1u64 << Q_COEF) as f64 / a0;
        Coef([b0 * k, b1 * k, b2 * k, a1 * k, a2 * k].map(|x| x as i64))
    }
}

/// Un biquad en forma directa I, con el estado de cada canal (x1, x2, y1, y2) en Q23 y el resto del redondeo de la
/// salida anterior: se suma a la siguiente (realimentación del error de primer orden). Sin eso, con polos tan cerca de 1
/// como los de un estante a 31 Hz, el truncado deja la salida atascada en un valor que no es cero tras el sonido (se
/// midió: 355 de 32.768 constantes, -39 dB, en el WAV con "Graves" y la música en pausa).
#[derive(Clone, Default)]
struct Biquad {
    estado: [[i64; 4]; 2],
    resto: [i64; 2],
}

/// Lo más que entra en un filtro: ±8,0 (deja margen en la cuenta de 64 bits).
const TOPE_FILTRO: i64 = 8 << (15 + EXTRA);

impl Biquad {
    fn procesar(&mut self, c: &Coef, buf: &mut [Cuadro]) {
        let [b0, b1, b2, a1, a2] = c.0;
        for m in buf.iter_mut() {
            for (canal, x) in m.iter_mut().enumerate() {
                let e = &mut self.estado[canal];
                let xi = ((*x as i64) << EXTRA).clamp(-TOPE_FILTRO, TOPE_FILTRO);
                let acc = b0 * xi + b1 * e[0] + b2 * e[1] - a1 * e[2] - a2 * e[3] + self.resto[canal];
                let y = acc >> Q_COEF;
                self.resto[canal] = acc - (y << Q_COEF);
                let y = y.clamp(-TOPE_FILTRO, TOPE_FILTRO);
                *e = [xi, e[0], y, e[2]];
                // A Q15 redondeando (no truncando: truncar da un desvío de media muestra hacia abajo).
                *x = ((y + (1 << (EXTRA - 1))) >> EXTRA) as i32;
            }
        }
    }
}

pub struct Ecualizador {
    frecuencia: u32,
    pedidas: [Banda; NUM_BANDAS],
    actuales: [Banda; NUM_BANDAS],
    coef: [Coef; NUM_BANDAS],
    filtros: [Biquad; NUM_BANDAS],
    preganancia: Ganancia,
    preajuste: i32,
}

/// Cuadros de cada tramo de la rampa del ecualizador.
const TRAMO: usize = 32;
/// Lo más que se mueve una banda por tramo: 0,5 dB de ganancia, un 6 % de frecuencia y Q.
const PASO_GANANCIA: i32 = 50;

fn acercar(actual: i32, pedido: i32, paso: i32) -> i32 {
    if actual < pedido { (actual + paso).min(pedido) } else { (actual - paso).max(pedido) }
}

fn acercar_proporcion(actual: i32, pedido: i32) -> i32 {
    let paso = (actual / 16).max(1);
    acercar(actual, pedido, paso)
}

impl Ecualizador {
    pub fn nuevo(frecuencia: u32) -> Self {
        let bandas = core::array::from_fn(|i| Banda { frecuencia: BANDAS_GRAFICO[i], ganancia: 0, q: 141, tipo: Tipo::Pico });
        Ecualizador {
            frecuencia,
            pedidas: bandas,
            actuales: bandas,
            coef: [Coef::PLANO; NUM_BANDAS],
            filtros: Default::default(),
            preganancia: Ganancia::nuevo(),
            preajuste: 0,
        }
    }

    pub fn banda(&self, i: usize) -> Option<Banda> {
        self.pedidas.get(i).copied()
    }

    fn poner_preajuste(&mut self, n: usize) {
        let (_, g) = PREAJUSTES[n];
        for i in 0..NUM_BANDAS {
            self.pedidas[i] = Banda { frecuencia: BANDAS_GRAFICO[i], ganancia: g[i] as i32 * 100, q: 141, tipo: Tipo::Pico };
        }
        // Los graves y los agudos de las puntas, en estante: el gráfico suena más natural así.
        self.pedidas[0].tipo = Tipo::Graves;
        self.pedidas[0].q = 71;
        self.pedidas[9].tipo = Tipo::Agudos;
        self.pedidas[9].q = 71;
        self.preajuste = n as i32;
    }

    fn quieto(&self) -> bool {
        self.pedidas == self.actuales
    }
}

impl Efecto for Ecualizador {
    fn nombre(&self) -> &'static str {
        "eq"
    }

    fn procesar(&mut self, buf: &mut [Cuadro]) {
        self.preganancia.procesar(buf);
        for tramo in buf.chunks_mut(TRAMO) {
            if !self.quieto() {
                for i in 0..NUM_BANDAS {
                    let (a, p) = (self.actuales[i], self.pedidas[i]);
                    if a == p {
                        continue;
                    }
                    self.actuales[i] = Banda {
                        frecuencia: acercar_proporcion(a.frecuencia, p.frecuencia),
                        ganancia: acercar(a.ganancia, p.ganancia, PASO_GANANCIA),
                        q: acercar_proporcion(a.q, p.q),
                        // El tipo cambia cuando la banda pasa por 0 dB (ahí el filtro es plano y no se nota).
                        tipo: if a.ganancia == 0 { p.tipo } else { a.tipo },
                    };
                    if a.tipo != p.tipo && a.ganancia != 0 {
                        // Hacia 0 dB primero.
                        self.actuales[i].ganancia = acercar(a.ganancia, 0, PASO_GANANCIA);
                    }
                    self.coef[i] = Coef::calcular(&self.actuales[i], self.frecuencia);
                }
            }
            for i in 0..NUM_BANDAS {
                if self.actuales[i].ganancia != 0 || self.coef[i] != Coef::PLANO {
                    self.filtros[i].procesar(&self.coef[i], tramo);
                }
            }
        }
    }

    fn poner(&mut self, parametro: u32, valor: i32) -> bool {
        match parametro {
            P_PREGANANCIA => {
                self.preganancia.poner(0, valor.clamp(-GANANCIA_MAX, GANANCIA_MAX));
            }
            P_PREAJUSTE => {
                if !(0..PREAJUSTES.len() as i32).contains(&valor) {
                    return false;
                }
                self.poner_preajuste(valor as usize);
            }
            p if (p as usize) < 4 * NUM_BANDAS => {
                let b = &mut self.pedidas[p as usize / 4];
                match p % 4 {
                    0 => b.frecuencia = valor.clamp(20, 20_000),
                    1 => b.ganancia = valor.clamp(-GANANCIA_MAX, GANANCIA_MAX),
                    2 => b.q = valor.clamp(10, 1000),
                    _ => {
                        b.tipo = match valor {
                            1 => Tipo::Graves,
                            2 => Tipo::Agudos,
                            _ => Tipo::Pico,
                        }
                    }
                }
                self.preajuste = -1;
            }
            _ => return false,
        }
        true
    }

    fn leer(&self, parametro: u32) -> Option<i32> {
        match parametro {
            P_PREGANANCIA => self.preganancia.leer(0),
            P_PREAJUSTE => Some(self.preajuste),
            p if (p as usize) < 4 * NUM_BANDAS => {
                let b = self.pedidas[p as usize / 4];
                Some(match p % 4 {
                    0 => b.frecuencia,
                    1 => b.ganancia,
                    2 => b.q,
                    _ => b.tipo as i32,
                })
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Limitador

/// Un limitador con mirada adelante: la salida nunca pasa del umbral (parámetro 0, en centésimas de dBFS, de -2400 a
/// 0; de serie -100, -1 dB), y sin chasquidos: la ganancia baja suave antes del pico (mínimo en la ventana y media
/// móvil) y vuelve despacio (parámetro 1, la vuelta en ms, de 10 a 2000; de serie 150). Retrasa la salida 1,5 ms.
///
/// La idea (mínimo deslizante y media de la misma longitud) es la de los limitadores con mirada adelante de la
/// literatura de audio; escrita aquí desde cero.
pub struct Limitador {
    umbral_cdb: i32,
    vuelta_ms: i32,
    /// El umbral en muestras (Q15).
    umbral: i64,
    largo: usize,
    retraso: VecDeque<Cuadro>,
    /// La ganancia necesaria de los últimos `largo + 1` cuadros (Q16), para el mínimo deslizante: cola monótona de
    /// (índice, ganancia).
    minimos: VecDeque<(u64, i64)>,
    /// La media móvil del mínimo: los últimos `largo` valores y su suma.
    medias: VecDeque<i64>,
    suma: i64,
    /// La ganancia aplicada (Q16) y lo que vuelve por cuadro (Q16 de Q16: fracción de lo que falta).
    envolvente: i64,
    vuelta: i64,
    n: u64,
    /// Cuadros en que actuó (bajó la ganancia), para las medidas.
    pub actuaciones: u64,
}

const UNO_G: i64 = 1 << 16;

impl Limitador {
    pub fn nuevo(frecuencia: u32) -> Self {
        let largo = (frecuencia as usize * 3 / 2000).max(1);
        let mut l = Limitador {
            umbral_cdb: -100,
            vuelta_ms: 150,
            umbral: 0,
            largo,
            retraso: VecDeque::from(vec![[0; 2]; largo]),
            minimos: VecDeque::new(),
            medias: VecDeque::from(vec![UNO_G; largo]),
            suma: UNO_G * largo as i64,
            envolvente: UNO_G,
            vuelta: 0,
            n: 0,
            actuaciones: 0,
        };
        l.recalcular(frecuencia);
        l
    }

    fn recalcular(&mut self, frecuencia: u32) {
        self.umbral = (db_a_lineal(self.umbral_cdb as f64 / 100.0) * UNO as f64) as i64;
        // Lo que falta se recorre en `vuelta_ms` con una exponencial: por cuadro, 1 - e^(-1/(τ·fs)) ≈ 1/(τ·fs).
        let cuadros = (self.vuelta_ms as i64 * frecuencia as i64 / 1000).max(1);
        self.vuelta = (UNO_G / cuadros).max(1);
    }

    pub fn retraso(&self) -> usize {
        self.largo
    }
}

impl Efecto for Limitador {
    fn nombre(&self) -> &'static str {
        "limitador"
    }

    fn procesar(&mut self, buf: &mut [Cuadro]) {
        let largo = self.largo as u64;
        // Lo de siempre: nada pasa del umbral, ni en lo que entra ni en lo que espera, y la ganancia está entera. Solo
        // hay que retrasar (sin el mínimo deslizante ni la media, que darían 1).
        let pico = buf.iter().map(|c| (c[0] as i64).abs().max((c[1] as i64).abs())).max().unwrap_or(0);
        if pico <= self.umbral && self.envolvente == UNO_G && self.suma == UNO_G * largo as i64 {
            self.minimos.clear();
            for c in buf.iter_mut() {
                self.retraso.push_back(*c);
                *c = self.retraso.pop_front().unwrap_or([0; 2]);
            }
            self.n += buf.len() as u64;
            return;
        }
        for c in buf.iter_mut() {
            let pico = (c[0] as i64).abs().max((c[1] as i64).abs());
            let necesaria = if pico > self.umbral { (self.umbral << 16) / pico } else { UNO_G };
            // Mínimo deslizante de los últimos largo + 1.
            while self.minimos.back().is_some_and(|&(_, g)| g >= necesaria) {
                self.minimos.pop_back();
            }
            self.minimos.push_back((self.n, necesaria));
            while self.minimos.front().is_some_and(|&(i, _)| i + largo < self.n) {
                self.minimos.pop_front();
            }
            let minimo = self.minimos.front().map(|&(_, g)| g).unwrap_or(UNO_G);
            // Media de los últimos `largo` mínimos: baja en rampa y llega al pico con lo que pide.
            self.suma += minimo - self.medias.pop_front().unwrap_or(UNO_G);
            self.medias.push_back(minimo);
            let media = self.suma / largo as i64;
            // Bajar, al momento (ya es suave); subir, despacio.
            if media < self.envolvente {
                self.envolvente = media;
            } else {
                self.envolvente = (self.envolvente + (((media - self.envolvente) * self.vuelta) >> 16).max(1)).min(media);
            }
            if self.envolvente < UNO_G {
                self.actuaciones += 1;
            }
            self.retraso.push_back(*c);
            let viejo = self.retraso.pop_front().unwrap_or([0; 2]);
            // Por si acaso (redondeos): nunca pasa del umbral.
            let u = self.umbral;
            *c = viejo.map(|x| ((x as i64 * self.envolvente) >> 16).clamp(-u, u) as i32);
            self.n += 1;
        }
    }

    fn poner(&mut self, parametro: u32, valor: i32) -> bool {
        match parametro {
            0 => self.umbral_cdb = valor.clamp(-2400, 0),
            1 => self.vuelta_ms = valor.clamp(10, 2000),
            _ => return false,
        }
        // La frecuencia se deduce del largo (1,5 ms).
        let fs = (self.largo as u32 * 2000 / 3).max(8000);
        self.recalcular(fs);
        true
    }

    fn leer(&self, parametro: u32) -> Option<i32> {
        match parametro {
            0 => Some(self.umbral_cdb),
            1 => Some(self.vuelta_ms),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Espacial

/// Sonido espacial sencillo, para auriculares. Parámetro 0, el modo: 0 apagado, 1 ensanchar (sube la diferencia entre
/// los lados), 2 auriculares (crossfeed: un poco de cada lado, retrasado ~0,3 ms y sin agudos, pasa al otro, como
/// llega a los oídos con altavoces). Parámetro 1, la cantidad, de 0 a 100. Al cambiar, se cruza de un modo a otro
/// en rampa.
pub struct Espacial {
    modo: i32,
    cantidad: i32,
    /// La cantidad aplicada de cada modo (Q16), que va hacia la pedida cuadro a cuadro.
    ancho: i64,
    cruce: i64,
    retraso: VecDeque<Cuadro>,
    /// El filtro de paso bajo (un polo) de lo que se cruza, en Q15 con 8 bits más.
    bajo: [i64; 2],
    coef_bajo: i64,
    paso: i64,
    /// La línea de retraso tiene algo de cuando estaba encendido.
    sucio: bool,
}

impl Espacial {
    pub fn nuevo(frecuencia: u32) -> Self {
        let r = (frecuencia as usize * 3 / 10_000).max(1);
        // Paso bajo a ~700 Hz: a = 1 - e^(-2π·700/fs).
        let a = 1.0 - crate::mates::exp(-2.0 * core::f64::consts::PI * 700.0 / frecuencia as f64);
        Espacial {
            modo: 0,
            cantidad: 50,
            ancho: 0,
            cruce: 0,
            retraso: VecDeque::from(vec![[0; 2]; r]),
            bajo: [0; 2],
            coef_bajo: (a * 65_536.0) as i64,
            // Una rampa entera en unos 20 ms.
            paso: (UNO_G * 50 / frecuencia as i64).max(1),
            sucio: false,
        }
    }

    fn objetivos(&self) -> (i64, i64) {
        let k = self.cantidad as i64 * UNO_G / 100;
        match self.modo {
            1 => (k, 0),
            2 => (0, k),
            _ => (0, 0),
        }
    }
}

impl Efecto for Espacial {
    fn nombre(&self) -> &'static str {
        "espacial"
    }

    fn procesar(&mut self, buf: &mut [Cuadro]) {
        let (oa, oc) = self.objetivos();
        if oa == 0 && oc == 0 && self.ancho == 0 && self.cruce == 0 {
            // Apagado y quieto: nada. Lo que quede en la línea de retraso es viejo, pero al encender entra en rampa
            // desde cero; aun así se borra, para no oír ni un eco de antes.
            if self.sucio {
                self.retraso.iter_mut().for_each(|c| *c = [0; 2]);
                self.bajo = [0; 2];
                self.sucio = false;
            }
            return;
        }
        self.sucio = true;
        for c in buf.iter_mut() {
            self.ancho = acercar64(self.ancho, oa, self.paso);
            self.cruce = acercar64(self.cruce, oc, self.paso);
            let (l, r) = (c[0] as i64, c[1] as i64);
            // Ensanchar: lado (L-R)/2 por 1 + ancho (hasta el doble).
            let medio = (l + r) / 2;
            let lado = (l - r) / 2;
            let lado = lado + ((lado * self.ancho) >> 16);
            let (mut l2, mut r2) = (medio + lado, medio - lado);
            // Crossfeed: el otro lado, retrasado y por un paso bajo, hasta un 45 %; se compensa el nivel.
            self.retraso.push_back(*c);
            let viejo = self.retraso.pop_front().unwrap_or([0; 2]);
            for k in 0..2 {
                let x = (viejo[k] as i64) << 8;
                self.bajo[k] += ((x - self.bajo[k]) * self.coef_bajo) >> 16;
            }
            let g = (self.cruce * 29_491) >> 16; // 0,45 · cruce
            if g > 0 {
                let comp = UNO_G - g / 3;
                l2 = ((l2 * comp) >> 16) + (((self.bajo[1] >> 8) * g) >> 16);
                r2 = ((r2 * comp) >> 16) + (((self.bajo[0] >> 8) * g) >> 16);
            }
            *c = [l2.clamp(i32::MIN as i64, i32::MAX as i64) as i32, r2.clamp(i32::MIN as i64, i32::MAX as i64) as i32];
        }
    }

    fn poner(&mut self, parametro: u32, valor: i32) -> bool {
        match parametro {
            0 => self.modo = valor.clamp(0, 2),
            1 => self.cantidad = valor.clamp(0, 100),
            _ => return false,
        }
        true
    }

    fn leer(&self, parametro: u32) -> Option<i32> {
        match parametro {
            0 => Some(self.modo),
            1 => Some(self.cantidad),
            _ => None,
        }
    }
}

fn acercar64(actual: i64, pedido: i64, paso: i64) -> i64 {
    if actual < pedido { (actual + paso).min(pedido) } else { (actual - paso).max(pedido) }
}

/// Las ganancias de un preajuste, en centésimas de dB.
pub fn preajuste(n: usize) -> Option<[i32; NUM_BANDAS]> {
    PREAJUSTES.get(n).map(|(_, g)| g.map(|x| x as i32 * 100))
}

#[cfg(test)]
#[path = "pruebas_efectos.rs"]
mod pruebas;

/// Para las pruebas y para quien quiera saber la respuesta de un ecualizador sin oírlo: la ganancia en centésimas de
/// dB a una frecuencia, midiendo un tono (lento: solo para pruebas).
pub fn medir_respuesta(eq: &mut dyn Efecto, frecuencia: u32, tono: u32) -> i32 {
    let n = frecuencia as usize / 4;
    let amp = UNO as f64 * 0.25;
    let mut buf: Vec<Cuadro> = (0..n)
        .map(|i| {
            let v = (seno(2.0 * core::f64::consts::PI * tono as f64 * i as f64 / frecuencia as f64) * amp) as i32;
            [v, v]
        })
        .collect();
    for t in buf.chunks_mut(256) {
        eq.procesar(t);
    }
    // Por el nivel cuadrático medio de la segunda mitad (el pico de las muestras engaña en los agudos).
    let mitad = &buf[n / 2..];
    let suma: f64 = mitad.iter().map(|c| (c[0] as f64) * (c[0] as f64)).sum();
    let rms = raiz(suma / mitad.len() as f64);
    let r = rms / (amp / raiz(2.0));
    // 20 · log10(r) en centésimas, por log10_centesimas de un entero escalado.
    let escala = (r * 1_000_000.0) as u64;
    crate::mates::log10_centesimas(escala.max(1)) * 20 - 600 * 20
}
