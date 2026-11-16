# Bitácora: DashOS de verdad en x86 (rama `x86-uso`)

Rama `x86-uso`, desde `origin/x86` (1abce2b8). Lo pidió el usuario: arrancar en una máquina virtual x86 con KVM el
escritorio que se usa en ARM (`make virt-uso`, `tools/qemu_a_mano.sh`), para verlo y quizá grabarlo. La rama `x86` solo
enseñaba un escritorio de prueba (dos ventanas y Ajustes) e iba 167 commits por detrás de `revision`. Cuando acabe, la
sesión de x86 une esta rama en la suya.

## 2026-11-16: `revision` (d45a8281) unida en x86

Nueve ficheros con conflicto; en todos, las dos partes. Lo de ARM no cambia.

- **Dos números que chocaban.** La llamada 56 era `puertos_permitir` en x86 y es `llaves_nombres` en `revision`:
  `puertos_permitir` pasa a la **59** (libre en todas las ramas). Y el tipo de llave de arranque 12 era "puertos de E/S"
  en x86 y es "permiso del reloj" en `revision`: los puertos pasan al **13**. Cambian `llamadas.rs`, `usuario_x86.s`,
  `usuario/src/lib.rs`, `inicio_x86.rs`, `usuario/src/bin/inicio.rs` (que nombra el 13) y `docs/referencia/kernel.md`.
- **El paquete de arranque** de `revision` lleva tras los programas los nombres de sus canales (la tabla de llaves):
  `inicio_x86.rs` los pone, como `inicio.rs`. Sin eso `inicio` leía mal el paquete.
- **Lo que `revision` añade al kernel y no estaba en x86**: la llamada 58 (`memoria_paginas`, el `RAM:` de una sesión)
  necesita `Espacio::paginas_de` en el espacio de x86 (hecho igual que en ARM; cada proyección guarda ahora su permiso
  para poner páginas nuevas). `grupo_uso` devuelve un cuarto resultado (las veces que topó): en x86 va en r10
  (`Marco::poner_x3`, y `dashos::grupo_topado` lo lee). La prueba de procesos de x86 pone el campo `nombres` nuevo.
- **`inicio_x86.rs` da las llaves que da `inicio.rs`**: "sistema", el permiso de máquinas virtuales, el de apagar, el
  del aviso de memoria y el del reloj. La red, como en `revision`: cliente de "archivos" y con "red-escuchar".
- **Pruebas de x86**: `arranque-x86` usa la red de las pruebas de `revision` (`RED_ANTES`/`RED_DESPUES`: el vecino del
  PC y el puerto reenviado; sin ellos QEMU no arrancaba). Dos líneas esperadas cambian: la raíz del disco (21
  entradas, el árbol de `SYS:` de `revision`) y el DHCP (con `restrict=on` ya no da puerta ni DNS).

Después, `revision` avanzó con la demo (ebb99cfc): unida sin conflictos (la opción `demo` es solo de ARM).

Las pruebas (`make prueba`, `arranque-virt`, `arranque-x86` y `prueba-x86`, con y sin `ACCEL=kvm`) pasaron con todo
el trabajo junto (la unión, la hora y `x86-uso`), no con cada commit por separado. `prueba-x86` con KVM falla en C28,
como antes de esta rama. `arranque-virt` falló una vez en "[dash-ventana] pulsado" y pasó a la segunda.

## 2026-11-16: `make -C kernel x86-uso`

- **La imagen**: opción `uso` del kernel en x86. `kernel/src/inicio_x86_uso.rs` es la lista de programas de `virt` con
  `uso`, en el mismo orden y con los mismos canales, con los controladores de un PC (NVMe y AHCI, xHCI, virtio-net,
  virtio-sound y HDA): consola, compositor, archivos, comprobador, eco, sesiones, red, fondo, centro, guardian (con
  "uso": la pantalla de entrada de verdad), imagen, sonido, avisos, barra, selector, panel, explorador, workbench y
  hora. Todos compilaban ya para x86 sin tocar nada.
- **Comprimidos con LZ4, como en ARM.** Con los 6,3 MB de programas sin comprimir el kernel iba de 1 MB a 8,1 MB y el
  cargador decía "la memoria de 0x100000 a 0x815000 está ocupada" (OVMF tiene cosas desde 8 MB). `build.rs` comprime
  los de `inicio_x86_uso.rs` con la opción `uso`, y `inicio_x86.rs` los descomprime al montar el paquete. Ahora el
  kernel llega a 0x61e000: quedan unos 1,9 MB hasta los 8 MB. Si el escritorio crece mucho más habrá que cargar el
  kernel más arriba o los programas aparte (pendiente).
- **El disco**: el de serie de las pruebas, con sus programas de x86 (`DASHOS_ARQ=x86_64` en
  `tools/disco_archivos.py`; el Shell y las órdenes, de `ordenes/` para `x86_64-unknown-none`). `target/nvme-x86.img`.
- **QEMU**: `tools/qemu_x86_a_mano.sh`, como `qemu_a_mano.sh`: q35 con OVMF, KVM y `-cpu host` si hay `/dev/kvm`,
  ventana GTK (`VENTANA=sdl` o `none`), tableta USB y teclado, el disco en NVMe (una copia), red de usuario de QEMU
  (virtio-net), sonido por HDA (`SONIDO_X86=virtio`) a PipeWire, pantalla a 1920x1080 (`PANTALLA=`), y QMP si se pide.
- **Sin caja Linux ni caja Amiga**: no están en la lista. Igual que en `virt-uso` la caja Amiga ya estaba fuera; la
  Linux sí está en ARM. `sesiones` tiene el permiso de máquinas, pero nadie lo pide.
- **La hora**: en x86 no se leía el reloj de la placa y la barra decía "--:--". Ahora el kernel lee el RTC del CMOS al
  arrancar (`base/x86_64/rtc.rs`, como el PL031 en ARM): "Hora: del reloj de la placa".

Cómo se arranca: `make -C kernel x86-uso` (`PANTALLA=1280x720`, `ACCEL=tcg` para no usar KVM).

## Lo comprobado (capturas en `~/recuperar/geosian-7d/x86-uso-capturas/`, fuera de git)

`tools/x86_uso_capturas.py` por QMP, sin ventana, a 1280x720 con KVM: la pantalla de entrada, entra Ana, el escritorio
(barra, Workbench con SYS:, Home, RAM y Work, el panel del monitor), el lanzador con "shell", el Shell con `List SYS:`,
su menú "Editar" dentro de la ventana, los menús en la barra (`Ajuste menus barra`) con "Shell" abierto, el
desplegable de procesos, Ajustes (Monitor del sistema, Apariencia, Acerca de, con la fecha), el panel rápido
(Super+A, red conectada) y el explorador en SYS:. Todo en unos 60 s desde que arranca QEMU.

## Lo que no funciona o no se ha visto en x86

- **Apagar** no apaga: en x86 `nucleos::apagar` solo sabe reiniciar (falta el S5 de ACPI; visto en el código, no probado desde el panel).
- Las cajas Linux y Amiga (arriba). Un programa 68k desde el explorador no tiene quien lo corra (igual que en
  `virt-uso`).
- La hora por la red: el disco de serie apunta `Prefs/Red/hora` al vecino de las pruebas (10.0.2.53), que en la red de
  usuario no está; `hora` reintenta sin fin. Igual en `virt-uso`. La hora sale del RTC.
- El panel del fondo enseña la hora en UTC y la barra en la de Madrid (no es de x86: es el mismo código; no mirado en
  ARM).
- No comprobado: la ventana GTK de verdad (todo se vio sin ventana, por QMP), que el sonido se oiga (con `AUDIO=none`),
  la red hacia fuera y TCG sin KVM en la imagen de usar.
