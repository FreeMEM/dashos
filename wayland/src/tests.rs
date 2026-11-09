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
            let v = m.entry(llave).or_insert_with(|| vec![0; 1 << 16]);
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
    }

    fn globales() -> [Global; 3] {
        [
            Global { nombre: 1, interfaz: &tablas::WL_COMPOSITOR, version: 6 },
            Global { nombre: 2, interfaz: &tablas::WL_SHM, version: 1 },
            Global { nombre: 3, interfaz: &tablas::XDG_WM_BASE, version: 6 },
        ]
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
    use crate::tablas::{wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base};

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

    #[test]
    fn asiento_puntero_y_teclado() {
        let mut escena = Escena::new(Mem::default(), 0);
        let (mut a, mut b) = (Conexion::new(), Conexion::new());
        let ev = abrir(&mut escena, &mut a, 1, 100, 100); // en (40, 40)
        // Al enlazar el asiento: puntero y teclado, y su nombre; al pedir el teclado, el mapa (sin mapa, con llave).
        assert!(ev.iter().any(|e| (e.0, e.1, e.2.clone()) == (11, wl_seat::ev::CAPABILITIES, 3u32.to_le_bytes().to_vec())));
        assert!(ev.iter().any(|e| (e.0, e.1) == (11, wl_seat::ev::NAME)));
        assert!(ev.iter().any(|e| (e.0, e.1) == (13, wl_keyboard::ev::KEYMAP)));
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
        let cli = &escena.clientes[&1];
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
