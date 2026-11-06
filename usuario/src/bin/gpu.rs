//! El servicio de la GPU (C50): la V3D fuera del kernel. Antes iba en el kernel (C24, C26, C27).
//!
//! Llaves: 1 el permiso DMA de la GPU (desplazamiento 0: la V3D ve direcciones físicas); 2 los registros de la V3D
//! (hub y núcleo 0); 3 el puente ASB; 4 el bloque PM; 5 el cprman; 6 el canal del buzón. `inicio` solo lo lanza si el
//! kernel encontró la V3D (en QEMU no está).
//!
//! Hoy: enciende la GPU, prepara su MMU y hace el primer trabajo (borrar 64 × 64 píxeles a un color y comprobar la
//! memoria). Después se queda esperando: el servidor de verdad atenderá a Mesa (S9). Sin probar: falta la Pi 4.

#![no_std]
#![no_main]

use dashos::graficos::gpu::{Gpu, Tabla};
use dashos::graficos::{listas, v3d};
use dashos::println;

dashos::programa!(principal);

const VA_V3D: usize = 0x0090_0000;
const VA_ASB: usize = 0x0098_0000;
const VA_PM: usize = 0x0099_0000;
const VA_CPRMAN: usize = 0x009A_0000;
const VA_TABLA: usize = 0x40_0000_0000;
const VA_BOS: usize = 0x50_0000_0000;

/// Una petición al buzón (llave 6), con su canal de respuesta.
fn buzon(m: &mut [u32]) -> bool {
    let mut b = [0u8; 256];
    let n = m.len() * 4;
    if n > b.len() {
        return false;
    }
    for (i, w) in m.iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    let (mio, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(6, &b[..n], &[suyo]) != 0 {
        dashos::cerrar(mio);
        dashos::cerrar(suyo);
        return false;
    }
    let mut ll = [0u32; 1];
    let leido = dashos::canal_leer(mio, &mut b, &mut ll).0;
    dashos::cerrar(mio);
    if leido != n as i64 {
        return false;
    }
    for (i, w) in m.iter_mut().enumerate() {
        *w = u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    }
    m[1] == 0x8000_0000
}

/// El primer trabajo de la GPU (C27): borra 64 × 64 píxeles a `20 40 80 FF` y comprueba la memoria.
fn primer_borrado(g: &mut Gpu) -> bool {
    let (Ok(dest), Ok(lista), Ok(baldosas)) = (g.crear(64 * 64 * 4), g.crear(4096), g.crear(4096)) else {
        println!("  [gpu] sin memoria para los búferes");
        return false;
    };
    let b = listas::Borrado { ancho: 64, alto: 64, color: 0xFF80_4020, destino: dest.1, paso: 256, generica: lista.1 + 0x100, memoria_baldosas: baldosas.1 };
    let (Some(rcl), generica) = (b.rcl(), b.lista_generica()) else { return false };
    // Los búferes se ven sin caché: lo escrito lo ve la GPU tal cual, y lo que ella escriba, la CPU.
    let cpu_lista = g.bo(lista.0).unwrap().cpu as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(rcl.como_bytes().as_ptr(), cpu_lista, rcl.len());
        core::ptr::copy_nonoverlapping(generica.como_bytes().as_ptr(), cpu_lista.add(0x100), generica.len());
    }
    let destino = g.bo(dest.0).unwrap().cpu as *mut u32;
    for i in 0..64 * 64 {
        unsafe { destino.add(i).write_volatile(0xA5A5_A5A5) };
    }
    let Some(h) = g.hardware() else { return false };
    h.lanzar_render(lista.1, lista.1 + rcl.len() as u32);
    let fin = h.esperar_evento(v3d::INT_FRDONE, 1_000_000);
    let bien = (0..64 * 64).filter(|&i| unsafe { destino.add(i).read_volatile() }.to_le_bytes() == [0x20, 0x40, 0x80, 0xFF]).count();
    println!("  [gpu] primer trabajo: fin {:x?}; píxeles con el color: {} de 4096", fin, bien);
    bien == 64 * 64
}

fn principal() -> u64 {
    for (llave, va) in [(2, VA_V3D), (3, VA_ASB), (4, VA_PM), (5, VA_CPRMAN)] {
        if dashos::registros_proyectar(llave, va) != 0 {
            println!("  [gpu] sin los registros de la V3D (llave {})", llave);
            return 1;
        }
    }
    let ventanas = v3d::Ventanas { v3d: VA_V3D, asb: VA_ASB, pm: VA_PM, cprman: VA_CPRMAN };
    let mmu = match v3d::Mmu::nueva(1, VA_TABLA) {
        Ok(m) => m,
        Err(e) => {
            println!("  [gpu] sin memoria para la tabla: {:?}", e);
            return 2;
        }
    };
    let g = match v3d::V3d::iniciar(ventanas, mmu, &buzon) {
        Ok(g) => g,
        Err(e) => {
            println!("  [gpu] la V3D no arranca: {:?}", e);
            return 3;
        }
    };
    // Con la V3D ya reiniciada, lo que dejara en cuarentena una GPU anterior puede volver (auditoría de la fase 4, M5).
    dashos::dma_liberar(1);
    let i = g.identidad;
    println!("  [gpu] V3D {}.{}: {} núcleo, {} slices, {} QPU, {} TMU; VA {} bits, PA {} bits; tabla en {:#x}",
        i.version / 10, i.version % 10, i.nucleos, i.slices, i.qpus, i.tmus, i.va_bits, i.pa_bits, g.mmu.tabla);
    let mut gpu = Gpu::nueva(Tabla::Gpu(g), 1, VA_BOS);
    let bien = primer_borrado(&mut gpu);
    println!("  [gpu] primer trabajo de la GPU: {}", if bien { "bien" } else { "MAL" });
    // El servidor de verdad atenderá aquí a Mesa (S9). Mientras, espera sin gastar CPU.
    let (a, _b) = dashos::canal_crear();
    let mut buf = [0u8; 1];
    let mut ll = [0u32; 1];
    loop {
        dashos::canal_leer(a, &mut buf, &mut ll);
    }
}
