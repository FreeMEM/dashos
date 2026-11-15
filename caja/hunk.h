/*
    DashOS, H4: cargador de ejecutables hunk de AmigaOS (el formato de LoadSeg).

    Escrito de la documentación pública del formato (The AmigaDOS Manual, capítulo de los ficheros objeto y de carga,
    y dos/doshunks.h del NDK): HUNK_HEADER, HUNK_CODE, HUNK_DATA, HUNK_BSS, HUNK_RELOC32, HUNK_RELOC32SHORT,
    HUNK_SYMBOL, HUNK_DEBUG, HUNK_NAME y HUNK_END. Las banderas de memoria (CHIP, FAST, o las dos y una palabra más con
    los atributos) se leen y se pasan a quien reserva, que aquí las ignora.

    Sin libc y sin llamadas al sistema: quien carga da la reserva de memoria y la traducción de direcciones del 68k
    (`struct hunk_ops`). Así se prueba igual en el PC (pruebas_hunk/) que en la caja. Todo lo que lee del fichero se
    comprueba antes: un fichero roto u hostil da un error, nunca escribe fuera de los hunks que reservó.
*/
#ifndef DASHOS_HUNK_H
#define DASHOS_HUNK_H

#include <stdint.h>
#include <stddef.h>

/* Los tipos de bloque (dos/doshunks.h). */
#define HUNK_UNIT          0x3E7
#define HUNK_NAME          0x3E8
#define HUNK_CODE          0x3E9
#define HUNK_DATA          0x3EA
#define HUNK_BSS           0x3EB
#define HUNK_RELOC32       0x3EC
#define HUNK_RELOC16       0x3ED
#define HUNK_RELOC8        0x3EE
#define HUNK_EXT           0x3EF
#define HUNK_SYMBOL        0x3F0
#define HUNK_DEBUG         0x3F1
#define HUNK_END           0x3F2
#define HUNK_HEADER        0x3F3
#define HUNK_OVERLAY       0x3F5
#define HUNK_BREAK         0x3F6
#define HUNK_DREL32        0x3F7
#define HUNK_RELOC32SHORT  0x3FC
#define HUNK_RELRELOC32    0x3FD

/* Bits altos de una palabra de tipo o de tamaño. */
#define HUNKF_ADVISORY (1u << 29)
#define HUNKF_CHIP     (1u << 30)
#define HUNKF_FAST     (1u << 31)

/* Errores (negativos). hunk_error() da su texto. */
enum {
    HUNK_BIEN = 0,
    HUNK_E_CORTO = -1,          /* el fichero se acaba a media estructura */
    HUNK_E_NO_ES_HUNK = -2,     /* no empieza por HUNK_HEADER */
    HUNK_E_LIBRERIAS = -3,      /* nombres de bibliotecas residentes: no se admiten en un ejecutable */
    HUNK_E_CUANTOS = -4,        /* número de hunks 0, absurdo o más de los que caben en la tabla */
    HUNK_E_PRIMERO = -5,        /* primer y último hunk que no cuadran con la tabla (overlays) */
    HUNK_E_TAMANO = -6,         /* un tamaño que no cuadra: datos más largos que el hunk, o un hunk enorme */
    HUNK_E_MEMORIA = -7,        /* quien reserva no da memoria */
    HUNK_E_TIPO = -8,           /* un bloque desconocido (sin la marca de "se puede saltar") */
    HUNK_E_RELOC = -9,          /* una reubicación a un hunk que no existe o fuera del hunk */
    HUNK_E_ORDEN = -10,         /* un bloque fuera de sitio: reubicación sin hunk, dos contenidos, END sin hunk */
    HUNK_E_SOBRAN = -11,        /* más hunks de los que dice la cabecera */
    HUNK_E_FALTAN = -12,        /* el fichero acaba antes de cargar todos */
    HUNK_E_OVERLAY = -13,       /* HUNK_OVERLAY o HUNK_BREAK: no se admiten */
};

/* Lo que pone quien carga. */
struct hunk_ops {
    void *ctx;
    /* Reserva `bytes` (múltiplo de 4, ya con los 8 de la cabecera del segmento) en la memoria del 68k. `atributos` son
       las banderas de la cabecera (HUNKF_CHIP, HUNKF_FAST) o los atributos MEMF_ de la palabra extra. Devuelve la
       dirección del 68k, alineada a 4, o 0 si no hay. */
    uint32_t (*reservar)(void *ctx, uint32_t bytes, uint32_t atributos);
    /* Devuelve lo reservado (al fallar la carga a medias, o con hunk_soltar). */
    void (*liberar)(void *ctx, uint32_t direccion, uint32_t bytes);
    /* Dónde ve el anfitrión [direccion, direccion + bytes) del 68k, o NULL si no es memoria del 68k. */
    uint8_t *(*ver)(void *ctx, uint32_t direccion, uint32_t bytes);
};

/* Un hunk cargado: dónde empieza su contenido (lo que ve el programa) y cuánto mide. */
struct hunk_seg {
    uint32_t direccion;   /* del contenido, tras la cabecera de 8 bytes del segmento */
    uint32_t bytes;       /* del contenido (tamaño de la cabecera del fichero × 4) */
    uint32_t tipo;        /* HUNK_CODE, HUNK_DATA o HUNK_BSS */
};

struct hunk_carga {
    uint32_t cuantos;       /* hunks cargados */
    uint32_t seglist;       /* BPTR al primer segmento, como lo da LoadSeg */
    uint32_t entrada;       /* dirección de la primera instrucción: el contenido del primer hunk */
    uint32_t relocs;        /* reubicaciones aplicadas */
    struct hunk_seg *segs;  /* la tabla que dio quien llama, con `cuantos` entradas */
};

/* Carga `fichero` (`largo` bytes). `segs` es una tabla con sitio para `max` hunks. Devuelve HUNK_BIEN y llena `carga`,
   o un error negativo; si falla, devuelve todo lo que reservó. */
int hunk_cargar(const uint8_t *fichero, size_t largo, const struct hunk_ops *ops, struct hunk_seg *segs, uint32_t max,
                struct hunk_carga *carga);

/* Devuelve la memoria de una carga que fue bien. */
void hunk_soltar(const struct hunk_ops *ops, struct hunk_carga *carga);

const char *hunk_error(int e);

#endif
