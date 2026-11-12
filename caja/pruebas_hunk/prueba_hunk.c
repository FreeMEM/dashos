/*
    DashOS, H4: pruebas del cargador hunk (../hunk.c) en el PC, con AddressSanitizer y UBSan (Makefile).

    1. El ejecutable de verdad (../pruebas68k/saludo.exe, de m68k-amigaos-gcc): tres hunks, la entrada, las
       reubicaciones entre hunks, la lista de segmentos y el BSS a 0.
    2. Un fichero hecho a mano con lo que gcc no saca: HUNK_RELOC32SHORT, HUNK_DREL32, HUNK_NAME, un bloque
       desconocido que se puede saltar, banderas CHIP/FAST y la palabra extra de atributos.
    3. Ficheros hostiles, cada uno con su error.
    4. Todos los cortes del ejecutable de verdad, y mutaciones al azar: nunca se rompe nada, lo que falla devuelve toda
       su memoria y lo que carga queda dentro de lo reservado.
*/
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../hunk.h"

/* --- La memoria del 68k de la prueba: 1 MB en $200000, con un reparto que solo avanza --- */

#define BASE 0x00200000u
#define TAM  (1u << 20)
static uint8_t arena[TAM];
static uint32_t libre;          /* lo siguiente por dar, desde BASE */
static int vivos;               /* bloques reservados y no devueltos */
static uint32_t ultimo_atributo;

static uint32_t reservar(void *ctx, uint32_t bytes, uint32_t atributos)
{
    (void)ctx;
    ultimo_atributo = atributos;
    if (bytes > TAM - libre) return 0;
    uint32_t d = BASE + libre;
    libre += (bytes + 7) & ~7u;
    vivos++;
    memset(arena + (d - BASE), 0xAA, bytes);   /* basura: el cargador tiene que poner los ceros */
    return d;
}

static void liberar(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx; (void)bytes;
    if (d < BASE || d >= BASE + TAM) { printf("MAL: liberar %08x, que no es de la arena\n", d); exit(1); }
    vivos--;
}

static uint8_t *ver(void *ctx, uint32_t d, uint32_t bytes)
{
    (void)ctx;
    if (d < BASE || bytes > TAM || d - BASE > TAM - bytes) return NULL;
    return arena + (d - BASE);
}

static const struct hunk_ops ops = { NULL, reservar, liberar, ver };

static void reinicia(void) { libre = 0; vivos = 0; }

static uint32_t lee32(uint32_t d) { uint8_t *b = ver(NULL, d, 4); return (uint32_t)b[0] << 24 | b[1] << 16 | b[2] << 8 | b[3]; }

static int fallos;
#define COMPRUEBA(c, ...) do { if (!(c)) { printf("MAL (%s:%d): ", __FILE__, __LINE__); printf(__VA_ARGS__); printf("\n"); fallos++; } } while (0)

/* --- Un fichero que se escribe palabra a palabra --- */

struct fich { uint8_t b[4096]; size_t n; };
static void l32(struct fich *f, uint32_t v) { f->b[f->n++] = v >> 24; f->b[f->n++] = v >> 16; f->b[f->n++] = v >> 8; f->b[f->n++] = v; }
static void l16(struct fich *f, uint32_t v) { f->b[f->n++] = v >> 8; f->b[f->n++] = v; }

static struct hunk_seg segs[64];

static int carga(const uint8_t *b, size_t n, struct hunk_carga *c)
{
    reinicia();
    int e = hunk_cargar(b, n, &ops, segs, 64, c);
    if (e != HUNK_BIEN) COMPRUEBA(vivos == 0, "tras el error %d quedan %d bloques sin devolver", e, vivos);
    return e;
}

/* --- 1. El ejecutable de verdad --- */

static uint8_t *saludo;
static size_t saludo_n;

static void prueba_saludo(const char *ruta)
{
    FILE *f = fopen(ruta, "rb");
    if (!f) { printf("MAL: no se abre %s\n", ruta); fallos++; return; }
    saludo = malloc(1 << 20);
    saludo_n = fread(saludo, 1, 1 << 20, f);
    fclose(f);
    struct hunk_carga c;
    int e = carga(saludo, saludo_n, &c);
    COMPRUEBA(e == HUNK_BIEN, "saludo.exe: %s", hunk_error(e));
    if (e) return;
    COMPRUEBA(c.cuantos == 3, "saludo.exe: %u hunks", c.cuantos);
    COMPRUEBA(segs[0].tipo == HUNK_CODE && segs[1].tipo == HUNK_DATA && segs[2].tipo == HUNK_BSS, "tipos de los hunks");
    COMPRUEBA(c.entrada == segs[0].direccion, "entrada");
    /* La entrada es "jmp _inicio" (saludo_entrada.s): $4EF9 y una dirección absoluta, reubicada al hunk de código. */
    uint32_t op = lee32(c.entrada) >> 16, destino = (lee32(c.entrada + 2));
    COMPRUEBA(op == 0x4EF9, "la entrada no es un jmp: %04x", op);
    COMPRUEBA(destino > segs[0].direccion && destino < segs[0].direccion + segs[0].bytes, "el jmp va a %08x, fuera del código", destino);
    /* La lista de segmentos: BPTR a la palabra "siguiente" de cada uno; delante, el tamaño del bloque. */
    uint32_t s = c.seglist;
    for (int i = 0; i < 3; i++) {
        COMPRUEBA(s != 0 && s * 4 + 4 == segs[i].direccion, "segmento %d: BPTR %08x", i, s);
        COMPRUEBA(lee32(s * 4 - 4) == segs[i].bytes + 8, "segmento %d: tamaño del bloque", i);
        s = lee32(s * 4);
    }
    COMPRUEBA(s == 0, "la lista no acaba en 0");
    /* El BSS, a 0 (la arena tenía $AA). */
    int ceros = 1;
    for (uint32_t i = 0; i < segs[2].bytes; i++) ceros &= *ver(NULL, segs[2].direccion + i, 1) == 0;
    COMPRUEBA(ceros && segs[2].bytes >= 1000, "el BSS no está a 0 o es pequeño (%u)", segs[2].bytes);
    /* Las reubicaciones que van al hunk de datos o al BSS: alguna palabra del código tiene que caer en ellos. */
    int a_datos = 0, a_bss = 0;
    for (uint32_t i = 0; i + 4 <= segs[0].bytes; i += 2) {
        uint32_t v = lee32(segs[0].direccion + i);
        a_datos |= v >= segs[1].direccion && v < segs[1].direccion + segs[1].bytes;
        a_bss |= v >= segs[2].direccion && v < segs[2].direccion + segs[2].bytes;
    }
    COMPRUEBA(a_datos && a_bss && c.relocs >= 4, "reubicaciones: a datos %d, a BSS %d, %u en total", a_datos, a_bss, c.relocs);
    printf("  saludo.exe: %u hunks (código %u, datos %u, BSS %u bytes), %u reubicaciones, entrada en %08x\n",
           c.cuantos, segs[0].bytes, segs[1].bytes, segs[2].bytes, c.relocs, c.entrada);
    int antes = vivos;
    hunk_soltar(&ops, &c);
    COMPRUEBA(vivos == 0 && antes == 3, "hunk_soltar: quedan %d de %d", vivos, antes);
}

/* --- 2. A mano: un hunk de código y uno de datos con todo lo que gcc no saca --- */

static void prueba_a_mano(void)
{
    struct fich f = { .n = 0 };
    l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 2); l32(&f, 0); l32(&f, 1);
    l32(&f, 4 | HUNKF_CHIP);                          /* código: 16 bytes, en memoria CHIP */
    l32(&f, 2 | HUNKF_CHIP | HUNKF_FAST); l32(&f, 0x10005);   /* datos: 8 bytes, con atributos extra */
    l32(&f, HUNK_NAME); l32(&f, 1); l32(&f, 0x636F6400);  /* "cod" */
    l32(&f, HUNK_CODE | HUNKF_CHIP); l32(&f, 3);
    l32(&f, 0x11111111); l32(&f, 0x00000004); l32(&f, 0x00000000);   /* el resto del hunk (4 bytes), a 0 */
    l32(&f, HUNK_RELOC32SHORT);
    l16(&f, 1); l16(&f, 1); l16(&f, 4);                /* +4: + datos */
    l16(&f, 1); l16(&f, 0); l16(&f, 8);                /* +8: + código */
    l16(&f, 0); l16(&f, 0);                            /* fin y relleno hasta la palabra larga */
    l32(&f, 0x20000000 | 0x123); l32(&f, 1); l32(&f, 0xDEADBEEF);    /* desconocido, con la marca de saltar */
    l32(&f, HUNK_END);
    l32(&f, HUNK_DATA); l32(&f, 2); l32(&f, 0); l32(&f, 0x00000010);
    l32(&f, HUNK_DREL32); l16(&f, 1); l16(&f, 0); l16(&f, 4); l16(&f, 0);   /* sin relleno: 4 palabras */
    l32(&f, HUNK_RELOC32); l32(&f, 1); l32(&f, 1); l32(&f, 0); l32(&f, 0);
    l32(&f, HUNK_SYMBOL); l32(&f, 1); l32(&f, 0x61626300); l32(&f, 0); l32(&f, 0);
    l32(&f, HUNK_DEBUG); l32(&f, 2); l32(&f, 1); l32(&f, 2);
    l32(&f, HUNK_END);
    struct hunk_carga c;
    int e = carga(f.b, f.n, &c);
    COMPRUEBA(e == HUNK_BIEN, "a mano: %s", hunk_error(e));
    if (e) return;
    uint32_t cod = segs[0].direccion, dat = segs[1].direccion;
    COMPRUEBA(lee32(cod) == 0x11111111 && lee32(cod + 4) == dat + 4 && lee32(cod + 8) == cod && lee32(cod + 12) == 0,
              "código: %08x %08x %08x %08x", lee32(cod), lee32(cod + 4), lee32(cod + 8), lee32(cod + 12));
    COMPRUEBA(lee32(dat) == dat && lee32(dat + 4) == cod + 0x10, "datos: %08x %08x", lee32(dat), lee32(dat + 4));
    COMPRUEBA(ultimo_atributo == 0x10005, "atributos extra: %x", ultimo_atributo);
    COMPRUEBA(c.relocs == 4, "%u reubicaciones", c.relocs);
    hunk_soltar(&ops, &c);
    printf("  a mano: RELOC32SHORT, DREL32, NAME, bloque que se salta y atributos de memoria: bien\n");
}

/* --- 3. Hostiles --- */

/* Una cabecera de `n` hunks de `tam` palabras, y un hunk de código de `datos` palabras. */
static void cabecera(struct fich *f, uint32_t n, uint32_t tam)
{
    f->n = 0;
    l32(f, HUNK_HEADER); l32(f, 0); l32(f, n); l32(f, 0); l32(f, n - 1);
    for (uint32_t i = 0; i < n; i++) l32(f, tam);
}
static void codigo(struct fich *f, uint32_t datos)
{
    l32(f, HUNK_CODE); l32(f, datos);
    for (uint32_t i = 0; i < datos; i++) l32(f, 0x4E714E71);
}

static void espera(const char *que, struct fich *f, int error)
{
    struct hunk_carga c;
    int e = carga(f->b, f->n, &c);
    COMPRUEBA(e == error, "%s: da \"%s\", se esperaba \"%s\"", que, hunk_error(e), hunk_error(error));
    if (e == HUNK_BIEN) hunk_soltar(&ops, &c);
}

static void prueba_hostiles(void)
{
    struct fich f;
    int n = 0;
#define H(que, err) do { espera(que, &f, err); n++; } while (0)
    f.n = 0; H("vacío", HUNK_E_CORTO);
    f.n = 0; l16(&f, 0); H("dos bytes", HUNK_E_CORTO);
    f.n = 0; l32(&f, HUNK_CODE); H("sin cabecera", HUNK_E_NO_ES_HUNK);
    f.n = 0; l32(&f, 0x7F454C46); H("un ELF", HUNK_E_NO_ES_HUNK);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 1); l32(&f, 0x41424344); l32(&f, 0); H("con bibliotecas residentes", HUNK_E_LIBRERIAS);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 0); l32(&f, 0); l32(&f, 0xFFFFFFFF); H("0 hunks", HUNK_E_CUANTOS);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 0xFFFFFFFF); l32(&f, 0); l32(&f, 0xFFFFFFFE); H("4 mil millones de hunks", HUNK_E_CUANTOS);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 65); l32(&f, 0); l32(&f, 64); H("más de los que caben", HUNK_E_CUANTOS);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 2); l32(&f, 1); l32(&f, 1); H("primero 1 (overlay)", HUNK_E_PRIMERO);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 2); l32(&f, 0); l32(&f, 5); H("último que no cuadra", HUNK_E_PRIMERO);
    f.n = 0; l32(&f, HUNK_HEADER); l32(&f, 0); l32(&f, 3); l32(&f, 0); l32(&f, 2); l32(&f, 1); H("faltan tamaños", HUNK_E_CORTO);
    cabecera(&f, 1, 0x3FFFFFFF); H("hunk de 4 GB", HUNK_E_TAMANO);
    cabecera(&f, 1, 0x10000000); H("hunk de 1 GB (no hay memoria)", HUNK_E_MEMORIA);
    cabecera(&f, 3, 0x20000); H("tres de 512 KB (el tercero no cabe)", HUNK_E_MEMORIA);
    cabecera(&f, 1, 2); codigo(&f, 3); l32(&f, HUNK_END); H("datos más largos que el hunk", HUNK_E_TAMANO);
    cabecera(&f, 1, 2); l32(&f, HUNK_CODE); l32(&f, 0x3FFFFFFF); H("datos de 4 GB", HUNK_E_TAMANO);
    cabecera(&f, 1, 0x100); l32(&f, HUNK_CODE); l32(&f, 0x100); l32(&f, 0); H("datos cortados", HUNK_E_CORTO);
    cabecera(&f, 1, 2); l32(&f, HUNK_BSS); l32(&f, 3); l32(&f, HUNK_END); H("BSS más grande que el hunk", HUNK_E_TAMANO);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_RELOC32); l32(&f, 1); l32(&f, 1); l32(&f, 0); l32(&f, 0); l32(&f, HUNK_END);
    H("reubicación a un hunk que no existe", HUNK_E_RELOC);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_RELOC32); l32(&f, 1); l32(&f, 0); l32(&f, 6); l32(&f, 0); l32(&f, HUNK_END);
    H("reubicación que se sale por 2 bytes", HUNK_E_RELOC);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_RELOC32); l32(&f, 1); l32(&f, 0); l32(&f, 0xFFFFFFFE); l32(&f, 0); l32(&f, HUNK_END);
    H("reubicación con desplazamiento enorme", HUNK_E_RELOC);
    cabecera(&f, 1, 0); l32(&f, HUNK_BSS); l32(&f, 0); l32(&f, HUNK_RELOC32); l32(&f, 1); l32(&f, 0); l32(&f, 0); l32(&f, 0); l32(&f, HUNK_END);
    H("reubicación en un hunk vacío", HUNK_E_RELOC);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_RELOC32); l32(&f, 0x40000000); l32(&f, 0); l32(&f, 0);
    H("mil millones de reubicaciones en un fichero corto", HUNK_E_CORTO);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_RELOC32SHORT); l16(&f, 1); l16(&f, 7); l16(&f, 0); l16(&f, 0); l32(&f, HUNK_END);
    H("reubicación corta a un hunk que no existe", HUNK_E_RELOC);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_RELOC32); l32(&f, 1); l32(&f, 0); l32(&f, 0);
    H("reubicaciones sin el 0 final", HUNK_E_CORTO);
    cabecera(&f, 1, 2); l32(&f, HUNK_RELOC32); l32(&f, 0); H("reubicación antes del contenido", HUNK_E_ORDEN);
    cabecera(&f, 2, 2); codigo(&f, 2); codigo(&f, 2); H("dos contenidos sin HUNK_END", HUNK_E_ORDEN);
    cabecera(&f, 1, 2); l32(&f, HUNK_END); l32(&f, HUNK_END); H("solo HUNK_END sin contenido", HUNK_E_FALTAN);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_END); l32(&f, HUNK_DEBUG); l32(&f, 0); l32(&f, HUNK_END);
    l32(&f, HUNK_RELOC32); l32(&f, 0); H("reubicación tras la depuración del final", HUNK_E_ORDEN);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_END); codigo(&f, 1); l32(&f, HUNK_END); H("un hunk de más", HUNK_E_SOBRAN);
    cabecera(&f, 2, 2); codigo(&f, 2); l32(&f, HUNK_END); H("falta un hunk", HUNK_E_FALTAN);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_END); l32(&f, HUNK_OVERLAY); l32(&f, 0); H("overlay", HUNK_E_OVERLAY);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_BREAK); H("break", HUNK_E_OVERLAY);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_EXT); l32(&f, 0); l32(&f, HUNK_END); H("HUNK_EXT (de un objeto)", HUNK_E_TIPO);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, 0x123); l32(&f, 0); l32(&f, HUNK_END); H("bloque desconocido", HUNK_E_TIPO);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_DEBUG); l32(&f, 0x7FFFFFFF); H("depuración más larga que el fichero", HUNK_E_CORTO);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_SYMBOL); l32(&f, 0x00FFFFFF); H("símbolo más largo que el fichero", HUNK_E_CORTO);
    cabecera(&f, 1, 2); codigo(&f, 2); l32(&f, HUNK_END); l16(&f, 0); H("dos bytes de más al final", HUNK_E_CORTO);
    cabecera(&f, 1, 2); codigo(&f, 2); H("el último sin HUNK_END (se admite)", HUNK_BIEN);
    printf("  %d ficheros hostiles, cada uno con su error\n", n);
}

/* --- 4. Cortes y mutaciones --- */

static uint64_t semilla = 0x9E3779B97F4A7C15ull;
static uint32_t azar(void) { semilla ^= semilla << 13; semilla ^= semilla >> 7; semilla ^= semilla << 17; return (uint32_t)semilla; }

static void prueba_cortes_y_mutaciones(void)
{
    if (!saludo) return;
    struct hunk_carga c;
    int buenos = 0, malos = 0;
    for (size_t n = 0; n < saludo_n; n++) {
        int e = carga(saludo, n, &c);
        if (e == HUNK_BIEN) { buenos++; hunk_soltar(&ops, &c); COMPRUEBA(vivos == 0, "corte %zu: sin soltar", n); }
        else malos++;
    }
    printf("  %zu cortes de saludo.exe: %d con error, %d cargan (los que solo pierden el final del último hunk)\n",
           saludo_n, malos, buenos);
    /* Que solo carguen los cortes tras el contenido del BSS (el último hunk): antes, todos fallan. */
    uint8_t *copia = malloc(saludo_n);
    int vueltas = 200000, cargan = 0;
    for (int v = 0; v < vueltas; v++) {
        memcpy(copia, saludo, saludo_n);
        int cambios = 1 + azar() % 4;
        for (int k = 0; k < cambios; k++) {
            uint32_t pos = azar() % saludo_n;
            switch (azar() % 4) {
                case 0: copia[pos] ^= 1 << (azar() % 8); break;
                case 1: copia[pos] = azar(); break;
                case 2: if (pos + 4 <= saludo_n) { uint32_t x = azar(); memcpy(copia + (pos & ~3u), &x, 4); } break;
                default: {   /* un número "interesante" en una palabra alineada */
                    static const uint32_t raros[] = { 0, 1, 2, 3, 0x7FFFFFFF, 0xFFFFFFFF, 0x80000000, 0x3FFFFFFF,
                                                      HUNK_END, HUNK_CODE, HUNK_RELOC32, HUNK_RELOC32SHORT, 0x40000000 };
                    uint32_t x = raros[azar() % (sizeof(raros) / 4)];
                    uint32_t p = pos & ~3u;
                    if (p + 4 <= saludo_n) { copia[p] = x >> 24; copia[p + 1] = x >> 16; copia[p + 2] = x >> 8; copia[p + 3] = x; }
                }
            }
        }
        size_t largo = azar() % 8 == 0 ? azar() % saludo_n : saludo_n;
        int e = carga(copia, largo, &c);
        if (e == HUNK_BIEN) {
            cargan++;
            for (uint32_t i = 0; i < c.cuantos; i++)
                COMPRUEBA(ver(NULL, segs[i].direccion - 8, segs[i].bytes + 8) != NULL, "mutación %d: hunk fuera de la arena", v);
            hunk_soltar(&ops, &c);
            COMPRUEBA(vivos == 0, "mutación %d: sin soltar", v);
        }
    }
    free(copia);
    printf("  %d mutaciones al azar de saludo.exe: %d cargan, %d con error, sin romper nada ni perder memoria\n",
           vueltas, cargan, vueltas - cargan);
}

int main(int argc, char **argv)
{
    printf("Cargador hunk (H4), en el PC:\n");
    prueba_saludo(argc > 1 ? argv[1] : "../pruebas68k/saludo.exe");
    prueba_a_mano();
    prueba_hostiles();
    prueba_cortes_y_mutaciones();
    printf("%s: cargador hunk (%d fallos)\n", fallos ? "MAL" : "BIEN", fallos);
    return fallos != 0;
}
