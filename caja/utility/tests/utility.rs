//! utility.library nativa: cuentas, etiquetas, textos y fechas, con casos hostiles.

use caja_exec::estructuras::memf;
use caja_exec::{Accion, Config, Exec, Memoria, MemoriaVec, Registros};
use caja_utility::{Utilidad, clockdata, entrada, tag};

const TAMANO: u32 = 4 << 20;
const PRIMERO: u16 = 0x800;

fn caja() -> (Exec<MemoriaVec>, Utilidad) {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).unwrap();
    let t = e.crear_tarea("principal", 0, 4096, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(t));
    let u = Utilidad::instalar(&mut e, PRIMERO).unwrap();
    (e, u)
}

fn llamar(e: &mut Exec<MemoriaVec>, u: &mut Utilidad, f: &str, d: [u32; 3], a: [u32; 3]) -> (u32, u32) {
    let mut r = Registros::default();
    r.d[..3].copy_from_slice(&d);
    r.a[..3].copy_from_slice(&a);
    let n = PRIMERO + entrada(f).expect("función") - 5;
    assert_eq!(u.llamar(e, n, &mut r), Accion::Seguir);
    (r.d[0], r.d[1])
}

fn etiquetas(e: &mut Exec<MemoriaVec>, pares: &[(u32, u32)]) -> u32 {
    let d = e.alloc_mem(pares.len() as u32 * 8 + 8, memf::CLEAR).unwrap();
    for (k, &(t, v)) in pares.iter().enumerate() {
        e.mem.escribir32(d + k as u32 * 8, t).unwrap();
        e.mem.escribir32(d + k as u32 * 8 + 4, v).unwrap();
    }
    d
}

fn cadena(e: &mut Exec<MemoriaVec>, s: &[u8]) -> u32 {
    let d = e.alloc_mem(s.len() as u32 + 1, memf::CLEAR).unwrap();
    e.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s);
    d
}

#[test]
fn se_abre_con_open_library() {
    let (mut e, u) = caja();
    let nombre = cadena(&mut e, b"utility.library");
    let mut r = Registros::default();
    r.a[1] = nombre;
    r.d[0] = 37;
    let n = caja_exec::lvo::por_nombre("OpenLibrary").unwrap();
    // OpenLibrary de una nativa pide su Open (nativo): se sigue como en la caja.
    let mut a = e.llamar(n, &mut r);
    for _ in 0..4 {
        match a {
            Accion::Llamar { destino: caja_exec::Destino::Nativa(m), regs, tras } => {
                let mut rr = regs;
                e.llamar(m, &mut rr);
                a = e.continuar(tras, &rr, &mut r);
            }
            _ => break,
        }
    }
    assert_eq!(r.d[0], u.base());
    // La entrada de SMult32 (-138) es una llamada nativa al número de utility.
    let ent = e.entrada(u.base() - 138).unwrap();
    assert_eq!(ent, caja_exec::Entrada::Nativa(PRIMERO + 18));
}

#[test]
fn cuentas_de_32_y_64_bits() {
    let (mut e, mut u) = caja();
    assert_eq!(llamar(&mut e, &mut u, "SMult32", [-7i32 as u32, 6, 0], [0; 3]).0, -42i32 as u32);
    assert_eq!(llamar(&mut e, &mut u, "UMult32", [0x10000, 0x10001, 0], [0; 3]).0, 0x10000);
    assert_eq!(llamar(&mut e, &mut u, "SDivMod32", [-17i32 as u32, 5, 0], [0; 3]), (-3i32 as u32, -2i32 as u32));
    assert_eq!(llamar(&mut e, &mut u, "UDivMod32", [100000, 7, 0], [0; 3]), (14285, 5));
    assert_eq!(llamar(&mut e, &mut u, "SMult64", [-2i32 as u32, 0x8000_0000, 0], [0; 3]), (1, 0));
    assert_eq!(llamar(&mut e, &mut u, "UMult64", [0xFFFF_FFFF, 0xFFFF_FFFF, 0], [0; 3]), (0xFFFF_FFFE, 1));
    // El caso de desborde de la división con signo: no hay pánico.
    assert_eq!(llamar(&mut e, &mut u, "SDivMod32", [0x8000_0000, u32::MAX, 0], [0; 3]).0, 0x8000_0000);
    assert!(u.avisos().is_empty());
    // Entre cero: aviso y 0.
    assert_eq!(llamar(&mut e, &mut u, "UDivMod32", [5, 0, 0], [0; 3]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "SDivMod32", [5, 0, 0], [0; 3]).0, 0);
    assert_eq!(u.avisos().len(), 2);
}

#[test]
fn etiquetas_con_more_skip_e_ignore() {
    let (mut e, mut u) = caja();
    let segunda = etiquetas(&mut e, &[(0x8000_0003, 33), (tag::DONE, 0)]);
    let primera = etiquetas(
        &mut e,
        &[
            (0x8000_0001, 11),
            (tag::IGNORE, 0),
            (tag::SKIP, 1),
            (0x8000_0009, 99),
            (0x8000_0002, 22),
            (tag::MORE, segunda),
        ],
    );
    assert_eq!(llamar(&mut e, &mut u, "GetTagData", [0x8000_0003, 7, 0], [primera, 0, 0]).0, 33);
    assert_eq!(llamar(&mut e, &mut u, "GetTagData", [0x8000_0009, 7, 0], [primera, 0, 0]).0, 7, "saltada");
    assert_eq!(llamar(&mut e, &mut u, "GetTagData", [0x8000_0004, 7, 0], [primera, 0, 0]).0, 7);
    assert_eq!(llamar(&mut e, &mut u, "FindTagItem", [0x8000_0002, 0, 0], [primera, 0, 0]).0, primera + 32);
    // NextTagItem, por un puntero a la lista.
    let p = e.alloc_mem(4, memf::CLEAR).unwrap();
    e.mem.escribir32(p, primera).unwrap();
    let mut vistas = vec![];
    loop {
        let t = llamar(&mut e, &mut u, "NextTagItem", [0; 3], [p, 0, 0]).0;
        if t == 0 {
            break;
        }
        vistas.push(e.mem.leer32(t).unwrap());
    }
    assert_eq!(vistas, vec![0x8000_0001, 0x8000_0002, 0x8000_0003]);
    // TagInArray, FilterTagItems, PackBoolTags, ApplyTagChanges.
    let tabla = e.alloc_mem(12, memf::CLEAR).unwrap();
    e.mem.escribir32(tabla, 0x8000_0002).unwrap();
    e.mem.escribir32(tabla + 4, 0x8000_0003).unwrap();
    assert_eq!(llamar(&mut e, &mut u, "TagInArray", [0x8000_0003, 0, 0], [tabla, 0, 0]).0, 1);
    assert_eq!(llamar(&mut e, &mut u, "TagInArray", [0x8000_0001, 0, 0], [tabla, 0, 0]).0, 0);
    let mapa = etiquetas(&mut e, &[(0x8000_0001, 1), (0x8000_0002, 4), (tag::DONE, 0)]);
    let bools = etiquetas(&mut e, &[(0x8000_0001, 0), (0x8000_0002, 1), (tag::DONE, 0)]);
    assert_eq!(llamar(&mut e, &mut u, "PackBoolTags", [0x11, 0, 0], [bools, mapa, 0]).0, 0x14);
    let cambios = etiquetas(&mut e, &[(0x8000_0003, 333), (0x8000_0005, 5), (tag::DONE, 0)]);
    llamar(&mut e, &mut u, "ApplyTagChanges", [0; 3], [primera, cambios, 0]);
    assert_eq!(llamar(&mut e, &mut u, "GetTagData", [0x8000_0003, 7, 0], [primera, 0, 0]).0, 333);
    assert_eq!(llamar(&mut e, &mut u, "FilterTagItems", [tag::FILTER_AND, 0, 0], [primera, tabla, 0]).0, 2);
    assert_eq!(llamar(&mut e, &mut u, "GetTagData", [0x8000_0001, 7, 0], [primera, 0, 0]).0, 7, "filtrada");
    assert!(u.avisos().is_empty(), "{:?}", u.avisos());
}

#[test]
fn clonar_y_soltar_etiquetas() {
    let (mut e, mut u) = caja();
    let libre = e.avail_mem(0).unwrap();
    let l = etiquetas(&mut e, &[(0x8000_0001, 1), (tag::IGNORE, 0), (0x8000_0002, 2), (tag::DONE, 0)]);
    let c = llamar(&mut e, &mut u, "CloneTagItems", [0; 3], [l, 0, 0]).0;
    assert_ne!(c, 0);
    assert_eq!(
        (e.mem.leer32(c).unwrap(), e.mem.leer32(c + 8).unwrap(), e.mem.leer32(c + 16).unwrap()),
        (0x8000_0001, 0x8000_0002, 0)
    );
    e.mem.escribir32(l + 4, 100).unwrap();
    llamar(&mut e, &mut u, "RefreshTagItemClones", [0; 3], [c, l, 0]);
    assert_eq!(e.mem.leer32(c + 4).unwrap(), 100);
    let v = llamar(&mut e, &mut u, "AllocateTagItems", [10, 0, 0], [0; 3]).0;
    assert!(e.mem.leer(v, 80).unwrap().iter().all(|&b| b == 0));
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [c, 0, 0]);
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [v, 0, 0]);
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [0, 0, 0]);
    assert!(u.avisos().is_empty(), "{:?}", u.avisos());
    e.free_mem(l, 40).unwrap();
    assert_eq!(e.avail_mem(0).unwrap(), libre);
    assert_eq!(u.listas_vivas(), 0);
    // Una lista vacía (NULL) da un clon con solo TAG_DONE.
    let c = llamar(&mut e, &mut u, "CloneTagItems", [0; 3], [0, 0, 0]).0;
    assert_eq!(e.mem.leer32(c).unwrap(), tag::DONE);
    // Si la tarea muere, exec suelta su memoria y utility la olvida.
    let t = e.tarea_actual();
    e.matar_tarea(t).unwrap();
    u.tarea_muerta(&e);
    assert_eq!(u.listas_vivas(), 0);
}

#[test]
fn textos_sin_mayusculas() {
    let (mut e, mut u) = caja();
    let a = cadena(&mut e, b"Hola Amiga \xe9");
    let b = cadena(&mut e, b"HOLA amiga \xc9");
    let c = cadena(&mut e, b"HOLA amigo");
    assert_eq!(llamar(&mut e, &mut u, "Stricmp", [0; 3], [a, b, 0]).0, 0);
    assert!((llamar(&mut e, &mut u, "Stricmp", [0; 3], [a, c, 0]).0 as i32) < 0);
    assert_eq!(llamar(&mut e, &mut u, "Strnicmp", [9, 0, 0], [a, c, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "ToUpper", [b'z' as u32, 0, 0], [0; 3]).0, b'Z' as u32);
    assert_eq!(llamar(&mut e, &mut u, "ToUpper", [0xF1, 0, 0], [0; 3]).0, 0xD1, "ñ");
    assert_eq!(llamar(&mut e, &mut u, "ToUpper", [0xF7, 0, 0], [0; 3]).0, 0xF7, "÷");
    assert_eq!(llamar(&mut e, &mut u, "ToLower", [0xC0, 0, 0], [0; 3]).0, 0xE0);
    assert_eq!(llamar(&mut e, &mut u, "ToLower", [b'5' as u32, 0, 0], [0; 3]).0, b'5' as u32);
}

#[test]
fn fechas() {
    let (mut e, mut u) = caja();
    let cd = e.alloc_mem(clockdata::TAMANO, memf::CLEAR).unwrap();
    let leer = |e: &Exec<MemoriaVec>| -> Vec<u16> { (0..7).map(|k| e.mem.leer16(cd + 2 * k).unwrap()).collect() };
    llamar(&mut e, &mut u, "Amiga2Date", [0, 0, 0], [cd, 0, 0]);
    assert_eq!(leer(&e), vec![0, 0, 0, 1, 1, 1978, 0], "domingo, 1 de enero de 1978");
    // 12 de noviembre de 2026, 22:45:30 (un jueves).
    let s = caja_utility::fecha_a_amiga([30, 45, 22, 12, 11, 2026]).unwrap();
    llamar(&mut e, &mut u, "Amiga2Date", [s, 0, 0], [cd, 0, 0]);
    assert_eq!(leer(&e), vec![30, 45, 22, 12, 11, 2026, 4]);
    assert_eq!(llamar(&mut e, &mut u, "Date2Amiga", [0; 3], [cd, 0, 0]).0, s);
    assert_eq!(llamar(&mut e, &mut u, "CheckDate", [0; 3], [cd, 0, 0]).0, s);
    // 29 de febrero: en 2024 sí, en 2100 no.
    for (a, bien) in [(2024u16, true), (2100, false), (2000, true)] {
        e.mem.escribir16(cd + clockdata::MDAY, 29).unwrap();
        e.mem.escribir16(cd + clockdata::MONTH, 2).unwrap();
        e.mem.escribir16(cd + clockdata::YEAR, a).unwrap();
        assert_eq!(llamar(&mut e, &mut u, "CheckDate", [0; 3], [cd, 0, 0]).0 != 0, bien, "{}", a);
    }
    // Ida y vuelta en muchas fechas.
    for s in (0..u32::MAX / 2).step_by(9_876_543) {
        let f = caja_utility::amiga_a_fecha(s);
        assert_eq!(caja_utility::fecha_a_amiga([f[0], f[1], f[2], f[3], f[4], f[5]]), Some(s));
    }
    let _ = &mut u;
}

#[test]
fn hostil() {
    let (mut e, mut u) = caja();
    // Una lista en círculo (TAG_MORE a sí misma) y una que se sale de la memoria.
    let circulo = etiquetas(&mut e, &[(tag::MORE, 0)]);
    e.mem.escribir32(circulo + 4, circulo).unwrap();
    let fuera = etiquetas(&mut e, &[(tag::SKIP, 0x2000_0000)]);
    let p = e.alloc_mem(4, memf::CLEAR).unwrap();
    let malos = [0u32, 4, 8, 0xFFF, TAMANO - 4, TAMANO + 64, 0xFFFF_FFF8, u32::MAX, circulo, fuera];
    let mut n_llamadas = 0;
    for &m in &malos {
        e.mem.escribir32(p, m).unwrap();
        for f in caja_utility::FUNCIONES {
            for d in [0, 1, 0x7FFF_FFFF, u32::MAX, m] {
                let mut r = Registros::default();
                r.d = [d, d, d, 0, 0, 0, 0, 0];
                r.a = [m, m, m, 0, 0, 0, 0, 0];
                if f == "NextTagItem" {
                    r.a[0] = p;
                }
                let n = PRIMERO + entrada(f).unwrap() - 5;
                assert_eq!(u.llamar(&mut e, n, &mut r), Accion::Seguir);
                n_llamadas += 1;
            }
        }
    }
    assert!(n_llamadas > 1000);
    assert!(u.avisos().len() > 100);
    // FreeTagItems de lo que no es suyo, dos veces o a medias: aviso, nada se suelta.
    let mio = e.alloc_vec(64, memf::CLEAR).unwrap();
    let libre = e.avail_mem(0).unwrap();
    u.tomar_avisos();
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [mio, 0, 0]);
    let c = llamar(&mut e, &mut u, "AllocateTagItems", [4, 0, 0], [0; 3]).0;
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [c + 8, 0, 0]);
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [c, 0, 0]);
    llamar(&mut e, &mut u, "FreeTagItems", [0; 3], [c, 0, 0]);
    assert_eq!(u.avisos().len(), 3);
    assert_eq!(e.avail_mem(0).unwrap(), libre);
    // Tamaños absurdos.
    assert_eq!(llamar(&mut e, &mut u, "AllocateTagItems", [u32::MAX, 0, 0], [0; 3]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "AllocateTagItems", [0, 0, 0], [0; 3]).0, 0);
    // Un número que no es de utility.
    let mut r = Registros::default();
    assert_eq!(u.llamar(&mut e, PRIMERO - 1, &mut r), Accion::Ajena);
    assert_eq!(u.llamar(&mut e, PRIMERO + 41, &mut r), Accion::Ajena);
}
