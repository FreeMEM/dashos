//! Lo común de las pruebas: una caja de mentira (exec con `MemoriaVec`), una salida que guarda lo que suena y el
//! bucle de la caja (`atender` cada poco, ejecutando los ganchos con una función de Rust en vez de 68k).

#![allow(dead_code)]

use caja_exec::estructuras::{memf, node, nt};
use caja_exec::{Accion, Config, Destino, Exec, Memoria, MemoriaVec, Registros, io, lvo};
use caja_sonido::*;

pub const TAMANO: u32 = 4 << 20;
pub const PRIMERO: u16 = 0x1100;

/// La salida de mentira: guarda todo lo escrito; `cabe` es lo que acepta cada vez.
pub struct SalidaPrueba {
    pub escritos: Vec<[i16; 2]>,
    pub cabe: usize,
    /// Con `Some`, cuántos de los escritos ya han sonado (el reloj del sonido).
    pub sonado: Option<usize>,
}

impl Salida for SalidaPrueba {
    fn hueco(&self) -> usize {
        self.cabe
    }
    fn escribir(&mut self, c: &[[i16; 2]]) {
        assert!(c.len() <= self.cabe);
        self.escritos.extend_from_slice(c);
    }
    fn pendientes(&self) -> Option<usize> {
        self.sonado.map(|s| self.escritos.len().saturating_sub(s))
    }
}

pub struct Caja {
    pub e: Exec<MemoriaVec>,
    pub s: Sonido,
    pub tarea: u32,
    pub ahora: u64,
    pub salida: SalidaPrueba,
    /// Los ganchos que se han ejecutado, con el cuadro producido entonces.
    pub ganchos: Vec<(Gancho, u64)>,
}

pub fn caja() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).unwrap();
    let tarea = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(tarea));
    let s = Sonido::instalar(&mut e, PRIMERO).unwrap();
    Caja {
        e,
        s,
        tarea,
        ahora: 1_000_000,
        salida: SalidaPrueba { escritos: Vec::new(), cabe: 1 << 20, sonado: None },
        ganchos: Vec::new(),
    }
}

impl Caja {
    /// Una función de exec por su nombre, siguiendo lo que pida (el Open, el BeginIO...), como la caja.
    pub fn exec(&mut self, f: &str, r: &mut Registros) -> Accion {
        let n = lvo::por_nombre(f).unwrap();
        let mut a = self.e.llamar(n, r);
        for _ in 0..8 {
            match a {
                Accion::Llamar { destino: Destino::Nativa(m), regs, tras } => {
                    let mut rr = regs;
                    if self.s.es_mio(m) {
                        self.s.llamar(&mut self.e, m, &mut rr);
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

    pub fn io(&mut self, f: &str, ior: u32) -> (Accion, u32) {
        let mut r = Registros::default();
        r.a[1] = ior;
        let a = self.exec(f, &mut r);
        (a, r.d[0])
    }

    /// Una función de la biblioteca de AHI (A6 = la base de ahi.device).
    pub fn lib(&mut self, f: &str, d: &[u32], a: &[(usize, u32)]) -> u32 {
        let n = self.s.numero(f).unwrap();
        let mut r = Registros::default();
        r.d[..d.len()].copy_from_slice(d);
        for &(i, v) in a {
            r.a[i] = v;
        }
        r.a[6] = self.s.base_ahi();
        assert_eq!(self.s.llamar(&mut self.e, n, &mut r), Accion::Seguir);
        r.d[0]
    }

    pub fn reservar(&mut self, n: u32) -> u32 {
        let d = self.e.alloc_mem(n, memf::PUBLIC | memf::CLEAR).unwrap();
        assert_ne!(d, 0);
        d
    }

    pub fn cadena(&mut self, s: &str) -> u32 {
        let d = self.reservar(s.len() as u32 + 1);
        self.e.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s.as_bytes());
        d
    }

    pub fn bytes(&mut self, b: &[u8]) -> u32 {
        let d = self.reservar(b.len() as u32);
        self.e.mem.leer_mut(d, b.len() as u32).unwrap().copy_from_slice(b);
        d
    }

    /// Una lista de etiquetas en la memoria del 68k.
    pub fn etiquetas(&mut self, t: &[(u32, u32)]) -> u32 {
        let d = self.reservar(8 * t.len() as u32 + 8);
        for (i, &(a, b)) in t.iter().enumerate() {
            self.e.mem.escribir32(d + 8 * i as u32, a).unwrap();
            self.e.mem.escribir32(d + 8 * i as u32 + 4, b).unwrap();
        }
        d
    }

    /// Puerto, petición de `tamano` y OpenDevice. Devuelve (puerto, petición, error).
    pub fn abrir(&mut self, disp: &str, unidad: u32, tamano: u32, version: u16) -> (u32, u32, i8) {
        let p = self.e.create_msg_port().unwrap();
        let ior = self.e.create_io_request(p, tamano).unwrap();
        assert_ne!(ior, 0);
        if tamano >= 50 {
            self.e.mem.escribir16(ior + AHIR_VERSION, version).unwrap();
        }
        let nombre = self.cadena(disp);
        let mut r = Registros::default();
        r.a[0] = nombre;
        r.d[0] = unidad;
        r.a[1] = ior;
        assert_eq!(self.exec("OpenDevice", &mut r), Accion::Seguir);
        (p, ior, r.d[0] as i8)
    }

    /// Una copia de una petición abierta (como hace un programa para tener dos): misma memoria, mismo puerto.
    pub fn copia(&mut self, ior: u32, tamano: u32) -> u32 {
        let p = self.e.mem.leer32(ior + 14).unwrap();
        let n = self.e.create_io_request(p, tamano).unwrap();
        let b = self.e.mem.leer(ior, tamano).unwrap().to_vec();
        self.e.mem.leer_mut(n, tamano).unwrap().copy_from_slice(&b);
        self.e.mem.escribir8(n + node::LN_TYPE, nt::REPLYMSG).unwrap();
        n
    }

    pub fn error(&self, ior: u32) -> i8 {
        self.e.mem.leer8(ior + io::IO_ERROR).unwrap() as i8
    }

    /// Si la petición ha vuelto a su puerto (está en su lista).
    pub fn ha_vuelto(&self, ior: u32) -> bool {
        self.e.mem.leer8(ior + node::LN_TYPE).unwrap() == nt::REPLYMSG
    }

    /// Saca la petición de su puerto si ha vuelto y es la primera (como `GetMsg`).
    pub fn recoger(&mut self, ior: u32) -> bool {
        if !self.ha_vuelto(ior) {
            return false;
        }
        let p = self.e.mem.leer32(ior + 14).unwrap();
        if self.e.mem.leer32(p + caja_exec::estructuras::msgport::MP_MSGLIST).unwrap() != ior {
            return false;
        }
        assert_eq!(self.e.get_msg(p).unwrap(), ior);
        true
    }

    /// Pasa `us` de tiempo llamando a `atender` cada `paso` us, como la caja; los ganchos los ejecuta `f`.
    pub fn pasar_con(&mut self, us: u64, paso: u64, f: &mut dyn FnMut(&mut Caja, Gancho)) {
        let fin = self.ahora + us;
        while self.ahora < fin {
            self.ahora = (self.ahora + paso).min(fin);
            self.atender_con(f);
        }
    }

    pub fn atender_con(&mut self, f: &mut dyn FnMut(&mut Caja, Gancho)) {
        for _ in 0..10_000 {
            let Some(g) = self.s.atender(&mut self.e, &mut self.salida, self.ahora, true) else { return };
            let p = self.s.producidos();
            self.ganchos.push((g, p));
            f(self, g);
            self.s.gancho_hecho();
        }
        panic!("demasiados ganchos seguidos");
    }

    pub fn pasar(&mut self, us: u64) {
        self.pasar_con(us, 1_000, &mut |_, _| {});
    }

    /// Los avisos de exec y de sonido, y los vacía.
    pub fn avisos(&mut self) -> Vec<String> {
        let mut v: Vec<String> =
            self.e.tomar_avisos().iter().map(|a| format!("exec {}: {:?}", a.funcion, a.fallo)).collect();
        v.extend(self.s.tomar_avisos().iter().map(|a| format!("{}: {:?}", a.funcion, a.fallo)));
        v
    }

    pub fn sin_avisos(&mut self) {
        let a = self.avisos();
        assert!(a.is_empty(), "{a:#?}");
    }
}

/// Microsegundos de `n` cuadros a 48 kHz.
pub fn us(n: u64) -> u64 {
    n * 1_000_000 / FRECUENCIA as u64
}
