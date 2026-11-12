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
    let r = dashos::maquina_crear(PERMISO, mem, RAM_IPA, RAM_IPA);
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
    /// Una trampa o un fallo: ESR y PC.
    Fallo(u64, u64),
    /// La llamada falló.
    Error(i64),
}

/// Corre la máquina hasta que acaba o pasan `plazo_us`, emulando el UART: cada línea que escribe sale en la consola.
fn correr(vm: u32, plazo_us: u64) -> Fin {
    let mut inf = Informe::default();
    let mut linea = [0u8; 128];
    let mut n = 0;
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
                    println!("  [caja-linux] el invitado dice: {}", core::str::from_utf8(&linea[..n]).unwrap_or("?"));
                    n = 0;
                }
                if c != b'\n' {
                    linea[n] = c;
                    n += 1;
                }
            }
            // FR: nunca lleno.
            m::LECTURA if inf.direccion == UART_FR => valor = 0,
            m::LECTURA => return Fin::Fuera(inf.direccion, false),
            m::ESCRITURA => return Fin::Fuera(inf.direccion, true),
            m::WFI => {}
            m::INTERRUMPIDO => {
                sacado += 1;
                if dashos::ahora_us() - t0 > plazo_us {
                    return Fin::Bucle(sacado);
                }
            }
            m::FALLO => return Fin::Fallo(inf.esr, inf.pc),
            e => return Fin::Error(e),
        }
    }
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
    let fin = correr(vm, 100_000);
    let bien = esperado(&fin);
    let que = match fin {
        Fin::Fuera(d, true) => texto(format_args!("escribe fuera de su memoria ({:#x})", d)),
        Fin::Fuera(d, false) => texto(format_args!("lee fuera de su memoria ({:#x})", d)),
        Fin::Bucle(n) => texto(format_args!("no acaba: el reloj lo sacó {} veces en 100 ms", n)),
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
    let tipo = dashos::maquina_crear(mem, mem, RAM_IPA, RAM_IPA);
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
    match correr(vm, 1_000_000) {
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
    bien &= hostil("temporizador del kernel", reloj, |f| matches!(f, Fin::Fallo(esr, _) if esr >> 26 == 0x18));

    // Después, otra máquina sigue funcionando.
    let sigue = match maquina(hola) {
        Ok(vm) => {
            let r = matches!(correr(vm, 1_000_000), Fin::Hvc(0, 0));
            dashos::cerrar(vm);
            r
        }
        Err(_) => false,
    };
    bien &= sigue;
    println!("  [caja-linux] tras los hostiles, una máquina nueva sigue en marcha: {}", if sigue { "bien" } else { "MAL" });
    println!("  [caja-linux] primer paso de la caja Linux: {}", if bien { "bien" } else { "MAL" });
    if bien { 0 } else { 1 }
}
