# Amigo: plan

Nombre provisional. La otra opción que se barajó es AmigaPi.

## Qué es

Amigo es un sistema operativo propio para la Raspberry Pi. Implementa la API de AmigaOS 3.1 de forma nativa en ARM.
Los programas 68k del sistema se ejecutan sin emular un Amiga entero. Un JIT traduce su código a ARM, y sus llamadas a
las bibliotecas van a las versiones nativas. Es la idea de Petunia en AmigaOS 4 y de Trance en MorphOS.

Los juegos y demos que tocan el chipset directamente no son objetivo. Petunia tampoco los ejecuta.

## Decisiones tomadas

Estas decisiones son del usuario, del 2026-11-03.

- **Kernel propio.** No hay Linux debajo.
- **AArch64 big-endian.** Los programas 68k leen y escriben a mano las estructuras del sistema. Por eso el sistema
  nativo guarda sus datos en el mismo orden de bytes que el 68k.
- **Punteros de 32 bits en las estructuras compartidas.** Todo lo que ve un programa 68k vive en los primeros 4 GB.
- **Emu68 como base del arranque y del JIT.** Emu68 ya compila para AArch64 big-endian y arranca sin sistema debajo.
- **Dash como lenguaje del sistema.** Las bibliotecas nativas se escriben en Dash. Para eso dashc necesita un backend
  nuevo.
- **La API se reimplementa, no se copia.** AROS sirve de referencia, y la ingeniería inversa del Kickstart 3.1 cubre
  lo que no está documentado.

## Las piezas

### Emu68

El repositorio es `michalsc/Emu68`, con licencia MPL-2.0. Se comprobó el 2026-11-03:

- Compila con `-mbig-endian` para AArch64.
- Tiene los objetivos `raspi64`, `pbpro`, `rockpro64` y `virt`. El objetivo `virt` es la máquina virtual de QEMU.
- Trae arranque, MMU, interrupciones, árbol de dispositivos, un gestor de memoria (TLSF) y cargadores de hunk y ELF.

Hoy Emu68 hace de CPU 68k completa: carga una ROM y ejecuta el sistema entero. En Amigo el reparto es el contrario.
Nuestro exec nativo es el sistema, y Emu68 es el motor que ejecuta las tareas 68k. Eso pide tres cambios:

1. **Llamadas a bibliotecas.** En el Amiga, cada entrada de la tabla de saltos es un `JMP`. En las bibliotecas
   nativas, la entrada lleva un código de operación reservado. El JIT lo reconoce y llama a la función nativa con los
   registros 68k.
2. **Varias tareas.** Emu68 da por hecho una sola CPU 68k. Amigo necesita un estado de CPU por tarea, y que el
   planificador nativo los cambie. Falta medir cuánto depende el JIT de un estado global.
3. **Sin chipset.** Se quita PiStorm y todo acceso a los chips del Amiga.

La MPL obliga por fichero. Lo que se cambie dentro de los ficheros de Emu68 sigue siendo MPL y se publica. El código
de Amigo, en ficheros propios, lleva la licencia que se elija.

### Dash

Dash vive en `~/Projects/FreeMEMLang`. Hoy dashc genera hunks 68k. Amigo le pide tres cosas:

1. **Un backend AArch64 big-endian con punteros de 32 bits.** GCC apenas lo soporta: su modo ILP32 está abandonado.
   Con dashc no se depende de eso.
2. **Un modo sin sistema debajo.** Hoy el runtime de Dash pide la memoria a exec. En Amigo, exec es lo que se escribe.
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
3. Se ejecuta el mismo binario 68k en Amigo, y se compara.

El mismo binario 68k sirve para las dos máquinas. Así cada biblioteca llega con su tanda de pruebas.

## Fases

1. **Emu68 en QEMU.** Compilar Emu68 para `virt` y arrancarlo en `qemu-system-aarch64`. Medir cuánto cuesta separar
   el JIT del resto.
2. **exec mínimo.** Tareas, señales, mensajes, memoria y bibliotecas, con salida por el puerto serie. Primero nativo,
   después con una tarea 68k que lo llame a través del JIT.
3. **Backend ARM de dashc.** En paralelo con la fase 2. Hasta que esté, exec puede empezar en C.
4. **Bibliotecas.** dos, graphics, layers e intuition, una a una, cada una con sus pruebas del oráculo.
5. **Hardware real.** Primero la Pi 4, que está mejor documentada. La Pi 5 lleva el chip RP1 y es otro trabajo.

## Preguntas abiertas

- El nombre definitivo. "AmigaPi" lleva la marca Amiga, que tiene dueño. Para publicarlo convendría otro nombre.
- La licencia del código propio.
- Si exec empieza en C mientras llega el backend ARM de Dash, o se espera.
- Cómo se dibuja la pantalla: framebuffer de la Pi con un graphics propio, o algo más parecido a RTG.

## Herramientas

- `qemu-system-aarch64` está instalado en shiva.
- No hay compilador cruzado AArch64. Emu68 necesita uno big-endian, por ejemplo `aarch64_be-none-elf` de ARM.
- El Kickstart 3.1 de A1200 está en `~/Descargas/Telegram Desktop/kick40068.A1200`.
