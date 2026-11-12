//! La consola del kernel en la pantalla del firmware y el diagnóstico del primer arranque en una placa nueva (opción
//! `diagnostico`; la lleva la imagen de la Pi 5, `docs/PI5_PRIMER_ARRANQUE.md`).
//!
//! Sin cable serie, lo único que se ve de una placa nueva es la pantalla. Arriba, un panel con letra grande y lo que hay
//! que saber de la máquina (placa, CPU, si hay big-endian en EL0 y si funciona de verdad, memoria, la caja Amiga); abajo,
//! todo lo que escriben el kernel y los programas, como en el puerto serie. Pensado para leerlo en una foto.
//!
//! La pantalla: la de un nodo `simple-framebuffer` del árbol (el cargador UEFI, QEMU `virt`) o, si no hay, la que se
//! pide al firmware de la Pi por el buzón (`base::pantalla`). La letra: Roboto Mono en mapa de bits de 12 x 24
//! (`tools/fuente_consola.py`, OFL 1.1).

use crate::base::pantalla as hw;
use crate::cerrojo::Cerrojo;
use crate::dtb::Dtb;
use core::fmt::{self, Write};

static FUENTE: &[u8] = include_bytes!("../fuentes/consola-12x24.bin");
const AN: u32 = 12;
const AL: u32 = 24;

// Colores que se leen igual si el firmware cambia el rojo por el azul (el verde y el magenta no cambian).
pub const TEXTO: u32 = 0xd8d8d8;
pub const TITULO: u32 = 0xffffff;
pub const BIEN: u32 = 0x30ff30;
pub const MAL: u32 = 0xff40ff;
pub const TENUE: u32 = 0x9a9a9a;
const FONDO: u32 = 0x000000;
const FONDO_PANEL: u32 = 0x202020;

/// Las líneas del panel. Las primeras las pone el kernel por su número; las de la caja Amiga se fijan solas desde la
/// salida de los programas (`fijar`).
pub const L_TITULO: usize = 0;
pub const L_PLACA: usize = 1;
pub const L_CPU: usize = 2;
pub const L_MMFR0: usize = 3;
pub const L_BIG_ENDIAN: usize = 4;
pub const L_MEMORIA: usize = 5;
pub const L_SISTEMA: usize = 6;
pub const L_CAJA: usize = 7;
const PRIMERA_FIJADA: usize = 8;
const LINEAS_PANEL: usize = 14;
const LARGO: usize = 160;

#[derive(Clone, Copy)]
struct Linea {
    b: [u8; LARGO],
    n: usize,
    color: u32,
}

impl Linea {
    const VACIA: Linea = Linea { b: [0; LARGO], n: 0, color: TEXTO };
}

fn linea(color: u32, texto: &str) -> Linea {
    let mut l = Linea::VACIA;
    let _ = l.write_str(texto);
    l.color = color;
    l
}

impl Write for Linea {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            if self.n < LARGO {
                self.b[self.n] = latin1(c);
                self.n += 1;
            }
        }
        Ok(())
    }
}

/// La letra de `c` en la fuente (Latin-1); lo que no está, con un parecido o "?".
fn latin1(c: char) -> u8 {
    match c {
        '\u{20}'..='\u{7e}' | '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '│' | '┃' => b'|',
        '—' | '–' | '─' => b'-',
        '→' => b'>',
        '“' | '”' => b'"',
        '‘' | '’' => b'\'',
        '…' => b'.',
        _ => b'?',
    }
}

struct Estado {
    activa: bool,
    ancho: u32,
    alto: u32,
    esc_panel: u32,
    esc_log: u32,
    panel: [Linea; LINEAS_PANEL],
    /// Dónde empieza el registro, y el cursor en él (columna y fila).
    y_log: u32,
    col: u32,
    fila: u32,
    /// La línea del registro en curso, para fijar en el panel las que importan.
    actual: Linea,
    fijadas: usize,
}

static ESTADO: Cerrojo<Estado> = Cerrojo::new(Estado {
    activa: false,
    ancho: 0,
    alto: 0,
    esc_panel: 2,
    esc_log: 2,
    panel: [Linea::VACIA; LINEAS_PANEL],
    y_log: 0,
    col: 0,
    fila: 0,
    actual: Linea::VACIA,
    fijadas: 0,
});

/// Las escalas van en medios: 2 es la letra tal cual (12 x 24), 3 es una vez y media (18 x 36), 4 el doble.
fn ancho_letra(e: u32) -> u32 {
    AN * e / 2
}

fn alto_letra(e: u32) -> u32 {
    AL * e / 2
}

fn glifo(b: u8) -> [u16; AL as usize] {
    let mut g = [0u16; AL as usize];
    let i = (b.max(0x20) - 0x20) as usize * AL as usize * 2;
    for (k, f) in g.iter_mut().enumerate() {
        *f = u16::from_le_bytes([FUENTE.get(i + 2 * k).copied().unwrap_or(0), FUENTE.get(i + 2 * k + 1).copied().unwrap_or(0)]);
    }
    g
}

impl Estado {
    fn cols(&self, esc: u32) -> u32 {
        self.ancho * 2 / (AN * esc)
    }

    fn filas_log(&self) -> u32 {
        (self.alto.saturating_sub(self.y_log)) / alto_letra(self.esc_log)
    }

    fn pintar_panel(&self, i: usize) {
        let e = self.esc_panel;
        let (an, al) = (ancho_letra(e), alto_letra(e));
        let y = i as u32 * al + al / 4;
        let l = &self.panel[i];
        let fondo = if i == L_TITULO { 0x303030 } else { FONDO_PANEL };
        hw::rellenar(0, y, self.ancho, al, fondo);
        for (k, &b) in l.b[..l.n].iter().enumerate().take((self.cols(e) as usize).saturating_sub(1)) {
            hw::glifo(an / 2 + k as u32 * an, y, &glifo(b), AN, an, al, l.color, fondo);
        }
    }

    fn nueva_linea(&mut self) {
        self.col = 0;
        self.fila += 1;
        if self.fila >= self.filas_log() {
            let h = alto_letra(self.esc_log);
            hw::subir(self.y_log, self.y_log + self.filas_log() * h, h, FONDO);
            self.fila = self.filas_log().saturating_sub(1);
        }
    }

    fn letra(&mut self, b: u8) {
        if self.col >= self.cols(self.esc_log) {
            self.nueva_linea();
        }
        let (an, al) = (ancho_letra(self.esc_log), alto_letra(self.esc_log));
        let color = if self.actual.n <= 2 && b == b'|' { TENUE } else { TEXTO };
        hw::glifo(self.col * an, self.y_log + self.fila * al, &glifo(b), AN, an, al, color, FONDO);
        self.col += 1;
    }

    /// Al acabar una línea del registro: las que dicen cómo va la caja Amiga (y los pánicos) suben al panel.
    fn fijar_si_importa(&mut self) {
        let l = self.actual;
        let t = &l.b[..l.n];
        let tiene = |p: &str| contiene(t, p);
        // La cabecera de la caja dice cómo va.
        if tiene("[caja-amiga] JIT de Emu68") {
            self.panel[L_CAJA] = linea(TITULO, "Caja Amiga: el JIT de Emu68 arrancó en un proceso big-endian");
            self.pintar_panel(L_CAJA);
        } else if tiene("[caja-amiga] saludo.exe acab") {
            self.panel[L_CAJA] = if tiene("bien") {
                linea(BIEN, "Caja Amiga: saludo.exe BIEN (ventana.exe y hostil.exe piden escritorio: aquí fallan, es lo esperado)")
            } else {
                linea(MAL, "Caja Amiga: saludo.exe MAL")
            };
            self.pintar_panel(L_CAJA);
        }
        let caja = tiene("[caja-amiga]")
            && ["el 68k dio", "saludo.exe: ", "saludo.exe acab", "hunk: hola", "hunk: 168", "no arrancan", "sin memoria para"]
                .iter()
                .any(|p| tiene(p));
        if caja || tiene("caja-amiga acab") || tiene("inicio: caja-amiga no se lanza") || tiene("PÁNICO") {
            // Sin la marca "| ", los espacios del principio ni "[caja-amiga] ".
            let mut ini = 0;
            while ini < l.n && (l.b[ini] == b'|' || l.b[ini] == b' ') {
                ini += 1;
            }
            if l.b[ini..l.n].starts_with(b"[caja-amiga] ") {
                ini += 13;
            }
            let mut f = Linea::VACIA;
            let _ = f.write_str("  ");
            for &b in &l.b[ini..l.n] {
                if f.n < LARGO {
                    f.b[f.n] = b;
                    f.n += 1;
                }
            }
            f.color = color_de(&f.b[..f.n]);
            let huecos = LINEAS_PANEL - PRIMERA_FIJADA;
            let i = if self.fijadas < huecos {
                self.fijadas += 1;
                PRIMERA_FIJADA + self.fijadas - 1
            } else {
                // Lleno: suben todas una y la nueva va la última.
                for k in PRIMERA_FIJADA..LINEAS_PANEL - 1 {
                    self.panel[k] = self.panel[k + 1];
                    self.pintar_panel(k);
                }
                LINEAS_PANEL - 1
            };
            self.panel[i] = f;
            self.pintar_panel(i);
        }
    }
}

/// ¿Contiene `t` (en Latin-1) el texto `p`?
fn contiene(t: &[u8], p: &str) -> bool {
    let mut q = [0u8; 64];
    let mut n = 0;
    for c in p.chars().take(64) {
        q[n] = latin1(c);
        n += 1;
    }
    n > 0 && t.windows(n).any(|w| w == &q[..n])
}

/// El color de una línea por lo que dice: verde si acaba bien, magenta si algo fue mal.
fn color_de(t: &[u8]) -> u32 {
    if ["MAL", "NO ", "no se lanza", "no arrancan", "PÁNICO"].iter().any(|p| contiene(t, p)) {
        MAL
    } else if contiene(t, "bien") || contiene(t, "BIEN") {
        BIEN
    } else {
        TEXTO
    }
}

/// Busca la pantalla y la prepara: la del árbol (`simple-framebuffer`) o, si no, la que da el firmware de la Pi por el
/// buzón. Devuelve de dónde salió, para el registro. También deja apuntado el buzón para `propiedad`.
pub fn iniciar(d: &Dtb) -> Option<&'static str> {
    if let Some(n) = d.compatible("brcm,bcm2835-mbox")
        && let Some((fisica, _)) = d.reg_cpu(&n, 0)
    {
        hw::poner_buzon(fisica);
    }
    let origen = if del_arbol(d) {
        "simple-framebuffer del árbol"
    } else if del_buzon() {
        "buzón del firmware"
    } else {
        return None;
    };
    let (_, ancho, alto, _) = hw::medidas()?;
    let mut e = ESTADO.tomar();
    e.ancho = ancho;
    e.alto = alto;
    // Unas 104 columnas en el panel: letra de 18 x 36 en una pantalla de 1920 de ancho, de 24 x 48 en una de 2560, de
    // 12 x 24 en una de 1280 o menos. El registro, medio punto menos.
    e.esc_panel = (ancho * 2 / (AN * 104)).clamp(2, 8);
    e.esc_log = (e.esc_panel - 1).max(2);
    e.y_log = (LINEAS_PANEL as u32 + 1) * alto_letra(e.esc_panel);
    if e.y_log + 6 * alto_letra(e.esc_log) > alto {
        e.esc_panel = 2;
        e.esc_log = 2;
        e.y_log = (LINEAS_PANEL as u32 + 1) * AL;
    }
    hw::rellenar(0, 0, ancho, alto, FONDO);
    hw::rellenar(0, 0, ancho, e.y_log - alto_letra(e.esc_panel) / 2, FONDO_PANEL);
    hw::rellenar(0, e.y_log - 4, ancho, 2, TENUE);
    e.activa = true;
    drop(e);
    Some(origen)
}

fn del_arbol(d: &Dtb) -> bool {
    let Some(n) = d.compatible("simple-framebuffer") else { return false };
    if let Some((datos, largo)) = d.propiedad(&n, "status")
        && largo >= 2
        && d.u8(datos) != b'o'
    {
        return false;
    }
    let Some((fisica, tamano)) = d.reg_cpu(&n, 0) else { return false };
    let celda = |nombre: &str| d.propiedad(&n, nombre).map(|(datos, _)| d.celda(datos, 0));
    let (Some(ancho), Some(alto), Some(paso)) = (celda("width"), celda("height"), celda("stride")) else { return false };
    let bgr = matches!(d.propiedad(&n, "format"), Some((datos, _)) if d.u8(datos + 1) == b'8' && d.u8(datos + 2) == b'b');
    hw::poner(fisica, tamano, ancho, alto, paso, bgr)
}

/// El desplazamiento de las direcciones de bus de la VideoCore: la Pi 5 ve la RAM tal cual (`dma-ranges` vacío en el
/// nodo `firmware` de su árbol).
const BUS: u64 = 0;

/// Pide la pantalla al firmware de la Pi (etiquetas de la "Mailbox property interface"): el tamaño del monitor, y una
/// pantalla de ese tamaño a 32 bits por píxel. Sin monitor (0 x 0), 1280 x 720.
fn del_buzon() -> bool {
    let mut m = [0u32; 8];
    m[..8].copy_from_slice(&[32, 0, 0x0004_0003, 8, 0, 0, 0, 0]);
    let (mut ancho, mut alto) = if hw::propiedades(&mut m, BUS) { (m[5], m[6]) } else { (0, 0) };
    if ancho == 0 || alto == 0 || ancho > 7680 || alto > 4320 {
        (ancho, alto) = (1280, 720);
    }
    #[rustfmt::skip]
    let mut p = [
        35 * 4, 0,
        0x0004_8003, 8, 0, ancho, alto,       // tamaño físico
        0x0004_8004, 8, 0, ancho, alto,       // tamaño virtual
        0x0004_8005, 4, 0, 32,                // bits por píxel
        0x0004_8006, 4, 0, 0,                 // orden "BGR": el píxel 0x00RRGGBB en little-endian (como el compositor)
        0x0004_0001, 8, 0, 4096, 0,           // reservar la pantalla (alineada a 4 KB): base y tamaño
        0x0004_0008, 4, 0, 0,                 // bytes por fila
        0, 0, 0,
    ];
    if !hw::propiedades(&mut p, BUS) {
        return false;
    }
    let (an, al, orden, base, tam, paso) = (p[5], p[6], p[20], p[25] & 0x3fff_ffff, p[26], p[30]);
    PEDIDA.store(((an as u64) << 32) | al as u64, core::sync::atomic::Ordering::Relaxed);
    // El orden 0 deja el píxel como 0x00RRGGBB en little-endian (a8r8g8b8); el 1, con rojo y azul cambiados.
    hw::poner(base as u64, tam as u64, an, al, paso, orden == 1)
}

static PEDIDA: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Una etiqueta del firmware de la Pi con `n` palabras de respuesta (y `entrada` delante), o `None` si no hay buzón o
/// no contesta. Solo antes de que el kernel se traslade (C38).
pub fn propiedad<const N: usize>(etiqueta: u32, entrada: &[u32]) -> Option<[u32; N]> {
    let mut m = [0u32; 64];
    let palabras = N.max(entrada.len());
    let total = 5 + palabras + 1;
    if total > 64 {
        return None;
    }
    m[0] = (total * 4) as u32;
    m[2] = etiqueta;
    m[3] = (palabras * 4) as u32;
    m[5..5 + entrada.len()].copy_from_slice(entrada);
    if !hw::propiedades(&mut m[..total], BUS) {
        return None;
    }
    let mut r = [0u32; N];
    r.copy_from_slice(&m[5..5 + N]);
    Some(r)
}

/// Pone la línea `i` del panel.
pub fn panel(i: usize, color: u32, args: fmt::Arguments) {
    let mut l = Linea::VACIA;
    let _ = l.write_fmt(args);
    l.color = color;
    let mut e = ESTADO.tomar();
    if i < LINEAS_PANEL {
        e.panel[i] = l;
        if e.activa {
            e.pintar_panel(i);
        }
    }
}

/// Lo que se escribe en el registro, también en la pantalla. Lo llama `serie` con su cerrojo tomado.
pub fn escribir(s: &str) {
    let mut e = ESTADO.tomar();
    if !e.activa {
        return;
    }
    for c in s.chars() {
        match c {
            '\r' => {}
            '\n' => {
                e.fijar_si_importa();
                e.actual = Linea::VACIA;
                e.nueva_linea();
            }
            _ => {
                let b = latin1(c);
                let mut a = e.actual;
                let _ = a.write_char(c);
                e.actual = a;
                e.letra(b);
            }
        }
    }
}

/// El pánico, en la pantalla aunque alguien tenga el cerrojo (sin esperar: si lo tiene este mismo núcleo, no se pinta).
pub fn panico(info: &core::panic::PanicInfo) {
    let Some(mut e) = ESTADO.intentar() else { return };
    if !e.activa {
        return;
    }
    let mut l = Linea::VACIA;
    let _ = write!(l, "PÁNICO: {}", info.message());
    l.color = MAL;
    e.panel[LINEAS_PANEL - 1] = l;
    e.pintar_panel(LINEAS_PANEL - 1);
}

/// La pantalla, si la hay: (física, ancho, alto, bytes por fila) y el tamaño que se pidió al firmware (ancho, alto).
pub fn medidas() -> Option<(u64, u32, u32, u32)> {
    hw::medidas()
}

pub fn pedida() -> (u32, u32) {
    let p = PEDIDA.load(core::sync::atomic::Ordering::Relaxed);
    ((p >> 32) as u32, p as u32)
}
