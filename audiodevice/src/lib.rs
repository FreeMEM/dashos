//! `audio.device` nativo (S19, fase 2): los cuatro canales de Paula, su reparto y sus órdenes, mezclados en estéreo
//! para un flujo del servicio de sonido.
//!
//! Escrito desde la documentación pública del Amiga (el capítulo "Audio Device" del ROM Kernel Reference Manual:
//! Devices, y `devices/audio.h`). No se ha copiado código de nadie.
//!
//! Esto es el corazón: recibe peticiones (lo que un programa manda con `BeginIO`), lleva los canales y genera el
//! sonido. Quien lo usa (la caja Amiga, con H4; hoy, un programa nativo de prueba) traduce sus `IOAudio` a
//! `Peticion` y devuelve cada `Aviso` a su programa con `ReplyMsg`.
//!
//! Cómo suena: cada canal lee muestras de 8 bits con signo, una cada `periodo` tics del reloj de Paula
//! (3.546.895 Hz en PAL), y las multiplica por su volumen (0 a 64). El valor se mantiene hasta la siguiente muestra,
//! como el conversor de Paula. Los canales 0 y 3 van a la izquierda y el 1 y el 2 a la derecha. Todo con enteros:
//! la posición en coma fija de 32.32, sin coma flotante.
//!
//! Diferencia con el diseño (S19): allí cada canal era un flujo del servicio remuestreado por él. Aquí los cuatro
//! salen ya mezclados en un flujo estéreo de 48 kHz, porque el período cambia en cada escritura (y con
//! `ADCMD_PERVOL`, a mitad de una) y la frecuencia de un flujo del servicio es fija.
//!
//! Lo que no sé seguro del comportamiento del original (comprobar con el oráculo de FS-UAE cuando haya caja):
//! con qué error vuelven las escrituras de un canal robado (aquí `ADIOERR_CHANNELSTOLEN`), y el de una escritura
//! cortada con `ADCMD_FINISH` (aquí `IOERR_ABORTED`).

#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub const RELOJ_PAL: u64 = 3_546_895;
pub const RELOJ_NTSC: u64 = 3_579_545;
/// El período más corto que lee la DMA de Paula.
pub const PERIODO_MINIMO: u16 = 124;

// Órdenes (exec y `devices/audio.h`).
pub const CMD_RESET: u16 = 1;
pub const CMD_READ: u16 = 2;
pub const CMD_WRITE: u16 = 3;
pub const CMD_UPDATE: u16 = 4;
pub const CMD_CLEAR: u16 = 5;
pub const CMD_STOP: u16 = 6;
pub const CMD_START: u16 = 7;
pub const CMD_FLUSH: u16 = 8;
pub const ADCMD_FREE: u16 = 9;
pub const ADCMD_SETPREC: u16 = 10;
pub const ADCMD_FINISH: u16 = 11;
pub const ADCMD_PERVOL: u16 = 12;
pub const ADCMD_LOCK: u16 = 13;
pub const ADCMD_WAITCYCLE: u16 = 14;
pub const ADCMD_ALLOCATE: u16 = 32;

// Banderas de `io_Flags`.
pub const ADIOF_PERVOL: u8 = 16;
pub const ADIOF_SYNCCYCLE: u8 = 32;
pub const ADIOF_NOWAIT: u8 = 64;
pub const ADIOF_WRITEMESSAGE: u8 = 128;

// Errores.
pub const IOERR_NOCMD: i8 = -3;
pub const IOERR_BADLENGTH: i8 = -4;
pub const IOERR_ABORTED: i8 = -2;
pub const ADIOERR_NOALLOCATION: i8 = -10;
pub const ADIOERR_ALLOCFAILED: i8 = -11;
pub const ADIOERR_CHANNELSTOLEN: i8 = -12;

/// Quién es una petición: la dirección de su `IOAudio`, o lo que use quien llama.
pub type Id = u64;

/// Una petición, con los campos de `IOAudio` que usa cada orden.
#[derive(Clone, Debug, Default)]
pub struct Peticion {
    pub id: Id,
    pub orden: u16,
    pub banderas: u8,
    /// `io_Unit`: los canales, un bit cada uno.
    pub unidad: u8,
    /// `ioa_AllocKey`.
    pub llave: u16,
    /// `ln_Pri` del mensaje: la precedencia, para asignar y para `ADCMD_SETPREC`.
    pub prec: i8,
    /// `ioa_Data`: las muestras de `CMD_WRITE`, o las combinaciones de canales de `ADCMD_ALLOCATE` (un byte cada una).
    pub datos: Vec<i8>,
    pub periodo: u16,
    pub volumen: u16,
    /// 0: sin fin.
    pub ciclos: u16,
}

/// Lo que vuelve a un programa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aviso {
    /// La petición acabó (`ReplyMsg`): su error (0 si bien), los canales y la llave.
    Hecha { id: Id, error: i8, unidad: u8, llave: u16 },
    /// Una escritura con `ADIOF_WRITEMESSAGE` empieza a sonar (`ioa_WriteMsg`).
    Empieza { id: Id },
}

struct Escritura {
    id: Id,
    datos: Vec<i8>,
    pervol: Option<(u16, u16)>,
    ciclos: u16,
    mensaje: bool,
}

/// Lo que espera al final del ciclo de un canal.
enum AlFinal {
    Esperar(Id),
    Acabar(Id),
    PerVol(Id, u16, u16),
}

#[derive(Default)]
struct Canal {
    /// La llave de quien lo tiene (0: libre) y su precedencia.
    llave: u16,
    prec: i8,
    /// Una petición de `ADCMD_LOCK` pendiente.
    cerrojo: Option<Id>,
    /// Robado por una asignación que espera a que lo suelten (estaba con cerrojo).
    pedido: bool,
    cola: VecDeque<Escritura>,
    actual: Option<Escritura>,
    /// Posición en la muestra actual, en coma fija 32.32, y ciclos hechos.
    pos: u64,
    hechos: u16,
    periodo: u16,
    volumen: u16,
    parado: bool,
    al_final: Vec<AlFinal>,
}

pub struct AudioDevice {
    canales: [Canal; 4],
    /// Asignaciones que esperan canales.
    esperando: VecDeque<Peticion>,
    siguiente_llave: u16,
    reloj: u64,
    frecuencia: u32,
    avisos: Vec<Aviso>,
}

fn hecha(id: Id, error: i8, unidad: u8, llave: u16) -> Aviso {
    Aviso::Hecha { id, error, unidad, llave }
}

impl AudioDevice {
    /// Con el reloj de Paula (`RELOJ_PAL` o `RELOJ_NTSC`) y la frecuencia de la salida.
    pub fn nuevo(reloj: u64, frecuencia: u32) -> Self {
        let mut d = AudioDevice {
            canales: Default::default(),
            esperando: VecDeque::new(),
            siguiente_llave: 1,
            reloj,
            frecuencia,
            avisos: Vec::new(),
        };
        for c in &mut d.canales {
            c.periodo = PERIODO_MINIMO;
        }
        d
    }

    /// Lo que ha vuelto desde la última vez, en orden.
    pub fn avisos(&mut self) -> Vec<Aviso> {
        core::mem::take(&mut self.avisos)
    }

    /// Los canales de `unidad` que tiene `llave`.
    fn suyos(&self, unidad: u8, llave: u16) -> u8 {
        (0..4).filter(|&i| unidad & (1 << i) != 0 && self.canales[i].llave == llave && llave != 0).fold(0, |m, i| m | 1 << i)
    }

    /// `BeginIO`. Lo que acaba ya, sale en `avisos`; lo demás, cuando toque.
    pub fn pedir(&mut self, p: Peticion) {
        let suyos = self.suyos(p.unidad, p.llave);
        let canales = || (0..4usize).filter(move |i| suyos & (1 << i) != 0);
        match p.orden {
            ADCMD_ALLOCATE => self.asignar(p),
            CMD_WRITE => {
                let Some(i) = canales().next() else { return self.avisos.push(hecha(p.id, ADIOERR_NOALLOCATION, 0, p.llave)) };
                if p.datos.len() < 2 {
                    return self.avisos.push(hecha(p.id, IOERR_BADLENGTH, 1 << i, p.llave));
                }
                let pervol = (p.banderas & ADIOF_PERVOL != 0).then_some((p.periodo, p.volumen));
                let e = Escritura { id: p.id, datos: p.datos, pervol, ciclos: p.ciclos, mensaje: p.banderas & ADIOF_WRITEMESSAGE != 0 };
                self.canales[i].cola.push_back(e);
                self.empezar(i);
            }
            _ if suyos == 0 && p.orden != CMD_UPDATE && p.orden != CMD_CLEAR => {
                self.avisos.push(hecha(p.id, ADIOERR_NOALLOCATION, 0, p.llave))
            }
            ADCMD_FREE => {
                for i in canales() {
                    self.soltar(i);
                }
                self.avisos.push(hecha(p.id, 0, suyos, p.llave));
                self.repartir();
            }
            ADCMD_SETPREC => {
                for i in canales() {
                    self.canales[i].prec = p.prec;
                }
                self.avisos.push(hecha(p.id, 0, suyos, p.llave));
                self.repartir();
            }
            ADCMD_LOCK => {
                // Uno por canal; la petición vuelve cuando se lo quieran quitar, por el primero.
                for i in canales() {
                    self.canales[i].cerrojo = Some(p.id);
                }
            }
            ADCMD_FINISH | ADCMD_PERVOL | ADCMD_WAITCYCLE => {
                let sinc = p.banderas & ADIOF_SYNCCYCLE != 0 || p.orden == ADCMD_WAITCYCLE;
                let mut espera = false;
                for i in canales() {
                    let c = &mut self.canales[i];
                    let sonando = c.actual.is_some() && !c.parado;
                    if sinc && sonando {
                        c.al_final.push(match p.orden {
                            ADCMD_FINISH => AlFinal::Acabar(p.id),
                            ADCMD_PERVOL => AlFinal::PerVol(p.id, p.periodo, p.volumen),
                            _ => AlFinal::Esperar(p.id),
                        });
                        espera = true;
                    } else if p.orden == ADCMD_FINISH {
                        self.acabar_actual(i, IOERR_ABORTED);
                    } else if p.orden == ADCMD_PERVOL {
                        c.periodo = p.periodo;
                        c.volumen = p.volumen;
                    }
                }
                // Con varios canales sincronizados, vuelve con el primero que acabe su ciclo.
                if !espera {
                    self.avisos.push(hecha(p.id, 0, suyos, p.llave));
                }
            }
            CMD_STOP | CMD_START => {
                for i in canales() {
                    self.canales[i].parado = p.orden == CMD_STOP;
                }
                self.avisos.push(hecha(p.id, 0, suyos, p.llave));
            }
            CMD_FLUSH | CMD_RESET => {
                for i in canales() {
                    self.vaciar(i, IOERR_ABORTED);
                    if p.orden == CMD_RESET {
                        let c = &mut self.canales[i];
                        c.parado = false;
                        c.periodo = PERIODO_MINIMO;
                        c.volumen = 0;
                    }
                }
                self.avisos.push(hecha(p.id, 0, suyos, p.llave));
            }
            CMD_READ => {
                // El original pone en `ioa_Data` la escritura que suena: quien llama la saca de `sonando`.
                self.avisos.push(hecha(p.id, 0, suyos, p.llave));
            }
            CMD_UPDATE | CMD_CLEAR => self.avisos.push(hecha(p.id, 0, suyos, p.llave)),
            _ => self.avisos.push(hecha(p.id, IOERR_NOCMD, 0, p.llave)),
        }
    }

    /// La escritura que suena en el canal `i` (para `CMD_READ`).
    pub fn sonando(&self, i: usize) -> Option<Id> {
        self.canales.get(i)?.actual.as_ref().map(|e| e.id)
    }

    /// Quién tiene el canal `i`: su llave (0 si nadie) y su precedencia.
    pub fn dueno(&self, i: usize) -> (u16, i8) {
        (self.canales[i].llave, self.canales[i].prec)
    }

    /// `AbortIO`: quita la petición de donde espere y vuelve con `IOERR_ABORTED`. Una que ya volvió, nada.
    pub fn abortar(&mut self, id: Id) {
        if let Some(k) = self.esperando.iter().position(|p| p.id == id) {
            let p = self.esperando.remove(k).unwrap();
            self.avisos.push(hecha(id, IOERR_ABORTED, 0, p.llave));
            return;
        }
        for i in 0..4 {
            let llave = self.canales[i].llave;
            if self.canales[i].actual.as_ref().is_some_and(|e| e.id == id) {
                self.acabar_actual(i, IOERR_ABORTED);
                return;
            }
            let c = &mut self.canales[i];
            if let Some(k) = c.cola.iter().position(|e| e.id == id) {
                c.cola.remove(k);
                self.avisos.push(hecha(id, IOERR_ABORTED, 1 << i, llave));
                return;
            }
            if c.cerrojo == Some(id) {
                let mut unidad = 0;
                for (j, d) in self.canales.iter_mut().enumerate() {
                    if d.cerrojo == Some(id) {
                        d.cerrojo = None;
                        unidad |= 1 << j;
                    }
                }
                self.avisos.push(hecha(id, IOERR_ABORTED, unidad, llave));
                return;
            }
            let c = &mut self.canales[i];
            if let Some(k) = c.al_final.iter().position(|a| matches!(a, AlFinal::Esperar(x) | AlFinal::Acabar(x) | AlFinal::PerVol(x, ..) if *x == id)) {
                c.al_final.remove(k);
                self.avisos.push(hecha(id, IOERR_ABORTED, 1 << i, llave));
                return;
            }
        }
    }

    /// Si una combinación de canales se puede dar a precedencia `prec`: libres, o de menos precedencia.
    fn se_puede(&self, combinacion: u8, prec: i8) -> bool {
        (0..4).filter(|i| combinacion & (1 << i) != 0).all(|i| {
            let c = &self.canales[i];
            c.llave == 0 || c.prec < prec
        })
    }

    fn asignar(&mut self, mut p: Peticion) {
        let combinaciones: Vec<u8> = p.datos.iter().map(|&c| c as u8 & 0xf).collect();
        let Some(&comb) = combinaciones.iter().find(|&&c| self.se_puede(c, p.prec)) else {
            if p.banderas & ADIOF_NOWAIT != 0 {
                return self.avisos.push(hecha(p.id, ADIOERR_ALLOCFAILED, 0, p.llave));
            }
            return self.esperando.push_back(p);
        };
        // Los que tienen cerrojo no se quitan: se avisa a su dueño, que los suelta con ADCMD_FREE; la asignación
        // espera (o falla, sin esperar).
        // Un canal ya avisado (`pedido`) sigue esperando a que lo suelten.
        let con_cerrojo: Vec<usize> = (0..4)
            .filter(|&i| comb & (1 << i) != 0 && self.canales[i].llave != 0 && (self.canales[i].cerrojo.is_some() || self.canales[i].pedido))
            .collect();
        if !con_cerrojo.is_empty() {
            for &i in &con_cerrojo {
                self.avisar_cerrojo(i);
                self.canales[i].pedido = true;
            }
            if p.banderas & ADIOF_NOWAIT != 0 {
                return self.avisos.push(hecha(p.id, ADIOERR_ALLOCFAILED, 0, p.llave));
            }
            return self.esperando.push_back(p);
        }
        if p.llave == 0 {
            p.llave = self.siguiente_llave;
            self.siguiente_llave = self.siguiente_llave.wrapping_add(1).max(1);
        }
        for i in 0..4 {
            if comb & (1 << i) == 0 {
                continue;
            }
            if self.canales[i].llave != 0 {
                // Robado: lo que tenía su dueño vuelve con error.
                self.vaciar(i, ADIOERR_CHANNELSTOLEN);
            }
            let c = &mut self.canales[i];
            c.llave = p.llave;
            c.prec = p.prec;
            c.pedido = false;
            c.parado = false;
        }
        self.avisos.push(hecha(p.id, 0, comb, p.llave));
    }

    /// Vuelve el cerrojo del canal `i` (y de los demás canales con la misma petición).
    fn avisar_cerrojo(&mut self, i: usize) {
        let Some(id) = self.canales[i].cerrojo else { return };
        let llave = self.canales[i].llave;
        let mut unidad = 0;
        for (j, c) in self.canales.iter_mut().enumerate() {
            if c.cerrojo == Some(id) {
                c.cerrojo = None;
                unidad |= 1 << j;
            }
        }
        self.avisos.push(hecha(id, ADIOERR_CHANNELSTOLEN, unidad, llave));
    }

    /// Vuelve a mirar las asignaciones que esperan, en orden.
    fn repartir(&mut self) {
        let esperando = core::mem::take(&mut self.esperando);
        for p in esperando {
            self.asignar(p);
        }
    }

    /// Suelta el canal `i`: lo suyo vuelve abortado, y su cerrojo también.
    fn soltar(&mut self, i: usize) {
        self.vaciar(i, IOERR_ABORTED);
        if let Some(id) = self.canales[i].cerrojo.take() {
            let llave = self.canales[i].llave;
            self.avisos.push(hecha(id, 0, 1 << i, llave));
        }
        let c = &mut self.canales[i];
        c.llave = 0;
        c.prec = 0;
        c.pedido = false;
    }

    /// Vacía el canal: la escritura que suena, las de la cola y lo que esperaba al final del ciclo vuelven con `error`.
    fn vaciar(&mut self, i: usize, error: i8) {
        let llave = self.canales[i].llave;
        let c = &mut self.canales[i];
        let mut ids: Vec<Id> = c.actual.take().into_iter().chain(c.cola.drain(..)).map(|e| e.id).collect();
        ids.extend(c.al_final.drain(..).map(|a| match a {
            AlFinal::Esperar(x) | AlFinal::Acabar(x) | AlFinal::PerVol(x, ..) => x,
        }));
        c.pos = 0;
        c.hechos = 0;
        for id in ids {
            self.avisos.push(hecha(id, error, 1 << i, llave));
        }
    }

    /// Acaba la escritura que suena en `i` con `error` y pasa a la siguiente.
    fn acabar_actual(&mut self, i: usize, error: i8) {
        let llave = self.canales[i].llave;
        if let Some(e) = self.canales[i].actual.take() {
            self.avisos.push(hecha(e.id, error, 1 << i, llave));
        }
        self.canales[i].pos = 0;
        self.canales[i].hechos = 0;
        self.empezar(i);
    }

    /// Si el canal `i` no suena, empieza su siguiente escritura.
    fn empezar(&mut self, i: usize) {
        let c = &mut self.canales[i];
        if c.actual.is_some() {
            return;
        }
        let Some(e) = c.cola.pop_front() else { return };
        if let Some((per, vol)) = e.pervol {
            c.periodo = per;
            c.volumen = vol;
        }
        if e.mensaje {
            self.avisos.push(Aviso::Empieza { id: e.id });
        }
        c.hechos = 0;
        c.actual = Some(e);
    }

    /// El final de un ciclo del canal `i`: lo que esperaba, y si la escritura acabó, la siguiente.
    fn fin_de_ciclo(&mut self, i: usize) {
        let llave = self.canales[i].llave;
        let mut acabar = false;
        for a in core::mem::take(&mut self.canales[i].al_final) {
            match a {
                AlFinal::Esperar(id) => self.avisos.push(hecha(id, 0, 1 << i, llave)),
                AlFinal::Acabar(id) => {
                    acabar = true;
                    self.avisos.push(hecha(id, 0, 1 << i, llave));
                }
                AlFinal::PerVol(id, per, vol) => {
                    self.canales[i].periodo = per;
                    self.canales[i].volumen = vol;
                    self.avisos.push(hecha(id, 0, 1 << i, llave));
                }
            }
        }
        let c = &mut self.canales[i];
        c.hechos = c.hechos.saturating_add(1);
        let ciclos = c.actual.as_ref().map_or(0, |e| e.ciclos);
        if acabar {
            self.acabar_actual(i, IOERR_ABORTED);
        } else if ciclos != 0 && c.hechos >= ciclos {
            self.acabar_actual(i, 0);
        }
    }

    /// Genera `salida.len()` cuadros estéreo de 16 bits a la frecuencia de la salida.
    pub fn mezclar(&mut self, salida: &mut [[i16; 2]]) {
        for cuadro in salida.iter_mut() {
            let mut lados = [0i32; 2];
            for i in 0..4 {
                let lado = if i == 0 || i == 3 { 0 } else { 1 };
                lados[lado] += self.muestra(i);
            }
            // Cada canal da ±8.192 (128 × 64); dos por lado llenan los 16 bits.
            *cuadro = lados.map(|v| (v * 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16);
        }
    }

    /// La muestra del canal `i` para este cuadro, y avanza.
    fn muestra(&mut self, i: usize) -> i32 {
        let c = &self.canales[i];
        if c.parado {
            return 0;
        }
        let Some(e) = &c.actual else { return 0 };
        let v = e.datos[(c.pos >> 32) as usize] as i32 * c.volumen.min(64) as i32;
        // Avanza: (reloj / período) muestras por segundo, en cuadros de la salida.
        let periodo = c.periodo.max(PERIODO_MINIMO) as u64;
        let paso = (self.reloj << 32) / (periodo * self.frecuencia as u64);
        self.canales[i].pos += paso;
        // Puede acabar más de un ciclo en un cuadro con muestras muy cortas. Una escritura nueva empieza desde su
        // principio (`acabar_actual` deja la posición a 0).
        loop {
            let c = &mut self.canales[i];
            let Some(e) = &c.actual else { break };
            let largo = e.datos.len() as u64;
            if (c.pos >> 32) < largo {
                break;
            }
            c.pos -= largo << 32;
            self.fin_de_ciclo(i);
        }
        v
    }
}

pub mod ahi;

#[cfg(test)]
mod pruebas;
#[cfg(test)]
mod pruebas_ahi;
