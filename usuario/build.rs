//! El controlador de la SD pide 128 KB de pila con PT_GNU_STACK, para probar que el cargador lo lee (C19).
fn main() {
    println!("cargo:rustc-link-arg-bin=sd=-zstack-size=131072");
    // RedoxFS pone bloques de 4 KB y registros en la pila: con 64 KB no llega a listar la raíz (FS1).
    println!("cargo:rustc-link-arg-bin=archivos=-zstack-size=1048576");
    // X7a: RelevoJIT (el crate `relevo`, característica "relevo"). Su intérprete no va hondo, pero el programa guarda en
    // la pila la tabla de resultados; 256 KB sobran.
    println!("cargo:rustc-link-arg-bin=relevo=-zstack-size=262144");
    // A15: el comprobador recorre el disco con la misma biblioteca.
    println!("cargo:rustc-link-arg-bin=comprobador=-zstack-size=1048576");
    // Paso 2 de las asignaciones: `sesiones` lleva la tabla de nombres de cada ficha (64 de 1 KB) en la pila.
    println!("cargo:rustc-link-arg-bin=sesiones=-zstack-size=524288");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=enlace.ld");
    println!("cargo:rerun-if-changed=enlace_x86.ld");
}
