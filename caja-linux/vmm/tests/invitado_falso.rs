//! Un "invitado de mentira": hace con los aparatos lo que haría un controlador de Linux (registros de virtio-mmio,
//! colas en su memoria, avisos), y también lo que haría un invitado hostil. Nada puede acabar en pánico.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use caja_vmm::gic::{self, Distribuidor};
use caja_vmm::maquina::{Maquina, Operacion, SalidaMmio};
use caja_vmm::memoria::{MemoriaInvitado, Ram};
use caja_vmm::pl011::{self, Pl011};
use caja_vmm::virtio::bloque::{self, Bloque, Bloques, DiscoEnMemoria, ErrorBloques};
use caja_vmm::virtio::cola::{DESC_F_INDIRECT, DESC_F_NEXT, DESC_F_WRITE};
use caja_vmm::virtio::consola::{Consola, ExtremoConsola};
use caja_vmm::virtio::globo::{self, ExtremoGlobo, Globo};
use caja_vmm::virtio::mmio::*;
use caja_vmm::virtio::red::{self, ExtremoRed, Red};
use caja_vmm::virtio::vsock::{self, Cabecera, ExtremoVsock, Vsock};
use caja_vmm::virtio::{F_INDIRECT_DESC, F_VERSION_1};

const RAM: u64 = 0x4000_0000;
const TAM_RAM: usize = 4 << 20;
const GICD: u64 = 0x0800_0000;
const UART: u64 = 0x0900_0000;
const CONSOLA: u64 = 0x0a00_0000;
const DISCO: u64 = 0x0a00_0200;
const SPI_UART: u32 = 1;
const SPI_CONSOLA: u32 = 16;
const SPI_DISCO: u32 = 17;
const RED: u64 = 0x0a00_0400;
const SPI_RED: u32 = 18;
const VSOCK: u64 = 0x0a00_0600;
const SPI_VSOCK: u32 = 19;
const GLOBO: u64 = 0x0a00_0800;
const SPI_GLOBO: u32 = 20;

/// El lado de DashOS del globo en las pruebas: apunta lo que mete y saca el invitado.
#[derive(Default)]
struct Hinchado {
    objetivo: u32,
    inflado: Vec<u64>,
    desinflado: Vec<u64>,
    actual: Vec<u32>,
    reiniciado: u32,
}

#[derive(Default, Clone)]
struct ExtremoHinchado(Rc<RefCell<Hinchado>>);

impl ExtremoGlobo for ExtremoHinchado {
    fn inflar(&mut self, dirs: &[u64]) {
        self.0.borrow_mut().inflado.extend_from_slice(dirs);
    }
    fn desinflar(&mut self, dirs: &[u64]) {
        self.0.borrow_mut().desinflado.extend_from_slice(dirs);
    }
    fn objetivo(&self) -> u32 {
        self.0.borrow().objetivo
    }
    fn actual(&mut self, paginas: u32) {
        self.0.borrow_mut().actual.push(paginas);
    }
    fn reiniciado(&mut self) {
        self.0.borrow_mut().reiniciado += 1;
    }
}

/// El lado de DashOS de vsock en las pruebas: escucha en el puerto 5000, guarda lo que llega por cada conexión (con un
/// tope para probar la espera) y tiene una cola de salida por conexión.
#[derive(Default)]
struct Enchufes {
    llegado: std::collections::BTreeMap<u32, Vec<u8>>,
    salen: std::collections::BTreeMap<u32, VecDeque<u8>>,
    cerradas: Vec<u32>,
    cerrar: Vec<u32>,
    /// Cuánto toma de cada vez (None: todo).
    toma: Option<usize>,
}

#[derive(Clone, Default)]
struct ExtremoEnchufes(Rc<RefCell<Enchufes>>);

impl ExtremoVsock for ExtremoEnchufes {
    fn conectar(&mut self, con: u32, puerto: u32) -> bool {
        if puerto != 5000 {
            return false;
        }
        let mut e = self.0.borrow_mut();
        e.llegado.insert(con, Vec::new());
        e.salen.insert(con, VecDeque::new());
        true
    }
    fn recibir(&mut self, con: u32, datos: &[u8]) -> usize {
        let mut e = self.0.borrow_mut();
        let n = e.toma.map_or(datos.len(), |t| t.min(datos.len()));
        e.llegado.get_mut(&con).unwrap().extend_from_slice(&datos[..n]);
        n
    }
    fn cerrada(&mut self, con: u32) {
        self.0.borrow_mut().cerradas.push(con);
    }
    fn enviar(&mut self, con: u32, buf: &mut [u8]) -> usize {
        let mut e = self.0.borrow_mut();
        let Some(q) = e.salen.get_mut(&con) else { return 0 };
        let n = buf.len().min(q.len());
        for b in buf.iter_mut().take(n) {
            *b = q.pop_front().unwrap();
        }
        n
    }
    fn hay_algo(&self) -> bool {
        let e = self.0.borrow();
        e.salen.values().any(|q| !q.is_empty()) || !e.cerrar.is_empty()
    }
    fn cerrar(&mut self) -> Option<u32> {
        self.0.borrow_mut().cerrar.pop()
    }
}

#[derive(Default)]
struct Cable {
    salieron: Vec<Vec<u8>>,
    entran: VecDeque<Vec<u8>>,
}

#[derive(Clone, Default)]
struct ExtremoCable(Rc<RefCell<Cable>>);

impl ExtremoRed for ExtremoCable {
    fn enviar(&mut self, t: &[u8]) {
        self.0.borrow_mut().salieron.push(t.to_vec());
    }
    fn hay_trama(&self) -> bool {
        !self.0.borrow().entran.is_empty()
    }
    fn recibir(&mut self) -> Option<Vec<u8>> {
        self.0.borrow_mut().entran.pop_front()
    }
}

#[derive(Default)]
struct Terminal {
    salida: Vec<u8>,
    entrada: VecDeque<u8>,
}

#[derive(Clone, Default)]
struct Extremo(Rc<RefCell<Terminal>>);

impl ExtremoConsola for Extremo {
    fn salida(&mut self, datos: &[u8]) {
        self.0.borrow_mut().salida.extend_from_slice(datos);
    }
    fn hay_entrada(&self) -> bool {
        !self.0.borrow().entrada.is_empty()
    }
    fn entrada(&mut self, buf: &mut [u8]) -> usize {
        let mut t = self.0.borrow_mut();
        let n = buf.len().min(t.entrada.len());
        for b in buf.iter_mut().take(n) {
            *b = t.entrada.pop_front().unwrap();
        }
        n
    }
}

#[derive(Clone)]
struct Disco(Rc<RefCell<DiscoEnMemoria>>);

impl Bloques for Disco {
    fn sectores(&self) -> u64 {
        self.0.borrow().sectores()
    }
    fn solo_lectura(&self) -> bool {
        self.0.borrow().solo_lectura
    }
    fn leer(&mut self, s: u64, b: &mut [u8]) -> Result<(), ErrorBloques> {
        self.0.borrow_mut().leer(s, b)
    }
    fn escribir(&mut self, s: u64, d: &[u8]) -> Result<(), ErrorBloques> {
        self.0.borrow_mut().escribir(s, d)
    }
}

struct Banco {
    m: Maquina<Ram<Vec<u8>>>,
    consola: Extremo,
    uart: Extremo,
    disco: Disco,
    cable: ExtremoCable,
    enchufes: ExtremoEnchufes,
    globo: ExtremoHinchado,
}

fn banco_con(solo_lectura: bool) -> Banco {
    let consola = Extremo::default();
    let uart = Extremo::default();
    let disco = Disco(Rc::new(RefCell::new(DiscoEnMemoria { datos: vec![0; 64 * 512], solo_lectura })));
    let mut m = Maquina::nueva(Ram::nueva(RAM, TAM_RAM), Distribuidor::nuevo(2, 64), GICD);
    m.añadir(UART, SPI_UART, Box::new(Pl011::nuevo(Box::new(uart.clone())))).unwrap();
    m.añadir(CONSOLA, SPI_CONSOLA, Box::new(TransporteMmio::nuevo(Consola::nueva(Box::new(consola.clone())))))
        .unwrap();
    m.añadir(DISCO, SPI_DISCO, Box::new(TransporteMmio::nuevo(Bloque::nuevo(Box::new(disco.clone()))))).unwrap();
    let cable = ExtremoCable::default();
    let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    m.añadir(RED, SPI_RED, Box::new(TransporteMmio::nuevo(Red::nueva(Box::new(cable.clone()), mac)))).unwrap();
    let enchufes = ExtremoEnchufes::default();
    m.añadir(VSOCK, SPI_VSOCK, Box::new(TransporteMmio::nuevo(Vsock::nuevo(Box::new(enchufes.clone()))))).unwrap();
    let globo = ExtremoHinchado::default();
    m.añadir(GLOBO, SPI_GLOBO, Box::new(TransporteMmio::nuevo(Globo::nuevo(Box::new(globo.clone()))))).unwrap();
    // El GIC encendido, con las SPI de los aparatos habilitadas, como lo deja Linux.
    m.mmio(0, GICD + gic::CTLR, 4, Operacion::Escribir(1));
    m.mmio(0, GICD + gic::ISENABLER + 4, 4, Operacion::Escribir(u32::MAX as u64));
    Banco { m, consola, uart, disco, cable, enchufes, globo }
}

fn banco() -> Banco {
    banco_con(false)
}

/// Una cola en la memoria del invitado: tabla, anillo disponible y anillo usado.
#[derive(Clone, Copy)]
struct Cola {
    tam: u16,
    desc: u64,
    disp: u64,
    usado: u64,
    /// Siguiente descriptor libre.
    libre: u16,
    idx: u16,
    usados_vistos: u16,
}

impl Cola {
    fn en(base: u64, tam: u16) -> Self {
        let desc = base;
        let disp = desc + 16 * tam as u64;
        let usado = (disp + 6 + 2 * tam as u64).next_multiple_of(4);
        Cola { tam, desc, disp, usado, libre: 0, idx: 0, usados_vistos: 0 }
    }
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

    /// La secuencia de §3.1.1 hasta FEATURES_OK. Devuelve si el aparato la aceptó.
    fn negociar(&mut self, base: u64, aceptar: u64) -> bool {
        self.w(base + R_STATUS, 0);
        self.w(base + R_STATUS, S_ACKNOWLEDGE);
        self.w(base + R_STATUS, S_ACKNOWLEDGE | S_DRIVER);
        self.w(base + R_DRIVER_FEATURES_SEL, 0);
        self.w(base + R_DRIVER_FEATURES, aceptar as u32);
        self.w(base + R_DRIVER_FEATURES_SEL, 1);
        self.w(base + R_DRIVER_FEATURES, (aceptar >> 32) as u32);
        self.w(base + R_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_FEATURES_OK);
        self.r(base + R_STATUS) & S_FEATURES_OK != 0
    }

    fn ofrecidas(&mut self, base: u64) -> u64 {
        self.w(base + R_DEVICE_FEATURES_SEL, 0);
        let lo = self.r(base + R_DEVICE_FEATURES) as u64;
        self.w(base + R_DEVICE_FEATURES_SEL, 1);
        lo | (self.r(base + R_DEVICE_FEATURES) as u64) << 32
    }

    fn preparar_cola(&mut self, base: u64, i: u32, c: &Cola) -> bool {
        self.w(base + R_QUEUE_SEL, i);
        assert!(self.r(base + R_QUEUE_NUM_MAX) >= c.tam as u32);
        self.w(base + R_QUEUE_NUM, c.tam as u32);
        self.w(base + R_QUEUE_DESC_LOW, c.desc as u32);
        self.w(base + R_QUEUE_DESC_HIGH, (c.desc >> 32) as u32);
        self.w(base + R_QUEUE_DRIVER_LOW, c.disp as u32);
        self.w(base + R_QUEUE_DRIVER_HIGH, (c.disp >> 32) as u32);
        self.w(base + R_QUEUE_DEVICE_LOW, c.usado as u32);
        self.w(base + R_QUEUE_DEVICE_HIGH, (c.usado >> 32) as u32);
        self.w(base + R_QUEUE_READY, 1);
        self.r(base + R_QUEUE_READY) == 1
    }

    /// Un aparato listo con `colas`, cada una en su zona de la RAM. Acepta todo lo que ofrece.
    fn arrancar(&mut self, base: u64, colas: &[Cola]) -> u64 {
        let f = self.ofrecidas(base);
        assert!(self.negociar(base, f));
        for (i, c) in colas.iter().enumerate() {
            assert!(self.preparar_cola(base, i as u32, c));
        }
        self.w(base + R_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_FEATURES_OK | S_DRIVER_OK);
        assert_eq!(self.r(base + R_STATUS) & S_DRIVER_OK, S_DRIVER_OK);
        f
    }

    /// Escribe una cadena (dirección, largo, de escritura) en la tabla y la pone en el anillo disponible.
    fn poner(&mut self, c: &mut Cola, segs: &[(u64, u32, bool)]) -> u16 {
        let cabeza = c.libre;
        for (k, &(dir, largo, esc)) in segs.iter().enumerate() {
            let i = (c.libre + k as u16) % c.tam;
            let mut flags = if esc { DESC_F_WRITE } else { 0 };
            if k + 1 < segs.len() {
                flags |= DESC_F_NEXT;
            }
            self.escribir_desc(c.desc, i, dir, largo, flags, (i + 1) % c.tam);
        }
        c.libre = (c.libre + segs.len() as u16) % c.tam;
        self.publicar(c, cabeza);
        cabeza
    }

    fn escribir_desc(&mut self, tabla: u64, i: u16, dir: u64, largo: u32, flags: u16, sig: u16) {
        let d = tabla + 16 * i as u64;
        let m = self.mem();
        m.escribir_u64(d, dir).unwrap();
        m.escribir_u32(d + 8, largo).unwrap();
        m.escribir_u16(d + 12, flags).unwrap();
        m.escribir_u16(d + 14, sig).unwrap();
    }

    fn publicar(&mut self, c: &mut Cola, cabeza: u16) {
        let ranura = (c.idx % c.tam) as u64;
        let (disp, idx) = (c.disp, c.idx);
        self.mem().escribir_u16(disp + 4 + 2 * ranura, cabeza).unwrap();
        c.idx = idx.wrapping_add(1);
        self.mem().escribir_u16(disp + 2, c.idx).unwrap();
    }

    /// Lo nuevo del anillo usado: (cabeza, bytes escritos).
    fn usados(&mut self, c: &mut Cola) -> Vec<(u32, u32)> {
        let idx = self.mem().leer_u16(c.usado + 2).unwrap();
        let mut v = Vec::new();
        while c.usados_vistos != idx {
            let e = c.usado + 4 + 8 * (c.usados_vistos % c.tam) as u64;
            v.push((self.mem().leer_u32(e).unwrap(), self.mem().leer_u32(e + 4).unwrap()));
            c.usados_vistos = c.usados_vistos.wrapping_add(1);
        }
        v
    }

    fn pendiente_en_gic(&self, spi: u32) -> bool {
        self.m.gic.pendientes(0).iter().any(|e| e.intid == 32 + spi)
    }

    fn necesita_reinicio(&mut self, base: u64) -> bool {
        self.r(base + R_STATUS) & S_DEVICE_NEEDS_RESET != 0
    }
}

const BUF: u64 = RAM + 0x10_0000;

#[test]
fn identificacion_y_negociacion() {
    let mut b = banco();
    assert_eq!(b.r(CONSOLA + R_MAGIA), MAGIA);
    assert_eq!(b.r(CONSOLA + R_VERSION), 2);
    assert_eq!(b.r(CONSOLA + R_DEVICE_ID), 3);
    assert_eq!(b.r(DISCO + R_DEVICE_ID), 2);
    let f = b.ofrecidas(DISCO);
    assert!(f & F_VERSION_1 != 0 && f & bloque::F_FLUSH != 0 && f & bloque::F_RO == 0);
    // Sin VERSION_1 (un controlador "legacy") no se acepta.
    assert!(!b.negociar(DISCO, f & !F_VERSION_1));
    // Ni una característica que no se ofreció.
    assert!(!b.negociar(DISCO, f | 1 << 40));
    assert!(b.negociar(DISCO, F_VERSION_1));
    // Capacidad en la configuración (64 sectores), leída de 4 en 4 y de 8.
    assert_eq!(b.r(DISCO + R_CONFIG), 64);
    assert_eq!(b.r(DISCO + R_CONFIG + 4), 0);
    assert_eq!(b.m.mmio(0, DISCO + R_CONFIG, 8, Operacion::Leer), SalidaMmio::Leido(64));
    assert_eq!(b.m.mmio(0, DISCO + R_CONFIG + 20, 4, Operacion::Leer), SalidaMmio::Leido(512));
    // Un registro de control leído con un tamaño que no es 4: 0.
    assert_eq!(b.m.mmio(0, CONSOLA + R_MAGIA, 2, Operacion::Leer), SalidaMmio::Leido(0));
    // DRIVER_OK sin FEATURES_OK no se queda.
    b.w(CONSOLA + R_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_DRIVER_OK);
    assert_eq!(b.r(CONSOLA + R_STATUS) & S_DRIVER_OK, 0);
}

#[test]
fn consola_salida_y_entrada() {
    let mut b = banco();
    let mut rx = Cola::en(RAM + 0x1000, 8);
    let mut tx = Cola::en(RAM + 0x2000, 8);
    b.arrancar(CONSOLA, &[rx, tx]);
    // Salida en dos segmentos.
    b.mem().escribir(BUF, b"hola, ").unwrap();
    b.mem().escribir(BUF + 0x100, b"DashOS\n").unwrap();
    let cab = b.poner(&mut tx, &[(BUF, 6, false), (BUF + 0x100, 7, false)]);
    b.w(CONSOLA + R_QUEUE_NOTIFY, 1);
    assert_eq!(b.consola.0.borrow().salida, b"hola, DashOS\n");
    assert_eq!(b.usados(&mut tx), vec![(cab as u32, 0)]);
    assert_eq!(b.r(CONSOLA + R_INTERRUPT_STATUS), INT_USADO);
    assert!(b.pendiente_en_gic(SPI_CONSOLA));
    b.w(CONSOLA + R_INTERRUPT_ACK, INT_USADO);
    assert!(!b.pendiente_en_gic(SPI_CONSOLA));

    // Entrada: búferes vacíos antes, teclas después.
    let c1 = b.poner(&mut rx, &[(BUF + 0x200, 2, true)]);
    let c2 = b.poner(&mut rx, &[(BUF + 0x300, 16, true)]);
    b.w(CONSOLA + R_QUEUE_NOTIFY, 0);
    assert!(b.usados(&mut rx).is_empty());
    b.consola.0.borrow_mut().entrada.extend(b"ls -l\n");
    b.m.sondear();
    assert_eq!(b.usados(&mut rx), vec![(c1 as u32, 2), (c2 as u32, 4)]);
    let mut buf = [0u8; 4];
    b.mem().leer(BUF + 0x300, &mut buf).unwrap();
    assert_eq!(&buf, b" -l\n");
    assert!(b.pendiente_en_gic(SPI_CONSOLA));
    // Sin búferes, la entrada espera.
    b.consola.0.borrow_mut().entrada.extend(b"x");
    b.m.sondear();
    assert!(b.usados(&mut rx).is_empty());
    assert_eq!(b.consola.0.borrow().entrada.len(), 1);
}

#[test]
fn consola_escritura_de_emergencia() {
    let mut b = banco();
    assert!(b.negociar(CONSOLA, F_VERSION_1 | caja_vmm::virtio::consola::F_EMERG_WRITE));
    b.w(CONSOLA + R_CONFIG + 8, b'!' as u32);
    assert_eq!(b.consola.0.borrow().salida, b"!");
    assert_eq!(b.r(CONSOLA + R_CONFIG) & 0xffff, 80);
}

/// Una petición de bloque: cabecera, datos y estado. Devuelve (estado, bytes escritos en el anillo usado).
fn peticion(b: &mut Banco, q: &mut Cola, tipo: u32, sector: u64, datos: Option<(u64, u32, bool)>) -> (u8, u32) {
    let cab = BUF + 0x8000;
    let est = BUF + 0x8100;
    b.mem().escribir_u32(cab, tipo).unwrap();
    b.mem().escribir_u32(cab + 4, 0).unwrap();
    b.mem().escribir_u64(cab + 8, sector).unwrap();
    b.mem().escribir(est, &[0xee]).unwrap();
    let mut segs = vec![(cab, 16, false)];
    segs.extend(datos);
    segs.push((est, 1, true));
    let c = b.poner(q, &segs);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    let u = b.usados(q);
    assert_eq!(u.len(), 1, "una petición, un usado");
    assert_eq!(u[0].0, c as u32);
    let mut s = [0u8];
    b.mem().leer(est, &mut s).unwrap();
    (s[0], u[0].1)
}

#[test]
fn disco_escribir_leer() {
    let mut b = banco();
    let mut q = Cola::en(RAM + 0x1000, 16);
    b.arrancar(DISCO, &[q]);
    let datos: Vec<u8> = (0..1024u32).map(|i| (i * 7) as u8).collect();
    b.mem().escribir(BUF, &datos).unwrap();
    assert_eq!(peticion(&mut b, &mut q, bloque::T_OUT, 3, Some((BUF, 1024, false))), (bloque::S_OK, 1));
    assert_eq!(&b.disco.0.borrow().datos[3 * 512..5 * 512], &datos[..]);
    assert_eq!(peticion(&mut b, &mut q, bloque::T_IN, 3, Some((BUF + 0x1000, 1024, true))), (bloque::S_OK, 1025));
    let mut leido = vec![0u8; 1024];
    b.mem().leer(BUF + 0x1000, &mut leido).unwrap();
    assert_eq!(leido, datos);
    assert_eq!(peticion(&mut b, &mut q, bloque::T_FLUSH, 0, None), (bloque::S_OK, 1));
    let (s, n) = peticion(&mut b, &mut q, bloque::T_GET_ID, 0, Some((BUF + 0x2000, 20, true)));
    assert_eq!((s, n), (bloque::S_OK, 21));
    let mut id = [0u8; 17];
    b.mem().leer(BUF + 0x2000, &mut id).unwrap();
    assert_eq!(&id, b"dashos-caja-linux");
    // Fuera del disco, de largo no múltiplo de 512, con un sector que desborda y de tipo desconocido.
    assert_eq!(peticion(&mut b, &mut q, bloque::T_IN, 63, Some((BUF, 1024, true))).0, bloque::S_IOERR);
    assert_eq!(peticion(&mut b, &mut q, bloque::T_OUT, 0, Some((BUF, 100, false))).0, bloque::S_IOERR);
    assert_eq!(peticion(&mut b, &mut q, bloque::T_IN, u64::MAX, Some((BUF, 512, true))).0, bloque::S_IOERR);
    assert_eq!(peticion(&mut b, &mut q, 99, 0, None).0, bloque::S_UNSUPP);
    // Más de 4 GiB "pedidos" en un descriptor: IOERR sin intentar copiar nada.
    assert_eq!(peticion(&mut b, &mut q, bloque::T_IN, 0, Some((BUF, u32::MAX - 511, true))).0, bloque::S_IOERR);
    assert!(!b.necesita_reinicio(DISCO));
}

#[test]
fn disco_solo_lectura() {
    let mut b = banco_con(true);
    let mut q = Cola::en(RAM + 0x1000, 8);
    let f = b.arrancar(DISCO, &[q]);
    assert!(f & bloque::F_RO != 0);
    assert_eq!(peticion(&mut b, &mut q, bloque::T_OUT, 0, Some((BUF, 512, false))).0, bloque::S_IOERR);
    assert_eq!(peticion(&mut b, &mut q, bloque::T_IN, 0, Some((BUF, 512, true))).0, bloque::S_OK);
}

#[test]
fn disco_con_tabla_indirecta() {
    let mut b = banco();
    let mut q = Cola::en(RAM + 0x1000, 8);
    let f = b.arrancar(DISCO, &[q]);
    assert!(f & F_INDIRECT_DESC != 0);
    b.disco.0.borrow_mut().datos[512..1024].fill(0x5a);
    let tabla = BUF + 0x4000;
    let (cab, est) = (BUF + 0x8000, BUF + 0x8100);
    b.mem().escribir_u32(cab, bloque::T_IN).unwrap();
    b.mem().escribir_u64(cab + 8, 1).unwrap();
    b.escribir_desc(tabla, 0, cab, 16, DESC_F_NEXT, 1);
    b.escribir_desc(tabla, 1, BUF, 512, DESC_F_NEXT | DESC_F_WRITE, 2);
    b.escribir_desc(tabla, 2, est, 1, DESC_F_WRITE, 0);
    b.escribir_desc(q.desc, 0, tabla, 48, DESC_F_INDIRECT, 0);
    b.publicar(&mut q, 0);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    assert_eq!(b.usados(&mut q), vec![(0, 513)]);
    assert_eq!(b.mem().leer_u32(BUF), Ok(0x5a5a_5a5a));
}

/// Prepara un disco listo y devuelve su cola, para los casos hostiles.
fn disco_listo() -> (Banco, Cola) {
    let mut b = banco();
    let q = Cola::en(RAM + 0x1000, 8);
    b.arrancar(DISCO, &[q]);
    (b, q)
}

fn espera_reinicio(b: &mut Banco) {
    assert!(b.necesita_reinicio(DISCO), "tenía que pedir reinicio");
    assert!(b.r(DISCO + R_INTERRUPT_STATUS) & INT_CONFIG != 0);
    // Mientras, no atiende la cola.
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    // Y tras reiniciarlo vuelve a funcionar.
    b.w(DISCO + R_STATUS, 0);
    assert_eq!(b.r(DISCO + R_STATUS), 0);
    assert_eq!(b.r(DISCO + R_INTERRUPT_STATUS), 0);
    let mut q = Cola::en(RAM + 0x3000, 8);
    b.arrancar(DISCO, &[q]);
    assert_eq!(peticion(b, &mut q, bloque::T_FLUSH, 0, None).0, bloque::S_OK);
}

#[test]
fn hostil_descriptor_fuera_de_la_ram() {
    let (mut b, mut q) = disco_listo();
    b.poner(&mut q, &[(0x1000, 16, false), (RAM + TAM_RAM as u64 - 4, 1, true)]);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_cadena_en_bucle() {
    let (mut b, mut q) = disco_listo();
    b.escribir_desc(q.desc, 0, BUF, 16, DESC_F_NEXT, 1);
    b.escribir_desc(q.desc, 1, BUF, 16, DESC_F_NEXT, 0);
    b.publicar(&mut q, 0);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_bucle_en_tabla_indirecta() {
    let (mut b, mut q) = disco_listo();
    let tabla = BUF + 0x4000;
    b.escribir_desc(tabla, 0, BUF, 16, DESC_F_NEXT, 0);
    b.escribir_desc(q.desc, 0, tabla, 16, DESC_F_INDIRECT, 0);
    b.publicar(&mut q, 0);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_indirecta_anidada_o_mala() {
    for (largo, flags_dentro) in [(48u32, DESC_F_INDIRECT), (20, 0), (0, 0), (!15, 0)] {
        let (mut b, mut q) = disco_listo();
        let tabla = BUF + 0x4000;
        b.escribir_desc(tabla, 0, tabla, 48, flags_dentro, 0);
        b.escribir_desc(q.desc, 0, tabla, largo, DESC_F_INDIRECT, 0);
        b.publicar(&mut q, 0);
        b.w(DISCO + R_QUEUE_NOTIFY, 0);
        espera_reinicio(&mut b);
    }
}

#[test]
fn hostil_siguiente_fuera_de_la_tabla() {
    let (mut b, mut q) = disco_listo();
    b.escribir_desc(q.desc, 0, BUF, 16, DESC_F_NEXT, 200);
    b.publicar(&mut q, 0);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_cabeza_fuera_de_la_tabla() {
    let (mut b, mut q) = disco_listo();
    b.publicar(&mut q, 9);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_indice_que_salta() {
    let (mut b, q) = disco_listo();
    b.mem().escribir_u16(q.disp + 2, 9).unwrap(); // 9 pendientes en una cola de 8
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_lectura_tras_escritura() {
    let (mut b, mut q) = disco_listo();
    b.poner(&mut q, &[(BUF, 1, true), (BUF + 16, 16, false)]);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_direccion_que_desborda() {
    let (mut b, mut q) = disco_listo();
    b.poner(&mut q, &[(u64::MAX - 4, 16, false), (BUF, 1, true)]);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_anillos_fuera_de_la_ram() {
    let mut b = banco();
    let q = Cola { desc: 0x10, disp: 0x2000, usado: u64::MAX - 7, ..Cola::en(0, 8) };
    let f = b.ofrecidas(DISCO);
    assert!(b.negociar(DISCO, f));
    assert!(b.preparar_cola(DISCO, 0, &q));
    b.w(DISCO + R_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_FEATURES_OK | S_DRIVER_OK);
    b.w(DISCO + R_QUEUE_NOTIFY, 0);
    espera_reinicio(&mut b);
}

#[test]
fn hostil_cola_imposible() {
    let mut b = banco();
    let f = b.ofrecidas(DISCO);
    assert!(b.negociar(DISCO, f));
    // Tamaño que no es potencia de 2, mayor que el máximo, 0, y anillos desalineados.
    for q in [
        Cola::en(RAM + 0x1000, 6),
        Cola::en(RAM + 0x1000, 512),
        Cola::en(RAM + 0x1000, 0),
        Cola { desc: RAM + 0x1008, ..Cola::en(RAM + 0x1000, 8) },
        Cola { usado: RAM + 0x1802, ..Cola::en(RAM + 0x1000, 8) },
    ] {
        b.w(DISCO + R_QUEUE_SEL, 0);
        b.w(DISCO + R_QUEUE_NUM, q.tam as u32);
        b.w(DISCO + R_QUEUE_DESC_LOW, q.desc as u32);
        b.w(DISCO + R_QUEUE_DEVICE_LOW, q.usado as u32);
        b.w(DISCO + R_QUEUE_DRIVER_LOW, q.disp as u32);
        b.w(DISCO + R_QUEUE_READY, 1);
        assert_eq!(b.r(DISCO + R_QUEUE_READY), 0);
        assert!(b.necesita_reinicio(DISCO));
    }
    // Avisos de colas que no existen o antes de DRIVER_OK: nada.
    b.w(DISCO + R_QUEUE_NOTIFY, 5);
    b.w(DISCO + R_QUEUE_NOTIFY, u32::MAX);
    b.w(DISCO + R_QUEUE_SEL, 77);
    assert_eq!(b.r(DISCO + R_QUEUE_NUM_MAX), 0);
    b.w(DISCO + R_QUEUE_READY, 1);
}

#[test]
fn hostil_consola_con_largo_enorme() {
    let mut b = banco();
    let mut rx = Cola::en(RAM + 0x1000, 8);
    let mut tx = Cola::en(RAM + 0x2000, 8);
    b.arrancar(CONSOLA, &[rx, tx]);
    // Un descriptor de 4 GiB: la consola saca como mucho 64 KiB de una cadena y sigue.
    b.poner(&mut tx, &[(BUF, u32::MAX, false)]);
    b.w(CONSOLA + R_QUEUE_NOTIFY, 1);
    assert_eq!(b.consola.0.borrow().salida.len(), 64 * 1024);
    assert!(b.r(CONSOLA + R_STATUS) & S_DEVICE_NEEDS_RESET == 0);
    // Uno que se sale de la RAM: error controlado.
    b.poner(&mut tx, &[(RAM + TAM_RAM as u64 - 16, 1000, false)]);
    b.w(CONSOLA + R_QUEUE_NOTIFY, 1);
    assert!(b.r(CONSOLA + R_STATUS) & S_DEVICE_NEEDS_RESET != 0);
    // Un búfer de entrada enorme también.
    let mut b = banco();
    b.arrancar(CONSOLA, &[rx, tx]);
    b.poner(&mut rx, &[(RAM + TAM_RAM as u64 - 2, 1000, true)]);
    b.consola.0.borrow_mut().entrada.extend(b"abcdef");
    b.m.sondear();
    assert!(b.r(CONSOLA + R_STATUS) & S_DEVICE_NEEDS_RESET != 0);
}

#[test]
fn pl011_basico() {
    let mut b = banco();
    for c in b"earlycon" {
        b.w(UART + pl011::DR, *c as u32);
    }
    assert_eq!(b.uart.0.borrow().salida, b"earlycon");
    assert_eq!(b.r(UART + pl011::FR) & (pl011::FR_TXFE | pl011::FR_RXFE), pl011::FR_TXFE | pl011::FR_RXFE);
    assert_eq!(b.r(UART + 0xfe0), 0x11);
    assert_eq!(b.r(UART + 0xffc), 0xb1);
    // Entrada con FIFO y RX enmascarada: RT salta con el primer byte.
    b.w(UART + pl011::LCR_H, 1 << 4);
    b.w(UART + pl011::CR, 0x301);
    b.w(UART + pl011::IMSC, pl011::INT_RX | pl011::INT_RT);
    b.uart.0.borrow_mut().entrada.extend(b"ab");
    b.m.sondear();
    assert!(b.pendiente_en_gic(SPI_UART));
    assert_eq!(b.r(UART + pl011::DR), b'a' as u32);
    assert_eq!(b.r(UART + pl011::DR), b'b' as u32);
    assert!(b.r(UART + pl011::FR) & pl011::FR_RXFE != 0);
    assert!(!b.pendiente_en_gic(SPI_UART));
    // TX: salta al escribir, se borra en ICR.
    b.w(UART + pl011::IMSC, pl011::INT_TX);
    b.w(UART + pl011::DR, b'x' as u32);
    assert!(b.pendiente_en_gic(SPI_UART));
    b.w(UART + pl011::ICR, pl011::INT_TX);
    assert!(!b.pendiente_en_gic(SPI_UART));
}

#[test]
fn maquina_direcciones() {
    let mut b = banco();
    assert_eq!(b.m.mmio(0, 0x1234_5678, 4, Operacion::Leer), SalidaMmio::SinAparato);
    assert_eq!(b.m.mmio(0, UART, 3, Operacion::Leer), SalidaMmio::SinAparato);
    assert_eq!(b.m.mmio(0, u64::MAX, 8, Operacion::Escribir(1)), SalidaMmio::SinAparato);
    // Solapes: con el GIC, con otro aparato, con una SPI fuera del distribuidor.
    let p = || Box::new(Pl011::nuevo(Box::new(Extremo::default())));
    assert!(b.m.añadir(GICD + 0x800, 5, p()).is_err());
    assert!(b.m.añadir(CONSOLA + 0x100, 5, p()).is_err());
    assert!(b.m.añadir(0x1000, 500, p()).is_err());
    assert!(b.m.añadir(u64::MAX - 10, 5, p()).is_err());
    assert!(b.m.añadir(0x1000, 5, p()).is_ok());
    assert_eq!(b.m.aparatos().len(), 7);
}

/// Accesos al azar a todo (registros de los aparatos y del GIC, memoria del invitado con colas apuntando a sitios
/// al azar, avisos y sondeos). Lo único que se comprueba es que no hay pánico.
#[test]
fn a_lo_loco() {
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut azar = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for ronda in 0..40 {
        let mut b = banco();
        b.consola.0.borrow_mut().entrada.extend(std::iter::repeat_n(b'z', 300));
        b.uart.0.borrow_mut().entrada.extend(std::iter::repeat_n(b'u', 40));
        // A veces arrancado de verdad, para llegar al código de las colas.
        if ronda % 2 == 0 {
            let q = Cola::en(RAM + 0x1000, 16);
            b.arrancar(DISCO, &[q]);
            b.arrancar(CONSOLA, &[Cola::en(RAM + 0x2000, 16), Cola::en(RAM + 0x3000, 16)]);
            b.arrancar(RED, &[Cola::en(RAM + 0x2800, 8), Cola::en(RAM + 0x3800, 8)]);
            b.arrancar(VSOCK, &[Cola::en(RAM + 0x1800, 8), Cola::en(RAM + 0x3c00, 8), Cola::en(RAM + 0x2c00, 4)]);
            b.arrancar(GLOBO, &[Cola::en(RAM + 0x3400, 8), Cola::en(RAM + 0x2400, 8)]);
            b.enchufes.0.borrow_mut().salen.insert(1, VecDeque::from(vec![7u8; 3000]));
            b.cable.0.borrow_mut().entran.extend((0..50).map(|i| vec![i as u8; i * 40]));
        }
        for _ in 0..20_000 {
            let r = azar();
            let base = [UART, CONSOLA, DISCO, GICD, RAM, RED, VSOCK, GLOBO][(r % 8) as usize];
            let desp = if r & 0x100 != 0 { (r >> 16) & 0x1fc } else { (r >> 16) & 0xfff };
            let tam = [1u8, 2, 4, 8, 3][((r >> 8) % 5) as usize];
            let v = azar();
            match (r >> 12) % 8 {
                0..=2 => {
                    b.m.mmio((r >> 40) as usize % 3, base + desp, tam, Operacion::Escribir(v));
                }
                3 | 4 => {
                    b.m.mmio((r >> 40) as usize % 3, base + desp, tam, Operacion::Leer);
                }
                5 => {
                    // Basura en las colas: descriptores, anillos e índices.
                    let dir = RAM + 0x1000 + (v % 0x3000);
                    let _ = b.m.mem.escribir_u64(dir & !7, azar() & 0x0000_0000_407f_ffff | (azar() & 0xf000));
                }
                6 => {
                    let base = [DISCO, CONSOLA, RED, VSOCK, GLOBO][(r % 5) as usize];
                    b.m.mmio(0, base + R_QUEUE_NOTIFY, 4, Operacion::Escribir(v % 3));
                }
                _ => {
                    b.globo.0.borrow_mut().objetivo = (v >> 20) as u32;
                    b.m.sondear()
                }
            }
            for c in 0..2 {
                for e in b.m.gic.pendientes(c) {
                    b.m.gic.cargada(c, &e);
                    b.m.gic.terminada(c, e.intid);
                }
            }
        }
    }
}

#[test]
fn red_enviar_y_recibir() {
    let mut b = banco();
    let mut rx = Cola::en(RAM + 0x1000, 8);
    let mut tx = Cola::en(RAM + 0x2000, 8);
    let f = b.arrancar(RED, &[rx, tx]);
    assert!(f & red::F_MAC != 0);
    assert_eq!(b.r(RED + R_CONFIG), 0x1200_5452);
    assert_eq!(b.m.mmio(0, RED + R_CONFIG + 6, 2, Operacion::Leer), SalidaMmio::Leido(1)); // enlace arriba
    // Enviar: cabecera de 12 bytes y trama, en dos segmentos.
    b.mem().escribir(BUF, &[0; 12]).unwrap();
    b.mem().escribir(BUF + 0x100, &[0xab; 60]).unwrap();
    b.poner(&mut tx, &[(BUF, 12, false), (BUF + 0x100, 60, false)]);
    // Una sin cabecera completa y otra enorme: se descartan sin romper nada.
    b.poner(&mut tx, &[(BUF, 8, false)]);
    b.poner(&mut tx, &[(BUF, 12, false), (BUF + 0x1000, 9000, false)]);
    b.w(RED + R_QUEUE_NOTIFY, 1);
    assert_eq!(b.cable.0.borrow().salieron, vec![vec![0xab; 60]]);
    assert_eq!(b.usados(&mut tx).len(), 3);
    assert!(!b.necesita_reinicio(RED));
    // Recibir: una que cabe y otra que no cabe en su búfer.
    let c1 = b.poner(&mut rx, &[(BUF + 0x2000, 1526, true)]);
    let c2 = b.poner(&mut rx, &[(BUF + 0x3000, 100, true)]);
    b.cable.0.borrow_mut().entran.extend([vec![0x11; 64], vec![0x22; 200]]);
    b.m.sondear();
    assert_eq!(b.usados(&mut rx), vec![(c1 as u32, 76), (c2 as u32, 0)]);
    assert_eq!(b.mem().leer_u16(BUF + 0x2000 + 10), Ok(1)); // num_buffers
    assert_eq!(b.mem().leer_u32(BUF + 0x2000 + 12), Ok(0x1111_1111));
    assert!(b.pendiente_en_gic(SPI_RED));
}

/// Una cabecera de vsock del invitado hacia DashOS.
fn cab(op: u16, su: u32, mio: u32, len: u32) -> Cabecera {
    Cabecera {
        src_cid: vsock::CID_INVITADO,
        dst_cid: vsock::CID_ANFITRION,
        src_port: su,
        dst_port: mio,
        len,
        tipo: vsock::TIPO_STREAM,
        op,
        flags: 0,
        buf_alloc: 64 * 1024,
        fwd_cnt: 0,
    }
}

struct Vs {
    rx: Cola,
    tx: Cola,
    /// Siguiente sitio libre para búferes.
    libre: u64,
    /// Cabeza de cada búfer de recibir → su dirección.
    buferes: std::collections::BTreeMap<u16, u64>,
}

impl Banco {
    fn vsock(&mut self) -> Vs {
        let rx = Cola::en(RAM + 0x1000, 64);
        let tx = Cola::en(RAM + 0x2000, 64);
        let ev = Cola::en(RAM + 0x3000, 4);
        let f = self.arrancar(VSOCK, &[rx, tx, ev]);
        assert!(f & F_VERSION_1 != 0);
        assert_eq!(self.r(VSOCK + R_DEVICE_ID), vsock::ID_VSOCK);
        assert_eq!(self.r(VSOCK + R_CONFIG), 3, "el CID del invitado");
        Vs { rx, tx, libre: BUF, buferes: Default::default() }
    }

    /// Manda un paquete (cabecera y datos en dos segmentos) y avisa.
    fn vs_mandar(&mut self, v: &mut Vs, h: &Cabecera, datos: &[u8]) {
        let (dc, dd) = (v.libre, v.libre + 0x40);
        v.libre += 0x40 + (datos.len() as u64).next_multiple_of(0x40).max(0x40);
        self.mem().escribir(dc, &h.bytes()).unwrap();
        self.mem().escribir(dd, datos).unwrap();
        let mut tx = v.tx;
        if datos.is_empty() {
            self.poner(&mut tx, &[(dc, 44, false)]);
        } else {
            self.poner(&mut tx, &[(dc, 44, false), (dd, datos.len() as u32, false)]);
        }
        v.tx = tx;
        self.w(VSOCK + R_QUEUE_NOTIFY, 1);
    }

    /// Pone `n` búferes de recibir de `tam` bytes.
    fn vs_buferes(&mut self, v: &mut Vs, n: usize, tam: u32) -> Vec<(u16, u64)> {
        let mut rx = v.rx;
        let mut out = Vec::new();
        for _ in 0..n {
            let d = v.libre;
            v.libre += tam as u64;
            let c = self.poner(&mut rx, &[(d, tam, true)]);
            v.buferes.insert(c, d);
            out.push((c, d));
        }
        v.rx = rx;
        self.w(VSOCK + R_QUEUE_NOTIFY, 0);
        out
    }

    /// Lo que llegó al invitado: (cabecera, datos), en orden.
    fn vs_recibido(&mut self, v: &mut Vs, _buferes: &[(u16, u64)]) -> Vec<(Cabecera, Vec<u8>)> {
        let mut rx = v.rx;
        let usados = self.usados(&mut rx);
        v.rx = rx;
        usados
            .into_iter()
            .map(|(c, n)| {
                let d = v.buferes[&(c as u16)];
                let mut h = [0u8; 44];
                self.mem().leer(d, &mut h).unwrap();
                let mut datos = vec![0u8; (n as usize).saturating_sub(44)];
                self.mem().leer(d + 44, &mut datos).unwrap();
                (Cabecera::leer(&h), datos)
            })
            .collect()
    }
}

/// vsock de punta a punta: conectar, datos en los dos sentidos, crédito, cierre por el invitado y por DashOS, y un
/// puerto sin nadie.
#[test]
fn vsock_conexion() {
    let mut b = banco();
    let mut v = b.vsock();
    let bufs = b.vs_buferes(&mut v, 8, 4096);
    // Un puerto en el que no escucha nadie: RST.
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 1024, 7, 0), &[]);
    let r = b.vs_recibido(&mut v, &bufs);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].0.op, r[0].0.dst_port, r[0].0.src_port), (vsock::OP_RST, 1024, 7));
    // El 5000: RESPONSE, con el crédito de DashOS.
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 1025, 5000, 0), &[]);
    let r = b.vs_recibido(&mut v, &bufs);
    assert_eq!(r[0].0.op, vsock::OP_RESPONSE);
    assert_eq!((r[0].0.src_cid, r[0].0.dst_cid), (2, 3));
    assert_eq!(r[0].0.buf_alloc, vsock::BUF_ALLOC);
    assert!(b.pendiente_en_gic(SPI_VSOCK));
    // Datos del invitado (la conexión es la 2: el número 1 fue el intento al puerto 7).
    b.vs_mandar(&mut v, &cab(vsock::OP_RW, 1025, 5000, 5), b"hola!");
    assert_eq!(b.enchufes.0.borrow().llegado[&2], b"hola!");
    // Con tan poco, no hace falta decirle el crédito: no le llega nada.
    assert!(b.vs_recibido(&mut v, &bufs).is_empty());
    // Con 70 000 bytes más (en paquetes de 10 000), sí: lo que se liberó.
    for _ in 0..7 {
        b.vs_mandar(&mut v, &cab(vsock::OP_RW, 1025, 5000, 10_000), &[3u8; 10_000]);
    }
    let r = b.vs_recibido(&mut v, &bufs);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].0.op, r[0].0.fwd_cnt), (vsock::OP_CREDIT_UPDATE, 70_005));
    v.libre = BUF + 0x2_0000;
    // Datos de DashOS: 10 000 bytes en búferes de 4096 (con cabecera, 4052 por paquete).
    b.enchufes.0.borrow_mut().salen.get_mut(&2).unwrap().extend((0..10_000).map(|i| i as u8));
    b.m.sondear();
    let r = b.vs_recibido(&mut v, &bufs);
    let todo: Vec<u8> = r.iter().flat_map(|(h, d)| {
        assert_eq!((h.op, h.len as usize), (vsock::OP_RW, d.len()));
        d.clone()
    }).collect();
    assert_eq!(todo, (0..10_000).map(|i| i as u8).collect::<Vec<_>>());
    // Crédito del invitado: dijo 64 KB. Con 70 000 bytes, pasan 64 KB y el resto espera a su CREDIT_UPDATE.
    let bufs = b.vs_buferes(&mut v, 30, 4096);
    b.enchufes.0.borrow_mut().salen.get_mut(&2).unwrap().extend(std::iter::repeat_n(1u8, 70_000));
    b.m.sondear();
    let n: usize = b.vs_recibido(&mut v, &bufs).iter().map(|(_, d)| d.len()).sum();
    assert_eq!(n, 64 * 1024 - 10_000);
    let mut h = cab(vsock::OP_CREDIT_UPDATE, 1025, 5000, 0);
    h.fwd_cnt = 64 * 1024;
    b.vs_mandar(&mut v, &h, &[]);
    let n: usize = b.vs_recibido(&mut v, &bufs).iter().map(|(_, d)| d.len()).sum();
    assert_eq!(n, 70_000 - (64 * 1024 - 10_000));
    // El invitado cierra: el extremo se entera y le llega RST.
    let bufs = b.vs_buferes(&mut v, 4, 4096);
    b.vs_mandar(&mut v, &cab(vsock::OP_SHUTDOWN, 1025, 5000, 0), &[]);
    assert_eq!(b.enchufes.0.borrow().cerradas, vec![2]);
    assert_eq!(b.vs_recibido(&mut v, &bufs)[0].0.op, vsock::OP_RST);
    // Otra conexión, que cierra DashOS.
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 1026, 5000, 0), &[]);
    b.enchufes.0.borrow_mut().cerrar.push(3);
    b.m.sondear();
    let r = b.vs_recibido(&mut v, &bufs);
    assert_eq!(r.iter().map(|x| x.0.op).collect::<Vec<_>>(), vec![vsock::OP_RESPONSE, vsock::OP_RST]);
    assert!(!b.necesita_reinicio(VSOCK));
}

/// El extremo toma poco a poco: lo que no toma espera en el aparato (sin perderse) y, cuando el invitado ya casi no
/// tiene crédito, se le anuncia el que se libera.
#[test]
fn vsock_extremo_lento() {
    let mut b = banco();
    let mut v = b.vsock();
    let bufs = b.vs_buferes(&mut v, 16, 4096);
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 1025, 5000, 0), &[]);
    assert_eq!(b.vs_recibido(&mut v, &bufs)[0].0.op, vsock::OP_RESPONSE);
    b.enchufes.0.borrow_mut().toma = Some(0);
    // Casi todo su crédito (250 000 de 262 144), sin que el extremo tome nada.
    for i in 0..5u8 {
        b.vs_mandar(&mut v, &cab(vsock::OP_RW, 1025, 5000, 50_000), &[i; 50_000]);
        v.libre = BUF + 0x1_0000;
    }
    assert!(b.enchufes.0.borrow().llegado[&1].is_empty());
    assert!(b.vs_recibido(&mut v, &bufs).is_empty());
    // Toma de 1000 en 1000 (el aparato le da todo lo que espera, a trozos): el invitado sabe que tiene sitio.
    b.enchufes.0.borrow_mut().toma = Some(1000);
    b.m.sondear();
    let r = b.vs_recibido(&mut v, &bufs);
    assert_eq!((r[0].0.op, r[0].0.fwd_cnt), (vsock::OP_CREDIT_UPDATE, 250_000));
    let llegado = b.enchufes.0.borrow().llegado[&1].clone();
    assert_eq!(llegado.len(), 250_000);
    assert!(llegado.chunks(50_000).enumerate().all(|(i, c)| c.iter().all(|&x| x == i as u8)), "en orden");
}

/// Hostiles: CID falso, tipo que no es de flujo, cabecera corta, largo mayor que lo que trae o que un paquete, datos
/// sin conexión, pasarse del crédito, órdenes que no existen, conexiones de más y un búfer de recibir donde no cabe la
/// cabecera. El aparato sigue (sin DEVICE_NEEDS_RESET) y una conexión buena funciona después.
#[test]
fn vsock_hostil() {
    let mut b = banco();
    let mut v = b.vsock();
    let bufs = b.vs_buferes(&mut v, 60, 128);
    let mut h = cab(vsock::OP_REQUEST, 1, 5000, 0);
    h.src_cid = 2; // se hace pasar por DashOS
    b.vs_mandar(&mut v, &h, &[]);
    let mut h = cab(vsock::OP_REQUEST, 1, 5000, 0);
    h.tipo = 2; // seqpacket, no negociado
    b.vs_mandar(&mut v, &h, &[]);
    // Cabecera corta.
    let mut tx = v.tx;
    b.poner(&mut tx, &[(BUF, 20, false)]);
    v.tx = tx;
    b.w(VSOCK + R_QUEUE_NOTIFY, 1);
    assert_eq!(b.enchufes.0.borrow().llegado.len(), 0);
    // Una conexión que se porta mal de varias formas.
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 2, 5000, 0), &[]);
    b.vs_mandar(&mut v, &cab(vsock::OP_RW, 2, 5000, 1000), &[1; 10]); // dice 1000 y trae 10
    assert!(b.enchufes.0.borrow().cerradas.contains(&1));
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 3, 5000, 0), &[]);
    b.vs_mandar(&mut v, &cab(vsock::OP_RW, 3, 5000, u32::MAX), &[]); // largo absurdo
    assert!(b.enchufes.0.borrow().cerradas.contains(&2));
    // Datos sin conexión, y una orden que no existe.
    b.vs_mandar(&mut v, &cab(vsock::OP_RW, 77, 5000, 4), b"nada");
    b.vs_mandar(&mut v, &cab(99, 78, 5000, 0), &[]);
    // Pasarse del crédito: el extremo no toma nada y el invitado manda más de BUF_ALLOC.
    b.enchufes.0.borrow_mut().toma = Some(0);
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 4, 5000, 0), &[]);
    let trozo = vec![5u8; 60_000];
    for _ in 0..5 {
        b.vs_mandar(&mut v, &cab(vsock::OP_RW, 4, 5000, trozo.len() as u32), &trozo);
    }
    assert!(b.enchufes.0.borrow().cerradas.contains(&3), "pasarse del crédito corta la conexión");
    b.enchufes.0.borrow_mut().toma = None;
    // Pedir dos veces la misma conexión la corta.
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 5, 5000, 0), &[]);
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 5, 5000, 0), &[]);
    assert!(b.enchufes.0.borrow().cerradas.contains(&4));
    // Un búfer de recibir donde no cabe la cabecera: vuelve vacío.
    let mut rx = v.rx;
    b.poner(&mut rx, &[(BUF + 0x8_0000, 10, true)]);
    v.rx = rx;
    b.w(VSOCK + R_QUEUE_NOTIFY, 0);
    let _ = b.vs_recibido(&mut v, &bufs);
    assert!(!b.necesita_reinicio(VSOCK));
    // Más conexiones de las que caben: las de más, RST.
    for p in 100..100 + vsock::MAX_CONEXIONES as u32 + 5 {
        b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, p, 5000, 0), &[]);
        if p % 8 == 0 {
            v.libre = BUF + 0x10_0000; // reusar memoria para no salirse de la RAM
        }
    }
    {
        let e = b.enchufes.0.borrow();
        assert_eq!(e.llegado.len() - e.cerradas.len(), vsock::MAX_CONEXIONES);
    }
    // Tras todo, una buena: sus datos llegan.
    for p in 100..100 + vsock::MAX_CONEXIONES as u32 {
        b.vs_mandar(&mut v, &cab(vsock::OP_RST, p, 5000, 0), &[]);
    }
    b.vs_mandar(&mut v, &cab(vsock::OP_REQUEST, 9, 5000, 0), &[]);
    b.vs_mandar(&mut v, &cab(vsock::OP_RW, 9, 5000, 4), b"bien");
    let ultima = *b.enchufes.0.borrow().llegado.keys().last().unwrap();
    assert_eq!(b.enchufes.0.borrow().llegado[&ultima], b"bien");
    assert!(!b.necesita_reinicio(VSOCK));
}

/// Escribe una lista de números de página (le32) en la RAM del invitado.
fn lista_de_paginas(b: &mut Banco, dir: u64, pfns: &[u32]) {
    for (i, &p) in pfns.iter().enumerate() {
        b.mem().escribir_u32(dir + 4 * i as u64, p).unwrap();
    }
}

fn globo_listo() -> (Banco, Cola, Cola) {
    let mut b = banco();
    let inflar = Cola::en(RAM + 0x1_0000, 8);
    let desinflar = Cola::en(RAM + 0x2_0000, 8);
    let f = b.arrancar(GLOBO, &[inflar, desinflar]);
    assert_eq!(f & !F_VERSION_1, globo::F_MUST_TELL_HOST | globo::F_DEFLATE_ON_OOM);
    (b, inflar, desinflar)
}

#[test]
fn globo_pide_mete_y_saca() {
    let (mut b, mut inflar, mut desinflar) = globo_listo();
    assert_eq!(b.r(GLOBO + R_DEVICE_ID), globo::ID_GLOBO);
    assert_eq!(b.r(GLOBO + R_CONFIG), 0, "al empezar no pide nada");
    // DashOS pide 300 páginas: al sondear, la configuración cambia y el invitado se entera por la interrupción.
    let generacion = b.r(GLOBO + R_CONFIG_GENERATION);
    b.globo.0.borrow_mut().objetivo = 300;
    b.m.sondear();
    assert_eq!(b.r(GLOBO + R_CONFIG), 300);
    assert_ne!(b.r(GLOBO + R_CONFIG_GENERATION), generacion);
    assert_eq!(b.r(GLOBO + R_INTERRUPT_STATUS) & INT_CONFIG, INT_CONFIG);
    assert!(b.pendiente_en_gic(SPI_GLOBO));
    b.w(GLOBO + R_INTERRUPT_ACK, INT_CONFIG);
    // Sondear otra vez sin cambios no avisa.
    b.m.sondear();
    assert_eq!(b.r(GLOBO + R_INTERRUPT_STATUS), 0);
    // El invitado mete tres páginas y lo dice en `actual`; `num_pages` no lo puede tocar.
    lista_de_paginas(&mut b, BUF, &[0x40100, 0x40101, 0x40200]);
    let c = b.poner(&mut inflar, &[(BUF, 12, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    assert_eq!(b.usados(&mut inflar), vec![(c as u32, 0)]);
    assert_eq!(b.globo.0.borrow().inflado, vec![0x4010_0000, 0x4010_1000, 0x4020_0000]);
    b.w(GLOBO + R_CONFIG + 4, 3);
    b.w(GLOBO + R_CONFIG, 7);
    assert_eq!(b.r(GLOBO + R_CONFIG), 300);
    assert_eq!(b.r(GLOBO + R_CONFIG + 4), 3);
    assert_eq!(b.globo.0.borrow().actual, vec![3]);
    // Y saca una: el aparato contesta después de pasarla al otro lado.
    lista_de_paginas(&mut b, BUF + 0x100, &[0x40101]);
    let c = b.poner(&mut desinflar, &[(BUF + 0x100, 4, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 1);
    assert_eq!(b.usados(&mut desinflar), vec![(c as u32, 0)]);
    assert_eq!(b.globo.0.borrow().desinflado, vec![0x4010_1000]);
    // Reiniciar el aparato: el otro lado tiene que devolver todo.
    let antes = b.globo.0.borrow().reiniciado;
    b.w(GLOBO + R_STATUS, 0);
    assert_eq!(b.globo.0.borrow().reiniciado, antes + 1);
}

#[test]
fn globo_hostil() {
    let (mut b, mut inflar, _) = globo_listo();
    // Un búfer de largo raro: lo que no completa un número se tira.
    lista_de_paginas(&mut b, BUF, &[0x40100, 0x40101]);
    b.poner(&mut inflar, &[(BUF, 7, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    assert_eq!(b.globo.0.borrow().inflado, vec![0x4010_0000]);
    // Un búfer de escritura no se lee.
    b.poner(&mut inflar, &[(BUF, 8, true)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    assert_eq!(b.globo.0.borrow().inflado.len(), 1);
    // Una lista enorme (1 MB, de la RAM): solo se leen los primeros 64 KB, por lotes de 256.
    b.poner(&mut inflar, &[(RAM, 1 << 20, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    assert_eq!(b.globo.0.borrow().inflado.len(), 1 + 16384);
    // Números de página de cualquier cosa: pasan tal cual (los mira el otro lado).
    lista_de_paginas(&mut b, BUF, &[u32::MAX, 0]);
    b.poner(&mut inflar, &[(BUF, 8, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    let v = b.globo.0.borrow().inflado.clone();
    assert_eq!(&v[v.len() - 2..], &[u32::MAX as u64 * 4096, 0]);
    // Una lista fuera de la RAM: el aparato pide reinicio y vuelve a funcionar tras él.
    b.poner(&mut inflar, &[(0x1_0000_0000, 64, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    assert!(b.necesita_reinicio(GLOBO));
    let mut inflar = Cola::en(RAM + 0x3_0000, 8);
    b.arrancar(GLOBO, &[inflar, Cola::en(RAM + 0x4_0000, 8)]);
    assert!(!b.necesita_reinicio(GLOBO));
    lista_de_paginas(&mut b, BUF, &[0x40300]);
    b.poner(&mut inflar, &[(BUF, 4, false)]);
    b.w(GLOBO + R_QUEUE_NOTIFY, 0);
    assert_eq!(*b.globo.0.borrow().inflado.last().unwrap(), 0x4030_0000);
    // Escribir la configuración de cualquier forma no rompe nada, y `num_pages` sigue siendo del aparato.
    for desp in 0..12u64 {
        for tam in [1u8, 2, 4, 8] {
            let _ = b.m.mmio(0, GLOBO + R_CONFIG + desp, tam, Operacion::Escribir(u64::MAX));
        }
    }
    assert_eq!(b.r(GLOBO + R_CONFIG), 0);
}
