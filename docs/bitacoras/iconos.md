# Iconos: Material Symbols, la colección de DashOS y los temas de Linux

Rama `iconos`, desde `revision` (5470528a). Encargo: partes (1) y (2) del plan de iconos aprobado el 2026-11-13
(`docs/REVISION_LOOP.md`), y dejar preparado cómo se instala y se elige un tema de Linux. La parte (3), `.info` de
Amiga e iconos de la caja Linux, es de la rama `iconos-cajas`. Diseño: `docs/diseño/S37-iconos.md`.

## Diseño en tres frases

Los símbolos de la interfaz son Material Symbols Rounded (Apache 2.0), bajados del repositorio oficial como SVG y
recortados a una lista; cada uno es una constante con su trazado, y el kit los pinta con el mismo rasterizador que las
letras de Roboto (`kit/src/trazos.rs`). La colección de DashOS son 72 iconos en un texto de capas de trazados SVG
(`kit/iconos/dashos.iconos`), cada capa con un papel que se vuelve un tono del tema (Material You, tonal), con formas
compartidas y emblemas de Material. Un tema de Linux instalado en `Home:Iconos/` se elige en Ajustes > Apariencia
(`Escritorio/iconos` de `ENV:`/`ENVARC:`), y el explorador lo busca con el algoritmo de la norma de freedesktop y lo
pinta con un lector de SVG mínimo propio (no hay pintor de SVG `no_std`).

## Hecho

- `kit/src/trazos.rs`: el rasterizador de `texto.rs`, sacado tal cual (el texto da los mismos bits) y compartido; más
  `cubrir`, que recorta lo de fuera del cuadro sin recorrerlo (un tramo a un millón de píxeles no da un millón de
  vueltas).
- `kit/src/iconos/`: `trazado.rs` (los datos `d` de SVG: M L H V C S Q T A Z, absolutas y relativas, arcos a cúbicas;
  un lector de números propio, sin `str::parse`), `mod.rs` (`Simbolo`, `Simbolos` con caché de 512 KB como tope, el
  símbolo de reserva), `coleccion.rs` (la colección, tonos por familia, icono de reserva), `freedesktop.rs`
  (`index.theme`, cadena de herencia, búsqueda de la norma, PNG y SVG, la elección), `svg.rs` (lector mínimo),
  `simbolos_datos.rs` (generado) y `pruebas.rs`.
- `tools/iconos_material.py`: baja (a `~/.cache/dashos-iconos/`, fuera de git), comprueba y genera. 94 símbolos, 37 KB
  de trazados, del commit 10ecc147 del repositorio de Google. `kit/iconos/NOTICE` y la licencia al lado.
- Sustituidos los dibujados a mano: la barra (lanzador, campana, CPU, memoria, red, sonido, la lupa del lanzador), el
  panel (mosaicos de red, modo oscuro y bloquear; botones de bloquear y apagado; menú de apagado; volumen, brillo,
  OSD; los chevrones, cerrar, añadir y quitar de la edición), los menús del kit (marca de casilla y flecha de
  submenú), el desplegable del kit (`arrow_drop_down`/`up`) y el explorador (barra de herramientas, lateral, pestañas,
  ordenar, carpeta vacía). Ajustes: las categorías llevan su icono (lista de navegación con `atr::ICONOS`).
- El explorador pinta los ficheros con la colección (`iconos.rs`: la clase y, afinado, por la extensión y por el
  nombre del cajón: Documentos, Música, Prefs, Libs...).
- Ajustes > Apariencia: filas "Iconos: ..." solo si hay algún tema en `Home:Iconos/`. El explorador lee la elección al
  prepararse y carga un icono del tema por vuelta, como las miniaturas.

## Pruebas

- `cargo test` del kit: 92 bien (antes 69). Nuevas: trazados (cuadrado, medio píxel, cuatro escrituras iguales, área
  del círculo, anillo, curvas), trazados malos (20 de basura, el tope de tramos, muy fuera del cuadro), todos los
  símbolos a 1-72, los tamaños límite (0, negativos, 1, 1024, 100 000, `i32::MAX`, fuera del lienzo), símbolo roto a la
  reserva, la colección entera y 12 colecciones malas (con su línea), cada icono en 6 tamaños y 3 temas, los tonos
  siguen al tema, el tope de la caché, SVG (sencillo, degradados y `defs`, 12 malos, hondura, elementos, tamaño),
  `index.theme` (con un cajón `../fuera`), la búsqueda en la cadena (exacto, escalable, cercano, heredado, `hicolor`,
  nombres más cortos, nombres que intentan salir), herencia infinita, PNG y SVG de un tema, la elección en texto y el
  lector de números.
- Cambiadas dos pruebas del kit (`un_ciclo`, `el_menu_de_un_desplegable`): contaban los píxeles del triángulo a mano
  del desplegable; ahora los del símbolo de Material (de 6 a 8 enteros en la fila ancha).
- Con el código final (iconos de la lista de Ajustes a 20, con 8 y 6 de hueco): `make prueba` BIEN a la tercera (las
  dos primeras, con la máquina a carga 28, fallaron líneas de SMP del núcleo que no toco: "C32: reparto de páginas
  desde varios núcleos" y "proceso con ASID 3 en el núcleo 1"). `arranque-virt` dos vueltas, las dos con fallos de la
  familia conocida de revision: "[dash-ventana] pulsado" y "[ajustes] el desplegable dice"; en las dos, el paso de
  Permisos del centro esperó "[centro] quita a" porque una ventana de la caja Amiga (ReAction Demo) o dash-ventana
  salió encima y se llevó los clics y las teclas (captura `despues/centro-version-final.png`). Por la regla del
  coordinador, se da por buena.
- Con el código de una vuelta antes (la lista con 8 y 8 de hueco, 4 píxeles de diferencia): `arranque-virt` BIEN
  entero (con el reinicio), `arranque-virt-escritorio`, `arranque-virt-shell` y `arranque-virt-menus` BIEN (las
  capturas de menús, a `~/recuperar/geosian-7d/iconos-capturas/despues/menus/`, no a la carpeta de menus-pulido). Las
  capturas de `despues/` son de esa vuelta.
- Lo que sí fue mío y se deshizo: con la ventana de Ajustes a 600 (para los iconos de 24 de Material), se salía de la
  pantalla de 800 y el guion no acertaba con el botón Quitar.
- Ninguna captura que compare píxeles cambió de resultado. Una línea esperada se tocó y se volvió a dejar como estaba
  (`[centro] vuelve a 560x420`).

## Lo medido

- Tamaño (texto y datos de solo lectura, en bytes, antes y después): explorador 626 968 → 774 628 (+148 KB: 37 KB de
  trazados de Material, 14 KB de la colección, el lector de SVG, `index.theme`, la búsqueda de freedesktop y la
  colección); centro +35 KB; panel +19 KB; barra +18 KB; workbench +15 KB, ajustes +13 KB y selector +11 KB (solo el
  kit: el desplegable y los menús ya pintan símbolos). La imagen `kernel8-virt.img`: 9 531 576 → 9 810 104 (+272 KB).
- Recortes hechos al medir: sin `str::parse::<f32>` (lector de números propio), sin el formato de floats en el SVG,
  sin `f32::clamp` con tope variable (su pánico traía el formato de floats, unos 10 KB por programa) y sin
  trigonometría en los programas que solo pintan símbolos (los arcos, solo en la colección y el SVG). Antes de esto:
  ajustes +22 KB, explorador +165 KB.
- Disco de pruebas: el explorador dice 1,9 MB libres de 7,3 MB, como antes (los programas van en la imagen, no en ese
  disco). A15 BIEN.
- Memoria (Ajustes > Monitor, en `arranque-virt`): 67,6 MB → 68,0 MB en total; el explorador pasa a 1,0 MB (antes no
  salía entre los once primeros, menos de 0,9 MB); centro, panel y ajustes, igual. La caché de la colección, con los 72
  iconos a 28 y 72: 125 KB (tope 512 KB).
- Coste en el PC (`cargo run --release --example medir_iconos`): un símbolo de 24 sin caché, 18 µs (la palabra
  "Archivos" a 14, 40 µs); con caché, 2 µs. Un icono de la colección de 72: 60 µs sin caché y 40 µs con ella (pintar
  sus capas píxel a píxel; antes de quitar el recorte por píxel, 60 µs).

## Decisiones (pendientes del usuario)

- Ajustes: la ventana sigue de 560. Con los iconos a la medida de Material (24 y 12 de hueco) no cabía: a 600 se salía
  de la pantalla de 800 de las pruebas (y el guion no daba con Quitar). Van de 20, con 8 y 6 de hueco.
- Los iconos de la barra van a 18 (los de Material dejan margen; a 16 se veían pequeños al lado del texto).
- Sin red, el icono "lan" con una raya encima (Material no tiene "lan_off").
- Colores de cada tipo de fichero (familias) y el papel de las hojas en oscuro (gris oscuro tonal, no blanco).
- El explorador lee el tema de iconos al prepararse: un cambio en Ajustes se ve al volver a arrancarlo (no vigila
  `ENV:` aún).
- Los textos nuevos de Ajustes ("Iconos: ...") van sin catálogo.

## Lo que queda y lo que no se ha comprobado

- No se ha probado un tema de Linux de verdad (Papirus, Adwaita) en DashOS: solo los temas de prueba en el PC. Las
  filas de Ajustes de los temas no salen en las pruebas (no hay ninguno instalado en su disco).
- El Workbench sigue con sus iconos a mano (no estaba en el encargo).
- Solo el tamaño óptico 24 de Material; a 16 se nota algo más fino que el diseño de 20.
- Los `.info` y los iconos de la caja Linux: rama `iconos-cajas`. Su `iconos.rs` del explorador choca con este: aquí
  `icono` toma la colección y el tema (`ico`, `linux`).
