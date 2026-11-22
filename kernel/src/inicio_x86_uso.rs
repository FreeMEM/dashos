//! La imagen de usar en x86-64 (`make x86-uso`): el escritorio de `virt` con `uso` (`inicio.rs`), con los aparatos de un
//! PC. Los mismos programas, en el mismo orden y con los mismos canales, para que cada uno tenga las mismas llaves que
//! en ARM; cambian los controladores (el NVMe y el AHCI, el xHCI, virtio-net, virtio-sound y el HDA). La virtio-gpu,
//! como en ARM, al final.
//!
//! Fuera, de momento: la caja Linux (su VMM es de ARM: virtio por MMIO, GIC, PSCI, árbol de dispositivos) y la caja
//! Amiga (el JIT de Emu68 es de ARM; en x86 irá RelevoJIT, X7a). `sesiones` tiene igual el permiso de máquinas
//! virtuales: si alguien pide una caja, no hay programa que la sirva y no se cuelga nada. También fuera lo que en
//! `virt` sirve solo a las pruebas (`disco`, `ficheros`, los tonos, `hostil`...) y la GPU de la Pi.
//!
//! Los programas van comprimidos con LZ4, como en ARM (`build.rs`): el kernel de x86 se carga en 1 MB y, con todos sin
//! comprimir (más de 6 MB), pisaba lo que OVMF tiene ocupado desde 8 MB.

use super::{DATOS, FIRMWARE, INTERRUPCION, MAQUINAS, OPCIONAL, PERMISO_DMA, PLAZO, REGISTRO, REGISTROS, RELOJ, PUERTOS, VARIABLES};

/// Los servicios que lanza `inicio`, en orden, con los aparatos (por nombre y tipo) y los canales (por nombre) que le
/// pasa a cada uno. El orden de los canales es el de `inicio.rs` (el primero que nombra un canal lo sirve), con los
/// controladores de x86 en el sitio de los de ARM.
pub(super) static PROGRAMAS: &[(&str, &[u8], &[(&str, u32)], &[&str])] = &[
    ("consola", programa!("consola"), &[("registro", REGISTRO), ("serie", PUERTOS)], &[]),
    // El compositor, con la pantalla del GOP: nombra "ventanas" antes que nadie.
    ("compositor", programa!("compositor"), &[("pantalla", FIRMWARE), ("pantalla-info", DATOS)], &["ventanas"]),
    ("xhci", programa!("xhci"), &[("dma-xhci", PERMISO_DMA), ("xhci", REGISTROS), ("xhci", INTERRUPCION | OPCIONAL)], &["ventanas"]),
    ("nvme", programa!("nvme"), &[("dma-nvme", PERMISO_DMA), ("nvme", REGISTROS), ("nvme", INTERRUPCION | OPCIONAL)], &["disco"]),
    // X5c: sin NVMe, el AHCI sirve el canal "disco" (`inicio_x86.rs`, `canal`); con NVMe, "sata", sin clientes.
    ("ahci", programa!("ahci"), &[("dma-ahci", PERMISO_DMA), ("ahci", REGISTROS), ("ahci", INTERRUPCION | OPCIONAL)], &["sata"]),
    // S32: en vivo (arrancado del pendrive), el sistema en memoria, servido como el disco de `archivos`; el NVMe y el
    // AHCI pasan a "nvme" y "sata" (`inicio_x86.rs`, `canal`). Sin pendrive no se lanza (le falta "vivo").
    ("disco-vivo", programa!("disco-vivo"), &[("vivo", DATOS)], &["disco"]),
    ("archivos", programa!("archivos"), &[], &["disco", "archivos"]),
    ("comprobador", programa!("comprobador"), &[], &["disco", "archivos", "comprobar"]),
    ("eco", programa!("eco"), &[], &["eco"]),
    ("sesiones", programa!("sesiones"), &[("plazo", PLAZO), ("hipervisor", MAQUINAS)], &["archivos", "eco", "sesiones", "comprobar"]),
    // S8: el servicio de paquetes, el único que escribe en `Paquetes:` (como en `inicio.rs`).
    ("paq", programa!("paq"), &[], &["archivos", "paq"]),
    ("red", programa!("red"), &[], &["archivos", "red", "red-estado", "red-escuchar"]),
    ("virtio-red", programa!("virtio-red"), &[("dma-vred", PERMISO_DMA), ("virtio-red", REGISTROS), ("vred-cfg", REGISTROS), ("virtio-red", INTERRUPCION | OPCIONAL)], &["red"]),
    ("fondo", programa!("fondo"), &[], &["ventanas", "archivos", "sesiones", "red-estado", "fondo"]),
    ("centro", programa!("centro"), &[], &["ventanas", "sesiones", "fondo", "centro", "archivos"]),
    // Con el canal "uso": la pantalla de entrada de verdad, sin el guion de las pruebas.
    ("guardian", programa!("guardian"), &[], &["ventanas", "sesiones", "fondo", "centro", "uso"]),
    ("imagen", programa!("imagen"), &[], &["archivos", "imagen"]),
    ("sonido", programa!("sonido"), &[("plazo", PLAZO)], &["sonido"]),
    ("avisos", programa!("avisos"), &[], &["avisos"]),
    ("barra", programa!("barra"), &[], &["ventanas", "sesiones", "archivos", "centro", "sonido", "avisos"]),
    // S27: el dock y los avisos en pantalla (OSD), como los de DMS.
    ("dock", programa!("dock"), &[], &["ventanas", "sesiones", "archivos", "sonido"]),
    ("virtio-sonido", programa!("virtio-sonido"), &[("dma-vsonido", PERMISO_DMA), ("virtio-sonido", REGISTROS), ("vsonido-cfg", REGISTROS), ("plazo", PLAZO), ("virtio-sonido", INTERRUPCION | OPCIONAL)], &["sonido"]),
    // X5c: el sonido de un PC (Intel HDA). Si hay los dos, suena el primero que se presenta al servicio.
    ("hda", programa!("hda"), &[("dma-hda", PERMISO_DMA), ("hda", REGISTROS), ("plazo", PLAZO), ("hda", INTERRUPCION | OPCIONAL)], &["sonido"]),
    ("selector", programa!("selector"), &[], &["ventanas", "archivos", "selector"]),
    ("panel", programa!("panel"), &[], &["ventanas", "sesiones", "centro", "sonido", "red-estado", "avisos"]),
    ("explorador", programa!("explorador"), &[], &["ventanas", "archivos", "sesiones", "explorador"]),
    ("workbench", programa!("workbench"), &[], &["ventanas", "archivos", "sesiones", "explorador"]),
    // S13: la hora por la red (SNTP), con el permiso del reloj.
    ("hora", programa!("hora"), &[("reloj", RELOJ)], &["red"]),
    // S32: el instalador, solo en vivo: los discos del PC enteros (por "nvme" y "sata"), el sistema que se copia, y
    // las variables de arranque de la NVRAM. Sirve "instalador" (la ventana y la orden del Shell).
    ("instalador", programa!("instalador"), &[("vivo-original", DATOS), ("variables", VARIABLES | OPCIONAL), ("hipervisor", MAQUINAS | OPCIONAL)], &["instalador", "nvme", "sata"]),
    // S32: la ventana "Instalar DashOS" (la misma que la orden `Instalar`), también solo en vivo.
    ("instalar", programa!("instalar"), &[("vivo-original", DATOS)], &["ventanas", "instalador", "sesiones"]),
    // x86-gpu: la virtio-gpu (en QEMU, `virtio-vga`), como en `virt`: da sus cabezas al compositor por "ventanas" y la
    // pantalla del GOP queda de reserva. Al final, como en `inicio.rs`: las marcas de los demás no cambian. Con las
    // medidas del GOP, para que su primera cabeza tenga el modo que puso el firmware (y no el tamaño de la ventana de
    // QEMU en ese momento).
    ("virtio-gpu", programa!("virtio-gpu"), &[("dma-vgpu", PERMISO_DMA), ("virtio-gpu", REGISTROS), ("vgpu-cfg", REGISTROS), ("virtio-gpu", INTERRUPCION | OPCIONAL), ("pantalla-info", DATOS | OPCIONAL)], &["ventanas"]),
    // Rama sonido-pro: la ventana "Sonido" (el mezclador). Tras la GPU, para no cambiar las marcas de los de antes.
    ("mezclador", programa!("mezclador"), &[], &["ventanas", "sonido"]),
];
