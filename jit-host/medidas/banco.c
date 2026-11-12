/* U7: programas 68k para medir lo que cuesta dar la vuelta a los bytes. Cada uno es un trozo de trabajo típico de un
   programa del Amiga, repetido para que dure; se compila uno por cada -DK_<nombre>. Al final deja una suma en
   `resultado` (el banco la compara entre los dos órdenes con la suma de toda la memoria). */
typedef unsigned char u8; typedef unsigned short u16; typedef unsigned long u32;
typedef signed char s8; typedef short s16; typedef long s32;

void principal(void);
void _start(void) { principal(); }

void *memcpy(void *d, const void *s, unsigned long n) { u8 *a = d; const u8 *b = s; while (n--) *a++ = *b++; return d; }
void *memset(void *d, int c, unsigned long n) { u8 *a = d; while (n--) *a++ = (u8)c; return d; }

volatile u32 resultado[4];
static u32 semilla = 12345;
static u32 azar(void) { semilla = semilla * 1103515245 + 12345; return semilla >> 8; }

#if defined(K_REGISTROS)
/* Solo registros: lo que no toca memoria no cambia (referencia de lo que cuesta nada) */
void principal(void) {
    u32 a = 1, b = 2, c = 3;
    for (u32 i = 0; i < 3000000; i++) { a += b ^ i; b = (b << 3) | (c >> 29); c += a - i; }
    resultado[0] = a ^ b ^ c;
}
#elif defined(K_COPIAR)
/* Copiar y sumar bloques de largos y de palabras (como un blit hecho a mano o mover una lista) */
static u32 ori[4096], des[4096];
void principal(void) {
    for (int i = 0; i < 4096; i++) ori[i] = azar();
    u32 s = 0;
    for (int r = 0; r < 120; r++) {
        u32 *p = ori, *q = des;
        for (int i = 0; i < 4096; i += 4) { q[0] = p[0]; q[1] = p[1]; q[2] = p[2]; q[3] = p[3]; p += 4; q += 4; }
        u16 *w = (u16 *)des;
        for (int i = 0; i < 8192; i++) s += w[i];
        ori[r & 4095] ^= s;
    }
    resultado[0] = s;
}
#elif defined(K_ORDENAR)
/* Ordenar palabras con signo (inserción y Shell): cargas y guardados de 16 bits con signo */
static s16 v[3000];
void principal(void) {
    u32 s = 0;
    for (int r = 0; r < 6; r++) {
        for (int i = 0; i < 3000; i++) v[i] = (s16)azar();
        for (int h = 1093; h > 0; h = h / 3) {
            for (int i = h; i < 3000; i++) { s16 x = v[i]; int j = i; while (j >= h && v[j - h] > x) { v[j] = v[j - h]; j -= h; } v[j] = x; }
        }
        for (int i = 0; i < 3000; i++) s = s * 31 + v[i];
    }
    resultado[0] = s;
}
#elif defined(K_CRC)
/* CRC-32 con tabla sobre un búfer de bytes: un byte (no se gira) y un largo de la tabla (sí) por vuelta */
static u32 tabla[256]; static u8 buf[16384];
void principal(void) {
    for (u32 i = 0; i < 256; i++) { u32 c = i; for (int k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >> 1) : c >> 1; tabla[i] = c; }
    for (int i = 0; i < 16384; i++) buf[i] = (u8)azar();
    u32 c = 0xffffffff;
    for (int r = 0; r < 60; r++) for (int i = 0; i < 16384; i++) c = tabla[(c ^ buf[i]) & 0xff] ^ (c >> 8);
    resultado[0] = ~c;
}
#elif defined(K_LISTA)
/* Listas enlazadas de estructuras (como las de exec): seguir punteros y tocar campos de 8, 16 y 32 bits */
struct nodo { struct nodo *sig, *ant; u8 tipo; s8 pri; u16 cuenta; u32 dato; };
static struct nodo nodos[2000];
void principal(void) {
    for (int i = 0; i < 2000; i++) {
        nodos[i].sig = &nodos[(i * 7 + 1) % 2000]; nodos[i].ant = &nodos[(i + 1999) % 2000];
        nodos[i].tipo = (u8)i; nodos[i].pri = (s8)(azar()); nodos[i].cuenta = 0; nodos[i].dato = azar();
    }
    u32 s = 0; struct nodo *n = &nodos[0];
    for (int i = 0; i < 400000; i++) {
        n->cuenta++;
        if (n->pri > 0) s += n->dato; else s ^= n->ant->dato;
        n->dato += n->tipo;
        n = n->sig;
    }
    resultado[0] = s;
}
#elif defined(K_FPU)
/* Coma flotante de doble precisión sobre tablas en memoria (Double se gira con REV64 de vector) */
static double a[1000], b[1000];
void principal(void) {
    for (int i = 0; i < 1000; i++) { a[i] = (double)(azar() & 0xffff) / 1000.0; b[i] = 0.5; }
    double s = 0;
    for (int r = 0; r < 120; r++) {
        for (int i = 1; i < 999; i++) b[i] = (a[i - 1] + a[i] + a[i + 1]) * 0.333 + b[i] * 0.5;
        for (int i = 0; i < 1000; i++) s += a[i] * b[i];
    }
    resultado[0] = (u32)s;
}
#else
#error "falta -DK_<nombre>"
#endif
