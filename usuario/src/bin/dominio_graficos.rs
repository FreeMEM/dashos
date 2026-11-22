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
//! - X13 (G6 de S38): la ventana compartida (`dashos::protocolo::compartida`). La página de control y los búferes que
//!   le dan los programas por el canal "graficos" se proyectan tal cual en la física del invitado (`maquina_memoria`),
//!   sin copia; un hilo (`compartida::atender`) atiende el canal y avisa a cada programa de lo que el dominio ya vio.
//!
//! Llaves (por su nombre): "hipervisor" (el permiso de máquinas), "gpu-dominio" (el aparato), "dominio-graficos" (el
//! kernel y el initramfs: el largo de cada uno, u64, y detrás, seguidos) y, si está, el canal "graficos" (X13). Sale con
//! 0 si el dominio acabó bien.

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
    use crate::dominio_x86::{Dominio, Extra};
    use alloc::string::String;
    use core::sync::atomic::Ordering;
    use dashos::protocolo::compartida as c;
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

    /// El PCI del aparato pasado y, además, el timbre de la ventana compartida (X13).
    struct ConTimbre<'a>(&'a mut PciPasado);

    impl Extra for ConTimbre<'_> {
        fn puerto(&mut self, puerto: u16, tam: u8, escribir: Option<u64>) -> Option<u64> {
            if puerto == c::PUERTO_AVISO {
                if escribir.is_some() {
                    crate::compartida::avisar_vistos();
                }
                return Some(0);
            }
            self.0.puerto(puerto, tam, escribir)
        }
        fn mmio(&mut self, dir: u64, tam: u8, escribir: Option<u64>) -> Option<u64> {
            self.0.mmio(dir, tam, escribir)
        }
        fn antes(&mut self, lapic: &mut caja_vmm::x86::apic::Lapic) {
            self.0.antes(lapic)
        }
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
        let mut linea = alloc::format!("{} tsc_early_khz={} dashos.graficos={}", LINEA, dashos::frecuencia() / 1000, SEGUNDOS);
        let graficos = tabla.buscar("graficos");
        if graficos.is_some() {
            linea += &alloc::format!(" dashos.compartida={:#x},{:#x}", c::VENTANA_GPA, c::VENTANA_TAM);
        }
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
        // X13: la página de control en el invitado y el hilo que atiende a los programas.
        let mut pila_compartida = alloc::vec![0u8; PILA];
        if let Some(g) = graficos {
            if let Err(e) = crate::compartida::preparar(m) {
                println!("  [dominio-graficos] la ventana compartida: {}", e);
                return 6;
            }
            if dashos::hilos::crear(crate::compartida::atender, g as u64 | (m as u64) << 32, &mut pila_compartida, 0) < 0 {
                println!("  [dominio-graficos] sin el hilo de la ventana compartida");
                return 6;
            }
            println!("  [dominio-graficos] ventana compartida en {:#x} del invitado ({} MB)", c::VENTANA_GPA, c::VENTANA_TAM >> 20);
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
        let r = dom.correr_con(PLAZO_US, &mut escribir, &mut ConTimbre(&mut pci));
        println!("  [dominio-graficos] interrupciones: {}", pci.estado());
        for a in &pci.avisos {
            println!("  [dominio-graficos] aviso: {}", a);
        }
        println!("  [dominio-graficos] {} salidas del invitado; {} interrupciones de la GPU entregadas", dom.salidas(), pci.entregadas);
        if graficos.is_some() {
            println!("  [dominio-graficos] ventana compartida: {}", crate::compartida::resumen());
        }
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

/// X13: la ventana compartida del lado del servicio (`dashos::protocolo::compartida`).
#[cfg(target_arch = "x86_64")]
mod compartida {
    use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use dashos::protocolo::compartida::{self as c, ranura as r};

    /// Donde se proyecta aquí la página de control.
    const VA_CONTROL: usize = 0x51_0000_0000;

    /// Cada ranura: el canal de respuestas de su programa (0 si libre) y lo último que se le dijo visto.
    static RESPUESTA: [AtomicU32; c::RANURAS] = [const { AtomicU32::new(0) }; c::RANURAS];
    static DICHO: [AtomicU64; c::RANURAS] = [const { AtomicU64::new(0) }; c::RANURAS];
    /// Dónde va el siguiente búfer dentro de la ventana.
    static SIGUIENTE: AtomicU64 = AtomicU64::new(c::ALINEADO);
    /// Para el resumen: búferes puestos, cuadros listos y avisos de visto.
    static PUESTAS: AtomicU64 = AtomicU64::new(0);
    static LISTOS: AtomicU64 = AtomicU64::new(0);
    static VISTOS: AtomicU64 = AtomicU64::new(0);

    fn campo32(i: usize, off: usize) -> &'static core::sync::atomic::AtomicU32 {
        // SAFETY: la página de control está proyectada en `VA_CONTROL` desde `preparar` y no se quita; el campo cae dentro
        // y alineado a 4.
        unsafe { &*((VA_CONTROL + c::RANURAS_DESDE + i * c::LARGO_RANURA + off) as *const core::sync::atomic::AtomicU32) }
    }

    fn campo64(i: usize, off: usize) -> &'static AtomicU64 {
        // SAFETY: como `campo32`, alineado a 8.
        unsafe { &*((VA_CONTROL + c::RANURAS_DESDE + i * c::LARGO_RANURA + off) as *const AtomicU64) }
    }

    /// La página de control: creada, con su cabecera, y en el invitado al principio de la ventana.
    pub fn preparar(m: u32) -> Result<(), &'static str> {
        let ctl = dashos::memoria_crear(c::CONTROL);
        if (ctl as i32) <= 0 || dashos::memoria_proyectar(ctl, VA_CONTROL, true) != 0 {
            return Err("sin memoria para la página de control");
        }
        // SAFETY: recién proyectada, de `CONTROL` bytes, para escribir.
        let cab = unsafe { core::slice::from_raw_parts_mut(VA_CONTROL as *mut u32, 4) };
        cab[1] = c::VERSION;
        cab[2] = c::RANURAS as u32;
        // La marca, lo último: con ella el dominio da la página por buena.
        // SAFETY: como arriba.
        unsafe { (*(VA_CONTROL as *const AtomicU32)).store(c::MAGIA, Ordering::Release) };
        if dashos::maquina_memoria(m, ctl, c::VENTANA_GPA) != 0 {
            return Err("la página de control no entra en el invitado");
        }
        Ok(())
    }

    fn responder(canal: u32, w: &[u32]) {
        let b: alloc::vec::Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        let _ = dashos::canal_escribir(canal, &b, &[]);
    }

    /// `PONER`: el búfer en la ventana, si cabe y vale; la ranura, o el código.
    fn poner(m: u32, w: &[u32; 5], mem: u32, canal: u32) -> i64 {
        let (ancho, alto, paso, formato) = (w[1], w[2], w[3], w[4]);
        let Some(i) = (0..c::RANURAS).find(|&i| RESPUESTA[i].load(Ordering::Acquire) == 0) else { return dashos::E_LIMITE };
        let tam = dashos::memoria_tamano(mem);
        if ancho == 0 || alto == 0 || ancho > 8192 || alto > 8192 || paso < ancho * 4 || formato > c::XRGB || tam < paso as i64 * alto as i64 {
            return dashos::E_ARGUMENTO;
        }
        let largo = (tam as u64).next_multiple_of(4096);
        let desde = SIGUIENTE.load(Ordering::Acquire);
        if desde + largo > c::VENTANA_TAM {
            return dashos::E_LIMITE;
        }
        let e = dashos::maquina_memoria(m, mem, c::VENTANA_GPA + desde);
        if e != 0 {
            return e;
        }
        SIGUIENTE.store((desde + largo).next_multiple_of(c::ALINEADO), Ordering::Release);
        campo32(i, r::ANCHO).store(ancho, Ordering::Relaxed);
        campo32(i, r::ALTO).store(alto, Ordering::Relaxed);
        campo32(i, r::PASO).store(paso, Ordering::Relaxed);
        campo64(i, r::DESPLAZAMIENTO).store(desde, Ordering::Relaxed);
        campo64(i, r::TAMANO).store(tam as u64, Ordering::Relaxed);
        campo64(i, r::SECUENCIA).store(0, Ordering::Relaxed);
        campo64(i, r::VISTO).store(0, Ordering::Relaxed);
        campo32(i, r::FORMATO).store(formato, Ordering::Relaxed);
        campo32(i, r::ESTADO).store(r::PUESTA, Ordering::Release);
        DICHO[i].store(0, Ordering::Relaxed);
        RESPUESTA[i].store(canal, Ordering::Release);
        PUESTAS.fetch_add(1, Ordering::Relaxed);
        i as i64
    }

    /// El timbre (`PUERTO_AVISO`, desde el hilo de la vCPU): a cada programa, lo que el dominio vio desde la última vez.
    pub fn avisar_vistos() {
        for i in 0..c::RANURAS {
            let canal = RESPUESTA[i].load(Ordering::Acquire);
            if canal == 0 {
                continue;
            }
            let visto = campo64(i, r::VISTO).load(Ordering::Acquire);
            if visto != DICHO[i].swap(visto, Ordering::AcqRel) {
                VISTOS.fetch_add(1, Ordering::Relaxed);
                responder(canal, &[c::VISTO, i as u32, visto as u32, (visto >> 32) as u32]);
            }
        }
    }

    /// El hilo del canal "graficos" (`arg`: la llave del canal y, arriba, la vCPU): atiende a los programas. Lo visto
    /// lo avisa el timbre (`avisar_vistos`).
    pub fn atender(arg: u64) -> u64 {
        let (canal, m) = (arg as u32, (arg >> 32) as u32);
        loop {
            let mut b = [0u8; 32];
            let mut ll = [0u32; 2];
            let (n, k, _) = dashos::canal_leer_plazo(canal, &mut b, &mut ll, 0);
            if n >= 4 {
                let n = (n as usize).min(b.len());
                let mut w = [0u32; 5];
                for (j, p) in w.iter_mut().enumerate().take(n / 4) {
                    *p = u32::from_le_bytes(b[4 * j..4 * j + 4].try_into().unwrap());
                }
                match w[0] {
                    c::PONER if n >= 20 && k == 2 => {
                        let res = poner(m, &w, ll[0], ll[1]);
                        responder(ll[1], &[c::PONER, res as u32]);
                        if res < 0 {
                            dashos::cerrar(ll[1]);
                        }
                    }
                    c::LISTO if n >= 16 && (w[1] as usize) < c::RANURAS && RESPUESTA[w[1] as usize].load(Ordering::Acquire) != 0 => {
                        let sec = w[2] as u64 | (w[3] as u64) << 32;
                        campo64(w[1] as usize, r::SECUENCIA).store(sec, Ordering::Release);
                        LISTOS.fetch_add(1, Ordering::Relaxed);
                    }
                    c::ACABAR if n >= 8 && (w[1] as usize) < c::RANURAS => {
                        campo32(w[1] as usize, r::ESTADO).store(r::ACABADA, Ordering::Release);
                    }
                    _ => {
                        for &l in &ll[..k.min(2)] {
                            dashos::cerrar(l);
                        }
                    }
                }
            } else if n == dashos::E_CERRADO {
                return 0;
            }
        }
    }

    pub fn resumen() -> alloc::string::String {
        let subir = (0..c::RANURAS).map(|i| campo64(i, r::SUBIR_US).load(Ordering::Relaxed)).sum::<u64>();
        alloc::format!(
            "{} búferes puestos, {} cuadros listos, {} avisos de visto; el dominio tardó {} ms en subirlos",
            PUESTAS.load(Ordering::Relaxed),
            LISTOS.load(Ordering::Relaxed),
            VISTOS.load(Ordering::Relaxed),
            subir / 1000
        )
    }
}
