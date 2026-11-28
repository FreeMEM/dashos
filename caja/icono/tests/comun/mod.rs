//! El banco de pruebas: una caja con exec (memoria de mentira), la dos.library de la caja sobre ficheros en memoria
//! (`MemFs`), icon.library y workbench.library, unas graphics, intuition y cybergraphics falsas que apuntan lo que se
//! les pide (y un lienzo ARGB para leer y escribir píxeles), y un generador de `.info` (clásico, NewIcons, de color
//! y PNG) hecho aquí: ningún icono de AmigaOS ni de programas comerciales.
#![allow(dead_code)]

use std::collections::BTreeMap;

use caja_dos::fs::Asa;
use caja_dos::{Dos, MemFs};
use caja_exec::estructuras::memf;
use caja_exec::{Accion, Config, Exec, Memoria, MemoriaVec, Registros};
use caja_icono::*;

pub const TAMANO: u32 = 8 << 20;
pub const PRIMERO: u16 = 0x900;
/// Los números nativos de las bibliotecas falsas: graphics, intuition y cybergraphics, de 0x100 en 0x100.
pub const N_GFX: u16 = 0xA00;
pub const N_INT: u16 = 0xB00;
pub const N_CGX: u16 = 0xC00;
const ENTRADAS_FALSAS: u16 = 120;

/// dos.library de la caja vista como `Caja` de dos sobre exec (como `CajaDos` del pegamento).
pub struct CajaEx<'a> {
    pub ex: &'a mut Exec<MemoriaVec>,
}

impl caja_dos::Memoria for CajaEx<'_> {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        self.ex.mem.trozo(dir, largo)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        self.ex.mem.trozo_mut(dir, largo)
    }
}

impl caja_dos::Caja for CajaEx<'_> {
    fn reservar(&mut self, largo: u32) -> u32 {
        self.ex.alloc_vec(largo, memf::PUBLIC | memf::CLEAR).unwrap_or(0)
    }
    fn liberar(&mut self, dir: u32, _largo: u32) {
        self.ex.free_vec(dir).unwrap();
    }
    fn tarea_actual(&self) -> u32 {
        self.ex.tarea_actual()
    }
}

/// Una llamada a una biblioteca falsa: (biblioteca, LVO, registros).
#[derive(Clone, Debug)]
pub struct Llamada {
    pub lib: &'static str,
    pub lvo: i32,
    pub r: Registros,
}

/// El `Entorno` de las pruebas: los ficheros por dos (como lo hará el pegamento), las bibliotecas falsas y lo que se
/// pide lanzar.
pub struct EntornoPrueba {
    pub dos: Dos<MemFs>,
    pub llamadas: Vec<Llamada>,
    /// El lienzo de cybergraphics: (x, y) → ARGB; lo que no está es `fondo`.
    pub lienzo: BTreeMap<(i32, i32), u32>,
    pub fondo: u32,
    pub sin_cgx: bool,
    pub lanzados: Vec<(Vec<(u32, Vec<u8>)>, u32)>,
    pub lanzar_falla: Option<u32>,
}

impl Entorno<MemoriaVec> for EntornoPrueba {
    fn leer(&mut self, ex: &mut Exec<MemoriaVec>, nombre: &[u8], tope: u32) -> Result<Vec<u8>, u32> {
        por_dos::leer(&mut self.dos, &mut CajaEx { ex }, nombre, tope)
    }
    fn escribir(&mut self, ex: &mut Exec<MemoriaVec>, nombre: &[u8], datos: &[u8]) -> Result<(), u32> {
        por_dos::escribir(&mut self.dos, &mut CajaEx { ex }, nombre, datos)
    }
    fn borrar(&mut self, ex: &mut Exec<MemoriaVec>, nombre: &[u8]) -> Result<(), u32> {
        por_dos::borrar(&mut self.dos, &mut CajaEx { ex }, nombre)
    }
    fn clase(&mut self, ex: &mut Exec<MemoriaVec>, nombre: &[u8]) -> Result<Clase, u32> {
        por_dos::clase(&mut self.dos, &mut CajaEx { ex }, nombre)
    }
    fn poner_io_err(&mut self, ex: &mut Exec<MemoriaVec>, codigo: u32) {
        por_dos::poner_io_err(&mut self.dos, &mut CajaEx { ex }, codigo)
    }
    fn nativa(&mut self, ex: &mut Exec<MemoriaVec>, n: u16, r: &mut Registros) -> bool {
        let (lib, base) = match n {
            _ if (N_GFX..N_GFX + ENTRADAS_FALSAS).contains(&n) => ("graphics", N_GFX),
            _ if (N_INT..N_INT + ENTRADAS_FALSAS).contains(&n) => ("intuition", N_INT),
            _ if (N_CGX..N_CGX + ENTRADAS_FALSAS).contains(&n) => ("cybergraphics", N_CGX),
            _ => return false,
        };
        if lib == "cybergraphics" && self.sin_cgx {
            return false;
        }
        let lvo = -(30 + 6 * (n - base) as i32);
        self.llamadas.push(Llamada { lib, lvo, r: *r });
        match (lib, lvo) {
            // TextLength: 8 puntos por letra.
            ("graphics", -54) => r.d[0] = r.d[0] * 8,
            // ReadPixelArray: el lienzo al tampón (ARGB).
            ("cybergraphics", -120) => {
                let (buf, modulo, x, y, w, h) = (r.a[0], r.d[2], r.d[3] as i32, r.d[4] as i32, r.d[5], r.d[6]);
                for j in 0..h {
                    for i in 0..w {
                        let v = *self.lienzo.get(&(x + i as i32, y + j as i32)).unwrap_or(&self.fondo);
                        ex.mem.escribir32(buf + j * modulo + i * 4, v).unwrap();
                    }
                }
            }
            // WritePixelArray: del tampón al lienzo.
            ("cybergraphics", -126) => {
                let (buf, modulo, x, y, w, h) = (r.a[0], r.d[2], r.d[3] as i32, r.d[4] as i32, r.d[5], r.d[6]);
                for j in 0..h {
                    for i in 0..w {
                        let v = ex.mem.leer32(buf + j * modulo + i * 4).unwrap();
                        self.lienzo.insert((x + i as i32, y + j as i32), v);
                    }
                }
            }
            _ => {}
        }
        true
    }
    fn lanzar(&mut self, _ex: &mut Exec<MemoriaVec>, args: &[(u32, Vec<u8>)], pila: u32) -> Result<(), u32> {
        if let Some(e) = self.lanzar_falla {
            return Err(e);
        }
        self.lanzados.push((args.to_vec(), pila));
        Ok(())
    }
}

pub struct Caja {
    pub e: Exec<MemoriaVec>,
    pub i: Iconos,
    pub en: EntornoPrueba,
    pub tarea: u32,
    /// Raíces de los volúmenes `Prueba:` y `ENV:`.
    pub prueba: Asa,
    pub env: Asa,
}

pub fn caja() -> Caja {
    let mut e = Exec::arrancar(MemoriaVec::nueva(TAMANO as usize), Config::para(TAMANO)).unwrap();
    let tarea = e.crear_tarea("principal", 0, 8192, 0x10_0000).unwrap();
    assert_eq!(e.elegir_siguiente(), Some(tarea));
    let i = Iconos::instalar(&mut e, PRIMERO).unwrap();
    for (nombre, n0) in [("graphics.library", N_GFX), ("intuition.library", N_INT), ("cybergraphics.library", N_CGX)] {
        let numeros: Vec<u16> = (0..ENTRADAS_FALSAS).map(|k| n0 + k).collect();
        e.crear_biblioteca_nativa(nombre, 40, 0, &numeros, 0).unwrap();
    }
    let mut fs = MemFs::nuevo();
    let prueba = fs.nuevo_volumen(false, 1 << 26);
    let env = fs.nuevo_volumen(false, 1 << 20);
    let mut dos = Dos::nuevo(fs);
    {
        let mut c = CajaEx { ex: &mut e };
        dos.montar(&mut c, "Prueba", prueba, false).unwrap();
        dos.montar(&mut c, "ENV", env, false).unwrap();
    }
    dos.fs.poner_cajon(env, "Sys");
    let en = EntornoPrueba {
        dos,
        llamadas: Vec::new(),
        lienzo: BTreeMap::new(),
        fondo: 0xFF00_0000,
        sin_cgx: false,
        lanzados: Vec::new(),
        lanzar_falla: None,
    };
    Caja { e, i, en, tarea, prueba, env }
}

impl Caja {
    /// Una función por su nombre (de icon.library o de workbench.library), con A6 su base.
    pub fn llamar(&mut self, f: &str, r: &mut Registros) -> u32 {
        let n = if let Some(k) = FUNCIONES_ICON.iter().position(|x| *x == f) {
            r.a[6] = self.i.icon_base();
            PRIMERO + k as u16
        } else {
            let k = FUNCIONES_WB.iter().position(|x| *x == f).unwrap();
            r.a[6] = self.i.wb_base();
            PRIMERO + (FUNCIONES_ICON.len() + k) as u16
        };
        assert_eq!(self.i.llamar(&mut self.e, n, r, &mut self.en), Accion::Seguir);
        r.d[0]
    }
    /// Una función con A0..A4 y D0..D2.
    pub fn f(&mut self, f: &str, a: &[u32], d: &[u32]) -> u32 {
        let mut r = Registros::default();
        r.a[..a.len()].copy_from_slice(a);
        r.d[..d.len()].copy_from_slice(d);
        self.llamar(f, &mut r)
    }
    pub fn cadena(&mut self, s: &[u8]) -> u32 {
        let d = self.e.alloc_mem(s.len() as u32 + 1, memf::CLEAR).unwrap();
        self.e.mem.leer_mut(d, s.len() as u32).unwrap().copy_from_slice(s);
        d
    }
    pub fn memoria(&mut self, largo: u32) -> u32 {
        self.e.alloc_mem(largo, memf::CLEAR).unwrap()
    }
    /// Una lista de etiquetas (con su TAG_DONE).
    pub fn etiquetas(&mut self, t: &[(u32, u32)]) -> u32 {
        let d = self.memoria(8 * t.len() as u32 + 8);
        for (k, (tag, v)) in t.iter().enumerate() {
            self.e.mem.escribir32(d + 8 * k as u32, *tag).unwrap();
            self.e.mem.escribir32(d + 8 * k as u32 + 4, *v).unwrap();
        }
        d
    }
    pub fn l8(&self, d: u32) -> u8 {
        self.e.mem.leer8(d).unwrap()
    }
    pub fn l16(&self, d: u32) -> u16 {
        self.e.mem.leer16(d).unwrap()
    }
    pub fn l32(&self, d: u32) -> u32 {
        self.e.mem.leer32(d).unwrap()
    }
    pub fn texto(&self, d: u32) -> Vec<u8> {
        self.e.leer_cadena(d).unwrap().chars().map(|c| c as u8).collect()
    }
    pub fn poner(&mut self, camino: &str, datos: &[u8]) {
        self.en.dos.fs.poner(self.prueba, camino, datos);
    }
    pub fn contenido(&self, camino: &str) -> Option<Vec<u8>> {
        self.en.dos.fs.contenido(self.prueba, camino)
    }
    /// `IoErr()` de la tarea, como lo vería el programa.
    pub fn ioerr(&mut self) -> u32 {
        let mut r = caja_dos::Registros::default();
        let mut c = CajaEx { ex: &mut self.e };
        self.en.dos.llamar(&mut c, caja_dos::lvo::IO_ERR, &mut r);
        r.d[0]
    }
    /// Los tooltypes del vector de un DiskObject.
    pub fn tooltypes(&self, dobj: u32) -> Vec<Vec<u8>> {
        let v = self.l32(dobj + 54);
        let mut t = Vec::new();
        if v == 0 {
            return t;
        }
        for k in 0.. {
            let p = self.l32(v + 4 * k);
            if p == 0 {
                break;
            }
            t.push(self.texto(p));
        }
        t
    }
    pub fn avisos(&mut self) -> Vec<caja_exec::Aviso> {
        self.i.tomar_avisos()
    }
}

/// Que la tarea de las pruebas no es un proceso (dos guarda su IoErr aparte).
pub fn no_es_proceso(c: &Caja) -> bool {
    c.e.mem.leer8(c.tarea + 8).unwrap() != caja_dos::estructuras::process::NT_PROCESS
}

// --- El generador de .info ---

/// Una imagen planar de `ancho` × `alto` y `prof` planos, con el color de cada punto de `color(x, y)`.
pub fn planar(ancho: u16, alto: u16, prof: u16, color: impl Fn(usize, usize) -> u8) -> info::Planar {
    let fila = (ancho as usize).div_ceil(16) * 2;
    let plano = fila * alto as usize;
    let mut datos = vec![0u8; plano * prof as usize];
    for y in 0..alto as usize {
        for x in 0..ancho as usize {
            let c = color(x, y);
            for p in 0..prof as usize {
                if (c >> p) & 1 != 0 {
                    datos[p * plano + y * fila + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
    }
    info::Planar {
        izq: 0,
        arriba: 0,
        ancho,
        alto,
        prof,
        pick: ((1u16 << prof) - 1) as u8,
        onoff: 0,
        datos: Some(datos),
    }
}

/// Un icono clásico de herramienta, 20 × 10 de 2 planos, con estos tooltypes.
pub fn clasico(tooltypes: &[&str]) -> info::Info {
    let mut i = info::Info::vacio(info::WBTOOL);
    i.ancho = 20;
    i.alto = 11;
    i.imagen1 = Some(planar(20, 10, 2, |x, y| ((x + y) % 4) as u8));
    i.imagen2 = Some(planar(20, 10, 2, |x, _| (x % 4) as u8));
    i.flags = info::GFLG_GADGIMAGE | info::GFLG_GADGHIMAGE;
    i.herramienta = Some(b"SYS:C/Herramienta".to_vec());
    i.tooltypes = Some(tooltypes.iter().map(|t| t.as_bytes().to_vec()).collect());
    i.x = 10;
    i.y = 20;
    i.pila = 8192;
    i
}

/// Bytes de un clásico.
pub fn bytes_clasico(tooltypes: &[&str]) -> Vec<u8> {
    info::escribir(&clasico(tooltypes), info::Opciones::default()).unwrap()
}

/// Una paleta de prueba de `n` colores.
pub fn paleta(n: usize) -> Vec<[u8; 3]> {
    (0..n).map(|k| [(k * 37) as u8, (k * 91) as u8, (255usize.wrapping_sub(k * 13)) as u8]).collect()
}

/// Las líneas de NewIcons de una imagen (`IM1=` o `IM2=`): cabecera, paleta y píxeles, 7 bits por letra.
pub fn lineas_newicons(
    prefijo: &str,
    ancho: usize,
    alto: usize,
    pal: &[[u8; 3]],
    px: &[u8],
    transparente: bool,
) -> Vec<String> {
    let colores = pal.len();
    let mut bits = 1;
    while (1usize << bits) < colores {
        bits += 1;
    }
    // Una tira de valores (de `n` bits) a letras de 7 bits.
    fn letras(valores: &[(u32, u32)]) -> String {
        let mut b: Vec<u8> = Vec::new();
        for &(v, n) in valores {
            for k in (0..n).rev() {
                b.push(((v >> k) & 1) as u8);
            }
        }
        while b.len() % 7 != 0 {
            b.push(0);
        }
        b.chunks(7)
            .map(|g| {
                let v = g.iter().fold(0u8, |a, &x| a << 1 | x);
                if v < 0x50 { (0x20 + v) as char } else { char::from_u32(0xA1 + (v - 0x50) as u32).unwrap() }
            })
            .collect()
    }
    let mut cab = String::new();
    cab.push(if transparente { 'B' } else { 'C' });
    cab.push((0x21 + ancho as u8) as char);
    cab.push((0x21 + alto as u8) as char);
    cab.push((0x21 + (colores >> 6) as u8) as char);
    cab.push((0x21 + (colores & 63) as u8) as char);
    let rgb: Vec<(u32, u32)> = pal.iter().flat_map(|c| c.iter().map(|&v| (v as u32, 8))).collect();
    let mut lineas = vec![format!("{prefijo}{cab}{}", letras(&rgb))];
    // Los píxeles, en líneas de hasta 100 valores.
    for trozo in px.chunks(100) {
        let v: Vec<(u32, u32)> = trozo.iter().map(|&p| (p as u32, bits)).collect();
        lineas.push(format!("{prefijo}{}", letras(&v)));
    }
    lineas
}

/// Un texto de NewIcons pasado a los bytes Latin-1 de un tooltype.
pub fn latin1(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

/// Un NewIcon: un clásico con las líneas de la imagen (16 × 12, 5 colores) en sus tooltypes.
pub fn newicon() -> (Vec<u8>, Vec<u8>, Vec<[u8; 3]>) {
    let mut i = clasico(&["PUBSCREEN=Workbench"]);
    let pal = paleta(5);
    let px: Vec<u8> = (0..16 * 12).map(|k| (k % 5) as u8).collect();
    let mut tt: Vec<Vec<u8>> = i.tooltypes.take().unwrap();
    tt.push(b" ".to_vec());
    tt.push(b"*** DON'T EDIT THE FOLLOWING LINES!! ***".to_vec());
    for l in lineas_newicons("IM1=", 16, 12, &pal, &px, true) {
        tt.push(latin1(&l));
    }
    i.tooltypes = Some(tt);
    let op = info::Opciones { sin_color: true, ..info::Opciones::default() };
    (info::escribir(&i, op).unwrap(), px, pal)
}

/// Un icono de color de OS 3.5 (`FORM ICON` tras lo clásico): 24 × 18, 7 colores, con imagen de seleccionado.
pub fn de_color() -> (Vec<u8>, color::Color) {
    let mut i = clasico(&["DONOTWAIT"]);
    let pal = paleta(7);
    let n = 24 * 18;
    let normal: Vec<u8> = (0..n).map(|k| if k % 24 < 3 { 0 } else { (k / 24 % 7) as u8 }).collect();
    let sel: Vec<u8> = normal.iter().map(|&v| if v == 0 { 0 } else { 6 - v % 6 }).collect();
    let c = color::Color {
        ancho: 24,
        alto: 18,
        sin_marco: true,
        aspecto: 0x11,
        img: [
            Some(color::ImagenColor { indices: normal, paleta: pal.clone(), transparente: Some(0) }),
            Some(color::ImagenColor { indices: sel, paleta: pal, transparente: Some(0) }),
        ],
        ..color::Color::default()
    };
    i.color = Some(c.clone());
    (info::escribir(&i, info::Opciones::default()).unwrap(), c)
}

fn crc_adler(datos: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in datos {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}

/// Un PNG RGBA de `ancho` × `alto` sin comprimir (bloques "stored" de deflate), con un trozo `icOn` cualquiera.
pub fn png(ancho: u32, alto: u32, px: impl Fn(u32, u32) -> u32) -> Vec<u8> {
    let mut crudo = Vec::new();
    for y in 0..alto {
        crudo.push(0);
        for x in 0..ancho {
            let p = px(x, y);
            crudo.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let trozos: Vec<&[u8]> = crudo.chunks(65535).collect();
    for (k, t) in trozos.iter().enumerate() {
        z.push((k + 1 == trozos.len()) as u8);
        z.extend_from_slice(&(t.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(t.len() as u16)).to_le_bytes());
        z.extend_from_slice(t);
    }
    z.extend_from_slice(&crc_adler(&crudo).to_be_bytes());
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut trozo = |id: &[u8; 4], d: &[u8]| {
        v.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut c = id.to_vec();
        c.extend_from_slice(d);
        v.extend_from_slice(&c);
        v.extend_from_slice(&kit::png::crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&ancho.to_be_bytes());
    ihdr.extend_from_slice(&alto.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    trozo(b"IHDR", &ihdr);
    trozo(b"icOn", &[0x80, 0, 0x10, 0x01, 0, 0, 0, 1]);
    trozo(b"IDAT", &z);
    trozo(b"IEND", &[]);
    v
}

/// Un icono PNG de 32 × 32 con su imagen de seleccionado detrás.
pub fn icono_png() -> Vec<u8> {
    let mut v = png(32, 32, |x, y| if x < 4 { 0 } else { 0xFF00_0000 | (x * 8) << 16 | (y * 8) << 8 | 0x40 });
    v.extend_from_slice(&png(32, 32, |x, _| if x < 4 { 0x80FF_0000 } else { 0xFFFF_0000 }));
    v
}
