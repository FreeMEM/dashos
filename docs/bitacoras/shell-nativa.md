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
| `usuario/src/orden.rs` | DashOS | Lo que usa cada orden: sus argumentos, escribir y leer por el Shell, y los ficheros por su ruta con nombre |
| `usuario/src/bin/shell.rs` | `SYS:System/Shell` | La ventana, el intérprete y los trabajos |
| `usuario/src/bin/ordenes/*.rs` | `SYS:C` | Las órdenes, un programa cada una |

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

1. **Cualquier programa lanzado del disco puede lanzar ayudantes con su ficha**, no solo el Shell. Hoy no hay perfiles
   en las fichas (todo lo que lanza `sesiones` es "como el Shell"), así que no da nada que no tuviera. Cuando haya
   perfiles encerrados, habrá que decidir si una aplicación encerrada puede.
2. **Lo lanzado desde el Shell sale en el monitor con el nombre del Shell.** La ficha tiene un solo nombre, que es la
   identidad para los permisos. S6b habla de "aplicación" y "nombre" por separado: falta el segundo campo.
3. **`*` vale como `#?`** en los patrones (en AmigaDOS es opcional).
4. **`..` y `.` se entienden** en las rutas, además de la `/` del Amiga.
5. **Ctrl-C es romper**, no copiar (copiar es Ctrl+Mayús+C, como en la terminal de la caja Linux).
