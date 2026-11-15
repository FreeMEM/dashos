# x86-64: plan y bitácora del bucle

Encargo del usuario (2026-11-14): "ponte en modo loop y empieza con la parte de ingeniería (diseño, etc,
repositorios) y luego a implementar". La cuota semanal se renueva el lunes 2026-11-16 por la noche; **con el 80 % de
uso, se para**. Diseño en `docs/diseño/S30-x86.md`.

Dos líneas a la vez (decisión 4 de S30):

- **Línea A, DashOS en x86-64** (tareas `X`). Rama `x86` de `~/Projects/dashos`, en `.worktrees/x86`. Otra sesión
  trabaja en `main`: aquí solo se añade, sin mover lo de ARM más de lo imprescindible, para que la fusión sea fácil.
  El nombre sigue siendo DashOS (UsirOS será el público, al publicar).
- **Línea B, RelevoJIT** (tareas `R`). `~/Projects/RelevoJIT`, su propio repositorio (privado en GitHub hasta que el
  usuario diga). Diseño en su `docs/DISEÑO.md`; tareas y bitácora en su `docs/BITACORA.md`.

## Reglas de cada vuelta

1. **Uso.** Al empezar, lee `~/.claude/state/uso-7d.txt` (`<porcentaje> <epoch>`). Con 76 % o más, no se lanza nada
   nuevo. Con 80 % o más, se cierra (regla 8).
2. **Quién hace qué.** La sesión del bucle coordina: lanza agentes en segundo plano, revisa lo que entregan, prueba
   y apunta. Desde el 2026-11-14 a las 22:3x (el usuario pidió gastar menos), **un solo agente a la vez** en todo el
   bucle, y nada nuevo con la ventana de 5 horas por encima del 80 % (`~/.claude/cache/usage-now.json`). Antes era
   como mucho **un agente por línea** a la vez (mismo árbol de trabajo); en la línea B, dos si trabajan en
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
- [x] X5 **PCIe y controladores estándar.** MCFG de ACPI, MSI y MSI-X en x86; los servicios `xhci`, `nvme` y
  `virtio_sonido` funcionando en `q35`.
- [x] X6 **Pantalla.** GOP hasta el compositor y el kit, como en `virt` (U4): el escritorio en QEMU x86. También
  la consola como proceso (COM1 por el mapa de E/S de la TSS) y `memoria_crear` con pausas para las interrupciones.
  Lo que queda abierto, para después:
  - Sin INTx en x86: su número sale del `_PRT` del AML de ACPI, que no se lee. Todos los aparatos de `q35` tienen MSI.
  - Las interrupciones del IOAPIC entregadas a un proceso siguen sin probar (los aparatos van por MSI).
  - Sin IOMMU (VT-d o AMD-Vi): un aparato con bus master alcanza toda la memoria. Hecho en X5b (si el firmware da
    DMAR o IVRS).
  - Otras llamadas largas con las interrupciones cerradas: `memoria_dma` pone a cero hasta 4 MB con el cerrojo del
    repartidor tomado, y soltar 4 MB o un proceso grande también va de una vez. Solo `memoria_crear` tiene pausas.
  - Lo que quedaba de X4 sigue igual: el camino con PCID sin probar, y `vaciar_tlb_otros` vacía la TLB entera.
- [~] X7a **RelevoJIT dentro de DashOS x86, sin caja Amiga.** La caja Amiga aún no está en `main` (vive en las ramas
  `caja-amiga*` de la otra sesión), así que primero RelevoJIT corre en un proceso de DashOS: compilado sin libc
  (capa de puertos en RelevoJIT: memoria, `memcpy`, matemáticas), su memoria de código sobre objetos de memoria de
  DashOS (doble proyección RW y RX), y un programa que ejecuta los programas de `jit-host/pruebas68k` con el
  intérprete y con el JIT, con la misma salida que en Linux. El cargador avisa con `relevo_invalidate`.
- [x] X5b **IOMMU** (VT-d de Intel y AMD-Vi): tablas DMAR e IVRS de ACPI, un dominio por aparato, el DMA de cada
  servicio limitado a sus ventanas, y el aviso de fallos de DMA. Requisito para pasar aparatos a la caja Linux y
  para el dominio de controladores de la zona GPL (S32 y S33 de `revision`). En QEMU: `-device intel-iommu` y
  `-device amd-iommu`. Prioridad de la línea A. Lo que queda abierto:
  - Remapeo de interrupciones (VT-d y AMD-Vi): no está. Hace falta para núcleos con ID de APIC de más de 8 bits y
    para pasar aparatos a una máquina virtual (X11) sin que puedan mandar cualquier interrupción.
  - Alias: un aparato detrás de un puente de PCIe a PCI hace DMA con el número del puente; no recibe dominio (su DMA
    falla). Tampoco los grupos de funciones sin ACS. Solo el segmento 0 del PCI.
  - Sin probar en una máquina de verdad (pegaso, VT-d; shiva, AMD-Vi), ni con RMRR o IVMD (QEMU no los da): lo de la
    pantalla de la gráfica integrada de Intel depende de las RMRR.
  - VT-d solo en modo clásico (tabla raíz y de contexto, segundo nivel), sin el modo escalable. No se mira si la
    unidad tiene el clásico: en una que solo tuviera el escalable, no funcionaría.
- [ ] X5c **Aparatos de un PC de verdad**: AHCI (SATA), HDA (sonido), y la red de cada placa o, mientras, la del
  dominio de controladores de la zona GPL; y compartir internet del móvil por USB (CDC-NCM y RNDIS).
- [ ] X10 a X17 **La gráfica compartida** (`docs/diseño/S34-grafica-compartida.md`): X10 hipervisor de x86 (VT-x con
  EPT, AMD-V con NPT); X11 pasar un aparato PCIe a una máquina virtual; X12 el dominio de gráficos; X13 canal y
  memoria compartida sin copia; X14 Mesa en DashOS (Venus y contexto nativo); X15 el compositor sobre el dominio;
  X16 CUDA en la caja por reenvío de la interfaz de Nvidia, y el dominio de cómputo; X17 vídeo por hardware. Detrás de X5b.
  - [~] X10 **Hipervisor de x86**: AMD-V con NPT hecho y probado (TCG y KVM en shiva); VT-x con EPT escrito y sin
    probar (en pegaso, `freemem` no puede abrir `/dev/kvm`). Lo que queda abierto:
    - Probar VT-x en pegaso (ver la bitácora: hace falta acceso a `/dev/kvm`).
    - El globo (llamada 55) en x86: da `E_NO_HAY`. Hará falta que las vCPU de otros núcleos salgan y tomen un ASID o
      VPID nuevo antes de devolver las páginas (el contador `generacion` del espacio ya está).
    - Sin APIC virtual por hardware (AVIC, APICv) ni interrupciones entregadas sin salir (posted interrupts): el APIC
      lo emula el servicio y cada EOI sale. Para el dominio de gráficos (S34) hará falta.
    - VT-x: PAE de 32 bits sin modo largo (los PDPTE no se cargan en el VMCS); el VMCS se limpia (VMCLEAR) tras cada
      llamada, así que cada `maquina_correr` hace VMLAUNCH (más lento que VMRESUME). AMD: CR4 no se vigila.
    - INS y OUTS, y las instrucciones sobre aparatos que no son MOV, MOVZX o MOVSX: la máquina muere.
    - DR0 a DR3 y DR6 no cambian de mundo. Un triple fallo mata la máquina en vez de reiniciarla.
    - `inicio_x86.rs` no da el permiso de máquinas ni lanza la caja Linux; el servicio `caja-linux` es de ARM (su
      emulación del GIC, el PSCI y el arranque de Linux para arm64). Hará falta su parte de x86 (APIC, IOAPIC, PIC,
      el protocolo de arranque de Linux de x86).
    - Arrancar un Linux en una VM de x86: después (no era parte de X10).
- [ ] X7 **Caja Amiga en x86** con el intérprete de RelevoJIT (necesita R3 y la caja Amiga en `main`).
- [ ] X8 **La caja con el JIT** de RelevoJIT (necesita R6).
- [ ] X9 **Un PC de verdad**, arrancando desde USB (necesita la máquina; decisión 5 de S30).

## Bloqueos

(ninguno)

## Bitácora

### 2026-11-14 19:xx — Puesta en marcha

Rama `x86` desde `main` (3170b53) en `.worktrees/x86`, con S30 y las decisiones del día en `PLAN.md`. RelevoJIT
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


### 2026-11-14 — X5: PCIe y controladores estándar en x86-64

Hecho:

- `acpi.rs` lee la MCFG (los tramos de ECAM). `mapa.rs` tiene el GB 257 para los registros que toca el kernel, sin
  caché (PCD y PWT), solo fuera de la RAM; `base/x86_64/mmio.rs` da una `Ventana` con accesos de 32 bits comprobados.
- `kernel/src/pci.rs`: recorre todos los buses de la MCFG y las funciones de cada aparato, mide las BAR (también de 64
  bits) y da una MSI-X (su primera entrada, la tabla en cualquier BAR) o, si no la tiene, una MSI.
- Las MSI van como las LPI de ARM: números desde la 8192, enmascaradas en el kernel, al núcleo 0, cada una por su
  vector (0xB0 a 0xEF). Las GSI del IOAPIC quedan en los vectores 0x30 a 0xAF (de la 32 a la 159).
- `kernel/src/inicio_x86.rs`: la imagen normal de x86 lanza `inicio` con el mismo paquete de arranque que ARM, los ELF
  de x86 y las llaves del xHCI, el NVMe y el sonido de virtio (registros, permiso DMA y MSI-X).
- Servicios en x86 sin cambios de lógica: `xhci`, `nvme`, `disco`, `archivos`, `ficheros`, `sonido`,
  `virtio-sonido` y los tonos. Las barreras de ARM pasan a `dashos::barrera` (en x86, del compilador o `mfence`).
  `entrada` (nuevo, de prueba) lee los eventos del xHCI mientras no hay compositor en x86.
- `virtio-sonido` pone la entrada 0 de MSI-X en cada cola: sin eso, con MSI-X el aparato no avisa. En ARM (INTx) no
  cambia nada.
- La consola de x86 es un hilo del kernel que saca el registro por COM1 con las interrupciones abiertas.
- AES y Poly1305 (de RedoxFS) van por software en x86 (`--cfg aes_force_soft`, `poly1305_force_soft`): sin SSE no
  compilaban.
- El montón de usuario pide trozos de 1 MB, no de 4. Se encontró midiendo: el sonido se cortaba al arrancar en x86 una
  vez de cada dos. `archivos` pedía 4 MB en una sola llamada, que en QEMU tardaba hasta 23 ms sin interrupciones en su
  núcleo, y la MSI del sonido se entregaba tarde.
- `make -C kernel arranque-x86`: q35 con `qemu-xhci` (teclado detrás de un concentrador y ratón), NVMe con
  `target/nvme.img` y `virtio-sound`; teclas y ratón por el monitor (`TECLAS=1`), el disco leído después en Linux y el
  WAV analizado con `tools/sonido_wav.py`, como `arranque-virt`.

Comprobado: `make -C kernel arranque-x86` da BIEN cinco veces seguidas (la salida, el disco y el sonido), con los tres
aparatos por MSI-X (8192 a 8194) y las BAR en 768 GB. A mano, con KVM (`-enable-kvm -cpu host`), también BIEN, con las
BAR en 56 TB. `make -C kernel prueba-x86`, `make -C kernel prueba`, `make -C kernel arranque` y
`make -C kernel arranque-virt` dan BIEN.

No comprobado: un PC de verdad; la INTx y el IOAPIC con un proceso; MSI sin MSI-X (el código está, `q35` no lo
usa); aparatos detrás de un puente del PCIe (se recorren todos los buses de la MCFG, pero `q35` los pone en el 0);
varios segmentos de PCIe (solo el 0); un núcleo 0 con ID de APIC de más de 8 bits (la MSI no llegaría: haría falta el
remapeo de interrupciones). Lo que queda está en X6.


### 2026-11-14 — X6: pantalla, consola como proceso y llamadas largas en x86-64

Hecho:

- `inicio_x86.rs` da la pantalla del GOP como en `virt`: la memoria (memoria del firmware, fuera de la RAM) y sus
  medidas en un objeto de memoria, con el formato del nodo `simple-framebuffer` de U4. Salen de la `InfoArranque`. El
  formato del GOP va al revés: 1 (B, G, R) es a8r8g8b8.
- `inicio` lanza en x86 el compositor, `ventana-a`, `ventana-b` y Ajustes, como en ARM. El compositor nombra antes que
  nadie el canal "ventanas" y lee el teclado y el ratón del xHCI. Fuera `entrada` (el programa de prueba de X5).
  El compositor, el kit y Wayland compilan para x86 sin cambios.
- La consola es un proceso, como en ARM. Objeto nuevo, `Puertos` (solo lo crea el kernel, en x86), y llamada 29,
  `puertos_permitir`: el proceso puede usar esos puertos con `in` y `out`. La TSS de cada núcleo lleva un mapa de E/S
  de los puertos 0 a 0x3FF (más allá del límite, todo prohibido). El cambio de hilo pone el del proceso que entra, si
  no lo tenía ya (una versión por cada vez que se dan puertos, global: un proceso nuevo en la misma dirección no hereda
  el mapa de otro). `inicio` da a la consola el registro y los puertos de COM1. Fuera el hilo de consola del kernel.
- `memoria_crear` pone a cero página a página fuera del cerrojo del repartidor, y tras cada página abre un momento las
  interrupciones (`cerrojo::ventana_irq`), en las dos arquitecturas. En x86 la ventana es `sti`, `nop`, un salto y
  `cli`: QEMU sin KVM solo mira las interrupciones al empezar un bloque traducido. Una página entre pausas y no ocho:
  en QEMU sin KVM, la primera escritura en una página del invitado tarda a veces más de 1 ms (la reserva el anfitrión).
- `reloj` apunta con cuánto retraso llega el temporizador de cada núcleo (`retraso_maximo`), para la prueba.
- `make -C kernel arranque-x86` captura la pantalla por el monitor de QEMU (`target/captura-x86.ppm`) y la mira con
  `tools/captura_ventanas.py`, como `arranque-virt`.

Comprobado: `make -C kernel arranque-x86` da BIEN: la pantalla de 1280x800, las dos ventanas de prueba con sus 30
cuadros, Ajustes (el campo recibe las "a", se maximiza a 1280x800 y vuelve), el ratón y el clic llegan a una ventana,
la consola lanzada como proceso y el registro por ella; la captura, el disco y el sonido, bien. `make -C kernel
prueba-x86` da BIEN, con dos pruebas nuevas. Puertos: un proceso con la llave de COM1 escribe y relee el registro de
prueba de la UART y muere (#GP) al leer 0x3F7; después, en el mismo núcleo, uno sin llave muere al tocar COM1; una llave
que se sale del mapa no se puede pedir. Llamada larga: un proceso pide y suelta 16 veces 4 MB en el núcleo 3, con un hilo
de plazo de 1 ms en el mismo núcleo, y el temporizador llega como mucho unos 1,1 ms tarde. Sin las pausas llegaba de 14 a
16 ms tarde; con pausas cada 8 páginas, 6 a 7 ms; con un proceso que solo gira, 80 µs. Con KVM, las 16 veces tardan 3 ms
en total. `make -C kernel prueba` y `make -C kernel arranque-virt` (ARM) dan BIEN.

No comprobado: un PC de verdad (otro GOP, otros formatos de píxel); la
ventana de interrupciones de ARM medida (la prueba es de x86; en ARM solo se comprobó que todo sigue pasando);
`puertos_permitir` con un proceso de varios hilos en varios núcleos (los otros hilos ven los puertos en su próximo
cambio). Captura de la pantalla de x86 en `kernel/target/captura-x86.ppm`.

### 2026-11-14 22:3x — Pausa por la ventana de 5 horas

El usuario pidió bajar el consumo: la ventana de 5 horas iba por el 95 % (semanal, 50 %). Se paró el agente de X7a a
medias. Su trabajo queda sin commit: en esta rama, cambios en `kernel/src/base/x86_64/espacio.rs`,
`kernel/src/llamadas.rs` y `usuario/` (Cargo.toml, build.rs, lib.rs); en RelevoJIT, rama `puertos`, la capa de
puertos (`include/relevo_port.h`, `src/port/`, `tests/port_libc.c`, `tests/runbin.c`) y cambios en `Makefile`,
`src/fpx_trans.c` y `src/interp/ops_fpu.c`. Sin probar. Se retoma con un solo agente cuando la ventana se renueve.
RelevoJIT: R7 fusionado (JIT 15 a 30 veces el intérprete en enteros).


### 2026-11-15 — Fusión de `revision` en `x86`

Lo pidió la revisión de arquitectura (S31, hallazgo 1) y lo decidió el usuario. Se fusionó `revision` en su commit
f9878b8 (633 commits: la caja Amiga, los hilos de usuario, las máquinas virtuales, el permiso de ejecutar, los cupos
como topes, el robo de trabajo entre núcleos, el escritorio entero). `revision` siguió avanzando después: eso queda
para la siguiente fusión. 19 ficheros con conflictos.

Cómo quedó lo común:

- **Planificador** (`base/planificador.rs`): el de `revision`, con lo de cada arquitectura detrás de `cfg` como antes.
  Nuevo en x86: el hilo que corre se apunta en la zona del núcleo (`gs:[24]`, lo que en ARM es TPIDR_EL1); los avisos
  entre núcleos van todos por `avisar_nucleo`; `EstadoFp` tiene `nuevo`, `guardar` y `cargar` en las dos; `punto_de_corte`
  usa `cerrojo::ventana_irq` en x86; `replanificar_si_toca` tras cada `syscall`. El arreglo de X4 (soltar el proceso antes
  de apuntar su fin) se conserva sobre el `terminar_actual` de `revision`, en las dos arquitecturas.
- **Memoria**: `pagina_usuario` y `nueva_en_llamada` de `revision`. Las pausas de X6 pasan a ser sus puntos de corte:
  cada 64 páginas en ARM (como en `revision`) y cada página en x86 (la medida de X6 sigue pasando). `iniciar_tramos`
  pone los contadores nuevos (aviso de memoria baja).
- **Espacio de x86** (`base/x86_64/espacio.rs`): `Acceso` (leer, escribir o ejecutar; el JIT escribe por una proyección
  y ejecuta por otra), `para_copiar`, `ejecutable`, `palabra` (la espera de los hilos), la página 0 y las vistas.
- **Llamadas**: `puertos_permitir` pasa de la 29 a la **56** (la 29 es `medir` en `revision`), y el tipo de llave de
  arranque "puertos de E/S", del 7 al 12 (el 7 es el permiso de plazo). `hilo_crear` en x86: entrada sin alinear, pila
  como tras un `call`, y cuesta 24 KB (la zona de XSAVE mide 4 KB).
- **`usuario/`**: guion de enlace propio para x86 (`enlace_x86.ld`; con la GOT en `.data`, el `hilos` de ARM no
  cargaba). `llamada3` en x86 para las llamadas con tres resultados. Lo que tenía ensamblador de ARM y se compila en
  x86 (`archivos`, `hijo`, `malo`, `hilos`, `xhci_comun`, `nvme`) va por `dashos::contador`, `frecuencia`, `barrera` o
  con `cfg`.
- **`inicio` en x86**: el sonido y su controlador reciben el permiso de plazo, como en ARM.
- **Pruebas**: `prueba_plazo` (C14) es la de `revision`, en `pruebas_hilos.rs`. En x86, nuevas: C52, hilos de usuario,
  con el mismo programa que ARM. Cambiadas por el ocioso sin tic y el robo de trabajo: el temporizador del APIC y C28
  ocupan los demás núcleos con hilos del kernel. C43 cuenta las tablas de pilas, como en ARM. `arranque-x86` usa un
  guion de teclas corto (`TECLAS=simple`) y comprobaciones básicas del disco y de la captura
  (`--comprobar-basico`, `--sin-barra`): las de `arranque-virt` piden el escritorio entero.

Los dos cambios pequeños del usuario:

- **PCID apagado de serie** en x86. Se enciende al compilar: `make -C kernel arranque-x86 PCID=1` (opción `pcid`).
- **Documentos renumerados**: `S21-x86.md` es `S30-x86.md` y `S22-revision-arquitectura.md` es
  `S31-revision-arquitectura.md` (en `revision` ya había hasta S29). Referencias al día en el código y los documentos.

Solo para ARM por ahora (en x86 no existe o está cerrado con `cfg`):

- Máquinas virtuales (`maquinas.rs`, `base/hiper.rs`, EL2): en x86 las llamadas 45 a 48 y la 55 dan `E_NO_HAY`.
- La caja Amiga (`caja/`, Emu68, proceso big-endian) y el orden de bytes por proceso. Es X7.
- La caja Linux, `sesiones`, `guardian`, la barra, el centro, el explorador, la red: `inicio_x86.rs` no los lanza.
- Apagar (llamada 51): en x86 solo reinicia. La hora (39) da 0. El azar no tiene semilla del firmware.
- Los aparatos de las placas: `bcm2836`, `rp1`, `pcie`, `dtb`, `gic`, `its`, el diagnóstico de la Pi 5, la ramfb.

X7a (el WIP): se conservan la opción `relevo` de `usuario/Cargo.toml` y lo de `usuario/build.rs`. Fuera: el `[[bin]]`
`relevo` (su fichero no existía en la rama) y `proyectar_memoria_ejecutable` / `memoria_proyectar_ejecutable`, que
sustituye lo de `revision` (permiso 4 de `memoria_proyectar`, `memoria_proyectar_codigo`), ya probado en x86 (`malo`).

Emu68 en este worktree: `third_party/Emu68` clonado del clon local (af63580) con los 14 parches aplicados, para que
`make -C kernel` compile la caja de ARM.

Un tropiezo que conviene saber: `reloj::programar` leía el contador una vez más en cada decisión del planificador (la
medida del retraso del temporizador, de X6). Con eso, `make arranque-virt` fallaba siempre en el mismo sitio (tres
líneas del arrastre del explorador): `test_trap_divzero` de la caja Amiga ya no llegaba a enseñar su ventana "Cero"
antes de morir, la cascada de ventanas se corría un sitio y el guion soltaba el fichero en la ventana que no era. Es
una carrera de microsegundos entre el programa 68k y el compositor, que el guion no aguanta (las bitácoras de
`revision` ya la apuntan como fallo suelto). La medida va ahora solo en x86 y el camino de ARM queda como en
`revision`. Con `revision` sola (f9878b8, en una copia aparte), `arranque-virt` pasaba.

Comprobado (QEMU, en la máquina de desarrollo, con otras QEMU de otras sesiones a la vez):

- ARM: `make -C kernel prueba` BIEN (con las pruebas de PC de Wayland, sonido, amigados, archivos, kit, caja Linux,
  el cargador hunk, el puente y `esperado.txt` en `raspi4b`). `make -C kernel arranque-virt` BIEN (la salida, el
  reinicio, el disco, la captura, el sonido y el cambio de usuario). `make -C kernel arranque-virt-vm` (la caja
  Linux con EL2) BIEN. `make -C kernel arranque` (la imagen normal en `raspi4b`) falla, y ya fallaba igual en
  `revision` sola (f9878b8): falta "llave 5: memoria del firmware de vc" (`pruebas/arranque.txt` se quedó atrás al
  añadir llaves a `inicio`).
- x86: `make -C kernel prueba-x86` BIEN (todo lo de X2 a X6, más C52 y, dentro de C43, el JIT por dos proyecciones, la
  página 0, las vistas, los topes de los grupos y el aviso de memoria). `make -C kernel arranque-x86` BIEN (la salida,
  el disco con TRIM, la captura y el sonido).

No comprobado: las demás variantes de `revision` (`-2`, `-linux`, `-arch`, `-wayland`, `-gpu`, `-diag`, `-usb`,
`pi5`), que piden imágenes de fuera de git o no tocan nada de lo fusionado a mano; el oráculo (FS-UAE); una Pi; un PC;
`PCID=1` (ni QEMU sin KVM ni el KVM de esta máquina dan PCID). Una vuelta de `prueba-x86` falló en tres pruebas de
tiempos (avisos entre núcleos, despertar un proceso, `memoria_crear`) con la máquina cargada; la siguiente, sin
cambiar nada, pasó.

Queda para después: la caja Amiga en x86 (X7, con RelevoJIT); el reloj del PC y una semilla para el azar (RDRAND);
apagar por ACPI; máquinas virtuales en x86; `sesiones` y el resto del escritorio en `inicio_x86.rs`; mover lo de ARM a
`base/aarch64/` y partir el planificador (S31); y lo que `revision` ha avanzado desde f9878b8.

### 2026-11-15 03:4x — Fusión de `revision` comprobada por el coordinador

`32535a3` (fusiona `revision` en `f9878b8`): `make -C kernel prueba`, `arranque-virt`, `prueba-x86` y `arranque-x86`
dan BIEN en la sesión del coordinador. Siguiente en la línea B: la reescritura de RelevoJIT en Rust
(`~/Projects/RelevoJIT/docs/REESCRITURA.md`, RR0 y RR1), cuando la ventana de 5 horas baje del 80 %.


### 2026-11-15 10:5x — Cierre del bucle (uso semanal al 78 %)

Resumen de la tanda del 14 y 15 de noviembre:

- Línea A (DashOS en x86-64): X1 a X6 hechas (arranque UEFI, memoria, núcleos, procesos, PCIe con xHCI, NVMe y
  virtio-sound, escritorio en QEMU `q35`). `revision` fusionada en `x86` (f9878b8) y comprobada. Rama subida.
  X7a a medias (commit WIP en `x86` y rama `puertos` de RelevoJIT). Pendientes: X5b (IOMMU, la primera), X5c, X7, X8
  y X9 (pegaso y shiva).
- Línea B (RelevoJIT): intérprete 68000 a 68040 con FPU y JIT x86-64 (R0 a R7), revisión (`REVISION.md`, V1 hecha),
  y reescritura en Rust: RR0 a RR3 hechas, `main` ya es la versión en Rust (a0c3c65). JIT en Rust: aritmética
  ~6000 MIPS, copia ~1650-1750, saltos ~3100. Pendiente: RR4 (retirar `c/`), V2 a V9.
- Decisiones del usuario: nombre público futuro UsirOS (DashOS mientras tanto), MMU y 68060 dentro, RelevoJIT en
  Rust y como motor único a largo plazo, interfaz de motor en la caja, fork `FreeMEM/Emu68` (rama `dashos`).

Se retoma tras la renovación semanal (lunes 2026-11-16 por la noche), con un solo agente a la vez: X5b (IOMMU) o
RR4, y la interfaz de motor de la caja.


### 2026-11-15 — X5b: IOMMU (VT-d y AMD-Vi)

Hecho (sala limpia: las especificaciones de Intel VT-d y de AMD n.º 48882; el código de QEMU solo se miró para
entender lo que emula):

- `acpi.rs` lee la DMAR (unidades DRHD con sus ámbitos, reservas RMRR) y la IVRS (bloques IVHD, uno por IOMMU, con sus
  entradas de aparatos; IVMD con proyección 1:1).
- `base/x86_64/iommu.rs`, lo que toca el hardware. VT-d en modo clásico: tabla raíz, tablas de contexto por bus,
  tablas de segundo nivel de 4 niveles (3 si no hay), invalidación por registros (contextos e IOTLB por dominio),
  `clflush` de cada entrada si la unidad no es coherente, el búfer de escrituras si lo pide, y apaga lo que dejara
  encendido el firmware (traducción, cola de invalidaciones, remapeo, regiones protegidas). AMD-Vi: tabla de aparatos
  de 2 MB con todos bloqueados (traducción con una tabla vacía), cola de órdenes (INVALIDATE_DEVTAB_ENTRY,
  INVALIDATE_IOMMU_PAGES, INVALIDATE_ALL y COMPLETION_WAIT con escritura en memoria) y registro de eventos.
- Los fallos llegan por el vector 0xF3 del núcleo 0 (VT-d por sus registros FEDATA y FEADDR; AMD-Vi por la MSI de su
  función del PCI). Se cuentan, se guardan los 16 últimos y se escriben en la consola (aparato, dirección, lectura o
  escritura, motivo); los seguidos iguales, una vez. El kernel sigue.
- `kernel/src/iommu.rs`: qué unidad lleva cada aparato (ámbitos resueltos por los puentes del PCIe, rangos de la IVRS,
  la de "todos"), las reservas del firmware 1:1 en el dominio de su aparato, y `dominio_para`.
- Un dominio por aparato (número del PCI). La dirección de E/S es la física: los servicios no cambian. El
  `PermisoDma` lleva el dominio: `memoria_dma` proyecta en él lo que da, la cuarentena sigue proyectada (el aparato
  puede seguir escribiendo) y `dma_liberar` la quita del dominio, con la IOTLB invalidada, antes de devolverla.
  `inicio_x86.rs` da a xHCI, NVMe y virtio-sound su permiso con dominio. `kmain` enumera el PCIe una vez y enciende las
  IOMMU antes de las pruebas y de `inicio`.
- `virtio-sonido` acepta `VIRTIO_F_ACCESS_PLATFORM` si el aparato la ofrece (en ARM no la ofrece: nada cambia).
- Makefile: `IOMMU=intel` (`-device intel-iommu`) e `IOMMU=amd` (`-device amd-iommu,dma-remap=on`; sin `dma-remap`
  la AMD-Vi de QEMU no traduce) en `prueba-x86` y `arranque-x86`. Con IOMMU, virtio-sound va con `iommu_platform=on`,
  `prueba-x86` añade un NVMe sin disco y los esperados suman `pruebas/*_iommu.txt`. `arranque-x86` con IOMMU falla si
  sale algún fallo de DMA.
- Prueba nueva en `pruebas_x86.rs` (X5b): el kernel hace de controlador del NVMe con colas en memoria DMA de un
  permiso con dominio y le pide "Identify" a cuatro sitios: en la ventana (escribe), en otra ventana (escribe), en esa
  misma tras soltarla y liberar su cuarentena (falla: comprueba la invalidación de la IOTLB) y en una página del kernel
  (falla, y la página queda intacta). Los dos fallos llegan por la interrupción, con el aparato y la dirección. Además,
  ningún fallo de DMA desde el arranque hasta la prueba.

Comprobado (QEMU 11.1.3, q35, TCG): `make -C kernel prueba-x86 IOMMU=intel` y `IOMMU=amd` BIEN; `make -C kernel
arranque-x86 IOMMU=intel` y `IOMMU=amd` BIEN (salida, disco, captura y sonido, los tres servicios en su dominio y ni un
fallo de DMA); `make -C kernel prueba-x86` y `arranque-x86` sin IOMMU BIEN ("IOMMU: no hay"); ARM: `make -C kernel
prueba` y `arranque-virt` BIEN.

Cosas de QEMU que conviene saber: el NVMe contesta con estado 0 aunque su DMA haya fallado (la prueba no mira el
estado); la AMD-Vi deja a 0 la dirección de sus eventos (una máscara de `<< 64` en su código) y pone sus propios bits
de permiso en las banderas, así que con AMD la prueba acepta dirección 0 y no mira si era escritura; y da decenas de
eventos por orden (se escriben una vez).

No comprobado: una máquina de verdad (pegaso con VT-d, shiva con AMD-Vi); RMRR e IVMD (QEMU no los da); varias
unidades; aparatos detrás de puentes; KVM. Lo que queda abierto está en la tarea X5b (remapeo de interrupciones,
alias, grupos, modo escalable).


### 2026-11-15 — X10: hipervisor de x86 (AMD-V con NPT y VT-x con EPT)

Hecho (sala limpia: el AMD64 APM vol. 2, cap. 15 y apéndice C, y el Intel SDM vol. 3C, caps. 24 a 29 y apéndice B;
ni KVM, ni Xen, ni QEMU):

- `base/x86_64/hiper.rs` y `hiper.s`, lo de la CPU, con la misma forma que `base/hiper.rs` de ARM: un `Espacio` (tablas
  NPT o EPT de 4 niveles sobre objetos de memoria, hasta 512 GB del invitado) y `Vcpu` (el VMCB o el VMCS, los
  registros, la FPU con XSAVE y el XCR0 del invitado). Al arrancar se mira qué hay (AMD-V con NPT; VT-x con EPT,
  invitado sin restricciones y las MSR "TRUE"; VPID si hay INVVPID) y se escribe en la consola; cada núcleo lo enciende
  la primera vez que corre una vCPU (EFER.SVME y la zona del anfitrión; CR4.VMXE y VMXON).
- AMD: VMSAVE/VMLOAD del anfitrión y del invitado alrededor de VMRUN, con GIF a 0 y el IF del anfitrión a 1 durante
  VMRUN (V_INTR_MASKING: una interrupción física saca al invitado). Interrupciones al invitado por V_IRQ (la CPU la da
  en cuanto puede, sin salir). Intel: el VMCS solo está cargado mientras se corre (VMCLEAR al volver: el hilo puede
  cambiar de núcleo), el estado del anfitrión escrito en cada llamada, las MSR de `syscall` cambiadas a mano, y la GDT
  y la TSS cargadas otra vez tras salir (VT-x deja la TSS con límite 0x67, sin mapa de puertos). Interrupciones por la
  información de entrada y la salida por ventana de interrupciones. En los dos, lo que la CPU estaba entregando al
  salir se reinyecta.
- La TLB: ASID (AMD) o VPID (Intel) por núcleo, uno nuevo al cambiar de núcleo o de generación; al acabarse, generación
  nueva y todo fuera. En Intel, cada VPID nuevo hace INVEPT de su EPT.
- El kernel resuelve sin salir CPUID (sin lo que no se virtualiza, con el bit de hipervisor y "DashOSDashOS"), EFER,
  PAT, IA32_TSC_DEADLINE, XSETBV, CR0/CR4/CR8 en Intel; las MSR que cambian de mundo pasan sin salir.
- `kernel/src/maquinas_x86.rs`: la misma interfaz que `maquinas.rs` (llamadas 45 a 48, mismos informes y bloques), con
  lo que cambia documentado en `docs/referencia/kernel.md` (sección x86-64): HVC = VMMCALL/VMCALL, LECTURA y ESCRITURA
  con la instrucción decodificada por el kernel (leída de la memoria del invitado recorriendo sus tablas: sin
  paginación, 32 bits, PAE y 4 niveles; o de la CPU en AMD con ayuda), motivo nuevo 8 (PUERTO), REGISTRO = MSR, WFI =
  HLT, el temporizador del invitado (el plazo del TSC) llevado por el kernel como el virtual de ARM (adelanta el tic
  del núcleo para sacar al invitado a tiempo), y huecos de interrupciones en vez de registros de lista. `maquina_estado`
  5 lleva el modo (protegido de 32 bits o real, para las SIPI). `llamadas.rs` ya no separa las llamadas de máquinas
  por arquitectura.
- Para X11: `Espacio::por_cada_pagina` da cada página como (física del invitado, física), para proyectarla en el
  dominio de la IOMMU del aparato con la misma dirección.
- Makefile: `ACCEL=kvm` (con `-accel kvm -cpu host`) en `prueba-x86` y `arranque-x86`; `tools/prueba_uefi.sh` con
  `ACCEL_X86` y `CPU_X86`.
- Prueba (`usuario/src/bin/maquina.rs`, lanzado por `prueba_maquinas` en `pruebas_procesos_x86.rs`): un invitado
  propio que empieza en modo protegido y comprueba cada salida desde el anfitrión: OUT de 1, 2 y 4 bytes e IN; accesos
  a aparatos (MOV de inmediato, MOV a registro, con dirección absoluta, MOVZX, a AH, de 16 bits, y uno que no contesta
  nadie con `maquina_aborto`); CPUID; la hipercall con su resultado puesto con `maquina_poner_registros`; WRMSR y RDMSR
  que van al servicio y EFER del kernel; GDT e IDT propias, una interrupción entregada a un HLT; el temporizador (el
  plazo del TSC, esperado en el kernel con `ESPERAR_WFI`); paso a modo largo (accesos de 8 bytes con REX por una
  dirección alta de sus tablas); y una segunda vCPU en modo real; luego parar la máquina.

Comprobado: `make -C kernel prueba-x86` BIEN (QEMU 11.1.3, TCG `-cpu max`: AMD-V con NPT, sin NRIPS ni ayuda para
decodificar: se usa el recorrido de tablas), también con `IOMMU=intel` y `IOMMU=amd`. `make -C kernel prueba-x86
ACCEL=kvm` en shiva (Ryzen 9 9955HX, `kvm_amd nested=1`): X10 bien con AMD-V anidado de verdad; falla solo C28, como ya
fallaba con KVM antes (bitácora de X4: los programas acaban en 4 tics). `make -C kernel arranque-x86` BIEN, con
`IOMMU=intel` y `IOMMU=amd`. ARM: `make -C kernel prueba`, `make -C kernel arranque-virt` y `make -C kernel arranque-virt-vm` (el hipervisor de
ARM, por las llamadas de máquinas, que ahora son comunes) BIEN.

No comprobado: **VT-x**. pegaso (i5-8500, `kvm_intel nested=Y`, QEMU 10.2.1) tiene la imagen y el guion en
`~/dashos-pruebas-x10/`, pero `freemem` no puede abrir `/dev/kvm` allí (no está en el grupo `kvm`; el ACL de la consola
es de `gdm-greeter`) y `sudo` pide contraseña: no se tocó su configuración. Para probarlo, con acceso a `/dev/kvm`:
`cd ~/dashos-pruebas-x10 && ARQ=x86_64 ACCEL_X86=kvm CPU_X86=host OVMF=/usr/share/OVMF/OVMF_CODE_4M.fd
OVMF_VARS=/usr/share/OVMF/OVMF_VARS_4M.fd ./prueba_uefi.sh dashos-uefi.efi esperados_x86.txt 150` (C28 fallará con
KVM, como en shiva). Tampoco: una máquina de verdad sin anidar; la ayuda para decodificar de AMD (ni TCG ni este KVM la
dan); varias vCPU corriendo a la vez en núcleos distintos (la prueba corre una y luego la otra); el globo. Lo que queda
está en la tarea X10.

