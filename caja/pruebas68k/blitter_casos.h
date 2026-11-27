/*
    B1 (docs/CHIPS_LOOP.md): los blits de blitter.c. Los lee también la prueba de Rust de caja/chips
    (tests/blitter.rs), que hace los mismos y compara las sumas con las de FS-UAE: una línea por caso, sin nada más
    en la línea, con números enteros (decimales o 0x).

    Todo sobre un trozo de 4096 bytes de chip RAM. "relleno" es cómo se llena antes del blit: 0 a ceros, 1 al azar,
    2 escaso (pocos bits a 1), -1 no se toca (sigue lo del caso anterior). "semilla" es la del generador al azar.
    Los desplazamientos (aoff...) son en bytes desde el principio del trozo; -1, el puntero no se escribe (sigue
    donde lo dejó el blit anterior).

    COPIA(relleno, semilla, con0, con1, afwm, alwm, aoff, boff, coff, doff, amod, bmod, cmod, dmod, adat, bdat, cdat,
          size)
    LINEA(relleno, semilla, x1, y1, x2, y2, minterm, textura, inicio_textura, sing): una línea en un mapa de 64 bytes
          por fila desde el principio del trozo, con los registros que pide el manual (blitter.c, linea()).
*/
/* 1-3: copia simple A a D; con desplazamiento de A y máscaras; A, B y C con D (galleta), con desplazamientos. */
COPIA(1, 1, 0x09F0, 0x0000, 0xFFFF, 0xFFFF, 0, 0, 0, 2048, 24, 0, 0, 24, 0x0000, 0x0000, 0x0000, 0x0288)
COPIA(1, 2, 0x49F0, 0x0000, 0x0FFF, 0xF000, 8, 0, 0, 2050, 22, 0, 0, 22, 0x0000, 0x0000, 0x0000, 0x0309)
COPIA(1, 3, 0x3FCA, 0x3000, 0xFFFF, 0x0000, 0, 1024, 2048, 2048, 30, 30, 30, 30, 0x0000, 0x0000, 0x0000, 0x0405)
/* 4-5: descendente, solapado; descendente con las cuatro y máscaras. */
COPIA(1, 4, 0x59F0, 0x0002, 0xFFFF, 0xFFFF, 1598, 0, 0, 1638, 32, 0, 0, 32, 0x0000, 0x0000, 0x0000, 0x0304)
COPIA(1, 5, 0x7FE2, 0xB002, 0xF0F0, 0x3C3C, 1200, 2400, 3600, 3700, 10, 12, 14, 16, 0x0000, 0x0000, 0x0000, 0x0207)
/* 6-7: solo D, de BLTADAT con máscaras, sin desplazar y desplazada. */
COPIA(1, 6, 0x01F0, 0x0000, 0x0FF0, 0xF00F, 0, 0, 0, 100, 0, 0, 0, 10, 0xA5C3, 0x0000, 0x0000, 0x0185)
COPIA(1, 7, 0x51F0, 0x0000, 0x0FF0, 0xF00F, 0, 0, 0, 100, 0, 0, 0, 10, 0xA5C3, 0x0000, 0x0000, 0x0185)
/* 8-9: sin D (solo BZERO): A y C al azar; A y C a ceros. */
COPIA(1, 8, 0x0AA0, 0x0000, 0xFFFF, 0xFFFF, 0, 0, 2048, 0, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x0410)
COPIA(0, 0, 0x0AA0, 0x0000, 0xFFFF, 0xFFFF, 0, 0, 2048, 0, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x0410)
/* 10: una palabra de ancho (las dos máscaras en la misma). */
COPIA(1, 10, 0x29F0, 0x0000, 0x3FFC, 0xF0F0, 0, 0, 0, 2048, 38, 0, 0, 38, 0x0000, 0x0000, 0x0000, 0x0401)
/* 11-12: encadenados: el segundo no escribe punteros ni rellena (siguen los del primero). */
COPIA(1, 11, 0x09F0, 0x0000, 0xFFFF, 0xFFFF, 0, 0, 0, 2048, 8, 0, 0, 8, 0x0000, 0x0000, 0x0000, 0x0104)
COPIA(-1, 0, 0x09F0, 0x0000, 0xFFFF, 0xFFFF, -1, -1, -1, -1, 8, 0, 0, 8, 0x0000, 0x0000, 0x0000, 0x0104)
/* 13: sin USEB y con BSH: B es BLTBDAT, desplazada al escribirla. */
COPIA(1, 13, 0x0BC0, 0x7000, 0xFFFF, 0xFFFF, 0, 0, 1024, 2048, 0, 0, 0, 0, 0x0000, 0x1234, 0x0000, 0x0208)
/* 14: módulo negativo: cada fila lee la misma de A. */
COPIA(1, 14, 0x09F0, 0x0000, 0xFFFF, 0xFFFF, 0, 0, 0, 2048, -16, 0, 0, 24, 0x0000, 0x0000, 0x0000, 0x0308)
/* 15-16: otros minterms: A^B^C con desplazamientos; 0x1E en descendente con ASH 15. */
COPIA(1, 15, 0x9F96, 0xD000, 0xFFFF, 0xFFFF, 0, 1024, 2048, 3072, 2, 4, 6, 8, 0x0000, 0x0000, 0x0000, 0x0286)
COPIA(1, 16, 0xFF1E, 0x1002, 0x7FFF, 0xFFFE, 1800, 2800, 3800, 1000, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x0145)
/* 17-20: relleno en su sitio (A = D, descendente): inclusivo, exclusivo, inclusivo con FCI, exclusivo con FCI. */
COPIA(2, 17, 0x09F0, 0x000A, 0xFFFF, 0xFFFF, 1638, 0, 0, 1638, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x050A)
COPIA(2, 18, 0x09F0, 0x0012, 0xFFFF, 0xFFFF, 1638, 0, 0, 1638, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x050A)
COPIA(2, 19, 0x09F0, 0x000E, 0xFFFF, 0xFFFF, 1638, 0, 0, 1638, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x050A)
COPIA(2, 20, 0x09F0, 0x0016, 0xFFFF, 0xFFFF, 1638, 0, 0, 1638, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x050A)
/* 21-23: relleno ascendente; con A^C y desplazamiento; con IFE y EFE a la vez. */
COPIA(2, 21, 0x09F0, 0x0014, 0xFFFF, 0xFFFF, 0, 0, 0, 2048, 4, 0, 0, 4, 0x0000, 0x0000, 0x0000, 0x0406)
COPIA(2, 22, 0x6B5A, 0x000A, 0xFFF0, 0x0FFF, 1500, 0, 3000, 3500, 6, 0, 6, 6, 0x0000, 0x0000, 0x0000, 0x0407)
COPIA(2, 23, 0x09F0, 0x001A, 0xFFFF, 0xFFFF, 1638, 0, 0, 1638, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x050A)
/* 24: ancho 0 (64 palabras). */
COPIA(1, 24, 0x09F0, 0x0000, 0xFFFF, 0xFFFF, 0, 0, 0, 2048, 0, 0, 0, 0, 0x0000, 0x0000, 0x0000, 0x0100)
/* 25-32: líneas en los ocho octantes, desde (100, 16). */
LINEA(0, 0, 100, 16, 130, 20, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 130, 12, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 70, 20, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 70, 12, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 104, 30, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 104, 2, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 96, 30, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 100, 16, 96, 2, 0xCA, 0xFFFF, 0, 0)
/* 33-36: horizontal, vertical, a 45 grados y de un punto. */
LINEA(0, 0, 100, 16, 140, 16, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 200, 1, 200, 30, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 10, 0, 41, 31, 0xCA, 0xFFFF, 0, 0)
LINEA(0, 0, 5, 5, 5, 5, 0xCA, 0xFFFF, 0, 0)
/* 37-39: XOR sobre un fondo al azar; con SING (un punto por fila); otra encima de la anterior. */
LINEA(1, 37, 300, 3, 450, 28, 0x4A, 0xFFFF, 0, 0)
LINEA(0, 0, 300, 3, 450, 28, 0x4A, 0xFFFF, 0, 1)
LINEA(-1, 0, 450, 3, 300, 28, 0x4A, 0xFFFF, 0, 1)
/* 40-42: con textura. */
LINEA(0, 0, 20, 4, 220, 25, 0xCA, 0xF0F0, 0, 0)
LINEA(0, 0, 20, 4, 220, 25, 0xCA, 0xF0F0, 7, 0)
LINEA(0, 0, 400, 30, 380, 0, 0xCA, 0xCCCC, 15, 0)
