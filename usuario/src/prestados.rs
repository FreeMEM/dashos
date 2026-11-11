//! A7 (docs/REVISION_ARQUITECTURA.md): leer ficheros con las páginas que presta el servicio de archivos (`PRESTAR`).
//! El servicio da trozos de su caché, solo de leer; aquí se proyectan y se lee de ellos sin viaje mientras duren. Un
//! viaje trae hasta `POR_PRESTAMO` trozos (2 MB).
//!
//! Se guardan como mucho `MAX_PRESTADOS - POR_PRESTAMO` antes de pedir más (se sueltan los más viejos): así el servicio
//! nunca quita uno que se esté usando (ver `PRESTAR` en `protocolo::archivos`). Antes de cerrar una asa, `cerrar_asa`.

use crate::protocolo::archivos::{BIEN, CABECERA, MAX_PRESTADOS, POR_PRESTAMO, PRESTAR, TROZO_PRESTADO};

#[derive(Clone, Copy)]
struct Trozo {
    asa: u32,
    desde: u64,
    llave: u32,
}

/// Las asas de las que se sabe lo que mide el fichero, como mucho.
const MAX_ASAS: usize = 8;

pub struct Prestados {
    /// Donde se proyectan: `MAX_PRESTADOS` huecos de `TROZO_PRESTADO`, libres en el espacio del programa.
    va: usize,
    huecos: [Option<Trozo>; MAX_PRESTADOS],
    /// Los huecos ocupados, en el orden en que llegaron (el primero, el más viejo), como lo cuenta el servicio.
    orden: [u8; MAX_PRESTADOS],
    n: usize,
    /// (asa, lo que mide su fichero) según la última respuesta.
    tam: [(u32, u64); MAX_ASAS],
    /// Viajes hechos (para medir).
    pub viajes: u64,
}

impl Prestados {
    /// `va`: el principio de `MAX_PRESTADOS * TROZO_PRESTADO` bytes libres (alineado a 4 KB).
    pub const fn nuevo(va: usize) -> Self {
        Self { va, huecos: [None; MAX_PRESTADOS], orden: [0; MAX_PRESTADOS], n: 0, tam: [(u32::MAX, 0); MAX_ASAS], viajes: 0 }
    }

    fn soltar_hueco(&mut self, h: usize) {
        if let Some(t) = self.huecos[h].take() {
            crate::memoria_quitar(self.va + h * TROZO_PRESTADO);
            crate::cerrar(t.llave);
            if let Some(i) = self.orden[..self.n].iter().position(|&x| x as usize == h) {
                self.orden.copy_within(i + 1..self.n, i);
                self.n -= 1;
            }
        }
    }

    /// Suelta lo prestado de `asa` (antes de cerrarla).
    pub fn cerrar_asa(&mut self, asa: u32) {
        for h in 0..MAX_PRESTADOS {
            if self.huecos[h].is_some_and(|t| t.asa == asa) {
                self.soltar_hueco(h);
            }
        }
        for t in self.tam.iter_mut() {
            if t.0 == asa {
                *t = (u32::MAX, 0);
            }
        }
    }

    fn tam_de(&self, asa: u32) -> Option<u64> {
        self.tam.iter().find(|t| t.0 == asa).map(|t| t.1)
    }

    fn poner_tam(&mut self, asa: u32, tam: u64) {
        let i = self.tam.iter().position(|t| t.0 == asa).or_else(|| self.tam.iter().position(|t| t.0 == u32::MAX)).unwrap_or(0);
        self.tam[i] = (asa, tam);
    }

    /// Pide los trozos desde `desde`. Falso si el servicio no presta (entonces, `LEER`).
    fn pedir(&mut self, peticion: u32, respuesta: u32, asa: u32, desde: u64) -> bool {
        while self.n + POR_PRESTAMO > MAX_PRESTADOS {
            self.soltar_hueco(self.orden[0] as usize);
        }
        let mut p = [0u8; CABECERA];
        p[0..4].copy_from_slice(&PRESTAR.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&desde.to_le_bytes());
        if crate::canal_escribir(peticion, &p, &[]) != 0 {
            return false;
        }
        self.viajes += 1;
        let mut b = [0u8; CABECERA];
        let mut ll = [0u32; POR_PRESTAMO];
        let (n, k) = crate::canal_leer(respuesta, &mut b, &mut ll);
        let estado = i32::from_le_bytes(b[0..4].try_into().unwrap());
        if n != CABECERA as i64 || estado != BIEN {
            for &l in &ll[..k] {
                crate::cerrar(l);
            }
            return false;
        }
        let primero = u64::from_le_bytes(b[8..16].try_into().unwrap());
        self.poner_tam(asa, u64::from_le_bytes(b[16..24].try_into().unwrap()));
        for (i, &llave) in ll[..k].iter().enumerate() {
            let d = primero + (i * TROZO_PRESTADO) as u64;
            // Si ya tenía ese trozo (de otro préstamo), fuera el viejo.
            if let Some(h) = (0..MAX_PRESTADOS).find(|&h| self.huecos[h].is_some_and(|t| t.asa == asa && t.desde == d)) {
                self.soltar_hueco(h);
            }
            let Some(h) = (0..MAX_PRESTADOS).find(|&h| self.huecos[h].is_none()) else {
                crate::cerrar(llave);
                continue;
            };
            if crate::memoria_proyectar(llave, self.va + h * TROZO_PRESTADO, false) != 0 {
                crate::cerrar(llave);
                continue;
            }
            self.huecos[h] = Some(Trozo { asa, desde: d, llave });
            self.orden[self.n] = h as u8;
            self.n += 1;
        }
        true
    }

    /// Lee de `asa`, desde `desde`, lo que quepa en `destino` (menos al final del fichero). `peticion` es la llave
    /// del canal del servicio y `respuesta` la de leer sus respuestas. `None` si el servicio no presta este fichero:
    /// entonces, con `LEER`.
    pub fn leer(&mut self, peticion: u32, respuesta: u32, asa: u32, desde: u64, destino: &mut [u8]) -> Option<usize> {
        let mut hecho = 0;
        let mut pedido = false;
        while hecho < destino.len() {
            let off = desde + hecho as u64;
            let tam = self.tam_de(asa);
            let trozo = (0..MAX_PRESTADOS).find(|&h| self.huecos[h].is_some_and(|t| t.asa == asa && t.desde <= off && off < t.desde + TROZO_PRESTADO as u64));
            match (tam, trozo) {
                (Some(t), Some(h)) if off < t => {
                    let d = self.huecos[h].unwrap().desde;
                    let dentro = (off - d) as usize;
                    let cuanto = (destino.len() - hecho).min(TROZO_PRESTADO - dentro).min((t - off) as usize);
                    // El trozo está proyectado aquí, solo de leer, mientras siga en `huecos`.
                    let origen = unsafe { core::slice::from_raw_parts((self.va + h * TROZO_PRESTADO + dentro) as *const u8, cuanto) };
                    destino[hecho..hecho + cuanto].copy_from_slice(origen);
                    hecho += cuanto;
                    pedido = false;
                }
                // Al final según lo último que se supo, o sin el trozo: se pregunta (una vez por trozo).
                _ if !pedido => {
                    if !self.pedir(peticion, respuesta, asa, off) {
                        return if hecho == 0 { None } else { Some(hecho) };
                    }
                    pedido = true;
                    if self.tam_de(asa).is_some_and(|t| off >= t) {
                        break;
                    }
                }
                _ => break,
            }
        }
        Some(hecho)
    }
}
