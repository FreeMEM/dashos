//! El controlador de la SD pide 128 KB de pila con PT_GNU_STACK, para probar que el cargador lo lee (C19).
fn main() {
    println!("cargo:rustc-link-arg-bin=sd=-zstack-size=131072");
    // RedoxFS pone bloques de 4 KB y registros en la pila: con 64 KB no llega a listar la raíz (FS1).
    println!("cargo:rustc-link-arg-bin=archivos=-zstack-size=1048576");
    // X7a: RelevoJIT compilado sin libc (`make freestanding` en su repositorio). El directorio de su librelevo.a lo da
    // `kernel/Makefile` en RELEVO_LIB_DIR; solo lo enlaza el programa `relevo` (característica "relevo").
    if let Ok(dir) = std::env::var("RELEVO_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rerun-if-changed={dir}/librelevo.a");
        println!("cargo:rustc-link-arg-bin=relevo=-zstack-size=1048576");
    }
    println!("cargo:rerun-if-env-changed=RELEVO_LIB_DIR");
    // A15: el comprobador recorre el disco con la misma biblioteca.
    println!("cargo:rustc-link-arg-bin=comprobador=-zstack-size=1048576");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=enlace.ld");
    println!("cargo:rerun-if-changed=enlace_x86.ld");
}
