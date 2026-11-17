# S37. Iconos

Plan aprobado por el usuario el 2026-11-13 (`docs/REVISION_LOOP.md`): Material Symbols para la interfaz, una colección
propia que toma el color del tema, temas de Linux instalables y, aparte, los `.info` de Amiga y los iconos de la caja
Linux (rama `iconos-cajas`). Principio de S0: amigable de 2026 y nivel hacker.

## 1. Símbolos de la interfaz: Material Symbols Rounded

- Fuente: el repositorio oficial de Google (`github.com/google/material-design-icons`, Apache 2.0), fijado en un commit.
  Procedencia y licencia en `kit/iconos/NOTICE` y `kit/iconos/LICENCIA-material-symbols.txt`.
- Se usan los SVG, no la fuente: la fuente variable pesa varios MB y recortarla pide `fonttools` (o hacer un recortador
  de `glyf` y `gvar`). Cada SVG es un solo trazado en la caja `0 -960 960 960`, el mismo contorno que el glifo de la
  instancia FILL 0 (o 1), peso 400, grado 0, tamaño óptico 24.
- La lista de los que se usan está en `kit/iconos/simbolos.lista` (más los emblemas de la colección). La herramienta
  `tools/iconos_material.py` baja lo que falte a una caché fuera de git, comprueba cada SVG (un `<path>`, la caja de
  siempre, solo órdenes y números) y escribe `kit/src/iconos/simbolos_datos.rs`: una constante por símbolo con su
  trazado (`simbolo::SEARCH`). Cada programa enlaza solo los que nombra; quien busca por nombre (`Simbolo::por_nombre`,
  la colección) los lleva todos (37 KB de trazados).
- Se pintan con el rasterizador de las letras del kit (`kit/src/trazos.rs`, compartido con `texto`): un trazado SVG es
  un contorno como el de un glifo. `Simbolos` guarda cada máscara (símbolo y tamaño) con un tope de 512 KB.
- Dónde: la barra (lanzador, campana, CPU y memoria, red, sonido, buscar), el panel rápido (mosaicos, botones,
  menú de apagado, volumen, brillo, el OSD), los menús del kit (la marca de casilla y la flecha de submenú), el
  desplegable del kit (`arrow_drop_down`), las categorías de Ajustes (lista de navegación con icono, `atr::ICONOS`;
  de 20, a 8 del borde y con el texto a 6, y no de 24 con 12 como pide Material 3, para caber en la ventana de 560: con 600 se salía de
  la pantalla de 800 de las pruebas y el guion no daba con el botón Quitar) y
  el explorador (barra de herramientas, barra lateral, pestañas, ordenar, carpeta vacía).
- Se quedan dibujadas a mano: la marca de la casilla del kit (`controles.rs`, es el dibujo de Material 3 de la casilla,
  no un icono), el punto de la opción, y los iconos del Workbench (no estaban en el encargo; pendiente).

## 2. La colección de DashOS

- 72 iconos (carpetas, tipos de fichero, programas, volúmenes, aparatos) en `kit/iconos/dashos.iconos`, un texto con
  capas de trazados SVG en una rejilla de 64. Cada capa lleva un papel (fondo, hondo, acento, sobre, claro, papel,
  sombra, luz) que se convierte en un tono del tema según la familia del icono (primario, secundario, terciario,
  neutro, error o la marca naranja de la caja Amiga). Así el icono toma el acento, y el acento puede salir del fondo de
  escritorio (C3): Material You, tonal.
- Formas compartidas (carpeta, hoja, baldosa, disco) más un emblema, que es un Material Symbol lleno. Los volúmenes
  raros (USB, SD, óptico, disquete, RAM) y la papelera están dibujados enteros. Debajo de 24 píxeles el emblema de las
  carpetas y hojas no se pinta (no se lee); en las baldosas sí, porque es el dibujo.
- Los tonos salen de mezclas de los papeles del `Tema` (enteros): calcular paletas HCT sin FPU cuesta cientos de ms.
- Cada icono dice sus nombres de la norma de freedesktop (`linux=`), para que un tema de Linux lo sustituya.
- La hoja: `cargo run --release --example iconos -- hoja.ppm simbolos.ppm [acento]` (en `kit/`).
- Por qué no SVG con `resvg`: `resvg` y `usvg` piden `std`; `tiny-skia` pinta sin `std` (con `no-std-float`) pero no
  lee SVG, y el rasterizador del kit ya hace lo que hace falta. Para los temas de Linux hay un lector mínimo propio
  (`kit/src/iconos/svg.rs`).

## 3. Temas de iconos de Linux (instalados por el usuario)

- DashOS no trae ninguno: Papirus es GPL y Adwaita CC-BY-SA.
- **Instalar**: copiar el cajón del tema (el que tiene `index.theme`) a `Home:Iconos/`. Con el explorador basta
  arrastrarlo; cuando haya paquetes (S8), `Paq` lo dejará ahí. `SYS:Iconos/` queda para temas de todo el sistema
  (vacío). `hicolor`, si se instala, es el último de la cadena, como dice la norma.
- **Elegir**: Ajustes > Apariencia enseña, si hay algún tema en `Home:Iconos/` (no oculto, con `index.theme` que se
  lee), las filas "Iconos: los de DashOS" e "Iconos: <nombre>"; elegir una guarda `Home:Prefs/iconos`
  (`tema: <cajón>` o `tema: dashos`). Sin temas instalados, las filas no salen.
- **Usar**: el explorador lee la elección al prepararse (`tema_linux.rs`): la cadena del tema (él, los que hereda,
  `hicolor`), y busca cada icono con el algoritmo de la norma (tamaño exacto, el más cercano, nombres más cortos por
  guiones). Lo pinta un icono por vuelta del bucle, como las miniaturas, y lo que no encuentra o no puede pintar sale
  de la colección de DashOS.
- **Lo que se lee**: PNG (con `png`) y SVG con el lector mínimo: `path`, `rect`, `circle`, `ellipse`, `polygon`,
  `polyline`, rellenos con color, `currentColor` (el texto del tema), el primer color de un degradado, opacidades,
  `transform` y `viewBox`. No pinta trazos (`stroke`), texto, máscaras ni filtros. XPM no se lee.
- **Seguridad**: todo lo del tema es de fuera. Topes de tamaño (`index.theme` 256 KB, icono 512 KB, 4000 elementos,
  32 de hondura, 100 000 tramos por trazado, lado de 1024), nombres que no salen del cajón (`..`, `/`, `.oculto`),
  herencia cortada a 8 temas y sin bucles. Un fichero malo da el icono de DashOS.

## Pendiente del usuario

- La familia de cada tipo de fichero (qué color lleva cada uno) y el papel de las hojas en oscuro (gris oscuro tonal,
  no blanco): elegido lo que sigue a Material You; se cambia en `dashos.iconos`.
- Los textos nuevos de Ajustes ("Iconos: ...") van sin catálogo, en castellano.
- Pasar el Workbench a la colección (hoy dibuja sus iconos a mano).
- Un tema de Linux de verdad no se ha probado en DashOS (solo con temas de prueba en el PC).
