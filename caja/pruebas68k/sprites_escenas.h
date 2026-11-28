/*
    B6 (docs/CHIPS_LOOP.md): las escenas de sprites.c. Las lee también la prueba de Rust de caja/chips
    (tests/sprites_oraculo.rs), que pinta lo mismo, lee CLXDAT a la misma hora y lo compara con FS-UAE: una línea por
    escena o por sprite, sin nada más en la línea, con números enteros (decimales o 0x).

    ESCENA(bplcon0, bplcon2, bplcon2_abajo, ddfstrt, ddfstop, clxcon, dibujo, grupo, segundos)

    La ventana es siempre la de 320 x 256 ($2C81-$2CC1); el mapa de cada plano tiene 256 filas y las palabras que lee
    el DMA en una línea, sin módulo. bplcon2_abajo es el BPLCON2 que pone el Copper en la línea $AC. dibujo es el de
    los planos (sprites.c, indice()): 0 diagonales; 1 diagonales finas (alta); 3 HAM; 4 campos dobles; 6 bandas de
    32 píxeles (índice (x >> 5) y la máscara de los planos). grupo dice qué sprites salen. segundos: lo que se queda
    quieta para la captura; 0, solo para los choques (no sale en la captura).

    SPRITE(grupo, canal, hstart, vstart, alto, dibujo, attach)

    Los sprites de cada canal, en orden, son su lista (y al final, las dos palabras a 0). dibujo da las palabras de
    cada línea y del sprite (sprites.c, datos()): 0 color 1; 1 color 2; 2 color 3; 3 rayas (DATA rota $F0F0 con la
    línea, DATB $FF00); 4 DATA $AAAA, DATB $CCCC; 5 DATA $F0F0, DATB $FF00 (4 y 5, unidos, dan los 15 colores).
    attach pone el bit ATTACH en SPRxCTL (del impar).

    En el grupo 1, el Copper también: en las líneas $80 a $87, SPR1POS a su sitio en el ciclo $20 y a HSTART $100 en
    el $70 (el sprite 1 sale dos veces en la línea); en la $B0, el sprite 5 a mano (SPR5POS, SPR5CTL, SPR5DATB y
    SPR5DATA, que lo arma, en HSTART $110); en la $C0, SPR5CTL a 0 (lo desarma).
*/
/* 1: baja, 3 planos; sprites delante de todo arriba, detrás de todo abajo (BPLCON2 0). */
ESCENA(0x3200, 0x0024, 0x0000, 0x38, 0xD0, 0x0000, 0, 1, 4)
/* 2: PF2P 1 arriba (solo la pareja 0 delante), 3 abajo; choques de los impares (ENSP). */
ESCENA(0x3200, 0x0009, 0x001B, 0x38, 0xD0, 0xF000, 0, 1, 4)
/* 3: campos dobles, PF1P 1 y PF2P 3; abajo PF2PRI, PF1P 3 y PF2P 1. */
ESCENA(0x6600, 0x0019, 0x004B, 0x38, 0xD0, 0x0000, 4, 1, 4)
/* 4: alta, 4 planos. */
ESCENA(0xC200, 0x0024, 0x0012, 0x3C, 0xD4, 0x0000, 1, 1, 4)
/* 5: DDFSTRT $28: los planos se comen los ciclos de los sprites 5, 6 y 7. */
ESCENA(0x3200, 0x0024, 0x0024, 0x28, 0xD0, 0x0000, 0, 2, 4)
/* 6: HAM6; abajo, PF2P 1. */
ESCENA(0x6A00, 0x0024, 0x0009, 0x38, 0xD0, 0x0000, 3, 1, 4)
/* 7-10: choques con los campos, sobre bandas. Plano 1 igual a 1 y plano 2 igual a 1; con campos dobles; CLXCON 0;
   plano 1 igual a 0. */
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x00C3, 6, 3, 0)
ESCENA(0x2600, 0x0024, 0x0024, 0x38, 0xD0, 0x00C3, 6, 3, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 6, 3, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0040, 6, 3, 0)
/* 11-14: choques entre sprites: CLXCON 0; ENSP de los cuatro impares; ENSP3 y ENSP5; ENSP7. */
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 0, 4, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0xF000, 0, 4, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x6000, 0, 4, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x8000, 0, 4, 0)
/* 15-17: el sprite 6 solo, con CLXCON 0 (choca con todo), fuera de la ventana: encima, a la izquierda y a la
   derecha. 18-19: el sprite 6 solo, con DDFSTRT $28 (se pierde) y con $38. */
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 0, 5, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 0, 6, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 0, 7, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x28, 0xD0, 0x0000, 0, 8, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 0, 8, 0)
/* 20-22: sin sprites (el grupo 9 no tiene ninguno): CLXCON 0, ENBP1 y ENBP2 (los dos planos a 0). */
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0000, 0, 9, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0040, 0, 9, 0)
ESCENA(0x2200, 0x0024, 0x0024, 0x38, 0xD0, 0x0080, 0, 9, 0)

/* Grupo 1: de todo. El 0 se reutiliza tres veces (la tercera, en la línea siguiente al VSTOP de la segunda); el 1
   se cruza con el 0; el 2 y el 3, unidos, con el 4 (HSTART impar) por encima; el 6 empieza a la izquierda de la
   ventana y otro sale por la derecha; el 7 empieza encima de la ventana y otro sale por abajo. El 5, a mano. */
SPRITE(1, 0, 0x90, 0x40, 20, 3, 0)
SPRITE(1, 0, 0x95, 0x70, 10, 0, 0)
SPRITE(1, 0, 0x9C, 0x7B, 5, 1, 0)
SPRITE(1, 1, 0x98, 0x48, 20, 2, 0)
SPRITE(1, 1, 0xA0, 0x7E, 12, 3, 0)
SPRITE(1, 2, 0xC0, 0x50, 16, 4, 0)
SPRITE(1, 3, 0xC0, 0x50, 16, 5, 1)
SPRITE(1, 4, 0xC9, 0x58, 30, 2, 0)
SPRITE(1, 6, 0x7A, 0x90, 16, 0, 0)
SPRITE(1, 6, 0x1B8, 0xB0, 8, 2, 0)
SPRITE(1, 7, 0x130, 0x20, 40, 3, 0)
SPRITE(1, 7, 0x118, 0x118, 30, 1, 0)

/* Grupo 2: con DDFSTRT $28. El 4 no tiene nada (su segundo ciclo, el $27, es de los planos en OCS y no en AGA). */
SPRITE(2, 0, 0x90, 0x40, 16, 2, 0)
SPRITE(2, 1, 0xA0, 0x50, 16, 3, 0)
SPRITE(2, 2, 0xB0, 0x60, 16, 0, 0)
SPRITE(2, 3, 0xC0, 0x70, 16, 1, 0)
SPRITE(2, 5, 0x100, 0x60, 16, 2, 0)
SPRITE(2, 6, 0x110, 0x70, 16, 2, 0)
SPRITE(2, 7, 0x120, 0x80, 16, 2, 0)

/* Grupo 3: cada sprite par sobre una banda: el 0 en la 1 (plano 1 a 1, plano 2 a 0), el 2 en la 3 (los dos a 1), el
   4 en la 2 (solo el plano 2) y el 6 en la 0. */
SPRITE(3, 0, 0xA8, 0x60, 4, 2, 0)
SPRITE(3, 2, 0xE8, 0x60, 4, 2, 0)
SPRITE(3, 4, 0xC8, 0x60, 4, 2, 0)
SPRITE(3, 6, 0x88, 0x60, 4, 2, 0)

/* Grupo 4: el 0 con el 2; el 3 con el 5; el 4 con el 7; el 1 con el 6 encima de la ventana; el 2 con el 6 a la
   izquierda de la ventana. */
SPRITE(4, 0, 0xA0, 0x60, 4, 2, 0)
SPRITE(4, 1, 0x100, 0x20, 4, 2, 0)
SPRITE(4, 2, 0xA8, 0x60, 4, 2, 0)
SPRITE(4, 2, 0x60, 0x80, 4, 2, 0)
SPRITE(4, 3, 0xC0, 0x60, 4, 2, 0)
SPRITE(4, 4, 0xE0, 0x60, 4, 2, 0)
SPRITE(4, 5, 0xC4, 0x60, 4, 2, 0)
SPRITE(4, 6, 0x100, 0x20, 4, 2, 0)
SPRITE(4, 6, 0x60, 0x80, 4, 2, 0)
SPRITE(4, 7, 0xE8, 0x60, 4, 2, 0)

/* Grupos 5-8: el sprite 6 solo, encima de la ventana, a la izquierda, a la derecha y dentro. */
SPRITE(5, 6, 0x100, 0x20, 4, 2, 0)
SPRITE(6, 6, 0x60, 0x80, 4, 2, 0)
SPRITE(7, 6, 0x1C8, 0x80, 4, 2, 0)
SPRITE(8, 6, 0x110, 0x70, 16, 2, 0)
