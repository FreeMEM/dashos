/* Prueba PPC F1d: coma flotante como la genera el compilador (double y float): las cuatro operaciones, multiplicar y
   sumar, conversiones entre enteros, float y double, comparaciones, valor absoluto, raíz por Newton, series. Los
   resultados van a `res` (como bits) y a `resf`. */
#include <stdint.h>

uint32_t res[64];
double resd[32];
float resf[32];
static double datos[16] = { 1.0, -2.5, 3.141592653589793, 1e10, -1e-10, 0.1, 123456.789, -0.0,
                            2.0, 0.5, 1e300, 1e-300, 7.0, -3.0, 65536.0, 1.0 / 3.0 };
static float fdatos[8] = { 1.5f, -0.25f, 3.0e10f, 1.0e-10f, 0.1f, -7.75f, 1024.0f, 2.0f / 3.0f };

__attribute__((noinline)) static double raiz(double x)
{
    double r = x > 1 ? x / 2 : 1;
    for (int i = 0; i < 30; i++) r = 0.5 * (r + x / r);
    return r;
}

__attribute__((noinline)) static float polinomio(float x)
{
    return ((2.0f * x - 3.0f) * x + 0.5f) * x - 1.25f;
}

__attribute__((section(".text.prueba"))) void prueba(void)
{
    double s = 0, p = 1;
    for (int i = 0; i < 16; i++) {
        s += datos[i] * datos[(i + 3) & 15];
        p = p * 0.75 + datos[i] / (i + 1);
        resd[i] = datos[i] * 2.0 - datos[(i + 5) & 15] / 3.0;
    }
    resd[16] = s;
    resd[17] = p;
    resd[18] = raiz(2.0);
    resd[19] = raiz(1e6);
    /* conversiones */
    for (int i = 0; i < 8; i++) {
        int32_t n = (int32_t)(datos[i] * 1000.0);
        res[i] = (uint32_t)n;
        resf[i] = (float)datos[i];
        res[8 + i] = (uint32_t)(int32_t)fdatos[i];
        resd[20 + i] = (double)(int32_t)(i * -12345 + 7);
    }
    /* comparaciones */
    uint32_t c = 0;
    for (int i = 0; i < 16; i++) {
        c = (c << 1) | (datos[i] < datos[(i + 1) & 15]);
        c = (c << 1) | (datos[i] >= 1.0);
    }
    res[16] = c;
    /* float */
    float f = 0;
    for (int i = 0; i < 8; i++) {
        f += fdatos[i] * fdatos[7 - i];
        resf[8 + i] = polinomio(fdatos[i]);
        resf[16 + i] = fdatos[i] < 0 ? -fdatos[i] : fdatos[i];
    }
    resf[24] = f;
    resd[28] = __builtin_fabs(datos[1]) + __builtin_fabs(datos[7]);
    resd[29] = -datos[2];
    resd[30] = datos[3] * datos[4] + datos[5];
    resd[31] = datos[6] * datos[8] - datos[9];
}
