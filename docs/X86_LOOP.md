# x86-64: plan y bitácora del bucle

Encargo del usuario (2026-11-14): "ponte en modo loop y empieza con la parte de ingeniería (diseño, etc,
repositorios) y luego a implementar". La cuota semanal se renueva el lunes 2026-11-16 por la noche; **con el 80 % de
uso, se para**. Diseño en `docs/diseño/S21-x86.md`.

Dos líneas a la vez (decisión 4 de S21):

- **Línea A, DashOS en x86-64** (tareas `X`). Rama `x86` de `~/Projects/dashos`, en `.worktrees/x86`. Otra sesión
  trabaja en `main`: aquí solo se añade, sin mover lo de ARM más de lo imprescindible, para que la fusión sea fácil.
  El nombre sigue siendo DashOS (UsirOS será el público, al publicar).
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
- [x] X3 **Tiempo, interrupciones y núcleos.** ACPI (MADT, HPET o TSC calibrado), APIC local (x2APIC si lo hay) con
  su temporizador, IOAPIC, arranque de los demás núcleos (INIT-SIPI), avisos entre núcleos. El planificador y los
  hilos de `base` funcionando, con las pilas de los hilos en el GB 256 (`mapa::pila_nueva`, que en x86 necesita
  avisar a los demás núcleos para vaciar la TLB). Una GDT y una TSS por núcleo.
- [x] X4 **Procesos.** Anillo 3, `syscall`/`sysret` (`swapgs`, `clac` al entrar), `espacio` de x86 (la mitad baja
  de cada proceso, con PCID en vez de ASID), cambio de contexto, cargador ELF x86-64, la biblioteca
  `usuario/` para `x86_64` (entrada al kernel). `make -C kernel prueba-x86` con las pruebas de `pruebas.rs` que no
  tocan hardware de la Pi.
- [ ] X5 **PCIe y controladores estándar.** MCFG de ACPI, MSI y MSI-X en x86; los servicios `xhci`, `nvme` y
  `virtio_sonido` funcionando en `q35`.
  Lo que dejó X4 para aquí:
  - Las interrupciones de los procesos en x86 son las GSI del IOAPIC (número = GSI + 32, al núcleo 0, en
    `interrupciones.rs`, módulo `arq`). Las MSI van como las LPI de ARM: hoy `arq::LPIS` es 0 en x86.
  - La imagen normal de x86 no lanza `inicio` (`inicio.rs` sigue siendo solo de ARM): hace falta el paquete de
    arranque con los ELF de x86 y las llaves de los aparatos de ACPI y PCIe. Sin él, ningún servicio corre en x86.
  - `registros_proyectar` en x86 acepta lo que esté fuera de la RAM (sin caché); el permiso DMA no lo da nadie aún
    (sin IOMMU, en un PC un aparato llega a toda la memoria).
  - El camino con PCID no está probado: ni QEMU TCG ni el KVM de esta máquina lo dan. Con PCID, cada CR3 lleva el suyo
    y uno soltado se vacía de la TLB la próxima vez que se carga (`espacio.rs`).
  - Quitar una proyección de un proceso vivo vacía la TLB entera de los demás núcleos (`vaciar_tlb_otros`). Con
    muchos núcleos o muchas `memoria_quitar` convendrá algo más fino (INVPCID, o solo los núcleos donde corre).
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


### 2026-11-14 — X3: tiempo, interrupciones y núcleos en x86-64

Hecho:

- `acpi.rs`: RSDP, XSDT (o RSDT), MADT (núcleos, APIC local, IOAPIC, cambios ISA), HPET y FADT (PM timer). Cada tabla
  se comprueba (firma, largo, suma) y solo se lee lo que se ve por el mapa lineal (`mapa::se_ve`).
- `tiempo.rs`: el contador es el TSC, calibrado contra el HPET (o el PM timer; si no hay ninguno, CPUID 0x15/0x16).
  El temporizador del APIC va en modo de plazo del TSC si lo hay; si no, de un disparo, con su frecuencia medida.
- `apic.rs`: x2APIC si lo hay, si no xAPIC. Los 8259 remapeados y enmascarados. Vectores: 0x30-0xEF aparatos
  (`manejar`), 0xF0 reloj, 0xF1 replanificar, 0xF2 TLB, 0xFF espuria. La IDT tiene ya las 256 entradas.
- `ioapic.rs`: todas las entradas enmascaradas al empezar; `isa` (con los cambios de la MADT), `dirigir`, `enmascarar`.
- `nucleos.rs` y `trampolin.s`: INIT, SIPI y SIPI con un trampolín en una página libre por debajo de 1 MB (modo real,
  protegido y largo, con las tablas del arranque). Cada núcleo tiene su GDT, su TSS y sus pilas IST (de
  `pila_nueva`), y su número en la zona a la que apunta GS. Al acabar, fuera la proyección 1:1 del arranque.
- Avisos entre núcleos por IPI. `vaciar_tlb_otros`: generación pedida y hecha por núcleo; quien espera atiende lo que
  le piden a él, y un núcleo que no contesta en 100 ms se avisa y se salta.
- `mapa.rs`: las pilas de los hilos en el GB 256, como en ARM. Al soltar una: fuera las entradas, `invlpg`, vaciado en
  los demás y solo después se devuelven las páginas y la ranura.
- `contexto.s`: `cambiar_contexto` (rbx, rbp, r12-r15 en la pila) e `hilo_entrada`.
- El planificador es el mismo fichero en las dos arquitecturas (`base/planificador.rs`). Lo de cada una, detrás de
  `cfg`: `Contexto` y `contexto_nuevo`, `avisar_nucleo`, `mitad_baja_vacia`, y lo de EL0 (solo ARM por ahora).
  `reloj.rs` también es común. `objetos_x86.rs` da en x86 los tipos de procesos que nombra el planificador.
- Las pruebas C8, C10 y C14 de ARM pasan a `pruebas_hilos.rs` y corren en las dos arquitecturas.

Comprobado: `make -C kernel prueba-x86` da BIEN (q35, `-smp 4`, `-cpu max`): ACPI con 4 núcleos, IOAPIC y HPET; 200 ms
del HPET son 200 ms del TSC (diferencia de 13 µs); 50 o 51 interrupciones del reloj en 500 ms del HPET en cada
núcleo; C8, C10 y C14; los 3 núcleos con su GDT, su TSS y su pila de fallo doble distintas; seis hilos en los
núcleos 1 a 3 que se reparten una suma y terminan; despertar un hilo del núcleo 2 tarda unos 8 ms sin aviso y 66 µs
con él (20 avisos recibidos); la IRQ 4 de la UART por el IOAPIC atendida en el núcleo 2; el núcleo 3 deja de ver una
pila soltada en el núcleo 0 (y sin el vaciado la seguía viendo, incluso con datos de otra: comprobado quitándolo);
100 hilos que terminan sin perder páginas; las pruebas de X2 siguen. También a mano: con `-cpu max,-x2apic` (xAPIC)
y con KVM (`-enable-kvm -cpu host`, que da el modo de plazo del TSC), todo bien. `make -C kernel arranque-x86`,
`make -C kernel prueba` y `make -C kernel arranque-virt` dan BIEN.

No comprobado ni hecho: un PC de verdad (MTRR de los demás núcleos, firmware sin HPET, más de 4 núcleos: solo se
arrancan 4); el PM timer y CPUID como referencia (sin HPET); x2APIC con ID por encima de 255 (el IOAPIC no los
alcanza sin remapeo de interrupciones); las NMI de la MADT (LINT1 va como NMI en todos); apagar o reiniciar núcleos.
Lo que queda para X4 está apuntado en su tarea.


### 2026-11-14 — X4: procesos en x86-64

Hecho:

- `objetos.rs`, `cupo`, `procesos`, `elf`, `llamadas`, `registro`, `temporizadores`, `azar`, `serie` e
  `interrupciones` son comunes. Fuera `objetos_x86.rs` y `serie_x86.rs`. En `interrupciones.rs`, lo del controlador va
  en un módulo `arq` por arquitectura: el GIC y el ITS en ARM, el IOAPIC en x86 (número = GSI + 32, al núcleo 0, por su
  vector de aparatos). El repartidor corre también en x86, y con él la recogida de hilos muertos y los temporizadores
  de los procesos, como en ARM. `Memoria` y `PermisoDma` (`base/memoria.rs`) también son comunes.
- Entrada al kernel por `syscall` (`vectores.s`): `swapgs`, la pila del kernel del hilo desde `gs:[16]`, un marco como
  el de una interrupción y `clac`. Vuelta por `sysret`, tras comprobar RIP canónica y RFLAGS sin IOPL. Las interrupciones
  y fallos desde el anillo 3 hacen `swapgs` y `clac` en la entrada común. RSP0 de la TSS y `gs:[16]` se ponen en cada
  cambio a un hilo de proceso. Bajada al anillo 3 con `iretq` (`entrar_anillo3_x86`).
- Misma ABI que en ARM salvo los registros: número en `rax`, argumentos en `rdi`, `rsi`, `rdx`, `r10`, `r8`,
  resultados en `rax`, `rdx`, `r8` (`Marco::llamada` y `poner_x0..2`; `llamadas.rs` ya no lee `x8` directamente). Al
  arrancar, `rdi` lleva la frecuencia del TSC. Documentado en `docs/referencia/kernel.md`, sección x86-64.
- `base/x86_64/espacio.rs`: la mitad baja es PML4[0] (512 GB, como en ARM) y el PML4 lleva la entrada 511 del kernel.
  PCID de 12 bits si lo hay, con un mapa de PCID "sucios" por núcleo: uno soltado se vacía de la TLB la próxima vez que
  se carga, sin avisos. Quitar una proyección avisa a los demás núcleos. Dentro del kernel, la mitad baja vacía (PCID
  0) además de SMAP. Copias con `rep movsb` entre `stac` y `clac` (`usuario.rs`), con recuperación del fallo.
- Un cerrojo que espera atiende los vaciados de TLB pendientes: así `vaciar_tlb_otros` se puede llamar con un cerrojo
  tomado.
- FPU por hilo con XSAVE (FXSAVE si no hay; x87, SSE, AVX y AVX-512 si cabe en 4 KB), perezosa con CR0.TS como C28.
  TLS: bases de FS y GS por hilo, con FSGSBASE encendido. `rdtsc` sí, `rdpmc` no.
- El planificador común: `bajar_a_el0`, `crear_proceso`, `fp_atrapado` y `fp_cambios` en las dos arquitecturas.
  Arreglo común: un proceso que acaba se suelta antes de apuntar su fin y despertar a quien lo espera (si no,
  `grupos` veía su grupo aún sin vaciar; en x86, con el despertar rápido, fallaba).
- `usuario/` compila para `x86_64-unknown-none` (`make -C kernel usuario-x86`): `llamada` con `syscall`, `_start` de
  ensamblador, `contador`/`frecuencia`, y `fpu_comun`, `tls` y `trampas` con su versión de x86. El PIE de C29 en
  `usuario/asm/pie_x86.s`. Sin RELRO y con la GOT en `.data`, para que el cargador no vea dos segmentos en una página.
- Pruebas en `pruebas_procesos_x86.rs` y los programas en ensamblador en `usuario_x86.s`. Antes de ellas se paran los
  tres hilos que C10 deja ocupando el núcleo 1 (con ellos, `tiempo` fallaba según dónde cayera).

Comprobado: `make -C kernel prueba-x86` da BIEN (varias veces seguidas, q35, `-cpu max`, 4 núcleos). Pasan, como en
ARM: C11, C12 y C13 (en ensamblador), C18 (100 procesos sin perder páginas), C28 (FPU por hilo, también MXCSR y la
palabra de control del x87), C29 (PIE), C32 (tenis, repartos, PCID distintos y reutilizados, TLS y trampas), C43 con
el lanzador (hola, padre, malo, llaves, tiempo y grupos, cada grupo a cero) y C51 (muchos). Y las de X4: despertar un
proceso desde otro núcleo (unos 200 µs en TCG), `syscall` conserva los registros, un proceso no lee la memoria del
kernel ni la de otro proceso, y `wrmsr`, `rdpmc`, `hlt` y saltar al kernel lo matan. Con KVM (`-enable-kvm -cpu host`,
AMD) también, salvo C28, que se queda en 3 cambios de dueño porque los programas acaban en 4 tics (la prueba cuenta
cambios, no fallos: los dos salen con 28). KVM encontró un fallo: AMD no le pone RPL 3 a SS en `sysret`; ahora va en
STAR. `make -C kernel arranque-x86`, `make -C kernel prueba` y `make -C kernel arranque-virt` dan BIEN.

No pasan en x86, porque son de aparatos de la Pi: C20 (serie), C21 (SD), C49 (USB), C50 (buzón, compositor y
gráficos), C46 (DMA con la ventana del árbol), C34 y C38 de ARM (tienen las suyas de X2). Tampoco las de big-endian
(hola-be, fpu_b en big-endian): x86 no lo tiene.

No comprobado: el camino con PCID (ni QEMU TCG ni este KVM lo dan); una CPU sin SMAP, FSGSBASE o XSAVE (el código lo
mira por CPUID, pero solo se probó con `-cpu max` y el anfitrión); interrupciones de aparatos entregadas a un proceso
por el IOAPIC (hay código, sin prueba: llega con X5); la imagen normal de x86 con `inicio` (no lo lanza). Lo que queda
para X5 está apuntado en su tarea.
