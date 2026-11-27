//! El portapapeles para un programa nativo de DashOS (H9, rama `caja-linux-apps`): lo que necesita un cliente de
//! Wayland para copiar y pegar con los programas de Linux (y con los demás), con la selección de siempre
//! (`wl_data_device`) y la primaria (`zwp_primary_selection_device_v1`). Lo usa la terminal de la caja Linux.
//!
//! En DashOS el descriptor de un `receive` o un `send` es la llave de un extremo de canal: quien pega crea un canal, da
//! un extremo con `receive` y lee del suyo hasta `E_CERRADO`; quien copia recibe el extremo en `send`, escribe en él lo
//! copiado en mensajes de hasta 256 bytes y lo cierra.
//!
//! Con topes: se pega como mucho `MAX_PEGAR` bytes y se espera como mucho `PLAZO_US`; lo que se copia, igual. Una fuente
//! de Linux que escribe sin fin o que nunca cierra no deja colgado al programa.

use alloc::string::String;
use alloc::vec::Vec;

use wayland::Escritor;
use wayland::tablas::{
    wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_registry, zwp_primary_selection_device_manager_v1,
    zwp_primary_selection_device_v1, zwp_primary_selection_offer_v1, zwp_primary_selection_source_v1,
};

/// Lo más que se pega de una vez.
pub const MAX_PEGAR: usize = 4096;
/// Lo que se espera a una fuente (al pegar) o a quien pega (al copiar).
pub const PLAZO_US: u64 = 10_000_000;
/// Los tipos de texto, del preferido al último.
pub const TIPOS: [&str; 5] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "TEXT", "STRING"];

/// Los números de objeto: por encima de los de `ventana_cliente` (hasta 14) y de los de la pantalla de la caja (40 a 44),
/// y por debajo de los avisos de frame de la terminal (desde 100).
const GESTOR: u32 = 60;
const DISPOSITIVO: u32 = 61;
const GESTOR_PRIMARIA: u32 = 62;
const DISPOSITIVO_PRIMARIA: u32 = 63;
/// Desde aquí, las fuentes (una por copia): lejos de los avisos de frame, que van subiendo desde 100 (y de los de la
/// pantalla, desde 0x0100_0000).
const PRIMERA_FUENTE: u32 = 0x2000_0000;

/// Una selección (0, la de siempre; 1, la primaria).
#[derive(Default)]
struct Sel {
    /// La oferta de la selección de ahora (0: ninguna) y los tipos de cada oferta viva.
    actual: u32,
    ofertas: Vec<(u32, Vec<String>)>,
    /// Nuestra fuente, si somos la selección, y su texto.
    fuente: Option<(u32, String)>,
}

/// Un pegado en marcha: el extremo de donde se lee, lo leído y hasta cuándo se espera.
struct Pegado {
    lee: u32,
    datos: Vec<u8>,
    hasta: u64,
}

/// Un envío en marcha (alguien pega lo nuestro): la llave, lo que falta y hasta cuándo.
struct Envio {
    llave: u32,
    falta: Vec<u8>,
    hasta: u64,
}

#[derive(Default)]
pub struct Portapapeles {
    sel: [Sel; 2],
    primaria: bool,
    siguiente: u32,
    pegado: Option<Pegado>,
    envios: Vec<Envio>,
    /// Cuentas, para el registro: pegados y copias servidas.
    pub pegados: u32,
    pub servidos: u32,
}

impl Portapapeles {
    /// Enlaza los gestores (si el compositor los tiene) y pide los dispositivos del asiento `asiento`.
    pub fn enlazar(&mut self, e: &mut Escritor, globales: &[(u32, String, u32)], registro: u32, asiento: u32) -> bool {
        let g = |n: &str| globales.iter().find(|x| x.1 == n).map(|x| x.0);
        let Some(gd) = g("wl_data_device_manager") else { return false };
        e.empezar(registro, wl_registry::req::BIND).uint(gd).texto(Some("wl_data_device_manager")).uint(3).uint(GESTOR).fin();
        e.empezar(GESTOR, wl_data_device_manager::req::GET_DATA_DEVICE).uint(DISPOSITIVO).uint(asiento).fin();
        if let Some(gp) = g("zwp_primary_selection_device_manager_v1") {
            e.empezar(registro, wl_registry::req::BIND).uint(gp).texto(Some("zwp_primary_selection_device_manager_v1")).uint(1).uint(GESTOR_PRIMARIA).fin();
            e.empezar(GESTOR_PRIMARIA, zwp_primary_selection_device_manager_v1::req::GET_DEVICE).uint(DISPOSITIVO_PRIMARIA).uint(asiento).fin();
            self.primaria = true;
        }
        self.siguiente = PRIMERA_FUENTE;
        true
    }

    /// Un evento del compositor: `true` si era del portapapeles. `llave` da la llave que trae (la de `send`).
    pub fn evento(&mut self, e: &mut Escritor, o: u32, orden: u16, cuerpo: &[u8], llave: &mut dyn FnMut() -> Option<u32>) -> bool {
        let palabra = |i: usize| cuerpo.get(4 * i..4 * i + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let texto = || -> String {
            let n = palabra(0) as usize;
            cuerpo.get(4..4 + n.saturating_sub(1)).and_then(|b| core::str::from_utf8(b).ok()).map(String::from).unwrap_or_default()
        };
        match (o, orden) {
            (DISPOSITIVO, wl_data_device::ev::DATA_OFFER) | (DISPOSITIVO_PRIMARIA, zwp_primary_selection_device_v1::ev::DATA_OFFER) => {
                let s = &mut self.sel[(o == DISPOSITIVO_PRIMARIA) as usize];
                if s.ofertas.len() < 8 {
                    s.ofertas.push((palabra(0), Vec::new()));
                }
            }
            (DISPOSITIVO, wl_data_device::ev::SELECTION) | (DISPOSITIVO_PRIMARIA, zwp_primary_selection_device_v1::ev::SELECTION) => {
                let k = (o == DISPOSITIVO_PRIMARIA) as usize;
                let nueva = palabra(0);
                // Las ofertas viejas se destruyen.
                let viejas: Vec<u32> = self.sel[k].ofertas.iter().map(|x| x.0).filter(|&x| x != nueva).collect();
                for v in viejas {
                    if k == 0 {
                        e.empezar(v, wl_data_offer::req::DESTROY).fin();
                    } else {
                        e.empezar(v, zwp_primary_selection_offer_v1::req::DESTROY).fin();
                    }
                }
                self.sel[k].ofertas.retain(|x| x.0 == nueva);
                self.sel[k].actual = nueva;
            }
            (DISPOSITIVO, _) | (DISPOSITIVO_PRIMARIA, _) => {}
            _ if self.sel.iter().any(|s| s.ofertas.iter().any(|x| x.0 == o)) => {
                // `offer` de una oferta (el 0 en las dos).
                let t = texto();
                for s in &mut self.sel {
                    if let Some(x) = s.ofertas.iter_mut().find(|x| x.0 == o)
                        && x.1.len() < 32
                        && t.len() < 256
                    {
                        x.1.push(t.clone());
                    }
                }
            }
            _ if self.sel.iter().any(|s| s.fuente.as_ref().is_some_and(|f| f.0 == o)) => {
                let k = self.sel.iter().position(|s| s.fuente.as_ref().is_some_and(|f| f.0 == o)).unwrap();
                let (ev_send, ev_cancelled) = if k == 0 {
                    (wl_data_source::ev::SEND, wl_data_source::ev::CANCELLED)
                } else {
                    (zwp_primary_selection_source_v1::ev::SEND, zwp_primary_selection_source_v1::ev::CANCELLED)
                };
                if orden == ev_send {
                    // Alguien pega lo nuestro: se le escribe en su llave (por partes, sin esperar).
                    if let Some(l) = llave() {
                        let texto = self.sel[k].fuente.as_ref().unwrap().1.clone().into_bytes();
                        if self.envios.len() < 8 {
                            self.envios.push(Envio { llave: l, falta: texto, hasta: dashos::ahora_us() + PLAZO_US });
                        } else {
                            dashos::cerrar(l);
                        }
                    }
                } else if orden == ev_cancelled {
                    // Ya no es la selección.
                    if k == 0 {
                        e.empezar(o, wl_data_source::req::DESTROY).fin();
                    } else {
                        e.empezar(o, zwp_primary_selection_source_v1::req::DESTROY).fin();
                    }
                    self.sel[k].fuente = None;
                }
            }
            _ => return false,
        }
        true
    }

    /// Pone `texto` como selección (`primaria` o la de siempre), con la serie de la tecla o el clic que lo pide.
    pub fn copiar(&mut self, e: &mut Escritor, primaria: bool, texto: String, serie: u32) {
        if primaria && !self.primaria || texto.is_empty() {
            return;
        }
        let k = primaria as usize;
        let id = self.siguiente;
        self.siguiente += 1;
        if primaria {
            e.empezar(GESTOR_PRIMARIA, zwp_primary_selection_device_manager_v1::req::CREATE_SOURCE).uint(id).fin();
            for t in TIPOS {
                e.empezar(id, zwp_primary_selection_source_v1::req::OFFER).texto(Some(t)).fin();
            }
            e.empezar(DISPOSITIVO_PRIMARIA, zwp_primary_selection_device_v1::req::SET_SELECTION).uint(id).uint(serie).fin();
        } else {
            e.empezar(GESTOR, wl_data_device_manager::req::CREATE_DATA_SOURCE).uint(id).fin();
            for t in TIPOS {
                e.empezar(id, wl_data_source::req::OFFER).texto(Some(t)).fin();
            }
            e.empezar(DISPOSITIVO, wl_data_device::req::SET_SELECTION).uint(id).uint(serie).fin();
        }
        // La de antes se destruye (ya no es la selección: el compositor le mandó `cancelled` o se lo mandará).
        if let Some((vieja, _)) = self.sel[k].fuente.replace((id, texto)) {
            if primaria {
                e.empezar(vieja, zwp_primary_selection_source_v1::req::DESTROY).fin();
            } else {
                e.empezar(vieja, wl_data_source::req::DESTROY).fin();
            }
        }
    }

    /// Empieza a pegar (la primaria o la de siempre): falso si no hay nada de texto que pegar o ya se está pegando.
    pub fn pegar(&mut self, e: &mut Escritor, primaria: bool) -> bool {
        if self.pegado.is_some() {
            return false;
        }
        let s = &self.sel[primaria as usize];
        let Some((oferta, tipos)) = s.ofertas.iter().find(|x| x.0 == s.actual && s.actual != 0) else { return false };
        let Some(tipo) = TIPOS.iter().find(|t| tipos.iter().any(|x| x == *t)) else { return false };
        let (lee, escribe) = dashos::canal_crear();
        if primaria {
            e.empezar(*oferta, zwp_primary_selection_offer_v1::req::RECEIVE).texto(Some(tipo)).llave(escribe).fin();
        } else {
            e.empezar(*oferta, wl_data_offer::req::RECEIVE).texto(Some(tipo)).llave(escribe).fin();
        }
        self.pegado = Some(Pegado { lee, datos: Vec::new(), hasta: dashos::ahora_us() + PLAZO_US });
        true
    }

    /// Si hay algo en marcha (para esperar menos al compositor).
    pub fn ocupado(&self) -> bool {
        self.pegado.is_some() || !self.envios.is_empty()
    }

    /// Mueve lo que esté en marcha, sin esperar: lo que se lee al pegar y lo que se escribe al copiar. Devuelve lo pegado
    /// cuando acaba (con su tope), o `None`.
    pub fn mover(&mut self) -> Option<Vec<u8>> {
        let ahora = dashos::ahora_us();
        // Lo nuestro que otros pegan.
        let mut servidos = 0;
        self.envios.retain_mut(|en| {
            while !en.falta.is_empty() {
                let n = en.falta.len().min(wayland::canal::MAX_CANAL);
                match dashos::canal_escribir(en.llave, &en.falta[..n], &[]) {
                    0 => {
                        en.falta.drain(..n);
                    }
                    dashos::E_LIMITE if ahora < en.hasta => return true,
                    _ => break,
                }
            }
            servidos += en.falta.is_empty() as u32;
            dashos::cerrar(en.llave);
            false
        });
        self.servidos += servidos;
        let p = self.pegado.as_mut()?;
        let mut buf = [0u8; wayland::canal::MAX_CANAL];
        let mut ll = [0u32; 8];
        let acabado = loop {
            let (n, k, _) = dashos::canal_leer_plazo(p.lee, &mut buf, &mut ll, 1);
            for &l in &ll[..k.min(8)] {
                dashos::cerrar(l);
            }
            if n == dashos::E_PLAZO {
                break ahora >= p.hasta;
            }
            if n < 0 {
                break true;
            }
            p.datos.extend_from_slice(&buf[..n as usize]);
            if p.datos.len() >= MAX_PEGAR {
                p.datos.truncate(MAX_PEGAR);
                break true;
            }
        };
        if !acabado {
            return None;
        }
        let p = self.pegado.take().unwrap();
        dashos::cerrar(p.lee);
        self.pegados += 1;
        Some(p.datos)
    }
}
