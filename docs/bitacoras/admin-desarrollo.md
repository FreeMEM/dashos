# Bitácora: la tanda de administrador, el modo de desarrollo y abrir encerrado (admin-desarrollo)

Rama `admin-desarrollo`, sacada de `revision` (2cffd5c5). Decisión 1 de S36 (aceptada el 2026-11-16): "una tanda es una
acción" y "modo de desarrollo por aplicación", con S0 ("Amigable de 2026"), S6, S6b (acciones de administrador,
`guardian`, perfiles, regla 6) y S4b (capas). Papel: implementa. No se leyó código ajeno a DashOS.

## 2026-11-16

### Lo que había

- Acciones de administrador: tres, en `protocolo::sesiones::ACCIONES`: `comprobar` (el disco, por el comprobador
  aparte), `matar` (un proceso de otro o del sistema) y `registro` (leer el de todos). Se piden con `ADMINISTRAR` (una
  acción y un objetivo). `guardian` **no tenía ventana** para la contraseña: en la prueba contesta él mismo con
  contraseñas fijas, y en la imagen de usar (`atender`) las negaba todas.
- Perfiles: ninguno en las fichas. Todo lo que lanza `sesiones` (barra, Workbench, Shell) tiene `Home:`, `Work:`,
  `RAM:` y la capa del usuario. La función de la tabla encerrada (`asignaciones::de_sesion(.., true)`) existía sin uso.

### 1. Una tanda es una acción

- **Biblioteca** `usuario/src/tanda.rs` (sin dependencias, probada en el PC): leer la lista (una acción por línea,
  `matar 65540`), su frase en llano y la respuesta (`Resultado`: estado, hechas, total, en cuál se paró y el resultado
  de cada una). Topes: 4 KB de texto, 16 acciones, 120 bytes por línea.
- **`sesiones`**, orden 120 `TANDA`: la lista llega en una memoria (64 KB como mucho), se **copia al llegar** y se lee
  entera. Si una línea no es de la lista cerrada (-1), no se entiende, sobra o falta un argumento, o la lista es enorme
  (-10), o nombra un proceso que no existe (-1): no se pregunta ni se hace nada, y queda en el registro. Si vale, guarda
  la lista en llano (con el nombre y la persona de cada proceso que cierra) y pregunta a `guardian` ("*tanda"). Una
  sin contestar por programa (`OCUPADO`); desde una sesión de detrás, no.
- **`guardian`**, ventana nueva (`Pregunta::Tanda`): pide la lista (orden 121 `TANDA_LISTA`), la enseña entera y
  numerada, con la frase secreta del administrador (S6b), y dos cajas: administrador (se propone la persona de delante
  si es administradora; Tab cambia) y contraseña (puntos). Intro hace, Esc no. Lo del primer segundo no cuenta; espera
  dos minutos. Contesta con `RESPUESTA_ADMIN` de siempre; la contraseña la comprueba `sesiones` y se borra.
- **Hacerla**: con la contraseña buena de un administrador, solo las de la lista copiada, en orden, cada una por quien la
  tiene a su cargo (`comprobar` por el comprobador aparte, sin parar a `sesiones`: la tanda sigue cuando contesta).
  **Se para en la primera que falle**: las de antes quedan hechas (cerrar no se deshace) y las de después no se hacen,
  porque pueden contar con las de antes (crear un usuario y luego sus grupos) y el usuario dijo que sí a esa lista en
  ese orden. Cada acción hecha o fallida, una línea en `Estado:Registros/seguridad`.
- **Una acción suelta** (`ADMINISTRAR`) sale en la imagen de usar con la misma ventana (una tanda de una): `sesiones`
  pone el proceso de "matar" en la pregunta. Antes se negaban todas allí.
- **Orden `Administrar`** (`ACCION/M,DESDE/K`): `Administrar comprobar "matar 65540" registro` o `Administrar DESDE
  plan.txt`; dice en llano qué se hizo, en cuál se paró y qué quedó sin hacer.

### 2. Modo de desarrollo por aplicación

- Es **un permiso más de lo concedido**, `desarrollo`, en `Home:Prefs/Permisos/concedidos`. Por qué ahí y con
  `Permiso`, y no una orden nueva: es lo que una persona deja hacer a un programa, por programa y usuario, confirmado
  en `guardian`; así ya se lista (`Permiso`, Ajustes > Permisos, con su frase), se quita, se guarda donde manda S36 y
  queda en el registro, sin un segundo sitio ni un segundo formato. Diferencias: no da llave, no se puede pedir como
  permiso de primer uso (`SERVICIO desarrollo` da -1) y un programa encerrado no puede dar ni quitar permisos (-2 sin
  preguntar).
- `Permiso DAR editor desarrollo` y `Permiso QUITAR editor desarrollo`; la pregunta dice "Shell quiere dar a editor
  permiso para cargar bibliotecas, órdenes y tipos de letra de tu Home: (modo de desarrollo)".
- Efecto: al abrir esa aplicación encerrada (abajo), además de su encierro tiene `Home:C`, `Home:Libs`, `Home:Fonts`
  y `Home:Classes` solo para leer (raíces 11 a 14), como volúmenes `HomeC:`, `HomeLibs:`, `HomeFonts:` y
  `HomeClasses:`, delante en `C:`, `LIBS:`, `FONTS:` y `CLASSES:`. Así `Assign` enseña de dónde sale cada cosa, y
  `Home:` entero sigue fuera de su alcance. Quitarlo: al relanzar vuelve el encierro (lo abierto conserva sus raíces
  hasta que acaba).
- **En su ventana**: la marca del compositor de S29 (la de las cajas) en magenta, con "DESARROLLO" en la pestaña o en
  la barra de DashOS, y "Desarrollo · " delante del título en la barra de la pantalla. Por qué así y no un texto en el
  título: la pinta el compositor encima de todo lo del programa, también maximizado y sin la barra de DashOS, y el
  programa no la puede tapar ni quitar; el título lo pone el programa. Se la dice `sesiones` (aviso sin marca
  `0x5536_0020`, antes de crear el proceso); ningún cliente la puede poner ni quitar.
- **En el registro**: cada vez que se abre en modo de desarrollo, una línea de seguridad ("se abre en modo de
  desarrollo: carga de Home:C, ...", "lo abrió Shell"); darlo y quitarlo, como cualquier permiso.

### 3. Abrir encerrado desde el Shell

- La ficha lleva **perfil** (`PERFIL_LIBRE`, el de siempre; `PERFIL_APLICACION`, encerrado) y si va en desarrollo; lo
  heredan sus ayudantes.
- **`sesiones`, orden 122 `ABRIR`** (dentro del mismo código que `LANZAR`): un programa de `Programas/` (el
  `Paquetes:` de hoy), con su identidad (sus permisos), raíces 0 (`PROGDIR:`), 1 (`SYS:`, leer) y 10
  (`Home:Datos/<programa>`, `Datos:`, creado suyo si falta, con `HUECO_SUYO`), sin `Home:`, `RAM:` ni `Work:`, y la
  tabla de la sesión sin la capa del usuario. Lo pide un programa de una persona con su sesión abierta que no esté
  encerrado (el Shell y lo que lanza).
- **Arreglado de paso**: el ayudante de un programa encerrado (`LANZAR_AYUDANTE`) nacía con `Home:`, `RAM:` y `Work:`
  (el alta era la de siempre) y podía pedir su `PROGDIR:` en `Home:`. Ahora nace encerrado como su padre, y su
  `PROGDIR:` solo puede salir de `PROGDIR:` o `SYS:`.
- **Orden `Abrir`** (`PROGRAMA/A,ESPERAR/S`): `Abrir editor`, `Abrir Programas:editor` o `Abrir SYS:Programas/editor`;
  con `ESPERAR`, acaba cuando acaba el programa. Nombre: lo que hace es lo del doble clic ("abrir"), y más adelante puede
  abrir un documento con su programa.

### Pruebas

- PC: `usuario/src/tanda.rs` (7: lista buena, lo que no vale, listas enormes, 20.000 listas al azar, frases con
  escapes, respuesta mala), en `make prueba`. `wayland`: `marca_del_modo_de_desarrollo` (la marca y el título; un cliente
  que se manda el aviso no consigue nada; se va al acabar).
- QEMU (`make arranque-virt`), en el guion del Shell (`S:Prueba-Shell`):
  - `Administrar bloquear registro`: no se pregunta. `Administrar comprobar registro`: ventana, contraseña de Ana (la
    teclea el guion), comprobar sigue tras el comprobador.
  - `SYS:Programas/espera` lanzado desde el Shell, la prueba hostil de la tanda: una no de administrador; listas enormes (17 acciones, 5000 bytes, 1 MB de
    memoria, sin memoria); un proceso que no existe; otra tanda mientras espera (`OCUPADO`); la lista cambiada en su
    memoria tras mandarla (se hace la enseñada: el proceso de la lista cambiada sigue vivo); una acción colada detrás
    (pide su confirmación, no se hace); una que falla en medio (se para en la 2, la 3 no se hace); la contraseña mala
    (nada).
  - `Abrir espera` tres veces: sin modo no carga `LIBS:prueba.library` de `Home:Libs`; con `Permiso DAR espera
    desarrollo`, sí (de `HomeLibs:`); quitado, otra vez no. En cada una no llega a `Home:`, `Work:`, `RAM:`
    ni `ENV:` por nombre ni a las raíces 2, 3 y 4 a mano, escribe en su `Datos:`, no se pone a sí misma el modo, no
    escribe lo concedido; su ayudante con `PROGDIR:` pedido en `Home:` nace sin él. El Shell no puede escribir el modo
    a mano en `concedidos`; `Abrir` de algo que no es un programa instalado, no.

- **Los programas de prueba van dentro de `Programas/espera`** (`usuario/src/prueba_encierro.rs`), que según cómo se
  lanza hace de aplicación encerrada (abierto con `Abrir`), de su ayudante, de la prueba hostil de la tanda (desde el
  Shell) o lo de siempre (la barra). Con dos programas aparte (`encerrada` y `tanda`, 100 y 112 KB) el disco de pruebas
  de 7 MB se llenaba y fallaba otra prueba ("[ficheros] A15 sujetar sin soltar": -11, lleno). Así `espera` crece de 73 a
  108 KB.
- Primera vuelta de la hostil: la segunda tanda se proyectaba donde seguía la primera (no se mandaba) y la lista se
  cambiaba antes de que `sesiones` la leyera (cambiarla antes de que llegue es mandar otra: se enseña esa). Ahora se
  cambia cuando la otra ya dio `OCUPADO` (`sesiones` atiende en orden: ya la copió).
- **`Abrir espera ESPERA` cogía "espera" como el interruptor** (las plantillas del Amiga no distinguen mayúsculas): el
  interruptor se llama `ESPERAR`.

### Dos arreglos fuera de mi zona (pequeños; que los mire quien lleva cada cosa)

- **`archivos`, A15**: con el disco sujeto, una escritura podía quedarse sin sitio ("[ficheros] A15 sujetar sin
  soltar: MAL", `escribir -11`), aunque A15 promete que no. `vigilar_sujecion` solo mira el recuento de bloques libres
  tras guardar; con el disco más lleno (aquí, 36 KB más de `espera`), el siguiente trozo de 64 KB no cabía antes de esa
  mirada (falló 2 de 4 vueltas). Ahora, si una escritura da `ENOSPC` con el disco sujeto, se suelta y se repite una
  vez (`soltar_por_sitio`). Lo que no sé: si es por bloques troceados o por lo que gasta cada vuelta además de los
  256 KB; el arreglo vale para los dos.
- **`cambio`, la prueba del panel impostor**: con la cola del compositor llena (la caja Amiga abriendo sus ventanas a
  la vez, que coincide porque la prueba del Shell es ahora más larga) perdía su primer mensaje ("sin compositor", o "0
  globales" y "objeto que no existe"). Reintenta como `ventana_cliente`: hasta 5 s para conectar y unos 2 s por mensaje.

### Resultados

- `make prueba`: bien (con `tanda.rs` y la prueba nueva de `wayland`). `cargo test` de `shell` (71) y `wayland` (117):
  bien.
- `make arranque-virt`: bien a la séptima. Las seis de antes, por lo de arriba: el disco lleno (dos veces), la segunda
  tanda en la misma dirección, `ESPERA`, el panel impostor (dos veces) y las líneas esperadas tras cambiar el formato.
  460 s con la construcción, el disco, las capturas y el reinicio.
- `make arranque-virt-2`, `make arranque-virt-linux` y `make arranque-virt-shell`: bien a la primera.

### Lo medido

Nada de rendimiento: una tanda son los viajes de siempre a `sesiones` (pedirla, la lista para `guardian` en páginas de
224 bytes, la contraseña) y cada acción lo que costaba suelta. `espera` pasa de 73 a 108 KB en el disco.

### Pendiente del usuario

- **Parar en la primera que falla** (y no seguir): razonado arriba.
- **La ventana de la tanda**: título "N acciones de administrador", la lista numerada, la frase secreta del
  administrador, dos cajas (administrador y contraseña), Intro hace y Esc no, Tab cambia de caja; 2 minutos de plazo;
  un solo intento (con la contraseña mala, la tanda entera es "no" y el programa la pide otra vez). Topes: 16
  acciones, 4 KB de lista.
- **Una acción suelta** sale en la imagen de usar con la misma ventana (antes se negaba sin preguntar).
- **El modo de desarrollo como permiso** (`desarrollo` en lo concedido, con `Permiso`) y la confirmación sin
  contraseña de administrador (la de `Permiso DAR`): es la capa del propio usuario, no de la máquina. S6 pide
  contraseña para el modo de desarrollo de la máquina (piezas de `SYS:`), que no está hecho.
- **La marca**: magenta, "DESARROLLO" y "Desarrollo · " en la barra.
- **Los nombres**: `Abrir` (y su `ESPERAR`), `Administrar`, `Datos:`, `HomeC:`, `HomeLibs:`, `HomeFonts:`,
  `HomeClasses:`.
- **El doble clic sigue lanzando como el Shell** (con `Home:`, `Work:` y `RAM:`): pasarlo a encerrado cambia lo que ven
  los programas de prueba de `Programas/` y es una decisión de cuándo. Hoy solo `Abrir` abre encerrado.
- **Lo que tiene una aplicación encerrada**: sin `RAM:` ni `T:` (S6b dice `RAM:T/<número>`, sin hacer), y su `ENVARC:`
  solo el de fábrica (S36, duda 4).

### Lo que queda

- El perfil "suelto": `Abrir` de un programa que no está en `SYS:Programas` se niega. Le falta identidad propia (S6b,
  2026-11-15): con el nombre del fichero se llevaría los permisos y el modo de desarrollo de otro.
- Instalar paquetes y crear usuarios como acciones de la tanda: no existen aún (S8, cuentas fijas en el código).
- Que quitar el modo de desarrollo a una aplicación abierta le quite ya las raíces (hoy, al relanzar).
- La lista de Ajustes > Permisos enseña el modo de desarrollo como un permiso más, con su frase: no se miró en pantalla.

### Lo que no se comprobó

- La ventana de la tanda ni la marca magenta en pantalla (no hay captura que las mire): solo sus líneas y la prueba de
  `wayland`. En la prueba del QEMU `espera` no abre ventanas, así que la marca no se pinta de verdad.
- Un administrador de verdad tecleando con el teclado real; el guion teclea por el monitor de QEMU.
- Que el arreglo de A15 sea el bueno para lo que de verdad pasa (ver arriba).
