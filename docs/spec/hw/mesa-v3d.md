# Mesa sobre la V3D 4.2: lo que pide al kernel

Especificación de sala limpia. La escribe el analista leyendo Mesa (controladores v3d y v3dv) y libdrm. Describe la
interfaz que Mesa usa con el kernel, para que DashOS la ofrezca y Mesa se porte sin tocar sus controladores. Quien
implemente no necesita leer Mesa: todo lo necesario tiene que estar aquí.

Este documento no repite el hardware. El hardware de la GPU está en [`v3d.md`](v3d.md). La pantalla está en
[`hvs.md`](hvs.md) y [`hdmi.md`](hdmi.md).

## 0. Origen y convenciones

- **Fuente**: `mesa/mesa` de freedesktop, commit `3f389de2337e3ef7f5a4c6478a5985d9dd94479d` (3 de noviembre de 2026,
  versión `27.0.0-devel`). Carpetas leídas: `src/gallium/drivers/v3d/`, `src/broadcom/vulkan/`,
  `src/broadcom/common/`, `src/broadcom/cle/`, `src/broadcom/perfcntrs/`, `src/broadcom/drm-shim/`,
  `src/gallium/winsys/v3d/`, `src/gallium/winsys/vc4/`, `src/gallium/winsys/kmsro/`,
  `src/gallium/auxiliary/renderonly/`, `src/gallium/auxiliary/pipe-loader/`, `src/egl/drivers/dri2/`,
  `src/egl/main/`, `src/loader/`, `src/gbm/`, `src/vulkan/runtime/`, `src/vulkan/wsi/`, `src/util/` y
  `include/drm-uapi/` (`v3d_drm.h`, `drm.h`, `drm_mode.h`, `drm_fourcc.h`, `vc4_drm.h`).
- **libdrm**: `mesa/drm` de freedesktop, commit `33fab8cbca7f7581e955a675597d14412702edc4` (27 de octubre de 2026).
  Ficheros `xf86drm.c`, `xf86drm.h` y `xf86drmMode.c`.
- **Citas**: cada dato lleva entre corchetes el fichero y la función de Mesa o libdrm de donde sale. Por ejemplo,
  [`v3d_bufmgr.c`, `v3d_bo_alloc`]. Las rutas se acortan cuando no hay duda.
- **Comprobado y deducido**: "Mesa hace X" quiere decir que se leyó en el código. "Se deduce" es una conclusión del
  analista. Nada de esto se ha ejecutado: ni se ha compilado Mesa ni se ha probado en una Pi 4.
- **Tamaños y desplazamientos**: se comprobaron compilando las cabeceras con `clang` para `aarch64_be-unknown-linux-gnu`
  (aserciones de tamaño) y con `gcc` en x86-64 (desplazamientos). Coinciden en las dos.
- **Notación**: "BO" es un objeto de búfer de la GPU. "VA" es una dirección virtual de la GPU, de 32 bits (ver
  `v3d.md`, sección 6). "Manija" es el número que da el kernel para nombrar un objeto dentro de un fichero abierto.
  "Llamada" es un `ioctl`.
- **Nombres**: se usan los de Mesa y de la cabecera del kernel (`DRM_IOCTL_V3D_CREATE_BO`, `drm_v3d_submit_cl`...),
  para poder contrastarlos.

## 1. Visión de conjunto

En Linux, la Pi 4 tiene dos dispositivos DRM:

- **v3d**: la GPU. Tiene nodo de render (`/dev/dri/renderD128`) y nodo primario (`/dev/dri/cardN`). Solo dibuja.
- **vc4**: la pantalla (HVS, pixel valves, HDMI). Solo tiene nodo primario. En la Pi 4 no dibuja: es "KMS-only".

Las piezas de Mesa que tocan el kernel son estas:

| Pieza | Qué es | Con qué dispositivo habla |
|---|---|---|
| `gallium/drivers/v3d` | Controlador de OpenGL ES y OpenGL | v3d |
| `broadcom/vulkan` (v3dv) | Controlador de Vulkan | v3d, y vc4 para los búferes de la ventana |
| `broadcom/common` | Código común: identificación, envíos de CPU, extensiones | v3d |
| `gallium/winsys/kmsro` y `auxiliary/renderonly` | Une una pantalla sin GPU con una GPU sin pantalla | vc4 y v3d |
| `gallium/winsys/vc4` | Decide si vc4 dibuja o hay que usar kmsro | vc4 |
| `egl/drivers/dri2`, `gbm`, `loader` | Buscan el dispositivo y hablan con Wayland o KMS | Los dos |
| `vulkan/runtime`, `vulkan/wsi` | Sincronía genérica de Vulkan y presentación | v3d y vc4 |

- Todas las llamadas de v3d y v3dv pasan por una función, `v3d_ioctl`, que llama a `drmIoctl` de libdrm. Con el
  simulador llama a otra cosa. [`broadcom/common/v3d_util.h`, `v3d_ioctl`]
- `drmIoctl` llama a `ioctl` y repite mientras el error sea `EINTR` o `EAGAIN`. [libdrm, `xf86drm.c`, `drmIoctl`]
- El resto de llamadas pasa por funciones de libdrm (`drmPrimeHandleToFD`, `drmSyncobjWait`...). Hay tres llamadas
  `ioctl` directas, sin libdrm: `DRM_IOCTL_VC4_GET_PARAM` [`vc4_drm_winsys.c`, `vc4_drm_screen_create`],
  `SYNC_IOC_MERGE` y `SYNC_IOC_FILE_INFO` [`util/libsync.h`].
- Se deduce que **sustituir libdrm y unas pocas llamadas de relibc basta para portar Mesa**. Los controladores no se
  tocan (salvo por el orden de bytes, sección 10).

## 2. Cómo encuentra Mesa los dispositivos

### 2.1. La enumeración de libdrm

Todas las vías de Mesa empiezan enumerando dispositivos con libdrm:

| Quién | Cómo busca | Fuente |
|---|---|---|
| EGL, lista de dispositivos | `drmGetDevices2`; solo guarda los que tienen nodo de render | [`egl/main/egldevice.c`, `_eglDeviceRefreshList` y `_eglAddDRMDevice`] |
| EGL sin ventanas ("surfaceless") | Recorre esa lista y abre el nodo de render | [`platform_surfaceless.c`, `surfaceless_probe_device`] |
| EGL en Wayland | Recibe un `dev_t` del compositor y busca su nodo de render con `drmGetDeviceFromDevId` | [`platform_wayland.c`, `dri2_wl_init_display_dev`; `loader.c`, `loader_get_render_node`] |
| kmsro (GBM sobre vc4) | `drmGetDevices2`; abre los nodos de render de bus `PLATFORM` cuyo nombre esté en una lista (incluye `"v3d"`) | [`loader.c`, `loader_open_render_node_platform_devices`; `pipe_loader_drm.c`, `pipe_loader_get_compatible_render_capable_device_fds`] |
| v3dv | `drmGetDevices2` (máximo 8); solo bus `PLATFORM` | [`v3dv_device.c`, `enumerate_devices`] |

Lo que libdrm hace en Linux para enumerar [libdrm, `xf86drm.c`, `drmGetDevices2`, `drmGetNodeTypeFromFd`,
`drmNodeIsDRM`, `drmParseSubsystemType` y las funciones de bus de plataforma]:

- Lee el directorio `/dev/dri`. Los nodos se llaman `cardN` (primario) y `renderDN` (render).
- Hace `stat` de cada nodo. Tiene que ser de caracteres, de número mayor 226.
- Comprueba que existe `/sys/dev/char/226:M/device/drm`.
- Sigue el enlace `/sys/dev/char/226:M/device/subsystem`. Si acaba en `platform`, el bus es `DRM_BUS_PLATFORM`.
- Para plataforma lee `/sys/dev/char/226:M/device/uevent`: las claves `OF_FULLNAME`, `OF_COMPATIBLE_N` y
  `OF_COMPATIBLE_0`...
- Agrupa los nodos del mismo dispositivo en un `drmDevice` con `available_nodes` y `nodes[]`.

El shim de pruebas de Mesa finge exactamente eso para la v3d: un nodo `renderD128`, el enlace a `bus/platform`, y un
`uevent` con `DRIVER=v3d`, `OF_FULLNAME=/rdb/v3d`, `OF_COMPATIBLE_0=brcm,2711-v3d` y `OF_COMPATIBLE_N=1`.
[`drm-shim/drm_shim.c`, `drm_shim_platform_device_setup`; `broadcom/drm-shim/v3d_noop.c`]

### 2.2. Lo que se mira de cada dispositivo

| Comprobación | Llamada | Qué espera Mesa | Fuente |
|---|---|---|---|
| Nombre del controlador | `DRM_IOCTL_VERSION` (`drmGetVersion`) | `"v3d"` para la GPU; `"vc4"` para la pantalla | [`loader.c`, `loader_get_kernel_driver_name`; `v3dv_device.c`, `try_device`] |
| Descripción | `DRM_IOCTL_VERSION` | No debe ser `"shim"`: si lo es, v3dv apaga el envío en un hilo aparte | [`v3dv_device.c`, `create_physical_device`] |
| Tipo de nodo | `fstat` del descriptor: número menor | Render o primario | [libdrm, `drmGetNodeTypeFromFd`] |
| Si vc4 dibuja | `DRM_IOCTL_VC4_GET_PARAM` con `DRM_VC4_PARAM_V3D_IDENT0` | **Tiene que fallar**. Si falla, Mesa usa kmsro con v3d | [`vc4_drm_winsys.c`, `vc4_drm_screen_create`] |
| Si es una pantalla | `drmIsKMS`: `DRM_IOCTL_MODE_GETRESOURCES` | Que no falle y que haya al menos un CRTC, un conector y un codificador | [libdrm, `xf86drmMode.c`, `drmIsKMS`; `v3dv_device.c`, `try_display_device`] |
| Si hay algo conectado | `drmModeGetResources` y `drmModeGetConnector` | Solo si la aplicación pide `VK_KHR_display` | [`v3dv_device.c`, `try_display_device`] |
| Identidad del dispositivo | `fstat`: `st_rdev` | v3dv lo publica en `VK_EXT_physical_device_drm` | [`v3dv_device.c`, `create_physical_device`] |

- El cargador de gallium elige el controlador por el nombre del controlador del kernel. Si no hay uno con ese nombre,
  prueba kmsro. [`pipe_loader_drm.c`, `pipe_loader_drm_probe_fd_nodup`]
- **v3dv quiere tres descriptores**: el nodo de render de v3d, el nodo primario de v3d y el nodo primario de un
  dispositivo de pantalla con KMS. Si falta el de pantalla, el dispositivo se crea, pero no puede reservar búferes
  para ventanas (sección 8.3). [`v3dv_device.c`, `enumerate_devices`]
- Si `fstat` del nodo primario de v3d falla, v3dv apunta un error de inicialización. Se deduce que conviene ofrecer
  también ese nodo.

## 3. Convenciones de las llamadas

### 3.1. Número de la llamada

- Es el `ioctl` estándar de Linux: dirección (2 bits), tamaño de la estructura (14 bits), tipo (`'d'`, `0x64`) y número
  (8 bits). [`include/drm-uapi/drm.h`]
- Las llamadas propias de un controlador empiezan en `DRM_COMMAND_BASE` (`0x40`). Las de v3d van de `0x40` a `0x4D`.
- Como el tamaño va dentro del número, **una estructura de otro tamaño es otra llamada**. Por eso las tablas dan el
  número completo.

### 3.2. Estructuras

- Mesa las rellena en la memoria del proceso, con el orden de bytes de la CPU, y pasa su dirección.
- Los punteros a otras zonas del proceso van siempre en campos de 64 bits (`__u64`). Mesa los rellena con la
  dirección convertida a entero.
- Los campos de 64 bits están alineados a 8 bytes. Los rellenos (`pad`, `mbz`) van a 0.
- Mesa pone a 0 toda la estructura antes de rellenarla. Se deduce que el lado del kernel puede exigir ceros en lo que
  no se usa, como hace Linux.

### 3.3. Resultado

- 0 si va bien. -1 con `errno` si falla.
- El único código de error que Mesa distingue es `ETIME`: plazo vencido en las esperas. [`v3d_bufmgr.c`,
  `v3d_bo_wait`; `vk_drm_syncobj.c`, `sync_has_sync_file`]
- `drmSyncobjWait` devuelve `-errno`. [libdrm, `drmSyncobjWait`]
- En los envíos, un error en v3dv marca la cola como perdida (`VK_ERROR_DEVICE_LOST`). En gallium solo se apunta en
  el registro. [`v3dv_queue.c`, `handle_cl_job`; `v3d_job.c`, `v3d_job_submit`]

### 3.4. Manijas y números

| Objeto | Tipo | Ámbito | Nota |
|---|---|---|---|
| BO (manija GEM) | `u32`, nunca 0 | El fichero abierto | Importar dos veces el mismo dma-buf en el mismo fichero da **la misma manija** (sección 6.5) |
| syncobj | `u32`, 0 = ninguno | El fichero abierto | |
| Monitor de rendimiento | `u32`, 0 = ninguno | El fichero abierto | |
| dma-buf, sync_file, syncobj exportado | Descriptor de fichero | El proceso; se pasa por Wayland | |

## 4. Llamadas propias de v3d

### 4.1. Resumen

| Llamada | Número | Dir. | Estructura (bytes) | Quién la usa | Fuente principal |
|---|---|---|---|---|---|
| `DRM_IOCTL_V3D_SUBMIT_CL` | `0xC0486440` | RW | `drm_v3d_submit_cl` (72) | v3d, v3dv | [`v3d_job.c`, `v3d_job_submit`; `v3dv_queue.c`, `handle_cl_job`] |
| `DRM_IOCTL_V3D_WAIT_BO` | `0xC0106441` | RW | `drm_v3d_wait_bo` (16) | v3d, v3dv | [`v3d_bufmgr.c`, `v3d_wait_bo_ioctl`; `v3dv_bo.c`, `v3dv_bo_wait`] |
| `DRM_IOCTL_V3D_CREATE_BO` | `0xC0106442` | RW | `drm_v3d_create_bo` (16) | v3d, v3dv | [`v3d_bufmgr.c`, `v3d_bo_alloc`; `v3dv_bo.c`, `v3dv_bo_alloc`] |
| `DRM_IOCTL_V3D_MMAP_BO` | `0xC0106443` | RW | `drm_v3d_mmap_bo` (16) | v3d, v3dv | [`v3d_bufmgr.c`, `v3d_bo_map_unsynchronized`; `v3dv_bo.c`, `v3dv_bo_map_unsynchronized`] |
| `DRM_IOCTL_V3D_GET_PARAM` | `0xC0106444` | RW | `drm_v3d_get_param` (16) | v3d, v3dv | [`v3d_device_info.c`, `v3d_get_device_info`; `v3d_screen.c`, `v3d_has_feature`] |
| `DRM_IOCTL_V3D_GET_BO_OFFSET` | `0xC0086445` | RW | `drm_v3d_get_bo_offset` (8) | v3d, v3dv | [`v3d_bufmgr.c`, `v3d_bo_open_handle`; `v3dv_device.c`, `device_import_bo`] |
| `DRM_IOCTL_V3D_SUBMIT_TFU` | `0x40686446` | W | `drm_v3d_submit_tfu` (104) | v3d, v3dv | [`v3dx_tfu.c`, `v3dX(tfu)`; `v3dv_queue.c`, `handle_tfu_job`] |
| `DRM_IOCTL_V3D_SUBMIT_CSD` | `0x40586447` | W | `drm_v3d_submit_csd` (88) | v3d, v3dv | [`v3dx_draw.c`, envío de cómputo; `v3dv_queue.c`, `handle_csd_job`] |
| `DRM_IOCTL_V3D_PERFMON_CREATE` | `0xC0286448` | RW | `drm_v3d_perfmon_create` (40) | v3d, v3dv | [`v3d_query_perfcnt.c`; `v3dv_query.c`, `kperfmon_create`] |
| `DRM_IOCTL_V3D_PERFMON_DESTROY` | `0xC0046449` | RW | `drm_v3d_perfmon_destroy` (4) | v3d, v3dv | Ídem, `kperfmon_destroy` |
| `DRM_IOCTL_V3D_PERFMON_GET_VALUES` | `0xC010644A` | RW | `drm_v3d_perfmon_get_values` (16) | v3d, v3dv | [`v3d_query_perfcnt.c`; `v3dv_query.c`] |
| `DRM_IOCTL_V3D_SUBMIT_CPU` | `0x4018644B` | W | `drm_v3d_submit_cpu` (24) | v3dv; v3d para marcas de tiempo | [`v3dv_queue.c`; `v3d_submit_util.c`, `v3d_submit_timestamp_query_ioctl`] |
| `DRM_IOCTL_V3D_PERFMON_GET_COUNTER` | `0xC168644C` | RW | `drm_v3d_perfmon_get_counter` (360) | v3d, v3dv | [`perfcntrs/v3dx_counter.c`] |
| `DRM_IOCTL_V3D_PERFMON_SET_GLOBAL` | `0x4008644D` | W | `drm_v3d_perfmon_set_global` (8) | Solo el productor de Perfetto | [`broadcom/ds/v3d_pps_driver.cc`] |

"RW" quiere decir que el kernel lee la estructura y escribe en ella. "W" quiere decir que solo la lee.

### 4.2. `GET_PARAM`: identificación y capacidades

| Desp. | Tamaño | Campo | Entra / sale | Qué es |
|---|---|---|---|---|
| 0 | 4 | `param` | Entra | Número del parámetro (tabla siguiente) |
| 4 | 4 | `pad` | Entra | 0 |
| 8 | 8 | `value` | Sale | Valor |

Parámetros que Mesa pide de verdad:

| Núm. | Nombre | Qué hace Mesa con él | Si falla | Fuente |
|---|---|---|---|---|
| 4 | `V3D_CORE0_IDENT0` | Versión mayor = bits 31:24 | Mesa no arranca | [`v3d_device_info.c`, `v3d_get_device_info`] |
| 5 | `V3D_CORE0_IDENT1` | Versión menor = bits 3:0; slices = 7:4; QPU por slice = 11:8; VPM = bits 31:28 × 8192 bytes | Mesa no arranca | Ídem |
| 3 | `V3D_HUB_IDENT3` | Revisión = bits 15:8; "compat_rev" = bits 23:16 | Mesa no arranca | Ídem |
| 13 | `MAX_PERF_COUNTERS` | Número de eventos de contador | Usa su tabla interna | Ídem |
| 15 | `GLOBAL_RESET_COUNTER` | Reinicios de la GPU; solo mira si la llamada funciona | Sin informe de reinicios | Ídem; [`v3d_context.c`, `v3d_get_reset_count`] |
| 16 | `CONTEXT_RESET_COUNTER` | Reinicios causados por este fichero | Ídem | [`v3d_context.c`, `v3d_get_reset_count`] |
| 7 | `SUPPORTS_TFU` | GL: activa generar mipmaps; v3dv: obligatorio | GL: sin esa capacidad | [`v3d_screen.c`; `v3dv_device.c`, `device_has_expected_features`] |
| 8 | `SUPPORTS_CSD` | GL: cómputo; v3dv: obligatorio | GL: sin cómputo | Ídem |
| 9 | `SUPPORTS_CACHE_FLUSH` | GL: imágenes y SSBO; v3dv: obligatorio | GL: sin esas capacidades | Ídem |
| 10 | `SUPPORTS_PERFMON` | Contadores de rendimiento | Sin ellos | Ídem |
| 11 | `SUPPORTS_MULTISYNC_EXT` | GL: consultas de tiempo; v3dv: obligatorio | GL: sin consultas de tiempo | Ídem |
| 12 | `SUPPORTS_CPU_QUEUE` | GL: consultas de tiempo; v3dv: obligatorio | Ídem | Ídem |
| 17 | `SUPPORTS_TFU_CCOL` | Solo en la 7.1 | Falso | Ídem |

- Mesa calcula la versión como mayor × 10 + menor. **Solo acepta 42 y 71.** [`v3d_device_info.c`]
- Mesa **no pide** los parámetros 0 (`UIFCFG`), 1 y 2 (`HUB_IDENT1` y `2`), 6 (`CORE0_IDENT2`) ni 14
  (`SUPPORTS_SUPER_PAGES`). Se comprobó buscando su nombre en todo `src/` (fuera del simulador y del shim).
- Para una capacidad, Mesa trata "llamada fallida" igual que "valor 0". [`v3d_screen.c`, `v3d_has_feature`;
  `v3dv_device.c`, `v3d_has_feature`]
- **v3dv exige cinco capacidades**: TFU, CSD, limpieza de cachés, varias vallas por envío y cola de CPU. Sin ellas no
  crea el dispositivo ("v3dv now requires kernel 6.8+"). [`v3dv_device.c`, `device_has_expected_features`]
- El shim de Mesa usa estos valores para una 4.2: `CORE0_IDENT0` = `0x04443356`, `CORE0_IDENT1` = `0x81001422`,
  `HUB_IDENT3` = `0x00000E00`. Dan versión 42, 2 slices × 4 QPU y VPM de 64 KB. Son valores de pruebas, no leídos de
  una placa. [`broadcom/drm-shim/v3d_noop.c`, tabla `gpus`]
- Los registros de donde salen estos valores se describen en `v3d.md`, sección 3. El campo "compat_rev" (bits 23:16
  de `HUB_IDENT3`) no está descrito en Linux.

### 4.3. `CREATE_BO`

| Desp. | Tamaño | Campo | Entra / sale | Qué es |
|---|---|---|---|---|
| 0 | 4 | `size` | Entra | Tamaño en bytes |
| 4 | 4 | `flags` | Entra | 0. La cabecera no define ninguna |
| 8 | 4 | `handle` | Sale | Manija del BO nuevo |
| 12 | 4 | `offset` | Sale | VA del BO. **Nunca 0** |

- Mesa siempre pide un múltiplo de 4096. [`v3d_bufmgr.c`, `v3d_bo_alloc`; `v3dv_bo.c`, `v3dv_bo_alloc`]
- v3dv comprueba con una aserción que la VA que vuelve es múltiplo de 4096. [`v3dv_bo.c`, `v3dv_bo_alloc`]
- Si falla, gallium vacía su caché de BO y lo intenta una vez más. [`v3d_bufmgr.c`, `v3d_bo_alloc`]
- El tamaño es de 32 bits. v3dv lo comprueba antes de pedir. [`v3dv_device.c`, `device_alloc`]
- La cabecera dice que la VA es privada del fichero abierto y vale mientras viva la manija. [`v3d_drm.h`, comentario
  de `drm_v3d_create_bo`]
- Se espera que el BO llegue a cero. Mesa no lo borra al crearlo. Se deduce del uso: v3dv expone la memoria tal cual
  a la aplicación, y Linux da páginas a cero.

### 4.4. `MMAP_BO` y la proyección en el proceso

| Desp. | Tamaño | Campo | Entra / sale | Qué es |
|---|---|---|---|---|
| 0 | 4 | `handle` | Entra | Manija |
| 4 | 4 | `flags` | Entra | 0 |
| 8 | 8 | `offset` | Sale | Desplazamiento "falso" para pasar a `mmap` sobre el mismo fichero |

- La llamada **no proyecta nada**. Devuelve un número para usar en `mmap`. [`v3d_drm.h`, comentario de
  `drm_v3d_mmap_bo`]
- Después Mesa llama a `mmap` con: dirección libre, el tamaño, lectura y escritura, `MAP_SHARED`, el descriptor del
  nodo de render y ese desplazamiento. [`v3d_bufmgr.c`, `v3d_bo_map_unsynchronized`]
- v3dv puede proyectar **solo una parte** del BO (desde el principio). [`v3dv_bo.c`, `v3dv_bo_map_unsynchronized`]
- Las proyecciones duran mucho: Mesa las guarda en el BO hasta liberarlo, y las quita con `munmap`. [`v3d_bufmgr.c`,
  `v3d_bo_free`; `v3dv_bo.c`, `v3dv_bo_unmap`]
- Mesa proyecta así también los BO importados de otro dispositivo (sección 6.5).
- La coherencia con la caché de la CPU va en la sección 6.3.

### 4.5. `GET_BO_OFFSET`

| Desp. | Tamaño | Campo | Entra / sale | Qué es |
|---|---|---|---|---|
| 0 | 4 | `handle` | Entra | Manija (normalmente recién importada) |
| 4 | 4 | `offset` | Sale | VA del BO, distinta de 0 |

- Mesa la usa tras importar un BO, porque la importación no da la VA. Comprueba con una aserción que no es 0.
  [`v3d_bufmgr.c`, `v3d_bo_open_handle`; `v3dv_device.c`, `device_import_bo`]

### 4.6. `WAIT_BO`

| Desp. | Tamaño | Campo | Entra / sale | Qué es |
|---|---|---|---|---|
| 0 | 4 | `handle` | Entra | Manija |
| 4 | 4 | `pad` | Entra | 0 |
| 8 | 8 | `timeout_ns` | Entra | Plazo **relativo**, en ns. 0 = solo mirar. `~0` = sin plazo |

- Espera a que acabe todo trabajo de la GPU que use el BO. Devuelve 0, o -1 con `ETIME` si vence el plazo.
  [`v3d_bufmgr.c`, `v3d_bo_wait`]
- Cualquier otro error hace que gallium aborte el proceso. [`v3d_bufmgr.c`, `v3d_bo_wait`]
- Gallium la usa antes de cada proyección sincronizada: leer lo que escribió la GPU, o escribir algo que la GPU
  podría estar leyendo. [`v3d_bufmgr.c`, `v3d_bo_map`]
- El comentario de la cabecera habla del último `SUBMIT_CL`. Mesa la usa también tras TFU y cómputo. Se deduce que
  tiene que cubrir **todos** los trabajos que nombren el BO, en cualquier cola y desde cualquier fichero.

### 4.7. `SUBMIT_CL`: binning y render

| Desp. | Tamaño | Campo | Qué es |
|---|---|---|---|
| 0 | 4 | `bcl_start` | VA del inicio de la lista de binning |
| 4 | 4 | `bcl_end` | VA del primer byte tras la lista. Igual a `bcl_start` = sin binning |
| 8 | 4 | `rcl_start` | VA del inicio de la lista de render |
| 12 | 4 | `rcl_end` | VA del primer byte tras la lista |
| 16 | 4 | `in_sync_bcl` | syncobj que esperar antes del binning; 0 = ninguno |
| 20 | 4 | `in_sync_rcl` | syncobj que esperar antes del render; 0 = ninguno |
| 24 | 4 | `out_sync` | syncobj donde dejar la valla de fin; 0 = ninguno |
| 28 | 4 | `qma` | VA de la memoria de baldosas (obligatoria en la 4.2) |
| 32 | 4 | `qms` | Su tamaño en bytes |
| 36 | 4 | `qts` | VA del estado de baldosas |
| 40 | 8 | `bo_handles` | **Puntero** a una lista de manijas `u32` |
| 48 | 4 | `bo_handle_count` | Cuántas |
| 52 | 4 | `flags` | Bit 0 `FLUSH_CACHE`; bit 1 `EXTENSION` |
| 56 | 4 | `perfmon_id` | Monitor de rendimiento; 0 = ninguno |
| 60 | 4 | `pad` | 0 |
| 64 | 8 | `extensions` | **Puntero** a la primera extensión, si `flags` lleva `EXTENSION` |

Lo que hace el kernel con cada campo está en `v3d.md`, secciones 7.2 a 7.4. Lo que añade Mesa:

- **Listas encadenadas.** Gallium y v3dv ponen `bcl_start` en el primer BO de la lista de binning. La lista puede
  saltar a otros BO. `rcl_*` está en un solo BO. [`v3dv_queue.c`, `handle_cl_job`]
- **Memoria de baldosas.** Mesa la reserva en cada envío. `qts` mide 256 bytes por baldosa y capa. `qms` es 128 bytes
  por baldosa y capa, redondeado a 4 KB, más 8 KB, más una reserva de hasta 512 KB según el número de dibujos.
  [`broadcom/common/v3d_util.c`, `v3d_tile_alloc_sizes`; `v3d_job.c`, `alloc_tile_state`]
- **Lista de BO.** Tiene que incluir todos los BO que usa el trabajo: listas, sombreadores, texturas, destinos,
  memoria de baldosas. Gallium envía cuando pasa de 2048. [`v3d_context.h`, `V3D_JOB_MAX_BO_HANDLE_COUNT`;
  `v3dx_draw.c`]
- **`FLUSH_CACHE`.** Mesa lo pone si los sombreadores del render escribieron por la TMU (imágenes, SSBO). Pide la
  limpieza de cachés de `v3d.md`, 7.7, al final del envío. [`v3d_job.c`, `v3d_job_submit`; `v3dv_queue.c`,
  `handle_cl_job`]
- **Gallium** usa los tres campos de sincronía (`in_sync_bcl`, `in_sync_rcl`, `out_sync`). **v3dv** los deja a 0 y
  usa siempre la extensión de varias vallas (4.11). [`v3dv_queue.c`, `handle_cl_job`]
- **Monitor de rendimiento.** Si cambia de un envío a otro, Mesa hace que el nuevo espere al anterior. [`v3d_job.c`,
  `v3d_job_submit`; `v3dv_queue.c`, `handle_cl_job`]

### 4.8. `SUBMIT_TFU`

| Desp. | Tamaño | Campo | Qué es |
|---|---|---|---|
| 0 | 4 | `icfg` | Valor de `TFU_ICFG` |
| 4 | 4 | `iia` | Valor de `TFU_IIA` (dirección de entrada: VA) |
| 8 | 4 | `iis` | Valor de `TFU_IIS` |
| 12 | 4 | `ica` | Valor de `TFU_ICA` |
| 16 | 4 | `iua` | Valor de `TFU_IUA` |
| 20 | 4 | `ioa` | Valor de `TFU_IOA` (dirección de salida: VA) |
| 24 | 4 | `ios` | Valor de `TFU_IOS` |
| 28 | 16 | `coef[4]` | `TFU_COEF0..3` |
| 44 | 16 | `bo_handles[4]` | Manijas. La primera es la salida. 0 = sin usar |
| 60 | 4 | `in_sync` | syncobj que esperar; 0 = ninguno |
| 64 | 4 | `out_sync` | syncobj donde dejar la valla |
| 68 | 4 | `flags` | Bit 1 `EXTENSION` |
| 72 | 8 | `extensions` | **Puntero** a extensiones |
| 80 | 24 | `v71` | Solo la 7.1 (`ioc`, `ccol[4]`, `pad`). En la 4.2, a 0 |

- Las manijas van **dentro** de la estructura, no por puntero.
- Mesa calcula los valores de registro completos. El kernel solo los copia (ver `v3d.md`, 7.5).
- **Gallium pone el mismo syncobj en `in_sync` y `out_sync`.** El trabajo espera a lo que hubiera en él y luego deja
  allí su propia valla. Se deduce que el kernel tiene que leer la valla de entrada **antes** de sustituirla.
  [`v3dx_tfu.c`, `v3dX(tfu)`]
- Si el envío falla, gallium devuelve "no se pudo" y hace la copia de otra forma. No es fatal. [`v3dx_tfu.c`;
  `v3d_blit.c`, `v3d_tfu_blit`]

### 4.9. `SUBMIT_CSD`: cómputo

| Desp. | Tamaño | Campo | Qué es |
|---|---|---|---|
| 0 | 28 | `cfg[7]` | Valores de `CSD_QUEUED_CFG0..6` |
| 28 | 16 | `coef[4]` | Sin uso en el kernel (`v3d.md`, 7.6) |
| 44 | 4 | (hueco) | Relleno de alineación, 0 |
| 48 | 8 | `bo_handles` | **Puntero** a manijas `u32` |
| 56 | 4 | `bo_handle_count` | Cuántas |
| 60 | 4 | `in_sync` | syncobj que esperar |
| 64 | 4 | `out_sync` | syncobj donde dejar la valla |
| 68 | 4 | `perfmon_id` | Monitor; 0 = ninguno |
| 72 | 8 | `extensions` | **Puntero** a extensiones |
| 80 | 4 | `flags` | Bit 1 `EXTENSION` |
| 84 | 4 | `pad` | 0 |

- `cfg[4]` lleva lotes menos 1 en la 4.2. `cfg[5]` es la VA del sombreador con bits de modo. `cfg[6]` es la VA de los
  uniformes. [`v3dx_draw.c`, envío de cómputo]
- Gallium resuelve el cómputo indirecto en la CPU: espera, proyecta el búfer de recuentos y lo lee. v3dv lo pide al
  kernel con un trabajo de CPU (4.10). [`v3dx_draw.c`; `v3dv_queue.c`, `handle_csd_indirect_cpu_job`]

### 4.10. `SUBMIT_CPU` y sus extensiones

| Desp. | Tamaño | Campo | Qué es |
|---|---|---|---|
| 0 | 8 | `bo_handles` | **Puntero** a manijas `u32` |
| 8 | 4 | `bo_handle_count` | Cuántas (depende del tipo, tabla siguiente) |
| 12 | 4 | `flags` | Siempre con `EXTENSION` |
| 16 | 8 | `extensions` | **Puntero** a la cadena de extensiones |

El tipo de trabajo lo da una extensión de la cadena. Lo que hace cada uno lo describe `v3d.md`, 7.8.

| Id | Extensión | Tamaño | BO que lleva el envío | Campos tras la cabecera (desplazamiento: campo) |
|---|---|---|---|---|
| 2 | `drm_v3d_indirect_csd` | 128 | Los del cómputo | 16: `submit` (una `drm_v3d_submit_csd` entera, 88 bytes); 104: `indirect` (manija del BO de recuentos); 108: `offset`; 112: `wg_size`; 116: `wg_uniform_offsets[3]` (`0xFFFFFFFF` = no reescribir) |
| 3 | `drm_v3d_timestamp_query` | 40 | 1: el de las marcas | 16: `offsets` (puntero a `u32`); 24: `syncs` (puntero a `u32`); 32: `count`; 36: `pad` |
| 4 | `drm_v3d_reset_timestamp_query` | 32 | 1: el de las marcas | 16: `syncs`; 24: `offset`; 28: `count` |
| 5 | `drm_v3d_copy_timestamp_query` | 48 | 2: destino y marcas | 16: `do_64bit` (u8); 17: `do_partial` (u8); 18: `availability_bit` (u8); 19: `pad`; 20: `offset`; 24: `stride`; 28: `count`; 32: `offsets`; 40: `syncs` |
| 6 | `drm_v3d_reset_performance_query` | 40 | Ninguno | 16: `syncs`; 24: `count`; 28: `nperfmons`; 32: `kperfmon_ids` |
| 7 | `drm_v3d_copy_performance_query` | 56 | 1: destino | 16: `do_64bit`; 17: `do_partial`; 18: `availability_bit`; 20: `offset`; 24: `stride`; 28: `nperfmons`; 32: `ncounters`; 36: `count`; 40: `syncs`; 48: `kperfmon_ids` |

- `syncs` son syncobj que hacen de "consulta disponible". El trabajo de marca de tiempo los señala. Los de borrado los
  vuelven a "sin señalar". [`v3d_drm.h`, comentarios de cada extensión]
- `kperfmon_ids` es un puntero a una lista de `count` punteros de 64 bits. Cada uno apunta a `nperfmons` números de
  monitor de 32 bits. Son **dos niveles de punteros**. [`v3dv_queue.c`, `handle_reset_query_cpu_job`]
- En el cómputo indirecto, el trabajo de CPU va unido a un trabajo de cómputo. El kernel lee los recuentos, rellena
  el cómputo y lo lanza. [`v3d_drm.h`, comentario de `drm_v3d_indirect_csd`]
- v3dv hace que los trabajos de marca de tiempo esperen a **todo** lo anterior de la cola. [`v3dv_queue.c`,
  `handle_timestamp_query_cpu_job`]
- El reloj de las marcas: v3dv calcula el periodo con `clock_getres(CLOCK_MONOTONIC)`. Se deduce que el kernel tiene
  que escribir nanosegundos de ese reloj. [`v3dv_device.c`, propiedades del dispositivo]
- Gallium solo usa la extensión 3, para las consultas de tiempo, y solo si hay cola de CPU y varias vallas.
  [`v3d_screen.c`; `v3d_query.c`]

### 4.11. Extensiones: la cadena y las varias vallas

Cabecera común de toda extensión (`drm_v3d_extension`, 16 bytes):

| Desp. | Tamaño | Campo | Qué es |
|---|---|---|---|
| 0 | 8 | `next` | **Puntero** a la siguiente extensión; 0 = fin |
| 8 | 4 | `id` | Tipo (1 a 7) |
| 12 | 4 | `flags` | 0 |

Extensión de varias vallas (`drm_v3d_multi_sync`, id 1, 48 bytes):

| Desp. | Tamaño | Campo | Qué es |
|---|---|---|---|
| 0 | 16 | `base` | Cabecera |
| 16 | 8 | `in_syncs` | **Puntero** a una lista de `drm_v3d_sem` que esperar |
| 24 | 8 | `out_syncs` | **Puntero** a una lista de `drm_v3d_sem` que señalar |
| 32 | 4 | `in_sync_count` | Cuántas de entrada |
| 36 | 4 | `out_sync_count` | Cuántas de salida |
| 40 | 4 | `wait_stage` | En qué cola se espera: 0 `BIN`, 1 `RENDER`, 2 `TFU`, 3 `CSD`, 4 `CACHE_CLEAN`, 5 `CPU` |
| 44 | 4 | `pad` | 0 |

Cada `drm_v3d_sem` mide 32 bytes: `handle` (0, u32), `flags` (4, u32), `point` (8, u64) y `mbz[2]` (16, dos u64).
Mesa solo rellena `handle`. El resto va a 0. [`v3d_submit_util.c`, `v3d_multisync_init`]

- Con esta extensión, los campos de sincronía sueltos del envío van a 0. [`v3dv_queue.c`]
- v3dv pone siempre **una** valla de salida y varias de entrada. [`v3dv_queue.c`, `set_multisync`]
- En `SUBMIT_CL`, `wait_stage` es `BIN` (esperar antes del binning) o `RENDER` (dejar correr el binning y esperar
  antes del render). [`v3dv_queue.c`, `handle_cl_job`]
- La cadena puede llevar varias extensiones. v3dv pone primero la de vallas y detrás la de trabajo de CPU. El kernel
  tiene que recorrerla entera. [`v3dv_queue.c`, `set_multisync` con `next`]

### 4.12. Contadores de rendimiento

| Estructura | Campos (desplazamiento: campo) | Qué hace |
|---|---|---|
| `drm_v3d_perfmon_create` (40) | 0: `id` (sale); 4: `ncounters`; 8: `counters[32]` (u8, número de evento) | Crea un monitor con hasta 32 eventos |
| `drm_v3d_perfmon_destroy` (4) | 0: `id` | Lo destruye |
| `drm_v3d_perfmon_get_values` (16) | 0: `id`; 4: `pad`; 8: `values_ptr` (**puntero** a `ncounters` valores u64) | Copia los valores acumulados. No espera: Mesa espera antes por su cuenta |
| `drm_v3d_perfmon_get_counter` (360) | 0: `counter` (u8); 1: `name[64]`; 65: `category[32]`; 97: `description[256]`; 353: `reserved[7]` | Nombre y descripción de un evento |
| `drm_v3d_perfmon_set_global` (8) | 0: `flags` (bit 0 = quitar); 4: `id` | Monitor para todos los trabajos. Solo lo usa Perfetto |

- v3dv parte una consulta de más de 32 eventos en varios monitores, uno por pasada. [`v3dv_query.c`,
  `kperfmon_create`]
- Si `MAX_PERF_COUNTERS` no responde, Mesa usa su propia tabla de eventos y no llama a `GET_COUNTER`.
  [`perfcntrs/v3dx_counter.c`]
- El hardware está en `v3d.md`, sección 12.

## 5. Llamadas genéricas de DRM y otras

### 5.1. Sobre el dispositivo v3d

| Llamada | Número | Estructura (bytes) | Para qué | Fuente |
|---|---|---|---|---|
| `DRM_IOCTL_VERSION` | `0xC0406400` | `drm_version` (64) | Nombre `"v3d"` y descripción | [libdrm, `drmGetVersion`] |
| `DRM_IOCTL_GET_CAP` | `0xC010640C` | `drm_get_cap` (16) | Solo `DRM_CAP_SYNCOBJ_TIMELINE` (20) | [`util/u_sync_provider.c`] |
| `DRM_IOCTL_GEM_CLOSE` | `0x40086409` | `drm_gem_close` (8) | Soltar una manija | [`v3d_bufmgr.c`, `v3d_bo_free`; `v3dv_bo.c`] |
| `DRM_IOCTL_PRIME_HANDLE_TO_FD` | `0xC00C642D` | `drm_prime_handle` (12) | Exportar un BO como dma-buf | [libdrm, `drmPrimeHandleToFD`] |
| `DRM_IOCTL_PRIME_FD_TO_HANDLE` | `0xC00C642E` | `drm_prime_handle` (12) | Importar un dma-buf | [libdrm, `drmPrimeFDToHandle`] |
| `DRM_IOCTL_GEM_FLINK` | `0xC008640A` | `drm_gem_flink` (8) | Nombre global de un BO (DRI2 de X11) | [`v3d_bufmgr.c`, `v3d_bo_flink`] |
| `DRM_IOCTL_GEM_OPEN` | `0xC010640B` | `drm_gem_open` (16) | Abrir por nombre global | [`v3d_bufmgr.c`, `v3d_bo_open_name`] |
| `DRM_IOCTL_SYNCOBJ_CREATE` | `0xC00864BF` | `drm_syncobj_create` (8) | Crear; bandera 1 = ya señalado | [libdrm, `drmSyncobjCreate`] |
| `DRM_IOCTL_SYNCOBJ_DESTROY` | `0xC00864C0` | `drm_syncobj_destroy` (8) | Destruir | Ídem |
| `DRM_IOCTL_SYNCOBJ_HANDLE_TO_FD` | `0xC01064C1` | `drm_syncobj_handle` (16) | Exportar: como syncobj, o (bandera 1) como sync_file | [libdrm, `drmSyncobjHandleToFD`, `drmSyncobjExportSyncFile`] |
| `DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE` | `0xC01064C2` | `drm_syncobj_handle` (16) | Importar: un syncobj, o (bandera 1) un sync_file dentro de uno existente | [libdrm, `drmSyncobjFDToHandle`, `drmSyncobjImportSyncFile`] |
| `DRM_IOCTL_SYNCOBJ_WAIT` | `0xC02864C3` | `drm_syncobj_wait` (40) | Esperar uno o varios | [libdrm, `drmSyncobjWait`] |
| `DRM_IOCTL_SYNCOBJ_RESET` | `0xC01064C4` | `drm_syncobj_array` (16) | Volver a "sin valla" | [libdrm, `drmSyncobjReset`] |
| `DRM_IOCTL_SYNCOBJ_SIGNAL` | `0xC01064C5` | `drm_syncobj_array` (16) | Señalar desde la CPU | [libdrm, `drmSyncobjSignal`] |
| `DRM_IOCTL_SYNCOBJ_TRANSFER` | `0xC02064CC` | `drm_syncobj_transfer` (32) | Copiar la valla de uno a otro | [libdrm, `drmSyncobjTransfer`] |
| `DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT`, `_SIGNAL`, `_QUERY` | `0xC03064CA`, `0xC01864CD`, `0xC01864CB` | — | Solo si `DRM_CAP_SYNCOBJ_TIMELINE` es distinto de 0 | [`util/u_sync_provider.c`] |

Detalles de las estructuras genéricas:

| Estructura | Campos (desplazamiento: campo) |
|---|---|
| `drm_gem_close` | 0: `handle`; 4: `pad` |
| `drm_prime_handle` | 0: `handle`; 4: `flags` (Mesa pasa `O_CLOEXEC` \| `O_RDWR`); 8: `fd` |
| `drm_version` | 0, 4, 8: versión mayor, menor y de parche (int); 16: `name_len`; 24: `name` (**puntero**); 32: `date_len`; 40: `date` (**puntero**); 48: `desc_len`; 56: `desc` (**puntero**). libdrm la llama dos veces: primero para saber las longitudes, luego con los búferes |
| `drm_get_cap` | 0: `capability` (u64); 8: `value` (u64) |
| `drm_syncobj_create` | 0: `handle` (sale); 4: `flags` |
| `drm_syncobj_destroy` | 0: `handle`; 4: `pad` |
| `drm_syncobj_handle` | 0: `handle`; 4: `flags`; 8: `fd`; 12: `pad` |
| `drm_syncobj_wait` | 0: `handles` (**puntero** a u32); 8: `timeout_nsec` (s64, **absoluto**); 16: `count_handles`; 20: `flags`; 24: `first_signaled` (sale); 28: `pad` |
| `drm_syncobj_array` | 0: `handles` (**puntero**); 8: `count_handles`; 12: `pad` |
| `drm_syncobj_transfer` | 0: `src_handle`; 4: `dst_handle`; 8: `src_point` (u64); 16: `dst_point` (u64); 24: `flags`; 28: `pad` |

- `GEM_FLINK` y `GEM_OPEN` solo hacen falta para X11 con DRI2. Se deduce que DashOS no las necesita.
- El plazo de `SYNCOBJ_WAIT` es un **instante** de `CLOCK_MONOTONIC` en ns, no una duración. Mesa lo calcula así y
  usa `INT64_MAX` para "sin plazo". [`v3d_fence.c`, `v3d_fence_wait`; `vk_drm_syncobj.c`]
- Banderas de espera que usa Mesa: `WAIT_ALL` (1) y `WAIT_FOR_SUBMIT` (2). `WAIT_FOR_SUBMIT` quiere decir: si el
  syncobj aún no tiene valla, esperar a que la tenga y luego a que se señale. [`vk_drm_syncobj.c`]
- Mesa comprueba que la espera funciona creando un syncobj señalado y esperándolo con plazo 0. Si falla, Vulkan pierde
  la espera en la CPU. [`vk_drm_syncobj.c`, `vk_drm_syncobj_get_type_from_provider`]

### 5.2. Sobre el dispositivo de pantalla (vc4)

| Llamada | Número | Estructura (bytes) | Para qué | Fuente |
|---|---|---|---|---|
| `DRM_IOCTL_VERSION` | `0xC0406400` | `drm_version` | Nombre `"vc4"` | [`loader.c`] |
| `DRM_IOCTL_VC4_GET_PARAM` | `0xC0106447` | `drm_vc4_get_param` (16) | **Tiene que fallar** en la Pi 4 | [`vc4_drm_winsys.c`] |
| `DRM_IOCTL_MODE_GETRESOURCES` | (de `drm_mode.h`) | `drm_mode_card_res` | `drmIsKMS`; listas con **punteros** | [libdrm, `drmIsKMS`] |
| `DRM_IOCTL_MODE_GETCONNECTOR` | (de `drm_mode.h`) | `drm_mode_get_connector` | Solo con `VK_KHR_display` | [`v3dv_device.c`, `try_display_device`] |
| `DRM_IOCTL_MODE_CREATE_DUMB` | `0xC02064B2` | `drm_mode_create_dumb` (32) | Reservar un búfer de pantalla | [`renderonly.c`; `v3dv_device.c`, `device_alloc_for_wsi`] |
| `DRM_IOCTL_MODE_DESTROY_DUMB` | `0xC00464B4` | `drm_mode_destroy_dumb` (4) | Liberarlo | Ídem |
| `DRM_IOCTL_PRIME_HANDLE_TO_FD` | `0xC00C642D` | `drm_prime_handle` | Exportar el búfer de pantalla | Ídem |
| `DRM_IOCTL_PRIME_FD_TO_HANDLE` | `0xC00C642E` | `drm_prime_handle` | Importar un BO de v3d en la pantalla | [`renderonly.c`, `renderonly_create_gpu_import_for_resource`] |

`drm_mode_create_dumb`: 0 `height`; 4 `width`; 8 `bpp`; 12 `flags`; 16 `handle` (sale); 20 `pitch` (sale); 24 `size`
(sale, u64).

### 5.3. Fuera de DRM

| Llamada | Sobre qué | Para qué | Fuente |
|---|---|---|---|
| `SYNC_IOC_MERGE` | sync_file | Juntar dos sync_file en uno nuevo que se señala cuando lo están los dos | [`util/libsync.h`, `sync_merge`; `v3dv_queue.c`, `merge_syncobjs`; `v3d_fence.c`, `v3d_fence_server_sync`] |
| `SYNC_IOC_FILE_INFO` | sync_file | Saber si un descriptor es un sync_file | [`util/libsync.h`] |
| `poll` | sync_file | Esperar a que se señale | [`util/libsync.h`, `sync_wait`] |
| `DMA_BUF_IOCTL_EXPORT_SYNC_FILE` | dma-buf | Sacar la valla implícita de un búfer compartido | [`wsi_common_drm.c`, `wsi_dma_buf_export_sync_file`] |
| `DMA_BUF_IOCTL_IMPORT_SYNC_FILE` | dma-buf | Añadir una valla implícita a un búfer compartido | [`wsi_common_drm.c`, `wsi_dma_buf_import_sync_file`] |
| `lseek(fd, 0, SEEK_END)` | dma-buf | Saber su tamaño | [`v3d_bufmgr.c`, `v3d_bo_open_dmabuf`; `v3dv_device.c`, `device_import_bo`] |

- Las dos de dma-buf son opcionales. Si fallan con `ENOTTY`, `EBADF` o `ENOSYS`, la WSI de Vulkan sigue sin ellas.
  [`wsi_common_drm.c`]

### 5.4. Campos que son punteros

Si DashOS pasa las llamadas a otro proceso por un canal, tiene que copiar también lo apuntado. Esta es la lista
completa de lo que Mesa apunta:

| Estructura | Campo | Qué apunta | Sentido |
|---|---|---|---|
| `submit_cl`, `submit_csd`, `submit_cpu` | `bo_handles` | `bo_handle_count` × u32 | Entra |
| `submit_cl`, `_tfu`, `_csd`, `_cpu` | `extensions` | Cadena de extensiones | Entra |
| Cualquier extensión | `next` | Siguiente extensión | Entra |
| `multi_sync` | `in_syncs`, `out_syncs` | `drm_v3d_sem` × cuenta (32 bytes cada una) | Entra |
| `indirect_csd` | `submit.bo_handles` | Manijas del cómputo | Entra |
| `timestamp_query`, `copy_timestamp_query` | `offsets`, `syncs` | `count` × u32 | Entra |
| `reset_timestamp_query`, `*_performance_query` | `syncs` | `count` × u32 | Entra |
| `*_performance_query` | `kperfmon_ids` | `count` × puntero de 64 bits, cada uno a `nperfmons` × u32 | Entra |
| `perfmon_get_values` | `values_ptr` | `ncounters` × u64 | Sale |
| `drm_version` | `name`, `date`, `desc` | Búferes de texto | Sale |
| `syncobj_wait`, `syncobj_array` | `handles` | u32 | Entra |
| `mode_card_res`, `mode_get_connector` | Listas de objetos | u32 y otros | Sale |

## 6. Búferes

### 6.1. Cómo los reserva Mesa

- Siempre en múltiplos de 4096. [`v3d_bufmgr.c`; `v3dv_bo.c`]
- Gallium añade 64 bytes a cada recurso. El comentario lo explica: la TFU lee 64 bytes por delante y la instrucción
  `ldunifa` lee 4 bytes por delante. Si caen fuera del BO, la MMU da error. [`v3d_resource.c`, `v3d_resource_bo_alloc`;
  `v3d_device_info.h`, `V3D_TFU_READAHEAD_SIZE`]
- Se deduce que el kernel puede dar error de MMU si se lee justo tras el final de un BO. Mesa evita eso por su cuenta.
  La página de relleno de `v3d.md`, 5.5, también lo cubre.
- Gallium y v3dv guardan los BO libres en una caché por tamaño y los reutilizan. Solo los cierran pasado un rato o si
  falta memoria. [`v3d_bufmgr.c`, `v3d_bo_last_unreference_locked_timed`; `v3dv_bo.c`]
- Un BO que se ha exportado no vuelve a la caché. [`v3d_bufmgr.c`, `v3d_bo_get_dmabuf`]
- v3dv anuncia un solo montón de memoria. Su tamaño sale de la memoria total del sistema, con un máximo de 4 GB porque
  la MMU es de 32 bits. [`v3dv_device.c`, `compute_heap_size`]

### 6.2. La VA

- La da el kernel al crear el BO, o con `GET_BO_OFFSET` tras importarlo.
- No es 0 nunca. Es múltiplo de 4096. No cambia mientras viva la manija.
- Mesa la escribe directamente en las listas de control, en los registros de sombreador, en los uniformes, en los
  registros de la TFU y del CSD. No hay reubicación en el kernel. [`v3d_cl.h`, `cl_aligned_reloc`; `v3d_job.c`]
- El kernel no valida esas direcciones (ver `v3d.md`, sección 6).
- La cabecera dice que la VA es "privada del fichero". En Linux hay una sola tabla para todos (`v3d.md`, 5.1). Mesa
  funciona igual con cualquiera de las dos cosas.

### 6.3. Coherencia con la CPU

- Mesa **nunca** limpia ni invalida la caché de la CPU. No hay llamada para ello. Se buscó en v3d y v3dv.
- v3dv anuncia su única memoria como "visible por la CPU y coherente" (`HOST_VISIBLE | HOST_COHERENT`).
  [`v3dv_device.c`, tipos de memoria]
- Se deduce que **las proyecciones de `MMAP_BO` tienen que ser coherentes** con lo que lee y escribe la GPU. En la Pi 4
  eso quiere decir memoria no cacheable para la CPU, como hace Linux (`v3d.md`, 2.7).
- Mesa lee resultados que escribe la GPU (consultas de oclusión, contadores de primitivas) por esas proyecciones,
  tras esperar. [`v3d_query_pipe.c`]

### 6.4. Formatos y modificadores

| Modificador | Valor de 64 bits | Qué es | Cuándo lo usa Mesa |
|---|---|---|---|
| `DRM_FORMAT_MOD_LINEAR` | `0x0000000000000000` | Filas seguidas | Búferes de pantalla, cursores, 1D, búferes; y si lo pide el otro lado |
| `DRM_FORMAT_MOD_BROADCOM_UIF` | `0x0700000000000006` | Baldosas UIF de la V3D | Por defecto para texturas compartidas, si el otro lado lo acepta |
| `DRM_FORMAT_MOD_BROADCOM_SAND128` | `0x0700000000000004` más un parámetro en los bits 8 a 55 | Columnas de 128 bytes (vídeo) | Solo para importar NV12, P030 y planos de 8 o 16 bits |
| `DRM_FORMAT_MOD_INVALID` | `0x00FFFFFFFFFFFFFF` | "Sin modificador explícito" | Al importar, Mesa lo trata como lineal |

[`drm_fourcc.h`; `v3d_screen.c`, `v3d_available_modifiers` y `v3d_screen_query_dmabuf_modifiers`; `v3d_resource.c`,
`v3d_resource_create_with_modifiers` y `v3d_resource_from_handle`]

- Con la marca de "búfer de pantalla" (`PIPE_BIND_SCANOUT`), gallium usa **siempre lineal**. [`v3d_resource.c`]
- Al exportar, Mesa da paso de fila, desplazamiento 0 y modificador. Al importar con UIF, el paso tiene que coincidir
  con el que calcula Mesa. [`v3d_resource.c`, `v3d_resource_get_handle` y `v3d_resource_from_handle`]
- Los formatos de píxel de DRM (`XRGB8888`...) se definen como números little-endian. Cómo los lee el HVS está en
  `hvs.md`, 6.3. El HVS de la Pi 4 no lee UIF: Linux solo le acepta lineal, T-tiled y SAND (`hvs.md`, 6.3).

### 6.5. Compartir: PRIME y dma-buf

- **Exportar**: `PRIME_HANDLE_TO_FD` con `O_CLOEXEC | O_RDWR`. Devuelve un descriptor de dma-buf.
  [`v3d_bufmgr.c`, `v3d_bo_get_dmabuf`; `v3dv_device.c`, `v3dv_GetMemoryFdKHR`]
- **Importar**: `PRIME_FD_TO_HANDLE` da una manija; `lseek` al final da el tamaño; `GET_BO_OFFSET` da la VA.
  [`v3d_bufmgr.c`, `v3d_bo_open_dmabuf`]
- **Misma manija**: Mesa guarda los BO por manija en una tabla. Si importa un dma-buf que ya tenía, espera recibir la
  manija de antes. v3dv lleva un contador de importaciones por manija. [`v3d_bufmgr.c`, `v3d_bo_open_handle`;
  `v3dv_device.c`, `device_import_bo`; `renderonly.c`, `renderonly_create_gpu_import_for_resource`]
- **Cerrar una vez**: `GEM_CLOSE` suelta la manija aunque se haya importado varias veces. Mesa solo la cierra cuando
  su propio contador llega a cero.
- **Proyectar lo importado**: Mesa usa `MMAP_BO` también con manijas importadas de vc4. Se deduce que el lado de la
  GPU tiene que poder proyectar memoria que reservó la pantalla.
- **Tamaño**: Mesa exige que el dma-buf mida al menos lo que espera. [`v3dv_device.c`, `device_import_bo`]

### 6.6. Búferes de pantalla: el truco de la "imagen de una página de ancho"

- Cuando un búfer tiene que verlo el HVS, Mesa **no lo reserva en v3d**. Lo pide al dispositivo de pantalla con
  `MODE_CREATE_DUMB` y luego lo importa en v3d. El comentario de v3dv lo explica: el dispositivo de pantalla de la
  Pi 4 no tiene MMU. [`v3dv_device.c`, `enumerate_devices` y `device_alloc_for_wsi`]
- Mesa no pide la imagen real. Pide una "imagen" de 1024 píxeles de ancho, 32 bits por píxel (una fila = 4096 bytes)
  y tantas filas como páginas necesite. Así obtiene un bloque de memoria del tamaño justo. Luego pone dentro su propio
  formato. [`v3d_resource.c`, `v3d_resource_create_with_modifiers`; `v3dv_device.c`, `device_alloc_for_wsi`]
- Gallium suma los 64 bytes de lectura adelantada antes de redondear. [`v3d_resource.c`]
- Se deduce que `MODE_CREATE_DUMB` del lado de la pantalla tiene que dar memoria **contigua, en el primer GB y no
  cacheable**, porque el HVS la lee por DMA sin MMU (`hvs.md`, 2.4).

## 7. Sincronización

### 7.1. Objetos

- **syncobj**: un contenedor del kernel que guarda una valla (o ninguna). Una valla es un "esto terminará". Mesa solo
  usa syncobj **binarios**: señalado o no.
- **sync_file**: un descriptor de fichero con una valla dentro. Sirve para pasar vallas entre procesos y a
  otras API (EGL, Wayland, `poll`).
- **Valla implícita**: la que lleva un BO compartido. Dice "alguien está escribiendo en este búfer".

### 7.2. Gallium v3d (OpenGL ES)

- Cada contexto tiene **un syncobj de salida** (`out_sync`), creado ya señalado. Todos sus envíos dejan ahí su valla.
  [`v3d_context.c`, `v3d_context_create`]
- El render de un envío espera a `out_sync` para no adelantarse a una TFU anterior. [`v3d_job.c`, `v3d_job_submit`]
- Cuando una aplicación pide una valla (`glFenceSync`, EGL), Mesa exporta `out_sync` como sync_file. El comentario
  dice que preferiría un syncobj, pero un sync_file es una foto fija del momento. [`v3d_context.c`, `v3d_pipe_flush`]
- Esperar esa valla: crear un syncobj, importar el sync_file, esperarlo con plazo absoluto y destruirlo.
  [`v3d_fence.c`, `v3d_fence_wait`]
- Vallas que llegan de fuera (`EGL_ANDROID_native_fence_sync`): Mesa las junta con `SYNC_IOC_MERGE`, las importa en un
  segundo syncobj (`in_syncobj`) y lo pone en `in_sync_bcl`. [`v3d_fence.c`, `v3d_fence_server_sync`; `v3d_job.c`]
- Consultas: syncobj propios que se esperan con plazo 0 para preguntar "¿ya está?". [`v3d_query_pipe.c`]
- Antes de leer o escribir un BO desde la CPU: `WAIT_BO`. [`v3d_bufmgr.c`, `v3d_bo_map`]

### 7.3. v3dv (Vulkan)

- Cada cola de Vulkan tiene cuatro syncobj, uno por cola del kernel: CL, TFU, CSD y CPU (`last_job_syncs`). Cada
  envío deja su valla en el de su cola. [`v3dv_device.c`, `queue_init`]
- **Entrada de un trabajo** [`v3dv_queue.c`, `gather_in_syncs`]:
  - El primer trabajo de cada cola del kernel en un envío de Vulkan espera a los semáforos de espera.
  - Si hay una barrera, espera además al último trabajo de las colas afectadas.
- **Salida de un envío de Vulkan**: Mesa exporta los cuatro syncobj como sync_file, los junta con `SYNC_IOC_MERGE` y
  los importa en cada semáforo o valla de señal. Si no hubo trabajos ni esperas, los señala con `SYNCOBJ_SIGNAL`.
  [`v3dv_queue.c`, `merge_syncobjs` y `v3dv_queue_driver_submit`]
- **Semáforos con línea de tiempo** (timelines): v3dv los **emula en el proceso** sobre syncobj binarios. Apaga a
  propósito los del kernel aunque existan. El comentario dice que "aún no se soportan en la uAPI". [`v3dv_device.c`,
  `create_physical_device`]
- **Envío en un hilo aparte**: v3dv lo activa siempre (salvo con el shim). Ese hilo espera a que los semáforos tengan
  valla con `WAIT_FOR_SUBMIT`. [`v3dv_device.c`, `queue_init`; `vk_drm_syncobj.c`]
- **Espera "a que exista la valla"** sin timelines: Mesa intenta exportar un sync_file en bucle hasta que funciona. Se
  deduce que exportar un syncobj **sin valla** tiene que fallar. [`vk_drm_syncobj.c`, `vk_drm_syncobj_wait_many` y
  `sync_has_sync_file`]
- **Copiar semáforos** sin pasar por la GPU (presentación sin órdenes): `SYNCOBJ_TRANSFER`. Con varias entradas usa un
  syncobj temporal con **puntos de línea de tiempo** 1, 2, 3... y luego lo copia como binario. Se deduce que
  `TRANSFER` necesita puntos aunque la capacidad de timelines diga que no. [`vk_drm_syncobj.c`,
  `vk_drm_syncobj_copy_payloads`; `wsi_common.c`, `wsi_queue_submit2_unordered`]
- **Semáforos exportables**: Vulkan exporta e importa syncobj como descriptor opaco y como sync_file.
  [`vk_drm_syncobj.c`, `vk_drm_syncobj_export_opaque_fd` y siguientes]

### 7.4. Garantías de orden que Mesa da por hechas

Comprobado en comentarios de la cabecera y de Mesa:

1. Dentro de un fichero, los binning se ejecutan en orden de envío. Los render, también. [`v3d_drm.h`, comentarios de
   `bcl_start` y `rcl_start`]
2. El render de un envío espera a su propio binning. [Ídem]
3. TFU y cómputo de un fichero se ejecutan en orden de envío. [`v3d_drm.h`, comentarios de `drm_v3d_submit_tfu` y
   `drm_v3d_submit_csd`]
4. Entre colas distintas, o entre ficheros distintos, **no hay orden**. Mesa lo pide con vallas. [Ídem; `v3dv_queue.c`,
   comentario en `handle_cl_job`]
5. La valla de salida se señala cuando termina **todo** el envío: el render, o la limpieza de cachés si se pidió.
   [`v3d_drm.h`, comentario de `drm_v3d_submit_cl`]
6. Cuando se señala la valla de un render, las escrituras de la memoria de baldosas ya están en la RAM. Las de la TMU
   solo con `FLUSH_CACHE`. [Ídem]

### 7.5. Sincronía implícita en búferes compartidos

- **EGL en Wayland no manda vallas al compositor.** Envía el trabajo y entrega el búfer enseguida. [`platform_wayland.c`;
  no hay uso de `wp_linux_drm_syncobj` en ese fichero]
- La WSI de Vulkan en Wayland solo usa vallas explícitas si los semáforos con línea de tiempo se pueden exportar como
  descriptor opaco. Con los de v3dv (emulados) no se puede. [`wsi_common.c`, `wsi_device_supports_explicit_sync`]
- Sin vallas explícitas, la WSI intenta `DMA_BUF_IOCTL_EXPORT/IMPORT_SYNC_FILE`. Si no existen, no hace nada más:
  v3dv no tiene otro mecanismo. [`wsi_common_drm.c`, `wsi_drm_init_swapchain_implicit_sync`; se buscó
  `implicit_sync` en v3dv sin resultados]
- Se deduce que **quien lea un búfer compartido tiene que poder esperar a la última escritura de la GPU en él**. En
  Linux eso lo hace el kernel: cada envío ata su valla a los BO de su lista. En DashOS hay dos caminos:
  - El servicio de la GPU guarda la última valla de cada búfer compartido y la ofrece (por `poll` sobre el dma-buf, o
    con `EXPORT_SYNC_FILE`).
  - O el compositor de DashOS no lee un búfer hasta que el servicio le diga que está listo.

## 8. El camino a la pantalla

### 8.1. OpenGL ES con GBM y KMS (un compositor que pinta con la GPU)

1. El compositor abre el nodo primario de vc4 y crea un dispositivo GBM.
2. GBM ve el nombre `"vc4"`. El controlador vc4 llama a `VC4_GET_PARAM`, falla, y pasa a kmsro.
   [`vc4_drm_winsys.c`, `vc4_drm_screen_create`]
3. kmsro busca un nodo de render de plataforma llamado `"v3d"` y crea el controlador v3d con un objeto `renderonly`.
   [`kmsro_drm_winsys.c`, `kmsro_drm_screen_create`]
4. Para cada búfer de pantalla: `MODE_CREATE_DUMB` en vc4, `PRIME_HANDLE_TO_FD` en vc4, `PRIME_FD_TO_HANDLE` en v3d,
   `GET_BO_OFFSET`. [`renderonly.c`, `renderonly_create_kms_dumb_buffer_for_resource`; `v3d_resource.c`]
5. Para dar el búfer al KMS, Mesa da la manija **de vc4** (`WINSYS_HANDLE_TYPE_KMS`). [`v3d_resource.c`,
   `v3d_resource_get_handle`]
6. El cambio de búfer en pantalla (`drmModeAddFB2`, `drmModeAtomicCommit`...) lo hace el compositor, no Mesa.
- GBM también crea búferes simples para cursores y para escribir desde la CPU, con `MODE_CREATE_DUMB` en vc4.
  [`gbm_dri.c`, `create_dumb`]

### 8.2. OpenGL ES como cliente de Wayland

1. Mesa se une a `zwp_linux_dmabuf_v1`. Con la versión 4 pide el "feedback": un `dev_t` del dispositivo principal y
   una tabla de formatos y modificadores. La tabla llega como un descriptor que Mesa proyecta con `mmap` de solo
   lectura y `MAP_PRIVATE`. [`platform_wayland.c`, `default_dmabuf_feedback_main_device` y
   `default_dmabuf_feedback_format_table`]
2. Con el `dev_t`, libdrm busca el nodo de render y Mesa lo abre. **Ese `dev_t` tiene que llevar a la v3d**, porque vc4
   no tiene nodo de render. [`platform_wayland.c`, `dri2_wl_init_display_dev`]
3. Aquí no hay kmsro: el controlador v3d reserva los búferes **en v3d**, sin pasar por vc4. Si el compositor marca un
   grupo de formatos como "para escanear" (`SCANOUT`), el búfer sale lineal, pero sigue siendo un BO de v3d.
   [`platform_wayland.c`, `create_dri_image_from_dmabuf_feedback`; `dri2.c`, uso de `__DRI_IMAGE_USE_SCANOUT`; `v3d_resource.c`]
4. Para presentar: exporta el BO como dma-buf y crea un `wl_buffer` con `zwp_linux_buffer_params_v1`: por cada plano,
   descriptor, desplazamiento, paso y modificador en dos mitades de 32 bits. Luego `create_immed` con ancho, alto y
   formato de DRM. Con la versión 5 añade el `dev_t` con `set_sampling_device`. [`platform_wayland.c`,
   `create_wl_buffer`]
5. El ritmo lo da `wl_surface.frame`. Mesa reutiliza un búfer cuando llega `wl_buffer.release`.
   [`platform_wayland.c`, `throttle` y `wl_buffer_release`]
6. No hay vallas explícitas (7.5).

### 8.3. Vulkan como cliente de Wayland

- La WSI de Vulkan hace lo mismo que EGL con `zwp_linux_dmabuf_v1`. [`wsi_common_wayland.c`]
- Pero la memoria de las imágenes de la ventana la reserva v3dv **en el dispositivo de pantalla**, con el truco de
  6.6, y la importa en v3d. Lo hace siempre que la WSI marca la reserva como "para ventana". [`v3dv_device.c`,
  `v3dv_AllocateMemory` y `device_alloc_for_wsi`]
- Por eso v3dv **necesita un dispositivo de pantalla con KMS** también en Wayland. Sin él, falla una aserción.
  [`v3dv_device.c`, `device_alloc_for_wsi`]
- v3dv acepta presentar en un dispositivo si es el de pantalla o el de render. [`v3dv_wsi.c`,
  `v3dv_wsi_can_present_on_device`]

### 8.4. Vulkan directo a la pantalla (`VK_KHR_display`)

- Usa KMS atómico completo sobre vc4: recursos, conectores, codificadores, planos, propiedades, `drmModeAtomicCommit`,
  `drmModeAddFB2WithModifiers`, `drmCrtcQueueSequence`, `drmSetMaster` y más. [`wsi_common_display.c`]
- Se deduce que es mucho más trabajo que todo lo anterior. Queda para el final (sección 11, fase 4).

### 8.5. Consecuencias para DashOS

- El compositor de S9 compone por CPU en las fases A y B. Para mostrar ventanas de clientes de GPU tiene que **leer**
  sus dma-buf. Le conviene anunciar solo `LINEAR` en `zwp_linux_dmabuf`, porque UIF no lo puede leer el HVS y
  descifrarlo por CPU cuesta. [`hvs.md`, 6.3]
- Un búfer de cliente reservado en v3d no es contiguo. El HVS no lo puede escanear directamente (`hvs.md`, 2.4). Para
  escanearlo sin copiar, tendría que venir del servicio de pantalla, como hace v3dv (8.3).
- El compositor tiene que esperar a la GPU antes de leer (7.5).

## 9. Lo que Mesa supone del sistema

| Tema | Qué supone | Fuente |
|---|---|---|
| Ficheros de dispositivo | `/dev/dri/cardN` y `/dev/dri/renderDN`, que se abren con `open(O_RDWR \| O_CLOEXEC)` | [`loader.c`, `loader_open_device`; `v3dv_device.c`, `try_device`] |
| `stat` y `fstat` | De caracteres, mayor 226; el menor dice si es render | [libdrm, `drmGetNodeTypeFromFd`] |
| sysfs | `/sys/dev/char/226:M/device/{drm,subsystem,uevent}` | [libdrm, sección 2.1] |
| `mmap` | `MAP_SHARED`, lectura y escritura, sobre el descriptor del nodo con un desplazamiento falso; y `MAP_PRIVATE` de solo lectura sobre un descriptor de Wayland | [`v3d_bufmgr.c`; `platform_wayland.c`] |
| Tamaño de página | `sysconf(_SC_PAGESIZE)`. Gallium alinea la memoria de baldosas y el cargador de estado a ese tamaño; v3dv y otras partes dan por hecho **4096** | [`v3d_device_info.c`, `os_get_page_size`; `v3dv_device.c`, `page_size = 4096`] |
| Memoria total | `sysconf(_SC_PHYS_PAGES)` y `_SC_PAGESIZE`; memoria libre leyendo `/proc/meminfo` (`MemAvailable`) | [`util/os_misc.c`] |
| Hilos | Hilos de C11 sobre pthreads: mutex, condiciones, hilos. v3dv crea un hilo de envío por cola | [`v3dv_device.c`, `queue_init`] |
| `futex` | Opcional. Sin él, los cerrojos rápidos de Mesa usan mutex normales | [`util/futex.h`, `UTIL_FUTEX_SUPPORTED`; `util/simple_mtx.h`] |
| Relojes | `clock_gettime(CLOCK_MONOTONIC)` y `clock_getres` | [`v3d_bufmgr.c`; `v3dv_device.c`] |
| `poll` | Sobre sync_file | [`util/libsync.h`] |
| `fcntl` | `F_DUPFD_CLOEXEC`; `F_DUPFD_QUERY` o la llamada `kcmp` para saber si dos descriptores son el mismo fichero | [`v3d_fence.c`; `util/os_file.c`, `os_same_file_description`] |
| `lseek` | Sobre dma-buf, para saber el tamaño | [6.5] |
| Pasar descriptores | Por el socket de Wayland (`SCM_RIGHTS`) | [protocolo Wayland] |
| ELF | v3dv busca la nota `build-id` de su propia biblioteca con `dladdr` y `dl_iterate_phdr`. **Sin ella no crea el dispositivo** | [`v3dv_device.c`, `init_uuids`; `util/build_id.c`] |
| Carga de bibliotecas | El cargador de Vulkan abre el controlador con `dlopen` | Conocido; no se leyó el cargador |
| Caché de sombreadores | Ficheros en el directorio de caché del usuario; se puede apagar con `MESA_SHADER_CACHE_DISABLE` | [`util/disk_cache*`] |
| Variables de entorno | `getenv` para opciones de depuración | Varias |

## 10. Orden de bytes

### 10.1. Lo que dice Mesa

- **El núcleo de Mesa admite big-endian.** Tiene `UTIL_ARCH_BIG_ENDIAN` y las notas de versión recogen arreglos para
  big-endian en llvmpipe, r300, formatos y GBM (por ejemplo, 26.2.5 y 24.2.6). [`util/u_endian.h`; `docs/relnotes/`]
- **v3d y v3dv no tienen ni una línea sobre el orden de bytes.** Se buscaron `endian`, `BIG_ENDIAN`, `bswap` y
  `cpu_to_le` en `src/broadcom/` y `src/gallium/drivers/v3d/`: cero resultados. Mesa no los prohíbe en big-endian
  al configurar: tampoco hay nada en sus ficheros de compilación.
- Se deduce que **compilarían en big-endian, pero darían datos al revés a la GPU**, que lee en little-endian
  (`v3d.md`, 13.2).

### 10.2. Dónde dan por hecho little-endian (comprobado)

| Sitio | Qué hace | Fuente |
|---|---|---|
| Empaquetado de paquetes de las listas | Casi todo se escribe byte a byte con desplazamientos: eso sale bien en cualquier CPU. **Pero** los campos de 32 bits alineados y solos (enteros y coma flotante) se copian con `memcpy` de la palabra nativa | [`broadcom/cle/gen_pack_header.py`, caso especial "floats" y enteros de 16/32 bits] |
| Palabras sueltas en las listas | `cl_u16`, `cl_u32`, `cl_aligned_u32` y `cl_aligned_f` escriben la palabra nativa. Se usan para direcciones y para el flujo de uniformes | [`v3d_cl.h`; `v3dv_cl.h`] |
| Código de los QPU | El compilador da instrucciones como `uint64_t` nativos y se copian tal cual | [`v3d_compiler.h`, `qpu_insts`; `v3d_program.c`; `v3dv_pipeline.c`] |
| Resultados de la GPU | Mesa lee contadores de oclusión (32 bits) y marcas de tiempo (64 bits) como palabras nativas | [`v3d_query_pipe.c`] |
| Datos de la aplicación | Vértices, índices, UBO, SSBO y texturas pasan tal cual. La GPU los interpreta en little-endian | Se deduce |

### 10.3. Qué quiere decir para DashOS

- **El objetivo "portar Mesa sin tocar v3d ni v3dv" solo se cumple si Mesa corre en un proceso little-endian.**
- La CPU lo permite: AArch64 deja que los procesos de usuario usen little-endian aunque el kernel sea big-endian
  (`ID_AA64MMFR0_EL1.BigEnd = 1`, ver S1). Es lo mismo que necesita la caja Linux de S7.
- Opción A, **procesos de GPU en little-endian**: Mesa, relibc y la aplicación se compilan para `aarch64` (little).
  Mesa no cambia. El servicio de la GPU (big-endian) tiene que dar la vuelta a los campos de cada mensaje que le
  llegue de un proceso así. Las aplicaciones nativas big-endian de DashOS no podrían enlazar Mesa directamente.
- Opción B, **Mesa en big-endian**: hay que cambiar el generador de paquetes, las escrituras de palabras, la subida
  de código y la lectura de resultados. Lo difícil son los datos de la aplicación: un búfer de vértices o un SSBO son
  bytes opacos para el controlador. Habría que convertirlos al subirlos, o hacer que el compilador dé la vuelta a cada
  acceso a memoria de los sombreadores. No se conoce ningún bit de la V3D que lo haga por hardware (`v3d.md`, "Lo que
  no está claro", 1).
- Se deduce que la opción A es la única que cumple el encargo. La B es un port de Mesa grande, fuera de este
  documento. La decisión es de diseño (S7, S9, S12).

### 10.4. Datos que escribe o lee el lado del kernel

- Los trabajos de CPU (marcas de tiempo, copias de consultas, cómputo indirecto) leen y escriben en BO.
  Lo que escriben lo leen después Mesa o la aplicación con la CPU. Lo que lee el cómputo indirecto puede venir de la
  aplicación o de la GPU.
- Con la opción A todo es little-endian: el servicio escribe y lee esos valores en little-endian. Con la B habría que
  decidir caso por caso.

## 11. Propuesta: la interfaz mínima de DashOS

### 11.1. Forma

Se propone **imitar DRM dentro del proceso**, sin tocar Mesa:

- **Un servicio de GPU** (proceso, S1 y S12) que lleva la V3D según `v3d.md`. Gestiona la MMU, los BO, las colas,
  las vallas y los trabajos de CPU.
- **Un servicio de pantalla** que lleva el HVS y el HDMI según `hvs.md` y `hdmi.md`. Para Mesa basta con que reserve
  búferes contiguos ("dumb"), los exporte y los importe.
- **Una libdrm de DashOS** en cada proceso cliente. Ofrece las mismas funciones que libdrm, con otra parte interna:
  - Enumeración (`drmGetDevices2`, `drmGetDevice2`, `drmGetDeviceFromDevId`, `drmGetNodeTypeFromFd`...) sin sysfs:
    pregunta al sistema qué servicios gráficos hay y rellena los `drmDevice` con bus `PLATFORM`.
  - `drmIoctl` traduce cada número de llamada de las secciones 4 y 5 a un mensaje por el canal del servicio, copiando
    lo apuntado (5.4).
- **En relibc**, lo que libdrm no cubre:
  - `open` de `/dev/dri/*` da un descriptor que es una capacidad del canal del servicio.
  - `fstat` de esos descriptores da un dispositivo de caracteres con mayor 226 y su menor.
  - `ioctl` reenvía a la libdrm de DashOS (por `VC4_GET_PARAM`, `SYNC_IOC_*` y `DMA_BUF_IOCTL_*`, que Mesa llama sin
    libdrm).
  - `mmap` con un desplazamiento falso de BO pide al servicio el objeto de memoria (S3) y lo proyecta. Tiene que ser
    memoria no cacheable (6.3).
  - Los dma-buf, sync_file y syncobj exportados son descriptores que envuelven capacidades. Viajan por Wayland como
    capacidades (S9).
  - `lseek` al final de un dma-buf da su tamaño. `poll` sobre un sync_file espera su valla.
- Ventaja: Mesa, libEGL, GBM y el cargador de Vulkan no cambian. Solo se escribe libdrm de DashOS y unas piezas de
  relibc.
- Alternativa: un sysfs y un `/dev` falsos, como el shim de Mesa. Se descarta: obliga a imitar ficheros de Linux sin
  necesidad.

### 11.2. Fase 1: un triángulo con OpenGL ES, sin pantalla

Meta: EGL "surfaceless", dibujar en un búfer fuera de pantalla y leer los píxeles con `glReadPixels`.

| Pieza | Qué hace falta | Puede faltar |
|---|---|---|
| Enumeración | Un dispositivo de plataforma con nodo de render; nombre `"v3d"` en `VERSION` | El nodo primario |
| `GET_PARAM` | `CORE0_IDENT0`, `CORE0_IDENT1`, `HUB_IDENT3` | Todo lo demás: responder con error o 0 |
| `CREATE_BO` | Sí | — |
| `MMAP_BO` y `mmap` | Sí, coherente | — |
| `GEM_CLOSE` | Sí | — |
| `WAIT_BO` | Sí, con `ETIME` | — |
| `SUBMIT_CL` | Sí, con `in_sync_bcl`, `in_sync_rcl`, `out_sync`, `qma`, `qms`, `qts`, lista de BO; sin extensiones | `FLUSH_CACHE` si `SUPPORTS_CACHE_FLUSH` = 0; `perfmon_id` |
| syncobj | `CREATE` (con señalado), `DESTROY`, `WAIT` (`WAIT_ALL`, plazo absoluto, 0 para mirar) | — |
| sync_file | `HANDLE_TO_FD` con exportar sync_file; `FD_TO_HANDLE` con importar; `SYNC_IOC_MERGE` | Solo si la aplicación usa vallas de EGL |
| `SUBMIT_TFU` | Recomendado | Si falla, gallium copia de otra forma |
| `GET_BO_OFFSET` y PRIME | No | Sí |
| Sistema | Hilos, `mmap`, `sysconf`, relojes, `fstat` | `futex` |

- Con `SUPPORTS_CSD`, `SUPPORTS_CACHE_FLUSH`, `SUPPORTS_PERFMON`, `SUPPORTS_MULTISYNC_EXT` y `SUPPORTS_CPU_QUEUE` a 0,
  gallium arranca sin cómputo, sin imágenes, sin contadores y sin consultas de tiempo. Basta para OpenGL ES 2 y un
  triángulo. Se deduce de 4.2; no se ha probado.
- Antes hay que resolver el orden de bytes (10.3).

### 11.3. Fase 2: OpenGL ES en la pantalla

| Pieza | Qué hace falta |
|---|---|
| PRIME en v3d | `PRIME_HANDLE_TO_FD`, `PRIME_FD_TO_HANDLE` (misma manija si se repite), `GET_BO_OFFSET`, `lseek` |
| Sincronía implícita | Que el compositor pueda esperar a la última escritura de la GPU en un búfer (7.5) |
| Wayland (cliente) | `zwp_linux_dmabuf_v1` versión 4 o más, con un `dev_t` que lleve a la v3d (8.2) |
| GBM sobre la pantalla (si el compositor pinta con la GPU) | Dispositivo `"vc4"` con nodo primario; `VC4_GET_PARAM` que falle; `MODE_GETRESOURCES` para `drmIsKMS`; `MODE_CREATE_DUMB` y `DESTROY_DUMB`; PRIME en los dos sentidos (8.1) |
| Capacidades de v3d | `SUPPORTS_TFU`, `SUPPORTS_CACHE_FLUSH` y `SUPPORTS_CSD` para OpenGL ES 3.1 |

### 11.4. Fase 3: Vulkan

Todo lo anterior, y además:

| Pieza | Qué hace falta |
|---|---|
| Capacidades obligatorias | `SUPPORTS_TFU`, `SUPPORTS_CSD`, `SUPPORTS_CACHE_FLUSH`, `SUPPORTS_MULTISYNC_EXT`, `SUPPORTS_CPU_QUEUE` a 1 |
| Envíos | `SUBMIT_CL`, `SUBMIT_TFU`, `SUBMIT_CSD` y `SUBMIT_CPU`, todos con la cadena de extensiones y la de varias vallas (4.11) |
| Trabajos de CPU | Cómputo indirecto y las cinco de consultas (4.10) |
| syncobj | Además: `RESET`, `SIGNAL`, `TRANSFER` (con puntos, 7.3), `WAIT_FOR_SUBMIT`, exportar e importar como descriptor opaco |
| Regla de exportar | Exportar un syncobj sin valla como sync_file tiene que fallar (7.3) |
| `GET_CAP` | `DRM_CAP_SYNCOBJ_TIMELINE` puede dar 0 |
| Nodos | Render y primario de v3d, y primario de pantalla con KMS (2.2 y 8.3) |
| ELF | Nota `build-id` en la biblioteca de v3dv; `dladdr` y `dl_iterate_phdr` en relibc |
| dma-buf | `DMA_BUF_IOCTL_EXPORT/IMPORT_SYNC_FILE` recomendados; pueden dar `ENOTTY` |

### 11.5. Fase 4: lo que puede esperar

- Contadores de rendimiento (4.12) y `GET_COUNTER`.
- Contadores de reinicios de la GPU.
- `VK_KHR_display` con KMS atómico (8.4).
- `GEM_FLINK` y `GEM_OPEN`: no hacen falta sin X11.
- `PERFMON_SET_GLOBAL`: solo para Perfetto.

## Lo que no está claro

1. **Orden de bytes.** El documento muestra que v3d y v3dv dan por hecho little-endian. No se ha medido cuánto costaría
   la opción B (10.3), ni si la opción A encaja con el resto del diseño de DashOS (aplicaciones Dash nativas que
   quieran OpenGL ES).
2. **Valla implícita.** En Linux, cada envío ata su valla a todos los BO de su lista, y los envíos de otros clientes
   esperan a ella. Mesa lo necesita para los búferes compartidos (7.5). No se ha leído en el kernel cómo trata Linux
   lectura y escritura en esa atadura: está fuera de las fuentes de este documento y no lo cubre `v3d.md`.
3. **VA privada o compartida.** La cabecera dice que la VA es privada del fichero, pero la V3D tiene una sola tabla.
   Con una tabla para todos, un proceso puede leer con la GPU la memoria de otro. No se sabe si la V3D 4.2 permite
   varias tablas.
4. **Ceros en BO nuevos.** Mesa no borra los BO al crearlos. Se da por hecho que el kernel los da a cero. No está
   escrito en Mesa.
5. **Exportar un syncobj sin valla.** Mesa espera que falle (7.3). Se deduce del bucle de Mesa; no está escrito.
6. **`TRANSFER` con puntos.** Mesa usa puntos de línea de tiempo en un syncobj temporal aunque los timelines estén
   apagados (7.3). No se ha comprobado si ese camino se recorre en la práctica con v3dv en Wayland.
7. **El `dev_t` del compositor.** En Linux, los compositores de la Pi 4 anuncian el de vc4 o el de v3d según cómo
   estén hechos. Mesa necesita que lleve a un nodo de render. No se ha visto qué hace Mesa si el `dev_t` es el de vc4
   y no tiene nodo de render; parece que se queda sin dispositivo.
8. **"compat_rev".** Mesa lee los bits 23:16 de `HUB_IDENT3` y Linux no los describe (`v3d.md`, sección 3).
9. **Tamaño de página distinto de 4 KB.** Gallium usa el tamaño de página del sistema en algunos sitios y 4096 en
   otros. Con páginas mayores podría haber incoherencias. Se recomienda páginas de 4 KB para la memoria de la GPU.
10. **Cache de shaders y `dlopen`.** No se leyó el cargador de Vulkan ni la caché de disco a fondo. Lo de la tabla de
    la sección 9 sobre ellos es conocido, no comprobado línea a línea.
11. **`MODE_GETRESOURCES` y `MODE_GETCONNECTOR`.** Solo se dan sus nombres. Sus estructuras están en `drm_mode.h` y no
    se han detallado aquí, porque solo hacen falta para `drmIsKMS` (contar) y para `VK_KHR_display`.
12. **Valores reales de identificación.** Los de 4.2 son del shim de Mesa. Hay que leerlos de una Pi 4.
13. **Nada se ha ejecutado.** Ni Mesa compilado para AArch64, ni una prueba en hardware.
