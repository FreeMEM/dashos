//! virtio-gpu 2D (S25) con un invitado falso: el camino normal (pantallas, recursos, respaldo, transferencias,
//! pantalla y vaciado) y lo hostil (órdenes rotas, recursos enormes, ids repetidos o que no existen, respaldos y
//! transferencias fuera de la memoria del invitado). Después de cada cosa mala, el aparato sigue funcionando.

use std::cell::RefCell;
use std::rc::Rc;

use caja_vmm::gic::{self, Distribuidor};
use caja_vmm::maquina::{Maquina, Operacion, SalidaMmio};
use caja_vmm::memoria::{MemoriaInvitado, Ram};
use caja_vmm::virtio::cola::{DESC_F_NEXT, DESC_F_WRITE};
use caja_vmm::virtio::gpu::{self, ExtremoGpu, Gpu, Rect, Vista};
use caja_vmm::virtio::mmio::*;

const RAM: u64 = 0x4000_0000;
const TAM_RAM: usize = 8 << 20;
const GICD: u64 = 0x0800_0000;
const GPU: u64 = 0x0a00_0000;
const SPI_GPU: u32 = 16;
const TOPE: u64 = 4 << 20;

/// Lo que ve DashOS: cada `mostrar` (pantalla, daño, las filas dañadas ya en XRGB) y cada `apagar`.
#[derive(Default)]
struct Pantallas {
    mostrado: Vec<(u32, Rect, Vec<Vec<u32>>)>,
    apagadas: Vec<u32>,
}

#[derive(Clone, Default)]
struct Extremo(Rc<RefCell<Pantallas>>);

impl ExtremoGpu for Extremo {
    fn mostrar(&mut self, pantalla: u32, vista: &Vista<'_>, d: Rect) {
        let filas = (0..d.alto)
            .map(|y| {
                let mut f = vec![0u32; d.ancho as usize];
                gpu::a_xrgb(vista.formato, vista.fila(d.x, d.y + y, d.ancho), &mut f);
                f
            })
            .collect();
        self.0.borrow_mut().mostrado.push((pantalla, d, filas));
    }
    fn apagar(&mut self, pantalla: u32) {
        self.0.borrow_mut().apagadas.push(pantalla);
    }
}

#[derive(Clone, Copy)]
struct Cola {
    tam: u16,
    desc: u64,
    disp: u64,
    usado: u64,
    libre: u16,
    idx: u16,
    vistos: u16,
}

impl Cola {
    fn en(base: u64, tam: u16) -> Self {
        let disp = base + 16 * tam as u64;
        let usado = (disp + 6 + 2 * tam as u64).next_multiple_of(4);
        Cola { tam, desc: base, disp, usado, libre: 0, idx: 0, vistos: 0 }
    }
}

struct Banco {
    m: Maquina<Ram<Vec<u8>>>,
    ext: Extremo,
    control: Cola,
    cursor: Cola,
    /// Dónde se escriben las órdenes y las respuestas.
    orden: u64,
    resp: u64,
}

/// Zonas de la RAM del invitado.
const COLAS: u64 = RAM + 0x1000;
const ORDENES: u64 = RAM + 0x8000;
const RESPUESTAS: u64 = RAM + 0x9000;
const PIXELES: u64 = RAM + 0x10_0000;

fn banco_con(pantallas: &[(u32, u32)]) -> Banco {
    let ext = Extremo::default();
    let mut m = Maquina::nueva(Ram::nueva(RAM, TAM_RAM), Distribuidor::nuevo(1, 64), GICD);
    m.añadir(GPU, SPI_GPU, Box::new(TransporteMmio::nuevo(Gpu::nueva(Box::new(ext.clone()), pantallas, TOPE)))).unwrap();
    m.mmio(0, GICD + gic::CTLR, 4, Operacion::Escribir(1));
    m.mmio(0, GICD + gic::ISENABLER + 4, 4, Operacion::Escribir(u32::MAX as u64));
    let mut b = Banco { m, ext, control: Cola::en(COLAS, 16), cursor: Cola::en(COLAS + 0x1000, 8), orden: ORDENES, resp: RESPUESTAS };
    let (c, k) = (b.control, b.cursor);
    b.arrancar(&[c, k]);
    b
}

fn banco() -> Banco {
    banco_con(&[(640, 480)])
}

impl Banco {
    fn w(&mut self, dir: u64, v: u32) {
        assert_eq!(self.m.mmio(0, dir, 4, Operacion::Escribir(v as u64)), SalidaMmio::Escrito);
    }
    fn r(&mut self, dir: u64) -> u32 {
        match self.m.mmio(0, dir, 4, Operacion::Leer) {
            SalidaMmio::Leido(v) => v as u32,
            otro => panic!("{otro:?}"),
        }
    }
    fn mem(&mut self) -> &mut Ram<Vec<u8>> {
        &mut self.m.mem
    }

    fn arrancar(&mut self, colas: &[Cola]) {
        self.w(GPU + R_STATUS, 0);
        self.w(GPU + R_STATUS, S_ACKNOWLEDGE | S_DRIVER);
        self.w(GPU + R_DEVICE_FEATURES_SEL, 1);
        let alto = self.r(GPU + R_DEVICE_FEATURES);
        self.w(GPU + R_DRIVER_FEATURES_SEL, 1);
        self.w(GPU + R_DRIVER_FEATURES, alto);
        self.w(GPU + R_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_FEATURES_OK);
        for (i, c) in colas.iter().enumerate() {
            self.w(GPU + R_QUEUE_SEL, i as u32);
            self.w(GPU + R_QUEUE_NUM, c.tam as u32);
            self.w(GPU + R_QUEUE_DESC_LOW, c.desc as u32);
            self.w(GPU + R_QUEUE_DRIVER_LOW, c.disp as u32);
            self.w(GPU + R_QUEUE_DEVICE_LOW, c.usado as u32);
            self.w(GPU + R_QUEUE_READY, 1);
        }
        self.w(GPU + R_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_FEATURES_OK | S_DRIVER_OK);
    }

    /// Pone una cadena en la cola y avisa. Devuelve los bytes escritos que dice el aparato.
    fn cadena(&mut self, cursor: bool, segs: &[(u64, u32, bool)]) -> u32 {
        let mut c = if cursor { self.cursor } else { self.control };
        let cabeza = c.libre;
        for (k, &(dir, largo, esc)) in segs.iter().enumerate() {
            let i = (c.libre + k as u16) % c.tam;
            let mut flags = if esc { DESC_F_WRITE } else { 0 };
            if k + 1 < segs.len() {
                flags |= DESC_F_NEXT;
            }
            let d = c.desc + 16 * i as u64;
            let m = self.mem();
            m.escribir_u64(d, dir).unwrap();
            m.escribir_u32(d + 8, largo).unwrap();
            m.escribir_u16(d + 12, flags).unwrap();
            m.escribir_u16(d + 14, (i + 1) % c.tam).unwrap();
        }
        c.libre = (c.libre + segs.len() as u16) % c.tam;
        let ranura = (c.idx % c.tam) as u64;
        self.mem().escribir_u16(c.disp + 4 + 2 * ranura, cabeza).unwrap();
        c.idx = c.idx.wrapping_add(1);
        let idx = c.idx;
        self.mem().escribir_u16(c.disp + 2, idx).unwrap();
        self.w(GPU + R_QUEUE_NOTIFY, cursor as u32);
        let usado = self.mem().leer_u16(c.usado + 2).unwrap();
        let mut escritos = u32::MAX;
        while c.vistos != usado {
            let e = c.usado + 4 + 8 * (c.vistos % c.tam) as u64;
            escritos = self.mem().leer_u32(e + 4).unwrap();
            c.vistos = c.vistos.wrapping_add(1);
        }
        if cursor {
            self.cursor = c;
        } else {
            self.control = c;
        }
        escritos
    }

    /// Manda una orden (cabecera de `tipo` con `flags` y `valla`, y el cuerpo) y devuelve el tipo de la respuesta y sus
    /// datos.
    fn mandar_con(&mut self, tipo: u32, flags: u32, valla: u64, cuerpo: &[u8], sitio: u32) -> (u32, Vec<u8>) {
        let mut o = Vec::new();
        o.extend_from_slice(&tipo.to_le_bytes());
        o.extend_from_slice(&flags.to_le_bytes());
        o.extend_from_slice(&valla.to_le_bytes());
        o.extend_from_slice(&[0; 8]);
        o.extend_from_slice(cuerpo);
        let (dir, resp) = (self.orden, self.resp);
        self.mem().escribir(dir, &o).unwrap();
        self.mem().escribir(resp, &vec![0xee; sitio as usize]).unwrap();
        let n = self.cadena(false, &[(dir, o.len() as u32, false), (resp, sitio, true)]);
        let mut r = vec![0u8; n.min(sitio) as usize];
        self.mem().leer(resp, &mut r).unwrap();
        let t = if r.len() >= 4 { u32::from_le_bytes(r[0..4].try_into().unwrap()) } else { 0 };
        (t, r)
    }

    fn mandar(&mut self, tipo: u32, cuerpo: &[u8]) -> u32 {
        self.mandar_con(tipo, 0, 0, cuerpo, 512).0
    }

    fn crear(&mut self, id: u32, formato: u32, ancho: u32, alto: u32) -> u32 {
        self.mandar(gpu::CMD_RESOURCE_CREATE_2D, &palabras(&[id, formato, ancho, alto]))
    }

    fn respaldar(&mut self, id: u32, entradas: &[(u64, u32)]) -> u32 {
        let mut c = palabras(&[id, entradas.len() as u32]);
        for &(dir, largo) in entradas {
            c.extend_from_slice(&dir.to_le_bytes());
            c.extend_from_slice(&largo.to_le_bytes());
            c.extend_from_slice(&[0; 4]);
        }
        self.mandar(gpu::CMD_RESOURCE_ATTACH_BACKING, &c)
    }

    fn transferir(&mut self, id: u32, r: [u32; 4], desp: u64) -> u32 {
        let mut c = palabras(&r);
        c.extend_from_slice(&desp.to_le_bytes());
        c.extend_from_slice(&palabras(&[id, 0]));
        self.mandar(gpu::CMD_TRANSFER_TO_HOST_2D, &c)
    }

    fn pantalla(&mut self, n: u32, id: u32, r: [u32; 4]) -> u32 {
        let mut c = palabras(&r);
        c.extend_from_slice(&palabras(&[n, id]));
        self.mandar(gpu::CMD_SET_SCANOUT, &c)
    }

    fn vaciar(&mut self, id: u32, r: [u32; 4]) -> u32 {
        let mut c = palabras(&r);
        c.extend_from_slice(&palabras(&[id, 0]));
        self.mandar(gpu::CMD_RESOURCE_FLUSH, &c)
    }

    fn quitar(&mut self, id: u32) -> u32 {
        self.mandar(gpu::CMD_RESOURCE_UNREF, &palabras(&[id, 0]))
    }

    fn roto(&mut self) -> bool {
        self.r(GPU + R_STATUS) & S_DEVICE_NEEDS_RESET != 0
    }
}

fn palabras(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Un recurso de `ancho` × `alto` (B8G8R8X8) con su respaldo en PIXELES, en trozos de 4 KB, con el píxel (x, y) =
/// x | y << 8 | 0x40 << 16, ya transferido entero y en la pantalla 0.
fn recurso_listo(b: &mut Banco, id: u32, ancho: u32, alto: u32) {
    assert_eq!(b.crear(id, gpu::FORMATO_B8G8R8X8, ancho, alto), gpu::RESP_OK_NODATA);
    let bytes = ancho * alto * 4;
    let mut px = Vec::new();
    for y in 0..alto {
        for x in 0..ancho {
            px.extend_from_slice(&(x | y << 8 | 0x40 << 16).to_le_bytes());
        }
    }
    b.mem().escribir(PIXELES, &px).unwrap();
    let entradas: Vec<(u64, u32)> = (0..bytes.div_ceil(4096)).map(|i| (PIXELES + 4096 * i as u64, (bytes - 4096 * i).min(4096))).collect();
    assert_eq!(b.respaldar(id, &entradas), gpu::RESP_OK_NODATA);
    assert_eq!(b.transferir(id, [0, 0, ancho, alto], 0), gpu::RESP_OK_NODATA);
    assert_eq!(b.pantalla(0, id, [0, 0, ancho, alto]), gpu::RESP_OK_NODATA);
}

#[test]
fn identificacion_y_pantallas() {
    let mut b = banco_con(&[(640, 480), (800, 600)]);
    assert_eq!(b.r(GPU + R_DEVICE_ID), gpu::ID_GPU);
    b.w(GPU + R_DEVICE_FEATURES_SEL, 0);
    assert_eq!(b.r(GPU + R_DEVICE_FEATURES) & 1, 0, "sin VIRGL");
    assert_eq!(b.r(GPU + R_CONFIG + 8), 2, "num_scanouts");
    let (t, r) = b.mandar_con(gpu::CMD_GET_DISPLAY_INFO, 0, 0, &[], 512);
    assert_eq!(t, gpu::RESP_OK_DISPLAY_INFO);
    assert_eq!(r.len(), 24 + 16 * 24);
    let modo = |i: usize, k: usize| u32::from_le_bytes(r[24 + 24 * i + k..24 + 24 * i + k + 4].try_into().unwrap());
    assert_eq!((modo(0, 8), modo(0, 12), modo(0, 16)), (640, 480, 1));
    assert_eq!((modo(1, 8), modo(1, 12), modo(1, 16)), (800, 600, 1));
    assert_eq!(modo(2, 16), 0);
}

#[test]
fn camino_normal() {
    let mut b = banco();
    recurso_listo(&mut b, 7, 100, 20);
    assert_eq!(b.vaciar(7, [10, 5, 30, 4]), gpu::RESP_OK_NODATA);
    let p = b.ext.0.borrow();
    assert_eq!(p.mostrado.len(), 1);
    let (n, d, filas) = &p.mostrado[0];
    assert_eq!((*n, *d), (0, Rect { x: 10, y: 5, ancho: 30, alto: 4 }));
    assert_eq!(filas[0][0], 0xff40_050a);
    assert_eq!(filas[3][29], 0xff40_0827);
}

#[test]
fn valla_y_respuesta() {
    let mut b = banco();
    let (t, r) = b.mandar_con(gpu::CMD_RESOURCE_CREATE_2D, gpu::FLAG_FENCE, 0x1234_5678_9abc, &palabras(&[1, 2, 8, 8]), 24);
    assert_eq!(t, gpu::RESP_OK_NODATA);
    assert_eq!(u32::from_le_bytes(r[4..8].try_into().unwrap()), gpu::FLAG_FENCE);
    assert_eq!(u64::from_le_bytes(r[8..16].try_into().unwrap()), 0x1234_5678_9abc);
}

#[test]
fn transferencia_de_un_trozo_con_desplazamiento() {
    let mut b = banco();
    recurso_listo(&mut b, 1, 64, 64);
    // Cambia en el respaldo el rectángulo (8, 16) de 4 × 2 y transfiere solo ese, con el desplazamiento de Linux
    // (y·paso + x·4).
    let paso = 64 * 4;
    for y in 16..18u64 {
        for x in 8..12u64 {
            b.mem().escribir_u32(PIXELES + y * paso + x * 4, 0x00ab_cdef).unwrap();
        }
    }
    assert_eq!(b.transferir(1, [8, 16, 4, 2], 16 * paso + 8 * 4), gpu::RESP_OK_NODATA);
    assert_eq!(b.vaciar(1, [0, 0, 64, 64]), gpu::RESP_OK_NODATA);
    let p = b.ext.0.borrow();
    let filas = &p.mostrado.last().unwrap().2;
    assert_eq!(filas[16][8], 0xffab_cdef);
    assert_eq!(filas[17][11], 0xffab_cdef);
    assert_eq!(filas[17][12], 0xff40_110c, "lo de al lado, sin tocar");
    assert_eq!(filas[15][8], 0xff40_0f08);
}

#[test]
fn trozo_de_pantalla_y_formatos() {
    let mut b = banco();
    // R8G8B8A8: los bytes en memoria son R, G, B, A.
    assert_eq!(b.crear(3, gpu::FORMATO_R8G8B8A8, 16, 16), gpu::RESP_OK_NODATA);
    b.mem().escribir(PIXELES, &[0x11, 0x22, 0x33, 0x44].repeat(256)).unwrap();
    assert_eq!(b.respaldar(3, &[(PIXELES, 1024)]), gpu::RESP_OK_NODATA);
    assert_eq!(b.transferir(3, [0, 0, 16, 16], 0), gpu::RESP_OK_NODATA);
    // La pantalla muestra el trozo (4, 4) de 8 × 8: un vaciado fuera de él no muestra nada; uno que lo cruza, el cruce.
    assert_eq!(b.pantalla(0, 3, [4, 4, 8, 8]), gpu::RESP_OK_NODATA);
    assert_eq!(b.vaciar(3, [0, 0, 4, 4]), gpu::RESP_OK_NODATA);
    assert!(b.ext.0.borrow().mostrado.is_empty());
    assert_eq!(b.vaciar(3, [10, 10, 100, 100]), gpu::RESP_OK_NODATA);
    let p = b.ext.0.borrow();
    let (_, d, filas) = &p.mostrado[0];
    assert_eq!(*d, Rect { x: 6, y: 6, ancho: 2, alto: 2 });
    assert_eq!(filas[0][0], 0xff11_2233);
}

#[test]
fn quitar_un_recurso_en_pantalla_la_apaga() {
    let mut b = banco();
    recurso_listo(&mut b, 9, 32, 32);
    assert_eq!(b.quitar(9), gpu::RESP_OK_NODATA);
    assert_eq!(b.ext.0.borrow().apagadas, vec![0]);
    assert_eq!(b.vaciar(9, [0, 0, 32, 32]), gpu::RESP_ERR_INVALID_RESOURCE_ID);
    // Y se puede volver a crear con el mismo número.
    recurso_listo(&mut b, 9, 32, 32);
    assert_eq!(b.pantalla(0, 0, [0, 0, 0, 0]), gpu::RESP_OK_NODATA);
    assert_eq!(b.ext.0.borrow().apagadas, vec![0, 0]);
}

#[test]
fn reinicio_suelta_todo() {
    let mut b = banco();
    recurso_listo(&mut b, 1, 64, 64);
    let (c, k) = (Cola::en(COLAS, 16), Cola::en(COLAS + 0x1000, 8));
    b.control = c;
    b.cursor = k;
    b.arrancar(&[c, k]);
    assert_eq!(b.ext.0.borrow().apagadas, vec![0]);
    assert_eq!(b.vaciar(1, [0, 0, 1, 1]), gpu::RESP_ERR_INVALID_RESOURCE_ID);
    // Y el presupuesto vuelve entero: cabe uno del tope.
    assert_eq!(b.crear(1, gpu::FORMATO_B8G8R8X8, 1024, 1024), gpu::RESP_OK_NODATA);
}

#[test]
fn cursor() {
    let mut b = banco();
    let mut o = palabras(&[gpu::CMD_MOVE_CURSOR, 0, 0, 0, 0, 0]);
    o.extend_from_slice(&palabras(&[0, 10, 20, 0, 0, 0, 0, 0]));
    b.mem().escribir(ORDENES, &o).unwrap();
    assert_eq!(b.cadena(true, &[(ORDENES, o.len() as u32, false), (RESPUESTAS, 24, true)]), 24);
    assert_eq!(b.mem().leer_u32(RESPUESTAS).unwrap(), gpu::RESP_OK_NODATA);
    // Una del cursor sin respuesta (Linux no la pide): se toma igual.
    assert_eq!(b.cadena(true, &[(ORDENES, o.len() as u32, false)]), 0);
    // La de control por la cola del cursor no vale.
    let o = palabras(&[gpu::CMD_GET_DISPLAY_INFO, 0, 0, 0, 0, 0]);
    b.mem().escribir(ORDENES, &o).unwrap();
    b.cadena(true, &[(ORDENES, 24, false), (RESPUESTAS, 24, true)]);
    assert_eq!(b.mem().leer_u32(RESPUESTAS).unwrap(), gpu::RESP_ERR_UNSPEC);
    assert!(!b.roto());
}

#[test]
fn hostil_recursos_enormes_y_numeros() {
    let mut b = banco();
    use gpu::*;
    assert_eq!(b.crear(0, FORMATO_B8G8R8X8, 8, 8), RESP_ERR_INVALID_RESOURCE_ID, "el 0");
    assert_eq!(b.crear(5, FORMATO_B8G8R8X8, 8, 8), RESP_OK_NODATA);
    assert_eq!(b.crear(5, FORMATO_B8G8R8X8, 8, 8), RESP_ERR_INVALID_RESOURCE_ID, "repetido");
    assert_eq!(b.crear(6, 999, 8, 8), RESP_ERR_INVALID_PARAMETER, "formato");
    assert_eq!(b.crear(6, FORMATO_B8G8R8X8, 0, 8), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.crear(6, FORMATO_B8G8R8X8, u32::MAX, u32::MAX), RESP_ERR_INVALID_PARAMETER, "desborda");
    assert_eq!(b.crear(6, FORMATO_B8G8R8X8, 65536, 65536), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.crear(6, FORMATO_B8G8R8X8, 16384, 16384), RESP_ERR_OUT_OF_MEMORY, "1 GB, más que el tope");
    // Llenar el presupuesto: 4 MB en recursos de 1 MB; el quinto no cabe; al quitar uno, cabe.
    for id in 10..13 {
        assert_eq!(b.crear(id, FORMATO_B8G8R8X8, 512, 512), RESP_OK_NODATA);
    }
    assert_eq!(b.crear(13, FORMATO_B8G8R8X8, 512, 512), RESP_ERR_OUT_OF_MEMORY);
    assert_eq!(b.quitar(10), RESP_OK_NODATA);
    assert_eq!(b.crear(13, FORMATO_B8G8R8X8, 512, 511), RESP_OK_NODATA);
    // Números que no existen en todas las órdenes.
    assert_eq!(b.quitar(77), RESP_ERR_INVALID_RESOURCE_ID);
    assert_eq!(b.quitar(10), RESP_ERR_INVALID_RESOURCE_ID, "ya quitado");
    assert_eq!(b.respaldar(77, &[(PIXELES, 4096)]), RESP_ERR_INVALID_RESOURCE_ID);
    assert_eq!(b.transferir(77, [0, 0, 1, 1], 0), RESP_ERR_INVALID_RESOURCE_ID);
    assert_eq!(b.vaciar(77, [0, 0, 1, 1]), RESP_ERR_INVALID_RESOURCE_ID);
    assert_eq!(b.pantalla(0, 77, [0, 0, 1, 1]), RESP_ERR_INVALID_RESOURCE_ID);
    assert_eq!(b.pantalla(99, 5, [0, 0, 1, 1]), RESP_ERR_INVALID_SCANOUT_ID);
    assert_eq!(b.pantalla(u32::MAX, 0, [0, 0, 0, 0]), RESP_ERR_INVALID_SCANOUT_ID);
    assert_eq!(b.mandar(CMD_RESOURCE_DETACH_BACKING, &palabras(&[77, 0])), RESP_ERR_INVALID_RESOURCE_ID);
    assert!(!b.roto());
    recurso_listo(&mut b, 100, 32, 32);
    assert_eq!(b.vaciar(100, [0, 0, 32, 32]), RESP_OK_NODATA);
}

#[test]
fn hostil_respaldos() {
    let mut b = banco();
    use gpu::*;
    assert_eq!(b.crear(1, FORMATO_B8G8R8X8, 64, 64), RESP_OK_NODATA);
    assert_eq!(b.respaldar(1, &[]), RESP_ERR_INVALID_PARAMETER, "sin entradas");
    assert_eq!(b.respaldar(1, &[(0x1000, 4096)]), RESP_ERR_INVALID_PARAMETER, "fuera de la RAM");
    assert_eq!(b.respaldar(1, &[(RAM + TAM_RAM as u64 - 100, 4096)]), RESP_ERR_INVALID_PARAMETER, "se sale por el final");
    assert_eq!(b.respaldar(1, &[(u64::MAX - 10, 4096)]), RESP_ERR_INVALID_PARAMETER, "desborda");
    assert_eq!(b.respaldar(1, &[(PIXELES, 0)]), RESP_ERR_INVALID_PARAMETER, "largo 0");
    assert_eq!(b.respaldar(1, &[(PIXELES, 4096), (0x4000, 4096)]), RESP_ERR_INVALID_PARAMETER, "una buena y una mala");
    // Dice 1000 entradas y trae 1.
    let mut c = palabras(&[1, 1000]);
    c.extend_from_slice(&PIXELES.to_le_bytes());
    c.extend_from_slice(&palabras(&[4096, 0]));
    assert_eq!(b.mandar(CMD_RESOURCE_ATTACH_BACKING, &c), RESP_ERR_INVALID_PARAMETER);
    // Más entradas que el tope.
    assert_eq!(b.mandar(CMD_RESOURCE_ATTACH_BACKING, &palabras(&[1, MAX_ENTRADAS + 1])), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.mandar(CMD_RESOURCE_ATTACH_BACKING, &palabras(&[1, u32::MAX])), RESP_ERR_INVALID_PARAMETER);
    // Transferir sin respaldo.
    assert_eq!(b.transferir(1, [0, 0, 64, 64], 0), RESP_ERR_UNSPEC);
    // Uno bueno, pero corto (una página para 16 KB): lo que se sale no se transfiere.
    assert_eq!(b.respaldar(1, &[(PIXELES, 4096)]), RESP_OK_NODATA);
    assert_eq!(b.respaldar(1, &[(PIXELES, 4096)]), RESP_ERR_UNSPEC, "dos veces");
    assert_eq!(b.transferir(1, [0, 0, 64, 16], 0), RESP_OK_NODATA);
    assert_eq!(b.transferir(1, [0, 0, 64, 17], 0), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.transferir(1, [0, 15, 64, 1], 15 * 256 + 4), RESP_ERR_INVALID_PARAMETER, "la última fila, corrida");
    assert_eq!(b.transferir(1, [0, 0, 1, 1], 4096), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.transferir(1, [0, 0, 1, 1], u64::MAX - 2), RESP_ERR_INVALID_PARAMETER, "desplazamiento que desborda");
    // Rectángulos fuera del recurso o que desbordan.
    assert_eq!(b.transferir(1, [60, 0, 5, 1], 0), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.transferir(1, [u32::MAX, 0, 2, 1], 0), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.transferir(1, [0, 1, 1, u32::MAX], 0), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.pantalla(0, 1, [0, 0, 65, 64]), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.pantalla(0, 1, [0, 0, 0, 0]), RESP_ERR_INVALID_PARAMETER, "trozo vacío");
    assert_eq!(b.pantalla(0, 1, [u32::MAX, u32::MAX, 2, 2]), RESP_ERR_INVALID_PARAMETER);
    assert_eq!(b.vaciar(1, [u32::MAX, u32::MAX, u32::MAX, u32::MAX]), RESP_OK_NODATA, "se recorta");
    // El respaldo apunta a la RAM y el invitado la cambia por otra que no existe: no puede, la dirección está
    // comprobada al darla. Al quitarlo, transferir vuelve a fallar.
    assert_eq!(b.mandar(CMD_RESOURCE_DETACH_BACKING, &palabras(&[1, 0])), RESP_OK_NODATA);
    assert_eq!(b.transferir(1, [0, 0, 1, 1], 0), RESP_ERR_UNSPEC);
    assert!(!b.roto());
    recurso_listo(&mut b, 2, 16, 16);
    assert_eq!(b.vaciar(2, [0, 0, 16, 16]), RESP_OK_NODATA);
    assert_eq!(b.ext.0.borrow().mostrado.len(), 1);
}

#[test]
fn hostil_ordenes_rotas() {
    let mut b = banco();
    use gpu::*;
    // Cabecera corta (10 bytes).
    b.mem().escribir(ORDENES, &palabras(&[CMD_GET_DISPLAY_INFO, 0, 0])).unwrap();
    assert_eq!(b.cadena(false, &[(ORDENES, 10, false), (RESPUESTAS, 64, true)]), 24);
    assert_eq!(b.mem().leer_u32(RESPUESTAS).unwrap(), RESP_ERR_UNSPEC);
    // Órdenes que no existen o que piden lo que no se ofrece (3D, EDID, capsets).
    for t in [0, 0x999, 0x0200, 0x0201, 0x0207, CMD_GET_EDID, CMD_GET_CAPSET_INFO, CMD_GET_CAPSET, 0x010c, u32::MAX] {
        assert_eq!(b.mandar(t, &[0; 32]), RESP_ERR_UNSPEC, "orden {t:#x}");
    }
    // Cuerpos cortos.
    assert_eq!(b.mandar(CMD_RESOURCE_CREATE_2D, &palabras(&[1, 2])), RESP_ERR_UNSPEC);
    assert_eq!(b.mandar(CMD_TRANSFER_TO_HOST_2D, &[0; 10]), RESP_ERR_UNSPEC);
    assert_eq!(b.mandar(CMD_SET_SCANOUT, &[0; 23]), RESP_ERR_UNSPEC);
    // Sin sitio para la respuesta, o con poco: la información de pantallas no cabe en 30 bytes.
    let (t, r) = b.mandar_con(CMD_GET_DISPLAY_INFO, 0, 0, &[], 30);
    assert_eq!((t, r.len()), (RESP_ERR_UNSPEC, 24));
    let (_, r) = b.mandar_con(CMD_GET_DISPLAY_INFO, 0, 0, &[], 10);
    assert_eq!(r.len(), 10);
    b.mem().escribir(ORDENES, &palabras(&[CMD_GET_DISPLAY_INFO, 0, 0, 0, 0, 0])).unwrap();
    assert_eq!(b.cadena(false, &[(ORDENES, 24, false)]), 0);
    // Una orden con la parte de lectura fuera de la RAM es una cola mal hecha: esa sí rompe el aparato.
    assert!(!b.roto());
    b.cadena(false, &[(0x10, 24, false), (RESPUESTAS, 24, true)]);
    assert!(b.roto());
    // Y tras reiniciarlo, funciona.
    let (c, k) = (Cola::en(COLAS, 16), Cola::en(COLAS + 0x1000, 8));
    b.control = c;
    b.cursor = k;
    b.arrancar(&[c, k]);
    assert!(!b.roto());
    recurso_listo(&mut b, 1, 8, 8);
    assert_eq!(b.vaciar(1, [0, 0, 8, 8]), RESP_OK_NODATA);
}

#[test]
fn gpu_a_lo_loco() {
    let mut s: u64 = 0x2545_f491_4f6c_dd1d;
    let mut azar = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    use gpu::*;
    for _ in 0..20 {
        let mut b = banco_con(&[(320, 200), (64, 64)]);
        for _ in 0..400 {
            let r = azar();
            // Números y tamaños pequeños la mayoría de las veces, para que pasen las comprobaciones y lleguen lejos.
            let mut v = || {
                let x = azar();
                match x % 4 {
                    0 => (x >> 8) as u32,
                    1 => (x >> 8) as u32 % 4,
                    _ => (x >> 8) as u32 % 300,
                }
            };
            let tipo = [
                CMD_GET_DISPLAY_INFO,
                CMD_RESOURCE_CREATE_2D,
                CMD_RESOURCE_UNREF,
                CMD_SET_SCANOUT,
                CMD_RESOURCE_FLUSH,
                CMD_TRANSFER_TO_HOST_2D,
                CMD_RESOURCE_ATTACH_BACKING,
                CMD_RESOURCE_DETACH_BACKING,
                (r >> 32) as u32,
            ][(r % 9) as usize];
            let mut cuerpo = palabras(&[v(), v(), v(), v(), v(), v(), v(), v()]);
            if tipo == CMD_RESOURCE_CREATE_2D {
                cuerpo[4..8].copy_from_slice(&[FORMATO_B8G8R8X8, FORMATO_R8G8B8A8, 999][(r >> 20) as usize % 3].to_le_bytes());
            }
            if tipo == CMD_RESOURCE_ATTACH_BACKING {
                // Entradas en la RAM o cerca.
                for _ in 0..(r >> 40) % 5 {
                    let dir = RAM + (azar() % (TAM_RAM as u64 + 0x10000)) - 0x8000;
                    cuerpo.extend_from_slice(&dir.to_le_bytes());
                    cuerpo.extend_from_slice(&palabras(&[(azar() % 0x30000) as u32, 0]));
                }
            }
            let sitio = [512, 24, 8, 0][(r >> 50) as usize % 4];
            if sitio == 0 {
                b.mem().escribir(ORDENES, &palabras(&[tipo, 0, 0, 0, 0, 0])).unwrap();
                b.cadena(false, &[(ORDENES, 24, false)]);
            } else {
                b.mandar_con(tipo, (r >> 60) as u32 & 1, r, &cuerpo, sitio);
            }
            assert!(!b.roto());
        }
        // Al final, sigue funcionando (vaciado, para tener sitio).
        let (c, k) = (Cola::en(COLAS, 16), Cola::en(COLAS + 0x1000, 8));
        b.control = c;
        b.cursor = k;
        b.arrancar(&[c, k]);
        assert_eq!(b.crear(4242, FORMATO_B8G8R8X8, 4, 4), RESP_OK_NODATA);
    }
}
