/* Un programa gráfico de Linux para la caja Linux de DashOS (H9): un cliente de Wayland con libwayland y wl_shm que
 * abre una ventana de xdg-shell de 320 x 240 y la pinta sin parar, un cuadro por cada aviso de frame. Corre dentro de
 * Arch Linux ARM; su ventana sale en el escritorio de DashOS por el puente (puente/, vsock y el compositor).
 *
 * Lo que pinta: un degradado que cambia de color con cada tecla, una banda blanca que baja un poco en cada cuadro y un
 * cuadrado amarillo donde se hizo el último clic. La ventana entera cambia en cada cuadro (el peor caso para la copia
 * del puente).
 *
 * Lo que dice por la salida (una línea por cosa, para las pruebas): que está en pantalla, cada tecla y cada clic, y
 * cada 5 s y al acabar cuántos cuadros por segundo lleva. Acaba con Esc o con q.
 *
 * Se compila en el PC para aarch64 (tools/caja_linux_invitado.py) con el libwayland-client del paquete de Arch Linux
 * ARM. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

#define ANCHO 320
#define ALTO 240

static struct wl_compositor *compositor;
static struct wl_shm *shm;
static struct xdg_wm_base *wm;
static struct wl_seat *asiento;
static struct wl_surface *superficie;
static struct wl_buffer *buferes[2];
static uint32_t *pxs[2];
static int configurada, acabar, cuadro, teclas, clics;
static uint32_t tono = 0x3366cc;
static int raton_x = -1, raton_y = -1, clic_x = -1, clic_y = -1;
static long long t0, t_ultimo;
static int cuadros_ultimo;

static long long ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000LL + t.tv_nsec / 1000000;
}

static void global(void *d, struct wl_registry *r, uint32_t nombre, const char *interfaz, uint32_t version) {
    (void)d; (void)version;
    if (!strcmp(interfaz, "wl_compositor")) compositor = wl_registry_bind(r, nombre, &wl_compositor_interface, 4);
    else if (!strcmp(interfaz, "wl_shm")) shm = wl_registry_bind(r, nombre, &wl_shm_interface, 1);
    else if (!strcmp(interfaz, "xdg_wm_base")) wm = wl_registry_bind(r, nombre, &xdg_wm_base_interface, 1);
    else if (!strcmp(interfaz, "wl_seat")) asiento = wl_registry_bind(r, nombre, &wl_seat_interface, 5);
}
static void global_quitado(void *d, struct wl_registry *r, uint32_t n) { (void)d; (void)r; (void)n; }
static const struct wl_registry_listener registro = {global, global_quitado};

static void mapa(void *d, struct wl_keyboard *k, uint32_t formato, int32_t fd, uint32_t tam) {
    (void)d; (void)k;
    printf("ventana_wayland: mapa de teclado %s de %u bytes\n", formato == WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1 ? "xkb" : "?", tam);
    close(fd);
}
static void k_entra(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *f, struct wl_array *t) {
    (void)d; (void)k; (void)s; (void)f; (void)t;
    printf("ventana_wayland: tengo el teclado\n");
}
static void k_sale(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *f) { (void)d; (void)k; (void)s; (void)f; }
static void k_tecla(void *d, struct wl_keyboard *k, uint32_t s, uint32_t t, uint32_t codigo, uint32_t estado) {
    (void)d; (void)k; (void)s; (void)t;
    if (estado != WL_KEYBOARD_KEY_STATE_PRESSED) return;
    teclas++;
    printf("ventana_wayland: tecla %u pulsada\n", codigo);
    /* Esc (1) o q (16): fuera. Cualquier otra cambia el color. */
    if (codigo == 1 || codigo == 16) acabar = 1;
    tono = (tono * 2654435761u + codigo * 40503u) & 0xffffff;
}
static void k_mods(void *d, struct wl_keyboard *k, uint32_t s, uint32_t a, uint32_t b, uint32_t c, uint32_t g) { (void)d; (void)k; (void)s; (void)a; (void)b; (void)c; (void)g; }
static void k_repetir(void *d, struct wl_keyboard *k, int32_t r, int32_t e) { (void)d; (void)k; (void)r; (void)e; }
static const struct wl_keyboard_listener oyente_teclado = {mapa, k_entra, k_sale, k_tecla, k_mods, k_repetir};

static void p_entra(void *d, struct wl_pointer *p, uint32_t s, struct wl_surface *f, wl_fixed_t x, wl_fixed_t y) {
    (void)d; (void)p; (void)s; (void)f;
    raton_x = wl_fixed_to_int(x); raton_y = wl_fixed_to_int(y);
}
static void p_sale(void *d, struct wl_pointer *p, uint32_t s, struct wl_surface *f) { (void)d; (void)p; (void)s; (void)f; }
static void p_mueve(void *d, struct wl_pointer *p, uint32_t t, wl_fixed_t x, wl_fixed_t y) {
    (void)d; (void)p; (void)t;
    raton_x = wl_fixed_to_int(x); raton_y = wl_fixed_to_int(y);
}
static void p_boton(void *d, struct wl_pointer *p, uint32_t s, uint32_t t, uint32_t boton, uint32_t estado) {
    (void)d; (void)p; (void)s; (void)t;
    if (estado != WL_POINTER_BUTTON_STATE_PRESSED) return;
    clics++;
    clic_x = raton_x; clic_y = raton_y;
    printf("ventana_wayland: clic del botón %u en (%d, %d)\n", boton, clic_x, clic_y);
}
static void p_eje(void *d, struct wl_pointer *p, uint32_t t, uint32_t e, wl_fixed_t v) { (void)d; (void)p; (void)t; (void)e; (void)v; }
static void p_marco(void *d, struct wl_pointer *p) { (void)d; (void)p; }
static void p_fuente(void *d, struct wl_pointer *p, uint32_t f) { (void)d; (void)p; (void)f; }
static void p_para(void *d, struct wl_pointer *p, uint32_t t, uint32_t e) { (void)d; (void)p; (void)t; (void)e; }
static void p_discreto(void *d, struct wl_pointer *p, uint32_t e, int32_t v) { (void)d; (void)p; (void)e; (void)v; }
static const struct wl_pointer_listener oyente_raton = {p_entra, p_sale, p_mueve, p_boton, p_eje, p_marco, p_fuente, p_para, p_discreto};

static void capacidades(void *d, struct wl_seat *s, uint32_t c) {
    (void)d;
    static int hecho;
    if (hecho) return;
    hecho = 1;
    if (c & WL_SEAT_CAPABILITY_KEYBOARD) wl_keyboard_add_listener(wl_seat_get_keyboard(s), &oyente_teclado, NULL);
    if (c & WL_SEAT_CAPABILITY_POINTER) wl_pointer_add_listener(wl_seat_get_pointer(s), &oyente_raton, NULL);
}
static void nombre_asiento(void *d, struct wl_seat *s, const char *n) { (void)d; (void)s; (void)n; }
static const struct wl_seat_listener oyente_asiento = {capacidades, nombre_asiento};

static void ping(void *d, struct xdg_wm_base *w, uint32_t serie) { (void)d; xdg_wm_base_pong(w, serie); }
static const struct xdg_wm_base_listener oyente_wm = {ping};

static void pintar(uint32_t *px, int n) {
    uint32_t r0 = tono >> 16 & 0xff, g0 = tono >> 8 & 0xff, b0 = tono & 0xff;
    int banda = (n * 2) % ALTO;
    for (int y = 0; y < ALTO; y++) {
        uint32_t *fila = px + y * ANCHO;
        if (y >= banda && y < banda + 8) {
            for (int x = 0; x < ANCHO; x++) fila[x] = 0xffffffff;
            continue;
        }
        for (int x = 0; x < ANCHO; x++) {
            uint32_t r = (r0 * (ANCHO - x) + 0x20 * x) / ANCHO, g = (g0 * (ALTO - y) + 0x80 * y) / ALTO, b = b0;
            fila[x] = 0xff000000 | r << 16 | g << 8 | b;
        }
    }
    if (clic_x >= 0)
        for (int y = clic_y - 10; y < clic_y + 10; y++)
            for (int x = clic_x - 10; x < clic_x + 10; x++)
                if (x >= 0 && y >= 0 && x < ANCHO && y < ALTO) px[y * ANCHO + x] = 0xffffcc00;
}

static void informe(const char *cuando) {
    long long ahora = ms();
    long long dt = ahora - t_ultimo;
    int dc = cuadro - cuadros_ultimo;
    printf("ventana_wayland: %s: %d cuadros en %lld ms, %d.%d por segundo (desde el principio: %d en %lld ms)\n", cuando, dc, dt,
           dt ? (int)(dc * 1000 / dt) : 0, dt ? (int)(dc * 10000 / dt % 10) : 0, cuadro, ahora - t0);
    t_ultimo = ahora;
    cuadros_ultimo = cuadro;
}

static const struct wl_callback_listener oyente_frame;
static void siguiente(void) {
    int b = cuadro % 2;
    pintar(pxs[b], cuadro);
    wl_surface_attach(superficie, buferes[b], 0, 0);
    wl_surface_damage(superficie, 0, 0, ANCHO, ALTO);
    wl_callback_add_listener(wl_surface_frame(superficie), &oyente_frame, NULL);
    wl_surface_commit(superficie);
}
static void hecho(void *d, struct wl_callback *cb, uint32_t t) {
    (void)d; (void)t;
    wl_callback_destroy(cb);
    if (cuadro == 0) {
        printf("ventana_wayland: primer cuadro en pantalla a los %lld ms\n", ms() - t0);
        t_ultimo = ms();
    }
    cuadro++;
    if (ms() - t_ultimo >= 5000) informe("en marcha");
    if (!acabar) siguiente();
}
static const struct wl_callback_listener oyente_frame = {hecho};

static void sup_configurar(void *d, struct xdg_surface *s, uint32_t serie) {
    (void)d;
    xdg_surface_ack_configure(s, serie);
    if (!configurada) {
        configurada = 1;
        printf("ventana_wayland: configurada\n");
        siguiente();
    }
}
static const struct xdg_surface_listener oyente_sup = {sup_configurar};
static void top_configurar(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *e) { (void)d; (void)t; (void)w; (void)h; (void)e; }
static void top_cerrar(void *d, struct xdg_toplevel *t) {
    (void)d; (void)t;
    printf("ventana_wayland: el compositor pide cerrar\n");
    acabar = 1;
}
static void top_limites(void *d, struct xdg_toplevel *t, int32_t w, int32_t h) { (void)d; (void)t; (void)w; (void)h; }
static void top_capacidades(void *d, struct xdg_toplevel *t, struct wl_array *c) { (void)d; (void)t; (void)c; }
static const struct xdg_toplevel_listener oyente_top = {top_configurar, top_cerrar, top_limites, top_capacidades};

int main(void) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    t0 = ms();
    struct wl_display *pantalla = wl_display_connect(NULL);
    if (!pantalla) {
        printf("ventana_wayland: no hay compositor de Wayland (WAYLAND_DISPLAY=%s)\n", getenv("WAYLAND_DISPLAY") ? getenv("WAYLAND_DISPLAY") : "");
        return 1;
    }
    printf("ventana_wayland: conectada a %s\n", getenv("WAYLAND_DISPLAY") ? getenv("WAYLAND_DISPLAY") : "wayland-0");
    struct wl_registry *r = wl_display_get_registry(pantalla);
    wl_registry_add_listener(r, &registro, NULL);
    wl_display_roundtrip(pantalla);
    if (!compositor || !shm || !wm) {
        printf("ventana_wayland: faltan globales (compositor %p, shm %p, xdg_wm_base %p)\n", (void *)compositor, (void *)shm, (void *)wm);
        return 1;
    }
    xdg_wm_base_add_listener(wm, &oyente_wm, NULL);
    if (asiento) wl_seat_add_listener(asiento, &oyente_asiento, NULL);
    int tam = ANCHO * ALTO * 4;
    int fd = memfd_create("ventana_wayland", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, 2 * tam) < 0) return 1;
    uint32_t *px = mmap(NULL, 2 * tam, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (px == MAP_FAILED) return 1;
    struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, 2 * tam);
    for (int i = 0; i < 2; i++) {
        buferes[i] = wl_shm_pool_create_buffer(pool, i * tam, ANCHO, ALTO, ANCHO * 4, WL_SHM_FORMAT_XRGB8888);
        pxs[i] = px + i * ANCHO * ALTO;
    }
    superficie = wl_compositor_create_surface(compositor);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm, superficie);
    xdg_surface_add_listener(xs, &oyente_sup, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(top, &oyente_top, NULL);
    xdg_toplevel_set_title(top, "Hola desde Linux");
    xdg_toplevel_set_app_id(top, "ventana_wayland");
    wl_surface_commit(superficie);
    while (!acabar && wl_display_dispatch(pantalla) != -1) {
    }
    informe("al acabar");
    printf("ventana_wayland: %d teclas y %d clics; adiós\n", teclas, clics);
    wl_display_disconnect(pantalla);
    return 0;
}
