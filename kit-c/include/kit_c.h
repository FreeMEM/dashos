/* kit-c: el kit de interfaz de DashOS (Material 3) con ABI de C. Lo usan los programas de Dash (las funciones mui_*
   con ./dash --dashos); explicación en kit-c/src/lib.rs.

   Cada cosa es un número (> 0); 0, nada. Los textos van en UTF-8. Los que se devuelven valen hasta unas cuantas
   llamadas después: quien los quiera, que los copie. Un solo hilo. El montón es el del programa: malloc y free.
   Se enlaza con libkit_c.a (make -C kernel kit-c en DashOS: ../build/libkit_c.a). */
#ifndef KIT_C_H
#define KIT_C_H

#include <stdint.h>

/* La tabla de llaves: quien lanza el programa (inicio o sesiones) dice qué es cada llave con que nace. dash_llave da
   el número de la que se llama así ("ventanas", "archivos", "sesiones", "selector"...), o 0 si no la tiene. Sin
   kit-c es la llamada 56 del kernel (docs/referencia/kernel.md). */
int32_t dash_llave(const char *nombre);

/* La llave del canal "ventanas" con el compositor para un programa sin tabla de llaves. Con tabla manda
   dash_llave("ventanas"), se diga aquí lo que se diga: ya no hace falta llamarla. */
void dash_ui_usar_llave(int32_t llave);

int32_t dash_ui_app_begin(const char *titulo, const char *base);   /* 0 sin compositor */
void dash_ui_app_end(void);

int32_t dash_ui_window_begin(const char *titulo, int32_t id);
void dash_ui_window_end(void);
int32_t dash_ui_window_open(int32_t ventana);                    /* 1 abierta, 0 no */
void dash_ui_window_close(int32_t ventana);

int32_t dash_ui_group_begin(int32_t horizontal);
void dash_ui_group_end(void);
int32_t dash_ui_text(const char *texto);
int32_t dash_ui_label(const char *texto);
int32_t dash_ui_button(const char *texto);
int32_t dash_ui_string(const char *inicial, int32_t maximo);
int32_t dash_ui_checkbox(int32_t marcada);
int32_t dash_ui_cycle(const char *opciones);                      /* "Una|Dos|Tres" */

int32_t dash_ui_list(void);
void dash_ui_list_insert(int32_t lista, int32_t pos, const char *texto);   /* pos < 0: al final */
void dash_ui_list_remove(int32_t lista, int32_t pos);
void dash_ui_list_clear(int32_t lista);
int32_t dash_ui_list_count(int32_t lista);
const char *dash_ui_list_get(int32_t lista, int32_t pos);

/* Los menús de la ventana (S10): en una fila dentro de ella o, si el ajuste del sistema lo dice, en la barra de arriba;
   el programa no cambia (docs/bitacoras/menus.md). `tecla`, la de MUI ("Q"): se enseña y vale como Ctrl+Q. */
int32_t dash_ui_menu_begin(const char *titulo);
int32_t dash_ui_menu_item(const char *titulo, const char *tecla, int32_t id);
void dash_ui_menu_separator(void);
void dash_ui_menu_end(void);
/* Desde los menús en la barra (todo lo de antes sigue igual):
   - una casilla o una opción (las opciones seguidas, sin separador entre medias, son un grupo); elegirla la cambia, y
     dash_ui_get_value / dash_ui_set_value dicen y ponen si está marcada;
   - un submenú, un nivel: lo que va entre _sub_begin y _sub_end; uno dentro de otro es un error, como en el Amiga
     (_sub_begin da 0 y lo que va hasta su _sub_end no se pone);
   - dash_ui_set_enabled también sobre un menú entero o un submenú. */
int32_t dash_ui_menu_check(const char *titulo, const char *tecla, int32_t id, int32_t marcada);
int32_t dash_ui_menu_radio(const char *titulo, const char *tecla, int32_t id, int32_t marcada);
int32_t dash_ui_menu_sub_begin(const char *titulo);
void dash_ui_menu_sub_end(void);

/* clase: 0 pulsar, 1 cambiar (valor o texto), 2 cerrar la ventana, 3 doble clic en una lista */
void dash_ui_on(int32_t objeto, int32_t clase, int32_t id);
const char *dash_ui_get_text(int32_t objeto);
void dash_ui_set_text(int32_t objeto, const char *texto);
int32_t dash_ui_get_value(int32_t objeto);
void dash_ui_set_value(int32_t objeto, int32_t valor);
void dash_ui_set_enabled(int32_t objeto, int32_t activo);

/* Botones separados por "|": devuelve 1, 2... desde la izquierda y 0 el último (o al cerrar) */
int32_t dash_ui_request(int32_t ventana, const char *titulo, const char *texto, const char *botones);
int32_t dash_ui_wait_event(void);                                 /* -1: acabar */

#endif
