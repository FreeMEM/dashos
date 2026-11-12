/* DashOS, H4: la caja Amiga, un ET_EXEC en 0x7000_0000. Emu68 guarda en el código traducido las direcciones de
   sus funciones de C con 32 bits, así que el ejecutable va por debajo de 4 GB; y por encima de la memoria del 68k
   (que empieza en $0010_0000 y crecerá). Código, datos de solo lectura y datos, cada uno en sus páginas (W^X). */
ENTRY(_start)
PHDRS
{
    codigo PT_LOAD FLAGS(5);
    lectura PT_LOAD FLAGS(4);
    datos PT_LOAD FLAGS(6);
    pila PT_GNU_STACK FLAGS(6);
}
SECTIONS
{
    . = 0x70000000;
    .text : { KEEP(*(.text._start)) *(.text .text.*) } :codigo
    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.* .srodata .srodata.*) } :lectura
    . = ALIGN(4096);
    .data : { *(.data .data.* .got .got.plt) } :datos
    .bss : { *(.bss .bss.* COMMON) } :datos
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) *(.interp) *(.dynamic) *(.dynsym) *(.dynstr) *(.hash) *(.gnu.hash) }
}
