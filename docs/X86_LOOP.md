# x86-64: plan y bitácora del bucle

Encargo del usuario (2026-11-14): "ponte en modo loop y empieza con la parte de ingeniería (diseño, etc,
repositorios) y luego a implementar". La cuota semanal se renueva el lunes 2026-11-16 por la noche; **con el 80 % de
uso, se para**. Diseño en `docs/diseño/S21-x86.md`.

Dos líneas a la vez (decisión 4 de S21):

- **Línea A, UsirOS (antes DashOS) en x86-64** (tareas `X`). Rama `x86` de `~/Projects/dashos`, en `.worktrees/x86`. Otra sesión
  trabaja en `main`: aquí solo se añade, sin mover lo de ARM más de lo imprescindible, para que la fusión sea fácil.
  El cambio de nombre DashOS → UsirOS no se hace en este bucle.
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

- [x] X1 **Arranque en x86-64.** El programa de `uefi/` compila también para `x86_64-unknown-uefi`
  (`EFI/BOOT/BOOTX64.EFI`), carga el kernel compilado para `x86_64-unknown-none` y salta a él con lo que necesite
  (mapa de memoria, tablas ACPI, GOP). El kernel escribe por COM1 (0x3F8) "DashOS en marcha (x86-64)". Lo de ARM
  (`base`, GIC, ITS, árbol de dispositivos, PL011) queda detrás de `#[cfg(target_arch = "aarch64")]`; lo de x86 va en
  `kernel/src/base/x86_64/`. `make -C kernel arranque-x86`: QEMU `q35` con OVMF
  (`/usr/share/edk2/x64/OVMF_CODE.4m.fd`). La compilación de ARM no cambia.
- [x] X2 **Memoria y excepciones.** GDT, IDT, TSS; tablas de páginas propias de 4 niveles con el mismo reparto que en
  ARM (lo que pide `base`: `mmu`, `espacio`, `memoria`, `monton`); la memoria física desde el mapa de UEFI; SMEP, SMAP
  y NX encendidos.
- [~] X3 **Tiempo, interrupciones y núcleos.** ACPI (MADT, HPET o TSC calibrado), APIC local (x2APIC si lo hay) con
  su temporizador, IOAPIC, arranque de los demás núcleos (INIT-SIPI), avisos entre núcleos. El planificador y los
  hilos de `base` funcionando, con las pilas de los hilos en el GB 256 (`mapa::pila_nueva`, que en x86 necesita
  avisar a los demás núcleos para vaciar la TLB). Una GDT y una TSS por núcleo.
- [ ] X4 **Procesos.** Anillo 3, `syscall`/`sysret` (`swapgs`, `clac` al entrar), `espacio` de x86 (la mitad baja
  de cada proceso, con PCID en vez de ASID), cambio de contexto, cargador ELF x86-64, la biblioteca
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

### 2026-11-14 — X1: arranque en x86-64

Hecho:

- `uefi/` compila también para `x86_64-unknown-uefi` (`BOOTX64.EFI`). Lo de x86 está en `uefi/src/x86_64.rs`. Lo de
  ARM de `main.rs` (árbol, PL011, `dc civac`, el salto) queda detrás de `cfg(target_arch = "aarch64")`.
- El cargador de x86 lleva dentro el kernel como ELF y pone cada segmento PT_LOAD en su dirección física, como código
  del cargador (OVMF marca los datos del cargador como no ejecutables). Le deja una `InfoArranque` con el mapa de
  memoria de UEFI, la RSDP de ACPI y la pantalla del GOP. Sale de los servicios de arranque y salta con la info en
  `rdi`.
- El kernel x86 está en `kernel/src/base/x86_64/`: entrada en ensamblador (`arranque.s`), COM1 (16550), la
  `InfoArranque` y `parar`. Enlazado en 1 MB (`enlace_x86.ld`), sin PIE, con la proyección 1:1 del firmware.
- `main.rs`: en x86 solo entra un `kmain` mínimo. Escribe por COM1 la versión, el mapa de memoria, la RSDP (firma y
  suma comprobadas), la pantalla y "DashOS en marcha (x86-64)", y se para con `hlt`.
- `make -C kernel arranque-x86`: QEMU `q35` con OVMF y una copia de sus variables, 60 s como mucho
  (`tools/prueba_uefi.sh` con `ARQ=x86_64`; sin `ARQ`, igual que antes). Esperados en `kernel/pruebas/arranque_x86.txt`.

Queda fuera en x86: todo el resto del kernel (memoria, MMU, excepciones, GIC e ITS, árbol, temporizadores,
planificador, procesos, llamadas, `pruebas.rs`), y `alloc`. Lo de ARM sigue entero, detrás de su `cfg`.
La mitad alta del kernel en x86 llega con sus tablas de páginas (X2).

Comprobado: `make -C kernel arranque-x86` da BIEN (106 entradas en el mapa, 2041 MB libres, ACPI 2, pantalla
1280x800). En ARM, `make -C kernel`, `make -C kernel prueba` y `make -C kernel arranque-virt` dan BIEN. `make prueba`
ya pasaba antes del cambio (comprobado en una copia limpia de 191966b). La imagen normal de ARM, compilada en el mismo
directorio antes y después, mide lo mismo y cambia en 3 bytes: los números de línea de `main.rs` de los mensajes de
pánico.

No comprobado: un PC de verdad; OVMF con arranque seguro; que 1 MB esté libre en otros firmwares (si no, el cargador
lo dice y no arranca).


### 2026-11-14 — X2: memoria y excepciones en x86-64

Hecho:

- El kernel x86 está enlazado en los 2 GB de arriba (`0xFFFF_FFFF_8000_0000` + física), como pide el modelo de
  código "kernel"; se carga en 1 MB físico. El cargador pone los segmentos en `p_paddr` y salta a la entrada física.
- `arranque.s` hace unas tablas de arranque en `.bss` (4 GB con páginas de 2 MB: 1:1, el mapa lineal y la imagen),
  carga CR3 y salta arriba.
- `excepciones.rs` y `vectores.s`: GDT con los segmentos del anillo 0 y 3 en el orden de `syscall`/`sysret`, TSS
  con pilas IST para fallo doble, NMI y comprobación de máquina, e IDT con las 32 excepciones. Lo que no se trata
  se cuenta por COM1 (vector, nombre, código de error, RIP, CR2 y registros) y el núcleo se para. `int3` avisa y
  sigue, como BRK en ARM.
- La memoria física sale del mapa de UEFI con el mismo repartidor de ARM (`base/memoria.rs`, con `iniciar_tramos`
  para x86). Libre: lo de los servicios de arranque y la memoria convencional; el primer MB, la imagen y lo del
  cargador quedan fuera. Zona DMA de 32 bits; sin zona solo para DMA (no hay IOMMU). Hasta 16 GB físicos.
- `mapa.rs`: tablas de 4 niveles propias, todo en PML4[511] como en ARM: el mapa lineal desde `KERNEL_VA` con
  páginas de 2 MB y NX, sin alias de la imagen; la imagen con W^X por páginas y guardas bajo cada pila; el GB 256
  reservado para las pilas de los hilos. Entradas globales.
- `cpu.rs`: CPUID, y se encienden NX (EFER.NXE), CR0.WP, PGE, SMEP, SMAP y UMIP si la CPU los tiene.
- El montón (`base/monton.rs`) y los cerrojos (`base/cerrojo.rs`, con IF en vez de DAIF) son los de ARM.
  `kprintln!` en x86 va por `serie_x86.rs`.
- `make -C kernel prueba-x86` con `pruebas_x86.rs` y `pruebas/esperado_x86.txt`.

Comprobado: `make -C kernel prueba-x86` da BIEN en QEMU `q35` con `-cpu max`. La memoria libre al empezar es la
que da el mapa de UEFI contada aparte (522168 páginas), y después eso menos las 10 tablas del mapa. Reparto y
devolución de páginas, y el montón (también un bloque de 64 KB que vuelve entero). Las entradas del mapa: código
sin escritura, `.rodata` y datos sin ejecución, la guarda y el alias lineal del código sin proyectar. Escribir y
leer una página sin proyectar y escribir en `.rodata` dan el fallo de página con el código y CR2 esperados, y se
recuperan. NX, SMEP y SMAP de verdad: ejecutar datos, ejecutar una página de usuario y leerla fallan. El desborde
de la pila del arranque da un fallo doble por la pila IST, con CR2 en la guarda. `make -C kernel arranque-x86`,
`make -C kernel prueba` y `make -C kernel arranque-virt` dan BIEN. La imagen normal de ARM mide lo mismo y cambia en
8 bytes (números de línea de los pánicos de `memoria.rs`).

No comprobado ni hecho: `espacio` en x86 (va con los procesos, X4: depende de `cupo`, `objetos` y el planificador);
las pilas de los hilos y una GDT y TSS por núcleo (X3); una CPU sin NX, SMEP o SMAP (el código las mira por CPUID,
pero solo se probó con `-cpu max`); un PC de verdad; RAM por encima de 16 GB (no se usa).
