# Bitácora: lo que quedó de las cuentas (cuentas-2)

Rama `cuentas-2`, sacada de `revision` (5470528a). Sigue a `docs/bitacoras/cuentas.md`. Diseño: S6b ("Dónde se guardan
las cuentas", "El cajón de cada usuario", "Entrar, bloquear y salir", "Primer arranque", "Administradores") y S0
("amigable de 2026 + nivel hacker"). Papel: implementa. No se leyó código ajeno a DashOS.

## 2026-11-16

### 1. Cambiar la propia contraseña

- **Sin reescribir los datos**: orden nueva del servicio de archivos, `CAMBIAR_CLAVE` (42, solo con el derecho de
  administrar, que tiene `sesiones`). La ranura que abre la contraseña de ahora vuelve a envolver la misma llave del
  volumen con la nueva, con una sal nueva y el mismo coste de Argon2 (`dashfs`: `change_volume_password`). Las dos
  copias del bloque de llaves cambian en la misma transacción: un corte de luz deja la de antes o la nueva. Con el
  volumen abierto (la sesión de la persona), lo abierto sigue.
- **El programa no ve ninguna contraseña**: `sesiones` (`CLAVE_PROPIA`, 130) le pregunta a `guardian` ("*clave"), que
  enseña su ventana con tres cajas (la de ahora, la nueva y repítela) y la frase de la persona. Cada intento va a
  `sesiones` (`RESPUESTA_CLAVE`, 131); si la de ahora no es, si hay que esperar o si la nueva es corta, la ventana sale
  otra vez con el aviso en rojo. Dos nuevas distintas lo dice la ventana sin preguntar a nadie.
- **Solo la propia**: la de la persona del programa (el centro de control, que es del escritorio, la de la persona de
  delante), desde su sesión, delante, y nunca un programa encerrado. Con el nombre de otra cuenta no se pregunta.
- Desde el Shell: `Usuario CLAVE`. Desde Ajustes: Cuentas, "Mi contraseña".
- Cada cambio, y cada negativa, en `Estado:Registros/seguridad`.

### 2. Entrar con el código de recuperación

- En la pantalla de entrada (y en la de bloqueo), con la contraseña a la vista, **Tab: olvidé la contraseña**: el
  código de recuperación y la nueva dos veces. `guardian` lo manda a `sesiones` (`RECUPERAR`, 132; solo de `guardian`).
- El código vale como se teclee: minúsculas, espacios y guiones donde sea (`cuentas::normalizar_codigo`), pero solo las
  24 letras del código.
- **El código se gasta**: todas las contraseñas de antes se van, la nueva es la única, y el código usado ya no vale:
  el servicio saca otro de su azar (`CAMBIAR_CLAVE` con `CON_CODIGO`; `dashfs`: `recover_volume`), que `guardian` enseña
  para apuntarlo otra vez. Con Intro, entra.

### 3. Espera que crece

`cuentas::Esperas`: tras 3 intentos malos seguidos con una cuenta, 5 s sin probar ninguno (ni el bueno), y el doble con
cada uno más, hasta 5 minutos. Cuentan juntos entrar, volver de un bloqueo, la contraseña de administrador de una
tanda o una acción, la de ahora al cambiarla y el código de recuperación; los nombres que no son de nadie, en una cuenta
aparte (la 0). Uno bueno lo deja a cero. Solo cuenta lo que pide `guardian` (un programa que pide entrar no llega a
probar nada y no puede dejar a otro esperando). Mientras, se contesta `OCUPADO` con los segundos que faltan, y la
pantalla dice "Demasiados intentos. Espera N s". Al empezar una espera, una línea en el registro de seguridad. Los 300 ms
de después de una mala, como antes.

### 4. Primer arranque

Sin `Estado:Cuentas/usuarios` (o vacío, sin ninguna línea, ni mala), `sesiones` lo dice (`CUENTAS_ENTRADA`, 134) y la
pantalla de entrada es un asistente: nombre corto, nombre completo, contraseña y repítela. `sesiones` (`PRIMERA_CUENTA`,
133; solo de `guardian` y solo si no hay ninguna cuenta) la crea como la acción `crear` de la tanda, la pone en `admin`,
con su volumen cifrado y su código de recuperación, que `guardian` enseña; con Intro, entra. Si `usuarios` tiene líneas
pero ninguna vale, no es un primer arranque (algo se dañó: no se ofrece crear una administradora encima) y nadie entra.

### 5. La lista de cuentas en la pantalla de entrada

La pantalla de entrada y la de "Cambiar de usuario" (Esc en el bloqueo) enseñan las cuentas activas (nombre completo,
nombre corto y un círculo con la inicial), con "Otra persona…" al final para teclear el nombre. Flechas e Intro para
elegir; una letra empieza a teclear el nombre (así siguen valiendo los guiones que lo teclean). Tras una contraseña mala,
la persona elegida en la lista se queda; un nombre tecleado se vuelve a pedir (como antes).

### 6. Ajustes: Cuentas

Una categoría nueva en el centro de control, la última, tras "Acerca de" (para no mover las de antes, que el guion de
`make arranque-virt` recorre con las flechas contra un plazo de 60 s) (el `Usuarios` de S4b): la lista de `Estado:Cuentas/`
(número, grupos, si está borrada y hasta cuándo se puede devolver), un campo para el nombre de una cuenta nueva o de un
grupo, y los botones Nueva cuenta, Grupo (pone o quita la elegida en el grupo escrito), Mi contraseña y Borrar (que dice
Devolver si la elegida está borrada). Todo pasa por la tanda de administrador que ya existía, como la orden `Usuario`.

### Lo hostil

- Contraseña de ahora mala (la ventana lo dice y deja probar otra vez), dos nuevas distintas (no se manda), una nueva
  corta (8 letras como mínimo), Esc (no se cambia).
- `Usuario CLAVE Carla` siendo Ana: no se pregunta. Un programa (`Programas/espera`, en el guion del Shell) que pide
  `CLAVE_PROPIA` con el nombre de Carla, `RECUPERAR`, `PRIMERA_CUENTA` o `CUENTAS_ENTRADA` (solo de `guardian`) o
  contesta por `guardian` (`RESPUESTA_CLAVE`): no. `CAMBIAR_CLAVE` directo al servicio de archivos, sin el derecho de
  administrar: -12.
- Recuperar con un código malo, con el bueno y una contraseña corta, con el gastado otra vez, con una cuenta que no
  existe: no. El bueno tecleado en minúsculas y sin guiones: sí.
- Crear "la primera cuenta" con cuentas ya hechas: -120 (`HayCuentas`), y al registro. En el asistente, un nombre
  reservado ("admin"): lo dice y deja corregirlo.
- Tres malas seguidas con Berta: la cuarta, la buena, ni se prueba (espera 5 s); pasada la espera, entra.

### Pendiente del usuario

- **La espera que crece**: 3 malas seguidas sin esperar, luego 5 s, 10, 20... hasta 5 minutos; por cuenta (también la
  contraseña de administrador y el código de recuperación cuentan); solo en memoria (al reiniciar, de cero). Quien
  conoce un nombre puede dejar esa cuenta esperando (como en otros sistemas): no hay forma de distinguirlo de un ataque.
- **El código de recuperación se gasta al usarlo** y sale otro (para apuntar otra vez). Recuperar quita todas las
  contraseñas de antes.
- **La lista de cuentas se enseña siempre** en la entrada y en "Cambiar de usuario" (U6, decisión 4: dice quién usa el
  equipo). No hay aún un ajuste para esconderla.
- **Primer arranque**: la primera cuenta es administradora; si `usuarios` tiene líneas pero ninguna vale, no se ofrece el
  asistente (nadie entra). Sin frase secreta (S6b: la frase de cada persona en las ventanas del sistema): las frases
  siguen fijas en `guardian` para Ana y Carla; una cuenta nueva no tiene.
- **Las teclas**: Tab en la contraseña es "olvidé la contraseña"; Esc vuelve a la lista. También con el ratón (las
  filas, las cajas y los botones del pie).
- **Ajustes, Cuentas**: Nueva cuenta con el nombre del campo, sin nombre completo (cambiarlo después no existe aún, ni
  en el Shell); Grupo pone o quita según esté; Borrar o Devolver según esté la elegida. Mientras la ventana del sistema
  pregunta, Ajustes espera sin repintar (como al comprobar el disco).

### Arreglos del guion de `make arranque-virt` (`tools/prueba_uefi.sh`)

- **El tramo del centro de control**: con la máquina cargada, las ventanas de la caja Amiga (Ajustes, ReAction) y la de
  Dash salían encima de Ajustes en medio de su tramo y se llevaban los clics y las teclas del guion; la prueba se
  quedaba ahí (varias vueltas seguidas) o no llegaba a "Acerca de" en los 60 s que le da `guardian`. Ahora el guion trae
  Ajustes delante con un clic dentro de su ventana, junto al borde derecho (un clic en la barra de título no la subía),
  antes de pulsar; si no se quitó el permiso lo repite con el ratón, y en todo el tramo, si la barra dice que el teclado
  lo tiene otra ventana, se lo devuelve a Ajustes. El plazo de `guardian` para ese tramo se queda en 60 s: con 90, `make
  arranque-virt-2` (sin guion, espera el plazo entero) no llegaba al bloqueo en sus 200 s.
- **El tiempo de `make arranque-virt`**: 720 s en vez de 600 (con la máquina cargada, el paso del explorador se comía
  lo que quedaba).
- **La ventana de cambiar la contraseña** guarda las nuevas tras "la de ahora no es esa": en la segunda vez el guion
  teclea solo la de ahora.
- **Cuentas, la última categoría**: puesta antes de "Acerca de", el guion (que baja con las flechas, una cada 4 s) no
  llegaba a "Acerca de" antes del plazo de 60 s de `guardian` para ese tramo.

### La prueba de las ventanas nuevas: `make arranque-virt-cuentas`

Arranca con un disco sin cuentas (`tools/disco_archivos.py --sin-cuentas`) y `tools/guion_cuentas.py` recorre todo con
el teclado y el ratón, con una captura de cada ventana (fuera de git, en `~/recuperar/geosian-7d/cuentas-2-capturas/`):
el asistente (primero "admin", que no vale), el código de Eva, Ajustes > Cuentas (crear a Fran, ponerlo en "fotos",
borrarlo y devolverlo, Mi contraseña), cerrar la sesión desde el panel, la lista, Fran olvida la contraseña (un código
malo, luego el bueno en minúsculas y sin guiones), su código nuevo, y al final Eva con la contraseña que se puso.

- La ventana de Ajustes no cabía con la categoría Cuentas (los botones se salían): dos filas de botones, textos más
  cortos y la lista de dos filas.
- Tras cerrarse una ventana del sistema, el primer clic en Ajustes solo le devuelve el foco: el guion pulsa antes dentro.
- La comprobación del disco tras `make arranque-virt` pilló un fallo: al recuperar, `sesiones` borraba la ventana
  compartida antes de copiar el código nuevo, y el que enseñaba eran ceros. Arreglado (se copia y luego se borra), y la
  prueba de `guardian` mira ahora que el código nuevo sea un código.

### Lo que vino de fuera

- `[ficheros] A15 sujetar sin soltar: MAL` (y `[pide] comprobar el disco: -11`) en una vuelta: con la máquina cargada,
  la prueba A15 del comprobador tardó 7,6 s (3,9 en otra máquina), y la acción "comprobar" de `pide` llegó en medio de
  la de `ficheros`, que sujetaba el disco. Es una carrera entre dos pruebas del servicio de archivos; no toca nada de
  esta rama.

### Pruebas (2026-11-17)

- `rustc --test usuario/src/cuentas.rs`: 13, bien. `cargo test --release` de `dashfs`: 171 + 5 + 9, bien.
- `make prueba`, `make arranque-virt-linux`, `make arranque-virt-shell`, `make arranque-virt-cuentas` (15 capturas):
  bien a la primera.
- `make arranque-virt`: la primera vuelta, solo `[ajustes] el desplegable dice: Destello` (la familia conocida de teclas
  perdidas con carga, la arregla `firefox-firme`); el guion se quedó ahí, y con él los pasos de `guarda` y de las
  commodities, que van después. Todas las líneas de esta rama salieron. La segunda vuelta, bien entera (con el disco).
- `make arranque-virt-2`: con el plazo de 90 s en el tramo del centro, no llegaba al bloqueo (vuelto a 60). Luego, una
  vuelta con las líneas bien y la captura de la segunda cabeza con una ventana de la caja Amiga encima (llegó tarde);
  la siguiente, bien.
- Las de `make prueba`, `arranque-virt-linux`, `-shell` y `-cuentas` se pasaron con el plazo de 90 s; el cambio a 60
  solo toca el tramo del centro de la prueba de `guardian`, que esas no recorren.
