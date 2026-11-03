Prueba de S1: kernel mínimo en Rust para AArch64 big-endian, en QEMU `raspi4b`.

    cargo +nightly build --release
    aarch64_be-linux-objcopy -O binary target/aarch64_be-unknown-none-softfloat/release/rust-be kernel.img
    qemu-system-aarch64 -M raspi4b -kernel kernel.img -dtb <bcm2711-rpi-4-b.dtb> -display none -serial stdio -serial null

Salida del 2026-11-03 (nightly 1.101.0 del 2026-11-01):

    DashOS: Rust en AArch64 big-endian
    Primer byte de 0x11223344 en memoria: 0000000000000011 (big-endian)
    ID_AA64MMFR0_EL1 = 0000000000001124
    BigEnd (bits 11:8) = 0000000000000001, BigEndEL0 (bits 19:16) = 0000000000000000

Añadido para S7: pone `SCTLR_EL1.E0E` (bit 24, orden de bytes de EL0) a 1 y lo vuelve a leer:

    SCTLR_EL1.E0E: antes 0000000000000000, tras ponerlo a 1 se lee 0000000000000001 (EE, bit 25 = 0000000000000000)

El bit se puede escribir: el modelo de A72 de QEMU admite procesos de usuario con su propio orden de bytes. No
prueba que un programa little-endian corra de verdad en EL0; para eso hace falta un kernel con EL0.
