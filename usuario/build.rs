//! El controlador de la SD pide 128 KB de pila con PT_GNU_STACK, para probar que el cargador lo lee (C19).
fn main() {
    println!("cargo:rustc-link-arg-bin=sd=-zstack-size=131072");
    // RedoxFS pone bloques de 4 KB y registros en la pila: con 64 KB no llega a listar la raíz (FS1).
    println!("cargo:rustc-link-arg-bin=archivos=-zstack-size=1048576");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=enlace.ld");
}
