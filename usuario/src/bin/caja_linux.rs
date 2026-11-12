//! La caja Linux (H9, `docs/bitacoras/caja-linux.md`), primer paso: el servicio que crea y corre máquinas virtuales.
//!
//! Hoy corre los invitados de prueba de `invitados/pruebas.s` (unas instrucciones AArch64 sin sistema) y emula para
//! ellos un PL011 en 0x09000000: lo que escriben sale en la consola. Mide cuánto cuesta entrar y salir del invitado y
//! prueba los hostiles: leer y escribir fuera de su memoria, un bucle sin fin, apagar la máquina por SMC y tocar el
//! temporizador del kernel. Cada máquina que hace algo prohibido se para (se cierra su llave) y el sistema sigue.
//!
//! Llave 1: el permiso de máquinas virtuales, que le da `inicio`.

#![no_std]
#![no_main]

use dashos::maquina::{self as m, Informe};
use dashos::println;

dashos::programa!(principal);

core::arch::global_asm!(include_str!("../../invitados/pruebas.s"));

unsafe extern "C" {
    static invitado_hola: u8;
    static invitado_hola_fin: u8;
    static invitado_eco: u8;
    static invitado_eco_fin: u8;
    static invitado_lee_fuera: u8;
    static invitado_lee_fuera_fin: u8;
    static invitado_escribe_fuera: u8;
    static invitado_escribe_fuera_fin: u8;
    static invitado_bucle: u8;
    static invitado_bucle_fin: u8;
    static invitado_apaga: u8;
    static invitado_apaga_fin: u8;
    static invitado_reloj: u8;
    static invitado_reloj_fin: u8;
}

const PERMISO: u32 = 1;
/// Dónde ve el invitado su RAM, y cuánta tiene.
const RAM_IPA: u64 = 0x4000_0000;
const RAM: usize = 64 * 1024;
/// Dónde la proyecta el servicio para cargar el invitado.
const VA: usize = 0x1000_0000;
/// El PL011 emulado: el registro de datos y el de estado.
const UART: u64 = 0x0900_0000;
const UART_FR: u64 = UART + 0x18;

/// Los bytes de un invitado, entre su etiqueta y la de su fin.
fn codigo(inicio: &'static u8, fin: &'static u8) -> &'static [u8] {
    let (i, f) = (inicio as *const u8 as usize, fin as *const u8 as usize);
    // SAFETY: las dos etiquetas son del mismo trozo de .rodata (pruebas.s), la de fin después.
    unsafe { core::slice::from_raw_parts(inicio as *const u8, f - i) }
}

/// Una máquina con `codigo` al principio de su RAM: la llave, o el error de `maquina_crear`.
fn maquina(codigo: &[u8]) -> Result<u32, i64> {
    let mem = dashos::memoria_crear(RAM) as i32;
    if mem <= 0 {
        return Err(mem as i64);
    }
    let mem = mem as u32;
    if dashos::memoria_proyectar(mem, VA, true) != 0 {
        dashos::cerrar(mem);
        return Err(-1);
    }
    // SAFETY: recién proyectada aquí, RAM bytes, y el invitado aún no existe.
    let ram = unsafe { core::slice::from_raw_parts_mut(VA as *mut u8, RAM) };
    ram[..codigo.len()].copy_from_slice(codigo);
    let r = dashos::maquina_crear(PERMISO, mem, RAM_IPA, RAM_IPA, 0);
    // La máquina sujeta su memoria: el servicio ya no la necesita.
    dashos::memoria_quitar(VA);
    dashos::cerrar(mem);
    if r > 0 { Ok(r as u32) } else { Err(r) }
}

/// Cómo acabó una máquina.
enum Fin {
    /// HVC #n con x0.
    Hvc(u64, u64),
    /// Un acceso a una dirección que no es suya ni del UART: (dirección, si escribía).
    Fuera(u64, bool),
    /// Seguía en marcha al pasar el plazo: cuántas veces la sacó el reloj.
    Bucle(u64),
    /// Tocó un registro del sistema atrapado: (su codificación, si escribía).
    Registro(u64, bool),
    /// Ya escribió todas las líneas que se le dejan.
    Lineas,
    /// Una trampa o un fallo: ESR y PC.
    Fallo(u64, u64),
    /// La llamada falló.
    Error(i64),
}

/// Cómo se corre una máquina.
struct Opciones<'a> {
    /// Lo que va delante de cada línea que escribe por el UART.
    prefijo: &'a str,
    /// Cuántas líneas se le dejan escribir.
    lineas: usize,
    /// Los registros atrapados (depuración, contadores) se leen como 0 y lo escrito se ignora, como hace falta para un
    /// kernel de Linux; si no, la máquina se para.
    registros: bool,
    plazo_us: u64,
}

/// Corre la máquina hasta que acaba o pasa el plazo, emulando el UART: cada línea que escribe sale en la consola.
fn correr(vm: u32, o: &Opciones) -> Fin {
    let mut inf = Informe::default();
    let mut linea = [0u8; 160];
    let mut n = 0;
    let mut lineas = 0;
    let mut valor = 0;
    let mut sacado = 0;
    let t0 = dashos::ahora_us();
    loop {
        let r = dashos::maquina_correr(vm, &mut inf, valor);
        valor = 0;
        match r {
            m::HVC => return Fin::Hvc(inf.tamano, inf.valor),
            m::ESCRITURA if inf.direccion == UART => {
                let c = inf.valor as u8;
                if c == b'\n' || n == linea.len() {
                    println!("  [caja-linux] {}{}", o.prefijo, core::str::from_utf8(&linea[..n]).unwrap_or("?"));
                    n = 0;
                    lineas += 1;
                    if lineas >= o.lineas {
                        return Fin::Lineas;
                    }
                }
                if c != b'\n' && c != b'\r' {
                    linea[n] = c;
                    n += 1;
                }
            }
            // FR: nunca lleno.
            m::LECTURA if inf.direccion == UART_FR => valor = 0,
            m::LECTURA => return Fin::Fuera(inf.direccion, false),
            m::ESCRITURA => return Fin::Fuera(inf.direccion, true),
            m::REGISTRO if o.registros => valor = 0,
            m::REGISTRO => return Fin::Registro(inf.direccion, inf.x1 == 0),
            m::WFI | m::INTERRUMPIDO => {
                if r == m::INTERRUMPIDO {
                    sacado += 1;
                }
                if dashos::ahora_us() - t0 > o.plazo_us {
                    return Fin::Bucle(sacado);
                }
            }
            m::FALLO => return Fin::Fallo(inf.esr, inf.pc),
            e => return Fin::Error(e),
        }
    }
}

/// Lo de siempre para los invitados de prueba.
fn prueba(plazo_us: u64) -> Opciones<'static> {
    Opciones { prefijo: "el invitado dice: ", lineas: 20, registros: false, plazo_us }
}

fn frecuencia() -> u64 {
    let f: u64;
    // SAFETY: CNTFRQ_EL0 se lee desde EL0.
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f.max(1)
}

/// Un invitado hostil: tiene que acabar como dice `esperado`, y entonces se para su máquina (se cierra su llave).
fn hostil(nombre: &str, codigo: &[u8], esperado: impl Fn(&Fin) -> bool) -> bool {
    let vm = match maquina(codigo) {
        Ok(v) => v,
        Err(e) => {
            println!("  [caja-linux] hostil ({}): no se pudo crear la máquina ({}); MAL", nombre, e);
            return false;
        }
    };
    let fin = correr(vm, &prueba(100_000));
    let bien = esperado(&fin);
    let que = match fin {
        Fin::Fuera(d, true) => texto(format_args!("escribe fuera de su memoria ({:#x})", d)),
        Fin::Fuera(d, false) => texto(format_args!("lee fuera de su memoria ({:#x})", d)),
        Fin::Bucle(n) => texto(format_args!("no acaba: el reloj lo sacó {} veces en 100 ms", n)),
        Fin::Registro(c, escribe) => texto(format_args!("{} un registro del sistema atrapado (codificación {:#x})", if escribe { "escribe en" } else { "lee" }, c)),
        Fin::Lineas => texto(format_args!("escribe demasiado")),
        Fin::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x}, clase {:#x})", pc, esr, esr >> 26)),
        Fin::Hvc(n, x0) => texto(format_args!("acaba con HVC #{} (x0 = {})", n, x0)),
        Fin::Error(e) => texto(format_args!("error {}", e)),
    };
    // Una máquina muerta en un fallo no vuelve a correr.
    let otra = dashos::maquina_correr(vm, &mut Informe::default(), 0);
    dashos::cerrar(vm);
    let muerta = !matches!(fin, Fin::Fallo(..)) || otra == dashos::E_CERRADO;
    println!("  [caja-linux] hostil ({}): {}; se para su máquina: {}", nombre, que.as_str(), if bien && muerta { "bien" } else { "MAL" });
    bien && muerta
}

/// Un texto corto con formato, sin montón.
struct Texto {
    b: [u8; 160],
    n: usize,
}

impl Texto {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("?")
    }
}

impl core::fmt::Write for Texto {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let k = s.len().min(self.b.len() - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s.as_bytes()[..k]);
        self.n += k;
        Ok(())
    }
}

fn texto(a: core::fmt::Arguments) -> Texto {
    let mut t = Texto { b: [0; 160], n: 0 };
    let _ = core::fmt::Write::write_fmt(&mut t, a);
    t
}

// --- Segundo paso: un kernel de Linux de verdad ---

/// Llave 2, si la hay (`make arranque-virt-linux`): la Image de un kernel de Linux para arm64, solo de lectura.
const LINUX: u32 = 2;
const VA_IMAGEN: usize = 0x2000_0000;
/// La RAM del invitado de Linux: 64 MB, en objetos de 16 MB (el tope de `memoria_crear`), proyectados seguidos aquí.
const VA_RAM_LINUX: usize = 0x4000_0000;
const TROZO: usize = 16 << 20;
const TROZOS: usize = 4;
/// Dónde va la Image en la RAM (alineada a 2 MB, como pide el protocolo de arranque de arm64); el árbol, al principio.
const IMAGEN_EN: usize = 2 << 20;
const BOOTARGS: &str = "earlycon=pl011,0x09000000 console=ttyAMA0 nokaslr panic=-1";

/// Un árbol de dispositivos (FDT, versión 17) mínimo para el invitado: la memoria, una CPU y la línea de órdenes con
/// la consola temprana en el PL011 emulado. Sin GIC ni temporizador: basta para las primeras líneas. Escrito con la
/// especificación de devicetree. Devuelve cuánto mide.
fn arbol(sal: &mut [u8], ram: u64) -> usize {
    struct A<'a> {
        e: [u8; 1024],
        ne: usize,
        c: [u8; 256],
        nc: usize,
        _s: core::marker::PhantomData<&'a ()>,
    }
    impl A<'_> {
        fn u32(&mut self, v: u32) {
            self.e[self.ne..self.ne + 4].copy_from_slice(&v.to_be_bytes());
            self.ne += 4;
        }
        fn bytes(&mut self, b: &[u8]) {
            self.e[self.ne..self.ne + b.len()].copy_from_slice(b);
            self.ne += b.len();
            while self.ne % 4 != 0 {
                self.e[self.ne] = 0;
                self.ne += 1;
            }
        }
        fn nodo(&mut self, nombre: &str) {
            self.u32(1);
            let mut n = [0u8; 32];
            n[..nombre.len()].copy_from_slice(nombre.as_bytes());
            self.bytes(&n[..nombre.len() + 1]);
        }
        fn fin(&mut self) {
            self.u32(2);
        }
        fn cadena(&mut self, nombre: &str) -> u32 {
            let mut i = 0;
            while i < self.nc {
                let fin = i + self.c[i..self.nc].iter().position(|&b| b == 0).unwrap_or(0);
                if &self.c[i..fin] == nombre.as_bytes() {
                    return i as u32;
                }
                i = fin + 1;
            }
            let o = self.nc;
            self.c[o..o + nombre.len()].copy_from_slice(nombre.as_bytes());
            self.nc += nombre.len() + 1;
            o as u32
        }
        fn prop(&mut self, nombre: &str, valor: &[u8]) {
            let o = self.cadena(nombre);
            self.u32(3);
            self.u32(valor.len() as u32);
            self.u32(o);
            self.bytes(valor);
        }
        fn prop_txt(&mut self, nombre: &str, t: &str) {
            let mut b = [0u8; 128];
            b[..t.len()].copy_from_slice(t.as_bytes());
            self.prop(nombre, &b[..t.len() + 1]);
        }
        fn prop_u32s(&mut self, nombre: &str, v: &[u32]) {
            let mut b = [0u8; 32];
            for (i, x) in v.iter().enumerate() {
                b[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
            }
            self.prop(nombre, &b[..4 * v.len()]);
        }
    }
    let mut a = A { e: [0; 1024], ne: 0, c: [0; 256], nc: 0, _s: core::marker::PhantomData };
    a.nodo("");
    a.prop_u32s("#address-cells", &[2]);
    a.prop_u32s("#size-cells", &[2]);
    a.prop_txt("compatible", "linux,dummy-virt");
    a.prop_txt("model", "DashOS, caja Linux");
    a.nodo("chosen");
    a.prop_txt("bootargs", BOOTARGS);
    a.fin();
    a.nodo("memory@40000000");
    a.prop_txt("device_type", "memory");
    a.prop_u32s("reg", &[0, RAM_IPA as u32, (ram >> 32) as u32, ram as u32]);
    a.fin();
    a.nodo("cpus");
    a.prop_u32s("#address-cells", &[1]);
    a.prop_u32s("#size-cells", &[0]);
    a.nodo("cpu@0");
    a.prop_txt("device_type", "cpu");
    a.prop_txt("compatible", "arm,cortex-a72");
    a.prop_u32s("reg", &[0]);
    a.fin();
    a.fin();
    a.fin();
    a.u32(9);
    // Cabecera (40 bytes), la tabla de reservas vacía (16 bytes a cero), la estructura y las cadenas.
    let (off_res, off_e) = (40usize, 56usize);
    let off_c = off_e + a.ne;
    let total = off_c + a.nc;
    for (i, v) in [0xd00d_feed, total as u32, off_e as u32, off_c as u32, off_res as u32, 17, 16, 0, a.nc as u32, a.ne as u32].iter().enumerate() {
        sal[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    sal[off_res..off_e].fill(0);
    sal[off_e..off_c].copy_from_slice(&a.e[..a.ne]);
    sal[off_c..total].copy_from_slice(&a.c[..a.nc]);
    total
}

fn u64_en(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7]])
}

/// Arranca la Image de la llave `LINUX` en una máquina de 64 MB, hasta 200 líneas por la consola o 10 s. Falso si no
/// llega a escribir nada.
fn linux() -> bool {
    let tam = dashos::memoria_tamano(LINUX);
    if tam <= 0 || dashos::memoria_proyectar(LINUX, VA_IMAGEN, false) != 0 {
        println!("  [caja-linux] linux: sin la Image ({}); MAL", tam);
        return false;
    }
    // SAFETY: recién proyectada aquí, solo de lectura.
    let imagen = unsafe { core::slice::from_raw_parts(VA_IMAGEN as *const u8, tam as usize) };
    // La cabecera de la Image de arm64 (Documentation/arch/arm64/booting.rst): text_offset en 8, image_size en 16 y
    // "ARM\x64" en 56.
    let (desplazamiento, ocupa) = (u64_en(imagen, 8) as usize, u64_en(imagen, 16) as usize);
    if imagen.len() < 64 || &imagen[56..60] != b"ARM\x64" || IMAGEN_EN + desplazamiento + ocupa.max(imagen.len()) > TROZO * TROZOS {
        println!("  [caja-linux] linux: la Image no vale (o no cabe en 64 MB); MAL");
        return false;
    }
    let mut mems = [0u32; TROZOS];
    for (i, m) in mems.iter_mut().enumerate() {
        let l = dashos::memoria_crear(TROZO) as i32;
        if l <= 0 || dashos::memoria_proyectar(l as u32, VA_RAM_LINUX + i * TROZO, true) != 0 {
            println!("  [caja-linux] linux: sin memoria para el invitado ({}); MAL", l);
            return false;
        }
        *m = l as u32;
    }
    // SAFETY: los objetos, proyectados seguidos aquí.
    let ram = unsafe { core::slice::from_raw_parts_mut(VA_RAM_LINUX as *mut u8, TROZO * TROZOS) };
    let mide = arbol(&mut ram[..4096], (TROZO * TROZOS) as u64);
    ram[IMAGEN_EN + desplazamiento..IMAGEN_EN + desplazamiento + imagen.len()].copy_from_slice(imagen);
    dashos::memoria_quitar(VA_IMAGEN);
    let entrada = RAM_IPA + (IMAGEN_EN + desplazamiento) as u64;
    let vm = dashos::maquina_crear(PERMISO, mems[0], RAM_IPA, entrada, RAM_IPA);
    let mut bien = vm > 0;
    for (i, &m) in mems.iter().enumerate().skip(1) {
        bien &= vm > 0 && dashos::maquina_memoria(vm as u32, m, RAM_IPA + (i * TROZO) as u64) == 0;
    }
    for (i, &m) in mems.iter().enumerate() {
        dashos::memoria_quitar(VA_RAM_LINUX + i * TROZO);
        dashos::cerrar(m);
    }
    if !bien {
        println!("  [caja-linux] linux: no se pudo crear la máquina ({}); MAL", vm);
        return false;
    }
    let vm = vm as u32;
    println!("  [caja-linux] linux: Image de {} KB en {:#x}, árbol de {} bytes en {:#x}, {} MB de RAM; arranca", imagen.len() / 1024, entrada, mide, RAM_IPA, (TROZO * TROZOS) >> 20);
    let t0 = dashos::ahora_us();
    let fin = correr(vm, &Opciones { prefijo: "linux: ", lineas: 200, registros: true, plazo_us: 10_000_000 });
    let ms = (dashos::ahora_us() - t0) / 1000;
    dashos::cerrar(vm);
    let que = match fin {
        Fin::Lineas => texto(format_args!("200 líneas")),
        Fin::Bucle(n) => texto(format_args!("sigue en marcha a los 10 s (el reloj lo sacó {} veces)", n)),
        Fin::Fuera(d, e) => texto(format_args!("{} {:#x}, donde no hay nada", if e { "escribe en" } else { "lee" }, d)),
        Fin::Hvc(n, x0) => texto(format_args!("HVC #{} con x0 = {:#x}", n, x0)),
        Fin::Fallo(esr, pc) => texto(format_args!("trampa en {:#x} (ESR {:#x}, clase {:#x})", pc, esr, esr >> 26)),
        Fin::Registro(c, _) => texto(format_args!("registro {:#x}", c)),
        Fin::Error(e) => texto(format_args!("error {}", e)),
    };
    println!("  [caja-linux] linux: se para la máquina a los {} ms: {}", ms, que.as_str());
    true
}

fn principal() -> u64 {
    // SAFETY: etiquetas de pruebas.s.
    let (hola, eco, lee, escribe, bucle, apaga, reloj) = unsafe {
        (
            codigo(&invitado_hola, &invitado_hola_fin),
            codigo(&invitado_eco, &invitado_eco_fin),
            codigo(&invitado_lee_fuera, &invitado_lee_fuera_fin),
            codigo(&invitado_escribe_fuera, &invitado_escribe_fuera_fin),
            codigo(&invitado_bucle, &invitado_bucle_fin),
            codigo(&invitado_apaga, &invitado_apaga_fin),
            codigo(&invitado_reloj, &invitado_reloj_fin),
        )
    };
    // Una llave que no es el permiso no crea máquinas, aunque sea de memoria.
    let mem = dashos::memoria_crear(RAM);
    let tipo = dashos::maquina_crear(mem, mem, RAM_IPA, RAM_IPA, 0);
    dashos::cerrar(mem);
    if tipo != -6 {
        println!("  [caja-linux] con una llave de memoria por permiso: {} (se esperaba E_TIPO); MAL", tipo);
    }

    let vm = match maquina(hola) {
        Ok(v) => v,
        Err(dashos::E_NO_HAY) => {
            println!("  [caja-linux] no hay hipervisor: DashOS no arrancó en EL2 (en QEMU, falta virtualization=on); sin máquinas virtuales");
            return 0;
        }
        Err(e) => {
            println!("  [caja-linux] no se pudo crear la máquina: {}", e);
            return 1;
        }
    };
    let mut bien = tipo == -6;
    match correr(vm, &prueba(1_000_000)) {
        Fin::Hvc(0, 0) => println!("  [caja-linux] el invitado acaba con HVC #0: bien"),
        _ => {
            println!("  [caja-linux] el invitado no acaba como debe; MAL");
            bien = false;
        }
    }
    dashos::cerrar(vm);

    // Lo que cuesta entrar y salir: N vueltas de HVC, cada una del servicio al invitado y de vuelta (con la llamada al
    // sistema), y de ellas lo que pasa la CPU fuera del kernel (EL2 y el invitado).
    if let Ok(vm) = maquina(eco) {
        const N: u64 = 2000;
        let mut inf = Informe::default();
        let mut dentro = 0;
        let mut vueltas = 0;
        let t0 = dashos::ahora_ns();
        while vueltas < N {
            match dashos::maquina_correr(vm, &mut inf, 0) {
                m::HVC => {
                    vueltas += 1;
                    dentro += inf.cuentas;
                }
                m::INTERRUMPIDO => {}
                _ => break,
            }
        }
        let total = dashos::ahora_ns() - t0;
        dashos::cerrar(vm);
        let dentro_ns = (dentro as u128 * 1_000_000_000 / frecuencia() as u128) as u64;
        if vueltas == N {
            println!("  [caja-linux] ida y vuelta al invitado: {} ns de media ({} vueltas; {} ns de ellos en EL2 y el invitado)", total / N, N, dentro_ns / N);
        } else {
            println!("  [caja-linux] la medida se cortó a las {} vueltas; MAL", vueltas);
            bien = false;
        }
    }

    // Los hostiles.
    bien &= hostil("leer fuera", lee, |f| matches!(f, Fin::Fuera(0x7000_0000, false)));
    bien &= hostil("escribir en el kernel", escribe, |f| matches!(f, Fin::Fuera(0x4020_0000, true)));
    bien &= hostil("bucle sin fin", bucle, |f| matches!(f, Fin::Bucle(n) if *n > 0));
    bien &= hostil("apagar por SMC", apaga, |f| matches!(f, Fin::Fallo(esr, _) if esr >> 26 == 0x17));
    // CNTP_CTL_EL0: op0 3, op1 3, CRn 14, CRm 2, op2 1, como en el ESR.
    bien &= hostil("temporizador del kernel", reloj, |f| matches!(f, Fin::Registro(0x32_f804, true)));

    // Después, otra máquina sigue funcionando.
    let sigue = match maquina(hola) {
        Ok(vm) => {
            let r = matches!(correr(vm, &prueba(1_000_000)), Fin::Hvc(0, 0));
            dashos::cerrar(vm);
            r
        }
        Err(_) => false,
    };
    bien &= sigue;
    println!("  [caja-linux] tras los hostiles, una máquina nueva sigue en marcha: {}", if sigue { "bien" } else { "MAL" });
    println!("  [caja-linux] primer paso de la caja Linux: {}", if bien { "bien" } else { "MAL" });
    // Con la Image de un kernel de Linux (llave 2), el segundo paso.
    if dashos::memoria_tamano(LINUX) > 0 {
        bien &= linux();
    }
    if bien { 0 } else { 1 }
}
