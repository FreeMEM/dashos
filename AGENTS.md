# DashOS: guía para agentes

DashOS es un sistema operativo propio para la Raspberry Pi. Implementa la API de AmigaOS 3.1 en AArch64 y ejecuta
los programas 68k con un JIT, al estilo de Petunia. Lo nativo es little-endian; la caja Amiga (el JIT y sus
bibliotecas) es big-endian, como el 68k. La primera placa es la Pi 4; se desarrolla en QEMU `raspi4b`.

Lee `PLAN.md` antes de empezar. Ahí están las decisiones tomadas, las piezas y las fases.

## Reglas

1. **Sala limpia.** Quien implementa no lee desensamblados del Kickstart ni código de AROS. Trabaja con
   `docs/spec/` y las pruebas. Quien analiza escribe especificaciones en prosa y no escribe código del sistema.
   Cada sesión dice al empezar qué papel tiene.
   Lo mismo con Linux para los controladores propios (MPL): quien analiza puede leer Linux y escribe la
   especificación; quien implementa no lo lee. Nada de un fichero MPL sale copiado ni traducido de código GPL.
2. **Nada de código de la ROM en el repositorio.** Ni desensamblados, ni traducciones, ni volcados.
3. **Los cambios en ficheros de Emu68 siguen siendo MPL-2.0.** Lo propio de DashOS va en ficheros aparte.
4. **Los cambios en Dash los hacen las sesiones de Dash** (`~/Projects/FreeMEMLang`). Aquí solo se usa.
5. **Cada función de la API llega con su prueba del oráculo.** El mismo binario 68k se ejecuta en FS-UAE con el
   Kickstart real y en DashOS, y las salidas tienen que coincidir.

6. **`unsafe` solo en `kernel/src/base/`.** El resto del kernel tiene `deny(unsafe_code)`. Lo que necesite del
   hardware se pide a `base` con una interfaz segura; una función pública de `base` que no sea segura con cualquier
   argumento es `unsafe fn`. Las únicas excepciones son las pruebas y los controladores que solo van con ellas.
7. **Controladores de Linux, solo en la zona GPL** (usuario, 2026-11-07). Se usan controladores de Linux tal cual
   para lo que no tiene controlador propio (Wi-Fi, GPU de portátiles, aparatos USB raros), cada uno como un proceso
   aparte con una capa que le imita el kernel de Linux (como DDE de Genode). Ese código y su capa son GPL-2.0, viven en
   un directorio propio con su licencia, y solo hablan con DashOS por canales. Ahí sí se lee y se copia Linux; de ahí
   no sale nada hacia el kernel ni hacia los ficheros MPL. Quien reparta DashOS con ellos ofrece su código fuente. El
   firmware que cargan lleva su propia licencia: se comprueba que se puede repartir antes de meterlo.

## Oráculo

```bash
export DASH_KICKSTART="$HOME/Descargas/Telegram Desktop/kick40068.A1200"
```

`tools/emu_run.py` de Dash ejecuta un programa en FS-UAE y devuelve su salida. Sin `DASH_KICKSTART` arranca AROS, y
las pruebas no valen como oráculo.

## Estructura

- `PLAN.md`: el plan.
- `docs/CONSTRUCCION_LOOP.md`: el bucle de construcción (tareas C y V, bitácora).
- `docs/spec/hw/`: especificaciones del hardware sin manual (vídeo, GPU), escritas por analistas.
- `kernel/`: el kernel en Rust. `make -C kernel` saca la imagen normal, sin pruebas; `make -C kernel arranque` la
  arranca en QEMU. `make -C kernel prueba` saca la imagen con las pruebas del arranque (`kernel/src/pruebas.rs`,
  opción `pruebas`) y los programas de `usuario/`, y la prueba en QEMU `raspi4b`.
- `usuario/`: biblioteca `dashos` y programas de usuario en Rust.
- `docs/referencia/kernel.md`: lo que un programa ve del kernel. Llamadas, llaves, formato del ELF, mapa de memoria
  y limitaciones. Quien cambie una llamada o el cargador la pone al día en el mismo commit. Lo que ve un programa de
  los servicios (USB...), en `docs/referencia/servicios.md`.
- `docs/DISEÑO_LOOP.md`: el bucle de diseño (tareas S0 a S15 y bitácora). Sus documentos van en `docs/diseño/`.
- `docs/spec/`: especificaciones de la sala limpia, una por función o por grupo.
- `jit-host/`: el JIT de Emu68 solo, como programa Linux big-endian. `make -C jit-host run` lo prueba con
  `qemu-aarch64_be`.
- `toolchains/`: ficheros de CMake y envoltorios para las toolchains de Bootlin. `toolchains/LEEME.md` explica cómo
  compilar Emu68.
- `patches/emu68/`: parches sobre Emu68 (MPL). `third_party/Emu68` es un clon que no va en git.

## Lenguajes

El kernel, en Rust (objetivo `aarch64-unknown-none-softfloat`, little-endian desde C33, con nightly). El JIT, en C. Las bibliotecas Amiga
y las aplicaciones, en Dash. Ensamblador solo en el arranque, las excepciones y el cambio de contexto.
