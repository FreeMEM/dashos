//! La memoria que crece (la caja Amiga): exec pide más cuando un `AllocMem` no cabe, la región de arriba crece, y
//! nunca se pasa del tope; `AvailMem` cuenta lo que queda por crecer. Lo hostil: un programa que pide sin fin.

use caja_exec::estructuras::memf;
use caja_exec::{Config, Exec, Memoria, MemoriaVec};

/// Una memoria de `visible` bytes que crece de `trozo` en `trozo` (o lo que pidan, si es más) hasta `tope`.
struct Creciente {
    m: MemoriaVec,
    visible: u32,
    tope: u32,
    trozo: u32,
    veces: u32,
}

impl Memoria for Creciente {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        if dir.checked_add(largo)? > self.visible {
            return None;
        }
        self.m.trozo(dir, largo)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        if dir.checked_add(largo)? > self.visible {
            return None;
        }
        self.m.trozo_mut(dir, largo)
    }
    fn ampliar(&mut self, minimo: u32) -> Option<(u32, u32)> {
        let antes = self.visible;
        let quiero = minimo.max(self.trozo);
        let despues = antes.saturating_add(quiero).min(self.tope);
        if despues <= antes {
            return None;
        }
        self.visible = despues;
        self.veces += 1;
        Some((antes, despues))
    }
    fn margen(&self) -> u32 {
        self.tope - self.visible
    }
}

fn caja(tope: u32) -> (Exec<Creciente>, u32) {
    let mem = Creciente { m: MemoriaVec::nueva(tope as usize), visible: 4 << 20, tope, trozo: 1 << 20, veces: 0 };
    let mut e = Exec::arrancar(mem, Config::para(4 << 20)).expect("arranca");
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    (e, t)
}

#[test]
fn crece_lo_que_haga_falta() {
    let (mut e, _t) = caja(64 << 20);
    let antes = e.avail_mem(memf::ANY).unwrap();
    assert!(antes > 60 << 20, "AvailMem cuenta lo que puede crecer: {antes}");
    assert_eq!(e.avail_mem(memf::CHIP).unwrap() < 2 << 20, true);
    // Un bloque de 10 MB, más grande que todo lo de al principio: cabe tras crecer.
    let a = e.alloc_mem(10 << 20, memf::CLEAR).unwrap();
    assert_ne!(a, 0);
    assert!(e.mem.veces >= 1);
    assert_eq!(e.mem.leer32(a + (10 << 20) - 4).unwrap(), 0);
    // La chip no crece.
    assert_eq!(e.alloc_mem(8 << 20, memf::CHIP).unwrap(), 0);
    // Lo soltado se puede volver a pedir sin crecer más.
    let veces = e.mem.veces;
    e.free_mem(a, 10 << 20).unwrap();
    let b = e.alloc_mem(10 << 20, 0).unwrap();
    assert_ne!(b, 0);
    assert_eq!(e.mem.veces, veces);
    // TypeOfMem de lo nuevo: fast.
    assert_ne!(e.type_of_mem(b + (9 << 20)).unwrap() & memf::FAST, 0);
    assert!(e.tomar_avisos().is_empty());
}

#[test]
fn pedir_sin_fin_no_pasa_del_tope() {
    let tope = 32 << 20;
    let (mut e, _t) = caja(tope);
    let mut total: u64 = 0;
    for _ in 0..10_000 {
        let d = e.alloc_mem(256 << 10, 0).unwrap();
        if d == 0 {
            break;
        }
        total += 256 << 10;
    }
    assert!(total < tope as u64 && total > (tope as u64) * 3 / 4, "{total}");
    assert_eq!(e.mem.visible, tope);
    assert_eq!(e.avail_mem(memf::LARGEST).unwrap() < 256 << 10, true);
    // Un tamaño absurdo: 0, sin crecer más ni pánico.
    assert_eq!(e.alloc_mem(u32::MAX - 16, 0).unwrap(), 0);
    assert_eq!(e.alloc_mem(1 << 31, 0).unwrap(), 0);
    assert_eq!(e.mem.visible, tope);
}

/// Una biblioteca nativa cuyas funciones dejan los códigos de condición (mathieeesingbas: `IEEESPCmp`): su entrada
/// salta a `LINEA_A n` / `TST.L D0` / `RTS`.
#[test]
fn nativa_con_codigos_de_condicion() {
    let (mut e, _t) = caja(8 << 20);
    let lib = e.crear_biblioteca_nativa_con_cc("x.library", 40, 1, &[0x900, 0x901], 34, &[0x901]).unwrap();
    // La primera, una entrada nativa normal; la segunda, un salto a su trozo.
    assert_eq!(e.mem.leer16(lib - 30).unwrap(), 0xA0FF);
    assert_eq!(e.mem.leer16(lib - 36).unwrap(), 0x4EF9);
    let trozo = e.mem.leer32(lib - 34).unwrap();
    assert_eq!(e.mem.leer16(trozo).unwrap(), 0xA0FF);
    assert_eq!(e.mem.leer16(trozo + 2).unwrap(), 0x901);
    assert_eq!(e.mem.leer16(trozo + 4).unwrap(), 0x4A80);
    assert_eq!(e.mem.leer16(trozo + 6).unwrap(), 0x4E75);
    // Un número que no es suyo: error, sin pánico.
    assert!(e.crear_biblioteca_nativa_con_cc("y.library", 40, 1, &[0x910], 34, &[0x999]).is_err());
}
