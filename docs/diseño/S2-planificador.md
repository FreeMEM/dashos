# S2. Núcleos y planificador

## El problema

El planificador decide qué hilo usa cada núcleo de la CPU y durante cuánto tiempo. De él depende el principio 1 de
S0: que el sistema responda siempre. Y una de sus metas: 60 cuadros por segundo al mover ventanas con los cuatro
núcleos al 100 %.

La Pi 4 tiene cuatro núcleos Cortex-A72 iguales, a 1,5 GHz (1,8 GHz en las placas recientes). No hay núcleos grandes
y pequeños, como en los móviles. La CPU se calienta: el firmware baja la frecuencia hacia los 82 °C, y el límite es
85 °C.

Hay cuatro clases de trabajo con necesidades distintas:

- **Lo que no puede llegar tarde**: el audio, el compositor y la entrada. Un corte de audio o un cuadro perdido se
  notan enseguida.
- **Lo interactivo**: la aplicación que el usuario tiene delante.
- **Lo de fondo**: compilar, comprimir, indexar ficheros.
- **La caja Amiga**: muchas tareas 68k con las prioridades de exec, de -128 a 127.

## Las opciones

### A. Prioridades fijas con turnos, como exec del Amiga o QNX

Siempre corre el hilo listo de prioridad más alta. Entre iguales, por turnos.

- Gana: es sencillo y predecible. Es lo que conocen los programas Amiga.
- Cuesta: un hilo de prioridad alta en un bucle bloquea todo lo que tiene por debajo. En el Amiga pasaba. Con
  programas de cualquier origen, eso no se puede permitir.

### B. Reparto justo, como Linux (CFS y ahora EEVDF) o el planificador justo de Fuchsia

Cada hilo tiene un peso. El tiempo de CPU se reparte en proporción a los pesos, y nadie se queda sin turno.

- Gana: nadie se muere de hambre. Con carga, todos van algo más lentos por igual.
- Cuesta: no garantiza que el audio o el compositor lleguen a tiempo. Linux lo arregla con reglas que adivinan qué
  es interactivo, y no siempre aciertan.

### C. Presupuestos de tiempo, como seL4 MCS o los perfiles de plazo de Fuchsia

Un hilo pide una capacidad y un periodo: "2 ms de CPU cada 16,6 ms". El planificador se lo garantiza si la suma de lo
pedido cabe en la máquina. Si un hilo agota su presupuesto, espera al siguiente periodo.

- Gana: garantías de verdad para lo que no puede llegar tarde.
- Cuesta: hay que decir cuánto se necesita, y la mayoría de los programas no lo sabe. Para el trabajo normal no sirve.

## Recomendación

**Dos clases, como Fuchsia: presupuestos (C) para lo que no puede llegar tarde, y reparto justo (B) para todo lo
demás.** Más una idea de seL4 para los mensajes entre procesos.

### Clase de plazo

- Hilos con capacidad y periodo. Siempre corren antes que los de la clase justa.
- Solo la usan los servicios del sistema que lo necesitan: compositor, entrada y audio. Una aplicación puede pedirla
  con un permiso aparte (S6). Por ejemplo, un reproductor de música o un sintetizador.
- El kernel admite un presupuesto nuevo solo si cabe. Así nunca se promete lo que no se puede dar.
- Ejemplo para la meta de S0: el compositor tiene 3 ms cada 16,6 ms. Aunque los cuatro núcleos estén al 100 %, ese
  tiempo está reservado.

### Clase justa

- Reparto por pesos con tiempo virtual, como EEVDF de Linux o el planificador justo de Fuchsia. Nadie se queda sin
  turno.
- La aplicación que tiene el foco del teclado sube de peso mientras lo tiene. Esto no adivina nada: lo dice el
  compositor, que sabe qué ventana está delante.
- Las tareas de fondo bajan de peso, y el usuario puede marcar un programa como "de fondo".

### Mensajes entre procesos: el servidor trabaja con el tiempo del cliente

Es la idea de los servidores pasivos de seL4 MCS. Cuando una aplicación pide algo al sistema de archivos, el hilo del
servidor corre con el presupuesto y la prioridad de la aplicación. Se consiguen tres cosas:

- No hay inversión de prioridades: un servidor no se queda atascado detrás de trabajo menos importante.
- El tiempo que gasta el servidor se le cobra a quien lo pidió. Una aplicación no puede esconder su consumo en un
  servidor.
- Con el paso directo de S1, el kernel salta del cliente al servidor sin pasar por la cola.

### Los cuatro núcleos

- **Una cola por núcleo**, como Fuchsia y Linux. Cada núcleo decide solo, sin cerrojo global.
- **Equilibrado**: un núcleo libre roba trabajo a uno cargado. Los hilos tienden a quedarse en su núcleo, para no
  perder la caché.
- **Afinidad**: se puede reservar un núcleo para la clase de plazo si hace falta. Por ejemplo, el audio en el
  núcleo 3.
- **Dentro del kernel**: estado por núcleo y cerrojos pequeños por objeto. Rust ayuda a que no haya carreras entre
  núcleos sin un cerrojo grande.

### La caja Amiga

- Dentro de la caja se respeta la prioridad estricta de exec, porque hay programas que dependen de ella.
- La caja entera es, para el kernel, un grupo de la clase justa. Si una tarea 68k se queda en un bucle con prioridad
  alta, se bloquea la caja, pero no el escritorio ni el resto del sistema.
- La caja usa un núcleo a la vez, con un testigo. Repartir una caja entre núcleos no es seguro con programas Amiga
  normales: ver S4.

### Energía y temperatura

- **Sin reloj fijo cuando no hay trabajo.** Un núcleo sin hilos duerme con `wfi` hasta la siguiente interrupción o
  temporizador. Es lo que Linux llama "tickless".
- **Frecuencia**: se cambia pidiéndoselo al firmware por el buzón de la VideoCore. Emu68 ya lo hace con las etiquetas
  `GetClockRate` (`0x00030002`), `GetMaxClockRate` (`0x00030004`) y `SetClockRate` (`0x00038002`), en
  `src/raspi/support_rpi.c`.
- **Política**: subir a la frecuencia máxima en cuanto haya carga interactiva o un plazo en peligro, y bajar despacio.
  Responder va antes que ahorrar (principio 1). La Pi 4 suele ir enchufada.
- **Temperatura**: el firmware se protege solo hacia los 82 °C. DashOS lee la temperatura para avisar en el
  escritorio y para no subir la frecuencia cuando ya está al límite.

## Lo que queda abierto

1. ¿Las tareas Amiga mantienen la prioridad estricta de exec dentro de la caja, aunque una tarea en bucle congele la
   caja entera? Recomendado: sí, porque solo se congela la caja.
2. ¿Perfil de energía por defecto "responder" (frecuencia máxima en cuanto hay carga) o "ahorrar"? Recomendado:
   responder.

## Cómo encaja

- Con S0: la clase de plazo es la que hace posible la meta de 60 cuadros con carga, y el principio 1.
- Con S1: usa el paso directo de mensajes y añade el préstamo de presupuesto del cliente al servidor.
- S4 diseña cómo exec reparte las tareas dentro de la caja.
- S6 decide quién puede pedir la clase de plazo.
- S9 fija el presupuesto real del compositor, que aquí es un ejemplo.
- S12 incluye el controlador del buzón de la VideoCore, que hace falta para la frecuencia.

## Fuentes

- Fuchsia, planificador de Zircon: [visión general](https://fuchsia.dev/docs/concepts/kernel/kernel_scheduling) y
  [planificador justo](https://fuchsia.dev/docs/concepts/kernel/fair_scheduler).
- seL4 MCS, contextos de planificación y servidores pasivos: [tutorial](https://docs.sel4.systems/Tutorials/mcs) y
  [artículo "It's Time"](https://arxiv.org/pdf/1606.00111).
- Temperatura de la Pi 4: [virten.net](https://www.virten.net/2020/11/thermal-throttling-on-raspberry-pi-4-running-esxi-arm/),
  [CNX Software](https://www.cnx-software.com/2019/07/26/how-to-overclock-raspberry-pi-4/) y
  [blog de Raspberry Pi](https://www.raspberrypi.com/news/thermal-testing-raspberry-pi-4/).
- Etiquetas del buzón: comprobadas en el código de Emu68 (`src/raspi/support_rpi.c`, líneas 183 a 296).

Lo que no se comprobó: el coste real de cambiar de hilo en una A72, y la etiqueta del buzón para leer la temperatura.
Los 3 ms del compositor son un ejemplo, no una medida.

## Matar procesos y señales que se cumplen

Petición del usuario (2026-11-03): poder matar cualquier proceso, y que las señales se cumplan siempre. En AmigaOS no
pasa: Ctrl-C solo pone una señal (`SIGBREAKF_CTRL_C`), y el programa la atiende si quiere. Un programa colgado o en
un bucle no se puede parar sin reiniciar.

### Por qué en AmigaOS no se puede

- **exec no sabe de quién es cada cosa.** La memoria, las ventanas, los ficheros abiertos y los puertos no tienen
  dueño apuntado. Si se quita una tarea a la fuerza, todo eso se queda perdido o, peor, otro programa lo sigue usando.
- **Todo está en la misma memoria.** Una tarea puede tener a medias una estructura que comparten otras, o estar dentro
  de `Forbid()`. Matarla ahí deja el sistema bloqueado.
- **Las señales son una petición.** El sistema no tiene forma de obligar.

### Programas nativos de DashOS: matar siempre funciona

El micronúcleo (S1) lo resuelve de raíz:

- **Todo lo que tiene un proceso es un objeto del kernel con dueño**: memoria, hilos, canales, ventanas (a través de
  su canal con el compositor) y ficheros (a través de su canal con el sistema de archivos). Al matar el proceso, el
  kernel lo cierra todo. Los servidores ven que el canal se ha cerrado y liberan lo suyo.
- **Matar es una capacidad.** Quien tiene la llave de un proceso con el derecho "terminar" lo puede matar. El kernel
  no le pregunta al proceso. Para el kernel, un hilo en un bucle es igual de fácil de parar que uno dormido.
- **Dos pasos, como en macOS o systemd.**
  1. Petición de cierre: el proceso recibe un aviso y tiene unos segundos para guardar y salir. Es lo que hace el
     botón de cerrar o Ctrl-C.
  2. Cierre forzoso: si no sale a tiempo, el kernel lo mata. No se puede ignorar ni bloquear.
- **El escritorio detecta los cuelgues.** El compositor manda un "¿sigues ahí?" a cada ventana, como el `ping` de
  Wayland. Si no contesta en unos segundos, la ventana se oscurece y ofrece "forzar salida".
- **Un gestor de tareas** lista procesos, consumo de CPU y memoria, y permite los dos pasos. Es una aplicación más,
  con la capacidad de terminar procesos del usuario.

### Dentro de la caja Amiga: el exec de DashOS lleva la cuenta

Aquí DashOS puede hacerlo mejor que AmigaOS, porque el exec de la caja es nuestro:

- **Cada recurso se apunta con su tarea dueña**: `AllocMem` y `AllocVec`, ventanas y pantallas, `Lock` y ficheros
  abiertos, bibliotecas y dispositivos abiertos, puertos, semáforos y señales reservadas.
- **Cada tarea 68k es un hilo del kernel.** La caja puede detener una tarea en cualquier momento, aunque esté en un
  bucle sin llamar al sistema.
- **Matar una tarea Amiga** sigue este orden:
  1. Se manda `SIGBREAKF_CTRL_C`, como en el Amiga, y se esperan unos segundos.
  2. Si no sale, se detiene su hilo.
  3. Si tenía `Forbid()`, `Disable()` o semáforos, se sueltan.
  4. Se cierran sus ventanas, ficheros, bibliotecas y puertos.
  5. La memoria se libera si nadie más la está usando. Una tarea pudo pasar un mensaje con un puntero a su memoria;
     esa memoria se queda viva hasta que el mensaje vuelve, o se marca como perdida.
- **Si aun así la caja queda tocada**, se mata la caja entera. Eso siempre es seguro, porque para el kernel la caja
  es un proceso como cualquier otro.
- **Programas de confianza dudosa en su propia caja.** Se puede lanzar un programa 68k en una caja aparte. Si falla,
  solo cae él.

El detalle de cómo lleva exec la cuenta va en S4.
