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
| Sonido | En silencio (paso 3) | Que la música de Ana no suene encima de lo de Carla |
| Volver | Con su contraseña | Como desbloquear. `sesiones` la comprueba con su volumen, sin reabrirlo |

`sesiones` sabe qué sesión está **delante**: la del último que entró o volvió con su contraseña. Es la única pieza
que lo decide. El compositor y `guardian` siguen lo que diga.

Una persona tiene una sesión como mucho. La sesión se nombra por su usuario: no hace falta otro número mientras sea
así. Caben 8 abiertas a la vez (lo que ya guarda `sesiones`).

## Elegir usuario

En la pantalla de bloqueo, **Esc** pasa a "Cambiar de usuario": se escribe el nombre y luego la contraseña, como en
la pantalla de entrada. Con Esc otra vez se vuelve al bloqueo de antes. Pie de la pantalla: "Esc: cambiar de
usuario".

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
| Ventanas de Ana | No las ve ni le llega su teclado | El compositor, por sesiones (paso 2). Capturar la pantalla, solo la propia sesión |
| Lanzar como Ana | No puede | `LANZAR` "como otro" solo se lo acepta a `guardian` |

Los programas del sistema (usuarios por debajo de 1000) se ven desde todas las sesiones, como hoy. `guardian` ve
todos los procesos: es quien vigila las sesiones.

Un administrador sentado en la sesión de Carla puede matar un programa de Ana con la acción de administrador
"matar", con su contraseña, como hoy. No puede leer el volumen de Ana: no tiene su contraseña ni su código de
recuperación.

## El compositor (paso 2)

Hoy un programa lanzado por `sesiones` no tiene llave del compositor: no abre ventanas. Antes de dársela:

1. `sesiones` manda al compositor, sin marca, el alta de cada cliente que lanza con su sesión (su usuario), igual
   que `inicio` le dice hoy el nombre de cada marca.
2. Al cambiar quién está delante, `sesiones` se lo dice al compositor (sin marca).
3. El compositor compone y manda entrada solo a los clientes de la sesión de delante y a los del sistema. Las
   ventanas de detrás no se pintan ni reciben eventos de puntero o teclado; siguen recibiendo los suyos de
   configuración y los cuadros, con el ritmo bajado.
4. Lo copiado (portapapeles, cuando exista) y los avisos, por sesión.
5. Entre una sesión y otra, la pantalla de bloqueo tapa todo: no se ve un cuadro de una sesión con el teclado de
   otra.

## Garantías

Un programa de Carla no puede, con fallos solo en programas:

- ver ni matar procesos de Ana;
- leer ni escribir sus ficheros, ni abrir, ver o cerrar su volumen;
- entrar, volver o salir como Ana, ni lanzar nada como ella;
- ver sus ventanas ni recibir su teclado (cuando haya paso 2);
- hacer que a Carla se le pregunte algo en nombre de un programa de Ana.

Lo que no cubre:

- Los servicios compartidos (sonido, red) atienden a las dos sesiones. Un fallo en uno ve lo de las dos (S6b).
- Canales laterales: cuánto tarda la CPU, cuánta memoria queda. Sin medir.
- Los ficheros compartidos del disco (`SYS:`, `Work:`) siguen sus permisos (capa 1): lo que Ana deje legible para
  todos, Carla lo lee.
- Los permisos concedidos son hoy por programa, no por programa y usuario (S6b los quiere en el `Home:` de cada
  uno). Lo que Ana conceda a un programa vale también para el mismo programa de Carla. Va con `Prefs` de usuarios.
- Los programas que lanza `inicio` con el usuario de una persona (en las pruebas: `centro`, `selector`, `ficheros`
  como 1000) son "de Ana" sin estar en su sesión. Es provisional: lo de una persona lo lanzará `sesiones`.

## Pasos

| Paso | Qué | Prueba |
|---|---|---|
| 1 | `sesiones`: sesión de delante; procesos y matar por usuario; preguntas, acciones y bloquear solo desde la de delante; entrar en una sesión abierta es volver; orden `ACTIVA`. `guardian`: Esc en el bloqueo para cambiar de usuario | En virt: con Ana detrás, un programa de Carla intenta listar, matar, abrir ficheros y volumen de Ana, y entrar o lanzar como ella; todo falla. El de Ana sigue en marcha detrás, no puede preguntar ni bloquear, y al volver Ana su fichero está |
| 2 | El compositor por sesiones; `sesiones` da la llave del compositor a lo que lanza | Una ventana de Ana no se ve ni recibe teclas con Carla delante |
| 3 | Sonido de detrás en silencio; las preguntas de detrás esperan en vez de negarse; lista de cuentas en el bloqueo; acción de administrador "cerrar la sesión de otro" | — |
| 4 | En la placa | — |

## Decisiones para confirmar

1. **Los programas de detrás siguen en marcha** (no se congelan). Congelar ahorra batería y CPU pero corta
   descargas; además el kernel no tiene aún cómo parar un grupo entero.
2. **Su sonido, en silencio** mientras no está delante.
3. **Sus preguntas, negadas** en el paso 1; en el paso 3, en espera hasta que vuelva.
4. **Esc en la pantalla de bloqueo** para cambiar de usuario, escribiendo el nombre. ¿Se enseña luego una lista de
   cuentas? Enseñarla dice quién usa el equipo; escribir el nombre no.
5. **Matar un proceso de otra persona da "no existe"**, no "no le toca": no se sabe ni que existe.
6. **Hasta 8 sesiones abiertas** a la vez.
7. **Cerrar la sesión de otro** (pierde lo no guardado) solo con una acción de administrador. Apagar con otras
   sesiones abiertas: aviso y contraseña de administrador.
