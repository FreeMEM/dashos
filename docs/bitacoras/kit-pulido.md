# Bitácora: el kit, más cerca de Material 3 (rama kit-pulido)

El usuario, de las capturas: "los botones y ventanas que estoy viendo en las capturas que se supone que son material se
ven horribles". Sigue a `kit-material.md`. Medidas y escalas sacadas de la especificación de Material 3
(m3.material.io: escala de tipos, diálogos, casillas, cajón de navegación); ningún código de otros proyectos.

## 1. Antes

Capturas de `make arranque-virt` sin tocar nada (las cinco BIEN), y del muestrario y la vista del kit en el PC. Lo que no
cuadraba con Material 3:

- La pregunta de permiso de `guardian` (U4): lo peor. Una caja de esquinas rectas con un borde de 2 en el primario,
  todo centrado ("Permiso" a 26, la frase a 20, "S: sí    N: no" a 18). No se parece a ningún diálogo de Material 3.
- Ajustes: el deslizador de 2024 (barra gruesa, agarre de raya, punto al final) en una fila con una casilla sin texto
  al lado. Sin rótulo, no se entiende qué es ni qué hace la casilla. Márgenes de 16 (los diálogos de Material 3 llevan
  24) y todo a 8: el título pegado a lo demás, los botones pegados al desplegable.
- Tipografía: los títulos (Ajustes, el centro, el selector, el Workbench, el panel rápido) a 22 en medium. En Material 3
  el título de un diálogo es headline small (24, regular) y title large es 22 regular; medium es solo para title
  medium/small y las etiquetas. El texto medía su alto con la fuente (19 a 16), no con la línea de la escala (24).
- Centro de control: la lista de categorías era una lista más (contenedor con la fila elegida en un rectángulo de
  esquinas de 12). Hace de navegación: en Material 3, el cajón de navegación (sin contenedor, etiquetas de 14 medium,
  la elegida en una píldora).
- Lo demás (botones, campo, desplegable, menú, barra, avisos, panel) ya seguía la especificación tras `kit-material`.

## 2. Cambios

Kit (`kit/src/controles.rs`):

- Escala de tipos: `Texto` con `ESTILO` 1 es headline small (24, regular; antes 22 medium); nuevo `ESTILO` 3, title
  small (14 medium, color suave) para rótulos de grupo. `alto_de_linea`: el texto mide la línea de Material 3 (24/32,
  22/28, 16/24, 14/20, 12/16).
- `Interfaz::margen` (16 de serie; 24 en los diálogos) y `Hijo::Espacio(n)`: un hueco fijo, para los 16 bajo el título y
  los 24 antes de los botones. El margen de serie no cambia: el Workbench y las pruebas del kit siguen igual.
- Casilla con `ETIQUETA`: el texto (16) detrás de la casilla, y toda la fila se pulsa (en Material 3 la etiqueta es parte
  del control). Sin etiqueta, como antes.
- Lista con `ESTILO` 1, la de navegación: sin contenedor, texto de etiqueta (14 medium) en el color suave, la elegida en
  una píldora (radio 28) del contenedor secundario.
- Pruebas nuevas: `margen_espacios_y_casilla_con_etiqueta`, `lista_de_navegacion`. `una_ventana_que_responde`, al día
  (el texto mide 24 de alto).

Programas:

- `guardian`: la pregunta es un diálogo de Material 3: contenedor alto, esquinas de 28 (fuera, transparente), 440 × 208,
  24 de relleno, título "¿Dar permiso a "x"?" (24; baja a 22 o 20 si el nombre no cabe),
  el texto (14, suave) y abajo a la derecha las respuestas con su tecla dibujada ("S Sí", "N No"). No son botones: no se
  pulsan, `guardian` solo atiende al teclado. Primero llevaba su propia sombra dentro del búfer, con 16 de margen; el
  compositor ya echa sombra a todas las capas, y se veía la suya, rectangular, alrededor del margen: fuera la propia.
- Ajustes: como un diálogo (margen 24, 16 bajo el título, 24 antes de los botones), el deslizador solo en su fila bajo
  "Volumen: 60", la casilla con su texto ("Desactivar Aceptar") en otra. 360 × 408 (antes 320): `arranque.txt`,
  `arranque_virt.txt`, `arranque_virt_2.txt` y `guion_multimonitor.py` dicen el tamaño nuevo. El clic del guion en
  (296, 26) sigue cayendo en el título.
- Centro: la lista de categorías es la de navegación; margen 24.
- Selector: margen 24; 16 más de alto (`56 × FILAS + 164`).
- Panel rápido: el título a 22 regular (title large).
- `kit/examples/vista.rs` y `muestrario.rs`, al día (casilla con etiqueta y lista de navegación en el muestrario).

## 3. Capturas

En `capturas-agente/` (fuera de git):

- Antes: `antes-*.png` (la tanda entera de `make arranque-virt`), `antes-muestrario.png`, `antes-vista.png`.
- Después: `despues-*.png` y `despues-mosaico.png`, `despues-muestrario.png`, `despues-vista.png`,
  `despues-vista-oscuro.png`.
- Comparadas lado a lado: `comp-pregunta.png` (la pregunta de permiso), `comp-vista.png` (Ajustes en el PC: antes,
  después, después en oscuro y azul), `comp-centro-menu.png` (el centro y el menú de Ajustes), `comp-selector.png`.

No cambia nada caro de pintar: la pregunta pinta menos que antes (sin sombra propia) y lo demás son medidas y colores.

## 4. Pruebas

- `cd kit && cargo test`: 64, bien (dos nuevas). `cd wayland && cargo test --lib`: 59, bien.
- `make prueba` (raspi): BIEN.
- `make arranque-virt`: las cinco BIEN, dos veces (la segunda, con la pregunta ya sin sombra propia).
  `tools/captura_ventanas.py` no hizo falta tocarlo: mira la superficie de Ajustes en (450, 270), que cae en su margen,
  y los píxeles del primario hasta y = 424, donde ahora llegan el deslizador y parte de lo demás (3576 en la última captura; pide 2000).

## Pendiente o por decidir

- El deslizador sigue siendo el de Material 3 de 2024 (carril de 16, agarre de raya, parada). Es lo que dice la
  especificación de ahora; si al usuario le parece raro, se puede volver al de 2022 (carril de 4 y bola de 20), que es
  el que más gente reconoce. Decide el usuario.
- Sin espaciado entre letras (Material 3 lo pide: 0,5 en body large, 0,1 en las etiquetas): `texto.rs` no lo tiene.
- La sombra del compositor es de radio 16; las capas de radio 28 (panel, avisos, la pregunta) dejan ver un poco de
  sombra en las esquinas por fuera de la curva. Se notaría poco; arreglarlo es dar el radio de cada capa al compositor.
- El Workbench sigue con margen 16 (son ventanas de ficheros, no diálogos) y el centro sigue con 420 de alto aunque le
  sobre.
- La lista no marca la fila que tiene el ratón encima (Material 3: capa del 8 %), ni la de navegación.
- La pantalla de bloqueo no se ha tocado: su campo de contraseña es una píldora llena, que Material 3 no tiene (sería
  un campo con contorno).
- Solo QEMU; sin probar en la Pi ni a 1920x1080.
