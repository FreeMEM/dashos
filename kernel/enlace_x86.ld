/* DashOS: guion de enlace del kernel en x86-64 (S21, X1 y X2).
 *
 * El cargador UEFI (uefi/src/x86_64.rs) pone cada segmento en su dirección física (la de AT, desde 1 MB, que OVMF deja
 * libre) y salta a la entrada en su dirección física. Las direcciones virtuales son las de la imagen del kernel en los
 * 2 GB de arriba (IMAGEN + física): el modelo de código "kernel" de x86-64 las quiere ahí. arranque.s entra con la
 * proyección 1:1 del firmware, pone las tablas de arranque y salta arriba.
 */
ENTRY(_start)
IMAGEN = 0xFFFFFFFF80000000;
SECTIONS
{
    . = IMAGEN + 0x100000;
    __kernel_inicio = .;
    .text : AT(ADDR(.text) - IMAGEN) { KEEP(*(.text.arranque)) *(.text .text.*) }
    . = ALIGN(4096);
    __texto_fin = .;
    .rodata : AT(ADDR(.rodata) - IMAGEN) ALIGN(4096) { *(.rodata .rodata.*) }
    . = ALIGN(4096);
    __rodata_fin = .;
    .data : AT(ADDR(.data) - IMAGEN) ALIGN(4096) { *(.data .data.*) *(.got .got.*) }
    /* .bss lleva también las tablas de páginas del arranque y las pilas: arranque.s lo pone todo a cero. Cada pila
     * tiene debajo una página de guarda, que mapa.rs no proyecta. */
    .bss (NOLOAD) : AT(ADDR(.bss) - IMAGEN) ALIGN(4096) {
        __bss_inicio = .;
        *(.bss .bss.*) *(COMMON)
        . = ALIGN(4096);
        __arranque_pml4 = .;
        . += 0x1000;
        __arranque_pdpt = .;
        . += 0x1000;
        __arranque_pd = .;
        . += 0x4000;
        __guarda_arranque = .;
        . += 0x1000;
        . += 0x10000;
        __pila_fin = .;
        __guarda_df = .;
        . += 0x1000;
        . += 0x4000;
        __pila_df_fin = .;
        __guarda_nmi = .;
        . += 0x1000;
        . += 0x4000;
        __pila_nmi_fin = .;
        __guarda_mc = .;
        . += 0x1000;
        . += 0x4000;
        __pila_mc_fin = .;
        __bss_fin = .;
    }
    __kernel_fin = .;
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
