/* Cliente de prueba de Wayland con libwayland (G0 de docs/GRAFICOS_LOOP.md): abre una ventana de xdg-shell de
 * 200 x 150, la pinta con memoria compartida (wl_shm, por memfd) y pinta 10 cuadros, uno por cada aviso de frame.
 * Sale con 0 si todo fue bien. Es la prueba de que el compositor de DashOS entiende a libwayland tal cual.
 *
 * Pinta un fondo azul (0xff3366cc) con una banda blanca en la fila 10 * cuadro: así la captura dice cuántos cuadros
 * llegaron a la pantalla.
 *
 * Con DOBLE=1 en el entorno, alterna dos búferes del mismo pool y solo declara dañadas la banda vieja y la nueva, como
 * los programas de verdad: así se prueba que quien copie solo lo dañado no deje bandas viejas en el otro búfer. */
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
static struct wl_seat *asiento;
static int mapa_bien = -1;
static struct wl_surface *superficie;
static struct wl_buffer *buferes[2];
static uint32_t *pxs[2];
static int doble;
static int configurada, cuadro, fallo;

static void global(void *d, struct wl_registry *r, uint32_t nombre, const char *interfaz, uint32_t version) {
    (void)d;
    if (!strcmp(interfaz, "wl_compositor")) compositor = wl_registry_bind(r, nombre, &wl_compositor_interface, 4);
    else if (!strcmp(interfaz, "wl_shm")) shm = wl_registry_bind(r, nombre, &wl_shm_interface, 1);
    else if (!strcmp(interfaz, "xdg_wm_base")) wm = wl_registry_bind(r, nombre, &xdg_wm_base_interface, 1);
    else if (!strcmp(interfaz, "wl_seat")) asiento = wl_registry_bind(r, nombre, &wl_seat_interface, 4);
    (void)version;
}
static void global_quitado(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener registro = {global, global_quitado};

/* G2: el mapa de teclado de xkb tiene que llegar en formato xkb_v1 y empezar como un mapa compilado. */
static void mapa(void *d, struct wl_keyboard *k, uint32_t formato, int32_t fd, uint32_t tam) {
    (void)d; (void)k;
    char *m = tam ? mmap(NULL, tam, PROT_READ, MAP_PRIVATE, fd, 0) : MAP_FAILED;
    mapa_bien = formato == WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1 && m != MAP_FAILED && !strncmp(m, "xkb_keymap {", 12) && m[tam - 1] == 0;
    printf("teclado: mapa xkb de %u bytes: %s\n", tam, mapa_bien ? "bien" : "MAL");
    if (m != MAP_FAILED) munmap(m, tam);
    close(fd);
}
static void k_entra(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *f, struct wl_array *t) { (void)d; (void)k; (void)s; (void)f; (void)t; }
static void k_sale(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *f) { (void)d; (void)k; (void)s; (void)f; }
static void k_tecla(void *d, struct wl_keyboard *k, uint32_t s, uint32_t t, uint32_t c, uint32_t e) { (void)d; (void)k; (void)s; (void)t; (void)c; (void)e; }
static void k_mods(void *d, struct wl_keyboard *k, uint32_t s, uint32_t a, uint32_t b, uint32_t c, uint32_t g) { (void)d; (void)k; (void)s; (void)a; (void)b; (void)c; (void)g; }
static void k_repetir(void *d, struct wl_keyboard *k, int32_t r, int32_t e) { (void)d; (void)k; (void)r; (void)e; }
static const struct wl_keyboard_listener oyente_teclado = {mapa, k_entra, k_sale, k_tecla, k_mods, k_repetir};

static void ping(void *d, struct xdg_wm_base *w, uint32_t serie) { (void)d; xdg_wm_base_pong(w, serie); }
static const struct xdg_wm_base_listener oyente_wm = {ping};

static void pintar(uint32_t *px, int n) {
    for (int y = 0; y < ALTO; y++)
        for (int x = 0; x < ANCHO; x++)
            px[y * ANCHO + x] = (y / 10 == n) ? 0xffffffff : 0xff3366cc;
}

static const struct wl_callback_listener oyente_frame;
static void siguiente(void) {
    int b = doble ? cuadro % 2 : 0;
    pintar(pxs[b], cuadro);
    wl_surface_attach(superficie, buferes[b], 0, 0);
    if (doble && cuadro > 0) {
        wl_surface_damage(superficie, 0, (cuadro - 1) * 10, ANCHO, 10);
        wl_surface_damage(superficie, 0, cuadro * 10, ANCHO, 10);
    } else {
        wl_surface_damage(superficie, 0, 0, ANCHO, ALTO);
    }
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
    if (asiento) wl_keyboard_add_listener(wl_seat_get_keyboard(asiento), &oyente_teclado, NULL);
    doble = getenv("DOBLE") && !strcmp(getenv("DOBLE"), "1");
    int fd = memfd_create("dashos-prueba", MFD_CLOEXEC);
    int tam = ANCHO * ALTO * 4;
    if (fd < 0 || ftruncate(fd, 2 * tam) < 0) return 4;
    uint32_t *px = mmap(NULL, 2 * tam, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (px == MAP_FAILED) return 4;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, 2 * tam);
    for (int i = 0; i < 2; i++) {
        pxs[i] = px + i * ANCHO * ALTO;
        buferes[i] = wl_shm_pool_create_buffer(pool, i * tam, ANCHO, ALTO, ANCHO * 4, WL_SHM_FORMAT_XRGB8888);
    }
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
    if (mapa_bien != 1) return 7;
    return fallo ? 6 : 0;
}
