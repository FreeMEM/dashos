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
- [x] X7a **RelevoJIT dentro de DashOS x86, sin caja Amiga.** El crate `relevo` (Rust, sin std) en un proceso de
  DashOS (`usuario/src/bin/relevo.rs`, característica "relevo"), su memoria de código sobre un objeto de memoria de
  DashOS (doble proyección RW y RX), y los programas de `jit-host/pruebas68k` con el intérprete y con el JIT, con la
  misma salida que en Linux. El cargador avisa con `relevo_invalidate`. En `make -C kernel prueba-x86`. La capa de
  puertos de la versión en C (rama `puertos` de RelevoJIT y el commit WIP c1cfa51) quedó obsoleta. Lo que queda
  abierto:
  - La dependencia va por git con `rev` fijo: hasta que la rama `x7a` de RelevoJIT esté en GitHub, solo compila
    `usuario/` donde Cargo ya tenga ese commit (`~/.cargo/git`).
  - Una sola CPU con JIT viva a la vez en el programa (direcciones fijas de la memoria de código).
  - La copia del banco (`copy`) va un 20 % más lenta con KVM que en Linux (1315 frente a 1699 MIPS); lo demás, igual.
    Sin mirar por qué (¿páginas de 4 KB y la paginación anidada?).
- [x] X5b **IOMMU** (VT-d de Intel y AMD-Vi): tablas DMAR e IVRS de ACPI, un dominio por aparato, el DMA de cada
  servicio limitado a sus ventanas, y el aviso de fallos de DMA. Requisito para pasar aparatos a la caja Linux y
  para el dominio de controladores de la zona GPL (S32 y S33 de `revision`). En QEMU: `-device intel-iommu` y
  `-device amd-iommu`. Prioridad de la línea A. Lo que queda abierto:
  - Remapeo de interrupciones: el de VT-d, hecho en X11b (todas las interrupciones del anfitrión, en formato
    remapeable y con el origen comprobado). AMD-Vi solo lo usa para los aparatos pasados a una máquina (X11): las MSI de
    los servicios van sin remapeo (IV = 0). Núcleos con ID de APIC de más de 8 bits: la tabla de VT-d ya los admite
    con EIME (formato de x2APIC), pero `interrupciones::msi` sigue dando las MSI al núcleo 0 con un destino de 8 bits.
  - Alias: un aparato detrás de un puente de PCIe a PCI hace DMA con el número del puente; no recibe dominio (su DMA
    falla). Tampoco los grupos de funciones sin ACS. Solo el segmento 0 del PCI.
  - Sin probar en una máquina de verdad (pegaso, VT-d; shiva, AMD-Vi), ni con RMRR o IVMD (QEMU no los da): lo de la
    pantalla de la gráfica integrada de Intel depende de las RMRR.
  - VT-d solo en modo clásico (tabla raíz y de contexto, segundo nivel), sin el modo escalable. No se mira si la
    unidad tiene el clásico: en una que solo tuviera el escalable, no funcionaría.
- [ ] X5c **Aparatos de un PC de verdad**: AHCI (SATA), HDA (sonido), y la red de cada placa o, mientras, la del
  dominio de controladores de la zona GPL; y compartir internet del móvil por USB (CDC-NCM y RNDIS). **A medias**
  (2026-11-16): hechos el AHCI (`ahci`, varios puertos, por el canal de los discos con `dashos::disco_servidor`), el HDA
  (`hda`, al servicio de mezcla) y la red de virtio en x86 (`virtio-red` con MSI-X y la pila de `red`), probados en
  QEMU sin IOMMU, con VT-d y con AMD-Vi. Lo que queda:
  - La tarjeta de red de cada placa: Realtek RTL8125 (shiva) e Intel (pegaso). La de Intel, con controlador propio
    (`intel-red`, 2026-11-16: el I219 de la familia "Cannon Point" y el 82574L), probado en QEMU con el 82574L
    (`RED_X86=e1000e`). **Sin probar en hardware** todo lo propio del I219: el semáforo con el ME, la salida del reposo
    ULP y del modo SMBus, el ciclo LANPHYPC, el acceso al PHY por sus páginas, la NVM en la flash y la configuración
    del PHY desde ella, los bits OEM, el reinicio global con sus lecturas de configuración, el vaciado de los anillos
    (errata del PCH), los ajustes al subir el enlace y la MSI. La RTL8125B con controlador propio (`rtl-red`,
    2026-11-16) y el intérprete de su firmware, probado solo con ficheros sintéticos; el controlador, **sin probar en
    hardware** (QEMU no la emula): la prueba es pasando la tarjeta de shiva a QEMU con VFIO
    (`make -C kernel arranque-x86 RED_X86=vfio PCI_RED=0000:03:00.0 ACCEL=kvm`, los comandos en `tools/vfio_red.sh`).
  - Compartir internet del móvil por USB (CDC-NCM y RNDIS): sin empezar. Necesita que el xHCI dé aparatos que no son
    HID a otros servicios.
  - AHCI: sin NCQ, multiplicadores de puertos, ATAPI, conexión en marcha ni ahorro de energía del enlace. Con un disco
    SATA y sin NVMe, el AHCI sirve "disco" y `archivos` va por él (`make -C kernel sata-x86`).
  - HDA: solo la salida, sin volumen por el amplificador, sin avisos de clavija, sin HDMI ni arreglos por códec.
  - Nada de esto probado en una máquina de verdad. `disco` (la prueba en bruto del NVMe, que escribe el sector 2) se
    lanza siempre en x86; desde X5c solo escribe si los sectores 0 y 1 tienen los textos del disco de prueba, como
    `disco-sata` (en un PC de verdad solo lee).
- [ ] X10 a X17 **La gráfica compartida** (`docs/diseño/S34-grafica-compartida.md`): X10 hipervisor de x86 (VT-x con
  EPT, AMD-V con NPT); X11 pasar un aparato PCIe a una máquina virtual; X12 el dominio de gráficos; X13 canal y
  memoria compartida sin copia; X14 Mesa en DashOS (Venus y contexto nativo); X15 el compositor sobre el dominio;
  X16 CUDA en la caja por reenvío de la interfaz de Nvidia, y el dominio de cómputo; X17 vídeo por hardware. Detrás de X5b.
  - [x] X10 **Hipervisor de x86**: AMD-V con NPT (TCG y KVM en shiva) y VT-x con EPT (KVM anidado en pegaso, probado
    por el coordinador el 2026-11-16), hechos y probados. Lo que queda abierto:
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
  - [x] X11 **Pasar un aparato PCIe a una máquina virtual** (`kernel/src/pasar.rs`, llamada 60 `maquina_aparato`):
    asignar (FLR si lo admite; su dominio de la IOMMU con la memoria del invitado, dirección de DMA = física del
    invitado; su MSI la programa el kernel y llega al monitor como interrupción de aparato), proyectar sus BAR (sin las
    páginas de MSI-X), su configuración filtrada, y al parar la máquina o morir el monitor, callado, con el dominio vacío
    y libre. Sin IOMMU se niega. Remapeo de interrupciones con AMD-Vi (una tabla de una entrada por aparato). Probado en
    QEMU con TCG (AMD-V) y AMD-Vi, VT-d y sin IOMMU. Lo que queda abierto:
    - ~~VT-d sin remapeo de interrupciones~~: hecho en X11b. Solo una VT-d sin remapeo (sin ECAP.IR, o la DMAR sin
      INTR_REMAP) sigue así: la llamada lo dice (bandera 1 a 0) y el kernel lo avisa en la consola.
    - Solo la entrada 0 de la MSI-X (y un mensaje de MSI): un aparato con varias colas (una gráfica, un NVMe) tendrá una
      sola interrupción. Hará falta una por vector, con su entrada en la tabla de remapeo.
    - Las páginas de la tabla y la PBA de MSI-X no se proyectan: si comparten página con otros registros del aparato,
      el monitor tendría que reenviarlos (no hay operación para eso aún). Tampoco se pasan las BAR de E/S ni las de menos
      de 4 KB, ni la ROM; y una BAR se proyecta una vez (si el invitado la mueve, no se sigue).
    - La configuración se escribe con una lista blanca corta (mando, estado, línea de caché, latencia, línea de
      interrupción, Device Control de PCI Express). Lo propio del fabricante o las demás capacidades (energía, PCIe Link
      Control) dan `E_DERECHOS`: puede que algún controlador las necesite (las gráficas, en X12).
    - FLR escrito y sin probar (el `edu` de QEMU no lo tiene; está detrás de la capacidad de PCI Express). Sin otros
      reinicios (el del bus secundario, el de energía D3).
    - Un aparato con RMRR o IVMD (memoria del firmware 1:1 en su dominio, como la gráfica integrada de Intel): al
      asignarlo, esa proyección se pierde (el dominio se vacía). Para X12 habrá que decidir qué hacer con ella.
    - `inicio_x86.rs` no crea objetos de aparato ni lanza un monitor de máquinas: solo las pruebas. Faltan los
      aparatos detrás de puentes de PCIe a PCI y los grupos sin ACS (lo mismo que en X5b): uno de esos no se debería
      pasar solo.
    - Sin interrupciones entregadas sin salir (posted interrupts, AVIC/APICv con la IOMMU): cada MSI pasa por el monitor.
    - En QEMU, el DMA de un aparato a 0xFEE0_0000 llega a la AMD-Vi sin número de aparato y la toma como del IOAPIC
      (sin remapeo): una "MSI falsa" por DMA sí llega en QEMU. En una AMD-Vi de verdad todo lo que sube lleva el número
      del aparato. La prueba del remapeo cambia en su lugar el dato de la MSI del aparato.
    - Con KVM y AMD-Vi, QEMU pide `kernel-irqchip=split` para `intremap=on` (el Makefile lo pone); sin probar con KVM.
  - [x] X11b **Remapeo de interrupciones de VT-d** (`base/x86_64/iommu.rs`, `iommu.rs`, `ioapic.rs`): la cola de
    invalidaciones, una tabla de remapeo para todas las unidades, y todas las interrupciones del anfitrión (MSI y MSI-X
    de los servicios, IOAPIC, el aparato pasado a una máquina) en formato remapeable, cada una con su entrada y el
    origen comprobado (SVT = 01 con su número de PCI); el formato compatible, bloqueado (CFI a 0). El aparato pasado
    tiene una sola entrada, a su vector de siempre; al soltarlo, se quita. Sin IR (unidad o DMAR), como antes. Probado
    en QEMU (TCG) y en pegaso (KVM anidado). Lo que queda abierto:
    - El bloqueo del formato compatible no se puede probar en QEMU: su VT-d deja pasar esos mensajes con el remapeo
      encendido (no mira CFI). Sin probar en una VT-d de verdad.
    - Una tabla de 256 entradas para todas las unidades. Una entrada por fuente (aparato y vector, o IOAPIC y GSI): si
      el vector de una MSI cambiara, la vieja se quedaría (hoy no pasa: cada aparato tiene un vector fijo).
    - Los destinos de más de 8 bits (EIME): la entrada y el IOAPIC ya los llevan, pero las MSI siguen yendo al núcleo
      0 con el destino de 8 bits de `interrupciones::msi`.
    - Los HPET (ámbito de tipo 4) no se apuntan: el kernel no usa sus interrupciones. Si se usan, irán como los IOAPIC.
    - Una sola unidad en QEMU: lo de varias unidades (unas con remapeo y otras sin él) está escrito y sin probar.
    - Sin interrupciones entregadas sin salir (posted interrupts, IM = 1 en la entrada).
  - [x] X12 **El dominio de gráficos con una GPU de QEMU** (G5 de S38, 2026-11-18): un Linux mínimo en una máquina de
    DashOS recibe la `virtio-gpu-gl-pci` (virgl) pasada con la IOMMU y pinta por KMS con Mesa; `make -C kernel
    graficos-x86` (KVM, IOMMU=intel o amd). X11 ampliada a varias MSI-X. Lo que queda abierto:
    - Solo la GPU de QEMU. Una Intel o AMD de verdad (G8) pedirá: la tabla de MSI-X en la página de otros registros
      (hoy no se proyecta esa página y el monitor no reenvía), las RMRR de la integrada de Intel, BAR de VRAM por
      encima de 4 GB (el monitor las pone por debajo), la ROM, la VGA heredada y más escrituras de configuración
      (energía, Link Control).
    - El dominio solo pinta un plasma y un cuadrado; el escritorio de DashOS compuesto allí espera al protocolo y al
      servidor de composición (G3 y G4, de otra sesión) y a la memoria compartida sin copia (X13).
    - El monitor emula el APIC: cada interrupción de la GPU, cada EOI y cada salida pasan por él (9 salidas y 3
      interrupciones por cuadro). Sin AVIC/APICv ni interrupciones entregadas sin salir.
    - Como mucho 8 interrupciones por aparato; sin MSI-X compartida con otros registros ni la entrada 0 de una MSI
      que pida varios mensajes.
  - [x] X13 **Memoria compartida sin copia con el dominio** (G6 de S38, 2026-11-22): la ventana compartida
    (`dashos::protocolo::compartida`). Un programa de DashOS da la memoria de una ventana al servicio `dominio-graficos`
    por el canal "graficos"; el servicio la proyecta tal cual en la física del invitado (`maquina_memoria`, que ya
    existía) y el dominio la lee por /dev/mem y la sube a la GPU. Una página de control con 16 ranuras (secuencia lista
    y vista), y un timbre (un OUT al puerto 0x5F0) para avisar sin que nadie mire la memoria en bucle. Probado en
    `make -C kernel graficos-x86` con `graficos-cliente` (300 cuadros de 640x480). Lo que queda abierto:
    - Quitar un búfer: no hay forma de sacar memoria de una máquina en marcha; las ranuras y su sitio no se reusan
      (16 y 256 MB en total).
    - El dominio ve la memoria también para escribir (`maquina_memoria` la pide con escribir): proyectarla solo para
      leer en la EPT/NPT.
    - La subida a la GPU sigue copiando dentro del dominio (glTexSubImage2D con virgl: 0,24 ms por cuadro de 640x480).
      Importar la memoria como textura sin copia (dmabuf) es la otra mitad de G6, con una GPU de verdad.
    - GLES 2 pide paso = ancho × 4; el compositor aún no la usa (G3 y G4: el protocolo de escena y el servidor).
    - El aviso al dominio de un cuadro nuevo va por la memoria (el dominio la mira cada 200 µs mientras espera): una
      interrupción hacia el invitado lo quitaría.
- [ ] X7 **Caja Amiga en x86** con RelevoJIT. En cuatro pasos (2026-11-22, alcance aprobado por jefe-dash; S31,
  secciones 2 y 3):
  - [x] 1. RelevoJIT, lo que pide la caja (rama `motor-caja` de RelevoJIT, f74486e): entrada anidada desde las
    funciones de línea A y F, base 0 (`RELEVO_OPT_BASE_ZERO`), `relevo_invalidate_all` y la carrera del aviso.
  - [x] 2. La interfaz de motor en `caja/` (`caja/motor.h`): Emu68 detrás de ella (`caja/motor_emu68.c`), sin
    cambiar lo que hace en ARM. Las funciones nativas de Rust reciben `motor_regs` (`Registros`, `#[repr(C)]`).
  - [ ] 3. La caja para x86: `motor_relevo`, la plataforma (llamadas al sistema, reloj, orden de bytes de
    `lee32`/`pon32`), el pegamento para `x86_64-unknown-none` y su Makefile.
  - [ ] 4. La caja en `x86-uso` y en `arranque-x86`, con un programa Amiga que abra su ventana; medir contra Emu68.
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


### 2026-11-15 — X7a: RelevoJIT dentro de un proceso de DashOS x86

Hecho con el crate de Rust de RelevoJIT (rama `x7a` de su repositorio, commits ac26a32 y 42fe7e6), no con la capa de
puertos de la versión en C (rama `puertos` y el WIP c1cfa51, obsoletos: no se fusionan).

- RelevoJIT: `relevo_config.code_memory`, una tabla de funciones con la que el anfitrión da la memoria de código del
  JIT en dos vistas del mismo objeto (escribir y ejecutar) y recibe el aviso de lo escrito. Sin ella, la del sistema
  (memfd en Linux), como antes; sin `std`, el JIT necesita esta. `Cargo.toml` solo con `rlib` (una `staticlib` sin
  `std` pedía manejador de pánico y asignador, y Cargo la compilaba también como dependencia). `tests/runbin.c`
  ejecuta binarios 68k planos con los dos motores: es la referencia. Detalle en su `docs/BITACORA.md`.
- `usuario/`: la característica "relevo" trae el crate por git con `rev` fijo (ac26a32), sin `std`; el programa
  `relevo` (`usuario/src/bin/relevo.rs`) da la memoria de código con un objeto de 8 MB proyectado para escribir en
  384 GB y para ejecutar en 388 GB (`memoria_proyectar_codigo`, permiso 4), y tras cada escritura un CPUID. Ejecuta
  los 7 programas de `jit-host/pruebas68k` con el intérprete y con el JIT (una CPU por motor; tras copiar cada
  programa, `relevo_invalidate`) y compara las 14 líneas con las de Linux (`jit-host/pruebas68k/ref/relevo.txt`, de
  `build/runbin` de RelevoJIT). Luego el banco de RelevoJIT (`arith`, `copy`, `bytes`) con los dos motores. Sale con 70.
- Se quitó lo de `RELEVO_LIB_DIR` de `usuario/build.rs` (la `librelevo.a` sin libc del intento en C).
- `kernel/Makefile`: "relevo" en `FUNCIONES_X86`, `relevo` en `BINES_X86`, y `RELEVO_DIR=<ruta>` para probar un
  RelevoJIT local (un `[patch]` por `--config`; la `rev` tiene que estar en GitHub igual y Cargo.lock cambia mientras).
- Prueba `prueba_relevo` en `pruebas_procesos_x86.rs`, con sus líneas en `pruebas/esperado_x86.txt`.
- `hilos` (C52): esperaba en la dirección de la función `nada` sin alinearla a 4; con opt-level "s" las funciones de
  x86 no se alinean, y al cambiar las características cayó en 0x400bd5 y C52 daba MAL (salía con 10). Se alinea.
- `docs/referencia/kernel.md`: en x86, tras escribir código por la otra proyección, una instrucción que serializa.

MIPS del banco (68020; 12, 4,1 y 20,8 millones de instrucciones), intérprete / JIT:

| | Linux en shiva | DashOS, QEMU con KVM | DashOS, QEMU con TCG |
|---|---|---|---|
| arith | 187 / 5975 | 192 / 5949 | 10 / 511 |
| copy | 141 / 1699 | 137 / 1315 | 11 / 147 |
| bytes | 190 / 3115 | 192 / 3049 | 10 / 198 |

Los 14 programas, con TCG: 0,35 a 0,5 s; con KVM, 31 ms. JIT: 724 bloques, 632 instrucciones al intérprete, 1412
escrituras avisadas, 1,7 MB de código, sin vaciados.

Comprobado: `make -C kernel prueba-x86` BIEN, también con `IOMMU=amd`; `make -C kernel arranque-x86` BIEN (salida,
disco, captura y sonido); con `ACCEL=kvm`, X7a bien y falla solo C28, como ya fallaba con KVM. ARM: `make -C kernel
prueba` y `make -C kernel arranque-virt` BIEN. RelevoJIT: `make test` con gcc y clang, `make sst` y `make sst JIT=1`
(317 500/317 500), `cargo test`, `cargo clippy -D warnings` (con y sin `std`), `cargo fmt --check`.

No comprobado: compilar `usuario/` desde GitHub (la rama `x7a` no está subida: aquí se trajo el commit del repositorio
local con `url.insteadOf` y `CARGO_NET_GIT_FETCH_WITH_CLI`, y Cargo lo guarda en `~/.cargo/git`); `prueba-x86` con
`IOMMU=intel` (no se pidió); el banco de Linux bajo `qemu-x86_64` (la glibc de shiva usa instrucciones que el TCG no
tiene), así que la columna de TCG no tiene contraparte en Linux; una máquina de verdad.


### 2026-11-16 — X10: VT-x probado en pegaso

Lo probó el coordinador: en pegaso (i5-8500, KVM anidado, `-cpu host`), con la imagen de X10 (`~/dashos-pruebas-x10`),
`prueba-x86` dio BIEN: "Hipervisor: VT-x con EPT, invitado sin restricciones y VPID" y "X10: máquinas virtuales bien";
C28 también bien allí. X10 queda hecha.


### 2026-11-16 — X11: pasar un aparato PCIe a una máquina virtual

Hecho (sala limpia: la especificación de PCI Express 6.0, la de la IOMMU de AMD n.º 48882 rev. 3.10 (2.2.2.1, la
entrada de la tabla de aparatos; 2.2.5, el remapeo de interrupciones; 2.4.5, INVALIDATE_INTERRUPT_TABLE), el AMD64 APM
vol. 2 y el Intel SDM 3C para NPT y EPT; el código de QEMU solo para entender qué emula su AMD-Vi y el `edu`):

- `kernel/src/pasar.rs`: el objeto "aparato del PCIe" (`Objeto::AparatoPci`, solo x86). Al crearlo, el kernel apunta
  el aparato como suyo (`reclamar`; `inicio_x86.rs` reclama los de los servicios: un aparato está en un lado o en el
  otro), mide sus BAR, mira dónde están la tabla y la PBA de MSI-X, le reserva una MSI del anfitrión y lo calla.
- Llamada 60 `maquina_aparato` (vCPU, aparato, operación): 0 asignar, 1 proyectar una BAR, 2 información de una BAR, 3 y
  4 leer y escribir la configuración (filtrada). Detalle en `docs/referencia/kernel.md`. Sin IOMMU, `E_NO_HAY`.
- Asignar: FLR si lo admite (100 ms durmiendo y las BAR repuestas); su dominio de la IOMMU vaciado y con toda la memoria
  del invitado (dirección de DMA = física del invitado; `base::iommu::Dominio::proyectar_pagina`, `invalidar` y
  `vaciar`); lo que se añada a la máquina después entra también (con la lista de aparatos tomada, para que no se cuele a
  medias). Su MSI la programa el kernel; la llave de su interrupción va al monitor, que la entrega a la vCPU por el
  bloque de interrupciones de X10.
- Remapeo de interrupciones con AMD-Vi: una tabla de una entrada por aparato pasado (IV = 1, IntCtl = 10b, IntTabLen 0,
  sin pases de NMI, INIT, ExtInt ni LINT); el aparato manda su MSI con el dato 0. Al soltarlo, IntCtl = 00b (todo se
  aborta). Los aparatos de los servicios siguen con IV = 0. Con VT-d no hay remapeo (ver la tarea): la llamada lo dice
  con su bandera y el kernel lo escribe en la consola.
- BAR en el invitado: `hiper::Espacio::proyectar_registros`, sin caché (NPT con PCD y PWT; EPT de tipo UC) y sin
  ejecutar, sin las páginas de MSI-X (salen al monitor). El espacio apunta esos tramos: nada se pone encima.
- Soltar: `Comun::parar` espera (100 ms como mucho) a que las vCPU salgan del invitado y suelta sus aparatos; el `Drop`
  de la máquina, igual (el monitor murió). Callado (sin memoria, E/S ni bus master; MSI y MSI-X apagadas), dominio
  vacío, interrupciones abortadas. Libre para otra máquina; su objeto de interrupción se reutiliza.
- `pci.rs`: accesos de 1, 2 y 4 bytes a la configuración, `callar`, `pedir_flr`, `tipo_cabecera`; `mmio::Ventana`, de 1 y
  2 bytes.
- Biblioteca: `maquina_aparato`, `maquina_aparato_bar`, `_info`, `_leer`, `_escribir`, y `E_DERECHOS`.
- Prueba: `usuario/src/bin/maquina_pci.rs` hace de monitor (emula para el invitado la configuración por 0xCF8 y 0xCFC,
  con su BAR 0 y su MSI) y su invitado mínimo usa el `edu` de QEMU: lee su configuración, pone su BAR y su MSI, lee la
  identificación y el registro de vida por la BAR sin salir, copia por DMA 64 bytes por el aparato dentro de su memoria,
  recibe la interrupción del aparato (vector 0x50) y le manda copiar a 0x0800_0000, fuera de su memoria. El monitor
  comprueba además lo que filtra el kernel (BAR y MSI no se escriben, las BAR se leen 0), que no se asigna dos veces, y
  que al parar la máquina el aparato se asigna a otra. El kernel (`prueba_pasar`, en `pruebas_procesos_x86.rs`)
  comprueba que el DMA de fuera falló en la IOMMU y no tocó esa física del anfitrión, que muerto el monitor el aparato
  está callado, libre y su DMA no alcanza lo que era del invitado, y con AMD-Vi el remapeo: su MSI llega y, con el
  dato de su MSI cambiado a mano al vector del anfitrión (como un mensaje sin remapeo), no llega. `make -C kernel
  prueba-x86` añade `-device edu,dma_mask=0xffffffff` siempre; con `IOMMU=amd`, `intremap=on` (y con KVM,
  `kernel-irqchip=split`).

Una cosa de QEMU: el DMA de un aparato (`pci_dma_write`) llega a su AMD-Vi sin número de aparato, y la tabla de
remapeo que mira es la del IOAPIC (IV = 0): una "MSI falsa" que el invitado le haga escribir por DMA a 0xFEE0_0000 sí
llega en QEMU. Las MSI de verdad sí llevan el número. Por eso la prueba del remapeo cambia el dato de la MSI.

Comprobado: `make -C kernel prueba-x86` BIEN sin IOMMU (la llamada se niega), con `IOMMU=amd` (con remapeo) y con
`IOMMU=intel` (sin remapeo), con TCG (AMD-V). En pegaso (VT-x con EPT, KVM anidado, `-cpu host`, QEMU 10.2.1) con
`intel-iommu`, en `~/dashos-pruebas-x11`: BIEN entero, también X11 y C28.
`make -C kernel arranque-x86 IOMMU=amd` BIEN (salida, disco, captura y sonido; sin fallos de DMA). ARM: `make -C kernel
prueba` BIEN; `make -C kernel arranque-virt` BIEN a la segunda (la primera se quedó en la búsqueda del explorador, con
la tecla "a" repetida, y faltó "no abre Sistema/DeCarla": el paso de la interfaz que a veces falla por tiempo).

No comprobado: FLR (el `edu` no lo tiene); una MSI-X; un aparato de verdad en una máquina de verdad; el remapeo en una
AMD-Vi de verdad (shiva); con KVM y AMD-Vi (`kernel-irqchip=split`). Lo que queda está en la tarea X11.


### 2026-11-16 — X11b: remapeo de interrupciones de VT-d

Hecho (sala limpia: la especificación de VT-d de Intel, 5.1, 6.5.2, 7.1.3, 9.10 y 10.4; el código de QEMU solo para
entender qué emula su `intel-iommu`):

- `base/x86_64/iommu.rs`: la cola de invalidaciones (una página, descriptores de 128 bits, espera con sello en
  memoria). Se enciende en las unidades con ECAP.IR, y desde ahí todas sus invalidaciones (contextos, IOTLB, caché de
  entradas de interrupción) van por ella; sin IR, por registros, como antes. Una tabla de remapeo de 256 entradas para
  todas las unidades (`encender_remapeo`: IRTA, SIRTP, invalidación global, CFI a 0, IRE), con EIME si la CPU va en
  x2APIC y todas las unidades tienen EIM. `irte_poner` da a cada fuente (número de PCI y clave) su entrada: entrega
  fija, destino físico, el modo de la fuente, y SVT = 01 con su SID (solo ese aparato la puede usar); un cambio de una
  entrada viva solo toca la palabra baja. `irte_quitar` las quita. Los fallos del remapeo (motivos 0x20 a 0x26) se
  escriben como "interrupción bloqueada", con el aparato y la entrada.
- `acpi.rs`: el número de enumeración de cada ámbito de la DMAR (el ID del IOAPIC) y la bandera INTR_REMAP.
- `iommu.rs`: el remapeo se enciende si la DMAR y la unidad lo admiten y cada IOAPIC de la MADT sale en el ámbito de
  alguna unidad (su número de PCI, para comprobar el origen); si no, se dice en la consola y todo sigue en formato
  compatible. `iommu::dar_msi` programa la MSI de un aparato en formato remapeable si su unidad remapea.
  `inicio_x86.rs` la usa para xHCI, NVMe y virtio-sound.
- `ioapic.rs`: cada entrada, en formato remapeable (índice en 63:49 y el bit 11, bit 48 a 1) con su entrada de la tabla
  (clave: la GSI); `rehacer` pasa a ese formato las que ya estuvieran en marcha.
- `pasar.rs`: el aparato pasado a una máquina tiene su entrada, a su vector de siempre en el anfitrión, y la bandera
  `REMAPEADAS` también con VT-d. Al soltarlo, sus entradas se quitan. El aviso de "VT-d sin remapeo" queda solo para
  una unidad sin IR.
- Prueba (X11, `msi_remapeada`): con VT-d, la MSI del aparato llega; con su dirección cambiada a mano a la entrada de
  otro aparato (al mismo vector y núcleo), no llega y sale el fallo de origen (0x26) con su número; suelto, no le queda
  ninguna entrada. La de X3 (la UART por el IOAPIC al núcleo 2) y la de X5b (las invalidaciones de la IOTLB, ahora por
  la cola) pasan con el remapeo encendido.
- Makefile: `IOMMU=intel` pone `intremap=on` (QEMU 11 ya lo trae por omisión con TCG; antes de esto la unidad lo
  ofrecía y no se usaba) e `INTREMAP=off` lo quita. Con `ACCEL=kvm` y cualquier IOMMU, `kernel-irqchip=split`. Con
  remapeo, los esperados suman su línea y la de X11 con VT-d. `arranque-x86` falla también con una "interrupción
  bloqueada".

Comprobado (QEMU 11.1.3, q35, TCG): `make -C kernel prueba-x86` BIEN sin IOMMU, con `IOMMU=intel` (remapeo con
destinos de 8 bits: TCG no da EIM), con `IOMMU=intel INTREMAP=off` (como antes: sin remapeo y el aviso) y con
`IOMMU=amd`; `make -C kernel arranque-x86 IOMMU=intel` BIEN (salida, disco, captura y sonido; las MSI-X de los tres
servicios remapeadas; ni fallos de DMA ni interrupciones bloqueadas). En pegaso (i5-8500, VT-x con KVM anidado,
`-cpu host`, `kernel-irqchip=split`, QEMU 10.2.1), en `~/dashos-pruebas-ir`: `prueba-x86` BIEN entera, con el remapeo
en formato de x2APIC (destinos de 32 bits). ARM: `make -C kernel prueba` y `make -C kernel arranque-virt` BIEN.

Una cosa de QEMU: con el remapeo encendido, su VT-d deja pasar los mensajes en formato compatible (no mira CFI), así
que ese bloqueo no se puede probar ahí. La comprobación del origen sí la hace, salvo para el DMA de un aparato (llega
sin número): por eso la prueba cambia la MSI de verdad del aparato.

No comprobado: el bloqueo del formato compatible; una VT-d de verdad; varias unidades (unas con remapeo y otras sin
él); `arranque-x86` con KVM. Lo que queda está en la tarea X11b.


### 2026-11-16 — X5c (a medias): AHCI, HDA y la red de virtio en x86

Papel: implementador. Hecho (sala limpia: Serial ATA AHCI 1.3.1 de Intel, ATA8-ACS para las órdenes, Intel High
Definition Audio 1.0a, virtio 1.2; nada de Linux, BSD ni QEMU):

- **AHCI** (`usuario/src/bin/ahci.rs`): traspaso del firmware si lo hay, reinicio del HBA, cada puerto con un disco ATA
  preparado (parado, su lista de órdenes y su zona de FIS, enlace con COMRESET si hace falta, firma), IDENTIFY, LBA de
  48 bits. Varios puertos: cada disco tiene su número. Órdenes sin cola en ranuras distintas (hasta 8 en vuelo),
  vaciar con FLUSH CACHE EXT, descartar con DATA SET MANAGEMENT (TRIM), recuperación de errores (6.2.2.1). Su MSI (sin
  MSI-X: el primer aparato de q35 que la usa; el código del kernel ya estaba).
- **El canal de los discos, común** (`usuario/src/disco_servidor.rs`): el protocolo del NVMe (tramos por nombre, tabla
  de particiones, ventanas, anillo con lotes, plazos) separado del controlador (`Controlador`). Varios discos: el
  número va al abrir, en los bytes del primer sector (el NVMe no lo mira). `nvme.rs` sigue con su copia para no tocar
  ARM; se puede pasar a este módulo más adelante.
- `inicio_x86.rs`: el AHCI (clase 0x010601, BAR 5) con el canal "sata" si hay NVMe y "disco" si no (va antes que
  `disco` en la lista: el primero que nombra un canal lo sirve; probándolo sin NVMe, `disco` se quedaba con el lado del
  servidor); `disco-sata`, su prueba, que solo escribe en discos que empiezan con la marca de `tools/disco_sata.py`.
- `make -C kernel sata-x86` (nuevo): un PC con un solo disco SATA (el de `arranque-virt`) y sin NVMe; `disco` (su prueba
  entera, también el plazo vencido de A16 y el anillo en orden de A15) y `archivos` van por el AHCI.
- **HDA** (`usuario/src/bin/hda.rs`): reinicio, CORB y RIRB, el códec recorrido (grupo de audio, patillas de salida
  conectadas por preferencia, camino a un DAC por mezcladores y selectores), el camino encendido a 0 dB, el primer
  flujo de salida con 8 búferes de 256 cuadros. Hacia el servicio `sonido`, igual que `virtio-sonido`. `sonido` acepta
  "virtio-sonido" o "hda" (el primero que manda la salida; solo sus tics marcan el ritmo).
- **Red** en x86: `red`, `virtio-red`, `red-gloton` y `red-prueba` en `inicio_x86.rs`, la opción `red` en
  `FUNCIONES_X86`. `virtio-red`: la entrada 0 de MSI-X en cada cola, `VIRTIO_F_ACCESS_PLATFORM` si se ofrece y las
  barreras de `dashos::barrera` (antes, `dmb` de ARM a mano; en ARM sale lo mismo).
- `make -C kernel arranque-x86`: dos discos SATA (puertos 0 y 2, `target/sata0.img` y `sata1.img`, nuevos en cada
  prueba y mirados después en Linux), el sonido por HDA (`intel-hda` y `hda-duplex`; `SONIDO_X86=virtio` vuelve a
  virtio-sound) y la red de `arranque-virt` (`RED_QEMU`, con el eco y la basura del PC; `iommu_platform=on` con IOMMU).
  El plazo pasa de 90 a 240 s (`TIEMPO_X86`). Los esperados del sonido y de la red, en `pruebas/arranque_x86_*.txt`.

Dos cosas que salieron probando:

- El RIRB de QEMU para el CORB al llegar a RINTCNT respuestas si RIRBCTL.RINTCTL está apagado: no hay más respuestas
  hasta que se borra RIRBSTS.RINTFL, que sin RINTCTL no se pone. Con RINTCTL encendido (y la interrupción del CORB
  apagada en INTCTL), y RIRBSTS borrado tras cada respuesta, va.
- Con VT-d, al arrancar, el controlador de sonido se queda hasta 30 ms sin correr (también `virtio-sonido`: "parado
  33 ms"). Con 8 búferes (42 ms) aguanta, pero el servicio solo tiene 4 períodos por delante: salía un período en
  silencio y la prueba del WAV daba dos chasquidos. El HDA ahora, si le faltan períodos, manda un tic y espera a la
  mezcla hasta 2 ms. Por qué se para tanto el arranque con VT-d no se ha mirado.

Comprobado (QEMU 11.1.3, q35, TCG): `make -C kernel arranque-x86` BIEN sin IOMMU, con `IOMMU=intel` (dos veces) y con
`IOMMU=amd`, y con `SONIDO_X86=virtio`: los dos discos SATA escritos y leídos en Linux, el WAV del HDA sin huecos ni
chasquidos, la red (DHCP, eco de 4 MB a unos 47 MB/s, puerto cerrado, basura), y con IOMMU el AHCI, el HDA y la red en
su dominio, con su MSI remapeada en VT-d, sin fallos de DMA ni interrupciones bloqueadas. `make -C kernel prueba-x86`
BIEN sin IOMMU, con `IOMMU=intel` y con `IOMMU=amd`. ARM: `make -C kernel prueba` y `make -C kernel arranque-virt`
BIEN.

`make -C kernel sata-x86` BIEN sin IOMMU, con `IOMMU=intel` y con `IOMMU=amd` (y el disco, leído después en Linux).
Tras mover `ahci` antes que `disco`, `arranque-x86` otra vez BIEN sin IOMMU, con `IOMMU=intel` y con `IOMMU=amd`.

No comprobado: una máquina de verdad (ningún disco SATA, códec ni tarjeta reales); errores de disco de verdad (la recuperación no se ha visto
funcionar); discos de 4 KB por sector; el HDA con mezcladores o selectores en el camino (el códec de QEMU va directo
de la patilla al DAC).

### 2026-11-16 — X5c: red con cable de Intel (I219 y 82574L)

Papel: implementador, sala limpia con `docs/spec/hw/i219.md` (del analista) y el código de DashOS; nada de Linux, BSD
ni QEMU. El manual del 82574 no se pudo descargar (sin conexión con las copias públicas).

- **`intel-red`** (`usuario/src/bin/intel_red.rs`, MPL): un solo programa para el I219 (familia `pch_cnp`: 0x15BB a
  0x15BE, 0x15DF a 0x15E2, 0x0D4C a 0x0D4F; pegaso es 0x15BC) y el 82574L (0x10D3, 0x10F6), con lo común (registros de
  la MAC, descriptores clásicos, MDIC) y las diferencias por el ID (§14). El mismo papel y el mismo trato con `red`
  que `virtio-red`: 128 búferes de recibir y 64 de enviar de 2 KB, copia entre la DMA y las ranuras, `MPE`, sin
  sumas, sin control de flujo, sin EEE. MSI-X en el 82574 (IVAR a la entrada 0) y MSI en el I219. El enlace se mira
  al llegar `LSC` y cada segundo, y se dice en el registro.
- I219, según la spec y **sin probar en hardware**: vaciado de los anillos al arrancar con un anillo propio (§13.1),
  salida del ULP con ME o sin él, PHY por SMBus o LANPHYPC (§6.2), semáforo `SWFLAG` (1,1 s como mucho), páginas
  del PHY por las direcciones MDIO 1 y 2, reinicio global sin lecturas en 20 ms y con la lectura de configuración
  antes y después, `LAN_INIT_DONE`, banco válido de la NVM y configuración "LCD" desde ella, bits OEM, bits
  obligatorios, `KABGTXD`, ajustes al subir el enlace (TIPG, EMI `0xB20C`, PLL, (776, 20), EEE apagado) y `FEXTNVM4`
  en cada mirada. Todas las esperas acotadas con los tiempos de la spec (la flash, 100 ms por ciclo).
- 82574: reinicio con el semáforo del MDIO y `EECD.AUTO_RD`, bits de §14.3 (TARC0, GCR, GCR2...), sin ASPM (L0s y
  L1 apagados en el control del enlace del PCIe), `RCTL.EN` sin apagar nunca tras encenderlo, `PBA` 32, MAC de
  `RAL0` o de la EEPROM (`EERD`). El cuelgue del PHY (§14.3) se detecta y se avisa, pero no se reinicia la tarjeta en
  marcha.
- `inicio_x86.rs`: `Quien::Ids`, varios aparatos de un fabricante; `intel-red` con BAR 0 y su configuración. El
  servicio `red` acepta `TARJETA` de "virtio-red" o de "intel-red".
- `make -C kernel arranque-x86 RED_X86=e1000e` (nuevo; por omisión `virtio`): la 82574L de QEMU en el mismo
  concentrador. Los esperados de cada tarjeta, en `pruebas/arranque_x86_red_{virtio,e1000e}.txt`. La línea de la IOMMU
  de la red sale del Makefile, como la del sonido.
- La 82574L tira ella misma lo corto (menos de 14 bytes) y lo largo (más de 1518 con `LPE` a 0): no llega al
  controlador. En la primera vuelta `red-prueba` sacaba el eco tras la basura en una línea aparte; desde la segunda
  (abajo) el controlador suma los contadores de la tarjeta y la prueba vuelve a ser la de siempre.

Comprobado (QEMU 11.1.3, q35, TCG): `make -C kernel arranque-x86 RED_X86=e1000e` BIEN sin IOMMU, con `IOMMU=intel` y
con `IOMMU=amd`: la 82574L con su MSI-X (remapeada en VT-d), la MAC de `RAL0`, el enlace arriba a 1000 Mb/s, DHCP, el
eco de 4 MB (50 a 60 MB/s), el puerto cerrado y el eco tras la basura; con IOMMU en su dominio y sin fallos de DMA.
La primera vez sin IOMMU falló `disco-sata` ("sin servicio de discos" al abrir el disco 2, nada de la red); repetida,
BIEN. `make -C kernel arranque-x86` (virtio-net) BIEN sin IOMMU y con `IOMMU=amd`; `make -C kernel prueba-x86` BIEN.
ARM: `make -C kernel prueba` BIEN; `make -C kernel arranque-virt` BIEN a la segunda (la primera, todos los esperados
bien, también la red, pero la captura del compositor con las ventanas fuera de sitio).

Segunda vuelta (el mismo día), con las respuestas del analista (`docs/spec/hw/i219.md`, §18, commit `c45b2030`):

- `PBA` se escribe antes del reinicio global (el reinicio lo conserva). `DRV_LOAD` otra vez tras cada reinicio.
- `FORCE_SMBUS`: sin ME, se quita cada vez que el PHY responde, también si solo respondió forzándolo; con ME se deja
  como lo ponga el ME. Si sigue a 1 tras el reinicio global, se avisa y se sigue: no es fallo.
- Las entradas 1 a 11 de direcciones del I219 (`SHRA`, del ME), sin tocar; en el 82574, las 14 restantes a 0 (antes
  15, una de más).
- 82574: su propia configuración del PHY (registro 16, MDI/MDI-X automático y polaridad; los registros 29 y 30 en el
  PHY de QEMU; reinicio suave para confirmar) en vez de los registros del I219; `TXDCTL` con `WTHRESH`, `GRAN` y
  `COUNT_DESC`, sin `PTHRESH` (el `PTHRESH` = 31 queda solo en el I219); `TXDCTL1` igual que `TXDCTL` en los dos.
- Sin reescribir `IMS` tras cada interrupción MSI-X (con `EIAC` = 0 no hace falta).
- Contadores de la tarjeta (§11.1: `RUC`, `ROC`, `RJC`, `MPC`): se borran tras el reinicio y cada segundo se suman a
  las cuentas de cortas, largas y sin sitio del servicio. Con ellos, la basura del PC vuelve a contarse con la
  82574L (21 cortas y 20 largas) y `red-prueba` vuelve a su línea de siempre.
- **Parada ordenada** (§12): recibir (no en el 82574) y enviar apagados, interrupciones fuera, vaciado de los
  anillos en el I219, reinicio global sin la parte del PHY, PHY apagado si el ME no lo usa, `DRV_LOAD` quitado
  después del reinicio y sin maestro de bus en el PCI; si el aparato lee todo a unos, solo esto último. Se hace
  cuando el servicio de red se va (su canal, cerrado). **Sin probar**: ninguna prueba cierra el servicio.
- Cuelgue del PHY del 82574, dos veces seguidas: parada (pasos 1 a 5) y arranque entero en marcha, con la misma DMA
  y el mismo canal; lo que estaba en vuelo para enviar se pierde. **Sin probar**: QEMU no cuelga el PHY.

Comprobado en la segunda vuelta: `make -C kernel arranque-x86 RED_X86=e1000e` BIEN sin IOMMU, con `IOMMU=intel` y con
`IOMMU=amd`; `make -C kernel arranque-x86` (virtio-net) BIEN; `make -C kernel prueba-x86` BIEN. `red-prueba` queda
como antes de esta tarea, así que ARM no cambia.

Sin hacer: la multidifusión exacta (§11), el estado del enlace hacia `red-estado` (solo en el registro), la prueba
del bucle en el PHY (§15) y las demás generaciones del I219 (`pch_spt`, `pch_tgp` y siguientes).

### 2026-11-16 — X5c: red con cable de Realtek (RTL8125B), sin probar en hardware

Papel: implementador, sala limpia con `docs/spec/hw/rtl8125.md` (del analista) y el código de DashOS; nada de Linux,
BSD ni QEMU. El firmware real (`rtl8125b-2.fw` de shiva) solo se leyó como datos, para ver que el intérprete lo acepta.

- **`rtl-red`** (`usuario/src/bin/rtl_red.rs`, MPL): solo la RTL8125B (XID 0x641); con otro XID, o en modo SFP, lo
  dice y sale sin tocar nada. Arranque en el orden de la spec (§8): D0, sin ASPM ni CLKREQ en el control del enlace,
  salida de OOB (con el PHY en "UPS" si lo está), reinicio, MAC de fábrica (`0x19E0`) escrita como activa, PHY
  (encendido, firmware, configuración de la 8125B, EEE apagado como OpenBSD, anuncio de 10 a 2500 sin pausa,
  negociación), ePHY y la parte común, anillos (parte alta antes que la baja), toda la multidifusión. Descriptores de
  16 bytes; 128 y 64 búferes de 2 KB; relleno a 60 bytes; CRC fuera; timbre otra vez si quedan pendientes. MSI-X en
  su entrada 0 (o cada 2 ms sin ella). El enlace, de `PHYstatus` al llegar `LinkChg` y cada segundo. Parada ordenada
  (§12) si el servicio se va; con `SYSErr`, parada y arranque en marcha con la misma DMA. Todas las esperas acotadas;
  lo que no responde se avisa y se sigue, salvo el reinicio de la MAC (se abandona).
- **El firmware** (`usuario/src/rtl8125_fw.rs`, MPL, sin dependencias): valida el fichero entero (cabecera, suma,
  `inicio` y `n`, formato antiguo, códigos, saltos y cambios de modo) antes de ejecutar nada, y lo interpreta sobre un
  `Bus` (OCP del PHY y de la MAC, esperas) con la base compartida de los dos modos. Tope de 200 000 acciones
  ejecutadas. Sus pruebas (9, con ficheros sintéticos y un aparato falso) van en `make -C kernel prueba`. En seco
  sobre el `rtl8125b-2.fw` de shiva: válido, versión `rtl8125b-2_0.0.2 07/13/20`, 800 acciones.
- **De dónde sale el firmware**: no hay en DashOS un sistema de ficheros para los controladores al arrancar, así que
  el Makefile copia el del sistema donde se compila (`/lib/firmware/rtl_nic/rtl8125b-2.fw[.zst|.xz]`, o
  `FIRMWARE_RTL8125=...`; vacío para dejarlo fuera) a `kernel/target/firmware/`, `build.rs` lo mete en el kernel de x86
  y `inicio_x86.rs` lo da a `rtl-red` como datos ("rtl8125b-fw", siempre que esté la tarjeta, con tamaño 0 si no hay).
  No va en git. **Aviso de licencia**: una imagen de x86 hecha en un sistema que lo tenga lo lleva dentro; si se
  reparte, con `LICENSE.r8169` al lado (§6.4).
- `inicio_x86.rs`: `rtl-red` con la BAR 2, su configuración y el firmware. `red` acepta "rtl-red".
- **La prueba con la tarjeta de verdad, preparada y sin ejecutar** (pide sudo): `make -C kernel arranque-x86
  RED_X86=vfio PCI_RED=0000:03:00.0 ACCEL=kvm [MAC_RED=...] [IOMMU=intel]` arranca q35 con `-device
  vfio-pci,host=...,rombar=0` en vez de la tarjeta emulada, sin el concentrador ni la basura del PC; con `IOMMU=intel`,
  la VT-d con `caching-mode=on` (AMD-Vi se niega). Esperados en `pruebas/arranque_x86_red_vfio.txt`: el aparato, el
  PHY, el firmware (aplicado o no, según lo lleve el kernel), la versión y la MAC (la de Linux con `MAC_RED`), el
  enlace, DHCP 10.0.2.15, el eco de 4 MB y el puerto cerrado. `tools/vfio_red.sh [PCI] [OTRO]` comprueba sin
  privilegios (la tarjeta sola en su grupo de IOMMU, ni ella ni OTRO con la ruta por defecto) e imprime los comandos:
  el otro lado (enp4s0, la I226-V: 10.0.2.2 y 10.0.2.100, ufw, `dnsmasq` dando 10.0.2.15 y el eco de
  `tools/red_eco.py` con `systemd-socket-activate` en el puerto 7), soltar la tarjeta de r8169 a vfio-pci, el límite
  de memoria bloqueada, la prueba y devolverlo todo.

Comprobado (QEMU 11.1.3, q35, TCG): `make -C kernel arranque-x86` (virtio-net) BIEN; `make -C kernel arranque-x86
RED_X86=e1000e` BIEN; `make -C kernel prueba-x86` BIEN. ARM: `make -C kernel prueba` BIEN, con las pruebas del
intérprete. `cargo clippy` de `rtl-red` sin avisos. `tools/vfio_red.sh` en shiva: la RTL8125 (03:00.0, r8169, enp3s0)
sola en el grupo 26, la ruta por defecto por wlan0; se niega si OTRO es wlan0.

**Probado en hardware (2026-11-16, shiva):** la RTL8125B de verdad pasada a QEMU con VFIO (KVM, AMD-V), con un cable
a la I226-V de shiva (Linux, `tools/vfio_red_preparar.sh`; se deshace con `tools/vfio_red_devolver.sh`). `make -C kernel
arranque-x86 RED_X86=vfio PCI_RED=0000:03:00.0 ACCEL=kvm MAC_RED=38:05:25:34:d8:db` BIEN a la primera, y con
`IOMMU=intel` también (rtl-red en su dominio, sin fallos de DMA). Se vio: XID 0x641, firmware aplicado (798 acciones),
PHY 001cc840, MSI-X, enlace a 100 Mb/s (el cable solo tiene dos pares), DHCP de dnsmasq, eco de 4 MB igual, 9,98 MB/s
(lo que da el enlace), ida y vuelta mediana 245 µs. Aviso en el arranque: "las FIFO no se vacían al salir de OOB; se
sigue" (sin efecto visible; pregunta 1). Sin probar: 1 y 2,5 Gb/s (falta un cable de cuatro pares) y la parada.
Segunda vuelta (2026-11-17, con las respuestas del analista, 0b22237f, y ya unidas x86-uso, x86-gpu, instalador y
x86-dominio): sin y con `IOMMU=intel`, BIEN a la primera, y ya sin el aviso de las FIFO.
Sin hacer: el paso 17 de §8 (latencia de L0s y L1; ASPM va apagado), el bucle de la MAC o del PHY
(§14), la multidifusión exacta, el parche de las tramas PTP cortas (§7.2; la pila no hace PTP) y el estado del enlace
hacia `red-estado` (solo en el registro).

Preguntas para el analista (`docs/spec/hw/rtl8125.md`):

1. §8 A.5.1 dice "vaciar FIFO (punto 11.2)", que no existe. Se esperan las FIFO vacías como en §12 3.2 y 3.3
   (`MCU` bits 4 y 5, `IntrMitigate` & 0x0103), **sin** poner `StopReq`. ¿Es eso, o va también `StopReq`?
2. §3.2 y §6.3: en modo PHY, ¿qué dirección tiene un registro por debajo de 0x10 con una página que no es la
   estándar? Ahora es un error que para el firmware.
3. §6.3: ¿qué devuelve leer el "registro 0x1F" en modo MAC? Ahora es un error.
4. §6.3: ¿un `PHY_BJMPN` de 0 es válido (se repetiría siempre)? Ahora invalida el fichero. ¿Un salto hacia delante
   que acaba justo al final de la lista es válido? Ahora sí. En modo PHY, ¿vale un registro mayor que 0x1F? Ahora
   sí (`0xA400 + 2·R`). ¿Linux pone algún tope a los bucles del fichero? Aquí, 200 000 acciones.
5. §6.3: ¿`inicio` puede caer dentro de la cabecera (menos de 45) o tiene que ir alineado? Ahora se exige al menos 45
   y no se mira la alineación.
6. §6.6 paso 5: el `BMCR` = 0x9200 (con el bit de reinicio) va después del firmware y de §6.5. ¿Ese reinicio no deshace
   lo que se escribió en §6.5 o el EEE apagado? Si Linux hace un reinicio "suave" distinto, ¿cuál es exactamente?
7. §8 A.2: "ECPM" se ha tomado como el bit 8 del control del enlace del PCIe (Enable Clock Power Management). ¿Es ese?
8. §7.2: los bits de error al enviar (23, 20, `[19:16]`) vienen del formato de 32 bytes de OpenBSD. ¿Valen en el de
   16? Ahora solo se mira el 23 y se avisa una vez.
9. §11: `SYSErr` no está en la máscara de Linux. ¿Sube en `IntrStatus` sin estar en la máscara? Aquí se lee el estado
   en cada vuelta y se reinicia si está.
10. §12 paso 8: aquí se apaga el PHY al parar. ¿Lo vuelve a encender r8169 de Linux al recuperar la tarjeta (tras
    devolverla desde vfio-pci), o hay que dejarlo encendido?

Con las respuestas del analista (`docs/spec/hw/rtl8125.md` §17, commit `7fea3aef`), el mismo día:

- (1) Al salir de OOB, las FIFO se vacían con `StopReq` puesto, como al parar (lo pone ahora `esperar_fifo`). Debería
  quitar el aviso visto en shiva.
- (6) El PHY, en el orden de Linux: tras el firmware y §6.5, reinicio suave (BMCR: sin el bit 10, con el 15 y el 9;
  hasta 600 ms a que baje el 15, y 1 ms), después el anuncio (ANAR, GBCR, `0xA5D4`) y por último reiniciar la
  negociación (bits 12 y 9, sin el 10 ni el 15). Antes era el anuncio y luego `BMCR` = 0x9200.
- (9) `SYSErr` solo ya no reinicia: se mira el estado del PCI (`0x06`); con algún bit de error (8, 11 a 15) se borran
  escribiéndolos, se avisa y se reinicia en marcha; sin ellos, se avisa una vez y se sigue.
- (4) Los saltos hacia delante se validan como Linux: uno que acaba justo al final de la lista es inválido
  (`i + 1 + R ≥ n`; `PHY_READCOUNT_EQ_SKIP`, `i + 2 ≥ n`). Con su prueba en `rtl8125_fw.rs` (las tres órdenes de saltar
  y la de la cuenta). El `rtl8125b-2.fw` de shiva sigue siendo válido (en seco: 798 acciones).
- Lo demás de §17 no obliga: los casos raros (2, 3, `PHY_BJMPN` de 0, `inicio` < 45) siguen como error, más prudente
  que Linux y sin efecto con el firmware real; 7, 8 y 10 confirman lo hecho.

Comprobado: `make -C kernel arranque-x86` (virtio-net) BIEN; `arranque-x86 RED_X86=e1000e` BIEN; `prueba-x86` BIEN;
ARM `make -C kernel prueba` BIEN (con las 9 pruebas del intérprete). `cargo clippy` de `rtl-red` sin avisos. **Sin
probar en la tarjeta**: estos cambios no han pasado aún por la RTL8125B de shiva (`RED_X86=vfio`, la lanza el
coordinador).


### 2026-11-18 — X12: el dominio de gráficos con la virtio-gpu de QEMU (G5 de S38)

Hecho (sala limpia: la especificación de PCI Local Bus 3.0 (3.2.2.3.2, 6.1, 6.2.5), la de PCI Express 6.0 (7.7.1 y
7.7.2), la de la IOMMU de AMD 48882 (2.2.2.1 y 2.2.5) y la de VT-d (5.1, 9.10), el RFC 6143 de VNC y las interfaces
públicas de EGL, GLES 2, GBM y libdrm. Linux y Mesa se compilan y se ejecutan, no se leen; para entender por qué no
llegaban las interrupciones se usaron las trazas de QEMU, no su código):

- **Kernel (X11 ampliada).** Un aparato pasado tiene una interrupción del anfitrión por cada una de las primeras
  entradas de su MSI-X (como mucho 8), cada una remapeada: con AMD-Vi, su tabla de 2^n entradas (la k, dato k); con
  VT-d, una entrada por mensaje. `maquina_aparato` dice cuántas (bits 15:8 de x1) y la operación 5 da la llave de cada
  una y su número (el del mensaje de `interrupcion_asociar`). `Funcion::dar_msix` programa la tabla del aparato: con la
  memoria del aparato encendida antes (callado, la tabla no se decodifica y lo escrito se perdía: así las tres
  interrupciones nunca llegaban). Al soltarlo, igual que antes.
- **Cargador.** `InfoArranque` versión 3: si junto al cargador están `EFI/DashOS/GRAFICOS.BZI` y `GRAFICOS.CPI`, van en
  una zona "DashGraf". La imagen `graficos` (`inicio_x86.rs`) da al servicio `dominio-graficos` el permiso de máquinas,
  la primera virtio-gpu como aparato del PCIe (tipo 21; ningún servicio la toma) y los dos ficheros.
- **El monitor.** `usuario/src/dominio_x86.rs` (la pieza del instalador) gana `Dominio::sin_disco`, `vcpu`, `salidas`
  y `correr_con` con el rasgo `Extra` (puertos, MMIO e interrupciones de quien lo usa); `nuevo` y `correr` siguen
  igual, y el instalador no cambia (solo un `allow(dead_code)`). `usuario/src/dominio_pci.rs`: el bus 0 por
  0xCF8/0xCFC con un puente (1b36:0008) y el aparato en 00:01.0; sus BAR ya puestas desde 0xC000_0000 (se miden con
  unos, se proyectan al encender la memoria); su MSI-X con el tamaño de tabla de las interrupciones del kernel, su
  control, su tabla y su PBA emulados (esa página no se proyecta); su MSI si no tiene MSI-X. Un hilo escucha las
  interrupciones (todas en un canal), las apunta como pendientes y patea a la vCPU; antes de entrar, las sin máscara
  van al APIC del invitado. `usuario/src/bin/dominio_graficos.rs`: el servicio (512 MB de RAM para el dominio; su
  tope de grupo, 576 MB, en `inicio.rs`).
- **El invitado** (fuera de git, `tools/dominio_graficos.py preparar` en `~/recuperar/dashos-97/dominio-graficos/`):
  Linux 7.3.4 con tinyconfig + `caja-linux/dominios/x86_graficos.config` (PCI por el mecanismo 1 sin ACPI, MSI, DRM y
  virtio-gpu; bzImage de 1,1 MB). Mesa 26.2.7 con solo virgl, sin LLVM, GLES 2 y GBM; libdrm 2.4.134; zlib 1.4; la
  toolchain de x86-64 con musl de Bootlin 2026.08-1 (musl dinámica, libstdc++). Sumas comprobadas; Mesa y libdrm
  además con su firma GPG (Eric Engestrom y Simon Ser). initramfs de 23 MB sin comprimir. El /init
  (`caja-linux/dominios/graficos_dominio.c`) monta /dev, /proc y /sys, abre `card0`, pinta con GLES 2 un plasma que
  cambia entero en cada cuadro y un cuadrado que gira, cambia de búfer con `drmModePageFlip`, comprueba un píxel
  contra la cuenta en la CPU y dice cuadros y CPU por cuadro (getrusage).
- **La prueba.** `make -C kernel graficos-x86` (`tools/prueba_graficos_x86.py`): q35 con KVM, `kernel-irqchip=split`,
  `intel-iommu,intremap=on` o `amd-iommu,dma-remap=on,intremap=on`, la VGA de q35 para DashOS y
  `virtio-gpu-gl-pci,iommu_platform=on` para el dominio, `-display egl-headless`. Mira la salida en orden, que no haya
  fallos de DMA ni interrupciones bloqueadas, y dos capturas de la GPU del dominio por VNC con medio segundo entre
  ellas (no negras y distintas). Con egl-headless el `screendump` de QMP no tiene superficie ("no surface"); el VNC sí
  recibe los cuadros: `tools/vnc_captura.py`, un cliente RFB mínimo sin programas de fuera. Antes, fuera de DashOS,
  `tools/dominio_graficos.py qemu` (el mismo invitado en QEMU con la misma GPU): BIEN.

Medido en shiva (Ryzen 9 9955HX, Radeon integrada detrás de virgl), 20 s pintando a 1280x720, mismo invitado:

| | Cuadros por segundo | CPU del programa por cuadro (en el dominio) | CPU de DashOS por cuadro (todos los núcleos) |
|---|---|---|---|
| Linux en QEMU con KVM, sin DashOS | 75,0 | 0,137 ms | — |
| DashOS, VT-d | 75,0 | 0,108 y 0,124 ms (dos vueltas) | 0,55 y 0,60 ms (0,58 sin lo de reposo, 2,1 ms/s) |
| DashOS, AMD-Vi | 75,0 | 0,110 ms | 0,56 ms (0,54 sin lo de reposo) |

- 75 cuadros por segundo es el refresco de la pantalla de la virtio-gpu (el cambio de búfer espera al anterior).
- Por cuadro, 9 salidas del invitado y 3 interrupciones de la GPU (todas por el monitor). La CPU de DashOS (lo que no
  está ocioso, sumando los 4 núcleos) incluye la vCPU del dominio: 0,55 ms por cuadro son un 4 % de un núcleo a 75 Hz.
  La de virgl en QEMU (el PC) no se mide.
- Del servicio en marcha al primer cuadro en pantalla: 650 a 680 ms (RAM, kernel, Mesa y EGL). La prueba entera, 25 s.
- `GL_RENDERER virgl (AMD Ryzen 9 9955HX ... radeonsi)`, OpenGL ES 3.2; el píxel comprobado, diferencia 0.

Comprobado: `make -C kernel graficos-x86` BIEN con `IOMMU=intel` (dos veces) y con `IOMMU=amd`; `make -C kernel
prueba-x86` BIEN sin IOMMU, con `IOMMU=intel` y con `IOMMU=amd` (la X11 de antes, con el `edu` y su MSI, sigue);
`make -C kernel arranque-x86` BIEN a la segunda (la primera, 2 chasquidos en el sonido, con la carga en 4,5: el
intermitente conocido); `make -C kernel prueba-instalar-x86` BIEN (46 de 46); `python3 tools/prueba_instalador.py
encoger-sintetico` BIEN (8 de 8: el dominio de ntfsresize, con `dominio_x86.rs` cambiado, sigue); ARM `make -C kernel
prueba` BIEN.

Lo que se vio por el camino:

- Sin ACPI el kernel del invitado no encuentra la INTx ("can't find IRQ for PCI INT A; probably buggy MP table"): da
  igual, va por MSI-X. Avisa también de dos MSR que el monitor no tiene (RDMSR 0x3B, TSC_ADJUST; WRMSR 0xC001001F, de
  AMD) y sigue.
- Las BAR de la virtio-gpu en el anfitrión: la 1 (la tabla de MSI-X, sola en su página) y la 4 de 16 KB a 64 bits,
  que q35 pone en 0x3800_0000_0000.

No comprobado: una GPU de verdad (G8); VT-x (pegaso); TCG (la prueba pide KVM); el escritorio de DashOS compuesto en
el dominio (G3, G4, X13). Lo que queda, en la tarea X12.

### 2026-11-22 — Unión de `revision` en x86, el kernel en 64 MB y X13

Lo pidió el usuario el 2026-11-22 (por jefe-dash): llevar x86 y ARM a la par y seguir con X12 y lo que venga. Rama `par`
(desde `x86`, en `.worktrees/x86`), sin subir.

- **Unión de `revision`** (5f57eaf5, 116 commits) en x86 (1f593ca9, 76), commit fafb6044. Once ficheros con conflicto,
  casi todos de juntar las dos cosas. Lo que no era juntar:
  - La acción de tanda "instalar": en x86 instalaba DashOS en el disco (S32, con un número de plan) y en `revision`
    instala un paquete (S8, Paq). La de S32 pasa a "instalar-sistema", al final de la lista (las de Paq no cambian de
    número). Cambian `tanda.rs`, `sesiones.rs`, la orden `Instalar` y el programa `instalar`.
  - El sonido: `revision` rehízo el servicio (el grafo). Lo de X5c se lleva encima: el primer controlador que manda
    `SALIDA` ("virtio-sonido" o "hda") es el que suena, y solo sus tics marcan el ritmo.
  - `xhci_comun.rs`: la barrera de antes del bit de ciclo, por `dashos::barrera::escrituras` (en `revision` era un
    `dmb` de ARM a pelo).
  - La imagen de usar de x86 (`inicio_x86_uso.rs`) con lo nuevo de `virt` con `uso`: `paq`, `dock` y `mezclador`;
    `guardian` con "fondo", `barra` con "sonido" y `panel` con "avisos". `build.rs` busca `paq` en
    `ordenes/target/<destino>` (antes, solo el de ARM).
  - `arranque_x86.txt`: la raíz del disco de serie trae `Fondos/` y `Paquetes/`.
- **El kernel de x86 en 64 MB** (14acca09). Con todo lo de ARM, la imagen de usar mide 7,5 MB y no arrancaba: el
  cargador no podía reservar de 1 MB a 8,5 MB (OVMF tiene ocupado desde 8 MB; en 16 MB descomprime su firmware). En
  64 MB está libre. Las tablas del arranque ya veían el primer GB: solo cambian `enlace_x86.ld` y la prueba X4 que lee
  el kernel por el mapa lineal.
- **X13**: la ventana compartida (arriba, en la tarea). Escrito con la interfaz de `maquina_memoria` (ya existía) y,
  del lado del dominio, las interfaces públicas de Linux (/dev/mem, `ioperm`/`outl` de musl) y GLES 2.

Medido en shiva con KVM, la prueba `graficos-cliente` (640x480, XRGB8888, 300 cuadros):

| | VT-d | AMD-Vi |
|---|---|---|
| Cuadros vistos y bien (en memoria los 300, en pantalla 10) | 300, 0 mal | 300, 0 mal |
| El dominio lee la memoria compartida | 1589 a 1939 MB/s (con caché) | — |
| De "listo" a "visto" (subir, pintar y esperar el refresco de 75 Hz), media y peor | 19,7 y 29,7 ms | 19,7 y 29,8 ms |
| Subir el cuadro a la GPU en el dominio (glTexSubImage2D con virgl) | 0,23 a 0,24 ms | 0,24 ms |
| Pintar el cuadro en DashOS (el programa) | 0,41 ms | 0,41 ms |

La CPU de DashOS por cuadro del plasma (lo de X12) salió entre 0,99 y 1,17 ms (antes, 0,55 a 0,69): con el timbre y
sin él igual, y con la máquina a una carga media de 16 (otras sesiones compilando). No vale para comparar; hay que
medirlo con el PC quieto.

Comprobado: `make -C kernel prueba` (ARM) BIEN; `prueba-x86`, `arranque-x86`, `graficos-x86` (IOMMU=intel y amd) BIEN;
la imagen de usar de x86 arranca en 1920x1080 con KVM (capturas en `~/recuperar/registros/dashos-x86-2026-11-22/`).
`make -C kernel prueba-instalar-x86` (el pendrive en vivo, con el kernel en 64 MB) BIEN, 46 de 46.
No comprobado: un PC de verdad con el kernel en 64 MB; el tope de memoria del programa `graficos-cliente` (el de serie)
con ventanas grandes.

### 2026-11-22 — X7, pasos 1 y 2: RelevoJIT para la caja y la interfaz de motor

- **Paso 1** (RelevoJIT, rama `motor-caja`, f74486e; su `docs/BITACORA.md`): entrada anidada, base 0,
  `relevo_invalidate_all` y la carrera del aviso. `make test` bien; `prueba-x86` con `RELEVO_DIR` bien y el banco igual.
- **Paso 2** (rama `motor` de DashOS, desde `par`): `caja/motor.h` y `caja/motor_emu68.c`. Lo cosido a Emu68 sale de
  `caja.c` tal cual: su estado, `MainLoop` y su anidado (con las unidades fijadas), el SR y las pilas en v21, `INTF`,
  olvidar lo traducido, la FPU y el arranque del JIT. `caja.c` queda con lo común: `caja_nativa` (lo que era
  `dashos_native_call`), los niveles anidados sobre `motor_anidado`, el arranque sobre `motor_arrancar` y las pruebas.
  Las cuatro entradas de Rust (`caja_llamada`, `caja_aparcar_en_vuelta`, `caja_caida`, `caja_turno`) reciben
  `Registros` (`#[repr(C)]`, el `motor_regs` de C) en vez de la copia x0..x30 de Emu68; `caja_llamada` sin copiarlos.
  `caja_sc` y `caja_pon_c` pasan a ser de la caja para los motores (las llamadas al sistema siguen siendo de ARM: paso 3).

Comprobado en ARM, `make -C kernel arranque-virt` (TCG), contra una vuelta de referencia en `par` el mismo día:

- La caja, todas sus pruebas bien en las cuatro vueltas. Una vez `ventana.exe` se cerró sola a los 40 s (el clic del
  guion no llegó; la prueba acepta las dos cosas); en las otras tres, con su gadget, como en la referencia.
- Tres de las cuatro vueltas se pararon en "[ram] Ana: avisos hostiles" (llegan 16 de 63 timbres): el intermitente
  apuntado en `docs/bitacoras/dock-discos.md`, con la misma cifra, del servicio de archivos y no de la caja. La
  referencia, una sola vuelta, no lo dio. La tercera vuelta, entera en verde.
- Tiempos de la caja: la suma de sus pruebas de 100 ms o más, 51,19 s en la referencia y 51,30 s con el motor (la
  mejor de dos; casi todo es esperar al guion). Más lentos: los cambios de tarea (`procesos-e`, 20.002 cambios: 129 ms
  en la referencia; 142 y 151 con la copia de registros en C y en Rust; 136 ms, un 5 %, sin la de Rust) y `procesos-b`
  (759 ms; 846 y 870; 817, un 8 %). Es la conversión entre la copia de Emu68 y `motor_regs` en cada llamada nativa
  (unos 117 ns más por llamada en TCG). Lo demás, dentro del ruido. Decisión (jefe-dash, 2026-11-22): se acepta y
  se sigue; hay que medirlo en una Pi de verdad. Se miró antes darle a `motor_regs` la disposición de la copia de
  Emu68 (pasar su puntero sin convertir): no vale, la copia no es contigua (A0-A4 en x13-x17, D0-D7 en x19-x26, A5-A7
  en x27-x29, con x18 en medio, de 64 bits cada uno).

### 2026-11-22 — Aparte: el intermitente de "[ram] Ana: avisos hostiles"

No es de la caja ni de x86. En `make -C kernel arranque-virt` (ARM, TCG), con la máquina a carga 7-10, la prueba de
`ram` dio "llegan 16 timbres" (de 63) en 3 de 5 vueltas el 2026-11-22 (las tres con la rama `motor`; la de referencia,
en `par`, una sola vuelta, bien). Es la misma cifra que apunta `docs/bitacoras/dock-discos.md` (16 de 63 en los 300 ms
que espera la prueba, con otras QEMU en marcha). Sin mirar la causa: para la sesión del servicio de archivos.
