//! `memcpy` para los programas de usuario (paso F11 de docs/MULTITAREA_LOOP.md): 64 bytes por vuelta con `ldp`/`stp`.
//! El de `compiler_builtins` (`compiler-builtins-mem`) copia de palabra en palabra, y el servicio de archivos copia
//! bloques de 4 KB sin parar. Este manda: el de `compiler_builtins` es débil.
//!
//! Con origen y destino de distinta alineación hace accesos de 8 bytes sin alinear, que la memoria normal admite
//! (SCTLR_EL1.A a 0) y la de aparatos no: para registros, accesos `volatile` de su tamaño, nunca `memcpy`.

core::arch::global_asm!(
    ".section .text.memcpy,\"ax\",%progbits",
    ".global memcpy",
    ".type memcpy, %function",
    "memcpy:",
    "    mov x3, x0",
    "    cmp x2, #64",
    "    b.lo 2f",
    "1:  ldp x4, x5, [x1]",
    "    ldp x6, x7, [x1, #16]",
    "    ldp x8, x9, [x1, #32]",
    "    ldp x10, x11, [x1, #48]",
    "    add x1, x1, #64",
    "    sub x2, x2, #64",
    "    stp x4, x5, [x3]",
    "    stp x6, x7, [x3, #16]",
    "    stp x8, x9, [x3, #32]",
    "    stp x10, x11, [x3, #48]",
    "    add x3, x3, #64",
    "    cmp x2, #64",
    "    b.hs 1b",
    "2:  cmp x2, #8",
    "    b.lo 3f",
    "    ldr x4, [x1], #8",
    "    str x4, [x3], #8",
    "    sub x2, x2, #8",
    "    b 2b",
    "3:  cbz x2, 4f",
    "    ldrb w4, [x1], #1",
    "    strb w4, [x3], #1",
    "    sub x2, x2, #1",
    "    b 3b",
    "4:  ret",
    ".size memcpy, . - memcpy",
);
