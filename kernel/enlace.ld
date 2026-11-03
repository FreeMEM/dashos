/* DashOS: guion de enlace del kernel. El firmware de la Pi carga la imagen en 0x80000. */
ENTRY(_start)
SECTIONS
{
    . = 0x80000;
    __kernel_inicio = .;
    .text : { KEEP(*(.text.arranque)) *(.text .text.*) }
    .rodata : ALIGN(16) { *(.rodata .rodata.*) }
    .data : ALIGN(16) { *(.data .data.*) }
    .bss (NOLOAD) : ALIGN(16) { __bss_inicio = .; *(.bss .bss.*) *(COMMON) . = ALIGN(16); __bss_fin = .; }
    . = ALIGN(4096);
    /* Pilas de arranque: 64 KB por núcleo, cuatro núcleos. */
    __pilas_inicio = .;
    . = . + 4 * 0x10000;
    __pilas_fin = .;
    __kernel_fin = .;
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
