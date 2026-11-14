/* DashOS: guion de enlace del kernel en x86-64 (S21, X1).
 *
 * El cargador UEFI (uefi/src/x86_64.rs) lee el ELF y pone cada segmento en su dirección física, que en X1 es también
 * la virtual: el kernel corre con las tablas de páginas del firmware, que proyectan la memoria 1:1. Va en 1 MB, que
 * OVMF deja libre. La mitad alta, como en ARM, llega con las tablas de páginas propias (X2).
 */
ENTRY(_start)
SECTIONS
{
    . = 0x100000;
    __kernel_inicio = .;
    .text : { KEEP(*(.text.arranque)) *(.text .text.*) }
    . = ALIGN(4096);
    .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
    . = ALIGN(4096);
    .data : ALIGN(4096) { *(.data .data.*) *(.got .got.*) }
    /* .bss lleva también la pila de arranque, de 64 KB: arranque.s lo pone todo a cero. */
    .bss (NOLOAD) : ALIGN(4096) {
        __bss_inicio = .;
        *(.bss .bss.*) *(COMMON)
        . = ALIGN(16);
        __pila_inicio = .;
        . += 0x10000;
        __pila_fin = .;
        __bss_fin = .;
    }
    __kernel_fin = .;
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
