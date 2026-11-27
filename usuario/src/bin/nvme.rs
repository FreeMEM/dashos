//! El controlador de discos NVMe (U6d), fuera del kernel. Escrito con la especificación NVM Express Base 2.0 (registros
//! del controlador, cap. 3.1; colas, cap. 4; órdenes de administración, cap. 5) y la del juego de órdenes NVM (Read).
//!
//! Llaves: la 1, su permiso DMA; la 2, sus registros (la BAR 0); la 3, su interrupción (MSI-X o INTx) si el kernel
//! la sabe; la siguiente, el canal "disco" (el extremo de lectura).
//!
//! Reinicia el controlador, monta las colas de administración, pide sus datos (Identify) y los del espacio de nombres
//! 1, crea una cola de E/S y lee los dos primeros sectores (U6d). Después sirve el disco por el canal (U6f; el
//! protocolo, en `docs/referencia/servicios.md`): cada cliente, por su marca, tiene una ventana de memoria DMA que
//! comparten los dos, y el disco lee y escribe en ella directamente, sin copias. Cada orden espera a su terminada
//! girando un poco (en QEMU acaban en microsegundos) y, si tarda, durmiendo hasta su interrupción (U6i); sin
//! interrupción, girando. Toda su memoria es memoria DMA del permiso, sin caché; su
//! dirección de bus es la física. Con páginas de 4 KB (CC.MPS = 0): una transferencia de hasta dos páginas se
//! describe con PRP1 y PRP2; una mayor, hasta la ventana entera (64 KB), con PRP2 apuntando a una lista de páginas
//! (4.1.1, "PRP List"; paso F de docs/MULTITAREA_LOOP.md: antes, 8 KB por orden como mucho).

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x0090_0000;

// Registros (3.1).
const CAP: usize = 0x00;
const VS: usize = 0x08;
const CC: usize = 0x14;
const CSTS: usize = 0x1C;
const AQA: usize = 0x24;
const ASQ: usize = 0x28;
const ACQ: usize = 0x30;
const TIMBRES: usize = 0x1000;
const CC_EN: u32 = 1 << 0;
/// Entradas de 64 bytes en las colas de envío (2^6) y de 16 en las de terminadas (2^4).
const CC_TAMANOS: u32 = (6 << 16) | (4 << 20);
const CSTS_RDY: u32 = 1 << 0;
const CSTS_CFS: u32 = 1 << 1;

// Memoria (un objeto DMA de 64 KB): dónde va cada cosa, cada una en su página.
const VA_DMA: usize = 0x30_0000_0000;
const TAM_DMA: usize = 64 * 1024;
const M_ADM_SQ: usize = 0x0000;
const M_ADM_CQ: usize = 0x1000;
const M_ES_SQ: usize = 0x2000;
const M_ES_CQ: usize = 0x3000;
const M_DATOS: usize = 0x4000;
/// Las listas de páginas de las transferencias de más de dos (4.1.1): una por orden en vuelo, de `PASO_LISTA` bytes
/// (la ventana entera, 33 páginas como mucho, pide 32 entradas: 256 bytes). La lista puede empezar dentro de la página.
const M_LISTA: usize = 0xF000;
const PASO_LISTA: usize = 512;
/// A15: órdenes del anillo en vuelo a la vez, como mucho (una lista de páginas y un tramo de descarte para cada una).
const EN_VUELO: usize = 8;
/// Su dirección de bus, cuando se sabe.
static LISTA_BUS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
const ENTRADAS: usize = 16;

// Órdenes (5 y NVM).
const O_CREAR_SQ: u8 = 0x01;
const O_CREAR_CQ: u8 = 0x05;
const O_IDENTIFICAR: u8 = 0x06;
const O_LEER: u8 = 0x02;
const O_ESCRIBIR: u8 = 0x01;
const O_VACIAR: u8 = 0x00;
/// Dataset Management (juego de órdenes NVM): con el atributo AD (CDW11, bit 2), los tramos que se le dan quedan
/// sin asignar (descarte o TRIM).
const O_DSM: u8 = 0x09;

// El servicio (U6f).
/// 128 KB: dos mitades de 64 KB, una para lo que se lee y otra para lo que se lee por adelantado (A14).
const VENTANA: usize = 128 * 1024;
const MAX_CLIENTES: usize = 5;
const E_CERRADO: i64 = -4;
const PEDIR_ABRIR: u32 = 0;
const PEDIR_LEER: u32 = 1;
const PEDIR_ESCRIBIR: u32 = 2;
const MAL_FORMADO: i32 = -1;
const FUERA_DEL_DISCO: i32 = -2;
const FUERA_DE_LA_VENTANA: i32 = -3;
const ERROR_DEL_DISCO: i32 = -4;
const SIN_SITIO: i32 = -5;
/// Quien no tiene tramo del disco (A13): no se le abre.
const SIN_PERMISO: i32 = -6;
/// Los programas con tramo del disco, por el nombre que les da `inicio` (A13 de docs/REVISION_ARQUITECTURA.md: antes,
/// quien tuviera la llave veía el disco entero). Cada uno ve su tramo como si fuera su disco: su sector 0 es el primero
/// del tramo y no puede ni nombrar lo de fuera (A14). `disco`, la prueba del disco en bruto: el primer MB, sin llegar a
/// la primera partición, y nada si la tabla es GPT (la tabla ocupa ese sitio). El servicio de archivos: la partición de
/// DashOS de la tabla de particiones (`dashos::particiones`); sin ella, nada. El comprobador (A15): la misma partición,
/// solo para leer (escribir y descartar dan -6). La caja Linux (H9): la partición de la caja (GPT, su tipo propio), que
/// es el disco de su invitado. Nadie más.
/// S32: y el instalador (solo en vivo, arrancado del pendrive), con el disco entero: lee las tablas de todos, aunque no
/// valgan, y escribe solo lo que dice su plan.
const NOMBRES: [&str; 5] = ["disco", "archivos", "comprobador", "caja-linux", "instalador"];
/// El de solo lectura, por su número en `NOMBRES`.
const SOLO_LEER: usize = 2;
const PEDIR_ANILLO: u32 = 3;
// La orden 4, "mira mi anillo", va sin llave de respuesta y no se contesta: solo despierta al servicio.
/// Vaciar la caché del disco (FS2): lo escrito antes queda guardado de verdad antes de lo que venga después. El sistema
/// de archivos lo pide antes y después de cada cabecera.
const PEDIR_VACIAR: u32 = 5;
/// A14: descartar (TRIM) `sectores` desde `primer sector`: el sistema de archivos ya no los usa. Con las mismas
/// comprobaciones que leer y escribir, sin ventana. Si el disco no sabe descartar, no hace nada y contesta bien (es
/// un aviso).
const PEDIR_DESCARTAR: u32 = 6;
/// A16, para probar: leer como la orden 1, pero sin esperar la terminada (vence el plazo, como un disco que tropieza).
/// Solo a quien tiene tramo, y solo de lo suyo. Lo siguiente tiene que seguir yendo bien.
const PEDIR_LEER_SIN_ESPERA: u32 = 7;
/// La dirección de bus de la página para la lista de tramos de un descarte (la de los datos de Identify, que después
/// no se usa); 0 si el disco no admite Dataset Management (ONCS, bit 2).
static DSM_BUS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// El anillo de cada cliente (U6l; S1, "anillos en memoria compartida"): una página de memoria compartida. Cabecera de
// palabras de 32 bits: cabeza de envíos (la mueve el servicio), cola de envíos (el cliente), cabeza de terminadas (el
// cliente), cola de terminadas (el servicio), "el servicio duerme" y "el cliente espera". Envíos de 32 bytes desde el
// byte 64 (orden, sectores, primer sector, desplazamiento, etiqueta); terminadas de 16 desde el 2048 (etiqueta,
// estado).
const VA_ANILLOS: usize = 0x31_0000_0000;
const A_SQ_CABEZA: usize = 0;
const A_SQ_COLA: usize = 4;
const A_CQ_CABEZA: usize = 8;
const A_CQ_COLA: usize = 12;
const A_DUERME: usize = 16;
const A_ESPERA: usize = 20;
const A_SQ: usize = 64;
const A_CQ: usize = 2048;
const A_ENTRADAS: u32 = 32;

fn leer(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile((VA + off) as *const u32) })
}

fn escribir(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA + off) as *mut u32, v.to_le()) }
}

fn escribir64(off: usize, v: u64) {
    escribir(off, v as u32);
    escribir(off + 4, (v >> 32) as u32);
}

fn dma8(off: usize) -> u8 {
    unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u8) }
}

fn dma32(off: usize) -> u32 {
    u32::from_le(unsafe { core::ptr::read_volatile((VA_DMA + off) as *const u32) })
}

fn poner32(off: usize, v: u32) {
    unsafe { core::ptr::write_volatile((VA_DMA + off) as *mut u32, v.to_le()) }
}

/// Espera a que se cumpla `cond`, como mucho `ms` milisegundos (girando: solo al arrancar).
fn esperar(ms: u64, cond: impl Fn() -> bool) -> bool {
    let fin = dashos::ahora_us() + ms * 1000;
    while dashos::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}

/// Una pareja de colas (envío y terminadas) con su número.
struct Cola {
    id: usize,
    sq: usize,
    cq: usize,
    cola: usize,
    cabeza: usize,
    fase: u32,
    cid: u16,
    /// Hasta dónde ha tomado el controlador las órdenes (lo dice cada terminada): la cola no puede alcanzarla.
    sq_cabeza: usize,
}

struct Nvme {
    /// El canal de avisos (lectura y escritura) y la llave de la interrupción, si la hay.
    avisos: Option<(u32, u32, u32)>,
    paso: usize, // la separación de los timbres (4 << CAP.DSTRD)
    espera_ms: u64,
    /// A16, para probar: la orden siguiente no espera su terminada (vence su plazo en el acto).
    sin_esperar: core::cell::Cell<bool>,
}

impl Nvme {
    /// Pone una orden en `c` y toca su timbre, sin esperar: su identificador. Nada si la cola está llena (el
    /// controlador aún no ha tomado las de antes).
    fn enviar(&self, c: &mut Cola, opcode: u8, nsid: u32, prp: (u64, u64), cdw: [u32; 6]) -> Option<u16> {
        if (c.cola + 1) % ENTRADAS == c.sq_cabeza {
            return None;
        }
        let (prp1, prp2) = prp;
        let e = c.sq + 64 * c.cola;
        for i in 0..16 {
            poner32(e + 4 * i, 0);
        }
        c.cid = c.cid.wrapping_add(1);
        poner32(e, opcode as u32 | (c.cid as u32) << 16);
        poner32(e + 4, nsid);
        poner32(e + 24, prp1 as u32);
        poner32(e + 28, (prp1 >> 32) as u32);
        poner32(e + 32, prp2 as u32);
        poner32(e + 36, (prp2 >> 32) as u32);
        for (i, v) in cdw.iter().enumerate() {
            poner32(e + 40 + 4 * i, *v);
        }
        dashos::barrera::escrituras();
        c.cola = (c.cola + 1) % ENTRADAS;
        escribir(TIMBRES + (2 * c.id) * self.paso, c.cola as u32);
        Some(c.cid)
    }

    /// La siguiente terminada de `c`, esperando como mucho `ms` (con 0, solo se mira): su identificador, su estado
    /// (0 = bien) y su DW0. Nada si no llega a tiempo.
    fn terminada(&self, c: &mut Cola, ms: u64) -> Option<(u16, u32, u32)> {
        // La terminada llega cuando su bit de fase (DW3, bit 16) es el que se espera (4.6).
        let t = c.cq + 16 * c.cabeza;
        let fase = c.fase;
        let hecha = || (dma32(t + 12) >> 16) & 1 == fase;
        let lista = match self.avisos {
            _ if ms == 0 => hecha(),
            None => esperar(ms, hecha),
            Some(a) => hecha() || esperar_aviso(a, ms, &hecha),
        };
        if !lista {
            return None;
        }
        dashos::barrera::lecturas();
        let (dw0, dw2, dw3) = (dma32(t), dma32(t + 8), dma32(t + 12));
        c.cabeza = (c.cabeza + 1) % ENTRADAS;
        if c.cabeza == 0 {
            c.fase ^= 1;
        }
        escribir(TIMBRES + (2 * c.id + 1) * self.paso, c.cabeza as u32);
        c.sq_cabeza = (dw2 & 0xffff) as usize % ENTRADAS;
        Some(((dw3 & 0xffff) as u16, (dw3 >> 17) & 0x7fff, dw0))
    }

    /// Manda una orden por `c` y espera a que termine: su estado (0 = bien) y su DW0.
    fn orden(&self, c: &mut Cola, opcode: u8, nsid: u32, prp: (u64, u64), cdw: [u32; 6]) -> Result<u32, u32> {
        let sin_esperar = self.sin_esperar.replace(false);
        let Some(cid) = self.enviar(c, opcode, nsid, prp, cdw) else { return Err(u32::MAX) };
        loop {
            // Si no llega, se deja pendiente: su terminada, cuando llegue, se descarta (abajo).
            let Some((id, estado, dw0)) = self.terminada(c, if sin_esperar { 0 } else { self.espera_ms }) else { return Err(u32::MAX) };
            // A16: la terminada de una orden anterior que venció su plazo. Antes se tomaba por la de esta, el
            // identificador no casaba y desde ahí todo daba error. Se descarta y se sigue esperando la nuestra.
            if id != cid {
                println!("  [nvme] terminada tardía descartada (orden {})", id);
                continue;
            }
            return if estado != 0 { Err(estado) } else { Ok(dw0) };
        }
    }
}

/// Espera a que se cumpla `hecha`: gira 50 µs y después duerme hasta la interrupción (que se vuelve a permitir) o
/// hasta el plazo, con un temporizador de un disparo en el mismo canal. Un aviso viejo (de otra orden, o de un
/// temporizador que ya había saltado) solo hace mirar otra vez.
fn esperar_aviso(avisos: (u32, u32, u32), ms: u64, hecha: &dyn Fn() -> bool) -> bool {
    if esperar_us(50, hecha) {
        return true;
    }
    let (lectura, escritura, irq) = avisos;
    let fin = dashos::ahora_us() + ms * 1000;
    let t = dashos::temporizador_crear(escritura, ms * 1000, 0);
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 1];
    let bien = loop {
        if hecha() {
            break true;
        }
        if dashos::ahora_us() >= fin {
            break hecha();
        }
        if dashos::canal_leer(lectura, &mut buf, &mut ll).0 == 4 {
            dashos::interrupcion_terminar(irq);
        }
    };
    if t > 0 {
        dashos::cerrar(t as u32);
    }
    bien
}

fn esperar_us(us: u64, cond: &dyn Fn() -> bool) -> bool {
    let fin = dashos::ahora_us() + us;
    while dashos::ahora_us() < fin {
        if cond() {
            return true;
        }
    }
    cond()
}

/// Los `n` bytes de texto de `off` en la memoria DMA, sin los espacios del final (los de Identify van rellenos).
fn texto(off: usize, n: usize, b: &mut [u8; 64]) -> &str {
    let mut l = 0;
    for i in 0..n.min(64) {
        let c = dma8(off + i);
        b[i] = if (0x20..0x7f).contains(&c) { c } else { b'?' };
        if c != b' ' && c != 0 {
            l = i + 1;
        }
    }
    core::str::from_utf8(&b[..l]).unwrap_or("?")
}

fn principal() -> u64 {
    // Lo primero, antes de crear llaves: si hay llave 4, la 3 es la interrupción y la 4 el canal "disco"; si no, el
    // canal es la 3.
    let existe = |k: u32| {
        let copia = dashos::llave_duplicar(k, dashos::LEER, 0);
        if copia > 0 {
            dashos::cerrar(copia as u32);
        }
        copia > 0
    };
    let (irq, disco) = if existe(4) { (Some(3), 4) } else { (None, 3) };
    if dashos::registros_proyectar(2, VA) != 0 {
        println!("  [nvme] sin registros");
        return 1;
    }
    let avisos = irq.and_then(|i| {
        let (a, b) = dashos::canal_crear();
        (dashos::interrupcion_asociar(i, b) == 0).then_some((a, b, i))
    });
    let cap = (leer(CAP) as u64) | (leer(CAP + 4) as u64) << 32;
    let vs = leer(VS);
    let n = Nvme { avisos, paso: 4 << ((cap >> 32) & 0xf), espera_ms: 500 * ((cap >> 24) & 0xff).max(1), sin_esperar: core::cell::Cell::new(false) };
    if (cap >> 48) & 0xf != 0 {
        println!("  [nvme] no admite páginas de 4 KB");
        return 2;
    }
    // Apagar (por si el firmware lo dejó encendido) y esperar a que deje de estar listo (3.5.2).
    escribir(CC, leer(CC) & !CC_EN);
    if !esperar(n.espera_ms, || leer(CSTS) & CSTS_RDY == 0) {
        println!("  [nvme] no se apaga");
        return 3;
    }
    // Con el controlador apagado, lo que dejara en cuarentena uno anterior puede volver (M5).
    dashos::dma_liberar(1);
    let (m, bus) = dashos::memoria_dma(1, TAM_DMA);
    LISTA_BUS.store(bus + M_LISTA as u64, core::sync::atomic::Ordering::Relaxed);
    if m <= 0 || dashos::memoria_proyectar(m as u32, VA_DMA, true) != 0 {
        println!("  [nvme] sin memoria DMA");
        return 4;
    }
    for i in (0..TAM_DMA).step_by(4) {
        poner32(i, 0);
    }
    // Colas de administración: tamaños (menos uno), direcciones, y encender (3.5.1).
    let q = (ENTRADAS - 1) as u32;
    escribir(AQA, q | q << 16);
    escribir64(ASQ, bus + M_ADM_SQ as u64);
    escribir64(ACQ, bus + M_ADM_CQ as u64);
    escribir(CC, CC_TAMANOS | CC_EN);
    if !esperar(n.espera_ms, || leer(CSTS) & (CSTS_RDY | CSTS_CFS) != 0) || leer(CSTS) & CSTS_CFS != 0 {
        println!("  [nvme] no se enciende");
        return 5;
    }
    let mut adm = Cola { id: 0, sq: M_ADM_SQ, cq: M_ADM_CQ, cola: 0, cabeza: 0, fase: 1, cid: 0, sq_cabeza: 0 };
    let datos = bus + M_DATOS as u64;
    // Identify del controlador (CNS 1): modelo en los bytes 24 a 63.
    if n.orden(&mut adm, O_IDENTIFICAR, 0, (datos, 0), [1, 0, 0, 0, 0, 0]).is_err() {
        println!("  [nvme] Identify falla");
        return 6;
    }
    let mut b = [0u8; 64];
    println!("  [nvme] versión {}.{}, modelo \"{}\"", vs >> 16, (vs >> 8) & 0xff, texto(M_DATOS + 24, 40, &mut b));
    // MDTS (byte 77): lo más que admite una transferencia, 2^MDTS páginas (0 = sin tope). Si no llega a la ventana,
    // las transferencias se quedan en dos páginas, como antes.
    let mdts = dma8(M_DATOS + 77);
    // ONCS (bytes 520 y 521), bit 2: admite Dataset Management.
    if dma8(M_DATOS + 520) & 4 != 0 {
        DSM_BUS.store(datos, core::sync::atomic::Ordering::Relaxed);
    }
    if mdts != 0 && (4096usize << mdts) < VENTANA {
        LISTA_BUS.store(0, core::sync::atomic::Ordering::Relaxed);
    }
    // Identify del espacio de nombres 1 (CNS 0): sectores (NSZE) y su tamaño (el formato que dice FLBAS).
    if n.orden(&mut adm, O_IDENTIFICAR, 1, (datos, 0), [0, 0, 0, 0, 0, 0]).is_err() {
        println!("  [nvme] sin espacio de nombres 1");
        return 7;
    }
    let sectores = dma32(M_DATOS) as u64 | (dma32(M_DATOS + 4) as u64) << 32;
    let formato = (dma8(M_DATOS + 26) & 0xf) as usize;
    let tam_sector = 1usize << ((dma32(M_DATOS + 128 + 4 * formato) >> 16) & 0xff);
    println!("  [nvme] disco de {} sectores de {} bytes ({} KB)", sectores, tam_sector, sectores * tam_sector as u64 / 1024);
    if tam_sector > 2048 {
        println!("  [nvme] sectores demasiado grandes");
        return 8;
    }
    // Una cola de E/S, la 1: primero la de terminadas (con interrupción, la del vector 0, si la hay) y luego la de
    // envío (5.4, 5.5).
    let tam = q << 16 | 1;
    if n.orden(&mut adm, O_CREAR_CQ, 0, (bus + M_ES_CQ as u64, 0), [tam, 1 | (n.avisos.is_some() as u32) << 1, 0, 0, 0, 0]).is_err()
        || n.orden(&mut adm, O_CREAR_SQ, 0, (bus + M_ES_SQ as u64, 0), [tam, 1 << 16 | 1, 0, 0, 0, 0]).is_err()
    {
        println!("  [nvme] no crea la cola de E/S");
        return 9;
    }
    let mut es = Cola { id: 1, sq: M_ES_SQ, cq: M_ES_CQ, cola: 0, cabeza: 0, fase: 1, cid: 0, sq_cabeza: 0 };
    // Los dos primeros sectores (NLB va menos uno).
    if let Err(e) = n.orden(&mut es, O_LEER, 1, (datos, 0), [0, 0, 1, 0, 0, 0]) {
        println!("  [nvme] la lectura falla (estado {:#x})", e);
        return 10;
    }
    for s in 0..2 {
        let off = M_DATOS + s * tam_sector;
        let firma = dma8(off + 510) as u16 | (dma8(off + 511) as u16) << 8;
        let l = (0..64).find(|&i| dma8(off + i) == 0).unwrap_or(64);
        println!("  [nvme] sector {}: \"{}\", firma {:#06x}", s, texto(off, l, &mut b), firma);
    }
    println!("  [nvme] avisos {}", if n.avisos.is_some() { "por su interrupción" } else { "girando" });
    println!("  [nvme] descartar: {}", if DSM_BUS.load(core::sync::atomic::Ordering::Relaxed) != 0 { "sí" } else { "no" });
    // A14: la tabla de particiones, sector a sector por la página de datos.
    let mut leer = |s: u64, b: &mut [u8]| {
        n.orden(&mut es, O_LEER, 1, (datos, 0), [s as u32, (s >> 32) as u32, 0, 0, 0, 0]).is_ok() && {
            for (i, x) in b.iter_mut().enumerate() {
                *x = dma8(M_DATOS + i);
            }
            true
        }
    };
    let tabla = dashos::particiones::leer_tabla(&mut leer, sectores, tam_sector);
    use dashos::particiones::Clase;
    let mb = (1u64 << 20) / tam_sector as u64;
    let tramos = match tabla {
        Ok(t) => {
            let clase = if t.clase == Clase::Gpt { "GPT" } else { "MBR" };
            match (t.clase, t.dashos) {
                (Clase::Ninguna, _) => println!("  [nvme] sin tabla de particiones"),
                (_, Some((a, b))) => println!("  [nvme] particiones: {}; DashOS, sectores {} a {}", clase, a, b - 1),
                (_, None) => println!("  [nvme] particiones: {}; ninguna de DashOS", clase),
            }
            if let Some((a, b)) = t.linux {
                println!("  [nvme] la caja Linux: sectores {} a {} ({} MB)", a, b - 1, (b - a) * tam_sector as u64 >> 20);
            }
            let disco = (t.clase != Clase::Gpt).then_some((0, mb.min(t.primera).min(sectores)));
            [disco, t.dashos, t.dashos, t.linux, Some((0, sectores))]
        }
        Err(e) => {
            // Una tabla que no cuadra: no se adivina dónde está nada (ni siquiera la prueba en bruto, que escribe).
            println!("  [nvme] la tabla de particiones no vale ({:?}): no se abre a nadie", e);
            [None, None, None, None, Some((0, sectores))]
        }
    };
    servir(&n, &mut es, tramos, tam_sector, disco)
}

/// Un cliente: su marca, su ventana (la llave y su dirección de bus) y, si lo pidió, su anillo (la llave de su
/// memoria, dónde está proyectado aquí y el canal por el que se le despierta).
#[derive(Clone, Copy)]
struct Cliente {
    marca: u64,
    llave: u32,
    bus: u64,
    anillo: Option<(u32, usize, u32)>,
    /// Sus sectores, [desde, hasta) (A13).
    tramo: (u64, u64),
    /// Solo puede leer (A15: el comprobador).
    solo_leer: bool,
}

fn anillo(va: usize, off: usize) -> &'static core::sync::atomic::AtomicU32 {
    unsafe { &*((va + off) as *const core::sync::atomic::AtomicU32) }
}

/// Una lectura o escritura en la ventana de `c`, comprobada antes de llegar al disco: su estado.
fn transferir(n: &Nvme, es: &mut Cola, c: &Cliente, orden: u32, cuantos: u64, primero: u64, off: usize, tam_sector: usize) -> i32 {
    if orden == PEDIR_LEER_SIN_ESPERA {
        n.sin_esperar.set(true);
        let r = transferir(n, es, c, PEDIR_LEER, cuantos, primero, off, tam_sector);
        n.sin_esperar.set(false);
        return r;
    }
    if orden == PEDIR_VACIAR {
        // Cuando llega aquí no hay nada en vuelo: todas las escrituras de antes ya terminaron.
        return if n.orden(es, O_VACIAR, 1, (0, 0), [0; 6]).is_ok() { 0 } else { ERROR_DEL_DISCO };
    }
    match preparar(c, orden, cuantos, primero, off, tam_sector, 0) {
        Err(e) => e,
        Ok((op, prp, cdw)) => {
            if n.orden(es, op, 1, prp, cdw).is_ok() { 0 } else { ERROR_DEL_DISCO }
        }
    }
}

/// Comprueba una lectura, escritura o descarte de `c` y prepara su orden NVMe, con su lista de páginas (o su tramo de
/// descarte) en el sitio `sitio` (uno por orden en vuelo): código, PRP y CDW 10 a 15. Si no hay que mandar nada, el
/// estado: el del rechazo, o 0 (un descarte en un disco que no sabe).
fn preparar(c: &Cliente, orden: u32, cuantos: u64, primero: u64, off: usize, tam_sector: usize, sitio: usize) -> Result<(u8, (u64, u64), [u32; 6]), i32> {
    let bytes = cuantos as usize * tam_sector;
    if orden != PEDIR_LEER && orden != PEDIR_ESCRIBIR && orden != PEDIR_DESCARTAR {
        Err(MAL_FORMADO)
    } else if c.solo_leer && orden != PEDIR_LEER {
        Err(SIN_PERMISO)
    } else if cuantos == 0 || primero.checked_add(cuantos).is_none_or(|f| f > c.tramo.1 - c.tramo.0) {
        // Fuera de su tramo, que para él es todo el disco (A13, A14).
        Err(FUERA_DEL_DISCO)
    } else if orden == PEDIR_DESCARTAR {
        let primero = c.tramo.0 + primero;
        let lista = DSM_BUS.load(core::sync::atomic::Ordering::Relaxed);
        if lista == 0 {
            return Err(0);
        }
        // Un tramo (16 bytes): atributos, cuántos sectores y el primero. NR = 0 (uno), AD.
        let m = M_DATOS + 16 * sitio;
        poner32(m, 0);
        poner32(m + 4, cuantos as u32);
        poner32(m + 8, primero as u32);
        poner32(m + 12, (primero >> 32) as u32);
        Ok((O_DSM, (lista + 16 * sitio as u64, 0), [0, 1 << 2, 0, 0, 0, 0]))
    } else if off % 4 != 0 || off.checked_add(bytes).is_none_or(|f| f > VENTANA) {
        Err(FUERA_DE_LA_VENTANA)
    } else {
        let primero = c.tramo.0 + primero;
        let prp1 = c.bus + off as u64;
        let paginas = (off % 4096 + bytes).div_ceil(4096);
        let lista = LISTA_BUS.load(core::sync::atomic::Ordering::Relaxed);
        let prp2 = match paginas {
            1 => 0,
            2 => (prp1 & !0xfff) + 4096,
            _ if lista != 0 => {
                // Las páginas segunda y siguientes, en su lista; PRP2 apunta a ella.
                let m = M_LISTA + PASO_LISTA * sitio;
                for k in 1..paginas {
                    let p = (prp1 & !0xfff) + 4096 * k as u64;
                    poner32(m + 8 * (k - 1), p as u32);
                    poner32(m + 8 * (k - 1) + 4, (p >> 32) as u32);
                }
                lista + (PASO_LISTA * sitio) as u64
            }
            // Sin lista (el controlador no admite tanto): como mucho dos páginas.
            _ => return Err(FUERA_DE_LA_VENTANA),
        };
        let op = if orden == PEDIR_LEER { O_LEER } else { O_ESCRIBIR };
        Ok((op, (prp1, prp2), [primero as u32, (primero >> 32) as u32, cuantos as u32 - 1, 0, 0, 0]))
    }
}

/// Un envío del anillo dentro de un lote (A15): lo que pide y cómo va.
#[derive(Clone, Copy)]
struct Pieza {
    orden: u32,
    primero: u64,
    cuantos: u64,
    off: usize,
    etiqueta: u32,
    /// El identificador de su orden NVMe mientras está en vuelo.
    cid: Option<u16>,
    estado: i32,
}

/// Si `b`, que va después de `a` en el anillo, tiene que esperar a que `a` acabe: el disco no guarda el orden entre
/// órdenes en vuelo. Chocan si tocan los mismos sectores y una escribe o descarta, o el mismo sitio de la ventana y una
/// lee (y escribe en ella). Con números raros (que dan la vuelta) se supone que chocan.
fn choca(a: &Pieza, b: &Pieza, tam_sector: usize) -> bool {
    let tramo = |p: &Pieza| (p.primero, p.primero.saturating_add(p.cuantos));
    let ventana = |p: &Pieza| (p.off, p.off.saturating_add((p.cuantos as usize).saturating_mul(tam_sector)));
    let pisa = |x: (u64, u64), y: (u64, u64)| x.0 < y.1 && y.0 < x.1;
    let cambia = |p: &Pieza| p.orden != PEDIR_LEER;
    let en_ventana = |p: &Pieza| p.orden != PEDIR_DESCARTAR;
    let v = |p: &Pieza| (ventana(p).0 as u64, ventana(p).1 as u64);
    (pisa(tramo(a), tramo(b)) && (cambia(a) || cambia(b)))
        || (en_ventana(a) && en_ventana(b) && pisa(v(a), v(b)) && (a.orden == PEDIR_LEER || b.orden == PEDIR_LEER))
}

/// Atiende lo que haya en el anillo de `c`: copia cada envío antes de mirarlo (el cliente puede cambiarlo a la vez),
/// pone su terminada y, si el cliente espera, le avisa. Devuelve cuántos atendió. Si no caben más terminadas, para.
///
/// A15: las lecturas, escrituras y descartes seguidos van al disco a la vez, en lotes de hasta `EN_VUELO`; antes, de
/// una en una, esperando cada terminada. Un lote se corta en lo que tenga que ir detrás de otro del lote (`choca`) y en
/// todo lo demás (vaciar, que va solo y con nada en vuelo; órdenes malas). Las terminadas se ponen en el orden del
/// anillo, cuando ha acabado el lote entero.
fn atender_anillo(n: &Nvme, es: &mut Cola, c: &Cliente, tam_sector: usize) -> u32 {
    use core::sync::atomic::Ordering::*;
    let Some((_, va, aviso)) = c.anillo else { return 0 };
    let mut hechas = 0;
    loop {
        let (cabeza, cola) = (anillo(va, A_SQ_CABEZA).load(Relaxed), anillo(va, A_SQ_COLA).load(Acquire));
        let (cq_cabeza, cq_cola) = (anillo(va, A_CQ_CABEZA).load(Acquire), anillo(va, A_CQ_COLA).load(Relaxed));
        let sitio = A_ENTRADAS.saturating_sub(cq_cola.wrapping_sub(cq_cabeza));
        let pendientes = cola.wrapping_sub(cabeza).min(A_ENTRADAS);
        if pendientes == 0 || sitio == 0 {
            break;
        }
        let mut lote = [Pieza { orden: 0, primero: 0, cuantos: 0, off: 0, etiqueta: 0, cid: None, estado: 0 }; EN_VUELO];
        let mut nl = 0;
        while nl < EN_VUELO && (nl as u32) < pendientes.min(sitio) {
            let e = va + A_SQ + 32 * (cabeza.wrapping_add(nl as u32) % A_ENTRADAS) as usize;
            let mut b = [0u8; 32];
            for (i, x) in b.iter_mut().enumerate() {
                *x = unsafe { core::ptr::read_volatile((e + i) as *const u8) };
            }
            let u32_en = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
            let mut p = Pieza {
                orden: u32_en(0),
                cuantos: u32_en(4) as u64,
                primero: u32_en(8) as u64 | (u32_en(12) as u64) << 32,
                off: u32_en(16) as usize,
                etiqueta: u32_en(20),
                cid: None,
                estado: 0,
            };
            let junta = p.orden == PEDIR_LEER || p.orden == PEDIR_ESCRIBIR || p.orden == PEDIR_DESCARTAR;
            if !junta {
                // Sola, con nada en vuelo: si el lote ya tiene algo, en la vuelta siguiente.
                if nl == 0 {
                    p.estado = transferir(n, es, c, p.orden, p.cuantos, p.primero, p.off, tam_sector);
                    lote[0] = p;
                    nl = 1;
                }
                break;
            }
            if lote[..nl].iter().any(|a| choca(a, &p, tam_sector)) {
                break;
            }
            match preparar(c, p.orden, p.cuantos, p.primero, p.off, tam_sector, nl) {
                Err(e) => p.estado = e,
                Ok((op, prp, cdw)) => match n.enviar(es, op, 1, prp, cdw) {
                    Some(id) => p.cid = Some(id),
                    None => p.estado = SIN_SITIO,
                },
            }
            lote[nl] = p;
            nl += 1;
        }
        // Las terminadas del lote, en el orden en que lleguen; si alguna no llega a tiempo, las que falten, con error
        // (su terminada, si llega después, se descarta como en A16).
        let fin = dashos::ahora_us() + n.espera_ms * 1000;
        while lote[..nl].iter().any(|p| p.cid.is_some()) {
            let ms = fin.saturating_sub(dashos::ahora_us()).div_ceil(1000);
            match n.terminada(es, ms.max(1)) {
                None => {
                    for p in lote[..nl].iter_mut().filter(|p| p.cid.is_some()) {
                        p.cid = None;
                        p.estado = ERROR_DEL_DISCO;
                    }
                }
                Some((id, estado, _)) => match lote[..nl].iter_mut().find(|p| p.cid == Some(id)) {
                    Some(p) => {
                        p.cid = None;
                        p.estado = if estado == 0 { 0 } else { ERROR_DEL_DISCO };
                    }
                    None => println!("  [nvme] terminada tardía descartada (orden {})", id),
                },
            }
        }
        for (k, p) in lote[..nl].iter().enumerate() {
            let t = va + A_CQ + 16 * (cq_cola.wrapping_add(k as u32) % A_ENTRADAS) as usize;
            anillo(t, 0).store(p.etiqueta, Relaxed);
            anillo(t, 4).store(p.estado as u32, Relaxed);
        }
        anillo(va, A_CQ_COLA).store(cq_cola.wrapping_add(nl as u32), Release);
        anillo(va, A_SQ_CABEZA).store(cabeza.wrapping_add(nl as u32), Release);
        hechas += nl as u32;
    }
    core::sync::atomic::fence(SeqCst);
    if hechas > 0 && anillo(va, A_ESPERA).swap(0, SeqCst) != 0 {
        dashos::canal_escribir(aviso, &[0; 4], &[]);
    }
    hechas
}

/// El número en `NOMBRES` del cliente de la marca `marca`, por su nombre en el directorio.
fn nombre_de(nombres: &[(u64, [u8; 16])], marca: u64) -> Option<usize> {
    let (_, n) = nombres.iter().find(|e| e.0 == marca && marca != 0)?;
    let nombre = &n[..n.iter().position(|&c| c == 0).unwrap_or(16)];
    NOMBRES.iter().position(|t| t.as_bytes() == nombre)
}

/// El tramo, en sectores, del cliente de la marca `marca`, por su nombre en el directorio (A13).
fn tramo_de(nombres: &[(u64, [u8; 16])], tramos: &[Option<(u64, u64)>; 5], marca: u64) -> Option<(u64, u64)> {
    let (_, n) = nombres.iter().find(|e| e.0 == marca && marca != 0)?;
    let nombre = &n[..n.iter().position(|&c| c == 0).unwrap_or(16)];
    tramos[NOMBRES.iter().position(|t| t.as_bytes() == nombre)?]
}

/// Sirve el disco por el canal de la llave `disco`. Cada petición son 24 bytes (orden, sectores, primer sector y
/// desplazamiento en la ventana) con una llave para la respuesta; la respuesta, 24 bytes (estado, sectores de su tramo,
/// tamaño del sector y de la ventana) y, al abrir, la ventana; al pedir el anillo, su memoria. Un mensaje de 24 bytes
/// con la orden 4 y sin llave solo despierta al servicio para que mire los anillos.
fn servir(n: &Nvme, es: &mut Cola, tramos: [Option<(u64, u64)>; 5], tam_sector: usize, disco: u32) -> u64 {
    let mut clientes: [Option<Cliente>; MAX_CLIENTES] = [None; MAX_CLIENTES];
    // El directorio de `inicio` (marca y nombre de cada cliente): de él sale su tramo (A13).
    let mut nombres: [(u64, [u8; 16]); 8] = [(0, [0; 16]); 8];
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        // Antes de dormir: "duermo" en cada anillo y, después, una última mirada (un cliente que puso algo sin ver la
        // marca lo habría puesto antes de ella y se ve aquí; uno que la ve, avisa).
        let mut quedaba = 0;
        for c in clientes.iter().flatten() {
            if let Some((_, va, _)) = c.anillo {
                anillo(va, A_DUERME).store(1, core::sync::atomic::Ordering::SeqCst);
            }
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        for c in clientes.iter().flatten() {
            quedaba += atender_anillo(n, es, c, tam_sector);
        }
        if quedaba > 0 {
            continue;
        }
        let (r, k, marca) = dashos::canal_leer_marca(disco, &mut buf, &mut ll);
        for c in clientes.iter().flatten() {
            if let Some((_, va, _)) = c.anillo {
                anillo(va, A_DUERME).store(0, core::sync::atomic::Ordering::SeqCst);
            }
        }
        if r == E_CERRADO {
            return 0;
        }
        if r < 0 {
            // Sin sitio en la tabla o sin memoria: el mensaje vuelve a la cola; se espera un tic para no girar (B16).
            dashos::dormir_us(10_000);
            continue;
        }
        for &l in ll.iter().take(k).skip(1) {
            dashos::cerrar(l);
        }
        // "Mira mi anillo": sin respuesta (el anillo se atiende al dar la vuelta). Sin llave de respuesta no hay a quién
        // contestar (el directorio de `inicio` es así: no hace falta aquí).
        if k == 0 {
            // Una entrada del directorio de `inicio`: 24 bytes sin marca (marca y nombre).
            if r == 24 && marca == 0 {
                let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
                if let Some(e) = nombres.iter_mut().find(|e| e.0 == 0 || e.0 == m) {
                    e.0 = m;
                    e.1.copy_from_slice(&buf[8..24]);
                }
            }
            continue;
        }
        let respuesta = ll[0];
        let u32_en = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        let (orden, cuantos, primero, off) = (u32_en(0), u32_en(4) as u64, u32_en(8) as u64 | (u32_en(12) as u64) << 32, u32_en(16) as usize);
        let mut ventana = None;
        // Sin marca no se sabe de quién es: solo los clientes que puso `inicio` (o a quienes ellos den su llave).
        let estado = if r != 24 || marca == 0 {
            MAL_FORMADO
        } else if orden == PEDIR_ABRIR && tramo_de(&nombres, &tramos, marca).is_none() {
            SIN_PERMISO
        } else if orden == PEDIR_ABRIR {
            let tramo = tramo_de(&nombres, &tramos, marca).unwrap();
            let hueco = clientes.iter().position(|c| c.is_some_and(|c| c.marca == marca)).or_else(|| clientes.iter().position(|c| c.is_none()));
            match hueco {
                Some(h) => {
                    if clientes[h].is_none() {
                        let (m, bus) = dashos::memoria_dma(1, VENTANA);
                        if m > 0 {
                            let solo_leer = nombre_de(&nombres, marca) == Some(SOLO_LEER);
                            clientes[h] = Some(Cliente { marca, llave: m as u32, bus, anillo: None, tramo, solo_leer });
                        }
                    }
                    match clientes[h] {
                        Some(c) => {
                            // Con TRANSFERIR, que hace falta para mandarla (y el cliente puede pasarla a otro proceso
                            // suyo, como un sistema de ficheros).
                            let copia = dashos::llave_duplicar(c.llave, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
                            ventana = (copia > 0).then_some(copia as u32);
                            if ventana.is_some() { 0 } else { SIN_SITIO }
                        }
                        None => SIN_SITIO,
                    }
                }
                None => SIN_SITIO,
            }
        } else if orden == PEDIR_LEER || orden == PEDIR_ESCRIBIR || orden == PEDIR_VACIAR || orden == PEDIR_DESCARTAR || orden == PEDIR_LEER_SIN_ESPERA {
            match clientes.iter().flatten().find(|c| c.marca == marca) {
                None => MAL_FORMADO,
                Some(c) => transferir(n, es, c, orden, cuantos, primero, off, tam_sector),
            }
        } else if orden == PEDIR_ANILLO {
            // El anillo: una página nueva, proyectada aquí, y el canal de la respuesta se queda para despertarle.
            match clientes.iter().position(|c| c.is_some_and(|c| c.marca == marca)) {
                Some(h) => {
                    // A15: si ya tenía uno, es un cliente relanzado (`inicio` le da la misma marca) o uno que empieza de
                    // nuevo: el viejo se suelta, con lo que tuviera a medias, y deja de atenderse. Antes, el relanzado
                    // se quedaba sin anillo.
                    if let Some((m_viejo, va_viejo, aviso_viejo)) = clientes[h].and_then(|c| c.anillo) {
                        dashos::memoria_quitar(va_viejo);
                        dashos::cerrar(m_viejo);
                        dashos::cerrar(aviso_viejo);
                        if let Some(c) = clientes[h].as_mut() {
                            c.anillo = None;
                        }
                        println!("  [nvme] anillo nuevo para un cliente que ya tenía uno: el viejo, fuera");
                    }
                    let m = dashos::memoria_crear(4096);
                    let va = VA_ANILLOS + 4096 * h;
                    if m > 0 && dashos::memoria_proyectar(m as u32, va, true) == 0 {
                        let copia = dashos::llave_duplicar(m as u32, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
                        ventana = (copia > 0).then_some(copia as u32);
                        let aviso = dashos::llave_duplicar(respuesta, dashos::ESCRIBIR, 0);
                        if let Some(c) = clientes[h].as_mut() {
                            c.anillo = Some((m as u32, va, aviso as u32));
                        }
                        if ventana.is_some() && aviso > 0 { 0 } else { SIN_SITIO }
                    } else {
                        SIN_SITIO
                    }
                }
                None => MAL_FORMADO, // sin abrir antes
            }
        } else {
            MAL_FORMADO
        };
        let mut r = [0u8; 24];
        r[..4].copy_from_slice(&estado.to_le_bytes());
        // Sus sectores: los de su tramo, que es lo que ve (A14).
        let suyos = tramo_de(&nombres, &tramos, marca).map_or(0, |t| t.1 - t.0);
        r[8..16].copy_from_slice(&suyos.to_le_bytes());
        r[16..20].copy_from_slice(&(tam_sector as u32).to_le_bytes());
        r[20..24].copy_from_slice(&(VENTANA as u32).to_le_bytes());
        let llaves: &[u32] = match &ventana {
            Some(v) => core::slice::from_ref(v),
            None => &[],
        };
        if dashos::canal_escribir(respuesta, &r, llaves) != 0 {
            if let Some(v) = ventana {
                dashos::cerrar(v);
                r[..4].copy_from_slice(&SIN_SITIO.to_le_bytes());
                dashos::canal_escribir(respuesta, &r, &[]);
            }
        }
        dashos::cerrar(respuesta);
    }
}
