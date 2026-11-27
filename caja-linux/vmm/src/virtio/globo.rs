//! virtio-balloon, el globo de memoria tradicional (virtio 1.2 §5.5): DashOS pide al invitado que le devuelva páginas
//! cuando le falta memoria (el aviso de memoria baja, `docs/bitacoras/aviso-cajas.md`).
//!
//! Escrito desde la especificación de virtio 1.2 (§5.5), sin leer código de QEMU, crosvm, Firecracker, Linux, KVM ni
//! Xen.
//!
//! Cola 0 (inflateq): el invitado mete páginas en el globo; cola 1 (deflateq): las saca. Cada búfer es una lista de
//! números de página (le32, en unidades de 4096 bytes) que el aparato solo lee. En la configuración, `num_pages` (lo
//! que DashOS quiere en el globo, en páginas de 4 KB) y `actual` (lo que el invitado dice que tiene).
//!
//! Se ofrecen VIRTIO_BALLOON_F_MUST_TELL_HOST (el invitado no vuelve a usar una página del globo hasta que el aparato
//! contesta a su búfer de la deflateq: así DashOS se la devuelve antes) y VIRTIO_BALLOON_F_DEFLATE_ON_OOM (si el
//! invitado se queda sin memoria, saca páginas del globo por su cuenta, también avisando). Sin la cola de
//! estadísticas, ni pistas de páginas libres, ni informes de páginas.

use alloc::boxed::Box;

use super::cola::ColaPartida;
use super::consola::copiar_config;
use super::{AparatoVirtio, ErrorVirtio, vaciar_cola};
use crate::memoria::MemoriaInvitado;

pub const ID_GLOBO: u32 = 5;
pub const F_MUST_TELL_HOST: u64 = 1 << 0;
pub const F_STATS_VQ: u64 = 1 << 1;
pub const F_DEFLATE_ON_OOM: u64 = 1 << 2;

const COLA_INFLAR: u16 = 0;
const COLA_DESINFLAR: u16 = 1;
/// Tamaño de una página para el globo (§5.5.6: siempre 4096, sea cual sea la página del invitado).
pub const PAGINA: u64 = 4096;
/// Números de página que se pasan juntos al otro lado.
pub const LOTE: usize = 256;
/// Bytes como mucho que se leen de un búfer (16384 páginas, 64 MB): una lista enorme no se lee entera.
const MAX_LISTA: u64 = 64 * 1024;

/// El otro lado del globo, en DashOS.
pub trait ExtremoGlobo {
    /// El invitado ha metido en el globo estas páginas (direcciones físicas suyas, alineadas a 4 KB; sin comprobar,
    /// pueden ser cualquier cosa): DashOS puede quitárselas.
    fn inflar(&mut self, dirs: &[u64]);
    /// El invitado las saca del globo: hay que devolvérselas antes de volver (con MUST_TELL_HOST, el invitado espera a
    /// que el aparato conteste para usarlas).
    fn desinflar(&mut self, dirs: &[u64]);
    /// Cuántas páginas de 4 KB quiere DashOS en el globo ahora.
    fn objetivo(&self) -> u32;
    /// El invitado dice cuántas tiene ya en el globo (el campo `actual`).
    fn actual(&mut self, _paginas: u32) {}
    /// El invitado reinició el aparato: ya no se acuerda de lo que había en el globo, así que DashOS tiene que
    /// devolvérselo todo.
    fn reiniciado(&mut self) {}
}

pub struct Globo {
    extremo: Box<dyn ExtremoGlobo>,
    /// `num_pages` y `actual` de la configuración.
    num_pages: u32,
    actual: u32,
}

impl Globo {
    pub fn nuevo(extremo: Box<dyn ExtremoGlobo>) -> Self {
        let num_pages = extremo.objetivo();
        Self { extremo, num_pages, actual: 0 }
    }

    /// Lo que pide DashOS y lo que dice el invitado, en páginas.
    pub fn estado(&self) -> (u32, u32) {
        (self.num_pages, self.actual)
    }

    fn config(&self) -> [u8; 8] {
        let mut c = [0u8; 8];
        c[0..4].copy_from_slice(&self.num_pages.to_le_bytes());
        c[4..8].copy_from_slice(&self.actual.to_le_bytes());
        c
    }

    /// Lee la lista de una cola y se la pasa al extremo por lotes.
    fn vaciar(&mut self, cola: u16, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        let extremo = &mut self.extremo;
        vaciar_cola(&mut colas[cola as usize], mem, |c, mem| {
            let mut lec = c.lector();
            let mut buf = [0u8; 4 * LOTE];
            let mut dirs = [0u64; LOTE];
            // Lo que no completa un número (un búfer de largo raro) se tira.
            let mut sobra = [0u8; 4];
            let mut n_sobra = 0;
            while lec.hecho < MAX_LISTA {
                let quiero = ((MAX_LISTA - lec.hecho) as usize).min(buf.len());
                let leidos = lec.leer(mem, &mut buf[..quiero])?;
                if leidos == 0 {
                    break;
                }
                let mut n = 0;
                for &b in &buf[..leidos] {
                    sobra[n_sobra] = b;
                    n_sobra += 1;
                    if n_sobra == 4 {
                        dirs[n] = u32::from_le_bytes(sobra) as u64 * PAGINA;
                        n += 1;
                        n_sobra = 0;
                    }
                }
                if n > 0 {
                    if cola == COLA_INFLAR {
                        extremo.inflar(&dirs[..n]);
                    } else {
                        extremo.desinflar(&dirs[..n]);
                    }
                }
            }
            Ok(0)
        })
    }
}

impl AparatoVirtio for Globo {
    fn tipo(&self) -> u32 {
        ID_GLOBO
    }

    fn caracteristicas(&self) -> u64 {
        F_MUST_TELL_HOST | F_DEFLATE_ON_OOM
    }

    fn colas_max(&self) -> &[u16] {
        &[256, 256]
    }

    fn leer_config(&self, desp: u64, datos: &mut [u8]) {
        copiar_config(&self.config(), desp, datos);
    }

    fn escribir_config(&mut self, desp: u64, datos: &[u8]) {
        // Solo `actual` (desplazamiento 4) es del invitado; `num_pages` es del aparato.
        let mut c = self.config();
        for (i, &b) in datos.iter().enumerate() {
            let Some(p) = desp.checked_add(i as u64).filter(|&p| (4..8).contains(&p)) else { continue };
            c[p as usize] = b;
        }
        let actual = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
        if actual != self.actual {
            self.actual = actual;
            self.extremo.actual(actual);
        }
    }

    fn reiniciar(&mut self) {
        self.actual = 0;
        self.extremo.reiniciado();
    }

    fn atender(&mut self, cola: u16, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        match cola {
            COLA_INFLAR | COLA_DESINFLAR => self.vaciar(cola, colas, mem),
            _ => Ok(false),
        }
    }

    fn config_nueva(&mut self) -> bool {
        let objetivo = self.extremo.objetivo();
        if objetivo == self.num_pages {
            return false;
        }
        self.num_pages = objetivo;
        true
    }
}
