/* DashOS: programas de usuario en x86-64 (X4). Como enlace.ld (se cargan en 0x400000, C19), pero la GOT pequeña que
   puede salir va con los datos, para que no quede en un segmento suelto que comparta página con .rodata. */
ENTRY(_start)
SECTIONS
{
    . = 0x400000;
    .text : { KEEP(*(.text._start)) *(.text .text.*) }
    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.*) }
    . = ALIGN(4096);
    .data : { *(.data .data.*) *(.got .got.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
