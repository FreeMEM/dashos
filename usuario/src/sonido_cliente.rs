//! Lo que usa un programa para sonar por el servicio "sonido" (S19, A4). El protocolo, en `docs/referencia/servicios.md`.
//!
//! Cada flujo es un anillo en memoria compartida: el programa escribe muestras y mueve la cabeza; el servicio las lee y
//! mueve la cola. Cabecera (palabras de 32 bits): cabeza (bytes escritos, desde el principio), cola (bytes leídos) y
//! tamaño de los datos (potencia de dos); los datos, desde el byte 64. Con el flujo va también el extremo de escritura
//! de un canal del programa: por él le despierta el servicio cuando el anillo baja de la mitad, y por él sabe el
//! servicio que el programa murió (el canal se cierra).

#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

pub const ABRIR: u32 = 10;
pub const VOLUMEN: u32 = 11;
pub const CERRAR: u32 = 12;
/// Lo que suena y las órdenes del mezclador (los mismos números que `dashos::protocolo::sonido`; aquí sin usar la
/// biblioteca, que en los tonos de prueba, sin montón, no enlazaría).
pub const MEDIOS: u32 = 30;
pub const MEDIOS_ORDEN: u32 = 31;
pub const I16: u32 = 0;
pub const I32: u32 = 1;
pub const F32: u32 = 2;
pub const DATOS: usize = 64;

pub struct Flujo {
    canal: u32,
    aviso: u32,
    va: usize,
    tam: u32,
    /// Otra copia del extremo por el que se le despierta: la que se da al servicio con `anunciar`, para que le lleguen
    /// por el mismo canal las órdenes del mezclador (reproducir, pausar...). 0 si ya se dio.
    ordenes: u32,
}

/// Lo que llega por el canal del flujo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Llega {
    /// El servicio pide más muestras.
    Muestras,
    /// Una orden del mezclador para lo que suena (`ORDEN_*` de `dashos::protocolo::sonido`).
    Orden(u32),
}

fn palabra(va: usize, i: usize) -> u32 {
    unsafe { read_volatile((va + 4 * i) as *const u32) }
}

fn poner(va: usize, i: usize, v: u32) {
    unsafe { write_volatile((va + 4 * i) as *mut u32, v) }
}

/// Abre un flujo en el servicio, por `canal`, con un anillo de `tam` bytes (potencia de dos) proyectado en `va`.
pub fn abrir(canal: u32, muestra: u32, canales: u32, frecuencia: u32, va: usize, tam: usize) -> Option<Flujo> {
    let m = dashos::memoria_crear(DATOS + tam);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
        return None;
    }
    poner(va, 0, 0);
    poner(va, 1, 0);
    poner(va, 2, tam as u32);
    let (aviso, suyo) = dashos::canal_crear();
    let ordenes = dashos::llave_duplicar(suyo, dashos::ESCRIBIR | dashos::TRANSFERIR, 0).max(0) as u32;
    let peticion: [u32; 4] = [ABRIR, muestra, canales, frecuencia];
    let bytes = unsafe { core::slice::from_raw_parts(peticion.as_ptr() as *const u8, 16) };
    // Una copia de la memoria: la proyección de aquí sigue con la llave propia.
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia <= 0 || dashos::canal_escribir(canal, bytes, &[copia as u32, suyo]) != 0 {
        return None;
    }
    Some(Flujo { canal, aviso, va, tam: tam as u32, ordenes })
}

impl Flujo {
    /// Los bytes que caben ahora en el anillo.
    pub fn libre(&self) -> usize {
        let (cabeza, cola) = (palabra(self.va, 0), palabra(self.va, 1));
        (self.tam - cabeza.wrapping_sub(cola).min(self.tam)) as usize
    }

    /// Escribe lo que quepa de `datos`; devuelve cuánto.
    pub fn escribir(&mut self, datos: &[u8]) -> usize {
        let n = datos.len().min(self.libre());
        let cabeza = palabra(self.va, 0);
        for (i, b) in datos[..n].iter().enumerate() {
            let p = (cabeza as usize + i) & (self.tam as usize - 1);
            unsafe { write_volatile((self.va + DATOS + p) as *mut u8, *b) };
        }
        fence(Ordering::SeqCst);
        poner(self.va, 0, cabeza.wrapping_add(n as u32));
        n
    }

    /// Escribe todo `datos`, esperando a que el servicio pida más cuando no quepa.
    pub fn escribir_todo(&mut self, mut datos: &[u8]) {
        while !datos.is_empty() {
            let n = self.escribir(datos);
            datos = &datos[n..];
            if !datos.is_empty() {
                self.esperar();
            }
        }
    }

    /// Duerme hasta que el servicio pida más muestras (o llegue una orden, que se pierde: quien las quiera, `esperar_algo`).
    pub fn esperar(&self) {
        let _ = self.esperar_algo();
    }

    /// Duerme hasta que el servicio pida más muestras o el mezclador mande una orden.
    pub fn esperar_algo(&self) -> Llega {
        let mut b = [0u8; 8];
        let mut ll = [0u32; 1];
        let (n, k) = dashos::canal_leer(self.aviso, &mut b, &mut ll);
        for &l in &ll[..(k as usize).min(1)] {
            dashos::cerrar(l);
        }
        let w = |i: usize| u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        if n == 8 && w(0) == MEDIOS_ORDEN {
            Llega::Orden(w(1))
        } else {
            Llega::Muestras
        }
    }

    /// Dice al servicio lo que suena (lo ve el mezclador, la barra y la pantalla de bloqueo): estado (`MED_*`), lo que
    /// se puede pedir (`PUEDE_*`), posición y duración en ms, título y artista (se cortan a 95 y 63 bytes). La primera
    /// vez va con el extremo por el que llegarán las órdenes.
    pub fn anunciar(&mut self, estado: u32, capacidades: u32, posicion_ms: u32, duracion_ms: u32, titulo: &str, artista: &str) {
        let corta = |t: &str, max: usize| -> usize {
            let mut f = t.len().min(max);
            while !t.is_char_boundary(f) {
                f -= 1;
            }
            f
        };
        let (lt, la) = (corta(titulo, 95), corta(artista, 63));
        // Sin montón (los tonos de prueba no tienen): en un búfer fijo.
        let mut b = [0u8; 28 + 95 + 63];
        for (i, w) in [MEDIOS, estado, capacidades, posicion_ms, duracion_ms, lt as u32, la as u32].iter().enumerate() {
            b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        b[28..28 + lt].copy_from_slice(&titulo.as_bytes()[..lt]);
        b[28 + lt..28 + lt + la].copy_from_slice(&artista.as_bytes()[..la]);
        let ll: &[u32] = if self.ordenes != 0 { &[self.ordenes] } else { &[] };
        if dashos::canal_escribir(self.canal, &b[..28 + lt + la], ll) == 0 {
            self.ordenes = 0;
        }
    }

    pub fn volumen(&self, izquierdo: f32, derecho: f32) {
        let p: [u32; 3] = [VOLUMEN, izquierdo.to_bits(), derecho.to_bits()];
        let bytes = unsafe { core::slice::from_raw_parts(p.as_ptr() as *const u8, 12) };
        dashos::canal_escribir(self.canal, bytes, &[]);
    }

    /// Cierra el flujo: el servicio suena lo que quede en el anillo y lo quita.
    pub fn cerrar(self) {
        dashos::canal_escribir(self.canal, &CERRAR.to_le_bytes(), &[]);
    }
}
