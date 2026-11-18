//! La memoria baja (aviso-cajas, `docs/bitacoras/aviso-cajas.md`): los manejadores de memoria baja de los programas
//! (`AddMemHandler`, `RemMemHandler`, V39) y devolver a DashOS la memoria del 68k que ha crecido y está libre.
//!
//! Escrito desde los autodocs de exec (`AddMemHandler`, `RemMemHandler`) y los include de AmigaOS (`exec/memory.h`:
//! `struct MemHandlerData`, `MEMHF_RECYCLE`, `MEM_DID_NOTHING`, `MEM_ALL_DONE`, `MEM_TRY_AGAIN`; `exec/interrupts.h`:
//! `struct Interrupt`). No se ha leído código de ningún exec.
//!
//! En un Amiga, exec llama a los manejadores cuando un `AllocMem` no cabe. En la caja la memoria crece sola, así que lo
//! que los llama es el aviso de memoria baja de DashOS (llega por `sesiones`): la caja lo pide con `pedir_soltar`, y en
//! la siguiente llamada a exec del programa en marcha (`empezar_a_soltar`, desde el pegamento) se llama a cada
//! manejador, en su orden de prioridad, como si un `AllocMem` de toda la memoria no cupiera: `A0` = su
//! `MemHandlerData`, `A1` = su `is_Data`, `A6` = ExecBase. Si dice `MEM_TRY_AGAIN`, se le llama otra vez (con
//! `MEMHF_RECYCLE`), hasta 4 veces; con `MEM_ALL_DONE` o `MEM_DID_NOTHING` se pasa al siguiente (todos tienen que
//! soltar lo que puedan, no solo hasta que quepa algo). Al acabar, la memoria crecida que quede libre al final de la
//! región de arriba se devuelve a DashOS (`Memoria::encoger`), y la llamada del programa sigue como si nada.

use alloc::vec::Vec;

use crate::estructuras::{execbase, interrupt, memchunk, memf, memheader};
use crate::{Accion, Continuacion, Exec, Fallo, Memoria, Registros};

/// `struct MemHandlerData` (`exec/memory.h`).
pub mod memh {
    pub const REQUEST_SIZE: u32 = 0;
    pub const REQUEST_FLAGS: u32 = 4;
    pub const FLAGS: u32 = 8;
    pub const TAMANO: u32 = 12;
    pub const MEMHF_RECYCLE: u32 = 1 << 0;
    pub const MEM_DID_NOTHING: u32 = 0;
    pub const MEM_ALL_DONE: u32 = u32::MAX;
    pub const MEM_TRY_AGAIN: u32 = 1;
}

/// Veces que se vuelve a llamar a un manejador que dice `MEM_TRY_AGAIN`.
const MAX_OTRA_VEZ: u8 = 4;

/// Lo que se soltó por un aviso.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Soltado {
    /// Manejadores de los programas que se llamaron, y cuántas llamadas fueron.
    pub manejadores: u32,
    pub llamadas: u32,
    /// Bytes de la memoria del 68k que se devolvieron a DashOS, y dónde acababa antes y después.
    pub bytes: u32,
    pub fin_antes: u32,
    pub fin_despues: u32,
}

#[derive(Default)]
pub(crate) struct MemoriaBaja {
    /// Hay un aviso sin atender.
    pedido: bool,
    /// La cadena de manejadores en marcha: los nodos, por cuál va y cuántas veces lo ha llamado.
    cadena: Vec<u32>,
    i: usize,
    vueltas: u8,
    /// Su `MemHandlerData` (en la memoria del 68k, del sistema).
    datos: u32,
    llamadas: u32,
    hecho: Option<Soltado>,
}

impl<M: Memoria> Exec<M> {
    /// `AddMemHandler`: el manejador entra en `ExecBase->ex_MemHandlers` por su prioridad.
    pub(crate) fn add_mem_handler(&mut self, is: u32) -> Result<(), Fallo> {
        if is == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(is, interrupt::TAMANO)?;
        self.enqueue(self.base + execbase::EX_MEMHANDLERS, is)
    }

    /// `RemMemHandler`: lo saca de la lista, si está (uno que no está no se toca: sus enlaces pueden ser cualquier cosa).
    pub(crate) fn rem_mem_handler(&mut self, is: u32) -> Result<(), Fallo> {
        if self.nodos(self.base + execbase::EX_MEMHANDLERS)?.contains(&is) {
            self.remove(is)?;
        }
        Ok(())
    }

    /// DashOS va justo de memoria (el aviso, por `sesiones`): en la siguiente llamada a exec, se llama a los manejadores
    /// y se suelta lo que se pueda.
    pub fn pedir_soltar(&mut self) {
        self.memoria_baja.pedido = true;
    }

    /// Si hay un aviso sin atender y no hay una cadena de manejadores en marcha.
    pub fn soltar_pendiente(&self) -> bool {
        self.memoria_baja.pedido && self.memoria_baja.cadena.is_empty()
    }

    /// Lo soltado por el último aviso, una vez.
    pub fn tomar_soltado(&mut self) -> Option<Soltado> {
        self.memoria_baja.hecho.take()
    }

    /// Un manejador vale si su `struct Interrupt` está entera en un bloque reservado (si su programa se fue, ya no) y
    /// tiene código.
    fn manejador_vivo(&self, is: u32) -> bool {
        let dentro = self
            .bloques
            .range(..=is)
            .next_back()
            .is_some_and(|(&d, b)| is.checked_add(interrupt::TAMANO).is_some_and(|f| f <= d.saturating_add(b.largo)));
        dentro && self.l32(is + interrupt::IS_CODE).is_ok_and(|c| c != 0 && c % 2 == 0)
    }

    /// El aviso, sin tarea en marcha (entre programas): sin llamar a nadie, solo devolver lo crecido que esté libre.
    pub fn soltar_sin_tarea(&mut self) {
        if !self.soltar_pendiente() {
            return;
        }
        self.memoria_baja.pedido = false;
        self.memoria_baja.llamadas = 0;
        self.acabar_de_soltar(0);
    }

    /// Atiende el aviso desde la llamada a exec `n` del programa en marcha (con sus registros `r`): llama a los
    /// manejadores (68k, uno tras otro) y suelta; al acabar, sigue con la llamada `n` como si nada.
    pub fn empezar_a_soltar(&mut self, n: u16, r: &mut Registros) -> Accion {
        self.memoria_baja.pedido = false;
        self.memoria_baja.llamadas = 0;
        let cadena: Vec<u32> = self
            .nodos(self.base + execbase::EX_MEMHANDLERS)
            .unwrap_or_default()
            .into_iter()
            .filter(|&is| self.manejador_vivo(is))
            .collect();
        let datos = if cadena.is_empty() { 0 } else { self.reservar(memh::TAMANO, memf::PUBLIC | memf::CLEAR, 0).unwrap_or(0) };
        if datos == 0 {
            self.acabar_de_soltar(0);
            return self.llamar(n, r);
        }
        // "Todo": que suelte lo que pueda.
        let _ = self.e32(datos + memh::REQUEST_SIZE, self.memoria_total());
        let _ = self.e32(datos + memh::REQUEST_FLAGS, memf::ANY);
        let _ = self.e32(datos + memh::FLAGS, 0);
        self.memoria_baja.cadena = cadena;
        self.memoria_baja.i = 0;
        self.memoria_baja.vueltas = 0;
        self.memoria_baja.datos = datos;
        self.llamar_manejador(n, r.d[0], r)
    }

    fn memoria_total(&self) -> u32 {
        self.regiones.iter().fold(0u32, |t, &(a, b)| t.saturating_add(b.saturating_sub(a)))
    }

    fn llamar_manejador(&mut self, n: u16, d0: u32, r: &mut Registros) -> Accion {
        let mb = &self.memoria_baja;
        let is = mb.cadena[mb.i];
        let datos = mb.datos;
        let (Ok(codigo), Ok(dato)) = (self.l32(is + interrupt::IS_CODE), self.l32(is + interrupt::IS_DATA)) else {
            return self.siguiente_manejador(n, d0, memh::MEM_DID_NOTHING, r);
        };
        let mut rr = *r;
        rr.a[0] = datos;
        rr.a[1] = dato;
        rr.a[6] = self.base;
        self.memoria_baja.llamadas += 1;
        self.llamar_codigo(codigo, rr, Continuacion::Manejador { n, d0 }, r)
    }

    /// Volvió un manejador con `res` en D0 (desde `continuar`).
    pub(crate) fn siguiente_manejador(&mut self, n: u16, d0: u32, res: u32, r: &mut Registros) -> Accion {
        r.d[0] = d0;
        let datos = self.memoria_baja.datos;
        if res == memh::MEM_TRY_AGAIN && self.memoria_baja.vueltas < MAX_OTRA_VEZ {
            self.memoria_baja.vueltas += 1;
            let _ = self.e32(datos + memh::FLAGS, memh::MEMHF_RECYCLE);
        } else {
            self.memoria_baja.i += 1;
            self.memoria_baja.vueltas = 0;
            let _ = self.e32(datos + memh::FLAGS, 0);
        }
        if self.memoria_baja.i < self.memoria_baja.cadena.len() {
            return self.llamar_manejador(n, d0, r);
        }
        let manejadores = self.memoria_baja.cadena.len() as u32;
        self.memoria_baja.cadena.clear();
        if let Err(f) = self.free_mem(datos, memh::TAMANO) {
            self.avisar("(memoria baja)", f);
        }
        self.memoria_baja.datos = 0;
        self.acabar_de_soltar(manejadores);
        self.llamar(n, r)
    }

    fn acabar_de_soltar(&mut self, manejadores: u32) {
        let fin_antes = self.regiones.iter().map(|r| r.1).max().unwrap_or(0);
        let bytes = match self.encoger() {
            Ok(b) => b,
            Err(f) => {
                self.avisar("(memoria baja)", f);
                0
            }
        };
        let fin_despues = self.regiones.iter().map(|r| r.1).max().unwrap_or(0);
        self.memoria_baja.hecho =
            Some(Soltado { manejadores, llamadas: self.memoria_baja.llamadas, bytes, fin_antes, fin_despues });
    }

    /// Devuelve a DashOS la memoria crecida que está libre al final de la región de arriba: el último trozo libre, si
    /// acaba donde acaba la región, se recorta (o se quita) hasta donde la memoria pueda soltar (`Memoria::encoger`, a
    /// trozos enteros de los que creció). Devuelve los bytes soltados.
    pub fn encoger(&mut self) -> Result<u32, Fallo> {
        let mut total = self.encoger_una_vez()?;
        // Si lo que queda arriba del todo es la reserva del hueco del chipset (`reservar_chipset`), se suelta y se
        // sigue encogiendo por debajo.
        for _ in 0..8 {
            let Some(fin) = self.regiones.iter().map(|r| r.1).max() else { break };
            let (h0, h1) = crate::HUECO_DEL_CHIPSET;
            let Some((&dir, b)) = self.bloques.range(h0..h1).find(|(d, b)| *d + b.largo == fin) else { break };
            let largo = b.largo;
            self.free_mem(dir, largo)?;
            let s = self.encoger_una_vez()?;
            if s == 0 {
                break;
            }
            total += s;
        }
        Ok(total)
    }

    fn encoger_una_vez(&mut self) -> Result<u32, Fallo> {
        let Some(fin) = self.regiones.iter().map(|r| r.1).max() else { return Ok(0) };
        let Some(mh) = self.nodos(self.base + execbase::MEMLIST)?.into_iter().find(|&mh| self.lp(mh + memheader::MH_UPPER) == Ok(fin))
        else {
            return Ok(0);
        };
        let mut ultimo = None;
        self.trozos(mh, |t| {
            if t.dir + t.largo == fin {
                ultimo = Some((t.dir, t.enlace));
            }
            true
        })?;
        let Some((dir, enlace)) = ultimo else { return Ok(0) };
        let Some(nuevo) = self.mem.encoger(dir) else { return Ok(0) };
        let nuevo = nuevo & !7;
        if nuevo >= fin || nuevo < dir {
            return Ok(0);
        }
        // Lo de `[nuevo, fin)` ya no existe: el trozo se recorta o, si no queda nada, sale de la lista (era el último:
        // el enlace que apuntaba a él pasa a 0).
        if nuevo == dir {
            self.e32(enlace, 0)?;
        } else {
            self.e32(dir + memchunk::MC_BYTES, nuevo - dir)?;
        }
        let soltado = fin - nuevo;
        let libre = self.l32(mh + memheader::MH_FREE)?;
        self.e32(mh + memheader::MH_FREE, libre.saturating_sub(soltado))?;
        self.e32(mh + memheader::MH_UPPER, nuevo)?;
        if let Some(r) = self.regiones.iter_mut().find(|r| r.1 == fin) {
            r.1 = nuevo;
        }
        // Lo que el JIT hubiera traducido de ahí, fuera: si la memoria vuelve a crecer, será otra.
        self.codigo_cambiado.push((nuevo, fin));
        Ok(soltado)
    }
}
