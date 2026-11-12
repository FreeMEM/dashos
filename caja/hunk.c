/*
    DashOS, H4: cargador de ejecutables hunk de AmigaOS. Ver hunk.h.

    Lo que hace, de la documentación del formato:
    - HUNK_HEADER: nombres de bibliotecas residentes (en un ejecutable, ninguno: un 0), el número de hunks, el primero y
      el último, y el tamaño de cada uno en palabras largas, con las banderas de memoria en los bits 30 y 31 (las dos a
      la vez: sigue una palabra con los atributos).
    - Cada hunk, en orden: HUNK_CODE o HUNK_DATA (tamaño y contenido) o HUNK_BSS (solo tamaño); después, sus
      reubicaciones (HUNK_RELOC32: grupos de "cuántas, a qué hunk, desplazamientos", acabados en 0; HUNK_RELOC32SHORT,
      lo mismo con palabras de 16 bits y relleno hasta la palabra larga), símbolos y depuración (se saltan), y HUNK_END.
    - Cada hunk va en un bloque con 8 bytes delante: su tamaño y el BPTR al siguiente (la lista de segmentos de
      LoadSeg). Lo que no llena el contenido queda a 0.
    Todas las cuentas van en 64 bits o se comprueban antes: ningún número del fichero hace que se lea fuera de él ni
    que se escriba fuera de un hunk reservado.
*/

#include "hunk.h"

struct lector {
    const uint8_t *p;
    size_t largo;
    size_t pos;
};

static size_t quedan(const struct lector *l) { return l->largo - l->pos; }

static int lee32(struct lector *l, uint32_t *v)
{
    if (quedan(l) < 4) return 0;
    const uint8_t *b = l->p + l->pos;
    *v = (uint32_t)b[0] << 24 | (uint32_t)b[1] << 16 | (uint32_t)b[2] << 8 | b[3];
    l->pos += 4;
    return 1;
}

static int lee16(struct lector *l, uint32_t *v)
{
    if (quedan(l) < 2) return 0;
    const uint8_t *b = l->p + l->pos;
    *v = (uint32_t)b[0] << 8 | b[1];
    l->pos += 2;
    return 1;
}

/* Salta `n` palabras largas, si las hay. */
static int salta(struct lector *l, uint64_t n)
{
    if (n > quedan(l) / 4) return 0;
    l->pos += (size_t)n * 4;
    return 1;
}

static uint32_t be32(const uint8_t *b) { return (uint32_t)b[0] << 24 | (uint32_t)b[1] << 16 | (uint32_t)b[2] << 8 | b[3]; }
static void pon_be32(uint8_t *b, uint32_t v) { b[0] = v >> 24; b[1] = v >> 16; b[2] = v >> 8; b[3] = v; }

const char *hunk_error(int e)
{
    switch (e) {
        case HUNK_BIEN: return "bien";
        case HUNK_E_CORTO: return "el fichero se acaba a media estructura";
        case HUNK_E_NO_ES_HUNK: return "no empieza por HUNK_HEADER";
        case HUNK_E_LIBRERIAS: return "pide bibliotecas residentes";
        case HUNK_E_CUANTOS: return "número de hunks absurdo";
        case HUNK_E_PRIMERO: return "primer y último hunk que no cuadran (overlay)";
        case HUNK_E_TAMANO: return "un tamaño que no cuadra";
        case HUNK_E_MEMORIA: return "sin memoria para los hunks";
        case HUNK_E_TIPO: return "un bloque desconocido";
        case HUNK_E_RELOC: return "una reubicación fuera de sitio";
        case HUNK_E_ORDEN: return "un bloque fuera de orden";
        case HUNK_E_SOBRAN: return "más hunks de los que dice la cabecera";
        case HUNK_E_FALTAN: return "faltan hunks";
        case HUNK_E_OVERLAY: return "overlays: no se admiten";
        default: return "error desconocido";
    }
}

static void suelta(const struct hunk_ops *ops, struct hunk_seg *segs, uint32_t n)
{
    for (uint32_t i = 0; i < n; i++)
        if (segs[i].direccion) ops->liberar(ops->ctx, segs[i].direccion - 8, segs[i].bytes + 8);
}

void hunk_soltar(const struct hunk_ops *ops, struct hunk_carga *carga)
{
    suelta(ops, carga->segs, carga->cuantos);
    carga->cuantos = 0;
    carga->seglist = 0;
    carga->entrada = 0;
}

/* Reubicaciones de 32 bits sobre el hunk `s`. `corto`: con palabras de 16 bits (HUNK_RELOC32SHORT). */
static int reubica(struct lector *l, const struct hunk_ops *ops, struct hunk_seg *segs, uint32_t cuantos,
                   struct hunk_seg *s, int corto, uint32_t *total)
{
    uint8_t *base = ops->ver(ops->ctx, s->direccion, s->bytes);
    if (!base && s->bytes) return HUNK_E_MEMORIA;
    for (;;) {
        uint32_t n, h;
        if (!(corto ? lee16(l, &n) : lee32(l, &n))) return HUNK_E_CORTO;
        if (n == 0) break;
        if (!(corto ? lee16(l, &h) : lee32(l, &h))) return HUNK_E_CORTO;
        if (h >= cuantos) return HUNK_E_RELOC;
        /* Antes de recorrerlos, que estén todos en el fichero. */
        if ((uint64_t)n * (corto ? 2 : 4) > quedan(l)) return HUNK_E_CORTO;
        for (uint32_t i = 0; i < n; i++) {
            uint32_t d;
            if (!(corto ? lee16(l, &d) : lee32(l, &d))) return HUNK_E_CORTO;
            if (s->bytes < 4 || d > s->bytes - 4) return HUNK_E_RELOC;
            pon_be32(base + d, be32(base + d) + segs[h].direccion);
        }
        *total += n;
    }
    /* Las palabras cortas se rellenan hasta la palabra larga (el bloque empieza alineado a 4). */
    if (corto && l->pos % 4) {
        uint32_t relleno;
        if (!lee16(l, &relleno)) return HUNK_E_CORTO;
    }
    return HUNK_BIEN;
}

int hunk_cargar(const uint8_t *fichero, size_t largo, const struct hunk_ops *ops, struct hunk_seg *segs, uint32_t max,
                struct hunk_carga *carga)
{
    struct lector l = { fichero, largo, 0 };
    uint32_t v, cuantos, primero, ultimo;
    int e = HUNK_BIEN;

    carga->cuantos = 0;
    carga->seglist = 0;
    carga->entrada = 0;
    carga->relocs = 0;
    carga->segs = segs;

    if (!lee32(&l, &v)) return HUNK_E_CORTO;
    if (v != HUNK_HEADER) return HUNK_E_NO_ES_HUNK;
    /* Bibliotecas residentes: en un ejecutable, la lista va vacía. */
    if (!lee32(&l, &v)) return HUNK_E_CORTO;
    if (v != 0) return HUNK_E_LIBRERIAS;
    if (!lee32(&l, &cuantos) || !lee32(&l, &primero) || !lee32(&l, &ultimo)) return HUNK_E_CORTO;
    if (cuantos == 0 || cuantos > max) return HUNK_E_CUANTOS;
    if (primero != 0 || ultimo != cuantos - 1) return HUNK_E_PRIMERO;

    /* Los tamaños, y la memoria de cada hunk. */
    for (uint32_t i = 0; i < cuantos; i++) segs[i].direccion = 0;
    for (uint32_t i = 0; i < cuantos; i++) {
        uint32_t atributos;
        if (!lee32(&l, &v)) { e = HUNK_E_CORTO; goto mal; }
        atributos = v & (HUNKF_CHIP | HUNKF_FAST);
        if (atributos == (HUNKF_CHIP | HUNKF_FAST) && !lee32(&l, &atributos)) { e = HUNK_E_CORTO; goto mal; }
        uint64_t bytes = (uint64_t)(v & 0x3FFFFFFF) * 4;
        if (bytes + 8 > 0xFFFFFFF0u) { e = HUNK_E_TAMANO; goto mal; }
        uint32_t d = ops->reservar(ops->ctx, (uint32_t)bytes + 8, atributos);
        uint8_t *m = d ? ops->ver(ops->ctx, d, (uint32_t)bytes + 8) : 0;
        if (!d || d % 4 || !m) {
            if (d) ops->liberar(ops->ctx, d, (uint32_t)bytes + 8);
            e = HUNK_E_MEMORIA;
            goto mal;
        }
        for (uint64_t k = 0; k < bytes + 8; k++) m[k] = 0;
        pon_be32(m, (uint32_t)bytes + 8);
        segs[i].direccion = d + 8;
        segs[i].bytes = (uint32_t)bytes;
        segs[i].tipo = 0;
    }
    /* La lista de segmentos: cada uno apunta (BPTR) a la palabra "siguiente" del otro. */
    for (uint32_t i = 0; i < cuantos; i++) {
        uint8_t *m = ops->ver(ops->ctx, segs[i].direccion - 8, 8);
        pon_be32(m + 4, i + 1 < cuantos ? (segs[i + 1].direccion - 4) >> 2 : 0);
    }

    /* Los hunks, uno tras otro. */
    uint32_t cargados = 0;
    struct hunk_seg *abierto = 0;   /* el hunk con contenido y aún sin HUNK_END */
    while (quedan(&l) > 0) {
        uint32_t palabra, n;
        if (!lee32(&l, &palabra)) { e = HUNK_E_CORTO; goto mal; }
        uint32_t tipo = palabra & 0x1FFFFFFF;
        switch (tipo) {
            case HUNK_CODE:
            case HUNK_DATA:
            case HUNK_BSS: {
                if (abierto) { e = HUNK_E_ORDEN; goto mal; }
                if (cargados == cuantos) { e = HUNK_E_SOBRAN; goto mal; }
                struct hunk_seg *s = &segs[cargados];
                if (!lee32(&l, &n)) { e = HUNK_E_CORTO; goto mal; }
                n &= 0x3FFFFFFF;
                if ((uint64_t)n * 4 > s->bytes) { e = HUNK_E_TAMANO; goto mal; }
                if (tipo != HUNK_BSS) {
                    if ((uint64_t)n * 4 > quedan(&l)) { e = HUNK_E_CORTO; goto mal; }
                    uint8_t *m = ops->ver(ops->ctx, s->direccion, n * 4);
                    if (!m && n) { e = HUNK_E_MEMORIA; goto mal; }
                    for (uint32_t k = 0; k < n * 4; k++) m[k] = l.p[l.pos + k];
                    l.pos += (size_t)n * 4;
                }
                s->tipo = tipo;
                abierto = s;
                break;
            }
            case HUNK_RELOC32:
            case HUNK_RELOC32SHORT:
            case HUNK_DREL32:   /* en un fichero de carga, lo mismo que HUNK_RELOC32SHORT (AmigaOS 2.0) */
                if (!abierto) { e = HUNK_E_ORDEN; goto mal; }
                e = reubica(&l, ops, segs, cuantos, abierto, tipo != HUNK_RELOC32, &carga->relocs);
                if (e) goto mal;
                break;
            case HUNK_SYMBOL:
                /* Grupos de "largo del nombre (con el tipo en el byte alto), nombre, valor", acabados en 0. */
                for (;;) {
                    if (!lee32(&l, &n)) { e = HUNK_E_CORTO; goto mal; }
                    if (n == 0) break;
                    if (!salta(&l, (uint64_t)(n & 0x00FFFFFF) + 1)) { e = HUNK_E_CORTO; goto mal; }
                }
                break;
            case HUNK_DEBUG:
            case HUNK_NAME:
                if (!lee32(&l, &n) || !salta(&l, n)) { e = HUNK_E_CORTO; goto mal; }
                break;
            case HUNK_END:
                /* Un HUNK_END sin contenido delante no cierra nada y se ignora: el ld de m68k-amigaos-gcc, con -g, pone
                   tras el último hunk un bloque de depuración y símbolos acabado en HUNK_END. */
                if (abierto) {
                    abierto = 0;
                    cargados++;
                }
                break;
            case HUNK_OVERLAY:
            case HUNK_BREAK:
                e = HUNK_E_OVERLAY;
                goto mal;
            default:
                /* Un bloque desconocido con la marca de "se puede saltar": largo y contenido, como HUNK_DEBUG. */
                if (palabra & HUNKF_ADVISORY) {
                    if (!lee32(&l, &n) || !salta(&l, n)) { e = HUNK_E_CORTO; goto mal; }
                    break;
                }
                e = HUNK_E_TIPO;
                goto mal;
        }
    }
    /* El último hunk sin HUNK_END al final del fichero cuenta como acabado. */
    if (abierto) cargados++;
    if (cargados != cuantos) { e = HUNK_E_FALTAN; goto mal; }

    carga->cuantos = cuantos;
    carga->seglist = (segs[0].direccion - 4) >> 2;
    carga->entrada = segs[0].direccion;
    return HUNK_BIEN;

mal:
    suelta(ops, segs, cuantos);
    for (uint32_t i = 0; i < cuantos; i++) segs[i].direccion = 0;
    carga->relocs = 0;
    return e;
}
