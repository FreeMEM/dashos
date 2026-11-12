//! Dispositivos: `OpenDevice`, `CloseDevice`, `DoIO`, `SendIO`, `CheckIO`, `WaitIO`, `AbortIO`, `AddDevice`,
//! `CreateIORequest` y `DeleteIORequest`.
//!
//! De los autodocs de exec y `exec/io.h`, `exec/devices.h` del NDK 3.1. Un dispositivo es una biblioteca en
//! `DeviceList` con dos entradas más: `BeginIO` (-30) y `AbortIO` (-36). Una petición (`IORequest`) es un mensaje: el
//! dispositivo la atiende en el acto (con `IOF_QUICK`, sin contestar) o más tarde, y entonces la devuelve con
//! `ReplyMsg` a su `mn_ReplyPort`. `WaitIO` espera esa vuelta en el puerto.
//!
//! Los dispositivos de la caja son nativos (timer.device, `caja/timer`): exec llama a su `Open`, `BeginIO` y `AbortIO`
//! con `Accion::Llamar`, como al `Open` de una biblioteca nativa. Si `DoIO` tiene que esperar, la tarea se bloquea y
//! la caja repite la llamada al despertar: exec apunta qué peticiones de `DoIO` ya salieron, para no mandarlas otra
//! vez.

use crate::estructuras::{library, message, msgport, node, nt, task};
use crate::tareas::Espera;
use crate::{Accion, Continuacion, Exec, Fallo, Memoria, Registros, execbase};

/// `struct IORequest` y `struct IOStdReq` (`exec/io.h`).
pub mod io {
    pub const IO_MESSAGE: u32 = 0;
    pub const IO_DEVICE: u32 = 20;
    pub const IO_UNIT: u32 = 24;
    pub const IO_COMMAND: u32 = 28;
    pub const IO_FLAGS: u32 = 30;
    pub const IO_ERROR: u32 = 31;
    pub const TAMANO: u32 = 32;
    pub const IO_ACTUAL: u32 = 32;
    pub const IO_LENGTH: u32 = 36;
    pub const IO_DATA: u32 = 40;
    pub const IO_OFFSET: u32 = 44;
    pub const STD_TAMANO: u32 = 48;

    pub const IOF_QUICK: u8 = 1;

    /// Errores (`exec/errors.h`).
    pub const IOERR_OPENFAIL: i8 = -1;
    pub const IOERR_ABORTED: i8 = -2;
    pub const IOERR_NOCMD: i8 = -3;
    pub const IOERR_BADLENGTH: i8 = -4;
    pub const IOERR_BADADDRESS: i8 = -5;
    pub const IOERR_UNITBUSY: i8 = -6;

    /// Órdenes de todos los dispositivos (`exec/io.h`).
    pub const CMD_INVALID: u16 = 0;
    pub const CMD_RESET: u16 = 1;
    pub const CMD_READ: u16 = 2;
    pub const CMD_WRITE: u16 = 3;
    pub const CMD_UPDATE: u16 = 4;
    pub const CMD_CLEAR: u16 = 5;
    pub const CMD_STOP: u16 = 6;
    pub const CMD_START: u16 = 7;
    pub const CMD_FLUSH: u16 = 8;
    pub const CMD_NONSTD: u16 = 9;
}

/// Los LVO de las dos entradas propias de los dispositivos.
pub const LVO_BEGINIO: u32 = 30;
pub const LVO_ABORTIO: u32 = 36;

impl<M: Memoria> Exec<M> {
    /// Crea un dispositivo nativo en `DeviceList`: `abrir` es el número nativo de su `Open` (comprueba la unidad y
    /// pone `io_Unit` y `io_Error`) y `numeros`, los de `BeginIO`, `AbortIO` y sus funciones, por orden desde -30.
    pub fn crear_dispositivo_nativo(
        &mut self,
        nombre: &str,
        version: u16,
        revision: u16,
        abrir: u16,
        numeros: &[u16],
        tamano_datos: u32,
    ) -> Result<u32, Fallo> {
        self.crear_nativa(nombre, version, revision, Some(abrir), numeros, tamano_datos, true)
    }

    /// `AddDevice`: a `DeviceList`, con su suma.
    pub fn add_device(&mut self, d: u32) -> Result<(), Fallo> {
        if d == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(d, library::TAMANO)?;
        // Un nodo que ya está en una lista (otra del sistema, o esta) la rompería al enlazarlo otra vez.
        if self.mensaje_en_cola(d) || self.dispositivos.contains(&d) {
            return Err(Fallo::Sospechoso { dir: d });
        }
        self.e8(d + node::LN_TYPE, nt::DEVICE)?;
        let f = self.l8(d + library::LIB_FLAGS)?;
        self.e8(d + library::LIB_FLAGS, f | library::LIBF_CHANGED)?;
        self.sum_library(d)?;
        self.enqueue(self.base + execbase::DEVICELIST, d)?;
        self.dispositivos.insert(d);
        Ok(())
    }

    fn es_dispositivo(&self, d: u32) -> Result<bool, Fallo> {
        Ok(d != 0 && self.dispositivos.contains(&d) && self.memoria_viva(d, library::TAMANO))
    }

    /// El dispositivo de una petición abierta, comprobado.
    fn dispositivo_de(&self, ior: u32) -> Result<u32, Fallo> {
        if ior == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(ior, io::TAMANO)?;
        let d = self.lp(ior + io::IO_DEVICE)?;
        if !self.es_dispositivo(d)? {
            return Err(Fallo::BibliotecaDesconocida { dir: d });
        }
        Ok(d)
    }

    /// `OpenDevice(devName, unit, ioRequest, flags)`: 0 si se abrió; si no, el error (`IOERR_OPENFAIL`...), también
    /// en `io_Error`. Cargarlo de `DEVS:` es cosa de ramlib, que no está: solo los de `DeviceList`.
    pub fn open_device(
        &mut self,
        nombre: u32,
        unidad: u32,
        ior: u32,
        flags: u32,
        r: &mut Registros,
    ) -> Result<Accion, Fallo> {
        if ior == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(ior, io::TAMANO)?;
        let n = self.cadena(nombre)?;
        let d = self.find_name(self.base + execbase::DEVICELIST, &n)?;
        self.e8(ior + io::IO_ERROR, 0)?;
        if d == 0 {
            self.e32(ior + io::IO_DEVICE, 0)?;
            self.e8(ior + io::IO_ERROR, io::IOERR_OPENFAIL as u8)?;
            r.d[0] = io::IOERR_OPENFAIL as i32 as u32;
            return Ok(Accion::Seguir);
        }
        self.e32(ior + io::IO_DEVICE, d)?;
        let mut rr = *r;
        rr.d[0] = unidad;
        rr.d[1] = flags;
        rr.a[1] = ior;
        rr.a[6] = d;
        Ok(self.llamar_codigo(d - 6, rr, Continuacion::AbrirDispositivo { dev: d, io: ior }, r))
    }

    /// Lo de después del `Open` del dispositivo.
    pub(crate) fn tras_abrir_dispositivo(&mut self, d: u32, ior: u32, r: &mut Registros) -> Result<(), Fallo> {
        let e = self.l8(ior + io::IO_ERROR)? as i8;
        r.d[0] = e as i32 as u32;
        if e != 0 {
            self.e32(ior + io::IO_DEVICE, 0)?;
            return Ok(());
        }
        let n = self.l16(d + library::LIB_OPENCNT)?;
        self.e16(d + library::LIB_OPENCNT, n.saturating_add(1))?;
        let t = self.tarea_actual();
        if let Some(i) = self.tareas.get_mut(&t) {
            i.bibliotecas.push(d);
        }
        Ok(())
    }

    /// `CloseDevice(ioRequest)`: llama a su `Close`; después `io_Device` e `io_Unit` valen -1.
    pub fn close_device(&mut self, ior: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        let d = self.dispositivo_de(ior)?;
        if self.l16(d + library::LIB_OPENCNT)? == 0 {
            return Err(Fallo::CierreDeMas { dir: d });
        }
        self.olvidar_apertura(d);
        let mut rr = *r;
        rr.a[1] = ior;
        rr.a[6] = d;
        Ok(self.llamar_codigo(d - 12, rr, Continuacion::CerrarDispositivo { io: ior }, r))
    }

    pub(crate) fn tras_cerrar_dispositivo(&mut self, ior: u32) -> Result<(), Fallo> {
        self.e32(ior + io::IO_DEVICE, u32::MAX)?;
        self.e32(ior + io::IO_UNIT, u32::MAX)
    }

    /// Llama a `BeginIO` del dispositivo de la petición.
    fn begin_io(&mut self, ior: u32, flags: u8, tras: Continuacion, r: &mut Registros) -> Result<Accion, Fallo> {
        let d = self.dispositivo_de(ior)?;
        self.e8(ior + io::IO_FLAGS, flags)?;
        // Una petición que va al dispositivo es un mensaje que aún no ha vuelto.
        self.e8(ior + node::LN_TYPE, nt::MESSAGE)?;
        let mut rr = *r;
        rr.a[1] = ior;
        rr.a[6] = d;
        Ok(self.llamar_codigo(d - LVO_BEGINIO, rr, tras, r))
    }

    /// `SendIO(ioRequest)`: la manda sin esperar.
    pub fn send_io(&mut self, ior: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        self.begin_io(ior, 0, Continuacion::Devolver, r)
    }

    /// `DoIO(ioRequest)`: la manda con `IOF_QUICK` y, si el dispositivo no la acabó en el acto, espera como
    /// `WaitIO`. Devuelve `io_Error`.
    pub fn do_io(&mut self, ior: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        if ior != 0 && self.doio_en_espera.contains(&ior) {
            // La repetición tras despertar: ya salió; solo queda esperar.
            return self.esperar_doio(ior, r);
        }
        self.begin_io(ior, io::IOF_QUICK, Continuacion::HacerIO { io: ior }, r)
    }

    /// Lo de después del `BeginIO` de `DoIO`.
    pub(crate) fn tras_doio(&mut self, ior: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        self.esperar_doio(ior, r)
    }

    fn esperar_doio(&mut self, ior: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        match self.wait_io(ior)? {
            Espera::Hecho(e) => {
                self.doio_en_espera.retain(|&x| x != ior);
                r.d[0] = e;
                Ok(Accion::Seguir)
            }
            Espera::Bloquea => {
                if !self.doio_en_espera.contains(&ior) {
                    self.doio_en_espera.push(ior);
                }
                Ok(Accion::Bloquear)
            }
        }
    }

    /// `CheckIO(ioRequest)`: la petición si ya acabó (rápida o devuelta), o 0.
    pub fn check_io(&self, ior: u32) -> Result<u32, Fallo> {
        if ior == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(ior, io::TAMANO)?;
        let rapida = self.l8(ior + io::IO_FLAGS)? & io::IOF_QUICK != 0;
        Ok(if rapida || self.l8(ior + node::LN_TYPE)? == nt::REPLYMSG { ior } else { 0 })
    }

    /// `WaitIO(ioRequest)`: espera a que vuelva; la saca del puerto y devuelve `io_Error`.
    pub fn wait_io(&mut self, ior: u32) -> Result<Espera, Fallo> {
        if ior == 0 {
            return Err(Fallo::PunteroNulo);
        }
        self.tramo(ior, io::TAMANO)?;
        let t = self.actual_o_fallo()?;
        let vuelve = self.vuelve_de_espera(t);
        let error = |e: &Self| -> Result<u32, Fallo> { Ok(e.l8(ior + io::IO_ERROR)? as i8 as i32 as u32) };
        if self.l8(ior + io::IO_FLAGS)? & io::IOF_QUICK != 0 {
            return Ok(Espera::Hecho(error(self)?));
        }
        let p = self.lp(ior + message::MN_REPLYPORT)?;
        let bit = if p != 0 { 1u32 << (self.l8(p + msgport::MP_SIGBIT)? & 31) } else { 0 };
        if vuelve {
            let rec = self.l32(t + task::TC_SIGRECVD)?;
            self.e32(t + task::TC_SIGRECVD, rec & !bit)?;
        }
        if self.l8(ior + node::LN_TYPE)? == nt::REPLYMSG {
            if self.mensaje_en_cola(ior) {
                self.remove(ior)?;
            }
            return Ok(Espera::Hecho(error(self)?));
        }
        if p == 0 {
            // Sin puerto de vuelta no puede volver nunca.
            return Err(Fallo::Sospechoso { dir: ior });
        }
        self.dormir(t, bit)?;
        Ok(Espera::Bloquea)
    }

    /// `AbortIO(ioRequest)`: se lo pide al dispositivo.
    pub fn abort_io(&mut self, ior: u32, r: &mut Registros) -> Result<Accion, Fallo> {
        let d = self.dispositivo_de(ior)?;
        let mut rr = *r;
        rr.a[1] = ior;
        rr.a[6] = d;
        Ok(self.llamar_codigo(d - LVO_ABORTIO, rr, Continuacion::Devolver, r))
    }

    /// `CreateIORequest(ioReplyPort, size)`: una petición a cero con su puerto de vuelta y su tamaño; 0 sin puerto,
    /// con un tamaño menor que un `Message` o sin memoria.
    pub fn create_io_request(&mut self, p: u32, tamano: u32) -> Result<u32, Fallo> {
        if p == 0 || !(message::TAMANO..=0x1_0000).contains(&tamano) {
            return Ok(0);
        }
        let d = self.alloc_mem(tamano, crate::estructuras::memf::PUBLIC | crate::estructuras::memf::CLEAR)?;
        if d == 0 {
            return Ok(0);
        }
        self.e8(d + node::LN_TYPE, nt::REPLYMSG)?;
        self.e32(d + message::MN_REPLYPORT, p)?;
        self.e16(d + message::MN_LENGTH, tamano as u16)?;
        Ok(d)
    }

    /// `DeleteIORequest(ioReq)`: suelta lo de `CreateIORequest` (su tamaño, de `mn_Length`). Con 0 no hace nada.
    pub fn delete_io_request(&mut self, ior: u32) -> Result<(), Fallo> {
        if ior == 0 {
            return Ok(());
        }
        self.tramo(ior, message::TAMANO)?;
        if self.mensaje_en_cola(ior) {
            return Err(Fallo::MensajeEnCola { dir: ior });
        }
        let largo = self.l16(ior + message::MN_LENGTH)? as u32;
        self.free_mem(ior, largo)?;
        self.doio_en_espera.retain(|&x| x != ior);
        Ok(())
    }
}
