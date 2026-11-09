extern crate std;

use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::tablas::{wl_display, wl_registry, wl_shm, wl_surface, xdg_toplevel};

fn args<'a>(i: &Interface, peticion: bool, orden: u16, msg: &'a [u8], llaves: &[u32]) -> Vec<Valor<'a>> {
    let (_, o, tam) = leer_cabecera(msg).unwrap();
    assert_eq!((o, tam), (orden, msg.len()));
    let m = if peticion { &i.requests[orden as usize] } else { &i.events[orden as usize] };
    leer_args(m, &msg[8..], &mut llaves.iter().copied()).unwrap()
}

#[test]
fn get_registry_es_el_de_siempre() {
    // wl_display.get_registry(2): lo primero que manda cualquier cliente, byte a byte.
    let mut e = Escritor::new();
    e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
    assert_eq!(e.bytes, [1, 0, 0, 0, 1, 0, 12, 0, 2, 0, 0, 0]);
    assert_eq!(args(&tablas::WL_DISPLAY, true, 1, &e.bytes, &[]), [Valor::NewId(2)]);
}

#[test]
fn global_y_bind() {
    let mut e = Escritor::new();
    e.empezar(2, wl_registry::ev::GLOBAL).uint(1).texto(Some("wl_compositor")).uint(6).fin();
    // 8 de cabecera, 4 del nombre, 4 + 14 rellenado a 16 del texto, 4 de la versión.
    assert_eq!(e.bytes.len(), 36);
    assert_eq!(&e.bytes[16..30], b"wl_compositor\0");
    assert_eq!(
        args(&tablas::WL_REGISTRY, false, 0, &e.bytes, &[]),
        [Valor::Uint(1), Valor::Str(Some("wl_compositor")), Valor::Uint(6)]
    );
    // bind lleva un objeto nuevo sin interfaz en la firma: va con su interfaz y su versión.
    e.vaciar();
    e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
    assert_eq!(
        args(&tablas::WL_REGISTRY, true, 0, &e.bytes, &[]),
        [Valor::Uint(1), Valor::NewIdDe { interface: "wl_compositor", version: 6, id: 3 }]
    );
}

#[test]
fn llaves_y_varios_mensajes_seguidos() {
    // wl_shm.create_pool(id, fd, size): el descriptor no ocupa bytes, va como llave.
    let mut e = Escritor::new();
    e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(77).int(4096).fin();
    e.empezar(6, wl_surface::req::COMMIT).fin();
    assert_eq!(e.llaves, [77]);
    let (o1, _, t1) = leer_cabecera(&e.bytes).unwrap();
    assert_eq!((o1, t1), (4, 16));
    let mut llaves = e.llaves.iter().copied();
    let a = leer_args(&tablas::WL_SHM.requests[0], &e.bytes[8..t1], &mut llaves).unwrap();
    assert_eq!(a, [Valor::NewId(5), Valor::Fd(77), Valor::Int(4096)]);
    let resto = &e.bytes[t1..];
    assert_eq!(leer_cabecera(resto).unwrap(), (6, wl_surface::req::COMMIT, 8));
}

#[test]
fn textos_arrays_y_fixed() {
    let mut e = Escritor::new();
    e.empezar(9, xdg_toplevel::req::SET_TITLE).texto(Some("Ventana de DashOS ñ")).fin();
    assert_eq!(args(&tablas::XDG_TOPLEVEL, true, 2, &e.bytes, &[]), [Valor::Str(Some("Ventana de DashOS ñ"))]);
    // configure(ancho, alto, estados): un array de u32.
    e.vaciar();
    let estados: Vec<u8> = [1u32, 4].iter().flat_map(|s| s.to_le_bytes()).collect();
    e.empezar(9, xdg_toplevel::ev::CONFIGURE).int(640).int(480).array(&estados).fin();
    assert_eq!(
        args(&tablas::XDG_TOPLEVEL, false, 0, &e.bytes, &[]),
        [Valor::Int(640), Valor::Int(480), Valor::Array(&estados)]
    );
    e.vaciar();
    e.empezar(9, 0).fixed(-3).fin();
    assert_eq!(&e.bytes[8..], &(-768i32).to_le_bytes());
}

#[test]
fn mensajes_malos() {
    let msg = |v: &[u32]| -> Vec<u8> { v.iter().flat_map(|w| w.to_le_bytes()).collect() };
    assert_eq!(leer_cabecera(&[1, 0, 0]), Err(Fallo::Corto));
    assert_eq!(leer_cabecera(&msg(&[1, 6 << 16])), Err(Fallo::Tamano));
    assert_eq!(leer_cabecera(&msg(&[1, 10 << 16, 0])), Err(Fallo::Tamano));
    assert_eq!(leer_cabecera(&msg(&[1, 16 << 16])), Err(Fallo::Corto));
    let m = &tablas::XDG_TOPLEVEL.requests[2];
    // Texto sin su 0, o nulo sin permitirlo.
    assert_eq!(leer_args(m, &msg(&[4, u32::from_le_bytes(*b"hola")]), &mut [].into_iter()), Err(Fallo::Texto));
    assert_eq!(leer_args(m, &msg(&[0]), &mut [].into_iter()), Err(Fallo::Texto));
    // Un descriptor sin llave.
    let pool = &tablas::WL_SHM.requests[0];
    assert_eq!(leer_args(pool, &msg(&[5, 4096]), &mut [].into_iter()), Err(Fallo::SinLlave));
    // Bytes de más.
    let commit = &tablas::WL_SURFACE.requests[6];
    assert_eq!(leer_args(commit, &msg(&[1]), &mut [].into_iter()), Err(Fallo::Tamano));
    // Un objeto nuevo con el número 0.
    let reg = &tablas::WL_DISPLAY.requests[1];
    assert_eq!(leer_args(reg, &msg(&[0]), &mut [].into_iter()), Err(Fallo::Nulo));
}

#[test]
fn tablas_como_en_el_xml() {
    assert_eq!(interfaz("wl_surface").unwrap().version, 7);
    assert_eq!(interfaz("xdg_wm_base").unwrap().version, 7);
    assert!(interfaz("wl_shell").is_none());
    let attach = &tablas::WL_SURFACE.requests[wl_surface::req::ATTACH as usize];
    assert_eq!(attach.name, "attach");
    let kinds: Vec<Kind> = attach.args.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, vec![Kind::Object, Kind::Int, Kind::Int]);
    assert!(attach.args[0].nullable);
    assert!(tablas::WL_SURFACE.requests[0].destructor);
}

mod transporte {
    use super::*;
    use crate::canal::*;
    use crate::tablas::xdg_toplevel;

    /// Manda por trozos y junta al otro lado: salen los mismos mensajes, cada uno con sus llaves.
    fn ida_y_vuelta(e: &Escritor) -> (Vec<Trozo>, Vec<(Recibido, Vec<u32>)>) {
        let trozos = trocear(e).unwrap();
        let mut j = Juntador::new();
        let mut out = Vec::new();
        let mut cuentas = e.cuentas.iter();
        for t in &trozos {
            assert!(t.bytes.len() <= MAX_CANAL && t.llaves.len() <= MAX_LLAVES);
            assert_eq!(t.bytes[0], TROZO_DATOS);
            j.meter(&t.bytes[1..], &t.llaves).unwrap();
            while let Some(m) = j.siguiente().unwrap() {
                // Las llaves de un mensaje ya están cuando está entero.
                let n = *cuentas.next().unwrap();
                let suyas: Vec<u32> = (0..n).map(|_| j.tomar_llave().expect("llave tarde")).collect();
                out.push((m, suyas));
            }
        }
        assert!(cuentas.next().is_none() && j.llaves.is_empty());
        (trozos, out)
    }

    #[test]
    fn mensajes_pequenos_juntos() {
        let mut e = Escritor::new();
        for i in 0..40 {
            e.empezar(10 + i, wl_surface::req::COMMIT).fin();
        }
        let (trozos, out) = ida_y_vuelta(&e);
        // 40 mensajes de 8 bytes: 320 bytes, dos trozos (31 enteros caben en el primero).
        assert_eq!(trozos.len(), 2);
        assert_eq!(out.len(), 40);
        assert_eq!(out[39].0, Recibido { objeto: 49, orden: 6, cuerpo: Vec::new() });
    }

    #[test]
    fn un_mensaje_largo_partido() {
        let titulo: alloc::string::String = core::iter::repeat_n('ñ', 400).collect();
        let mut e = Escritor::new();
        e.empezar(1, 0).fin();
        e.empezar(9, xdg_toplevel::req::SET_TITLE).texto(Some(&titulo)).fin();
        e.empezar(2, 0).fin();
        let (trozos, out) = ida_y_vuelta(&e);
        assert!(trozos.len() >= 4);
        assert_eq!(out.len(), 3);
        let a = leer_args(&tablas::XDG_TOPLEVEL.requests[2], &out[1].0.cuerpo, &mut [].into_iter()).unwrap();
        assert_eq!(a, [Valor::Str(Some(&titulo))]);
    }

    #[test]
    fn muchas_llaves() {
        // 20 create_pool de 16 bytes caben en un trozo por bytes, pero no por llaves: 8 por trozo.
        let mut e = Escritor::new();
        for i in 0..20 {
            e.empezar(4, wl_shm::req::CREATE_POOL).uint(100 + i).llave(500 + i).int(4096).fin();
        }
        let (trozos, out) = ida_y_vuelta(&e);
        assert_eq!(trozos.len(), 3);
        for (i, (m, llaves)) in out.iter().enumerate() {
            let a = leer_args(&tablas::WL_SHM.requests[0], &m.cuerpo, &mut llaves.iter().copied()).unwrap();
            assert_eq!(a[1], Valor::Fd(500 + i as u32));
        }
    }

    #[test]
    fn juntador_rechaza_basura() {
        let mut j = Juntador::new();
        j.meter(&[1, 0, 0, 0, 6, 0, 0, 0], &[]).unwrap();
        assert_eq!(j.siguiente(), Err(Fallo::Tamano));
        let mut j = Juntador::new();
        assert_eq!(j.meter(&alloc::vec![0; MAX_PENDIENTE + 1], &[]), Err(Fallo::Tamano));
        // Medio mensaje: espera, no falla.
        let mut j = Juntador::new();
        j.meter(&[1, 0, 0, 0, 0, 0, 16, 0, 1, 2], &[]).unwrap();
        assert_eq!(j.siguiente(), Ok(None));
    }
}
