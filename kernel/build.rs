//! Elige el guion de enlace según la placa (U3): la Pi carga el kernel en 0x80000 y el cargador UEFI de QEMU `virt`,
//! en 0x40200000.
fn main() {
    let guion = if std::env::var_os("CARGO_FEATURE_VIRT").is_some() { "enlace_virt.ld" } else { "enlace.ld" };
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=-T{}/{}", dir, guion);
    println!("cargo:rerun-if-changed=enlace.ld");
    println!("cargo:rerun-if-changed=enlace_virt.ld");
    comprimir_programas(&dir);
    // La prueba de kit-c va en la imagen solo si se pudo compilar (hace falta Dash: `make kit-c`).
    println!("cargo::rustc-check-cfg=cfg(dash_ventana)");
    if std::path::Path::new(&format!("{}/../build/dash-ventana", dir)).exists() {
        println!("cargo:rustc-cfg=dash_ventana");
    }
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
        // La prueba de kit-c, un programa de Dash (`make kit-c`): en `build/`.
        let ruta = if nombre == "caja-amiga" {
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
