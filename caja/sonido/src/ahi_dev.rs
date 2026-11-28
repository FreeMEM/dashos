//! ahi.device por peticiones (`AHIRequest`): abrir, cerrar, `BeginIO` y `AbortIO`, y el reparto de las escrituras
//! en las voces del flujo del corazón (`audiodevice::ahi::AhiDevice`).
//!
//! Cada `CMD_WRITE` sin `ahir_Link` (o con uno que ya acabó) suena en una voz libre, a la vez que las demás; con
//! `ahir_Link` a una que aún suena, va detrás de ella en su voz y empieza en el cuadro siguiente a su último, sin
//! hueco (el doble búfer de AmigaAMP). Las muestras se copian al recibir la escritura: el programa no puede tocar su
//! búfer hasta que vuelve. Vuelve (`ReplyMsg`) cuando su último cuadro ya ha sonado, no al mezclarlo.

use alloc::vec::Vec;

use audiodevice::ahi::{EscrituraAhi, Muestras, UNO, tamano_cuadro};
use caja_exec::estructuras::node;
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros, io};

use crate::mem::*;
use crate::*;

/// Lo más largo de una escritura: 4 MB (más de 20 s de estéreo de 16 bits a 48 kHz).
pub const MAX_ESCRITURA: u32 = 4 << 20;
/// La frecuencia más alta de una escritura o un canal (el límite de `AHI_SetFreq`).
pub const MAX_FRECUENCIA: u32 = 262_143;

/// Lo que AHI llama "unidad" de una petición: el índice de su `struct Unit`.
fn indice_unidad(unidades: &[u32; 5], u: u32) -> Option<u8> {
    unidades.iter().position(|&x| x == u).map(|i| i as u8)
}

fn junta(a: Accion, b: Accion) -> Accion {
    if a == Accion::Ceder || b == Accion::Ceder { Accion::Ceder } else { Accion::Seguir }
}

impl Sonido {
    /// `Open(ioRequest, unit, flags)(a1/d0/d1)`: unidad 0 a 3 o `AHI_NO_UNIT`, y `ahir_Version` hasta la 6.
    pub(crate) fn ahi_open<M: Memoria>(&mut self, ex: &mut Exec<M>, r: &mut Registros) -> Result<Accion, Fallo> {
        let (ior, unidad) = (r.a[1], r.d[0]);
        tramo(ex, ior, io::TAMANO)?;
        // Una IORequest corta (sin `ahir_Version`) solo vale para la biblioteca: versión 0.
        let version = if tramo(ex, ior, AHIR_VERSION + 2).is_ok() { l16(ex, ior + AHIR_VERSION)? } else { 0 };
        let i = match unidad {
            0..=3 => unidad as u8,
            AHI_NO_UNIT => 4,
            _ => 5,
        };
        if i == 5 || version > VERSION_AHI {
            e8(ex, ior + io::IO_ERROR, io::IOERR_OPENFAIL as u8)?;
            return Ok(Accion::Seguir);
        }
        let u = self.unidades[i as usize];
        e32(ex, ior + io::IO_UNIT, u)?;
        let n = l16(ex, u + 36)?;
        e16(ex, u + 36, n.saturating_add(1))?;
        e8(ex, ior + io::IO_ERROR, 0)?;
        self.aperturas.push(Apertura { ior, tarea: ex.tarea_actual(), disp: Disp::Ahi(i) });
        Ok(Accion::Seguir)
    }

    /// `Close(ioRequest)(a1)`. Lo que aún suena de esa tarea en esa unidad vuelve abortado (AHI pide que no quede
    /// nada, pero un programa que se va con prisa lo deja); si la tarea ya no tiene ahi.device abierto sin unidad,
    /// sus `AHIAudioCtrl` se sueltan.
    pub(crate) fn ahi_close<M: Memoria>(&mut self, ex: &mut Exec<M>, r: &mut Registros) -> Result<Accion, Fallo> {
        let ior = r.a[1];
        let t = ex.tarea_actual();
        let k = self
            .aperturas
            .iter()
            .position(|a| a.ior == ior && matches!(a.disp, Disp::Ahi(_)))
            .or_else(|| self.aperturas.iter().position(|a| a.tarea == t && matches!(a.disp, Disp::Ahi(_))));
        let mut accion = Accion::Seguir;
        if let Some(k) = k {
            let a = self.aperturas.remove(k);
            let Disp::Ahi(i) = a.disp else { unreachable!() };
            let u = self.unidades[i as usize];
            if let Ok(n) = l16(ex, u + 36) {
                let _ = e16(ex, u + 36, n.saturating_sub(1));
            }
            if i < 4 {
                let quedan: Vec<u32> = self
                    .escrituras
                    .iter()
                    .filter(|(_, e)| e.tarea == a.tarea && e.unidad == i)
                    .map(|(&x, _)| x)
                    .collect();
                if !quedan.is_empty() {
                    self.avisar(ex, "ahi.device Close", Fallo::Sospechoso { dir: ior });
                }
                for x in quedan {
                    accion = junta(accion, self.abortar_escritura(ex, x)?);
                }
            } else if !self.aperturas.iter().any(|b| b.tarea == a.tarea && b.disp == Disp::Ahi(4)) {
                let suyos: Vec<u32> =
                    self.controles.iter().filter(|(_, c)| c.tarea == a.tarea).map(|(&d, _)| d).collect();
                for d in suyos {
                    self.avisar(ex, "ahi.device Close", Fallo::Sospechoso { dir: d });
                    self.soltar_control(ex, d);
                }
            }
        }
        self.cerrar_en_exec(ex, self.base_ahi, r)?;
        Ok(accion)
    }

    pub(crate) fn ahi_begin_io<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<Accion, Fallo> {
        tramo(ex, ior, io::STD_TAMANO)?;
        if self.escrituras.contains_key(&ior) {
            // Mandarla otra vez mientras suena rompería la cola del puerto: se queda la de antes.
            return Err(Fallo::MensajeEnCola { dir: ior });
        }
        let orden = l16(ex, ior + io::IO_COMMAND)?;
        let Some(unidad) = indice_unidad(&self.unidades, l32(ex, ior + io::IO_UNIT)?) else {
            self.avisar(ex, "ahi.device BeginIO", Fallo::Sospechoso { dir: ior });
            return self.acabar(ex, ior, io::IOERR_NOCMD);
        };
        e32(ex, ior + io::IO_ACTUAL, 0)?;
        match orden {
            io::CMD_WRITE => self.escribir_flujo(ex, ior, unidad),
            // No hay entrada de sonido.
            io::CMD_READ => self.acabar(ex, ior, io::IOERR_NOCMD),
            io::CMD_FLUSH | io::CMD_RESET => {
                let todas: Vec<u32> = self.escrituras.keys().copied().collect();
                let mut accion = Accion::Seguir;
                for x in &todas {
                    accion = junta(accion, self.abortar_escritura(ex, *x)?);
                }
                if orden == io::CMD_RESET {
                    self.parado = 0;
                    self.colocar();
                }
                e32(ex, ior + io::IO_ACTUAL, todas.len() as u32)?;
                Ok(junta(accion, self.acabar(ex, ior, 0)?))
            }
            io::CMD_STOP => {
                self.parado = self.parado.saturating_add(1);
                self.acabar(ex, ior, 0)
            }
            io::CMD_START => {
                self.parado = self.parado.saturating_sub(1);
                self.colocar();
                self.acabar(ex, ior, 0)
            }
            io::CMD_UPDATE | io::CMD_CLEAR => self.acabar(ex, ior, 0),
            NSCMD_DEVICEQUERY => {
                let (d, l) = (l32(ex, ior + io::IO_DATA)?, l32(ex, ior + io::IO_LENGTH)?);
                if l < 16 {
                    return self.acabar(ex, ior, io::IOERR_BADLENGTH);
                }
                tramo(ex, d, 16)?;
                e32(ex, d, 0)?;
                e32(ex, d + 4, 16)?;
                e16(ex, d + 8, 0)?;
                e16(ex, d + 10, 0)?;
                e32(ex, d + 12, self.ordenes)?;
                e32(ex, ior + io::IO_ACTUAL, 16)?;
                self.acabar(ex, ior, 0)
            }
            _ => self.acabar(ex, ior, io::IOERR_NOCMD),
        }
    }

    /// `CMD_WRITE`: comprueba y copia las muestras, y la escritura espera voz.
    fn escribir_flujo<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32, unidad: u8) -> Result<Accion, Fallo> {
        if tramo(ex, ior, AHIR_TAMANO).is_err() {
            self.avisar(ex, "ahi.device BeginIO", Fallo::FueraDeMemoria { dir: ior, largo: AHIR_TAMANO });
            return self.acabar(ex, ior, io::IOERR_BADLENGTH);
        }
        if unidad == 4 {
            // Abierto con AHI_NO_UNIT: solo la biblioteca.
            return self.acabar(ex, ior, io::IOERR_NOCMD);
        }
        let datos = l32(ex, ior + io::IO_DATA)?;
        let largo = l32(ex, ior + io::IO_LENGTH)?;
        let tipo = l32(ex, ior + AHIR_TYPE)?;
        let mut frecuencia = l32(ex, ior + AHIR_FREQUENCY)?;
        let volumen = (l32(ex, ior + AHIR_VOLUME)? as i32).clamp(-4 * UNO as i32, 4 * UNO as i32);
        let paneo = (l32(ex, ior + AHIR_POSITION)? as i32).unsigned_abs().min(UNO);
        let enlace = l32(ex, ior + AHIR_LINK)?;
        let Some(tc) = tamano_cuadro(tipo) else { return self.acabar(ex, ior, AHIE_BADSAMPLETYPE as i8) };
        if largo % tc != 0 || largo > MAX_ESCRITURA {
            return self.acabar(ex, ior, io::IOERR_BADLENGTH);
        }
        if largo == 0 {
            return self.acabar(ex, ior, 0);
        }
        if frecuencia == 0 {
            return self.acabar(ex, ior, AHIE_UNKNOWN as i8);
        }
        if frecuencia > MAX_FRECUENCIA {
            self.avisar(ex, "ahi.device BeginIO", Fallo::Sospechoso { dir: ior });
            frecuencia = MAX_FRECUENCIA;
        }
        let bytes = match comprobar(datos, largo, false).and_then(|_| ex.mem.leer(datos, largo)) {
            Ok(b) => b,
            Err(f) => {
                self.avisar(ex, "ahi.device BeginIO", f);
                return self.acabar(ex, ior, io::IOERR_BADADDRESS);
            }
        };
        let Ok(muestras) = Muestras::desde_amiga(tipo, bytes) else {
            return self.acabar(ex, ior, AHIE_BADSAMPLETYPE as i8);
        };
        self.no_rapida(ex, ior)?;
        e8(ex, ior + node::LN_TYPE, caja_exec::estructuras::nt::MESSAGE)?;
        let serie = self.siguiente_serie();
        let id = (serie as u64) << 32 | ior as u64;
        self.escrituras
            .insert(ior, Escrita { serie, tarea: ex.tarea_actual(), unidad, voz: None, mezclada: false, largo });
        let e = EscrituraAhi { id, muestras, frecuencia, volumen, paneo };
        self.sin_voz.push_back(Esperando { ior, enlace, e });
        self.colocar();
        Ok(Accion::Seguir)
    }

    /// Pone en su voz las escrituras que esperan, por orden: detrás de su enlace si aún suena (o espera), o en una
    /// voz libre. Con `CMD_STOP`, ninguna.
    pub(crate) fn colocar(&mut self) {
        if self.parado > 0 {
            return;
        }
        let todas: Vec<Esperando> = self.sin_voz.drain(..).collect();
        for p in todas {
            let enlace = if p.enlace == p.ior { 0 } else { p.enlace };
            let destino = match self.escrituras.get(&enlace) {
                // Su enlace aún suena: detrás, en su voz.
                Some(Escrita { voz: Some(v), .. }) if enlace != 0 => Some(*v),
                // Su enlace aún espera voz: espera con él (va detrás en la cola).
                Some(Escrita { voz: None, mezclada: false, .. }) if enlace != 0 => None,
                _ => self.voces.iter().position(|&n| n == 0).map(|v| v as u16),
            };
            match destino {
                // (La voz existe y la escritura no está vacía ni a 0 Hz: el corazón la acepta.)
                Some(v) => {
                    let _ = self.flujo.escribir(v, p.e);
                    self.voces[v as usize] += 1;
                    if let Some(e) = self.escrituras.get_mut(&p.ior) {
                        e.voz = Some(v);
                    }
                }
                None => self.sin_voz.push_back(p),
            }
        }
    }

    /// Quita una escritura de todas partes, sin devolverla.
    pub(crate) fn quitar_escritura(&mut self, ior: u32) -> Option<Escrita> {
        let e = self.escrituras.remove(&ior)?;
        if let Some(v) = e.voz {
            self.flujo.abortar((e.serie as u64) << 32 | ior as u64);
            let _ = self.flujo.hechas();
            self.voces[v as usize] = self.voces[v as usize].saturating_sub(1);
        }
        self.sin_voz.retain(|p| p.ior != ior);
        Some(e)
    }

    /// Aborta una escritura: vuelve ya con `IOERR_ABORTED`; la que fuera detrás en su voz suena desde su principio.
    fn abortar_escritura<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<Accion, Fallo> {
        let Some(e) = self.quitar_escritura(ior) else { return Ok(Accion::Seguir) };
        self.colocar();
        e32(ex, ior + io::IO_ACTUAL, if e.mezclada { e.largo } else { 0 })?;
        self.acabar(ex, ior, io::IOERR_ABORTED)
    }

    /// `AbortIO(ioRequest)(a1)`: 0 si estaba en marcha (y vuelve abortada), -1 si no.
    pub(crate) fn ahi_abort_io<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<u32, Fallo> {
        if !self.escrituras.contains_key(&ior) {
            return Ok(u32::MAX);
        }
        self.abortar_escritura(ex, ior)?;
        Ok(0)
    }

    /// Tras mezclar un trozo que empezó en el cuadro `p0` (de la salida) cuando el flujo llevaba `h0` mezclados: las
    /// escrituras que acabaron vuelven cuando suene su último cuadro, y sus voces quedan libres para las que esperan.
    pub(crate) fn recoger_flujo(&mut self, p0: u64, h0: u64) {
        let hechas = self.flujo.hechas_con_cuadro();
        if hechas.is_empty() {
            return;
        }
        for (id, c) in hechas {
            let ior = id as u32;
            let Some(e) = self.escrituras.get_mut(&ior) else { continue };
            if e.serie != (id >> 32) as u32 {
                continue;
            }
            if let Some(v) = e.voz.take() {
                self.voces[v as usize] = self.voces[v as usize].saturating_sub(1);
            }
            e.mezclada = true;
            let cuadro = p0 + c.saturating_sub(h0);
            self.poner_vuelta(Vuelta { cuadro, que: Que::Flujo { id } });
        }
        self.colocar();
    }

    /// Una escritura ya sonó: vuelve (si sigue siendo la misma petición).
    pub(crate) fn devolver_flujo<M: Memoria>(&mut self, ex: &mut Exec<M>, id: u64) {
        let ior = id as u32;
        match self.escrituras.get(&ior) {
            Some(e) if e.serie == (id >> 32) as u32 && e.mezclada => {}
            _ => return,
        }
        let e = self.escrituras.remove(&ior).unwrap();
        let hecho = e32(ex, ior + io::IO_ACTUAL, e.largo).and_then(|_| self.acabar(ex, ior, 0));
        if let Err(f) = hecho {
            // La memoria de la petición ya no está (el programa la soltó sin esperarla): se olvida.
            self.avisar(ex, "ahi.device (vuelve una escritura)", f);
        }
    }
}
