//! ramlib: `OpenLibrary` y `OpenDevice` de lo que no está en memoria piden a la caja que lo cargue; exec busca el
//! `RomTag`, lo inicia, lo abre y, al cerrarlo el último, lo echa (`Expunge`) y suelta sus segmentos. La "caja" de
//! estas pruebas es un bucle que hace lo que haría la de verdad: carga una biblioteca hecha a mano y "ejecuta" su
//! código 68k (su `init` y su `Expunge` son direcciones que el bucle reconoce). Y lo malo: RomTag falso, tabla de
//! saltos fuera de la memoria, versión que miente, cerrar dos veces, nombres raros, una que se abre a sí misma desde
//! su `init`, una que se llama como una nativa.

mod comun;

use caja_exec::estructuras::*;
use caja_exec::ramlib::resident as rt;
use caja_exec::{Accion, Destino, Entrada, Exec, Memoria, MemoriaVec, Registros, io, lvo};
use comun::*;

const N_CARGAR: u16 = 0x900;
const N_DESCARGAR: u16 = 0x901;
const INIT: u32 = 0x00F0_0010;
const EXPUNGE: u32 = 0x00F0_0003;

/// Cómo es la biblioteca de disco de la prueba.
#[derive(Clone, Copy)]
struct Disco {
    /// El `rt_MatchTag` apunta a otro sitio (un RomTag falso).
    romtag_falso: bool,
    /// La tabla de vectores apunta fuera de la memoria.
    vectores_fuera: bool,
    /// La versión que pone su `init` en la base.
    version: u16,
    /// El nombre del RomTag (y de la base).
    nombre: &'static str,
    /// Un dispositivo.
    dispositivo: bool,
    /// El `init` abre la propia biblioteca (que se está cargando).
    se_abre: bool,
    /// Sin RomTag.
    sin_romtag: bool,
}

fn c_str(e: &Exec<MemoriaVec>, d: u32) -> String {
    let mut v = Vec::new();
    while let Ok(b) = e.mem.leer8(d + v.len() as u32) {
        if b == 0 || v.len() > 512 {
            break;
        }
        v.push(b);
    }
    String::from_utf8_lossy(&v).into_owned()
}

const BUENA: Disco = Disco {
    romtag_falso: false,
    vectores_fuera: false,
    version: 40,
    nombre: "disco.library",
    dispositivo: false,
    se_abre: false,
    sin_romtag: false,
};

struct Caja {
    e: Exec<MemoriaVec>,
    disco: Disco,
    cargas: Vec<(String, u32)>,
    soltadas: Vec<u32>,
    expunges: u32,
}

impl Caja {
    fn nueva(disco: Disco) -> Caja {
        let (mut e, _t) = caja_con_tarea();
        e.poner_ramlib(N_CARGAR, N_DESCARGAR);
        Caja { e, disco, cargas: Vec::new(), soltadas: Vec::new(), expunges: 0 }
    }

    /// Hace la lista de segmentos de la biblioteca de disco (un segmento: el RomTag, su tabla, sus vectores y su nombre).
    fn hacer_segmentos(&mut self) -> u32 {
        let e = &mut self.e;
        let bloque = e.alloc_mem(256, memf::CLEAR).unwrap();
        e.mem.escribir32(bloque, 256).unwrap();
        e.mem.escribir32(bloque + 4, 0).unwrap();
        let c = bloque + 8;
        let r = c + 16; // con algo delante: se busca
        let tabla = r + rt::TAMANO + 2;
        let vectores = tabla + 16;
        let nombre = vectores + 4 * 5;
        let b = e.base();
        // Los vectores: Open y Close nativos (los trampolines de exec), Expunge "68k" y la reservada.
        let open = e.trampolin(1).unwrap_or(b - 6);
        let close = e.trampolin(2).unwrap_or(b - 12);
        let res = e.trampolin(4).unwrap_or(b - 24);
        if !self.disco.sin_romtag {
            e.mem.escribir16(r, rt::RTC_MATCHWORD).unwrap();
            e.mem.escribir32(r + rt::RT_MATCHTAG, if self.disco.romtag_falso { r + 2 } else { r }).unwrap();
        }
        e.mem.escribir8(r + rt::RT_FLAGS, rt::RTF_AUTOINIT).unwrap();
        e.mem.escribir8(r + rt::RT_VERSION, self.disco.version as u8).unwrap();
        e.mem.escribir8(r + rt::RT_TYPE, if self.disco.dispositivo { nt::DEVICE } else { nt::LIBRARY }).unwrap();
        e.mem.escribir32(r + rt::RT_NAME, nombre).unwrap();
        e.mem.escribir32(r + rt::RT_INIT, tabla).unwrap();
        e.mem.escribir32(tabla, library::TAMANO).unwrap();
        e.mem.escribir32(tabla + 4, if self.disco.vectores_fuera { 0xFFFF_FF00 } else { vectores }).unwrap();
        e.mem.escribir32(tabla + 8, 0).unwrap();
        e.mem.escribir32(tabla + 12, INIT).unwrap();
        for (k, v) in [open, close, EXPUNGE, res, u32::MAX].into_iter().enumerate() {
            e.mem.escribir32(vectores + 4 * k as u32, v).unwrap();
        }
        e.mem.leer_mut(nombre, self.disco.nombre.len() as u32).unwrap().copy_from_slice(self.disco.nombre.as_bytes());
        (bloque + 4) >> 2
    }

    /// Lo que haría la caja con lo que pide exec, hasta que la llamada acabe.
    fn seguir(&mut self, mut a: Accion, r: &mut Registros) {
        for _ in 0..64 {
            let Accion::Llamar { destino, regs, tras } = a else { return };
            let mut rr = regs;
            match destino {
                Destino::Nativa(N_CARGAR) => {
                    let n = c_str(&self.e, rr.a[1]);
                    let seg = self.hacer_segmentos();
                    self.cargas.push((n, rr.d[1]));
                    rr.d[0] = seg;
                }
                Destino::Nativa(N_DESCARGAR) => {
                    self.soltadas.push(rr.d[1]);
                    rr.d[0] = 0;
                }
                Destino::Nativa(n) => {
                    self.e.llamar(n, &mut rr);
                }
                Destino::Codigo68k(INIT) => {
                    // El init de la biblioteca: pone su nombre y versión (con D0 = la base) y la devuelve.
                    let lib = rr.d[0];
                    let seg_nombre = {
                        let s = rr.a[0] << 2;
                        s + 4 + 16 + rt::TAMANO + 2 + 16 + 20
                    };
                    self.e.mem.escribir32(lib + node::LN_NAME, seg_nombre).unwrap();
                    self.e.mem.escribir16(lib + library::LIB_VERSION, self.disco.version).unwrap();
                    if self.disco.se_abre {
                        let mut r2 = regs;
                        r2.a[1] = seg_nombre;
                        r2.d[0] = 0;
                        let acc = llamar(&mut self.e, "OpenLibrary", &mut r2);
                        assert_eq!(acc, Accion::Seguir, "no se vuelve a cargar");
                        assert_eq!(r2.d[0], 0);
                    }
                }
                Destino::Codigo68k(EXPUNGE) => {
                    // Su Expunge: fuera de la lista y devuelve su lista de segmentos (no la soltó).
                    self.expunges += 1;
                    let lib = rr.a[6];
                    self.e.remove(lib).unwrap();
                    let seg = self.e.cargadas_de_disco().iter().find(|(b, _)| *b == lib).map(|x| x.1).unwrap_or(0);
                    rr.d[0] = seg;
                }
                Destino::Codigo68k(d) => match self.e.entrada(d) {
                    Ok(Entrada::Nativa(n)) => {
                        self.e.llamar(n, &mut rr);
                    }
                    _ => panic!("68k en ${d:x} que la prueba no sabe ejecutar"),
                },
            }
            a = self.e.continuar(tras, &rr, r);
        }
        panic!("demasiadas vueltas");
    }

    fn open_library(&mut self, nombre: &str, version: u32) -> u32 {
        let s = cadena(&mut self.e, nombre);
        let mut r = Registros::default();
        r.a[1] = s;
        r.d[0] = version;
        let a = llamar(&mut self.e, "OpenLibrary", &mut r);
        self.seguir(a, &mut r);
        r.d[0]
    }

    fn close_library(&mut self, lib: u32) {
        let mut r = Registros::default();
        r.a[1] = lib;
        let a = llamar(&mut self.e, "CloseLibrary", &mut r);
        self.seguir(a, &mut r);
    }
}

#[test]
fn cargar_abrir_cerrar_y_echar() {
    let mut c = Caja::nueva(BUENA);
    let lib = c.open_library("disco.library", 37);
    assert_ne!(lib, 0);
    assert_eq!(c.cargas, vec![("disco.library".to_string(), 0)]);
    assert_eq!(c.e.mem.leer16(lib + library::LIB_OPENCNT).unwrap(), 1);
    assert_eq!(c.e.cargadas_de_disco().len(), 1);
    // Otra apertura: ya está en memoria, no se carga otra vez.
    let lib2 = c.open_library("disco.library", 0);
    assert_eq!(lib2, lib);
    assert_eq!(c.cargas.len(), 1);
    // Con camino: "LIBS:disco.library" y "algo/disco.library" encuentran la de memoria.
    assert_eq!(c.open_library("LIBS:disco.library", 0), lib);
    assert_eq!(c.cargas.len(), 1);
    c.close_library(lib);
    c.close_library(lib);
    assert_eq!(c.expunges, 0, "aún abierta");
    c.close_library(lib);
    assert_eq!(c.expunges, 1, "el último cierre la echa");
    assert_eq!(c.soltadas.len(), 1, "y suelta sus segmentos");
    assert!(c.e.cargadas_de_disco().is_empty());
    // Ya no está: se carga otra vez.
    let lib3 = c.open_library("disco.library", 0);
    assert_ne!(lib3, 0);
    assert_eq!(c.cargas.len(), 2);
}

#[test]
fn lo_nativo_gana() {
    let mut c = Caja::nueva(BUENA);
    // exec.library está: nunca se pide a la caja.
    let b = c.open_library("exec.library", 0);
    assert_eq!(b, c.e.base());
    assert!(c.cargas.is_empty());
    // Un RomTag que se llama como una que ya está: se carga, no se inicia, se suelta.
    let mut c = Caja::nueva(Disco { nombre: "exec.library", ..BUENA });
    assert_eq!(c.open_library("otra.library", 0), 0);
    assert_eq!(c.cargas.len(), 1);
    assert_eq!(c.soltadas.len(), 1);
    assert!(c.e.cargadas_de_disco().is_empty());
}

#[test]
fn hostiles() {
    for (que, disco) in [
        ("RomTag falso", Disco { romtag_falso: true, ..BUENA }),
        ("sin RomTag", Disco { sin_romtag: true, ..BUENA }),
        ("tabla de saltos fuera de la memoria", Disco { vectores_fuera: true, ..BUENA }),
    ] {
        let mut c = Caja::nueva(disco);
        assert_eq!(c.open_library("disco.library", 0), 0, "{que}");
        assert_eq!(c.soltadas.len(), 1, "{que}: lo cargado se suelta");
        assert!(c.e.cargadas_de_disco().is_empty(), "{que}");
        assert!(!c.e.esta_en_memoria(b"disco.library"), "{que}");
    }
    // Versión que miente (dice 30 y se pide la 40): no se abre, y se echa en seguida.
    let mut c = Caja::nueva(Disco { version: 30, ..BUENA });
    assert_eq!(c.open_library("disco.library", 40), 0);
    assert_eq!(c.expunges, 1);
    assert_eq!(c.soltadas.len(), 1);
    // Cerrar dos veces: aviso, nada más.
    let mut c = Caja::nueva(BUENA);
    let lib = c.open_library("disco.library", 0);
    c.close_library(lib);
    let avisos = c.e.avisos().len();
    c.close_library(lib);
    assert!(c.e.avisos().len() > avisos, "el segundo cierre se apunta");
    assert_eq!(c.soltadas.len(), 1);
    // Nombres raros: se piden tal cual (la caja los rechaza); uno enorme o vacío, ni se pide.
    let mut c = Caja::nueva(BUENA);
    let largo = "x".repeat(300);
    assert_eq!(c.open_library(&largo, 0), 0);
    assert_eq!(c.open_library("", 0), 0);
    assert!(c.cargas.is_empty());
    c.open_library("LIBS:../../../etc/passwd", 0);
    assert_eq!(c.cargas[0].0, "LIBS:../../../etc/passwd");
    // Una que se abre a sí misma desde su init: no se carga otra vez (el OpenLibrary de dentro da 0).
    let mut c = Caja::nueva(Disco { se_abre: true, ..BUENA });
    assert_ne!(c.open_library("disco.library", 0), 0);
    assert_eq!(c.cargas.len(), 1);
    // Sin ramlib (otra caja): ni se pide.
    let (mut e, _t) = caja_con_tarea();
    let s = cadena(&mut e, "disco.library");
    let mut r = Registros::default();
    r.a[1] = s;
    assert_eq!(llamar(&mut e, "OpenLibrary", &mut r), Accion::Seguir);
    assert_eq!(r.d[0], 0);
}

#[test]
fn dispositivo_de_disco() {
    let mut c = Caja::nueva(Disco { dispositivo: true, nombre: "disco.device", ..BUENA });
    let ior = c.e.alloc_mem(io::TAMANO + 16, memf::CLEAR).unwrap();
    let s = cadena(&mut c.e, "disco.device");
    let mut r = Registros::default();
    r.a[0] = s;
    r.a[1] = ior;
    let a = llamar(&mut c.e, "OpenDevice", &mut r);
    c.seguir(a, &mut r);
    assert_eq!(r.d[0], 0, "abierto");
    assert_eq!(c.cargas, vec![("disco.device".to_string(), 1)]);
    let d = c.e.mem.leer32(ior + io::IO_DEVICE).unwrap();
    assert_ne!(d, 0);
    let mut r = Registros::default();
    r.a[1] = ior;
    let a = llamar(&mut c.e, "CloseDevice", &mut r);
    c.seguir(a, &mut r);
    assert_eq!(c.expunges, 1);
    assert_eq!(c.soltadas.len(), 1);
    // Uno que no carga: IOERR_OPENFAIL.
    let mut c = Caja::nueva(Disco { sin_romtag: true, dispositivo: true, ..BUENA });
    let ior = c.e.alloc_mem(io::TAMANO + 16, memf::CLEAR).unwrap();
    let s = cadena(&mut c.e, "otro.device");
    let mut r = Registros::default();
    r.a[0] = s;
    r.a[1] = ior;
    let a = llamar(&mut c.e, "OpenDevice", &mut r);
    c.seguir(a, &mut r);
    assert_eq!(r.d[0] as i32, io::IOERR_OPENFAIL as i32);
    let _ = lvo::ENTRADAS;
}
