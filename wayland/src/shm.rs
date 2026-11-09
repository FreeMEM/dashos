//! Memoria compartida (W4): `wl_shm`, `wl_shm_pool` y `wl_buffer` del lado del compositor.
//!
//! El cliente crea un objeto de memoria, lo pinta y manda su llave en `create_pool`. El compositor la proyecta solo
//! para leer (`Memorias`) y la mira cuando compone. Un `wl_buffer` es un trozo del pool: desplazamiento, ancho, alto,
//! bytes por línea y formato. Las reglas, de `wayland.xml`: el pool solo crece; destruirlo no quita la memoria a los
//! búferes que ya la usan (se suelta con el último); un búfer que no cabe en el pool o con un formato que no se
//! anunció es un error de `wl_shm`.

use alloc::collections::BTreeMap;

use crate::servidor::ErrorProtocolo;

/// Formatos (de `wl_shm.format`): los dos que todo compositor anuncia.
pub const ARGB8888: u32 = 0;
pub const XRGB8888: u32 = 1;

/// Errores de `wl_shm`.
pub const INVALID_FORMAT: u32 = 0;
pub const INVALID_STRIDE: u32 = 1;
pub const INVALID_FD: u32 = 2;

/// Lo más grande que se acepta de un pool: 64 MB (una pantalla 4K a 32 bits, dos veces, cabe).
pub const MAX_POOL: usize = 64 << 20;

/// Quien proyecta los objetos de memoria: en DashOS, `memoria_proyectar` solo para leer; en las pruebas, vectores.
pub trait Memorias {
    /// Proyecta `tamano` bytes del objeto de memoria `llave`; da dónde empiezan. Si ya estaba proyectado (un pool que
    /// crece), quita la proyección vieja.
    fn proyectar(&mut self, llave: u32, tamano: usize) -> Option<*const u8>;
    /// Quita la proyección y suelta la llave.
    fn soltar(&mut self, llave: u32, base: *const u8, tamano: usize);
    /// Una llave de memoria vacía, de solo lectura, para dar a un cliente (el mapa de teclas sin mapa, W8).
    fn llave_vacia(&mut self) -> Option<u32> {
        None
    }
}

struct Pool {
    llave: u32,
    base: *const u8,
    tamano: usize,
    /// Búferes vivos que lo usan.
    buferes: u32,
    /// El cliente ya lo destruyó: se suelta con el último búfer.
    destruido: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bufer {
    pub pool: u32,
    pub desplazamiento: usize,
    pub ancho: u32,
    pub alto: u32,
    pub linea: usize,
    pub formato: u32,
}

/// Los pools y búferes de una conexión, por número de objeto.
#[derive(Default)]
pub struct Shm {
    pools: BTreeMap<u32, Pool>,
    pub buferes: BTreeMap<u32, Bufer>,
}

impl Shm {
    pub fn crear_pool(
        &mut self,
        memorias: &mut impl Memorias,
        shm: u32,
        id: u32,
        llave: u32,
        tamano: i32,
    ) -> Result<(), ErrorProtocolo> {
        if tamano <= 0 || tamano as usize > MAX_POOL {
            return Err(ErrorProtocolo::new(shm, INVALID_STRIDE, "tamaño de pool malo"));
        }
        let tamano = tamano as usize;
        let Some(base) = memorias.proyectar(llave, tamano) else {
            return Err(ErrorProtocolo::new(shm, INVALID_FD, "memoria que no se puede proyectar"));
        };
        self.pools.insert(id, Pool { llave, base, tamano, buferes: 0, destruido: false });
        Ok(())
    }

    pub fn crecer_pool(&mut self, memorias: &mut impl Memorias, id: u32, tamano: i32) -> Result<(), ErrorProtocolo> {
        let pool = self.pools.get_mut(&id).ok_or(ErrorProtocolo::new(id, INVALID_FD, "pool"))?;
        if tamano < pool.tamano as i32 || tamano as usize > MAX_POOL {
            return Err(ErrorProtocolo::new(id, INVALID_STRIDE, "un pool no encoge"));
        }
        // Se proyecta otra vez con el tamaño nuevo: el objeto de memoria es el mismo.
        let Some(base) = memorias.proyectar(pool.llave, tamano as usize) else {
            return Err(ErrorProtocolo::new(id, INVALID_FD, "memoria que no crece"));
        };
        pool.base = base;
        pool.tamano = tamano as usize;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn crear_bufer(
        &mut self,
        pool: u32,
        id: u32,
        desplazamiento: i32,
        ancho: i32,
        alto: i32,
        linea: i32,
        formato: u32,
    ) -> Result<(), ErrorProtocolo> {
        let p = self.pools.get_mut(&pool).ok_or(ErrorProtocolo::new(pool, INVALID_FD, "pool"))?;
        if formato != ARGB8888 && formato != XRGB8888 {
            return Err(ErrorProtocolo::new(pool, INVALID_FORMAT, "formato que no se anunció"));
        }
        let fin = (desplazamiento as i64) + (linea as i64) * (alto as i64);
        if desplazamiento < 0
            || ancho <= 0
            || alto <= 0
            || (linea as i64) < (ancho as i64) * 4
            || fin > p.tamano as i64
        {
            return Err(ErrorProtocolo::new(pool, INVALID_STRIDE, "búfer que no cabe en el pool"));
        }
        p.buferes += 1;
        self.buferes.insert(
            id,
            Bufer {
                pool,
                desplazamiento: desplazamiento as usize,
                ancho: ancho as u32,
                alto: alto as u32,
                linea: linea as usize,
                formato,
            },
        );
        Ok(())
    }

    /// El cliente destruyó un pool: se suelta ya si no lo usa ningún búfer.
    pub fn destruir_pool(&mut self, memorias: &mut impl Memorias, id: u32) {
        if let Some(p) = self.pools.get_mut(&id) {
            p.destruido = true;
        }
        self.soltar_si_toca(memorias, id);
    }

    pub fn destruir_bufer(&mut self, memorias: &mut impl Memorias, id: u32) {
        if let Some(b) = self.buferes.remove(&id) {
            if let Some(p) = self.pools.get_mut(&b.pool) {
                p.buferes -= 1;
            }
            self.soltar_si_toca(memorias, b.pool);
        }
    }

    fn soltar_si_toca(&mut self, memorias: &mut impl Memorias, id: u32) {
        if self.pools.get(&id).is_some_and(|p| p.destruido && p.buferes == 0) {
            let p = self.pools.remove(&id).unwrap();
            memorias.soltar(p.llave, p.base, p.tamano);
        }
    }

    /// Suelta todo (la conexión se cierra).
    pub fn soltar_todo(&mut self, memorias: &mut impl Memorias) {
        self.buferes.clear();
        for (_, p) in core::mem::take(&mut self.pools) {
            memorias.soltar(p.llave, p.base, p.tamano);
        }
    }

    /// Los píxeles de un búfer: sus bytes desde la primera línea, y su descripción. Lo que el cliente cambie mientras
    /// se lee sale a medias en pantalla, nada más: son bytes.
    pub fn pixeles(&self, id: u32) -> Option<(&[u8], Bufer)> {
        let b = *self.buferes.get(&id)?;
        let p = self.pools.get(&b.pool)?;
        let largo = b.linea * (b.alto as usize - 1) + b.ancho as usize * 4;
        if b.desplazamiento + largo > p.tamano {
            return None;
        }
        // La proyección vive mientras el pool esté en la tabla, y crear_bufer comprobó que el búfer cabe.
        let bytes = unsafe { core::slice::from_raw_parts(p.base.add(b.desplazamiento), largo) };
        Some((bytes, b))
    }
}
