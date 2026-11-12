# Bitácora: menús, foco en la lista y etiquetas en el kit (rama kit-menus)

Sigue lo pendiente de `docs/bitacoras/kit-material.md`. Valores de los tokens públicos de Material 3 (los ficheros
de tokens del repositorio de Material Web: `md-comp-menus` de 2024, `md-comp-outlined-text-field`,
`md-comp-outlined-select`, `md-comp-list`, `md-comp-focus-ring`). Solo los valores; nada de su código.

## 1. Antes

- `cd kit && cargo test`: 40, bien.
- Capturas en `capturas-agente/` (fuera de git): `muestrario-antes.png` y `vista-antes.png`. Lo que se veía mal:
  - La lista no estaba en el muestrario; con Tab se llegaba a ella sin ver qué fila tenía el foco.
  - El desplegable solo pasaba de una opción a la siguiente con cada clic.
  - El campo no tenía etiqueta, ni ayuda, ni error.
  - El anillo de foco, del primario: Material 3 lo pide del secundario (`md.comp.focus-ring.color`).

## 2. El tema: secundario, terciario y error

- Cuatro papeles nuevos en `Tema`: `secundario` (el anillo de foco), `contenedor_terciario` y
  `sobre_contenedor_terciario` (la opción elegida de un menú, como pide el menú de 2024) y `error`.
- `error` es igual con cualquier acento (en Material 3 la paleta de error es fija): va solo en `CLARO` y `OSCURO`.
- Los otros tres salen de las paletas secundaria y terciaria en `desde_color`. Para los acentos del centro, una tabla
  aparte, `TABLA_OTROS`, con su función. No toqué `TABLA` para no chocar con `fondo-c3`, que trabaja en el mismo
  fichero. Una prueba que se salta (`imprimir_tabla_otros`) la vuelve a sacar si cambia el cálculo.
- `tabla_igual_que_el_calculo` y `la_referencia_sale_del_violeta` miran ya los papeles nuevos, y la prueba de
  contraste mira también el texto sobre el contenedor terciario.
- Lo que no hice: el contenedor secundario sigue siendo la mezcla de la superficie con el primario (como lo dejó
  `kit-material`), no el tono de la paleta secundaria. Cambiarlo cambia el aspecto de toda la interfaz (botones
  tonales, deslizador, lista): mejor decidirlo aparte, mirando capturas.

## 3. El teclado

- `Tecla::Escape` (código 1) y `Tecla::AltAbajo` y `Tecla::AltArriba` (Alt de la izquierda, código 56, con las
  flechas). AltGr (código 100) no cuenta como Alt.
- `guardian` lee Esc por su cuenta (el comentario dice que el kit no lo da). No lo toqué: no es de esta rama.

## 4. La lista: la fila actual y la elegida

- La fila actual (la del foco del teclado) es la elegida o, si no hay elegida, la primera que se ve. La elección
  sigue al foco, como antes: los programas no cambian.
- Con el foco del teclado (el anillo de `kit-material`), la fila actual lleva la capa de foco (10 % de su texto) y el
  anillo del secundario por dentro (3 de grosor, esquinas de 12). La elegida sigue en el contenedor secundario. Así se
  distingue una fila actual sin elegir (Tab a una lista sin nada elegido) de una elegida.
- El espacio elige la actual; sin elegida, las flechas también eligen la actual.
- No hay capa por fila al pasar el ratón: `Estado` no sabe dónde está el puntero dentro del objeto. Queda pendiente.

## 5. El desplegable y su menú

- `atr::ABIERTO`. Un clic, Intro, el espacio o Alt+abajo lo ponen a sí; la `Interfaz` lo ve y abre el menú. Un
  programa también puede abrirlo o cerrarlo con ese atributo. Cerrado y con el foco, las flechas siguen pasando de
  una opción a otra sin abrirlo.
- El menú lo pinta la `Interfaz` encima de todo, en la misma superficie (el kit no tiene subsuperficies). Material 3,
  menú de 2024: contenedor bajo de superficie, esquinas de 16, elevación 2 (dos sombras: 1 abajo y 2 de difuminado al
  30 %, y 2 abajo, 2 más ancha y 6 de difuminado al 15 %), 2 de relleno, opciones de 44 con 16 a los lados y texto de
  etiqueta (14, medium). La elegida, en el contenedor terciario con esquinas de 12; las demás con esquinas de 4, y la
  primera y la última, de 12 por fuera. Encima con el ratón, capa del 8 %; la actual con el teclado, capa del 10 % y
  anillo del secundario por dentro.
- Va debajo del desplegable, 4 más abajo, de su ancho. Si no cabe debajo, encima (con 8 más de hueco si tiene
  etiqueta, para no taparla). Si no cabe en ninguno de los dos, donde haya más sitio, con las opciones que quepan; se
  desplaza con la actual. No sale de la ventana (con 8 de margen).
- Abierto: flechas, Inicio y Fin mueven la actual; una letra salta a la siguiente opción que empieza por ella; Intro,
  el espacio o Alt+arriba eligen; Esc cierra sin cambiar; Tab cierra sin cambiar y pasa el foco. Con el ratón: la
  opción que tiene encima pasa a ser la actual; un clic en ella la elige; un clic fuera cierra sin cambiar y no pulsa
  lo que haya debajo (ni arrastrando hasta otro control antes de soltar).
- Abierto, el desplegable tiene el contorno del foco y la flecha mira arriba.
- Lienzo: `redondeado_esquinas` (un radio por esquina) y `sombra` (sin coma flotante: raíz entera en octavos de
  píxel; solo pinta fuera del rectángulo).
- `Interfaz::menu_abierto` y `Interfaz::sitio_opcion_menu`, para programas y pruebas.

## 6. El campo: etiqueta, ayuda y error

- `atr::ETIQUETA`: dentro (16, color suave) mientras está vacío y sin foco; si no, arriba (12), centrada sobre la
  línea de arriba, que se corta 4 antes y 4 después. Colores de Material 3: suave, del texto encima, primario con el
  foco, error con error.
- `atr::AYUDA` y `atr::ERROR`: una línea de 12 debajo (4 de hueco, 16 de relleno). Si el campo tiene cualquiera de
  los dos atributos, aunque vacío, reserva esa línea (mide 76 en vez de 56): así no salta lo de abajo al salir un
  error. Con un error, el contorno, la etiqueta, el cursor y la línea van en el color de error, y la ayuda no se ve.
- El contorno del campo encima pasa al color del texto (antes no cambiaba).
- El desplegable acepta también `ETIQUETA` (siempre arriba: siempre tiene una opción).

## 7. Pruebas del kit

- Nuevas: `el_menu_de_un_desplegable`, `el_menu_cabe_en_la_ventana`, `la_fila_actual_y_la_elegida`,
  `un_campo_con_etiqueta_y_error`, `esquinas_distintas_y_sombra`; cambiadas: `un_ciclo` (un clic ya no cambia la
  opción), `una_lista` (capa y anillo de la actual), `casilla_y_deslizador` (anillo del secundario),
  `un_campo_de_texto` (el contorno encima) y `teclas_que_no_son_letras` (Esc y Alt).
- `cd kit && cargo test`: 45 bien, 1 saltada (la que imprime la tabla).

## 8. Ajustes

- Ajustes (la ventana de prueba del kit) gana el campo con la etiqueta "Nombre" y un desplegable "Aviso" (Pitido,
  Destello, Ninguno). Pasa de 360 × 250 a 360 × 320 (lo mínimo es 312). `kit/examples/vista.rs` la dibuja igual; con
  `menu` detrás, con el menú abierto.
- Ajustes dice dónde está el desplegable y, al abrir el menú (las tres primeras veces), dónde cae la segunda opción.
- Guion (`tools/prueba_uefi.sh`): cuando el campo ya tiene su "a", y sin el centro de control encima, el cursor va
  al desplegable, un clic abre el menú (con `CAPTURA_MENU=fichero.ppm`, se guarda la pantalla), otro clic elige la
  segunda opción y el cursor vuelve a (300, 300). Líneas esperadas nuevas: "menú abierto" y "el desplegable dice:
  Destello". `captura_ventanas.py` cuenta el primario en los 320 de alto.
- Desde (300, 300), los clics de W8 caen ahora en el desplegable: abren y cierran su menú, y alguno elige una
  opción. No rompe nada (antes caían en Aceptar).
- Centro, selector: no los toqué. La Apariencia del centro ganaría con dos desplegables (modo y color) en vez de su
  lista, pero cambia su guion; queda propuesto.

## Pendiente

- El menú no sale de la ventana: para eso hace falta una subsuperficie o un `xdg_popup` (el compositor ya tiene
  subsuperficies, G3; el kit no). En una ventana pequeña, el menú enseña menos opciones y se desplaza.
- Sin rueda del ratón en el menú ni en la lista (la `Interfaz` no recibe la rueda).
- Sin capa por fila al pasar el ratón por la lista.
- El contenedor secundario de verdad (paleta secundaria), a decidir con capturas.
- La Apariencia del centro con desplegables.
- Sin probar en la Pi de verdad: solo QEMU.
