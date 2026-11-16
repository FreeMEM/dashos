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
- **Sin Ctrl ni Alt no hay atajo** ("no vale"): una letra, un número o Mayúsculas con algo es escribir. Esto deja
  fuera las teclas de función solas (F1..F12), que FKey del Amiga usaba: pendiente del usuario.

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

### `commodities.library` en la caja Amiga

Paso 4: ver al final de esta bitácora.
