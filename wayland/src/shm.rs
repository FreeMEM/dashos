//! Memoria compartida (W4): `wl_shm`, `wl_shm_pool` y `wl_buffer` del lado del compositor.
//!
//! El cliente crea un objeto de memoria, lo pinta y manda su llave en `create_pool`. El compositor la proyecta solo
//! para leer (`Memorias`) y la mira cuando compone. Un `wl_buffer` es un trozo del pool: desplazamiento, ancho, alto,
//! bytes por línea y formato. Las reglas, de `wayland.xml`: el pool solo crece; destruirlo no quita la memoria a los
//! búferes que ya la usan (se suelta con el último); un búfer que no cabe en el pool o con un formato que no se
//! anunció es un error de `wl_shm`.

use alloc::collections::BTreeMap;

use crate::servidor::{ErrorProtocolo, NO_MEMORY};

/// Formatos (de `wl_shm.format`): los dos que todo compositor anuncia.
pub const ARGB8888: u32 = 0;
pub const XRGB8888: u32 = 1;

/// Errores de `wl_shm`.
pub const INVALID_FORMAT: u32 = 0;
pub const INVALID_STRIDE: u32 = 1;
pub const INVALID_FD: u32 = 2;

/// Lo más grande que se acepta de un pool: 64 MB (una pantalla 4K a 32 bits, dos veces, cabe).
pub const MAX_POOL: usize = 64 << 20;
/// Lo más de pools de un cliente (E3): cada uno es una proyección en el compositor.
pub const MAX_POOLS: usize = 16;

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

    /// Una llave de solo lectura de una memoria con `datos`, para dar a un cliente (el mapa de teclado, G2). La
    /// memoria se puede crear una vez y dar copias. `None` si no se puede: entonces va sin mapa.
    fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
        None
    }

    /// Antes de leer un búfer de dmabuf tras su commit (G6): en Linux, esperar a que la tarjeta acabe de pintarlo.
    fn antes_de_leer(&mut self, _llave: u32) {}
}

struct Pool {
    llave: u32,
    base: *const u8,
    tamano: usize,
    /// Búferes vivos que lo usan.
    buferes: u32,
    /// El cliente ya lo destruyó: se suelta con el último búfer.
    destruido: bool,
    /// Es la memoria de un búfer de dmabuf (G6), no un pool del cliente.
    dmabuf: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bufer {
    pub pool: u32,
    pub desplazamiento: usize,
    pub ancho: u32,
    pub alto: u32,
    pub linea: usize,
    pub formato: u32,
}

/// Desde aquí, los números privados de lo que el cliente destruyó y aún se usa (por debajo de los del servidor).
pub const PRIMERO_PRIVADO: u32 = 0xFE00_0000;

/// Los pools y búferes de una conexión, por número de objeto.
#[derive(Default)]
pub struct Shm {
    pools: BTreeMap<u32, Pool>,
    pub buferes: BTreeMap<u32, Bufer>,
    siguiente_privado: u32,
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
        if self.pools.len() >= MAX_POOLS {
            return Err(ErrorProtocolo::new(1, NO_MEMORY, "demasiados pools"));
        }
        let tamano = tamano as usize;
        let Some(base) = memorias.proyectar(llave, tamano) else {
            return Err(ErrorProtocolo::new(shm, INVALID_FD, "memoria que no se puede proyectar"));
        };
        self.pools.insert(id, Pool { llave, base, tamano, buferes: 0, destruido: false, dmabuf: false });
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

    /// Un búfer de dmabuf (G6): un plano lineal de una memoria. Va como un pool con un solo búfer, que se suelta con
    /// él. `formato`, ya en el de `wl_shm`. Falso si la memoria no se puede proyectar o no mide eso (importación
    /// fallida: lo decide quien llama).
    #[allow(clippy::too_many_arguments)]
    pub fn crear_dmabuf(
        &mut self,
        memorias: &mut impl Memorias,
        id: u32,
        llave: u32,
        desplazamiento: u32,
        ancho: i32,
        alto: i32,
        linea: u32,
        formato: u32,
    ) -> bool {
        let tamano = desplazamiento as u64 + linea as u64 * alto as u64;
        if tamano as usize > MAX_POOL || self.pools.len() >= MAX_POOLS * 4 {
            memorias.soltar(llave, core::ptr::null(), 0);
            return false;
        }
        let Some(base) = memorias.proyectar(llave, tamano as usize) else {
            memorias.soltar(llave, core::ptr::null(), 0);
            return false;
        };
        let k = self.privado();
        self.pools.insert(k, Pool { llave, base, tamano: tamano as usize, buferes: 1, destruido: true, dmabuf: true });
        self.buferes.insert(
            id,
            Bufer {
                pool: k,
                desplazamiento: desplazamiento as usize,
                ancho: ancho as u32,
                alto: alto as u32,
                linea: linea as usize,
                formato,
            },
        );
        true
    }

    /// La llave de un búfer si es de dmabuf (para esperar a la tarjeta antes de leerlo).
    pub fn llave_dmabuf(&self, id: u32) -> Option<u32> {
        let p = self.pools.get(&self.buferes.get(&id)?.pool)?;
        p.dmabuf.then_some(p.llave)
    }

    /// El cliente destruyó un pool: se suelta ya si no lo usa ningún búfer.
    pub fn destruir_pool(&mut self, memorias: &mut impl Memorias, id: u32) {
        // Si aún tiene búferes, sigue vivo con un número privado: el cliente puede usar el suyo para otro pool (G4,
        // swaybg lo hace).
        if self.pools.get(&id).is_some_and(|p| p.buferes > 0 && !p.dmabuf) {
            let mut p = self.pools.remove(&id).unwrap();
            p.destruido = true;
            let k = self.privado();
            self.pools.insert(k, p);
            for b in self.buferes.values_mut().filter(|b| b.pool == id) {
                b.pool = k;
            }
            return;
        }
        if let Some(p) = self.pools.get_mut(&id).filter(|p| !p.dmabuf) {
            p.destruido = true;
        }
        self.soltar_si_toca(memorias, id);
    }

    /// Un número que ningún cliente usa, para lo que sigue vivo después de que el cliente lo destruyó.
    fn privado(&mut self) -> u32 {
        self.siguiente_privado = self.siguiente_privado.max(PRIMERO_PRIVADO).wrapping_add(1);
        self.siguiente_privado
    }

    /// Un búfer que el cliente destruye mientras se ve: pasa a un número privado y sigue viéndose hasta que la
    /// superficie ponga otro (como wlroots; swaybg destruye el suyo tras el commit). Devuelve su número nuevo.
    pub fn apartar_bufer(&mut self, id: u32) -> Option<u32> {
        let b = self.buferes.remove(&id)?;
        let k = self.privado();
        self.buferes.insert(k, b);
        Some(k)
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
