# El Shell nativo

Rama `shell-nativa`, desde `revision` (41e5b81). Encargo: DashOS no tenía terminal ni Shell nativos (la única terminal
era la de la caja Linux). Aquí va el Shell, al estilo del de AmigaOS, y sus primeras órdenes.

Diseños de partida: S4b (rutas `VOL:cajón/fichero`, asignaciones por capas, `SYS:C`, `SYS:System/Shell`,
`S:Shell-Startup`), S6b ("El Shell es como la caja": `Home:`, `Work:` y red, y lo que lanza hereda su ficha) y
`docs/bitacoras/asignaciones.md` (la tabla de nombres por proceso; faltaba "lanzar por nombre").

## Diseño

### Las piezas

| Pieza | Dónde | Qué es |
|---|---|---|
| `shell/` (crate `dashos-shell`) | PC y DashOS | Lo que no depende del sistema: patrones, plantillas (`ReadArgs`), líneas, rutas, el intérprete de guiones y el texto de la consola. Se prueba con `cargo test` |
| `ordenes/src/lib.rs` (crate `dashos-ordenes`) | DashOS | Lo que usa cada orden: sus argumentos, escribir y leer por el Shell, y los ficheros por su ruta con nombre |
| `ordenes/src/bin/shell.rs` | `SYS:System/Shell` | La ventana, el intérprete y los trabajos |
| `ordenes/src/base/*.rs` | `SYS:C` | Las órdenes de base, cada una en su módulo |

`ordenes/` va aparte de `usuario/` por su perfil de compilación (para ocupar poco: `opt-level = "z"` y LTO).

### Dónde están las órdenes

Cada orden es un módulo con su `principal`, y se reparte de dos formas:

- **Juntas en `SYS:C/Ordenes`** (lo que va hoy en el disco): un programa que hace la orden por cuyo nombre lo llaman.
  El Shell, si le piden `Dir` y no hay un programa `Dir` en el camino, lanza `C:Ordenes` y le dice "eres `Dir`" (el
  nombre va en el arranque).
- **Cada una en su programa** (`SYS:C/Dir`, `SYS:C/Copy`...; `ordenes/src/bin/sueltas/`), que es lo que pedía el encargo
  y lo que hace el Amiga. Se compilan siempre y van al disco con `DASHOS_ORDENES_SUELTAS=1`. Si están, el Shell las
  prefiere.

Por qué juntas: DashOS no tiene bibliotecas compartidas, así que cada programa suelto lleva su copia de lo común (el
formato, el montón, los nombres, `ReadArgs`): unos 40 KB cada uno. Medido: las 17 sueltas, 880 KB; juntas, 97 KB. El
disco de pruebas mide 7 MB y lo comparten las pruebas de todos (su tamaño está también en las herramientas de la caja
Linux). Con las órdenes sueltas quedaban 1,9 MB libres en vez de 3,3, y la prueba de archivos "A15 sujetar sin soltar"
se quedaba sin sitio. Además `C/` es todavía el `Home:` provisional del escritorio: 17 ficheros más ahí descolocan las
pruebas del explorador.

### La ventana

Una consola de líneas, como la `CON:` del Amiga, no un VT100: lo escrito se guarda por líneas (hasta 4.000), se parte
en filas al ancho de la ventana (al cambiar de tamaño se vuelve a partir), se puede volver atrás (rueda, RePág y AvPág),
seleccionar con el ratón y copiar. La línea que se teclea tiene su editor (flechas, Inicio, Fin, Ctrl-A/E/K/W/X) y su
historial (200 líneas). Letra de ancho fijo del kit (Roboto Mono) y los colores del tema del sistema: cambia con él.

Portapapeles, como la terminal de la caja Linux: soltar el ratón pone la selección primaria, el botón central la
pega, Ctrl+Mayús+C copia y Ctrl+Mayús+V pega. Ctrl-C no copia: es "romper", como en el Amiga.

### El intérprete

Una máquina de estados (`shell::interprete`): `paso` hace una línea y dice qué toca. Así el Shell nunca se queda parado
dentro de un guion: entre paso y paso atiende su ventana, y un bucle sin fin se corta con Ctrl-C.

Una línea pasa por: parámetros del guion (`<nombre>`, tras `.KEY`), comentario (`;`), variables (`$nombre`, locales y
luego `ENV:`), alias, y se parte en órdenes (comillas, `>`, `>>`, `<`, `|`). Los argumentos llegan crudos a cada orden,
que los reparte con su plantilla, como en AmigaDOS.

| Dónde se hace | Órdenes |
|---|---|
| En el intérprete (control) | `If`/`Else`/`EndIf`, `Skip`/`Lab`/`EndSkip`, `FailAt`, `Quit`, `Execute`, `EndShell`/`EndCLI` |
| En el Shell (tocan su estado) | `Set`, `Unset`, `Get`, `SetEnv`, `GetEnv`, `UnsetEnv`, `Alias`, `Unalias`, `Path`, `Which`, `Run`, `Status`, `Break`, `Why`, `Cls`, `NewShell`, y las del Shell potente (`Panel`, `Prompt`, `Historial`, `Completa`, `Completar`, `Resaltar`, `Teclea`, `Medidas`) |
| Programas de `SYS:C` | `Assign`, `Cd`, `Dir`, `List`, `Copy`, `Delete`, `Rename`, `MakeDir`, `Type`, `Echo`, `Info`, `Date`, `Version`, `Protect`, `Filenote`, `Wait`, `Ask` |

`Path`, `Which` y `Run` son internas también en AmigaOS. `Status` y `Break` allí son programas porque los procesos son
de todo el sistema; aquí hablan de los trabajos de este Shell, así que van dentro.

Códigos de retorno, los de AmigaDOS: 0 bien, 5 aviso, 10 error, 20 fallo. Un guion se para con un código igual o
mayor que `FailAt` (10 de serie).

Una orden se busca: con ruta (`SYS:C/Dir`, `sub/orden`), solo ahí; sin ruta, en el cajón actual, en los cajones de
`Path` y en `C:` (que es `Home:C` y `SYS:C`, por capas). Un fichero con cabecera ELF y permiso de ejecutar se lanza; uno
con el bit de guion del Amiga se ejecuta como guion.

Tuberías: las órdenes corren a la vez, unidas por tubos de 64 KB (ver "7. Tuberías de verdad"). En el primer paso
iban una tras otra.

### El arranque de una orden

Un programa de DashOS nace sin argumentos. Quien lo lanza puede darle una llave más, **el canal de arranque** (en su
tabla de llaves, "arranque"; tras la unión con `union`, ya no es la cuarta por fuerza: el selector puede ir antes), con
`ARRANQUE` y `DATOS` ya en la cola: su cajón actual y sus argumentos. Si lo lanza un Shell, `ARRANQUE` trae **la
puerta**: una llave para escribir en el buzón del Shell, marcada con el número del trabajo. Por ella la orden pide
(escribir, leer una línea, cambiar el cajón actual, cambiar una asignación, acabar con un código) y el Shell contesta
por el canal de arranque. Los números, en `shell/src/protocolo.rs`.

- El Shell espera en un solo canal (un proceso solo espera en uno): el del compositor. La puerta es una copia marcada
  de ese mismo canal, así que lo de las órdenes y lo de la ventana llegan juntos.
- **Ctrl-C**: cada respuesta lleva las señales. La orden que ve "roto" acaba con el código 20 y el Shell dice
  `***Break`. Si en 2 segundos no ha acabado (no escribe ni pregunta), el Shell pide a `sesiones` que la mate.
- `Assign` y `Cd` son programas, pero lo que cambian es del Shell: se lo piden por la puerta. Las asignaciones son por
  proceso (S4b), y un programa aparte solo cambiaría las suyas.
- Una orden que muere sin avisar (un fallo) se nota porque su canal se cierra: código 20.

### Lanzar por nombre (`sesiones`)

Orden nueva `LANZAR_AYUDANTE` (40): "lanza esto con mi misma ficha". Es el "lanzar ... con la misma ficha (un
ayudante)" de S6b.

- **El programa lo lee quien lanza**, con sus propias raíces y permisos, y manda a `sesiones` la memoria con el ELF.
  `sesiones` no abre nada por él (S6b, regla 1: un servicio no hace por el cliente lo que su ficha no permite). Lanzar
  algo a lo que el Shell no llega falla al leerlo, en el servicio de archivos.
- **El hijo hereda la ficha**: usuario, grupo, identidad (el nombre de quien lo lanza: los permisos concedidos son por
  nombre, y un fichero que se llame como un programa con permisos no los gana), raíces y asignaciones (con lo que el
  Shell haya cambiado).
- **`PROGDIR:` del hijo**: quien lanza dice en qué raíz suya y en qué cajón estaba el programa. `sesiones` le pone ahí
  una raíz más (la 5), con el tope de la raíz de origen: por `SYS:` solo se lee.
- Solo lo pide un programa lanzado del disco, y como mucho 32 procesos por usuario a la vez (que uno malo no llene la
  tabla de fichas).

Y en `LANZAR`, un cambio pequeño: si la petición trae una llave más, es el canal de arranque del programa.
Así `guardian` lanza el Shell de la prueba con argumentos.

## Decisiones pendientes del usuario

0. **Las órdenes de base van juntas en un programa (`SYS:C/Ordenes`)**, no una por fichero, por el sitio del disco de
   pruebas (arriba). Sueltas también existen y funcionan igual. Se puede volver a "una por fichero" cuando el disco de
   pruebas crezca o haya bibliotecas compartidas.

1. **Cualquier programa lanzado del disco puede lanzar ayudantes con su ficha**, no solo el Shell. Hoy no hay perfiles
   en las fichas (todo lo que lanza `sesiones` es "como el Shell"), así que no da nada que no tuviera. Cuando haya
   perfiles encerrados, habrá que decidir si una aplicación encerrada puede.
2. **Lo lanzado desde el Shell sale en el monitor con el nombre del Shell.** La ficha tiene un solo nombre, que es la
   identidad para los permisos. S6b habla de "aplicación" y "nombre" por separado: falta el segundo campo.
3. **`*` vale como `#?`** en los patrones (en AmigaDOS es opcional).
4. **`..` y `.` se entienden** en las rutas, además de la `/` del Amiga.
5. **Ctrl-C es romper**, no copiar (copiar es Ctrl+Mayús+C, como en la terminal de la caja Linux).

Del Shell potente:

6. **Las teclas de los paneles son las de Terminator**: Ctrl+Mayús+O parte en horizontal, Ctrl+Mayús+E en vertical,
   Ctrl+Mayús+T pestaña nueva, Ctrl+Mayús+W cierra el panel, Alt+flechas cambia de panel, Ctrl+RePág y Ctrl+AvPág
   de pestaña.
7. **Cerrar un panel mata sus trabajos sin preguntar** (S0: deshacer antes que preguntar; aquí no hay cómo deshacer).
8. **El indicador de serie es el de segmentos** con el cajón, los trabajos de fondo y el código si la orden falló. El
   clásico del Amiga (`1.Home:> `) es el tema `amiga`.
9. **Los colores del resaltado**: la orden en verde si existe y en rojo si no, las cadenas en amarillo, las rutas que
   existen subrayadas, las redirecciones en magenta, las variables en cian, las palabras de la plantilla en azul.
10. **El historial guarda 2.000 líneas** y no guarda lo que empieza por espacio.
11. **`Dir` y `List` salen con colores en una consola** sin pedirlo (`NOCOLOR` los quita).
12. **Un programa no cambia el título de la ventana** ni lee el portapapeles con escapes: se ignoran.
13. **Las asignaciones las comparten los paneles de una ventana**; cada panel tiene su cajón actual y sus variables.
14. **Los ajustes del Shell están en `ENVARC:Shell/`** (`Historial`, `Indicador`, `Perfil`) y los complementos en
    `S:Shell-Complementos/`.
15. **Tres alias de serie**: `ll`, `la` y `md` (`SYS:S/Shell-Complementos/alias`).

## Bitácora

### Paso 1: el Shell de base

Hecho: el crate `shell/` (patrones, `ReadArgs`, líneas, rutas, intérprete, consola), el Shell con su ventana, 17
órdenes de base, lanzar con la ficha propia en `sesiones`, la entrada "Shell" en el lanzador de la barra y la prueba en
QEMU.

- **`sesiones`**: `LANZAR` acepta una llave más (el canal de arranque del programa); orden nueva `LANZAR_AYUDANTE`
  (40); la barra y el escritorio pueden lanzar `System/Shell` además de lo de `Programas/`. Los cambios son un brazo
  nuevo, dos funciones y cuatro líneas tocadas en `LANZAR`, `alta_archivos` y `tabla_de`.
- **`guardian`** (en la prueba): tras el cambio rápido de usuario lanza `System/Shell` como Ana con
  `REGISTRO ORDEN Execute S:Prueba-Shell` y espera a que acabe.
- **Disco de pruebas**: `System/Shell`, `C/Ordenes` y `S/Prueba-Shell`. Quedan 3.256 KB libres de 7.168 (antes, unos
  3.700).

Pruebas en el PC (`cd shell && cargo test`, en `make prueba`): 36 de este paso (67 con las de los módulos del Shell
potente que entran en el mismo commit: completar, historial, resaltar, indicador, perfil y paneles, aún sin usar). Hostiles: comillas sin cerrar,
líneas de 200.000 letras, patrones a mala idea (`#(#(#(#a)))` contra 200 letras: hay un tope de pasos), 5.000 ficheros
que casan, un guion con un bucle sin fin cortado con Ctrl-C, un guion que se llama a sí mismo (16 niveles), escapes
con parámetros sin fin, un megabyte sin saltos de línea.

Prueba en QEMU (`make arranque-virt`, el guion `S:Prueba-Shell`, 100 líneas): todas las órdenes; redirecciones,
tuberías y entrada desde un fichero; asignaciones (poner, añadir capa, quitar; tapar `SYS:` da error); variables;
`If`, `Skip`, `Lab`; un guion con `.KEY`. Y lo hostil: comillas sin cerrar, una orden que no existe, una línea de
3.000 letras, redirigir, copiar, crear y `Delete ... ALL` en `SYS:` (todo "está protegido"), leer y lanzar un fichero
de Carla (no se puede), lanzar un texto con permiso de ejecutar ("no es un programa de DashOS"), un patrón por todo
`SYS:`, y `Break` a un guion con bucle sin fin y a un `Wait 100`.

Lo que salió por el camino:

- Las órdenes sueltas no cabían en el disco de pruebas (arriba, "Dónde están las órdenes").
- Un guion sin su argumento obligatorio seguía corriendo si quien lo llamaba tenía `FailAt 21`. Ahora se sale de él.
- `Type` de un fichero de otra persona decía "no existe"; ahora dice "está protegido".
- El guion de teclas de `make arranque-virt` se quedó parado en dos pasadas seguidas (no pulsó el botón de
  `dash-ventana`, y de ahí en adelante nada). A la tercera, sin cambiar nada de eso, pasó entera. No lo he explicado.

`make prueba`, `make arranque-virt`, `make arranque-virt-2` y `make arranque-virt-linux`: bien.

Medido: la prueba del Shell entera (70 órdenes lanzadas, con 4 segundos de `Wait`) tarda menos de 20 segundos en
QEMU: entre su primera línea y la última, el registro solo trae una de las marcas que el compositor escribe cada 10 s.
No he medido lo que cuesta lanzar una orden suelta.

### Paso 2: el Shell potente (los ocho puntos del encargo del 2026-11-15)

El diseño, en "El Shell potente", más abajo. Qué quedó hecho de cada punto:

| Punto | Hecho | Sin hacer |
|---|---|---|
| 1. Completar con Tab | Órdenes (internas, alias, de base, programas del cajón actual, de `Path` y de `C:`), rutas con volúmenes y asignaciones, palabras de la plantilla (las de serie y las de los complementos), tras redirecciones; menú con varias, que se recorre con Tab y las flechas | Completar variables (`$nom`) |
| 2. Historial | Guardado en `ENVARC:Shell/Historial` (2.000 líneas), Ctrl-R, flecha arriba por prefijo, lo que empieza por espacio no se guarda, orden `Historial` | Compartirlo al momento entre paneles abiertos (cada uno lo lee al abrirse) |
| 3. Sugerencia y resaltado | Sugerencia en gris del historial (derecha o Fin la aceptan; Ctrl+derecha, una palabra); orden que existe o no, cadenas, rutas que existen, redirecciones, variables, palabras de la plantilla, comentario | — |
| 4. Indicador | Segmentos (`cajon`, `codigo`, `trabajos`, `hora`, `usuario`, `git`), tres temas, colores por segmento, fichero `ENVARC:Shell/Indicador`, `Prompt` | `usuario` no dice nada: un programa no tiene cómo saber el nombre de su usuario. `git`, el hueco |
| 5. Pestañas, paneles y perfiles | Partir en horizontal y vertical, pestañas, cambiar con Alt+flechas y Ctrl+RePág/AvPág, cerrar, ratón; perfil (`ENVARC:Shell/Perfil`): letra, colores, tamaño inicial, translucidez | Arrastrar la raya entre paneles (siempre a medias). La translucidez está escrita y no la he visto en pantalla |
| 6. Colores | SGR entero (negrita, tenue, subrayado, inverso, 16 y 256 colores, color directo, en letra y fondo); `Dir` y `List` con colores por tipo en una consola | Terminal completo (pantalla alternativa): estudiado y diseñado, no hecho (más abajo, "Estudio") |
| 7. Tuberías | De verdad (a la vez, con tubos de 64 KB y sin tope), sustitución de órdenes, `NIL:` | Un guion dentro de una tubería |
| 8. Complementos | `S:Shell-Complementos/` (alias, variables, `Completa`); el que falla se dice y se salta | Funciones; segmentos del indicador desde un complemento |

Cómo quedó el programa: la ventana tiene pestañas y paneles (`shell::paneles`), y cada panel es una sesión entera
(su consola, su intérprete, su cajón actual, sus variables y sus trabajos). Las asignaciones son del programa (las
lleva `sesiones` por proceso), así que las comparten todos los paneles de una ventana.

Pruebas en el PC: 68 (`cd shell && cargo test`). Las hostiles nuevas: completar con 500 entradas (se dan 200 y se
dice que hay más), líneas de 100.000 letras, comillas a medias; un historial con bytes que no son texto, líneas
enormes y escapes; un fichero de indicador y uno de perfil rotos o enormes; escapes mal formados o a mala idea (el
título de la ventana, el portapapeles, mover el cursor, cadenas que no se cierran, colores a medias, escapes
partidos); sustituciones sin cerrar, de más y que hacen la línea enorme.

Prueba en QEMU (`S:Prueba-Shell`, ahora 190 líneas): `Completar`, `Resaltar` y `Teclea` contra el disco de verdad
(la sugerencia, Tab con menú, Ctrl-R, flecha arriba por prefijo); el historial en `ENVARC:`; un historial dañado (una
imagen) que lee un panel nuevo; `Panel`; el fichero del indicador con una línea mala; tres complementos (uno falla)
cargados por otro Shell; una tubería de 313 KB por un tubo de 64 KB; cortar una tubería; sustitución de órdenes.

Lo que salió por el camino:

- **El disco de pruebas, otra vez.** Lo que se escribe en `Home:` (el volumen de Ana) ocupa sitio del mismo disco de
  7 MB, y borrarlo no lo devuelve a tiempo: con 1,5 MB de ficheros de prueba, la copia del explorador (de otra prueba)
  se quedó sin sitio. La prueba del Shell ya no escribe más de 100 KB a la vez.
- **El registro del sistema.** Copiar 25.000 líneas de la consola al registro lo llenaba y se perdían líneas de otros.
  `Medidas CALLA` y `Medidas HABLA` paran y reanudan la copia.
- Leer de un tubo línea a línea era lento (25.000 viajes): ahora el Shell manda las líneas enteras que caben en una
  respuesta y la orden las reparte.

Lo que la prueba no comprueba: el teclado y el ratón de verdad sobre la ventana del Shell. Las teclas se prueban con
`Teclea`, que pasa por el mismo camino que una tecla del compositor (`tecla_con`) pero no por el mapa de teclado ni
por los modificadores. En la pasada de `make arranque-virt` llegaron al Shell 3 teclas y 8 eventos del ratón (los
del guion, pensados para otras ventanas). Tampoco se ha mirado en una captura cómo se ven los paneles, la barra de
pestañas, el menú de completar ni la translucidez.

### Lo medido

En QEMU (`Medidas`, una pasada; el PC estaba con más pruebas a la vez):

| Qué | Bytes | Tarda | En la consola | Cuadros pintados |
|---|---|---|---|---|
| `Type` de 25.000 líneas sin colores | 313.890 | 290 ms | 79 ms | 0 |
| `Type` de 16.384 líneas con colores, fondos y subrayado | 1.228.800 | 718 ms | 185 ms | 50, en 22 ms |

"En la consola" es lo que cuesta meter el texto en las líneas (`Consola::escribir`); el resto es el viaje de cada
trozo entre la orden y el Shell. Pintar un cuadro cuesta menos de medio milisegundo (solo se repintan las filas que
cambian). En la primera fila no se pintó ningún cuadro en 290 ms: el aviso de cuadro del compositor no llegó en ese
rato. No sé por qué; en la segunda llegaron a su ritmo.

En el PC (banco aparte, lo mejor de 20 vueltas), la consola de antes de los colores (celdas de 8 bytes, 16 colores)
frente a la de ahora (16 bytes, fondo, estilos, 256 colores):

| Qué | Antes | Ahora |
|---|---|---|
| Escribir 25.000 líneas sin colores | 1,9 ms | 2,1 ms |
| Escribir 16.384 líneas con colores | 3,8 ms | 4,9 ms |
| Sacar 1.000 veces lo que se ve (sin colores) | 0,62 ms | 0,80 ms |
| Sacar 1.000 veces lo que se ve (con colores) | 0,73 ms | 0,99 ms |

Los colores cuestan entre un 10 % y un 35 % más en la consola. Frente al viaje de cada trozo, no se nota.

Tubería de 313 KB (`Type grande.txt | Type NUMBER`, 463.890 bytes a la consola): 7,4 s leyendo del tubo línea a
línea; 4,1 s con varias líneas por viaje. Sigue siendo lento (el mismo fichero sin tubería tarda 0,3 s): no he
separado cuánto es del tubo y cuánto de la orden que numera, que trabaja línea a línea.

### Unión con `union` (2026-11-16)

Choques en `sesiones.rs`, `usuario/Cargo.toml`, `tools/disco_archivos.py` y `servicios.md`. Lo que cambió de fondo:

- **La tabla de llaves** (llamada 56): ahora cada llave con que nace un programa tiene nombre, y lo que lanza
  `sesiones` puede llevar el selector como cuarta. El canal de arranque deja de ser "la llave 4": va el último, se
  llama `arranque`, y las órdenes lo buscan por su nombre. Los ayudantes (`LANZAR_AYUDANTE`) nacen también con su
  tabla y con el selector, como lo de `LANZAR`.
- **`RAM:` en memoria**: el `PROGDIR:` de un ayudante que sale de `RAM:` va a ese volumen, no al cajón de antes.
- No he usado llamadas del kernel nuevas: no hay choque con las 56 a 58.

Sobre la unión: `make prueba`, `make arranque-virt`, `make arranque-virt-2` y `make arranque-virt-linux`, bien. Las
medidas del Shell, parecidas (302 ms, 761 ms y 4,2 s).

### Visto en pantalla (2026-11-16)

Rama `shell-capturas`. Hasta aquí nadie había visto el Shell: sus pruebas lo manejan por guion. Ahora hay una prueba que
lo usa como una persona y lo captura: **`make arranque-virt-shell`**. La imagen es la de usar (`virt-uso`, con la
pantalla de entrada de verdad), con una copia del disco de pruebas. `tools/guion_shell.py` entra como Ana, abre el Shell
desde el lanzador de la barra y teclea en él con el teclado y el ratón USB de QEMU: así pasa por el mapa de teclado
español y los modificadores, que `Teclea` se salta. Las capturas, en `kernel/target/shell/shell.*.png` (y en
`capturas-agente/shell-*.png`, fuera de git):

| Captura | Qué se teclea |
|---|---|
| `1-recien-abierto` | Nada: el indicador de serie (`Home:` en su bloque) y el cursor |
| `2-list` | `List SYS:`: los cajones en azul, el resumen |
| `3-sugerencia` | `Echo "hola desde el teclado"`, y luego `Ech`: la sugerencia en gris |
| `4-orden-mala` | `NoExiste SYS:S "sin cerrar`, sin Intro: rojo, ruta subrayada, comillas abiertas en rojo |
| `5-menu-tab` | `Dir SYS:S` y Tab dos veces: el menú (`S/ Storage/ System/`) con el primero elegido |
| `6-paneles` | Ctrl+Mayús+E y `Dir SYS:C` en el panel nuevo |
| `7-pestana` | Ctrl+Mayús+T y `Version` en la pestaña nueva |

La última orden tecleada es `Assign CapturasHechas: RAM:`, que `sesiones` dice en el registro: es la línea final que
espera la prueba (llegaron las teclas, con `:` y mayúsculas, y el guion acabó).

Se ve bien: el indicador, el cursor en su sitio (también sobre la primera letra de la sugerencia), los colores del
resaltado, la sugerencia en gris (se lee: 3,2 a 1 sobre el fondo claro), el menú de Tab, los dos paneles con la raya del
que tiene el teclado, la barra de pestañas, `Dir` en dos columnas y las letras con acento (`Cajón`). Nada cortado ni
pisado. Ctrl+Mayús+E y Ctrl+Mayús+T, de verdad por el teclado, funcionan.

Arreglado, con su prueba:

- **El texto del indicador no se leía**: negro sobre el azul del bloque (2,7 a 1). El indicador pedía el "blanco vivo"
  de la paleta (el 15), que en la paleta clara es oscuro a propósito (para que se lea sobre la superficie). En el tema
  oscuro el fallo era el contrario: blanco sobre un azul claro (2,9 a 1). Ahora la letra de los bloques es un color
  nuevo de la consola, `SOBRE_FONDO`, que decide quien pinta: de la letra clara y la oscura del tema, la que más
  contrasta con el fondo que tiene la celda en pantalla (`consola::mejor_sobre`, con su prueba, hostiles incluidas).
  En el tema claro sale blanco sobre azul (6 a 1).
- **"1 ficheros"** en el resumen de `List`: ahora singular y plural (`1 fichero`, `1 cajón`, `1 byte`). La prueba del
  Shell lista un cajón con un solo fichero y `arranque_virt.txt` espera `1 fichero - 0 cajones -` (`List` de un
  fichero suelto no saca el resumen).

Visto y sin arreglar (apuntado):

1. **La ventana del Shell no tiene barra de título ni botón de cerrar** (`1-recien-abierto`): un rectángulo plano sobre
   el escritorio, sin borde ni sombra. Las ventanas de la caja Linux sí llevan la barra de DashOS. No hay dónde pulsar
   para moverla o cerrarla (no he mirado si el compositor tiene teclas para eso); desde dentro, `EndShell` o
   Ctrl+Mayús+W en cada panel. Hay que darle la decoración del kit o la del
   compositor: no es un cambio pequeño.
2. **Las fechas de `List` van en UTC y la barra en hora local** (`2-list`: los ficheros dicen 07:41 y el reloj 08:41).
   La barra usa la zona de `Prefs/zona`; `shell::fecha`, no (y tampoco `Date` ni el segmento `hora` del indicador).
   Arreglarlo es leer la zona en las órdenes: queda para otro paso.
3. **Lo elegido en el menú de Tab sale en un bloque casi negro** (`5-menu-tab`): es `INVERSO` sobre la letra de serie.
   La selección y el cursor usan los colores del tema (el primario y su contenedor); el menú podría usar los mismos.
   Pendiente del usuario: es cuestión de gusto.
4. **Mientras se teclea una orden, sale en rojo hasta que está entera** (`3-sugerencia`: `Ech` en rojo con la
   sugerencia `o "hola..."` detrás). Es lo que hace zsh con su resaltado; con la sugerencia delante choca un poco.
   Pendiente del usuario: dejarlo, o no poner rojo si la sugerencia completa una orden que existe.
5. **Las pestañas se llaman por el cajón** (`7-pestana`: `1 Home:` y `2 Home:`): dos pestañas en el mismo cajón solo
   se distinguen por el número.
6. **El indicador azul no es del color del tema** (el cursor y la pestaña elegida son del primario, morado aquí). El
   color de cada segmento es un ajuste (`ENVARC:Shell/Indicador`); el de serie podría ser el primario del tema.
   Pendiente del usuario.

Pruebas: `cd shell && cargo test` (69), `make prueba`, `make arranque-virt` (a la segunda: la primera, el resumen de
`List` de un fichero suelto no sale, y cambié la prueba), `make arranque-virt-2` y `make arranque-virt-shell`, bien.
No pasé `make arranque-virt-linux`: no toco archivos, sesiones ni el compositor, y del disco solo cambia el guion de
la prueba del Shell.

No visto: el tema oscuro (las cuentas de contraste de arriba son con su paleta, no con una captura), la translucidez,
Ctrl-R, los paneles en horizontal y el ratón sobre la ventana (seleccionar, pegar, la rueda).

## El Shell potente (encargo del 2026-11-15)

Petición del usuario: "quiero una Shell potente como la que tengo ahora en Shiva con Terminator y zsh y oh-my-zsh". Se
mantienen la sintaxis y las órdenes de AmigaDOS; lo que sube es la experiencia de uso. Y el principio nuevo de S0
("sencillo por delante, todo a la vista por detrás"): recién abierto sirve sin configurar nada, y todo lo que se
ajusta es un fichero de texto de `ENVARC:` que se puede leer y cambiar a mano.

Sala limpia: de zsh, oh-my-zsh, Terminator y VTE se toma el comportamiento que documentan, no su código.

### Dónde va cada cosa

Lo que se puede probar en el PC va al crate `shell/`; el programa solo lo conecta con la ventana y los ficheros.

| Paso | Módulo nuevo | Qué hace |
|---|---|---|
| 1 | `shell::completar` | Dada la línea y el cursor, qué se está escribiendo (una orden, una ruta, un argumento) y con qué se puede completar |
| 1 | `shell::ordenes` | La plantilla de cada orden conocida (las internas y las de base): una sola tabla para repartir argumentos y para completar |
| 2 | `shell::historial` | El historial con tope, buscar hacia atrás y por prefijo, leerlo y escribirlo como texto |
| 3 | `shell::resaltar` | Los trozos de la línea y de qué clase es cada uno (orden que existe o no, comillas, ruta, redirección, variable) |
| 4 | `shell::indicador` | El indicador por segmentos, sus temas y su fichero de ajustes |
| 5 | `shell::paneles` | El árbol de pestañas y paneles: partir, cerrar, cambiar, y el rectángulo de cada uno |
| 6 | `shell::consola` | Fondo, negrita, subrayado, inverso, 256 colores y color directo en las celdas |

### 1. Completar con Tab

- **La primera palabra** es una orden: las internas, las de control, los alias, las de base y los programas del cajón
  actual, de `Path` y de `C:` (con permiso de ejecutar o bit de guion). Si lleva `:` o `/`, es una ruta.
- **Las demás** son, según la plantilla de la orden: sus palabras clave e interruptores (`ALL`, `TO`, `QUIET`), y rutas.
  Tras una palabra `/K`, lo que toca es su valor. Una orden sin plantilla conocida completa rutas.
- **Las rutas**: `Ho` da los volúmenes y asignaciones que empiezan así (`Home:`); `Home:Do` lista `Home:` y da
  `Home:Documentos/`. Un cajón acaba en `/` (o se queda en `:`), para seguir; un fichero, en un espacio. Un nombre con
  espacios se pone entre comillas.
- **Con una sola posibilidad**, se pone. **Con varias**, se pone lo que tienen en común y sale un menú debajo de la
  línea; Tab y Mayús+Tab (o las flechas) recorren el menú, Intro elige, Esc lo quita.
- Las plantillas: de `shell::ordenes` para lo de serie; un complemento añade las suyas con `Completa orden plantilla`.
- Topes: se miran como mucho 2.000 entradas por cajón y el menú enseña 200.

### 2. Historial

- En `ENVARC:Shell/Historial` (la capa del usuario: `Home:Prefs/Env-Archive/Shell/Historial`), una línea por orden,
  las últimas 2.000. Se lee al abrir y se añade al acabar cada línea (así dos Shell abiertos no se pisan lo anterior:
  cada uno añade lo suyo).
- Un fichero dañado (bytes que no son texto, líneas enormes, vacío) no rompe nada: lo que no vale se salta.
- **Flecha arriba con algo tecleado** busca las líneas que empiezan así. **Ctrl-R** busca hacia atrás lo que contenga
  lo que se teclea; Ctrl-R otra vez, la anterior; Intro la ejecuta, Esc la deja para editar.
- Una línea que empieza por espacio no se guarda (para lo que no se quiere dejar escrito).

### 3. Sugerencia y resaltado

- **Sugerencia**: mientras se teclea, en gris, el resto de la línea más reciente del historial que empieza igual.
  Flecha derecha (o Fin) al final de la línea la acepta entera; Ctrl+Derecha, una palabra.
- **Resaltado**: la orden en verde si existe (interna, alias, o un programa que se encuentra) y en rojo si no; las
  cadenas entre comillas, en amarillo (en rojo si no se cierran); las rutas que existen, subrayadas; las redirecciones
  y las tuberías, en otro color; las variables, en otro. Los colores son los del tema.
- Mirar si algo existe cuesta un viaje al servicio de archivos: se recuerda lo mirado durante 2 segundos y como mucho
  se miran 8 cosas nuevas por tecla.

### 4. Indicador

- Por segmentos: `usuario`, `cajon`, `codigo` (solo si la última orden falló), `hora`, `trabajos` (si hay de fondo) y
  `git` (el hueco: hoy no dice nada; cuando haya git dirá rama y estado).
- En `ENVARC:Shell/Indicador`, texto: `tema`, `segmentos`, y el color de cada uno. Temas de serie: `amiga` (el clásico
  `1.Home:> `, sin colores), `segmentos` (bloques de color, el de serie) y `sobrio` (una línea, colores sin fondo).
- `Prompt` (la orden de AmigaDOS) cambia el del Shell en que se teclea; el fichero, el de todos.

### 5. Pestañas, paneles y perfiles

- Una ventana tiene pestañas; cada pestaña, un árbol de paneles (partir en horizontal o en vertical). Cada panel es
  una sesión propia: su consola, su intérprete, su cajón actual, sus variables y sus trabajos.
- Las asignaciones son de todo el programa (las lleva `sesiones` por proceso): un `Assign` en un panel lo ven los demás
  paneles de la ventana, como en el Amiga, donde son de todo el sistema. Otra ventana (`NewShell`) es otro proceso.
- Teclas, las de Terminator: Ctrl+Mayús+O parte en horizontal, Ctrl+Mayús+E en vertical, Ctrl+Mayús+T pestaña nueva,
  Ctrl+Mayús+W cierra el panel, Alt+flechas cambia de panel, Ctrl+AvPág y Ctrl+RePág cambian de pestaña.
- Perfiles en `ENVARC:Shell/Perfil`: letra (tamaño), colores (los del tema del sistema o propios) y translucidez del
  fondo, si el compositor la da.

### 6. Colores, y el terminal completo

- La consola entiende SGR entero: negrita, tenue, cursiva (como tenue), subrayado, inverso, los 16 colores, los 256 y
  el color directo, en letra y en fondo. `Dir` y `List` dan color por tipo cuando escriben en una consola (no a un
  fichero ni a una tubería).
- Lo que **no** hace una secuencia: cambiar el título (se ignora: un programa no engaña con el título de la ventana),
  mover el cursor fuera de la línea, leer la pantalla ni el portapapeles (OSC 52), contestar nada. Lo que no se
  entiende se salta entero, con tope.
- **Terminal completo** (pantalla alternativa, cursor direccionable, para un editor a pantalla completa): ver "Estudio:
  el terminal completo", más abajo.

### 7. Tuberías de verdad y sustitución de órdenes

- Las órdenes de una tubería corren a la vez. Lo que escribe una va a un tubo de 64 KB; cuando se llena, a la que
  escribe no se le contesta hasta que la otra lee (así espera sin gastar, y no hay tope de tamaño). Si la que lee
  acaba antes, la que escribe acaba también, con el código 0. El código de la tubería es el de su última orden.
- Una orden interna puede ir en una tubería (lo que escribe va al tubo); un guion, no (con `Execute`, tampoco hoy).
- Sustitución: `` `orden` `` en una línea se cambia por lo que escribe esa orden (sin el salto final, y con los de
  dentro como espacios), como en AmigaDOS. Con tope (64 KB, 8 por línea); lo que sale no se vuelve a sustituir.
- `NIL:` vale en las redirecciones, como en el Amiga: `>NIL:` tira lo escrito y `<NIL:` no da nada.
- Los comodines ya los expanden las órdenes, como en el Amiga (`Copy #?.txt`), no el Shell.

### 8. Complementos

- Guiones del propio Shell en `S:Shell-Complementos/` (por capas: los del usuario en `Home:S`, los del sistema en
  `SYS:S`). Se cargan al abrir, por orden alfabético, tras `S:Shell-Startup`.
- Un complemento define alias, variables y plantillas de completado (`Completa orden plantilla`). Uno que falla se
  dice y se salta (lo que le quedaba); los demás siguen.
- De serie hay uno, `SYS:S/Shell-Complementos/alias`: `ll` (`List`), `la` (`Dir ALL`) y `md` (`MakeDir`).
- Lo que no hay: funciones. Un complemento que quiera una orden nueva pone un guion con el bit de guion en un cajón
  y lo añade a `Path`, o un alias. Tampoco define segmentos del indicador.

### Todo desde el Shell (S0)

Lo que se hace con las teclas o el ratón en la ventana tiene su orden, para usarlo desde un guion y para probarlo:

| Orden | Qué hace |
|---|---|
| `Panel [VERTICAL] [PESTANA] [CERRAR] [SIGUIENTE]` | Parte el panel, abre una pestaña, cierra este panel, pasa el teclado al siguiente |
| `Completar línea` | Dice lo que daría Tab al final de esa línea |
| `Resaltar línea` | Dice la clase de cada letra de esa línea |
| `Teclea texto` | Teclea en este panel (letras y `<TAB>`, `<INTRO>`, `<CTRL-R>`...) y dice cómo queda la línea |
| `Historial [BORRAR]` | Enseña el historial, o lo borra (también el del disco) |
| `Prompt [texto]` | Cambia el indicador de este panel (`%S` el cajón, `%N` el número, `%R` el código) |
| `Completa [orden plantilla]` | Da la plantilla de una orden para completar y resaltar; sin nada, las enseña |
| `Medidas` | Lo que ha costado escribir en la consola y pintarla desde la última vez |

### Estudio: el terminal completo

Lo que haría falta para programas a pantalla completa (un editor, un visor con páginas, `top`).

**Lo que es.** Un programa así no escribe líneas una tras otra: pide una pantalla suya (la "alternativa"), pone el
cursor donde quiere (`CSI fila;columna H`), borra trozos, y lee el teclado tecla a tecla (sin que nadie edite la línea
ni haga eco). Al acabar, vuelve la pantalla de antes. Es lo que hacen `vi`, `less` y compañía con las secuencias de
un VT100 o de xterm.

**Lo que ya hay.** `usuario/src/terminal.rs` es justo eso: una rejilla de celdas que entiende mover el cursor, borrar,
insertar y quitar líneas, los colores y la pregunta por la posición del cursor. La usa la terminal de la caja Linux y
tiene sus pruebas hostiles. No hay que escribir otra.

**Lo que falta**, en orden:

1. **El modo crudo en el protocolo de las órdenes.** Hoy una orden lee líneas (`LEER`); hace falta `LEER_TECLA` (cada
   tecla según se pulsa, con las flechas como secuencias de escape) y `MODO` (pedir y soltar la pantalla alternativa,
   quitar el eco). El Shell, mientras una orden tiene el modo crudo, no edita la línea ni mira Tab ni Ctrl-R: le
   pasa las teclas. Ctrl-C deja de ser "romper" y pasa como tecla; hace falta otra salida de emergencia (cerrar el
   panel la tiene: mata sus trabajos).
2. **El panel con dos consolas.** La de líneas (la de ahora) y, mientras dure el modo, una `Terminal` del tamaño del
   panel. Pintarla es lo mismo (filas de celdas). Al cambiar el panel de tamaño, la orden tiene que saberlo: un aviso
   en las señales de la respuesta siguiente ("ha cambiado el tamaño") y `CONSOLA` para preguntar cuánto mide.
3. **`terminal.rs` al crate `shell/`**, con celdas de las de ahora (fondo, 256 colores, color directo, estilos), que
   hoy no tiene: solo 16 colores, negrita e inverso.
4. **Una biblioteca para las órdenes** que quieran pantalla completa (poner el cursor, borrar, leer una tecla), para
   que no escriban escapes a mano. Y un programa que lo use: el primero sería `More`, luego un editor.
5. **Seguridad**: las mismas reglas de ahora. Ni el título, ni el portapapeles, ni contestar a preguntas que no sean
   la posición del cursor y el tamaño. Una orden rota en modo crudo no deja el panel inservible: al morir se vuelve
   a la consola de líneas.

**Por qué no está hecho.** Cabe bien en la consola (es añadir una segunda vista al panel, no cambiar la que hay), pero
sin un programa que lo use no se puede probar de verdad, y el primero (un editor o `More`) es otro encargo. Hacer solo
la mitad del Shell sería un apaño: queda diseñado y apuntado.
