# DashOS: compilar el kernel y sacar la imagen que carga el firmware de la Pi (o QEMU raspi4b).
#
# make         la imagen normal, sin pruebas (target/kernel8.img)
# make prueba  la imagen con las pruebas del arranque y los programas de usuario que lanzan, y la prueba en QEMU
# make arranque  la imagen normal en QEMU: tiene que llegar a "DashOS en marcha"
# make virt    la imagen para QEMU virt (U3, docs/UEFI_LOOP.md), que carga el programa de UEFI
# make arranque-virt  esa imagen dentro del programa de UEFI, arrancada por EDK2 en QEMU virt
# make arranque-virt-2  lo mismo con dos monitores (virtio-gpu con dos cabezas)
# make arranque-virt-vm  la caja Linux (H9): la imagen de virt en QEMU con virtualization=on (EL2)
# make arranque-virt-linux  lo mismo, y la caja arranca Linux hasta busybox (LINUX=Image, INITRD=cpio.gz, fuera de git)
# make arranque-virt-arch  lo mismo con Arch Linux ARM desde la partición de la caja en el NVMe (fuera de git)
# make arranque-virt-wayland  lo mismo, con programas gráficos de Linux en ventanas de DashOS (el puente de Wayland)
# make arranque-virt-gpu  lo mismo, con programas de GL de Linux por Wayland y por la virtio-gpu de la caja (S25)
# make demo    el vídeo de la demo: el escritorio, la caja Amiga y la caja Linux, grabados (tools/demo.py)
# make pi5     la imagen para la Raspberry Pi 5 (target/kernel_2712.img), con el diagnóstico en pantalla
# make tarjeta-pi5  esa imagen en una tarjeta para grabar (~/.cache/dashos-pi5/dashos-pi5.img, tools/preparar_pi5.py)
# make arranque-virt-diag  el diagnóstico de la Pi 5 en QEMU virt (pantalla del firmware de UEFI): sus líneas y una captura
# make pi5-usb  la imagen del USB de la Pi 5 (target/dashos-usb.img); make arranque-virt-usb, su prueba en QEMU virt
BOOTLIN ?= $(HOME)/.local/opt/toolchains/aarch64be--glibc--stable-2026.08-1/bin
OBJCOPY ?= $(BOOTLIN)/aarch64_be-linux-objcopy
PIE     := ../usuario/target/pie
PERFIL  ?= release
ELF     := target/aarch64-unknown-none-softfloat/$(PERFIL)/dashos-kernel
IMAGEN  := target/kernel8.img
PRUEBAS := target/kernel8-pruebas.img
CARGO_PERFIL := $(if $(filter release,$(PERFIL)),--release,)

all: $(IMAGEN)

$(IMAGEN): usuario FORCE
	cargo build $(CARGO_PERFIL)
	$(OBJCOPY) -O binary $(ELF) $(IMAGEN)

usuario: kit-c FORCE
	cd ../usuario && cargo build --release --features archivos,ventanas,sonido,imagenes,red,caja,ordenes
	@# El Shell nativo y las órdenes de SYS:C (van al disco, no a la imagen del kernel).
	cd ../ordenes && cargo build --release
	cd ../usuario && cargo build --release --target aarch64_be-unknown-none-softfloat --bin hola --bin fpu_b
	$(BOOTLIN)/aarch64_be-linux-as -EL -o $(PIE).o ../usuario/asm/pie.s
	$(BOOTLIN)/aarch64_be-linux-ld -EL -pie --no-dynamic-linker -z noexecstack -z stack-size=32768 -o $(PIE) $(PIE).o
	@# H4: la caja Amiga (caja/), con el JIT de Emu68: solo si está el clon de third_party/Emu68 (toolchains/LEEME.md).
	if [ -d ../third_party/Emu68/src ]; then $(MAKE) -C ../caja; else echo "Sin third_party/Emu68: la caja Amiga no se compila"; fi

# Las ventanas de los programas de Dash (kit-c/): ../build/libkit_c.a, el kit con ABI de C que enlaza `./dash --dashos`
# cuando el programa usa mui_* (docs/bitacoras/dash-ventanas.md). Con el memcpy de la biblioteca dashos débil: manda el
# del runtime de Dash. Y su prueba, un programa de Dash con ventana (kit-c/pruebas/ventana.dash, en la imagen como
# `dash-ventana`, y su catálogo, en el disco de la prueba), si está Dash en DASH_DIR; si no, la imagen va sin ella.
KIT_C := ../build/libkit_c.a
DASH_DIR ?= $(HOME)/Projects/FreeMEMLang
kit-c: FORCE
	cd ../kit-c && cargo build --release
	mkdir -p ../build
	llvm-objcopy --weaken-symbol=memcpy ../kit-c/target/aarch64-unknown-none/release/libkit_c.a $(KIT_C)
	rm -f ../build/dash-ventana ../build/DashVentana.catalog
	if [ -x $(DASH_DIR)/dash ]; then DASHOS_DIR=$(abspath ..) $(DASH_DIR)/dash --dashos ../kit-c/pruebas/ventana.dash -o ../build/dash-ventana && python3 $(DASH_DIR)/tools/catalogo.py catalog ../kit-c/pruebas/ventana_es.ct --utf8 -o ../build/DashVentana.catalog || { echo "El Dash de $(DASH_DIR) no compila la prueba de kit-c: la imagen va sin ella"; rm -f ../build/dash-ventana; }; else echo "Sin Dash en $(DASH_DIR): la prueba de kit-c no se compila"; fi

$(PRUEBAS): usuario FORCE
	cargo build $(CARGO_PERFIL) --features pruebas
	$(OBJCOPY) -O binary $(ELF) $(PRUEBAS)

# Idiomas (paso 4 de docs/REVISION_LOOP.md): el árbol de `LOCALE:` (../build/Locale: catálogos, idiomas y países) que
# sale de ../locale con `tools/catalogo.py`. Falla si un .cd no está al día con los tr() del código, o si una traducción
# no cuadra con su original; entonces: python3 tools/catalogo.py todo -o build/Locale, y revisar los .ct.
catalogos: FORCE
	python3 ../tools/catalogo.py todo --comprobar -o ../build/Locale

# Antes de QEMU, las pruebas de PC de las bibliotecas de los servicios (B6): Wayland y la mezcla del sonido.
prueba: $(PRUEBAS) catalogos
	@# Idiomas y catálogos: el crate (catálogos rotos, locale.prefs roto, formatos) y lo que hace tools/catalogo.py.
	cd ../idioma && cargo test --quiet
	cd ../wayland && cargo test --quiet
	cd ../sonido && cargo test --quiet
	cd ../amigados && cargo test --quiet --release
	@# El Shell nativo: patrones, plantillas, líneas, rutas, el intérprete y la consola (hostiles incluidos).
	cd ../shell && cargo test --quiet
	cd ../archivos/herramientas && cargo test --quiet --release
	cd ../kit && cargo test --quiet
	cd ../caja-linux/vmm && cargo test --quiet
	$(MAKE) -C ../caja/pruebas_hunk
	mkdir -p target && rustc --edition 2024 --test src/rp1.rs -o target/rp1_pruebas && ./target/rp1_pruebas -q
	cd ../usuario && mkdir -p target && rustc --edition 2024 --test src/panel_mosaicos.rs -o target/panel_mosaicos_pruebas && ./target/panel_mosaicos_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/lanzables.rs -o target/lanzables_pruebas && ./target/lanzables_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/terminal.rs -o target/terminal_pruebas && ./target/terminal_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/protocolo/avisos.rs -o target/avisos_pruebas && ./target/avisos_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/cortafuegos_caja.rs -o target/cortafuegos_caja_pruebas && ./target/cortafuegos_caja_pruebas -q
	@# S27: el volumen y el OSD del panel, la lista del lanzador y la fecha de la barra.
	cd ../usuario && rustc --edition 2024 --test src/volumen_osd.rs -o target/volumen_osd_pruebas && ./target/volumen_osd_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/lanzador_lista.rs -o target/lanzador_lista_pruebas && ./target/lanzador_lista_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/reloj.rs -o target/reloj_pruebas && ./target/reloj_pruebas -q
	@# S13, segunda parte: DNS (respuestas falsas, nombres en bucle, cortadas), SNTP, las zonas horarias y el cortafuegos de entrada.
	cd ../usuario && rustc --edition 2024 --test src/dns.rs -o target/dns_pruebas && ./target/dns_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/sntp.rs -o target/sntp_pruebas && ./target/sntp_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/zona.rs -o target/zona_pruebas && ./target/zona_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/vigilancia.rs -o target/vigilancia_pruebas && ./target/vigilancia_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/protocolo/archivos.rs -o target/protocolo_archivos_pruebas && ./target/protocolo_archivos_pruebas -q
	@# S35, paso 2: virtio por mensajes entre virtio-red y el servicio de un dominio (mensajes malos, colas fuera de la ventana).
	cd ../usuario && rustc --edition 2024 --test src/protocolo/virtio_msg.rs -o target/virtio_msg_pruebas && ./target/virtio_msg_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/cortafuegos.rs -o target/cortafuegos_pruebas && ./target/cortafuegos_pruebas -q
	@# Trasfondo nativo: las cuentas del monitor (contadores raros, procesos que nacen y mueren) y sus ajustes.
	cd ../usuario && rustc --edition 2024 --test src/monitor.rs -o target/monitor_pruebas && ./target/monitor_pruebas -q
	@# El explorador de archivos: la lógica y los trabajos largos contra un servicio de archivos falso (hostiles incluidos).
	cd ../usuario && rustc --edition 2024 --test src/explorador/pruebas.rs -o target/explorador_pruebas && ./target/explorador_pruebas -q
	@# Paso 2 de las asignaciones: la tabla de nombres, resolver, capas, heredar y lo hostil, contra un servicio falso.
	cd ../usuario && rustc --edition 2024 --test src/asignaciones.rs -o target/asignaciones_pruebas && ./target/asignaciones_pruebas -q
	@# La tabla de llaves: buscar por nombre, y lo hostil (truncada, nombres raros, sin tabla).
	cd ../usuario && rustc --edition 2024 --test src/llaves.rs -o target/llaves_pruebas && ./target/llaves_pruebas -q
	@# El selector de ficheros: peticiones y respuestas (malas incluidas), nombres que no valen y patrones.
	cd ../usuario && rustc --edition 2024 --test src/protocolo/selector.rs -o target/selector_pruebas && ./target/selector_pruebas -q
	@# S6, S36: el registro común (líneas hostiles, quién ve qué, rotar, el ritmo) y los permisos en texto con su frase.
	cd ../usuario && rustc --edition 2024 --test src/registro.rs -o target/registro_pruebas && ./target/registro_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/permisos_texto.rs -o target/permisos_texto_pruebas && ./target/permisos_texto_pruebas -q
	@# S36, decisión 1: la tanda de acciones de administrador (listas malas y enormes, basura al azar, frases, respuesta).
	cd ../usuario && rustc --edition 2024 --test src/tanda.rs -o target/tanda_pruebas && ./target/tanda_pruebas -q
	@# S28: la pregunta de la guardia de ficheros de la caja Amiga (permisos sin llave, detalle, nombres raros).
	cd ../usuario && rustc --edition 2024 --test src/preguntas_ficheros.rs -o target/preguntas_ficheros_pruebas && ./target/preguntas_ficheros_pruebas -q
	../puente/pruebas/g0.sh
	../puente/pruebas/g1.sh
	../puente/pruebas/g4.sh && ../puente/pruebas/g5.sh && ../puente/pruebas/g6.sh
	../puente/pruebas/s29_espia.sh
	../tools/prueba_qemu.sh $(PRUEBAS) pruebas/esperado.txt 90

arranque: $(IMAGEN)
	../tools/prueba_qemu.sh $(IMAGEN) pruebas/arranque.txt 5

VIRT := target/kernel8-virt.img

virt: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt
	$(OBJCOPY) -O binary $(ELF) $(VIRT)

# Un disco de 8 MB para el NVMe (U6d): en el primer MB, un texto y la firma 0x55AA en el sector 0 y otro texto en el
# 1; desde el MB 1, un RedoxFS para el servicio de archivos (FS1, `tools/disco_archivos.py`). Nuevo en cada prueba: el
# cliente de U6f escribe el sector 2, y lo de una vuelta anterior no debe servir de prueba.
DISCO_NVME := target/nvme.img
DASHFS_AR := ../archivos/dashfs/target/release/dashfs-ar
VER := ../archivos/herramientas/target/release/ver
$(DISCO_NVME): catalogos FORCE
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar --bin dashfs-volume
	cd ../archivos/herramientas && cargo build --release
	python3 ../tools/disco_archivos.py $@ $(DASHFS_AR)

# La red (S13): una tarjeta virtio-net en un concentrador con la red de usuario de QEMU (DHCP, 10.0.2.15; el eco del PC
# en 10.0.2.100:7, `tools/red_eco.py`, lanzado por QEMU para cada conexión) y con `tools/red_basura.py`, que manda
# tramas malas por un socket Unix durante 20 s desde el primer TCP de DashOS (se para también al acabar la prueba).
RED := $(abspath target/red)
RED_MAC := 52:54:00:12:34:56
# S13, segunda parte. En el mismo concentrador, `tools/red_vecino.py`: otro aparato (10.0.2.53) con un servidor de
# nombres de mentira, la hora (SNTP) y un eco por UDP, para probar sin Internet. Y un puerto libre del PC (se busca
# antes de arrancar QEMU), que QEMU reenvía al 7777 de DashOS (`hostfwd`): por él entra `tools/red_entra.py`.
RED_ANTES := RED_PUERTO=$$(python3 -I -c "import socket; s = socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1])"); python3 -I ../tools/red_vecino.py $(RED) $(RED_MAC) 900 & VECINO=$$!; python3 -I ../tools/red_entra.py $$RED_PUERTO 900 & ENTRA=$$!
RED_DESPUES := kill $$BASURA $$VECINO $$ENTRA 2>/dev/null
# Con `restrict=on`, la red de usuario de QEMU no deja salir nada al PC ni a Internet (DHCP, `guestfwd` y `hostfwd` sí):
# ninguna prueba habla con fuera (ni el DNS de QEMU, 10.0.2.3, que reenviaría al del PC). Así su DHCP no da puerta de
# enlace ni servidor de nombres: DashOS no tiene ruta hacia fuera.
RED_QEMU := -netdev user,id=red0,restrict=on,hostfwd=tcp:127.0.0.1:$$RED_PUERTO-10.0.2.15:7777,guestfwd=tcp:10.0.2.100:7-cmd:$(abspath ../tools/red_eco.py) -netdev dgram,id=red5,local.type=unix,local.path=$(RED)/qemu2.sock,remote.type=unix,remote.path=$(RED)/vecino.sock -netdev hubport,id=red6,hubid=0,netdev=red5 -netdev dgram,id=red1,local.type=unix,local.path=$(RED)/qemu.sock,remote.type=unix,remote.path=$(RED)/pc.sock -netdev hubport,id=red2,hubid=0,netdev=red0 -netdev hubport,id=red3,hubid=0,netdev=red1 -netdev hubport,id=red4,hubid=0 -device virtio-net-pci,netdev=red4,disable-legacy=on,mac=$(RED_MAC),romfile=

# Al final, el guion pulsa Reiniciar en el menú de apagado del panel y se espera al segundo arranque (REINICIO,
# pruebas/arranque_virt_reinicio.txt; decisión del usuario del 2026-11-13).
arranque-virt: virt $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT)) cargo build --release
	rm -f target/explorador*.ppm target/captura.ppm target/amiga.ppm target/dash.ppm target/dash-nativo.ppm target/dash-nativo-pregunta.ppm target/gadtools.ppm target/gadtools-menu.ppm target/gadtools-dash.ppm target/requester.ppm target/reaction.ppm target/reaction.orig.ppm target/sonido.wav $(RED)/qemu.sock $(RED)/pc.sock $(RED)/qemu2.sock $(RED)/vecino.sock target/u6.ana.ppm target/u6.carla.ppm
	mkdir -p $(RED)
	python3 -I ../tools/red_basura.py $(RED) $(RED_MAC) 300 & BASURA=$$!; $(RED_ANTES); \
	GUARDAR=$(abspath target/salida_virt.txt) CAPTURA=$(abspath target/captura.ppm) CAPTURA_AMIGA=$(abspath target/amiga.ppm) CAPTURA_DASH=$(abspath target/dash.ppm) CAPTURA_DASHOS=$(abspath target/dash-nativo.ppm) CAPTURA_GT=$(abspath target/gadtools.ppm) CAPTURA_GT_MENU=$(abspath target/gadtools-menu.ppm) CAPTURA_GT_DASH=$(abspath target/gadtools-dash.ppm) CAPTURA_REQ=$(abspath target/requester.ppm) CAPTURA_RA=$(abspath target/reaction) CAPTURA_EXPLORADOR=$(abspath target/explorador) CAPTURA_U6=$(abspath target/u6) CAPTURA_MONITOR=$(abspath target/monitor) REINICIO=$(abspath pruebas/arranque_virt_reinicio.txt) TECLAS=1 MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2 -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv -audiodev wav,id=snd0,path=$(abspath target/sonido.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0 $(RED_QEMU)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt.txt $(or $(TIEMPO_VIRT),600); \
	R=$$?; $(RED_DESPUES); exit $$R
	python3 ../tools/disco_archivos.py --comprobar $(DISCO_NVME) $(VER)
	python3 ../tools/captura_ventanas.py target/captura.ppm
	python3 ../tools/sonido_wav.py target/sonido.wav
	python3 ../tools/captura_cambio.py target/u6.ana.ppm target/u6.carla.ppm

# La imagen para usar a mano (`tools/qemu_a_mano.sh`): `virt` con `uso`, sin los programas de prueba y con la pantalla
# de entrada de verdad. No es una prueba: nada la comprueba.
VIRT_USO := target/kernel8-virt-uso.img
virt-uso: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,uso
	$(OBJCOPY) -O binary $(ELF) $(VIRT_USO)

# El Shell nativo en pantalla (docs/bitacoras/shell-nativa.md, "Visto en pantalla"): la imagen de usar (`virt-uso`),
# con una copia del disco de pruebas. `tools/guion_shell.py` entra como Ana, abre el Shell desde el lanzador de la barra
# y teclea en él con el teclado de QEMU (mapa de teclado y modificadores de verdad), con una captura en cada momento:
# recién abierto, `List`, la sugerencia, una orden que no existe, el menú de Tab, dos paneles, una segunda pestaña y la
# ventana movida por su barra (target/shell/shell.*.png; y en capturas-agente/, fuera de git). Al final la cierra con el
# botón de su barra.
SHELL_CAPTURAS := 1-recien-abierto 2-list 3-sugerencia 4-orden-mala 5-menu-tab 6-paneles 7-pestana 8-titulo
arranque-virt-shell: virt-uso $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_USO)) cargo build --release
	rm -rf target/shell && mkdir -p target/shell
	cp --reflink=auto $(DISCO_NVME) target/shell/disco.img
	GUARDAR=$(abspath target/salida_virt_shell.txt) SHELL_CAPTURAS=$(abspath target/shell/shell) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/shell/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_shell.txt 300
	@cat target/shell/shell.guion.txt
	for c in $(SHELL_CAPTURAS); do test -s target/shell/shell.$$c.png || { echo "FALTA la captura $$c"; exit 1; }; done
	mkdir -p ../capturas-agente && for c in $(SHELL_CAPTURAS); do cp target/shell/shell.$$c.png ../capturas-agente/shell-$$c.png; done

# S10, los menús de los programas (docs/bitacoras/menus.md): la imagen de usar con la prueba de kit-c (`dash-ventana`)
# y una copia del disco de pruebas. `tools/guion_menus.py` entra como Ana y mira los menús del Shell (el kit nativo) y
# de `dash-ventana` (kit-c) dentro de la ventana; los pasa a la barra con `Ajuste menus barra` (tecleado en el Shell),
# los abre allí con un clic y con el botón derecho, elige en ellos, y vuelve dentro; con una captura en cada momento
# (target/menus/menus.*.png; y en ~/recuperar/geosian-7d/menus-capturas/, fuera de git).
VIRT_MENUS := target/kernel8-virt-menus.img
MENUS_CAPTURAS := 01-kitc-dentro 02-kitc-dentro-abierto 03-shell-dentro 04-shell-dentro-abierto 05-barra-shell 06-barra-shell-abierto 07-barra-elegido 08-barra-boton-derecho 09-barra-kitc 10-barra-kitc-abierto 11-kitc-elegido 12-dentro-otra-vez 13-f10
MENUS_FUERA ?= $(HOME)/recuperar/geosian-7d/menus-capturas
arranque-virt-menus: usuario $(DISCO_NVME) FORCE
	cargo build $(CARGO_PERFIL) --features virt,uso,menus
	$(OBJCOPY) -O binary $(ELF) $(VIRT_MENUS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_MENUS)) cargo build --release
	rm -rf target/menus && mkdir -p target/menus
	cp --reflink=auto $(DISCO_NVME) target/menus/disco.img
	GUARDAR=$(abspath target/salida_virt_menus.txt) MENUS_CAPTURAS=$(abspath target/menus/menus) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/menus/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_menus.txt 400
	@cat target/menus/menus.guion.txt
	for c in $(MENUS_CAPTURAS); do test -s target/menus/menus.$$c.png || { echo "FALTA la captura $$c"; exit 1; }; done
	mkdir -p $(MENUS_FUERA) && for c in $(MENUS_CAPTURAS); do cp target/menus/menus.$$c.png $(MENUS_FUERA)/$$c.png; done

# La caja Linux (H9, docs/bitacoras/caja-linux.md): la misma imagen de `virt` en una QEMU virt con virtualization=on,
# sin disco ni USB. EDK2 y el cargador entran en EL2: el kernel deja ahí el trozo del hipervisor y el servicio
# `caja-linux` corre sus invitados de prueba. Sin virtualization=on (`arranque-virt`), el servicio dice que no hay
# hipervisor.
arranque-virt-vm: virt
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT)) cargo build --release
	GUARDAR=$(abspath target/salida_virt_vm.txt) MAQUINA=highmem=off,virtualization=on QEMU_EXTRA="-device ramfb" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_vm.txt 120
	grep -E "\[(caja-linux|sin-maquina)\]|Hipervisor" target/salida_virt_vm.txt

# La caja Linux con un Linux de verdad: lo mismo con la Image de un kernel de Linux para arm64 y su initramfs, que no
# van en git (`tools/caja_linux_invitado.py preparar` los deja en ~/.cache/dashos-caja-linux: Image-caja, un Linux 6.18
# pequeño de 5 MB, y busybox).
# Entran en el paquete de arranque (opción `linux`) y `caja-linux` los arranca en una máquina de 256 MB con los
# aparatos del crate caja-linux/vmm (PL011, virtio-console, virtio-blk), el GIC virtual y el temporizador virtual,
# hasta que el /init de busybox lee y escribe el disco y apaga la máquina por PSCI. LINUX=Image, INITRD=cpio.gz. Con
# 3 GB (2 GB para procesos): con 2 la caja no tiene sitio para los 256 MB de su invitado y se queda con menos.
# El teclado y el ratón USB, para teclear en la terminal de la caja (tools/guion_caja_linux.py), y la red de QEMU (DHCP
# para DashOS, 10.0.2.2 de puerta de enlace) para la del invitado.
USB_QEMU := -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2
RED_CAJA := -netdev user,id=red0 -device virtio-net-pci,netdev=red0,disable-legacy=on,mac=$(RED_MAC),romfile=
# Sin `restrict=on`: con él, el DHCP de QEMU no da puerta de enlace, y el invitado de la caja Linux (su ping a la puerta
# en `arranque-virt-linux`, `pacman -Sy` en las de Arch) no tendría a quién hablar. DashOS no sale por su cuenta: en
# `arranque-virt-linux` (sin disco) no tiene servidor de hora, y en las de Arch el suyo es el vecino (10.0.2.53).
LINUX ?= $(HOME)/.cache/dashos-caja-linux/Image-caja
INITRD ?= $(HOME)/.cache/dashos-caja-linux/initramfs.cpio.gz
VIRT_LINUX := target/kernel8-virt-linux.img
arranque-virt-linux: usuario FORCE
	@test -f "$(LINUX)" || { echo "Falta el kernel de Linux (LINUX=$(LINUX)): no va en git; tools/caja_linux_invitado.py preparar"; exit 1; }
	@test -f "$(INITRD)" || { echo "Falta el initramfs (INITRD=$(INITRD)): tools/caja_linux_invitado.py preparar"; exit 1; }
	mkdir -p target/linux
	cp "$(LINUX)" target/linux/Image
	cp "$(INITRD)" target/linux/initrd
	cargo build $(CARGO_PERFIL) --features virt,linux
	$(OBJCOPY) -O binary $(ELF) $(VIRT_LINUX)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_LINUX)) cargo build --release
	rm -f target/caja-terminal.ppm target/caja-terminal.png target/caja-terminal.fin.ppm
	GUARDAR=$(abspath target/salida_virt_linux.txt) MEMORIA=3G MAQUINA=highmem=off,virtualization=on CAJA_LINUX=$(abspath target/caja-terminal) QEMU_EXTRA="-device ramfb $(USB_QEMU) $(RED_CAJA)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_linux.txt 400
	grep -E "\[(caja-linux|sin-maquina)\]|Hipervisor" target/salida_virt_linux.txt | grep -vE "linux: (\[ |[0-9]+$$)" || true
	@# La captura de la terminal, fuera de git (capturas-agente/ está en .git/info/exclude).
	mkdir -p ../capturas-agente && cp target/caja-terminal.png ../capturas-agente/caja-linux-terminal.png 2>/dev/null || true

# Un dominio de controladores (S35, paso 2; docs/bitacoras/dominio-red.md): la imagen de `virt` con `dominio`, en QEMU
# con virtualization=on. La red de DashOS no tiene virtio-net-pci: tiene una e1000 que DashOS presta (sin IOMMU, solo
# en QEMU) a un Linux mínimo en una máquina virtual (`dominio`); ese Linux la lleva y devuelve a DashOS una virtio-net
# por mensajes, que lleva `virtio-red` de siempre. La e1000 está en el concentrador con la red de usuario de QEMU (DHCP
# y el eco del PC en 10.0.2.100:7, con restrict=on: nada sale del PC) y el vecino de prueba (`tools/red_vecino.py`,
# 10.0.2.53). `dominio-prueba` hace DHCP, ecos TCP y UDP, y relanza el dominio en modos hostiles (oferta falsa,
# descriptores que no son suyos, leer fuera de su memoria, colgarse y morir a media transferencia). El kernel y el
# initramfs del dominio, fuera de git: `tools/dominio_red.py preparar` (DOMINIO_IMAGE, DOMINIO_INITRD). Con el disco de
# las pruebas (NVMe): así `sesiones` escribe el registro común, donde quedan las caídas del dominio, y la prueba lo lee.
DOMINIO_DIR ?= $(HOME)/recuperar/geosian-7d/dominio-red
DOMINIO_IMAGE ?= $(DOMINIO_DIR)/Image
DOMINIO_INITRD ?= $(DOMINIO_DIR)/initramfs.cpio
VIRT_DOMINIO := target/kernel8-virt-dominio.img
RED_DOMINIO := -netdev user,id=red0,restrict=on,guestfwd=tcp:10.0.2.100:7-cmd:$(abspath ../tools/red_eco.py) -netdev dgram,id=red5,local.type=unix,local.path=$(RED)/qemu2.sock,remote.type=unix,remote.path=$(RED)/vecino.sock -netdev hubport,id=red6,hubid=0,netdev=red5 -netdev hubport,id=red2,hubid=0,netdev=red0 -netdev hubport,id=red4,hubid=0 -device e1000,netdev=red4,mac=$(RED_MAC),romfile=
arranque-virt-dominio: usuario $(DISCO_NVME) FORCE
	@test -f "$(DOMINIO_IMAGE)" || { echo "Falta el kernel del dominio (DOMINIO_IMAGE=$(DOMINIO_IMAGE)): no va en git; tools/dominio_red.py preparar"; exit 1; }
	@test -f "$(DOMINIO_INITRD)" || { echo "Falta el initramfs del dominio (DOMINIO_INITRD=$(DOMINIO_INITRD)): tools/dominio_red.py preparar"; exit 1; }
	mkdir -p target/dominio
	cp "$(DOMINIO_IMAGE)" target/dominio/Image
	cp "$(DOMINIO_INITRD)" target/dominio/initrd
	cargo build $(CARGO_PERFIL) --features virt,dominio
	$(OBJCOPY) -O binary $(ELF) $(VIRT_DOMINIO)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_DOMINIO)) cargo build --release
	rm -f $(RED)/qemu2.sock $(RED)/vecino.sock
	mkdir -p $(RED)
	python3 -I ../tools/red_vecino.py $(RED) $(RED_MAC) 900 & VECINO=$$!; \
	GUARDAR=$(abspath target/salida_virt_dominio.txt) MEMORIA=3G MAQUINA=highmem=off,virtualization=on QEMU_EXTRA="-device ramfb -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv $(RED_DOMINIO)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_dominio.txt $(or $(TIEMPO_DOMINIO),900); \
	R=$$?; kill $$VECINO 2>/dev/null; exit $$R
	grep -E "\[(dominio|dominio-prueba|virtio-red)\]" target/salida_virt_dominio.txt | grep -vE "linux: \[ " || true

# Arch Linux ARM en la caja Linux (H9, docs/bitacoras/caja-linux.md): la Image de Image-arch (tinyconfig con lo que
# pide systemd), sin initramfs, y el disco de Arch como NVMe: una GPT con la partición de DashOS (en ella, el RedoxFS de
# $(DISCO_NVME), copiado en cada prueba) y la de la caja Linux, con un ext4 con el rootfs oficial de Arch Linux ARM
# para aarch64 (con lo mínimo cambiado: getty entra como root en hvc0, sin
# timesyncd ni sshd). `tools/caja_linux_invitado.py preparar-arch` deja los dos en ~/.cache/dashos-caja-linux (fuera
# de git). El servicio de disco da esa partición a `caja-linux`, que la ofrece al invitado como virtio-blk; el invitado
# arranca el systemd de Arch, toma dirección por DHCP (por el puente de tramas del servicio de red) y el guion
# (`tools/guion_caja_linux.py`, modo arch) teclea en su terminal: uname, systemctl, ip, date, `pacman -Sy` y poweroff.
# El disco va con snapshot=on: lo que escribe Arch no se guarda. Con 3 GB, el invitado tiene 256 MB: el kernel de DashOS
# solo usa la RAM por debajo de los 4 GB de direcciones físicas (su mapa lineal), y en `virt` empieza en el GB 1.
ARCH_IMAGE ?= $(HOME)/.cache/dashos-caja-linux/Image-arch
ARCH_DISCO ?= $(HOME)/.cache/dashos-caja-linux/arch/disco-arch.img
# Cada prueba trabaja sobre su copia (en btrfs, `--reflink` la hace al momento): `poner-dashos` escribe DashOS en el
# disco y, con el de la caché, dos pruebas a la vez (o dos worktrees) se pisaban y fallaban sin motivo.
DISCO_ARCH_COPIA := target/disco-arch.img
VIRT_ARCH := target/kernel8-virt-arch.img
arranque-virt-arch: usuario $(DISCO_NVME) FORCE
	@test -f "$(ARCH_IMAGE)" || { echo "Falta el kernel (ARCH_IMAGE=$(ARCH_IMAGE)): no va en git; tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	@test -f "$(ARCH_DISCO)" || { echo "Falta el disco de Arch (ARCH_DISCO=$(ARCH_DISCO)): tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	mkdir -p target/linux
	cp "$(ARCH_IMAGE)" target/linux/Image
	: > target/linux/initrd
	cp --reflink=auto "$(ARCH_DISCO)" $(DISCO_ARCH_COPIA)
	python3 ../tools/caja_linux_invitado.py poner-dashos $(DISCO_NVME) $(DISCO_ARCH_COPIA)
	cargo build $(CARGO_PERFIL) --features virt,linux
	$(OBJCOPY) -O binary $(ELF) $(VIRT_ARCH)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_ARCH)) cargo build --release
	rm -f target/caja-arch.ppm target/caja-arch.png
	GUARDAR=$(abspath target/salida_virt_arch.txt) MEMORIA=3G MAQUINA=highmem=off,virtualization=on CAJA_LINUX=$(abspath target/caja-arch) CAJA_MODO=arch QEMU_EXTRA="-device ramfb $(USB_QEMU) $(RED_CAJA) -drive file=$(abspath $(DISCO_ARCH_COPIA)),if=none,id=nv,format=raw,snapshot=on -device nvme,serial=caja,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_arch.txt 1800
	grep -E "\[(caja-linux|nvme)\]" target/salida_virt_arch.txt | grep -vE "linux: (\[ |[0-9]+$$)" | grep -vE "hostil|invitado dice" || true
	@# La captura de la terminal, fuera de git (capturas-agente/ está en .git/info/exclude).
	mkdir -p ../capturas-agente && cp target/caja-arch.png ../capturas-agente/caja-linux-arch.png 2>/dev/null || true

# Las ventanas de Linux en el escritorio de DashOS (H9, el puente de Wayland; docs/bitacoras/caja-linux.md): lo mismo
# que `arranque-virt-arch` (el mismo disco: `preparar-arch` mete en él el puente, `ventana_wayland` y foot), pero el
# guion (modo wayland) abre programas gráficos de Linux: el puente de Arch los pasa por vsock a la caja y la caja al
# compositor como subconexiones. Antes, `puente --hostil` manda tramas rotas por vsock. Capturas en capturas-agente/.
# Desde caja-linux-apps: la barra de DashOS (xdg-decoration), el portapapeles entre foot y la terminal de la caja, los
# hostiles de Linux (`puente --cliente-hostil`), GTK4 (gtk4-widget-factory) y Firefox;
# las medidas del guion, en target/caja-wayland.medidas.txt. Sin Firefox: CAJA_FIREFOX=0.
# El disco de los programas grandes, en su propia caché: DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch.
# Desde caja-linux-grandes, con 8 GB y la RAM por encima de los 4 GB (highmem, pero con el ECAM y las ventanas del PCIe
# por debajo, donde las busca el kernel): DashOS da 512 MB al invitado (la regla de serie, `dashos::maquina`), y GTK4
# carga sus iconos con el zram del invitado; Firefox cabe. DASHOS_CAJA_LINUX_MB=1024 (al compilar) da otra cantidad al
# invitado. Hace falta el disco de los programas grandes rehecho desde caja-linux-grandes (zram en Image-arch).
MAQUINA_GRANDE := highmem=on,highmem-ecam=off,highmem-mmio=off
MEMORIA_GRANDE ?= 8G
# Con CAJA_SOLO=gtk, firefox o grandes (solo eso, tools/guion_caja_linux.py), otras líneas esperadas.
ESPERADO_WAYLAND ?= pruebas/arranque_virt_wayland.txt
APPS_IMAGE ?= $(HOME)/.cache/dashos-caja-linux-apps/Image-arch
APPS_DISCO ?= $(HOME)/.cache/dashos-caja-linux-apps/arch/disco-arch.img
arranque-virt-wayland: ARCH_IMAGE = $(APPS_IMAGE)
arranque-virt-wayland: ARCH_DISCO = $(APPS_DISCO)
arranque-virt-wayland: usuario $(DISCO_NVME) FORCE
	@test -f "$(ARCH_IMAGE)" || { echo "Falta el kernel (APPS_IMAGE=$(ARCH_IMAGE)): no va en git; DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	@test -f "$(ARCH_DISCO)" || { echo "Falta el disco de Arch (APPS_DISCO=$(ARCH_DISCO)): DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	mkdir -p target/linux
	cp "$(ARCH_IMAGE)" target/linux/Image
	: > target/linux/initrd
	cp --reflink=auto "$(ARCH_DISCO)" $(DISCO_ARCH_COPIA)
	python3 ../tools/caja_linux_invitado.py poner-dashos $(DISCO_NVME) $(DISCO_ARCH_COPIA)
	cargo build $(CARGO_PERFIL) --features virt,linux
	$(OBJCOPY) -O binary $(ELF) $(VIRT_ARCH)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_ARCH)) cargo build --release
	rm -f target/caja-wayland*.ppm target/caja-wayland*.png target/caja-wayland.medidas.txt
	GUARDAR=$(abspath target/salida_virt_wayland.txt) MEMORIA=$(MEMORIA_GRANDE) MAQUINA=$(MAQUINA_GRANDE),virtualization=on CAJA_LINUX=$(abspath target/caja-wayland) CAJA_MODO=wayland QEMU_EXTRA="-device ramfb $(USB_QEMU) $(RED_CAJA) -drive file=$(abspath $(DISCO_ARCH_COPIA)),if=none,id=nv,format=raw,snapshot=on -device nvme,serial=caja,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi $(ESPERADO_WAYLAND) 4500
	grep -E "\[(caja-linux|compositor)\]" target/salida_virt_wayland.txt | grep -E "wayland|puente|foot|ventana|terminal|hostil|gtk|firefox|arrastre" | grep -vE "linux: \[ " || true
	cat target/caja-wayland.medidas.txt 2>/dev/null || true
	@# Las capturas, fuera de git (capturas-agente/ está en .git/info/exclude).
	mkdir -p ../capturas-agente && cp target/caja-wayland.png ../capturas-agente/caja-linux-wayland.png 2>/dev/null || true
	cp target/caja-wayland.foot.png ../capturas-agente/caja-linux-wayland-foot.png 2>/dev/null || true
	cp target/caja-wayland.grande.png ../capturas-agente/caja-linux-wayland-grande.png 2>/dev/null || true
	for c in portapapeles gtk gtk-menu gtk-grande firefox; do cp target/caja-wayland.$$c.png ../capturas-agente/caja-linux-wayland-$$c.png 2>/dev/null || true; done

# S26 (docs/diseño/S26-aspecto-de-las-cajas.md): el aspecto de los programas de la caja Linux. Lo mismo que
# `arranque-virt-wayland`, con la imagen de `tools/aspecto_caja_linux.py preparar` (la de los programas grandes, con GTK,
# más el agente del aspecto: ARCH_IMAGE y ARCH_DISCO de su caché) y los Prefs del usuario de ASPECTO_PREFS (fondo,
# apariencia y aspecto de las cajas): el centro los pone al arrancar, el compositor los reparte y el agente de Arch
# escribe el tema de GTK. El guion (modo aspecto) abre una muestra de GTK 4 y foot, y captura cada uno
# en capturas-agente/aspecto-$(ASPECTO_NOMBRE)-{gtk4,foot}.png. Para Full HD: DASHOS_PANTALLA=1920x1080.
ASPECTO_PREFS ?= fondo=ondas.png;apariencia=claro fondo;cajas=general dashos
ASPECTO_NOMBRE ?= dashos-ondas
arranque-virt-aspecto: usuario catalogos FORCE
	@test -f "$(ARCH_IMAGE)" || { echo "Falta el kernel (ARCH_IMAGE=$(ARCH_IMAGE)): tools/aspecto_caja_linux.py preparar"; exit 1; }
	@test -f "$(ARCH_DISCO)" || { echo "Falta el disco de Arch (ARCH_DISCO=$(ARCH_DISCO)): tools/aspecto_caja_linux.py preparar"; exit 1; }
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar --bin dashfs-volume
	cd ../archivos/herramientas && cargo build --release
	DASHOS_PREFS="$(ASPECTO_PREFS)" python3 ../tools/disco_archivos.py target/nvme-aspecto.img $(DASHFS_AR)
	mkdir -p target/linux
	cp "$(ARCH_IMAGE)" target/linux/Image
	: > target/linux/initrd
	python3 ../tools/caja_linux_invitado.py poner-dashos target/nvme-aspecto.img "$(ARCH_DISCO)"
	cargo build $(CARGO_PERFIL) --features virt,linux
	$(OBJCOPY) -O binary $(ELF) $(VIRT_ARCH)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_ARCH)) cargo build --release
	rm -f target/caja-aspecto*.ppm target/caja-aspecto*.png
	GUARDAR=$(abspath target/salida_virt_aspecto.txt) MEMORIA=3G MAQUINA=highmem=off,virtualization=on CAJA_LINUX=$(abspath target/caja-aspecto) CAJA_MODO=aspecto QEMU_EXTRA="-device ramfb $(USB_QEMU) $(RED_CAJA) -drive file=$(ARCH_DISCO),if=none,id=nv,format=raw,snapshot=on -device nvme,serial=caja,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_aspecto.txt 1800
	grep -E "\[(caja-linux|compositor|centro|caja-amiga)\]" target/salida_virt_aspecto.txt | grep -E "aspecto|muestra|tema|al arrancar|foot" | grep -vE "linux: \[ " || true
	mkdir -p ../capturas-agente && for q in gtk4 foot; do cp target/caja-aspecto.$$q.png ../capturas-agente/aspecto-$(ASPECTO_NOMBRE)-$$q.png 2>/dev/null || true; done

# Los gráficos de la caja Linux (S25, docs/diseño/S25-gpu-caja-linux.md): lo mismo que `arranque-virt-wayland` (el mismo
# disco, con Mesa, mesa-utils, vulkan-swrast y `cuadros_gl` de `preparar-arch`), pero el guion (modo gpu) mide
# programas de GL de Linux: `cuadros_gl wayland` y `es2gears_wayland` por el puente de Wayland (búferes de wl_shm por
# vsock) y `cuadros_gl kms` por la virtio-gpu de la caja (su pantalla sale en una ventana de DashOS). Capturas en
# capturas-agente/.
arranque-virt-gpu: usuario $(DISCO_NVME) FORCE
	@test -f "$(ARCH_IMAGE)" || { echo "Falta el kernel (ARCH_IMAGE=$(ARCH_IMAGE)): no va en git; tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	@test -f "$(ARCH_DISCO)" || { echo "Falta el disco de Arch (ARCH_DISCO=$(ARCH_DISCO)): tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	mkdir -p target/linux
	cp "$(ARCH_IMAGE)" target/linux/Image
	: > target/linux/initrd
	cp --reflink=auto "$(ARCH_DISCO)" $(DISCO_ARCH_COPIA)
	python3 ../tools/caja_linux_invitado.py poner-dashos $(DISCO_NVME) $(DISCO_ARCH_COPIA)
	cargo build $(CARGO_PERFIL) --features virt,linux
	$(OBJCOPY) -O binary $(ELF) $(VIRT_ARCH)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_ARCH)) cargo build --release
	rm -f target/caja-gpu*.ppm target/caja-gpu*.png
	GUARDAR=$(abspath target/salida_virt_gpu.txt) MEMORIA=3G MAQUINA=highmem=off,virtualization=on CAJA_LINUX=$(abspath target/caja-gpu) CAJA_MODO=gpu QEMU_EXTRA="-device ramfb $(USB_QEMU) $(RED_CAJA) -drive file=$(abspath $(DISCO_ARCH_COPIA)),if=none,id=nv,format=raw,snapshot=on -device nvme,serial=caja,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_gpu.txt 1800
	grep -E "\[(caja-linux|compositor)\]" target/salida_virt_gpu.txt | grep -E "gpu|cuadros_gl|FPS|frames|ventana" | grep -vE "linux: \[ " || true
	@# Las capturas, fuera de git (capturas-agente/ está en .git/info/exclude).
	mkdir -p ../capturas-agente
	for c in wayland gears kms; do cp target/caja-gpu.$$c.png ../capturas-agente/caja-linux-gpu-$$c.png 2>/dev/null || true; done

# El multimonitor en QEMU virt: lo mismo que `arranque-virt` y, además, una virtio-gpu con dos cabezas (la 0 a 800x600,
# como la ramfb; la 1, sin monitor, sale a 1024x768). Sin el guion de siempre: `tools/guion_multimonitor.py` (MULTI)
# lleva el cursor de una a otra, pasa Ajustes a la segunda con Super+Mayúsculas+Derecha, la maximiza allí, la devuelve
# y la trae, baja el centro de control a Pantallas y captura cada cabeza (target/multi.*.ppm);
# `tools/captura_multimonitor.py` las mira. Para en cuanto salen las líneas de `pruebas/arranque_virt_2.txt`.
arranque-virt-2: virt $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT)) cargo build --release
	rm -f target/multi.*.ppm $(RED)/qemu.sock $(RED)/pc.sock $(RED)/qemu2.sock $(RED)/vecino.sock
	mkdir -p $(RED)
	python3 -I ../tools/red_basura.py $(RED) $(RED_MAC) 300 & BASURA=$$!; $(RED_ANTES); \
	GUARDAR=$(abspath target/salida_virt_2.txt) MULTI=$(abspath target/multi) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -device virtio-gpu-pci,id=vgpu,max_outputs=2,xres=800,yres=600 -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2 -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv -audiodev none,id=snd0 -device virtio-sound-pci,audiodev=snd0 $(RED_QEMU)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_2.txt 200; \
	R=$$?; $(RED_DESPUES); exit $$R
	python3 ../tools/captura_multimonitor.py target/multi
	grep "en 10 s" target/salida_virt_2.txt | tail -5

# El vídeo de la demo (docs/bitacoras/demo.md): la imagen de usar (`uso`) con la caja Linux de Arch y sus programas
# grandes (el disco de `arranque-virt-wayland`) y la caja Amiga de las capturas (CAJA_INTERACTIVA=1, en caja/build-demo),
# con el AmigaOS del usuario (DEMO_AMIGAOS) y sus programas 68k (DEMO_PROGRAMAS: fichero:fichero..., con el nombre que
# llevarán en el disco delante de un "=" si se quiere otro). Nada de eso va en git. `tools/demo.py` arranca QEMU (8 GB,
# tableta USB, a DEMO_PANTALLA), entra, recorre el escritorio con el teclado y el ratón (monitor de QEMU y QMP),
# esperando cada cosa en la salida serie, y graba la pantalla (`screendump` a ritmo fijo): fotogramas y marcas de
# tiempo en DEMO_DIR/grabacion. El vídeo se monta después con `tools/demo_montar.py`. DEMO_GUION=no: solo arranca y
# deja los sockets, para manejarla a mano (`tools/demo.py orden`).
DEMO_AMIGAOS ?= $(HOME)/.cache/dash-emu/sys32mui
DEMO_PROGRAMAS ?= Tachyon=$(HOME)/Projects/tachyon/build/dashweb:$(HOME)/Projects/tachyon/iconos:Calculator=$(HOME)/recuperar/tachyon-scratch/sys39x/Workbench/Tools/Calculator:Forja=$(HOME)/Projects/forja/build/Forja
DEMO_PANTALLA ?= 1280x720
DEMO_DIR ?= $(HOME)/recuperar/geosian-7d/demo
DEMO_GUION ?= si
VIRT_DEMO := target/kernel8-virt-demo.img
demo: usuario catalogos FORCE
	@test -f "$(APPS_IMAGE)" || { echo "Falta el kernel de Arch (APPS_IMAGE=$(APPS_IMAGE)): DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	@test -f "$(APPS_DISCO)" || { echo "Falta el disco de Arch (APPS_DISCO=$(APPS_DISCO)): DASHOS_CAJA_APPS=1 tools/caja_linux_invitado.py preparar-arch"; exit 1; }
	$(MAKE) -C ../caja B=build-demo CAJA_INTERACTIVA=1
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar --bin dashfs-volume
	cd ../archivos/herramientas && cargo build --release
	rm -rf target/demo && mkdir -p target/demo/programas target/linux
	python3 ../tools/demo.py programas "$(DEMO_PROGRAMAS)" target/demo/programas > target/demo/programas.txt
	DASHOS_AMIGAOS="$(DEMO_AMIGAOS)" DASHOS_PROGRAMAS_AMIGA="$$(cat target/demo/programas.txt)" python3 ../tools/disco_archivos.py target/demo/nvme.img $(DASHFS_AR)
	cp "$(APPS_IMAGE)" target/linux/Image
	: > target/linux/initrd
	python3 ../tools/demo.py disco target/demo/nvme.img "$(APPS_DISCO)" target/demo/disco-arch.img
	cargo build $(CARGO_PERFIL) --features virt,uso,linux,demo
	$(OBJCOPY) -O binary $(ELF) $(VIRT_DEMO)
	cd ../uefi && DASHOS_PANTALLA=$(DEMO_PANTALLA) DASHOS_NUCLEO=$(abspath $(VIRT_DEMO)) cargo build --release
	python3 ../tools/demo.py grabar --efi ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi --disco target/demo/disco-arch.img --pantalla $(DEMO_PANTALLA) --dir $(DEMO_DIR) --guion $(DEMO_GUION)

# Los cortes del sonido con los cuatro núcleos ocupados (S19, A6): la imagen de virt con cuatro programas de carga,
# sin disco ni USB. Mide; el WAV se comprueba, pero un fallo de la comprobación no para la orden.
VIRT_CARGA := target/kernel8-virt-carga.img
sonido-carga: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,carga
	$(OBJCOPY) -O binary $(ELF) $(VIRT_CARGA)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_CARGA)) cargo build --release
	rm -f target/sonido-carga.wav
	GUARDAR=$(abspath target/salida_carga.txt) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -audiodev wav,id=snd0,path=$(abspath target/sonido-carga.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/sonido_carga.txt 90
	grep -E "\[(sonido|carga)\]" target/salida_carga.txt
	-python3 ../tools/sonido_wav.py target/sonido-carga.wav

# audio.device nativo (S19, fase 2): la imagen de virt con `paula-prueba` en lugar de los dos tonos, sin disco ni
# USB. El WAV se comprueba con `sonido_wav.py --paula`.
VIRT_PAULA := target/kernel8-virt-paula.img
sonido-paula: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,paula
	$(OBJCOPY) -O binary $(ELF) $(VIRT_PAULA)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_PAULA)) cargo build --release
	rm -f target/sonido-paula.wav
	GUARDAR=$(abspath target/salida_paula.txt) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -audiodev wav,id=snd0,path=$(abspath target/sonido-paula.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/sonido_paula.txt 60
	python3 ../tools/sonido_wav.py --paula target/sonido-paula.wav

# El banco de medidas de la multitarea (M0, docs/MULTITAREA_LOOP.md): la imagen de raspi4b con solo la consola y el
# orquestador `medir`, y los hijos de prueba en Rust, C y Dash. VUELTAS arranques; medianas en target/medir.json.
MEDIR := target/kernel8-medir.img
VUELTAS ?= 7
medir: usuario FORCE
	python3 ../tools/medir.py paquete ../usuario/target/medir.paq
	cargo build $(CARGO_PERFIL) --features medir
	$(OBJCOPY) -O binary $(ELF) $(MEDIR)
	python3 ../tools/medir.py correr $(MEDIR) target/medir.json -n $(VUELTAS)

# Paso F (docs/MULTITAREA_LOOP.md): el banco del sistema de archivos en QEMU virt, con un disco de 1 + 64 MB con un
# RedoxFS vacío, nuevo en cada vuelta. Linux, con `tools/medir.py linux-fs`.
MEDIR_FS := target/kernel8-medir-fs.img
medir-fs: usuario FORCE
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar
	cd ../archivos/herramientas && cargo build --release
	python3 ../tools/medir.py disco-fs target/medir-fs.img $(DASHFS_AR)
	cargo build $(CARGO_PERFIL) --features virt,medir_fs
	$(OBJCOPY) -O binary $(ELF) $(MEDIR_FS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(MEDIR_FS)) cargo build --release
	python3 ../tools/medir.py dashos-fs ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi target/medir-fs.img target/medir-fs.json -n $(VUELTAS)

# P3 (docs/MULTITAREA_LOOP.md): el mismo banco en la Raspberry Pi 3 (QEMU raspi3b, o una de verdad con
# `tools/sd_pi3.py`).
MEDIR_PI3 := target/kernel8-medir-pi3.img
medir-pi3: usuario FORCE
	python3 ../tools/medir.py paquete ../usuario/target/medir.paq
	cargo build $(CARGO_PERFIL) --features pi3,medir
	$(OBJCOPY) -O binary $(ELF) $(MEDIR_PI3)
	python3 ../tools/medir.py pi3 $(MEDIR_PI3) target/medir-pi3.json -n $(VUELTAS)

# La Raspberry Pi 5 (docs/bitacoras/pi5.md, docs/PI5_PRIMER_ARRANQUE.md): la imagen que carga su firmware, con la consola
# y el diagnóstico en la pantalla (la pide al firmware por el buzón) y solo la consola y la caja Amiga como programas.
# QEMU no emula la Pi 5: se prueba en una de verdad; lo que se puede probar aquí, con `arranque-virt-diag`.
PI5 := target/kernel_2712.img
pi5: usuario FORCE
	cargo build $(CARGO_PERFIL) --features pi5
	$(OBJCOPY) -O binary $(ELF) $(PI5)
	@ls -l $(PI5)

# La tarjeta (o el SSD) para la Pi 5: una imagen de disco con la partición FAT de arranque (config.txt, la imagen y los
# árboles oficiales de la Pi 5, bajados de github.com/raspberrypi/firmware; nada de eso va en git). Fuera de git.
TARJETA_PI5 ?= $(HOME)/.cache/dashos-pi5/dashos-pi5.img
tarjeta-pi5: pi5-escritorio pi5-usb pi5
	python3 ../tools/preparar_pi5.py $(PI5) $(TARJETA_PI5) $(PI5_ESCRITORIO) $(PI5_USB)

# El diagnóstico de la Pi 5 en QEMU virt: la misma consola en pantalla, con la pantalla que deja EDK2 (ramfb) en lugar
# de la del buzón, y los mismos programas. Comprueba sus líneas en el puerto serie (pruebas/arranque_virt_diag.txt) y
# deja la captura de la pantalla en target/diag.ppm (y en capturas-agente/pi5-diag-virt.png).
VIRT_DIAG := target/kernel8-virt-diag.img
arranque-virt-diag: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,diagnostico
	$(OBJCOPY) -O binary $(ELF) $(VIRT_DIAG)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_DIAG)) cargo build --release
	rm -f target/diag.ppm
	GUARDAR=$(abspath target/salida_virt_diag.txt) CAPTURA=$(abspath target/diag.ppm) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_diag.txt 120
	python3 ../tools/captura_diag.py target/diag.ppm
	mkdir -p ../capturas-agente && python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])' target/diag.ppm ../capturas-agente/pi5-diag-virt.png 2>/dev/null || true

# El paso 4 (docs/bitacoras/pi5.md): el escritorio en la pantalla del firmware, sin GPU ni teclado. `make pi5-escritorio`
# saca target/dashos-escritorio.img, que `tarjeta-pi5` mete también en la tarjeta (se elige en config.txt). En QEMU virt,
# `arranque-virt-escritorio`: la pantalla pasa de la consola del kernel al compositor y la caja Amiga abre su ventana.
PI5_ESCRITORIO := target/dashos-escritorio.img
pi5-escritorio: usuario FORCE
	cargo build $(CARGO_PERFIL) --features pi5,escritorio,usb
	$(OBJCOPY) -O binary $(ELF) $(PI5_ESCRITORIO)
	@ls -l $(PI5_ESCRITORIO)

VIRT_ESCRITORIO := target/kernel8-virt-escritorio.img
arranque-virt-escritorio: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,escritorio
	$(OBJCOPY) -O binary $(ELF) $(VIRT_ESCRITORIO)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_ESCRITORIO)) cargo build --release
	rm -f target/escritorio.ppm
	GUARDAR=$(abspath target/salida_virt_escritorio.txt) CAPTURA=$(abspath target/escritorio.ppm) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_escritorio.txt 120
	mkdir -p ../capturas-agente && python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])' target/escritorio.ppm ../capturas-agente/pi5-escritorio-virt.png 2>/dev/null || true

# El teclado y el ratón USB de la Pi 5 (docs/bitacoras/pi5.md, "el RP1"): `make pi5-usb` saca target/dashos-usb.img, el
# diagnóstico con el PCIe del RP1, el RP1 y sus dos xHCI en el panel, y cada tecla en pantalla (`rp1.rs`, `xhci0`,
# `xhci1`). `tarjeta-pi5` lo mete en la tarjeta (se elige en config.txt). En QEMU virt, `arranque-virt-usb`: sin RP1 (el
# panel lo dice y sigue), con el xHCI de QEMU como `xhci0`, un teclado y un ratón, y teclas mandadas por el monitor.
PI5_USB := target/dashos-usb.img
pi5-usb: usuario FORCE
	cargo build $(CARGO_PERFIL) --features pi5,usb
	$(OBJCOPY) -O binary $(ELF) $(PI5_USB)
	@ls -l $(PI5_USB)

VIRT_USB := target/kernel8-virt-usb.img
arranque-virt-usb: usuario FORCE
	cargo build $(CARGO_PERFIL) --features virt,usb
	$(OBJCOPY) -O binary $(ELF) $(VIRT_USB)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_USB)) cargo build --release
	rm -f target/usb.ppm
	GUARDAR=$(abspath target/salida_virt_usb.txt) CAPTURA=$(abspath target/usb.ppm) TECLAS=1 MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_usb.txt 120
	mkdir -p ../capturas-agente && python3 -c 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])' target/usb.ppm ../capturas-agente/pi5-usb-virt.png 2>/dev/null || true

PRUEBAS_VIRT := target/kernel8-pruebas-virt.img

# Las pruebas del arranque en QEMU virt por UEFI (U3c): las que valen para las dos placas. Con GIC v3 (y su ITS): el
# v2 ya lo prueban `make prueba` (la Pi) y `make arranque-virt`.
prueba-virt: usuario FORCE
	cargo build $(CARGO_PERFIL) --features pruebas,virt
	$(OBJCOPY) -O binary $(ELF) $(PRUEBAS_VIRT)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(PRUEBAS_VIRT)) cargo build --release
	GIC=3 QEMU_EXTRA="-device ramfb" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/esperado_virt.txt 120

clean:
	cargo clean

FORCE:
