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

Resultados de la última tanda (2026-11-18, con la máquina compartida con las pruebas de otras ramas): `make prueba`
bien; `arranque-virt` bien a la tercera (la primera faltó "[dash-ventana] pulsado"; la segunda, la lista de cuentas
de `cuentas-2` sin Berta porque la comprobación corrió antes de que el Shell la creara: los dos, fallos sueltos ya vistos
en otras ramas, `~/recuperar/geosian-7d/salida_av_tanda_panel.txt`); `arranque-virt-2`, `arranque-virt-linux`,
`arranque-virt-dms`, `arranque-virt-shell` y `sonido-carga`, bien a la primera; `arranque-virt-sonido`, bien (con las
siete capturas).

Fallos que encontraron las pruebas en marcha (y su arreglo): la barra caía al cerrar el Dash (el estado del sonido
estaba en la misma dirección que el Dash: ahora todos los que lo miran lo ponen en 0x68_0000_0000); `guardian` y el
centro no eran nadie para el servicio con la llave que les da `sesiones` (ahora `sesiones` le dice al sonido el nombre
de un programa del sistema que la pide); la barra no se enteraba de una pausa hasta su siguiente vuelta (ahora el
servicio la despierta cuando cambia lo que suena); el ecualizador atascado (arriba, en Medidas).

## Medidas

Todas en QEMU `virt` (TCG, sin FPU de verdad: coma fija) salvo donde se dice PC.

- **CPU del grafo** (lo que tarda en correr el grafo por período de 256 cuadros, media de los períodos con sonido):
  - El mezclador de antes: 19 µs con los dos tonos (medido en `revision` y en `dock-discos`).
  - El grafo, primera versión: 227 a 247 µs con los dos tonos (la cadena mezcla, ecualizador, espacial y limitador
    corre aunque esté plana). Lo que costaba: la preganancia del ecualizador calculaba una exponencial en coma
    flotante por software en cada cuanto; el limitador hacía tres colas por cuadro aunque no actuara; el espacial
    apagado movía su línea de retraso; y los vúmetros se medían también en los efectos.
  - Tras arreglarlo (ganancia calculada al cambiar, limitador con camino rápido, espacial apagado sin trabajo, grafo en
    reposo sin procesar efectos callados, vúmetros solo de lo que se enseña): 116 µs con los dos tonos; 70 µs con la
    melodía y el ecualizador en "Graves". El peor período, 3,6 ms (un tic que recupera varios períodos de golpe).
  - PC (`cargo test --release -- --ignored --nocapture coste`): 1,4 µs por cuanto en reposo, 5,1 µs con un flujo y todo
    plano, 9,1 µs con "Graves".
- **Latencia de la salida** (de escribir una ranura a que el aparato la devuelve, medida por el servicio): media 20,5 a
  21,2 ms, la peor 30,6 a 31,8 ms, con 4 períodos de 256 por delante (lo esperado: 4 × 5,33 = 21,3 ms). "Para tocar"
  (2 por delante, cuanto de 64) no está medido: el aparato sigue con períodos de 256 (ver "Lo que queda").
- **Cortes**: el servicio pedía 3 ms de plazo cada 5,3 ms; con el repartidor del núcleo ya en la clase de plazo, el
  kernel lo negaba ("sin clase de plazo": también en `dock-discos` y sin el grafo) y la salida llevaba 111 vacíos al
  final de `arranque-virt` en esa rama. Con el grafo y en la justa: 47; pidiendo 2 ms (el servicio con plazo, el
  controlador sin él): 17. Con 1,5 ms (los dos con plazo): 15, casi todos al arrancar (9 en el primer recuento), con
  la máquina cargada por las pruebas de otras dos ramas a la vez (QEMU de `dock-discos` y `fondos`).
- **Cortes con carga** (`make sonido-carga`: cuatro programas girando en los cuatro núcleos mientras suenan los tonos,
  la medida de A6): 0 faltas, 0 vacíos, el WAV sin huecos ni chasquidos; el grafo, 106 a 122 µs de media por período
  (el peor 10 ms, un tic que recupera tras una parada), y la latencia, 20,9 ms de media y 30,4 la peor.
- Con la máquina cargada, la media del grafo en `arranque-virt` sube (341 µs en una vuelta, con un peor de 16 ms): son
  µs de reloj en un QEMU que comparte la CPU, no de un núcleo de verdad.
- **El compositor con la ventana "Sonido" abierta**: pintándola entera 30 veces por segundo se llevaba casi todo su
  tiempo (9,9 s de cada 10 componiendo, 170 Mpx); diciéndole solo las zonas que se mueven (vúmetros, espectro, asas) y
  a 20 cuadros por segundo, de 3,6 a 5,5 s de cada 10 (37 a 58 Mpx), con el widget de la barra animado a la vez.
- **El ecualizador en el WAV**: con "Graves" y la música en pausa, el WAV se quedaba en 355 (de 32.768) constante: el
  truncado en el filtro de 31 Hz dejaba la salida atascada. Arreglado con realimentación del error (prueba nueva:
  ningún preajuste deja más de 1 tras el sonido). `tools/sonido_partir.py` saca 4 s antes y después de "Graves"
  (`~/recuperar/geosian-7d/sonido-pro-capturas/ecualizador-antes.wav` y `-despues.wav`): la energía por debajo de
  150 Hz pasa del 42,6 % al 44,3 % (la melodía ya es de graves; el estante de "Graves" sube 6 dB a 31 Hz).

## Capturas

`~/recuperar/geosian-7d/sonido-pro-capturas/` (fuera de git), a 1920 × 1080 con el perfil de shiva (tema oscuro, barra
de shiva, fondo Fuji): `1-panel-salida` (el detalle de la salida en el centro de control), `2-mezclador` (la ventana
"Sonido" con la melodía), `3-mezclador-graves`, `4-mezclador-pausa`, `5-osd-volumen` (dice "Altavoces"),
`6-ajustes-sonido` y `7-bloqueo-musica` (lo que suena en la pantalla de bloqueo). En todas, la barra con el widget de
música y su visualizador.

## Pendiente del usuario

Decidido por defecto (cambiable):

1. **Quién manda en el sonido**: el panel, la ventana "Sonido" y Ajustes (por el nombre que da `inicio`) cambian el
   volumen, el silencio y la salida de los programas, los efectos y la latencia; un programa solo su volumen. Ven el
   estado (qué suena y sus niveles) esos y la barra, el dock y la pantalla de bloqueo.
2. **"No molestar"** calla todos los sonidos del sistema salvo el tic del volumen.
3. **El tic al cambiar el volumen** (DMS lo trae puesto): no se manda (el panel no lo pide). Si se quiere, es una línea.
4. **Tema de serie "Cristal"**, sonidos al 70 %. La primera salida al arrancar no suena "conectar".
5. **La ventana "Sonido"** vive siempre sin ventana y se abre desde el detalle de la salida del centro de control y desde
   Ajustes > Sonido (no hay entrada en `Programas/`).
6. **El widget "Música" de la barra** solo se ve si algún programa dice lo que suena (como DMS); en la barra de serie
   ya estaba (`musica`, antes "falta").
7. **Lo que cambia la ventana "Sonido"** (ecualizador, efectos) vale hasta reiniciar; lo que se elige en Ajustes >
   Sonido se guarda (`ENV:Sonido`) y se pone al arrancar.

## Lo que queda

- **Que las cajas suenen.** El grafo y el estado ya las tratan como nodos con su icono, pero ninguna abre aún un flujo:
  la caja Amiga necesita `audio.device` dentro de la caja (el crate `audiodevice` ya existe; falta enchufarlo como
  `timer.device`, con la llave del sonido pedida a `sesiones`, sin tocar `caja/caja.c`); la caja Linux necesita un
  virtio-sound en el VMM y un núcleo invitado con `CONFIG_SND_VIRTIO` (los dos de `~/.cache/dashos-caja-linux` no lo
  traen).
- **Períodos de verdad de 64 cuadros**: "Para tocar" baja el cuanto del grafo y lo que se mezcla por delante, pero el
  controlador de virtio-sound sigue pidiendo al aparato períodos de 256 (habría que reconfigurarlo en marcha).
- **Entradas** (micrófono, S19 fase 6): la ventana las enseña vacías.
- **Varias salidas**: el grafo y el protocolo las llevan (pruebas de PC), pero en QEMU hay un solo virtio-sound.
- **Del grafo** (S19b): nodos en paralelo en varios núcleos, cambios de parámetro con su muestra exacta, puertos MIDI.
- La cantidad del espacial no tiene deslizador (va al 50 %); el visualizador de la pantalla de bloqueo no se mueve; el
  OSD vertical no dice la salida.

