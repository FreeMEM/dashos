//! Bibliotecas: la tabla de saltos con las entradas nativas, `OpenLibrary`, `CloseLibrary`, `AddLibrary`,
//! `RemLibrary`, `MakeLibrary`, `MakeFunctions`, `InitStruct`, `SetFunction` y `SumLibrary`.
//!
//! Una biblioteca es su base: detrás, la tabla de saltos (`lib_NegSize` bytes, 6 por entrada); delante, la
//! `struct Library` y sus datos (`lib_PosSize`). En AmigaOS cada entrada es `JMP función`. En las bibliotecas
//! nativas de la caja es `$A0FF n RTS` (S4 §1): el JIT convierte `$A0FF n` en la llamada a la función nativa `n`
//! y luego hace el `RTS`. Es el formato del parche `0002-llamada-nativa-linea-a.patch` (número de 16 bits).
//!
//! `SetFunction` escribe `JMP nueva` en la entrada y devuelve la de antes. Si la de antes era nativa, devuelve su
//! trampolín: una copia de `$A0FF n RTS` en un sitio fijo, para que el parche pueda llamar a la original.
//! Cuando exec llama por la tabla (el `Open` de una biblioteca, por ejemplo), mira la entrada: si es nativa de
//! exec, la llama directamente; si no, pide a la caja que la ejecute (`Accion::Llamar`). Así se respetan los
//! parches, como en MorphOS.

use crate::estructuras::{library, memf, node, nt};
use crate::{Accion, Continuacion, Destino, Exec, Fallo, MAX_VUELTAS, Memoria, Registros, execbase};
use crate::{io, lvo};

/// La instrucción de línea A de las entradas nativas.
pub const LINEA_A_NATIVA: u16 = 0xA0FF;
const JMP_ABS: u16 = 0x4EF9;
const RTS: u16 = 0x4E75;
const TST_L_D0: u16 = 0x4A80;

/// Lo que hay en una entrada de la tabla de saltos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrada {
    /// `$A0FF n RTS`.
    Nativa(u16),
    /// `JMP dirección`.
    Salto(u32),
    /// Otra cosa (código 68k que se ejecuta tal cual).
    Otra,
}

impl<M: Memoria> Exec<M> {
    pub(crate) fn escribir_entrada_nativa(&mut self, d: u32, n: u16) -> Result<(), Fallo> {
        self.e16(d, LINEA_A_NATIVA)?;
        self.e16(d + 2, n)?;
        self.e16(d + 4, RTS)
    }

    fn escribir_salto(&mut self, d: u32, destino: u32) -> Result<(), Fallo> {
        self.e16(d, JMP_ABS)?;
        self.e32(d + 2, destino)
    }

    /// Qué hay en la entrada que empieza en `d`.
    pub fn entrada(&self, d: u32) -> Result<Entrada, Fallo> {
        self.tramo(d, 6)?;
        let op = self.l16(d)?;
        if op == LINEA_A_NATIVA && self.l16(d + 4)? == RTS {
            Ok(Entrada::Nativa(self.l16(d + 2)?))
        } else if op == JMP_ABS {
            Ok(Entrada::Salto(self.l32(d + 2)?))
        } else {
            Ok(Entrada::Otra)
        }
    }

    /// El trampolín (`$A0FF n RTS` fijo) de una función nativa.
    pub fn trampolin(&self, n: u16) -> Option<u32> {
        self.trampolines.get(&n).copied()
    }

    /// Ejecuta el código de `pc` con los registros `rr` y sigue con `tras`. Si `pc` es una entrada nativa de exec,
    /// sin salir de aquí; si no, pidiéndoselo a la caja.
    pub(crate) fn llamar_codigo(
        &mut self,
        pc: u32,
        mut rr: Registros,
        tras: Continuacion,
        r: &mut Registros,
    ) -> Accion {
        match self.entrada(pc) {
            Ok(Entrada::Nativa(n)) if n <= lvo::ULTIMO_DE_EXEC => {
                // Las de la tabla (Open, Close, Expunge, init) no esperan ni ceden.
                let _ = self.llamar(n, &mut rr);
                self.continuar(tras, &rr, r)
            }
            Ok(Entrada::Nativa(n)) => Accion::Llamar { destino: Destino::Nativa(n), regs: rr, tras },
            // Un `JMP`: directamente a su destino, sin pasar por la entrada.
            Ok(Entrada::Salto(d)) => Accion::Llamar { destino: Destino::Codigo68k(d), regs: rr, tras },
            _ => Accion::Llamar { destino: Destino::Codigo68k(pc), regs: rr, tras },
        }
    }

    /// Termina una llamada que exec pidió a la caja con `Accion::Llamar`. `hecho` son los registros al volver de
    /// ese código; `r`, los de la tarea que llamó a exec.
    pub fn continuar(&mut self, tras: Continuacion, hecho: &Registros, r: &mut Registros) -> Accion {
        let d0 = hecho.d[0];
        r.d[0] = d0;
        match tras {
            Continuacion::AbrirBiblioteca { .. } => {
                // Lo que abre el código de una biblioteca de disco es suyo, no de la tarea en la que corre (como su
                // memoria): matar esa tarea no lo debe cerrar.
                if d0 != 0 && !self.memoria_del_sistema {
                    let t = self.tarea_actual();
                    if let Some(i) = self.tareas.get_mut(&t) {
                        i.bibliotecas.push(d0);
                    }
                }
            }
            Continuacion::CerrarBiblioteca { lib } => return self.tras_cerrar(lib, d0, r),
            Continuacion::IniciarBiblioteca | Continuacion::Devolver => {}
            Continuacion::Cargada(a) => return self.tras_carga(a, d0, r),
            Continuacion::Fallida(a) => return self.apertura_fallida(&a, r),
            Continuacion::Residente { a, seglist } => return self.tras_residente(a, seglist, d0, r),
            Continuacion::AutoIniciada { tipo, tras } => {
                let lib = self.tras_autoinit(tipo, d0);
                let siguiente = self.sacar(tras);
                let mut h = *hecho;
                h.d[0] = lib;
                return self.continuar(siguiente, &h, r);
            }
            Continuacion::Echada { lib, d0: v } => return self.tras_echar(lib, d0, v, r),
            Continuacion::Valor(v) => r.d[0] = v,
            Continuacion::Entrega { id } => return self.entregar_caracter(id, r, hecho.a[3]),
            Continuacion::Manejador { n, d0: antes } => return self.siguiente_manejador(n, antes, d0, r),
            Continuacion::AbrirDispositivo { dev, io } => {
                if let Err(f) = self.tras_abrir_dispositivo(dev, io, r) {
                    self.avisar("OpenDevice", f);
                    r.d[0] = io::IOERR_OPENFAIL as i32 as u32;
                }
            }
            Continuacion::CerrarDispositivo { io, dev } => {
                if let Err(f) = self.tras_cerrar_dispositivo(io) {
                    self.avisar("CloseDevice", f);
                }
                return self.tras_cerrar(dev, d0, r);
            }
            Continuacion::HacerIO { io } => match self.tras_doio(io, r) {
                Ok(a) => return a,
                Err(f) => {
                    self.avisar("DoIO", f);
                    r.d[0] = io::IOERR_NOCMD as i32 as u32;
                }
            },
        }
        Accion::Seguir
    }

    fn es_biblioteca(&self, lib: u32) -> Result<bool, Fallo> {
        if lib == 0 {
            return Ok(false);
        }
        Ok(self.nodos(self.base + execbase::LIBLIST)?.contains(&lib))
    }

    // --- Las cuatro entradas de toda biblioteca nativa ---

    /// `Open` de una biblioteca nativa: cuenta la apertura y devuelve la base.
    pub(crate) fn lib_open(&mut self, lib: u32) -> Result<u32, Fallo> {
        self.tramo(lib, library::TAMANO)?;
        let n = self.l16(lib + library::LIB_OPENCNT)?;
        self.e16(lib + library::LIB_OPENCNT, n.saturating_add(1))?;
        let f = self.l8(lib + library::LIB_FLAGS)?;
        self.e8(lib + library::LIB_FLAGS, f & !library::LIBF_DELEXP)?;
        Ok(lib)
    }

    /// `Close` de una biblioteca nativa. Las nativas no se van de memoria: devuelve 0.
    pub(crate) fn lib_close(&mut self, lib: u32) -> Result<u32, Fallo> {
        self.tramo(lib, library::TAMANO)?;
        let n = self.l16(lib + library::LIB_OPENCNT)?;
        if n == 0 {
            return Err(Fallo::CierreDeMas { dir: lib });
        }
        self.e16(lib + library::LIB_OPENCNT, n - 1)?;
        Ok(0)
    }

    /// `Expunge` de una biblioteca nativa: se queda, con la marca de "echar cuando se pueda".
    pub(crate) fn lib_expunge(&mut self, lib: u32) -> Result<u32, Fallo> {
        self.tramo(lib, library::TAMANO)?;
        let f = self.l8(lib + library::LIB_FLAGS)?;
        self.e8(lib + library::LIB_FLAGS, f | library::LIBF_DELEXP)?;
        Ok(0)
    }

    // --- Abrir y cerrar ---

    /// `OpenLibrary`: busca por nombre en `LibList` y llama a su `Open`. D0: la base, o 0 si no está o es más
    /// vieja que `version`. (Cargarla de `LIBS:` es cosa de ramlib y dos, que aún no están.)
    pub fn open_library(&mut self, nombre: u32, version: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        let n = self.cadena(nombre)?;
        let mut lib = self.find_name(self.base + execbase::LIBLIST, &n)?;
        // "gadgets/layout.gadget", "images/label.image": la biblioteca se llama como el final del camino (lo que
        // tendría en la lista si la hubiera cargado ramlib de LIBS:Gadgets/).
        if lib == 0
            && let Some(k) = n.iter().rposition(|&c| c == b'/' || c == b':')
        {
            lib = self.find_name(self.base + execbase::LIBLIST, &n[k + 1..])?;
        }
        // No está: ramlib la busca en disco (LIBS:...) y vuelve aquí.
        if lib == 0
            && let Some(acc) = self.pedir_carga(nombre, &n, crate::ramlib::Apertura { nombre, version, disp: None }, r)
        {
            return Ok(acc);
        }
        if lib == 0 || (self.l16(lib + library::LIB_VERSION)? as u32) < version {
            r.d[0] = 0;
            return Ok(Accion::Seguir);
        }
        let mut rr = *r;
        rr.d[0] = version;
        rr.a[6] = lib;
        Ok(self.llamar_codigo(lib - 6, rr, Continuacion::AbrirBiblioteca { lib }, r))
    }

    /// `CloseLibrary`. Con 0 no hace nada. Una base que no es de la lista da un fallo y no se llama a su `Close`;
    /// tampoco cerrar de más: en una nativa, si nadie la tiene abierta (`lib_OpenCnt`); en una de disco o del programa,
    /// cuya cuenta lleva ella a su manera (locale y diskfont de 3.2 usan un contador suyo, no `lib_OpenCnt`), si la
    /// tarea que cierra (ni otra) no la abrió. Lo que cierra el código de una biblioteca de disco no se mira: lo que
    /// abrió no se apuntó a ninguna tarea.
    pub fn close_library(&mut self, lib: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        r.d[0] = 0;
        if lib == 0 {
            return Ok(Accion::Seguir);
        }
        if !self.es_biblioteca(lib)? {
            return Err(Fallo::BibliotecaDesconocida { dir: lib });
        }
        let nativa = matches!(self.entrada(lib.wrapping_sub(12)), Ok(Entrada::Nativa(_)));
        if nativa {
            if self.l16(lib + library::LIB_OPENCNT)? == 0 {
                return Err(Fallo::CierreDeMas { dir: lib });
            }
        } else if !self.memoria_del_sistema && !self.tareas.values().any(|i| i.bibliotecas.contains(&lib)) {
            return Err(Fallo::CierreDeMas { dir: lib });
        }
        if !self.memoria_del_sistema {
            self.olvidar_apertura(lib);
        }
        let mut rr = *r;
        rr.a[6] = lib;
        Ok(self.llamar_codigo(lib - 12, rr, Continuacion::CerrarBiblioteca { lib }, r))
    }

    /// Borra una apertura de `lib` de la cuenta de su tarea (de la actual si la tiene; si no, de cualquiera).
    pub(crate) fn olvidar_apertura(&mut self, lib: u32) {
        let t = self.tarea_actual();
        let quien = if self.tareas.get(&t).is_some_and(|i| i.bibliotecas.contains(&lib)) {
            Some(t)
        } else {
            self.tareas.iter().find(|(_, i)| i.bibliotecas.contains(&lib)).map(|(t, _)| *t)
        };
        if let Some(i) = quien.and_then(|q| self.tareas.get_mut(&q))
            && let Some(k) = i.bibliotecas.iter().position(|&x| x == lib)
        {
            i.bibliotecas.remove(k);
        }
    }

    /// Al matar una tarea: baja la cuenta de aperturas sin llamar a `Close` (que podría ser código 68k de la
    /// propia tarea muerta).
    pub(crate) fn cerrar_a_la_fuerza(&mut self, lib: u32) {
        if let Ok(n) = self.l16(lib.wrapping_add(library::LIB_OPENCNT))
            && n > 0
        {
            let _ = self.e16(lib + library::LIB_OPENCNT, n - 1);
        }
    }

    /// `AddLibrary`: a `LibList`, con su suma.
    pub fn add_library(&mut self, lib: u32) -> Result<(), Fallo> {
        if lib == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(lib, library::TAMANO)?;
        self.e8(lib + node::LN_TYPE, nt::LIBRARY)?;
        let f = self.l8(lib + library::LIB_FLAGS)?;
        self.e8(lib + library::LIB_FLAGS, f | library::LIBF_CHANGED)?;
        self.sum_library(lib)?;
        self.enqueue(self.base + execbase::LIBLIST, lib)
    }

    /// `RemLibrary`: llama a su `Expunge`. Las nativas se quedan (devuelven 0).
    pub fn rem_library(&mut self, lib: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        if !self.es_biblioteca(lib)? {
            return Err(Fallo::BibliotecaDesconocida { dir: lib });
        }
        let mut rr = *r;
        rr.a[6] = lib;
        Ok(self.llamar_codigo(lib - 18, rr, Continuacion::Devolver, r))
    }

    // --- Construir bibliotecas ---

    /// `MakeFunctions`: escribe la tabla de saltos de `destino` hacia atrás. Con `base_desp` 0, `tabla` son
    /// direcciones de 32 bits; si no, desplazamientos de 16 bits desde `base_desp`. Las dos acaban en -1.
    /// Devuelve el tamaño de la tabla en bytes.
    pub fn make_functions(&mut self, destino: u32, tabla: u32, base_desp: u32) -> Result<u32, Fallo> {
        let n = self.contar_vectores(tabla, base_desp != 0)?;
        for i in 0..n {
            let f = if base_desp != 0 {
                base_desp.wrapping_add(self.l16(tabla.wrapping_add(2 * i))? as i16 as i32 as u32)
            } else {
                self.l32(tabla.wrapping_add(4 * i))?
            };
            let e = destino.checked_sub(6 * (i + 1)).ok_or(Fallo::TablaMala { dir: tabla })?;
            self.escribir_salto(e, f)?;
        }
        self.codigo_cambiado.push((destino.saturating_sub(6 * n), destino));
        Ok(6 * n)
    }

    fn contar_vectores(&self, tabla: u32, cortos: bool) -> Result<u32, Fallo> {
        for i in 0..4096 {
            let fin = if cortos {
                self.l16(tabla.wrapping_add(2 * i))? == 0xFFFF
            } else {
                self.l32(tabla.wrapping_add(4 * i))? == 0xFFFF_FFFF
            };
            if fin {
                return Ok(i);
            }
        }
        Err(Fallo::TablaMala { dir: tabla })
    }

    /// `InitStruct`: borra `tamano` bytes de `mem` y la rellena con la tabla de órdenes de `exec/initializers.h`.
    /// Cada orden es un byte `ddssnnnn`: `dd` 00 copia `n+1` datos seguidos, 01 repite uno `n+1` veces, 10 lleva
    /// detrás el desplazamiento en un byte y 11 en tres; `ss` es el tamaño (00 LONG, 01 WORD, 10 BYTE). Los
    /// datos de WORD y LONG van alineados a 2, y cada orden empieza en dirección par. Un 0 acaba.
    pub fn init_struct(&mut self, tabla: u32, mem: u32, tamano: u32) -> Result<(), Fallo> {
        if tabla == 0 || mem == 0 {
            return Err(Fallo::PunteroNulo);
        }
        if tamano != 0 {
            self.cero(mem, tamano)?;
        }
        let mut s = tabla;
        let mut d = mem;
        for _ in 0..MAX_VUELTAS {
            s = s.wrapping_add(1) & !1;
            let orden = self.l8(s)?;
            s = s.wrapping_add(1);
            if orden == 0 {
                return Ok(());
            }
            let veces = (orden & 0x0F) as u32 + 1;
            let ancho = match (orden >> 4) & 3 {
                0 => 4,
                1 => 2,
                2 => 1,
                _ => return Err(Fallo::TablaMala { dir: s.wrapping_sub(1) }),
            };
            match orden >> 6 {
                2 => {
                    d = mem.wrapping_add(self.l8(s)? as u32);
                    s = s.wrapping_add(1);
                }
                3 => {
                    let desp = ((self.l8(s)? as u32) << 16) | self.l16(s.wrapping_add(1))? as u32;
                    d = mem.wrapping_add(desp);
                    s = s.wrapping_add(3);
                }
                _ => {}
            }
            if ancho > 1 {
                s = s.wrapping_add(1) & !1;
            }
            let repetir = orden >> 6 == 1;
            for i in 0..veces {
                let o = if repetir { s } else { s.wrapping_add(i * ancho) };
                match ancho {
                    1 => {
                        let v = self.l8(o)?;
                        self.e8(d, v)?
                    }
                    2 => {
                        let v = self.l16(o)?;
                        self.e16(d, v)?
                    }
                    _ => {
                        let v = self.l32(o)?;
                        self.e32(d, v)?
                    }
                }
                d = d.wrapping_add(ancho);
            }
            s = s.wrapping_add(if repetir { ancho } else { veces * ancho });
        }
        Err(Fallo::TablaMala { dir: tabla })
    }

    /// `MakeLibrary`: reserva la tabla y los datos, escribe los saltos, aplica `InitStruct` si hay estructura y
    /// llama a `init` (D0 = biblioteca, A0 = segList, A6 = SysBase) si lo hay. D0: la biblioteca, o 0.
    /// Si el primer WORD de `vectores` es -1, son desplazamientos de 16 bits desde `vectores`.
    #[allow(clippy::too_many_arguments)]
    pub fn make_library(
        &mut self,
        vectores: u32,
        estructura: u32,
        init: u32,
        tamano_datos: u32,
        seglist: u32,
        r: &mut Registros,
    ) -> Result<Accion, Fallo> {
        self.make_library_con(vectores, estructura, init, tamano_datos, seglist, 0, Continuacion::IniciarBiblioteca, r)
    }

    /// `MakeLibrary` que sigue con `tras` tras el `init`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn make_library_con(
        &mut self,
        vectores: u32,
        estructura: u32,
        init: u32,
        tamano_datos: u32,
        seglist: u32,
        rt: u32,
        tras: Continuacion,
        r: &mut Registros,
    ) -> Result<Accion, Fallo> {
        if vectores == 0 {
            return Err(Fallo::PunteroNulo);
        }
        let cortos = self.l16(vectores)? == 0xFFFF;
        let tabla = if cortos { vectores.wrapping_add(2) } else { vectores };
        let n = self.contar_vectores(tabla, cortos)?;
        let neg = (n * 6 + 3) & !3;
        let total = neg.checked_add(tamano_datos).ok_or(Fallo::TablaMala { dir: vectores })?;
        let m = self.reservar(total, memf::PUBLIC | memf::CLEAR, 0)?;
        r.d[0] = 0;
        if m == 0 {
            return Ok(Accion::Seguir);
        }
        let lib = m + neg;
        self.make_functions(lib, tabla, if cortos { vectores } else { 0 })?;
        if rt != 0 {
            // `InitResident` con `RTF_AUTOINIT`: el nodo sale del `RomTag` (tipo, prioridad, nombre, versión, id). Muchas
            // bibliotecas no traen tabla de `InitStruct` y su `Init` solo pone la revisión: sin esto no tendrían nombre
            // y ningún `OpenLibrary` las encontraría. Su tabla, si la traen, va después y manda.
            use crate::ramlib::resident;
            self.e8(lib + node::LN_TYPE, self.l8(rt + resident::RT_TYPE)?)?;
            self.e8(lib + node::LN_PRI, self.l8(rt + resident::RT_PRI)?)?;
            self.e32(lib + node::LN_NAME, self.l32(rt + resident::RT_NAME)?)?;
            self.e16(lib + library::LIB_VERSION, self.l8(rt + resident::RT_VERSION)? as u16)?;
            self.e32(lib + library::LIB_IDSTRING, self.l32(rt + resident::RT_IDSTRING)?)?;
            self.e8(lib + library::LIB_FLAGS, library::LIBF_SUMUSED | library::LIBF_CHANGED)?;
        }
        if estructura != 0 {
            self.init_struct(estructura, lib, 0)?;
        }
        self.e16(lib + library::LIB_NEGSIZE, neg as u16)?;
        self.e16(lib + library::LIB_POSSIZE, tamano_datos as u16)?;
        if init == 0 {
            r.d[0] = lib;
            return Ok(Accion::Seguir);
        }
        let mut rr = *r;
        rr.d[0] = lib;
        rr.a[0] = seglist;
        rr.a[6] = self.base;
        Ok(self.llamar_codigo(init, rr, tras, r))
    }

    /// Crea una biblioteca nativa de la caja (dos, graphics, intuition... cuando las haya) y la añade a
    /// `LibList`. `numeros` son los números nativos de sus funciones desde -30; Open, Close, Expunge y la
    /// reservada son las genéricas de exec. Los números deben ser mayores que `lvo::ULTIMO_DE_EXEC`: la caja los
    /// recibe con `Accion::Ajena` y los lleva a su biblioteca. Devuelve la base.
    pub fn crear_biblioteca_nativa(
        &mut self,
        nombre: &str,
        version: u16,
        revision: u16,
        numeros: &[u16],
        tamano_datos: u32,
    ) -> Result<u32, Fallo> {
        self.crear_nativa(nombre, version, revision, None, numeros, tamano_datos, false)
    }

    /// Como `crear_biblioteca_nativa`, y las funciones `con_cc` dejan los códigos de condición según D0 (`TST.L D0`
    /// al volver), como piden sus autodocs (`IEEESPCmp`: "cc = gt..."): su entrada salta a un trozo de 68k
    /// `LINEA_A n` / `TST.L D0` / `RTS` en la zona del sistema.
    pub fn crear_biblioteca_nativa_con_cc(
        &mut self,
        nombre: &str,
        version: u16,
        revision: u16,
        numeros: &[u16],
        tamano_datos: u32,
        con_cc: &[u16],
    ) -> Result<u32, Fallo> {
        let lib = self.crear_nativa(nombre, version, revision, None, numeros, tamano_datos, false)?;
        for &n in con_cc {
            let Some(&(l, desp)) = self.ajenas.get(&n) else { return Err(Fallo::TablaMala { dir: lib }) };
            let trozo = self.reservar(8, memf::PUBLIC, 0)?;
            if trozo == 0 {
                return Err(Fallo::SinSitio);
            }
            self.escribir_entrada_nativa(trozo, n)?;
            self.e16(trozo + 4, TST_L_D0)?;
            self.e16(trozo + 6, RTS)?;
            self.escribir_salto(l.wrapping_add_signed(desp), trozo)?;
            self.trampolines.insert(n, trozo);
        }
        self.sum_library(lib)?;
        Ok(lib)
    }

    /// Lo de `crear_biblioteca_nativa` para una biblioteca o un dispositivo (en `DeviceList`, con su `Open` propio:
    /// el número nativo `abrir`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn crear_nativa(
        &mut self,
        nombre: &str,
        version: u16,
        revision: u16,
        abrir: Option<u16>,
        numeros: &[u16],
        tamano_datos: u32,
        dispositivo: bool,
    ) -> Result<u32, Fallo> {
        if numeros.iter().chain(abrir.iter()).any(|&n| n <= lvo::ULTIMO_DE_EXEC || self.ajenas.contains_key(&n)) {
            return Err(Fallo::TablaMala { dir: 0 });
        }
        let total_n = 4 + numeros.len() as u32;
        let neg = (total_n * 6 + 3) & !3;
        let datos = tamano_datos.max(library::TAMANO);
        let largo_nombre = nombre.len() as u32 + 1;
        let m = self.reservar(neg + datos + largo_nombre, memf::PUBLIC | memf::CLEAR, 0)?;
        let tr = self.reservar(6 * numeros.len() as u32, memf::PUBLIC, 0)?;
        if m == 0 || (tr == 0 && !numeros.is_empty()) {
            return Err(Fallo::SinSitio);
        }
        let lib = m + neg;
        for i in 1..=4u16 {
            self.escribir_entrada_nativa(lib - 6 * i as u32, i)?;
        }
        if let Some(n) = abrir {
            self.escribir_entrada_nativa(lib - 6, n)?;
            self.ajenas.insert(n, (lib, -6));
        }
        for (i, &n) in numeros.iter().enumerate() {
            let k = 5 + i as u32;
            self.escribir_entrada_nativa(lib - 6 * k, n)?;
            let t = tr + 6 * i as u32;
            self.escribir_entrada_nativa(t, n)?;
            self.trampolines.insert(n, t);
            self.ajenas.insert(n, (lib, -6 * k as i32));
        }
        let pn = lib + datos;
        self.mem.leer_mut(pn, nombre.len() as u32)?.copy_from_slice(nombre.as_bytes());
        self.e8(pn + nombre.len() as u32, 0)?;
        self.e32(lib + node::LN_NAME, pn)?;
        self.e8(lib + library::LIB_FLAGS, library::LIBF_SUMUSED)?;
        self.e16(lib + library::LIB_NEGSIZE, neg as u16)?;
        self.e16(lib + library::LIB_POSSIZE, datos as u16)?;
        self.e16(lib + library::LIB_VERSION, version)?;
        self.e16(lib + library::LIB_REVISION, revision)?;
        self.e32(lib + library::LIB_IDSTRING, pn)?;
        if dispositivo {
            self.add_device(lib)?;
        } else {
            self.add_library(lib)?;
        }
        Ok(lib)
    }

    /// De qué biblioteca nativa (y entrada) es un número ajeno.
    pub fn ajena(&self, n: u16) -> Option<(u32, i32)> {
        self.ajenas.get(&n).copied()
    }

    // --- Parches ---

    /// `SetFunction`: pone `JMP nueva` en la entrada `desp` (negativo) de `lib` y devuelve la función de antes:
    /// el destino del salto que hubiera, o el trampolín si era nativa. El JIT debe olvidar esa entrada
    /// (`tomar_codigo_cambiado`).
    pub fn set_function(&mut self, lib: u32, desp: i32, nueva: u32) -> Result<u32, Fallo> {
        if lib == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(lib, library::TAMANO)?;
        let neg = self.l16(lib + library::LIB_NEGSIZE)?;
        if desp >= 0 || desp % 6 != 0 || desp.unsigned_abs() > neg as u32 {
            return Err(Fallo::LvoMalo { lvo: desp });
        }
        let e = lib.wrapping_add(desp as u32);
        let antes = match self.entrada(e)? {
            Entrada::Nativa(n) => self.trampolin(n).ok_or(Fallo::TablaMala { dir: e })?,
            Entrada::Salto(d) => d,
            // Código suelto en la entrada: no hay a dónde volver sin él. Se devuelve igual, como exec.
            Entrada::Otra => e,
        };
        self.escribir_salto(e, nueva)?;
        let f = self.l8(lib + library::LIB_FLAGS)?;
        self.e8(lib + library::LIB_FLAGS, f | library::LIBF_CHANGED)?;
        self.sum_library(lib)?;
        self.codigo_cambiado.push((e, e + 6));
        Ok(antes)
    }

    /// `SumLibrary`: la suma de los LONG de la tabla de saltos en `lib_Sum`. Si la biblioteca usa la suma y no ha
    /// cambiado, y no coincide, avisa (exec daría un `Alert`).
    pub fn sum_library(&mut self, lib: u32) -> Result<(), Fallo> {
        self.tramo(lib, library::TAMANO)?;
        let neg = self.l16(lib + library::LIB_NEGSIZE)? as u32;
        let inicio = lib.checked_sub(neg).ok_or(Fallo::TablaMala { dir: lib })?;
        let mut suma: u32 = 0;
        for i in 0..neg / 4 {
            suma = suma.wrapping_add(self.l32(inicio + 4 * i)?);
        }
        let f = self.l8(lib + library::LIB_FLAGS)?;
        if f & library::LIBF_SUMUSED != 0 && f & library::LIBF_CHANGED == 0 && self.l32(lib + library::LIB_SUM)? != suma
        {
            self.avisar("SumLibrary", Fallo::TablaMala { dir: lib });
        }
        self.e32(lib + library::LIB_SUM, suma)?;
        self.e8(lib + library::LIB_FLAGS, f & !library::LIBF_CHANGED)
    }
}
