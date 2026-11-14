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

// --- Objetos con nombre (V39) ---

const ANO_NAMESPACE: u32 = 4000;
const ANO_USERSPACE: u32 = 4001;
const ANO_PRIORITY: u32 = 4002;
const ANO_FLAGS: u32 = 4003;

fn nombre_de(e: &Exec<MemoriaVec>, d: u32) -> Vec<u8> {
    let mut v = Vec::new();
    while let Ok(c) = e.mem.leer8(d + v.len() as u32) {
        if c == 0 {
            break;
        }
        v.push(c);
    }
    v
}

/// Lo que hace datatypes: un espacio propio en el raíz, objetos dentro por prioridad, buscarlos (sin mayúsculas o con
/// ellas), soltarlos, quitarlos y liberarlos; la memoria de usuario.
#[test]
fn objetos_con_nombre() {
    let (mut e, mut u) = caja();
    let n_dt = cadena(&mut e, b"DataTypes");
    let t = etiquetas(&mut e, &[(ANO_NAMESPACE, 1), (ANO_FLAGS, 1)]);
    let dt = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n_dt, t, 0]).0;
    assert_ne!(dt, 0);
    assert_eq!(e.mem.leer32(dt).unwrap(), 0, "sin memoria de usuario, no_Object es 0");
    let nombre = llamar(&mut e, &mut u, "NamedObjectName", [0; 3], [dt, 0, 0]).0;
    assert_eq!(nombre_de(&e, nombre), b"DataTypes");
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [0, dt, 0]).0, 1);
    // Se encuentra en el raíz sin mirar mayúsculas.
    let n_min = cadena(&mut e, b"datatypes");
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [0, n_min, 0]).0, dt);
    // Tres dentro, con prioridades; el espacio no admite repetidos (NSF_NODUPS).
    let mut hijos = Vec::new();
    for (n, p) in [(&b"ilbm"[..], 0u32), (b"png", 5), (b"gif", 0)] {
        let c = cadena(&mut e, n);
        let t = etiquetas(&mut e, &[(ANO_PRIORITY, p), (ANO_USERSPACE, 10)]);
        let o = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [c, t, 0]).0;
        let datos = e.mem.leer32(o).unwrap();
        assert!(datos != 0 && datos % 4 == 0, "memoria de usuario alineada a 4");
        e.mem.escribir32(datos + 6, 0x1234_5678).unwrap();
        assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [dt, o, 0]).0, 1);
        // El uso de quien lo creó: soltado, otros lo pueden quitar.
        llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [o, 0, 0]);
        hijos.push(o);
    }
    let otro_png = {
        let c = cadena(&mut e, b"PNG");
        llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [c, 0, 0]).0
    };
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [dt, otro_png, 0]).0, 0, "NSF_NODUPS sin mayúsculas");
    // Todos en orden: png (5) primero, luego ilbm y gif (en el orden en que entraron).
    let mut orden = Vec::new();
    let mut ultimo = 0;
    loop {
        let o = llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, 0, ultimo]).0;
        if o == 0 {
            break;
        }
        orden.push(o);
        ultimo = o;
    }
    assert_eq!(orden, [hijos[1], hijos[0], hijos[2]]);
    for &o in &orden {
        llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [o, 0, 0]);
    }
    // AttemptRemNamedObject: con un uso más (alguien lo encontró) no; soltado, sí.
    let n_gif = cadena(&mut e, b"GIF");
    let gif = llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_gif, 0]).0;
    assert_eq!(gif, hijos[2]);
    let gif2 = llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_gif, 0]).0;
    assert_eq!(llamar(&mut e, &mut u, "AttemptRemNamedObject", [0; 3], [gif, 0, 0]).0, 0);
    llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [gif2, 0, 0]);
    llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [gif, 0, 0]);
    assert_eq!(llamar(&mut e, &mut u, "AttemptRemNamedObject", [0; 3], [gif, 0, 0]).0, 1);
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_gif, 0]).0, 0);
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [gif, 0, 0]);
    // RemNamedObject con alguien usándolo: se quita al soltarse, y se contesta el mensaje con el objeto en ln_Name.
    let puerto = e.create_msg_port().unwrap();
    let m = e.alloc_mem(20, memf::CLEAR | memf::PUBLIC).unwrap();
    e.mem.escribir32(m + 14, puerto).unwrap();
    let n_ilbm = cadena(&mut e, b"ilbm");
    let ilbm = llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_ilbm, 0]).0;
    let ilbm2 = llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_ilbm, 0]).0;
    llamar(&mut e, &mut u, "RemNamedObject", [0; 3], [ilbm, m, 0]);
    assert_eq!(e.get_msg(puerto).unwrap(), 0, "aún lo usa otro");
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_ilbm, 0]).0, 0, "ya no se encuentra");
    llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [ilbm2, 0, 0]);
    assert_eq!(e.get_msg(puerto).unwrap(), m);
    assert_eq!(e.mem.leer32(m + 10).unwrap(), ilbm);
    // Liberar: uno en un espacio, no; el espacio con hijos, no; vacío, sí.
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [ilbm, 0, 0]);
    assert!(u.tomar_avisos().is_empty(), "{:?}", u.avisos());
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [hijos[1], 0, 0]);
    assert_eq!(u.tomar_avisos().len(), 1, "png sigue en el espacio");
    let n_png = cadena(&mut e, b"png");
    let png = llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [dt, n_png, 0]).0;
    llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [png, 0, 0]);
    assert_eq!(llamar(&mut e, &mut u, "AttemptRemNamedObject", [0; 3], [png, 0, 0]).0, 1);
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [png, 0, 0]);
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [otro_png, 0, 0]);
    llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [dt, 0, 0]);
    assert_eq!(llamar(&mut e, &mut u, "AttemptRemNamedObject", [0; 3], [dt, 0, 0]).0, 1);
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [dt, 0, 0]);
    assert!(u.tomar_avisos().is_empty(), "{:?}", u.avisos());
    // utility ya no conoce ninguno (su memoria se soltó: la vuelve a usar quien la pida).
    for o in [dt, otro_png].into_iter().chain(hijos.clone()) {
        assert_eq!(llamar(&mut e, &mut u, "NamedObjectName", [0; 3], [o, 0, 0]).0, 0);
    }
    assert_eq!(u.tomar_avisos().len(), 5);
    // Y no se pierde memoria: cien vueltas de crear, meter, sacar y soltar dejan los mismos bloques.
    let n = cadena(&mut e, b"vuelta");
    let t = etiquetas(&mut e, &[(ANO_NAMESPACE, 1), (ANO_USERSPACE, 100)]);
    let antes = e.bloques_de(e.tarea_actual()).len();
    for _ in 0..100 {
        let o = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n, t, 0]).0;
        assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [0, o, 0]).0, 1);
        assert_eq!(llamar(&mut e, &mut u, "AttemptRemNamedObject", [0; 3], [o, 0, 0]).0, 1);
        llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [o, 0, 0]);
    }
    assert_eq!(e.bloques_de(e.tarea_actual()).len(), antes);
    assert!(u.tomar_avisos().is_empty(), "{:?}", u.avisos());
}

/// Lo hostil: objetos que utility no dio, nulos, ya soltados; un objeto en dos espacios o en el suyo; nombres sin fin
/// o fuera de la memoria; memoria de usuario absurda; etiquetas en círculo; una tarea que acaba sin soltarlos.
#[test]
fn objetos_con_nombre_hostil() {
    let (mut e, mut u) = caja();
    let falso = e.alloc_mem(64, memf::CLEAR).unwrap();
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [0, falso, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [falso, 0, 0]).0, 0);
    for f in ["ReleaseNamedObject", "FreeNamedObject", "AttemptRemNamedObject", "NamedObjectName"] {
        assert_eq!(llamar(&mut e, &mut u, f, [0; 3], [falso, 0, 0]).0, 0, "{f}");
    }
    assert_eq!(u.tomar_avisos().len(), 6);
    // Nulos: lo que dice el autodoc (0, sin aviso) salvo AllocNamedObjectA sin nombre.
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [0, 0, 0]).0, 0);
    llamar(&mut e, &mut u, "ReleaseNamedObject", [0; 3], [0, 0, 0]);
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [0, 0, 0]);
    assert_eq!(llamar(&mut e, &mut u, "NamedObjectName", [0; 3], [0, 0, 0]).0, 0);
    assert!(u.tomar_avisos().is_empty());
    assert_eq!(llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [0, 0, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [0xFFFF_FFF0, 0, 0]).0, 0);
    // Un nombre sin fin (4 KB de letras sin cero).
    let largo = e.alloc_mem(4096, 0).unwrap();
    e.mem.leer_mut(largo, 4096).unwrap().fill(b'a');
    assert_eq!(llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [largo, 0, 0]).0, 0);
    assert_eq!(u.tomar_avisos().len(), 3);
    // Memoria de usuario absurda: 0, sin aviso (no hay memoria).
    let n = cadena(&mut e, b"x");
    let t = etiquetas(&mut e, &[(ANO_USERSPACE, 0xFFFF_FFF0)]);
    assert_eq!(llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n, t, 0]).0, 0);
    // Etiquetas en círculo: un aviso, 0.
    let t = etiquetas(&mut e, &[(2, 0)]);
    e.mem.escribir32(t + 4, t).unwrap();
    assert_eq!(llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n, t, 0]).0, 0);
    assert_eq!(u.tomar_avisos().len(), 1);
    // Un objeto en dos espacios, o en el suyo propio, o un espacio dentro de su hijo: no.
    let t_ns = etiquetas(&mut e, &[(ANO_NAMESPACE, 1)]);
    let a = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n, t_ns, 0]).0;
    let b = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n, t_ns, 0]).0;
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [a, a, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [a, b, 0]).0, 1);
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [0, b, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [b, a, 0]).0, 0);
    // Uno sin espacio no sirve de espacio.
    let c = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n, 0, 0]).0;
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [c, a, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [c, 0, 0]).0, 0);
    // lastObject de otro espacio: aviso, 0.
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [0, 0, b]).0, 0);
    assert_eq!(u.tomar_avisos().len(), 6);
    // Soltar dos veces: el segundo, ya no es suyo.
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [c, 0, 0]);
    llamar(&mut e, &mut u, "FreeNamedObject", [0; 3], [c, 0, 0]);
    assert_eq!(u.tomar_avisos().len(), 1);
    // Una tarea que acaba con objetos vivos: la caja suelta su memoria y utility se olvida de ellos.
    let otra = e.crear_tarea("otra", 5, 1024, 0x1000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(otra));
    let n2 = cadena(&mut e, b"suyo");
    let suyo = llamar(&mut e, &mut u, "AllocNamedObjectA", [0; 3], [n2, 0, 0]).0;
    assert_eq!(llamar(&mut e, &mut u, "AddNamedObject", [0; 3], [0, suyo, 0]).0, 1);
    e.matar_tarea(otra).unwrap();
    u.tarea_muerta(&e);
    assert_eq!(llamar(&mut e, &mut u, "FindNamedObject", [0; 3], [0, n, 0]).0, 0);
    assert_eq!(llamar(&mut e, &mut u, "NamedObjectName", [0; 3], [suyo, 0, 0]).0, 0);
    assert_eq!(u.tomar_avisos().len(), 1);
}
