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

    /// E2: las llaves que esperan tienen tope, y lo que no cabe no se queda a medias.
    #[test]
    fn juntador_con_tope_de_llaves() {
        let mut j = Juntador::new();
        for i in 0..MAX_LLAVES_PENDIENTES / 8 {
            j.meter(&[0], &[i as u32; 8]).unwrap();
        }
        assert_eq!(j.meter(&[0], &[9]), Err(Fallo::Llaves));
        assert_eq!(j.llaves.len(), MAX_LLAVES_PENDIENTES);
        assert_eq!(j.meter(&[], &[]), Ok(()));
    }
}

mod servidor {
    use super::*;
    use crate::servidor::*;
    use crate::tablas::{wl_callback, wl_compositor, wl_registry, wl_surface};

    /// Un servicio de prueba: apunta lo que le llega.
    #[derive(Default)]
    struct Apuntes {
        peticiones: Vec<(u32, &'static str, u16)>,
        enlazados: Vec<(&'static str, u32)>,
        destruidos: Vec<u32>,
    }

    impl Servicio for Apuntes {
        fn peticion(&mut self, con: &mut Conexion, objeto: u32, orden: u16, _args: &[Valor]) -> Result<(), ErrorProtocolo> {
            self.peticiones.push((objeto, con.interfaz(objeto).unwrap().name, orden));
            Ok(())
        }
        fn enlazado(&mut self, _con: &mut Conexion, g: &Global, id: u32) {
            self.enlazados.push((g.interfaz.name, id));
        }
        fn destruido(&mut self, _con: &mut Conexion, objeto: u32) {
            self.destruidos.push(objeto);
        }
    }

    fn globales() -> [Global; 2] {
        [
            Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
            Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        ]
    }

    fn mandar(con: &mut Conexion, s: &mut Apuntes, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
        con.juntador.meter(&e.bytes, &e.llaves).unwrap();
        con.atender(&globales(), s);
        eventos(&con.sacar())
    }

    #[test]
    fn registro_sync_bind_y_destruir() {
        let (mut con, mut s) = (Conexion::new(), Apuntes::default());
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(1, wl_display::req::SYNC).uint(3).fin();
        let ev = mandar(&mut con, &mut s, &e);
        // Dos globales, el done del sync y su delete_id.
        assert_eq!(ev.len(), 4);
        assert_eq!((ev[0].0, ev[0].1), (2, wl_registry::ev::GLOBAL));
        assert_eq!(leer_args(&tablas::WL_REGISTRY.events[0], &ev[1].2, &mut [].into_iter()).unwrap()[1], Valor::Str(Some("wl_shm")));
        assert_eq!((ev[2].0, ev[2].1), (3, wl_callback::ev::DONE));
        assert_eq!((ev[3].0, ev[3].1, ev[3].2.clone()), (1, wl_display::ev::DELETE_ID, 3u32.to_le_bytes().to_vec()));

        // bind de wl_compositor en la versión 4, y una superficie: tiene la versión 4.
        e.vaciar();
        e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(4).uint(4).fin();
        e.empezar(4, wl_compositor::req::CREATE_SURFACE).uint(5).fin();
        assert!(mandar(&mut con, &mut s, &e).is_empty());
        assert_eq!(s.enlazados, [("wl_compositor", 4)]);
        assert_eq!(s.peticiones, [(4, "wl_compositor", wl_compositor::req::CREATE_SURFACE)]);
        assert_eq!(con.objetos[&5].version, 4);
        assert_eq!(con.objetos[&5].interfaz.name, "wl_surface");

        // destroy: el servicio la ve, se quita y el cliente recibe delete_id.
        e.vaciar();
        e.empezar(5, wl_surface::req::DESTROY).fin();
        let ev = mandar(&mut con, &mut s, &e);
        assert_eq!(s.destruidos, [5]);
        assert!(!con.objetos.contains_key(&5));
        assert_eq!((ev[0].0, ev[0].1), (1, wl_display::ev::DELETE_ID));
        assert!(con.roto.is_none());
    }

    fn error_de(ev: &[(u32, u16, Vec<u8>)]) -> (u32, u32) {
        let (o, orden, cuerpo) = ev.last().unwrap();
        assert_eq!((*o, *orden), (1, wl_display::ev::ERROR));
        let a = leer_args(&tablas::WL_DISPLAY.events[0], cuerpo, &mut [].into_iter()).unwrap();
        (a[0].uint(), a[1].uint())
    }

    #[test]
    fn errores_de_protocolo() {
        let caso = |preparar: &dyn Fn(&mut Escritor)| {
            let (mut con, mut s) = (Conexion::new(), Apuntes::default());
            let mut e = Escritor::new();
            e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
            e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(4).uint(4).fin();
            e.empezar(4, wl_compositor::req::CREATE_SURFACE).uint(5).fin();
            preparar(&mut e);
            let ev = mandar(&mut con, &mut s, &e);
            assert!(con.roto.is_some());
            // Lo que venga después no se atiende.
            let mut otra = Escritor::new();
            otra.empezar(1, wl_display::req::SYNC).uint(50).fin();
            assert!(mandar(&mut con, &mut s, &otra).is_empty());
            error_de(&ev)
        };
        // Un objeto que no existe.
        assert_eq!(caso(&|e| { e.empezar(99, 0).fin(); }), (1, INVALID_OBJECT));
        // Una petición más nueva que el objeto: offset es de la versión 5, la superficie es de la 4.
        assert_eq!(caso(&|e| { e.empezar(5, wl_surface::req::OFFSET).int(0).int(0).fin(); }), (5, INVALID_METHOD));
        // Una orden que no existe.
        assert_eq!(caso(&|e| { e.empezar(5, 40).fin(); }), (5, INVALID_METHOD));
        // bind en una versión más alta que la del global, o de otra interfaz.
        assert_eq!(
            caso(&|e| { e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(7).uint(9).fin(); }),
            (2, INVALID_METHOD)
        );
        assert_eq!(
            caso(&|e| { e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_compositor")).uint(1).uint(9).fin(); }),
            (2, INVALID_METHOD)
        );
        // Un número de objeto nuevo que ya está, o del servidor.
        assert_eq!(caso(&|e| { e.empezar(4, wl_compositor::req::CREATE_SURFACE).uint(5).fin(); }), (1, INVALID_OBJECT));
        assert_eq!(
            caso(&|e| { e.empezar(4, wl_compositor::req::CREATE_SURFACE).uint(PRIMERO_DEL_SERVIDOR).fin(); }),
            (1, INVALID_OBJECT)
        );
        // Argumentos que no cuadran con la firma.
        assert_eq!(caso(&|e| { e.empezar(5, wl_surface::req::COMMIT).uint(1).fin(); }), (1, INVALID_METHOD));
        // E3: más objetos vivos que el tope.
        assert_eq!(
            caso(&|e| {
                for i in 0..MAX_OBJETOS as u32 {
                    e.empezar(4, wl_compositor::req::CREATE_SURFACE).uint(10 + i).fin();
                }
            }),
            (1, NO_MEMORY)
        );
    }
}

mod memoria_compartida {
    use super::*;
    use crate::shm::*;
    use alloc::collections::BTreeMap;

    /// Objetos de memoria de prueba: cada llave, 1 MB; apunta lo que se suelta.
    #[derive(Default)]
    struct Falsas {
        objetos: BTreeMap<u32, Vec<u8>>,
        sueltas: Vec<u32>,
    }

    impl Memorias for Falsas {
        fn proyectar(&mut self, llave: u32, tamano: usize) -> Option<*const u8> {
            let v = self.objetos.entry(llave).or_insert_with(|| vec![0; 1 << 20]);
            (tamano <= v.len() && llave != 666).then(|| v.as_ptr())
        }
        fn soltar(&mut self, llave: u32, _base: *const u8, _tamano: usize) {
            self.sueltas.push(llave);
        }
    }

    #[test]
    fn pool_buferes_y_pixeles() {
        let (mut m, mut shm) = (Falsas::default(), Shm::default());
        shm.crear_pool(&mut m, 3, 10, 70, 64 * 64 * 4).unwrap();
        // El cliente pinta: un píxel rojo en (2, 1) de un búfer de 16x16 que empieza en el byte 4096.
        m.objetos.get_mut(&70).unwrap()[4096 + 64 + 8..4096 + 64 + 12].copy_from_slice(&[0, 0, 255, 255]);
        shm.crear_bufer(10, 11, 4096, 16, 16, 64, ARGB8888).unwrap();
        let (px, b) = shm.pixeles(11).unwrap();
        assert_eq!((b.ancho, b.alto, b.linea), (16, 16, 64));
        assert_eq!(&px[64 + 8..64 + 12], &[0, 0, 255, 255]);
        // Destruir el pool no quita la memoria al búfer; se suelta con él.
        shm.destruir_pool(&mut m, 10);
        assert!(m.sueltas.is_empty() && shm.pixeles(11).is_some());
        shm.destruir_bufer(&mut m, 11);
        assert_eq!(m.sueltas, [70]);
    }

    #[test]
    fn errores() {
        let (mut m, mut shm) = (Falsas::default(), Shm::default());
        let codigo = |r: Result<(), crate::servidor::ErrorProtocolo>| r.unwrap_err().codigo;
        assert_eq!(codigo(shm.crear_pool(&mut m, 3, 10, 70, 0)), INVALID_STRIDE);
        assert_eq!(codigo(shm.crear_pool(&mut m, 3, 10, 666, 4096)), INVALID_FD);
        shm.crear_pool(&mut m, 3, 10, 70, 4096).unwrap();
        // Formato no anunciado; línea menor que el ancho; no cabe; desplazamiento negativo.
        assert_eq!(codigo(shm.crear_bufer(10, 11, 0, 8, 8, 32, 0x3432_5258)), INVALID_FORMAT);
        assert_eq!(codigo(shm.crear_bufer(10, 11, 0, 8, 8, 28, XRGB8888)), INVALID_STRIDE);
        assert_eq!(codigo(shm.crear_bufer(10, 11, 1024, 32, 32, 128, XRGB8888)), INVALID_STRIDE);
        assert_eq!(codigo(shm.crear_bufer(10, 11, -4, 8, 8, 32, XRGB8888)), INVALID_STRIDE);
        // Desbordar con números enormes no cuela.
        assert_eq!(codigo(shm.crear_bufer(10, 11, i32::MAX, 8, 8, 32, XRGB8888)), INVALID_STRIDE);
        // Crecer sí; encoger no.
        shm.crecer_pool(&mut m, 10, 8192).unwrap();
        shm.crear_bufer(10, 11, 4096, 32, 32, 128, XRGB8888).unwrap();
        assert_eq!(codigo(shm.crecer_pool(&mut m, 10, 4096)), INVALID_STRIDE);
        shm.soltar_todo(&mut m);
        assert_eq!(m.sueltas, [70]);
    }
}

mod composicion {
    use super::*;
    use crate::escena::*;
    use crate::servidor::*;
    use crate::shm::{ARGB8888, Memorias, XRGB8888};
    use crate::tablas::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface};
    use alloc::collections::BTreeMap;
    use core::cell::RefCell;
    use std::rc::Rc;

    /// Memoria de prueba compartida con el "cliente", que pinta en ella.
    #[derive(Clone, Default)]
    struct Compartida(Rc<RefCell<BTreeMap<u32, Vec<u8>>>>);

    impl Memorias for Compartida {
        fn proyectar(&mut self, llave: u32, tamano: usize) -> Option<*const u8> {
            let mut m = self.0.borrow_mut();
            let v = m.entry(llave).or_insert_with(|| vec![0; 1 << 16]);
            (tamano <= v.len()).then(|| v.as_ptr())
        }
        fn soltar(&mut self, _llave: u32, _base: *const u8, _tamano: usize) {}
    }

    fn globales() -> [Global; 2] {
        [
            Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
            Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
        ]
    }

    /// Un cliente con una superficie de `w`×`h` de un color, en un búfer de shm.
    fn cliente(escena: &mut Escena<Compartida>, con: &mut Conexion, c: Cliente, llave: u32, w: i32, h: i32, color: u32, formato: u32) {
        {
            let mut m = escena.memorias.0.borrow_mut();
            let v = m.entry(llave).or_insert_with(|| vec![0; ((w * h * 4) as usize).max(1 << 16)]);
            for p in v[..(w * h * 4) as usize].chunks_mut(4) {
                p.copy_from_slice(&color.to_le_bytes());
            }
        }
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
        e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
        e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(llave).int(w * h * 4).fin();
        e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(w).int(h).int(w * 4).uint(formato).fin();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
        e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
        e.empezar(7, wl_surface::req::FRAME).uint(8).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        con.juntador.meter(&e.bytes, &e.llaves).unwrap();
        con.atender(&globales(), &mut escena.de(c));
        assert!(con.roto.is_none(), "{:?}", con.roto);
    }

    #[test]
    fn dos_ventanas_apiladas_y_avisos() {
        let mut escena = Escena::new(Compartida::default(), 0xFF20_4060);
        let (mut a, mut b) = (Conexion::new(), Conexion::new());
        cliente(&mut escena, &mut a, 1, 70, 20, 10, 0xFFFF_0000, XRGB8888);
        cliente(&mut escena, &mut b, 2, 71, 10, 10, 0x8000_0080, ARGB8888); // azul a medias, premultiplicado
        // wl_shm anunció sus dos formatos al enlazarlo.
        let ev = eventos(&a.sacar());
        assert_eq!(ev.iter().filter(|e| e.0 == 4 && e.1 == wl_shm::ev::FORMAT).count(), 2);
        b.sacar();
        escena.colocar(1, 7, 5, 5);
        escena.colocar(2, 7, 15, 10);
        let (w, h) = (40, 30);
        let mut pantalla = vec![0u32; (w * h) as usize];
        let mut avisos = Vec::new();
        let danos = escena.componer(&mut pantalla, w, h, w as usize, 1234, &mut |c, aviso, ms| avisos.push((c, aviso, ms)));
        assert!(!danos.is_empty());
        let px = |p: &[u32], x: i32, y: i32| p[(y * w + x) as usize];
        let pa = &pantalla;
        assert_eq!(px(pa, 0, 0), 0xFF20_4060); // fondo
        assert_eq!(px(pa, 6, 6), 0xFFFF_0000); // la roja
        assert_eq!(px(pa, 30, 18), 0xFF20_4060); // fuera de las dos
        // La azul, a medias sobre la roja y sobre el fondo.
        assert_eq!(px(pa, 16, 11), 0xFF7F_0080);
        assert_eq!(px(pa, 24, 16), 0xFF10_20B0);
        assert_eq!(avisos, [(1, 8, 1234), (2, 8, 1234)]);

        // Otra vez sin cambios: nada que componer, ni avisos.
        avisos.clear();
        assert!(escena.componer(&mut pantalla, w, h, w as usize, 1250, &mut |c, a, ms| avisos.push((c, a, ms))).is_empty());
        assert!(avisos.is_empty());

        // La roja cambia de búfer: el viejo se devuelve con release.
        let mut e = Escritor::new();
        e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(9).int(0).int(20).int(10).int(80).uint(XRGB8888).fin();
        e.empezar(7, wl_surface::req::ATTACH).uint(9).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        a.juntador.meter(&e.bytes, &[]).unwrap();
        a.atender(&globales(), &mut escena.de(1));
        let ev = eventos(&a.sacar());
        assert_eq!(ev, [(6, wl_buffer::ev::RELEASE, Vec::new())]);

        // El cliente azul se va: su sitio vuelve a ser rojo y fondo.
        escena.adios(2);
        escena.componer(&mut pantalla, w, h, w as usize, 1300, &mut |_, _, _| {});
        assert_eq!(px(&pantalla, 16, 11), 0xFFFF_0000);
        assert_eq!(px(&pantalla, 24, 16), 0xFF20_4060);
    }

    /// E8: lo que cuesta componer una pantalla de 800 x 600 entera con tres ventanas (una XRGB, una ARGB opaca y una
    /// ARGB a medias que se solapan). Se mide en el PC: `cargo test --release -- --ignored medir_componer --nocapture`.
    #[test]
    #[ignore]
    fn medir_componer() {
        extern crate std;
        let mut escena = Escena::new(Compartida::default(), 0xFF20_4060);
        let (mut a, mut b, mut c) = (Conexion::new(), Conexion::new(), Conexion::new());
        let variante: u32 = std::env::var("VARIANTE").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        if variante == 0 || variante == 2 {
            cliente(&mut escena, &mut a, 1, 70, 400, 300, 0xFFFF_0000, XRGB8888);
            escena.colocar(1, 7, 50, 50);
        }
        if variante == 0 || variante == 3 {
            cliente(&mut escena, &mut b, 2, 71, 400, 300, 0xFF33_6699, ARGB8888);
            escena.colocar(2, 7, 200, 150);
        }
        if variante == 0 || variante == 4 {
            cliente(&mut escena, &mut c, 3, 72, 300, 200, 0x8000_0080, ARGB8888);
            escena.colocar(3, 7, 300, 250);
        }
        let (w, h) = (800, 600);
        let mut pantalla = vec![0u32; (w * h) as usize];
        let vueltas = 300;
        let t0 = std::time::Instant::now();
        for _ in 0..vueltas {
            escena.danos.push(Rect { x: 0, y: 0, w, h });
            escena.componer(&mut pantalla, w, h, w as usize, 0, &mut |_, _, _| {});
        }
        let us = t0.elapsed().as_micros() as f64 / vueltas as f64;
        std::println!("componer 800x600 con 3 ventanas: {:.0} µs por cuadro", us);
        if variante != 0 {
            return;
        }
        // Lo que sale tiene que ser lo de siempre.
        let px = |x: i32, y: i32| pantalla[(y * w + x) as usize];
        assert_eq!((px(10, 10), px(60, 60), px(250, 200)), (0xFF20_4060, 0xFFFF_0000, 0xFF33_6699));
        assert_eq!(px(350, 300), mezclar(0x8000_0080, 0xFF33_6699));
    }

    #[test]
    fn adjuntar_algo_que_no_es_un_bufer() {
        let mut escena = Escena::new(Compartida::default(), 0);
        let mut a = Conexion::new();
        cliente(&mut escena, &mut a, 1, 70, 4, 4, 0, XRGB8888);
        let mut e = Escritor::new();
        e.empezar(7, wl_surface::req::ATTACH).uint(5).int(0).int(0).fin(); // 5 es el pool
        a.juntador.meter(&e.bytes, &[]).unwrap();
        a.atender(&globales(), &mut escena.de(1));
        assert!(a.roto.is_some());
    }
}

mod ventanas {
    use super::*;
    use crate::escena::*;
    use crate::servidor::*;
    use crate::shm::{Memorias, XRGB8888};
    use crate::tablas::{wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};

    #[derive(Default)]
    struct Mem(alloc::collections::BTreeMap<u32, Vec<u8>>);
    impl Memorias for Mem {
        fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
            Some(self.0.entry(llave).or_insert_with(|| vec![0x11; 1 << 16]).as_ptr())
        }
        fn soltar(&mut self, _: u32, _: *const u8, _: usize) {}
        fn llave_con(&mut self, _: &'static [u8]) -> Option<u32> {
            Some(0xDA7A)
        }
    }

    fn globales() -> [Global; 10] {
        [
            Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
            Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
            Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
            Global { nombre: 4, interfaz: &tablas::WL_SUBCOMPOSITOR, version: 1 },
            Global { nombre: 5, interfaz: &tablas::ZWLR_LAYER_SHELL_V1, version: 4 },
            Global { nombre: 6, interfaz: &tablas::EXT_SESSION_LOCK_MANAGER_V1, version: 1 },
            Global { nombre: 7, interfaz: &tablas::WL_OUTPUT, version: 4 },
            Global { nombre: 8, interfaz: &tablas::ZWP_LINUX_DMABUF_V1, version: 4 },
            Global { nombre: 9, interfaz: &tablas::EXT_IDLE_NOTIFIER_V1, version: 2 },
            Global { nombre: 10, interfaz: &tablas::WL_SEAT, version: 7 },
        ]
    }

    /// U3: avisos de inactividad. Cada uno cuenta desde que se pide; vence con su plazo (`idled`), una tecla o el
    /// ratón dan `resumed` y vuelven a contar; destruir uno lo quita sin tocar los demás; de más es un error; y al irse
    /// el cliente no queda nada.
    #[test]
    fn inactividad() {
        use crate::tablas::{ext_idle_notification_v1, ext_idle_notifier_v1};
        let mut escena = Escena::new(Mem::default(), 0);
        let mut c = Conexion::new();
        escena.reloj_ms = 1_000;
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(9).texto(Some("ext_idle_notifier_v1")).uint(2).uint(50).fin();
        e.empezar(2, wl_registry::req::BIND).uint(10).texto(Some("wl_seat")).uint(7).uint(51).fin();
        e.empezar(50, ext_idle_notifier_v1::req::GET_IDLE_NOTIFICATION).uint(52).uint(5_000).uint(51).fin();
        e.empezar(50, ext_idle_notifier_v1::req::GET_INPUT_IDLE_NOTIFICATION).uint(53).uint(8_000).uint(51).fin();
        mandar(&mut escena, &mut c, 4, &e);
        assert!(c.roto.is_none(), "{:?}", c.roto);
        // Antes de tiempo, nada; falta lo del primero.
        escena.reloj_ms = 5_999;
        assert_eq!(escena.revisar_inactividad(), Some(1));
        assert!(escena.avisos_inactividad.is_empty());
        escena.reloj_ms = 6_000;
        assert_eq!(escena.revisar_inactividad(), Some(3_000));
        assert_eq!(core::mem::take(&mut escena.avisos_inactividad), [(4, 52, true)]);
        // Un idled no se repite.
        escena.reloj_ms = 9_000;
        assert_eq!(escena.revisar_inactividad(), None);
        assert_eq!(core::mem::take(&mut escena.avisos_inactividad), [(4, 53, true)]);
        assert_eq!(escena.revisar_inactividad(), None);
        assert!(escena.avisos_inactividad.is_empty());
        // Una tecla (aunque nadie tenga el teclado): resumed a los dos, y vuelven a contar desde ella.
        escena.reloj_ms = 9_500;
        escena.tecla(4, true);
        assert_eq!(core::mem::take(&mut escena.avisos_inactividad), [(4, 52, false), (4, 53, false)]);
        escena.reloj_ms = 14_499;
        assert_eq!(escena.revisar_inactividad(), Some(1));
        // El ratón también cuenta, y una segunda entrada sin idled entre medias no da otro resumed.
        escena.raton(1, 0, 0);
        escena.raton(1, 0, 0);
        assert!(escena.avisos_inactividad.is_empty());
        // Los eventos salen con su objeto.
        avisar_inactividad(&mut c, 52, true);
        avisar_inactividad(&mut c, 52, false);
        let ev: Vec<_> = eventos(&c.sacar()).into_iter().map(|x| (x.0, x.1)).collect();
        assert_eq!(ev, [(52, ext_idle_notification_v1::ev::IDLED), (52, ext_idle_notification_v1::ev::RESUMED)]);
        // Destruir uno: el otro sigue.
        let mut e = Escritor::new();
        e.empezar(52, ext_idle_notification_v1::req::DESTROY).fin();
        mandar(&mut escena, &mut c, 4, &e);
        escena.reloj_ms = 30_000;
        assert_eq!(escena.revisar_inactividad(), None);
        assert_eq!(core::mem::take(&mut escena.avisos_inactividad), [(4, 53, true)]);
        // Hostil: pedir de más da error, no memoria sin fin.
        let mut h = Conexion::new();
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(9).texto(Some("ext_idle_notifier_v1")).uint(2).uint(50).fin();
        e.empezar(2, wl_registry::req::BIND).uint(10).texto(Some("wl_seat")).uint(7).uint(51).fin();
        for i in 0..MAX_INACTIVIDAD as u32 {
            e.empezar(50, ext_idle_notifier_v1::req::GET_IDLE_NOTIFICATION).uint(60 + i).uint(0).uint(51).fin();
        }
        mandar(&mut escena, &mut h, 5, &e);
        assert!(h.roto.is_some(), "pidió más de {MAX_INACTIVIDAD} y no dio error");
        // Al irse los dos, no queda nada que vigilar.
        escena.adios(5);
        escena.adios(4);
        escena.reloj_ms = 90_000;
        assert_eq!(escena.revisar_inactividad(), None);
        assert!(escena.avisos_inactividad.is_empty());
    }

    /// G5: un bloqueador. Objetos: 3 compositor, 4 shm, 39 pantalla, 40 manager, 41 lock, 42 superficie, 43 lock_surface, 44 pool,
    /// 45 búfer de la pantalla entera (100 x 60). Devuelve los eventos del lock.
    fn bloquear(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente) -> Vec<(u32, u16, Vec<u8>)> {
        use crate::tablas::ext_session_lock_manager_v1;
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
        e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
        e.empezar(2, wl_registry::req::BIND).uint(6).texto(Some("ext_session_lock_manager_v1")).uint(1).uint(40).fin();
        e.empezar(2, wl_registry::req::BIND).uint(7).texto(Some("wl_output")).uint(4).uint(39).fin();
        e.empezar(40, ext_session_lock_manager_v1::req::LOCK).uint(41).fin();
        mandar(escena, con, c, &e)
    }

    /// Pone la pantalla de bloqueo: get_lock_surface, ack y búfer del tamaño que pidió.
    fn pantalla_de_bloqueo(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, llave: u32) {
        use crate::tablas::{ext_session_lock_surface_v1, ext_session_lock_v1};
        let mut e = Escritor::new();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(42).fin();
        e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(43).uint(42).uint(39).fin();
        let ev = mandar(escena, con, c, &e);
        let conf = ev.iter().find(|x| (x.0, x.1) == (43, ext_session_lock_surface_v1::ev::CONFIGURE)).expect("sin configure");
        assert_eq!(&conf.2[4..12], &[100u32.to_le_bytes(), 60u32.to_le_bytes()].concat()[..], "la pantalla entera");
        let serie = u32::from_le_bytes(conf.2[..4].try_into().unwrap());
        let mut e = Escritor::new();
        e.empezar(43, ext_session_lock_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
        e.empezar(4, wl_shm::req::CREATE_POOL).uint(44).llave(llave).int(100 * 60 * 4).fin();
        e.empezar(44, wl_shm_pool::req::CREATE_BUFFER).uint(45).int(0).int(100).int(60).int(400).uint(XRGB8888).fin();
        e.empezar(42, wl_surface::req::ATTACH).uint(45).int(0).int(0).fin();
        e.empezar(42, wl_surface::req::COMMIT).fin();
        mandar(escena, con, c, &e);
        assert!(con.roto.is_none(), "{:?}", con.roto);
    }

    /// G5: bloquear tapa todo al momento (negro) y quita el teclado; la pantalla de bloqueo lo toma; un segundo
    /// bloqueador nace acabado; si el bloqueador muere sigue bloqueado; otro toma el relevo y desbloquea.
    #[test]
    fn bloqueo_de_sesion() {
        use crate::tablas::{ext_session_lock_v1, ext_session_lock_v1::ev as lev};
        let mut escena = Escena::new(Mem::default(), 0);
        escena.fondo = 0xFF12_3456;
        let mut pantalla = vec![0u32; 100 * 60];
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        let mut v = Conexion::new();
        abrir(&mut escena, &mut v, 1, 70);
        assert_eq!(escena.teclado, Some((1, 7)));
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 100 + 41], 0xFF11_1111);
        // Bloquear: locked, y ya no se ve la ventana ni tiene el teclado.
        let mut b = Conexion::new();
        let ev = bloquear(&mut escena, &mut b, 9);
        assert!(ev.iter().any(|x| (x.0, x.1) == (41, lev::LOCKED)));
        assert_eq!(escena.teclado, None);
        assert!(escena.raices().is_empty());
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 100 + 41], 0xFF00_0000, "la ventana sigue viéndose");
        assert_eq!(pantalla[5 * 100 + 5], 0xFF00_0000, "el fondo de siempre, no negro");
        // La pantalla de bloqueo: se ve y tiene el teclado; las teclas van a ella, y Super+Arriba no hace nada.
        pantalla_de_bloqueo(&mut escena, &mut b, 9, 80);
        assert_eq!(escena.raices(), [(9, 42)]);
        assert_eq!(escena.teclado, Some((9, 42)));
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 100 + 41], 0xFF11_1111, "la pantalla de bloqueo");
        // La ventana no puede tomar el teclado ni subir.
        assert!(!escena.puede_tomar_teclado(1));
        // Otro bloqueador mientras este vive: finished.
        let mut otro = Conexion::new();
        let ev = bloquear(&mut escena, &mut otro, 10);
        assert!(ev.iter().any(|x| (x.0, x.1) == (41, lev::FINISHED)));
        assert_eq!(escena.bloqueo.as_ref().unwrap().cliente, 9);
        // Muere el bloqueador: sigue bloqueado, en negro, sin teclado.
        escena.adios(9);
        assert!(escena.bloqueo.is_some());
        assert!(escena.raices().is_empty());
        assert_eq!(escena.teclado, None);
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 100 + 41], 0xFF00_0000);
        // La ventana no puede quitarlo: su unlock no es de nadie.
        // Otro con el poder toma el relevo y desbloquea: vuelve la ventana con el teclado.
        let mut relevo = Conexion::new();
        let ev = bloquear(&mut escena, &mut relevo, 11);
        assert!(ev.iter().any(|x| (x.0, x.1) == (41, lev::LOCKED)));
        let mut e = Escritor::new();
        e.empezar(41, ext_session_lock_v1::req::UNLOCK_AND_DESTROY).fin();
        mandar(&mut escena, &mut relevo, 11, &e);
        assert!(relevo.roto.is_none(), "{:?}", relevo.roto);
        assert!(escena.bloqueo.is_none());
        assert_eq!(escena.teclado, Some((1, 7)));
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 100 + 41], 0xFF11_1111);
        assert_eq!(pantalla[5 * 100 + 5], 0xFF12_3456);
    }

    /// G6: dmabuf (ya enlazado). Objetos: 50 linux_dmabuf, 51 feedback, 52 params, 53 búfer (16 x 16, lineal, llave 90).
    fn dmabuf(e: &mut Escritor, modificador: u64, inmediato: bool) {
        use crate::tablas::{zwp_linux_buffer_params_v1 as par, zwp_linux_dmabuf_v1 as dma};
        e.empezar(50, dma::req::CREATE_PARAMS).uint(52).fin();
        e.empezar(52, par::req::ADD).llave(90).uint(0).uint(0).uint(64).uint((modificador >> 32) as u32).uint(modificador as u32).fin();
        if inmediato {
            e.empezar(52, par::req::CREATE_IMMED).uint(53).int(16).int(16).uint(DRM_XRGB8888).uint(0).fin();
        } else {
            e.empezar(52, par::req::CREATE).int(16).int(16).uint(DRM_XRGB8888).uint(0).fin();
        }
        e.empezar(52, par::req::DESTROY).fin();
    }

    /// G6: la tabla de formatos y una ventana con un búfer de dmabuf, que se ve como uno de wl_shm.
    #[test]
    fn dmabuf_lineal() {
        use crate::tablas::{zwp_linux_buffer_params_v1 as par, zwp_linux_dmabuf_feedback_v1 as fb, zwp_linux_dmabuf_v1 as dma};
        let mut escena = Escena::new(Mem::default(), 0);
        escena.dispositivo = 0xE280;
        let mut con = Conexion::new();
        abrir(&mut escena, &mut con, 1, 70);
        let mut e = Escritor::new();
        e.empezar(2, wl_registry::req::BIND).uint(8).texto(Some("zwp_linux_dmabuf_v1")).uint(4).uint(50).fin();
        e.empezar(50, dma::req::GET_DEFAULT_FEEDBACK).uint(51).fin();
        let ev = mandar(&mut escena, &mut con, 1, &e);
        let ordenes: Vec<u16> = ev.iter().filter(|x| x.0 == 51).map(|x| x.1).collect();
        assert_eq!(ordenes, [fb::ev::FORMAT_TABLE, fb::ev::MAIN_DEVICE, fb::ev::TRANCHE_TARGET_DEVICE, fb::ev::TRANCHE_FORMATS, fb::ev::TRANCHE_FLAGS, fb::ev::TRANCHE_DONE, fb::ev::DONE]);
        let disp = &ev.iter().find(|x| (x.0, x.1) == (51, fb::ev::MAIN_DEVICE)).unwrap().2;
        assert_eq!(&disp[..], &[&8u32.to_le_bytes()[..], &0xE280u64.to_le_bytes()].concat()[..]);
        assert_eq!(&TABLA_FORMATOS[..4], b"AR24");
        assert_eq!(&TABLA_FORMATOS[16..20], b"XR24");
        // create_immed lineal: el búfer va a la ventana y se ve.
        let mut e = Escritor::new();
        dmabuf(&mut e, MOD_LINEAL, true);
        e.empezar(7, wl_surface::req::ATTACH).uint(53).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut con, 1, &e);
        assert!(con.roto.is_none(), "{:?}", con.roto);
        assert_eq!(escena.clientes[&1].superficies[&7].bufer, Some(53));
        assert_eq!(escena.clientes[&1].shm.llave_dmabuf(53), Some(90));
        assert!(escena.clientes[&1].params.is_empty());
        // create (no inmediato): el búfer lo numera el servidor y llega con created.
        let mut e = Escritor::new();
        e.empezar(50, dma::req::CREATE_PARAMS).uint(54).fin();
        e.empezar(54, par::req::ADD).llave(91).uint(0).uint(0).uint(64).uint(0).uint(0).fin();
        e.empezar(54, par::req::CREATE).int(16).int(16).uint(DRM_ARGB8888).uint(0).fin();
        let ev = mandar(&mut escena, &mut con, 1, &e);
        let creado = ev.iter().find(|x| (x.0, x.1) == (54, par::ev::CREATED)).expect("sin created");
        let id = u32::from_le_bytes(creado.2[..4].try_into().unwrap());
        assert!(id >= crate::servidor::PRIMERO_DEL_SERVIDOR);
        assert!(escena.clientes[&1].shm.buferes.contains_key(&id));
        // Un modificador que no se anunció, con create: failed, y la conexión sigue.
        let mut e = Escritor::new();
        e.empezar(50, dma::req::CREATE_PARAMS).uint(55).fin();
        e.empezar(55, par::req::ADD).llave(92).uint(0).uint(0).uint(64).uint(0x0100_0000).uint(1).fin();
        e.empezar(55, par::req::CREATE).int(16).int(16).uint(DRM_XRGB8888).uint(0).fin();
        let ev = mandar(&mut escena, &mut con, 1, &e);
        assert!(ev.iter().any(|x| (x.0, x.1) == (55, par::ev::FAILED)));
        assert!(con.roto.is_none());
    }

    #[test]
    fn errores_de_dmabuf() {
        use crate::tablas::{zwp_linux_buffer_params_v1 as par, zwp_linux_dmabuf_v1 as dma};
        let base = |e: &mut Escritor| {
            e.empezar(2, wl_registry::req::BIND).uint(8).texto(Some("zwp_linux_dmabuf_v1")).uint(4).uint(50).fin();
            e.empezar(50, dma::req::CREATE_PARAMS).uint(52).fin();
        };
        let add = |e: &mut Escritor, plano: u32| {
            e.empezar(52, par::req::ADD).llave(90 + plano).uint(plano).uint(0).uint(64).uint(0).uint(0).fin();
        };
        let crear = |e: &mut Escritor, w: i32, f: u32| {
            e.empezar(52, par::req::CREATE_IMMED).uint(53).int(w).int(16).uint(f).uint(0).fin();
        };
        assert_eq!(error(&|e| { base(e); add(e, 4); }), (52, DMABUF_PLANO));
        assert_eq!(error(&|e| { base(e); add(e, 0); add(e, 0); }), (52, DMABUF_PLANO_PUESTO));
        assert_eq!(error(&|e| { base(e); add(e, 0); crear(e, 16, DRM_XRGB8888); add(e, 1); }), (52, DMABUF_USADO));
        assert_eq!(error(&|e| { base(e); add(e, 0); crear(e, 16, u32::from_le_bytes(*b"NV12")); }), (52, DMABUF_FORMATO));
        assert_eq!(error(&|e| { base(e); add(e, 1); crear(e, 16, DRM_XRGB8888); }), (52, DMABUF_INCOMPLETO));
        assert_eq!(error(&|e| { base(e); add(e, 0); add(e, 1); crear(e, 16, DRM_XRGB8888); }), (52, DMABUF_INCOMPLETO));
        assert_eq!(error(&|e| { base(e); add(e, 0); crear(e, 0, DRM_XRGB8888); }), (52, DMABUF_MEDIDAS));
        // 64 bytes por línea no bastan para 17 de ancho.
        assert_eq!(error(&|e| { base(e); add(e, 0); crear(e, 17, DRM_XRGB8888); }), (52, DMABUF_FUERA));
        assert_eq!(error(&|e| { base(e); e.empezar(52, par::req::DESTROY).fin(); dmabuf(e, 0x0100_0000_0000_0001, true); }), (52, DMABUF_BUFER));
    }

    #[test]
    fn errores_de_bloqueo() {
        use crate::tablas::{ext_session_lock_surface_v1, ext_session_lock_v1};
        let probar = |mas: &dyn Fn(&mut Escritor)| {
            let mut escena = Escena::new(Mem::default(), 0);
            let mut pantalla = vec![0u32; 100 * 60];
            escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
            let mut con = Conexion::new();
            bloquear(&mut escena, &mut con, 9);
            let mut e = Escritor::new();
            e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(42).fin();
            e.empezar(4, wl_shm::req::CREATE_POOL).uint(44).llave(80).int(100 * 60 * 4).fin();
            e.empezar(44, wl_shm_pool::req::CREATE_BUFFER).uint(45).int(0).int(100).int(60).int(400).uint(XRGB8888).fin();
            e.empezar(44, wl_shm_pool::req::CREATE_BUFFER).uint(46).int(0).int(50).int(60).int(200).uint(XRGB8888).fin();
            mas(&mut e);
            mandar(&mut escena, &mut con, 9, &e);
            let r = con.roto.expect("tenía que cortarse");
            (r.objeto, r.codigo)
        };
        // destroy con la sesión bloqueada.
        assert_eq!(probar(&|e| { e.empezar(41, ext_session_lock_v1::req::DESTROY).fin(); }), (41, BLOQUEO_DESTRUIR));
        // Búfer antes de su configure.
        assert_eq!(
            probar(&|e| {
                e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(43).uint(42).uint(39).fin();
                e.empezar(42, wl_surface::req::ATTACH).uint(45).int(0).int(0).fin();
                e.empezar(42, wl_surface::req::COMMIT).fin();
            }),
            (43, BLOQUEO_COMMIT_SIN_ACK)
        );
        // Un búfer que no tapa la pantalla entera.
        assert_eq!(
            probar(&|e| {
                e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(43).uint(42).uint(39).fin();
                e.empezar(43, ext_session_lock_surface_v1::req::ACK_CONFIGURE).uint(1).fin();
                e.empezar(42, wl_surface::req::ATTACH).uint(46).int(0).int(0).fin();
                e.empezar(42, wl_surface::req::COMMIT).fin();
            }),
            (43, BLOQUEO_TAMANO)
        );
        // Dos para la misma pantalla.
        assert_eq!(
            probar(&|e| {
                e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(47).fin();
                e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(43).uint(42).uint(39).fin();
                e.empezar(41, ext_session_lock_v1::req::GET_LOCK_SURFACE).uint(48).uint(47).uint(39).fin();
            }),
            (41, BLOQUEO_PANTALLA_REPETIDA)
        );
    }

    /// G4: una barra arriba (capa 2) de 20 de alto con su zona exclusiva; la ventana nueva se coloca debajo.
    #[test]
    fn capa_con_zona_exclusiva() {
        use crate::tablas::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
        let mut escena = Escena::new(Mem::default(), 0);
        let mut pantalla = vec![0u32; 200 * 150];
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        let mut barra = Conexion::new();
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
        e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
        e.empezar(2, wl_registry::req::BIND).uint(5).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(30).fin();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
        e.empezar(30, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(32).uint(31).uint(0).uint(2).texto(Some("barra")).fin();
        e.empezar(32, zwlr_layer_surface_v1::req::SET_SIZE).uint(0).uint(20).fin();
        e.empezar(32, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(ARRIBA | IZQUIERDA | DERECHA).fin();
        e.empezar(32, zwlr_layer_surface_v1::req::SET_EXCLUSIVE_ZONE).int(20).fin();
        e.empezar(31, wl_surface::req::COMMIT).fin();
        let ev = mandar(&mut escena, &mut barra, 9, &e);
        let conf = ev.iter().find(|x| (x.0, x.1) == (32, zwlr_layer_surface_v1::ev::CONFIGURE)).expect("sin configure");
        let serie = u32::from_le_bytes(conf.2[..4].try_into().unwrap());
        assert_eq!(&conf.2[4..12], &[200u32.to_le_bytes(), 20u32.to_le_bytes()].concat()[..], "toda la anchura, 20 de alto");
        let mut e = Escritor::new();
        e.empezar(32, zwlr_layer_surface_v1::req::ACK_CONFIGURE).uint(serie).fin();
        e.empezar(4, wl_shm::req::CREATE_POOL).uint(33).llave(72).int(200 * 20 * 4).fin();
        e.empezar(33, wl_shm_pool::req::CREATE_BUFFER).uint(34).int(0).int(200).int(20).int(800).uint(XRGB8888).fin();
        e.empezar(31, wl_surface::req::ATTACH).uint(34).int(0).int(0).fin();
        e.empezar(31, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut barra, 9, &e);
        assert!(barra.roto.is_none(), "{:?}", barra.roto);
        assert_eq!(escena.clientes[&9].superficies[&31].sitio, Some((0, 0)));
        assert_eq!(escena.area, Rect { x: 0, y: 20, w: 200, h: 130 });
        // La ventana nueva, en cascada dentro de lo que queda.
        let mut v = Conexion::new();
        abrir(&mut escena, &mut v, 1, 70);
        assert_eq!(escena.clientes[&1].superficies[&7].sitio, Some((40, 60)));
        // La barra (capa de arriba) se pinta encima de las ventanas.
        assert_eq!(escena.raices().last(), Some(&(9, 31)));
    }

    #[test]
    fn errores_de_capas() {
        use crate::tablas::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};
        let base = |e: &mut Escritor| {
            e.empezar(2, wl_registry::req::BIND).uint(5).texto(Some("zwlr_layer_shell_v1")).uint(4).uint(30).fin();
            e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
        };
        // Tamaño 0 de ancho sin anclas a izquierda y derecha.
        assert_eq!(
            error(&|e| {
                base(e);
                e.empezar(30, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(32).uint(31).uint(0).uint(2).texto(Some("x")).fin();
                e.empezar(32, zwlr_layer_surface_v1::req::SET_ANCHOR).uint(ARRIBA).fin();
                e.empezar(31, wl_surface::req::COMMIT).fin();
            }),
            (32, CAPA_TAMANO_MALO)
        );
        // Una capa que no existe, y una ventana que quiere ser capa.
        assert_eq!(error(&|e| { base(e); e.empezar(30, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(32).uint(31).uint(0).uint(7).texto(Some("x")).fin(); }), (30, CAPA_INVALIDA));
        assert_eq!(error(&|e| { base(e); e.empezar(30, zwlr_layer_shell_v1::req::GET_LAYER_SURFACE).uint(32).uint(7).uint(0).uint(2).texto(Some("x")).fin(); }), (30, CAPA_ROL));
    }

    /// G3: una subsuperficie de 8 x 8 en (5, 5) de una ventana de 16 x 16 en (40, 40). Objetos: 20 subcompositor,
    /// 21 superficie hija, 22 pool (llave 71: el primer búfer 0x22, el segundo 0x33), 23 y 25 búferes, 24 subsurface.
    #[test]
    fn subsuperficies() {
        use crate::tablas::{wl_subcompositor, wl_subsurface};
        let mut escena = Escena::new(Mem::default(), 0);
        let mut con = Conexion::new();
        abrir(&mut escena, &mut con, 1, 70);
        let mut e = Escritor::new();
        e.empezar(2, wl_registry::req::BIND).uint(4).texto(Some("wl_subcompositor")).uint(1).uint(20).fin();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(21).fin();
        e.empezar(4, wl_shm::req::CREATE_POOL).uint(22).llave(71).int(2 * 8 * 8 * 4).fin();
        e.empezar(22, wl_shm_pool::req::CREATE_BUFFER).uint(23).int(0).int(8).int(8).int(32).uint(XRGB8888).fin();
        e.empezar(22, wl_shm_pool::req::CREATE_BUFFER).uint(25).int(256).int(8).int(8).int(32).uint(XRGB8888).fin();
        e.empezar(20, wl_subcompositor::req::GET_SUBSURFACE).uint(24).uint(21).uint(7).fin();
        e.empezar(24, wl_subsurface::req::SET_POSITION).int(5).int(5).fin();
        e.empezar(21, wl_surface::req::ATTACH).uint(23).int(0).int(0).fin();
        e.empezar(21, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut con, 1, &e);
        assert!(con.roto.is_none(), "{:?}", con.roto);
        {
            let m = escena.memorias.0.get_mut(&71).unwrap();
            m[..256].fill(0x22);
            m[256..512].fill(0x33);
        }
        let mut pantalla = vec![0u32; 200 * 150];
        let px = |p: &Vec<u32>, x: usize, y: usize| p[y * 200 + x];
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        // Sincronizada: hasta el commit de la madre, no se ve.
        assert_eq!(px(&pantalla, 46, 46), 0xFF11_1111);
        let mut e = Escritor::new();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut con, 1, &e);
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        assert_eq!(px(&pantalla, 46, 46), 0xFF22_2222, "la hija, encima y en (45, 45)");
        assert_eq!(px(&pantalla, 54, 54), 0xFF11_1111, "fuera de la hija, la madre");
        assert_eq!(escena.bajo(46, 46).map(|b| b.0), Some((1, 21)), "el ratón ve la hija");
        // Debajo de la madre: deja de verse.
        let mut e = Escritor::new();
        e.empezar(24, wl_subsurface::req::PLACE_BELOW).uint(7).fin();
        mandar(&mut escena, &mut con, 1, &e);
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        assert_eq!(px(&pantalla, 46, 46), 0xFF11_1111);
        // Encima otra vez y sin sincronizar: su commit se ve sin el de la madre.
        let mut e = Escritor::new();
        e.empezar(24, wl_subsurface::req::PLACE_ABOVE).uint(7).fin();
        e.empezar(24, wl_subsurface::req::SET_DESYNC).fin();
        e.empezar(21, wl_surface::req::ATTACH).uint(25).int(0).int(0).fin();
        e.empezar(21, wl_surface::req::DAMAGE).int(0).int(0).int(8).int(8).fin();
        e.empezar(21, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut con, 1, &e);
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        assert_eq!(px(&pantalla, 46, 46), 0xFF33_3333);
        assert!(con.roto.is_none(), "{:?}", con.roto);
    }

    #[test]
    fn errores_de_subsuperficies() {
        use crate::tablas::wl_subcompositor;
        let preparar_sub = |e: &mut Escritor| {
            e.empezar(2, wl_registry::req::BIND).uint(4).texto(Some("wl_subcompositor")).uint(1).uint(20).fin();
            e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(31).fin();
            e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(32).fin();
        };
        // Una superficie con papel de ventana no puede ser hija.
        assert_eq!(
            error(&|e| {
                preparar_sub(e);
                e.empezar(20, wl_subcompositor::req::GET_SUBSURFACE).uint(30).uint(7).uint(31).fin();
            }),
            (20, BAD_SURFACE)
        );
        // Ni madre de sí misma, ni madre que cuelga de la hija.
        assert_eq!(error(&|e| { preparar_sub(e); e.empezar(20, wl_subcompositor::req::GET_SUBSURFACE).uint(30).uint(31).uint(31).fin(); }), (20, BAD_PARENT));
        assert_eq!(
            error(&|e| {
                preparar_sub(e);
                e.empezar(20, wl_subcompositor::req::GET_SUBSURFACE).uint(33).uint(31).uint(32).fin();
                e.empezar(20, wl_subcompositor::req::GET_SUBSURFACE).uint(34).uint(32).uint(31).fin();
            }),
            (20, BAD_PARENT)
        );
    }

    /// Objetos del cliente: 3 compositor, 4 shm, 5 pool, 6 búfer, 7 superficie, 8 xdg_wm_base, 9 xdg_surface,
    /// 10 xdg_toplevel.
    fn preparar(e: &mut Escritor, llave: u32) {
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
        e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
        e.empezar(2, wl_registry::req::BIND).uint(3).texto(Some("xdg_wm_base")).uint(6).uint(8).fin();
        e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(llave).int(16 * 16 * 4).fin();
        e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(16).int(16).int(64).uint(XRGB8888).fin();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
        e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(9).uint(7).fin();
        e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(10).fin();
        e.empezar(10, xdg_toplevel::req::SET_TITLE).texto(Some("Prueba")).fin();
    }

    fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
        con.juntador.meter(&e.bytes, &e.llaves).unwrap();
        con.atender(&globales(), &mut escena.de(c));
        eventos(&con.sacar())
    }

    /// Una ventana entera: commit sin búfer, configure, ack, búfer. Devuelve la serie del configure.
    fn abrir(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, llave: u32) -> u32 {
        let mut e = Escritor::new();
        preparar(&mut e, llave);
        e.empezar(7, wl_surface::req::COMMIT).fin();
        let ev = mandar(escena, con, c, &e);
        let conf: Vec<_> = ev.iter().filter(|x| x.0 == 9 || x.0 == 10).collect();
        assert_eq!((conf[0].0, conf[0].1), (10, xdg_toplevel::ev::CONFIGURE));
        assert_eq!((conf[1].0, conf[1].1), (9, xdg_surface::ev::CONFIGURE));
        let serie = u32::from_le_bytes(conf[1].2[..4].try_into().unwrap());
        // Sin ack todavía: nada en pantalla.
        assert!(escena.orden.is_empty() || !escena.orden.contains(&(c, 7)));
        let mut e = Escritor::new();
        e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
        e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(escena, con, c, &e);
        assert!(con.roto.is_none(), "{:?}", con.roto);
        serie
    }

    /// S8: mientras se escribe en una ventana, una nueva de otro programa no se lleva el teclado.
    #[test]
    fn ventana_nueva_no_roba_el_teclado() {
        let mut escena = Escena::new(Mem::default(), 0);
        let (mut a, mut b, mut c) = (Conexion::new(), Conexion::new(), Conexion::new());
        escena.reloj_ms = 1_000;
        abrir(&mut escena, &mut a, 1, 70);
        assert_eq!(escena.teclado, Some((1, 7)));
        escena.tecla(4, true);
        escena.tecla(4, false);
        escena.reloj_ms = 1_500;
        abrir(&mut escena, &mut b, 2, 71);
        assert_eq!(escena.teclado, Some((1, 7)), "se la llevó mientras se escribía");
        assert_eq!(escena.orden.last(), Some(&(2, 7)), "sale encima");
        // Pasado el plazo sin teclear, sí.
        escena.reloj_ms = 1_000 + crate::escena::SIN_ROBAR_MS;
        abrir(&mut escena, &mut c, 3, 72);
        assert_eq!(escena.teclado, Some((3, 7)));
    }

    /// E8: el mismo búfer otra vez, con un daño pequeño, solo compone ese trozo (antes, la superficie entera tres veces),
    /// y lo nuevo se ve. Un búfer distinto sí se compone entero.
    #[test]
    fn mismo_bufer_solo_su_dano() {
        let mut escena = Escena::new(Mem::default(), 0);
        let mut a = Conexion::new();
        let mut pantalla = vec![0u32; 100 * 60];
        abrir(&mut escena, &mut a, 1, 70);
        escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        // El píxel (2, 3) de la ventana pasa a 0x22 en su memoria; se vuelve a dar el mismo búfer con ese daño.
        let px = escena.memorias.0.get_mut(&70).unwrap();
        px[(3 * 16 + 2) * 4..(3 * 16 + 2) * 4 + 4].copy_from_slice(&[0x22, 0x22, 0x22, 0]);
        let mut e = Escritor::new();
        e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::DAMAGE).int(2).int(3).int(1).int(1).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut a, 1, &e);
        let danos = escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        let area: i64 = danos.iter().map(|r| r.w as i64 * r.h as i64).sum();
        assert_eq!(area, 1, "{danos:?}");
        let (x, y) = escena.clientes[&1].superficies[&7].sitio.unwrap();
        assert_eq!(pantalla[(y + 3) as usize * 100 + (x + 2) as usize], 0xFF22_2222);
        // Otro búfer (del mismo pool): entera.
        let mut e = Escritor::new();
        e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(11).int(0).int(16).int(16).int(64).uint(XRGB8888).fin();
        e.empezar(7, wl_surface::req::ATTACH).uint(11).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut a, 1, &e);
        let danos = escena.componer(&mut pantalla, 100, 60, 100, 0, &mut |_, _, _| {});
        let area: i64 = danos.iter().map(|r| r.w as i64 * r.h as i64).sum();
        assert!(area >= 16 * 16, "{danos:?}");
    }

    /// U5: la ventana nueva de un cliente del sistema (el selector) sí se lleva el teclado mientras se escribe; al irse,
    /// deja de serlo.
    #[test]
    fn ventana_del_sistema_toma_el_teclado() {
        let mut escena = Escena::new(Mem::default(), 0);
        let (mut a, mut b) = (Conexion::new(), Conexion::new());
        escena.reloj_ms = 1_000;
        abrir(&mut escena, &mut a, 1, 70);
        escena.tecla(4, true);
        escena.tecla(4, false);
        escena.reloj_ms = 1_200;
        escena.del_sistema.push(2);
        abrir(&mut escena, &mut b, 2, 71);
        assert_eq!(escena.teclado, Some((2, 7)));
        escena.adios(2);
        assert!(escena.del_sistema.is_empty());
    }

    #[test]
    fn dos_ventanas_en_cascada() {
        let mut escena = Escena::new(Mem::default(), 0);
        let (mut a, mut b) = (Conexion::new(), Conexion::new());
        abrir(&mut escena, &mut a, 1, 70);
        abrir(&mut escena, &mut b, 2, 71);
        assert_eq!(escena.orden, [(1, 7), (2, 7)]);
        let sitio = |e: &Escena<Mem>, c: Cliente| e.clientes[&c].superficies[&7].sitio;
        assert_eq!(sitio(&escena, 1), Some((40, 40)));
        assert_eq!(sitio(&escena, 2), Some((72, 72)));
        assert_eq!(escena.clientes[&1].xdg[&9].titulo, "Prueba");
        // Se compone: el búfer (0x11111111 en XRGB) sale en su sitio.
        let mut pantalla = vec![0u32; 200 * 150];
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 200 + 41], 0xFF11_1111);
        assert_eq!(pantalla[10 * 200 + 10], 0);
        // Destruir la ventana la quita de la pantalla.
        let mut e = Escritor::new();
        e.empezar(10, xdg_toplevel::req::DESTROY).fin();
        mandar(&mut escena, &mut a, 1, &e);
        assert_eq!(escena.orden, [(2, 7)]);
        escena.componer(&mut pantalla, 200, 150, 200, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[41 * 200 + 41], 0);
    }

    fn error(preparar_mas: &dyn Fn(&mut Escritor)) -> (u32, u32) {
        let mut escena = Escena::new(Mem::default(), 0);
        let mut con = Conexion::new();
        let mut e = Escritor::new();
        preparar(&mut e, 70);
        preparar_mas(&mut e);
        mandar(&mut escena, &mut con, 1, &e);
        let r = con.roto.expect("tenía que cortarse");
        (r.objeto, r.codigo)
    }

    /// E3: mil daños antes de un commit se quedan en uno que los cubre a todos.
    #[test]
    fn danos_con_tope() {
        let mut escena = Escena::new(Mem::default(), 0);
        let mut con = Conexion::new();
        abrir(&mut escena, &mut con, 1, 70);
        let mut e = Escritor::new();
        for i in 0..1000 {
            e.empezar(7, wl_surface::req::DAMAGE).int(i % 16).int(i / 100).int(1).int(1).fin();
        }
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut con, 1, &e);
        assert!(con.roto.is_none(), "{:?}", con.roto);
        let s = &escena.clientes[&1].superficies[&7];
        assert!(s.danos.len() <= crate::escena::MAX_DANOS, "{}", s.danos.len());
        assert!(s.danos.iter().any(|r| r.x == 0 && r.y == 0 && r.w >= 16 && r.h >= 10), "{:?}", s.danos);
    }

    #[test]
    fn errores_de_ventana() {
        // Búfer antes de confirmar el configure.
        assert_eq!(
            error(&|e| {
                e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
                e.empezar(7, wl_surface::req::COMMIT).fin();
            }),
            (9, UNCONFIGURED_BUFFER)
        );
        // Confirmar un configure que no se mandó.
        assert_eq!(error(&|e| { e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(77).fin(); }), (9, INVALID_SERIAL));
        // Dos papeles para una superficie.
        assert_eq!(error(&|e| { e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(11).uint(7).fin(); }), (8, XDG_WM_BASE_ROLE));
        // Dos ventanas para una xdg_surface.
        assert_eq!(error(&|e| { e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(11).fin(); }), (9, ALREADY_CONSTRUCTED));
        // Una xdg_surface para una superficie que ya tiene búfer.
        assert_eq!(
            error(&|e| {
                e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(12).fin();
                e.empezar(12, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
                e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(13).uint(12).fin();
            }),
            (8, INVALID_SURFACE_STATE)
        );
        // Commit de una xdg_surface sin ventana.
        assert_eq!(
            error(&|e| {
                e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(12).fin();
                e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(13).uint(12).fin();
                e.empezar(12, wl_surface::req::COMMIT).fin();
            }),
            (13, NOT_CONSTRUCTED)
        );
    }
}

mod entrada {
    use super::*;
    use crate::escena::*;
    use crate::servidor::*;
    use crate::shm::{Memorias, XRGB8888};
    use crate::tablas::{wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};

    #[derive(Default)]
    struct Mem(alloc::collections::BTreeMap<u32, Vec<u8>>);
    impl Memorias for Mem {
        fn proyectar(&mut self, llave: u32, _t: usize) -> Option<*const u8> {
            Some(self.0.entry(llave).or_insert_with(|| vec![0; 1 << 16]).as_ptr())
        }
        fn soltar(&mut self, _: u32, _: *const u8, _: usize) {}
        fn llave_vacia(&mut self) -> Option<u32> {
            Some(99)
        }
        fn llave_con(&mut self, _datos: &'static [u8]) -> Option<u32> {
            Some(98)
        }
    }

    fn globales() -> [Global; 4] {
        [
            Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
            Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
            Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
            Global { nombre: 4, interfaz: &tablas::WL_SEAT, version: 7 },
        ]
    }

    fn mandar(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, e: &Escritor) -> Vec<(u32, u16, Vec<u8>)> {
        con.juntador.meter(&e.bytes, &e.llaves).unwrap();
        con.atender(&globales(), &mut escena.de(c));
        assert!(con.roto.is_none(), "{:?}", con.roto);
        eventos(&con.sacar())
    }

    /// Un cliente con una ventana de `w`×`h`, el asiento, su puntero (12) y su teclado (13).
    fn abrir(escena: &mut Escena<Mem>, con: &mut Conexion, c: Cliente, w: i32, h: i32) -> Vec<(u32, u16, Vec<u8>)> {
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(2).fin();
        e.empezar(2, wl_registry::req::BIND).uint(1).texto(Some("wl_compositor")).uint(6).uint(3).fin();
        e.empezar(2, wl_registry::req::BIND).uint(2).texto(Some("wl_shm")).uint(1).uint(4).fin();
        e.empezar(2, wl_registry::req::BIND).uint(3).texto(Some("xdg_wm_base")).uint(6).uint(8).fin();
        e.empezar(2, wl_registry::req::BIND).uint(4).texto(Some("wl_seat")).uint(7).uint(11).fin();
        e.empezar(11, wl_seat::req::GET_POINTER).uint(12).fin();
        e.empezar(11, wl_seat::req::GET_KEYBOARD).uint(13).fin();
        e.empezar(4, wl_shm::req::CREATE_POOL).uint(5).llave(70 + c as u32).int(w * h * 4).fin();
        e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(6).int(0).int(w).int(h).int(w * 4).uint(XRGB8888).fin();
        e.empezar(3, wl_compositor::req::CREATE_SURFACE).uint(7).fin();
        e.empezar(8, xdg_wm_base::req::GET_XDG_SURFACE).uint(9).uint(7).fin();
        e.empezar(9, xdg_surface::req::GET_TOPLEVEL).uint(10).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        let ev = mandar(escena, con, c, &e);
        let serie = ev.iter().find(|x| x.0 == 9).map(|x| u32::from_le_bytes(x.2[..4].try_into().unwrap())).unwrap();
        let mut e = Escritor::new();
        e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
        e.empezar(7, wl_surface::req::ATTACH).uint(6).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(escena, con, c, &e);
        let _ = xdg_toplevel::req::SET_TITLE;
        ev
    }

    /// Super+Arriba maximiza la ventana que tiene el teclado: a (0, 0), con un configure del tamaño de la pantalla y
    /// el estado "maximizada"; el cliente lo confirma. Super+Abajo la devuelve a su sitio con un configure de 0 × 0.
    /// Las flechas con Super no llegan a la ventana; sin Super, sí.
    #[test]
    fn maximizar_y_volver() {
        let mut escena = Escena::new(Mem::default(), 0);
        let (mut a, mut b) = (Conexion::new(), Conexion::new());
        abrir(&mut escena, &mut a, 1, 100, 100); // en (40, 40)
        abrir(&mut escena, &mut b, 2, 100, 100); // en (72, 72), con el teclado
        let mut pantalla = vec![0u32; 300 * 200];
        escena.componer(&mut pantalla, 300, 200, 300, 0, &mut |_, _, _| {});
        escena.pendientes.clear();
        assert_eq!(escena.tecla(0xE3, true), [(2, Entrada::Tecla { codigo: 125, pulsada: true }), (2, Entrada::Mods { pulsados: 64 })]);
        // El cursor está en (0, 0): ahora sobre la ventana, que se entera.
        let ev = escena.tecla(0x52, true);
        assert_eq!(
            ev,
            [(2, Entrada::Configura { xdg: 9, ancho: 300, alto: 200, maximizada: true }), (2, Entrada::Entra { superficie: 7, x: 0, y: 0 })]
        );
        assert!(escena.tecla(0x52, false).is_empty());
        assert_eq!(escena.clientes[&2].superficies[&7].sitio, Some((0, 0)));
        // Otra vez arriba: ya lo está.
        assert!(escena.tecla(0x52, true).is_empty());

        // Al cliente: configure de la ventana (300, 200, [maximizada, activa]) y el de la superficie con su número.
        escribir_entrada(&mut b, escena.clientes.get_mut(&2).unwrap(), ev[0].1, 0);
        let sal = eventos(&b.sacar());
        assert_eq!(sal.iter().map(|e| (e.0, e.1)).collect::<Vec<_>>(), [(10, xdg_toplevel::ev::CONFIGURE), (9, xdg_surface::ev::CONFIGURE)]);
        let conf = leer_args(&tablas::XDG_TOPLEVEL.events[0], &sal[0].2, &mut [].into_iter()).unwrap();
        assert_eq!(conf[..2], [Valor::Int(300), Valor::Int(200)]);
        let serie = u32::from_le_bytes(sal[1].2[..4].try_into().unwrap());
        let mut e = Escritor::new();
        e.empezar(9, xdg_surface::req::ACK_CONFIGURE).uint(serie).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut b, 2, &e);
        assert!(escena.clientes[&2].xdg[&9].pedidos.is_empty());

        // Super+Abajo: a su sitio y 0 × 0.
        let ev = escena.tecla(0x51, true);
        assert_eq!(ev, [(2, Entrada::Configura { xdg: 9, ancho: 0, alto: 0, maximizada: false }), (2, Entrada::Sale { superficie: 7 })]);
        assert_eq!(escena.clientes[&2].superficies[&7].sitio, Some((72, 72)));
        // Sin Super, la flecha es una tecla más.
        escena.tecla(0xE3, false);
        assert_eq!(escena.tecla(0x51, true), [(2, Entrada::Tecla { codigo: 108, pulsada: true })]);
    }

    /// Un cliente que destruye su búfer y pone otro más pequeño (como al volver de maximizada): se repinta lo que
    /// ocupaba el viejo, aunque ya no exista, para que no se quede en pantalla.
    #[test]
    fn bufer_mas_pequeno_repinta_lo_de_antes() {
        let mut escena = Escena::new(Mem::default(), 0);
        let mut a = Conexion::new();
        abrir(&mut escena, &mut a, 1, 100, 100); // en (40, 40)
        let mut pantalla = vec![0u32; 300 * 300];
        escena.componer(&mut pantalla, 300, 300, 300, 0, &mut |_, _, _| {});
        assert!(escena.danos.is_empty());
        let mut e = Escritor::new();
        e.empezar(6, wl_buffer::req::DESTROY).fin();
        e.empezar(5, wl_shm_pool::req::CREATE_BUFFER).uint(20).int(0).int(50).int(50).int(200).uint(XRGB8888).fin();
        e.empezar(7, wl_surface::req::ATTACH).uint(20).int(0).int(0).fin();
        e.empezar(7, wl_surface::req::COMMIT).fin();
        mandar(&mut escena, &mut a, 1, &e);
        assert!(escena.danos.contains(&Rect { x: 40, y: 40, w: 100, h: 100 }), "{:?}", escena.danos);
        // Compuesto, lo de fuera del búfer nuevo es fondo otra vez.
        escena.componer(&mut pantalla, 300, 300, 300, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[120 * 300 + 120], 0);
    }

    #[test]
    fn asiento_puntero_y_teclado() {
        let mut escena = Escena::new(Mem::default(), 0);
        let (mut a, mut b) = (Conexion::new(), Conexion::new());
        let ev = abrir(&mut escena, &mut a, 1, 100, 100); // en (40, 40)
        // Al enlazar el asiento: puntero y teclado, y su nombre; al pedir el teclado, el mapa (sin mapa, con llave).
        assert!(ev.iter().any(|e| (e.0, e.1, e.2.clone()) == (11, wl_seat::ev::CAPABILITIES, 3u32.to_le_bytes().to_vec())));
        assert!(ev.iter().any(|e| (e.0, e.1) == (11, wl_seat::ev::NAME)));
        // G2: el mapa de xkb (formato 1), con su tamaño, y el texto empieza como un mapa compilado y acaba en 0.
        let mapa = ev.iter().find(|e| (e.0, e.1) == (13, wl_keyboard::ev::KEYMAP)).expect("sin mapa");
        assert_eq!(mapa.2[..4], 1u32.to_le_bytes());
        assert_eq!(mapa.2[4..8], (crate::MAPA_TECLADO.len() as u32).to_le_bytes());
        assert!(crate::MAPA_TECLADO.starts_with(b"xkb_keymap {") && crate::MAPA_TECLADO.ends_with(b"\0"));
        abrir(&mut escena, &mut b, 2, 100, 100); // en (72, 72), encima
        let mut pantalla = vec![0u32; 300 * 300];
        escena.componer(&mut pantalla, 300, 300, 300, 0, &mut |_, _, _| {});

        // El cursor empieza en (0, 0): fuera de todo. Entra en a por (50, 50).
        assert_eq!(escena.raton(50, 50, 0), [(1, Entrada::Entra { superficie: 7, x: 10, y: 10 })]);
        assert_eq!(escena.raton(5, 0, 0), [(1, Entrada::Mueve { x: 15, y: 10 })]);
        // A (80, 80): b está encima.
        assert_eq!(
            escena.raton(25, 30, 0),
            [(1, Entrada::Sale { superficie: 7 }), (2, Entrada::Entra { superficie: 7, x: 8, y: 8 })]
        );
        // b se llevó el teclado al aparecer (la última ventana nueva), y a lo perdió.
        assert_eq!(escena.teclado, Some((2, 7)));
        assert!(escena.pendientes.contains(&(1, Entrada::TecladoSale { superficie: 7 })));
        assert!(escena.pendientes.contains(&(2, Entrada::TecladoEntra { superficie: 7 })));
        // Pulsar sobre b: ya tenía el teclado; solo le llega el botón.
        assert_eq!(escena.raton(0, 0, 1), [(2, Entrada::Boton { boton: 0x110, pulsado: true })]);
        escena.raton(0, 0, 0);
        // La a de un teclado HID (0x04) es la tecla 30; Mayúsculas, el modificador 1.
        assert_eq!(escena.tecla(0x04, true), [(2, Entrada::Tecla { codigo: 30, pulsada: true })]);
        assert_eq!(
            escena.tecla(0xE1, true),
            [(2, Entrada::Tecla { codigo: 42, pulsada: true }), (2, Entrada::Mods { pulsados: 1 })]
        );
        // Volver a a, donde no la tapa b, y pulsar: sube encima y se lleva el teclado.
        escena.raton(-35, -35, 0);
        let ev = escena.raton(0, 0, 1);
        assert_eq!(escena.orden.last(), Some(&(1, 7)));
        assert!(ev.contains(&(2, Entrada::TecladoSale { superficie: 7 })));
        assert!(ev.contains(&(1, Entrada::TecladoEntra { superficie: 7 })));
        // El cursor no sale de la pantalla.
        escena.raton(-1000, 5000, 0);
        assert_eq!(escena.cursor, (0, 299));
        // Y se ve: la punta de la flecha, negra, en su sitio.
        escena.componer(&mut pantalla, 300, 300, 300, 0, &mut |_, _, _| {});
        assert_eq!(pantalla[299 * 300], 0xff00_0000);

        // Cómo llega al cliente: enter y botón con su serie y frame (puntero de la versión 7).
        let cli = escena.clientes.get_mut(&1).unwrap();
        escribir_entrada(&mut a, cli, Entrada::Entra { superficie: 7, x: 3, y: 4 }, 100);
        escribir_entrada(&mut a, cli, Entrada::Tecla { codigo: 30, pulsada: true }, 101);
        let ev = eventos(&a.sacar());
        let ordenes: Vec<(u32, u16)> = ev.iter().map(|e| (e.0, e.1)).collect();
        assert_eq!(ordenes, [(12, wl_pointer::ev::ENTER), (12, wl_pointer::ev::FRAME), (13, wl_keyboard::ev::KEY)]);
        let enter = leer_args(&tablas::WL_POINTER.events[0], &ev[0].2, &mut [].into_iter()).unwrap();
        assert_eq!(enter[2], Valor::Fixed(3 * 256));
        let key = leer_args(&tablas::WL_KEYBOARD.events[3], &ev[2].2, &mut [].into_iter()).unwrap();
        assert_eq!(&key[1..], [Valor::Uint(101), Valor::Uint(30), Valor::Uint(1)]);

        // Si el cliente se va, la entrada deja de apuntarle.
        escena.adios(1);
        assert!(escena.teclado.is_none());
    }
}
