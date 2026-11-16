# Bitácora: el vídeo de la demo (rama `demo`)

Rama `demo`, desde `revision` (618576a). Lo pidió el usuario: un vídeo que enseñe cómo va el proyecto, con lo que ya
funciona. El vídeo, las capturas y el audio no van en git (`~/recuperar/geosian-7d/demo/`, solo en el equipo).

## 2026-11-16: fase 1, el inventario (antes de grabar nada)

Qué hay hoy en `revision` de cada cosa que pidió el usuario, si arranca en QEMU, con qué, y qué le falta para la demo.

| Pedido | ¿Existe en `revision`? | ¿Arranca en QEMU? ¿Con qué? | Qué falta para la demo |
|---|---|---|---|
| Arranque y entrada | Sí (`guardian` con pantalla de entrada en la imagen de usar) | Sí: `make virt-uso`, `tools/qemu_a_mano.sh` | Nada |
| Escritorio (barra, fondo, Workbench nativo, panel rápido) | Sí | Sí, en la imagen de usar | Nada |
| Ajustes (centro de control) | Sí (`centro`) | Sí; con `uso` no sale solo: se abre desde la barra o el panel | Nada |
| Explorador de archivos nativo | Sí (`explorador`, el Workbench le pide abrir los volúmenes) | Sí, en la imagen de usar | Nada |
| Shell | Sí (`shell-nativa` y `shell-capturas`, unidas) | Sí: `make arranque-virt-shell` (`tools/guion_shell.py`) | Nada |
| "Explorador de Trasfondo" (monitor del sistema) | Sí, unido a `revision` (rama `trasfondo`, 0 commits por unir): panel en el fondo, píldora en la barra con los procesos, "Monitor del sistema" y "Panel del escritorio" en Ajustes | Sí, en la imagen de usar | Nada. Entiendo "explorador de Trasfondo" como el monitor de procesos; si era otra cosa, pendiente del usuario |
| Caja Amiga con programas 68k | Sí (`caja-amiga`: exec, dos, graphics, intuition, gadtools, layers, cybergraphics... nativas) | Solo en las imágenes de prueba (`make arranque-virt`): con `uso` está fuera ("aún corre sus programas de prueba") | La caja es todavía una batería de pruebas: corre sus programas de prueba en orden y al final se apaga sola. No hay forma de lanzar un programa 68k a mano (ni desde el explorador ni desde el Shell). Los programas del usuario entran por `DASHOS_PROGRAMAS_AMIGA` (`tools/disco_archivos.py`) y la caja los ejecuta en su turno (paso 8b4), 6 s cada uno (30 s con `CAJA_INTERACTIVA=1`) |
| AmigaOS del usuario | Sí, como capa (`DASHOS_AMIGAOS=carpeta`, a `SYS:Amiga/AmigaOS`) | Sí con `make arranque-virt` y la variable | Solo carpetas (un HDF todavía no). Se usa `~/.cache/dash-emu/sys32mui` (C, Classes, Devs, Libs, MUI, S: no trae Workbench ni Tools) |
| Workbench del Amiga | No: `workbench.library` e `icon.library` del AmigaOS no cargan (piden una privada de exec) y la caja no tiene un Workbench propio con iconos; su "Workbench" es la pantalla de 800x600 donde abre las ventanas | - | Un Workbench de la caja (ventana con iconos, doble clic que lanza) |
| Tachyon (68k, MUI 5) | Programa del usuario (`~/Projects/tachyon/build/dashweb`), no de DashOS | Sí, con el AmigaOS del usuario, `MEMORIA=3G` y `DASHOS_PROGRAMAS_AMIGA`: pinta su página de inicio con iconos y logo (bitácora de caja-amiga, 15/11) | Sin red (no hay `bsdsocket.library` en la caja): solo la página de inicio. Sale 6 s (30 con `CAJA_INTERACTIVA`) |
| Calculator del Workbench | En el AmigaOS 3.9 del usuario (`~/recuperar/tachyon-scratch/sys39x/Workbench/Tools/Calculator`), no en `sys32mui` | Nunca probada en la caja | Probarla como programa del usuario |
| Forja (el entorno de Dash para el Amiga) | Programa del usuario (`~/Projects/forja/build/Forja`), MUI | Nunca probada en la caja | Probarla como programa del usuario |
| Hangar (cliente) | Programa 68k del usuario (`~/Proyectos/hangar/cliente`, MUI y `bsdsocket.library`) | No probado; sin red en la caja no puede buscar en el catálogo | `bsdsocket.library` nativa (S13) para que sirva de algo |
| Paq (gestor de paquetes, S8) | Solo diseño (`docs/diseño/S8-paquetes.md`; rama `s8-paq-hangar` con un commit de diseño, sin unir) | No: no hay código | Todo |
| Chromium en la caja Linux | No instalado. Está en los repositorios de Arch Linux ARM (`extra`: chromium 156) | No probado | Meterlo en `ARCH_PAQUETES` con `DASHOS_CAJA_APPS=1` (otro disco de caché: el de los programas grandes lo usan otros agentes) y probar si cabe en los 512 MB del invitado (Firefox tarda 33-44 s en abrir con 512 MB) |
| VS Code o Code - OSS en la caja Linux | No. Arch Linux ARM no trae `code`; Microsoft da un tar para arm64 (binario de terceros, nunca en git) | No probado | Bajar ese tar a la caché, Electron con `--ozone-platform=wayland`, y probablemente 1 GB para el invitado (`DASHOS_CAJA_LINUX_MB=1024`) |
| Firefox en la caja Linux | Sí (disco de los programas grandes, `~/.cache/dashos-caja-linux-apps`) | Sí: `make arranque-virt-wayland` (con teclado y ratón desde `firefox-entrada`) | Nada; 33-44 s hasta su ventana |
| GTK4 (gtk4-widget-factory), foot | Sí, el mismo disco | Sí: `make arranque-virt-wayland` | Nada |
| GL en la caja Linux (es2gears, vkcube) | Sí | Sí: `make arranque-virt-gpu` | Nada |

Cuánto tardaría instalar Chromium: no medido. El disco de los programas grandes se rehízo en unos 2 minutos con los
paquetes ya bajados (13/11, 11:44 a 11:46); habría que sumar la bajada de Chromium y lo que pida.

## 2026-11-16: fase 2, la forma de grabar

- **`make demo`** (en `kernel/`): la imagen nueva `demo` (`uso` + `linux`, y la caja Amiga, que con `uso` no va), con
  la caja Amiga de las capturas (`CAJA_INTERACTIVA=1`, compilada aparte en `caja/build-demo`: `kernel/build.rs` la
  toma de ahí con la opción `demo`). El disco: el de las pruebas con el AmigaOS del usuario (`DEMO_AMIGAOS`) y sus
  programas 68k (`DEMO_PROGRAMAS`: Tachyon y sus iconos, la Calculator, Forja), dentro de una GPT nueva con la
  partición de la caja Linux copiada del disco de los programas grandes (`tools/demo.py disco`: con el AmigaOS, el
  RedoxFS pasa de 7 a 27 MB y no cabía en la partición de DashOS del disco de Arch). Nada de eso va a git.
- **Cómo se graba: `screendump` a ritmo fijo**, por un monitor de QEMU propio de la grabación (otro para el guion y QMP
  para la tableta), a 8 cuadros por segundo, en JPEG con su hora. Elegido frente a VNC porque no hace falta ningún
  cliente de VNC ni nada más que QEMU, va a la resolución de la pantalla de DashOS (1280x720, `DEMO_PANTALLA`) y no
  cambia lo que ve DashOS. Con la máquina cargada se consiguen los 8 cuadros por segundo.
- **El ratón es una tableta USB** (posición absoluta por QMP, `input-send-event`): el guion va a sitios exactos, con
  movimiento visible, sin el truco de llevar el ratón a la esquina.
- **El guion** (`tools/demo_guion.py`) espera cada cosa en la salida serie (la pantalla de entrada, "entra Ana", las
  ventanas que pone el compositor, las de la caja, lo que dice la barra) y deja pausas para verlo; apunta en
  `marcas.txt` cuándo empieza cada tramo. Contesta "no" a las preguntas de la guardia a los 3 s.
- **El montaje** (`tools/demo_montar.py`): 1920x1080, DashOS a la izquierda (a escala) con el título del tramo y la
  locución como subtítulo; a la derecha, EnMarcha (la web de DashOS) con el escenario del tramo y el rótulo
  "Animación del diseño, no una traza en directo". Cada tramo dura lo que su locución; lo que sobra de grabación se
  acelera (hasta x10, con "x N" en la esquina). La música, por debajo de la voz. Lo que no debe salir se emborrona
  (`tapar.json`, que apunta el guion: la ventana de la prueba de Dash de la caja escribe el nombre del lenguaje).
- **EnMarcha**: la web, compilada en una copia fuera de su repositorio (sin tocar `webs-freemem`), servida en local y
  grabada con Playwright (Chromium sin cabeza), un escenario por carpeta, a velocidad "Lenta", escondiendo los botones.

Cómo se vuelve a grabar: `cd kernel && make demo` (unos 12 minutos de QEMU); luego `tools/demo_montar.py` con la
grabación, los escenarios de EnMarcha, la locución y la música (ver su cabecera).

### Lo que se vio al probarlo

- La caja Amiga va a su ritmo desde el arranque: las preguntas de la guardia a los 70-110 s, Tachyon hacia los 3,5
  minutos, la Calculator y Forja después, GadTools 90 s, y al final la caja acaba sola. Por eso el Amiga va justo
  después de entrar. La caja Linux se para sola a los 25 minutos de arrancar (`plazo_s`): va al final, antes de eso.
- **La Calculator del Workbench (3.9) no arranca**: abre `icon.library`, que la caja aún no tiene nativa (la del
  AmigaOS 3.2 llama a una función privada de exec) y acaba con 20 en 30 ms. Sin ventana.
- **Forja arranca** (68k, MUI): su ventana entera, con el árbol del proyecto (lista `SYS:`), los botones y "Listo.".
- Tachyon: su página de inicio, como en la bitácora de la caja Amiga. Pide `bsdsocket.library` (sin red).
- La caja de las capturas espera a que alguien pulse "Vale" en el requester del error de Dash (`test_trap_illegal`):
  sin eso no acaba nunca. El guion lo pulsa.
- Lo que falló en las pruebas del guion y está arreglado: esperar a Tachyon por el nombre de su ventana (no por
  cualquier ventana); no cerrar a mano las ventanas de la caja (el sitio de su botón depende de cómo las coloca la
  cascada, y un clic fuera abría los avisos); el botón del monitor, donde dice la barra (el desplegable cambia de alto
  con las filas); cerrar el Shell con tres `EndShell` justos (los de más iban a la terminal de Linux); GTK y Firefox con
  una sola visita a la terminal (con dos, un clic a destiempo mandaba las teclas a GTK).
- En Ajustes de la imagen de usar no hay entrada en el lanzador: se llega por la píldora de la barra ("Abrir el monitor
  del sistema"). La lista del monitor solo enseña tres filas (no cabe más en la ventana).

## Diseño: la traza de verdad para la derecha del vídeo (nivel 2, sin hacer)

Lo que pidió el usuario: que la derecha enseñe los mensajes de verdad. Tres piezas:

1. **La traza del kernel**, solo en una variante de prueba (opción `traza` del kernel, apagada en todas las demás:
   con la opción apagada no hay ni una instrucción de más, porque las llamadas van dentro de `#[cfg(feature =
   "traza")]`).
   - Qué se apunta, sin contenido de los mensajes: la hora (contador del sistema), el núcleo, el tipo y dos números.
     Tipos: `escribe` (proceso que manda, canal, lado, bytes, llaves que viajan) en `canal_escribir` al salir bien;
     `lee` (proceso que recibe, canal, lado) en `canal_leer` al salir bien; `irq` (número de interrupción) en
     `interrupciones::atender` cuando es de un proceso; `nace` (quien lanza, el nuevo) en `proceso_crear`; y, si se
     quiere, `llamada` (proceso, número) en `llamadas::atender` (mucho volumen: con un filtro).
   - El proceso, por un número que el kernel le da al crearlo (un contador en `Proceso`); el canal, por un número
     también suyo. De quién a quién va un mensaje se saca juntando el `escribe` y el `lee` del mismo canal y lado.
   - Dónde: un anillo fijo en memoria del kernel, con atómicos (sin `unsafe`: `AtomicU64` en un `static`), que cada
     núcleo llena sin cerrojos (`fetch_add` del índice y la marca de "escrito" al final; si el anillo da la vuelta,
     se cuentan los perdidos). Lo vacía un hilo del kernel (como el repartidor) cada 50 ms por el puerto serie, en
     líneas cortas (`T <µs> <tipo> <a> <b> <c>`), o a un fichero de la grabación si va por otra salida de QEMU (un
     segundo puerto serie es lo más limpio: no se mezcla con lo de siempre).
   - Prueba: en `make prueba`, que con la opción encendida un canal de ida y vuelta da su `escribe` y su `lee` en
     orden, que la interrupción del temporizador sale, que un anillo lleno cuenta los perdidos y no cuelga; y que la
     imagen sin la opción es igual byte a byte en `.text` que antes (o al menos que no hay símbolos de la traza).
2. **El traductor** (`tools/traza_nodos.py`): de los números de proceso a los nodos de `datos.ts` (`inicio`,
   `compositor`, `archivos`, `sonido`, `red`, `guardian`/`sesiones`, `usb`, `nvme`, `virtio-sonido`, `virtio-red`,
   `pantalla`, la caja Amiga, la caja Linux, "programa nativo"). Los nombres se sacan del orden: `inicio` lanza en el
   orden de su lista y dice "[inicio] X lanzado"; los `nace` de `inicio` van en el mismo orden. Lo que lanza
   `sesiones` lo dice igual. Las interrupciones, por su número, al controlador que la tiene asociada. Sale un JSON con
   los pasos en el formato de `ESCENARIOS` (`de`, `a`, `tipo`: canal, irq, local), con su hora.
3. **El modo traza de EnMarcha** (propuesta, sin tocar el repositorio de las webs):
   - En `datos.ts`, un tipo `PasoTraza = Paso & { t: number }` y nada más: los nodos y los enlaces ya están.
   - En `EnMarcha.tsx`, una propiedad opcional `traza?: { pasos: PasoTraza[]; lentitud: number }`: si viene, no se
     enseñan las situaciones; un reloj (`performance.now()` desde que empieza, por `lentitud`, p. ej. 20 veces más
     lento que la realidad) dice qué pasos toca dibujar; varios a la vez se dibujan a la vez (hoy solo hay un paso en
     pantalla), cada uno con su punto, y se apagan solos a los 600 ms. El texto del paso dice "proceso → proceso,
     canal, N bytes" en lugar del texto escrito.
   - Una página aparte (`/en-marcha/traza`), que lee el JSON de la URL o de un fichero local, para grabarla con
     Playwright a la vez que se monta el vídeo, sincronizada por la hora de la traza y la de los cuadros de DashOS.
   - Ojo: a ritmo real hay miles de mensajes por segundo (el compositor y la barra hablan todo el rato); hará falta
     un filtro por tramo (solo lo que toca a la ventana activa, o una muestra) para que se entienda algo.

Por qué no lo he hecho en esta rama: la traza en sí cabe, pero para que sirva hace falta el traductor y el modo de la
web, y la web es de otro repositorio que no se toca sin permiso. Queda la propuesta.

## 2026-11-16: fase 3, el primer vídeo

- **Dónde**: `~/recuperar/geosian-7d/demo/dashos-demo.mp4` (solo en el equipo, sin publicar): 1920x1080, 25 cuadros por
  segundo, H.264 High en yuv420p, AAC; **3 min 56 s** (3:47 de demo y 9 s de créditos). Al lado, la mezcla y la voz
  sola en WAV y el montaje en JSON (tramos y subtítulos).
- **Qué sale, en orden** (con su escenario de EnMarcha a la derecha): el arranque (EDK2 y la pantalla de entrada, a
  medio ritmo), la entrada como Ana, el escritorio (barra, discos del Workbench, el panel de Trasfondo y las dos cajas
  ya en marcha), la guardia de la caja Amiga preguntando por los virus de laboratorio (se contesta que no), programas
  de Dash y ReAction en la caja, Tachyon, la Calculator que no arranca (dicho en la locución), Forja, GadTools; el
  monitor del sistema desde la barra; Ajustes (modo oscuro, Seguridad con las negativas de antes, Acerca de); el
  explorador (SYS: en iconos y en lista, el volumen Amiga); el Shell (List con colores, Echo, la sugerencia, Tab,
  dos paneles, una pestaña); la caja Linux (gtk4-widget-factory y Firefox con su página). Y los créditos.
- **La locución**: OmniVoice en pegaso, perfil d84e5976 con sus ajustes de serie, 22 frases (2 min 27 s de voz).
  Cada frase comprobada con faster-whisper `small` en CPU; rehechas las que no decían lo escrito ("ordenador ARM" salía
  "ordenadora ARME": quitado; "Tachyon" se escribe "Táquion" y en una frase que lo deja suelto: "se llama Táquion";
  "al estilo del Amiga" salía "de la miga"; una de Firefox decía "tardo"). Whisper sobre la mezcla final: entiende
  todas las frases (al final, sobre la música sola, se inventa un "¡Suscríbete!", lo típico de Whisper con música).
- **La música**: Psilocybin (Legend & Yolk / Parallax, deep psilocybin mix, 1995), la del vídeo de Trasfondo:
  -10 dB sin voz y -25 dB con voz (con rampas de 0,4 s), fundido de 2 s al empezar y de 5 s al acabar; acaba con el
  vídeo (dura 8 minutos). Créditos al final como en el de Trasfondo, sin nombres de cuentas.
- **Lo que no debe salir**: la ventana de la prueba de Dash de la caja escribe el nombre del lenguaje, que lleva el de
  la cuenta del usuario: emborronada (el guion apunta dónde y cuándo; comprobado que sin emborronar el OCR la encuentra,
  y con ello, no). OCR de un cuadro por segundo de todo el vídeo (`tools/demo_ocr.py`): ningún nombre de cuenta ni ruta
  de `/home`; un solo "correo", que es `modprobe@fuse.service` en el arranque de systemd de la caja Linux.
  Las rutas del equipo no salen (la grabación no enseña terminales del PC).

### El texto de la locución

- `arranque-1`: Esto es DashOS, mi sistema operativo. Lo estoy escribiendo desde cero, en Rust, con la idea del Amiga de siempre, pero pensado para hoy.
- `arranque-2`: Aquí arranca en un emulador, como un ordenador de sesenta y cuatro bits. A la derecha va una animación del diseño: cómo se pasan los mensajes las piezas del sistema. No es una traza de lo que pasa ahora, es el dibujo de cómo funciona por dentro.
- `entrada-1`: Lo primero, la pantalla de entrada. Escribo el usuario y la contraseña, y cada tecla viaja como un mensaje: del teclado al controlador, del controlador al compositor, y de ahí a la ventana.
- `escritorio-1`: Y ya estoy en el escritorio. Arriba, la barra. A la derecha, los discos. Y en el fondo, un panel con lo que gasta el sistema.
- `escritorio-2`: Ya hay dos cajas en marcha: una terminal de Linux y una ventana del Amiga.
- `guardia-1`: La caja Amiga corre programas de sesenta y ocho mil, con las bibliotecas del sistema escritas de nuevo en Rust.
- `guardia-2`: Ahora está pasando sus pruebas, y algunas son virus de laboratorio. Cuando uno intenta tocar un ejecutable o un disco, el sistema me pregunta. Le digo que no, y no pasa.
- `dash-1`: Aquí salen programas hechos con Dash, mi lenguaje para el Amiga, con sus ventanas y sus controles de siempre.
- `tachyon-1`: Mi navegador para Amiga se llama Tachyon. Es el programa de verdad, el mismo que corre en un Amiga, y aquí abre dentro de DashOS. Todavía sin red, pero pinta su página de inicio entera.
- `calculadora-1`: La calculadora del Workbench todavía no arranca: le falta la biblioteca de los iconos, y la caja lo deja apuntado.
- `forja-2`: Esto es Forja, el entorno para programar en Dash. También es un programa de Amiga, y abre tal cual.
- `gadtools-1`: Y para acabar con el Amiga, una ventana con todos los controles clásicos.
- `monitor-1`: Vuelvo al escritorio. En la barra tengo el monitor del sistema, que viene de Trasfondo: quién gasta procesador y memoria. La caja Linux sale con su etiqueta.
- `ajustes-1`: Desde ahí se abren los Ajustes. Pongo el modo oscuro, y cambia todo a la vez.
- `ajustes-2`: En Seguridad queda apuntado lo que negué antes, lo de los virus del Amiga.
- `explorador-1`: Este es el explorador de archivos. Cada programa ve solo lo que le dejo ver, y el Amiga tiene su propio volumen.
- `shell-1`: Y el Shell, al estilo Amiga, pero con lo que le pido hoy: colores, sugerencias del historial, y completar con el tabulador.
- `shell-2`: También se puede partir en paneles y abrir pestañas.
- `linux-1`: Por último, la caja Linux. Es un Arch Linux de verdad, en una máquina virtual, y sus ventanas salen en mi escritorio.
- `linux-2`: Aquí, un programa de GTK, con todos sus controles.
- `firefox-1`: Y ahora, Firefox. Tarda un poco en abrir, pero luego va con el ratón y con el teclado.
- `cierre-1`: Hasta aquí, lo que ya funciona. Faltan cosas, como el gestor de paquetes o la red en el Amiga, pero esto ya va tomando forma.

### Lo que no sale y por qué

- **Chromium y VS Code**: no están en el disco de la caja Linux. Chromium está en Arch Linux ARM; VS Code (o Code -
  OSS) no, haría falta el tar de Microsoft para arm64. No se ha probado ninguno (hay que rehacer un disco de la caja,
  en una caché aparte de la que usan las demás sesiones). Sale Firefox en su lugar.
- **Paq**: solo diseño (S8), sin código.
- **Hangar**: el cliente es un programa 68k con MUI que necesita `bsdsocket.library`; la caja no tiene red. No se
  probó.
- **La Calculator**: no arranca (falta `icon.library` nativa). Se dice en el vídeo.
- **El Workbench del Amiga** (con iconos, para lanzar desde él): no existe aún en la caja; los programas 68k los lanza
  la caja en su turno de pruebas.
- **El sonido**: la demo no suena (no hay programa que suene en la imagen de usar); la animación de "un programa
  suena" no se usó.
- **La traza de verdad a la derecha** (nivel 2): diseñada arriba, sin hacer.

### Pendiente del usuario

- Si "el explorador de Trasfondo" era el monitor del sistema (lo que he enseñado) u otra cosa.
- El orden y el ritmo del vídeo: el Amiga va justo después de entrar porque la caja corre sola desde el arranque;
  para elegir el orden haría falta poder lanzar la caja (o un programa 68k) a mano.
- Que la caja Amiga se pueda arrancar y usar en la imagen de usar sin su batería de pruebas: es lo que más limita la
  demo (hoy solo se ve lo que la batería enseña, a su ritmo).
