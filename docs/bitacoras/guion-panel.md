# Bitácora: el guion de `make arranque-virt` no toca nada mientras `guardian` pregunta (guion-panel)

Rama `guion-panel`, copia de `fusion-fondo` (345d29da: tecla-pegada, centro-vivo, paq-formato y a15-carrera). El
encargo: tras la unión, `make arranque-virt` pasó una vez y la segunda falló en "[tanda] colar otra detrás pide su
confirmación y no se hace: MAL" y dos más de la tanda (`~/recuperar/geosian-7d/salida_av_tanda_panel.txt`, ~2878-2942).

## 2026-11-17

### La causa

`guardian` saca la tanda "Cerrar el proceso 65843" y, justo después, el paso del panel rápido manda Super+A: el panel
sale encima y se lleva el teclado (una capa con teclado exclusivo se lo lleva al salir). Lo que el guion teclea para
contestar a la tanda le llega al panel; la tanda acaba en "el usuario dice que no" y se encadenan los MAL.

El paso del panel de tecla-pegada sí esperaba a las ventanas de `guardian`, pero solo una vez abierto: la condición para
abrirlo (Super+A) no la miraba. Y el paso que contesta a `guardian` tecleaba a ciegas, a 1,3 s de ver la línea, sin
saber quién tenía el teclado. Es la misma familia que los otros: cualquier paso que mandara teclas o clics con una
pregunta de `guardian` en pantalla se los daba a ella (una "s" de "avisa" contestaría que sí) o abría algo encima.

### El arreglo, de raíz en el guion (`tools/prueba_uefi.sh`) y una línea nueva de `guardian`

- **`guardian` dice cuándo su ventana del sistema tiene el teclado** (`usuario/src/bin/guardian.rs`): "[guardian]
  ventana del sistema: tiene el teclado" y "...: pierde el teclado". Siempre detrás de la línea "en pantalla" de su
  ventana (si el teclado llega antes, lo dice justo después), así que en la salida el orden es fiable.
- **Una sola puerta, en `mandar`**: con una pregunta de `guardian` pendiente (pregunta de permisos sin "El usuario
  dice", o una tanda, código o cambio de contraseña sin su cierre) y la sesión sin bloquear, `mandar` no deja pasar
  teclas ni pulsaciones de botón (los movimientos y las capturas, sí) salvo de los pasos que le contestan
  (`contestando`, que se pone en esos pasos y se quita al empezar cada vuelta). Lo retenido se apunta en `GUION_TRAZA`
  ("RETENIDO") y `mandar` devuelve si mandó algo.
- **Contestar espera al teclado**: los cinco pasos que contestan (preguntas de permisos, tandas, código de
  recuperación, cambio de contraseña y la confirmación de reiniciar) esperan, hasta 10 s, a que la ventana de
  `guardian` diga que tiene el teclado.
- **El panel no se abre** con una pregunta pendiente ni hasta 4 s después, y solo cuenta como abierto si Super+A salió.
  Los pasos de Dash y del panel esperan ahora también a las preguntas de permisos (antes, solo a las ventanas del
  sistema).

### Pruebas

- `make arranque-virt`, tres vueltas seguidas: BIEN las tres (con el disco, la captura, el sonido y el cambio de
  usuario). En cada una, la puerta retuvo de 8 a 10 envíos (el clic y Super+Arriba/Abajo de Ajustes, y alguna "a" de
  fondo, con una pregunta de `guardian` en pantalla; Ajustes repite su paso y salió bien) y `guardian` dijo 40 veces
  "tiene el teclado" y ninguna "pierde el teclado": ninguna ventana se le puso encima.
- Una vuelta más con 16 procesos gastando CPU a la vez (carga de 18 en 32 hilos): BIEN; 9 envíos retenidos.
- `make prueba`: BIEN.
- No he visto la forma exacta del fallo (el panel encima de la tanda) en estas vueltas: no la he provocado a propósito.

### Pendiente del usuario

- En el sistema, una capa con teclado exclusivo que sale (el panel con Super+A) le quita el teclado a la pregunta de
  `guardian` que hubiera. Para una persona puede tener sentido (lo pidió ella), pero es una ventana de seguridad: lo
  sensato sería que una capa de un cliente que no es del sistema no se lleve el teclado de una de `guardian`. No lo he
  cambiado (es del compositor y cambia lo que se ve); el guion ya no lo provoca.
