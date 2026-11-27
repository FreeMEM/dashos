//! Lo que un programa deja cambiado en el sistema de la caja al acabar, deshecho (S28, `docs/diseño/S28-malware-amiga.md`).
//!
//! En el Amiga, un virus se quedaba en memoria parcheando las tablas de saltos de exec y dos (`SetFunction` de
//! `DoIO`, `OpenLibrary`, `LoadSeg`...: así se contagiaba a cada programa y a cada disco) y los vectores de reinicio
//! de ExecBase (`ColdCapture`, `CoolCapture`, `KickTagPtr`...: así volvía tras un reinicio en caliente). En la caja,
//! los parches de un programa son suyos: cuando acaba, se deshacen.
//!
//! Al arrancar se guarda una foto de las tablas de saltos de todas las bibliotecas y dispositivos de exec (las
//! nativas: exec, dos, graphics, intuition, utility, timer.device) y de los campos de ExecBase que tocaban los virus.
//! Al acabar cada programa se comparan y lo cambiado vuelve a la foto (con `SumLibrary`), se haya cambiado con
//! `SetFunction` o escribiendo a mano en la tabla. El JIT olvida lo traducido antes del programa siguiente.
//!
//! Hoy corre una tarea 68k a la vez. Con varias, esto desharía también los parches de otra que siga viva: entonces
//! hará falta apuntar en exec quién hizo cada `SetFunction` (S28, "lo que falta").

use alloc::vec::Vec;

use caja_exec::estructuras::{execbase, library};
use caja_exec::{Exec, Memoria};

use crate::memoria::MemoriaCaja;

/// Los campos de ExecBase que se miran: los vectores de reinicio, los de interrupción, los módulos residentes y las
/// funciones por defecto de las tareas. (desde, largo) en bytes.
const CAMPOS_EXEC: &[(u32, u32, &str)] = &[
    (execbase::COLDCAPTURE, 12, "ColdCapture, CoolCapture y WarmCapture"),
    (execbase::INTVECTS, 16 * 12, "IntVects"),
    (execbase::RESMODULES, 4, "ResModules"),
    (execbase::TASKTRAPCODE, 12, "TaskTrapCode, TaskExceptCode y TaskExitCode"),
    (execbase::KICKMEMPTR, 12, "KickMemPtr, KickTagPtr y KickCheckSum"),
];
/// Tope de bibliotecas y dispositivos que se fotografían.
const MAX_BIBLIOTECAS: usize = 64;

/// La foto del sistema de la caja recién montado.
pub struct Foto {
    /// Base de cada biblioteca y su tabla de saltos, `[base - lib_NegSize, base)`.
    tablas: Vec<(u32, Vec<u8>)>,
    /// Los campos de ExecBase.
    exec: Vec<(u32, Vec<u8>)>,
}

/// Lo que se deshizo tras un programa.
#[derive(Clone, Copy, Default)]
pub struct Deshecho {
    /// Entradas de tablas de saltos (de 6 bytes) que había cambiado.
    pub entradas: u32,
    /// Grupos de campos de ExecBase que había cambiado (de `CAMPOS_EXEC`).
    pub campos: u32,
}

impl Foto {
    pub fn hacer(ex: &Exec<MemoriaCaja>) -> Foto {
        let mut tablas = Vec::new();
        let base = ex.base();
        let mut libs = Vec::from([base]);
        for l in [execbase::LIBLIST, execbase::DEVICELIST] {
            libs.extend(ex.nodos(base + l).unwrap_or_default());
        }
        libs.dedup();
        for lib in libs.into_iter().take(MAX_BIBLIOTECAS) {
            let Ok(neg) = ex.mem.leer16(lib + library::LIB_NEGSIZE) else { continue };
            let desde = lib.wrapping_sub(neg as u32);
            if let Ok(t) = ex.mem.leer(desde, neg as u32)
                && !tablas.iter().any(|(b, _)| *b == lib)
            {
                tablas.push((lib, t.to_vec()));
            }
        }
        let exec = CAMPOS_EXEC
            .iter()
            .filter_map(|&(d, n, _)| ex.mem.leer(base + d, n).ok().map(|v| (base + d, v.to_vec())))
            .collect();
        Foto { tablas, exec }
    }

    /// Cuántas bibliotecas (y dispositivos) tiene la foto.
    pub fn bibliotecas(&self) -> usize {
        self.tablas.len()
    }

    /// Compara con la foto y deshace lo cambiado. Devuelve lo deshecho y los nombres de los campos de ExecBase.
    pub fn deshacer(&self, ex: &mut Exec<MemoriaCaja>) -> (Deshecho, Vec<&'static str>) {
        let mut d = Deshecho::default();
        let mut nombres = Vec::new();
        for (lib, foto) in &self.tablas {
            let desde = lib.wrapping_sub(foto.len() as u32);
            let Ok(ahora) = ex.mem.leer(desde, foto.len() as u32) else { continue };
            if ahora == foto.as_slice() {
                continue;
            }
            // Las entradas van de 6 en 6 hacia abajo desde la base; lo de antes de la primera es relleno.
            let n = foto.len() / 6;
            let fin = foto.len();
            d.entradas += (1..=n).filter(|k| ahora[fin - 6 * k..fin - 6 * k + 6] != foto[fin - 6 * k..fin - 6 * k + 6]).count()
                as u32;
            if let Ok(m) = ex.mem.leer_mut(desde, foto.len() as u32) {
                m.copy_from_slice(foto);
            }
            let _ = ex.sum_library(*lib);
        }
        for (i, (dir, foto)) in self.exec.iter().enumerate() {
            let Ok(ahora) = ex.mem.leer(*dir, foto.len() as u32) else { continue };
            if ahora != foto.as_slice() {
                d.campos += 1;
                nombres.push(CAMPOS_EXEC[i].2);
                if let Ok(m) = ex.mem.leer_mut(*dir, foto.len() as u32) {
                    m.copy_from_slice(foto);
                }
            }
        }
        (d, nombres)
    }
}
