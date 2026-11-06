ENTRY(_start)
SECTIONS
{
    . = 0x40200000;
    .text : { KEEP(*(.text.entrada)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.*) }
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
