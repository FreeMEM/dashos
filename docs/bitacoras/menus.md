# Los menús de los programas: en la ventana o en la barra

Rama `menus`, desde `revision` (2cffd5c5). Encargo: la decisión del usuario del 2026-11-16 (S10, "Dónde van los menús
de un programa"; S36, reglas 1 y 9): de serie, una fila de menús dentro de cada ventana; como opción de todo el sistema
(Ajustes > Escritorio, la orden `Ajuste`, `ENV:Escritorio/menus`), los del programa activo en la barra de arriba, con
los títulos a la vista, clic normal para abrir y el botón derecho desde cualquier sitio, como el Amiga. En el kit
nativo y en kit-c, sin cambiar lo que ya usa Dash.

## Diseño en tres frases

El menú de una ventana es un dato (`wayland::menus`: un texto de una cosa por línea con títulos, entradas numeradas,
separadores, atajos, desactivadas, casillas, opciones y un nivel de submenú) que el kit pinta él mismo en la fila de la
ventana o da al compositor por `dashos_menus_v1`, siempre, para que cambiar de sitio sea inmediato. El compositor solo
enseña a la barra el menú de la ventana con el teclado de quien está delante (la misma regla que el título de la barra),
y lo que se elige en la barra vuelve, comprobado (generación y número), solo al programa de ese menú. El sitio lo pone
el centro de control desde `ENV:Escritorio/menus` (que vigila), el compositor lo reparte a todos, y si la barra no está
los menús vuelven a las ventanas.

## Lo que había

- Kit nativo (`kit/`): ningún menú de programa; solo el menú de los desplegables (`Ciclo`) dentro de `Interfaz`.
- kit-c: una fila de botones de texto (`TituloMenu`) que abría el menú de los desplegables; los separadores no se veían
  y la tecla de MUI se ignoraba.
- La barra (`barra.rs`): nada de menús; el título de la ventana activa por `dashos_tema_v1.titulo`.

## 1. El menú como datos (`wayland/src/menus.rs`)

```
menus 1
M\tProyecto
E\t10\tAcerca de…\tCtrl+?
-
E\t11\tSalir\tCtrl+Q
M\tVer
E\t20\tBarra de estado\t\tcx
E\t21\tIconos\t\tox
S\tOrdenar por
>E\t30\tNombre\t\tox
```

`M` menú (con `-`, desactivado entero), `E` entrada (número de 1 a 2³¹−1 sin repetir, título, atajo que se enseña,
banderas `-` desactivada, `c` casilla, `o` opción, `x` marcada), `-` separador, `S` submenú y sus cosas con `>`. Las
opciones seguidas son un grupo. Topes: 16 KB, 16 menús, 48 cosas por menú, 32 por submenú, 512 entradas, títulos de 64
bytes y atajos de 24. Lo que no vale, no vale entero (con su línea y el porqué). Se lee y se escribe igual byte a byte.
`elegir` aplica lo de las casillas y las opciones; `marcar`, sin mirar si se puede elegir; `por_atajo`, el atajo.

Vive en `wayland` (sin dependencias) porque lo leen el compositor, los dos kits y la barra; el kit depende ahora de
`wayland` (solo de este módulo y del formato).

## 2. El protocolo (`wayland/protocolos/dashos-menus-v1.xml`, compositor en `wayland/src/menus_escena.rs`)

- `dashos_menus_v1` (global 40, lo ven todos): `menu(id, superficie)` crea el `dashos_menu_v1` de una ventana suya;
  `poner_sitio(sitio)` solo el centro de control (los de `ponen_tema`); `barra(id)` solo la barra (los de `barras`, por
  el nombre que da `inicio`); evento `sitio` (al enlazar y al cambiar).
- `dashos_menu_v1`: `poner(datos)` el menú entero; `abrir()` (F10 con los menús en la barra: solo si es el que enseña
  la barra); evento `elegido(numero)`.
- `dashos_menus_barra_v1`: `elegir(generacion, numero)`; eventos `menu(generacion, datos)` y `abrir()`.

Por qué sin permiso: un programa solo da menús de sus superficies (los números de objeto son de su conexión); el
compositor solo enseña el de la ventana con el teclado de quien está delante (`ventana_con_titulo`: nunca de una sesión
de detrás ni con la sesión bloqueada); y elegir es solo de la barra, comprobado contra lo que enseña. Un programa no
tiene ninguna petición que llegue a otro. La barra, que es del sistema y común a todas las sesiones, nunca recibe más
que lo que ve quien está sentado.

Hostil (errores de protocolo): un menú que no vale o de más de 16 KB, sobre una superficie que no es suya, dos en la
misma ventana, más de 16 por programa, poner el sitio sin ser el centro, ser la barra sin serlo. Un programa de detrás
puede cambiar su menú cuanto quiera: se guarda y no llega a la barra. Cien cambios seguidos (del sitio o del menú) dejan
un solo aviso pendiente por destinatario, con lo último. Elegir con una generación vieja, una entrada desactivada o que
no existe: nada (se apunta en el registro del compositor).

El botón derecho (`menus_boton`, desde `raton`): con los menús en la barra, sobre una ventana con menú de DashOS (de
cualquier programa del kit, también la de detrás), no llega a la ventana ni la sube, ni su soltar; la barra abre los del
programa activo. Sobre una ventana sin menú de DashOS (los programas de Linux, las cajas) llega como siempre.

## 3. El componente del kit (`kit/src/menus.rs`) y la `Interfaz`

Un solo componente para la fila dentro de la ventana y para la barra: títulos (píldoras de 32, etiqueta 14 medium), lo
desplegado como el menú de Material 3 del kit (contenedor bajo, esquinas de 16, elevación 2, entradas de 44, separadores
de 17), marcas (✓ de casilla, punto de opción, aro de opción sin marcar), atajos a la derecha en el color suave, flecha
de submenú, desactivadas al 38 %, desplazamiento con la rueda y el teclado si no cabe. Ratón: clic en un título abre; con
uno abierto, pasar por otro abre ese; soltar en una entrada elige (también pulsar, arrastrar y soltar); un clic fuera
cierra y no hace nada más. Teclado: F10, flechas, Inicio, Fin, una letra, Intro, Esc; Ctrl y el atajo eligen sin abrir
("Ctrl+Q", "Ctrl+Mayús+T"). Todo en un espacio de coordenadas con `origen` para pintar en otra superficie (la barra).

`Interfaz`: `poner_menus(datos)`, `menus_fuera(bool)`, `elegido_menu()`, `elegir_menu_fuera(n)` y `abrir_fuera` (F10
con los menús en la barra). Con menús dentro, una fila de 40 arriba con una raya debajo, y lo demás baja; fuera, nada.

Muestra en el PC: `cargo run --example menus -- x.ppm` (kit).

## 4. kit-c

- La fila es la del kit (se fue `TituloMenu`). Los separadores se ven, y la tecla de MUI es Ctrl y esa tecla.
- Cada opción lleva un número propio por ventana (1, 2...) y se traduce al evento del programa: los ids de Dash pueden
  repetirse o ser 0 sin romper el formato.
- Con el compositor (`dashos_menus_v1`, si lo hay): da el menú de cada ventana al abrirla y cada vez que cambia
  (`set_enabled`, `set_text`, una marca); con el sitio en la barra quita la fila y la ventana mengua lo de la fila (si
  no la ha dimensionado el compositor); lo elegido en la barra llega como un evento más.
- ABI de C: lo de antes, igual (`dash_ui_menu_begin`, `_item`, `_separator`, `_end`). Nuevo, compatible:
  `dash_ui_menu_check`, `dash_ui_menu_radio`, `dash_ui_menu_sub_begin`, `dash_ui_menu_sub_end`;
  `dash_ui_get_value`/`set_value` sobre una casilla u opción de menú; `dash_ui_set_enabled` sobre un menú entero.

**Para pedir a la sesión de Dash** (no se tocó `~/Projects/FreeMEMLang`): que `mui_*` tenga casillas y opciones de menú
(`MUIA_Menuitem_Checkit`, `Checked`, `Exclude` en MUI) llamando a `dash_ui_menu_check`/`_radio`, submenús
(`dash_ui_menu_sub_begin`/`_end`) y leer o poner la marca (`mui_get_value`/`mui_set_value` sobre la opción). Hoy el
runtime de Dash solo declara las cuatro de antes (`compiler/codegen.py`), que siguen igual.

## 5. La barra (`usuario/src/barra_menus.rs`, enganchada en `barra.rs`)

Tras el título de la ventana (más corto, 200 como mucho, con menús), los títulos de sus menús. Un clic abre lo desplegado
en una capa propia bajo la barra (anclada arriba, abajo e izquierda, del ancho de todos sus menús con sus submenús, con
el teclado mientras se ve; lo de fuera de un menú es transparente y un clic ahí cierra). Si el teclado se va de esa capa
(un clic en una ventana), se cierra. El botón derecho sobre la barra, o el evento `abrir` del compositor, abre el
primero; con lo desplegado abierto, el derecho también elige. Lo elegido va al compositor con la generación.

## 6. El ajuste (`ENV:Escritorio/menus`, clave `sitio`: `dentro` o `barra`)

- El centro de control lo aplica al arrancar (la de fábrica, por `ENVARC:`) y con cada aviso de cambio de `ENV:` (ya
  vigilaba `ENV:Escritorio`): se lo dice al compositor con una conexión de un momento. Sin el fichero (o al salir de la
  sesión), dentro.
- Ajustes > Escritorio (antes "Panel del escritorio"): la primera fila, "Menús de los programas: en la ventana / en la
  barra de arriba"; Intro la cambia, la escribe en `ENV:`, la guarda en `ENVARC:` y la pone en el compositor. Catálogo
  del centro: textos 101 a 103, y 7 y 93 cambiados (inglés al día).
- La orden: `Ajuste menus barra` / `Ajuste menus dentro` (con `GUARDAR`, también en `ENVARC:`); otro valor, error.

## 7. El Shell (el kit nativo en un programa que se pinta solo)

Menús "Shell" (Pestaña nueva, Partir en vertical, Partir en horizontal, Cerrar el panel) y "Editar" (Copiar, Pegar,
Borrar la pantalla), con los atajos que ya tenía (el menú solo los enseña: los sigue haciendo el Shell). Copiar se
activa y desactiva en marcha según haya algo seleccionado, en la fila y en la barra. F10 abre el primero dentro, o lo
pide al compositor si están en la barra. Con la fila, la ventana se abre 40 más alta (las letras de serie caben igual).
Sus menús van con la letra de la consola (Roboto Mono), no con Roboto: las dos de Roboto eran 62 KB más en el disco de
pruebas, que va justo (con ellas, A15 "sujetar sin soltar" de `arranque-virt` se quedó sin sitio: `escribir` -11 a la
sexta ronda). Aun así el Shell crece 29 KB (488 → 517 KB) y `C:Ordenes` 4 KB.

## Pruebas

Resultados, sobre lo último (con la máquina cargada por otras pruebas):

- `cargo test` de `wayland` (129, 1 ignorada) y de `kit` (70, 1 ignorada): bien. `make prueba`: bien.
- `make arranque-virt-menus`: bien (la tercera vuelta; las dos primeras destaparon fallos de esta rama: lo desplegado
  en la barra iba en una capa transparente de arriba abajo, que el compositor sombreaba entera de gris y que se llevaba
  los clics; y un clic del guion caía en la otra ventana). Capturas en `~/recuperar/geosian-7d/menus-capturas/`.
- `make arranque-virt-2`, `arranque-virt-linux`, `arranque-virt-shell` y `arranque-virt-wayland`: bien.
- `make arranque-virt`: la primera vuelta, sin sitio en A15 (el Shell con las letras de Roboto: ver 7); con la letra de
  la consola, A15 bien, y faltó solo "[shell] ¿Sigo? Ask dijo sí", que salió partida en dos líneas por otra salida
  intercalada (no es de esta rama); la tercera vuelta, bien entera.
- No se midió rendimiento: lo que añade el compositor tras cada mensaje (`revisar_menus`) compara el menú activo con lo
  último dicho sin copiarlo; con los menús dentro, ni eso.

- PC: `cargo test` de `wayland` (formato: leer y escribir igual, casillas y opciones, hostiles de formato, topes, 3.000
  textos al azar; compositor: el sitio y el menú de la activa, el de detrás que no llega, bloqueada, elegir solo a su
  programa, generación vieja, desactivada, sin objeto de barra, cien cambios, la barra que se va, el botón derecho, y lo
  que no puede pedir un programa) y de `kit` (la fila y elegir con el ratón, casillas, opciones y submenú, el teclado,
  menús que no caben, pintar con origen, y la `Interfaz` con menús dentro y fuera).
- QEMU, `make arranque-virt`: el hostil (ser la barra o poner el sitio sin serlo, un menú mal formado, uno de más de
  16 KB, elegir sin objeto de barra; y uno de casi 16 KB que vale, en muchos trozos del canal) y el guion del Shell
  (`Ajuste menus` con un valor malo, barra, dentro, y cien cambios a mano con `Echo`).
- QEMU, `make arranque-virt-menus` (nuevo; imagen de usar con `dash-ventana`, guion `tools/guion_menus.py`): todo de
  punta a punta con capturas.

## Decisiones pendientes del usuario

1. Lo que se ve de la fila: píldoras de texto a 8 del borde, 40 de alto, raya debajo; lo desplegado como el menú del
   kit (esquinas de 16, entradas de 44, separadores de 17); la casilla sin marcar no se ve (como en el Amiga), la
   opción sin marcar es un aro.
2. Los atajos son Ctrl (y Mayús con una letra en mayúscula), no una tecla "Amiga" (Super es del compositor). La tecla
   de MUI de kit-c pasa a ser Ctrl y esa tecla, y los separadores de MUI ahora se ven.
3. El botón derecho: se abre con pulsar (y se elige con otro clic), no "mantener y soltar" del Amiga. Solo se lo quita a
   las ventanas con menú de DashOS (también a una de detrás, sin subirla: abre los del programa activo); a las de Linux
   y las cajas les sigue llegando.
4. La categoría "Panel del escritorio" de Ajustes pasa a llamarse "Escritorio" y lleva la fila de los menús arriba.
5. El nombre del ajuste: `Escritorio/menus`, clave `sitio`, valores `dentro` y `barra`; en la orden, `Ajuste menus`.
6. En la barra, el título de la ventana se corta a 200 con menús; los títulos que no caben antes del reloj no se ven.
7. Cambiar de sitio cambia el alto de las ventanas del kit (lo de la fila) si no las ha dimensionado el compositor.
8. En el Shell, los menús con su letra de ancho fijo (por el sitio en el disco de pruebas); en la barra, con Roboto.
9. Con los menús en la barra y sin sitio para sus títulos antes del reloj, el reloj va detrás de ellos o, si tampoco
   cabe, no se ve.

## Lo que queda

- El sitio es uno para todo el compositor (como el tema): con el cambio rápido de usuario (U6) vale el de quien lo puso
  último, no el de cada persona.
- La caja Amiga y los programas de Linux siguen con sus menús (S10). Workbench y el explorador no tienen menús aún.
- Con un menú abierto en la barra, un clic en una ventana lo cierra y además le llega a la ventana.
- Las teclas del submenú abierto en la barra y la rueda en un submenú que no cabe, sin probar en QEMU.
