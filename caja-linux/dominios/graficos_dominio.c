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

    drmEventContext ev = {.version = 2, .page_flip_handler = al_cambiar};
    struct gbm_bo *anterior = NULL;
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
        if (!eglSwapBuffers(d, s)) {
            decir("eglSwapBuffers: 0x%x", eglGetError());
            codigo = 6;
            break;
        }
        struct gbm_bo *bo = gbm_surface_lock_front_buffer(sup);
        uint32_t id = bo ? fb_de(bo) : 0;
        if (!id) {
            decir("sin búfer de pantalla");
            codigo = 6;
            break;
        }
        if (!cuadros) {
            if (drmModeSetCrtc(fd, crtc, id, 0, 0, &conector, 1, &modo)) {
                decir("drmModeSetCrtc: %s", strerror(errno));
                codigo = 6;
                break;
            }
        } else {
            cambio_pendiente = 1;
            if (drmModePageFlip(fd, crtc, id, DRM_MODE_PAGE_FLIP_EVENT, &cambio_pendiente)) {
                decir("drmModePageFlip: %s", strerror(errno));
                codigo = 6;
                break;
            }
            while (cambio_pendiente) {
                struct pollfd pf = {.fd = fd, .events = POLLIN};
                if (poll(&pf, 1, 3000) <= 0) {
                    decir("el cambio de búfer no llega");
                    codigo = 6;
                    break;
                }
                drmHandleEvent(fd, &ev);
            }
            if (codigo)
                break;
        }
        if (anterior)
            gbm_surface_release_buffer(sup, anterior);
        anterior = bo;
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
    apagar(codigo);
    return 0;
}
