# Bitácora: programas Amiga de verdad en la caja Amiga (rama `amiga-apps`)

Rama `amiga-apps`, desde `revision` (ba0c2a2e). Lo pidió el usuario: "Instala también ProTracker de Amiga, DOpus5,
LightWave 5, etc." en la caja Amiga, y después Tachyon, Forja y Hangar, con red de verdad, sonido, medidas y la causa
de lo que falle. Nada del Amiga (sistema, programas, ROMs, módulos) entra en git: se instala en una copia del AmigaOS
del usuario fuera del repositorio (`~/.cache/dash-emu/sys32mui-apps`).

## 2026-11-17: cómo se prueban los programas

### La copia del sistema

- `~/recuperar/geosian-7d/amiga-apps/instalar.sh` (fuera de git) rehace `~/.cache/dash-emu/sys32mui-apps` desde
  `sys32mui` (no toca el original) y pone en `Apps/`: ProTracker 4.1 (Aminet), Directory Opus 5 (APL; su
  `dopus5.library` también en `Libs/`), LightWave 5.0r (copia del usuario, con `amiga.lwo`), Tachyon (lo que copia su
  `Instalar`, sin las versiones viejas), Forja (la `forja-*.lha` más nueva), Hangar (`build/paquete/Hangar`),
  MultiView (del AmigaOS 3.1 del usuario) y lo de Aminet bajado a `~/recuperar/geosian-7d/amiga-apps/aminet`: OctaMED
  SoundStudio 1.03c (freeware desde 1998, lo dice su readme), HippoPlayer 2.45 y su actualización 2.67 (freeware),
  dos buscaminas (AMines 1.2 y AmiMineSweeper 1.0, de Aminet) y un módulo (`mods/pop/Elysium`).
- Personal Paint 7: en Aminet solo están sus actualizaciones y catálogos (el programa entero es de Cloanto, "Only
  Cloanto, Aminet and Aminet CD-ROM", y no está allí): no se instala.

### El lanzador y las escenas

- La caja todavía es una batería de pruebas: los programas del usuario entran por `DASHOS_PROGRAMAS_AMIGA` y la caja
  los ejecuta uno tras otro (paso 8b4 de `caja.c`), sin argumentos, 6 s (30 s con la caja de las capturas) y luego
  les cierra las ventanas; a los 10 s más, el vigilante los corta.
- `caja/pruebas68k/lanzador.c` (nuevo; no va en `caja.c`): un programa 68k que lee un guion
  (`PROGDIR:lanzador/<su nombre>.txt`) y hace lo que haría una persona en un Shell: asignaciones, variables, el cajón,
  `lanza programa argumentos` (LoadSeg y CreateNewProc con su CLI), esperas y marcas para el guion de las capturas.
  Espera a sus hijos (si acabara antes, se los llevaría). Dice cada paso con su tiempo: así se mide cuánto tarda cada
  programa en abrir.
- Las escenas, en `tools/amiga_apps/<escena>.txt`; cada una es una copia del lanzador con ese nombre.
- `make amiga-apps` (en `kernel/`): la imagen de la demo (`uso` con la caja de las capturas) sin el disco de la caja
  Linux, el disco de pruebas con la copia del sistema y las escenas, y `tools/amiga_apps.py correr`: entra Ana, pone
  el perfil de shiva, contesta "no" a la guardia y saca una captura en cada "marca captura" del lanzador. Salida en
  `~/recuperar/geosian-7d/amiga-apps/vuelta/` (fuera de git).

### Lo que se vio en las tres primeras vueltas (sin red real ni sonido aún)

| Programa | Qué pasa | Por qué | Arreglo |
|---|---|---|---|
| LightWave 5.0r (Layout, FPU) | Abre a pantalla completa con su rejilla y sus botones, sin el texto de los botones | `PrintIText` no estaba (104 llamadas), ni `AddFont`, `GetPrefs`, `NewModifyProp` | Hechas en intuition y graphics nativas, con pruebas (`intuition/tests/amiga_apps.rs`) |
| Tachyon | Su página de inicio; con red, "timed out connecting" | No había `bsdsocket.library`; con la nativa, `red_cliente` hablaba en el orden de bytes del procesador y la caja es big-endian | bsdsocket nativa (`caja/bsdsocket`) y `red_cliente` en little-endian siempre |
| Forja | Abre con su árbol del proyecto ("Cannot start the extension Git / Inferencia": son suyas) | - | - |
| Hangar | Abre; "no bsdsocket.library (a TCP/IP stack is missing)" | Lo mismo que Tachyon | Lo mismo |
| Directory Opus 5 | Tumba la caja entera nada más arrancar ("excepción de clase 0x24, FAR 0x34") | Su arranque pone `RNF_WILDSTAR` en `DOSBase->dl_Root->rn_Flags` y la dos de la caja tenía `dl_Root` a 0: escribía en la dirección $34 (página 0, de solo lectura: S28 mata la caja entera) | `dl_Root` con su `RootNode`, `DosInfo` y `rn_CliList` (`poner_raiz_de_dos`). Se averiguó con un volcado de registros nuevo del kernel al terminar un proceso por un fallo (x18 es el PC del 68k al empezar el bloque del JIT) y el desensamblado del programa |
| ProTracker 4.1 | Acaba en 15 ms | Abre `audio.device` (no había) y luego `AutoRequest` (no estaba) | `AutoRequest` en intuition (sobre el requester de `EasyRequestArgs`); audio.device nativo (`caja/sonido`). Además programa la CIA-B a mano (abajo) |
| AMines 1.2 (juego) | Su ventana, vacía | `DrawImage` no estaba (480 llamadas: cada casilla) | `DrawImage` planar en intuition, con prueba |
| MultiView (3.1) | Acaba sin ventana | Pide `asl.library` (la de 3.2 usa una privada de exec) | Pendiente: asl.library nativa |
| HippoPlayer 2.67 | Acaba sin ventana | Pide `reqtools.library` (de Aminet: instalada en la copia) y luego `DisplayAlert` | reqtools en la copia; `DisplayAlert`, pendiente |
| OctaMED SoundStudio | Acaba en 15 s sin ventana | Sin averiguar aún | - |

- **ProTracker y la CIA**: tras `OpenResource("ciab.resource")` y `AddICRVector`, escribe el periodo del temporizador
  en `$BFD400`/`$BFD500` y arranca con `$BFDE00` (desensamblado, `$26266`-`$262C0`). En la caja no hay CIA: hasta ahora
  esas escrituras iban a la memoria del 68k (la de otro, si la memoria había crecido hasta ahí) o mataban la caja.
  Arreglo de fondo en exec: lo que crece hasta `$A0_0000`-`$FF_FFFF` (CIA, chipset, ROM) se reserva para el sistema y no
  se da nunca (`HUECO_DEL_CHIPSET`, con su prueba): esas escrituras caen en memoria de nadie, que una CIA imitada podrá
  leer como registros.
- **Fallo de una herramienta ajena**: `dashfs-ar` se cae (`unwrap` en `archive.rs:111`) con un nombre de fichero que no
  es UTF-8 (los `.lha` de Aminet traen nombres en Latin-1). `instalar.sh` los pasa a UTF-8; el fallo de `dashfs-ar`
  queda para quien lleve `archivos` (debería dar un error, no caerse).

## 2026-11-18: lo nativo nuevo y los arreglos

### Bibliotecas, dispositivos y recursos nativos nuevos (cada uno con sus pruebas de cargo)

- **bsdsocket.library** (`caja/bsdsocket`, de un ayudante; interfaz AmiTCP 4 y lo más usado de Roadshow): cada
  `OpenLibrary` da su base (cambio pequeño en exec: `anadir_base_propia`), `WaitSelect` con las señales de exec y
  sin parar a las demás tareas 68k (Bloquear y un sondeo `atender` en cada llamada nativa y en las esperas de la caja),
  errno por puntero de 1, 2 y 4 bytes, `gethostbyname` con su `hostent` en la memoria del 68k. Sobre el servicio de
  red de DashOS (`pegamento/src/red.rs`, la llave "red" que ahora da `inicio` a la caja); escuchar (servidores) no:
  la caja no tiene el permiso `red-escuchar`.
- **`red_cliente`** (usuario, compartido): `Conexion::empezar`, `Udp::empezar`, `resolver_empezar` (que no esperan:
  la caja corre todas sus tareas en un hilo) y `Oido::aceptar` con plazo 0 que mira una vez. Y un fallo de fondo: hablaba
  con el servicio en el orden de bytes del procesador; la caja es big-endian y el servicio lee little-endian, así que
  sus órdenes llegaban al revés (Tachyon: "timed out connecting"). Ahora siempre en little-endian (en los programas
  little-endian no cambia nada).
- **icon.library y workbench.library** (`caja/icono`, de un ayudante): `.info` clásicos, NewIcons, los de color de 3.5 y
  PNG (sin su trozo `icOn`: no hay documentación pública), tooltypes, iconos de serie dibujados aquí, objetos App en
  una tabla. Los ficheros, por la dos de la caja (con su guardia).
- **ahi.device, la biblioteca de AHI y audio.device** (`caja/sonido`, de un ayudante, sobre el corazón de
  `audiodevice/`): por el servicio de sonido (`pegamento/src/sonido.rs`, la llave "sonido"; el flujo se abre la primera
  vez que suena algo). Los ganchos de AHI (el jugador de un reproductor, el fin de una vuelta) se ejecutan en 68k desde
  las llamadas nativas, la interrupción de nivel 6 de la caja (que ahora llega también con una sola tarea si suena algo)
  y las esperas.
- **ciaa.resource y ciab.resource** (`pegamento/src/cia.rs`): `AddICRVector`, `RemICRVector`, `AbleICR`, `SetICR`, y los
  temporizadores A y B leídos de donde los escribe el programa (el hueco del chipset, que `AddICRVector` hace existir
  con `cubrir_hasta` de exec); su interrupción se llama a ese ritmo (reloj E de 709.379 Hz). Para ProTracker y OctaMED.
  Prueba en la caja: `caja/pruebas68k/cia_prueba.c` (escena `cia`).
- **mathffp.library** (`pegamento/src/ffp.rs`, del autodoc): la coma flotante rápida de Motorola (LightWave). Prueba
  con `rustc --test`. El autodoc y el `.fd` no dicen lo mismo de los registros de `SPCmp`: se sigue el `.fd` y la
  biblioteca FFP de Motorola (D0 con D1).
- **console.device e input.device** (`pegamento/src/dispositivos.rs`): `RawKeyConvert` (= `MapRawKey` de keymap),
  `CDInputHandler`, `PeekQualifier`, `IND_ADDHANDLER` (se apunta; la caja aún no pasa sus eventos por una cadena de
  manejadores de 68k). Una consola en ventana (`CON:`) no está.
- **intuition y graphics**: `AutoRequest`, `PrintIText`, `GetPrefs`/`GetDefPrefs`, `AddFont`/`RemFont`,
  `ModifyProp`/`NewModifyProp` (el valor; los gadgets de proporción clásicos aún no se pintan), `DrawImage` (planar) y
  `DisplayAlert`/`TimedDisplayAlert` (en un requester). Pruebas en `intuition/tests/amiga_apps.rs`.
- **dos**: `DOSBase->dl_Root` (la `RootNode`, que tenía 0: Directory Opus escribía en $34) y el cargador hunk admite un
  hunk que empieza sin el `HUNK_END` del anterior, como AmigaDOS (el ejecutable comprimido de OctaMED daba
  `ERROR_BAD_HUNK`).
- **exec**: el hueco del chipset reservado al crecer, `cubrir_hasta` y `crear_recurso_nativo`.
- **El kernel** dice los registros del proceso cuando lo termina por un fallo (dos líneas más tras "excepción de clase").
- **La traza de llamadas nativas**: `variable traza-amiga NombreDeTarea` en una escena (o `*`): cada llamada con su
  biblioteca, su LVO, los registros, quién la hace y lo que devuelve.
- **asl.library** (`pegamento/src/asl.rs` y el pedidor de `intuition/src/pedidor.rs`): el pedidor de ficheros de
  verdad, una ventana de la caja pintada por el tema (la lista del cajón con los cajones primero, el cajón y el fichero
  en dos campos, "Aceptar"/"Volúmenes"/"Padre"/"Cancelar"; doble clic entra o acepta, Intro, Esc, Tab y las flechas),
  con los cajones leídos por la dos de la caja (`listar_cajon`, con las asignaciones múltiples y su guardia). Etiquetas:
  título, texto de aceptar, cajón, fichero y patrón iniciales, modo de guardar, solo cajones, sin `.info`, `UserData`.
  El de modos de pantalla da el único modo de la caja; el de fuentes aún no (FALSE, apuntado como "falta").
- **`InitRequester`, `Request` y `EndRequest`** (Hangar, por MUI: el requester vacío de "ocupado"): la ventana deja de
  atender al ratón dentro, `ReqCount`, `IDCMP_REQSET`/`REQCLEAR`. Lo que dibuje un requester clásico con gadgets aún no se
  pinta.

### Para la sesión de `sonido-pro`

La caja Amiga es ya un cliente más del servicio de sonido: con la llave "sonido" (nueva en `kernel/src/inicio.rs`)
abre su propio flujo (estéreo, 16 bits, 48 kHz, anillo de 32 KB en `pegamento/src/sonido.rs`) la primera vez que suena
algo por ahi.device o audio.device, y mezcla con 40 ms por delante. No cambia nada del servicio. Si el protocolo del
flujo cambia, hay que cambiar también `sonido.rs` (escribe la cabecera y las muestras en little-endian a mano, porque
la caja es big-endian).

### Cambios que harían falta en `caja/caja.c` (no lo toco: lo lleva otra sesión)

1. **El plazo de cada programa del usuario** (paso 8b4): hoy son 6 s (30 con `CAJA_INTERACTIVA`) y luego se le cierran
   las ventanas y, 10 s después, el vigilante lo corta. Para usar de verdad un programa (renderizar en LightWave, cargar
   un módulo y escucharlo, navegar) hace falta más: que la línea de `SYS:C/Prefs/programas-amiga` pueda llevar sus
   segundos (`ruta segundos`). El cambio: `caja_programa_de_disco` (pegamento) devolvería también esos segundos (un
   parámetro de salida más) y 8b4 los usaría en lugar de 30000/6000 en `SC_HILO_CREAR` de `manda_ctrl_c`. Mejor aún,
   un modo "de uso" de la caja que no sea una batería de pruebas (lanzar programas a petición del explorador o del
   Shell), que es lo que pide la demo.
2. **El hueco del chipset en el crecimiento** (`caja_ampliar_ram`): exec ya no da la memoria de `$A0_0000`-`$FF_FFFF`,
   pero `caja_ampliar_ram` la proyecta igual (es memoria que se gasta, 6 MB, en cuanto la caja pasa de 10 MB). Lo
   ideal sería no proyectarla y que un acceso ahí lo atendiera la caja (las CIA y Paula imitadas, S18 fase 7), pero
   eso necesita que el kernel entregue los fallos de página de un proceso a un manejador suyo (hoy los termina).
3. **Un fallo de página de un programa mata la caja entera** (S28: la página 0 es de solo lectura y cualquier otro
   acceso fuera de la memoria del 68k termina el proceso). Con varios programas a la vez, un `NULL->campo = x` de uno
   se lleva a todos. Arreglarlo bien necesita lo mismo del kernel (entregar el fallo y seguir), y en la caja, quitar
   solo la tarea que lo hizo.

### Un fallo que metí y su causa: las llaves de los canales cambian de número

Con "red" y "sonido" en la lista de canales de la caja, `memh` dejó de recibir el aviso de memoria baja ("no llegó el
aviso en 90 s"). `inicio` da las llaves de los canales en el orden en que se nombró cada canal por primera vez en su
lista (el bit de cada canal), no en el de la lista del programa: "red" y "sonido" los nombran antes sus servicios, así
que "presion" dejó de ser la llave 5 (`aviso.rs` la tenía fija). Ahora se busca por su nombre en la tabla de llaves
(`dashos::llaves::buscar("presion")`), como ya hacían la red y el sonido. Las 2, 3 y 4 (ventanas, archivos, sesiones)
no se mueven.

### Pruebas de la tanda (commits c2ca78d3 a ca5e6897)

- `cargo test` de exec (145), dos (87), intuition (168), timer, utility, audiodevice (18), sonido (25), bsdsocket (29) e
  icono (28): bien; la de mathffp con `rustc --test` (ahora dentro de `make prueba`): bien.
- `make prueba`: BIEN (cargador hunk y esperado.txt).
- `make arranque-virt`: todo lo de la caja Amiga bien (147 líneas "bien", ninguna "MAL"). La vuelta entera falló dos
  veces, cada una en una línea distinta y ajena: "[explorador] ventana 1 en SYS:Programas" y la de los timbres de la
  prueba de `RAM:` ("llegan 16 timbres" en vez de 63), con la máquina a carga 8-11 (otras QEMU a la vez). Antes, con
  la llave de "presion" mal, fallaba `memh` (arreglado arriba). No he conseguido una vuelta entera en verde por esos
  dos fallos de tiempo; lo apunto como no comprobado.
