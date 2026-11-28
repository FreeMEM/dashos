//! La tabla de llamadas: número de entrada → función de dos, con los registros del 68k.
//!
//! El número es el de la entrada en la tabla de saltos (`lvo.rs`): Open es la 5, en -30. Cada función lee sus
//! argumentos de los registros del `.fd` (D1 a D7) y deja el resultado en D0. Si falla, `IoErr()` queda con el error
//! del Amiga y D0 con lo que devuelve esa función al fallar (0, o -1 en Read, Write, Seek...). Un fallo del programa
//! (un puntero malo, un cerrojo que dos no dio) se apunta además como aviso. Nada tumba la caja.

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::SistemaArchivos;
use crate::lvo::*;
use crate::*;

/// Lo que devuelve en D0 cada función al fallar.
fn valor_de_fallo(n: u16) -> u32 {
    match n {
        READ
        | WRITE
        | SEEK
        | SET_FILE_SIZE
        | FGET_C
        | FPUT_C
        | FPUTS
        | VFPRINTF
        | VFWRITEF
        | VPRINTF
        | PUT_STR
        | WRITE_CHARS
        | GET_VAR
        | SPLIT_NAME
        | STR_TO_LONG
        | FIND_ARG
        | SAME_LOCK
        | SYSTEM_TAG_LIST
        | RUN_COMMAND
        | PARSE_PATTERN
        | PARSE_PATTERN_NO_CASE => u32::MAX,
        _ => 0,
    }
}

/// Tipos propios en la tabla de objetos para MakeDosEntry.
const OBJ_DOSLIST: u32 = 0xD05_0000;
const OBJ_NOMBRE: u32 = 0xD05_0001;

impl<F: SistemaArchivos> Dos<F> {
    /// Llama a la función `n` de dos (número de entrada de la tabla de saltos) con los registros de la tarea en
    /// marcha. Con un número que no es de dos (las cuatro de toda biblioteca, que hace exec, u otro) devuelve
    /// `Accion::Ajena` sin tocar nada.
    pub fn llamar<C: Caja>(&mut self, c: &mut C, n: u16, r: &mut Registros) -> Accion {
        if !(PRIMERA..=ULTIMA).contains(&n) {
            return Accion::Ajena;
        }
        match self.ejecutar(c, n, r) {
            Ok(a) => a,
            Err(e) => {
                let codigo = match e {
                    E::Io(x) => x,
                    E::Mal(f) => {
                        self.avisar(c, nombre(n), f);
                        f.codigo()
                    }
                };
                self.poner_io_err(c, codigo);
                r.d[0] = valor_de_fallo(n);
                Accion::Seguir
            }
        }
    }

    fn ejecutar<C: Caja>(&mut self, c: &mut C, n: u16, r: &mut Registros) -> R<Accion> {
        let d = r.d;
        let mut d0 = |v: u32| -> R<Accion> {
            r.d[0] = v;
            Ok(Accion::Seguir)
        };
        match n {
            OPEN => {
                let nombre = Self::cadena(c, d[1])?;
                let fh = self.open(c, &nombre, d[2] as i32)?;
                d0(fh >> 2)
            }
            CLOSE => {
                if d[1] != 0 {
                    let a = self.fichero(d[1])?;
                    self.cerrar_fichero(c, a)?;
                }
                d0(DOSTRUE)
            }
            READ => d0(self.read(c, d[1], d[2], d[3])?),
            WRITE => d0(self.write(c, d[1], d[2], d[3])?),
            INPUT => d0(self.campo(c, process::PR_CIS)),
            OUTPUT => d0(self.campo(c, process::PR_COS)),
            SEEK => d0(self.seek(c, d[1], d[2], d[3])?),
            DELETE_FILE => {
                let nombre = Self::cadena(c, d[1])?;
                self.delete_file(c, &nombre)?;
                d0(DOSTRUE)
            }
            RENAME => {
                let viejo = Self::cadena(c, d[1])?;
                let nuevo = Self::cadena(c, d[2])?;
                self.rename(c, &viejo, &nuevo)?;
                d0(DOSTRUE)
            }
            LOCK => {
                let nombre = Self::cadena(c, d[1])?;
                let l = self.lock_ruta(c, &nombre, d[2] as i32)?;
                d0(l >> 2)
            }
            UN_LOCK => {
                self.unlock(c, d[1])?;
                d0(DOSTRUE)
            }
            DUP_LOCK => d0(self.dup_lock(c, d[1])?),
            EXAMINE => {
                self.examine(c, d[1], d[2])?;
                d0(DOSTRUE)
            }
            EX_NEXT => {
                self.ex_next(c, d[1], d[2])?;
                d0(DOSTRUE)
            }
            INFO => {
                self.info(c, d[1], d[2])?;
                d0(DOSTRUE)
            }
            CREATE_DIR => {
                let nombre = Self::cadena(c, d[1])?;
                let l = self.crear_cajon(c, &nombre)?;
                d0(l >> 2)
            }
            CURRENT_DIR => d0(self.current_dir(c, d[1])?),
            IO_ERR => d0(self.io_err(c)),
            LOAD_SEG => d0(self.load_seg(c, d[1])?),
            // InternalLoadSeg(fh, table, funcarray, stack)(d0/a0/a1/a2): con un fichero de dos, se lee desde donde
            // esté hasta el final y se carga como LoadSeg (las funciones de `funcarray` no se llaman: las de dos hacen
            // lo mismo). Con otra cosa (un flujo propio leído por funciones 68k), aún no: aviso y 0.
            INTERNAL_LOAD_SEG => {
                let fh = d[0];
                d0(self.internal_load_seg(c, fh)?)
            }
            INTERNAL_UN_LOAD_SEG => {
                self.unload_seg(c, d[1])?;
                d0(DOSTRUE)
            }
            UN_LOAD_SEG => {
                self.unload_seg(c, d[1])?;
                d0(DOSTRUE)
            }
            SET_COMMENT => {
                let nombre = Self::cadena(c, d[1])?;
                let cm = Self::cadena(c, d[2])?;
                self.set_comment(c, &nombre, &cm)?;
                d0(DOSTRUE)
            }
            SET_PROTECTION => {
                let nombre = Self::cadena(c, d[1])?;
                self.set_protection(c, &nombre, d[2])?;
                d0(DOSTRUE)
            }
            DATE_STAMP => {
                Self::tramo(c, d[1], datestamp::TAMANO)?;
                let (s, us) = c.hora();
                Self::poner_fecha(c, d[1], s, us)?;
                d0(d[1])
            }
            DATE_TO_STR => d0(self.date_to_str(c, d[1])?),
            DELAY => {
                r.d[0] = 0;
                Ok(if (d[1] as i32) > 0 { Accion::Dormir { tics: d[1] } } else { Accion::Seguir })
            }
            WAIT_FOR_CHAR => {
                self.fichero(d[1])?;
                d0(DOSFALSE)
            }
            PARENT_DIR => d0(self.parent_dir(c, d[1])?),
            IS_INTERACTIVE => {
                let a = self.fichero(d[1])?;
                d0(if matches!(self.ficheros[&a].flujo, Flujo::Consola) { DOSTRUE } else { DOSFALSE })
            }
            ALLOC_DOS_OBJECT => d0(self.alloc_dos_object(c, d[1])?),
            FREE_DOS_OBJECT => {
                self.free_dos_object(c, d[1], d[2])?;
                d0(0)
            }
            SELECT_INPUT => {
                let v = self.campo(c, process::PR_CIS);
                self.poner_campo(c, process::PR_CIS, d[1]);
                d0(v)
            }
            SELECT_OUTPUT => {
                let v = self.campo(c, process::PR_COS);
                self.poner_campo(c, process::PR_COS, d[1]);
                d0(v)
            }
            FGET_C => d0(self.fgetc(c, d[1])?),
            FPUT_C => {
                let a = self.fichero(d[1])?;
                if self.escribir_en(c, a, &[d[2] as u8])? != 1 {
                    return Err(E::Io(ERROR_DISK_FULL));
                }
                d0(d[2] & 0xFF)
            }
            UN_GET_C => d0(self.ungetc(d[1], d[2])?),
            FREAD => d0(self.fread_fwrite(c, d[1], d[2], d[3], d[4], false)?),
            FWRITE => d0(self.fread_fwrite(c, d[1], d[2], d[3], d[4], true)?),
            FGETS => d0(self.fgets(c, d[1], d[2], d[3])?),
            FPUTS => {
                let a = self.fichero(d[1])?;
                let s = Self::cadena(c, d[2])?;
                self.escribir_en(c, a, &s)?;
                d0(0)
            }
            VFPRINTF => {
                let a = self.fichero(d[1])?;
                let t = Self::formatear(c, d[2], d[3])?;
                d0(self.escribir_en(c, a, &t)?)
            }
            FLUSH => {
                self.flush(d[1])?;
                d0(DOSTRUE)
            }
            SET_VBUF => {
                self.fichero(d[1])?;
                d0(0)
            }
            DUP_LOCK_FROM_FH => d0(self.dup_lock_from_fh(c, d[1])?),
            OPEN_FROM_LOCK => d0(self.open_from_lock(c, d[1])? >> 2),
            PARENT_OF_FH => d0(self.parent_of_fh(c, d[1])?),
            EXAMINE_FH => {
                self.examine_fh(c, d[1], d[2])?;
                d0(DOSTRUE)
            }
            SET_FILE_DATE => {
                let nombre = Self::cadena(c, d[1])?;
                self.set_file_date(c, &nombre, d[2])?;
                d0(DOSTRUE)
            }
            NAME_FROM_LOCK => {
                self.name_from_lock(c, d[1], d[2], d[3])?;
                d0(DOSTRUE)
            }
            NAME_FROM_FH => {
                self.name_from_fh(c, d[1], d[2], d[3])?;
                d0(DOSTRUE)
            }
            SPLIT_NAME => d0(Self::split_name(c, d[1], d[2], d[3], d[4], d[5])?),
            SAME_LOCK => d0(self.same_lock(d[1], d[2])? as u32),
            SET_MODE => {
                self.fichero(d[1])?;
                d0(DOSTRUE)
            }
            SET_FILE_SIZE => d0(self.set_file_size(c, d[1], d[2], d[3])?),
            SET_IO_ERR => {
                let v = self.io_err(c);
                self.poner_io_err(c, d[1]);
                d0(v)
            }
            FAULT => d0(Self::fault(c, d[1], d[2], d[3], d[4])?),
            PRINT_FAULT => {
                if d[1] != 0 {
                    let mut t = Self::texto_de_fallo(c, d[1], d[2])?;
                    t.push(b'\n');
                    self.a_salida(c, &t)?;
                }
                self.poner_io_err(c, d[1]);
                d0(DOSTRUE)
            }
            CLI => {
                let b = self.campo(c, process::PR_CLI);
                d0(Self::baddr(b).unwrap_or(0))
            }
            GET_CONSOLE_TASK => d0(self.campo(c, process::PR_CONSOLETASK)),
            GET_FILE_SYS_TASK => d0(self.campo(c, process::PR_FILESYSTEMTASK)),
            GET_ARG_STR => d0(self.campo(c, process::PR_ARGUMENTS)),
            GET_PROGRAM_DIR => d0(self.campo(c, process::PR_HOMEDIR)),
            SET_CONSOLE_TASK | SET_FILE_SYS_TASK | SET_ARG_STR | SET_PROGRAM_DIR => {
                let campo = match n {
                    SET_CONSOLE_TASK => process::PR_CONSOLETASK,
                    SET_FILE_SYS_TASK => process::PR_FILESYSTEMTASK,
                    SET_ARG_STR => process::PR_ARGUMENTS,
                    _ => process::PR_HOMEDIR,
                };
                let v = self.campo(c, campo);
                self.poner_campo(c, campo, d[1]);
                d0(v)
            }
            GET_PROGRAM_NAME | GET_CURRENT_DIR_NAME | GET_PROMPT => {
                let campo = match n {
                    GET_PROGRAM_NAME => cli::CLI_COMMANDNAME,
                    GET_CURRENT_DIR_NAME => cli::CLI_SETNAME,
                    _ => cli::CLI_PROMPT,
                };
                d0(self.texto_de_cli(c, campo, d[1], d[2])?)
            }
            ASSIGN_LOCK => {
                let nombre = Self::cadena(c, d[1])?;
                let l = Self::baddr(d[2])?;
                self.assign_lock(c, &nombre, l)?;
                d0(DOSTRUE)
            }
            ASSIGN_LATE | ASSIGN_PATH => {
                let nombre = Self::cadena(c, d[1])?;
                let ruta = Self::cadena(c, d[2])?;
                let destino = if n == ASSIGN_LATE { Destino::Tarde(ruta) } else { Destino::Ruta(ruta) };
                self.assign_texto(c, &nombre, destino)?;
                d0(DOSTRUE)
            }
            ASSIGN_ADD => {
                let nombre = Self::cadena(c, d[1])?;
                let l = Self::baddr(d[2])?;
                self.assign_add(c, &nombre, l)?;
                d0(DOSTRUE)
            }
            REM_ASSIGN_LIST => {
                let nombre = Self::cadena(c, d[1])?;
                let l = Self::baddr(d[2])?;
                let quitado = self.rem_assign_list(c, &nombre, l)?;
                d0(if quitado { DOSTRUE } else { DOSFALSE })
            }
            LOCK_DOS_LIST | ATTEMPT_LOCK_DOS_LIST => {
                self.asegurar_cabeza(c)?;
                d0(self.cabeza_doslist)
            }
            UN_LOCK_DOS_LIST => d0(0),
            NEXT_DOS_ENTRY => d0(self.siguiente_doslist(d[1], d[2], None).unwrap_or(0)),
            FIND_DOS_ENTRY => {
                let nombre = Self::cadena(c, d[2])?;
                d0(self.siguiente_doslist(d[1], d[3], Some(&nombre)).unwrap_or(0))
            }
            MAKE_DOS_ENTRY => d0(self.make_dos_entry(c, d[1], d[2])?),
            FREE_DOS_ENTRY => {
                self.free_dos_entry(c, d[1])?;
                d0(0)
            }
            IS_FILE_SYSTEM => {
                let nombre = Self::cadena(c, d[1])?;
                d0(if self.sitios(c, &nombre).is_ok() { DOSTRUE } else { DOSFALSE })
            }
            COMPARE_DATES => {
                let a = Self::fecha_en_tics(c, d[1])?;
                let b = Self::fecha_en_tics(c, d[2])?;
                d0(match a.cmp(&b) {
                    core::cmp::Ordering::Greater => u32::MAX,
                    core::cmp::Ordering::Equal => 0,
                    core::cmp::Ordering::Less => 1,
                })
            }
            CHECK_SIGNAL => d0(c.senales(d[1])),
            READ_ARGS => d0(self.read_args(c, d[1], d[2], d[3])?),
            FIND_ARG => d0(Self::find_arg(c, d[1], d[2])?),
            STR_TO_LONG => d0(Self::str_to_long(c, d[1], d[2])?),
            FREE_ARGS => {
                self.free_args(c, d[1])?;
                d0(0)
            }
            FILE_PART => d0(Self::file_part(c, d[1])?),
            PATH_PART => d0(Self::path_part(c, d[1])?),
            ADD_PART => {
                if !Self::add_part(c, d[1], d[2], d[3])? {
                    return Err(E::Io(ERROR_LINE_TOO_LONG));
                }
                d0(DOSTRUE)
            }
            SET_VAR => {
                self.set_var(c, d[1], d[2], d[3], d[4])?;
                d0(DOSTRUE)
            }
            GET_VAR => d0(self.get_var(c, d[1], d[2], d[3], d[4])?),
            DELETE_VAR => {
                self.delete_var(c, d[1], d[2])?;
                d0(DOSTRUE)
            }
            FIND_VAR => Err(E::Io(ERROR_OBJECT_NOT_FOUND)),
            WRITE_CHARS => {
                if (d[2] as i32) < 0 {
                    return Err(E::Io(ERROR_BAD_NUMBER));
                }
                let b = Self::bytes(c, d[1], d[2])?;
                d0(self.a_salida(c, &b)?)
            }
            PUT_STR => {
                let s = Self::cadena(c, d[1])?;
                self.a_salida(c, &s)?;
                d0(0)
            }
            VPRINTF => {
                let t = Self::formatear(c, d[1], d[2])?;
                d0(self.a_salida(c, &t)?)
            }
            SAME_DEVICE => {
                let a = self.sitio_de_cerrojo(d[1])?;
                let b = self.sitio_de_cerrojo(d[2])?;
                d0(if a.vol == b.vol { DOSTRUE } else { DOSFALSE })
            }
            GET_DEVICE_PROC => d0(self.get_device_proc(c, d[1], d[2])?),
            FREE_DEVICE_PROC => {
                self.free_device_proc(c, d[1]);
                d0(0)
            }
            EXALL => d0(self.ex_all(c, d[1], d[2], d[3], d[4], d[5])?),
            EXALL_END => {
                Self::ex_all_end(c, d[5])?;
                d0(DOSTRUE)
            }
            PARSE_PATTERN | PARSE_PATTERN_NO_CASE => {
                d0(Self::parse_pattern(c, d[1], d[2], d[3], n == PARSE_PATTERN_NO_CASE)?)
            }
            MATCH_FIRST => {
                let r = self.match_first(c, d[1], d[2]);
                d0(self.codigo_busqueda(c, n, r))
            }
            MATCH_NEXT => {
                let r = self.match_next(c, d[1]);
                d0(self.codigo_busqueda(c, n, r))
            }
            MATCH_END => {
                self.match_end(c, d[1]);
                d0(0)
            }
            MATCH_PATTERN | MATCH_PATTERN_NO_CASE => {
                d0(Self::match_pattern(c, d[1], d[2], n == MATCH_PATTERN_NO_CASE)?)
            }
            _ => Err(Fallo::NoImplementada { n }.into()),
        }
    }

    fn fecha_en_tics<C: Caja>(c: &C, ds: u32) -> R<(u32, u32, u32)> {
        Self::comprobar(ds, datestamp::TAMANO)?;
        Ok((
            Self::l32(c, ds + datestamp::DS_DAYS)?,
            Self::l32(c, ds + datestamp::DS_MINUTE)?,
            Self::l32(c, ds + datestamp::DS_TICK)?,
        ))
    }

    /// LoadSeg(name): lee el fichero entero y se lo da al cargador de la caja.
    fn load_seg<C: Caja>(&mut self, c: &mut C, nombre: u32) -> R<u32> {
        let n = Self::cadena(c, nombre)?;
        let datos = self.leer_entero(c, &n, 64 << 20)?;
        let seg = c.cargar_segmentos(&datos).map_err(E::Io)?;
        if seg == 0 {
            return Err(E::Io(ERROR_BAD_HUNK));
        }
        self.segmentos.insert(seg, c.tarea_actual());
        Ok(seg)
    }

    /// InternalLoadSeg con un fichero de dos: lo que queda de él, cargado como LoadSeg.
    fn internal_load_seg<C: Caja>(&mut self, c: &mut C, fh: u32) -> R<u32> {
        let Ok(d) = self.fichero(fh) else {
            return Err(Fallo::NoImplementada { n: INTERNAL_LOAD_SEG }.into());
        };
        // De la consola o de NIL: no hay ejecutable que leer (la consola esperaría al teclado).
        if !matches!(self.ficheros[&d].flujo, Flujo::Disco { .. }) {
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        const TROZO: u32 = 64 << 10;
        const MAX: usize = 64 << 20;
        let buf = Self::reservar(c, TROZO)?;
        let r = (|| -> R<Vec<u8>> {
            let mut v = Vec::new();
            loop {
                let n = self.read(c, fh, buf, TROZO)?;
                if n == 0 || n as i32 == -1 {
                    break;
                }
                v.extend_from_slice(&Self::bytes(c, buf, n)?);
                if v.len() > MAX {
                    return Err(E::Io(ERROR_OBJECT_TOO_LARGE));
                }
            }
            Ok(v)
        })();
        c.liberar(buf, TROZO);
        let datos = r?;
        let seg = c.cargar_segmentos(&datos).map_err(E::Io)?;
        if seg == 0 {
            return Err(E::Io(ERROR_BAD_HUNK));
        }
        self.segmentos.insert(seg, c.tarea_actual());
        Ok(seg)
    }

    /// UnLoadSeg(seglist).
    fn unload_seg<C: Caja>(&mut self, c: &mut C, seg: u32) -> R<()> {
        if seg == 0 {
            return Ok(());
        }
        if self.segmentos.remove(&seg).is_none() {
            return Err(Fallo::ObjetoDesconocido { dir: seg }.into());
        }
        c.descargar_segmentos(seg);
        Ok(())
    }

    /// AllocDosObject(type, tags). Las marcas no se miran.
    fn alloc_dos_object<C: Caja>(&mut self, c: &mut C, tipo: u32) -> R<u32> {
        let largo = match tipo {
            dos_obj::DOS_FIB => fib::TAMANO,
            dos_obj::DOS_FILEHANDLE => filehandle::TAMANO,
            dos_obj::DOS_RDARGS => rdargs::TAMANO,
            dos_obj::DOS_EXALLCONTROL => 16,
            dos_obj::DOS_STDPKT | dos_obj::DOS_CLI => return Err(Fallo::NoImplementada { n: ALLOC_DOS_OBJECT }.into()),
            _ => return Err(E::Io(ERROR_BAD_NUMBER)),
        };
        let d = Self::reservar(c, largo)?;
        self.objetos.insert(d, (tipo, largo));
        Ok(d)
    }

    /// FreeDosObject(type, ptr).
    fn free_dos_object<C: Caja>(&mut self, c: &mut C, tipo: u32, d: u32) -> R<()> {
        if d == 0 {
            return Ok(());
        }
        match self.objetos.get(&d) {
            Some(&(t, largo)) if t == tipo => {
                self.objetos.remove(&d);
                if tipo == dos_obj::DOS_RDARGS {
                    // Lo que ReadArgs reservó con este RDArgs.
                    if let Some((v, _)) = self.argumentos.remove(&d) {
                        for (x, l) in v {
                            c.liberar(x, l);
                        }
                    }
                }
                c.liberar(d, largo);
                Ok(())
            }
            _ => Err(Fallo::ObjetoDesconocido { dir: d }.into()),
        }
    }

    /// MakeDosEntry(name, type): una `DosList` suelta, sin añadir.
    fn make_dos_entry<C: Caja>(&mut self, c: &mut C, nombre: u32, tipo: u32) -> R<u32> {
        let n = Self::cadena(c, nombre)?;
        let n = &n[..n.len().min(255)];
        let dl = Self::reservar(c, doslist::TAMANO)?;
        let largo = n.len() as u32 + 1;
        let b = match Self::reservar(c, largo) {
            Ok(b) => b,
            Err(e) => {
                c.liberar(dl, doslist::TAMANO);
                return Err(e);
            }
        };
        Self::e8(c, b, n.len() as u8)?;
        Self::poner(c, b + 1, n)?;
        Self::e32(c, dl + doslist::DOL_TYPE, tipo)?;
        Self::e32(c, dl + doslist::DOL_NAME, b >> 2)?;
        self.objetos.insert(dl, (OBJ_DOSLIST, doslist::TAMANO));
        self.objetos.insert(b, (OBJ_NOMBRE, largo));
        Ok(dl)
    }

    /// FreeDosEntry(dlist): solo las de MakeDosEntry.
    fn free_dos_entry<C: Caja>(&mut self, c: &mut C, dl: u32) -> R<()> {
        if dl == 0 {
            return Ok(());
        }
        if self.objetos.get(&dl).map(|o| o.0) != Some(OBJ_DOSLIST) {
            return Err(Fallo::ObjetoDesconocido { dir: dl }.into());
        }
        let b = Self::baddr(Self::l32(c, dl + doslist::DOL_NAME)?)?;
        if let Some(&(OBJ_NOMBRE, l)) = self.objetos.get(&b) {
            self.objetos.remove(&b);
            c.liberar(b, l);
        }
        self.objetos.remove(&dl);
        c.liberar(dl, doslist::TAMANO);
        Ok(())
    }

    /// GetProgramName, GetCurrentDirName, GetPrompt: un BSTR del CLI a un búfer. Sin CLI, cadena vacía y DOSFALSE.
    fn texto_de_cli<C: Caja>(&mut self, c: &mut C, campo: u32, buf: u32, largo: u32) -> R<u32> {
        if (largo as i32) <= 0 {
            return Ok(DOSFALSE);
        }
        Self::tramo(c, buf, largo)?;
        let cli = Self::baddr(self.campo(c, process::PR_CLI))?;
        if cli == 0 {
            Self::e8(c, buf, 0)?;
            return Ok(DOSFALSE);
        }
        Self::comprobar(cli, cli::TAMANO)?;
        let b = Self::baddr(Self::l32(c, cli + campo)?)?;
        let t = if b == 0 { alloc::vec::Vec::new() } else { Self::bytes(c, b + 1, Self::l8(c, b)? as u32)? };
        if !Self::poner_cadena(c, buf, largo, &t)? {
            Self::e8(c, buf, 0)?;
            return Err(E::Io(ERROR_LINE_TOO_LONG));
        }
        Ok(DOSTRUE)
    }
}
