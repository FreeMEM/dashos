//! El protocolo del servicio de sonido (S19, S19b; encargo `sonido-pro`), compartido con sus clientes. Lo describe
//! `docs/referencia/servicios.md`, sección "Sonido".
//!
//! Los programas que suenan mandan `ABRIR`, `VOLUMEN` y `CERRAR` (`usuario/src/sonido_cliente.rs`). El mezclador de la
//! interfaz (el panel, la ventana "Sonido", Ajustes, la barra, el dock y la pantalla de bloqueo, por el nombre que da
//! `inicio`) se suscribe al **estado**: una memoria que crea él y que el servicio rellena unas 30 veces por segundo con
//! el grafo (flujos, salidas y sus niveles), los efectos, lo que suena ahora y, si alguien mira, el espectro. Se lee
//! con un cerrojo de secuencia: impar mientras el servicio escribe.
//!
//! Sin montón (la biblioteca `dashos` no lo usa): leer y escribir el estado con textos y listas está en
//! `usuario/src/sonido_estado.rs`, que incluye quien lo necesita.
//!
//! Sin nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/protocolo/sonido.rs`.

#![allow(dead_code)]


// Órdenes (la primera palabra de cada mensaje).
pub const SALIDA: u32 = 1;
pub const TIC: u32 = 2;
pub const ABRIR: u32 = 10;
pub const VOLUMEN: u32 = 11;
pub const CERRAR: u32 = 12;
pub const GENERAL: u32 = 13;
/// Suscribirse al estado: llaves, la memoria (`TAM_ESTADO` bytes como poco) y un extremo para despertar al suscriptor.
pub const SUSCRIBIR: u32 = 20;
/// [id, izquierdo f32, derecho f32]: el volumen de un nodo (solo el mezclador).
pub const NODO_VOLUMEN: u32 = 21;
/// [id, 0|1].
pub const NODO_SILENCIO: u32 = 22;
/// [flujo, salida]: por dónde suena un flujo (solo el mezclador; con fundido cruzado).
pub const NODO_SALIDA: u32 = 23;
/// [salida]: la salida del sistema (a ella van los flujos nuevos y los que no eligieron otra).
pub const POR_DEFECTO: u32 = 24;
/// [qué, valor]: un efecto de la salida (ver `EF_*`).
pub const EFECTO: u32 = 25;
/// [modo]: el período (`PERIODO_*`).
pub const PERIODO: u32 = 26;
/// Que la ventana "Sonido" se abra (la escucha el mezclador por el estado: `ventana`).
pub const VENTANA: u32 = 27;
/// [entrada]: la entrada del sistema.
pub const ENTRADA: u32 = 28;
/// Lo que suena ahora, de un programa: [estado, capacidades, posición ms, duración ms, largo del título, largo del
/// artista] y los dos textos; con una llave opcional por donde recibir las órdenes (`MEDIOS_ORDEN`, [orden]).
pub const MEDIOS: u32 = 30;
/// [id del medio, orden]: reproducir o pausar, siguiente, anterior (del mezclador).
pub const MEDIOS_ORDEN: u32 = 31;
pub const MEDIOS_FIN: u32 = 32;
/// [evento]: un sonido del sistema (`SON_*`).
pub const SONAR: u32 = 40;
/// [tema, silenciados 0|1, volumen 0 a 100]: los ajustes de los sonidos del sistema.
pub const SONIDOS: u32 = 41;
/// [0|1]: el "no molestar" (del panel).
pub const NO_MOLESTAR: u32 = 42;

// Los efectos (`EFECTO`).
pub const EF_EQ_PREAJUSTE: u32 = 1;
pub const EF_EQ_ACTIVO: u32 = 2;
pub const EF_LIMITADOR: u32 = 3;
pub const EF_ESPACIAL_MODO: u32 = 4;
pub const EF_ESPACIAL_CANTIDAD: u32 = 5;
pub const EF_LIMITADOR_UMBRAL: u32 = 6;
/// La banda `b` del gráfico: `EF_EQ_BANDA + b`, en centésimas de dB.
pub const EF_EQ_BANDA: u32 = 0x100;

pub const PERIODO_NORMAL: u32 = 0;
pub const PERIODO_TOCAR: u32 = 1;
pub const PERIODO_AHORRO: u32 = 2;
/// Cuadros de cada período, por modo.
pub const CUADROS_PERIODO: [u32; 3] = [256, 64, 1024];

// Los medios.
pub const MED_PARADO: u32 = 0;
pub const MED_SUENA: u32 = 1;
pub const MED_PAUSA: u32 = 2;
pub const PUEDE_PAUSAR: u32 = 1;
pub const PUEDE_SIGUIENTE: u32 = 2;
pub const PUEDE_ANTERIOR: u32 = 4;
pub const ORDEN_REPRODUCIR_PAUSAR: u32 = 1;
pub const ORDEN_SIGUIENTE: u32 = 2;
pub const ORDEN_ANTERIOR: u32 = 3;

// Los sonidos del sistema.
pub const SON_AVISO: u32 = 1;
pub const SON_BLOQUEO: u32 = 2;
pub const SON_ERROR: u32 = 3;
pub const SON_CONECTAR: u32 = 4;
pub const SON_DESCONECTAR: u32 = 5;
pub const SON_VOLUMEN: u32 = 6;
pub const SON_ENTRAR: u32 = 7;
pub const NOMBRES_SONIDOS: [&str; 7] = ["Aviso", "Bloqueo", "Error", "Conectar", "Desconectar", "Volumen", "Entrar"];

// El estado: palabras de 32 bits.
pub const TAM_ESTADO: usize = 16 * 1024;
pub const VERSION: u32 = 1;
pub const W_SECUENCIA: usize = 0;
pub const W_VERSION: usize = 1;
/// Lo escribe el suscriptor que mira el espectro (lo sube en cada cuadro que pinta): sin él, el servicio no calcula.
pub const W_LATIDO: usize = 2;
pub const W_NODOS: usize = 3;
pub const W_DEFECTO: usize = 4;
pub const W_CUANTO: usize = 5;
pub const W_VACIOS: usize = 6;
pub const W_EQ_PREAJUSTE: usize = 7;
pub const W_EQ_ACTIVO: usize = 8;
pub const W_LIMITADOR: usize = 9;
pub const W_ESPACIAL_MODO: usize = 10;
pub const W_ESPACIAL_CANTIDAD: usize = 11;
pub const W_BANDAS: usize = 12;
pub const W_TEMA_SONIDOS: usize = 13;
/// Bit 0: sonidos del sistema silenciados; bit 1: no molestar.
pub const W_SONIDOS_FLAGS: usize = 14;
pub const W_SONIDOS_VOLUMEN: usize = 15;
pub const W_EQ_BANDAS: usize = 16;
pub const W_LIMITADOR_ACTUA: usize = 26;
pub const W_MEDIOS: usize = 27;
/// Sube cada vez que alguien pide abrir la ventana "Sonido".
pub const W_VENTANA: usize = 28;
pub const W_MEZCLA_US: usize = 29;
pub const W_MEZCLA_PEOR_US: usize = 30;
/// La latencia de la salida por defecto, en µs (lo mezclado por delante más el período del aparato).
pub const W_LATENCIA_US: usize = 31;
pub const W_ENTRADA: usize = 32;
pub const W_PERIODO_MODO: usize = 33;
pub const W_ESPECTRO: usize = 64;
pub const MAX_BANDAS: usize = 32;
pub const W_ONDA: usize = 80;
pub const ONDA: usize = 128;
pub const W_TABLA: usize = 144;
pub const PALABRAS_NODO: usize = 32;
pub const MAX_NODOS: usize = 64;
pub const W_TABLA_MEDIOS: usize = W_TABLA + PALABRAS_NODO * MAX_NODOS;
pub const PALABRAS_MEDIO: usize = 64;
pub const MAX_MEDIOS: usize = 4;
const _: () = assert!(4 * (W_TABLA_MEDIOS + PALABRAS_MEDIO * MAX_MEDIOS) <= TAM_ESTADO);

pub const CLASE_FLUJO: u32 = 0;
pub const CLASE_MEZCLA: u32 = 1;
pub const CLASE_EFECTO: u32 = 2;
pub const CLASE_SALIDA: u32 = 3;
pub const CLASE_ENTRADA: u32 = 4;

/// De quién es un flujo, para el icono.
pub const DUENO_SISTEMA: u32 = 0;
pub const DUENO_PROGRAMA: u32 = 1;
pub const DUENO_AMIGA: u32 = 2;
pub const DUENO_LINUX: u32 = 3;

/// Bits de `Nodo::marcas`.
pub const N_SALIENDO: u32 = 1;
/// En silencio porque su sesión está detrás (U6).
pub const N_DETRAS: u32 = 2;
/// La salida es privada (auriculares): nunca se salta sola de ella a unos altavoces (S19).
pub const N_PRIVADA: u32 = 4;

/// El nombre para enseñar de un programa que suena (los del sistema con nombre propio; los demás, tal cual).
pub fn nombre_visible(programa: &str) -> &str {
    match programa {
        "caja-amiga" => "Caja Amiga",
        "caja-linux" => "Caja Linux",
        "tono-a" => "Tono A",
        "tono-b" => "Tono B",
        "melodia" => "Melodía",
        "sistema" => "Sonidos del sistema",
        "paula-prueba" => "Paula",
        _ => programa,
    }
}

/// Un nivel (1,0 = 32.768) en decibelios enteros, de -60 a +12 (lo de debajo es -60).
pub fn a_db(nivel: u32) -> i32 {
    if nivel == 0 {
        return -60;
    }
    // 20·log10(n/32768) = 6,0206·log2(n/32768), con log2 entero y la mantisa en lineal (error < 0,6 dB).
    let n = nivel as u64;
    let l2 = 63 - n.leading_zeros() as i64;
    let mantisa_mil = (((n << 16) >> l2) as i64 - 65_536) * 1000 / 65_536;
    let log2_mil = l2 * 1000 + mantisa_mil - 15_000;
    (log2_mil * 6021 / 1_000_000).clamp(-60, 12) as i32
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn decibelios() {
        assert_eq!(a_db(32_768), 0);
        assert_eq!(a_db(16_384), -6);
        assert_eq!(a_db(0), -60);
        assert_eq!(a_db(1), -60);
        assert!((a_db(3277) + 20).abs() <= 1);
        assert_eq!(a_db(u32::MAX), 12);
    }
}
