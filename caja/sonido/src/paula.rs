//! audio.device: las `IOAudio` del 68k pasan al corazón (`audiodevice::AudioDevice`, los cuatro canales de Paula) y
//! sus avisos vuelven como `ReplyMsg`.
//!
//! Las muestras de `CMD_WRITE` no se copian: se leen de la memoria del 68k (la "chip") en cada mezcla, como la DMA de
//! Paula; un programa que rellena su búfer mientras suena (un anillo con `ioa_Cycles` 0) se oye como en un Amiga.
//! Lo que acaba al mezclar (una escritura, un `ADCMD_FINISH` o `ADCMD_WAITCYCLE` al final del ciclo) vuelve cuando
//! suena ese cuadro; lo que acaba al pedirlo (asignar, soltar, un canal robado...), en el acto.
//!
//! `OpenDevice` con `ioa_Length` distinto de 0 asigna canales como `ADCMD_ALLOCATE` sin esperar (el RKRM); si no
//! puede, falla con `ADIOERR_ALLOCFAILED`. `CloseDevice` suelta los canales de su llave.

use alloc::vec::Vec;

use audiodevice::{
    ADCMD_ALLOCATE, ADCMD_FREE, ADIOERR_ALLOCFAILED, ADIOF_NOWAIT, Aviso as AvisoPaula, CMD_READ, CMD_WRITE, Peticion,
};
use caja_exec::estructuras::node;
use caja_exec::{Accion, Exec, Fallo, Memoria, Registros, io};

use crate::mem::*;
use crate::*;

/// Lo más largo de una escritura de Paula: 65.535 palabras.
pub const MAX_ESCRITURA_PAULA: u32 = 131_070;

fn junta(a: Accion, b: Accion) -> Accion {
    if a == Accion::Ceder || b == Accion::Ceder { Accion::Ceder } else { Accion::Seguir }
}

impl Sonido {
    /// Un número de petición propio (soltar canales al cerrar o al morir una tarea): su aviso no vuelve a nadie.
    fn sintetica(&mut self) -> u64 {
        (self.siguiente_serie() as u64) << 32
    }

    /// La petición del corazón de una `IOAudio`.
    fn peticion<M: Memoria>(&mut self, ex: &Exec<M>, ior: u32, id: u64) -> Result<(Peticion, u32, u32), Fallo> {
        let p = Peticion {
            id,
            orden: l16(ex, ior + io::IO_COMMAND)?,
            banderas: l8(ex, ior + io::IO_FLAGS)?,
            unidad: (l32(ex, ior + io::IO_UNIT)? & 0xf) as u8,
            llave: l16(ex, ior + IOA_ALLOCKEY)?,
            prec: l8(ex, ior + node::LN_PRI)? as i8,
            periodo: l16(ex, ior + IOA_PERIOD)?,
            volumen: l16(ex, ior + IOA_VOLUME)?,
            ciclos: l16(ex, ior + IOA_CYCLES)?,
            ..Default::default()
        };
        Ok((p, l32(ex, ior + IOA_DATA)?, l32(ex, ior + IOA_LENGTH)?))
    }

    /// Las combinaciones de canales de una asignación (un byte cada una, hasta 16).
    fn combinaciones<M: Memoria>(&mut self, ex: &Exec<M>, datos: u32, largo: u32) -> Result<Vec<i8>, Fallo> {
        let n = largo.min(16);
        if n == 0 {
            return Ok(Vec::new());
        }
        comprobar(datos, n, false)?;
        Ok(ex.mem.leer(datos, n)?.iter().map(|&b| b as i8).collect())
    }

    /// `Open(ioRequest, unit, flags)(a1/d0/d1)`.
    pub(crate) fn paula_open<M: Memoria>(&mut self, ex: &mut Exec<M>, r: &mut Registros) -> Result<Accion, Fallo> {
        let ior = r.a[1];
        tramo(ex, ior, IOA_TAMANO)?;
        e8(ex, ior + io::IO_ERROR, 0)?;
        let largo = l32(ex, ior + IOA_LENGTH)?;
        let mut accion = Accion::Seguir;
        if largo != 0 {
            let id = self.sintetica();
            let (mut p, datos, largo) = self.peticion(ex, ior, id)?;
            p.orden = ADCMD_ALLOCATE;
            p.banderas = ADIOF_NOWAIT;
            p.datos = self.combinaciones(ex, datos, largo)?;
            self.paula.pedir(p);
            let mut bien = false;
            for a in self.paula.avisos() {
                match a {
                    AvisoPaula::Hecha { id: x, error: 0, unidad, llave } if x == id => {
                        e32(ex, ior + io::IO_UNIT, unidad as u32)?;
                        e16(ex, ior + IOA_ALLOCKEY, llave)?;
                        self.llaves.insert(llave, ex.tarea_actual());
                        bien = true;
                    }
                    AvisoPaula::Hecha { id: x, .. } if x == id => {}
                    a => accion = junta(accion, self.aplicar_paula(ex, a)),
                }
            }
            if !bien {
                e8(ex, ior + io::IO_ERROR, ADIOERR_ALLOCFAILED as u8)?;
                return Ok(accion);
            }
        }
        self.aperturas.push(Apertura { ior, tarea: ex.tarea_actual(), disp: Disp::Audio });
        Ok(accion)
    }

    /// `Close(ioRequest)(a1)`: suelta los canales de su llave (lo que sonaba en ellos vuelve abortado).
    pub(crate) fn paula_close<M: Memoria>(&mut self, ex: &mut Exec<M>, r: &mut Registros) -> Result<Accion, Fallo> {
        let ior = r.a[1];
        if let Some(k) = self.aperturas.iter().position(|a| a.ior == ior && a.disp == Disp::Audio) {
            self.aperturas.remove(k);
        }
        let mut accion = Accion::Seguir;
        if tramo(ex, ior, IOA_TAMANO).is_ok()
            && let Ok(llave) = l16(ex, ior + IOA_ALLOCKEY)
            && llave != 0
        {
            let id = self.sintetica();
            self.paula.pedir(Peticion { id, orden: ADCMD_FREE, unidad: 0xf, llave, ..Default::default() });
            self.llaves.remove(&llave);
            for a in self.paula.avisos() {
                accion = junta(accion, self.aplicar_paula(ex, a));
            }
        }
        self.cerrar_en_exec(ex, self.base_audio, r)?;
        Ok(accion)
    }

    pub(crate) fn paula_begin_io<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<Accion, Fallo> {
        tramo(ex, ior, IOA_TAMANO)?;
        if self.peticiones_paula.contains_key(&ior) {
            return Err(Fallo::MensajeEnCola { dir: ior });
        }
        let serie = self.siguiente_serie();
        let id = (serie as u64) << 32 | ior as u64;
        let (mut p, datos, largo) = self.peticion(ex, ior, id)?;
        match p.orden {
            ADCMD_ALLOCATE => match self.combinaciones(ex, datos, largo) {
                Ok(c) => p.datos = c,
                Err(f) => {
                    self.avisar(ex, "audio.device BeginIO", f);
                    return self.acabar(ex, ior, io::IOERR_BADADDRESS);
                }
            },
            CMD_WRITE => {
                // Paula lee palabras: un byte suelto al final no suena.
                let largo = largo & !1;
                if largo > MAX_ESCRITURA_PAULA {
                    return self.acabar(ex, ior, io::IOERR_BADLENGTH);
                }
                if largo >= 2
                    && let Err(f) = tramo(ex, datos, largo)
                {
                    self.avisar(ex, "audio.device BeginIO", f);
                    return self.acabar(ex, ior, io::IOERR_BADADDRESS);
                }
                p.fuera = Some((datos, largo));
            }
            _ => {}
        }
        self.peticiones_paula.insert(ior, PetPaula { serie, tarea: ex.tarea_actual() });
        self.paula.pedir(p);
        let avisos = self.paula.avisos();
        let al_acto = avisos.iter().any(|a| matches!(a, AvisoPaula::Hecha { id: x, .. } if *x == id));
        if !al_acto {
            // Espera (a sonar, a un canal...): DoIO la esperará en su puerto.
            self.no_rapida(ex, ior)?;
        }
        let mut accion = Accion::Seguir;
        for a in avisos {
            accion = junta(accion, self.aplicar_paula(ex, a));
        }
        Ok(accion)
    }

    /// `AbortIO(ioRequest)(a1)`: 0 si estaba en marcha, -1 si no.
    pub(crate) fn paula_abort_io<M: Memoria>(&mut self, ex: &mut Exec<M>, ior: u32) -> Result<u32, Fallo> {
        let Some(p) = self.peticiones_paula.get(&ior).copied() else { return Ok(u32::MAX) };
        let id = (p.serie as u64) << 32 | ior as u64;
        self.paula.abortar(id);
        let mut avisos = self.paula.avisos();
        if !avisos.iter().any(|a| matches!(a, AvisoPaula::Hecha { id: x, .. } if *x == id)) {
            // Ya acabó de mezclarse y espera a sonar: vuelve ya, como acabó.
            if let Some(k) = self
                .vueltas
                .iter()
                .position(|v| matches!(v.que, Que::Paula(AvisoPaula::Hecha { id: x, .. }) if x == id))
            {
                let v = self.vueltas.remove(k).unwrap();
                if let Que::Paula(a) = v.que {
                    avisos.push(a);
                }
            }
        }
        for a in avisos {
            self.aplicar_paula(ex, a);
        }
        Ok(0)
    }

    /// Un aviso del corazón: la petición vuelve (o su `ioa_WriteMsg`), si sigue siendo la misma.
    pub(crate) fn aplicar_paula<M: Memoria>(&mut self, ex: &mut Exec<M>, a: AvisoPaula) -> Accion {
        let (id, empieza) = match a {
            AvisoPaula::Hecha { id, .. } => (id, false),
            AvisoPaula::Empieza { id } => (id, true),
        };
        let ior = id as u32;
        match self.peticiones_paula.get(&ior) {
            Some(p) if p.serie == (id >> 32) as u32 => {}
            _ => return Accion::Seguir,
        }
        let hecho = if empieza {
            ex.reply_msg(ior + IOA_WRITEMSG)
        } else {
            let p = self.peticiones_paula.remove(&ior).unwrap();
            self.terminar_paula(ex, ior, p, a)
        };
        match hecho {
            Ok(acc) => acc,
            Err(f) => {
                // La memoria de la petición ya no está (el programa la soltó sin esperarla): se olvida.
                self.avisar(ex, "audio.device (vuelve una petición)", f);
                Accion::Seguir
            }
        }
    }

    fn terminar_paula<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        ior: u32,
        p: PetPaula,
        a: AvisoPaula,
    ) -> Result<Accion, Fallo> {
        let AvisoPaula::Hecha { error, unidad, llave, .. } = a else { return Ok(Accion::Seguir) };
        let orden = l16(ex, ior + io::IO_COMMAND)?;
        if orden == ADCMD_ALLOCATE && error == 0 {
            e32(ex, ior + io::IO_UNIT, unidad as u32)?;
            e16(ex, ior + IOA_ALLOCKEY, llave)?;
            self.llaves.insert(llave, p.tarea);
        }
        if orden == CMD_READ {
            // La escritura que suena en el canal (la `IOAudio`), o 0.
            let i = (unidad.trailing_zeros() as usize).min(3);
            let v = self.paula.sonando(i).map_or(0, |x| x as u32);
            e32(ex, ior + IOA_DATA, v)?;
        }
        self.acabar(ex, ior, error)
    }

    /// Lo de audio.device de una tarea que muere: sus peticiones se olvidan y sus canales se sueltan. Lo que eso
    /// devuelva a otras tareas (una asignación que esperaba esos canales) vuelve en el siguiente `atender`.
    pub(crate) fn paula_tarea_muerta(&mut self, t: u32) {
        let suyas: Vec<(u32, PetPaula)> =
            self.peticiones_paula.iter().filter(|(_, p)| p.tarea == t).map(|(&i, &p)| (i, p)).collect();
        for (ior, p) in suyas {
            self.peticiones_paula.remove(&ior);
            self.paula.abortar((p.serie as u64) << 32 | ior as u64);
        }
        let llaves: Vec<u16> = self.llaves.iter().filter(|(_, x)| **x == t).map(|(&k, _)| k).collect();
        for llave in llaves {
            let id = self.sintetica();
            self.paula.pedir(Peticion { id, orden: ADCMD_FREE, unidad: 0xf, llave, ..Default::default() });
            self.llaves.remove(&llave);
        }
        for a in self.paula.avisos().into_iter().rev() {
            self.vueltas.push_front(Vuelta { cuadro: 0, que: Que::Paula(a) });
        }
    }
}
