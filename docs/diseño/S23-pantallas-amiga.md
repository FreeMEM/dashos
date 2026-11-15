# S23. Las pantallas del Amiga

Del 2026-11-10. Lo pidió el usuario: cómo se resuelven las pantallas (screens) de los programas del Amiga: los que usan
el Workbench, los que abren la suya, los que clonan otra. Junta y completa lo que decían S4 (sección 3), S10
("Pantallas") y S10b ("Las ventanas de la caja Amiga"). Es diseño: no hay código.

## El problema

En el Amiga, una pantalla (`Screen`) es a la vez un espacio con su resolución, su profundidad de color y su paleta, y
un sitio donde viven ventanas. Los programas las usan de cuatro maneras:

1. **En el Workbench**: abren ventanas en la pantalla pública por defecto. La mayoría de las herramientas y de los
   programas MUI.
2. **En otra pantalla pública con nombre**: `LockPubScreen("DOPUS.1")`, MUI que se abre donde diga su configuración,
   pantallas que se pasan de un programa a otro.
3. **En una pantalla propia**: `OpenScreen` con su modo (PAL, HiRes, entrelazado, HAM, AGA, RTG), su profundidad y su
   paleta. Programas de dibujo, juegos, demos, reproductores.
4. **En una pantalla que clona otra**: `SA_LikeWorkbench`, o copiar a mano el modo, el tamaño, las fuentes y los
   colores del Workbench, para tener "un Workbench propio" (DOpus, Scalos, programas con su pantalla pública).

Y además: las pantallas se arrastran hacia abajo dejando ver la de detrás (y se pueden dejar a medias, viendo dos), se
pasan al frente o al fondo, pueden ser más grandes que el monitor (se desplazan solas con el ratón), y cada una tiene
su barra de título y sus menús, que salen con el botón derecho.

En DashOS no hay chips del Amiga ni un monitor de 640 x 256: hay un compositor de Wayland, monitores modernos y
escritorios virtuales. Los programas tienen que funcionar igual, y verse bien.

## La regla que lo une todo

**Cada pantalla del Amiga es una pantalla de DashOS** (un escritorio virtual de la lista de S10), y tiene dos partes:

- **Su fondo**: el mapa de bits de la pantalla, con lo que el programa pinta directamente en ella y sus ventanas de
  fondo (`BACKDROP`) o sin borde que la llenan. El compositor lo escala a la pantalla real.
- **Sus ventanas**: cada ventana de Intuition con borde es una ventana del compositor encima de ese fondo, con su
  sitio en el mosaico o flotante, como cualquier otra.

Con esa regla, los cuatro casos salen solos:

| Caso | Fondo | Ventanas | Cómo se ve |
|---|---|---|---|
| Workbench | El fondo del escritorio de DashOS (y lo que pinte, por ejemplo, Trasfondo de 68k) | Ventanas del escritorio, mezcladas con las nativas | Como cualquier programa de DashOS |
| Otra pantalla pública | El de esa pantalla | Ventanas del compositor en esa pantalla de DashOS | Un escritorio virtual con nombre ("DOPUS.1") |
| Pantalla propia de un juego o un programa de dibujo | Su mapa de bits, escalado con su proporción | Las que tenga, encima | A pantalla completa, como en el Amiga |
| Pantalla clonada del Workbench | El de esa pantalla | Ventanas del compositor | Otro escritorio virtual con el mismo modo y aspecto que el Workbench |

## Workbench

- **La pantalla de Workbench es el escritorio de DashOS** (S4). Su tamaño es el área útil del escritorio, en píxeles
  lógicos (con la escala del monitor), y su modo es uno RTG de 32 bits ("DashOS: 2560 x 1440 x 32", por ejemplo).
- **Compatibilidad por programa**: algunos programas viejos se rompen con un Workbench enorme de 32 bits (cuentan con
  8 colores, 640 de ancho o fuentes Topaz). Cada programa puede tener una ficha que le enseñe un Workbench "clásico"
  (por ejemplo 640 x 512 x 8 con sus plumas): ese programa ve ese Workbench, y sus ventanas se dibujan con esa
  paleta y se escalan. Se elige en las propiedades del programa (como "modo de compatibilidad" en Windows), y la caja
  trae una lista de programas conocidos.
- **Los adornos**: por dentro, como en el Amiga (barra de título y gadgets de Intuition); por fuera, el borde de
  DashOS. Con el tema "Amiga clásico" (S10), solo los de Intuition.

## Pantallas públicas

- Cada pantalla pública con nombre es una pantalla de DashOS de la lista. `LockPubScreen`, `UnlockPubScreen`,
  `PubScreenStatus`, `SetDefaultPubScreen` y los avisos de cierre se cumplen como en el Amiga.
- **Pantalla pública por defecto**: el Workbench, salvo que el usuario elija otra (Prefs, o el programa con
  `SetDefaultPubScreen` y los modos `SHANGHAI` y `POPPUBSCREEN`).
- Una pantalla pública vacía y sin dueño se cierra sola, como en el Amiga.

## Pantallas propias y modos

- **La base de modos** (`GetDisplayInfoData`, `NextDisplayModeID`, `BestModeID`, el pedidor de modos de ASL) ofrece:
  - Los **modos del chipset**: PAL y NTSC; LowRes, HiRes y SuperHiRes; entrelazado y DblPAL; profundidades de 1 a 8
    planos; EHB, HAM6 y HAM8. Se dibujan en mapas planares (S4) que la caja convierte a píxeles en cada cuadro.
  - Los **modos RTG** de 8, 15, 16, 24 y 32 bits, como Picasso96 y CyberGraphX, con las resoluciones del monitor real
    y las de siempre.
- **Escalado con su proporción**: un modo PAL LowRes tiene píxeles más altos que anchos; se escala respetando eso, en
  múltiplos enteros cuando cabe (nítido), con bandas negras si no. Opción de filtro "monitor de la época" (líneas de
  barrido) para el que lo quiera.
- **Paleta**: en pantallas de 256 colores o menos, el compositor traduce la paleta al mostrar (S4). Los cambios de
  paleta (`LoadRGB32`, ciclos de color) cuestan solo cambiar la tabla, a 60 cuadros.
- **Promoción de modos**: el usuario puede subir los modos del chipset de un programa a RTG (como ModePro o la
  promoción de AmigaOS 3.2), por programa.
- **Pantallas más grandes que el monitor**: se desplazan solas al llevar el ratón al borde (autoscroll), como en el
  Amiga.
- **Lo que toca los chips a mano** (listas de Copper propias, registros) no pasa por aquí: va a la caja de chips de
  S18, cuya imagen es también una pantalla de DashOS.

## Pantallas clonadas

- `SA_LikeWorkbench` y lo que copia el modo del Workbench a mano reciben lo que ve ese programa del Workbench (el de
  verdad, o el clásico de su ficha de compatibilidad): mismo modo, tamaño, profundidad, fuentes y plumas.
- La pantalla clonada es otra pantalla de DashOS de la lista, con su propio fondo y sus ventanas. Así DOpus o Scalos en
  su pantalla propia se ven como un escritorio virtual más, con el mismo aspecto que el Workbench.

## Pasar, arrastrar y ordenar

- **Al frente y al fondo** (`ScreenToFront`, `ScreenToBack`, Amiga izquierda + M y N): cambiar de pantalla de DashOS.
- **Arrastrar hacia abajo** por la barra de título deja ver la pantalla de detrás, a 60 cuadros, en el compositor
  (S10b). **Y se puede dejar a medias**, viendo dos pantallas a la vez, como en el Amiga. Cada una con su resolución.
- **Varios monitores**: cada pantalla del Amiga va al monitor donde esté; se pueden mover de uno a otro como los
  escritorios virtuales (S10).

## Barra y menús

- **La barra de título de una pantalla del Amiga** es la barra de esa pantalla de DashOS (S10, "La barra de la
  pantalla", arriba como en el Amiga). En el Workbench, la barra de DashOS.
- **Los menús de Intuition** de la ventana activa salen en esa barra al pulsar el botón derecho, como en el Amiga,
  con sus atajos de Amiga derecha. Los programas nativos con menús (GadTools o MUI, S11) hacen lo mismo.

## Cómo se prueba

- **Contra el oráculo** (AGENTS.md, regla 5): los mismos programas 68k en FS-UAE con el Kickstart y en DashOS:
  `OpenScreen` con cada etiqueta, `LockPubScreen`, `GetScreenData`, la base de modos, `SA_LikeWorkbench`. Se comparan
  los valores que devuelven, no los píxeles.
- **Capturas** por QMP: una pantalla PAL LowRes con su proporción, una HAM6, una con ciclo de colores, dos pantallas
  a medias.
- **Programas de verdad**: Deluxe Paint (pantalla propia), DOpus Magellan (pantalla pública y clonada), un programa
  MUI que se abre en otra pantalla pública, Trasfondo de 68k en el fondo del Workbench.

## Fases

Van con H5 (S15): necesitan exec, graphics, layers e intuition nativas.

| Fase | Qué | Prueba |
|---|---|---|
| P1 | Workbench: tamaño, modo RTG, ventanas como ventanas del compositor, adornos | Una ventana de 68k en el escritorio; valores contra el oráculo |
| P2 | Pantallas públicas con nombre como pantallas de DashOS | Un programa MUI en "PRUEBA.1"; `PubScreenStatus` contra el oráculo |
| P3 | Pantallas propias: base de modos, escalado con proporción, paletas, planares, HAM | Capturas de PAL LowRes, HiRes entrelazado, HAM6 y AGA de 8 planos |
| P4 | Pantallas clonadas y la ficha de compatibilidad por programa | `SA_LikeWorkbench` contra el oráculo; un programa viejo con su Workbench clásico |
| P5 | Arrastrar, dejar dos a la vez, al frente y al fondo, autoscroll, varios monitores | Captura con dos pantallas a medias; teclas Amiga + M y N |
| P6 | Menús en la barra con el botón derecho | Un programa GadTools de 68k: su menú en la barra |
| P7 | Promoción de modos por programa | Un programa PAL promovido a RTG |

## Lo que queda abierto

1. ¿Cada pantalla del Amiga, una pantalla de DashOS (escritorio virtual), con su mapa de bits de fondo y sus ventanas
   como ventanas del compositor? Recomendado: sí. Es lo que une los cuatro casos.
2. ¿Las pantallas propias a pantalla completa por defecto, con la opción de verlas en ventana con una tecla?
   Recomendado: sí; es lo que espera quien viene del Amiga.
3. ¿Ficha de compatibilidad por programa con un Workbench clásico? Recomendado: sí, con una lista de programas
   conocidos de serie.

## Cómo encaja

- S4: graphics, layers e intuition nativas; mapas planares; la caja como aplicación.
- S10 y S10b: las pantallas son los escritorios virtuales; el compositor arrastra a 60 cuadros.
- S11: los menús en la barra, también para los nativos.
- S18: lo que toca los chips va a la caja de chips, cuya imagen es una pantalla más.
- S21: escalar y traducir paletas en el HVS cuando se pueda (un plano con su tabla de color).
