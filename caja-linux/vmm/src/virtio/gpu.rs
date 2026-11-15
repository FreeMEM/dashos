//! virtio-gpu en 2D (virtio 1.2 §5.7): la pantalla del invitado y sus recursos, sin 3D (S25,
//! `docs/diseño/S25-gpu-caja-linux.md`).
//!
//! Dos colas: 0 (controlq), las órdenes; 1 (cursorq), el cursor. Cada orden empieza con una cabecera de 24 bytes
//! (little-endian): type u32, flags u32, fence_id u64, ctx_id u32, ring_idx u8 y 3 de relleno. La respuesta, también
//! con su cabecera, va en la parte de escritura de la misma cadena.
//!
//! Un recurso es una imagen de `ancho` × `alto` píxeles de 4 bytes que el aparato guarda en su propia memoria (la de
//! DashOS, dentro de un presupuesto). El invitado le da un respaldo (páginas de su RAM, `ATTACH_BACKING`), copia de
//! ahí al recurso (`TRANSFER_TO_HOST_2D`), pone el recurso en una pantalla (`SET_SCANOUT`) y dice qué cambió
//! (`RESOURCE_FLUSH`): entonces el aparato se lo da a su [`ExtremoGpu`] (en DashOS, una ventana).
//!
//! Todo lo del invitado se comprueba: ids que no existen o repetidos, formatos, tamaños (con aritmética comprobada y el
//! presupuesto de memoria), rectángulos fuera del recurso, respaldos con demasiadas entradas, con largo 0 o fuera de
//! su RAM, y transferencias que se salen del respaldo. Cada error contesta con su código y se cuenta; el aparato no
//! pasa a DEVICE_NEEDS_RESET por eso (solo por colas mal formadas, como los demás).

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::cola::{Cadena, ColaPartida};
use super::consola::copiar_config;
use super::{AparatoVirtio, ErrorVirtio, F_INDIRECT_DESC, vaciar_cola};
use crate::memoria::MemoriaInvitado;

pub const ID_GPU: u32 = 16;

/// Características (§5.7.3). No se ofrece ninguna: ni 3D (VIRGL), ni EDID, ni recursos *blob*.
pub const F_VIRGL: u64 = 1 << 0;
pub const F_EDID: u64 = 1 << 1;
pub const F_RESOURCE_UUID: u64 = 1 << 2;
pub const F_RESOURCE_BLOB: u64 = 1 << 3;
pub const F_CONTEXT_INIT: u64 = 1 << 4;

/// Bits de `events_read` (§5.7.4).
pub const EVENTO_PANTALLA: u32 = 1;

// Órdenes (§5.7.6.7).
pub const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
pub const CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const CMD_SET_SCANOUT: u32 = 0x0103;
pub const CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
pub const CMD_GET_CAPSET_INFO: u32 = 0x0108;
pub const CMD_GET_CAPSET: u32 = 0x0109;
pub const CMD_GET_EDID: u32 = 0x010a;
pub const CMD_UPDATE_CURSOR: u32 = 0x0300;
pub const CMD_MOVE_CURSOR: u32 = 0x0301;

// Respuestas.
pub const RESP_OK_NODATA: u32 = 0x1100;
pub const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
pub const RESP_ERR_UNSPEC: u32 = 0x1200;
pub const RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
pub const RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
pub const RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
pub const RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
pub const RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

/// En `flags` de la cabecera: la orden lleva valla; la respuesta la repite (§5.7.6.7).
pub const FLAG_FENCE: u32 = 1 << 0;
/// En `flags`: `ring_idx` vale (solo con CONTEXT_INIT, que no se ofrece).
pub const FLAG_INFO_RING_IDX: u32 = 1 << 1;

// Formatos (§5.7.6.8): el nombre dice el orden de los bytes en memoria.
pub const FORMATO_B8G8R8A8: u32 = 1;
pub const FORMATO_B8G8R8X8: u32 = 2;
pub const FORMATO_A8R8G8B8: u32 = 3;
pub const FORMATO_X8R8G8B8: u32 = 4;
pub const FORMATO_R8G8B8A8: u32 = 67;
pub const FORMATO_X8B8G8R8: u32 = 68;
pub const FORMATO_A8B8G8R8: u32 = 121;
pub const FORMATO_R8G8B8X8: u32 = 134;

pub const TAM_CABECERA: usize = 24;
/// Pantallas como mucho (§5.7.4: `VIRTIO_GPU_MAX_SCANOUTS`).
pub const MAX_PANTALLAS: usize = 16;
/// Lado máximo de un recurso, en píxeles. Lo de verdad lo limita el presupuesto.
pub const MAX_LADO: u32 = 16384;
/// Recursos vivos a la vez.
pub const MAX_RECURSOS: usize = 4096;
/// Entradas de un respaldo: 16384 páginas de 4 KB son 64 MB.
pub const MAX_ENTRADAS: u32 = 16384;

const COLA_CONTROL: u16 = 0;
const COLA_CURSOR: u16 = 1;
const BYTES_POR_PIXEL: u64 = 4;

/// Un rectángulo en píxeles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub ancho: u32,
    pub alto: u32,
}

impl Rect {
    fn leer(b: &[u8]) -> Self {
        Rect { x: u32_(b, 0), y: u32_(b, 4), ancho: u32_(b, 8), alto: u32_(b, 12) }
    }

    /// Si cabe entero en un recurso de `ancho` × `alto` (sin desbordar).
    pub fn dentro(&self, ancho: u32, alto: u32) -> bool {
        self.x.checked_add(self.ancho).is_some_and(|f| f <= ancho) && self.y.checked_add(self.alto).is_some_and(|f| f <= alto)
    }

    /// La parte común con `o` (vacía si no se tocan).
    pub fn cruce(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x as u64 + self.ancho as u64).min(o.x as u64 + o.ancho as u64);
        let y1 = (self.y as u64 + self.alto as u64).min(o.y as u64 + o.alto as u64);
        if x1 <= x0 as u64 || y1 <= y0 as u64 {
            return Rect::default();
        }
        Rect { x: x0, y: y0, ancho: (x1 - x0 as u64) as u32, alto: (y1 - y0 as u64) as u32 }
    }

    pub fn vacio(&self) -> bool {
        self.ancho == 0 || self.alto == 0
    }
}

fn u32_(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn u64_(b: &[u8], i: usize) -> u64 {
    u32_(b, i) as u64 | (u32_(b, i + 4) as u64) << 32
}

/// Dónde están rojo, verde y azul dentro de los 4 bytes de un píxel de `formato`; `None` si no es uno que se admita.
pub fn posiciones(formato: u32) -> Option<(usize, usize, usize)> {
    match formato {
        FORMATO_B8G8R8A8 | FORMATO_B8G8R8X8 => Some((2, 1, 0)),
        FORMATO_A8R8G8B8 | FORMATO_X8R8G8B8 => Some((1, 2, 3)),
        FORMATO_R8G8B8A8 | FORMATO_R8G8B8X8 => Some((0, 1, 2)),
        FORMATO_X8B8G8R8 | FORMATO_A8B8G8R8 => Some((3, 2, 1)),
        _ => None,
    }
}

/// Pasa una fila de píxeles de `formato` a XRGB8888 de DashOS (u32 0xffRRGGBB). Copia lo que quepa en los dos.
pub fn a_xrgb(formato: u32, origen: &[u8], destino: &mut [u32]) {
    if matches!(formato, FORMATO_B8G8R8A8 | FORMATO_B8G8R8X8) {
        // Es el mismo orden: solo el alfa a 0xff.
        for (d, s) in destino.iter_mut().zip(origen.chunks_exact(4)) {
            *d = 0xff00_0000 | u32::from_le_bytes([s[0], s[1], s[2], 0]);
        }
        return;
    }
    let (r, g, b) = posiciones(formato).unwrap_or((2, 1, 0));
    for (d, s) in destino.iter_mut().zip(origen.chunks_exact(4)) {
        *d = 0xff00_0000 | (s[r] as u32) << 16 | (s[g] as u32) << 8 | s[b] as u32;
    }
}

/// Lo que se ve en una pantalla: su trozo del recurso.
pub struct Vista<'a> {
    /// El recurso entero, fila tras fila.
    pub datos: &'a [u8],
    /// Bytes por fila del recurso.
    pub paso: usize,
    pub formato: u32,
    /// El trozo del recurso que muestra la pantalla (lo que dijo `SET_SCANOUT`).
    pub trozo: Rect,
}

impl Vista<'_> {
    /// La fila `y` de la pantalla (0 = arriba del trozo), desde la columna `x` y `ancho` píxeles, en bytes del
    /// recurso. Vacía si se sale.
    pub fn fila(&self, x: u32, y: u32, ancho: u32) -> &[u8] {
        let (x, y) = (self.trozo.x as usize + x as usize, self.trozo.y as usize + y as usize);
        let desde = y * self.paso + x * BYTES_POR_PIXEL as usize;
        self.datos.get(desde..desde + ancho as usize * BYTES_POR_PIXEL as usize).unwrap_or(&[])
    }
}

/// El lado de DashOS: lo que se hace con lo que muestra el invitado.
pub trait ExtremoGpu {
    /// La pantalla `pantalla` muestra `vista`; cambió lo de `danado` (en píxeles de la pantalla, dentro del trozo).
    fn mostrar(&mut self, pantalla: u32, vista: &Vista<'_>, danado: Rect);
    /// La pantalla se apaga (`SET_SCANOUT` con el recurso 0, o el recurso se destruye, o el aparato se reinicia).
    fn apagar(&mut self, _pantalla: u32) {}
    /// Las cuentas del aparato, tras atender cada aviso del invitado (para el registro de quien lo tenga).
    fn cuentas(&mut self, _c: &Cuentas) {}
}

/// Cuentas para el registro y las pruebas.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cuentas {
    pub ordenes: u64,
    /// Órdenes contestadas con un error (de cualquier clase).
    pub errores: u64,
    /// Órdenes que no se entendían (cortas, de un tipo que no existe o sin sitio para la respuesta).
    pub malas: u64,
    pub transferencias: u64,
    pub bytes_transferidos: u64,
    pub presentados: u64,
    pub cursor: u64,
    pub recursos: u64,
}

struct Recurso {
    ancho: u32,
    alto: u32,
    formato: u32,
    datos: Vec<u8>,
    /// El respaldo en la RAM del invitado: (desde qué byte del respaldo, dirección, largo).
    respaldo: Vec<(u64, u64, u32)>,
    /// Bytes que suma el respaldo.
    tam_respaldo: u64,
}

impl Recurso {
    fn paso(&self) -> u64 {
        self.ancho as u64 * BYTES_POR_PIXEL
    }

    /// Copia `buf.len()` bytes del respaldo desde el byte `desp`. Falso si se sale del respaldo o de la RAM.
    fn leer_respaldo(&self, mem: &dyn MemoriaInvitado, desp: u64, buf: &mut [u8]) -> bool {
        let Some(fin) = desp.checked_add(buf.len() as u64) else { return false };
        if fin > self.tam_respaldo {
            return false;
        }
        // La entrada donde empieza: la última con `desde` <= desp.
        let mut i = self.respaldo.partition_point(|e| e.0 <= desp).saturating_sub(1);
        let (mut desp, mut hecho) = (desp, 0usize);
        while hecho < buf.len() {
            let Some(&(desde, dir, largo)) = self.respaldo.get(i) else { return false };
            let dentro = desp - desde;
            let k = ((largo as u64 - dentro) as usize).min(buf.len() - hecho);
            if mem.leer(dir + dentro, &mut buf[hecho..hecho + k]).is_err() {
                return false;
            }
            hecho += k;
            desp += k as u64;
            i += 1;
        }
        true
    }
}

#[derive(Clone, Copy)]
struct Pantalla {
    ancho: u32,
    alto: u32,
    /// El recurso que muestra y su trozo; 0 si está apagada.
    recurso: u32,
    trozo: Rect,
}

pub struct Gpu {
    extremo: Box<dyn ExtremoGpu>,
    pantallas: Vec<Pantalla>,
    recursos: BTreeMap<u32, Recurso>,
    /// Bytes de recursos vivos y el tope.
    gastado: u64,
    tope: u64,
    eventos: u32,
    /// La configuración cambió (una pantalla de otro tamaño): quien tiene el transporte llama a `config_cambiada`.
    pub config_nueva: bool,
    pub cuentas: Cuentas,
}

impl Gpu {
    /// Un aparato con estas pantallas (ancho y alto de cada una; como mucho 16) y `tope` bytes para recursos.
    pub fn nueva(extremo: Box<dyn ExtremoGpu>, pantallas: &[(u32, u32)], tope: u64) -> Self {
        let pantallas = pantallas
            .iter()
            .take(MAX_PANTALLAS)
            .map(|&(ancho, alto)| Pantalla { ancho, alto, recurso: 0, trozo: Rect::default() })
            .collect();
        Self { extremo, pantallas, recursos: BTreeMap::new(), gastado: 0, tope, eventos: 0, config_nueva: false, cuentas: Cuentas::default() }
    }

    /// Bytes de recursos vivos.
    pub fn gastado(&self) -> u64 {
        self.gastado
    }

    pub fn recursos_vivos(&self) -> usize {
        self.recursos.len()
    }

    /// La pantalla `n` tiene otro tamaño (la ventana de DashOS cambió): se avisa al invitado (EVENTO_PANTALLA).
    pub fn cambiar_pantalla(&mut self, n: usize, ancho: u32, alto: u32) {
        if let Some(p) = self.pantallas.get_mut(n)
            && (p.ancho, p.alto) != (ancho, alto)
        {
            p.ancho = ancho;
            p.alto = alto;
            self.eventos |= EVENTO_PANTALLA;
            self.config_nueva = true;
        }
    }

    fn config(&self) -> [u8; 16] {
        let mut c = [0u8; 16];
        c[0..4].copy_from_slice(&self.eventos.to_le_bytes());
        c[8..12].copy_from_slice(&(self.pantallas.len() as u32).to_le_bytes());
        c
    }

    fn apagar_pantallas_de(&mut self, recurso: u32) {
        for (i, p) in self.pantallas.iter_mut().enumerate() {
            if p.recurso == recurso && recurso != 0 {
                p.recurso = 0;
                self.extremo.apagar(i as u32);
            }
        }
    }

    /// Atiende una orden de la cola de control. Devuelve la respuesta (tipo y lo que va tras la cabecera).
    fn orden(&mut self, tipo: u32, cuerpo: &[u8], c: &Cadena, mem: &dyn MemoriaInvitado) -> (u32, Vec<u8>) {
        let corta = |n: usize| cuerpo.len() < n;
        match tipo {
            CMD_GET_DISPLAY_INFO => {
                let mut d = alloc::vec![0u8; MAX_PANTALLAS * 24];
                for (i, p) in self.pantallas.iter().enumerate() {
                    let e = &mut d[i * 24..i * 24 + 24];
                    e[8..12].copy_from_slice(&p.ancho.to_le_bytes());
                    e[12..16].copy_from_slice(&p.alto.to_le_bytes());
                    e[16..20].copy_from_slice(&1u32.to_le_bytes());
                }
                (RESP_OK_DISPLAY_INFO, d)
            }
            CMD_RESOURCE_CREATE_2D => {
                if corta(16) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                let (id, formato, ancho, alto) = (u32_(cuerpo, 0), u32_(cuerpo, 4), u32_(cuerpo, 8), u32_(cuerpo, 12));
                (self.crear(id, formato, ancho, alto), Vec::new())
            }
            CMD_RESOURCE_UNREF => {
                if corta(4) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                let id = u32_(cuerpo, 0);
                match self.recursos.remove(&id) {
                    Some(r) => {
                        self.gastado -= r.datos.len() as u64;
                        self.apagar_pantallas_de(id);
                        (RESP_OK_NODATA, Vec::new())
                    }
                    None => (RESP_ERR_INVALID_RESOURCE_ID, Vec::new()),
                }
            }
            CMD_SET_SCANOUT => {
                if corta(24) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                let (r, n, id) = (Rect::leer(cuerpo), u32_(cuerpo, 16), u32_(cuerpo, 20));
                (self.poner_pantalla(n, id, r), Vec::new())
            }
            CMD_RESOURCE_FLUSH => {
                if corta(20) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                (self.vaciar(Rect::leer(cuerpo), u32_(cuerpo, 16)), Vec::new())
            }
            CMD_TRANSFER_TO_HOST_2D => {
                if corta(28) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                (self.transferir(Rect::leer(cuerpo), u64_(cuerpo, 16), u32_(cuerpo, 24), mem), Vec::new())
            }
            CMD_RESOURCE_ATTACH_BACKING => {
                if corta(8) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                (self.respaldar(u32_(cuerpo, 0), u32_(cuerpo, 4), c, mem), Vec::new())
            }
            CMD_RESOURCE_DETACH_BACKING => {
                if corta(4) {
                    return (RESP_ERR_UNSPEC, Vec::new());
                }
                match self.recursos.get_mut(&u32_(cuerpo, 0)) {
                    Some(r) => {
                        r.respaldo = Vec::new();
                        r.tam_respaldo = 0;
                        (RESP_OK_NODATA, Vec::new())
                    }
                    None => (RESP_ERR_INVALID_RESOURCE_ID, Vec::new()),
                }
            }
            // Lo que pide características que no se ofrecen (3D, EDID, capsets), y lo que no existe.
            _ => {
                self.cuentas.malas += 1;
                (RESP_ERR_UNSPEC, Vec::new())
            }
        }
    }

    fn crear(&mut self, id: u32, formato: u32, ancho: u32, alto: u32) -> u32 {
        if id == 0 || self.recursos.contains_key(&id) {
            return RESP_ERR_INVALID_RESOURCE_ID;
        }
        if posiciones(formato).is_none() || ancho == 0 || alto == 0 || ancho > MAX_LADO || alto > MAX_LADO {
            return RESP_ERR_INVALID_PARAMETER;
        }
        let bytes = ancho as u64 * alto as u64 * BYTES_POR_PIXEL;
        if self.recursos.len() >= MAX_RECURSOS || self.gastado + bytes > self.tope {
            return RESP_ERR_OUT_OF_MEMORY;
        }
        // Sin pánico si el montón de DashOS no da para tanto.
        let mut datos = Vec::new();
        if datos.try_reserve_exact(bytes as usize).is_err() {
            return RESP_ERR_OUT_OF_MEMORY;
        }
        datos.resize(bytes as usize, 0);
        self.gastado += bytes;
        self.cuentas.recursos += 1;
        self.recursos.insert(id, Recurso { ancho, alto, formato, datos, respaldo: Vec::new(), tam_respaldo: 0 });
        RESP_OK_NODATA
    }

    fn poner_pantalla(&mut self, n: u32, id: u32, r: Rect) -> u32 {
        let Some(p) = self.pantallas.get(n as usize).copied() else { return RESP_ERR_INVALID_SCANOUT_ID };
        if id == 0 {
            if p.recurso != 0 {
                self.pantallas[n as usize].recurso = 0;
                self.extremo.apagar(n);
            }
            return RESP_OK_NODATA;
        }
        let Some(rec) = self.recursos.get(&id) else { return RESP_ERR_INVALID_RESOURCE_ID };
        if r.vacio() || !r.dentro(rec.ancho, rec.alto) {
            return RESP_ERR_INVALID_PARAMETER;
        }
        self.pantallas[n as usize].recurso = id;
        self.pantallas[n as usize].trozo = r;
        RESP_OK_NODATA
    }

    fn vaciar(&mut self, r: Rect, id: u32) -> u32 {
        let Some(rec) = self.recursos.get(&id) else { return RESP_ERR_INVALID_RESOURCE_ID };
        let r = r.cruce(&Rect { x: 0, y: 0, ancho: rec.ancho, alto: rec.alto });
        for (i, p) in self.pantallas.iter().enumerate() {
            if p.recurso != id {
                continue;
            }
            let d = r.cruce(&p.trozo);
            if d.vacio() {
                continue;
            }
            let vista = Vista { datos: &rec.datos, paso: rec.paso() as usize, formato: rec.formato, trozo: p.trozo };
            let danado = Rect { x: d.x - p.trozo.x, y: d.y - p.trozo.y, ancho: d.ancho, alto: d.alto };
            self.extremo.mostrar(i as u32, &vista, danado);
            self.cuentas.presentados += 1;
        }
        RESP_OK_NODATA
    }

    fn transferir(&mut self, r: Rect, desp: u64, id: u32, mem: &dyn MemoriaInvitado) -> u32 {
        let Some(rec) = self.recursos.get_mut(&id) else { return RESP_ERR_INVALID_RESOURCE_ID };
        if !r.dentro(rec.ancho, rec.alto) {
            return RESP_ERR_INVALID_PARAMETER;
        }
        if r.vacio() {
            return RESP_OK_NODATA;
        }
        if rec.respaldo.is_empty() {
            return RESP_ERR_UNSPEC;
        }
        let paso = rec.paso();
        let fila = r.ancho as usize * BYTES_POR_PIXEL as usize;
        // La fila h del rectángulo está en el respaldo en `desp + h·paso` (el respaldo tiene la forma del recurso).
        let Some(ultima) = (r.alto as u64 - 1).checked_mul(paso).and_then(|u| u.checked_add(desp)).and_then(|u| u.checked_add(fila as u64))
        else {
            return RESP_ERR_INVALID_PARAMETER;
        };
        if ultima > rec.tam_respaldo {
            return RESP_ERR_INVALID_PARAMETER;
        }
        let mut datos = core::mem::take(&mut rec.datos);
        let mut bien = true;
        for h in 0..r.alto as u64 {
            let destino = ((r.y as u64 + h) * paso + r.x as u64 * BYTES_POR_PIXEL) as usize;
            if !rec.leer_respaldo(mem, desp + h * paso, &mut datos[destino..destino + fila]) {
                bien = false;
                break;
            }
        }
        rec.datos = datos;
        if !bien {
            // Un trozo del respaldo fuera de la RAM del invitado: lo copiado hasta ahí se queda.
            return RESP_ERR_INVALID_PARAMETER;
        }
        self.cuentas.transferencias += 1;
        self.cuentas.bytes_transferidos += r.alto as u64 * fila as u64;
        RESP_OK_NODATA
    }

    fn respaldar(&mut self, id: u32, n: u32, c: &Cadena, mem: &dyn MemoriaInvitado) -> u32 {
        let Some(rec) = self.recursos.get_mut(&id) else { return RESP_ERR_INVALID_RESOURCE_ID };
        if !rec.respaldo.is_empty() {
            return RESP_ERR_UNSPEC;
        }
        if n == 0 || n > MAX_ENTRADAS {
            return RESP_ERR_INVALID_PARAMETER;
        }
        // Las entradas siguen a la orden en la parte de lectura de la cadena: 16 bytes cada una.
        let mut lec = c.lector();
        lec.saltar((TAM_CABECERA + 8) as u64);
        let mut buf = alloc::vec![0u8; n as usize * 16];
        if !matches!(lec.leer(mem, &mut buf), Ok(k) if k == buf.len()) {
            return RESP_ERR_INVALID_PARAMETER;
        }
        let mut respaldo = Vec::with_capacity(n as usize);
        let mut total = 0u64;
        let mut uno = [0u8; 1];
        for e in buf.chunks_exact(16) {
            let (dir, largo) = (u64_(e, 0), u32_(e, 8));
            // Cada entrada, entera en la RAM del invitado: su primer y su último byte (la RAM es un trozo seguido).
            let fin = dir.checked_add(largo as u64 - (largo > 0) as u64);
            if largo == 0 || fin.is_none() || mem.leer(dir, &mut uno).is_err() || mem.leer(fin.unwrap(), &mut uno).is_err() {
                return RESP_ERR_INVALID_PARAMETER;
            }
            respaldo.push((total, dir, largo));
            total += largo as u64;
        }
        rec.respaldo = respaldo;
        rec.tam_respaldo = total;
        RESP_OK_NODATA
    }
}

impl Gpu {
    /// Lee la orden de una cadena (cabecera y hasta 32 bytes de cuerpo), la atiende y escribe la respuesta. Devuelve
    /// los bytes escritos.
    fn contestar(&mut self, c: &Cadena, mem: &mut dyn MemoriaInvitado, cursor: bool) -> Result<u32, ErrorVirtio> {
        let mut lec = c.lector();
        let mut cab = [0u8; TAM_CABECERA];
        let leida = lec.leer(mem, &mut cab)?;
        // Lo que puede ir tras la cabecera de una orden de 2D: 32 bytes como mucho (las entradas de un respaldo se
        // leen aparte).
        let mut cuerpo = [0u8; 32];
        let n = if leida == TAM_CABECERA { lec.leer(mem, &mut cuerpo)? } else { 0 };
        self.cuentas.ordenes += 1;
        let (tipo, datos) = if leida < TAM_CABECERA {
            self.cuentas.malas += 1;
            (RESP_ERR_UNSPEC, Vec::new())
        } else if cursor {
            self.cuentas.cursor += 1;
            match u32_(&cab, 0) {
                // El cursor lo pone DashOS: se aceptan y se cuentan, sin más.
                CMD_UPDATE_CURSOR | CMD_MOVE_CURSOR if n >= 32 => (RESP_OK_NODATA, Vec::new()),
                _ => {
                    self.cuentas.malas += 1;
                    (RESP_ERR_UNSPEC, Vec::new())
                }
            }
        } else {
            self.orden(u32_(&cab, 0), &cuerpo[..n], c, &*mem)
        };
        if tipo >= RESP_ERR_UNSPEC {
            self.cuentas.errores += 1;
        }
        let mut resp = alloc::vec![0u8; TAM_CABECERA + datos.len()];
        resp[0..4].copy_from_slice(&tipo.to_le_bytes());
        // La valla se repite tal cual (el aparato acaba cada orden antes de contestar).
        if leida == TAM_CABECERA && u32_(&cab, 4) & FLAG_FENCE != 0 {
            resp[4..8].copy_from_slice(&FLAG_FENCE.to_le_bytes());
            resp[8..20].copy_from_slice(&cab[8..20]);
        }
        resp[TAM_CABECERA..].copy_from_slice(&datos);
        let mut esc = c.escritor();
        if esc.quedan() < resp.len() as u64 {
            // Sin sitio para la respuesta entera: un error con solo la cabecera, o lo que quepa de ella.
            self.cuentas.malas += 1;
            resp.truncate(TAM_CABECERA);
            resp[0..4].copy_from_slice(&RESP_ERR_UNSPEC.to_le_bytes());
            let cabe = (esc.quedan() as usize).min(TAM_CABECERA);
            return Ok(esc.escribir(mem, &resp[..cabe])? as u32);
        }
        Ok(esc.escribir(mem, &resp)? as u32)
    }
}

impl AparatoVirtio for Gpu {
    fn tipo(&self) -> u32 {
        ID_GPU
    }

    fn caracteristicas(&self) -> u64 {
        F_INDIRECT_DESC
    }

    fn colas_max(&self) -> &[u16] {
        &[256, 64]
    }

    fn leer_config(&self, desp: u64, datos: &mut [u8]) {
        copiar_config(&self.config(), desp, datos);
    }

    fn escribir_config(&mut self, desp: u64, datos: &[u8]) {
        // events_clear (desplazamiento 4): borra esos bits de events_read.
        if desp == 4 && datos.len() == 4 {
            self.eventos &= !u32_(datos, 0);
        }
    }

    fn reiniciar(&mut self) {
        for i in 0..self.pantallas.len() {
            if self.pantallas[i].recurso != 0 {
                self.pantallas[i].recurso = 0;
                self.extremo.apagar(i as u32);
            }
        }
        self.recursos.clear();
        self.gastado = 0;
        self.eventos = 0;
    }

    fn atender(&mut self, cola: u16, colas: &mut [ColaPartida], mem: &mut dyn MemoriaInvitado) -> Result<bool, ErrorVirtio> {
        let cursor = match cola {
            COLA_CONTROL => false,
            COLA_CURSOR => true,
            _ => return Ok(false),
        };
        let r = vaciar_cola(&mut colas[cola as usize], mem, |c, mem| self.contestar(c, mem, cursor));
        self.extremo.cuentas(&self.cuentas);
        r
    }
}
