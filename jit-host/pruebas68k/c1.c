/* U7: un programa 68k en C que toca la memoria de muchas formas (bytes, palabras, largos, estructuras, campos de
   bits, Float y Double, tablas de saltos, 64 bits...). Escribe sus resultados en su zona de datos; el banco de
   pruebas compara la memoria entera y los registros entre el JIT big-endian y el little-endian. */
typedef unsigned char u8; typedef unsigned short u16; typedef unsigned long u32;
typedef signed char s8; typedef short s16; typedef long s32;
typedef unsigned long long u64; typedef long long s64;

void principal(void);
void _start(void) { principal(); }

void *memcpy(void *d, const void *s, unsigned long n) { u8 *a = d; const u8 *b = s; while (n--) *a++ = *b++; return d; }
void *memset(void *d, int c, unsigned long n) { u8 *a = d; while (n--) *a++ = (u8)c; return d; }

static u32 crc_tab[256];
static u32 crc(const u8 *p, u32 n) {
    u32 c = 0xffffffff;
    while (n--) c = crc_tab[(c ^ *p++) & 0xff] ^ (c >> 8);
    return ~c;
}

struct campos { unsigned a : 3, b : 7, c : 13, d : 9; };
struct mezcla { u8 b; s16 w; s32 l; u8 bb[3]; double d; float f; s64 q; long double x; };

volatile u32 resultado[64];
s16 cortos[200]; s8 bytes[200]; s32 largos[200];
struct mezcla mz[8];
struct campos cp[16];
double dd[32]; float ff[32];
long double xx[8];
u64 qq[16];
char texto[128];

static int __attribute__((noinline)) sw(int v) {
    switch (v) { case 0: return 11; case 1: return 22; case 2: return 33; case 3: return 44; case 4: return 55;
                 case 5: return 66; case 6: return 77; case 7: return 88; default: return -1; }
}

static void __attribute__((noinline)) ordenar16(s16 *a, int n) {
    for (int i = 1; i < n; i++) { s16 v = a[i]; int j = i - 1; while (j >= 0 && a[j] > v) { a[j + 1] = a[j]; j--; } a[j + 1] = v; }
}
static void __attribute__((noinline)) ordenar8(s8 *a, int n) {
    for (int i = 1; i < n; i++) { s8 v = a[i]; int j = i - 1; while (j >= 0 && a[j] > v) { a[j + 1] = a[j]; j--; } a[j + 1] = v; }
}
static void __attribute__((noinline)) ordenar32(s32 *a, int n) {
    for (int i = 1; i < n; i++) { s32 v = a[i]; int j = i - 1; while (j >= 0 && a[j] > v) { a[j + 1] = a[j]; j--; } a[j + 1] = v; }
}

void principal(void) {
    for (u32 i = 0; i < 256; i++) { u32 c = i; for (int k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >> 1) : c >> 1; crc_tab[i] = c; }
    u32 r = 12345;
    for (int i = 0; i < 200; i++) { r = r * 1103515245 + 12345; cortos[i] = (s16)(r >> 8); bytes[i] = (s8)(r >> 3); largos[i] = (s32)(r ^ (r << 7)); }
    ordenar16(cortos, 200); ordenar8(bytes, 200); ordenar32(largos, 200);
    resultado[0] = crc((u8 *)cortos, sizeof cortos);
    resultado[1] = crc((u8 *)bytes, sizeof bytes);
    resultado[2] = crc((u8 *)largos, sizeof largos);
    for (int i = 0; i < 16; i++) { cp[i].a = i; cp[i].b = i * 9; cp[i].c = i * 511; cp[i].d = 511 - i; }
    u32 s = 0;
    for (int i = 0; i < 16; i++) s = s * 31 + cp[i].a + cp[i].b * 3 + cp[i].c * 5 + cp[i].d * 7;
    resultado[3] = s;
    resultado[4] = crc((u8 *)cp, sizeof cp);
    for (int i = 0; i < 8; i++) {
        mz[i].b = i; mz[i].w = -i * 1000; mz[i].l = i * 0x01020304; mz[i].bb[0] = 1; mz[i].bb[1] = 2; mz[i].bb[2] = i;
        mz[i].d = i * 1.25 - 3.5; mz[i].f = i * 0.5f; mz[i].q = (s64)i * 0x123456789LL; mz[i].x = (long double)i / 3;
    }
    struct mezcla t = mz[3]; mz[7] = t;
    resultado[5] = crc((u8 *)mz, sizeof mz);
    double acd = 0; float acf = 0;
    for (int i = 0; i < 32; i++) { dd[i] = i * 0.1 + mz[i & 7].d; ff[i] = (float)dd[i] * 2.0f; acd += dd[i] * dd[i]; acf += ff[i]; }
    for (int i = 0; i < 8; i++) { xx[i] = mz[i].x * mz[i].x + 1; }
    resultado[6] = (u32)(s32)(acd * 1000); resultado[7] = (u32)(s32)(acf * 100);
    resultado[8] = crc((u8 *)dd, sizeof dd); resultado[9] = crc((u8 *)ff, sizeof ff); resultado[10] = crc((u8 *)xx, sizeof xx);
    u64 q = 0x0123456789abcdefULL;
    for (int i = 0; i < 16; i++) { q = q * 6364136223846793005ULL + 1442695040888963407ULL; qq[i] = q; }
    resultado[11] = crc((u8 *)qq, sizeof qq);
    s32 sd = 0; for (int i = 1; i < 100; i++) sd += largos[i] / i + (s32)((u32)largos[i] % (u32)i);
    resultado[12] = sd;
    int k = 0; for (int i = 0; i < 20; i++) k = k * 3 + sw(i % 10);
    resultado[13] = k;
    const char *h = "Hola, DashOS en little-endian";
    int n = 0; while (h[n]) { texto[n] = h[n]; n++; } texto[n] = 0;
    for (int i = 0; i < n; i++) if (texto[i] >= 'a' && texto[i] <= 'z') texto[i] -= 32;
    resultado[14] = crc((u8 *)texto, sizeof texto);
    u16 *pw = (u16 *)texto; u32 sw2 = 0; for (int i = 0; i < 16; i++) sw2 += pw[i] ^ (pw[i] >> 3);
    resultado[15] = sw2;
}
