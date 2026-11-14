//! Elige el guion de enlace según la placa (U3): la Pi carga el kernel en 0x80000 y el cargador UEFI de QEMU `virt`,
//! en 0x40200000. En x86-64 (S21, X1), `enlace_x86.ld`: el cargador UEFI lo pone en 0x100000.
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
}
