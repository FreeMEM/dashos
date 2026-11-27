//! Memoria: AllocMem y compañía, con casos hostiles.

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Fallo, Memoria, Registros};
use comun::*;

fn libre(e: &caja_exec::Exec<caja_exec::MemoriaVec>) -> u32 {
    e.avail_mem(memf::ANY).unwrap()
}

#[test]
fn alloc_mem_y_free_mem() {
    let mut e = caja();
    let antes = libre(&e);
    let a = e.alloc_mem(100, memf::ANY).unwrap();
    assert_ne!(a, 0);
    assert_eq!(a % 8, 0);
    assert_eq!(libre(&e), antes - 104, "se redondea a 8");
    // Por defecto, de la fast (más prioridad).
    assert_eq!(e.type_of_mem(a).unwrap(), memf::PUBLIC | memf::FAST);
    let b = e.alloc_mem(8, memf::CHIP).unwrap();
    assert_eq!(e.type_of_mem(b).unwrap() & memf::CHIP, memf::CHIP);
    assert!(b < 0x20_0000);
    e.free_mem(a, 100).unwrap();
    e.free_mem(b, 8).unwrap();
    assert_eq!(libre(&e), antes);
    // Al juntarse los trozos, vuelve a haber un solo trozo libre por región.
    for mh in e.nodos(e.base() + execbase::MEMLIST).unwrap() {
        let primero = l32(&e, mh + memheader::MH_FIRST);
        assert_eq!(l32(&e, primero + memchunk::MC_NEXT), 0);
    }
}

#[test]
fn memf_clear_y_dueno() {
    let (mut e, t) = caja_con_tarea();
    let a = e.alloc_mem(64, 0).unwrap();
    e.mem.leer_mut(a, 64).unwrap().fill(0xAA);
    e.free_mem(a, 64).unwrap();
    let b = e.alloc_mem(64, memf::CLEAR).unwrap();
    assert_eq!(b, a, "primer hueco que cabe");
    assert!(e.mem.leer(b, 64).unwrap().iter().all(|&x| x == 0));
    assert_eq!(e.bloque(b).unwrap().dueno, t);
    assert!(e.bloques_de(t).iter().any(|(d, _)| *d == b));
}

#[test]
fn memf_reverse_da_lo_de_arriba() {
    let mut e = caja();
    let a = e.alloc_mem(16, memf::FAST | memf::REVERSE).unwrap();
    assert_eq!(a, TAMANO - 16);
    e.free_mem(a, 16).unwrap();
}

#[test]
fn trozos_que_se_juntan_en_cualquier_orden() {
    let mut e = caja();
    let antes = libre(&e);
    let v: Vec<u32> = (0..6).map(|_| e.alloc_mem(24, 0).unwrap()).collect();
    for i in [1, 3, 5, 0, 4, 2] {
        e.free_mem(v[i], 24).unwrap();
    }
    assert_eq!(libre(&e), antes);
}

#[test]
fn sin_sitio_da_cero() {
    let mut e = caja();
    assert_eq!(e.alloc_mem(64 << 20, 0).unwrap(), 0);
    assert_eq!(e.alloc_mem(0, 0).unwrap(), 0);
    assert_eq!(e.alloc_mem(u32::MAX, 0).unwrap(), 0);
    assert_eq!(e.alloc_mem(16, memf::KICK).unwrap(), 0, "ninguna región con esos atributos");
    assert!(e.avisos().is_empty());
}

#[test]
fn avail_mem_largest_y_total() {
    let mut e = caja();
    let total = e.avail_mem(memf::TOTAL).unwrap();
    assert!(total >= libre(&e));
    let mayor = e.avail_mem(memf::LARGEST | memf::FAST).unwrap();
    assert_eq!(mayor, e.avail_mem(memf::FAST).unwrap());
    // Un hueco en medio parte la fast en dos.
    let a = e.alloc_mem(1 << 20, memf::FAST).unwrap();
    let _b = e.alloc_mem(8, memf::FAST).unwrap();
    e.free_mem(a, 1 << 20).unwrap();
    assert!(e.avail_mem(memf::LARGEST | memf::FAST).unwrap() < e.avail_mem(memf::FAST).unwrap());
}

#[test]
fn alloc_vec_guarda_el_tamano() {
    let mut e = caja();
    let antes = libre(&e);
    let v = e.alloc_vec(100, memf::CLEAR).unwrap();
    assert_eq!(v % 8, 4, "AllocVec da memoria alineada a 4, como en AmigaOS");
    assert_eq!(l32(&e, v - 4), 104);
    e.free_vec(v).unwrap();
    assert_eq!(libre(&e), antes);
    e.free_vec(0).unwrap();
}

#[test]
fn alloc_abs() {
    let mut e = caja();
    let antes = libre(&e);
    let d = e.alloc_abs(100, 0x30_0003).unwrap();
    assert_eq!(d, 0x30_0000);
    assert_eq!(libre(&e), antes - 104);
    assert_eq!(e.alloc_abs(8, 0x30_0010).unwrap(), 0, "ya está cogida");
    assert!(e.alloc_mem(8, memf::FAST).unwrap() < 0x30_0000);
    assert_eq!(e.alloc_abs(8, 0x8000_0000).unwrap(), 0, "fuera de las regiones");
    e.free_mem(d, 104).unwrap();
}

#[test]
fn allocate_y_deallocate_en_un_memheader_propio() {
    let mut e = caja();
    // Un programa monta su propio MemHeader en un bloque suyo.
    let zona = e.alloc_mem(1024, memf::CLEAR).unwrap();
    let mh = zona;
    let primero = zona + 32;
    e.mem.escribir32(mh + memheader::MH_FIRST, primero).unwrap();
    e.mem.escribir32(mh + memheader::MH_LOWER, primero).unwrap();
    e.mem.escribir32(mh + memheader::MH_UPPER, zona + 1024).unwrap();
    e.mem.escribir32(mh + memheader::MH_FREE, 992).unwrap();
    e.mem.escribir32(primero + memchunk::MC_BYTES, 992).unwrap();
    let mut r = Registros::default();
    r.a[0] = mh;
    r.d[0] = 100;
    llamar(&mut e, "Allocate", &mut r);
    assert_eq!(r.d[0], primero);
    assert_eq!(l32(&e, mh + memheader::MH_FREE), 992 - 104);
    r.a[1] = primero;
    r.d[0] = 100;
    llamar(&mut e, "Deallocate", &mut r);
    assert_eq!(l32(&e, mh + memheader::MH_FREE), 992);
    // Liberar dos veces en el MemHeader: se nota.
    assert_eq!(e.deallocate(mh, primero, 100), Err(Fallo::YaLibre { dir: primero }));
    assert_eq!(e.deallocate(mh, zona + 2000, 8), Err(Fallo::FueraDelBloque { dir: zona + 2000 }));
}

#[test]
fn alloc_entry_y_free_entry() {
    let mut e = caja();
    let antes = libre(&e);
    let ml = e.alloc_mem(memlist::ML_ME + 2 * 8, memf::CLEAR).unwrap();
    e.mem.escribir16(ml + memlist::ML_NUMENTRIES, 2).unwrap();
    e.mem.escribir32(ml + 16, memf::CLEAR).unwrap();
    e.mem.escribir32(ml + 20, 200).unwrap();
    e.mem.escribir32(ml + 24, memf::CHIP).unwrap();
    e.mem.escribir32(ml + 28, 50).unwrap();
    let copia = e.alloc_entry(ml).unwrap();
    assert_eq!(copia & 0x8000_0000, 0);
    let a0 = l32(&e, copia + 16);
    let a1 = l32(&e, copia + 24);
    assert_ne!(a0, 0);
    assert!(a1 < 0x20_0000, "la segunda, de chip");
    assert_eq!(l32(&e, copia + 20), 200);
    e.free_entry(copia).unwrap();
    e.free_mem(ml, memlist::ML_ME + 16).unwrap();
    assert_eq!(libre(&e), antes);
    // Una que no cabe: devuelve sus requisitos con el bit 31 y no deja nada reservado.
    let ml = e.alloc_mem(memlist::ML_ME + 2 * 8, memf::CLEAR).unwrap();
    let antes = libre(&e);
    e.mem.escribir16(ml + memlist::ML_NUMENTRIES, 2).unwrap();
    e.mem.escribir32(ml + 20, 100).unwrap();
    e.mem.escribir32(ml + 24, memf::CHIP).unwrap();
    e.mem.escribir32(ml + 28, 100 << 20).unwrap();
    assert_eq!(e.alloc_entry(ml).unwrap(), 0x8000_0000 | memf::CHIP);
    assert_eq!(libre(&e), antes);
}

#[test]
fn copy_mem_tambien_solapado() {
    let mut e = caja();
    let a = e.alloc_mem(64, 0).unwrap();
    for i in 0..16u32 {
        e.mem.escribir8(a + i, i as u8).unwrap();
    }
    e.copy_mem(a, a + 4, 16).unwrap();
    assert_eq!(e.mem.leer(a + 4, 16).unwrap(), (0..16u8).collect::<Vec<_>>().as_slice());
    assert!(e.copy_mem(a, 0x100, 4).is_err(), "a la página 0, no");
    assert!(e.copy_mem(a, TAMANO - 2, 4).is_err());
}

#[test]
fn por_la_tabla_de_llamadas() {
    let (mut e, _) = caja_con_tarea();
    let mut r = Registros::default();
    r.d[0] = 40;
    r.d[1] = memf::CLEAR;
    llamar(&mut e, "AllocMem", &mut r);
    let a = r.d[0];
    assert_ne!(a, 0);
    r.d[1] = memf::ANY;
    llamar(&mut e, "AvailMem", &mut r);
    assert!(r.d[0] > 0);
    r.a[1] = a;
    llamar(&mut e, "TypeOfMem", &mut r);
    assert_eq!(r.d[0], memf::PUBLIC | memf::FAST);
    r.a[1] = a;
    r.d[0] = 40;
    llamar(&mut e, "FreeMem", &mut r);
    r.d[0] = 10;
    r.d[1] = 0;
    llamar(&mut e, "AllocVec", &mut r);
    r.a[1] = r.d[0];
    llamar(&mut e, "FreeVec", &mut r);
    assert!(e.avisos().is_empty(), "{:?}", e.avisos());
}

// --- Hostiles ---

#[test]
fn free_mem_de_lo_no_reservado() {
    let mut e = caja();
    let antes = libre(&e);
    assert_eq!(e.free_mem(0x30_0000, 16), Err(Fallo::NoReservado { dir: 0x30_0000 }));
    assert_eq!(e.free_mem(0x100, 16), Err(Fallo::NoReservado { dir: 0x100 }));
    assert_eq!(e.free_mem(0xFFFF_FFF8, 16), Err(Fallo::NoReservado { dir: 0xFFFF_FFF8 }));
    e.free_mem(0, 16).unwrap();
    assert_eq!(libre(&e), antes);
}

#[test]
fn liberar_dos_veces() {
    let mut e = caja();
    let a = e.alloc_mem(32, 0).unwrap();
    let antes = libre(&e);
    e.free_mem(a, 32).unwrap();
    assert_eq!(e.free_mem(a, 32), Err(Fallo::NoReservado { dir: a }));
    assert_eq!(libre(&e), antes + 32);
    let v = e.alloc_vec(10, 0).unwrap();
    e.free_vec(v).unwrap();
    assert_eq!(e.free_vec(v), Err(Fallo::NoReservado { dir: v }));
}

#[test]
fn free_mem_con_otro_tamano_o_del_medio() {
    let mut e = caja();
    let a = e.alloc_mem(64, 0).unwrap();
    assert_eq!(e.free_mem(a, 128), Err(Fallo::TamanoDistinto { dir: a, reservado: 64, pedido: 128 }));
    assert_eq!(e.free_mem(a + 8, 56), Err(Fallo::NoReservado { dir: a + 8 }));
    // 60 se redondea a 64: vale.
    e.free_mem(a, 60).unwrap();
}

#[test]
fn free_vec_de_alloc_mem() {
    let mut e = caja();
    let a = e.alloc_mem(64, 0).unwrap();
    assert_eq!(e.free_vec(a + 4), Err(Fallo::NoEsDeAllocVec { dir: a + 4 }));
    assert_eq!(e.free_vec(3), Err(Fallo::NoReservado { dir: 3 }));
    e.free_mem(a, 64).unwrap();
}

#[test]
fn memheader_estropeado() {
    let mut e = caja();
    let a = e.alloc_mem(64, memf::FAST).unwrap();
    let mh = e.nodos(e.base() + execbase::MEMLIST).unwrap()[0];
    let primero = l32(&e, mh + memheader::MH_FIRST);
    // Un programa escribe encima de la memoria libre (el MemChunk).
    e.mem.escribir32(primero + memchunk::MC_BYTES, 3).unwrap();
    assert!(matches!(e.alloc_mem(16, memf::FAST), Err(Fallo::ListaRota { .. })));
    // Por la tabla: aviso y D0 = 0.
    let mut r = Registros::default();
    r.d[0] = 16;
    r.d[1] = memf::FAST;
    llamar(&mut e, "AllocMem", &mut r);
    assert_eq!(r.d[0], 0);
    assert_eq!(avisos_de(&e, "AllocMem"), 1);
    // Liberar en esa región también se niega, sin tocar nada.
    assert!(e.free_mem(a, 64).is_err());
}

#[test]
fn alloc_entry_hostil() {
    let mut e = caja();
    assert_eq!(e.alloc_entry(0), Err(Fallo::PunteroNulo));
    assert!(e.alloc_entry(0x10).is_err());
    assert!(e.free_entry(0xFFFF_FFFE).is_err());
    assert!(e.alloc_entry(TAMANO - 4).is_err());
}

/// S28: lo que se suelta queda a cero (FreeMem, FreeVec, FreePooled y la muerte de la tarea), para que el siguiente
/// programa no lea lo que dejó otro en la memoria libre.
#[test]
fn lo_soltado_queda_a_cero() {
    let (mut e, t) = caja_con_tarea();
    let a = e.alloc_mem(256, 0).unwrap();
    e.mem.leer_mut(a, 256).unwrap().fill(0xAA);
    e.free_mem(a, 256).unwrap();
    // Los 8 primeros son el MemChunk (o se juntaron con otro trozo); el resto, a cero.
    assert!(e.mem.leer(a + 8, 248).unwrap().iter().all(|&x| x == 0));
    let v = e.alloc_vec(100, 0).unwrap();
    e.mem.leer_mut(v, 100).unwrap().fill(0xBB);
    e.free_vec(v).unwrap();
    assert!(e.mem.leer(v + 8, 92).unwrap().iter().all(|&x| x == 0));
    // Lo que deja una tarea al morir.
    let b = e.alloc_mem(512, 0).unwrap();
    e.cambiar_dueno(b, t);
    e.mem.leer_mut(b, 512).unwrap().fill(0xCC);
    let _ = e.matar_tarea(t);
    assert!(e.mem.leer(b + 8, 504).unwrap().iter().all(|&x| x == 0));
}
