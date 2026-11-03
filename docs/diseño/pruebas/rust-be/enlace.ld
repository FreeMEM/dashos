ENTRY(_start)
SECTIONS {
  . = 0x80000;
  .text : { KEEP(*(.text.boot)) *(.text*) }
  .rodata : { *(.rodata*) }
  .data : { *(.data*) }
  .bss : { *(.bss*) }
  . = ALIGN(16);
  . = . + 0x10000;
  __pila = .;
}
