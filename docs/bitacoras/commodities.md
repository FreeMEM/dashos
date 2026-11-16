# Bitácora: Commodities y Exchange (S10, paso 7 de `docs/REVISION_LOOP.md`)

Rama `commodities`, sacada de `revision` en 2344aad5. Diseño de partida: S10 ("Commodities y Exchange", decidido el
2026-11-15), S10b (poderes del compositor, atajos reservados, rescate), S6b (permisos con su frase en llano) y S0 ("lo
de la ventana, también por orden o guion").

## 2026-11-16: diseño corto

### La idea

En el Amiga una commodity se engancha a toda la entrada. Aquí nadie la recibe: la commodity pide al compositor lo
concreto, el compositor comprueba su permiso y le pasa solo lo suyo. Exchange es un cliente más que ve la lista y la
manda, y la misma lista se maneja desde el Shell con la orden `Exchange`.

### El protocolo: uno nuevo, `dashos_commodities_v1`, no una extensión del tema

`dashos_tema_v1` lo ven todos los programas y sus peticiones se reparten por el nombre que da `inicio` (centro, barra,
panel). Las commodities son programas del usuario, lanzados del disco, con permisos que se conceden y se quitan. Por
la regla de S10b ("un cliente sin el poder no ve el protocolo"), van en un global aparte que el compositor solo anuncia
a quien tiene alguno de los tres permisos de abajo. Pedirlo sin anunciarse es "global que no existe", y se corta.

- `dashos_commodities_v1` (global): `registrar` (una commodity nueva, con nombre, título y descripción) y `vigilar`
  (un Exchange).
- `dashos_commodity_v1`: `atajo(número, combinación)`, `quitar_atajo`, `inactividad(segundos)`, `oscurecer`,
  `comportamiento(cuál, sí/no)`. Eventos: `pulsado`, `atajo_negado(número, motivo, quién)`, `activo(sí/no)`,
  `ensenar`, `quitar`, `inactivo(sí/no)` y `retirado(permiso)`.
- `dashos_exchange_v1`: `activar(id, sí/no)`, `ensenar(id)`, `quitar(id)`, `ajuste(cuál, valor)`. Eventos:
  `commodity` (alta o cambio), `atajo` (cada combinación y su estado), `fuera`, `ajuste` y `listo` (tras la lista
  inicial).

Al hacerlo se vio que `dashos-tema-v1.xml` iba por la versión 5 y las tablas por la 7 (`marca` y `apilar` se
añadieron a mano a `tablas.rs`): regenerar las tablas las borraba. Se ponen en el XML, tal como están en las tablas.

### Qué pide cada commodity y con qué permiso

| Commodity | Qué pide al compositor | Permiso |
|---|---|---|
| FKey | Atajos concretos ("Ctrl+Alt+F1"); al pulsarlo, lanza su programa o su guion | `atajos` |
| Blanker | Aviso de inactividad (N segundos); con él, `oscurecer`: el compositor pinta la pantalla de negro hasta la siguiente tecla o movimiento | `fondo` |
| NoCapsLock | El comportamiento "sin Bloq Mayús": el compositor se traga esa tecla mientras la commodity viva y esté activa | `fondo` |
| ClickToFront, AutoPoint | Nada: son **ajustes del compositor** (traer al frente con clic o con doble clic; activar la ventana bajo el ratón). Los cambia Exchange | `commodities` |
| Exchange (ventana y orden) | Ver las commodities de su persona, sus atajos; activar, desactivar, enseñar, quitar; los dos ajustes | `commodities` |

Los permisos, con su frase en llano para la pregunta de `guardian` (la tabla de S6b; los nombres internos van en
"Detalles"):

| Permiso | Qué da | Lo que lee el usuario ("FKey quiere...") |
|---|---|---|
| `atajos` | Registrar combinaciones de teclas concretas y recibir solo esas, aunque la ventana activa sea otra | "usar teclas rápidas en todo el sistema" |
| `fondo` | Saber cuándo no se usa el ordenador (desde 10 s), apagar la pantalla mientras tanto y cambiar comportamientos del teclado | "saber cuándo no usas el ordenador y apagar la pantalla" |
| `commodities` | Ver y mandar en las commodities de la misma persona; los ajustes ClickToFront y AutoPoint | "ver y manejar tus utilidades de fondo" |

Cómo llega el permiso: la commodity lo pide a `sesiones` como cualquier servicio (`SERVICIO`). Si el paquete (hoy, la
tabla provisional `PERMISOS`) lo trae, o el usuario dice que sí, `sesiones` no da una llave: manda al compositor, sin
marca, `AVISO_PODER` (marca, permisos, dar o quitar; número `0x5536_0010`, alto) y contesta "bien". El aviso va antes
que la respuesta por el mismo canal del compositor, así que cuando la commodity se conecta su permiso ya está. Si ya
estaba conectada, el compositor le anuncia el global en el acto. Quitarlo en Ajustes (`RETIRAR`) manda el aviso de
quitar: el compositor le borra sus atajos y comportamientos en el acto y le dice `retirado`.

### Cómo se ve que un programa tiene un atajo global

- En Exchange (ventana) y con `Exchange LISTA` en el Shell: cada commodity con sus combinaciones, y las que no
  consiguió con el motivo ("Ctrl+Alt+F: la tiene FKey").
- En Ajustes, categoría Permisos, y con la orden `Permiso`: el permiso `atajos` concedido a ese programa.
- El compositor apunta cada atajo puesto y negado en su registro.

### Teclas que no se pueden coger nunca

Se rechazan al pedirlas (`atajo_negado` con motivo "reservado"), aunque se tenga el permiso:

- **Todo lo que lleve Super (la tecla Amiga)**: es del sistema. Hoy: Super sola (lanzador), Super+A, Super+L,
  Super+1..9, Super+flechas, Super+Mayús+flechas, Super y arrastrar; y los atajos del Amiga (Amiga+M y N).
- **El rescate y el reinicio** (S10b, pregunta 9): Ctrl+Alt+Supr. Y Ctrl+Alt+Retroceso, que queda apartado para
  "cerrar la sesión" (en Linux es matar el servidor gráfico).
- **AltGr**: con ella se escriben @, #, €, [ ]... Un atajo con AltGr se comería letras.
- **Las teclas del volumen**: van al panel rápido.
- **Sin Ctrl y Alt a la vez no hay atajo** ("no vale"): una letra, un número o Mayúsculas con algo es escribir, y Ctrl
  o Alt solos son los atajos de los programas (Ctrl+C, Ctrl+Mayús+N, Alt+F4), que una commodity no puede quitarles.
  Primero se pedía Ctrl **o** Alt; la prueba de `virt` lo cazó: sus cien atajos se quedaron con Ctrl+Mayús+N y el
  explorador ya no creaba carpetas. Esto deja fuera las teclas de función solas (F1..F12), que FKey del Amiga usaba:
  pendiente del usuario.

Y aunque la combinación sea buena, **no llega a la commodity** si la sesión está bloqueada, si tiene el teclado una
ventana del sistema (`guardian`, el selector: una contraseña a medio escribir) o si la commodity es de una persona con
la sesión detrás (U6). Entonces la tecla sigue su camino normal.

### Límites (prueba hostil)

- 32 atajos por programa y 256 en total; 4 commodities por programa; 8 Exchange a la vez. De más: `atajo_negado`
  ("demasiados") o error de protocolo para los objetos.
- Dos que piden la misma combinación (de la misma persona): la primera se la queda; la segunda recibe `atajo_negado`
  con motivo "ocupado" y el nombre de quien la tiene, y Exchange lo enseña.
- La commodity que muere con atajos puestos: el compositor los suelta con el cliente (`adios`), y su combinación queda
  libre para otra.
- Inactividad: como poco 10 s (de menos, se sube a 10): con plazos cortos, el aviso de "vuelve a haber actividad"
  diría cuándo se pulsa cada tecla. Una por commodity. Va aparte de la de `guardian` (su tope de 16 no se comparte).
- `oscurecer` solo vale si esa commodity está ahora inactiva (le llegó `inactivo(1)` y no ha habido entrada después):
  un programa no puede apagar la pantalla cuando quiera para molestar.
- Lo que despierta la pantalla (la tecla o el clic) no llega a nadie: lo que no se ve no se pulsa.

### Exchange

- `SYS:Tools/Commodities/Exchange`: ventana con la lista (nombre, título, activa, atajos) y los botones Activar,
  Desactivar, Enseñar y Quitar; y los dos ajustes.
- La orden `Exchange` (en `SYS:C`): `Exchange LISTA`, `ACTIVAR <nombre>`, `DESACTIVAR <nombre>`, `ENSENAR <nombre>`,
  `QUITAR <nombre>`, `CLICKTOFRONT CLIC|DOBLECLIC`, `AUTOPOINT SI|NO`.
- Desactivar: sus atajos no llegan (pero siguen siendo suyos, para que otra no se los quede por sorpresa), sus
  comportamientos se apagan, no recibe inactividad. Se le dice `activo(0)`.
- Quitar: se le pide que se vaya (`quitar`). Si no se va, se para con `Break` (`sesiones`, matar), como cualquier
  programa.
- Solo ve y manda en las commodities de su misma persona.
- Categoría de Ajustes: la misma lista en el centro de control. Se deja para cuando se una `ajustes-texto` (otra rama
  rehace ahora el centro): pendiente.

### Ajustes ClickToFront y AutoPoint

- AutoPoint (de serie, no): la ventana bajo el ratón se lleva el teclado sin subir. Nunca quita el teclado a una
  ventana del sistema ni a la pantalla de bloqueo; no se mueve con un botón pulsado.
- ClickToFront (de serie "clic"): "clic" sube la ventana al pulsarla (lo de hoy); "dobleclic" es el Amiga clásico con
  ClickToFront: el clic da el teclado y el doble clic la sube.
- Se guardan en `Prefs/commodities` (texto: `clicktofront clic`, `autopoint no`), que escriben la ventana y la orden.
  Que el compositor los lea al entrar en la sesión lo hará quien aplique los ajustes al entrar (rama `ajustes-texto`):
  pendiente, dicho en "Lo que queda".

### `commodities.library` en la caja Amiga: diseñada, sin hacer

No cabe bien en este encargo: necesita una biblioteca nativa nueva en la caja (como `caja/intuition` o
`caja/utility`, en Rust), su alta en la lista de `OpenLibrary` de exec, la cadena de entrada de intuition y, para los
atajos globales, que la caja hable `dashos_commodities_v1`. Lo que la conecta con el resto pasa por `caja/caja.c`, que
ahora rehace otra sesión (la interfaz del motor). Se deja diseñada, con lo que se sabe de la documentación pública de
AmigaOS (los autodocs de `commodities.library` y el capítulo de Commodities Exchange del RKM), sin leer AROS ni UAE.

- **Qué es en el Amiga**: un "broker" por programa (`CxBroker` con una `NewBroker`: nombre, título, descripción,
  `NBU_UNIQUE`, `COF_SHOW_HIDE`, prioridad, puerto) y un árbol de objetos colgado de él (`CxFilter` con una descripción
  de entrada que lee `ParseIX`, `CxSender`, `CxSignal`, `CxTranslate`, `CxCustom`), que se activa con `ActivateCxObj`.
  Toda la entrada del sistema pasa por la lista de brokers, por prioridad. Exchange los ve y les manda
  `CXCMD_DISABLE`, `ENABLE`, `APPEAR`, `DISAPPEAR`, `KILL` y `UNIQUE` como mensajes a su puerto.
- **Qué es en DashOS**: la misma API, pero la entrada que corre por la cadena es **solo la de las ventanas de la caja**:
  lo que el compositor manda a una ventana Amiga cuando tiene el teclado o el puntero. Un "registrador de teclas" de
  68k ve lo que se teclea en Amiga, y nada de lo nativo ni de la caja Linux. Lo mismo `input.device` (`AddHandler`).
- **Exchange de 68k** (el de Workbench, Yak, MCP) ve la lista de la caja, con los mismos mensajes.
- **Exchange nativo**: la caja da de alta cada broker como una `dashos_commodity_v1` (nombre del broker, su título y su
  descripción); activar, desactivar, enseñar y quitar desde DashOS se traducen a `CXCMD_ENABLE`, `DISABLE`, `APPEAR` y
  `KILL` en el puerto del broker. Con más de 4 brokers (el tope por programa), uno de alta por cada uno no cabe: la caja
  necesitará un tope propio o agruparlos (por decidir al hacerlo).
- **Atajos globales**: si un filtro es una tecla con calificadores (lo que da `ParseIX` sin clase rara), y la caja
  tiene el permiso `atajos` (se pregunta una vez, como a cualquier programa: "La caja Amiga quiere usar teclas
  rápidas en todo el sistema"), la caja pide esa combinación al compositor. Al llegar `pulsado`, mete el evento en la
  cadena como si se hubiera tecleado en una ventana suya, y el filtro hace lo de siempre. Sin el permiso, o si el
  compositor la niega (reservada, ocupada), el atajo sigue valiendo dentro de las ventanas de la caja.
- **Las teclas del Amiga**: `lalt` y `ralt` son Alt (en el Amiga no hay AltGr); `lcommand`/`rcommand` (Amiga) son
  Super, que es del sistema: esos atajos solo valen dentro de la caja. Muchos atajos de commodities clásicas llevan la
  tecla Amiga: es una pérdida que se dice en Exchange ("solo en las ventanas de la caja").
- **Salvapantallas de 68k**: se apagan en su propia pantalla (S23: una pantalla de Amiga es un espacio de trabajo), sin
  tapar el escritorio. Su inactividad es la de la entrada de la caja, o la del compositor si la caja tiene `fondo`.
- **Pruebas**: de oráculo (regla 5 de `AGENTS.md`) para `CxBroker`, `ParseIX`, `MatchIX`, los mensajes de Exchange y el
  orden por prioridad; hostil: un 68k que registra un filtro de todas las teclas no ve lo tecleado en una ventana
  nativa (el guion teclea una contraseña en `guardian`), y un broker con atajo global de un 68k sin el permiso no
  salta fuera de la caja.
- **Fases**: 1, la biblioteca con la cadena local y Exchange de 68k; 2, el alta en el Exchange nativo; 3, los atajos
  globales con el permiso.

## 2026-11-16: lo hecho

### El compositor (`wayland/`, `usuario/src/bin/compositor.rs`)

- `wayland/protocolos/dashos-commodities-v1.xml` y sus tablas. Y `dashos-tema-v1.xml` puesto al día con `marca` (v6)
  y `apilar` (v7), que solo estaban en `tablas.rs`: regenerar ya no las borra.
- `wayland/src/atajos.rs`: leer y escribir combinaciones ("Ctrl+Alt+F", "ctrl alt f"), qué se puede coger y los
  modificadores por lado (el Alt derecho es AltGr). Sin nada del compositor: lo usan FKey y Exchange.
- `wayland/src/commodities.rs` (submódulo de la escena): las tres interfaces, los permisos por cliente
  (`Escena::poner_poderes`), atajos (que se tragan la tecla y su soltar), inactividad aparte de la de `guardian`,
  pantalla apagada (negro sin cursor; la tecla, el clic o la rueda que la despiertan no llegan), sin Bloq Mayús,
  AutoPoint, ClickToFront con doble clic, Exchange. Enganches pequeños en `escena.rs`: `tecla`, `raton`, `rueda`,
  `componer`, `actividad`, `revisar_inactividad`, `adios` y el aviso de `sesiones`.
- `AVISO_PODER` (`0x5536_0010`, 24 bytes) entre `sesiones` y el compositor, sin marca. Con marca no hace nada.
- El compositor anuncia el global 21 (`dashos_commodities_v1`) solo a quien tiene algún permiso; si le llega después
  de conectarse, se lo anuncia en el acto; si se le quita, se lo retira. Reparte los avisos y escribe en su registro
  cada atajo puesto y negado ("demasiados", una vez por commodity).

### `sesiones` y `guardian`

- `atajos`, `fondo` y `commodities` entran al final de `REPARTE` (no cambian el número de los guardados). No dan llave:
  `dar_servicio` manda `AVISO_PODER` antes de contestar. `DEJAR` y `RETIRAR` mandan el de quitar. La baja de un programa
  (el aviso de persona con usuario 0) borra sus permisos en el compositor.
- `PERMISOS` (provisional hasta los paquetes, S8): FKey `atajos`, Blanker y NoCapsLock `fondo`, Exchange
  `commodities`; y `commodity-prueba` los tres. Y el Shell `commodities`: lo que se lanza desde él lleva su ficha (se
  llama "Shell", no "Exchange"), y la prueba de `virt` lo cazó: la orden `Exchange` acababa en la pregunta de
  `guardian` ("Shell quiere..."). Mandar en las commodities propias es menos que matarlas, que el Shell ya puede.
  Pendiente del usuario. Consecuencia: FKey, Blanker o NoCapsLock lanzados desde el Shell preguntan en nombre del
  Shell; lanzados desde el Workbench o el lanzador, en el suyo.
- La baja de un programa del sistema también se le dice al compositor (antes, solo la de los de una persona o los que
  lanza `sesiones`), y el compositor suelta en el acto la conexión de un programa que acabó, la suya y las de su marca.
  Sin esto, una commodity muerta seguía con sus atajos hasta que el compositor intentaba escribirle (y esa pulsación se
  perdía); la prueba de `virt` lo cazó: no salía "commodity Prueba se va".
- `guardian` pregunta estos tres con su frase en llano (`PODERES_COMPOSITOR` en `protocolo/sesiones.rs`). Los demás
  permisos siguen saliendo con su nombre interno (no se tocó).
- La tabla de permisos de S6b lleva los tres, con su frase.

### Los programas

- `usuario/src/commodity_cliente.rs`: pedir el permiso, conectar, registrar, pedir, leer eventos; va con
  `ventana_cliente` por `#[path]`.
- `SYS:Tools/Commodities/Blanker` (`usuario/src/bin/blanker.rs`): 300 s y apaga la pantalla.
- `SYS:Tools/Commodities/NoCapsLock` (`usuario/src/bin/nocapslock.rs`).
- `SYS:Tools/Commodities/FKey` (`ordenes/src/bin/fkey.rs`): lee `Home:Prefs/FKey` (texto: combinación, programa y
  argumentos; sin fichero, Ctrl+Alt+S abre el Shell) y lanza como ayudante con su ficha, como el Shell. `FKey LISTA`
  enseña lo que leería.
- `SYS:Tools/Commodities/Exchange` (`ordenes/src/bin/exchange.rs`): desde el Shell es una orden (`LISTA`, `ACTIVAR`,
  `DESACTIVAR`, `ENSENAR`, `QUITAR`, `CLICKTOFRONT`, `AUTOPOINT`); sin Shell, su ventana con el kit. Los dos ajustes se
  guardan en `Home:Prefs/commodities`. En el disco, un alias de serie (`Alias Exchange SYS:Tools/Commodities/Exchange`)
  en vez de una copia en `SYS:C`: así no cambia lo que ven el selector y el hostil en `C`.
- `usuario/src/bin/commodity_prueba.rs`: la prueba de `virt` (la lanza `inicio`, al final de la lista).

### Decisiones tomadas aquí: pendientes del usuario

1. **Protocolo propio** (`dashos_commodities_v1`) y no una extensión de `dashos_tema_v1`: el tema lo ven todos y sus
   poderes van por el nombre que da `inicio`; las commodities necesitan un global oculto a quien no tiene permiso.
2. **Tres permisos** (`atajos`, `fondo`, `commodities`) con sus frases. S10 nombraba dos; el tercero es el de Exchange.
3. **Un atajo lleva Ctrl y Alt a la vez.** Fuera Super (sistema), AltGr, Ctrl+Alt+Supr, Ctrl+Alt+Retroceso y el
   volumen. Las teclas de función solas (las de FKey del Amiga) no se pueden coger.
4. **La tecla, el clic o la rueda que despiertan la pantalla apagada no llegan a nadie.**
5. **Inactividad de como poco 10 s**; Blanker de serie a 300 s, sin ajuste todavía.
6. **ClickToFront**: "clic" de serie (lo de hoy); "dobleclic", el Amiga clásico. **AutoPoint**: no de serie. Son del
   compositor entero, no de cada persona (con dos sesiones, el de quien lo cambió vale para las dos).
7. **Desactivada no suelta sus atajos**: siguen siendo suyos.
8. **Exchange solo ve y manda en las de su persona.**
9. **El Shell tiene `commodities` de serie** (ver arriba).
10. **El tope por programa**: 32 atajos, 4 commodities, 256 atajos en total, 8 Exchange.
11. **Las letras de un atajo son las teclas físicas** con nombres del teclado de EE. UU. (en el francés, Ctrl+Alt+A es
    la tecla de la Q).

### Lo que queda

- **Que los ajustes de `Home:Prefs/commodities` se apliquen al entrar en la sesión.** Hoy los escribe Exchange y los
  pone en el compositor en el acto, pero al reiniciar vuelven los de serie. Lo natural es que lo haga quien aplique los
  ajustes de texto al entrar (rama `ajustes-texto`, en marcha): no se tocó el centro para no pisarla.
- **La categoría de Ajustes** con la misma lista (S27): igual, tras `ajustes-texto`.
- **Arrancar de `WBStartup`**: no hay quien lo lea todavía.
- **El lanzador de la barra** solo lanza de `Programas/` y el Shell: las commodities de `SYS:Tools/Commodities` se
  lanzan hoy desde el Shell (y preguntan como el Shell) o con su ruta. Abrirlas con su nombre propio pide que
  `lanzables::lanzable` acepte ese cajón.
- **Los avisos de qué ventana se activa** (S10, cuarto punto): no hay ninguna commodity que los use; no se hicieron.
- **`commodities.library`** en la caja Amiga: diseñada arriba.
- **Las frases en llano de los demás permisos** en `guardian`: solo se pusieron las tres nuevas.

### Pruebas

- `cargo test` de `wayland`: 115 bien. Nuevas: `atajos` (leer, escribir, reservados, lo que no lleva Ctrl y Alt) y
  `tests_commodities` (11), con las hostiles: sin permiso no hay commodity, atajo ni Exchange (y el error corta);
  reservados y Ctrl+C, Ctrl+Mayús+N, Alt+F4 negados sin cortar; cien atajos (32 dados, los demás negados y apuntados
  con tope); dos con la misma (la primera se la queda, la segunda sabe quién); la que muere con atajos (quedan libres,
  Exchange se entera); con una ventana del sistema con el teclado o la sesión detrás no salta; apagar la pantalla sin
  estar inactiva no hace nada; inactividad de 1 s se sube a 10; la tecla y el clic que despiertan no llegan; sin Bloq
  Mayús y un comportamiento que no existe; Exchange de otra persona no la ve; quitar el permiso con todo puesto (por el
  aviso sin marca; con marca no hace nada); AutoPoint (no sube, no quita el teclado a una del sistema) y ClickToFront.
- `make prueba`: bien. `make arranque-virt`: bien (la quinta vuelta; las tres primeras cazaron lo de Ctrl+Mayús+N, el
  permiso de la orden desde el Shell y la baja que no llegaba al compositor, ya arreglados). Su prueba: sin permiso no
  se anuncia y enlazarlo corta; los permisos por `sesiones`; ocupada por Prueba; 3 reservados y 1 sin Ctrl y Alt; 68
  de cien negados por demasiados; Exchange ve 2 con 32 atajos; **Ctrl+Alt+K tecleado por el guion (QEMU `sendkey`) le
  llega**; desactivada se entera; al acabar, el compositor la suelta con sus 32 atajos. En la prueba del Shell, como
  Ana: `Exchange LISTA`, `Exchange QUITAR NoExiste` y `SYS:Tools/Commodities/FKey LISTA`.
- `make arranque-virt-2` y `make arranque-virt-linux`: bien.
- `make arranque-virt-wayland` con `APPS_IMAGE=~/.cache/dashos-caja-linux/Image-arch` y
  `APPS_DISCO=~/.cache/dashos-caja-linux/arch/disco-arch.img`: lo de foot, el puente, el portapapeles y los hostiles
  de Linux, bien; faltan 12 líneas (GTK, Firefox, la partición de 6135 MB, "memoria: RAM"), que son del disco de
  programas grandes (`~/.cache/dashos-caja-linux-apps`, que no está en esta máquina). Se paró a mano, por su PID, al
  quedarse esperando esas líneas. No se comparó con `revision` en la misma máquina.

### Lo que no se ha comprobado

- La pantalla apagada de verdad en QEMU (solo en las pruebas del PC, con la escena): Blanker en `virt` apagaría la
  pantalla en medio de las capturas del guion.
- Blanker, NoCapsLock y FKey lanzados de verdad (FKey solo con `LISTA`), y la ventana de Exchange: compilan; no se
  han visto en pantalla ni se han pulsado. ClickToFront y AutoPoint, solo en las pruebas del PC.
- Rendimiento: no se midió; lo nuevo en el camino de cada tecla es recorrer las commodities (como mucho 4 por programa)
  y en `componer` un `if`.
- La regla de `AGENTS.md` de `unsafe` solo en `kernel/src/base/`: FKey copia el programa a la memoria que pasa a
  `sesiones` con un bloque `unsafe`, igual que el Shell (`ordenes/src/bin/shell.rs`, `cargar`). Lo demás de los
  programas no añade `unsafe` (Exchange pinta en la memoria de `ventana_cliente::Lamina`).
