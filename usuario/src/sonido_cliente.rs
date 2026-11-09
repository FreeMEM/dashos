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
pub const I16: u32 = 0;
pub const I32: u32 = 1;
pub const F32: u32 = 2;
pub const DATOS: usize = 64;

pub struct Flujo {
    canal: u32,
    aviso: u32,
    va: usize,
    tam: u32,
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
    let peticion: [u32; 4] = [ABRIR, muestra, canales, frecuencia];
    let bytes = unsafe { core::slice::from_raw_parts(peticion.as_ptr() as *const u8, 16) };
    // Una copia de la memoria: la proyección de aquí sigue con la llave propia.
    let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if copia <= 0 || dashos::canal_escribir(canal, bytes, &[copia as u32, suyo]) != 0 {
        return None;
    }
    Some(Flujo { canal, aviso, va, tam: tam as u32 })
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

    /// Duerme hasta que el servicio pida más muestras.
    pub fn esperar(&self) {
        let mut b = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(self.aviso, &mut b, &mut ll);
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
