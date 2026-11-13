/* S26: una ventana de GTK con los controles de siempre, para ver el aspecto de la caja Linux (como DashOS u original).
 * Compila con GTK 3 o con GTK 4 (`pkg-config gtk+-3.0` o `gtk4`). Dice por la salida (y por /dev/hvc0, en la caja)
 * cuándo está en pantalla, con el tema que usa, y se cierra sola a los SEGUNDOS que diga su argumento (20 si no).
 *
 *     muestra-gtk [SEGUNDOS]
 *
 * Licencia MPL-2.0, como el resto de DashOS.
 */
#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>

static void decir(const char *t)
{
    printf("%s\n", t);
    fflush(stdout);
    FILE *f = fopen("/dev/hvc0", "w");
    if (f) {
        fprintf(f, "%s\n", t);
        fclose(f);
    }
}

static gboolean salir(gpointer w)
{
    decir("muestra-gtk: adiós");
#if GTK_MAJOR_VERSION >= 4
    gtk_window_destroy(GTK_WINDOW(w));
#else
    gtk_widget_destroy(GTK_WIDGET(w));
#endif
    return G_SOURCE_REMOVE;
}

static gboolean en_pantalla(gpointer w)
{
    char *tema = NULL;
    gboolean oscuro = FALSE;
    g_object_get(gtk_settings_get_default(), "gtk-theme-name", &tema, "gtk-application-prefer-dark-theme", &oscuro, NULL);
    const char *env = getenv("GTK_THEME");
    char t[256];
    snprintf(t, sizeof t, "muestra-gtk: GTK %d en pantalla, tema %s%s%s%s", GTK_MAJOR_VERSION, env ? env : (tema ? tema : "?"),
             env ? " (por GTK_THEME)" : "", oscuro ? ", oscuro" : "", "");
    decir(t);
    g_free(tema);
    (void)w;
    return G_SOURCE_REMOVE;
}

#if GTK_MAJOR_VERSION >= 4
#define CAJA(o) gtk_box_new(o, 12)
#define METER(c, w) gtk_box_append(GTK_BOX(c), w)
#else
#define CAJA(o) gtk_box_new(o, 12)
#define METER(c, w) gtk_box_pack_start(GTK_BOX(c), w, FALSE, FALSE, 0)
#endif

static GtkWidget *boton(const char *texto, const char *clase)
{
    GtkWidget *b = gtk_button_new_with_label(texto);
    if (clase)
        gtk_style_context_add_class(gtk_widget_get_style_context(b), clase);
    return b;
}

static GtkWidget *pagina(void)
{
    GtkWidget *col = CAJA(GTK_ORIENTATION_VERTICAL);
    gtk_widget_set_margin_start(col, 18);
    gtk_widget_set_margin_end(col, 18);
    gtk_widget_set_margin_top(col, 18);
    gtk_widget_set_margin_bottom(col, 18);

    GtkWidget *botones = CAJA(GTK_ORIENTATION_HORIZONTAL);
    METER(botones, boton("Normal", NULL));
    METER(botones, boton("Aceptar", "suggested-action"));
    METER(botones, boton("Borrar", "destructive-action"));
    GtkWidget *apagado = boton("Apagado", NULL);
    gtk_widget_set_sensitive(apagado, FALSE);
    METER(botones, apagado);
    METER(col, botones);

    GtkWidget *e = gtk_entry_new();
#if GTK_MAJOR_VERSION >= 4
    gtk_editable_set_text(GTK_EDITABLE(e), "Ñandú en la caja Linux");
#else
    gtk_entry_set_text(GTK_ENTRY(e), "Ñandú en la caja Linux");
#endif
    METER(col, e);

    GtkWidget *fila = CAJA(GTK_ORIENTATION_HORIZONTAL);
    GtkWidget *c1 = gtk_check_button_new_with_label("Casilla");
    GtkWidget *c2 = gtk_check_button_new_with_label("Marcada");
#if GTK_MAJOR_VERSION >= 4
    gtk_check_button_set_active(GTK_CHECK_BUTTON(c2), TRUE);
    GtkWidget *r1 = gtk_check_button_new_with_label("Uno");
    GtkWidget *r2 = gtk_check_button_new_with_label("Dos");
    gtk_check_button_set_group(GTK_CHECK_BUTTON(r2), GTK_CHECK_BUTTON(r1));
    gtk_check_button_set_active(GTK_CHECK_BUTTON(r1), TRUE);
#else
    gtk_toggle_button_set_active(GTK_TOGGLE_BUTTON(c2), TRUE);
    GtkWidget *r1 = gtk_radio_button_new_with_label(NULL, "Uno");
    GtkWidget *r2 = gtk_radio_button_new_with_label_from_widget(GTK_RADIO_BUTTON(r1), "Dos");
#endif
    METER(fila, c1);
    METER(fila, c2);
    METER(fila, r1);
    METER(fila, r2);
    GtkWidget *s1 = gtk_switch_new();
    gtk_switch_set_active(GTK_SWITCH(s1), TRUE);
    METER(fila, s1);
    METER(fila, gtk_switch_new());
    METER(col, fila);

    GtkWidget *esc = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 100, 1);
    gtk_range_set_value(GTK_RANGE(esc), 60);
    METER(col, esc);
    GtkWidget *pb = gtk_progress_bar_new();
    gtk_progress_bar_set_fraction(GTK_PROGRESS_BAR(pb), 0.4);
    METER(col, pb);

    GtkWidget *fila2 = CAJA(GTK_ORIENTATION_HORIZONTAL);
    GtkWidget *sp = gtk_spin_button_new_with_range(0, 99, 1);
    gtk_spin_button_set_value(GTK_SPIN_BUTTON(sp), 42);
    METER(fila2, sp);
#if GTK_MAJOR_VERSION >= 4
    const char *opciones[] = {"Pitido", "Destello", "Ninguno", NULL};
    METER(fila2, gtk_drop_down_new_from_strings(opciones));
#else
    GtkWidget *combo = gtk_combo_box_text_new();
    gtk_combo_box_text_append_text(GTK_COMBO_BOX_TEXT(combo), "Pitido");
    gtk_combo_box_text_append_text(GTK_COMBO_BOX_TEXT(combo), "Destello");
    gtk_combo_box_set_active(GTK_COMBO_BOX(combo), 0);
    METER(fila2, combo);
#endif
    METER(col, fila2);

    /* Una lista con una fila elegida. */
    GtkWidget *lista = gtk_list_box_new();
    const char *filas[] = {"Documentos", "Imágenes", "Música"};
    for (int i = 0; i < 3; i++) {
        GtkWidget *l = gtk_label_new(filas[i]);
        gtk_widget_set_halign(l, GTK_ALIGN_START);
        gtk_widget_set_margin_start(l, 8);
        gtk_widget_set_margin_top(l, 6);
        gtk_widget_set_margin_bottom(l, 6);
#if GTK_MAJOR_VERSION >= 4
        gtk_list_box_append(GTK_LIST_BOX(lista), l);
#else
        gtk_container_add(GTK_CONTAINER(lista), l);
#endif
    }
    gtk_list_box_select_row(GTK_LIST_BOX(lista), gtk_list_box_get_row_at_index(GTK_LIST_BOX(lista), 1));
    METER(col, lista);
    return col;
}

static void activar(GtkApplication *app, gpointer datos)
{
    int segundos = GPOINTER_TO_INT(datos);
    GtkWidget *w = gtk_application_window_new(app);
    gtk_window_set_default_size(GTK_WINDOW(w), 560, 470);
    GtkWidget *hb = gtk_header_bar_new();
#if GTK_MAJOR_VERSION >= 4
    gtk_window_set_title(GTK_WINDOW(w), "GTK 4 en DashOS");
#else
    gtk_header_bar_set_title(GTK_HEADER_BAR(hb), "GTK 3 en DashOS");
    gtk_header_bar_set_show_close_button(GTK_HEADER_BAR(hb), TRUE);
#endif
    gtk_window_set_titlebar(GTK_WINDOW(w), hb);

    GtkWidget *nb = gtk_notebook_new();
    gtk_notebook_append_page(GTK_NOTEBOOK(nb), pagina(), gtk_label_new("Controles"));
    gtk_notebook_append_page(GTK_NOTEBOOK(nb), gtk_label_new("Otra"), gtk_label_new("Más"));
#if GTK_MAJOR_VERSION >= 4
    gtk_window_set_child(GTK_WINDOW(w), nb);
    gtk_window_present(GTK_WINDOW(w));
#else
    gtk_container_add(GTK_CONTAINER(w), nb);
    gtk_widget_show_all(w);
#endif
    g_timeout_add(500, en_pantalla, w);
    g_timeout_add_seconds(segundos, salir, w);
}

int main(int argc, char **argv)
{
    int segundos = argc > 1 ? atoi(argv[1]) : 20;
    if (segundos <= 0)
        segundos = 20;
#if GLIB_CHECK_VERSION(2, 74, 0)
    GtkApplication *app = gtk_application_new("es.dashos.MuestraGtk", G_APPLICATION_DEFAULT_FLAGS | G_APPLICATION_NON_UNIQUE);
#else
    GtkApplication *app = gtk_application_new("es.dashos.MuestraGtk", G_APPLICATION_NON_UNIQUE);
#endif
    g_signal_connect(app, "activate", G_CALLBACK(activar), GINT_TO_POINTER(segundos));
    char *sin[] = {argv[0], NULL};
    int r = g_application_run(G_APPLICATION(app), 1, sin);
    g_object_unref(app);
    return r;
}
