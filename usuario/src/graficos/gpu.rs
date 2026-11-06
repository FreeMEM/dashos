//! Búferes de la GPU (C26; desde C50, en un proceso): lo que Mesa llama BO.
//!
//! Sigue lo que Mesa espera (`docs/spec/hw/mesa-v3d.md`, 4.3 a 4.6 y 6.2): un BO tiene una VA de la GPU que nunca es
//! 0, es múltiplo de 4 KB y no cambia mientras viva; llega a cero. Una sola tabla para todos (`v3d.md`, 5.1).
//!
//! En un proceso, cada BO es un objeto de memoria DMA (páginas seguidas, a cero, sin caché) con el permiso de la GPU,
//! proyectado en el proceso en `cpu_base` más su VA de la GPU. Sin montón: como mucho `MAX_BOS` a la vez.

use super::v3d::{Mmu, V3d};

const PAGINA: u64 = 4096;
/// La VA de la GPU es de 32 bits; la página 0 no se da nunca (`v3d.md`, 6).
const VA_INICIO: u64 = PAGINA;
const VA_FIN: u64 = 1 << 32;
pub const MAX_BOS: usize = 32;
const MAX_TRAMOS: usize = MAX_BOS + 1;

#[derive(Clone, Copy)]
pub struct Bo {
    pub va: u32,
    pub paginas: u32,
    /// Física de la primera página (son seguidas).
    pub fisica: u64,
    /// Dónde se ve en este proceso.
    pub cpu: usize,
    llave: u32,
}

pub enum Tabla {
    Gpu(V3d),
    Sola(Mmu),
}

pub struct Gpu {
    tabla: Tabla,
    permiso: u32,
    cpu_base: usize,
    libres: [(u64, u64); MAX_TRAMOS], // tramos libres de VA [inicio, fin), ordenados; (0, 0) vacío
    bos: [Option<Bo>; MAX_BOS + 1],   // la manija 0 no existe
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Tamano,
    SinMemoria,
    SinVa,
    Manija,
}

impl Gpu {
    /// Con la GPU (que tiene la tabla) o, sin ella (QEMU), solo con la tabla, para probar. `permiso` es el permiso
    /// DMA de la GPU (desplazamiento 0: su dirección de bus es la física).
    pub fn nueva(tabla: Tabla, permiso: u32, cpu_base: usize) -> Self {
        let mut libres = [(0, 0); MAX_TRAMOS];
        libres[0] = (VA_INICIO, VA_FIN);
        Self { tabla, permiso, cpu_base, libres, bos: [None; MAX_BOS + 1] }
    }

    pub fn tabla(&self) -> &Mmu {
        match &self.tabla {
            Tabla::Gpu(g) => &g.mmu,
            Tabla::Sola(m) => m,
        }
    }

    pub fn hardware(&self) -> Option<&V3d> {
        match &self.tabla {
            Tabla::Gpu(g) => Some(g),
            Tabla::Sola(_) => None,
        }
    }

    fn vaciar(&self) {
        if let Some(g) = self.hardware() {
            let _ = g.vaciar_mmu();
        }
    }

    fn n_libres(&self) -> usize {
        self.libres.iter().take_while(|t| t.1 != 0).count()
    }

    fn reservar_va(&mut self, n: u64) -> Option<u64> {
        let k = self.n_libres();
        let i = self.libres[..k].iter().position(|&(a, b)| b - a >= n)?;
        let (a, b) = self.libres[i];
        if b - a == n {
            self.libres.copy_within(i + 1..k, i);
            self.libres[k - 1] = (0, 0);
        } else {
            self.libres[i] = (a + n, b);
        }
        Some(a)
    }

    fn devolver_va(&mut self, a: u64, n: u64) {
        let k = self.n_libres();
        let i = self.libres[..k].iter().position(|&(x, _)| x > a).unwrap_or(k);
        self.libres.copy_within(i..k, i + 1);
        self.libres[i] = (a, a + n);
        let mut k = k + 1;
        // Juntar con los vecinos.
        if i + 1 < k && self.libres[i].1 == self.libres[i + 1].0 {
            self.libres[i].1 = self.libres[i + 1].1;
            self.libres.copy_within(i + 2..k, i + 1);
            k -= 1;
            self.libres[k] = (0, 0);
        }
        if i > 0 && self.libres[i - 1].1 == self.libres[i].0 {
            self.libres[i - 1].1 = self.libres[i].1;
            self.libres.copy_within(i + 1..k, i);
            k -= 1;
            self.libres[k] = (0, 0);
        }
    }

    /// `CREATE_BO`: devuelve (manija, VA). La manija 0 no existe.
    pub fn crear(&mut self, tamano: u32) -> Result<(u32, u32), Error> {
        if tamano == 0 {
            return Err(Error::Tamano);
        }
        let h = (1..=MAX_BOS).find(|&i| self.bos[i].is_none()).ok_or(Error::SinMemoria)?;
        // Hace falta un tramo libre de más por si al liberar queda un hueco.
        if self.n_libres() >= MAX_TRAMOS - 1 {
            return Err(Error::SinVa);
        }
        let n = (tamano as u64).div_ceil(PAGINA);
        let (llave, fisica) = crate::memoria_dma(self.permiso, (n * PAGINA) as usize);
        if llave <= 0 {
            return Err(Error::SinMemoria);
        }
        let Some(va) = self.reservar_va(n * PAGINA) else {
            crate::cerrar(llave as u32);
            return Err(Error::SinVa);
        };
        let cpu = self.cpu_base + va as usize;
        if crate::memoria_proyectar(llave as u32, cpu, true) != 0 {
            crate::cerrar(llave as u32);
            self.devolver_va(va, n * PAGINA);
            return Err(Error::SinMemoria);
        }
        self.tabla().mapear(va as u32, (0..n).map(|i| fisica + i * PAGINA));
        self.vaciar();
        self.bos[h] = Some(Bo { va: va as u32, paginas: n as u32, fisica, cpu, llave: llave as u32 });
        Ok((h as u32, va as u32))
    }

    /// `GET_BO_OFFSET`.
    pub fn va(&self, h: u32) -> Option<u32> {
        self.bo(h).map(|b| b.va)
    }

    pub fn bo(&self, h: u32) -> Option<&Bo> {
        if h == 0 { None } else { self.bos.get(h as usize)?.as_ref() }
    }

    /// Libera un BO: quita sus entradas, vacía la MMU y devuelve su memoria y su VA.
    pub fn liberar(&mut self, h: u32) -> Result<(), Error> {
        if h == 0 {
            return Err(Error::Manija);
        }
        let bo = self.bos.get_mut(h as usize).and_then(|b| b.take()).ok_or(Error::Manija)?;
        self.tabla().quitar(bo.va, bo.paginas as usize);
        self.vaciar();
        crate::memoria_quitar(bo.cpu);
        crate::cerrar(bo.llave);
        self.devolver_va(bo.va as u64, bo.paginas as u64 * PAGINA);
        Ok(())
    }

    pub fn tramos_libres(&self) -> usize {
        self.n_libres()
    }
}
