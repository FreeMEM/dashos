/* Las pruebas PPC (F1): un binario plano en 0x00140000 que empieza por prueba(), en .text.prueba. */
ENTRY(prueba)
SECTIONS
{
    . = 0x00140000;
    .text : { *(.text.prueba) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) *(.sdata .sdata.*) }
    .bss : { *(.sbss .sbss.*) *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) *(.gnu.attributes) }
}
