# S1. Arquitectura del kernel

## El problema

Hay que decidir qué hace el kernel y qué se queda fuera. Esa decisión marca todo lo demás: cómo se escriben los
controladores, cuánto daño hace un fallo, cómo se aíslan las aplicaciones y cuánto cuesta hablar entre procesos.

El kernel va en Rust, para AArch64 big-endian, y la primera placa es la Pi 4 (`PLAN.md`). Los principios de S0 que
más pesan aquí son tres: el sistema responde siempre (1), es seguro por defecto (2) y una persona puede leer el
kernel entero (7).

## Lo que se comprobó

Una prueba pequeña en `docs/diseño/pruebas/rust-be/` lo confirma:

- Rust nightly compila para `aarch64_be-unknown-none-softfloat` con `-Zbuild-std=core`. El resultado es un ELF de
  64 bits big-endian.
- Ese kernel arranca en QEMU `raspi4b`, pone la CPU en big-endian y escribe por el puerto serie de la Pi 4.
- QEMU entra en EL2, el nivel del hipervisor. El firmware real de la Pi 4 también.
- La CPU declara `ID_AA64MMFR0_EL1.BigEnd = 1`. Eso quiere decir que admite los dos órdenes de bytes en el kernel y
  en los programas de usuario. Es el mismo valor que da el manual de la Cortex-A72, y es la puerta para los
  programas Linux little-endian de S7.

Lo que no se comprobó: el mismo kernel en una Pi 4 real.

Dos cosas que salieron de la prueba y afectan al diseño:

- **Los periféricos de la Pi son little-endian.** Con la CPU en big-endian, cada lectura o escritura de un registro
  de un periférico hay que darle la vuelta. En la prueba se ve con el puerto serie (`swap_bytes`). El kernel necesita
  un tipo para registros de periférico que lo haga siempre. Si no, cada controlador es una trampa.
- **Muchas bibliotecas de Rust para controladores dan por hecho little-endian.** No se pueden usar tal cual. Hay que
  revisarlas una a una (S12).

## Las opciones

### A. Kernel monolítico, como Linux

Todo va dentro del kernel: controladores, sistemas de archivos y red.

- Gana: las llamadas internas son llamadas de función, sin coste extra. Es el camino más conocido.
- Cuesta: un fallo en un controlador tira el sistema entero. El kernel crece sin límite: Linux tiene unos 30
  millones de líneas. El aislamiento de aplicaciones hay que añadirlo después, como en Linux con espacios de nombres
  y seccomp. Choca con los principios 2 y 7.

### B. Micronúcleo con capacidades, como seL4, QNX o Fuchsia

El kernel solo hace lo imprescindible: espacios de direcciones, hilos, comunicación entre procesos, capacidades e
interrupciones. Controladores, sistemas de archivos, red y escritorio son procesos normales.

Una capacidad es un permiso que se tiene en la mano, como una llave. Un proceso solo puede usar lo que alguien le ha
dado. Si una aplicación no tiene la llave de la red, no hay forma de que llegue a la red.

- Gana: un controlador que falla se reinicia sin tirar el sistema. El aislamiento de aplicaciones sale gratis de las
  capacidades. El kernel es pequeño: seL4 tiene unas 10.000 líneas de C. QNX demuestra desde hace décadas que este
  diseño responde bien, en coches y en máquinas industriales.
- Cuesta: hablar con un controlador o con el sistema de archivos es un mensaje entre procesos, no una llamada de
  función. Hay que hacer esos mensajes muy rápidos. Redox, un micronúcleo en Rust, ha recortado su latencia un 24 %
  este año cambiando de hilo directamente del cliente al servidor, sin pasar por el planificador.

### C. Framekernel, como Asterinas

Un solo kernel, como el monolítico, pero el código `unsafe` de Rust se encierra en una base pequeña. El resto del
kernel, controladores incluidos, se escribe en Rust seguro. Asterinas lo hace así: implementa más de 200 llamadas
al sistema de Linux, y solo el 14 % de su código es de confianza.

- Gana: rendimiento de kernel monolítico con buena parte de la seguridad de Rust.
- Cuesta: un controlador que se cuelga o entra en bucle sigue tirando el sistema. Rust evita fallos de memoria, no
  fallos de lógica. Y el aislamiento de aplicaciones sigue siendo cosa aparte. Asterinas funciona en x86 y RISC-V;
  no consta que funcione en AArch64.

### D. Híbrido

Un micronúcleo con algunas piezas dentro por rendimiento, como el macOS actual o Windows NT.

- Gana: flexibilidad.
- Cuesta: con el tiempo todo acaba dentro, y se pierden las ventajas de B.

### E. Exokernel, como Aegis y XOK del MIT

El kernel solo reparte el hardware de forma segura: páginas de memoria, bloques de disco, tiempo de CPU. No ofrece
ficheros, ni procesos con forma, ni red. Cada aplicación lleva enlazada una biblioteca que monta esas cosas a su
gusto (una "library OS").

- Gana: cada aplicación hace las cosas a su manera y sin capas de por medio. En los experimentos del MIT, algunos
  servidores web iban varias veces más rápido.
- Cuesta: compartir es difícil. Si cada programa lleva su propio sistema de archivos, ponerlos de acuerdo sobre el
  mismo disco es un problema. Nunca salió del laboratorio para un escritorio.

### F. Un solo espacio de direcciones, protegido por el lenguaje, como Singularity o Theseus

No hay cambio de tabla de páginas entre procesos. Todo vive en el mismo espacio, como en el Amiga. La protección no
la da la MMU, sino el lenguaje: el compilador de Rust (Theseus) o de C# (Singularity de Microsoft) garantiza que un
programa no toca la memoria de otro.

- Gana: comunicar procesos cuesta lo mismo que una llamada de función. Es lo más parecido al Amiga en espíritu.
- Cuesta: solo protege al código escrito en ese lenguaje seguro. El código 68k del JIT, los programas en C y los
  programas de Linux no pasan por ese compilador. Para ellos hace falta la MMU igualmente. DashOS tiene que ejecutar
  justo esa clase de código, así que esta opción no le sirve como base. Sí sirve como idea dentro de la caja Amiga,
  que ya es un espacio compartido (S4).

### G. Multikernel, como Barrelfish

Un kernel por núcleo de CPU, y los kernels se hablan con mensajes, como si la máquina fuera una red pequeña.

- Gana: escala muy bien en máquinas de decenas o cientos de núcleos.
- Cuesta: con los cuatro núcleos de la Pi 4 no aporta nada, y complica todo.

### H. Unikernel, como MirageOS o Unikraft

Una sola aplicación compilada junto con el trozo de kernel que necesita.

- Gana: arranque en milisegundos, tamaño mínimo. Útil para servicios en la nube.
- Cuesta: no hay varios programas a la vez. No sirve para un escritorio.

### Las opciones lado a lado

| | Rendimiento | Un fallo de controlador | Aislar aplicaciones | Tamaño del kernel | Encaje con DashOS |
|---|---|---|---|---|---|
| A. Monolítico | el mejor | tira el sistema | se añade aparte | enorme | malo |
| B. Micronúcleo | muy bueno si los mensajes son rápidos | se reinicia el controlador | viene con las capacidades | pequeño | el mejor |
| C. Framekernel | el mejor | tira el sistema | se añade aparte | mediano | bueno |
| D. Híbrido | bueno | depende de dónde esté | se añade aparte | mediano, y crece | regular |
| E. Exokernel | el mejor para cada aplicación | depende | difícil de compartir | mínimo | malo |
| F. Un solo espacio | el mejor | depende del lenguaje | solo para código seguro | pequeño | no sirve como base |
| G. Multikernel | escala a muchos núcleos | se reinicia | viene con el diseño | pequeño por núcleo | sobra para 4 núcleos |
| H. Unikernel | el mejor | tira la aplicación | no hay varias | mínimo | no sirve |

### Micronúcleo puro o pragmático

Dentro de B hay dos escuelas:

- **Puro, como seL4.** El kernel ni siquiera decide cómo se reparte la memoria: lo hace un proceso. Es lo más pequeño
  y lo que se ha podido demostrar correcto con matemáticas. A cambio, todo cuesta más de montar.
- **Pragmático, como QNX o Zircon.** El kernel gestiona la memoria y el planificador con sus políticas dentro. Es
  algo más grande, pero mucho más sencillo de usar.

Para DashOS conviene el pragmático. No se busca una demostración formal, sino un sistema de escritorio que se pueda
terminar.

### Cuánto cuestan de verdad los mensajes

El miedo al micronúcleo viene de Mach, en los años 90, cuyos mensajes eran lentos. Los de la familia L4, a la que
pertenece seL4, cuestan del orden de cientos de ciclos según sus propias publicaciones. A 1,5 GHz son décimas de
microsegundo. Leer un fichero en DashOS serían dos saltos: de la aplicación al sistema de archivos, y de este al
controlador del disco. Lo caro es mover datos, y eso va por memoria compartida, sin copiar.

## Recomendación

**B, un micronúcleo pragmático con capacidades.** Con dos ideas tomadas de C. El usuario dio el visto bueno al micronúcleo el 2026-11-03.

Razones:

1. **Responder siempre (principio 1).** En un micronúcleo, un controlador lento no puede bloquear el escritorio. El
   compositor y la entrada son procesos con prioridad alta y su propio presupuesto de tiempo (S2).
2. **Seguro por defecto (principio 2).** Las capacidades son el aislamiento. No hay que inventar un sistema de
   permisos encima: cada aplicación arranca con las llaves que le da quien la lanza.
3. **Comprensible (principio 7).** Un kernel de entre 15.000 y 25.000 líneas de Rust cabe en la cabeza de una
   persona. La meta de S0 (menos de 50.000) se cumple con holgura.
4. **Encaja con el Amiga.** exec ya era un sistema de mensajes entre tareas, con puertos. Los puertos de mensajes de
   DashOS se parecen a los del Amiga, y la caja Amiga (S4) los traduce sin esfuerzo.

Lo que se toma de C:

- **El `unsafe` vive en una sola caja de Rust**, la base del kernel: registros, tablas de páginas, cambio de
  contexto y acceso a periféricos. El resto del kernel y todos los servidores, en Rust seguro. Es la disciplina de
  OSTD, la base de Asterinas.
- **Asterinas como referencia para la capa de Linux (S7).** Su lista de llamadas al sistema de Linux, ya escritas
  en Rust y con licencia MPL-2.0, es un buen punto de partida.

### Qué hace el kernel

- **Memoria**: espacios de direcciones, objetos de memoria que se pueden compartir y proyectar, y las tablas de
  páginas (S3).
- **Hilos y planificador**, con los cuatro núcleos (S2).
- **Comunicación entre procesos**, de dos clases:
  - Mensajes síncronos cortos, con paso directo del cliente al servidor. Es el camino rápido para preguntar y
    responder.
  - Anillos en memoria compartida para datos en volumen: disco, red, audio y pantalla. El kernel solo avisa de que
    hay trabajo. Es la idea de `io_uring` de Linux, que Redox también ha copiado este año.
- **Capacidades**: cada objeto del kernel (memoria, hilo, canal, interrupción) se usa a través de una llave con
  derechos concretos. Las llaves se pueden pasar en los mensajes y recortar, pero nunca ampliar.
- **Interrupciones**: llegan al controlador como un mensaje.
- **Temporizadores**.

### Qué queda fuera, en procesos

Controladores, sistemas de archivos, red, compositor y escritorio, la caja Amiga y la capa de Linux. Un proceso
especial, el gestor de arranque, recibe todas las capacidades al principio y las reparte (S14).

### Bibliotecas de Rust que se pueden aprovechar

Con cuidado por el orden de bytes:

- `aarch64-cpu` para los registros del sistema. No toca memoria, así que no le afecta el orden de bytes.
- Un analizador del árbol de dispositivos (`fdt`). El árbol es big-endian por definición, así que en DashOS se lee
  sin dar la vuelta a nada.
- `smoltcp` para la pila de red (S13). Hay que comprobar cómo trata el orden de bytes.

La versión de Rust nightly se fija en un `rust-toolchain.toml`. Así una versión nueva no rompe el kernel sin aviso.
La compilación de la prueba ya avisa de que `core` lleva código que Rust rechazará en el futuro.

## Lo que queda abierto

1. ¿Micronúcleo con capacidades (recomendado), o prefieres otra de las opciones?
2. ¿Qué licencia lleva el kernel? MPL-2.0, como Emu68 y Asterinas, deja aprovechar su código. MIT, como Dash, es más
   abierta. La GPL obligaría a publicar todo lo que se derive.

## Cómo encaja

- Con S0: cumple los principios 1, 2 y 7, y la meta de tamaño del kernel.
- S2 diseña el planificador y el paso directo de los mensajes.
- S3 diseña los objetos de memoria y el hueco de 4 GB de la caja Amiga.
- S4 traduce los puertos de mensajes del Amiga a los de DashOS.
- S6 construye la seguridad sobre las capacidades.
- S7 usa que la CPU admite los dos órdenes de bytes, y Asterinas como referencia.
- S12 necesita el tipo de registro de periférico que da la vuelta a los bytes.

## Fuentes

- seL4, rendimiento y pruebas: [sel4.systems](https://sel4.systems/About/Performance) y
  [sel4bench](https://github.com/seL4/sel4bench/).
- Asterinas: [artículo en USENIX ATC 2025](https://www.usenix.org/conference/atc25/presentation/peng-yuke),
  [preimpresión](https://arxiv.org/abs/2506.03876) y [estado del proyecto](https://x-cmd.com/blog/250624/).
- Redox, paso directo de mensajes y anillos tipo `io_uring`:
  [Phoronix, septiembre de 2026](https://phoronix.com/news/Redox-OS-September-2026) y
  [agosto de 2026](https://phoronix.com/news/Redox-OS-August-2026).

Las cifras de seL4 (unas 10.000 líneas) y de QNX son de su documentación pública conocida. No se buscó una medida
del coste de los mensajes de seL4 en una Cortex-A72: sus páginas solo publican la A57.
