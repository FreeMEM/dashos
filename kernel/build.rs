//! Elige el guion de enlace según la placa (U3): la Pi carga el kernel en 0x80000 y el cargador UEFI de QEMU `virt`,
//! en 0x40200000. En x86-64 (S30, X1), `enlace_x86.ld`: el cargador UEFI lo pone en 0x100000.
fn main() {
    let x86 = std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64");
    let guion = if x86 {
        "enlace_x86.ld"
    } else if std::env::var_os("CARGO_FEATURE_VIRT").is_some() {
        "enlace_virt.ld"
    } else {
        "enlace.ld"
    };
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=-T{}/{}", dir, guion);
    println!("cargo:rerun-if-changed=enlace.ld");
    println!("cargo:rerun-if-changed=enlace_virt.ld");
    println!("cargo:rerun-if-changed=enlace_x86.ld");
    // Los programas comprimidos son los de la imagen de ARM (`src/inicio.rs`) y los de la de usar de x86
    // (`src/inicio_x86_uso.rs`); la de las pruebas de x86 los lleva sin comprimir (`src/inicio_x86.rs`).
    if !x86 {
        comprimir_programas(&dir, "src/inicio.rs", "aarch64-unknown-none-softfloat");
    } else {
        firmware_rtl8125(&dir);
        if std::env::var_os("CARGO_FEATURE_USO").is_some() {
            // La imagen de usar de x86 (`make x86-uso`) lleva todo el escritorio: sin comprimir no cabe por debajo de lo
            // que ocupa OVMF desde 8 MB (el kernel de x86 se carga en 1 MB). Sus programas, como los de ARM.
            comprimir_programas(&dir, "src/inicio_x86_uso.rs", "x86_64-unknown-none");
        }
    }
    // La prueba de kit-c va en la imagen solo si se pudo compilar (hace falta Dash: `make kit-c`). Es de ARM.
    println!("cargo::rustc-check-cfg=cfg(dash_ventana)");
    if !x86 && std::path::Path::new(&format!("{}/../build/dash-ventana", dir)).exists() {
        println!("cargo:rustc-cfg=dash_ventana");
    }
}

/// X5c: el firmware del PHY de la RTL8125B para `src/inicio_x86.rs`. El Makefile lo deja descomprimido en
/// `target/firmware/rtl8125b-2.fw` (vacío si el sistema no lo tiene); sin él, uno vacío: el controlador sigue sin
/// firmware.
fn firmware_rtl8125(dir: &str) {
    let ruta = format!("{}/target/firmware/rtl8125b-2.fw", dir);
    println!("cargo:rerun-if-changed={}", ruta);
    let datos = std::fs::read(&ruta).unwrap_or_default();
    let out = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{}/rtl8125b-2.fw", out), datos).unwrap();
}

/// Los programas que nombra `fuente` (`src/inicio.rs`) con `programa!("...")`, de `usuario/target/<destino>`,
/// comprimidos con LZ4 en OUT_DIR (`nombre.lz4`): el tamaño original (u32), su huella FNV-1a de 64 bits y el bloque. Los
/// descomprime `src/lz4.rs` al arrancar. Uno que no esté compilado se salta (el `programa!` que lo nombra no llega a
/// usarse: es de otra placa o de otra prueba).
fn comprimir_programas(dir: &str, fuente: &str, destino: &str) {
    let usuario = format!("{}/../usuario/target/{}/release", dir, destino);
    println!("cargo:rerun-if-changed={}", fuente);
    let fuente = std::fs::read_to_string(format!("{}/{}", dir, fuente)).unwrap();
    let out = std::env::var("OUT_DIR").unwrap();
    let mut nombres: Vec<&str> = fuente.split("programa!(\"").skip(1).filter_map(|t| t.split('"').next()).collect();
    nombres.sort();
    nombres.dedup();
    for nombre in nombres {
        // H4: la caja Amiga es un programa de C aparte (`caja/`), no de `usuario/`.
        // La prueba de kit-c, un programa de Dash (`make kit-c`): en `build/`.
        // La demo (`make demo`) lleva la caja de las capturas, compilada aparte en caja/build-demo.
        let ruta = if nombre == "caja-amiga" && std::env::var_os("CARGO_FEATURE_DEMO").is_some() {
            format!("{}/../caja/build-demo/{}", dir, nombre)
        } else if nombre == "caja-amiga" {
            format!("{}/../caja/build/{}", dir, nombre)
        } else if nombre == "dash-ventana" {
            format!("{}/../build/{}", dir, nombre)
        } else {
            format!("{}/{}", usuario, nombre)
        };
        println!("cargo:rerun-if-changed={}", ruta);
        let Ok(datos) = std::fs::read(&ruta) else { continue };
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in &datos {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        let mut salida = Vec::with_capacity(datos.len() / 2);
        salida.extend_from_slice(&(datos.len() as u32).to_le_bytes());
        salida.extend_from_slice(&h.to_le_bytes());
        salida.extend_from_slice(&lz4_flex::block::compress(&datos));
        std::fs::write(format!("{}/{}.lz4", out, nombre), salida).unwrap();
    }
}
