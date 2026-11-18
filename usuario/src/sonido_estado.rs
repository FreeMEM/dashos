//! El estado del servicio de sonido, leído y escrito con textos y listas (encargo `sonido-pro`). Las constantes y la
//! disposición de las palabras están en `dashos::protocolo::sonido`; esto necesita montón, así que no va en la
//! biblioteca: lo incluye con `#[path]` quien lo usa (el servicio, el mezclador, el panel, la barra...).
//!
//! Se prueba en el PC con `rustc --edition 2024 --test usuario/src/sonido_estado.rs`.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
#[cfg(not(test))]
use dashos::protocolo::sonido::*;
#[cfg(test)]
#[path = "protocolo/sonido.rs"]
mod protocolo_sonido;
#[cfg(test)]
use protocolo_sonido::*;


/// Un nodo tal como lo ve el mezclador.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nodo {
    pub id: u32,
    pub clase: u32,
    /// Para un flujo, la salida por la que suena (0 si ninguna).
    pub destino: u32,
    /// Volumen de cada lado: 1,0 = 65.536.
    pub volumen: [u32; 2],
    pub silencio: bool,
    /// Pico y nivel medio de cada lado desde la última vez: 1,0 = 32.768.
    pub pico: [u32; 2],
    pub rms: [u32; 2],
    pub dueno: u32,
    pub marcas: u32,
    pub periodo: u32,
    pub nombre: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Medio {
    pub id: u32,
    pub estado: u32,
    pub capacidades: u32,
    pub posicion_ms: u32,
    pub duracion_ms: u32,
    pub flujo: u32,
    pub titulo: String,
    pub artista: String,
    pub programa: String,
}

#[derive(Clone, Debug, Default)]
pub struct Estado {
    pub secuencia: u32,
    pub defecto: u32,
    pub entrada: u32,
    pub cuanto: u32,
    pub periodo_modo: u32,
    pub vacios: u32,
    pub eq_preajuste: i32,
    pub eq_activo: bool,
    pub eq_bandas: [i32; 10],
    pub limitador: bool,
    pub limitador_actua: u32,
    pub espacial_modo: u32,
    pub espacial_cantidad: u32,
    pub tema_sonidos: u32,
    pub sonidos_silenciados: bool,
    pub no_molestar: bool,
    pub sonidos_volumen: u32,
    pub ventana: u32,
    pub mezcla_us: u32,
    pub mezcla_peor_us: u32,
    pub latencia_us: u32,
    pub bandas: Vec<u16>,
    pub onda: Vec<i16>,
    pub nodos: Vec<Nodo>,
    pub medios: Vec<Medio>,
}

impl Estado {
    pub fn nodo(&self, id: u32) -> Option<&Nodo> {
        self.nodos.iter().find(|n| n.id == id)
    }

    /// Las salidas (aparatos).
    pub fn salidas(&self) -> impl Iterator<Item = &Nodo> {
        self.nodos.iter().filter(|n| n.clase == CLASE_SALIDA)
    }

    pub fn entradas(&self) -> impl Iterator<Item = &Nodo> {
        self.nodos.iter().filter(|n| n.clase == CLASE_ENTRADA)
    }

    /// Los flujos de los programas (y de las cajas).
    pub fn flujos(&self) -> impl Iterator<Item = &Nodo> {
        self.nodos.iter().filter(|n| n.clase == CLASE_FLUJO && n.marcas & N_SALIENDO == 0)
    }

    /// La mezcla de una salida: su volumen es el de la salida.
    pub fn mezcla_de(&self, salida: u32) -> Option<&Nodo> {
        let nombre = &self.nodo(salida)?.nombre;
        self.nodos.iter().find(|n| n.clase == CLASE_MEZCLA && &n.nombre == nombre)
    }

    /// El nombre de la salida del sistema.
    pub fn nombre_defecto(&self) -> &str {
        self.nodo(self.defecto).map(|n| n.nombre.as_str()).unwrap_or("Sin salida")
    }
}

/// Un texto en palabras, cortado sin partir un carácter y con ceros al final.
pub fn texto_a_palabras(t: &str, palabras: &mut [u32]) {
    let max = palabras.len() * 4 - 1;
    let mut fin = t.len().min(max);
    while !t.is_char_boundary(fin) {
        fin -= 1;
    }
    let mut b = alloc::vec![0u8; palabras.len() * 4];
    b[..fin].copy_from_slice(&t.as_bytes()[..fin]);
    for (i, p) in palabras.iter_mut().enumerate() {
        *p = u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
    }
}

pub fn palabras_a_texto(palabras: &[u32]) -> String {
    let b: Vec<u8> = palabras.iter().flat_map(|p| p.to_le_bytes()).collect();
    let fin = b.iter().position(|&x| x == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..fin]).into_owned()
}

/// Lee el estado de las palabras `w` (una copia de la memoria). `None` si el servicio estaba a mitad de escribir (la
/// secuencia es impar) o la versión no es esta.
pub fn leer(w: &[u32]) -> Option<Estado> {
    if w.len() * 4 < TAM_ESTADO || w[W_SECUENCIA] & 1 == 1 || w[W_VERSION] != VERSION {
        return None;
    }
    let n = (w[W_NODOS] as usize).min(MAX_NODOS);
    let nodos = (0..n)
        .map(|i| {
            let b = &w[W_TABLA + i * PALABRAS_NODO..W_TABLA + (i + 1) * PALABRAS_NODO];
            Nodo {
                id: b[0],
                clase: b[1],
                destino: b[2],
                volumen: [b[3], b[4]],
                silencio: b[5] != 0,
                pico: [b[6], b[7]],
                rms: [b[8], b[9]],
                dueno: b[10],
                marcas: b[11],
                periodo: b[12],
                nombre: palabras_a_texto(&b[16..32]),
            }
        })
        .collect();
    let m = (w[W_MEDIOS] as usize).min(MAX_MEDIOS);
    let medios = (0..m)
        .map(|i| {
            let b = &w[W_TABLA_MEDIOS + i * PALABRAS_MEDIO..W_TABLA_MEDIOS + (i + 1) * PALABRAS_MEDIO];
            Medio {
                id: b[0],
                estado: b[1],
                capacidades: b[2],
                posicion_ms: b[3],
                duracion_ms: b[4],
                flujo: b[5],
                titulo: palabras_a_texto(&b[16..40]),
                artista: palabras_a_texto(&b[40..56]),
                programa: palabras_a_texto(&b[56..64]),
            }
        })
        .collect();
    let nb = (w[W_BANDAS] as usize).min(MAX_BANDAS);
    Some(Estado {
        secuencia: w[W_SECUENCIA],
        defecto: w[W_DEFECTO],
        entrada: w[W_ENTRADA],
        cuanto: w[W_CUANTO],
        periodo_modo: w[W_PERIODO_MODO],
        vacios: w[W_VACIOS],
        eq_preajuste: w[W_EQ_PREAJUSTE] as i32,
        eq_activo: w[W_EQ_ACTIVO] != 0,
        eq_bandas: core::array::from_fn(|i| w[W_EQ_BANDAS + i] as i32),
        limitador: w[W_LIMITADOR] != 0,
        limitador_actua: w[W_LIMITADOR_ACTUA],
        espacial_modo: w[W_ESPACIAL_MODO],
        espacial_cantidad: w[W_ESPACIAL_CANTIDAD],
        tema_sonidos: w[W_TEMA_SONIDOS],
        sonidos_silenciados: w[W_SONIDOS_FLAGS] & 1 != 0,
        no_molestar: w[W_SONIDOS_FLAGS] & 2 != 0,
        sonidos_volumen: w[W_SONIDOS_VOLUMEN],
        ventana: w[W_VENTANA],
        mezcla_us: w[W_MEZCLA_US],
        mezcla_peor_us: w[W_MEZCLA_PEOR_US],
        latencia_us: w[W_LATENCIA_US],
        bandas: (0..nb).map(|i| (w[W_ESPECTRO + i / 2] >> (16 * (i % 2))) as u16).collect(),
        onda: (0..ONDA).map(|i| (w[W_ONDA + i / 2] >> (16 * (i % 2))) as u16 as i16).collect(),
        nodos,
        medios,
    })
}

/// Escribe un nodo en su sitio de la tabla (`w` es la tabla entera de palabras).
pub fn escribir_nodo(w: &mut [u32], i: usize, n: &Nodo) {
    let b = &mut w[W_TABLA + i * PALABRAS_NODO..W_TABLA + (i + 1) * PALABRAS_NODO];
    b[..13].copy_from_slice(&[n.id, n.clase, n.destino, n.volumen[0], n.volumen[1], n.silencio as u32, n.pico[0], n.pico[1], n.rms[0], n.rms[1], n.dueno, n.marcas, n.periodo]);
    b[13..16].fill(0);
    texto_a_palabras(&n.nombre, &mut b[16..32]);
}

pub fn escribir_medio(w: &mut [u32], i: usize, m: &Medio) {
    let b = &mut w[W_TABLA_MEDIOS + i * PALABRAS_MEDIO..W_TABLA_MEDIOS + (i + 1) * PALABRAS_MEDIO];
    b[..6].copy_from_slice(&[m.id, m.estado, m.capacidades, m.posicion_ms, m.duracion_ms, m.flujo]);
    b[6..16].fill(0);
    texto_a_palabras(&m.titulo, &mut b[16..40]);
    texto_a_palabras(&m.artista, &mut b[40..56]);
    texto_a_palabras(&m.programa, &mut b[56..64]);
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn ida_y_vuelta() {
        let mut w = alloc::vec![0u32; TAM_ESTADO / 4];
        w[W_VERSION] = VERSION;
        w[W_NODOS] = 2;
        w[W_MEDIOS] = 1;
        w[W_BANDAS] = 3;
        w[W_ESPECTRO] = 1000 | (500 << 16);
        w[W_ESPECTRO + 1] = 7;
        let a = Nodo { id: 3, clase: CLASE_FLUJO, destino: 9, volumen: [65_536, 32_768], silencio: true, pico: [1, 2], rms: [3, 4], dueno: DUENO_AMIGA, marcas: N_DETRAS, periodo: 0, nombre: "Caja Amiga".into() };
        // Un nombre larguísimo con letras de varios bytes se corta sin partir una.
        let largo: String = core::iter::repeat("ñ").take(100).collect();
        let b = Nodo { id: 9, clase: CLASE_SALIDA, nombre: largo, ..Default::default() };
        escribir_nodo(&mut w, 0, &a);
        escribir_nodo(&mut w, 1, &b);
        let m = Medio { id: 1, estado: MED_SUENA, capacidades: 7, titulo: "Canción".into(), artista: "Nadie".into(), programa: "melodia".into(), ..Default::default() };
        escribir_medio(&mut w, 0, &m);
        let e = leer(&w).unwrap();
        assert_eq!(e.nodos[0], a);
        assert_eq!(e.nodos[1].nombre.chars().count(), 31);
        assert_eq!(e.medios[0], m);
        assert_eq!(e.bandas, [1000, 500, 7]);
        assert_eq!(e.salidas().count(), 1);
        w[W_SECUENCIA] = 1;
        assert!(leer(&w).is_none(), "a mitad de escribir");
        w[W_SECUENCIA] = 2;
        // Hostil: cuentas absurdas no salen de la tabla.
        w[W_NODOS] = u32::MAX;
        w[W_MEDIOS] = u32::MAX;
        w[W_BANDAS] = u32::MAX;
        let e = leer(&w).unwrap();
        assert_eq!((e.nodos.len(), e.medios.len(), e.bandas.len()), (MAX_NODOS, MAX_MEDIOS, MAX_BANDAS));
        assert!(leer(&w[..10]).is_none());
    }
}
