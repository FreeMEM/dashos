# Bitácora: avisos (notificaciones, S10)

Rama `avisos`, sacada de `revision`. Diseño en `docs/diseño/S10-escritorio.md` ("Lanzador, notificaciones y
Commodities") y `docs/diseño/U6-cambio-rapido.md`.

## 2026-11-12, primera tanda: cortada al empezar

Se paró por el cupo semanal antes de escribir código. Quedó el diseño (abajo, ya hecho).

## 2026-11-12, segunda tanda: primera versión

Antes, `git merge revision` (la red y los arreglos de `sesiones`), y a mano los dos últimos de `revision` (cb7941b,
el paso de Ajustes espera al selector; 5952ac7, la línea del lanzador sin número): se juntarán al fusionar.

### El servicio `avisos`

- Proceso propio, `usuario/src/bin/avisos.rs`. Lo lanza `inicio` (solo en virt), antes de la barra, con su canal
  "avisos" (llave 1).
- Protocolo e historial en `usuario/src/protocolo/avisos.rs`, sin `alloc` ni nada de DashOS. Para los programas,
  `dashos::avisos_cliente::avisar`.
- `AVISAR`: urgencia (baja, normal, urgente), título (hasta 48 bytes) y texto (hasta 160), en un mensaje. Contesta
  estado, número y el nombre que puso el servicio.
- El aviso no lleva "de quién": lo pone el servicio por la marca. El nombre le llega solo sin marca: la entrada de
  directorio de `inicio` (24 bytes) y `AVISO_PERSONA` de `sesiones`. Lo que mande un programa con esa forma, con su marca,
  no cambia nada. Un programa sin nombre conocido no avisa (-2).
- Textos: lo que no es UTF-8 y las letras de control pasan a "?" (un salto de línea, a espacio). Largos que no cuadran
  con el mensaje, o mayores que el tope: -1, y el servicio sigue.
- Historial de 50, como mucho 8 de un mismo programa: uno que inunda solo se pisa a sí mismo. Ritmo: 10 avisos por
  minuto por programa; los demás, -3 (y lo dice una vez). Todo de tamaño fijo: inundar no le gasta memoria.
- Solo la barra (el nombre `barra` que da `inicio`) se suscribe, lee, marca como leídos y borra. A otro, -2.
- El timbre: la barra le da al suscribirse una copia marcada del extremo por el que le escribe el compositor (un
  proceso solo espera en un canal). Un solo timbre pendiente hasta que la barra pida algo: no llena esa cola.

### Sesiones (U6)

- `avisos` entra en `SIGUEN_SESIONES` de `inicio` y de `sesiones`, como el compositor y el sonido: `sesiones` le dice
  de quién es cada programa y qué sesiones están detrás.
- Un aviso de una persona con la sesión detrás se guarda sin tocar el timbre y no sale en el historial de la de
  delante. Al volver ella, sale en su historial, sin leer y marcado como retenido: la barra no lo enseña como toast.
- `sesiones` reparte la llave de `avisos` como la del sonido (`REPARTE`, sin entrada de directorio). Permisos fijos:
  `cambio` y `avisa` (en `PERMISOS`, por nombre de fichero, como el de `cambio` para el sonido).

### La barra

- `usuario/src/ventana_cliente.rs`: `Cliente::conectar_con_puerta(marca)`. Lo que llega con esa marca se cuenta en
  `timbres` y no se mezcla con el compositor; `evento_plazo` vuelve al llegar.
- La campana, entre la hora y el botón del panel: tonal, llena con el historial abierto, y con la insignia pequeña de
  Material 3 (punto del color de error) si hay avisos sin leer.
- Toast: capa arriba a la derecha, a 8 bajo la barra, sin teclado. Tarjeta de 344 × 80 en el contenedor alto,
  esquinas de 16 y sombra de elevación 3. Círculo con la inicial (urgente: color de error), "de · hora", título y texto. 5 s, o un
  clic lo cierra. Al abrir el historial, se cierra.
- Historial: capa como el panel rápido, esquinas de 28, "Avisos" y el botón de texto "Borrar todo". Hasta 5 filas;
  con más, la rueda mueve la lista y el pie dice cuáles se ven ("1–5 de 10"). Abrirlo los marca como leídos; los que no
  se habían leído llevan fondo alto y un punto del primario mientras está abierto. Tras "Borrar todo", la capa encoge
  a "No hay avisos".
- Lo lanzado del disco sale como "nombre (programa)"; lo del sistema, con el nombre de `inicio`.
- `kernel/src/inicio.rs`: el servicio, el canal "avisos" de la barra (su llave 4) y el programa de prueba `avisador`.

### Pruebas

- PC: `rustc --edition 2024 --test usuario/src/protocolo/avisos.rs` (7), en `make prueba`.
- Virt (`tools/prueba_uefi.sh`, tras la barra):
  - `avisador` (de `inicio`, con la llave): un aviso bueno; entrada de directorio y `AVISO_PERSONA` falsos y un aviso
    que dice ser de guardian, y sale como `avisador`; seis mensajes mal hechos rechazados; bytes raros guardados
    limpios; 100 avisos seguidos topan con el ritmo y el servicio sigue.
  - Un programa sin la llave no puede ni escribir en el canal: no hay a quién mandar nada. Lo que se prueba es que
    `sesiones` no la da sin permiso (como `eco`).
  - `cambio` de Ana avisa con su sesión detrás: se guarda; al volver Ana, sale en su historial.
  - El guion lanza `Programas/avisa` desde el lanzador (como Ana): sale el toast con punto en la campana, un clic lo
    cierra, la campana abre el historial (avisa, cambio y avisador: la inundación no tapa a los demás), "Borrar todo"
    lo vacía y la campana lo cierra.
- Capturas en `capturas-agente/` (fuera de git), con `CAPTURA_AVISOS=fichero.ppm`: `avisos.png` (el toast, con el punto
  en la campana), `avisos.hist.png` (el historial: avisa, cambio y cinco de los ocho que quedan del avisador) y
  `avisos.vacio.png` (tras "Borrar todo").

### Cambios en el guion que no son de los avisos

- Con dos programas más en el arranque, Ajustes y el selector salían más juntos: el clic del paso de Ajustes caía
  justo después de salir el selector, le quitaba el teclado y el paso del selector esperaba sin fin (sin dejar pasar a
  los de detrás: el menú, Super+L). Es el fallo (3) de `docs/REVISION_LOOP.md` con el otro orden. Ahora el paso del
  selector pulsa antes en su cabecera, a la derecha, donde Ajustes no llega.
- Al acabar el paso de los avisos, el cursor va abajo a la derecha: los clics de siempre, desde la campana, volvían a
  abrir el historial y el panel, y la captura final salía con ellos.
- `ESTADO_GUION=fichero`: el estado del guion cada 2 s. Con él se vio en qué paso se quedaba.

### Resultado

- `make prueba`: BIEN (una vuelta, antes de la última, falló en C14 y S2, que miden tiempos, con el PC cargado).
- `make arranque-virt`: tres vueltas fallaron por el selector (arriba), una con el historial abierto en la captura
  final; la séptima, todo BIEN (arranque, disco, captura, sonido y la ventana de Ana). No se repitió más veces.

### Decisiones provisionales (a confirmar por el usuario)

- Los avisos de una sesión de detrás salen al volver solo en el historial, sin toast.
- Lo lanzado del disco sale con su nombre de fichero y "(programa)": no es del sistema.
- Se tocaron las líneas de la tabla de `kernel/src/inicio.rs`: el servicio, el canal de la barra y `avisador`.

### Pendiente

- "No molestar" en el panel rápido (mosaico): no se hizo.
- Los permisos fijos por nombre de fichero (`avisa`, `cambio`) son de prueba; con paquetes (S8), del paquete.
- Avisos con acciones (botones), iconos de los programas, agrupar por programa, y el puente de freedesktop y del Amiga.
- La hora es UTC, como la de la barra.
