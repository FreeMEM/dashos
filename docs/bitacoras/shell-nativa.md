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
| En el Shell (tocan su estado) | `Set`, `Unset`, `Get`, `SetEnv`, `GetEnv`, `UnsetEnv`, `Alias`, `Unalias`, `Path`, `Which`, `Run`, `Status`, `Break`, `Why`, `Cls` |
| Programas de `SYS:C` | `Assign`, `Cd`, `Dir`, `List`, `Copy`, `Delete`, `Rename`, `MakeDir`, `Type`, `Echo`, `Info`, `Date`, `Version`, `Protect`, `Filenote`, `Wait`, `Ask` |

`Path`, `Which` y `Run` son internas también en AmigaOS. `Status` y `Break` allí son programas porque los procesos son
de todo el sistema; aquí hablan de los trabajos de este Shell, así que van dentro.

Códigos de retorno, los de AmigaDOS: 0 bien, 5 aviso, 10 error, 20 fallo. Un guion se para con un código igual o
mayor que `FailAt` (10 de serie).

Una orden se busca: con ruta (`SYS:C/Dir`, `sub/orden`), solo ahí; sin ruta, en el cajón actual, en los cajones de
`Path` y en `C:` (que es `Home:C` y `SYS:C`, por capas). Un fichero con cabecera ELF y permiso de ejecutar se lanza; uno
con el bit de guion del Amiga se ejecuta como guion.

Tuberías: una tras otra, no a la vez. La salida de la primera se guarda (hasta 1 MB) y es la entrada de la segunda.

### El arranque de una orden

Un programa de DashOS nace sin argumentos. Quien lo lanza puede darle una cuarta llave, **el canal de arranque**, con
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

Y en `LANZAR`, un cambio pequeño: si la petición trae una llave más, es la cuarta del programa (su canal de arranque).
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

- Las órdenes de una tubería corren a la vez. Lo que escribe una va a un depósito de 64 KB; cuando se llena, a la que
  escribe no se le contesta hasta que la otra lee (así espera sin gastar, y no hay tope de tamaño).
- Sustitución: `` `orden` `` en una línea se cambia por lo que escribe esa orden (sin el salto final), como en
  AmigaDOS. Con tope (64 KB) y sin anidar.
- Los comodines ya los expanden las órdenes, como en el Amiga (`Copy #?.txt`), no el Shell.

### 8. Complementos

- Guiones del propio Shell en `S:Shell-Complementos/` (por capas: los del usuario en `Home:S`, los del sistema en
  `SYS:S`). Se cargan al abrir, por orden alfabético, tras `S:Shell-Startup`.
- Un complemento define alias, variables, plantillas de completado (`Completa`) y segmentos del indicador. Uno que
  falla se dice y se salta; los demás siguen.
