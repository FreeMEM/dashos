/* Un programa de OpenGL ES 2 para medir la caja Linux de DashOS (S25): pinta lo mismo por dos caminos y dice cuántos
 * cuadros por segundo saca.
 *
 *     cuadros_gl wayland [SEGUNDOS [INTERVALO]]
 *                                     una ventana de Wayland (EGL de Mesa sobre wl_egl_window): sale en el escritorio
 *                                     de DashOS por el puente (hoy, búferes de wl_shm copiados por vsock). Con
 *                                     INTERVALO 0, sin esperar al compositor entre cuadros (eglSwapInterval 0)
 *     cuadros_gl kms [SEGUNDOS]       la pantalla de la virtio-gpu de la caja (/dev/dri/card0, GBM y EGL, cambio de
 *                                     búfer con drmModePageFlip): sale en la ventana "pantalla de la caja" de DashOS
 *
 * Lo que pinta, 640 x 480: un fondo que cambia de color y un triángulo que gira, con un sombreador; la ventana entera
 * cambia en cada cuadro. Dice qué pinta (GL_RENDERER: con llvmpipe, la CPU del invitado), cada 5 s y al acabar los
 * cuadros por segundo, y "adiós". Por defecto dura 20 s.
 *
 * Se compila en el PC para aarch64 (tools/caja_linux_invitado.py) con las bibliotecas de los paquetes de Arch Linux
 * ARM (libEGL y libGLESv2 de libglvnd, libgbm de mesa, libdrm, libwayland-client y libwayland-egl). Solo usa sus
 * interfaces públicas. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <gbm.h>
#include <wayland-client.h>
#include <wayland-egl.h>
#include <xf86drm.h>
#include <xf86drmMode.h>
#include "xdg-shell-client-protocol.h"

#define ANCHO 640
#define ALTO 480

static const char *modo = "?";
static long long t0, t_ultimo;
static int cuadros, cuadros_ultimo;

static long long ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000LL + t.tv_nsec / 1000000;
}

/* --- Lo que se pinta (igual en los dos caminos) --- */

static GLuint programa;
static GLint u_angulo, u_color;

static GLuint sombreador(GLenum tipo, const char *fuente) {
    GLuint s = glCreateShader(tipo);
    glShaderSource(s, 1, &fuente, NULL);
    glCompileShader(s);
    GLint bien = 0;
    glGetShaderiv(s, GL_COMPILE_STATUS, &bien);
    if (!bien) {
        char log[512];
        glGetShaderInfoLog(s, sizeof log, NULL, log);
        printf("cuadros_gl: el sombreador no compila: %s\n", log);
        exit(1);
    }
    return s;
}

static void preparar_gl(void) {
    const char *vs =
        "attribute vec2 pos; uniform float angulo; varying vec2 v;\n"
        "void main() { float c = cos(angulo), s = sin(angulo);\n"
        "  v = pos; gl_Position = vec4(c * pos.x - s * pos.y, s * pos.x + c * pos.y, 0.0, 1.0); }\n";
    const char *fs =
        "precision mediump float; varying vec2 v; uniform vec3 color;\n"
        "void main() { gl_FragColor = vec4(color * (0.6 + 0.4 * v.x) + vec3(0.2, 0.1, 0.0) * v.y, 1.0); }\n";
    programa = glCreateProgram();
    glAttachShader(programa, sombreador(GL_VERTEX_SHADER, vs));
    glAttachShader(programa, sombreador(GL_FRAGMENT_SHADER, fs));
    glBindAttribLocation(programa, 0, "pos");
    glLinkProgram(programa);
    glUseProgram(programa);
    u_angulo = glGetUniformLocation(programa, "angulo");
    u_color = glGetUniformLocation(programa, "color");
    printf("cuadros_gl: %s: GL_RENDERER %s, GL_VERSION %s\n", modo, glGetString(GL_RENDERER), glGetString(GL_VERSION));
}

static void pintar(void) {
    static const GLfloat tri[] = {0.0f, 0.8f, -0.7f, -0.6f, 0.7f, -0.6f};
    float t = cuadros * 0.05f;
    glViewport(0, 0, ANCHO, ALTO);
    glClearColor(0.1f + 0.1f * sinf(t), 0.15f, 0.25f + 0.1f * cosf(t * 0.7f), 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    glUniform1f(u_angulo, t);
    glUniform3f(u_color, 0.9f, 0.6f + 0.3f * sinf(t * 1.3f), 0.2f);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, tri);
    glEnableVertexAttribArray(0);
    glDrawArrays(GL_TRIANGLES, 0, 3);
}

/* Cuenta un cuadro y, cada 5 s, dice cuántos por segundo. */
static void contar(void) {
    cuadros++;
    long long t = ms();
    if (cuadros == 1) printf("cuadros_gl: %s: primer cuadro a los %lld ms\n", modo, t - t0);
    if (t - t_ultimo >= 5000) {
        printf("cuadros_gl: %s: en marcha: %d cuadros en %lld ms, %.1f por segundo\n", modo, cuadros - cuadros_ultimo,
               t - t_ultimo, (cuadros - cuadros_ultimo) * 1000.0 / (t - t_ultimo));
        fflush(stdout);
        t_ultimo = t;
        cuadros_ultimo = cuadros;
    }
}

static void final(void) {
    long long t = ms() - t0;
    printf("cuadros_gl: %s: al acabar: %d cuadros en %lld ms, %.1f por segundo; adiós\n", modo, cuadros, t,
           t > 0 ? cuadros * 1000.0 / t : 0.0);
}

static EGLConfig elegir_config(EGLDisplay d, EGLint visual) {
    const EGLint atr[] = {EGL_SURFACE_TYPE, EGL_WINDOW_BIT, EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8,
                          EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE};
    EGLConfig cs[64];
    EGLint n = 0;
    if (!eglChooseConfig(d, atr, cs, 64, &n) || n == 0) {
        printf("cuadros_gl: %s: ninguna configuración de EGL\n", modo);
        exit(1);
    }
    for (int i = 0; visual && i < n; i++) {
        EGLint v = 0;
        eglGetConfigAttrib(d, cs[i], EGL_NATIVE_VISUAL_ID, &v);
        if (v == visual) return cs[i];
    }
    return cs[0];
}

static EGLContext contexto(EGLDisplay d, EGLConfig c) {
    const EGLint atr[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    eglBindAPI(EGL_OPENGL_ES_API);
    EGLContext x = eglCreateContext(d, c, EGL_NO_CONTEXT, atr);
    if (x == EGL_NO_CONTEXT) {
        printf("cuadros_gl: %s: sin contexto de GLES2 (EGL %#x)\n", modo, eglGetError());
        exit(1);
    }
    return x;
}

/* --- Por Wayland --- */

static struct wl_compositor *compositor;
static struct xdg_wm_base *wm;
static int configurada, cerrada;

static void global(void *d, struct wl_registry *r, uint32_t nombre, const char *interfaz, uint32_t version) {
    (void)d; (void)version;
    if (!strcmp(interfaz, "wl_compositor")) compositor = wl_registry_bind(r, nombre, &wl_compositor_interface, 4);
    else if (!strcmp(interfaz, "xdg_wm_base")) wm = wl_registry_bind(r, nombre, &xdg_wm_base_interface, 1);
}
static void global_quitado(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener registro = {global, global_quitado};
static void ping(void *d, struct xdg_wm_base *w, uint32_t s) { (void)d; xdg_wm_base_pong(w, s); }
static const struct xdg_wm_base_listener oyente_wm = {ping};
static void s_configure(void *d, struct xdg_surface *s, uint32_t serie) {
    (void)d;
    xdg_surface_ack_configure(s, serie);
    configurada = 1;
}
static const struct xdg_surface_listener oyente_superficie = {s_configure};
static void t_configure(void *d, struct xdg_toplevel *t, int32_t a, int32_t b, struct wl_array *e) { (void)d; (void)t; (void)a; (void)b; (void)e; }
static void t_cerrar(void *d, struct xdg_toplevel *t) { (void)d; (void)t; cerrada = 1; }
static const struct xdg_toplevel_listener oyente_ventana = {t_configure, t_cerrar};

static int por_wayland(int segundos, int intervalo) {
    struct wl_display *dpy = wl_display_connect(NULL);
    if (!dpy) {
        printf("cuadros_gl: wayland: no hay compositor (WAYLAND_DISPLAY)\n");
        return 1;
    }
    struct wl_registry *r = wl_display_get_registry(dpy);
    wl_registry_add_listener(r, &registro, NULL);
    wl_display_roundtrip(dpy);
    if (!compositor || !wm) {
        printf("cuadros_gl: wayland: faltan globales\n");
        return 1;
    }
    xdg_wm_base_add_listener(wm, &oyente_wm, NULL);
    struct wl_surface *sup = wl_compositor_create_surface(compositor);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm, sup);
    xdg_surface_add_listener(xs, &oyente_superficie, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(top, &oyente_ventana, NULL);
    xdg_toplevel_set_title(top, "cuadros_gl (Wayland)");
    wl_surface_commit(sup);
    while (!configurada && wl_display_dispatch(dpy) != -1) {}

    EGLDisplay d = eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_KHR, dpy, NULL);
    if (d == EGL_NO_DISPLAY || !eglInitialize(d, NULL, NULL)) {
        printf("cuadros_gl: wayland: EGL no arranca (%#x)\n", eglGetError());
        return 1;
    }
    EGLConfig c = elegir_config(d, 0);
    EGLContext x = contexto(d, c);
    struct wl_egl_window *w = wl_egl_window_create(sup, ANCHO, ALTO);
    EGLSurface s = eglCreatePlatformWindowSurface(d, c, w, NULL);
    if (s == EGL_NO_SURFACE || !eglMakeCurrent(d, s, s, x)) {
        printf("cuadros_gl: wayland: sin superficie de EGL (%#x)\n", eglGetError());
        return 1;
    }
    preparar_gl();
    t0 = t_ultimo = ms();
    /* Con intervalo 1, eglSwapBuffers espera el aviso de frame del compositor: un cuadro por cuadro del escritorio
     * como mucho, como cualquier programa. Con 0, no espera (mide solo pintar y copiar). */
    eglSwapInterval(d, intervalo);
    printf("cuadros_gl: wayland: intervalo %d\n", intervalo);
    while (!cerrada && ms() - t0 < segundos * 1000LL) {
        pintar();
        if (!eglSwapBuffers(d, s)) {
            printf("cuadros_gl: wayland: eglSwapBuffers falla (%#x)\n", eglGetError());
            break;
        }
        contar();
        if (wl_display_dispatch_pending(dpy) == -1) break;
    }
    final();
    eglMakeCurrent(d, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroySurface(d, s);
    wl_egl_window_destroy(w);
    eglTerminate(d);
    xdg_toplevel_destroy(top);
    xdg_surface_destroy(xs);
    wl_surface_destroy(sup);
    wl_display_disconnect(dpy);
    return 0;
}

/* --- Por KMS (la pantalla de la virtio-gpu) --- */

static int esperando;

static void girado(int fd, unsigned int a, unsigned int b, unsigned int c, void *dato) {
    (void)fd; (void)a; (void)b; (void)c; (void)dato;
    esperando = 0;
}

static void soltar_fb(struct gbm_bo *bo, void *dato) {
    int fd = gbm_device_get_fd(gbm_bo_get_device(bo));
    uint32_t fb = (uint32_t)(uintptr_t)dato;
    if (fb) drmModeRmFB(fd, fb);
}

static uint32_t fb_de(int fd, struct gbm_bo *bo) {
    uint32_t fb = (uint32_t)(uintptr_t)gbm_bo_get_user_data(bo);
    if (fb) return fb;
    if (drmModeAddFB(fd, gbm_bo_get_width(bo), gbm_bo_get_height(bo), 24, 32, gbm_bo_get_stride(bo),
                     gbm_bo_get_handle(bo).u32, &fb)) {
        printf("cuadros_gl: kms: drmModeAddFB falla: %s\n", strerror(errno));
        exit(1);
    }
    gbm_bo_set_user_data(bo, (void *)(uintptr_t)fb, soltar_fb);
    return fb;
}

static int por_kms(int segundos) {
    int fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        printf("cuadros_gl: kms: no hay /dev/dri/card0: %s\n", strerror(errno));
        return 1;
    }
    drmVersionPtr v = drmGetVersion(fd);
    printf("cuadros_gl: kms: controlador %s (%s)\n", v ? v->name : "?", v ? v->desc : "?");
    drmModeResPtr res = drmModeGetResources(fd);
    drmModeConnectorPtr con = NULL;
    for (int i = 0; res && i < res->count_connectors && !con; i++) {
        drmModeConnectorPtr c = drmModeGetConnector(fd, res->connectors[i]);
        if (c && c->connection == DRM_MODE_CONNECTED && c->count_modes > 0) con = c;
        else drmModeFreeConnector(c);
    }
    if (!con) {
        printf("cuadros_gl: kms: ninguna pantalla conectada\n");
        return 1;
    }
    drmModeModeInfo modo_drm = con->modes[0];
    for (int i = 0; i < con->count_modes; i++)
        if (con->modes[i].type & DRM_MODE_TYPE_PREFERRED) modo_drm = con->modes[i];
    uint32_t crtc = res->crtcs[0];
    drmModeEncoderPtr e = con->encoder_id ? drmModeGetEncoder(fd, con->encoder_id) : NULL;
    if (e && e->crtc_id) crtc = e->crtc_id;
    printf("cuadros_gl: kms: pantalla %ux%u (%s)\n", modo_drm.hdisplay, modo_drm.vdisplay, modo_drm.name);
    if (modo_drm.hdisplay != ANCHO || modo_drm.vdisplay != ALTO)
        printf("cuadros_gl: kms: aviso: la pantalla no es de %dx%d; se pinta en su esquina\n", ANCHO, ALTO);

    struct gbm_device *gbm = gbm_create_device(fd);
    struct gbm_surface *gs = gbm ? gbm_surface_create(gbm, modo_drm.hdisplay, modo_drm.vdisplay, GBM_FORMAT_XRGB8888,
                                                      GBM_BO_USE_SCANOUT | GBM_BO_USE_RENDERING) : NULL;
    if (!gs) {
        printf("cuadros_gl: kms: GBM no da superficie: %s\n", strerror(errno));
        return 1;
    }
    EGLDisplay d = eglGetPlatformDisplay(EGL_PLATFORM_GBM_KHR, gbm, NULL);
    if (d == EGL_NO_DISPLAY || !eglInitialize(d, NULL, NULL)) {
        printf("cuadros_gl: kms: EGL no arranca (%#x)\n", eglGetError());
        return 1;
    }
    EGLConfig c = elegir_config(d, GBM_FORMAT_XRGB8888);
    EGLContext x = contexto(d, c);
    EGLSurface s = eglCreatePlatformWindowSurface(d, c, gs, NULL);
    if (s == EGL_NO_SURFACE || !eglMakeCurrent(d, s, s, x)) {
        printf("cuadros_gl: kms: sin superficie de EGL (%#x)\n", eglGetError());
        return 1;
    }
    preparar_gl();
    drmEventContext ev = {.version = 2, .page_flip_handler = girado};
    struct gbm_bo *anterior = NULL;
    int voltear = 1;
    t0 = t_ultimo = ms();
    while (ms() - t0 < segundos * 1000LL) {
        pintar();
        eglSwapBuffers(d, s);
        struct gbm_bo *bo = gbm_surface_lock_front_buffer(gs);
        uint32_t fb = fb_de(fd, bo);
        if (!anterior) {
            if (drmModeSetCrtc(fd, crtc, fb, 0, 0, &con->connector_id, 1, &modo_drm)) {
                printf("cuadros_gl: kms: drmModeSetCrtc falla: %s\n", strerror(errno));
                return 1;
            }
        } else if (voltear && drmModePageFlip(fd, crtc, fb, DRM_MODE_PAGE_FLIP_EVENT, NULL) == 0) {
            /* Hasta que el cambio esté hecho no se puede soltar el búfer de antes. */
            esperando = 1;
            while (esperando) {
                struct pollfd p = {.fd = fd, .events = POLLIN};
                if (poll(&p, 1, 1000) <= 0) {
                    printf("cuadros_gl: kms: el cambio de búfer no llega\n");
                    esperando = 0;
                    voltear = 0;
                    break;
                }
                drmHandleEvent(fd, &ev);
            }
        } else {
            if (voltear) printf("cuadros_gl: kms: sin drmModePageFlip (%s): drmModeSetCrtc en cada cuadro\n", strerror(errno));
            voltear = 0;
            drmModeSetCrtc(fd, crtc, fb, 0, 0, &con->connector_id, 1, &modo_drm);
        }
        if (anterior) gbm_surface_release_buffer(gs, anterior);
        anterior = bo;
        contar();
    }
    final();
    eglMakeCurrent(d, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroySurface(d, s);
    eglTerminate(d);
    gbm_surface_destroy(gs);
    gbm_device_destroy(gbm);
    close(fd);
    return 0;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    int segundos = argc > 2 ? atoi(argv[2]) : 20;
    if (segundos <= 0) segundos = 20;
    if (argc > 1 && !strcmp(argv[1], "kms")) {
        modo = "kms";
        return por_kms(segundos);
    }
    if (argc > 1 && !strcmp(argv[1], "wayland")) {
        modo = "wayland";
        return por_wayland(segundos, argc > 3 ? atoi(argv[3]) : 1);
    }
    fprintf(stderr, "uso: cuadros_gl wayland [segundos [intervalo]] | cuadros_gl kms [segundos]\n");
    return 2;
}
