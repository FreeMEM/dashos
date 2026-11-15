/* kit-c: el kit de interfaz de DashOS (Material 3) con ABI de C. Lo usan los programas de Dash (las funciones mui_*
   con ./dash --dashos); explicación en kit-c/src/lib.rs.

   Cada cosa es un número (> 0); 0, nada. Los textos van en UTF-8. Los que se devuelven valen hasta unas cuantas
   llamadas después: quien los quiera, que los copie. Un solo hilo. El montón es el del programa: malloc y free.
   Se enlaza con libkit_c.a (make -C kernel kit-c en DashOS: ../build/libkit_c.a). */
#ifndef KIT_C_H
#define KIT_C_H

#include <stdint.h>

/* La llave del canal "ventanas" con el compositor. De serie, la 3 si existe (lo que lanza sesiones: 1 su canal,
   2 archivos, 3 ventanas) y si no la 1 (lo que lanza inicio con "ventanas" y "archivos") */
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

int32_t dash_ui_menu_begin(const char *titulo);
int32_t dash_ui_menu_item(const char *titulo, const char *tecla, int32_t id);
void dash_ui_menu_separator(void);
void dash_ui_menu_end(void);

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
