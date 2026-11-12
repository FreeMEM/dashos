# Bitácora: fondo de escritorio y colores del fondo (C3)

Rama `fondo-c3`, sacada de `revision`. Fase C3 de S10c (`docs/diseño/S10c-centro-de-control.md`): "colores sacados del
fondo (esquema de Material 3 hecho aquí, desde la especificación)", con la prueba "los colores salen con contraste
suficiente en claro y oscuro".

## 2026-11-12: fondo, semilla, esquemas repartidos y sus pruebas

### Lo hecho

- **PNG en el kit** (`kit/src/png.rs`, nuevo). Hecho desde las especificaciones (PNG del W3C, zlib y deflate de las
  RFC 1950 y 1951). Todos los tipos de color, 1 a 16 bits, paleta con transparencia y entrelazado Adam7. Comprueba el
  CRC de cada trozo y el Adler-32. Antes de pedir memoria mira lo que mide: más de lo que se deja es "demasiado
  grande", y lo descomprimido no puede pasar de lo que dice la cabecera (una bomba de zlib da error). Pide la memoria
  con `try_reserve`: sin memoria, un error, no un aborto. El "servicio de imágenes" que había (`imagen.rs`) es el de
  los discos ADF, no lee imágenes de dibujo.
- **Escalar** (`kit/src/imagen.rs`, nuevo). Cubre la pantalla como un fondo: sin bandas, recortando lo que sobra por
  igual a cada lado. Bilineal con enteros; si encoge a menos de la mitad, antes reduce por cuadros.
- **Semilla** (`kit/src/semilla.rs`, nuevo). El método de Material ("Material Color Utilities"), hecho desde su
  descripción: muestras (como mucho unas 16 000), cuantizar a 128 colores con el método de Wu (todo con enteros),
  puntuar en HCT por proporción del matiz (±15°) y croma, descartar croma < 5 y proporción ≤ 1 %, y elegir con matices
  separados (de 90° a 15°). Material refina con k-medias en Lab; aquí no (sin FPU costaría segundos, y para la
  semilla no hace falta). Una imagen gris no da semilla: el centro pone la de serie (violeta).
- **Tema** (`kit/src/tema.rs`). `esquemas(color)` da los dos esquemas de una vez. `Tema::del_sistema` mira antes los
  esquemas que llegaron hechos (`recordar_esquemas`, `recordar_evento_esquema`) y recuerda los últimos cuatro que
  calculó. Sin `unsafe`: atómicos con una versión (impar mientras se escribe).
- **Protocolo** `dashos_tema_v1` versión 2: la petición `esquema` (solo el centro, 20 colores justos) y el evento
  `esquema`, que el compositor manda antes de `tema` a quien enlazó la 2, si el color es el de los esquemas. Los
  eventos nuevos van al final (el número de `panel` no cambia). Prueba de PC en `wayland/src/tests.rs`.
- **`fondo`** (`usuario/src/bin/fondo.rs`, nuevo). Capa de fondo de layer-shell (nivel 0, zona exclusiva -1), con el
  poder de escritorio. Lee `C/Prefs/fondo` (si no hay, `ondas.png`) y pinta `C/Fondos/<nombre>`. Si no se puede, se
  queda el color liso o el fondo de antes. Por su canal "fondo" solo le habla el centro: "poner <nombre>" y
  "semilla". Pantallas de más de 4 MB (lo que mide un objeto de memoria): en franjas, una capa cada una. Formato en
  `docs/referencia/servicios.md`.
- **Centro** (`centro.rs`). En Apariencia, "Color del fondo" tras los cuatro acentos, y debajo una fila por cada PNG
  de `Fondos/` (marca en el que se ve). "Color del fondo" pide la semilla a `fondo`, calcula los dos esquemas una vez,
  los guarda en `Prefs/apariencia` ("oscuro fondo 3d6f88" y los veinte colores) y los da al compositor. Poner otro
  fondo: si `fondo` lo pone, se guarda en `Prefs/fondo`, y si el color es el del fondo, el tema sale del nuevo. Al
  abrir y al cambiar el modo desde el panel, los esquemas guardados van otra vez al compositor, sin calcular.
- **Ajustes** enlaza la versión 2 y usa los esquemas que llegan; dice si vinieron hechos, y su superficie y primario.
- Compositor: `fondo` tiene el poder de escritorio; tema en versión 2; las capas de fondo no echan sombra. `inicio`: ficha de `fondo` (usuario 1000,
  raíz `C`). `kernel/src/inicio.rs`: una entrada para `fondo` (antes del centro, que es cliente de su canal) y el canal
  "fondo" en la lista del centro. No hay otra forma de lanzar un programa; no toqué nada más del kernel.

### Medidas (QEMU virt, sin FPU, el PC cargado por otras sesiones)

| Qué | Tiempo |
|---|---|
| Leer ondas.png (150 KB, 960 × 600) del disco y descomprimirlo | 91 ms |
| Leer atardecer.png (23 KB, 640 × 480, RGBA) | 60 ms |
| Semilla (muestras, Wu, HCT de 128 colores, puntos) | 11 a 16 ms |
| Escalar a 800 × 600 y pintar | 15 ms |
| Los dos esquemas de la semilla (`esquemas`) | 552 a 747 ms |
| Tema con los esquemas recibidos (Ajustes) | 9 a 69 µs |

Los esquemas pasan de 50 ms de largo: se calculan una vez en el centro al elegir el color o al cambiar de fondo, se
guardan con la apariencia y los reparte el compositor. En el PC (con FPU): leer 15 ms, semilla 1 ms.

### Pruebas

- Kit (`cargo test`): PNG de todos los tipos (hechos con `tools/fondos_prueba.py --pruebas`, en `kit/src/pruebas/`),
  zlib de Python, rotos, cortados, basura al azar y enormes; escalar; semillas; y la de contraste nueva,
  `contraste_con_colores_del_fondo`: imágenes muy saturadas, grises, casi negras, casi blancas y otras; con su semilla
  (o la de serie), en claro y oscuro, texto sobre superficie y contenedores, sobre primario, sobre contenedor primario,
  contenedor secundario y variante, a 4,5:1 como poco. Pasa.
- Wayland: `esquemas_del_centro`.
- Virt: en el disco, `Prefs/fondo` dice `enorme.png` (40000 × 40000): el fondo no lo pone y se queda el color liso.
  Desde Apariencia, el guion pone `ondas.png`, elige "Color del fondo" (Ajustes, abierta, lo sigue con los esquemas
  del centro), pone `rota.png` (CRC malo: se quedan el fondo y el tema) y `atardecer.png` (el tema sale del nuevo).
  La comprobación del disco exige `Prefs/fondo` = atardecer.png y la apariencia con su semilla y sus veinte colores.
  `captura_ventanas.py` calcula el color del fondo en cada punto como el kit (`fondos_prueba.cubrir_en`) y toma los
  colores de Ajustes de la salida.
- Los fondos de prueba los hace `tools/fondos_prueba.py` (degradados y ondas calculados, sin imágenes de nadie). Se
  guardan en `kernel/target/fondos-prueba/` para no rehacerlos (cuestan 11 s en Python).
- `C/Fondos` va antes que `Prefs` y `Privado`: el selector necesita una flecha más para llegar a `Privado` (guion).
  Sin eso, la primera vuelta falló en cadena (selector, Ajustes y Super+L esperaban al guion atascado).
- Las franjas: una vuelta a mano con el tope de memoria a 1 MB (dos franjas en 800 × 600). Se pintaban bien, pero la
  franja de abajo echaba sombra sobre la de arriba (una raya oscura). Arreglado en `escena.rs`: las capas de fondo no
  echan sombra. Prueba de PC `franjas_de_fondo_sin_sombra` (falla sin el arreglo).

### Decisiones que debe mirar el usuario

- Sin semilla (imagen gris), el violeta de serie; Material pone el azul de Google (0x4285F4).
- Sin el refinado de k-medias de Material: con imágenes de pocas zonas de color sale la misma semilla; con fotos
  podría salir otra cercana.
- `fondo` lo lanza `inicio` (entrada en `kernel/src/inicio.rs`), solo en virt como el centro.

### Pendiente

- `panel.rs` y `guardian.rs` (no eran de esta rama) siguen con la versión 1: con el color del fondo calculan el esquema
  ellos (200 a 350 ms la primera vez; luego lo recuerdan). Basta con enlazar la 2 y llamar a
  `kit::tema::recordar_evento_esquema` con el evento `esquema`. Lo mismo el selector.
- Que el tema guardado se ponga al arrancar (hoy, al abrir el centro o tocar el modo en el panel).
- Elegir fondo con el selector de ficheros y no solo de `Fondos/`; JPEG; las franjas no están en la prueba de virt
  (su pantalla cabe en una).
- Al fusionar con `escritorio` (la barra de arriba): `captura_ventanas.py` mira el fondo en (5, 60); allí también hay
  que usar `fondo_en`. El fondo ya pide zona exclusiva -1, así que va también bajo la barra.
- "Automático" (claro u oscuro por la hora), letras y movimiento, el resto de C3.
