//! Virtio por mensajes (S35, paso 2; `docs/bitacoras/dominio-red.md`): el transporte entre un conductor virtio de
//! DashOS (`virtio-red`) y el servicio de un dominio de controladores (`dominio`), que es quien pone el aparato.
//!
//! Sigue la idea del transporte virtio-msg (borrador de Linaro para virtio 1.4): los registros de un transporte de
//! siempre se cambian por mensajes (info del aparato, funciones, estado, colas) y los avisos por eventos (`AVISO`, del
//! conductor al aparato; `USADOS`, al revés). Las colas siguen en memoria compartida: el conductor da un objeto de
//! memoria suyo (`MEMORIA`) y el bus contesta en qué dirección lo ve el aparato; esa es la "dirección de bus" de todo
//! lo que el conductor pone en sus descriptores. La codificación es propia (little-endian, sin relleno).
//!
//! Cada mensaje empieza por su tipo (u32) y la generación del aparato (u32): cada vez que el dominio arranca de nuevo,
//! la generación sube y lo de la anterior se rechaza (`VIEJA`). `APARATO` dice al conductor que hay aparato (de esa
//! generación) o que se cayó: entonces el conductor lo reinicia todo y espera al siguiente.
//!
//! Cómo se conocen: el servicio del dominio escribe `HOLA` por el canal del conductor ("dominio-red") con una llave
//! para escribirle a él; el conductor contesta `HOLA` por esa llave con otra para escribirle a él. Desde ahí, todo va
//! por esas dos llaves. Sin dependencias: se prueba en el PC (`make prueba`).

/// Los mensajes del conductor al bus (las peticiones) y lo que contesta el bus, con `RESPUESTA` en el tipo.
pub const HOLA: u32 = 1;
pub const INFO: u32 = 2;
pub const FUNCIONES: u32 = 3;
pub const MEMORIA: u32 = 4;
pub const COLA: u32 = 5;
pub const ESTADO: u32 = 6;
/// El conductor avisa colas (EVENT_AVAIL). Sin respuesta.
pub const AVISO: u32 = 7;
pub const RESPUESTA: u32 = 0x80;
/// Del bus al conductor: usados nuevos en esas colas (EVENT_USED).
pub const USADOS: u32 = 0x40;
/// Del bus al conductor: hay aparato (de esta generación) o se cayó.
pub const APARATO: u32 = 0x41;

/// Errores en las respuestas.
pub const BIEN: i32 = 0;
/// No hay aparato todavía (el dominio no ha hecho su oferta), o se cayó.
pub const SIN_APARATO: i32 = -1;
/// De otra generación del aparato.
pub const VIEJA: i32 = -2;
/// Lo pedido no vale (funciones que no ofrece, una cola fuera de la ventana, un orden que no toca).
pub const NO_VALE: i32 = -3;
/// El bus no pudo (sin memoria, la ventana no se pudo poner).
pub const NO_PUEDE: i32 = -4;

/// El tamaño máximo del espacio de configuración que viaja en `INFO`.
pub const TAM_CONFIG: usize = 64;
/// Colas como mucho por aparato en este transporte.
pub const MAX_COLAS: u32 = 2;
/// El mensaje más largo (la respuesta a `INFO`).
pub const MAX_MENSAJE: usize = 12 + 20 + TAM_CONFIG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    pub tipo: u32,
    pub colas: u32,
    pub tam_cola: u32,
    pub funciones: u64,
    pub config: [u8; TAM_CONFIG],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cola {
    pub indice: u32,
    pub tam: u32,
    pub desc: u64,
    pub disp: u64,
    pub usados: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mensaje {
    Hola,
    Info,
    Funciones(u64),
    /// Con una llave: el objeto de memoria.
    Memoria,
    Cola(Cola),
    Estado(u8),
    Aviso(u32),
    Usados(u32),
    Aparato(bool),
    RInfo(i32, Info),
    RFunciones(i32),
    /// Error, dónde ve el aparato la memoria y cuánto mide.
    RMemoria(i32, u64, u64),
    RCola(i32),
    /// Error y el estado en que queda el aparato.
    REstado(i32, u8),
}

fn u32_de(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn u64_de(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

struct Escritor<'a> {
    b: &'a mut [u8; MAX_MENSAJE],
    n: usize,
}

impl Escritor<'_> {
    fn u32(&mut self, v: u32) -> &mut Self {
        self.b[self.n..self.n + 4].copy_from_slice(&v.to_le_bytes());
        self.n += 4;
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.b[self.n..self.n + 8].copy_from_slice(&v.to_le_bytes());
        self.n += 8;
        self
    }
}

impl Mensaje {
    /// El tipo que lleva en la cabecera.
    pub fn tipo(&self) -> u32 {
        match self {
            Mensaje::Hola => HOLA,
            Mensaje::Info => INFO,
            Mensaje::Funciones(_) => FUNCIONES,
            Mensaje::Memoria => MEMORIA,
            Mensaje::Cola(_) => COLA,
            Mensaje::Estado(_) => ESTADO,
            Mensaje::Aviso(_) => AVISO,
            Mensaje::Usados(_) => USADOS,
            Mensaje::Aparato(_) => APARATO,
            Mensaje::RInfo(..) => INFO | RESPUESTA,
            Mensaje::RFunciones(_) => FUNCIONES | RESPUESTA,
            Mensaje::RMemoria(..) => MEMORIA | RESPUESTA,
            Mensaje::RCola(_) => COLA | RESPUESTA,
            Mensaje::REstado(..) => ESTADO | RESPUESTA,
        }
    }

    /// Escribe el mensaje de la generación `generacion` en `b`; devuelve cuántos bytes ocupa.
    pub fn escribir(&self, generacion: u32, b: &mut [u8; MAX_MENSAJE]) -> usize {
        let mut e = Escritor { b, n: 0 };
        e.u32(self.tipo()).u32(generacion);
        match *self {
            Mensaje::Hola | Mensaje::Info | Mensaje::Memoria => {}
            Mensaje::Funciones(f) => {
                e.u64(f);
            }
            Mensaje::Cola(c) => {
                e.u32(c.indice).u32(c.tam).u64(c.desc).u64(c.disp).u64(c.usados);
            }
            Mensaje::Estado(s) => {
                e.u32(s as u32);
            }
            Mensaje::Aviso(m) | Mensaje::Usados(m) => {
                e.u32(m);
            }
            Mensaje::Aparato(p) => {
                e.u32(p as u32);
            }
            Mensaje::RInfo(err, i) => {
                e.u32(err as u32).u32(i.tipo).u32(i.colas).u32(i.tam_cola).u64(i.funciones);
                let n = e.n;
                e.b[n..n + TAM_CONFIG].copy_from_slice(&i.config);
                e.n += TAM_CONFIG;
            }
            Mensaje::RFunciones(err) | Mensaje::RCola(err) => {
                e.u32(err as u32);
            }
            Mensaje::RMemoria(err, base, tam) => {
                e.u32(err as u32).u64(base).u64(tam);
            }
            Mensaje::REstado(err, s) => {
                e.u32(err as u32).u32(s as u32);
            }
        }
        e.n
    }

    /// Lee un mensaje: (generación, mensaje). Nada si no se entiende: un tipo que no existe, un largo que no es el
    /// suyo o un valor fuera de rango. Lo que llega por un canal puede venir de cualquiera.
    pub fn leer(b: &[u8]) -> Option<(u32, Mensaje)> {
        let tipo = u32_de(b, 0)?;
        let generacion = u32_de(b, 4)?;
        let largo = |n: usize| (b.len() == 8 + n).then_some(());
        let m = match tipo {
            HOLA => largo(0).map(|_| Mensaje::Hola)?,
            INFO => largo(0).map(|_| Mensaje::Info)?,
            MEMORIA => largo(0).map(|_| Mensaje::Memoria)?,
            FUNCIONES => {
                largo(8)?;
                Mensaje::Funciones(u64_de(b, 8)?)
            }
            COLA => {
                largo(32)?;
                Mensaje::Cola(Cola { indice: u32_de(b, 8)?, tam: u32_de(b, 12)?, desc: u64_de(b, 16)?, disp: u64_de(b, 24)?, usados: u64_de(b, 32)? })
            }
            ESTADO => {
                largo(4)?;
                Mensaje::Estado(u8::try_from(u32_de(b, 8)?).ok()?)
            }
            AVISO | USADOS => {
                largo(4)?;
                let m = u32_de(b, 8)?;
                if tipo == AVISO { Mensaje::Aviso(m) } else { Mensaje::Usados(m) }
            }
            APARATO => {
                largo(4)?;
                match u32_de(b, 8)? {
                    0 => Mensaje::Aparato(false),
                    1 => Mensaje::Aparato(true),
                    _ => return None,
                }
            }
            t if t == INFO | RESPUESTA => {
                largo(24 + TAM_CONFIG)?;
                let mut config = [0u8; TAM_CONFIG];
                config.copy_from_slice(b.get(32..32 + TAM_CONFIG)?);
                let i = Info { tipo: u32_de(b, 12)?, colas: u32_de(b, 16)?, tam_cola: u32_de(b, 20)?, funciones: u64_de(b, 24)?, config };
                Mensaje::RInfo(u32_de(b, 8)? as i32, i)
            }
            t if t == FUNCIONES | RESPUESTA || t == COLA | RESPUESTA => {
                largo(4)?;
                let e = u32_de(b, 8)? as i32;
                if t == FUNCIONES | RESPUESTA { Mensaje::RFunciones(e) } else { Mensaje::RCola(e) }
            }
            t if t == MEMORIA | RESPUESTA => {
                largo(20)?;
                Mensaje::RMemoria(u32_de(b, 8)? as i32, u64_de(b, 12)?, u64_de(b, 20)?)
            }
            t if t == ESTADO | RESPUESTA => {
                largo(8)?;
                Mensaje::REstado(u32_de(b, 8)? as i32, u8::try_from(u32_de(b, 12)?).ok()?)
            }
            _ => return None,
        };
        Some((generacion, m))
    }
}

/// Comprueba una cola que pide el conductor contra la ventana (`base`, `tam`) y el tamaño máximo del aparato (virtio
/// 1.2, §2.7): tamaño potencia de dos hasta `max`; descriptores alineados a 16 (16 bytes cada uno), el anillo del
/// conductor a 2 (6 + 2·n bytes) y el de usados a 4 (6 + 8·n bytes), todo dentro de la ventana.
pub fn cola_valida(c: &Cola, base: u64, tam: u64, max: u32) -> bool {
    let n = c.tam as u64;
    let dentro = |d: u64, largo: u64| d >= base && d.checked_add(largo).is_some_and(|f| f <= base.saturating_add(tam));
    c.indice < MAX_COLAS
        && c.tam.is_power_of_two()
        && c.tam <= max
        && c.desc % 16 == 0
        && c.disp % 2 == 0
        && c.usados % 4 == 0
        && dentro(c.desc, 16 * n)
        && dentro(c.disp, 6 + 2 * n)
        && dentro(c.usados, 6 + 8 * n)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn ida_y_vuelta(m: Mensaje) {
        let mut b = [0u8; MAX_MENSAJE];
        let n = m.escribir(7, &mut b);
        assert_eq!(Mensaje::leer(&b[..n]), Some((7, m)), "{m:?}");
        // Cortado o con un byte de más, no se entiende.
        assert_eq!(Mensaje::leer(&b[..n - 1]), None, "{m:?}");
        if n < MAX_MENSAJE {
            assert_eq!(Mensaje::leer(&b[..n + 1]), None, "{m:?}");
        }
    }

    #[test]
    fn todos_los_mensajes() {
        let mut config = [0u8; TAM_CONFIG];
        config[..6].copy_from_slice(&[0x52, 0x54, 0, 0x12, 0x34, 0x56]);
        let cola = Cola { indice: 1, tam: 64, desc: 0xa000_2000, disp: 0xa000_2400, usados: 0xa000_2800 };
        for m in [
            Mensaje::Hola,
            Mensaje::Info,
            Mensaje::Funciones(1 << 32 | 1 << 5),
            Mensaje::Memoria,
            Mensaje::Cola(cola),
            Mensaje::Estado(15),
            Mensaje::Aviso(3),
            Mensaje::Usados(1),
            Mensaje::Aparato(true),
            Mensaje::Aparato(false),
            Mensaje::RInfo(BIEN, Info { tipo: 1, colas: 2, tam_cola: 256, funciones: 1 << 32, config }),
            Mensaje::RFunciones(NO_VALE),
            Mensaje::RMemoria(BIEN, 0xa000_0000, 0x6_4000),
            Mensaje::RCola(VIEJA),
            Mensaje::REstado(SIN_APARATO, 0),
        ] {
            ida_y_vuelta(m);
        }
    }

    #[test]
    fn lo_que_no_se_entiende() {
        assert_eq!(Mensaje::leer(&[]), None);
        assert_eq!(Mensaje::leer(&[1, 0, 0]), None);
        // Tipos que no existen.
        for t in [0u32, 8, 0x3f, 0x42, 0x81 | 0x40, 0xffff_ffff, HOLA | RESPUESTA, AVISO | RESPUESTA] {
            let mut b = [0u8; 8];
            b[..4].copy_from_slice(&t.to_le_bytes());
            assert_eq!(Mensaje::leer(&b), None, "{t:#x}");
        }
        // Un estado de más de 8 bits y un APARATO que no es 0 ni 1.
        let mut b = [0u8; 12];
        b[..4].copy_from_slice(&ESTADO.to_le_bytes());
        b[8..12].copy_from_slice(&0x100u32.to_le_bytes());
        assert_eq!(Mensaje::leer(&b), None);
        b[..4].copy_from_slice(&APARATO.to_le_bytes());
        b[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(Mensaje::leer(&b), None);
        // Al azar: nunca un pánico.
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut buf = [0u8; MAX_MENSAJE + 8];
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            for (i, v) in buf.iter_mut().enumerate() {
                *v = (x >> ((i % 8) * 8)) as u8 ^ i as u8;
            }
            let tipo = [HOLA, INFO, FUNCIONES, MEMORIA, COLA, ESTADO, AVISO, USADOS, APARATO, INFO | RESPUESTA, COLA | RESPUESTA][(x % 11) as usize];
            buf[..4].copy_from_slice(&tipo.to_le_bytes());
            let n = (x >> 20) as usize % buf.len();
            let _ = Mensaje::leer(&buf[..n]);
        }
    }

    #[test]
    fn colas() {
        let base = 0xa000_0000;
        let c = Cola { indice: 0, tam: 128, desc: base, disp: base + 0x800, usados: base + 0x1000 };
        assert!(cola_valida(&c, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { tam: 512, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { tam: 100, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { tam: 0, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { indice: 2, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { desc: base + 8, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { usados: base + 0x1002, ..c }, base, 0x6_4000, 256));
        // Fuera de la ventana, por delante, por detrás y dando la vuelta.
        assert!(!cola_valida(&Cola { desc: base - 0x1000, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { usados: base + 0x6_4000 - 8, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&Cola { disp: u64::MAX - 1, ..c }, base, 0x6_4000, 256));
        assert!(!cola_valida(&c, base, 0, 256));
    }
}
