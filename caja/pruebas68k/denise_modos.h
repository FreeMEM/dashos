/*
    B4 (docs/CHIPS_LOOP.md): los modos de imagen de denise.c. Los lee también la prueba de Rust de caja/chips
    (tests/denise.rs), que pinta lo mismo y lo compara con la captura de FS-UAE: una línea por modo, sin nada más en la
    línea, con números enteros (decimales o 0x).

    MODO(bplcon0, bplcon1, bplcon2, bplcon2_abajo, ddfstrt, ddfstop, diwstrt, diwstop, bytes_fila, filas, bpl1mod,
         bpl2mod, dibujo)

    bytes_fila y filas son las del mapa de cada plano; bplcon2_abajo, el BPLCON2 que pone el Copper en la línea $AC.
    dibujo dice cómo se llena el mapa (denise.c, indice()): 0 diagonales; 1 diagonales finas (alta); 2 los 64 colores
    de EHB; 3 HAM (un color de la paleta y 15 cambios de un componente cada 16 píxeles); 4 campos dobles (el 1 en
    columnas, el 2 en filas); 5 diagonales para el desplazamiento fino.
*/
/* 1: baja, 5 planos, la pantalla de siempre. */
MODO(0x5200, 0x0000, 0x0000, 0x0000, 0x38, 0xD0, 0x2C81, 0x2CC1, 40, 256, 0, 0, 0)
/* 2: alta, 4 planos. */
MODO(0xC200, 0x0000, 0x0000, 0x0000, 0x3C, 0xD4, 0x2C81, 0x2CC1, 80, 256, 0, 0, 1)
/* 3: EHB, 6 planos. */
MODO(0x6200, 0x0000, 0x0000, 0x0000, 0x38, 0xD0, 0x2C81, 0x2CC1, 40, 256, 0, 0, 2)
/* 4: HAM6. */
MODO(0x6A00, 0x0000, 0x0000, 0x0000, 0x38, 0xD0, 0x2C81, 0x2CC1, 40, 256, 0, 0, 3)
/* 5: campos dobles, 3 + 3 planos; el campo 2 pasa delante en la línea $AC. */
MODO(0x6600, 0x0000, 0x0000, 0x0040, 0x38, 0xD0, 0x2C81, 0x2CC1, 40, 256, 0, 0, 4)
/* 6: baja, 3 planos, desplazamiento fino (5 el campo 1, 3 el 2), una palabra más de lectura con módulo, y otra
   ventana (líneas $40 a $F0, de $91 a $1B1). */
MODO(0x3200, 0x0035, 0x0000, 0x0000, 0x30, 0xD0, 0x4091, 0xF0B1, 44, 256, 2, 2, 5)
