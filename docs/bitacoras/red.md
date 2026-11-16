# Bitácora de la red (S13)

Rama `red`, sacada de `revision`. Primera versión de la red con cable en QEMU `virt`.

## 2026-11-12: virtio-net, smoltcp, DHCP y TCP de punta a punta

Lo hecho:

- **Controlador** `usuario/src/bin/virtio_red.rs`, en su proceso. virtio 1.2, PCI moderno (en QEMU, `disable-legacy=on`),
  escrito con la especificación de OASIS. Funciones: VERSION_1 y MAC. Una cola de recibir de 128 búferes de 2 KB y una
  de enviar de 64, en memoria DMA del permiso. Espera su interrupción (INTx); sin ella, mira cada 2 ms. El kernel lo
  busca por fabricante y aparato (0x1af4, 0x1041) y le da registros, configuración, interrupción y permiso DMA, como al
  sonido.
- **Servicio** `usuario/src/bin/red.rs`, en su proceso, con smoltcp 0.14 (0BSD) sin `std`: Ethernet, IPv4, DHCP y TCP.
  Habla con el controlador por una memoria de ranuras (128 para recibir, 64 para enviar) y mensajes por el canal
  "red". Cada trama se copia antes de dársela a la pila.
- **Protocolo** para los programas, `usuario/src/protocolo/red.rs`, y su lado del programa, `usuario/src/red_cliente.rs`:
  conectar, enviar, recibir, esperar y cerrar. Los datos van por dos anillos en una memoria compartida. Explicado en
  `docs/referencia/servicios.md`, sección "Red".
- **Permiso**: la llave del canal "red", que da `inicio`. Más adelante, `sesiones`.
- **Pruebas** en `make arranque-virt` (líneas al final de `kernel/pruebas/arranque_virt.txt`):
  - `red-prueba`: el eco del PC (`tools/red_eco.py`, que QEMU lanza por cada conexión a 10.0.2.100:7 con `guestfwd`)
    con 4 MB al azar que vuelven igual; caudal; ida y vuelta; un puerto cerrado da "rechazada"; hacerse pasar por el
    controlador, tocar conexiones ajenas y pedir cosas mal formadas se rechaza.
  - `red-gloton`: abre conexiones sin parar. Le dan 8 y después "límite", 50 veces. Mientras las tiene, el eco de
    `red-prueba` va bien. Al cerrarlas, puede abrir otra.
  - `sin-red`: sin la llave, prueba las 1024 llaves posibles. No llega ninguna.
  - Basura del PC: `tools/red_basura.py` habla con QEMU por un socket Unix (`-netdev dgram`) unido a la tarjeta y a la
    red de usuario por un concentrador (`-netdev hubport`). Empieza con el primer TCP que sale de DashOS y, durante
    20 s, cada 0,2 s manda tramas cortas, largas, enormes (64 KB), IPv4 rotas hacia 10.0.2.15 y ARP rotas. El controlador cuenta y tira las cortas y largas; QEMU tira las enormes; la
    pila, las rotas. Después, el eco sigue bien. El Makefile lo lanza antes de QEMU y lo para por su PID al acabar.

Lo medido (QEMU `virt`, 4 núcleos, TCG, con el PC cargado por otros agentes, carga entre 9 y 76):

| Medida | Valor |
|---|---|
| Caudal del eco, 4 MB, anillos de 64 KB | 37 a 63 MB/s con el PC poco cargado; 15 a 29 con carga de 20 a 76. Lo mismo de vuelta |
| Ida y vuelta, 64 bytes, 50 veces | mediana 0,3 a 0,5 ms con el PC poco cargado (hasta 3,1 con mucha carga); mínimo 0,2 a 0,8 ms; máximo 7 a 35 ms |
| DHCP | 10.0.2.15/24, puerta 10.0.2.2, DNS 10.0.2.3, en menos de un segundo tras arrancar el servicio |

El caudal empezó en 2,75 MB/s. Dos cambios lo subieron a 19: no retrasar los ACK (smoltcp los retrasa 10 ms de serie)
y 128 ranuras de recibir en vez de 64 (con 64 se tiraban tramas por falta de sitio).

Decisiones:

- **smoltcp, provisional**, como recomienda S13. Su CUBIC no compila sin `std` (usa `f64::powi` y `mul_add`): va con
  Reno. Para CUBIC hace falta darle una raíz cúbica y una potencia propias.
- **La memoria de cada conexión la crea el servicio**, no el programa. Si la diera el programa, podría darla revocable
  y revocarla: el servicio caería al tocarla. Cuesta cupo del servicio: 32 conexiones de 64 KB por anillo son 4 MB, y
  los búferes de la pila otros 4.
- **Copias entre DMA y ranuras**: el servicio no toca los descriptores, y el controlador no se fía de lo que dice el
  servicio ni el servicio del controlador (índices y largos acotados).
- **`inicio` vigila hasta 64 programas** (antes 32). Con los 5 de la red, `caida` pasaba del puesto 32 y ya no se
  relanzaba.

Las pruebas que no son de la red y fallan a ratos: en 8 vueltas de `make arranque-virt` con la red, 4 pasaron enteras y
4 no, cada vez por una distinta: Ajustes maximizada con Super+Arriba (la pregunta de `guardian` sale justo cuando el
guion pulsa), el doble clic del selector, A15 "sujetar sin soltar" y A7 "préstamos" del hostil. Las de la red pasaron
en todas. Esas vueltas fueron con el PC muy cargado (carga 20 a 76) y casi todas con la basura del PC durante toda
la vuelta; ahora dura 20 s.

Vuelta de referencia, con el PC menos cargado (carga 6 a 14): la base de la rama (3b11fd4, sin la red) pasó 2 de 2, y
la rama con la red, 4 de 4 seguidas desde que la basura dura 20 s. No se puede descartar del todo que la red, al
cargar la CPU al arrancar, mueva el ritmo de esas pruebas con el PC muy cargado; con carga normal no se vio.

Pendiente:

- IPv6 (smoltcp lo tiene; falta la dirección por SLAAC y la familia en `CONECTAR`), DNS, UDP y escuchar (con su permiso
  aparte y el cortafuegos de `Prefs/Red`).
- Que `sesiones` reparta la llave de "red" en vez de `inicio`.
- Si el servicio se cae y `inicio` lo relanza, se queda sin tarjeta: el controlador le mandó `TARJETA` al de antes.
- La memoria de las tramas la da el controlador: uno malo podría darla revocable y tumbar al servicio. Hoy el
  controlador es del sistema; con controladores de terceros, que la cree el servicio.
- CUBIC, y medir en una red de verdad (GENET en la Pi 4).
- Los otros controladores: GENET (Pi 4), RP1 (Pi 5), USB (Pi 3).
- TLS con rustls.

## 2026-11-15: UDP, nombres, escuchar, cortafuegos, IPv6 y la hora (rama `red-dns`)

Rama `red-dns`, sacada de `revision` (41e5b81). Lo que faltaba para que la red se pueda usar. TLS no se ha hecho: su
estudio está en `docs/diseño/S13b-tls-propuesta.md`.

Lo hecho, por el orden del encargo:

1. **UDP** (`UDP`, y `Udp` en `red_cliente`). Un zócalo con la misma memoria de una conexión; en los anillos van los
   datagramas con una cabecera de 24 bytes (a quién o de quién). El tope de la cola de recepción es el anillo: lo que
   llega con él lleno se pierde y se cuenta. Una cabecera imposible deja roto solo ese zócalo. Sin el permiso de
   escuchar, un zócalo solo recibe de aquellos a quienes ha enviado en los últimos 120 s (un cortafuegos con estado):
   lo demás se tira y se cuenta.
2. **Nombres** (`RESOLVER`, y `resolver` en `red_cliente`; `usuario/src/dns.rs`). Un resolvedor dentro del servicio:
   preguntas A y AAAA al servidor de nombres del DHCP, caché que respeta el TTL (un día de tope; un "no existe", 30 s),
   tres intentos (1, 2 y 4 s) y `hosts` (`Prefs/Red/hosts` del disco del sistema, más `localhost`). Contra el
   envenenamiento: cada pregunta sale de un zócalo propio en un puerto al azar, con un identificador al azar que cambia
   en cada intento; solo vale la respuesta que viene de la dirección y el puerto del servidor, con ese identificador y
   esa pregunta; de ella solo se usa lo que habla del nombre preguntado o del final de su cadena de alias. Los nombres
   comprimidos solo apuntan hacia atrás, con tope de saltos. Topes: 8 preguntas en curso por programa, 64 en total, 16 a
   la vez a los servidores, 256 respuestas guardadas.
3. **Escuchar** (`ESCUCHAR` para TCP y `UDP` con un puerto elegido; `Oido` en `red_cliente`). El permiso es la llave
   de otro canal, "red-escuchar", que atiende un hilo aparte del servicio y pasa al bucle principal. **Cortafuegos de
   entrada cerrado de serie** (`usuario/src/cortafuegos.rs`): un intento de conexión o un datagrama a un puerto que
   nadie tiene abierto se tira sin contestar y se cuenta. Abre un puerto quien escucha con el permiso.
4. **La hora y la zona horaria**. El kernel tiene un permiso nuevo, el del reloj, y la llamada 39 con 1 pone la hora
   (`docs/referencia/kernel.md`). Solo lo tiene `hora` (`usuario/src/bin/hora.rs`), un servicio nuevo que pregunta por
   SNTP (`usuario/src/sntp.rs`) al servidor de `Prefs/Red/hora` y pone el reloj. La barra enseña la hora en la zona de
   `Prefs/zona` (`usuario/src/zona.rs`): UTC, 25 zonas de Europa con la regla de la Unión Europea (Europe/Madrid y
   Atlantic/Canary entre ellas) y 13 sin cambio de hora.
5. **IPv6 básico**: dirección local del enlace y, por SLAAC, la global del prefijo que anuncia el encaminador, con su
   ruta. `CONECTAR` con familia 6; UDP y escuchar valen con las dos familias.
6. **TLS**: solo el estudio, con una medida de tamaño (rustls sin `std` con un proveedor en Rust: 664 KB).

Pruebas. En el PC (`make prueba`): `dns.rs` (7), `sntp.rs` (2), `zona.rs` (4) y `cortafuegos.rs` (2), con sus
hostiles: respuestas cortadas por cualquier sitio y con bytes cambiados al azar (200.000 cada una), punteros en bucle,
alias en bucle, registros de otros nombres, tramas rotas. En QEMU (`make arranque-virt`), sin Internet:

- `tools/red_vecino.py`: otro aparato en el concentrador de QEMU (10.0.2.53), que hace las tramas a mano: un servidor
  de nombres de mentira (puerto 10053) que, según el nombre, contesta bien o como quien quiere envenenar; la hora por
  SNTP tras tres respuestas que no valen; un eco por UDP que antes manda lo mismo desde otra dirección; una ráfaga; y
  "llama", que manda datagramas que nadie pidió desde otra dirección y dice si DashOS le contestó algo.
- `tools/red_entra.py`: intenta entrar por un puerto del PC que QEMU reenvía al 7777 de DashOS (`hostfwd`; el puerto
  del PC se busca libre en cada vuelta, para que varias pruebas a la vez no choquen).
- `red-prueba` (con los dos permisos): todo lo de arriba. `red-gloton` (solo conectar): escuchar sin el permiso,
  hacerse pasar por el hilo de escuchar, poner la hora sin el permiso y 2000 preguntas de nombres de golpe.
- `hora`: una hora de 1999 y una llave que no es el permiso no ponen el reloj; después lo pone del vecino.
- `hostil` (A14): antes comprobaba que la llamada 39 con 1 daba "argumento malo"; ahora, que con ninguna de sus llaves
  pone la hora.
- El DNS de QEMU (10.0.2.3) se prueba con `example.org`, pero no cuenta: sin Internet en el PC no contesta, y la línea
  lo dice.

Por qué el vecino no usa los puertos de siempre: el concentrador de QEMU reparte todas las tramas a todos, también a
la red de usuario de QEMU, que reenvía al PC (127.0.0.1) lo que va a cualquier dirección de 10.0.2.x. En el 53 del PC
puede haber un servidor de verdad que conteste antes que el vecino. Por lo mismo la conexión entrante de prueba va por
`hostfwd` y no desde el vecino: la red de usuario de QEMU contestaría con un RST a los segmentos de DashOS hacia el
vecino, que no son suyos. El puerto 123 de la hora no se puede cambiar (lo usa `hora`): si en el PC hubiera un servidor
de hora en 127.0.0.1, llegarían dos respuestas buenas y valdría la primera.

Lo medido (QEMU `virt`, 4 núcleos, TCG, con el PC con carga de 5 a 7; dos vueltas de `make arranque-virt`):

| Medida | Valor |
|---|---|
| Caudal del eco TCP, 4 MB | 37,4 y 33,0 MB/s (la vuelta de referencia de la rama, antes de tocar nada: 29,3) |
| Ida y vuelta TCP, 64 bytes, mediana | 0,50 y 0,37 ms (antes: 0,73) |
| 2000 preguntas de nombres de golpe | mandadas y contestadas (1992 con "límite") en 154 ms |
| Un servidor de nombres mudo | "sin respuesta" a los 7,0 s (1 + 2 + 4) |
| SNTP con el vecino, ida y vuelta | 1,0 y 1,3 ms; el reloj de QEMU iba 34 y 967 ms atrasado |
| Un puerto cerrado por IPv6 | rechazada en 2 ms |
| Ráfaga de 200 datagramas de 1200 bytes sin leer, anillo de 16 KB | llegan 13; el zócalo cuenta 3 y 46 perdidos; la tarjeta, 155 y 0 sin sitio |
| Intentos de entrar cortados antes de escuchar | 16 |
| Tamaño de `red` | 275 KB (con IPv6, UDP y el resolvedor; el de antes no se midió); `hora`, 83 KB |

El caudal y la ida y vuelta de TCP no empeoran con el cortafuegos de entrada en el camino de cada trama (las
diferencias son de la carga del PC).

Decisiones tomadas aquí, **pendientes del usuario**:

1. **`hosts` va antes que el DNS**, como en todas partes, y no detrás ("de respaldo" decía el encargo). Un nombre de
   `hosts` no se pregunta fuera. Si fuera al revés, un servidor de nombres malo podría pisar lo que el administrador
   escribió. Sirve de respaldo igual: funciona sin servidor.
2. **Lo que se configura va en `Prefs/Red` del disco del sistema** (`hosts`, `hora`), y la zona horaria en
   `Prefs/zona`. Son del sistema: solo las cambia quien puede escribir ahí. El servicio de red es cliente del de
   archivos solo para leer eso, al arrancar. Falta el camino de S6b (la acción de administrador "cambiar la hora o la red
   del sistema", que pasa por `sesiones` y `guardian`) y que el servicio vea los cambios sin relanzarlo.
3. **La zona horaria es del sistema, no de cada usuario.** El encargo pedía la del usuario. La barra es del sistema y
   no lee el `Home:` de nadie; el camino natural es el del tema (el centro de control la guarda en lo del usuario y se
   la da al compositor, que la reparte), pero toca el centro, el compositor y el protocolo de la barra, que otros
   agentes están cambiando. Queda por hacer, con la del sistema como valor de serie.
4. **El servidor de hora de serie es `pool.ntp.org`** si `Prefs/Red/hora` no dice otro. Ese proyecto pide a quien
   reparte un sistema que use una zona propia (o un servidor propio). Hay que decidir cuál. Con él, DashOS habla con un
   tercero nada más tener red.
5. **El reloj se pone de golpe**, también hacia atrás, cada hora. No hay ajuste fino del ritmo. Un programa que mida
   con `hora_ns` puede ver un salto (para medir está `ahora_us`, que no se toca).
6. **`hora` es un servicio aparte** del de red, con el permiso del reloj: quien analiza todo lo que llega de la red no
   puede poner la hora.
7. **El permiso de escuchar es la llave de otro canal** ("red-escuchar"), como "red-estado". S6b habla de "derechos por
   cliente" en el alta; con las llaves de hoy (las reparte `inicio`) el canal es lo que hace que tener la llave sea
   tener el permiso. Cuando reparta `sesiones`, puede seguir igual.
8. **El cortafuegos tira sin contestar** (ni RST ni ICMP): quien tantea no sabe si hay alguien. A cambio, quien se
   conecta por error a un puerto cerrado espera a que venza su plazo en vez de enterarse al momento.
9. **Un zócalo UDP sin el permiso de escuchar solo recibe respuestas** (de aquellos a quienes envió en los últimos
   120 s). Es más estricto que en otros sistemas: un programa que espere datagramas de un tercero necesita el permiso.
10. **Un programa puede elegir a qué servidor de nombres preguntar** (y en qué puerto), como `dig @servidor`. No le da
    nada que no tuviera (con el permiso de red ya puede mandar UDP), y sus respuestas se guardan aparte.
11. **Los nombres de `RESOLVER` miden hasta 240 bytes**, no los 253 de DNS: es lo que cabe en un mensaje de canal.
12. **TLS**: lo que se decida con `docs/diseño/S13b-tls-propuesta.md`.

Pendiente (lo que falta):

- Nombres: preguntar por TCP cuando la respuesta viene truncada (hoy, error -12), más de un servidor, servidores por
  IPv6 y los que anuncia el encaminador (RDNSS), dominios de búsqueda, IDN, DNS cifrado, DNSSEC, mDNS. Que el DHCP
  traiga el servidor de hora (opción 42).
- IPv6: comprobar que la dirección no está repetida (DAD), direcciones que no delaten la MAC, DHCPv6, contestar al
  ping. La prueba de IPv6 es la dirección por SLAAC y una conexión a un puerto cerrado del PC; no hay eco por IPv6
  (el `guestfwd` de QEMU es solo IPv4) ni UDP por IPv6 de punta a punta.
- Cortafuegos: reglas configurables (`Prefs/Red`), y lo que sale no se filtra.
- Escuchar: no hay cola de espera por puerto (cada `ESCUCHAR` atiende a uno); la prueba de "puerto ocupado por otro
  programa" con TCP no está (solo hay un programa con el permiso); con UDP sí.
- Zonas horarias: las reglas de fuera de Europa con cambio de hora, la historia y las zonas al estilo POSIX. Elegir la
  zona en el centro de control. El centro de control ("Acerca de") sigue enseñando la hora en UTC.
- La barra lee la zona al arrancar: un cambio pide relanzarla.
- `hora`: varios servidores y descartar al que miente; no ajusta el ritmo.
- Que `sesiones` reparta las llaves de "red" y "red-escuchar" en vez de `inicio`.
- Lo de antes que sigue: el servicio relanzado se queda sin tarjeta; CUBIC; GENET, RP1 y USB; medir en una red de
  verdad.

Lo que no se ha comprobado:

- Nada de esto en una red de verdad ni en una Pi: solo en QEMU.
- `arranque-virt-arch`, `-wayland`, `-gpu` y `-aspecto` (las de la caja Linux con disco de Arch) no se han pasado. En
  ellas no hay vecino: `hora` usará `pool.ntp.org` si el PC tiene Internet.
- La caja Linux con el cortafuegos de entrada nuevo: su puerto pasa por su propio cortafuegos antes; solo se ha pasado
  `arranque-virt-linux`.
- El resolvedor con un servidor de nombres de verdad, más allá de una pregunta por `example.org` al de QEMU.

## 2026-11-16: ninguna prueba sale a Internet, y una sola zona horaria (rama `red-zona`)

Rama `red-zona`, sacada de `revision` (6351ce64).

### Lo que salía fuera

- En `arranque-virt-linux` (sin disco: el servicio de red no puede leer `Prefs/Red`) se usaba el servidor de hora de
  serie: `red` preguntaba `pool.ntp.org` al DNS de QEMU (10.0.2.3, que reenvía al del PC y de ahí a Internet) y `hora`
  mandaba SNTP a un servidor de verdad (`162.159.200.123` en la salida de `shell-nativa`).
- En `arranque-virt` y `arranque-virt-2`, `red-prueba` preguntaba `example.org` al DNS de QEMU ("no cuenta").
- Las variantes con disco ya apuntaban al vecino (`hora.prueba` → 10.0.2.53 por `hosts`): sin pregunta DNS.

### Lo hecho

1. **El ajuste del servidor de hora** (`Prefs/Red/hora`, el de siempre; lo interpreta `dashos::sntp::servidor`, con
   pruebas de PC): la primera línea que no está vacía ni empieza por `#`. `ninguno` lo apaga. Sin fichero (o vacío, o
   solo comentarios), `pool.ntp.org`. **Si el fichero está pero no se deja leer, o el servicio no puede leer sus
   ajustes (sin disco del sistema), apagado**: no se habla con un tercero sin saber qué dice el ajuste. Una línea que
   no es ni nombre ni dirección, también apagado (como antes). El estado lo dice en una palabra nueva,
   `E_HORA_APAGADA` (`E_PALABRAS` pasa de 31 a 32), y `hora` entonces no pregunta a nadie: lo dice una vez y mira el
   estado cada minuto.
2. **QEMU con `restrict=on`** en `arranque-virt` y `arranque-virt-2` (`RED_QEMU`): la red de usuario no deja salir
   nada al PC ni a Internet; DHCP, `guestfwd` (el eco) y `hostfwd` (`red_entra.py`) siguen. Con él, **el DHCP de QEMU
   no da puerta de enlace ni servidor de nombres**: DashOS no tiene ruta hacia fuera (otra barrera más), y la línea
   esperada pasa a ser `[red] DHCP: 10.0.2.15/24` (antes, con `, puerta 10.0.2.2, DNS 10.0.2.3`). De paso, lo que va
   al vecino ya no lo reenvía QEMU también al 127.0.0.1 del PC (la carrera con un servidor del PC apuntada el 15/11).
   En `arranque-virt-linux` se probó y **no vale**: sin puerta de enlace, el cortafuegos de la caja Linux corta el
   ARP del invitado hacia 10.0.2.2 y su `ping` no llega. Esa y las de Arch (`pacman -Sy`) siguen con `RED_CAJA`
   abierta: ahí lo que impide que DashOS salga es el ajuste (sin disco, sin servidor de hora; con disco, el vecino).
   En las de Arch, `hora.prueba` → 10.0.2.53, que QEMU lleva al 127.0.0.1:123 del PC: no sale a Internet, pero si el
   PC tiene un servidor de hora en local, contesta él.
3. **`red-prueba` ya no pregunta `example.org`**. Con eso, el camino "servidor de nombres del DHCP" no tiene prueba de
   punta a punta (el de un servidor elegido, con el vecino, sí; es el mismo código salvo la elección del servidor).

### Una sola zona horaria

Las fechas y horas que escribe el crate `idioma` (`Local::fecha_larga`, `fecha_hora_corta`…) van en la zona del
sistema, la misma que la barra: `SYS:Prefs/zona`, con la tabla de `usuario/src/zona.rs`, que `idioma` incluye tal cual
(`#[path]`; una sola tabla y una sola forma de leer el fichero, `zona::de_fichero`). Los minutos (`gmt`) de
`locale.prefs` solo valen si no hay zona (no está, no se lee, no es texto, mide más de 256 bytes o no está en la
tabla). `ha_cambiado` también mira la firma de `SYS:Prefs/zona`. Cargar el idioma hace una pregunta más al servicio de
archivos (tres sin `locale.prefs`).

La zona es del sistema (decisión 3 de arriba), así que **la prueba de `cambio` cambia**: Carla (inglés, Reino Unido,
`gmt` 0) ve ahora la hora de Madrid, "03/11/2026 18:05", y no "17:05". Es justo lo que comprueba que la zona manda: con
el `gmt` de Ana (-60) las dos fuentes dan lo mismo en noviembre. Que Londres da "17:05" en ese momento está en las
pruebas de PC.

Pruebas de PC nuevas (`idioma`, `cargo test`; `sntp.rs` y `zona.rs`, `make prueba`):

- `sesion_zona_del_sistema`: sin zona, el `gmt`; con Madrid, 18:05 en noviembre y 19:05 en julio (hora de verano), el
  salto de las 02:00 a las 03:00 del 29 de marzo y de las 03:00 a las 02:00 del 25 de octubre, el día que cambia a las
  22:00 UTC de julio; Londres, 17:05 en noviembre y 18:05 en julio; instantes extremos sin reventar.
- `sesion_zona_rota_o_que_no_existe` (hostil): una zona que no existe, a medias, vacía, con la línea en blanco
  delante, bytes que no son texto, UTF-8 cortado, ceros, un camino, más de 256 bytes y un fichero que no se deja leer:
  sin zona, el `gmt`, y `ha_cambiado` no se dispara solo. 20.000 ficheros al azar.
- `zona.rs`: `de_fichero` con lo bueno y lo malo. `sntp.rs`: `el_ajuste` (de serie, `ninguno`, comentarios, bytes que
  no son texto).

### Decisiones, pendientes del usuario

1. **Sin poder leer los ajustes, sin hora por la red** (en vez de `pool.ntp.org`). Una máquina sin disco del sistema, o
   con el fichero ilegible, no pregunta la hora a nadie. La otra opción es usar el de serie también entonces.
2. **La palabra para apagarlo es `ninguno`.**
3. **La hora de los programas es la de la zona del sistema para todos los usuarios**, aunque su `locale.prefs` diga
   otra diferencia: Carla, del Reino Unido, ve la hora de Madrid. Una zona por usuario sigue por hacer (decisión 3 de
   arriba).

### Lo que queda

- La barra y `idioma` leen la zona al arrancar; un cambio de `Prefs/zona` lo ve `ha_cambiado`, pero nadie vuelve a
  cargar por eso salvo el centro al reabrir su ventana.
- Una prueba de punta a punta del DNS del DHCP sin salir fuera (haría falta que QEMU anunciara al vecino como
  servidor de nombres).
- `arranque-virt-linux` y las de Arch siguen con la red de QEMU abierta (el invitado necesita la puerta de enlace, y
  las de Arch, Internet). Cerrarla pide que QEMU dé puerta con `restrict=on`, o que el invitado de `-linux` no la use.
- Con `restrict=on`, `arranque-virt` ya no comprueba que el DHCP traiga puerta y DNS.
