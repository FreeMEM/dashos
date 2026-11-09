# Bucle del kit de interfaz (S11)

Orden del usuario (2026-11-09): tras el compositor Wayland (E1, `docs/WAYLAND_LOOP.md`), el kit de interfaz en Rust;
el sonido (S19) detrás. Diseño: S11 (el modelo de MUI, nativo: clases, atributos, avisos entre objetos, grupos que se
colocan solos, ventanas que se adaptan a la fuente) y S10 (aspecto Material 3: esquinas redondeadas, superficies con
tono, tipografía y espaciado de Material; claro y oscuro). El kit dibuja en superficies de Wayland (S9).

## Dónde está

- Copia de trabajo `.worktrees/kit`, rama `kit`, desde la del compositor (`wayland`).
- `kit/`: la biblioteca (no_std con alloc), probada en el PC dibujando en memoria.

## Decisiones tomadas por defecto (cambiables)

- **Fuente: Roboto** (la de Material 3). Roboto Classic 3.016, la publicación oficial de googlefonts, sin hinting,
  normal y Medium; licencia SIL OFL 1.1 (ya no Apache), que deja llevarla en el repositorio con su licencia
  (`kit/fuentes/OFL.txt`).
- **Rasterizar TrueType con `fontdue`** (MIT o Apache 2.0, sin std), en vez de escribir un rasterizador: es lo más
  grande del kit y está resuelto.
- **Rust primero** (S11 dice Dash para las aplicaciones; Dash nativo para ARM es A7, que va aparte). La API del kit
  se piensa para que los builtins de MUI de Dash la puedan usar después tal cual.

## Pasos

| Paso | Qué | Prueba | Estado |
|------|-----|--------|--------|
| K1 | Lienzo: rectángulos, esquinas redondeadas, bordes, mezcla con alfa, recortes | Píxeles en el PC | hecho |
| K2 | Texto: Roboto con `fontdue`, caché de glifos, medir y dibujar líneas en UTF-8 | Medidas y píxeles; ñ, acentos | hecho |
| K3 | Objetos al modo MUI: clases, atributos (poner y leer), avisos entre objetos (`notify`) | Un deslizador que mueve un texto por aviso | hecho |
| K4 | Grupos que se colocan solos: horizontal y vertical, mínimo, máximo y pesos, espaciado de Material | Medidas al píxel en varios tamaños | hecho |
| K5 | Controles: texto, botón, casilla, campo de texto, deslizador, ciclo, lista | Cada uno, dibujado y con su entrada | texto, botón, casilla y deslizador hechos; campo, ciclo y lista, pendientes |
| K6 | Tema Material 3: colores de un tono base (claro y oscuro), estados (encima, pulsado, foco) | Colores de los estados | el esquema de referencia, hecho; sacarlo de un color base, pendiente |
| K7 | Ventana de Wayland: la superficie, el tamaño que pide el compositor, cuadros a su ritmo, ratón y teclado al objeto con foco | En QEMU, una aplicación de prueba con captura comparada | |

## Bitácora

### 2026-11-09

- K1: `kit/src/lienzo.rs`. Píxeles ARGB8888 premultiplicados (los de `wl_shm`); colores dados sin premultiplicar
  y mezclados encima. Rectángulos, esquinas redondeadas suavizadas (4 × 4 muestras en cada píxel de la curva),
  bordes hacia dentro, recortes apilados. Pruebas: rellenar y mezclar (rojo a medias bajo negro a medias), alfa sobre
  transparente, esquinas (fuera, dentro, un píxel que cruza la curva a medias, simétrico en las cuatro), borde y
  recorte. La primera fórmula de la cobertura estaba mal; se rehízo en octavos de píxel.
- K2: `kit/src/texto.rs`. Roboto normal y Medium (388 KB cada una, con cirílico y griego: se puede recortar más
  adelante) rasterizadas con `fontdue` (sin std: con su `hashbrown`; redondeos con `libm`, que `core` no trae para
  `f32`). Caché de glifos por letra y tamaño, espaciado entre pares de la fuente, medidas de línea (ancho, lo que
  sube y lo que baja de la base). Pruebas: medidas (más texto, más ancho; el doble de tamaño, el doble de ancho; la
  Medium más ancha), las letras del castellano están todas (ñ, tildes, diéresis, ¿¡, €, ç), y "Ñandú" se dibuja
  entero dentro de su caja, con la tilde de la Ñ por encima de la altura de la x. Compila para
  `aarch64-unknown-none-softfloat`.
- K3: `kit/src/objetos.rs`. `Arbol` con objetos de una `Clase` (que ajusta o rechaza valores), atributos numerados
  como las etiquetas de MUI (comunes del 1 al 99: texto, desactivado, valor, pulsado, clic, mínimo, máximo, peso) y
  `notificar` como `MUIM_Notify`: cuándo (siempre o con un valor) y qué se pone en el destino (fijo, el valor que
  cambió, su contrario, o como texto con formato). Un aviso solo salta si el valor cambia, y una cadena de avisos se
  corta a los 32 saltos. Lo que cambia queda "sucio" para dibujarlo. Pruebas: un deslizador (0 a 10) que pone
  "Volumen: 7" en un texto y se queda en 10 si se le pide 99; una casilla que desactiva un botón; dos deslizadores
  atados en los dos sentidos; una cadena que cambiaría siempre, cortada.
- K4: `kit/src/grupos.rs`. Árbol de colocación aparte de los objetos (`Nodo`: hoja con su `Medida`, o grupo
  horizontal o vertical con espacio y margen). `medir` saca la medida de un grupo de la de sus hijos; `repartir` da a
  cada uno su mínimo y reparte lo que sobra por pesos sin pasar del máximo (sin perder píxeles al redondear);
  `colocar` deja el rectángulo de cada hoja, cada una centrada a lo ancho si no llega. Pruebas al píxel: dos botones
  iguales, pesos 100 y 300 con uno fijo en medio, un máximo que cede lo suyo, centrado, grupos dentro de grupos (una
  ventana con texto y fila de botones), y menos sitio que el mínimo (se sale, no se aplasta).
- K6 (adelantado, lo necesitan los controles): `kit/src/tema.rs`. El esquema de referencia de Material 3 (el
  violeta "baseline"), claro y oscuro: primario, contenedor, superficie y su variante, contornos, con sus colores de
  contenido; radio de los botones y tamaños de letra de etiqueta (14) y texto (16). Capas de estado del color del
  contenido: encima 8 %, foco y pulsado 10 %; desactivado: contenido al 38 % y fondo al 12 %. Los valores de los
  colores están puestos de memoria de la especificación: sin comprobar todavía contra m3.material.io. Sacar el
  esquema de un color base (Material You, espacio HCT) queda para después. Pruebas: las capas y que en los dos
  temas el texto contrasta con su fondo.
- K5 (primera tanda): `kit/src/controles.rs`. La `Clase` de K3 gana, con valores por defecto, medida, dibujo,
  pulsable, arrastre y clic. Controles de Material 3: texto; botón relleno (40 de alto, 24 de relleno, esquinas
  redondas, Roboto Medium 14; su `CLIC` pasa a sí y vuelve a no con cada clic); casilla (18 en una zona de 40, con
  marca); deslizador (carril de 4, agarre de 20, valor al arrastrar). Capas de estado y desactivado del tema.
  `Interfaz`: los objetos en filas y columnas (`Hijo`), colocados con los grupos de K4 (8 entre controles, 16 de
  margen), dibujados sobre la superficie, y el ratón (encima, pulsar, arrastrar, clic solo si se suelta dentro,
  foco al pulsar). Prueba: una ventana con texto, deslizador, casilla y dos botones atados con avisos; medidas al
  píxel, colores, capa al pasar por encima, clic en Aceptar, la casilla que lo desactiva (y un clic que ya no hace
  nada), arrastrar el deslizador a 0 y a 75, pulsar fuera y soltar dentro. Se encontró y arregló que la medida
  máxima no sumaba el margen. `cargo run --example vista -- f.ppm [oscuro]` la dibuja en el PC.

