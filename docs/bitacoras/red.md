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
