//! La tabla de llamadas: número de función nativa → función de exec, con los registros del 68k.
//!
//! El número es el de la entrada en la tabla de saltos (`lvo.rs`): AllocMem es la 33, en -198. Cada función lee
//! sus argumentos de los registros del `.fd` y deja el resultado en D0. Un fallo se apunta como aviso y la
//! función devuelve 0 en D0 (lo que devolvería exec al no poder), sin tumbar la caja.

use crate::lvo::{self, FIN_DE_TAREA, ULTIMO_DE_EXEC};
use crate::tareas::Espera;
use crate::{Accion, Exec, Fallo, Memoria, Registros};

fn d0(r: &mut Registros, v: u32) -> Result<Accion, Fallo> {
    r.d[0] = v;
    Ok(Accion::Seguir)
}

fn espera(e: Espera, r: &mut Registros) -> Accion {
    match e {
        Espera::Hecho(v) => {
            r.d[0] = v;
            Accion::Seguir
        }
        Espera::Bloquea => Accion::Bloquear,
    }
}

impl<M: Memoria> Exec<M> {
    /// Llama a la función nativa `n` de exec con los registros de la tarea actual. Es lo que hace la caja cuando
    /// el JIT encuentra `$A0FF n` y `n` es de exec (`n <= lvo::ULTIMO_DE_EXEC`); con otro número devuelve
    /// `Accion::Ajena`.
    pub fn llamar(&mut self, n: u16, r: &mut Registros) -> Accion {
        if n > ULTIMO_DE_EXEC {
            return Accion::Ajena;
        }
        match self.ejecutar(n, r) {
            Ok(a) => a,
            Err(f) => {
                self.avisar(lvo::nombre(n), f);
                r.d[0] = 0;
                Accion::Seguir
            }
        }
    }

    fn ejecutar(&mut self, n: u16, r: &mut Registros) -> Result<Accion, Fallo> {
        let (d, a) = (r.d, r.a);
        match n {
            // Open, Close, Expunge y la reservada de cualquier biblioteca nativa (A6 = su base).
            1 => {
                let v = self.lib_open(a[6])?;
                d0(r, v)
            }
            2 => {
                let v = self.lib_close(a[6])?;
                d0(r, v)
            }
            3 => {
                let v = self.lib_expunge(a[6])?;
                d0(r, v)
            }
            4 => d0(r, 0),
            // InitStruct(initTable, memory, size)(a1/a2/d0)
            13 => {
                self.init_struct(a[1], a[2], d[0])?;
                Ok(Accion::Seguir)
            }
            // MakeLibrary(vectors, structure, init, dataSize, segList)(a0/a1/a2/d0/d1)
            14 => self.make_library(a[0], a[1], a[2], d[0], d[1], r),
            // MakeFunctions(target, functionArray, funcDispBase)(a0/a1/a2)
            15 => {
                let v = self.make_functions(a[0], a[1], a[2])?;
                d0(r, v)
            }
            20 => {
                self.disable()?;
                Ok(Accion::Seguir)
            }
            21 => self.enable(),
            22 => {
                self.forbid()?;
                Ok(Accion::Seguir)
            }
            23 => self.permit(),
            // Allocate(freeList, byteSize)(a0/d0)
            31 => {
                let v = self.allocate(a[0], d[0], false)?;
                d0(r, v)
            }
            // Deallocate(freeList, memoryBlock, byteSize)(a0/a1/d0)
            32 => {
                self.deallocate(a[0], a[1], d[0])?;
                Ok(Accion::Seguir)
            }
            // AllocMem(byteSize, requirements)(d0/d1)
            33 => {
                let v = self.alloc_mem(d[0], d[1])?;
                d0(r, v)
            }
            // AllocAbs(byteSize, location)(d0/a1)
            34 => {
                let v = self.alloc_abs(d[0], a[1])?;
                d0(r, v)
            }
            // FreeMem(memoryBlock, byteSize)(a1/d0)
            35 => {
                self.free_mem(a[1], d[0])?;
                Ok(Accion::Seguir)
            }
            // AvailMem(requirements)(d1)
            36 => {
                let v = self.avail_mem(d[1])?;
                d0(r, v)
            }
            // AllocEntry(memList)(a0)
            37 => {
                let v = self.alloc_entry(a[0])?;
                d0(r, v)
            }
            // FreeEntry(memList)(a0)
            38 => {
                self.free_entry(a[0])?;
                Ok(Accion::Seguir)
            }
            // Insert(list, node, pred)(a0/a1/a2)
            39 => {
                self.insert(a[0], a[1], a[2])?;
                Ok(Accion::Seguir)
            }
            40 => {
                self.add_head(a[0], a[1])?;
                Ok(Accion::Seguir)
            }
            41 => {
                self.add_tail(a[0], a[1])?;
                Ok(Accion::Seguir)
            }
            42 => {
                self.remove(a[1])?;
                Ok(Accion::Seguir)
            }
            43 => {
                let v = self.rem_head(a[0])?;
                d0(r, v)
            }
            44 => {
                let v = self.rem_tail(a[0])?;
                d0(r, v)
            }
            45 => {
                self.enqueue(a[0], a[1])?;
                Ok(Accion::Seguir)
            }
            // FindName(list, name)(a0/a1)
            46 => {
                let nombre = self.cadena(a[1])?;
                let v = self.find_name(a[0], &nombre)?;
                d0(r, v)
            }
            // AddTask(task, initPC, finalPC)(a1/a2/a3): D0 la tarea (V36).
            47 => {
                let t = self.add_task(a[1], a[2], a[3])?;
                r.d[0] = t;
                self.hay_que_ceder()
            }
            48 => self.rem_task(a[1]),
            49 => {
                let v = self.find_task(a[1])?;
                d0(r, v)
            }
            // SetTaskPri(task, priority)(a1/d0)
            50 => {
                let (antes, acc) = self.set_task_pri(a[1], d[0] as i8)?;
                r.d[0] = antes as i32 as u32;
                Ok(acc)
            }
            // SetSignal(newSignals, signalSet)(d0/d1)
            51 => {
                let v = self.set_signal(d[0], d[1])?;
                d0(r, v)
            }
            53 => {
                let e = self.wait(d[0])?;
                Ok(espera(e, r))
            }
            // Signal(task, signalSet)(a1/d0)
            54 => self.signal(a[1], d[0]),
            // AllocSignal(signalNum)(d0): BYTE, -1 = cualquiera.
            55 => {
                let v = self.alloc_signal(d[0] as i8)?;
                d0(r, v as i32 as u32)
            }
            56 => {
                self.free_signal(d[0] as i8 as i32)?;
                Ok(Accion::Seguir)
            }
            59 => {
                self.add_port(a[1])?;
                Ok(Accion::Seguir)
            }
            60 => {
                self.rem_port(a[1])?;
                Ok(Accion::Seguir)
            }
            // PutMsg(port, message)(a0/a1)
            61 => self.put_msg(a[0], a[1]),
            62 => {
                let v = self.get_msg(a[0])?;
                d0(r, v)
            }
            63 => self.reply_msg(a[1]),
            64 => {
                let e = self.wait_port(a[0])?;
                Ok(espera(e, r))
            }
            65 => {
                let v = self.find_port(a[1])?;
                d0(r, v)
            }
            66 => {
                self.add_library(a[1])?;
                Ok(Accion::Seguir)
            }
            67 => self.rem_library(a[1], r),
            // OldOpenLibrary(libName)(a1)
            68 => self.open_library(a[1], 0, r),
            69 => self.close_library(a[1], r),
            // SetFunction(library, funcOffset, newFunction)(a1/a0/d0)
            70 => {
                let v = self.set_function(a[1], a[0] as i32, d[0])?;
                d0(r, v)
            }
            71 => {
                self.sum_library(a[1])?;
                Ok(Accion::Seguir)
            }
            89 => {
                let v = self.type_of_mem(a[1])?;
                d0(r, v)
            }
            // OpenLibrary(libName, version)(a1/d0)
            92 => self.open_library(a[1], d[0], r),
            // CopyMem / CopyMemQuick(source, dest, size)(a0/a1/d0)
            104 | 105 => {
                self.copy_mem(a[0], a[1], d[0])?;
                Ok(Accion::Seguir)
            }
            // CacheClearU() y CacheClearE(address, length, caches)(a0/d0/d1): el JIT olvida lo traducido.
            106 => {
                self.codigo_cambiado.push((0, 0));
                Ok(Accion::Seguir)
            }
            107 => {
                self.codigo_cambiado.push((a[0], a[0].saturating_add(d[0])));
                Ok(Accion::Seguir)
            }
            111 => {
                let v = self.create_msg_port()?;
                d0(r, v)
            }
            112 => {
                self.delete_msg_port(a[0])?;
                Ok(Accion::Seguir)
            }
            // AllocVec(byteSize, requirements)(d0/d1)
            114 => {
                let v = self.alloc_vec(d[0], d[1])?;
                d0(r, v)
            }
            115 => {
                self.free_vec(a[1])?;
                Ok(Accion::Seguir)
            }
            FIN_DE_TAREA => self.rem_task(0),
            _ => Err(Fallo::NoImplementada { n }),
        }
    }
}
