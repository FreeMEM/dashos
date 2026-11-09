//! La mezcla del servicio de sonido de DashOS (S19, fase 1).
//!
//! Cada programa tiene uno o varios flujos, cada uno en su formato (enteros de 16 o 32 bits o coma flotante de 32,
//! mono o estéreo, de 8 a 192 kHz). El mezclador pasa cada flujo a coma flotante, lo remuestrea a la frecuencia de la
//! salida, le aplica su volumen (izquierdo y derecho, que también sirve de paneo) y suma todos. Los cambios de volumen
//! van en rampa a lo largo de un período, para que no chasquen; un flujo nuevo entra subiendo desde cero y uno que se
//! cierra (o cuyo programa muere) sale con un fundido de 5 ms, gastando lo que le quedaba en la cola.
//!
//! El remuestreo es lineal. Cada flujo puede reajustar su ritmo unas partes por millón, para seguir el reloj real del
//! hardware (S18).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// El tipo de cada muestra, en little endian.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Muestra {
    I16,
    I32,
    F32,
}

impl Muestra {
    pub fn bytes(self) -> usize {
        match self {
            Muestra::I16 => 2,
            Muestra::I32 | Muestra::F32 => 4,
        }
    }

    /// El número con que va en los mensajes del servicio.
    pub fn desde(n: u32) -> Option<Muestra> {
        match n {
            0 => Some(Muestra::I16),
            1 => Some(Muestra::I32),
            2 => Some(Muestra::F32),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Formato {
    pub muestra: Muestra,
    pub canales: u8,
    pub frecuencia: u32,
}

impl Formato {
    pub fn valido(&self) -> bool {
        matches!(self.canales, 1 | 2) && (8000..=192_000).contains(&self.frecuencia)
    }

    pub fn bytes_por_cuadro(&self) -> usize {
        self.muestra.bytes() * self.canales as usize
    }
}

/// Un cuadro: la muestra izquierda y la derecha, entre -1 y 1.
pub type Cuadro = [f32; 2];

fn leer_muestra(m: Muestra, b: &[u8]) -> f32 {
    match m {
        Muestra::I16 => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
        Muestra::I32 => (i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 / 2_147_483_648.0) as f32,
        Muestra::F32 => {
            let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) }
        }
    }
}

/// Pasa a cuadros los cuadros enteros de `bytes` y los pone al final de `cola`. Devuelve los bytes usados: un cuadro
/// partido se queda sin usar.
pub fn convertir(f: &Formato, bytes: &[u8], cola: &mut VecDeque<Cuadro>) -> usize {
    let tam = f.bytes_por_cuadro();
    let n = bytes.len() / tam;
    let m = f.muestra.bytes();
    for c in bytes[..n * tam].chunks_exact(tam) {
        let i = leer_muestra(f.muestra, &c[..m]);
        let d = if f.canales == 2 { leer_muestra(f.muestra, &c[m..]) } else { i };
        cola.push_back([i, d]);
    }
    n * tam
}

/// Un cuadro en coma flotante a dos muestras de 16 bits, redondeando y sin pasarse.
pub fn a_i16(c: Cuadro) -> [i16; 2] {
    c.map(|x| {
        let v = x.clamp(-1.0, 1.0) * 32767.0;
        (if v >= 0.0 { v + 0.5 } else { v - 0.5 }) as i16
    })
}

/// El remuestreo lineal: `pos` es dónde cae la siguiente muestra de salida entre `previo` y el primero de la cola (de
/// 0 a 1), y avanza `paso` cuadros de entrada por cada uno de salida.
struct Remuestreo {
    previo: Cuadro,
    pos: f64,
    nominal: f64,
    paso: f64,
}

impl Remuestreo {
    fn nuevo(entrada: u32, salida: u32) -> Remuestreo {
        let nominal = entrada as f64 / salida as f64;
        Remuestreo { previo: [0.0; 2], pos: 0.0, nominal, paso: nominal }
    }

    /// El siguiente cuadro de salida, o `None` si a la cola le faltan cuadros (sin gastar nada).
    fn siguiente(&mut self, cola: &mut VecDeque<Cuadro>) -> Option<Cuadro> {
        while self.pos >= 1.0 {
            self.previo = cola.pop_front()?;
            self.pos -= 1.0;
        }
        let b = *cola.front()?;
        let t = self.pos as f32;
        let a = self.previo;
        self.pos += self.paso;
        Some([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Estado {
    Suena,
    /// Cerrándose: le quedan estos cuadros de fundido.
    Saliendo(u32),
}

struct Flujo {
    id: u32,
    formato: Formato,
    cola: VecDeque<Cuadro>,
    remuestreo: Remuestreo,
    /// El volumen pedido y el que se aplicó al final del último período.
    volumen: Cuadro,
    actual: Cuadro,
    estado: Estado,
    /// Ya recibió muestras: desde entonces, quedarse sin ellas es una falta.
    empezado: bool,
    faltas: u64,
}

/// Lo que cabe en la cola de un flujo: un segundo de su entrada.
fn cabida(f: &Formato) -> usize {
    f.frecuencia as usize
}

pub struct Mezclador {
    frecuencia: u32,
    flujos: Vec<Flujo>,
    siguiente: u32,
    mezcla: Vec<Cuadro>,
}

impl Mezclador {
    /// Un mezclador cuya salida va a `frecuencia` cuadros por segundo.
    pub fn nuevo(frecuencia: u32) -> Mezclador {
        Mezclador { frecuencia, flujos: Vec::new(), siguiente: 1, mezcla: Vec::new() }
    }

    pub fn frecuencia(&self) -> u32 {
        self.frecuencia
    }

    /// Los cuadros que dura el fundido de salida (5 ms).
    pub fn fundido(&self) -> u32 {
        (self.frecuencia / 200).max(1)
    }

    /// Un flujo nuevo, a volumen 1 en los dos lados. `None` si el formato no vale.
    pub fn abrir(&mut self, formato: Formato) -> Option<u32> {
        if !formato.valido() {
            return None;
        }
        let id = self.siguiente;
        self.siguiente = self.siguiente.wrapping_add(1).max(1);
        self.flujos.push(Flujo {
            id,
            formato,
            cola: VecDeque::new(),
            remuestreo: Remuestreo::nuevo(formato.frecuencia, self.frecuencia),
            volumen: [1.0; 2],
            actual: [0.0; 2],
            estado: Estado::Suena,
            empezado: false,
            faltas: 0,
        });
        Some(id)
    }

    fn flujo(&mut self, id: u32) -> Option<&mut Flujo> {
        self.flujos.iter_mut().find(|f| f.id == id)
    }

    /// Añade muestras al flujo. Devuelve los bytes que tomó (cuadros enteros, hasta llenar un segundo de cola); 0 si el
    /// flujo no existe o se está cerrando.
    pub fn escribir(&mut self, id: u32, bytes: &[u8]) -> usize {
        let Some(f) = self.flujo(id) else { return 0 };
        if f.estado != Estado::Suena {
            return 0;
        }
        let caben = cabida(&f.formato).saturating_sub(f.cola.len()) * f.formato.bytes_por_cuadro();
        let usados = convertir(&f.formato, &bytes[..bytes.len().min(caben)], &mut f.cola);
        if usados > 0 {
            f.empezado = true;
        }
        usados
    }

    /// Cuadros de entrada que esperan en la cola del flujo.
    pub fn pendientes(&self, id: u32) -> Option<usize> {
        self.flujos.iter().find(|f| f.id == id).map(|f| f.cola.len())
    }

    /// Cuántas veces se quedó sin muestras a mitad de un período.
    pub fn faltas(&self, id: u32) -> Option<u64> {
        self.flujos.iter().find(|f| f.id == id).map(|f| f.faltas)
    }

    /// El volumen de cada lado, de 0 a 1 (más de 1 amplifica, hasta 4).
    pub fn volumen(&mut self, id: u32, izquierdo: f32, derecho: f32) -> bool {
        let Some(f) = self.flujo(id) else { return false };
        f.volumen = [izquierdo, derecho].map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 4.0) });
        true
    }

    /// Reajusta el ritmo del flujo, en partes por millón (de -10.000 a 10.000): positivo gasta la entrada más deprisa.
    pub fn ajustar(&mut self, id: u32, ppm: i32) -> bool {
        let Some(f) = self.flujo(id) else { return false };
        let ppm = ppm.clamp(-10_000, 10_000);
        f.remuestreo.paso = f.remuestreo.nominal * (1.0 + ppm as f64 / 1e6);
        true
    }

    /// Cierra el flujo: sigue sonando lo que tenga en la cola mientras baja durante el fundido, y luego se quita.
    pub fn cerrar(&mut self, id: u32) -> bool {
        let fundido = self.fundido();
        let Some(f) = self.flujo(id) else { return false };
        if f.estado == Estado::Suena {
            f.estado = Estado::Saliendo(fundido);
        }
        true
    }

    /// Flujos vivos (también los que se están cerrando).
    pub fn flujos(&self) -> usize {
        self.flujos.len()
    }

    /// Mezcla un período en `salida`.
    pub fn mezclar(&mut self, salida: &mut [Cuadro]) {
        salida.fill([0.0; 2]);
        let n = salida.len();
        if n == 0 {
            return;
        }
        let fundido = self.fundido() as f32;
        for f in self.flujos.iter_mut() {
            let mut falto = false;
            for (i, s) in salida.iter_mut().enumerate() {
                // Rampa del volumen a lo largo del período.
                let t = (i + 1) as f32 / n as f32;
                let mut g = [0, 1].map(|c| f.actual[c] + (f.volumen[c] - f.actual[c]) * t);
                if let Estado::Saliendo(queda) = f.estado {
                    if queda == 0 {
                        break;
                    }
                    let k = (queda - 1) as f32 / fundido;
                    g = g.map(|x| x * k);
                    f.estado = Estado::Saliendo(queda - 1);
                }
                match f.remuestreo.siguiente(&mut f.cola) {
                    Some(c) => {
                        s[0] += c[0] * g[0];
                        s[1] += c[1] * g[1];
                    }
                    None => falto = true,
                }
            }
            f.actual = f.volumen;
            if falto && f.empezado && f.estado == Estado::Suena {
                f.faltas += 1;
            }
        }
        self.flujos.retain(|f| f.estado != Estado::Saliendo(0));
    }

    /// Mezcla un período en estéreo de 16 bits entrelazado (izquierda, derecha): `salida` tiene dos muestras por
    /// cuadro.
    pub fn mezclar_i16(&mut self, salida: &mut [i16]) {
        let n = salida.len() / 2;
        let mut mezcla = core::mem::take(&mut self.mezcla);
        mezcla.resize(n, [0.0; 2]);
        self.mezclar(&mut mezcla);
        for (c, s) in mezcla.iter().zip(salida.chunks_exact_mut(2)) {
            s.copy_from_slice(&a_i16(*c));
        }
        self.mezcla = mezcla;
    }
}

#[cfg(test)]
mod tests;
