# S0. Principios y metas

## El problema

El encargo pide un sistema "mejor que Linux". Linux tiene treinta años de controladores, millones de programas y
miles de personas detrás. DashOS no puede ganarle en todo, y si lo intenta no terminará nunca.

Este documento elige en qué compite DashOS y en qué no. Cada meta lleva un número y una forma de medirlo. Una meta
sin medida no sirve para decidir nada.

## Principios

Estos principios ordenan las decisiones de las tareas siguientes. Cuando dos chocan, gana el que va antes.

1. **Responde siempre.** El puntero, el teclado y el escritorio no se traban nunca, aunque el sistema esté cargado.
   Es la sensación del Amiga, y es lo que más se nota al usarlo. El planificador (S2) y el compositor (S9) se
   diseñan para esto antes que para el rendimiento total.
2. **Seguro por defecto.** Cada aplicación ve solo lo que se le da: sus ficheros, la red si la pide, la cámara si
   la pide. Nada de "todo programa puede leer todo lo del usuario", como en Linux de escritorio (S6).
3. **Todo o nada.** El sistema base no se toca a mano. Se cambia con actualizaciones atómicas: se aplican enteras o
   no se aplican, y se puede volver atrás (S5, S8, S14).
4. **Un solo sistema.** Un escritorio, un juego de controles, una forma de configurar y una documentación. En Linux
   cada pieza viene de un proyecto distinto y se nota. En DashOS todo sale del mismo diseño.
5. **Amiga de primera.** Los programas 68k funcionan como ciudadanos del escritorio, no encerrados en un emulador
   (S4). Y DashOS recoge lo mejor del Amiga: asignaciones lógicas de volúmenes, guiones entre programas como ARexx,
   arranque inmediato y un sistema que se entiende entero.
6. **Linux como invitado.** El software de Linux se aprovecha, pero no manda en el diseño. Corre en una capa o en
   una máquina virtual, no como base del sistema (S7).
7. **Pequeño y comprensible.** Una persona puede leer el kernel entero. Lo que no cabe en esa idea va fuera del
   kernel (S1).
8. **Medido.** Cada meta de este documento tiene una prueba automática. Si una versión empeora una medida, no sale.

## Metas medibles

Las referencias de Linux son orientativas. Las únicas que valen son las medidas en la misma placa con el mismo
método, y aún no hay una Pi 4 donde medir.

| Meta | DashOS | Referencia en Linux | Cómo se mide |
|---|---|---|---|
| Del kernel al escritorio | menos de 1 s | unos 13 s | Marca de tiempo por el puerto serie al entrar al kernel y al pintar el primer cuadro del escritorio |
| De encender a escritorio, Pi 4 | menos de 5 s | unos 17 s con Raspbian (PC-Welt) | Cronómetro con vídeo, o un pin GPIO que se enciende al pintar el escritorio |
| Memoria en reposo con escritorio | menos de 128 MB | unos 294 MB en una pila mínima de 64 bits (RDK) | Contador del kernel, tras un minuto en reposo |
| Fluidez con carga | 60 cuadros por segundo al mover ventanas con los cuatro núcleos al 100 % | se pierden cuadros | Prueba de estrés que mide los cuadros que no llegan a tiempo |
| Latencia de entrada | igual o menor que Linux en la misma placa | ver Nett, abajo | Clic a fotón con fotodiodo, el método de Nett |
| Arranque de la caja Amiga | menos de 1 s hasta el primer programa 68k | Amiberry con Kickstart: varios segundos | Marca de tiempo al crear la caja y al primer `OpenWindow` |
| Volver atrás tras una actualización | un reinicio, menos de 10 s | depende de la distribución | Prueba que estropea una actualización a propósito |
| Aislamiento | una aplicación sin permiso no lee `HOME` | en Linux puede | Prueba que lo intenta y tiene que fallar |
| Tamaño del kernel | menos de 50.000 líneas de Rust | unos 30 millones de líneas (con controladores) | Recuento de líneas en cada versión |

Sobre el arranque: el firmware de la Pi 4 tarda unos 3,5 s antes de cargar ningún kernel. Unos 2 s de eso son la
puesta en marcha de la memoria. Esa parte no la controla DashOS. Por eso la meta propia es "del kernel al
escritorio", y la de "encender a escritorio" depende del firmware. `initial_turbo` en `config.txt` recorta
alrededor de un segundo.

Sobre la latencia: la medida más cuidada que hay (Marco Nett, 2026) encuentra diferencias de menos de un
milisegundo entre compositores de Linux. El coste grande aparece con XWayland, unos 3 ms. Otras medidas caseras dan
de 42 a 64 ms en total, pero con métodos que no se pueden comparar. Conclusión: la latencia de Linux con un
compositor moderno ya es buena. DashOS no debe prometer ganar ahí, sino no perder, y ganar en lo que pasa con el
sistema cargado.

Las metas de memoria y de tamaño del kernel son provisionales. Se revisan en S1 y S3.

## En qué no compite DashOS

- **Variedad de hardware.** Pi 4 primero, después Pi 5 y alguna placa más. No hay x86, ni portátiles, ni tarjetas
  gráficas de PC.
- **Servidores y nube.** DashOS es un sistema de escritorio para una persona delante de la pantalla.
- **Todo el software de Linux de forma nativa.** El que haga falta va por la capa de S7.
- **Juegos 3D exigentes.** La GPU de la Pi 4 no da para eso, y portar sus controladores cuesta mucho (S9).
- **Cumplir POSIX entero.** Se ofrece lo que haga falta para portar programas (S11), no la norma completa.

## Hardware que cubre

- **Raspberry Pi 4** de 1, 2, 4 y 8 GB, y la CM4. Es la placa de referencia: todas las metas se miden ahí.
- **QEMU `raspi4b`** para desarrollar. No sirve para medir: su temporización no es la del hardware.
- **Raspberry Pi 5**, en una fase posterior. Antes hay que comprobar que su CPU admite big-endian (`PLAN.md`).
- **Raspberry Pi 3**: posible placa de pruebas, porque el usuario tiene varias. Su CPU admite big-endian, pero su
  controlador de interrupciones es otro y tiene 1 GB de memoria. Ver la pregunta 2.

## Lo que queda abierto

1. ¿Valen estas metas? Sobre todo las de arranque (menos de 5 s) y memoria (menos de 128 MB).
2. ¿La Pi 3 entra como placa soportada, o solo la Pi 4 y después la Pi 5? Soportarla obliga a dos controladores de
   interrupciones desde el principio.
3. ¿DashOS es para una persona por máquina, o para varias cuentas? Cambia mucho la seguridad (S6).
4. ¿Hace falta comprar ya una Pi 4 para tener la medida de referencia con Raspberry Pi OS? Sin ella, las
   comparaciones con Linux son de oídas.

## Cómo encaja

Este documento no depende de ninguna tarea anterior. Las siguientes lo usan así:

- S1 y S3 confirman o corrigen las metas de tamaño del kernel y de memoria.
- S2 y S9 se diseñan para el principio 1 y la meta de fluidez con carga.
- S5, S8 y S14 se diseñan para el principio 3 y la meta de volver atrás.
- S6 depende de la pregunta 3.
- S12 depende de la pregunta 2.

## Fuentes

- Tiempos del firmware de la Pi 4: [raspberrypi/firmware#1375](https://github.com/raspberrypi/firmware/issues/1375)
  y [un caso con kernel reducido](https://windows-hexerror.linestarve.com/q/so60288459-striped-down-kernel-take-long-time-to-load-on-raspberry-pi-4).
- Cadena de arranque de la Pi 4, documentada por Haiku: [haiku-os.org](https://haiku-os.org/docs/develop/kernel/arch/arm/rpi4.html).
- Latencia de entrada en Linux: [Marco Nett, X11 frente a Wayland](https://itsfoss.com/news/x11-vs-wayland-input-latency/).
- Arranque de Raspbian en la Pi 4: [PC-Welt](https://www.pcwelt.de/?p=1188045).
- Memoria en reposo de una pila mínima de 64 bits en la Pi 4: [RDK](https://wiki.rdkcentral.com/x/GlC7D).

Lo que no se comprobó: ninguna de las cifras de Linux se ha medido en una Pi 4 propia. La de "del kernel al
escritorio en Linux, unos 13 s" sale de restar los 4 s de firmware a los 17 s de PC-Welt, y es una estimación.
