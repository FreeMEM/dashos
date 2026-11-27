//! aviso-cajas: el aviso de memoria baja de DashOS en la caja Amiga (`docs/bitacoras/aviso-cajas.md`).
//!
//! Al arrancar, la caja se apunta en `sesiones` (`ESCUCHAR_MEMORIA`, por la llave 4) con el lado de escribir de un
//! canal que solo ella lee: por ahí le llega el aviso, y solo `sesiones` puede mandarlo. Un hilo lo lee y lo deja
//! pedido (`tomar_pedido`). Exec lo atiende en la siguiente llamada del programa en marcha (los manejadores de memoria
//! baja de los programas, `AddMemHandler`) o, sin programa, entre uno y otro (`caja_atender_memoria`): al acabar,
//! devuelve a DashOS la memoria del 68k que ha crecido y está libre, y dice a `sesiones` lo soltado (`SOLTADO`), que lo
//! compara con lo que tiene cobrado el grupo de la caja.
//!
//! El hilo no reserva memoria (el montón de Rust de la caja no tiene cerrojo: solo lo usa el hilo principal).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use caja_exec::memoria_baja::Soltado;
use dashos::protocolo::sesiones as s;

use crate::preguntador::LLAVE_SESIONES;

/// La llave 5 de la caja, en las imágenes de prueba: el canal "presion" (`usuario/src/bin/presion.rs`).
const LLAVE_PRESION: u32 = 5;
/// Lo más que se espera a que `sesiones` conteste al apuntarse.
const ESPERA_US: u64 = 10_000_000;

/// Llegó un aviso de memoria baja (nivel 1) y no se ha atendido.
static PEDIDO: AtomicBool = AtomicBool::new(false);

struct Pila(UnsafeCell<[u8; 16 << 10]>);
// SEGURIDAD: solo la usa el hilo del aviso, que se crea una vez.
unsafe impl Sync for Pila {}
static PILA: Pila = Pila(UnsafeCell::new([0; 16 << 10]));
static ESCUCHA: AtomicBool = AtomicBool::new(false);
/// `sesiones` la ha apuntado y el hilo escucha.
static OYE: AtomicBool = AtomicBool::new(false);

/// Se apunta en `sesiones` para el aviso y lo escucha en un hilo. Sin `sesiones` (el diagnóstico, el escritorio de la
/// Pi), nada.
pub fn escuchar() {
    if ESCUCHA.swap(true, Ordering::AcqRel) {
        return;
    }
    let (lectura, escritura) = dashos::canal_crear();
    let (resp, vuelta) = dashos::canal_crear();
    if dashos::canal_escribir(LLAVE_SESIONES, &s::ESCUCHAR_MEMORIA.to_le_bytes(), &[vuelta, escritura]) != 0 {
        for k in [lectura, escritura, resp, vuelta] {
            dashos::cerrar(k);
        }
        return;
    }
    let mut b = [0u8; 4];
    let (n, _, _) = dashos::canal_leer_plazo(resp, &mut b, &mut [], ESPERA_US);
    dashos::cerrar(resp);
    if n != 4 || i32::from_le_bytes(b) != 0 {
        crate::di(&alloc::format!("sesiones no le da el aviso de memoria baja ({})", if n == 4 { i32::from_le_bytes(b) as i64 } else { n }));
        dashos::cerrar(lectura);
        return;
    }
    // SEGURIDAD: `ESCUCHA` asegura que solo se llega aquí una vez; la pila es del hilo desde ahora.
    let pila = unsafe { &mut *PILA.0.get() };
    if dashos::hilos::crear(hilo, lectura as u64, pila, 0) < 0 {
        crate::di("sin hilo para el aviso de memoria baja");
        return;
    }
    OYE.store(true, Ordering::Release);
    crate::di("escucha el aviso de memoria baja");
}

fn hilo(lectura: u64) -> u64 {
    let mut b = [0u8; s::LARGO_SOLTAR];
    loop {
        let (n, _) = dashos::canal_leer(lectura as u32, &mut b, &mut []);
        if n == dashos::E_CERRADO {
            return 0;
        }
        if n == s::LARGO_SOLTAR as i64
            && u32::from_le_bytes(b[0..4].try_into().unwrap()) == s::SOLTAR
            && u32::from_le_bytes(b[4..8].try_into().unwrap()) == 1
        {
            PEDIDO.store(true, Ordering::Release);
        }
    }
}

/// Si llegó un aviso sin atender (y lo da por atendido).
pub fn tomar_pedido() -> bool {
    PEDIDO.swap(false, Ordering::AcqRel)
}

/// Lo soltado por un aviso: al registro y a `sesiones`.
pub fn informar(s_: Soltado, ram: u32) {
    let mb = |fin: u32| fin.saturating_sub(ram) >> 20;
    crate::di(&alloc::format!(
        "memoria baja: {} manejadores del 68k ({} llamadas); la memoria del 68k pasa de {} MB a {} MB: devuelve {} KB a DashOS",
        s_.manejadores,
        s_.llamadas,
        mb(s_.fin_antes),
        mb(s_.fin_despues),
        s_.bytes >> 10
    ));
    let mut m = [0u8; 12];
    m[0..4].copy_from_slice(&s::SOLTADO.to_le_bytes());
    m[4..12].copy_from_slice(&(s_.bytes as u64).to_le_bytes());
    let (lectura, vuelta) = dashos::canal_crear();
    if dashos::canal_escribir(LLAVE_SESIONES, &m, &[vuelta]) != 0 {
        dashos::cerrar(vuelta);
    }
    // La respuesta no hace falta.
    dashos::cerrar(lectura);
}

/// Para la prueba (`caja.c`): dice a `presion` que la caja tiene algo que soltar. Verdadero si hay a quién decírselo.
#[unsafe(no_mangle)]
pub extern "C" fn caja_presion_listo() -> i32 {
    (OYE.load(Ordering::Acquire) && dashos::canal_escribir(LLAVE_PRESION, b"caja-amiga", &[]) == 0) as i32
}
