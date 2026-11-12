//! Fija la disposición de las estructuras (NDK 3.1), los LVO de exec, los registros del JIT y el arranque.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::registros::*;
use caja_exec::{Config, Exec, Fallo, Memoria, MemoriaVec, lvo};
use comun::*;

#[test]
fn tamanos_de_las_estructuras() {
    assert_eq!(node::TAMANO, 14);
    assert_eq!(minnode::TAMANO, 8);
    assert_eq!(list::TAMANO, 14);
    assert_eq!(minlist::TAMANO, 12);
    assert_eq!(task::TAMANO, 92);
    assert_eq!(msgport::TAMANO, 34);
    assert_eq!(message::TAMANO, 20);
    assert_eq!(library::TAMANO, 34);
    assert_eq!(memchunk::TAMANO, 8);
    assert_eq!(memheader::TAMANO, 32);
    assert_eq!(memlist::TAMANO, 24);
    assert_eq!(signalsemaphore::TAMANO, 46);
    assert_eq!(interrupt::TAMANO, 22);
    assert_eq!(interrupt::IV_TAMANO, 12);
    assert_eq!(interrupt::SH_TAMANO, 16);
    assert_eq!(execbase::TAMANO, 632);
}

#[test]
fn campos_seguidos_sin_huecos() {
    // Cada estructura empieza con su nodo y los campos van uno tras otro.
    assert_eq!(task::TC_FLAGS, node::TAMANO);
    assert_eq!(task::TC_MEMENTRY + list::TAMANO, task::TC_USERDATA);
    assert_eq!(task::TC_USERDATA + 4, task::TAMANO);
    assert_eq!(msgport::MP_MSGLIST + list::TAMANO, msgport::TAMANO);
    assert_eq!(message::MN_REPLYPORT, node::TAMANO);
    assert_eq!(library::LIB_FLAGS, node::TAMANO);
    assert_eq!(library::LIB_OPENCNT + 2, library::TAMANO);
    assert_eq!(memheader::MH_ATTRIBUTES, node::TAMANO);
    assert_eq!(memlist::ML_ME + memlist::ME_TAMANO, memlist::TAMANO);
    assert_eq!(signalsemaphore::SS_WAITQUEUE + minlist::TAMANO, signalsemaphore::SS_MULTIPLELINK);
    // ExecBase: las listas, de 14 en 14.
    assert_eq!(execbase::SOFTVER, library::TAMANO);
    assert_eq!(execbase::INTVECTS + 16 * interrupt::IV_TAMANO, execbase::THISTASK);
    let listas = [
        execbase::MEMLIST,
        execbase::RESOURCELIST,
        execbase::DEVICELIST,
        execbase::INTRLIST,
        execbase::LIBLIST,
        execbase::PORTLIST,
        execbase::TASKREADY,
        execbase::TASKWAIT,
        execbase::SOFTINTS,
    ];
    for w in listas.windows(2) {
        assert_eq!(w[0] + list::TAMANO, w[1]);
    }
    assert_eq!(execbase::SOFTINTS + 5 * interrupt::SH_TAMANO, execbase::LASTALERT);
    assert_eq!(execbase::LASTALERT + 16, execbase::VBLANKFREQUENCY);
    assert_eq!(execbase::SEMAPHORELIST + list::TAMANO, execbase::KICKMEMPTR);
    assert_eq!(execbase::EX_MEMHANDLERS + minlist::TAMANO, execbase::EX_MEMHANDLER);
    assert_eq!(execbase::EX_MEMHANDLER + 4, execbase::TAMANO);
}

#[test]
fn desplazamientos_conocidos_de_execbase() {
    // Los que más leen los programas, en hexadecimal como en las tablas de referencia.
    assert_eq!(execbase::THISTASK, 0x114);
    assert_eq!(execbase::IDNESTCNT, 0x126);
    assert_eq!(execbase::TDNESTCNT, 0x127);
    assert_eq!(execbase::ATTNFLAGS, 0x128);
    assert_eq!(execbase::MEMLIST, 0x142);
    assert_eq!(execbase::LIBLIST, 0x17A);
    assert_eq!(execbase::PORTLIST, 0x188);
    assert_eq!(execbase::TASKREADY, 0x196);
    assert_eq!(execbase::TASKWAIT, 0x1A4);
    assert_eq!(execbase::VBLANKFREQUENCY, 0x212);
    assert_eq!(execbase::EX_ECLOCKFREQUENCY, 0x238);
}

#[test]
fn lvo_de_exec() {
    let casos = [
        ("Disable", -120),
        ("Enable", -126),
        ("Forbid", -132),
        ("Permit", -138),
        ("AllocMem", -198),
        ("FreeMem", -210),
        ("AvailMem", -216),
        ("Enqueue", -270),
        ("FindName", -276),
        ("AddTask", -282),
        ("RemTask", -288),
        ("FindTask", -294),
        ("SetTaskPri", -300),
        ("SetSignal", -306),
        ("Wait", -318),
        ("Signal", -324),
        ("AllocSignal", -330),
        ("PutMsg", -366),
        ("GetMsg", -372),
        ("ReplyMsg", -378),
        ("WaitPort", -384),
        ("OldOpenLibrary", -408),
        ("CloseLibrary", -414),
        ("SetFunction", -420),
        ("OpenDevice", -444),
        ("DoIO", -456),
        ("RawDoFmt", -522),
        ("TypeOfMem", -534),
        ("OpenLibrary", -552),
        ("CopyMem", -624),
        ("CacheClearU", -636),
        ("CreateMsgPort", -666),
        ("DeleteMsgPort", -672),
        ("AllocVec", -684),
        ("FreeVec", -690),
        ("CreatePool", -696),
        ("ObtainQuickVector", -786),
        ("MakeLibrary", -84),
        ("InitStruct", -78),
    ];
    for (nombre, desp) in casos {
        let n = lvo::por_nombre(nombre).unwrap();
        assert_eq!(lvo::lvo(n), desp, "{nombre}");
        assert_eq!(lvo::numero(desp), Some(n));
        assert_eq!(lvo::nombre(n), nombre);
    }
    assert_eq!(lvo::ENTRADAS, 137);
    assert_eq!(lvo::lvo(lvo::ENTRADAS), -822);
    assert_eq!(lvo::numero(-828), None);
    assert_eq!(lvo::numero(-200), None);
    assert_eq!(lvo::numero(6), None);
}

#[test]
fn registros_del_jit() {
    let mut x = [0u64; X_TOTAL];
    for (i, v) in x.iter_mut().enumerate() {
        *v = 0xDEAD_0000_0000_0000 | i as u64;
    }
    let r = Registros::desde_jit(&x).unwrap();
    assert_eq!(r.d[0], 19);
    assert_eq!(r.d[7], 26);
    assert_eq!(r.a[0], 13);
    assert_eq!(r.a[4], 17);
    assert_eq!(r.a[5], 27);
    assert_eq!(r.a[6], 28);
    assert_eq!(r.a[7], 29);
    let mut y = [0u64; X_TOTAL];
    assert!(r.hacia_jit(&mut y));
    assert_eq!(Registros::desde_jit(&y), Some(r));
    assert_eq!(y[19], 19, "los 32 bits de arriba quedan a 0");
    assert_eq!(Registros::desde_jit(&x[..20]), None);
    assert!(!r.hacia_jit(&mut y[..10]));
}

#[test]
fn memoria_big_endian_y_limites() {
    let mut m = MemoriaVec::nueva(0x100);
    m.escribir32(0x10, 0x1234_5678).unwrap();
    assert_eq!(m.leer(0x10, 4).unwrap(), &[0x12, 0x34, 0x56, 0x78]);
    assert_eq!(m.leer16(0x12).unwrap(), 0x5678);
    assert_eq!(m.leer8(0x10).unwrap(), 0x12);
    assert_eq!(m.leer32(0xFE), Err(Fallo::FueraDeMemoria { dir: 0xFE, largo: 4 }));
    assert!(m.escribir8(0x100, 1).is_err());
    assert!(m.leer32(u32::MAX).is_err());
    assert!(m.leer(u32::MAX - 1, 10).is_err());
}

#[test]
fn arranque() {
    let e = caja();
    let b = e.base();
    assert_eq!(l32(&e, 4), b);
    assert_eq!(e.leer_cadena(l32(&e, b + node::LN_NAME)).unwrap(), "exec.library");
    assert_eq!(e.mem.leer16(b + library::LIB_VERSION).unwrap(), 40);
    assert_eq!(l8(&e, b + node::LN_TYPE), nt::LIBRARY);
    assert_eq!(l8(&e, b + execbase::TDNESTCNT), 0xFF);
    assert_eq!(l8(&e, b + execbase::IDNESTCNT), 0xFF);
    assert_eq!(l32(&e, b + execbase::THISTASK), 0);
    assert_eq!(l32(&e, b + execbase::CHKBASE), !b);
    assert_eq!(e.mem.leer16(b + library::LIB_NEGSIZE).unwrap(), 824);
    // La entrada de AllocMem: $A0FF 33 RTS.
    let d = b - 198;
    assert_eq!(e.mem.leer(d, 6).unwrap(), &[0xA0, 0xFF, 0x00, 33, 0x4E, 0x75]);
    // exec está en LibList y se encuentra por nombre.
    assert_eq!(e.find_name(b + execbase::LIBLIST, b"exec.library").unwrap(), b);
    // Dos regiones: la fast (prioridad 0) delante de la chip (-10).
    let mh = e.nodos(b + execbase::MEMLIST).unwrap();
    assert_eq!(mh.len(), 2);
    assert_eq!(e.mem.leer16(mh[0] + memheader::MH_ATTRIBUTES).unwrap() as u32, memf::PUBLIC | memf::FAST);
    assert_eq!(e.leer_cadena(l32(&e, mh[1] + node::LN_NAME)).unwrap(), "chip memory");
    assert!(e.avail_mem(memf::CHIP).unwrap() > 1_900_000);
    assert!(e.avail_mem(memf::FAST).unwrap() > 2_000_000);
    assert_eq!(e.avail_mem(memf::ANY).unwrap(), e.avail_mem(memf::CHIP).unwrap() + e.avail_mem(memf::FAST).unwrap());
    assert!(e.avisos().is_empty());
}

#[test]
fn arranque_sin_sitio() {
    assert_eq!(Exec::arrancar(MemoriaVec::nueva(0x2000), Config::para(0x2000)).err(), Some(Fallo::SinSitio));
    // Con la zona del sistema, pero sin regiones.
    let mut c = Config::para(0x10000);
    c.regiones.clear();
    assert_eq!(Exec::arrancar(MemoriaVec::nueva(0x10000), c).err(), Some(Fallo::SinSitio));
}

#[test]
fn una_caja_solo_con_chip() {
    let e = Exec::arrancar(MemoriaVec::nueva(1 << 20), Config::para(1 << 20)).unwrap();
    assert_eq!(e.nodos(e.base() + execbase::MEMLIST).unwrap().len(), 1);
    assert_eq!(e.avail_mem(memf::FAST).unwrap(), 0);
}

/// La memoria de la caja de DashOS: la página 0 y, aparte, `[1 MB, 9 MB)`. Lo de en medio no existe.
struct MemoriaDeCaja {
    pagina0: Vec<u8>,
    ram: Vec<u8>,
}

const RAM: u32 = 0x10_0000;

impl Memoria for MemoriaDeCaja {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        let fin = dir.checked_add(largo)?;
        if fin <= 0x1000 {
            return self.pagina0.get(dir as usize..fin as usize);
        }
        let i = dir.checked_sub(RAM)? as usize;
        self.ram.get(i..i + largo as usize)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        let fin = dir.checked_add(largo)?;
        if fin <= 0x1000 {
            return self.pagina0.get_mut(dir as usize..fin as usize);
        }
        let i = dir.checked_sub(RAM)? as usize;
        self.ram.get_mut(i..i + largo as usize)
    }
}

#[test]
fn arranque_con_la_memoria_de_la_caja() {
    let m = MemoriaDeCaja { pagina0: vec![0; 0x1000], ram: vec![0; 8 << 20] };
    let c = Config::caja((0x10_2000, 0x11_0000), 0x15_0000, 0x90_0000);
    let mut e = Exec::arrancar(m, c).unwrap();
    let b = e.base();
    assert!((0x10_2000..0x11_0000).contains(&b));
    assert_eq!(e.mem.leer32(4).unwrap(), b);
    // Lo repartible empieza donde se pidió: nada entre la zona del sistema y 0x150000.
    let p = e.alloc_mem(64, memf::CHIP).unwrap();
    assert!((0x15_0000..0x20_0000).contains(&p));
    let q = e.alloc_mem(64, memf::FAST).unwrap();
    assert!((0x20_0000..0x90_0000).contains(&q));
    assert_eq!(e.alloc_abs(16, 0x14_0000).unwrap(), 0);
    // Una zona del sistema en la página 0, o que no está en memoria, no vale.
    let m = MemoriaDeCaja { pagina0: vec![0; 0x1000], ram: vec![0; 8 << 20] };
    assert_eq!(Exec::arrancar(m, Config::caja((0, 0x4000), 0x15_0000, 0x90_0000)).err(), Some(Fallo::SinSitio));
    let m = MemoriaDeCaja { pagina0: vec![0; 0x1000], ram: vec![0; 8 << 20] };
    assert_eq!(Exec::arrancar(m, Config::caja((0x1000, 0x4000), 0x15_0000, 0x90_0000)).err(), Some(Fallo::SinSitio));
}
