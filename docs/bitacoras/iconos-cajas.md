# Los iconos de las cajas (plan de iconos, parte 3)

Rama `iconos-cajas`, desde `revision` (5470528a). Plan de iconos aprobado por el usuario el 2026-11-13
(`docs/REVISION_LOOP.md`, "Iconos, decidido por el usuario"), parte 3: leer los `.info` de la caja Amiga (clásicos,
NewIcons, GlowIcons, PNG de OS4 y MorphOS) y los iconos de los programas de la caja Linux. Solo el formato: ningún
icono ajeno en git; los de prueba, dibujados por `tools/iconos_amiga_prueba.py`. La rama `iconos` (colección propia y
Material Symbols) cambia a la vez `usuario/src/explorador/iconos.rs`: aquí se le toca lo mínimo.

Sala limpia: la descripción pública de los formatos de Dirk Stöcker (2002: clásico, NewIcons, OS 3.5) y las normas de
freedesktop (Desktop Entry 1.5, Icon Theme 0.13). El trozo `ARGB` de OS4 no tiene documentación pública: se dedujo de
los bytes de iconos de verdad de la máquina (10 bytes de cabecera, zlib, píxeles A R G B), sin leer código de nadie.
Ni AROS, ni UAE, ni el conversor de iconos de JavaScript que salió al buscar.

## Lo hecho

| Pieza | Qué |
|---|---|
| `iconos-cajas/` (crate nuevo, `no_std` + alloc, MPL-2.0) | `amiga`: un `.info` a `kit::imagen::Imagen` (ARGB) con la de seleccionado; `escritorio`: `.desktop`, buscar el icono en el tema, el catálogo de programas; `dibujo`: pintar uno en el lienzo del kit a 28, 48 o 72 |
| `tools/iconos_amiga_prueba.py` | Los `.info` de prueba, uno por formato y uno roto, dibujados con formas calculadas |
| `usuario/src/explorador/iconos_amiga.rs` (nuevo) | El explorador lee a ratos los `.info` de lo que se ve en la caja Amiga y el `Disk.info` del volumen |
| `iconos.rs`, `pintar.rs`, `entrada.rs`, `explorador.rs` | Lo mínimo: un campo `info` en `Fichero` (si lo trae, se pinta ese), la llamada en el bucle, el campo de `App`, esconder los `.info` de otra cosa y el volumen en la barra lateral |
| `puente/src/bin/programas.rs` (nuevo) | Dentro del invitado de Linux: los programas instalados con su icono, en lista o en un catálogo |

### Los `.info`

Se usa lo mejor que traiga el icono, y si está roto, lo siguiente: el `FORM ICON` de OS 3.5 (primero los trozos
`ARGB`, luego los `IMAG` con paleta), luego NewIcons (`IM1=` e `IM2=` en los ToolTypes), luego la imagen clásica. Un
`.info` que empieza por la firma de PNG es de OS4 o MorphOS: el PNG y, si hay otro pegado detrás, el de seleccionado.

- **Clásico**: planos de bits con PlanePick y PlaneOnOff, la paleta de Workbench 2 y 3 de serie (gris, negro, blanco,
  azul 6688BB) y, del 4 al 7, la de MagicWB; el color 0 es transparente. Seleccionado: la segunda imagen si la hay; si
  no, el complemento de todo el rectángulo (`GADGHCOMP`) o sin tocar el fondo (`GADGBACKFILL`), como el Workbench.
- **NewIcons**: cada línea se acaba con lo suyo (los bits que sobran se tiran) y la imagen empieza en la línea
  siguiente a la de la paleta. Comprobado con NewIcons de verdad: salen las filas exactas.
- **GlowIcons**: `FACE` (medidas, sin marco), `IMAG` con las repeticiones de ILBM sobre una tira de bits, la segunda
  con la paleta de la primera si no trae.
- **Defensivo**: medidas negativas o de más de 512, profundidad de más de 8, cuentas de ToolTypes que no caben,
  largos que se salen, zlib que da más de lo que cabe: error, nunca pánico, y se prueba lo siguiente o el icono de
  siempre. Lo que se pide de memoria se mide antes.

### En el explorador

Cada fichero o cajón de la caja Amiga con su `X.info` al lado se ve con ese icono (en la rejilla de 72 y en la lista de
28); el volumen "Amiga" de la barra lateral, con el de su `Disk.info`. Al elegirlo, la imagen de seleccionado.
A su tamaño si cabe, multiplicado por el entero que quepa (píxeles nítidos), o reducido haciendo la media con alfa.
Los `.info` se leen en el propio explorador (S6b), con un tope de 512 KB, unos cuantos por vuelta (8 ms como mucho),
y se guardan por cajón y nombre con su tamaño y su fecha (si cambian, se leen otra vez); 256 como mucho.

### Linux

Hoy ni el lanzador ni la caja Linux enseñan los programas de Linux: el lanzador solo lista `Programas/`
(`docs/bitacoras/escritorio.md`, "de momento, solo `Programas/`") y los programas del invitado se lanzan desde su
terminal. Hecho lo que no depende de cómo lleguen al lanzador:

- `escritorio::leer_desktop` (nombre en el idioma de `LANG`, `NoDisplay`, `Hidden`, solo `Type=Application`, solo el
  grupo `[Desktop Entry]`), `palabras` (el `Exec` partido, sin `%f`, `%U`...), `buscar_icono` (el tema del usuario y
  `hicolor`, del tamaño justo o más grande antes que más pequeño, PNG antes que SVG, `/usr/share/pixmaps`; un nombre
  con barras o `..` no sale del tema).
- El catálogo (`escribir_catalogo` y `leer_catalogo`): nombre, terminal y el PNG del icono (o el camino del SVG) de cada
  programa, con topes (1000 programas, nombres de 120 bytes, iconos de 256 KB) y lectura hostil.
- `programas` (del puente, para el invitado): lo arma recorriendo `applications/` de las carpetas de XDG.

## Pruebas

- `cd iconos-cajas && cargo test` (en `make prueba`): un icono de cada formato hecho aquí con sus píxeles comprobados
  (transparente, colores de la paleta, seleccionado por complemento, segunda imagen, paleta compartida, alfa a medias),
  los hostiles a mano (el `.info` de mentira del disco, medidas negativas, profundidad 9, 600 de ancho, un `FORM ICON`
  roto que deja la clásica, un millón de ToolTypes, un ARGB que miente), y mil mutados al azar con semilla (bytes
  cambiados, cortes, valores extremos, trozos repetidos): ni pánico ni bucles. Los `.desktop` (de verdad, rotos, sin
  UTF-8, gigantes) y el catálogo (ida y vuelta, cortado en cada byte, cuentas falsas, iconos enormes, números
  repetidos).
- Con iconos de verdad (fuera de git; `DASHOS_ICONOS_REALES=carpeta`): los 1148 `.info` de la máquina (Workbench 3.9,
  Directory Opus 5, AmiFire y otros): se leen todos los de Amiga (461 clásicos, 545 GlowIcons, 60 ARGB, 76 NewIcons,
  4 PNG); los 2 que no son los `.info` de GNU texinfo. Vistos en una hoja: bien.
- `programas` en este PC: 130 programas (86 con PNG, 34 con SVG, 10 sin icono, de temas que no son hicolor); los 86 PNG
  del catálogo se leen y se pintan a 48 (`DASHOS_CATALOGO`, prueba opcional).
- En `make arranque-virt`, el paso de la pestaña nueva del explorador va a `Amiga:Iconos` (los `.info` sueltos, cada
  uno con su icono) y a `Amiga:` (el cajón `Iconos` con el suyo; `Iconos.info` y `Disk.info` escondidos), con líneas
  nuevas en `kernel/pruebas/arranque_virt.txt`.

- `make arranque-virt-iconos` (nuevo; `tools/guion_iconos.py`, imagen de usar y copia del disco de pruebas): entra
  Ana, abre el explorador con un doble clic en SYS:, va a `Amiga:Iconos` (los cinco `.info` con su formato y su
  medida; el roto, con el de siempre), elige el de GlowIcons (su imagen de seleccionado), va a `Amiga:` (se ve
  `Iconos` y no sus `.info`, ni el `Disk.info`), Ctrl+H (se ven, con su icono) y Ctrl+2 (la lista). BIEN, con cinco
  capturas en `~/recuperar/geosian-7d/iconos-cajas-capturas/explorador-*.png`.
- `make prueba` BIEN; `arranque-virt-escritorio` y `arranque-virt-linux` BIEN. `arranque-virt`: MAL cinco veces, siempre
  en la familia conocida de teclas y clics perdidos con la máquina cargada ("[ajustes] el compositor pide 800x560",
  "[dash-ventana] pulsado" tres veces con la sesión de Ana bloqueada, "[ajustes] menú abierto; la segunda opción"),
  antes de llegar al paso del explorador: sus líneas nuevas no se han visto salir ahí (sí en `arranque-virt-iconos`,
  que es lo mismo con su propio guion).

## Medido

- `explorador` (release, aarch64): 696 672 bytes en la base, 717 152 con los iconos (+20 KB; el crate va con
  `opt-level = "s"`: con el 3 de las dependencias eran +33 KB). Va en el paquete de arranque, no en el disco.
- Disco de pruebas (`dashfs-fsck` del disco recién hecho): 552 bloques libres de 4 KB sin los iconos y 543 con ellos
  (36 KB: el cajón `Iconos` con 5 `.info` y dos `.info` en la raíz de la caja, 10 KB de datos). Primero eran 18
  bloques; se quitaron los ficheros de compañía de cada `.info` y el ARGB pasó de 64 a 48 (el disco va justo: A15).
  En la prueba, el explorador dice "1,9 MB libres de 7,3 MB", como las otras ramas de la misma base.

## Pendiente del usuario

- **Esconder los `.info`** que son de otra cosa (y el `Disk.info`) en el explorador, como el Workbench; se ven con
  "Mostrar los ocultos". Elegido así para no ver cada cosa dos veces. Copiar, mover, cambiar el nombre o tirar a la
  papelera aún no se lleva el `.info` con su fichero.
- **El icono del volumen Amiga** en la barra lateral es su `Disk.info` en color, entre símbolos de un color.
- **Aspecto de los clásicos**: se pintan con píxeles cuadrados; muchos se dibujaron para 640 × 256 y se ven chatos.
  ¿Doblar el alto de los clásicos?
- **Paleta**: del 4 al 7, la de MagicWB; más allá del 8, un cubo de colores fijo (dependían de la pantalla).
- **Cómo llegan los programas de Linux al lanzador**: quién hace de puente (la caja Linux por el puente de Wayland, una
  trama nueva `PROGRAMAS`, y el lanzador por un canal con la caja o con `sesiones`), cómo se lanza uno (orden al
  invitado con el número del catálogo, con los permisos de S6 tipo Flatpak) y si el usuario elige tema de iconos.
  Toca `inicio`, `sesiones` o el compositor: no se hizo aquí.

## Lo que queda

- Mandar el catálogo por el puente y enseñarlo en el lanzador y en el explorador; lanzar un programa de Linux desde
  ellos. Meter `programas` en la imagen del invitado (`tools/caja_linux_invitado.py`).
- SVG: 34 de 130 programas en este PC solo tienen SVG. Lo pinta quien elija la rama `iconos`.
- Los temas: leer el `index.theme` y sus `Inherits` (hoy, las carpetas de siempre de `hicolor` y del tema que se diga).
- El trozo `icOn` de los PNG de OS4 (tipo, herramienta, ToolTypes): no se lee.
- El Workbench (`workbench.rs`) sigue con sus iconos: no lee los `.info`.
- Los clásicos pequeños (44 × 22) caben una vez en la rejilla de 72: se ven pequeños. Ver "Aspecto de los clásicos".
