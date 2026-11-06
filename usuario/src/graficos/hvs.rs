//! HVS de la BCM2711: el compositor de vídeo por hardware (C23; desde C50, en un proceso).
//!
//! Escrito solo con `docs/spec/hw/hvs.md` (sala limpia). Las referencias "§" son a sus secciones.
//!
//! Primera forma: el modo de vídeo (pixel valve y HDMI) lo deja puesto el firmware, y DashOS solo cambia la lista
//! de planos del canal que el firmware tiene encendido (§ "Lo que no está claro", punto 17). Así el HVS compone por
//! hardware el escritorio, el cursor y las ventanas que se quieran como planos, sin tocar los tiempos de la pantalla.
//!
//! Sin probar: QEMU `raspi4b` no emula el HVS. En QEMU solo se comprueba la codificación de la lista (contra el
//! ejemplo de §5.10). Falta la Pi 4.
//!
//! En un proceso, los planos se dan por su dirección de bus (la que da `memoria_dma` o el firmware), y no hay que
//! limpiar cachés: la memoria DMA y la de la VideoCore se proyectan sin caché.

use super::Reg;

/// Dirección física de la ventana del HVS (§2.1), con el modo de periféricos bajo.
pub const FISICA: u64 = 0xFE40_0000;

// Registros (§3.1).
const DISPCTRL: usize = 0x000;
const DISPLIST: usize = 0x020; // + 4·canal
const DISPLACT: usize = 0x030; // + 4·canal
const DISPCTRLX: usize = 0x040; // + 0x10·canal
const DISPBKGNDX: usize = 0x044;
const DISPSTATX: usize = 0x048;
const LISTAS: usize = 0x4000; // memoria de listas, 4096 palabras (§4)

const DISPCTRL_ENABLE: u32 = 1 << 31;
const DISPCTRLX_ENABLE: u32 = 1 << 31;
const DISPBKGND_FILL: u32 = 1 << 24;

// Entrada de plano (§5).
const CTL0_END: u32 = 1 << 31;
const CTL0_VALID: u32 = 1 << 30;
const CTL0_UNITY: u32 = 1 << 15;
const CTL0_ALPHA_EXPAND: u32 = 1 << 12;
const CTL0_RGB_EXPAND: u32 = 1 << 11;
const CTL2_ALPHA_FIJO: u32 = 1 << 30;
const CTL2_PREMULT: u32 = 1 << 29;
const CONTEXTO: u32 = 0xC0C0_C0C0;

/// Las palabras 0 a 31 de la memoria de listas son del firmware (§4). DashOS usa dos zonas para el doble búfer.
const ZONA: [usize; 2] = [32, 32 + 1024];
const TAMANO_ZONA: usize = 1024;

/// Palabras de una entrada RGB sin escalado (§5.1).
pub const PALABRAS_PLANO: usize = 8;

/// Formatos de píxel que DashOS da al HVS.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Formato {
    /// Palabra nativa del kernel `0xAARRGGBB`. En little-endian (desde C33) queda en memoria B, G, R, A, como
    /// `XRGB8888`: código 7, `ORDER` 2. En big-endian sería A, R, G, B: `ORDER` 1 según §6.4, deducido y sin probar.
    Argb8888Nativo,
    /// `XRGB8888` de DRM (en memoria B, G, R, X): código 7, `ORDER` 2 (§6.3). El del ejemplo de §5.10.
    Xrgb8888Drm,
}

impl Formato {
    fn codigo_y_orden(self) -> (u32, u32) {
        match self {
            Formato::Argb8888Nativo => (7, if cfg!(target_endian = "big") { 1 } else { 2 }),
            Formato::Xrgb8888Drm => (7, 2),
        }
    }
}

/// Cómo se mezcla el plano con lo de debajo (§5.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Alfa {
    /// Opaco, o translúcido entero con el alfa fijo del plano (12 bits, `0xFFF` opaco).
    Fijo(u16),
    /// Alfa por píxel, con el color ya multiplicado por el alfa.
    PorPixelPremultiplicado,
}

/// Un plano sin escalado.
#[derive(Clone, Copy, Debug)]
pub struct Plano {
    pub x: u32,
    pub y: u32,
    pub ancho: u32,
    pub alto: u32,
    /// Dirección de bus del primer píxel. Tiene que ser del primer GB: 0xC0000000 más la física (§2.4).
    pub bus: u32,
    /// Bytes por línea.
    pub paso: u32,
    pub formato: Formato,
    pub alfa: Alfa,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    FueraDelPrimerGb,
    Medidas,
    SinSitio,
}

/// Dirección de bus que ve el HVS para una física del primer GB (§2.4).
pub fn bus(fisica: u64) -> Option<u32> {
    if fisica < 0x4000_0000 { Some((fisica + 0xC000_0000) as u32) } else { None }
}

/// ¿Es una dirección de bus del primer GB, la única que alcanza el HVS?
fn del_primer_gb(bus: u32) -> bool {
    bus >= 0xC000_0000
}

impl Plano {
    /// Escribe la entrada del plano (§5.1, caso RGB sin escalado) y devuelve cuántas palabras usó.
    pub fn codificar(&self, p: &mut [u32]) -> Result<usize, Error> {
        if self.ancho == 0 || self.alto == 0 || self.ancho > 8191 || self.alto > 8191 || self.x > 4095 || self.y > 4095
            || self.paso > 0xffff
        {
            return Err(Error::Medidas);
        }
        if p.len() < PALABRAS_PLANO {
            return Err(Error::SinSitio);
        }
        if !del_primer_gb(self.bus) {
            return Err(Error::FueraDelPrimerGb);
        }
        let puntero = self.bus;
        let (codigo, orden) = self.formato.codigo_y_orden();
        p[0] = CTL0_VALID | ((PALABRAS_PLANO as u32) << 24) | CTL0_UNITY | (orden << 13) | CTL0_ALPHA_EXPAND
            | CTL0_RGB_EXPAND | codigo;
        p[1] = (self.y << 16) | self.x; // POS0 (§5.3)
        p[2] = match self.alfa {
            Alfa::Fijo(a) => CTL2_ALPHA_FIJO | (((a & 0xfff) as u32) << 4),
            Alfa::PorPixelPremultiplicado => CTL2_PREMULT | (0xfff << 4),
        };
        p[3] = (self.alto << 16) | self.ancho; // POS2 (§5.6)
        p[4] = CONTEXTO; // POS3
        p[5] = puntero; // PTR0
        p[6] = CONTEXTO;
        p[7] = self.paso; // PITCH0 (§5.7)
        Ok(PALABRAS_PLANO)
    }
}

/// Codifica una lista entera: los planos de abajo arriba y la palabra de fin (§4). Devuelve su longitud.
pub fn codificar_lista(planos: &[Plano], p: &mut [u32]) -> Result<usize, Error> {
    let mut n = 0;
    for pl in planos {
        n += pl.codificar(&mut p[n..])?;
    }
    if n >= p.len() {
        return Err(Error::SinSitio);
    }
    p[n] = CTL0_END;
    Ok(n + 1)
}

/// El HVS y el canal que dejó encendido el firmware.
pub struct Hvs {
    /// Donde están proyectados sus registros en el proceso.
    base: usize,
    pub canal: usize,
    pub ancho: u32,
    pub alto: u32,
    zona: usize,
}

impl Hvs {
    fn r(&self, off: usize) -> Reg {
        Reg(self.base + off)
    }

    /// Busca el HVS encendido y un canal activo, con sus registros proyectados en `base`. Quien da la llave de los
    /// registros (el kernel) ya ha comprobado que el HVS existe: en QEMU, leerlo daría un fallo de bus.
    pub fn detectar(base: usize) -> Option<Self> {
        let mut h = Self { base, canal: 0, ancho: 0, alto: 0, zona: 1 };
        if h.r(DISPCTRL).leer() & DISPCTRL_ENABLE == 0 {
            return None;
        }
        for c in 0..3 {
            let v = h.r(DISPCTRLX + 0x10 * c).leer();
            if v & DISPCTRLX_ENABLE != 0 {
                // §3.6: ancho en 28:16, alto en 12:0.
                h.canal = c;
                h.ancho = (v >> 16) & 0x1fff;
                h.alto = v & 0x1fff;
                return Some(h);
            }
        }
        None
    }

    /// Índice de la lista que el hardware usa ahora (§3.5).
    pub fn lista_activa(&self) -> u32 {
        self.r(DISPLACT + 4 * self.canal).leer()
    }

    /// Línea que compone el HVS (§3.8).
    pub fn linea(&self) -> u32 {
        self.r(DISPSTATX + 0x10 * self.canal).leer() & 0xfff
    }

    /// Escribe la lista en la zona libre y la pone para el cuadro siguiente (§12.4). Devuelve su índice; el cambio
    /// está hecho cuando `lista_activa` lo devuelva.
    pub fn instalar(&mut self, planos: &[Plano]) -> Result<u32, Error> {
        let mut palabras = [0u32; TAMANO_ZONA];
        let n = codificar_lista(planos, &mut palabras)?;
        let zona = 1 - self.zona;
        let inicio = ZONA[zona];
        for (i, &w) in palabras[..n].iter().enumerate() {
            self.r(LISTAS + 4 * (inicio + i)).escribir(w); // siempre de 32 bits (§2.5)
        }
        // Fondo con relleno, como Linux en la BCM2711 (§3.7, §9).
        let fondo = self.r(DISPBKGNDX + 0x10 * self.canal);
        fondo.escribir(fondo.leer() | DISPBKGND_FILL);
        unsafe { core::arch::asm!("dsb sy") };
        self.r(DISPLIST + 4 * self.canal).escribir(inicio as u32);
        self.zona = zona;
        Ok(inicio as u32)
    }

    /// Espera a que el hardware use la lista `indice`, como mucho `intentos` vueltas.
    pub fn esperar(&self, indice: u32, intentos: usize) -> bool {
        (0..intentos).any(|_| self.lista_activa() == indice)
    }
}
