//! Un cliente de Wayland de prueba (W7), común a `ventana-a` y `ventana-b`: se conecta al compositor por el canal
//! "ventanas" (llave 1, marcada por `inicio`), pide una ventana con `xdg-shell`, la pinta en memoria compartida y la
//! anima 30 cuadros, cada uno cuando el compositor avisa con `frame`. Después se queda con la ventana abierta.

use alloc::vec::Vec;
use dashos::println;
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{
    self, wl_callback, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base,
};
use wayland::{Escritor, leer_args};

pub const VENTANAS: u32 = 1;
pub const VA: usize = 0x40_0000_0000;
pub const CUADROS: u32 = 30;

// Los objetos de este cliente.
pub const REGISTRO: u32 = 2;
pub const SYNC: u32 = 3;
pub const COMPOSITOR: u32 = 4;
pub const SHM: u32 = 5;
pub const XDG: u32 = 6;
pub const POOL: u32 = 7;
pub const BUFER: u32 = 8;
pub const SUPERFICIE: u32 = 9;
pub const XDG_SUPERFICIE: u32 = 10;
pub const VENTANA: u32 = 11;
pub const ASIENTO: u32 = 12;
pub const PUNTERO: u32 = 13;
pub const TECLADO: u32 = 14;
/// Los avisos de frame, uno por cuadro desde aquí.
pub const AVISO: u32 = 100;

pub struct Cliente {
    respuesta: u32,
    pub juntador: Juntador,
    /// S10: la marca de la puerta (0, sin puerta) y cuántos mensajes han llegado por ella sin atender.
    puerta: u64,
    pub timbres: u32,
}

/// Al acabar, se cierran su canal y las llaves que llegaron sin usar: si no, quien se conecta muchas veces (`guardian`,
/// una por pantalla y por pregunta) llena su tabla de llaves.
impl Drop for Cliente {
    fn drop(&mut self) {
        while let Some(l) = self.juntador.tomar_llave() {
            dashos::cerrar(l);
        }
        dashos::cerrar(self.respuesta);
    }
}

impl Cliente {
    /// Se conecta al compositor por el canal "ventanas" (llave 1).
    pub fn conectar() -> Option<Cliente> {
        Self::conectar_con_puerta(0).map(|(c, _)| c)
    }

    /// S10: como `conectar`, y además una "puerta": una copia del extremo por el que escribe el compositor, marcada con
    /// `marca` (que no sea 0), para que otro servicio (los avisos, con la barra) toque el timbre en el mismo canal que
    /// se espera. Un proceso solo espera en un canal. Lo que llega con esa marca no se mezcla con lo del compositor:
    /// se cuenta en `timbres`, y `evento_plazo` vuelve al llegar. La copia viaja una vez y solo sirve para escribir.
    pub fn conectar_con_puerta(marca: u64) -> Option<(Cliente, u32)> {
        let (mio, suyo) = dashos::canal_crear();
        let puerta = if marca != 0 { dashos::llave_duplicar(suyo, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, marca) } else { 0 };
        // Con el compositor atareado (al arrancar, con muchos clientes a la vez y una pantalla grande), su cola puede
        // estar llena: se reintenta hasta 5 s en vez de rendirse. Si no, se dice por qué y se sueltan las llaves.
        let mut intentos = 0;
        let r = loop {
            let e = dashos::canal_escribir(VENTANAS, &[TROZO_CONECTAR], &[suyo]);
            if e == 0 || e == dashos::E_CERRADO || intentos >= 5_000 {
                break e;
            }
            intentos += 1;
            dashos::dormir_us(1_000);
        };
        if r != 0 {
            dashos::println!("  [ventanas] no se puede conectar al compositor: {}", r);
            for l in [mio, suyo] {
                dashos::cerrar(l);
            }
            if puerta > 0 {
                dashos::cerrar(puerta as u32);
            }
            return None;
        }
        Some((Cliente { respuesta: mio, juntador: Juntador::new(), puerta: if puerta > 0 { marca } else { 0 }, timbres: 0 }, puerta.max(0) as u32))
    }

    /// Si lo leído con la marca `marca` llegó por la puerta: se cuenta y se cierran sus llaves.
    fn por_la_puerta(&mut self, marca: u64, llaves: &[u32]) -> bool {
        if self.puerta == 0 || marca != self.puerta {
            return false;
        }
        self.timbres = self.timbres.saturating_add(1);
        llaves.iter().for_each(|&l| { dashos::cerrar(l); });
        true
    }

    /// Los globales del compositor (nombre, interfaz, versión), con un sync.
    pub fn globales(&mut self) -> Option<Vec<(u32, alloc::string::String, u32)>> {
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        self.mandar(&e);
        let mut globales = Vec::new();
        loop {
            let (o, orden, cuerpo) = self.evento()?;
            if o == REGISTRO && orden == wl_registry::ev::GLOBAL {
                let a = leer_args(&tablas::WL_REGISTRY.events[0], &cuerpo, &mut core::iter::empty()).unwrap_or_default();
                if a.len() == 3 {
                    globales.push((a[0].uint(), alloc::string::String::from(a[1].texto()), a[2].uint()));
                }
            } else if o == SYNC && orden == wl_callback::ev::DONE {
                return Some(globales);
            }
        }
    }

    pub fn mandar(&mut self, e: &Escritor) {
        for t in canal::trocear(e).expect("mensajes propios bien formados") {
            // Con la cola llena, se espera un poco y se reintenta. Mientras, se recoge lo que mande el compositor: si
            // no, él puede estar esperando a que leamos para seguir leyendo su cola, y nosotros a que la lea para
            // escribir; al segundo nos suelta (E8). Así se quedaba sin pantalla de entrada `guardian`, 1 de cada 3
            // pasadas de la prueba con el PC cargado.
            for _ in 0..1000 {
                if dashos::canal_escribir(VENTANAS, &t.bytes, &t.llaves) == 0 {
                    break;
                }
                if !self.recoger() {
                    dormir();
                }
            }
        }
    }

    /// Lee un trozo que haya llegado, sin esperar, y lo guarda para `evento`. `false` si no había nada. Quien espera a
    /// otro servicio lo llama mientras tanto (la barra, a `sesiones`): así el compositor no le suelta por no leer.
    pub fn recoger(&mut self) -> bool {
        let mut buf = [0u8; canal::MAX_CANAL];
        let mut ll = [0u32; canal::MAX_LLAVES];
        let (n, k, marca) = dashos::canal_leer_plazo(self.respuesta, &mut buf, &mut ll, 1);
        if n < 0 {
            return false;
        }
        if self.por_la_puerta(marca, &ll[..k]) {
            return true;
        }
        if n == 0 {
            return false;
        }
        if buf[0] == TROZO_DATOS && self.juntador.meter(&buf[1..n as usize], &ll[..k]).is_ok() {
            return true;
        }
        ll[..k].iter().for_each(|&l| { dashos::cerrar(l); });
        true
    }

    /// Como `evento`, esperando como mucho `us` microsegundos: `Ok(None)` si no llegó nada (U3: `guardian` espera a la
    /// vez a `sesiones` sin dejar de leer al compositor, que suelta a quien no lee).
    pub fn evento_plazo(&mut self, us: u64) -> Result<Option<(u32, u16, Vec<u8>)>, ()> {
        loop {
            if let Ok(Some(m)) = self.juntador.siguiente() {
                return Ok(Some((m.objeto, m.orden, m.cuerpo)));
            }
            let mut buf = [0u8; canal::MAX_CANAL];
            let mut ll = [0u32; canal::MAX_LLAVES];
            let (n, k, marca) = dashos::canal_leer_plazo(self.respuesta, &mut buf, &mut ll, us.max(1));
            if n == dashos::E_PLAZO {
                return Ok(None);
            }
            if n >= 0 && self.por_la_puerta(marca, &ll[..k]) {
                return Ok(None);
            }
            if n <= 0 {
                dashos::println!("  [ventanas] no se pudo leer del compositor: {}", n);
                return Err(());
            }
            if buf[0] == TROZO_DATOS && self.juntador.meter(&buf[1..n as usize], &ll[..k]).is_err() {
                dashos::println!("  [ventanas] demasiado por juntar del compositor");
                return Err(());
            }
        }
    }

    /// El siguiente evento: objeto, orden y cuerpo.
    pub fn evento(&mut self) -> Option<(u32, u16, Vec<u8>)> {
        loop {
            if let Ok(Some(m)) = self.juntador.siguiente() {
                return Some((m.objeto, m.orden, m.cuerpo));
            }
            let mut buf = [0u8; canal::MAX_CANAL];
            let mut ll = [0u32; canal::MAX_LLAVES];
            let (n, k, marca) = dashos::canal_leer_marca(self.respuesta, &mut buf, &mut ll);
            if n >= 0 && self.por_la_puerta(marca, &ll[..k]) {
                continue;
            }
            if n <= 0 {
                return None;
            }
            if buf[0] == TROZO_DATOS {
                self.juntador.meter(&buf[1..n as usize], &ll[..k]).ok()?;
            }
        }
    }
}

/// Memoria para los búferes de una superficie, con su `wl_shm_pool`, proyectada en `va`. Para lo que se abre y se cierra
/// (el lanzador, un aviso): se pide al abrirlo y se suelta al cerrarlo, en vez de tenerla siempre (kit-memoria: la
/// barra tenía 2,1 MB para la barra a 3840 de ancho, el lanzador, el aviso y el historial, se vieran o no).
pub struct Lamina {
    llave: u32,
    va: usize,
    pub pool: u32,
    pub tamano: usize,
}

impl Lamina {
    /// Crea `tamano` bytes, los proyecta en `va` y pone en `e` el `create_pool` del objeto `pool` (el compositor lee con
    /// una copia de solo lectura de la llave).
    pub fn nueva(e: &mut Escritor, tamano: usize, va: usize, pool: u32) -> Option<Lamina> {
        let m = dashos::memoria_crear(tamano);
        if (m as i32) <= 0 {
            return None;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        if dashos::memoria_proyectar(m, va, true) != 0 || copia <= 0 {
            if copia > 0 {
                dashos::cerrar(copia as u32);
            }
            dashos::memoria_quitar(va);
            dashos::cerrar(m);
            return None;
        }
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(pool).llave(copia as u32).int(tamano as i32).fin();
        Some(Lamina { llave: m, va, pool, tamano })
    }

    /// Los primeros `n` puntos (como mucho, los que caben).
    pub fn px(&mut self, n: usize) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u32, n.min(self.tamano / 4)) }
    }

    /// Pone en `e` el `destroy` del pool y suelta la memoria: sus páginas vuelven cuando el compositor suelte también su
    /// copia (al destruirse el pool y sus búferes).
    pub fn soltar(self, e: &mut Escritor) {
        e.empezar(self.pool, wl_shm_pool::req::DESTROY).fin();
        dashos::memoria_quitar(self.va);
        dashos::cerrar(self.llave);
    }
}

pub fn dormir() {
    let (a, b) = dashos::canal_crear();
    let t = dashos::temporizador_crear(b, 1_000, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
}

/// Pinta el cuadro `n` de una ventana de `ancho` × `alto` en `px` con `pintar(x, y, n) -> ARGB`.
fn pintar(px: &mut [u32], ancho: u32, alto: u32, n: u32, pintar: &impl Fn(u32, u32, u32) -> u32) {
    for y in 0..alto {
        for x in 0..ancho {
            px[(y * ancho + x) as usize] = pintar(x, y, n);
        }
    }
}

/// Abre una ventana de `ancho` × `alto` con `formato` (de `wl_shm`) y la anima. Devuelve si todo fue como debía.
pub fn ventana(
    nombre: &'static str,
    titulo: &str,
    ancho: u32,
    alto: u32,
    formato: u32,
    color: impl Fn(u32, u32, u32) -> u32,
) -> bool {
    let Some(mut c) = Cliente::conectar() else {
        println!("  [{}] no se puede conectar", nombre);
        return false;
    };
    let Some(globales) = c.globales() else { return false };
    let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
    let (Some(gc), Some(gs), Some(gx), Some(ga)) =
        (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat"))
    else {
        println!("  [{}] faltan globales", nombre);
        return false;
    };

    // La memoria: se pinta aquí y el compositor la lee con una copia de solo lectura de la llave.
    let tamano = (ancho * alto * 4) as usize;
    let m = dashos::memoria_crear(tamano);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m as u32, VA, true) != 0 {
        println!("  [{}] sin memoria", nombre);
        return false;
    }
    let px = unsafe { core::slice::from_raw_parts_mut(VA as *mut u32, (ancho * alto) as usize) };
    let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::TRANSFERIR, 0);

    let mut e = Escritor::new();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
    e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
    e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
    e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOL).llave(copia as u32).int(tamano as i32).fin();
    e.empezar(POOL, wl_shm_pool::req::CREATE_BUFFER)
        .uint(BUFER)
        .int(0)
        .int(ancho as i32)
        .int(alto as i32)
        .int(ancho as i32 * 4)
        .uint(formato)
        .fin();
    e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
    e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
    e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(titulo)).fin();
    e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
    c.mandar(&e);

    // Un solo bucle: su configure y sus cuadros, el ping, y la entrada (W8).
    let mut cuadro = 0;
    let mut configurada = false;
    let (mut raton, mut tecla) = (false, false);
    loop {
        let Some((o, orden, cuerpo)) = c.evento() else { return cuadro >= CUADROS };
        let palabra = |i: usize| u32::from_le_bytes(cuerpo[4 * i..4 * i + 4].try_into().unwrap());
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                c.mandar(&e);
            }
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) if !configurada => {
                configurada = true;
                pintar(px, ancho, alto, 0, &color);
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFER).int(0).int(0).fin();
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho as i32).int(alto as i32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(AVISO).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            (aviso, wl_callback::ev::DONE) if cuadro < CUADROS && aviso == AVISO + cuadro => {
                cuadro += 1;
                if cuadro == CUADROS {
                    println!("  [{}] {} cuadros: bien", nombre, CUADROS);
                    continue;
                }
                pintar(px, ancho, alto, cuadro, &color);
                let mut e = Escritor::new();
                e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(ancho as i32).int(alto as i32).fin();
                e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(AVISO + cuadro).fin();
                e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
                c.mandar(&e);
            }
            // El mapa de teclas llega con una llave de memoria: sin mapa todavía, se suelta.
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = c.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER) if !raton => {
                raton = true;
                println!("  [ventana] ratón dentro ({})", nombre);
            }
            // La a (tecla 30 de evdev), pulsada.
            (TECLADO, wl_keyboard::ev::KEY) if !tecla && palabra(2) == 30 && palabra(3) == 1 => {
                tecla = true;
                println!("  [ventana] tecla 30 recibida ({})", nombre);
            }
            (1, wl_display::ev::ERROR) => {
                println!("  [{}] error de protocolo", nombre);
                return false;
            }
            _ => {}
        }
    }
}
