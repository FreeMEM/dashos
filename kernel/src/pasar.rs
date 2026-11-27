//! X11: pasar un aparato del PCIe a una máquina virtual (`docs/diseño/S34-grafica-compartida.md`: así recibirá el
//! dominio de gráficos su gráfica). Solo en x86-64 y solo con IOMMU: sin ella, el aparato alcanzaría por DMA toda la
//! memoria física, y la llamada se niega.
//!
//! Escrito con la especificación de PCI Express 6.0 (7.5.1, la cabecera de tipo 0 y su registro de mando; 6.6.2, FLR;
//! 7.7.1 y 7.7.2, MSI y MSI-X), la de la IOMMU de AMD (n.º 48882, 2.2.5, el remapeo de interrupciones) y la de VT-d
//! de Intel (5.1 y 9.10, el remapeo de interrupciones). Sala limpia: ni VFIO ni KVM ni Xen.
//!
//! Un `Aparato` es una función del PCIe (cabecera de tipo 0) que ya no es de ningún servicio del anfitrión: el kernel
//! la apunta como suya al crear el objeto (`reclamar`) y `inicio` no da sus registros ni su permiso DMA a nadie. La
//! llave del objeto la tiene el monitor de máquinas. Con `maquina_aparato` (llamada 60) el monitor:
//!
//! - **La asigna a una máquina.** FLR si lo admite (con sus BAR repuestas). Su dominio de la IOMMU traduce con la
//!   memoria del invitado: la dirección de DMA es la física del invitado, y lo que se añada después a la máquina
//!   (`maquina_memoria`) también entra. Su MSI (o sus MSI-X: X12, una por cada una de sus primeras entradas, como mucho
//!   `MAX_VECTORES`) la programa el kernel, nunca el invitado: cada una llega al anfitrión como una interrupción de
//!   aparato, cuya llave recibe el monitor; el monitor la entrega a la vCPU como cualquier otra (el
//!   bloque de interrupciones de X10). Además, sus interrupciones pasan por el remapeo de la IOMMU: con AMD-Vi, una tabla
//!   de remapeo suya con una entrada por mensaje; con VT-d, su entrada en la tabla de todo el sistema, que solo vale para él
//!   (se comprueba su número de PCI) y apunta a su vector de siempre, con las de formato compatible bloqueadas. Aunque
//!   el invitado le haga mandar lo que quiera, no puede llevar al anfitrión otra interrupción que la suya. Con una VT-d
//!   sin remapeo (sin ECAP.IR), el aparato se pasa igual, pero podría mandar cualquiera: la llamada lo dice.
//! - **Proyecta sus BAR** en el espacio físico del invitado (NPT o EPT, sin caché) donde diga, salvo las páginas de la
//!   tabla y la PBA de MSI-X, que siguen saliendo al monitor (las emula él).
//! - **Lee y escribe su configuración**, filtrada: se lee entera salvo las BAR y la ROM (dan 0: las emula el monitor, que
//!   sabe sus tamaños por la información de cada BAR); se escribe solo el mando (memoria, bus master, paridad y SERR; la
//!   INTx siempre apagada y la E/S nunca), el estado (sus bits de borrar escribiendo 1), el tamaño de línea de caché, la
//!   latencia, la línea de interrupción (no hace nada) y el Device Control de PCI Express (sin pedir FLR). El resto
//!   (las BAR, la MSI y la MSI-X, las capacidades) da `E_DERECHOS`: el monitor lo emula.
//!
//! Al parar la máquina (o al soltarse su última vCPU: el monitor murió), el aparato se calla (sin bus master ni
//! decodificación, sin MSI), su dominio se vacía (todo su DMA falla) y sus interrupciones se abortan (su tabla de
//! AMD-Vi sin remapeo, o su entrada de VT-d quitada), y queda libre para otra máquina.

use crate::base::hiper::{self, Espacio};
use crate::cerrojo::Cerrojo;
use crate::interrupciones::Interrupcion;
use crate::iommu::{self, Dominio};
use crate::memoria::PAGINA;
use crate::pci::{self, Funcion};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

/// Los aparatos que ya tienen dueño en el anfitrión: los de los servicios (`inicio_x86.rs`) y los que se pasan a
/// máquinas. Un aparato está en uno de los dos lados, nunca en los dos.
static RECLAMADOS: Cerrojo<Vec<u16>> = Cerrojo::new(Vec::new());

/// Apunta `f` como de un dueño. Falso si ya lo tenía otro.
pub fn reclamar(f: &Funcion) -> bool {
    let n = iommu::numero(f);
    let mut r = RECLAMADOS.tomar();
    if r.contains(&n) || r.try_reserve(1).is_err() {
        return false;
    }
    r.push(n);
    true
}

/// Una BAR de memoria del aparato, como la dejó el firmware.
#[derive(Clone, Copy)]
struct Bar {
    fisica: u64,
    tamano: u64,
    bits64: bool,
    prefetch: bool,
}

/// En la información de una BAR (`info_bar`): de 64 bits, prefetchable, de E/S (no se proyecta).
pub const BAR_64: u64 = 1;
pub const BAR_PREFETCH: u64 = 2;
pub const BAR_ES: u64 = 4;

/// En lo que devuelve `asignar`: las interrupciones del aparato pasan por el remapeo de la IOMMU; se hizo FLR. En los
/// bits 15:8, cuántas interrupciones tiene (X12: una por entrada de su MSI-X, como mucho `MAX_VECTORES`; 1 con MSI).
pub const REMAPEADAS: u64 = 1;
pub const CON_FLR: u64 = 2;
pub const VECTORES_DESDE: u32 = 8;

/// Las interrupciones de un aparato pasado, como mucho (X12): las primeras entradas de su MSI-X.
pub const MAX_VECTORES: usize = crate::base::iommu::MAX_REMAPEADAS;

pub enum ErrorCfg {
    /// Tamaño, alineación o sitio que no valen.
    Argumento,
    /// Algo que el invitado no puede cambiar (lo emula el monitor).
    Derechos,
}

pub struct Aparato {
    f: Funcion,
    numero: u16,
    bars: [Option<Bar>; 6],
    /// Cuáles son de E/S (no se proyectan; el monitor las emularía).
    es: u8,
    /// Lo que no se proyecta: (BAR, desde, cuánto), en páginas enteras. La tabla y la PBA de MSI-X.
    huecos: Vec<(usize, u64, u64)>,
    /// Los registros de las seis BAR y de la ROM como los dejó el firmware: se reponen tras un FLR.
    crudas: [u32; 7],
    /// Sus MSI en el anfitrión: (interrupción, dirección y dato), una por entrada de su MSI-X (las primeras, como mucho
    /// `MAX_VECTORES`), o una si solo tiene MSI; ninguna si no tiene.
    msis: Vec<(u32, u64, u32)>,
    /// Si las de `msis` van por MSI-X.
    msix: bool,
    estado: Cerrojo<Estado>,
}

struct Estado {
    asignado: bool,
    /// Ya conectado a una máquina (dominio e interrupciones); `asignado` solo, a medias.
    dominio: Option<Dominio>,
    /// Las BAR ya proyectadas en su máquina, un bit por BAR.
    proyectadas: u8,
    /// Los objetos de sus interrupciones, mientras alguien tenga su llave: si se vuelve a asignar, son los mismos.
    interrupciones: Vec<Weak<Interrupcion>>,
    /// Y mientras está asignado, sujetos aquí (para dar sus llaves con `interrupcion`).
    activas: Vec<Arc<Interrupcion>>,
}

const CFG_BAR0: usize = 0x10;
const CFG_ROM: usize = 0x30;

impl Aparato {
    /// El objeto de `f`, si es un aparato (cabecera de tipo 0) que no tiene ningún servicio. Se calla al crearlo.
    pub fn nuevo(f: Funcion) -> Option<Arc<Self>> {
        if f.tipo_cabecera() != 0 || !reclamar(&f) {
            return None;
        }
        let numero = iommu::numero(&f);
        let mut crudas = [0u32; 7];
        for (k, c) in crudas.iter_mut().enumerate().take(6) {
            *c = f.leer(CFG_BAR0 + 4 * k);
        }
        crudas[6] = f.leer(CFG_ROM);
        let mut bars = [None; 6];
        let mut es = 0u8;
        let mut i = 0;
        while i < 6 {
            let crudo = crudas[i];
            if crudo & 1 != 0 {
                es |= 1 << i;
                i += 1;
                continue;
            }
            let bits64 = crudo & 0x6 == 0x4;
            if let Some((fisica, tamano)) = f.bar(i) {
                bars[i] = Some(Bar { fisica, tamano, bits64, prefetch: crudo & 8 != 0 });
            }
            i += if bits64 { 2 } else { 1 };
        }
        let mut huecos = Vec::new();
        if let Some(p) = f.capacidad(pci::CAP_MSIX) {
            let entradas = ((f.leer(p) >> 16) & 0x7ff) as u64 + 1;
            for (reg, largo) in [(f.leer(p + 4), entradas * 16), (f.leer(p + 8), entradas.div_ceil(64) * 8)] {
                let desde = (reg & !7) as u64;
                let inicio = desde & !(PAGINA - 1);
                let fin = (desde + largo).next_multiple_of(PAGINA);
                if huecos.try_reserve(1).is_err() {
                    return None;
                }
                huecos.push(((reg & 7) as usize, inicio, fin - inicio));
            }
        }
        // X12: una MSI del anfitrión por cada una de las primeras entradas de su MSI-X (una gráfica, un NVMe: una por
        // cola); con MSI, una. Las que no se puedan dar, no (al menos la primera).
        let msix = f.msix_entradas();
        let cuantas = if msix > 0 { msix.min(MAX_VECTORES) } else if f.capacidad(pci::CAP_MSI).is_some() { 1 } else { 0 };
        let mut msis = Vec::new();
        if msis.try_reserve_exact(cuantas).is_err() {
            return None;
        }
        for _ in 0..cuantas {
            match crate::interrupciones::msi() {
                Some(m) => msis.push(m),
                None => break,
            }
        }
        f.callar();
        let estado = Cerrojo::new(Estado { asignado: false, dominio: None, proyectadas: 0, interrupciones: Vec::new(), activas: Vec::new() });
        Arc::try_new(Aparato { f, numero, bars, es, huecos, crudas, msis, msix: msix > 0, estado }).ok()
    }

    /// Su número de PCI (bus, aparato y función).
    pub fn numero(&self) -> u16 {
        self.numero
    }

    /// El vector de su (primera) MSI en el anfitrión (para las pruebas).
    pub fn vector(&self) -> Option<u8> {
        self.msis.first().map(|&(_, _, dato)| dato as u8)
    }

    /// Cuántas interrupciones tiene (las de `msis`).
    pub fn vectores(&self) -> usize {
        self.msis.len()
    }

    /// X12: la interrupción `k` (la de la entrada `k` de su MSI-X) mientras está asignado, y su número (el que llega en
    /// el mensaje de `interrupcion_asociar`).
    pub fn interrupcion(&self, k: usize) -> Option<(Arc<Interrupcion>, u32)> {
        let e = self.estado.tomar();
        let i = e.activas.get(k)?.clone();
        let n = i.intid;
        Some((i, n))
    }

    /// ¿Está ahora en una máquina?
    pub fn asignado(&self) -> bool {
        self.estado.tomar().asignado
    }

    /// La función del PCIe (para las pruebas).
    pub fn funcion(&self) -> &Funcion {
        &self.f
    }

    /// (tamaño, banderas) de la BAR `i`: `BAR_64`, `BAR_PREFETCH`, `BAR_ES`. Tamaño 0 si no hay o es de E/S.
    pub fn info_bar(&self, i: usize) -> Option<(u64, u64)> {
        if i > 5 {
            return None;
        }
        if self.es & (1 << i) != 0 {
            return Some((0, BAR_ES));
        }
        Some(match self.bars[i] {
            Some(b) => (b.tamano, if b.bits64 { BAR_64 } else { 0 } | if b.prefetch { BAR_PREFETCH } else { 0 }),
            None => (0, 0),
        })
    }

    /// Primer paso de asignarlo a una máquina: lo toma (`Limite` si ya está en otra) y le hace FLR si lo admite. Puede
    /// dormir 100 ms: sin cerrojos tomados. Sin IOMMU, `NoHay`. Devuelve si hubo FLR.
    pub fn tomar(&self) -> Result<bool, hiper::Error> {
        if !iommu::activa() {
            return Err(hiper::Error::NoHay);
        }
        {
            let mut e = self.estado.tomar();
            if e.asignado {
                return Err(hiper::Error::Limite);
            }
            e.asignado = true;
        }
        self.f.callar();
        let flr = self.f.pedir_flr();
        if flr {
            // PCI Express 6.0, 6.6.2: 100 ms antes de volver a tocarlo. El FLR borra las BAR: se reponen.
            crate::planificador::dormir_us(100_000);
            for (k, &c) in self.crudas.iter().enumerate().take(6) {
                self.f.escribir(CFG_BAR0 + 4 * k, c);
            }
            self.f.escribir(CFG_ROM, self.crudas[6]);
            self.f.callar();
        }
        Ok(flr)
    }

    /// Segundo paso (tras `tomar`): su dominio de la IOMMU con la memoria del invitado (dirección de DMA = física del
    /// invitado) y su interrupción. Con quien llama sujetando la lista de aparatos de la máquina (para que no entre
    /// memoria nueva a medias). Si falla, el aparato queda como antes de `tomar`. Devuelve su interrupción (si tiene
    /// MSI o MSI-X) y las banderas (`REMAPEADAS`).
    pub fn conectar(&self, espacio: &Espacio) -> Result<(Option<Arc<Interrupcion>>, u64), hiper::Error> {
        let r = self.conectar_dentro(espacio);
        if r.is_err() {
            self.soltar();
        }
        r
    }

    fn conectar_dentro(&self, espacio: &Espacio) -> Result<(Option<Arc<Interrupcion>>, u64), hiper::Error> {
        let d = match iommu::dominio_para(&self.f) {
            Ok(Some(d)) => d,
            Ok(None) => return Err(hiper::Error::NoHay),
            Err(()) => return Err(hiper::Error::Memoria),
        };
        self.estado.tomar().dominio = Some(d);
        d.vaciar();
        let mut bien = true;
        espacio.por_cada_pagina(&mut |gpa, fisica| bien &= d.proyectar_pagina(gpa, fisica, true));
        d.invalidar();
        if !bien {
            return Err(hiper::Error::Memoria);
        }
        let mut banderas = (self.msis.len() as u64) << VECTORES_DESDE;
        let mut irq = None;
        if let Some(&(_, direccion, _)) = self.msis.first() {
            let destino = ((direccion >> 12) & 0xff) as u32;
            let vectores: Vec<u8> = self.msis.iter().map(|m| m.2 as u8).collect();
            // Los mensajes que manda el aparato: con remapeo, los que da la IOMMU (con el índice de su entrada; AMD-Vi:
            // el dato k; VT-d: en la dirección, en formato remapeable); sin él, los del anfitrión tal cual.
            let mensajes: Vec<(u64, u32)> = match d.remapear_interrupciones(destino, &vectores) {
                Some(m) => {
                    banderas |= REMAPEADAS;
                    m
                }
                // Con remapeo (AMD-Vi, o VT-d con él encendido), si no se pudo (sin memoria o sin entradas), no se pasa.
                None if d.con_remapeo() => return Err(hiper::Error::Memoria),
                None => {
                    crate::kprintln!("X11: VT-d sin remapeo de interrupciones (la unidad no lo tiene): el aparato pasado podría mandar cualquier MSI");
                    self.msis.iter().map(|m| (m.1, m.2)).collect()
                }
            };
            let bien = if self.msix { self.f.dar_msix(&mensajes) } else { self.f.dar_msi(mensajes[0].0, mensajes[0].1).is_some() };
            if bien {
                let mut e = self.estado.tomar();
                let mut activas = Vec::new();
                activas.try_reserve_exact(self.msis.len()).map_err(|_| hiper::Error::Memoria)?;
                for (k, m) in self.msis.iter().enumerate() {
                    let i = match e.interrupciones.get(k).and_then(|w| w.upgrade()) {
                        Some(i) => i,
                        None => Interrupcion::nueva(m.0).ok_or(hiper::Error::Limite)?,
                    };
                    activas.push(i);
                }
                e.interrupciones = activas.iter().map(Arc::downgrade).collect();
                irq = activas.first().cloned();
                e.activas = activas;
            }
        }
        // Memoria y bus master (lo que deja `dar_msi`); sin INTx nunca.
        self.f.poner_mando(pci::MANDO_MEMORIA | pci::MANDO_BUS_MASTER | pci::MANDO_SIN_INTX);
        self.estado.tomar().proyectadas = 0;
        Ok((irq, banderas))
    }

    /// Lo devuelve a nadie: callado, su dominio vacío y sus interrupciones abortadas. Las proyecciones de sus BAR se
    /// quedan en la máquina, que ya está parada (o muerta) y no vuelve a correr.
    pub fn soltar(&self) {
        let mut e = self.estado.tomar();
        if !e.asignado {
            return;
        }
        self.f.callar();
        if let Some(d) = e.dominio.take() {
            d.bloquear_interrupciones();
            d.vaciar();
        }
        e.asignado = false;
        e.proyectadas = 0;
        // Sus interrupciones se sueltan fuera del cerrojo (soltar la última referencia toma los suyos).
        let activas = core::mem::take(&mut e.activas);
        drop(e);
        drop(activas);
    }

    /// Añade a su dominio la memoria que el invitado ve desde `gpa`, recién añadida a `espacio` (`maquina_memoria` con
    /// el aparato ya en la máquina). Falso si falta memoria para las tablas.
    pub fn proyectar_memoria(&self, gpa: u64, espacio: &Espacio) -> bool {
        let Some(d) = self.estado.tomar().dominio else { return true };
        let mut bien = true;
        espacio.por_cada_pagina_de(Some(gpa), &mut |g, fisica| bien &= d.proyectar_pagina(g, fisica, true));
        d.invalidar();
        bien
    }

    /// Proyecta la BAR `i` en el invitado desde `gpa`, sin la tabla ni la PBA de MSI-X. Una vez por asignación. Las
    /// BAR de menos de una página no se pasan (en su página podría haber registros de otro aparato).
    pub fn proyectar_bar(&self, i: usize, gpa: u64, espacio: &Espacio, cupo: &Arc<crate::cupo::Cupo>) -> Result<(), hiper::Error> {
        let mut e = self.estado.tomar();
        if e.dominio.is_none() || i > 5 || e.proyectadas & (1 << i) != 0 {
            return Err(hiper::Error::Argumento);
        }
        let Some(b) = self.bars[i] else { return Err(hiper::Error::Argumento) };
        if b.tamano < PAGINA || b.fisica % PAGINA != 0 {
            return Err(hiper::Error::Argumento);
        }
        let mut huecos: Vec<(u64, u64)> = Vec::new();
        huecos.try_reserve(self.huecos.len()).map_err(|_| hiper::Error::Memoria)?;
        huecos.extend(self.huecos.iter().filter(|h| h.0 == i).map(|h| (h.1, h.2)));
        espacio.proyectar_registros(gpa, b.fisica, b.tamano, &huecos, cupo)?;
        e.proyectadas |= 1 << i;
        Ok(())
    }

    /// Lee `bytes` (1, 2 o 4) de su configuración en `off` (alineado). Las BAR y la ROM dan 0.
    pub fn config_leer(&self, off: usize, bytes: usize) -> Result<u32, ErrorCfg> {
        if !matches!(bytes, 1 | 2 | 4) || off % bytes != 0 || off + bytes > 4096 {
            return Err(ErrorCfg::Argumento);
        }
        let mut v = self.f.leer_n(off, bytes);
        for k in 0..bytes {
            let b = off + k;
            if (CFG_BAR0..CFG_BAR0 + 24).contains(&b) || (CFG_ROM..CFG_ROM + 4).contains(&b) {
                v &= !(0xff << (8 * k));
            }
        }
        Ok(v)
    }

    /// Escribe `bytes` (1, 2 o 4) de `v` en su configuración en `off` (alineado), si todos sus bytes se pueden
    /// escribir (ver arriba); el mando y el Device Control, saneados.
    pub fn config_escribir(&self, off: usize, bytes: usize, v: u32) -> Result<(), ErrorCfg> {
        if !matches!(bytes, 1 | 2 | 4) || off % bytes != 0 || off + bytes > 4096 {
            return Err(ErrorCfg::Argumento);
        }
        let devctl = self.f.capacidad(pci::CAP_PCIE).map(|p| p + 8);
        let mut v = v;
        for k in 0..bytes {
            let b = off + k;
            // (bits que se pueden poner, bits que se ponen siempre) de este byte.
            let (deja, pone): (u32, u32) = match b {
                // Mando: memoria, bus master y paridad; SERR, y la INTx siempre apagada (bit 10).
                0x04 => (0b0100_0110, 0),
                0x05 => (0b0000_0001, 0b0000_0100),
                // Estado (borrar escribiendo 1), tamaño de línea, latencia y línea de interrupción.
                0x06 | 0x07 | 0x0c | 0x0d | 0x3c => (0xff, 0),
                // Device Control de PCI Express, sin el bit 15 (pedir FLR).
                _ if devctl == Some(b) => (0xff, 0),
                _ if devctl.map(|d| d + 1) == Some(b) => (0x7f, 0),
                _ => return Err(ErrorCfg::Derechos),
            };
            let byte = ((v >> (8 * k)) & deja) | pone;
            v = (v & !(0xff << (8 * k))) | byte << (8 * k);
        }
        self.f.escribir_n(off, bytes, v);
        Ok(())
    }
}

impl Drop for Aparato {
    fn drop(&mut self) {
        self.soltar();
        self.f.callar();
        RECLAMADOS.tomar().retain(|&n| n != self.numero);
    }
}
