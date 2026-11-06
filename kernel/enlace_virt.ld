/* DashOS: guion de enlace del kernel.
 *
 * QEMU virt (U3): el cargador UEFI (uefi/) pone la imagen en la dirección física 0x40200000 (LMA); la RAM empieza en
 * 1 GB. Lo demás, como en enlace.ld (la Pi), que carga en 0x80000. El kernel está enlazado en la mitad
 * alta (VMA): la memoria física se ve entera a partir de KERNEL_VA (S3). arranque.s enciende la MMU antes de
 * entrar en Rust, así que todo el código de Rust corre ya en direcciones altas.
 */
ENTRY(_start)
KERNEL_VA = 0xFFFFFF8000000000;
SECTIONS
{
    . = KERNEL_VA + 0x40200000;
    __kernel_inicio = .;
    /* Cada sección empieza en página propia: el mapa del kernel (mapa.rs, C34) les da permisos distintos. */
    .text : AT(ADDR(.text) - KERNEL_VA) { KEEP(*(.text.arranque)) *(.text .text.*) }
    . = ALIGN(4096);
    __texto_fin = .;
    .rodata : ALIGN(4096) { *(.rodata .rodata.*) }
    . = ALIGN(4096);
    __rodata_fin = .;
    .data : ALIGN(4096) { *(.data .data.*) }
    .bss (NOLOAD) : ALIGN(16) { __bss_inicio = .; *(.bss .bss.*) *(COMMON) . = ALIGN(16); __bss_fin = .; }
    /* Tablas de páginas del arranque: nivel 1 y un nivel 2 para el cuarto GB. */
    .tablas (NOLOAD) : ALIGN(4096) { __tabla_l1 = .; . += 4096; __tabla_l2_g3 = .; . += 4096; }
    /* Pilas de arranque: una ranura de 32 KB por núcleo, alineada a 32 KB, como las de los hilos (mapa.rs, C34): la
       mitad de abajo es la guarda y la de arriba, 16 KB de pila. */
    . = ALIGN(0x8000);
    __pilas_inicio = .;
    . = . + 4 * 0x8000;
    __pilas_fin = .;
    __kernel_fin = .;
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
