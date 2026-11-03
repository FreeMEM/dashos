# DashOS: plan

Nombre elegido por el usuario el 2026-11-03. Antes se llamó Amigo, de forma provisional.

## Qué es

DashOS es un sistema operativo propio para placas ARM. Implementa la API de AmigaOS 3.1 de forma nativa en ARM.
Los programas 68k del sistema se ejecutan sin emular un Amiga entero. Un JIT traduce su código a ARM, y sus llamadas a
las bibliotecas van a las versiones nativas. Es la idea de Petunia en AmigaOS 4 y de Trance en MorphOS.

Los juegos y demos que tocan el chipset directamente no son objetivo. Petunia tampoco los ejecuta.

## Decisiones tomadas

Estas decisiones son del usuario, del 2026-11-03.

- **Placas.** La primera placa real es la Raspberry Pi 4. La Pi 5 viene después y sería lo más redondo. Más
  adelante, otras placas como las Orange Pi. Mientras no haya una Pi 4, se desarrolla en QEMU `raspi4b`.
- **Sistema moderno.** Tiene protección de memoria y usa todos los núcleos de la CPU.
- **Kernel propio.** No hay Linux debajo.
- **AArch64 big-endian.** Los programas 68k leen y escriben a mano las estructuras del sistema. Por eso el sistema
  nativo guarda sus datos en el mismo orden de bytes que el 68k.
- **Punteros de 32 bits en las estructuras compartidas.** Todo lo que ve un programa 68k vive en los primeros 4 GB.
- **Emu68 como base del JIT.** El arranque y el kernel son propios. Se explica en la sección de Emu68.
- **Lenguajes.** El kernel, en Rust. El JIT, en C: es el de Emu68 y no se reescribe. Las bibliotecas Amiga, las
  aplicaciones y las pruebas del oráculo, en Dash. Ensamblador AArch64 solo donde no hay otro remedio: entrada del
  arranque, tabla de excepciones y cambio de contexto. Python para las herramientas.
- **Dash como lenguaje del sistema que ve el usuario.** Las bibliotecas nativas se escriben en Dash. Para eso dashc
  necesita un backend nuevo. Dash no entra en el kernel.
- **Licencia MPL-2.0** para el código propio de DashOS, la misma de Emu68 (usuario, 2026-11-03). Texto en `LICENSE`.
- **La API se reimplementa, no se copia.** AROS sirve de referencia, y la ingeniería inversa del Kickstart 3.1 cubre
  lo que no está documentado.

## Moderno y compatible a la vez

La API de AmigaOS 3 choca con la protección de memoria y con varios núcleos:

- Los programas se pasan mensajes con punteros a su propia memoria. Cualquier programa puede leer la memoria de otro.
- Los programas leen y escriben a mano las estructuras del sistema.
- `Forbid()` y `Disable()` paran el resto del sistema para que un programa trabaje solo. Esto da por hecho que hay
  un único núcleo.

Si el sistema protege la memoria de cada programa 68k, muchos programas fallan. MorphOS y AmigaOS 4 no lo han
resuelto en veinte años.

La salida es separar dos mundos, como hizo MorphOS con su ABox o Mac OS X con el entorno Classic:

- **El kernel de DashOS es moderno.** Cada proceso tiene su memoria protegida, y el planificador reparte los
  procesos entre todos los núcleos.
- **La caja Amiga es un proceso más.** Dentro de la caja, todos los programas 68k y las bibliotecas Amiga comparten la
  memoria, como en un Amiga. `Forbid()` es un cerrojo de la caja y no para el resto del sistema.

Así un programa 68k que falla tira la caja, pero no el sistema. Al principio la caja usa un solo núcleo. Más tarde
puede repartir sus tareas entre varios núcleos con un cerrojo grande, como hizo Linux en sus primeras versiones con
varios procesadores.

Falta una decisión del usuario: si los programas nativos nuevos de DashOS usan la API Amiga dentro de la caja, una API
moderna fuera de ella, o las dos.

## Las piezas

### Emu68

El repositorio es `michalsc/Emu68`, con licencia MPL-2.0. Se comprobó el 2026-11-03:

- Compila con `-mbig-endian` para AArch64.
- Tiene los objetivos `raspi64`, `pbpro`, `rockpro64` y `virt`. El objetivo `virt` es la máquina virtual de QEMU.
- Trae arranque, MMU, interrupciones, árbol de dispositivos, un gestor de memoria (TLSF) y cargadores de hunk y ELF.

Hoy Emu68 hace de CPU 68k completa: carga una ROM y ejecuta el sistema entero. En DashOS el reparto es el contrario.
Nuestro exec nativo es el sistema, y Emu68 es el motor que ejecuta las tareas 68k. Eso pide tres cambios:

1. **Llamadas a bibliotecas.** En el Amiga, cada entrada de la tabla de saltos es un `JMP`. En las bibliotecas
   nativas, la entrada lleva un código de operación reservado. El JIT lo reconoce y llama a la función nativa con los
   registros 68k.
2. **Varias tareas.** Emu68 da por hecho una sola CPU 68k. DashOS necesita un estado de CPU por tarea, y que el
   planificador nativo los cambie. Falta medir cuánto depende el JIT de un estado global.
3. **Sin chipset.** Se quita PiStorm y todo acceso a los chips del Amiga.

La MPL obliga por fichero. Lo que se cambie dentro de los ficheros de Emu68 sigue siendo MPL y se publica. El código
de DashOS, en ficheros propios, lleva la licencia que se elija.

#### Lo que dio la primera prueba (2026-11-03)

- Emu68 compila para `virt` con las toolchains de Bootlin, después de un parche de 27 líneas en
  `patches/emu68/`. El objetivo `virt` estaba abandonado: le faltaban símbolos para enlazar.
- En QEMU arranca, pero se queda esperando a los núcleos 1, 2 y 3. Los despierta con el método de la Raspberry Pi,
  que escribe una dirección en una tabla de memoria fija. QEMU `virt` usa otro método, PSCI. Tampoco saca nada por
  el puerto serie. No se siguió depurando, por lo que dice el punto siguiente.
- Para Raspberry Pi (`raspi64`) compila con el mismo parche, y **arranca en QEMU `raspi4b` en big-endian**. Despierta
  los cuatro núcleos, encuentra el controlador de interrupciones (GICv2) y pide la pantalla al firmware. El registro
  está en `docs/arranque-emu68-qemu-raspi4b.log`. Se para al preparar las cachés del JIT, seguramente porque no tiene
  ROM que cargar; no se investigó.
- **Emu68 no tiene soporte para la Pi 5.** No aparece el chip BCM2712 en su código.
- Su arranque está hecho para un único programa que ocupa toda la máquina. No tiene procesos ni protección de memoria.

Conclusión: el arranque, la MMU, los núcleos y los controladores son de DashOS. De Emu68 se aprovecha el JIT: los
ficheros `M68k_*.c`, `RegisterAllocator64.c` y lo que necesiten.

#### El JIT separado (2026-11-03)

`jit-host/` compila solo el traductor de Emu68 como programa de Linux AArch64 big-endian, sin libc, y lo ejecuta en
shiva con `qemu-aarch64_be` (`make -C jit-host run`). Ejecuta un programa 68k que suma de 1 a 100.000 y lo comprueba.
Funciona: unas 300.000 instrucciones 68k traducidas, con el resultado correcto.

Lo que el JIT necesita de fuera es poco, y nada es del hardware de la Pi:

- Salida de texto: `kprintf` y `vkprintf`.
- Variables globales: `__m68k_state`, `debug_not_implemented` y `dcache_mask_bits`.
- Guardar y cargar el estado 68k en los registros ARM: `M68K_LoadContext` y `M68K_SaveContext`. Están copiados en
  `jit-host/emu68_context.c`, que sigue siendo MPL.
- El desensamblador, que se deja vacío.
- Dos montones de memoria con TLSF: uno general y otro para el código traducido.
- La memoria del código traducido, proyectada dos veces: se escribe en una dirección y se ejecuta en la misma con el
  bit 36 puesto. En el kernel serán dos entradas de MMU.

Lo que esto dice para DashOS:

- **El estado de una tarea 68k son los registros ARM.** Los registros del 68k viven fijos en x13-x29 y en v19-v26.
  Cambiar de tarea 68k es guardar y cargar registros, como con cualquier hilo. No hay un estado global escondido.
- **La caché de traducciones sí es global.** Todas las tareas de la caja la comparten, y con varios núcleos necesitará
  cerrojos.
- **Las direcciones 68k son direcciones reales del proceso.** La caja tiene que poner su memoria en los primeros 4 GB.
- **Emu68 da por hecho que la dirección 0 es memoria válida.** `M68K_InitializeCache` escribe a través de
  `__m68k_state` antes de que `M68K_StartEmu` le dé valor. En el banco de pruebas se le da valor antes.
- **`MainLoop` vuelve cuando el PC del 68k llega a 0.** Así termina el programa de prueba, con un `rts`.

Falta por probar: llamar a una función nativa desde el código 68k, varias tareas, excepciones y la FPU. Ojo: QEMU de
usuario no ve que se reescribe código a través de la otra dirección. Si el JIT reutiliza memoria, QEMU puede ejecutar
traducciones viejas. En hardware real no pasa, porque el JIT limpia las cachés.

### Dash

Dash vive en `~/Projects/FreeMEMLang`. Hoy dashc genera hunks 68k. DashOS le pide tres cosas:

1. **Un backend AArch64 big-endian con punteros de 32 bits.** GCC apenas lo soporta: su modo ILP32 está abandonado.
   Con dashc no se depende de eso.
2. **Un modo sin sistema debajo.** Hoy el runtime de Dash pide la memoria a exec. En DashOS, exec es lo que se escribe.
3. **Puentes 68k a nativo generados.** Se generan a partir de los ficheros `.fd` y `.sfd`. Cada puente pasa los
   argumentos de los registros 68k, como `d0` o `a1`, a la convención de llamada de ARM.

Los cambios en Dash los hacen las sesiones de Dash, con sus pruebas en las dos líneas (dashc y dash1).

### La API

Los nombres, los desplazamientos de cada función (LVO) y las estructuras salen del NDK 3.2. Hyperion lo publica
gratis.

AROS sirve para entender la API, no para copiarla. Su licencia es la APL.

El comportamiento no documentado se saca del Kickstart 3.1. Ejemplos: los efectos secundarios, los registros que se
conservan y los campos privados que usan los programas reales.

## Ingeniería inversa en sala limpia

En la UE se puede hacer ingeniería inversa para lograr interoperabilidad. Lo permite el artículo 6 de la directiva
2009/24/CE. No se puede copiar el código de la ROM ni traducirlo línea a línea. Esto no es consejo legal.

El trabajo se reparte en dos papeles que no se mezclan:

- **El analista** lee el desensamblado del Kickstart 3.1 con Ghidra. Escribe la especificación de cada función en
  prosa, en `docs/spec/`, y las pruebas del oráculo.
- **El implementador** no ve nunca el desensamblado. Trabaja solo con la especificación y las pruebas.

Cada papel es una sesión distinta. Las especificaciones quedan en el repositorio como registro de cómo se hizo.

## El oráculo

`tools/emu_run.py` de Dash ejecuta programas en FS-UAE con el Kickstart 3.1 real y devuelve su salida. Eso es el
oráculo.

1. Se escribe en Dash un programa que prueba una función. Por ejemplo, `AllocMem` con tamaños raros.
2. Se ejecuta con el Kickstart real, y se guarda la salida como la respuesta correcta.
3. Se ejecuta el mismo binario 68k en DashOS, y se compara.

El mismo binario 68k sirve para las dos máquinas. Así cada biblioteca llega con su tanda de pruebas.

## Fases

1. **Separar el JIT de Emu68.** Hecho el 2026-11-03 en `jit-host/`. Lo siguiente es que el código 68k llame a una
   función nativa a través de una tabla de saltos de biblioteca.
2. **Kernel mínimo en Rust, en QEMU `raspi4b`.** Arranque propio en big-endian, MMU, los cuatro núcleos, temporizador,
   interrupciones, puerto serie y pantalla. Procesos con memoria protegida. El arranque de Emu68 para la Pi sirve de
   referencia.
3. **La caja Amiga.** Un proceso con exec dentro: tareas, señales, mensajes, memoria y bibliotecas. Primero con
   código nativo, después con una tarea 68k que lo llame a través del JIT.
4. **Backend ARM de dashc.** En paralelo con las fases 2 y 3. Hasta que esté, el código puede empezar en C.
5. **Bibliotecas.** dos, graphics, layers e intuition, una a una, cada una con sus pruebas del oráculo.
6. **Hardware real.** La Pi 4 primero. Su CPU (Cortex-A72) admite big-endian: Emu68 funciona así en ella a diario.
   Lo más costoso es el USB, que va por el chip VL805 conectado por PCIe. Después la Pi 5: lo básico está en el chip
   BCM2712, y USB, red y GPIO van por el chip RP1, también por PCIe. QEMU no emula la Pi 5.

## Preguntas abiertas

- Antes de la Pi 5, comprobar en una real que su CPU, Cortex-A76, admite big-endian en el kernel. Lo dice el registro
  `ID_AA64MMFR0_EL1`, campo `BigEnd`. Si no lo admite, todo el diseño cambia.
- Si los programas nativos nuevos usan la API Amiga dentro de la caja, una API moderna fuera, o las dos.
- Comprobar que "DashOS" no choca con otro proyecto conocido antes de publicar.
- Si exec, dentro de la caja, empieza en C mientras llega el backend ARM de Dash, o se espera.
- Cómo se dibuja la pantalla: framebuffer de la Pi con un graphics propio, o algo más parecido a RTG.

## Herramientas

- `qemu-system-aarch64` está instalado en shiva.
- Compiladores cruzados de Bootlin 2026.08-1 (GCC 15.3) en `~/.local/opt/toolchains/`: `aarch64be` y
  `powerpc-440fp`. Los ficheros de CMake están en `toolchains/`. El de PowerPC solo lo pide el ROM PPC de Emu68.
- `qemu-aarch64_be` está instalado: ejecuta programas Linux AArch64 big-endian en shiva.
- El Kickstart 3.1 de A1200 está en `~/Descargas/Telegram Desktop/kick40068.A1200`.
