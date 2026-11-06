//! C50: las pruebas de los gráficos de la Pi 4 desde un proceso (antes C23, C24, C26 y C27 en el kernel). Lo que se
//! puede comprobar sin el hardware: la codificación de las listas del HVS y de la V3D contra los ejemplos de las
//! especificaciones, la tabla de la MMU de la GPU y los búferes de la GPU. Llave 1: un permiso DMA de desplazamiento
//! 0, como el del servicio `gpu`. El kernel comprueba después que la memoria vuelve entera.
//!
//! Sale con 50 si todo va bien, o con el número de la prueba que falló (23, 24, 26 o 27).

#![no_std]
#![no_main]

use dashos::graficos::gpu::{self, Gpu, Tabla};
use dashos::graficos::hvs::{self, Alfa, Formato, Plano};
use dashos::graficos::{listas, v3d};
use dashos::println;

dashos::programa!(principal);

const VA_TABLA: usize = 0x40_0000_0000;
const VA_BOS: usize = 0x50_0000_0000;

fn prueba_hvs() -> bool {
    // El ejemplo de §5.10: 1920 × 1080 en XRGB8888 de DRM, en (0, 0), paso 7680, búfer en la física 0x10000000.
    let ejemplo = Plano { x: 0, y: 0, ancho: 1920, alto: 1080, bus: hvs::bus(0x1000_0000).unwrap(), paso: 7680, formato: Formato::Xrgb8888Drm, alfa: Alfa::Fijo(0xfff) };
    let esperado = [0x4800_D807u32, 0, 0x4000_FFF0, 0x0438_0780, 0xC0C0_C0C0, 0xD000_0000, 0xC0C0_C0C0, 0x0000_1E00, 0x8000_0000];
    let mut p = [0u32; 32];
    let n = hvs::codificar_lista(&[ejemplo], &mut p);
    let bien_ejemplo = n == Ok(9) && p[..9] == esperado;
    println!("  [graficos] lista del HVS del ejemplo de la especificación: {}", if bien_ejemplo { "igual palabra a palabra" } else { "DISTINTA" });
    // Escritorio nativo de DashOS y un cursor con alfa por píxel encima.
    let escritorio = Plano { formato: Formato::Argb8888Nativo, ..ejemplo };
    let cursor = Plano { x: 100, y: 200, ancho: 64, alto: 64, bus: hvs::bus(0x2000_0000).unwrap(), paso: 256, formato: Formato::Argb8888Nativo, alfa: Alfa::PorPixelPremultiplicado };
    let n2 = hvs::codificar_lista(&[escritorio, cursor], &mut p);
    let bien_dos = n2 == Ok(17) && p[0] == 0x4800_D807 && p[8] == 0x4800_D807 && p[9] == (200 << 16 | 100)
        && p[10] == 0x2000_FFF0 && p[11] == 0x0040_0040 && p[13] == 0xE000_0000 && p[16] == 0x8000_0000;
    // Una dirección que no es del primer GB (sin el 0xC0000000 delante) se rechaza.
    let fuera = Plano { bus: 0x4000_0000, ..cursor }.codificar(&mut p) == Err(hvs::Error::FueraDelPrimerGb);
    println!("  [graficos] escritorio y cursor: {} palabras; fuera del primer GB se rechaza: {}", n2.unwrap_or(0), if fuera { "sí" } else { "no" });
    bien_ejemplo && bien_dos && fuera
}

fn prueba_v3d() -> bool {
    // §5.2: una página física en 0x12345000 da la entrada 0x30012345, guardada en little-endian.
    let bytes = v3d::entrada(0x1234_5000).to_ne_bytes();
    let bien_pte = bytes == [0x45, 0x23, 0x01, 0x30];
    // §5.4, §9.2, §9.3: los valores que da la especificación, rehechos con los bits.
    let bien_reg = v3d::MMU_CTL_ARRANQUE == 0x060D_0C01 && v3d::MASCARA_NUCLEO == 0xA7 && v3d::MASCARA_HUB == 0x3A;
    // La tabla de verdad (4 MB de memoria DMA): mapear dos páginas, leerlas y quitarlas.
    let bien_mmu = match v3d::Mmu::nueva(1, VA_TABLA) {
        Err(_) => false,
        Ok(t) => {
            t.mapear(0x10000, [0x1234_5000, 0x1234_6000].into_iter());
            let puestas = t.ver(0x10000) == v3d::entrada(0x1234_5000) && t.ver(0x11000) == v3d::entrada(0x1234_6000) && t.ver(0x12000) == 0;
            t.quitar(0x10000, 2);
            puestas && t.ver(0x10000) == 0
        }
    };
    println!("  [graficos] V3D: entrada de la tabla en memoria {:02x?}; registros como la especificación: {}; tabla de 4 MB: {}",
        bytes, if bien_reg { "sí" } else { "no" }, if bien_mmu { "bien" } else { "MAL" });
    bien_pte && bien_reg && bien_mmu
}

fn prueba_bo() -> bool {
    let Ok(t) = v3d::Mmu::nueva(1, VA_TABLA) else { return false };
    let mut g = Gpu::nueva(Tabla::Sola(t), 1, VA_BOS);
    let (Ok(a), Ok(b), Ok(c)) = (g.crear(4096), g.crear(10_000), g.crear(4096)) else { return false };
    let bien_va = [a.1, b.1, c.1].iter().all(|&v| v != 0 && v % 4096 == 0) && a.1 + 4096 == b.1 && b.1 + 3 * 4096 == c.1;
    // Cada página del BO está en la tabla, en orden, y llega a cero.
    let bo = *g.bo(b.0).unwrap();
    let bien_tabla = (0..bo.paginas).all(|i| g.tabla().ver(b.1 + i * 4096) == v3d::entrada(bo.fisica + i as u64 * 4096))
        && (0..bo.paginas as usize * 512).all(|i| unsafe { (bo.cpu as *const u64).add(i).read_volatile() } == 0);
    // Liberar el del medio deja un hueco que se reutiliza; al final todo vuelve.
    let _ = g.liberar(b.0);
    let hueco = g.tramos_libres() == 2 && g.tabla().ver(b.1) == 0;
    let Ok(d) = g.crear(8192) else { return false };
    let reutiliza = d.1 == b.1 && g.va(d.0) == Some(d.1);
    for h in [a.0, c.0, d.0] {
        let _ = g.liberar(h);
    }
    let junta = g.tramos_libres() == 1 && g.liberar(a.0) == Err(gpu::Error::Manija) && g.liberar(0) == Err(gpu::Error::Manija);
    println!("  [graficos] búferes: VA {:#x}, {:#x}, {:#x}; tabla {}; hueco reutilizado {}; tramos juntados {}", a.1, b.1, c.1,
        if bien_tabla { "bien" } else { "MAL" }, if hueco && reutiliza { "sí" } else { "no" }, if junta { "sí" } else { "no" });
    bien_va && bien_tabla && hueco && reutiliza && junta
}

fn prueba_listas() -> bool {
    // Las listas de borrado del ejemplo de v3d-listas.md §8, comparadas byte a byte con sus volcados.
    let ejemplo = listas::Borrado { ancho: 64, alto: 64, color: 0xFF80_4020, destino: 0x0010_0000, paso: 256, generica: 0x0020_0100, memoria_baldosas: 0x0020_1000 };
    let rcl_esperada: [u8; 106] = [
        0x79, 0x00, 0x40, 0x00, 0x40, 0x00, 0x40, 0x00, 0x00, 0x79, 0x03, 0x20, 0x40, 0x80, 0xFF, 0x00, 0x00, 0x00,
        0x79, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79, 0x02, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00,
        0x7E, 0x05, 0x7B, 0x00, 0x10, 0x20, 0x00, 0x7A, 0x00, 0x00, 0x01, 0x01, 0x01, 0x10, 0x00, 0x00,
        0x7C, 0x00, 0x00, 0x00, 0x1A, 0x1D, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x19, 0x03, 0x1B, 0x7C, 0x00, 0x00, 0x00, 0x1A, 0x1D, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x1B, 0x13, 0x14, 0x00, 0x01, 0x20, 0x00, 0x11, 0x01, 0x20, 0x00, 0x17, 0x00, 0x00, 0x0D,
    ];
    let generica_esperada: [u8; 17] = [0x7D, 0x1A, 0x1D, 0x00, 0xB0, 0x01, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x1B, 0x12];
    let Some(rcl) = ejemplo.rcl() else { return false };
    let generica = ejemplo.lista_generica();
    let bien = rcl.como_bytes() == rcl_esperada && generica.como_bytes() == generica_esperada;
    println!("  [graficos] listas de borrado del ejemplo: RCL {} bytes, genérica {} bytes, iguales al volcado: {}", rcl.len(), generica.len(), if bien { "sí" } else { "no" });
    bien
}

fn principal() -> u64 {
    for (n, prueba) in [(23, prueba_hvs as fn() -> bool), (24, prueba_v3d), (26, prueba_bo), (27, prueba_listas)] {
        if !prueba() {
            return n;
        }
    }
    50
}
