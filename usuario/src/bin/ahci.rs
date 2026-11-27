//! El controlador de discos SATA por AHCI (X5c), fuera del kernel. Escrito con la especificación Serial ATA AHCI 1.3.1
//! de Intel (registros del HBA, 3.1; de cada puerto, 3.3; lista de órdenes y tabla de órdenes, 4.2; arranque, 10.1;
//! recuperación de errores, 6.2.2) y el juego de órdenes ATA (ATA8-ACS: IDENTIFY DEVICE, READ y WRITE DMA EXT, FLUSH
//! CACHE EXT, DATA SET MANAGEMENT con TRIM).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 5, ABAR); la 3, su interrupción (MSI) si el kernel la
//! sabe; la siguiente, el canal donde sirve los discos (el extremo de lectura).
//!
//! Reinicia el HBA, prepara cada puerto con un disco ATA (varios puertos: cada uno es un disco del servicio, por orden
//! de puerto), les pide sus datos (IDENTIFY) y lee sus dos primeros sectores. Después sirve los discos por el canal con
//! el protocolo del NVMe (`dashos::disco_servidor`). Órdenes sin cola (no NCQ), hasta 8 en ranuras distintas: el HBA
//! las hace de una en una. Cada orden lleva una sola entrada de PRD (la ventana del cliente es seguida en el bus). Las
//! espera girando 50 µs y después durmiendo hasta su interrupción; sin interrupción, durmiendo de milisegundo en
//! milisegundo.

#![no_std]
#![no_main]

use dashos::disco_servidor::{self as ds, Orden};
use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;
const VA_DMA: usize = 0x30_0000_0000;

// Registros del HBA (3.1).
const CAP: usize = 0x00;
const GHC: usize = 0x04;
const IS: usize = 0x08;
const PI: usize = 0x0C;
const VS: usize = 0x10;
const CAP2: usize = 0x24;
const BOHC: usize = 0x28;
const GHC_HR: u32 = 1 << 0;
const GHC_IE: u32 = 1 << 1;
const GHC_AE: u32 = 1 << 31;
const CAP_SCLO: u32 = 1 << 24;
const CAP_SSS: u32 = 1 << 27;
const CAP_SAM: u32 = 1 << 18;

// Registros de cada puerto (3.3), desde 0x100 + 0x80 por puerto.
const P_CLB: usize = 0x00;
const P_FB: usize = 0x08;
const P_IS: usize = 0x10;
const P_IE: usize = 0x14;
const P_CMD: usize = 0x18;
const P_TFD: usize = 0x20;
const P_SIG: usize = 0x24;
const P_SSTS: usize = 0x28;
const P_SCTL: usize = 0x2C;
const P_SERR: usize = 0x30;
const P_CI: usize = 0x38;
const CMD_ST: u32 = 1 << 0;
const CMD_SUD: u32 = 1 << 1;
const CMD_CLO: u32 = 1 << 3;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const TFD_BSY: u32 = 1 << 7;
const TFD_DRQ: u32 = 1 << 3;
/// Los avisos que se piden (3.3.6): registro de vuelta, PIO, DMA, bits del dispositivo, PRD hecho, y los errores
/// (archivo de tareas, bus del host, datos del host, interfaz, desbordamiento).
const IE_TODOS: u32 = 0x0000_002F | 0x7900_0000;
/// Los avisos de error (3.3.5): TFES, HBFS, HBDS, IFS, OFS.
const IS_ERRORES: u32 = 0x7900_0000;
/// La firma de un disco ATA (no ATAPI ni multiplicador de puertos).
const SIG_ATA: u32 = 0x0000_0101;

// La memoria DMA de cada puerto (16 KB): la lista de órdenes (1 KB, alineada a 1 KB), lo que recibe (256 B), una
// página para IDENTIFY y los sectores del arranque, las listas de tramos de los descartes (512 B por sitio) y las tablas
// de órdenes (256 B cada una, alineadas a 128).
const POR_PUERTO: usize = 16 * 1024;
const M_LISTA: usize = 0x0000;
const M_FIS: usize = 0x0400;
const M_DATOS: usize = 0x1000;
const M_DSM: usize = 0x2000;
const M_TABLAS: usize = 0x3000;
const TAM_TABLA: usize = 256;
/// Ranuras que se usan como mucho (caben 16 tablas en los 4 KB del final).
const RANURAS: u32 = 16;
const MAX_PUERTOS: usize = 32;

// Órdenes ATA.
const ATA_IDENTIFICAR: u8 = 0xEC;
const ATA_LEER: u8 = 0x25;
const ATA_ESCRIBIR: u8 = 0x35;
const ATA_VACIAR: u8 = 0xEA;
const ATA_VACIAR28: u8 = 0xE7;
const ATA_DSM: u8 = 0x06;

fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile((VA + off) as *const u32) })
}

fn escribir(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA + off) as *mut u32, v.to_le()) }
}

fn dma8(off: usize) -> u8 {
    unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u8) }
}

fn dma16(off: usize) -> u16 {
    u16::from_le(unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u16) })
}

fn poner8(off: usize, v: u8) {
    unsafe { core::ptr::write_volatile((VA_DMA + off) as *mut u8, v) }
}

fn poner32(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA_DMA + off) as *mut u32, v.to_le()) }
}

fn esperar(ms: u64, cond: impl Fn() -> bool) -> bool {
    let fin = dashos::ahora_us() + ms * 1000;
    while dashos::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}

/// Un puerto con un disco.
struct Puerto {
    n: usize,
    /// Dónde está su memoria DMA (desplazamiento dentro de la nuestra) y su dirección de bus.
    m: usize,
    bus: u64,
    sectores: u64,
    tam_sector: usize,
    trim: bool,
    vaciar_ext: bool,
    /// Las ranuras mandadas y no devueltas; las que fallaron (esperan a devolverse con error).
    en_vuelo: u32,
    fallidas: u32,
    ids: [u16; RANURAS as usize],
    siguiente_id: u16,
}

impl Puerto {
    fn reg(&self, off: usize) -> usize {
        0x100 + 0x80 * self.n + off
    }
}

struct Ahci {
    puertos: [Option<Puerto>; MAX_PUERTOS],
    /// Los discos, por orden: el número de su puerto en `puertos`.
    discos: [usize; ds::MAX_DISCOS],
    cuantos: usize,
    ranuras: u32,
    cap: u32,
    /// El canal de avisos (lectura y escritura) y la llave de la interrupción, si la hay.
    avisos: Option<(u32, u32, u32)>,
}

impl Ahci {
    fn puerto(&mut self, d: usize) -> &mut Puerto {
        self.puertos[self.discos[d]].as_mut().unwrap()
    }

    /// Pone una orden en una ranura libre del disco `d` y la manda: su identificador. `datos` es (bus, bytes, el
    /// HBA escribe en memoria); FIS H2D de 20 bytes (Serial ATA, 10.3.4).
    fn mandar(&mut self, d: usize, orden: u8, rasgos: u16, lba: u64, cuenta: u16, datos: Option<(u64, u32, bool)>) -> Option<u16> {
        let ranuras = self.ranuras;
        let p = self.puerto(d);
        let ocupadas = p.en_vuelo | p.fallidas | leer(p.reg(P_CI));
        let r = (0..ranuras).find(|r| ocupadas & (1 << r) == 0)? as usize;
        let tabla = p.m + M_TABLAS + r * TAM_TABLA;
        for i in (0..TAM_TABLA).step_by(4) {
            poner32(tabla + i, 0);
        }
        let fis = [
            0x27,
            0x80,
            orden,
            rasgos as u8,
            lba as u8,
            (lba >> 8) as u8,
            (lba >> 16) as u8,
            if matches!(orden, ATA_LEER | ATA_ESCRIBIR) { 0x40 } else { 0 }, // LBA
            (lba >> 24) as u8,
            (lba >> 32) as u8,
            (lba >> 40) as u8,
            (rasgos >> 8) as u8,
            cuenta as u8,
            (cuenta >> 8) as u8,
        ];
        for (i, b) in fis.iter().enumerate() {
            poner8(tabla + i, *b);
        }
        let mut dw0 = 5u32; // CFL: 5 palabras
        if let Some((bus, bytes, escribe)) = datos {
            // Una entrada de PRD (4.2.3.3): dirección, y en DW3 los bytes menos uno (pares).
            poner32(tabla + 0x80, bus as u32);
            poner32(tabla + 0x84, (bus >> 32) as u32);
            poner32(tabla + 0x8C, (bytes - 1) | 1);
            dw0 |= 1 << 16;
            if !escribe {
                dw0 |= 1 << 6; // W: del host al disco
            }
        }
        let cab = p.m + M_LISTA + 32 * r;
        let tabla_bus = p.bus + (M_TABLAS + r * TAM_TABLA) as u64;
        poner32(cab, dw0);
        poner32(cab + 4, 0);
        poner32(cab + 8, tabla_bus as u32);
        poner32(cab + 12, (tabla_bus >> 32) as u32);
        p.siguiente_id = p.siguiente_id.wrapping_add(1);
        p.ids[r] = p.siguiente_id;
        p.en_vuelo |= 1 << r;
        dashos::barrera::total();
        escribir(p.reg(P_CI), 1 << r);
        Some(p.siguiente_id)
    }

    /// Mira el disco `d`: baja sus avisos y devuelve una ranura terminada (identificador, bien), si hay. Si el disco
    /// dio un error, recupera el puerto y todo lo que estaba en vuelo vuelve con error.
    fn mirar(&mut self, d: usize) -> Option<(u16, bool)> {
        let cap = self.cap;
        let p = self.puerto(d);
        let is = leer(p.reg(P_IS));
        if is != 0 {
            escribir(p.reg(P_IS), is);
            escribir(IS, 1 << p.n);
        }
        let ci = leer(p.reg(P_CI));
        // PxIS se pone aunque su aviso no esté pedido (PxIE solo decide la interrupción).
        if is & IS_ERRORES != 0 {
            println!("  [ahci] puerto {}: error (aviso {:#x}, estado {:#x}); se recupera", p.n, is, leer(p.reg(P_TFD)));
            p.fallidas |= p.en_vuelo & ci;
            recuperar(p, cap);
        }
        let hechas = (p.en_vuelo & !ci & !p.fallidas) | p.fallidas;
        if hechas == 0 {
            return None;
        }
        let r = hechas.trailing_zeros();
        let bien = p.fallidas & (1 << r) == 0;
        p.en_vuelo &= !(1 << r);
        p.fallidas &= !(1 << r);
        Some((p.ids[r as usize], bien))
    }
}

/// Recuperación tras un error (6.2.2.1): parar el puerto, borrar los errores, soltar el disco si sigue ocupado y
/// volver a encenderlo. Lo que estaba en vuelo se pierde (CI vuelve a 0).
fn recuperar(p: &Puerto, cap: u32) {
    escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) & !CMD_ST);
    esperar(500, || leer(p.reg(P_CMD)) & CMD_CR == 0);
    escribir(p.reg(P_SERR), u32::MAX);
    escribir(p.reg(P_IS), u32::MAX);
    if leer(p.reg(P_TFD)) & (TFD_BSY | TFD_DRQ) != 0 {
        if cap & CAP_SCLO != 0 {
            escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) | CMD_CLO);
            esperar(500, || leer(p.reg(P_CMD)) & CMD_CLO == 0);
        } else {
            reiniciar_enlace(p);
        }
    }
    escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) | CMD_ST);
}

/// COMRESET (3.3.11): DET a 1 al menos 1 ms y otra vez a 0; espera a que haya enlace (DET = 3).
fn reiniciar_enlace(p: &Puerto) -> bool {
    let sctl = leer(p.reg(P_SCTL)) & !0xf;
    escribir(p.reg(P_SCTL), sctl | 1);
    let fin = dashos::ahora_us() + 2000;
    while dashos::ahora_us() < fin {}
    escribir(p.reg(P_SCTL), sctl);
    let bien = esperar(1000, || leer(p.reg(P_SSTS)) & 0xf == 3);
    escribir(p.reg(P_SERR), u32::MAX);
    bien
}

impl ds::Controlador for Ahci {
    fn discos(&self) -> usize {
        self.cuantos
    }

    fn sectores(&self, d: usize) -> u64 {
        self.puertos[self.discos[d]].as_ref().unwrap().sectores
    }

    fn tam_sector(&self, d: usize) -> usize {
        self.puertos[self.discos[d]].as_ref().unwrap().tam_sector
    }

    fn plazo_ms(&self) -> u64 {
        // Un disco de verdad puede tener que arrancar el motor.
        10_000
    }

    fn descarta(&self, d: usize) -> bool {
        self.puertos[self.discos[d]].as_ref().unwrap().trim
    }

    fn max_bytes(&self, d: usize) -> usize {
        // La cuenta de READ DMA EXT es de 16 bits, y una entrada de PRD llega a 4 MB.
        (0xFFFF * self.tam_sector(d)).min(4 << 20)
    }

    fn enviar(&mut self, d: usize, o: Orden, sitio: usize) -> Option<u16> {
        let tam = self.tam_sector(d);
        match o {
            Orden::Leer { primero, cuantos, bus } => self.mandar(d, ATA_LEER, 0, primero, cuantos as u16, Some((bus, cuantos * tam as u32, true))),
            Orden::Escribir { primero, cuantos, bus } => self.mandar(d, ATA_ESCRIBIR, 0, primero, cuantos as u16, Some((bus, cuantos * tam as u32, false))),
            Orden::Vaciar => {
                let orden = if self.puerto(d).vaciar_ext { ATA_VACIAR } else { ATA_VACIAR28 };
                self.mandar(d, orden, 0, 0, 0, None)
            }
            Orden::Descartar { primero, cuantos } => {
                // Un bloque de 512 bytes con hasta 64 tramos de 8 (ACS: LBA en 47:0, cuántos en 63:48). Lo que no
                // quepa no se descarta: es un aviso.
                let p = self.puerto(d);
                let m = p.m + M_DSM + 512 * sitio;
                let bus = p.bus + (M_DSM + 512 * sitio) as u64;
                let (mut lba, mut quedan) = (primero, cuantos);
                for k in 0..64 {
                    let n = quedan.min(0xFFFF);
                    let e = if n == 0 { 0 } else { lba | n << 48 };
                    poner32(m + 8 * k, e as u32);
                    poner32(m + 8 * k + 4, (e >> 32) as u32);
                    lba += n;
                    quedan -= n;
                }
                self.mandar(d, ATA_DSM, 1, 0, 1, Some((bus, 512, false)))
            }
        }
    }

    fn terminada(&mut self, d: usize, ms: u64) -> Option<(u16, bool)> {
        if let Some(t) = self.mirar(d) {
            return Some(t);
        }
        if ms == 0 {
            return None;
        }
        let ahora = dashos::ahora_us();
        let fin = ahora + ms * 1000;
        // Unos 50 µs girando: en QEMU casi todo acaba antes.
        while dashos::ahora_us() < (ahora + 50).min(fin) {
            if let Some(t) = self.mirar(d) {
                return Some(t);
            }
        }
        let temporizador = self.avisos.map(|(_, escritura, _)| dashos::temporizador_crear(escritura, ms * 1000, 0));
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        let r = loop {
            if let Some(t) = self.mirar(d) {
                break Some(t);
            }
            if dashos::ahora_us() >= fin {
                break None;
            }
            match self.avisos {
                Some((lectura, _, irq)) => {
                    if dashos::canal_leer(lectura, &mut buf, &mut ll).0 == 4 {
                        dashos::interrupcion_terminar(irq);
                    }
                }
                None => dashos::dormir_us(1000),
            }
        };
        if let Some(t) = temporizador.filter(|&t| t > 0) {
            dashos::cerrar(t as u32);
        }
        r
    }

    fn leer_sector(&mut self, d: usize, s: u64, b: &mut [u8]) -> bool {
        let (m, bus) = {
            let p = self.puerto(d);
            (p.m + M_DATOS, p.bus + M_DATOS as u64)
        };
        let Some(id) = self.enviar(d, Orden::Leer { primero: s, cuantos: 1, bus }, 0) else { return false };
        match self.terminada(d, self.plazo_ms()) {
            Some((t, true)) if t == id => {
                for (i, x) in b.iter_mut().enumerate() {
                    *x = dma8(m + i);
                }
                true
            }
            _ => false,
        }
    }
}

/// El texto ATA de `palabras` palabras desde `off` (cada palabra con sus dos bytes al revés), sin los espacios del final.
fn texto_ata(off: usize, palabras: usize, b: &mut [u8; 64]) -> &str {
    let mut l = 0;
    for i in 0..(2 * palabras).min(64) {
        let c = dma8(off + (i ^ 1));
        b[i] = if (0x20..0x7f).contains(&c) { c } else { b'?' };
        if c != b' ' && c != 0 {
            l = i + 1;
        }
    }
    core::str::from_utf8(&b[..l]).unwrap_or("?")
}

/// Prepara el puerto `n` (10.1.2): parado, con su memoria, recibiendo FIS, con enlace y el disco libre. Su firma, o
/// nada si no hay disco.
fn preparar_puerto(p: &Puerto, cap: u32) -> Option<u32> {
    // Parar lo que dejara en marcha el firmware.
    escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) & !(CMD_ST | CMD_FRE));
    if !esperar(500, || leer(p.reg(P_CMD)) & (CMD_CR | CMD_FR) == 0) {
        println!("  [ahci] puerto {}: no se para", p.n);
        return None;
    }
    escribir(p.reg(P_CLB), (p.bus + M_LISTA as u64) as u32);
    escribir(p.reg(P_CLB + 4), ((p.bus + M_LISTA as u64) >> 32) as u32);
    escribir(p.reg(P_FB), (p.bus + M_FIS as u64) as u32);
    escribir(p.reg(P_FB + 4), ((p.bus + M_FIS as u64) >> 32) as u32);
    escribir(p.reg(P_SERR), u32::MAX);
    escribir(p.reg(P_IS), u32::MAX);
    if cap & CAP_SSS != 0 {
        escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) | CMD_SUD);
    }
    escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) | CMD_FRE);
    // El enlace: DET = 3 (dispositivo y comunicación). Sin nada (DET = 0), el puerto está vacío; a medias, un COMRESET.
    let det = || leer(p.reg(P_SSTS)) & 0xf;
    esperar(20, || det() == 3);
    if det() == 0 {
        return None;
    }
    if det() != 3 && !reiniciar_enlace(p) {
        println!("  [ahci] puerto {}: sin enlace (DET {})", p.n, det());
        return None;
    }
    escribir(p.reg(P_SERR), u32::MAX);
    if !esperar(5000, || leer(p.reg(P_TFD)) & (TFD_BSY | TFD_DRQ) == 0) {
        println!("  [ahci] puerto {}: el disco sigue ocupado", p.n);
        return None;
    }
    Some(leer(p.reg(P_SIG)))
}

fn principal() -> u64 {
    // Lo primero, antes de crear llaves: si hay llave 4, la 3 es la interrupción y la 4 el canal; si no, el canal es
    // la 3.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, canal) = if existe(4) { (Some(3), 4) } else { (None, 3) };
    if dashos::registros_proyectar(2, VA) != 0 {
        println!("  [ahci] sin registros");
        return 1;
    }
    let avisos = irq.and_then(|i| {
        let (a, b) = dashos::canal_crear();
        (dashos::interrupcion_asociar(i, b) == 0).then_some((a, b, i))
    });
    let (cap, vs) = (leer(CAP), leer(VS));
    // El traspaso del firmware al sistema (10.6), si el HBA lo tiene (CAP2.BOH).
    if leer(CAP2) & 1 != 0 {
        escribir(BOHC, leer(BOHC) | (1 << 1));
        esperar(25, || leer(BOHC) & 1 == 0);
        if leer(BOHC) & (1 << 4) != 0 {
            esperar(2000, || leer(BOHC) & 1 == 0);
        }
    }
    // Modo AHCI y reinicio del HBA (10.4.3): para todos los puertos y deja los registros como al encender.
    escribir(GHC, leer(GHC) | GHC_AE);
    escribir(GHC, leer(GHC) | GHC_HR);
    if !esperar(1000, || leer(GHC) & GHC_HR == 0) {
        println!("  [ahci] el HBA no sale del reinicio");
        return 2;
    }
    if cap & CAP_SAM == 0 {
        escribir(GHC, leer(GHC) | GHC_AE);
    }
    escribir(IS, u32::MAX);
    if avisos.is_some() {
        escribir(GHC, leer(GHC) | GHC_IE);
    }
    let pi = leer(PI);
    let ranuras = ((cap >> 8) & 0x1f) + 1;
    println!("  [ahci] versión {}.{}, {} puertos ({:#x}), {} ranuras, 64 bits: {}", vs >> 16, (vs >> 8) & 0xff, (cap & 0x1f) + 1, pi, ranuras, if cap >> 31 != 0 { "sí" } else { "no" });
    dashos::dma_liberar(1);
    let mut a = Ahci { puertos: [const { None }; MAX_PUERTOS], discos: [0; ds::MAX_DISCOS], cuantos: 0, ranuras: ranuras.min(RANURAS), cap, avisos };
    let mut b = [0u8; 64];
    // Los trozos de memoria DMA ya proyectados (uno por puerto que se probó; los de los que no valen se sueltan o se
    // quedan, pero su sitio no se reusa).
    let mut usados = 0;
    for n in 0..MAX_PUERTOS {
        if pi & (1 << n) == 0 || a.cuantos == ds::MAX_DISCOS {
            continue;
        }
        let (mem, bus) = dashos::memoria_dma(1, POR_PUERTO);
        let m = usados * POR_PUERTO;
        usados += 1;
        if mem <= 0 || dashos::memoria_proyectar(mem as u32, VA_DMA + m, true) != 0 {
            println!("  [ahci] sin memoria DMA");
            return 3;
        }
        if cap >> 31 == 0 && bus + POR_PUERTO as u64 > 1 << 32 {
            println!("  [ahci] memoria DMA por encima de 4 GB y el HBA solo llega a 32 bits");
            return 4;
        }
        for i in (0..POR_PUERTO).step_by(4) {
            poner32(m + i, 0);
        }
        let p = Puerto { n, m, bus, sectores: 0, tam_sector: 512, trim: false, vaciar_ext: false, en_vuelo: 0, fallidas: 0, ids: [0; RANURAS as usize], siguiente_id: 0 };
        let Some(firma) = preparar_puerto(&p, cap) else {
            dashos::memoria_quitar(VA_DMA + m);
            dashos::cerrar(mem as u32);
            continue;
        };
        if firma != SIG_ATA {
            println!("  [ahci] puerto {}: firma {:#x}, no es un disco ATA; no se usa", n, firma);
            escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) & !CMD_FRE);
            dashos::memoria_quitar(VA_DMA + m);
            dashos::cerrar(mem as u32);
            continue;
        }
        escribir(p.reg(P_IE), if avisos.is_some() { IE_TODOS } else { 0 });
        escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) | CMD_ST);
        let d = a.cuantos;
        a.puertos[n] = Some(p);
        a.discos[d] = n;
        a.cuantos += 1;
        // IDENTIFY DEVICE (PIO, 512 bytes) en la página de datos.
        let datos = bus + M_DATOS as u64;
        let id = a.mandar(d, ATA_IDENTIFICAR, 0, 0, 0, Some((datos, 512, true)));
        let bien = id.is_some_and(|id| ds::Controlador::terminada(&mut a, d, 5000) == Some((id, true)));
        let w = |i: usize| dma16(m + M_DATOS + 2 * i) as u32;
        // Palabra 83: LBA de 48 bits (bit 10) y FLUSH CACHE EXT (bit 13). 100 a 103: los sectores. 106 y 117-118: el
        // tamaño del sector lógico. 169, bit 0: TRIM.
        if !bien || w(83) & (1 << 10) == 0 {
            println!("  [ahci] puerto {}: {}; no se usa", n, if bien { "sin LBA de 48 bits" } else { "IDENTIFY falla" });
            a.cuantos -= 1;
            if let Some(p) = a.puertos[n].take() {
                escribir(p.reg(P_CMD), leer(p.reg(P_CMD)) & !(CMD_ST | CMD_FRE));
            }
            continue;
        }
        let sectores = (w(100) | w(101) << 16) as u64 | ((w(102) | w(103) << 16) as u64) << 32;
        let tam = if w(106) & 0xC000 == 0x4000 && w(106) & (1 << 12) != 0 { 2 * (w(117) | w(118) << 16) as usize } else { 512 };
        let p = a.puertos[n].as_mut().unwrap();
        p.sectores = sectores;
        p.tam_sector = tam;
        p.trim = w(169) & 1 != 0;
        p.vaciar_ext = w(83) & (1 << 13) != 0;
        let trim = p.trim;
        println!("  [ahci] puerto {}: disco {}, \"{}\", {} sectores de {} bytes ({} KB); TRIM: {}", n, d, texto_ata(m + M_DATOS + 54, 20, &mut b), sectores, tam, sectores * tam as u64 / 1024, if trim { "sí" } else { "no" });
        if !(512..=4096).contains(&tam) || !tam.is_power_of_two() {
            println!("  [ahci] puerto {}: sectores de {} bytes: no se usa", n, tam);
            a.cuantos -= 1;
            a.puertos[n] = None;
            continue;
        }
        // Los dos primeros sectores.
        let mut s = [0u8; 4096];
        for k in 0..2u64 {
            if !ds::Controlador::leer_sector(&mut a, d, k, &mut s[..tam]) {
                println!("  [ahci] puerto {}: la lectura falla", n);
                break;
            }
            let l = (0..64).find(|&i| s[i] == 0).unwrap_or(64);
            let t: &[u8] = &s[..l];
            let t = core::str::from_utf8(t).unwrap_or("?");
            println!("  [ahci] disco {}, sector {}: \"{}\", firma {:#06x}", d, k, t, s[510] as u16 | (s[511] as u16) << 8);
        }
    }
    if a.cuantos == 0 {
        println!("  [ahci] ningún disco");
    }
    println!("  [ahci] {} discos; avisos {}", a.cuantos, if a.avisos.is_some() { "por su interrupción" } else { "durmiendo" });
    ds::servir(&mut a, 1, canal, "ahci")
}
