//! X12 (G5 de `docs/diseño/S38-gpu-escritorio.md`): el dominio de gráficos de x86. Un Linux mínimo en una máquina
//! virtual de DashOS que recibe una GPU del PCIe pasada con la IOMMU (`maquina_aparato`, X11; en QEMU, la virtio-gpu con
//! virgl, como se pasaría una Intel o una AMD de verdad) y saca cuadros por KMS con Mesa.
//!
//! - La máquina es la del dominio de x86 (`dominio_x86.rs`: la RAM, el kernel por el protocolo de 32 bits, la tabla MP,
//!   el APIC y el IOAPIC, la consola en el 0x3F8), sin disco.
//! - El PCI, en `dominio_pci.rs`: un bus con un puente y la GPU, sus BAR proyectadas en el invitado y su MSI-X emulada,
//!   con una interrupción del anfitrión por entrada (un hilo las escucha y patea a la vCPU).
//! - El invitado (`caja-linux/dominios/graficos_dominio.c`, preparado fuera de git por `tools/dominio_graficos.py`) pinta
//!   `SEGUNDOS` segundos y dice cuántos cuadros y cuánta CPU suya por cuadro; aquí se mide además la CPU de DashOS entero
//!   (todos los núcleos) mientras pinta.
//!
//! Llaves (por su nombre): "hipervisor" (el permiso de máquinas), "gpu-dominio" (el aparato) y "dominio-graficos" (el
//! kernel y el initramfs: el largo de cada uno, u64, y detrás, seguidos). Sale con 0 si el dominio acabó bien.

#![no_std]
#![no_main]

extern crate alloc;

dashos::programa!(principal);
dashos::monton!();

#[cfg(not(target_arch = "x86_64"))]
fn principal() -> u64 {
    dashos::println!("  [dominio-graficos] solo en x86");
    0
}

#[cfg(target_arch = "x86_64")]
#[path = "../dominio_x86.rs"]
#[allow(dead_code)]
mod dominio_x86;
#[cfg(target_arch = "x86_64")]
#[path = "../dominio_pci.rs"]
mod dominio_pci;

#[cfg(target_arch = "x86_64")]
fn principal() -> u64 {
    x86::principal()
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use crate::dominio_pci::{self, PciPasado};
    use crate::dominio_x86::Dominio;
    use alloc::string::String;
    use core::sync::atomic::Ordering;
    use dashos::{maquina, println};

    /// Donde se proyectan aquí el kernel y el initramfs del dominio.
    const VA_DATOS: usize = 0x50_0000_0000;
    /// La RAM del dominio: el initramfs (unos 23 MB, desempaquetado otra vez en la RAM) y Mesa.
    const RAM_MB: u64 = 512;
    /// Lo que pinta el invitado.
    const SEGUNDOS: u64 = 20;
    /// Hasta cuándo se espera al dominio.
    const PLAZO_US: u64 = 180_000_000;
    const PILA: usize = 64 * 1024;
    /// La línea del kernel del dominio (`LINEA` de `tools/dominio_graficos.py`, con el porqué), sin la frecuencia del TSC
    /// ni los segundos.
    const LINEA: &str = "earlyprintk=serial,0x3f8,115200,keep tsc=reliable printk.devkmsg=on";

    /// Lo que no está ocioso, sumando todos los núcleos en marcha (µs).
    fn ocupado() -> u64 {
        let nucleos = dashos::medir(14, 0).max(0) as u64;
        let ahora = dashos::ahora_us();
        let mut total = 0u64;
        for n in 0..64 {
            if nucleos & (1 << n) != 0 {
                total += ahora.saturating_sub(dashos::medir(3, n).max(0) as u64);
            }
        }
        total
    }

    pub fn principal() -> u64 {
        let tabla = dashos::llaves::Tabla::leer();
        let (Some(permiso), Some(gpu), Some(datos)) = (tabla.buscar("hipervisor"), tabla.buscar("gpu-dominio"), tabla.buscar("dominio-graficos")) else {
            println!("  [dominio-graficos] faltan llaves (hipervisor, gpu-dominio, dominio-graficos)");
            return 2;
        };
        let tam = dashos::memoria_tamano(datos);
        if tam < 16 || dashos::memoria_proyectar(datos, VA_DATOS, false) != 0 {
            println!("  [dominio-graficos] no se puede ver el kernel del dominio");
            return 2;
        }
        // SAFETY: recién proyectado aquí, de `tam` bytes, de solo lectura, y no se quita.
        let zona: &'static [u8] = unsafe { core::slice::from_raw_parts(VA_DATOS as *const u8, tam as usize) };
        let n_bz = u64::from_le_bytes(zona[0..8].try_into().unwrap()) as usize;
        let n_cpio = u64::from_le_bytes(zona[8..16].try_into().unwrap()) as usize;
        let (Some(bz), Some(cpio)) = (zona.get(16..16 + n_bz), zona.get(16 + n_bz..16 + n_bz + n_cpio)) else {
            println!("  [dominio-graficos] el kernel o el initramfs se salen de su objeto");
            return 2;
        };
        // Lo que gasta DashOS sin el dominio (todos los núcleos), para restarlo de lo medido mientras pinta.
        let (b0, o0) = (dashos::ahora_us(), ocupado());
        dashos::dormir_us(2_000_000);
        let reposo = (ocupado() - o0) as f32 / (dashos::ahora_us() - b0) as f32;
        println!("  [dominio-graficos] DashOS en reposo: {:.1} ms de CPU por segundo (todos los núcleos)", reposo * 1000.0);
        let linea = alloc::format!("{} tsc_early_khz={} dashos.graficos={}", LINEA, dashos::frecuencia() / 1000, SEGUNDOS);
        let t0 = dashos::ahora_us();
        let mut dom = match Dominio::sin_disco(permiso, RAM_MB, bz, cpio, &linea) {
            Ok(d) => d,
            Err(e) => {
                println!("  [dominio-graficos] {}", e);
                return 3;
            }
        };
        let m = dom.vcpu();
        let (irq, banderas) = dashos::maquina_aparato(m, gpu);
        if irq < 0 {
            println!("  [dominio-graficos] no se puede pasar la GPU: {}", irq);
            return 4;
        }
        if irq > 0 {
            dashos::cerrar(irq as u32);
        }
        let vectores = ((banderas >> maquina::APARATO_VECTORES_DESDE) & 0xff) as usize;
        let id = dashos::maquina_aparato_leer(m, gpu, 0, 4);
        println!(
            "  [dominio-graficos] GPU {:04x}:{:04x} pasada al dominio: {} interrupciones{}{}",
            id & 0xffff,
            (id >> 16) & 0xffff,
            vectores,
            if banderas & maquina::APARATO_REMAPEADAS != 0 { ", remapeadas por la IOMMU" } else { ", SIN remapeo" },
            if banderas & maquina::APARATO_FLR != 0 { ", con FLR" } else { "" }
        );
        let mut pci = PciPasado::nuevo(m, gpu, vectores);
        for i in 0..6 {
            let (gpa, t) = pci.bar(i);
            if t > 0 {
                println!("  [dominio-graficos] BAR {}: {} KB en {:#x} del invitado", i, t / 1024, gpa);
            }
        }
        let (lee, escribe) = dashos::canal_crear();
        if !pci.asociar(escribe) {
            println!("  [dominio-graficos] las interrupciones de la GPU no se asocian");
            return 5;
        }
        let mut pila = alloc::vec![0u8; PILA];
        if dashos::hilos::crear(dominio_pci::escuchar, lee as u64, &mut pila, 0) < 0 {
            println!("  [dominio-graficos] sin el hilo de las interrupciones");
            return 5;
        }
        // Lo que se mide mientras pinta: desde "pintando" hasta la línea de los cuadros.
        let mut desde: Option<(u64, u64, u64, u64)> = None;
        let mut medida: Option<String> = None;
        let mut escribir = |l: &str| {
            println!("  [dominio-graficos] linux: {}", l);
            let ahora = dashos::ahora_us();
            let (salidas, avisos) = (dominio_pci::VUELTAS.load(Ordering::Relaxed), dominio_pci::AVISOS.load(Ordering::Relaxed));
            if l.contains("dominio: pintando") {
                desde = Some((ahora, ocupado(), salidas, avisos));
                println!("  [dominio-graficos] arrancado y pintando en {} ms", (ahora - t0) / 1000);
            }
            if l.contains("dominio: cuadros ") && l.contains(" en ") {
                if let Some((t, o, s, a)) = desde {
                    let cuadros: u64 = l.split("dominio: cuadros ").nth(1).and_then(|x| x.split(' ').next()).and_then(|x| x.parse().ok()).unwrap_or(0).max(1);
                    let o2 = ocupado();
                    let neto = ((o2 - o) as f32 - reposo * (ahora - t) as f32).max(0.0);
                    medida = Some(alloc::format!(
                        "{} cuadros en {} ms; CPU de DashOS (todos los núcleos) {} ms, {} µs por cuadro, {:.0} µs por cuadro sin lo de reposo; {} salidas del invitado y {:.1} interrupciones de la GPU por cuadro",
                        cuadros,
                        (ahora - t) / 1000,
                        (o2 - o) / 1000,
                        (o2 - o) / cuadros,
                        neto / cuadros as f32,
                        (salidas - s) / cuadros,
                        (avisos - a) as f32 / cuadros as f32
                    ));
                }
            }
        };
        let r = dom.correr_con(PLAZO_US, &mut escribir, &mut pci);
        println!("  [dominio-graficos] interrupciones: {}", pci.estado());
        for a in &pci.avisos {
            println!("  [dominio-graficos] aviso: {}", a);
        }
        println!("  [dominio-graficos] {} salidas del invitado; {} interrupciones de la GPU entregadas", dom.salidas(), pci.entregadas);
        if let Some(md) = medida {
            println!("  [dominio-graficos] medida: {}", md);
        }
        match r {
            Ok(fin) => {
                println!("  [dominio-graficos] {} en {} ms", fin, (dashos::ahora_us() - t0) / 1000);
                if fin.trim_end() == "dominio: fin graficos 0" { 0 } else { 1 }
            }
            Err(e) => {
                println!("  [dominio-graficos] MAL: {}", e);
                1
            }
        }
    }
}
