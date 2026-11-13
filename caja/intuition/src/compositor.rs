//! El puente con el compositor de DashOS (S4 §3, S10b, S23): cada ventana de Intuition es una ventana del
//! escritorio. La caja implementa `Compositor` sobre Wayland: una superficie `xdg_toplevel` por ventana, con un
//! búfer `wl_shm` ARGB8888 del tamaño de la ventana entera (bordes de Intuition incluidos), `wl_surface.damage`
//! con lo que cambió, y los eventos de `wl_pointer`, `wl_keyboard` y `xdg_toplevel` de vuelta.
//!
//! Los adornos (provisional, S4 "Lo que queda abierto" 1 y S23 "Workbench"): por dentro, los de Intuition
//! (barra de título, cerrar, profundidad, tamaño) dibujados en el búfer de la ventana, como en el Amiga; por
//! fuera, el borde del tema de DashOS que ponga el compositor. Con el tema "Amiga clásico", el compositor no pone
//! borde. Los gadgets de Intuition piden al compositor lo que harían en el Amiga: arrastrar, cambiar el tamaño,
//! pasar al frente o al fondo.
//!
//! `CompositorDePrueba` es uno de mentira para las pruebas: guarda los búferes y deja meter eventos.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::lienzo::Rect;

/// El número de una ventana en el compositor.
pub type Superficie = u32;

/// Botones de `wl_pointer.button` (los de evdev).
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// Lo que llega del compositor para una ventana. Las posiciones son de la superficie: (0, 0) es la esquina de la
/// ventana de Intuition, borde incluido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evento {
    /// El ratón se ha movido (`wl_pointer.motion`).
    Raton { x: i32, y: i32 },
    /// Un botón (`wl_pointer.button`, con el código de evdev: `BTN_LEFT`...).
    Boton { boton: u32, pulsado: bool },
    /// Una tecla (`wl_keyboard.key`, con el código de evdev). `caracter` es lo que da la distribución del
    /// teclado con los modificadores de ahora (xkb, en la caja), si da algo; sin él, se usa una distribución
    /// estadounidense. `repeticion` si es la repetición automática.
    Tecla { codigo: u32, pulsada: bool, repeticion: bool, caracter: Option<char> },
    /// La ventana gana o pierde el foco del teclado.
    Foco(bool),
    /// El compositor le da otro tamaño (`xdg_toplevel.configure`), borde de Intuition incluido.
    Tamano { ancho: u32, alto: u32 },
    /// El compositor la ha movido (en coordenadas de la pantalla del Amiga).
    Posicion { x: i32, y: i32 },
    /// El usuario la quiere cerrar desde fuera (`xdg_toplevel.close`, el borde de DashOS).
    Cerrar,
    /// El compositor ha perdido el búfer: hay que mandarlo entero.
    Exponer,
}

/// Lo que se pide al abrir una ventana.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NuevaVentana {
    pub titulo: String,
    pub x: i32,
    pub y: i32,
    pub ancho: u32,
    pub alto: u32,
    pub activa: bool,
    /// Sin borde de DashOS (las `BORDERLESS` y las de fondo).
    pub sin_borde: bool,
    /// Tamaños mínimo y máximo (`WindowLimits`).
    pub minimo: (u32, u32),
    pub maximo: (u32, u32),
}

/// Los colores del tema de DashOS (Material 3) para pintar los controles de un programa Amiga "como DashOS" (S26):
/// los papeles de Material que manda el compositor, ya en ARGB. Los saca la caja del tema que llega del fondo
/// (`caja/pegamento/src/tema.rs`); aquí no se escribe ningún color a mano salvo `CLARO`, el de serie de Material 3
/// (acento 6750a4), para las pruebas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub primario: u32,
    pub sobre_primario: u32,
    pub contenedor_primario: u32,
    pub sobre_contenedor_primario: u32,
    pub superficie: u32,
    pub sobre_superficie: u32,
    pub variante_superficie: u32,
    pub sobre_variante_superficie: u32,
    pub contorno: u32,
    pub variante_contorno: u32,
    /// Los contenedores de superficie (bajo, normal y alto): menús, barra de título, campos de solo lectura.
    pub contenedor_bajo: u32,
    pub contenedor: u32,
    pub contenedor_alto: u32,
    /// El acento suave: la fila elegida de una lista, un botón tonal, el elemento señalado de un menú.
    pub contenedor_secundario: u32,
    pub sobre_contenedor_secundario: u32,
}

impl Tokens {
    /// El esquema claro de serie de Material 3 (el del kit con el acento 6750a4). Solo para las pruebas.
    pub const CLARO: Tokens = Tokens {
        primario: 0xff67_50a4,
        sobre_primario: 0xffff_ffff,
        contenedor_primario: 0xffea_ddff,
        sobre_contenedor_primario: 0xff21_005d,
        superficie: 0xfffe_f7ff,
        sobre_superficie: 0xff1d_1b20,
        variante_superficie: 0xffe7_e0ec,
        sobre_variante_superficie: 0xff49_454f,
        contorno: 0xff79_747e,
        variante_contorno: 0xffca_c4d0,
        contenedor_bajo: 0xfff9_f2fa,
        contenedor: 0xfff3_edf5,
        contenedor_alto: 0xffee_e7ef,
        contenedor_secundario: 0xffe6_ddf1,
        sobre_contenedor_secundario: 0xff1d_1b20,
    };
}

/// El puente. Todo lo que no es crear, cerrar y presentar tiene una versión por defecto que no hace nada.
pub trait Compositor {
    /// El tamaño del escritorio, que es el de la pantalla de Workbench (S23), en píxeles lógicos.
    fn tamano_escritorio(&self) -> (u32, u32);
    /// Una ventana nueva; `None` si no puede.
    fn crear_ventana(&mut self, v: &NuevaVentana) -> Option<Superficie>;
    fn cerrar_ventana(&mut self, s: Superficie);
    /// Manda los píxeles: `pix` es la ventana entera (`ancho` x `alto`, fila a fila) y `dano` lo que cambió.
    fn presentar(&mut self, s: Superficie, pix: &[u32], ancho: u32, alto: u32, dano: Rect);
    fn poner_titulo(&mut self, _s: Superficie, _titulo: &str) {}
    /// El programa la mueve (`MoveWindow`, `ChangeWindowBox`): el compositor contesta con `Evento::Posicion`.
    fn mover(&mut self, _s: Superficie, _x: i32, _y: i32) {}
    /// El programa le pide otro tamaño (`SizeWindow`, `ChangeWindowBox`): el compositor contesta con
    /// `Evento::Tamano` (o no, si no la deja).
    fn pedir_tamano(&mut self, _s: Superficie, _ancho: u32, _alto: u32) {}
    fn limites(&mut self, _s: Superficie, _minimo: (u32, u32), _maximo: (u32, u32)) {}
    fn al_frente(&mut self, _s: Superficie) {}
    fn al_fondo(&mut self, _s: Superficie) {}
    /// El gadget de profundidad: al fondo si está delante, al frente si no.
    fn cambiar_profundidad(&mut self, _s: Superficie) {}
    fn activar(&mut self, _s: Superficie) {}
    /// Pulsado en la barra de arrastre: que el compositor la arrastre (`xdg_toplevel.move`).
    fn arrastrar(&mut self, _s: Superficie) {}
    /// Pulsado en el gadget de tamaño (`xdg_toplevel.resize` desde la esquina de abajo a la derecha).
    fn redimensionar(&mut self, _s: Superficie) {}
    /// `DisplayBeep`.
    fn pitido(&mut self) {}
    /// Los eventos que han llegado desde la última vez.
    fn sacar_eventos(&mut self) -> Vec<(Superficie, Evento)> {
        Vec::new()
    }
    /// S26: los colores de DashOS si el programa `programa` (el nombre de su tarea) va "como DashOS"; `None` si va
    /// con su aspecto original (o aún no se sabe el tema).
    fn tema_amiga(&self, _programa: &str) -> Option<Tokens> {
        None
    }
    /// Cambia cada vez que cambia el tema o el ajuste: intuition repinta entonces sus ventanas.
    fn version_tema(&self) -> u32 {
        0
    }
}

/// Una ventana del compositor de prueba.
#[derive(Clone, Debug)]
pub struct VentanaDePrueba {
    pub pedida: NuevaVentana,
    pub titulo: String,
    pub x: i32,
    pub y: i32,
    pub ancho: u32,
    pub alto: u32,
    /// Lo último que se mandó con `presentar` (las zonas dañadas copiadas sobre lo de antes).
    pub pix: Vec<u32>,
    pub danos: Vec<Rect>,
    pub cerrada: bool,
}

impl VentanaDePrueba {
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        if x >= self.ancho || y >= self.alto {
            return 0;
        }
        self.pix.get((y * self.ancho + x) as usize).copied().unwrap_or(0)
    }
}

/// Un compositor de mentira: guarda las ventanas y sus búferes, apunta lo que se le pide y deja meter eventos.
/// `pedir_tamano` y `mover` contestan enseguida con su evento, como lo haría uno que los acepta.
#[derive(Clone, Debug)]
pub struct CompositorDePrueba {
    pub escritorio: (u32, u32),
    pub ventanas: BTreeMap<Superficie, VentanaDePrueba>,
    pub eventos: Vec<(Superficie, Evento)>,
    /// Lo que se le ha pedido, en orden ("frente 1", "arrastrar 2"...).
    pub registro: Vec<String>,
    pub pitidos: u32,
    /// Si es `false`, `crear_ventana` falla.
    pub acepta: bool,
    /// S26: el tema de DashOS (`None`: todos con el aspecto original) y los programas que van con el original.
    pub tema: Option<Tokens>,
    pub originales: Vec<String>,
    pub version_tema: u32,
    siguiente: Superficie,
}

impl CompositorDePrueba {
    pub fn nuevo(ancho: u32, alto: u32) -> CompositorDePrueba {
        CompositorDePrueba {
            escritorio: (ancho, alto),
            ventanas: BTreeMap::new(),
            eventos: Vec::new(),
            registro: Vec::new(),
            pitidos: 0,
            acepta: true,
            tema: None,
            originales: Vec::new(),
            version_tema: 0,
            siguiente: 1,
        }
    }
    /// Mete un evento para una ventana.
    pub fn meter(&mut self, s: Superficie, e: Evento) {
        self.eventos.push((s, e));
    }
    pub fn ventana(&self, s: Superficie) -> &VentanaDePrueba {
        &self.ventanas[&s]
    }
    fn apuntar(&mut self, que: &str, s: Superficie) {
        self.registro.push(alloc::format!("{que} {s}"));
    }
}

impl Compositor for CompositorDePrueba {
    fn tamano_escritorio(&self) -> (u32, u32) {
        self.escritorio
    }
    fn crear_ventana(&mut self, v: &NuevaVentana) -> Option<Superficie> {
        if !self.acepta {
            return None;
        }
        let s = self.siguiente;
        self.siguiente += 1;
        self.ventanas.insert(
            s,
            VentanaDePrueba {
                pedida: v.clone(),
                titulo: v.titulo.clone(),
                x: v.x,
                y: v.y,
                ancho: v.ancho,
                alto: v.alto,
                pix: vec![0; v.ancho as usize * v.alto as usize],
                danos: Vec::new(),
                cerrada: false,
            },
        );
        self.apuntar("crear", s);
        Some(s)
    }
    fn cerrar_ventana(&mut self, s: Superficie) {
        if let Some(v) = self.ventanas.get_mut(&s) {
            v.cerrada = true;
        }
        self.apuntar("cerrar", s);
    }
    fn presentar(&mut self, s: Superficie, pix: &[u32], ancho: u32, alto: u32, dano: Rect) {
        let Some(v) = self.ventanas.get_mut(&s) else { return };
        if v.ancho != ancho || v.alto != alto || v.pix.len() != pix.len() {
            v.ancho = ancho;
            v.alto = alto;
            v.pix = vec![0; pix.len()];
        }
        let d = dano.interseccion(&Rect::de_tamano(ancho, alto));
        if !d.vacio() {
            for y in d.y0..=d.y1 {
                let i = (y as u32 * ancho) as usize;
                v.pix[i + d.x0 as usize..=i + d.x1 as usize]
                    .copy_from_slice(&pix[i + d.x0 as usize..=i + d.x1 as usize]);
            }
        }
        v.danos.push(dano);
    }
    fn poner_titulo(&mut self, s: Superficie, titulo: &str) {
        if let Some(v) = self.ventanas.get_mut(&s) {
            v.titulo = String::from(titulo);
        }
        self.apuntar("titulo", s);
    }
    fn mover(&mut self, s: Superficie, x: i32, y: i32) {
        if let Some(v) = self.ventanas.get_mut(&s) {
            v.x = x;
            v.y = y;
        }
        self.eventos.push((s, Evento::Posicion { x, y }));
        self.apuntar("mover", s);
    }
    fn pedir_tamano(&mut self, s: Superficie, ancho: u32, alto: u32) {
        self.eventos.push((s, Evento::Tamano { ancho, alto }));
        self.apuntar("tamano", s);
    }
    fn limites(&mut self, s: Superficie, _minimo: (u32, u32), _maximo: (u32, u32)) {
        self.apuntar("limites", s);
    }
    fn al_frente(&mut self, s: Superficie) {
        self.apuntar("frente", s);
    }
    fn al_fondo(&mut self, s: Superficie) {
        self.apuntar("fondo", s);
    }
    fn cambiar_profundidad(&mut self, s: Superficie) {
        self.apuntar("profundidad", s);
    }
    fn activar(&mut self, s: Superficie) {
        self.apuntar("activar", s);
    }
    fn arrastrar(&mut self, s: Superficie) {
        self.apuntar("arrastrar", s);
    }
    fn redimensionar(&mut self, s: Superficie) {
        self.apuntar("redimensionar", s);
    }
    fn pitido(&mut self) {
        self.pitidos += 1;
    }
    fn sacar_eventos(&mut self) -> Vec<(Superficie, Evento)> {
        core::mem::take(&mut self.eventos)
    }
    fn tema_amiga(&self, programa: &str) -> Option<Tokens> {
        self.tema.filter(|_| !self.originales.iter().any(|p| p == programa))
    }
    fn version_tema(&self) -> u32 {
        self.version_tema
    }
}
