# Bitácora: graphics.library e intuition.library nativas de la caja Amiga

Rama `caja-amiga-intuition`, desde `934fd74` de `revision`. Es el principio de H5 (S15): que los programas Amiga abran
ventanas y que cada ventana Amiga sea una ventana del compositor (S4 §3, la idea de Newbench; S23 para pantallas y
Workbench). Crate `caja/intuition/` (`dashos-caja-intuition`, biblioteca `caja_intuition`): graphics e intuition en un
solo crate, `no_std` con `alloc`, sin `unsafe`, probado en el PC. Depende de `caja/exec` (por ruta).

Sala limpia: escrito desde el ROM Kernel Reference Manual (Libraries: graphics, text, intuition), los autodocs de
graphics.library e intuition.library, los includes del NDK y los `.fd` como especificación de la interfaz. Los
desplazamientos de las estructuras se sacaron con `offsetof` en el compilador del 68k sobre esos includes, y las
pruebas los fijan. Sin código de AmigaOS, AROS, UAE ni Amiberry. Nada de la ROM: la fuente topaz/8 está dibujada a
mano aquí (`src/fuente.rs`); solo se copian sus medidas (8 x 8, línea base 6), que son interfaz.

## Qué hay

**graphics.library 40.1** (tabla de saltos completa de `graphics_lib.fd` hasta V40, 172 entradas):

- Estructuras de 3.1: `BitMap`, `RastPort`, `TextAttr`, `TextFont`, `ColorMap`, `ViewPort`, `Layer`, `GfxBase`.
- Dos clases de mapa:
  - **Nativo**: ARGB de 32 bits fuera de la memoria del 68k (`Lienzo`), lo que espera el compositor, con la zona
    cambiada apuntada. Son los de las ventanas, la pantalla y los de `AllocBitMap` de más de 8 planos o con un amigo
    nativo. Su `struct BitMap` dice 8 planos con `Planes[]` a cero.
  - **Planar**: el que monta el programa (`InitBitMap` + `AllocRaster`, o `AllocBitMap` de hasta 8 planos, también
    entrelazado). Se dibuja bit a bit en sus planos, en la memoria del 68k, como el chipset (planos 0 y "todo unos",
    `rp_Mask`).
- Dibujo: `InitRastPort`, `SetAPen`, `SetBPen`, `SetDrMd`, `SetABPenDrMd`, `Get*Pen`, `GetDrMd`, `SetOutlinePen`,
  `SetWriteMask`, `Move`, `Draw` (Bresenham en los ocho octantes, con `LinePtrn`), `PolyDraw`, `RectFill` (con
  `AreaPtrn`), `WritePixel`, `ReadPixel`, `SetRast`, `ScrollRaster`/`ScrollRasterBF`, `ClearEOL`, `ClearScreen`. Modos
  JAM1, JAM2, COMPLEMENT e INVERSVID. Todo recortado al mapa y a la capa de la ventana.
- Bloques: `BltBitMap` (con minitérmino y máscara, solapado bien, entre planar y nativo pasando por la paleta),
  `ClipBlit`, `BltBitMapRastPort`, `BltTemplate`, `BltClear`, `WriteChunkyPixels`.
- Texto: `Text` (lee la `TextFont` de la memoria del 68k, así que vale cualquier fuente que monte el programa: fija
  o proporcional, con `CharSpace` y `CharKern`), `TextLength`, `SetFont`, `AskFont`, `OpenFont` (busca en
  `GfxBase->TextFonts` sin mirar mayúsculas, el tamaño más cercano), `CloseFont`. topaz/8 propia: ASCII y lo de Latin-1
  para castellano, catalán y portugués (ñ, ¿, ¡, vocales con tilde, ç, ü, °); lo demás sale como una caja.
- Mapas: `InitBitMap`, `AllocRaster`, `FreeRaster`, `AllocBitMap`, `FreeBitMap`, `GetBitMapAttr`.
- Paletas: `GetColorMap`, `FreeColorMap`, `SetRGB4`, `SetRGB4CM`, `SetRGB32`, `SetRGB32CM`, `LoadRGB4`, `LoadRGB32`,
  `GetRGB4`, `GetRGB32`, `FindColor`, `ObtainBestPenA`, `ObtainPen`, `ReleasePen`. Cada `ColorMap` tiene su tabla
  ARGB nativa y se copia a `ColorTable`/`LowColorBits` para quien los lea a mano.
- `WaitTOF`, `WaitBlit`, `OwnBlitter`, `DisownBlitter`, `VBeamPos`: no esperan a nada.

**intuition.library 40.1** (tabla de `intuition_lib.fd` hasta V40, 134 entradas):

- Estructuras de 3.1: `Screen`, `Window`, `NewWindow`/`ExtNewWindow`, `IntuiMessage`/`ExtIntuiMessage`, `DrawInfo`,
  `IntuitionBase`, y las etiquetas `WA_*`, `WFLG_*`, `IDCMP_*`, `IECODE_*`, `IEQUALIFIER_*`.
- La pantalla de Workbench: del tamaño del escritorio que dice el compositor, mapa nativo de 32 bits, `ColorMap` de 256
  colores (los cuatro de 3.1 delante), `DrawInfo` con las plumas de 3.1, `TextAttr` topaz/8.
- `OpenWindow`, `OpenWindowTagList` (con `NewWindow`, `ExtNewWindow` y etiquetas: `TAG_MORE`, `TAG_SKIP`,
  `TAG_IGNORE`), `CloseWindow`, `RefreshWindowFrame`, `SetWindowTitles`, `ModifyIDCMP`, `ActivateWindow`,
  `WindowToFront`, `WindowToBack`, `MoveWindow`, `SizeWindow`, `ChangeWindowBox`, `WindowLimits`, `BeginRefresh`,
  `EndRefresh`, `LockPubScreen`, `UnlockPubScreen`, `GetScreenData`, `GetScreenDrawInfo`, `FreeScreenDrawInfo`,
  `ViewPortAddress`, `CurrentTime`, `DisplayBeep`, `SetMenuStrip`/`ClearMenuStrip`/`ResetMenuStrip` (se guardan, no
  salen), `SetPointer`/`ClearPointer`/`SetWindowPointerA` (no hacen nada), `ScreenToFront`/`ScreenToBack`.
- Etiquetas de ventana: posición, tamaño, `WA_InnerWidth/Height`, plumas, `WA_IDCMP`, `WA_Flags` y todas las
  booleanas de gadgets y banderas, título de ventana y de pantalla, `WA_CustomScreen`, `WA_PubScreen`,
  `WA_PubScreenName` con `WA_PubScreenFallBack`, límites, `WA_MouseQueue`, `WA_RptQueue`.
- IDCMP desde los eventos del compositor: `CLOSEWINDOW` (gadget de cerrar o el borde de DashOS), `MOUSEBUTTONS`
  (SELECT, MIDDLE; MENU solo con `RMBTRAP`), `MOUSEMOVE` (con `REPORTMOUSE`), `RAWKEY` (código crudo del Amiga desde
  evdev, con el de soltar, calificadores, teclado numérico y repetición; `IAddress` a ceros para `MapRawKey`),
  `VANILLAKEY` (el carácter que dé el compositor, o una distribución estadounidense), `NEWSIZE`, `REFRESHWINDOW`,
  `CHANGEWINDOW`, `ACTIVEWINDOW`, `INACTIVEWINDOW`. Topes de `MouseQueue` (5) y `RptQueue` (3).
- Cada `Window` tiene en la memoria del 68k su `RastPort`, una `Layer` (con `rp`, `bounds`, `Window`, tamaño) y su
  `BitMap`; `UserPort` lo crea `CreateMsgPort` de la tarea que abre; `WindowPort` es de intuition (`PA_IGNORE`).
- Los gadgets del sistema dentro del búfer: cerrar (pulsar y soltar dentro), profundidad, tamaño y barra de arrastre
  (estos tres se los pide al compositor). Borde activo azul y título en la fuente de la pantalla.

## Pruebas

`cargo test` en `caja/intuition/`: **47 pruebas** en 4 ficheros, todas bien; `clippy --all-targets` sin avisos.
Compila para `aarch64-unknown-none-softfloat` y `aarch64_be-unknown-none-softfloat` (nightly, `-Z build-std`).

- `estructuras.rs` (8): desplazamientos y tamaños de todas las estructuras, etiquetas, LVO conocidos, las bibliotecas
  abiertas con `OpenLibrary` de exec (y la entrada de `Move` que exec da como `Ajena`), `GfxBase` y la fuente (cada
  ASCII con dibujo propio), la pantalla de Workbench.
- `dibujo.rs` (15): una línea de Bresenham conocida punto a punto; 16 direcciones en mapa nativo y planar (extremos,
  un punto por columna o fila, nada fuera); líneas y rectángulos que salen del mapa, al revés o con coordenadas
  extremas; JAM1, JAM2, INVERSVID y COMPLEMENT con texto comparado con el glifo; COMPLEMENT en nativo y esquinas de
  una polilínea; `WritePixel`/`ReadPixel` dentro y fuera; máscara de planos; patrón de línea y de área; fuentes;
  paletas (12 y 32 bits, `LoadRGB*`, `ObtainPen`, `FindColor`); copias entre planar y nativo, solapadas y con
  minitérmino; `ScrollRaster`, `BltTemplate`, `WriteChunkyPixels`; `AllocBitMap` entrelazado y nativo; planos 0 y
  "todo unos".
- `ventanas.rs` (15): abrir con etiquetas y con `ExtNewWindow`, bordes y tamaños, lo que recibe el compositor (píxeles y
  daños), cerrar soltando toda la memoria, siete mensajes IDCMP en orden con sus campos y `ReplyMsg`, la señal que
  despierta a la tarea en `Wait`, gadgets, cambio de tamaño con límites, teclado crudo y `VANILLAKEY`, cola del ratón,
  `ModifyIDCMP` y puerto compartido entre dos ventanas, pantallas públicas, títulos y foco, y una captura.
- `hostil.rs` (9): `RastPort` nulos, en la página 0 o fuera; `BitMap` de 9 planos, con planos fuera o filas que no
  caben; tamaños negativos, cero y enormes (mapas y ventanas); cerrar dos veces o lo que no es una ventana; `NewWindow`
  fuera, pantalla que no existe, etiquetas en círculo, compositor que no acepta (sin fugas); mensajes sin contestar al
  cerrar (la ventana espera y se suelta cuando vuelven, o al morir la tarea); un `ReplyMsg` ajeno al `WindowPort`;
  fuentes y paletas rotas; y **400.000 llamadas al azar** (cuatro semillas) a todas las funciones, con punteros
  buenos, malos y números extremos, eventos del compositor y bytes de las estructuras estropeados a ratos: ningún
  pánico (en depuración, con comprobación de desbordes). Esa prueba encontró sumas de punteros que daban la vuelta,
  un bucle sin fin al recoger mensajes de un puerto estropeado y mapas planares sin planos de tamaño enorme.

La captura: `CAPTURA_PNG=fichero.png cargo test --test ventanas captura` guarda la ventana que dibuja la prueba.

No hay aún prueba del oráculo (regla 5): hace falta que la caja corra binarios 68k con graphics e intuition
enchufadas. Lo que se dio por bueno sin oráculo está en "Decisiones provisionales".

## Cómo se enchufa

En la caja:

```rust
let mut gr = CajaGrafica::instalar(&mut exec, MiCompositorWayland::nuevo(..), Numeros::default())?;
// En dashos_native_call, cuando exec devuelve Accion::Ajena:
match gr.llamar(&mut exec, n, &mut regs) {
    Accion::Ajena => { /* de otra biblioteca nativa (dos...) */ }
    Accion::Ceder => { /* despertó a una tarea de más prioridad: pasar el testigo */ }
    _ => {}
}
let _ = gr.tomar_avisos(); // al registro del Enforcer, con los de exec
```

1. **Números nativos**: `Numeros::default()` reparte graphics desde `0x0400` e intuition desde `0x0600` (fila `i` del
   `.fd` = número base + `i`). Tienen que ser mayores que 255 y no chocar con los de dos (rama `caja-amiga-dos`): si
   chocan, se cambia `Numeros`. Las bibliotecas se crean con `crear_biblioteca_nativa` de exec y quedan en `LibList`.
2. **Ninguna función bloquea**: no hay que repetir llamadas. `llamar` devuelve `Seguir` (o `Ajena`).
3. **El compositor** es el rasgo `Compositor` (`src/compositor.rs`). La caja lo implementa sobre Wayland: una
   superficie `xdg_toplevel` por ventana con un búfer `wl_shm` ARGB8888 del tamaño de la ventana entera (bordes de
   Intuition incluidos); `presentar` copia la zona dañada al búfer y manda `wl_surface.damage`; `arrastrar`,
   `redimensionar` son `xdg_toplevel.move`/`resize`; `mover`/`pedir_tamano` se contestan con `Evento::Posicion` y
   `Evento::Tamano` cuando el compositor los acepte (`xdg_toplevel.configure`). Los eventos de `wl_pointer`
   (posición de la superficie, botones de evdev), `wl_keyboard` (código de evdev y, si se puede, el carácter de xkb
   con los modificadores de ahora) y el foco salen por `sacar_eventos`.
4. **El bucle**: con el testigo de la caja tomado (exec e intuition no son de varios hilos), a cada cuadro o al llegar
   eventos, `gr.atender(&mut exec)`: recoge los mensajes contestados, convierte los eventos en IDCMP (con `PutMsg`, que
   da la señal a la tarea) y manda al compositor lo que ha cambiado. Si devuelve `true`, `exec.elegir_siguiente()`.
   Fuera de eso, `gr.presentar()` manda los daños (por ejemplo, al volver de una tanda de llamadas nativas).
5. **La hora**: `gr.poner_hora(segundos, micros)` desde 1978 (para `CurrentTime` y los mensajes).
6. **Matar una tarea**: antes de `exec.matar_tarea(t)`, `gr.tarea_muerta(&mut exec, t)` cierra sus ventanas.

## Decisiones provisionales

- **Adornos** (S4 "Lo que queda abierto" 1): por dentro, los de Intuition dibujados en el búfer; por fuera, el borde
  de DashOS (y ninguno con `BORDERLESS` o `BACKDROP`, `NuevaVentana::sin_borde`). Es lo que recomiendan S4 y S23. El
  dibujo de los gadgets es propio. Medidas: barra de 11 (2 + fuente + 1), lados de 4, abajo 2, gadget de tamaño 18 x
  10 a la derecha (abajo con `SIZEBBOTTOM`), cerrar 20 de ancho, profundidad 23. A comprobar con el oráculo.
- **Cada ventana tiene su mapa**: `Window->RPort->BitMap` es el de la ventana, no el de la pantalla, y la capa no
  traslada (las coordenadas del `RastPort` ya son las de la ventana). Un programa que dibuje en el `BitMap` de la
  pantalla sumando `LeftEdge` no verá nada en su ventana.
- **El `BitMap` nativo** dice `Depth` 8 y `BytesPerRow` como si fuera planar, con `Planes[]` a 0: quien escriba en los
  planos da en la página 0 y lo apunta el Enforcer. `GetBitMapAttr` dice la verdad (32 bits).
- **Plumas en mapas de 32 bits**: se traducen al dibujar; cambiar la paleta no cambia lo ya pintado. COMPLEMENT
  invierte el color (RGB) en los nativos y los bits de los planos de la máscara en los planares; `rp_Mask` no cuenta
  en los nativos.
- `InitRastPort` pone `FgPen` = `AOlPen` = 255, `Mask` 0xFF, JAM2, `LinePtrn` 0xFFFF y topaz.
- `Draw` en COMPLEMENT no pinta el primer punto de una línea encadenada (sin `Move` antes), para no invertir dos
  veces las esquinas; la de cierre de un polígono sí se invierte dos veces.
- Los patrones de línea toman un bit por punto del 15 hacia abajo desde `linpatcnt`; los de área se anclan al origen
  del mapa.
- Sin límites en `NewWindow` (0) el tamaño mínimo y máximo es el de apertura, como dice el autodoc: sin
  `WA_MaxWidth/Height` la ventana no crece. Las ventanas que no caben en la pantalla se mueven y se encogen (como
  `WA_AutoAdjust`). Solo hay Workbench: `WA_CustomScreen` u otra pantalla dan 0.
- `CloseWindow` suelta los mensajes que esperan en su `UserPort` (en un puerto compartido, solo los suyos, como hace
  `CloseWindowSafely`); si el programa tiene alguno sin contestar, la ventana queda pendiente con su `WindowPort` vivo
  hasta que vuelvan. `ModifyIDCMP(0)` y `CloseWindow` solo borran el `UserPort` que creó intuition.
- El botón derecho sin `RMBTRAP` no llega (los menús no están). La tecla Menú del PC es Amiga derecha; Inicio, Fin,
  Re Pág, Av Pág, Insert, F11 y F12 no dan código crudo.
- Paleta del Workbench: los cuatro colores de 3.1, cuatro más propios, un cubo de 6 x 6 x 6 y grises. La `DrawInfo`
  dice profundidad 8.
- `ObtainPen` reparte de la pluma 4 a la 251. `WaitTOF` no espera al cuadro. `OpenFont` no va a disco (eso es de
  diskfont).
- Topes: mapas nativos de hasta 16384 de lado y 64 millones de píxeles; 4096 etiquetas; 4096 mensajes recogidos de
  una vez. Un mapa planar sin ningún plano de verdad se trata como vacío.

## Lo que falta

- **Planos virtuales** de los mapas nativos (que un programa que escriba en `Planes[]` de una ventana vea planos y se
  conviertan a ARGB): hace falta que la caja atrape esos accesos (páginas sin permiso en el proceso) y los avise.
- Pantallas propias (`OpenScreen`, `OpenScreenTagList`, `CloseScreen`, la base de modos, P2-P7 de S23), pantallas
  públicas con nombre, mapas planares de pantalla convertidos en cada cuadro, HAM y EHB.
- layers.library nativa (capas de verdad, `ClipRect`, regiones, `InstallClipRegion`), `GIMMEZEROZERO` y
  `SUPER_BITMAP`, `BorderRPort`, el refresco simple con zonas dañadas de verdad.
- Gadgets de usuario (`AddGadget`, `RefreshGList`, boopsi), menús (`MENUPICK`, la barra de la pantalla, S23 P6),
  requesters, `EasyRequest`, `AutoRequest`, el puntero propio, `INTUITICKS`, `MENUVERIFY`, `SIZEVERIFY`.
- graphics: elipses, `Area*` y `Flood`, estilos algorítmicos (`SetSoftStyle`), `TmpRas`, `ReadPixelArray8`,
  `WritePixelArray8`, `BltPattern`, `BltMaskBitMapRastPort`, gels y sprites, vistas y copper (`LoadView`, `MakeVPort`),
  la base de modos (`GetDisplayInfoData`, `BestModeIDA`). Lo que no está da el aviso `NoImplementada` y D0 = 0.
- gadtools.library y diskfont.library nativas.
- La prueba del oráculo de cada función, cuando la caja ejecute binarios 68k con estas bibliotecas.
