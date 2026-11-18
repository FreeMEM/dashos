//! La memoria que crece (la caja Amiga): exec pide más cuando un `AllocMem` no cabe, la región de arriba crece, y
//! nunca se pasa del tope; `AvailMem` cuenta lo que queda por crecer. Lo hostil: un programa que pide sin fin.

use caja_exec::estructuras::memf;
use caja_exec::estructuras::interrupt;
use caja_exec::memoria_baja::memh;
use caja_exec::{Accion, Config, Continuacion, Destino, Exec, Memoria, MemoriaVec, Registros};

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
    /// Suelta de MB en MB (lo de antes de crecer, nunca).
    fn encoger(&mut self, desde: u32) -> Option<u32> {
        let nuevo = desde.next_multiple_of(1 << 20).max(4 << 20);
        if nuevo >= self.visible {
            return None;
        }
        self.visible = nuevo;
        Some(nuevo)
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

/// aviso-cajas: con memoria baja en DashOS, lo crecido que está libre al final se devuelve; lo que está en uso, no.
#[test]
fn memoria_baja_suelta_lo_crecido_libre() {
    let (mut e, _t) = caja(64 << 20);
    let a = e.alloc_mem(10 << 20, 0).unwrap();
    let b = e.alloc_mem(8 << 20, 0).unwrap();
    assert!(e.mem.visible >= 22 << 20, "{}", e.mem.visible >> 20);
    // Sin aviso, nada.
    e.soltar_sin_tarea();
    assert_eq!(e.tomar_soltado(), None);
    // Con `b` en uso, solo lo de encima de `b`.
    e.free_mem(a, 10 << 20).unwrap();
    let total = e.avail_mem(memf::TOTAL).unwrap();
    e.pedir_soltar();
    e.soltar_sin_tarea();
    let s = e.tomar_soltado().expect("lo soltado");
    assert_eq!(s.manejadores, 0);
    assert!(e.mem.visible >= b + (8 << 20) && e.mem.visible < b + (9 << 20), "{:x} {:x}", e.mem.visible, b);
    assert_eq!(s.fin_despues, e.mem.visible);
    assert_eq!(s.bytes, s.fin_antes - s.fin_despues);
    // AvailMem(MEMF_TOTAL) cuenta también lo que puede crecer: lo mismo que antes.
    assert_eq!(e.avail_mem(memf::TOTAL).unwrap(), total);
    // Soltado `b`, todo lo crecido vuelve; y después la memoria crece otra vez si hace falta.
    e.free_mem(b, 8 << 20).unwrap();
    e.pedir_soltar();
    e.soltar_sin_tarea();
    assert!(e.tomar_soltado().unwrap().bytes > 0);
    assert_eq!(e.mem.visible, 4 << 20);
    let c = e.alloc_mem(12 << 20, memf::CLEAR).unwrap();
    assert_ne!(c, 0);
    assert_eq!(e.mem.leer32(c + (12 << 20) - 4).unwrap(), 0);
    e.free_mem(c, 12 << 20).unwrap();
    assert!(e.tomar_avisos().is_empty(), "{:?}", e.tomar_avisos());
}

/// Una `struct Interrupt` de manejador, en memoria reservada, con su código en `codigo`.
fn manejador(e: &mut Exec<Creciente>, codigo: u32, dato: u32, pri: i8) -> u32 {
    let is = e.alloc_mem(interrupt::TAMANO, memf::CLEAR).unwrap();
    e.mem.escribir32(is + interrupt::IS_CODE, codigo).unwrap();
    e.mem.escribir32(is + interrupt::IS_DATA, dato).unwrap();
    e.mem.escribir8(is + 9, pri as u8).unwrap();
    let mut r = Registros::default();
    r.a[1] = is;
    assert_eq!(e.llamar(129, &mut r), Accion::Seguir);
    assert!(e.tomar_avisos().is_empty());
    is
}

/// Lo que pide exec al llamar a un manejador: su código y sus registros.
fn llamado(a: Accion) -> (u32, Registros, Continuacion) {
    match a {
        Accion::Llamar { destino: Destino::Codigo68k(d), regs, tras } => (d, regs, tras),
        otra => panic!("se esperaba un manejador: {otra:?}"),
    }
}

/// aviso-cajas: los manejadores de los programas (`AddMemHandler`) se llaman en su orden, con su `MemHandlerData`, y
/// `MEM_TRY_AGAIN` los repite con `MEMHF_RECYCLE`; después se suelta y la llamada del programa sigue (aquí `AvailMem`,
/// con su D1). Uno quitado (`RemMemHandler`) no se llama.
#[test]
fn memoria_baja_llama_a_los_manejadores() {
    let (mut e, _t) = caja(64 << 20);
    let a = e.alloc_mem(12 << 20, 0).unwrap();
    e.free_mem(a, 12 << 20).unwrap();
    // Su código, en memoria del programa (no en la zona del sistema, que tiene trampolines nativos).
    let c = e.alloc_mem(0x100, memf::CLEAR).unwrap();
    let (c1, c2, c3) = (c, c + 0x40, c + 0x80);
    let bajo = manejador(&mut e, c1, 0x1111, -5);
    let _alto = manejador(&mut e, c2, 0x2222, 10);
    let quitado = manejador(&mut e, c3, 0x3333, 0);
    let mut r = Registros::default();
    r.a[1] = quitado;
    e.llamar(130, &mut r);
    e.pedir_soltar();
    assert!(e.soltar_pendiente());
    assert!(e.tomar_avisos().is_empty());
    // El programa llama a AvailMem(MEMF_ANY): antes, los manejadores.
    let mut r = Registros::default();
    r.d[0] = 0xdead;
    r.d[1] = memf::ANY;
    let (codigo, regs, tras) = llamado(e.empezar_a_soltar(36, &mut r));
    assert_eq!((codigo, regs.a[1], regs.a[6]), (c2, 0x2222, e.base()));
    let datos = regs.a[0];
    assert_eq!(e.mem.leer32(datos + memh::FLAGS).unwrap(), 0);
    assert!(e.mem.leer32(datos + memh::REQUEST_SIZE).unwrap() >= 4 << 20);
    // El primero dice "otra vez": se repite con MEMHF_RECYCLE.
    let mut hecho = regs;
    hecho.d[0] = memh::MEM_TRY_AGAIN;
    let (codigo, regs, tras) = llamado(e.continuar(tras, &hecho, &mut r));
    assert_eq!(codigo, c2);
    assert_eq!(e.mem.leer32(datos + memh::FLAGS).unwrap(), memh::MEMHF_RECYCLE);
    assert_eq!(r.d[0], 0xdead, "el D0 del programa no se pierde");
    let mut hecho = regs;
    hecho.d[0] = memh::MEM_ALL_DONE;
    let (codigo, regs, tras) = llamado(e.continuar(tras, &hecho, &mut r));
    assert_eq!((codigo, regs.a[1]), (c1, 0x1111));
    assert_eq!(e.mem.leer32(datos + memh::FLAGS).unwrap(), 0);
    // El último no hace nada: se suelta y sigue el AvailMem del programa.
    let mut hecho = regs;
    hecho.d[0] = memh::MEM_DID_NOTHING;
    assert_eq!(e.continuar(tras, &hecho, &mut r), Accion::Seguir);
    let s = e.tomar_soltado().expect("lo soltado");
    assert_eq!((s.manejadores, s.llamadas), (2, 3));
    assert!(s.bytes >= 11 << 20, "{}", s.bytes >> 20);
    assert_eq!(r.d[0], e.avail_mem(memf::ANY).unwrap());
    assert!(!e.soltar_pendiente());
    // Un manejador que dice "otra vez" sin fin se deja a las 4 repeticiones.
    let mut r = Registros::default();
    r.a[1] = bajo;
    e.llamar(130, &mut r);
    e.pedir_soltar();
    let mut r = Registros::default();
    r.d[1] = memf::ANY;
    let (_, mut regs, mut tras) = llamado(e.empezar_a_soltar(36, &mut r));
    let mut veces = 1;
    loop {
        regs.d[0] = memh::MEM_TRY_AGAIN;
        match e.continuar(tras, &regs, &mut r) {
            Accion::Llamar { destino: Destino::Codigo68k(d), regs: rr, tras: t } if d == c2 => {
                veces += 1;
                regs = rr;
                tras = t;
            }
            Accion::Seguir => break,
            otra => panic!("{otra:?}"),
        }
    }
    assert_eq!(veces, 5);
    assert!(e.tomar_avisos().is_empty(), "{:?}", e.tomar_avisos());
}

/// Lo que crece hasta las direcciones del chipset ($A0_0000-$FF_FFFF) no se da nunca: un programa que escribe en los
/// registros de la CIA o de Paula escribe en memoria reservada, no en la de otro.
#[test]
fn el_hueco_del_chipset_no_se_da() {
    let (h0, h1) = caja_exec::HUECO_DEL_CHIPSET;
    let (mut e, _t) = caja(64 << 20);
    let mut dados = Vec::new();
    for _ in 0..40 {
        let a = e.alloc_mem(1 << 20, 0).unwrap();
        if a == 0 {
            break;
        }
        dados.push(a);
        assert!(a + (1 << 20) <= h0 || a >= h1, "un bloque en el hueco del chipset: ${a:x}");
    }
    assert!(dados.iter().any(|&a| a >= h1), "la memoria pasa del hueco");
    // AllocAbs dentro del hueco: no; y lo de allí se puede escribir sin que lo note nadie (memoria de nadie).
    assert_eq!(e.alloc_abs(16, 0xBF_D400).unwrap(), 0);
    // No es del sistema (0) ni de la tarea: no cuenta en lo que se mira tras cada programa.
    assert!(e.bloques_de(0).iter().all(|&(d, _)| !(0xA0_0000..0x100_0000).contains(&d)));
    e.mem.escribir8(0xBF_D400, 0x12).unwrap();
    e.mem.escribir8(0xDF_F096, 0x80).unwrap();
    for &a in &dados {
        e.free_mem(a, 1 << 20).unwrap();
    }
    // Un bloque grande sigue cabiendo (por encima del hueco).
    let b = e.alloc_mem(20 << 20, 0).unwrap();
    assert!(b >= h1, "${b:x}");
    assert!(e.tomar_avisos().is_empty());
}

/// Los registros de la CIA tienen que existir antes de que un programa escriba en ellos: `cubrir_hasta` hace crecer la
/// memoria hasta ahí (sin pasar del hueco), reservado para el sistema; y un recurso nativo se encuentra con
/// `OpenResource` (en su lista) y tiene sus funciones desde -6.
#[test]
fn cubrir_hasta_la_cia_y_recurso_nativo() {
    let (mut e, _t) = caja(64 << 20);
    assert!(e.cubrir_hasta(0xC0_0000).unwrap());
    let visible = e.mem.visible;
    assert!(visible >= 0xC0_0000 && visible <= 0x100_0000, "{visible:x}");
    e.mem.escribir8(0xBF_D400, 0x37).unwrap();
    assert_eq!(e.alloc_abs(8, 0xBF_D400).unwrap(), 0, "reservado");
    // Más allá del tope: no.
    assert!(!e.cubrir_hasta(0x7000_0000).unwrap());
    let r = e.crear_recurso_nativo("ciab.resource", 40, &[0x1400, 0x1401, 0x1402, 0x1403]).unwrap();
    assert_eq!(e.mem.leer16(r - 6).unwrap(), 0xA0FF);
    assert_eq!(e.mem.leer16(r - 4).unwrap(), 0x1400);
    assert_eq!(e.mem.leer16(r - 22).unwrap(), 0x1403);
    assert_eq!(e.ajena(0x1401), Some((r, -12)));
    // Hostil: un número de exec, o repetido: no.
    assert!(e.crear_recurso_nativo("x.resource", 1, &[5]).is_err());
    assert!(e.crear_recurso_nativo("x.resource", 1, &[0x1400]).is_err());
    assert!(e.crear_recurso_nativo("x.resource", 1, &[]).is_err());
}
