# Bitácora: Material 3 en el resto del kit (rama kit-material)

Sigue el repaso de Material 3 de `docs/REVISION_LOOP.md` (botones, lista, ventanas). Aquí, los controles que
faltaban y la ventana de prueba Ajustes. Valores sacados de los tokens de Material 3 (los de Compose: `SliderTokens`,
`CheckboxTokens`) y de la página de cada componente.

## 1. Antes

- `make arranque-virt` sin tocar nada: MAL por `[ficheros] A15 sujetar sin soltar` (35 rondas, soltar
  -2147483648), con el PC a carga 53. Es la prueba que ya fallaba por tiempo con el PC cargado. Las capturas del
  centro sí salieron; la de Ajustes (`target/captura.ppm`) no.
- Para ver los controles sin QEMU: `kit/examples/muestrario.rs` (nuevo). Dibuja cada control en cada estado
  (normal, encima, foco, pulsado, desactivado), en claro y en oscuro. Y Ajustes de antes, con el kit de `HEAD`, en
  el PC.
- Lo que se veía mal:
  - Deslizador: el de Material 3 de 2022 (carril de 4, bola de 20). El de ahora es otro.
  - Casilla: sin anillo de foco ni teclado; la marca, a escalones (cuadros de 2 × 2); el contorno no cambia encima.
  - Ciclo: una píldora con contorno y la etiqueta en el primario. Material 3 no tiene eso: el desplegable es un campo
    con contorno y una flecha.
  - Ajustes: botones rellenos los dos, centrados; sin título.

## 2. Los controles

- Deslizador (el de Material 3 de 2024): carril de 16, redondo por fuera; el tramo activo en el primario, el otro en
  el contenedor secundario; 6 de hueco a cada lado del agarre, que es una barra de 4 × 44 (de 2 al pulsar o con el
  foco). Junto al agarre, los tramos tienen esquinas de 2. Al final, un punto de 4 (la parada). Desactivado: activo y
  agarre al 38 %, inactivo al 12 %, del color del texto. Mide 44 de alto (antes 40).
  Teclado: flechas de 1 en 1, Inicio y Fin a los extremos. Anillo de foco alrededor del agarre.
- Casilla: la marca con trazos suavizados (`Lienzo::trazo`, nuevo, en octavos de píxel). El contorno pasa al color
  del texto encima, con el foco o pulsada. La capa: del primario sobre una marcada o al pulsar una sin marcar.
  Anillo de foco como el del botón (3 de grosor, 2 por fuera de la capa de 40). El espacio la cambia.
- Ciclo: ahora un desplegable con contorno, como el campo: 56 de alto, esquinas de 4, texto de 16, flecha en el
  color suave (con las puntas suavizadas). Contorno del texto encima, de 2 y del primario con el foco. Teclado:
  abajo, derecha, espacio o Intro, la siguiente; arriba o izquierda, la anterior. Ningún programa lo usa todavía.
- `Lienzo::redondeado_lados`: radio distinto a izquierda y derecha, en una pasada. Hacía falta: dos tramos
  translúcidos (el deslizador desactivado) pintados uno encima del otro salían más oscuros donde se pisaban.
- Campo: sin tocar.
- Pruebas nuevas: `casilla_y_deslizador` (colores por papel, anillo, teclado, el desactivado sin zonas más oscuras),
  `un_trazo_suavizado`; `un_ciclo` mira ahora el contorno en cada estado y el teclado. 40 pruebas del kit, bien.

## 3. El anillo de foco, solo con el teclado

- En la captura final de virt, Aceptar tenía el anillo de foco tras un clic del ratón (un clic del guion le cae
  encima). En Material 3 el anillo solo sale cuando el foco llega con el teclado (como `:focus-visible` en la web).
- `Estado::anillo` (nuevo) y `Interfaz::foco_teclado`: Tab, o una tecla que hace algo en el control con el foco, lo
  ponen; un clic lo quita. Primero valía cualquier tecla, y las "a" del guion que llegan a Aceptar tras el clic
  volvían a encender el anillo (se vio en la captura). El anillo y la capa del foco (10 %) miran `anillo`; el
  contorno del campo y del desplegable sigue mirando `foco` (con el ratón también se escribe en ellos).
- Prueba: en `casilla_y_deslizador`, tras dos clics la casilla tiene el foco y no se ve el anillo, tampoco tras una
  "a"; tras Tab, sí.

## 4. Ajustes

- Título "Ajustes" (estilo 1), el texto que cambia como secundario (estilo 2), y abajo un hueco, Cancelar de texto y
  Aceptar relleno. Sigue en 360 × 250: lo mínimo es 248 de alto.
- `kit/examples/vista.rs` dibuja ahora la misma ventana.
- El guion de virt hacía clic en (300, 300) de la pantalla para darle el teclado. Ahí está ahora Cancelar: se habría
  llevado el foco y las "a" no llegarían al campo. El clic pasa a (400, 130), junto al título, donde no hay nada que
  pulsar; luego el cursor vuelve a (300, 300), así lo que viene después (W8) no cambia.

## 5. Pruebas

- `cd kit && cargo test`: 40, bien.
- `make prueba` (raspi): BIEN, las tres veces.
- `make arranque-virt`: las cuatro BIEN dos veces (carga 15 a 20). Con la carga entre 50 y 63 (otras sesiones) falló
  tres veces, por tiempo y en cosas que no usan el kit: A15 de `ficheros` (como sin mis cambios) y, dos veces,
  `[hostil] ... el tema sigue: no`. Esa es una carrera: con la máquina lenta, el guion pone el modo oscuro en el
  centro antes de que `hostil` mire que el tema sigue siendo el de serie. No es del kit; queda apuntada.
- Capturas: el centro no cambia (no usa estos controles); Ajustes, en la captura final, en oscuro y azul, sin
  anillo en Aceptar tras el clic. En `capturas-agente/` (fuera de git): `antes*`, `despues*`, `muestrario-antes.png`,
  `muestrario-despues.png`, `vista-antes.png`, `vista.png`.

## Pendiente

- La lista no enseña qué fila tiene el foco del teclado (Material 3: la capa de foco sobre la fila). Con Tab se llega
  a ella sin verlo.
- La carrera de `hostil` con el tema (arriba): `hostil` podría mirar el tema antes de que el guion toque el centro,
  o aceptar el que haya puesto el centro.
- En virt, los clics de W8 (un clic cada medio segundo mientras el cursor avanza) caen a veces en Aceptar de Ajustes
  ("el texto dice: Aceptado"). No rompe nada: el campo ya tiene su "a". Antes también pasaba con otros sitios.
- El desplegable (Ciclo) solo pasa de una opción a otra; no abre un menú. Ningún programa lo usa todavía.
- Sin probar en la Pi de verdad: solo QEMU.
