/* DashOS, H4: la caja Amiga, un PIE estático (ET_DYN). El kernel la carga en una base al azar entre 128 y 256 GB, y su
   pila entre 256 y 384 GB (docs/referencia/kernel.md): nada de la caja queda en los 4 GB bajos, que son del 68k (S3,
   S28). Emu68 carga las direcciones de sus funciones de C con 64 bits (parche 0014 de Emu68). El kernel no
   aplica reubicaciones: entrada.S aplica las R_AARCH64_RELATIVE de .rela.dyn antes de nada. Por eso lo que lleva
   punteros (.data.rel.ro, .got) va con los datos, que se pueden escribir. Código, datos de solo lectura y datos, cada
   uno en sus páginas (W^X). */
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
    /* Enlazada en 0: con otra dirección, ld la marca ET_EXEC aunque sea PIE. */
    . = 0;
    .text : { __caja_imagen = .; KEEP(*(.text._start)) *(.text .text.*) } :codigo
    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.* .srodata .srodata.*) } :lectura
    .rela.dyn : { __rela_inicio = .; *(.rela.dyn .rela.*) __rela_fin = .; } :lectura
    /* Sin ellas ld no marca el ELF como ET_DYN; nadie las lee. */
    .dynsym : { *(.dynsym) } :lectura
    .dynstr : { *(.dynstr) } :lectura
    .hash : { *(.hash) } :lectura
    .gnu.hash : { *(.gnu.hash) } :lectura
    .dynamic : { *(.dynamic) } :lectura
    . = ALIGN(4096);
    .data : { *(.data.rel.ro .data.rel.ro.* .data .data.* .got .got.plt) } :datos
    .bss : { *(.bss .bss.* COMMON) __caja_fin = .; } :datos
    /* entrada.S toma la dirección de _start como base de carga. */
    ASSERT(_start == 0, "_start tiene que ir en 0 (entrada.S)")
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) *(.interp) }
}
