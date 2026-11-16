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
    // Los programas comprimidos son los de la imagen de ARM (`src/inicio.rs`); la de x86 los lleva sin comprimir
    // (`src/inicio_x86.rs`).
    if !x86 {
        comprimir_programas(&dir);
    } else {
        firmware_rtl8125(&dir);
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

/// Los programas que nombra `src/inicio.rs` con `programa!("...")`, comprimidos con LZ4 en OUT_DIR (`nombre.lz4`): el
/// tamaño original (u32), su huella FNV-1a de 64 bits y el bloque. Los descomprime `src/lz4.rs` al arrancar. Uno que no
/// esté compilado se salta (el `programa!` que lo nombra no llega a usarse: es de otra placa o de otra prueba).
fn comprimir_programas(dir: &str) {
    let usuario = format!("{}/../usuario/target/aarch64-unknown-none-softfloat/release", dir);
    let fuente = std::fs::read_to_string(format!("{}/src/inicio.rs", dir)).unwrap();
    println!("cargo:rerun-if-changed=src/inicio.rs");
    let out = std::env::var("OUT_DIR").unwrap();
    let mut nombres: Vec<&str> = fuente.split("programa!(\"").skip(1).filter_map(|t| t.split('"').next()).collect();
    nombres.sort();
    nombres.dedup();
    for nombre in nombres {
        // H4: la caja Amiga es un programa de C aparte (`caja/`), no de `usuario/`.
        let ruta = if nombre == "caja-amiga" { format!("{}/../caja/build/{}", dir, nombre) } else { format!("{}/{}", usuario, nombre) };
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
