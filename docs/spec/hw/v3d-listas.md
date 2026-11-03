# Listas de control mínimas de la V3D 4.2: borrar y guardar sin Mesa

Especificación de sala limpia. La escribe el analista leyendo Mesa. Describe el formato de las listas de control de
la V3D 4.2 que hacen falta para el primer trabajo de la GPU: un render que borra las baldosas a un color fijo y las
guarda en un búfer lineal RGBA8888. Quien implemente no necesita leer Mesa: todo lo necesario tiene que estar aquí.

El hardware y el lanzamiento de trabajos están en [`v3d.md`](v3d.md) (secciones 7.2, 7.4 y 13). Lo que pide Mesa al
kernel está en [`mesa-v3d.md`](mesa-v3d.md). Este documento solo cubre el contenido de las listas.

## 0. Origen y convenciones

- **Fuente**: `mesa/mesa` de freedesktop, commit `668d1b7a50d94a6aedd6fdac83ad905c31196b17` (3 de noviembre de 2026).
  Es posterior al de `mesa-v3d.md`. Ficheros leídos:
  - `src/broadcom/cle/v3d_packet.xml`: la definición de todos los paquetes.
  - `src/broadcom/cle/gen_pack_header.py`, `v3d_packet_helpers.h`, `v3d_decoder.c`: cómo se empaquetan.
  - `src/gallium/drivers/v3d/v3dx_rcl.c`, `v3dx_draw.c`, `v3d_job.c`, `v3d_resource.c`, `v3d_cl.h`: OpenGL ES.
  - `src/broadcom/vulkan/v3dvx_meta_common.c`, `v3dvx_cmd_buffer.c`, `v3dv_cmd_buffer.c`, `v3dv_queue.c`,
    `v3dvx_formats.c`, `v3dv_pass.h`: Vulkan, sobre todo el relleno de búferes y el borrado de imágenes.
  - `src/broadcom/common/v3d_util.c`, `v3d_limits.h`, `v3d_tiling.h`; `src/broadcom/clif/`; `src/util/u_pack_color.h`.
- **Licencia**: MIT. Se puede leer y citar.
- **Citas**: cada dato lleva entre corchetes el fichero y la función de Mesa de donde sale. Por ejemplo,
  [`v3dvx_meta_common.c`, `emit_frame_setup`]. "XML" es `v3d_packet.xml`.
- **Comprobado y deducido**: "Mesa hace X" quiere decir que se leyó en el código. "Se deduce" es una conclusión del
  analista. **Nada de esto se ha ejecutado en una GPU.** Los volcados de la sección 8 se calcularon a mano y se
  contrastaron con los empaquetadores que Mesa genera de su XML (`gen_pack_header.py` con versión `42`), compilados
  en el ordenador del analista con un programa de prueba fuera del repositorio. Eso comprueba la codificación, no
  que la GPU haga lo esperado.
- **Notación**: "VA" es una dirección virtual de la GPU, de 32 bits (`v3d.md`, sección 6). Los bits de un paquete se
  cuentan desde el primer bit del código de operación: los bits 0 a 7 son el código. Un campo `[h:l]` ocupa los bits
  de `l` a `h`, ambos incluidos. "RT" es un destino de dibujo ("render target").
- **Nombres**: los de Mesa (`STORE_TILE_BUFFER_GENERAL`, `END_OF_LOADS`...), para poder contrastarlos.
- **Versión**: el XML lleva paquetes con `min_ver` y `max_ver`. Aquí solo valen los de la versión 42. Un paquete sin
  esos atributos vale para todas. Donde la 4.2 y la 7.1 usan el mismo código con otro formato, se da el de la 4.2.

## 1. Visión de conjunto

### 1.1. Cómo dibuja la V3D

- La V3D dibuja por **baldosas**. Cada baldosa se dibuja entera en una memoria interna, el "tile buffer" o TLB.
  Luego se guarda en la RAM con un paquete de guardado ("store").
- El trabajo tiene dos fases. El **binning** (hilo 0 del CLE) reparte las primitivas y escribe una lista por
  baldosa en la memoria de baldosas. El **render** (hilo 1) recorre las baldosas. Para cada una ejecuta su lista.
- La lista del render se llama **RCL**. La del binning, **BCL**.
- Las baldosas se agrupan en **supertiles**: rectángulos de baldosas. La RCL pide supertiles uno a uno. El hardware
  recorre las baldosas de cada supertile. [`v3dx_rcl.c`, `emit_render_layer`]
- Para cada baldosa, el render ejecuta una **lista genérica de baldosa** ("generic tile list"). Es una sublista que
  vale para todas las baldosas. Contiene las cargas, un salto a la lista que escribió el binner para esa baldosa
  (`BRANCH_TO_IMPLICIT_TILE_LIST`) y los guardados. [`v3dx_rcl.c`, `v3d_rcl_emit_generic_per_tile_list`]
- Se deduce que el hardware calcula solo el desplazamiento de cada baldosa dentro de la imagen: la lista genérica es
  una sola para todas y lleva una sola dirección de destino.

### 1.2. Mesa siempre hace binning

- **Mesa no envía nunca un render sin binning.** Incluso para borrar una imagen o rellenar un búfer, v3dv crea una
  BCL vacía: configuración, `START_TILE_BINNING` y `FLUSH`. [`v3dvx_meta_common.c`, `meta_fill_buffer`;
  `v3dvx_cmd_buffer.c`, `job_emit_binning_prolog` y `job_emit_binning_flush`]
- Gallium envía siempre memoria de baldosas y estado de baldosas en la 4.2. [`v3d_job.c`, `v3d_job_submit`]
- La cabecera del kernel dice que un render sin binning sirve para copias y borrados que recorren un conjunto fijo de
  baldosas (`v3d.md`, 7.4). Mesa no lo usa.
- Sin binning, la memoria de baldosas no tiene listas válidas. Si la lista genérica salta a ellas con
  `BRANCH_TO_IMPLICIT_TILE_LIST`, el CLE ejecuta basura. Se deduce que **un render sin binning no debe llevar ese
  salto**.

### 1.3. Las dos variantes de este documento

| Variante | Qué es | Trabajos | Base |
|---|---|---|---|
| **A** (principal) | Render solo. La RCL de Mesa para rellenar un búfer, **sin** `BRANCH_TO_IMPLICIT_TILE_LIST` | Render | Se deduce de Mesa |
| **B** (respaldo) | Lo que hace Mesa exactamente: un binning vacío y luego el mismo render, **con** el salto | Binning y render | Comprobado en Mesa |

- La variante A es lo que pide la tarea. Si el render no termina o no escribe, se prueba la B (sección 11).
- Las dos usan el mismo borrado. El color sale de un paquete de configuración. El TLB se borra una vez al principio.
  Como ninguna baldosa dibuja nada, cada baldosa guarda el color de borrado. Es lo que hace Mesa al rellenar un búfer.
  [`v3dvx_meta_common.c`, `meta_emit_fill_buffer_rcl` y `emit_frame_setup`]

## 2. Cómo se codifica un paquete

[`gen_pack_header.py`, clases `Field` y `Group`; `v3d_packet_helpers.h`; `v3d_decoder.c`,
`v3d_spec_find_instruction`]

### 2.1. Forma

- Un paquete es una serie de bytes seguidos en la memoria. **No hay alineación**: un paquete empieza en el byte
  siguiente al anterior. Mesa reserva la lista genérica con alineación 1. [`v3dx_rcl.c`,
  `v3d_rcl_emit_generic_per_tile_list`, `v3d_cl_ensure_space(cl, 200, 1)`]
- El **byte 0 es el código de operación**.
- **No hay campo de longitud.** Cada código tiene una longitud fija. El decodificador de Mesa la saca de la tabla.
  Se deduce que el CLE hace lo mismo.
- El código 121 (`TILE_RENDERING_MODE_CFG`) tiene varios subtipos. Se distinguen por el campo "sub-id", en los bits
  `[11:8]` (los cuatro bits bajos del byte 1). Todos miden 9 bytes. [`v3d_decoder.c`, `v3d_spec_find_instruction`]

### 2.2. Campos

- En el XML, la posición de un campo se cuenta desde el primer bit **después** del código. El generador le suma 8.
  En este documento las posiciones ya llevan esa suma: son bits del paquete entero.
- El bit `n` del paquete es el bit `n mod 8` del byte `n / 8`. Bit 0 es el menos significativo del byte.
- Un campo que cruza bytes va en **little-endian**: sus bits bajos en el byte de menor dirección. Es como escribir
  el paquete entero como un número little-endian de `8 × longitud` bits y poner cada campo con `valor << inicio`.
- Los bits sin campo van a 0. El generador escribe 0 en los bytes que no tienen ningún campo.
- Los campos sin valor explícito valen 0. Mesa rellena una estructura con todo a 0 salvo lo que pone a mano.
- **Campos "menos uno"** (`minus_one` en el XML): se escribe el valor menos 1. Mesa comprueba que el valor sea al
  menos 1. En las tablas se marcan con "−1".
- **Booleanos**: un bit.
- **Enumerados y enteros sin signo**: el número tal cual.
- **Coma flotante** (`float`): IEEE 754 de 32 bits, en little-endian. Ocupa 4 bytes enteros.

### 2.3. Direcciones

- Una dirección es una **VA de la GPU**. Mesa la calcula como la VA del búfer más el desplazamiento dentro de él.
  [`v3d_cl.h`, `__gen_address_offset`]
- Un campo de dirección de 32 bits ocupa 4 bytes enteros, en little-endian.
- Un campo de dirección más estrecho ocupa los bits **altos** de la VA. Ejemplo: `address` de
  `MULTICORE_RENDERING_TILE_LIST_SET_BASE` mide 26 bits y empieza en el bit 14. Guarda los bits 31 a 6 de la VA.
  Mesa escribe la VA entera desde el byte 1, con una OR sobre los campos bajos de ese byte. Por eso los bits bajos de
  la VA tienen que ser 0: **la VA tiene que estar alineada a 2^(32 − tamaño)**, aquí 64 bytes.
  [`gen_pack_header.py`, `emit_pack_function`, caso `address`; `v3d_cl.h`, `__unpack_address`]

### 2.4. Paquetes y listas

- El CLE ejecuta la RCL desde `CLE_CT1QBA` hasta `CLE_CT1QEA` (`v3d.md`, 7.4). Mesa pone en `CLE_CT1QEA` la dirección
  del primer byte **después** del último paquete. [`v3d_job.c`, `v3d_job_submit`, `rcl_end`]
- La RCL acaba con `END_OF_RENDERING`. [`v3dx_rcl.c`, `v3dX(emit_rcl)`]
- La lista genérica es una sublista. Se da con su inicio y su fin, y acaba con `RETURN_FROM_SUB_LIST`.
- El código 0 es `HALT` y el 1 es `NOP`. Se deduce que una memoria a cero, leída como lista, es una serie de `HALT`:
  el CLE se pararía ahí.

### 2.5. Orden de bytes en DashOS

- La GPU lee las listas en little-endian (`v3d.md`, 13.2). La CPU de DashOS es big-endian.
- Lo más sencillo es construir las listas **byte a byte**, como en los volcados de la sección 8. Los campos de varios
  bytes (anchuras, direcciones, el color, el flotante de Z) se escriben con sus bytes en orden little-endian.
- Si se escriben palabras de 32 bits, hay que invertir sus bytes (`to_le`). Ojo: los paquetes no están alineados, así
  que una palabra de un campo casi nunca cae en una dirección múltiplo de 4.

## 3. Los paquetes

### 3.1. Resumen

| Código | Hex | Nombre en Mesa | Bytes | Lista | Variante |
|---|---|---|---|---|---|
| 121, sub-id 0 | `0x79` | `TILE_RENDERING_MODE_CFG_COMMON` | 9 | RCL | A y B |
| 121, sub-id 1 | `0x79` | `TILE_RENDERING_MODE_CFG_COLOR` | 9 | RCL | A y B |
| 121, sub-id 2 | `0x79` | `TILE_RENDERING_MODE_CFG_ZS_CLEAR_VALUES` | 9 | RCL | A y B |
| 121, sub-id 3 | `0x79` | `TILE_RENDERING_MODE_CFG_CLEAR_COLORS_PART1` | 9 | RCL | A y B |
| 121, sub-id 4 | `0x79` | `TILE_RENDERING_MODE_CFG_CLEAR_COLORS_PART2` | 9 | RCL | No hace falta (solo 64 y 128 bpp) |
| 121, sub-id 5 | `0x79` | `TILE_RENDERING_MODE_CFG_CLEAR_COLORS_PART3` | 9 | RCL | No hace falta (128 bpp o UIF) |
| 126 | `0x7E` | `TILE_LIST_INITIAL_BLOCK_SIZE` | 2 | RCL | A y B |
| 123 | `0x7B` | `MULTICORE_RENDERING_TILE_LIST_SET_BASE` | 5 | RCL | A y B |
| 122 | `0x7A` | `MULTICORE_RENDERING_SUPERTILE_CFG` | 9 | RCL | A y B |
| 124 | `0x7C` | `TILE_COORDINATES` | 4 | RCL | A y B |
| 26 | `0x1A` | `END_OF_LOADS` | 1 | RCL y genérica | A y B |
| 29 | `0x1D` | `STORE_TILE_BUFFER_GENERAL` | 13 | RCL y genérica | A y B |
| 25 | `0x19` | `CLEAR_TILE_BUFFERS` | 2 | RCL | A y B |
| 27 | `0x1B` | `END_OF_TILE_MARKER` | 1 | RCL y genérica | A y B |
| 19 | `0x13` | `FLUSH_VCD_CACHE` | 1 | RCL y BCL | A y B |
| 20 | `0x14` | `START_ADDRESS_OF_GENERIC_TILE_LIST` | 9 | RCL | A y B |
| 23 | `0x17` | `SUPERTILE_COORDINATES` | 3 | RCL | A y B |
| 13 | `0x0D` | `END_OF_RENDERING` | 1 | RCL | A y B |
| 125 | `0x7D` | `TILE_COORDINATES_IMPLICIT` | 1 | Genérica | A y B |
| 18 | `0x12` | `RETURN_FROM_SUB_LIST` | 1 | Genérica | A y B |
| 21 | `0x15` | `BRANCH_TO_IMPLICIT_TILE_LIST` | 2 | Genérica | Solo B |
| 119 | `0x77` | `NUMBER_OF_LAYERS` | 2 | BCL | Solo B |
| 120 | `0x78` | `TILE_BINNING_MODE_CFG` | 9 | BCL | Solo B |
| 6 | `0x06` | `START_TILE_BINNING` | 1 | BCL | Solo B |
| 4 | `0x04` | `FLUSH` | 1 | BCL | Solo B |
| 0 | `0x00` | `HALT` | 1 | — | No se usa |
| 1 | `0x01` | `NOP` | 1 | — | Sirve de relleno si hace falta |

Longitudes: [`gen_pack_header.py`, macro `_length` generada para la versión 42]. El XML marca con `cl="R"` los que
solo valen en la RCL y con `cl="B"` los que solo valen en la BCL.

En la 4.2, `CLEAR_TILE_BUFFERS` (25) tiene campos. En la 7.1 el mismo código es `CLEAR_RENDER_TARGETS`, sin campos.
`TILE_RENDERING_MODE_CFG` con sub-id 1, 3, 4 y 5 cambia de significado en la 7.1. [XML, `max_ver="42"`]

### 3.2. `TILE_RENDERING_MODE_CFG_COMMON` (121, sub-id 0, 9 bytes)

Configuración general del render. **Tiene que ser el primer `TILE_RENDERING_MODE_CFG`.** [`v3dx_rcl.c`,
comentario en `v3dX(emit_rcl)`]

| Bits | Campo | Significado | Valor aquí |
|---|---|---|---|
| `[7:0]` | código | 121 | `0x79` |
| `[11:8]` | sub-id | 0 | 0 |
| `[15:12]` | Number of Render Targets, −1 | Número de RT | 1 (se escribe 0) |
| `[31:16]` | Image Width (pixels) | Anchura de la imagen. **Sin** −1 | 64 |
| `[47:32]` | Image Height (pixels) | Altura. **Sin** −1 | 64 |
| `[49:48]` | Maximum BPP of all render targets | 0: 32 bpp; 1: 64; 2: 128 | 0 |
| `[50]` | Multisample Mode (4x) | MSAA 4× | 0 |
| `[51]` | Double-buffer in non-ms mode | Doble TLB | 0 |
| `[53]` | Early-Z Test and Update Direction | 0: LT/LE; 1: GT/GE | 0 |
| `[54]` | Early-Z disable | Apagar early-Z | 1 |
| `[58:55]` | Internal Depth Type | 0: `depth_32f`; 1: `depth_24`; 2: `depth_16` | 0 |
| `[59]` | Early Depth/Stencil Clear | Borrado temprano de Z/S | 0 |
| `[71:60]` | Pad | Relleno para UIF | 0 |

- v3dv, para sus operaciones internas, pone early-Z apagado, un RT, el tipo de profundidad `depth_32f` y el bpp del
  formato. [`v3dvx_meta_common.c`, `emit_rcl_prologue`; `v3dX(meta_framebuffer_init)`]
- La imagen mide como mucho 4096 × 4096 en la 4.2. [`v3d_limits.h`, `V3D_MAX_FRAMEBUFFER_SIZE`]
- **En la 4.2 la RCL no lleva el tamaño de baldosa.** Se deduce que el hardware lo calcula con el número de RT, el bpp
  máximo, el MSAA y el doble búfer (sección 4).

### 3.3. `TILE_RENDERING_MODE_CFG_CLEAR_COLORS_PART1` (121, sub-id 3, 9 bytes)

Los 56 bits bajos del color de borrado de un RT.

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x79` |
| `[11:8]` | sub-id | 3 |
| `[15:12]` | Render Target number | 0 |
| `[47:16]` | Clear Color low 32 bits | El color empaquetado (3.9) |
| `[71:48]` | Clear Color next 24 bits | 0 (con 32 bpp no hay más) |

- Mesa lo emite después de `COMMON` y antes de `COLOR`, uno por RT. [`v3dx_rcl.c`, `v3dX(emit_rcl)`]
- Con 64 bpp o más hace falta también `PART2` (sub-id 4: RT en `[15:12]`, "mid-low 32 bits" en `[47:16]`,
  "mid-high 24 bits" en `[71:48]`). Con 128 bpp, también `PART3` (sub-id 5: RT en `[15:12]`, "high 16 bits" en
  `[31:16]`, "Raster Row Stride or Image Height in Pixels" en `[47:32]`, "UIF padded height in UIF blocks" en
  `[60:48]`). Mesa no rellena nunca el campo de paso de `PART3`. [`v3dvx_meta_common.c`, `emit_rcl_prologue`]

### 3.4. `TILE_RENDERING_MODE_CFG_COLOR` (121, sub-id 1, 9 bytes)

Tipo interno de cada RT en el TLB.

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x79` |
| `[11:8]` | sub-id | 1 |
| `[13:12]` | Render Target 0 Internal BPP (0: 32; 1: 64; 2: 128) | 0 |
| `[17:14]` | Render Target 0 Internal Type | 2 (`8`, es decir, unorm de 8 bits) |
| `[19:18]` | Render Target 0 Clamp (0: none; 1: norm; 2: pos; 3: int) | 0 |
| `[21:20]`, `[25:22]`, `[27:26]` | Lo mismo para el RT 1 | 0 |
| `[29:28]`, `[33:30]`, `[35:34]` | Lo mismo para el RT 2 | 0 |
| `[37:36]`, `[41:38]`, `[43:42]` | Lo mismo para el RT 3 | 0 |
| `[71:44]` | Pad | 0 |

- Tipos internos: 0 `8i`; 1 `8ui`; 2 `8`; 4 `16i`; 5 `16ui`; 6 `16f`; 8 `32i`; 9 `32ui`; 10 `32f`. [XML,
  `Internal Type`]
- El formato de salida `rgba8` usa tipo `8` y 32 bpp. [`v3dvx_formats.c`,
  `v3dX(get_internal_type_bpp_for_output_format)`]
- v3dv pone siempre "clamp" 0 en sus operaciones internas. [`v3dvx_meta_common.c`, `emit_rcl_prologue`]

### 3.5. `TILE_RENDERING_MODE_CFG_ZS_CLEAR_VALUES` (121, sub-id 2, 9 bytes)

Valores de borrado de profundidad y plantilla. **Cierra la configuración: tiene que ser el último
`TILE_RENDERING_MODE_CFG`.** [`v3dx_rcl.c`, comentarios en `v3dX(emit_rcl)`]

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x79` |
| `[11:8]` | sub-id | 2 |
| `[23:16]` | Stencil Clear Value | 0 |
| `[55:24]` | Z Clear Value (flotante de 32 bits) | 1.0 (`0x3F800000`) |
| `[71:56]` | unused | 0 |

v3dv usa Z = 1.0 y plantilla = 0 cuando no hay valor de borrado. [`v3dvx_meta_common.c`, `emit_rcl_prologue`]

### 3.6. `TILE_LIST_INITIAL_BLOCK_SIZE` (126, 2 bytes)

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x7E` |
| `[9:8]` | Size of first block in chained tile lists (0: 64 B; 1: 128 B; 2: 256 B) | 1 |
| `[10]` | Use auto-chained tile lists | 1 |

- Mesa lo pone siempre antes del primer salto. El tamaño **tiene que coincidir** con el del binning
  (`TILE_BINNING_MODE_CFG`). [`v3dx_rcl.c`, comentario en `v3dX(emit_rcl)`]
- Mesa usa bloques iniciales de 128 bytes. [`v3d_limits.h`, `V3D_TILE_ALLOC_INITIAL_BLOCK_SIZE` y `_ENUM`]
- En la variante A no hay salto. Se mantiene igual para no apartarse de Mesa.

### 3.7. `MULTICORE_RENDERING_TILE_LIST_SET_BASE` (123, 5 bytes)

Dirección de la memoria de baldosas, donde el binner dejó las listas.

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x7B` |
| `[11:8]` | Tile List Set Number | 0 |
| `[39:14]` | address: bits 31 a 6 de la VA. **VA alineada a 64 bytes** (2.3) | VA de la memoria de baldosas |

- Mesa le da la VA del búfer de baldosas más `capa × baldosas_x × baldosas_y × 128`. [`v3dx_rcl.c`,
  `emit_render_layer`; `v3dvx_meta_common.c`, `emit_frame_setup`]
- Se deduce que cada baldosa tiene su bloque inicial de 128 bytes, en orden, desde esa base.
- En la variante A se deduce que el hardware no lee esta memoria, porque nada salta a ella. Aun así se le da una
  página válida llena de ceros, para que un acceso no provoque un fallo de la MMU.

### 3.8. `MULTICORE_RENDERING_SUPERTILE_CFG` (122, 9 bytes)

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x7A` |
| `[15:8]` | Supertile Width in Tiles, −1 | 1 (se escribe 0) |
| `[23:16]` | Supertile Height in Tiles, −1 | 1 (se escribe 0) |
| `[31:24]` | Total Frame Width in Supertiles | 1 |
| `[39:32]` | Total Frame Height in Supertiles | 1 |
| `[51:40]` | Total Frame Width in Tiles | 1 |
| `[63:52]` | Total Frame Height in Tiles | 1 |
| `[64]` | Multicore Enable | 0 |
| `[68]` | Supertile Raster Order | 0 |
| `[71:69]` | Number of Bin Tile Lists, −1 | 1 (se escribe 0) |

Mesa pone siempre una lista y deja a 0 los dos bits de multinúcleo. [`v3dx_rcl.c`, `emit_render_layer`] El cálculo de
supertiles está en la sección 4.

### 3.9. Color de borrado para `rgba8`

- Con tipo interno `8`, Mesa empaqueta el color como `R8G8B8A8_UNORM` y copia la primera palabra: `R` en los bits 7
  a 0, `G` en 15 a 8, `B` en 23 a 16 y `A` en 31 a 24. [`v3dvx_device.c`, `v3dX(get_hw_clear_color)`;
  `v3dx_draw.c`, borrado de gallium; `u_pack_color.h`, `util_pack_color`, rama por defecto]
- Esa palabra va en "Clear Color low 32 bits" de `PART1`. En el paquete queda en little-endian, así que sus bytes
  son `R`, `G`, `B`, `A` en ese orden. Son los mismos bytes que tendrá cada píxel en la memoria (sección 9).
- El hardware **no** limita ("clamp") el color de borrado. Mesa lo limita antes. [`v3dx_draw.c`, comentario del
  borrado] Con valores de 8 bits ya en rango no importa.

### 3.10. `TILE_COORDINATES` (124, 4 bytes)

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x7C` |
| `[19:8]` | tile column number | 0 |
| `[31:20]` | tile row number | 0 |

Fija una baldosa a mano. Mesa lo usa solo en las baldosas de preparación (sección 5.2), siempre con (0, 0).

### 3.11. `END_OF_LOADS` (26, 1 byte), `END_OF_TILE_MARKER` (27, 1 byte)

- `END_OF_LOADS` (`0x1A`) separa las cargas de lo demás dentro de una baldosa. Mesa lo pone aunque no haya cargas.
  El comentario de Mesa dice que en la 4.x basta un `TILE_COORDINATES` y que `END_OF_LOADS` cambia de cargar a
  dibujar. [`v3dx_rcl.c`, `v3d_rcl_emit_generic_per_tile_list`]
- `END_OF_TILE_MARKER` (`0x1B`) cierra la baldosa.

### 3.12. `STORE_TILE_BUFFER_GENERAL` (29, 13 bytes)

Guarda un búfer del TLB en la memoria.

| Bits | Campo | Significado | Preparación | Guardado real |
|---|---|---|---|---|
| `[7:0]` | código | 29 | `0x1D` | `0x1D` |
| `[11:8]` | Buffer to Store | 0 a 3: RT 0 a 3; 8: ninguno; 9: Z; 10: plantilla; 11: Z y plantilla | 8 | 0 |
| `[14:12]` | Memory Format | 0: raster; 1: lineartile; 2: UB-linear de 1 bloque; 3: de 2; 4: UIF sin XOR; 5: UIF con XOR | 0 | 0 |
| `[15]` | Flip Y | Volteo vertical | 0 | 0 |
| `[17:16]` | Dither Mode | 0: ninguno; 1: RGB; 2: A; 3: RGBA | 0 | 0 |
| `[19:18]` | Decimate mode | 0: muestra 0; 1: 4×; 3: todas | 0 | 0 |
| `[25:20]` | Output Image Format | 27: `rgba8` (lista completa en el XML) | 0 | 27 |
| `[26]` | Clear buffer being stored | Borrar al guardar | 0 | 0 |
| `[27]` | Channel Reverse | Invertir canales | 0 | 0 |
| `[28]` | R/B swap | Cambiar R y B | 0 | 0 |
| `[55:36]` | Height in UB or Stride | Raster: **paso de fila en bytes**. UIF: altura en bloques | 0 | 256 |
| `[71:56]` | Height | El XML dice "para el volteo" | 0 | 0 |
| `[103:72]` | Address | VA del píxel (0, 0) de la imagen | 0 | VA del destino |

- v3dv guarda en un búfer lineal con: RT 0, raster, paso = anchura × 4, decimado 0, sin borrar, y formato
  `rgba8ui`. [`v3dvx_meta_common.c`, `emit_linear_store` y `emit_fill_buffer`] Aquí se usa `rgba8` porque el tipo
  interno es `8`; con `8ui` sería `rgba8ui` (34). Para el borrado da igual.
- El nombre del formato da los canales de los bits bajos a los altos. `rgba8` deja `R` en el byte 0 de cada píxel.
  [XML, comentario de `Output Image Format`]
- Si se quiere `B`, `G`, `R`, `A` en la memoria, se pone `R/B swap` = 1. Es lo que hace v3dv para
  `B8G8R8A8_UNORM`. [`v3dvx_formats.c`, tabla de formatos; `v3dvx_meta_common.c`, `emit_image_store`] Se deduce.
- El campo de paso mide 20 bits: hasta 1 048 575 bytes.
- **Recorte**: v3dv rellena búferes con imágenes de anchura impar (por ejemplo 25 × 4) usando baldosas de 64 × 64 y
  el paso exacto. Se deduce que el guardado **se recorta a la anchura y altura de la imagen** de `COMMON`, y que no
  escribe fuera. [`v3dvx_meta_common.c`, `framebuffer_size_for_pixel_count` y `meta_fill_buffer`]
- **Alineación del destino**: Mesa usa VA de búferes (4 KB) más desplazamientos múltiplos de 4 (el relleno de Vulkan
  exige múltiplos de 4). No se sabe si el hardware acepta menos. Se recomienda 4 KB.
- Mesa no pide alineación especial del paso en raster, salvo para el simulador. [`v3d_resource.c`, `setup_slices`]

### 3.13. `CLEAR_TILE_BUFFERS` (25, 2 bytes, solo hasta la 4.2)

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x19` |
| `[8]` | Clear all Render Targets | 1 |
| `[9]` | Clear Z/Stencil Buffer | 1 |

- Borra el TLB con los valores de la configuración. v3dv lo pone en la primera baldosa de preparación, con los dos
  bits a 1. [`v3dvx_meta_common.c`, `emit_frame_setup`]
- Erratas GFXH-1461 y GFXH-1689: el bit de borrar de cada guardado no funciona con Z/S, y el bit Z/S de este
  paquete tampoco; el bit de los RT acaba borrando también Z/S. [`v3dx_rcl.c`, comentario en `v3d_rcl_emit_stores`]

### 3.14. `FLUSH_VCD_CACHE` (19, 1 byte)

`0x13`. Vacía la caché de lectura de vértices. Mesa lo pone en la RCL tras las baldosas de preparación y en la BCL
tras la configuración. [`v3dvx_meta_common.c`, `emit_frame_setup`; `v3dvx_cmd_buffer.c`, `job_emit_binning_prolog`]
El kernel no toca esa caché (`v3d.md`, 8.1). Para un borrado no hace falta, pero se mantiene.

### 3.15. `START_ADDRESS_OF_GENERIC_TILE_LIST` (20, 9 bytes)

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x14` |
| `[39:8]` | start | VA del primer paquete de la lista genérica |
| `[71:40]` | end | VA del byte siguiente al `RETURN_FROM_SUB_LIST` |

Mesa da como fin la posición de escritura tras el último paquete. [`v3dvx_meta_common.c`,
`emit_fill_buffer_per_tile_list`] El volcador de Mesa recorre la sublista hasta llegar a ese fin. [`clif_dump.c`,
`clif_dump_cl`]

### 3.16. `SUPERTILE_COORDINATES` (23, 3 bytes)

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x17` |
| `[15:8]` | column number in supertiles | 0 |
| `[23:16]` | row number in supertiles | 0 |

Uno por supertile que haya que dibujar, después de `START_ADDRESS_OF_GENERIC_TILE_LIST`. Mesa los emite por filas, de
izquierda a derecha. [`v3dvx_meta_common.c`, `emit_supertile_coordinates`] Se deduce que cada uno hace que el
hardware ejecute la lista genérica para cada baldosa del supertile.

### 3.17. Paquetes sin campos

| Código | Nombre | Uso |
|---|---|---|
| 13 (`0x0D`) | `END_OF_RENDERING` | Último paquete de la RCL |
| 125 (`0x7D`) | `TILE_COORDINATES_IMPLICIT` | Primer paquete de la lista genérica: la baldosa la pone el hardware |
| 18 (`0x12`) | `RETURN_FROM_SUB_LIST` | Último paquete de la lista genérica |
| 6 (`0x06`) | `START_TILE_BINNING` | BCL. El comentario de Mesa cita el manual: tiene que ir tras el estado previo y antes de la lista |
| 4 (`0x04`) | `FLUSH` | Último paquete de la BCL |

### 3.18. `BRANCH_TO_IMPLICIT_TILE_LIST` (21, 2 bytes, solo variante B)

| Bits | Campo | Valor |
|---|---|---|
| `[7:0]` | código | `0x15` |
| `[15:8]` | tile list set number | 0 |

Salta a la lista que el binner escribió para la baldosa actual. v3dv lo pone tras `END_OF_LOADS` y antes del guardado.
[`v3dvx_meta_common.c`, `emit_fill_buffer_per_tile_list`]

### 3.19. Paquetes del binning (solo variante B)

`NUMBER_OF_LAYERS` (119, 2 bytes): `[15:8]` número de capas, −1. Mesa lo pone antes de la configuración y dice que
hace falta para las imágenes con capas. [`v3dvx_cmd_buffer.c`, `job_emit_binning_prolog`]

`TILE_BINNING_MODE_CFG` (120, 9 bytes, versión de la 4.2):

| Bits | Campo | Valor aquí |
|---|---|---|
| `[7:0]` | código | `0x78` |
| `[11:10]` | tile allocation initial block size (0: 64 B; 1: 128 B; 2: 256 B) | 1 |
| `[13:12]` | tile allocation block size (ídem) | 0 (64 B) |
| `[19:16]` | Number of Render Targets, −1 | 1 (se escribe 0) |
| `[21:20]` | Maximum BPP of all render targets | 0 |
| `[22]` | Multisample Mode (4x) | 0 |
| `[23]` | Double-buffer in non-ms mode | 0 |
| `[55:40]` | Width (in pixels), −1 | 64 (se escribe 63) |
| `[71:56]` | Height (in pixels), −1 | 64 (se escribe 63) |

- Ojo: aquí la anchura y la altura llevan −1. En `COMMON` no.
- El bloque inicial tiene que coincidir con `TILE_LIST_INITIAL_BLOCK_SIZE`. Mesa usa 128 B iniciales y 64 B de
  continuación. [`v3d_limits.h`]

## 4. Tamaño de baldosa y supertiles

### 4.1. Baldosa

[`v3d_util.c`, `v3d_choose_tile_size`, rama de la 4.x]

Tabla de tamaños, por índice: 0: 64 × 64; 1: 64 × 32; 2: 32 × 32; 3: 32 × 16; 4: 16 × 16; 5: 16 × 8; 6: 8 × 8.

El índice parte de 0 y se le suma:

- 1 si hay 2 RT; 2 si hay 3 o 4.
- 2 con MSAA; 1 con doble búfer (son incompatibles).
- El bpp máximo como enumerado (0, 1 o 2).

**Un RT de 32 bpp, sin MSAA ni doble búfer: 64 × 64.** Una imagen de 64 × 64 es una sola baldosa.

### 4.2. Supertiles

[`v3dx_rcl.c`, `emit_render_layer`; `v3dv_cmd_buffer.c`, `job_compute_frame_tiling`]

1. Baldosas: `bx = ceil(anchura / 64)`, `by = ceil(altura / 64)`.
2. Empezar con supertiles de 1 × 1 baldosas.
3. Calcular `sx = ceil(bx / ancho_st)` y `sy = ceil(by / alto_st)`.
4. Si `sx < 256`, `sy < 256` y `sx × sy ≤ 256`, terminar.
5. Si no, agrandar el supertile: el ancho si es menor que el alto; si no, el alto. Volver a 3.

El comentario de Mesa dice que el hardware admite hasta 256 supertiles, pero no 1 × 256 ni 256 × 1.
[`v3d_limits.h`, `V3D_MAX_SUPERTILES`] Para 64 × 64: una baldosa, un supertile de 1 × 1.

## 5. La secuencia de la variante A

### 5.1. RCL

| Paso | Paquete | Por qué |
|---|---|---|
| 1 | `TILE_RENDERING_MODE_CFG_COMMON` | Configuración. Primero de los 121 |
| 2 | `TILE_RENDERING_MODE_CFG_CLEAR_COLORS_PART1` (RT 0) | Color de borrado |
| 3 | `TILE_RENDERING_MODE_CFG_COLOR` | Tipo interno del RT 0 |
| 4 | `TILE_RENDERING_MODE_CFG_ZS_CLEAR_VALUES` | Cierra la configuración |
| 5 | `TILE_LIST_INITIAL_BLOCK_SIZE` | Como Mesa |
| 6 | `MULTICORE_RENDERING_TILE_LIST_SET_BASE` | Como Mesa; apunta a una página a cero |
| 7 | `MULTICORE_RENDERING_SUPERTILE_CFG` | Baldosas y supertiles |
| 8 | `TILE_COORDINATES` (0, 0) | Baldosa de preparación 1 |
| 9 | `END_OF_LOADS` | |
| 10 | `STORE_TILE_BUFFER_GENERAL`, búfer 8 (ninguno) | Guardado de relleno |
| 11 | `CLEAR_TILE_BUFFERS`, los dos bits | Aquí se borra el TLB |
| 12 | `END_OF_TILE_MARKER` | |
| 13 | `TILE_COORDINATES` (0, 0) | Baldosa de preparación 2 |
| 14 | `END_OF_LOADS` | |
| 15 | `STORE_TILE_BUFFER_GENERAL`, búfer 8 | Guardado de relleno |
| 16 | `END_OF_TILE_MARKER` | Sin borrado (no hay doble búfer) |
| 17 | `FLUSH_VCD_CACHE` | Como Mesa |
| 18 | `START_ADDRESS_OF_GENERIC_TILE_LIST` | Inicio y fin de la lista genérica |
| 19 | `SUPERTILE_COORDINATES`, uno por supertile | Aquí se dibujan las baldosas de verdad |
| 20 | `END_OF_RENDERING` | Fin |

Fuentes: pasos 1 a 5 [`v3dvx_meta_common.c`, `emit_rcl_prologue`]; 6 a 17 [`emit_frame_setup`]; 18
[`emit_fill_buffer_per_tile_list`]; 19 [`emit_supertile_coordinates`]; 20 [`meta_emit_fill_buffer_rcl`]. Gallium hace lo mismo. [`v3dx_rcl.c`,
`v3dX(emit_rcl)` y `emit_render_layer`]

- **Las dos baldosas de preparación** son el arreglo de la errata **GFXH-1742**. El comentario de Mesa dice que hay
  una carrera entre el cambio de tipo y tamaño internos del TLB y el arranque de los QPU. Hacen falta dos guardados
  de relleno en la 4.x entre cambios de tipo. Además, la primera borra el TLB, así ninguna baldosa hereda contenido
  viejo. [`v3dx_rcl.c`, comentario en `emit_render_layer`]
- El borrado con doble búfer se repite en la segunda baldosa. Aquí no hay doble búfer. [`v3dv_pass.h`,
  `v3dv_do_double_initial_tile_clear`]

### 5.2. Lista genérica de baldosa

| Paso | Paquete |
|---|---|
| 1 | `TILE_COORDINATES_IMPLICIT` |
| 2 | `END_OF_LOADS` |
| — | (variante B: `BRANCH_TO_IMPLICIT_TILE_LIST`) |
| 3 | `STORE_TILE_BUFFER_GENERAL`, RT 0, raster, `rgba8`, paso, destino |
| 4 | `END_OF_TILE_MARKER` |
| 5 | `RETURN_FROM_SUB_LIST` |

[`v3dvx_meta_common.c`, `emit_fill_buffer_per_tile_list`]. Gallium añade `PRIM_LIST_FORMAT` y `SET_INSTANCEID`
antes del salto, porque el binner empieza suponiendo triángulos. Para un borrado no hace falta. [`v3dx_rcl.c`,
`v3d_rcl_emit_generic_per_tile_list`]

La lista genérica puede ir en otro búfer o en el mismo que la RCL. Mesa la pone en otro (`indirect`).

## 6. Memoria necesaria

| Búfer | Variante | Tamaño | Alineación | Contenido | Quién lo da a la GPU |
|---|---|---|---|---|---|
| RCL | A y B | 106 bytes para una baldosa (sección 8) | Ninguna conocida; usar 4 KB | Paquetes | `CLE_CT1QBA`, `CLE_CT1QEA` |
| Lista genérica | A y B | 17 bytes (A) o 19 (B) | Ninguna (Mesa usa 1) | Paquetes | `START_ADDRESS_OF_GENERIC_TILE_LIST` |
| Memoria de baldosas | A | Una página a cero (se usarían `bx × by × 128` bytes) | 64 bytes (campo de 26 bits) | Ceros; se deduce que no se lee | `MULTICORE_RENDERING_TILE_LIST_SET_BASE` |
| Memoria de baldosas | B | `alinear(bx × by × 128, 4096) + 8192`, redondeado a página; para una baldosa, 12 KB | 64 bytes; Mesa usa la del búfer | La escribe el binner | `CLE_CT0QMA`/`QMS` y `SET_BASE` |
| Estado de baldosas | B | `bx × by × 256` bytes; búfer de 4 KB para una baldosa | La del búfer (4 KB); `v3d.md` 7.2 exige el bit 1 libre | Lo usa el binner | `CLE_CT0QTS` |
| BCL | B | 14 bytes | Ninguna conocida | Paquetes | `CLE_CT0QBA`, `CLE_CT0QEA` |
| Destino | A y B | `paso × altura`; 64 × 64 × 4 = 16 384 bytes | Usar 4 KB (3.12) | Lo escribe el render | `STORE_TILE_BUFFER_GENERAL` |

- El tamaño de la memoria de baldosas sale de [`v3d_util.c`, `v3d_tile_alloc_sizes`], sin dibujos (`draws` = 0). El
  comentario dice que los 8 KB de más evitan que el hardware pida memoria en las primeras reservas.
- **El render no usa el estado de baldosas.** Solo se da al binning (`CLE_CT0QTS`, `v3d.md` 7.2). En la variante A
  no hace falta.
- Todos los búferes tienen que estar mapeados en la MMU de la GPU. La página 0 no se usa nunca (`v3d.md`, 6).
- Coherencia: la V3D no ve las cachés de la CPU (`v3d.md`, 2.7). La CPU tiene que escribir las listas en memoria no
  cacheable, o limpiar su caché antes de lanzar. Antes de leer el destino, la CPU tiene que invalidar su caché sobre
  esa zona.

## 7. Variante B: binning vacío

Es lo que hace v3dv para rellenar un búfer. [`v3dvx_meta_common.c`, `meta_fill_buffer`; `v3dvx_cmd_buffer.c`]

1. BCL: `NUMBER_OF_LAYERS` (1), `TILE_BINNING_MODE_CFG`, `FLUSH_VCD_CACHE`, `START_TILE_BINNING`, `FLUSH`.
2. RCL: la de la variante A. La lista genérica lleva `BRANCH_TO_IMPLICIT_TILE_LIST` tras `END_OF_LOADS`. Eso cambia
   el campo `end` de `START_ADDRESS_OF_GENERIC_TILE_LIST`.
3. `SET_BASE` apunta a la misma memoria de baldosas que se da al binning en `CLE_CT0QMA`.
4. Lanzar el binning con `qma`, `qms` y `qts` (`v3d.md`, 7.2). Esperar `FLDONE`.
5. Lanzar el render. Esperar `FRDONE`.

- Se deduce que el binner, sin primitivas, escribe en cada bloque inicial una lista que no hace nada y vuelve.
- Gallium añade a la BCL `OCCLUSION_QUERY_COUNTER` a cero, para apagar consultas viejas. [`v3dx_draw.c`,
  `v3dX(start_binning)`] Sin consultas no hace falta.
- El kernel debería atender `OUTOMEM` aunque no se espere (`v3d.md`, 7.3).

## 8. Ejemplo: 64 × 64, color fijo, destino en `0x00100000`

**Se deduce, sin probar.** Calculado a mano y contrastado con los empaquetadores de Mesa (sección 0).

### 8.1. Datos

| Dato | Valor |
|---|---|
| Imagen | 64 × 64 píxeles; una baldosa de 64 × 64; un supertile |
| Color | `R` = `0x20`, `G` = `0x40`, `B` = `0x80`, `A` = `0xFF` (opaco). Palabra: `0xFF804020` |
| Destino | VA `0x00100000`, 16 384 bytes (4 páginas), paso 256 bytes |
| RCL | VA `0x00200000` |
| Lista genérica | VA `0x00200100` (en la misma página que la RCL) |
| Memoria de baldosas | VA `0x00201000`, una página a cero (A) o 12 KB (B) |
| BCL (solo B) | VA `0x00204000` |
| Estado de baldosas (solo B) | VA `0x00205000`, una página |

### 8.2. Cuentas de los paquetes con campos

- `COMMON`: byte 1 = (1 − 1) << 4 | 0 = `00`. Anchura 64 = `0x0040` → `40 00`. Altura igual. Byte 6 (bits 48 a 55):
  early-Z disable en el bit 54 → bit 6 del byte → `40`. Resto 0.
- `CLEAR_COLORS_PART1`: byte 1 = RT 0 << 4 | 3 = `03`. Bytes 2 a 5: `0xFF804020` en little-endian → `20 40 80 FF`.
- `COLOR`: byte 1 = sub-id 1 | bpp 0 << 4 | tipo 2 << 6 = `0x81`. Byte 2: tipo 2 >> 2 = 0.
- `ZS_CLEAR_VALUES`: byte 1 = `02`. Byte 2: plantilla 0. Bytes 3 a 6: 1.0 = `0x3F800000` → `00 00 80 3F`.
- `TILE_LIST_INITIAL_BLOCK_SIZE`: 1 | 1 << 2 = `05`.
- `SET_BASE`: VA `0x00201000` desde el byte 1 → `00 10 20 00`. Conjunto 0.
- `SUPERTILE_CFG`: bytes 1 y 2: 1 − 1 = 0. Bytes 3 y 4: 1 y 1. Anchura en baldosas 1 en los bits 40 a 51 → byte 5 =
  `01`. Altura en baldosas 1 en los bits 52 a 63 → bit 4 del byte 6 → `10`.
- `CLEAR_TILE_BUFFERS`: bits 8 y 9 → `03`.
- Guardado de relleno: búfer 8 → byte 1 = `08`. Resto 0.
- Guardado real: byte 1 = 0 (RT 0, raster). Formato 27 en los bits 20 a 25: 27 << 4 = `0x1B0` → byte 2 = `B0`, byte
  3 = `01`. Paso 256 en los bits 36 a 55: 256 << 4 = `0x1000` → bytes 4 a 6 = `00 10 00`. Bytes 7 y 8 (altura): 0.
  Bytes 9 a 12: `0x00100000` → `00 00 10 00`.
- `START_ADDRESS_OF_GENERIC_TILE_LIST`: inicio `0x00200100` → `00 01 20 00`. Fin `0x00200111` → `11 01 20 00`.
- `SUPERTILE_COORDINATES`: (0, 0) → `00 00`.

### 8.3. Volcado de la RCL (variante A)

Dirección de la GPU, paquete y bytes en orden de memoria (el de menor dirección primero):

```
00200000  TILE_RENDERING_MODE_CFG_COMMON              79 00 40 00 40 00 40 00 00
00200009  TILE_RENDERING_MODE_CFG_CLEAR_COLORS_PART1  79 03 20 40 80 FF 00 00 00
00200012  TILE_RENDERING_MODE_CFG_COLOR               79 81 00 00 00 00 00 00 00
0020001B  TILE_RENDERING_MODE_CFG_ZS_CLEAR_VALUES     79 02 00 00 00 80 3F 00 00
00200024  TILE_LIST_INITIAL_BLOCK_SIZE                7E 05
00200026  MULTICORE_RENDERING_TILE_LIST_SET_BASE      7B 00 10 20 00
0020002B  MULTICORE_RENDERING_SUPERTILE_CFG           7A 00 00 01 01 01 10 00 00
00200034  TILE_COORDINATES                            7C 00 00 00
00200038  END_OF_LOADS                                1A
00200039  STORE_TILE_BUFFER_GENERAL (ninguno)         1D 08 00 00 00 00 00 00 00 00 00 00 00
00200046  CLEAR_TILE_BUFFERS                          19 03
00200048  END_OF_TILE_MARKER                          1B
00200049  TILE_COORDINATES                            7C 00 00 00
0020004D  END_OF_LOADS                                1A
0020004E  STORE_TILE_BUFFER_GENERAL (ninguno)         1D 08 00 00 00 00 00 00 00 00 00 00 00
0020005B  END_OF_TILE_MARKER                          1B
0020005C  FLUSH_VCD_CACHE                             13
0020005D  START_ADDRESS_OF_GENERIC_TILE_LIST          14 00 01 20 00 11 01 20 00
00200066  SUPERTILE_COORDINATES                       17 00 00
00200069  END_OF_RENDERING                            0D
0020006A  (fin: CLE_CT1QEA = 0x0020006A; 106 bytes)
```

### 8.4. Volcado de la lista genérica (variante A)

```
00200100  TILE_COORDINATES_IMPLICIT                   7D
00200101  END_OF_LOADS                                1A
00200102  STORE_TILE_BUFFER_GENERAL (RT 0)            1D 00 B0 01 00 10 00 00 00 00 00 10 00
0020010F  END_OF_TILE_MARKER                          1B
00200110  RETURN_FROM_SUB_LIST                        12
00200111  (fin; 17 bytes)
```

### 8.5. Registros (variante A)

1. Invalidar cachés (`v3d.md`, 8.2). `lanzar_render` ya lo hace.
2. `CLE_CT1QBA` = `0x00200000`.
3. `CLE_CT1QEA` = `0x0020006A`. Lanza el render.
4. Esperar `FRDONE`.

### 8.6. Variante B: lo que cambia

Lista genérica, con el salto:

```
00200100  TILE_COORDINATES_IMPLICIT                   7D
00200101  END_OF_LOADS                                1A
00200102  BRANCH_TO_IMPLICIT_TILE_LIST                15 00
00200104  STORE_TILE_BUFFER_GENERAL (RT 0)            1D 00 B0 01 00 10 00 00 00 00 00 10 00
00200111  END_OF_TILE_MARKER                          1B
00200112  RETURN_FROM_SUB_LIST                        12
00200113  (fin; 19 bytes)
```

En la RCL solo cambia `START_ADDRESS_OF_GENERIC_TILE_LIST`, en `0x0020005D`:

```
0020005D  START_ADDRESS_OF_GENERIC_TILE_LIST          14 00 01 20 00 13 01 20 00
```

BCL:

```
00204000  NUMBER_OF_LAYERS                            77 00
00204002  TILE_BINNING_MODE_CFG                       78 04 00 00 00 3F 00 3F 00
0020400B  FLUSH_VCD_CACHE                             13
0020400C  START_TILE_BINNING                          06
0020400D  FLUSH                                       04
0020400E  (fin; 14 bytes)
```

Registros: `lanzar_binning(0x00204000, 0x0020400E, qma = 0x00201000, qms = 12288, qts = 0x00205000)`, esperar
`FLDONE`, y luego el render como en 8.5.

## 9. Cómo comprobarlo desde la CPU

1. Antes de lanzar, llenar el destino con un patrón que no se parezca al color, por ejemplo todo `0xA5`. Si el
   búfer es mayor de 16 KB, llenar también lo que sigue.
2. Lanzar y esperar `FRDONE`. La cabecera del kernel dice que, al llegar, lo guardado desde el TLB ya está en la RAM
   (`v3d.md`, 7.4).
3. Invalidar la caché de la CPU sobre el destino, si es cacheable.
4. Comprobar los bytes:
   - Cada píxel ocupa 4 bytes. En la memoria van `20 40 80 FF`: `R`, `G`, `B`, `A`, de menor a mayor dirección.
   - Las filas van seguidas, de arriba abajo, con paso 256. El píxel `(x, y)` está en `0x00100000 + y × 256 + x × 4`.
   - Los 16 384 bytes tienen que repetir ese patrón: 4096 píxeles.
   - Lo que sigue al byte 16 383 tiene que seguir siendo `0xA5`.
5. **Orden de bytes**: la GPU escribe bytes. No hay nada que invertir si se comparan bytes. Si la CPU big-endian lee
   un píxel como palabra de 32 bits, ve `0x204080FF`. Con `u32::from_le` sale `0xFF804020`, el valor de la palabra
   del color de borrado (3.9).
6. Un color con los cuatro bytes distintos, como este, delata tanto un cambio de `R` por `B` (`80 40 20 FF`) como una
   inversión de canales (`FF 80 40 20`).

## 10. Más de una baldosa

Para una imagen de `w × h` (como mucho 4096 × 4096):

- `COMMON`: anchura `w` y altura `h`.
- `SUPERTILE_CFG`: baldosas `bx × by` y supertiles según 4.2.
- Un `SUPERTILE_COORDINATES` por supertile: filas de `0` a `sy − 1`, y dentro de cada fila columnas de `0` a `sx − 1`.
- El guardado: paso `w × 4` y **la misma dirección** de destino, la del píxel (0, 0). Se deduce que el hardware suma
  la posición de cada baldosa.
- Variante A: la memoria de baldosas a cero puede seguir siendo una página si `bx × by × 128 ≤ 4096`. Si no, se
  agranda, aunque se deduce que no se lee.
- Variante B: memoria de baldosas y estado de baldosas según la sección 6. `TILE_BINNING_MODE_CFG` con `w − 1` y
  `h − 1`.
- Ejemplo para 128 × 128: `SUPERTILE_CFG` = `7A 00 00 02 02 02 20 00 00`, y cuatro `SUPERTILE_COORDINATES`:
  `17 00 00`, `17 01 00`, `17 00 01`, `17 01 01`. Paso 512 → bytes 4 a 6 del guardado = `00 20 00`.
- Una prueba con varias baldosas comprueba lo que una sola no puede: el desplazamiento por baldosa y el recorte.

## 11. Si no funciona

| Síntoma | Qué mirar |
|---|---|
| No llega `FRDONE` | `CLE_CT1CA` y `CLE_CT1RA` dicen dónde se paró el CLE (`v3d.md`, 4.2). Si está dentro de la lista genérica o de la memoria de baldosas, probar la variante B |
| Error de la MMU | La VA que falla (`v3d.md`, 5.7). Revisar que todos los búferes estén mapeados, también la memoria de baldosas |
| Llega `FRDONE` pero el destino no cambia | Coherencia de cachés (sección 6); el campo de dirección del guardado; que el búfer guardado sea el 0 y no el 8 |
| Colores cambiados | `R/B swap` y `Channel Reverse` (3.12); el empaquetado del color (3.9) |
| Solo cambia parte del destino | Paso, anchura y altura en `COMMON`; el número de baldosas y supertiles |

Si la variante B tampoco termina, un paso más simple para diagnosticar: quitar la lista genérica y los supertiles y
poner el guardado real (RT 0 al destino) en la primera baldosa de preparación, con `TILE_COORDINATES` (0, 0). Para
una sola baldosa el desplazamiento es 0. Mesa nunca guarda así un RT: se deduce que funciona porque es la misma forma
que las baldosas de preparación.

## Lo que no está claro

1. **Render sin binning.** Mesa no lo hace nunca. Que la variante A funcione se deduce de la cabecera del kernel y de
   que la lista genérica sin salto no lee la memoria de baldosas. No está escrito en Mesa.
2. **Si `TILE_LIST_INITIAL_BLOCK_SIZE` y `SET_BASE` hacen falta sin binning.** Se mantienen por prudencia. No se
   sabe si el hardware lee algo de la memoria de baldosas sin el salto.
3. **Qué escribe el binner en una BCL vacía.** Se deduce que una lista vacía por baldosa. Mesa confía en ello sin
   comentarlo.
4. **Longitud por código en el CLE.** El decodificador de Mesa la saca de la tabla. Que el hardware haga lo mismo se
   deduce de que los paquetes no llevan longitud.
5. **Recorte del guardado.** Que el guardado no escriba fuera de `anchura × altura` se deduce del relleno de búferes
   de v3dv. Con 64 × 64 no se pone a prueba.
6. **Desplazamiento por baldosa.** Que el hardware sume la posición de cada baldosa a la dirección del guardado se
   deduce de que la lista genérica es una para todas. Con una sola baldosa no se pone a prueba.
7. **Alineación de las listas y del destino.** No hay alineación escrita para la RCL ni para el destino. Mesa usa
   búferes de 4 KB. La lista genérica va sin alinear. Solo `SET_BASE` exige 64 bytes, por el tamaño del campo.
8. **Campo `Height` del guardado** y campo de paso de `CLEAR_COLORS_PART3`. Mesa no los rellena. El XML solo dice que
   el primero es para el volteo vertical.
9. **`FLUSH_VCD_CACHE` en la RCL.** Mesa lo pone. No se sabe si en un render sin dibujos sirve de algo.
10. **`CLEAR_TILE_BUFFERS` con el bit de Z/S.** Las erratas citadas en 3.13 dicen que el bit Z/S no funciona y que el
    de los RT borra también Z/S. Mesa pone los dos a 1. No se sabe si importa.
11. **Si el CLE lee las listas a través de la L2T.** El kernel invalida la L2T antes de cada render, así que no debería
    importar. No está escrito.
12. **`R/B swap` para `B`, `G`, `R`, `A`.** Se deduce de la tabla de formatos de v3dv. Hará falta para la pantalla,
    según el formato que pida el HVS.
13. **QEMU.** No emula la V3D (`v3d.md`, "Lo que no está claro", 19). Esto solo se puede probar en una Pi 4.
