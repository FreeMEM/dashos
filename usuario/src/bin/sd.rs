//! C21: controlador de tarjeta SD (SDHCI), fuera del kernel. Lee los dos primeros sectores sin interrupciones ni
//! DMA, por consulta.
//!
//! Sigue la "SD Host Controller Simplified Specification" y la "Physical Layer Simplified Specification" públicas
//! de la SD Association. El kernel le da hasta dos llaves de registros: la 1, el EMMC2 (donde va la tarjeta en la
//! Pi 4); la 2, el SDHCI clásico (donde la pone QEMU). Usa el primero que tenga tarjeta.
//!
//! Los registros son little-endian y aquí se tocan siempre de 32 bits: el SDHCI de la BCM2835 no admite accesos
//! más estrechos.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const BASES: [usize; 2] = [0x0090_0000, 0x0091_0000];

// Registros (desplazamientos de la especificación).
const BLOQUE: usize = 0x04; // tamaño de bloque (15:0) y número de bloques (31:16)
const ARGUMENTO: usize = 0x08;
const MODO_ORDEN: usize = 0x0C; // modo de transferencia (15:0) y orden (31:16)
const RESPUESTA: usize = 0x10;
const DATOS: usize = 0x20;
const ESTADO: usize = 0x24;
const CONTROL: usize = 0x28; // control 1, potencia (15:8)...
const RELOJ: usize = 0x2C; // reloj (15:0), tiempo de espera (19:16), reinicio (26:24)
const AVISOS: usize = 0x30; // avisos normales (15:0) y de error (31:16)
const AVISOS_ACTIVOS: usize = 0x34;
const SENALES: usize = 0x38;
const CAPACIDADES: usize = 0x40;

const ESTADO_ORDEN_OCUPADA: u32 = 1 << 0;
const ESTADO_DATOS_OCUPADOS: u32 = 1 << 1;
const ESTADO_TARJETA: u32 = 1 << 16;
const AVISO_ORDEN_HECHA: u32 = 1 << 0;
const AVISO_TRANSFERENCIA_HECHA: u32 = 1 << 1;
const AVISO_LEER_LISTO: u32 = 1 << 5;
const AVISO_ERROR: u32 = 1 << 15;

// Tipos de respuesta del registro de orden.
const SIN: u32 = 0;
const R136: u32 = 1 | 1 << 3; // R2: 136 bits, con CRC
const R48: u32 = 2 | 1 << 3 | 1 << 4; // R1, R6, R7: CRC e índice
const R48_SIN_CRC: u32 = 2; // R3
const R48_OCUPADA: u32 = 3 | 1 << 3 | 1 << 4; // R1b
const CON_DATOS: u32 = 1 << 5;

struct Sd {
    base: usize,
}

#[derive(Debug)]
#[allow(dead_code)] // los campos solo se leen al escribir el error
enum Error {
    Tiempo(u32),
    Orden(u32, u32),
}

impl Sd {
    fn leer(&self, off: usize) -> u32 {
        u32::from_le(unsafe { ((self.base + off) as *const u32).read_volatile() })
    }
    fn escribir(&self, off: usize, v: u32) {
        unsafe { ((self.base + off) as *mut u32).write_volatile(v.to_le()) }
    }

    fn esperar(&self, que: impl Fn(&Self) -> bool, codigo: u32) -> Result<(), Error> {
        for _ in 0..1_000_000 {
            if que(self) {
                return Ok(());
            }
        }
        Err(Error::Tiempo(codigo))
    }

    /// Manda una orden y espera a que termine. Devuelve la primera palabra de la respuesta.
    fn orden(&self, indice: u32, argumento: u32, tipo: u32, modo: u32) -> Result<u32, Error> {
        let ocupado = ESTADO_ORDEN_OCUPADA | if tipo & CON_DATOS != 0 || tipo == R48_OCUPADA { ESTADO_DATOS_OCUPADOS } else { 0 };
        self.esperar(|s| s.leer(ESTADO) & ocupado == 0, 100 + indice)?;
        self.escribir(AVISOS, 0xffff_ffff);
        self.escribir(ARGUMENTO, argumento);
        self.escribir(MODO_ORDEN, (indice << 24) | (tipo << 16) | modo);
        self.esperar(|s| s.leer(AVISOS) & (AVISO_ORDEN_HECHA | AVISO_ERROR) != 0, 200 + indice)?;
        let avisos = self.leer(AVISOS);
        if avisos & AVISO_ERROR != 0 {
            self.escribir(AVISOS, 0xffff_ffff);
            self.reiniciar(1 << 25); // reiniciar la línea de órdenes
            return Err(Error::Orden(indice, avisos));
        }
        self.escribir(AVISOS, AVISO_ORDEN_HECHA);
        Ok(self.leer(RESPUESTA))
    }

    fn reiniciar(&self, bit: u32) {
        self.escribir(RELOJ, self.leer(RELOJ) | (bit << 0));
        let _ = self.esperar(|s| s.leer(RELOJ) & bit == 0, 1);
    }

    /// Divisor de reloj para no pasar de `hz`, en el formato de 10 bits de la versión 3.
    fn poner_reloj(&self, hz: u32) -> Result<(), Error> {
        let base_mhz = (self.leer(CAPACIDADES) >> 8) & 0xff;
        let base = if base_mhz == 0 { 100_000_000 } else { base_mhz * 1_000_000 };
        let mut div = 1;
        while base / (2 * div) > hz && div < 0x3ff {
            div += 1;
        }
        let r = self.leer(RELOJ) & !0xffff;
        self.escribir(RELOJ, r | ((div & 0xff) << 8) | ((div >> 8) << 6) | 1 | (0xe << 16));
        self.esperar(|s| s.leer(RELOJ) & 2 != 0, 2)?;
        self.escribir(RELOJ, self.leer(RELOJ) | 4);
        Ok(())
    }

    /// Lee un sector de 512 bytes. `direccion` va en bloques (SDHC) o en bytes (SDSC), según la tarjeta.
    fn leer_sector(&self, direccion: u32, sector: &mut [u8; 512]) -> Result<(), Error> {
        self.escribir(BLOQUE, (1 << 16) | 512);
        self.orden(17, direccion, R48 | CON_DATOS, 1 << 4)?; // READ_SINGLE_BLOCK; modo: leer
        self.esperar(|s| s.leer(AVISOS) & (AVISO_LEER_LISTO | AVISO_ERROR) != 0, 300)?;
        if self.leer(AVISOS) & AVISO_ERROR != 0 {
            return Err(Error::Orden(17, self.leer(AVISOS)));
        }
        self.escribir(AVISOS, AVISO_LEER_LISTO);
        for i in 0..128 {
            let w = self.leer(DATOS);
            sector[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        self.esperar(|s| s.leer(AVISOS) & AVISO_TRANSFERENCIA_HECHA != 0, 301)?;
        self.escribir(AVISOS, AVISO_TRANSFERENCIA_HECHA);
        Ok(())
    }

    /// Arranca el controlador y la tarjeta. Devuelve (RCA, si es de alta capacidad).
    fn iniciar(&self) -> Result<(u32, bool), Error> {
        self.reiniciar(1 << 24); // reinicio completo
        self.escribir(CONTROL, (self.leer(CONTROL) & !0xff00) | (0x0f << 8)); // 3,3 V y alimentación
        self.poner_reloj(400_000)?;
        self.escribir(AVISOS_ACTIVOS, 0xffff_ffff);
        self.escribir(SENALES, 0); // sin interrupciones: por consulta

        self.orden(0, 0, SIN, 0)?; // GO_IDLE_STATE
        let v2 = match self.orden(8, 0x1aa, R48, 0) {
            Ok(r) => r & 0xfff == 0x1aa,
            Err(_) => false, // tarjeta de la versión 1
        };
        let mut ocr = 0;
        for _ in 0..1000 {
            self.orden(55, 0, R48, 0)?; // APP_CMD
            ocr = self.orden(41, if v2 { 0x40ff_8000 } else { 0x00ff_8000 }, R48_SIN_CRC, 0)?; // SD_SEND_OP_COND
            if ocr & (1 << 31) != 0 {
                break;
            }
        }
        if ocr & (1 << 31) == 0 {
            return Err(Error::Tiempo(41));
        }
        self.orden(2, 0, R136, 0)?; // ALL_SEND_CID
        let rca = self.orden(3, 0, R48, 0)? >> 16; // SEND_RELATIVE_ADDR
        self.orden(7, rca << 16, R48_OCUPADA, 0)?; // SELECT_CARD
        self.poner_reloj(25_000_000)?;
        let alta = ocr & (1 << 30) != 0;
        if !alta {
            self.orden(16, 512, R48, 0)?; // SET_BLOCKLEN
        }
        Ok((rca, alta))
    }
}

fn texto(b: &[u8]) -> &str {
    let fin = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    core::str::from_utf8(&b[..fin]).unwrap_or("?")
}

fn principal() -> u64 {
    let mut sd = None;
    for (i, &base) in BASES.iter().enumerate() {
        if dashos::registros_proyectar(i as u32 + 1, base) != 0 {
            continue;
        }
        let s = Sd { base };
        if s.leer(ESTADO) & ESTADO_TARJETA != 0 {
            println!("  [sd] tarjeta presente en el controlador {} (versión {})", i + 1, (s.leer(0xfc) >> 16) & 0xff);
            sd = Some(s);
            break;
        }
    }
    let Some(sd) = sd else {
        println!("  [sd] no hay tarjeta");
        return 1;
    };
    let (rca, alta) = match sd.iniciar() {
        Ok(x) => x,
        Err(e) => {
            println!("  [sd] no arranca: {:?}", e);
            return 2;
        }
    };
    println!("  [sd] tarjeta lista: RCA {:#x}, {}", rca, if alta { "alta capacidad" } else { "capacidad estándar" });
    let mut sector = [0u8; 512];
    for n in 0..2u32 {
        if let Err(e) = sd.leer_sector(if alta { n } else { n * 512 }, &mut sector) {
            println!("  [sd] error al leer el sector {}: {:?}", n, e);
            return 3;
        }
        if n == 0 {
            println!("  [sd] sector 0: \"{}\", firma {:#04x}{:02x}", texto(&sector[..64]), sector[510], sector[511]);
            if sector[510] != 0x55 || sector[511] != 0xaa {
                return 4;
            }
        } else {
            println!("  [sd] sector 1: \"{}\"", texto(&sector[..64]));
        }
    }
    9
}
