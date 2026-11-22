//! X13 (G6 de `docs/diseño/S38-gpu-escritorio.md`): la ventana compartida entre los programas de DashOS y el dominio de
//! gráficos, sin copia. La memoria de un programa (un objeto de memoria: el búfer de una ventana) se proyecta tal cual en
//! la física del invitado (`maquina_memoria`): DashOS escribe sus píxeles una vez y el dominio los lee de la misma
//! memoria para subirlos a la GPU.
//!
//! - **La ventana** es un tramo de la física del invitado (`VENTANA_GPA`, `VENTANA_TAM`), fuera de su e820: el kernel del
//!   dominio la recibe en su línea (`dashos.compartida=<dirección>,<tamaño>`) y la ve por /dev/mem.
//! - **La página de control** (los primeros `CONTROL` bytes) es del servicio `dominio-graficos`: la cabecera y
//!   `RANURAS` ranuras de `LARGO_RANURA` bytes desde `RANURAS_DESDE`, cada una con una ventana de un programa.
//! - **Los búferes** van detrás, cada uno en su sitio alineado a 2 MB, en el orden en que llegan (no se reusan aún).
//!
//! Lo que pide un programa al servicio, por el canal "graficos" (palabras de 32 bits, little-endian):
//!
//! - `PONER`: `[PONER, ancho, alto, paso, formato]` con dos llaves: su objeto de memoria (leer y escribir: lo pide
//!   `maquina_memoria`) y el lado de escribir de un canal suyo para las respuestas. Respuesta: `[PONER, ranura]`, o
//!   `[PONER, código]` con un código negativo (`E_*` de `dashos`) si no se puede.
//! - `LISTO`: `[LISTO, ranura, secuencia (2 palabras)]`: el cuadro de esa secuencia está escrito en su memoria. El
//!   programa no lo vuelve a tocar hasta que el dominio diga que lo vio.
//! - `ACABAR`: `[ACABAR, ranura]`: ya no habrá más cuadros (la ranura queda acabada; su memoria sigue en el dominio).
//!
//! Lo que dice el servicio, por el canal de respuestas:
//!
//! - `VISTO`: `[VISTO, ranura, secuencia (2 palabras)]`: el dominio subió ese cuadro (el último que vio).
//!
//! **El timbre**: tras apuntar lo visto, el dominio escribe el número de la ranura en el puerto de E/S `PUERTO_AVISO`
//! (una salida del invitado); el monitor avisa entonces a los programas. Nadie mira la memoria en bucle.
//!
//! La misma disposición está en `caja-linux/dominios/graficos_dominio.c` (`struct ranura`).

/// Dónde está la ventana en la física del invitado: entre su RAM y las BAR de los aparatos (`dominio_pci::BASE_BAR`).
pub const VENTANA_GPA: u64 = 0xA000_0000;
pub const VENTANA_TAM: u64 = 256 << 20;
/// El puerto del timbre (libre en la máquina del dominio: ni el PCI, ni la consola, ni los del PC de siempre).
pub const PUERTO_AVISO: u16 = 0x5f0;
/// La página de control: la del servicio, al principio de la ventana.
pub const CONTROL: usize = 64 * 1024;
/// Cada búfer, alineado a esto dentro de la ventana.
pub const ALINEADO: u64 = 2 << 20;

/// La cabecera de la página de control: la marca ("DSHM"), la versión y cuántas ranuras.
pub const MAGIA: u32 = 0x4D48_5344;
pub const VERSION: u32 = 1;
pub const RANURAS: usize = 16;
pub const RANURAS_DESDE: usize = 0x40;
pub const LARGO_RANURA: usize = 0x40;

/// Los campos de una ranura (desplazamientos dentro de ella).
pub mod ranura {
    /// u32: `LIBRE`, `PUESTA` o `ACABADA`. Se escribe lo último al poner una (el dominio la lee entonces).
    pub const ESTADO: usize = 0x00;
    pub const ANCHO: usize = 0x04;
    pub const ALTO: usize = 0x08;
    /// u32: bytes por fila. El dominio pide hoy `ancho * 4` (GLES 2 no sube filas con hueco).
    pub const PASO: usize = 0x0c;
    /// u64: desde el principio de la ventana.
    pub const DESPLAZAMIENTO: usize = 0x10;
    pub const TAMANO: usize = 0x18;
    /// u64: el último cuadro listo (lo escribe el servicio).
    pub const SECUENCIA: usize = 0x20;
    /// u64: el último cuadro subido (lo escribe el dominio).
    pub const VISTO: usize = 0x28;
    /// u32: `RGBA` o `XRGB`.
    pub const FORMATO: usize = 0x30;
    /// u64: lo que llevó subir los cuadros, sumado, en µs (lo escribe el dominio, para medir).
    pub const SUBIR_US: usize = 0x38;

    pub const LIBRE: u32 = 0;
    pub const PUESTA: u32 = 1;
    pub const ACABADA: u32 = 2;
}

/// Los formatos: bytes R, G, B, A; o B, G, R, A (XRGB8888 de DashOS, el de las ventanas).
pub const RGBA: u32 = 0;
pub const XRGB: u32 = 1;

/// Las órdenes y avisos del canal.
pub const PONER: u32 = 1;
pub const LISTO: u32 = 2;
pub const ACABAR: u32 = 3;
pub const VISTO: u32 = 4;
