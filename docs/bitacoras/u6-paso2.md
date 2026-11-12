# Bitácora: cambio rápido de usuario, paso 2 (U6)

Rama `u6-paso2`, sacada de `revision`. Diseño en `docs/diseño/U6-cambio-rapido.md` (puesto al día); el paso 1, en
`docs/bitacoras/usuarios-u6.md`.

## 2026-11-12

### Cómo sabe el compositor quién está detrás

- `inicio` da a `sesiones`, sin marca, una copia sin marca del canal del compositor y del de sonido (orden nueva
  `CANAL_SISTEMA`, 19). La del compositor, al lanzar `sesiones`; la del sonido, al crear su canal. Solo `inicio` y
  `sesiones` tienen una así: un cliente, con su marca, no puede decir nada de sesiones.
- `sesiones` les manda, sin marca, dos avisos (`dashos::protocolo::sesiones` y los mismos números en
  `wayland::escena`): `AVISO_PERSONA` (marca, usuario y nombre; 32 bytes) y `AVISO_DETRAS` (las personas con su sesión
  abierta detrás; 40 bytes). De longitud distinta a las entradas de directorio de `inicio` (24).
- Cuándo: el de persona, al dar de alta un programa de una persona, antes de lanzar uno (con cualquier usuario: así
  el compositor tiene su nombre) y, con usuario 0, en su baja. El de detrás, cuando cambia, antes de contestar a
  `guardian`: llega al compositor antes que el desbloqueo, por el mismo canal.
- Si `inicio` relanza al compositor o al sonido, `sesiones` le dice todo otra vez al ver su alta de vuelta.
- Lo que lanza `sesiones` nace con una tercera llave, la del compositor, marcada con su número. Su nombre es solo
  para los mensajes: los poderes siguen siendo por el nombre de `inicio` (un `Programas/panel` no sería el panel).

### El compositor

- `Escena`: `personas` (cliente → usuario), `detras` y `oculto(cliente)`. `aviso_de_sesiones(marca, mensaje)` solo
  hace caso con la marca 0.
- Lo oculto sale de `raices()`: no se pinta y el puntero no lo encuentra. `dar_teclado` y `puede_tomar_teclado` lo
  rechazan; la capa con teclado exclusivo, también. Al cambiar de sesión: daño entero, quien se oculta pierde teclado
  y puntero (con sus eventos de salida) y el teclado pasa a la ventana de arriba que se ve. Lo mismo al desbloquear.
- Los avisos de `frame` de lo oculto, uno por segundo como mucho (`RITMO_OCULTO_MS`): el compositor espera como mucho
  eso si alguno espera.
- Prueba de la escena, `sesion_de_detras`: un cliente no cambia quién está detrás; con Ana detrás, su ventana no se
  pinta, no recibe teclas ni clics, no se lleva el teclado y sus avisos salen a ritmo bajo; al volver, se ve.

### El sonido

- Los flujos de una sesión de detrás, con volumen 0 (con la rampa del mezclador: sin chasquido). Se siguen leyendo a
  su ritmo. El volumen que pida el programa se guarda y se pone al volver. Lo dice: "en silencio, su sesión está
  detrás" y "suena otra vez; en silencio se leyeron N KB de su anillo".

### Permisos por programa y usuario

- `CONCEDIDOS` en memoria y en el fichero llevan el usuario: "programa servicio usuario". Una línea sin usuario no vale.
- Varias copias de un programa esperando la misma pregunta: se guarda para el usuario de quien preguntó primero (va en
  la pregunta); las demás reciben la llave esa vez. En la prueba, las tres copias de `pide` (dos del sistema y una de
  Carla) se juntan en una pregunta; si la de Carla llega tarde, se le pregunta aparte. `guardian` ya no exige "2
  preguntas a pide", sino una por servicio y usuario.
- El centro de control ve y quita lo de su usuario y lo del sistema; `guardian`, todo. `guardian` retira al final lo
  que quede de `pide` y de `cambio`, para que el disco acabe sin permisos.
- `sonido` entra en lo que reparte `sesiones` (con la llave de `CANAL_SISTEMA`), sin entrada de directorio. Permiso
  fijo de `cambio` para sonar.

### La prueba en virt

- `cambio` como Ana abre una ventana de 240 × 160 de un color que no tiene nadie (0xe010b0), suena (400 Hz muy bajo,
  para que `sonido_wav.py` no lo confunda ni un corte cuente como chasquido) y pide el permiso de plazo. `guardian`,
  en la prueba, contesta a `cambio`: sí como Ana, no como Carla.
- Con Carla delante: el guion captura la pantalla (`CAPTURA_U6`); `tools/captura_cambio.py` comprueba que con Ana
  delante la ventana se ve y con Carla no queda ni un píxel. El guion sigue tecleando "a" y pulsando: a la ventana de
  Ana no le llega nada (lo cuenta el propio `cambio`). Sus avisos de frame, a ritmo bajo; su tono se sigue escribiendo
  y el servicio lo dice en silencio.
- El `cambio` de Carla pide el mismo permiso: se le pregunta y es -2.
- Prueba hostil (aviso de la rama `escritorio`: en `sesiones`, un `Programas/guardian` habría tenido los poderes de
  `guardian`; allí ya está arreglado): en el compositor, los poderes por nombre (`ESCRITORIOS`, `PONEN_TEMA`,
  `ABREN_PANEL`, `SISTEMA`) solo miran los nombres que da `inicio`; los de lo que lanza `sesiones` van a otro mapa,
  solo para los mensajes. En el sonido, igual (`PONEN_GENERAL`), y `sesiones` no le da entradas de directorio. En el
  disco hay una copia de `cambio` llamada `Programas/panel`; `guardian` la lanza como él mismo (sin `Home:`): no ve
  las capas, el bloqueo ni los avisos de inactividad, y pedir el bloqueo da error de protocolo.
- `sesiones` da 8 MB a cada programa que lanza (antes 4: con montón, que pide trozos de 4 MB, `cambio` no arrancaba),
  y su cupo pasa de 16 a 64 MB en `inicio`.

### Pruebas

- `cd wayland && cargo test --lib`: 43 bien (nueva, `sesion_de_detras`).
- `make prueba`: BIEN.
- `make arranque-virt`: la primera vuelta, `cambio` no arrancaba (sin memoria para el montón). La segunda, Ana contó
  una entrada del teclado "detrás" que era de después de volver (se medía tarde) y la captura de Carla salía con su
  sesión ya bloqueada. Arreglado: se cuenta hasta la última vez que `ACTIVA` dijo detrás, y `guardian` deja 3 s la
  sesión de Carla antes de bloquearla. Tercera vuelta: las cuatro BIEN y la de la ventana de Ana. Cuarta, con la prueba hostil del nombre: igual, todo BIEN.
- Capturas en `capturas-agente/` (fuera de git): `u6-ana.png` (la ventana de Ana, magenta, delante) y `u6-carla.png`
  (la sesión de Carla: no está).

## Pendiente

- Lo copiado (portapapeles) y los avisos, por sesión; capturar la pantalla, solo la propia sesión (cuando existan).
- Las preguntas de detrás esperan en vez de negarse; lista de cuentas en el bloqueo; "cerrar la sesión de otro"
  (paso 3).
- Lo concedido, en el `Home:` de cada uno (S6b), con `Prefs` de usuarios.
- `sesiones` sigue reconociendo a `guardian`, `entrada` y `centro` por el nombre de su ficha: lo arregla la rama
  `escritorio` (al fusionar, juntar con esta).
- Probar en la placa.
