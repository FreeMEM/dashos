/* Cliente de prueba de Wayland con libwayland (G0 de docs/GRAFICOS_LOOP.md): abre una ventana de xdg-shell de
 * 200 x 150, la pinta con memoria compartida (wl_shm, por memfd) y pinta 10 cuadros, uno por cada aviso de frame.
 * Sale con 0 si todo fue bien. Es la prueba de que el compositor de DashOS entiende a libwayland tal cual.
 *
 * Pinta un fondo azul (0xff3366cc) con una banda blanca en la fila 10 * cuadro: así la captura dice cuántos cuadros
 * llegaron a la pantalla. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

#define ANCHO 200
#define ALTO 150
#define CUADROS 10

static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *wm;
static struct wl_surface *superficie;
static struct wl_buffer *bufer;
static uint32_t *px;
static int configurada, cuadro, fallo;

static void global(void *d, struct wl_registry *r, uint32_t nombre, const char *interfaz, uint32_t version) {
    (void)d;
    if (!strcmp(interfaz, "wl_compositor")) compositor = wl_registry_bind(r, nombre, &wl_compositor_interface, 4);
    else if (!strcmp(interfaz, "wl_shm")) shm = wl_registry_bind(r, nombre, &wl_shm_interface, 1);
    else if (!strcmp(interfaz, "xdg_wm_base")) wm = wl_registry_bind(r, nombre, &xdg_wm_base_interface, 1);
    (void)version;
}
static void global_quitado(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener registro = {global, global_quitado};

static void ping(void *d, struct xdg_wm_base *w, uint32_t serie) { (void)d; xdg_wm_base_pong(w, serie); }
static const struct xdg_wm_base_listener oyente_wm = {ping};

static void pintar(int n) {
    for (int y = 0; y < ALTO; y++)
        for (int x = 0; x < ANCHO; x++)
            px[y * ANCHO + x] = (y / 10 == n) ? 0xffffffff : 0xff3366cc;
}

static const struct wl_callback_listener oyente_frame;
static void siguiente(void) {
    pintar(cuadro);
    wl_surface_attach(superficie, bufer, 0, 0);
    wl_surface_damage(superficie, 0, 0, ANCHO, ALTO);
    struct wl_callback *cb = wl_surface_frame(superficie);
    wl_callback_add_listener(cb, &oyente_frame, NULL);
    wl_surface_commit(superficie);
}
static void hecho(void *d, struct wl_callback *cb, uint32_t t) {
    (void)d; (void)t;
    wl_callback_destroy(cb);
    if (++cuadro < CUADROS) siguiente();
}
static const struct wl_callback_listener oyente_frame = {hecho};

static void configurar(void *d, struct xdg_surface *s, uint32_t serie) {
    (void)d;
    xdg_surface_ack_configure(s, serie);
    if (!configurada) {
        configurada = 1;
        siguiente();
    }
}
static const struct xdg_surface_listener oyente_xdg = {configurar};

static void top_configurar(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *e) {
    (void)d; (void)t; (void)w; (void)h; (void)e;
}
static void top_cerrar(void *d, struct xdg_toplevel *t) { (void)d; (void)t; fallo = 1; }
static const struct xdg_toplevel_listener oyente_top = {top_configurar, top_cerrar};

int main(void) {
    struct wl_display *pantalla = wl_display_connect(NULL);
    if (!pantalla) {
        fprintf(stderr, "cliente: sin compositor\n");
        return 2;
    }
    struct wl_registry *r = wl_display_get_registry(pantalla);
    wl_registry_add_listener(r, &registro, NULL);
    wl_display_roundtrip(pantalla);
    if (!compositor || !shm || !wm) {
        fprintf(stderr, "cliente: faltan globales\n");
        return 3;
    }
    xdg_wm_base_add_listener(wm, &oyente_wm, NULL);
    int fd = memfd_create("dashos-prueba", MFD_CLOEXEC);
    int tam = ANCHO * ALTO * 4;
    if (fd < 0 || ftruncate(fd, tam) < 0) return 4;
    px = mmap(NULL, tam, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (px == MAP_FAILED) return 4;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, tam);
    bufer = wl_shm_pool_create_buffer(pool, 0, ANCHO, ALTO, ANCHO * 4, WL_SHM_FORMAT_XRGB8888);
    close(fd);
    superficie = wl_compositor_create_surface(compositor);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm, superficie);
    xdg_surface_add_listener(xs, &oyente_xdg, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(top, &oyente_top, NULL);
    xdg_toplevel_set_title(top, "Prueba de libwayland");
    wl_surface_commit(superficie);
    while (cuadro < CUADROS && !fallo) {
        if (wl_display_dispatch(pantalla) < 0) {
            fprintf(stderr, "cliente: error de protocolo o conexión cerrada\n");
            return 5;
        }
    }
    printf("cliente: %d cuadros\n", cuadro);
    wl_display_disconnect(pantalla);
    return fallo ? 6 : 0;
}
