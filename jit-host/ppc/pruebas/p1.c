/* Prueba PPC F1: enteros, saltos, cargas y guardados, llamadas (con la pila y LR), tablas de saltos. Los resultados
   van a `res`; el banco y el oráculo comparan los registros finales y la suma de la memoria. */
#include <stdint.h>

#define N 64
uint32_t res[64];
static uint8_t bytes[N];
static uint16_t medias[N];
static int32_t datos[N] = { 3, -7, 100000, -2147483647, 12, 0, 1, -1, 77, 65535, -65536, 31, 5, 9, 2, 4 };

struct punto { int16_t x, y; uint8_t c; int32_t z; };
static struct punto puntos[8];

__attribute__((noinline)) static uint32_t fib(uint32_t n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

__attribute__((noinline)) static int32_t elige(int32_t k, int32_t a)
{
    switch (k & 7) {
    case 0: return a + 1;
    case 1: return a * 3;
    case 2: return a ^ 0x5a5a;
    case 3: return a >> 3;
    case 4: return (int32_t)((uint32_t)a >> 5);
    case 5: return a - 1000;
    case 6: return ~a;
    default: return -a;
    }
}

static uint32_t (*funcs[2])(uint32_t) = { fib, 0 };

__attribute__((section(".text.prueba"))) void prueba(void)
{
    uint32_t s = 0, x = 0x12345678;
    int32_t m = 0;
    for (int i = 0; i < N; i++) {
        bytes[i] = (uint8_t)(x >> (i & 24));
        medias[i] = (uint16_t)(x * (uint32_t)i);
        x = x * 1103515245u + 12345u;               /* mullw */
        s += bytes[i] + medias[i];
        m += elige(i, datos[i & 15]);
    }
    res[0] = s;
    res[1] = (uint32_t)m;
    res[2] = fib(15);
    res[3] = funcs[0](10);
    /* divisiones con y sin signo */
    int32_t a = -1000003, b = 7;
    uint32_t ua = 4000000000u, ub = 13;
    res[4] = (uint32_t)(a / b);
    res[5] = (uint32_t)(a % b);
    res[6] = ua / ub;
    res[7] = ua % ub;
    /* desplazamientos y rotaciones */
    res[8] = (x << 7) | (x >> 25);
    res[9] = (uint32_t)((int32_t)x >> 13);
    res[10] = (x & 0x00ff0f00) >> 4;
    res[11] = __builtin_clz(x | 1);
    /* 64 bits en 32: suma con acarreo, multiplicación alta */
    uint64_t g = (uint64_t)x * 0x9e3779b9u + 0xffffffffull;
    res[12] = (uint32_t)(g >> 32);
    res[13] = (uint32_t)g;
    int64_t h = (int64_t)(int32_t)x * -12345;
    res[14] = (uint32_t)((uint64_t)h >> 32);
    /* signo de bytes y medias palabras */
    int8_t sb = (int8_t)bytes[5];
    int16_t sh = (int16_t)medias[9];
    res[15] = (uint32_t)(sb + sh);
    /* estructuras */
    for (int i = 0; i < 8; i++) {
        puntos[i].x = (int16_t)(i * -300);
        puntos[i].y = (int16_t)(i * 1000);
        puntos[i].c = (uint8_t)(i * 37);
        puntos[i].z = puntos[i].x * puntos[i].y;
    }
    uint32_t t = 0;
    for (int i = 0; i < 8; i++) t = t * 31 + (uint32_t)puntos[i].z + puntos[i].c;
    res[16] = t;
    /* comparaciones con y sin signo */
    uint32_t c = 0;
    for (int i = 0; i < 16; i++) {
        c = (c << 1) | (datos[i] < datos[(i + 1) & 15]);
        c = (c << 1) | ((uint32_t)datos[i] < (uint32_t)datos[(i + 3) & 15]);
    }
    res[17] = c;
}
