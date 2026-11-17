# Bitácora: el dock y los avisos en pantalla como los de DMS (S27)

Rama `dms-dock`, sacada de `revision` (e60553a6). Petición del usuario (2026-11-16): un escritorio configurable como DMS
de niri, portándolo ("si te lo puedes fusilar, estaría bien"); y después: "si puedes darle el mismo aspecto que mi
escritorio en Shiva ya sería la rehostia". Diseño: `docs/diseño/S27-escritorio-dms.md` (sección 0: DMS es MIT desde el
12/11/2025, se porta su código con su aviso).

## Diseño en tres frases

El dock es un programa propio (`dock`, cliente del compositor como la barra y el panel) que pinta con el kit lo que
calculan tres módulos sin nada de DashOS, portados del QML de DMS v2.1.2: qué lleva y dónde (`dock_modelo.rs`), los
avisos en pantalla (`osd_modelo.rs`) y los muelles y curvas de sus animaciones (`muelle.rs`). Las ventanas abiertas
llegan por un protocolo nuevo, `dashos_dock_v1`, que solo ve el dock: la lista de quien está delante, solo los cambios
tras cada tanda, y traer, cerrar y el lanzador; por ahí llegan también el espacio de trabajo, el volumen y Bloq Mayús.
Los ajustes son texto en `ENV:Escritorio/dock` y `ENV:Escritorio/osd` (S36, regla 1), que el dock vigila y aplica en
marcha, y los escribe Ajustes > Dock (o la orden `Ajuste`, o un editor); lo de serie es lo del escritorio de shiva.

## Lo portado de DMS (licencia MIT, aviso en `docs/licencias/DankMaterialShell-MIT.txt`)

El texto de la licencia es el de upstream en la etiqueta v2.1.2 (el mismo que puso la rama `dms-barra`, mismo nombre
de fichero). Cada fichero portado lo dice en su cabecera, con los ficheros de DMS de los que sale:

| DashOS | De DMS |
|---|---|
| `usuario/src/dock_modelo.rs` | `ApplicationStrip.qml` (agrupar o no, fijados, lanzador, separador), `ApplicationModel.js` (desbordado), `ApplicationItem.qml`, `DockBody.qml` y `DockGeometry.qml` (cuerpo, capa, zona exclusiva, cuándo se ve, deslizamiento), `DockAppButton.qml` (indicador, letra sin icono, clics), `DockHoverBounce.qml` (saltito), `DockContextMenu*.qml` (menú) |
| `usuario/src/osd_modelo.rs` | `DOSD.qml` (sitio por `osdPosition`, tiempos, escala y opacidad), `LevelOSD.qml`, `OsdLevelRow.qml`, `OsdIcon.qml`, `DSlider.qml` (pista y tirador "s" y "m"), `VolumeOSD.qml`, `BrightnessOSD.qml`, `CapsLockOSD.qml`, `WorkspaceOSD.qml`, `OSDManager.qml` |
| `usuario/src/muelle.rs` | `SpringMotion.qml` (integrador), `Theme.qml` (`springSpecs`, `tunedSpring`, duraciones), `Anims.qml` (curvas) |
| `usuario/src/dock_ajustes.rs` | `DockConfig.js` (`create`, `normalize`, `pick`), `SettingsSpec.js` (los `osd*`) |
| `usuario/src/bin/dock.rs` | lo visible de lo de arriba, pintado con el kit |

Medidas de DMS que se quedan tal cual: grueso = icono + 2 × relleno; hueco para el saltito = 35 % del icono; capa =
grueso + 30 % del icono + ese hueco; escondido = grueso + 10; saltito al 25 % y luego al 20 % del icono (200 ms con
`emphasizedAccel` y `emphasizedDecel`); indicador `circle` de max(4, icono/10), el activo tres veces más largo y en el
primario; 250 ms de gracia al salir el ratón y 250 ms para la burbuja; el deslizamiento con el muelle "fast" de la
duración corta (75 ms con la base de 250); menú con filas de 28, de 180 a 400 de ancho, a 10 de los bordes; OSD de nivel
de 264 + 56 por 56 (vertical 60 × 352), Bloq Mayús 60 × 60, espacio desde 140; 3 s, 2 s y 1,5 s; escala de 0,92 a 1
con el muelle "default" de la duración media y opacidad con OutQuart; pista "s" de 24 con tirador de 4 × 36 y hueco de 6.

## Lo de serie: el escritorio de shiva

De `~/recuperar/geosian-7d/aspecto-shiva/dms-settings.json` (su `dockConfigs[0]` y sus `osd*`): abajo, iconos de 40,
relleno 4, separación 4, margen 0, opacidad 66 %, se esconde solo, sin agrupar, indicador de punto, con el lanzador;
el radio de las esquinas es el de niri en shiva (`niriLayoutRadiusOverride` 3); los OSD arriba en el centro
(`osdPosition` 4) con el valor siempre a la vista, y la redondez 38 (`radiusStrength`). El del espacio de trabajo no
sale de serie (DMS tampoco, y shiva no lo pone); se pone en Ajustes > Dock.

## El compositor: `dashos_dock_v1`

`wayland/protocolos/dashos-dock-v1.xml` (tablas regeneradas: solo se añade), `wayland/src/dock_escena.rs` y unos
enganches en `escena.rs` y `compositor.rs`. Solo lo ve el dock (por el nombre de `inicio`; a los demás no se les
anuncia). Las ventanas: de primer nivel, en pantalla, de quien está delante (no las de una sesión de detrás), de todos
los espacios, como mucho 128, cada una con un número que no se repite. El programa de cada ventana es el nombre del que
la abrió (el de `Programas/`, que dice `sesiones`); las de las cajas, "linux:" o "amiga:" y su `app_id`, así un programa
de una caja no se hace pasar por uno nativo. Tras cada tanda se dice solo lo que cambió y `hecho`: abrir y cerrar mil
veces en una tanda no dice nada. Traer una ventana de otro espacio pasa a ese espacio. Con la sesión bloqueada, traer y
el lanzador no hacen nada.

## El volumen, ahora del dock

El OSD del volumen era del panel (S27, primera oleada). Pasa al dock, como en DMS: el compositor avisa al dock de cada
tecla del volumen (además de dársela al panel, que sigue llevando el volumen) y le pasa el nivel que el panel le da con
`estado`. El panel ya no pinta OSD; `volumen_osd.rs` se queda con el volumen. Las líneas de la prueba cambian:
"[panel] OSD abierto/cerrado" por "[dock] OSD volumen" y "[dock] OSD cerrado: juntó 7". El OSD dura ahora 3 s (DMS).

## Permisos nuevos (por el nombre "dock" de `inicio`)

- Compositor: las capas y `dashos_dock_v1`.
- `sesiones`: lanzar lo fijado (`LANZADORES`, solo de `Programas/`, como quien está delante), leer `ENV:`
  (`LEEN_IDIOMA`) y guardar sus ajustes en `ENVARC:` (`GUARDAN_AJUSTES`: escribe su `ENV:` y pide `GUARDAR_ENV`), y sale
  como programa del escritorio en el monitor (`ESCRITORIO`).
- `inicio`: su ficha (como el centro: de Ana, encerrado en `C`) y el disco para leer (los ajustes de fábrica).

## Pruebas

(Se completa abajo con los resultados.)

## Pendiente del usuario

1. **`GUARDAN_AJUSTES` para el dock**: para guardar lo fijado desde su menú, el dock escribe en el `ENV:` de la sesión y
   pide `GUARDAR_ENV`; con eso podría escribir cualquier fichero de `ENV:` del usuario. Alternativa: que lo fijado lo
   guarde el centro a petición del dock. Elegí lo primero (es lo que hace DMS: el dock guarda sus fijados).
2. **El OSD del espacio de trabajo no sale de serie** (como DMS y como shiva). Se pone en Ajustes > Dock.
3. **Un clic en la ventana que ya tiene el teclado** la trae (no hay minimizar en DashOS); DMS la minimiza.
4. **El radio del dock** (3, el de niri en shiva) y la redondez de los OSD (38) van en los ficheros del dock y del OSD;
   en DMS son del tema entero. Cuando `dms-barra` tenga el aspecto común, que salgan de ahí.
5. **Sin icono**: la letra en un cuadrado redondeado (como DMS). `icono_programa::icono` es donde enchufar la colección
   de `iconos` e `iconos-cajas`. Los glifos de los OSD y del menú van trazados con el lienzo hasta tener Material Symbols.
6. **El botón del lanzador del dock** abre el lanzador de la barra (DMS abre el suyo, que es el mismo en el centro).

## Lo que no se hizo

- El aumento al pasar el ratón (`magnification`): se lee y se guarda, pero no se pinta (shiva lo tiene apagado).
- Arrastrar para ordenar los fijados, el modo de edición de los widgets del dock, la papelera, "lanzar con la gráfica
  dedicada" y las acciones de escritorio del menú: DashOS no tiene esas piezas.
- Varios docks (DMS deja uno por borde): uno, en los monitores que diga `monitores`.
- El OSD del brillo está hecho pero no sale: no hay quien lleve el brillo.
- Esconderse si una ventana lo tapa (`ocultar_con_ventanas`, `smartAutoHide`): se lee; falta que el compositor diga si
  una ventana tapa el borde.
- Pantalla completa: DashOS no tiene ventanas a pantalla completa aún; el ajuste se lee.
