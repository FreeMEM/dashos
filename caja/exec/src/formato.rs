//! `RawDoFmt`: el formato de exec (`%[-][0][ancho][.límite][l]tipo`, tipos `d u x X c s b`, datos de 16 bits sin
//! `l`), escrito del autodoc de exec (NDK 3.2, `exec.doc`). Lo mismo que hace dos para `VPrintf` (`dos/src/texto.rs`),
//! sobre la memoria de exec.
//!
//! El texto se hace en nativo y después se entrega: con `PutChProc` a 0 (V45.1 y después) o con el `stuffChar` del
//! autodoc (`move.b d0,(a3)+` / `rts`), se escribe directamente en `PutChData`; con otra función de 68k, se la llama
//! una vez por carácter (y una más con el 0 del final), cada vez con el A3 que dejó la anterior.

use alloc::vec::Vec;

use crate::estructuras::memf;
use crate::{Accion, Continuacion, Destino, Exec, Fallo, Memoria, Registros};

/// Tope del texto de un formato (lo demás se corta).
const MAX_TEXTO: usize = 64 << 10;

/// `move.b d0,(a3)+` y `rts`: el `stuffChar` del autodoc.
const STUFF_CHAR: u32 = 0x16C0_4E75;

/// Lo que queda por entregar a un `PutChProc` de 68k.
pub(crate) struct Entrega {
    texto: Vec<u8>,
    pos: usize,
    proc: u32,
    siguiente: u32,
}

impl<M: Memoria> Exec<M> {
    /// El texto de `formato` con los datos de `datos`, y dónde acaban los datos.
    pub(crate) fn formatear(&self, formato: u32, datos: u32) -> Result<(Vec<u8>, u32), Fallo> {
        let f = self.cadena(formato)?;
        let mut out = Vec::new();
        let mut a = datos;
        let mut i = 0;
        while i < f.len() && out.len() < MAX_TEXTO {
            if f[i] != b'%' {
                out.push(f[i]);
                i += 1;
                continue;
            }
            i += 1;
            let (mut izquierda, mut ceros) = (false, false);
            while i < f.len() && (f[i] == b'-' || f[i] == b'0') {
                if f[i] == b'-' {
                    izquierda = true;
                } else {
                    ceros = true;
                }
                i += 1;
            }
            let mut ancho = 0usize;
            while i < f.len() && f[i].is_ascii_digit() {
                ancho = (ancho * 10 + (f[i] - b'0') as usize).min(1024);
                i += 1;
            }
            let mut limite = usize::MAX;
            if i < f.len() && f[i] == b'.' {
                i += 1;
                let mut l = 0usize;
                while i < f.len() && f[i].is_ascii_digit() {
                    l = (l * 10 + (f[i] - b'0') as usize).min(1 << 16);
                    i += 1;
                }
                limite = l;
            }
            let mut largo = false;
            if i < f.len() && f[i] == b'l' {
                largo = true;
                i += 1;
            }
            let Some(&tipo) = f.get(i) else { break };
            i += 1;
            let mut leer = |e: &Self, largo: bool| -> Result<u32, Fallo> {
                if largo {
                    let v = e.l32(a)?;
                    a = a.wrapping_add(4);
                    Ok(v)
                } else {
                    let v = e.l16(a)?;
                    a = a.wrapping_add(2);
                    Ok(v as u32)
                }
            };
            let mut campo: Vec<u8> = match tipo {
                b'%' => {
                    out.push(b'%');
                    continue;
                }
                b'd' | b'D' => {
                    let v = leer(self, largo)?;
                    let v = if largo { v as i32 as i64 } else { v as u16 as i16 as i64 };
                    alloc::format!("{v}").into_bytes()
                }
                b'u' | b'U' => alloc::format!("{}", leer(self, largo)?).into_bytes(),
                // %x en mayúsculas, como en AmigaOS (el oráculo: pruebas68k/formato.c).
                b'x' => alloc::format!("{:X}", leer(self, largo)?).into_bytes(),
                b'X' => alloc::format!("{:X}", leer(self, largo)?).into_bytes(),
                b'c' => alloc::vec![leer(self, largo)? as u8],
                b's' => {
                    let p = leer(self, true)?;
                    if p == 0 { Vec::new() } else { self.cadena(p)? }
                }
                b'b' => {
                    let p = leer(self, true)? << 2;
                    if p == 0 {
                        Vec::new()
                    } else {
                        let n = self.l8(p)? as u32;
                        (1..=n).map(|k| self.l8(p + k)).collect::<Result<Vec<u8>, Fallo>>()?
                    }
                }
                // Un tipo que no se conoce sale tal cual.
                otro => alloc::vec![b'%', otro],
            };
            if matches!(tipo, b's' | b'b') {
                campo.truncate(limite);
            }
            if campo.len() < ancho {
                let relleno = ancho - campo.len();
                if izquierda {
                    campo.resize(ancho, b' ');
                } else {
                    let c = if ceros && !matches!(tipo, b's' | b'b' | b'c') { b'0' } else { b' ' };
                    let mut v = alloc::vec![c; relleno];
                    // Con ceros, el signo va delante de ellos.
                    if c == b'0' && campo.first() == Some(&b'-') {
                        v[0] = b'-';
                        campo[0] = b'0';
                    }
                    v.extend_from_slice(&campo);
                    campo = v;
                }
            }
            out.extend_from_slice(&campo);
        }
        out.truncate(MAX_TEXTO);
        Ok((out, a))
    }

    /// `RawDoFmt(FormatString, DataStream, PutChProc, PutChData)(a0/a1/a2/a3)`: D0 = el final de los datos.
    pub(crate) fn raw_do_fmt(&mut self, r: &mut Registros) -> Result<Accion, Fallo> {
        let (formato, datos, proc, dest) = (r.a[0], r.a[1], r.a[2], r.a[3]);
        let (mut texto, siguiente) = self.formatear(formato, datos)?;
        texto.push(0);
        if proc == 0 || self.l32(proc)? == STUFF_CHAR {
            self.comprobar(dest, texto.len() as u32, true)?;
            self.mem.leer_mut(dest, texto.len() as u32)?.copy_from_slice(&texto);
            r.a[3] = dest.wrapping_add(texto.len() as u32);
            r.d[0] = siguiente;
            return Ok(Accion::Seguir);
        }
        // Otra función: un carácter cada vez.
        let id = self.siguiente_entrega();
        self.entregas.insert(id, Entrega { texto, pos: 0, proc, siguiente });
        Ok(self.entregar_caracter(id, r, dest))
    }

    fn siguiente_entrega(&mut self) -> u32 {
        // Con tope: un PutChProc que nunca vuelve no llena la memoria.
        if self.entregas.len() > 16 {
            self.entregas.clear();
        }
        self.n_entrega = self.n_entrega.wrapping_add(1);
        self.n_entrega
    }

    /// El siguiente carácter de la entrega `id` a su `PutChProc` (D0 = el carácter, A3 = `a3`), o el final.
    pub(crate) fn entregar_caracter(&mut self, id: u32, r: &mut Registros, a3: u32) -> Accion {
        let Some(e) = self.entregas.get_mut(&id) else {
            return Accion::Seguir;
        };
        if e.pos >= e.texto.len() {
            let siguiente = e.siguiente;
            self.entregas.remove(&id);
            r.d[0] = siguiente;
            return Accion::Seguir;
        }
        let ch = e.texto[e.pos];
        e.pos += 1;
        let proc = e.proc;
        let mut rr = *r;
        rr.d[0] = ch as u32;
        rr.a[3] = a3;
        Accion::Llamar { destino: Destino::Codigo68k(proc), regs: rr, tras: Continuacion::Entrega { id } }
    }
}

/// Para las pruebas: reserva memoria (como `AllocMem` del sistema) para un `stuffChar` de 68k.
#[doc(hidden)]
pub fn stuff_char<M: Memoria>(e: &mut Exec<M>) -> Result<u32, Fallo> {
    let d = e.alloc_mem(4, memf::PUBLIC)?;
    e.mem.escribir32(d, STUFF_CHAR)?;
    Ok(d)
}
