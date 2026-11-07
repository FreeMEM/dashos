//! El compositor por CPU como proceso (C50). Antes iba dentro del kernel (C16, fase A de S9).
//!
//! Llaves, en la Pi: la 1, el canal del buzón (C50); la 2, el canal de entrada, por el que llegan los eventos del USB
//! (C49). La pantalla le llega del buzón, como una llave de la memoria de la VideoCore recortada a ella (auditoría de
//! la fase 4, M4): no ve el resto de esa memoria.
//!
//! Por UEFI (U4): la 1, la pantalla que dejó el firmware (memoria del firmware); la 2, sus medidas (un objeto de
//! memoria: ancho, alto, bytes por línea, formato y desplazamiento dentro de la página); la 3, el canal de entrada. Esa
//! pantalla no tiene desplazamiento virtual: un solo búfer, y se repintan los daños en él.
//!
//! Dos búferes en la pantalla virtual del firmware: se pinta en el que no se ve y se cambia con el desplazamiento
//! virtual. Solo se repinta lo que cambia (los daños). Como cada búfer se vio hace dos cuadros, en el que se va a pintar
//! hay que rehacer los daños de este cuadro y los del anterior. El ratón mueve la ventana.
//!
//! El compositor de verdad hablará Wayland (S9); esto es para medir y para tener algo en pantalla.

#![no_std]
#![no_main]

use dashos::{ahora_us, println};

dashos::programa!(principal);

const VA_VC: usize = 0x60_0000_0000;
const ANCHO: u32 = 640;
const ALTO: u32 = 480;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        if self.w == 0 || self.h == 0 {
            return o;
        }
        if o.w == 0 || o.h == 0 {
            return self;
        }
        let (x0, y0) = (self.x.min(o.x), self.y.min(o.y));
        let (x1, y1) = ((self.x + self.w).max(o.x + o.w), (self.y + self.h).max(o.y + o.h));
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    fn contiene(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn recortar(self, w: u32, h: u32) -> Rect {
        let x1 = (self.x + self.w).min(w);
        let y1 = (self.y + self.h).min(h);
        Rect { x: self.x.min(w), y: self.y.min(h), w: x1.saturating_sub(self.x), h: y1.saturating_sub(self.y) }
    }
}

/// Un mensaje al buzón (llave 1): palabras en el orden de la CPU; vuelve con la respuesta. `None` si no la aceptó; si
/// no, la llave que trajera la respuesta (0 si ninguna).
fn buzon(m: &mut [u32]) -> Option<u32> {
    let mut b = [0u8; 256];
    let n = m.len() * 4;
    for (i, w) in m.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    // La respuesta vuelve por un canal propio de esta petición: mandamos uno de sus extremos con ella.
    let (mio, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(1, &b[..n], &[suyo]) != 0 {
        dashos::cerrar(mio);
        dashos::cerrar(suyo);
        return None;
    }
    let mut ll = [0u32; 1];
    let (leido, k) = dashos::canal_leer(mio, &mut b, &mut ll);
    dashos::cerrar(mio);
    let llave = if k > 0 { ll[0] } else { 0 };
    if leido != n as i64 {
        return None;
    }
    for (i, w) in m.iter_mut().enumerate() {
        *w = u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    }
    (m[1] == 0x8000_0000).then_some(llave)
}

struct Pantalla {
    ancho: u32,
    alto: u32,
    paso: u32,
    /// Dónde se ve en este proceso.
    base: usize,
    /// Dos búferes con desplazamiento virtual (la del buzón) o uno solo (la de UEFI).
    doble: bool,
    /// Los colores van R, G, B en memoria (a8b8g8r8) y no B, G, R.
    rb: bool,
}

const VA_INFO: usize = 0x61_0000_0000;

/// La pantalla que dejó el firmware de UEFI (U4): llave 1, la memoria; llave 2, sus medidas.
fn pantalla_uefi() -> Option<Pantalla> {
    if dashos::registros_proyectar(1, VA_VC) != 0 || dashos::memoria_proyectar(2, VA_INFO, false) != 0 {
        return None;
    }
    let leer = |i: usize| unsafe { core::ptr::read_volatile((VA_INFO + 4 * i) as *const u32) };
    let (ancho, alto, paso, formato, desp) = (leer(0), leer(1), leer(2), leer(3), leer(4));
    Some(Pantalla { ancho, alto, paso, base: VA_VC + desp as usize, doble: false, rb: formato == 1 })
}

/// Pide una pantalla de `ancho` × `alto` a 32 bits con `buferes` pantallas virtuales una debajo de otra. La memoria
/// llega con la respuesta, como una llave que solo cubre la pantalla; se proyecta en `VA_VC`.
fn pantalla(ancho: u32, alto: u32, buferes: u32) -> Option<Pantalla> {
    #[rustfmt::skip]
    let mut m = [
        0, 0,
        0x0004_8003, 8, 0, ancho, alto,
        0x0004_8004, 8, 0, ancho, alto * buferes,
        0x0004_8005, 4, 0, 32,
        0x0004_8006, 4, 0, 0,               // B, G, R: el píxel 0x00RRGGBB en little-endian
        0x0004_0001, 8, 0, 4096, 0,
        0x0004_0008, 4, 0, 0,
        0,
    ];
    m[0] = m.len() as u32 * 4;
    let llave = buzon(&mut m)?;
    if m[23] == 0 || llave == 0 {
        return None;
    }
    if dashos::registros_proyectar(llave, VA_VC) != 0 {
        println!("  [compositor] no se puede proyectar la pantalla");
        return None;
    }
    // La llave empieza en la página de la pantalla: dentro, su desplazamiento.
    let fisica = m[23] & 0x3fff_ffff;
    Some(Pantalla { ancho: m[5], alto: m[6], paso: m[28], base: VA_VC + (fisica & 0xfff) as usize, doble: true, rb: false })
}

fn desplazar(y: u32) -> bool {
    let mut m = [0, 0, 0x0004_8009, 8, 0, 0, y, 0];
    m[0] = m.len() as u32 * 4;
    buzon(&mut m).is_some()
}

/// El fondo: degradado del azul al violeta con un tablero rojo y blanco en el centro.
fn fondo(p: &Pantalla, x: u32, y: u32) -> u32 {
    let (cx, cy) = (p.ancho / 2, p.alto / 2);
    if x + 120 >= cx && x < cx + 120 && y + 120 >= cy && y < cy + 120 {
        let (i, j) = ((x + 120 - cx) / 30, (y + 120 - cy) / 30);
        if (i + j) % 2 == 0 { 0x00e0_2020 } else { 0x00f0_f0f0 }
    } else {
        let r = 40 + y * 100 / p.alto;
        let b = 120 + y * 100 / p.alto;
        (r << 16) | (40 << 8) | b
    }
}

/// Una ventana sencilla: borde, barra de título oscura y cuerpo claro.
fn ventana(v: &Rect, x: u32, y: u32) -> u32 {
    let (dx, dy) = (x - v.x, y - v.y);
    if dx < 2 || dy < 2 || dx >= v.w - 2 || dy >= v.h - 2 {
        0x0020_2028
    } else if dy < 24 {
        if dx >= v.w - 22 && dx < v.w - 8 && dy >= 6 && dy < 18 { 0x00f0_f0f0 } else { 0x0030_3a5a }
    } else {
        0x00ec_eef4
    }
}

struct Compositor {
    p: Pantalla,
    visible: u32,
    ventana: Rect,
    danos_previos: Rect,
    pixeles_pintados: u64,
}

impl Compositor {
    fn esperado(&self, x: u32, y: u32) -> u32 {
        if self.ventana.contiene(x, y) { ventana(&self.ventana, x, y) } else { fondo(&self.p, x, y) }
    }

    /// La memoria de la pantalla se proyecta sin caché: se escribe píxel a píxel y no hay que limpiar nada.
    fn pintar(&mut self, bufer: u32, r: Rect) {
        let r = r.recortar(self.p.ancho, self.p.alto);
        let fila0 = bufer * self.p.alto;
        for y in r.y..r.y + r.h {
            let linea = self.p.base + ((fila0 + y) * self.p.paso) as usize;
            for x in r.x..r.x + r.w {
                let c = self.esperado(x, y);
                let c = if self.p.rb { (c & 0xff00) | (c >> 16 & 0xff) | (c & 0xff) << 16 } else { c };
                unsafe { ((linea + x as usize * 4) as *mut u32).write_volatile(c.to_le()) };
            }
        }
        self.pixeles_pintados += (r.w * r.h) as u64;
    }

    /// Mueve la ventana y presenta un cuadro nuevo. Con `todo`, repinta la pantalla entera (para comparar).
    fn mover(&mut self, x: u32, y: u32, todo: bool) {
        let antes = self.ventana;
        self.ventana.x = x.min(self.p.ancho - self.ventana.w);
        self.ventana.y = y.min(self.p.alto - self.ventana.h);
        let danos = antes.union(self.ventana);
        if !self.p.doble {
            // Un solo búfer (U4): los daños de este cuadro, directamente en lo que se ve.
            let zona = if todo { Rect { x: 0, y: 0, w: self.p.ancho, h: self.p.alto } } else { danos };
            self.pintar(0, zona);
            return;
        }
        let oculto = 1 - self.visible;
        let zona = if todo { Rect { x: 0, y: 0, w: self.p.ancho, h: self.p.alto } } else { danos.union(self.danos_previos) };
        self.pintar(oculto, zona);
        self.danos_previos = danos;
        desplazar(oculto * self.p.alto);
        self.visible = oculto;
    }

    fn leer(&self, x: u32, y: u32) -> u32 {
        let fila = self.visible * self.p.alto + y;
        let c = u32::from_le(unsafe { ((self.p.base + (fila * self.p.paso + x * 4) as usize) as *const u32).read_volatile() });
        if self.p.rb { (c & 0xff00) | (c >> 16 & 0xff) | (c & 0xff) << 16 } else { c }
    }

    /// El búfer visible tiene la ventana en su sitio y nada de ella fuera.
    fn comprobar(&self) -> bool {
        let v = self.ventana;
        let (w, h) = (self.p.ancho, self.p.alto);
        let puntos = [(v.x + 1, v.y + 1), (v.x + 50, v.y + 10), (v.x + 100, v.y + 100), (5, 5), (w / 2, 10), (w - 1, h - 1), (v.x + v.w, v.y), (v.x, v.y + v.h)];
        puntos.iter().filter(|&&(x, y)| x < w && y < h).all(|&(x, y)| self.leer(x, y) == self.esperado(x, y))
    }
}

fn principal() -> u64 {
    // Por UEFI, la pantalla llega como llave; si no, se pide al buzón (la Pi). La entrada va detrás.
    let (p, entrada) = match pantalla_uefi() {
        Some(p) => (p, 3),
        None => match pantalla(ANCHO, ALTO, 2) {
            Some(p) => (p, 2),
            None => {
                println!("  [compositor] el firmware no da pantalla");
                return 3;
            }
        },
    };
    if p.doble {
        println!("  [compositor] pantalla {}x{} a 32 bits, doble búfer, solo con su memoria", p.ancho, p.alto);
    } else {
        println!("  [compositor] pantalla de UEFI de {}x{} a 32 bits, un búfer", p.ancho, p.alto);
    }
    let mut c = Compositor { p, visible: 0, ventana: Rect { x: 40, y: 40, w: 200, h: 140 }, danos_previos: Rect::default(), pixeles_pintados: 0 };
    let todo = Rect { x: 0, y: 0, w: c.p.ancho, h: c.p.alto };
    c.pintar(0, todo);
    if c.p.doble {
        c.pintar(1, todo);
    }
    if c.p.doble {
        desplazar(0);
    }

    // Lo que costaba en el kernel (C16), ahora desde un proceso: 60 cuadros repintando todo y 60 solo con los daños.
    let medir = |c: &mut Compositor, todo: bool| {
        let pintados = c.pixeles_pintados;
        let t = ahora_us();
        for i in 0..60u32 {
            c.mover(40 + i * 5, 40 + i * 3, todo);
        }
        ((ahora_us() - t) / 60, (c.pixeles_pintados - pintados) / 60)
    };
    let (us_todo, px_todo) = medir(&mut c, true);
    c.mover(40, 40, true);
    c.mover(40, 40, true);
    let (us_danos, px_danos) = medir(&mut c, false);
    println!("  [compositor] por cuadro: repintando todo {} µs ({} píxeles); solo daños {} µs ({} píxeles)", us_todo, px_todo, us_danos, px_danos);
    let bien = c.comprobar() && px_danos * 4 < px_todo;
    println!("  [compositor] doble búfer y daños: {}", if bien { "bien" } else { "MAL" });

    // El ratón mueve la ventana. La primera vez se comprueba el búfer visible.
    let (mut x, mut y) = (c.ventana.x as i32, c.ventana.y as i32);
    let mut primera = true;
    let mut ev = [0u8; 16];
    let mut ll = [0u32; 8];
    loop {
        let (n, k) = dashos::canal_leer(entrada, &mut ev, &mut ll);
        if n == -4 {
            return 0; // la entrada se cerró
        }
        // Un mensaje que no vale no para el compositor; las llaves que lleguen, fuera (M6).
        for &l in ll.iter().take(k) {
            dashos::cerrar(l);
        }
        if n < 0 {
            continue;
        }
        if n != 8 || ev[0] != 2 {
            continue;
        }
        x = (x + ev[2] as i8 as i32).clamp(0, (c.p.ancho - c.ventana.w) as i32);
        y = (y + ev[3] as i8 as i32).clamp(0, (c.p.alto - c.ventana.h) as i32);
        c.mover(x as u32, y as u32, false);
        if primera {
            primera = false;
            println!("  [compositor] ventana movida con el ratón a ({}, {}): {}", x, y, if c.comprobar() { "bien" } else { "MAL" });
        }
    }
}
