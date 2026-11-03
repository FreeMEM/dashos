/* DashOS: programas de usuario. Se cargan en 0x400000 (C19). */
ENTRY(_start)
SECTIONS
{
    . = 0x400000;
    .text : { KEEP(*(.text._start)) *(.text .text.*) }
    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.*) }
    . = ALIGN(4096);
    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.*) *(COMMON) }
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
