/* El /init del dominio de gráficos de x86 (X12, paso G5 de docs/diseño/S38-gpu-escritorio.md): pinta con la GPU que
 * DashOS le pasa (en QEMU, la virtio-gpu con virgl) y saca los cuadros por KMS, y mide la CPU que gasta cada cuadro.
 *
 * Lo que hace:
 *
 *  - monta devtmpfs en /dev, proc en /proc y sysfs en /sys (libdrm busca el aparato por /sys), y abre /dev/kmsg: todo
 *    lo que dice va ahí, con el prefijo "dominio: " (sale por la consola temprana del kernel);
 *  - abre /dev/dri/card0, busca un conector conectado y su modo preferido, y monta GBM y EGL (plataforma GBM) con un
 *    contexto de OpenGL ES 2. Dice qué pinta (GL_RENDERER: con virgl, la GPU del PC que hay detrás de QEMU);
 *  - durante dashos.graficos=<segundos> (de la línea del kernel; 20 si no está) pinta cada cuadro entero: un fondo
 *    animado por un sombreador (un plasma que cambia en cada cuadro) y un cuadrado que gira, como el fondo y las
 *    ventanas de un escritorio; lo cambia por KMS (drmModePageFlip, esperando a que el anterior se vea);
 *  - dice "dominio: pintando ..." tras el primer cuadro en pantalla, cada 5 s los cuadros por segundo y, al final, los
 *    cuadros, el tiempo y la CPU del programa por cuadro (getrusage: lo suyo y lo del kernel del dominio en sus
 *    llamadas; no las interrupciones);
 *  - comprueba un píxel del último cuadro (glReadPixels) contra la cuenta del sombreador hecha aquí;
 *  - X13, si la línea del kernel trae dashos.compartida=<dirección>,<tamaño>: la ventana compartida con DashOS (la
 *    memoria de un programa de DashOS proyectada en el dominio, sin copia; `usuario/src/protocolo/compartida.rs`). Por
 *    /dev/mem ve la página de control y el búfer de la ranura 0; cada vez que DashOS dice que hay un cuadro nuevo
 *    (su secuencia), comprueba unos píxeles en la memoria compartida contra la cuenta de su dibujo, lo sube a una
 *    textura, lo pinta encima del fondo, lo cambia a la pantalla y apunta lo visto. Cada 30, comprueba también un píxel
 *    de la pantalla. Tras apuntar cada cuadro visto, toca el timbre (un OUT al puerto 0x5F0) para que DashOS avise al
 *    programa. Acaba a los 3 s sin cuadros nuevos (o si DashOS marca la ranura como acabada);
 *  - "dominio: fin graficos <código>" (0 si todo bien), sync y apaga. Sin ACPI el kernel no sabe apagar y se para; el
 *    último cuadro se queda en la pantalla.
 *
 * Se compila con la toolchain de musl de Bootlin contra el Mesa (solo virgl), libdrm y GBM que hace
 * tools/dominio_graficos.py. Solo usa sus interfaces públicas (EGL, GLES 2, GBM, libdrm). */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <poll.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <sys/io.h>
#include <sys/mman.h>
#include <sys/mount.h>
#include <sys/reboot.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <gbm.h>
#include <xf86drm.h>
#include <xf86drmMode.h>

static int kmsg = -1;

static void decir(const char *fmt, ...) {
    char b[600];
    int n = snprintf(b, sizeof b, "<5>dominio: ");
    va_list ap;
    va_start(ap, fmt);
    n += vsnprintf(b + n, sizeof b - n, fmt, ap);
    va_end(ap);
    if (n > (int)sizeof b - 2)
        n = sizeof b - 2;
    b[n++] = '\n';
    if (kmsg >= 0)
        write(kmsg, b, n);
    else
        write(2, b + 3, n - 3);
}

static void apagar(int codigo) {
    decir("fin graficos %d", codigo);
    sync();
    reboot(RB_POWER_OFF);
    for (;;)
        pause();
}

static double ahora(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static double cpu_propia(void) {
    struct rusage r;
    getrusage(RUSAGE_SELF, &r);
    return r.ru_utime.tv_sec + r.ru_utime.tv_usec / 1e6 + r.ru_stime.tv_sec + r.ru_stime.tv_usec / 1e6;
}

/* El valor de `nombre` en la línea del kernel (en `b`, hasta el espacio), o NULL. */
static char *de_la_linea(const char *nombre, char *b, int largo) {
    int f = open("/proc/cmdline", O_RDONLY);
    if (f < 0)
        return NULL;
    int n = read(f, b, largo - 1);
    close(f);
    if (n <= 0)
        return NULL;
    b[n] = 0;
    char *p = strstr(b, nombre);
    if (!p)
        return NULL;
    p += strlen(nombre);
    p[strcspn(p, " \n")] = 0;
    return p;
}

/* La orden: dashos.graficos=<segundos>. */
static int segundos_pedidos(void) {
    char b[2048];
    int f = open("/proc/cmdline", O_RDONLY);
    if (f < 0)
        return 20;
    int n = read(f, b, sizeof b - 1);
    close(f);
    if (n <= 0)
        return 20;
    b[n] = 0;
    char *p = strstr(b, "dashos.graficos=");
    if (!p)
        return 20;
    int s = atoi(p + strlen("dashos.graficos="));
    return s > 0 && s < 3600 ? s : 20;
}

/* Las interrupciones del dominio (/proc/interrupts), una línea cada una: para ver por dónde le llegan las de la GPU. */
static void decir_interrupciones(void) {
    char b[4096];
    int f = open("/proc/interrupts", O_RDONLY);
    if (f < 0)
        return;
    int n = read(f, b, sizeof b - 1);
    close(f);
    if (n <= 0)
        return;
    b[n] = 0;
    for (char *l = strtok(b, "\n"); l; l = strtok(NULL, "\n")) {
        while (*l == ' ')
            l++;
        decir("interrupciones: %s", l);
    }
}

/* --- Lo que se pinta --- */

static const char *vs_fondo = "attribute vec2 pos;\n"
                              "varying vec2 uv;\n"
                              "void main() { uv = pos * 0.5 + 0.5; gl_Position = vec4(pos, 0.0, 1.0); }\n";
/* El plasma: cada píxel cambia con el tiempo. La misma cuenta está en `plasma` (abajo), para comprobar un píxel. */
static const char *fs_fondo = "precision mediump float;\n"
                              "varying vec2 uv;\n"
                              "uniform float t;\n"
                              "void main() {\n"
                              "  float v = sin(uv.x * 10.0 + t) + sin(uv.y * 8.0 - t * 1.3) + sin((uv.x + uv.y) * 6.0 + t * 0.7);\n"
                              "  gl_FragColor = vec4(0.5 + 0.5 * sin(v), 0.5 + 0.5 * sin(v + 2.094), 0.5 + 0.5 * sin(v + 4.189), 1.0);\n"
                              "}\n";
static const char *vs_cuadro = "attribute vec2 pos;\n"
                               "uniform float a;\n"
                               "uniform vec2 escala;\n"
                               "void main() {\n"
                               "  mat2 r = mat2(cos(a), sin(a), -sin(a), cos(a));\n"
                               "  gl_Position = vec4((r * pos) * escala, 0.0, 1.0);\n"
                               "}\n";
static const char *fs_cuadro = "precision mediump float;\n"
                               "void main() { gl_FragColor = vec4(0.95, 0.95, 0.92, 1.0); }\n";

static void plasma(float x, float y, float t, float rgb[3]) {
    float v = sinf(x * 10.0f + t) + sinf(y * 8.0f - t * 1.3f) + sinf((x + y) * 6.0f + t * 0.7f);
    rgb[0] = 0.5f + 0.5f * sinf(v);
    rgb[1] = 0.5f + 0.5f * sinf(v + 2.094f);
    rgb[2] = 0.5f + 0.5f * sinf(v + 4.189f);
}

static GLuint sombreador(GLenum tipo, const char *fuente) {
    GLuint s = glCreateShader(tipo);
    glShaderSource(s, 1, &fuente, NULL);
    glCompileShader(s);
    GLint bien = 0;
    glGetShaderiv(s, GL_COMPILE_STATUS, &bien);
    if (!bien) {
        char log[512] = "";
        glGetShaderInfoLog(s, sizeof log, NULL, log);
        decir("el sombreador no compila: %s", log);
        apagar(5);
    }
    return s;
}

static GLuint programa(const char *vs, const char *fs) {
    GLuint p = glCreateProgram();
    glAttachShader(p, sombreador(GL_VERTEX_SHADER, vs));
    glAttachShader(p, sombreador(GL_FRAGMENT_SHADER, fs));
    glBindAttribLocation(p, 0, "pos");
    glLinkProgram(p);
    GLint bien = 0;
    glGetProgramiv(p, GL_LINK_STATUS, &bien);
    if (!bien) {
        decir("el programa de GL no enlaza");
        apagar(5);
    }
    return p;
}

/* --- KMS --- */

static int fd = -1;
static uint32_t crtc, conector;
static drmModeModeInfo modo;
static int cambio_pendiente;

static void al_cambiar(int f, unsigned int cuadro, unsigned int sec, unsigned int usec, void *dato) {
    (void)f;
    (void)cuadro;
    (void)sec;
    (void)usec;
    *(int *)dato = 0;
}

static void soltar_fb(struct gbm_bo *bo, void *dato) {
    (void)bo;
    uint32_t id = (uint32_t)(uintptr_t)dato;
    if (id)
        drmModeRmFB(fd, id);
}

static uint32_t fb_de(struct gbm_bo *bo) {
    void *d = gbm_bo_get_user_data(bo);
    if (d)
        return (uint32_t)(uintptr_t)d;
    uint32_t id = 0;
    if (drmModeAddFB(fd, gbm_bo_get_width(bo), gbm_bo_get_height(bo), 24, 32, gbm_bo_get_stride(bo), gbm_bo_get_handle(bo).u32, &id)) {
        decir("drmModeAddFB: %s", strerror(errno));
        apagar(4);
    }
    gbm_bo_set_user_data(bo, (void *)(uintptr_t)id, soltar_fb);
    return id;
}

static void buscar_pantalla(void) {
    drmModeRes *res = drmModeGetResources(fd);
    if (!res) {
        decir("drmModeGetResources: %s", strerror(errno));
        apagar(3);
    }
    drmModeConnector *c = NULL;
    for (int i = 0; i < res->count_connectors; i++) {
        c = drmModeGetConnector(fd, res->connectors[i]);
        if (c && c->connection == DRM_MODE_CONNECTED && c->count_modes > 0)
            break;
        drmModeFreeConnector(c);
        c = NULL;
    }
    if (!c) {
        decir("ningún conector conectado");
        apagar(3);
    }
    conector = c->connector_id;
    modo = c->modes[0];
    for (int i = 0; i < c->count_modes; i++)
        if (c->modes[i].type & DRM_MODE_TYPE_PREFERRED) {
            modo = c->modes[i];
            break;
        }
    drmModeEncoder *e = c->encoder_id ? drmModeGetEncoder(fd, c->encoder_id) : NULL;
    if (e && e->crtc_id)
        crtc = e->crtc_id;
    else {
        for (int i = 0; i < c->count_encoders && !crtc; i++) {
            drmModeEncoder *x = drmModeGetEncoder(fd, c->encoders[i]);
            for (int j = 0; x && j < res->count_crtcs; j++)
                if (x->possible_crtcs & (1u << j)) {
                    crtc = res->crtcs[j];
                    break;
                }
            drmModeFreeEncoder(x);
        }
    }
    if (!crtc) {
        decir("sin CRTC para el conector %u", conector);
        apagar(3);
    }
    decir("pantalla: conector %u, CRTC %u, %ux%u a %u Hz", conector, crtc, modo.hdisplay, modo.vdisplay, modo.vrefresh);
    drmModeFreeEncoder(e);
    drmModeFreeConnector(c);
    drmModeFreeResources(res);
}

/* El cuadro pintado, a la pantalla por KMS: el primero con drmModeSetCrtc, los demás con drmModePageFlip, esperando a
 * que se vea. 0 si bien; 6 si algo falló (ya dicho). */
static struct gbm_bo *anterior;

static int mostrar(EGLDisplay d, EGLSurface s, struct gbm_surface *sup, int primero) {
    drmEventContext ev = {.version = 2, .page_flip_handler = al_cambiar};
    if (!eglSwapBuffers(d, s)) {
        decir("eglSwapBuffers: 0x%x", eglGetError());
        return 6;
    }
    struct gbm_bo *bo = gbm_surface_lock_front_buffer(sup);
    uint32_t id = bo ? fb_de(bo) : 0;
    if (!id) {
        decir("sin búfer de pantalla");
        return 6;
    }
    if (primero) {
        if (drmModeSetCrtc(fd, crtc, id, 0, 0, &conector, 1, &modo)) {
            decir("drmModeSetCrtc: %s", strerror(errno));
            return 6;
        }
    } else {
        cambio_pendiente = 1;
        if (drmModePageFlip(fd, crtc, id, DRM_MODE_PAGE_FLIP_EVENT, &cambio_pendiente)) {
            decir("drmModePageFlip: %s", strerror(errno));
            return 6;
        }
        while (cambio_pendiente) {
            struct pollfd pf = {.fd = fd, .events = POLLIN};
            if (poll(&pf, 1, 3000) <= 0) {
                decir("el cambio de búfer no llega");
                return 6;
            }
            drmHandleEvent(fd, &ev);
        }
    }
    if (anterior)
        gbm_surface_release_buffer(sup, anterior);
    anterior = bo;
    return 0;
}

/* --- X13: la ventana compartida con DashOS (`usuario/src/protocolo/compartida.rs`, la misma disposición) --- */

#define MAGIA 0x4D485344u /* "DSHM" */
#define RANURAS_DESDE 0x40
#define CONTROL 0x10000
#define PUERTO_AVISO 0x5f0 /* el timbre: tras apuntar lo visto, el número de la ranura */

struct ranura {
    uint32_t estado; /* 0 libre, 1 puesta, 2 acabada */
    uint32_t ancho, alto, paso;
    uint64_t desplazamiento, tamano;
    uint64_t secuencia; /* el último cuadro listo (lo escribe DashOS) */
    uint64_t visto;     /* el último subido (lo escribe el dominio) */
    uint32_t formato;   /* 0: bytes R, G, B, A; 1: B, G, R, A (XRGB8888 de DashOS) */
    uint32_t relleno;
    uint64_t subir_us; /* lo que llevó subirlos, sumado (lo escribe el dominio, para medir) */
};

/* El dibujo de prueba de `graficos-cliente` (`usuario/src/bin/graficos_cliente.rs`, `dibujo`): bloques de 16 x 16 que
 * cambian con el cuadro `k`, como XRGB8888. */
static uint32_t dibujo(uint32_t x, uint32_t y, uint64_t k) {
    uint32_t bx = x / 16, by = y / 16, kk = (uint32_t)k;
    uint32_t r = (bx * 7 + kk * 3) & 255, g = (by * 11 + kk * 5) & 255, b = ((bx ^ by) * 13 + kk) & 255;
    return 0xff000000u | r << 16 | g << 8 | b;
}

static const char *vs_ventana = "attribute vec2 pos;\n"
                                "uniform float esc;\n"
                                "varying vec2 uv;\n"
                                "void main() { uv = vec2(0.5 + pos.x * 0.5, 0.5 - pos.y * 0.5); gl_Position = vec4(pos * esc, 0.0, 1.0); }\n";
static const char *fs_ventana = "precision mediump float;\n"
                                "varying vec2 uv;\n"
                                "uniform sampler2D tex;\n"
                                "uniform float bgra;\n"
                                "void main() { vec4 c = texture2D(tex, uv); gl_FragColor = vec4(mix(c.rgb, c.bgr, bgra), 1.0); }\n";

/* Lo que mide la ventana en la pantalla (de -ESC a ESC en las dos direcciones). */
#define ESC 0.7f

/* La columna o fila de la pantalla (contando desde abajo en y) que cae en la textura en `quiere` (de 0 a 1), lejos del
 * borde de un bloque; y el texel que le toca. */
static void punto(int pantalla, int texeles, float quiere, int abajo, int *p, int *texel) {
    for (int d = 0; d < 64; d++) {
        float n0 = (quiere * 2 - 1) * ESC;
        if (abajo)
            n0 = -n0;
        int c = (int)((n0 + 1) / 2 * pantalla) + d;
        float n = ((c + 0.5f) / pantalla * 2 - 1) / ESC;
        float u = abajo ? 0.5f - n * 0.5f : 0.5f + n * 0.5f;
        int t = (int)(u * texeles);
        if (t % 16 >= 3 && t % 16 <= 12) {
            *p = c;
            *texel = t;
            return;
        }
    }
    *p = pantalla / 2;
    *texel = texeles / 2;
}

static int compartida(EGLDisplay d, EGLSurface s, struct gbm_surface *sup, GLuint p_fondo, GLint u_t, double t_arranque) {
    char linea[2048];
    char *v = de_la_linea("dashos.compartida=", linea, sizeof linea);
    if (!v)
        return 0;
    uint64_t base = strtoull(v, &v, 0), tam = *v == ',' ? strtoull(v + 1, NULL, 0) : 0;
    if (!base || tam < CONTROL) {
        decir("compartida: la línea no vale");
        return 8;
    }
    int mem = open("/dev/mem", O_RDWR | O_CLOEXEC);
    volatile uint8_t *control = mem < 0 ? MAP_FAILED : mmap(NULL, CONTROL, PROT_READ | PROT_WRITE, MAP_SHARED, mem, base);
    if (control == MAP_FAILED) {
        decir("compartida: no se ve la página de control en %#llx: %s", (unsigned long long)base, strerror(errno));
        return 8;
    }
    if (*(volatile uint32_t *)control != MAGIA) {
        decir("compartida: la página de control no es de DashOS (%#x)", *(volatile uint32_t *)control);
        return 8;
    }
    struct ranura *r = (struct ranura *)(control + RANURAS_DESDE);
    double espera = ahora();
    while (__atomic_load_n(&r->estado, __ATOMIC_ACQUIRE) == 0) {
        if (ahora() - espera > 60) {
            decir("compartida: nadie pone una ventana en 60 s");
            return 9;
        }
        usleep(1000);
    }
    uint32_t ancho = r->ancho, alto = r->alto, paso = r->paso, formato = r->formato;
    uint64_t desp = r->desplazamiento, largo = r->tamano;
    if (paso != ancho * 4 || (uint64_t)paso * alto > largo || desp < CONTROL || desp + largo > tam || desp % 4096) {
        decir("compartida: la ranura 0 no vale (%ux%u, paso %u, %llu bytes en +%#llx)", ancho, alto, paso, (unsigned long long)largo, (unsigned long long)desp);
        return 9;
    }
    if (ioperm(PUERTO_AVISO, 4, 1)) {
        decir("compartida: sin el puerto del timbre: %s", strerror(errno));
        return 9;
    }
    const uint8_t *px = mmap(NULL, largo, PROT_READ, MAP_SHARED, mem, base + desp);
    if (px == MAP_FAILED) {
        decir("compartida: no se ve el búfer: %s", strerror(errno));
        return 9;
    }
    decir("compartida: ventana de %ux%u (formato %u) en %#llx, %.0f ms tras pedirla", ancho, alto, formato, (unsigned long long)(base + desp), (ahora() - espera) * 1000);
    /* Cuánto cuesta leer la memoria compartida (si el dominio la viera sin caché, aquí se notaría). */
    uint8_t *copia = malloc(largo);
    double t = ahora();
    for (int i = 0; i < 4; i++)
        memcpy(copia, px, largo);
    double mbs = 4.0 * largo / (ahora() - t) / 1e6;
    free(copia);
    decir("compartida: leer el búfer compartido, %.0f MB/s", mbs);

    GLuint p = programa(vs_ventana, fs_ventana);
    GLint u_esc = glGetUniformLocation(p, "esc"), u_bgra = glGetUniformLocation(p, "bgra");
    GLuint tex;
    glGenTextures(1, &tex);
    glBindTexture(GL_TEXTURE_2D, tex);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glPixelStorei(GL_UNPACK_ALIGNMENT, 4);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, ancho, alto, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    static const GLfloat quad[] = {-1, -1, 1, -1, -1, 1, 1, 1};
    int sx, sy, tx, ty;
    punto(modo.hdisplay, ancho, 0.53f, 0, &sx, &tx);
    punto(modo.vdisplay, alto, 0.48f, 1, &sy, &ty);
    const uint32_t muestras[5][2] = {{0, 0}, {ancho - 1, 0}, {0, alto - 1}, {ancho - 1, alto - 1}, {ancho / 2 + 5, alto / 2 + 5}};

    int cuadros = 0, en_memoria = 0, en_pantalla = 0, mal = 0;
    double subir = 0, subir_cpu = 0, cpu0 = cpu_propia(), t0 = ahora(), ultimo = ahora();
    uint64_t visto = 0;
    for (;;) {
        uint64_t k = __atomic_load_n(&r->secuencia, __ATOMIC_ACQUIRE);
        if (k == visto) {
            if (__atomic_load_n(&r->estado, __ATOMIC_ACQUIRE) == 2 || (cuadros && ahora() - ultimo > 3) || ahora() - ultimo > 60)
                break;
            usleep(200);
            continue;
        }
        ultimo = ahora();
        /* Lo que dice la memoria compartida, sin copiarla: el cuadro k tal cual lo dejó DashOS. */
        for (int i = 0; i < 5; i++) {
            uint32_t x = muestras[i][0], y = muestras[i][1];
            uint32_t hay = *(const volatile uint32_t *)(px + (uint64_t)y * paso + 4 * x), quiere = dibujo(x, y, k);
            if (hay != quiere) {
                if (mal < 5)
                    decir("compartida: cuadro %llu, (%u, %u) en memoria: %#x, esperado %#x", (unsigned long long)k, x, y, hay, quiere);
                mal++;
            }
        }
        en_memoria++;
        double a = ahora(), ac = cpu_propia();
        glTexSubImage2D(GL_TEXTURE_2D, 0, 0, 0, ancho, alto, GL_RGBA, GL_UNSIGNED_BYTE, px);
        subir += ahora() - a;
        subir_cpu += cpu_propia() - ac;
        glUseProgram(p_fondo);
        glUniform1f(u_t, (float)(ahora() - t_arranque));
        glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, quad);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        glUseProgram(p);
        glUniform1f(u_esc, ESC);
        glUniform1f(u_bgra, formato == 1 ? 1.0f : 0.0f);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        if (cuadros % 30 == 0) {
            unsigned char c[4] = {0};
            glReadPixels(sx, sy, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, c);
            uint32_t q = dibujo(tx, ty, k);
            int dr = abs((int)((q >> 16) & 255) - c[0]), dg = abs((int)((q >> 8) & 255) - c[1]), db = abs((int)(q & 255) - c[2]);
            if (dr > 2 || dg > 2 || db > 2) {
                if (mal < 5)
                    decir("compartida: cuadro %llu, pantalla (%d, %d) = texel (%d, %d): %u %u %u, esperado %u %u %u", (unsigned long long)k, sx, sy, tx, ty, c[0], c[1], c[2], (q >> 16) & 255, (q >> 8) & 255, q & 255);
                mal++;
            }
            en_pantalla++;
        }
        int e = mostrar(d, s, sup, 0);
        if (e)
            return e;
        cuadros++;
        visto = k;
        r->subir_us = (uint64_t)(subir * 1e6);
        __atomic_store_n(&r->visto, k, __ATOMIC_RELEASE);
        outl(0, PUERTO_AVISO);
    }
    double tiempo = ultimo - t0, cpu = cpu_propia() - cpu0;
    if (!cuadros) {
        decir("compartida: ningún cuadro en 60 s");
        return 10;
    }
    decir("compartida: %d cuadros de %ux%u en %.0f ms, %d comprobados en memoria y %d en pantalla, %d mal; subir %.3f ms por cuadro (CPU %.3f ms); CPU del programa %.3f ms por cuadro", cuadros, ancho, alto, tiempo * 1000, en_memoria, en_pantalla, mal, subir * 1000 / cuadros, subir_cpu * 1000 / cuadros, cpu * 1000 / cuadros);
    return mal ? 10 : 0;
}

int main(void) {
    mkdir("/dev", 0755);
    mkdir("/proc", 0555);
    mkdir("/sys", 0555);
    mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
    mount("proc", "/proc", "proc", 0, NULL);
    mount("sysfs", "/sys", "sysfs", 0, NULL);
    kmsg = open("/dev/kmsg", O_WRONLY | O_CLOEXEC);
    int segundos = segundos_pedidos();
    double t_arranque = ahora();

    for (int i = 0; i < 50 && fd < 0; i++) {
        fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC);
        if (fd < 0)
            usleep(100000);
    }
    if (fd < 0) {
        decir("no hay /dev/dri/card0: %s", strerror(errno));
        decir_interrupciones();
        apagar(2);
    }
    drmVersion *v = drmGetVersion(fd);
    if (v) {
        decir("DRM: %s %d.%d.%d (%s)", v->name, v->version_major, v->version_minor, v->version_patchlevel, v->desc);
        drmFreeVersion(v);
    }
    buscar_pantalla();

    struct gbm_device *gbm = gbm_create_device(fd);
    struct gbm_surface *sup = gbm ? gbm_surface_create(gbm, modo.hdisplay, modo.vdisplay, GBM_FORMAT_XRGB8888, GBM_BO_USE_SCANOUT | GBM_BO_USE_RENDERING) : NULL;
    if (!sup) {
        decir("GBM: no se pudo crear la superficie");
        apagar(4);
    }
    EGLDisplay d = eglGetPlatformDisplay(EGL_PLATFORM_GBM_KHR, gbm, NULL);
    EGLint mayor, menor;
    if (d == EGL_NO_DISPLAY || !eglInitialize(d, &mayor, &menor)) {
        decir("EGL no arranca (0x%x)", eglGetError());
        apagar(4);
    }
    eglBindAPI(EGL_OPENGL_ES_API);
    static const EGLint pide[] = {EGL_SURFACE_TYPE, EGL_WINDOW_BIT, EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE};
    EGLConfig cfgs[64];
    EGLint n = 0;
    eglChooseConfig(d, pide, cfgs, 64, &n);
    EGLConfig cfg = NULL;
    for (int i = 0; i < n; i++) {
        EGLint id;
        if (eglGetConfigAttrib(d, cfgs[i], EGL_NATIVE_VISUAL_ID, &id) && id == GBM_FORMAT_XRGB8888) {
            cfg = cfgs[i];
            break;
        }
    }
    if (!cfg) {
        decir("EGL: ninguna configuración XRGB8888 (%d)", n);
        apagar(4);
    }
    static const EGLint ctx_attr[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    EGLContext ctx = eglCreateContext(d, cfg, EGL_NO_CONTEXT, ctx_attr);
    EGLSurface s = eglCreatePlatformWindowSurface(d, cfg, sup, NULL);
    if (ctx == EGL_NO_CONTEXT || s == EGL_NO_SURFACE || !eglMakeCurrent(d, s, s, ctx)) {
        decir("EGL: sin contexto o superficie (0x%x)", eglGetError());
        apagar(4);
    }
    decir("EGL %d.%d; GL_RENDERER %s; GL_VERSION %s", mayor, menor, glGetString(GL_RENDERER), glGetString(GL_VERSION));

    GLuint p_fondo = programa(vs_fondo, fs_fondo), p_cuadro = programa(vs_cuadro, fs_cuadro);
    GLint u_t = glGetUniformLocation(p_fondo, "t"), u_a = glGetUniformLocation(p_cuadro, "a"), u_esc = glGetUniformLocation(p_cuadro, "escala");
    static const GLfloat pantalla[] = {-1, -1, 1, -1, -1, 1, 1, 1};
    static const GLfloat cuadro[] = {-1, -1, 1, -1, -1, 1, 1, 1};
    glViewport(0, 0, modo.hdisplay, modo.vdisplay);
    glEnableVertexAttribArray(0);
    float aspecto = (float)modo.vdisplay / modo.hdisplay;

    int cuadros = 0, cuadros_ultimo = 0, codigo = 0;
    double t0 = 0, t_ultimo = 0, cpu0 = 0;
    float t_ult = 0;
    for (;;) {
        double t = ahora();
        if (cuadros == 1) {
            /* La medida empieza con el primer cuadro en pantalla (sin lo de montar EGL ni compilar sombreadores). */
            t0 = t_ultimo = t;
            cpu0 = cpu_propia();
        }
        if (cuadros > 1 && t - t0 >= segundos)
            break;
        float tt = (float)(t - t_arranque);
        t_ult = tt;
        glUseProgram(p_fondo);
        glUniform1f(u_t, tt);
        glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, pantalla);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        glUseProgram(p_cuadro);
        glUniform1f(u_a, tt * 1.5f);
        glUniform2f(u_esc, 0.25f * aspecto, 0.25f);
        glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, cuadro);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        if ((codigo = mostrar(d, s, sup, !cuadros)))
            break;
        cuadros++;
        if (cuadros == 1)
            decir("pintando %ux%u por KMS (%.0f ms desde el /init)", modo.hdisplay, modo.vdisplay, (ahora() - t_arranque) * 1000);
        if (cuadros > 1 && t - t_ultimo >= 5) {
            decir("cuadros: %d, %.1f por segundo", cuadros - 1, (cuadros - cuadros_ultimo) / (t - t_ultimo));
            t_ultimo = t;
            cuadros_ultimo = cuadros;
        }
    }
    if (!codigo) {
        /* Un píxel del último cuadro (aún en el búfer de atrás no, en el de delante: se pinta otra vez el fondo solo
         * con el mismo tiempo y se lee), contra la cuenta hecha aquí. Una esquina, lejos del cuadrado. */
        double t = ahora();
        int medidos = cuadros - 1;
        double cpu = cpu_propia() - cpu0, tiempo = t - t0;
        glUseProgram(p_fondo);
        glUniform1f(u_t, t_ult);
        glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, pantalla);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        unsigned char px[4] = {0};
        int x = modo.hdisplay / 8, y = modo.vdisplay / 8;
        glReadPixels(x, y, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, px);
        float rgb[3];
        plasma((x + 0.5f) / modo.hdisplay, (y + 0.5f) / modo.vdisplay, t_ult, rgb);
        int dif = 0;
        for (int k = 0; k < 3; k++) {
            int e = (int)lroundf(rgb[k] * 255.0f) - px[k];
            if (abs(e) > dif)
                dif = abs(e);
        }
        decir("píxel (%d, %d): %u %u %u, esperado %d %d %d (diferencia %d)", x, y, px[0], px[1], px[2], (int)lroundf(rgb[0] * 255), (int)lroundf(rgb[1] * 255), (int)lroundf(rgb[2] * 255), dif);
        if (dif > 8)
            codigo = 7;
        decir_interrupciones();
        decir("cuadros %d en %.0f ms (%.1f por segundo); CPU del programa %.0f ms (%.3f ms por cuadro)", medidos, tiempo * 1000,
              medidos / tiempo, cpu * 1000, cpu * 1000 / medidos);
        /* Lo último a la vista: el cuadro que se pintó al final (el de la comprobación no se cambia a la pantalla). */
    }
    if (!codigo)
        codigo = compartida(d, s, sup, p_fondo, u_t, t_arranque);
    apagar(codigo);
    return 0;
}
