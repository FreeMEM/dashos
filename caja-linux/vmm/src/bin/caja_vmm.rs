//! `caja-vmm preparar`: prepara un arranque de Linux con el DTB y la carga de este crate, para probarlo en el PC
//! (`tools/caja_linux_invitado.py probar` lo usa con QEMU).
//!
//! Deja en `--salida`: `kernel.bin`, `initrd.bin`, `invitado.dtb`, `trampolin.bin` y `plan.json` con la dirección de
//! cada fichero y el PC de arranque (el trampolín).

use std::path::PathBuf;
use std::process::exit;

use caja_vmm::arbol::{Aparato, ConfigInvitado, Gic, generar_dtb};
use caja_vmm::arranque::{leer_cabecera, planificar, trampolin};

fn num(s: &str) -> u64 {
    let r = if let Some(h) = s.strip_prefix("0x") { u64::from_str_radix(h, 16) } else { s.parse() };
    r.unwrap_or_else(|_| fallo(&format!("número malo: {s}")))
}

fn par(s: &str) -> (u64, u64) {
    let (a, b) = s.split_once(':').unwrap_or_else(|| fallo(&format!("se esperaba a:b, no {s}")));
    (num(a), num(b))
}

fn fallo(m: &str) -> ! {
    eprintln!("caja-vmm: {m}");
    exit(1)
}

fn uso() -> ! {
    fallo(
        "uso: caja-vmm preparar --kernel Image [--initrd f] --memoria base:tam [--cpus n] \
         --gic v2:dist:cpu|v3:dist:redist [--uart base:spi] [--virtio base:spi]... [--bootargs t] --salida dir",
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("preparar") {
        uso();
    }
    let mut kernel = None;
    let mut initrd = None;
    let mut memoria = None;
    let mut cpus = 1u32;
    let mut gic = None;
    let mut uart = None;
    let mut virtio = Vec::new();
    let mut bootargs = String::from("console=hvc0 earlycon");
    let mut salida = None;
    let mut it = args[1..].iter();
    while let Some(op) = it.next() {
        let mut valor = || it.next().cloned().unwrap_or_else(|| uso());
        match op.as_str() {
            "--kernel" => kernel = Some(PathBuf::from(valor())),
            "--initrd" => initrd = Some(PathBuf::from(valor())),
            "--memoria" => memoria = Some(par(&valor())),
            "--cpus" => cpus = num(&valor()) as u32,
            "--gic" => {
                let v = valor();
                let p: Vec<&str> = v.split(':').collect();
                gic = Some(match p.as_slice() {
                    ["v2", d, c] => Gic::V2 { distribuidor: num(d), cpu: num(c), tam_cpu: caja_vmm::gic::TAM_CPU },
                    ["v3", d, r] => Gic::V3 { distribuidor: num(d), redistribuidores: num(r) },
                    _ => uso(),
                });
            }
            "--uart" => {
                let (b, s) = par(&valor());
                uart = Some(Aparato { base: b, tam: caja_vmm::pl011::TAM_REGION, spi: s as u32 });
            }
            "--virtio" => {
                let (b, s) = par(&valor());
                virtio.push(Aparato { base: b, tam: caja_vmm::virtio::mmio::TAM_REGION, spi: s as u32 });
            }
            "--bootargs" => bootargs = valor(),
            "--salida" => salida = Some(PathBuf::from(valor())),
            _ => uso(),
        }
    }
    let (Some(kernel), Some(memoria), Some(gic), Some(salida)) = (kernel, memoria, gic, salida) else { uso() };
    let leer = |p: &PathBuf| std::fs::read(p).unwrap_or_else(|e| fallo(&format!("{}: {e}", p.display())));
    let imagen = leer(&kernel);
    let datos_initrd = initrd.as_ref().map(leer);
    let cab = leer_cabecera(&imagen).unwrap_or_else(|e| fallo(&format!("Image: {e:?}")));
    let plan = planificar(&cab, imagen.len() as u64, memoria, datos_initrd.as_ref().map(|d| d.len() as u64))
        .unwrap_or_else(|e| fallo(&format!("plan: {e:?}")));
    let mut semilla = [0u8; 32];
    {
        use std::io::Read;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            let _ = f.read_exact(&mut semilla);
        }
    }
    let config = ConfigInvitado {
        memoria: vec![memoria],
        cpus,
        gic,
        uart,
        virtio,
        bootargs,
        initrd: plan.initrd,
        semilla: Some(semilla),
        reservas: Vec::new(),
    };
    let dtb = generar_dtb(&config);
    if dtb.len() as u64 > plan.dtb_max {
        fallo("el DTB no cabe");
    }
    // El trampolín va en el bloque de 2 MiB del DTB, detrás de él: solo se usa antes de saltar al kernel.
    let pc = plan.dtb + (dtb.len() as u64).next_multiple_of(64);
    std::fs::create_dir_all(&salida).unwrap_or_else(|e| fallo(&e.to_string()));
    let escribir =
        |n: &str, d: &[u8]| std::fs::write(salida.join(n), d).unwrap_or_else(|e| fallo(&format!("{n}: {e}")));
    escribir("kernel.bin", &imagen);
    escribir("invitado.dtb", &dtb);
    escribir("trampolin.bin", &trampolin(plan.dtb, plan.kernel));
    let mut trozos = vec![
        format!(r#"{{"fichero": "kernel.bin", "direccion": {}}}"#, plan.kernel),
        format!(r#"{{"fichero": "invitado.dtb", "direccion": {}}}"#, plan.dtb),
        format!(r#"{{"fichero": "trampolin.bin", "direccion": {pc}}}"#),
    ];
    if let (Some(d), Some((ini, _))) = (&datos_initrd, plan.initrd) {
        escribir("initrd.bin", d);
        trozos.push(format!(r#"{{"fichero": "initrd.bin", "direccion": {ini}}}"#));
    }
    let json = format!(
        "{{\"pc\": {pc}, \"kernel\": {}, \"dtb\": {}, \"text_offset\": {}, \"image_size\": {}, \"trozos\": [{}]}}\n",
        plan.kernel,
        plan.dtb,
        cab.text_offset,
        cab.image_size,
        trozos.join(", ")
    );
    escribir("plan.json", json.as_bytes());
    println!(
        "kernel en {:#x} (hasta {:#x}), DTB en {:#x} ({} bytes), initrd en {:?}, trampolín en {pc:#x}",
        plan.kernel,
        plan.fin_kernel,
        plan.dtb,
        dtb.len(),
        plan.initrd.map(|(a, b)| (format!("{a:#x}"), format!("{b:#x}")))
    );
}
