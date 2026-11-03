/* DashOS: guion de enlace del kernel.
 *
 * El firmware de la Pi carga la imagen en la dirección física 0x80000 (LMA). El kernel está enlazado en la mitad
 * alta (VMA): la memoria física se ve entera a partir de KERNEL_VA (S3). arranque.s enciende la MMU antes de
 * entrar en Rust, así que todo el código de Rust corre ya en direcciones altas.
 */
ENTRY(_start)
KERNEL_VA = 0xFFFFFF8000000000;
SECTIONS
{
    . = KERNEL_VA + 0x80000;
    __kernel_inicio = .;
    .text : AT(ADDR(.text) - KERNEL_VA) { KEEP(*(.text.arranque)) *(.text .text.*) }
    .rodata : ALIGN(16) { *(.rodata .rodata.*) }
    .data : ALIGN(16) { *(.data .data.*) }
    .bss (NOLOAD) : ALIGN(16) { __bss_inicio = .; *(.bss .bss.*) *(COMMON) . = ALIGN(16); __bss_fin = .; }
    /* Tablas de páginas del arranque: nivel 1 y un nivel 2 para el cuarto GB. */
    .tablas (NOLOAD) : ALIGN(4096) { __tabla_l1 = .; . += 4096; __tabla_l2_g3 = .; . += 4096; }
    . = ALIGN(4096);
    /* Pilas de arranque: 64 KB por núcleo, cuatro núcleos. */
    __pilas_inicio = .;
    . = . + 4 * 0x10000;
    __pilas_fin = .;
    __kernel_fin = .;
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
