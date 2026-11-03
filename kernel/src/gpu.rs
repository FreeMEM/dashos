//! Búferes de la GPU (C26): lo que Mesa llama BO.
//!
//! Sigue lo que Mesa espera del kernel (`docs/spec/hw/mesa-v3d.md`, 4.3 a 4.6 y 6.2): un BO tiene una VA de la GPU
//! que nunca es 0, es múltiplo de 4 KB y no cambia mientras viva; llega a cero; sus páginas no tienen que ser
//! contiguas, porque las junta la MMU de la GPU (`v3d.md`, 2.6). Una sola tabla para todos (`v3d.md`, 5.1).
//!
//! Provisional: vive en el kernel y se usa desde las pruebas. El servidor de la GPU (S9) lo dará por llaves.

use crate::memoria::{paginas, Zona, PAGINA};
use crate::v3d::{Mmu, V3d};
use alloc::vec::Vec;

/// La VA de la GPU es de 32 bits; la página 0 no se da nunca (`v3d.md`, 6).
const VA_INICIO: u64 = PAGINA;
const VA_FIN: u64 = 1 << 32;

pub struct Bo {
    pub va: u32,
    pub paginas: Vec<u64>,
}

pub struct Gpu {
    /// Con hardware, la GPU (que tiene la tabla). Sin él (QEMU), solo la tabla, para probar.
    v3d: Option<V3d>,
    sola: Option<Mmu>,
    libres: Vec<(u64, u64)>, // tramos libres de VA [inicio, fin), ordenados
    bos: Vec<Option<Bo>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Tamano,
    SinMemoria,
    SinVa,
    Manija,
}

impl Gpu {
    pub fn con_hardware(v3d: V3d) -> Self {
        Self { v3d: Some(v3d), sola: None, libres: alloc::vec![(VA_INICIO, VA_FIN)], bos: Vec::new() }
    }

    pub fn sin_hardware() -> Result<Self, Error> {
        let mmu = Mmu::nueva().map_err(|_| Error::SinMemoria)?;
        Ok(Self { v3d: None, sola: Some(mmu), libres: alloc::vec![(VA_INICIO, VA_FIN)], bos: Vec::new() })
    }

    pub fn tabla(&self) -> &Mmu {
        match &self.v3d {
            Some(g) => &g.mmu,
            None => self.sola.as_ref().unwrap(),
        }
    }

    pub fn hardware(&self) -> Option<&V3d> {
        self.v3d.as_ref()
    }

    fn vaciar(&self) {
        if let Some(g) = &self.v3d {
            let _ = g.vaciar_mmu();
        }
    }

    fn reservar_va(&mut self, n: u64) -> Option<u64> {
        let i = self.libres.iter().position(|&(a, b)| b - a >= n)?;
        let (a, b) = self.libres[i];
        if b - a == n {
            self.libres.remove(i);
        } else {
            self.libres[i] = (a + n, b);
        }
        Some(a)
    }

    fn devolver_va(&mut self, a: u64, n: u64) {
        let i = self.libres.iter().position(|&(x, _)| x > a).unwrap_or(self.libres.len());
        self.libres.insert(i, (a, a + n));
        // Juntar con los vecinos.
        if i + 1 < self.libres.len() && self.libres[i].1 == self.libres[i + 1].0 {
            self.libres[i].1 = self.libres[i + 1].1;
            self.libres.remove(i + 1);
        }
        if i > 0 && self.libres[i - 1].1 == self.libres[i].0 {
            self.libres[i - 1].1 = self.libres[i].1;
            self.libres.remove(i);
        }
    }

    /// `CREATE_BO`: devuelve (manija, VA). La manija 0 no existe.
    pub fn crear(&mut self, tamano: u32) -> Result<(u32, u32), Error> {
        if tamano == 0 {
            return Err(Error::Tamano);
        }
        let n = (tamano as u64).div_ceil(PAGINA);
        let mut pags = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let pagina = paginas().pagina(Zona::Normal);
            match pagina {
                Some(p) => {
                    let va = crate::mmu::virt(p as usize);
                    unsafe { core::ptr::write_bytes(va as *mut u8, 0, PAGINA as usize) };
                    crate::hvs::limpiar_cache(va, PAGINA as usize);
                    pags.push(p);
                }
                None => {
                    for &p in &pags {
                        unsafe { paginas().liberar(p, 1) };
                    }
                    return Err(Error::SinMemoria);
                }
            }
        }
        let Some(va) = self.reservar_va(n * PAGINA) else {
            for &p in &pags {
                unsafe { paginas().liberar(p, 1) };
            }
            return Err(Error::SinVa);
        };
        self.tabla().mapear(va as u32, &pags);
        self.vaciar();
        let bo = Bo { va: va as u32, paginas: pags };
        if self.bos.is_empty() {
            self.bos.push(None);
        }
        let h = match self.bos.iter().skip(1).position(|b| b.is_none()) {
            Some(i) => {
                self.bos[i + 1] = Some(bo);
                i + 1
            }
            None => {
                self.bos.push(Some(bo));
                self.bos.len() - 1
            }
        };
        Ok((h as u32, va as u32))
    }

    /// `GET_BO_OFFSET`.
    pub fn va(&self, h: u32) -> Option<u32> {
        self.bos.get(h as usize)?.as_ref().map(|b| b.va)
    }

    pub fn bo(&self, h: u32) -> Option<&Bo> {
        self.bos.get(h as usize)?.as_ref()
    }

    /// Libera un BO: quita sus entradas, vacía la MMU y devuelve páginas y VA.
    pub fn liberar(&mut self, h: u32) -> Result<(), Error> {
        let bo = self.bos.get_mut(h as usize).and_then(|b| b.take()).ok_or(Error::Manija)?;
        let n = bo.paginas.len();
        self.tabla().quitar(bo.va, n);
        self.vaciar();
        for &p in &bo.paginas {
            unsafe { paginas().liberar(p, 1) };
        }
        self.devolver_va(bo.va as u64, n as u64 * PAGINA);
        Ok(())
    }

    pub fn tramos_libres(&self) -> usize {
        self.libres.len()
    }
}
