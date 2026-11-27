# U6. Cambio rápido de usuario

Propuesta del 2026-11-12. Sale de S6b ("Una sesión cada vez en la primera versión. El cambio rápido de usuario cabe
después: las fichas ya llevan sesión") y es lo que queda de U6/R19 en `docs/REVISION_LOOP.md`. Las decisiones que
tiene que confirmar el usuario van al final.

## Qué es

Ana tiene su sesión abierta. Se levanta y la bloquea. Llega Carla y, desde la pantalla de bloqueo, entra en su
propia sesión sin cerrar la de Ana. Cuando Carla bloquea, Ana vuelve con su contraseña y encuentra todo como lo dejó.

## La sesión de quien no está delante

| Qué | Cómo queda | Por qué |
|---|---|---|
| Sus programas | Siguen en marcha | Una descarga o una compilación no se cortan. Es lo que hacen macOS, Windows y GNOME |
| Su volumen cifrado | Sigue abierto | Sus programas lo usan. Nadie más tiene una raíz en él |
| Sus ventanas | No se ven | El compositor solo enseña las de la sesión de delante y las del sistema |
| Teclado y ratón | No le llegan | Igual: solo a la sesión de delante |
| Preguntar al usuario | No puede | Quien está delante es otra persona. Sus preguntas de permisos y sus acciones de administrador se niegan (paso 1) y luego esperarán a que vuelva (paso 3) |
| Bloquear | No puede | Ya está bloqueada. Si pudiera, pintaría su pantalla de bloqueo encima de la sesión de otra |
| Sonido | En silencio (paso 2) | Que la música de Ana no suene encima de lo de Carla. Sus anillos se siguen leyendo a su ritmo: no se le llenan ni se cuelga |
| Volver | Con su contraseña | Como desbloquear. `sesiones` la comprueba con su volumen, sin reabrirlo |

Lo que se negó a la sesión de detrás no se pierde en silencio. Queda en el registro (`Estado:Registros/`, S6), con
el programa, lo que pidió y la hora. Y al volver, su dueño ve un aviso con lo que se pidió mientras no estaba ("Editor
quiso usar la red y se le dijo que no"), para que sepa por qué falló su programa. Por hacer.

`sesiones` sabe qué sesión está **delante**: la del último que entró o volvió con su contraseña. Es la única pieza
que lo decide. El compositor y `guardian` siguen lo que diga.

Una persona tiene una sesión como mucho. La sesión se nombra por su usuario: no hace falta otro número mientras sea
así. Caben 8 abiertas a la vez (lo que ya guarda `sesiones`).

## Elegir usuario

En la pantalla de bloqueo, **Esc** pasa a "Cambiar de usuario": se escribe el nombre y luego la contraseña, como en
la pantalla de entrada. Con Esc otra vez se vuelve al bloqueo de antes. Pie de la pantalla: "Esc: cambiar de
usuario". Ese pie es además un botón "Cambiar de usuario" que se pulsa con el ratón (S0: todo se puede hacer con el
ratón). Hoy solo está la tecla.

- Si el nombre es el de alguien con la sesión abierta, se vuelve a ella (contraseña comprobada, volumen sin tocar).
- Si no, se entra: se abre su volumen y empieza su sesión.
- La sesión de antes queda detrás, bloqueada.

Más adelante, en vez de escribir el nombre, una lista de cuentas (con `Prefs` de usuarios, S10c).

## Qué ve cada uno

| Qué | Un programa de Carla | Cómo se garantiza |
|---|---|---|
| Procesos | Los suyos y los del sistema. Los de Ana no salen en la lista | `sesiones` filtra `PROCESOS` por la ficha de quien pregunta (su marca) |
| Matar | Uno de Ana da "no existe", como si no estuviera | `MATAR` mira lo mismo que la lista: no se puede ni saber si existe |
| Ficheros de Ana | No llega | Solo los programas de Ana nacen con una raíz en el volumen "Ana". Las órdenes de volúmenes (abrir, ver, cerrar) piden el derecho de administrar, que un programa no tiene. La raíz del volumen es de Ana y `0700` |
| Su volumen | No lo abre ni lo cierra, ni con la contraseña buena | Igual: abrir y cerrar volúmenes es de `sesiones`, y entrar o salir solo se lo acepta a `guardian` |
| Ventanas de Ana | No las ve ni le llega su teclado | El compositor, por sesiones (paso 2). Capturar la pantalla, solo la propia sesión (cuando haya captura) |
| Permisos de Ana | Lo que Ana concedió a un programa no vale para el mismo programa de Carla | Lo concedido es por programa y usuario (paso 2) |
| Lanzar como Ana | No puede | `LANZAR` "como otro" solo se lo acepta a `guardian` |

Los programas del sistema (usuarios por debajo de 1000) se ven desde todas las sesiones, como hoy. `guardian` ve
todos los procesos: es quien vigila las sesiones.

Un administrador sentado en la sesión de Carla puede matar un programa de Ana con la acción de administrador
"matar", con su contraseña, como hoy. No puede leer el volumen de Ana: no tiene su contraseña ni su código de
recuperación.

## El compositor y el sonido (paso 2, hecho)

1. `inicio` da a `sesiones`, sin marca, una copia sin marca del canal del compositor y del de sonido (orden
   `CANAL_SISTEMA`). Solo `inicio` y `sesiones` tienen una así: es el canal que solo tiene `sesiones` para decir
   quién está detrás. Un mensaje con la marca de un cliente nunca cambia nada.
2. Por ella, `sesiones` les dice de quién es cada programa de una persona y cada uno que lanza (`AVISO_PERSONA`:
   marca, usuario y nombre), antes de lanzarlo; y, al cambiar quién está delante, qué personas tienen su sesión
   detrás (`AVISO_DETRAS`), antes de contestar a `guardian` (así llega al compositor antes que el desbloqueo, por el
   mismo canal).
3. Lo que lanza `sesiones` nace con su llave del compositor (la tercera, marcada con su número). Su nombre sirve solo
   para los mensajes: los poderes (escritorio, tema, panel, sistema) siguen siendo por el nombre que da `inicio`. Un
   `Programas/panel` lanzado del disco no es el panel (prueba hostil en virt).
4. El compositor no pinta lo de una sesión de detrás, no le da teclado ni ratón (el puntero no la encuentra) y no deja
   que se lleve el teclado; si lo tenía, pasa a la ventana de arriba que se ve. Sus avisos de `frame` salen a ritmo
   bajo, uno por segundo como mucho: no se quedan esperando ni se les acumulan.
5. El sonido mezcla los flujos de una sesión de detrás con volumen 0, con su rampa: se siguen leyendo a su ritmo. Al
   volver delante, cada uno con el volumen que pidió.
6. Entre una sesión y otra, la pantalla de bloqueo tapa todo.
7. Falta: lo copiado (portapapeles, cuando exista) y los avisos, por sesión.

Quién es "de una sesión" sale del usuario de su ficha: un programa de una persona con la sesión abierta y detrás.
Los que lanza `inicio` con el usuario de una persona (`centro`, `selector`) siguen esa regla: con Carla delante y
Ana detrás no se ven. Si la persona no tiene la sesión abierta, se ven (son de las pruebas; lo de una persona lo
lanzará `sesiones`).

## Permisos por programa y usuario (paso 2, hecho)

Lo concedido se guarda con el usuario: una línea "programa servicio usuario" en `Estado:Servicios/sesiones/permisos`.
Una línea vieja, sin usuario, no vale (se pregunta otra vez). Si varias copias de un programa esperan la misma
pregunta, lo que diga el usuario se guarda para el usuario de quien preguntó primero; las demás reciben la llave esa
vez. El centro de control ve y quita lo de su usuario y lo del sistema; `guardian`, todo.

## Garantías

Un programa de Carla no puede, con fallos solo en programas:

- ver ni matar procesos de Ana;
- leer ni escribir sus ficheros, ni abrir, ver o cerrar su volumen;
- entrar, volver o salir como Ana, ni lanzar nada como ella;
- ver sus ventanas ni recibir su teclado, ni oír su sonido;
- usar un permiso que Ana concedió a un programa;
- hacer que a Carla se le pregunte algo en nombre de un programa de Ana.

Lo que no cubre:

- Los servicios compartidos (sonido, red) atienden a las dos sesiones. Un fallo en uno ve lo de las dos (S6b).
- Canales laterales: cuánto tarda la CPU, cuánta memoria queda. Sin medir.
- Los ficheros compartidos del disco (`SYS:`, `Work:`) siguen sus permisos (capa 1): lo que Ana deje legible para
  todos, Carla lo lee.
- Lo concedido se guarda hoy en el cajón de `sesiones`, con el usuario. Su sitio es el de S6b:
  `Home:Prefs/Permisos/`, en texto que el dueño lee, con la orden `Permiso`. Va con `Prefs` de usuarios.
- Los programas que lanza `inicio` con el usuario de una persona (en las pruebas: `centro`, `selector`, `ficheros`
  como 1000) son "de Ana" sin estar en su sesión. Es provisional: lo de una persona lo lanzará `sesiones`.

## Pasos

| Paso | Qué | Prueba |
|---|---|---|
| 1 | `sesiones`: sesión de delante; procesos y matar por usuario; preguntas, acciones y bloquear solo desde la de delante; entrar en una sesión abierta es volver; orden `ACTIVA`. `guardian`: Esc en el bloqueo para cambiar de usuario | En virt: con Ana detrás, un programa de Carla intenta listar, matar, abrir ficheros y volumen de Ana, y entrar o lanzar como ella; todo falla. El de Ana sigue en marcha detrás, no puede preguntar ni bloquear, y al volver Ana su fichero está |
| 2 | El compositor por sesiones; `sesiones` da la llave del compositor a lo que lanza; sonido de detrás en silencio; permisos por programa y usuario | Hecho. En la escena: una ventana de detrás no se pinta ni recibe teclas ni clics, y un cliente no cambia la sesión de delante. En virt: con Carla delante, la ventana de Ana no está en la captura ni recibe teclado ni ratón; su sonido suena en silencio; el permiso que Ana concede a `cambio` no vale para el de Carla |
| 3 | Las preguntas de detrás esperan en vez de negarse; lista de cuentas en el bloqueo; acción de administrador "cerrar la sesión de otro"; portapapeles y avisos por sesión | — |
| 4 | En la placa | — |

## Decisiones para confirmar

Del paso 2:

- **Los avisos de `frame` de detrás, uno por segundo** (no ninguno): así un programa que dibuja al ritmo de la pantalla
  no se queda parado ni se le acumulan.
- **Los programas de `inicio` con el usuario de una persona** se ocultan solo si esa persona tiene la sesión abierta
  y detrás. Sin sesión abierta, se ven (provisional, de las pruebas).
- **Una pregunta para varias copias del mismo programa** de usuarios distintos: lo que diga el usuario se guarda solo
  para el primero que preguntó; los demás reciben la llave esa vez.

Del paso 1:

1. **Los programas de detrás siguen en marcha** (no se congelan). Congelar ahorra batería y CPU pero corta
   descargas; además el kernel no tiene aún cómo parar un grupo entero.
2. **Su sonido, en silencio** mientras no está delante (hecho en el paso 2: se sigue leyendo, sin sonar).
3. **Sus preguntas, negadas** en el paso 1; en el paso 3, en espera hasta que vuelva.
4. **Esc en la pantalla de bloqueo** para cambiar de usuario, escribiendo el nombre. ¿Se enseña luego una lista de
   cuentas? Enseñarla dice quién usa el equipo; escribir el nombre no.
5. **Matar un proceso de otra persona da "no existe"**, no "no le toca": no se sabe ni que existe.
6. **Hasta 8 sesiones abiertas** a la vez.
7. **Cerrar la sesión de otro** (pierde lo no guardado) solo con una acción de administrador. Apagar con otras
   sesiones abiertas: aviso y contraseña de administrador.
