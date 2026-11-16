//! El Shell nativo de DashOS, al estilo del de AmigaOS (`docs/bitacoras/shell-nativa.md`). Está en `SYS:System/Shell`.
//!
//! Tres cosas en un programa: la ventana (una consola de líneas con desplazamiento, selección, portapapeles e
//! historial; `shell::consola`), el intérprete de órdenes y guiones (`shell::interprete`) y los trabajos (las órdenes
//! lanzadas, que le piden cosas por su puerta; `shell::protocolo`).
//!
//! `Shell [FROM guion] [REGISTRO] [SINARRANQUE] [ORDEN línea]`: de serie hace `S:Shell-Startup` y pide órdenes. Con
//! `FROM`, ese guion en vez del de arranque. Con `ORDEN`, hace esa línea y acaba con su código. Con `REGISTRO`, lo que
//! sale por la consola va también al registro del sistema (para las pruebas).
//!
//! Lanzado por otro Shell (`Run` de un guion o de una orden interna) no abre ventana: su consola es la del Shell que
//! lo lanzó.
//!
//! Llave 1: `sesiones`; 2: archivos; 3: el compositor; 4: el canal de arranque, si lo hay.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../../usuario/src/portapapeles_cliente.rs"]
mod portapapeles_cliente;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use dashos::asignaciones::{self as asig, Cambio};
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as ps;
use kit::lienzo::{Lienzo, Rect as RectLienzo};
use kit::menus::{Datos, Menus, Resultado as MenuResultado, FILA as FILA_MENUS};
use kit::teclado::{Tecla, Teclado};
use kit::tema::Tema;
use kit::texto::Fuente;
use ordenes::{self as orden, Cli, Sistema, SESIONES};
use shell::completar::{self, Completado};
use shell::consola::{self, Consola, Punto};
use shell::paneles::{Disposicion, Hacia, Rect as RectPanel};
use shell::resaltar::{self, Clase};
use shell::interprete::{Interprete, Mundo, Paso};
use shell::linea::Orden as OrdenLinea;
use shell::protocolo as p;
use shell::{indicador, perfil, plantilla, ruta, RC_AVISO, RC_BIEN, RC_ERROR, RC_FALLO};
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};
use wayland::tablas::{
    dashos_menu_v1, dashos_menus_v1, dashos_tema_v1, wl_buffer, wl_callback, wl_compositor, wl_display, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface, xdg_surface, xdg_toplevel, xdg_wm_base, zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use wayland::{leer_args, Escritor};

dashos::programa!(principal);
dashos::monton!();

const VENTANAS: u32 = 3;
/// Dónde se proyecta un programa mientras se lee del disco, y los píxeles de la ventana (dos sitios: al cambiar de
/// tamaño se pinta en el otro).
const VA_PROGRAMA: usize = 0x48_0000_0000;
const VA_PIXELES: [usize; 2] = [0x50_0000_0000, 0x54_0000_0000];
/// Lo más que mide un programa (lo que admite `sesiones`).
const MAX_PROGRAMA: usize = 4 << 20;
/// Los programas que se guardan ya leídos (un guion que llama cien veces a `Echo` no lo lee cien veces).
const MAX_GUARDADOS: usize = 8;
/// Lo más que se lee de un fichero de entrada (`<fichero`).
const MAX_ENTRADA: usize = 1 << 20;
/// Lo que cabe en un tubo entre dos órdenes: lleno, quien escribe espera a que la otra lea. Y lo más que se guarda de
/// una sustitución de órdenes.
const MAX_TUBO: usize = 64 * 1024;
const MAX_CAPTURA: usize = 64 * 1024;
/// Lo que se espera a una orden rota con Ctrl-C antes de matarla.
const PLAZO_ROTO_US: u64 = 2_000_000;
/// Los trabajos a la vez.
const MAX_TRABAJOS: usize = 16;
/// Lo que se espera a `sesiones`.
const PLAZO_SESIONES_US: u64 = 10_000_000;

// Los objetos de la ventana.
const REGISTRO: u32 = 2;
const SYNC: u32 = 3;
const COMPOSITOR: u32 = 4;
const SHM: u32 = 5;
const XDG: u32 = 6;
const SUPERFICIE: u32 = 9;
const XDG_SUPERFICIE: u32 = 10;
const VENTANA: u32 = 11;
const ASIENTO: u32 = 12;
const PUNTERO: u32 = 13;
const TECLADO: u32 = 14;
const POOLES: [u32; 2] = [20, 22];
const BUFERES: [u32; 2] = [21, 23];
const TEMA: u32 = 43;
/// La decoración del compositor (`zxdg_decoration_manager_v1`): su gestor y la de nuestra ventana.
const DECORACIONES: u32 = 44;
const DECORACION: u32 = 45;
/// El modo "la barra la pone el compositor" de `zxdg_toplevel_decoration_v1`.
const DECORACION_DEL_COMPOSITOR: u32 = 2;
/// El título de la ventana sin cajón (y lo que se le pone delante): el del programa.
const TITULO: &str = "Shell";
/// Entre el cajón y la orden, en el título de una pestaña y de la ventana.
const SEPARADOR_TITULO: &str = " · ";
/// S10: los menús de la ventana (`docs/bitacoras/menus.md`): el global del compositor (dónde van) y el menú de la
/// ventana. Los atajos los hace el Shell, como siempre; el menú solo los enseña.
const MENUS_G: u32 = 46;
const MENU_V: u32 = 47;
const SITIO_BARRA: u32 = 1;
const M_PESTANA: u32 = 1;
const M_VERTICAL: u32 = 2;
const M_HORIZONTAL: u32 = 3;
const M_CERRAR: u32 = 4;
const M_COPIAR: u32 = 10;
const M_PEGAR: u32 = 11;
const M_LIMPIAR: u32 = 12;
const KEY_F10: u32 = 68;

/// Los menús del Shell: "Shell" (pestañas y paneles) y "Editar". Copiar, solo con algo seleccionado.
fn menus_del_shell(copiar: bool) -> Datos {
    let t = format!(
        "menus 1\nM\tShell\nE\t{M_PESTANA}\tPestaña nueva\tCtrl+Mayús+T\nE\t{M_VERTICAL}\tPartir en vertical\tCtrl+Mayús+E\nE\t{M_HORIZONTAL}\tPartir en horizontal\tCtrl+Mayús+O\n-\nE\t{M_CERRAR}\tCerrar el panel\tCtrl+Mayús+W\nM\tEditar\nE\t{M_COPIAR}\tCopiar\tCtrl+Mayús+C{}\nE\t{M_PEGAR}\tPegar\tCtrl+Mayús+V\n-\nE\t{M_LIMPIAR}\tBorrar la pantalla\tCtrl+L\n",
        if copiar { "" } else { "\t-" }
    );
    Datos::leer(t.as_bytes()).unwrap_or_default()
}
const AVISO: u32 = 100;

/// El borde de cada panel, en píxeles, y las columnas de un Shell sin ventana que no sabe las de su consola.
const BORDE: i32 = 8;
const COLUMNAS: usize = 80;
/// Lo más que mide el título de una pestaña, en letras.
const ANCHO_PESTANA: usize = 28;
/// Los ficheros del Shell en `ENVARC:` (la capa del usuario es `Home:Prefs/Env-Archive/Shell`) y sus complementos.
const HISTORIAL: &str = "ENVARC:Shell/Historial";
const INDICADOR: &str = "ENVARC:Shell/Indicador";
const PERFIL: &str = "ENVARC:Shell/Perfil";
const COMPLEMENTOS: &str = "S:Shell-Complementos";
/// Los 16 colores de una terminal, para fondo oscuro y para fondo claro.
const PALETA_OSCURA: [u32; 16] = [
    0xff1d1b20, 0xffd0605e, 0xff7fb36d, 0xffd9b85c, 0xff6f9bd6, 0xffb48ad9, 0xff5fb8b8, 0xffcac4cf, 0xff6b6670, 0xfff2898a, 0xffa6d68f, 0xfff0d58a,
    0xff9cbdf0, 0xffd0b0f0, 0xff8ad8d8, 0xffffffff,
];
const PALETA_CLARA: [u32; 16] = [
    0xff1d1b20, 0xffb3261e, 0xff2e7d32, 0xff8a6d00, 0xff1a5fb4, 0xff7b3fa0, 0xff00796b, 0xff605d66, 0xff49454f, 0xffd32f2f, 0xff388e3c, 0xffa07800,
    0xff1976d2, 0xff9c27b0, 0xff00897b, 0xff1d1b20,
];

// ---------------------------------------------------------------------------------------------------------------
// El buzón: el único canal donde espera el Shell. Por él llegan los trozos del compositor (sin marca) y lo que piden
// las órdenes por su puerta (con la marca de su trabajo).

struct Buzon {
    lectura: u32,
    /// Una llave para escribir en el buzón, de la que salen las puertas de los trabajos.
    puerta: u32,
    juntador: Juntador,
    peticiones: VecDeque<(u64, Vec<u8>)>,
    /// Si el compositor cerró (o nunca hubo).
    cerrado: bool,
}

impl Buzon {
    fn nuevo() -> (Buzon, u32) {
        let (lectura, suyo) = dashos::canal_crear();
        let puerta = dashos::llave_duplicar(suyo, dashos::ESCRIBIR | dashos::TRANSFERIR, 0).max(0) as u32;
        (Buzon { lectura, puerta, juntador: Juntador::new(), peticiones: VecDeque::new(), cerrado: false }, suyo)
    }

    /// Lee un mensaje, esperando como mucho `plazo_us`. Falso si no llegó nada.
    fn recoger(&mut self, plazo_us: u64) -> bool {
        let mut buf = [0u8; canal::MAX_CANAL];
        let mut ll = [0u32; canal::MAX_LLAVES];
        let (n, k, marca) = dashos::canal_leer_plazo(self.lectura, &mut buf, &mut ll, plazo_us.max(1));
        if n < 0 {
            if n == dashos::E_CERRADO {
                self.cerrado = true;
            }
            return false;
        }
        let n = n as usize;
        if marca != 0 {
            // De un trabajo: sus llaves no hacen falta.
            ll[..k].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
            if self.peticiones.len() < 64 {
                self.peticiones.push_back((marca, Vec::from(&buf[..n])));
            }
            return true;
        }
        if n > 0 && buf[0] == TROZO_DATOS {
            if self.juntador.meter(&buf[1..n], &ll[..k]).is_err() {
                self.cerrado = true;
            }
        } else {
            ll[..k].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
        }
        true
    }

    fn evento(&mut self) -> Option<(u32, u16, Vec<u8>)> {
        match self.juntador.siguiente() {
            Ok(Some(m)) => Some((m.objeto, m.orden, m.cuerpo)),
            Ok(None) => None,
            Err(_) => {
                self.cerrado = true;
                None
            }
        }
    }

    /// Manda al compositor. Con su cola llena se espera, recogiendo mientras lo que llegue (si no, él espera a que
    /// leamos y nosotros a que lea).
    fn mandar(&mut self, e: &Escritor) {
        let Ok(trozos) = canal::trocear(e) else { return };
        for t in trozos {
            for _ in 0..2000 {
                match dashos::canal_escribir(VENTANAS, &t.bytes, &t.llaves) {
                    0 => break,
                    dashos::E_LIMITE => {
                        if !self.recoger(1_000) {
                            dashos::dormir_us(500);
                        }
                    }
                    _ => {
                        self.cerrado = true;
                        return;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// La ventana: el búfer, el tema, la letra y la entrada. Lo que se ve dentro (las consolas de los paneles) lo da el
// Shell al pintar.

struct Ventana {
    fuente: Fuente,
    /// El tamaño de la letra (del perfil) y la celda: ancho, alto y línea base.
    letra: u32,
    celda: (i32, i32, i32),
    tema: Tema,
    tema_visto: (u32, bool),
    colores: perfil::Colores,
    /// Cuánto se ve lo de detrás (del perfil, de 0 a 90): el fondo de la ventana lleva esa transparencia.
    translucidez: u32,
    /// El tamaño con que se abre, en letras (del perfil).
    de_serie: (usize, usize),
    /// El búfer en uso (0 o 1), su memoria y su tamaño en píxeles.
    cual: usize,
    mem: u32,
    medida: (i32, i32),
    pedida: (i32, i32),
    configurada: bool,
    /// El aviso de frame que se espera (0, ninguno) y el siguiente número.
    esperando: u32,
    siguiente_aviso: u32,
    esperando_desde: u64,
    sucia: bool,
    /// Lo que se pintó en cada fila de cada panel la última vez (para no repintar lo que no cambia), y en la barra de
    /// pestañas.
    pintadas: Vec<(u32, RectPanel, Vec<u64>)>,
    barra_pintada: u64,
    /// Hay que borrar la ventana entera (cambió el tamaño, el tema o el reparto de paneles).
    entera: bool,
    teclado: Teclado,
    control: u8,
    mayusculas: u8,
    alt: bool,
    raton: (i32, i32),
    pp: portapapeles_cliente::Portapapeles,
    con_portapapeles: bool,
    serie: u32,
    /// El título que tiene la ventana (el de la pestaña con el teclado).
    titulo: String,
    /// S10: los menús (con la letra de la consola), si van fuera (en la barra), si el compositor los conoce, lo último que
    /// se le dio, y si hay que pintar la fila otra vez. El botón izquierdo, para arrastrar en ellos.
    menus: Menus,
    menus_fuera: bool,
    con_menus: bool,
    menus_dicho: Vec<u8>,
    menus_sucios: bool,
    regular: Fuente,
    medium: Fuente,
    izquierdo: bool,
}

/// Lo que llega de la ventana para el Shell.
enum Entrada {
    Nada,
    /// Una tecla pulsada: lo que da el mapa de teclado (si da algo) y su código.
    Tecla { t: Option<Tecla>, codigo: u32 },
    Mueve,
    Boton { boton: u32, pulsado: bool, serie: u32 },
    Rueda(i32),
    /// Cambió el tamaño (o es la primera vez): hay que recolocar los paneles.
    Medida,
    Cerrar,
    /// S10: los menús cambian de sitio (hay que recolocar), o en la barra se eligió una entrada.
    Sitio,
    Menu(u32),
}

/// Lo que se pinta de un panel.
struct Vista<'a> {
    id: u32,
    rect: RectPanel,
    consola: &'a Consola,
    activo: bool,
}

/// El color de pantalla de un color de celda: el de serie, uno de la paleta (los 16 del tema, el cubo de 6×6×6 y los
/// 24 grises de xterm), uno directo, o el gris de la sugerencia.
fn color_de(c: consola::Color, de_serie: u32, fondo: u32, paleta: &[u32; 16]) -> u32 {
    match c >> 24 {
        1 => match c & 0xff {
            n @ 0..=15 => paleta[n as usize],
            n @ 16..=231 => {
                let nivel = |v: u32| if v == 0 { 0 } else { 55 + 40 * v };
                let k = n - 16;
                0xff00_0000 | nivel(k / 36) << 16 | nivel(k / 6 % 6) << 8 | nivel(k % 6)
            }
            n => {
                let g = 8 + 10 * (n - 232);
                0xff00_0000 | g << 16 | g << 8 | g
            }
        },
        2 => 0xff00_0000 | (c & 0xff_ffff),
        3 => kit::tema::mezcla(de_serie, fondo, 130),
        _ => de_serie,
    }
}

fn techo(x: f32) -> i32 {
    let i = x as i32;
    if (i as f32) < x { i + 1 } else { i }
}

impl Ventana {
    /// Conecta con el compositor y pide la ventana. `None` si no hay compositor.
    fn abrir(b: &mut Buzon, suyo: u32, perfil_: &perfil::Perfil) -> Option<Ventana> {
        let mut intentos = 0;
        loop {
            let e = dashos::canal_escribir(VENTANAS, &[TROZO_CONECTAR], &[suyo]);
            if e == 0 {
                break;
            }
            intentos += 1;
            if e != dashos::E_LIMITE || intentos > 5000 {
                println!("  [shell] no se puede conectar al compositor: {}", e);
                dashos::cerrar(suyo);
                return None;
            }
            dashos::dormir_us(1_000);
        }
        let mut e = Escritor::new();
        e.empezar(1, wl_display::req::GET_REGISTRY).uint(REGISTRO).fin();
        e.empezar(1, wl_display::req::SYNC).uint(SYNC).fin();
        b.mandar(&e);
        let mut globales: Vec<(u32, String, u32)> = Vec::new();
        let hasta = dashos::ahora_us() + 10_000_000;
        'globales: loop {
            while let Some((o, orden, cuerpo)) = b.evento() {
                if o == REGISTRO && orden == wl_registry::ev::GLOBAL {
                    let args = leer_args(&wayland::tablas::WL_REGISTRY.events[0], &cuerpo, &mut core::iter::empty()).unwrap_or_default();
                    if args.len() == 3 {
                        globales.push((args[0].uint(), String::from(args[1].texto()), args[2].uint()));
                    }
                } else if o == SYNC && orden == wl_callback::ev::DONE {
                    break 'globales;
                }
            }
            if b.cerrado || dashos::ahora_us() > hasta {
                println!("  [shell] el compositor no contesta");
                return None;
            }
            b.recoger(100_000);
        }
        let global = |n: &str| globales.iter().find(|g| g.1 == n).map(|g| g.0);
        let (Some(gc), Some(gs), Some(gx), Some(ga)) = (global("wl_compositor"), global("wl_shm"), global("xdg_wm_base"), global("wl_seat")) else {
            println!("  [shell] faltan globales del compositor");
            return None;
        };
        let mut fuente = Fuente::mono();
        let letra = perfil_.letra;
        let cw = techo(fuente.avance('M', letra)).max(1);
        let ch = letra as i32 + 3;
        let base = (ch * 3 + 3) / 4;
        let mut e = Escritor::new();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gc).texto(Some("wl_compositor")).uint(6).uint(COMPOSITOR).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gs).texto(Some("wl_shm")).uint(1).uint(SHM).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(gx).texto(Some("xdg_wm_base")).uint(6).uint(XDG).fin();
        e.empezar(REGISTRO, wl_registry::req::BIND).uint(ga).texto(Some("wl_seat")).uint(7).uint(ASIENTO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_KEYBOARD).uint(TECLADO).fin();
        e.empezar(ASIENTO, wl_seat::req::GET_POINTER).uint(PUNTERO).fin();
        if let Some(gt) = global("dashos_tema_v1") {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gt).texto(Some("dashos_tema_v1")).uint(1).uint(TEMA).fin();
        }
        // S10: dónde van los menús (si el compositor lo sabe; si no, dentro).
        let global_menus = global("dashos_menus_v1");
        if let Some(gm) = global_menus {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gm).texto(Some("dashos_menus_v1")).uint(1).uint(MENUS_G).fin();
        }
        let mut pp = portapapeles_cliente::Portapapeles::default();
        let con_portapapeles = pp.enlazar(&mut e, &globales, REGISTRO, ASIENTO);
        e.empezar(COMPOSITOR, wl_compositor::req::CREATE_SURFACE).uint(SUPERFICIE).fin();
        e.empezar(XDG, xdg_wm_base::req::GET_XDG_SURFACE).uint(XDG_SUPERFICIE).uint(SUPERFICIE).fin();
        e.empezar(XDG_SUPERFICIE, xdg_surface::req::GET_TOPLEVEL).uint(VENTANA).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(TITULO)).fin();
        // S10: el menú de la ventana, para el compositor (lo enseña en la barra si toca).
        let menus = Menus::new(menus_del_shell(false));
        let mut menus_dicho = Vec::new();
        if global_menus.is_some() {
            menus_dicho = menus.datos.escribir().unwrap_or_default();
            e.empezar(MENUS_G, dashos_menus_v1::req::MENU).uint(MENU_V).uint(SUPERFICIE).fin();
            e.empezar(MENU_V, dashos_menu_v1::req::PONER).array(&menus_dicho).fin();
        }
        // La barra de la ventana (título, maximizar, cerrar, moverla y cambiarle el tamaño) la pone el compositor, la
        // de todo el sistema (`wayland/src/decoracion.rs`): el kit no tiene una cabecera que compartir y el Shell no
        // dibuja la suya. Se pide antes del primer búfer, como manda el protocolo.
        if let Some(gd) = global("zxdg_decoration_manager_v1") {
            e.empezar(REGISTRO, wl_registry::req::BIND).uint(gd).texto(Some("zxdg_decoration_manager_v1")).uint(1).uint(DECORACIONES).fin();
            e.empezar(DECORACIONES, zxdg_decoration_manager_v1::req::GET_TOPLEVEL_DECORATION).uint(DECORACION).uint(VENTANA).fin();
            e.empezar(DECORACION, zxdg_toplevel_decoration_v1::req::SET_MODE).uint(DECORACION_DEL_COMPOSITOR).fin();
        }
        e.empezar(VENTANA, xdg_toplevel::req::SET_APP_ID).texto(Some("shell")).fin();
        e.empezar(VENTANA, xdg_toplevel::req::SET_MIN_SIZE).int(20 * cw + 2 * BORDE).int(4 * ch + 2 * BORDE).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        b.mandar(&e);
        let tema = match perfil_.colores {
            perfil::Colores::Claro => Tema::CLARO,
            _ => Tema::OSCURO,
        };
        Some(Ventana {
            fuente,
            letra,
            celda: (cw, ch, base),
            tema,
            tema_visto: (0xff67_50a4, true),
            colores: perfil_.colores,
            translucidez: perfil_.translucidez,
            de_serie: (perfil_.columnas, perfil_.filas),
            cual: 0,
            mem: 0,
            medida: (0, 0),
            pedida: (0, 0),
            configurada: false,
            esperando: 0,
            siguiente_aviso: AVISO,
            esperando_desde: 0,
            sucia: true,
            pintadas: Vec::new(),
            barra_pintada: 0,
            entera: true,
            teclado: Teclado::new(),
            control: 0,
            mayusculas: 0,
            alt: false,
            raton: (0, 0),
            pp,
            con_portapapeles,
            serie: 0,
            titulo: String::from(TITULO),
            menus,
            menus_fuera: false,
            con_menus: global_menus.is_some(),
            menus_dicho,
            menus_sucios: true,
            // Con la letra de la consola: las de Roboto ocupan 62 KB más en el disco, y el de pruebas va justo
            // (A15, "sujetar sin soltar", se queda sin sitio).
            regular: Fuente::mono(),
            medium: Fuente::mono(),
            izquierdo: false,
        })
    }

    /// S10: lo que ocupa la fila de menús arriba (nada con los menús en la barra).
    fn alto_menus(&self) -> i32 {
        if self.menus_fuera { 0 } else { FILA_MENUS }
    }

    /// S10: los títulos en la fila, y lo desplegado dentro de la ventana.
    fn colocar_menus(&mut self) {
        let (w, h) = self.medida;
        self.menus.colocar(RectLienzo::new(8, 0, w - 16, FILA_MENUS), RectLienzo::new(0, 0, w, h), &mut self.regular, &mut self.medium);
        self.menus_sucios = true;
        self.sucia = true;
    }

    /// S10: el menú al compositor, si cambió.
    fn mandar_menus(&mut self, b: &mut Buzon) {
        if !self.con_menus {
            return;
        }
        let datos = self.menus.datos.escribir().unwrap_or_default();
        if datos != self.menus_dicho {
            let mut e = Escritor::new();
            e.empezar(MENU_V, dashos_menu_v1::req::PONER).array(&datos).fin();
            b.mandar(&e);
            self.menus_dicho = datos;
        }
    }

    /// S10: dónde está cada título (para las pruebas: un clic ahí lo abre).
    fn decir_menus(&self) {
        if self.menus_fuera {
            return;
        }
        let sitios: Vec<String> = (0..self.menus.datos.menus.len())
            .filter_map(|k| self.menus.sitio_titulo(k).map(|r| format!("\"{}\" en ({}, {})", self.menus.datos.menus[k].titulo, r.x + r.w / 2, r.y + r.h / 2)))
            .collect();
        println!("  [shell] menús de la ventana: {}", sitios.join("; "));
    }

    /// Un tamaño nuevo: memoria y búfer nuevos (en el otro sitio).
    fn medir(&mut self, b: &mut Buzon, w: i32, h: i32) -> bool {
        let (cw, ch, _) = self.celda;
        let (w, h) = (w.clamp(20 * cw + 2 * BORDE, 3840), h.clamp(4 * ch + 2 * BORDE, 2160));
        if self.mem != 0 && (w, h) == self.medida {
            return true;
        }
        let tam = (w * h * 4) as usize;
        let otra = if self.mem == 0 { 0 } else { 1 - self.cual };
        let m = dashos::memoria_crear(tam);
        if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_PIXELES[otra], true) != 0 {
            if (m as i32) > 0 {
                dashos::cerrar(m);
            }
            println!("  [shell] sin memoria para una ventana de {}x{}", w, h);
            return false;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        let mut e = Escritor::new();
        if self.mem != 0 {
            e.empezar(BUFERES[self.cual], wl_buffer::req::DESTROY).fin();
            e.empezar(POOLES[self.cual], wl_shm_pool::req::DESTROY).fin();
        }
        e.empezar(SHM, wl_shm::req::CREATE_POOL).uint(POOLES[otra]).llave(copia as u32).int(tam as i32).fin();
        e.empezar(POOLES[otra], wl_shm_pool::req::CREATE_BUFFER).uint(BUFERES[otra]).int(0).int(w).int(h).int(w * 4).uint(if self.translucidez > 0 { wayland::shm::ARGB8888 } else { wayland::shm::XRGB8888 }).fin();
        b.mandar(&e);
        if self.mem != 0 {
            dashos::memoria_quitar(VA_PIXELES[self.cual]);
            dashos::cerrar(self.mem);
        }
        self.mem = m;
        self.cual = otra;
        self.medida = (w, h);
        self.entera = true;
        self.sucia = true;
        true
    }

    fn oscuro(&self) -> bool {
        let c = self.tema.superficie;
        ((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff) < 384
    }

    /// Lo que mide la barra de pestañas (solo está si hay más de una).
    fn alto_barra(&self, pestanas: usize) -> i32 {
        if pestanas > 1 { self.celda.1 + 8 } else { 0 }
    }

    /// Cuántas letras caben en un panel de ese rectángulo.
    fn letras_en(&self, r: RectPanel) -> (usize, usize) {
        let (cw, ch, _) = self.celda;
        ((((r.w - 2 * BORDE) / cw).max(1)) as usize, (((r.h - 2 * BORDE) / ch).max(1)) as usize)
    }

    /// La letra del panel de `r` que cae bajo el ratón: columna y fila.
    fn letra_bajo_el_raton(&self, r: RectPanel) -> (usize, usize) {
        let (cw, ch, _) = self.celda;
        ((((self.raton.0 - r.x - BORDE).max(0)) / cw) as usize, (((self.raton.1 - r.y - BORDE).max(0)) / ch) as usize)
    }

    /// Pinta las filas que hayan cambiado de cada panel (y la barra de pestañas) y las presenta. Como mucho una vez por
    /// cuadro. Dice si pintó algo.
    fn pintar(&mut self, b: &mut Buzon, vistas: &[Vista], pestanas: &[(String, bool)]) -> bool {
        if !self.configurada || self.mem == 0 || !self.sucia {
            return false;
        }
        // Si el aviso de frame no llega (la sesión está detrás), no se pinta: no hay quien lo vea. Pasado un rato se
        // pinta igual, por si se perdió.
        if self.esperando != 0 && dashos::ahora_us().saturating_sub(self.esperando_desde) < 1_000_000 {
            return false;
        }
        self.sucia = false;
        let (w, h) = self.medida;
        let (cw, ch, base) = self.celda;
        // SAFETY: la memoria del búfer en uso, proyectada en `medir` con ese tamaño; solo la toca este hilo.
        let px = unsafe { core::slice::from_raw_parts_mut(VA_PIXELES[self.cual] as *mut u32, (w * h) as usize) };
        let mut l = Lienzo::new(px, w, h, w as usize);
        // El fondo, con la transparencia del perfil (el lienzo guarda el color ya multiplicado por su opacidad).
        let opacidad = 255 - 255 * self.translucidez.min(90) / 100;
        let (fondo, letra) = ((self.tema.superficie & 0x00ff_ffff) | opacidad << 24, self.tema.sobre_superficie);
        let paleta = if self.oscuro() { &PALETA_OSCURA } else { &PALETA_CLARA };
        let mut cambio = false;
        if self.entera {
            // Todo: el color de las rayas entre paneles; cada panel borra luego lo suyo.
            self.entera = false;
            self.pintadas.clear();
            self.barra_pintada = 0;
            self.menus_sucios = true;
            l.borrar(RectLienzo::new(0, 0, w, h), self.tema.variante_contorno);
            cambio = true;
        }
        // S10: la fila de menús, arriba (con su raya debajo).
        let arriba = self.alto_menus();
        if arriba > 0 && core::mem::take(&mut self.menus_sucios) {
            l.borrar(RectLienzo::new(0, 0, w, arriba), (self.tema.superficie & 0x00ff_ffff) | opacidad << 24);
            l.borrar(RectLienzo::new(0, arriba - 1, w, 1), self.tema.variante_contorno);
            let t = self.tema;
            self.menus.dibujar_fila(&mut l, &t, &mut self.medium, (0, 0));
            cambio = true;
        }
        // La barra de pestañas.
        if pestanas.len() > 1 {
            let mut suma = 0xcbf2_9ce4_8422_2325u64 ^ fondo as u64;
            for (t, activa) in pestanas {
                for c in t.chars() {
                    suma = (suma ^ c as u64).wrapping_mul(0x100_0000_01b3);
                }
                suma = (suma ^ *activa as u64 ^ 0xff).wrapping_mul(0x100_0000_01b3);
            }
            if suma != self.barra_pintada {
                self.barra_pintada = suma;
                cambio = true;
                let alto = self.alto_barra(pestanas.len());
                l.borrar(RectLienzo::new(0, arriba, w, alto), self.tema.variante_superficie);
                let mut x = 4;
                for (k, (t, activa)) in pestanas.iter().enumerate() {
                    let texto: String = format!(" {} {} ", k + 1, t).chars().take(ANCHO_PESTANA).collect();
                    let ancho = texto.chars().count() as i32 * cw;
                    if x + ancho > w {
                        break;
                    }
                    let (detras, color) = if *activa { (self.tema.primario, self.tema.sobre_primario) } else { (self.tema.variante_superficie, self.tema.sobre_variante_superficie) };
                    l.borrar(RectLienzo::new(x, arriba + 2, ancho, alto - 4), detras);
                    self.fuente.dibujar(&mut l, x, arriba + 2 + base, &texto, self.letra, color);
                    x += ancho + 4;
                }
            }
        }
        self.pintadas.retain(|p| vistas.iter().any(|v| v.id == p.0));
        for v in vistas {
            let r = v.rect;
            if r.w <= 0 || r.h <= 0 {
                continue;
            }
            let filas = v.consola.filas();
            let k = match self.pintadas.iter().position(|p| p.0 == v.id) {
                Some(k) => k,
                None => {
                    self.pintadas.push((v.id, RectPanel::default(), Vec::new()));
                    self.pintadas.len() - 1
                }
            };
            if self.pintadas[k].1 != r || self.pintadas[k].2.len() != filas {
                self.pintadas[k] = (v.id, r, alloc::vec![u64::MAX; filas]);
                l.borrar(RectLienzo::new(r.x, r.y, r.w, r.h), fondo);
                cambio = true;
            }
            // El panel con el teclado lleva una raya a la izquierda (si hay más de uno).
            let marca = if v.activo && vistas.len() > 1 { self.tema.primario } else { fondo };
            let vista = v.consola.vista();
            for y in 0..filas {
                let fila = vista.get(y);
                // Lo que se ve en la fila, resumido en un número.
                let mut suma = 0xcbf2_9ce4_8422_2325u64 ^ fondo as u64 ^ (marca as u64) << 32;
                let mut mezclar = |x: u64| suma = (suma ^ x).wrapping_mul(0x100_0000_01b3);
                if let Some(f) = fila {
                    mezclar(if v.activo { f.cursor.map_or(u64::MAX, |c| c as u64) } else { u64::MAX - 1 });
                    for (x, c) in f.celdas.iter().enumerate() {
                        mezclar(c.c as u64 | (c.estilo as u64) << 32 | (v.consola.seleccionada(f.linea, f.desde + x) as u64) << 40);
                        mezclar(c.letra as u64 | (c.fondo as u64) << 32);
                    }
                }
                if self.pintadas[k].2[y] == suma {
                    continue;
                }
                self.pintadas[k].2[y] = suma;
                cambio = true;
                let py = r.y + BORDE + y as i32 * ch;
                l.borrar(RectLienzo::new(r.x, py, r.w, ch), fondo);
                l.borrar(RectLienzo::new(r.x, py, 2, ch), marca);
                let Some(f) = fila else { continue };
                for x in 0..v.consola.columnas() {
                    let c = f.celdas.get(x);
                    let px_ = r.x + BORDE + x as i32 * cw;
                    // Los colores de la celda: los suyos, o los del tema; en negrita, los ocho primeros pasan a los vivos.
                    let (mut color, mut detras, estilo) = match c {
                        Some(c) => {
                            let viva = |k: consola::Color| if c.estilo & consola::NEGRITA != 0 && (consola::paleta(0)..consola::paleta(8)).contains(&k) { k + 8 } else { k };
                            (color_de(viva(c.letra), letra, fondo, paleta), color_de(c.fondo, fondo, fondo, paleta), c.estilo)
                        }
                        None => (letra, fondo, 0),
                    };
                    // La letra que se decide con el fondo ya puesto (los bloques del indicador): clara u oscura, la que
                    // más contraste.
                    if c.is_some_and(|c| c.letra == consola::SOBRE_FONDO) {
                        color = consola::mejor_sobre(detras, self.tema.superficie | 0xff00_0000, self.tema.sobre_superficie | 0xff00_0000);
                    }
                    if estilo & consola::INVERSO != 0 {
                        core::mem::swap(&mut color, &mut detras);
                    }
                    if estilo & consola::TENUE != 0 {
                        color = kit::tema::mezcla(color, detras, 110);
                    }
                    if v.activo && f.cursor == Some(x) {
                        detras = self.tema.primario;
                        color = self.tema.sobre_primario;
                    } else if c.is_some() && v.consola.seleccionada(f.linea, f.desde + x) {
                        detras = self.tema.contenedor_primario;
                        color = self.tema.sobre_contenedor_primario;
                    }
                    if detras != fondo {
                        l.borrar(RectLienzo::new(px_, py, cw, ch), detras);
                    }
                    if let Some(c) = c.filter(|c| c.c != ' ') {
                        // Lo que la fuente no tiene sale como "?": así la caché de letras no crece sin fin.
                        let letra_ = if self.fuente.tiene(c.c) { c.c } else { '?' };
                        let mut buf = [0u8; 4];
                        self.fuente.dibujar(&mut l, px_, py + base, letra_.encode_utf8(&mut buf), self.letra, color);
                    }
                    if estilo & consola::SUBRAYADO != 0 && c.is_some() {
                        l.borrar(RectLienzo::new(px_, py + base + 2, cw, 1), color);
                    }
                }
            }
        }
        // S10: lo desplegado, encima de todo (al cerrarse se pinta la ventana entera otra vez).
        if self.menus.abierto() && !self.menus_fuera {
            let t = self.tema;
            self.menus.dibujar_abierto(&mut l, &t, &mut self.regular, &mut self.medium, (0, 0));
            cambio = true;
        }
        if !cambio {
            return false;
        }
        let mut e = Escritor::new();
        e.empezar(SUPERFICIE, wl_surface::req::ATTACH).uint(BUFERES[self.cual]).int(0).int(0).fin();
        e.empezar(SUPERFICIE, wl_surface::req::DAMAGE).int(0).int(0).int(w).int(h).fin();
        e.empezar(SUPERFICIE, wl_surface::req::FRAME).uint(self.siguiente_aviso).fin();
        e.empezar(SUPERFICIE, wl_surface::req::COMMIT).fin();
        self.esperando = self.siguiente_aviso;
        self.esperando_desde = dashos::ahora_us();
        self.siguiente_aviso = if self.siguiente_aviso >= 0x00ff_ffff { AVISO } else { self.siguiente_aviso + 1 };
        b.mandar(&e);
        true
    }

    /// Un evento del compositor. Lo que sea para el Shell, en la entrada.
    fn evento(&mut self, b: &mut Buzon, o: u32, orden: u16, cuerpo: &[u8]) -> Entrada {
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()));
        match (o, orden) {
            (XDG, xdg_wm_base::ev::PING) => {
                let mut e = Escritor::new();
                e.empezar(XDG, xdg_wm_base::req::PONG).uint(palabra(0)).fin();
                b.mandar(&e);
            }
            (VENTANA, xdg_toplevel::ev::CONFIGURE) => self.pedida = (palabra(0) as i32, palabra(1) as i32),
            (VENTANA, xdg_toplevel::ev::CLOSE) => return Entrada::Cerrar,
            (XDG_SUPERFICIE, xdg_surface::ev::CONFIGURE) => {
                let mut e = Escritor::new();
                e.empezar(XDG_SUPERFICIE, xdg_surface::req::ACK_CONFIGURE).uint(palabra(0)).fin();
                b.mandar(&e);
                let (cw, ch, _) = self.celda;
                let (w, h) = if self.pedida.0 > 0 && self.pedida.1 > 0 {
                    self.pedida
                } else if self.configurada {
                    self.medida
                } else {
                    // S10: con la fila de menús, más alta: las letras de serie caben igual.
                    (self.de_serie.0 as i32 * cw + 2 * BORDE, self.de_serie.1 as i32 * ch + 2 * BORDE + self.alto_menus())
                };
                if self.medir(b, w, h) {
                    self.configurada = true;
                    self.esperando = 0;
                    return Entrada::Medida;
                }
            }
            (aviso, wl_callback::ev::DONE) if aviso == self.esperando => self.esperando = 0,
            (TEMA, dashos_tema_v1::ev::ESQUEMA) => {
                kit::tema::recordar_evento_esquema(cuerpo);
            }
            (TEMA, dashos_tema_v1::ev::TEMA) => {
                let visto = (palabra(0) | 0xff00_0000, palabra(1) != 0);
                // Con colores propios en el perfil, el tema del sistema no cambia los de la ventana.
                if visto != self.tema_visto && self.colores == perfil::Colores::Sistema {
                    self.tema_visto = visto;
                    self.tema = Tema::del_sistema(visto.0, visto.1);
                    self.entera = true;
                    self.sucia = true;
                }
            }
            (TECLADO, wl_keyboard::ev::KEYMAP) => {
                if let Some(l) = b.juntador.tomar_llave() {
                    dashos::cerrar(l);
                }
            }
            (TECLADO, wl_keyboard::ev::LEAVE) => {
                self.control = 0;
                self.mayusculas = 0;
                self.alt = false;
            }
            (TECLADO, wl_keyboard::ev::KEY) => {
                self.serie = palabra(0);
                let (codigo, pulsada) = (palabra(2), palabra(3) == 1);
                match codigo {
                    29 | 97 => self.control = if pulsada { self.control.saturating_add(1) } else { self.control.saturating_sub(1) },
                    42 | 54 => self.mayusculas = if pulsada { self.mayusculas.saturating_add(1) } else { self.mayusculas.saturating_sub(1) },
                    56 => self.alt = pulsada,
                    _ => {}
                }
                let t = self.teclado.tecla(codigo, pulsada);
                if pulsada && !matches!(codigo, 29 | 97 | 42 | 54 | 56 | 100) {
                    self.sucia = true;
                    return Entrada::Tecla { t, codigo };
                }
            }
            (PUNTERO, wl_pointer::ev::ENTER) => self.raton = ((palabra(2) as i32) >> 8, (palabra(3) as i32) >> 8),
            (PUNTERO, wl_pointer::ev::MOTION) => {
                self.raton = ((palabra(1) as i32) >> 8, (palabra(2) as i32) >> 8);
                return Entrada::Mueve;
            }
            (PUNTERO, wl_pointer::ev::BUTTON) => {
                if palabra(2) == 272 {
                    self.izquierdo = palabra(3) == 1;
                }
                return Entrada::Boton { boton: palabra(2), pulsado: palabra(3) == 1, serie: palabra(0) };
            }
            // S10: dónde van los menús, y lo elegido en la barra.
            (MENUS_G, dashos_menus_v1::ev::SITIO) => {
                let fuera = palabra(0) == SITIO_BARRA;
                if fuera != self.menus_fuera {
                    self.menus_fuera = fuera;
                    self.menus.fuera = fuera;
                    self.menus.cerrar();
                    self.entera = true;
                    self.sucia = true;
                    // Si el compositor no le ha dado un tamaño (maximizada, en mosaico), la ventana crece o mengua lo
                    // de la fila: las letras no cambian.
                    if self.configurada && self.pedida == (0, 0) {
                        let (w, h) = self.medida;
                        let h = if fuera { h - FILA_MENUS } else { h + FILA_MENUS };
                        self.medir(b, w, h);
                    }
                    return Entrada::Sitio;
                }
            }
            (MENU_V, dashos_menu_v1::ev::ELEGIDO) => {
                let n = palabra(0);
                if self.menus.elegir_fuera(n) {
                    return Entrada::Menu(n);
                }
            }
            // La rueda (eje vertical).
            (PUNTERO, wl_pointer::ev::AXIS) if palabra(1) == 0 => return Entrada::Rueda(kit::controles::pasos_rueda(palabra(2))),
            (1, wl_display::ev::ERROR) => {
                println!("  [shell] error de protocolo en el objeto {}: {}", palabra(0), palabra(1));
                return Entrada::Cerrar;
            }
            (o, orden) => {
                let mut e = Escritor::new();
                if self.pp.evento(&mut e, o, orden, cuerpo, &mut || b.juntador.tomar_llave()) && !e.bytes.is_empty() {
                    b.mandar(&e);
                }
            }
        }
        Entrada::Nada
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Los trabajos.

/// A dónde va lo que escribe una orden.
enum Destino {
    Consola,
    Fichero { asa: u32, pos: u64 },
    /// Al tubo de ese número de la tubería: lo lee la orden siguiente.
    Tubo(usize),
    /// A la memoria: es una sustitución de órdenes.
    Captura,
    /// A ningún sitio (`>NIL:`, como en el Amiga).
    Nada,
}

/// De dónde lee una orden.
enum Fuente_ {
    Teclado,
    /// Un fichero ya leído: los bytes y por dónde va.
    Datos(Vec<u8>, usize),
    /// Del tubo de ese número: lo que escribe la orden anterior.
    Tubo(usize),
    Nada,
}

struct Trabajo {
    /// Su marca en el buzón.
    id: u64,
    /// Su número en `sesiones`, para matarlo.
    pid: u64,
    nombre: String,
    /// Nuestro extremo de su canal de arranque: por él se le contesta, y se cierra cuando muere.
    canal: u32,
    primer_plano: bool,
    /// Su número de trabajo (`Status`, `Break`), si es de fondo.
    numero: usize,
    /// Su sitio en la tubería en marcha (0, la primera orden), si es de ella.
    etapa: Option<usize>,
    salida: Destino,
    entrada: Fuente_,
    /// Si espera una línea (del teclado, o del tubo).
    leyendo: bool,
    /// Lo que queda de una línea tecleada que no cupo en una respuesta.
    resto: Vec<u8>,
    /// Lo que escribió y aún no cabe en su tubo: no se le contesta hasta que quepa (así espera a quien lee).
    por_escribir: Option<Vec<u8>>,
    /// Cuándo se le dijo "roto".
    roto: Option<u64>,
}

/// Un tubo entre dos órdenes de una tubería: lo escrito y no leído, y si alguno de los dos lados ya no está.
#[derive(Default)]
struct Tubo {
    datos: VecDeque<u8>,
    sin_escritor: bool,
    sin_lector: bool,
}

/// Una tubería en marcha (o una orden sola, que es una tubería de una): sus tubos, las órdenes que le quedan vivas,
/// el código de la última y si lo que escribe se guarda (una sustitución de órdenes).
struct Tuberia {
    tubos: Vec<Tubo>,
    etapas: usize,
    vivas: usize,
    rc: i32,
    r2: i32,
    captura: Option<Vec<u8>>,
}

/// Un programa ya leído del disco.
struct Guardado {
    raiz: u8,
    camino: String,
    tamano: u64,
    fecha: u64,
    mem: u32,
}

/// Lo que es una orden que no es interna.
enum Hallada {
    Programa { ruta: String },
    Guion { ruta: String },
}

/// El menú de completar: los candidatos, cuál está elegido y dónde acaba en la línea lo que se ha puesto.
struct Menu {
    c: Completado,
    elegido: Option<usize>,
    hasta: usize,
}

/// Un panel de la ventana: una sesión entera, con su consola, su intérprete, su cajón actual y sus trabajos.
struct Panel {
    id: u32,
    consola: Consola,
    int: Interprete,
    /// El cajón actual de este panel (el de `Sistema` es el del panel en que se trabaja).
    actual: String,
    trabajos: Vec<Trabajo>,
    tuberia: Option<Tuberia>,
    /// Los cajones de `Path`, además del actual y de `C:`.
    camino: Vec<String>,
    /// Las líneas tecleadas que esperan.
    tecleadas: VecDeque<String>,
    /// Los complementos que quedan por cargar, y el que se está cargando.
    complementos: VecDeque<String>,
    cargando: Option<String>,
    /// La línea de `ORDEN`, hasta que le toca.
    orden_pendiente: Option<String>,
    /// Con `ORDEN`: se acaba al terminarla.
    de_una_orden: bool,
    indicador_puesto: bool,
    /// El indicador puesto con `Prompt` (si no, el de los ajustes).
    indicador_propio: Option<String>,
    /// Las plantillas que dan los complementos (`Completa`), para completar y resaltar.
    plantillas: Vec<(String, String)>,
    salir: Option<i32>,
    menu: Option<Menu>,
    /// La versión de lo tecleado que ya está resaltada, y si quedó algo por mirar.
    resaltada: u64,
    por_mirar: bool,
    /// Dónde empezó a arrastrarse el ratón.
    arrastre: Option<Punto>,
    rect: RectPanel,
    /// La última orden tecleada (su nombre), en marcha o acabada: va en el título de la pestaña, tras el cajón.
    ultima: String,
}

impl Panel {
    fn nuevo(id: u32, actual: &str, columnas: usize, filas: usize) -> Panel {
        Panel {
            id,
            consola: Consola::nueva(columnas, filas),
            int: Interprete::nuevo(),
            actual: String::from(actual),
            trabajos: Vec::new(),
            tuberia: None,
            camino: Vec::new(),
            tecleadas: VecDeque::new(),
            complementos: VecDeque::new(),
            cargando: None,
            orden_pendiente: None,
            de_una_orden: false,
            indicador_puesto: false,
            indicador_propio: None,
            plantillas: Vec::new(),
            salir: None,
            menu: None,
            resaltada: u64::MAX,
            por_mirar: false,
            arrastre: None,
            rect: RectPanel::default(),
            ultima: String::new(),
        }
    }
}

struct Shell {
    s: Sistema,
    buzon: Buzon,
    ventana: Option<Ventana>,
    /// El Shell que nos lanzó, si nuestra consola es la suya.
    padre: Option<Cli>,
    registro: bool,
    registro_callado: bool,
    /// La línea a medias que va al registro.
    para_registro: String,
    /// El panel en que se trabaja (el número `en` de `paneles`, donde queda un hueco mientras tanto) y los demás.
    p: Panel,
    paneles: Vec<Panel>,
    en: usize,
    disp: Disposicion,
    siguiente_panel: u32,
    siguiente_id: u64,
    guardados: Vec<Guardado>,
    /// Los ajustes del indicador (`ENVARC:Shell/Indicador`).
    indicador: indicador::Ajustes,
    /// Lo mirado en el disco para resaltar: qué, si existe y cuándo se miró.
    vistos: Vec<(String, bool, u64)>,
    /// Cuántas cosas nuevas se pueden mirar aún en esta pasada del resaltado.
    cupo_de_miradas: usize,
    /// Cómo arranca un panel nuevo: sin guion de arranque, o con otro.
    sin_arranque: bool,
    desde: Option<String>,
    /// El código con que acaba el Shell entero.
    fin: Option<i32>,
    vigilado_us: u64,
    pulso_us: u64,
    columnas: usize,
    /// Lo que se va midiendo (`Medidas`): desde cuándo, los bytes escritos en la consola y lo que costó ponerlos, y
    /// los cuadros pintados y lo que costó pintarlos (µs).
    medido_desde: u64,
    medido_bytes: u64,
    medido_escribir: u64,
    medido_cuadros: u64,
    medido_pintar: u64,
    /// Las teclas y los eventos del ratón que han llegado del compositor (para saber, en una prueba, si llegan).
    teclas_reales: u32,
    raton_real: u32,
}

/// El intérprete necesita el mundo mientras el Shell lo tiene prestado: esto le da lo justo.
struct MundoShell<'a> {
    shell: &'a mut Shell,
}

impl Mundo for MundoShell<'_> {
    fn decir(&mut self, texto: &str) {
        self.shell.di_linea(texto);
    }

    fn leer_guion(&mut self, ruta: &str) -> Result<String, i32> {
        // Como una orden: tal cual, o en `S:` si no lleva ruta.
        let d = match self.shell.s.leer_entero(ruta, orden::MAX_ENTERO) {
            Err(a::NO_EXISTE) if !ruta.contains([':', '/']) => self.shell.s.leer_entero(&format!("S:{}", ruta), orden::MAX_ENTERO),
            x => x,
        }?;
        Ok(String::from_utf8_lossy(&d).into_owned())
    }

    fn existe(&mut self, ruta: &str) -> bool {
        self.shell.s.mirar(ruta).is_ok()
    }

    fn env(&mut self, nombre: &str) -> Option<String> {
        self.shell.env(nombre)
    }
}

/// Un argumento que es todo él una cadena entre comillas, sin ellas (para pasar a una orden interna una línea con
/// redirecciones, tuberías o comillas sin que el Shell las atienda); si no, tal cual.
fn entre_comillas(texto: &str) -> String {
    shell::plantilla::trozos(texto).ok().filter(|t| t.len() == 1 && t[0].comillas).map(|t| t[0].texto.clone()).unwrap_or_else(|| String::from(texto))
}

fn nombre_de_variable(n: &str) -> bool {
    !n.is_empty() && n.len() <= 30 && n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
}

impl Shell {
    // --- La consola ---

    /// Escribe en la consola (la ventana, o la del Shell que nos lanzó) y, con `REGISTRO`, en el registro del sistema.
    fn di(&mut self, texto: &str) {
        if texto.is_empty() {
            return;
        }
        if self.registro && !self.registro_callado {
            for c in texto.chars() {
                if c == '\n' {
                    println!("  [shell] {}", self.para_registro);
                    self.para_registro.clear();
                } else if self.para_registro.len() < 200 && (c as u32) >= 0x20 {
                    self.para_registro.push(c);
                }
            }
        }
        if let Some(v) = &mut self.ventana {
            let t0 = dashos::ahora_us();
            self.p.consola.escribir(texto);
            self.medido_escribir += dashos::ahora_us() - t0;
            self.medido_bytes += texto.len() as u64;
            v.sucia = true;
        } else if let Some(c) = &mut self.padre {
            let mut resto = texto;
            while !resto.is_empty() {
                let mut n = resto.len().min(p::POR_ESCRITURA);
                while !resto.is_char_boundary(n) {
                    n -= 1;
                }
                let mut m = [0u8; 4 + p::POR_ESCRITURA];
                m[0..4].copy_from_slice(&p::SALIDA.to_le_bytes());
                m[4..4 + n].copy_from_slice(&resto.as_bytes()[..n]);
                // Sin plazo: si va a un tubo lleno, se espera a que la otra orden lea.
                if c.pedir(p::ESCRIBIR, &m[..4 + n], 0).is_none() {
                    // Quien nos lanzó ya no está.
                    self.p.salir = Some(RC_FALLO);
                    return;
                }
                resto = &resto[n..];
            }
        } else if !self.registro {
            dashos::escribir(texto);
        }
    }

    /// Una línea entera, empezando en una línea nueva si la de ahora está a medias.
    fn di_linea(&mut self, texto: &str) {
        if self.ventana.is_some() && !self.p.consola.al_principio() {
            self.di("\n");
        }
        self.di(texto);
        self.di("\n");
    }

    fn env(&mut self, nombre: &str) -> Option<String> {
        if !nombre_de_variable(nombre) {
            return None;
        }
        let d = self.s.leer_entero(&format!("ENV:{}", nombre), 4096).ok()?;
        Some(String::from_utf8_lossy(&d).trim_end_matches(['\n', '\r']).to_string())
    }

    /// Hace algo con el intérprete, dándole el Shell como su mundo.
    fn con_interprete<R>(&mut self, f: impl FnOnce(&mut Interprete, &mut MundoShell) -> R) -> R {
        let mut int = core::mem::take(&mut self.p.int);
        let r = f(&mut int, &mut MundoShell { shell: self });
        self.p.int = int;
        r
    }

    // --- Las órdenes ---

    /// Dónde está la orden `nombre`: con ruta, solo ahí; sin ella, en el cajón actual, en los de `Path` y en `C:`.
    fn buscar(&mut self, nombre: &str) -> Result<Hallada, i32> {
        let mut sitios: Vec<String> = Vec::new();
        if nombre.contains([':', '/']) {
            sitios.push(String::from(nombre));
        } else {
            sitios.push(String::from(nombre));
            sitios.extend(self.p.camino.iter().map(|c| ruta::unir(c, nombre)));
            sitios.push(format!("C:{}", nombre));
            // Las órdenes de base van juntas en `C:Ordenes`, que hace la que se le diga: un programa suelto con ese
            // nombre en el camino va antes.
            if ordenes::base::es_de_base(nombre) {
                sitios.push(String::from("C:Ordenes"));
            }
        }
        let mut error = a::NO_EXISTE;
        for sitio in sitios {
            let r = match self.s.ruta(&sitio) {
                Ok(r) => r,
                Err(e) => {
                    error = e;
                    continue;
                }
            };
            match self.s.mirar(&r) {
                Ok(d) if d.cajon() => error = a::ES_CAJON,
                // Un guion (el bit "s" del Amiga) o un programa (con permiso de ejecutar).
                Ok(d) if d.bits & 4 != 0 => return Ok(Hallada::Guion { ruta: r }),
                Ok(d) if d.modo & 0o111 != 0 => return Ok(Hallada::Programa { ruta: r }),
                Ok(_) => error = a::PROTEGIDO,
                Err(e) if e != a::NO_EXISTE && error == a::NO_EXISTE => error = e,
                Err(_) => {}
            }
        }
        Err(error)
    }

    /// El programa de `ruta`, leído a memoria (o de los guardados): una llave de su memoria para `sesiones` (solo de
    /// leer), lo que mide, y dónde estaba (la raíz y el cajón, para su `PROGDIR:`).
    fn cargar(&mut self, ruta_: &str) -> Result<(u32, usize, u8, String), i32> {
        let (raiz, camino, d) = self.s.donde(ruta_)?;
        let cajon = camino.rfind('/').map_or("", |i| &camino[..i]).to_string();
        let copia = |mem: u32| -> Result<u32, i32> {
            let c = dashos::llave_duplicar(mem, dashos::LEER | dashos::TRANSFERIR, 0);
            if c > 0 { Ok(c as u32) } else { Err(a::SIN_SITIO) }
        };
        if let Some(g) = self.guardados.iter().find(|g| g.raiz == raiz && g.camino == camino && g.tamano == d.tamano && g.fecha == d.fecha) {
            return Ok((copia(g.mem)?, d.tamano as usize, raiz, cajon));
        }
        let tam = d.tamano as usize;
        if tam < 64 || tam > MAX_PROGRAMA {
            return Err(a::MAL_FORMADO);
        }
        let x = self.s.fs.orden(a::BUSCAR, raiz as u32, 0, 0, camino.as_bytes());
        if x.e != a::BIEN {
            return Err(x.e);
        }
        let mem = dashos::memoria_crear(tam);
        if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_PROGRAMA, true) != 0 {
            if (mem as i32) > 0 {
                dashos::cerrar(mem);
            }
            self.s.cerrar(x.asa);
            return Err(a::SIN_SITIO);
        }
        // SAFETY: recién proyectada aquí, `tam` bytes; se quita antes de salir.
        let destino = unsafe { core::slice::from_raw_parts_mut(VA_PROGRAMA as *mut u8, tam) };
        let mut hecho = 0;
        let mut r = Ok(());
        while hecho < tam {
            match self.s.leer(x.asa, hecho as u64, &mut destino[hecho..]) {
                Ok(0) => {
                    r = Err(a::ERROR_DEL_DISCO);
                    break;
                }
                Ok(n) => hecho += n,
                Err(e) => {
                    r = Err(e);
                    break;
                }
            }
        }
        // Un programa de DashOS es un ELF; lo demás no se manda a `sesiones`.
        if r.is_ok() && &destino[..4] != b"\x7fELF" {
            r = Err(a::MAL_FORMADO);
        }
        dashos::memoria_quitar(VA_PROGRAMA);
        self.s.cerrar(x.asa);
        if let Err(e) = r {
            dashos::cerrar(mem);
            return Err(e);
        }
        if self.guardados.len() >= MAX_GUARDADOS {
            let viejo = self.guardados.remove(0);
            dashos::cerrar(viejo.mem);
        }
        self.guardados.push(Guardado { raiz, camino, tamano: d.tamano, fecha: d.fecha, mem });
        Ok((copia(mem)?, tam, raiz, cajon))
    }

    /// Lanza el programa de `ruta` con `args`, como un ayudante con la ficha del Shell. El trabajo queda apuntado.
    fn lanzar(&mut self, ruta_: &str, nombre: &str, args: &str, primer_plano: bool, entrada: Fuente_, salida: Destino, servido: bool) -> Result<u64, i32> {
        if self.p.trabajos.len() >= MAX_TRABAJOS {
            return Err(a::SIN_SITIO);
        }
        if self.s.actual.len() + nombre.len() + args.len() > p::MAX_ARRANQUE {
            return Err(asig::NO_CABE);
        }
        let (mem, tam, raiz, cajon) = self.cargar(ruta_)?;
        let corto = ruta::ultimo(ruta_);
        let id = self.siguiente_id;
        self.siguiente_id += 1;
        // El canal de arranque, con todo ya en su cola: el cajón actual, los argumentos y, si se le atiende, la puerta.
        let (nuestro, suyo) = dashos::canal_crear();
        let puerta = if servido { dashos::llave_duplicar(self.buzon.puerta, dashos::ESCRIBIR | dashos::TRANSFERIR | dashos::UNA_VEZ, id) } else { 0 };
        let cerrar_todo = |e: i32| -> i32 {
            for l in [mem, nuestro, suyo] {
                dashos::cerrar(l);
            }
            e
        };
        if servido && puerta <= 0 {
            return Err(cerrar_todo(a::SIN_SITIO));
        }
        let mut m = [0u8; 256];
        let interactiva = primer_plano && matches!(entrada, Fuente_::Teclado);
        // Con colores solo si lo que escribe llega a una consola de verdad (la de la ventana).
        let a_consola = matches!(salida, Destino::Consola) && self.ventana.is_some();
        let marcas = if servido { p::SERVIDO } else { 0 } | if interactiva { 0 } else { p::ENTRADA_NO_INTERACTIVA } | if a_consola { 0 } else { p::SALIDA_NO_CONSOLA };
        m[0..4].copy_from_slice(&p::ARRANQUE.to_le_bytes());
        m[4..8].copy_from_slice(&marcas.to_le_bytes());
        m[8..12].copy_from_slice(&(self.s.actual.len() as u32).to_le_bytes());
        m[12..16].copy_from_slice(&(args.len() as u32).to_le_bytes());
        m[16..18].copy_from_slice(&(self.columnas.min(u16::MAX as usize) as u16).to_le_bytes());
        m[20..24].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        let llaves = [puerta as u32];
        if dashos::canal_escribir(nuestro, &m[..p::LARGO_ARRANQUE], if servido { &llaves } else { &[] }) != 0 {
            if puerta > 0 {
                dashos::cerrar(puerta as u32);
            }
            return Err(cerrar_todo(a::SIN_SITIO));
        }
        let mut datos = Vec::from(self.s.actual.as_bytes());
        datos.extend_from_slice(nombre.as_bytes());
        datos.extend_from_slice(args.as_bytes());
        for trozo in datos.chunks(p::POR_DATOS) {
            m[0..4].copy_from_slice(&p::DATOS.to_le_bytes());
            m[4..4 + trozo.len()].copy_from_slice(trozo);
            if dashos::canal_escribir(nuestro, &m[..4 + trozo.len()], &[]) != 0 {
                return Err(cerrar_todo(asig::NO_CABE));
            }
        }
        // A `sesiones`: el ELF en memoria, de dónde salió y su canal de arranque.
        // Un cajón demasiado hondo no cabe en la petición: el programa se lanza igual, sin `PROGDIR:`.
        let (raiz, cajon) = if cajon.len() <= ps::MAX_CAJON_AYUDANTE { (raiz, cajon.as_str()) } else { (ps::SIN_RAIZ, "") };
        let mut pet = Vec::with_capacity(12 + corto.len() + cajon.len());
        pet.extend_from_slice(&ps::LANZAR_AYUDANTE.to_le_bytes());
        pet.extend_from_slice(&(tam as u32).to_le_bytes());
        pet.push(raiz);
        pet.push(corto.len().min(255) as u8);
        pet.extend_from_slice(&(cajon.len() as u16).to_le_bytes());
        pet.extend_from_slice(corto.as_bytes());
        pet.extend_from_slice(cajon.as_bytes());
        if pet.len() > 250 {
            return Err(cerrar_todo(asig::NO_CABE));
        }
        let (lectura, respuesta) = dashos::canal_crear();
        let mut r = [0u8; 12];
        let n = if dashos::canal_escribir(SESIONES, &pet, &[respuesta, mem, suyo]) == 0 {
            dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_SESIONES_US).0
        } else {
            for l in [respuesta, mem, suyo] {
                dashos::cerrar(l);
            }
            -1
        };
        dashos::cerrar(lectura);
        let estado = if n == 12 { i32::from_le_bytes(r[0..4].try_into().unwrap()) } else { a::ERROR_DEL_DISCO };
        if estado != 0 {
            dashos::cerrar(nuestro);
            return Err(estado);
        }
        let pid = u64::from_le_bytes(r[4..12].try_into().unwrap());
        let numero = if primer_plano { 0 } else { (1..).find(|n| !self.p.trabajos.iter().any(|t| t.numero == *n)).unwrap_or(1) };
        self.p.trabajos.push(Trabajo { id, pid, nombre: String::from(nombre), canal: nuestro, primer_plano, numero, etapa: None, salida, entrada, leyendo: false, resto: Vec::new(), por_escribir: None, roto: None });
        Ok(pid)
    }

    /// Lleva lo que escribe una orden a su destino.
    fn a_destino(&mut self, destino: &mut Destino, datos: &[u8]) -> i32 {
        match destino {
            Destino::Consola => {
                self.di(&String::from_utf8_lossy(datos));
                0
            }
            Destino::Fichero { asa, pos } => match self.s.escribir(*asa, *pos, datos) {
                Ok(()) => {
                    *pos += datos.len() as u64;
                    0
                }
                Err(e) => e,
            },
            Destino::Tubo(k) => {
                // Lo de una orden interna (lo de un programa pasa por `escribir_en_tubo`, que lo hace esperar).
                let Some(t) = self.p.tuberia.as_mut().and_then(|t| t.tubos.get_mut(*k)) else { return 0 };
                if t.sin_lector {
                    return p::SIN_LECTOR;
                }
                t.datos.extend(datos.iter().copied());
                0
            }
            Destino::Nada => 0,
            Destino::Captura => {
                let Some(c) = self.p.tuberia.as_mut().and_then(|t| t.captura.as_mut()) else { return 0 };
                if c.len() + datos.len() > MAX_CAPTURA {
                    return a::LLENO;
                }
                c.extend_from_slice(datos);
                0
            }
        }
    }

    fn cerrar_destino(&mut self, destino: Destino) {
        if let Destino::Fichero { asa, .. } = destino {
            self.s.cerrar(asa);
        }
    }

    /// Abre la entrada y la salida de una orden según sus redirecciones. Si no se puede, lo dice.
    fn abrir_redirecciones(&mut self, o: &OrdenLinea, entrada_de_serie: Fuente_, salida_de_serie: Destino) -> Result<(Fuente_, Destino), i32> {
        let entrada = match &o.entrada {
            // `NIL:` no es un fichero: de ahí no se lee nada, y lo que se escribe se pierde.
            Some(f) if f.eq_ignore_ascii_case("NIL:") => Fuente_::Nada,
            Some(f) => match self.s.leer_entero(f, MAX_ENTRADA) {
                Ok(d) => Fuente_::Datos(d, 0),
                Err(e) => {
                    self.di_linea(&format!("No se puede leer de {}: {}", f, orden::texto_error(e)));
                    self.cerrar_destino(salida_de_serie);
                    return Err(e);
                }
            },
            None => entrada_de_serie,
        };
        let salida = match &o.salida {
            Some(f) if f.eq_ignore_ascii_case("NIL:") => {
                self.cerrar_destino(salida_de_serie);
                Destino::Nada
            }
            Some(f) => match self.s.abrir_para_escribir(f, o.anadir) {
                Ok((asa, pos)) => {
                    self.cerrar_destino(salida_de_serie);
                    Destino::Fichero { asa, pos }
                }
                Err(e) => {
                    self.di_linea(&format!("No se puede escribir en {}: {}", f, orden::texto_error(e)));
                    self.cerrar_destino(salida_de_serie);
                    return Err(e);
                }
            },
            None => salida_de_serie,
        };
        Ok((entrada, salida))
    }

    /// Empieza una tubería: lanza todas sus órdenes a la vez, unidas por sus tubos. Con `captura`, lo que escriba la
    /// última se guarda (una sustitución de órdenes).
    fn empezar_tuberia(&mut self, ordenes: Vec<OrdenLinea>, captura: bool) {
        let n = ordenes.len();
        self.p.tuberia = Some(Tuberia { tubos: (1..n).map(|_| Tubo::default()).collect(), etapas: n, vivas: 0, rc: 0, r2: 0, captura: captura.then(Vec::new) });
        for (i, o) in ordenes.iter().enumerate() {
            let entrada = if i == 0 { Fuente_::Teclado } else { Fuente_::Tubo(i - 1) };
            let salida = if i + 1 < n {
                Destino::Tubo(i)
            } else if captura {
                Destino::Captura
            } else {
                Destino::Consola
            };
            match self.una_orden(o, entrada, salida, n == 1 && !captura, i) {
                // Lanzada: sigue viva. O era un guion, y el intérprete ya está en él (y no hay tubería).
                None => {
                    let Some(t) = &mut self.p.tuberia else { return };
                    t.vivas += 1;
                }
                // Ya hecha (interna, o no se pudo lanzar): sus dos lados de los tubos se cierran.
                Some((rc, r2)) => self.etapa_acabada(i, rc, r2),
            }
        }
        self.mover_tubos();
        self.acabar_tuberia_si_toca();
    }

    /// La orden número `i` de la tubería acabó: quien la leía ve el final, y quien le escribía, que ya no hay nadie.
    fn etapa_acabada(&mut self, i: usize, rc: i32, r2: i32) {
        let Some(t) = &mut self.p.tuberia else { return };
        if let Some(tubo) = t.tubos.get_mut(i) {
            tubo.sin_escritor = true;
        }
        if i > 0 && let Some(tubo) = t.tubos.get_mut(i - 1) {
            tubo.sin_lector = true;
            tubo.datos.clear();
        }
        // El código de la tubería es el de su última orden.
        if i + 1 == t.etapas {
            t.rc = rc;
            t.r2 = r2;
        }
    }

    /// Si ya no queda ninguna orden viva de la tubería, se acaba: el intérprete sigue con su código (o con lo
    /// capturado).
    fn acabar_tuberia_si_toca(&mut self) {
        if self.p.tuberia.as_ref().is_some_and(|t| t.vivas == 0) {
            let t = self.p.tuberia.take().unwrap();
            match t.captura {
                Some(c) => {
                    let texto = String::from_utf8_lossy(&c).into_owned();
                    self.con_interprete(|i, m| i.capturada(m, &texto, t.rc));
                }
                None => self.con_interprete(|i, m| i.acabada(m, t.rc, t.r2)),
            }
        }
    }

    /// Mueve lo que haya en los tubos: lo que espera a caber entra si hay sitio, y quien espera una línea la recibe si
    /// la hay (o el final, si ya nadie escribe). Hasta que no se mueva nada más.
    fn mover_tubos(&mut self) {
        loop {
            let mut movido = false;
            for k in 0..self.p.trabajos.len() {
                // Quien escribe y no cabía.
                if let (Some(datos), Destino::Tubo(n)) = (&self.p.trabajos[k].por_escribir, &self.p.trabajos[k].salida) {
                    let n = *n;
                    let roto = self.p.trabajos[k].roto.is_some();
                    let estado = match self.p.tuberia.as_mut().and_then(|t| t.tubos.get_mut(n)) {
                        None => Some(p::SIN_LECTOR),
                        Some(t) if t.sin_lector || roto => Some(p::SIN_LECTOR),
                        Some(t) if t.datos.len() + datos.len() <= MAX_TUBO => {
                            t.datos.extend(datos.iter().copied());
                            Some(0)
                        }
                        Some(_) => None,
                    };
                    if let Some(e) = estado {
                        self.p.trabajos[k].por_escribir = None;
                        self.contestar(k, e, &[]);
                        movido = true;
                    }
                }
                // Quien lee del tubo y no había nada.
                if self.p.trabajos[k].leyendo && matches!(self.p.trabajos[k].entrada, Fuente_::Tubo(_)) {
                    if let Some(d) = self.entrada_de(k) {
                        self.p.trabajos[k].leyendo = false;
                        self.contestar(k, d.len() as i32, &d);
                        movido = true;
                    }
                }
            }
            if !movido {
                return;
            }
        }
    }

    /// Hace una orden en primer plano, la número `etapa` de su tubería. `Some` con su código si ya está hecha (era
    /// interna, o no se pudo lanzar); `None` si quedó en marcha (o era un guion).
    fn una_orden(&mut self, o: &OrdenLinea, entrada: Fuente_, salida: Destino, sola: bool, etapa: usize) -> Option<(i32, i32)> {
        let (entrada, mut salida) = match self.abrir_redirecciones(o, entrada, salida) {
            Ok(x) => x,
            Err(e) => return Some((RC_FALLO, e)),
        };
        let mut texto = String::new();
        if let Some(rc) = self.interna(o, &mut texto) {
            let e = self.a_destino(&mut salida, texto.as_bytes());
            self.cerrar_destino(salida);
            return Some(if e != 0 && e != p::SIN_LECTOR { (RC_FALLO, e) } else { (rc, 0) });
        }
        match self.buscar(&o.nombre) {
            Ok(Hallada::Guion { ruta }) if sola => {
                self.cerrar_destino(salida);
                let leido = self.s.leer_entero(&ruta, orden::MAX_ENTERO);
                match leido {
                    Ok(d) => {
                        let texto = String::from_utf8_lossy(&d).into_owned();
                        self.p.tuberia = None;
                        self.con_interprete(|i, m| i.guion(m, &ruta, &texto, &o.args));
                        None
                    }
                    Err(e) => {
                        self.di_linea(&format!("{}: {}", o.nombre, orden::texto_error(e)));
                        Some((RC_FALLO, e))
                    }
                }
            }
            Ok(Hallada::Guion { .. }) => {
                self.cerrar_destino(salida);
                self.di_linea(&format!("{}: un guion no va en una tubería ni en una sustitución (con Execute, sí)", o.nombre));
                Some((RC_FALLO, 0))
            }
            Ok(Hallada::Programa { ruta }) => {
                // Si no se lanza, la salida (un fichero abierto) se cierra dentro: aquí ya no se tiene.
                let asa = if let Destino::Fichero { asa, .. } = &salida { Some(*asa) } else { None };
                match self.lanzar(&ruta, &o.nombre, &o.args, true, entrada, salida, true) {
                    Ok(_) => {
                        if let Some(t) = self.p.trabajos.last_mut() {
                            t.etapa = Some(etapa);
                        }
                        None
                    }
                    Err(e) => {
                        if let Some(asa) = asa {
                            self.s.cerrar(asa);
                        }
                        let porque = match e {
                            a::MAL_FORMADO => "no es un programa de DashOS",
                            ps::PROTEGIDO => "sesiones no deja lanzarlo",
                            e => orden::texto_error(e),
                        };
                        self.di_linea(&format!("{}: no se puede lanzar: {}", o.nombre, porque));
                        Some((RC_FALLO, e))
                    }
                }
            }
            Err(e) => {
                self.cerrar_destino(salida);
                let porque = match e {
                    a::NO_EXISTE => "orden desconocida",
                    a::PROTEGIDO => "no tiene permiso de ejecutar",
                    e => orden::texto_error(e),
                };
                self.di_linea(&format!("{}: {}", o.nombre, porque));
                Some((RC_ERROR, e))
            }
        }
    }

    // --- Las órdenes internas ---

    /// Las órdenes que tocan el estado del Shell. `None` si `o` no es una. Lo que escriben, en `sal`.
    fn interna(&mut self, o: &OrdenLinea, sal: &mut String) -> Option<i32> {
        let n = o.nombre.to_ascii_lowercase();
        // La plantilla de cada una, de la tabla de todas (`shell::ordenes`). `NewShell` pasa sus argumentos tal cual.
        let plantilla_ = shell::ordenes::interna(&n)?;
        let plantilla_ = if n == "newshell" { "ARGS/F" } else { plantilla_ };
        let args = match plantilla::leer(plantilla_, &o.args) {
            Ok(x) => x,
            Err(plantilla::Error::Ayuda) => {
                sal.push_str(&format!("{}\n", plantilla_));
                return Some(RC_BIEN);
            }
            Err(e) => {
                self.di_linea(&format!("{}: {}", o.nombre, e));
                return Some(RC_FALLO);
            }
        };
        let nombre = args.texto("NAME").unwrap_or("");
        let valor = args.texto("STRING").unwrap_or("");
        Some(match n.as_str() {
            "set" if nombre.is_empty() => {
                for (k, v) in &self.p.int.locales {
                    sal.push_str(&format!("{:<16} {}\n", k, v));
                }
                RC_BIEN
            }
            "set" => {
                // `Set nombre=valor` también vale.
                let (nombre, valor) = match nombre.split_once('=') {
                    Some((k, v)) if valor.is_empty() => (k, v),
                    _ => (nombre, valor),
                };
                if !nombre_de_variable(nombre) || self.p.int.locales.len() >= 256 || valor.len() > 1024 {
                    self.di_linea("Set: ese nombre o ese valor no valen");
                    return Some(RC_ERROR);
                }
                self.p.int.poner_local(nombre, valor);
                RC_BIEN
            }
            "unset" => {
                self.p.int.locales.retain(|(k, _)| !k.eq_ignore_ascii_case(nombre));
                RC_BIEN
            }
            "get" => match self.p.int.locales.iter().find(|(k, _)| k.eq_ignore_ascii_case(nombre)) {
                Some((_, v)) => {
                    sal.push_str(&format!("{}\n", v));
                    RC_BIEN
                }
                None => RC_AVISO,
            },
            "alias" if nombre.is_empty() => {
                for (k, v) in &self.p.int.alias {
                    sal.push_str(&format!("{:<16} {}\n", k, v));
                }
                RC_BIEN
            }
            "alias" => {
                self.p.int.alias.retain(|(k, _)| !k.eq_ignore_ascii_case(nombre));
                if !valor.is_empty() && self.p.int.alias.len() < 64 {
                    self.p.int.alias.push((String::from(nombre), String::from(valor)));
                }
                RC_BIEN
            }
            "unalias" => {
                self.p.int.alias.retain(|(k, _)| !k.eq_ignore_ascii_case(nombre));
                RC_BIEN
            }
            "setenv" if nombre.is_empty() => match self.s.listar("ENV:") {
                Ok(entradas) => {
                    for e in entradas.iter().filter(|e| !e.datos.cajon()) {
                        let v = self.env(&e.nombre).unwrap_or_default();
                        sal.push_str(&format!("{:<16} {}\n", e.nombre, v));
                    }
                    RC_BIEN
                }
                Err(e) => {
                    self.di_linea(&format!("SetEnv: ENV: {}", orden::texto_error(e)));
                    RC_ERROR
                }
            },
            "setenv" => {
                if !nombre_de_variable(nombre) {
                    self.di_linea("SetEnv: ese nombre no vale");
                    return Some(RC_ERROR);
                }
                let f = format!("ENV:{}", nombre);
                let r = self.s.abrir_para_escribir(&f, false).and_then(|(asa, _)| {
                    let r = self.s.escribir(asa, 0, valor.as_bytes());
                    self.s.cerrar(asa);
                    r
                });
                match r {
                    Ok(()) => RC_BIEN,
                    Err(e) => {
                        self.di_linea(&format!("SetEnv: {}: {}", f, orden::texto_error(e)));
                        RC_ERROR
                    }
                }
            }
            "getenv" => match self.env(nombre) {
                Some(v) => {
                    sal.push_str(&format!("{}\n", v));
                    RC_BIEN
                }
                None => RC_AVISO,
            },
            "unsetenv" => {
                if nombre_de_variable(nombre) {
                    let _ = self.s.borrar(&format!("ENV:{}", nombre));
                }
                RC_BIEN
            }
            "path" => {
                let mut rc = RC_BIEN;
                if args.si("RESET") {
                    self.p.camino.clear();
                }
                let nuevos = args.varios("PATH");
                if !nuevos.is_empty() && !args.si("ADD") {
                    self.p.camino.clear();
                }
                for c in nuevos {
                    match self.s.ruta(c).and_then(|r| self.s.mirar(&r).map(|d| (r, d))) {
                        Ok((r, d)) if d.cajon() => {
                            if !self.p.camino.iter().any(|x| x.eq_ignore_ascii_case(&r)) && self.p.camino.len() < 32 {
                                self.p.camino.push(r);
                            }
                        }
                        Ok(_) => {
                            self.di_linea(&format!("Path: {}: no es un cajón", c));
                            rc = RC_ERROR;
                        }
                        Err(e) => {
                            self.di_linea(&format!("Path: {}: {}", c, orden::texto_error(e)));
                            rc = RC_ERROR;
                        }
                    }
                }
                if nuevos.is_empty() || args.si("SHOW") {
                    sal.push_str("El cajón actual\n");
                    for c in &self.p.camino {
                        sal.push_str(&format!("{}\n", c));
                    }
                    sal.push_str("C:\n");
                }
                rc
            }
            "which" => {
                let f = args.texto("FILE").unwrap_or("");
                if shell::ordenes::es_interna(f) {
                    sal.push_str("INTERNA\n");
                    RC_BIEN
                } else {
                    match self.buscar(f) {
                        Ok(Hallada::Programa { ruta }) | Ok(Hallada::Guion { ruta }) => {
                            // Dónde está de verdad (qué capa de `C:`), si se sabe.
                            let capa = self.capa_de(&ruta).unwrap_or(ruta);
                            sal.push_str(&format!("{}\n", capa));
                            RC_BIEN
                        }
                        Err(_) => RC_AVISO,
                    }
                }
            }
            "run" => self.run(args.texto("COMMAND").unwrap_or(""), sal),
            "status" => {
                for t in &self.p.trabajos {
                    let que = if t.primer_plano { String::from("primer plano") } else { format!("[{}] de fondo", t.numero) };
                    sal.push_str(&format!("Proceso {:>6}: {} ({}{})\n", t.pid, t.nombre, que, if t.roto.is_some() { ", roto" } else { "" }));
                }
                if self.p.trabajos.is_empty() {
                    sal.push_str("No hay trabajos en marcha\n");
                }
                RC_BIEN
            }
            "break" => {
                let cual = args.numero("PROCESS");
                let ahora = dashos::ahora_us();
                let mut alguno = false;
                for t in self.p.trabajos.iter_mut().filter(|t| !t.primer_plano && cual.is_none_or(|c| c == t.numero as i64 || c == t.pid as i64)) {
                    t.roto.get_or_insert(ahora);
                    alguno = true;
                }
                if alguno {
                    RC_BIEN
                } else {
                    self.di_linea("Break: no hay ese trabajo");
                    RC_AVISO
                }
            }
            "why" => {
                let (rc, r2) = (self.p.int.rc, self.p.int.resultado2);
                if rc == 0 {
                    sal.push_str("La última orden acabó bien\n");
                } else if r2 != 0 {
                    sal.push_str(&format!("La última orden acabó con el código {}: {} ({})\n", rc, orden::texto_error(r2), r2));
                } else {
                    sal.push_str(&format!("La última orden acabó con el código {}\n", rc));
                }
                RC_BIEN
            }
            "cls" => {
                if let Some(v) = &mut self.ventana {
                    self.p.consola.limpiar();
                    v.sucia = true;
                }
                RC_BIEN
            }
            "completa" => {
                // Lo que dan los complementos: la plantilla de una orden, para completar y resaltar.
                let (orden_, plantilla_nueva) = (args.texto("ORDEN").unwrap_or(""), args.texto("PLANTILLA").unwrap_or(""));
                if orden_.is_empty() {
                    for (k, v) in &self.p.plantillas {
                        sal.push_str(&format!("{:<16} {}\n", k, v));
                    }
                    RC_BIEN
                } else if orden_.len() > 30 || plantilla_nueva.len() > 200 || matches!(plantilla::leer(plantilla_nueva, ""), Err(plantilla::Error::Plantilla)) {
                    self.di_linea(&format!("Completa: la plantilla de {} no vale", orden_));
                    RC_ERROR
                } else {
                    self.p.plantillas.retain(|x| !x.0.eq_ignore_ascii_case(orden_));
                    if self.p.plantillas.len() < 128 {
                        self.p.plantillas.push((String::from(orden_), String::from(plantilla_nueva)));
                    }
                    RC_BIEN
                }
            }
            "medidas" if args.si("CALLA") || args.si("HABLA") => {
                // Con `REGISTRO`, deja de copiar la consola al registro del sistema (para una salida muy larga, que lo
                // llenaría) o vuelve a copiarla.
                self.registro_callado = args.si("CALLA");
                RC_BIEN
            }
            "medidas" => {
                // Lo que ha costado la consola desde la última vez que se preguntó, y se empieza a medir otra vez.
                let ahora = dashos::ahora_us();
                sal.push_str(&format!(
                    "Medidas: en {} ms, {} bytes a la consola en {} ms; {} cuadros pintados en {} ms\n",
                    (ahora - self.medido_desde) / 1000,
                    self.medido_bytes,
                    self.medido_escribir / 1000,
                    self.medido_cuadros,
                    self.medido_pintar / 1000
                ));
                (self.medido_desde, self.medido_bytes, self.medido_escribir, self.medido_cuadros, self.medido_pintar) = (ahora, 0, 0, 0, 0);
                RC_BIEN
            }
            "teclea" => {
                // Teclea en este panel, como desde el teclado: letras, y entre ángulos las teclas con nombre (<TAB>,
                // <INTRO>, <ESC>, <ARRIBA>, <ABAJO>, <IZQUIERDA>, <DERECHA>, <INICIO>, <FIN>, <BORRA>, <CTRL-R>...).
                // Después dice cómo queda la línea: lo tecleado, la sugerencia entre corchetes y el menú.
                let texto = args.texto("TEXTO").unwrap_or("");
                let texto = shell::plantilla::trozos(texto).ok().filter(|t| t.len() == 1).map(|t| t[0].texto.clone()).unwrap_or_else(|| String::from(texto));
                let mut resto = texto.as_str();
                while let Some(c) = resto.chars().next() {
                    let nombre = resto.strip_prefix('<').and_then(|r| r.find('>').map(|f| &r[..f]));
                    let tecla = nombre.and_then(|n| match n.to_ascii_uppercase().as_str() {
                        "TAB" => Some((Tecla::Tabulador, false)),
                        "INTRO" => Some((Tecla::Intro, false)),
                        "ESC" => Some((Tecla::Escape, false)),
                        "ARRIBA" => Some((Tecla::Arriba, false)),
                        "ABAJO" => Some((Tecla::Abajo, false)),
                        "IZQUIERDA" => Some((Tecla::Izquierda, false)),
                        "DERECHA" => Some((Tecla::Derecha, false)),
                        "INICIO" => Some((Tecla::Inicio, false)),
                        "FIN" => Some((Tecla::Fin, false)),
                        "BORRA" => Some((Tecla::Retroceso, false)),
                        "CTRL-DERECHA" => Some((Tecla::Derecha, true)),
                        x if x.len() == 6 && x.starts_with("CTRL-") => Some((Tecla::Letra(x.as_bytes()[5].to_ascii_lowercase() as char), true)),
                        _ => None,
                    });
                    match (tecla, nombre) {
                        (Some((t, control)), Some(n)) => {
                            self.tecla_con(Some(t), 0, control, false, false);
                            resto = &resto[n.len() + 2..];
                        }
                        _ => {
                            self.tecla_con(Some(Tecla::Letra(c)), 0, false, false, false);
                            resto = &resto[c.len_utf8()..];
                        }
                    }
                    // La sugerencia y el resaltado, al día tras cada tecla (como entre dos teclas de verdad).
                    self.p.resaltada = u64::MAX;
                    self.cupo_de_miradas = 64;
                    self.resaltar();
                }
                let ed = &self.p.consola.editor;
                sal.push_str(&format!("«{}»", ed.texto()));
                if !ed.sugerencia.is_empty() {
                    sal.push_str(&format!(" [{}]", ed.sugerencia));
                }
                if let Some(b) = ed.buscando() {
                    sal.push_str(&format!(" (buscar: {})", b));
                }
                if let Some(m) = &self.p.menu {
                    let muestras: Vec<&str> = m.c.candidatos.iter().map(|x| x.muestra.as_str()).collect();
                    sal.push_str(&format!(" menú: {}", muestras.join(" ")));
                    if let Some(k) = m.elegido {
                        sal.push_str(&format!(" (elegido {})", m.c.candidatos[k].muestra));
                    }
                }
                sal.push('\n');
                RC_BIEN
            }
            "completar" => {
                // Lo que daría Tab al final de esa línea (para verlo desde un guion).
                let linea_ = entre_comillas(args.texto("LINEA").unwrap_or(""));
                let c = completar::completar(&linea_, linea_.len(), &mut FuenteShell { shell: self });
                let muestras: Vec<&str> = c.candidatos.iter().map(|x| x.muestra.as_str()).collect();
                sal.push_str(&format!("{}{}\n", muestras.join("  "), if c.mas { "  ..." } else { "" }));
                if c.candidatos.is_empty() { RC_AVISO } else { RC_BIEN }
            }
            "resaltar" => {
                // La clase de cada letra de esa línea: o una orden que existe, x una que no, c una cadena, a una sin
                // cerrar, r una ruta que existe, + un operador, v una variable, k una palabra de la plantilla.
                let linea_ = entre_comillas(args.texto("LINEA").unwrap_or(""));
                self.cupo_de_miradas = 64;
                let clases = resaltar::resaltar(&linea_, &mut MiraShell { shell: self });
                let letras: String = clases
                    .iter()
                    .map(|c| match c {
                        Clase::Normal => '.',
                        Clase::Orden => 'o',
                        Clase::OrdenMala => 'x',
                        Clase::Cadena => 'c',
                        Clase::CadenaAbierta => 'a',
                        Clase::Ruta => 'r',
                        Clase::Operador => '+',
                        Clase::Variable => 'v',
                        Clase::Clave => 'k',
                        Clase::Comentario => ';',
                    })
                    .collect();
                sal.push_str(&format!("{}\n", letras));
                RC_BIEN
            }
            "historial" => {
                if args.si("BORRAR") {
                    self.p.consola.editor.historial.borrar();
                    if self.ventana.is_some() {
                        let _ = self.escribir_fichero(HISTORIAL, b"", false);
                    }
                } else {
                    for (i, l) in self.p.consola.editor.historial.lineas().iter().enumerate() {
                        sal.push_str(&format!("{:>5}  {}\n", i + 1, l));
                    }
                }
                RC_BIEN
            }
            "prompt" => {
                // Sin nada, el de los ajustes; con texto, ese (con %S, %N y %R, como en AmigaDOS).
                let texto = args.texto("PROMPT").unwrap_or("");
                let texto = shell::plantilla::trozos(texto).ok().filter(|t| t.len() == 1).map(|t| t[0].texto.clone()).unwrap_or_else(|| String::from(texto));
                self.p.indicador_propio = (!texto.is_empty()).then_some(texto);
                RC_BIEN
            }
            "panel" => {
                // Lo mismo que las teclas: partir el panel, abrir una pestaña, cerrar este, pasar al siguiente.
                if self.ventana.is_none() {
                    self.di_linea("Panel: este Shell no tiene ventana");
                    RC_AVISO
                } else if args.si("CERRAR") {
                    self.p.salir = Some(self.p.int.rc);
                    RC_BIEN
                } else if args.si("SIGUIENTE") {
                    let ids = self.disp.paneles_de(self.disp.pestana());
                    let k = ids.iter().position(|&x| x == self.disp.activo()).map_or(0, |k| (k + 1) % ids.len().max(1));
                    if let Some(&id) = ids.get(k) {
                        self.disp.activar(id);
                    }
                    RC_BIEN
                } else if self.abrir_panel(args.si("VERTICAL"), args.si("PESTANA")) {
                    sal.push_str(&format!("{} paneles en {} pestañas\n", self.disp.todos().len(), self.disp.pestanas()));
                    RC_BIEN
                } else {
                    self.di_linea("Panel: no cabe otro panel");
                    RC_AVISO
                }
            }
            "newshell" => match self.buscar("SYS:System/Shell") {
                Ok(Hallada::Programa { ruta }) => match self.lanzar(&ruta, "Shell", &o.args, false, Fuente_::Nada, Destino::Consola, false) {
                    Ok(_) => {
                        // No se le atiende: no es un trabajo de este Shell.
                        if let Some(t) = self.p.trabajos.pop() {
                            dashos::cerrar(t.canal);
                        }
                        RC_BIEN
                    }
                    Err(e) => {
                        self.di_linea(&format!("NewShell: {}", orden::texto_error(e)));
                        RC_FALLO
                    }
                },
                _ => {
                    self.di_linea("NewShell: no está SYS:System/Shell");
                    RC_FALLO
                }
            },
            _ => RC_BIEN,
        })
    }

    /// La ruta de `ruta_` con el nombre del volumen donde está de verdad (`C:Dir` es `SYS:C/Dir`).
    fn capa_de(&mut self, ruta_: &str) -> Option<String> {
        let (raiz, camino, _) = self.s.donde(ruta_).ok()?;
        let tabla = self.s.tabla.clone();
        let vol = tabla.volumenes().find(|l| l.destinos.strip_prefix(b"@").and_then(|d| core::str::from_utf8(d).ok()).and_then(|d| d.parse::<u8>().ok()) == Some(raiz))?;
        Some(format!("{}:{}", core::str::from_utf8(vol.nombre).ok()?, camino))
    }

    /// `Run`: lanza una orden de fondo. Un programa, tal cual; lo demás (una interna, un guion, una tubería), en otro
    /// Shell sin ventana, que escribe en esta consola.
    fn run(&mut self, linea_: &str, sal: &mut String) -> i32 {
        let ordenes = match shell::linea::partir(linea_) {
            Ok(o) if !o.is_empty() => o,
            Ok(_) => {
                self.di_linea("Run: falta la orden");
                return RC_FALLO;
            }
            Err(e) => {
                self.di_linea(&format!("Run: {}", e));
                return RC_FALLO;
            }
        };
        let directa = if ordenes.len() == 1 && self.interna_o_control(&ordenes[0].nombre).is_none() {
            match self.buscar(&ordenes[0].nombre) {
                Ok(Hallada::Programa { ruta }) => Some(ruta),
                Ok(Hallada::Guion { .. }) => None,
                Err(e) => {
                    self.di_linea(&format!("Run: {}: {}", ordenes[0].nombre, if e == a::NO_EXISTE { "orden desconocida" } else { orden::texto_error(e) }));
                    return RC_ERROR;
                }
            }
        } else {
            None
        };
        let r = match directa {
            Some(ruta) => {
                let o = &ordenes[0];
                match self.abrir_redirecciones(o, Fuente_::Nada, Destino::Consola) {
                    Ok((entrada, salida)) => {
                        let asa = if let Destino::Fichero { asa, .. } = &salida { Some(*asa) } else { None };
                        let r = self.lanzar(&ruta, &o.nombre, &o.args, false, entrada, salida, true);
                        if let (Err(_), Some(asa)) = (&r, asa) {
                            self.s.cerrar(asa);
                        }
                        r
                    }
                    Err(e) => Err(e),
                }
            }
            None => match self.buscar("SYS:System/Shell") {
                Ok(Hallada::Programa { ruta }) => {
                    let nombre = format!("Shell ({})", ordenes[0].nombre);
                    self.lanzar(&ruta, &nombre, &format!("SINARRANQUE ORDEN {}", linea_), false, Fuente_::Nada, Destino::Consola, true)
                }
                _ => Err(a::NO_EXISTE),
            },
        };
        match r {
            Ok(pid) => {
                let numero = self.p.trabajos.last().map_or(0, |t| t.numero);
                sal.push_str(&format!("[{}] {} (proceso {})\n", numero, ordenes[0].nombre, pid));
                RC_BIEN
            }
            Err(e) => {
                self.di_linea(&format!("Run: no se puede lanzar: {}", orden::texto_error(e)));
                RC_FALLO
            }
        }
    }

    fn interna_o_control(&self, nombre: &str) -> Option<()> {
        shell::ordenes::es_interna(nombre).then_some(())
    }

    // --- Lo que piden los trabajos ---

    fn contestar(&self, k: usize, estado: i32, datos: &[u8]) {
        let t = &self.p.trabajos[k];
        let mut r = [0u8; 256];
        let l = datos.len().min(248);
        r[0..4].copy_from_slice(&estado.to_le_bytes());
        r[4..8].copy_from_slice(&(if t.roto.is_some() { p::ROTO } else { 0 }).to_le_bytes());
        r[8..8 + l].copy_from_slice(&datos[..l]);
        dashos::canal_escribir(t.canal, &r[..8 + l], &[]);
    }

    /// Una línea (o un trozo) de la entrada del trabajo `k`, si hay: `None` si tiene que esperar al teclado.
    fn entrada_de(&mut self, k: usize) -> Option<Vec<u8>> {
        let t = &mut self.p.trabajos[k];
        if !t.resto.is_empty() {
            let n = t.resto.len().min(p::POR_LECTURA);
            return Some(t.resto.drain(..n).collect());
        }
        match &mut t.entrada {
            Fuente_::Nada => Some(Vec::new()),
            Fuente_::Datos(d, pos) => {
                // Las líneas enteras que quepan en una respuesta (o un trozo, si una no cabe).
                let resto = &d[*pos..];
                let tope = resto.len().min(p::POR_LECTURA);
                let n = resto[..tope].iter().rposition(|&c| c == b'\n').map_or(tope, |i| i + 1);
                *pos += n;
                Some(Vec::from(&resto[..n]))
            }
            Fuente_::Tubo(n) => {
                // Una línea entera si la hay (o lo que quepa en una respuesta); con el escritor fuera, lo que quede y
                // luego el final. Si no hay línea aún, se espera.
                let n = *n;
                let tubo = self.p.tuberia.as_mut().and_then(|x| x.tubos.get_mut(n))?;
                // Las líneas enteras que quepan en una respuesta.
                let linea = (0..tubo.datos.len().min(p::POR_LECTURA)).rev().find(|&i| tubo.datos[i] == b'\n').map(|i| i + 1);
                let cuanto = match linea {
                    Some(c) => c,
                    None if tubo.datos.len() >= p::POR_LECTURA => p::POR_LECTURA,
                    None if tubo.sin_escritor => tubo.datos.len(),
                    None => return None,
                };
                Some(tubo.datos.drain(..cuanto).collect())
            }
            Fuente_::Teclado if !t.primer_plano => Some(Vec::new()),
            Fuente_::Teclado => {
                // Sin ventana no hay teclado: es el final.
                if self.ventana.is_none() {
                    return Some(Vec::new());
                }
                let mut l = self.p.tecleadas.pop_front()?.into_bytes();
                l.push(b'\n');
                let t = &mut self.p.trabajos[k];
                t.resto = l;
                let n = t.resto.len().min(p::POR_LECTURA);
                Some(t.resto.drain(..n).collect())
            }
        }
    }

    fn peticion(&mut self, marca: u64, m: &[u8]) {
        let Some(k) = self.p.trabajos.iter().position(|t| t.id == marca) else { return };
        if m.len() < 4 {
            return;
        }
        let cuerpo = &m[4..];
        match u32::from_le_bytes(m[0..4].try_into().unwrap()) {
            p::ESCRIBIR if cuerpo.len() >= 4 && matches!(self.p.trabajos[k].salida, Destino::Tubo(_)) && u32::from_le_bytes(cuerpo[0..4].try_into().unwrap()) != p::ERRORES => {
                // A un tubo: si no cabe, no se le contesta hasta que quien lee haga sitio.
                self.p.trabajos[k].por_escribir = Some(Vec::from(&cuerpo[4..]));
                self.mover_tubos();
            }
            p::ESCRIBIR if cuerpo.len() >= 4 => {
                let mut destino = core::mem::replace(&mut self.p.trabajos[k].salida, Destino::Consola);
                // Los errores van siempre a la consola, aunque la salida esté redirigida.
                let estado = if u32::from_le_bytes(cuerpo[0..4].try_into().unwrap()) == p::ERRORES {
                    self.a_destino(&mut Destino::Consola, &cuerpo[4..])
                } else {
                    self.a_destino(&mut destino, &cuerpo[4..])
                };
                self.p.trabajos[k].salida = destino;
                self.contestar(k, estado, &[]);
            }
            p::LEER => match self.entrada_de(k) {
                Some(d) => {
                    self.contestar(k, d.len() as i32, &d);
                    // Con el sitio que deja, quizá entra lo que otra esperaba a escribir.
                    self.mover_tubos();
                }
                None => self.p.trabajos[k].leyendo = true,
            },
            p::PULSO => self.contestar(k, 0, &[]),
            p::CD => {
                let estado = match core::str::from_utf8(cuerpo).ok().map(|c| (c, self.s.mirar(c))) {
                    Some((c, Ok(d))) if d.cajon() && c.contains(':') => {
                        self.s.actual = String::from(c);
                        self.p.actual = self.s.actual.clone();
                        0
                    }
                    Some((_, Ok(_))) => a::NO_ES_CAJON,
                    Some((_, Err(e))) => e,
                    None => a::MAL_FORMADO,
                };
                self.contestar(k, estado, &[]);
            }
            p::ASIGNAR if cuerpo.len() >= 4 => {
                let estado = match Cambio::desde(u32::from_le_bytes(cuerpo[0..4].try_into().unwrap())) {
                    Some(c) => asig::asignar_en_sesiones(SESIONES, c, &cuerpo[4..]),
                    None => a::MAL_FORMADO,
                };
                if estado == 0 {
                    self.s.refrescar();
                }
                self.contestar(k, estado, &[]);
            }
            p::SALIR if cuerpo.len() >= 8 => {
                let rc = i32::from_le_bytes(cuerpo[0..4].try_into().unwrap());
                let r2 = i32::from_le_bytes(cuerpo[4..8].try_into().unwrap());
                self.acabar(k, rc, r2);
            }
            _ => self.contestar(k, a::MAL_FORMADO, &[]),
        }
    }

    /// El trabajo `k` acabó: se suelta lo suyo y se sigue con lo que esperaba por él.
    fn acabar(&mut self, k: usize, rc: i32, r2: i32) {
        let t = self.p.trabajos.remove(k);
        dashos::cerrar(t.canal);
        self.cerrar_destino(t.salida);
        if !t.primer_plano {
            if rc != 0 || t.roto.is_some() {
                self.di_linea(&format!("[{}] {} acabó con el código {}", t.numero, t.nombre, rc));
            }
            return;
        }
        match (t.etapa, &mut self.p.tuberia) {
            (Some(etapa), Some(tu)) => {
                tu.vivas = tu.vivas.saturating_sub(1);
                self.etapa_acabada(etapa, rc, r2);
                self.mover_tubos();
                self.acabar_tuberia_si_toca();
            }
            _ => self.con_interprete(|i, m| i.acabada(m, rc, r2)),
        }
    }

    /// Mira los trabajos: el que murió sin avisar (su canal se cerró) y el que no acaba tras romperlo.
    fn vigilar(&mut self) {
        let ahora = dashos::ahora_us();
        let mut k = 0;
        while k < self.p.trabajos.len() {
            let t = &self.p.trabajos[k];
            let mut b = [0u8; 256];
            let mut ll = [0u32; 8];
            let (n, kk, _) = dashos::canal_leer_plazo(t.canal, &mut b, &mut ll, 1);
            ll[..kk.min(8)].iter().for_each(|&l| {
                dashos::cerrar(l);
            });
            if n == dashos::E_CERRADO {
                let nombre = t.nombre.clone();
                if t.roto.is_none() {
                    self.di_linea(&format!("{}: acabó sin avisar (un fallo del programa)", nombre));
                }
                self.acabar(k, RC_FALLO, 0);
                continue;
            }
            if t.roto.is_some_and(|r| ahora.saturating_sub(r) > PLAZO_ROTO_US) {
                // No hace caso: `sesiones` lo mata (es del mismo usuario).
                let (pid, nombre) = (t.pid, t.nombre.clone());
                let estado = matar(pid);
                self.di_linea(&format!("{}: no acababa y se ha matado ({})", nombre, estado));
                self.acabar(k, RC_FALLO, 0);
                continue;
            }
            k += 1;
        }
    }

    /// Ctrl-C: rompe lo que hay en primer plano (una orden, una tubería, un guion).
    fn romper(&mut self) {
        let ahora = dashos::ahora_us();
        let mut alguno = false;
        for k in 0..self.p.trabajos.len() {
            if self.p.trabajos[k].primer_plano {
                self.p.trabajos[k].roto.get_or_insert(ahora);
                alguno = true;
                if core::mem::take(&mut self.p.trabajos[k].leyendo) {
                    self.contestar(k, 0, &[]);
                }
            }
        }
        // Las que esperaban a escribir en un tubo dejan de esperar (se les contesta, con "roto").
        self.mover_tubos();
        let habia = self.con_interprete(|i, m| i.romper(m));
        if alguno && !habia {
            // Una orden sin intérprete por medio (no pasa hoy): que se diga igual.
            self.di_linea("***Break");
        }
        if !alguno && !habia {
            // Nada en marcha: la línea que se estaba tecleando se deja.
            if let Some(v) = &mut self.ventana {
                self.p.consola.editor.borrar();
                v.sucia = true;
            }
        }
    }

    /// Final de la entrada (Ctrl-D): a la orden que lee; sin ninguna y sin nada tecleado, el Shell acaba.
    fn fin_de_entrada(&mut self) {
        if let Some(k) = self.p.trabajos.iter().position(|t| t.primer_plano && t.leyendo) {
            self.p.trabajos[k].leyendo = false;
            self.contestar(k, 0, &[]);
        } else if !self.p.int.ocupado() && self.p.trabajos.iter().all(|t| !t.primer_plano) && self.ventana.is_some() && self.p.consola.editor.vacio() {
            self.p.salir = Some(self.p.int.rc);
        }
    }

    /// Una línea tecleada: a la orden que la espera, o a la cola.
    fn linea_tecleada(&mut self, l: String) {
        if self.p.tecleadas.len() < 256 {
            self.p.tecleadas.push_back(l);
        }
        self.p.indicador_puesto = false;
        if let Some(k) = self.p.trabajos.iter().position(|t| t.primer_plano && t.leyendo)
            && let Some(d) = self.entrada_de(k)
        {
            self.p.trabajos[k].leyendo = false;
            self.contestar(k, d.len() as i32, &d);
        }
    }

    // --- Los paneles ---

    /// Pasa a trabajar en el panel número `k`: sus cosas quedan en `self.p`, y su cajón actual es el de los ficheros.
    fn entrar(&mut self, k: usize) {
        if k != self.en && k < self.paneles.len() {
            self.p.actual = self.s.actual.clone();
            let en = self.en;
            core::mem::swap(&mut self.p, &mut self.paneles[en]);
            core::mem::swap(&mut self.p, &mut self.paneles[k]);
            self.en = k;
            self.s.actual = self.p.actual.clone();
        }
    }

    fn panel(&self, k: usize) -> &Panel {
        if k == self.en { &self.p } else { &self.paneles[k] }
    }

    fn numero_de(&self, id: u32) -> Option<usize> {
        (0..self.paneles.len()).find(|&k| self.panel(k).id == id)
    }

    /// El panel del trabajo de la marca `marca`.
    fn panel_del_trabajo(&self, marca: u64) -> Option<usize> {
        (0..self.paneles.len()).find(|&k| self.panel(k).trabajos.iter().any(|t| t.id == marca))
    }

    /// El área de los paneles en la ventana (bajo la barra de pestañas).
    fn area(&self) -> RectPanel {
        match &self.ventana {
            Some(v) => {
                let barra = v.alto_menus() + v.alto_barra(self.disp.pestanas());
                RectPanel { x: 0, y: barra, w: v.medida.0, h: v.medida.1 - barra }
            }
            None => RectPanel::default(),
        }
    }

    /// Reparte la ventana entre los paneles de la pestaña que se ve, y cada consola se parte a su ancho.
    fn colocar(&mut self) {
        let Some(v) = &mut self.ventana else { return };
        v.entera = true;
        v.sucia = true;
        v.colocar_menus();
        let barra = v.alto_menus() + v.alto_barra(self.disp.pestanas());
        let area = RectPanel { x: 0, y: barra, w: v.medida.0, h: v.medida.1 - barra };
        for (id, r) in self.disp.rects(area, 1) {
            let (columnas, filas) = self.ventana.as_ref().unwrap().letras_en(r);
            let Some(k) = self.numero_de(id) else { continue };
            let p = if k == self.en { &mut self.p } else { &mut self.paneles[k] };
            p.rect = r;
            p.consola.medir(columnas, filas);
        }
    }

    /// Un panel nuevo, con su historial, su guion de arranque y sus complementos por cargar. No se coloca aún.
    fn panel_nuevo(&mut self, actual: &str) -> Panel {
        let id = self.siguiente_panel;
        self.siguiente_panel += 1;
        let (columnas, filas) = self.ventana.as_ref().map_or((self.columnas, 25), |v| v.de_serie);
        let mut p = Panel::nuevo(id, actual, columnas, filas);
        if self.ventana.is_some() {
            // El historial guardado; si ha crecido mucho, se deja en lo que se recuerda.
            if let Ok(d) = self.s.leer_entero(HISTORIAL, 1 << 20) {
                p.consola.editor.historial.cargar(&d);
                if d.len() > 256 * 1024 {
                    let texto = p.consola.editor.historial.texto();
                    let _ = self.escribir_fichero(HISTORIAL, texto.as_bytes(), false);
                }
            }
        }
        if !self.sin_arranque {
            // Los complementos: los guiones de `S:Shell-Complementos`, por orden alfabético (el del usuario tapa al del
            // sistema que se llame igual, por las capas de `S:`).
            if let Ok(entradas) = self.s.listar(COMPLEMENTOS) {
                p.complementos = entradas.into_iter().filter(|e| !e.datos.cajon()).take(64).map(|e| format!("{}/{}", COMPLEMENTOS, e.nombre)).collect();
            }
            // El guion de arranque: el de `FROM`, o `S:Shell-Startup` (el del usuario tapa al del sistema).
            let guion = self.desde.clone().unwrap_or_else(|| String::from("S:Shell-Startup"));
            match self.s.leer_entero(&guion, ordenes::MAX_ENTERO) {
                Ok(d) => {
                    let texto = String::from_utf8_lossy(&d).into_owned();
                    let mut mudo = Mudo;
                    p.int.guion(&mut mudo, &guion, &texto, "");
                }
                Err(a::NO_EXISTE) if guion == "S:Shell-Startup" => {}
                Err(e) => p.consola.escribir(&format!("Shell: {}: {}\n", guion, ordenes::texto_error(e))),
            }
        }
        p
    }

    /// Parte el panel activo (o abre una pestaña) con un panel nuevo, que se queda el teclado. Falso si no cabe.
    fn abrir_panel(&mut self, vertical: bool, pestana: bool) -> bool {
        if self.ventana.is_none() || self.paneles.len() >= 32 {
            return false;
        }
        let actual = self.s.actual.clone();
        let nuevo = self.panel_nuevo(&actual);
        let id = nuevo.id;
        let cabe = if pestana { self.disp.pestana_nueva(id) } else { self.disp.partir(vertical, id) };
        if !cabe {
            return false;
        }
        // Los ajustes del indicador se leen otra vez: lo que se haya cambiado en su fichero vale desde aquí.
        self.leer_indicador();
        self.paneles.push(nuevo);
        self.colocar();
        // Si los paneles quedan demasiado pequeños, se deshace.
        let k = self.paneles.len() - 1;
        let r = self.paneles[k].rect;
        let pequeno = self.ventana.as_ref().is_some_and(|v| r.w < 12 * v.celda.0 + 2 * BORDE || r.h < 3 * v.celda.1 + 2 * BORDE);
        if pequeno {
            self.disp.cerrar(id);
            self.paneles.pop();
            self.colocar();
            return false;
        }
        true
    }

    /// Quita el panel número `k` (que ya acabó): sus trabajos se matan. Falso si era el último de la ventana.
    /// Mata los trabajos del panel en que se trabaja.
    fn matar_trabajos(&mut self) {
        for t in core::mem::take(&mut self.p.trabajos) {
            matar(t.pid);
            dashos::cerrar(t.canal);
            self.cerrar_destino(t.salida);
        }
    }

    fn quitar_panel(&mut self, k: usize) -> bool {
        self.entrar(k);
        self.matar_trabajos();
        let id = self.p.id;
        if !self.disp.cerrar(id) || self.paneles.len() <= 1 {
            return false;
        }
        // Se trabaja en otro, y este sale de la lista.
        let otro = if k == 0 { 1 } else { 0 };
        self.entrar(otro);
        self.paneles.remove(k);
        if self.en > k {
            self.en -= 1;
        }
        self.colocar();
        true
    }

    /// El número del panel que tiene el teclado.
    fn activo(&self) -> usize {
        self.numero_de(self.disp.activo()).unwrap_or(self.en)
    }

    // --- Los ficheros de ajustes y el historial ---

    /// Escribe `datos` en `fichero` (entero, o al final). Si falta su cajón de `ENVARC:`, lo crea en la capa del usuario.
    fn escribir_fichero(&mut self, fichero: &str, datos: &[u8], anadir: bool) -> Result<(), i32> {
        let abierto = match self.s.abrir_para_escribir(fichero, anadir) {
            Err(a::NO_EXISTE) | Err(a::NO_ES_CAJON) | Err(a::PROTEGIDO) | Err(a::SOLO_LECTURA) => {
                // La capa del usuario de `ENVARC:` y el cajón del Shell, si faltan.
                for cajon in ["Home:Prefs", "Home:Prefs/Env-Archive", "ENVARC:Shell"] {
                    if let Ok(asa) = self.s.crear(cajon, a::CAJON) {
                        self.s.cerrar(asa);
                    }
                }
                self.s.abrir_para_escribir(fichero, anadir)
            }
            x => x,
        };
        let (asa, pos) = abierto?;
        let r = self.s.escribir(asa, pos, datos);
        self.s.cerrar(asa);
        r
    }

    /// Lee los ajustes del indicador (`ENVARC:Shell/Indicador`); lo que no se entienda se dice en el panel.
    fn leer_indicador(&mut self) {
        let mut avisos = Vec::new();
        if let Ok(d) = self.s.leer_entero(INDICADOR, indicador::MAX_FICHERO) {
            self.indicador = indicador::Ajustes::leer(&d, &mut avisos);
        }
        for aviso in avisos {
            self.di_linea(&format!("{}: {}", INDICADOR, aviso));
        }
    }

    /// Pone el indicador en el panel en que se trabaja.
    fn poner_indicador(&mut self) {
        if self.ventana.is_some() && !self.p.consola.al_principio() {
            self.di("\n");
        }
        let hora = match dashos::hora_ns() {
            0 => None,
            ns => {
                // En la hora local del sistema, como la barra.
                let (_, _, _, h, m, _) = shell::fecha::partes(self.s.local_s(ns / 1_000_000_000));
                Some((h, m))
            }
        };
        let celdas = match &self.p.indicador_propio {
            // El de `Prompt`, con las letras de AmigaDOS: %S el cajón, %N el número del Shell, %R el último código.
            Some(texto) => {
                let t = texto.replace("%S", &self.s.actual).replace("%s", &self.s.actual).replace("%N", "1").replace("%n", "1").replace("%R", &format!("{}", self.p.int.rc)).replace("%r", &format!("{}", self.p.int.rc));
                t.chars().filter(|c| (*c as u32) >= 0x20).take(200).map(consola::Celda::normal).collect()
            }
            None => {
                let fondo = self.p.trabajos.iter().filter(|t| !t.primer_plano).count();
                self.indicador.pintar(&indicador::Datos { usuario: "", cajon: &self.s.actual, codigo: self.p.int.rc, trabajos: fondo, hora, git: None })
            }
        };
        // El indicador no va al registro: solo a la ventana.
        self.p.consola.escribir_celdas(&celdas);
        if let Some(v) = &mut self.ventana {
            v.sucia = true;
        }
    }

    // --- Completar, sugerir y resaltar ---

    /// Tab: completa lo que hay antes del cursor. Con una posibilidad, la pone; con varias, lo común y el menú.
    fn completar(&mut self) {
        let linea = self.p.consola.editor.texto();
        let cursor = self.p.consola.editor.cursor_en_bytes();
        let c = completar::completar(&linea, cursor, &mut FuenteShell { shell: self });
        match c.candidatos.len() {
            0 => {}
            1 => {
                let uno = &c.candidatos[0];
                let texto = format!("{}{}", uno.texto, if uno.entero { " " } else { "" });
                self.p.consola.editor.reemplazar(c.desde, c.hasta, &texto);
            }
            _ => {
                let comun = c.comun();
                let mut hasta = c.hasta;
                if comun.len() > c.hasta - c.desde {
                    self.p.consola.editor.reemplazar(c.desde, c.hasta, &comun);
                    hasta = c.desde + comun.len();
                }
                self.p.menu = Some(Menu { c, elegido: None, hasta });
                self.pintar_menu();
            }
        }
    }

    /// Mueve lo elegido en el menú (`paso` 1 o -1) y lo pone en la línea.
    fn mover_menu(&mut self, paso: isize) {
        let Some(m) = &mut self.p.menu else { return };
        let n = m.c.candidatos.len() as isize;
        let k = match m.elegido {
            None if paso > 0 => 0,
            None => n - 1,
            Some(k) => (k as isize + paso).rem_euclid(n),
        } as usize;
        m.elegido = Some(k);
        let texto = m.c.candidatos[k].texto.clone();
        let (desde, hasta) = (m.c.desde, m.hasta);
        m.hasta = desde + texto.len();
        self.p.consola.editor.reemplazar(desde, hasta, &texto);
        self.pintar_menu();
    }

    /// Cierra el menú. Con `elegir`, lo elegido se queda (con su espacio detrás si ya está entero).
    fn cerrar_menu(&mut self, elegir: bool) {
        if let Some(m) = self.p.menu.take() {
            if let (true, Some(k)) = (elegir, m.elegido) {
                if m.c.candidatos[k].entero {
                    self.p.consola.editor.reemplazar(m.hasta, m.hasta, " ");
                }
            }
            self.p.consola.pie.clear();
        }
    }

    /// Las filas del menú, debajo de la línea: los candidatos en columnas, el elegido resaltado.
    fn pintar_menu(&mut self) {
        let Some(m) = &self.p.menu else { return };
        let columnas = self.p.consola.columnas();
        let ancho = (m.c.candidatos.iter().map(|c| c.muestra.chars().count()).max().unwrap_or(1) + 2).min(columnas).max(1);
        let por_fila = (columnas / ancho).max(1);
        let filas = (self.p.consola.filas() / 2).clamp(1, 8);
        // La página en la que está el elegido.
        let por_pagina = por_fila * filas;
        let pagina = m.elegido.unwrap_or(0) / por_pagina;
        let mut pie: Vec<Vec<consola::Celda>> = Vec::new();
        for (i, c) in m.c.candidatos.iter().enumerate().skip(pagina * por_pagina).take(por_pagina) {
            if (i - pagina * por_pagina) % por_fila == 0 {
                pie.push(Vec::new());
            }
            let elegido = m.elegido == Some(i);
            let (letra, estilo) = match (elegido, c.cajon) {
                (true, _) => (consola::NORMAL, consola::INVERSO),
                (false, true) => (consola::paleta(4), consola::NEGRITA),
                (false, false) => (consola::NORMAL, 0),
            };
            let fila = pie.last_mut().unwrap();
            let mut n = 0;
            for l in c.muestra.chars().take(ancho.saturating_sub(1)) {
                fila.push(consola::Celda::con(l, letra, consola::NORMAL, estilo));
                n += 1;
            }
            while n < ancho {
                fila.push(consola::Celda::normal(' '));
                n += 1;
            }
        }
        let total = m.c.candidatos.len();
        if total > por_pagina || m.c.mas {
            let texto = format!("({} de {}{})", (pagina * por_pagina + por_pagina).min(total), total, if m.c.mas { " o más" } else { "" });
            pie.push(texto.chars().map(|l| consola::Celda::con(l, consola::TENUE_GRIS, consola::NORMAL, 0)).collect());
        }
        self.p.consola.pie = pie;
    }

    /// Pone al día la sugerencia y el resaltado de lo que se teclea en el panel en que se trabaja.
    fn resaltar(&mut self) {
        let ed = &self.p.consola.editor;
        if ed.version == self.p.resaltada && !self.p.por_mirar {
            return;
        }
        self.p.resaltada = ed.version;
        let texto = ed.texto();
        // La sugerencia: del historial, si se está al final de la línea y no se busca ni hay menú.
        let sugerencia = if ed.al_final() && ed.buscando().is_none() && self.p.menu.is_none() { ed.historial.sugerencia(&texto).map(String::from) } else { None };
        self.cupo_de_miradas = 8;
        let clases = resaltar::resaltar(&texto, &mut MiraShell { shell: self });
        self.p.por_mirar = self.cupo_de_miradas == 0;
        let ed = &mut self.p.consola.editor;
        ed.resaltado = clases
            .iter()
            .map(|c| match c {
                Clase::Normal => (consola::NORMAL, 0),
                Clase::Orden => (consola::paleta(2), consola::NEGRITA),
                Clase::OrdenMala => (consola::paleta(1), consola::NEGRITA),
                Clase::Cadena => (consola::paleta(3), 0),
                Clase::CadenaAbierta => (consola::paleta(1), 0),
                Clase::Ruta => (consola::NORMAL, consola::SUBRAYADO),
                Clase::Operador => (consola::paleta(5), consola::NEGRITA),
                Clase::Variable => (consola::paleta(6), 0),
                Clase::Clave => (consola::paleta(4), 0),
                Clase::Comentario => (consola::TENUE_GRIS, 0),
            })
            .collect();
        ed.sugerencia = sugerencia.unwrap_or_default();
        if let Some(v) = &mut self.ventana {
            v.sucia = true;
        }
    }

    /// Si existe algo (una orden por su nombre, o una ruta), recordando lo mirado un rato. `None` si ya no se puede
    /// mirar más en esta pasada.
    fn visto(&mut self, clave: String, mirar: impl FnOnce(&mut Shell) -> bool) -> Option<bool> {
        let ahora = dashos::ahora_us();
        if let Some(v) = self.vistos.iter().find(|v| v.0 == clave && ahora.saturating_sub(v.2) < 2_000_000) {
            return Some(v.1);
        }
        if self.cupo_de_miradas == 0 {
            return None;
        }
        self.cupo_de_miradas -= 1;
        let existe = mirar(self);
        self.vistos.retain(|v| v.0 != clave);
        if self.vistos.len() >= 64 {
            self.vistos.remove(0);
        }
        self.vistos.push((clave, existe, ahora));
        Some(existe)
    }

    /// Todas las órdenes que se pueden lanzar por su nombre: las de serie, los alias y los programas del camino.
    fn todas_las_ordenes(&mut self) -> Vec<String> {
        let mut v: Vec<String> = shell::ordenes::BASE.iter().chain(shell::ordenes::INTERNAS).chain(shell::ordenes::CONTROL).map(|x| String::from(x.0)).collect();
        v.extend(self.p.int.alias.iter().map(|x| x.0.clone()));
        let mut cajones = alloc::vec![String::new()];
        cajones.extend(self.p.camino.iter().cloned());
        cajones.push(String::from("C:"));
        for c in cajones {
            if let Ok(entradas) = self.s.listar(&c) {
                // `Ordenes` es el programa de las de base: no se lanza por ese nombre.
                v.extend(entradas.into_iter().filter(|e| !e.datos.cajon() && (e.datos.modo & 0o111 != 0 || e.datos.bits & 4 != 0) && !(c == "C:" && e.nombre == "Ordenes")).take(500).map(|e| e.nombre));
            }
        }
        v
    }

    // --- Las teclas y el ratón ---

    /// Una tecla en el panel que tiene el teclado, con las de Control, Mayúsculas y Alt como estén.
    fn tecla(&mut self, t: Option<Tecla>, codigo: u32) {
        let (control, mayus, alt) = self.ventana.as_ref().map_or((false, false, false), |v| (v.control > 0, v.mayusculas > 0, v.alt));
        self.tecla_con(t, codigo, control, mayus, alt);
    }

    /// Una tecla en el panel en que se trabaja.
    fn tecla_con(&mut self, t: Option<Tecla>, codigo: u32, control: bool, mayus: bool, alt: bool) {
        // S10: con un menú abierto, las teclas son suyas; F10 los abre (en la barra, se lo pide al compositor).
        if let Some(v) = self.ventana.as_mut()
            && (v.menus.abierto() || (codigo == KEY_F10 && !control && !alt))
        {
            let estaba = v.menus.abierto();
            let r = v.menus.tecla(codigo, true);
            self.tras_menus(r, estaba);
            return;
        }
        // Lo de la ventana, con las teclas de Terminator: partir, pestaña nueva, cerrar, copiar y pegar.
        if control && mayus {
            match codigo {
                // O: partir en horizontal; E: en vertical; T: pestaña; W: cerrar el panel.
                24 => {
                    self.abrir_panel(false, false);
                }
                18 => {
                    self.abrir_panel(true, false);
                }
                20 => {
                    self.abrir_panel(false, true);
                }
                17 => self.p.salir = Some(self.p.int.rc),
                46 => {
                    let texto = self.p.consola.texto_seleccionado();
                    if let Some(v) = &mut self.ventana {
                        let mut e = Escritor::new();
                        v.pp.copiar(&mut e, false, texto, v.serie);
                        self.buzon.mandar(&e);
                    }
                }
                47 => {
                    if let Some(v) = &mut self.ventana {
                        let mut e = Escritor::new();
                        if v.pp.pegar(&mut e, false) {
                            self.buzon.mandar(&e);
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        // Alt+flechas: el teclado pasa al panel de ese lado.
        if alt {
            let hacia = match (codigo, t) {
                (105, _) => Some(Hacia::Izquierda),
                (106, _) => Some(Hacia::Derecha),
                (103, _) | (_, Some(Tecla::AltArriba)) => Some(Hacia::Arriba),
                (108, _) | (_, Some(Tecla::AltAbajo)) => Some(Hacia::Abajo),
                _ => None,
            };
            if let Some(h) = hacia {
                let area = self.area();
                self.disp.mover(h, area);
                if let Some(v) = &mut self.ventana {
                    v.sucia = true;
                }
                return;
            }
        }
        // RePág y AvPág: lo escrito, hacia atrás y hacia delante. Con Control, la pestaña de al lado.
        if codigo == 104 || codigo == 109 {
            if control {
                self.disp.cambiar_pestana(if codigo == 109 { 1 } else { -1 });
                self.colocar();
            } else {
                let paso = self.p.consola.filas().saturating_sub(1).max(1) as isize;
                self.p.consola.desplazar(if codigo == 104 { paso } else { -paso });
            }
            return;
        }
        let Some(t) = t else { return };
        self.p.consola.al_final();
        // Con el menú de completar abierto.
        if self.p.menu.is_some() {
            match t {
                Tecla::Tabulador | Tecla::Abajo | Tecla::Derecha => return self.mover_menu(1),
                Tecla::TabuladorAtras | Tecla::Arriba | Tecla::Izquierda => return self.mover_menu(-1),
                Tecla::Intro => return self.cerrar_menu(true),
                Tecla::Escape => return self.cerrar_menu(false),
                _ => self.cerrar_menu(true),
            }
        }
        // Buscando hacia atrás (Ctrl-R).
        if self.p.consola.editor.buscando().is_some() {
            let ed = &mut self.p.consola.editor;
            match t {
                Tecla::Letra(c) if control && c.eq_ignore_ascii_case(&'r') => return ed.buscar(),
                Tecla::Letra(c) if control && (c.eq_ignore_ascii_case(&'c') || c.eq_ignore_ascii_case(&'g')) => return ed.buscar_fin(true),
                Tecla::Letra(c) if !control => return ed.buscar_letra(Some(c)),
                Tecla::Retroceso => return ed.buscar_letra(None),
                Tecla::Escape => return ed.buscar_fin(true),
                Tecla::Intro => {}
                _ => ed.buscar_fin(false),
            }
        }
        if control {
            if let Tecla::Letra(c) = t {
                let ed = &mut self.p.consola.editor;
                match c.to_ascii_lowercase() {
                    'c' => return self.romper(),
                    'd' | '\\' => return self.fin_de_entrada(),
                    'r' => ed.buscar(),
                    'a' => ed.inicio(),
                    'e' => ed.fin(),
                    'k' => ed.borrar_hasta_el_fin(),
                    'u' | 'x' => ed.borrar(),
                    'w' => ed.borrar_palabra(),
                    'l' => self.p.consola.limpiar(),
                    _ => {}
                }
                return;
            }
            // Ctrl+derecha: una palabra de la sugerencia.
            if t == Tecla::Derecha {
                self.p.consola.editor.aceptar_sugerencia(true);
                return;
            }
        }
        self.p.consola.quitar_seleccion();
        let ed = &mut self.p.consola.editor;
        match t {
            Tecla::Letra(c) => ed.insertar(c),
            Tecla::Retroceso => ed.retroceso(),
            Tecla::Suprimir => ed.suprimir(),
            Tecla::Izquierda => ed.izquierda(),
            Tecla::Derecha => ed.derecha(),
            Tecla::Inicio => ed.inicio(),
            Tecla::Fin => ed.fin(),
            Tecla::Arriba | Tecla::AltArriba => ed.arriba(),
            Tecla::Abajo | Tecla::AltAbajo => ed.abajo(),
            Tecla::Intro => {
                let l = self.p.consola.intro();
                if let Some(g) = self.p.consola.editor.tomar_por_guardar() {
                    let _ = self.escribir_fichero(HISTORIAL, format!("{}\n", g).as_bytes(), true);
                }
                self.linea_tecleada(l);
            }
            Tecla::Tabulador => self.completar(),
            Tecla::TabuladorAtras | Tecla::Escape => {}
        }
    }

    /// El panel (su número) que hay bajo el ratón, si hay.
    fn panel_bajo_el_raton(&self) -> Option<usize> {
        let (x, y) = self.ventana.as_ref()?.raton;
        let area = self.area();
        let (id, _) = self.disp.rects(area, 1).into_iter().find(|(_, r)| r.contiene(x, y))?;
        self.numero_de(id)
    }

    fn punto_del_raton(&self) -> Option<Punto> {
        let (col, fila) = self.ventana.as_ref()?.letra_bajo_el_raton(self.p.rect);
        self.p.consola.punto(col.min(self.p.consola.columnas() - 1), fila)
    }

    fn raton(&mut self, e: Entrada) {
        // S10: los menús de la fila, antes que los paneles.
        if let Some(v) = self.ventana.as_mut().filter(|v| !v.menus_fuera) {
            let (x, y) = v.raton;
            let estaba = v.menus.abierto();
            let r = match e {
                Entrada::Mueve => v.menus.raton(x, y, v.izquierdo),
                Entrada::Boton { boton: 272, pulsado, .. } => v.menus.raton(x, y, pulsado),
                Entrada::Rueda(pasos) if estaba => v.menus.rueda(x, y, pasos),
                _ if estaba => MenuResultado::Hecho,
                _ => MenuResultado::Nada,
            };
            if r != MenuResultado::Nada {
                self.tras_menus(r, estaba);
                return;
            }
        }
        match e {
            Entrada::Mueve => {
                if let (Some(desde), Some(aqui)) = (self.p.arrastre, self.punto_del_raton()) {
                    self.p.consola.seleccionar(desde, aqui);
                    if let Some(v) = &mut self.ventana {
                        v.sucia = true;
                    }
                }
            }
            Entrada::Boton { boton: 272, pulsado: true, .. } => {
                // En la barra de pestañas: la pestaña de ese sitio.
                let (x, y) = self.ventana.as_ref().map_or((0, 0), |v| v.raton);
                if y < self.area().y {
                    let cw = self.ventana.as_ref().map_or(8, |v| v.celda.0);
                    let mut desde = 4;
                    for k in 0..self.disp.pestanas() {
                        let ancho = format!(" {} {} ", k + 1, self.titulo_de_pestana(k)).chars().take(ANCHO_PESTANA).count() as i32 * cw;
                        if x >= desde && x < desde + ancho {
                            self.disp.ir_a_pestana(k);
                            self.colocar();
                            break;
                        }
                        desde += ancho + 4;
                    }
                    return;
                }
                if let Some(k) = self.panel_bajo_el_raton() {
                    let id = self.panel(k).id;
                    self.disp.activar(id);
                    self.entrar(k);
                    self.p.consola.quitar_seleccion();
                    self.p.arrastre = self.punto_del_raton();
                }
                if let Some(v) = &mut self.ventana {
                    v.sucia = true;
                }
            }
            Entrada::Boton { boton: 272, pulsado: false, serie } => {
                if self.p.arrastre.take().is_some() {
                    let texto = self.p.consola.texto_seleccionado();
                    if texto.is_empty() {
                        self.p.consola.quitar_seleccion();
                    } else if let Some(v) = &mut self.ventana {
                        // Al soltar, lo seleccionado es la selección primaria.
                        let mut e = Escritor::new();
                        v.pp.copiar(&mut e, true, texto, serie);
                        self.buzon.mandar(&e);
                    }
                    if let Some(v) = &mut self.ventana {
                        v.sucia = true;
                    }
                }
            }
            Entrada::Boton { boton: 274, pulsado: true, .. } => {
                if let Some(v) = &mut self.ventana {
                    let mut e = Escritor::new();
                    if v.pp.pegar(&mut e, true) {
                        self.buzon.mandar(&e);
                    }
                }
            }
            Entrada::Rueda(pasos) => {
                if let Some(k) = self.panel_bajo_el_raton() {
                    self.entrar(k);
                }
                self.p.consola.desplazar(-3 * pasos as isize);
                if let Some(v) = &mut self.ventana {
                    v.sucia = true;
                }
            }
            _ => {}
        }
    }

    /// S10: lo que dejaron los menús: lo elegido se hace; F10 con los menús en la barra se pide al compositor; y lo
    /// que cambió se pinta (al cerrarse, la ventana entera: lo desplegado tapaba los paneles).
    fn tras_menus(&mut self, r: MenuResultado, estaba: bool) {
        let Some(v) = self.ventana.as_mut() else { return };
        if r == MenuResultado::AbrirFuera && v.con_menus {
            let mut e = Escritor::new();
            e.empezar(MENU_V, dashos_menu_v1::req::ABRIR).fin();
            self.buzon.mandar(&e);
            return;
        }
        if r != MenuResultado::Hecho {
            v.menus_sucios = true;
            v.sucia = true;
            // Con algo desplegado (o que se acaba de cerrar), la ventana entera: lo desplegado tapa los paneles, que
            // solo repintan sus filas cambiadas; si no, quedarían restos del menú de antes.
            if estaba || v.menus.abierto() {
                v.entera = true;
            }
        }
        if let MenuResultado::Elegido(n) = r {
            self.menu_elegido(n);
        }
    }

    /// S10: una entrada de los menús (de la fila, o de la barra por el compositor): lo mismo que su atajo.
    fn menu_elegido(&mut self, n: u32) {
        let titulo = self.ventana.as_ref().and_then(|v| v.menus.datos.buscar(n)).map(|e| e.titulo.clone()).unwrap_or_default();
        println!("  [shell] menú: {}", titulo);
        match n {
            M_PESTANA => self.tecla_con(None, 20, true, true, false),
            M_VERTICAL => self.tecla_con(None, 18, true, true, false),
            M_HORIZONTAL => self.tecla_con(None, 24, true, true, false),
            M_CERRAR => self.tecla_con(None, 17, true, true, false),
            M_COPIAR => self.tecla_con(None, 46, true, true, false),
            M_PEGAR => self.tecla_con(None, 47, true, true, false),
            M_LIMPIAR => {
                self.p.consola.limpiar();
                if let Some(v) = &mut self.ventana {
                    v.sucia = true;
                }
            }
            _ => {}
        }
    }

    /// S10: Copiar, activa solo con algo seleccionado (en la fila y en la barra, al momento).
    fn menus_al_dia(&mut self) {
        let hay = self.p.consola.seleccion().is_some_and(|(a, b)| a != b);
        let Some(v) = self.ventana.as_mut() else { return };
        if v.menus.datos.buscar(M_COPIAR).is_some_and(|e| e.activa != hay) {
            if let Some(e) = v.menus.datos.buscar_mut(M_COPIAR) {
                e.activa = hay;
            }
            v.menus_sucios = true;
            v.sucia = true;
            v.mandar_menus(&mut self.buzon);
        }
    }

    /// Lo que se lee en la pestaña `k`: el cajón actual de su panel con el teclado.
    fn titulo_de_pestana(&self, k: usize) -> String {
        let ids = self.disp.paneles_de(k);
        let id = if k == self.disp.pestana() { self.disp.activo() } else { ids.first().copied().unwrap_or(0) };
        let (cajon, ultima) = match self.numero_de(id) {
            Some(n) if n == self.en => (&self.s.actual, &self.p.ultima),
            Some(n) => (&self.paneles[n].actual, &self.paneles[n].ultima),
            None => return String::new(),
        };
        // El cajón y la última orden tecleada (como Terminator, que pone lo que corre): así dos pestañas en el mismo
        // cajón se distinguen.
        if ultima.is_empty() { cajon.clone() } else { format!("{}{}{}", cajon, SEPARADOR_TITULO, ultima) }
    }

    // --- El bucle ---

    /// Si hay algo en primer plano por lo que esperar.
    fn esperando_a_una_orden(&self) -> bool {
        self.p.trabajos.iter().any(|t| t.primer_plano)
    }

    /// Avanza el intérprete del panel en que se trabaja mientras pueda, sin pasar de unos milisegundos (la ventana
    /// tiene que seguir viva).
    fn avanzar(&mut self) {
        let hasta = dashos::ahora_us() + 4_000;
        for _ in 0..200 {
            if self.p.salir.is_some() || self.esperando_a_una_orden() || self.p.tuberia.is_some() {
                return;
            }
            match self.con_interprete(|i, m| i.paso(m)) {
                Paso::Nada => {}
                Paso::Ejecutar(ordenes) => self.empezar_tuberia(ordenes, false),
                Paso::Capturar(ordenes) => self.empezar_tuberia(ordenes, true),
                Paso::Fin(rc) => {
                    self.p.salir = Some(rc);
                    return;
                }
                Paso::Pedir => {
                    // Un complemento que acabó mal se dice, y se sigue con los demás.
                    if let Some(c) = self.p.cargando.take() {
                        if self.p.int.rc >= shell::RC_ERROR {
                            self.di_linea(&format!("El complemento {} ha fallado (código {}): se salta lo que le quedaba", c, self.p.int.rc));
                            self.p.int.rc = 0;
                        }
                    }
                    if let Some(c) = self.p.complementos.pop_front() {
                        match self.s.leer_entero(&c, ordenes::MAX_ENTERO) {
                            Ok(d) => {
                                let texto = String::from_utf8_lossy(&d).into_owned();
                                self.p.cargando = Some(c.clone());
                                self.con_interprete(|i, m| i.guion(m, &c, &texto, ""));
                            }
                            Err(e) => self.di_linea(&format!("El complemento {} no se puede leer: {}", c, ordenes::texto_error(e))),
                        }
                    } else if let Some(l) = self.p.orden_pendiente.take() {
                        self.con_interprete(|i, _| i.linea(&l));
                    } else if self.p.de_una_orden || self.ventana.is_none() {
                        self.p.salir = Some(self.p.int.rc);
                        return;
                    } else if let Some(l) = self.p.tecleadas.pop_front() {
                        if let Some(n) = shell::linea::nombre_de_orden(&l) {
                            self.p.ultima = n;
                        }
                        self.con_interprete(|i, _| i.linea(&l));
                    } else {
                        if !self.p.indicador_puesto {
                            self.p.indicador_puesto = true;
                            self.poner_indicador();
                        }
                        return;
                    }
                }
            }
            if dashos::ahora_us() > hasta {
                return;
            }
        }
    }

    fn bucle(&mut self) -> i32 {
        loop {
            // Cada panel avanza lo suyo; el que acaba se quita (el último, con la ventana).
            let mut k = 0;
            while k < self.paneles.len() {
                self.entrar(k);
                self.avanzar();
                if let Some(rc) = self.p.salir {
                    // Un Shell de una orden (`ORDEN`) acaba entero con ella, tenga los paneles que tenga.
                    if self.p.de_una_orden || !self.quitar_panel(k) {
                        return rc;
                    }
                    continue;
                }
                k += 1;
            }
            if let Some(rc) = self.fin {
                return rc;
            }
            // Cuánto esperar: nada si algún intérprete tiene más que hacer; poco con trabajos (hay que vigilarlos).
            let con_prisa = (0..self.paneles.len()).any(|k| {
                let p = self.panel(k);
                p.int.ocupado() && !p.trabajos.iter().any(|t| t.primer_plano) && p.tuberia.is_none()
            });
            let con_trabajos = (0..self.paneles.len()).any(|k| !self.panel(k).trabajos.is_empty() || self.panel(k).por_mirar);
            let plazo = if con_prisa {
                1
            } else if con_trabajos || self.ventana.as_ref().is_some_and(|v| v.pp.ocupado() || (v.sucia && v.esperando != 0)) {
                20_000
            } else {
                500_000
            };
            let mut llego = self.buzon.recoger(plazo);
            for _ in 0..32 {
                if !self.buzon.recoger(1) {
                    break;
                }
                llego = true;
            }
            // Lo de la ventana: al panel que tiene el teclado.
            while let Some((o, orden_, cuerpo)) = self.buzon.evento() {
                let Some(v) = &mut self.ventana else { continue };
                let entrada = v.evento(&mut self.buzon, o, orden_, &cuerpo);
                let activo = self.activo();
                self.entrar(activo);
                match entrada {
                    Entrada::Nada => {}
                    Entrada::Tecla { t, codigo } => {
                        self.teclas_reales += 1;
                        self.tecla(t, codigo)
                    }
                    Entrada::Medida => {
                        let primera = self.p.rect.w == 0;
                        self.colocar();
                        if let (Some(v), true) = (&self.ventana, primera || self.registro) {
                            println!(
                                "  [shell] ventana {}: {}x{} letras, {}x{} px{}",
                                if primera { "en pantalla" } else { "con otro tamaño" },
                                self.p.consola.columnas(),
                                self.p.consola.filas(),
                                v.medida.0,
                                v.medida.1,
                                if primera && v.con_portapapeles { ", con portapapeles" } else { "" }
                            );
                            if primera {
                                v.decir_menus();
                            }
                        }
                    }
                    // S10: los menús cambian de sitio: la fila sale o se va, y los paneles se recolocan.
                    Entrada::Sitio => {
                        self.colocar();
                        if let Some(v) = &self.ventana {
                            println!("  [shell] menús: {}", if v.menus_fuera { "en la barra" } else { "dentro de la ventana" });
                            v.decir_menus();
                        }
                    }
                    Entrada::Menu(n) => self.menu_elegido(n),
                    Entrada::Cerrar => {
                        // Cerrar la ventana (su botón en la barra) es cerrar todos sus paneles: sus trabajos se matan
                        // sin preguntar, como al cerrar un panel (decisión 7).
                        let mut matados = 0;
                        for k in 0..self.paneles.len() {
                            self.entrar(k);
                            matados += self.p.trabajos.len();
                            self.matar_trabajos();
                        }
                        println!("  [shell] ventana cerrada desde su barra; trabajos matados: {}", matados);
                        return self.p.int.rc;
                    }
                    e => {
                        self.raton_real += 1;
                        self.raton(e)
                    }
                }
            }
            if self.ventana.is_some() && self.buzon.cerrado {
                println!("  [shell] el compositor cerró la conexión");
                return RC_FALLO;
            }
            // S10: los menús, al día con lo que hay (Copiar, con algo seleccionado).
            self.menus_al_dia();
            // Lo pegado, como si se tecleara en el panel con el teclado.
            let pegado = self.ventana.as_mut().and_then(|v| v.pp.mover());
            if let Some(datos) = pegado {
                let activo = self.activo();
                self.entrar(activo);
                let texto = String::from_utf8_lossy(&datos).into_owned();
                self.cerrar_menu(false);
                self.p.consola.al_final();
                for l in self.p.consola.pegar(&texto) {
                    self.linea_tecleada(l);
                }
                if let Some(v) = &mut self.ventana {
                    v.sucia = true;
                }
            }
            // Lo de los trabajos, cada uno en su panel.
            while let Some((marca, m)) = self.buzon.peticiones.pop_front() {
                if let Some(k) = self.panel_del_trabajo(marca) {
                    self.entrar(k);
                    self.peticion(marca, &m);
                }
            }
            let ahora = dashos::ahora_us();
            if !llego && con_trabajos && ahora.saturating_sub(self.vigilado_us) > 100_000 {
                self.vigilado_us = ahora;
                for k in 0..self.paneles.len() {
                    self.entrar(k);
                    self.vigilar();
                }
            }
            // Sin ventana: de vez en cuando se pregunta a quien nos lanzó si nos han roto.
            if self.padre.is_some() && ahora.saturating_sub(self.pulso_us) > 100_000 {
                self.pulso_us = ahora;
                let c = self.padre.as_mut().unwrap();
                if c.pedir(p::PULSO, &[], 60_000_000).is_none() {
                    return RC_FALLO;
                }
                if c.roto {
                    self.romper();
                    // Lo que quede en marcha se mata al salir; el código, el de roto.
                    for t in &self.p.trabajos {
                        matar(t.pid);
                    }
                    return p::RC_ROTO;
                }
            }
            if self.ventana.is_some() {
                // El resaltado y la sugerencia del panel con el teclado, y a pintar.
                let activo = self.activo();
                self.entrar(activo);
                self.resaltar();
                self.columnas = self.p.consola.columnas();
                let pestanas: Vec<(String, bool)> = (0..self.disp.pestanas()).map(|k| (self.titulo_de_pestana(k), k == self.disp.pestana())).collect();
                // El título de la ventana, el de la pestaña con el teclado (lo enseñan su barra y la de la pantalla).
                let titulo = pestanas.iter().find(|p| p.1).map(|p| p.0.as_str()).filter(|t| !t.is_empty()).unwrap_or(TITULO);
                if let Some(v) = &mut self.ventana
                    && v.titulo != titulo
                {
                    v.titulo = String::from(titulo);
                    let mut e = Escritor::new();
                    e.empezar(VENTANA, xdg_toplevel::req::SET_TITLE).texto(Some(titulo)).fin();
                    self.buzon.mandar(&e);
                }
                let area = self.area();
                let activo_id = self.disp.activo();
                let rects = self.disp.rects(area, 1);
                let vistas: Vec<Vista> = rects
                    .iter()
                    .filter_map(|&(id, rect)| {
                        let k = (0..self.paneles.len()).find(|&k| if k == self.en { self.p.id == id } else { self.paneles[k].id == id })?;
                        let consola = if k == self.en { &self.p.consola } else { &self.paneles[k].consola };
                        Some(Vista { id, rect, consola, activo: id == activo_id })
                    })
                    .collect();
                if let Some(v) = &mut self.ventana {
                    let t0 = dashos::ahora_us();
                    if v.pintar(&mut self.buzon, &vistas, &pestanas) {
                        self.medido_cuadros += 1;
                        self.medido_pintar += dashos::ahora_us() - t0;
                    }
                }
            }
        }
    }
}

/// Para completar: de dónde salen las órdenes, los nombres y lo que hay en un cajón.
struct FuenteShell<'a> {
    shell: &'a mut Shell,
}

impl completar::Fuente for FuenteShell<'_> {
    fn ordenes(&mut self) -> Vec<String> {
        self.shell.todas_las_ordenes()
    }

    fn nombres(&mut self) -> Vec<String> {
        self.shell.s.tabla.lineas().map(|l| String::from_utf8_lossy(l.nombre).into_owned()).collect()
    }

    fn listar(&mut self, cajon: &str) -> Vec<(String, bool)> {
        self.shell.s.listar(cajon).map(|v| v.into_iter().take(2000).map(|e| (e.nombre, e.datos.cajon())).collect()).unwrap_or_default()
    }

    fn plantilla(&mut self, orden_: &str) -> Option<String> {
        self.shell.p.plantillas.iter().find(|x| x.0.eq_ignore_ascii_case(orden_)).map(|x| x.1.clone())
    }
}

/// Para resaltar: si una orden o una ruta existen (con memoria y con tope de miradas al disco).
struct MiraShell<'a> {
    shell: &'a mut Shell,
}

impl resaltar::Mira for MiraShell<'_> {
    fn orden(&mut self, nombre: &str) -> Option<bool> {
        let sh = &mut *self.shell;
        if shell::ordenes::es_interna(nombre) || ordenes::base::es_de_base(nombre) || sh.p.int.alias.iter().any(|x| x.0.eq_ignore_ascii_case(nombre)) {
            return Some(true);
        }
        if nombre.len() > 255 {
            return Some(false);
        }
        let clave = format!("o:{}:{}", sh.s.actual, nombre);
        let nombre = String::from(nombre);
        sh.visto(clave, |sh| sh.buscar(&nombre).is_ok())
    }

    fn ruta(&mut self, ruta_: &str) -> Option<bool> {
        let sh = &mut *self.shell;
        if ruta_.is_empty() || ruta_.len() > 255 || shell::patron::es_patron(ruta_) {
            return Some(false);
        }
        let clave = format!("r:{}:{}", sh.s.actual, ruta_);
        let ruta_ = String::from(ruta_);
        sh.visto(clave, |sh| sh.s.mirar(&ruta_).is_ok())
    }

    fn plantilla(&mut self, orden_: &str) -> Option<String> {
        self.shell.p.plantillas.iter().find(|x| x.0.eq_ignore_ascii_case(orden_)).map(|x| x.1.clone())
    }
}

/// Un mundo que no dice nada: para meter el guion de arranque en un intérprete recién hecho.
struct Mudo;

impl Mundo for Mudo {
    fn decir(&mut self, _: &str) {}

    fn leer_guion(&mut self, _: &str) -> Result<String, i32> {
        Err(a::NO_EXISTE)
    }

    fn existe(&mut self, _: &str) -> bool {
        false
    }

    fn env(&mut self, _: &str) -> Option<String> {
        None
    }
}

/// Pide a `sesiones` que mate el proceso `pid` (uno nuestro): su estado.
fn matar(pid: u64) -> i32 {
    let mut m = [0u8; 12];
    m[0..4].copy_from_slice(&ps::MATAR.to_le_bytes());
    m[4..12].copy_from_slice(&pid.to_le_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let n = if dashos::canal_escribir(SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], PLAZO_SESIONES_US).0 } else { -1 };
    dashos::cerrar(lectura);
    if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
}

fn principal() -> u64 {
    let arr = ordenes::arranque();
    let Some(mut s) = Sistema::nuevo() else {
        println!("  [shell] sin el servicio de archivos");
        return RC_FALLO as u64;
    };
    let (mut args, mut padre, mut columnas) = (String::new(), None, COLUMNAS);
    if let Some(x) = arr {
        if !x.actual.is_empty() {
            s.actual = x.actual;
        }
        args = x.args;
        padre = x.puerta.map(Cli::nuevo);
        if x.columnas > 0 {
            columnas = x.columnas;
        }
    }
    // El perfil de la ventana (`ENVARC:Shell/Perfil`): lo que no se entienda se dirá en el primer panel.
    let mut avisos = Vec::new();
    let perfil_ = match s.leer_entero(PERFIL, 4096) {
        Ok(d) => perfil::Perfil::leer(&d, &mut avisos),
        Err(_) => perfil::Perfil::default(),
    };
    let (mut buzon, suyo) = Buzon::nuevo();
    let ventana = if padre.is_some() {
        dashos::cerrar(suyo);
        None
    } else {
        Ventana::abrir(&mut buzon, suyo, &perfil_)
    };
    let actual = s.actual.clone();
    let mut sh = Shell {
        s,
        buzon,
        ventana,
        padre,
        registro: false,
        registro_callado: false,
        para_registro: String::new(),
        p: Panel::nuevo(0, &actual, columnas, 25),
        paneles: Vec::new(),
        en: 0,
        disp: Disposicion::nueva(1),
        siguiente_panel: 1,
        siguiente_id: 1,
        guardados: Vec::new(),
        indicador: indicador::Ajustes::default(),
        vistos: Vec::new(),
        cupo_de_miradas: 0,
        sin_arranque: false,
        desde: None,
        fin: None,
        vigilado_us: 0,
        pulso_us: dashos::ahora_us(),
        columnas,
        medido_desde: dashos::ahora_us(),
        medido_bytes: 0,
        medido_escribir: 0,
        medido_cuadros: 0,
        medido_pintar: 0,
        teclas_reales: 0,
        raton_real: 0,
    };
    let (mut orden_pendiente, mut mal) = (None, None);
    match plantilla::leer("FROM/K,REGISTRO/S,SINARRANQUE/S,ORDEN/K/F", &args) {
        Ok(x) => {
            sh.registro = x.si("REGISTRO");
            sh.sin_arranque = x.si("SINARRANQUE");
            sh.desde = x.texto("FROM").map(String::from);
            orden_pendiente = x.texto("ORDEN").map(String::from);
        }
        Err(e) => mal = Some(format!("Shell: {}", e)),
    }
    if sh.ventana.is_none() && sh.padre.is_none() {
        println!("  [shell] sin ventana: no hay compositor");
        if orden_pendiente.is_none() {
            return RC_FALLO as u64;
        }
    }
    // El primer panel (el 1), con su arranque; va en `p` y su hueco, en la lista.
    let primero = sh.panel_nuevo(&actual);
    sh.paneles.push(core::mem::replace(&mut sh.p, primero));
    sh.p.de_una_orden = orden_pendiente.is_some();
    sh.p.orden_pendiente = orden_pendiente;
    if sh.ventana.is_some() {
        sh.leer_indicador();
    }
    for aviso in avisos {
        sh.di_linea(&format!("{}: {}", PERFIL, aviso));
    }
    if let Some(m) = mal {
        sh.di_linea(&m);
    }
    let rc = sh.bucle();
    if sh.registro {
        println!("  [shell] del compositor llegaron {} teclas y {} eventos del ratón", sh.teclas_reales, sh.raton_real);
        println!("  [shell] acaba con el código {}", rc);
    }
    if let Some(c) = &sh.padre {
        c.salir(rc, sh.p.int.resultado2);
    }
    rc as u64
}
