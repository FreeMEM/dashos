# DashOS: compilar el kernel y sacar la imagen que carga el firmware de la Pi (o QEMU raspi4b).
#
# make         la imagen normal, sin pruebas (target/kernel8.img)
# make prueba  la imagen con las pruebas del arranque y los programas de usuario que lanzan, y la prueba en QEMU
# make arranque  la imagen normal en QEMU: tiene que llegar a "DashOS en marcha"
# make virt    la imagen para QEMU virt (U3, docs/UEFI_LOOP.md), que carga el programa de UEFI
# make arranque-virt  esa imagen dentro del programa de UEFI, arrancada por EDK2 en QEMU virt
# make arranque-x86  el kernel de x86-64 dentro del programa de UEFI (BOOTX64.EFI), arrancado por OVMF en QEMU q35,
#              con el escritorio y el guion largo de arranque-virt (X7) y los controladores de un PC (X5, X5c)
# make prueba-x86  lo mismo con las pruebas del arranque de x86 (pruebas_x86.rs) y los programas de usuario de x86
#              Las dos, con IOMMU=intel (VT-d) o IOMMU=amd (AMD-Vi): con esa IOMMU en QEMU (X5b), con remapeo de
#              interrupciones (con IOMMU=intel INTREMAP=off, la VT-d sin él)
#              Y con ACCEL=kvm, con KVM y -cpu host en vez de TCG (X10: el hipervisor de x86 en una CPU de verdad)
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

# U7: la imagen de virt lleva la caja Amiga en los dos órdenes (caja/build y caja/build-le, donde el JIT gira los bytes
# del 68k), e `inicio` da la de little-endian si la CPU no tiene big-endian en EL0. CAJA_ORDEN=le la da siempre, para
# probarla en QEMU (build.rs, con DASHOS_CAJA_ORDEN).
CAJA_ORDEN ?= be
export DASHOS_CAJA_ORDEN := $(CAJA_ORDEN)

$(IMAGEN): usuario FORCE
	cargo build $(CARGO_PERFIL)
	$(OBJCOPY) -O binary $(ELF) $(IMAGEN)

usuario: kit-c FORCE
	cd ../usuario && cargo build --release --features archivos,ventanas,sonido,imagenes,red,caja,ordenes
	@# El Shell nativo y las órdenes de SYS:C (van al disco, no a la imagen del kernel).
	cd ../ordenes && cargo build --release
	cd ../usuario && cargo build --release --target aarch64_be-unknown-none-softfloat --bin hola --bin fpu_b --bin fallos
	$(BOOTLIN)/aarch64_be-linux-as -EL -o $(PIE).o ../usuario/asm/pie.s
	$(BOOTLIN)/aarch64_be-linux-ld -EL -pie --no-dynamic-linker -z noexecstack -z stack-size=32768 -o $(PIE) $(PIE).o
	@# H4: la caja Amiga (caja/), con el JIT de Emu68: solo si está el clon de third_party/Emu68 (toolchains/LEEME.md).
	if [ -d ../third_party/Emu68/src ]; then $(MAKE) -C ../caja && $(MAKE) -C ../caja ORDEN=le; else echo "Sin third_party/Emu68: la caja Amiga no se compila"; fi

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
	@# Los iconos de las cajas: los .info de cada formato, hostiles y mil mutados; los .desktop y los temas.
	cd ../iconos-cajas && cargo test --quiet
	cd ../caja-linux/vmm && cargo test --quiet
	$(MAKE) -C ../caja/pruebas_hunk
	$(MAKE) -C ../caja/pruebas_fallo
	mkdir -p target && rustc --edition 2024 --test src/rp1.rs -o target/rp1_pruebas && ./target/rp1_pruebas -q
	cd ../usuario && mkdir -p target && rustc --edition 2024 --test src/cc_rejilla.rs -o target/cc_rejilla_pruebas && ./target/cc_rejilla_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/lanzables.rs -o target/lanzables_pruebas && ./target/lanzables_pruebas -q
	@# centro-vivo: la cola fija entre hilos (la del canal del centro y la del fondo).
	cd ../usuario && rustc --edition 2024 --test src/anillo.rs -o target/anillo_pruebas && ./target/anillo_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/terminal.rs -o target/terminal_pruebas && ./target/terminal_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/protocolo/avisos.rs -o target/avisos_pruebas && ./target/avisos_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/avisos_ajustes.rs -o target/avisos_ajustes_pruebas && ./target/avisos_ajustes_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/dms_medidas.rs -o target/dms_medidas_pruebas && ./target/dms_medidas_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/calendario.rs -o target/calendario_pruebas && ./target/calendario_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/cortafuegos_caja.rs -o target/cortafuegos_caja_pruebas && ./target/cortafuegos_caja_pruebas -q
	@# X5c: el intérprete del firmware del PHY de la RTL8125 (ficheros sintéticos y un aparato falso).
	cd ../usuario && rustc --edition 2024 --test src/rtl8125_fw.rs -o target/rtl8125_fw_pruebas && ./target/rtl8125_fw_pruebas -q
	@# S27: el volumen del panel, la lista del lanzador y la fecha de la barra.
	cd ../usuario && rustc --edition 2024 --test src/volumen_osd.rs -o target/volumen_osd_pruebas && ./target/volumen_osd_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/lanzador_lista.rs -o target/lanzador_lista_pruebas && ./target/lanzador_lista_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/reloj.rs -o target/reloj_pruebas && ./target/reloj_pruebas -q
	@# S27, la barra de DMS: el modelo de las barras, el importador de settings.json y su lector de JSON (hostiles incluidos).
	cd ../usuario && rustc --edition 2024 --test src/json_mini.rs -o target/json_mini_pruebas && ./target/json_mini_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/barra_config.rs -o target/barra_config_pruebas && ./target/barra_config_pruebas -q
	@# S27, el dock y los OSD como los de DMS: muelles y curvas, ajustes en texto (fichero malo, bytes al azar), lo que
	@# lleva el dock y dónde (mil ventanas, ráfagas, monitor que no existe) y los OSD (posiciones, ráfagas, relojes raros).
	cd ../usuario && rustc --edition 2024 --test src/muelle.rs -o target/muelle_pruebas && ./target/muelle_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/dock_ajustes.rs -o target/dock_ajustes_pruebas && ./target/dock_ajustes_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/dock_modelo.rs -o target/dock_modelo_pruebas && ./target/dock_modelo_pruebas -q
	@# Rama dock-discos: la lupa, el rebote, el indicador y arrastrar del dock; los discos del escritorio (el panel y sus
	@# ajustes; mil volúmenes, ráfagas de montar y desmontar, bytes al azar).
	cd ../usuario && rustc --edition 2024 --test src/dock_vivo.rs -o target/dock_vivo_pruebas && ./target/dock_vivo_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/discos_ajustes.rs -o target/discos_ajustes_pruebas && ./target/discos_ajustes_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/discos_modelo.rs -o target/discos_modelo_pruebas && ./target/discos_modelo_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/osd_modelo.rs -o target/osd_modelo_pruebas && ./target/osd_modelo_pruebas -q
	@# tecla-pegada: cada teclado de arranque lo suyo (dos a la vez, el que se va suelta lo pulsado, informes malos) y la
	@# cola sin sitio (nunca se tira un soltar; ráfagas).
	cd ../usuario && rustc --edition 2024 --test src/hid_teclas.rs -o target/hid_teclas_pruebas && ./target/hid_teclas_pruebas -q
	@# S13, segunda parte: DNS (respuestas falsas, nombres en bucle, cortadas), SNTP, las zonas horarias y el cortafuegos de entrada.
	cd ../usuario && rustc --edition 2024 --test src/dns.rs -o target/dns_pruebas && ./target/dns_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/sntp.rs -o target/sntp_pruebas && ./target/sntp_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/zona.rs -o target/zona_pruebas && ./target/zona_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/vigilancia.rs -o target/vigilancia_pruebas && ./target/vigilancia_pruebas -q
	cd ../usuario && rustc --edition 2024 --test src/protocolo/archivos.rs -o target/protocolo_archivos_pruebas && ./target/protocolo_archivos_pruebas -q
	@# Rama amiga-apps: mathffp.library nativa de la caja Amiga (la coma flotante rápida de Motorola).
	mkdir -p ../caja/pegamento/target && cd ../caja/pegamento && rustc --edition 2024 --test src/ffp.rs -o target/ffp_pruebas && ./target/ffp_pruebas -q
	@# S35, paso 2: virtio por mensajes entre virtio-red y el servicio de un dominio (mensajes malos, colas fuera de la ventana).
	cd ../usuario && rustc --edition 2024 --test src/protocolo/virtio_msg.rs -o target/virtio_msg_pruebas && ./target/virtio_msg_pruebas -q
	@# S35, paso 2 con USB: el protocolo del aparato prestado entre el xHCI y el servicio del dominio.
	cd ../usuario && rustc --edition 2024 --test src/protocolo/usb_prestado.rs -o target/usb_prestado_pruebas && ./target/usb_prestado_pruebas -q
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
	@# S6b, rama cuentas: las cuentas de usuario (ficheros dañados y truncados, nombres hostiles, el último administrador).
	cd ../usuario && rustc --edition 2024 --test src/cuentas.rs -o target/cuentas_pruebas && ./target/cuentas_pruebas -q
	@# S32: el instalador (GPT con sus dos copias rotas o distintas, MBR híbrido, FAT con nombres largos, planes hostiles,
	@# y cortes de luz en cada sector de la tabla, de la ESP y de la instalación entera junto a un Windows).
	cd ../usuario && rustc --edition 2024 -O --test src/instalador/pruebas.rs -o target/instalador_pruebas && ./target/instalador_pruebas -q
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
RED_HUB := -netdev user,id=red0,restrict=on,hostfwd=tcp:127.0.0.1:$$RED_PUERTO-10.0.2.15:7777,guestfwd=tcp:10.0.2.100:7-cmd:$(abspath ../tools/red_eco.py) -netdev dgram,id=red5,local.type=unix,local.path=$(RED)/qemu2.sock,remote.type=unix,remote.path=$(RED)/vecino.sock -netdev hubport,id=red6,hubid=0,netdev=red5 -netdev dgram,id=red1,local.type=unix,local.path=$(RED)/qemu.sock,remote.type=unix,remote.path=$(RED)/pc.sock -netdev hubport,id=red2,hubid=0,netdev=red0 -netdev hubport,id=red3,hubid=0,netdev=red1 -netdev hubport,id=red4,hubid=0
RED_QEMU := $(RED_HUB) -device virtio-net-pci,netdev=red4,disable-legacy=on,mac=$(RED_MAC),romfile=

# Al final, el guion pulsa Reiniciar en el menú de apagado del panel y se espera al segundo arranque (REINICIO,
# pruebas/arranque_virt_reinicio.txt; decisión del usuario del 2026-11-13). 900 s (antes 720): con las ramas de DMS y
# de los iconos unidas, el guion hace también el dock, los avisos, el Dash y los iconos de la caja Amiga, y no llegaba
# al último paso (el selector desde `guarda`).
arranque-virt: virt $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT)) cargo build --release
	rm -f target/explorador*.ppm target/captura.ppm target/amiga.ppm target/dash.ppm target/dash-nativo.ppm target/dash-nativo-pregunta.ppm target/gadtools.ppm target/gadtools-menu.ppm target/gadtools-dash.ppm target/requester.ppm target/reaction.ppm target/reaction.orig.ppm target/sonido.wav $(RED)/qemu.sock $(RED)/pc.sock $(RED)/qemu2.sock $(RED)/vecino.sock target/u6.ana.ppm target/u6.carla.ppm
	mkdir -p $(RED)
	python3 -I ../tools/red_basura.py $(RED) $(RED_MAC) 300 & BASURA=$$!; $(RED_ANTES); \
	GUARDAR=$(abspath target/salida_virt.txt) CAPTURA=$(abspath target/captura.ppm) CAPTURA_AMIGA=$(abspath target/amiga.ppm) CAPTURA_DASH=$(abspath target/dash.ppm) CAPTURA_DASHOS=$(abspath target/dash-nativo.ppm) CAPTURA_GT=$(abspath target/gadtools.ppm) CAPTURA_GT_MENU=$(abspath target/gadtools-menu.ppm) CAPTURA_GT_DASH=$(abspath target/gadtools-dash.ppm) CAPTURA_REQ=$(abspath target/requester.ppm) CAPTURA_RA=$(abspath target/reaction) CAPTURA_EXPLORADOR=$(abspath target/explorador) CAPTURA_U6=$(abspath target/u6) CAPTURA_MONITOR=$(abspath target/monitor) REINICIO=$(abspath pruebas/arranque_virt_reinicio.txt) TECLAS=1 MAQUINA=highmem=off QEMU_EXTRA="-device ramfb -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2 -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv -audiodev wav,id=snd0,path=$(abspath target/sonido.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0 $(RED_QEMU)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt.txt $(or $(TIEMPO_VIRT),900); \
	R=$$?; $(RED_DESPUES); exit $$R
	python3 ../tools/disco_archivos.py --comprobar $(DISCO_NVME) $(VER)
	python3 ../tools/captura_ventanas.py target/captura.ppm
	python3 ../tools/sonido_wav.py target/sonido.wav
	python3 ../tools/captura_cambio.py target/u6.ana.ppm target/u6.carla.ppm

# S27, oleada 3 (docs/bitacoras/dms-avisos.md): las capturas de los avisos, el Dash del reloj y el centro de control,
# con la imagen de usar y `tools/guion_dms.py` (entra Ana, avisa, abre cada cosa). A 1920 × 1080 y con el aspecto del
# usuario: DASHOS_PANTALLA=1920x1080 DASHOS_FONDO_EXTRA=fondo.png DASHOS_PREFS="apariencia=oscuro fondo;fondo=fondo.png"
# (el fondo, fuera de git). Las capturas, en target/dms/ (PNG). Rama `centro-vivo`: su disco, con un fondo guardado
# (sin DASHOS_PREFS, atardecer.png), que sale al entrar Ana aunque `fondo` arranque antes que el servicio de archivos.
arranque-virt-dms: virt-uso $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_USO)) cargo build --release
	rm -rf target/dms && mkdir -p target/dms
	DASHOS_PREFS="$(or $(DASHOS_PREFS),fondo=atardecer.png)" python3 ../tools/disco_archivos.py target/dms/disco.img $(DASHFS_AR)
	GUARDAR=$(abspath target/salida_virt_dms.txt) DMS_CAPTURAS=$(abspath target/dms/dms) DMS_ANCHO=$(firstword $(subst x, ,$(or $(DASHOS_PANTALLA),800x600))) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/dms/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_dms.txt 400
	@cat target/dms/dms.guion.txt

# Los discos del escritorio (rama dock-discos): capturas con la imagen de usar y `tools/guion_discos.py`, en
# target/discos/. Con el aspecto del usuario, como `arranque-virt-dms` (DASHOS_PANTALLA, DASHOS_FONDO_EXTRA,
# DASHOS_PREFS y DMS_SHIVA=1). No es una prueba: las líneas de pruebas/capturas_discos.txt solo dicen cuándo acabar.
capturas-discos: virt-uso $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_USO)) cargo build --release
	rm -rf target/discos && mkdir -p target/discos
	DASHOS_PREFS="$(or $(DASHOS_PREFS),fondo=atardecer.png)" python3 ../tools/disco_archivos.py target/discos/disco.img $(DASHFS_AR)
	-GUARDAR=$(abspath target/salida_capturas_discos.txt) DISCOS_CAPTURAS=$(abspath target/discos/discos) DMS_ANCHO=$(firstword $(subst x, ,$(or $(DASHOS_PANTALLA),800x600))) DMS_ALTO=$(lastword $(subst x, ,$(or $(DASHOS_PANTALLA),800x600))) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/discos/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/capturas_discos.txt 300
	@cat target/discos/discos.guion.txt

# Rama sonido-pro (docs/bitacoras/sonido-pro.md): el mezclador, el visualizador y los controles de medios en pantalla,
# con la imagen de usar y un reproductor de prueba (`melodia`) que suena desde el arranque. `tools/guion_sonido.py`
# entra como Ana (con DMS_SHIVA=1, el aspecto de shiva), abre el detalle de la salida en el centro de control y de ahí
# la ventana "Sonido", cambia el ecualizador y pausa la música, con una captura en cada momento (target/sonido/*.png;
# y en $(SONIDO_FUERA), fuera de git). Lo que suena, en target/sonido/sonido.wav (con el antes y el después del
# ecualizador, que el guion apunta en su .guion.txt). A 1920 × 1080: DASHOS_PANTALLA=1920x1080, y el fondo del
# usuario con DASHOS_FONDO_EXTRA=... DASHOS_PREFS="apariencia=oscuro fondo;fondo=...".
VIRT_SONIDO := target/kernel8-virt-sonido.img
SONIDO_FUERA ?= $(HOME)/recuperar/geosian-7d/sonido-pro-capturas
SONIDO_CAPTURAS := 1-panel-salida 2-mezclador 3-mezclador-graves 4-mezclador-pausa 5-osd-volumen 6-ajustes-sonido 7-bloqueo-musica
arranque-virt-sonido: usuario $(DISCO_NVME) FORCE
	cargo build $(CARGO_PERFIL) --features virt,uso,sonido_demo
	$(OBJCOPY) -O binary $(ELF) $(VIRT_SONIDO)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_SONIDO)) cargo build --release
	rm -rf target/sonido && mkdir -p target/sonido
	DASHOS_PREFS="$(or $(DASHOS_PREFS),fondo=atardecer.png)" python3 ../tools/disco_archivos.py target/sonido/disco.img $(DASHFS_AR)
	GUARDAR=$(abspath target/salida_virt_sonido.txt) SONIDO_CAPTURAS=$(abspath target/sonido/sonido) DMS_ANCHO=$(firstword $(subst x, ,$(or $(DASHOS_PANTALLA),800x600))) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/sonido/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv -audiodev wav,id=snd0,path=$(abspath target/sonido/sonido.wav),out.frequency=48000 -device virtio-sound-pci,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_sonido.txt 400
	@cat target/sonido/sonido.guion.txt
	for c in $(SONIDO_CAPTURAS); do test -s target/sonido/sonido.$$c.png || { echo "FALTA la captura $$c"; exit 1; }; done
	mkdir -p $(SONIDO_FUERA) && for c in $(SONIDO_CAPTURAS); do cp target/sonido/sonido.$$c.png $(SONIDO_FUERA)/$$c.png; done
	-python3 ../tools/sonido_partir.py target/sonido/sonido.wav target/sonido/sonido.guion.txt $(SONIDO_FUERA)

# Rama `fondos` (docs/bitacoras/fondos.md): la herramienta de fondos, las transiciones y los fondos animados en pantalla,
# con `tools/guion_fondos.py` y la imagen de usar: capturas en target/fondos/ (PNG) y, fuera de git, en
# ~/recuperar/geosian-7d/fondos-capturas/; y lo que gasta cada fondo animado en reposo (lo dicen `fondo` y el compositor).
# A 1920 × 1080 con el aspecto del usuario: DASHOS_PANTALLA=1920x1080 FONDOS_SHIVA=1 DASHOS_FONDO_EXTRA=fondo.png.
FONDOS_FUERA ?= $(HOME)/recuperar/geosian-7d/fondos-capturas
arranque-virt-fondos: virt-uso $(DISCO_NVME)
	cd ../uefi && DASHOS_PANTALLA=$(or $(DASHOS_PANTALLA),800x600) DASHOS_NUCLEO=$(abspath $(VIRT_USO)) cargo build --release
	rm -rf target/fondos && mkdir -p target/fondos
	DASHOS_PREFS="$(or $(DASHOS_PREFS),fondo=atardecer.png)" python3 ../tools/disco_archivos.py target/fondos/disco.img $(DASHFS_AR)
	GUARDAR=$(abspath target/salida_virt_fondos.txt) FONDOS_CAPTURAS=$(abspath target/fondos/fondos) FONDOS_ANCHO=$(firstword $(subst x, ,$(or $(DASHOS_PANTALLA),800x600))) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/fondos/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_fondos.txt 900
	@cat target/fondos/fondos.guion.txt
	mkdir -p $(FONDOS_FUERA)/$(or $(DASHOS_PANTALLA),800x600) && cp target/fondos/*.png $(FONDOS_FUERA)/$(or $(DASHOS_PANTALLA),800x600)/ 2>/dev/null; true

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

# Plan de iconos, parte 3 (docs/bitacoras/iconos-cajas.md): la imagen de usar con una copia del disco de pruebas.
# `tools/guion_iconos.py` entra como Ana, abre el explorador y mira los .info de la caja Amiga (uno de cada formato, el
# volumen, los escondidos y la lista), con capturas (target/iconos/iconos.*.png y, fuera de git,
# ~/recuperar/geosian-7d/iconos-cajas-capturas/).
ICONOS_CAPTURAS := 01-iconos 02-elegido 03-volumen 04-ocultos 05-lista
ICONOS_FUERA ?= $(HOME)/recuperar/geosian-7d/iconos-cajas-capturas
arranque-virt-iconos: virt-uso $(DISCO_NVME)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_USO)) cargo build --release
	rm -rf target/iconos && mkdir -p target/iconos
	cp --reflink=auto $(DISCO_NVME) target/iconos/disco.img
	GUARDAR=$(abspath target/salida_virt_iconos.txt) ICONOS_CAPTURAS=$(abspath target/iconos/iconos) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/iconos/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_iconos.txt 400
	@cat target/iconos/iconos.guion.txt
	for c in $(ICONOS_CAPTURAS); do test -s target/iconos/iconos.$$c.png || { echo "FALTA la captura $$c"; exit 1; }; done
	mkdir -p $(ICONOS_FUERA) && for c in $(ICONOS_CAPTURAS); do cp target/iconos/iconos.$$c.png $(ICONOS_FUERA)/explorador-$$c.png; done

# S10, los menús de los programas (docs/bitacoras/menus.md): la imagen de usar con la prueba de kit-c (`dash-ventana`)
# y una copia del disco de pruebas. `tools/guion_menus.py` entra como Ana y mira los menús del Shell (el kit nativo) y
# de `dash-ventana` (kit-c) dentro de la ventana; los pasa a la barra con `Ajuste menus barra` (tecleado en el Shell),
# los abre allí con un clic y con el botón derecho, elige en ellos, y vuelve dentro; con una captura en cada momento
# (target/menus/menus.*.png; y en ~/recuperar/geosian-7d/menus-pulido-capturas/, fuera de git).
VIRT_MENUS := target/kernel8-virt-menus.img
MENUS_CAPTURAS := 01-kitc-dentro 02-kitc-dentro-abierto 03-shell-dentro 04-shell-dentro-abierto 05-barra-shell 06-barra-shell-abierto 07-barra-elegido 08-barra-boton-derecho 09-barra-kitc 10-barra-kitc-abierto 11-kitc-elegido 12-dentro-otra-vez 13-f10 \
	14-shell-teclas 15-kitc-teclas-fin 16-kitc-rueda 17-barra-submenu-teclas 18-barra-largo-rueda 19-barra-submenu-rueda 20-clic-que-cierra 21-cambia-desplegado 22-dentro-submenu-teclas 23-dentro-largo-rueda 24-ventana-cerrada \
	25-kitc-ver 26-kitc-submenu 27-ajustes-abierta-menus-barra
MENUS_FUERA ?= $(HOME)/recuperar/geosian-7d/menus-pulido-capturas
arranque-virt-menus: usuario $(DISCO_NVME) FORCE
	cargo build $(CARGO_PERFIL) --features virt,uso,menus
	$(OBJCOPY) -O binary $(ELF) $(VIRT_MENUS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_MENUS)) cargo build --release
	rm -rf target/menus && mkdir -p target/menus
	cp --reflink=auto $(DISCO_NVME) target/menus/disco.img
	GUARDAR=$(abspath target/salida_virt_menus.txt) MENUS_CAPTURAS=$(abspath target/menus/menus) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/menus/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_menus.txt 900
	@cat target/menus/menus.guion.txt
	for c in $(MENUS_CAPTURAS); do test -s target/menus/menus.$$c.png || { echo "FALTA la captura $$c"; exit 1; }; done
	mkdir -p $(MENUS_FUERA) && for c in $(MENUS_CAPTURAS); do cp target/menus/menus.$$c.png $(MENUS_FUERA)/$$c.png; done

# Rama cuentas-2, las cuentas en pantalla (docs/bitacoras/cuentas-2.md): la imagen de usar con `cuentas` (`guardian`
# escribe los códigos de recuperación en la consola) y un disco de pruebas sin cuentas (primer arranque).
# `tools/guion_cuentas.py`: el asistente crea a Eva (un nombre reservado primero: no), Ajustes > Cuentas crea a Fran, lo
# pone en un grupo, lo borra y lo devuelve, y cambia la contraseña de Eva; al cerrar su sesión, la pantalla de entrada
# con la lista, y Fran entra con su código de recuperación (uno malo primero); al cerrar la de Fran, vuelve Eva. Con una captura en cada momento (target/cuentas/cuentas.*.png; y en
# ~/recuperar/geosian-7d/cuentas-2-capturas/, fuera de git).
VIRT_CUENTAS := target/kernel8-virt-cuentas.img
CUENTAS_CAPTURAS := 01-asistente 02-asistente-reservado 03-codigo-primera 04-ajustes-cuentas 05-tanda-nueva 06-codigo-fran \
	07-cuentas-fran 08-borrada 09-mi-contrasena 10-entrada-lista 11-olvide 12-codigo-malo 13-codigo-nuevo 14-entrada-otra-vez 15-entrada-clave
CUENTAS_FUERA ?= $(HOME)/recuperar/geosian-7d/cuentas-2-capturas
arranque-virt-cuentas: usuario $(DISCO_NVME) FORCE
	cargo build $(CARGO_PERFIL) --features virt,uso,cuentas
	$(OBJCOPY) -O binary $(ELF) $(VIRT_CUENTAS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_CUENTAS)) cargo build --release
	rm -rf target/cuentas && mkdir -p target/cuentas
	python3 ../tools/disco_archivos.py --sin-cuentas target/cuentas/disco.img $(DASHFS_AR)
	GUARDAR=$(abspath target/salida_virt_cuentas.txt) CUENTAS_CAPTURAS=$(abspath target/cuentas/cuentas) MAQUINA=highmem=off QEMU_EXTRA="-device ramfb $(USB_QEMU) -drive file=$(abspath target/cuentas/disco.img),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_cuentas.txt 900
	@cat target/cuentas/cuentas.guion.txt
	for c in $(CUENTAS_CAPTURAS); do test -s target/cuentas/cuentas.$$c.png || { echo "FALTA la captura $$c"; exit 1; }; done
	mkdir -p $(CUENTAS_FUERA) && for c in $(CUENTAS_CAPTURAS); do cp target/cuentas/cuentas.$$c.png $(CUENTAS_FUERA)/$$c.png; done

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

# Un dominio de controladores (S35, paso 2; docs/bitacoras/dominio-red.md y dominio-usb.md): la imagen de `virt` con
# `dominio`, en QEMU con virtualization=on. La red de DashOS no tiene virtio-net-pci: tiene un adaptador de red USB
# (el usb-net de QEMU, en el puerto 3 del xHCI, junto al teclado y el ratón) que el xHCI de DashOS presta por USB/IP a
# un Linux mínimo en una máquina virtual (`dominio`); ese Linux lo lleva con su controlador (cdc_ether) y devuelve a
# DashOS una virtio-net por mensajes, que lleva `virtio-red` de siempre. El adaptador está en el concentrador con la red
# de usuario de QEMU (DHCP y el eco del PC en 10.0.2.100:7, con restrict=on: nada sale del PC) y el vecino de prueba
# (`tools/red_vecino.py`, 10.0.2.53). `dominio-prueba` hace DHCP, ecos TCP y UDP, y relanza el dominio en modos
# hostiles (oferta falsa, USB/IP falso o enorme, otro aparato, tormenta de peticiones, basura en el flujo, colgarse y
# morir a media transferencia), pide desenchufar y enchufar el adaptador (lo hace `tools/guion_dominio_usb.py` por el
# monitor de QEMU), mata al conductor y lo hace caer tres veces seguidas (el aviso). El kernel y el initramfs del
# dominio, fuera de git: `tools/dominio_red.py preparar usb` (DOMINIO_IMAGE, DOMINIO_INITRD). Con el disco de las
# pruebas (NVMe): así `sesiones` escribe el registro común, donde quedan las caídas y la consola del dominio, y la prueba
# lo lee.
DOMINIO_DIR ?= $(HOME)/recuperar/geosian-7d/dominio-usb
DOMINIO_IMAGE ?= $(DOMINIO_DIR)/Image
DOMINIO_INITRD ?= $(DOMINIO_DIR)/initramfs.cpio
VIRT_DOMINIO := target/kernel8-virt-dominio.img
ADAPTADOR_USB := usb-net,netdev=red4,mac=$(RED_MAC),bus=xhci.0,port=3,id=usbred
RED_DOMINIO := -netdev user,id=red0,restrict=on,guestfwd=tcp:10.0.2.100:7-cmd:$(abspath ../tools/red_eco.py) -netdev dgram,id=red5,local.type=unix,local.path=$(RED)/qemu2.sock,remote.type=unix,remote.path=$(RED)/vecino.sock -netdev hubport,id=red6,hubid=0,netdev=red5 -netdev hubport,id=red2,hubid=0,netdev=red0 -netdev hubport,id=red4,hubid=0 $(USB_QEMU) -device $(ADAPTADOR_USB)
arranque-virt-dominio: usuario $(DISCO_NVME) FORCE
	@test -f "$(DOMINIO_IMAGE)" || { echo "Falta el kernel del dominio (DOMINIO_IMAGE=$(DOMINIO_IMAGE)): no va en git; tools/dominio_red.py preparar usb"; exit 1; }
	@test -f "$(DOMINIO_INITRD)" || { echo "Falta el initramfs del dominio (DOMINIO_INITRD=$(DOMINIO_INITRD)): tools/dominio_red.py preparar usb"; exit 1; }
	mkdir -p target/dominio
	cp "$(DOMINIO_IMAGE)" target/dominio/Image
	cp "$(DOMINIO_INITRD)" target/dominio/initrd
	cargo build $(CARGO_PERFIL) --features virt,dominio
	$(OBJCOPY) -O binary $(ELF) $(VIRT_DOMINIO)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(VIRT_DOMINIO)) cargo build --release
	rm -f $(RED)/qemu2.sock $(RED)/vecino.sock
	mkdir -p $(RED)
	python3 -I ../tools/red_vecino.py $(RED) $(RED_MAC) 900 & VECINO=$$!; \
	DOMINIO_USB="$(ADAPTADOR_USB)" GUARDAR=$(abspath target/salida_virt_dominio.txt) MEMORIA=3G MAQUINA=highmem=off,virtualization=on QEMU_EXTRA="-device ramfb -drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv $(RED_DOMINIO)" ../tools/prueba_uefi.sh ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi pruebas/arranque_virt_dominio.txt $(or $(TIEMPO_DOMINIO),900); \
	R=$$?; kill $$VECINO 2>/dev/null; exit $$R
	grep -E "\[(dominio|dominio-prueba|virtio-red|xhci)\]" target/salida_virt_dominio.txt | grep -vE "linux: \[ |tecla|ratón" || true

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

# Rama amiga-apps (docs/bitacoras/amiga-apps.md): programas Amiga de verdad (ProTracker, Directory Opus, LightWave,
# Tachyon...) en la caja Amiga, instalados en una copia del AmigaOS del usuario (AMIGA_APPS_SISTEMA, fuera de git).
# La imagen de la demo (`uso` y la caja de las capturas), sin el disco de la caja Linux. Cada escena
# (tools/amiga_apps/<escena>.txt) es una copia de caja/pruebas68k/lanzador.exe que arranca sus programas; el guion
# (`tools/amiga_apps.py`) entra como Ana, contesta a la guardia y saca las capturas que piden las escenas. Con
# AMIGA_APPS_RED=--red, red de verdad por la red de usuario de QEMU (solo a mano: las pruebas no salen a Internet).
AMIGA_APPS_SISTEMA ?= $(HOME)/.cache/dash-emu/sys32mui-apps
AMIGA_APPS_ESCENAS ?= pt,dopus,lw,tachyon,forja,hangar
AMIGA_APPS_PANTALLA ?= 1920x1080
AMIGA_APPS_DIR ?= $(HOME)/recuperar/geosian-7d/amiga-apps/vuelta
AMIGA_APPS_FONDO ?= $(HOME)/recuperar/geosian-7d/dms-bloqueo-capturas/fondo/fuji-256.png
AMIGA_APPS_OPCIONES ?= --shiva
amiga-apps: usuario catalogos FORCE
	$(MAKE) -C ../caja B=build-demo CAJA_INTERACTIVA=1
	$(MAKE) -C ../caja/pruebas68k lanzador.exe cia_prueba.exe
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar --bin dashfs-volume
	cd ../archivos/herramientas && cargo build --release
	rm -rf target/amiga-apps && mkdir -p target/amiga-apps
	python3 ../tools/amiga_apps.py preparar --escenas "$(AMIGA_APPS_ESCENAS)" --dir target/amiga-apps
	DASHOS_AMIGAOS="$(AMIGA_APPS_SISTEMA)" DASHOS_PROGRAMAS_AMIGA="$$(cat target/amiga-apps/programas.txt)" DASHOS_FONDO_EXTRA="$(AMIGA_APPS_FONDO)" DASHOS_PREFS="apariencia=oscuro fondo;fondo=$(notdir $(AMIGA_APPS_FONDO))" python3 ../tools/disco_archivos.py target/amiga-apps/nvme.img $(DASHFS_AR)
	# La imagen de la demo lleva la caja Linux (`linux`); aquí sin su núcleo: no arranca y no estorba.
	mkdir -p target/linux && : > target/linux/Image && : > target/linux/initrd
	cargo build $(CARGO_PERFIL) --features virt,uso,linux,demo
	$(OBJCOPY) -O binary $(ELF) $(VIRT_DEMO)
	cd ../uefi && DASHOS_PANTALLA=$(AMIGA_APPS_PANTALLA) DASHOS_NUCLEO=$(abspath $(VIRT_DEMO)) cargo build --release
	python3 ../tools/amiga_apps.py correr --efi ../uefi/target/aarch64-unknown-uefi/release/dashos-uefi.efi --disco target/amiga-apps/nvme.img --pantalla $(AMIGA_APPS_PANTALLA) --dir $(AMIGA_APPS_DIR) $(AMIGA_APPS_OPCIONES) $(AMIGA_APPS_RED)

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

# x86-64 (S30, X1): el kernel para x86_64-unknown-none, como ELF sin la información de depuración, dentro del programa
# de UEFI para x86_64-unknown-uefi; OVMF lo arranca en QEMU q35 y el kernel tiene que hablar por COM1.
OBJCOPY_X86 ?= objcopy
coma := ,
ELF_X86     := target/x86_64-unknown-none/$(PERFIL)/dashos-kernel
NUCLEO_X86  := target/kernel-x86.elf

# Los servicios que lanza `inicio` en x86 (X5, `inicio_x86.rs`). Las mismas características que `usuario-x86`, para
# no recompilar la biblioteca entre una y otra.
# X7, paso 4: también `imagenes` (el servicio de los ADF) y `ordenes` (`guardian` lanza el Shell), para el escritorio de
# `arranque-virt`.
FUNCIONES_X86 := archivos,ventanas,sonido,red,imagenes,ordenes,relevo

# X7a: el crate `relevo` (RelevoJIT) viene por git con `rev` fijo (usuario/Cargo.toml). RELEVO_DIR=<ruta> lo cambia por
# un RelevoJIT local, para probar cambios suyos sin subirlos (un `[patch]` por `--config`). La `rev` de Cargo.toml tiene
# que estar en GitHub igual, y Cargo.lock cambia mientras se use: no se sube así.
RELEVO_PATCH := $(if $(RELEVO_DIR),--config 'patch."https://github.com/FreeMEM/RelevoJIT".relevo.path="$(abspath $(RELEVO_DIR))"')
# X7, paso 4: los de `arranque-virt` (`inicio.rs` con `virt`) que no son de ARM, más los controladores de un PC. `paq`
# sale de `ordenes/`, como en ARM. Y los programas del disco de la prueba (`tools/disco_archivos.py`), también de x86.
SERVICIOS_X86 := inicio consola compositor xhci nvme disco ahci disco-sata archivos comprobador eco sesiones red virtio-red intel-red rtl-red fondo centro guardian pide entrada ficheros imagen adf-prueba ventana-a ventana-b ajustes sonido avisos barra dock avisador virtio-sonido hda tono-a tono-b selector hostil presa red-gloton red-prueba sin-red panel caida explorador workbench hola virtio-gpu presion sin-maquina espia hora commodity-prueba mezclador
DISCO_PROGRAMAS_PRUEBA_X86 := pide cambio ram avisa guarda espera blanker nocapslock

# X5c: el firmware del PHY de la RTL8125B (`rtl8125b-2.fw` de linux-firmware), si este sistema lo tiene: va dentro del
# kernel de x86 (`inicio_x86.rs`) y `rtl-red` lo aplica. No va en git. Se descomprime (.zst o .xz) en
# target/firmware/; FIRMWARE_RTL8125= (vacío) lo deja fuera, y el controlador sigue sin él. Es de Realtek: una imagen que
# se reparta con él lleva también su licencia (`LICENSE.r8169`, en /usr/share/licenses/linux-firmware*/).
FIRMWARE_RTL8125 ?= $(firstword $(wildcard /lib/firmware/rtl_nic/rtl8125b-2.fw /lib/firmware/rtl_nic/rtl8125b-2.fw.zst /lib/firmware/rtl_nic/rtl8125b-2.fw.xz))
FW_RTL := target/firmware/rtl8125b-2.fw
firmware-x86: FORCE
	mkdir -p target/firmware
	case "$(FIRMWARE_RTL8125)" in \
	  "") : > $(FW_RTL).nuevo ;; \
	  *.zst) zstd -q -d -c "$(FIRMWARE_RTL8125)" > $(FW_RTL).nuevo ;; \
	  *.xz) xz -d -c "$(FIRMWARE_RTL8125)" > $(FW_RTL).nuevo ;; \
	  *) cat "$(FIRMWARE_RTL8125)" > $(FW_RTL).nuevo ;; \
	esac
	if cmp -s $(FW_RTL).nuevo $(FW_RTL); then rm -f $(FW_RTL).nuevo; else mv -f $(FW_RTL).nuevo $(FW_RTL); fi
	@echo "Firmware de la RTL8125B: $(or $(FIRMWARE_RTL8125),ninguno) ($$(stat -c %s $(FW_RTL)) bytes)"

usuario-x86-servicios: FORCE
	cd ../usuario && cargo build --release --target x86_64-unknown-none --features $(FUNCIONES_X86) $(RELEVO_PATCH) $(addprefix --bin ,$(SERVICIOS_X86) $(DISCO_PROGRAMAS_PRUEBA_X86))
	cd ../ordenes && cargo build --release --target x86_64-unknown-none

# X5b: IOMMU=intel pone en QEMU la VT-d (`-device intel-iommu`) e IOMMU=amd la AMD-Vi (`-device amd-iommu,dma-remap=on`:
# sin dma-remap, la AMD-Vi de QEMU deja pasar sin traducir el DMA de los aparatos que emula). Con IOMMU, virtio-sound va
# con iommu_platform=on: si no, QEMU le deja hacer DMA saltándose la IOMMU. Los esperados son los de siempre más los de
# la IOMMU (`pruebas/*_iommu.txt`, todos con "~") y la línea de su unidad. En arranque-x86, además, ni un fallo de DMA;
# en prueba-x86 lo mira la prueba de X5b (solo los suyos).
# X11: la AMD-Vi con intremap=on (el remapeo de interrupciones de los aparatos pasados a una máquina; sin él, QEMU tira
# las MSI de un aparato con IV = 1). Con KVM, eso pide kernel-irqchip=split.
# X11b: la VT-d también con intremap=on (el remapeo de todas las interrupciones del anfitrión; ECAP.IR y la bandera
# INTR_REMAP de la DMAR; la de QEMU 11 ya lo trae por omisión con TCG). Con INTREMAP=off, sin él: todo en formato
# compatible, como antes. Con KVM, kernel-irqchip=split
# (QEMU no deja remapear con el APIC de KVM).
# Con RED_X86=vfio, la VT-d de QEMU avisa de cada cambio de sus tablas (caching-mode=on) para que QEMU las siga.
INTREMAP_INTEL := $(coma)intremap=$(if $(filter off,$(INTREMAP)),off,on)$(if $(filter vfio,$(RED_X86)),$(coma)caching-mode=on)
IOMMU_QEMU := $(if $(filter intel,$(IOMMU)),-device intel-iommu$(INTREMAP_INTEL),$(if $(filter amd,$(IOMMU)),-device amd-iommu$(coma)dma-remap=on$(coma)intremap=on))
MAQUINA_IOMMU := $(if $(and $(filter kvm,$(ACCEL)),$(IOMMU)),MAQUINA=kernel-irqchip=split)
# Las líneas que añade el remapeo de la VT-d: su unidad y, en prueba-x86, la prueba de X11 con remapeo.
IR_INTEL := $(and $(filter intel,$(IOMMU)),$(if $(filter off,$(INTREMAP)),,si))
ifneq ($(IOMMU),)
ifeq ($(IOMMU_QEMU),)
$(error IOMMU tiene que ser intel o amd)
endif
endif
VSONIDO_IOMMU := $(if $(IOMMU),$(coma)iommu_platform=on)
esperados_x86 = $(if $(IOMMU),{ cat $(1) $(2); echo "~IOMMU: $(if $(filter intel,$(IOMMU)),VT-d,AMD-Vi) en"; $(if $(IR_INTEL),echo "~remapeo de interrupciones encendido";) } > $(3),cp $(1) $(3))

# X10: ACCEL=kvm corre QEMU con KVM y `-cpu host` (sin él, TCG con `-cpu max`, que emula AMD-V con NPT). Para probar el
# hipervisor de x86 en una CPU de verdad: con KVM anidado (kvm_amd nested=1 o kvm_intel nested=Y), AMD-V en un AMD y
# VT-x en un Intel.
ACCEL_QEMU := $(if $(filter kvm,$(ACCEL)),ACCEL_X86=kvm CPU_X86=host)
ifneq ($(ACCEL),)
ifeq ($(ACCEL_QEMU),)
$(error ACCEL solo puede ser kvm)
endif
endif

# PCID=1 enciende PCID en el kernel de x86 (opción `pcid`); sin él, apagado (decisión del usuario del 2026-11-15).
# X7, paso 4: con la caja Amiga de x86 (`caja/build-x86/caja-amiga`, RelevoJIT de GitHub o RELEVO_DIR), que va dentro.
# X8: MOTOR=jit (por omisión) o MOTOR=interprete elige el motor de RelevoJIT de la caja; lo esperado de la caja dice cuál
# (`MOTOR_ESPERADO`: la línea del nombre del motor, que en los ficheros es la del JIT).
MOTOR ?= jit
MOTOR_ESPERADO := $(if $(filter interprete,$(MOTOR)),sed 's/\] JIT de RelevoJIT/] Intérprete de RelevoJIT/',cat)
x86: usuario-x86-servicios firmware-x86 FORCE
	$(MAKE) -C ../caja x86 MOTOR=$(MOTOR) $(if $(RELEVO_DIR),RELEVO_DIR=$(abspath $(RELEVO_DIR)))
	cargo build $(CARGO_PERFIL) --target x86_64-unknown-none $(if $(PCID),--features pcid)
	$(OBJCOPY_X86) --strip-debug $(ELF_X86) $(NUCLEO_X86)

# Como arranque-virt (U6, S19, U4): q35 con qemu-xhci (el teclado detrás de un concentrador y el ratón), un disco NVMe
# (el mismo de arranque-virt, que se mira después en Linux), virtio-sound (el WAV se analiza después) y la pantalla del
# GOP (la VGA de q35), que se captura por el monitor y se mira con el mismo programa que la de virt (X6).
# X5c: dos discos SATA en el AHCI de q35, en los puertos 0 y 2 (nuevos en cada prueba, `tools/disco_sata.py`); se miran
# después en Linux.
SATA_QEMU := -drive file=$(abspath target/sata0.img),if=none,id=sata0,format=raw,discard=unmap -device ide-hd,drive=sata0,bus=ide.0 -drive file=$(abspath target/sata1.img),if=none,id=sata1,format=raw,discard=unmap -device ide-hd,drive=sata1,bus=ide.2
# X5c: el sonido, por omisión el de un PC (Intel HDA con el códec hda-duplex); SONIDO_X86=virtio, virtio-sound. Sus
# esperados, en pruebas/arranque_x86_sonido_*.txt.
SONIDO_X86 ?= hda
SONIDO_QEMU := $(if $(filter virtio,$(SONIDO_X86)),-device virtio-sound-pci$(coma)audiodev=snd0$(VSONIDO_IOMMU),-device intel-hda -device hda-duplex$(coma)audiodev=snd0)
SONIDO_NOMBRE := $(if $(filter virtio,$(SONIDO_X86)),virtio-sonido,hda)
ifeq ($(filter hda virtio,$(SONIDO_X86)),)
$(error SONIDO_X86 tiene que ser hda o virtio)
endif
# X5c: la tarjeta de red, por omisión virtio-net (con iommu_platform=on si hay IOMMU); RED_X86=e1000e, la 82574L de
# QEMU con el controlador de Intel (`intel-red`). Las dos en el concentrador de RED_HUB, con la misma MAC. Sus esperados,
# en pruebas/arranque_x86_red_*.txt.
# RED_X86=vfio PCI_RED=0000:03:00.0 ACCEL=kvm: una tarjeta de verdad del PC pasada a QEMU con VFIO (la RTL8125 de
# shiva, con `rtl-red`; sin su ROM, que OVMF no la toque), en vez de la emulada y sin el concentrador: al otro lado
# del cable, lo que diga
# `tools/vfio_red.sh` (DHCP y eco en Linux). Antes, la tarjeta en vfio-pci (el guion imprime los comandos). Sus
# esperados, en pruebas/arranque_x86_red_vfio.txt; con MAC_RED=xx:xx:..., también esa MAC (la que tenía en Linux).
RED_X86 ?= virtio
RED_TARJETA_X86 := $(if $(filter vfio,$(RED_X86)),-device vfio-pci$(coma)host=$(PCI_RED)$(coma)rombar=0,$(if $(filter e1000e,$(RED_X86)),-device e1000e$(coma)netdev=red4$(coma)mac=$(RED_MAC)$(coma)romfile=,-device virtio-net-pci$(coma)netdev=red4$(coma)disable-legacy=on$(coma)mac=$(RED_MAC)$(coma)romfile=$(VSONIDO_IOMMU)))
RED_NOMBRE := $(if $(filter vfio,$(RED_X86)),rtl-red,$(if $(filter e1000e,$(RED_X86)),intel-red,virtio-red))
ifeq ($(filter virtio e1000e vfio,$(RED_X86)),)
$(error RED_X86 tiene que ser virtio, e1000e o vfio)
endif
ifeq ($(RED_X86),vfio)
ifeq ($(PCI_RED),)
$(error RED_X86=vfio necesita PCI_RED (por ejemplo PCI_RED=0000:03:00.0; ver tools/vfio_red.sh))
endif
ifneq ($(ACCEL),kvm)
$(error RED_X86=vfio necesita ACCEL=kvm)
endif
ifeq ($(IOMMU),amd)
$(error RED_X86=vfio va sin IOMMU en QEMU o con IOMMU=intel (la AMD-Vi de QEMU no lleva aparatos de VFIO))
endif
endif
RED_HUB_X86 := $(if $(filter vfio,$(RED_X86)),,$(RED_HUB))
# Los esperados de la tarjeta pasada: la línea del firmware depende de si va dentro del kernel; la MAC, si se da.
ESPERADOS_VFIO = { sed -n '1,/^\# --- firmware/p' pruebas/arranque_x86_red_vfio.txt; \
	if [ -s $(FW_RTL) ]; then echo "~│   [rtl-red] firmware aplicado: "; else echo "~│   [rtl-red] sin firmware del PHY"; fi; \
	$(if $(MAC_RED),echo "~MAC $(MAC_RED);";) \
	sed -n '/^\# --- firmware/,$$p' pruebas/arranque_x86_red_vfio.txt; } >> target/esperados_arranque_x86.txt
# X7, paso 4: el escritorio de `arranque-virt` (`inicio_x86.rs`) con su guion largo (TECLAS=1, con el reinicio del
# final) y a 800x600, como la ramfb de virt (el guion pulsa en sitios de esa pantalla). El disco, como el de
# arranque-virt pero con los programas de x86 (DASHOS_ARQ=x86_64) y 256 KB más grande, para que quede el mismo sitio
# libre (el código de x86 ocupa más en DashFS; ver `tools/disco_archivos.py`), nuevo en cada prueba y mirado después en
# Linux.
#
# Lo esperado de x86 y de ARM es el mismo fichero, para que vayan a la par: `pruebas/arranque_virt.txt`, sin sus bloques
# solo de ARM (entre "#[arm" y "#]") y con sus líneas solo de x86 (las que empiezan por "#x86 ", que ARM ignora como
# comentario), detrás de `pruebas/arranque_x86.txt` (el arranque de x86 y lo que solo hay en un PC: el AHCI y sus
# discos) y delante de lo de la tarjeta de sonido y la de red que se elijan. Quien añada algo a arranque_virt.txt lo
# añade a los dos; si no sale en x86, va en un bloque de ARM, con el porqué.
DISCO_NVME_X86 := target/nvme-x86-prueba.img
$(DISCO_NVME_X86): catalogos usuario-x86-servicios FORCE
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar --bin dashfs-volume
	cd ../archivos/herramientas && cargo build --release
	DASHOS_ARQ=x86_64 DASHOS_DISCO_MAS_KB=256 python3 ../tools/disco_archivos.py $@ $(DASHFS_AR)
arranque-x86: x86 $(DISCO_NVME_X86)
	cd ../uefi && DASHOS_PANTALLA=800x600 DASHOS_NUCLEO=$(abspath $(NUCLEO_X86)) cargo build --release --target x86_64-unknown-uefi
	rm -f target/sonido-x86.wav target/captura-x86.ppm target/u6-x86.ana.ppm target/u6-x86.carla.ppm
	python3 ../tools/disco_sata.py crear target/sata0.img 0
	python3 ../tools/disco_sata.py crear target/sata1.img 1
	{ cat pruebas/arranque_x86.txt; awk '/^#\[arm/ { fuera = 1; next } /^#\]/ { fuera = 0; next } fuera { next } { sub(/^#x86 /, ""); print }' pruebas/arranque_virt.txt | $(MOTOR_ESPERADO); } > target/arranque_x86_comun.txt
	$(call esperados_x86,target/arranque_x86_comun.txt,pruebas/arranque_x86_iommu.txt,target/esperados_arranque_x86.txt)
	cat pruebas/arranque_x86_sonido_$(SONIDO_X86).txt >> target/esperados_arranque_x86.txt
	$(if $(IOMMU),echo "~$(SONIDO_NOMBRE): DMA por la IOMMU$(coma) en su dominio" >> target/esperados_arranque_x86.txt)
	$(if $(IOMMU),echo "~$(RED_NOMBRE): DMA por la IOMMU$(coma) en su dominio" >> target/esperados_arranque_x86.txt)
	$(if $(filter vfio,$(RED_X86)),$(ESPERADOS_VFIO),cat pruebas/arranque_x86_red_$(RED_X86).txt pruebas/arranque_x86_red.txt >> target/esperados_arranque_x86.txt)
	mkdir -p $(RED)
	rm -f $(RED)/qemu.sock $(RED)/pc.sock $(RED)/qemu2.sock $(RED)/vecino.sock
	$(if $(filter vfio,$(RED_X86)),true & BASURA=$$!,python3 -I ../tools/red_basura.py $(RED) $(RED_MAC) 300 & BASURA=$$!; $(RED_ANTES)); \
	$(ACCEL_QEMU) $(MAQUINA_IOMMU) GUARDAR=$(abspath target/salida_x86.txt) CAPTURA=$(abspath target/captura-x86.ppm) CAPTURA_U6=$(abspath target/u6-x86) REINICIO=$(abspath pruebas/arranque_virt_reinicio.txt) ARQ=x86_64 TECLAS=1 QEMU_EXTRA="$(IOMMU_QEMU) -device qemu-xhci,id=xhci -device usb-hub,bus=xhci.0,port=1 -device usb-kbd,bus=xhci.0,port=1.1 -device usb-mouse,bus=xhci.0,port=2 -drive file=$(abspath $(DISCO_NVME_X86)),if=none,id=nv,format=raw,discard=unmap -device nvme,serial=dashos,drive=nv $(SATA_QEMU) -audiodev wav,id=snd0,path=$(abspath target/sonido-x86.wav),out.frequency=48000 $(SONIDO_QEMU) $(RED_HUB_X86) $(RED_TARJETA_X86)" ../tools/prueba_uefi.sh ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi target/esperados_arranque_x86.txt $(or $(TIEMPO_X86),900); \
	R=$$?; $(RED_DESPUES); exit $$R
	@if grep -q "fallo de DMA\|interrupción bloqueada" target/salida_x86.txt; then echo "MAL: fallos de DMA o interrupciones bloqueadas en el arranque"; grep "fallo de DMA\|interrupción bloqueada" target/salida_x86.txt | head; exit 1; fi
	DASHOS_ARQ=x86_64 DASHOS_SALIDA=$(abspath target/salida_x86.txt) python3 ../tools/disco_archivos.py --comprobar $(DISCO_NVME_X86) $(VER)
	python3 ../tools/disco_sata.py comprobar target/sata0.img 0
	python3 ../tools/disco_sata.py comprobar target/sata1.img 1
	python3 ../tools/captura_ventanas.py target/captura-x86.ppm
	python3 ../tools/sonido_wav.py target/sonido-x86.wav
	python3 ../tools/captura_cambio.py target/u6-x86.ana.ppm target/u6-x86.carla.ppm

# X5c: un PC con un solo disco SATA y sin NVMe. El AHCI sirve entonces el canal "disco" y todo va por él: `disco` (la
# prueba entera del NVMe, con el plazo vencido y el anillo) y `archivos`. El disco es el de arranque-virt
# (`target/sata-disco.img`, nuevo en cada prueba) y se mira después en Linux. Sonido por HDA, sin red.
sata-x86: x86 FORCE
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(NUCLEO_X86)) cargo build --release --target x86_64-unknown-uefi
	python3 ../tools/disco_archivos.py target/sata-disco.img $(DASHFS_AR)
	$(ACCEL_QEMU) $(MAQUINA_IOMMU) GUARDAR=$(abspath target/salida_sata_x86.txt) ARQ=x86_64 QEMU_EXTRA="$(IOMMU_QEMU) -drive file=$(abspath target/sata-disco.img),if=none,id=s0,format=raw,discard=unmap -device ide-hd,drive=s0,bus=ide.1 -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0" ../tools/prueba_uefi.sh ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi pruebas/sata_x86.txt 180
	python3 ../tools/disco_archivos.py --comprobar-basico target/sata-disco.img $(VER)

# Los programas de usuario de x86-64 que no tocan aparatos (S30, X4), y el PIE de C29 (con las binutils del sistema).
PIE_X86 := ../usuario/target/pie_x86
BINES_X86 := hola fpu_a fpu_b malo tenis repartos tls trampas hijo padre llaves grupos lanzador tiempo muchos hilos maquina maquina_pci relevo fallos

usuario-x86: FORCE
	cd ../usuario && cargo build --release --target x86_64-unknown-none --features $(FUNCIONES_X86) $(RELEVO_PATCH) $(addprefix --bin ,$(BINES_X86))
	as --64 -o $(PIE_X86).o ../usuario/asm/pie_x86.s
	ld -pie --no-dynamic-linker -z noexecstack -z stack-size=32768 -o $(PIE_X86) $(PIE_X86).o

# Las pruebas del arranque en x86-64 (S30, X2 a X4): memoria, montón, mapa, excepciones y defensas; núcleos, reloj e
# interrupciones; procesos en el anillo 3 con los programas de usuario.
NUCLEO_X86_PRUEBAS := target/kernel-x86-pruebas.elf

# X11: el aparato `edu` de QEMU, para pasarlo a una máquina (con DMA de 32 bits: la prueba le hace escribir en
# 0xFEE0_0000).
EDU_X11 := -device edu,dma_mask=0xffffffff
prueba-x86: usuario-x86 firmware-x86 FORCE
	cargo build $(CARGO_PERFIL) --target x86_64-unknown-none --features pruebas$(if $(PCID),$(coma)pcid)
	$(OBJCOPY_X86) --strip-debug $(ELF_X86) $(NUCLEO_X86_PRUEBAS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(NUCLEO_X86_PRUEBAS)) cargo build --release --target x86_64-unknown-uefi
	$(call esperados_x86,pruebas/esperado_x86.txt,pruebas/esperado_x86_iommu.txt,target/esperados_x86.txt)
	$(if $(filter amd,$(IOMMU)),echo "~X11: con el remapeo de AMD-Vi$(coma) su MSI llega y otra no bien" >> target/esperados_x86.txt)
	$(if $(IR_INTEL),echo "~X11: con el remapeo de VT-d$(coma) su MSI llega y otra no bien" >> target/esperados_x86.txt)
	$(ACCEL_QEMU) $(MAQUINA_IOMMU) GUARDAR=$(abspath target/salida_x86_pruebas.txt) ARQ=x86_64 QEMU_EXTRA="$(EDU_X11) $(if $(IOMMU),$(IOMMU_QEMU) -device nvme$(coma)serial=prueba)" ../tools/prueba_uefi.sh ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi target/esperados_x86.txt 150

# X12 (G5 de docs/diseño/S38-gpu-escritorio.md): el dominio de gráficos de x86. La imagen `graficos` (la consola y el
# servicio `dominio-graficos`) en QEMU q35 con KVM y la IOMMU (IOMMU=intel, por omisión, o amd; con remapeo de
# interrupciones y kernel-irqchip=split), una virtio-gpu con virgl (`virtio-gpu-gl-pci`, iommu_platform=on) que se pasa
# al dominio y la VGA de q35 para DashOS; `-display egl-headless`. El dominio (un Linux con DRM y Mesa, fuera de git:
# `tools/dominio_graficos.py preparar`, en DOMINIO_GRAFICOS) pinta por KMS; `tools/prueba_graficos_x86.py` mira la
# salida, dos capturas de la GPU por VNC (no negras y distintas) y escribe lo medido. Necesita /dev/kvm con KVM anidado.
DOMINIO_GRAFICOS ?= $(HOME)/recuperar/dashos-97/dominio-graficos
NUCLEO_X86_GRAFICOS := target/kernel-x86-graficos.elf
graficos-x86: FORCE
	@test -f "$(DOMINIO_GRAFICOS)/bzImage" -a -f "$(DOMINIO_GRAFICOS)/initramfs.cpio" || { echo "Falta el dominio de gráficos en $(DOMINIO_GRAFICOS) (no va en git): python3 tools/dominio_graficos.py preparar"; exit 1; }
	cd ../usuario && cargo build --release --target x86_64-unknown-none --features $(FUNCIONES_X86),caja $(RELEVO_PATCH) --bin inicio --bin consola --bin dominio-graficos --bin graficos-cliente
	cargo build $(CARGO_PERFIL) --target x86_64-unknown-none --features graficos$(if $(PCID),$(coma)pcid)
	$(OBJCOPY_X86) --strip-debug $(ELF_X86) $(NUCLEO_X86_GRAFICOS)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(NUCLEO_X86_GRAFICOS)) cargo build --release --target x86_64-unknown-uefi
	python3 ../tools/prueba_graficos_x86.py ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi $(DOMINIO_GRAFICOS) --iommu $(or $(IOMMU),intel) --salida target/graficos

# X7, X8: la caja Amiga de x86-64 (`caja/build-x86/caja-amiga`, con el JIT de RelevoJIT o, con MOTOR=interprete, su
# intérprete) con sus programas de
# prueba: la imagen `caja` (la consola, el compositor, el disco NVMe, el servicio de archivos y la caja) en QEMU q35, y
# lo que dice la caja contra pruebas/caja_x86.txt. RelevoJIT, de GitHub (rama motor-caja; RELEVO_DIR=<ruta> para un clon local).
NUCLEO_X86_CAJA := target/kernel-x86-caja.elf
caja-x86: firmware-x86 $(DISCO_NVME) FORCE
	$(MAKE) -C ../caja x86 MOTOR=$(MOTOR) $(if $(RELEVO_DIR),RELEVO_DIR=$(abspath $(RELEVO_DIR)))
	cd ../usuario && cargo build --release --target x86_64-unknown-none --features $(FUNCIONES_X86) $(RELEVO_PATCH) --bin inicio --bin consola --bin compositor --bin nvme --bin archivos
	cargo build $(CARGO_PERFIL) --target x86_64-unknown-none --features caja$(if $(PCID),$(coma)pcid)
	$(OBJCOPY_X86) --strip-debug $(ELF_X86) $(NUCLEO_X86_CAJA)
	cd ../uefi && DASHOS_NUCLEO=$(abspath $(NUCLEO_X86_CAJA)) cargo build --release --target x86_64-unknown-uefi
	$(MOTOR_ESPERADO) < pruebas/caja_x86.txt > target/esperados_caja_x86.txt
	$(ACCEL_QEMU) GUARDAR=$(abspath target/salida_caja_x86.txt) ARQ=x86_64 QEMU_EXTRA="-drive file=$(abspath $(DISCO_NVME)),if=none,id=nv,format=raw -device nvme,serial=dashos,drive=nv" ../tools/prueba_uefi.sh ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi target/esperados_caja_x86.txt $(or $(TIEMPO_X86),300)

# La imagen de usar en x86-64 (`make x86-uso`): el escritorio de `virt-uso` (`kernel/src/inicio_x86_uso.rs`) en QEMU q35
# con OVMF, en una ventana, con KVM si hay /dev/kvm (SIN_KVM=1 lo quita), una tableta USB y el teclado, el disco de
# serie (compilado para x86: DASHOS_ARQ=x86_64), la red de usuario de QEMU, el sonido y la pantalla por virtio-gpu
# (x86-gpu). Lo arranca `tools/qemu_x86_a_mano.sh` (PANTALLA=ANCHOxALTO, SONIDO_X86=hda|virtio, GPU_X86=virtio|vga,
# VENTANA=gtk|sdl|none). No es una prueba: nada la comprueba. Sin la caja Linux ni la caja Amiga (aún no van en x86).
SERVICIOS_X86_USO := inicio consola compositor xhci nvme ahci disco-vivo instalador instalar archivos comprobador eco sesiones red virtio-red fondo centro guardian imagen sonido avisos barra virtio-sonido hda selector panel explorador workbench hora virtio-gpu dock mezclador
# (`paq`, el servicio de paquetes, sale de `ordenes/`, que se compila entero aquí debajo.)
# Los programas que van en el disco (`tools/disco_archivos.py`), también de x86.
DISCO_PROGRAMAS_X86 := hola pide cambio ram avisa guarda espera blanker nocapslock
FUNCIONES_X86_USO := archivos,ventanas,sonido,red,imagenes,ordenes,caja
NUCLEO_X86_USO := target/kernel-x86-uso.elf
DISCO_X86 := target/nvme-x86.img
ACCEL_USO := $(if $(SIN_KVM),,$(if $(wildcard /dev/kvm),kvm))
usuario-x86-uso: FORCE
	cd ../usuario && cargo build --release --target x86_64-unknown-none --features $(FUNCIONES_X86_USO) $(addprefix --bin ,$(SERVICIOS_X86_USO) $(DISCO_PROGRAMAS_X86))
	cd ../ordenes && cargo build --release --target x86_64-unknown-none
x86-uso-imagen: usuario-x86-uso FORCE
	cargo build $(CARGO_PERFIL) --target x86_64-unknown-none --features uso$(if $(PCID),$(coma)pcid)
	$(OBJCOPY_X86) --strip-debug $(ELF_X86) $(NUCLEO_X86_USO)
$(DISCO_X86): catalogos usuario-x86-uso FORCE
	cd ../archivos/dashfs && cargo build --release --bin dashfs-ar --bin dashfs-volume
	DASHOS_ARQ=x86_64 python3 ../tools/disco_archivos.py $@ $(DASHFS_AR)
x86-uso: x86-uso-imagen $(DISCO_X86)
	ACCEL=$(ACCEL_USO) ../tools/qemu_x86_a_mano.sh

# S32: el pendrive en vivo de x86 (`target/pendrive-x86.img`, para grabarlo en un USB con dd): GPT con una ESP que
# lleva el cargador (con la imagen de usar dentro) como EFI/BOOT/BOOTX64.EFI y el sistema (el DashFS del disco de
# serie) como EFI/DashOS/VIVO.IMG, que el cargador lee entero a la memoria. DashOS arranca en vivo, sin tocar los
# discos del PC, y desde él se instala (`docs/bitacoras/instalador.md`). SIN_MANOS=<plan> pone ese plan en
# EFI/DashOS/SINMANOS.TXT (las pruebas del banco). La de la Pi 5, pendiente (S32).
PENDRIVE_X86 := target/pendrive-x86.img
# El dominio Linux de x86 con ntfsresize (`tools/dominio_x86.py preparar`, fuera de git): si está, va en el pendrive
# para encoger Windows.
DOMINIO_X86 ?= $(HOME)/recuperar/geosian-7d/dominio-x86
DOMINIO_X86_FICHEROS := $(and $(wildcard $(DOMINIO_X86)/bzImage),$(wildcard $(DOMINIO_X86)/initramfs.cpio),--dominio $(DOMINIO_X86)/bzImage $(DOMINIO_X86)/initramfs.cpio)
pendrive-x86: x86-uso-imagen $(DISCO_X86)
	cd ../uefi && DASHOS_PANTALLA=$(or $(PANTALLA),1280x800) DASHOS_NUCLEO=$(abspath $(NUCLEO_X86_USO)) cargo build --release --target x86_64-unknown-uefi
	python3 ../tools/pendrive_x86.py $(PENDRIVE_X86) ../uefi/target/x86_64-unknown-uefi/release/dashos-uefi.efi $(DISCO_X86) $(if $(SIN_MANOS),--sin-manos $(SIN_MANOS)) $(DOMINIO_X86_FICHEROS)

# S32: el pendrive en vivo en QEMU (q35 con OVMF, KVM si hay), como USB, con un disco NVMe vacío de 64 GB al lado para
# instalar (`target/vivo-destino.img`, nuevo cada vez). Para usarlo a mano; las pruebas, `prueba-instalar-x86`.
vivo-x86: pendrive-x86
	rm -f target/vivo-destino.img && truncate -s 64G target/vivo-destino.img
	cp /usr/share/edk2/x64/OVMF_VARS.4m.fd target/vivo-vars.fd
	qemu-system-x86_64 -name "DashOS en vivo" -M q35 $(if $(ACCEL_USO),-accel kvm -cpu host,-cpu max) -smp 4 -m 2G \
	    -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd -drive if=pflash,format=raw,file=target/vivo-vars.fd \
	    -device qemu-xhci,id=xhci -device usb-kbd,bus=xhci.0 -device usb-tablet,bus=xhci.0 \
	    -drive file=$(PENDRIVE_X86),format=raw,if=none,id=usb0,readonly=on -device usb-storage,bus=xhci.0,drive=usb0,bootindex=1 \
	    -drive file=target/vivo-destino.img,format=raw,if=none,id=nv -device nvme,serial=destino,drive=nv \
	    -serial file:target/vivo-serie.txt -display gtk,zoom-to-fit=on -audiodev none,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0

# S32: la prueba del instalador en el banco de AmiCachy (`tools/banco_instalador.py`, los discos base no se tocan):
# disco vacío, Windows con hueco, Debian con hueco, el pendrive como destino y la GPT rota, y un corte de luz.
prueba-instalar-x86: pendrive-x86
	python3 ../tools/prueba_instalador.py

clean:
	cargo clean

FORCE:
