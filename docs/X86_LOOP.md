# x86-64: plan y bitácora del bucle

Encargo del usuario (2026-11-14): "ponte en modo loop y empieza con la parte de ingeniería (diseño, etc,
repositorios) y luego a implementar". La cuota semanal se renueva el lunes 2026-11-16 por la noche; **con el 80 % de
uso, se para**. Diseño en `docs/diseño/S21-x86.md`.

Dos líneas a la vez (decisión 4 de S21):

- **Línea A, AlborOS en x86-64** (tareas `X`). Rama `x86` de `~/Projects/dashos`, en `.worktrees/x86`. Otra sesión
  trabaja en `main`: aquí solo se añade, sin mover lo de ARM más de lo imprescindible, para que la fusión sea fácil.
  El cambio de nombre DashOS → AlborOS no se hace en este bucle.
- **Línea B, RelevoJIT** (tareas `R`). `~/Projects/RelevoJIT`, su propio repositorio (privado en GitHub hasta que el
  usuario diga). Diseño en su `docs/DISEÑO.md`; tareas y bitácora en su `docs/BITACORA.md`.

## Reglas de cada vuelta

1. **Uso.** Al empezar, lee `~/.claude/state/uso-7d.txt` (`<porcentaje> <epoch>`). Con 76 % o más, no se lanza nada
   nuevo. Con 80 % o más, se cierra (regla 8).
2. **Quién hace qué.** La sesión del bucle coordina: lanza agentes en segundo plano, revisa lo que entregan, prueba
   y apunta. Como mucho **un agente por línea** a la vez (mismo árbol de trabajo); en la línea B, dos si trabajan en
   ficheros distintos (grupos de instrucciones) y en worktrees aparte.
3. **El encargo del agente.** El texto de la tarea, más: papel de implementador; las reglas de sala limpia de
   `AGENTS.md` (regla 1) o de `RelevoJIT/docs/DISEÑO.md`; `unsafe` solo en `kernel/src/base/` (regla 6); deja una
   prueba automática; commit local `<tarea>: <resumen>` cuando las pruebas pasan; sin push; contesta con lo hecho, lo
   probado y lo que falta, en pocas líneas.
4. **Revisar.** Al volver un agente: la sesión del bucle ejecuta las pruebas ella misma (`make -C kernel prueba` en
   ARM sigue pasando en la línea A; `make test` en la B). Si algo está roto y no se arregla, se revierte el commit.
5. **Apuntar.** `[x]` en la tarea y una entrada en la bitácora de su línea: qué se hizo, qué se comprobó, qué no.
6. **Ritmo.** Los agentes avisan al terminar. Entre medias, un despertador de respaldo de 20 a 30 minutos.
7. **Bloqueos.** Una tarea atascada dos veces va a "Bloqueos" y se pasa a la siguiente.
8. **Cierre** (80 % de uso, o todo hecho): parar los agentes que queden (TaskStop), revertir lo que esté a medias,
   escribir "Resumen" al final de cada bitácora, mandar un PushNotification con el resultado y parar el bucle.
   RelevoJIT se sube a GitHub (push) al cerrar cada fase `R` y al cierre.

## Tareas de la línea A

- [ ] X1 **Arranque en x86-64.** El programa de `uefi/` compila también para `x86_64-unknown-uefi`
  (`EFI/BOOT/BOOTX64.EFI`), carga el kernel compilado para `x86_64-unknown-none` y salta a él con lo que necesite
  (mapa de memoria, tablas ACPI, GOP). El kernel escribe por COM1 (0x3F8) "DashOS en marcha (x86-64)". Lo de ARM
  (`base`, GIC, ITS, árbol de dispositivos, PL011) queda detrás de `#[cfg(target_arch = "aarch64")]`; lo de x86 va en
  `kernel/src/base/x86_64/`. `make -C kernel arranque-x86`: QEMU `q35` con OVMF
  (`/usr/share/edk2/x64/OVMF_CODE.4m.fd`). La compilación de ARM no cambia.
- [ ] X2 **Memoria y excepciones.** GDT, IDT, TSS; tablas de páginas propias de 4 niveles con el mismo reparto que en
  ARM (lo que pide `base`: `mmu`, `espacio`, `memoria`, `monton`); la memoria física desde el mapa de UEFI; SMEP, SMAP
  y NX encendidos.
- [ ] X3 **Tiempo, interrupciones y núcleos.** ACPI (MADT, HPET o TSC calibrado), APIC local (x2APIC si lo hay) con
  su temporizador, IOAPIC, arranque de los demás núcleos (INIT-SIPI), avisos entre núcleos. El planificador y los
  hilos de `base` funcionando.
- [ ] X4 **Procesos.** Anillo 3, `syscall`/`sysret`, cambio de contexto, cargador ELF x86-64, la biblioteca
  `usuario/` para `x86_64` (entrada al kernel). `make -C kernel prueba-x86` con las pruebas de `pruebas.rs` que no
  tocan hardware de la Pi.
- [ ] X5 **PCIe y controladores estándar.** MCFG de ACPI, MSI y MSI-X en x86; los servicios `xhci`, `nvme` y
  `virtio_sonido` funcionando en `q35`.
- [ ] X6 **Pantalla.** GOP hasta el compositor y el kit, como en `virt` (U4): el escritorio en QEMU x86.
- [ ] X7 **Caja Amiga en x86** con el intérprete de RelevoJIT (necesita R3 y la caja Amiga en `main`).
- [ ] X8 **La caja con el JIT** de RelevoJIT (necesita R6).
- [ ] X9 **Un PC de verdad**, arrancando desde USB (necesita la máquina; decisión 5 de S21).

## Bloqueos

(ninguno)

## Bitácora

### 2026-11-14 19:xx — Puesta en marcha

Rama `x86` desde `main` (3170b53) en `.worktrees/x86`, con S21 y las decisiones del día en `PLAN.md`. RelevoJIT
creado en `~/Projects/RelevoJIT` (MPL-2.0, diseño en `docs/DISEÑO.md`). Uso semanal al empezar: 34 %.
