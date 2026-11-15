//! timer.device y los dispositivos de exec (OpenDevice, DoIO, SendIO, WaitIO...), con casos hostiles.

use caja_exec::estructuras::{memf, msgport, node, nt, task};
use caja_exec::{Accion, Config, Destino, Exec, Memoria, MemoriaVec, Registros, io, lvo};
use caja_timer::*;

const TAMANO: u32 = 4 << 20;
const PRIMERO: u16 = 0x880;

struct Caja {
    e: Exec<MemoriaVec>,
    t: Temporizador,
    reloj: Reloj,
    tarea: u32,
}

fn caja() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).unwrap();
    let tarea = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(tarea));
    let t = Temporizador::instalar(&mut e, PRIMERO).unwrap();
    Caja { e, t, reloj: Reloj { monotono_us: 1_000_000, sistema_us: 1_536_000_000_000_000 }, tarea }
}

impl Caja {
    /// Una función de exec por su nombre, siguiendo lo que pida (el Open o el BeginIO del dispositivo), como la caja.
    fn exec(&mut self, f: &str, r: &mut Registros) -> Accion {
        let n = lvo::por_nombre(f).unwrap();
        let mut a = self.e.llamar(n, r);
        for _ in 0..8 {
            match a {
                Accion::Llamar { destino: Destino::Nativa(m), regs, tras } => {
                    let mut rr = regs;
                    if self.t.es_mio(m) {
                        self.t.llamar(&mut self.e, m, &mut rr, self.reloj);
                    } else {
                        self.e.llamar(m, &mut rr);
                    }
                    a = self.e.continuar(tras, &rr, r);
                }
                Accion::Llamar { .. } => panic!("68k"),
                _ => return a,
            }
        }
        a
    }
    fn io(&mut self, f: &str, ior: u32) -> (Accion, u32) {
        let mut r = Registros::default();
        r.a[1] = ior;
        r.a[0] = ior;
        let a = self.exec(f, &mut r);
        (a, r.d[0])
    }
    /// Una función de la biblioteca del dispositivo (AddTime...), por su nombre.
    fn lib(&mut self, f: &str, a0: u32, a1: u32) -> u32 {
        let n = PRIMERO + 1 + FUNCIONES.iter().position(|x| *x == f).unwrap() as u16;
        let mut r = Registros::default();
        r.a[0] = a0;
        r.a[1] = a1;
        r.a[6] = self.t.base();
        assert_eq!(self.t.llamar(&mut self.e, n, &mut r, self.reloj), Accion::Seguir);
        r.d[0]
    }
    fn pasar(&mut self, us: u64) -> Accion {
        self.reloj.monotono_us += us;
        self.reloj.sistema_us += us;
        self.t.vencidos(&mut self.e, self.reloj).unwrap()
    }
    fn cadena(&mut self, s: &str) -> u32 {
        let d = self.e.alloc_mem(s.len() as u32 + 1, memf::CLEAR).unwrap();
        self.e.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s.as_bytes());
        d
    }
    /// Puerto, petición y OpenDevice de la unidad.
    fn abrir(&mut self, unidad: u32) -> (u32, u32, u32) {
        let p = self.e.create_msg_port().unwrap();
        let ior = self.e.create_io_request(p, TR_TAMANO).unwrap();
        assert_ne!(ior, 0);
        let nombre = self.cadena("timer.device");
        let mut r = Registros::default();
        r.a[0] = nombre;
        r.d[0] = unidad;
        r.a[1] = ior;
        assert_eq!(self.exec("OpenDevice", &mut r), Accion::Seguir);
        (p, ior, r.d[0])
    }
    fn pedir(&mut self, ior: u32, orden: u16, s: u32, us: u32) {
        self.e.mem.escribir16(ior + io::IO_COMMAND, orden).unwrap();
        self.e.mem.escribir32(ior + TR_SECS, s).unwrap();
        self.e.mem.escribir32(ior + TR_MICRO, us).unwrap();
    }
}

#[test]
fn abrir_y_cerrar() {
    let mut c = caja();
    let (_, ior, e) = c.abrir(UNIT_MICROHZ);
    assert_eq!(e, 0);
    assert_eq!(c.e.mem.leer32(ior + io::IO_DEVICE).unwrap(), c.t.base());
    assert_eq!(c.e.mem.leer32(ior + io::IO_UNIT).unwrap(), UNIT_MICROHZ);
    assert_eq!(c.e.mem.leer16(c.t.base() + 32).unwrap(), 1, "lib_OpenCnt");
    // En DeviceList, como dispositivo.
    let b = c.e.base();
    assert!(c.e.nodos(b + caja_exec::estructuras::execbase::DEVICELIST).unwrap().contains(&c.t.base()));
    assert_eq!(c.e.mem.leer8(c.t.base() + node::LN_TYPE).unwrap(), nt::DEVICE);
    assert_eq!(c.io("CloseDevice", ior).1, 0);
    assert_eq!(c.e.mem.leer32(ior + io::IO_DEVICE).unwrap(), u32::MAX);
    assert_eq!(c.e.mem.leer16(c.t.base() + 32).unwrap(), 0);
    assert!(c.e.avisos().is_empty() && c.t.avisos().is_empty(), "{:?} {:?}", c.e.avisos(), c.t.avisos());
    // Una unidad que no existe y un dispositivo que no está.
    let (_, ior2, e) = c.abrir(9);
    assert_eq!(e as i32, -1);
    assert_eq!(c.e.mem.leer32(ior2 + io::IO_DEVICE).unwrap(), 0);
    let otro = c.cadena("serial.device");
    let mut r = Registros::default();
    r.a[0] = otro;
    r.a[1] = ior2;
    c.exec("OpenDevice", &mut r);
    assert_eq!(r.d[0] as i32, -1);
    assert_eq!(c.e.mem.leer8(ior2 + io::IO_ERROR).unwrap() as i8, io::IOERR_OPENFAIL);
}

#[test]
fn sendio_espera_y_vuelve_al_puerto() {
    let mut c = caja();
    let (p, ior, _) = c.abrir(UNIT_MICROHZ);
    c.pedir(ior, TR_ADDREQUEST, 1, 500_000);
    c.io("SendIO", ior);
    assert_eq!(c.t.pendientes(), 1);
    assert_eq!(c.io("CheckIO", ior).1, 0, "aún no");
    c.pasar(1_000_000);
    assert_eq!(c.io("CheckIO", ior).1, 0);
    c.pasar(500_000);
    assert_eq!(c.t.pendientes(), 0);
    assert_eq!(c.io("CheckIO", ior).1, ior);
    // En el puerto, y la tarea tiene su señal.
    let bit = c.e.mem.leer8(p + msgport::MP_SIGBIT).unwrap();
    assert_ne!(c.e.mem.leer32(c.tarea + task::TC_SIGRECVD).unwrap() & (1 << bit), 0);
    assert_eq!(c.io("WaitIO", ior), (Accion::Seguir, 0));
    assert!(c.e.lista_vacia(p + msgport::MP_MSGLIST).unwrap(), "WaitIO la saca del puerto");
    assert!(c.e.avisos().is_empty() && c.t.avisos().is_empty(), "{:?} {:?}", c.e.avisos(), c.t.avisos());
}

#[test]
fn waitio_bloquea_y_doio_no_repite() {
    let mut c = caja();
    let (_, ior, _) = c.abrir(UNIT_VBLANK);
    c.pedir(ior, TR_ADDREQUEST, 0, 20_000);
    c.io("SendIO", ior);
    assert_eq!(c.io("WaitIO", ior).0, Accion::Bloquear);
    c.pasar(20_000);
    assert_eq!(c.e.elegir_siguiente(), Some(c.tarea), "despierta");
    assert_eq!(c.io("WaitIO", ior), (Accion::Seguir, 0));
    // DoIO: se bloquea; al despertar, la caja repite DoIO y no se manda otra vez.
    c.pedir(ior, TR_ADDREQUEST, 2, 0);
    assert_eq!(c.io("DoIO", ior).0, Accion::Bloquear);
    assert_eq!(c.t.pendientes(), 1);
    c.pasar(2_000_000);
    assert_eq!(c.e.elegir_siguiente(), Some(c.tarea));
    assert_eq!(c.io("DoIO", ior), (Accion::Seguir, 0));
    assert_eq!(c.t.pendientes(), 0, "no se mandó dos veces");
    // Un DoIO que acaba en el acto (la hora): IOF_QUICK y sin pasar por el puerto.
    c.pedir(ior, TR_GETSYSTIME, 0, 0);
    assert_eq!(c.io("DoIO", ior), (Accion::Seguir, 0));
    let s = c.e.mem.leer32(ior + TR_SECS).unwrap();
    assert_eq!(s as u64, c.reloj.sistema_us / 1_000_000);
    // Esperar 0: vuelve enseguida.
    c.pedir(ior, TR_ADDREQUEST, 0, 0);
    assert_eq!(c.io("DoIO", ior), (Accion::Seguir, 0));
    assert!(c.e.avisos().is_empty() && c.t.avisos().is_empty(), "{:?} {:?}", c.e.avisos(), c.t.avisos());
}

#[test]
fn abortio_y_orden_de_vencimiento() {
    let mut c = caja();
    let (p, a, _) = c.abrir(UNIT_MICROHZ);
    let b = c.e.create_io_request(p, TR_TAMANO).unwrap();
    c.e.copy_mem(a, b, TR_TAMANO).unwrap();
    c.pedir(a, TR_ADDREQUEST, 5, 0);
    c.pedir(b, TR_ADDREQUEST, 1, 0);
    c.io("SendIO", a);
    c.io("SendIO", b);
    c.pasar(1_000_000);
    assert_eq!(c.t.peticiones(), vec![a]);
    assert_eq!(c.e.get_msg(p).unwrap(), b);
    assert_eq!(c.io("AbortIO", a).1, 0);
    assert_eq!(c.io("WaitIO", a).1 as i32, io::IOERR_ABORTED as i32);
    assert_eq!(c.io("AbortIO", a).1, u32::MAX, "ya no espera");
    // Una orden que no existe: IOERR_NOCMD.
    c.pedir(a, 77, 0, 0);
    assert_eq!(c.io("DoIO", a).1 as i32, io::IOERR_NOCMD as i32);
}

#[test]
fn unidades_de_espera_hasta() {
    let mut c = caja();
    let (_, ior, _) = c.abrir(UNIT_WAITUNTIL);
    let ahora_s = (c.reloj.sistema_us / 1_000_000) as u32;
    c.pedir(ior, TR_ADDREQUEST, ahora_s + 3, 0);
    c.io("SendIO", ior);
    c.pasar(2_000_000);
    assert_eq!(c.t.pendientes(), 1);
    c.pasar(1_000_000);
    assert_eq!(c.t.pendientes(), 0);
    // Una hora pasada: enseguida.
    c.io("WaitIO", ior);
    c.pedir(ior, TR_ADDREQUEST, 10, 0);
    c.io("SendIO", ior);
    assert_eq!(c.t.pendientes(), 0);
    // EClock: medio segundo de ciclos.
    let (_, e2, _) = c.abrir(UNIT_ECLOCK);
    c.pedir(e2, TR_ADDREQUEST, 0, (ECLOCK / 2) as u32);
    c.io("SendIO", e2);
    c.pasar(499_000);
    assert_eq!(c.t.pendientes(), 1);
    c.pasar(2_000);
    assert_eq!(c.t.pendientes(), 0);
}

#[test]
fn funciones_de_la_biblioteca() {
    let mut c = caja();
    let x = c.e.alloc_mem(8, memf::CLEAR).unwrap();
    let y = c.e.alloc_mem(8, memf::CLEAR).unwrap();
    for (d, s, us) in [(x, 3u32, 900_000u32), (y, 1, 200_000)] {
        c.e.mem.escribir32(d, s).unwrap();
        c.e.mem.escribir32(d + 4, us).unwrap();
    }
    assert_eq!(c.lib("CmpTime", x, y), u32::MAX, "x es más");
    assert_eq!(c.lib("CmpTime", y, x), 1);
    c.lib("AddTime", x, y);
    assert_eq!((c.e.mem.leer32(x).unwrap(), c.e.mem.leer32(x + 4).unwrap()), (5, 100_000));
    c.lib("SubTime", x, y);
    c.lib("SubTime", x, y);
    assert_eq!((c.e.mem.leer32(x).unwrap(), c.e.mem.leer32(x + 4).unwrap()), (2, 700_000));
    assert_eq!(c.lib("CmpTime", x, x), 0);
    let f = c.lib("ReadEClock", x, 0);
    assert_eq!(f as u64, ECLOCK);
    let ciclos = (c.e.mem.leer32(x).unwrap() as u64) << 32 | c.e.mem.leer32(x + 4).unwrap() as u64;
    assert_eq!(ciclos, a_eclock(c.reloj.monotono_us));
    c.lib("GetSysTime", y, 0);
    assert_eq!(c.e.mem.leer32(y).unwrap() as u64, c.reloj.sistema_us / 1_000_000);
    // TR_SETSYSTIME cambia lo que da GetSysTime.
    let (_, ior, _) = c.abrir(UNIT_MICROHZ);
    c.pedir(ior, TR_SETSYSTIME, 1000, 0);
    assert_eq!(c.io("DoIO", ior).1, 0);
    c.lib("GetSysTime", y, 0);
    assert_eq!(c.e.mem.leer32(y).unwrap(), 1000);
    assert!(c.t.avisos().is_empty());
}

#[test]
fn hostil() {
    let mut c = caja();
    let (p, ior, _) = c.abrir(UNIT_MICROHZ);
    let malos = [0u32, 4, 0x800, TAMANO - 8, TAMANO + 64, 0xFFFF_FFF0, u32::MAX, p];
    for &m in &malos {
        for f in ["SendIO", "DoIO", "CheckIO", "WaitIO", "AbortIO", "CloseDevice", "DeleteIORequest"] {
            let (a, _) = c.io(f, m);
            assert!(matches!(a, Accion::Seguir | Accion::Bloquear), "{} {:#x}: {:?}", f, m, a);
            let _ = c.e.elegir_siguiente();
        }
        for (k, _) in FUNCIONES.iter().enumerate() {
            let mut r = Registros { d: [m; 8], a: [m; 8] };
            assert_eq!(c.t.llamar(&mut c.e, PRIMERO + k as u16, &mut r, c.reloj), Accion::Seguir);
        }
        let mut r = Registros { d: [m; 8], a: [m; 8] };
        c.exec("OpenDevice", &mut r);
        c.exec("CreateIORequest", &mut r);
    }
    let _ = c.e.elegir_siguiente();
    assert!(c.e.avisos().len() + c.t.avisos().len() > 20);
    // Mandarla dos veces mientras espera: la segunda se rechaza y la cola sigue bien.
    c.pedir(ior, TR_ADDREQUEST, 1, 0);
    c.io("SendIO", ior);
    c.t.tomar_avisos();
    c.io("SendIO", ior);
    assert_eq!(c.t.avisos().len(), 1);
    assert_eq!(c.t.pendientes(), 1);
    // Soltar la petición mientras espera (sin AbortIO): al vencer, aviso y nada más.
    c.e.mem.escribir16(ior + caja_exec::estructuras::message::MN_LENGTH, TR_TAMANO as u16).unwrap();
    c.e.free_mem(ior, TR_TAMANO).unwrap();
    c.t.tomar_avisos();
    c.pasar(2_000_000);
    assert_eq!(c.t.pendientes(), 0);
    assert_eq!(c.t.avisos().len(), 1);
    // Tiempos absurdos: no desbordan.
    let (_, i2, _) = c.abrir(UNIT_MICROHZ);
    c.pedir(i2, TR_ADDREQUEST, u32::MAX, u32::MAX);
    c.io("SendIO", i2);
    let (_, i3, _) = c.abrir(UNIT_WAITECLOCK);
    c.pedir(i3, TR_ADDREQUEST, u32::MAX, u32::MAX);
    c.io("SendIO", i3);
    c.pasar(1);
    assert_eq!(c.t.pendientes(), 2);
    // La tarea muere: se olvidan.
    c.t.tarea_muerta(c.tarea);
    c.e.matar_tarea(c.tarea).unwrap();
    assert_eq!(c.t.pendientes(), 0);
    // DeleteIORequest dos veces y con mn_Length cambiado: aviso, sin soltar de más.
    let t2 = c.e.crear_tarea("otra", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(c.e.elegir_siguiente(), Some(t2));
    let p2 = c.e.create_msg_port().unwrap();
    let r1 = c.e.create_io_request(p2, 40).unwrap();
    let libre = c.e.avail_mem(0).unwrap();
    c.e.mem.escribir16(r1 + caja_exec::estructuras::message::MN_LENGTH, 4000).unwrap();
    assert!(c.e.delete_io_request(r1).is_err());
    assert_eq!(c.e.avail_mem(0).unwrap(), libre);
    c.e.mem.escribir16(r1 + caja_exec::estructuras::message::MN_LENGTH, 40).unwrap();
    c.e.delete_io_request(r1).unwrap();
    assert!(c.e.delete_io_request(r1).is_err());
    assert_eq!(c.e.create_io_request(0, 40).unwrap(), 0);
    assert_eq!(c.e.create_io_request(p2, 4).unwrap(), 0);
    assert_eq!(c.e.create_io_request(p2, u32::MAX).unwrap(), 0);
}
