/* componer_gl: el prototipo del servidor de composición del dominio de gráficos (S38). Compone con la GPU, por
 * OpenGL ES 2 y Mesa, lo mismo que el compositor de DashOS compone hoy por CPU (`wayland/src/escena.rs`): un fondo,
 * cuatro ventanas (dos opacas, una con esquinas redondeadas y una a medias) y la sombra de cada una. El fondo es
 * animado y lo calcula un sombreador (un "plasma"): cambia entero en cada cuadro.
 *
 *     componer_gl [--dispositivo RUTA | --sin-dispositivo] [--ancho N] [--alto N] [--cuadros N] [--modo MODO]
 *
 *   --dispositivo RUTA   el nodo de DRM de la GPU (de serie /dev/dri/renderD128), por GBM
 *   --sin-dispositivo    EGL "surfaceless" de Mesa: elige Mesa (con LIBGL_ALWAYS_SOFTWARE=1, llvmpipe)
 *   --modo fondo         solo el fondo animado
 *          escritorio    el fondo y las cuatro ventanas con sus sombras (de serie)
 *          subida        como escritorio, y además una ventana de 800 x 600 sube entera cada cuadro (un cliente de
 *                        wl_shm que repinta toda su ventana: lo que cuesta pasar sus píxeles a la GPU)
 *          lectura       como escritorio, y además el cuadro entero vuelve a la memoria (glReadPixels): lo que
 *                        costaría devolverlo a DashOS en vez de que la GPU lo saque a la pantalla
 *
 * Antes de medir compone un cuadro con el tiempo a 0 y comprueba píxeles conocidos contra lo que sale de las mismas
 * cuentas en la CPU: "comprobación BIEN" o "comprobación MAL" (y sale con 1). Después mide N cuadros (de serie 300):
 * cada uno se compone entero y se espera a que acabe (glFinish). Dice la mediana y el percentil 95 del tiempo por
 * cuadro y la CPU que gastó el proceso por cuadro (todos sus hilos: con llvmpipe, los suyos). Las líneas empiezan por
 * "componer_gl:".
 *
 * Errores: 2 argumentos malos; 3 sin GPU o sin EGL/GLES; 1 la comprobación falló.
 *
 * Solo usa las interfaces públicas de EGL, GLES2 y GBM (sala limpia: Mesa se ejecuta, no se lee). Se compila igual en
 * el PC (x86-64) y para la caja (aarch64, tools/gpu_componer.py). */
#define _GNU_SOURCE
#include <fcntl.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <time.h>
#include <unistd.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <gbm.h>

#define MAX_LADO 8192
#define MAX_CUADROS 100000

/* Las sombras, como `escena.rs`: 4 abajo, 14 de difuminado, radio 16, alfa 70 de 255. */
#define SOMBRA_BAJA 4.0
#define SOMBRA_BORROSA 14.0
#define SOMBRA_ALFA (70.0 / 255.0)

struct ventana {
    int x, y, w, h;
    uint32_t color; /* ARGB premultiplicado, como wl_shm */
    int esquinas;   /* radio de las esquinas transparentes (0: ninguna) */
    GLuint tex;
    uint8_t *px;    /* B, G, R, A por píxel: la memoria de un búfer ARGB8888 de wl_shm */
};

static struct ventana ventanas[4] = {
    {100, 80, 800, 600, 0xFFFF0000u, 0, 0, NULL},
    {1100, 120, 700, 500, 0xFF008040u, 0, 0, NULL},
    {500, 350, 900, 650, 0xFF336699u, 16, 0, NULL},
    {1300, 650, 500, 350, 0x80000080u, 0, 0, NULL},
};

static int ancho = 1920, alto = 1080;

static double ahora(clockid_t reloj) {
    struct timespec t;
    clock_gettime(reloj, &t);
    return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static void fallo(int codigo, const char *que) {
    fprintf(stderr, "componer_gl: %s\n", que);
    exit(codigo);
}

/* --- El fondo: la misma cuenta en el sombreador y aquí (para comprobar) --- */

#define PLASMA_GLSL                                                                                                    \
    "float plasma(vec2 p, float t) {\n"                                                                                \
    "  float v = sin(p.x * 0.013 + t) + sin(p.y * 0.017 - t * 1.3) + sin((p.x + p.y) * 0.009 + t * 0.7)\n"            \
    "          + sin(length(p - vec2(960.0, 540.0)) * 0.011 - t);\n"                                                    \
    "  return v;\n"                                                                                                    \
    "}\n"                                                                                                              \
    "vec3 paleta(float v) {\n"                                                                                         \
    "  return 0.25 + 0.2 * cos(6.2831853 * (v * 0.125 + vec3(0.0, 0.33, 0.67)));\n"                                    \
    "}\n"

static void plasma_cpu(double x, double y, double t, double rgb[3]) {
    double dx = x - 960.0, dy = y - 540.0;
    double v = sin(x * 0.013 + t) + sin(y * 0.017 - t * 1.3) + sin((x + y) * 0.009 + t * 0.7) +
               sin(sqrt(dx * dx + dy * dy) * 0.011 - t);
    for (int i = 0; i < 3; i++) rgb[i] = 0.25 + 0.2 * cos(6.2831853 * (v * 0.125 + i * 0.33 + (i == 2 ? 0.01 : 0.0)));
}

/* --- Sombreadores --- */

static const char *vs_rect =
    "attribute vec2 a;\n"
    "uniform vec4 rect;\n" /* x, y, ancho, alto en píxeles, desde arriba a la izquierda */
    "uniform vec2 pantalla;\n"
    "varying vec2 uv;\n"
    "void main() {\n"
    "  uv = a;\n"
    "  vec2 p = rect.xy + a * rect.zw;\n"
    "  gl_Position = vec4(p.x / pantalla.x * 2.0 - 1.0, 1.0 - p.y / pantalla.y * 2.0, 0.0, 1.0);\n"
    "}\n";

static const char *fs_fondo =
    "#ifdef GL_FRAGMENT_PRECISION_HIGH\nprecision highp float;\n#else\nprecision mediump float;\n#endif\n"
    "uniform float t;\n"
    "uniform vec2 pantalla;\n" PLASMA_GLSL
    "void main() {\n"
    "  vec2 p = vec2(gl_FragCoord.x, pantalla.y - gl_FragCoord.y);\n"
    "  gl_FragColor = vec4(paleta(plasma(p, t)), 1.0);\n"
    "}\n";

/* La memoria de wl_shm (B, G, R, A) sube como RGBA: el sombreador da la vuelta a los canales, sin copias en la CPU. */
static const char *fs_ventana =
    "precision mediump float;\n"
    "uniform sampler2D tex;\n"
    "varying vec2 uv;\n"
    "void main() { gl_FragColor = texture2D(tex, uv).bgra; }\n";

static const char *fs_sombra =
    "#ifdef GL_FRAGMENT_PRECISION_HIGH\nprecision highp float;\n#else\nprecision mediump float;\n#endif\n"
    "uniform vec4 ventana;\n" /* la ventana (x, y, ancho, alto) */
    "uniform float radio;\n"
    "uniform vec2 pantalla;\n"
    "void main() {\n"
    "  vec2 p = vec2(gl_FragCoord.x, pantalla.y - gl_FragCoord.y);\n"
    "  float r = min(radio, min(ventana.z, ventana.w) * 0.5);\n"
    "  vec2 c0 = ventana.xy + vec2(r, " "4.0" " + r);\n"
    "  vec2 c1 = ventana.xy + ventana.zw + vec2(-r, 4.0 - r);\n"
    "  vec2 d = max(max(c0 - p, p - c1), 0.0);\n"
    "  float dist = length(d) - r;\n"
    "  float f = clamp(14.0 - dist, 0.0, 14.0) / 14.0;\n"
    "  float a = dist <= 0.0 ? 70.0 / 255.0 : 70.0 / 255.0 * f * f;\n"
    "  gl_FragColor = vec4(0.0, 0.0, 0.0, a);\n"
    "}\n";

static GLuint compilar(GLenum tipo, const char *fuente) {
    GLuint s = glCreateShader(tipo);
    glShaderSource(s, 1, &fuente, NULL);
    glCompileShader(s);
    GLint bien = 0;
    glGetShaderiv(s, GL_COMPILE_STATUS, &bien);
    if (!bien) {
        char log[1024];
        glGetShaderInfoLog(s, sizeof log, NULL, log);
        fprintf(stderr, "componer_gl: el sombreador no compila: %s\n", log);
        exit(3);
    }
    return s;
}

static GLuint programa(const char *fs) {
    GLuint p = glCreateProgram();
    glAttachShader(p, compilar(GL_VERTEX_SHADER, vs_rect));
    glAttachShader(p, compilar(GL_FRAGMENT_SHADER, fs));
    glBindAttribLocation(p, 0, "a");
    glLinkProgram(p);
    GLint bien = 0;
    glGetProgramiv(p, GL_LINK_STATUS, &bien);
    if (!bien) fallo(3, "el programa de GL no enlaza");
    return p;
}

static GLuint p_fondo, p_ventana, p_sombra;

static void rect(GLuint p, double x, double y, double w, double h) {
    glUniform4f(glGetUniformLocation(p, "rect"), x, y, w, h);
    glUniform2f(glGetUniformLocation(p, "pantalla"), ancho, alto);
    glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
}

static void pintar_ventana_cpu(struct ventana *v, int cuadro) {
    /* El color de la ventana, con una raya que se mueve (para que una subida no sea siempre igual) y las esquinas. */
    uint8_t a = v->color >> 24, r = v->color >> 16, g = v->color >> 8, b = v->color;
    for (int y = 0; y < v->h; y++) {
        for (int x = 0; x < v->w; x++) {
            uint8_t *p = v->px + ((size_t)y * v->w + x) * 4;
            int raya = cuadro > 0 && ((x + cuadro * 4) % 64) < 2;
            p[0] = raya ? a : b;
            p[1] = raya ? a : g;
            p[2] = raya ? a : r;
            p[3] = a;
            if (v->esquinas) {
                int rr = v->esquinas;
                int cx = x < rr ? rr - x : (x >= v->w - rr ? x - (v->w - rr) + 1 : 0);
                int cy = y < rr ? rr - y : (y >= v->h - rr ? y - (v->h - rr) + 1 : 0);
                if (cx * cx + cy * cy > rr * rr) memset(p, 0, 4);
            }
        }
    }
}

static void componer(double t, int modo_ventanas) {
    glViewport(0, 0, ancho, alto);
    glDisable(GL_BLEND);
    glUseProgram(p_fondo);
    glUniform1f(glGetUniformLocation(p_fondo, "t"), t);
    rect(p_fondo, 0, 0, ancho, alto);
    if (!modo_ventanas) return;
    glEnable(GL_BLEND);
    glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA); /* premultiplicado, como Wayland */
    for (int i = 0; i < 4; i++) {
        struct ventana *v = &ventanas[i];
        glUseProgram(p_sombra);
        glUniform4f(glGetUniformLocation(p_sombra, "ventana"), v->x, v->y, v->w, v->h);
        glUniform1f(glGetUniformLocation(p_sombra, "radio"), v->esquinas ? v->esquinas : 16);
        rect(p_sombra, v->x - SOMBRA_BORROSA, v->y + SOMBRA_BAJA - SOMBRA_BORROSA, v->w + 2 * SOMBRA_BORROSA,
             v->h + 2 * SOMBRA_BORROSA);
        glUseProgram(p_ventana);
        glBindTexture(GL_TEXTURE_2D, v->tex);
        rect(p_ventana, v->x, v->y, v->w, v->h);
    }
}

/* Lee el píxel (x, y) contado desde arriba: R, G, B. */
static void leer(int x, int y, int rgb[3]) {
    uint8_t p[4];
    glReadPixels(x, alto - 1 - y, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, p);
    for (int i = 0; i < 3; i++) rgb[i] = p[i];
}

static int cerca(const int got[3], const double esperado[3], int tol, const char *que) {
    for (int i = 0; i < 3; i++) {
        int e = (int)lround(esperado[i] * 255.0);
        if (abs(got[i] - e) > tol) {
            printf("componer_gl: comprobación MAL en %s: sale (%d, %d, %d), se esperaba (%d, %d, %d)\n", que, got[0],
                   got[1], got[2], (int)lround(esperado[0] * 255), (int)lround(esperado[1] * 255),
                   (int)lround(esperado[2] * 255));
            return 0;
        }
    }
    return 1;
}

static int comprobar(void) {
    componer(0.0, 1);
    glFinish();
    int bien = 1, px[3];
    double e[3], f[3];
    /* El fondo, lejos de todo. */
    plasma_cpu(1880.5, 1060.5, 0.0, e);
    leer(1880, 1060, px);
    bien &= cerca(px, e, 3, "el fondo (1880, 1060)");
    plasma_cpu(30.5, 30.5, 0.0, e);
    leer(30, 30, px);
    bien &= cerca(px, e, 3, "el fondo (30, 30)");
    /* Dentro de la roja, donde no tapa nadie. */
    double rojo[3] = {1, 0, 0};
    leer(150, 150, px);
    bien &= cerca(px, rojo, 1, "la ventana roja");
    /* La esquina transparente de la azul (500, 350): se ve el fondo y la sombra que la roja echa ahí debajo. Basta con
     * que no sea el azul de la ventana. */
    leer(501, 351, px);
    if (abs(px[0] - 0x33) < 4 && abs(px[1] - 0x66) < 4 && abs(px[2] - 0x99) < 4) {
        printf("componer_gl: comprobación MAL: la esquina redondeada de la ventana azul no es transparente\n");
        bien = 0;
    }
    /* La azul por dentro, opaca. */
    double azul[3] = {0x33 / 255.0, 0x66 / 255.0, 0x99 / 255.0};
    leer(900, 700, px);
    bien &= cerca(px, azul, 1, "la ventana azul");
    /* La de alfa 0x80 sobre el fondo (1700, 900), con su sombra entera debajo (como en la CPU: la sombra va antes de
     * la ventana): azul 0x80 + fondo * (1 - sombra) * (1 - 0x80/255). */
    plasma_cpu(1700.5, 900.5, 0.0, f);
    for (int i = 0; i < 3; i++) e[i] = f[i] * (1.0 - SOMBRA_ALFA) * (1.0 - 128.0 / 255.0) + (i == 2 ? 128.0 / 255.0 : 0.0);
    leer(1700, 900, px);
    bien &= cerca(px, e, 4, "la ventana a medias");
    /* Justo debajo de la verde (1100..1800, 120..620), en el centro: la sombra oscurece el fondo como la de la CPU. */
    plasma_cpu(1450.5, 622.5, 0.0, f);
    double y1 = 620 + 4 - 16;
    double dy = 622.5 - y1 > 0 ? 622.5 - y1 : 0;
    double dist = dy - 16, a = dist <= 0 ? SOMBRA_ALFA : SOMBRA_ALFA * pow((14 - dist) / 14, 2);
    for (int i = 0; i < 3; i++) e[i] = f[i] * (1 - a);
    leer(1450, 622, px);
    bien &= cerca(px, e, 4, "la sombra de la verde");
    printf("componer_gl: comprobación %s\n", bien ? "BIEN" : "MAL");
    return bien;
}

static int cmp(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}

int main(int argc, char **argv) {
    const char *dispositivo = "/dev/dri/renderD128", *modo = "escritorio";
    int cuadros = 300, sin_dispositivo = 0;
    for (int i = 1; i < argc; i++) {
        const char *a = argv[i], *v = i + 1 < argc ? argv[i + 1] : NULL;
        char *fin = NULL;
        if (!strcmp(a, "--sin-dispositivo")) {
            sin_dispositivo = 1;
            continue;
        }
        if (!v) fallo(2, "falta el valor de un argumento");
        i++;
        if (!strcmp(a, "--dispositivo")) dispositivo = v;
        else if (!strcmp(a, "--modo")) modo = v;
        else if (!strcmp(a, "--ancho") || !strcmp(a, "--alto") || !strcmp(a, "--cuadros")) {
            long n = strtol(v, &fin, 10);
            if (!*v || *fin) fallo(2, "un número que no es un número");
            if (!strcmp(a, "--cuadros")) {
                if (n < 1 || n > MAX_CUADROS) fallo(2, "--cuadros va de 1 a 100000");
                cuadros = n;
            } else {
                if (n < 1 || n > MAX_LADO) fallo(2, "las medidas van de 1 a 8192");
                *(!strcmp(a, "--ancho") ? &ancho : &alto) = n;
            }
        } else fallo(2, "argumento desconocido");
    }
    int m_fondo = !strcmp(modo, "fondo"), m_subida = !strcmp(modo, "subida"), m_lectura = !strcmp(modo, "lectura");
    if (!m_fondo && !m_subida && !m_lectura && strcmp(modo, "escritorio")) fallo(2, "modo desconocido");
    if (ancho < 1920 || alto < 1080) fallo(2, "la escena mide 1920 x 1080 como poco");

    PFNEGLGETPLATFORMDISPLAYEXTPROC plataforma =
        (PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
    if (!plataforma) fallo(3, "EGL sin eglGetPlatformDisplayEXT");
    EGLDisplay dpy;
    if (sin_dispositivo) {
        dpy = plataforma(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL);
    } else {
        int fd = open(dispositivo, O_RDWR | O_CLOEXEC);
        if (fd < 0) fallo(3, "no se abre el dispositivo de la GPU");
        /* Que sea un nodo de DRM (número mayor 226): con otra cosa, Mesa caería en la CPU sin decir nada. */
        struct stat st;
        if (fstat(fd, &st) || !S_ISCHR(st.st_mode) || major(st.st_rdev) != 226) fallo(3, "eso no es una GPU (DRM)");
        struct gbm_device *gbm = gbm_create_device(fd);
        if (!gbm) fallo(3, "GBM no acepta el dispositivo");
        dpy = plataforma(EGL_PLATFORM_GBM_KHR, gbm, NULL);
    }
    EGLint mayor, menor;
    if (dpy == EGL_NO_DISPLAY || !eglInitialize(dpy, &mayor, &menor)) fallo(3, "EGL no arranca");
    eglBindAPI(EGL_OPENGL_ES_API);
    const EGLint atributos[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    EGLContext ctx = eglCreateContext(dpy, EGL_NO_CONFIG_KHR, EGL_NO_CONTEXT, atributos);
    if (ctx == EGL_NO_CONTEXT || !eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, ctx))
        fallo(3, "sin contexto de GLES2 sin superficie");
    const char *quien = (const char *)glGetString(GL_RENDERER);
    printf("componer_gl: GL_RENDERER %s; %s\n", quien, (const char *)glGetString(GL_VERSION));
    if (!sin_dispositivo && quien && (strstr(quien, "llvmpipe") || strstr(quien, "softpipe")))
        fallo(3, "se pidió una GPU y Mesa pinta con la CPU");

    /* El cuadro: una textura de la pantalla entera, de destino. */
    GLuint destino, fbo;
    glGenTextures(1, &destino);
    glBindTexture(GL_TEXTURE_2D, destino);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, ancho, alto, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    glGenFramebuffers(1, &fbo);
    glBindFramebuffer(GL_FRAMEBUFFER, fbo);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, destino, 0);
    if (glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) fallo(3, "el cuadro no se puede pintar");

    static const float esquinas[] = {0, 0, 1, 0, 0, 1, 1, 1};
    GLuint vbo;
    glGenBuffers(1, &vbo);
    glBindBuffer(GL_ARRAY_BUFFER, vbo);
    glBufferData(GL_ARRAY_BUFFER, sizeof esquinas, esquinas, GL_STATIC_DRAW);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, 0);
    glEnableVertexAttribArray(0);
    p_fondo = programa(fs_fondo);
    p_ventana = programa(fs_ventana);
    p_sombra = programa(fs_sombra);

    for (int i = 0; i < 4; i++) {
        struct ventana *v = &ventanas[i];
        v->px = malloc((size_t)v->w * v->h * 4);
        if (!v->px) fallo(3, "sin memoria");
        pintar_ventana_cpu(v, 0);
        glGenTextures(1, &v->tex);
        glBindTexture(GL_TEXTURE_2D, v->tex);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, v->w, v->h, 0, GL_RGBA, GL_UNSIGNED_BYTE, v->px);
    }
    if (glGetError() != GL_NO_ERROR) fallo(3, "GL dio un error al preparar");

    int bien = comprobar();

    uint8_t *vuelta = m_lectura ? malloc((size_t)ancho * alto * 4) : NULL;
    double *t = malloc(sizeof(double) * cuadros);
    if (!t || (m_lectura && !vuelta)) fallo(3, "sin memoria");
    /* Un cuadro de calentamiento, y a medir. */
    componer(0.0, !m_fondo);
    glFinish();
    double cpu0 = ahora(CLOCK_PROCESS_CPUTIME_ID), pared0 = ahora(CLOCK_MONOTONIC);
    for (int i = 0; i < cuadros; i++) {
        double a = ahora(CLOCK_MONOTONIC);
        if (m_subida) {
            struct ventana *v = &ventanas[0];
            pintar_ventana_cpu(v, i + 1);
            glBindTexture(GL_TEXTURE_2D, v->tex);
            glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, v->w, v->h, GL_RGBA, GL_UNSIGNED_BYTE, v->px);
        }
        componer(i / 60.0, !m_fondo);
        if (m_lectura) glReadPixels(0, 0, ancho, alto, GL_RGBA, GL_UNSIGNED_BYTE, vuelta);
        glFinish();
        t[i] = ahora(CLOCK_MONOTONIC) - a;
    }
    double cpu = ahora(CLOCK_PROCESS_CPUTIME_ID) - cpu0, pared = ahora(CLOCK_MONOTONIC) - pared0;
    /* Lo que gasta en la CPU pintar la ventana que sube (en un cliente de verdad lo pinta el cliente, no el
     * compositor): se mide aparte y se descuenta. */
    double pintar = 0;
    if (m_subida) {
        double c = ahora(CLOCK_PROCESS_CPUTIME_ID);
        for (int i = 0; i < 20; i++) pintar_ventana_cpu(&ventanas[0], i + 1);
        pintar = (ahora(CLOCK_PROCESS_CPUTIME_ID) - c) / 20;
    }
    if (glGetError() != GL_NO_ERROR) fallo(3, "GL dio un error al componer");
    qsort(t, cuadros, sizeof(double), cmp);
    printf("componer_gl: %s %dx%d, %d cuadros: mediana %.2f ms, p95 %.2f ms por cuadro (%.0f por segundo); CPU del "
           "proceso %.2f ms por cuadro%s\n",
           modo, ancho, alto, cuadros, t[cuadros / 2], t[cuadros * 95 / 100], cuadros * 1000.0 / pared,
           cpu / cuadros - pintar, m_subida ? " (sin lo que cuesta pintar la ventana en la CPU)" : "");
    return bien ? 0 : 1;
}
