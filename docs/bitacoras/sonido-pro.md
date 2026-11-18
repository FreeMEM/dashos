# Bitácora de `sonido-pro`: un sistema de sonido espectacular

Rama `sonido-pro`, desde `revision` (ba0c2a2e). Petición del usuario: "También quisiera un sistema de sonido
espectacular". Diseños aceptados: `docs/diseño/S19-sonido.md` y `S19b-sonido-de-estudio.md`.

## El diseño en tres frases

El servicio `sonido` deja de ser un mezclador y corre un **grafo** (S19b, E1): los flujos de los programas son nodos
que van a la mezcla de una salida, y de ella por ecualizador, espacial y limitador a la salida; el grafo corre en
cuantos, en orden topológico, con rampas en cada conexión y cada ganancia (nada chasca al cambiar). Quien enseña el
sonido (la ventana "Sonido", el panel, la barra, el dock) se suscribe a un **estado** en memoria compartida que el
servicio rellena unas 30 veces por segundo (niveles, espectro solo si alguien lo mira, lo que suena) y le manda órdenes
que solo acepta de los programas del sistema por su nombre. Lo que suena lo dicen los propios programas (`MEDIOS`) y las
órdenes del mezclador les llegan por el servicio, por el mismo canal por el que piden muestras.

## Lo hecho

### Paso 1: el motor (S19b, E1)

- `sonido/src/grafo.rs`: nodos (flujo, mezcla, efecto, salida), conexiones con rampa de un cuanto (mover un flujo de
  salida es un fundido cruzado), ciclos rechazados, orden de Kahn recalculado solo cuando cambia el grafo, un cuanto
  para todo el grafo (16 a 4096; el período del modo) y un período por salida (cada salida junta cuantos en su cola, que
  no pasa de un segundo: lo de más se tira y se cuenta). Volumen, silencio y fundido de salida por nodo, en rampa.
  Medidor por nodo (pico y RMS de lo que sale) para los vúmetros. Tope de 256 nodos y 1024 conexiones.
- `usuario/src/bin/sonido.rs`: el servicio sobre el grafo. Cada salida de hardware tiene su cadena; hasta 8 flujos
  por programa (las cajas abren varios) y 64 en total; mide la latencia de la salida (de escribir una ranura a que el
  aparato la devuelve) y lo que cuesta correr el grafo por período.
- Los flujos de las cajas son nodos como los demás (el estado los marca por su dueño, con su icono). **Que las cajas
  suenen de verdad está pendiente** (ver abajo).

### Paso 2: el mezclador visible

- `usuario/src/bin/mezclador.rs`, la ventana "Sonido": programas (volumen, silencio, salida por programa con un clic,
  vúmetro estéreo de segmentos con pico que espera y cae), salidas (volumen, elegir la del sistema, vúmetro), entradas,
  el visualizador, ecualizador de 10 bandas con 8 preajustes, limitador, espacial, latencia (Normal, Para tocar,
  Ahorro) y lo que suena con sus botones. Vive sin ventana hasta que se pide (orden `VENTANA`); abierta pinta a ~30
  cuadros por segundo, con lo fijo en un fondo que solo se repinta cuando cambia.
- El panel (centro de control): el detalle de "Salida de sonido" dice por dónde sale, los programas que suenan (un clic
  los silencia) y abre la ventana "Sonido". Si otro cambia el volumen de la salida, el panel lo toma (la barra y el OSD
  dicen lo mismo).
- El OSD del volumen (dock) dice por qué salida suena ("Altavoces").

### Paso 3: efectos

- `sonido/src/efectos.rs`: ecualizador paramétrico (bandas de pico y estantes; fórmulas del "Audio EQ Cookbook" de RBJ,
  documento público) y gráfico con preajustes; limitador con mirada adelante de 1,5 ms (la salida nunca pasa de -1 dB);
  espacial (ensanchar y crossfeed para auriculares); ganancia. Coma fija; los coeficientes se calculan en coma flotante
  (`sonido/src/mates.rs`, seno, exponencial y raíz propios) solo al cambiar un parámetro, en tramos de 32 cuadros.
  Parámetros absurdos se recortan.

### Paso 4: el visualizador

- `sonido/src/espectro.rs`: FFT de 512 en coma fija, 24 bandas logarítmicas de 50 Hz a 12 kHz, con sensibilidad
  automática y caída lenta. Se calcula solo mientras alguien mira (el latido del estado); en la ventana "Sonido" y en
  el widget "Música" de la barra (seis barras como el de DMS).

### Paso 5: sonidos del sistema

- `sonido/src/sonidos.rs`: sintetizados aquí (sin ficheros: MPL-2.0, hechos en este repositorio). Tres temas (Cristal,
  Suave, Retro) y siete eventos (aviso, bloqueo, error, conectar, desconectar, volumen, entrar). Respetan el "no
  molestar" (lo dice el panel) y se pueden silenciar.

### Paso 6: controles de medios

- Protocolo `MEDIOS` / `MEDIOS_ORDEN` (`docs/referencia/servicios.md`). `melodia` (reproductor de prueba, música
  sintetizada propia) lo usa. Widget "Música" en la barra (como `Media.qml` de DMS: visualizador, "título • artista" y
  tres botones).

## Pruebas

- PC: `sonido` (31 pruebas: grafo, efectos, espectro, sonidos, cuentas), `usuario/src/protocolo/sonido.rs`,
  `usuario/src/sonido_estado.rs`, `usuario/src/sonido_medidor.rs` y `usuario/src/osd_modelo.rs` con `rustc --test`.
- Hostiles de PC: mil flujos (para en 256 nodos), muestras basura (NaN, infinitos, 1e30), parámetros absurdos en el
  ecualizador (estable con todo al tope), el limitador con golpes de 6 veces el fondo de escala (nunca pasa de -1 dB),
  cuentas absurdas en el estado (no se sale de la tabla), ráfagas de dt en los vúmetros.
- Hostiles en QEMU (`hostil`, en `arranque-virt`): tocar el volumen, el silencio y la salida de otro, efectos con
  valores absurdos, ver el estado, mandar órdenes a otro programa, sonidos del sistema, abrir la ventana, lo que suena
  con largos imposibles y con texto que no es UTF-8, y mil flujos (para en 8 por programa; el servicio sigue).
- El WAV de `arranque-virt` (`tools/sonido_wav.py`): los dos tonos, sin huecos ni chasquidos, y ahora también los
  sonidos del sistema que suenan después (avisos, bloqueo): empiezan en silencio, sin saltos y sin pasar de -1 dB.
- `make arranque-virt-sonido` (nuevo): la imagen de usar con `melodia`; el guion abre el detalle de la salida, la
  ventana "Sonido", pone "Graves", pausa y sigue, el OSD del volumen, Ajustes > Sonido y la pantalla de bloqueo.

## Medidas

(se completa abajo)

## Pendiente del usuario

## Lo que queda
