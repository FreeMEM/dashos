# DashOS: guía para agentes

DashOS es un sistema operativo propio para la Raspberry Pi. Implementa la API de AmigaOS 3.1 en AArch64 big-endian y
ejecuta los programas 68k con un JIT, al estilo de Petunia. La primera placa es la Pi 4; se desarrolla en QEMU
`raspi4b`.

Lee `PLAN.md` antes de empezar. Ahí están las decisiones tomadas, las piezas y las fases.

## Reglas

1. **Sala limpia.** Quien implementa no lee desensamblados del Kickstart ni código de AROS. Trabaja con
   `docs/spec/` y las pruebas. Quien analiza escribe especificaciones en prosa y no escribe código del sistema.
   Cada sesión dice al empezar qué papel tiene.
2. **Nada de código de la ROM en el repositorio.** Ni desensamblados, ni traducciones, ni volcados.
3. **Los cambios en ficheros de Emu68 siguen siendo MPL-2.0.** Lo propio de DashOS va en ficheros aparte.
4. **Los cambios en Dash los hacen las sesiones de Dash** (`~/Projects/FreeMEMLang`). Aquí solo se usa.
5. **Cada función de la API llega con su prueba del oráculo.** El mismo binario 68k se ejecuta en FS-UAE con el
   Kickstart real y en DashOS, y las salidas tienen que coincidir.

## Oráculo

```bash
export DASH_KICKSTART="$HOME/Descargas/Telegram Desktop/kick40068.A1200"
```

`tools/emu_run.py` de Dash ejecuta un programa en FS-UAE y devuelve su salida. Sin `DASH_KICKSTART` arranca AROS, y
las pruebas no valen como oráculo.

## Estructura

- `PLAN.md`: el plan.
- `docs/spec/`: especificaciones de la sala limpia, una por función o por grupo.
- `jit-host/`: el JIT de Emu68 solo, como programa Linux big-endian. `make -C jit-host run` lo prueba con
  `qemu-aarch64_be`.
- `toolchains/`: ficheros de CMake y envoltorios para las toolchains de Bootlin. `toolchains/LEEME.md` explica cómo
  compilar Emu68.
- `patches/emu68/`: parches sobre Emu68 (MPL). `third_party/Emu68` es un clon que no va en git.

## Lenguajes

El kernel, en Rust (objetivo `aarch64_be-unknown-none-softfloat`, con nightly). El JIT, en C. Las bibliotecas Amiga
y las aplicaciones, en Dash. Ensamblador solo en el arranque, las excepciones y el cambio de contexto.
