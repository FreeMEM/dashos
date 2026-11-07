/* DashOS (fase F1 de la caja PowerPC): el banco no lleva el ROM PPC de Emu68 (src/ppc/, que no se reutiliza por la
   sala limpia: S17-caja-powerpc.md, apartado 5). PPC_ExecutionLoop.cpp lo incluye; aquí queda vacío. */
uint8_t ppc_rom_img[4096];
uint32_t ppc_rom_img_len = 0;
